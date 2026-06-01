use std::{cell::RefCell, error::Error, ffi::OsString, fmt::Write, rc::Rc};
use thiserror::Error;

use crate::nes::{
    apu::Apu,
    bus_cpu::{BusCpu, BusCpuError, IOResult},
    controller::Controller,
    cpu::{Cpu, CpuError},
    game_rom::ROMErr,
    mapper::Mapper,
    ppu::{BusPpu, BusPpuChips, Ppu, PpuError, PpuRegister, ppu_flags::STATUS_VBLANK},
};

mod apu;
pub mod bus_cpu;
pub mod controller;
pub mod cpu;
mod game_rom;
pub mod instruction_set;
mod mapper;
mod ppu;

pub struct Nes {
    pub cpu: Cpu,
    pub ppu: Ppu,
    pub bus_cpu: BusCpu,
    pub bus_ppu: BusPpu,
    pub apu: Apu,
    pub controller: Controller,
    pub mapper: Box<dyn Mapper>,
    oam_dma_buffer: [u8; 256],
    ppu_cpu_clock_frequency: u8,
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
}

type Result<T> = std::result::Result<T, NesError>;

impl Nes {
    pub fn new(game_file_path: OsString) -> Result<Self> {
        let mapper = game_rom::load_rom(game_file_path)?;
        let cpu = Cpu::new();
        let bus_cpu = BusCpu::new();
        let ppu = Ppu::new();
        let apu = Apu {};
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
            oam_dma_buffer: [0u8; 256],
            ppu_cpu_clock_frequency: 0,
        })
    }

    pub fn reset(&mut self) -> Result<()> {
        let instance = std::ptr::from_mut(self);
        self.cpu.reset(instance);
        self.ppu.reset();
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
                let d = self.trigger_read_event(e)?;
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
                self.trigger_write_event(e, data)?;
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
    pub fn ppu_write(&mut self, addr: u16, data: u8) {
        // let _span = tracy_client::span!("ppu write");
        self.bus_ppu.write(addr, data, &mut self.mapper);
    }

    pub fn clock(&mut self, _: Option<&mut String>) -> Result<()> {
        // let _span = tracy_client::span!("Clock Nes");

        let instance = std::ptr::from_mut(self);

        self.ppu.frame_complete = false;
        self.ppu.clock(instance);
        self.ppu.clock(instance);
        self.ppu.clock(instance);
        self.ppu_cpu_clock_frequency += 1;

        let nmi_signal = self.ppu.nmi_signal;
        if nmi_signal {
            self.cpu.nmi(instance);
            self.ppu.nmi_signal = false;
        }
        self.cpu.clock(None, instance);

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

    pub fn finish_nestest_fmt(&self, s: &mut String) {
        s.push_str("PPU:");
        let (mut sc, ppu_cyc) = self.ppu.get_cycle_and_sc();
        sc += 1;
        match sc {
            0..=9 => {
                s.push_str("  ");
            }
            10..=99 | -1 => {
                s.push(' ');
            }
            _ => {}
        }
        write!(s, "{},", sc);
        match ppu_cyc {
            0..=9 => {
                s.push_str("  ");
            }
            10..=99 => {
                s.push(' ');
            }
            _ => {}
        }
        write!(s, "{} ", ppu_cyc);
        write!(s, "CYC:{}", self.cpu.state.clock_counter - 1);
    }
}

// Communication between components
#[derive(Debug)]
enum NesEvent {
    PpuRegWrittenTo(PpuRegister),
    PpuRegReadFrom(PpuRegister),
    OAMDMATransfer(u8),
    ControllerReadFrom(u16),
    ControllerWrittenTo,
    ApuWrittenTo(u16),
    ApuReadFrom(u16),
}

// Event logic
impl Nes {
    fn trigger_read_event(&mut self, e: NesEvent) -> std::result::Result<u8, Box<dyn Error>> {
        let instance = std::ptr::from_mut(self);
        match e {
            NesEvent::PpuRegReadFrom(r) => Ok(self.ppu.event_cpu_has_read(r, instance)?),
            NesEvent::ControllerReadFrom(addr) => Ok(self.controller.event_cpu_has_read(addr)),
            NesEvent::ApuReadFrom(addr) => Ok(self.apu.event_cpu_has_read(addr)),
            _ => Err(Box::new(NesError::UnhandledEvent(e))),
        }
    }

    fn trigger_write_event(
        &mut self,
        e: NesEvent,
        data: u8,
    ) -> std::result::Result<(), Box<dyn Error>> {
        let instance = std::ptr::from_mut(self);
        match e {
            NesEvent::PpuRegWrittenTo(r) => {
                Ok(self.ppu.event_cpu_has_written(r, data, instance)?)
            }
            NesEvent::ApuWrittenTo(addr) => Ok(self.apu.event_cpu_has_written(addr, data)),
            NesEvent::ControllerWrittenTo => Ok(self.controller.event_cpu_has_written(data)),
            NesEvent::OAMDMATransfer(addr_start) => {
                let page = (addr_start as u16) << 8;
                for i in 0..256 {
                    self.oam_dma_buffer[i] = self.read(page + i as u16)?;
                }
                Ok(self
                    .ppu
                    .event_oam_dma_transfer(self.oam_dma_buffer.as_slice())?)
            }
            _ => Err(Box::new(NesError::UnhandledEvent(e))),
        }
    }
}
