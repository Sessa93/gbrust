use std::fs;
use std::path::{Path, PathBuf};

use crate::emulator::gba::GbaEmulator;
use crate::emulator::gbc::GbcEmulator;

fn save_path(rom_path: &Path, ext: &str) -> PathBuf {
    rom_path.with_extension(ext)
}

// ─── GBC Save/Load ────────────────────────────────────

pub fn save_gbc_sram(rom_path: &Path, emu: &GbcEmulator) {
    if emu.bus.cart.cart_type.has_battery() && !emu.bus.cart.ram.is_empty() {
        let path = save_path(rom_path, "sav");
        if let Err(e) = fs::write(&path, &emu.bus.cart.ram) {
            log::error!("Failed to save SRAM to {}: {}", path.display(), e);
        } else {
            log::info!("Saved SRAM to {}", path.display());
        }
    }
}

pub fn load_gbc_sram(rom_path: &Path, emu: &mut GbcEmulator) {
    if emu.bus.cart.cart_type.has_battery() {
        let path = save_path(rom_path, "sav");
        if path.exists() {
            match fs::read(&path) {
                Ok(data) => {
                    let len = data.len().min(emu.bus.cart.ram.len());
                    emu.bus.cart.ram[..len].copy_from_slice(&data[..len]);
                    log::info!("Loaded SRAM from {}", path.display());
                }
                Err(e) => log::error!("Failed to load SRAM from {}: {}", path.display(), e),
            }
        }
    }
}

pub fn save_gbc_state(rom_path: &Path, slot: u8, emu: &GbcEmulator) -> Result<(), String> {
    let path = save_path(rom_path, &format!("ss{}", slot));
    let data = bincode::serialize(emu).map_err(|e| format!("Serialize error: {}", e))?;
    fs::write(&path, &data).map_err(|e| format!("Write error: {}", e))?;
    log::info!("Saved state to {}", path.display());
    Ok(())
}

pub fn load_gbc_state(rom_path: &Path, slot: u8) -> Result<GbcEmulator, String> {
    let path = save_path(rom_path, &format!("ss{}", slot));
    let data = fs::read(&path).map_err(|e| format!("Read error: {}", e))?;
    let emu: GbcEmulator =
        bincode::deserialize(&data).map_err(|e| format!("Deserialize error: {}", e))?;
    log::info!("Loaded state from {}", path.display());
    Ok(emu)
}

// ─── GBA Save/Load ────────────────────────────────────

pub fn save_gba_backup(rom_path: &Path, emu: &GbaEmulator) {
    use crate::cartridge::GbaBackupType;
    let path = save_path(rom_path, "sav");
    let data = match emu.bus.cart.backup_type {
        GbaBackupType::Sram => &emu.bus.cart.sram,
        GbaBackupType::Flash64k | GbaBackupType::Flash128k => &emu.bus.cart.flash,
        GbaBackupType::Eeprom => &emu.bus.cart.eeprom,
        GbaBackupType::None => return,
    };
    if let Err(e) = fs::write(&path, data) {
        log::error!("Failed to save backup to {}: {}", path.display(), e);
    } else {
        log::info!("Saved backup to {}", path.display());
    }
}

pub fn load_gba_backup(rom_path: &Path, emu: &mut GbaEmulator) {
    use crate::cartridge::GbaBackupType;
    let path = save_path(rom_path, "sav");
    if !path.exists() {
        return;
    }
    match fs::read(&path) {
        Ok(data) => {
            let target = match emu.bus.cart.backup_type {
                GbaBackupType::Sram => &mut emu.bus.cart.sram,
                GbaBackupType::Flash64k | GbaBackupType::Flash128k => &mut emu.bus.cart.flash,
                GbaBackupType::Eeprom => &mut emu.bus.cart.eeprom,
                GbaBackupType::None => return,
            };
            let len = data.len().min(target.len());
            target[..len].copy_from_slice(&data[..len]);
            log::info!("Loaded backup from {}", path.display());
        }
        Err(e) => log::error!("Failed to load backup from {}: {}", path.display(), e),
    }
}

pub fn save_gba_state(rom_path: &Path, slot: u8, emu: &GbaEmulator) -> Result<(), String> {
    let path = save_path(rom_path, &format!("ss{}", slot));
    let data = bincode::serialize(emu).map_err(|e| format!("Serialize error: {}", e))?;
    fs::write(&path, &data).map_err(|e| format!("Write error: {}", e))?;
    log::info!("Saved state to {}", path.display());
    Ok(())
}

pub fn load_gba_state(rom_path: &Path, slot: u8) -> Result<GbaEmulator, String> {
    let path = save_path(rom_path, &format!("ss{}", slot));
    let data = fs::read(&path).map_err(|e| format!("Read error: {}", e))?;
    let emu: GbaEmulator =
        bincode::deserialize(&data).map_err(|e| format!("Deserialize error: {}", e))?;
    log::info!("Loaded state from {}", path.display());
    Ok(emu)
}
