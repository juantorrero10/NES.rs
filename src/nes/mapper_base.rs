use crate::nes::{
    InfoReq, MapperInfoReq, Nes, NesEvent, bus_cpu::{BusCpuChips, BusCpuTarget}, game_rom::MemoryMirroring, ppu::{BusPpuChips, BusPpuTarget}
};

pub trait Mapper {
    fn cpu_need_intercept(&mut self, addr: u16) -> bool;


    fn cpu_intercept<'a>(
        &'a mut self,
        addr: u16,
        chips: &'a mut BusCpuChips,
        data: u8,
        write: bool,
    ) -> BusCpuTarget<'a>;

    fn on_cpu_clock(&mut self) -> Option<NesEvent> { None }

    fn is_irq_pending(&self) -> bool {
        false
    }

    fn irq_clear(&mut self) {}

    fn ppu_need_intercept(&mut self, addr: u16) -> bool;
    
    fn ppu_intercept<'a>(
        &'a mut self,
        addr: u16,
        chips: &'a mut BusPpuChips,
        data: u8,
        write: bool,
    ) -> BusPpuTarget<'a>;

    fn on_ppu_clock(&mut self) -> Option<NesEvent> { None }

    fn mirroring(&self) -> MemoryMirroring;

    fn info_request_response(&mut self, _i: MapperInfoReq, _data_back: Option<u64>) {}

    fn debug_information(&self, _out: &mut String) -> bool { false }
}