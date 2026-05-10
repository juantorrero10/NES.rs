use thiserror::Error;

// RAM Constants
const ADDR_RAM_START: u16 = 0x0000;
const ADDR_RAM_END: u16 = 0x1FFF;
const SIZE_RAM: usize = 0x0800;

// PPU register constants
const ADDR_PPU_REG_START: u16 = 0x2000;
const ADDR_PPU_REG_END: u16 = 0x2007;
const SIZE_PPU_REG: usize = 0x8;
const ADDITIONAL_PPU_REG: u16 = 0x4014;

// Controler IO constants
const ADDR_IO_START: u16 = 0x4000;
const ADDR_IO_END: u16 = 0x401F;
const SIZE_IO: usize = 0x20;

const ADDR_ROM_START: u16 = 0x8000;
const ADDR_ROM_END: u16 = 0xFFFB;

#[derive(Error, Debug)]
pub enum BusCpuError {
    #[error("Tried to access address out of range: ${0:04X}")]
    AddrError(u16),
    #[error("Tried to access unimplemented memory range: ${0:04X} ")]
    AddrRangeNotImplemented(u16),
}

#[derive(Debug)]
pub struct BusCpu {
    ram: [u8; SIZE_RAM],
    ppu_reg: [u8; SIZE_PPU_REG],
    // TODO : better name
    ppu_additional_reg: u8,
    controller_io: [u8; SIZE_IO],
    cart_ram: Vec<u8>,
    cart_prg: Vec<u8>,
    fake_value: u8,
}

impl BusCpu {
    pub fn new() -> BusCpu {
        // TODO: Cart
        BusCpu {
            ram: [0u8; SIZE_RAM],
            ppu_reg: [0u8; SIZE_PPU_REG],
            ppu_additional_reg: 0,
            controller_io: [0u8; SIZE_IO],
            cart_ram: Vec::new(),
            cart_prg: Vec::new(),
            fake_value: 0,
        }
    }

    fn resolve_addr(&mut self, mut addr: u16) -> Result<&mut u8, BusCpuError> {
        let input_addr = addr;
        match input_addr {
            ADDR_RAM_START..=ADDR_RAM_END => {
                addr &= SIZE_RAM as u16 - 1;
                return self
                    .ram
                    .get_mut(addr as usize)
                    .ok_or(BusCpuError::AddrError(input_addr));
            }
            ADDR_ROM_START..=ADDR_ROM_END => {
                addr &= !(1 << 15);
                return self
                    .cart_prg
                    .get_mut(addr as usize)
                    .ok_or(BusCpuError::AddrError(input_addr));
            }
            0xFFFC => {
                self.fake_value = 0x00;
                return Ok(&mut self.fake_value);
            }
            0xFFFD => {
                self.fake_value = 0x80;
                return Ok(&mut self.fake_value);
            }
            _ => Err(BusCpuError::AddrRangeNotImplemented(input_addr)),
        }
    }

    #[inline(always)]
    pub fn read(&mut self, addr: u16) -> Result<u8, BusCpuError> {
        return Ok(*(self.resolve_addr(addr)?));
    }

    // #[cfg(debug_assertions)]
    pub fn debug_load_prg(&mut self, prg: &[u8]) {
        self.cart_prg.extend_from_slice(prg);
    }

    /// Read 16bit number (little endian, LSB first)
    #[inline(always)]
    pub fn read16(&mut self, addr: u16) -> Result<u16, BusCpuError> {
        Ok((self.read(addr)? as u16) | (self.read(addr + 1)? as u16) << 8)
    }

    #[inline(always)]
    pub fn write(&mut self, addr: u16, data: u8) -> Result<(), BusCpuError> {
        *(self.resolve_addr(addr)?) = data;
        Ok(())
    }
}
