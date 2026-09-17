use crate::memory::{MemoryError, PhysAddr, read_byte, read_word, write_byte, write_word};
use crate::registers::{Reg8, Reg16, SegmentReg};
use core::fmt::{self, Debug};
use mod_rm::{RegisterOrMemoryOperand, parse_mod_rm};
use thiserror::Error;

mod mod_rm;

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
    #[error("Memory Error: {0}")]
    MemoryError(#[from] MemoryError),
}

pub(crate) enum CpuStepResult {
    Continue,
    Interrupt(u8),
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

impl Cpu {
    pub(crate) fn new() -> Self {
        Cpu {
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
        }
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

    fn set_segment_reg(&mut self, reg: SegmentReg, value: u16) {
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

    fn set_reg16(&mut self, reg: Reg16, value: u16) {
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

    fn read_operand8(
        &self,
        memory: &[u8],
        operand: &RegisterOrMemoryOperand<Reg8>,
    ) -> Result<u8, CpuError> {
        let value = match operand {
            RegisterOrMemoryOperand::Memory(seg_reg, addr) => {
                let seg_addr = self.get_segment_reg(*seg_reg);
                read_byte(memory, PhysAddr::new(seg_addr, *addr))?
            }
            RegisterOrMemoryOperand::Register(reg) => self.get_reg8(*reg),
        };
        Ok(value)
    }

    fn read_operand16(
        &self,
        memory: &[u8],
        operand: &RegisterOrMemoryOperand<Reg16>,
    ) -> Result<u16, CpuError> {
        let value = match operand {
            RegisterOrMemoryOperand::Memory(seg_reg, addr) => {
                let seg_addr = self.get_segment_reg(*seg_reg);
                read_word(memory, PhysAddr::new(seg_addr, *addr))?
            }
            RegisterOrMemoryOperand::Register(reg) => self.get_reg16(*reg),
        };
        Ok(value)
    }

    fn write_operand8(
        &mut self,
        memory: &mut [u8],
        operand: &RegisterOrMemoryOperand<Reg8>,
        value: u8,
    ) -> Result<(), CpuError> {
        match operand {
            RegisterOrMemoryOperand::Memory(seg_reg, addr) => {
                let seg_addr = self.get_segment_reg(*seg_reg);
                write_byte(memory, PhysAddr::new(seg_addr, *addr), value)?
            }
            RegisterOrMemoryOperand::Register(reg) => self.set_reg8(*reg, value),
        };
        Ok(())
    }

    fn write_operand16(
        &mut self,
        memory: &mut [u8],
        operand: &RegisterOrMemoryOperand<Reg16>,
        value: u16,
    ) -> Result<(), CpuError> {
        match operand {
            RegisterOrMemoryOperand::Memory(seg_reg, addr) => {
                let seg_addr = self.get_segment_reg(*seg_reg);
                write_word(memory, PhysAddr::new(seg_addr, *addr), value)?
            }
            RegisterOrMemoryOperand::Register(reg) => self.set_reg16(*reg, value),
        };
        Ok(())
    }

    fn update_logic_flags8(&mut self, result: u8) {
        self.set_flag(Flag::Carry, false);
        self.set_flag(Flag::Overflow, false);
        self.set_result_flags8(result);
    }

    fn update_logic_flags16(&mut self, result: u16) {
        self.set_flag(Flag::Carry, false);
        self.set_flag(Flag::Overflow, false);
        self.set_result_flags16(result);
    }

    /// Fetch, decode, and execute one instruction
    pub(crate) fn step(&mut self, memory: &mut [u8]) -> Result<CpuStepResult, CpuError> {
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
                eprint!("\x1b[1m{:#04X}\x1b[0m \x1b[92m({})\x1b[0m ", byte, seg);
                continue;
            }

            // Check for REPE/REPNE prefix
            if byte == 0xF2 || byte == 0xF3 {
                rep_prefix = Some(byte);
                eprint!("\x1b[1m{:#04X}\x1b[0m ", byte);
                continue;
            }

            break;
        }

        let mut result = CpuStepResult::Continue;
        // Table 4-13
        match byte {
            // Arithmetic
            0x00 => {
                // ADD r/m8, r8
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let source_reg = Reg8::try_from(parsed.reg_field)?;
                let rm = self.decode_rm8_operand(memory, parsed, segment_override)?;
                print_instruction(byte, &format_args!("add {}, {}", rm, source_reg));

                let reg_value = self.get_reg8(source_reg);
                let rm_value = self.read_operand8(memory, &rm)?;
                let value = self.add8(reg_value, rm_value, false);
                self.write_operand8(memory, &rm, value)?;
            }
            0x01 => {
                // ADD r/m16, r16
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let source_reg = Reg16::try_from(parsed.reg_field)?;
                let rm = self.decode_rm16_operand(memory, parsed, segment_override)?;
                print_instruction(byte, &format_args!("add {}, {}", rm, source_reg));

                let reg_value = self.get_reg16(source_reg);
                let rm_value = self.read_operand16(memory, &rm)?;
                let value = self.add16(reg_value, rm_value, false);
                self.write_operand16(memory, &rm, value)?;
            }
            0x02 => {
                // ADD r8, r/m8
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let dest_reg = Reg8::try_from(parsed.reg_field)?;
                let rm = self.decode_rm8_operand(memory, parsed, segment_override)?;
                print_instruction(byte, &format_args!("add {}, {}", dest_reg, rm));

                let reg_value = self.get_reg8(dest_reg);
                let rm_value = self.read_operand8(memory, &rm)?;
                let value = self.add8(reg_value, rm_value, false);
                self.set_reg8(dest_reg, value);
            }
            0x03 => {
                // ADD r16, r/m16
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let dest_reg = Reg16::try_from(parsed.reg_field)?;
                let rm = self.decode_rm16_operand(memory, parsed, segment_override)?;
                print_instruction(byte, &format_args!("add {}, {}", dest_reg, rm));

                let reg_value = self.get_reg16(dest_reg);
                let rm_value = self.read_operand16(memory, &rm)?;
                let value = self.add16(reg_value, rm_value, false);
                self.set_reg16(dest_reg, value);
            }
            0x04 => {
                // ADD AL, IMMED8
                let immediate = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("add al, {:04X}h", immediate));

                let reg_value = self.get_reg8(Reg8::Al);
                let value = self.add8(reg_value, immediate, false);
                self.set_reg8(Reg8::Al, value);
            }
            0x05 => {
                // ADD AX, IMMED16
                let immediate = self.fetch_next_word_and_advance_ip(memory);
                print_instruction(byte, &format_args!("add ax, {:04X}h", immediate));

                let reg_value = self.get_reg16(Reg16::Ax);
                let value = self.add16(reg_value, immediate, false);
                self.set_reg16(Reg16::Ax, value);
            }
            0x08 => {
                // OR REG8/MEM8, REG8
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let source_reg = Reg8::try_from(parsed.reg_field)?;
                let rm = self.decode_rm8_operand(memory, parsed, segment_override)?;
                print_instruction(byte, &format_args!("or {}, {}", rm, source_reg));

                let reg_value = self.get_reg8(source_reg);
                let rm_value = self.read_operand8(memory, &rm)?;

                let value = reg_value | rm_value;
                self.write_operand8(memory, &rm, value)?;
                self.update_logic_flags8(value);
            }
            0x09 => {
                // OR REG16/MEM16, REG16
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let source_reg = Reg16::try_from(parsed.reg_field)?;
                let rm = self.decode_rm16_operand(memory, parsed, segment_override)?;
                print_instruction(byte, &format_args!("or {}, {}", rm, source_reg));

                let reg_value = self.get_reg16(source_reg);
                let rm_value = self.read_operand16(memory, &rm)?;

                let value = reg_value | rm_value;
                self.write_operand16(memory, &rm, value)?;
                self.update_logic_flags16(value);
            }
            0x0A => {
                // OR REG8, REG8/MEM8
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let dest_reg = Reg8::try_from(parsed.reg_field)?;
                let rm = self.decode_rm8_operand(memory, parsed, segment_override)?;
                print_instruction(byte, &format_args!("or {}, {}", dest_reg, rm));

                let reg_value = self.get_reg8(dest_reg);
                let rm_value = self.read_operand8(memory, &rm)?;

                let value = reg_value | rm_value;
                self.set_reg8(dest_reg, value);
                self.update_logic_flags8(value);
            }
            0x0B => {
                // OR REG16, REG16/MEM16
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let dest_reg = Reg16::try_from(parsed.reg_field)?;
                let rm = self.decode_rm16_operand(memory, parsed, segment_override)?;
                print_instruction(byte, &format_args!("or {}, {}", dest_reg, rm));

                let reg_value = self.get_reg16(dest_reg);
                let rm_value = self.read_operand16(memory, &rm)?;

                let value = reg_value | rm_value;
                self.set_reg16(dest_reg, value);
                self.update_logic_flags16(value);
            }
            0x0C => {
                // OR AL, IMMED8
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("or al, {}", operand));

                let value = operand | self.get_reg8(Reg8::Al);
                self.set_reg8(Reg8::Al, value);
                self.update_logic_flags8(value);
            }
            0x0E => {
                // PUSH CS
                print_instruction(byte, &format_args!("push cs"));
                self.push_onto_stack(memory, self.cs)?;
            }
            0x1E => {
                // PUSH DS
                print_instruction(byte, &format_args!("push ds"));
                self.push_onto_stack(memory, self.ds)?;
            }
            0x1F => {
                // POP DS
                print_instruction(byte, &format_args!("pop ds"));
                self.ds = self.pop_from_stack(memory)?;
            }
            0x24 => {
                // AND AL, IMMED8
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("and al, {:X}h", operand));
                let reg_value = self.get_reg8(Reg8::Al);
                let value = reg_value & operand;
                self.set_reg8(Reg8::Al, value);
                self.update_logic_flags8(value);
            }
            0x25 => {
                // AND AX, IMMED16
                let operand = self.fetch_next_word_and_advance_ip(memory);
                print_instruction(byte, &format_args!("and ax, {:X}h", operand));
                let reg_value = self.get_reg16(Reg16::Ax);
                let value = reg_value & operand;
                self.set_reg16(Reg16::Ax, value);
                self.update_logic_flags16(value);
            }
            0x28 => {
                // SUB r/m8, r8
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let source_reg = Reg8::try_from(parsed.reg_field)?;
                let rm = self.decode_rm8_operand(memory, parsed, segment_override)?;
                print_instruction(byte, &format_args!("sub {}, {}", rm, source_reg));

                let rm_value = self.read_operand8(memory, &rm)?;
                let value = self.subtract8(rm_value, self.get_reg8(source_reg), false);
                self.write_operand8(memory, &rm, value)?;
            }
            0x2A => {
                // SUB r8, r/m8
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let dest_reg = Reg8::try_from(parsed.reg_field)?;
                let rm = self.decode_rm8_operand(memory, parsed, segment_override)?;
                print_instruction(byte, &format_args!("sub {}, {}", dest_reg, rm));

                let rm_value = self.read_operand8(memory, &rm)?;
                let value = self.subtract8(self.get_reg8(dest_reg), rm_value, false);
                self.set_reg8(dest_reg, value);
            }
            0x2C => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("sub al, {:X}h", operand));
                let new_val = self.subtract8(self.get_reg8(Reg8::Al), operand, false);
                self.set_reg8(Reg8::Al, new_val);
            }
            0x31 => {
                // XOR REG16/MEM16, REG16
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let source_reg = Reg16::try_from(parsed.reg_field)?;
                let rm = self.decode_rm16_operand(memory, parsed, segment_override)?;
                print_instruction(byte, &format_args!("xor {}, {}", rm, source_reg));

                let reg_value = self.get_reg16(source_reg);
                let rm_value = self.read_operand16(memory, &rm)?;

                let value = reg_value ^ rm_value;
                self.write_operand16(memory, &rm, value)?;
                self.update_logic_flags16(value);
            }
            0x32 => {
                // XOR REG8, REG8/MEM8
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let dest_reg = Reg8::try_from(parsed.reg_field)?;
                let rm = self.decode_rm8_operand(memory, parsed, segment_override)?;
                print_instruction(byte, &format_args!("xor {}, {}", dest_reg, rm));

                let reg_value = self.get_reg8(dest_reg);
                let rm_value = self.read_operand8(memory, &rm)?;

                let value = reg_value ^ rm_value;
                self.set_reg8(dest_reg, value);
                self.update_logic_flags8(value);
            }
            0x33 => {
                // XOR REG16, REG16/MEM16
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let dest_reg = Reg16::try_from(parsed.reg_field)?;
                let rm = self.decode_rm16_operand(memory, parsed, segment_override)?;
                print_instruction(byte, &format_args!("xor {}, {}", dest_reg, rm));

                let reg_value = self.get_reg16(dest_reg);
                let rm_value = self.read_operand16(memory, &rm)?;

                let value = reg_value ^ rm_value;
                self.set_reg16(dest_reg, value);
                self.update_logic_flags16(value);
            }
            0x3A => {
                // CMP r8, r/m8
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let dest_reg = Reg8::try_from(parsed.reg_field)?;
                let rm = self.decode_rm8_operand(memory, parsed, segment_override)?;
                print_instruction(byte, &format_args!("cmp {}, {}", dest_reg, rm));

                let rm_value = self.read_operand8(memory, &rm)?;
                self.subtract8(self.get_reg8(dest_reg), rm_value, false);
            }
            0x3B => {
                // CMP r16, r/m16
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let dest_reg = Reg16::try_from(parsed.reg_field)?;
                let rm = self.decode_rm16_operand(memory, parsed, segment_override)?;
                print_instruction(byte, &format_args!("cmp {}, {}", dest_reg, rm));

                let rm_value = self.read_operand16(memory, &rm)?;
                self.subtract16(self.get_reg16(dest_reg), rm_value);
            }
            0x3C => {
                // CMP AL, imm8 - basically SUB AL that doesn't change AL
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("cmp al, {:X}h", operand));
                self.subtract8(self.get_reg8(Reg8::Al), operand, false);
            }
            0x3D => {
                // CMP AX, imm16
                let operand = self.fetch_next_word_and_advance_ip(memory);
                print_instruction(byte, &format_args!("cmp ax, {:X}h", operand));
                self.subtract16(self.get_reg16(Reg16::Ax), operand);
            }
            0x40 => self.inc16(byte, Reg16::Ax),
            0x41 => self.inc16(byte, Reg16::Cx),
            0x42 => self.inc16(byte, Reg16::Dx),
            0x43 => self.inc16(byte, Reg16::Bx),
            0x44 => self.inc16(byte, Reg16::Sp),
            0x45 => self.inc16(byte, Reg16::Bp),
            0x46 => self.inc16(byte, Reg16::Si),
            0x47 => self.inc16(byte, Reg16::Di),
            0x48 => self.dec16(byte, Reg16::Ax),
            0x49 => self.dec16(byte, Reg16::Cx),
            0x4A => self.dec16(byte, Reg16::Dx),
            0x4B => self.dec16(byte, Reg16::Bx),
            0x4C => self.dec16(byte, Reg16::Sp),
            0x4D => self.dec16(byte, Reg16::Bp),
            0x4E => self.dec16(byte, Reg16::Si),
            0x4F => self.dec16(byte, Reg16::Di),
            // PUSH/POP 50-5F
            0x50 => {
                print_instruction(byte, &format_args!("push ax"));
                self.push_onto_stack(memory, self.ax)?;
            }
            0x51 => {
                print_instruction(byte, &format_args!("push cx"));
                self.push_onto_stack(memory, self.cx)?;
            }
            0x52 => {
                print_instruction(byte, &format_args!("push dx"));
                self.push_onto_stack(memory, self.dx)?;
            }
            0x53 => {
                print_instruction(byte, &format_args!("push bx"));
                self.push_onto_stack(memory, self.bx)?;
            }
            0x54 => {
                print_instruction(byte, &format_args!("push sp"));
                // 8086/8088 hardware quirk
                let value = self.sp.wrapping_sub(2);
                self.push_onto_stack(memory, value)?;
            }
            0x55 => {
                print_instruction(byte, &format_args!("push bp"));
                self.push_onto_stack(memory, self.bp)?;
            }
            0x56 => {
                print_instruction(byte, &format_args!("push si"));
                self.push_onto_stack(memory, self.si)?;
            }
            0x57 => {
                print_instruction(byte, &format_args!("push di"));
                self.push_onto_stack(memory, self.di)?;
            }
            0x58 => {
                print_instruction(byte, &format_args!("pop ax"));
                self.ax = self.pop_from_stack(memory)?;
            }
            0x59 => {
                print_instruction(byte, &format_args!("pop cx"));
                self.cx = self.pop_from_stack(memory)?;
            }
            0x5A => {
                print_instruction(byte, &format_args!("pop dx"));
                self.dx = self.pop_from_stack(memory)?;
            }
            0x5B => {
                print_instruction(byte, &format_args!("pop bx"));
                self.bx = self.pop_from_stack(memory)?;
            }
            0x5C => {
                print_instruction(byte, &format_args!("pop sp"));
                self.sp = self.pop_from_stack(memory)?;
            }
            0x5D => {
                print_instruction(byte, &format_args!("pop bp"));
                self.bp = self.pop_from_stack(memory)?;
            }
            0x5E => {
                print_instruction(byte, &format_args!("pop si"));
                self.si = self.pop_from_stack(memory)?;
            }
            0x5F => {
                print_instruction(byte, &format_args!("pop di"));
                self.di = self.pop_from_stack(memory)?;
            }
            // Jumps
            0x72 => {
                // Jump if Below
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                print_instruction(byte, &format_args!("jb {:X}h", operand));
                // If CF is 1, then jump ("jump if carry")
                if self.get_flag(Flag::Carry) {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x73 => {
                // JAE/JNC/JNB
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                print_instruction(byte, &format_args!("jae {:X}h", operand));
                // Jump if CF is clear
                if !self.get_flag(Flag::Carry) {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x74 => {
                // JE/JZ
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                print_instruction(byte, &format_args!("je short {:X}h", operand));
                // If zero flag is set, then jump
                if self.get_flag(Flag::Zero) {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x75 => {
                // JNE/JNZ
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                print_instruction(byte, &format_args!("jnz short {:X}h", operand));
                // If zero flag is not set, then jump
                if !self.get_flag(Flag::Zero) {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x76 => {
                // JBE/JNA (jump if below or equal)
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                print_instruction(byte, &format_args!("jbe short {:X}h", operand));
                // If CF or ZF are set, jump
                if self.get_flag(Flag::Carry) || self.get_flag(Flag::Zero) {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x77 => {
                // Jump if Above
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                print_instruction(byte, &format_args!("ja {:X}h", operand));
                // If CF is 0 AND ZF = 0, jump
                if !self.get_flag(Flag::Carry) && !self.get_flag(Flag::Zero) {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x7C => {
                // JL/JNGE (jump if signed less-than)
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                print_instruction(byte, &format_args!("jl {:X}h", operand));
                // If SF != OF, jump
                if self.get_flag(Flag::Sign) != self.get_flag(Flag::Overflow) {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x7F => {
                // JNLE/JG (jump if not less than or equal)
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                print_instruction(byte, &format_args!("jg {:X}h", operand));
                // If not ZF and SF == OF
                if !self.get_flag(Flag::Zero)
                    && (self.get_flag(Flag::Sign) == self.get_flag(Flag::Overflow))
                {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            0x80 => {
                // Does a lot of things! on REG8/MEM8, IMMED8
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let reg_field = parsed.reg_field;
                let rm = self.decode_rm8_operand(memory, parsed, segment_override)?;
                let immediate = self.fetch_next_byte_and_advance_ip(memory);
                let destination = self.read_operand8(memory, &rm)?;
                let mut is_cmp = false;
                let value = match reg_field {
                    0 => {
                        print_instruction(byte, &format_args!("add {}, {:X}h", rm, immediate));
                        self.add8(destination, immediate, false)
                    } // ADD
                    1 => {
                        // OR
                        print_instruction(byte, &format_args!("or {}, {:X}h", rm, immediate));
                        let result = destination | immediate;
                        self.update_logic_flags8(result);
                        result
                    }
                    2 => {
                        // ADC
                        print_instruction(byte, &format_args!("adc {}, {:X}h", rm, immediate));
                        self.add8(destination, immediate, self.get_flag(Flag::Carry))
                    }
                    3 => {
                        // SBB
                        print_instruction(byte, &format_args!("sbb {}, {:X}h", rm, immediate));
                        self.subtract8(destination, immediate, self.get_flag(Flag::Carry))
                    }
                    4 => {
                        // AND
                        print_instruction(byte, &format_args!("and {}, {:X}h", rm, immediate));
                        let result = destination & immediate;
                        self.update_logic_flags8(result);
                        result
                    }
                    5 => {
                        // SUB
                        print_instruction(byte, &format_args!("sub {}, {:X}h", rm, immediate));
                        self.subtract8(destination, immediate, false)
                    }
                    6 => {
                        // XOR
                        print_instruction(byte, &format_args!("xor {}, {:X}h", rm, immediate));
                        let result = destination ^ immediate;
                        self.update_logic_flags8(result);
                        result
                    }
                    7 => {
                        // CMP
                        print_instruction(byte, &format_args!("cmp {}, {:X}h", rm, immediate));
                        is_cmp = true;
                        self.subtract8(destination, immediate, false)
                    }
                    _ => return Err(CpuError::InvalidRegisterValue(reg_field)),
                };
                if !is_cmp {
                    self.write_operand8(memory, &rm, value)?;
                }
            }
            0x83 => {
                // Various things to do with r/m16, immed8
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let reg_field = parsed.reg_field;
                let rm = self.decode_rm16_operand(memory, parsed, segment_override)?;
                let immediate = self.fetch_next_byte_and_advance_ip(memory) as i8 as i16 as u16;
                let destination = self.read_operand16(memory, &rm)?;
                let value = match reg_field {
                    0b000 => {
                        // ADD r/m16, immed8
                        print_instruction(byte, &format_args!("add {}, {:X}h", rm, immediate));
                        self.add16(destination, immediate, false)
                    }
                    _ => return Err(CpuError::InvalidRegisterValue(reg_field)),
                };
                self.write_operand16(memory, &rm, value)?;
            }
            0x86 => {
                // XCHG REG8,REG8/MEM8
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let first_reg = Reg8::try_from(parsed.reg_field)?;
                let rm = self.decode_rm8_operand(memory, parsed, segment_override)?;
                print_instruction(byte, &format_args!("xchg {}, {}", first_reg, rm));

                let first_value = self.get_reg8(first_reg);
                let second_value = self.read_operand8(memory, &rm)?;
                self.set_reg8(first_reg, second_value);
                self.write_operand8(memory, &rm, first_value)?;
            }
            // MOVs
            0x88 => {
                // Mov reg8/mem8, reg8
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let reg = Reg8::try_from(parsed.reg_field)?;
                let rm = self.decode_rm8_operand(memory, parsed, segment_override)?;

                print_instruction(byte, &format_args!("mov {}, {}", rm, reg));

                let value = self.get_reg8(reg);
                self.write_operand8(memory, &rm, value)?;
            }
            0x89 => {
                // mov reg16/mem16, reg16
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let reg = Reg16::try_from(parsed.reg_field)?;
                let rm = self.decode_rm16_operand(memory, parsed, segment_override)?;
                print_instruction(byte, &format_args!("mov {}, {}", rm, reg));
                let value = self.get_reg16(reg);
                self.write_operand16(memory, &rm, value)?;
            }
            0x8A => {
                // MOV REG8, R/M8
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let reg = Reg8::try_from(parsed.reg_field)?;
                let rm = self.decode_rm8_operand(memory, parsed, segment_override)?;
                print_instruction(byte, &format_args!("mov {}, {}", reg, rm));
                let value = self.read_operand8(memory, &rm)?;
                self.set_reg8(reg, value);
            }
            0x8B => {
                // MOV REG16, R/M16
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let reg = Reg16::try_from(parsed.reg_field)?;
                let rm = self.decode_rm16_operand(memory, parsed, segment_override)?;
                print_instruction(byte, &format_args!("mov {}, {}", reg, rm));
                let value = self.read_operand16(memory, &rm)?;
                self.set_reg16(reg, value);
            }
            0x8C => {
                // MOV r/m16, segment register
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let source_segment = SegmentReg::try_from(parsed.reg_field)?;
                let rm = self.decode_rm16_operand(memory, parsed, segment_override)?;
                print_instruction(byte, &format_args!("mov {}, {}", rm, source_segment));

                let value = self.get_segment_reg(source_segment);
                self.write_operand16(memory, &rm, value)?;
            }
            0x8D => {
                // LEA REG16,MEM16
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let dest_reg = Reg16::try_from(parsed.reg_field)?;
                let mod_field = parsed.mod_field;
                let rm = self.decode_rm16_operand(memory, parsed, segment_override)?;
                print_instruction(byte, &format_args!("lea {}, {}", dest_reg, rm));
                match rm {
                    // We write the address, not the memory at address. We ignore the segment.
                    RegisterOrMemoryOperand::Memory(_, addr) => self.set_reg16(dest_reg, addr),
                    RegisterOrMemoryOperand::Register(_) => {
                        // LEA can only have a memory source
                        return Err(CpuError::UnsupportedModField(mod_field));
                    }
                }
            }
            0x8E => {
                // MOV SEGREG, REG16/MEM16
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let dest_segment = SegmentReg::try_from(parsed.reg_field)?;
                let rm = self.decode_rm16_operand(memory, parsed, segment_override)?;
                print_instruction(byte, &format_args!("mov {}, {}", dest_segment, rm));

                let value = self.read_operand16(memory, &rm)?;
                self.set_segment_reg(dest_segment, value);
            }
            // XCHG AX,
            0x90 => {
                // XCHG AX, AX (NOP)
                print_instruction(byte, &format_args!("xchg ax, ax"));
            }
            0x91 => {
                // XCHG AX, CX
                print_instruction(byte, &format_args!("xchg ax, cx"));
                std::mem::swap(&mut self.ax, &mut self.cx);
            }
            0x92 => {
                // XCHG AX, DX
                print_instruction(byte, &format_args!("xchg ax, dx"));
                std::mem::swap(&mut self.ax, &mut self.dx);
            }
            0x93 => {
                // XCHG AX, BX
                print_instruction(byte, &format_args!("xchg ax, bx"));
                std::mem::swap(&mut self.ax, &mut self.bx);
            }
            0x94 => {
                // XCHG AX, SP
                print_instruction(byte, &format_args!("xchg ax, sp"));
                std::mem::swap(&mut self.ax, &mut self.sp);
            }
            0x95 => {
                // XCHG AX, BP
                print_instruction(byte, &format_args!("xchg ax, bp"));
                std::mem::swap(&mut self.ax, &mut self.bp);
            }
            0x96 => {
                // XCHG AX, SI
                print_instruction(byte, &format_args!("xchg ax, si"));
                std::mem::swap(&mut self.ax, &mut self.si);
            }
            0x97 => {
                // XCHG AX, DI
                print_instruction(byte, &format_args!("xchg ax, di"));
                std::mem::swap(&mut self.ax, &mut self.di);
            }
            0x98 => {
                // CBW
                let al = self.get_reg8(Reg8::Al);
                let value = (al as i8) as i16 as u16;
                print_instruction(byte, &format_args!("cbw"));
                self.set_reg16(Reg16::Ax, value);
            }
            0xA0 => {
                // MOV AL, MEM8
                let mem_addr = self.fetch_next_word_and_advance_ip(memory);
                let seg = segment_override.unwrap_or(SegmentReg::Ds);
                print_instruction(byte, &format_args!("mov al, [{}:{:04X}h]", seg, mem_addr));
                let seg_addr = self.get_segment_reg(seg);
                let mem_value = read_byte(memory, PhysAddr::new(seg_addr, mem_addr))?;
                self.set_reg8(Reg8::Al, mem_value);
            }
            0xA1 => {
                // MOV AX, MEM16
                let mem_addr = self.fetch_next_word_and_advance_ip(memory);
                let seg = segment_override.unwrap_or(SegmentReg::Ds);
                print_instruction(byte, &format_args!("mov ax, [{}:{:04X}h]", seg, mem_addr));
                let seg_addr = self.get_segment_reg(seg);
                let mem_value = read_word(memory, PhysAddr::new(seg_addr, mem_addr))?;
                self.set_reg16(Reg16::Ax, mem_value);
            }
            0xA2 => {
                // MOV MEM8, AL
                let mem_addr = self.fetch_next_word_and_advance_ip(memory);
                let al = self.get_reg8(Reg8::Al);
                let seg = segment_override.unwrap_or(SegmentReg::Ds);
                let seg_addr = self.get_segment_reg(seg);
                print_instruction(byte, &format_args!("mov [{}:{:04X}h], al", seg, mem_addr));
                write_byte(memory, PhysAddr::new(seg_addr, mem_addr), al)?;
            }
            0xA3 => {
                // MOV MEM16, AX
                let mem_addr = self.fetch_next_word_and_advance_ip(memory);
                let ax = self.get_reg16(Reg16::Ax);
                let seg = segment_override.unwrap_or(SegmentReg::Ds);
                let seg_addr = self.get_segment_reg(seg);
                print_instruction(byte, &format_args!("mov [{}:{:04X}h], ax", seg, mem_addr));
                write_word(memory, PhysAddr::new(seg_addr, mem_addr), ax)?;
            }
            0xA6 => {
                // CMPSB DEST-STR8, SRC-STR8
                print_instruction(byte, &format_args!("cmpsb"));
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
            0xAA => {
                // STOSB ("store string byte")
                print_instruction(byte, &format_args!("stosb"));
                let reps = if rep_prefix == Some(0xF3) { self.cx } else { 1 };
                for _ in 0..reps {
                    let seg_addr = self.get_segment_reg(SegmentReg::Es);
                    write_byte(
                        memory,
                        PhysAddr::new(seg_addr, self.di),
                        self.get_reg8(Reg8::Al),
                    )?;
                    if self.get_flag(Flag::Direction) {
                        self.di = self.di.wrapping_sub(1);
                    } else {
                        self.di = self.di.wrapping_add(1);
                    }
                    if rep_prefix == Some(0xF3) {
                        self.cx = self.cx.wrapping_sub(1);
                    }
                }
            }
            0xAB => {
                // STOSW ("store string word")
                print_instruction(byte, &format_args!("stosw"));
                let reps = if rep_prefix == Some(0xF3) { self.cx } else { 1 };
                for _ in 0..reps {
                    let seg_addr = self.get_segment_reg(SegmentReg::Es);
                    write_word(memory, PhysAddr::new(seg_addr, self.di), self.ax)?;
                    if self.get_flag(Flag::Direction) {
                        self.di = self.di.wrapping_sub(2);
                    } else {
                        self.di = self.di.wrapping_add(2);
                    }
                    if rep_prefix == Some(0xF3) {
                        self.cx = self.cx.wrapping_sub(1);
                    }
                }
            }
            0xAC => {
                // LODSB
                print_instruction(byte, &format_args!("lodsb"));
                // Repeat if REP prefix was set
                let reps = if rep_prefix == Some(0xF3) { self.cx } else { 1 };
                // TODO: Support interrupt/resume between repetitions
                for _ in 0..reps {
                    let seg = segment_override.unwrap_or(SegmentReg::Ds);
                    let seg_addr = self.get_segment_reg(seg);
                    let byte = read_byte(memory, PhysAddr::new(seg_addr, self.si))?;
                    self.set_reg8(Reg8::Al, byte);
                    if self.get_flag(Flag::Direction) {
                        self.si = self.si.wrapping_sub(1);
                    } else {
                        self.si = self.si.wrapping_add(1);
                    }
                    if rep_prefix == Some(0xF3) {
                        self.cx = self.cx.wrapping_sub(1);
                    }
                }
            }
            // B0-B7: move an 8-bit immediate into an 8-bit register
            0xB0 => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("mov al, {:X}h", operand));
                self.set_reg8(Reg8::Al, operand);
            }
            0xB1 => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("mov cl, {:X}h", operand));
                self.set_reg8(Reg8::Cl, operand);
            }
            0xB2 => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("mov dl, {:X}h", operand));
                self.set_reg8(Reg8::Dl, operand);
            }
            0xB3 => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("mov bl, {:X}h", operand));
                self.set_reg8(Reg8::Bl, operand);
            }
            0xB4 => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("mov ah, {:X}h", operand));
                self.set_reg8(Reg8::Ah, operand);
            }
            0xB5 => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("mov ch, {:X}h", operand));
                self.set_reg8(Reg8::Ch, operand);
            }
            0xB6 => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("mov dh, {:X}h", operand));
                self.set_reg8(Reg8::Dh, operand);
            }
            0xB7 => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("mov bh, {:X}h", operand));
                self.set_reg8(Reg8::Bh, operand);
            }
            // B8-BF: move a 16-bit immediate into a 16-bit register
            0xB8 => {
                let operand = self.fetch_next_word_and_advance_ip(memory);
                print_instruction(byte, &format_args!("mov ax, {:X}h", operand));
                self.ax = operand;
            }
            0xB9 => {
                let operand = self.fetch_next_word_and_advance_ip(memory);
                print_instruction(byte, &format_args!("mov cx, {:X}h", operand));
                self.cx = operand;
            }
            0xBA => {
                let operand = self.fetch_next_word_and_advance_ip(memory);
                print_instruction(byte, &format_args!("mov dx, {:X}h", operand));
                self.dx = operand;
            }
            0xBB => {
                let operand = self.fetch_next_word_and_advance_ip(memory);
                print_instruction(byte, &format_args!("mov bx, {:X}h", operand));
                self.bx = operand;
            }
            0xBC => {
                let operand = self.fetch_next_word_and_advance_ip(memory);
                print_instruction(byte, &format_args!("mov sp, {:X}h", operand));
                self.sp = operand;
            }
            0xBD => {
                let operand = self.fetch_next_word_and_advance_ip(memory);
                print_instruction(byte, &format_args!("mov bp, {:X}h", operand));
                self.bp = operand;
            }
            0xBE => {
                let operand = self.fetch_next_word_and_advance_ip(memory);
                print_instruction(byte, &format_args!("mov si, {:X}h", operand));
                self.si = operand;
            }
            0xBF => {
                let operand = self.fetch_next_word_and_advance_ip(memory);
                print_instruction(byte, &format_args!("mov di, {:X}h", operand));
                self.di = operand;
            }
            // JMP
            0xEB => {
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                print_instruction(byte, &format_args!("jmp {:X}h", operand));
                self.ip = self.ip.wrapping_add_signed(operand as i16);
            }
            // RET
            0xC3 => {
                print_instruction(byte, &format_args!("ret"));
                self.ip = self.pop_from_stack(memory)?;
            }
            // MOV r/m8, imm8
            0xC6 => {
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                if parsed.reg_field != 0 {
                    // The register value is required to be 0
                    return Err(CpuError::InvalidRegisterValue(parsed.reg_field));
                }
                let rm = self.decode_rm8_operand(memory, parsed, segment_override)?;
                let immediate = self.fetch_next_byte_and_advance_ip(memory);

                print_instruction(byte, &format_args!("mov {}, {}", rm, immediate));
                self.write_operand8(memory, &rm, immediate)?;
            }
            // MOV r/m16, imm16
            0xC7 => {
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                if parsed.reg_field != 0 {
                    // The register value is required to be 0
                    return Err(CpuError::InvalidRegisterValue(parsed.reg_field));
                }
                let rm = self.decode_rm16_operand(memory, parsed, segment_override)?;
                let immediate = self.fetch_next_word_and_advance_ip(memory);

                print_instruction(byte, &format_args!("mov {}, {:X}h", rm, immediate));
                self.write_operand16(memory, &rm, immediate)?;
            }
            // Interrupts
            0xCD => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("int {:X}h", operand));
                result = CpuStepResult::Interrupt(operand);
            }
            0xCF => {
                // IRET
                print_instruction(byte, &format_args!("iret"));
                self.ip = self.pop_from_stack(memory)?;
                self.cs = self.pop_from_stack(memory)?;
                self.flags = self.pop_from_stack(memory)?;
            }
            0xD0 => {
                // Does many things with REG8/MEM8, 1
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let reg_field = parsed.reg_field;
                let rm = self.decode_rm8_operand(memory, parsed, segment_override)?;
                match reg_field {
                    0 => {
                        // ROL r/m8, 1
                        print_instruction(byte, &format_args!("rol {}, 1", rm));
                        let operand = self.read_operand8(memory, &rm)?;
                        let value = operand.rotate_left(1);
                        self.write_operand8(memory, &rm, value)?;
                        let cf = (operand & 0x80) != 0;
                        self.set_flag(Flag::Carry, cf);
                        self.set_flag(Flag::Overflow, ((value & 0x80) != 0) ^ cf);
                    }
                    _ => return Err(CpuError::InvalidRegisterValue(reg_field)),
                };
            }
            0xD2 => {
                // Does many things with REG8/MEM8, CL
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let reg_field = parsed.reg_field;
                let rm = self.decode_rm8_operand(memory, parsed, segment_override)?;
                match reg_field {
                    4 => {
                        // Shift an 8-bit operand left by CL, and store result in rm
                        print_instruction(byte, &format_args!("shl {}, cl", rm));
                        let cl = self.get_reg8(Reg8::Cl);
                        if cl != 0 {
                            let operand = self.read_operand8(memory, &rm)?;
                            let mut result = operand;
                            let mut last_bit_7 = false;
                            for _ in 0..cl {
                                last_bit_7 = (result & 0x80) != 0;
                                result = result << 1;
                            }
                            self.write_operand8(memory, &rm, result)?;
                            self.set_result_flags8(result);
                            // Last bit shifted out of bit 7
                            self.set_flag(Flag::Carry, last_bit_7);
                            if cl == 1 {
                                self.set_flag(
                                    Flag::Overflow,
                                    ((result & 0x80) != 0) ^ self.get_flag(Flag::Carry),
                                )
                            }
                        }
                    }
                    _ => return Err(CpuError::InvalidRegisterValue(reg_field)),
                };
            }
            0xE2 => {
                // LOOP rel8
                let displacement = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("loop {:X}h", displacement));
                self.cx = self.cx.wrapping_sub(1);
                if self.cx != 0 {
                    self.ip = self.ip.wrapping_add_signed((displacement as i8) as i16);
                }
            }
            0xE6 => {
                // OUT imm8, AL
                let port = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("out {:X}h, AL", port));
                if port == 0x20 && self.get_reg8(Reg8::Al) == 0x20 {
                    eprintln!("-- Ignored Port 0x20, byte 0x20 --");
                } else {
                    panic!("0xE6 not supported properly");
                }
                // TODO: PortWrite { port: u16, value: u8 }
                // TODO: Machine handles behavior
            }
            // CALL
            0xE8 => {
                // Fetch 16-bit relative displacement
                let displacement = self.fetch_next_word_and_advance_ip(memory);
                print_instruction(byte, &format_args!("call {:X}h", displacement));
                // Push the current IP - it now points after the entire CALL instruction
                self.push_onto_stack(memory, self.ip)?;
                // Add the displacement to IP with 16-bit wrapping
                self.ip = self.ip.wrapping_add(displacement)
            }
            0xE9 => {
                // near relative JMP
                let displacement = self.fetch_next_word_and_advance_ip(memory);
                print_instruction(byte, &format_args!("jmp {:X}h", displacement));
                // Add the displacement to IP with 16-bit wrapping
                self.ip = self.ip.wrapping_add(displacement)
            }
            0xEA => {
                // JMP FAR-LABEL
                let offset = self.fetch_next_word_and_advance_ip(memory);
                let segment = self.fetch_next_word_and_advance_ip(memory);
                print_instruction(byte, &format_args!("jmp {:04X}h:{:04X}h", segment, offset));
                self.ip = offset;
                self.cs = segment;
            }
            0xF6 => {
                // Does many things with REG8/MEM8(, IMMED8)
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let reg_field = parsed.reg_field;
                let rm = self.decode_rm8_operand(memory, parsed, segment_override)?;
                match reg_field {
                    0 => {
                        // TEST R/M8, IMMED8
                        let immediate = self.fetch_next_byte_and_advance_ip(memory);
                        print_instruction(byte, &format_args!("test {}, {:X}h", rm, immediate));
                        let operand = self.read_operand8(memory, &rm)?;
                        let result = operand & immediate;
                        self.update_logic_flags8(result);
                    }
                    _ => return Err(CpuError::InvalidRegisterValue(reg_field)),
                };
            }
            0xF7 => {
                // Does a lot of things! on REG16/MEM16 (and on IMMED16 for TEST only)
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let reg_field = parsed.reg_field;
                let rm = self.decode_rm16_operand(memory, parsed, segment_override)?;
                // let immediate = self.fetch_next_byte_and_advance_ip(memory);
                let operand = self.read_operand16(memory, &rm)?;
                match reg_field {
                    // 0b000 => {
                    //     // TEST r/m 16, imm16
                    //     let immed16 = self.fetch_next_word_and_advance_ip(memory);
                    // }
                    // 0b010 => 0, // NOT r/m 16
                    // 0b011 => 0, // NEG r/m 16
                    0b100 => {
                        // MUL r/m 16
                        print_instruction(byte, &format_args!("mul {}", rm));
                        let ax = self.get_reg16(Reg16::Ax);
                        let value = (ax as u32) * (operand as u32);
                        self.dx = (value >> 16) as u16; // dx has the upper half
                        self.ax = (value & 0xFFFF) as u16; // ax holds the lower half
                        let upper_half_non_zero = self.dx != 0;
                        self.set_flag(Flag::Carry, upper_half_non_zero);
                        self.set_flag(Flag::Overflow, upper_half_non_zero);
                    }
                    // 0b101 => 0, // IMUL r/m 16
                    0b110 => {
                        // DIV r/m 16
                        print_instruction(byte, &format_args!("div {}", rm));
                        let dx = self.get_reg16(Reg16::Dx);
                        let ax = self.get_reg16(Reg16::Ax);
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
                    // 0b111 => 0, // IDIV r/m 16
                    _ => return Err(CpuError::InvalidRegisterValue(reg_field)),
                }
            }
            // Flag operations
            0xF8 => {
                // CLC
                print_instruction(byte, &format_args!("clc"));
                self.set_flag(Flag::Carry, false);
            }
            0xFA => {
                // CLI
                print_instruction(byte, &format_args!("cli"));
                self.set_flag(Flag::InterruptEnable, false);
            }
            0xFB => {
                // STI
                // TODO: When hardware IRQs are implemented, defer maskable interrupt delivery
                // until after the next instruction.
                print_instruction(byte, &format_args!("sti"));
                self.set_flag(Flag::InterruptEnable, true);
            }
            0xFC => {
                // CLD
                print_instruction(byte, &format_args!("cld"));
                self.set_flag(Flag::Direction, false);
            }
            // INC r/m8 or DEC r/m8
            0xFE => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let is_inc = match parsed.reg_field {
                    0 => true,
                    1 => false,
                    _ => return Err(CpuError::InvalidRegisterValue(parsed.reg_field)),
                };
                let rm = self.decode_rm8_operand(memory, parsed, segment_override)?;
                print_instruction(
                    byte,
                    &format_args!("{} {}", if is_inc { "inc" } else { "dec" }, rm),
                );
                let old_value = self.read_operand8(memory, &rm)?;
                let new_value = (old_value as i8).wrapping_add(if is_inc { 1 } else { -1 }) as u8;
                self.write_operand8(memory, &rm, new_value)?;

                self.set_result_flags8(new_value);
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
            _ => return Err(CpuError::UnsupportedInstruction(byte)),
        }
        self.trace();
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
        self.ip = read_word(memory, PhysAddr::new(0, ip_vec_offset))?;
        self.cs = read_word(memory, PhysAddr::new(0, cs_vec_offset))?;
        Ok(())
    }

    fn set_result_flags8(&mut self, result: u8) {
        self.set_flag(Flag::Zero, result == 0);
        self.set_flag(Flag::Sign, result & (1 << 7) != 0);
        self.set_flag(Flag::Parity, result.count_ones() % 2 == 0);
    }

    fn set_result_flags16(&mut self, result: u16) {
        self.set_flag(Flag::Zero, result == 0);
        // Is bit 15 set (i.e., negative)
        self.set_flag(Flag::Sign, result & (1 << 15) != 0);
        // Parity (even number of 1s in the lower byte)
        self.set_flag(Flag::Parity, (result as u8).count_ones() % 2 == 0);
    }

    fn push_onto_stack(&mut self, memory: &mut [u8], word: u16) -> Result<(), CpuError> {
        self.sp = self.sp.wrapping_sub(2);
        Ok(write_word(memory, PhysAddr::new(self.ss, self.sp), word)?)
    }

    fn pop_from_stack(&mut self, memory: &[u8]) -> Result<u16, CpuError> {
        let value = read_word(memory, PhysAddr::new(self.ss, self.sp))?;
        self.sp = self.sp.wrapping_add(2);
        Ok(value)
    }

    fn fetch_next_byte_and_advance_ip(&mut self, memory: &[u8]) -> u8 {
        let phys_addr = PhysAddr::new(self.cs, self.ip);
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
        self.set_result_flags8(result);

        result
    }

    fn subtract16(&mut self, lhs: u16, rhs: u16) -> u16 {
        let (result, borrow) = lhs.overflowing_sub(rhs);

        // Carry flag: did subtraction require a borrow?
        self.set_flag(Flag::Carry, borrow);
        // Overflow: (1) old_reg and operand have different signs, AND
        // (2) the result's sign differs from old_reg's sign
        self.set_flag(
            Flag::Overflow,
            ((lhs & 0x8000) != (rhs & 0x8000)) && ((result & 0x8000) != (lhs & 0x8000)),
        );
        self.set_flag(Flag::AuxiliaryCarry, (lhs & 0x0F) < (rhs & 0x0F));
        self.set_result_flags16(result);

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
        self.set_result_flags8(result);

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
        self.set_result_flags16(result);

        result
    }

    fn inc16(&mut self, instruction: u8, reg: Reg16) {
        print_instruction(instruction, &format_args!("inc {}", reg));
        let old_value = self.get_reg16(reg);
        let new_value = old_value.wrapping_add(1);
        self.set_reg16(reg, new_value);
        // Signed overflow: 0x7FFF became 0x8000, meaning +32767 became -32768
        self.set_flag(Flag::Overflow, old_value == 0x7FFF);
        // Was original low nibble 0b1111
        self.set_flag(Flag::AuxiliaryCarry, old_value & 0xF == 0xF);
        self.set_result_flags16(new_value);
    }

    fn dec16(&mut self, instruction: u8, reg: Reg16) {
        print_instruction(instruction, &format_args!("dec {}", reg));
        let old_value = self.get_reg16(reg);
        let new_value = old_value.wrapping_sub(1);
        self.set_reg16(reg, new_value);
        // Overflow: 0x8000 became 0x7FFF, meaning -32768 became +32767
        self.set_flag(Flag::Overflow, old_value == 0x8000);
        // Did decrementing borrowing into lower nibble (was it 0)
        self.set_flag(Flag::AuxiliaryCarry, old_value & 0x0F == 0);
        self.set_result_flags16(new_value);
    }

    pub(crate) fn trace(&self) {
        eprintln!("\x1b[37m{:?}\x1b[0m", self);
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

fn print_instruction(op: u8, instruction: &dyn fmt::Debug) {
    eprintln!(
        "\x1b[1m{:#04X}\x1b[0m \x1b[94m({:?})\x1b[0m",
        op, instruction
    );
}
