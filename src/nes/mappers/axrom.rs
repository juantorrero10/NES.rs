use std::fmt::Write;

use serde::{Deserialize, Serialize};

use crate::nes::{
    bus_cpu::BusCpuTarget, game_rom::PpuNameTblMirroring, mappers::Mapper, ppu::BusPpuTarget,
};

const PRG_WINDOW_SIZE: usize = 32768;

#[derive(Serialize, Deserialize)]
pub struct AxROM {
    prg_bank_selected: usize,
    prg_rom: Vec<u8>,
    chr_ram: Vec<u8>,
    mirror: PpuNameTblMirroring,
}

impl AxROM {
    pub fn new(prg_rom: Vec<u8>) -> Self {
        Self {
            prg_bank_selected: 0,
            prg_rom,
            chr_ram: vec![0u8; 8192],
            mirror: PpuNameTblMirroring::OnScreenLO,
        }
    }
}

impl Mapper for AxROM {
    fn cpu_need_intercept(&mut self, addr: u16) -> bool {
        addr >= 0x8000
    }

    fn cpu_intercept<'a>(
        &'a mut self,
        addr: u16,
        _: &'a mut crate::nes::bus_cpu::BusCpuChips,
        data: u8,
        write: bool,
    ) -> BusCpuTarget<'a> {
        if write {
            let num_banks = self.prg_rom.len() / PRG_WINDOW_SIZE;
            let bank_mask = (num_banks - 1) as u8;

            self.prg_bank_selected = (data & bank_mask) as usize;
            self.mirror = match data & 0x10 != 0 {
                false => PpuNameTblMirroring::OnScreenLO,
                true => PpuNameTblMirroring::OnScreenHI,
            };
            return BusCpuTarget::OpenBus;
        }

        BusCpuTarget::Memory(
            &mut self.prg_rom[self.prg_bank_selected * PRG_WINDOW_SIZE + addr as usize - 0x8000],
        )
    }

    fn ppu_need_intercept(&mut self, addr: u16, _: bool) -> bool {
        addr <= 0x1FFF
    }

    fn mirroring(&self) -> PpuNameTblMirroring {
        self.mirror
    }

    fn ppu_intercept<'a>(
        &'a mut self,
        addr: u16,
        _: &'a mut crate::nes::ppu::BusPpuChips,
        _: u8,
        _: bool,
    ) -> BusPpuTarget<'a> {
        BusPpuTarget::Memory(&mut self.chr_ram[addr as usize])
    }

    fn get_snapshot_size(&self) -> usize {
        size_of::<Self>() + self.prg_rom.len() + self.chr_ram.len()
    }

    fn take_snapshot(&self, buff: &mut [u8]) -> Result<(), postcard::Error> {
        assert_eq!(buff.len(), self.get_snapshot_size());
        postcard::to_slice(self, buff)?;
        Ok(())
    }

    fn debug_information(&self, out: &mut String) -> bool {
        let _ = write!(
            out,
            "$8000-$FFFF mapped to: ${:06X} (bank no: {})",
            self.prg_bank_selected * PRG_WINDOW_SIZE,
            self.prg_bank_selected
        );
        true
    }
}
