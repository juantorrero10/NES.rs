use std::fmt::Write;

use serde::{Deserialize, Serialize};

use crate::nes::{
    bus_cpu::BusCpuTarget, game_rom::PpuNameTblMirroring, mappers::Mapper, ppu::BusPpuTarget,
};

const CHR_WINDOW_SIZE: usize = 8192;

#[derive(Serialize, Deserialize)]
pub struct CNROM {
    prg_data: Vec<u8>,
    chr_data: Vec<u8>,
    chr_bank_selected: usize,
    mirror: PpuNameTblMirroring,
}

impl CNROM {
    pub fn new(prg_data: Vec<u8>, chr_data: Vec<u8>, mirror: PpuNameTblMirroring) -> Self {
        Self {
            prg_data,
            chr_data,
            mirror,
            chr_bank_selected: 0,
        }
    }
}

impl Mapper for CNROM {
    fn cpu_need_intercept(&mut self, addr: u16) -> bool {
        addr >= 0x8000
    }

    fn cpu_intercept<'a>(
        &'a mut self,
        addr: u16,
        _: &'a mut crate::nes::bus_cpu::BusCpuChips,
        data: u8,
        write: bool,
    ) -> crate::nes::bus_cpu::BusCpuTarget<'a> {
        if write {
            self.chr_bank_selected = (data & 0x0F) as usize;
            return BusCpuTarget::OpenBus;
        }

        BusCpuTarget::Memory(&mut self.prg_data[(addr - 0x8000) as usize])
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
    ) -> crate::nes::ppu::BusPpuTarget<'a> {
        if write {
            return BusPpuTarget::OpenBus;
        }

        BusPpuTarget::Memory(
            &mut self.chr_data[(self.chr_bank_selected * CHR_WINDOW_SIZE) + addr as usize],
        )
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
        let _ = write!(
            out,
            "(PPU) $0000-$1FFF mapped to: ${:06X} (bank no: {})",
            self.chr_bank_selected * CHR_WINDOW_SIZE,
            self.chr_bank_selected
        );
        true
    }
}
