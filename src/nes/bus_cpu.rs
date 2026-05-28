use std::{error::Error, mem};

use thiserror::Error;

use crate::nes::{NesComm, NesEvent, cpu::Flag6502::D, mapper::CpuMapper, ppu::PpuRegister, trigger_read_event, trigger_write_event};

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
    AddrUnMirrored(u16),
    #[error("Tried to access unimplemented memory range: ${0:04X} ")]
    AddrRangeNotImplemented(u16),
    #[error("{0}")]
    UnknownError(#[from] Box<dyn Error>),
    #[error("The Bus of the CPU couldn't talk to other components as they are not registered.")]
    CommunicationsNotConnected
}

pub struct BusCpu {
    ram: [u8; SIZE_RAM],

    debug_ppu_reg: [u8; SIZE_PPU_REG],
    // TODO : better name
    ppu_additional_reg: u8,
    controller_io: [u8; SIZE_IO],
    prg_data: Vec<u8>,
    prg_ram: Vec<u8>,
    fake_value: u8,

    mapper: Box<dyn CpuMapper>,
    communications: Option<NesComm>,
}

type Result<T> = std::result::Result<T, BusCpuError>;


enum BusTarget<'a> {
    Memory(&'a mut u8),
    HardwareIntercept {
        // Some memory accesses dont lie behind clean containters. Instead they need a hardware device manage it. 
        event: NesEvent,
        // Debuggers should not trigger the events but at the same time have the data updated
        // All interceptions requiere a mirrored simulated memory read/write in order to maintain them fresh.
        debug_simulated_target: *mut u8,
    }
}

impl<'a> BusTarget<'a> {
    fn unwrap_memory(self) -> &'a mut u8 {
        if let Memory(re) = self {
            return re;
        }
        panic!("It is a hardware intercept");
    }
}


use BusTarget::*;

impl BusCpu {
    pub fn new(mapper: Box<dyn CpuMapper>, prg_data: Vec<u8>, prg_ram: Vec<u8>) -> BusCpu {
        // TODO: Cart
        BusCpu {
            ram: [0u8; SIZE_RAM],

            // Just debug, for dissasembler and memory viewer
            debug_ppu_reg: [0u8; SIZE_PPU_REG],


            ppu_additional_reg: 0,
            controller_io: [0u8; SIZE_IO],
            mapper,
            prg_data,
            prg_ram,
            communications: None,
            fake_value: 0,
        }
    }

    pub fn add_communications(&mut self, comm: NesComm) {
        self.communications = Some(comm);
    }

    fn resolve_addr(&mut self, mut addr: u16, write: bool, trigger_events: bool) -> Result<BusTarget> {
        let input_addr = addr;
        // Map address
        addr = self.mapper.cpu_map_addr(addr);

        match input_addr {
            ADDR_RAM_START..=ADDR_RAM_END => {
                addr &= SIZE_RAM as u16 - 1;
                return Ok(Memory(self
                    .ram
                    .get_mut(addr as usize)
                    .ok_or(BusCpuError::AddrUnMirrored(input_addr))?));
            }
            ADDR_PPU_REG_START..=ADDR_PPU_REG_END => {
                addr &= SIZE_PPU_REG as u16 - 1;

                let debug_simulated_target = self
                    .debug_ppu_reg
                    .get_mut(addr as usize)
                    .ok_or(BusCpuError::AddrUnMirrored(input_addr))?;

                if trigger_events {
                    return if write {
                        Ok(
                            HardwareIntercept { 
                                event: NesEvent::PpuRegWrittenTo(PpuRegister::from_0_to_7(addr)),
                                debug_simulated_target,
                            }
                        )
                    } else {
                        Ok(
                            HardwareIntercept { 
                                event: NesEvent::PpuRegReadFrom(PpuRegister::from_0_to_7(addr)),
                                debug_simulated_target,
                            }
                        )
                    }
                }
                // Debug only
                Ok(Memory(debug_simulated_target))
            }
            ADDR_ROM_START..=ADDR_ROM_END => {
                addr &= !(1 << 15);
                return Ok(Memory(self
                    .prg_data
                    .get_mut(addr as usize)
                    .ok_or(BusCpuError::AddrUnMirrored(input_addr))?));
            }
            _ => Err(BusCpuError::AddrRangeNotImplemented(input_addr)),
        }
    }

    #[inline(always)]
    pub fn read(&mut self, addr: u16) -> Result<u8> {
        match self.resolve_addr(addr, false, true)? {
            Memory(r) => { return Ok(*r); }
            HardwareIntercept{ event, debug_simulated_target} => {
                let comms = self.communications.as_ref().unwrap();
                let data = trigger_read_event(comms, event)?;
                unsafe {
                    *debug_simulated_target = data;
                }
                Ok(data)
            }
        }
    }

    /// Read 16bit number (little endian, LSB first)
    #[inline(always)]
    pub fn read16(&mut self, addr: u16) -> Result<u16> {
        Ok((self.read(addr)? as u16) | (self.read(addr + 1)? as u16) << 8)
    }

    #[inline(always)]
    pub fn write(&mut self, addr: u16, data: u8) -> Result<()> {
        match self.resolve_addr(addr, true, true)? {
            Memory(r) => { *r = data; Ok(()) }
            HardwareIntercept{ event, debug_simulated_target} => {
                let comms = self.communications.as_ref().unwrap();
                trigger_write_event(comms, event, data)?;
                unsafe {
                    *debug_simulated_target = data;
                }
                Ok(())
            }
        }
    }

    #[inline(always)]
    pub fn debug_read(&mut self, addr: u16) -> Result<u8> {
        return Ok(*(self.resolve_addr(addr, false, false)?.unwrap_memory()));
    }

    #[inline(always)]
    pub fn debug_read16(&mut self, addr: u16) -> Result<u16> {
        Ok((self.debug_read(addr)? as u16) | (self.debug_read(addr + 1)? as u16) << 8)
    }
}
