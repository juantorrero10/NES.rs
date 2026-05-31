use std::{error::Error, mem};

use thiserror::Error;

use crate::nes::{
    NesComm, NesEvent, cpu::Flag6502::D, mapper::CpuMapper, ppu::PpuRegister, trigger_read_event,
    trigger_write_event,
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

const ADDR_OPEN_BUS_START: u16 = 0x4020;
const ADDR_OPEN_BUS_END: u16 = 0x5FFF;

const ADDR_MAYBE_SRAM_START: u16 = 0x6000;
const ADDR_MAYBE_SRAM_END: u16 = 0x7FFF;

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
    CommunicationsNotConnected,
}

pub struct BusCpu {
    fake_value: u8,

    open_bus_sim_reg: u8,

    mapper: Box<dyn CpuMapper>,

    chips: BusCpuChips,
}

pub struct BusCpuChips {
    pub ram: [u8; SIZE_RAM],
    pub debug_ppu_reg: [u8; SIZE_PPU_REG],
    pub debug_controller_io: [u8; SIZE_IO],
    pub oam_transfer_buffer: [u8; 256],
    pub communications: Option<NesComm>,
}

type Result<T> = std::result::Result<T, BusCpuError>;

pub enum BusTarget<'a> {
    Memory(&'a mut u8),
    HardwareIntercept {
        // Some memory accesses dont lie behind clean containters. Instead they need a hardware device manage it.
        event: NesEvent,
        // Debuggers should not trigger the events but at the same time have the data updated
        // All interceptions requiere a mirrored simulated memory read/write in order to maintain them fresh.
        debug_simulated_target: *mut u8,
    },
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
    pub fn new(mapper: Box<dyn CpuMapper>) -> BusCpu {
        // TODO: Cart
        BusCpu {
            chips: BusCpuChips {
                ram: [0u8; SIZE_RAM],
                // Just debug, for dissasembler and memory viewer
                debug_ppu_reg: [0u8; SIZE_PPU_REG],
                debug_controller_io: [0u8; SIZE_IO],
                oam_transfer_buffer: [0u8; 256],
                communications: None,
            },

            open_bus_sim_reg: 0,

            mapper,

            fake_value: 0,
        }
    }

    pub fn add_communications(&mut self, comm: NesComm) {
        self.chips.communications = Some(comm);
    }

    fn bus_operation(
        &mut self,
        mut addr: u16,
        write: bool,
        data: u8,
        trigger_events: bool,
    ) -> Result<BusTarget> {
        let input_addr = addr;
        // Map address

        if self.mapper.need_intercept(addr) {
            return Ok(self.mapper.intercept(addr, &mut self.chips, write));
        }

        match input_addr {
            ADDR_RAM_START..=ADDR_RAM_END => {
                addr &= SIZE_RAM as u16 - 1;
                return Ok(Memory(
                    self.chips
                        .ram
                        .get_mut(addr as usize)
                        .ok_or(BusCpuError::AddrUnMirrored(input_addr))?,
                ));
            }
            ADDR_PPU_REG_START..=ADDR_PPU_REG_END => {
                addr &= SIZE_PPU_REG as u16 - 1;

                let debug_simulated_target = self
                    .chips
                    .debug_ppu_reg
                    .get_mut(addr as usize)
                    .ok_or(BusCpuError::AddrUnMirrored(input_addr))?;

                if trigger_events {
                    return if write {
                        Ok(HardwareIntercept {
                            event: NesEvent::PpuRegWrittenTo(PpuRegister::from_0_to_7(addr)),
                            debug_simulated_target,
                        })
                    } else {
                        Ok(HardwareIntercept {
                            event: NesEvent::PpuRegReadFrom(PpuRegister::from_0_to_7(addr)),
                            debug_simulated_target,
                        })
                    };
                }
                // Debug only
                Ok(Memory(debug_simulated_target))
            }
            ADDR_IO_START..=ADDR_IO_END => {
                macro_rules! get_simulated {
                    ($addr:expr) => {
                        self.chips
                            .debug_controller_io
                            .get_mut((addr & 0x1f) as usize)
                            .ok_or(BusCpuError::AddrUnMirrored(input_addr))?
                    };
                }
                if trigger_events {
                    if write {
                        return match addr {
                            0x4000..=0x4013 | 0x4015 | 0x4017 => Ok(HardwareIntercept {
                                event: NesEvent::ApuWrittenTo(addr),
                                debug_simulated_target: get_simulated!(addr),
                            }),
                            // OAM DMA transfer
                            0x4014 => {
                                let starting_address = (data as u16) << 8;
                                for i in 0..256 {
                                    let val = self.read(starting_address + i).unwrap();
                                    self.chips.oam_transfer_buffer[i as usize] = val;
                                }
                                Ok(HardwareIntercept {
                                    event: NesEvent::OAMDMATransfer(
                                        self.chips.oam_transfer_buffer.as_ptr(),
                                        256,
                                    ),
                                    debug_simulated_target: get_simulated!(addr),
                                })
                            }
                            0x4016 => Ok(HardwareIntercept {
                                event: NesEvent::ControllerWrittenTo,
                                debug_simulated_target: get_simulated!(addr),
                            }),
                            _ => Ok(Memory(&mut self.open_bus_sim_reg)),
                        };
                    } else {
                        return match addr {
                            0x4000..=0x4014 => {
                                self.fake_value = 0;
                                Ok(Memory(&mut self.fake_value))
                            }
                            0x4015 => Ok(HardwareIntercept {
                                event: NesEvent::ApuReadFrom(addr),
                                debug_simulated_target: get_simulated!(addr),
                            }),
                            0x4016 | 0x4017 => Ok(HardwareIntercept {
                                event: NesEvent::ControllerReadFrom(addr),
                                debug_simulated_target: get_simulated!(addr),
                            }),
                            _ => Ok(Memory(&mut self.open_bus_sim_reg)),
                        };
                    }
                }

                Ok(Memory(get_simulated!(addr)))
            }
            _ => Ok(Memory(&mut self.open_bus_sim_reg)),
        }
    }

    #[inline(always)]
    pub fn read(&mut self, addr: u16) -> Result<u8> {
        let data_ret;
        match self.bus_operation(addr, false, 0, true)? {
            Memory(r) => {
                data_ret = *r;
            }
            HardwareIntercept {
                event,
                debug_simulated_target,
            } => {
                let comms = self.chips.communications.as_ref().unwrap();
                let data = trigger_read_event(comms, event)?;
                unsafe {
                    *debug_simulated_target = data;
                }
                data_ret = data;
            }
        }
        self.open_bus_sim_reg = data_ret;
        Ok(data_ret)
    }

    /// Read 16bit number (little endian, LSB first)
    #[inline(always)]
    pub fn read16(&mut self, addr: u16) -> Result<u16> {
        Ok((self.read(addr)? as u16) | (self.read(addr + 1)? as u16) << 8)
    }

    #[inline(always)]
    pub fn write(&mut self, addr: u16, data: u8) -> Result<()> {
        match self.bus_operation(addr, true, data, true)? {
            Memory(r) => {
                *r = data;
                self.open_bus_sim_reg = data;
                Ok(())
            }
            HardwareIntercept {
                event,
                debug_simulated_target,
            } => {
                let comms = self.chips.communications.as_ref().unwrap();
                trigger_write_event(comms, event, data)?;
                unsafe {
                    *debug_simulated_target = data;
                }
                self.open_bus_sim_reg = data;
                Ok(())
            }
        }
    }

    #[inline(always)]
    pub fn debug_read(&mut self, addr: u16) -> Result<u8> {
        return Ok(*(self.bus_operation(addr, false, 0, false)?.unwrap_memory()));
    }

    #[inline(always)]
    pub fn debug_read16(&mut self, addr: u16) -> Result<u16> {
        Ok((self.debug_read(addr)? as u16) | (self.debug_read(addr + 1)? as u16) << 8)
    }
}
