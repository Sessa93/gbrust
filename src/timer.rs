use serde::{Deserialize, Serialize};

// ─── GBC Timer ─────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize)]
pub struct GbcTimer {
    pub div: u16,     // Internal 16-bit counter (DIV = upper 8 bits)
    pub tima: u8,     // Timer counter
    pub tma: u8,      // Timer modulo
    pub tac: u8,      // Timer control
}

impl GbcTimer {
    pub fn new() -> Self {
        Self {
            div: 0,
            tima: 0,
            tma: 0,
            tac: 0,
        }
    }

    pub fn tick(&mut self, cycles: u32) -> bool {
        let mut irq = false;
        for _ in 0..cycles {
            let old_div = self.div;
            self.div = self.div.wrapping_add(1);

            if self.tac & 0x04 != 0 {
                let bit = match self.tac & 0x03 {
                    0 => 9,  // 4096 Hz
                    1 => 3,  // 262144 Hz
                    2 => 5,  // 65536 Hz
                    3 => 7,  // 16384 Hz
                    _ => 9,
                };
                // Falling edge detection
                let old_bit = (old_div >> bit) & 1;
                let new_bit = (self.div >> bit) & 1;
                if old_bit == 1 && new_bit == 0 {
                    let (new_tima, overflow) = self.tima.overflowing_add(1);
                    if overflow {
                        self.tima = self.tma;
                        irq = true;
                    } else {
                        self.tima = new_tima;
                    }
                }
            }
        }
        irq
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF04 => (self.div >> 8) as u8,
            0xFF05 => self.tima,
            0xFF06 => self.tma,
            0xFF07 => self.tac | 0xF8,
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0xFF04 => self.div = 0,
            0xFF05 => self.tima = val,
            0xFF06 => self.tma = val,
            0xFF07 => self.tac = val & 0x07,
            _ => {}
        }
    }
}

// ─── GBA Timers (4 hardware timers) ────────────────────

#[derive(Clone, Serialize, Deserialize)]
pub struct GbaTimer {
    pub counter: u16,
    pub reload: u16,
    pub control: u16,
    pub prescaler: u16,
    pub internal_counter: u32,
    pub enabled: bool,
    pub cascade: bool,
    pub irq_enabled: bool,
}

impl GbaTimer {
    pub fn new() -> Self {
        Self {
            counter: 0,
            reload: 0,
            control: 0,
            prescaler: 1,
            internal_counter: 0,
            enabled: false,
            cascade: false,
            irq_enabled: false,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GbaTimers {
    pub timers: [GbaTimer; 4],
}

impl GbaTimers {
    pub fn new() -> Self {
        Self {
            timers: [
                GbaTimer::new(),
                GbaTimer::new(),
                GbaTimer::new(),
                GbaTimer::new(),
            ],
        }
    }

    pub fn tick(&mut self, cycles: u32) -> u16 {
        let mut irqs = 0u16;
        let mut overflow = [false; 4];

        for i in 0..4 {
            if !self.timers[i].enabled {
                continue;
            }

            if self.timers[i].cascade && i > 0 {
                if overflow[i - 1] {
                    let (new_val, overflowed) = self.timers[i].counter.overflowing_add(1);
                    if overflowed {
                        self.timers[i].counter = self.timers[i].reload;
                        overflow[i] = true;
                        if self.timers[i].irq_enabled {
                            irqs |= 1 << (3 + i);
                        }
                    } else {
                        self.timers[i].counter = new_val;
                    }
                }
                continue;
            }

            self.timers[i].internal_counter += cycles as u32;
            let prescaler = self.timers[i].prescaler as u32;

            while self.timers[i].internal_counter >= prescaler {
                self.timers[i].internal_counter -= prescaler;
                let (new_val, overflowed) = self.timers[i].counter.overflowing_add(1);
                if overflowed {
                    self.timers[i].counter = self.timers[i].reload;
                    overflow[i] = true;
                    if self.timers[i].irq_enabled {
                        irqs |= 1 << (3 + i);
                    }
                } else {
                    self.timers[i].counter = new_val;
                }
            }
        }

        irqs
    }

    pub fn read(&self, offset: u32) -> u8 {
        let timer_idx = ((offset - 0x100) / 4) as usize;
        let reg = (offset - 0x100) % 4;

        if timer_idx >= 4 {
            return 0;
        }

        match reg {
            0 => self.timers[timer_idx].counter as u8,
            1 => (self.timers[timer_idx].counter >> 8) as u8,
            2 => self.timers[timer_idx].control as u8,
            3 => (self.timers[timer_idx].control >> 8) as u8,
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: u32, val: u8) {
        let timer_idx = ((offset - 0x100) / 4) as usize;
        let reg = (offset - 0x100) % 4;

        if timer_idx >= 4 {
            return;
        }

        match reg {
            0 => self.timers[timer_idx].reload = (self.timers[timer_idx].reload & 0xFF00) | val as u16,
            1 => self.timers[timer_idx].reload = (self.timers[timer_idx].reload & 0x00FF) | ((val as u16) << 8),
            2 => {
                let old_enabled = self.timers[timer_idx].enabled;
                let control = (self.timers[timer_idx].control & 0xFF00) | val as u16;
                self.timers[timer_idx].control = control;
                self.update_timer_control(timer_idx, old_enabled);
            }
            3 => {
                let old_enabled = self.timers[timer_idx].enabled;
                let control = (self.timers[timer_idx].control & 0x00FF) | ((val as u16) << 8);
                self.timers[timer_idx].control = control;
                self.update_timer_control(timer_idx, old_enabled);
            }
            _ => {}
        }
    }

    fn update_timer_control(&mut self, idx: usize, old_enabled: bool) {
        let control = self.timers[idx].control;
        self.timers[idx].enabled = control & 0x80 != 0;
        self.timers[idx].cascade = control & 0x04 != 0;
        self.timers[idx].irq_enabled = control & 0x40 != 0;
        self.timers[idx].prescaler = match control & 3 {
            0 => 1,
            1 => 64,
            2 => 256,
            3 => 1024,
            _ => 1,
        };

        // Reload counter on enable transition
        if !old_enabled && self.timers[idx].enabled {
            self.timers[idx].counter = self.timers[idx].reload;
            self.timers[idx].internal_counter = 0;
        }
    }
}
