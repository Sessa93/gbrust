use serde::{Deserialize, Serialize};

use crate::cpu::sm83::Sm83;
use crate::memory::gbc_bus::GbcBus;
use crate::cartridge::GbcCartridge;
use crate::{GBC_WIDTH, GBC_HEIGHT};

#[derive(Clone, Serialize, Deserialize)]
pub struct GbcEmulator {
    pub cpu: Sm83,
    pub bus: GbcBus,
    pub frame_cycles: u32,
    pub total_frames: u64,
}

impl GbcEmulator {
    pub fn new(cart: GbcCartridge) -> Self {
        Self {
            cpu: Sm83::new(),
            bus: GbcBus::new(cart),
            frame_cycles: 0,
            total_frames: 0,
        }
    }

    /// Run one full frame (70224 T-cycles in normal speed, 140448 in double speed).
    /// Returns the framebuffer.
    pub fn run_frame(&mut self) -> &[u32] {
        let cycles_per_frame = if self.bus.double_speed {
            70224 * 2
        } else {
            70224
        };

        self.frame_cycles = 0;

        while self.frame_cycles < cycles_per_frame {
            let cpu_cycles = self.cpu.step(&mut self.bus);

            // In double speed, CPU runs at 2x but PPU/APU at 1x
            let bus_cycles = if self.bus.double_speed {
                cpu_cycles / 2
            } else {
                cpu_cycles
            };

            self.bus.tick(bus_cycles);
            self.frame_cycles += cpu_cycles;
        }

        self.total_frames += 1;
        &self.bus.ppu.framebuffer
    }

    pub fn screen_width(&self) -> u32 {
        GBC_WIDTH as u32
    }

    pub fn screen_height(&self) -> u32 {
        GBC_HEIGHT as u32
    }

    pub fn audio_buffer(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.bus.apu.audio_buffer)
    }
}
