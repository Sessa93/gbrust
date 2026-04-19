use serde::{Deserialize, Serialize};

const DUTY_TABLE: [[u8; 8]; 4] = [
    [0, 0, 0, 0, 0, 0, 0, 1], // 12.5%
    [1, 0, 0, 0, 0, 0, 0, 1], // 25%
    [1, 0, 0, 0, 0, 1, 1, 1], // 50%
    [0, 1, 1, 1, 1, 1, 1, 0], // 75%
];

#[derive(Clone, Serialize, Deserialize)]
pub struct SquareChannel {
    pub enabled: bool,
    pub duty: u8,
    pub length_counter: u8,
    pub length_enabled: bool,
    pub volume: u8,
    pub volume_initial: u8,
    pub envelope_add: bool,
    pub envelope_period: u8,
    pub envelope_timer: u8,
    pub frequency: u16,
    pub freq_timer: u16,
    pub duty_pos: u8,
    // Sweep (channel 1 only)
    pub sweep_period: u8,
    pub sweep_negate: bool,
    pub sweep_shift: u8,
    pub sweep_timer: u8,
    pub sweep_enabled: bool,
    pub sweep_shadow: u16,
}

impl SquareChannel {
    pub fn new() -> Self {
        Self {
            enabled: false,
            duty: 0,
            length_counter: 0,
            length_enabled: false,
            volume: 0,
            volume_initial: 0,
            envelope_add: false,
            envelope_period: 0,
            envelope_timer: 0,
            frequency: 0,
            freq_timer: 0,
            duty_pos: 0,
            sweep_period: 0,
            sweep_negate: false,
            sweep_shift: 0,
            sweep_timer: 0,
            sweep_enabled: false,
            sweep_shadow: 0,
        }
    }

    pub fn tick(&mut self) {
        if self.freq_timer == 0 {
            self.freq_timer = (2048 - self.frequency) * 4;
            self.duty_pos = (self.duty_pos + 1) & 7;
        }
        self.freq_timer = self.freq_timer.saturating_sub(1);
    }

    pub fn output(&self) -> f32 {
        if !self.enabled || self.volume == 0 {
            return 0.0;
        }
        let sample = DUTY_TABLE[self.duty as usize][self.duty_pos as usize];
        (sample as f32) * (self.volume as f32) / 15.0
    }

    pub fn clock_length(&mut self) {
        if self.length_enabled && self.length_counter > 0 {
            self.length_counter -= 1;
            if self.length_counter == 0 {
                self.enabled = false;
            }
        }
    }

    pub fn clock_envelope(&mut self) {
        if self.envelope_period == 0 {
            return;
        }
        self.envelope_timer = self.envelope_timer.saturating_sub(1);
        if self.envelope_timer == 0 {
            self.envelope_timer = self.envelope_period;
            if self.envelope_add && self.volume < 15 {
                self.volume += 1;
            } else if !self.envelope_add && self.volume > 0 {
                self.volume -= 1;
            }
        }
    }

    pub fn clock_sweep(&mut self) {
        if !self.sweep_enabled || self.sweep_period == 0 {
            return;
        }
        self.sweep_timer = self.sweep_timer.saturating_sub(1);
        if self.sweep_timer == 0 {
            self.sweep_timer = if self.sweep_period > 0 { self.sweep_period } else { 8 };
            let new_freq = self.calc_sweep_freq();
            if new_freq <= 2047 && self.sweep_shift > 0 {
                self.frequency = new_freq;
                self.sweep_shadow = new_freq;
                // Check again
                if self.calc_sweep_freq() > 2047 {
                    self.enabled = false;
                }
            } else if new_freq > 2047 {
                self.enabled = false;
            }
        }
    }

    fn calc_sweep_freq(&self) -> u16 {
        let shifted = self.sweep_shadow >> self.sweep_shift;
        if self.sweep_negate {
            self.sweep_shadow.wrapping_sub(shifted)
        } else {
            self.sweep_shadow.wrapping_add(shifted)
        }
    }

    pub fn trigger(&mut self) {
        self.enabled = true;
        if self.length_counter == 0 {
            self.length_counter = 64;
        }
        self.freq_timer = (2048 - self.frequency) * 4;
        self.volume = self.volume_initial;
        self.envelope_timer = self.envelope_period;
        // Sweep
        self.sweep_shadow = self.frequency;
        self.sweep_timer = if self.sweep_period > 0 { self.sweep_period } else { 8 };
        self.sweep_enabled = self.sweep_period > 0 || self.sweep_shift > 0;
        if self.sweep_shift > 0 && self.calc_sweep_freq() > 2047 {
            self.enabled = false;
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct WaveChannel {
    pub enabled: bool,
    pub dac_enabled: bool,
    pub length_counter: u16,
    pub length_enabled: bool,
    pub volume_code: u8,
    pub frequency: u16,
    pub freq_timer: u16,
    pub wave_ram: [u8; 16],
    pub wave_pos: u8,
    pub sample_buffer: u8,
}

impl WaveChannel {
    pub fn new() -> Self {
        Self {
            enabled: false,
            dac_enabled: false,
            length_counter: 0,
            length_enabled: false,
            volume_code: 0,
            frequency: 0,
            freq_timer: 0,
            wave_ram: [0; 16],
            wave_pos: 0,
            sample_buffer: 0,
        }
    }

    pub fn tick(&mut self) {
        if self.freq_timer == 0 {
            self.freq_timer = (2048 - self.frequency) * 2;
            self.wave_pos = (self.wave_pos + 1) & 31;
            let byte = self.wave_ram[(self.wave_pos / 2) as usize];
            self.sample_buffer = if self.wave_pos & 1 == 0 {
                (byte >> 4) & 0xF
            } else {
                byte & 0xF
            };
        }
        self.freq_timer = self.freq_timer.saturating_sub(1);
    }

    pub fn output(&self) -> f32 {
        if !self.enabled || !self.dac_enabled {
            return 0.0;
        }
        let shift = match self.volume_code {
            0 => 4, // Mute
            1 => 0, // 100%
            2 => 1, // 50%
            3 => 2, // 25%
            _ => 4,
        };
        let sample = self.sample_buffer >> shift;
        (sample as f32) / 15.0
    }

    pub fn clock_length(&mut self) {
        if self.length_enabled && self.length_counter > 0 {
            self.length_counter -= 1;
            if self.length_counter == 0 {
                self.enabled = false;
            }
        }
    }

    pub fn trigger(&mut self) {
        self.enabled = true;
        if self.length_counter == 0 {
            self.length_counter = 256;
        }
        self.freq_timer = (2048 - self.frequency) * 2;
        self.wave_pos = 0;
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct NoiseChannel {
    pub enabled: bool,
    pub length_counter: u8,
    pub length_enabled: bool,
    pub volume: u8,
    pub volume_initial: u8,
    pub envelope_add: bool,
    pub envelope_period: u8,
    pub envelope_timer: u8,
    pub clock_shift: u8,
    pub width_mode: bool,
    pub divisor_code: u8,
    pub freq_timer: u16,
    pub lfsr: u16,
}

impl NoiseChannel {
    pub fn new() -> Self {
        Self {
            enabled: false,
            length_counter: 0,
            length_enabled: false,
            volume: 0,
            volume_initial: 0,
            envelope_add: false,
            envelope_period: 0,
            envelope_timer: 0,
            clock_shift: 0,
            width_mode: false,
            divisor_code: 0,
            freq_timer: 0,
            lfsr: 0x7FFF,
        }
    }

    pub fn tick(&mut self) {
        if self.freq_timer == 0 {
            let divisor = match self.divisor_code {
                0 => 8,
                n => (n as u16) * 16,
            };
            self.freq_timer = divisor << self.clock_shift;

            let xor = (self.lfsr & 1) ^ ((self.lfsr >> 1) & 1);
            self.lfsr = (self.lfsr >> 1) | (xor << 14);
            if self.width_mode {
                self.lfsr &= !(1 << 6);
                self.lfsr |= xor << 6;
            }
        }
        self.freq_timer = self.freq_timer.saturating_sub(1);
    }

    pub fn output(&self) -> f32 {
        if !self.enabled || self.volume == 0 {
            return 0.0;
        }
        let bit = (!self.lfsr & 1) as f32;
        bit * (self.volume as f32) / 15.0
    }

    pub fn clock_length(&mut self) {
        if self.length_enabled && self.length_counter > 0 {
            self.length_counter -= 1;
            if self.length_counter == 0 {
                self.enabled = false;
            }
        }
    }

    pub fn clock_envelope(&mut self) {
        if self.envelope_period == 0 {
            return;
        }
        self.envelope_timer = self.envelope_timer.saturating_sub(1);
        if self.envelope_timer == 0 {
            self.envelope_timer = self.envelope_period;
            if self.envelope_add && self.volume < 15 {
                self.volume += 1;
            } else if !self.envelope_add && self.volume > 0 {
                self.volume -= 1;
            }
        }
    }

    pub fn trigger(&mut self) {
        self.enabled = true;
        if self.length_counter == 0 {
            self.length_counter = 64;
        }
        self.volume = self.volume_initial;
        self.envelope_timer = self.envelope_period;
        self.lfsr = 0x7FFF;
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GbcApu {
    pub enabled: bool,
    pub ch1: SquareChannel,
    pub ch2: SquareChannel,
    pub ch3: WaveChannel,
    pub ch4: NoiseChannel,

    pub nr50: u8, // Volume control
    pub nr51: u8, // Panning

    pub frame_sequencer: u32,
    pub frame_step: u8,
    pub sample_counter: u32,

    pub audio_buffer: Vec<f32>,
    pub sample_rate: u32,
}

impl GbcApu {
    pub fn new() -> Self {
        Self {
            enabled: true,
            ch1: SquareChannel::new(),
            ch2: SquareChannel::new(),
            ch3: WaveChannel::new(),
            ch4: NoiseChannel::new(),
            nr50: 0x77,
            nr51: 0xF3,
            frame_sequencer: 0,
            frame_step: 0,
            sample_counter: 0,
            audio_buffer: Vec::with_capacity(4096),
            sample_rate: 44100,
        }
    }

    pub fn tick(&mut self, cycles: u32) {
        if !self.enabled {
            return;
        }

        for _ in 0..cycles {
            self.ch1.tick();
            self.ch2.tick();
            self.ch3.tick();
            self.ch4.tick();

            // Frame sequencer at 512 Hz (CPU_CLOCK / 8192)
            self.frame_sequencer += 1;
            if self.frame_sequencer >= 8192 {
                self.frame_sequencer = 0;
                self.clock_frame_sequencer();
            }

            // Downsample to output sample rate
            self.sample_counter += self.sample_rate;
            if self.sample_counter >= 4_194_304 {
                self.sample_counter -= 4_194_304;
                self.generate_sample();
            }
        }
    }

    fn clock_frame_sequencer(&mut self) {
        match self.frame_step {
            0 => {
                self.ch1.clock_length();
                self.ch2.clock_length();
                self.ch3.clock_length();
                self.ch4.clock_length();
            }
            2 => {
                self.ch1.clock_length();
                self.ch2.clock_length();
                self.ch3.clock_length();
                self.ch4.clock_length();
                self.ch1.clock_sweep();
            }
            4 => {
                self.ch1.clock_length();
                self.ch2.clock_length();
                self.ch3.clock_length();
                self.ch4.clock_length();
            }
            6 => {
                self.ch1.clock_length();
                self.ch2.clock_length();
                self.ch3.clock_length();
                self.ch4.clock_length();
                self.ch1.clock_sweep();
            }
            7 => {
                self.ch1.clock_envelope();
                self.ch2.clock_envelope();
                self.ch4.clock_envelope();
            }
            _ => {}
        }
        self.frame_step = (self.frame_step + 1) & 7;
    }

    fn generate_sample(&mut self) {
        let ch1 = self.ch1.output();
        let ch2 = self.ch2.output();
        let ch3 = self.ch3.output();
        let ch4 = self.ch4.output();

        let left_vol = ((self.nr50 >> 4) & 7) as f32 / 7.0;
        let right_vol = (self.nr50 & 7) as f32 / 7.0;

        let mut left = 0.0f32;
        let mut right = 0.0f32;

        if self.nr51 & 0x10 != 0 { left += ch1; }
        if self.nr51 & 0x20 != 0 { left += ch2; }
        if self.nr51 & 0x40 != 0 { left += ch3; }
        if self.nr51 & 0x80 != 0 { left += ch4; }

        if self.nr51 & 0x01 != 0 { right += ch1; }
        if self.nr51 & 0x02 != 0 { right += ch2; }
        if self.nr51 & 0x04 != 0 { right += ch3; }
        if self.nr51 & 0x08 != 0 { right += ch4; }

        left *= left_vol * 0.25;
        right *= right_vol * 0.25;

        self.audio_buffer.push(left);
        self.audio_buffer.push(right);
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF10 => {
                (self.ch1.sweep_period << 4)
                    | if self.ch1.sweep_negate { 0x08 } else { 0 }
                    | self.ch1.sweep_shift
            }
            0xFF11 => (self.ch1.duty << 6) | 0x3F,
            0xFF12 => {
                (self.ch1.volume_initial << 4)
                    | if self.ch1.envelope_add { 0x08 } else { 0 }
                    | self.ch1.envelope_period
            }
            0xFF13 => 0xFF, // Write-only
            0xFF14 => (if self.ch1.length_enabled { 0x40 } else { 0 }) | 0xBF,

            0xFF16 => (self.ch2.duty << 6) | 0x3F,
            0xFF17 => {
                (self.ch2.volume_initial << 4)
                    | if self.ch2.envelope_add { 0x08 } else { 0 }
                    | self.ch2.envelope_period
            }
            0xFF18 => 0xFF,
            0xFF19 => (if self.ch2.length_enabled { 0x40 } else { 0 }) | 0xBF,

            0xFF1A => (if self.ch3.dac_enabled { 0x80 } else { 0 }) | 0x7F,
            0xFF1B => 0xFF,
            0xFF1C => (self.ch3.volume_code << 5) | 0x9F,
            0xFF1D => 0xFF,
            0xFF1E => (if self.ch3.length_enabled { 0x40 } else { 0 }) | 0xBF,

            0xFF20 => 0xFF,
            0xFF21 => {
                (self.ch4.volume_initial << 4)
                    | if self.ch4.envelope_add { 0x08 } else { 0 }
                    | self.ch4.envelope_period
            }
            0xFF22 => {
                (self.ch4.clock_shift << 4)
                    | if self.ch4.width_mode { 0x08 } else { 0 }
                    | self.ch4.divisor_code
            }
            0xFF23 => (if self.ch4.length_enabled { 0x40 } else { 0 }) | 0xBF,

            0xFF24 => self.nr50,
            0xFF25 => self.nr51,
            0xFF26 => {
                let mut val = if self.enabled { 0x80 } else { 0 };
                if self.ch1.enabled { val |= 0x01; }
                if self.ch2.enabled { val |= 0x02; }
                if self.ch3.enabled { val |= 0x04; }
                if self.ch4.enabled { val |= 0x08; }
                val | 0x70
            }

            0xFF30..=0xFF3F => self.ch3.wave_ram[(addr - 0xFF30) as usize],

            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        if !self.enabled && addr != 0xFF26 && !(0xFF30..=0xFF3F).contains(&addr) {
            return;
        }

        match addr {
            0xFF10 => {
                self.ch1.sweep_period = (val >> 4) & 7;
                self.ch1.sweep_negate = val & 0x08 != 0;
                self.ch1.sweep_shift = val & 7;
            }
            0xFF11 => {
                self.ch1.duty = (val >> 6) & 3;
                self.ch1.length_counter = 64 - (val & 0x3F);
            }
            0xFF12 => {
                self.ch1.volume_initial = val >> 4;
                self.ch1.envelope_add = val & 0x08 != 0;
                self.ch1.envelope_period = val & 7;
            }
            0xFF13 => {
                self.ch1.frequency = (self.ch1.frequency & 0x700) | val as u16;
            }
            0xFF14 => {
                self.ch1.frequency = (self.ch1.frequency & 0xFF) | (((val & 7) as u16) << 8);
                self.ch1.length_enabled = val & 0x40 != 0;
                if val & 0x80 != 0 {
                    self.ch1.trigger();
                }
            }

            0xFF16 => {
                self.ch2.duty = (val >> 6) & 3;
                self.ch2.length_counter = 64 - (val & 0x3F);
            }
            0xFF17 => {
                self.ch2.volume_initial = val >> 4;
                self.ch2.envelope_add = val & 0x08 != 0;
                self.ch2.envelope_period = val & 7;
            }
            0xFF18 => {
                self.ch2.frequency = (self.ch2.frequency & 0x700) | val as u16;
            }
            0xFF19 => {
                self.ch2.frequency = (self.ch2.frequency & 0xFF) | (((val & 7) as u16) << 8);
                self.ch2.length_enabled = val & 0x40 != 0;
                if val & 0x80 != 0 {
                    self.ch2.trigger();
                }
            }

            0xFF1A => {
                self.ch3.dac_enabled = val & 0x80 != 0;
                if !self.ch3.dac_enabled {
                    self.ch3.enabled = false;
                }
            }
            0xFF1B => self.ch3.length_counter = 256 - val as u16,
            0xFF1C => self.ch3.volume_code = (val >> 5) & 3,
            0xFF1D => {
                self.ch3.frequency = (self.ch3.frequency & 0x700) | val as u16;
            }
            0xFF1E => {
                self.ch3.frequency = (self.ch3.frequency & 0xFF) | (((val & 7) as u16) << 8);
                self.ch3.length_enabled = val & 0x40 != 0;
                if val & 0x80 != 0 {
                    self.ch3.trigger();
                }
            }

            0xFF20 => self.ch4.length_counter = 64 - (val & 0x3F),
            0xFF21 => {
                self.ch4.volume_initial = val >> 4;
                self.ch4.envelope_add = val & 0x08 != 0;
                self.ch4.envelope_period = val & 7;
            }
            0xFF22 => {
                self.ch4.clock_shift = val >> 4;
                self.ch4.width_mode = val & 0x08 != 0;
                self.ch4.divisor_code = val & 7;
            }
            0xFF23 => {
                self.ch4.length_enabled = val & 0x40 != 0;
                if val & 0x80 != 0 {
                    self.ch4.trigger();
                }
            }

            0xFF24 => self.nr50 = val,
            0xFF25 => self.nr51 = val,
            0xFF26 => {
                let was_enabled = self.enabled;
                self.enabled = val & 0x80 != 0;
                if was_enabled && !self.enabled {
                    // Power off - reset all channels
                    self.ch1 = SquareChannel::new();
                    self.ch2 = SquareChannel::new();
                    self.ch3.enabled = false;
                    self.ch4 = NoiseChannel::new();
                    self.nr50 = 0;
                    self.nr51 = 0;
                }
            }

            0xFF30..=0xFF3F => {
                self.ch3.wave_ram[(addr - 0xFF30) as usize] = val;
            }

            _ => {}
        }
    }
}
