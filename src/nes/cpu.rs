use std::error::Error;
use std::fmt::{Debug, Write};

use colored::Colorize;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::nes::mappers::Mapper;
use crate::nes::{Nes, NesError};
use crate::nes::bus_cpu::{BusCpu, BusCpuError};
use crate::nes::instruction_set::{
    INSTRUCTION_LUT, Instruction6502, addressing,
};
use crate::ui::framestate::{CpuInfo};

const STACK_TOP: u16 = 0x0100;
const STACK_BOTTOM: u16 = 0x01FF;

const RESET_VEC: u16 = 0xFFFC;
const IRQ_VEC: u16 = 0xFFFE;
const NMI_VEC: u16 = 0xFFFA;

#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct RegFile6502 {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub sr: u8,
    pub pc: u16,
}

impl Default for RegFile6502 {
    fn default() -> Self {
        RegFile6502 {
            a: 0,
            x: 0,
            y: 0,
            sp: 0xFD,
            sr: 0x00 | Flag6502::U as u8 | Flag6502::I as u8,
            pc: 0,
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy)]
pub enum Flag6502 {
    C = 1,        //Carry
    Z = (1 << 1), //Zero
    I = (1 << 2), //Interrupt Enable (LOW)
    D = (1 << 3), //Decimal
    B = (1 << 4), //Break
    U = (1 << 5), //Unused
    V = (1 << 6), //Overflow
    N = (1 << 7), //Negative
}

impl Into<&'static str> for Flag6502 {
    fn into(self) -> &'static str {
        match self {
            Flag6502::C => "C",
            Flag6502::Z => "Z",
            Flag6502::I => "I",
            Flag6502::D => "D",
            Flag6502::B => "B",
            Flag6502::U => "U",
            Flag6502::V => "V",
            Flag6502::N => "N",
        }
    }
}

impl Flag6502 {
    pub fn reg_has(&self, reg: u8) -> bool {
        return ((*self as u8) & reg) != 0;
    }

    fn display_one(&self, reg: u8, f: &mut std::fmt::Formatter<'_>) {
        let c: &str = (*self).into();
        let enable = self.reg_has(reg);
        if enable {
            write!(f, "{}", c.green());
        } else {
            write!(f, "{}", c.red());
        }
    }
}

// Addtiional state that the CPU works with but they are not part of the official "Registers"
#[derive(Default, Deserialize, Serialize)]
pub struct State6502 {
    pub addr_abs: u16,
    pub addr_rel: i16,
    pub cycles_left: u8,
    pub fetched: u8,
    pub last_opcode: u8,
    pub clock_counter: u64,
}

#[derive(Serialize, Deserialize)]
pub struct Cpu {
    pub registers: RegFile6502,
    pub state: State6502,
    #[cfg(feature = "log")]
    pub log: bool,
}

#[derive(Error, Debug)]
pub enum CpuError {
    #[error("The main bus (the CPU Bus) failed: {0}")]
    IOError(String),
    #[error("{0}")]
    InstError(String),
}

macro_rules! nes {
    ($nes_ptr:expr) => {
        unsafe {
            ($nes_ptr.as_mut().unwrap())
        }
    };
}

type Result<T> = std::result::Result<T, Box<dyn Error>>;

impl Cpu {
    pub fn new() -> Self {
        Cpu {
            registers: RegFile6502::default(),
    #       [cfg(feature = "log")]
            log: false,
            state: State6502::default(),
        }
    }


    #[inline(always)]
    pub fn set_flag(&mut self, f: Flag6502, b: bool) {
        if b {
            self.registers.sr |= f as u8;
        } else {
            self.registers.sr &= !(f as u8);
        }
    }

    #[inline(always)]
    pub fn read(&self, addr: u16, nes: *mut Nes) -> u8 {
        nes!(nes).read(addr).unwrap()
    }


    #[inline(always)]
    pub fn write(&self, addr: u16, data: u8, nes: *mut Nes) {
        nes!(nes).write(addr, data).unwrap()
    }

    #[inline(always)]
    pub fn read16(&self, addr: u16, nes: *mut Nes) -> u16 {
        nes!(nes).read16(addr).unwrap()
    }


    #[inline(always)]
    pub fn get_flag(&self, f: Flag6502) -> bool {
        (self.registers.sr & (f as u8)) != 0
    }

    #[inline(always)]
    pub fn fetch(&mut self, nes: *mut Nes) -> u8 {
        if INSTRUCTION_LUT[self.state.last_opcode as usize].addressing_handler
            != addressing::AddrMode::IMP
        {
            self.state.fetched = self.read(self.state.addr_abs, nes);
        }
        return self.state.fetched;
    }

    #[inline(always)]
    pub fn get_pc(&self) -> u16 {
        self.registers.pc
    }

    pub fn reset(&mut self, nes: *mut Nes) {
        self.registers = RegFile6502::default();
        self.registers.pc = self.read16(RESET_VEC, nes);
        self.set_flag(Flag6502::U, true);

        self.state = State6502::default();
        self.state.cycles_left = 7;
    }

    #[inline(always)]
    pub fn push(&mut self, data: u8, nes: *mut Nes) {
        nes!(nes).write(STACK_TOP + self.registers.sp as u16, data);
        self.registers.sp -= 1;
    }

    #[inline(always)]
    pub fn pull(&mut self, nes: *mut Nes) -> u8 {
        self.registers.sp += 1;
        self.read(STACK_TOP + self.registers.sp as u16, nes)
    }

    pub fn clock(&mut self, _: Option<&mut String>, nes: *mut Nes, mapper: &mut Box<dyn Mapper>) {
        
        if let Some(e) = mapper.on_cpu_clock() {
            let instance = unsafe { nes.as_mut().unwrap() };
            instance.trigger_event(e);
        }

        if mapper.is_irq_pending() && !self.get_flag(Flag6502::I){
            self.irq(nes);
            mapper.irq_clear();
        }

        // let _span = tracy_client::span!("Clock CPU");
        if self.state.cycles_left != 0 {
            self.state.cycles_left -= 1;
            self.state.clock_counter += 1;
            return;
        }

        // Load up a new instruction
        self.set_flag(Flag6502::U, true);


        /*if let Some(out) = log {
            Instruction6502::dissasemble(
                self.registers.pc,
                unsafe { nes.as_mut().unwrap() },
                out,
            )
            .unwrap();
            write!(
                out,
                "A:{:02X} X:{:02X} Y:{:02X} P:{:02X} SP:{:02X} CYC: {:02X}",
                self.registers.a,
                self.registers.x,
                self.registers.y,
                self.registers.sr,
                self.registers.sp,
                self.state.cycles_left,
            );
        }*/


        self.state.last_opcode = self.read(self.registers.pc, nes);
        self.registers.pc += 1;

        let ins = { 
            // let _span = tracy_client::span!("Decoding");
            INSTRUCTION_LUT[self.state.last_opcode as usize]
        };

        self.state.cycles_left = ins.n_cycles;

        let nes_ref = unsafe {
            // let _span = tracy_client::span!("Weird place");
            nes.as_mut().unwrap()
        };

        // Execute addressing handler
        // Additional clock cycle if needed
        let additional = {
            // let _span = tracy_client::span!("Addressing");
                ((ins.addressing_handler.get_fptr())(self, nes_ref).map_err(|e| {
                log::error!(
                    "Failure on addressing handler: opc 0x{:02X}",
                    self.state.last_opcode
                );
                return CpuError::InstError(format!("{} => {}", ins.pneumonic, e));
            })).unwrap() 
        };
        // Execute instruction
        let additional2 = {

            // let _span = tracy_client::span!("Instruction");
            (ins.instruction_handler)(self, nes_ref).map_err(|e| {
            log::error!(
                "Failure on instruction_handler handler: opc 0x{:02X}",
                self.state.last_opcode
            );
            return CpuError::InstError(format!("{} => {}", ins.pneumonic, e));
        }).unwrap() };

        if additional && additional2 {
            self.state.cycles_left += 1;
        }

        self.set_flag(Flag6502::U, true);

        self.state.cycles_left -= 1;
        self.state.clock_counter += 1;
    }

    // -- Interrupt logic --
    #[inline]
    fn interrupt_save_state(&mut self, nes: *mut Nes) {
        self.push((self.registers.pc >> 8) as u8, nes);
        self.push((self.registers.pc & 0x00FF) as u8, nes);

        let pushed_sr = (self.registers.sr | Flag6502::U as u8) & !(Flag6502::B as u8);
        
        self.set_flag(Flag6502::I, true);

        self.push(pushed_sr, nes);
    }

    #[inline(always)]
    pub fn irq(&mut self, nes: *mut Nes) {
        // If they are disabled (I: Interrupt enable [Set low]).
        if self.get_flag(Flag6502::I) {
            return;
        }
        
        self.interrupt_save_state(nes);
        let addr = self.read16(IRQ_VEC, nes);
        self.state.cycles_left = 7;
        self.registers.pc = addr;
        #[cfg(feature = "log")]
        {
            if self.log {
                log::info!("CPU IRQ! new pc => {}", self.registers.pc);
            }
        }
    }

    #[cfg(feature = "log")]
    pub fn toggle_log(&mut self) {
        self.log ^= true;
    }

    #[inline(always)]
    pub fn nmi(&mut self, nes: *mut Nes) {
        self.interrupt_save_state(nes);
        let addr = self.read16(NMI_VEC, nes);
        self.state.cycles_left = 7;
        self.registers.pc = addr;
    }
}
use Flag6502::*;

impl Debug for Cpu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let r = &self.registers;
        write!(
            f,
            "Registers: A: ${:02X}, X: ${:02X}, Y: ${:02X}, SP: ${:02X}, PC: ${:04X}, ",
            r.a, r.x, r.y, r.sp, r.pc
        )?;
        write!(f, "SR: ")?;
        for flag in [C, Z, I, D, B, U, V, N] {
            flag.display_one(self.registers.sr, f);
        }
        write!(
            f,
            "\nState: Cycles Left: {}, Total Cyles: {}",
            self.state.cycles_left, self.state.clock_counter
        )?;
        Ok(())
    }
}

impl Into<CpuInfo> for &Cpu {
    fn into(self) -> CpuInfo {
        CpuInfo {
            regs: self.registers,
            cycles_left: self.state.cycles_left as usize,
            global: self.state.clock_counter as usize,
        }
    }
}
