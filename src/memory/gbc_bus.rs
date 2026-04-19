use serde::{Deserialize, Serialize};

use crate::apu::gbc_apu::GbcApu;
use crate::cartridge::mbc::{Mbc, MbcType};
use crate::cartridge::{CartridgeType, GbcCartridge};
use crate::cpu::sm83::Sm83Bus;
use crate::input::GbcInput;
use crate::ppu::gbc_ppu::GbcPpu;
use crate::timer::GbcTimer;

#[derive(Clone, Serialize, Deserialize)]
pub struct GbcBus {
    pub cart: GbcCartridge,
    pub mbc: Mbc,
    pub ppu: GbcPpu,
    pub apu: GbcApu,
    pub timer: GbcTimer,
    pub input: GbcInput,
    pub wram: Vec<u8>,
    pub hram: Vec<u8>,
    pub ie: u8,
    pub iflag: u8,
    pub wram_bank: u8,
    pub dma_active: bool,
    pub dma_byte: u8,
    pub dma_source: u16,
    // CGB HDMA
    pub hdma_source: u16,
    pub hdma_dest: u16,
    pub hdma_length: u8,
    pub hdma_active: bool,
    // Speed switch
    pub double_speed: bool,
    pub speed_switch_pending: bool,
    // Serial
    pub serial_data: u8,
    pub serial_control: u8,
}

impl GbcBus {
    pub fn new(cart: GbcCartridge) -> Self {
        let mbc_type = match cart.cart_type {
            CartridgeType::RomOnly => MbcType::None,
            CartridgeType::Mbc1 | CartridgeType::Mbc1Ram | CartridgeType::Mbc1RamBattery => {
                MbcType::Mbc1
            }
            CartridgeType::Mbc3
            | CartridgeType::Mbc3Ram
            | CartridgeType::Mbc3RamBattery
            | CartridgeType::Mbc3TimerBattery
            | CartridgeType::Mbc3TimerRamBattery => MbcType::Mbc3,
            CartridgeType::Mbc5
            | CartridgeType::Mbc5Ram
            | CartridgeType::Mbc5RamBattery
            | CartridgeType::Mbc5Rumble
            | CartridgeType::Mbc5RumbleRam
            | CartridgeType::Mbc5RumbleRamBattery => MbcType::Mbc5,
            _ => MbcType::None,
        };

        let is_cgb = cart.is_cgb;

        Self {
            cart,
            mbc: Mbc::new(mbc_type),
            ppu: GbcPpu::new(is_cgb),
            apu: GbcApu::new(),
            timer: GbcTimer::new(),
            input: GbcInput::new(),
            wram: vec![0; 0x8000], // 8 banks for CGB
            hram: vec![0; 127],
            ie: 0,
            iflag: 0,
            wram_bank: 1,
            dma_active: false,
            dma_byte: 0,
            dma_source: 0,
            hdma_source: 0,
            hdma_dest: 0,
            hdma_length: 0xFF,
            hdma_active: false,
            double_speed: false,
            speed_switch_pending: false,
            serial_data: 0,
            serial_control: 0,
        }
    }

    pub fn tick(&mut self, cycles: u32) {
        let timer_irq = self.timer.tick(cycles);
        if timer_irq {
            self.iflag |= 0x04;
        }

        let ppu_irqs = self.ppu.tick(cycles);
        self.iflag |= ppu_irqs;

        self.apu.tick(cycles);

        // OAM DMA
        if self.dma_active {
            for i in 0..160u16 {
                let byte = self.dma_read(self.dma_source + i);
                self.ppu.oam[i as usize] = byte;
            }
            self.dma_active = false;
        }
    }

    fn dma_read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.mbc.read_rom(addr, &self.cart.rom),
            0x8000..=0x9FFF => self.ppu.read_vram(addr),
            0xA000..=0xBFFF => self.mbc.read_ram(addr, &self.cart.ram),
            0xC000..=0xDFFF => {
                let offset = (addr - 0xC000) as usize;
                if offset < 0x1000 {
                    self.wram[offset]
                } else {
                    let bank = self.wram_bank.max(1) as usize;
                    self.wram[bank * 0x1000 + (offset - 0x1000)]
                }
            }
            _ => 0xFF,
        }
    }

    pub fn request_interrupt(&mut self, bit: u8) {
        self.iflag |= bit;
    }
}

impl Sm83Bus for GbcBus {
    fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.mbc.read_rom(addr, &self.cart.rom),
            0x8000..=0x9FFF => self.ppu.read_vram(addr),
            0xA000..=0xBFFF => self.mbc.read_ram(addr, &self.cart.ram),
            0xC000..=0xCFFF => self.wram[(addr - 0xC000) as usize],
            0xD000..=0xDFFF => {
                let bank = self.wram_bank.max(1) as usize;
                self.wram[bank * 0x1000 + (addr - 0xD000) as usize]
            }
            0xE000..=0xFDFF => {
                // Echo RAM
                self.read(addr - 0x2000)
            }
            0xFE00..=0xFE9F => self.ppu.oam[(addr - 0xFE00) as usize],
            0xFEA0..=0xFEFF => 0xFF,
            0xFF00 => self.input.read(),
            0xFF01 => self.serial_data,
            0xFF02 => self.serial_control,
            0xFF04..=0xFF07 => self.timer.read(addr),
            0xFF0F => self.iflag | 0xE0,
            0xFF10..=0xFF3F => self.apu.read(addr),
            0xFF40..=0xFF4B => self.ppu.read_io(addr),
            0xFF4D => {
                let speed_bit = if self.double_speed { 0x80 } else { 0 };
                let pending_bit = if self.speed_switch_pending { 0x01 } else { 0 };
                speed_bit | pending_bit
            }
            0xFF4F => self.ppu.vram_bank | 0xFE,
            0xFF51..=0xFF55 => {
                // HDMA registers
                match addr {
                    0xFF51 => (self.hdma_source >> 8) as u8,
                    0xFF52 => (self.hdma_source & 0xF0) as u8,
                    0xFF53 => (self.hdma_dest >> 8) as u8,
                    0xFF54 => (self.hdma_dest & 0xF0) as u8,
                    0xFF55 => {
                        if self.hdma_active { 0x00 | self.hdma_length } else { 0x80 | self.hdma_length }
                    }
                    _ => 0xFF,
                }
            }
            0xFF68..=0xFF6B => self.ppu.read_io(addr),
            0xFF70 => self.wram_bank | 0xF8,
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize],
            0xFFFF => self.ie,
            _ => 0xFF,
        }
    }

    fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0x0000..=0x7FFF => self.mbc.write_register(addr, val),
            0x8000..=0x9FFF => self.ppu.write_vram(addr, val),
            0xA000..=0xBFFF => self.mbc.write_ram(addr, val, &mut self.cart.ram),
            0xC000..=0xCFFF => self.wram[(addr - 0xC000) as usize] = val,
            0xD000..=0xDFFF => {
                let bank = self.wram_bank.max(1) as usize;
                self.wram[bank * 0x1000 + (addr - 0xD000) as usize] = val;
            }
            0xE000..=0xFDFF => {
                self.write(addr - 0x2000, val);
            }
            0xFE00..=0xFE9F => self.ppu.oam[(addr - 0xFE00) as usize] = val,
            0xFEA0..=0xFEFF => {} // Unusable
            0xFF00 => self.input.write(val),
            0xFF01 => self.serial_data = val,
            0xFF02 => self.serial_control = val,
            0xFF04..=0xFF07 => self.timer.write(addr, val),
            0xFF0F => self.iflag = val & 0x1F,
            0xFF10..=0xFF3F => self.apu.write(addr, val),
            0xFF40..=0xFF45 => self.ppu.write_io(addr, val),
            0xFF46 => {
                self.dma_source = (val as u16) << 8;
                self.dma_active = true;
            }
            0xFF47..=0xFF4B => self.ppu.write_io(addr, val),
            0xFF4D => {
                self.speed_switch_pending = val & 1 != 0;
            }
            0xFF4F => self.ppu.vram_bank = val & 1,
            0xFF51 => self.hdma_source = (self.hdma_source & 0x00FF) | ((val as u16) << 8),
            0xFF52 => self.hdma_source = (self.hdma_source & 0xFF00) | ((val & 0xF0) as u16),
            0xFF53 => self.hdma_dest = (self.hdma_dest & 0x00FF) | (((val & 0x1F) as u16) << 8),
            0xFF54 => self.hdma_dest = (self.hdma_dest & 0xFF00) | ((val & 0xF0) as u16),
            0xFF55 => {
                self.hdma_length = val & 0x7F;
                self.hdma_active = val & 0x80 == 0;
                if self.hdma_active {
                    // General purpose DMA - transfer immediately
                    let len = ((self.hdma_length as u16) + 1) * 16;
                    for i in 0..len {
                        let src = self.hdma_source.wrapping_add(i);
                        let byte = self.dma_read(src);
                        let dst = 0x8000 + ((self.hdma_dest.wrapping_add(i)) & 0x1FFF);
                        self.ppu.write_vram(dst, byte);
                    }
                    self.hdma_source = self.hdma_source.wrapping_add(len);
                    self.hdma_dest = self.hdma_dest.wrapping_add(len);
                    self.hdma_length = 0xFF;
                    self.hdma_active = false;
                }
            }
            0xFF68..=0xFF6B => self.ppu.write_io(addr, val),
            0xFF70 => self.wram_bank = val & 0x07,
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize] = val,
            0xFFFF => self.ie = val,
            _ => {}
        }
    }
}
