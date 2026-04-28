use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};

use crate::cpu::arm7tdmi::Arm7Tdmi;
use crate::memory::nds_bus::NdsBus;
use crate::{NDS_HEIGHT, NDS_WIDTH};

const NDS_SCREEN_HEIGHT: usize = NDS_HEIGHT / 2;
const NDS_HEADER_SIZE: usize = 0x170;
const NDS_ARM9_CYCLES_PER_FRAME: u32 = 1_117_132;
const NDS_ARM7_CYCLES_PER_FRAME: u32 = 558_566;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NdsRomHeader {
    pub game_title: String,
    pub game_code: String,
    pub maker_code: String,
    pub unit_code: u8,
    pub device_type: u8,
    pub device_capacity: u8,
    pub arm9_rom_offset: u32,
    pub arm9_entry_address: u32,
    pub arm9_ram_address: u32,
    pub arm9_size: u32,
    pub arm7_rom_offset: u32,
    pub arm7_entry_address: u32,
    pub arm7_ram_address: u32,
    pub arm7_size: u32,
}

impl NdsRomHeader {
    pub fn parse(rom: &[u8]) -> Result<Self, String> {
        if rom.len() < NDS_HEADER_SIZE {
            return Err(format!(
                "ROM is too small for an NDS header: expected at least {} bytes, got {}",
                NDS_HEADER_SIZE,
                rom.len()
            ));
        }

        let header = Self {
            game_title: Self::read_ascii(rom, 0x000, 12),
            game_code: Self::read_ascii(rom, 0x00C, 4),
            maker_code: Self::read_ascii(rom, 0x010, 2),
            unit_code: rom[0x012],
            device_type: rom[0x013],
            device_capacity: rom[0x014],
            arm9_rom_offset: Self::read_u32(rom, 0x020),
            arm9_entry_address: Self::read_u32(rom, 0x024),
            arm9_ram_address: Self::read_u32(rom, 0x028),
            arm9_size: Self::read_u32(rom, 0x02C),
            arm7_rom_offset: Self::read_u32(rom, 0x030),
            arm7_entry_address: Self::read_u32(rom, 0x034),
            arm7_ram_address: Self::read_u32(rom, 0x038),
            arm7_size: Self::read_u32(rom, 0x03C),
        };

        header.validate(rom.len())?;
        Ok(header)
    }

    pub fn display_title(&self) -> &str {
        if self.game_title.is_empty() {
            "Untitled NDS ROM"
        } else {
            &self.game_title
        }
    }

    fn validate(&self, rom_len: usize) -> Result<(), String> {
        Self::validate_section("ARM9", self.arm9_rom_offset, self.arm9_size, rom_len)?;
        Self::validate_section("ARM7", self.arm7_rom_offset, self.arm7_size, rom_len)?;
        Ok(())
    }

    fn validate_section(name: &str, rom_offset: u32, size: u32, rom_len: usize) -> Result<(), String> {
        if size == 0 {
            return Err(format!("{} program section has zero length", name));
        }

        let end = rom_offset
            .checked_add(size)
            .ok_or_else(|| format!("{} program section overflows the header range", name))?;

        if end as usize > rom_len {
            return Err(format!(
                "{} program section exceeds ROM size: end=0x{:X}, rom_len=0x{:X}",
                name,
                end,
                rom_len
            ));
        }

        Ok(())
    }

    fn read_ascii(rom: &[u8], offset: usize, len: usize) -> String {
        String::from_utf8_lossy(&rom[offset..offset + len])
            .trim_matches(char::from(0))
            .trim()
            .to_string()
    }

    fn read_u32(rom: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(rom[offset..offset + 4].try_into().expect("header slice is in bounds"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NdsArm9 {
    cpu: Arm7Tdmi,
}

impl NdsArm9 {
    pub fn new(entry_address: u32) -> Self {
        let mut cpu = Arm7Tdmi::new();
        cpu.regs[15] = entry_address & !3;
        cpu.regs[13] = 0x0203_FF00;
        cpu.banked_regs[2][5] = 0x0203_FE00;
        cpu.banked_regs[3][5] = 0x0203_FF80;

        Self { cpu }
    }
}

impl Deref for NdsArm9 {
    type Target = Arm7Tdmi;

    fn deref(&self) -> &Self::Target {
        &self.cpu
    }
}

impl DerefMut for NdsArm9 {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.cpu
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct NdsEmulator {
    pub header: NdsRomHeader,
    pub arm7: Arm7Tdmi,
    pub arm9: NdsArm9,
    pub bus: NdsBus,
    pub framebuffer: Vec<u32>,
    pub total_frames: u64,
}

impl NdsEmulator {
    pub fn new(rom: Vec<u8>) -> Result<Self, String> {
        let header = NdsRomHeader::parse(&rom)?;

        let mut arm7 = Arm7Tdmi::new();
        arm7.regs[15] = header.arm7_entry_address & !3;
        arm7.regs[13] = 0x0380_FD80;
        arm7.banked_regs[2][5] = 0x0380_FCC0;
        arm7.banked_regs[3][5] = 0x0380_FDC0;

        let arm9 = NdsArm9::new(header.arm9_entry_address);
        let bus = NdsBus::new(rom, &header)?;

        let mut emu = Self {
            header,
            arm7,
            arm9,
            bus,
            framebuffer: vec![0xFF11161C; NDS_WIDTH * NDS_HEIGHT],
            total_frames: 0,
        };
        emu.refresh_placeholder_framebuffer();
        Ok(emu)
    }

    fn refresh_placeholder_framebuffer(&mut self) {
        let accent_seed = self
            .header
            .game_code
            .bytes()
            .chain(self.header.maker_code.bytes())
            .fold(0u32, |acc, byte| acc.wrapping_mul(33).wrapping_add(byte as u32));
        let accent_r = 64 + ((accent_seed >> 0) & 0x3F) as u8;
        let accent_g = 96 + ((accent_seed >> 8) & 0x3F) as u8;
        let accent_b = 128 + ((accent_seed >> 16) & 0x3F) as u8;
        let sweep_line = (self.total_frames as usize * 2) % NDS_SCREEN_HEIGHT;

        for y in 0..NDS_HEIGHT {
            let in_top_screen = y < NDS_SCREEN_HEIGHT;
            let screen_y = if in_top_screen { y } else { y - NDS_SCREEN_HEIGHT };

            for x in 0..NDS_WIDTH {
                let idx = y * NDS_WIDTH + x;
                let border = x < 4
                    || x >= NDS_WIDTH - 4
                    || screen_y < 4
                    || screen_y >= NDS_SCREEN_HEIGHT - 4;
                let sweep_hit = screen_y.abs_diff(sweep_line) <= 1;

                self.framebuffer[idx] = if border {
                    argb(42, 48, 56)
                } else if sweep_hit {
                    argb(accent_r, accent_g, accent_b)
                } else if in_top_screen {
                    argb(22, 28, 36)
                } else {
                    argb(18, 23, 30)
                };
            }
        }
    }

    fn run_arm9_slice(&mut self, target_cycles: u32) {
        let mut arm9_cycles = 0;

        while arm9_cycles < target_cycles {
            if self.bus.arm9_halt {
                self.arm9.halted = true;
                self.bus.arm9_halt = false;
            }

            if self.bus.arm9_check_irq() {
                self.arm9.handle_irq();
            }

            let arm9 = &mut self.arm9;
            let bus = &mut self.bus;
            let cycles = {
                let mut arm9_bus = bus.arm9_view();
                arm9.step(&mut arm9_bus)
            };

            bus.tick_arm9(cycles);
            arm9_cycles += cycles;
        }
    }

    fn run_arm7_slice(&mut self, target_cycles: u32) {
        let mut arm7_cycles = 0;

        while arm7_cycles < target_cycles {
            if self.bus.halt {
                self.arm7.halted = true;
                self.bus.halt = false;
            }

            if self.bus.check_irq() {
                self.arm7.handle_irq();
            }

            let cycles = self.arm7.step(&mut self.bus);
            self.bus.tick(cycles);
            arm7_cycles += cycles;
        }
    }

    pub fn run_frame(&mut self) -> &[u32] {
        self.run_arm9_slice(NDS_ARM9_CYCLES_PER_FRAME);
        self.run_arm7_slice(NDS_ARM7_CYCLES_PER_FRAME);
        self.total_frames += 1;
        self.refresh_placeholder_framebuffer();
        &self.framebuffer
    }

    pub fn screen_width(&self) -> u32 {
        NDS_WIDTH as u32
    }

    pub fn screen_height(&self) -> u32 {
        NDS_HEIGHT as u32
    }

    pub fn audio_buffer(&mut self) -> Vec<f32> {
        Vec::new()
    }
}

fn argb(r: u8, g: u8, b: u8) -> u32 {
    0xFF00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

#[cfg(test)]
mod tests {
    use super::{NdsEmulator, NdsRomHeader};

    fn build_test_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 0x400];
        rom[0x000..0x00C].copy_from_slice(b"TEST CART   ");
        rom[0x00C..0x010].copy_from_slice(b"TST0");
        rom[0x010..0x012].copy_from_slice(b"AB");
        rom[0x012] = 0;
        rom[0x013] = 0;
        rom[0x014] = 7;

        rom[0x020..0x024].copy_from_slice(&0x0000_0200u32.to_le_bytes());
        rom[0x024..0x028].copy_from_slice(&0x0200_0000u32.to_le_bytes());
        rom[0x028..0x02C].copy_from_slice(&0x0200_0000u32.to_le_bytes());
        rom[0x02C..0x030].copy_from_slice(&0x0000_0010u32.to_le_bytes());
        rom[0x030..0x034].copy_from_slice(&0x0000_0300u32.to_le_bytes());
        rom[0x034..0x038].copy_from_slice(&0x0380_0000u32.to_le_bytes());
        rom[0x038..0x03C].copy_from_slice(&0x0380_0000u32.to_le_bytes());
        rom[0x03C..0x040].copy_from_slice(&0x0000_0010u32.to_le_bytes());

        rom[0x200..0x204].copy_from_slice(&0xE581_0000u32.to_le_bytes());
        rom[0x204..0x208].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes());
        rom[0x300..0x304].copy_from_slice(&0xE581_0000u32.to_le_bytes());
        rom[0x304..0x308].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes());
        for (index, byte) in (0u8..8).enumerate() {
            rom[0x208 + index] = byte.wrapping_add(0x80);
            rom[0x308 + index] = byte.wrapping_add(0xC0);
        }

        rom
    }

    #[test]
    fn nds_header_parser_extracts_program_sections() {
        let rom = build_test_rom();
        let header = NdsRomHeader::parse(&rom).expect("test ROM header should parse");

        assert_eq!(header.display_title(), "TEST CART");
        assert_eq!(header.game_code, "TST0");
        assert_eq!(header.maker_code, "AB");
        assert_eq!(header.arm9_rom_offset, 0x200);
        assert_eq!(header.arm7_rom_offset, 0x300);
    }

    #[test]
    fn nds_emulator_bootstraps_dual_cpu_state_and_memory() {
        let rom = build_test_rom();
        let emu = NdsEmulator::new(rom.clone()).expect("test ROM should bootstrap");

        assert_eq!(emu.arm9.regs[15], 0x0200_0000);
        assert_eq!(emu.arm7.regs[15], 0x0380_0000);
        assert_eq!(&emu.bus.memory.main_ram[..16], &rom[0x200..0x210]);
        assert_eq!(&emu.bus.memory.arm7_wram[..8], &rom[0x300..0x308]);
    }

    #[test]
    fn nds_emulator_runs_arm9_against_shared_bus_state() {
        let rom = build_test_rom();
        let mut emu = NdsEmulator::new(rom).expect("test ROM should bootstrap");

        emu.arm9.regs[0] = 0xCAFE_BABE;
        emu.arm9.regs[1] = 0x0200_0020;
        emu.run_arm9_slice(3);

        assert_eq!(u32::from_le_bytes(emu.bus.memory.main_ram[0x20..0x24].try_into().unwrap()), 0xCAFE_BABE);
        assert!(emu.bus.arm9_cycles >= 3);
    }

    #[test]
    fn nds_emulator_rejects_short_roms() {
        let result = NdsEmulator::new(vec![0; 0x80]);
        assert!(matches!(result, Err(ref error) if error.contains("too small")));
    }
}