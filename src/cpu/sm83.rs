use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sm83 {
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub sp: u16,
    pub pc: u16,
    pub ime: bool,
    pub ime_pending: bool,
    pub halted: bool,
    pub stopped: bool,
    pub cycles: u64,
}

const FLAG_Z: u8 = 0x80;
const FLAG_N: u8 = 0x40;
const FLAG_H: u8 = 0x20;
const FLAG_C: u8 = 0x10;

pub trait Sm83Bus {
    fn read(&self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, val: u8);
}

impl Sm83 {
    pub fn new() -> Self {
        Self {
            a: 0x11,
            f: 0xB0,
            b: 0x00,
            c: 0x13,
            d: 0x00,
            e: 0xD8,
            h: 0x01,
            l: 0x4D,
            sp: 0xFFFE,
            pc: 0x0100,
            ime: false,
            ime_pending: false,
            halted: false,
            stopped: false,
            cycles: 0,
        }
    }

    fn af(&self) -> u16 {
        (self.a as u16) << 8 | (self.f & 0xF0) as u16
    }
    fn bc(&self) -> u16 {
        (self.b as u16) << 8 | self.c as u16
    }
    fn de(&self) -> u16 {
        (self.d as u16) << 8 | self.e as u16
    }
    fn hl(&self) -> u16 {
        (self.h as u16) << 8 | self.l as u16
    }

    fn set_af(&mut self, v: u16) {
        self.a = (v >> 8) as u8;
        self.f = (v & 0xF0) as u8;
    }
    fn set_bc(&mut self, v: u16) {
        self.b = (v >> 8) as u8;
        self.c = v as u8;
    }
    fn set_de(&mut self, v: u16) {
        self.d = (v >> 8) as u8;
        self.e = v as u8;
    }
    fn set_hl(&mut self, v: u16) {
        self.h = (v >> 8) as u8;
        self.l = v as u8;
    }

    fn flag(&self, f: u8) -> bool {
        self.f & f != 0
    }
    fn set_flag(&mut self, f: u8, v: bool) {
        if v {
            self.f |= f;
        } else {
            self.f &= !f;
        }
        self.f &= 0xF0;
    }

    fn read_byte(&mut self, bus: &impl Sm83Bus) -> u8 {
        let v = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }

    fn read_word(&mut self, bus: &impl Sm83Bus) -> u16 {
        let lo = bus.read(self.pc) as u16;
        let hi = bus.read(self.pc.wrapping_add(1)) as u16;
        self.pc = self.pc.wrapping_add(2);
        lo | (hi << 8)
    }

    fn push(&mut self, bus: &mut impl Sm83Bus, val: u16) {
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, (val >> 8) as u8);
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, val as u8);
    }

    fn pop(&mut self, bus: &impl Sm83Bus) -> u16 {
        let lo = bus.read(self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        let hi = bus.read(self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        lo | (hi << 8)
    }

    // ALU operations
    fn alu_add(&mut self, val: u8) {
        let a = self.a;
        let r = a.wrapping_add(val);
        self.set_flag(FLAG_Z, r == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, (a & 0x0F) + (val & 0x0F) > 0x0F);
        self.set_flag(FLAG_C, (a as u16) + (val as u16) > 0xFF);
        self.a = r;
    }

    fn alu_adc(&mut self, val: u8) {
        let a = self.a;
        let c = if self.flag(FLAG_C) { 1u8 } else { 0 };
        let r = a.wrapping_add(val).wrapping_add(c);
        self.set_flag(FLAG_Z, r == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, (a & 0x0F) + (val & 0x0F) + c > 0x0F);
        self.set_flag(FLAG_C, (a as u16) + (val as u16) + (c as u16) > 0xFF);
        self.a = r;
    }

    fn alu_sub(&mut self, val: u8) {
        let a = self.a;
        let r = a.wrapping_sub(val);
        self.set_flag(FLAG_Z, r == 0);
        self.set_flag(FLAG_N, true);
        self.set_flag(FLAG_H, (a & 0x0F) < (val & 0x0F));
        self.set_flag(FLAG_C, a < val);
        self.a = r;
    }

    fn alu_sbc(&mut self, val: u8) {
        let a = self.a;
        let c = if self.flag(FLAG_C) { 1u8 } else { 0 };
        let r = a.wrapping_sub(val).wrapping_sub(c);
        self.set_flag(FLAG_Z, r == 0);
        self.set_flag(FLAG_N, true);
        self.set_flag(FLAG_H, (a & 0x0F) < (val & 0x0F) + c);
        self.set_flag(FLAG_C, (a as u16) < (val as u16) + (c as u16));
        self.a = r;
    }

    fn alu_and(&mut self, val: u8) {
        self.a &= val;
        self.set_flag(FLAG_Z, self.a == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, true);
        self.set_flag(FLAG_C, false);
    }

    fn alu_xor(&mut self, val: u8) {
        self.a ^= val;
        self.set_flag(FLAG_Z, self.a == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, false);
        self.set_flag(FLAG_C, false);
    }

    fn alu_or(&mut self, val: u8) {
        self.a |= val;
        self.set_flag(FLAG_Z, self.a == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, false);
        self.set_flag(FLAG_C, false);
    }

    fn alu_cp(&mut self, val: u8) {
        let a = self.a;
        self.alu_sub(val);
        self.a = a;
    }

    fn alu_inc(&mut self, val: u8) -> u8 {
        let r = val.wrapping_add(1);
        self.set_flag(FLAG_Z, r == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, (val & 0x0F) + 1 > 0x0F);
        r
    }

    fn alu_dec(&mut self, val: u8) -> u8 {
        let r = val.wrapping_sub(1);
        self.set_flag(FLAG_Z, r == 0);
        self.set_flag(FLAG_N, true);
        self.set_flag(FLAG_H, (val & 0x0F) == 0);
        r
    }

    fn alu_add16(&mut self, val: u16) {
        let hl = self.hl();
        let r = hl.wrapping_add(val);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, (hl & 0x0FFF) + (val & 0x0FFF) > 0x0FFF);
        self.set_flag(FLAG_C, (hl as u32) + (val as u32) > 0xFFFF);
        self.set_hl(r);
    }

    fn alu_add_sp_e8(&mut self, bus: &impl Sm83Bus) -> u16 {
        let e = self.read_byte(bus) as i8 as i16 as u16;
        let sp = self.sp;
        self.set_flag(FLAG_Z, false);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, (sp & 0x0F) + (e & 0x0F) > 0x0F);
        self.set_flag(FLAG_C, (sp & 0xFF) + (e & 0xFF) > 0xFF);
        sp.wrapping_add(e)
    }

    // Rotate/shift operations
    fn rlc(&mut self, val: u8) -> u8 {
        let c = val >> 7;
        let r = (val << 1) | c;
        self.set_flag(FLAG_Z, r == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, false);
        self.set_flag(FLAG_C, c != 0);
        r
    }

    fn rrc(&mut self, val: u8) -> u8 {
        let c = val & 1;
        let r = (val >> 1) | (c << 7);
        self.set_flag(FLAG_Z, r == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, false);
        self.set_flag(FLAG_C, c != 0);
        r
    }

    fn rl(&mut self, val: u8) -> u8 {
        let old_c = if self.flag(FLAG_C) { 1 } else { 0 };
        let c = val >> 7;
        let r = (val << 1) | old_c;
        self.set_flag(FLAG_Z, r == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, false);
        self.set_flag(FLAG_C, c != 0);
        r
    }

    fn rr(&mut self, val: u8) -> u8 {
        let old_c = if self.flag(FLAG_C) { 1u8 } else { 0 };
        let c = val & 1;
        let r = (val >> 1) | (old_c << 7);
        self.set_flag(FLAG_Z, r == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, false);
        self.set_flag(FLAG_C, c != 0);
        r
    }

    fn sla(&mut self, val: u8) -> u8 {
        let c = val >> 7;
        let r = val << 1;
        self.set_flag(FLAG_Z, r == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, false);
        self.set_flag(FLAG_C, c != 0);
        r
    }

    fn sra(&mut self, val: u8) -> u8 {
        let c = val & 1;
        let r = (val >> 1) | (val & 0x80);
        self.set_flag(FLAG_Z, r == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, false);
        self.set_flag(FLAG_C, c != 0);
        r
    }

    fn swap(&mut self, val: u8) -> u8 {
        let r = (val >> 4) | (val << 4);
        self.set_flag(FLAG_Z, r == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, false);
        self.set_flag(FLAG_C, false);
        r
    }

    fn srl(&mut self, val: u8) -> u8 {
        let c = val & 1;
        let r = val >> 1;
        self.set_flag(FLAG_Z, r == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, false);
        self.set_flag(FLAG_C, c != 0);
        r
    }

    fn bit(&mut self, bit: u8, val: u8) {
        self.set_flag(FLAG_Z, val & (1 << bit) == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, true);
    }

    fn res(bit: u8, val: u8) -> u8 {
        val & !(1 << bit)
    }

    fn set(bit: u8, val: u8) -> u8 {
        val | (1 << bit)
    }

    fn get_reg(&self, idx: u8, bus: &impl Sm83Bus) -> u8 {
        match idx {
            0 => self.b,
            1 => self.c,
            2 => self.d,
            3 => self.e,
            4 => self.h,
            5 => self.l,
            6 => bus.read(self.hl()),
            7 => self.a,
            _ => unreachable!(),
        }
    }

    fn set_reg(&mut self, idx: u8, val: u8, bus: &mut impl Sm83Bus) {
        match idx {
            0 => self.b = val,
            1 => self.c = val,
            2 => self.d = val,
            3 => self.e = val,
            4 => self.h = val,
            5 => self.l = val,
            6 => bus.write(self.hl(), val),
            7 => self.a = val,
            _ => unreachable!(),
        }
    }

    pub fn handle_interrupts(&mut self, bus: &mut impl Sm83Bus) -> u32 {
        let ie = bus.read(0xFFFF);
        let iflag = bus.read(0xFF0F);
        let pending = ie & iflag & 0x1F;

        if pending != 0 {
            self.halted = false;
        }

        if !self.ime || pending == 0 {
            return 0;
        }

        self.ime = false;

        let bit = pending.trailing_zeros() as u8;
        bus.write(0xFF0F, iflag & !(1 << bit));

        self.push(bus, self.pc);
        self.pc = 0x0040 + (bit as u16) * 8;
        20
    }

    pub fn step(&mut self, bus: &mut impl Sm83Bus) -> u32 {
        if self.ime_pending {
            self.ime_pending = false;
            self.ime = true;
        }

        let int_cycles = self.handle_interrupts(bus);
        if int_cycles > 0 {
            self.cycles += int_cycles as u64;
            return int_cycles;
        }

        if self.halted {
            self.cycles += 4;
            return 4;
        }

        let opcode = self.read_byte(bus);
        let cycles = self.execute(opcode, bus);
        self.cycles += cycles as u64;
        cycles
    }

    fn execute(&mut self, opcode: u8, bus: &mut impl Sm83Bus) -> u32 {
        match opcode {
            // NOP
            0x00 => 4,
            // LD BC,d16
            0x01 => { let v = self.read_word(bus); self.set_bc(v); 12 }
            // LD (BC),A
            0x02 => { bus.write(self.bc(), self.a); 8 }
            // INC BC
            0x03 => { let v = self.bc().wrapping_add(1); self.set_bc(v); 8 }
            // INC B
            0x04 => { self.b = self.alu_inc(self.b); 4 }
            // DEC B
            0x05 => { self.b = self.alu_dec(self.b); 4 }
            // LD B,d8
            0x06 => { self.b = self.read_byte(bus); 8 }
            // RLCA
            0x07 => {
                self.a = self.rlc(self.a);
                self.set_flag(FLAG_Z, false);
                4
            }
            // LD (a16),SP
            0x08 => {
                let addr = self.read_word(bus);
                bus.write(addr, self.sp as u8);
                bus.write(addr.wrapping_add(1), (self.sp >> 8) as u8);
                20
            }
            // ADD HL,BC
            0x09 => { self.alu_add16(self.bc()); 8 }
            // LD A,(BC)
            0x0A => { self.a = bus.read(self.bc()); 8 }
            // DEC BC
            0x0B => { let v = self.bc().wrapping_sub(1); self.set_bc(v); 8 }
            // INC C
            0x0C => { self.c = self.alu_inc(self.c); 4 }
            // DEC C
            0x0D => { self.c = self.alu_dec(self.c); 4 }
            // LD C,d8
            0x0E => { self.c = self.read_byte(bus); 8 }
            // RRCA
            0x0F => {
                self.a = self.rrc(self.a);
                self.set_flag(FLAG_Z, false);
                4
            }
            // STOP
            0x10 => { self.stopped = true; self.pc = self.pc.wrapping_add(1); 4 }
            // LD DE,d16
            0x11 => { let v = self.read_word(bus); self.set_de(v); 12 }
            // LD (DE),A
            0x12 => { bus.write(self.de(), self.a); 8 }
            // INC DE
            0x13 => { let v = self.de().wrapping_add(1); self.set_de(v); 8 }
            // INC D
            0x14 => { self.d = self.alu_inc(self.d); 4 }
            // DEC D
            0x15 => { self.d = self.alu_dec(self.d); 4 }
            // LD D,d8
            0x16 => { self.d = self.read_byte(bus); 8 }
            // RLA
            0x17 => {
                self.a = self.rl(self.a);
                self.set_flag(FLAG_Z, false);
                4
            }
            // JR r8
            0x18 => {
                let e = self.read_byte(bus) as i8;
                self.pc = self.pc.wrapping_add(e as u16);
                12
            }
            // ADD HL,DE
            0x19 => { self.alu_add16(self.de()); 8 }
            // LD A,(DE)
            0x1A => { self.a = bus.read(self.de()); 8 }
            // DEC DE
            0x1B => { let v = self.de().wrapping_sub(1); self.set_de(v); 8 }
            // INC E
            0x1C => { self.e = self.alu_inc(self.e); 4 }
            // DEC E
            0x1D => { self.e = self.alu_dec(self.e); 4 }
            // LD E,d8
            0x1E => { self.e = self.read_byte(bus); 8 }
            // RRA
            0x1F => {
                self.a = self.rr(self.a);
                self.set_flag(FLAG_Z, false);
                4
            }
            // JR NZ,r8
            0x20 => {
                let e = self.read_byte(bus) as i8;
                if !self.flag(FLAG_Z) {
                    self.pc = self.pc.wrapping_add(e as u16);
                    12
                } else {
                    8
                }
            }
            // LD HL,d16
            0x21 => { let v = self.read_word(bus); self.set_hl(v); 12 }
            // LD (HL+),A
            0x22 => {
                let hl = self.hl();
                bus.write(hl, self.a);
                self.set_hl(hl.wrapping_add(1));
                8
            }
            // INC HL
            0x23 => { let v = self.hl().wrapping_add(1); self.set_hl(v); 8 }
            // INC H
            0x24 => { self.h = self.alu_inc(self.h); 4 }
            // DEC H
            0x25 => { self.h = self.alu_dec(self.h); 4 }
            // LD H,d8
            0x26 => { self.h = self.read_byte(bus); 8 }
            // DAA
            0x27 => {
                let mut a = self.a as u16;
                if !self.flag(FLAG_N) {
                    if self.flag(FLAG_H) || (a & 0x0F) > 9 {
                        a += 0x06;
                    }
                    if self.flag(FLAG_C) || a > 0x9F {
                        a += 0x60;
                    }
                } else {
                    if self.flag(FLAG_H) {
                        a = a.wrapping_sub(6) & 0xFF;
                    }
                    if self.flag(FLAG_C) {
                        a = a.wrapping_sub(0x60);
                    }
                }
                self.set_flag(FLAG_H, false);
                if a & 0x100 != 0 {
                    self.set_flag(FLAG_C, true);
                }
                self.a = a as u8;
                self.set_flag(FLAG_Z, self.a == 0);
                4
            }
            // JR Z,r8
            0x28 => {
                let e = self.read_byte(bus) as i8;
                if self.flag(FLAG_Z) {
                    self.pc = self.pc.wrapping_add(e as u16);
                    12
                } else {
                    8
                }
            }
            // ADD HL,HL
            0x29 => { let hl = self.hl(); self.alu_add16(hl); 8 }
            // LD A,(HL+)
            0x2A => {
                let hl = self.hl();
                self.a = bus.read(hl);
                self.set_hl(hl.wrapping_add(1));
                8
            }
            // DEC HL
            0x2B => { let v = self.hl().wrapping_sub(1); self.set_hl(v); 8 }
            // INC L
            0x2C => { self.l = self.alu_inc(self.l); 4 }
            // DEC L
            0x2D => { self.l = self.alu_dec(self.l); 4 }
            // LD L,d8
            0x2E => { self.l = self.read_byte(bus); 8 }
            // CPL
            0x2F => {
                self.a = !self.a;
                self.set_flag(FLAG_N, true);
                self.set_flag(FLAG_H, true);
                4
            }
            // JR NC,r8
            0x30 => {
                let e = self.read_byte(bus) as i8;
                if !self.flag(FLAG_C) {
                    self.pc = self.pc.wrapping_add(e as u16);
                    12
                } else {
                    8
                }
            }
            // LD SP,d16
            0x31 => { self.sp = self.read_word(bus); 12 }
            // LD (HL-),A
            0x32 => {
                let hl = self.hl();
                bus.write(hl, self.a);
                self.set_hl(hl.wrapping_sub(1));
                8
            }
            // INC SP
            0x33 => { self.sp = self.sp.wrapping_add(1); 8 }
            // INC (HL)
            0x34 => {
                let hl = self.hl();
                let v = self.alu_inc(bus.read(hl));
                bus.write(hl, v);
                12
            }
            // DEC (HL)
            0x35 => {
                let hl = self.hl();
                let v = self.alu_dec(bus.read(hl));
                bus.write(hl, v);
                12
            }
            // LD (HL),d8
            0x36 => { let v = self.read_byte(bus); bus.write(self.hl(), v); 12 }
            // SCF
            0x37 => {
                self.set_flag(FLAG_N, false);
                self.set_flag(FLAG_H, false);
                self.set_flag(FLAG_C, true);
                4
            }
            // JR C,r8
            0x38 => {
                let e = self.read_byte(bus) as i8;
                if self.flag(FLAG_C) {
                    self.pc = self.pc.wrapping_add(e as u16);
                    12
                } else {
                    8
                }
            }
            // ADD HL,SP
            0x39 => { self.alu_add16(self.sp); 8 }
            // LD A,(HL-)
            0x3A => {
                let hl = self.hl();
                self.a = bus.read(hl);
                self.set_hl(hl.wrapping_sub(1));
                8
            }
            // DEC SP
            0x3B => { self.sp = self.sp.wrapping_sub(1); 8 }
            // INC A
            0x3C => { self.a = self.alu_inc(self.a); 4 }
            // DEC A
            0x3D => { self.a = self.alu_dec(self.a); 4 }
            // LD A,d8
            0x3E => { self.a = self.read_byte(bus); 8 }
            // CCF
            0x3F => {
                self.set_flag(FLAG_N, false);
                self.set_flag(FLAG_H, false);
                let c = self.flag(FLAG_C);
                self.set_flag(FLAG_C, !c);
                4
            }

            // LD B,r8 (0x40-0x47)
            0x40..=0x47 => {
                let src = opcode & 0x07;
                let v = self.get_reg(src, bus);
                self.b = v;
                if src == 6 { 8 } else { 4 }
            }
            // LD C,r8 (0x48-0x4F)
            0x48..=0x4F => {
                let src = opcode & 0x07;
                let v = self.get_reg(src, bus);
                self.c = v;
                if src == 6 { 8 } else { 4 }
            }
            // LD D,r8 (0x50-0x57)
            0x50..=0x57 => {
                let src = opcode & 0x07;
                let v = self.get_reg(src, bus);
                self.d = v;
                if src == 6 { 8 } else { 4 }
            }
            // LD E,r8 (0x58-0x5F)
            0x58..=0x5F => {
                let src = opcode & 0x07;
                let v = self.get_reg(src, bus);
                self.e = v;
                if src == 6 { 8 } else { 4 }
            }
            // LD H,r8 (0x60-0x67)
            0x60..=0x67 => {
                let src = opcode & 0x07;
                let v = self.get_reg(src, bus);
                self.h = v;
                if src == 6 { 8 } else { 4 }
            }
            // LD L,r8 (0x68-0x6F)
            0x68..=0x6F => {
                let src = opcode & 0x07;
                let v = self.get_reg(src, bus);
                self.l = v;
                if src == 6 { 8 } else { 4 }
            }
            // LD (HL),r8 (0x70-0x75, 0x77)
            0x70..=0x75 | 0x77 => {
                let src = opcode & 0x07;
                let v = self.get_reg(src, bus);
                bus.write(self.hl(), v);
                8
            }
            // HALT
            0x76 => { self.halted = true; 4 }
            // LD A,r8 (0x78-0x7F)
            0x78..=0x7F => {
                let src = opcode & 0x07;
                let v = self.get_reg(src, bus);
                self.a = v;
                if src == 6 { 8 } else { 4 }
            }

            // ADD A,r8 (0x80-0x87)
            0x80..=0x87 => {
                let src = opcode & 0x07;
                let v = self.get_reg(src, bus);
                self.alu_add(v);
                if src == 6 { 8 } else { 4 }
            }
            // ADC A,r8 (0x88-0x8F)
            0x88..=0x8F => {
                let src = opcode & 0x07;
                let v = self.get_reg(src, bus);
                self.alu_adc(v);
                if src == 6 { 8 } else { 4 }
            }
            // SUB r8 (0x90-0x97)
            0x90..=0x97 => {
                let src = opcode & 0x07;
                let v = self.get_reg(src, bus);
                self.alu_sub(v);
                if src == 6 { 8 } else { 4 }
            }
            // SBC A,r8 (0x98-0x9F)
            0x98..=0x9F => {
                let src = opcode & 0x07;
                let v = self.get_reg(src, bus);
                self.alu_sbc(v);
                if src == 6 { 8 } else { 4 }
            }
            // AND r8 (0xA0-0xA7)
            0xA0..=0xA7 => {
                let src = opcode & 0x07;
                let v = self.get_reg(src, bus);
                self.alu_and(v);
                if src == 6 { 8 } else { 4 }
            }
            // XOR r8 (0xA8-0xAF)
            0xA8..=0xAF => {
                let src = opcode & 0x07;
                let v = self.get_reg(src, bus);
                self.alu_xor(v);
                if src == 6 { 8 } else { 4 }
            }
            // OR r8 (0xB0-0xB7)
            0xB0..=0xB7 => {
                let src = opcode & 0x07;
                let v = self.get_reg(src, bus);
                self.alu_or(v);
                if src == 6 { 8 } else { 4 }
            }
            // CP r8 (0xB8-0xBF)
            0xB8..=0xBF => {
                let src = opcode & 0x07;
                let v = self.get_reg(src, bus);
                self.alu_cp(v);
                if src == 6 { 8 } else { 4 }
            }

            // RET NZ
            0xC0 => {
                if !self.flag(FLAG_Z) {
                    self.pc = self.pop(bus);
                    20
                } else {
                    8
                }
            }
            // POP BC
            0xC1 => { let v = self.pop(bus); self.set_bc(v); 12 }
            // JP NZ,a16
            0xC2 => {
                let addr = self.read_word(bus);
                if !self.flag(FLAG_Z) {
                    self.pc = addr;
                    16
                } else {
                    12
                }
            }
            // JP a16
            0xC3 => { self.pc = self.read_word(bus); 16 }
            // CALL NZ,a16
            0xC4 => {
                let addr = self.read_word(bus);
                if !self.flag(FLAG_Z) {
                    self.push(bus, self.pc);
                    self.pc = addr;
                    24
                } else {
                    12
                }
            }
            // PUSH BC
            0xC5 => { let bc = self.bc(); self.push(bus, bc); 16 }
            // ADD A,d8
            0xC6 => { let v = self.read_byte(bus); self.alu_add(v); 8 }
            // RST 00H
            0xC7 => { self.push(bus, self.pc); self.pc = 0x00; 16 }
            // RET Z
            0xC8 => {
                if self.flag(FLAG_Z) {
                    self.pc = self.pop(bus);
                    20
                } else {
                    8
                }
            }
            // RET
            0xC9 => { self.pc = self.pop(bus); 16 }
            // JP Z,a16
            0xCA => {
                let addr = self.read_word(bus);
                if self.flag(FLAG_Z) {
                    self.pc = addr;
                    16
                } else {
                    12
                }
            }
            // CB prefix
            0xCB => {
                let cb_op = self.read_byte(bus);
                self.execute_cb(cb_op, bus)
            }
            // CALL Z,a16
            0xCC => {
                let addr = self.read_word(bus);
                if self.flag(FLAG_Z) {
                    self.push(bus, self.pc);
                    self.pc = addr;
                    24
                } else {
                    12
                }
            }
            // CALL a16
            0xCD => {
                let addr = self.read_word(bus);
                self.push(bus, self.pc);
                self.pc = addr;
                24
            }
            // ADC A,d8
            0xCE => { let v = self.read_byte(bus); self.alu_adc(v); 8 }
            // RST 08H
            0xCF => { self.push(bus, self.pc); self.pc = 0x08; 16 }
            // RET NC
            0xD0 => {
                if !self.flag(FLAG_C) {
                    self.pc = self.pop(bus);
                    20
                } else {
                    8
                }
            }
            // POP DE
            0xD1 => { let v = self.pop(bus); self.set_de(v); 12 }
            // JP NC,a16
            0xD2 => {
                let addr = self.read_word(bus);
                if !self.flag(FLAG_C) {
                    self.pc = addr;
                    16
                } else {
                    12
                }
            }
            // CALL NC,a16
            0xD4 => {
                let addr = self.read_word(bus);
                if !self.flag(FLAG_C) {
                    self.push(bus, self.pc);
                    self.pc = addr;
                    24
                } else {
                    12
                }
            }
            // PUSH DE
            0xD5 => { let de = self.de(); self.push(bus, de); 16 }
            // SUB d8
            0xD6 => { let v = self.read_byte(bus); self.alu_sub(v); 8 }
            // RST 10H
            0xD7 => { self.push(bus, self.pc); self.pc = 0x10; 16 }
            // RET C
            0xD8 => {
                if self.flag(FLAG_C) {
                    self.pc = self.pop(bus);
                    20
                } else {
                    8
                }
            }
            // RETI
            0xD9 => {
                self.pc = self.pop(bus);
                self.ime = true;
                16
            }
            // JP C,a16
            0xDA => {
                let addr = self.read_word(bus);
                if self.flag(FLAG_C) {
                    self.pc = addr;
                    16
                } else {
                    12
                }
            }
            // CALL C,a16
            0xDC => {
                let addr = self.read_word(bus);
                if self.flag(FLAG_C) {
                    self.push(bus, self.pc);
                    self.pc = addr;
                    24
                } else {
                    12
                }
            }
            // SBC A,d8
            0xDE => { let v = self.read_byte(bus); self.alu_sbc(v); 8 }
            // RST 18H
            0xDF => { self.push(bus, self.pc); self.pc = 0x18; 16 }
            // LDH (a8),A
            0xE0 => {
                let offset = self.read_byte(bus) as u16;
                bus.write(0xFF00 + offset, self.a);
                12
            }
            // POP HL
            0xE1 => { let v = self.pop(bus); self.set_hl(v); 12 }
            // LD (C),A
            0xE2 => { bus.write(0xFF00 + self.c as u16, self.a); 8 }
            // PUSH HL
            0xE5 => { let hl = self.hl(); self.push(bus, hl); 16 }
            // AND d8
            0xE6 => { let v = self.read_byte(bus); self.alu_and(v); 8 }
            // RST 20H
            0xE7 => { self.push(bus, self.pc); self.pc = 0x20; 16 }
            // ADD SP,r8
            0xE8 => { self.sp = self.alu_add_sp_e8(bus); 16 }
            // JP (HL)
            0xE9 => { self.pc = self.hl(); 4 }
            // LD (a16),A
            0xEA => { let addr = self.read_word(bus); bus.write(addr, self.a); 16 }
            // XOR d8
            0xEE => { let v = self.read_byte(bus); self.alu_xor(v); 8 }
            // RST 28H
            0xEF => { self.push(bus, self.pc); self.pc = 0x28; 16 }
            // LDH A,(a8)
            0xF0 => {
                let offset = self.read_byte(bus) as u16;
                self.a = bus.read(0xFF00 + offset);
                12
            }
            // POP AF
            0xF1 => { let v = self.pop(bus); self.set_af(v); 12 }
            // LD A,(C)
            0xF2 => { self.a = bus.read(0xFF00 + self.c as u16); 8 }
            // DI
            0xF3 => { self.ime = false; 4 }
            // PUSH AF
            0xF5 => { let af = self.af(); self.push(bus, af); 16 }
            // OR d8
            0xF6 => { let v = self.read_byte(bus); self.alu_or(v); 8 }
            // RST 30H
            0xF7 => { self.push(bus, self.pc); self.pc = 0x30; 16 }
            // LD HL,SP+r8
            0xF8 => {
                let v = self.alu_add_sp_e8(bus);
                self.set_hl(v);
                12
            }
            // LD SP,HL
            0xF9 => { self.sp = self.hl(); 8 }
            // LD A,(a16)
            0xFA => { let addr = self.read_word(bus); self.a = bus.read(addr); 16 }
            // EI
            0xFB => { self.ime_pending = true; 4 }
            // CP d8
            0xFE => { let v = self.read_byte(bus); self.alu_cp(v); 8 }
            // RST 38H
            0xFF => { self.push(bus, self.pc); self.pc = 0x38; 16 }

            // Illegal opcodes
            _ => {
                log::warn!("Illegal opcode: 0x{:02X} at PC=0x{:04X}", opcode, self.pc.wrapping_sub(1));
                4
            }
        }
    }

    fn execute_cb(&mut self, opcode: u8, bus: &mut impl Sm83Bus) -> u32 {
        let reg = opcode & 0x07;
        let val = self.get_reg(reg, bus);
        let is_hl = reg == 6;
        let base_cycles = if is_hl { 16 } else { 8 };

        let result = match opcode >> 3 {
            0 => self.rlc(val),
            1 => self.rrc(val),
            2 => self.rl(val),
            3 => self.rr(val),
            4 => self.sla(val),
            5 => self.sra(val),
            6 => self.swap(val),
            7 => self.srl(val),
            // BIT b,r
            8..=15 => {
                let bit = (opcode >> 3) & 0x07;
                self.bit(bit, val);
                return if is_hl { 12 } else { 8 };
            }
            // RES b,r
            16..=23 => {
                let bit = (opcode >> 3) & 0x07;
                Self::res(bit, val)
            }
            // SET b,r
            24..=31 => {
                let bit = (opcode >> 3) & 0x07;
                Self::set(bit, val)
            }
            _ => unreachable!(),
        };

        self.set_reg(reg, result, bus);
        base_cycles
    }
}

impl Default for Sm83 {
    fn default() -> Self {
        Self::new()
    }
}
