use std::fmt::Write;

use serde::{Deserialize, Serialize};

use crate::nes::{bus_cpu::BusCpuTarget::{self}, game_rom::PpuNameTblMirroring, mappers::Mapper, ppu::BusPpuTarget};



const PRG_WINDOW_SIZE: usize = 16384;
const CHR_WINDOW_SIZE: usize = 8192;

#[derive(Serialize, Deserialize)]
pub struct UxROM {
    prg_data: Vec<u8>,
    chr_data: Vec<u8>,
    is_chr_ram: bool,
    mirror: PpuNameTblMirroring,
    upper_bank_selected: usize
}


impl UxROM {
    pub fn new(prg_data: Vec<u8>, chr_data: Vec<u8>, mirror: PpuNameTblMirroring) -> Self {
        let (chr_data, is_ram) = match chr_data.len() {
            0 => (vec![0u8; CHR_WINDOW_SIZE], true),
            _ => (chr_data, false),
        };

        Self { prg_data, chr_data, mirror, upper_bank_selected: 0, is_chr_ram: is_ram }
    }
}

impl Mapper for UxROM {

    fn cpu_need_intercept(&mut self, addr: u16) -> bool {
        addr >= 0x8000
    }

    fn cpu_intercept<'a>(
        &'a mut self,
        addr: u16,
        _: &'a mut crate::nes::bus_cpu::BusCpuChips,
        data: u8,
        write: bool,    
    ) -> crate::nes::bus_cpu::BusCpuTarget<'a>
    {
        if write {
            self.upper_bank_selected = data as usize;
            return BusCpuTarget::OpenBus
        }

        return match addr {
            0x8000..=0xBFFF => {
                let rel = addr as usize - 0x8000;
                BusCpuTarget::Memory(&mut self.prg_data[self.upper_bank_selected * PRG_WINDOW_SIZE + rel])
            }
            0xC000..=0xFFFF => {
                let rel = addr as usize - 0xC000;
                let last_bank = self.prg_data.len() - PRG_WINDOW_SIZE;
                BusCpuTarget::Memory(&mut self.prg_data[last_bank + rel])
            }
            _ => unreachable!()
        }
    }

    fn ppu_need_intercept(&mut self, addr: u16, _: bool) -> bool {
        addr <= 0x1FFF
    }

    fn ppu_intercept<'a>(
        &'a mut self,
        addr: u16,
        _: &'a mut crate::nes::ppu::BusPpuChips,
        _: u8,
        write: bool,
    ) -> crate::nes::ppu::BusPpuTarget<'a>
    {
        if write && !self.is_chr_ram {
            return BusPpuTarget::OpenBus;
        }

        BusPpuTarget::Memory(&mut self.chr_data[(addr & 0x1FFF) as usize])
    }

    fn mirroring(&self) -> PpuNameTblMirroring {
        self.mirror
    }

    fn get_snapshot_size(&self) -> usize {
        size_of::<Self>() + self.prg_data.len() + self.chr_data.len()
    }

    fn take_snapshot(&self, buff: &mut [u8]) -> Result<(), postcard::Error> {
        assert_eq!(buff.len(), self.get_snapshot_size());
        postcard::to_slice(self, buff)?;
        Ok(())
    }

    fn debug_information(&self, out: &mut String) -> bool {
        let _ = write!(out, "$8000-$BFFF mapped to: ${:06X} (bank no: {})", self.upper_bank_selected * PRG_WINDOW_SIZE, self.upper_bank_selected);
        true
    }
}