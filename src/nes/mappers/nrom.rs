use serde::{Deserialize, Serialize};

use crate::{nes::game_rom::PpuNameTblMirroring};

use super::*;

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

#[derive(Clone, Serialize, Deserialize)]
pub struct NROM {
    prg_ram: Vec<u8>,
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    mirroring: PpuNameTblMirroring,
}

impl NROM {
    pub fn new(
        prg_rom: Vec<u8>,
        mut prg_ram_size: usize,
        chr_rom: Vec<u8>,
        mirroring: PpuNameTblMirroring,
    ) -> Self {
        if prg_ram_size == 0 {
            prg_ram_size = 8192;
        }
        NROM {
            prg_rom,
            chr_rom,
            prg_ram: vec![0u8; prg_ram_size],
            mirroring,
        }
    }
}

impl Mapper for NROM {
    fn cpu_need_intercept(&mut self, addr: u16) -> bool {
        return addr >= 0x6000;
    }
    fn cpu_intercept<'a>(
        &'a mut self,
        addr: u16,
        _: &'a mut BusCpuChips,
        _: u8,
        write: bool,
    ) -> BusCpuTarget<'a> {
        match addr {
            0x6000..=0x7FFF => BusCpuTarget::Memory(&mut self.prg_ram[addr as usize - 0x6000]),
            0x8000..=0xBFFF => rom!(self, write, &mut self.prg_rom[addr as usize - 0x8000]),
            0xC000..=0xFFFF => {
                let rom_size = self.prg_rom.len();
                rom!(
                    self,
                    write,
                    &mut self.prg_rom
                        [addr as usize - if rom_size > 16384 { 0x8000 } else { 0xC000 }]
                )
            }
            _ => unreachable!(),
        }
    }

    fn ppu_need_intercept(&mut self, addr: u16, _: bool) -> bool {
        return addr <= ADDR_CHR_END;
    }
    fn ppu_intercept<'a>(
        &'a mut self,
        addr: u16,
        _: &'a mut BusPpuChips,
        _: u8,
        write: bool,
    ) -> BusPpuTarget<'a> {
        ppu_rom!(self, write, &mut self.chr_rom[addr as usize])
    }

    fn mirroring(&self) -> PpuNameTblMirroring {
        self.mirroring
    }

    fn get_snapshot_size(&self) -> usize {
        size_of::<Self>() + self.prg_rom.len() + self.prg_ram.len() + self.chr_rom.len()
    }

    fn take_snapshot(&self, buff: &mut [u8]) -> Result<(), postcard::Error> {
        assert_eq!(buff.len(), self.get_snapshot_size());
        postcard::to_slice(self, buff)?;
        Ok(())
    }
}
