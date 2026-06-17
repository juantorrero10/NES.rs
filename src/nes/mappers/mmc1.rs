use std::fmt::Write;

use serde::{Deserialize, Serialize};

use crate::nes::{bus_cpu::BusCpuTarget, game_rom::PpuNameTblMirroring, mappers::Mapper, ppu::{ADDR_NAME_START, BusPpuTarget}};

const PRG_WINDOW_SIZE: u16 = 0x8000;
const CHR_WINDOW_SIZE: u16 = 0x2000;

#[derive(Serialize, Deserialize)]
pub struct MMC1 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr_data: Vec<u8>,

    is_chr_ram: bool,

    control: u8,

    prg_bank: u8,
    chr_bank_lo: u8,
    chr_bank_hi: u8,

    shift_register: u8,

    is_battery_backed: bool
}

impl MMC1 {
    pub fn new(prg_rom: Vec<u8>, mut chr_data: Vec<u8>, is_battery_backed: bool) -> Self {
        let mut is_chr_ram = false;
        if chr_data.len() == 0 {
            chr_data = vec![0u8; 8192];
            is_chr_ram = true;
        }
        MMC1 {
            chr_data,
            prg_rom,
            is_chr_ram,
            prg_ram: vec![0u8; 8192],
            shift_register: 0b10000,
            prg_bank: 0,
            control: 0x0C,
            chr_bank_lo: 0,
            chr_bank_hi: 0,
            is_battery_backed
        }
    }

    fn registers_write(&mut self, addr: u16, data: u8) {
        let is_fifth_write = (self.shift_register & 1) != 0;
        self.shift_register >>= 1;
        self.shift_register |= (data & 0x1) << 4;

        if is_fifth_write {
            let register_to_modify = match addr {
                0x8000..=0x9FFF => &mut self.control,
                0xA000..=0xBFFF => &mut self.chr_bank_lo,
                0xC000..=0xDFFF => &mut self.chr_bank_hi,
                0xE000..=0xFFFF => &mut self.prg_bank,
                _ => unreachable!()
            };
            *register_to_modify = self.shift_register;
            self.prg_bank &= 0x0F;
            self.shift_register = 0b10000;

        }

        // reset
        if (data & (1 << 7)) != 0 {
            self.shift_register = 0b10000;
            self.control |= 0b01100;
        }

    }
}

impl Mapper for MMC1 {
    fn cpu_intercept<'a>(
        &'a mut self,
        addr: u16,
        _: &'a mut crate::nes::bus_cpu::BusCpuChips,
        data: u8,
        write: bool,    
    ) -> crate::nes::bus_cpu::BusCpuTarget<'a>
    {
        match addr {
            0x6000..=0x7FFF => {
                BusCpuTarget::Memory(&mut self.prg_ram[addr as usize - 0x6000])
            }
            0x8000..=0xFFFF => {
                if write {
                    self.registers_write(addr, data);
                    return BusCpuTarget::OpenBus;
                }
                let prg_bank_mode = (self.control & 0b01100) >> 2;
                let mut target_offset: usize = match prg_bank_mode {
                    // switch 32 KB at $8000, ignoring low bit of bank number
                    0 | 1 => {
                        let bank_off = (self.prg_bank as usize & !1) * PRG_WINDOW_SIZE as usize; 
                        bank_off + (addr as usize & 0x7FFF)
                    }
                    // fix first bank at $8000 and switch 16 KB bank at $C000
                    2 => {
                        if addr >= 0xC000 {
                            let bank_offset = (self.prg_bank as usize) * (PRG_WINDOW_SIZE as usize / 2);
                            bank_offset + (addr as usize & 0x3FFF)
                        } else {
                            // First bank at $8000 - $BFFF
                            addr as usize & 0x3FFF 
                        }
                    }
                    // fix last bank at $C000 and switch 16 KB bank at $8000
                    3 => {
                        if addr >= 0xC000 {
                            // last bank at $C000 - $FFFF
                            let bank_off = self.prg_rom.len() - (PRG_WINDOW_SIZE as usize / 2);
                            bank_off + (addr as usize & 0x3FFF)
                        } else {
                            let bank_off = (self.prg_bank as usize) * (PRG_WINDOW_SIZE as usize / 2);
                            bank_off + (addr as usize & 0x3FFF)
                        }
                    }
                    _ => unreachable!()
                };
                target_offset %= self.prg_rom.len();
                BusCpuTarget::Memory(&mut self.prg_rom[target_offset])
            }
            _ => unreachable!()
        }
    }

    fn get_snapshot_size(&self) -> usize {
        size_of::<Self>() + self.prg_ram.len() + self.prg_rom.len() + self.chr_data.len()
    }

    fn take_snapshot(&self, buff: &mut [u8]) -> Result<(), postcard::Error> {
        assert_eq!(buff.len(), self.get_snapshot_size());
        postcard::to_slice(self, buff)?;
        Ok(())
    }

    fn cpu_need_intercept(&mut self, addr: u16) -> bool {
        addr >= 0x6000
    }

    fn ppu_need_intercept(&mut self, addr: u16, _: bool) -> bool {
        addr < ADDR_NAME_START
    }

    fn is_sram_battery_backed(&self) -> bool {
        self.is_battery_backed
    }

    fn save_sram(&self) -> Option<&[u8]> {
        Some(&self.prg_ram)
    }

    fn load_sram(&mut self, buff: &[u8]) {
        if self.prg_ram.len() != buff.len() {
            log::error!(
                "Tried to load sram to mapper but sizes doesnt match: len(prg_ram): {} != len(file_buff): {}.",
                self.prg_ram.len(),
                buff.len()
            );
            return;
        }

        self.prg_ram.copy_from_slice(buff);
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

        let chr_bank_mode = (self.control & 0x10) != 0;
        // Two banks of 4Kib each. 
        if chr_bank_mode {
            let target_offset = if addr >= 0x1000 { 
                let addr = addr & 0xFFF;    
                ((self.chr_bank_hi as u16 & 0x1f) * (CHR_WINDOW_SIZE / 2)) + addr
            } else {
                ((self.chr_bank_lo as u16 & 0x1f) * (CHR_WINDOW_SIZE / 2)) + addr
            };
            let target_offset = target_offset as usize % self.chr_data.len();
            return BusPpuTarget::Memory(&mut self.chr_data[target_offset]);
        }
        // One bank of 8Kib
        let mut target_offset = ((self.chr_bank_lo as usize & !1) * CHR_WINDOW_SIZE as usize) + addr as usize;
        target_offset %= self.chr_data.len();
        BusPpuTarget::Memory(&mut self.chr_data[target_offset])
    }

    fn mirroring(&self) -> PpuNameTblMirroring {
        match self.control & 0x3 {
            0 => PpuNameTblMirroring::OnScreenLO,
            1 => PpuNameTblMirroring::OnScreenHI,
            2 => PpuNameTblMirroring::Vertical,
            3 => PpuNameTblMirroring::Horizontal,
            _ => unreachable!()
        }
    }

    #[allow(unused)]
    fn debug_information(&self, out: &mut String) -> bool {
        writeln!(out, "CONTROL:     %{:05b}", self.control);
        writeln!(out, "SR:          %{:05b}", self.shift_register);
        writeln!(out, "PRG_BANK:    ${:02X}", self.prg_bank);
        writeln!(out, "CHR_BANK_LO: ${:02X}", self.chr_bank_lo);
        writeln!(out, "CHR_BANK_HI: ${:02X}", self.chr_bank_hi);
        true
    }
}