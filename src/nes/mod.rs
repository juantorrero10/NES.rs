use std::{cell::RefCell, error::Error, ffi::OsString, fmt::Write, rc::Rc};

use lazy_static::lazy_static;
use thiserror::Error;

use crate::nes::{
    apu::Apu, bus_cpu::{BusCpu, BusCpuError}, controller::Controller, cpu::{Cpu, CpuError}, game_rom::ROMErr, mapper::{CpuMapper, PpuMapper}, ppu::{BusPpu, BusPpuChips, Ppu, PpuError, PpuRegister, ppu_flags::STATUS_VBLANK}
};

pub mod bus_cpu;
pub mod cpu;
mod game_rom;
pub mod instruction_set;
mod mapper;
mod ppu;
pub mod controller;
mod apu;


pub struct Nes {
    pub cpu: Cpu,
    pub ppu: Ppu,
    pub bus_cpu: BusCpu,
    pub bus_ppu: BusPpu,
    pub apu: Apu,
    pub controller: Controller,
    pub cpu_mapper: Box<dyn CpuMapper>,
    pub ppu_mapper: Box<dyn PpuMapper>,
    oam_dma_buffer: [u8; 256],
    ppu_cpu_clock_frequency: u8,
}

#[derive(Error, Debug)]
pub enum NesError {
    #[error("Could not read/write to Cpu bus: {0}")]
    BusCpuError(#[from] BusCpuError),
    #[error("{0}")]
    NesError(#[from] ROMErr),
    #[error("{0}")]
    CpuError(#[from] CpuError),
    #[error("Couldn't respond to '{0:?}' event")]
    UnhandledEvent(NesEvent),
}

type Result<T> = std::result::Result<T, NesError>;

impl Nes {
    pub fn new(game_file_path: OsString) -> Result<Self> {
        let rom = game_rom::load_rom(game_file_path)?;
        let cpu_mapper = rom.cpu_mapper;
        let ppu_mapper = rom.ppu_mapper;
        let cpu = Cpu::new();
        let bus_cpu = BusCpu::new();
        let ppu = Ppu::new();
        let apu = Apu{};
        let controller = Controller::default();
        let bus_ppu = BusPpu::new();


        Ok(Nes {
            bus_cpu,
            bus_ppu,
            ppu, 
            cpu,
            apu,
            controller,
            cpu_mapper,
            ppu_mapper,
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

    pub fn read(&mut self, addr: u16) -> Result<u8> {
        todo!()
    }

    pub fn debug_read(&mut self, addr: u16) -> Result<u8> {
        todo!()
    }

    pub fn write(&mut self, addr: u16, data: u8) -> Result<()> {
        todo!()
    }

    pub fn read16(&mut self, addr: u16) -> Result<u16> {
        todo!()
    }

    pub fn debug_read16(&mut self, addr: u16) -> Result<u16> {
        todo!()
    }

    pub fn ppu_read(&mut self, addr: u16) -> u8 {
        todo!()
    }

    pub fn ppu_write(&mut self, addr: u16, data: u8) {
        todo!()
    }

    pub fn clock(&mut self, log: Option<&mut String>) -> Result<()> {

        let instance = std::ptr::from_mut(self);

        self.ppu.frame_complete = false;
        self.ppu.clock(instance);
        self.ppu.clock(instance);
        self.ppu.clock(instance);
        self.ppu_cpu_clock_frequency += 1;

        if true{// self.ppu_cpu_clock_frequency >= 3 {
            self.ppu_cpu_clock_frequency = 0;

            // VBlank check 
            let nmi_signal = self.ppu.nmi_signal;
            if nmi_signal {
                self.cpu.nmi(instance);
                self.ppu.nmi_signal = false;
            }

            match log {
                None => { self.cpu.clock(None, instance); }
                Some(s) => {
                    if self.cpu.state.cycles_left != 0 {
                        self.cpu.clock(None, instance);
                    }
                    else {
                        self.cpu.clock(Some(s), instance);
                        self.finish_nestest_fmt(s);
                    }
                }
            }
        }
        
        Ok(())
    }

    pub fn finish_nestest_fmt(&self, s: &mut String) {
        s.push_str("PPU:");
        let (mut sc, ppu_cyc) = self.ppu.get_cycle_and_sc();
        sc += 1;
        match sc {
            0..=9 => { s.push_str("  "); }
            10..=99 | -1 => { s.push(' '); }
            _ => {}
        }
        write!(s, "{},", sc);
        match ppu_cyc {
            0..=9 => { s.push_str("  "); }
            10..=99 => { s.push(' '); }
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

struct NesComm {
    ppu: Rc<RefCell<Ppu>>,
    apu: Rc<RefCell<Apu>>,
    controller: Rc<RefCell<Controller>>
    // Maybe more in the future
}

fn trigger_read_event(comm: &NesComm, e: NesEvent) -> std::result::Result<u8, Box<dyn Error>> {
    match e {
        NesEvent::PpuRegReadFrom(r) => {
            Ok(comm.ppu.borrow_mut().event_cpu_has_read(r)?)
        },
        NesEvent::ControllerReadFrom(addr) => {
            Ok(comm.controller.borrow_mut().event_cpu_has_read(addr))
        },
        NesEvent::ApuReadFrom(addr) => {
            Ok(comm.apu.borrow_mut().event_cpu_has_read(addr))
        }
        _ => Err(Box::new(NesError::UnhandledEvent(e)))
    }
}

fn trigger_write_event(comm: &NesComm, e: NesEvent, data: u8) -> std::result::Result<(), Box<dyn Error>> {
    match e {
        NesEvent::PpuRegWrittenTo(r) => {
            Ok(comm.ppu.borrow_mut().event_cpu_has_written(r, data)?)
        },
        NesEvent::ApuWrittenTo(addr) => {
            Ok(comm.apu.borrow_mut().event_cpu_has_written(addr, data))
        }
        NesEvent::ControllerWrittenTo => {
            Ok(comm.controller.borrow_mut().event_cpu_has_written(data))
        }
        NesEvent::OAMDMATransfer(ptr, len) => {
            let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
            Ok(comm.ppu.borrow_mut().event_oam_dma_transfer(slice)?)
        }
        _ => Err(Box::new(NesError::UnhandledEvent(e)))
    }
}
