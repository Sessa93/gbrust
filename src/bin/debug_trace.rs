use std::fs;
use std::collections::{BTreeMap, BTreeSet};
use gbrust::cartridge::GbaCartridge;
use gbrust::cpu::arm7tdmi::Arm7Bus;
use gbrust::emulator::gba::GbaEmulator;
use gbrust::input::GbaKey;
use gbrust::save;

fn parse_press_arg(flag: &str) -> Option<GbaKey> {
    match flag {
        "--press-start" => Some(GbaKey::Start),
        "--press-a" => Some(GbaKey::A),
        "--press-up" => Some(GbaKey::Up),
        "--press-down" => Some(GbaKey::Down),
        "--press-left" => Some(GbaKey::Left),
        "--press-right" => Some(GbaKey::Right),
        "--press-l" => Some(GbaKey::L),
        "--press-r" => Some(GbaKey::R),
        _ => None,
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

fn obj_screen_pos(attr0: u16, attr1: u16) -> (i16, i16) {
    let y = (attr0 & 0x00FF) as i16;
    let x = (attr1 & 0x01FF) as i16;
    let screen_y = if y >= 160 { y - 256 } else { y };
    let screen_x = if x >= 240 { x - 512 } else { x };
    (screen_x, screen_y)
}

fn find_u16_pattern(memory: &[u8], pattern: &[u16]) -> Vec<usize> {
    if pattern.is_empty() || memory.len() < pattern.len() * 2 {
        return Vec::new();
    }

    let mut matches = Vec::new();
    for start in 0..=memory.len() - pattern.len() * 2 {
        let mut matched = true;
        for (index, expected) in pattern.iter().enumerate() {
            let offset = start + index * 2;
            let actual = (memory[offset] as u16) | ((memory[offset + 1] as u16) << 8);
            if actual != *expected {
                matched = false;
                break;
            }
        }
        if matched {
            matches.push(start);
        }
    }
    matches
}

fn dump_4bpp_obj(emu: &GbaEmulator, index: usize, attr0: u16, attr1: u16, attr2: u16) {
    if attr0 & 0x2000 != 0 {
        return;
    }

    let shape = (attr0 >> 14) & 3;
    let size = (attr1 >> 14) & 3;
    let (w, h) = obj_size(shape, size);
    let tile_num = (attr2 & 0x03FF) as usize;
    let palette_num = ((attr2 >> 12) & 0xF) as usize;
    let h_flip = attr1 & 0x1000 != 0;
    let v_flip = attr1 & 0x2000 != 0;
    let obj_mapping_1d = emu.bus.ppu.dispcnt & 0x40 != 0;

    println!("  OBJ{:03} decoded indices ({}x{}, pal {:X}):", index, w, h, palette_num);
    for y in 0..h {
        print!("    ");
        let src_y = if v_flip { h - 1 - y } else { y };
        for x in 0..w {
            let src_x = if h_flip { w - 1 - x } else { x };
            let tile_row = src_y / 8;
            let fine_y = src_y % 8;
            let tile_col = src_x / 8;
            let fine_x = src_x % 8;
            let tile_offset = if obj_mapping_1d {
                tile_num + tile_row * (w / 8) + tile_col
            } else {
                tile_num + tile_row * 32 + tile_col
            };
            let offset = 0x10000 + tile_offset * 32 + fine_y * 4 + fine_x / 2;
            let byte = emu.bus.ppu.vram[offset];
            let pal_idx = if fine_x & 1 == 0 { byte & 0xF } else { byte >> 4 };
            if pal_idx == 0 {
                print!(".");
            } else {
                print!("{:X}", pal_idx);
            }
        }
        println!();
    }
}

fn main() {
    let _ = env_logger::try_init();

    let mut args = std::env::args().skip(1);
    let rom_path = args
        .next()
        .unwrap_or_else(|| "roms/super-mario-advance-europe.gba".to_string());
    let remaining: Vec<String> = args.collect();
    let mut arg_index = 0;

    let mut target_cycles: u64 = 5_000_000;
    let mut target_frames: Option<u64> = None;
    let mut state_slot: Option<u8> = None;
    if remaining.get(arg_index).map(String::as_str) == Some("--frames") {
        target_frames = remaining
            .get(arg_index + 1)
            .and_then(|value| value.parse::<u64>().ok());
        arg_index += 2;
    } else if let Some(value) = remaining.get(arg_index) {
        if let Ok(cycles) = value.parse::<u64>() {
            target_cycles = cycles;
            arg_index += 1;
        }
    }

    if remaining.get(arg_index).map(String::as_str) == Some("--state-slot") {
        state_slot = remaining
            .get(arg_index + 1)
            .and_then(|value| value.parse().ok());
        arg_index += 2;
    }

    let mut scripted_presses = Vec::new();
    while let Some(flag) = remaining.get(arg_index) {
        let Some(key) = parse_press_arg(flag) else {
            break;
        };
        if let Some(frame) = remaining
            .get(arg_index + 1)
            .and_then(|value| value.parse::<u64>().ok())
        {
            let len = remaining
                .get(arg_index + 2)
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(2);
            scripted_presses.push((key, frame, len));
        }
        arg_index += 3;
    }

    let dump_path = remaining.get(arg_index).cloned();
    let data = fs::read(&rom_path).expect("Failed to read ROM");
    println!("ROM loaded: {} bytes", data.len());
    println!("ROM header bytes: {:02X} {:02X} {:02X} {:02X}", data[0], data[1], data[2], data[3]);

    let cart = GbaCartridge::load(data);
    println!("Title: {}", cart.title);
    println!("Backup: {:?}", cart.backup_type);

    let mut emu = GbaEmulator::new(cart);

    println!("\nInitial CPU state:");
    println!("  PC=0x{:08X} CPSR=0x{:08X} Thumb={}", 
        emu.cpu.regs[15], emu.cpu.cpsr, emu.cpu.thumb_mode());
    println!("  SP=0x{:08X} LR=0x{:08X}", emu.cpu.regs[13], emu.cpu.regs[14]);

    // Trace first 200 instructions
    println!("\n--- Instruction trace (first 200) ---");
    for i in 0..200 {
        let pc = emu.cpu.regs[15];
        let thumb = emu.cpu.thumb_mode();
        let instr = if thumb {
            emu.bus.read16(pc & !1) as u32
        } else {
            emu.bus.read32(pc & !3)
        };

        if thumb {
            print!("[{:4}] T 0x{:08X}: {:04X}", i, pc, instr);
        } else {
            print!("[{:4}] A 0x{:08X}: {:08X}", i, pc, instr);
        }

        // Check for IRQ before stepping
        if emu.bus.check_irq() {
            emu.cpu.handle_irq();
            println!(" -> IRQ!");
            continue;
        }

        let cycles = emu.cpu.step(&mut emu.bus);
        emu.bus.tick(cycles);

        // Print destination for branches/loads
        let new_pc = emu.cpu.regs[15];
        if new_pc != pc.wrapping_add(if thumb { 2 } else { 4 }) {
            println!(" -> PC=0x{:08X} (T={})", new_pc, emu.cpu.thumb_mode());
        } else {
            println!();
        }

        // Print register state every 50 instructions
        if (i + 1) % 50 == 0 {
            println!("  -- Regs: R0={:08X} R1={:08X} R2={:08X} R3={:08X}", 
                emu.cpu.regs[0], emu.cpu.regs[1], emu.cpu.regs[2], emu.cpu.regs[3]);
            println!("           R4={:08X} R5={:08X} R6={:08X} R7={:08X}",
                emu.cpu.regs[4], emu.cpu.regs[5], emu.cpu.regs[6], emu.cpu.regs[7]);
            println!("           R12={:08X} SP={:08X} LR={:08X} PC={:08X} CPSR={:08X}",
                emu.cpu.regs[12], emu.cpu.regs[13], emu.cpu.regs[14], emu.cpu.regs[15], emu.cpu.cpsr);
        }
    }

    // Now run multiple frames and check progress
    println!("\n--- Running multiple frames ---");
    let mut emu2 = if let Some(slot) = state_slot {
        save::load_gba_state(std::path::Path::new(&rom_path), slot)
            .unwrap_or_else(|err| panic!("Failed to load state slot {}: {}", slot, err))
    } else {
        let rom_data = fs::read(&rom_path).expect("Failed to read ROM");
        let cart = GbaCartridge::load(rom_data);
        GbaEmulator::new(cart)
    };

    let mut total_cycles: u64 = 0;
    
    if let Some(frames) = target_frames {
        for frame in 0..frames {
            for (key, start_frame, len) in &scripted_presses {
                if frame == *start_frame {
                    emu2.bus.input.key_down(*key);
                }
                if frame == *start_frame + *len {
                    emu2.bus.input.key_up(*key);
                }
            }
            emu2.run_frame();
        }
        total_cycles = emu2.cpu.cycles;
    } else {
        // Run a few million cycles to get past init
        while total_cycles < target_cycles {
            if emu2.bus.check_irq() {
                emu2.cpu.handle_irq();
            }
            let c = emu2.cpu.step(&mut emu2.bus);
            emu2.bus.tick(c);
            total_cycles += c as u64;
        }
    }

    if let Some(frames) = target_frames {
        println!("After {} frames ({} cycles): PC=0x{:08X} T={} CPSR=0x{:08X}", 
            frames,
            total_cycles,
            emu2.cpu.regs[15], emu2.cpu.thumb_mode(), emu2.cpu.cpsr);
    } else {
        println!("After {} cycles: PC=0x{:08X} T={} CPSR=0x{:08X}", 
            target_cycles,
            emu2.cpu.regs[15], emu2.cpu.thumb_mode(), emu2.cpu.cpsr);
    }
    println!("  HALTED={} DISPCNT=0x{:04X} DISPSTAT=0x{:04X} IME={} IE=0x{:04X} IF=0x{:04X}",
        emu2.cpu.halted,
        emu2.bus.ppu.dispcnt,
        emu2.bus.ppu.read_dispstat(),
        emu2.bus.ime,
        emu2.bus.ie,
        emu2.bus.iflag);
    println!(
        "  BLDCNT=0x{:04X} BLDALPHA=0x{:04X} BLDY=0x{:04X}",
        emu2.bus.ppu.bldcnt, emu2.bus.ppu.bldalpha, emu2.bus.ppu.bldy
    );
    println!(
        "  BGCNT=[{:04X}, {:04X}, {:04X}, {:04X}]",
        emu2.bus.ppu.bgcnt[0], emu2.bus.ppu.bgcnt[1], emu2.bus.ppu.bgcnt[2], emu2.bus.ppu.bgcnt[3]
    );
    println!(
        "  WINH=[{:04X}, {:04X}] WINV=[{:04X}, {:04X}] WININ=0x{:04X} WINOUT=0x{:04X}",
        emu2.bus.ppu.winh[0],
        emu2.bus.ppu.winh[1],
        emu2.bus.ppu.winv[0],
        emu2.bus.ppu.winv[1],
        emu2.bus.ppu.winin,
        emu2.bus.ppu.winout
    );
    println!(
        "  SOUNDCNT_H=0x{:04X} SOUNDBIAS=0x{:04X} FIFO_A={} FIFO_B={}",
        emu2.bus.apu.soundcnt_h,
        emu2.bus.apu.soundbias,
        emu2.bus.apu.fifo_a.len(),
        emu2.bus.apu.fifo_b.len()
    );
    print!("  BG palette[0..16]=");
    for i in 0..16usize {
        let lo = emu2.bus.ppu.palette[i * 2] as u16;
        let hi = emu2.bus.ppu.palette[i * 2 + 1] as u16;
        let color = lo | (hi << 8);
        print!(" {:04X}", color);
    }
    println!();
    print!("  OBJ palette[0..16]=");
    for i in 0..16usize {
        let base = 0x200 + i * 2;
        let lo = emu2.bus.ppu.palette[base] as u16;
        let hi = emu2.bus.ppu.palette[base + 1] as u16;
        let color = lo | (hi << 8);
        print!(" {:04X}", color);
    }
    println!();
    println!("  First visible OAM entries:");
    let mut printed = 0usize;
    let mut active_obj_palettes = BTreeSet::new();
    let mut visible_palette_samples = BTreeMap::new();
    for i in 0..128usize {
        let base = i * 8;
        let attr0 = (emu2.bus.ppu.oam[base] as u16) | ((emu2.bus.ppu.oam[base + 1] as u16) << 8);
        let attr1 = (emu2.bus.ppu.oam[base + 2] as u16) | ((emu2.bus.ppu.oam[base + 3] as u16) << 8);
        let attr2 = (emu2.bus.ppu.oam[base + 4] as u16) | ((emu2.bus.ppu.oam[base + 5] as u16) << 8);
        if attr0 == 0 && attr1 == 0 && attr2 == 0 {
            continue;
        }
        if attr0 & 0x2000 == 0 {
            active_obj_palettes.insert(((attr2 >> 12) & 0xF) as usize);

            let shape = (attr0 >> 14) & 3;
            let size = (attr1 >> 14) & 3;
            let (w, h) = obj_size(shape, size);
            let (x, y) = obj_screen_pos(attr0, attr1);
            let visible = x < 240 && x + w as i16 > 0 && y < 160 && y + h as i16 > 0;
            if visible && printed < 16 {
                println!(
                    "    OBJ{:03}: x={} y={} {}x{} pal={} attr0=0x{:04X} attr1=0x{:04X} attr2=0x{:04X}",
                    i,
                    x,
                    y,
                    w,
                    h,
                    (attr2 >> 12) & 0xF,
                    attr0,
                    attr1,
                    attr2
                );
                printed += 1;
            }
            if visible {
                visible_palette_samples
                    .entry(((attr2 >> 12) & 0xF) as usize)
                    .or_insert((i, attr0, attr1, attr2));
            }
        }
    }
    if printed == 0 {
        println!("    (no visible OAM entries)");
    }
    for pal in active_obj_palettes {
        print!("  OBJ palette bank {:X}[0..16]=", pal);
        for i in 0..16usize {
            let base = 0x200 + pal * 32 + i * 2;
            let lo = emu2.bus.ppu.palette[base] as u16;
            let hi = emu2.bus.ppu.palette[base + 1] as u16;
            let color = lo | (hi << 8);
            print!(" {:04X}", color);
        }
        println!();
    }

    let brendan_live_palette = [
        0x530E, 0x530E, 0x4B1F, 0x4B1F, 0x210F, 0x210F, 0x30E5, 0x30E5,
        0x1C82, 0x1C82, 0x2F1F, 0x2F1F, 0x2D9F, 0x2D9F, 0x7FFF, 0x7FFF,
    ];
    let brendan_reference_palette = [
        0x530E, 0x5B5F, 0x4B1F, 0x3A5B, 0x210F, 0x3D27, 0x30E5, 0x28A3,
        0x1C82, 0x779B, 0x2F1F, 0x2E77, 0x2D9F, 0x2118, 0x7FFF, 0x0000,
    ];
    let live_matches = find_u16_pattern(&emu2.bus.ewram, &brendan_live_palette);
    let reference_matches = find_u16_pattern(&emu2.bus.ewram, &brendan_reference_palette);
    println!(
        "  EWRAM Brendan duplicated palette matches: {:?}",
        live_matches.iter().take(8).map(|offset| format!("0x{:06X}", offset)).collect::<Vec<_>>()
    );
    println!(
        "  EWRAM Brendan reference palette matches: {:?}",
        reference_matches.iter().take(8).map(|offset| format!("0x{:06X}", offset)).collect::<Vec<_>>()
    );

    for (_palette, (index, attr0, attr1, attr2)) in visible_palette_samples {
        dump_4bpp_obj(&emu2, index, attr0, attr1, attr2);
    }
    
    // Now trace what's happening at the stuck address
    println!("\n--- Tracing around stuck point ---");
    // Collect PC values for the next 100 instructions to identify the loop
    let mut pcs: Vec<(u32, u32, bool)> = Vec::new();
    for _ in 0..100 {
        let pc = emu2.cpu.regs[15];
        let thumb = emu2.cpu.thumb_mode();
        let instr = if thumb {
            emu2.bus.read16(pc & !1) as u32
        } else {
            emu2.bus.read32(pc & !3)
        };
        pcs.push((pc, instr, thumb));

        if emu2.bus.check_irq() {
            emu2.cpu.handle_irq();
        } else {
            emu2.cpu.step(&mut emu2.bus);
        }
    }

    // Print unique PCs to find the loop
    for (pc, instr, thumb) in &pcs {
        if *thumb {
            println!("  T 0x{:08X}: {:04X}", pc, instr);
        } else {
            println!("  A 0x{:08X}: {:08X}", pc, instr);
        }
    }

    // Also dump some key memory/state
    println!("\nKey registers:");
    for i in 0..16 {
        println!("  R{:2}=0x{:08X}", i, emu2.cpu.regs[i]);
    }

    // Check what's at the ROM address region the loop is reading
    println!("\nROM bytes at 0x080B6A80-0x080B6AA0:");
    for addr in (0x080B6A80u32..0x080B6AA0).step_by(2) {
        let val = emu2.bus.read16(addr);
        print!("{:04X} ", val);
    }
    println!();

    // Check what VCOUNT is doing
    println!("\nPPU VCOUNT after cycles: {}", emu2.bus.ppu.vcount);

    if let Some(path) = dump_path {
        let width = 240usize;
        let height = 160usize;
        let mut ppm = format!("P6\n{} {}\n255\n", width, height).into_bytes();
        for &argb in &emu2.bus.ppu.framebuffer {
            ppm.push(((argb >> 16) & 0xFF) as u8);
            ppm.push(((argb >> 8) & 0xFF) as u8);
            ppm.push((argb & 0xFF) as u8);
        }
        fs::write(&path, ppm).expect("Failed to write framebuffer dump");
        println!("Framebuffer dumped to {}", path);
    }
}
