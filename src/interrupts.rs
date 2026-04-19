/// Interrupt bit constants for GBC
pub mod gbc {
    pub const VBLANK: u8 = 0x01;
    pub const STAT: u8 = 0x02;
    pub const TIMER: u8 = 0x04;
    pub const SERIAL: u8 = 0x08;
    pub const JOYPAD: u8 = 0x10;

    pub const VBLANK_ADDR: u16 = 0x0040;
    pub const STAT_ADDR: u16 = 0x0048;
    pub const TIMER_ADDR: u16 = 0x0050;
    pub const SERIAL_ADDR: u16 = 0x0058;
    pub const JOYPAD_ADDR: u16 = 0x0060;

    pub fn interrupt_addr(bit: u8) -> u16 {
        match bit {
            0 => VBLANK_ADDR,
            1 => STAT_ADDR,
            2 => TIMER_ADDR,
            3 => SERIAL_ADDR,
            4 => JOYPAD_ADDR,
            _ => 0x0040,
        }
    }
}

/// Interrupt bit constants for GBA
pub mod gba {
    pub const VBLANK: u16 = 1 << 0;
    pub const HBLANK: u16 = 1 << 1;
    pub const VCOUNTER: u16 = 1 << 2;
    pub const TIMER0: u16 = 1 << 3;
    pub const TIMER1: u16 = 1 << 4;
    pub const TIMER2: u16 = 1 << 5;
    pub const TIMER3: u16 = 1 << 6;
    pub const SERIAL: u16 = 1 << 7;
    pub const DMA0: u16 = 1 << 8;
    pub const DMA1: u16 = 1 << 9;
    pub const DMA2: u16 = 1 << 10;
    pub const DMA3: u16 = 1 << 11;
    pub const KEYPAD: u16 = 1 << 12;
    pub const GAMEPAK: u16 = 1 << 13;
}
