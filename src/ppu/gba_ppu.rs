use serde::{Deserialize, Serialize};

pub const SCREEN_W: usize = 240;
pub const SCREEN_H: usize = 160;

#[derive(Clone, Serialize, Deserialize)]
pub struct GbaPpu {
    pub framebuffer: Vec<u32>,
    pub vram: Vec<u8>,       // 96KB
    pub palette: Vec<u8>,    // 1KB
    pub oam: Vec<u8>,        // 1KB

    // Registers
    pub dispcnt: u16,
    pub dispstat: u16,
    pub vcount: u16,

    // BG control
    pub bgcnt: [u16; 4],
    pub bghofs: [u16; 4],
    pub bgvofs: [u16; 4],
    // Affine BG
    pub bg_ref_x: [i32; 2],   // BG2/BG3 reference X (28.4 fixed point internal)
    pub bg_ref_y: [i32; 2],
    pub bg_pa: [i16; 2],
    pub bg_pb: [i16; 2],
    pub bg_pc: [i16; 2],
    pub bg_pd: [i16; 2],
    pub bg_internal_x: [i32; 2],
    pub bg_internal_y: [i32; 2],

    // Window
    pub winh: [u16; 2],
    pub winv: [u16; 2],
    pub winin: u16,
    pub winout: u16,

    // Blend
    pub bldcnt: u16,
    pub bldalpha: u16,
    pub bldy: u16,

    // Mosaic
    pub mosaic: u16,

    pub cycles: u32,
    pub frame_ready: bool,
}

impl GbaPpu {
    pub fn new() -> Self {
        Self {
            framebuffer: vec![0xFF_FF_FF_FF; SCREEN_W * SCREEN_H],
            vram: vec![0; 0x18000],
            palette: vec![0; 0x400],
            oam: vec![0; 0x400],
            dispcnt: 0,
            dispstat: 0,
            vcount: 0,
            bgcnt: [0; 4],
            bghofs: [0; 4],
            bgvofs: [0; 4],
            bg_ref_x: [0; 2],
            bg_ref_y: [0; 2],
            bg_pa: [0x100; 2],
            bg_pb: [0; 2],
            bg_pc: [0; 2],
            bg_pd: [0x100; 2],
            bg_internal_x: [0; 2],
            bg_internal_y: [0; 2],
            winh: [0; 2],
            winv: [0; 2],
            winin: 0,
            winout: 0,
            bldcnt: 0,
            bldalpha: 0,
            bldy: 0,
            mosaic: 0,
            cycles: 0,
            frame_ready: false,
        }
    }

    pub fn tick(&mut self, cycles: u32) -> u16 {
        let mut irqs = 0u16;
        self.cycles += cycles;

        // 1232 cycles per scanline = 960 (visible) + 272 (hblank)
        // 228 scanlines total = 160 visible + 68 vblank
        while self.cycles >= 1232 {
            self.cycles -= 1232;

            // Render current line if in visible area
            if self.vcount < 160 {
                self.render_scanline();
            }

            // HBlank
            if self.vcount < 160 {
                irqs |= 0x0200; // HBlank event (for DMA)
                if self.dispstat & 0x10 != 0 {
                    irqs |= 0x02; // HBlank IRQ
                }
                // Update affine reference points
                for i in 0..2 {
                    self.bg_internal_x[i] = self.bg_internal_x[i].wrapping_add(self.bg_pb[i] as i32);
                    self.bg_internal_y[i] = self.bg_internal_y[i].wrapping_add(self.bg_pd[i] as i32);
                }
            }

            self.vcount += 1;

            if self.vcount == 160 {
                // VBlank start
                self.frame_ready = true;
                irqs |= 0x0100; // VBlank event (for DMA)
                if self.dispstat & 0x08 != 0 {
                    irqs |= 0x01; // VBlank IRQ
                }
            }

            if self.vcount >= 228 {
                self.vcount = 0;
                // Reload affine reference points
                for i in 0..2 {
                    self.bg_internal_x[i] = self.bg_ref_x[i];
                    self.bg_internal_y[i] = self.bg_ref_y[i];
                }
            }

            // LYC check
            let lyc = (self.dispstat >> 8) as u16;
            if self.vcount == lyc {
                self.dispstat |= 0x04; // V-Counter flag
                if self.dispstat & 0x20 != 0 {
                    irqs |= 0x04; // V-Counter IRQ
                }
            } else {
                self.dispstat &= !0x04;
            }

            // Update VBlank/HBlank flags
            if self.vcount >= 160 {
                self.dispstat |= 0x01;
            } else {
                self.dispstat &= !0x01;
            }
        }

        irqs
    }

    fn render_scanline(&mut self) {
        let line = self.vcount as usize;
        if line >= SCREEN_H {
            return;
        }

        let mode = self.dispcnt & 0x07;
        let row_start = line * SCREEN_W;

        // Clear with backdrop color
        let backdrop = self.palette_color(0);
        for px in &mut self.framebuffer[row_start..row_start + SCREEN_W] {
            *px = backdrop;
        }

        match mode {
            0 => self.render_mode0(line),
            1 => self.render_mode1(line),
            2 => self.render_mode2(line),
            3 => self.render_mode3(line),
            4 => self.render_mode4(line),
            5 => self.render_mode5(line),
            _ => {}
        }

        // Render sprites
        if self.dispcnt & 0x1000 != 0 {
            self.render_sprites(line);
        }
    }

    fn render_mode0(&mut self, line: usize) {
        // 4 text BGs
        for bg in (0..4).rev() {
            if self.dispcnt & (1 << (8 + bg)) != 0 {
                self.render_text_bg(bg, line);
            }
        }
    }

    fn render_mode1(&mut self, line: usize) {
        // BG0, BG1 text; BG2 affine
        if self.dispcnt & (1 << 10) != 0 {
            self.render_affine_bg(0, line);
        }
        for bg in [1, 0] {
            if self.dispcnt & (1 << (8 + bg)) != 0 {
                self.render_text_bg(bg, line);
            }
        }
    }

    fn render_mode2(&mut self, line: usize) {
        // BG2, BG3 affine
        for i in (0..2).rev() {
            if self.dispcnt & (1 << (10 + i)) != 0 {
                self.render_affine_bg(i, line);
            }
        }
    }

    fn render_mode3(&mut self, line: usize) {
        // 240x160 16-bit color bitmap
        let row_start = line * SCREEN_W;
        for x in 0..SCREEN_W {
            let offset = (line * SCREEN_W + x) * 2;
            if offset + 1 < self.vram.len() {
                let color = (self.vram[offset] as u16) | ((self.vram[offset + 1] as u16) << 8);
                self.framebuffer[row_start + x] = Self::rgb555_to_argb(color);
            }
        }
    }

    fn render_mode4(&mut self, line: usize) {
        // 240x160 8-bit indexed bitmap, 2 frames
        let frame_offset = if self.dispcnt & 0x10 != 0 { 0xA000 } else { 0 };
        let row_start = line * SCREEN_W;
        for x in 0..SCREEN_W {
            let offset = frame_offset + line * SCREEN_W + x;
            if offset < self.vram.len() {
                let pal_idx = self.vram[offset];
                if pal_idx != 0 {
                    self.framebuffer[row_start + x] = self.palette_color(pal_idx as usize * 2);
                }
            }
        }
    }

    fn render_mode5(&mut self, line: usize) {
        // 160x128 16-bit color bitmap, 2 frames
        if line >= 128 { return; }
        let frame_offset = if self.dispcnt & 0x10 != 0 { 0xA000 } else { 0 };
        let row_start = line * SCREEN_W;
        for x in 0..160 {
            let offset = frame_offset + (line * 160 + x) * 2;
            if offset + 1 < self.vram.len() {
                let color = (self.vram[offset] as u16) | ((self.vram[offset + 1] as u16) << 8);
                self.framebuffer[row_start + x] = Self::rgb555_to_argb(color);
            }
        }
    }

    fn render_text_bg(&mut self, bg: usize, line: usize) {
        let cnt = self.bgcnt[bg];
        let char_base = ((cnt as usize >> 2) & 3) * 0x4000;
        let screen_base = ((cnt as usize >> 8) & 0x1F) * 0x800;
        let color_256 = cnt & 0x80 != 0;
        let screen_size = (cnt >> 14) & 3;

        let (map_w, map_h): (usize, usize) = match screen_size {
            0 => (32, 32),
            1 => (64, 32),
            2 => (32, 64),
            3 => (64, 64),
            _ => (32, 32),
        };

        let scroll_x = self.bghofs[bg] as usize;
        let scroll_y = self.bgvofs[bg] as usize;
        let y = (line + scroll_y) % (map_h * 8);
        let tile_y = y / 8;
        let fine_y = y % 8;
        let row_start = line * SCREEN_W;

        for px in 0..SCREEN_W {
            let x = (px + scroll_x) % (map_w * 8);
            let tile_x = x / 8;
            let fine_x = x % 8;

            // Calculate screen block offset for wide/tall maps
            let mut screen_block = 0;
            let local_tx = tile_x % 32;
            let local_ty = tile_y % 32;
            if tile_x >= 32 { screen_block += 1; }
            if tile_y >= 32 { screen_block += if map_w == 64 { 2 } else { 1 }; }

            let map_offset = screen_base + screen_block * 0x800 + (local_ty * 32 + local_tx) * 2;
            if map_offset + 1 >= self.vram.len() { continue; }

            let entry = (self.vram[map_offset] as u16) | ((self.vram[map_offset + 1] as u16) << 8);
            let tile_num = (entry & 0x3FF) as usize;
            let h_flip = entry & 0x400 != 0;
            let v_flip = entry & 0x800 != 0;
            let pal = ((entry >> 12) & 0xF) as usize;

            let ty = if v_flip { 7 - fine_y } else { fine_y };
            let tx = if h_flip { 7 - fine_x } else { fine_x };

            let color = if color_256 {
                let offset = char_base + tile_num * 64 + ty * 8 + tx;
                if offset >= self.vram.len() { continue; }
                let pal_idx = self.vram[offset];
                if pal_idx == 0 { continue; }
                self.palette_color(pal_idx as usize * 2)
            } else {
                let offset = char_base + tile_num * 32 + ty * 4 + tx / 2;
                if offset >= self.vram.len() { continue; }
                let byte = self.vram[offset];
                let pal_idx = if tx & 1 == 0 { byte & 0xF } else { byte >> 4 };
                if pal_idx == 0 { continue; }
                self.palette_color((pal * 32 + pal_idx as usize * 2) as usize)
            };

            self.framebuffer[row_start + px] = color;
        }
    }

    fn render_affine_bg(&mut self, idx: usize, line: usize) {
        let bg = idx + 2; // Affine BGs are BG2/BG3
        let cnt = self.bgcnt[bg];
        let char_base = ((cnt as usize >> 2) & 3) * 0x4000;
        let screen_base = ((cnt as usize >> 8) & 0x1F) * 0x800;
        let wrap = cnt & 0x2000 != 0;

        let screen_size = match (cnt >> 14) & 3 {
            0 => 128,
            1 => 256,
            2 => 512,
            3 => 1024,
            _ => 128,
        };
        let map_size = screen_size / 8;

        let ref_x = self.bg_internal_x[idx];
        let ref_y = self.bg_internal_y[idx];
        let pa = self.bg_pa[idx] as i32;
        let pc = self.bg_pc[idx] as i32;

        let row_start = line * SCREEN_W;

        for px in 0..SCREEN_W {
            let tex_x = (ref_x + pa * px as i32) >> 8;
            let tex_y = (ref_y + pc * px as i32) >> 8;

            let (tx, ty) = if wrap {
                (tex_x.rem_euclid(screen_size as i32), tex_y.rem_euclid(screen_size as i32))
            } else {
                if tex_x < 0 || tex_x >= screen_size as i32 || tex_y < 0 || tex_y >= screen_size as i32 {
                    continue;
                }
                (tex_x, tex_y)
            };

            let tile_x = (tx / 8) as usize;
            let tile_y = (ty / 8) as usize;
            let fine_x = (tx % 8) as usize;
            let fine_y = (ty % 8) as usize;

            let map_offset = screen_base + tile_y * map_size + tile_x;
            if map_offset >= self.vram.len() { continue; }

            let tile_num = self.vram[map_offset] as usize;
            let pixel_offset = char_base + tile_num * 64 + fine_y * 8 + fine_x;
            if pixel_offset >= self.vram.len() { continue; }

            let pal_idx = self.vram[pixel_offset];
            if pal_idx == 0 { continue; }

            self.framebuffer[row_start + px] = self.palette_color(pal_idx as usize * 2);
        }
    }

    fn render_sprites(&mut self, line: usize) {
        // Parse OAM and render sprites
        for i in (0..128).rev() {
            let base = i * 8;
            let attr0 = (self.oam[base] as u16) | ((self.oam[base + 1] as u16) << 8);
            let attr1 = (self.oam[base + 2] as u16) | ((self.oam[base + 3] as u16) << 8);
            let attr2 = (self.oam[base + 4] as u16) | ((self.oam[base + 5] as u16) << 8);

            let obj_mode = (attr0 >> 8) & 3;
            if obj_mode == 2 { continue; } // Hidden

            let shape = (attr0 >> 14) & 3;
            let size = (attr1 >> 14) & 3;

            let (w, h) = Self::obj_size(shape, size);

            let y = (attr0 & 0xFF) as i32;
            let y = if y >= 160 { y - 256 } else { y };

            if (line as i32) < y || (line as i32) >= y + h as i32 {
                continue;
            }

            let x = (attr1 & 0x1FF) as i32;
            let x = if x >= 240 { x - 512 } else { x };

            let tile_num = (attr2 & 0x3FF) as usize;
            let palette_num = ((attr2 >> 12) & 0xF) as usize;
            let color_256 = attr0 & 0x2000 != 0;
            let h_flip = attr1 & 0x1000 != 0 && obj_mode != 1;
            let v_flip = attr1 & 0x2000 != 0 && obj_mode != 1;

            let sprite_y = if v_flip {
                (h - 1 - ((line as i32 - y) as usize))
            } else {
                (line as i32 - y) as usize
            };

            let tile_row = sprite_y / 8;
            let fine_y = sprite_y % 8;
            let obj_mapping_1d = self.dispcnt & 0x40 != 0;

            let row_start = line * SCREEN_W;

            for sprite_x in 0..w {
                let screen_x = x + sprite_x as i32;
                if screen_x < 0 || screen_x >= SCREEN_W as i32 { continue; }

                let px = if h_flip { w - 1 - sprite_x } else { sprite_x };

                let tile_col = px / 8;
                let fine_x = px % 8;

                let tile = if color_256 {
                    let tile_offset = if obj_mapping_1d {
                        tile_num + tile_row * (w / 8) * 2 + tile_col * 2
                    } else {
                        tile_num + tile_row * 32 + tile_col * 2
                    };
                    let offset = 0x10000 + tile_offset * 32 + fine_y * 8 + fine_x;
                    if offset >= self.vram.len() { continue; }
                    let pal_idx = self.vram[offset];
                    if pal_idx == 0 { continue; }
                    self.sprite_palette_color(pal_idx as usize * 2)
                } else {
                    let tile_offset = if obj_mapping_1d {
                        tile_num + tile_row * (w / 8) + tile_col
                    } else {
                        tile_num + tile_row * 32 + tile_col
                    };
                    let offset = 0x10000 + tile_offset * 32 + fine_y * 4 + fine_x / 2;
                    if offset >= self.vram.len() { continue; }
                    let byte = self.vram[offset];
                    let pal_idx = if fine_x & 1 == 0 { byte & 0xF } else { byte >> 4 };
                    if pal_idx == 0 { continue; }
                    self.sprite_palette_color(palette_num * 32 + pal_idx as usize * 2)
                };

                self.framebuffer[row_start + screen_x as usize] = tile;
            }
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

    fn palette_color(&self, offset: usize) -> u32 {
        if offset + 1 < self.palette.len() {
            let color = (self.palette[offset] as u16) | ((self.palette[offset + 1] as u16) << 8);
            Self::rgb555_to_argb(color)
        } else {
            0xFF_00_00_00
        }
    }

    fn sprite_palette_color(&self, offset: usize) -> u32 {
        // Sprite palette starts at 0x200 in palette RAM
        let actual_offset = 0x200 + offset;
        self.palette_color(actual_offset)
    }

    fn rgb555_to_argb(color: u16) -> u32 {
        let r = ((color & 0x1F) as u32) * 255 / 31;
        let g = (((color >> 5) & 0x1F) as u32) * 255 / 31;
        let b = (((color >> 10) & 0x1F) as u32) * 255 / 31;
        0xFF_00_00_00 | (r << 16) | (g << 8) | b
    }

    pub fn read_io(&self, addr: u32) -> u8 {
        let offset = addr & 0xFFF;
        match offset {
            0x000 => self.dispcnt as u8,
            0x001 => (self.dispcnt >> 8) as u8,
            0x004 => self.dispstat as u8,
            0x005 => (self.dispstat >> 8) as u8,
            0x006 => self.vcount as u8,
            0x007 => (self.vcount >> 8) as u8,
            0x008..=0x00F => {
                let bg = ((offset - 0x008) / 2) as usize;
                if offset & 1 == 0 { self.bgcnt[bg] as u8 } else { (self.bgcnt[bg] >> 8) as u8 }
            }
            0x048 => self.winin as u8,
            0x049 => (self.winin >> 8) as u8,
            0x04A => self.winout as u8,
            0x04B => (self.winout >> 8) as u8,
            0x050 => self.bldcnt as u8,
            0x051 => (self.bldcnt >> 8) as u8,
            0x052 => self.bldalpha as u8,
            0x053 => (self.bldalpha >> 8) as u8,
            _ => 0,
        }
    }

    pub fn write_io(&mut self, addr: u32, val: u8) {
        let offset = addr & 0xFFF;
        match offset {
            0x000 => self.dispcnt = (self.dispcnt & 0xFF00) | val as u16,
            0x001 => self.dispcnt = (self.dispcnt & 0x00FF) | ((val as u16) << 8),
            0x008..=0x00F => {
                let bg = ((offset - 0x008) / 2) as usize;
                if offset & 1 == 0 {
                    self.bgcnt[bg] = (self.bgcnt[bg] & 0xFF00) | val as u16;
                } else {
                    self.bgcnt[bg] = (self.bgcnt[bg] & 0x00FF) | ((val as u16) << 8);
                }
            }
            0x010..=0x01F => {
                let reg = ((offset - 0x010) / 2) as usize;
                let bg = reg / 2;
                let is_vofs = reg & 1 == 1;
                if bg < 4 {
                    if is_vofs {
                        if offset & 1 == 0 {
                            self.bgvofs[bg] = (self.bgvofs[bg] & 0xFF00) | val as u16;
                        } else {
                            self.bgvofs[bg] = (self.bgvofs[bg] & 0x00FF) | (((val & 1) as u16) << 8);
                        }
                    } else {
                        if offset & 1 == 0 {
                            self.bghofs[bg] = (self.bghofs[bg] & 0xFF00) | val as u16;
                        } else {
                            self.bghofs[bg] = (self.bghofs[bg] & 0x00FF) | (((val & 1) as u16) << 8);
                        }
                    }
                }
            }
            // BG2/BG3 affine parameters
            0x020..=0x03F => self.write_affine_reg(offset, val),
            0x040 => self.winh[0] = (self.winh[0] & 0xFF00) | val as u16,
            0x041 => self.winh[0] = (self.winh[0] & 0x00FF) | ((val as u16) << 8),
            0x042 => self.winh[1] = (self.winh[1] & 0xFF00) | val as u16,
            0x043 => self.winh[1] = (self.winh[1] & 0x00FF) | ((val as u16) << 8),
            0x044 => self.winv[0] = (self.winv[0] & 0xFF00) | val as u16,
            0x045 => self.winv[0] = (self.winv[0] & 0x00FF) | ((val as u16) << 8),
            0x046 => self.winv[1] = (self.winv[1] & 0xFF00) | val as u16,
            0x047 => self.winv[1] = (self.winv[1] & 0x00FF) | ((val as u16) << 8),
            0x048 => self.winin = (self.winin & 0xFF00) | val as u16,
            0x049 => self.winin = (self.winin & 0x00FF) | ((val as u16) << 8),
            0x04A => self.winout = (self.winout & 0xFF00) | val as u16,
            0x04B => self.winout = (self.winout & 0x00FF) | ((val as u16) << 8),
            0x04C => self.mosaic = (self.mosaic & 0xFF00) | val as u16,
            0x04D => self.mosaic = (self.mosaic & 0x00FF) | ((val as u16) << 8),
            0x050 => self.bldcnt = (self.bldcnt & 0xFF00) | val as u16,
            0x051 => self.bldcnt = (self.bldcnt & 0x00FF) | ((val as u16) << 8),
            0x052 => self.bldalpha = (self.bldalpha & 0xFF00) | val as u16,
            0x053 => self.bldalpha = (self.bldalpha & 0x00FF) | ((val as u16) << 8),
            0x054 => self.bldy = val as u16 & 0x1F,
            _ => {}
        }
    }

    fn write_affine_reg(&mut self, offset: u32, val: u8) {
        // BG2 affine: 0x20-0x2F, BG3 affine: 0x30-0x3F
        let bg_idx = if offset >= 0x30 { 1usize } else { 0 };
        let reg = (offset & 0x0F) as usize;

        match reg {
            0x0 => self.bg_pa[bg_idx] = (self.bg_pa[bg_idx] & !0xFF) | val as i16,
            0x1 => self.bg_pa[bg_idx] = (self.bg_pa[bg_idx] & 0xFF) | ((val as i16) << 8),
            0x2 => self.bg_pb[bg_idx] = (self.bg_pb[bg_idx] & !0xFF) | val as i16,
            0x3 => self.bg_pb[bg_idx] = (self.bg_pb[bg_idx] & 0xFF) | ((val as i16) << 8),
            0x4 => self.bg_pc[bg_idx] = (self.bg_pc[bg_idx] & !0xFF) | val as i16,
            0x5 => self.bg_pc[bg_idx] = (self.bg_pc[bg_idx] & 0xFF) | ((val as i16) << 8),
            0x6 => self.bg_pd[bg_idx] = (self.bg_pd[bg_idx] & !0xFF) | val as i16,
            0x7 => self.bg_pd[bg_idx] = (self.bg_pd[bg_idx] & 0xFF) | ((val as i16) << 8),
            0x8..=0xB => {
                // Reference point X (28-bit signed, 4-byte write)
                let byte_idx = reg - 0x8;
                let mut raw = self.bg_ref_x[bg_idx] as u32;
                let shift = byte_idx * 8;
                raw = (raw & !(0xFF << shift)) | ((val as u32) << shift);
                // Sign extend from bit 27
                self.bg_ref_x[bg_idx] = if raw & (1 << 27) != 0 {
                    (raw | 0xF000_0000) as i32
                } else {
                    (raw & 0x0FFF_FFFF) as i32
                };
                self.bg_internal_x[bg_idx] = self.bg_ref_x[bg_idx];
            }
            0xC..=0xF => {
                let byte_idx = reg - 0xC;
                let mut raw = self.bg_ref_y[bg_idx] as u32;
                let shift = byte_idx * 8;
                raw = (raw & !(0xFF << shift)) | ((val as u32) << shift);
                self.bg_ref_y[bg_idx] = if raw & (1 << 27) != 0 {
                    (raw | 0xF000_0000) as i32
                } else {
                    (raw & 0x0FFF_FFFF) as i32
                };
                self.bg_internal_y[bg_idx] = self.bg_ref_y[bg_idx];
            }
            _ => {}
        }
    }

    pub fn read_dispstat(&self) -> u16 {
        let vblank = if self.vcount >= 160 && self.vcount < 228 { 1 } else { 0 };
        let vcounter = if self.vcount == (self.dispstat >> 8) { 4 } else { 0 };
        (self.dispstat & 0xFFF8) | vcounter | vblank
    }

    pub fn write_dispstat(&mut self, byte: u32, val: u8) {
        if byte == 0 {
            // Only bits 3-5 are writable in low byte
            self.dispstat = (self.dispstat & 0xFF07) | ((val as u16 & 0x38) as u16);
        } else {
            self.dispstat = (self.dispstat & 0x00FF) | ((val as u16) << 8);
        }
    }
}
