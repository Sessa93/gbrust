use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use eframe::egui;

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

pub struct EmuApp {
    emu: Emulator,
    rom_path: Option<PathBuf>,
    texture: Option<egui::TextureHandle>,
    screen_width: usize,
    screen_height: usize,
    audio_stream: Option<AudioStream>,
    save_slot: u8,
    status_msg: String,
    paused: bool,
}

struct AudioStream {
    _stream: cpal::Stream,
    buffer: Arc<Mutex<Vec<f32>>>,
}

// cpal::Stream is not Send on some platforms, but we only use it from the main thread
unsafe impl Send for AudioStream {}
unsafe impl Sync for AudioStream {}

impl EmuApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            emu: Emulator::None,
            rom_path: None,
            texture: None,
            screen_width: 240,
            screen_height: 160,
            audio_stream: None,
            save_slot: 1,
            status_msg: "No ROM loaded. File -> Open ROM".to_string(),
            paused: false,
        }
    }

    fn load_rom(&mut self, path: PathBuf) {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();

        let console = match ConsoleType::from_extension(&ext) {
            Some(c) => c,
            None => {
                self.status_msg = format!("Unknown file extension: .{}", ext);
                return;
            }
        };

        let rom_data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(e) => {
                self.status_msg = format!("Failed to read ROM: {}", e);
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
                self.status_msg = format!("Loaded GBC ROM: {}", path.file_name().unwrap_or_default().to_string_lossy());
            }
            ConsoleType::GameBoyAdvance => {
                let cart = GbaCartridge::load(rom_data);
                let mut emu = GbaEmulator::new(cart);
                save::load_gba_backup(&path, &mut emu);
                self.screen_width = crate::GBA_WIDTH;
                self.screen_height = crate::GBA_HEIGHT;
                self.emu = Emulator::Gba(emu);
                self.status_msg = format!("Loaded GBA ROM: {}", path.file_name().unwrap_or_default().to_string_lossy());
            }
        }

        self.rom_path = Some(path);
        self.texture = None;
        self.setup_audio();
    }

    fn setup_audio(&mut self) {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

        let host = cpal::default_host();
        let device = match host.default_output_device() {
            Some(d) => d,
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

        let buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
        let buf_clone = buffer.clone();

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut buf = buf_clone.lock().unwrap();
                    for sample in data.iter_mut() {
                        *sample = if !buf.is_empty() {
                            buf.remove(0)
                        } else {
                            0.0
                        };
                    }
                },
                |err| log::error!("Audio stream error: {}", err),
                None,
            )
            .ok();

        if let Some(ref s) = stream {
            let _ = s.play();
        }

        self.audio_stream = stream.map(|s| AudioStream {
            _stream: s,
            buffer,
        });
    }

    fn handle_input(&mut self, ctx: &egui::Context) {
        ctx.input(|i| match &mut self.emu {
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
                    if i.key_pressed(ekey) {
                        emu.bus.input.key_down(gkey);
                    }
                    if i.key_released(ekey) {
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
                    if i.key_pressed(ekey) {
                        emu.bus.input.key_down(gkey);
                    }
                    if i.key_released(ekey) {
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
            match result {
                Ok(()) => self.status_msg = format!("State saved to slot {}", self.save_slot),
                Err(e) => self.status_msg = format!("Save failed: {}", e),
            }
        }
    }

    fn load_state(&mut self) {
        if let Some(ref path) = self.rom_path {
            match &self.emu {
                Emulator::Gbc(_) => match save::load_gbc_state(path, self.save_slot) {
                    Ok(emu) => {
                        self.emu = Emulator::Gbc(emu);
                        self.status_msg =
                            format!("State loaded from slot {}", self.save_slot);
                    }
                    Err(e) => self.status_msg = format!("Load failed: {}", e),
                },
                Emulator::Gba(_) => match save::load_gba_state(path, self.save_slot) {
                    Ok(emu) => {
                        self.emu = Emulator::Gba(emu);
                        self.status_msg =
                            format!("State loaded from slot {}", self.save_slot);
                    }
                    Err(e) => self.status_msg = format!("Load failed: {}", e),
                },
                Emulator::None => {}
            }
        }
    }
}

impl eframe::App for EmuApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Menu bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open ROM...").clicked() {
                        let file = rfd::FileDialog::new()
                            .add_filter("ROMs", &["gb", "gbc", "gba"])
                            .pick_file();
                        if let Some(path) = file {
                            self.load_rom(path);
                        }
                        ui.close_menu();
                    }
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("Emulation", |ui| {
                    let pause_text = if self.paused { "Resume" } else { "Pause" };
                    if ui.button(pause_text).clicked() {
                        self.paused = !self.paused;
                        ui.close_menu();
                    }
                    if ui.button("Reset").clicked() {
                        if let Some(ref path) = self.rom_path.clone() {
                            self.load_rom(path.clone());
                        }
                        ui.close_menu();
                    }
                });

                ui.menu_button("Save/Load", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Slot:");
                        for s in 1..=5u8 {
                            if ui
                                .selectable_label(self.save_slot == s, format!("{}", s))
                                .clicked()
                            {
                                self.save_slot = s;
                            }
                        }
                    });
                    if ui.button("Save State").clicked() {
                        self.save_state();
                        ui.close_menu();
                    }
                    if ui.button("Load State").clicked() {
                        self.load_state();
                        ui.close_menu();
                    }
                });
            });
        });

        // Status bar
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status_msg);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label("Z/X=A/B  Enter=Start  Arrows=D-Pad  A/S=L/R");
                });
            });
        });

        // Handle input
        self.handle_input(ctx);

        // Run emulator frame
        if !self.paused {
            let framebuffer: Vec<u32> = match &mut self.emu {
                Emulator::Gbc(emu) => emu.run_frame().to_vec(),
                Emulator::Gba(emu) => emu.run_frame().to_vec(),
                Emulator::None => vec![],
            };

            // Push audio
            if let Some(ref audio) = self.audio_stream {
                let samples = match &mut self.emu {
                    Emulator::Gbc(emu) => emu.audio_buffer(),
                    Emulator::Gba(emu) => emu.audio_buffer(),
                    Emulator::None => vec![],
                };
                if let Ok(mut buf) = audio.buffer.lock() {
                    // Keep buffer from growing too large
                    if buf.len() < 8192 {
                        buf.extend_from_slice(&samples);
                    }
                }
            }

            // Update texture
            if !framebuffer.is_empty() {
                let pixels: Vec<egui::Color32> = framebuffer
                    .iter()
                    .map(|&argb| {
                        let r = ((argb >> 16) & 0xFF) as u8;
                        let g = ((argb >> 8) & 0xFF) as u8;
                        let b = (argb & 0xFF) as u8;
                        egui::Color32::from_rgb(r, g, b)
                    })
                    .collect();

                let image = egui::ColorImage {
                    size: [self.screen_width, self.screen_height],
                    pixels,
                };

                let tex = ctx.load_texture(
                    "screen",
                    image,
                    egui::TextureOptions::NEAREST,
                );
                self.texture = Some(tex);
            }
        }

        // Central panel with game screen
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(egui::Color32::BLACK))
            .show(ctx, |ui| {
                if let Some(ref tex) = self.texture {
                    let available = ui.available_size();
                    let aspect = self.screen_width as f32 / self.screen_height as f32;
                    let (w, h) = if available.x / available.y > aspect {
                        (available.y * aspect, available.y)
                    } else {
                        (available.x, available.x / aspect)
                    };

                    let padding_x = (available.x - w) / 2.0;
                    let padding_y = (available.y - h) / 2.0;

                    ui.add_space(padding_y);
                    ui.horizontal(|ui| {
                        ui.add_space(padding_x);
                        ui.image(egui::load::SizedTexture::new(
                            tex.id(),
                            egui::vec2(w, h),
                        ));
                    });
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.heading("gbrust");
                    });
                }
            });

        // Auto-save SRAM on each frame (debounced internally)
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

        // Request continuous repaint when emulating
        if !matches!(self.emu, Emulator::None) && !self.paused {
            ctx.request_repaint();
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Save SRAM on exit
        if let Some(ref path) = self.rom_path {
            match &self.emu {
                Emulator::Gbc(emu) => save::save_gbc_sram(path, emu),
                Emulator::Gba(emu) => save::save_gba_backup(path, emu),
                Emulator::None => {}
            }
        }
    }
}

pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([520.0, 420.0])
            .with_min_inner_size([320.0, 280.0]),
        ..Default::default()
    };

    eframe::run_native(
        "gbrust",
        options,
        Box::new(|cc| Ok(Box::new(EmuApp::new(cc)))),
    )
}
