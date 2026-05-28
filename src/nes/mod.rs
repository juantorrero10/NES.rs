use std::{cell::RefCell, error::Error, rc::Rc};

use thiserror::Error;

use crate::nes::{
    bus_cpu::BusCpu,
    cpu::{Cpu, CpuError},
    game_rom::ROMErr,
    ppu::{Ppu, PpuError, PpuRegister},
};

pub mod bus_cpu;
pub mod cpu;
mod game_rom;
pub mod instruction_set;
mod mapper;
mod ppu;

pub struct Nes {
    pub cpu: Cpu,
    pub ppu: Rc<RefCell<Ppu>>,
    pub bus: Rc<RefCell<BusCpu>>,

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

        let comms = NesComm {
            ppu: ppu.clone(),
        };
        bus.borrow_mut().add_communications(comms);

        Ok(Nes {
            bus: bus.clone(),
            ppu, 
            cpu: Cpu::new(bus.clone()),
            ppu_cpu_clock_frequency: 0,
        })
    }

    pub fn clock(&mut self) -> Result<()> {
        self.ppu.borrow_mut().clock();
        if self.ppu_cpu_clock_frequency == 3 {
            self.ppu_cpu_clock_frequency = 0;
            self.cpu.clock()?;
        }
        self.ppu_cpu_clock_frequency += 1;
        Ok(())
    }
}

// Communication between components
#[derive(Debug)]
enum NesEvent {
    PpuRegWrittenTo(PpuRegister),
    PpuRegReadFrom(PpuRegister),
}

struct NesComm {
    ppu: Rc<RefCell<Ppu>>,
    // Maybe more in the future
}

fn trigger_read_event(comm: &NesComm, e: NesEvent) -> std::result::Result<u8, Box<dyn Error>> {
    match e {
        NesEvent::PpuRegReadFrom(r) => {
            Ok(comm.ppu.borrow_mut().event_cpu_has_read(r)?)
        },
        _ => Err(Box::new(NesError::UnhandledEvent(e)))
    }
}

fn trigger_write_event(comm: &NesComm, e: NesEvent, data: u8) -> std::result::Result<(), Box<dyn Error>> {
    match e {
        NesEvent::PpuRegWrittenTo(r) => {
            Ok(comm.ppu.borrow_mut().event_cpu_has_written(r, data)?)
        },
        _ => Err(Box::new(NesError::UnhandledEvent(e)))
    }
}
