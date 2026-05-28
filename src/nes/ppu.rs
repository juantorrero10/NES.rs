use crate::nes::{game_rom::MemoryMirroring, mapper::PpuMapper};



pub struct Ppu {
    mapper: Box<dyn PpuMapper>,
    mirroring: MemoryMirroring,
    chr_data: Vec<u8>,
    cycles_left: u8,
}

impl Ppu {
    pub fn new(mapper: Box<dyn PpuMapper>, chr_data: Vec<u8>, mirroring: MemoryMirroring) -> Self {
        Ppu { mapper, cycles_left: 0, chr_data, mirroring }
    }
}