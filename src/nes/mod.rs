use std::{cell::RefCell, rc::Rc};

use crate::nes::{bus_cpu::BusCpu, cpu::Cpu, game_rom::ROMErr, ppu::Ppu};

pub mod bus_cpu;
pub mod cpu;
mod game_rom;
pub mod instruction_set;
mod mapper;
mod ppu;

pub struct Nes {
    pub cpu: Cpu,
    pub ppu: Ppu,
    pub bus: Rc<RefCell<BusCpu>>,
}

impl Nes {
    pub fn new(game_file_path: &str) -> Result<Self, ROMErr> {
        let rom = game_rom::load_rom(game_file_path)?;
        let bus = Rc::new(RefCell::new(BusCpu::new(
            rom.cpu_mapper,
            rom.prg_data,
            rom.prg_ram,
        )));
        Ok(Nes {
            bus: bus.clone(),
            ppu: Ppu::new(rom.ppu_mapper, rom.chr_data, rom.mirror),
            cpu: Cpu::new(bus.clone()),
        })
    }
}
