use std::{error::Error, fmt::Write};

use crate::nes::{
    Nes, cpu::{Cpu}, instruction_set::addressing::AddrMode
};

/*#[derive(Error, Debug)]
pub enum InstructionError {
    #[error("Could not read/write: {0}")]
    IOoperationFailed(#[from] NesError),
    #[error("CPU fail: {0}")]
    CpuError(#[from] CpuError),
    #[error("Bus fail: {0}")]
    BusCpuError(#[from] BusCpuError),
    #[error("FMT error: {0}")]
    FmtError(#[from] std::fmt::Error),
}*/

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Clone, Copy)]
pub struct Instruction6502 {
    pub n_cycles: u8,
    pub pneumonic: &'static str,
    pub addressing_handler: addressing::AddrMode,
    pub instruction_handler: fn(&mut Cpu, &mut Nes) -> Result<bool>,
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
        nes: &mut Nes,
        out: &mut String,
    ) -> Result<()> {
        match a {
            IMM => {
                let imm = nes.debug_read(arg_start)?;
                write!(out, "#{}", imm)?;
                Ok(())
            }
            ABS => {
                let addr = nes.debug_read16(arg_start)?;
                write!(out, "${:04X}", addr)?;
                Ok(())
            }
            REL => {
                let addr = nes.debug_read(arg_start)? as i8 as i16;
                let sum = arg_start.wrapping_add(1);
                let addr: u16 = sum.wrapping_add_signed(addr);
                write!(out, "${:04X}", addr)?;
                Ok(())
            }
            ABX => {
                let addr = nes.debug_read16(arg_start)?;
                write!(out, "${:04X}, X", addr)?;
                Ok(())
            }
            ABY => {
                let addr = nes.debug_read16(arg_start)?;
                write!(out, "${:04X}, Y", addr)?;
                Ok(())
            }
            ZP0 => {
                let addr = nes.debug_read(arg_start)? as u16;
                write!(out, "${:04X}", addr)?;
                Ok(())
            }
            ZPX => {
                let addr = nes.debug_read(arg_start)? as u16;
                write!(out, "${:04X}, X", addr)?;
                Ok(())
            }
            ZPY => {
                let addr = nes.debug_read(arg_start)? as u16;
                write!(out, "${:04X}, Y", addr)?;
                Ok(())
            }
            IND => {
                let addr = nes.debug_read16(arg_start)?;
                write!(out, "(${:04X})", addr)?;
                Ok(())
            }
            IZX => {
                let addr = nes.debug_read(arg_start)? as u16;
                write!(out, "(${:04X}, X)", addr)?;
                Ok(())
            }
            IZY => {
                let addr = nes.debug_read(arg_start)? as u16;
                write!(out, "(${:04X}), Y", addr)?;
                Ok(())
            }
            IMP => Ok(()),
        }
    }

    pub fn dissasemble(ip: u16, nes: &mut Nes, out: &mut String) -> Result<usize> {
        write!(out, "${:04X}:   ", ip)?;
        let opcode = nes.debug_read(ip)?;
        write!(out, "{:02X}", opcode)?;
        let extra = Self::get_size(opcode) - 1;
        for i in 0..2 {
            if i < extra
                && let Ok(b) = nes.debug_read(ip.wrapping_add(1) + (i as u16))
            {
                write!(out, " {:02X}", b)?;
            } else {
                out.push_str("   ");
            }
        }
        write!(out, "   {} ", Self::get_pneumonic(opcode))?;
        Self::fmt_push_args(
            ip.wrapping_add(1),
            INSTRUCTION_LUT[opcode as usize].addressing_handler,
            nes,
            out,
        )?;
        out.push('\n');
        Ok(extra + 1)
    }

    /*
    fn fmt_push_args_nestest_log(
        arg_start: u16,
        a: AddrMode,
        bus: &mut BusCpu,
        out: &mut String,
    ) -> Result<()> {
        match a {
            IMM => {
                let imm = bus.debug_read(arg_start)?;
                write!(out, "#${:02X}", imm)?;
                Ok(())
            }
            ABS => {
                let addr = bus.debug_read16(arg_start)?;
                write!(out, "${:04X}", addr)?;
                Ok(())
            }
            REL => {
                let addr = bus.debug_read(arg_start)? as i8 as i16;
                let sum = arg_start.wrapping_add(1);
                let addr: u16 = sum.wrapping_add_signed(addr);
                write!(out, "${:04X}", addr)?;
                Ok(())
            }
            ABX => {
                let addr = bus.debug_read16(arg_start)?;
                write!(out, "${:04X}, X", addr)?;
                Ok(())
            }
            ABY => {
                let addr = bus.debug_read16(arg_start)?;
                write!(out, "${:04X}, Y", addr)?;
                Ok(())
            }
            ZP0 => {
                let addr = bus.debug_read(arg_start)? as u16;
                write!(out, "${:04X}", addr)?;
                Ok(())
            }
            ZPX => {
                let addr = bus.debug_read(arg_start)? as u16;
                write!(out, "${:04X}, X", addr)?;
                Ok(())
            }
            ZPY => {
                let addr = bus.debug_read(arg_start)? as u16;
                write!(out, "${:04X}, Y", addr)?;
                Ok(())
            }
            IND => {
                let addr = bus.debug_read16(arg_start)?;
                write!(out, "(${:04X})", addr)?;
                Ok(())
            }
            IZX => {
                let addr = bus.debug_read(arg_start)? as u16;
                write!(out, "(${:04X}, X)", addr)?;
                Ok(())
            }
            IZY => {
                let addr = bus.debug_read(arg_start)? as u16;
                write!(out, "(${:04X}), Y", addr)?;
                Ok(())
            }
            IMP => Ok(()),
        }
    }

    pub fn dissasemble_nestest_log(ip: u16, bus: &mut BusCpu, out: &mut String) -> Result<usize> {
        write!(out, "{:04X}  ", ip)?;
        let opcode = bus.debug_read(ip)?;
        write!(out, "{:02X}", opcode)?;
        let extra = Self::get_size(opcode) - 1;
        for i in 0..2 {
            if i < extra
                && let Ok(b) = bus.debug_read(ip.wrapping_add(1) + (i as u16))
            {
                write!(out, " {:02X}", b)?;
            } else {
                out.push_str("   ");
            }
        }
        let diff =  out.len();
        write!(out, "  {} ", Self::get_pneumonic(opcode))?;
        Self::fmt_push_args_nestest_log(
            ip.wrapping_add(1),
            INSTRUCTION_LUT[opcode as usize].addressing_handler,
            bus,
            out,
        )?;
        let diff = 34 - (out.len() - diff);
        (0..diff).for_each(|_| out.push(' '));
        Ok(extra + 1)
    }*/
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
    table[0x00] = ins!("BRK", brk, IMM, 2);

    // Unofficial instruction
    table[0x03] = ins!("SLO", slo, IZX, 8);
    table[0x07] = ins!("SLO", slo, ZP0, 5);
    table[0x0F] = ins!("SLO", slo, ABS, 6);
    table[0x13] = ins!("SLO", slo, IZY, 8);
    table[0x17] = ins!("SLO", slo, ZPX, 6);
    table[0x1B] = ins!("SLO", slo, ABY, 7);
    table[0x1F] = ins!("SLO", slo, ABX, 7);

    table[0x69] = ins!("ADC", adc, IMM, 2);
    table[0x65] = ins!("ADC", adc, ZP0, 3);
    table[0x75] = ins!("ADC", adc, ZPX, 4);
    table[0x6D] = ins!("ADC", adc, ABS, 4);
    table[0x7D] = ins!("ADC", adc, ABX, 4);
    table[0x79] = ins!("ADC", adc, ABY, 4);
    table[0x61] = ins!("ADC", adc, IZX, 6);
    table[0x71] = ins!("ADC", adc, IZY, 5);

    table[0x29] = ins!("AND", and, IMM, 2);
    table[0x25] = ins!("AND", and, ZP0, 3);
    table[0x35] = ins!("AND", and, ZPX, 4);
    table[0x2D] = ins!("AND", and, ABS, 4);
    table[0x3D] = ins!("AND", and, ABX, 4);
    table[0x39] = ins!("AND", and, ABY, 4);
    table[0x21] = ins!("AND", and, IZX, 6);
    table[0x31] = ins!("AND", and, IZY, 5);

    table[0x0A] = ins!("ASL", asl, IMP, 2);
    table[0x06] = ins!("ASL", asl, ZP0, 5);
    table[0x16] = ins!("ASL", asl, ZPX, 6);
    table[0x0E] = ins!("ASL", asl, ABS, 6);
    table[0x1E] = ins!("ASL", asl, ABX, 7);

    table[0x90] = ins!("BCC", bcc, REL, 2);
    table[0xB0] = ins!("BCS", bcs, REL, 2);

    table[0xF0] = ins!("BEQ", beq, REL, 2);
    table[0xD0] = ins!("BNE", bne, REL, 2);

    table[0x50] = ins!("BVC", bvc, REL, 2);
    table[0x70] = ins!("BVS", bvs, REL, 2);

    table[0x10] = ins!("BPL", bpl, REL, 2);
    table[0x30] = ins!("BMI", bmi, REL, 2);

    table[0x24] = ins!("BIT", bit, ZP0, 3);
    table[0x2C] = ins!("BIT", bit, ABS, 4);

    table[0x18] = ins!("CLC", clc, IMP, 2);
    table[0xD8] = ins!("CLD", cld, IMP, 2);
    table[0x58] = ins!("CLI", cli, IMP, 2);
    table[0xB8] = ins!("CLV", clv, IMP, 2);

    table[0xC9] = ins!("CMP", cmp, IMM, 2);
    table[0xC5] = ins!("CMP", cmp, ZP0, 3);
    table[0xD5] = ins!("CMP", cmp, ZPX, 4);
    table[0xCD] = ins!("CMP", cmp, ABS, 4);
    table[0xDD] = ins!("CMP", cmp, ABX, 4);
    table[0xD9] = ins!("CMP", cmp, ABY, 4);
    table[0xC1] = ins!("CMP", cmp, IZX, 6);
    table[0xD1] = ins!("CMP", cmp, IZY, 5);

    table[0xE0] = ins!("CPX", cpx, IMM, 2);
    table[0xE4] = ins!("CPX", cpx, ZP0, 3);
    table[0xEC] = ins!("CPX", cpx, ABS, 4);

    table[0xC0] = ins!("CPY", cpy, IMM, 2);
    table[0xC4] = ins!("CPY", cpy, ZP0, 3);
    table[0xCC] = ins!("CPY", cpy, ABS, 4);

    table[0xC6] = ins!("DEC", dec, ZP0, 5);
    table[0xD6] = ins!("DEC", dec, ZPX, 6);
    table[0xCE] = ins!("DEC", dec, ABS, 6);
    table[0xDE] = ins!("DEC", dec, ABX, 7);

    table[0xCA] = ins!("DEX", dex, IMP, 2);
    table[0x88] = ins!("DEY", dey, IMP, 2);

    table[0x49] = ins!("EOR", eor, IMM, 2);
    table[0x45] = ins!("EOR", eor, ZP0, 3);
    table[0x55] = ins!("EOR", eor, ZPX, 4);
    table[0x4D] = ins!("EOR", eor, ABS, 4);
    table[0x5D] = ins!("EOR", eor, ABX, 4);
    table[0x59] = ins!("EOR", eor, ABY, 4);
    table[0x41] = ins!("EOR", eor, IZX, 6);
    table[0x51] = ins!("EOR", eor, IZY, 5);

    table[0xE6] = ins!("INC", inc, ZP0, 5);
    table[0xF6] = ins!("INC", inc, ZPX, 6);
    table[0xEE] = ins!("INC", inc, ABS, 6);
    table[0xFE] = ins!("INC", inc, ABX, 7);

    table[0xE8] = ins!("INX", inx, IMP, 2);
    table[0xC8] = ins!("INY", iny, IMP, 2);

    table[0x4C] = ins!("JMP", jmp, ABS, 3);
    table[0x6C] = ins!("JMP", jmp, IND, 5);

    table[0x20] = ins!("JSR", jsr, ABS, 6);

    table[0xA9] = ins!("LDA", lda, IMM, 2);
    table[0xA5] = ins!("LDA", lda, ZP0, 3);
    table[0xB5] = ins!("LDA", lda, ZPX, 4);
    table[0xAD] = ins!("LDA", lda, ABS, 4);
    table[0xBD] = ins!("LDA", lda, ABX, 4);
    table[0xB9] = ins!("LDA", lda, ABY, 4);
    table[0xA1] = ins!("LDA", lda, IZX, 6);
    table[0xB1] = ins!("LDA", lda, IZY, 5);

    table[0xA2] = ins!("LDX", ldx, IMM, 2);
    table[0xA6] = ins!("LDX", ldx, ZP0, 3);
    table[0xB6] = ins!("LDX", ldx, ZPY, 4);
    table[0xAE] = ins!("LDX", ldx, ABS, 4);
    table[0xBE] = ins!("LDX", ldx, ABY, 4);

    table[0xA0] = ins!("LDX", ldy, IMM, 2);
    table[0xA4] = ins!("LDY", ldy, ZP0, 3);
    table[0xB4] = ins!("LDY", ldy, ZPX, 4);
    table[0xAC] = ins!("LDY", ldy, ABS, 4);
    table[0xBC] = ins!("LDY", ldy, ABX, 4);

    table[0x4A] = ins!("LSR", lsr, IMP, 2);
    table[0x46] = ins!("LSR", lsr, ZP0, 5);
    table[0x56] = ins!("LSR", lsr, ZPX, 6);
    table[0x4E] = ins!("LSR", lsr, ABS, 6);
    table[0x5E] = ins!("LSR", lsr, ABX, 7);

    table[0x09] = ins!("ORA", ora, IMM, 2);
    table[0x05] = ins!("ORA", ora, ZP0, 3);
    table[0x15] = ins!("ORA", ora, ZPX, 4);
    table[0x0D] = ins!("ORA", ora, ABS, 4);
    table[0x1D] = ins!("ORA", ora, ABX, 4);
    table[0x19] = ins!("ORA", ora, ABY, 4);
    table[0x01] = ins!("ORA", ora, IZX, 6);
    table[0x11] = ins!("ORA", ora, IZY, 5);

    table[0x48] = ins!("PHA", pha, IMP, 3);
    table[0x08] = ins!("PHP", php, IMP, 3);

    table[0x68] = ins!("PLA", pla, IMP, 4);
    table[0x28] = ins!("PLP", plp, IMP, 4);

    table[0x2A] = ins!("ROL", rol, IMP, 2);
    table[0x26] = ins!("ROL", rol, ZP0, 5);
    table[0x36] = ins!("ROL", rol, ZPX, 6);
    table[0x2E] = ins!("ROL", rol, ABS, 6);
    table[0x3E] = ins!("ROL", rol, ABX, 7);

    table[0x6A] = ins!("ROR", ror, IMP, 2);
    table[0x66] = ins!("ROR", ror, ZP0, 5);
    table[0x76] = ins!("ROR", ror, ZPX, 6);
    table[0x6E] = ins!("ROR", ror, ABS, 6);
    table[0x7E] = ins!("ROR", ror, ABX, 7);

    table[0x40] = ins!("RTI", rti, IMP, 6);
    table[0x60] = ins!("RTS", rts, IMP, 6);

    table[0xE9] = ins!("SBC", sbc, IMM, 2);
    table[0xE5] = ins!("SBC", sbc, ZP0, 3);
    table[0xF5] = ins!("SBC", sbc, ZPX, 4);
    table[0xED] = ins!("SBC", sbc, ABS, 4);
    table[0xFD] = ins!("SBC", sbc, ABX, 4);
    table[0xF9] = ins!("SBC", sbc, ABY, 4);
    table[0xE1] = ins!("SBC", sbc, IZX, 6);
    table[0xF1] = ins!("SBC", sbc, IZY, 5);

    table[0x38] = ins!("SEC", sec, IMP, 2);
    table[0xF8] = ins!("SED", sed, IMP, 2);
    table[0x78] = ins!("SEI", sei, IMP, 2);

    table[0x85] = ins!("STA", sta, ZP0, 3);
    table[0x95] = ins!("STA", sta, ZPX, 4);
    table[0x8D] = ins!("STA", sta, ABS, 4);
    table[0x9D] = ins!("STA", sta, ABX, 5);
    table[0x99] = ins!("STA", sta, ABY, 5);
    table[0x81] = ins!("STA", sta, IZX, 6);
    table[0x91] = ins!("STA", sta, IZY, 6);

    table[0x86] = ins!("STX", stx, ZP0, 3);
    table[0x96] = ins!("STX", stx, ZPY, 4);
    table[0x8E] = ins!("STX", stx, ABS, 4);

    table[0x84] = ins!("STY", sty, ZP0, 3);
    table[0x94] = ins!("STY", sty, ZPX, 4);
    table[0x8C] = ins!("STY", sty, ABS, 4);

    table[0xAA] = ins!("TAX", tax, IMP, 2);
    table[0xA8] = ins!("TAY", tay, IMP, 2);
    table[0xBA] = ins!("TSX", tsx, IMP, 2);
    table[0x8A] = ins!("TXA", txa, IMP, 2);
    table[0x9A] = ins!("TXS", txs, IMP, 2);
    table[0x98] = ins!("TYA", tya, IMP, 2);

    table[0xEA] = ins!("NOP", nop, IMP, 2);

    table[0x80] = ins!("*NOP", nop, IMM, 2);
    table[0x82] = ins!("*NOP", nop, IMM, 2);
    table[0x89] = ins!("*NOP", nop, IMM, 2);
    table[0xC2] = ins!("*NOP", nop, IMM, 2);
    table[0xE2] = ins!("*NOP", nop, IMM, 2);

    table[0x1C] = ins!("*NOP", nop, ABX, 4);
    table[0x3C] = ins!("*NOP", nop, ABX, 4);
    table[0x5C] = ins!("*NOP", nop, ABX, 4);
    table[0x7C] = ins!("*NOP", nop, ABX, 4);
    table[0xDC] = ins!("*NOP", nop, ABX, 4);
    table[0xFC] = ins!("*NOP", nop, ABX, 4);

    table[0x04] = ins!("*NOP", nop, ZP0, 3);
    table[0x44] = ins!("*NOP", nop, ZP0, 3);
    table[0x64] = ins!("*NOP", nop, ZP0, 3);

    table[0x14] = ins!("*NOP", nop, ZPX, 4);
    table[0x34] = ins!("*NOP", nop, ZPX, 4);
    table[0x54] = ins!("*NOP", nop, ZPX, 4);
    table[0x74] = ins!("*NOP", nop, ZPX, 4);
    table[0xF4] = ins!("*NOP", nop, ZPX, 4);
    table[0xD4] = ins!("*NOP", nop, ZPX, 4);

    table[0x0C] = ins!("*NOP", nop, ABS, 4);

    table
};

macro_rules! handler {
    ($fn_name:ident, $c:ident, $b:ident, $body:block) => {
        #[inline(always)]
        #[allow(unused)]
        pub fn $fn_name($c: &mut Cpu, $b: &mut Nes) -> Result<bool> {
            $body
        }
    };
}

pub mod addressing {
    use super::*;

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
        pub fn get_fptr(&self) -> fn(&mut Cpu, &mut Nes) -> Result<bool> {
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

    handler!(imp, c, bus, {
        c.state.fetched = c.registers.a;
        Ok(false)
    });

    handler!(imm, c, bus, {
        c.state.addr_abs = c.registers.pc;
        c.registers.pc += 1;
        Ok(false)
    });

    handler!(abs, c, bus, {
        c.state.addr_abs = bus.read16(c.registers.pc)?;
        c.registers.pc += 2;
        Ok(false)
    });

    handler!(rel, c, bus, {
        c.state.addr_rel = bus.read(c.registers.pc)? as i8 as i16;
        c.registers.pc += 1;
        c.state.addr_abs = c.registers.pc.wrapping_add_signed(c.state.addr_rel);
        // Add 1 CPU cycle if boundary crossed
        if c.registers.pc & 0xFF00 != c.state.addr_abs & 0xFF00 {
            return Ok(true);
        }
        Ok(false)
    });

    handler!(ind, c, bus, {
        let ptr = bus.read16(c.registers.pc)?;
        c.registers.pc += 2;
        // Simulate 6502 page boundary bug
        // This behaviour is incorrect on purpose
        if (ptr & 0x00FF) == 0x00FF {
            let addr_hi = {
                ((bus.read(ptr & 0xFF00)? as u16) << 8)
            };
            let addr_lo = {
                bus.read(ptr)? as u16
            };
            c.state.addr_abs = addr_hi | addr_lo;
        } else {
            // Correct behaviour
            c.state.addr_abs = bus.read16(ptr)?;
        }
        Ok(false)
    });

    handler!(abx, c, bus, {
        c.state.addr_abs = bus.read16(c.registers.pc)?;
        c.registers.pc += 2;
        let old_page = c.state.addr_abs >> 8;
        c.state.addr_abs += c.registers.x as u16;

        // Additional cycle if pagechange
        if old_page != c.state.addr_abs >> 8 {
            return Ok(true);
        }
        Ok(false)
    });

    handler!(aby, c, bus, {
        c.state.addr_abs = bus.read16(c.registers.pc)?;
        c.registers.pc += 2;
        let old_page = c.state.addr_abs >> 8;
        c.state.addr_abs += c.registers.y as u16;
        // Additional cycle if pagechange
        if old_page != c.state.addr_abs >> 8 {
            return Ok(true);
        }
        Ok(false)
    });

    handler!(zp0, c, bus, {
        c.state.addr_abs = bus.read(c.registers.pc)? as u16;
        c.registers.pc += 1;
        c.state.addr_abs &= 0x00FF;
        Ok(false)
    });

    handler!(zpx, c, bus, {
        c.state.addr_abs = (bus.read(c.registers.pc)? + c.registers.x) as u16;
        c.registers.pc += 1;
        c.state.addr_abs &= 0x00FF;
        Ok(false)
    });

    handler!(zpy, c, bus, {
        c.state.addr_abs = (bus.read(c.registers.pc)? + c.registers.y) as u16;
        c.registers.pc += 1;
        c.state.addr_abs &= 0x00FF;
        Ok(false)
    });

    handler!(izx, c, bus, {
        let ptr = (bus.read(c.registers.pc)? + c.registers.x) as u16;
        c.registers.pc += 1;
        let lo = bus.read(ptr & 0x00FF)? as u16;
        let hi = bus.read((ptr + 1) & 0x00FF)? as u16;

        c.state.addr_abs = (hi << 8) | lo;
        Ok(false)
    });

    handler!(izy, c, bus, {
        let ptr = bus.read(c.registers.pc)? as u16;
        c.registers.pc += 1;
        let lo = bus.read(ptr & 0x00FF)? as u16;
        let hi = bus.read((ptr + 1) & 0x00FF)? as u16;

        c.state.addr_abs = (hi << 8) | lo;
        let old_page = c.state.addr_abs >> 8;
        c.state.addr_abs = c.state.addr_abs.wrapping_add(c.registers.y as u16);
        if old_page != c.state.addr_abs >> 8 {
            // dummy read
            let _ = bus.read(ptr);
            return Ok(true);
        }
        Ok(false)
    });
}

pub mod instructions {

    use crate::{nes::cpu::Flag6502::{self}};

    use super::*;

    pub fn nop(c: &mut Cpu, _: &mut Nes) -> Result<bool> {
        match c.state.last_opcode {
            0x1C | 0x3C | 0x5C | 0x7C | 0xDC | 0xFC => Ok(true),
            _ => Ok(false),
        }
    }

    pub fn ldx(c: &mut Cpu, bus: &mut Nes) -> Result<bool> {
        c.registers.x = bus.read(c.state.addr_abs)?;
        c.set_flag(Flag6502::Z, c.registers.x == 0);
        c.set_flag(Flag6502::N, (c.registers.x & 0x80) != 0);
        Ok(true)
    }

    handler!(ldy, c, bus, {
        c.registers.y = bus.read(c.state.addr_abs)?;
        c.set_flag(Flag6502::Z, c.registers.y == 0);
        c.set_flag(Flag6502::N, (c.registers.y & 0x80) != 0);
        Ok(true)
    });

    handler!(lda, c, bus, {
        c.registers.a = bus.read(c.state.addr_abs)?;
        c.set_flag(Flag6502::Z, c.registers.a == 0);
        c.set_flag(Flag6502::N, (c.registers.a & 0x80) != 0);
        Ok(true)
    });

    handler!(stx, c, bus, {
        bus.write(c.state.addr_abs, c.registers.x)?;
        Ok(false)
    });

    handler!(adc, c, bus, {
        let b = bus.read(c.state.addr_abs)? as u16;
        let result: u16 = c.registers.a as u16 + b + c.get_flag(Flag6502::C) as u16;
        c.set_flag(Flag6502::C, result > 0xFF);
        c.set_flag(Flag6502::Z, (result & 0x00FF) == 0);
        c.set_flag(
            Flag6502::V,
            ((result ^ c.registers.a as u16) & (result ^ b as u16) & 0x0080) != 0,
        );
        c.set_flag(Flag6502::N, (result & 0x80) != 0);
        c.registers.a = (result & 0x00FF) as u8;
        Ok(true)
    });

    handler!(and, c, bus, {
        let mem = bus.read(c.state.addr_abs)?;
        c.registers.a &= mem;
        c.set_flag(Flag6502::Z, c.registers.a == 0);
        c.set_flag(Flag6502::N, (c.registers.a & 0x80) != 0);
        Ok(true)
    });

    handler!(asl, c, bus, {
        c.fetch(bus);
        c.set_flag(Flag6502::C, c.state.fetched > 0x7F);
        let result: u8 = c.state.fetched << 1;
        c.set_flag(Flag6502::N, result > 0x7F);
        c.set_flag(Flag6502::Z, result == 0);
        if INSTRUCTION_LUT[c.state.last_opcode as usize].addressing_handler == IMP {
            c.registers.a = result;
        } else {
            bus.write(c.state.addr_abs, result)?;
        }
        Ok(false)
    });

    handler!(bcc, c, bus, {
        if c.get_flag(Flag6502::C) {
            return Ok(false);
        }
        c.state.cycles_left += 1;
        c.registers.pc = c.state.addr_abs;
        Ok(true)
    });

    handler!(bcs, c, bus, {
        if !c.get_flag(Flag6502::C) {
            return Ok(false);
        }
        c.state.cycles_left += 1;
        c.registers.pc = c.state.addr_abs;
        Ok(true)
    });

    handler!(beq, c, bus, {
        if !c.get_flag(Flag6502::Z) {
            return Ok(false);
        }
        c.state.cycles_left += 1;
        c.registers.pc = c.state.addr_abs;
        Ok(true)
    });

    handler!(bne, c, bus, {
        if c.get_flag(Flag6502::Z) {
            return Ok(false);
        }
        c.state.cycles_left += 1;
        c.registers.pc = c.state.addr_abs;
        Ok(true)
    });

    handler!(bpl, c, bus, {
        if c.get_flag(Flag6502::N) {
            return Ok(false);
        }
        c.state.cycles_left += 1;
        c.registers.pc = c.state.addr_abs;
        Ok(true)
    });

    handler!(bmi, c, bus, {
        if !c.get_flag(Flag6502::N) {
            return Ok(false);
        }
        c.state.cycles_left += 1;
        c.registers.pc = c.state.addr_abs;
        Ok(true)
    });

    handler!(bvc, c, bus, {
        if c.get_flag(Flag6502::V) {
            return Ok(false);
        }
        c.state.cycles_left += 1;
        c.registers.pc = c.state.addr_abs;
        Ok(true)
    });

    handler!(bvs, c, bus, {
        if !c.get_flag(Flag6502::V) {
            return Ok(false);
        }
        c.state.cycles_left += 1;
        c.registers.pc = c.state.addr_abs;
        Ok(true)
    });

    handler!(bit, c, bus, {
        let mem = bus.read(c.state.addr_abs)?;
        c.set_flag(Flag6502::Z, (c.registers.a & mem) == 0);
        c.set_flag(Flag6502::V, (mem & 0x40) != 0);
        c.set_flag(Flag6502::N, (mem & 0x80) != 0);
        Ok(false)
    });

    handler!(brk, c, bus, {
        let p = c.registers.pc + 2;
        c.push(((p & 0xFF00) >> 8) as u8, bus);
        c.push((p & 0x00FF) as u8, bus);
        c.set_flag(Flag6502::I, true);
        c.set_flag(Flag6502::B, true);
        c.push(c.registers.sr, bus);
        c.registers.pc = bus.read16(0xFFFE)?;
        Ok(false)
    });

    handler!(clc, c, bus, {
        c.set_flag(Flag6502::C, false);
        Ok(false)
    });

    handler!(cld, c, bus, {
        c.set_flag(Flag6502::D, false);
        Ok(false)
    });

    handler!(cli, c, bus, {
        c.set_flag(Flag6502::I, false);
        Ok(false)
    });

    handler!(clv, c, bus, {
        c.set_flag(Flag6502::V, false);
        Ok(false)
    });

    handler!(cmp, c, bus, {
        let mem: u8 = bus.read(c.state.addr_abs)?;
        let result = c.registers.a.wrapping_sub(mem);
        c.set_flag(Flag6502::C, c.registers.a >= mem);
        c.set_flag(Flag6502::Z, result == 0);
        c.set_flag(Flag6502::N, result > 0x7F);
        Ok(true)
    });

    handler!(cpx, c, bus, {
        let mem: u8 = bus.read(c.state.addr_abs)?;
        let result = c.registers.x.wrapping_sub(mem);
        c.set_flag(Flag6502::C, c.registers.x >= mem);
        c.set_flag(Flag6502::Z, result == 0);
        c.set_flag(Flag6502::N, result > 0x7F);
        Ok(false)
    });

    handler!(cpy, c, bus, {
        let mem: u8 = bus.read(c.state.addr_abs)?;
        let result = c.registers.y.wrapping_sub(mem);
        c.set_flag(Flag6502::C, c.registers.y >= mem);
        c.set_flag(Flag6502::Z, result == 0);
        c.set_flag(Flag6502::N, result > 0x7F);
        Ok(false)
    });

    handler!(dec, c, bus, {
        let mem = bus.read(c.state.addr_abs)? - 1;
        bus.write(c.state.addr_abs, mem)?;
        c.set_flag(Flag6502::Z, mem == 0);
        c.set_flag(Flag6502::N, (mem & 0x80) != 0);
        Ok(false)
    });

    handler!(dex, c, bus, {
        c.registers.x -= 1;
        c.set_flag(Flag6502::Z, c.registers.x == 0);
        c.set_flag(Flag6502::N, (c.registers.x & 0x80) != 0);
        Ok(false)
    });

    handler!(dey, c, bus, {
        c.registers.y -= 1;
        c.set_flag(Flag6502::Z, c.registers.y == 0);
        c.set_flag(Flag6502::N, (c.registers.y & 0x80) != 0);
        Ok(false)
    });

    handler!(eor, c, bus, {
        c.registers.a ^= bus.read(c.state.addr_abs)?;
        c.set_flag(Flag6502::Z, c.registers.a == 0);
        c.set_flag(Flag6502::N, (c.registers.a & 0x80) != 0);
        Ok(true)
    });

    handler!(inc, c, bus, {
        let mem = bus.read(c.state.addr_abs)? + 1;
        bus.write(c.state.addr_abs, mem)?;
        c.set_flag(Flag6502::Z, mem == 0);
        c.set_flag(Flag6502::N, (mem & 0x80) != 0);
        Ok(false)
    });

    handler!(inx, c, bus, {
        c.registers.x += 1;
        c.set_flag(Flag6502::Z, c.registers.x == 0);
        c.set_flag(Flag6502::N, (c.registers.x & 0x80) != 0);
        Ok(false)
    });

    handler!(iny, c, bus, {
        c.registers.y += 1;
        c.set_flag(Flag6502::Z, c.registers.y == 0);
        c.set_flag(Flag6502::N, (c.registers.y & 0x80) != 0);
        Ok(false)
    });

    handler!(jmp, c, bus, {
        c.registers.pc = c.state.addr_abs;
        Ok(false)
    });

    handler!(jsr, c, bus, {
        c.registers.pc -= 1;
        c.push(((c.registers.pc & 0xFF00) >> 8) as u8, bus);
        c.push((c.registers.pc & 0x00FF) as u8, bus);
        c.registers.pc = c.state.addr_abs;
        Ok(false)
    });

    handler!(lsr, c, bus, {
        c.fetch(bus);
        c.set_flag(Flag6502::C, (c.state.fetched & 1) != 0);
        let result: u8 = c.state.fetched >> 1;
        c.set_flag(Flag6502::Z, result == 0);
        c.set_flag(Flag6502::N, false);
        if INSTRUCTION_LUT[c.state.last_opcode as usize].addressing_handler == IMP {
            c.registers.a = result;
        } else {
            bus.write(c.state.addr_abs, result)?;
        }
        Ok(false)
    });

    handler!(ora, c, bus, {
        c.registers.a |= bus.read(c.state.addr_abs)?;
        c.set_flag(Flag6502::Z, c.registers.a == 0);
        c.set_flag(Flag6502::N, (c.registers.a & 0x80) != 0);
        Ok(true)
    });

    handler!(pha, c, bus, {
        c.push(c.registers.a, bus);
        Ok(false)
    });

    handler!(php, c, bus, {
        let sr = c.registers.sr | Flag6502::B as u8 | Flag6502::U as u8;
        c.push(sr, bus);
        Ok(false)
    });

    handler!(pla, c, bus, {
        c.registers.a = c.pull(bus);
        c.set_flag(Flag6502::Z, c.registers.a == 0);
        c.set_flag(Flag6502::N, (c.registers.a & 0x80) != 0);
        Ok(false)
    });

    handler!(plp, c, bus, {
        let mut pulled = c.pull(bus);
        let og_b = c.get_flag(Flag6502::B);
        c.registers.sr = pulled;
        c.set_flag(Flag6502::B, og_b);
        c.set_flag(Flag6502::U, true);
        Ok(false)
    });

    handler!(rol, c, bus, {
        c.fetch(bus);
        let result: u8 = c.state.fetched << 1 | c.get_flag(Flag6502::C) as u8;
        c.set_flag(Flag6502::C, c.state.fetched > 0x7F);
        c.set_flag(Flag6502::Z, result == 0);
        c.set_flag(Flag6502::N, result > 0x7F);
        if INSTRUCTION_LUT[c.state.last_opcode as usize].addressing_handler == IMP {
            c.registers.a = result;
        } else {
            bus.write(c.state.addr_abs, result)?;
        }
        Ok(false)
    });

    handler!(ror, c, bus, {
        c.fetch(bus);
        let result: u8 = (c.get_flag(Flag6502::C) as u8) << 7 | c.state.fetched >> 1;
        c.set_flag(Flag6502::C, (c.state.fetched & 1) != 0);
        c.set_flag(Flag6502::Z, result == 0);
        c.set_flag(Flag6502::N, result > 0x7F);
        if INSTRUCTION_LUT[c.state.last_opcode as usize].addressing_handler == IMP {
            c.registers.a = result;
        } else {
            bus.write(c.state.addr_abs, result)?;
        }
        Ok(false)
    });

    handler!(rti, c, bus, {
        #[cfg(feature = "log")]
        {
            if c.log {
                log::info!("RTI!");
                bus.log_ppu_state();
            }
        }
        let mut sr = c.pull(bus);
        sr &= !(Flag6502::B as u8);
        sr |= Flag6502::U as u8;
        c.registers.sr = sr;
        c.registers.pc = c.pull(bus) as u16 | (c.pull(bus) as u16) << 8;
        Ok(false)
    });

    handler!(rts, c, bus, {
        c.registers.pc = c.pull(bus) as u16 | (c.pull(bus) as u16) << 8;
        c.registers.pc += 1;
        Ok(false)
    });

    handler!(sbc, c, bus, {
        let mem = bus.read(c.state.addr_abs)?;

        let not_mem = (mem ^ 0xFF) as u16;
        let a = c.registers.a as u16;
        let carry = c.get_flag(Flag6502::C) as u16;
        let mut result: u16 = a + not_mem + carry;

        // Set if no underflow ocurred
        c.set_flag(Flag6502::C, result > 0xFF);
        c.set_flag(Flag6502::Z, (result & 0x00FF) == 0);
        c.set_flag(
            Flag6502::V,
            ((result ^ a) & (result ^ not_mem) & 0x80) != 0,
        );
        let result = (result & 0x00FF) as u8;
        c.set_flag(Flag6502::N, (result & 0x80) != 0);

        c.registers.a = result;
        Ok(true)
    });

    handler!(sec, c, bus, {
        c.set_flag(Flag6502::C, true);
        Ok(false)
    });

    handler!(sed, c, bus, {
        c.set_flag(Flag6502::D, true);
        Ok(false)
    });

    handler!(sei, c, bus, {
        c.set_flag(Flag6502::I, true);
        Ok(false)
    });

    handler!(sta, c, bus, {
        bus.write(c.state.addr_abs, c.registers.a)?;
        Ok(false)
    });

    handler!(sty, c, bus, {
        bus.write(c.state.addr_abs, c.registers.y)?;
        Ok(false)
    });

    handler!(tax, c, bus, {
        c.registers.x = c.registers.a;
        c.set_flag(Flag6502::Z, c.registers.x == 0);
        c.set_flag(Flag6502::N, (c.registers.x & 0x80) != 0);
        Ok(false)
    });

    handler!(tay, c, bus, {
        c.registers.y = c.registers.a;
        c.set_flag(Flag6502::Z, c.registers.y == 0);
        c.set_flag(Flag6502::N, (c.registers.y & 0x80) != 0);
        Ok(false)
    });

    handler!(tsx, c, bus, {
        c.registers.x = c.registers.sp;
        c.set_flag(Flag6502::Z, c.registers.x == 0);
        c.set_flag(Flag6502::N, (c.registers.x & 0x80) != 0);
        Ok(false)
    });

    handler!(txa, c, bus, {
        c.registers.a = c.registers.x;
        c.set_flag(Flag6502::Z, c.registers.a == 0);
        c.set_flag(Flag6502::N, (c.registers.a & 0x80) != 0);
        Ok(false)
    });

    handler!(txs, c, bus, {
        c.registers.sp = c.registers.x;
        Ok(false)
    });

    handler!(tya, c, bus, {
        c.registers.a = c.registers.y;
        c.set_flag(Flag6502::Z, c.registers.a == 0);
        c.set_flag(Flag6502::N, (c.registers.a & 0x80) != 0);
        Ok(false)
    });

    handler!(slo, c, bus, {
        let mut mem = bus.read(c.state.addr_abs)?;
        c.set_flag(Flag6502::C, (mem & 0x80) != 0);
        mem <<= 1;
        bus.write(c.state.addr_abs, mem)?;
        c.registers.a |= mem;
        c.set_flag(Flag6502::Z, c.registers.a == 0);
        c.set_flag(Flag6502::N, (c.registers.a & 0x80) != 0);
        Ok(true)
    });
}
