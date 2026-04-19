pub mod mbc;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CartridgeType {
    RomOnly,
    Mbc1,
    Mbc1Ram,
    Mbc1RamBattery,
    Mbc3,
    Mbc3Ram,
    Mbc3RamBattery,
    Mbc3TimerBattery,
    Mbc3TimerRamBattery,
    Mbc5,
    Mbc5Ram,
    Mbc5RamBattery,
    Mbc5Rumble,
    Mbc5RumbleRam,
    Mbc5RumbleRamBattery,
    Unknown(u8),
}

impl CartridgeType {
    pub fn from_byte(b: u8) -> Self {
        match b {
            0x00 => Self::RomOnly,
            0x01 => Self::Mbc1,
            0x02 => Self::Mbc1Ram,
            0x03 => Self::Mbc1RamBattery,
            0x0F => Self::Mbc3TimerBattery,
            0x10 => Self::Mbc3TimerRamBattery,
            0x11 => Self::Mbc3,
            0x12 => Self::Mbc3Ram,
            0x13 => Self::Mbc3RamBattery,
            0x19 => Self::Mbc5,
            0x1A => Self::Mbc5Ram,
            0x1B => Self::Mbc5RamBattery,
            0x1C => Self::Mbc5Rumble,
            0x1D => Self::Mbc5RumbleRam,
            0x1E => Self::Mbc5RumbleRamBattery,
            other => Self::Unknown(other),
        }
    }

    pub fn has_battery(self) -> bool {
        matches!(
            self,
            Self::Mbc1RamBattery
                | Self::Mbc3RamBattery
                | Self::Mbc3TimerBattery
                | Self::Mbc3TimerRamBattery
                | Self::Mbc5RamBattery
                | Self::Mbc5RumbleRamBattery
        )
    }

    pub fn has_ram(self) -> bool {
        matches!(
            self,
            Self::Mbc1Ram
                | Self::Mbc1RamBattery
                | Self::Mbc3Ram
                | Self::Mbc3RamBattery
                | Self::Mbc3TimerRamBattery
                | Self::Mbc5Ram
                | Self::Mbc5RamBattery
                | Self::Mbc5RumbleRam
                | Self::Mbc5RumbleRamBattery
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GbcCartridge {
    pub rom: Vec<u8>,
    pub ram: Vec<u8>,
    pub cart_type: CartridgeType,
    pub title: String,
    pub rom_banks: usize,
    pub ram_banks: usize,
    pub is_cgb: bool,
}

impl GbcCartridge {
    pub fn load(data: Vec<u8>) -> Self {
        let title: String = data[0x134..0x143]
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as char)
            .collect();

        let cart_type = CartridgeType::from_byte(data[0x147]);
        let is_cgb = data[0x143] == 0x80 || data[0x143] == 0xC0;

        let rom_size = match data[0x148] {
            0 => 2,
            1 => 4,
            2 => 8,
            3 => 16,
            4 => 32,
            5 => 64,
            6 => 128,
            7 => 256,
            8 => 512,
            _ => 2,
        };

        let ram_size = match data[0x149] {
            0 => 0,
            1 => 1,
            2 => 1,
            3 => 4,
            4 => 16,
            5 => 8,
            _ => 0,
        };

        let ram = vec![0u8; ram_size * 8192];

        Self {
            rom: data,
            ram,
            cart_type,
            title,
            rom_banks: rom_size,
            ram_banks: ram_size,
            is_cgb,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GbaBackupType {
    None,
    Sram,
    Flash64k,
    Flash128k,
    Eeprom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GbaCartridge {
    pub rom: Vec<u8>,
    pub sram: Vec<u8>,
    pub flash: Vec<u8>,
    pub eeprom: Vec<u8>,
    pub backup_type: GbaBackupType,
    pub title: String,
    pub flash_bank: u8,
    pub flash_state: u8,
    pub flash_cmd_stage: u8,
    pub flash_id_mode: bool,
    // EEPROM serial state
    pub eeprom_state: EepromState,
    pub eeprom_buffer: u64,
    pub eeprom_bits_written: u8,
    pub eeprom_address: u16,
    pub eeprom_bits_read: u8,
    pub eeprom_read_buffer: u64,
    pub eeprom_addr_len: u8, // 6 or 14 bits
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EepromState {
    Idle,
    ReadingCommand,
    ReadingAddress,
    WritingData,
    WritingFinish,
    ReadReady,
    ReadingData,
}

impl GbaCartridge {
    pub fn load(data: Vec<u8>) -> Self {
        let title: String = data[0xA0..0xAC]
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as char)
            .collect();

        let backup_type = Self::detect_backup_type(&data);

        let (sram, flash, eeprom) = match backup_type {
            GbaBackupType::Sram => (vec![0xFF; 0x8000], vec![], vec![]),
            GbaBackupType::Flash64k => (vec![], vec![0xFF; 0x10000], vec![]),
            GbaBackupType::Flash128k => (vec![], vec![0xFF; 0x20000], vec![]),
            GbaBackupType::Eeprom => (vec![], vec![], vec![0xFF; 0x2000]),
            GbaBackupType::None => (vec![0xFF; 0x8000], vec![], vec![]),
        };

        let eeprom_addr_len = if data.len() > 16 * 1024 * 1024 { 14 } else { 6 };

        Self {
            rom: data,
            sram,
            flash,
            eeprom,
            backup_type,
            title,
            flash_bank: 0,
            flash_state: 0,
            flash_cmd_stage: 0,
            flash_id_mode: false,
            eeprom_state: EepromState::Idle,
            eeprom_buffer: 0,
            eeprom_bits_written: 0,
            eeprom_address: 0,
            eeprom_bits_read: 0,
            eeprom_read_buffer: 0,
            eeprom_addr_len,
        }
    }

    fn detect_backup_type(data: &[u8]) -> GbaBackupType {
        let text = String::from_utf8_lossy(data);
        if text.contains("SRAM_V") || text.contains("SRAM_F_V") {
            GbaBackupType::Sram
        } else if text.contains("FLASH1M_V") {
            GbaBackupType::Flash128k
        } else if text.contains("FLASH_V") || text.contains("FLASH512_V") {
            GbaBackupType::Flash64k
        } else if text.contains("EEPROM_V") {
            GbaBackupType::Eeprom
        } else {
            GbaBackupType::Sram // Default fallback
        }
    }

    pub fn read_rom(&self, addr: u32) -> u8 {
        let offset = (addr & 0x01FF_FFFF) as usize;
        if offset < self.rom.len() {
            self.rom[offset]
        } else {
            (offset >> 1) as u8 // Open bus
        }
    }

    pub fn read_sram(&self, addr: u32) -> u8 {
        let offset = (addr & 0x7FFF) as usize;
        match self.backup_type {
            GbaBackupType::Sram => {
                if offset < self.sram.len() { self.sram[offset] } else { 0xFF }
            }
            GbaBackupType::Flash64k | GbaBackupType::Flash128k => {
                if self.flash_id_mode {
                    match offset {
                        0 => 0x62, // Macronix manufacturer
                        1 => if self.backup_type == GbaBackupType::Flash128k { 0x13 } else { 0x09 },
                        _ => 0xFF,
                    }
                } else {
                    let bank_offset = (self.flash_bank as usize) * 0x10000 + offset;
                    if bank_offset < self.flash.len() { self.flash[bank_offset] } else { 0xFF }
                }
            }
            _ => 0xFF,
        }
    }

    pub fn write_sram(&mut self, addr: u32, val: u8) {
        let offset = (addr & 0x7FFF) as usize;
        match self.backup_type {
            GbaBackupType::Sram => {
                if offset < self.sram.len() {
                    self.sram[offset] = val;
                }
            }
            GbaBackupType::Flash64k | GbaBackupType::Flash128k => {
                self.handle_flash_write(offset, val);
            }
            _ => {}
        }
    }

    fn handle_flash_write(&mut self, offset: usize, val: u8) {
        if offset == 0x5555 && self.flash_cmd_stage == 0 && val == 0xAA {
            self.flash_cmd_stage = 1;
            return;
        }
        if offset == 0x2AAA && self.flash_cmd_stage == 1 && val == 0x55 {
            self.flash_cmd_stage = 2;
            return;
        }
        if self.flash_cmd_stage == 2 {
            self.flash_cmd_stage = 0;
            if offset == 0x5555 {
                match val {
                    0x90 => self.flash_id_mode = true,
                    0xF0 => self.flash_id_mode = false,
                    0x80 => self.flash_state = 1,
                    0x10 if self.flash_state == 1 => {
                        self.flash.fill(0xFF);
                        self.flash_state = 0;
                    }
                    0xA0 => self.flash_state = 2,
                    0xB0 => self.flash_state = 3,
                    _ => self.flash_state = 0,
                }
            } else if self.flash_state == 1 && val == 0x30 {
                // Sector erase
                let sector = offset / 0x1000;
                let bank_base = (self.flash_bank as usize) * 0x10000;
                let start = bank_base + sector * 0x1000;
                let end = (start + 0x1000).min(self.flash.len());
                if start < self.flash.len() {
                    self.flash[start..end].fill(0xFF);
                }
                self.flash_state = 0;
            }
            return;
        }
        if self.flash_state == 2 {
            // Write byte
            let bank_offset = (self.flash_bank as usize) * 0x10000 + offset;
            if bank_offset < self.flash.len() {
                self.flash[bank_offset] = val;
            }
            self.flash_state = 0;
            return;
        }
        if self.flash_state == 3 && offset == 0 {
            // Bank switch
            self.flash_bank = val & 1;
            self.flash_state = 0;
        }
    }

    pub fn eeprom_read(&self) -> u8 {
        match self.eeprom_state {
            EepromState::ReadingData => {
                // Return one bit at a time from read_buffer, MSB first
                let bit = if self.eeprom_bits_read < 4 {
                    // First 4 bits are garbage (return 0)
                    0
                } else {
                    let data_bit = self.eeprom_bits_read - 4;
                    let bit_idx = 63u8.saturating_sub(data_bit);
                    ((self.eeprom_read_buffer >> bit_idx) & 1) as u8
                };
                bit
            }
            _ => 1, // Ready/idle: return 1
        }
    }

    pub fn eeprom_read_advance(&mut self) {
        if self.eeprom_state == EepromState::ReadingData {
            self.eeprom_bits_read += 1;
            if self.eeprom_bits_read >= 68 {
                self.eeprom_state = EepromState::Idle;
            }
        }
    }

    pub fn eeprom_write(&mut self, val: u8) {
        let bit = val & 1;

        match self.eeprom_state {
            EepromState::Idle => {
                self.eeprom_buffer = bit as u64;
                self.eeprom_bits_written = 1;
                self.eeprom_state = EepromState::ReadingCommand;
            }
            EepromState::ReadingCommand => {
                self.eeprom_buffer = (self.eeprom_buffer << 1) | bit as u64;
                self.eeprom_bits_written += 1;
                if self.eeprom_bits_written == 2 {
                    let cmd = self.eeprom_buffer & 3;
                    self.eeprom_buffer = 0;
                    self.eeprom_bits_written = 0;
                    self.eeprom_address = 0;
                    match cmd {
                        3 => {
                            // Read command (11)
                            self.eeprom_state = EepromState::ReadingAddress;
                        }
                        2 => {
                            // Write command (10)
                            self.eeprom_state = EepromState::ReadingAddress;
                        }
                        _ => {
                            self.eeprom_state = EepromState::Idle;
                        }
                    }
                }
            }
            EepromState::ReadingAddress => {
                self.eeprom_address = (self.eeprom_address << 1) | bit as u16;
                self.eeprom_bits_written += 1;
                if self.eeprom_bits_written >= self.eeprom_addr_len {
                    self.eeprom_bits_written = 0;
                    self.eeprom_state = EepromState::ReadReady;
                }
            }
            EepromState::ReadReady => {
                // This 0-bit terminates the command
                if bit == 0 {
                    // Read: load 64 bits from EEPROM
                    let addr = self.eeprom_address as usize;
                    let byte_addr = addr * 8;
                    self.eeprom_read_buffer = 0;
                    for i in 0..8 {
                        let b = if byte_addr + i < self.eeprom.len() {
                            self.eeprom[byte_addr + i]
                        } else {
                            0xFF
                        };
                        self.eeprom_read_buffer = (self.eeprom_read_buffer << 8) | b as u64;
                    }
                    self.eeprom_bits_read = 0;
                    self.eeprom_state = EepromState::ReadingData;
                } else {
                    // Write: start collecting 64 data bits
                    self.eeprom_buffer = bit as u64;
                    self.eeprom_bits_written = 1;
                    self.eeprom_state = EepromState::WritingData;
                }
            }
            EepromState::WritingData => {
                self.eeprom_buffer = (self.eeprom_buffer << 1) | bit as u64;
                self.eeprom_bits_written += 1;
                if self.eeprom_bits_written >= 64 {
                    // Write 8 bytes to EEPROM
                    let addr = self.eeprom_address as usize;
                    let byte_addr = addr * 8;
                    for i in 0..8 {
                        let b = ((self.eeprom_buffer >> (56 - i * 8)) & 0xFF) as u8;
                        if byte_addr + i < self.eeprom.len() {
                            self.eeprom[byte_addr + i] = b;
                        }
                    }
                    self.eeprom_state = EepromState::WritingFinish;
                }
            }
            EepromState::WritingFinish => {
                // End bit after write
                self.eeprom_state = EepromState::Idle;
            }
            EepromState::ReadingData => {
                // Shouldn't write during read, go back to idle
                self.eeprom_state = EepromState::Idle;
            }
        }
    }
}
