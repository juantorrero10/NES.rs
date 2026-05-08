use thiserror::Error;

use crate::nes::{bus_cpu::BusCpuError, cpu::{Cpu, CpuError}};

#[derive(Error, Debug)]
pub enum InstructionError {
    #[error("CPU fail: {0}")]
    CpuError(#[from] CpuError),
    #[error("Bus fail: {0}")]
    BusCpuError(#[from] BusCpuError)
}

type Result<T> = std::result::Result<T, InstructionError>;

#[derive(Clone, Copy)]
pub struct Instruction6502 {
    pub n_cycles: u8,
    pub pneumonic: &'static str,
    pub addressing_handler: fn(&mut Cpu) -> Result<bool>,
    pub instruction_handler: fn(&mut Cpu) -> Result<bool>,
}


const DEF: Instruction6502 = Instruction6502 { 
    n_cycles: 2, 
    pneumonic: "???", 
    addressing_handler: addressing::imp, 
    instruction_handler: instructions::nop 
};

impl Default for Instruction6502 {
    fn default() -> Self {
        DEF
    }
}

impl Instruction6502 {
    fn new(
        pneumonic: &'static str, 
        instruction_handler: fn(&mut Cpu) -> Result<bool>,
        addressing_handler: fn(&mut Cpu) -> Result<bool>,
        n_cycles: u8
    ) -> Self {
        Self { n_cycles, pneumonic, addressing_handler, instruction_handler }    
    }
}




use addressing::*;
use instructions::*;

pub static INSTRUCTION_LUT: [Instruction6502; 256] = {
    macro_rules! ins {
        ($pneumonic:expr, $instruction_handler:expr, $addressing_handler:expr, $n_cycles:expr) => {
                Instruction6502 { 
                    n_cycles: $n_cycles, 
                    pneumonic: $pneumonic, 
                    addressing_handler: $addressing_handler, 
                    instruction_handler: $instruction_handler  
                }
        };
    }
    let mut table = [DEF; 256];
    table[0xA2] = ins!("LDX", ldx, imm, 2);
    table
}; 

macro_rules! handler {
    ($fn_name:ident, $c:ident, $body:block) => {
        #[inline(always)]
        #[allow(unused)]
        pub fn $fn_name($c: &mut Cpu) -> Result<bool> {$body}
    };
}


pub mod addressing {
    use crate::bus;

use super::*;


    handler!(imp, c, {
        c.state.fetched = c.registers.a;
        Ok(false)
    });

    handler!(imm, c, 
    {
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
        c.state.addr_rel = bus!(c).read(c.registers.pc)? as i16;
        c.registers.pc += 1;
        Ok(false)
    });

    handler!(ind, c, {
        let ptr = bus!(c).read16(c.registers.pc)?;
        c.registers.pc += 2;
        // Simulate 6502 page boundary bug
        // This behaviour is incorrect on purpose
        if (ptr & 0x00FF) == 0x00FF {
            c.state.addr_abs = ((bus!(c).read(ptr & 0xFF00)? as u16) << 8 ) | bus!(c).read(ptr)? as u16;
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

    handler!(idx, c, {
        let mut ptr = (bus!(c).read(c.registers.pc)? + c.registers.x) as u16;
        c.registers.pc += 1;
        ptr += c.registers.x as u16;
        let lo = bus!(c).read(ptr       & 0x00FF)?;
        let hi = bus!(c).read((ptr + 1) & 0x00FF)?;

        c.state.addr_abs = (hi << 8 | lo) as u16;
        Ok(false)
    });

    handler!(idy, c, {
        let mut ptr = (bus!(c).read(c.registers.pc)?) as u16;
        c.registers.pc += 1;
        ptr += c.registers.x as u16;
        let lo = bus!(c).read(ptr       & 0x00FF)?;
        let hi = bus!(c).read((ptr + 1) & 0x00FF)?;

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
    use crate::nes::cpu::Flag6502;

    use super::*;

    pub fn nop(c: &mut Cpu) -> Result<bool> {
        match c.state.last_opcode {
            0x1C|
            0x3C|
            0x5C|
            0x7C|
            0xDC|
            0xFC => Ok(true),
            _ => Ok(false)
        }
    }

    pub fn ldx(c: &mut Cpu) -> Result<bool> {
        c.registers.x = c.fetch()?;
        c.set_flag(Flag6502::Z, c.registers.x == 0);
        c.set_flag(Flag6502::N, (c.registers.x & 0x80) != 0);
        Ok(true)
    }
}

