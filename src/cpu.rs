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
    #[error("Unsupported instruction: {0:X}")]
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

    fn get_segment_reg(&self, reg: SegmentReg) -> u16 {
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
        let byte = self.fetch_next_byte_and_advance_ip(memory);
        let mut result = CpuStepResult::Continue;
        // Table 4-13
        match byte {
            // Arithmetic
            0x08 => {
                // OR REG8/MEM8, REG8
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let source_reg = Reg8::try_from(parsed.reg_field)?;
                let rm = self.decode_rm8_operand(memory, parsed)?;
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
                let rm = self.decode_rm16_operand(memory, parsed)?;
                print_instruction(byte, &format_args!("or {}, {}", rm, source_reg));

                let reg_value = self.get_reg16(source_reg);
                let rm_value = self.read_operand16(memory, &rm)?;

                let value = reg_value | rm_value;
                self.write_operand16(memory, &rm, value)?;
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
                let rm = self.decode_rm16_operand(memory, parsed)?;
                print_instruction(byte, &format_args!("xor {}, {}", rm, source_reg));

                let reg_value = self.get_reg16(source_reg);
                let rm_value = self.read_operand16(memory, &rm)?;

                let value = reg_value ^ rm_value;
                self.write_operand16(memory, &rm, value)?;
                self.update_logic_flags16(value);
            }
            0x3A => {
                // CMP r8, r/m8
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let dest_reg = Reg8::try_from(parsed.reg_field)?;
                let rm = self.decode_rm8_operand(memory, parsed)?;
                print_instruction(byte, &format_args!("cmp {}, {}", dest_reg, rm));

                let rm_value = self.read_operand8(memory, &rm)?;
                self.subtract8(self.get_reg8(dest_reg), rm_value, false);
            }
            0x3B => {
                // CMP r16, r/m16
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let dest_reg = Reg16::try_from(parsed.reg_field)?;
                let rm = self.decode_rm16_operand(memory, parsed)?;
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
            0x46 => {
                print_instruction(byte, &format_args!("inc si"));
                let old_si = self.si;
                self.si = self.si.wrapping_add(1);
                // Signed overflow: 0x7FFF became 0x8000, meaning +32767 became -32768
                self.set_flag(Flag::Overflow, old_si == 0x7FFF);
                // Was original low nibble 0b1111
                self.set_flag(Flag::AuxiliaryCarry, old_si & 0xF == 0xF);
                self.set_result_flags16(self.si);
            }
            0x49 => {
                print_instruction(byte, &format_args!("dec cx"));
                let old_cx = self.cx;
                self.cx = self.cx.wrapping_sub(1);
                self.set_result_flags16(self.cx);
                // Overflow: 0x8000 became 0x7FFF, meaning -32768 became +32767
                self.set_flag(Flag::Overflow, old_cx == 0x8000);
                // Did decrementing borrowing into lower nibble (was it 0)
                self.set_flag(Flag::AuxiliaryCarry, old_cx & 0x0F == 0);
            }
            // PUSH/POP 50-5F
            0x50 => {
                print_instruction(byte, &format_args!("push ax"));
                self.push_onto_stack(memory, self.ax)?;
            }
            0x53 => {
                print_instruction(byte, &format_args!("push bx"));
                self.push_onto_stack(memory, self.bx)?;
            }
            0x58 => {
                print_instruction(byte, &format_args!("pop ax"));
                self.ax = self.pop_from_stack(memory)?;
            }
            0x5B => {
                print_instruction(byte, &format_args!("pop bx"));
                self.bx = self.pop_from_stack(memory)?;
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
                let rm = self.decode_rm8_operand(memory, parsed)?;
                let immediate = self.fetch_next_byte_and_advance_ip(memory);
                let destination = self.read_operand8(memory, &rm)?;
                let mut is_cmp = false;
                let value = match reg_field {
                    0 => self.add8(destination, immediate, false), // ADD
                    1 => {
                        // OR
                        let result = destination | immediate;
                        self.update_logic_flags8(result);
                        result
                    }
                    2 => self.add8(destination, immediate, self.get_flag(Flag::Carry)), // ADC
                    3 => self.subtract8(destination, immediate, self.get_flag(Flag::Carry)), // SBB
                    4 => {
                        // AND
                        let result = destination & immediate;
                        self.update_logic_flags8(result);
                        result
                    }
                    5 => self.subtract8(destination, immediate, false), // SUB
                    6 => {
                        // XOR
                        let result = destination ^ immediate;
                        self.update_logic_flags8(result);
                        result
                    }
                    7 => {
                        // CMP
                        is_cmp = true;
                        self.subtract8(destination, immediate, false)
                    }
                    _ => return Err(CpuError::InvalidRegisterValue(reg_field)),
                };
                if !is_cmp {
                    self.write_operand8(memory, &rm, value)?;
                }
            }
            0x86 => {
                // XCHG REG8,REG8/MEM8
                let parsed = parse_mod_rm(self.fetch_next_byte_and_advance_ip(memory));
                let first_reg = Reg8::try_from(parsed.reg_field)?;
                let rm = self.decode_rm8_operand(memory, parsed)?;
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
                let rm = self.decode_rm8_operand(memory, parsed)?;

                print_instruction(byte, &format_args!("mov {}, {}", rm, reg));

                let value = self.get_reg8(reg);
                self.write_operand8(memory, &rm, value)?;
            }
            0x89 => {
                // mov reg16/mem16, reg16
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let reg = Reg16::try_from(parsed.reg_field)?;
                let rm = self.decode_rm16_operand(memory, parsed)?;
                print_instruction(byte, &format_args!("mov {}, {}", rm, reg));
                let value = self.get_reg16(reg);
                self.write_operand16(memory, &rm, value)?;
            }
            0x8A => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let reg = Reg8::try_from(parsed.reg_field)?;
                let rm = self.decode_rm8_operand(memory, parsed)?;
                print_instruction(byte, &format_args!("mov {}, {}", reg, rm));
                let value = self.read_operand8(memory, &rm)?;
                self.set_reg8(reg, value);
            }
            0x8C => {
                // MOV r/m16, segment register
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let source_segment = SegmentReg::try_from(parsed.reg_field)?;
                let rm = self.decode_rm16_operand(memory, parsed)?;
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
                let rm = self.decode_rm16_operand(memory, parsed)?;
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
                let rm = self.decode_rm16_operand(memory, parsed)?;
                print_instruction(byte, &format_args!("mov {}, {}", dest_segment, rm));

                let value = self.read_operand16(memory, &rm)?;
                self.set_segment_reg(dest_segment, value);
            }
            0xAC => {
                // LODSB
                print_instruction(byte, &format_args!("lodsb"));
                let byte = read_byte(memory, PhysAddr::new(self.ds, self.si))?;
                self.set_reg8(Reg8::Al, byte);
                if self.get_flag(Flag::Direction) {
                    self.si = self.si.wrapping_sub(1);
                } else {
                    self.si = self.si.wrapping_add(1);
                }
            }
            // B0-B7: move an 8-bit immediate into an 8-bit register
            0xB0 => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("mov al, {:X}h", operand));
                self.set_reg8(Reg8::Al, operand);
            }
            0xB4 => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("mov ah, {:X}h", operand));
                self.set_reg8(Reg8::Ah, operand);
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
                let rm = self.decode_rm8_operand(memory, parsed)?;
                let immediate = self.fetch_next_byte_and_advance_ip(memory);

                print_instruction(byte, &format_args!("mov {}, {}", rm, immediate));
                self.write_operand8(memory, &rm, immediate)?;
            }
            // Interrupts
            0xCD => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("int {:X}h", operand));
                result = CpuStepResult::Interrupt(operand);
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
            // CLD
            0xFC => {
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
                let rm = self.decode_rm8_operand(memory, parsed)?;
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

    pub(crate) fn trace(&self) {
        eprintln!("{:?}", self);
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
    eprintln!("{:#X} \x1b[96m({:?})\x1b[0m", op, instruction);
}
