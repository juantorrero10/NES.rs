use std::{collections::HashMap, ffi::OsString, ops::SubAssign};

use crate::{SCREEN_SIZE, nes::cpu::RegFile6502};


pub struct FrameState {
    pub show_press_start_msg: bool,
    pub dev_mode: bool,
    pub cpu_info: CpuInfo,
    pub dissasembly: Dissassebly,
    pub memory_editor: MemEdit,
    pub ppu_info: PpuInfo,
    pub file_action: FileAction,
    pub audio: AudioSettings,
    pub video: VideoSettings,
}

#[derive(Default)]
pub enum FileAction {
    #[default]
    None,
    LoadROM(OsString),
    LoadSRAM(OsString),
    StoreSRAM,
    TakeSnapshot(OsString),
    LoadSnapshot(OsString),
}

#[derive(Clone)]
pub struct AudioSettings {
    pub volume: f32,
    pub pw1_mut: bool,
    pub pw2_mut: bool,
    pub trg_mut: bool,
    pub nch_mut: bool,
    pub dmc_mut: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            volume: 1.0,
            pw1_mut: false,
            pw2_mut: false,
            trg_mut: false,
            nch_mut: false,
            dmc_mut: false,
        }
    }
}

#[derive(Default)]
pub struct VideoSettings {
    pub hide_overscan_region: bool,
}

pub struct ChrBanks(pub [u8; 128 * 128 * 4], pub [u8; 128 * 128 * 4]);

impl Default for ChrBanks {
    fn default() -> Self {
        Self([0u8; 128 * 128 * 4], [0u8; 128 * 128 * 4])
    }
}

pub struct Nametables(pub [u8; 256 * 240 * 4 * 4]);

impl Default for Nametables {
    fn default() -> Self {
        Self([0u8; 256 * 240 * 4 * 4])
    }
}

#[derive(Default)]
pub struct PpuInfo {
    pub chr_banks: ChrBanks,
    pub palettes: [[[u8; 4]; 4]; 8],
    pub nametables: Nametables,
    pub selected_palette: u16,
    pub ppu_regs: String,
    pub toggle_mapper: bool,
    pub display_mapper_info: bool,
    pub mapper_info: String,
}

impl PpuInfo {
    fn initialize(&mut self) {
        self.chr_banks.0.fill(0);
        self.chr_banks.1.fill(0);
        self.palettes.iter_mut().for_each(|p| p.fill([0u8; 4]));
        self.selected_palette = 0;
        self.ppu_regs = String::with_capacity(128);
        self.mapper_info = String::with_capacity(512);
        self.toggle_mapper = false;
        self.display_mapper_info = false;
    }
}

#[derive(Default)]
pub struct Dissassebly {
    pub update: bool,
    pub text: String,
    pub mapping: HashMap<u16, (usize, usize, u16)>,
    pub bkpt: Option<u16>,
    pub options: DissOptions
}

pub struct DissOptions {
    pub focus_pc: bool,
    pub from: u16,
    pub to: u16,
    pub show: u16,
}

impl Default for DissOptions {
    fn default() -> Self {
        Self {
            focus_pc: true,
            from: 0,
            to: 200,
            show: 200,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum MemPage {
    Cpu(u8),
    Ppu(u8)
}

impl MemPage {
    pub fn dec(&mut self)  {
        let p = match self {
            MemPage::Cpu(p) => p,
            MemPage::Ppu(p) => p,
        };
        *p -= 1;
    }
    pub fn inc(&mut self)  {
        let p = match self {
            MemPage::Cpu(p) => p,
            MemPage::Ppu(p) => p,
        };
        *p += 1;
    }
}

pub struct MemEdit {
    pub paged_changed: bool,
    pub current_page: MemPage,
    pub search_byte_highlight: Option<u8>,
    pub modify_byte: Option<(u16, u8)>,
    pub page_data: [Option<u8>; 256],
}

impl Default for MemEdit {
    fn default() -> Self {
        Self {
            paged_changed: false,
            current_page: MemPage::Cpu(0),
            search_byte_highlight: None,
            modify_byte: None,
            page_data: [None; 256],
        }
    }
}

#[derive(Default)]
pub struct CpuInfo {
    pub regs: RegFile6502,
    pub cycles_left: usize,
    pub global: usize,
}

impl FrameState {
    #[inline]
    pub fn clear(&mut self) {
        self.dissasembly.update = false;
        self.file_action = FileAction::None;
        self.memory_editor.modify_byte = None;
        self.memory_editor.paged_changed = false;
    }

    #[inline]
    pub fn clear_debug(&mut self) {
        self.dissasembly.text.clear();
        self.dissasembly.mapping.clear();
        self.ppu_info.ppu_regs.clear();
        self.ppu_info.mapper_info.clear();
    }

    // It is a very heavy type, will likely trigger a stack overflow if not boxed.
    pub fn new_boxed() -> Box<Self> {
        let mut ret = Box::<Self>::new_uninit();
        unsafe {
            let ptr = ret.as_mut_ptr();
            std::ptr::write(&mut (*ptr).ppu_info,  PpuInfo::default());
            std::ptr::write(&mut (*ptr).cpu_info, CpuInfo::default());
            std::ptr::write(&mut (*ptr).memory_editor, MemEdit::default());
            std::ptr::write(&mut (*ptr).file_action, FileAction::default());
            std::ptr::write(&mut (*ptr).audio, AudioSettings::default());
            std::ptr::write(&mut (*ptr).video, VideoSettings::default());
            std::ptr::write(&mut (*ptr).dev_mode, false);
            std::ptr::write(&mut (*ptr).dissasembly, Dissassebly::default());
            std::ptr::write(&mut (*ptr).show_press_start_msg, true);
            ret.assume_init()
        }

    }
}


