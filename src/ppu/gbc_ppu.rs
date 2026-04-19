use serde::{Deserialize, Serialize};

pub const SCREEN_W: usize = 160;
pub const SCREEN_H: usize = 144;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PpuMode {
    HBlank = 0,
    VBlank = 1,
    OamScan = 2,
    Drawing = 3,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GbcPpu {
    pub framebuffer: Vec<u32>,
    pub vram: Vec<u8>,       // 2 banks for CGB
    pub oam: Vec<u8>,
    pub vram_bank: u8,
    pub is_cgb: bool,

    // LCD Control
    pub lcdc: u8,
    pub stat: u8,
    pub scy: u8,
    pub scx: u8,
    pub ly: u8,
    pub lyc: u8,
    pub wy: u8,
    pub wx: u8,

    // Palettes (DMG)
    pub bgp: u8,
    pub obp0: u8,
    pub obp1: u8,

    // CGB palettes
    pub bgp_index: u8,
    pub bgp_auto_inc: bool,
    pub bgp_data: Vec<u8>,
    pub obp_index: u8,
    pub obp_auto_inc: bool,
    pub obp_data: Vec<u8>,

    pub mode: PpuMode,
    pub cycles: u32,
    pub window_line: u8,
    pub frame_ready: bool,
}

impl GbcPpu {
    pub fn new(is_cgb: bool) -> Self {
        let vram_size = if is_cgb { 0x4000 } else { 0x2000 };
        Self {
            framebuffer: vec![0xFF_FF_FF_FF; SCREEN_W * SCREEN_H],
            vram: vec![0; vram_size],
            oam: vec![0; 160],
            vram_bank: 0,
            is_cgb,
            lcdc: 0x91,
            stat: 0,
            scy: 0,
            scx: 0,
            ly: 0,
            lyc: 0,
            wy: 0,
            wx: 0,
            bgp: 0xFC,
            obp0: 0xFF,
            obp1: 0xFF,
            bgp_index: 0,
            bgp_auto_inc: false,
            bgp_data: vec![0xFF; 64],
            obp_index: 0,
            obp_auto_inc: false,
            obp_data: vec![0xFF; 64],
            mode: PpuMode::OamScan,
            cycles: 0,
            window_line: 0,
            frame_ready: false,
        }
    }

    pub fn tick(&mut self, cycles: u32) -> u8 {
        if self.lcdc & 0x80 == 0 {
            return 0;
        }

        let mut irqs = 0u8;
        self.cycles += cycles;

        match self.mode {
            PpuMode::OamScan => {
                if self.cycles >= 80 {
                    self.cycles -= 80;
                    self.mode = PpuMode::Drawing;
                }
            }
            PpuMode::Drawing => {
                if self.cycles >= 172 {
                    self.cycles -= 172;
                    self.mode = PpuMode::HBlank;
                    self.render_scanline();

                    if self.stat & 0x08 != 0 {
                        irqs |= 0x02; // STAT interrupt
                    }
                }
            }
            PpuMode::HBlank => {
                if self.cycles >= 204 {
                    self.cycles -= 204;
                    self.ly += 1;

                    if self.ly == self.lyc && (self.stat & 0x40 != 0) {
                        irqs |= 0x02;
                    }

                    if self.ly >= 144 {
                        self.mode = PpuMode::VBlank;
                        irqs |= 0x01; // VBlank interrupt
                        self.frame_ready = true;

                        if self.stat & 0x10 != 0 {
                            irqs |= 0x02;
                        }
                    } else {
                        self.mode = PpuMode::OamScan;
                        if self.stat & 0x20 != 0 {
                            irqs |= 0x02;
                        }
                    }
                }
            }
            PpuMode::VBlank => {
                if self.cycles >= 456 {
                    self.cycles -= 456;
                    self.ly += 1;

                    if self.ly == self.lyc && (self.stat & 0x40 != 0) {
                        irqs |= 0x02;
                    }

                    if self.ly >= 154 {
                        self.ly = 0;
                        self.window_line = 0;
                        self.mode = PpuMode::OamScan;

                        if self.stat & 0x20 != 0 {
                            irqs |= 0x02;
                        }
                    }
                }
            }
        }

        irqs
    }

    fn render_scanline(&mut self) {
        let line = self.ly as usize;
        if line >= SCREEN_H {
            return;
        }

        // Clear line to white
        let row_start = line * SCREEN_W;
        for px in &mut self.framebuffer[row_start..row_start + SCREEN_W] {
            *px = 0xFF_FF_FF_FF;
        }

        if self.lcdc & 0x01 != 0 || self.is_cgb {
            self.render_bg_line(line);
        }

        if self.lcdc & 0x20 != 0 {
            self.render_window_line(line);
        }

        if self.lcdc & 0x02 != 0 {
            self.render_sprites_line(line);
        }
    }

    fn render_bg_line(&mut self, line: usize) {
        let tile_data_base: u16 = if self.lcdc & 0x10 != 0 { 0x8000 } else { 0x8800 };
        let tile_map_base: u16 = if self.lcdc & 0x08 != 0 { 0x9C00 } else { 0x9800 };
        let signed_tiles = self.lcdc & 0x10 == 0;

        let y = line as u8;
        let scroll_y = self.scy.wrapping_add(y);
        let tile_row = (scroll_y / 8) as u16;

        for px in 0..SCREEN_W {
            let scroll_x = self.scx.wrapping_add(px as u8);
            let tile_col = (scroll_x / 8) as u16;
            let tile_map_addr = tile_map_base + tile_row * 32 + tile_col;

            let tile_index = self.read_vram_bank(tile_map_addr, 0);

            let tile_data_addr = if signed_tiles {
                let signed_index = tile_index as i8;
                (0x9000_i32 + (signed_index as i32) * 16) as u16
            } else {
                tile_data_base + (tile_index as u16) * 16
            };

            let fine_y = (scroll_y % 8) as u16;
            let fine_x = scroll_x % 8;

            // CGB attributes
            let (vram_bank, palette_num, x_flip, y_flip, priority) = if self.is_cgb {
                let attrs = self.read_vram_bank(tile_map_addr, 1);
                let bank = (attrs >> 3) & 1;
                let pal = attrs & 0x07;
                let xf = attrs & 0x20 != 0;
                let yf = attrs & 0x40 != 0;
                let pri = attrs & 0x80 != 0;
                (bank, pal, xf, yf, pri)
            } else {
                (0, 0, false, false, false)
            };

            let actual_y = if y_flip { 7 - fine_y } else { fine_y };
            let byte1 = self.read_vram_bank(tile_data_addr + actual_y * 2, vram_bank);
            let byte2 = self.read_vram_bank(tile_data_addr + actual_y * 2 + 1, vram_bank);

            let bit = if x_flip { fine_x } else { 7 - fine_x };
            let color_idx = ((byte2 >> bit) & 1) << 1 | ((byte1 >> bit) & 1);

            let color = if self.is_cgb {
                self.cgb_bg_color(palette_num, color_idx)
            } else {
                self.dmg_color(self.bgp, color_idx)
            };

            let _ = priority; // Used for sprite priority in full implementation
            self.framebuffer[line * SCREEN_W + px] = color;
        }
    }

    fn render_window_line(&mut self, line: usize) {
        if self.wy > line as u8 {
            return;
        }

        let wx = self.wx.wrapping_sub(7);
        let tile_data_base: u16 = if self.lcdc & 0x10 != 0 { 0x8000 } else { 0x8800 };
        let tile_map_base: u16 = if self.lcdc & 0x40 != 0 { 0x9C00 } else { 0x9800 };
        let signed_tiles = self.lcdc & 0x10 == 0;

        let win_y = self.window_line;
        let tile_row = (win_y / 8) as u16;
        let mut drew_pixel = false;

        for px in 0..SCREEN_W {
            if (px as u8) < wx {
                continue;
            }
            drew_pixel = true;
            let win_x = (px as u8) - wx;
            let tile_col = (win_x / 8) as u16;
            let tile_map_addr = tile_map_base + tile_row * 32 + tile_col;

            let tile_index = self.read_vram_bank(tile_map_addr, 0);

            let tile_data_addr = if signed_tiles {
                let signed_index = tile_index as i8;
                (0x9000_i32 + (signed_index as i32) * 16) as u16
            } else {
                tile_data_base + (tile_index as u16) * 16
            };

            let fine_y = (win_y % 8) as u16;
            let fine_x = win_x % 8;

            let (vram_bank, palette_num, x_flip, y_flip) = if self.is_cgb {
                let attrs = self.read_vram_bank(tile_map_addr, 1);
                ((attrs >> 3) & 1, attrs & 0x07, attrs & 0x20 != 0, attrs & 0x40 != 0)
            } else {
                (0, 0, false, false)
            };

            let actual_y = if y_flip { 7 - fine_y } else { fine_y };
            let byte1 = self.read_vram_bank(tile_data_addr + actual_y * 2, vram_bank);
            let byte2 = self.read_vram_bank(tile_data_addr + actual_y * 2 + 1, vram_bank);

            let bit = if x_flip { fine_x } else { 7 - fine_x };
            let color_idx = ((byte2 >> bit) & 1) << 1 | ((byte1 >> bit) & 1);

            let color = if self.is_cgb {
                self.cgb_bg_color(palette_num, color_idx)
            } else {
                self.dmg_color(self.bgp, color_idx)
            };

            self.framebuffer[line * SCREEN_W + px] = color;
        }

        if drew_pixel {
            self.window_line += 1;
        }
    }

    fn render_sprites_line(&mut self, line: usize) {
        let tall = self.lcdc & 0x04 != 0;
        let sprite_height: u8 = if tall { 16 } else { 8 };

        let mut sprites_on_line: Vec<(u8, u8, u8, u8)> = Vec::new(); // (x, idx, y, tile)

        for i in 0..40 {
            let base = i * 4;
            let sy = self.oam[base].wrapping_sub(16);
            let sx = self.oam[base + 1].wrapping_sub(8);
            let tile = self.oam[base + 2];
            let flags = self.oam[base + 3];

            if (line as u8) >= sy && (line as u8) < sy.wrapping_add(sprite_height) {
                sprites_on_line.push((sx, i as u8, sy, tile | (flags << 0)));
                // Store flags in last byte - we'll re-read from OAM
                if sprites_on_line.len() >= 10 {
                    break;
                }
            }
        }

        // Draw in reverse order so lower-index sprites have priority
        for &(_, idx, _, _) in sprites_on_line.iter().rev() {
            let base = (idx as usize) * 4;
            let sy = self.oam[base].wrapping_sub(16);
            let sx = self.oam[base + 1].wrapping_sub(8);
            let mut tile = self.oam[base + 2];
            let flags = self.oam[base + 3];

            let y_flip = flags & 0x40 != 0;
            let x_flip = flags & 0x20 != 0;
            let bg_priority = flags & 0x80 != 0;

            if tall {
                tile &= 0xFE;
            }

            let sprite_y = if y_flip {
                (sprite_height - 1) - ((line as u8).wrapping_sub(sy))
            } else {
                (line as u8).wrapping_sub(sy)
            };

            let (vram_bank_s, palette_num_s) = if self.is_cgb {
                ((flags >> 3) & 1, flags & 0x07)
            } else {
                (0, if flags & 0x10 != 0 { 1u8 } else { 0 })
            };

            let tile_addr = 0x8000 + (tile as u16) * 16 + (sprite_y as u16) * 2;
            let byte1 = self.read_vram_bank(tile_addr, vram_bank_s);
            let byte2 = self.read_vram_bank(tile_addr + 1, vram_bank_s);

            for bit in 0..8u8 {
                let screen_x = sx.wrapping_add(bit) as usize;
                if screen_x >= SCREEN_W {
                    continue;
                }

                let actual_bit = if x_flip { bit } else { 7 - bit };
                let color_idx = ((byte2 >> actual_bit) & 1) << 1 | ((byte1 >> actual_bit) & 1);

                if color_idx == 0 {
                    continue; // Transparent
                }

                if bg_priority {
                    // Skip if BG pixel is not color 0
                    // Simplified: just draw anyway for now
                }

                let color = if self.is_cgb {
                    self.cgb_obj_color(palette_num_s, color_idx)
                } else {
                    let pal = if palette_num_s == 1 { self.obp1 } else { self.obp0 };
                    self.dmg_color(pal, color_idx)
                };

                self.framebuffer[line * SCREEN_W + screen_x] = color;
            }
        }
    }

    fn read_vram_bank(&self, addr: u16, bank: u8) -> u8 {
        let offset = (addr - 0x8000) as usize + (bank as usize) * 0x2000;
        if offset < self.vram.len() { self.vram[offset] } else { 0xFF }
    }

    fn dmg_color(&self, palette: u8, idx: u8) -> u32 {
        let shade = (palette >> (idx * 2)) & 0x03;
        match shade {
            0 => 0xFF_E0_F8_D0, // Lightest (greenish white)
            1 => 0xFF_88_C0_70, // Light
            2 => 0xFF_34_68_56, // Dark
            3 => 0xFF_08_18_20, // Darkest
            _ => 0xFF_FF_FF_FF,
        }
    }

    fn cgb_bg_color(&self, palette: u8, idx: u8) -> u32 {
        let offset = (palette as usize) * 8 + (idx as usize) * 2;
        if offset + 1 < self.bgp_data.len() {
            let lo = self.bgp_data[offset] as u16;
            let hi = self.bgp_data[offset + 1] as u16;
            let rgb555 = lo | (hi << 8);
            Self::rgb555_to_argb(rgb555)
        } else {
            0xFF_FF_FF_FF
        }
    }

    fn cgb_obj_color(&self, palette: u8, idx: u8) -> u32 {
        let offset = (palette as usize) * 8 + (idx as usize) * 2;
        if offset + 1 < self.obp_data.len() {
            let lo = self.obp_data[offset] as u16;
            let hi = self.obp_data[offset + 1] as u16;
            let rgb555 = lo | (hi << 8);
            Self::rgb555_to_argb(rgb555)
        } else {
            0xFF_FF_FF_FF
        }
    }

    fn rgb555_to_argb(rgb555: u16) -> u32 {
        let r = ((rgb555 & 0x1F) as u32) * 255 / 31;
        let g = (((rgb555 >> 5) & 0x1F) as u32) * 255 / 31;
        let b = (((rgb555 >> 10) & 0x1F) as u32) * 255 / 31;
        0xFF_00_00_00 | (r << 16) | (g << 8) | b
    }

    pub fn read_vram(&self, addr: u16) -> u8 {
        let offset = (addr - 0x8000) as usize + (self.vram_bank as usize) * 0x2000;
        if offset < self.vram.len() { self.vram[offset] } else { 0xFF }
    }

    pub fn write_vram(&mut self, addr: u16, val: u8) {
        let offset = (addr - 0x8000) as usize + (self.vram_bank as usize) * 0x2000;
        if offset < self.vram.len() {
            self.vram[offset] = val;
        }
    }

    pub fn read_io(&self, addr: u16) -> u8 {
        match addr {
            0xFF40 => self.lcdc,
            0xFF41 => {
                let mode_bits = self.mode as u8;
                let lyc_flag = if self.ly == self.lyc { 0x04 } else { 0 };
                (self.stat & 0xF8) | lyc_flag | mode_bits
            }
            0xFF42 => self.scy,
            0xFF43 => self.scx,
            0xFF44 => self.ly,
            0xFF45 => self.lyc,
            0xFF47 => self.bgp,
            0xFF48 => self.obp0,
            0xFF49 => self.obp1,
            0xFF4A => self.wy,
            0xFF4B => self.wx,
            // CGB palette
            0xFF68 => self.bgp_index | if self.bgp_auto_inc { 0x80 } else { 0 },
            0xFF69 => self.bgp_data[self.bgp_index as usize & 0x3F],
            0xFF6A => self.obp_index | if self.obp_auto_inc { 0x80 } else { 0 },
            0xFF6B => self.obp_data[self.obp_index as usize & 0x3F],
            _ => 0xFF,
        }
    }

    pub fn write_io(&mut self, addr: u16, val: u8) {
        match addr {
            0xFF40 => {
                let was_on = self.lcdc & 0x80 != 0;
                self.lcdc = val;
                if was_on && self.lcdc & 0x80 == 0 {
                    self.ly = 0;
                    self.cycles = 0;
                    self.mode = PpuMode::HBlank;
                }
            }
            0xFF41 => self.stat = (val & 0xF8) | (self.stat & 0x07),
            0xFF42 => self.scy = val,
            0xFF43 => self.scx = val,
            0xFF44 => {} // LY is read-only
            0xFF45 => self.lyc = val,
            0xFF47 => self.bgp = val,
            0xFF48 => self.obp0 = val,
            0xFF49 => self.obp1 = val,
            0xFF4A => self.wy = val,
            0xFF4B => self.wx = val,
            // CGB palette
            0xFF68 => {
                self.bgp_index = val & 0x3F;
                self.bgp_auto_inc = val & 0x80 != 0;
            }
            0xFF69 => {
                self.bgp_data[(self.bgp_index & 0x3F) as usize] = val;
                if self.bgp_auto_inc {
                    self.bgp_index = (self.bgp_index + 1) & 0x3F;
                }
            }
            0xFF6A => {
                self.obp_index = val & 0x3F;
                self.obp_auto_inc = val & 0x80 != 0;
            }
            0xFF6B => {
                self.obp_data[(self.obp_index & 0x3F) as usize] = val;
                if self.obp_auto_inc {
                    self.obp_index = (self.obp_index + 1) & 0x3F;
                }
            }
            _ => {}
        }
    }
}
