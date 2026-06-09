const PRG_BANK_SIZE: usize = 16384;
const CHR_BANK_SIZE: usize = 8192;

const NES_MAGIC: u32 = 0x1a53454e; // "NES\x1a"

#[repr(u8)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MemoryMirroring {
    Horizontal,
    Vertical,
    OnScreenLO,
    OnScreenHI,
}

#[repr(packed)]
#[derive(Debug)]
struct NesGameHeader {
    magic: u32,
    prg_rom_chunks: u8,
    chr_rom_chunks: u8,
    flag6: u8,
    flag7: u8,
    prg_ram_size: u8,
    tv_system1: u8,
    tv_system2: u8,
    unused: [u8; 5],
}

type Result<T> = std::result::Result<T, ROMErr>;

#[derive(Error, Debug)]
pub enum ROMErr {
    #[error("Could not open file: {0}")]
    IoError(#[from] std::io::Error),
    #[error("{0:?} is not a NES ROM")]
    InvalidMagic(OsString),
    #[error("Mapper {0} is invalid or not supported by the emulator currently.")]
    MapperNotImplemented(u8),
    #[error("{0}")]
    PostCardError(#[from] postcard::Error),
}
use std::{
    ffi::OsString,
    fs::File,
    io::{Read, Seek},
    mem,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::nes::{
    game_rom::ROMErr::MapperNotImplemented,
    mappers::Mapper,
    mappers::{mmc0::MMC0, mmc3::MMC3},
};

pub fn load_rom(filepath: &OsString) -> Result<(u8, Box<dyn Mapper>)> {
    let mut f = File::open(filepath)?;
    let mut header = [0u8; size_of::<NesGameHeader>()];
    f.read(header.as_mut_slice())?;
    let header: NesGameHeader = unsafe { mem::transmute(header) };
    log::info!("Loaded {:?}:\n{:#?}", filepath, header);

    if header.magic != NES_MAGIC {
        return Err(ROMErr::InvalidMagic(filepath.to_owned()));
    }

    // Skip trainer if present
    if (header.flag6 & 0b0000_0100) != 0 {
        // Advance cursor by 512
        f.seek(std::io::SeekFrom::Current(512))?;
    }
    let mapper_id = (header.flag6 >> 4) | (header.flag7 & 0x0F);
    let mirror = if (header.flag6 & 1) != 0 {
        MemoryMirroring::Vertical
    } else {
        MemoryMirroring::Horizontal
    };
    log::info!("Mapper_id: {mapper_id}");
    log::info!("Mirroring: {mirror:?}");

    let mut prg_data = Vec::with_capacity(header.prg_rom_chunks as usize * PRG_BANK_SIZE);
    let mut chr_data = Vec::with_capacity(header.chr_rom_chunks as usize * CHR_BANK_SIZE);

    unsafe {
        prg_data.set_len(header.prg_rom_chunks as usize * PRG_BANK_SIZE);
        chr_data.set_len(header.chr_rom_chunks as usize * CHR_BANK_SIZE);
    }

    f.read_exact(prg_data.as_mut_slice())?;
    f.read_exact(chr_data.as_mut_slice())?;

    Ok((
        mapper_id,
        create_mapper(
            mapper_id,
            header.prg_ram_size as usize,
            prg_data,
            chr_data,
            mirror,
            header.flag6 & 0x02 != 0,
        )?,
    ))
}

#[allow(unused_variables)]
fn create_mapper(
    mapper_id: u8,
    prg_ram_size: usize,
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    mirroring: MemoryMirroring,
    battery_backed: bool,
) -> Result<Box<dyn Mapper>> {
    match mapper_id {
        0 => Ok(Box::new(MMC0::new(
            prg_rom,
            prg_ram_size,
            chr_rom,
            mirroring,
        ))),
        4 => Ok(Box::new(MMC3::new(
            prg_rom,
            prg_ram_size,
            chr_rom,
            mirroring,
            battery_backed,
        ))),
        _ => Err(MapperNotImplemented(mapper_id)),
    }
}

pub fn create_mapper_from_snapshot(
    id: u8,
    snap: &[u8],
) -> Result<Box<dyn Mapper>> {
    match id {
        0 => Ok(Box::new({ let m: MMC0 = postcard::from_bytes(snap)?; m })),
        4 => Ok(Box::new({ let m: MMC3 = postcard::from_bytes(snap)?; m })),
        _ => Err(MapperNotImplemented(id))
    }
}
