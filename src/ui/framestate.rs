use std::collections::HashMap;

use crate::nes::cpu::RegFile6502;

pub struct FrameState<'a> {
    pub screen: GameScreen,
    pub cpu_info: CpuInfo,
    pub dissasembly: Dissassebly<'a>,
}

pub struct GameScreen {
    pub update: bool,
}

pub struct Dissassebly<'a> {
    pub update: bool,
    pub text: &'a str,
    pub mapping: &'a HashMap<u16, (usize, usize, u16)>,
}

pub struct CpuInfo {
    pub regs: RegFile6502,
    pub cycles_left: usize,
    pub global: usize,
}
