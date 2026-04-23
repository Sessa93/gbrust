use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};

use crate::cartridge::{GbaCartridge, GbcCartridge};
use crate::emulator::gba::GbaEmulator;
use crate::emulator::gbc::GbcEmulator;
use crate::input::{GbaKey, GbcKey};
use crate::save;
use crate::ConsoleType;

enum Emulator {
    None,
    Gbc(GbcEmulator),
    Gba(GbaEmulator),
}

const FRAME_DURATION: Duration = Duration::from_nanos(16_742_706);
const MAX_AUDIO_BUFFER_SAMPLES: usize = 8192;
const MAX_CATCH_UP_FRAMES: u32 = 4;
const SIDE_PANEL_WIDTH: f32 = 220.0;
const BACKGROUND_IMAGE_SIZE: egui::Vec2 = egui::vec2(1920.0, 1080.0);
const BACKGROUND_SCREEN_MIN: egui::Vec2 = egui::vec2(551.0, 242.0);
const BACKGROUND_SCREEN_SIZE: egui::Vec2 = egui::vec2(813.0, 542.0);
const BUTTON_PRESS_IN_SPEED: f32 = 15.0;
const BUTTON_PRESS_OUT_SPEED: f32 = 11.0;
const BUTTON_OVERLAY_MAX_ALPHA: f32 = 144.0;
const BUTTON_OVERLAY_TINT: u8 = 160;

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
            Self::A => include_bytes!("../resources/a.png"),
            Self::B => include_bytes!("../resources/b.png"),
            Self::L => include_bytes!("../resources/l.png"),
            Self::R => include_bytes!("../resources/r.png"),
            Self::Start => include_bytes!("../resources/start.png"),
            Self::Select => include_bytes!("../resources/select.png"),
            Self::Up => include_bytes!("../resources/up.png"),
            Self::Down => include_bytes!("../resources/down.png"),
            Self::Left => include_bytes!("../resources/left.png"),
            Self::Right => include_bytes!("../resources/right.png"),
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
            Self::Down => egui::vec2(160.0, 530.0),
            Self::Left => egui::vec2(73.0, 406.0),
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

        let background_texture = Self::load_background_texture(&cc.egui_ctx);
        let button_overlays = ButtonOverlays::load(&cc.egui_ctx);

        Self {
            emu: Emulator::None,
            rom_path: None,
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
            button_animation: [0.0; OverlayButton::COUNT],
        }
    }

    fn load_background_texture(ctx: &egui::Context) -> egui::TextureHandle {
        let image = image::load_from_memory(include_bytes!("../resources/background.png"))
            .expect("background.png should be a valid PNG")
            .to_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        let color_image = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());

        ctx.load_texture(
            "gba-overlay-background",
            color_image,
            egui::TextureOptions::LINEAR,
        )
    }

    fn fit_background(available: egui::Vec2) -> egui::Vec2 {
        let aspect = BACKGROUND_IMAGE_SIZE.x / BACKGROUND_IMAGE_SIZE.y;
        if available.x / available.y > aspect {
            egui::vec2(available.y * aspect, available.y)
        } else {
            egui::vec2(available.x, available.x / aspect)
        }
    }

    fn background_screen_rect(background_rect: egui::Rect) -> egui::Rect {
        let scale = background_rect.width() / BACKGROUND_IMAGE_SIZE.x;
        let min = background_rect.min + BACKGROUND_SCREEN_MIN * scale;
        let size = BACKGROUND_SCREEN_SIZE * scale;
        egui::Rect::from_min_size(min, size)
    }

    fn update_button_animation(&mut self, ctx: &egui::Context, elapsed: Duration) -> bool {
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
        let scale = background_rect.width() / BACKGROUND_IMAGE_SIZE.x;
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
    }

    fn rom_name(&self) -> String {
        self.rom_path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "No ROM loaded".to_string())
    }

    fn rom_name_from_path(&self, path: &PathBuf) -> String {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
    }

    fn console_name(&self) -> &'static str {
        match self.emu {
            Emulator::Gbc(_) => "Game Boy Color",
            Emulator::Gba(_) => "Game Boy Advance",
            Emulator::None => "No console",
        }
    }

    fn resolution_label(&self) -> String {
        format!("{} x {}", self.screen_width, self.screen_height)
    }

    fn has_rom_loaded(&self) -> bool {
        !matches!(self.emu, Emulator::None)
    }

    fn key_legend(&self) -> &'static str {
        match self.emu {
            Emulator::Gba(_) => {
                "Z/X = A/B, Enter = Start, Backspace = Select, Arrows = D-Pad, A/S = L/R"
            }
            _ => "Z/X = A/B, Enter = Start, Backspace = Select, Arrows = D-Pad",
        }
    }

    fn sync_audio_volume(&self) {
        if let Some(ref audio) = self.audio_stream {
            if let Ok(mut volume) = audio.volume.lock() {
                *volume = self.volume;
            }
        }
    }

    fn save_backup(&self) {
        if let Some(ref path) = self.rom_path {
            match &self.emu {
                Emulator::Gbc(emu) => save::save_gbc_sram(path, emu),
                Emulator::Gba(emu) => save::save_gba_backup(path, emu),
                Emulator::None => {}
            }
        }
    }

    fn open_rom_dialog(&mut self) {
        let file = rfd::FileDialog::new()
            .add_filter("ROMs", &["gb", "gbc", "gba"])
            .pick_file();

        if let Some(path) = file {
            self.load_rom(path);
        }
    }

    fn load_rom(&mut self, path: PathBuf) {
        let ext = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_string();

        let Some(console) = ConsoleType::from_extension(&ext) else {
            self.status_msg = format!("Unknown file extension: .{}", ext);
            return;
        };

        let rom_data = match std::fs::read(&path) {
            Ok(data) => data,
            Err(error) => {
                self.status_msg = format!("Failed to read ROM: {}", error);
                return;
            }
        };

        match console {
            ConsoleType::GameBoyColor => {
                let cart = GbcCartridge::load(rom_data);
                let mut emu = GbcEmulator::new(cart);
                save::load_gbc_sram(&path, &mut emu);
                self.screen_width = crate::GBC_WIDTH;
                self.screen_height = crate::GBC_HEIGHT;
                self.emu = Emulator::Gbc(emu);
                self.status_msg =
                    format!("Loaded GBC ROM: {}", self.rom_name_from_path(&path));
            }
            ConsoleType::GameBoyAdvance => {
                let cart = GbaCartridge::load(rom_data);
                let mut emu = GbaEmulator::new(cart);
                save::load_gba_backup(&path, &mut emu);
                self.screen_width = crate::GBA_WIDTH;
                self.screen_height = crate::GBA_HEIGHT;
                self.emu = Emulator::Gba(emu);
                self.status_msg =
                    format!("Loaded GBA ROM: {}", self.rom_name_from_path(&path));
            }
        }

        self.rom_path = Some(path);
        self.texture = None;
        self.last_framebuffer = None;
        self.paused = false;
        self.setup_audio();
        self.sync_audio_volume();
        self.reset_timing();
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
        ctx.input(|input| match &mut self.emu {
            Emulator::Gbc(emu) => {
                let map: &[(egui::Key, GbcKey)] = &[
                    (egui::Key::Z, GbcKey::A),
                    (egui::Key::X, GbcKey::B),
                    (egui::Key::Enter, GbcKey::Start),
                    (egui::Key::Backspace, GbcKey::Select),
                    (egui::Key::ArrowUp, GbcKey::Up),
                    (egui::Key::ArrowDown, GbcKey::Down),
                    (egui::Key::ArrowLeft, GbcKey::Left),
                    (egui::Key::ArrowRight, GbcKey::Right),
                ];

                for &(ekey, gkey) in map {
                    if input.key_pressed(ekey) {
                        emu.bus.input.key_down(gkey);
                    }
                    if input.key_released(ekey) {
                        emu.bus.input.key_up(gkey);
                    }
                }
            }
            Emulator::Gba(emu) => {
                let map: &[(egui::Key, GbaKey)] = &[
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

                for &(ekey, gkey) in map {
                    if input.key_pressed(ekey) {
                        emu.bus.input.key_down(gkey);
                    }
                    if input.key_released(ekey) {
                        emu.bus.input.key_up(gkey);
                    }
                }
            }
            Emulator::None => {}
        });
    }

    fn save_state(&mut self) {
        if let Some(ref path) = self.rom_path {
            let result = match &self.emu {
                Emulator::Gbc(emu) => save::save_gbc_state(path, self.save_slot, emu),
                Emulator::Gba(emu) => save::save_gba_state(path, self.save_slot, emu),
                Emulator::None => return,
            };

            self.status_msg = match result {
                Ok(()) => format!("State saved to slot {}", self.save_slot),
                Err(error) => format!("Save failed: {}", error),
            };
        }
    }

    fn load_state(&mut self) {
        if let Some(ref path) = self.rom_path {
            match &self.emu {
                Emulator::Gbc(_) => match save::load_gbc_state(path, self.save_slot) {
                    Ok(emu) => {
                        self.emu = Emulator::Gbc(emu);
                        self.reset_timing();
                        self.status_msg = format!("State loaded from slot {}", self.save_slot);
                    }
                    Err(error) => self.status_msg = format!("Load failed: {}", error),
                },
                Emulator::Gba(_) => match save::load_gba_state(path, self.save_slot) {
                    Ok(emu) => {
                        self.emu = Emulator::Gba(emu);
                        self.reset_timing();
                        self.status_msg = format!("State loaded from slot {}", self.save_slot);
                    }
                    Err(error) => self.status_msg = format!("Load failed: {}", error),
                },
                Emulator::None => {}
            }
        }
    }

    fn reset_emulator(&mut self) {
        if let Some(path) = self.rom_path.clone() {
            self.load_rom(path);
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
                    .add_enabled(
                        has_rom,
                        egui::Button::new(if self.paused { "Resume" } else { "Pause" }),
                    )
                    .clicked()
                {
                    self.paused = !self.paused;
                    self.status_msg = if self.paused {
                        "Emulation paused.".to_string()
                    } else {
                        "Emulation resumed.".to_string()
                    };
                }

                if ui.add_enabled(has_rom, egui::Button::new("Reset")).clicked() {
                    self.reset_emulator();
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

    fn draw_side_panel(&mut self, ctx: &egui::Context, open_rom: &mut bool) -> bool {
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
                });

                ui.group(|ui| {
                    ui.label(RichText::new("Quick Actions").strong());
                    if ui
                        .add_sized([ui.available_width(), 28.0], egui::Button::new("Open ROM..."))
                        .clicked()
                    {
                        *open_rom = true;
                    }

                    if ui
                        .add_enabled(
                            has_rom,
                            egui::Button::new(if self.paused {
                                "Resume Emulation"
                            } else {
                                "Pause Emulation"
                            }),
                        )
                        .clicked()
                    {
                        self.paused = !self.paused;
                    }

                    if ui.add_enabled(has_rom, egui::Button::new("Reset ROM")).clicked() {
                        self.reset_emulator();
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
                        self.load_state();
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
                    display_changed |= ui
                        .add(
                            egui::Slider::new(&mut self.display_settings.gamma, 0.60..=2.40)
                                .text("Gamma"),
                        )
                        .changed();
                    display_changed |= ui
                        .add(
                            egui::Slider::new(
                                &mut self.display_settings.saturation,
                                0.0..=2.0,
                            )
                            .text("Saturation"),
                        )
                        .changed();
                    display_changed |= ui
                        .add(
                            egui::Slider::new(&mut self.display_settings.red_gain, 0.5..=1.5)
                                .text("Red"),
                        )
                        .changed();
                    display_changed |= ui
                        .add(
                            egui::Slider::new(&mut self.display_settings.green_gain, 0.5..=1.5)
                                .text("Green"),
                        )
                        .changed();
                    display_changed |= ui
                        .add(
                            egui::Slider::new(&mut self.display_settings.blue_gain, 0.5..=1.5)
                                .text("Blue"),
                        )
                        .changed();

                    if ui.button("Reset Display").clicked() {
                        self.display_settings = DisplaySettings::default();
                        display_changed = true;
                    }
                });

                ui.group(|ui| {
                    ui.label(RichText::new("Keyboard").strong());
                    ui.label(self.key_legend());
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
            });
        });
    }

    fn draw_screen(&mut self, ctx: &egui::Context, open_rom: &mut bool) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::from_rgb(11, 13, 17)))
            .show(ctx, |ui| {
                let available = ui.available_size();

                ui.centered_and_justified(|ui| {
                    let background_size = Self::fit_background(available);
                    let (background_rect, _) =
                        ui.allocate_exact_size(background_size, egui::Sense::hover());
                    let painter = ui.painter_at(background_rect);
                    let full_uv = egui::Rect::from_min_max(
                        egui::pos2(0.0, 0.0),
                        egui::pos2(1.0, 1.0),
                    );
                    let screen_rect = Self::background_screen_rect(background_rect);
                    let scale = background_rect.width() / BACKGROUND_IMAGE_SIZE.x;
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

        if self.paused || matches!(self.emu, Emulator::None) {
            self.frame_accumulator = Duration::ZERO;
        } else {
            let max_accumulator = FRAME_DURATION
                .checked_mul(MAX_CATCH_UP_FRAMES)
                .unwrap_or(FRAME_DURATION);
            self.frame_accumulator = (self.frame_accumulator + elapsed).min(max_accumulator);

            let mut latest_framebuffer: Option<Vec<u32>> = None;
            let mut frames_run = 0;

            while self.frame_accumulator >= FRAME_DURATION && frames_run < MAX_CATCH_UP_FRAMES {
                self.frame_accumulator -= FRAME_DURATION;
                frames_run += 1;

                let (framebuffer, samples) = match &mut self.emu {
                    Emulator::Gbc(emu) => (emu.run_frame().to_vec(), emu.audio_buffer()),
                    Emulator::Gba(emu) => (emu.run_frame().to_vec(), emu.audio_buffer()),
                    Emulator::None => (vec![], vec![]),
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

        self.draw_screen(ctx, &mut open_rom);

        if open_rom {
            self.open_rom_dialog();
        }

        if button_animation_changed {
            ctx.request_repaint();
        }

        if let (Some(path), false) = (&self.rom_path, self.paused) {
            match &self.emu {
                Emulator::Gbc(emu) => {
                    if emu.total_frames % 60 == 0 {
                        save::save_gbc_sram(path, emu);
                    }
                }
                Emulator::Gba(emu) => {
                    if emu.total_frames % 60 == 0 {
                        save::save_gba_backup(path, emu);
                    }
                }
                Emulator::None => {}
            }
        }

        if !matches!(self.emu, Emulator::None) && !self.paused {
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