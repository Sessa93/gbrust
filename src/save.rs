use std::fs;
use std::path::{Path, PathBuf};

use crate::emulator::gba::GbaEmulator;
use crate::emulator::gbc::GbcEmulator;

const GBA_PALETTE_RAM_SIZE: usize = 0x400;
const GBA_PALETTE_BANK_SIZE: usize = 0x20;

fn bank_has_duplicated_low_halfwords(src: &[u8], dst: &[u8]) -> bool {
    if src.len() != GBA_PALETTE_BANK_SIZE || dst.len() != GBA_PALETTE_BANK_SIZE {
        return false;
    }

    let mut informative_word_found = false;
    for offset in (0..GBA_PALETTE_BANK_SIZE).step_by(4) {
        let src_lo = &src[offset..offset + 2];
        let src_hi = &src[offset + 2..offset + 4];
        let dst_word = &dst[offset..offset + 4];

        if src_lo != src_hi {
            informative_word_found = true;
        }

        if dst_word[0] != src_lo[0]
            || dst_word[1] != src_lo[1]
            || dst_word[2] != src_lo[0]
            || dst_word[3] != src_lo[1]
        {
            return false;
        }
    }

    informative_word_found
}

fn bank_is_duplicated_low_halfwords(dst: &[u8]) -> bool {
    if dst.len() != GBA_PALETTE_BANK_SIZE {
        return false;
    }

    for offset in (0..GBA_PALETTE_BANK_SIZE).step_by(4) {
        if dst[offset] != dst[offset + 2] || dst[offset + 1] != dst[offset + 3] {
            return false;
        }
    }

    true
}

pub(crate) fn gba_palette_has_duplicated_banks(emu: &GbaEmulator) -> bool {
    emu.bus
        .ppu
        .palette
        .chunks_exact(GBA_PALETTE_BANK_SIZE)
        .any(bank_is_duplicated_low_halfwords)
}

pub(crate) fn repair_gba_palette_state_if_needed(emu: &mut GbaEmulator) -> bool {
    if emu.bus.ppu.palette.len() != GBA_PALETTE_RAM_SIZE
        || emu.bus.ewram.len() < GBA_PALETTE_RAM_SIZE * 2
    {
        return false;
    }

    let mut repaired_bank_offsets = Vec::new();
    let mut repaired_sources = Vec::new();

    let scan_start = (emu.bus.ewram.len() / 2).max(GBA_PALETTE_RAM_SIZE);
    let mut offset = (emu.bus.ewram.len() - GBA_PALETTE_RAM_SIZE) & !3;
    while offset >= scan_start {
        let src_start = offset - GBA_PALETTE_RAM_SIZE;

        for bank_offset in (0..GBA_PALETTE_RAM_SIZE).step_by(GBA_PALETTE_BANK_SIZE) {
            if repaired_bank_offsets.contains(&bank_offset) {
                continue;
            }

            let src_bank = &emu.bus.ewram[src_start + bank_offset..src_start + bank_offset + GBA_PALETTE_BANK_SIZE];
            let dst_bank = &emu.bus.ewram[offset + bank_offset..offset + bank_offset + GBA_PALETTE_BANK_SIZE];
            let palette_bank = &emu.bus.ppu.palette[bank_offset..bank_offset + GBA_PALETTE_BANK_SIZE];

            if bank_has_duplicated_low_halfwords(src_bank, dst_bank) && palette_bank == dst_bank {
                let src_bank = src_bank.to_vec();
                emu.bus.ewram[offset + bank_offset..offset + bank_offset + GBA_PALETTE_BANK_SIZE]
                    .copy_from_slice(&src_bank);
                emu.bus.ppu.palette[bank_offset..bank_offset + GBA_PALETTE_BANK_SIZE]
                    .copy_from_slice(&src_bank);
                repaired_bank_offsets.push(bank_offset);
                repaired_sources.push(offset);
            }
        }

        if offset < scan_start + 4 {
            break;
        }
        offset -= 4;
    }

    if !repaired_bank_offsets.is_empty() {
        log::debug!(
            "Repaired {} corrupted GBA palette banks from save state using EWRAM offsets {:?}",
            repaired_bank_offsets.len(),
            repaired_sources
                .iter()
                .map(|offset| format!("0x{:05X}", offset))
                .collect::<Vec<_>>()
        );
        return true;
    }

    false
}

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
    let mut emu: GbaEmulator =
        bincode::deserialize(&data).map_err(|e| format!("Deserialize error: {}", e))?;
    emu.repair_legacy_palette_state = repair_gba_palette_state_if_needed(&mut emu);
    log::info!("Loaded state from {}", path.display());
    Ok(emu)
}

#[cfg(test)]
mod tests {
    use super::{repair_gba_palette_state_if_needed, GBA_PALETTE_BANK_SIZE, GBA_PALETTE_RAM_SIZE};
    use crate::cartridge::GbaCartridge;
    use crate::emulator::gba::GbaEmulator;

    #[test]
    fn gba_state_palette_repair_restores_duplicated_banks() {
        let mut emu = GbaEmulator::new(GbaCartridge::load(vec![0; 0x200]));
        let match_offset = 0x30000usize;
        let src_start = match_offset - GBA_PALETTE_RAM_SIZE;

        for bank_offset in (0..GBA_PALETTE_RAM_SIZE).step_by(GBA_PALETTE_BANK_SIZE) {
            for word_offset in (0..GBA_PALETTE_BANK_SIZE).step_by(4) {
                let base = src_start + bank_offset + word_offset;
                let value = [
                    (bank_offset + word_offset) as u8,
                    (bank_offset + word_offset + 1) as u8,
                    (bank_offset + word_offset + 2) as u8,
                    (bank_offset + word_offset + 3) as u8,
                ];
                emu.bus.ewram[base..base + 4].copy_from_slice(&value);
                emu.bus.ewram[match_offset + bank_offset + word_offset] = value[0];
                emu.bus.ewram[match_offset + bank_offset + word_offset + 1] = value[1];
                emu.bus.ewram[match_offset + bank_offset + word_offset + 2] = value[0];
                emu.bus.ewram[match_offset + bank_offset + word_offset + 3] = value[1];
            }
        }

        emu.bus.ppu.palette.copy_from_slice(
            &emu.bus.ewram[match_offset..match_offset + GBA_PALETTE_RAM_SIZE],
        );

        assert!(repair_gba_palette_state_if_needed(&mut emu));

        assert_eq!(
            &emu.bus.ewram[match_offset..match_offset + GBA_PALETTE_RAM_SIZE],
            &emu.bus.ewram[src_start..src_start + GBA_PALETTE_RAM_SIZE]
        );
        assert_eq!(
            &emu.bus.ppu.palette[..],
            &emu.bus.ewram[src_start..src_start + GBA_PALETTE_RAM_SIZE]
        );
    }
}
