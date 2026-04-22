use serde::{Deserialize, Serialize};

use crate::apu::gba_apu::GbaApu;
use crate::cartridge::GbaCartridge;
use crate::cpu::arm7tdmi::Arm7Bus;
use crate::dma::GbaDma;
use crate::input::GbaInput;
use crate::ppu::gba_ppu::GbaPpu;
use crate::timer::GbaTimers;

pub const BIOS_SIZE: usize = 0x4000;

#[derive(Clone, Serialize, Deserialize)]
pub struct GbaBus {
    pub cart: GbaCartridge,
    pub ppu: GbaPpu,
    pub apu: GbaApu,
    pub timers: GbaTimers,
    pub dma: GbaDma,
    pub input: GbaInput,
    pub ewram: Vec<u8>,     // 256KB external work RAM
    pub iwram: Vec<u8>,     // 32KB internal work RAM
    pub io: Vec<u8>,        // IO registers
    pub ie: u16,
    pub iflag: u16,
    pub ime: bool,
    pub waitcnt: u16,
    pub halt: bool,
    pub post_boot: bool,
    pub bios: Vec<u8>,
    pub last_bios_value: u32,
}

impl GbaBus {
    pub fn new(cart: GbaCartridge) -> Self {
        Self {
            cart,
            ppu: GbaPpu::new(),
            apu: GbaApu::new(),
            timers: GbaTimers::new(),
            dma: GbaDma::new(),
            input: GbaInput::new(),
            ewram: vec![0; 0x40000],
            iwram: vec![0; 0x8000],
            io: vec![0; 0x400],
            ie: 0,
            iflag: 0,
            ime: false,
            waitcnt: 0,
            halt: false,
            post_boot: true,
            bios: Self::generate_hle_bios(),
            last_bios_value: 0,
        }
    }

    fn generate_hle_bios() -> Vec<u8> {
        let mut bios = vec![0u8; BIOS_SIZE];
        // Reset vector: Jump to cartridge at 0x08000000
        let reset: u32 = 0xE3A0_F302; // MOV PC, #0x08000000
        bios[0..4].copy_from_slice(&reset.to_le_bytes());
        // SWI handler at 0x08 - handled via HLE, fallback returns
        let movs_pc_lr: u32 = 0xE1B0_F00E; // MOVS PC, LR
        bios[0x08..0x0C].copy_from_slice(&movs_pc_lr.to_le_bytes());
        // IRQ handler at 0x18 - dispatch to user handler at [0x03007FFC]
        let irq_code: [u32; 9] = [
            0xE92D_500F, // STMFD SP!, {R0-R3, R12, LR}
            0xE3A0_0403, // MOV R0, #0x03000000
            0xE280_0C7F, // ADD R0, R0, #0x7F00
            0xE280_00FC, // ADD R0, R0, #0xFC
            0xE590_0000, // LDR R0, [R0]              ; load handler addr
            0xE1A0_E00F, // MOV LR, PC                ; return addr
            0xE12F_FF10, // BX R0                     ; call handler
            0xE8BD_500F, // LDMFD SP!, {R0-R3, R12, LR}
            0xE25E_F004, // SUBS PC, LR, #4           ; return from IRQ
        ];
        for (i, &word) in irq_code.iter().enumerate() {
            let offset = 0x18 + i * 4;
            bios[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        }
        bios
    }

    pub fn tick(&mut self, cycles: u32) {
        let ppu_events = self.ppu.tick(cycles);
        self.iflag |= ppu_events & 0x07; // Lower bits are IRQ flags

        // Trigger DMA on VBlank/HBlank events
        if ppu_events & 0x0100 != 0 {
            self.dma.notify_vblank();
        }
        if ppu_events & 0x0200 != 0 {
            self.dma.notify_hblank();
        }

        let (timer_irqs, timer_overflows) = self.timers.tick(cycles);
        self.iflag |= timer_irqs;

        // Propagate timer overflows to APU (advance DirectSound FIFO sample)
        // and trigger sound DMA channels so they refill the FIFOs.
        for i in 0..4usize {
            if timer_overflows & (1 << i) != 0 {
                self.apu.timer_overflow(i);
                self.trigger_sound_dma(i);
            }
        }

        self.apu.tick(cycles);

        // Process DMA
        self.process_dma();
    }

    /// Activate DMA1/DMA2 sound channels whose tied timer just overflowed.
    fn trigger_sound_dma(&mut self, timer_id: usize) {
        let fifo_a_timer = if self.apu.soundcnt_h & 0x0400 != 0 { 1usize } else { 0 };
        let fifo_b_timer = if self.apu.soundcnt_h & 0x4000 != 0 { 1usize } else { 0 };
        for ch in 1..=2usize {
            if self.dma.channels[ch].enabled && self.dma.channels[ch].timing == 3 {
                let dst = self.dma.channels[ch].dst_addr;
                let fifo_ready = if dst == 0x0400_00A0 {
                    timer_id == fifo_a_timer && self.apu.fifo_a.len() <= 16
                } else if dst == 0x0400_00A4 {
                    timer_id == fifo_b_timer && self.apu.fifo_b.len() <= 16
                } else {
                    false
                };
                if fifo_ready {
                    self.dma.channels[ch].active = true;
                }
            }
        }
    }

    fn process_dma(&mut self) {
        for ch in 0..4 {
            if !self.dma.channels[ch].active {
                continue;
            }
            // DirectSound DMA (ch 1/2, timing=3) always transfers exactly 4 words.
            let count = if (ch == 1 || ch == 2) && self.dma.channels[ch].timing == 3 {
                4u32
            } else {
                let c = self.dma.channels[ch].count as u32;
                if c == 0 {
                    if ch == 3 { 0x10000 } else { 0x4000 }
                } else {
                    c
                }
            };

            let word_size = if self.dma.channels[ch].word_size { 4u32 } else { 2 };
            let src_inc: i32 = match self.dma.channels[ch].src_control {
                0 => word_size as i32,
                1 => -(word_size as i32),
                2 => 0,
                _ => word_size as i32,
            };
            let dst_inc: i32 = match self.dma.channels[ch].dst_control {
                0 | 3 => word_size as i32,
                1 => -(word_size as i32),
                2 => 0,
                _ => word_size as i32,
            };

            let mut src = self.dma.channels[ch].src_addr;
            let mut dst = self.dma.channels[ch].dst_addr;

            if self.cart.backup_type == crate::cartridge::GbaBackupType::Eeprom
                && ((src >> 24 == 0x0D) || (dst >> 24 == 0x0D))
            {
                self.cart.notify_eeprom_dma(count);
            }

            for _ in 0..count {
                if word_size == 4 {
                    let val = self.read32_dma(src);
                    self.write32_dma(dst, val);
                } else {
                    let val = self.read16_dma(src);
                    self.write16_dma(dst, val);
                }
                src = (src as i64 + src_inc as i64) as u32;
                dst = (dst as i64 + dst_inc as i64) as u32;
            }

            self.dma.channels[ch].src_addr = src;
            if self.dma.channels[ch].dst_control != 3 {
                self.dma.channels[ch].dst_addr = dst;
            }

            if self.dma.channels[ch].repeat && self.dma.channels[ch].timing != 0 {
                // Wait for next VBlank/HBlank trigger
                self.dma.channels[ch].active = false;
                // Reload dst if dst_control is increment/reload
                if self.dma.channels[ch].dst_control == 3 {
                    self.dma.channels[ch].dst_addr = self.dma.channels[ch].dst_latch;
                }
            } else {
                self.dma.channels[ch].active = false;
                self.dma.channels[ch].enabled = false;
                // Clear enable bit in control register so games polling it see completion
                self.dma.channels[ch].control &= !0x8000;
            }

            if self.dma.channels[ch].irq {
                self.iflag |= 1 << (8 + ch);
            }
        }
    }

    fn read16_dma(&mut self, addr: u32) -> u16 {
        if addr >> 24 == 0x0D && self.cart.backup_type == crate::cartridge::GbaBackupType::Eeprom {
            let val = self.cart.eeprom_read() as u16;
            self.cart.eeprom_read_advance();
            return val;
        }
        self.read16(addr)
    }

    fn read32_dma(&mut self, addr: u32) -> u32 {
        if addr >> 24 == 0x0D && self.cart.backup_type == crate::cartridge::GbaBackupType::Eeprom {
            let val = self.cart.eeprom_read() as u32;
            self.cart.eeprom_read_advance();
            return val;
        }
        self.read32(addr)
    }

    fn write16_dma(&mut self, addr: u32, val: u16) {
        if addr >> 24 == 0x0D && self.cart.backup_type == crate::cartridge::GbaBackupType::Eeprom {
            self.cart.eeprom_write(val as u8);
            return;
        }
        self.write16(addr, val);
    }

    fn write32_dma(&mut self, addr: u32, val: u32) {
        if addr >> 24 == 0x0D && self.cart.backup_type == crate::cartridge::GbaBackupType::Eeprom {
            self.cart.eeprom_write(val as u8);
            return;
        }
        self.write32(addr, val);
    }

    pub fn check_irq(&self) -> bool {
        self.ime && (self.ie & self.iflag) != 0
    }

    fn read_io_reg(&self, offset: u32) -> u8 {
        let off = offset as usize;
        match offset {
            // Display
            0x000..=0x001 => self.ppu.read_io(0x0400_0000 + offset) as u8,
            0x002..=0x003 => 0, // Green Swap (unused)
            0x004..=0x005 => {
                let v = self.ppu.read_dispstat();
                if off & 1 == 0 { v as u8 } else { (v >> 8) as u8 }
            }
            0x006..=0x007 => {
                let v = self.ppu.vcount;
                if off & 1 == 0 { v as u8 } else { 0 }
            }
            0x008..=0x05F => self.ppu.read_io(0x0400_0000 + offset) as u8,

            // Sound
            0x060..=0x0A7 => self.apu.read_io(offset),

            // DMA
            0x0B0..=0x0DF => self.dma.read(offset),

            // Timers
            0x100..=0x10F => self.timers.read(offset),

            // Serial (stub)
            0x120..=0x12F => 0,

            // Keypad
            0x130 => self.input.read_keyinput() as u8,
            0x131 => (self.input.read_keyinput() >> 8) as u8,
            0x132 => self.input.keycnt as u8,
            0x133 => (self.input.keycnt >> 8) as u8,

            // Interrupt control
            0x200 => self.ie as u8,
            0x201 => (self.ie >> 8) as u8,
            0x202 => self.iflag as u8,
            0x203 => (self.iflag >> 8) as u8,
            0x204 => self.waitcnt as u8,
            0x205 => (self.waitcnt >> 8) as u8,
            0x208 => self.ime as u8,
            0x209 => 0,

            // Post-boot flag
            0x300 => self.post_boot as u8,

            _ => {
                if off < self.io.len() { self.io[off] } else { 0 }
            }
        }
    }

    fn write_io_reg(&mut self, offset: u32, val: u8) {
        let off = offset as usize;
        if off < self.io.len() {
            self.io[off] = val;
        }

        match offset {
            // Display
            0x000..=0x001 | 0x008..=0x05F => {
                self.ppu.write_io(0x0400_0000 + offset, val);
            }
            0x002..=0x003 => {} // Green Swap (unused)
            0x004..=0x005 => self.ppu.write_dispstat(offset & 1, val),

            // Sound
            0x060..=0x0A7 => self.apu.write_io(offset, val),

            // DMA
            0x0B0..=0x0DF => self.dma.write(offset, val),

            // Timers
            0x100..=0x10F => self.timers.write(offset, val),

            // Keypad
            0x132 => self.input.keycnt = (self.input.keycnt & 0xFF00) | val as u16,
            0x133 => self.input.keycnt = (self.input.keycnt & 0x00FF) | ((val as u16) << 8),

            // Interrupt control
            0x200 => self.ie = (self.ie & 0xFF00) | val as u16,
            0x201 => self.ie = (self.ie & 0x00FF) | ((val as u16) << 8),
            0x202 => self.iflag &= !val as u16,
            0x203 => self.iflag &= !((val as u16) << 8),
            0x204 => self.waitcnt = (self.waitcnt & 0xFF00) | val as u16,
            0x205 => self.waitcnt = (self.waitcnt & 0x00FF) | ((val as u16) << 8),
            0x208 => self.ime = val & 1 != 0,
            0x209 => {}

            // HALTCNT
            0x301 => {
                self.halt = true;
            }

            // Post-boot
            0x300 => self.post_boot = val & 1 != 0,

            _ => {}
        }
    }
}

impl Arm7Bus for GbaBus {
    fn read8(&self, addr: u32) -> u8 {
        match addr >> 24 {
            0x00 => {
                // BIOS
                if (addr as usize) < self.bios.len() {
                    self.bios[addr as usize]
                } else {
                    0
                }
            }
            0x02 => self.ewram[(addr & 0x3FFFF) as usize],
            0x03 => self.iwram[(addr & 0x7FFF) as usize],
            0x04 => self.read_io_reg(addr & 0x3FF),
            0x05 => self.ppu.palette[(addr & 0x3FF) as usize],
            0x06 => {
                let offset = (addr & 0x1FFFF) as usize;
                let offset = if offset >= 0x18000 { offset - 0x8000 } else { offset };
                self.ppu.vram[offset]
            }
            0x07 => self.ppu.oam[(addr & 0x3FF) as usize],
            0x08..=0x0C => self.cart.read_rom(addr),
            0x0D => {
                use crate::cartridge::GbaBackupType;
                if self.cart.backup_type == GbaBackupType::Eeprom {
                    self.cart.eeprom_read()
                } else {
                    self.cart.read_rom(addr)
                }
            }
            0x0E..=0x0F => self.cart.read_sram(addr),
            _ => 0,
        }
    }

    fn read16(&mut self, addr: u32) -> u16 {
        if addr >> 24 == 0x0D && self.cart.backup_type == crate::cartridge::GbaBackupType::Eeprom {
            let val = self.cart.eeprom_read() as u16;
            self.cart.eeprom_read_advance();
            return val;
        }
        let lo = self.read8(addr) as u16;
        let hi = self.read8(addr.wrapping_add(1)) as u16;
        lo | (hi << 8)
    }

    fn read32(&mut self, addr: u32) -> u32 {
        if addr >> 24 == 0x0D && self.cart.backup_type == crate::cartridge::GbaBackupType::Eeprom {
            let val = self.cart.eeprom_read() as u32;
            self.cart.eeprom_read_advance();
            return val;
        }
        let b0 = self.read8(addr) as u32;
        let b1 = self.read8(addr.wrapping_add(1)) as u32;
        let b2 = self.read8(addr.wrapping_add(2)) as u32;
        let b3 = self.read8(addr.wrapping_add(3)) as u32;
        b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
    }

    fn write8(&mut self, addr: u32, val: u8) {
        match addr >> 24 {
            0x02 => self.ewram[(addr & 0x3FFFF) as usize] = val,
            0x03 => self.iwram[(addr & 0x7FFF) as usize] = val,
            0x04 => self.write_io_reg(addr & 0x3FF, val),
            0x05 => {
                // Palette - 8-bit writes duplicate to both bytes of halfword
                let aligned = (addr & 0x3FE) as usize;
                self.ppu.palette[aligned] = val;
                self.ppu.palette[aligned + 1] = val;
            }
            0x06 => {
                // VRAM - 8-bit writes duplicate to both bytes
                let offset = (addr & 0x1FFFF) as usize;
                let offset = if offset >= 0x18000 { offset - 0x8000 } else { offset };
                let aligned = offset & !1;
                if aligned + 1 < self.ppu.vram.len() {
                    self.ppu.vram[aligned] = val;
                    self.ppu.vram[aligned + 1] = val;
                }
            }
            0x07 => {} // OAM ignores 8-bit writes
            0x0D => {
                use crate::cartridge::GbaBackupType;
                if self.cart.backup_type == GbaBackupType::Eeprom {
                    self.cart.eeprom_write(val);
                }
            }
            0x0E..=0x0F => self.cart.write_sram(addr, val),
            _ => {}
        }
    }

    fn write16(&mut self, addr: u32, val: u16) {
        let addr = addr & !1;
        if addr >> 24 == 0x0D && self.cart.backup_type == crate::cartridge::GbaBackupType::Eeprom {
            self.cart.eeprom_write(val as u8);
            return;
        }
        match addr >> 24 {
            0x05 => {
                let offset = (addr & 0x3FE) as usize;
                self.ppu.palette[offset] = val as u8;
                self.ppu.palette[offset + 1] = (val >> 8) as u8;
            }
            0x06 => {
                let offset = (addr & 0x1FFFE) as usize;
                let offset = if offset >= 0x18000 { offset - 0x8000 } else { offset };
                if offset + 1 < self.ppu.vram.len() {
                    self.ppu.vram[offset] = val as u8;
                    self.ppu.vram[offset + 1] = (val >> 8) as u8;
                }
            }
            0x07 => {
                let offset = (addr & 0x3FE) as usize;
                if offset + 1 < self.ppu.oam.len() {
                    self.ppu.oam[offset] = val as u8;
                    self.ppu.oam[offset + 1] = (val >> 8) as u8;
                }
            }
            _ => {
                self.write8(addr, val as u8);
                self.write8(addr.wrapping_add(1), (val >> 8) as u8);
            }
        }
    }

    fn write32(&mut self, addr: u32, val: u32) {
        if addr >> 24 == 0x0D && self.cart.backup_type == crate::cartridge::GbaBackupType::Eeprom {
            self.cart.eeprom_write(val as u8);
            return;
        }
        self.write16(addr, val as u16);
        self.write16(addr.wrapping_add(2), (val >> 16) as u16);
    }
}
