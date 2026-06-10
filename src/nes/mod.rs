use imgui::Key::P;
use std::{
    cell::RefCell,
    env,
    error::Error,
    ffi::{OsStr, OsString},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    rc::Rc,
    thread,
    time::Duration,
};
use thiserror::Error;

use crate::{
    AudioProducer,
    main_loop::EMU_SAMPLE_RATE,
    nes::{
        apu::Apu,
        bus_cpu::{BusCpu, BusCpuError, IOResult},
        controller::Controller,
        cpu::{Cpu},
        game_rom::ROMErr,
        mappers::Mapper,
        ppu::{BusPpu,Ppu, PpuRegister},
    },
};

mod apu;
pub mod bus_cpu;
pub mod controller;
pub mod cpu;
mod game_rom;
pub mod instruction_set;
mod ppu;

mod mappers;

pub struct Nes {
    pub cpu: Cpu,
    pub ppu: Ppu,
    pub bus_cpu: BusCpu,
    pub bus_ppu: BusPpu,
    pub apu: Apu,
    pub controller: Controller,
    pub mapper: Box<dyn Mapper>,
    mapper_id: u8,
    oam_dma_buffer: [u8; 256],
    ppu_cpu_clock_frequency: u8,
    save_file_path: OsString,

    #[cfg(feature = "log")]
    log: bool,
}

#[derive(Error, Debug)]
pub enum NesError {
    #[error("Could not read/write to Cpu bus: {0}")]
    BusCpuError(#[from] BusCpuError),
    #[error("{0}")]
    RomError(#[from] ROMErr),
    #[error("Error ocurred during event: {0}")]
    EventError(#[from] Box<dyn Error>),
    #[error("Couldn't respond to '{0:?}' event")]
    UnhandledEvent(NesEvent),
    #[error("Error ocurred on IO operaation: {0}")]
    IOError(#[from] std::io::Error),
    #[error("'{0:?}' is not a valid path")]
    InvalidPath(OsString),
    #[error("Couldnt serialize/capture the emulator: {0}")]
    PostCardError(#[from] postcard::Error),
    #[error("File loaded didnt match the requiered length: {0}")]
    LengthMismatch(String),
}

type Result<T> = std::result::Result<T, NesError>;

impl Nes {
    pub fn new(game_file_path: &OsString, audio_producer: AudioProducer) -> Result<Self> {
        let (mapper_id, mapper) = game_rom::load_rom(game_file_path)?;
        let cpu = Cpu::new();
        let bus_cpu = BusCpu::new();
        let ppu = Ppu::new();
        let apu = Apu::new(audio_producer, EMU_SAMPLE_RATE);
        let controller = Controller::default();
        let bus_ppu = BusPpu::new();

        Ok(Nes {
            bus_cpu,
            bus_ppu,
            ppu,
            cpu,
            apu,
            controller,
            mapper,
            mapper_id,
            oam_dma_buffer: [0u8; 256],
            ppu_cpu_clock_frequency: 0,
            save_file_path: Self::default_save_file_name(game_file_path)?,
            #[cfg(feature = "log")]
            log: false,
        })
    }

    pub fn swap_rom(&mut self, game_file_path: &OsString) -> Result<()> {
        // Save progress.
        self.try_store_save_file(None)?;

        let (id, new_mapper) = game_rom::load_rom(game_file_path)?;
        self.mapper = new_mapper;
        self.mapper_id = id;

        // Try load new game's data.
        let new_save_file_path = Self::default_save_file_name(game_file_path)?;
        self.try_load_save_file(Some(&new_save_file_path))?;
        Ok(())
    }

    #[cfg(feature = "log")]
    pub fn toggle_log(&mut self) {
        self.log ^= true;
    }

    #[cfg(feature = "log")]
    pub fn log_ppu_state(&self) {
        log::info!("PPU SC: {}, CYC: {}", self.ppu.scanline, self.ppu.cycle);
    }

    pub fn reset(&mut self) -> Result<()> {
        let instance = std::ptr::from_mut(self);
        self.cpu.reset(instance);
        self.ppu.reset();
        self.apu.reset();
        Ok(())
    }

    #[inline(always)]
    pub fn read(&mut self, addr: u16) -> Result<u8> {
        // let _span = tracy_client::span!("cpu read");
        let r = self.bus_cpu.read(addr, &mut self.mapper)?;
        let d = self.resolve_cpu_read_result(r)?;
        // log::info!("READ ${addr:04X} = {d:02X}");
        Ok(d)
    }

    #[inline(always)]
    fn resolve_cpu_read_result(&mut self, r: IOResult) -> Result<u8> {
        match r {
            IOResult::OkRead(data) => Ok(data),
            IOResult::TriggerEvent(e, debug_backup) => {
                let d = self.trigger_event(e)?.expect("Read event returned no data");
                unsafe {
                    *debug_backup = d;
                }
                Ok(d)
            }
            _ => unreachable!(),
        }
    }

    #[inline(always)]
    fn resolve_cpu_write_result(&mut self, r: IOResult, data: u8) -> Result<()> {
        match r {
            IOResult::OkWrite => Ok(()),
            IOResult::TriggerEvent(e, debug_backup) => {
                self.trigger_event(e)?;
                unsafe {
                    *debug_backup = data;
                }
                Ok(())
            }
            _ => unreachable!(),
        }
    }

    #[inline(always)]
    pub fn debug_read(&mut self, addr: u16) -> Result<u8> {
        Ok(self.bus_cpu.debug_read(addr, &mut self.mapper)?)
    }

    pub fn debug_write(&mut self, addr: u16, data: u8) {
        self.bus_cpu.debug_write(addr, data, &mut self.mapper)
    }

    #[inline(always)]
    pub fn write(&mut self, addr: u16, data: u8) -> Result<()> {
        // let _span = tracy_client::span!("cpu write");
        let r = self.bus_cpu.write(addr, data, &mut self.mapper)?;
        self.resolve_cpu_write_result(r, data)
    }

    #[inline(always)]
    pub fn read16(&mut self, addr: u16) -> Result<u16> {
        // let _span = tracy_client::span!("cpu read16");
        let (rl, rh) = self.bus_cpu.read16(addr, &mut self.mapper)?;
        let lo = self.resolve_cpu_read_result(rl)?;
        let hi = self.resolve_cpu_read_result(rh)?;
        let d = (hi as u16) << 8 | lo as u16;
        // log::info!("READ ${addr:04X} = {d:04X}");
        Ok(d)
    }

    #[inline(always)]
    pub fn debug_read16(&mut self, addr: u16) -> Result<u16> {
        Ok(self.bus_cpu.debug_read16(addr, &mut self.mapper)?)
    }

    #[inline(always)]
    pub fn ppu_read(&mut self, addr: u16) -> u8 {
        // let _span = tracy_client::span!("ppu read");
        self.bus_ppu.read(addr, &mut self.mapper)
    }

    #[inline(always)]
    pub fn ppu_debug_read(&mut self, addr: u16) -> u8 {
        // let _span = tracy_client::span!("ppu read");
        self.bus_ppu.debug_read(addr, &mut self.mapper)
    }

    #[inline(always)]
    pub fn ppu_write(&mut self, addr: u16, data: u8) {
        // let _span = tracy_client::span!("ppu write");
        self.bus_ppu.write(addr, data, &mut self.mapper);
    }

    pub fn clock(&mut self, _: Option<&mut String>) -> Result<()> {
        // let _span = tracy_client::span!("Clock Nes");

        let instance = std::ptr::from_mut(self);

        self.ppu.clock(instance, &mut self.mapper);
        self.ppu.clock(instance, &mut self.mapper);
        self.ppu.clock(instance, &mut self.mapper);
        self.ppu_cpu_clock_frequency += 1;

        #[cfg(feature = "log")]
        {
            if self.log && self.mapper.is_irq_pending() {
                log::info!(
                    "IRQ Is pending: SC: {}, CYC: {}",
                    self.ppu.scanline,
                    self.ppu.cycle
                );
            }
        }

        let nmi_signal = self.ppu.nmi_signal;
        if nmi_signal {
            self.cpu.nmi(instance);
            self.ppu.nmi_signal = false;
        }

        self.cpu.clock(None, instance, &mut self.mapper);
        self.apu.clock(instance);
        if self.apu.irq_pending() {
            self.cpu.irq(instance);
        }

        Ok(())
    }

    pub fn step_frame(&mut self, break_point: Option<u16>) -> Result<bool> {
        // let _span = tracy_client::span!("Step frame");
        loop {
            self.clock(None)?;
            if let Some(a) = break_point
                && a == self.cpu.get_pc()
            {
                return Ok(true);
            }
            if self.ppu.frame_complete {
                break;
            }
        }
        Ok(false)
    }

    // pub fn finish_nestest_fmt(&self, s: &mut String) {
    //     s.push_str("PPU:");
    //     let (mut sc, ppu_cyc) = self.ppu.get_cycle_and_sc();
    //     sc += 1;
    //     match sc {
    //         0..=9 => {
    //             s.push_str("  ");
    //         }
    //         10..=99 | -1 => {
    //             s.push(' ');
    //         }
    //         _ => {}
    //     }
    //     write!(s, "{},", sc);
    //     match ppu_cyc {
    //         0..=9 => {
    //             s.push_str("  ");
    //         }
    //         10..=99 => {
    //             s.push(' ');
    //         }
    //         _ => {}
    //     }
    //     write!(s, "{} ", ppu_cyc);
    //     write!(s, "CYC:{}", self.cpu.state.clock_counter - 1);
    // }
}

// Communication between components
#[derive(Debug)]
pub enum NesEvent {
    PpuRegWrittenTo(PpuRegister, u8),
    PpuRegReadFrom(PpuRegister),
    OAMDMATransfer(u8),
    ControllerReadFrom(u16),
    ControllerWrittenTo(u8),
    ApuWrittenTo(u16, u8),
    ApuReadFrom(u16),
    InfoReq(MapperInfoReq),
}

#[derive(Debug)]
pub enum MapperInfoReq {
    PpuAddressBus,
}

// Event logic
impl Nes {
    #[allow(unreachable_patterns)]
    pub fn trigger_event(
        &mut self,
        e: NesEvent,
    ) -> std::result::Result<Option<u8>, Box<dyn Error>> {
        let instance = std::ptr::from_mut(self);
        match e {
            NesEvent::PpuRegWrittenTo(r, data) => {
                self.ppu.event_cpu_has_written(r, data, instance)?;
                Ok(None)
            }
            NesEvent::ApuWrittenTo(addr, data) => {
                self.apu.event_cpu_has_written(addr, data);
                Ok(None)
            }
            NesEvent::ControllerWrittenTo(data) => {
                self.controller.event_cpu_has_written(data);
                Ok(None)
            }
            NesEvent::OAMDMATransfer(addr_start) => {
                let page = (addr_start as u16) << 8;
                for i in 0..256 {
                    self.oam_dma_buffer[i] = self.read(page + i as u16)?;
                }
                self.ppu
                    .event_oam_dma_transfer(self.oam_dma_buffer.as_slice())?;
                Ok(None)
            }
            NesEvent::InfoReq(i) => {
                match i {
                    MapperInfoReq::PpuAddressBus => {
                        // log::info!("Info request: \n\tPrev: {:014b}\n\tCurr: {:014b}", self.bus_ppu.prev_address_bus, self.bus_ppu.address_bus);
                        let response = self.bus_ppu.address_bus as u64;
                        self.mapper.info_request_response(i, Some(response));
                    }
                }
                Ok(None)
            }
            NesEvent::PpuRegReadFrom(r) => Ok(Some(self.ppu.event_cpu_has_read(r, instance)?)),
            NesEvent::ControllerReadFrom(addr) => {
                Ok(Some(self.controller.event_cpu_has_read(addr)))
            }
            NesEvent::ApuReadFrom(addr) => Ok(Some(self.apu.event_cpu_has_read(addr))),
            _ => Err(Box::new(NesError::UnhandledEvent(e))),
        }
    }
}




// Saving/Loading
impl Nes {
    #[inline]
    pub fn default_save_file_name(game_file_path: &OsString) -> Result<OsString> {
        // Ex: C:\\Users\\xxx\\smb.rom -> {CWD}\\savefiles\\smb.rom.sram
        let file_name = Path::new(game_file_path)
            .file_name()
            .ok_or_else(|| NesError::InvalidPath(game_file_path.clone()))?;

        let mut save_path = env::current_dir()?;
        save_path.push("savefiles");

        if !save_path.exists() {
            std::fs::create_dir_all(&save_path)?;
        }

        save_path.push(file_name);
        let mut save_path = save_path.into_os_string();
        save_path.push(".sram");
        Ok(save_path)
    }

    pub fn try_load_save_file(&mut self, path: Option<&OsString>) -> Result<()> {
        let path = match path {
            Some(s) => s,
            None => &self.save_file_path,
        };

        if !self.mapper.is_sram_battery_backed() {
            log::info!("Game does not support saving.");
            return Ok(());
        }
        let mut save_file = match std::fs::File::open(path) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("Couldn't load save file: {e}");
                return Ok(());
            }
        };

        let mut file_contents: Vec<u8> = Vec::new();
        save_file.read_to_end(&mut file_contents)?;
        self.mapper.load_sram(&file_contents);

        // Change the save file path
        log::info!("Loaded save file: {:?}", path);
        self.save_file_path = path.clone();
        Ok(())
    }

    pub fn try_store_save_file(&mut self, path: Option<&OsString>) -> Result<()> {
        let path = match path {
            Some(s) => s,
            None => &self.save_file_path,
        };

        if !self.mapper.is_sram_battery_backed() {
            log::warn!("Cannot save since game does not support saving.");
            println!(
                "   >   You can use 'File > Capture emulator state' to save your progress although saving this way is considerably heavier."
            );
            return Ok(());
        }

        let sram = match self.mapper.save_sram() {
            Some(s) => s,
            None => return Ok(()),
        };

        let mut save_file = std::fs::File::create(path)?;
        save_file.write(sram)?;

        log::info!("Saved progress to {:?}", path);
        Ok(())
    }

    pub fn take_snapshot(&self, path: &OsString) -> Result<()> {
        let f = fs::File::create(path)?;
        let mut encoder = zstd::Encoder::new(f, 22)?;
        let length = size_of::<u8>()
            + size_of::<Cpu>()
            + self.bus_cpu.chips.ram.len()
            + self.bus_ppu.get_nametables().len()
            + self.bus_ppu.get_palette_data().len()
            + self.mapper.get_snapshot_size();

        let mut snapshot = Vec::with_capacity(length);
        unsafe {
            snapshot.set_len(length);
        }
        let mut ptr = 0;
        snapshot[0] = self.mapper_id;
        ptr += size_of::<u8>();
        postcard::to_slice(&self.cpu, &mut snapshot[ptr..(ptr + size_of::<Cpu>())])?;
        ptr += size_of::<Cpu>();
        snapshot[ptr..(ptr + self.bus_cpu.chips.ram.len())]
            .copy_from_slice(&self.bus_cpu.chips.ram);

        ptr += self.bus_cpu.chips.ram.len();
        snapshot[ptr..(ptr + self.bus_ppu.get_nametables().len())]
            .copy_from_slice(self.bus_ppu.get_nametables());
        ptr += self.bus_ppu.get_nametables().len();
        snapshot[ptr..(ptr + self.bus_ppu.get_palette_data().len())]
            .copy_from_slice(self.bus_ppu.get_palette_data());
        ptr += self.bus_ppu.get_palette_data().len();

        self.mapper
            .take_snapshot(&mut snapshot[ptr..(ptr + self.mapper.get_snapshot_size())])?;
        encoder.write_all(&snapshot)?;
        encoder.finish()?;
        Ok(())
    }

    pub fn load_snapshot(&mut self, path: &OsString) -> Result<()> {
        let f = fs::File::open(path)?;
        let mut decoder = zstd::Decoder::new(f)?;
        let length = size_of::<u8>()
            + size_of::<Cpu>()
            + self.bus_cpu.chips.ram.len()
            + self.bus_ppu.get_nametables().len()
            + self.bus_ppu.get_palette_data().len()
            + self.mapper.get_snapshot_size();

        let mut snap = Vec::new();
        decoder.read_to_end(&mut snap)?;
        decoder.finish();
        if snap.len() != length {
            return Err(NesError::LengthMismatch(format!(
                "Snapshot size: {length} != file loaded size: {}",
                snap.len()
            )));
        }
        self.mapper_id = snap[0];
        let mut ptr = 1;
        self.cpu = postcard::from_bytes(&snap[ptr..(ptr + size_of::<Cpu>())])?;
        ptr += size_of::<Cpu>();
        let ram_size = self.bus_cpu.chips.ram.len();
        (&mut self.bus_cpu.chips.ram).copy_from_slice(&snap[ptr..(ptr + ram_size)]);
        ptr += ram_size;
        self.bus_ppu.set_nametables(&snap[ptr..(ptr + self.bus_ppu.get_nametables().len())]);
        ptr += self.bus_ppu.get_nametables().len();
        self.bus_ppu.set_palette_data(&snap[ptr..(ptr + self.bus_ppu.get_palette_data().len())]);
        ptr += self.bus_ppu.get_palette_data().len();
        self.mapper = game_rom::create_mapper_from_snapshot(self.mapper_id, &snap[ptr..])?;
        Ok(())
    }
}
