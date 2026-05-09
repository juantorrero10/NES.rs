use std::{cell::RefCell, fmt::Write, ptr::addr_of};

use thiserror::Error;

use crate::nes::{
    bus_cpu::{BusCpu, BusCpuError},
    cpu::{Cpu, CpuError},
    instruction_set::addressing::AddrMode,
};

#[derive(Error, Debug)]
pub enum InstructionError {
    #[error("CPU fail: {0}")]
    CpuError(#[from] CpuError),
    #[error("Bus fail: {0}")]
    BusCpuError(#[from] BusCpuError),
    #[error("FMT error: {0}")]
    FmtError(#[from] std::fmt::Error),
}

type Result<T> = std::result::Result<T, InstructionError>;

#[derive(Clone, Copy)]
pub struct Instruction6502 {
    pub n_cycles: u8,
    pub pneumonic: &'static str,
    pub addressing_handler: addressing::AddrMode,
    pub instruction_handler: fn(&mut Cpu) -> Result<bool>,
}

const DEF: Instruction6502 = Instruction6502 {
    n_cycles: 2,
    pneumonic: "???",
    addressing_handler: IMP,
    instruction_handler: instructions::nop,
};

impl Default for Instruction6502 {
    fn default() -> Self {
        DEF
    }
}

impl Instruction6502 {
    fn get_size(opcode: u8) -> usize {
        match INSTRUCTION_LUT[opcode as usize].addressing_handler {
            IMP => 1,
            IMM => 2,
            REL => 2,
            ABS => 3,
            ABX => 3,
            ABY => 3,
            ZP0 => 2,
            ZPX => 2,
            ZPY => 2,
            IZX => 2,
            IZY => 2,
            IND => 3,
        }
    }

    #[inline]
    fn get_pneumonic(opcode: u8) -> &'static str {
        INSTRUCTION_LUT[opcode as usize].pneumonic
    }

    fn fmt_push_args(
        arg_start: u16,
        a: AddrMode,
        bus: &mut BusCpu,
        out: &mut String,
    ) -> Result<()> {
        match a {
            IMM => {
                let imm = bus.read(arg_start)?;
                write!(out, "#{}", imm)?;
                Ok(())
            }
            ABS => {
                let addr = bus.read16(arg_start)?;
                write!(out, "${:04X}", addr)?;
                Ok(())
            }
            REL => {
                let addr = bus.read(arg_start)? as i8 as i16;
                let sum = arg_start + 1;
                let addr: u16 = sum.wrapping_add_signed(addr);
                write!(out, "${:04X}", addr)?;
                Ok(())
            }
            ABX => {
                let addr = bus.read16(arg_start)?;
                write!(out, "${:04X}, X", addr)?;
                Ok(())
            }
            ABY => {
                let addr = bus.read16(arg_start)?;
                write!(out, "${:04X}, Y", addr)?;
                Ok(())
            }
            ZP0 => {
                let addr = bus.read(arg_start)? as u16;
                write!(out, "${:04X}", addr)?;
                Ok(())
            }
            ZPX => {
                let addr = bus.read(arg_start)? as u16;
                write!(out, "${:04X}, X", addr)?;
                Ok(())
            }
            ZPY => {
                let addr = bus.read(arg_start)? as u16;
                write!(out, "${:04X}, Y", addr)?;
                Ok(())
            }
            IND => {
                let addr = bus.read16(arg_start)?;
                write!(out, "(${:04X})", addr)?;
                Ok(())
            }
            IZX => {
                let addr = bus.read(arg_start)? as u16;
                write!(out, "(${:04X}, X)", addr)?;
                Ok(())
            }
            IZY => {
                let addr = bus.read(arg_start)? as u16;
                write!(out, "(${:04X}), Y", addr)?;
                Ok(())
            }
            IMP => Ok(()),
        }
    }

    pub fn dissasemble(ip: u16, bus: &mut BusCpu, out: &mut String) -> Result<usize> {
        write!(out, "${:04X}:   ", ip)?;
        let opcode = bus.read(ip)?;
        write!(out, "{:02X}", opcode)?;
        let extra = Self::get_size(opcode) - 1;
        for i in 0..2 {
            if i < extra && let Ok(b) = bus.read(ip + 1 + (i as u16)) {
                write!(out, " {:02X}", b)?;
            } else {
                out.push_str("   ");
            }
        }
        write!(out, "   {} ", Self::get_pneumonic(opcode))?;
        Self::fmt_push_args(
            ip + 1,
            INSTRUCTION_LUT[opcode as usize].addressing_handler,
            bus,
            out,
        )?;
        out.push('\n');
        Ok(extra + 1)
    }
}

use addressing::AddrMode::*;
use instructions::*;

pub static INSTRUCTION_LUT: [Instruction6502; 256] = {
    macro_rules! ins {
        ($pneumonic:expr, $instruction_handler:expr, $addressing_handler:expr, $n_cycles:expr) => {
            Instruction6502 {
                n_cycles: $n_cycles,
                pneumonic: $pneumonic,
                addressing_handler: $addressing_handler,
                instruction_handler: $instruction_handler,
            }
        };
    }
    let mut table = [DEF; 256];
    table[0x18] = ins!("CLC", clc, IMP, 2);
    table[0x86] = ins!("STX", stx, ZP0, 3);
    table[0x6D] = ins!("ADC", adc, ABS, 4);
    table[0x96] = ins!("STX", stx, ZPY, 4);
    table[0x88] = ins!("DEY", dey, IMP, 2);
    table[0x8D] = ins!("STA", sta, ABS, 4);
    table[0x8E] = ins!("STX", stx, ABS, 4);
    table[0xA0] = ins!("LDY", ldy, IMM, 2);
    table[0xA2] = ins!("LDX", ldx, IMM, 2);
    table[0xA4] = ins!("LDY", ldy, ZP0, 3);
    table[0xA9] = ins!("LDA", lda, IMM, 2);
    table[0xAC] = ins!("LDY", ldy, ABS, 4);
    table[0xB4] = ins!("LDY", ldy, ZPX, 4);
    table[0xBC] = ins!("LDY", ldy, ABX, 4);
    table[0xD0] = ins!("BNE", bne, REL, 2);
    table[0xEA] = ins!("NOP", nop, IMP, 2);
    table
};

macro_rules! handler {
    ($fn_name:ident, $c:ident, $body:block) => {
        #[inline(always)]
        #[allow(unused)]
        pub fn $fn_name($c: &mut Cpu) -> Result<bool> {
            $body
        }
    };
}

pub mod addressing {
    use super::*;
    use crate::bus;

    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum AddrMode {
        IMM,
        IMP,
        REL,
        ABS,
        ABX,
        ABY,
        ZP0,
        ZPX,
        ZPY,
        IND,
        IZX,
        IZY,
    }

    impl AddrMode {
        pub fn get_fptr(&self) -> fn(&mut Cpu) -> Result<bool> {
            match self {
                Self::IMM => imm,
                Self::IMP => imp,
                Self::ABS => abs,
                Self::REL => rel,
                Self::ABX => abx,
                Self::ABY => aby,
                Self::ZP0 => zp0,
                Self::ZPX => zpx,
                Self::ZPY => zpy,
                Self::IND => ind,
                Self::IZX => izx,
                Self::IZY => izy,
            }
        }
    }

    handler!(imp, c, {
        c.state.fetched = c.registers.a;
        Ok(false)
    });

    handler!(imm, c, {
        c.state.addr_abs = c.registers.pc;
        c.registers.pc += 1;
        Ok(false)
    });

    handler!(abs, c, {
        c.state.addr_abs = bus!(c).read16(c.registers.pc)?;
        c.registers.pc += 2;
        Ok(false)
    });

    handler!(rel, c, {
        c.state.addr_rel = bus!(c).read(c.registers.pc)? as i8 as i16;
        c.registers.pc += 1;
        let sum = c.registers.pc;
        c.state.addr_abs = sum.wrapping_add_signed(c.state.addr_rel);
        // Add 1 CPU cycle if boundary crossed
        Ok(false)
    });

    handler!(ind, c, {
        let ptr = bus!(c).read16(c.registers.pc)?;
        c.registers.pc += 2;
        // Simulate 6502 page boundary bug
        // This behaviour is incorrect on purpose
        if (ptr & 0x00FF) == 0x00FF {
            c.state.addr_abs =
                ((bus!(c).read(ptr & 0xFF00)? as u16) << 8) | bus!(c).read(ptr)? as u16;
        } else {
            // Correct behaviour
            c.state.addr_abs = bus!(c).read16(ptr)?;
        }
        Ok(false)
    });

    handler!(abx, c, {
        c.state.addr_abs = bus!(c).read16(c.registers.pc)?;
        c.registers.pc += 2;
        let old_page = c.state.addr_abs >> 8;
        c.state.addr_abs += c.registers.x as u16;
        // Additional cycle if pagechange
        if old_page != c.state.addr_abs >> 8 {
            return Ok(true);
        }
        Ok(false)
    });

    handler!(aby, c, {
        c.state.addr_abs = bus!(c).read16(c.registers.pc)?;
        c.registers.pc += 2;
        let old_page = c.state.addr_abs >> 8;
        c.state.addr_abs += c.registers.y as u16;
        // Additional cycle if pagechange
        if old_page != c.state.addr_abs >> 8 {
            return Ok(true);
        }
        Ok(false)
    });

    handler!(zp0, c, {
        c.state.addr_abs = bus!(c).read(c.registers.pc)? as u16;
        c.registers.pc += 1;
        c.state.addr_abs &= 0x00FF;
        Ok(false)
    });

    handler!(zpx, c, {
        c.state.addr_abs = (bus!(c).read(c.registers.pc)? + c.registers.x) as u16;
        c.registers.pc += 1;
        c.state.addr_abs &= 0x00FF;
        Ok(false)
    });

    handler!(zpy, c, {
        c.state.addr_abs = (bus!(c).read(c.registers.pc)? + c.registers.y) as u16;
        c.registers.pc += 1;
        c.state.addr_abs &= 0x00FF;
        Ok(false)
    });

    handler!(izx, c, {
        let mut ptr = (bus!(c).read(c.registers.pc)? + c.registers.x) as u16;
        c.registers.pc += 1;
        ptr += c.registers.x as u16;
        let lo = bus!(c).read(ptr & 0x00FF)? as u16;
        let hi = bus!(c).read((ptr + 1) & 0x00FF)? as u16;

        c.state.addr_abs = (hi << 8 | lo) as u16;
        Ok(false)
    });

    handler!(izy, c, {
        let mut ptr = (bus!(c).read(c.registers.pc)?) as u16;
        c.registers.pc += 1;
        ptr += c.registers.x as u16;
        let lo = bus!(c).read(ptr & 0x00FF)? as u16;
        let hi = bus!(c).read((ptr + 1) & 0x00FF)? as u16;

        c.state.addr_abs = (hi << 8 | lo) as u16;
        let old_page = c.state.addr_abs >> 8;
        c.state.addr_abs += c.registers.y as u16;
        if old_page != c.state.addr_abs >> 8 {
            return Ok(true);
        }
        Ok(false)
    });
}

pub mod instructions {
    use crate::{bus, nes::cpu::Flag6502};

    use super::*;

    pub fn nop(c: &mut Cpu) -> Result<bool> {
        match c.state.last_opcode {
            0x1C | 0x3C | 0x5C | 0x7C | 0xDC | 0xFC => Ok(true),
            _ => Ok(false),
        }
    }

    pub fn ldx(c: &mut Cpu) -> Result<bool> {
        c.registers.x = c.fetch()?;
        c.set_flag(Flag6502::Z, c.registers.x == 0);
        c.set_flag(Flag6502::N, (c.registers.x & 0x80) != 0);
        Ok(false)
    }

    handler!(ldy, c, {
        c.registers.y = bus!(c).read(c.state.addr_abs)?;
        c.set_flag(Flag6502::Z, c.registers.y == 0);
        c.set_flag(Flag6502::N, (c.registers.y & 0x80) != 0);
        Ok(false)
    });

    handler!(lda, c, {
        c.registers.a = bus!(c).read(c.state.addr_abs)?;
        c.set_flag(Flag6502::Z, c.registers.y == 0);
        c.set_flag(Flag6502::N, (c.registers.y & 0x80) != 0);
        Ok(false)
    });

    handler!(stx, c, {
        bus!(c).write(c.state.addr_abs, c.registers.x);
        Ok(false)
    });

    handler!(clc, c, {
        c.set_flag(Flag6502::C, false);
        Ok(false)
    });

    handler!(adc, c, {
        let b = bus!(c).read(c.state.addr_abs)?;
        let result: u16 = (c.registers.a + b + c.get_flag(Flag6502::C) as u8) as u16;
        c.set_flag(Flag6502::C, result > 0xFF);
        c.set_flag(Flag6502::Z, result == 0);
        c.set_flag(
            Flag6502::V,
            ((result ^ c.registers.a as u16) & (result ^ b as u16) & 0x0080) != 0,
        );
        c.set_flag(Flag6502::N, (result & 0x80) != 0);
        c.registers.a = (result & 0x00FF) as u8;
        Ok(false)
    });

    handler!(dey, c, {
        c.registers.y -= 1;
        c.set_flag(Flag6502::Z, c.registers.y == 0);
        c.set_flag(Flag6502::N, (c.registers.y & 0x80) != 0);
        Ok(false)
    });

    handler!(bne, c, {
        if c.get_flag(Flag6502::Z) != false {
            return Ok(false);
        }
        // Add one more if page crossed.
        if (c.registers.pc >> 8) != (c.state.addr_abs >> 8) {
            c.state.cycles_left += 1;
        }
        c.registers.pc = c.state.addr_abs;
        Ok(true)
    });

    handler!(sta, c, {
        bus!(c).write(c.state.addr_abs, c.registers.a);
        Ok(false)
    });
}
