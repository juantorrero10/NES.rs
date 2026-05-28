use std::cell::RefCell;
use std::fmt::Debug;
use std::rc::Rc;

use colored::Colorize;
use thiserror::Error;

use crate::nes::bus_cpu::{BusCpu, BusCpuError};
use crate::nes::instruction_set::{INSTRUCTION_LUT, addressing, instructions};
use crate::ui::framestate::{self, CpuInfo, FrameState};

const STACK_TOP: u16 = 0x0100;
const STACK_BOTTOM: u16 = 0x01FF;

const RESET_VEC: u16 = 0xFFFC;
const IRQ_VEC: u16 = 0xFFFE;
const NMI_VEC: u16 = 0xFFFA;

#[derive(Clone, Copy)]
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
            sr: 0x00 | Flag6502::U as u8,
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
#[derive(Default)]
pub struct State6502 {
    pub addr_abs: u16,
    pub addr_rel: i16,
    pub cycles_left: u8,
    pub fetched: u8,
    pub last_opcode: u8,
    pub clock_counter: u64,
}

pub struct Cpu {
    pub registers: RegFile6502,
    pub bus: Rc<RefCell<BusCpu>>,
    pub state: State6502,
}

#[derive(Error, Debug)]
pub enum CpuError {
    #[error("The main bus (the CPU Bus) failed: {0}")]
    BusCpuError(#[from] BusCpuError),
    #[error("{0}")]
    InstError(String),
}

type Result<T> = std::result::Result<T, CpuError>;

#[macro_export]
macro_rules! bus {
    ($self:expr) => {
        $self.bus.borrow_mut()
    };
}

impl Cpu {
    pub fn new(bus: Rc<RefCell<BusCpu>>) -> Self {
        Cpu {
            registers: RegFile6502::default(),
            bus,
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
    pub fn get_flag(&self, f: Flag6502) -> bool {
        (self.registers.sr & (f as u8)) != 0
    }

    #[inline(always)]
    pub fn fetch(&mut self) -> Result<u8> {
        if INSTRUCTION_LUT[self.state.last_opcode as usize].addressing_handler
            != addressing::AddrMode::IMP
        {
            self.state.fetched = bus!(self).read(self.state.addr_abs)?;
        }
        return Ok(self.state.fetched);
    }

    pub fn reset(&mut self) -> Result<()> {
        self.registers = RegFile6502::default();
        self.registers.pc = bus!(self).read16(RESET_VEC)?;

        self.state = State6502::default();
        self.state.cycles_left = 8;
        Ok(())
    }

    #[inline(always)]
    pub fn push(&mut self, data: u8) -> Result<()> {
        bus!(self).write(STACK_BOTTOM + self.registers.sp as u16, data)?;
        self.registers.sp -= 1;
        Ok(())
    }

    #[inline(always)]
    pub fn pull(&mut self) -> Result<u8> {
        self.registers.sp += 1;
        Ok(bus!(self).read(STACK_BOTTOM + self.registers.sp as u16)?)
    }

    pub fn clock(&mut self) -> Result<()> {
        if self.state.cycles_left != 0 {
            self.state.cycles_left -= 1;
            self.state.clock_counter += 1;
            return Ok(());
        }

        // Load up a new instruction
        self.set_flag(Flag6502::U, true);
        self.state.last_opcode = bus!(self).read(self.registers.pc)?;
        self.registers.pc += 1;

        let ins = INSTRUCTION_LUT[self.state.last_opcode as usize];

        self.state.cycles_left = ins.n_cycles;

        // Execute addressing handler
        // Additional clock cycle if needed
        self.state.cycles_left += ((ins.addressing_handler.get_fptr())(self).map_err(|e| {
            log::error!(
                "Failure on addressing handler: opc 0x{:02X}",
                self.state.last_opcode
            );
            return CpuError::InstError(format!("{} => {}", ins.pneumonic, e));
        }))? as u8;
        // Execute instruction
        self.state.cycles_left += (ins.instruction_handler)(self).map_err(|e| {
            log::error!(
                "Failure on instruction_handler handler: opc 0x{:02X}",
                self.state.last_opcode
            );
            return CpuError::InstError(format!("{} => {}", ins.pneumonic, e));
        })? as u8;

        self.set_flag(Flag6502::U, true);

        self.state.cycles_left -= 1;
        self.state.clock_counter += 1;
        Ok(())
    }

    // -- Interrupt logic --
    #[inline]
    fn interrupt_save_state(&mut self) -> Result<()> {
        self.push((self.registers.pc >> 8) as u8)?;
        self.push((self.registers.pc & 0x00FF) as u8)?;

        self.set_flag(Flag6502::I, true);
        
        let pushed_sr = self.registers.sr | Flag6502::U as u8 & !(Flag6502::B as u8);
        self.push(pushed_sr)?;

        Ok(())
    }

    #[inline(always)]
    pub fn irq(&mut self) -> Result<()> {
        // If they are disabled (I: Interrupt enable [Set low]).
        if self.get_flag(Flag6502::I) {
            return Ok(());
        }
        self.interrupt_save_state()?;
        let addr = bus!(self).read16(IRQ_VEC)?;
        self.registers.pc = addr;
        Ok(())
    }

    #[inline(always)]
    pub fn nmi(&mut self) -> Result<()> {
        self.interrupt_save_state()?;
        let addr = bus!(self).read16(NMI_VEC)?;
        self.registers.pc = addr;
        Ok(())
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
