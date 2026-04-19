use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct DmaChannel {
    pub src_addr: u32,
    pub dst_addr: u32,
    pub count: u16,
    pub control: u16,
    pub enabled: bool,
    pub active: bool,
    pub repeat: bool,
    pub word_size: bool,   // false=16bit, true=32bit
    pub timing: u8,        // 0=immediate, 1=vblank, 2=hblank, 3=special
    pub irq: bool,
    pub src_control: u8,   // 0=inc, 1=dec, 2=fixed, 3=prohibited
    pub dst_control: u8,   // 0=inc, 1=dec, 2=fixed, 3=inc/reload
    // Internal latch
    pub src_latch: u32,
    pub dst_latch: u32,
    pub count_latch: u16,
}

impl DmaChannel {
    pub fn new() -> Self {
        Self {
            src_addr: 0,
            dst_addr: 0,
            count: 0,
            control: 0,
            enabled: false,
            active: false,
            repeat: false,
            word_size: false,
            timing: 0,
            irq: false,
            src_control: 0,
            dst_control: 0,
            src_latch: 0,
            dst_latch: 0,
            count_latch: 0,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GbaDma {
    pub channels: [DmaChannel; 4],
}

impl GbaDma {
    pub fn new() -> Self {
        Self {
            channels: [
                DmaChannel::new(),
                DmaChannel::new(),
                DmaChannel::new(),
                DmaChannel::new(),
            ],
        }
    }

    pub fn read(&self, offset: u32) -> u8 {
        let ch = ((offset - 0xB0) / 12) as usize;
        let reg = (offset - 0xB0) % 12;

        if ch >= 4 {
            return 0;
        }

        match reg {
            // Source, destination are write-only
            0..=7 => 0,
            // Count is write-only
            8..=9 => 0,
            // Control
            10 => self.channels[ch].control as u8,
            11 => (self.channels[ch].control >> 8) as u8,
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: u32, val: u8) {
        let ch = ((offset - 0xB0) / 12) as usize;
        let reg = (offset - 0xB0) % 12;

        if ch >= 4 {
            return;
        }

        match reg {
            0 => self.channels[ch].src_addr = (self.channels[ch].src_addr & 0xFFFFFF00) | val as u32,
            1 => self.channels[ch].src_addr = (self.channels[ch].src_addr & 0xFFFF00FF) | ((val as u32) << 8),
            2 => self.channels[ch].src_addr = (self.channels[ch].src_addr & 0xFF00FFFF) | ((val as u32) << 16),
            3 => self.channels[ch].src_addr = (self.channels[ch].src_addr & 0x00FFFFFF) | ((val as u32) << 24),
            4 => self.channels[ch].dst_addr = (self.channels[ch].dst_addr & 0xFFFFFF00) | val as u32,
            5 => self.channels[ch].dst_addr = (self.channels[ch].dst_addr & 0xFFFF00FF) | ((val as u32) << 8),
            6 => self.channels[ch].dst_addr = (self.channels[ch].dst_addr & 0xFF00FFFF) | ((val as u32) << 16),
            7 => self.channels[ch].dst_addr = (self.channels[ch].dst_addr & 0x00FFFFFF) | ((val as u32) << 24),
            8 => self.channels[ch].count = (self.channels[ch].count & 0xFF00) | val as u16,
            9 => self.channels[ch].count = (self.channels[ch].count & 0x00FF) | ((val as u16) << 8),
            10 => {
                self.channels[ch].control = (self.channels[ch].control & 0xFF00) | val as u16;
                self.update_control(ch);
            }
            11 => {
                let old_enable = self.channels[ch].enabled;
                self.channels[ch].control = (self.channels[ch].control & 0x00FF) | ((val as u16) << 8);
                self.update_control(ch);

                // If newly enabled, latch values and maybe start immediately
                if !old_enable && self.channels[ch].enabled {
                    self.channels[ch].src_latch = self.channels[ch].src_addr;
                    self.channels[ch].dst_latch = self.channels[ch].dst_addr;
                    self.channels[ch].count_latch = self.channels[ch].count;

                    if self.channels[ch].timing == 0 {
                        self.channels[ch].active = true;
                    }
                }
            }
            _ => {}
        }
    }

    fn update_control(&mut self, ch: usize) {
        let ctrl = self.channels[ch].control;
        self.channels[ch].dst_control = ((ctrl >> 5) & 3) as u8;
        self.channels[ch].src_control = ((ctrl >> 7) & 3) as u8;
        self.channels[ch].repeat = ctrl & 0x0200 != 0;
        self.channels[ch].word_size = ctrl & 0x0400 != 0;
        self.channels[ch].timing = ((ctrl >> 12) & 3) as u8;
        self.channels[ch].irq = ctrl & 0x4000 != 0;
        self.channels[ch].enabled = ctrl & 0x8000 != 0;
    }

    pub fn notify_vblank(&mut self) {
        for ch in &mut self.channels {
            if ch.enabled && ch.timing == 1 {
                ch.active = true;
            }
        }
    }

    pub fn notify_hblank(&mut self) {
        for ch in &mut self.channels {
            if ch.enabled && ch.timing == 2 {
                ch.active = true;
            }
        }
    }
}
