use crate::nes::{
    Nes, bus_cpu::{BusCpuChips, BusCpuTarget}, ppu::{ADDR_CHR_END, BusPpuChips, BusPpuTarget},
    game_rom::PpuNameTblMirroring
};

pub mod nrom;
pub mod mmc3;
pub mod mmc1;
pub mod uxrom;
pub mod cnrom;
pub mod axrom;

use crate::nes::{NesEvent};

pub trait Mapper{
    fn cpu_need_intercept(&mut self, addr: u16) -> bool;

    fn cpu_intercept<'a>(
        &'a mut self,
        addr: u16,
        chips: &'a mut BusCpuChips,
        data: u8,
        write: bool,    
    ) -> BusCpuTarget<'a>;

    fn on_cpu_clock(&mut self, _nes: *mut Nes) -> Option<NesEvent> {
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

    fn on_ppu_clock(&mut self, _nes: *mut Nes) -> Option<NesEvent> {
        None
    }

    fn mirroring(&self) -> PpuNameTblMirroring;

    //fn info_request_response(&mut self, _i: MapperInfoReq, _data_back: Option<u64>) {}

    fn debug_information(&self, _out: &mut String) -> bool {
        false
    }

    fn is_sram_battery_backed(&self) -> bool { false }

    fn load_sram(&mut self, _buff: &[u8]) {}

    fn save_sram(&self) -> Option<&[u8]> { None }

    fn get_snapshot_size(&self) -> usize;

    fn take_snapshot(&self, buff: &mut [u8]) -> Result<(), postcard::Error>;

    #[cfg(feature = "log")]
    fn toggle_log(&mut self) {}
}
