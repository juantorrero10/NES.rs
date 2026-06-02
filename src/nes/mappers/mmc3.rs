use std::fmt::Write;

use crate::nes::{MapperInfoReq, Nes, NesEvent, ppu};

use super::*;

const PRG_WINDOW_SIZE: usize = 8192;
const CHR_WINDOW_SIZE: usize = 1024;

pub struct MMC3 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_ram: Vec<u8>,

    // Bank select register (inflated)
    target_register: u8,
    bank_mode: bool,
    chr_inversion: bool,

    prg_ram_enable: bool,
    prg_ram_allow_write: bool,

    registers: [usize; 8],

    prg_offsets: [usize; 4], // Offsets for the four 8KB PRG windows
    chr_offsets: [usize; 8], // Offsets for the eight 1KB CHR windows

    mirroring: MemoryMirroring,

    last_ppu_addr: u16,
    a12_filter: u8,

    #[cfg(feature = "log")]
    log_enable: bool,
    #[cfg(feature = "log")]
    last_counter_seen: u8,

    // --- Internal Hardware Scanline Counter (IRQ) ---
    irq_latch: u8,
    irq_counter: u8,
    irq_enabled: bool,
    irq_reload: bool,
    irq_is_pending: bool,
}

impl MMC3 {
    pub fn new(
        prg_rom: Vec<u8>,
        mut prg_ram_size: usize,
        chr_rom: Vec<u8>,
        mirroring: MemoryMirroring,
    ) -> Self {
        if prg_ram_size == 0 {
            prg_ram_size = 8192;
        }

        let last_prg_offset = prg_rom.len().saturating_sub(8192);
        let second_to_last_prg_offset = prg_rom.len().saturating_sub(16384);

        MMC3 {
            prg_rom,
            chr_rom,
            prg_ram: vec![0u8; prg_ram_size],

            target_register: 0,
            bank_mode: false,
            chr_inversion: false,

            prg_ram_enable: true,
            prg_ram_allow_write: true,

            last_ppu_addr: 0,
            a12_filter: 3,

            registers: [0; 8],
            prg_offsets: [0, 0x2000, second_to_last_prg_offset, last_prg_offset],
            chr_offsets: [
                0x0000, 0x0400, 0x0600, 0x0800, 0x0C00, 0x1000, 0x1400, 0x1800,
            ],
            #[cfg(feature = "log")]
            log_enable: false,
            #[cfg(feature = "log")]
            last_counter_seen: 1,
            mirroring,
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_reload: false,
            irq_is_pending: false,
        }
    }

    fn register_write(&mut self, addr: u16, data: u8) {
        let is_odd = (addr & 1) != 0;

        match addr {
            0x8000..=0x9FFF => {
                // Bank select
                if !is_odd {
                    // log::indo!("BANK SELECT: {data}");
                    self.target_register = data & 0x7;
                    self.bank_mode = (data & 0x40) != 0;
                    self.chr_inversion = (data & 0x80) != 0;
                // Bank data
                } else {
                    // log::indo!("BANK DATA (register #{}): {data}", self.target_register);
                    self.registers[self.target_register as usize] = data as usize;
                    self.update_offsets();
                }
            }
            0xA000..=0xBFFF => {
                // name table arrangement
                if !is_odd {
                    // let m = self.mirroring;
                    self.mirroring = match data & 1 {
                        0 => MemoryMirroring::Vertical,
                        1 => MemoryMirroring::Horizontal,
                        _ => unreachable!(),
                    };
                    // log::indo!("MIRRORING {:?} -> {:?}", m, self.mirroring);
                    // PRG Ram protect
                } else {
                    // log::info!("RAM PROTECT: {data}");
                    self.prg_ram_enable = (data & 0x80) != 0;
                    // self.prg_ram_allow_write = (data & 0x40) != 0;
                }
            }
            0xC000..=0xDFFF => {
                // IRQ LATCH
                if !is_odd {
                    // log::indo!("IRQ LATCH: {data}");
                    self.irq_latch = data;
                // IRQ RELOAD
                } else {
                    // log::indo!("IRQ RELOAD: Counter: {} -> 0", self.irq_counter);
                    self.irq_counter = 0;
                    self.irq_reload = true;
                }
            }
            0xE000..=0xFFFF => {
                // IRQ DISABLE AND ACKNOWLEDGE
                if !is_odd {
                    self.irq_enabled = false;
                    self.irq_is_pending = false;
                // IRQ ENABLE
                } else {
                    self.irq_enabled = true;
                }
            }

            _ => unreachable!(),
        }
    }
}

impl Mapper for MMC3 {
    fn cpu_intercept<'a>(
        &'a mut self,
        addr: u16,
        _: &'a mut BusCpuChips,
        data: u8,
        write: bool,
    ) -> BusCpuTarget<'a> {
        // Read/Write only PRG RAM if enabled
        match addr {
            0x6000..=0x7FFF => {
                if !self.prg_ram_enable || (!self.prg_ram_allow_write && write) {
                    return BusCpuTarget::OpenBus;
                }
                return BusCpuTarget::Memory(&mut self.prg_ram[addr as usize - 0x6000]);
            }
            _ => {}
        }
        // Write -> Mapper registers
        if write {
            self.register_write(addr, data);
            return BusCpuTarget::OpenBus;
        }

        // Read -> Actual banks
        let addr = addr as usize;
        let index_to_offset = (addr - 0x8000) / PRG_WINDOW_SIZE;
        let mapped_addr = self.prg_offsets[index_to_offset] + (addr % PRG_WINDOW_SIZE);

        BusCpuTarget::Memory(&mut self.prg_rom[mapped_addr])
    }
    fn cpu_need_intercept(&mut self, addr: u16) -> bool {
        addr >= 0x6000
    }
    fn is_irq_pending(&self) -> bool {
        self.irq_is_pending
    }
    fn irq_clear(&mut self) {
        self.irq_is_pending = false;
    }
    fn mirroring(&self) -> MemoryMirroring {
        self.mirroring
    }

    #[cfg(feature = "log")]
    fn toggle_log(&mut self) {
        self.log_enable ^= true;
    }

    fn ppu_intercept<'a>(
        &'a mut self,
        addr: u16,
        _: &'a mut BusPpuChips,
        _: u8,
        write: bool,
    ) -> BusPpuTarget<'a> {
        // Not writable
        if write {
            return BusPpuTarget::OpenBus;
        }

        // Read -> read mapped CHR banks
        let addr = addr as usize;
        let index_to_offset = addr / CHR_WINDOW_SIZE;
        let mapped_addr = self.chr_offsets[index_to_offset] + (addr % CHR_WINDOW_SIZE);

        BusPpuTarget::Memory(&mut self.chr_rom[mapped_addr])
    }

    fn ppu_need_intercept(&mut self, addr: u16) -> bool {
        let old_a12 = (self.last_ppu_addr & 0x1000) != 0;
        let new_a12 = (addr & 0x1000) != 0;

        self.last_ppu_addr = addr;

        let mut a12_happened = false;

        if !old_a12 && new_a12 && self.a12_filter == 3 {
            a12_happened = true;
            if self.irq_reload {
                self.irq_counter = self.irq_latch;
                self.irq_reload = false;
            } else {
                self.irq_counter = self.irq_counter.wrapping_sub(1);
            }

            if self.irq_counter == 0 && self.irq_enabled {
                self.irq_is_pending = true;
            } else if self.irq_counter == 255 {
                self.irq_counter = self.irq_latch;
                if self.irq_counter == 0 && self.irq_enabled {
                    self.irq_is_pending = true;
                }
            }
        }
        if new_a12 {
            self.a12_filter = 0;
        }
        #[cfg(feature = "log")]
        {
            if self.log_enable && self.last_counter_seen != self.irq_counter {
                self.last_counter_seen = self.irq_counter;
                if a12_happened {
                    log::info!(
                        "a12 re? (YES). IRQ STATE => C: {}, L: {}, E: {}, R: {}, P: {}",
                        self.irq_counter,
                        self.irq_latch,
                        self.irq_enabled,
                        self.irq_reload,
                        self.irq_is_pending
                    );
                } else {
                    log::info!(
                        "a12 re? ( NO). IRQ STATE => C: {}, L: {}, E: {}, R: {}, P: {}",
                        self.irq_counter,
                        self.irq_latch,
                        self.irq_enabled,
                        self.irq_reload,
                        self.irq_is_pending
                    );
                }
            }
            
            
        }
        

        addr < 0x2000
    }

    #[allow(unreachable_patterns)]
    fn info_request_response(&mut self, i: crate::nes::MapperInfoReq, data_back: Option<u64>) {
        match i {
            MapperInfoReq::PpuAddressBus => {}
            _ => return,
        }
        if let Some(info_back) = data_back {
            let address_bus = (info_back & 0xFFFF) as u16;
            // This happens on cpu clock
            // log::info!("-- Cpu Clock: addr_bus: {:04X}", ppu_address_bus);
            // log::info!("address_bus: {:014b} prev_address_bus: {:014b}", address_bus, prev_address_bus);
            if (address_bus & 0x1000) == 0 && self.a12_filter < 3 {
                // log::info!("Filter + 1, {}", self.a12_filter);
                self.a12_filter += 1;
            }
        }
    }

    fn on_cpu_clock(&mut self) -> Option<NesEvent> {
        // Request PPU address bus
        Some(NesEvent::InfoReq(MapperInfoReq::PpuAddressBus))
    }

    fn debug_information(&self, out: &mut String) -> bool {
        // "
        // CHR:                 PRG:
        // $0000-$03FF: 0000    $8000-$9FFF: 0000
        // $0400-$07FF: 0000    $A000-$BFFF: 0000
        // $0800-$0BFF: 0000    $C000-$DFFF: 0000
        // $0C00-$0FFF: 0000    $E000-$FFFF: 0000
        // $1000-$13FF: 0000
        // $1400-$17FF: 0000
        // $1800-$1BFF: 0000
        // $1C00-$1FFF: 0000

        // Registers:
        // [0]:00 [1]:00 [2]:00 [3]:00 [4]:00 [5]:00
        // [6]:00 [7]:00

        // IRQ Latch: 0
        // IRQ Counter: 0
        // IRQ Enable: true
        // IRQ Reload: true
        // IRQ Pending: false
        // "
        write!(out, "CHR:                   PRG:\n");
        write!(
            out,
            "$0000-$03FF: {:06X}    $8000-$9FFF: {:06X}\n",
            self.chr_offsets[0], self.prg_offsets[0]
        );
        write!(
            out,
            "$0400-$07FF: {:06X}    $A000-$BFFF: {:06X}\n",
            self.chr_offsets[1], self.prg_offsets[1]
        );
        write!(
            out,
            "$0800-$0BFF: {:06X}    $C000-$DFFF: {:06X}\n",
            self.chr_offsets[2], self.prg_offsets[2]
        );
        write!(
            out,
            "$0C00-$0FFF: {:06X}    $E000-$FFFF: {:06X}\n",
            self.chr_offsets[3], self.prg_offsets[3]
        );
        write!(out, "$1000-$13FF: {:06X}    \n", self.chr_offsets[4]);
        write!(out, "$1400-$17FF: {:06X}    \n", self.chr_offsets[5]);
        write!(
            out,
            "$1800-$1BFF: {:06X}    bank_mode:{}\n",
            self.chr_offsets[6], self.bank_mode
        );
        write!(
            out,
            "$1C00-$1FFF: {:06X}    chr_invert:{}\n\nRegisters: \n",
            self.chr_offsets[7], self.chr_inversion
        );
        write!(
            out,
            "[0]:{:02X} [1]:{:02X} [2]:{:02X} [3]:{:02X} [4]:{:02X} [5]:{:02X}\n",
            self.registers[0],
            self.registers[1],
            self.registers[2],
            self.registers[3],
            self.registers[4],
            self.registers[5],
        );

        write!(
            out,
            "[6]:{:02X} [7]:{:02X}\n\n",
            self.registers[6], self.registers[7],
        );

        write!(
            out,
            "IRQ Latch: {}\nIRQ Counter: {}\nIRQ Enable: {}\nIRQ Reload: {}\nIRQ Pending: {}",
            self.irq_latch,
            self.irq_counter,
            self.irq_enabled,
            self.irq_reload,
            self.irq_is_pending
        );
        write!(
            out,
            "\n\nRam enabled: {}, Ram enable write: {}\n",
            self.prg_ram_enable, self.prg_ram_allow_write
        );
        true
    }
}

impl MMC3 {
    fn update_offsets(&mut self) {
        let n_prg_banks = self.prg_rom.len() / PRG_WINDOW_SIZE;
        let n_chr_banks = self.chr_rom.len() / CHR_WINDOW_SIZE;

        let prg_mask = n_prg_banks - 1;
        let chr_mask = n_chr_banks - 1;

        // PRG WINDOWS
        self.prg_offsets[1] = (self.registers[7] & prg_mask) * PRG_WINDOW_SIZE;
        self.prg_offsets[3] = (n_prg_banks.saturating_sub(1) & prg_mask) * PRG_WINDOW_SIZE;

        let r6_off = (self.registers[6] & prg_mask) * PRG_WINDOW_SIZE;
        let stlast_bank_off = (n_prg_banks.saturating_sub(2) & prg_mask) * PRG_WINDOW_SIZE;

        (self.prg_offsets[0], self.prg_offsets[2]) = match self.bank_mode {
            true => (stlast_bank_off, r6_off),
            false => (r6_off, stlast_bank_off),
        };

        // CHR WINDOWS
        let r0_off_base = (self.registers[0] & !1 & chr_mask) * CHR_WINDOW_SIZE;
        let r1_off_base = (self.registers[1] & !1 & chr_mask) * CHR_WINDOW_SIZE;

        if !self.chr_inversion {
            self.chr_offsets[0] = r0_off_base;
            self.chr_offsets[1] = r0_off_base + CHR_WINDOW_SIZE;
            self.chr_offsets[2] = r1_off_base;
            self.chr_offsets[3] = r1_off_base + CHR_WINDOW_SIZE;

            self.chr_offsets[4] = (self.registers[2] & chr_mask) * CHR_WINDOW_SIZE;
            self.chr_offsets[5] = (self.registers[3] & chr_mask) * CHR_WINDOW_SIZE;
            self.chr_offsets[6] = (self.registers[4] & chr_mask) * CHR_WINDOW_SIZE;
            self.chr_offsets[7] = (self.registers[5] & chr_mask) * CHR_WINDOW_SIZE;
        } else {
            self.chr_offsets[0] = (self.registers[2] & chr_mask) * CHR_WINDOW_SIZE;
            self.chr_offsets[1] = (self.registers[3] & chr_mask) * CHR_WINDOW_SIZE;
            self.chr_offsets[2] = (self.registers[4] & chr_mask) * CHR_WINDOW_SIZE;
            self.chr_offsets[3] = (self.registers[5] & chr_mask) * CHR_WINDOW_SIZE;

            self.chr_offsets[4] = r0_off_base;
            self.chr_offsets[5] = r0_off_base + CHR_WINDOW_SIZE;
            self.chr_offsets[6] = r1_off_base;
            self.chr_offsets[7] = r1_off_base + CHR_WINDOW_SIZE;
        }

        #[cfg(feature = "log")]
        {
            if self.log_enable {
                log::info!("Banks swapped");
            }
        }
    }
}
