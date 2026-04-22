use serde::{Deserialize, Serialize};

use super::gbc_apu::{NoiseChannel, SquareChannel, WaveChannel};

/// GBA APU extends the GBC APU with two DMA sound channels (DirectSound A & B)
#[derive(Clone, Serialize, Deserialize)]
pub struct GbaApu {
    pub enabled: bool,
    pub ch1: SquareChannel,
    pub ch2: SquareChannel,
    pub ch3: WaveChannel,
    pub ch4: NoiseChannel,

    pub nr50: u8,
    pub nr51: u8,

    // GBA specific: SOUNDCNT_H
    pub soundcnt_h: u16,

    // DirectSound FIFOs
    pub fifo_a: Vec<i8>,
    pub fifo_b: Vec<i8>,
    pub fifo_a_sample: i8,
    pub fifo_b_sample: i8,

    // SOUNDBIAS
    pub soundbias: u16,

    pub psg_cycle_accum: u32,
    pub frame_sequencer: u32,
    pub frame_step: u8,
    pub sample_counter: u32,
    pub audio_buffer: Vec<f32>,
    pub sample_rate: u32,
}

impl GbaApu {
    pub fn new() -> Self {
        Self {
            enabled: true,
            ch1: SquareChannel::new(),
            ch2: SquareChannel::new(),
            ch3: WaveChannel::new(),
            ch4: NoiseChannel::new(),
            nr50: 0x77,
            nr51: 0xF3,
            soundcnt_h: 0,
            fifo_a: Vec::with_capacity(32),
            fifo_b: Vec::with_capacity(32),
            fifo_a_sample: 0,
            fifo_b_sample: 0,
            soundbias: 0x200,
            psg_cycle_accum: 0,
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

        // The legacy PSG runs at 4.194304 MHz on GBA, while the CPU runs at 16.777216 MHz.
        self.psg_cycle_accum += cycles;
        while self.psg_cycle_accum >= 4 {
            self.psg_cycle_accum -= 4;
            self.ch1.tick();
            self.ch2.tick();
            self.ch3.tick();
            self.ch4.tick();
        }

        self.frame_sequencer += cycles;
        while self.frame_sequencer >= 32_768 {
            self.frame_sequencer -= 32_768;
            self.clock_frame_sequencer();
        }

        // Downsample from the 16.78 MHz master clock.
        self.sample_counter = self.sample_counter.wrapping_add(cycles.saturating_mul(self.sample_rate));
        while self.sample_counter >= 16_777_216 {
            self.sample_counter -= 16_777_216;
            self.generate_sample();
        }
    }

    fn clock_frame_sequencer(&mut self) {
        match self.frame_step {
            0 | 2 | 4 | 6 => {
                self.ch1.clock_length();
                self.ch2.clock_length();
                self.ch3.clock_length();
                self.ch4.clock_length();
                if self.frame_step == 2 || self.frame_step == 6 {
                    self.ch1.clock_sweep();
                }
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
        // PSG channels
        let psg_ch1 = self.ch1.output();
        let psg_ch2 = self.ch2.output();
        let psg_ch3 = self.ch3.output();
        let psg_ch4 = self.ch4.output();

        let psg_vol = match self.soundcnt_h & 3 {
            0 => 0.25,
            1 => 0.5,
            2 => 1.0,
            _ => 0.0,
        };

        let psg_left_vol = ((self.nr50 >> 4) & 7) as f32 / 7.0;
        let psg_right_vol = (self.nr50 & 7) as f32 / 7.0;

        let mut psg_left = 0.0f32;
        let mut psg_right = 0.0f32;

        if self.nr51 & 0x10 != 0 { psg_left += psg_ch1; }
        if self.nr51 & 0x20 != 0 { psg_left += psg_ch2; }
        if self.nr51 & 0x40 != 0 { psg_left += psg_ch3; }
        if self.nr51 & 0x80 != 0 { psg_left += psg_ch4; }

        if self.nr51 & 0x01 != 0 { psg_right += psg_ch1; }
        if self.nr51 & 0x02 != 0 { psg_right += psg_ch2; }
        if self.nr51 & 0x04 != 0 { psg_right += psg_ch3; }
        if self.nr51 & 0x08 != 0 { psg_right += psg_ch4; }

        psg_left *= psg_left_vol * 0.25 * psg_vol;
        psg_right *= psg_right_vol * 0.25 * psg_vol;

        // DirectSound channels
        let fifo_a_vol = if self.soundcnt_h & 0x04 != 0 { 1.0 } else { 0.5 };
        let fifo_b_vol = if self.soundcnt_h & 0x08 != 0 { 1.0 } else { 0.5 };

        let fifo_a = (self.fifo_a_sample as f32 / 128.0) * fifo_a_vol;
        let fifo_b = (self.fifo_b_sample as f32 / 128.0) * fifo_b_vol;

        let mut left = psg_left;
        let mut right = psg_right;

        if self.soundcnt_h & 0x0200 != 0 { left += fifo_a; }
        if self.soundcnt_h & 0x0100 != 0 { right += fifo_a; }
        if self.soundcnt_h & 0x2000 != 0 { left += fifo_b; }
        if self.soundcnt_h & 0x1000 != 0 { right += fifo_b; }

        // Clamp
        left = left.clamp(-1.0, 1.0);
        right = right.clamp(-1.0, 1.0);

        self.audio_buffer.push(left);
        self.audio_buffer.push(right);
    }

    pub fn timer_overflow(&mut self, timer_id: usize) {
        // FIFO A uses timer specified in bit 10
        let fifo_a_timer = if self.soundcnt_h & 0x0400 != 0 { 1 } else { 0 };
        if timer_id == fifo_a_timer {
            self.fifo_a_sample = if !self.fifo_a.is_empty() {
                self.fifo_a.remove(0)
            } else {
                0
            };
        }

        // FIFO B uses timer specified in bit 14
        let fifo_b_timer = if self.soundcnt_h & 0x4000 != 0 { 1 } else { 0 };
        if timer_id == fifo_b_timer {
            self.fifo_b_sample = if !self.fifo_b.is_empty() {
                self.fifo_b.remove(0)
            } else {
                0
            };
        }
    }

    pub fn write_fifo(&mut self, fifo: usize, val: u32) {
        let target = if fifo == 0 { &mut self.fifo_a } else { &mut self.fifo_b };
        if target.len() < 32 {
            target.push(val as i8);
            target.push((val >> 8) as i8);
            target.push((val >> 16) as i8);
            target.push((val >> 24) as i8);
        }
    }

    pub fn read_io(&self, offset: u32) -> u8 {
        match offset {
            0x060 => {
                (self.ch1.sweep_period << 4)
                    | if self.ch1.sweep_negate { 0x08 } else { 0 }
                    | self.ch1.sweep_shift
            }
            0x062 => (self.ch1.duty << 6) | 0x3F,
            0x063 => {
                (self.ch1.volume_initial << 4)
                    | if self.ch1.envelope_add { 0x08 } else { 0 }
                    | self.ch1.envelope_period
            }
            0x065 => (if self.ch1.length_enabled { 0x40 } else { 0 }) | 0xBF,

            0x068 => (self.ch2.duty << 6) | 0x3F,
            0x069 => {
                (self.ch2.volume_initial << 4)
                    | if self.ch2.envelope_add { 0x08 } else { 0 }
                    | self.ch2.envelope_period
            }
            0x06D => (if self.ch2.length_enabled { 0x40 } else { 0 }) | 0xBF,

            0x070 => (if self.ch3.dac_enabled { 0x80 } else { 0 }) | 0x7F,
            0x073 => (self.ch3.volume_code << 5) | 0x9F,
            0x075 => (if self.ch3.length_enabled { 0x40 } else { 0 }) | 0xBF,

            0x079 => {
                (self.ch4.volume_initial << 4)
                    | if self.ch4.envelope_add { 0x08 } else { 0 }
                    | self.ch4.envelope_period
            }
            0x07C => {
                (self.ch4.clock_shift << 4)
                    | if self.ch4.width_mode { 0x08 } else { 0 }
                    | self.ch4.divisor_code
            }
            0x07D => (if self.ch4.length_enabled { 0x40 } else { 0 }) | 0xBF,

            0x080 => self.nr50,
            0x081 => self.nr51,
            0x082 => self.soundcnt_h as u8,
            0x083 => (self.soundcnt_h >> 8) as u8,
            0x084 => {
                let mut val = if self.enabled { 0x80 } else { 0 };
                if self.ch1.enabled { val |= 0x01; }
                if self.ch2.enabled { val |= 0x02; }
                if self.ch3.enabled { val |= 0x04; }
                if self.ch4.enabled { val |= 0x08; }
                val | 0x70
            }

            0x088 => self.soundbias as u8,
            0x089 => (self.soundbias >> 8) as u8,

            0x090..=0x09F => self.ch3.wave_ram[(offset - 0x090) as usize],

            _ => 0,
        }
    }

    pub fn write_io(&mut self, offset: u32, val: u8) {
        match offset {
            // Channel 1 (sweep)
            0x060 => {
                self.ch1.sweep_period = (val >> 4) & 7;
                self.ch1.sweep_negate = val & 0x08 != 0;
                self.ch1.sweep_shift = val & 7;
            }
            0x062 => {
                self.ch1.duty = (val >> 6) & 3;
                self.ch1.length_counter = 64 - (val & 0x3F);
            }
            0x063 => {
                self.ch1.volume_initial = val >> 4;
                self.ch1.envelope_add = val & 0x08 != 0;
                self.ch1.envelope_period = val & 7;
            }
            0x064 => self.ch1.frequency = (self.ch1.frequency & 0x700) | val as u16,
            0x065 => {
                self.ch1.frequency = (self.ch1.frequency & 0xFF) | (((val & 7) as u16) << 8);
                self.ch1.length_enabled = val & 0x40 != 0;
                if val & 0x80 != 0 { self.ch1.trigger(); }
            }

            // Channel 2
            0x068 => {
                self.ch2.duty = (val >> 6) & 3;
                self.ch2.length_counter = 64 - (val & 0x3F);
            }
            0x069 => {
                self.ch2.volume_initial = val >> 4;
                self.ch2.envelope_add = val & 0x08 != 0;
                self.ch2.envelope_period = val & 7;
            }
            0x06C => self.ch2.frequency = (self.ch2.frequency & 0x700) | val as u16,
            0x06D => {
                self.ch2.frequency = (self.ch2.frequency & 0xFF) | (((val & 7) as u16) << 8);
                self.ch2.length_enabled = val & 0x40 != 0;
                if val & 0x80 != 0 { self.ch2.trigger(); }
            }

            // Channel 3 (wave)
            0x070 => {
                self.ch3.dac_enabled = val & 0x80 != 0;
                if !self.ch3.dac_enabled { self.ch3.enabled = false; }
            }
            0x072 => self.ch3.length_counter = 256 - val as u16,
            0x073 => self.ch3.volume_code = (val >> 5) & 3,
            0x074 => self.ch3.frequency = (self.ch3.frequency & 0x700) | val as u16,
            0x075 => {
                self.ch3.frequency = (self.ch3.frequency & 0xFF) | (((val & 7) as u16) << 8);
                self.ch3.length_enabled = val & 0x40 != 0;
                if val & 0x80 != 0 { self.ch3.trigger(); }
            }

            // Channel 4 (noise)
            0x078 => self.ch4.length_counter = 64 - (val & 0x3F),
            0x079 => {
                self.ch4.volume_initial = val >> 4;
                self.ch4.envelope_add = val & 0x08 != 0;
                self.ch4.envelope_period = val & 7;
            }
            0x07C => {
                self.ch4.clock_shift = val >> 4;
                self.ch4.width_mode = val & 0x08 != 0;
                self.ch4.divisor_code = val & 7;
            }
            0x07D => {
                self.ch4.length_enabled = val & 0x40 != 0;
                if val & 0x80 != 0 { self.ch4.trigger(); }
            }

            // Sound control
            0x080 => self.nr50 = val,
            0x081 => self.nr51 = val,
            0x082 => self.soundcnt_h = (self.soundcnt_h & 0xFF00) | val as u16,
            0x083 => {
                self.soundcnt_h = (self.soundcnt_h & 0x00FF) | ((val as u16) << 8);
                // Reset FIFOs if bits set
                if val & 0x08 != 0 {
                    self.fifo_a.clear();
                    self.fifo_a_sample = 0;
                }
                if val & 0x80 != 0 {
                    self.fifo_b.clear();
                    self.fifo_b_sample = 0;
                }
            }
            0x084 => {
                let was_enabled = self.enabled;
                self.enabled = val & 0x80 != 0;
                if was_enabled && !self.enabled {
                    self.ch1 = SquareChannel::new();
                    self.ch2 = SquareChannel::new();
                    self.ch3.enabled = false;
                    self.ch4 = NoiseChannel::new();
                    self.nr50 = 0;
                    self.nr51 = 0;
                }
            }

            0x088 => self.soundbias = (self.soundbias & 0xFF00) | val as u16,
            0x089 => self.soundbias = (self.soundbias & 0x00FF) | ((val as u16) << 8),

            0x090..=0x09F => self.ch3.wave_ram[(offset - 0x090) as usize] = val,

            // FIFO writes
            0x0A0..=0x0A3 => {
                if self.fifo_a.len() < 32 { self.fifo_a.push(val as i8); }
            }
            0x0A4..=0x0A7 => {
                if self.fifo_b.len() < 32 { self.fifo_b.push(val as i8); }
            }

            _ => {}
        }
    }
}
