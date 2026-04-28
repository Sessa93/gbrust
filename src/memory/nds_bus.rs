use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

use crate::cpu::arm7tdmi::Arm7Bus;
use crate::dma::GbaDma;
use crate::emulator::nds::NdsRomHeader;
use crate::input::{NdsInput, NdsKey};
use crate::timer::GbaTimers;

const ARM7_BIOS_SIZE: usize = 0x4000;
const ARM9_BIOS_SIZE: usize = 0x8000;
const IO_SIZE: usize = 0x2000;
const ARM9_BIOS_BASE: u32 = 0xFFFF_0000;
const SCANLINE_CYCLES: u32 = 1232;
const HBLANK_START_CYCLES: u32 = 960;
const VISIBLE_SCANLINES: u16 = 192;
const TOTAL_SCANLINES: u16 = 263;
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
const IO_END: u32 = IO_BASE + IO_SIZE as u32 - 1;
const PPU_MAIN_BASE: u32 = 0x0400_0000;
const PPU_SUB_BASE: u32 = 0x0400_1000;
const PPU_MAIN_END: u32 = PPU_MAIN_BASE + 0x06D;
const PPU_SUB_END: u32 = PPU_SUB_BASE + 0x06D;

const REG_KEYINPUT: u32 = 0x0400_0130;
const REG_KEYINPUT_HI: u32 = REG_KEYINPUT + 1;
const REG_EXTKEYIN: u32 = 0x0400_0136;
const REG_EXTKEYIN_HI: u32 = REG_EXTKEYIN + 1;
const REG_DISPCNT: u32 = 0x0400_0000;
const REG_DISPCNT_END: u32 = REG_DISPCNT + 3;
const REG_DISPSTAT: u32 = 0x0400_0004;
const REG_DISPSTAT_HI: u32 = REG_DISPSTAT + 1;
const REG_VCOUNT: u32 = 0x0400_0006;
const REG_VCOUNT_HI: u32 = REG_VCOUNT + 1;
const REG_IPCSYNC: u32 = 0x0400_0180;
const REG_IPCSYNC_HI: u32 = REG_IPCSYNC + 1;
const REG_IPCFIFOCNT: u32 = 0x0400_0184;
const REG_IPCFIFOCNT_HI: u32 = REG_IPCFIFOCNT + 1;
const REG_IPCFIFOSEND: u32 = 0x0400_0188;
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
const IPC_FIFO_CAPACITY: usize = 16;

#[derive(Clone, Copy)]
enum NdsCpu {
    Arm7,
    Arm9,
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct NdsIpcState {
    pub arm7_sync_out: u8,
    pub arm9_sync_out: u8,
    pub arm7_fifo_enabled: bool,
    pub arm9_fifo_enabled: bool,
    pub arm7_recv_irq_enabled: bool,
    pub arm9_recv_irq_enabled: bool,
    pub arm7_recv_fifo: VecDeque<u32>,
    pub arm9_recv_fifo: VecDeque<u32>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct NdsMemory {
    pub main_ram: Vec<u8>,
    pub shared_wram: Vec<u8>,
    pub arm7_wram: Vec<u8>,
    pub vram: Vec<u8>,
    pub palette: Vec<u8>,
    pub oam: Vec<u8>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct NdsPpuRegisters {
    pub dispcnt: u32,
    pub bgcnt: [u16; 4],
    pub bghofs: [u16; 4],
    pub bgvofs: [u16; 4],
    pub bg_pa: [i16; 2],
    pub bg_pb: [i16; 2],
    pub bg_pc: [i16; 2],
    pub bg_pd: [i16; 2],
    pub bg_ref_x: [i32; 2],
    pub bg_ref_y: [i32; 2],
    pub winh: [u16; 2],
    pub winv: [u16; 2],
    pub winin: u16,
    pub winout: u16,
    pub mosaic: u16,
    pub bldcnt: u16,
    pub bldalpha: u16,
    pub bldy: u16,
    pub disp3dcnt: u16,
    pub dispcapcnt: u32,
    pub master_bright: u16,
}

impl NdsPpuRegisters {
    fn new() -> Self {
        Self {
            dispcnt: 0,
            bgcnt: [0; 4],
            bghofs: [0; 4],
            bgvofs: [0; 4],
            bg_pa: [0x100; 2],
            bg_pb: [0; 2],
            bg_pc: [0; 2],
            bg_pd: [0x100; 2],
            bg_ref_x: [0; 2],
            bg_ref_y: [0; 2],
            winh: [0; 2],
            winv: [0; 2],
            winin: 0,
            winout: 0,
            mosaic: 0,
            bldcnt: 0,
            bldalpha: 0,
            bldy: 0,
            disp3dcnt: 0,
            dispcapcnt: 0,
            master_bright: 0,
        }
    }

    fn read_byte(&self, offset: u32) -> u8 {
        match offset {
            0x000..=0x003 => ((self.dispcnt >> ((offset & 3) * 8)) & 0xFF) as u8,
            0x008..=0x00F => {
                let bg = ((offset - 0x008) / 2) as usize;
                let value = self.bgcnt[bg];
                if offset & 1 == 0 { value as u8 } else { (value >> 8) as u8 }
            }
            0x010..=0x01F => {
                let reg = ((offset - 0x010) / 2) as usize;
                let bg = reg / 2;
                let is_vofs = reg & 1 == 1;
                let value = if is_vofs { self.bgvofs[bg] } else { self.bghofs[bg] };
                if offset & 1 == 0 { value as u8 } else { (value >> 8) as u8 }
            }
            0x020..=0x03F => self.read_affine_byte(offset),
            0x040 => self.winh[0] as u8,
            0x041 => (self.winh[0] >> 8) as u8,
            0x042 => self.winh[1] as u8,
            0x043 => (self.winh[1] >> 8) as u8,
            0x044 => self.winv[0] as u8,
            0x045 => (self.winv[0] >> 8) as u8,
            0x046 => self.winv[1] as u8,
            0x047 => (self.winv[1] >> 8) as u8,
            0x048 => self.winin as u8,
            0x049 => (self.winin >> 8) as u8,
            0x04A => self.winout as u8,
            0x04B => (self.winout >> 8) as u8,
            0x04C => self.mosaic as u8,
            0x04D => (self.mosaic >> 8) as u8,
            0x050 => self.bldcnt as u8,
            0x051 => (self.bldcnt >> 8) as u8,
            0x052 => self.bldalpha as u8,
            0x053 => (self.bldalpha >> 8) as u8,
            0x054 => self.bldy as u8,
            0x055 => (self.bldy >> 8) as u8,
            0x060 => self.disp3dcnt as u8,
            0x061 => (self.disp3dcnt >> 8) as u8,
            0x064..=0x067 => ((self.dispcapcnt >> ((offset - 0x064) * 8)) & 0xFF) as u8,
            0x06C => self.master_bright as u8,
            0x06D => (self.master_bright >> 8) as u8,
            _ => 0,
        }
    }

    fn write_byte(&mut self, offset: u32, value: u8) {
        match offset {
            0x000..=0x003 => {
                let shift = (offset & 3) * 8;
                self.dispcnt = (self.dispcnt & !(0xFF << shift)) | ((value as u32) << shift);
            }
            0x008..=0x00F => {
                let bg = ((offset - 0x008) / 2) as usize;
                if offset & 1 == 0 {
                    self.bgcnt[bg] = (self.bgcnt[bg] & 0xFF00) | value as u16;
                } else {
                    self.bgcnt[bg] = (self.bgcnt[bg] & 0x00FF) | ((value as u16) << 8);
                }
            }
            0x010..=0x01F => {
                let reg = ((offset - 0x010) / 2) as usize;
                let bg = reg / 2;
                let target = if reg & 1 == 1 { &mut self.bgvofs[bg] } else { &mut self.bghofs[bg] };
                if offset & 1 == 0 {
                    *target = (*target & 0xFF00) | value as u16;
                } else {
                    *target = (*target & 0x00FF) | ((value as u16) << 8);
                }
            }
            0x020..=0x03F => self.write_affine_byte(offset, value),
            0x040 => self.winh[0] = (self.winh[0] & 0xFF00) | value as u16,
            0x041 => self.winh[0] = (self.winh[0] & 0x00FF) | ((value as u16) << 8),
            0x042 => self.winh[1] = (self.winh[1] & 0xFF00) | value as u16,
            0x043 => self.winh[1] = (self.winh[1] & 0x00FF) | ((value as u16) << 8),
            0x044 => self.winv[0] = (self.winv[0] & 0xFF00) | value as u16,
            0x045 => self.winv[0] = (self.winv[0] & 0x00FF) | ((value as u16) << 8),
            0x046 => self.winv[1] = (self.winv[1] & 0xFF00) | value as u16,
            0x047 => self.winv[1] = (self.winv[1] & 0x00FF) | ((value as u16) << 8),
            0x048 => self.winin = (self.winin & 0xFF00) | value as u16,
            0x049 => self.winin = (self.winin & 0x00FF) | ((value as u16) << 8),
            0x04A => self.winout = (self.winout & 0xFF00) | value as u16,
            0x04B => self.winout = (self.winout & 0x00FF) | ((value as u16) << 8),
            0x04C => self.mosaic = (self.mosaic & 0xFF00) | value as u16,
            0x04D => self.mosaic = (self.mosaic & 0x00FF) | ((value as u16) << 8),
            0x050 => self.bldcnt = (self.bldcnt & 0xFF00) | value as u16,
            0x051 => self.bldcnt = (self.bldcnt & 0x00FF) | ((value as u16) << 8),
            0x052 => self.bldalpha = (self.bldalpha & 0xFF00) | value as u16,
            0x053 => self.bldalpha = (self.bldalpha & 0x00FF) | ((value as u16) << 8),
            0x054 => self.bldy = (self.bldy & 0xFF00) | value as u16,
            0x055 => self.bldy = (self.bldy & 0x00FF) | ((value as u16) << 8),
            0x060 => self.disp3dcnt = (self.disp3dcnt & 0xFF00) | value as u16,
            0x061 => self.disp3dcnt = (self.disp3dcnt & 0x00FF) | ((value as u16) << 8),
            0x064..=0x067 => {
                let shift = (offset - 0x064) * 8;
                self.dispcapcnt = (self.dispcapcnt & !(0xFF << shift)) | ((value as u32) << shift);
            }
            0x06C => self.master_bright = (self.master_bright & 0xFF00) | value as u16,
            0x06D => self.master_bright = (self.master_bright & 0x00FF) | ((value as u16) << 8),
            _ => {}
        }
    }

    fn read_affine_byte(&self, offset: u32) -> u8 {
        let bg = if offset >= 0x30 { 1usize } else { 0usize };
        match offset & 0x0F {
            0x0 => self.bg_pa[bg] as u8,
            0x1 => (self.bg_pa[bg] >> 8) as u8,
            0x2 => self.bg_pb[bg] as u8,
            0x3 => (self.bg_pb[bg] >> 8) as u8,
            0x4 => self.bg_pc[bg] as u8,
            0x5 => (self.bg_pc[bg] >> 8) as u8,
            0x6 => self.bg_pd[bg] as u8,
            0x7 => (self.bg_pd[bg] >> 8) as u8,
            0x8..=0xB => ((self.bg_ref_x[bg] as u32 >> ((offset & 0x03) * 8)) & 0xFF) as u8,
            0xC..=0xF => ((self.bg_ref_y[bg] as u32 >> ((offset & 0x03) * 8)) & 0xFF) as u8,
            _ => 0,
        }
    }

    fn write_affine_byte(&mut self, offset: u32, value: u8) {
        let bg = if offset >= 0x30 { 1usize } else { 0usize };
        match offset & 0x0F {
            0x0 => self.bg_pa[bg] = (self.bg_pa[bg] & !0x00FF) | value as i16,
            0x1 => self.bg_pa[bg] = (self.bg_pa[bg] & 0x00FF) | ((value as i16) << 8),
            0x2 => self.bg_pb[bg] = (self.bg_pb[bg] & !0x00FF) | value as i16,
            0x3 => self.bg_pb[bg] = (self.bg_pb[bg] & 0x00FF) | ((value as i16) << 8),
            0x4 => self.bg_pc[bg] = (self.bg_pc[bg] & !0x00FF) | value as i16,
            0x5 => self.bg_pc[bg] = (self.bg_pc[bg] & 0x00FF) | ((value as i16) << 8),
            0x6 => self.bg_pd[bg] = (self.bg_pd[bg] & !0x00FF) | value as i16,
            0x7 => self.bg_pd[bg] = (self.bg_pd[bg] & 0x00FF) | ((value as i16) << 8),
            0x8..=0xB => {
                let shift = (offset & 0x03) * 8;
                let mut raw = self.bg_ref_x[bg] as u32;
                raw = (raw & !(0xFF << shift)) | ((value as u32) << shift);
                self.bg_ref_x[bg] = if raw & (1 << 27) != 0 {
                    (raw | 0xF000_0000) as i32
                } else {
                    (raw & 0x0FFF_FFFF) as i32
                };
            }
            0xC..=0xF => {
                let shift = (offset & 0x03) * 8;
                let mut raw = self.bg_ref_y[bg] as u32;
                raw = (raw & !(0xFF << shift)) | ((value as u32) << shift);
                self.bg_ref_y[bg] = if raw & (1 << 27) != 0 {
                    (raw | 0xF000_0000) as i32
                } else {
                    (raw & 0x0FFF_FFFF) as i32
                };
            }
            _ => {}
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct NdsVideoState {
    pub vcount: u16,
    pub line_cycles: u32,
    pub in_hblank: bool,
}

impl NdsVideoState {
    fn new() -> Self {
        Self {
            vcount: 0,
            line_cycles: 0,
            in_hblank: false,
        }
    }
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
    pub video: NdsVideoState,
    pub ppu_main: NdsPpuRegisters,
    pub ppu_sub: NdsPpuRegisters,
    pub dispstat: u16,
    pub arm9_dispstat: u16,
    pub timers: GbaTimers,
    pub arm9_timers: GbaTimers,
    pub dma: GbaDma,
    pub arm9_dma: GbaDma,
    pub ipc: NdsIpcState,
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
            video: NdsVideoState::new(),
            ppu_main: NdsPpuRegisters::new(),
            ppu_sub: NdsPpuRegisters::new(),
            dispstat: 0,
            arm9_dispstat: 0,
            timers: GbaTimers::new(),
            arm9_timers: GbaTimers::new(),
            dma: GbaDma::new(),
            arm9_dma: GbaDma::new(),
            ipc: NdsIpcState {
                arm7_fifo_enabled: true,
                arm9_fifo_enabled: true,
                ..NdsIpcState::default()
            },
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
        let (timer_irqs, _) = self.timers.tick(cycles);
        self.iflag |= timer_irqs as u32;
        self.process_dma(NdsCpu::Arm7);
        self.cycles += cycles as u64;
        if self.halt && self.check_irq() {
            self.halt = false;
        }
    }

    pub fn tick_arm9(&mut self, cycles: u32) {
        let (timer_irqs, _) = self.arm9_timers.tick(cycles);
        self.arm9_iflag |= timer_irqs as u32;
        self.tick_video(cycles);
        self.process_dma(NdsCpu::Arm9);
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

    pub fn read_dispstat_value(&self) -> u16 {
        self.read_dispstat(NdsCpu::Arm7)
    }

    pub fn read_arm9_dispstat_value(&self) -> u16 {
        self.read_dispstat(NdsCpu::Arm9)
    }

    fn read_ppu_byte(&self, addr: u32) -> Option<u8> {
        match addr {
            PPU_MAIN_BASE..=PPU_MAIN_END => {
                let offset = addr - PPU_MAIN_BASE;
                if (0x004..=0x007).contains(&offset) {
                    None
                } else {
                    Some(self.ppu_main.read_byte(offset))
                }
            }
            PPU_SUB_BASE..=PPU_SUB_END => Some(self.ppu_sub.read_byte(addr - PPU_SUB_BASE)),
            _ => None,
        }
    }

    fn write_ppu_byte(&mut self, addr: u32, value: u8) -> bool {
        match addr {
            PPU_MAIN_BASE..=PPU_MAIN_END => {
                let offset = addr - PPU_MAIN_BASE;
                if (0x004..=0x007).contains(&offset) {
                    false
                } else {
                    self.ppu_main.write_byte(offset, value);
                    true
                }
            }
            PPU_SUB_BASE..=PPU_SUB_END => {
                self.ppu_sub.write_byte(addr - PPU_SUB_BASE, value);
                true
            }
            _ => false,
        }
    }

    fn in_vblank(&self) -> bool {
        self.video.vcount >= VISIBLE_SCANLINES && self.video.vcount < TOTAL_SCANLINES
    }

    fn read_dispstat(&self, cpu: NdsCpu) -> u16 {
        let control = match cpu {
            NdsCpu::Arm7 => self.dispstat,
            NdsCpu::Arm9 => self.arm9_dispstat,
        } & 0xFFF8;

        let vblank = if self.in_vblank() { 1 } else { 0 };
        let hblank = if self.video.in_hblank && self.video.vcount < VISIBLE_SCANLINES { 2 } else { 0 };
        let vcounter = if self.video.vcount == (control >> 8) { 4 } else { 0 };

        control | vblank | hblank | vcounter
    }

    fn write_dispstat_byte(&mut self, cpu: NdsCpu, byte: u32, value: u8) {
        let dispstat = match cpu {
            NdsCpu::Arm7 => &mut self.dispstat,
            NdsCpu::Arm9 => &mut self.arm9_dispstat,
        };

        if byte == 0 {
            *dispstat = (*dispstat & 0xFF07) | ((value as u16) & 0x0038);
        } else {
            *dispstat = (*dispstat & 0x00FF) | ((value as u16) << 8);
        }
    }

    fn tick_video(&mut self, cycles: u32) {
        let mut remaining = cycles;

        while remaining > 0 {
            let boundary = if self.video.in_hblank {
                SCANLINE_CYCLES - self.video.line_cycles
            } else {
                HBLANK_START_CYCLES.saturating_sub(self.video.line_cycles)
            };
            let step = boundary.min(remaining);
            self.video.line_cycles += step;
            remaining -= step;

            if self.video.in_hblank {
                if self.video.line_cycles < SCANLINE_CYCLES {
                    continue;
                }

                self.video.in_hblank = false;
                self.video.line_cycles = 0;
                self.video.vcount += 1;

                if self.video.vcount == VISIBLE_SCANLINES {
                    self.dma.notify_vblank();
                    self.arm9_dma.notify_vblank();
                    if self.dispstat & 0x0008 != 0 {
                        self.iflag |= crate::interrupts::gba::VBLANK as u32;
                    }
                    if self.arm9_dispstat & 0x0008 != 0 {
                        self.arm9_iflag |= crate::interrupts::gba::VBLANK as u32;
                    }
                }

                if self.video.vcount >= TOTAL_SCANLINES {
                    self.video.vcount = 0;
                }

                self.update_vcounter_irq();
            } else {
                if self.video.line_cycles < HBLANK_START_CYCLES {
                    continue;
                }

                self.video.in_hblank = true;
                if self.video.vcount < VISIBLE_SCANLINES {
                    self.dma.notify_hblank();
                    self.arm9_dma.notify_hblank();
                    if self.dispstat & 0x0010 != 0 {
                        self.iflag |= crate::interrupts::gba::HBLANK as u32;
                    }
                    if self.arm9_dispstat & 0x0010 != 0 {
                        self.arm9_iflag |= crate::interrupts::gba::HBLANK as u32;
                    }
                }
            }
        }
    }

    fn update_vcounter_irq(&mut self) {
        if self.video.vcount == (self.dispstat >> 8) && self.dispstat & 0x0020 != 0 {
            self.iflag |= crate::interrupts::gba::VCOUNTER as u32;
        }
        if self.video.vcount == (self.arm9_dispstat >> 8) && self.arm9_dispstat & 0x0020 != 0 {
            self.arm9_iflag |= crate::interrupts::gba::VCOUNTER as u32;
        }
    }

    fn read_ipcsync(&self, cpu: NdsCpu) -> u16 {
        let (local, remote) = match cpu {
            NdsCpu::Arm7 => (self.ipc.arm7_sync_out, self.ipc.arm9_sync_out),
            NdsCpu::Arm9 => (self.ipc.arm9_sync_out, self.ipc.arm7_sync_out),
        };
        (local as u16 & 0x000F) | (((remote as u16) & 0x000F) << 8)
    }

    fn write_ipcsync(&mut self, cpu: NdsCpu, value: u16) {
        let nibble = (value & 0x000F) as u8;
        match cpu {
            NdsCpu::Arm7 => self.ipc.arm7_sync_out = nibble,
            NdsCpu::Arm9 => self.ipc.arm9_sync_out = nibble,
        }
    }

    fn recv_fifo(&self, cpu: NdsCpu) -> &VecDeque<u32> {
        match cpu {
            NdsCpu::Arm7 => &self.ipc.arm7_recv_fifo,
            NdsCpu::Arm9 => &self.ipc.arm9_recv_fifo,
        }
    }

    fn recv_fifo_mut(&mut self, cpu: NdsCpu) -> &mut VecDeque<u32> {
        match cpu {
            NdsCpu::Arm7 => &mut self.ipc.arm7_recv_fifo,
            NdsCpu::Arm9 => &mut self.ipc.arm9_recv_fifo,
        }
    }

    fn peek_ipc_fifo(&self, cpu: NdsCpu) -> u32 {
        self.recv_fifo(cpu).front().copied().unwrap_or(0)
    }

    fn pop_ipc_fifo(&mut self, cpu: NdsCpu) -> u32 {
        self.recv_fifo_mut(cpu).pop_front().unwrap_or(0)
    }

    fn fifo_enabled(&self, cpu: NdsCpu) -> bool {
        match cpu {
            NdsCpu::Arm7 => self.ipc.arm7_fifo_enabled,
            NdsCpu::Arm9 => self.ipc.arm9_fifo_enabled,
        }
    }

    fn write_ipcfifocnt(&mut self, cpu: NdsCpu, value: u16) {
        let enable = value & 0x8000 != 0;
        let recv_irq_enabled = value & 0x0400 != 0;
        let clear_recv = value & 0x0008 != 0;

        match cpu {
            NdsCpu::Arm7 => {
                self.ipc.arm7_fifo_enabled = enable;
                self.ipc.arm7_recv_irq_enabled = recv_irq_enabled;
                if clear_recv {
                    self.ipc.arm7_recv_fifo.clear();
                }
            }
            NdsCpu::Arm9 => {
                self.ipc.arm9_fifo_enabled = enable;
                self.ipc.arm9_recv_irq_enabled = recv_irq_enabled;
                if clear_recv {
                    self.ipc.arm9_recv_fifo.clear();
                }
            }
        }
    }

    fn read_ipcfifocnt(&self, cpu: NdsCpu) -> u16 {
        let recv_fifo = self.recv_fifo(cpu);
        let send_fifo = self.recv_fifo(match cpu {
            NdsCpu::Arm7 => NdsCpu::Arm9,
            NdsCpu::Arm9 => NdsCpu::Arm7,
        });

        let mut value = 0u16;
        if send_fifo.is_empty() {
            value |= 1 << 0;
        }
        if send_fifo.len() >= IPC_FIFO_CAPACITY {
            value |= 1 << 1;
        }
        if recv_fifo.is_empty() {
            value |= 1 << 8;
        }
        if recv_fifo.len() >= IPC_FIFO_CAPACITY {
            value |= 1 << 9;
        }
        if match cpu {
            NdsCpu::Arm7 => self.ipc.arm7_recv_irq_enabled,
            NdsCpu::Arm9 => self.ipc.arm9_recv_irq_enabled,
        } {
            value |= 1 << 10;
        }
        if self.fifo_enabled(cpu) {
            value |= 1 << 15;
        }

        value
    }

    fn send_ipc_word(&mut self, cpu: NdsCpu, value: u32) {
        let peer = match cpu {
            NdsCpu::Arm7 => NdsCpu::Arm9,
            NdsCpu::Arm9 => NdsCpu::Arm7,
        };

        if !self.fifo_enabled(cpu) || !self.fifo_enabled(peer) {
            return;
        }

        let recv_fifo = self.recv_fifo_mut(peer);
        if recv_fifo.len() < IPC_FIFO_CAPACITY {
            recv_fifo.push_back(value);
        }
    }

    pub fn arm7_dma_active_count(&self) -> usize {
        self.dma.channels.iter().filter(|channel| channel.active || channel.enabled).count()
    }

    pub fn arm9_dma_active_count(&self) -> usize {
        self.arm9_dma
            .channels
            .iter()
            .filter(|channel| channel.active || channel.enabled)
            .count()
    }

    pub fn arm7_ipc_depth(&self) -> usize {
        self.ipc.arm7_recv_fifo.len()
    }

    pub fn arm9_ipc_depth(&self) -> usize {
        self.ipc.arm9_recv_fifo.len()
    }

    fn process_dma(&mut self, cpu: NdsCpu) {
        for channel_index in 0..4usize {
            let channel = match cpu {
                NdsCpu::Arm7 => self.dma.channels[channel_index].clone(),
                NdsCpu::Arm9 => self.arm9_dma.channels[channel_index].clone(),
            };

            if !channel.active {
                continue;
            }

            let count = match channel.count as u32 {
                0 => 0x1_0000,
                value => value,
            };
            let word_size = if channel.word_size { 4u32 } else { 2u32 };
            let src_inc = match channel.src_control {
                0 => word_size as i32,
                1 => -(word_size as i32),
                2 => 0,
                _ => word_size as i32,
            };
            let dst_inc = match channel.dst_control {
                0 | 3 => word_size as i32,
                1 => -(word_size as i32),
                2 => 0,
                _ => word_size as i32,
            };

            let mut src = channel.src_addr;
            let mut dst = channel.dst_addr;

            for _ in 0..count {
                if channel.word_size {
                    let value = self.dma_read32(cpu, src);
                    self.dma_write32(cpu, dst, value);
                } else {
                    let value = self.dma_read16(cpu, src);
                    self.dma_write16(cpu, dst, value);
                }

                src = (src as i32).wrapping_add(src_inc) as u32;
                dst = (dst as i32).wrapping_add(dst_inc) as u32;
            }

            let active_channel = match cpu {
                NdsCpu::Arm7 => &mut self.dma.channels[channel_index],
                NdsCpu::Arm9 => &mut self.arm9_dma.channels[channel_index],
            };

            active_channel.src_addr = src;
            if active_channel.dst_control != 3 {
                active_channel.dst_addr = dst;
            }

            if active_channel.repeat && active_channel.timing != 0 {
                active_channel.active = false;
                if active_channel.dst_control == 3 {
                    active_channel.dst_addr = active_channel.dst_latch;
                }
            } else {
                active_channel.active = false;
                active_channel.enabled = false;
                active_channel.control &= !0x8000;
            }

            if active_channel.irq {
                let irq_bit = (crate::interrupts::gba::DMA0 as u32) << channel_index;
                match cpu {
                    NdsCpu::Arm7 => self.iflag |= irq_bit,
                    NdsCpu::Arm9 => self.arm9_iflag |= irq_bit,
                }
            }
        }
    }

    fn dma_read8(&self, cpu: NdsCpu, addr: u32) -> u8 {
        match cpu {
            NdsCpu::Arm7 => match addr {
                0x0000_0000..=0x0000_3FFF => Self::read_region(&self.bios7, 0x0000_0000, addr),
                MAIN_RAM_BASE..=0x023F_FFFF => Self::read_region(&self.memory.main_ram, MAIN_RAM_BASE, addr),
                SHARED_WRAM_BASE..=0x0300_7FFF => Self::read_region(&self.memory.shared_wram, SHARED_WRAM_BASE, addr),
                ARM7_WRAM_BASE..=0x0380_FFFF => Self::read_region(&self.memory.arm7_wram, ARM7_WRAM_BASE, addr),
                IO_BASE..=IO_END => self.read_io_byte(addr),
                PALETTE_BASE..=0x0500_0FFF => Self::read_region(&self.memory.palette, PALETTE_BASE, addr),
                VRAM_BASE..=0x060A_3FFF => Self::read_region(&self.memory.vram, VRAM_BASE, addr),
                OAM_BASE..=0x0700_03FF => Self::read_region(&self.memory.oam, OAM_BASE, addr),
                CART_BASE..=0x09FF_FFFF => {
                    let offset = (addr - CART_BASE) as usize;
                    self.cartridge_rom.get(offset).copied().unwrap_or(0xFF)
                }
                _ => 0,
            },
            NdsCpu::Arm9 => match addr {
                MAIN_RAM_BASE..=0x023F_FFFF => Self::read_region(&self.memory.main_ram, MAIN_RAM_BASE, addr),
                SHARED_WRAM_BASE..=0x0300_7FFF => Self::read_region(&self.memory.shared_wram, SHARED_WRAM_BASE, addr),
                IO_BASE..=IO_END => self.read_arm9_io_byte(addr),
                PALETTE_BASE..=0x0500_0FFF => Self::read_region(&self.memory.palette, PALETTE_BASE, addr),
                VRAM_BASE..=0x060A_3FFF => Self::read_region(&self.memory.vram, VRAM_BASE, addr),
                OAM_BASE..=0x0700_03FF => Self::read_region(&self.memory.oam, OAM_BASE, addr),
                CART_BASE..=0x09FF_FFFF => {
                    let offset = (addr - CART_BASE) as usize;
                    self.cartridge_rom.get(offset).copied().unwrap_or(0xFF)
                }
                ARM9_BIOS_BASE..=0xFFFF_7FFF => Self::read_region(&self.bios9, ARM9_BIOS_BASE, addr),
                _ => 0,
            },
        }
    }

    fn dma_read16(&self, cpu: NdsCpu, addr: u32) -> u16 {
        u16::from_le_bytes([self.dma_read8(cpu, addr), self.dma_read8(cpu, addr.wrapping_add(1))])
    }

    fn dma_read32(&self, cpu: NdsCpu, addr: u32) -> u32 {
        u32::from_le_bytes([
            self.dma_read8(cpu, addr),
            self.dma_read8(cpu, addr.wrapping_add(1)),
            self.dma_read8(cpu, addr.wrapping_add(2)),
            self.dma_read8(cpu, addr.wrapping_add(3)),
        ])
    }

    fn dma_write8(&mut self, cpu: NdsCpu, addr: u32, value: u8) {
        match cpu {
            NdsCpu::Arm7 => match addr {
                MAIN_RAM_BASE..=0x023F_FFFF => Self::write_region(&mut self.memory.main_ram, MAIN_RAM_BASE, addr, value),
                SHARED_WRAM_BASE..=0x0300_7FFF => Self::write_region(&mut self.memory.shared_wram, SHARED_WRAM_BASE, addr, value),
                ARM7_WRAM_BASE..=0x0380_FFFF => Self::write_region(&mut self.memory.arm7_wram, ARM7_WRAM_BASE, addr, value),
                IO_BASE..=IO_END => self.write_io_byte(addr, value),
                PALETTE_BASE..=0x0500_0FFF => Self::write_region(&mut self.memory.palette, PALETTE_BASE, addr, value),
                VRAM_BASE..=0x060A_3FFF => Self::write_region(&mut self.memory.vram, VRAM_BASE, addr, value),
                OAM_BASE..=0x0700_03FF => Self::write_region(&mut self.memory.oam, OAM_BASE, addr, value),
                _ => {}
            },
            NdsCpu::Arm9 => match addr {
                MAIN_RAM_BASE..=0x023F_FFFF => Self::write_region(&mut self.memory.main_ram, MAIN_RAM_BASE, addr, value),
                SHARED_WRAM_BASE..=0x0300_7FFF => Self::write_region(&mut self.memory.shared_wram, SHARED_WRAM_BASE, addr, value),
                IO_BASE..=IO_END => self.write_arm9_io_byte(addr, value),
                PALETTE_BASE..=0x0500_0FFF => Self::write_region(&mut self.memory.palette, PALETTE_BASE, addr, value),
                VRAM_BASE..=0x060A_3FFF => Self::write_region(&mut self.memory.vram, VRAM_BASE, addr, value),
                OAM_BASE..=0x0700_03FF => Self::write_region(&mut self.memory.oam, OAM_BASE, addr, value),
                _ => {}
            },
        }
    }

    fn dma_write16(&mut self, cpu: NdsCpu, addr: u32, value: u16) {
        let bytes = value.to_le_bytes();
        self.dma_write8(cpu, addr, bytes[0]);
        self.dma_write8(cpu, addr.wrapping_add(1), bytes[1]);
    }

    fn dma_write32(&mut self, cpu: NdsCpu, addr: u32, value: u32) {
        let bytes = value.to_le_bytes();
        self.dma_write8(cpu, addr, bytes[0]);
        self.dma_write8(cpu, addr.wrapping_add(1), bytes[1]);
        self.dma_write8(cpu, addr.wrapping_add(2), bytes[2]);
        self.dma_write8(cpu, addr.wrapping_add(3), bytes[3]);
    }

    fn read_io_byte(&self, addr: u32) -> u8 {
        match addr {
            REG_DISPCNT..=REG_DISPCNT_END => self.ppu_main.read_byte(addr - PPU_MAIN_BASE),
            REG_DISPSTAT => self.read_dispstat(NdsCpu::Arm7) as u8,
            REG_DISPSTAT_HI => (self.read_dispstat(NdsCpu::Arm7) >> 8) as u8,
            REG_VCOUNT => self.video.vcount as u8,
            REG_VCOUNT_HI => (self.video.vcount >> 8) as u8,
            _ if self.read_ppu_byte(addr).is_some() => self.read_ppu_byte(addr).unwrap_or(0),
            0x0400_00B0..=0x0400_00DF => self.dma.read(addr - IO_BASE),
            0x0400_0100..=0x0400_010F => self.timers.read(addr - IO_BASE),
            REG_KEYINPUT => self.input.read_keyinput() as u8,
            REG_KEYINPUT_HI => (self.input.read_keyinput() >> 8) as u8,
            REG_EXTKEYIN => self.input.read_extkeyin() as u8,
            REG_EXTKEYIN_HI => (self.input.read_extkeyin() >> 8) as u8,
            REG_IPCSYNC => self.read_ipcsync(NdsCpu::Arm7) as u8,
            REG_IPCSYNC_HI => (self.read_ipcsync(NdsCpu::Arm7) >> 8) as u8,
            REG_IPCFIFOCNT => self.read_ipcfifocnt(NdsCpu::Arm7) as u8,
            REG_IPCFIFOCNT_HI => (self.read_ipcfifocnt(NdsCpu::Arm7) >> 8) as u8,
            0x0400_0188..=0x0400_018B => {
                let shift = ((addr - REG_IPCFIFOSEND) * 8) as u32;
                (self.peek_ipc_fifo(NdsCpu::Arm7) >> shift) as u8
            }
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
            REG_DISPCNT..=REG_DISPCNT_END => self.ppu_main.read_byte(addr - PPU_MAIN_BASE),
            REG_DISPSTAT => self.read_dispstat(NdsCpu::Arm9) as u8,
            REG_DISPSTAT_HI => (self.read_dispstat(NdsCpu::Arm9) >> 8) as u8,
            REG_VCOUNT => self.video.vcount as u8,
            REG_VCOUNT_HI => (self.video.vcount >> 8) as u8,
            _ if self.read_ppu_byte(addr).is_some() => self.read_ppu_byte(addr).unwrap_or(0),
            0x0400_00B0..=0x0400_00DF => self.arm9_dma.read(addr - IO_BASE),
            0x0400_0100..=0x0400_010F => self.arm9_timers.read(addr - IO_BASE),
            REG_IPCSYNC => self.read_ipcsync(NdsCpu::Arm9) as u8,
            REG_IPCSYNC_HI => (self.read_ipcsync(NdsCpu::Arm9) >> 8) as u8,
            REG_IPCFIFOCNT => self.read_ipcfifocnt(NdsCpu::Arm9) as u8,
            REG_IPCFIFOCNT_HI => (self.read_ipcfifocnt(NdsCpu::Arm9) >> 8) as u8,
            0x0400_0188..=0x0400_018B => {
                let shift = ((addr - REG_IPCFIFOSEND) * 8) as u32;
                (self.peek_ipc_fifo(NdsCpu::Arm9) >> shift) as u8
            }
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
            REG_DISPCNT..=REG_DISPCNT_END => {
                self.ppu_main.write_byte(addr - PPU_MAIN_BASE, value);
            }
            REG_DISPSTAT => self.write_dispstat_byte(NdsCpu::Arm7, 0, value),
            REG_DISPSTAT_HI => self.write_dispstat_byte(NdsCpu::Arm7, 1, value),
            _ if self.write_ppu_byte(addr, value) => {}
            0x0400_00B0..=0x0400_00DF => self.dma.write(addr - IO_BASE, value),
            0x0400_0100..=0x0400_010F => self.timers.write(addr - IO_BASE, value),
            REG_IPCSYNC => self.write_ipcsync(NdsCpu::Arm7, value as u16),
            REG_IPCSYNC_HI => self.write_ipcsync(NdsCpu::Arm7, (value as u16) << 8),
            REG_IPCFIFOCNT => self.write_ipcfifocnt(NdsCpu::Arm7, value as u16),
            REG_IPCFIFOCNT_HI => self.write_ipcfifocnt(NdsCpu::Arm7, (value as u16) << 8),
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
            REG_DISPCNT..=REG_DISPCNT_END => {
                self.ppu_main.write_byte(addr - PPU_MAIN_BASE, value);
            }
            REG_DISPSTAT => self.write_dispstat_byte(NdsCpu::Arm9, 0, value),
            REG_DISPSTAT_HI => self.write_dispstat_byte(NdsCpu::Arm9, 1, value),
            _ if self.write_ppu_byte(addr, value) => {}
            0x0400_00B0..=0x0400_00DF => self.arm9_dma.write(addr - IO_BASE, value),
            0x0400_0100..=0x0400_010F => self.arm9_timers.write(addr - IO_BASE, value),
            REG_IPCSYNC => self.write_ipcsync(NdsCpu::Arm9, value as u16),
            REG_IPCSYNC_HI => self.write_ipcsync(NdsCpu::Arm9, (value as u16) << 8),
            REG_IPCFIFOCNT => self.write_ipcfifocnt(NdsCpu::Arm9, value as u16),
            REG_IPCFIFOCNT_HI => self.write_ipcfifocnt(NdsCpu::Arm9, (value as u16) << 8),
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
            IO_BASE..=IO_END => self.bus.read_arm9_io_byte(addr),
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
        if (REG_IPCFIFOSEND..=REG_IPCFIFOSEND + 3).contains(&addr) {
            return self.bus.pop_ipc_fifo(NdsCpu::Arm9);
        }

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
            IO_BASE..=IO_END => self.bus.write_arm9_io_byte(addr, val),
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
        if addr == REG_IPCFIFOSEND {
            self.bus.send_ipc_word(NdsCpu::Arm9, val);
            return;
        }

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
            IO_BASE..=IO_END => self.read_io_byte(addr),
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
        if (REG_IPCFIFOSEND..=REG_IPCFIFOSEND + 3).contains(&addr) {
            return self.pop_ipc_fifo(NdsCpu::Arm7);
        }

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
            IO_BASE..=IO_END => self.write_io_byte(addr, val),
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
        if addr == REG_IPCFIFOSEND {
            self.send_ipc_word(NdsCpu::Arm7, val);
            return;
        }

        let bytes = val.to_le_bytes();
        self.write8(addr, bytes[0]);
        self.write8(addr.wrapping_add(1), bytes[1]);
        self.write8(addr.wrapping_add(2), bytes[2]);
        self.write8(addr.wrapping_add(3), bytes[3]);
    }
}

#[cfg(test)]
mod tests {
    use super::{NdsBus, REG_DISPCNT, REG_DISPSTAT, REG_EXTKEYIN, REG_IPCFIFOCNT, REG_IPCFIFOSEND, REG_KEYINPUT, REG_VCOUNT};
    use crate::cpu::arm7tdmi::{Arm7Bus, Arm7Tdmi};
    use crate::emulator::nds::NdsRomHeader;
    use crate::input::NdsKey;

    const REG_SUB_DISPCNT: u32 = 0x0400_1000;
    const REG_SUB_BG0CNT: u32 = 0x0400_1008;
    const REG_SUB_MASTER_BRIGHT: u32 = 0x0400_106C;
    const REG_DMA0SAD: u32 = 0x0400_00B0;
    const REG_DMA0DAD: u32 = 0x0400_00B4;
    const REG_DMA0CNT_L: u32 = 0x0400_00B8;
    const REG_DMA0CNT_H: u32 = 0x0400_00BA;
    const REG_TM0CNT_L: u32 = 0x0400_0100;
    const REG_TM0CNT_H: u32 = 0x0400_0102;

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

    #[test]
    fn nds_bus_timer_irq_sets_arm7_interrupt_flags() {
        let rom = build_test_rom();
        let header = NdsRomHeader::parse(&rom).expect("test ROM header should parse");
        let mut bus = NdsBus::new(rom, &header).expect("bus should initialize");

        bus.write32(super::REG_IE, 1 << 3);
        bus.write32(super::REG_IME, 1);
        bus.write16(REG_TM0CNT_L, 0xFFFE);
        bus.write16(REG_TM0CNT_H, 0x00C0);
        bus.tick(2);

        assert_ne!(bus.iflag & (1 << 3), 0);
        assert!(bus.check_irq());
    }

    #[test]
    fn nds_bus_timer_irq_sets_arm9_interrupt_flags() {
        let rom = build_test_rom();
        let header = NdsRomHeader::parse(&rom).expect("test ROM header should parse");
        let mut bus = NdsBus::new(rom, &header).expect("bus should initialize");

        {
            let mut arm9_bus = bus.arm9_view();
            arm9_bus.write32(super::REG_IE, 1 << 3);
            arm9_bus.write32(super::REG_IME, 1);
            arm9_bus.write16(REG_TM0CNT_L, 0xFFFE);
            arm9_bus.write16(REG_TM0CNT_H, 0x00C0);
        }

        bus.tick_arm9(2);

        assert_ne!(bus.arm9_iflag & (1 << 3), 0);
        assert!(bus.arm9_check_irq());
    }

    #[test]
    fn nds_bus_ipc_fifo_moves_words_between_cpus() {
        let rom = build_test_rom();
        let header = NdsRomHeader::parse(&rom).expect("test ROM header should parse");
        let mut bus = NdsBus::new(rom, &header).expect("bus should initialize");

        bus.write16(REG_IPCFIFOCNT, 0x8000);
        {
            let mut arm9_bus = bus.arm9_view();
            arm9_bus.write16(REG_IPCFIFOCNT, 0x8000);
        }

        bus.write32(REG_IPCFIFOSEND, 0x1234_5678);

        {
            let mut arm9_bus = bus.arm9_view();
            assert_eq!(arm9_bus.read32(REG_IPCFIFOSEND), 0x1234_5678);
            assert_ne!(arm9_bus.read16(REG_IPCFIFOCNT) & (1 << 8), 0);
        }
    }

    #[test]
    fn nds_bus_arm9_dma_copies_words_and_raises_irq() {
        let rom = build_test_rom();
        let header = NdsRomHeader::parse(&rom).expect("test ROM header should parse");
        let mut bus = NdsBus::new(rom, &header).expect("bus should initialize");

        bus.memory.main_ram[0x100..0x104].copy_from_slice(&0xAABB_CCDDu32.to_le_bytes());

        {
            let mut arm9_bus = bus.arm9_view();
            arm9_bus.write32(REG_DMA0SAD, 0x0200_0100);
            arm9_bus.write32(REG_DMA0DAD, 0x0200_0200);
            arm9_bus.write16(REG_DMA0CNT_L, 1);
            arm9_bus.write16(REG_DMA0CNT_H, 0xC400);
        }

        bus.tick_arm9(1);

        assert_eq!(u32::from_le_bytes(bus.memory.main_ram[0x200..0x204].try_into().unwrap()), 0xAABB_CCDD);
        assert_ne!(bus.arm9_iflag & (crate::interrupts::gba::DMA0 as u32), 0);
        assert_eq!(bus.arm9_dma_active_count(), 0);
    }

    #[test]
    fn nds_bus_main_ppu_registers_round_trip() {
        let rom = build_test_rom();
        let header = NdsRomHeader::parse(&rom).expect("test ROM header should parse");
        let mut bus = NdsBus::new(rom, &header).expect("bus should initialize");

        bus.write32(REG_DISPCNT, 0x4433_2211);
        bus.write16(REG_DISPCNT + 0x08, 0x80C1);
        bus.write16(REG_DISPCNT + 0x6C, 0x001F);

        assert_eq!(bus.read32(REG_DISPCNT), 0x4433_2211);
        assert_eq!(bus.ppu_main.bgcnt[0], 0x80C1);
        assert_eq!(bus.ppu_main.master_bright, 0x001F);
    }

    #[test]
    fn nds_bus_sub_ppu_registers_round_trip_via_arm9_view() {
        let rom = build_test_rom();
        let header = NdsRomHeader::parse(&rom).expect("test ROM header should parse");
        let mut bus = NdsBus::new(rom, &header).expect("bus should initialize");

        {
            let mut arm9_bus = bus.arm9_view();
            arm9_bus.write32(REG_SUB_DISPCNT, 0x8877_6655);
            arm9_bus.write16(REG_SUB_BG0CNT, 0x1234);
            arm9_bus.write16(REG_SUB_MASTER_BRIGHT, 0x000F);

            assert_eq!(arm9_bus.read32(REG_SUB_DISPCNT), 0x8877_6655);
            assert_eq!(arm9_bus.read16(REG_SUB_BG0CNT), 0x1234);
        }

        assert_eq!(bus.ppu_sub.dispcnt, 0x8877_6655);
        assert_eq!(bus.ppu_sub.bgcnt[0], 0x1234);
        assert_eq!(bus.ppu_sub.master_bright, 0x000F);
    }

    #[test]
    fn nds_bus_video_timing_updates_dispstat_and_vcount() {
        let rom = build_test_rom();
        let header = NdsRomHeader::parse(&rom).expect("test ROM header should parse");
        let mut bus = NdsBus::new(rom, &header).expect("bus should initialize");

        bus.tick_arm9(super::HBLANK_START_CYCLES);
        assert_ne!(bus.read16(REG_DISPSTAT) & 0x0002, 0);

        bus.tick_arm9(super::SCANLINE_CYCLES - super::HBLANK_START_CYCLES);
        assert_eq!(bus.read16(REG_DISPSTAT) & 0x0002, 0);
        assert_eq!(bus.read16(REG_VCOUNT), 1);
    }

    #[test]
    fn nds_bus_video_vblank_and_vcounter_irq_reach_both_cpus() {
        let rom = build_test_rom();
        let header = NdsRomHeader::parse(&rom).expect("test ROM header should parse");
        let mut bus = NdsBus::new(rom, &header).expect("bus should initialize");

        bus.write16(REG_DISPSTAT, 0x0020);
        bus.write16(REG_DISPSTAT + 1, 1);
        {
            let mut arm9_bus = bus.arm9_view();
            arm9_bus.write16(REG_DISPSTAT, 0x0028);
            arm9_bus.write16(REG_DISPSTAT + 1, super::VISIBLE_SCANLINES as u16);
        }

        bus.tick_arm9(super::SCANLINE_CYCLES);
        assert_ne!(bus.iflag & (crate::interrupts::gba::VCOUNTER as u32), 0);

        bus.tick_arm9((super::VISIBLE_SCANLINES as u32 - 1) * super::SCANLINE_CYCLES);
        assert_ne!(bus.arm9_iflag & (crate::interrupts::gba::VBLANK as u32), 0);
        assert_ne!(bus.read_dispstat_value() & 0x0001, 0);
        assert_ne!(bus.read_arm9_dispstat_value() & 0x0001, 0);
    }
}