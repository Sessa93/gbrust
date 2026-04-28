use serde::{Deserialize, Serialize};

use crate::cpu::arm7tdmi::Arm7Bus;
use crate::emulator::nds::NdsRomHeader;
use crate::input::{NdsInput, NdsKey};

const ARM7_BIOS_SIZE: usize = 0x4000;
const ARM9_BIOS_SIZE: usize = 0x8000;
const IO_SIZE: usize = 0x1000;
const ARM9_BIOS_BASE: u32 = 0xFFFF_0000;
const MAIN_RAM_BASE: u32 = 0x0200_0000;
const MAIN_RAM_SIZE: usize = 4 * 1024 * 1024;
const SHARED_WRAM_BASE: u32 = 0x0300_0000;
const SHARED_WRAM_SIZE: usize = 32 * 1024;
const ARM7_WRAM_BASE: u32 = 0x0380_0000;
const ARM7_WRAM_SIZE: usize = 64 * 1024;
const PALETTE_BASE: u32 = 0x0500_0000;
const PALETTE_SIZE: usize = 0x1000;
const VRAM_BASE: u32 = 0x0600_0000;
const VRAM_SIZE: usize = 0x000A_4000;
const OAM_BASE: u32 = 0x0700_0000;
const OAM_SIZE: usize = 0x1000;
const CART_BASE: u32 = 0x0800_0000;
const IO_BASE: u32 = 0x0400_0000;

const REG_KEYINPUT: u32 = 0x0400_0130;
const REG_KEYINPUT_HI: u32 = REG_KEYINPUT + 1;
const REG_EXTKEYIN: u32 = 0x0400_0136;
const REG_EXTKEYIN_HI: u32 = REG_EXTKEYIN + 1;
const REG_IME: u32 = 0x0400_0208;
const REG_IME_HI_1: u32 = REG_IME + 1;
const REG_IME_HI_2: u32 = REG_IME + 2;
const REG_IME_HI_3: u32 = REG_IME + 3;
const REG_IE: u32 = 0x0400_0210;
const REG_IE_HI_1: u32 = REG_IE + 1;
const REG_IE_HI_2: u32 = REG_IE + 2;
const REG_IE_HI_3: u32 = REG_IE + 3;
const REG_IF: u32 = 0x0400_0214;
const REG_IF_HI_1: u32 = REG_IF + 1;
const REG_IF_HI_2: u32 = REG_IF + 2;
const REG_IF_HI_3: u32 = REG_IF + 3;
const REG_POSTFLG: u32 = 0x0400_0300;
const REG_HALTCNT: u32 = 0x0400_0301;

#[derive(Clone, Serialize, Deserialize)]
pub struct NdsMemory {
    pub main_ram: Vec<u8>,
    pub shared_wram: Vec<u8>,
    pub arm7_wram: Vec<u8>,
    pub vram: Vec<u8>,
    pub palette: Vec<u8>,
    pub oam: Vec<u8>,
}

impl NdsMemory {
    pub fn new() -> Self {
        Self {
            main_ram: vec![0; MAIN_RAM_SIZE],
            shared_wram: vec![0; SHARED_WRAM_SIZE],
            arm7_wram: vec![0; ARM7_WRAM_SIZE],
            vram: vec![0; VRAM_SIZE],
            palette: vec![0; PALETTE_SIZE],
            oam: vec![0; OAM_SIZE],
        }
    }

    pub fn load_program_sections(&mut self, rom: &[u8], header: &NdsRomHeader) -> Result<(), String> {
        let arm9_start = header.arm9_rom_offset as usize;
        let arm9_end = (header.arm9_rom_offset + header.arm9_size) as usize;
        let arm7_start = header.arm7_rom_offset as usize;
        let arm7_end = (header.arm7_rom_offset + header.arm7_size) as usize;

        self.load_section("ARM9", header.arm9_ram_address, &rom[arm9_start..arm9_end])?;
        self.load_section("ARM7", header.arm7_ram_address, &rom[arm7_start..arm7_end])?;

        Ok(())
    }

    fn load_section(&mut self, name: &str, ram_address: u32, data: &[u8]) -> Result<(), String> {
        if ram_address >= MAIN_RAM_BASE && ram_address < MAIN_RAM_BASE + MAIN_RAM_SIZE as u32 {
            return Self::copy_to_region(name, ram_address, data, MAIN_RAM_BASE, &mut self.main_ram);
        }

        if ram_address >= SHARED_WRAM_BASE && ram_address < SHARED_WRAM_BASE + SHARED_WRAM_SIZE as u32 {
            return Self::copy_to_region(
                name,
                ram_address,
                data,
                SHARED_WRAM_BASE,
                &mut self.shared_wram,
            );
        }

        if ram_address >= ARM7_WRAM_BASE && ram_address < ARM7_WRAM_BASE + ARM7_WRAM_SIZE as u32 {
            return Self::copy_to_region(name, ram_address, data, ARM7_WRAM_BASE, &mut self.arm7_wram);
        }

        Err(format!(
            "{} RAM destination 0x{:08X} is outside the currently modelled NDS memory regions",
            name,
            ram_address
        ))
    }

    fn copy_to_region(
        name: &str,
        ram_address: u32,
        data: &[u8],
        base_address: u32,
        region: &mut [u8],
    ) -> Result<(), String> {
        let offset = (ram_address - base_address) as usize;
        let end = offset + data.len();
        if end > region.len() {
            return Err(format!(
                "{} program does not fit in region starting at 0x{:08X}: end offset 0x{:X}, region size 0x{:X}",
                name,
                base_address,
                end,
                region.len()
            ));
        }

        region[offset..end].copy_from_slice(data);
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct NdsBus {
    pub input: NdsInput,
    pub memory: NdsMemory,
    pub io: Vec<u8>,
    pub bios7: Vec<u8>,
    pub bios9: Vec<u8>,
    pub cartridge_rom: Vec<u8>,
    pub ie: u32,
    pub iflag: u32,
    pub ime: bool,
    pub halt: bool,
    pub postflg: u8,
    pub last_bios_value: u32,
    pub cycles: u64,
    pub arm9_ie: u32,
    pub arm9_iflag: u32,
    pub arm9_ime: bool,
    pub arm9_halt: bool,
    pub arm9_postflg: u8,
    pub arm9_last_bios_value: u32,
    pub arm9_cycles: u64,
}

impl NdsBus {
    pub fn new(rom: Vec<u8>, header: &NdsRomHeader) -> Result<Self, String> {
        let mut memory = NdsMemory::new();
        memory.load_program_sections(&rom, header)?;

        Ok(Self {
            input: NdsInput::new(),
            memory,
            io: vec![0; IO_SIZE],
            bios7: Self::generate_hle_bios7(),
            bios9: Self::generate_hle_bios9(),
            cartridge_rom: rom,
            ie: 0,
            iflag: 0,
            ime: false,
            halt: false,
            postflg: 1,
            last_bios_value: 0,
            cycles: 0,
            arm9_ie: 0,
            arm9_iflag: 0,
            arm9_ime: false,
            arm9_halt: false,
            arm9_postflg: 1,
            arm9_last_bios_value: 0,
            arm9_cycles: 0,
        })
    }

    fn generate_hle_bios7() -> Vec<u8> {
        let mut bios = vec![0u8; ARM7_BIOS_SIZE];
        let movs_pc_lr: u32 = 0xE1B0_F00E;
        bios[0x08..0x0C].copy_from_slice(&movs_pc_lr.to_le_bytes());
        bios
    }

    fn generate_hle_bios9() -> Vec<u8> {
        let mut bios = vec![0u8; ARM9_BIOS_SIZE];
        let movs_pc_lr: u32 = 0xE1B0_F00E;
        bios[0x08..0x0C].copy_from_slice(&movs_pc_lr.to_le_bytes());
        bios
    }

    pub fn tick(&mut self, cycles: u32) {
        self.cycles += cycles as u64;
        if self.halt && self.check_irq() {
            self.halt = false;
        }
    }

    pub fn tick_arm9(&mut self, cycles: u32) {
        self.arm9_cycles += cycles as u64;
        if self.arm9_halt && self.arm9_check_irq() {
            self.arm9_halt = false;
        }
    }

    pub fn check_irq(&self) -> bool {
        self.ime && (self.ie & self.iflag) != 0
    }

    pub fn arm9_check_irq(&self) -> bool {
        self.arm9_ime && (self.arm9_ie & self.arm9_iflag) != 0
    }

    pub fn arm9_view(&mut self) -> NdsArm9Bus<'_> {
        NdsArm9Bus { bus: self }
    }

    pub fn set_key(&mut self, key: NdsKey, pressed: bool) {
        if pressed {
            self.input.key_down(key);
        } else {
            self.input.key_up(key);
        }
    }

    fn read_io_byte(&self, addr: u32) -> u8 {
        match addr {
            REG_KEYINPUT => self.input.read_keyinput() as u8,
            REG_KEYINPUT_HI => (self.input.read_keyinput() >> 8) as u8,
            REG_EXTKEYIN => self.input.read_extkeyin() as u8,
            REG_EXTKEYIN_HI => (self.input.read_extkeyin() >> 8) as u8,
            REG_IME => self.ime as u8,
            REG_IME_HI_1 | REG_IME_HI_2 | REG_IME_HI_3 => 0,
            REG_IE => self.ie as u8,
            REG_IE_HI_1 => (self.ie >> 8) as u8,
            REG_IE_HI_2 => (self.ie >> 16) as u8,
            REG_IE_HI_3 => (self.ie >> 24) as u8,
            REG_IF => self.iflag as u8,
            REG_IF_HI_1 => (self.iflag >> 8) as u8,
            REG_IF_HI_2 => (self.iflag >> 16) as u8,
            REG_IF_HI_3 => (self.iflag >> 24) as u8,
            REG_POSTFLG => self.postflg,
            REG_HALTCNT => (self.halt as u8) << 7,
            _ if (IO_BASE..IO_BASE + IO_SIZE as u32).contains(&addr) => {
                self.io[(addr - IO_BASE) as usize]
            }
            _ => 0,
        }
    }

    fn read_arm9_io_byte(&self, addr: u32) -> u8 {
        match addr {
            REG_IME => self.arm9_ime as u8,
            REG_IME_HI_1 | REG_IME_HI_2 | REG_IME_HI_3 => 0,
            REG_IE => self.arm9_ie as u8,
            REG_IE_HI_1 => (self.arm9_ie >> 8) as u8,
            REG_IE_HI_2 => (self.arm9_ie >> 16) as u8,
            REG_IE_HI_3 => (self.arm9_ie >> 24) as u8,
            REG_IF => self.arm9_iflag as u8,
            REG_IF_HI_1 => (self.arm9_iflag >> 8) as u8,
            REG_IF_HI_2 => (self.arm9_iflag >> 16) as u8,
            REG_IF_HI_3 => (self.arm9_iflag >> 24) as u8,
            REG_POSTFLG => self.arm9_postflg,
            REG_HALTCNT => (self.arm9_halt as u8) << 7,
            _ if (IO_BASE..IO_BASE + IO_SIZE as u32).contains(&addr) => {
                self.io[(addr - IO_BASE) as usize]
            }
            _ => 0,
        }
    }

    fn write_io_byte(&mut self, addr: u32, value: u8) {
        match addr {
            REG_IME => self.ime = value & 0x01 != 0,
            REG_IE => self.ie = (self.ie & !0x0000_00FF) | value as u32,
            REG_IE_HI_1 => self.ie = (self.ie & !0x0000_FF00) | ((value as u32) << 8),
            REG_IE_HI_2 => self.ie = (self.ie & !0x00FF_0000) | ((value as u32) << 16),
            REG_IE_HI_3 => self.ie = (self.ie & !0xFF00_0000) | ((value as u32) << 24),
            REG_IF => self.iflag &= !(value as u32),
            REG_IF_HI_1 => self.iflag &= !((value as u32) << 8),
            REG_IF_HI_2 => self.iflag &= !((value as u32) << 16),
            REG_IF_HI_3 => self.iflag &= !((value as u32) << 24),
            REG_POSTFLG => self.postflg = value,
            REG_HALTCNT => self.halt = value & 0x80 != 0,
            _ if (IO_BASE..IO_BASE + IO_SIZE as u32).contains(&addr) => {
                self.io[(addr - IO_BASE) as usize] = value;
            }
            _ => {}
        }
    }

    fn write_arm9_io_byte(&mut self, addr: u32, value: u8) {
        match addr {
            REG_IME => self.arm9_ime = value & 0x01 != 0,
            REG_IE => self.arm9_ie = (self.arm9_ie & !0x0000_00FF) | value as u32,
            REG_IE_HI_1 => self.arm9_ie = (self.arm9_ie & !0x0000_FF00) | ((value as u32) << 8),
            REG_IE_HI_2 => self.arm9_ie = (self.arm9_ie & !0x00FF_0000) | ((value as u32) << 16),
            REG_IE_HI_3 => self.arm9_ie = (self.arm9_ie & !0xFF00_0000) | ((value as u32) << 24),
            REG_IF => self.arm9_iflag &= !(value as u32),
            REG_IF_HI_1 => self.arm9_iflag &= !((value as u32) << 8),
            REG_IF_HI_2 => self.arm9_iflag &= !((value as u32) << 16),
            REG_IF_HI_3 => self.arm9_iflag &= !((value as u32) << 24),
            REG_POSTFLG => self.arm9_postflg = value,
            REG_HALTCNT => self.arm9_halt = value & 0x80 != 0,
            _ if (IO_BASE..IO_BASE + IO_SIZE as u32).contains(&addr) => {
                self.io[(addr - IO_BASE) as usize] = value;
            }
            _ => {}
        }
    }

    fn read_region(region: &[u8], base: u32, addr: u32) -> u8 {
        region[(addr - base) as usize]
    }

    fn write_region(region: &mut [u8], base: u32, addr: u32, value: u8) {
        region[(addr - base) as usize] = value;
    }
}

pub struct NdsArm9Bus<'a> {
    bus: &'a mut NdsBus,
}

impl Arm7Bus for NdsArm9Bus<'_> {
    fn read8(&self, addr: u32) -> u8 {
        match addr {
            MAIN_RAM_BASE..=0x023F_FFFF => NdsBus::read_region(&self.bus.memory.main_ram, MAIN_RAM_BASE, addr),
            SHARED_WRAM_BASE..=0x0300_7FFF => {
                NdsBus::read_region(&self.bus.memory.shared_wram, SHARED_WRAM_BASE, addr)
            }
            IO_BASE..=0x0400_0FFF => self.bus.read_arm9_io_byte(addr),
            PALETTE_BASE..=0x0500_0FFF => NdsBus::read_region(&self.bus.memory.palette, PALETTE_BASE, addr),
            VRAM_BASE..=0x060A_3FFF => NdsBus::read_region(&self.bus.memory.vram, VRAM_BASE, addr),
            OAM_BASE..=0x0700_03FF => NdsBus::read_region(&self.bus.memory.oam, OAM_BASE, addr),
            CART_BASE..=0x09FF_FFFF => {
                let offset = (addr - CART_BASE) as usize;
                self.bus.cartridge_rom.get(offset).copied().unwrap_or(0xFF)
            }
            ARM9_BIOS_BASE..=0xFFFF_7FFF => NdsBus::read_region(&self.bus.bios9, ARM9_BIOS_BASE, addr),
            _ => 0,
        }
    }

    fn read16(&mut self, addr: u32) -> u16 {
        u16::from_le_bytes([self.read8(addr), self.read8(addr.wrapping_add(1))])
    }

    fn read32(&mut self, addr: u32) -> u32 {
        let value = u32::from_le_bytes([
            self.read8(addr),
            self.read8(addr.wrapping_add(1)),
            self.read8(addr.wrapping_add(2)),
            self.read8(addr.wrapping_add(3)),
        ]);
        self.bus.arm9_last_bios_value = value;
        value
    }

    fn write8(&mut self, addr: u32, val: u8) {
        match addr {
            MAIN_RAM_BASE..=0x023F_FFFF => {
                NdsBus::write_region(&mut self.bus.memory.main_ram, MAIN_RAM_BASE, addr, val)
            }
            SHARED_WRAM_BASE..=0x0300_7FFF => {
                NdsBus::write_region(&mut self.bus.memory.shared_wram, SHARED_WRAM_BASE, addr, val)
            }
            IO_BASE..=0x0400_0FFF => self.bus.write_arm9_io_byte(addr, val),
            PALETTE_BASE..=0x0500_0FFF => {
                NdsBus::write_region(&mut self.bus.memory.palette, PALETTE_BASE, addr, val)
            }
            VRAM_BASE..=0x060A_3FFF => NdsBus::write_region(&mut self.bus.memory.vram, VRAM_BASE, addr, val),
            OAM_BASE..=0x0700_03FF => NdsBus::write_region(&mut self.bus.memory.oam, OAM_BASE, addr, val),
            _ => {}
        }
    }

    fn write16(&mut self, addr: u32, val: u16) {
        let bytes = val.to_le_bytes();
        self.write8(addr, bytes[0]);
        self.write8(addr.wrapping_add(1), bytes[1]);
    }

    fn write32(&mut self, addr: u32, val: u32) {
        let bytes = val.to_le_bytes();
        self.write8(addr, bytes[0]);
        self.write8(addr.wrapping_add(1), bytes[1]);
        self.write8(addr.wrapping_add(2), bytes[2]);
        self.write8(addr.wrapping_add(3), bytes[3]);
    }
}

impl Arm7Bus for NdsBus {
    fn read8(&self, addr: u32) -> u8 {
        match addr {
            0x0000_0000..=0x0000_3FFF => Self::read_region(&self.bios7, 0x0000_0000, addr),
            MAIN_RAM_BASE..=0x023F_FFFF => Self::read_region(&self.memory.main_ram, MAIN_RAM_BASE, addr),
            SHARED_WRAM_BASE..=0x0300_7FFF => {
                Self::read_region(&self.memory.shared_wram, SHARED_WRAM_BASE, addr)
            }
            ARM7_WRAM_BASE..=0x0380_FFFF => Self::read_region(&self.memory.arm7_wram, ARM7_WRAM_BASE, addr),
            IO_BASE..=0x0400_0FFF => self.read_io_byte(addr),
            PALETTE_BASE..=0x0500_0FFF => Self::read_region(&self.memory.palette, PALETTE_BASE, addr),
            VRAM_BASE..=0x060A_3FFF => Self::read_region(&self.memory.vram, VRAM_BASE, addr),
            OAM_BASE..=0x0700_03FF => Self::read_region(&self.memory.oam, OAM_BASE, addr),
            CART_BASE..=0x09FF_FFFF => {
                let offset = (addr - CART_BASE) as usize;
                self.cartridge_rom.get(offset).copied().unwrap_or(0xFF)
            }
            _ => 0,
        }
    }

    fn read16(&mut self, addr: u32) -> u16 {
        u16::from_le_bytes([self.read8(addr), self.read8(addr.wrapping_add(1))])
    }

    fn read32(&mut self, addr: u32) -> u32 {
        let value = u32::from_le_bytes([
            self.read8(addr),
            self.read8(addr.wrapping_add(1)),
            self.read8(addr.wrapping_add(2)),
            self.read8(addr.wrapping_add(3)),
        ]);
        self.last_bios_value = value;
        value
    }

    fn write8(&mut self, addr: u32, val: u8) {
        match addr {
            MAIN_RAM_BASE..=0x023F_FFFF => Self::write_region(&mut self.memory.main_ram, MAIN_RAM_BASE, addr, val),
            SHARED_WRAM_BASE..=0x0300_7FFF => {
                Self::write_region(&mut self.memory.shared_wram, SHARED_WRAM_BASE, addr, val)
            }
            ARM7_WRAM_BASE..=0x0380_FFFF => {
                Self::write_region(&mut self.memory.arm7_wram, ARM7_WRAM_BASE, addr, val)
            }
            IO_BASE..=0x0400_0FFF => self.write_io_byte(addr, val),
            PALETTE_BASE..=0x0500_0FFF => Self::write_region(&mut self.memory.palette, PALETTE_BASE, addr, val),
            VRAM_BASE..=0x060A_3FFF => Self::write_region(&mut self.memory.vram, VRAM_BASE, addr, val),
            OAM_BASE..=0x0700_03FF => Self::write_region(&mut self.memory.oam, OAM_BASE, addr, val),
            _ => {}
        }
    }

    fn write16(&mut self, addr: u32, val: u16) {
        let bytes = val.to_le_bytes();
        self.write8(addr, bytes[0]);
        self.write8(addr.wrapping_add(1), bytes[1]);
    }

    fn write32(&mut self, addr: u32, val: u32) {
        let bytes = val.to_le_bytes();
        self.write8(addr, bytes[0]);
        self.write8(addr.wrapping_add(1), bytes[1]);
        self.write8(addr.wrapping_add(2), bytes[2]);
        self.write8(addr.wrapping_add(3), bytes[3]);
    }
}

#[cfg(test)]
mod tests {
    use super::{NdsBus, REG_EXTKEYIN, REG_KEYINPUT};
    use crate::cpu::arm7tdmi::{Arm7Bus, Arm7Tdmi};
    use crate::emulator::nds::NdsRomHeader;
    use crate::input::NdsKey;

    fn build_test_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 0x400];
        rom[0x000..0x00C].copy_from_slice(b"TEST CART   ");
        rom[0x00C..0x010].copy_from_slice(b"TST0");
        rom[0x010..0x012].copy_from_slice(b"AB");
        rom[0x014] = 7;

        rom[0x020..0x024].copy_from_slice(&0x0000_0200u32.to_le_bytes());
        rom[0x024..0x028].copy_from_slice(&0x0200_0000u32.to_le_bytes());
        rom[0x028..0x02C].copy_from_slice(&0x0200_0000u32.to_le_bytes());
        rom[0x02C..0x030].copy_from_slice(&0x0000_0010u32.to_le_bytes());
        rom[0x030..0x034].copy_from_slice(&0x0000_0300u32.to_le_bytes());
        rom[0x034..0x038].copy_from_slice(&0x0380_0000u32.to_le_bytes());
        rom[0x038..0x03C].copy_from_slice(&0x0380_0000u32.to_le_bytes());
        rom[0x03C..0x040].copy_from_slice(&0x0000_0008u32.to_le_bytes());

        rom[0x300..0x304].copy_from_slice(&0xE581_0000u32.to_le_bytes());
        rom[0x304..0x308].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes());

        rom
    }

    #[test]
    fn nds_bus_exposes_input_registers() {
        let rom = build_test_rom();
        let header = NdsRomHeader::parse(&rom).expect("test ROM header should parse");
        let mut bus = NdsBus::new(rom, &header).expect("bus should initialize");

        assert_eq!(bus.read16(REG_KEYINPUT), 0x03FF);
        assert_eq!(bus.read16(REG_EXTKEYIN), 0x007F);

        bus.set_key(NdsKey::A, true);
        bus.set_key(NdsKey::X, true);

        assert_eq!(bus.read16(REG_KEYINPUT) & 0x0001, 0);
        assert_eq!(bus.read16(REG_EXTKEYIN) & 0x0001, 0);
    }

    #[test]
    fn nds_bus_runs_arm7_memory_accesses() {
        let rom = build_test_rom();
        let header = NdsRomHeader::parse(&rom).expect("test ROM header should parse");
        let mut bus = NdsBus::new(rom, &header).expect("bus should initialize");
        let mut cpu = Arm7Tdmi::new();

        cpu.regs[15] = 0x0380_0000;
        cpu.regs[0] = 0xDEAD_BEEF;
        cpu.regs[1] = 0x0380_0020;
        cpu.step(&mut bus);

        assert_eq!(bus.read32(0x0380_0020), 0xDEAD_BEEF);
    }
}