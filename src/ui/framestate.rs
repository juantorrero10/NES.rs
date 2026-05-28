use std::collections::HashMap;

use crate::nes::cpu::RegFile6502;

pub struct FrameState<'a> {
    pub screen: GameScreen,
    pub cpu_info: CpuInfo,
    pub dissasembly: Dissassebly<'a>,
    pub memory_editor: MemEdit<'a>,
    pub ppu_info: PpuInfo<'a>,
}

pub struct PpuInfo<'a> {
    pub chr_bank_0: &'a [u8],
    pub chr_bank_1: &'a [u8],
    pub palettes: [[[u8; 4]; 4]; 8],
    pub selected_palette: u16,
}
pub struct GameScreen {
    pub update: bool,
    pub video_output: (*const u8, usize),
}

pub struct Dissassebly<'a> {
    pub update: bool,
    pub text: &'a str,
    pub mapping: &'a HashMap<u16, (usize, usize, u16)>,
}

pub struct MemEdit<'a> {
    pub paged_changed: bool,
    pub current_page: u8,
    pub search_byte_highlight: Option<u8>,
    pub page_data: &'a [Option<u8>],
}

pub struct CpuInfo {
    pub regs: RegFile6502,
    pub cycles_left: usize,
    pub global: usize,
}

pub struct PersistentFrameState {
    pub mem_edit_current_page: u8,
    pub mem_edit_search_byte_highlight: Option<u8>,
    pub mem_edit_page_changed: bool,
}

impl PersistentFrameState {
    pub fn save_persistent_info(fs: FrameState) -> Self {
        Self {
            mem_edit_current_page: fs.memory_editor.current_page,
            mem_edit_search_byte_highlight: fs.memory_editor.search_byte_highlight,
            mem_edit_page_changed: fs.memory_editor.paged_changed
        }
    }
}

impl Default for PersistentFrameState {
    fn default() -> Self {
        Self {
            mem_edit_page_changed: true,
            mem_edit_current_page: 0,
            mem_edit_search_byte_highlight: None,
        }
    }
}
