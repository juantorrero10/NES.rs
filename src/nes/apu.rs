use core::f32;

use ringbuf::traits::Producer;

use crate::{
    AudioProducer,
    nes::{Nes, NesError},
};

pub struct Apu {
    producer: AudioProducer,

    total_cycles: u32,
    cycles_per_sample: f64,
    cycle_accumulator: f64,

    frame_counter: u32,

    sample_rate: u32,

    frame_irq_signal: bool,
    frame_irq_inhibit: bool,
    sequencer_mode: bool,

    pulse_wave_chs: [PulseWaveChannel; 2],
    triangle_ch: TriangleWaveChannel,
    noise_ch: NoiseChannel,
    dmc_ch: DmcChannel,

    irq_pending: bool,

    hpf90: HighPassFilter,
    lpf21k: LowPassFilter,
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

#[derive(Default, Clone, Copy)]
struct TriangleWaveChannel {
    enabled: bool,
    timer_reload: u16,
    timer_countdown: u16,
    sequence_step: usize,

    settings: TWCSettings,

    // Length Counter
    length_counter_load: u8,

    // The Linear Counter (Specific to Triangle only!)
    linear_counter_value: u8,
    linear_counter_reload_flag: bool,
}

#[derive(Default, Clone, Copy)]
struct TWCSettings {
    length_counter_halt: bool,
    linear_counter_load: u8,
}

#[derive(Clone, Copy)]
struct NoiseChannel {
    enabled: bool,
    settings: NCSettings,
    noise_mode: bool,
    noise_period: u16,

    timer_countdown: u16,
    length_counter_load: u8,

    shift_register: u16,
    envelope_start: bool,
    envelope_volume: u8,
    envolope_divider: u8,
}

impl Default for NoiseChannel {
    fn default() -> Self {
        Self {
            enabled: false,
            settings: NCSettings::default(),
            noise_mode: false,
            noise_period: 0,
            timer_countdown: 0,
            length_counter_load: 0,
            shift_register: 1,
            envelope_start: false,
            envelope_volume: 0,
            envolope_divider: 0,
        }
    }
}

#[derive(Default, Clone, Copy)]
struct NCSettings {
    length_counter_halt: bool,
    constant_volume: bool,
    envelope: u8,
}

#[derive(Default, Clone, Copy)]
struct DmcChannel {
    enabled: bool,

    // Flags from $4010
    irq_enable: bool,
    loop_flag: bool,
    frequency: u8,

    // Timer registers
    timer_reload: u16,
    timer_countdown: u16,

    // Memory Reader registers
    sample_address: u16,  // The base address calculated from $4012
    sample_length: u16,   // The total length calculated from $4013
    current_address: u16, // Where the reader is currently looking
    bytes_remaining: u16, // How many bytes are left to fetch

    // Audio Buffer & Shift Register
    sample_buffer: Option<u8>, // None = empty, Some(byte) = full
    shift_register: u8,
    bits_remaining: u8, // Ticks down from 8 to 0
    silence_flag: bool,

    load_counter: u8,

    // Separate signal from the frame counter IRQ to distinguish them in code.
    irq_signal: bool,
}

const LENGTH_LUT: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];

const NOISE_PERIOD_LUT: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];

const DMC_PERIOD_LUT: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54,
];

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
            sample_rate,
            frame_irq_inhibit: false,
            frame_irq_signal: false,
            sequencer_mode: false,
            pulse_wave_chs: [PulseWaveChannel::default(); 2],
            triangle_ch: TriangleWaveChannel::default(),
            noise_ch: NoiseChannel::default(),
            dmc_ch: DmcChannel::default(),

            irq_pending: false,

            hpf90: HighPassFilter::new(sample_rate, 90.0),
            lpf21k: LowPassFilter::new(sample_rate, 21000.0),
        }
    }

    // Runs at 1.79 Mhz
    pub fn clock(&mut self, nes: *mut Nes) {
        self.total_cycles += 1;

        self.sequencer_update();
        self.dmc_update(nes).unwrap();

        if self.total_cycles % 2 == 0 {
            self.frame_counter += 1;
            // 4 step
            match self.frame_counter {
                3729 | 11186 => {
                    self.envelope_update();
                    self.linear_counter_update();
                }
                7457 | 14916 => {
                    self.envelope_update();
                    self.linear_counter_update();
                    self.length_counter_update();
                    self.sweep_update();
                    if self.frame_counter == 14916 {
                        self.frame_counter = 0;
                        if !self.frame_irq_inhibit && !self.sequencer_mode {
                            if !self.frame_irq_signal {
                                self.frame_irq_signal = true;
                                self.irq_pending = true;
                            }
                        }
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
        let sample_t = self.digital_triangle_wave_sample();
        let sample_n = self.digital_noise_sample();
        let sample_d = if self.dmc_ch.enabled {
            self.dmc_ch.load_counter as f32
        } else {
            0.0
        };

        // The sample produced at this stage is purely positive (DC-offset).
        // Very hard to work with since it will clip as soon as some gain is applied
        let mixed_sample = Self::mix_samples(sample_p1, sample_p2, sample_t, sample_n, sample_d);

        // The Nes audio chip applies some filters to introduce the simmetry in the waveform
        // Applying the same filters as the original hardware produces very muffled audio.
        // I've only kept the 90Hz hpf and ignored the 440Hz hpf.
        // I also moved the original 14kHz lpf to 21KHz to maintain the original gritt.
        // They solve the DC-offset problem and make the sound accurate enough.

        // Apply 90Hz HPf (cut off base)
        let mut filtered = self.hpf90.apply(mixed_sample);
        // Apply 21KHz Lpf (barely a difference)
        filtered = self.lpf21k.apply(filtered);

        // Make-up gain
        filtered *= 2.0;

        // Push down to the lockless consumer buffer
        let _ = self.producer.try_push(filtered);
    }

    fn mix_samples(pw1: f32, pw2: f32, triangle: f32, noise: f32, dmc: f32) -> f32 {
        let pulses = 95.88 / ((8192.0 / (pw1 + pw2)) + 100.0);
        let rest =
            159.79 / (1.0 / ((triangle / 8227.0) + (noise / 12241.0) + (dmc / 22638.0)) + 100.0);
        pulses + rest
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
            (if ch.settings.constant_volume {
                ch.settings.envelope
            } else {
                ch.envelope_volume
            }) as f32
        } else {
            0.0
        }
    }

    fn digital_triangle_wave_sample(&self) -> f32 {
        let ch = &self.triangle_ch;

        if !ch.enabled
            || ch.timer_reload < 2
            || ch.length_counter_load == 0
            || ch.linear_counter_value == 0
        {
            return 0.0;
        }

        // Fixed hardcoded 32-step triangle waveform sequencer steps
        const TRIANGLE_SEQUENCE: [u8; 32] = [
            15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
            11, 12, 13, 14, 15,
        ];

        // Return the raw sequence step value (0 to 15)
        TRIANGLE_SEQUENCE[ch.sequence_step] as f32
    }

    fn digital_noise_sample(&self) -> f32 {
        let ch = &self.noise_ch;

        if !ch.enabled || ch.length_counter_load == 0 {
            return 0.0;
        }

        if ch.shift_register & 0x1 == 0 {
            (if ch.settings.constant_volume {
                ch.settings.envelope
            } else {
                ch.envelope_volume
            }) as f32
        } else {
            0.0
        }
    }

    fn sequencer_update(&mut self) {
        if self.total_cycles % 2 == 0 {
            // PW Channels
            for ch in self.pulse_wave_chs.iter_mut() {
                if ch.timer_countdown == 0 {
                    ch.timer_countdown = ch.timer_reload + 1;
                    ch.sequence_step = (ch.sequence_step + 1) % 8;
                } else {
                    ch.timer_countdown -= 1;
                }
            }
        }
        // Noise channel
        let ch = &mut self.noise_ch;
        if ch.timer_countdown == 0 {
            ch.timer_countdown = ch.noise_period + 1;
            let shift_amount = if ch.noise_mode { 6 } else { 1 };
            let feedback = (ch.shift_register & 0x1) ^ ((ch.shift_register >> shift_amount) & 0x1);

            ch.shift_register >>= 1;
            ch.shift_register |= feedback << 14;
        } else {
            ch.timer_countdown -= 1;
        }
        // Triangle channel
        let ch = &mut self.triangle_ch;
        if ch.timer_countdown == 0 {
            ch.timer_countdown = ch.timer_reload + 1;

            if ch.length_counter_load > 0 && ch.linear_counter_value > 0 && ch.timer_reload >= 2 {
                ch.sequence_step = (ch.sequence_step + 1) % 32;
            }
        } else {
            ch.timer_countdown -= 1;
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
            if !ch.settings.length_counter_halt && ch.length_counter_load > 0 && ch.enabled {
                ch.length_counter_load -= 1;
            }
        }
        let nch = &mut self.noise_ch;
        if !nch.settings.length_counter_halt && nch.length_counter_load > 0 && nch.enabled {
            nch.length_counter_load -= 1;
        }
    }

    fn linear_counter_update(&mut self) {
        let ch = &mut self.triangle_ch;

        if ch.linear_counter_reload_flag {
            ch.linear_counter_value = ch.settings.linear_counter_load;
        } else if ch.linear_counter_value > 0 {
            ch.linear_counter_value -= 1;
        }

        // If the control flag (length_counter_halt) is clear,
        // the reload flag clears itself every step so the counter can tick down.
        if !ch.settings.length_counter_halt {
            ch.linear_counter_reload_flag = false;
        }
    }

    #[inline]
    fn channel_envelope_update(
        start: &mut bool,
        volume: &mut u8,
        divider: &mut u8,
        divider_reload: u8,
        lch: bool,
    ) {
        if *start {
            *start = false;
            *volume = 15;
            *divider = divider_reload;
            return;
        }

        if *divider != 0 {
            *divider -= 1;
            return;
        }
        *divider = divider_reload;
        if *volume > 0 {
            *volume -= 1;
        } else if lch {
            *volume = 15;
        }
    }

    fn envelope_update(&mut self) {
        // PW Envelope update
        for ch in &mut self.pulse_wave_chs {
            Self::channel_envelope_update(
                &mut ch.envelope_start,
                &mut ch.envelope_volume,
                &mut ch.envolope_divider,
                ch.settings.envelope,
                ch.settings.length_counter_halt,
            );
        }
        // Noise channel
        let ch = &mut self.noise_ch;
        Self::channel_envelope_update(
            &mut ch.envelope_start,
            &mut ch.envelope_volume,
            &mut ch.envolope_divider,
            ch.settings.envelope,
            ch.settings.length_counter_halt,
        );
    }

    fn dmc_update(&mut self, nes: *mut Nes) -> Result<(), NesError> {
        let nes = unsafe { nes.as_mut().unwrap() };
        let ch = &mut self.dmc_ch;

        if !ch.enabled {
            return Ok(());
        }

        if ch.sample_buffer.is_none() && ch.bytes_remaining > 0 {
            let data = nes.read(ch.current_address)?;
            ch.sample_buffer = Some(data);
            // Stal CPU 4 cycles
            nes.cpu.state.cycles_left += 4;

            if ch.current_address == 0xFFFF {
                ch.current_address = 0x8000;
            } else {
                ch.current_address += 1;
            }

            ch.bytes_remaining -= 1;
            if ch.bytes_remaining == 0 {
                if ch.loop_flag {
                    // Restart sample playback automatically if loop is enabled
                    ch.current_address = ch.sample_address;
                    ch.bytes_remaining = ch.sample_length;
                } else if ch.irq_enable && !ch.irq_signal {
                    // Fire the IRQ edge trigger if loops are disabled
                    ch.irq_signal = true;
                    self.irq_pending = true;
                }
            }
        }

        if ch.timer_countdown != 0 {
            ch.timer_countdown -= 1;
            return Ok(());
        }

        ch.timer_countdown = ch.timer_reload + 1;
        if !ch.silence_flag {
            let bit = ch.shift_register & 0x1;

            if bit == 1 {
                if ch.load_counter <= 125 {
                    ch.load_counter += 2;
                }
            } else {
                if ch.load_counter >= 2 {
                    ch.load_counter -= 2;
                }
            }
        }

        ch.shift_register >>= 1;
        ch.bits_remaining = ch.bits_remaining.saturating_sub(1);

        if ch.bits_remaining == 0 {
            ch.bits_remaining = 8;
            match ch.sample_buffer {
                Some(b) => {
                    ch.sample_buffer = None;
                    ch.shift_register = b;
                    ch.silence_flag = false;
                }
                None => {
                    ch.silence_flag = true;
                }
            }
        }

        Ok(())
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
                let len_idx = ((data & !0x7) >> 3) as usize;
                ch.length_counter_load = LENGTH_LUT[len_idx];

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

    fn modify_triangle_channel(&mut self, mut byte_off: u8, data: u8) {
        byte_off &= 0x3;
        match byte_off {
            0 => {
                self.triangle_ch.settings = TWCSettings {
                    length_counter_halt: (data & 0x80) != 0,
                    linear_counter_load: data & 0x7f,
                };
            }
            1 => {}
            2 => {
                self.triangle_ch.timer_reload =
                    (self.triangle_ch.timer_reload & 0x0700) | data as u16;
            }
            3 => {
                let len_idx = ((data & !0x7) >> 3) as usize;
                self.triangle_ch.length_counter_load = LENGTH_LUT[len_idx];
                self.triangle_ch.timer_reload =
                    (self.triangle_ch.timer_reload & 0x00FF) | (data as u16 & 0x7) << 8;

                self.triangle_ch.linear_counter_reload_flag = true;
            }
            _ => unreachable!(),
        }
    }

    fn modify_noise_channel(&mut self, mut byte_off: u8, data: u8) {
        let ch = &mut self.noise_ch;
        byte_off &= 0x3;
        match byte_off {
            0 => {
                ch.settings = NCSettings {
                    length_counter_halt: (data & 0x20) != 0,
                    constant_volume: (data & 0x10) != 0,
                    envelope: (data & 0x0F),
                };
            }
            1 => {}
            2 => {
                ch.noise_mode = data & 0x80 != 0;
                ch.noise_period = NOISE_PERIOD_LUT[(data & 0xF) as usize];
            }
            3 => {
                let len_idx = ((data & !0x7) >> 3) as usize;
                ch.length_counter_load = LENGTH_LUT[len_idx];

                // Reset envelope
                ch.envelope_start = true;
                ch.envelope_volume = 15;
                ch.envolope_divider = ch.settings.envelope;
            }
            _ => unreachable!(),
        }
    }

    fn modify_dmc_channel(&mut self, mut byte_off: u8, data: u8) {
        byte_off &= 0x3;
        let ch = &mut self.dmc_ch;
        match byte_off {
            0 => {
                ch.irq_enable = (data & 0x80) != 0;
                ch.loop_flag = (data & 0x40) != 0;

                ch.timer_reload = DMC_PERIOD_LUT[(data & 0x0F) as usize];

                if !ch.irq_enable {
                    ch.irq_signal = false;
                }
            }
            1 => {
                ch.load_counter = (data & 0x7f) >> 1;
            }
            2 => {
                ch.sample_address = 0xC000 | (data as u16) << 6;
            }
            3 => {
                ch.sample_length = (data as u16) << 4 | 1;
            }
            _ => unreachable!(),
        }
    }

    pub fn event_cpu_has_written(&mut self, addr: u16, data: u8) {
        match addr {
            0x4000..=0x4003 => self.modify_pulse_wave_channel(0, (addr as u8) & 0x3, data),
            0x4004..=0x4007 => self.modify_pulse_wave_channel(1, (addr as u8) & 0x3, data),
            0x4008..=0x400B => self.modify_triangle_channel((addr as u8) & 0x3, data),
            0x400C..=0x400F => self.modify_noise_channel((addr as u8) & 0x3, data),
            0x4010..=0x4013 => self.modify_dmc_channel((addr as u8) & 0x3, data),
            0x4015 => {
                let enable_pwc1 = data & 0x01 != 0;
                let enable_pwc2 = data & 0x02 != 0;
                let enable_trig = data & 0x04 != 0;
                let enable_noise = data & 0x08 != 0;
                let enable_dmc = data & 0x10 != 0;
                self.pulse_wave_chs[0].enabled = enable_pwc1;
                self.pulse_wave_chs[1].enabled = enable_pwc2;
                self.triangle_ch.enabled = enable_trig;
                self.noise_ch.enabled = enable_noise;
                self.dmc_ch.enabled = enable_dmc;

                if !enable_pwc1 {
                    self.pulse_wave_chs[0].length_counter_load = 0;
                }

                if !enable_pwc2 {
                    self.pulse_wave_chs[1].length_counter_load = 0;
                }

                if !enable_trig {
                    self.triangle_ch.length_counter_load = 0;
                }

                if !enable_noise {
                    self.noise_ch.length_counter_load = 0;
                }

                if !enable_dmc {
                    self.dmc_ch.bytes_remaining = 0;
                } else if self.dmc_ch.bytes_remaining == 0 {
                    self.dmc_ch.current_address = self.dmc_ch.sample_address;
                    self.dmc_ch.bytes_remaining = self.dmc_ch.sample_length;
                }

                self.dmc_ch.irq_signal = false;
            }
            0x4017 => {
                self.sequencer_mode = (data & 0x80) != 0;
                if self.sequencer_mode {
                    self.length_counter_update();
                }
                self.frame_irq_inhibit = (data & 0x40) != 0;
            }
            _ => {}
        }
    }

    pub fn event_cpu_has_read(&mut self, addr: u16) -> u8 {
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

        if self.triangle_ch.length_counter_load > 0 {
            ret |= 0x04;
        }

        if self.noise_ch.length_counter_load > 0 {
            ret |= 0x08;
        }

        if self.dmc_ch.bytes_remaining > 0 {
            ret |= 0x10;
        }

        if self.frame_irq_signal {
            ret |= 0x40;
            self.frame_irq_signal = false;
        }

        if self.dmc_ch.irq_signal {
            ret |= 0x80;
        }

        ret
    }

    // Expose an IRQ line to the CPU, (Rising edge)
    #[inline(always)]
    pub fn irq_pending(&mut self) -> bool {
        if self.irq_pending {
            self.irq_pending = false;
            return true;
        }
        false
    }
}

struct HighPassFilter {
    coeff: f32,
    prev_sample: f32,
    prev_input: f32,
}

impl HighPassFilter {
    pub fn new(sample_rate: u32, cutoff_freq: f32) -> Self {
        let delta_time = 1.0 / sample_rate as f32;
        let rc = 1.0 / (2.0 * f32::consts::PI * cutoff_freq);

        Self {
            coeff: rc / (rc + delta_time),
            prev_sample: 0.0,
            prev_input: 0.0
        }
    }

    pub fn apply(&mut self, input: f32) -> f32 {
        self.prev_sample = self.coeff * (self.prev_sample + input - self.prev_input);
        self.prev_input = input;
        self.prev_sample
    }
}

struct LowPassFilter {
    coeff: f32,
    prev_sample: f32,
}

impl LowPassFilter {
    pub fn new(sample_rate: u32, cutoff_freq: f32) -> Self {
        let delta_time = 1.0 / sample_rate as f32;
        let rc = 1.0 / (2.0 * f32::consts::PI * cutoff_freq);

        Self {
            coeff: delta_time / (rc + delta_time),
            prev_sample: 0.0,
        }
    }

    pub fn apply(&mut self, input: f32) -> f32 {
        self.prev_sample += self.coeff * (input - self.prev_sample);
        self.prev_sample
    }
}
