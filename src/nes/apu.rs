use core::f32;

use ringbuf::traits::{Observer, Producer};

use crate::{AudioProducer, nes::Nes};

pub struct Apu {
    producer: AudioProducer,
    sample_rate: u32,

    cycles_per_sample: f64,
    cycle_accumulator: f64,
    cycle_per_frame: u64,

    sample_index: u64,
    target_frequency: f32,
}

const NES_CPU_HZ: f64 = 1_789_773.0;

impl Apu {
    pub fn new(aup: AudioProducer, sample_rate: u32) -> Self {
        let cycles_per_sample = NES_CPU_HZ / (sample_rate as f64);
        Self {
            producer: aup,
            sample_rate,
            cycle_accumulator: 0.0,
            cycles_per_sample,
            cycle_per_frame: 0,
            sample_index: 0,
            target_frequency: 440.0,
        }
    }


    pub fn clock(&mut self, _nes: *mut Nes) {

        self.cycle_accumulator += 1.0;
        self.cycle_per_frame += 1;

        if self.cycle_accumulator < self.cycles_per_sample {
            return;
        }

        self.cycle_accumulator -= self.cycles_per_sample;

        let time = (self.sample_index as f32) / (self.sample_rate as f32);
        let phase = 2.0 * f32::consts::PI * (self.target_frequency as f32) * time;
        let sample = 0.2 * phase.sin();

        while self.producer.try_push(sample).is_err() {
            // Spin loop or hint to CPU to avoid burning core power
            std::hint::spin_loop();
        }
        
        // log::info!("{}", self.producer.occupied_len());
        
        self.sample_index += 1;
    }

    pub fn event_cpu_has_written(&self, addr: u16, data: u8) {}

    pub fn event_cpu_has_read(&self, addr: u16) -> u8 {
        0
    }
}
