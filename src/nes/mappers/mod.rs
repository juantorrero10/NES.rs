use std::ffi::OsString;

use crate::nes::{
    bus_cpu::{BusCpuChips, BusCpuTarget},
    game_rom::MemoryMirroring,
    ppu::{ADDR_CHR_END, BusPpuChips, BusPpuTarget},
};

pub mod mmc0;
pub mod mmc3;

use crate::nes::{MapperInfoReq, NesEvent};

pub trait Mapper {
    fn cpu_need_intercept(&mut self, addr: u16) -> bool;

    fn cpu_intercept<'a>(
        &'a mut self,
        addr: u16,
        chips: &'a mut BusCpuChips,
        data: u8,
        write: bool,
    ) -> BusCpuTarget<'a>;

    fn on_cpu_clock(&mut self) -> Option<NesEvent> {
        None
    }

    fn is_irq_pending(&self) -> bool {
        false
    }

    fn irq_clear(&mut self) {}

    fn ppu_need_intercept(&mut self, addr: u16, debug_mode: bool) -> bool;

    fn ppu_intercept<'a>(
        &'a mut self,
        addr: u16,
        chips: &'a mut BusPpuChips,
        data: u8,
        write: bool,
    ) -> BusPpuTarget<'a>;

    fn on_ppu_clock(&mut self) -> Option<NesEvent> {
        None
    }

    fn mirroring(&self) -> MemoryMirroring;

    fn info_request_response(&mut self, _i: MapperInfoReq, _data_back: Option<u64>) {}

    fn debug_information(&self, _out: &mut String) -> bool {
        false
    }

    fn is_sram_battery_backed(&self) -> bool { false }

    fn load_sram(&mut self, _buff: &[u8]) {}

    fn save_sram(&self) -> Option<&[u8]> { None }

    #[cfg(feature = "log")]
    fn toggle_log(&mut self) {}
}
