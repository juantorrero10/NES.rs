use core::f32;

use ringbuf::traits::Producer;

use crate::{AudioProducer, nes::Nes};

pub struct Apu {
    producer: AudioProducer,

    total_cycles: u32,
    cycles_per_sample: f64,
    cycle_accumulator: f64,

    frame_counter: u32,

    target_frequency: f32,

    sample_index: u32,
    sample_rate: u32,

    pulse_wave_chs: [PulseWaveChannel; 2],
}

#[derive(Default, Clone, Copy)]
struct PulseWaveChannel {
    enabled: bool,

    timer_reload: u16,
    timer_countdown: u16,
    length_counter_load: u8,
    settings: PWCSettings,
    sweep_unit: PWCSweep,
    sequence_step: usize,

    envelope_start: bool,
    envelope_volume: u8,
    envolope_divider: u8,
}

#[derive(Default, Clone, Copy)]
struct PWCSettings {
    duty: u8,
    length_counter_halt: bool,
    constant_volume: bool,
    envelope: u8,
}

#[derive(Default, Clone, Copy)]
struct PWCSweep {
    enabled: bool,
    negative: bool,
    period: u8,
    shift: u8,
    countdown: u8,
}

const NES_CPU_HZ: f64 = 1_789_773.0;

impl Apu {
    pub fn new(aup: AudioProducer, sample_rate: u32) -> Self {
        let cycles_per_sample = NES_CPU_HZ / (sample_rate as f64);
        Self {
            producer: aup,
            cycle_accumulator: 0.0,
            total_cycles: 0,
            frame_counter: 0,
            cycles_per_sample,
            sample_index: 0,
            sample_rate,
            target_frequency: 440.0,
            pulse_wave_chs: [PulseWaveChannel::default(); 2],
        }
    }

    // Runs at 1.79 Mhz
    pub fn clock(&mut self, _nes: *mut Nes) {
        self.total_cycles += 1;
        if self.total_cycles % 2 == 0 {
            self.sequencer_update();
        }

        if self.total_cycles % 2 == 0 {
            self.frame_counter += 1;
            match self.frame_counter {
                3729 | 11186 => self.envelope_update(),
                7457 | 14916 => {
                    self.envelope_update();
                    self.length_counter_update();
                    self.sweep_update();
                    if self.frame_counter == 14916 {
                        self.frame_counter = 0;
                    }
                }
                _ => {}
            }
        }

        self.cycle_accumulator += 1.0;
        if self.cycle_accumulator < self.cycles_per_sample {
            return;
        }

        self.cycle_accumulator -= self.cycles_per_sample;

        // 3. Sample the outputs from both pulse channels
        let sample_p1 = self.digital_pulse_wave_sample(&self.pulse_wave_chs[0]);
        let sample_p2 = self.digital_pulse_wave_sample(&self.pulse_wave_chs[1]);

        // High pass / Low pass mixer formula placeholder.
        // For now, simple linear blending will get you clear audio:
        let mixed_sample = 95.88 / ((8192.0 / (sample_p1 + sample_p2)) + 100.0);

        // Push down to the lockless consumer buffer
        let _ = self.producer.try_push(mixed_sample);

        self.sample_index += 1;
    }

    fn digital_pulse_wave_sample(&self, ch: &PulseWaveChannel) -> f32 {


        // Gating
        if !ch.enabled || !(8..=0x7ff).contains(&ch.timer_reload) || ch.length_counter_load == 0 {
            return 0.0;
        }

        // Standard hardcoded NES sequencer duty matrices
        let duty_sequences = [
            [0, 1, 0, 0, 0, 0, 0, 0], // 12.5% Duty
            [0, 1, 1, 0, 0, 0, 0, 0], // 25% Duty
            [0, 1, 1, 1, 1, 0, 0, 0], // 50% Duty
            [1, 0, 0, 1, 1, 1, 1, 1], // 25% Inverted Duty
        ];

        let current_duty = ch.settings.duty as usize;
        if duty_sequences[current_duty][ch.sequence_step] == 1 {
            // Output volume normalized between 0.0 and 1.0
            (if ch.settings.constant_volume {
                ch.settings.envelope
            } else {
                ch.envelope_volume
            }) as f32
        } else {
            0.0
        }
    }

    #[inline]
    fn sequencer_update(&mut self) {
        for ch in self.pulse_wave_chs.iter_mut() {
            if ch.timer_countdown == 0 {
                ch.timer_countdown = ch.timer_reload + 1;
                ch.sequence_step = (ch.sequence_step + 1) % 8;
            } else {
                ch.timer_countdown -= 1;
            }
        }
    }

    // Called at roughtly 240Hz
    fn sweep_update(&mut self) {
        for i in 0..2 {
            let ch = &mut self.pulse_wave_chs[i];

            if ch.sweep_unit.countdown != 0 {
                ch.sweep_unit.countdown -= 1;
                continue;
            }
            ch.sweep_unit.countdown = ch.sweep_unit.period + 1;

            if !ch.sweep_unit.enabled || ch.sweep_unit.shift == 0 {
                continue;
            }

            let delta = (ch.timer_reload >> ch.sweep_unit.shift) as i32;

            // Promote the integers to prevent wrap around
            let target_timer: i32 = if ch.sweep_unit.negative {
                ch.timer_reload as i32 - delta - if i == 0 { 1 } else { 0 }
            } else {
                ch.timer_reload as i32 + delta
            };

            //dbg!(ch.timer_reload, delta, target_timer, ch.sweep_unit.shift);

            if !(8..=0x7FF).contains(&target_timer) {
                ch.length_counter_load = 0;
            } else {
                ch.timer_reload = target_timer as u16;
            }
        }
    }

    #[inline]
    fn length_counter_update(&mut self) {
        for ch in &mut self.pulse_wave_chs {
            if !ch.settings.length_counter_halt && ch.length_counter_load > 0 {
                ch.length_counter_load -= 1;
            }
        }
    }

    fn envelope_update(&mut self) {
        for ch in &mut self.pulse_wave_chs {
            if ch.envelope_start {
                ch.envelope_start = false;
                ch.envelope_volume = 15;
                ch.envolope_divider = ch.settings.envelope;
                continue;
            }

            if ch.envolope_divider != 0 {
                ch.envolope_divider -= 1;
                continue;
            }
            ch.envolope_divider = ch.settings.envelope;
            if ch.envelope_volume > 0 {
                ch.envelope_volume -= 1;
            } else if ch.settings.length_counter_halt {
                ch.envelope_volume = 15;
            }
        }
    }

    fn modify_pulse_wave_channel(&mut self, mut no: usize, mut byte_off: u8, data: u8) {
        no &= 0x1;
        byte_off &= 0x3;
        let ch = &mut self.pulse_wave_chs[no];
        match byte_off {
            // Settings
            0 => {
                ch.settings = PWCSettings {
                    duty: (data & 0xC0) >> 6,
                    length_counter_halt: data & 0x20 != 0,
                    constant_volume: data & 0x10 != 0,
                    envelope: data & 0x0F,
                }
            }
            1 => {
                let p = (data & 0x70) >> 4;
                ch.sweep_unit = PWCSweep {
                    enabled: data & 0x80 != 0,
                    negative: data & 0x08 != 0,
                    period: p,
                    shift: data & 0x7,
                    countdown: p + 1,
                }
            }
            2 => {
                ch.timer_reload = (ch.timer_reload & 0x0700) | data as u16;
            }
            3 => {
                ch.timer_reload = (data as u16 & 0x7) << 8 | ch.timer_reload & 0x00FF;

                // Hardcoded NES length counter conversion mapping table
                let length_table = [
                    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18,
                    48, 20, 96, 22, 192, 24, 72, 26, 16, 28, 32, 30,
                ];
                let len_idx = ((data & !0x7) >> 3) as usize;
                ch.length_counter_load = length_table[len_idx];

                ch.sequence_step = 0;

                ch.sweep_unit.countdown = ch.sweep_unit.period + 1;

                // Reset envelope
                ch.envelope_start = true;
                ch.envelope_volume = 15;
                ch.envolope_divider = ch.settings.envelope;
            }
            _ => unreachable!(),
        }
    }

    pub fn event_cpu_has_written(&mut self, addr: u16, data: u8) {
        match addr {
            0x4000..=0x4003 => self.modify_pulse_wave_channel(0, (addr as u8) & 0x3, data),
            0x4004..=0x4007 => self.modify_pulse_wave_channel(1, (addr as u8) & 0x3, data),
            0x4015 => {
                let enable_pwc1 = data & 0x01 != 0;
                let enable_pwc2 = data & 0x02 != 0;
                self.pulse_wave_chs[0].enabled = enable_pwc1;
                self.pulse_wave_chs[1].enabled = enable_pwc2;

                if !enable_pwc1 {
                    self.pulse_wave_chs[0].length_counter_load = 0;
                }

                if !enable_pwc2 {
                    self.pulse_wave_chs[1].length_counter_load = 0;
                }
            }
            _ => {}
        }
    }

    pub fn event_cpu_has_read(&self, addr: u16) -> u8 {
        if addr != 0x4015 {
            return 0;
        }

        let mut ret = 0;

        if self.pulse_wave_chs[0].length_counter_load > 0 {
            ret |= 0x01;
        }

        if self.pulse_wave_chs[1].length_counter_load > 0 {
            ret |= 0x02;
        }

        ret
    }
}
