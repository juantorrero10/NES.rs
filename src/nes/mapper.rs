use crate::nes::{bus_cpu::{BusCpuChips, BusTarget::{self, Memory}}, game_rom::MemoryMirroring, ppu::{ADDR_CHR_END, BusPpuChips}};

pub trait CpuMapper {
    fn need_intercept(&self, addr: u16) -> bool;
    #[allow(unused_variables)]
    fn intercept<'a>(&'a mut self, addr: u16, chips: &'a mut BusCpuChips, write: bool) -> BusTarget<'a>;
}
pub trait PpuMapper {
    fn need_intercept(&self, addr: u16) -> bool;
    #[allow(unused_variables)]
    fn intercept<'a>(&'a mut self, addr: u16, chips: &'a mut BusPpuChips, mirroring: MemoryMirroring) -> &'a mut u8;
}

#[derive(Clone)]
pub struct CpuMapper0 {
    n_prg_banks: usize,
    prg_ram: Vec<u8>,
    prg_rom: Vec<u8>,
}

pub struct PpuMapper0 {
    chr_rom: Vec<u8>,
}

impl CpuMapper0 {
    pub fn new(prg_rom: Vec<u8>, n_prg_banks: usize, mut prg_ram_size: usize) -> Self {
        if prg_ram_size == 0 {
            prg_ram_size = 8192;
        }
        CpuMapper0 {
            prg_rom,
            n_prg_banks,
            prg_ram: vec![0u8; prg_ram_size],
        }
    }
}

impl CpuMapper for CpuMapper0 {
    fn need_intercept(&self, addr: u16) -> bool {
        return addr >= 0x6000
    }
    fn intercept<'a>(&'a mut self, addr: u16, _: &'a mut BusCpuChips, _: bool) -> BusTarget<'a> {
        match addr {
            0x6000..=0x7FFF => Memory(&mut self.prg_ram[ addr as usize - 0x6000]),
            0x8000..=0xBFFF => Memory(&mut self.prg_rom[ addr as usize - 0x8000]),
            0xC000..=0xFFFF => {
                Memory(&mut self.prg_rom[ addr as usize - if self.n_prg_banks > 1 {
                    0x8000
                } else {
                    // Mirror first bank
                    0xC000
                }])
            }
            _ => unreachable!()
        }
    }
}

impl PpuMapper0 {
    pub fn new(chr_rom: Vec<u8>) -> Self {
        PpuMapper0 { chr_rom }
    }
}

impl PpuMapper for PpuMapper0 {
    fn need_intercept(&self, addr: u16) -> bool {
        return addr <= ADDR_CHR_END;
    }
    fn intercept<'a>(&'a mut self, addr: u16, _: &'a mut BusPpuChips, _: MemoryMirroring) -> &'a mut u8 {
        &mut self.chr_rom[addr as usize]
    }
}


