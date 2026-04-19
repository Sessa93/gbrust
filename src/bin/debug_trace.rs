use std::fs;
use gbrust::cartridge::GbaCartridge;
use gbrust::cpu::arm7tdmi::Arm7Bus;
use gbrust::emulator::gba::GbaEmulator;

fn main() {
    let rom_path = "roms/super-mario-advance-europe.gba";
    let data = fs::read(rom_path).expect("Failed to read ROM");
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
    let mut emu2 = {
        let rom_data = fs::read(rom_path).expect("Failed to read ROM");
        let cart = GbaCartridge::load(rom_data);
        GbaEmulator::new(cart)
    };

    // Run until stuck or interesting, then trace
    const CYCLES_PER_FRAME: u32 = 280896;
    let mut total_cycles: u64 = 0;
    
    // Run a few million cycles to get past init
    while total_cycles < 5_000_000 {
        if emu2.bus.check_irq() {
            emu2.cpu.handle_irq();
        }
        let c = emu2.cpu.step(&mut emu2.bus);
        emu2.bus.tick(c);
        total_cycles += c as u64;
    }

    println!("After 5M cycles: PC=0x{:08X} T={} CPSR=0x{:08X}", 
        emu2.cpu.regs[15], emu2.cpu.thumb_mode(), emu2.cpu.cpsr);
    println!("  DISPCNT=0x{:04X} IME={} IE=0x{:04X} IF=0x{:04X}",
        emu2.bus.ppu.dispcnt, emu2.bus.ime, emu2.bus.ie, emu2.bus.iflag);
    
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
}
