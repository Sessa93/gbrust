pub mod cpu;
pub mod memory;
pub mod cartridge;
pub mod ppu;
pub mod apu;
pub mod timer;
pub mod interrupts;
pub mod input;
pub mod dma;
pub mod save;
pub mod emulator;
pub mod gui;

pub const GBC_WIDTH: usize = 160;
pub const GBC_HEIGHT: usize = 144;
pub const GBA_WIDTH: usize = 240;
pub const GBA_HEIGHT: usize = 160;

pub const GBC_CLOCK_SPEED: u32 = 4_194_304;
pub const GBA_CLOCK_SPEED: u32 = 16_777_216;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ConsoleType {
    GameBoyColor,
    GameBoyAdvance,
}

impl ConsoleType {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "gbc" | "gb" => Some(ConsoleType::GameBoyColor),
            "gba" => Some(ConsoleType::GameBoyAdvance),
            _ => None,
        }
    }

    pub fn screen_width(self) -> usize {
        match self {
            ConsoleType::GameBoyColor => GBC_WIDTH,
            ConsoleType::GameBoyAdvance => GBA_WIDTH,
        }
    }

    pub fn screen_height(self) -> usize {
        match self {
            ConsoleType::GameBoyColor => GBC_HEIGHT,
            ConsoleType::GameBoyAdvance => GBA_HEIGHT,
        }
    }
}
