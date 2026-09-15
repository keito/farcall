use crate::memory::PhysAddr;
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
    #[error("Out of bounds memory access: {0}")]
    OutOfBoundsMemoryAccess(String),
    #[error("Unsupported mod field: {0}")]
    UnsupportedModField(u8),
    #[error("Invalid register value: {0}")]
    InvalidRegisterValue(u8),
    #[error("Unsupported r/m field: {0}")]
    UnsupportedRmField(u8),
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

    fn get_reg8(&self, reg: Reg8) -> u8 {
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

    fn get_reg16(&self, reg: Reg16) -> u16 {
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

    /// Fetch, decode, and execute one instruction
    pub(crate) fn step(&mut self, memory: &mut [u8]) -> Result<CpuStepResult, CpuError> {
        let byte = self.fetch_next_byte_and_advance_ip(memory);
        let mut result = CpuStepResult::Continue;
        // Table 4-13
        match byte {
            // Arithmetic
            0x2C => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("sub al, {:X}h", operand));
                let new_val = self.subtract_immediate8(Reg8::Al, operand);
                self.set_reg8(Reg8::Al, new_val);
            }
            0x3C => {
                // CMP AL - basically SUB AL that doesn't change AL
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("cmp al, {:X}h", operand));
                self.subtract_immediate8(Reg8::Al, operand);
            }
            0x46 => {
                print_instruction(byte, &format_args!("inc si"));
                let old_si = self.si;
                self.si = self.si.wrapping_add(1);
                // Signed overflow: 0x7FFF became 0x8000, meaning +32767 became -32768
                self.set_flag(Flag::Overflow, old_si == 0x7FFF);
                // Was original low nibble 0b1111
                self.set_flag(Flag::AuxiliaryCarry, old_si & 0xF == 0xF);
                self.set_flag(Flag::Zero, self.si == 0);
                self.set_flag(Flag::Sign, self.si & (1 << 15) != 0);
                self.set_flag(Flag::Parity, (self.si as u8).count_ones() % 2 == 0);
            }
            0x49 => {
                print_instruction(byte, &format_args!("dec cx"));
                let old_cx = self.cx;
                self.cx = self.cx.wrapping_sub(1);
                // Set or clear the zero flag
                self.set_flag(Flag::Zero, self.cx == 0);
                // Overflow: 0x8000 became 0x7FFF, meaning -32768 became +32767
                self.set_flag(Flag::Overflow, old_cx == 0x8000);
                // Is bit 15 set (i.e., negative)
                self.set_flag(Flag::Sign, self.cx & (1 << 15) != 0);
                // Parity (even number of 1s in the lower byte)
                self.set_flag(Flag::Parity, (self.cx as u8).count_ones() % 2 == 0);
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
            // MOVs
            0x88 => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let reg = Reg8::try_from(parsed.reg_field)?;
                let rm = self.decode_rm8_operand(memory, parsed)?;

                print_instruction(byte, &format_args!("mov {}, {}", rm, reg));

                // Read the value in the register
                let value = self.get_reg8(reg);
                match rm {
                    RegisterOrMemoryOperand::Memory(seg_reg, addr) => {
                        let seg_addr = self.get_segment_reg(seg_reg);
                        write_byte(memory, PhysAddr::new(seg_addr, addr), value)?
                    }
                    RegisterOrMemoryOperand::Register(reg) => self.set_reg8(reg, value),
                }
            }
            0x8A => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let parsed = parse_mod_rm(operand);
                let reg = Reg8::try_from(parsed.reg_field)?;
                let rm = self.decode_rm8_operand(memory, parsed)?;

                print_instruction(byte, &format_args!("mov {}, {}", reg, rm));
                let value = match rm {
                    RegisterOrMemoryOperand::Memory(seg_reg, addr) => {
                        let seg_addr = self.get_segment_reg(seg_reg);
                        read_byte(memory, PhysAddr::new(seg_addr, addr))?
                    }
                    RegisterOrMemoryOperand::Register(reg) => self.get_reg8(reg),
                };
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
                match rm {
                    RegisterOrMemoryOperand::Memory(seg_reg, addr) => {
                        let seg_addr = self.get_segment_reg(seg_reg);
                        write_word(memory, PhysAddr::new(seg_addr, addr), value)?
                    }
                    RegisterOrMemoryOperand::Register(dest_reg) => self.set_reg16(dest_reg, value),
                }
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

                let value = match rm {
                    RegisterOrMemoryOperand::Memory(seg_reg, addr) => {
                        let seg_addr = self.get_segment_reg(seg_reg);
                        read_word(memory, PhysAddr::new(seg_addr, addr))?
                    }
                    RegisterOrMemoryOperand::Register(reg) => self.get_reg16(reg),
                };
                self.set_segment_reg(dest_segment, value);
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
                // Push the current IP - it now points after the entire CALL instruction
                self.push_onto_stack(memory, self.ip)?;
                // Add the displacement to IP with 16-bit wrapping
                self.ip = self.ip.wrapping_add(displacement)
            }
            _ => return Err(CpuError::UnsupportedInstruction(byte)),
        }
        self.trace();
        Ok(result)
    }

    fn push_onto_stack(&mut self, memory: &mut [u8], word: u16) -> Result<(), CpuError> {
        self.sp = self.sp.wrapping_sub(2);
        write_word(memory, PhysAddr::new(self.ss, self.sp), word)
    }

    fn pop_from_stack(&mut self, memory: &[u8]) -> Result<u16, CpuError> {
        let value = read_word(memory, PhysAddr::new(self.ss, self.sp))?;
        self.sp = self.sp.wrapping_add(2);
        Ok(value)
    }

    fn fetch_next_byte_and_advance_ip(&mut self, memory: &[u8]) -> u8 {
        let byte = memory[self.ip as usize];
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

    /// Implements shared functionality for subtracting an 8-bit immediate
    /// from an 8-bit register and setting the correct flags.
    fn subtract_immediate8(&mut self, reg: Reg8, operand: u8) -> u8 {
        let old_reg = self.get_reg8(reg);
        let (result, borrow) = old_reg.overflowing_sub(operand);

        // Carry flag: did subtraction require a borrow?
        self.set_flag(Flag::Carry, borrow);
        // Set or clear the zero flag
        self.set_flag(Flag::Zero, result == 0);
        // Overflow: (1) old_reg and operand have different signs, AND
        // (2) the result's sign differs from old_reg's sign
        self.set_flag(
            Flag::Overflow,
            ((old_reg & 0x80) != (operand & 0x80)) && ((result & 0x80) != (old_reg & 0x80)),
        );
        self.set_flag(Flag::Sign, result & (1 << 7) != 0);
        self.set_flag(Flag::Parity, result.count_ones() % 2 == 0);
        self.set_flag(Flag::AuxiliaryCarry, (old_reg & 0x0F) < (operand & 0x0F));

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

fn write_byte(memory: &mut [u8], address: PhysAddr, value: u8) -> Result<(), CpuError> {
    let Some(mem_mut) = memory.get_mut(address.as_usize()) else {
        return Err(CpuError::OutOfBoundsMemoryAccess(format!("{:#X}", address)));
    };
    *mem_mut = value;
    Ok(())
}

fn read_byte(memory: &[u8], address: PhysAddr) -> Result<u8, CpuError> {
    let Some(data) = memory.get(address.as_usize()) else {
        return Err(CpuError::OutOfBoundsMemoryAccess(format!("{:#X}", address)));
    };
    Ok(*data)
}

fn calc_word_addr(memory: &[u8], address: PhysAddr) -> Result<(usize, usize), CpuError> {
    let byte1 = address.as_usize();
    // The second byte might wrap
    let byte2 = (byte1 + 1) & 0xFFFFF;
    if memory.get(byte1).is_none() || memory.get(byte2).is_none() {
        return Err(CpuError::OutOfBoundsMemoryAccess(format!("{:#X}", address)));
    };
    Ok((byte1, byte2))
}

fn write_word(memory: &mut [u8], address: PhysAddr, value: u16) -> Result<(), CpuError> {
    let (byte1, byte2) = calc_word_addr(memory, address)?;
    let [low, high] = value.to_le_bytes();
    memory[byte1] = low;
    memory[byte2] = high;
    Ok(())
}

fn read_word(memory: &[u8], address: PhysAddr) -> Result<u16, CpuError> {
    let (byte1, byte2) = calc_word_addr(memory, address)?;
    Ok(u16::from_le_bytes([memory[byte1], memory[byte2]]))
}
