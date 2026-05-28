

pub trait CpuMapper {
    fn cpu_map_addr(&self, addr: u16) -> u16; 
}
pub trait PpuMapper {
    fn ppu_map_addr(&self, addr: u16) -> u16; 
}

#[derive(Clone)]
pub struct Mapper0 {
    n_prg_banks: usize,
    n_chr_banks: usize,
}

impl Mapper0 {
    pub fn new(n_prg_banks: usize, n_chr_banks: usize) -> Self{
        Mapper0 { n_prg_banks, n_chr_banks }
    }
}

impl CpuMapper for Mapper0 {
    fn cpu_map_addr(&self, addr: u16) -> u16 {
        if addr < 0x8000 { return addr }

        return 0x8000 + (addr & if self.n_prg_banks > 1 {
            0x7FFF
        } else {
            0x3FFF
        })
    }
}

impl PpuMapper for Mapper0 {
    fn ppu_map_addr(&self, addr: u16) -> u16 {
        return addr;
    }
}