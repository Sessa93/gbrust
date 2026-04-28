use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};

use crate::cartridge::{GbaCartridge, GbcCartridge};
use crate::emulator::gba::GbaEmulator;
use crate::emulator::gbc::GbcEmulator;
use crate::emulator::nds::NdsEmulator;
use crate::input::{GbaKey, GbcKey, NdsKey};
use crate::save;
use crate::ConsoleType;

enum Emulator {
    None,
    Gbc(GbcEmulator),
    Gba(GbaEmulator),
    Nds(NdsEmulator),
}

impl Emulator {
    fn is_loaded(&self) -> bool {
        !matches!(self, Self::None)
    }

    fn console_name(&self) -> &'static str {
        match self {
            Self::Gbc(_) => "Game Boy Color",
            Self::Gba(_) => "Game Boy Advance",
            Self::Nds(_) => "Nintendo DS",
            Self::None => "No console",
        }
    }

    fn key_legend(&self) -> &'static str {
        match self {
            Self::Gba(_) => {
                "Z/X = A/B, Enter = Start, Backspace = Select, Arrows = D-Pad, A/S = L/R"
            }
            Self::Nds(_) => {
                "Z/X = A/B, Q/W = X/Y, Enter = Start, Backspace = Select, Arrows = D-Pad, A/S = L/R"
            }
            Self::Gbc(_) | Self::None => {
                "Z/X = A/B, Enter = Start, Backspace = Select, Arrows = D-Pad"
            }
        }
    }

    fn total_frames(&self) -> Option<u64> {
        match self {
            Self::Gbc(emu) => Some(emu.total_frames),
            Self::Gba(emu) => Some(emu.total_frames),
            Self::Nds(emu) => Some(emu.total_frames),
            Self::None => None,
        }
    }

    fn run_frame(&mut self) -> Option<(Vec<u32>, Vec<f32>)> {
        match self {
            Self::Gbc(emu) => Some((emu.run_frame().to_vec(), emu.audio_buffer())),
            Self::Gba(emu) => Some((emu.run_frame().to_vec(), emu.audio_buffer())),
            Self::Nds(emu) => Some((emu.run_frame().to_vec(), emu.audio_buffer())),
            Self::None => None,
        }
    }

    fn screen_dimensions(&self) -> Option<(usize, usize)> {
        match self {
            Self::Gbc(emu) => Some((emu.screen_width() as usize, emu.screen_height() as usize)),
            Self::Gba(emu) => Some((emu.screen_width() as usize, emu.screen_height() as usize)),
            Self::Nds(emu) => Some((emu.screen_width() as usize, emu.screen_height() as usize)),
            Self::None => None,
        }
    }

    fn apply_key_map<K: Copy>(
        input: &egui::InputState,
        map: &[(egui::Key, K)],
        mut apply: impl FnMut(K, bool),
    ) {
        for &(egui_key, emu_key) in map {
            if input.key_pressed(egui_key) {
                apply(emu_key, true);
            }
            if input.key_released(egui_key) {
                apply(emu_key, false);
            }
        }
    }

    fn handle_input(&mut self, input: &egui::InputState) {
        match self {
            Self::Gbc(emu) => {
                Self::apply_key_map(input, &GBC_KEY_MAP, |key, pressed| {
                    if pressed {
                        emu.bus.input.key_down(key);
                    } else {
                        emu.bus.input.key_up(key);
                    }
                });
            }
            Self::Gba(emu) => {
                Self::apply_key_map(input, &GBA_KEY_MAP, |key, pressed| {
                    if pressed {
                        emu.bus.input.key_down(key);
                    } else {
                        emu.bus.input.key_up(key);
                    }
                });
            }
            Self::Nds(emu) => {
                Self::apply_key_map(input, &NDS_KEY_MAP, |key, pressed| {
                    emu.bus.set_key(key, pressed);
                });
            }
            Self::None => {}
        }
    }

    fn save_backup(&self, path: &Path) {
        match self {
            Self::Gbc(emu) => save::save_gbc_sram(path, emu),
            Self::Gba(emu) => save::save_gba_backup(path, emu),
            Self::Nds(_) => {}
            Self::None => {}
        }
    }

    fn save_state(&self, path: &Path, slot: u8) -> Result<(), String> {
        match self {
            Self::Gbc(emu) => save::save_gbc_state(path, slot, emu),
            Self::Gba(emu) => save::save_gba_state(path, slot, emu),
            Self::Nds(emu) => save::save_nds_state(path, slot, emu),
            Self::None => Ok(()),
        }
    }

    fn load_state(&self, path: &Path, slot: u8) -> Result<Option<Self>, String> {
        match self {
            Self::Gbc(_) => save::load_gbc_state(path, slot).map(Self::Gbc).map(Some),
            Self::Gba(_) => save::load_gba_state(path, slot).map(Self::Gba).map(Some),
            Self::Nds(_) => save::load_nds_state(path, slot).map(Self::Nds).map(Some),
            Self::None => Ok(None),
        }
    }

    fn memory_regions(&self) -> &'static [DebugMemoryRegion] {
        match self {
            Self::Gbc(_) => &GBC_DEBUG_REGIONS,
            Self::Gba(_) => &GBA_DEBUG_REGIONS,
            Self::Nds(_) => &NDS_DEBUG_REGIONS,
            Self::None => &[],
        }
    }

    fn memory_view(&self, region: DebugMemoryRegion) -> Option<MemoryRegionView<'_>> {
        match (self, region) {
            (Self::Gbc(emu), DebugMemoryRegion::GbcWram) => Some(MemoryRegionView {
                label: region.label(),
                base_address: 0x0000_C000,
                bytes: &emu.bus.wram,
            }),
            (Self::Gbc(emu), DebugMemoryRegion::GbcVram) => Some(MemoryRegionView {
                label: region.label(),
                base_address: 0x0000_8000,
                bytes: &emu.bus.ppu.vram,
            }),
            (Self::Gbc(emu), DebugMemoryRegion::GbcHram) => Some(MemoryRegionView {
                label: region.label(),
                base_address: 0x0000_FF80,
                bytes: &emu.bus.hram,
            }),
            (Self::Gbc(emu), DebugMemoryRegion::GbcOam) => Some(MemoryRegionView {
                label: region.label(),
                base_address: 0x0000_FE00,
                bytes: &emu.bus.ppu.oam,
            }),
            (Self::Gba(emu), DebugMemoryRegion::GbaEwram) => Some(MemoryRegionView {
                label: region.label(),
                base_address: 0x0200_0000,
                bytes: &emu.bus.ewram,
            }),
            (Self::Gba(emu), DebugMemoryRegion::GbaIwram) => Some(MemoryRegionView {
                label: region.label(),
                base_address: 0x0300_0000,
                bytes: &emu.bus.iwram,
            }),
            (Self::Gba(emu), DebugMemoryRegion::GbaVram) => Some(MemoryRegionView {
                label: region.label(),
                base_address: 0x0600_0000,
                bytes: &emu.bus.ppu.vram,
            }),
            (Self::Gba(emu), DebugMemoryRegion::GbaPalette) => Some(MemoryRegionView {
                label: region.label(),
                base_address: 0x0500_0000,
                bytes: &emu.bus.ppu.palette,
            }),
            (Self::Gba(emu), DebugMemoryRegion::GbaOam) => Some(MemoryRegionView {
                label: region.label(),
                base_address: 0x0700_0000,
                bytes: &emu.bus.ppu.oam,
            }),
            (Self::Nds(emu), DebugMemoryRegion::NdsMainRam) => Some(MemoryRegionView {
                label: region.label(),
                base_address: 0x0200_0000,
                bytes: &emu.bus.memory.main_ram,
            }),
            (Self::Nds(emu), DebugMemoryRegion::NdsSharedWram) => Some(MemoryRegionView {
                label: region.label(),
                base_address: 0x0300_0000,
                bytes: &emu.bus.memory.shared_wram,
            }),
            (Self::Nds(emu), DebugMemoryRegion::NdsArm7Wram) => Some(MemoryRegionView {
                label: region.label(),
                base_address: 0x0380_0000,
                bytes: &emu.bus.memory.arm7_wram,
            }),
            (Self::Nds(emu), DebugMemoryRegion::NdsVram) => Some(MemoryRegionView {
                label: region.label(),
                base_address: 0x0600_0000,
                bytes: &emu.bus.memory.vram,
            }),
            (Self::Nds(emu), DebugMemoryRegion::NdsPalette) => Some(MemoryRegionView {
                label: region.label(),
                base_address: 0x0500_0000,
                bytes: &emu.bus.memory.palette,
            }),
            (Self::Nds(emu), DebugMemoryRegion::NdsOam) => Some(MemoryRegionView {
                label: region.label(),
                base_address: 0x0700_0000,
                bytes: &emu.bus.memory.oam,
            }),
            _ => None,
        }
    }
}

const FRAME_DURATION: Duration = Duration::from_nanos(16_742_706);
const MAX_AUDIO_BUFFER_SAMPLES: usize = 8192;
const MAX_CATCH_UP_FRAMES: u32 = 4;
const SIDE_PANEL_WIDTH: f32 = 220.0;
const BUTTON_PRESS_IN_SPEED: f32 = 15.0;
const BUTTON_PRESS_OUT_SPEED: f32 = 11.0;
const BUTTON_OVERLAY_MAX_ALPHA: f32 = 50.0;
const BUTTON_OVERLAY_TINT: u8 = 100;
const MEMORY_BYTES_PER_ROW: usize = 16;
const GBC_KEY_MAP: [(egui::Key, GbcKey); 8] = [
    (egui::Key::Z, GbcKey::A),
    (egui::Key::X, GbcKey::B),
    (egui::Key::Enter, GbcKey::Start),
    (egui::Key::Backspace, GbcKey::Select),
    (egui::Key::ArrowUp, GbcKey::Up),
    (egui::Key::ArrowDown, GbcKey::Down),
    (egui::Key::ArrowLeft, GbcKey::Left),
    (egui::Key::ArrowRight, GbcKey::Right),
];
const GBA_KEY_MAP: [(egui::Key, GbaKey); 10] = [
    (egui::Key::Z, GbaKey::A),
    (egui::Key::X, GbaKey::B),
    (egui::Key::Enter, GbaKey::Start),
    (egui::Key::Backspace, GbaKey::Select),
    (egui::Key::ArrowUp, GbaKey::Up),
    (egui::Key::ArrowDown, GbaKey::Down),
    (egui::Key::ArrowLeft, GbaKey::Left),
    (egui::Key::ArrowRight, GbaKey::Right),
    (egui::Key::A, GbaKey::L),
    (egui::Key::S, GbaKey::R),
];
const NDS_KEY_MAP: [(egui::Key, NdsKey); 12] = [
    (egui::Key::Z, NdsKey::A),
    (egui::Key::X, NdsKey::B),
    (egui::Key::Enter, NdsKey::Start),
    (egui::Key::Backspace, NdsKey::Select),
    (egui::Key::ArrowUp, NdsKey::Up),
    (egui::Key::ArrowDown, NdsKey::Down),
    (egui::Key::ArrowLeft, NdsKey::Left),
    (egui::Key::ArrowRight, NdsKey::Right),
    (egui::Key::A, NdsKey::L),
    (egui::Key::S, NdsKey::R),
    (egui::Key::Q, NdsKey::X),
    (egui::Key::W, NdsKey::Y),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum BackgroundStyle {
    Gb,
    Gbc,
    Gba,
    Nds,
}

impl BackgroundStyle {
    fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "gb" => Some(Self::Gb),
            "gbc" => Some(Self::Gbc),
            "gba" => Some(Self::Gba),
            "nds" => Some(Self::Nds),
            _ => None,
        }
    }

    fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|extension| extension.to_str())
            .and_then(Self::from_extension)
    }

    fn texture_name(self) -> &'static str {
        match self {
            Self::Gb => "gb-overlay-background",
            Self::Gbc => "gbc-overlay-background",
            Self::Gba => "gba-overlay-background",
            Self::Nds => "nds-overlay-background",
        }
    }

    fn image_bytes(self) -> &'static [u8] {
        match self {
            Self::Gb => include_bytes!("../resources/gb/background.jpeg"),
            Self::Gbc => include_bytes!("../resources/gbc/background.jpeg"),
            Self::Gba => include_bytes!("../resources/gba/background.png"),
            Self::Nds => &[],
        }
    }

    fn image_size(self) -> egui::Vec2 {
        match self {
            Self::Gb | Self::Gbc => egui::vec2(3840.0, 2160.0),
            Self::Gba => egui::vec2(1920.0, 1080.0),
            Self::Nds => egui::vec2(1024.0, 1680.0),
        }
    }

    fn screen_min(self) -> egui::Vec2 {
        match self {
            Self::Gb => egui::vec2(1440.0, 432.0),
            Self::Gbc => egui::vec2(1440.0, 405.0),
            Self::Gba => egui::vec2(551.0, 242.0),
            Self::Nds => egui::vec2(128.0, 264.0),
        }
    }

    fn screen_size(self) -> egui::Vec2 {
        match self {
            Self::Gb => egui::vec2(960.0, 864.0),
            Self::Gbc => egui::vec2(964.0, 862.0),
            Self::Gba => egui::vec2(813.0, 542.0),
            Self::Nds => egui::vec2(768.0, 1152.0),
        }
    }

    fn shows_button_overlays(self) -> bool {
        matches!(self, Self::Gba)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DebugMemoryRegion {
    GbcWram,
    GbcVram,
    GbcHram,
    GbcOam,
    GbaEwram,
    GbaIwram,
    GbaVram,
    GbaPalette,
    GbaOam,
    NdsMainRam,
    NdsSharedWram,
    NdsArm7Wram,
    NdsVram,
    NdsPalette,
    NdsOam,
}

impl DebugMemoryRegion {
    fn label(self) -> &'static str {
        match self {
            Self::GbcWram => "GBC WRAM",
            Self::GbcVram => "GBC VRAM",
            Self::GbcHram => "GBC HRAM",
            Self::GbcOam => "GBC OAM",
            Self::GbaEwram => "GBA EWRAM",
            Self::GbaIwram => "GBA IWRAM",
            Self::GbaVram => "GBA VRAM",
            Self::GbaPalette => "GBA Palette",
            Self::GbaOam => "GBA OAM",
            Self::NdsMainRam => "NDS Main RAM",
            Self::NdsSharedWram => "NDS Shared WRAM",
            Self::NdsArm7Wram => "NDS ARM7 WRAM",
            Self::NdsVram => "NDS VRAM",
            Self::NdsPalette => "NDS Palette",
            Self::NdsOam => "NDS OAM",
        }
    }
}

struct MemoryRegionView<'a> {
    label: &'static str,
    base_address: usize,
    bytes: &'a [u8],
}

const GBC_DEBUG_REGIONS: [DebugMemoryRegion; 4] = [
    DebugMemoryRegion::GbcWram,
    DebugMemoryRegion::GbcVram,
    DebugMemoryRegion::GbcHram,
    DebugMemoryRegion::GbcOam,
];

const GBA_DEBUG_REGIONS: [DebugMemoryRegion; 5] = [
    DebugMemoryRegion::GbaEwram,
    DebugMemoryRegion::GbaIwram,
    DebugMemoryRegion::GbaVram,
    DebugMemoryRegion::GbaPalette,
    DebugMemoryRegion::GbaOam,
];

const NDS_DEBUG_REGIONS: [DebugMemoryRegion; 6] = [
    DebugMemoryRegion::NdsMainRam,
    DebugMemoryRegion::NdsSharedWram,
    DebugMemoryRegion::NdsArm7Wram,
    DebugMemoryRegion::NdsVram,
    DebugMemoryRegion::NdsPalette,
    DebugMemoryRegion::NdsOam,
];

#[derive(Clone, Copy)]
struct DisplaySettings {
    gamma: f32,
    saturation: f32,
    red_gain: f32,
    green_gain: f32,
    blue_gain: f32,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            gamma: 0.6,
            saturation: 1.5,
            red_gain: 1.10,
            green_gain: 1.05,
            blue_gain: 1.0,
        }
    }
}

impl DisplaySettings {
    fn apply(self, argb: u32) -> Color32 {
        let mut r = ((argb >> 16) & 0xFF) as f32 / 255.0;
        let mut g = ((argb >> 8) & 0xFF) as f32 / 255.0;
        let mut b = (argb & 0xFF) as f32 / 255.0;

        r = (r * self.red_gain).clamp(0.0, 1.0);
        g = (g * self.green_gain).clamp(0.0, 1.0);
        b = (b * self.blue_gain).clamp(0.0, 1.0);

        let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        r = (luma + (r - luma) * self.saturation).clamp(0.0, 1.0);
        g = (luma + (g - luma) * self.saturation).clamp(0.0, 1.0);
        b = (luma + (b - luma) * self.saturation).clamp(0.0, 1.0);

        let gamma_curve = 1.0 / self.gamma.max(0.01);
        r = r.powf(gamma_curve);
        g = g.powf(gamma_curve);
        b = b.powf(gamma_curve);

        Color32::from_rgb(
            (r * 255.0 + 0.5) as u8,
            (g * 255.0 + 0.5) as u8,
            (b * 255.0 + 0.5) as u8,
        )
    }

    fn slider(
        ui: &mut egui::Ui,
        value: &mut f32,
        range: std::ops::RangeInclusive<f32>,
        label: &str,
    ) -> bool {
        ui.add(egui::Slider::new(value, range).text(label)).changed()
    }

    fn draw_controls(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        changed |= Self::slider(ui, &mut self.gamma, 0.60..=2.40, "Gamma");
        changed |= Self::slider(ui, &mut self.saturation, 0.0..=2.0, "Saturation");
        changed |= Self::slider(ui, &mut self.red_gain, 0.5..=1.5, "Red");
        changed |= Self::slider(ui, &mut self.green_gain, 0.5..=1.5, "Green");
        changed |= Self::slider(ui, &mut self.blue_gain, 0.5..=1.5, "Blue");
        changed
    }
}

#[derive(Clone, Copy)]
#[repr(usize)]
enum OverlayButton {
    A = 0,
    B,
    L,
    R,
    Start,
    Select,
    Up,
    Down,
    Left,
    Right,
}

impl OverlayButton {
    const COUNT: usize = 10;
    const ALL: [Self; Self::COUNT] = [
        Self::A,
        Self::B,
        Self::L,
        Self::R,
        Self::Start,
        Self::Select,
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
    ];

    fn texture_name(self) -> &'static str {
        match self {
            Self::A => "button-a-overlay",
            Self::B => "button-b-overlay",
            Self::L => "button-l-overlay",
            Self::R => "button-r-overlay",
            Self::Start => "button-start-overlay",
            Self::Select => "button-select-overlay",
            Self::Up => "button-up-overlay",
            Self::Down => "button-down-overlay",
            Self::Left => "button-left-overlay",
            Self::Right => "button-right-overlay",
        }
    }

    fn image_bytes(self) -> &'static [u8] {
        match self {
            Self::A => include_bytes!("../resources/gba/a.png"),
            Self::B => include_bytes!("../resources/gba/b.png"),
            Self::L => include_bytes!("../resources/gba/l.png"),
            Self::R => include_bytes!("../resources/gba/r.png"),
            Self::Start => include_bytes!("../resources/gba/start.png"),
            Self::Select => include_bytes!("../resources/gba/select.png"),
            Self::Up => include_bytes!("../resources/gba/up.png"),
            Self::Down => include_bytes!("../resources/gba/down.png"),
            Self::Left => include_bytes!("../resources/gba/left.png"),
            Self::Right => include_bytes!("../resources/gba/right.png"),
        }
    }

    fn base_min(self) -> egui::Vec2 {
        match self {
            Self::A => egui::vec2(1707.0, 372.0),
            Self::B => egui::vec2(1524.0, 435.0),
            Self::L => egui::vec2(-18.0, -4.0),
            Self::R => egui::vec2(1458.0, -4.0),
            Self::Start => egui::vec2(316.0, 728.0),
            Self::Select => egui::vec2(315.0, 841.0),
            Self::Up => egui::vec2(125.0, 340.0),
            Self::Down => egui::vec2(160.0, 545.0),
            Self::Left => egui::vec2(62.0, 410.0),
            Self::Right => egui::vec2(253.0, 428.0),
        }
    }

    fn press_offset(self) -> egui::Vec2 {
        match self {
            Self::A | Self::B | Self::Start | Self::Select => egui::vec2(4.0, 4.0),
            Self::L | Self::R => egui::vec2(0.0, 6.0),
            Self::Up => egui::vec2(0.0, 4.0),
            Self::Down => egui::vec2(0.0, -4.0),
            Self::Left => egui::vec2(4.0, 0.0),
            Self::Right => egui::vec2(-4.0, 0.0),
        }
    }

    fn pressed_scale(self) -> f32 {
        match self {
            Self::L | Self::R => 0.985,
            Self::Up | Self::Down | Self::Left | Self::Right => 0.97,
            Self::A | Self::B | Self::Start | Self::Select => 0.96,
        }
    }

    fn key(self) -> egui::Key {
        match self {
            Self::A => egui::Key::Z,
            Self::B => egui::Key::X,
            Self::L => egui::Key::A,
            Self::R => egui::Key::S,
            Self::Start => egui::Key::Enter,
            Self::Select => egui::Key::Backspace,
            Self::Up => egui::Key::ArrowUp,
            Self::Down => egui::Key::ArrowDown,
            Self::Left => egui::Key::ArrowLeft,
            Self::Right => egui::Key::ArrowRight,
        }
    }
}

struct ButtonOverlay {
    texture: egui::TextureHandle,
    base_min: egui::Vec2,
    size: egui::Vec2,
    press_offset: egui::Vec2,
    pressed_scale: f32,
}

impl ButtonOverlay {
    fn load(ctx: &egui::Context, button: OverlayButton) -> Self {
        let image = image::load_from_memory(button.image_bytes())
            .expect("button overlay should be a valid PNG")
            .to_rgba8();
        let size = egui::vec2(image.width() as f32, image.height() as f32);
        let color_image = egui::ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.as_raw(),
        );

        Self {
            texture: ctx.load_texture(
                button.texture_name(),
                color_image,
                egui::TextureOptions::LINEAR,
            ),
            base_min: button.base_min(),
            size,
            press_offset: button.press_offset(),
            pressed_scale: button.pressed_scale(),
        }
    }
}

struct ButtonOverlays {
    overlays: [ButtonOverlay; OverlayButton::COUNT],
}

impl ButtonOverlays {
    fn load(ctx: &egui::Context) -> Self {
        Self {
            overlays: std::array::from_fn(|index| {
                ButtonOverlay::load(ctx, OverlayButton::ALL[index])
            }),
        }
    }

    fn get(&self, button: OverlayButton) -> &ButtonOverlay {
        &self.overlays[button as usize]
    }
}

pub struct EmuApp {
    emu: Emulator,
    rom_path: Option<PathBuf>,
    background_style: BackgroundStyle,
    background_texture: egui::TextureHandle,
    button_overlays: ButtonOverlays,
    texture: Option<egui::TextureHandle>,
    last_framebuffer: Option<Vec<u32>>,
    screen_width: usize,
    screen_height: usize,
    audio_stream: Option<AudioStream>,
    volume: f32,
    display_settings: DisplaySettings,
    save_slot: u8,
    status_msg: String,
    paused: bool,
    last_frame_time: Instant,
    frame_accumulator: Duration,
    fps_sample_time: Duration,
    fps_sample_frames: u32,
    displayed_fps: f32,
    show_registers_window: bool,
    show_memory_window: bool,
    selected_memory_region: Option<DebugMemoryRegion>,
    button_animation: [f32; OverlayButton::COUNT],
}

struct AudioStream {
    _stream: cpal::Stream,
    buffer: Arc<Mutex<VecDeque<f32>>>,
    volume: Arc<Mutex<f32>>,
}

unsafe impl Send for AudioStream {}
unsafe impl Sync for AudioStream {}

impl EmuApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(18, 20, 24);
        visuals.extreme_bg_color = Color32::from_rgb(10, 12, 16);
        visuals.faint_bg_color = Color32::from_rgb(28, 32, 38);
        visuals.selection.bg_fill = Color32::from_rgb(72, 98, 148);
        cc.egui_ctx.set_visuals(visuals);

        let mut style = (*cc.egui_ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
        cc.egui_ctx.set_style(style);

        let background_style = BackgroundStyle::Gba;
        let background_texture = Self::load_background_texture(&cc.egui_ctx, background_style);
        let button_overlays = ButtonOverlays::load(&cc.egui_ctx);

        Self {
            emu: Emulator::None,
            rom_path: None,
            background_style,
            background_texture,
            button_overlays,
            texture: None,
            last_framebuffer: None,
            screen_width: 240,
            screen_height: 160,
            audio_stream: None,
            volume: 0.15,
            display_settings: DisplaySettings::default(),
            save_slot: 1,
            status_msg: "Open a ROM to start playing.".to_string(),
            paused: false,
            last_frame_time: Instant::now(),
            frame_accumulator: Duration::ZERO,
            fps_sample_time: Duration::ZERO,
            fps_sample_frames: 0,
            displayed_fps: 0.0,
            show_registers_window: false,
            show_memory_window: false,
            selected_memory_region: None,
            button_animation: [0.0; OverlayButton::COUNT],
        }
    }

    fn load_background_texture(
        ctx: &egui::Context,
        background_style: BackgroundStyle,
    ) -> egui::TextureHandle {
        let color_image = match background_style {
            BackgroundStyle::Nds => Self::build_nds_background_image(),
            _ => {
                let image = image::load_from_memory(background_style.image_bytes())
                    .expect("background image should be a valid texture")
                    .to_rgba8();
                let size = [image.width() as usize, image.height() as usize];
                egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw())
            }
        };

        ctx.load_texture(
            background_style.texture_name(),
            color_image,
            egui::TextureOptions::LINEAR,
        )
    }

    fn build_nds_background_image() -> egui::ColorImage {
        let width = 1024usize;
        let height = 1680usize;
        let mut pixels = vec![Color32::from_rgb(12, 14, 18); width * height];

        for y in 0..height {
            let t = y as f32 / (height.saturating_sub(1)) as f32;
            let r = (18.0 + 18.0 * (1.0 - t)) as u8;
            let g = (22.0 + 20.0 * (1.0 - t)) as u8;
            let b = (28.0 + 28.0 * (1.0 - t)) as u8;
            let row_color = Color32::from_rgb(r, g, b);

            for x in 0..width {
                pixels[y * width + x] = row_color;
            }
        }

        let shell_min_x = 80usize;
        let shell_max_x = width - 80;
        let shell_min_y = 96usize;
        let shell_max_y = height - 96;
        for y in shell_min_y..shell_max_y {
            for x in shell_min_x..shell_max_x {
                pixels[y * width + x] = Color32::from_rgb(32, 36, 44);
            }
        }

        let hinge_min_y = 776usize;
        let hinge_max_y = 904usize;
        for y in hinge_min_y..hinge_max_y {
            for x in shell_min_x + 56..shell_max_x - 56 {
                pixels[y * width + x] = Color32::from_rgb(40, 44, 54);
            }
        }

        egui::ColorImage {
            size: [width, height],
            pixels,
        }
    }

    fn apply_background_style(&mut self, ctx: &egui::Context, background_style: BackgroundStyle) {
        if self.background_style == background_style {
            return;
        }

        self.background_style = background_style;
        self.background_texture = Self::load_background_texture(ctx, background_style);

        if !background_style.shows_button_overlays() {
            self.button_animation.fill(0.0);
        }
    }

    fn format_memory_line(base_address: usize, bytes: &[u8]) -> String {
        let mut hex = String::new();
        let mut ascii = String::new();

        for &byte in bytes {
            if !hex.is_empty() {
                hex.push(' ');
            }
            hex.push_str(&format!("{:02X}", byte));
            ascii.push(if byte.is_ascii_graphic() || byte == b' ' {
                byte as char
            } else {
                '.'
            });
        }

        format!("{:08X}  {:<47}  {}", base_address, hex, ascii)
    }

    fn draw_gbc_registers(ui: &mut egui::Ui, emu: &GbcEmulator) {
        let cpu = &emu.cpu;
        let af = ((cpu.a as u16) << 8) | ((cpu.f & 0xF0) as u16);
        let bc = ((cpu.b as u16) << 8) | cpu.c as u16;
        let de = ((cpu.d as u16) << 8) | cpu.e as u16;
        let hl = ((cpu.h as u16) << 8) | cpu.l as u16;

        egui::Grid::new("gbc_register_grid")
            .num_columns(2)
            .spacing([16.0, 6.0])
            .striped(true)
            .show(ui, |ui| {
                ui.monospace(format!("A  {:02X}", cpu.a));
                ui.monospace(format!("F  {:02X}", cpu.f));
                ui.end_row();
                ui.monospace(format!("B  {:02X}", cpu.b));
                ui.monospace(format!("C  {:02X}", cpu.c));
                ui.end_row();
                ui.monospace(format!("D  {:02X}", cpu.d));
                ui.monospace(format!("E  {:02X}", cpu.e));
                ui.end_row();
                ui.monospace(format!("H  {:02X}", cpu.h));
                ui.monospace(format!("L  {:02X}", cpu.l));
                ui.end_row();
                ui.monospace(format!("AF {:04X}", af));
                ui.monospace(format!("BC {:04X}", bc));
                ui.end_row();
                ui.monospace(format!("DE {:04X}", de));
                ui.monospace(format!("HL {:04X}", hl));
                ui.end_row();
                ui.monospace(format!("SP {:04X}", cpu.sp));
                ui.monospace(format!("PC {:04X}", cpu.pc));
                ui.end_row();
            });

        ui.separator();
        ui.label(format!(
            "Flags: Z={} N={} H={} C={} | IME={} Pending={} Halted={} Stopped={} | Cycles={}",
            cpu.f & 0x80 != 0,
            cpu.f & 0x40 != 0,
            cpu.f & 0x20 != 0,
            cpu.f & 0x10 != 0,
            cpu.ime,
            cpu.ime_pending,
            cpu.halted,
            cpu.stopped,
            cpu.cycles
        ));
    }

    fn draw_gba_registers(ui: &mut egui::Ui, emu: &GbaEmulator) {
        let cpu = &emu.cpu;

        egui::Grid::new("gba_register_grid")
            .num_columns(4)
            .spacing([16.0, 6.0])
            .striped(true)
            .show(ui, |ui| {
                for row in 0..4 {
                    for col in 0..4 {
                        let index = row * 4 + col;
                        ui.monospace(format!("R{:02} {:08X}", index, cpu.regs[index]));
                    }
                    ui.end_row();
                }
            });

        ui.separator();
        ui.label(format!(
            "CPSR {:08X} | Mode={:?} | N={} Z={} C={} V={} I={} T={} | Halted={} | Cycles={}",
            cpu.cpsr,
            crate::cpu::arm7tdmi::CpuMode::from_bits(cpu.cpsr),
            cpu.cpsr & (1 << 31) != 0,
            cpu.cpsr & (1 << 30) != 0,
            cpu.cpsr & (1 << 29) != 0,
            cpu.cpsr & (1 << 28) != 0,
            cpu.cpsr & (1 << 7) != 0,
            cpu.cpsr & (1 << 5) != 0,
            cpu.halted,
            cpu.cycles,
        ));
    }

    fn draw_nds_registers(ui: &mut egui::Ui, emu: &NdsEmulator) {
        ui.label(format!(
            "{} | Code={} | Maker={}",
            emu.header.display_title(),
            emu.header.game_code,
            emu.header.maker_code
        ));
        ui.label(format!(
            "ARM9 entry=0x{:08X} load=0x{:08X} size=0x{:X} | ARM7 entry=0x{:08X} load=0x{:08X} size=0x{:X}",
            emu.header.arm9_entry_address,
            emu.header.arm9_ram_address,
            emu.header.arm9_size,
            emu.header.arm7_entry_address,
            emu.header.arm7_ram_address,
            emu.header.arm7_size,
        ));
        ui.label(format!(
            "KEYINPUT=0x{:04X} EXTKEYIN=0x{:04X} Touch=({}, {}) pressed={}",
            emu.bus.input.read_keyinput(),
            emu.bus.input.read_extkeyin(),
            emu.bus.input.touchscreen_x,
            emu.bus.input.touchscreen_y,
            emu.bus.input.touchscreen_pressed,
        ));
        ui.label(format!(
            "ARM7 IME={} IE=0x{:08X} IF=0x{:08X} POSTFLG=0x{:02X} HALT={} Bus cycles={}",
            emu.bus.ime,
            emu.bus.ie,
            emu.bus.iflag,
            emu.bus.postflg,
            emu.bus.halt,
            emu.bus.cycles,
        ));
        ui.label(format!(
            "IPC queues ARM7={} ARM9={} | DMA active ARM7={} ARM9={}",
            emu.bus.arm7_ipc_depth(),
            emu.bus.arm9_ipc_depth(),
            emu.bus.arm7_dma_active_count(),
            emu.bus.arm9_dma_active_count(),
        ));
        ui.label(format!(
            "VCOUNT={} ARM7 DISPSTAT=0x{:04X} ARM9 DISPSTAT=0x{:04X}",
            emu.bus.video.vcount,
            emu.bus.read_dispstat_value(),
            emu.bus.read_arm9_dispstat_value(),
        ));
        ui.label(format!(
            "PPU A DISPCNT=0x{:08X} BG0CNT=0x{:04X} BRIGHT=0x{:04X} | PPU B DISPCNT=0x{:08X} BG0CNT=0x{:04X} BRIGHT=0x{:04X}",
            emu.bus.ppu_main.dispcnt,
            emu.bus.ppu_main.bgcnt[0],
            emu.bus.ppu_main.master_bright,
            emu.bus.ppu_sub.dispcnt,
            emu.bus.ppu_sub.bgcnt[0],
            emu.bus.ppu_sub.master_bright,
        ));

        ui.separator();
        ui.label(RichText::new("ARM9").strong());
        egui::Grid::new("nds_arm9_register_grid")
            .num_columns(4)
            .spacing([16.0, 6.0])
            .striped(true)
            .show(ui, |ui| {
                for row in 0..4 {
                    for col in 0..4 {
                        let index = row * 4 + col;
                        ui.monospace(format!("R{:02} {:08X}", index, emu.arm9.regs[index]));
                    }
                    ui.end_row();
                }
            });
        ui.label(format!(
            "CPSR {:08X} | Mode={:?} | Halted={} | Cycles={}",
            emu.arm9.cpsr,
            crate::cpu::arm7tdmi::CpuMode::from_bits(emu.arm9.cpsr),
            emu.arm9.halted,
            emu.arm9.cycles,
        ));
        ui.label(format!(
            "ARM9 IME={} IE=0x{:08X} IF=0x{:08X} POSTFLG=0x{:02X} HALT={} Bus cycles={}",
            emu.bus.arm9_ime,
            emu.bus.arm9_ie,
            emu.bus.arm9_iflag,
            emu.bus.arm9_postflg,
            emu.bus.arm9_halt,
            emu.bus.arm9_cycles,
        ));

        ui.separator();
        ui.label(RichText::new("ARM7").strong());
        egui::Grid::new("nds_arm7_register_grid")
            .num_columns(4)
            .spacing([16.0, 6.0])
            .striped(true)
            .show(ui, |ui| {
                for row in 0..4 {
                    for col in 0..4 {
                        let index = row * 4 + col;
                        ui.monospace(format!("R{:02} {:08X}", index, emu.arm7.regs[index]));
                    }
                    ui.end_row();
                }
            });
        ui.label(format!(
            "CPSR {:08X} | Mode={:?} | Halted={} | Cycles={}",
            emu.arm7.cpsr,
            crate::cpu::arm7tdmi::CpuMode::from_bits(emu.arm7.cpsr),
            emu.arm7.halted,
            emu.arm7.cycles,
        ));
    }

    fn draw_registers_contents(&mut self, ui: &mut egui::Ui) {
        match &self.emu {
            Emulator::Gbc(emu) => Self::draw_gbc_registers(ui, emu),
            Emulator::Gba(emu) => Self::draw_gba_registers(ui, emu),
            Emulator::Nds(emu) => Self::draw_nds_registers(ui, emu),
            Emulator::None => {
                ui.label("Load a ROM to inspect register state.");
            }
        }
    }

    fn draw_memory_inspector_contents(&mut self, ui: &mut egui::Ui) {
        let regions = self.emu.memory_regions();
        if regions.is_empty() {
            ui.label("Load a ROM to inspect RAM or VRAM.");
            return;
        }

        let active_region = self
            .selected_memory_region
            .filter(|region| regions.contains(region))
            .unwrap_or(regions[0]);
        self.selected_memory_region = Some(active_region);

        egui::ComboBox::from_label("Region")
            .selected_text(active_region.label())
            .show_ui(ui, |ui| {
                for &region in regions {
                    ui.selectable_value(&mut self.selected_memory_region, Some(region), region.label());
                }
            });

        let active_region = self.selected_memory_region.unwrap_or(regions[0]);
        let Some(view) = self.emu.memory_view(active_region) else {
            ui.label("Selected region is unavailable for the current console.");
            return;
        };

        ui.label(format!(
            "{} | Base 0x{:08X} | {} bytes",
            view.label,
            view.base_address,
            view.bytes.len()
        ));
        ui.separator();

        let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
        let row_count = (view.bytes.len() + MEMORY_BYTES_PER_ROW - 1) / MEMORY_BYTES_PER_ROW;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, row_height, row_count, |ui, row_range| {
                for row in row_range {
                    let start = row * MEMORY_BYTES_PER_ROW;
                    let end = (start + MEMORY_BYTES_PER_ROW).min(view.bytes.len());
                    ui.monospace(Self::format_memory_line(
                        view.base_address + start,
                        &view.bytes[start..end],
                    ));
                }
            });
    }

    fn draw_registers_window(&mut self, ctx: &egui::Context) {
        if !self.show_registers_window {
            return;
        }

        let viewport_id = egui::ViewportId::from_hash_of("gbrust-registers-window");
        let builder = egui::ViewportBuilder::default()
            .with_title("Registers")
            .with_inner_size([420.0, 320.0])
            .with_min_inner_size([360.0, 240.0]);

        ctx.show_viewport_immediate(viewport_id, builder, |ctx, class| {
            if ctx.input(|input| input.viewport().close_requested()) {
                self.show_registers_window = false;
            }

            if matches!(class, egui::ViewportClass::Embedded) {
                let mut open = self.show_registers_window;
                egui::Window::new("Registers")
                    .open(&mut open)
                    .resizable(true)
                    .vscroll(true)
                    .default_width(420.0)
                    .show(ctx, |ui| self.draw_registers_contents(ui));
                self.show_registers_window = open;
            } else {
                egui::CentralPanel::default().show(ctx, |ui| {
                    self.draw_registers_contents(ui);
                });
            }
        });
    }

    fn draw_memory_inspector_window(&mut self, ctx: &egui::Context) {
        if !self.show_memory_window {
            return;
        }

        let viewport_id = egui::ViewportId::from_hash_of("gbrust-memory-window");
        let builder = egui::ViewportBuilder::default()
            .with_title("Memory Inspector")
            .with_inner_size([640.0, 420.0])
            .with_min_inner_size([520.0, 320.0]);

        ctx.show_viewport_immediate(viewport_id, builder, |ctx, class| {
            if ctx.input(|input| input.viewport().close_requested()) {
                self.show_memory_window = false;
            }

            if matches!(class, egui::ViewportClass::Embedded) {
                let mut open = self.show_memory_window;
                egui::Window::new("Memory Inspector")
                    .open(&mut open)
                    .resizable(true)
                    .default_size([640.0, 420.0])
                    .show(ctx, |ui| self.draw_memory_inspector_contents(ui));
                self.show_memory_window = open;
            } else {
                egui::CentralPanel::default().show(ctx, |ui| {
                    self.draw_memory_inspector_contents(ui);
                });
            }
        });
    }

    fn fit_background(&self, available: egui::Vec2) -> egui::Vec2 {
        let image_size = self.background_style.image_size();
        let aspect = image_size.x / image_size.y;
        if available.x / available.y > aspect {
            egui::vec2(available.y * aspect, available.y)
        } else {
            egui::vec2(available.x, available.x / aspect)
        }
    }

    fn background_screen_rect(&self, background_rect: egui::Rect) -> egui::Rect {
        let image_size = self.background_style.image_size();
        let scale = background_rect.width() / image_size.x;
        let min = background_rect.min + self.background_style.screen_min() * scale;
        let size = self.background_style.screen_size() * scale;
        egui::Rect::from_min_size(min, size)
    }

    fn update_button_animation(&mut self, ctx: &egui::Context, elapsed: Duration) -> bool {
        if !self.background_style.shows_button_overlays() {
            return false;
        }

        let delta_seconds = elapsed.as_secs_f32().min(0.05);
        if delta_seconds <= f32::EPSILON {
            return false;
        }

        let button_down = ctx.input(|input| OverlayButton::ALL.map(|button| input.key_down(button.key())));
        let mut changed = false;

        for (index, target_down) in button_down.into_iter().enumerate() {
            let current = self.button_animation[index];
            let next = if target_down {
                (current + delta_seconds * BUTTON_PRESS_IN_SPEED).min(1.0)
            } else {
                (current - delta_seconds * BUTTON_PRESS_OUT_SPEED).max(0.0)
            };
            if (next - current).abs() > 0.001 {
                changed = true;
            }
            self.button_animation[index] = next;
        }

        changed
    }

    fn draw_button_overlays(&self, painter: &egui::Painter, background_rect: egui::Rect) {
        if !self.background_style.shows_button_overlays() {
            return;
        }

        let scale = background_rect.width() / self.background_style.image_size().x;
        let full_uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));

        for button in OverlayButton::ALL {
            let amount = self.button_animation[button as usize];
            if amount <= 0.001 {
                continue;
            }

            let overlay = self.button_overlays.get(button);
            let base_rect = egui::Rect::from_min_size(
                background_rect.min + overlay.base_min * scale,
                overlay.size * scale,
            );
            let animated_rect = egui::Rect::from_center_size(
                base_rect.center() + overlay.press_offset * scale * amount,
                base_rect.size() * (1.0 - (1.0 - overlay.pressed_scale) * amount),
            );
            let alpha = (amount * BUTTON_OVERLAY_MAX_ALPHA).round() as u8;

            painter.image(
                overlay.texture.id(),
                animated_rect,
                full_uv,
                Color32::from_rgba_unmultiplied(
                    BUTTON_OVERLAY_TINT,
                    BUTTON_OVERLAY_TINT,
                    BUTTON_OVERLAY_TINT,
                    alpha,
                ),
            );
        }
    }

    fn reset_timing(&mut self) {
        self.last_frame_time = Instant::now();
        self.frame_accumulator = Duration::ZERO;
        self.fps_sample_time = Duration::ZERO;
        self.fps_sample_frames = 0;
        self.displayed_fps = 0.0;
    }

    fn update_fps(&mut self, elapsed: Duration, frames_run: u32) {
        if !self.has_rom_loaded() || self.paused {
            self.fps_sample_time = Duration::ZERO;
            self.fps_sample_frames = 0;
            self.displayed_fps = 0.0;
            return;
        }

        self.fps_sample_time += elapsed;
        self.fps_sample_frames += frames_run;

        if self.fps_sample_time >= Duration::from_millis(250) {
            let seconds = self.fps_sample_time.as_secs_f32();
            if seconds > 0.0 {
                self.displayed_fps = self.fps_sample_frames as f32 / seconds;
            }
            self.fps_sample_time = Duration::ZERO;
            self.fps_sample_frames = 0;
        }
    }

    fn rom_name(&self) -> String {
        self.rom_path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "No ROM loaded".to_string())
    }

    fn rom_name_from_path(&self, path: &Path) -> String {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
    }

    fn console_name(&self) -> &'static str {
        self.emu.console_name()
    }

    fn resolution_label(&self) -> String {
        format!("{} x {}", self.screen_width, self.screen_height)
    }

    fn has_rom_loaded(&self) -> bool {
        self.emu.is_loaded()
    }

    fn pause_button_label(&self) -> &'static str {
        if self.paused { "Resume" } else { "Pause" }
    }

    fn pause_action_label(&self) -> &'static str {
        if self.paused {
            "Resume Emulation"
        } else {
            "Pause Emulation"
        }
    }

    fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        self.status_msg = if self.paused {
            "Emulation paused.".to_string()
        } else {
            "Emulation resumed.".to_string()
        };
    }

    fn key_legend(&self) -> &'static str {
        self.emu.key_legend()
    }

    fn sync_audio_volume(&self) {
        if let Some(ref audio) = self.audio_stream {
            if let Ok(mut volume) = audio.volume.lock() {
                *volume = self.volume;
            }
        }
    }

    fn save_backup(&self) {
        if let Some(path) = self.rom_path.as_deref() {
            self.emu.save_backup(path);
        }
    }

    fn auto_save_due(&self) -> bool {
        matches!(self.emu.total_frames(), Some(total_frames) if total_frames != 0 && total_frames % 60 == 0)
    }

    fn open_rom_dialog(&mut self, ctx: &egui::Context) {
        let file = rfd::FileDialog::new()
            .add_filter("ROMs", &["gb", "gbc", "gba", "nds"])
            .pick_file();

        if let Some(path) = file {
            self.load_rom(ctx, path);
        }
    }

    fn finish_rom_load(&mut self, path: PathBuf) {
        self.rom_path = Some(path);
        self.texture = None;
        self.last_framebuffer = None;
        self.paused = false;
        self.sync_screen_dimensions();
        self.setup_audio();
        self.sync_audio_volume();
        self.reset_timing();
    }

    fn sync_screen_dimensions(&mut self) {
        if let Some((width, height)) = self.emu.screen_dimensions() {
            self.screen_width = width;
            self.screen_height = height;
        }
    }

    fn load_rom(&mut self, ctx: &egui::Context, path: PathBuf) {
        let ext = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default();

        let Some(console) = ConsoleType::from_extension(&ext) else {
            self.status_msg = format!("Unknown file extension: .{}", ext);
            return;
        };
        let Some(background_style) = BackgroundStyle::from_extension(ext) else {
            self.status_msg = format!("Unsupported background for extension: .{}", ext);
            return;
        };

        let rom_data = match std::fs::read(&path) {
            Ok(data) => data,
            Err(error) => {
                self.status_msg = format!("Failed to read ROM: {}", error);
                return;
            }
        };

        let rom_name = self.rom_name_from_path(&path);

        let status_msg = match console {
            ConsoleType::GameBoyColor => {
                let cart = GbcCartridge::load(rom_data);
                let mut emu = GbcEmulator::new(cart);
                save::load_gbc_sram(&path, &mut emu);
                self.emu = Emulator::Gbc(emu);
                format!("Loaded GBC ROM: {}", rom_name)
            }
            ConsoleType::GameBoyAdvance => {
                let cart = GbaCartridge::load(rom_data);
                let mut emu = GbaEmulator::new(cart);
                save::load_gba_backup(&path, &mut emu);
                self.emu = Emulator::Gba(emu);
                format!("Loaded GBA ROM: {}", rom_name)
            }
            ConsoleType::NintendoDs => {
                let emu = match NdsEmulator::new(rom_data) {
                    Ok(emu) => emu,
                    Err(error) => {
                        self.status_msg = format!("Failed to load NDS ROM: {}", error);
                        return;
                    }
                };
                let title = emu.header.display_title().to_string();
                self.emu = Emulator::Nds(emu);
                format!("Loaded NDS ROM shell: {} ({})", rom_name, title)
            }
        };

        self.apply_background_style(ctx, background_style);
        self.status_msg = status_msg;
        self.finish_rom_load(path);
    }

    fn setup_audio(&mut self) {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

        let host = cpal::default_host();
        let device = match host.default_output_device() {
            Some(device) => device,
            None => {
                log::warn!("No audio output device found");
                return;
            }
        };

        let config = cpal::StreamConfig {
            channels: 2,
            sample_rate: cpal::SampleRate(44100),
            buffer_size: cpal::BufferSize::Default,
        };

        let buffer: Arc<Mutex<VecDeque<f32>>> =
            Arc::new(Mutex::new(VecDeque::with_capacity(MAX_AUDIO_BUFFER_SAMPLES)));
        let buffer_clone = buffer.clone();
        let volume = Arc::new(Mutex::new(self.volume));
        let volume_clone = volume.clone();

        let stream = device
            .build_output_stream(
                &config,
                {
                    let mut last_sample = 0.0f32;
                    move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                        let mut buffer = buffer_clone.lock().unwrap();
                        let volume = *volume_clone.lock().unwrap();
                        for sample in data.iter_mut() {
                            let next = buffer.pop_front().unwrap_or(last_sample);
                            last_sample = next;
                            *sample = next * volume;
                        }
                    }
                },
                |error| log::error!("Audio stream error: {}", error),
                None,
            )
            .ok();

        if let Some(ref active_stream) = stream {
            let _ = active_stream.play();
        }

        self.audio_stream = stream.map(|active_stream| AudioStream {
            _stream: active_stream,
            buffer,
            volume,
        });
    }

    fn handle_input(&mut self, ctx: &egui::Context) {
        ctx.input(|input| self.emu.handle_input(input));
    }

    fn save_state(&mut self) {
        if let Some(path) = self.rom_path.as_deref() {
            let result = self.save_state_to_slot(path);

            self.status_msg = match result {
                Ok(()) => format!("State saved to slot {}", self.save_slot),
                Err(error) => format!("Save failed: {}", error),
            };
        }
    }

    fn load_state(&mut self, ctx: &egui::Context) {
        if let Some(path) = self.rom_path.as_deref() {
            match self.load_state_from_slot(path) {
                Ok(Some(emu)) => {
                    self.emu = emu;
                    if let Some(background_style) = BackgroundStyle::from_path(path) {
                        self.apply_background_style(ctx, background_style);
                    }
                    self.sync_screen_dimensions();
                    self.reset_timing();
                    self.status_msg = format!("State loaded from slot {}", self.save_slot);
                }
                Ok(None) => {}
                Err(error) => {
                    self.status_msg = format!("Load failed: {}", error);
                }
            }
        }
    }

    fn save_state_to_slot(&self, path: &Path) -> Result<(), String> {
        self.emu.save_state(path, self.save_slot)
    }

    fn load_state_from_slot(&self, path: &Path) -> Result<Option<Emulator>, String> {
        self.emu.load_state(path, self.save_slot)
    }

    fn reset_emulator(&mut self, ctx: &egui::Context) {
        if let Some(path) = self.rom_path.clone() {
            self.load_rom(ctx, path);
        }
    }

    fn update_screen_texture(&mut self, ctx: &egui::Context, framebuffer: Vec<u32>) {
        self.last_framebuffer = Some(framebuffer);
        self.refresh_texture(ctx);
    }

    fn refresh_texture(&mut self, ctx: &egui::Context) {
        let Some(framebuffer) = self.last_framebuffer.as_ref() else {
            return;
        };

        let display_settings = self.display_settings;
        let pixels: Vec<egui::Color32> = framebuffer
            .iter()
            .map(|&argb| display_settings.apply(argb))
            .collect();

        let image = egui::ColorImage {
            size: [self.screen_width, self.screen_height],
            pixels,
        };

        self.texture = Some(ctx.load_texture("screen", image, egui::TextureOptions::NEAREST));
    }

    fn draw_top_bar(&mut self, ctx: &egui::Context, open_rom: &mut bool) {
        let has_rom = self.has_rom_loaded();

        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Open ROM").clicked() {
                    *open_rom = true;
                }

                if ui
                    .add_enabled(has_rom, egui::Button::new(self.pause_button_label()))
                    .clicked()
                {
                    self.toggle_pause();
                }

                if ui.add_enabled(has_rom, egui::Button::new("Reset")).clicked() {
                    self.reset_emulator(ctx);
                }

                if ui.button("Quit").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }

                ui.separator();
                ui.label(RichText::new(self.rom_name()).strong());

                if has_rom && self.paused {
                    ui.colored_label(Color32::from_rgb(255, 210, 120), "Paused");
                }
            });
        });
    }

    fn draw_side_panel(&mut self, ctx: &egui::Context, _open_rom: &mut bool) -> bool {
        let has_rom = self.has_rom_loaded();
        let mut display_changed = false;

        egui::SidePanel::left("controls")
            .default_width(SIDE_PANEL_WIDTH)
            .min_width(180.0)
            .max_width(420.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("gbrust");
                ui.label(
                    RichText::new(
                        "Back to a simple desktop frontend with quick controls and save-state access.",
                    )
                    .color(Color32::from_gray(180)),
                );

                ui.add_space(8.0);

                ui.group(|ui| {
                    ui.label(RichText::new("Session").strong());
                    ui.label(self.console_name());
                    ui.label(self.resolution_label());
                    ui.monospace(self.rom_name());
                    if let Emulator::Nds(emu) = &self.emu {
                        if let Some(warning) = emu.video_warning() {
                            ui.add_space(4.0);
                            ui.label(RichText::new(warning).color(Color32::from_rgb(225, 180, 96)).small());
                        }
                    }
                });

                ui.group(|ui| {
                    ui.label(RichText::new("Quick Actions").strong());
                    if ui
                        .add_sized([ui.available_width(), 28.0], egui::Button::new("Open ROM..."))
                        .clicked()
                    {
                        self.open_rom_dialog(ctx);
                    }

                    if ui
                        .add_enabled(has_rom, egui::Button::new(self.pause_action_label()))
                        .clicked()
                    {
                        self.toggle_pause();
                    }

                    if ui.add_enabled(has_rom, egui::Button::new("Reset ROM")).clicked() {
                        self.reset_emulator(ctx);
                    }
                });

                ui.group(|ui| {
                    ui.label(RichText::new("Save States").strong());
                    ui.horizontal(|ui| {
                        ui.label("Slot");
                        for slot in 1..=5u8 {
                            if ui
                                .selectable_label(self.save_slot == slot, slot.to_string())
                                .clicked()
                            {
                                self.save_slot = slot;
                            }
                        }
                    });

                    if ui
                        .add_enabled(has_rom, egui::Button::new("Save State"))
                        .clicked()
                    {
                        self.save_state();
                    }

                    if ui
                        .add_enabled(has_rom, egui::Button::new("Load State"))
                        .clicked()
                    {
                        self.load_state(ctx);
                    }
                });

                ui.group(|ui| {
                    ui.label(RichText::new("Audio").strong());
                    let response = ui.add(
                        egui::Slider::new(&mut self.volume, 0.0..=1.0)
                            .text("Volume")
                            .clamping(egui::SliderClamping::Always),
                    );
                    if response.changed() {
                        self.sync_audio_volume();
                    }
                });

                ui.group(|ui| {
                    ui.label(RichText::new("Display").strong());
                    display_changed |= self.display_settings.draw_controls(ui);

                    if ui.button("Reset Display").clicked() {
                        self.display_settings = DisplaySettings::default();
                        display_changed = true;
                    }
                });

                ui.group(|ui| {
                    ui.label(RichText::new("Keyboard").strong());
                    ui.label(self.key_legend());
                });

                ui.group(|ui| {
                    ui.label(RichText::new("Debug").strong());
                    ui.checkbox(&mut self.show_registers_window, "Registers");
                    ui.checkbox(&mut self.show_memory_window, "Memory inspector");
                });
            });

        display_changed
    }

    fn draw_status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status_msg);
                ui.separator();
                ui.label(format!("{} | {}", self.console_name(), self.resolution_label()));
                ui.separator();
                if self.has_rom_loaded() {
                    ui.label(format!("FPS {:.1}", self.displayed_fps));
                } else {
                    ui.label("FPS --");
                }
            });
        });
    }

    fn draw_screen(&mut self, ctx: &egui::Context, open_rom: &mut bool) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::from_rgb(11, 13, 17)))
            .show(ctx, |ui| {
                let available = ui.available_size();

                ui.centered_and_justified(|ui| {
                    let background_size = self.fit_background(available);
                    let (background_rect, _) =
                        ui.allocate_exact_size(background_size, egui::Sense::hover());
                    let painter = ui.painter_at(background_rect);
                    let full_uv = egui::Rect::from_min_max(
                        egui::pos2(0.0, 0.0),
                        egui::pos2(1.0, 1.0),
                    );
                    let screen_rect = self.background_screen_rect(background_rect);
                    let scale = background_rect.width() / self.background_style.image_size().x;
                    let lcd_mask_rect = screen_rect.expand(4.0 * scale);

                    painter.image(
                        self.background_texture.id(),
                        background_rect,
                        full_uv,
                        Color32::WHITE,
                    );
                    painter.rect_filled(lcd_mask_rect, 12.0 * scale, Color32::BLACK);

                    if let Some(ref texture) = self.texture {
                        painter.image(texture.id(), screen_rect, full_uv, Color32::WHITE);
                    } else {
                        painter.text(
                            screen_rect.center_top() + egui::vec2(0.0, screen_rect.height() * 0.38),
                            egui::Align2::CENTER_CENTER,
                            "Open a ROM from the sidebar",
                            egui::FontId::proportional((30.0 * scale).clamp(14.0, 32.0)),
                            Color32::from_gray(220),
                        );
                        painter.text(
                            screen_rect.center_top() + egui::vec2(0.0, screen_rect.height() * 0.48),
                            egui::Align2::CENTER_CENTER,
                            "The game will render inside this display.",
                            egui::FontId::proportional((18.0 * scale).clamp(11.0, 20.0)),
                            Color32::from_gray(160),
                        );

                        let button_rect = egui::Rect::from_center_size(
                            screen_rect.center_top() + egui::vec2(0.0, screen_rect.height() * 0.64),
                            egui::vec2(
                                (220.0 * scale).clamp(120.0, screen_rect.width() - 16.0),
                                (42.0 * scale).clamp(28.0, 48.0),
                            ),
                        );
                        if ui.put(button_rect, egui::Button::new("Choose ROM")).clicked() {
                            *open_rom = true;
                        }
                    }

                    self.draw_button_overlays(&painter, background_rect);
                });
            });
    }
}

impl eframe::App for EmuApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut open_rom = false;

        self.draw_top_bar(ctx, &mut open_rom);
        let display_changed = self.draw_side_panel(ctx, &mut open_rom);
        self.draw_status_bar(ctx);
        self.handle_input(ctx);

        if display_changed {
            self.refresh_texture(ctx);
        }

        let now = Instant::now();
        let elapsed = now.duration_since(self.last_frame_time);
        self.last_frame_time = now;
        let button_animation_changed = self.update_button_animation(ctx, elapsed);
        let mut frames_run = 0;

        if self.paused || !self.emu.is_loaded() {
            self.frame_accumulator = Duration::ZERO;
        } else {
            let max_accumulator = FRAME_DURATION
                .checked_mul(MAX_CATCH_UP_FRAMES)
                .unwrap_or(FRAME_DURATION);
            self.frame_accumulator = (self.frame_accumulator + elapsed).min(max_accumulator);

            let mut latest_framebuffer: Option<Vec<u32>> = None;

            while self.frame_accumulator >= FRAME_DURATION && frames_run < MAX_CATCH_UP_FRAMES {
                self.frame_accumulator -= FRAME_DURATION;
                frames_run += 1;

                let Some((framebuffer, samples)) = self.emu.run_frame() else {
                    break;
                };

                if let Some(ref audio) = self.audio_stream {
                    if let Ok(mut buffer) = audio.buffer.lock() {
                        buffer.extend(samples);
                        while buffer.len() > MAX_AUDIO_BUFFER_SAMPLES {
                            buffer.pop_front();
                        }
                    }
                }

                if !framebuffer.is_empty() {
                    latest_framebuffer = Some(framebuffer);
                }
            }

            if let Some(framebuffer) = latest_framebuffer {
                self.update_screen_texture(ctx, framebuffer);
            }
        }

        self.update_fps(elapsed, frames_run);

        self.draw_screen(ctx, &mut open_rom);
        self.draw_registers_window(ctx);
        self.draw_memory_inspector_window(ctx);

        if open_rom {
            self.open_rom_dialog(ctx);
        }

        if button_animation_changed {
            ctx.request_repaint();
        }

        if !self.paused && self.auto_save_due() {
            self.save_backup();
        }

        if self.emu.is_loaded() && !self.paused {
            if self.frame_accumulator >= FRAME_DURATION {
                ctx.request_repaint();
            } else {
                ctx.request_repaint_after(FRAME_DURATION - self.frame_accumulator);
            }
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_backup();
    }
}

pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([980.0, 720.0])
            .with_min_inner_size([720.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "gbrust",
        options,
        Box::new(|cc| Ok(Box::new(EmuApp::new(cc)))),
    )
}