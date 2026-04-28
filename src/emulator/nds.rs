use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};

use crate::cpu::arm7tdmi::Arm7Tdmi;
use crate::memory::nds_bus::NdsBus;
use crate::{NDS_HEIGHT, NDS_WIDTH};

const NDS_SCREEN_HEIGHT: usize = NDS_HEIGHT / 2;
const NDS_HEADER_SIZE: usize = 0x170;
const NDS_ARM9_CYCLES_PER_FRAME: u32 = 1_117_132;
const NDS_ARM7_CYCLES_PER_FRAME: u32 = 558_566;
const NDS_SCREEN_PIXELS: usize = NDS_WIDTH * NDS_SCREEN_HEIGHT;
const NDS_SCREEN_BYTES_16BPP: usize = NDS_SCREEN_PIXELS * 2;
const MAIN_SCREEN_VRAM_OFFSET: usize = 0x00000;
const SUB_SCREEN_VRAM_OFFSET: usize = 0x20000;
const MAIN_BG_PALETTE_OFFSET: usize = 0x000;
const MAIN_OBJ_PALETTE_OFFSET: usize = 0x200;
const SUB_BG_PALETTE_OFFSET: usize = 0x400;
const SUB_OBJ_PALETTE_OFFSET: usize = 0x600;
const ENABLE_3D_BIT: u32 = 1 << 3;
const DISP3DCNT_CLEAR_BMP_BIT: u16 = 1 << 14;
const BG_ENABLE_BITS: [u32; 4] = [1 << 8, 1 << 9, 1 << 10, 1 << 11];
const OBJ_ENABLE_BIT: u32 = 1 << 12;
const OBJ_1D_MAPPING_BIT: u32 = 1 << 4;
const OAM_SCREEN_BYTES: usize = 0x400;

#[derive(Clone, Copy)]
enum NdsBgKind {
    ThreeD,
    Text,
    Affine,
    Bitmap8,
    Bitmap16,
}

#[derive(Clone, Copy)]
struct NdsBgLayer {
    bg: usize,
    kind: NdsBgKind,
}

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
    pub main_3d_framebuffer: Vec<u16>,
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
            main_3d_framebuffer: vec![0; NDS_SCREEN_PIXELS],
            framebuffer: vec![0xFF11161C; NDS_WIDTH * NDS_HEIGHT],
            total_frames: 0,
        };
        emu.refresh_framebuffer();
        Ok(emu)
    }

    fn refresh_framebuffer(&mut self) {
        self.refresh_placeholder_framebuffer();
        let main_bg_vram = self.bus.mapped_main_bg_vram();
        let sub_bg_vram = self.bus.mapped_sub_bg_vram();
        let main_obj_vram = self.bus.mapped_main_obj_vram();
        let sub_obj_vram = self.bus.mapped_sub_obj_vram();
        let main_3d_framebuffer = self.main_3d_framebuffer.clone();
        let mut main_3d_surface = main_3d_framebuffer;
        apply_3d_clear_plane(
            &mut main_3d_surface,
            self.bus.ppu_main.clear_color,
            self.bus.ppu_main.disp3dcnt,
        );
        let main_oam = self.bus.memory.oam[..OAM_SCREEN_BYTES].to_vec();
        let sub_oam = self.bus.memory.oam[OAM_SCREEN_BYTES..OAM_SCREEN_BYTES * 2].to_vec();
        let main_state = (
            self.bus.ppu_main.dispcnt,
            self.bus.ppu_main.bgcnt,
            self.bus.ppu_main.bghofs,
            self.bus.ppu_main.bgvofs,
            self.bus.ppu_main.bg_pa,
            self.bus.ppu_main.bg_pb,
            self.bus.ppu_main.bg_pc,
            self.bus.ppu_main.bg_pd,
            self.bus.ppu_main.bg_ref_x,
            self.bus.ppu_main.bg_ref_y,
            self.bus.ppu_main.master_bright,
        );
        let mut main_priority_buffer = vec![4u8; NDS_SCREEN_PIXELS];
        if !self.render_bg_layers(
            0,
            &main_bg_vram,
            MAIN_BG_PALETTE_OFFSET,
            Some(&main_3d_surface),
            main_state.0,
            main_state.1,
            main_state.2,
            main_state.3,
            main_state.4,
            main_state.5,
            main_state.6,
            main_state.7,
            main_state.8,
            main_state.9,
            main_state.10,
            &mut main_priority_buffer,
        ) {
            self.render_screen_preview(0, MAIN_SCREEN_VRAM_OFFSET, main_state.0, main_state.10);
        }
        self.render_obj_layers(
            0,
            &main_obj_vram,
            &main_oam,
            MAIN_OBJ_PALETTE_OFFSET,
            main_state.0,
            main_state.10,
            &mut main_priority_buffer,
        );

        let sub_state = (
            self.bus.ppu_sub.dispcnt,
            self.bus.ppu_sub.bgcnt,
            self.bus.ppu_sub.bghofs,
            self.bus.ppu_sub.bgvofs,
            self.bus.ppu_sub.bg_pa,
            self.bus.ppu_sub.bg_pb,
            self.bus.ppu_sub.bg_pc,
            self.bus.ppu_sub.bg_pd,
            self.bus.ppu_sub.bg_ref_x,
            self.bus.ppu_sub.bg_ref_y,
            self.bus.ppu_sub.master_bright,
        );
        let mut sub_priority_buffer = vec![4u8; NDS_SCREEN_PIXELS];
        if !self.render_bg_layers(
            NDS_SCREEN_HEIGHT,
            &sub_bg_vram,
            SUB_BG_PALETTE_OFFSET,
            None,
            sub_state.0,
            sub_state.1,
            sub_state.2,
            sub_state.3,
            sub_state.4,
            sub_state.5,
            sub_state.6,
            sub_state.7,
            sub_state.8,
            sub_state.9,
            sub_state.10,
            &mut sub_priority_buffer,
        ) {
            self.render_screen_preview(NDS_SCREEN_HEIGHT, SUB_SCREEN_VRAM_OFFSET, sub_state.0, sub_state.10);
        }
        self.render_obj_layers(
            NDS_SCREEN_HEIGHT,
            &sub_obj_vram,
            &sub_oam,
            SUB_OBJ_PALETTE_OFFSET,
            sub_state.0,
            sub_state.10,
            &mut sub_priority_buffer,
        );
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

    fn render_screen_preview(&mut self, screen_y: usize, vram_offset: usize, dispcnt: u32, master_bright: u16) {
        if !self.screen_has_preview_data(vram_offset, dispcnt, master_bright) {
            return;
        }

        let vram = &self.bus.memory.vram;
        let available = vram.len().saturating_sub(vram_offset);
        let bytes_to_render = available.min(NDS_SCREEN_BYTES_16BPP);
        let pixels_to_render = bytes_to_render / 2;

        for pixel in 0..pixels_to_render {
            let src = vram_offset + pixel * 2;
            let color = u16::from_le_bytes([vram[src], vram[src + 1]]);
            let lit = apply_master_brightness(color, master_bright);
            let x = pixel % NDS_WIDTH;
            let y = pixel / NDS_WIDTH;
            let dst = (screen_y + y) * NDS_WIDTH + x;
            self.framebuffer[dst] = rgb555_to_argb(lit);
        }
    }

    fn render_bg_layers(
        &mut self,
        screen_y: usize,
        vram: &[u8],
        palette_offset: usize,
        main_3d_framebuffer: Option<&[u16]>,
        dispcnt: u32,
        bgcnt: [u16; 4],
        bghofs: [u16; 4],
        bgvofs: [u16; 4],
        bg_pa: [i16; 2],
        bg_pb: [i16; 2],
        bg_pc: [i16; 2],
        bg_pd: [i16; 2],
        bg_ref_x: [i32; 2],
        bg_ref_y: [i32; 2],
        master_bright: u16,
        priority_buffer: &mut [u8],
    ) -> bool {
        if !supports_2d_layer_render(dispcnt) {
            return false;
        }

        let active_layers = active_2d_bgs(dispcnt, bgcnt, main_3d_framebuffer.is_some());
        if active_layers.is_empty() {
            return false;
        }

        let palette = &self.bus.memory.palette;
        if vram.is_empty() || palette_offset + 1 >= palette.len() {
            return false;
        }

        let framebuffer = &mut self.framebuffer;
        priority_buffer.fill(4);
        let backdrop = rgb555_to_argb(apply_master_brightness(read_color_16(palette, palette_offset), master_bright));
        for y in 0..NDS_SCREEN_HEIGHT {
            let row_start = (screen_y + y) * NDS_WIDTH;
            framebuffer[row_start..row_start + NDS_WIDTH].fill(backdrop);
        }

        for screen_line in 0..NDS_SCREEN_HEIGHT {
            let row_start = (screen_y + screen_line) * NDS_WIDTH;

            for px in 0..NDS_WIDTH {
                for layer in &active_layers {
                    let raw_color = match layer.kind {
                        NdsBgKind::ThreeD => {
                            let Some(main_3d_framebuffer) = main_3d_framebuffer else {
                                continue;
                            };
                            sample_3d_pixel(main_3d_framebuffer, px, screen_line)
                        }
                        NdsBgKind::Text => sample_text_bg_pixel(
                            vram,
                            palette,
                            palette_offset,
                            bgcnt[layer.bg],
                            bghofs[layer.bg],
                            bgvofs[layer.bg],
                            px,
                            screen_line,
                        ),
                        NdsBgKind::Bitmap8 | NdsBgKind::Bitmap16 => {
                            let affine_index = layer.bg - 2;
                            sample_bitmap_bg_pixel(
                                vram,
                                palette,
                                palette_offset,
                                bgcnt[layer.bg],
                                bg_pa[affine_index],
                                bg_pb[affine_index],
                                bg_pc[affine_index],
                                bg_pd[affine_index],
                                bg_ref_x[affine_index],
                                bg_ref_y[affine_index],
                                px,
                                screen_line,
                                matches!(layer.kind, NdsBgKind::Bitmap16),
                            )
                        }
                        NdsBgKind::Affine => {
                            let affine_index = layer.bg - 2;
                            sample_affine_bg_pixel(
                                vram,
                                palette,
                                palette_offset,
                                bgcnt[layer.bg],
                                bg_pa[affine_index],
                                bg_pb[affine_index],
                                bg_pc[affine_index],
                                bg_pd[affine_index],
                                bg_ref_x[affine_index],
                                bg_ref_y[affine_index],
                                px,
                                screen_line,
                            )
                        }
                    };

                    if let Some(raw_color) = raw_color {
                        framebuffer[row_start + px] = rgb555_to_argb(apply_master_brightness(raw_color, master_bright));
                        priority_buffer[screen_line * NDS_WIDTH + px] = (bgcnt[layer.bg] & 0x3) as u8;
                        break;
                    }
                }
            }
        }

        true
    }

    fn render_obj_layers(
        &mut self,
        screen_y: usize,
        obj_vram: &[u8],
        oam: &[u8],
        palette_offset: usize,
        dispcnt: u32,
        master_bright: u16,
        priority_buffer: &mut [u8],
    ) {
        if dispcnt & OBJ_ENABLE_BIT == 0 || ((dispcnt >> 16) & 0x3) > 1 {
            return;
        }

        let palette = &self.bus.memory.palette;
        let framebuffer = &mut self.framebuffer;
        let obj_mapping_1d = dispcnt & OBJ_1D_MAPPING_BIT != 0;

        for i in (0..128usize).rev() {
            let base = i * 8;
            if base + 5 >= oam.len() {
                break;
            }

            let attr0 = u16::from_le_bytes([oam[base], oam[base + 1]]);
            let attr1 = u16::from_le_bytes([oam[base + 2], oam[base + 3]]);
            let attr2 = u16::from_le_bytes([oam[base + 4], oam[base + 5]]);

            let affine = attr0 & 0x0100 != 0;
            let double_size_or_disable = attr0 & 0x0200 != 0;
            let obj_mode = (attr0 >> 10) & 0x3;
            if affine || (!affine && double_size_or_disable) || obj_mode == 2 {
                continue;
            }

            let (width, height) = obj_size((attr0 >> 14) & 0x3, (attr1 >> 14) & 0x3);
            let y = (attr0 & 0xFF) as i32;
            let y = if y >= 192 { y - 256 } else { y };
            let x = (attr1 & 0x1FF) as i32;
            let x = if x >= 256 { x - 512 } else { x };
            let tile_num = (attr2 & 0x03FF) as usize;
            let priority = ((attr2 >> 10) & 0x3) as u8;
            let palette_num = ((attr2 >> 12) & 0xF) as usize;
            let color_256 = attr0 & 0x2000 != 0;
            let h_flip = attr1 & 0x1000 != 0;
            let v_flip = attr1 & 0x2000 != 0;

            for sprite_y in 0..height {
                let screen_line = y + sprite_y as i32;
                if !(0..NDS_SCREEN_HEIGHT as i32).contains(&screen_line) {
                    continue;
                }

                let src_y = if v_flip { height - 1 - sprite_y } else { sprite_y };
                let dst_row_start = (screen_y + screen_line as usize) * NDS_WIDTH;
                let priority_row_start = screen_line as usize * NDS_WIDTH;

                for sprite_x in 0..width {
                    let screen_x = x + sprite_x as i32;
                    if !(0..NDS_WIDTH as i32).contains(&screen_x) {
                        continue;
                    }

                    let src_x = if h_flip { width - 1 - sprite_x } else { sprite_x };
                    let Some(color) = sample_obj_pixel(
                        obj_vram,
                        palette,
                        palette_offset,
                        tile_num,
                        palette_num,
                        color_256,
                        obj_mapping_1d,
                        width,
                        src_x,
                        src_y,
                    ) else {
                        continue;
                    };

                    let priority_index = priority_row_start + screen_x as usize;
                    if priority > priority_buffer[priority_index] {
                        continue;
                    }

                    framebuffer[dst_row_start + screen_x as usize] =
                        rgb555_to_argb(apply_master_brightness(color, master_bright));
                    priority_buffer[priority_index] = priority;
                }
            }
        }
    }

    fn screen_has_preview_data(&self, vram_offset: usize, dispcnt: u32, master_bright: u16) -> bool {
        if dispcnt != 0 || master_bright != 0 {
            return true;
        }

        let vram = &self.bus.memory.vram;
        if vram_offset >= vram.len() {
            return false;
        }

        let end = (vram_offset + NDS_SCREEN_BYTES_16BPP).min(vram.len());
        vram[vram_offset..end].iter().any(|&byte| byte != 0)
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
        self.refresh_framebuffer();
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

    pub fn video_warning(&self) -> Option<String> {
        let mut warnings = Vec::new();
        if let Some(main) = describe_video_warning("Main", true, self.bus.ppu_main.dispcnt, self.bus.ppu_main.disp3dcnt)
        {
            warnings.push(main);
        }
        if let Some(sub) = describe_video_warning("Sub", false, self.bus.ppu_sub.dispcnt, self.bus.ppu_sub.disp3dcnt)
        {
            warnings.push(sub);
        }

        if warnings.is_empty() {
            None
        } else {
            Some(format!("DS video is partial: {}", warnings.join(" | ")))
        }
    }
}

fn argb(r: u8, g: u8, b: u8) -> u32 {
    0xFF00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

fn apply_master_brightness(color: u16, master_bright: u16) -> u16 {
    let amount = (master_bright & 0x001F).min(16);
    match (master_bright >> 14) & 0x3 {
        1 => adjust_rgb555(color, amount, true),
        2 => adjust_rgb555(color, amount, false),
        _ => color,
    }
}

fn adjust_rgb555(color: u16, amount: u16, brighten: bool) -> u16 {
    let adjust = |channel: u16| {
        if brighten {
            channel + (((31 - channel) * amount) + 7) / 16
        } else {
            channel.saturating_sub(((channel * amount) + 7) / 16)
        }
    };

    let r = adjust(color & 0x1F);
    let g = adjust((color >> 5) & 0x1F);
    let b = adjust((color >> 10) & 0x1F);
    r | (g << 5) | (b << 10)
}

fn rgb555_to_argb(color: u16) -> u32 {
    let r = ((color & 0x1F) as u32) * 255 / 31;
    let g = (((color >> 5) & 0x1F) as u32) * 255 / 31;
    let b = (((color >> 10) & 0x1F) as u32) * 255 / 31;
    0xFF00_0000 | (r << 16) | (g << 8) | b
}

fn read_color_16(region: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([region[offset], region[offset + 1]])
}

fn supports_2d_layer_render(dispcnt: u32) -> bool {
    let display_mode = (dispcnt >> 16) & 0x3;
    let bg_mode = dispcnt & 0x7;
    display_mode <= 1 && bg_mode <= 5
}

fn describe_video_warning(name: &str, main_engine: bool, dispcnt: u32, disp3dcnt: u16) -> Option<String> {
    let display_mode = (dispcnt >> 16) & 0x3;
    let bg_mode = dispcnt & 0x7;
    let mut reasons = Vec::new();

    if display_mode > 1 {
        reasons.push(format!("display mode {}", display_mode));
    }
    if bg_mode > 5 {
        reasons.push(format!("BG mode {}", bg_mode));
    }
    if main_engine && dispcnt & ENABLE_3D_BIT != 0 {
        reasons.push("3D rasterizer".to_string());
    } else if disp3dcnt != 0 {
        reasons.push("3D engine state".to_string());
    }

    if reasons.is_empty() {
        None
    } else {
        Some(format!("{} engine uses {}", name, reasons.join(", ")))
    }
}

fn active_2d_bgs(dispcnt: u32, bgcnt: [u16; 4], main_3d_framebuffer: bool) -> Vec<NdsBgLayer> {
    let mut active = Vec::new();
    let bg_mode = (dispcnt & 0x7) as u8;
    let main_3d = main_3d_framebuffer && dispcnt & ENABLE_3D_BIT != 0;
    for bg in 0..4usize {
        let enabled = dispcnt & BG_ENABLE_BITS[bg] != 0;
        let kind = bg_kind(bg_mode, bg, bgcnt[bg], main_3d);
        if enabled {
            if let Some(kind) = kind {
                active.push(NdsBgLayer { bg, kind });
            }
        }
    }

    active.sort_by_key(|layer| ((bgcnt[layer.bg] & 0x3) as usize, layer.bg));
    active
}

fn bg_kind(bg_mode: u8, bg: usize, bgcnt: u16, main_3d: bool) -> Option<NdsBgKind> {
    if main_3d && bg == 0 {
        return Some(NdsBgKind::ThreeD);
    }

    match bg_mode {
        0 => Some(NdsBgKind::Text),
        1 => match bg {
            0 | 1 | 2 => Some(NdsBgKind::Text),
            3 => Some(bitmap_or_affine_kind(bgcnt)),
            _ => None,
        },
        2 => match bg {
            0 | 1 => Some(NdsBgKind::Text),
            2 | 3 => Some(NdsBgKind::Affine),
            _ => None,
        },
        3 => match bg {
            0 | 1 | 2 => Some(NdsBgKind::Text),
            3 => Some(bitmap_or_affine_kind(bgcnt)),
            _ => None,
        },
        4 => match bg {
            0 | 1 => Some(NdsBgKind::Text),
            2 => Some(NdsBgKind::Affine),
            3 => Some(bitmap_or_affine_kind(bgcnt)),
            _ => None,
        },
        5 => match bg {
            0 | 1 => Some(NdsBgKind::Text),
            2 | 3 => Some(bitmap_or_affine_kind(bgcnt)),
            _ => None,
        },
        _ => None,
    }
}

fn sample_3d_pixel(main_3d_framebuffer: &[u16], px: usize, screen_line: usize) -> Option<u16> {
    let pixel = *main_3d_framebuffer.get(screen_line * NDS_WIDTH + px)?;
    if pixel & 0x8000 == 0 {
        return None;
    }

    Some(pixel & 0x7FFF)
}

fn bitmap_or_affine_kind(bgcnt: u16) -> NdsBgKind {
    if bgcnt & 0x0080 != 0 {
        if bgcnt & 0x0004 != 0 {
            NdsBgKind::Bitmap16
        } else {
            NdsBgKind::Bitmap8
        }
    } else {
        NdsBgKind::Affine
    }
}

fn obj_size(shape: u16, size: u16) -> (usize, usize) {
    match (shape, size) {
        (0, 0) => (8, 8),
        (0, 1) => (16, 16),
        (0, 2) => (32, 32),
        (0, 3) => (64, 64),
        (1, 0) => (16, 8),
        (1, 1) => (32, 8),
        (1, 2) => (32, 16),
        (1, 3) => (64, 32),
        (2, 0) => (8, 16),
        (2, 1) => (8, 32),
        (2, 2) => (16, 32),
        (2, 3) => (32, 64),
        _ => (8, 8),
    }
}

fn sample_obj_pixel(
    obj_vram: &[u8],
    palette: &[u8],
    palette_offset: usize,
    tile_num: usize,
    palette_num: usize,
    color_256: bool,
    obj_mapping_1d: bool,
    width: usize,
    src_x: usize,
    src_y: usize,
) -> Option<u16> {
    let tile_row = src_y / 8;
    let fine_y = src_y % 8;
    let tile_col = src_x / 8;
    let fine_x = src_x % 8;

    if color_256 {
        let tile_offset = if obj_mapping_1d {
            tile_num + tile_row * (width / 8) * 2 + tile_col * 2
        } else {
            tile_num + tile_row * 32 + tile_col * 2
        };
        let offset = tile_offset * 32 + fine_y * 8 + fine_x;
        let palette_index = *obj_vram.get(offset)? as usize;
        if palette_index == 0 {
            return None;
        }

        let palette_entry = palette_offset + palette_index * 2;
        if palette_entry + 1 >= palette.len() {
            return None;
        }
        Some(read_color_16(palette, palette_entry))
    } else {
        let tile_offset = if obj_mapping_1d {
            tile_num + tile_row * (width / 8) + tile_col
        } else {
            tile_num + tile_row * 32 + tile_col
        };
        let offset = tile_offset * 32 + fine_y * 4 + fine_x / 2;
        let byte = *obj_vram.get(offset)?;
        let palette_index = if fine_x & 1 == 0 { byte & 0x0F } else { byte >> 4 } as usize;
        if palette_index == 0 {
            return None;
        }

        let palette_entry = palette_offset + palette_num * 32 + palette_index * 2;
        if palette_entry + 1 >= palette.len() {
            return None;
        }
        Some(read_color_16(palette, palette_entry))
    }
}

fn sample_text_bg_pixel(
    vram: &[u8],
    palette: &[u8],
    palette_offset: usize,
    bgcnt: u16,
    bghofs: u16,
    bgvofs: u16,
    px: usize,
    screen_line: usize,
) -> Option<u16> {
    let char_base = ((bgcnt as usize >> 2) & 0xF) * 0x4000;
    let screen_base = ((bgcnt as usize >> 8) & 0x1F) * 0x800;
    let color_256 = bgcnt & 0x0080 != 0;
    let (map_w, map_h) = match (bgcnt >> 14) & 0x3 {
        0 => (32usize, 32usize),
        1 => (64usize, 32usize),
        2 => (32usize, 64usize),
        3 => (64usize, 64usize),
        _ => (32usize, 32usize),
    };

    let x = (px + bghofs as usize) % (map_w * 8);
    let y = (screen_line + bgvofs as usize) % (map_h * 8);
    let tile_x = x / 8;
    let tile_y = y / 8;
    let fine_x = x % 8;
    let fine_y = y % 8;

    let mut screen_block = 0usize;
    let local_tx = tile_x % 32;
    let local_ty = tile_y % 32;
    if tile_x >= 32 {
        screen_block += 1;
    }
    if tile_y >= 32 {
        screen_block += if map_w == 64 { 2 } else { 1 };
    }

    let map_offset = screen_base + screen_block * 0x800 + (local_ty * 32 + local_tx) * 2;
    if map_offset + 1 >= vram.len() {
        return None;
    }

    let entry = read_color_16(vram, map_offset);
    let tile_num = (entry & 0x03FF) as usize;
    let h_flip = entry & 0x0400 != 0;
    let v_flip = entry & 0x0800 != 0;
    let palette_bank = ((entry >> 12) & 0xF) as usize;
    let ty = if v_flip { 7 - fine_y } else { fine_y };
    let tx = if h_flip { 7 - fine_x } else { fine_x };

    if color_256 {
        let tile_offset = char_base + tile_num * 64 + ty * 8 + tx;
        if tile_offset >= vram.len() {
            return None;
        }
        let palette_index = vram[tile_offset] as usize;
        if palette_index == 0 {
            return None;
        }
        let palette_entry = palette_offset + palette_index * 2;
        if palette_entry + 1 >= palette.len() {
            return None;
        }
        Some(read_color_16(palette, palette_entry))
    } else {
        let tile_offset = char_base + tile_num * 32 + ty * 4 + tx / 2;
        if tile_offset >= vram.len() {
            return None;
        }
        let packed = vram[tile_offset];
        let palette_index = if tx & 1 == 0 { packed & 0x0F } else { packed >> 4 } as usize;
        if palette_index == 0 {
            return None;
        }
        let palette_entry = palette_offset + (palette_bank * 16 + palette_index) * 2;
        if palette_entry + 1 >= palette.len() {
            return None;
        }
        Some(read_color_16(palette, palette_entry))
    }
}

fn sample_affine_bg_pixel(
    vram: &[u8],
    palette: &[u8],
    palette_offset: usize,
    bgcnt: u16,
    bg_pa: i16,
    bg_pb: i16,
    bg_pc: i16,
    bg_pd: i16,
    bg_ref_x: i32,
    bg_ref_y: i32,
    px: usize,
    screen_line: usize,
) -> Option<u16> {
    let char_base = ((bgcnt as usize >> 2) & 0xF) * 0x4000;
    let screen_base = ((bgcnt as usize >> 8) & 0x1F) * 0x800;
    let wrap = bgcnt & 0x2000 != 0;
    let screen_size = match (bgcnt >> 14) & 0x3 {
        0 => 128i32,
        1 => 256i32,
        2 => 512i32,
        3 => 1024i32,
        _ => 128i32,
    };
    let map_size = (screen_size as usize) / 8;
    let tex_x = (bg_ref_x + bg_pa as i32 * px as i32 + bg_pb as i32 * screen_line as i32) >> 8;
    let tex_y = (bg_ref_y + bg_pc as i32 * px as i32 + bg_pd as i32 * screen_line as i32) >> 8;

    let (tx, ty) = if wrap {
        (tex_x.rem_euclid(screen_size), tex_y.rem_euclid(screen_size))
    } else {
        if tex_x < 0 || tex_x >= screen_size || tex_y < 0 || tex_y >= screen_size {
            return None;
        }
        (tex_x, tex_y)
    };

    let tile_x = (tx / 8) as usize;
    let tile_y = (ty / 8) as usize;
    let fine_x = (tx % 8) as usize;
    let fine_y = (ty % 8) as usize;

    let map_offset = screen_base + tile_y * map_size + tile_x;
    if map_offset >= vram.len() {
        return None;
    }

    let tile_num = vram[map_offset] as usize;
    let pixel_offset = char_base + tile_num * 64 + fine_y * 8 + fine_x;
    if pixel_offset >= vram.len() {
        return None;
    }

    let palette_index = vram[pixel_offset] as usize;
    if palette_index == 0 {
        return None;
    }

    let palette_entry = palette_offset + palette_index * 2;
    if palette_entry + 1 >= palette.len() {
        return None;
    }

    Some(read_color_16(palette, palette_entry))
}

fn sample_bitmap_bg_pixel(
    vram: &[u8],
    palette: &[u8],
    palette_offset: usize,
    bgcnt: u16,
    bg_pa: i16,
    bg_pb: i16,
    bg_pc: i16,
    bg_pd: i16,
    bg_ref_x: i32,
    bg_ref_y: i32,
    px: usize,
    screen_line: usize,
    direct_color: bool,
) -> Option<u16> {
    let bitmap_base = ((bgcnt as usize >> 8) & 0x1F) * 0x4000;
    let wrap = bgcnt & 0x2000 != 0;
    let (width, height) = match (bgcnt >> 14) & 0x3 {
        0 => (128i32, 128i32),
        1 => (256i32, 256i32),
        2 => (512i32, 256i32),
        3 => (512i32, 512i32),
        _ => (128i32, 128i32),
    };
    let tex_x = (bg_ref_x + bg_pa as i32 * px as i32 + bg_pb as i32 * screen_line as i32) >> 8;
    let tex_y = (bg_ref_y + bg_pc as i32 * px as i32 + bg_pd as i32 * screen_line as i32) >> 8;

    let (tx, ty) = if wrap {
        (tex_x.rem_euclid(width), tex_y.rem_euclid(height))
    } else {
        if tex_x < 0 || tex_x >= width || tex_y < 0 || tex_y >= height {
            return None;
        }
        (tex_x, tex_y)
    };

    let pixel_index = ty as usize * width as usize + tx as usize;
    if direct_color {
        let offset = bitmap_base + pixel_index * 2;
        if offset + 1 >= vram.len() {
            return None;
        }
        let color = read_color_16(vram, offset);
        if color & 0x8000 == 0 {
            return None;
        }
        Some(color & 0x7FFF)
    } else {
        let offset = bitmap_base + pixel_index;
        let palette_index = *vram.get(offset)? as usize;
        if palette_index == 0 {
            return None;
        }

        let palette_entry = palette_offset + palette_index * 2;
        if palette_entry + 1 >= palette.len() {
            return None;
        }
        Some(read_color_16(palette, palette_entry))
    }
}

fn apply_3d_clear_plane(main_3d_framebuffer: &mut [u16], clear_color: u32, disp3dcnt: u16) {
    if disp3dcnt & DISP3DCNT_CLEAR_BMP_BIT != 0 {
        return;
    }

    let alpha = ((clear_color >> 16) & 0x1F) as u16;
    if alpha == 0 {
        return;
    }

    let fill = 0x8000 | (clear_color as u16 & 0x7FFF);
    for pixel in main_3d_framebuffer.iter_mut() {
        if *pixel & 0x8000 == 0 {
            *pixel = fill;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_master_brightness, describe_video_warning, rgb555_to_argb, supports_2d_layer_render,
        NdsEmulator, NdsRomHeader, BG_ENABLE_BITS, ENABLE_3D_BIT, MAIN_BG_PALETTE_OFFSET,
        MAIN_OBJ_PALETTE_OFFSET, MAIN_SCREEN_VRAM_OFFSET, NDS_SCREEN_HEIGHT, NDS_WIDTH,
        OBJ_1D_MAPPING_BIT, OBJ_ENABLE_BIT, SUB_BG_PALETTE_OFFSET, SUB_OBJ_PALETTE_OFFSET,
        SUB_SCREEN_VRAM_OFFSET,
    };
    use crate::cpu::arm7tdmi::Arm7Bus;

    const REG_VRAMCNT_A: u32 = 0x0400_0240;
    const REG_VRAMCNT_C: u32 = 0x0400_0242;
    const REG_VRAMCNT_D: u32 = 0x0400_0243;
    const REG_GFX_CLEAR_COLOR: u32 = 0x0400_0350;

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

    #[test]
    fn nds_emulator_renders_main_and_sub_vram_previews() {
        let rom = build_test_rom();
        let mut emu = NdsEmulator::new(rom).expect("test ROM should bootstrap");

        emu.bus.ppu_main.dispcnt = 1;
        emu.bus.ppu_sub.dispcnt = 1;
        emu.bus.memory.vram[MAIN_SCREEN_VRAM_OFFSET..MAIN_SCREEN_VRAM_OFFSET + 2]
            .copy_from_slice(&0x001Fu16.to_le_bytes());
        emu.bus.memory.vram[SUB_SCREEN_VRAM_OFFSET..SUB_SCREEN_VRAM_OFFSET + 2]
            .copy_from_slice(&0x7C00u16.to_le_bytes());

        emu.run_frame();

        assert_eq!(emu.framebuffer[0], rgb555_to_argb(0x001F));
        assert_eq!(emu.framebuffer[NDS_WIDTH * NDS_SCREEN_HEIGHT], rgb555_to_argb(0x7C00));
    }

    #[test]
    fn nds_emulator_applies_master_brightness_to_screen_preview() {
        let rom = build_test_rom();
        let mut emu = NdsEmulator::new(rom).expect("test ROM should bootstrap");

        emu.bus.ppu_main.dispcnt = 1;
        emu.bus.ppu_main.master_bright = (1 << 14) | 8;
        emu.bus.memory.vram[MAIN_SCREEN_VRAM_OFFSET..MAIN_SCREEN_VRAM_OFFSET + 2]
            .copy_from_slice(&0x4210u16.to_le_bytes());

        emu.run_frame();

        let expected = rgb555_to_argb(apply_master_brightness(0x4210, emu.bus.ppu_main.master_bright));
        assert_eq!(emu.framebuffer[0], expected);
    }

    #[test]
    fn nds_emulator_renders_main_and_sub_bg0_text_layers() {
        let rom = build_test_rom();
        let mut emu = NdsEmulator::new(rom).expect("test ROM should bootstrap");

        emu.bus.ppu_main.dispcnt = BG_ENABLE_BITS[0];
        emu.bus.ppu_main.bgcnt[0] = 0x0080 | (1 << 8);
        emu.bus.ppu_sub.dispcnt = BG_ENABLE_BITS[0];
        emu.bus.ppu_sub.bgcnt[0] = 0x0080 | (1 << 8);

        {
            let mut arm9_bus = emu.bus.arm9_view();
            arm9_bus.write8(REG_VRAMCNT_A, 0x80 | 0x01);
            arm9_bus.write8(REG_VRAMCNT_C, 0x80 | 0x04);
            arm9_bus.write8(0x0600_0000, 1);
            arm9_bus.write16(0x0600_0800, 0);
            arm9_bus.write8(0x0620_0000, 1);
            arm9_bus.write16(0x0620_0800, 0);
        }

        emu.bus.memory.palette[MAIN_BG_PALETTE_OFFSET + 2..MAIN_BG_PALETTE_OFFSET + 4]
            .copy_from_slice(&0x03E0u16.to_le_bytes());
        emu.bus.memory.palette[SUB_BG_PALETTE_OFFSET + 2..SUB_BG_PALETTE_OFFSET + 4]
            .copy_from_slice(&0x7C00u16.to_le_bytes());

        emu.run_frame();

        assert_eq!(emu.framebuffer[0], rgb555_to_argb(0x03E0));
        assert_eq!(emu.framebuffer[NDS_WIDTH * NDS_SCREEN_HEIGHT], rgb555_to_argb(0x7C00));
    }

    #[test]
    fn nds_emulator_prefers_higher_priority_text_bg_pixels() {
        let rom = build_test_rom();
        let mut emu = NdsEmulator::new(rom).expect("test ROM should bootstrap");

        emu.bus.ppu_main.dispcnt = BG_ENABLE_BITS[0] | BG_ENABLE_BITS[1];
        emu.bus.ppu_main.bgcnt[0] = 0x0080 | (1 << 8) | 1;
        emu.bus.ppu_main.bgcnt[1] = 0x0080 | (2 << 8) | (1 << 2);

        {
            let mut arm9_bus = emu.bus.arm9_view();
            arm9_bus.write8(REG_VRAMCNT_A, 0x80 | 0x01);
            arm9_bus.write8(0x0600_0000, 1);
            arm9_bus.write16(0x0600_0800, 0);
            arm9_bus.write8(0x0600_4000, 2);
            arm9_bus.write16(0x0600_1000, 0);
        }

        emu.bus.memory.palette[MAIN_BG_PALETTE_OFFSET + 2..MAIN_BG_PALETTE_OFFSET + 4]
            .copy_from_slice(&0x03E0u16.to_le_bytes());
        emu.bus.memory.palette[MAIN_BG_PALETTE_OFFSET + 4..MAIN_BG_PALETTE_OFFSET + 6]
            .copy_from_slice(&0x001Fu16.to_le_bytes());

        emu.run_frame();

        assert_eq!(emu.framebuffer[0], rgb555_to_argb(0x001F));
    }

    #[test]
    fn nds_emulator_renders_affine_bg2_layers() {
        let rom = build_test_rom();
        let mut emu = NdsEmulator::new(rom).expect("test ROM should bootstrap");

        emu.bus.ppu_main.dispcnt = (1 << 16) | 1 | BG_ENABLE_BITS[2];
        emu.bus.ppu_main.bgcnt[2] = 1 << 8;
        emu.bus.ppu_main.bg_pa[0] = 0x0100;
        emu.bus.ppu_main.bg_pd[0] = 0x0100;
        {
            let mut arm9_bus = emu.bus.arm9_view();
            arm9_bus.write8(REG_VRAMCNT_A, 0x80 | 0x01);
            arm9_bus.write8(0x0600_0000, 1);
            arm9_bus.write8(0x0600_0800, 0);
        }
        emu.bus.memory.palette[MAIN_BG_PALETTE_OFFSET + 2..MAIN_BG_PALETTE_OFFSET + 4]
            .copy_from_slice(&0x7C00u16.to_le_bytes());

        emu.run_frame();

        assert_eq!(emu.framebuffer[0], rgb555_to_argb(0x7C00));
    }

    #[test]
    fn nds_emulator_renders_main_bg3_bitmap_layers() {
        let rom = build_test_rom();
        let mut emu = NdsEmulator::new(rom).expect("test ROM should bootstrap");

        emu.bus.ppu_main.dispcnt = (1 << 16) | 3 | BG_ENABLE_BITS[3];
        emu.bus.ppu_main.bgcnt[3] = 0x0080 | (1 << 8);
        emu.bus.ppu_main.bg_pa[1] = 0x0100;
        emu.bus.ppu_main.bg_pd[1] = 0x0100;
        {
            let mut arm9_bus = emu.bus.arm9_view();
            arm9_bus.write8(REG_VRAMCNT_A, 0x80 | 0x01);
            arm9_bus.write8(0x0600_4000, 1);
        }
        emu.bus.memory.palette[MAIN_BG_PALETTE_OFFSET + 2..MAIN_BG_PALETTE_OFFSET + 4]
            .copy_from_slice(&0x03E0u16.to_le_bytes());

        emu.run_frame();

        assert_eq!(emu.framebuffer[0], rgb555_to_argb(0x03E0));
    }

    #[test]
    fn nds_emulator_renders_sub_bg2_direct_bitmap_layers() {
        let rom = build_test_rom();
        let mut emu = NdsEmulator::new(rom).expect("test ROM should bootstrap");

        emu.bus.ppu_sub.dispcnt = (1 << 16) | 5 | BG_ENABLE_BITS[2];
        emu.bus.ppu_sub.bgcnt[2] = 0x0080 | 0x0004 | (1 << 8);
        emu.bus.ppu_sub.bg_pa[0] = 0x0100;
        emu.bus.ppu_sub.bg_pd[0] = 0x0100;
        {
            let mut arm9_bus = emu.bus.arm9_view();
            arm9_bus.write8(REG_VRAMCNT_C, 0x80 | 0x04);
            arm9_bus.write16(0x0620_4000, 0x8000 | 0x001F);
        }

        emu.run_frame();

        assert_eq!(emu.framebuffer[NDS_WIDTH * NDS_SCREEN_HEIGHT], rgb555_to_argb(0x001F));
    }

    #[test]
    fn nds_emulator_renders_main_bg0_3d_surface() {
        let rom = build_test_rom();
        let mut emu = NdsEmulator::new(rom).expect("test ROM should bootstrap");

        emu.bus.ppu_main.dispcnt = (1 << 16) | ENABLE_3D_BIT | BG_ENABLE_BITS[0];
        emu.main_3d_framebuffer[0] = 0x8000 | 0x03E0;

        emu.run_frame();

        assert_eq!(emu.framebuffer[0], rgb555_to_argb(0x03E0));
    }

    #[test]
    fn nds_emulator_renders_main_3d_clear_plane() {
        let rom = build_test_rom();
        let mut emu = NdsEmulator::new(rom).expect("test ROM should bootstrap");

        emu.bus.ppu_main.dispcnt = (1 << 16) | ENABLE_3D_BIT | BG_ENABLE_BITS[0];
        {
            let mut arm9_bus = emu.bus.arm9_view();
            arm9_bus.write32(REG_GFX_CLEAR_COLOR, (31 << 16) | 0x001F);
        }

        emu.run_frame();

        assert_eq!(emu.framebuffer[0], rgb555_to_argb(0x001F));
    }

    #[test]
    fn nds_emulator_renders_main_obj_sprites() {
        let rom = build_test_rom();
        let mut emu = NdsEmulator::new(rom).expect("test ROM should bootstrap");

        emu.bus.ppu_main.dispcnt = OBJ_ENABLE_BIT | OBJ_1D_MAPPING_BIT;
        {
            let mut arm9_bus = emu.bus.arm9_view();
            arm9_bus.write8(REG_VRAMCNT_A, 0x80 | 0x02);
            arm9_bus.write8(0x0640_0000, 1);
            arm9_bus.write16(0x0700_0000, 0x2000);
            arm9_bus.write16(0x0700_0002, 0);
            arm9_bus.write16(0x0700_0004, 0);
        }
        emu.bus.memory.palette[MAIN_OBJ_PALETTE_OFFSET + 2..MAIN_OBJ_PALETTE_OFFSET + 4]
            .copy_from_slice(&0x001Fu16.to_le_bytes());

        emu.run_frame();

        assert_eq!(emu.framebuffer[0], rgb555_to_argb(0x001F));
    }

    #[test]
    fn nds_emulator_renders_sub_obj_sprites() {
        let rom = build_test_rom();
        let mut emu = NdsEmulator::new(rom).expect("test ROM should bootstrap");

        emu.bus.ppu_sub.dispcnt = OBJ_ENABLE_BIT | OBJ_1D_MAPPING_BIT;
        {
            let mut arm9_bus = emu.bus.arm9_view();
            arm9_bus.write8(REG_VRAMCNT_D, 0x80 | 0x04);
            arm9_bus.write8(0x0660_0000, 1);
            arm9_bus.write16(0x0700_0400, 0x2000);
            arm9_bus.write16(0x0700_0402, 0);
            arm9_bus.write16(0x0700_0404, 0);
        }
        emu.bus.memory.palette[SUB_OBJ_PALETTE_OFFSET + 2..SUB_OBJ_PALETTE_OFFSET + 4]
            .copy_from_slice(&0x7C00u16.to_le_bytes());

        emu.run_frame();

        assert_eq!(emu.framebuffer[NDS_WIDTH * NDS_SCREEN_HEIGHT], rgb555_to_argb(0x7C00));
    }

    #[test]
    fn nds_2d_renderer_rejects_unsupported_display_modes() {
        assert!(supports_2d_layer_render(BG_ENABLE_BITS[0]));
        assert!(supports_2d_layer_render((1 << 16) | BG_ENABLE_BITS[0]));
        assert!(supports_2d_layer_render((1 << 16) | ENABLE_3D_BIT | BG_ENABLE_BITS[0]));
        assert!(supports_2d_layer_render((1 << 16) | 5 | BG_ENABLE_BITS[3]));
        assert!(!supports_2d_layer_render(2 << 16));
        assert!(!supports_2d_layer_render(6));
    }

    #[test]
    fn nds_video_warning_reports_unsupported_modes() {
        let warning =
            describe_video_warning("Main", true, ENABLE_3D_BIT | (2 << 16) | 6, 0).expect("warning expected");
        assert!(warning.contains("display mode 2"));
        assert!(warning.contains("BG mode 6"));
        assert!(warning.contains("3D rasterizer"));
    }
}