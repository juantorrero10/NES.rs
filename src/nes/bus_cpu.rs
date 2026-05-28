use thiserror::Error;

use crate::nes::{
    mapper::CpuMapper,
};

// RAM Constants
const ADDR_RAM_START: u16 = 0x0000;
const ADDR_RAM_END: u16 = 0x1FFF;
const SIZE_RAM: usize = 0x0800;

// PPU register constants
const ADDR_PPU_REG_START: u16 = 0x2000;
const ADDR_PPU_REG_END: u16 = 0x3FFF;
const SIZE_PPU_REG: usize = 0x8;
const ADDITIONAL_PPU_REG: u16 = 0x4014;

// Controler IO constants
const ADDR_IO_START: u16 = 0x4000;
const ADDR_IO_END: u16 = 0x401F;
const SIZE_IO: usize = 0x20;

const ADDR_ROM_START: u16 = 0x8000;
const ADDR_ROM_END: u16 = 0xFFFF;

#[derive(Error, Debug)]
pub enum BusCpuError {
    #[error("Tried to access unmirrored address inside region: ${0:04X}")]
    AddrError(u16),
    #[error("Tried to access unimplemented memory range: ${0:04X} ")]
    AddrRangeNotImplemented(u16),
}

pub struct BusCpu {
    ram: [u8; SIZE_RAM],
    ppu_reg: [u8; SIZE_PPU_REG],
    // TODO : better name
    ppu_additional_reg: u8,
    controller_io: [u8; SIZE_IO],
    prg_data: Vec<u8>,
    prg_ram: Vec<u8>,
    fake_value: u8,

    mapper: Box<dyn CpuMapper>,
}

type Result<T> = std::result::Result<T, BusCpuError>;

impl BusCpu {
    pub fn new(mapper: Box<dyn CpuMapper>, prg_data: Vec<u8>, prg_ram: Vec<u8>) -> BusCpu {
        // TODO: Cart
        BusCpu {
            ram: [0u8; SIZE_RAM],
            ppu_reg: [0u8; SIZE_PPU_REG],
            ppu_additional_reg: 0,
            controller_io: [0u8; SIZE_IO],
            mapper,
            prg_data,
            prg_ram,
            fake_value: 0,
        }
    }

    fn resolve_addr(&mut self, mut addr: u16) -> Result<&mut u8> {
        let input_addr = addr;
        // Map address
        addr = self.mapper.cpu_map_addr(addr);

        match input_addr {
            ADDR_RAM_START..=ADDR_RAM_END => {
                addr &= SIZE_RAM as u16 - 1;
                return self
                    .ram
                    .get_mut(addr as usize)
                    .ok_or(BusCpuError::AddrError(input_addr));
            }
            ADDR_PPU_REG_START..=ADDR_PPU_REG_END => {
                addr &= SIZE_PPU_REG as u16 - 1;
                return self
                    .ppu_reg
                    .get_mut(addr as usize)
                    .ok_or(BusCpuError::AddrError(input_addr));
            }
            ADDR_ROM_START..=ADDR_ROM_END => {
                addr &= !(1 << 15);
                return self
                    .prg_data
                    .get_mut(addr as usize)
                    .ok_or(BusCpuError::AddrError(input_addr));
            }
            _ => Err(BusCpuError::AddrRangeNotImplemented(input_addr)),
        }
    }

    #[inline(always)]
    pub fn read(&mut self, addr: u16) -> Result<u8> {
        return Ok(*(self.resolve_addr(addr)?));
    }

    /// Read 16bit number (little endian, LSB first)
    #[inline(always)]
    pub fn read16(&mut self, addr: u16) -> Result<u16> {
        Ok((self.read(addr)? as u16) | (self.read(addr + 1)? as u16) << 8)
    }

    #[inline(always)]
    pub fn write(&mut self, addr: u16, data: u8) -> Result<()> {
        *(self.resolve_addr(addr)?) = data;
        Ok(())
    }
}
