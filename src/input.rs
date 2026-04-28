use serde::{Deserialize, Serialize};

// ─── GBC Input ─────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize)]
pub struct GbcInput {
    pub buttons: u8,   // A, B, Select, Start (active low)
    pub dpad: u8,      // Right, Left, Up, Down (active low)
    pub select: u8,    // Which group is selected
}

impl GbcInput {
    pub fn new() -> Self {
        Self {
            buttons: 0x0F,
            dpad: 0x0F,
            select: 0x30,
        }
    }

    pub fn read(&self) -> u8 {
        let mut val = self.select | 0xC0;
        if self.select & 0x10 == 0 {
            // Direction keys
            val = (val & 0xF0) | self.dpad;
        }
        if self.select & 0x20 == 0 {
            // Button keys
            val = (val & 0xF0) | self.buttons;
        }
        val
    }

    pub fn write(&mut self, val: u8) {
        self.select = val & 0x30;
    }

    /// Press a key. bit: 0=A/Right, 1=B/Left, 2=Select/Up, 3=Start/Down
    pub fn key_down(&mut self, key: GbcKey) {
        match key {
            GbcKey::Right => self.dpad &= !0x01,
            GbcKey::Left => self.dpad &= !0x02,
            GbcKey::Up => self.dpad &= !0x04,
            GbcKey::Down => self.dpad &= !0x08,
            GbcKey::A => self.buttons &= !0x01,
            GbcKey::B => self.buttons &= !0x02,
            GbcKey::Select => self.buttons &= !0x04,
            GbcKey::Start => self.buttons &= !0x08,
        }
    }

    pub fn key_up(&mut self, key: GbcKey) {
        match key {
            GbcKey::Right => self.dpad |= 0x01,
            GbcKey::Left => self.dpad |= 0x02,
            GbcKey::Up => self.dpad |= 0x04,
            GbcKey::Down => self.dpad |= 0x08,
            GbcKey::A => self.buttons |= 0x01,
            GbcKey::B => self.buttons |= 0x02,
            GbcKey::Select => self.buttons |= 0x04,
            GbcKey::Start => self.buttons |= 0x08,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GbcKey {
    A,
    B,
    Select,
    Start,
    Right,
    Left,
    Up,
    Down,
}

// ─── GBA Input ─────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize)]
pub struct GbaInput {
    pub keyinput: u16, // Active low: bits 0-9 for A,B,Select,Start,Right,Left,Up,Down,R,L
    pub keycnt: u16,
}

impl GbaInput {
    pub fn new() -> Self {
        Self {
            keyinput: 0x03FF, // All released
            keycnt: 0,
        }
    }

    pub fn read_keyinput(&self) -> u16 {
        self.keyinput
    }

    pub fn key_down(&mut self, key: GbaKey) {
        self.keyinput &= !(1 << key as u16);
    }

    pub fn key_up(&mut self, key: GbaKey) {
        self.keyinput |= 1 << key as u16;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GbaKey {
    A = 0,
    B = 1,
    Select = 2,
    Start = 3,
    Right = 4,
    Left = 5,
    Up = 6,
    Down = 7,
    R = 8,
    L = 9,
}

// ─── NDS Input ─────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize)]
pub struct NdsInput {
    pub keyinput: u16,
    pub extkeyin: u16,
    pub touchscreen_x: u16,
    pub touchscreen_y: u16,
    pub touchscreen_pressed: bool,
}

impl NdsInput {
    pub fn new() -> Self {
        Self {
            keyinput: 0x03FF,
            extkeyin: 0x007F,
            touchscreen_x: 0,
            touchscreen_y: 0,
            touchscreen_pressed: false,
        }
    }

    pub fn read_keyinput(&self) -> u16 {
        self.keyinput
    }

    pub fn read_extkeyin(&self) -> u16 {
        self.extkeyin
    }

    pub fn key_down(&mut self, key: NdsKey) {
        match key {
            NdsKey::A
            | NdsKey::B
            | NdsKey::Select
            | NdsKey::Start
            | NdsKey::Right
            | NdsKey::Left
            | NdsKey::Up
            | NdsKey::Down
            | NdsKey::R
            | NdsKey::L => {
                self.keyinput &= !(1 << (key as u16));
            }
            NdsKey::X => self.extkeyin &= !0x0001,
            NdsKey::Y => self.extkeyin &= !0x0002,
        }
    }

    pub fn key_up(&mut self, key: NdsKey) {
        match key {
            NdsKey::A
            | NdsKey::B
            | NdsKey::Select
            | NdsKey::Start
            | NdsKey::Right
            | NdsKey::Left
            | NdsKey::Up
            | NdsKey::Down
            | NdsKey::R
            | NdsKey::L => {
                self.keyinput |= 1 << (key as u16);
            }
            NdsKey::X => self.extkeyin |= 0x0001,
            NdsKey::Y => self.extkeyin |= 0x0002,
        }
    }

    pub fn set_touchscreen(&mut self, x: u16, y: u16, pressed: bool) {
        self.touchscreen_x = x;
        self.touchscreen_y = y;
        self.touchscreen_pressed = pressed;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NdsKey {
    A = 0,
    B = 1,
    Select = 2,
    Start = 3,
    Right = 4,
    Left = 5,
    Up = 6,
    Down = 7,
    R = 8,
    L = 9,
    X,
    Y,
}

#[cfg(test)]
mod tests {
    use super::{NdsInput, NdsKey};

    #[test]
    fn nds_keys_use_active_low_registers() {
        let mut input = NdsInput::new();

        input.key_down(NdsKey::A);
        input.key_down(NdsKey::X);

        assert_eq!(input.read_keyinput() & 0x0001, 0);
        assert_eq!(input.read_extkeyin() & 0x0001, 0);

        input.key_up(NdsKey::A);
        input.key_up(NdsKey::X);

        assert_ne!(input.read_keyinput() & 0x0001, 0);
        assert_ne!(input.read_extkeyin() & 0x0001, 0);
    }
}
