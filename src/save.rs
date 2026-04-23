use std::fs;
use std::path::{Path, PathBuf};

use crate::emulator::gba::GbaEmulator;
use crate::emulator::gbc::GbcEmulator;

const GBA_PALETTE_RAM_SIZE: usize = 0x400;
const GBA_PALETTE_BANK_SIZE: usize = 0x20;
const GBA_FLASH_SECTOR_SIZE: usize = 0x1000;
const POKEMON_GBA_SAVE_SIGNATURE: u32 = 0x0801_2025;
const POKEMON_GBA_SECTION_COUNT: usize = 14;

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

fn bank_is_informatively_duplicated_low_halfwords(dst: &[u8]) -> bool {
    if dst.len() != GBA_PALETTE_BANK_SIZE {
        return false;
    }

    let mut has_non_zero_halfword = false;
    let mut distinct_halfwords = false;
    let mut previous_halfword = None;

    for offset in (0..GBA_PALETTE_BANK_SIZE).step_by(4) {
        if dst[offset] != dst[offset + 2] || dst[offset + 1] != dst[offset + 3] {
            return false;
        }

        let halfword = [dst[offset], dst[offset + 1]];
        if halfword != [0, 0] {
            has_non_zero_halfword = true;
        }
        if let Some(previous) = previous_halfword {
            if previous != halfword {
                distinct_halfwords = true;
            }
        } else {
            previous_halfword = Some(halfword);
        }
    }

    has_non_zero_halfword && distinct_halfwords
}

pub(crate) fn gba_palette_has_duplicated_banks(emu: &GbaEmulator) -> bool {
    emu.bus
        .ppu
        .palette
        .chunks_exact(GBA_PALETTE_BANK_SIZE)
    .any(bank_is_informatively_duplicated_low_halfwords)
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

fn gba_backup_is_incomplete_pokemon_flash_save(data: &[u8]) -> bool {
    if data.len() < GBA_FLASH_SECTOR_SIZE {
        return false;
    }

    let mut has_pokemon_footer = false;
    let mut section_masks = Vec::<(u32, u16)>::new();
    let complete_mask = (1u16 << POKEMON_GBA_SECTION_COUNT) - 1;

    for sector in data.chunks_exact(GBA_FLASH_SECTOR_SIZE) {
        let footer = &sector[GBA_FLASH_SECTOR_SIZE - 12..];
        let signature = u32::from_le_bytes([footer[4], footer[5], footer[6], footer[7]]);
        if signature != POKEMON_GBA_SAVE_SIGNATURE {
            continue;
        }

        has_pokemon_footer = true;

        let section_id = u16::from_le_bytes([footer[0], footer[1]]) as usize;
        if section_id >= POKEMON_GBA_SECTION_COUNT {
            continue;
        }

        let save_index = u32::from_le_bytes([footer[8], footer[9], footer[10], footer[11]]);
        if let Some((_, mask)) = section_masks.iter_mut().find(|(index, _)| *index == save_index) {
            *mask |= 1u16 << section_id;
        } else {
            section_masks.push((save_index, 1u16 << section_id));
        }
    }

    has_pokemon_footer && !section_masks.iter().any(|(_, mask)| *mask == complete_mask)
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
    let data: &[u8] = match emu.bus.cart.backup_type {
        GbaBackupType::Sram => &emu.bus.cart.sram,
        GbaBackupType::Flash64k | GbaBackupType::Flash128k => &emu.bus.cart.flash,
        GbaBackupType::Eeprom => &emu.bus.cart.eeprom[..emu.bus.cart.eeprom_size()],
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
            if emu.bus.cart.backup_type == GbaBackupType::Flash128k
                && gba_backup_is_incomplete_pokemon_flash_save(&data)
            {
                log::warn!(
                    "Ignoring legacy incomplete Pokemon-style flash save at {}",
                    path.display()
                );
                return;
            }

            let target: &mut [u8] = match emu.bus.cart.backup_type {
                GbaBackupType::Sram => &mut emu.bus.cart.sram,
                GbaBackupType::Flash64k | GbaBackupType::Flash128k => &mut emu.bus.cart.flash,
                GbaBackupType::Eeprom => {
                    if let Some(addr_len) = emu.bus.cart.forced_eeprom_addr_len() {
                        emu.bus.cart.eeprom_addr_len = addr_len;
                        let size = emu.bus.cart.eeprom_size();
                        &mut emu.bus.cart.eeprom[..size]
                    } else {
                        emu.bus.cart.eeprom_addr_len = if data.len() > 0x200 { 14 } else { 6 };
                        let size = emu.bus.cart.eeprom_size();
                        &mut emu.bus.cart.eeprom[..size]
                    }
                }
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
    use super::{
        bank_is_informatively_duplicated_low_halfwords,
        gba_backup_is_incomplete_pokemon_flash_save,
        repair_gba_palette_state_if_needed,
        GBA_FLASH_SECTOR_SIZE,
        GBA_PALETTE_BANK_SIZE,
        GBA_PALETTE_RAM_SIZE,
        POKEMON_GBA_SAVE_SIGNATURE,
    };
    use crate::cartridge::GbaCartridge;
    use crate::emulator::gba::GbaEmulator;

    #[test]
    fn duplicated_bank_detector_ignores_zeroed_bank() {
        assert!(!bank_is_informatively_duplicated_low_halfwords(
            &[0; GBA_PALETTE_BANK_SIZE]
        ));
    }

    #[test]
    fn duplicated_bank_detector_accepts_varied_duplicated_bank() {
        let mut bank = [0u8; GBA_PALETTE_BANK_SIZE];
        for (index, value) in [0x530E_u16, 0x4AFB, 0x212D, 0x7FFF].into_iter().enumerate() {
            let offset = index * 4;
            let bytes = value.to_le_bytes();
            bank[offset..offset + 2].copy_from_slice(&bytes);
            bank[offset + 2..offset + 4].copy_from_slice(&bytes);
        }

        assert!(bank_is_informatively_duplicated_low_halfwords(&bank));
    }

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

    fn write_pokemon_flash_footer(data: &mut [u8], sector: usize, section_id: u16, save_index: u32) {
        let footer = &mut data[sector * GBA_FLASH_SECTOR_SIZE + GBA_FLASH_SECTOR_SIZE - 12
            ..sector * GBA_FLASH_SECTOR_SIZE + GBA_FLASH_SECTOR_SIZE];
        footer[0..2].copy_from_slice(&section_id.to_le_bytes());
        footer[2..4].copy_from_slice(&0u16.to_le_bytes());
        footer[4..8].copy_from_slice(&POKEMON_GBA_SAVE_SIGNATURE.to_le_bytes());
        footer[8..12].copy_from_slice(&save_index.to_le_bytes());
    }

    #[test]
    fn gba_flash_pokemon_complete_section_set_is_not_flagged_incomplete() {
        let mut data = vec![0xFF; 0x20000];
        for section_id in 0..14u16 {
            write_pokemon_flash_footer(&mut data, section_id as usize, section_id, 7);
        }

        assert!(!gba_backup_is_incomplete_pokemon_flash_save(&data));
    }

    #[test]
    fn gba_flash_pokemon_incomplete_section_set_is_flagged() {
        let mut data = vec![0xFF; 0x20000];
        for (sector, section_id) in [(6usize, 13u16), (7, 0), (16, 9), (17, 10), (18, 11), (19, 12), (20, 5), (21, 6), (22, 7), (23, 8)] {
            write_pokemon_flash_footer(&mut data, sector, section_id, 1);
        }

        assert!(gba_backup_is_incomplete_pokemon_flash_save(&data));
    }
}
