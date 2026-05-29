use std::{cell::RefCell, error::Error, fmt::Write, rc::Rc};

use thiserror::Error;

use crate::nes::{
    apu::Apu, bus_cpu::BusCpu, controller::Controller, cpu::{Cpu, CpuError}, game_rom::ROMErr, ppu::{Ppu, PpuError, PpuRegister, ppu_flags::STATUS_VBLANK}
};

pub mod bus_cpu;
pub mod cpu;
mod game_rom;
pub mod instruction_set;
mod mapper;
mod ppu;
mod controller;
mod apu;

pub struct Nes {
    pub cpu: Cpu,
    pub ppu: Rc<RefCell<Ppu>>,
    pub bus: Rc<RefCell<BusCpu>>,
    pub apu: Rc<RefCell<Apu>>,
    pub controller: Rc<RefCell<Controller>>,
    ppu_cpu_clock_frequency: u8,
}

#[derive(Error, Debug)]
pub enum NesError {
    #[error("{0}")]
    PpuError(#[from] PpuError),
    #[error("{0}")]
    NesError(#[from] ROMErr),
    #[error("{0}")]
    CpuError(#[from] CpuError),
    #[error("Couldn't respond to '{0:?}' event")]
    UnhandledEvent(NesEvent),
}

type Result<T> = std::result::Result<T, NesError>;

impl Nes {
    pub fn new(game_file_path: &str) -> Result<Self> {
        let rom = game_rom::load_rom(game_file_path)?;
        
        let bus = Rc::new(RefCell::new(BusCpu::new(
            rom.cpu_mapper,
            rom.prg_data,
            rom.prg_ram,
        )));
        
        let ppu = Rc::new(RefCell::new(Ppu::new( 
            rom.ppu_mapper, 
            rom.chr_data, 
            rom.mirror
        )));

        let apu = Rc::new(RefCell::new(Apu{}));
        let controller = Rc::new(RefCell::new(Controller{}));

        let comms = NesComm {
            ppu: ppu.clone(),
            apu: apu.clone(),
            controller: controller.clone(),
        };
        bus.borrow_mut().add_communications(comms);

        Ok(Nes {
            bus: bus.clone(),
            ppu, 
            cpu: Cpu::new(bus.clone()),
            apu,
            controller,
            ppu_cpu_clock_frequency: 0,
        })
    }

    pub fn reset(&mut self) -> Result<()> {
        self.cpu.reset()?;
        self.ppu.borrow_mut().reset();
        Ok(())
    }

    pub fn clock(&mut self, log: Option<&mut String>) -> Result<()> {

        self.ppu_cpu_clock_frequency += 1;

        if self.ppu_cpu_clock_frequency >= 3 {
            self.ppu_cpu_clock_frequency = 0;

            // VBlank check 
            let nmi_signal = self.ppu.borrow().nmi_signal;
            if nmi_signal {
                self.cpu.nmi()?;
                self.ppu.borrow_mut().nmi_signal = false;
            }

            match log {
                None => { self.cpu.clock(None); }
                Some(s) => {
                    if self.cpu.state.cycles_left != 0 {
                        self.cpu.clock(None);
                    }
                    else {
                        self.cpu.clock(Some(s));
                        self.finish_nestest_fmt(s);
                    }
                }
            }
        }

        self.ppu.borrow_mut().clock();
        
        Ok(())
    }

    pub fn finish_nestest_fmt(&self, s: &mut String) {
        s.push_str("PPU:");
        let (mut sc, ppu_cyc) = self.ppu.borrow().get_cycle_and_sc();
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
    ControllerReadFrom(u16),
    ControllerWrittenTo(u16),
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
        NesEvent::ControllerWrittenTo(addr) => {
            Ok(comm.controller.borrow_mut().event_cpu_has_written(addr, data))
        }
        _ => Err(Box::new(NesError::UnhandledEvent(e)))
    }
}
