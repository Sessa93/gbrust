use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CpuMode {
    User = 0x10,
    Fiq = 0x11,
    Irq = 0x12,
    Supervisor = 0x13,
    Abort = 0x17,
    Undefined = 0x1B,
    System = 0x1F,
}

impl CpuMode {
    pub fn from_bits(bits: u32) -> Self {
        match bits & 0x1F {
            0x10 => CpuMode::User,
            0x11 => CpuMode::Fiq,
            0x12 => CpuMode::Irq,
            0x13 => CpuMode::Supervisor,
            0x17 => CpuMode::Abort,
            0x1B => CpuMode::Undefined,
            0x1F => CpuMode::System,
            _ => CpuMode::User,
        }
    }

    fn bank_index(self) -> usize {
        match self {
            CpuMode::User | CpuMode::System => 0,
            CpuMode::Fiq => 1,
            CpuMode::Irq => 2,
            CpuMode::Supervisor => 3,
            CpuMode::Abort => 4,
            CpuMode::Undefined => 5,
        }
    }
}

pub trait Arm7Bus {
    fn read8(&self, addr: u32) -> u8;
    fn read16(&mut self, addr: u32) -> u16;
    fn read32(&mut self, addr: u32) -> u32;
    fn write8(&mut self, addr: u32, val: u8);
    fn write16(&mut self, addr: u32, val: u16);
    fn write32(&mut self, addr: u32, val: u32);
}

const CPSR_N: u32 = 1 << 31;
const CPSR_Z: u32 = 1 << 30;
const CPSR_C: u32 = 1 << 29;
const CPSR_V: u32 = 1 << 28;
const CPSR_I: u32 = 1 << 7;
const CPSR_F: u32 = 1 << 6;
const CPSR_T: u32 = 1 << 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Arm7Tdmi {
    pub regs: [u32; 16],
    pub cpsr: u32,
    pub spsr: [u32; 6],
    pub banked_regs: [[u32; 7]; 6], // R8-R14 for each mode
    pub cycles: u64,
    pub halted: bool,
}

impl Arm7Tdmi {
    pub fn new() -> Self {
        let mut cpu = Self {
            regs: [0; 16],
            cpsr: CpuMode::System as u32,
            spsr: [0; 6],
            banked_regs: [[0; 7]; 6],
            cycles: 0,
            halted: false,
        };
        cpu.regs[15] = 0x0800_0000; // ROM entry point
        cpu.regs[13] = 0x0300_7F00; // Stack pointer in IWRAM
        cpu.banked_regs[2][5] = 0x0300_7FA0; // IRQ stack
        cpu.banked_regs[3][5] = 0x0300_7FE0; // SVC stack
        cpu
    }

    pub fn thumb_mode(&self) -> bool {
        self.cpsr & CPSR_T != 0
    }

    fn condition_passed(&self, cond: u32) -> bool {
        match cond {
            0x0 => self.cpsr & CPSR_Z != 0,                                    // EQ
            0x1 => self.cpsr & CPSR_Z == 0,                                    // NE
            0x2 => self.cpsr & CPSR_C != 0,                                    // CS/HS
            0x3 => self.cpsr & CPSR_C == 0,                                    // CC/LO
            0x4 => self.cpsr & CPSR_N != 0,                                    // MI
            0x5 => self.cpsr & CPSR_N == 0,                                    // PL
            0x6 => self.cpsr & CPSR_V != 0,                                    // VS
            0x7 => self.cpsr & CPSR_V == 0,                                    // VC
            0x8 => (self.cpsr & CPSR_C != 0) && (self.cpsr & CPSR_Z == 0),    // HI
            0x9 => (self.cpsr & CPSR_C == 0) || (self.cpsr & CPSR_Z != 0),    // LS
            0xA => ((self.cpsr >> 31) & 1) == ((self.cpsr >> 28) & 1),         // GE
            0xB => ((self.cpsr >> 31) & 1) != ((self.cpsr >> 28) & 1),         // LT
            0xC => (self.cpsr & CPSR_Z == 0) && (((self.cpsr >> 31) & 1) == ((self.cpsr >> 28) & 1)), // GT
            0xD => (self.cpsr & CPSR_Z != 0) || (((self.cpsr >> 31) & 1) != ((self.cpsr >> 28) & 1)), // LE
            0xE => true,                                                        // AL
            _ => true,
        }
    }

    fn current_mode(&self) -> CpuMode {
        CpuMode::from_bits(self.cpsr)
    }

    fn switch_mode(&mut self, new_mode: CpuMode) {
        let old_mode = self.current_mode();
        if old_mode == new_mode {
            return;
        }
        let old_bank = old_mode.bank_index();
        let new_bank = new_mode.bank_index();

        // Save current banked registers
        if old_mode == CpuMode::Fiq {
            for i in 0..7 {
                self.banked_regs[old_bank][i] = self.regs[8 + i];
            }
        } else {
            self.banked_regs[old_bank][5] = self.regs[13];
            self.banked_regs[old_bank][6] = self.regs[14];
        }

        // Load new banked registers
        if new_mode == CpuMode::Fiq {
            for i in 0..7 {
                self.regs[8 + i] = self.banked_regs[new_bank][i];
            }
        } else {
            self.regs[13] = self.banked_regs[new_bank][5];
            self.regs[14] = self.banked_regs[new_bank][6];
        }

        self.cpsr = (self.cpsr & !0x1F) | (new_mode as u32);
    }

    fn set_nz(&mut self, result: u32) {
        if result == 0 {
            self.cpsr |= CPSR_Z;
        } else {
            self.cpsr &= !CPSR_Z;
        }
        if result & 0x8000_0000 != 0 {
            self.cpsr |= CPSR_N;
        } else {
            self.cpsr &= !CPSR_N;
        }
    }

    fn barrel_shift(&mut self, val: u32, shift_type: u32, amount: u32, set_carry: bool) -> u32 {
        if amount == 0 {
            return val;
        }
        let (result, carry) = match shift_type {
            0 => { // LSL
                if amount >= 32 {
                    let c = if amount == 32 { val & 1 != 0 } else { false };
                    (0, c)
                } else {
                    (val << amount, val & (1 << (32 - amount)) != 0)
                }
            }
            1 => { // LSR
                if amount >= 32 {
                    let c = if amount == 32 { val & 0x8000_0000 != 0 } else { false };
                    (0, c)
                } else {
                    (val >> amount, val & (1 << (amount - 1)) != 0)
                }
            }
            2 => { // ASR
                if amount >= 32 {
                    let c = val & 0x8000_0000 != 0;
                    (if c { 0xFFFF_FFFF } else { 0 }, c)
                } else {
                    ((val as i32 >> amount) as u32, val & (1 << (amount - 1)) != 0)
                }
            }
            3 => { // ROR
                let amt = amount & 31;
                if amt == 0 {
                    (val, val & 0x8000_0000 != 0)
                } else {
                    (val.rotate_right(amt), val & (1 << (amt - 1)) != 0)
                }
            }
            _ => (val, false),
        };
        if set_carry {
            if carry {
                self.cpsr |= CPSR_C;
            } else {
                self.cpsr &= !CPSR_C;
            }
        }
        result
    }

    pub fn handle_irq(&mut self) {
        if self.cpsr & CPSR_I != 0 {
            return;
        }
        let old_cpsr = self.cpsr;
        self.switch_mode(CpuMode::Irq);
        self.spsr[CpuMode::Irq.bank_index()] = old_cpsr;
        self.regs[14] = self.regs[15].wrapping_add(4);
        self.cpsr = (self.cpsr & !CPSR_T) | CPSR_I;
        self.regs[15] = 0x0000_0018;
        self.halted = false;
    }

    pub fn step(&mut self, bus: &mut impl Arm7Bus) -> u32 {
        if self.halted {
            self.cycles += 1;
            return 1;
        }

        let cycles = if self.thumb_mode() {
            self.step_thumb(bus)
        } else {
            self.step_arm(bus)
        };
        self.cycles += cycles as u64;
        cycles
    }

    fn step_arm(&mut self, bus: &mut impl Arm7Bus) -> u32 {
        let pc = self.regs[15];
        let instr = bus.read32(pc & !3);
        self.regs[15] = pc.wrapping_add(4);

        let cond = instr >> 28;
        if !self.condition_passed(cond) {
            return 1;
        }

        let bits_27_20 = (instr >> 20) & 0xFF;
        let bits_7_4 = (instr >> 4) & 0xF;

        // Decode ARM instructions
        match (bits_27_20 >> 5, bits_7_4) {
            // Branch and Exchange (BX)
            _ if instr & 0x0FFF_FFF0 == 0x012F_FF10 => {
                let rn = (instr & 0xF) as usize;
                let addr = self.regs[rn];
                if addr & 1 != 0 {
                    self.cpsr |= CPSR_T;
                    self.regs[15] = addr & !1;
                } else {
                    self.cpsr &= !CPSR_T;
                    self.regs[15] = addr & !3;
                }
                3
            }

            // Software Interrupt
            _ if bits_27_20 >> 4 == 0xF => {
                self.arm_swi(instr, bus);
                3
            }

            // Branch / Branch with Link
            _ if instr & 0x0E00_0000 == 0x0A00_0000 => {
                let link = instr & (1 << 24) != 0;
                let offset = ((instr & 0x00FF_FFFF) as i32) << 8 >> 6;
                if link {
                    self.regs[14] = self.regs[15]; // next instruction after BL
                }
                // PC+8 semantics: add 4 extra since we only advanced by 4
                self.regs[15] = ((self.regs[15].wrapping_add(4)) as i32).wrapping_add(offset) as u32;
                3
            }

            // Block Data Transfer (LDM/STM)
            _ if instr & 0x0E00_0000 == 0x0800_0000 => {
                self.arm_block_transfer(instr, bus)
            }

            // Single Data Transfer (LDR/STR)
            _ if instr & 0x0C00_0000 == 0x0400_0000 => {
                self.arm_single_transfer(instr, bus)
            }

            // Halfword/Signed Data Transfer
            _ if instr & 0x0E00_0090 == 0x0000_0090 && (bits_7_4 & 0x9) == 0x9 && bits_7_4 != 0x9 => {
                self.arm_halfword_transfer(instr, bus)
            }

            // Multiply / Multiply Long
            _ if instr & 0x0FC0_00F0 == 0x0000_0090 => {
                self.arm_multiply(instr)
            }
            _ if instr & 0x0F80_00F0 == 0x0080_0090 => {
                self.arm_multiply_long(instr)
            }

            // MRS
            _ if instr & 0x0FBF_0FFF == 0x010F_0000 => {
                let rd = ((instr >> 12) & 0xF) as usize;
                let use_spsr = instr & (1 << 22) != 0;
                self.regs[rd] = if use_spsr {
                    self.spsr[self.current_mode().bank_index()]
                } else {
                    self.cpsr
                };
                1
            }

            // MSR
            _ if instr & 0x0DB0_F000 == 0x0120_F000 => {
                self.arm_msr(instr);
                1
            }

            // Data Processing
            _ if instr & 0x0C00_0000 == 0x0000_0000 => {
                self.arm_data_processing(instr, bus)
            }

            _ => {
                log::warn!("Unimplemented ARM instruction: 0x{:08X} at 0x{:08X}", instr, pc);
                1
            }
        }
    }

    fn arm_data_processing(&mut self, instr: u32, bus: &impl Arm7Bus) -> u32 {
        let opcode = (instr >> 21) & 0xF;
        let set_flags = instr & (1 << 20) != 0;
        let rn = ((instr >> 16) & 0xF) as usize;
        let rd = ((instr >> 12) & 0xF) as usize;

        let op1 = if rn == 15 { self.regs[15].wrapping_add(4) } else { self.regs[rn] };

        // Calculate operand2
        let (op2, carry_out) = if instr & (1 << 25) != 0 {
            // Immediate
            let imm = instr & 0xFF;
            let rot = ((instr >> 8) & 0xF) * 2;
            let val = imm.rotate_right(rot);
            let c = if rot == 0 {
                self.cpsr & CPSR_C != 0
            } else {
                val & 0x8000_0000 != 0
            };
            (val, c)
        } else {
            // Register
            let rm = (instr & 0xF) as usize;
            let rm_val = if rm == 15 { self.regs[15].wrapping_add(4) } else { self.regs[rm] };
            let shift_type = (instr >> 5) & 3;

            let amount = if instr & (1 << 4) != 0 {
                let rs = ((instr >> 8) & 0xF) as usize;
                self.regs[rs] & 0xFF
            } else {
                (instr >> 7) & 0x1F
            };

            if amount == 0 && instr & (1 << 4) == 0 {
                // Special shift amounts of 0
                match shift_type {
                    0 => (rm_val, self.cpsr & CPSR_C != 0),
                    1 => (0, rm_val & 0x8000_0000 != 0),
                    2 => {
                        if rm_val & 0x8000_0000 != 0 {
                            (0xFFFF_FFFF, true)
                        } else {
                            (0, false)
                        }
                    }
                    3 => { // RRX
                        let c = self.cpsr & CPSR_C != 0;
                        let r = (rm_val >> 1) | if c { 0x8000_0000 } else { 0 };
                        (r, rm_val & 1 != 0)
                    }
                    _ => unreachable!(),
                }
            } else {
                let old_carry = self.cpsr & CPSR_C != 0;
                let shifted = self.barrel_shift(rm_val, shift_type, amount, false);
                let carry = match shift_type {
                    0 => if amount == 0 { old_carry } else if amount >= 32 { if amount == 32 { rm_val & 1 != 0 } else { false } } else { rm_val & (1 << (32 - amount)) != 0 },
                    1 => if amount >= 32 { if amount == 32 { rm_val & 0x8000_0000 != 0 } else { false } } else { rm_val & (1 << (amount - 1)) != 0 },
                    2 => if amount >= 32 { rm_val & 0x8000_0000 != 0 } else { rm_val & (1 << (amount - 1)) != 0 },
                    3 => { let a = amount & 31; if a == 0 { rm_val & 0x8000_0000 != 0 } else { rm_val & (1 << (a - 1)) != 0 } },
                    _ => old_carry,
                };
                (shifted, carry)
            }
        };

        let _ = bus; // bus not needed for data processing

        let result = match opcode {
            0x0 => { // AND
                let r = op1 & op2;
                if set_flags { self.set_nz(r); if carry_out { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; } }
                if rd != 15 { self.regs[rd] = r; }
                r
            }
            0x1 => { // EOR
                let r = op1 ^ op2;
                if set_flags { self.set_nz(r); if carry_out { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; } }
                if rd != 15 { self.regs[rd] = r; }
                r
            }
            0x2 => { // SUB
                let (r, borrow) = op1.overflowing_sub(op2);
                if set_flags {
                    self.set_nz(r);
                    if !borrow { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                    let v = ((op1 ^ op2) & (op1 ^ r)) >> 31;
                    if v != 0 { self.cpsr |= CPSR_V; } else { self.cpsr &= !CPSR_V; }
                }
                if rd != 15 { self.regs[rd] = r; }
                r
            }
            0x3 => { // RSB
                let (r, borrow) = op2.overflowing_sub(op1);
                if set_flags {
                    self.set_nz(r);
                    if !borrow { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                    let v = ((op2 ^ op1) & (op2 ^ r)) >> 31;
                    if v != 0 { self.cpsr |= CPSR_V; } else { self.cpsr &= !CPSR_V; }
                }
                if rd != 15 { self.regs[rd] = r; }
                r
            }
            0x4 => { // ADD
                let (r, carry) = op1.overflowing_add(op2);
                if set_flags {
                    self.set_nz(r);
                    if carry { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                    let v = (!(op1 ^ op2) & (op1 ^ r)) >> 31;
                    if v != 0 { self.cpsr |= CPSR_V; } else { self.cpsr &= !CPSR_V; }
                }
                if rd != 15 { self.regs[rd] = r; }
                r
            }
            0x5 => { // ADC
                let c = if self.cpsr & CPSR_C != 0 { 1u32 } else { 0 };
                let r = op1.wrapping_add(op2).wrapping_add(c);
                if set_flags {
                    self.set_nz(r);
                    let carry = (op1 as u64) + (op2 as u64) + (c as u64) > 0xFFFF_FFFF;
                    if carry { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                    let v = (!(op1 ^ op2) & (op1 ^ r)) >> 31;
                    if v != 0 { self.cpsr |= CPSR_V; } else { self.cpsr &= !CPSR_V; }
                }
                if rd != 15 { self.regs[rd] = r; }
                r
            }
            0x6 => { // SBC
                let c = if self.cpsr & CPSR_C != 0 { 1u32 } else { 0 };
                let r = op1.wrapping_sub(op2).wrapping_sub(1 - c);
                if set_flags {
                    self.set_nz(r);
                    let borrow = (op1 as u64) < (op2 as u64) + (1 - c as u64);
                    if !borrow { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                    let v = ((op1 ^ op2) & (op1 ^ r)) >> 31;
                    if v != 0 { self.cpsr |= CPSR_V; } else { self.cpsr &= !CPSR_V; }
                }
                if rd != 15 { self.regs[rd] = r; }
                r
            }
            0x7 => { // RSC
                let c = if self.cpsr & CPSR_C != 0 { 1u32 } else { 0 };
                let r = op2.wrapping_sub(op1).wrapping_sub(1 - c);
                if set_flags {
                    self.set_nz(r);
                    let borrow = (op2 as u64) < (op1 as u64) + (1 - c as u64);
                    if !borrow { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                    let v = ((op2 ^ op1) & (op2 ^ r)) >> 31;
                    if v != 0 { self.cpsr |= CPSR_V; } else { self.cpsr &= !CPSR_V; }
                }
                if rd != 15 { self.regs[rd] = r; }
                r
            }
            0x8 => { // TST
                let r = op1 & op2;
                self.set_nz(r);
                if carry_out { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                r
            }
            0x9 => { // TEQ
                let r = op1 ^ op2;
                self.set_nz(r);
                if carry_out { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                r
            }
            0xA => { // CMP
                let (r, borrow) = op1.overflowing_sub(op2);
                self.set_nz(r);
                if !borrow { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                let v = ((op1 ^ op2) & (op1 ^ r)) >> 31;
                if v != 0 { self.cpsr |= CPSR_V; } else { self.cpsr &= !CPSR_V; }
                r
            }
            0xB => { // CMN
                let (r, carry) = op1.overflowing_add(op2);
                self.set_nz(r);
                if carry { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                let v = (!(op1 ^ op2) & (op1 ^ r)) >> 31;
                if v != 0 { self.cpsr |= CPSR_V; } else { self.cpsr &= !CPSR_V; }
                r
            }
            0xC => { // ORR
                let r = op1 | op2;
                if set_flags { self.set_nz(r); if carry_out { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; } }
                if rd != 15 { self.regs[rd] = r; }
                r
            }
            0xD => { // MOV
                let r = op2;
                if set_flags { self.set_nz(r); if carry_out { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; } }
                if rd != 15 { self.regs[rd] = r; }
                r
            }
            0xE => { // BIC
                let r = op1 & !op2;
                if set_flags { self.set_nz(r); if carry_out { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; } }
                if rd != 15 { self.regs[rd] = r; }
                r
            }
            0xF => { // MVN
                let r = !op2;
                if set_flags { self.set_nz(r); if carry_out { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; } }
                if rd != 15 { self.regs[rd] = r; }
                r
            }
            _ => unreachable!(),
        };

        // Handle writes to PC for non-test operations
        if rd == 15 && !matches!(opcode, 0x8 | 0x9 | 0xA | 0xB) {
            self.regs[15] = result;
        }
        // S flag with Rd=PC: restore CPSR from SPSR and switch register banks
        if rd == 15 && set_flags {
            let new_cpsr = self.spsr[self.current_mode().bank_index()];
            let new_mode = CpuMode::from_bits(new_cpsr);
            self.switch_mode(new_mode);
            self.cpsr = new_cpsr;
        }

        1
    }

    fn arm_multiply(&mut self, instr: u32) -> u32 {
        let accumulate = instr & (1 << 21) != 0;
        let set_flags = instr & (1 << 20) != 0;
        let rd = ((instr >> 16) & 0xF) as usize;
        let rn = ((instr >> 12) & 0xF) as usize;
        let rs = ((instr >> 8) & 0xF) as usize;
        let rm = (instr & 0xF) as usize;

        let result = if accumulate {
            self.regs[rm].wrapping_mul(self.regs[rs]).wrapping_add(self.regs[rn])
        } else {
            self.regs[rm].wrapping_mul(self.regs[rs])
        };

        self.regs[rd] = result;

        if set_flags {
            self.set_nz(result);
        }
        if accumulate { 2 } else { 1 }
    }

    fn arm_multiply_long(&mut self, instr: u32) -> u32 {
        let signed = instr & (1 << 22) != 0;
        let accumulate = instr & (1 << 21) != 0;
        let set_flags = instr & (1 << 20) != 0;
        let rdhi = ((instr >> 16) & 0xF) as usize;
        let rdlo = ((instr >> 12) & 0xF) as usize;
        let rs = ((instr >> 8) & 0xF) as usize;
        let rm = (instr & 0xF) as usize;

        let result: u64 = if signed {
            let a = self.regs[rm] as i32 as i64;
            let b = self.regs[rs] as i32 as i64;
            let mut r = a.wrapping_mul(b) as u64;
            if accumulate {
                let acc = ((self.regs[rdhi] as u64) << 32) | (self.regs[rdlo] as u64);
                r = r.wrapping_add(acc);
            }
            r
        } else {
            let a = self.regs[rm] as u64;
            let b = self.regs[rs] as u64;
            let mut r = a.wrapping_mul(b);
            if accumulate {
                let acc = ((self.regs[rdhi] as u64) << 32) | (self.regs[rdlo] as u64);
                r = r.wrapping_add(acc);
            }
            r
        };

        self.regs[rdhi] = (result >> 32) as u32;
        self.regs[rdlo] = result as u32;

        if set_flags {
            if result == 0 { self.cpsr |= CPSR_Z; } else { self.cpsr &= !CPSR_Z; }
            if result & 0x8000_0000_0000_0000 != 0 { self.cpsr |= CPSR_N; } else { self.cpsr &= !CPSR_N; }
        }
        if accumulate { 3 } else { 2 }
    }

    fn arm_single_transfer(&mut self, instr: u32, bus: &mut impl Arm7Bus) -> u32 {
        let pre = instr & (1 << 24) != 0;
        let up = instr & (1 << 23) != 0;
        let byte = instr & (1 << 22) != 0;
        let writeback = instr & (1 << 21) != 0;
        let load = instr & (1 << 20) != 0;
        let rn = ((instr >> 16) & 0xF) as usize;
        let rd = ((instr >> 12) & 0xF) as usize;

        let base = if rn == 15 { self.regs[15].wrapping_add(4) } else { self.regs[rn] };

        let offset = if instr & (1 << 25) != 0 {
            let rm = (instr & 0xF) as usize;
            let shift_type = (instr >> 5) & 3;
            let amount = (instr >> 7) & 0x1F;
            self.barrel_shift(self.regs[rm], shift_type, amount, false)
        } else {
            instr & 0xFFF
        };

        let addr = if pre {
            if up { base.wrapping_add(offset) } else { base.wrapping_sub(offset) }
        } else {
            base
        };

        if load {
            let val = if byte {
                bus.read8(addr) as u32
            } else {
                let v = bus.read32(addr & !3);
                let shift = (addr & 3) * 8;
                v.rotate_right(shift)
            };
            if rd == 15 {
                self.regs[15] = val & !1;
                if val & 1 != 0 {
                    self.cpsr |= CPSR_T;
                }
            } else {
                self.regs[rd] = val;
            }
        } else {
            let val = if rd == 15 { self.regs[15].wrapping_add(4) } else { self.regs[rd] };
            if byte {
                bus.write8(addr, val as u8);
            } else {
                bus.write32(addr & !3, val);
            }
        }

        let post_addr = if !pre {
            if up { base.wrapping_add(offset) } else { base.wrapping_sub(offset) }
        } else {
            addr
        };

        if (!pre || writeback) && rn != 15 {
            self.regs[rn] = post_addr;
        }

        if load { 3 } else { 2 }
    }

    fn arm_halfword_transfer(&mut self, instr: u32, bus: &mut impl Arm7Bus) -> u32 {
        let pre = instr & (1 << 24) != 0;
        let up = instr & (1 << 23) != 0;
        let imm_offset = instr & (1 << 22) != 0;
        let writeback = instr & (1 << 21) != 0;
        let load = instr & (1 << 20) != 0;
        let rn = ((instr >> 16) & 0xF) as usize;
        let rd = ((instr >> 12) & 0xF) as usize;
        let sh = (instr >> 5) & 3;

        let base = self.regs[rn];
        let offset = if imm_offset {
            ((instr >> 4) & 0xF0) | (instr & 0xF)
        } else {
            self.regs[(instr & 0xF) as usize]
        };

        let addr = if pre {
            if up { base.wrapping_add(offset) } else { base.wrapping_sub(offset) }
        } else {
            base
        };

        if load {
            let val = match sh {
                1 => bus.read16(addr & !1) as u32, // LDRH
                2 => bus.read8(addr) as i8 as i32 as u32, // LDRSB
                3 => bus.read16(addr & !1) as i16 as i32 as u32, // LDRSH
                _ => 0,
            };
            self.regs[rd] = val;
        } else {
            match sh {
                1 => bus.write16(addr & !1, self.regs[rd] as u16), // STRH
                _ => {}
            }
        }

        let post_addr = if !pre {
            if up { base.wrapping_add(offset) } else { base.wrapping_sub(offset) }
        } else {
            addr
        };

        if (!pre || writeback) && rn != 15 {
            self.regs[rn] = post_addr;
        }

        if load { 3 } else { 2 }
    }

    fn arm_block_transfer(&mut self, instr: u32, bus: &mut impl Arm7Bus) -> u32 {
        let pre = instr & (1 << 24) != 0;
        let up = instr & (1 << 23) != 0;
        let psr = instr & (1 << 22) != 0;
        let writeback = instr & (1 << 21) != 0;
        let load = instr & (1 << 20) != 0;
        let rn = ((instr >> 16) & 0xF) as usize;
        let rlist = instr & 0xFFFF;

        let count = rlist.count_ones();
        let base = self.regs[rn];

        let start_addr = if up {
            if pre { base.wrapping_add(4) } else { base }
        } else {
            if pre {
                base.wrapping_sub(count * 4)
            } else {
                base.wrapping_sub(count * 4).wrapping_add(4)
            }
        };

        let mut addr = start_addr;

        for i in 0..16u32 {
            if rlist & (1 << i) == 0 {
                continue;
            }
            if load {
                self.regs[i as usize] = bus.read32(addr & !3);
            } else {
                let val = if i == 15 {
                    self.regs[15].wrapping_add(4)
                } else {
                    self.regs[i as usize]
                };
                bus.write32(addr & !3, val);
            }
            addr = addr.wrapping_add(4);
        }

        if writeback && rn != 15 {
            self.regs[rn] = if up {
                base.wrapping_add(count * 4)
            } else {
                base.wrapping_sub(count * 4)
            };
        }

        // PSR flag with LDM + PC in list: restore CPSR from SPSR and switch register banks
        if psr && load && (rlist & (1 << 15) != 0) {
            let new_cpsr = self.spsr[self.current_mode().bank_index()];
            let new_mode = CpuMode::from_bits(new_cpsr);
            self.switch_mode(new_mode);
            self.cpsr = new_cpsr;
        }

        count + if load { 2 } else { 1 }
    }

    fn arm_msr(&mut self, instr: u32) {
        let use_spsr = instr & (1 << 22) != 0;
        let mask_bits = (instr >> 16) & 0xF;

        let val = if instr & (1 << 25) != 0 {
            let imm = instr & 0xFF;
            let rot = ((instr >> 8) & 0xF) * 2;
            imm.rotate_right(rot)
        } else {
            self.regs[(instr & 0xF) as usize]
        };

        let mut mask = 0u32;
        if mask_bits & 1 != 0 { mask |= 0x0000_00FF; }
        if mask_bits & 2 != 0 { mask |= 0x0000_FF00; }
        if mask_bits & 4 != 0 { mask |= 0x00FF_0000; }
        if mask_bits & 8 != 0 { mask |= 0xFF00_0000; }

        if use_spsr {
            let idx = self.current_mode().bank_index();
            self.spsr[idx] = (self.spsr[idx] & !mask) | (val & mask);
        } else {
            let old = self.cpsr;
            self.cpsr = (old & !mask) | (val & mask);
            if (old ^ self.cpsr) & 0x1F != 0 {
                let new_mode = CpuMode::from_bits(self.cpsr);
                self.switch_mode(new_mode);
            }
        }
    }

    fn arm_swi(&mut self, instr: u32, bus: &mut impl Arm7Bus) {
        let swi_num = (instr >> 16) & 0xFF;
        self.handle_swi(swi_num, bus);
    }

    // THUMB instruction execution
    fn step_thumb(&mut self, bus: &mut impl Arm7Bus) -> u32 {
        let pc = self.regs[15];
        let instr = bus.read16(pc & !1) as u32;
        self.regs[15] = pc.wrapping_add(2);

        match instr >> 13 {
            0b000 => {
                if (instr >> 11) & 3 == 3 {
                    self.thumb_add_sub(instr, bus)
                } else {
                    self.thumb_move_shifted(instr)
                }
            }
            0b001 => self.thumb_imm_op(instr),
            0b010 => {
                if instr & (1 << 12) != 0 {
                    // Formats 7/8: Load/store with register offset (0101xxxx)
                    self.thumb_load_store_reg(instr, bus)
                } else if instr & (1 << 11) != 0 {
                    // Format 6: PC-relative load (01001xxx)
                    self.thumb_pc_relative_load(instr, bus)
                } else if instr & (1 << 10) != 0 {
                    // Format 5: Hi register operations / BX (010001xx)
                    self.thumb_hi_reg(instr, bus)
                } else {
                    // Format 4: ALU operations (010000xx)
                    self.thumb_alu(instr)
                }
            }
            0b011 => self.thumb_load_store_imm(instr, bus),
            0b100 => {
                if instr & (1 << 12) != 0 {
                    self.thumb_sp_relative_load_store(instr, bus)
                } else {
                    self.thumb_load_store_halfword(instr, bus)
                }
            }
            0b101 => {
                if instr & (1 << 12) != 0 {
                    if instr & (1 << 10) != 0 {
                        self.thumb_push_pop(instr, bus)
                    } else {
                        self.thumb_add_sp(instr)
                    }
                } else {
                    self.thumb_load_address(instr)
                }
            }
            0b110 => {
                if instr & (1 << 12) != 0 {
                    if (instr >> 8) & 0xF == 0xF {
                        self.thumb_swi(bus)
                    } else {
                        self.thumb_cond_branch(instr)
                    }
                } else {
                    self.thumb_multiple_load_store(instr, bus)
                }
            }
            0b111 => {
                if instr & (1 << 12) != 0 {
                    self.thumb_long_branch(instr)
                } else {
                    self.thumb_unconditional_branch(instr)
                }
            }
            _ => unreachable!(),
        }
    }

    fn thumb_move_shifted(&mut self, instr: u32) -> u32 {
        let op = (instr >> 11) & 3;
        let offset = (instr >> 6) & 0x1F;
        let rs = ((instr >> 3) & 7) as usize;
        let rd = (instr & 7) as usize;
        let val = self.regs[rs];

        let result = match op {
            0 => { // LSL
                if offset == 0 {
                    val
                } else {
                    if val & (1 << (32 - offset)) != 0 { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                    val << offset
                }
            }
            1 => { // LSR
                let amt = if offset == 0 { 32 } else { offset };
                if amt == 32 {
                    if val & 0x8000_0000 != 0 { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                    0
                } else {
                    if val & (1 << (amt - 1)) != 0 { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                    val >> amt
                }
            }
            2 => { // ASR
                let amt = if offset == 0 { 32 } else { offset };
                if amt >= 32 {
                    if val & 0x8000_0000 != 0 {
                        self.cpsr |= CPSR_C;
                        0xFFFF_FFFF
                    } else {
                        self.cpsr &= !CPSR_C;
                        0
                    }
                } else {
                    if val & (1 << (amt - 1)) != 0 { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                    (val as i32 >> amt) as u32
                }
            }
            _ => unreachable!(),
        };

        self.regs[rd] = result;
        self.set_nz(result);
        1
    }

    fn thumb_add_sub(&mut self, instr: u32, _bus: &impl Arm7Bus) -> u32 {
        let imm = instr & (1 << 10) != 0;
        let sub = instr & (1 << 9) != 0;
        let rn_imm = ((instr >> 6) & 7) as u32;
        let rs = ((instr >> 3) & 7) as usize;
        let rd = (instr & 7) as usize;

        let op1 = self.regs[rs];
        let op2 = if imm { rn_imm } else { self.regs[rn_imm as usize] };

        let result = if sub {
            let (r, borrow) = op1.overflowing_sub(op2);
            self.set_nz(r);
            if !borrow { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
            let v = ((op1 ^ op2) & (op1 ^ r)) >> 31;
            if v != 0 { self.cpsr |= CPSR_V; } else { self.cpsr &= !CPSR_V; }
            r
        } else {
            let (r, carry) = op1.overflowing_add(op2);
            self.set_nz(r);
            if carry { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
            let v = (!(op1 ^ op2) & (op1 ^ r)) >> 31;
            if v != 0 { self.cpsr |= CPSR_V; } else { self.cpsr &= !CPSR_V; }
            r
        };

        self.regs[rd] = result;
        1
    }

    fn thumb_imm_op(&mut self, instr: u32) -> u32 {
        let op = (instr >> 11) & 3;
        let rd = ((instr >> 8) & 7) as usize;
        let imm = instr & 0xFF;

        match op {
            0 => { // MOV
                self.regs[rd] = imm;
                self.set_nz(imm);
            }
            1 => { // CMP
                let (r, borrow) = self.regs[rd].overflowing_sub(imm);
                self.set_nz(r);
                if !borrow { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                let v = ((self.regs[rd] ^ imm) & (self.regs[rd] ^ r)) >> 31;
                if v != 0 { self.cpsr |= CPSR_V; } else { self.cpsr &= !CPSR_V; }
            }
            2 => { // ADD
                let (r, carry) = self.regs[rd].overflowing_add(imm);
                self.set_nz(r);
                if carry { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                let v = (!(self.regs[rd] ^ imm) & (self.regs[rd] ^ r)) >> 31;
                if v != 0 { self.cpsr |= CPSR_V; } else { self.cpsr &= !CPSR_V; }
                self.regs[rd] = r;
            }
            3 => { // SUB
                let (r, borrow) = self.regs[rd].overflowing_sub(imm);
                self.set_nz(r);
                if !borrow { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                let v = ((self.regs[rd] ^ imm) & (self.regs[rd] ^ r)) >> 31;
                if v != 0 { self.cpsr |= CPSR_V; } else { self.cpsr &= !CPSR_V; }
                self.regs[rd] = r;
            }
            _ => unreachable!(),
        }
        1
    }

    fn thumb_alu(&mut self, instr: u32) -> u32 {
        let op = (instr >> 6) & 0xF;
        let rs = ((instr >> 3) & 7) as usize;
        let rd = (instr & 7) as usize;
        let a = self.regs[rd];
        let b = self.regs[rs];

        match op {
            0x0 => { // AND
                let r = a & b; self.regs[rd] = r; self.set_nz(r);
            }
            0x1 => { // EOR
                let r = a ^ b; self.regs[rd] = r; self.set_nz(r);
            }
            0x2 => { // LSL
                let amt = b & 0xFF;
                let r = if amt == 0 { a } else if amt < 32 {
                    if a & (1 << (32 - amt)) != 0 { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                    a << amt
                } else if amt == 32 {
                    if a & 1 != 0 { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                    0
                } else { self.cpsr &= !CPSR_C; 0 };
                self.regs[rd] = r; self.set_nz(r);
            }
            0x3 => { // LSR
                let amt = b & 0xFF;
                let r = if amt == 0 { a } else if amt < 32 {
                    if a & (1 << (amt - 1)) != 0 { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                    a >> amt
                } else if amt == 32 {
                    if a & 0x8000_0000 != 0 { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                    0
                } else { self.cpsr &= !CPSR_C; 0 };
                self.regs[rd] = r; self.set_nz(r);
            }
            0x4 => { // ASR
                let amt = b & 0xFF;
                let r = if amt == 0 { a } else if amt < 32 {
                    if a & (1 << (amt - 1)) != 0 { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                    (a as i32 >> amt) as u32
                } else {
                    let c = a & 0x8000_0000 != 0;
                    if c { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                    if c { 0xFFFF_FFFF } else { 0 }
                };
                self.regs[rd] = r; self.set_nz(r);
            }
            0x5 => { // ADC
                let c = if self.cpsr & CPSR_C != 0 { 1u32 } else { 0 };
                let r = a.wrapping_add(b).wrapping_add(c);
                self.set_nz(r);
                let carry = (a as u64) + (b as u64) + (c as u64) > 0xFFFF_FFFF;
                if carry { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                let v = (!(a ^ b) & (a ^ r)) >> 31;
                if v != 0 { self.cpsr |= CPSR_V; } else { self.cpsr &= !CPSR_V; }
                self.regs[rd] = r;
            }
            0x6 => { // SBC
                let c = if self.cpsr & CPSR_C != 0 { 1u32 } else { 0 };
                let r = a.wrapping_sub(b).wrapping_sub(1 - c);
                self.set_nz(r);
                let borrow = (a as u64) < (b as u64) + (1 - c as u64);
                if !borrow { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                let v = ((a ^ b) & (a ^ r)) >> 31;
                if v != 0 { self.cpsr |= CPSR_V; } else { self.cpsr &= !CPSR_V; }
                self.regs[rd] = r;
            }
            0x7 => { // ROR
                let amt = b & 0xFF;
                let r = if amt == 0 { a } else {
                    let eff = amt & 31;
                    if eff == 0 {
                        if a & 0x8000_0000 != 0 { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                        a
                    } else {
                        if a & (1 << (eff - 1)) != 0 { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                        a.rotate_right(eff)
                    }
                };
                self.regs[rd] = r; self.set_nz(r);
            }
            0x8 => { // TST
                let r = a & b; self.set_nz(r);
            }
            0x9 => { // NEG
                let (r, borrow) = 0u32.overflowing_sub(b);
                self.set_nz(r);
                if !borrow { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                let v = (b & r) >> 31;
                if v != 0 { self.cpsr |= CPSR_V; } else { self.cpsr &= !CPSR_V; }
                self.regs[rd] = r;
            }
            0xA => { // CMP
                let (r, borrow) = a.overflowing_sub(b);
                self.set_nz(r);
                if !borrow { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                let v = ((a ^ b) & (a ^ r)) >> 31;
                if v != 0 { self.cpsr |= CPSR_V; } else { self.cpsr &= !CPSR_V; }
            }
            0xB => { // CMN
                let (r, carry) = a.overflowing_add(b);
                self.set_nz(r);
                if carry { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                let v = (!(a ^ b) & (a ^ r)) >> 31;
                if v != 0 { self.cpsr |= CPSR_V; } else { self.cpsr &= !CPSR_V; }
            }
            0xC => { // ORR
                let r = a | b; self.regs[rd] = r; self.set_nz(r);
            }
            0xD => { // MUL
                let r = a.wrapping_mul(b); self.regs[rd] = r; self.set_nz(r);
            }
            0xE => { // BIC
                let r = a & !b; self.regs[rd] = r; self.set_nz(r);
            }
            0xF => { // MVN
                let r = !b; self.regs[rd] = r; self.set_nz(r);
            }
            _ => unreachable!(),
        }
        1
    }

    fn thumb_hi_reg(&mut self, instr: u32, _bus: &impl Arm7Bus) -> u32 {
        let op = (instr >> 8) & 3;
        let h1 = (instr >> 7) & 1;
        let h2 = (instr >> 6) & 1;
        let rs = (((h2 << 3) | ((instr >> 3) & 7)) & 0xF) as usize;
        let rd = (((h1 << 3) | (instr & 7)) & 0xF) as usize;

        match op {
            0 => { // ADD
                self.regs[rd] = self.regs[rd].wrapping_add(self.regs[rs]);
                if rd == 15 { self.regs[15] &= !1; }
            }
            1 => { // CMP
                let a = self.regs[rd];
                let b = self.regs[rs];
                let (r, borrow) = a.overflowing_sub(b);
                self.set_nz(r);
                if !borrow { self.cpsr |= CPSR_C; } else { self.cpsr &= !CPSR_C; }
                let v = ((a ^ b) & (a ^ r)) >> 31;
                if v != 0 { self.cpsr |= CPSR_V; } else { self.cpsr &= !CPSR_V; }
            }
            2 => { // MOV
                self.regs[rd] = self.regs[rs];
                if rd == 15 { self.regs[15] &= !1; }
            }
            3 => { // BX
                let addr = self.regs[rs];
                if addr & 1 != 0 {
                    self.cpsr |= CPSR_T;
                    self.regs[15] = addr & !1;
                } else {
                    self.cpsr &= !CPSR_T;
                    self.regs[15] = addr & !3;
                }
            }
            _ => unreachable!(),
        }
        1
    }

    fn thumb_pc_relative_load(&mut self, instr: u32, bus: &mut impl Arm7Bus) -> u32 {
        let rd = ((instr >> 8) & 7) as usize;
        let offset = (instr & 0xFF) << 2;
        let addr = (self.regs[15].wrapping_add(2) & !2).wrapping_add(offset);
        self.regs[rd] = bus.read32(addr & !3);
        3
    }

    fn thumb_load_store_reg(&mut self, instr: u32, bus: &mut impl Arm7Bus) -> u32 {
        let ro = ((instr >> 6) & 7) as usize;
        let rb = ((instr >> 3) & 7) as usize;
        let rd = (instr & 7) as usize;
        let addr = self.regs[rb].wrapping_add(self.regs[ro]);

        if instr & (1 << 9) == 0 {
            // Format 7: STR/STRB/LDR/LDRB
            let op = (instr >> 10) & 3;
            match op {
                0 => bus.write32(addr & !3, self.regs[rd]),      // STR
                1 => bus.write8(addr, self.regs[rd] as u8),      // STRB
                2 => self.regs[rd] = bus.read32(addr & !3),       // LDR
                3 => self.regs[rd] = bus.read8(addr) as u32,      // LDRB
                _ => unreachable!(),
            }
            if op >= 2 { 3 } else { 2 }
        } else {
            // Format 8: STRH/LDSB/LDRH/LDSH
            let op = (instr >> 10) & 3;
            match op {
                0 => bus.write16(addr & !1, self.regs[rd] as u16),                    // STRH
                1 => self.regs[rd] = bus.read8(addr) as i8 as i32 as u32,             // LDSB
                2 => self.regs[rd] = bus.read16(addr & !1) as u32,                    // LDRH
                3 => self.regs[rd] = bus.read16(addr & !1) as i16 as i32 as u32,      // LDSH
                _ => unreachable!(),
            }
            if op >= 1 { 3 } else { 2 }
        }
    }

    fn thumb_load_store_imm(&mut self, instr: u32, bus: &mut impl Arm7Bus) -> u32 {
        let byte = instr & (1 << 12) != 0;
        let load = instr & (1 << 11) != 0;
        let offset = (instr >> 6) & 0x1F;
        let rb = ((instr >> 3) & 7) as usize;
        let rd = (instr & 7) as usize;

        let addr = if byte {
            self.regs[rb].wrapping_add(offset)
        } else {
            self.regs[rb].wrapping_add(offset << 2)
        };

        if load {
            self.regs[rd] = if byte {
                bus.read8(addr) as u32
            } else {
                bus.read32(addr & !3)
            };
            3
        } else {
            if byte {
                bus.write8(addr, self.regs[rd] as u8);
            } else {
                bus.write32(addr & !3, self.regs[rd]);
            }
            2
        }
    }

    fn thumb_load_store_halfword(&mut self, instr: u32, bus: &mut impl Arm7Bus) -> u32 {
        let load = instr & (1 << 11) != 0;
        let offset = ((instr >> 6) & 0x1F) << 1;
        let rb = ((instr >> 3) & 7) as usize;
        let rd = (instr & 7) as usize;
        let addr = self.regs[rb].wrapping_add(offset);

        if load {
            self.regs[rd] = bus.read16(addr & !1) as u32;
            3
        } else {
            bus.write16(addr & !1, self.regs[rd] as u16);
            2
        }
    }

    fn thumb_sp_relative_load_store(&mut self, instr: u32, bus: &mut impl Arm7Bus) -> u32 {
        let load = instr & (1 << 11) != 0;
        let rd = ((instr >> 8) & 7) as usize;
        let offset = (instr & 0xFF) << 2;
        let addr = self.regs[13].wrapping_add(offset);

        if load {
            self.regs[rd] = bus.read32(addr & !3);
            3
        } else {
            bus.write32(addr & !3, self.regs[rd]);
            2
        }
    }

    fn thumb_load_address(&mut self, instr: u32) -> u32 {
        let sp = instr & (1 << 11) != 0;
        let rd = ((instr >> 8) & 7) as usize;
        let offset = (instr & 0xFF) << 2;

        self.regs[rd] = if sp {
            self.regs[13].wrapping_add(offset)
        } else {
            (self.regs[15].wrapping_add(2) & !2).wrapping_add(offset)
        };
        1
    }

    fn thumb_add_sp(&mut self, instr: u32) -> u32 {
        let offset = (instr & 0x7F) << 2;
        if instr & (1 << 7) != 0 {
            self.regs[13] = self.regs[13].wrapping_sub(offset);
        } else {
            self.regs[13] = self.regs[13].wrapping_add(offset);
        }
        1
    }

    fn thumb_push_pop(&mut self, instr: u32, bus: &mut impl Arm7Bus) -> u32 {
        let pop = instr & (1 << 11) != 0;
        let pc_lr = instr & (1 << 8) != 0;
        let rlist = instr & 0xFF;

        if pop {
            let mut addr = self.regs[13];
            for i in 0..8u32 {
                if rlist & (1 << i) != 0 {
                    self.regs[i as usize] = bus.read32(addr & !3);
                    addr = addr.wrapping_add(4);
                }
            }
            if pc_lr {
                let val = bus.read32(addr & !3);
                self.regs[15] = val & !1;
                if val & 1 == 0 {
                    self.cpsr &= !CPSR_T;
                }
                addr = addr.wrapping_add(4);
            }
            self.regs[13] = addr;
        } else {
            let count = rlist.count_ones() + if pc_lr { 1 } else { 0 };
            let mut addr = self.regs[13].wrapping_sub(count * 4);
            self.regs[13] = addr;
            for i in 0..8u32 {
                if rlist & (1 << i) != 0 {
                    bus.write32(addr & !3, self.regs[i as usize]);
                    addr = addr.wrapping_add(4);
                }
            }
            if pc_lr {
                bus.write32(addr & !3, self.regs[14]);
            }
        }
        rlist.count_ones() + if pc_lr { 1 } else { 0 } + if pop { 2 } else { 1 }
    }

    fn thumb_multiple_load_store(&mut self, instr: u32, bus: &mut impl Arm7Bus) -> u32 {
        let load = instr & (1 << 11) != 0;
        let rb = ((instr >> 8) & 7) as usize;
        let rlist = instr & 0xFF;
        let mut addr = self.regs[rb];

        for i in 0..8u32 {
            if rlist & (1 << i) == 0 {
                continue;
            }
            if load {
                self.regs[i as usize] = bus.read32(addr & !3);
            } else {
                bus.write32(addr & !3, self.regs[i as usize]);
            }
            addr = addr.wrapping_add(4);
        }

        // Writeback
        if !load || (rlist & (1 << rb)) == 0 {
            self.regs[rb] = addr;
        }

        rlist.count_ones() + if load { 2 } else { 1 }
    }

    fn thumb_cond_branch(&mut self, instr: u32) -> u32 {
        let cond = (instr >> 8) & 0xF;
        if self.condition_passed(cond) {
            let offset = ((instr & 0xFF) as i8 as i32) << 1;
            self.regs[15] = ((self.regs[15].wrapping_add(2)) as i32).wrapping_add(offset) as u32;
            3
        } else {
            1
        }
    }

    fn thumb_unconditional_branch(&mut self, instr: u32) -> u32 {
        let offset = (((instr & 0x7FF) as i32) << 21) >> 20;
        self.regs[15] = ((self.regs[15].wrapping_add(2)) as i32).wrapping_add(offset) as u32;
        3
    }

    fn thumb_long_branch(&mut self, instr: u32) -> u32 {
        let hi = instr & (1 << 11) != 0;
        let offset = instr & 0x7FF;

        if !hi {
            // First instruction: LR = PC + (offset << 12)
            let off = ((offset as i32) << 21) >> 9;
            self.regs[14] = ((self.regs[15].wrapping_add(2)) as i32).wrapping_add(off) as u32;
            1
        } else {
            // Second instruction: PC = LR + (offset << 1), LR = next_instr | 1
            let next_pc = self.regs[15]; // address of next instruction
            self.regs[15] = self.regs[14].wrapping_add(offset << 1);
            self.regs[14] = next_pc | 1;
            3
        }
    }

    fn thumb_swi(&mut self, bus: &mut impl Arm7Bus) -> u32 {
        let swi_instr = bus.read16(self.regs[15].wrapping_sub(2) & !1);
        let swi_num = (swi_instr & 0xFF) as u32;
        self.handle_swi(swi_num, bus);
        3
    }

    fn handle_swi(&mut self, num: u32, bus: &mut impl Arm7Bus) {
        match num {
            0x01 => {
                // RegisterRamReset
                self.swi_register_ram_reset(bus);
            }
            0x02 => {
                // Halt
                self.halted = true;
            }
            0x04 => {
                // IntrWait
                self.halted = true;
            }
            0x05 => {
                // VBlankIntrWait
                self.halted = true;
            }
            0x06 => {
                // Div: R0/R1
                let num = self.regs[0] as i32;
                let den = self.regs[1] as i32;
                if den != 0 {
                    self.regs[0] = (num / den) as u32;
                    self.regs[1] = (num % den) as u32;
                    self.regs[3] = (num / den).unsigned_abs();
                }
            }
            0x07 => {
                // DivArm: R1/R0
                let num = self.regs[1] as i32;
                let den = self.regs[0] as i32;
                if den != 0 {
                    self.regs[0] = (num / den) as u32;
                    self.regs[1] = (num % den) as u32;
                    self.regs[3] = (num / den).unsigned_abs();
                }
            }
            0x08 => {
                // Sqrt
                let val = self.regs[0];
                self.regs[0] = (val as f64).sqrt() as u32;
            }
            0x0B => {
                // CpuSet
                let src = self.regs[0];
                let dst = self.regs[1];
                let ctrl = self.regs[2];
                let count = ctrl & 0x1FFFFF;
                let fill = ctrl & (1 << 24) != 0;
                let word = ctrl & (1 << 26) != 0;

                if word {
                    let fill_val = if fill { bus.read32(src) } else { 0 };
                    for i in 0..count {
                        let val = if fill { fill_val } else { bus.read32(src.wrapping_add(i * 4)) };
                        bus.write32(dst.wrapping_add(i * 4), val);
                    }
                } else {
                    let fill_val = if fill { bus.read16(src) } else { 0 };
                    for i in 0..count {
                        let val = if fill { fill_val } else { bus.read16(src.wrapping_add(i * 2)) };
                        bus.write16(dst.wrapping_add(i * 2), val);
                    }
                }
            }
            0x0C => {
                // CpuFastSet (32-bit, multiples of 8 words)
                let src = self.regs[0];
                let dst = self.regs[1];
                let ctrl = self.regs[2];
                let count = (ctrl & 0x1FFFFF) & !7;
                let fill = ctrl & (1 << 24) != 0;
                let fill_val = if fill { bus.read32(src) } else { 0 };

                for i in 0..count {
                    let val = if fill { fill_val } else { bus.read32(src.wrapping_add(i * 4)) };
                    bus.write32(dst.wrapping_add(i * 4), val);
                }
            }
            0x0E => {
                // BgAffineSet
                self.swi_bg_affine_set(bus);
            }
            0x0F => {
                // ObjAffineSet
                self.swi_obj_affine_set(bus);
            }
            0x11 => {
                // LZ77UnCompWram
                self.swi_lz77_decompress_wram(bus);
            }
            0x12 => {
                // LZ77UnCompVram
                self.swi_lz77_decompress_vram(bus);
            }
            _ => {
                log::warn!("Unimplemented SWI: 0x{:02X}", num);
            }
        }
    }

    fn bios_sin_cos(angle: u8) -> (i32, i32) {
        let radians = (angle as f64) * (2.0 * PI / 256.0);
        let sin = (radians.sin() * 4096.0).round() as i32;
        let cos = (radians.cos() * 4096.0).round() as i32;
        (sin, cos)
    }

    fn swi_bg_affine_set(&mut self, bus: &mut impl Arm7Bus) {
        let src = self.regs[0];
        let dst = self.regs[1];
        let count = self.regs[2];

        for i in 0..count {
            let src_entry = src.wrapping_add(i * 20);
            let dst_entry = dst.wrapping_add(i * 16);

            let tex_x = bus.read32(src_entry) as i32;
            let tex_y = bus.read32(src_entry.wrapping_add(4)) as i32;
            let scr_x = bus.read16(src_entry.wrapping_add(8)) as i16 as i32;
            let scr_y = bus.read16(src_entry.wrapping_add(10)) as i16 as i32;
            let scale_x = bus.read16(src_entry.wrapping_add(12)) as i16 as i32;
            let scale_y = bus.read16(src_entry.wrapping_add(14)) as i16 as i32;
            let angle = (bus.read16(src_entry.wrapping_add(16)) >> 8) as u8;

            let (sin, cos) = Self::bios_sin_cos(angle);
            let pa = ((scale_x * cos) >> 12) as i16;
            let pb = ((-(scale_x * sin)) >> 12) as i16;
            let pc = ((scale_y * sin) >> 12) as i16;
            let pd = ((scale_y * cos) >> 12) as i16;
            let dx = tex_x - (pa as i32 * scr_x + pb as i32 * scr_y);
            let dy = tex_y - (pc as i32 * scr_x + pd as i32 * scr_y);

            bus.write16(dst_entry, pa as u16);
            bus.write16(dst_entry.wrapping_add(2), pb as u16);
            bus.write16(dst_entry.wrapping_add(4), pc as u16);
            bus.write16(dst_entry.wrapping_add(6), pd as u16);
            bus.write32(dst_entry.wrapping_add(8), dx as u32);
            bus.write32(dst_entry.wrapping_add(12), dy as u32);
        }
    }

    fn swi_obj_affine_set(&mut self, bus: &mut impl Arm7Bus) {
        let src = self.regs[0];
        let dst = self.regs[1];
        let count = self.regs[2];
        let offset = self.regs[3];

        for i in 0..count {
            let src_entry = src.wrapping_add(i * 8);
            let dst_entry = dst.wrapping_add(i * offset * 4);

            let scale_x = bus.read16(src_entry) as i16 as i32;
            let scale_y = bus.read16(src_entry.wrapping_add(2)) as i16 as i32;
            let angle = (bus.read16(src_entry.wrapping_add(4)) >> 8) as u8;

            let (sin, cos) = Self::bios_sin_cos(angle);
            let pa = ((scale_x * cos) >> 12) as i16;
            let pb = ((-(scale_x * sin)) >> 12) as i16;
            let pc = ((scale_y * sin) >> 12) as i16;
            let pd = ((scale_y * cos) >> 12) as i16;

            bus.write16(dst_entry, pa as u16);
            bus.write16(dst_entry.wrapping_add(offset), pb as u16);
            bus.write16(dst_entry.wrapping_add(offset * 2), pc as u16);
            bus.write16(dst_entry.wrapping_add(offset * 3), pd as u16);
        }
    }

    fn swi_lz77_decompress_wram(&mut self, bus: &mut impl Arm7Bus) {
        let src = self.regs[0];
        let dst = self.regs[1];
        let header = bus.read32(src);
        let decompressed_size = header >> 8;

        let mut src_pos = src + 4;
        let mut dst_pos = dst;
        let mut remaining = decompressed_size;

        while remaining > 0 {
            let flags = bus.read8(src_pos);
            src_pos += 1;

            for bit in (0..8).rev() {
                if remaining == 0 {
                    break;
                }
                if flags & (1 << bit) != 0 {
                    let b1 = bus.read8(src_pos) as u32;
                    let b2 = bus.read8(src_pos + 1) as u32;
                    src_pos += 2;
                    let length = ((b1 >> 4) + 3).min(remaining);
                    let disp = ((b1 & 0xF) << 8 | b2) + 1;
                    for _ in 0..length {
                        let val = bus.read8(dst_pos.wrapping_sub(disp));
                        bus.write8(dst_pos, val);
                        dst_pos += 1;
                        remaining -= 1;
                    }
                } else {
                    let val = bus.read8(src_pos);
                    src_pos += 1;
                    bus.write8(dst_pos, val);
                    dst_pos += 1;
                    remaining -= 1;
                }
            }
        }
    }

    fn swi_lz77_decompress_vram(&mut self, bus: &mut impl Arm7Bus) {
        let src = self.regs[0];
        let dst = self.regs[1];
        let header = bus.read32(src);
        let decompressed_size = header >> 8;

        let mut src_pos = src + 4;
        let mut out: Vec<u8> = Vec::with_capacity(decompressed_size as usize);

        while (out.len() as u32) < decompressed_size {
            let flags = bus.read8(src_pos);
            src_pos += 1;

            for bit in (0..8).rev() {
                if (out.len() as u32) >= decompressed_size {
                    break;
                }
                if flags & (1 << bit) != 0 {
                    let b1 = bus.read8(src_pos) as u32;
                    let b2 = bus.read8(src_pos + 1) as u32;
                    src_pos += 2;
                    let length = (b1 >> 4) + 3;
                    let disp = ((b1 & 0xF) << 8 | b2) + 1;
                    for _ in 0..length {
                        if (out.len() as u32) >= decompressed_size {
                            break;
                        }
                        let back = out.len().saturating_sub(disp as usize);
                        let val = out[back];
                        out.push(val);
                    }
                } else {
                    let val = bus.read8(src_pos);
                    src_pos += 1;
                    out.push(val);
                }
            }
        }

        // VRAM variant writes 16-bit units.
        let mut dst_pos = dst;
        let mut i = 0usize;
        while i < out.len() {
            let lo = out[i] as u16;
            let hi = if i + 1 < out.len() { out[i + 1] as u16 } else { 0 };
            bus.write16(dst_pos, lo | (hi << 8));
            dst_pos = dst_pos.wrapping_add(2);
            i += 2;
        }
    }

    fn swi_register_ram_reset(&mut self, bus: &mut impl Arm7Bus) {
        let flags = self.regs[0] as u8;

        if flags & 0x01 != 0 {
            for addr in 0x0200_0000..=0x0203_FFFF {
                bus.write8(addr, 0);
            }
        }
        if flags & 0x02 != 0 {
            // BIOS keeps the top 0x200 bytes of IWRAM intact.
            for addr in 0x0300_0000..=0x0300_7DFF {
                bus.write8(addr, 0);
            }
        }
        if flags & 0x04 != 0 {
            for addr in 0x0500_0000..=0x0500_03FF {
                bus.write8(addr, 0);
            }
        }
        if flags & 0x08 != 0 {
            for addr in 0x0600_0000..=0x0601_7FFF {
                bus.write8(addr, 0);
            }
        }
        if flags & 0x10 != 0 {
            for addr in 0x0700_0000..=0x0700_03FF {
                bus.write8(addr, 0);
            }
        }
        if flags & 0x20 != 0 {
            for addr in 0x0400_0120..=0x0400_012A {
                bus.write8(addr, 0);
            }
            bus.write8(0x0400_0134, 0);
            bus.write8(0x0400_0135, 0);
        }
        if flags & 0x40 != 0 {
            for addr in 0x0400_0060..=0x0400_00A7 {
                bus.write8(addr, 0);
            }
            // Restore the GBA default audio bias.
            bus.write8(0x0400_0088, 0x00);
            bus.write8(0x0400_0089, 0x02);
        }
        if flags & 0x80 != 0 {
            for addr in 0x0400_0000..=0x0400_0055 {
                bus.write8(addr, 0);
            }
            for addr in 0x0400_00B0..=0x0400_00DF {
                bus.write8(addr, 0);
            }
            for addr in 0x0400_0100..=0x0400_0111 {
                bus.write8(addr, 0);
            }
            for addr in 0x0400_0200..=0x0400_020B {
                bus.write8(addr, 0);
            }
        }
    }
}

impl Default for Arm7Tdmi {
    fn default() -> Self {
        Self::new()
    }
}
