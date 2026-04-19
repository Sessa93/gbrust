use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MbcType {
    None,
    Mbc1,
    Mbc3,
    Mbc5,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mbc {
    pub mbc_type: MbcType,
    pub rom_bank: u16,
    pub ram_bank: u8,
    pub ram_enabled: bool,
    pub mode: u8, // MBC1: 0=ROM banking, 1=RAM banking
    // MBC3 RTC
    pub rtc_regs: [u8; 5],
    pub rtc_latch: u8,
    pub rtc_latched: [u8; 5],
}

impl Mbc {
    pub fn new(mbc_type: MbcType) -> Self {
        Self {
            mbc_type,
            rom_bank: 1,
            ram_bank: 0,
            ram_enabled: false,
            mode: 0,
            rtc_regs: [0; 5],
            rtc_latch: 0xFF,
            rtc_latched: [0; 5],
        }
    }

    pub fn read_rom(&self, addr: u16, rom: &[u8]) -> u8 {
        match self.mbc_type {
            MbcType::None => {
                let offset = addr as usize;
                if offset < rom.len() { rom[offset] } else { 0xFF }
            }
            MbcType::Mbc1 => {
                if addr < 0x4000 {
                    let bank = if self.mode == 1 {
                        ((self.ram_bank as usize) << 5) % (rom.len() / 0x4000).max(1)
                    } else {
                        0
                    };
                    let offset = bank * 0x4000 + addr as usize;
                    if offset < rom.len() { rom[offset] } else { 0xFF }
                } else {
                    let bank = ((self.ram_bank as usize) << 5 | self.rom_bank as usize) % (rom.len() / 0x4000).max(1);
                    let offset = bank * 0x4000 + (addr as usize - 0x4000);
                    if offset < rom.len() { rom[offset] } else { 0xFF }
                }
            }
            MbcType::Mbc3 => {
                if addr < 0x4000 {
                    rom[addr as usize]
                } else {
                    let bank = if self.rom_bank == 0 { 1 } else { self.rom_bank as usize };
                    let offset = bank * 0x4000 + (addr as usize - 0x4000);
                    if offset < rom.len() { rom[offset] } else { 0xFF }
                }
            }
            MbcType::Mbc5 => {
                if addr < 0x4000 {
                    rom[addr as usize]
                } else {
                    let bank = self.rom_bank as usize;
                    let offset = bank * 0x4000 + (addr as usize - 0x4000);
                    if offset < rom.len() { rom[offset] } else { 0xFF }
                }
            }
        }
    }

    pub fn read_ram(&self, addr: u16, ram: &[u8]) -> u8 {
        if !self.ram_enabled || ram.is_empty() {
            return 0xFF;
        }

        match self.mbc_type {
            MbcType::None => 0xFF,
            MbcType::Mbc1 => {
                let bank = if self.mode == 1 { self.ram_bank as usize } else { 0 };
                let offset = bank * 0x2000 + (addr as usize - 0xA000);
                if offset < ram.len() { ram[offset] } else { 0xFF }
            }
            MbcType::Mbc3 => {
                if self.ram_bank <= 3 {
                    let offset = (self.ram_bank as usize) * 0x2000 + (addr as usize - 0xA000);
                    if offset < ram.len() { ram[offset] } else { 0xFF }
                } else if self.ram_bank >= 0x08 && self.ram_bank <= 0x0C {
                    self.rtc_latched[(self.ram_bank - 0x08) as usize]
                } else {
                    0xFF
                }
            }
            MbcType::Mbc5 => {
                let offset = (self.ram_bank as usize) * 0x2000 + (addr as usize - 0xA000);
                if offset < ram.len() { ram[offset] } else { 0xFF }
            }
        }
    }

    pub fn write_ram(&mut self, addr: u16, val: u8, ram: &mut [u8]) {
        if !self.ram_enabled {
            return;
        }

        match self.mbc_type {
            MbcType::None => {}
            MbcType::Mbc1 => {
                let bank = if self.mode == 1 { self.ram_bank as usize } else { 0 };
                let offset = bank * 0x2000 + (addr as usize - 0xA000);
                if offset < ram.len() {
                    ram[offset] = val;
                }
            }
            MbcType::Mbc3 => {
                if self.ram_bank <= 3 {
                    let offset = (self.ram_bank as usize) * 0x2000 + (addr as usize - 0xA000);
                    if offset < ram.len() {
                        ram[offset] = val;
                    }
                } else if self.ram_bank >= 0x08 && self.ram_bank <= 0x0C {
                    self.rtc_regs[(self.ram_bank - 0x08) as usize] = val;
                }
            }
            MbcType::Mbc5 => {
                let offset = (self.ram_bank as usize) * 0x2000 + (addr as usize - 0xA000);
                if offset < ram.len() {
                    ram[offset] = val;
                }
            }
        }
    }

    pub fn write_register(&mut self, addr: u16, val: u8) {
        match self.mbc_type {
            MbcType::None => {}
            MbcType::Mbc1 => match addr {
                0x0000..=0x1FFF => self.ram_enabled = (val & 0x0F) == 0x0A,
                0x2000..=0x3FFF => {
                    let bank = val & 0x1F;
                    self.rom_bank = if bank == 0 { 1 } else { bank as u16 };
                }
                0x4000..=0x5FFF => self.ram_bank = val & 0x03,
                0x6000..=0x7FFF => self.mode = val & 1,
                _ => {}
            },
            MbcType::Mbc3 => match addr {
                0x0000..=0x1FFF => self.ram_enabled = (val & 0x0F) == 0x0A,
                0x2000..=0x3FFF => {
                    let bank = val & 0x7F;
                    self.rom_bank = if bank == 0 { 1 } else { bank as u16 };
                }
                0x4000..=0x5FFF => self.ram_bank = val,
                0x6000..=0x7FFF => {
                    if self.rtc_latch == 0x00 && val == 0x01 {
                        self.rtc_latched = self.rtc_regs;
                    }
                    self.rtc_latch = val;
                }
                _ => {}
            },
            MbcType::Mbc5 => match addr {
                0x0000..=0x1FFF => self.ram_enabled = (val & 0x0F) == 0x0A,
                0x2000..=0x2FFF => {
                    self.rom_bank = (self.rom_bank & 0x100) | val as u16;
                }
                0x3000..=0x3FFF => {
                    self.rom_bank = (self.rom_bank & 0xFF) | ((val as u16 & 1) << 8);
                }
                0x4000..=0x5FFF => self.ram_bank = val & 0x0F,
                _ => {}
            },
        }
    }
}
