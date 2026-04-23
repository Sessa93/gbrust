use serde::{Deserialize, Serialize};

use crate::cpu::arm7tdmi::Arm7Tdmi;
use crate::memory::gba_bus::GbaBus;
use crate::cartridge::GbaCartridge;
use crate::{GBA_WIDTH, GBA_HEIGHT};

#[derive(Clone, Serialize, Deserialize)]
pub struct GbaEmulator {
    pub cpu: Arm7Tdmi,
    pub bus: GbaBus,
    pub frame_cycles: u32,
    pub total_frames: u64,
    #[serde(default, skip)]
    pub repair_legacy_palette_state: bool,
}

impl GbaEmulator {
    pub fn new(cart: GbaCartridge) -> Self {
        Self {
            cpu: Arm7Tdmi::new(),
            bus: GbaBus::new(cart),
            frame_cycles: 0,
            total_frames: 0,
            repair_legacy_palette_state: false,
        }
    }

    /// Run one full frame (280896 cycles = 228 scanlines * 1232 cycles each).
    /// Returns the framebuffer.
    pub fn run_frame(&mut self) -> &[u32] {
        const CYCLES_PER_FRAME: u32 = 280896;

        self.frame_cycles = 0;

        while self.frame_cycles < CYCLES_PER_FRAME {
            // Propagate bus HALT (HALTCNT write) to CPU
            if self.bus.halt {
                self.cpu.halted = true;
                self.bus.halt = false;
            }

            // Check for IRQ
            if self.bus.check_irq() {
                self.cpu.handle_irq();
            }

            let cpu_cycles = self.cpu.step(&mut self.bus);
            self.bus.tick(cpu_cycles);
            self.frame_cycles += cpu_cycles;
        }

        self.total_frames += 1;

        if crate::save::gba_palette_has_duplicated_banks(self) {
            self.repair_legacy_palette_state |= crate::save::repair_gba_palette_state_if_needed(self);
        }

        &self.bus.ppu.framebuffer
    }

    pub fn screen_width(&self) -> u32 {
        GBA_WIDTH as u32
    }

    pub fn screen_height(&self) -> u32 {
        GBA_HEIGHT as u32
    }

    pub fn audio_buffer(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.bus.apu.audio_buffer)
    }
}
