use std::{cell::RefCell, rc::Rc};

use crate::nes::{bus_cpu::BusCpu, cpu::Cpu};

pub mod bus_cpu;
pub mod cpu;
pub mod err;
pub mod instruction_set;

pub struct Nes {
    pub cpu: Cpu,
    pub bus: Rc<RefCell<BusCpu>>,
}

impl Nes {
    pub fn new() -> Self {
        let bus = Rc::new(RefCell::new(BusCpu::new()));
        Nes {
            bus: bus.clone(),
            cpu: Cpu::new(bus.clone()),
        }
    }
}
