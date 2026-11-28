use crate::memory::{
    MemoryError, PhysAddr, SegmentedAddr, read_byte, read_far_pointer, read_word, write_byte,
    write_word,
};
use crate::registers::{GeneralReg, Reg8, Reg16, SegmentReg};
use core::fmt::{self, Debug};
use mod_rm::{RegisterOrMemoryOperand, parse_mod_rm};
use std::fmt::Display;
use thiserror::Error;
pub(crate) use value::CpuValue;

mod mod_rm;
mod value;

#[cfg(test)]
mod tests;

pub(crate) struct Cpu {
    pub(crate) ax: u16,
    pub(crate) bx: u16,
    pub(crate) cx: u16,
    pub(crate) dx: u16,
    pub(crate) si: u16,
    pub(crate) di: u16,
    pub(crate) sp: u16,
    pub(crate) bp: u16,
    pub(crate) ip: u16,
    pub(crate) cs: u16,
    pub(crate) ds: u16,
    pub(crate) es: u16,
    pub(crate) ss: u16,
    pub(crate) flags: u16,
    pub(crate) halted: bool,
    pub(crate) model: CpuModel,
    pub(crate) verbosity: u8,
}

#[derive(Error, Debug)]
pub(crate) enum CpuError {
    #[error("Unsupported instruction: {0:02X}h")]
    UnsupportedInstruction(u8),
    #[error("Unsupported mod field: {0}")]
    UnsupportedModField(u8),
    #[error("Invalid register value: {0}")]
    InvalidRegisterValue(u8),
    #[error("Unsupported r/m field: {0}")]
    UnsupportedRmField(u8),
    #[error("Unsupported ModRm register field in {0:02X}h: {1} ({1:b})")]
    UnsupportedRegisterField(u8, u8),
    #[error("Memory Error: {0}")]
    MemoryError(#[from] MemoryError),
    #[error("Unsupported ALU operation: {0}")]
    UnsupportedAluOp(u8),
    #[error("Unsupported shift operation: {0}")]
    UnsupportedShiftOp(u8),
}

#[allow(dead_code)]
pub(crate) enum CpuStepResult {
    Continue,
    Interrupt(u8),
    PortRead8 { port: u16 },
    PortWrite8 { port: u16, value: u8 },
    PortRead16 { port: u16 },
    PortWrite16 { port: u16, value: u16 },
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(crate) enum Flag {
    Carry = 0,           // CF
    Parity = 2,          // PF
    AuxiliaryCarry = 4,  // AF
    Zero = 6,            // ZF
    Sign = 7,            // SF
    Overflow = 11,       // OF
    InterruptEnable = 9, // IF
    Direction = 10,      // DF
    Trap = 8,            // TF
}

#[derive(PartialEq, Eq)]
pub(crate) enum CpuModel {
    I8088,
    I80286,
}

impl Cpu {
    pub(crate) fn new(model: CpuModel, verbosity: u8) -> Self {
        let mut cpu = Cpu {
            ax: 0x0,
            bx: 0x0,
            cx: 0x0,
            dx: 0x0,
            si: 0x0,
            di: 0x0,
            sp: 0x0,
            bp: 0x0,
            ip: 0x0,
            cs: 0x0,
            ds: 0x0,
            es: 0x0,
            ss: 0x0,
            flags: 0,
            halted: false,
            model,
            verbosity,
        };
        cpu.flags = cpu.normalize_flags(0);
        cpu
    }

    pub(crate) fn set_flag(&mut self, flag: Flag, value: bool) {
        let mask = 1 << (flag as u8);
        self.flags = (self.flags & !mask) | ((value as u16) << (flag as u8));
    }

    pub(crate) fn get_flag(&self, flag: Flag) -> bool {
        let mask = 1 << (flag as u8);
        self.flags & mask != 0
    }

    pub(crate) fn set_reg8(&mut self, reg: Reg8, value: u8) {
        match reg {
            Reg8::Al => self.ax = (self.ax & 0xFF00) | value as u16,
            Reg8::Bl => self.bx = (self.bx & 0xFF00) | value as u16,
            Reg8::Cl => self.cx = (self.cx & 0xFF00) | value as u16,
            Reg8::Dl => self.dx = (self.dx & 0xFF00) | value as u16,
            Reg8::Ah => self.ax = (self.ax & 0x00FF) | ((value as u16) << 8),
            Reg8::Bh => self.bx = (self.bx & 0x00FF) | ((value as u16) << 8),
            Reg8::Ch => self.cx = (self.cx & 0x00FF) | ((value as u16) << 8),
            Reg8::Dh => self.dx = (self.dx & 0x00FF) | ((value as u16) << 8),
        }
    }

    pub(crate) fn get_reg8(&self, reg: Reg8) -> u8 {
        (match reg {
            Reg8::Al => self.ax,
            Reg8::Bl => self.bx,
            Reg8::Cl => self.cx,
            Reg8::Dl => self.dx,
            Reg8::Ah => self.ax >> 8,
            Reg8::Bh => self.bx >> 8,
            Reg8::Ch => self.cx >> 8,
            Reg8::Dh => self.dx >> 8,
        }) as u8
    }

    pub(crate) fn set_segment_reg(&mut self, reg: SegmentReg, value: u16) {
        match reg {
            SegmentReg::Es => self.es = value,
            SegmentReg::Cs => self.cs = value,
            SegmentReg::Ss => self.ss = value,
            SegmentReg::Ds => self.ds = value,
        }
    }

    pub(crate) fn get_segment_reg(&self, reg: SegmentReg) -> u16 {
        match reg {
            SegmentReg::Es => self.es,
            SegmentReg::Cs => self.cs,
            SegmentReg::Ss => self.ss,
            SegmentReg::Ds => self.ds,
        }
    }

    pub(crate) fn set_reg16(&mut self, reg: Reg16, value: u16) {
        match reg {
            Reg16::Ax => self.ax = value,
            Reg16::Cx => self.cx = value,
            Reg16::Dx => self.dx = value,
            Reg16::Bx => self.bx = value,
            Reg16::Sp => self.sp = value,
            Reg16::Bp => self.bp = value,
            Reg16::Si => self.si = value,
            Reg16::Di => self.di = value,
        }
    }

    pub(crate) fn get_reg16(&self, reg: Reg16) -> u16 {
        match reg {
            Reg16::Ax => self.ax,
            Reg16::Cx => self.cx,
            Reg16::Dx => self.dx,
            Reg16::Bx => self.bx,
            Reg16::Sp => self.sp,
            Reg16::Bp => self.bp,
            Reg16::Si => self.si,
            Reg16::Di => self.di,
        }
    }

    pub(crate) fn get_reg<R: GeneralReg>(&self, reg: R) -> R::Value {
        reg.get(self)
    }

    pub(crate) fn set_reg<R: GeneralReg>(&mut self, reg: R, value: R::Value) {
        reg.set(self, value)
    }

    fn read_operand<R: GeneralReg + Copy>(
        &self,
        memory: &[u8],
        operand: &RegisterOrMemoryOperand<R>,
    ) -> Result<R::Value, CpuError> {
        let value = match operand {
            RegisterOrMemoryOperand::Memory(seg_reg, addr) => {
                let seg_addr = self.get_segment_reg(*seg_reg);
                R::Value::read(memory, SegmentedAddr::new(seg_addr, *addr))?
            }
            RegisterOrMemoryOperand::Register(reg) => self.get_reg(*reg),
        };
        Ok(value)
    }

    fn write_operand<R: GeneralReg + Copy>(
        &mut self,
        memory: &mut [u8],
        operand: &RegisterOrMemoryOperand<R>,
        value: R::Value,
    ) -> Result<(), CpuError> {
        match operand {
            RegisterOrMemoryOperand::Memory(seg_reg, addr) => {
                let seg_addr = self.get_segment_reg(*seg_reg);
                value.write(memory, SegmentedAddr::new(seg_addr, *addr))?
            }
            RegisterOrMemoryOperand::Register(reg) => self.set_reg(*reg, value),
        };
        Ok(())
    }

    fn update_logic_flags<V: CpuValue>(&mut self, result: V) {
        self.set_flag(Flag::Carry, false);
        self.set_flag(Flag::Overflow, false);
        self.set_result_flags(result);
    }

    /// Fetch, decode, and execute one instruction
    pub(crate) fn step(&mut self, memory: &mut [u8]) -> Result<CpuStepResult, CpuError> {
        if self.halted {
            return Ok(CpuStepResult::Continue);
        }

        let instruction_ip = self.ip;
        let mut byte;
        let mut segment_override = None;
        let mut rep_prefix = None;

        loop {
            byte = self.fetch_next_byte_and_advance_ip(memory);

            // Check for segment override prefix
            let prefix_segment = match byte {
                0x2E => Some(SegmentReg::Cs),
                0x26 => Some(SegmentReg::Es),
                0x36 => Some(SegmentReg::Ss),
                0x3E => Some(SegmentReg::Ds),
                _ => None,
            };
            if let Some(seg) = prefix_segment {
                segment_override = Some(seg);
                if self.verbosity > 1 {
                    eprint!("\x1b[1m{:#04X}\x1b[0m \x1b[92m({})\x1b[0m ", byte, seg);
                }
                continue;
            }

            // Check for REPE/REPNE prefix
            if byte == 0xF2 || byte == 0xF3 {
                rep_prefix = Some(byte);
                if self.verbosity > 1 {
                    eprint!("\x1b[1m{:#04X}\x1b[0m ", byte);
                }
                continue;
            }

            break;
        }

        let mut result = CpuStepResult::Continue;
        // Table 4-13
        match byte {
            // ALU ops
            0x00 | 0x08 | 0x10 | 0x18 | 0x20 | 0x28 | 0x30 | 0x38 | 0x02 | 0x0A | 0x12 | 0x1A
            | 0x22 | 0x2A | 0x32 | 0x3A => {
                // ALU ops on r/m8, r8 and r8, r/m8
                let op = AluOp::try_from((byte >> 3) & 7)?;
                let (reg, rm) = self.fetch_reg_rm::<Reg8>(memory, segment_override)?;
                let reg = RegisterOrMemoryOperand::Register(reg);
                let (dest, source) = if byte & 2 == 0 { (rm, reg) } else { (reg, rm) };
                self.print_instruction(byte, &format_args!("{} {}, {}", op, dest, source));

                let lhs = self.read_operand(memory, &dest)?;
                let rhs = self.read_operand(memory, &source)?;
                let result = self.alu8(op, lhs, rhs);

                if op != AluOp::Cmp {
                    self.write_operand(memory, &dest, result)?;
                }
            }
            0x01 | 0x09 | 0x11 | 0x19 | 0x21 | 0x29 | 0x31 | 0x39 | 0x03 | 0x0B | 0x13 | 0x1B
            | 0x23 | 0x2B | 0x33 | 0x3B => {
                // ALU ops on r/m16, r16 and r16, r/m16
                let op = AluOp::try_from((byte >> 3) & 7)?;
                let (reg, rm) = self.fetch_reg_rm::<Reg16>(memory, segment_override)?;
                let reg = RegisterOrMemoryOperand::Register(reg);
                let (dest, source) = if byte & 2 == 0 { (rm, reg) } else { (reg, rm) };
                self.print_instruction(byte, &format_args!("{} {}, {}", op, dest, source));

                let lhs = self.read_operand(memory, &dest)?;
                let rhs = self.read_operand(memory, &source)?;
                let result = self.alu16(op, lhs, rhs);

                if op != AluOp::Cmp {
                    self.write_operand(memory, &dest, result)?;
                }
            }
            0x04 | 0x0C | 0x14 | 0x1C | 0x24 | 0x2C | 0x34 | 0x3C => {
                // ALU ops on AL, IMMED8
                let op = AluOp::try_from((byte >> 3) & 7)?;
                let immediate = self.fetch_next_byte_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("{} al, {:X}h", op, immediate));

                let result = self.alu8(op, self.get_reg(Reg8::Al), immediate);
                if op != AluOp::Cmp {
                    self.set_reg(Reg8::Al, result);
                }
            }
            0x05 | 0x0D | 0x15 | 0x1D | 0x25 | 0x2D | 0x35 | 0x3D => {
                // ALU ops on AX, IMMED16
                let op = AluOp::try_from((byte >> 3) & 7)?;
                let immediate = self.fetch_next_word_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("{} ax, {:04X}h", op, immediate));

                let result = self.alu16(op, self.get_reg(Reg16::Ax), immediate);
                if op != AluOp::Cmp {
                    self.set_reg(Reg16::Ax, result);
                }
            }
            0x06 => {
                // PUSH ES
                self.print_instruction(byte, &format_args!("push es"));
                self.push_onto_stack(memory, self.es)?;
            }
            0x07 => {
                // POP ES
                self.print_instruction(byte, &format_args!("pop es"));
                self.es = self.pop_from_stack(memory)?;
            }
            0x0E => {
                // PUSH CS
                self.print_instruction(byte, &format_args!("push cs"));
                self.push_onto_stack(memory, self.cs)?;
            }
            0x16 => {
                // PUSH SS
                self.print_instruction(byte, &format_args!("push ss"));
                self.push_onto_stack(memory, self.ss)?;
            }
            0x17 => {
                // POP SS
                self.print_instruction(byte, &format_args!("pop ss"));
                self.ss = self.pop_from_stack(memory)?;
                // TODO: delay interrupt recognition until after next instruction
            }
            0x1E => {
                // PUSH DS
                self.print_instruction(byte, &format_args!("push ds"));
                self.push_onto_stack(memory, self.ds)?;
            }
            0x1F => {
                // POP DS
                self.print_instruction(byte, &format_args!("pop ds"));
                self.ds = self.pop_from_stack(memory)?;
            }
            0x27 => {
                // Decimal Adjust AL After Addition
                self.print_instruction(byte, &format_args!("daa"));
                let al = self.get_reg(Reg8::Al);
                let original_af = self.get_flag(Flag::AuxiliaryCarry);
                let mut result = al;
                // If low nibble is greater than 9 or incoming AF is set
                if (al & 0x0f) > 0b1001 || self.get_flag(Flag::AuxiliaryCarry) {
                    result = result.wrapping_add(0x06);
                    self.set_flag(Flag::AuxiliaryCarry, true);
                } else {
                    self.set_flag(Flag::AuxiliaryCarry, false);
                }
                if al > if original_af { 0x9F } else { 0x99 } || self.get_flag(Flag::Carry) {
                    result = result.wrapping_add(0x60);
                    self.set_flag(Flag::Carry, true);
                } else {
                    self.set_flag(Flag::Carry, false);
                }
                self.set_reg(Reg8::Al, result);
                self.set_result_flags(result);
            }
            0x2F => {
                // DAS - Decimal Adjust AL After Subtraction
                self.print_instruction(byte, &format_args!("das"));
                let original_al = self.get_reg(Reg8::Al);
                let original_cf = self.get_flag(Flag::Carry);
                let original_af = self.get_flag(Flag::AuxiliaryCarry);
                let mut result = original_al;

                if (result & 0x0F) > 9 || self.get_flag(Flag::AuxiliaryCarry) {
                    result = result.wrapping_sub(0x06) & 0xFF;
                    self.set_flag(Flag::AuxiliaryCarry, true);
                } else {
                    self.set_flag(Flag::AuxiliaryCarry, false);
                }

                if original_al > (if original_af { 0x9F } else { 0x99 }) || original_cf {
                    result = result.wrapping_sub(0x60) & 0xFF;
                    self.set_flag(Flag::Carry, true);
                } else {
                    self.set_flag(Flag::Carry, false);
                }

                self.set_reg(Reg8::Al, result);
                self.set_result_flags(result);
            }
            0x37 => {
                // AAA: ASCII Adjust After Addition
                self.print_instruction(byte, &format_args!("aaa"));
                let mut al = self.get_reg(Reg8::Al);
                let flag;
                if (al & 0x0F) > 9 || self.get_flag(Flag::AuxiliaryCarry) {
                    al = al.wrapping_add(6);
                    self.set_reg(Reg8::Ah, self.get_reg(Reg8::Ah).wrapping_add(1));
                    flag = true;
                } else {
                    flag = false;
                }
                self.set_flag(Flag::AuxiliaryCarry, flag);
                self.set_flag(Flag::Carry, flag);
                self.set_reg(Reg8::Al, al & 0x0F);
            }
            0x3F => {
                // AAS: ASCII Adjust After Subtraction
                self.print_instruction(byte, &format_args!("aas"));
                let mut al = self.get_reg(Reg8::Al);
                if (al & 0xf) > 9 || self.get_flag(Flag::AuxiliaryCarry) {
                    al = al.wrapping_sub(6);
                    self.set_reg(Reg8::Ah, self.get_reg(Reg8::Ah).wrapping_sub(1));
                    self.set_flag(Flag::AuxiliaryCarry, true);
                    self.set_flag(Flag::Carry, true);
                } else {
                    self.set_flag(Flag::AuxiliaryCarry, false);
                    self.set_flag(Flag::Carry, false);
                }
                // Clear the upper nibble
                self.set_reg(Reg8::Al, al & 0xf);
            }
            // Register INC/DEC
            0x40..=0x47 => self.inc16(byte, Reg16::try_from(byte & 7)?),
            0x48..=0x4F => self.dec16(byte, Reg16::try_from(byte & 7)?),
            // PUSH/POP 50-5F
            0x50..=0x57 => {
                let reg = Reg16::try_from(byte & 7)?;
                self.print_instruction(byte, &format_args!("push {}", reg));
                let value = if self.model == CpuModel::I8088 && reg == Reg16::Sp {
                    // 8086/8088 hardware quirk
                    self.sp.wrapping_sub(2)
                } else {
                    self.get_reg(reg)
                };
                self.push_onto_stack(memory, value)?;
            }
            0x58..=0x5F => {
                let reg = Reg16::try_from(byte & 7)?;
                self.print_instruction(byte, &format_args!("pop {}", reg));
                let value = self.pop_from_stack(memory)?;
                self.set_reg(reg, value);
            }
            0x60 if self.model == CpuModel::I80286 => {
                // 286: PUSHA (save all general purpose registers)
                self.print_instruction(byte, &format_args!("pusha"));
                let original_sp = self.sp;
                self.push_onto_stack(memory, self.ax)?;
                self.push_onto_stack(memory, self.cx)?;
                self.push_onto_stack(memory, self.dx)?;
                self.push_onto_stack(memory, self.bx)?;
                self.push_onto_stack(memory, original_sp)?;
                self.push_onto_stack(memory, self.bp)?;
                self.push_onto_stack(memory, self.si)?;
                self.push_onto_stack(memory, self.di)?;
            }
            0x61 if self.model == CpuModel::I80286 => {
                // 286: POPA (restore all general purpose registers)
                self.print_instruction(byte, &format_args!("popa"));
                self.di = self.pop_from_stack(memory)?;
                self.si = self.pop_from_stack(memory)?;
                self.bp = self.pop_from_stack(memory)?;
                self.sp = self.sp.wrapping_add(2);
                self.bx = self.pop_from_stack(memory)?;
                self.dx = self.pop_from_stack(memory)?;
                self.cx = self.pop_from_stack(memory)?;
                self.ax = self.pop_from_stack(memory)?;
            }
            // Jumps
            0x60 | 0x70 if byte == 0x70 || self.model == CpuModel::I8088 => {
                // Jump if Overflow
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                self.print_instruction(byte, &format_args!("jo {:X}h", operand));
                if self.get_flag(Flag::Overflow) {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x61 | 0x71 if byte == 0x71 || self.model == CpuModel::I8088 => {
                // Jump if No Overflow
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                self.print_instruction(byte, &format_args!("jno {:X}h", operand));
                if !self.get_flag(Flag::Overflow) {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x62 if self.model == CpuModel::I80286 => {
                // BOUND /r
                let (reg, rm) = self.fetch_reg_rm::<Reg16>(memory, segment_override)?;
                self.print_instruction(byte, &format_args!("bound {}, {}", reg, rm));

                // The second operand must be a memory operand
                let RegisterOrMemoryOperand::Memory(segment, offset) = rm else {
                    self.ip = instruction_ip;
                    return Ok(CpuStepResult::Interrupt(6)); // #UD in protected mode
                };

                // All two words must fit be below 0FFDh in real mode
                if offset >= 0xFFFD {
                    self.ip = instruction_ip;
                    return Ok(CpuStepResult::Interrupt(13));
                }

                let address = SegmentedAddr::new(self.get_segment_reg(segment), offset);
                let lower_word = read_word(memory, address)? as i16;
                let upper_word = read_word(memory, address.wrapping_add(2))? as i16;
                let index = self.get_reg16(reg) as i16;

                if index < lower_word || index > upper_word {
                    self.ip = instruction_ip;
                    return Ok(CpuStepResult::Interrupt(5));
                }
            }
            0x62 | 0x72 if byte == 0x72 || self.model == CpuModel::I8088 => {
                // Jump if Below
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                self.print_instruction(byte, &format_args!("jb {:X}h", operand));
                // If CF is 1, then jump ("jump if carry")
                if self.get_flag(Flag::Carry) {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x63 | 0x73 => {
                // JAE/JNC/JNB
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                self.print_instruction(byte, &format_args!("jae {:X}h", operand));
                // Jump if CF is clear
                if !self.get_flag(Flag::Carry) {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x64 | 0x74 => {
                // JE/JZ
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                self.print_instruction(byte, &format_args!("je short {:X}h", operand));
                // If zero flag is set, then jump
                if self.get_flag(Flag::Zero) {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x65 | 0x75 => {
                // JNE/JNZ
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                self.print_instruction(byte, &format_args!("jnz short {:X}h", operand));
                // If zero flag is not set, then jump
                if !self.get_flag(Flag::Zero) {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x66 | 0x76 => {
                // JBE/JNA (jump if below or equal)
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                self.print_instruction(byte, &format_args!("jbe short {:X}h", operand));
                // If CF or ZF are set, jump
                if self.get_flag(Flag::Carry) || self.get_flag(Flag::Zero) {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x67 | 0x77 => {
                // Jump if Above
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                self.print_instruction(byte, &format_args!("ja {:X}h", operand));
                // If CF is 0 AND ZF = 0, jump
                if !self.get_flag(Flag::Carry) && !self.get_flag(Flag::Zero) {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x68 if self.model == CpuModel::I80286 => {
                // 286: PUSH imm16
                let operand = self.fetch_next_word_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("push {:X}h", operand));
                self.push_onto_stack(memory, operand)?;
            }
            0x68 | 0x78 if byte == 0x78 || self.model == CpuModel::I8088 => {
                // JS (jump if sign)
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                self.print_instruction(byte, &format_args!("js {:X}h", operand));
                if self.get_flag(Flag::Sign) {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x69 if self.model == CpuModel::I80286 => {
                // IMUL r16, r/m16, imm16
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let rm = self.decode_rm_operand::<Reg16>(memory, &parsed, segment_override)?;
                let immediate = self.fetch_next_word_and_advance_ip(memory);
                let source = self.read_operand(memory, &rm)?;
                let dest_reg = Reg16::try_from(parsed.reg_field)?;

                self.print_instruction(
                    byte,
                    &format_args!("imul {}, {}, {}", dest_reg, rm, immediate),
                );
                let result = (source as i16 as i32) * (immediate as i16 as i32);
                self.set_reg16(dest_reg, result as u16);
                let overflow = !(-32768..=32767).contains(&result);
                self.set_flag(Flag::Carry, overflow);
                self.set_flag(Flag::Overflow, overflow);
            }
            0x69 | 0x79 if byte == 0x79 || self.model == CpuModel::I8088 => {
                // JNS (jump if not sign)
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                self.print_instruction(byte, &format_args!("jns {:X}h", operand));
                if !self.get_flag(Flag::Sign) {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x6A if self.model == CpuModel::I80286 => {
                // 286: PUSH imm8
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("push {:X}h", operand));
                self.push_onto_stack(memory, operand as i8 as u16)?;
            }
            0x6A | 0x7A if byte == 0x7A || self.model == CpuModel::I8088 => {
                // JP (jump if parity)
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                self.print_instruction(byte, &format_args!("jp {:X}h", operand));
                if self.get_flag(Flag::Parity) {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x6B if self.model == CpuModel::I80286 => {
                // IMUL r16, r/m16, imm8
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let rm = self.decode_rm_operand::<Reg16>(memory, &parsed, segment_override)?;
                let immediate = self.fetch_next_byte_and_advance_ip(memory);
                let source = self.read_operand(memory, &rm)?;
                let dest_reg = Reg16::try_from(parsed.reg_field)?;

                self.print_instruction(
                    byte,
                    &format_args!("imul {}, {}, {}", dest_reg, rm, immediate),
                );
                let result = (source as i16 as i32) * (immediate as i8 as i16 as i32);
                self.set_reg16(dest_reg, result as u16);
                let overflow = !(-32768..=32767).contains(&result);
                self.set_flag(Flag::Carry, overflow);
                self.set_flag(Flag::Overflow, overflow);
            }
            0x6B | 0x7B if byte == 0x7B || self.model == CpuModel::I8088 => {
                // JNP (jump if not parity)
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                self.print_instruction(byte, &format_args!("jnp {:X}h", operand));
                if !self.get_flag(Flag::Parity) {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x6C | 0x7C => {
                // JL/JNGE (jump if signed less-than)
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                self.print_instruction(byte, &format_args!("jl {:X}h", operand));
                // If SF != OF, jump
                if self.get_flag(Flag::Sign) != self.get_flag(Flag::Overflow) {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x6D | 0x7D => {
                // JNL/JGE (jump if not signed less-than)
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                self.print_instruction(byte, &format_args!("jnl {:X}h", operand));
                // If SF == OF, jump
                if self.get_flag(Flag::Sign) == self.get_flag(Flag::Overflow) {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x6E if self.model == CpuModel::I80286 => {
                // OUTSB
                self.print_instruction(byte, &format_args!("outsb"));

                // We go back through step() for each REP repetition
                if rep_prefix.is_some() && self.cx == 0 {
                    return Ok(CpuStepResult::Continue);
                }

                let seg = segment_override.unwrap_or(SegmentReg::Ds);
                let value = read_byte(memory, PhysAddr::new(self.get_segment_reg(seg), self.si))?;
                if self.get_flag(Flag::Direction) {
                    self.si = self.si.wrapping_sub(1);
                } else {
                    self.si = self.si.wrapping_add(1);
                }

                // Go back for another repetition, since cx wasn't 0
                if rep_prefix.is_some() {
                    self.cx = self.cx.wrapping_sub(1);
                    if self.cx > 0 {
                        self.ip = instruction_ip;
                    }
                }

                return Ok(CpuStepResult::PortWrite8 {
                    port: self.dx as u16,
                    value,
                });
            }
            0x6E | 0x7E if byte == 0x7E || self.model == CpuModel::I8088 => {
                // JLE/JNG
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                self.print_instruction(byte, &format_args!("jle {:X}h", operand));
                // If ZF or SF != OF
                if self.get_flag(Flag::Zero)
                    || (self.get_flag(Flag::Sign) != self.get_flag(Flag::Overflow))
                {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x6F if self.model == CpuModel::I80286 => {
                // OUTSW
                self.print_instruction(byte, &format_args!("outsw"));

                // We go back through step() for each REP repetition
                if rep_prefix.is_some() && self.cx == 0 {
                    return Ok(CpuStepResult::Continue);
                }

                if self.si == 0xFFFF {
                    // Cannot supply a complete word
                    self.ip = instruction_ip;
                    return Ok(CpuStepResult::Interrupt(13));
                }

                let seg = segment_override.unwrap_or(SegmentReg::Ds);
                let value = read_word(
                    memory,
                    SegmentedAddr::new(self.get_segment_reg(seg), self.si),
                )?;
                if self.get_flag(Flag::Direction) {
                    self.si = self.si.wrapping_sub(2);
                } else {
                    self.si = self.si.wrapping_add(2);
                }

                // Go back for another repetition, since cx wasn't 0
                if rep_prefix.is_some() {
                    self.cx = self.cx.wrapping_sub(1);
                    if self.cx > 0 {
                        self.ip = instruction_ip;
                    }
                }

                return Ok(CpuStepResult::PortWrite16 {
                    port: self.dx as u16,
                    value,
                });
            }
            0x6F | 0x7F if byte == 0x7F || self.model == CpuModel::I8088 => {
                // JNLE/JG (jump if not less than or equal)
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                self.print_instruction(byte, &format_args!("jg {:X}h", operand));
                // If not ZF and SF == OF
                if !self.get_flag(Flag::Zero)
                    && (self.get_flag(Flag::Sign) == self.get_flag(Flag::Overflow))
                {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x80 | 0x82 => {
                // Does a lot of ALU ops on REG8/MEM8, IMMED8
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let rm = self.decode_rm_operand::<Reg8>(memory, &parsed, segment_override)?;
                let immediate = self.fetch_next_byte_and_advance_ip(memory);
                let destination = self.read_operand(memory, &rm)?;
                let op = AluOp::try_from(parsed.reg_field)?;
                self.print_instruction(byte, &format_args!("{} {}, {:X}h", op, rm, immediate));
                let value = self.alu8(op, destination, immediate);
                if op != AluOp::Cmp {
                    self.write_operand(memory, &rm, value)?;
                }
            }
            0x81 | 0x83 => {
                // 0x81: Does a lot of ALU ops on R/M16, IMMED16
                // 0x83: Various ALU ops on r/m16, immed8
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let rm = self.decode_rm_operand::<Reg16>(memory, &parsed, segment_override)?;
                let immediate = if byte == 0x81 {
                    self.fetch_next_word_and_advance_ip(memory)
                } else {
                    self.fetch_next_byte_and_advance_ip(memory) as i8 as i16 as u16
                };
                let destination = self.read_operand(memory, &rm)?;
                let op = AluOp::try_from(parsed.reg_field)?;
                self.print_instruction(byte, &format_args!("{} {}, {:X}h", op, rm, immediate));
                let value = self.alu16(op, destination, immediate);
                if op != AluOp::Cmp {
                    self.write_operand(memory, &rm, value)?;
                }
            }
            0x84 => {
                // TEST R/M8, REG8
                let (reg, rm) = self.fetch_reg_rm::<Reg8>(memory, segment_override)?;
                let operand = self.read_operand(memory, &rm)?;
                let reg_value = self.get_reg(reg);

                self.print_instruction(byte, &format_args!("test {}, {}", rm, reg));
                let result = operand & reg_value;
                self.update_logic_flags(result);
            }
            0x85 => {
                // TEST R/M16, REG16
                let (reg, rm) = self.fetch_reg_rm::<Reg16>(memory, segment_override)?;
                let operand = self.read_operand(memory, &rm)?;
                let reg_value = self.get_reg(reg);

                self.print_instruction(byte, &format_args!("test {}, {}", rm, reg));
                let result = operand & reg_value;
                self.update_logic_flags(result);
            }
            0x86 => {
                // XCHG REG8,REG8/MEM8
                let (first_reg, rm) = self.fetch_reg_rm::<Reg8>(memory, segment_override)?;
                self.print_instruction(byte, &format_args!("xchg {}, {}", first_reg, rm));

                let first_value = self.get_reg(first_reg);
                let second_value = self.read_operand(memory, &rm)?;
                self.set_reg(first_reg, second_value);
                self.write_operand(memory, &rm, first_value)?;
            }
            0x87 => {
                // XCHG REG16, R/M16
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let first_reg = Reg16::try_from(parsed.reg_field)?;
                let rm = self.decode_rm_operand::<Reg16>(memory, &parsed, segment_override)?;
                self.print_instruction(byte, &format_args!("xchg {}, {}", first_reg, rm));

                let first_value = self.get_reg(first_reg);
                let second_value = self.read_operand(memory, &rm)?;
                self.set_reg(first_reg, second_value);
                self.write_operand(memory, &rm, first_value)?;
            }
            // MOVs
            0x88 => {
                // Mov reg8/mem8, reg8
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let reg = Reg8::try_from(parsed.reg_field)?;
                let rm = self.decode_rm_operand::<Reg8>(memory, &parsed, segment_override)?;

                self.print_instruction(byte, &format_args!("mov {}, {}", rm, reg));

                let value = self.get_reg(reg);
                self.write_operand(memory, &rm, value)?;
            }
            0x89 => {
                // mov reg16/mem16, reg16
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let reg = Reg16::try_from(parsed.reg_field)?;
                let rm = self.decode_rm_operand::<Reg16>(memory, &parsed, segment_override)?;
                self.print_instruction(byte, &format_args!("mov {}, {}", rm, reg));
                let value = self.get_reg(reg);
                self.write_operand(memory, &rm, value)?;
            }
            0x8A => {
                // MOV REG8, R/M8
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let reg = Reg8::try_from(parsed.reg_field)?;
                let rm = self.decode_rm_operand::<Reg8>(memory, &parsed, segment_override)?;
                self.print_instruction(byte, &format_args!("mov {}, {}", reg, rm));
                let value = self.read_operand(memory, &rm)?;
                self.set_reg(reg, value);
            }
            0x8B => {
                // MOV REG16, R/M16
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let reg = Reg16::try_from(parsed.reg_field)?;
                let rm = self.decode_rm_operand::<Reg16>(memory, &parsed, segment_override)?;
                self.print_instruction(byte, &format_args!("mov {}, {}", reg, rm));
                let value = self.read_operand(memory, &rm)?;
                self.set_reg(reg, value);
            }
            0x8C => {
                // MOV r/m16, segment register
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                // 8088 ignores the high order bit of the reg field
                let source_segment = SegmentReg::try_from(parsed.reg_field & 3)?;
                let rm = self.decode_rm_operand::<Reg16>(memory, &parsed, segment_override)?;
                self.print_instruction(byte, &format_args!("mov {}, {}", rm, source_segment));

                let value = self.get_segment_reg(source_segment);
                self.write_operand(memory, &rm, value)?;
            }
            0x8D => {
                // LEA REG16,MEM16
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let dest_reg = Reg16::try_from(parsed.reg_field)?;
                let rm = self.decode_rm_operand::<Reg16>(memory, &parsed, segment_override)?;
                self.print_instruction(byte, &format_args!("lea {}, {}", dest_reg, rm));
                match rm {
                    // We write the address, not the memory at address. We ignore the segment.
                    RegisterOrMemoryOperand::Memory(_, addr) => self.set_reg(dest_reg, addr),
                    RegisterOrMemoryOperand::Register(_) => {
                        // LEA can only have a memory source
                        return Err(CpuError::UnsupportedModField(parsed.mod_field));
                    }
                }
            }
            0x8E => {
                // MOV SEGREG, REG16/MEM16
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                // 8088 ignores the high order bit of the reg field
                let dest_segment = SegmentReg::try_from(parsed.reg_field & 3)?;
                let rm = self.decode_rm_operand::<Reg16>(memory, &parsed, segment_override)?;
                self.print_instruction(byte, &format_args!("mov {}, {}", dest_segment, rm));

                let value = self.read_operand(memory, &rm)?;
                self.set_segment_reg(dest_segment, value);
            }
            0x8F => {
                // POP R/M16
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                if parsed.reg_field != 0 {
                    // The register value is required to be 0
                    return Err(CpuError::UnsupportedRegisterField(byte, parsed.reg_field));
                }
                let rm = self.decode_rm_operand::<Reg16>(memory, &parsed, segment_override)?;
                self.print_instruction(byte, &format_args!("pop {}", rm));

                let value = self.pop_from_stack(memory)?;
                self.write_operand(memory, &rm, value)?;
            }
            // XCHG AX,
            0x90..=0x97 => {
                let reg = Reg16::try_from(byte & 7)?;
                self.print_instruction(byte, &format_args!("xchg ax, {}", reg));
                let other_reg_val = self.get_reg(reg);
                self.set_reg(reg, self.ax);
                self.ax = other_reg_val;
            }
            0x98 => {
                // CBW
                let al = self.get_reg(Reg8::Al);
                let value = (al as i8) as i16 as u16;
                self.print_instruction(byte, &format_args!("cbw"));
                self.set_reg(Reg16::Ax, value);
            }
            0x99 => {
                // CWD
                let result = if (self.ax & 0x8000) != 0 {
                    0xFFFF
                } else {
                    0x0000
                };
                self.print_instruction(byte, &format_args!("cwd"));
                self.set_reg(Reg16::Dx, result);
            }
            0x9A => {
                // CALL FAR_PROC
                let offset = self.fetch_next_word_and_advance_ip(memory);
                let segment = self.fetch_next_word_and_advance_ip(memory);
                self.print_instruction(
                    byte,
                    &format_args!("call far {:04X}h:{:04X}h", segment, offset),
                );

                self.push_onto_stack(memory, self.cs)?;
                self.push_onto_stack(memory, self.ip)?;
                self.cs = segment;
                self.ip = offset;
            }
            0x9C => {
                // PUSHF
                self.print_instruction(byte, &format_args!("pushf"));
                self.push_onto_stack(memory, self.flags)?;
            }
            0x9D => {
                // POPF
                self.print_instruction(byte, &format_args!("popf"));
                let flags = self.pop_from_stack(memory)?;
                self.flags = self.normalize_flags(flags);
            }
            0x9E => {
                // SAHF
                self.print_instruction(byte, &format_args!("sahf"));
                let ah = self.get_reg(Reg8::Ah) as u16;
                self.flags = (self.flags & !0x00D5) | (ah & 0xD5);
            }
            0x9F => {
                // LAHF
                self.print_instruction(byte, &format_args!("lahf"));
                self.set_reg(Reg8::Ah, (self.flags as u8 & 0xD5) | 0x02);
            }
            0xA0 => {
                // MOV AL, MEM8
                let mem_addr = self.fetch_next_word_and_advance_ip(memory);
                let seg = segment_override.unwrap_or(SegmentReg::Ds);
                self.print_instruction(byte, &format_args!("mov al, [{}:{:04X}h]", seg, mem_addr));
                let seg_addr = self.get_segment_reg(seg);
                let mem_value = read_byte(memory, PhysAddr::new(seg_addr, mem_addr))?;
                self.set_reg(Reg8::Al, mem_value);
            }
            0xA1 => {
                // MOV AX, MEM16
                let mem_addr = self.fetch_next_word_and_advance_ip(memory);
                let seg = segment_override.unwrap_or(SegmentReg::Ds);
                self.print_instruction(byte, &format_args!("mov ax, [{}:{:04X}h]", seg, mem_addr));
                let seg_addr = self.get_segment_reg(seg);
                let mem_value = read_word(memory, SegmentedAddr::new(seg_addr, mem_addr))?;
                self.set_reg(Reg16::Ax, mem_value);
            }
            0xA2 => {
                // MOV MEM8, AL
                let mem_addr = self.fetch_next_word_and_advance_ip(memory);
                let al = self.get_reg(Reg8::Al);
                let seg = segment_override.unwrap_or(SegmentReg::Ds);
                let seg_addr = self.get_segment_reg(seg);
                self.print_instruction(byte, &format_args!("mov [{}:{:04X}h], al", seg, mem_addr));
                write_byte(memory, PhysAddr::new(seg_addr, mem_addr), al)?;
            }
            0xA3 => {
                // MOV MEM16, AX
                let mem_addr = self.fetch_next_word_and_advance_ip(memory);
                let ax = self.get_reg(Reg16::Ax);
                let seg = segment_override.unwrap_or(SegmentReg::Ds);
                let seg_addr = self.get_segment_reg(seg);
                self.print_instruction(byte, &format_args!("mov [{}:{:04X}h], ax", seg, mem_addr));
                write_word(memory, SegmentedAddr::new(seg_addr, mem_addr), ax)?;
            }
            0xA4 => {
                // MOVSB
                self.print_instruction(byte, &format_args!("movsb"));
                // Repeat if REP prefix was set (actual hardware treats F2/F3 interchangeably)
                let reps = if rep_prefix.is_some() { self.cx } else { 1 };
                // TODO: Support interrupt/resume between repetitions
                for _ in 0..reps {
                    let seg = segment_override.unwrap_or(SegmentReg::Ds);
                    let value =
                        read_byte(memory, PhysAddr::new(self.get_segment_reg(seg), self.si))?;
                    write_byte(
                        memory,
                        PhysAddr::new(self.get_segment_reg(SegmentReg::Es), self.di),
                        value,
                    )?;
                    if self.get_flag(Flag::Direction) {
                        self.di = self.di.wrapping_sub(1);
                        self.si = self.si.wrapping_sub(1);
                    } else {
                        self.di = self.di.wrapping_add(1);
                        self.si = self.si.wrapping_add(1);
                    }
                    if rep_prefix.is_some() {
                        self.cx = self.cx.wrapping_sub(1);
                    }
                }
            }
            0xA5 => {
                // MOVSW
                self.print_instruction(byte, &format_args!("movsw"));
                // Repeat if REP prefix was set (actual hardware treats F2/F3 interchangeably)
                let reps = if rep_prefix.is_some() { self.cx } else { 1 };
                // TODO: Support interrupt/resume between repetitions
                for _ in 0..reps {
                    let seg = segment_override.unwrap_or(SegmentReg::Ds);
                    let value = read_word(
                        memory,
                        SegmentedAddr::new(self.get_segment_reg(seg), self.si),
                    )?;
                    write_word(
                        memory,
                        SegmentedAddr::new(self.get_segment_reg(SegmentReg::Es), self.di),
                        value,
                    )?;
                    if self.get_flag(Flag::Direction) {
                        self.di = self.di.wrapping_sub(2);
                        self.si = self.si.wrapping_sub(2);
                    } else {
                        self.di = self.di.wrapping_add(2);
                        self.si = self.si.wrapping_add(2);
                    }
                    if rep_prefix.is_some() {
                        self.cx = self.cx.wrapping_sub(1);
                    }
                }
            }
            0xA6 => {
                // CMPSB DEST-STR8, SRC-STR8
                self.print_instruction(byte, &format_args!("cmpsb"));
                let reps = if rep_prefix.is_some() { self.cx } else { 1 };
                for _ in 0..reps {
                    let source_seg = segment_override.unwrap_or(SegmentReg::Ds);
                    let source_addr = PhysAddr::new(self.get_segment_reg(source_seg), self.si);
                    let dest_addr = PhysAddr::new(self.get_segment_reg(SegmentReg::Es), self.di);
                    let source_byte = read_byte(memory, source_addr)?;
                    let dest_byte = read_byte(memory, dest_addr)?;
                    self.subtract8(source_byte, dest_byte, false);
                    if self.get_flag(Flag::Direction) {
                        self.si = self.si.wrapping_sub(1);
                        self.di = self.di.wrapping_sub(1);
                    } else {
                        self.si = self.si.wrapping_add(1);
                        self.di = self.di.wrapping_add(1);
                    }
                    if rep_prefix == Some(0xF3) {
                        // Compare until a mismatch or CX reaches zero
                        self.cx = self.cx.wrapping_sub(1);
                        if !self.get_flag(Flag::Zero) {
                            break;
                        }
                    } else if rep_prefix == Some(0xF2) {
                        // Compare until a match or CX reaches zero
                        self.cx = self.cx.wrapping_sub(1);
                        if self.get_flag(Flag::Zero) {
                            break;
                        }
                    }
                }
            }
            0xA7 => {
                // CMPSW DEST-STR16, SRC-STR16
                self.print_instruction(byte, &format_args!("cmpsw"));
                let reps = if rep_prefix.is_some() { self.cx } else { 1 };
                for _ in 0..reps {
                    let source_seg = segment_override.unwrap_or(SegmentReg::Ds);
                    let source_addr = SegmentedAddr::new(self.get_segment_reg(source_seg), self.si);
                    let dest_addr =
                        SegmentedAddr::new(self.get_segment_reg(SegmentReg::Es), self.di);
                    let source_word = read_word(memory, source_addr)?;
                    let dest_word = read_word(memory, dest_addr)?;
                    self.subtract16(source_word, dest_word, false);
                    if self.get_flag(Flag::Direction) {
                        self.si = self.si.wrapping_sub(2);
                        self.di = self.di.wrapping_sub(2);
                    } else {
                        self.si = self.si.wrapping_add(2);
                        self.di = self.di.wrapping_add(2);
                    }
                    if rep_prefix == Some(0xF3) {
                        // Compare until a mismatch or CX reaches zero
                        self.cx = self.cx.wrapping_sub(1);
                        if !self.get_flag(Flag::Zero) {
                            break;
                        }
                    } else if rep_prefix == Some(0xF2) {
                        // Compare until a match or CX reaches zero
                        self.cx = self.cx.wrapping_sub(1);
                        if self.get_flag(Flag::Zero) {
                            break;
                        }
                    }
                }
            }
            0xA8 => {
                // TEST AL, IMMED8
                let immediate = self.fetch_next_byte_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("test al, {:X}h", immediate));
                let reg_val = self.get_reg(Reg8::Al);
                let result = reg_val & immediate;
                self.update_logic_flags(result);
            }
            0xA9 => {
                // TEST AX, IMMED16
                let immediate = self.fetch_next_word_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("test ax, {:X}h", immediate));
                let reg_val = self.get_reg(Reg16::Ax);
                let result = reg_val & immediate;
                self.update_logic_flags(result);
            }
            0xAA => {
                // STOSB ("store string byte")
                self.print_instruction(byte, &format_args!("stosb"));
                // Documentation says only F3 works, but actual hardware allows F2
                let reps = if rep_prefix.is_some() { self.cx } else { 1 };
                for _ in 0..reps {
                    let seg_addr = self.get_segment_reg(SegmentReg::Es);
                    write_byte(
                        memory,
                        PhysAddr::new(seg_addr, self.di),
                        self.get_reg(Reg8::Al),
                    )?;
                    if self.get_flag(Flag::Direction) {
                        self.di = self.di.wrapping_sub(1);
                    } else {
                        self.di = self.di.wrapping_add(1);
                    }
                    if rep_prefix.is_some() {
                        self.cx = self.cx.wrapping_sub(1);
                    }
                }
            }
            0xAB => {
                // STOSW ("store string word")
                self.print_instruction(byte, &format_args!("stosw"));
                // Documentation says only F3 works, but actual hardware allows F2
                let reps = if rep_prefix.is_some() { self.cx } else { 1 };
                for _ in 0..reps {
                    let seg_addr = self.get_segment_reg(SegmentReg::Es);
                    write_word(memory, SegmentedAddr::new(seg_addr, self.di), self.ax)?;
                    if self.get_flag(Flag::Direction) {
                        self.di = self.di.wrapping_sub(2);
                    } else {
                        self.di = self.di.wrapping_add(2);
                    }
                    if rep_prefix.is_some() {
                        self.cx = self.cx.wrapping_sub(1);
                    }
                }
            }
            0xAC => {
                // LODSB
                self.print_instruction(byte, &format_args!("lodsb"));
                // Repeat if REP prefix was set (actual hardware supports F3/F2)
                let reps = if rep_prefix.is_some() { self.cx } else { 1 };
                // TODO: Support interrupt/resume between repetitions
                for _ in 0..reps {
                    let seg = segment_override.unwrap_or(SegmentReg::Ds);
                    let seg_addr = self.get_segment_reg(seg);
                    let byte = read_byte(memory, PhysAddr::new(seg_addr, self.si))?;
                    self.set_reg(Reg8::Al, byte);
                    if self.get_flag(Flag::Direction) {
                        self.si = self.si.wrapping_sub(1);
                    } else {
                        self.si = self.si.wrapping_add(1);
                    }
                    if rep_prefix.is_some() {
                        self.cx = self.cx.wrapping_sub(1);
                    }
                }
            }
            0xAD => {
                // LODSW
                self.print_instruction(byte, &format_args!("lodsw"));
                // Repeat if REP prefix was set
                let reps = if rep_prefix.is_some() { self.cx } else { 1 };
                // TODO: Support interrupt/resume between repetitions
                for _ in 0..reps {
                    let seg = segment_override.unwrap_or(SegmentReg::Ds);
                    let seg_addr = self.get_segment_reg(seg);
                    let word = read_word(memory, SegmentedAddr::new(seg_addr, self.si))?;
                    self.set_reg(Reg16::Ax, word);
                    if self.get_flag(Flag::Direction) {
                        self.si = self.si.wrapping_sub(2);
                    } else {
                        self.si = self.si.wrapping_add(2);
                    }
                    if rep_prefix.is_some() {
                        self.cx = self.cx.wrapping_sub(1);
                    }
                }
            }
            0xAE => {
                // SCASB: compare AL with [ES:DI], then advance DI
                self.print_instruction(byte, &format_args!("scasb"));
                let reps = if rep_prefix.is_some() { self.cx } else { 1 };
                for _ in 0..reps {
                    let al = self.get_reg(Reg8::Al);
                    let esdi_addr = PhysAddr::new(self.get_segment_reg(SegmentReg::Es), self.di);
                    let esdi_byte = read_byte(memory, esdi_addr)?;
                    self.subtract8(al, esdi_byte, false);
                    if self.get_flag(Flag::Direction) {
                        self.di = self.di.wrapping_sub(1);
                    } else {
                        self.di = self.di.wrapping_add(1);
                    }
                    if rep_prefix == Some(0xF3) {
                        // Compare until a mismatch or CX reaches zero
                        self.cx = self.cx.wrapping_sub(1);
                        if !self.get_flag(Flag::Zero) {
                            break;
                        }
                    } else if rep_prefix == Some(0xF2) {
                        // Compare until a match or CX reaches zero
                        self.cx = self.cx.wrapping_sub(1);
                        if self.get_flag(Flag::Zero) {
                            break;
                        }
                    }
                }
            }
            0xAF => {
                // SCASW: compare AX with [ES:DI], then advance DI
                self.print_instruction(byte, &format_args!("scasw"));
                let reps = if rep_prefix.is_some() { self.cx } else { 1 };
                for _ in 0..reps {
                    let ax = self.get_reg(Reg16::Ax);
                    let esdi_addr =
                        SegmentedAddr::new(self.get_segment_reg(SegmentReg::Es), self.di);
                    let esdi_word = read_word(memory, esdi_addr)?;
                    self.subtract16(ax, esdi_word, false);
                    if self.get_flag(Flag::Direction) {
                        self.di = self.di.wrapping_sub(2);
                    } else {
                        self.di = self.di.wrapping_add(2);
                    }
                    if rep_prefix == Some(0xF3) {
                        // Compare until a mismatch or CX reaches zero
                        self.cx = self.cx.wrapping_sub(1);
                        if !self.get_flag(Flag::Zero) {
                            break;
                        }
                    } else if rep_prefix == Some(0xF2) {
                        // Compare until a match or CX reaches zero
                        self.cx = self.cx.wrapping_sub(1);
                        if self.get_flag(Flag::Zero) {
                            break;
                        }
                    }
                }
            }
            // B0-B7: move an 8-bit immediate into an 8-bit register
            0xB0..=0xB7 => {
                let reg = Reg8::try_from(byte & 7)?;
                let immediate = self.fetch_next_byte_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("mov {}, {:X}h", reg, immediate));
                self.set_reg(reg, immediate);
            }
            // B8-BF: move a 16-bit immediate into a 16-bit register
            0xB8..=0xBF => {
                let reg = Reg16::try_from(byte & 7)?;
                let immediate = self.fetch_next_word_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("mov {}, {:X}h", reg, immediate));
                self.set_reg(reg, immediate);
            }
            0xC0 if self.model == CpuModel::I80286 => {
                // 286: Shift/rotate an 8-bit operand by an immediate count
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let rm = self.decode_rm_operand::<Reg8>(memory, &parsed, segment_override)?;
                let op = ShiftOp::try_from(parsed.reg_field)?;
                let immediate = self.fetch_next_byte_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("{} {}, {}", op, rm, immediate));

                let operand = self.read_operand(memory, &rm)?;
                let count = self.effective_shift_count(immediate);
                if count != 0 {
                    let result = self.shift_op8(op, operand, count);
                    self.write_operand(memory, &rm, result)?;
                }
            }
            0xC1 if self.model == CpuModel::I80286 => {
                // 286: Shift/rotate a 16-bit operand by an immediate count
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let rm = self.decode_rm_operand::<Reg16>(memory, &parsed, segment_override)?;
                let op = ShiftOp::try_from(parsed.reg_field)?;
                let immediate = self.fetch_next_byte_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("{} {}, {}", op, rm, immediate));

                let operand = self.read_operand(memory, &rm)?;
                let count = self.effective_shift_count(immediate);
                if count != 0 {
                    let result = self.shift_op16(op, operand, count);
                    self.write_operand(memory, &rm, result)?;
                }
            }
            0xC2 | 0xC0 if byte == 0xC2 || self.model == CpuModel::I8088 => {
                // RET IMMED16 (intrasegment)
                let immediate = self.fetch_next_word_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("ret {:X}h", immediate));
                self.ip = self.pop_from_stack(memory)?;
                self.sp = self.sp.wrapping_add(immediate);
            }
            0xC3 | 0xC1 if byte == 0xC3 || self.model == CpuModel::I8088 => {
                // RET
                self.print_instruction(byte, &format_args!("ret"));
                self.ip = self.pop_from_stack(memory)?;
            }
            0xC4 => {
                // LES: Load a far pointer into ES and a reg16
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let rm = self.decode_rm_operand::<Reg16>(memory, &parsed, segment_override)?;
                let RegisterOrMemoryOperand::Memory(seg_reg, mem_addr) = rm else {
                    return Err(CpuError::UnsupportedRmField(parsed.rm_field));
                };
                let dest_reg = Reg16::try_from(parsed.reg_field)?;
                self.print_instruction(byte, &format_args!("les {}, {}", dest_reg, rm));
                let seg_addr = self.get_segment_reg(seg_reg);
                let pointer = read_far_pointer(memory, SegmentedAddr::new(seg_addr, mem_addr))?;
                self.set_reg(dest_reg, pointer.offset);
                self.es = pointer.segment;
            }
            0xC5 => {
                // LDS: Load a far pointer into DS and a reg16
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let rm = self.decode_rm_operand::<Reg16>(memory, &parsed, segment_override)?;
                let RegisterOrMemoryOperand::Memory(seg_reg, mem_addr) = rm else {
                    return Err(CpuError::UnsupportedRmField(parsed.rm_field));
                };
                let dest_reg = Reg16::try_from(parsed.reg_field)?;
                self.print_instruction(byte, &format_args!("lds {}, {}", dest_reg, rm));
                let seg_addr = self.get_segment_reg(seg_reg);
                let pointer = read_far_pointer(memory, SegmentedAddr::new(seg_addr, mem_addr))?;
                self.set_reg(dest_reg, pointer.offset);
                self.ds = pointer.segment;
            }
            // MOV r/m8, imm8
            0xC6 => {
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                if parsed.reg_field != 0 {
                    // The register value is required to be 0
                    return Err(CpuError::UnsupportedRegisterField(byte, parsed.reg_field));
                }
                let rm = self.decode_rm_operand::<Reg8>(memory, &parsed, segment_override)?;
                let immediate = self.fetch_next_byte_and_advance_ip(memory);

                self.print_instruction(byte, &format_args!("mov {}, {}", rm, immediate));
                self.write_operand(memory, &rm, immediate)?;
            }
            // MOV r/m16, imm16
            0xC7 => {
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                if parsed.reg_field != 0 {
                    // The register value is required to be 0
                    return Err(CpuError::UnsupportedRegisterField(byte, parsed.reg_field));
                }
                let rm = self.decode_rm_operand::<Reg16>(memory, &parsed, segment_override)?;
                let immediate = self.fetch_next_word_and_advance_ip(memory);

                self.print_instruction(byte, &format_args!("mov {}, {:X}h", rm, immediate));
                self.write_operand(memory, &rm, immediate)?;
            }
            0xC8 if self.model == CpuModel::I80286 => {
                // ENTER imm16, imm8
                let dyn_storage_size = self.fetch_next_word_and_advance_ip(memory);
                let nesting_level = self.fetch_next_byte_and_advance_ip(memory) & 0x1F;
                self.print_instruction(
                    byte,
                    &format_args!("enter {}, {}", dyn_storage_size, nesting_level),
                );

                self.push_onto_stack(memory, self.bp)?;
                let frame_ptr = self.sp;

                if nesting_level > 0 {
                    for _ in 1..nesting_level {
                        self.bp = self.bp.wrapping_sub(2);
                        let pointer = read_word(memory, SegmentedAddr::new(self.ss, self.bp))?;
                        self.push_onto_stack(memory, pointer)?;
                    }
                    self.push_onto_stack(memory, frame_ptr)?;
                }

                self.bp = frame_ptr;
                self.sp = self.sp.wrapping_sub(dyn_storage_size);
            }
            0xC9 if self.model == CpuModel::I80286 => {
                // LEAVE
                self.print_instruction(byte, &format_args!("leave"));
                self.sp = self.bp;
                self.bp = self.pop_from_stack(memory)?;
            }
            0xC8 | 0xCA if byte == 0xCA || self.model == CpuModel::I8088 => {
                // Far return (pops IP *and* CS) + immed16
                let immediate = self.fetch_next_word_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("retf {:X}h", immediate));
                let ip = self.pop_from_stack(memory)?;
                let cs = self.pop_from_stack(memory)?;
                self.ip = ip;
                self.cs = cs;
                self.sp = self.sp.wrapping_add(immediate);
            }
            0xC9 | 0xCB if byte == 0xCB || self.model == CpuModel::I8088 => {
                // Far return (pops IP *and* CS)
                self.print_instruction(byte, &format_args!("retf"));
                let ip = self.pop_from_stack(memory)?;
                let cs = self.pop_from_stack(memory)?;
                self.ip = ip;
                self.cs = cs;
            }
            0xCC => {
                // INT3
                self.print_instruction(byte, &format_args!("int3"));
                result = CpuStepResult::Interrupt(0x3);
            }
            0xCD => {
                // INT
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("int {:X}h", operand));
                result = CpuStepResult::Interrupt(operand);
            }
            0xCE => {
                // INTO: Interrupt on Overflow
                self.print_instruction(byte, &format_args!("into"));
                if self.get_flag(Flag::Overflow) {
                    result = CpuStepResult::Interrupt(4);
                }
            }
            0xCF => {
                // IRET
                self.print_instruction(byte, &format_args!("iret"));
                self.ip = self.pop_from_stack(memory)?;
                self.cs = self.pop_from_stack(memory)?;
                let flags = self.pop_from_stack(memory)?;
                self.flags = self.normalize_flags(flags);
            }
            0xD0 | 0xD2 => {
                // 0xD0: Shift ops with R/M8, 1
                // 0xD2: Shift ops with R/M8, CL
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let rm = self.decode_rm_operand::<Reg8>(memory, &parsed, segment_override)?;
                let op = ShiftOp::try_from(parsed.reg_field)?;
                self.print_instruction(
                    byte,
                    &format_args!("{} {}, {}", op, rm, if byte == 0xD2 { "cl" } else { "1" }),
                );

                let operand = self.read_operand(memory, &rm)?;
                let count = self.effective_shift_count(match byte {
                    0xD2 => self.get_reg(Reg8::Cl),
                    _ => 1,
                });
                if count != 0 {
                    let result = self.shift_op8(op, operand, count);
                    self.write_operand(memory, &rm, result)?;
                }
            }
            0xD1 | 0xD3 => {
                // 0xD1: Shift ops with R/M16, 1
                // 0xD3: Shift ops with R/M16, CL
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let rm = self.decode_rm_operand::<Reg16>(memory, &parsed, segment_override)?;
                let op = ShiftOp::try_from(parsed.reg_field)?;
                self.print_instruction(
                    byte,
                    &format_args!("{} {}, {}", op, rm, if byte == 0xD3 { "cl" } else { "1" }),
                );

                let operand = self.read_operand(memory, &rm)?;
                let count = self.effective_shift_count(match byte {
                    0xD3 => self.get_reg(Reg8::Cl),
                    _ => 1,
                });
                if count != 0 {
                    let result = self.shift_op16(op, operand, count);
                    self.write_operand(memory, &rm, result)?;
                }
            }
            0xD4 => {
                // AAM: ASCII Adjust after Multiply
                let base = self.fetch_next_byte_and_advance_ip(memory);
                if base == 0 {
                    return Ok(CpuStepResult::Interrupt(0x0));
                }
                self.print_instruction(byte, &format_args!("aam {:X}h", base));
                let al = self.get_reg(Reg8::Al);
                let remainder = al % base;
                self.set_reg(Reg8::Ah, al / base);
                self.set_reg(Reg8::Al, remainder);
                self.set_result_flags(remainder);
            }
            0xD5 => {
                // AAD: ASCII Adjust AX before Division
                let base = self.fetch_next_byte_and_advance_ip(memory) as u16;
                self.print_instruction(byte, &format_args!("aad {:X}h", base));
                let al = self.get_reg(Reg8::Al) as u16;
                let ah = self.get_reg(Reg8::Ah) as u16;
                let new_al = (al + ah * base) as u8;
                self.set_reg(Reg8::Ah, 0);
                self.set_reg(Reg8::Al, new_al);
                self.set_result_flags(new_al);
            }
            0xD7 => {
                // XLAT SOURCE-TABLE
                self.print_instruction(byte, &format_args!("xlat"));
                let offset = self.bx.wrapping_add(self.get_reg(Reg8::Al) as u16);
                let seg_addr = self.get_segment_reg(segment_override.unwrap_or(SegmentReg::Ds));
                let result = read_byte(memory, PhysAddr::new(seg_addr, offset))?;
                self.set_reg(Reg8::Al, result);
            }
            0xD8..=0xDF => {
                // ESC: 8087 instruction
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let rm = self.decode_rm_operand::<Reg16>(memory, &parsed, segment_override)?;
                if let RegisterOrMemoryOperand::Memory(_, _) = rm {
                    // Perform dummy read of the word at that location
                    self.read_operand(memory, &rm)?;
                }
            }
            0xE0 => {
                // LOOPNE rel8
                let displacement = self.fetch_next_byte_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("loopne {:X}h", displacement));
                self.cx = self.cx.wrapping_sub(1);
                if self.cx != 0 && !self.get_flag(Flag::Zero) {
                    self.ip = self.ip.wrapping_add_signed((displacement as i8) as i16);
                }
            }
            0xE1 => {
                // LOOPE rel8
                let displacement = self.fetch_next_byte_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("loope {:X}h", displacement));
                self.cx = self.cx.wrapping_sub(1);
                if self.cx != 0 && self.get_flag(Flag::Zero) {
                    self.ip = self.ip.wrapping_add_signed((displacement as i8) as i16);
                }
            }
            0xE2 => {
                // LOOP rel8
                let displacement = self.fetch_next_byte_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("loop {:X}h", displacement));
                self.cx = self.cx.wrapping_sub(1);
                if self.cx != 0 {
                    self.ip = self.ip.wrapping_add_signed((displacement as i8) as i16);
                }
            }
            0xE3 => {
                // JCXZ: Jump if CX is zero
                let displacement = self.fetch_next_byte_and_advance_ip(memory) as i8;
                self.print_instruction(byte, &format_args!("jcxz {:X}h", displacement));
                if self.cx == 0 {
                    self.ip = self.ip.wrapping_add_signed(displacement as i16);
                }
            }
            0xE4 => {
                // IN AL, imm8
                let port = self.fetch_next_byte_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("in al, {:X}h", port));
                return Ok(CpuStepResult::PortRead8 { port: port as u16 });
            }
            0xE5 => {
                // IN AX, imm8
                let port = self.fetch_next_byte_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("in ax, {:X}h", port));
                return Ok(CpuStepResult::PortRead16 { port: port as u16 });
            }
            0xE6 => {
                // OUT imm8, AL
                let port = self.fetch_next_byte_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("out {:X}h, al", port));
                return Ok(CpuStepResult::PortWrite8 {
                    port: port as u16,
                    value: self.get_reg(Reg8::Al),
                });
            }
            0xE7 => {
                // OUT imm8, AX
                let port = self.fetch_next_byte_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("out {:X}h, ax", port));
                return Ok(CpuStepResult::PortWrite16 {
                    port: port as u16,
                    value: self.get_reg(Reg16::Ax),
                });
            }
            // CALL
            0xE8 => {
                // Fetch 16-bit relative displacement
                let displacement = self.fetch_next_word_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("call {:X}h", displacement));
                // Push the current IP - it now points after the entire CALL instruction
                self.push_onto_stack(memory, self.ip)?;
                // Add the displacement to IP with 16-bit wrapping
                self.ip = self.ip.wrapping_add(displacement)
            }
            0xE9 => {
                // near relative JMP
                let displacement = self.fetch_next_word_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("jmp {:X}h", displacement));
                // Add the displacement to IP with 16-bit wrapping
                self.ip = self.ip.wrapping_add(displacement)
            }
            0xEA => {
                // JMP FAR-LABEL
                let offset = self.fetch_next_word_and_advance_ip(memory);
                let segment = self.fetch_next_word_and_advance_ip(memory);
                self.print_instruction(byte, &format_args!("jmp {:04X}h:{:04X}h", segment, offset));
                self.ip = offset;
                self.cs = segment;
            }
            // JMP
            0xEB => {
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                self.print_instruction(byte, &format_args!("jmp {:X}h", operand));
                self.ip = self.ip.wrapping_add_signed(operand as i16);
            }
            0xEC => {
                // IN AL, DX
                self.print_instruction(byte, &format_args!("in al, dx"));
                let port = self.dx;
                return Ok(CpuStepResult::PortRead8 { port: port });
            }
            0xED => {
                // IN AX, DX
                self.print_instruction(byte, &format_args!("in ax, dx"));
                let port = self.dx;
                return Ok(CpuStepResult::PortRead16 { port: port });
            }
            0xEE => {
                // OUT DX, AL
                self.print_instruction(byte, &format_args!("out dx, al"));
                let port = self.dx;
                return Ok(CpuStepResult::PortWrite8 {
                    port: port,
                    value: self.get_reg(Reg8::Al),
                });
            }
            0xEF => {
                // OUT DX, AX
                self.print_instruction(byte, &format_args!("out dx, ax"));
                return Ok(CpuStepResult::PortWrite16 {
                    port: self.dx,
                    value: self.ax,
                });
            }
            0xF4 => {
                // HLT
                self.halted = true;
                return Ok(CpuStepResult::Continue);
            }
            0xF6 => {
                // Does many things with REG8/MEM8(, IMMED8)
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let rm = self.decode_rm_operand::<Reg8>(memory, &parsed, segment_override)?;
                let operand = self.read_operand(memory, &rm)?;
                match parsed.reg_field {
                    0b000 | 0b001 => {
                        // TEST R/M8, IMMED8
                        let immediate = self.fetch_next_byte_and_advance_ip(memory);
                        self.print_instruction(
                            byte,
                            &format_args!("test {}, {:X}h", rm, immediate),
                        );
                        let result = operand & immediate;
                        self.update_logic_flags(result);
                    }
                    0b010 => {
                        // NOT r/m8
                        self.print_instruction(byte, &format_args!("not {}", rm));
                        let result = !operand;
                        self.write_operand(memory, &rm, result)?;
                    }
                    0b011 => {
                        // NEG r/m8
                        self.print_instruction(byte, &format_args!("neg {}", rm));
                        let result = self.subtract8(0, operand, false);
                        self.write_operand(memory, &rm, result)?;
                    }
                    0b100 => {
                        // MUL R/M8 (AX := AL * r/m8).
                        self.print_instruction(byte, &format_args!("mul {}", rm));
                        let al = self.get_reg(Reg8::Al);
                        let value = (al as u16) * (operand as u16);
                        self.ax = value;
                        let upper_half_non_zero = value >> 8 != 0;
                        self.set_flag(Flag::Carry, upper_half_non_zero);
                        self.set_flag(Flag::Overflow, upper_half_non_zero);
                    }
                    0b101 => {
                        // IMUL r/m 8
                        self.print_instruction(byte, &format_args!("imul {}", rm));
                        let result = (self.ax as i8 as i16) * (operand as i8 as i16);
                        self.set_reg(Reg8::Al, result as u8);
                        self.set_reg(Reg8::Ah, (result >> 8) as u8);
                        let upper_half_non_zero = !(-128..=127).contains(&result);
                        self.set_flag(Flag::Carry, upper_half_non_zero);
                        self.set_flag(Flag::Overflow, upper_half_non_zero);
                    }
                    0b110 => {
                        // DIV R/M8 (divide AX by r/m8, AL := quotient, AH := remainder)
                        self.print_instruction(byte, &format_args!("div {}", rm));
                        let ax = self.get_reg(Reg16::Ax);
                        let operand = operand as u16;
                        if operand == 0 {
                            // Trigger interrupt 0
                            return Ok(CpuStepResult::Interrupt(0));
                        }
                        let quotient = ax / operand;
                        let remainder = ax % operand;
                        if quotient > 0xFF {
                            // Trigger interrupt 0
                            return Ok(CpuStepResult::Interrupt(0));
                        }
                        self.set_reg(Reg8::Al, quotient as u8);
                        self.set_reg(Reg8::Ah, remainder as u8);
                    }
                    0b111 => {
                        // IDIV r/m8
                        self.print_instruction(byte, &format_args!("idiv {}", rm));
                        let dividend = self.ax as i16 as i32;
                        let divisor = operand as i8 as i32;
                        if divisor == 0 {
                            // Trigger interrupt 0
                            return Ok(CpuStepResult::Interrupt(0));
                        }
                        let quotient = dividend / divisor;
                        let remainder = dividend % divisor;
                        // 8086/8088 quirk
                        if !(-127..=127).contains(&quotient) {
                            // Trigger interrupt 0
                            return Ok(CpuStepResult::Interrupt(0));
                        }
                        self.set_reg(Reg8::Al, quotient as u8);
                        self.set_reg(Reg8::Ah, remainder as u8);
                    }
                    _ => return Err(CpuError::UnsupportedRegisterField(byte, parsed.reg_field)),
                };
            }
            0xF7 => {
                // Does a lot of things! on REG16/MEM16 (and on IMMED16 for TEST only)
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let rm = self.decode_rm_operand::<Reg16>(memory, &parsed, segment_override)?;
                let operand = self.read_operand(memory, &rm)?;
                match parsed.reg_field {
                    0b000 | 0b001 => {
                        // TEST r/m 16, imm16
                        let immediate = self.fetch_next_word_and_advance_ip(memory);
                        self.print_instruction(
                            byte,
                            &format_args!("test {}, {:X}h", rm, immediate),
                        );
                        let result = operand & immediate;
                        self.update_logic_flags(result);
                    }
                    0b010 => {
                        // NOT r/m16
                        self.print_instruction(byte, &format_args!("not {}", rm));
                        let result = !operand;
                        self.write_operand(memory, &rm, result)?;
                    }
                    0b011 => {
                        // NEG r/m 16
                        self.print_instruction(byte, &format_args!("neg {}", rm));
                        let result = self.subtract16(0, operand, false);
                        self.write_operand(memory, &rm, result)?;
                    }
                    0b100 => {
                        // MUL r/m 16
                        self.print_instruction(byte, &format_args!("mul {}", rm));
                        let ax = self.get_reg(Reg16::Ax);
                        let value = (ax as u32) * (operand as u32);
                        self.dx = (value >> 16) as u16; // dx has the upper half
                        self.ax = (value & 0xFFFF) as u16; // ax holds the lower half
                        let upper_half_non_zero = self.dx != 0;
                        self.set_flag(Flag::Carry, upper_half_non_zero);
                        self.set_flag(Flag::Overflow, upper_half_non_zero);
                    }
                    0b101 => {
                        // IMUL r/m 16
                        self.print_instruction(byte, &format_args!("imul {}", rm));
                        let result = (self.ax as i16 as i32) * (operand as i16 as i32);
                        self.ax = result as u16;
                        self.dx = (result >> 16) as u16;
                        let overflow = !(-32768..=32767).contains(&result);
                        self.set_flag(Flag::Carry, overflow);
                        self.set_flag(Flag::Overflow, overflow);
                    }
                    0b110 => {
                        // DIV r/m 16
                        self.print_instruction(byte, &format_args!("div {}", rm));
                        let dx = self.get_reg(Reg16::Dx);
                        let ax = self.get_reg(Reg16::Ax);
                        let lhs = ((dx as u32) << 16) + (ax as u32);
                        if operand == 0 {
                            // Trigger interrupt 0
                            return Ok(CpuStepResult::Interrupt(0));
                        }
                        let quotient = lhs / operand as u32;
                        if quotient > 0xFFFF {
                            // Trigger interrupt 0
                            return Ok(CpuStepResult::Interrupt(0));
                        }
                        let remainder = lhs % operand as u32;
                        self.ax = quotient as u16;
                        self.dx = remainder as u16;
                    }
                    0b111 => {
                        // IDIV r/m 16
                        self.print_instruction(byte, &format_args!("idiv {}", rm));
                        let dx = self.get_reg(Reg16::Dx);
                        let ax = self.get_reg(Reg16::Ax);
                        let dividend = (((dx as u32) << 16) | ax as u32) as i32 as i64;
                        let divisor = operand as i16 as i64;
                        if divisor == 0 {
                            // Trigger interrupt 0
                            return Ok(CpuStepResult::Interrupt(0));
                        }
                        let quotient = dividend / divisor;
                        // 8086 quirk
                        if !(-32767..=32767).contains(&quotient) {
                            return Ok(CpuStepResult::Interrupt(0));
                        }
                        let remainder = dividend % divisor;
                        self.ax = quotient as u16;
                        self.dx = remainder as u16;
                    }
                    _ => return Err(CpuError::UnsupportedRegisterField(byte, parsed.reg_field)),
                }
            }
            // Flag operations
            0xF5 => {
                // CMC
                self.print_instruction(byte, &format_args!("cmc"));
                self.set_flag(Flag::Carry, !self.get_flag(Flag::Carry));
            }
            0xF8 => {
                // CLC
                self.print_instruction(byte, &format_args!("clc"));
                self.set_flag(Flag::Carry, false);
            }
            0xF9 => {
                // STC
                self.print_instruction(byte, &format_args!("stc"));
                self.set_flag(Flag::Carry, true);
            }
            0xFA => {
                // CLI
                self.print_instruction(byte, &format_args!("cli"));
                self.set_flag(Flag::InterruptEnable, false);
            }
            0xFB => {
                // STI
                // TODO: When hardware IRQs are implemented, defer maskable interrupt delivery
                // until after the next instruction.
                self.print_instruction(byte, &format_args!("sti"));
                self.set_flag(Flag::InterruptEnable, true);
            }
            0xFC => {
                // CLD
                self.print_instruction(byte, &format_args!("cld"));
                self.set_flag(Flag::Direction, false);
            }
            0xFD => {
                // STD
                self.print_instruction(byte, &format_args!("std"));
                self.set_flag(Flag::Direction, true);
            }
            // INC r/m8 or DEC r/m8
            0xFE => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let is_inc = match parsed.reg_field {
                    0 => true,
                    1 => false,
                    _ => return Err(CpuError::UnsupportedRegisterField(byte, parsed.reg_field)),
                };
                let rm = self.decode_rm_operand::<Reg8>(memory, &parsed, segment_override)?;
                self.print_instruction(
                    byte,
                    &format_args!("{} {}", if is_inc { "inc" } else { "dec" }, rm),
                );
                let old_value = self.read_operand(memory, &rm)?;
                let new_value = (old_value as i8).wrapping_add(if is_inc { 1 } else { -1 }) as u8;
                self.write_operand(memory, &rm, new_value)?;

                self.set_result_flags(new_value);
                if is_inc {
                    // Signed overflow: 0x7F became 0x80, meaning +127 became -128
                    self.set_flag(Flag::Overflow, old_value == 0x7F);
                    // Was original low nibble 0b1111
                    self.set_flag(Flag::AuxiliaryCarry, old_value & 0xF == 0xF);
                } else {
                    // decrement
                    // Overflow: 0x80 became 0x7F, meaning -128 became +127
                    self.set_flag(Flag::Overflow, old_value == 0x80);
                    // Did decrementing borrowing into lower nibble (was it 0)
                    self.set_flag(Flag::AuxiliaryCarry, old_value & 0x0F == 0);
                }
            }
            0xFF => {
                // Does a lot of things! on MEM16 and R/M16
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let rm = self.decode_rm_operand::<Reg16>(memory, &parsed, segment_override)?;
                match parsed.reg_field {
                    0b000 => {
                        // INC r/m 16
                        self.print_instruction(byte, &format_args!("inc {}", rm));
                        let carry = self.get_flag(Flag::Carry);
                        let operand = self.read_operand(memory, &rm)?;
                        let new_value = self.add16(operand, 1, false);
                        // Must preserve carry
                        self.set_flag(Flag::Carry, carry);
                        self.write_operand(memory, &rm, new_value)?;
                    }
                    0b001 => {
                        // DEC r/m 16
                        self.print_instruction(byte, &format_args!("dec {}", rm));
                        let carry = self.get_flag(Flag::Carry);
                        let operand = self.read_operand(memory, &rm)?;
                        let new_value = self.subtract16(operand, 1, false);
                        // Must preserve carry
                        self.set_flag(Flag::Carry, carry);
                        self.write_operand(memory, &rm, new_value)?;
                    }
                    0b010 => {
                        // CALL R/M16 (intrasegment)
                        self.print_instruction(byte, &format_args!("call {}", rm));
                        let target_ip = self.read_operand(memory, &rm)?;
                        self.push_onto_stack(memory, self.ip)?;
                        self.ip = target_ip;
                    }
                    0b011 => {
                        // CALL MEM16 (intersegment)
                        self.print_instruction(byte, &format_args!("call {}", rm));
                        let RegisterOrMemoryOperand::Memory(seg_reg, mem_addr) = rm else {
                            return Err(CpuError::UnsupportedRmField(parsed.rm_field));
                        };
                        let seg_addr = self.get_segment_reg(seg_reg);
                        let pointer =
                            read_far_pointer(memory, SegmentedAddr::new(seg_addr, mem_addr))?;
                        self.push_onto_stack(memory, self.cs)?;
                        self.push_onto_stack(memory, self.ip)?;
                        self.cs = pointer.segment;
                        self.ip = pointer.offset;
                    }
                    0b100 => {
                        // JMP R/M16 (intra)
                        self.print_instruction(byte, &format_args!("jmp {}", rm));
                        self.ip = self.read_operand(memory, &rm)?;
                    }
                    0b101 => {
                        // JMP MEM16 (intersegment)
                        let RegisterOrMemoryOperand::Memory(seg_reg, mem_addr) = rm else {
                            return Err(CpuError::UnsupportedRmField(parsed.rm_field));
                        };
                        let seg_addr = self.get_segment_reg(seg_reg);
                        let pointer =
                            read_far_pointer(memory, SegmentedAddr::new(seg_addr, mem_addr))?;
                        let target_ip = pointer.offset;
                        let target_cs = pointer.segment;
                        self.print_instruction(
                            byte,
                            &format_args!("jmp far {:04X}h:{:04X}h", target_cs, target_ip),
                        );
                        self.ip = target_ip;
                        self.cs = target_cs;
                    }
                    0b110 | 0b111 => {
                        // PUSH r/m16
                        self.print_instruction(byte, &format_args!("push {}", rm));
                        let value = match rm {
                            RegisterOrMemoryOperand::Register(Reg16::Sp)
                                if self.model == CpuModel::I8088 =>
                            {
                                // 8086/8088 hardware quirk for PUSH SP
                                self.sp.wrapping_sub(2)
                            }
                            _ => self.read_operand(memory, &rm)?,
                        };
                        self.push_onto_stack(memory, value)?;
                    }
                    _ => return Err(CpuError::UnsupportedRegisterField(byte, parsed.reg_field)),
                };
            }
            _ => return Err(CpuError::UnsupportedInstruction(byte)),
        }
        Ok(result)
    }

    pub(crate) fn enter_interrupt(
        &mut self,
        memory: &mut [u8],
        interrupt: u8,
    ) -> Result<(), CpuError> {
        // Push the original FLAGS
        self.push_onto_stack(memory, self.flags)?;
        // Clear IF and TF
        self.set_flag(Flag::InterruptEnable, false);
        self.set_flag(Flag::Trap, false);
        // Push CS
        self.push_onto_stack(memory, self.cs)?;
        // Push the return IP
        self.push_onto_stack(memory, self.ip)?;
        // Load IP and CS from the interrupt vector table
        let ip_vec_offset = (interrupt as u16) * 4;
        let cs_vec_offset = (interrupt as u16) * 4 + 2;
        self.ip = read_word(memory, SegmentedAddr::new(0, ip_vec_offset))?;
        self.cs = read_word(memory, SegmentedAddr::new(0, cs_vec_offset))?;

        if self.ip == 0 && self.cs == 0 && self.verbosity > 0 {
            eprintln!("Warning: INT {interrupt:02X}h vector points to 0000:0000");
        }

        self.halted = false;

        Ok(())
    }

    fn set_result_flags<T: CpuValue>(&mut self, result: T) {
        self.set_flag(Flag::Zero, result.is_zero());
        self.set_flag(Flag::Sign, result.is_negative());
        self.set_flag(Flag::Parity, result.low_byte().count_ones() % 2 == 0);
    }

    fn push_onto_stack(&mut self, memory: &mut [u8], word: u16) -> Result<(), CpuError> {
        self.sp = self.sp.wrapping_sub(2);
        Ok(write_word(
            memory,
            SegmentedAddr::new(self.ss, self.sp),
            word,
        )?)
    }

    fn pop_from_stack(&mut self, memory: &[u8]) -> Result<u16, CpuError> {
        let value = read_word(memory, SegmentedAddr::new(self.ss, self.sp))?;
        self.sp = self.sp.wrapping_add(2);
        Ok(value)
    }

    fn fetch_next_byte_and_advance_ip(&mut self, memory: &[u8]) -> u8 {
        let phys_addr = SegmentedAddr::new(self.cs, self.ip);
        let byte = memory[phys_addr.as_usize()];
        self.ip = self.ip.wrapping_add(1);
        byte
    }

    fn fetch_next_word_and_advance_ip(&mut self, memory: &[u8]) -> u16 {
        // The next byte is the low-order byte (little endian)
        let lob = self.fetch_next_byte_and_advance_ip(memory);
        // The next byte is the high-order byte
        let hob = self.fetch_next_byte_and_advance_ip(memory);

        u16::from_le_bytes([lob, hob])
    }

    /// Implements shared functionality for subtracting an 8-bit rhs
    /// from an 8-bit lhs and setting the correct flags.
    fn subtract8(&mut self, lhs: u8, rhs: u8, borrow_in: bool) -> u8 {
        let new_rhs = rhs as u16 + borrow_in as u16;
        let wide_lhs = lhs as u16;
        let result = wide_lhs.wrapping_sub(new_rhs) as u8;

        // Carry flag: did subtraction require a borrow?
        self.set_flag(Flag::Carry, wide_lhs < new_rhs);
        // Overflow: (1) old_reg and operand have different signs, AND
        // (2) the result's sign differs from old_reg's sign
        self.set_flag(
            Flag::Overflow,
            ((lhs & 0x80) != (rhs & 0x80)) && ((result & 0x80) != (lhs & 0x80)),
        );
        self.set_flag(
            Flag::AuxiliaryCarry,
            (wide_lhs & 0x0F) < ((rhs & 0x0F) + borrow_in as u8) as u16,
        );
        self.set_result_flags(result);

        result
    }

    fn subtract16(&mut self, lhs: u16, rhs: u16, borrow_in: bool) -> u16 {
        let new_rhs = rhs as u32 + borrow_in as u32;
        let wide_lhs = lhs as u32;
        let result = wide_lhs.wrapping_sub(new_rhs) as u16;

        // Carry flag: did subtraction require a borrow?
        self.set_flag(Flag::Carry, wide_lhs < new_rhs);
        // Overflow: (1) old_reg and operand have different signs, AND
        // (2) the result's sign differs from old_reg's sign
        self.set_flag(
            Flag::Overflow,
            ((lhs & 0x8000) != (rhs & 0x8000)) && ((result & 0x8000) != (lhs & 0x8000)),
        );
        self.set_flag(
            Flag::AuxiliaryCarry,
            (wide_lhs & 0x0F) < ((rhs & 0x0F) + borrow_in as u16) as u32,
        );
        self.set_result_flags(result);

        result
    }

    fn add8(&mut self, lhs: u8, rhs: u8, carry_in: bool) -> u8 {
        let big_result = lhs as u16 + rhs as u16 + carry_in as u16;
        let result = big_result as u8;

        self.set_flag(Flag::Carry, big_result > 0xFF);
        // Overflow: operands have the same signs AND the result's sign differs
        self.set_flag(
            Flag::Overflow,
            ((lhs & 0x80) == (rhs & 0x80)) && ((result & 0x80) != (lhs & 0x80)),
        );
        self.set_flag(
            Flag::AuxiliaryCarry,
            (lhs & 0x0F) + (rhs & 0x0F) + carry_in as u8 > 0x0F,
        );
        self.set_result_flags(result);

        result
    }

    fn add16(&mut self, lhs: u16, rhs: u16, carry_in: bool) -> u16 {
        let big_result = lhs as u32 + rhs as u32 + carry_in as u32;
        let result = big_result as u16;

        self.set_flag(Flag::Carry, big_result > 0xFFFF);
        // Overflow: operands have the same signs AND the result's sign differs
        self.set_flag(
            Flag::Overflow,
            ((lhs & 0x8000) == (rhs & 0x8000)) && ((result & 0x8000) != (lhs & 0x8000)),
        );
        self.set_flag(
            Flag::AuxiliaryCarry,
            (lhs & 0x0F) + (rhs & 0x0F) + carry_in as u16 > 0x0F,
        );
        self.set_result_flags(result);

        result
    }

    fn inc16(&mut self, instruction: u8, reg: Reg16) {
        self.print_instruction(instruction, &format_args!("inc {}", reg));
        let old_value = self.get_reg(reg);
        let new_value = old_value.wrapping_add(1);
        self.set_reg(reg, new_value);
        // Signed overflow: 0x7FFF became 0x8000, meaning +32767 became -32768
        self.set_flag(Flag::Overflow, old_value == 0x7FFF);
        // Was original low nibble 0b1111
        self.set_flag(Flag::AuxiliaryCarry, old_value & 0xF == 0xF);
        self.set_result_flags(new_value);
    }

    fn dec16(&mut self, instruction: u8, reg: Reg16) {
        self.print_instruction(instruction, &format_args!("dec {}", reg));
        let old_value = self.get_reg(reg);
        let new_value = old_value.wrapping_sub(1);
        self.set_reg(reg, new_value);
        // Overflow: 0x8000 became 0x7FFF, meaning -32768 became +32767
        self.set_flag(Flag::Overflow, old_value == 0x8000);
        // Did decrementing borrowing into lower nibble (was it 0)
        self.set_flag(Flag::AuxiliaryCarry, old_value & 0x0F == 0);
        self.set_result_flags(new_value);
    }

    fn alu8(&mut self, op: AluOp, lhs: u8, rhs: u8) -> u8 {
        match op {
            AluOp::Add => self.add8(lhs, rhs, false),
            AluOp::Or => {
                let result = lhs | rhs;
                self.update_logic_flags(result);
                result
            }
            AluOp::Adc => self.add8(lhs, rhs, self.get_flag(Flag::Carry)),
            AluOp::Sbb => self.subtract8(lhs, rhs, self.get_flag(Flag::Carry)),
            AluOp::And => {
                let result = lhs & rhs;
                self.update_logic_flags(result);
                result
            }
            AluOp::Sub | AluOp::Cmp => self.subtract8(lhs, rhs, false),
            AluOp::Xor => {
                let result = lhs ^ rhs;
                self.update_logic_flags(result);
                result
            }
        }
    }

    fn alu16(&mut self, op: AluOp, lhs: u16, rhs: u16) -> u16 {
        match op {
            AluOp::Add => self.add16(lhs, rhs, false),
            AluOp::Or => {
                let result = lhs | rhs;
                self.update_logic_flags(result);
                result
            }
            AluOp::Adc => self.add16(lhs, rhs, self.get_flag(Flag::Carry)),
            AluOp::Sbb => self.subtract16(lhs, rhs, self.get_flag(Flag::Carry)),
            AluOp::And => {
                let result = lhs & rhs;
                self.update_logic_flags(result);
                result
            }
            AluOp::Sub | AluOp::Cmp => self.subtract16(lhs, rhs, false),
            AluOp::Xor => {
                let result = lhs ^ rhs;
                self.update_logic_flags(result);
                result
            }
        }
    }

    fn effective_shift_count(&self, count: u8) -> u8 {
        if self.model == CpuModel::I8088 {
            count
        } else {
            // Mask to the lowest 5 bits
            count & 0x1F
        }
    }

    fn shift_op8(&mut self, op: ShiftOp, value: u8, count: u8) -> u8 {
        match op {
            ShiftOp::Rol => {
                let result = value.rotate_left(count as u32);
                let cf = (result & 1) != 0;
                self.set_flag(Flag::Carry, cf);
                self.set_flag(Flag::Overflow, ((result & 0x80) != 0) ^ cf);
                result
            }
            ShiftOp::Ror => {
                let result = value.rotate_right(count as u32);
                // Last bit rotated out (i.e., the final result's bit 7)
                self.set_flag(Flag::Carry, result & 0x80 != 0);
                // XOR of result's bits 7 and 6
                self.set_flag(Flag::Overflow, (result & 0x80 != 0) ^ (result & 0x40 != 0));
                result
            }
            ShiftOp::Rcl => {
                // Rotate 9 bits (CF,r/m byte) left count times
                let mut result = value;
                let mut carry = self.get_flag(Flag::Carry);
                for _ in 0..count {
                    let next_carry = result & 0x80 != 0;
                    result = (result << 1) | carry as u8;
                    carry = next_carry;
                }
                self.set_flag(Flag::Carry, carry);
                self.set_flag(Flag::Overflow, ((result & 0x80) != 0) ^ carry);
                result
            }
            ShiftOp::Rcr => {
                // Rotate 9 bits (CF,r/m byte) right count times
                let mut result = value;
                let mut carry = self.get_flag(Flag::Carry);
                for _ in 0..count {
                    let next_carry = result & 1 != 0;
                    result = (result >> 1) | ((carry as u8) << 7);
                    carry = next_carry;
                }
                self.set_flag(Flag::Carry, carry);
                self.set_flag(
                    Flag::Overflow,
                    ((result & 0x80) != 0) ^ ((result & 0x40) != 0),
                );
                result
            }
            ShiftOp::Shl => {
                // Shift operand left by count
                let mut result = value;
                let mut last_bit_7 = false;
                for _ in 0..count {
                    last_bit_7 = (result & 0x80) != 0;
                    result = result << 1;
                }
                self.set_result_flags(result);
                // Last bit shifted out of bit 7
                self.set_flag(Flag::Carry, last_bit_7);
                self.set_flag(
                    Flag::Overflow,
                    ((result & 0x80) != 0) ^ self.get_flag(Flag::Carry),
                );
                result
            }
            ShiftOp::Shr => {
                // Shift operand right by count
                let mut result = value;
                let mut last_bit_0 = false;
                for _ in 0..count {
                    last_bit_0 = (result & 1) != 0;
                    result >>= 1;
                }
                self.set_result_flags(result);
                // Last bit shifted out of bit 0
                self.set_flag(Flag::Carry, last_bit_0);
                self.set_flag(
                    Flag::Overflow,
                    ((result & 0x80) != 0) ^ ((result & 0x40) != 0),
                );
                result
            }
            ShiftOp::Sar => {
                // Shift right by count, preserving sign bit
                let mut result = value;
                let mut carry = false;
                for _ in 0..count {
                    carry = result & 1 != 0;
                    result = ((result as i8) >> 1) as u8;
                }
                self.set_result_flags(result);
                self.set_flag(Flag::Carry, carry);
                self.set_flag(Flag::Overflow, false);
                result
            }
        }
    }

    fn shift_op16(&mut self, op: ShiftOp, value: u16, count: u8) -> u16 {
        match op {
            ShiftOp::Rol => {
                let result = value.rotate_left(count as u32);
                let cf = (result & 1) != 0;
                self.set_flag(Flag::Carry, cf);
                self.set_flag(Flag::Overflow, ((result & 0x8000) != 0) ^ cf);
                result
            }
            ShiftOp::Ror => {
                let result = value.rotate_right(count as u32);
                self.set_flag(Flag::Carry, result & 0x8000 != 0);
                self.set_flag(
                    Flag::Overflow,
                    (result & 0x8000 != 0) ^ (result & 0x4000 != 0),
                );
                result
            }
            ShiftOp::Rcl => {
                let mut result = value;
                let mut carry = self.get_flag(Flag::Carry);
                for _ in 0..count {
                    let next_carry = result & 0x8000 != 0;
                    result = (result << 1) | carry as u16;
                    carry = next_carry;
                }
                self.set_flag(Flag::Carry, carry);
                self.set_flag(Flag::Overflow, ((result & 0x8000) != 0) ^ carry);
                result
            }
            ShiftOp::Rcr => {
                let mut result = value;
                let mut carry = self.get_flag(Flag::Carry);
                for _ in 0..count {
                    let next_carry = result & 1 != 0;
                    result = (result >> 1) | ((carry as u16) << 15);
                    carry = next_carry;
                }
                self.set_flag(Flag::Carry, carry);
                self.set_flag(
                    Flag::Overflow,
                    ((result & 0x8000) != 0) ^ ((result & 0x4000) != 0),
                );
                result
            }
            ShiftOp::Shl => {
                // Shift value left by count
                let mut result = value;
                let mut last_bit_15 = false;
                for _ in 0..count {
                    last_bit_15 = (result & 0x8000) != 0;
                    result = result << 1;
                }
                self.set_result_flags(result);
                // Last bit shifted out of bit 15
                self.set_flag(Flag::Carry, last_bit_15);
                self.set_flag(
                    Flag::Overflow,
                    ((result & 0x8000) != 0) ^ self.get_flag(Flag::Carry),
                );
                result
            }
            ShiftOp::Shr => {
                // Shift value right by count
                let mut result = value;
                let mut last_bit_0 = false;
                for _ in 0..count {
                    last_bit_0 = (result & 1) != 0;
                    result >>= 1;
                }
                self.set_result_flags(result);
                // Last bit shifted out of bit 0
                self.set_flag(Flag::Carry, last_bit_0);
                self.set_flag(
                    Flag::Overflow,
                    ((result & 0x8000) != 0) ^ ((result & 0x4000) != 0),
                );
                result
            }
            ShiftOp::Sar => {
                // Shift right by count, preserving sign bit
                let mut result = value;
                let mut carry = false;
                for _ in 0..count {
                    carry = result & 1 != 0;
                    result = ((result as i16) >> 1) as u16;
                }
                self.set_result_flags(result);
                self.set_flag(Flag::Carry, carry);
                self.set_flag(Flag::Overflow, false);
                result
            }
        }
    }

    pub(crate) fn trace(&self) {
        if self.verbosity > 2 {
            eprintln!("\x1b[37m{:?}\x1b[0m", self);
        }
    }

    fn print_instruction(&self, op: u8, instruction: &dyn fmt::Debug) {
        if self.verbosity > 1 {
            eprintln!(
                "\x1b[1m{:#04X}\x1b[0m \x1b[94m({:?})\x1b[0m",
                op, instruction
            );
        }
    }

    /// Normalize bits in flags to avoid writing reserved bits
    fn normalize_flags(&self, flags: u16) -> u16 {
        let high_bit_mask = match self.model {
            CpuModel::I8088 => 0xF002,
            CpuModel::I80286 => 0x0002,
        };
        (flags & 0x0FD5) | high_bit_mask
    }
}

impl Debug for Cpu {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct("Cpu")
            .field("ax", &format_args!("{:#06X}", self.ax))
            .field("bx", &format_args!("{:#06X}", self.bx))
            .field("cx", &format_args!("{:#06X}", self.cx))
            .field("dx", &format_args!("{:#06X}", self.dx))
            .field("si", &format_args!("{:#06X}", self.si))
            .field("di", &format_args!("{:#06X}", self.di))
            .field("sp", &format_args!("{:#06X}", self.sp))
            .field("bp", &format_args!("{:#06X}", self.bp))
            .field("ip", &format_args!("{:#06X}", self.ip))
            .field("cs", &format_args!("{:#06X}", self.cs))
            .field("ds", &format_args!("{:#06X}", self.ds))
            .field("es", &format_args!("{:#06X}", self.es))
            .field("ss", &format_args!("{:#06X}", self.ss))
            .field("flags", &format_args!("{:09b}", self.flags))
            .finish()
    }
}

#[derive(Copy, Clone, PartialEq)]
enum AluOp {
    Add = 0,
    Or = 1,
    Adc = 2,
    Sbb = 3,
    And = 4,
    Sub = 5,
    Xor = 6,
    Cmp = 7,
}

impl Display for AluOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AluOp::Add => write!(f, "add"),
            AluOp::Or => write!(f, "or"),
            AluOp::Adc => write!(f, "adc"),
            AluOp::Sbb => write!(f, "sbb"),
            AluOp::And => write!(f, "and"),
            AluOp::Sub => write!(f, "sub"),
            AluOp::Xor => write!(f, "xor"),
            AluOp::Cmp => write!(f, "cmp"),
        }
    }
}

impl TryFrom<u8> for AluOp {
    type Error = CpuError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(AluOp::Add),
            1 => Ok(AluOp::Or),
            2 => Ok(AluOp::Adc),
            3 => Ok(AluOp::Sbb),
            4 => Ok(AluOp::And),
            5 => Ok(AluOp::Sub),
            6 => Ok(AluOp::Xor),
            7 => Ok(AluOp::Cmp),
            _ => return Err(CpuError::UnsupportedAluOp(value)),
        }
    }
}

#[derive(Copy, Clone, PartialEq)]
enum ShiftOp {
    Rol = 0,
    Ror = 1,
    Rcl = 2,
    Rcr = 3,
    Shl = 4,
    Shr = 5,
    Sar = 7,
}

impl Display for ShiftOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShiftOp::Rol => write!(f, "rol"),
            ShiftOp::Ror => write!(f, "ror"),
            ShiftOp::Rcl => write!(f, "rcl"),
            ShiftOp::Rcr => write!(f, "rcr"),
            ShiftOp::Shl => write!(f, "shl"),
            ShiftOp::Shr => write!(f, "shr"),
            ShiftOp::Sar => write!(f, "sar"),
        }
    }
}

impl TryFrom<u8> for ShiftOp {
    type Error = CpuError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(ShiftOp::Rol),
            1 => Ok(ShiftOp::Ror),
            2 => Ok(ShiftOp::Rcl),
            3 => Ok(ShiftOp::Rcr),
            4 => Ok(ShiftOp::Shl),
            5 => Ok(ShiftOp::Shr),
            7 => Ok(ShiftOp::Sar),
            _ => Err(CpuError::UnsupportedShiftOp(value)),
        }
    }
}
