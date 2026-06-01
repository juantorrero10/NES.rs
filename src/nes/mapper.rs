use crate::nes::{
    bus_cpu::{BusCpuChips, BusCpuTarget},
    game_rom::MemoryMirroring,
    ppu::{ADDR_CHR_END, BusPpuChips, BusPpuTarget},
};

pub trait Mapper {
    fn cpu_need_intercept(&self, addr: u16) -> bool;
    #[allow(unused_variables)]
    fn cpu_intercept<'a>(
        &'a mut self,
        addr: u16,
        chips: &'a mut BusCpuChips,
        write: bool,
    ) -> BusCpuTarget<'a>;

    fn irq_active(&self) -> bool {
        false
    }

    fn irq_clear(&mut self) {}

    fn ppu_need_intercept(&self, addr: u16) -> bool;
    #[allow(unused_variables)]
    fn ppu_intercept<'a>(
        &'a mut self,
        addr: u16,
        chips: &'a mut BusPpuChips,
        write: bool,
    ) -> BusPpuTarget<'a>;

    fn mirroring(&self) -> MemoryMirroring;

    fn a12_edge_detector(&mut self, _old_addr: u16, _new_addr: u16) {}
}

macro_rules! rom {
    ($self:expr, $write:expr, $read_ret:expr) => {
        match $write {
            true => BusCpuTarget::OpenBus,
            false => BusCpuTarget::Memory($read_ret),
        }
    };
}

macro_rules! ppu_rom {
    ($self:expr, $write:expr, $read_ret:expr) => {
        match $write {
            true => BusPpuTarget::OpenBus,
            false => BusPpuTarget::Memory($read_ret),
        }
    };
}

#[derive(Clone)]
pub struct MMC0 {
    n_prg_banks: usize,
    prg_ram: Vec<u8>,
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    mirroring: MemoryMirroring,
}

impl MMC0 {
    pub fn new(
        prg_rom: Vec<u8>,
        n_prg_banks: usize,
        mut prg_ram_size: usize,
        chr_rom: Vec<u8>,
        mirroring: MemoryMirroring,
    ) -> Self {
        if prg_ram_size == 0 {
            prg_ram_size = 8192;
        }
        MMC0 {
            prg_rom,
            chr_rom,
            n_prg_banks,
            prg_ram: vec![0u8; prg_ram_size],
            mirroring,
        }
    }
}

impl Mapper for MMC0 {
    fn cpu_need_intercept(&self, addr: u16) -> bool {
        return addr >= 0x6000;
    }
    fn cpu_intercept<'a>(
        &'a mut self,
        addr: u16,
        _: &'a mut BusCpuChips,
        write: bool,
    ) -> BusCpuTarget<'a> {
        match addr {
            0x6000..=0x7FFF => BusCpuTarget::Memory(&mut self.prg_ram[addr as usize - 0x6000]),
            0x8000..=0xBFFF => rom!(self, write, &mut self.prg_rom[addr as usize - 0x8000]),
            0xC000..=0xFFFF => {
                rom!(
                    self,
                    write,
                    &mut self.prg_rom
                        [addr as usize - if self.n_prg_banks > 1 { 0x8000 } else { 0xC000 }]
                )
            }
            _ => unreachable!(),
        }
    }

    fn ppu_need_intercept(&self, addr: u16) -> bool {
        return addr <= ADDR_CHR_END;
    }
    fn ppu_intercept<'a>(
        &'a mut self,
        addr: u16,
        _: &'a mut BusPpuChips,
        write: bool,
    ) -> BusPpuTarget<'a> {
        ppu_rom!(self, write, &mut self.chr_rom[addr as usize])
    }

    fn mirroring(&self) -> MemoryMirroring {
        self.mirroring
    }
}

pub struct MMC3 {
    n_prg_banks: usize,
    n_chr_banks: usize,
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_ram: Vec<u8>,

    bank_select: u8,
    registers: [usize; 4],

    prg_offsets: [usize; 4], // Offsets for the four 8KB PRG windows
    chr_offsets: [usize; 8], // Offsets for the eight 1KB CHR windows

    mirroring: MemoryMirroring,

    // --- Internal Hardware Scanline Counter (IRQ) ---
    irq_latch: u8,
    irq_counter: u8,
    irq_enabled: bool,
    irq_reload: bool,
}

impl MMC3 {
    pub fn new(
        prg_rom: Vec<u8>,
        n_prg_banks: usize,
        n_chr_banks: usize,
        mut prg_ram_size: usize,
        chr_rom: Vec<u8>,
        mirroring: MemoryMirroring,
    ) -> Self {
        MMC3 {
            n_prg_banks,
            n_chr_banks,
            prg_rom,
            chr_rom,
            prg_ram: vec![0u8; prg_ram_size],
            bank_select: 0,
            registers: [0usize; 4],
            prg_offsets: [0, 0x2000, 0x4000, 0x6000],
            chr_offsets: [0, 0x0400, 0x0600, 0x0800, 0x0C00, 0x1000, 0x1400, 0x1800],
            mirroring,
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_reload: false,
        }
    }
}


impl Mapper for MMC3 {
    fn a12_edge_detector(&mut self, _old_addr: u16, _new_addr: u16) {
        todo!()
    }
    fn cpu_intercept<'a>(
        &'a mut self,
        addr: u16,
        chips: &'a mut BusCpuChips,
        write: bool,
    ) -> BusCpuTarget<'a>
    {
        todo!()
    }
    fn cpu_need_intercept(&self, addr: u16) -> bool {
        todo!()
    }
    fn irq_active(&self) -> bool {
        todo!()
    }
    fn irq_clear(&mut self) {
        todo!()
    }
    fn mirroring(&self) -> MemoryMirroring {
        todo!()
    }
    fn ppu_intercept<'a>(
        &'a mut self,
        addr: u16,
        chips: &'a mut BusPpuChips,
        write: bool,
    ) -> BusPpuTarget<'a>
    {
        todo!()
    }
    fn ppu_need_intercept(&self, addr: u16) -> bool {
        todo!()
    }
}