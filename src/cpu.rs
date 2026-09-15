use core::fmt::{self, Debug};
use std::fmt::Display;
use thiserror::Error;

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

#[allow(dead_code)]
enum Reg8 {
    Al = 0b000,
    Cl = 0b001,
    Dl = 0b010,
    Bl = 0b011,
    Ah = 0b100,
    Ch = 0b101,
    Dh = 0b110,
    Bh = 0b111,
}

impl fmt::Display for Reg8 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reg8::Al => write!(f, "al"),
            Reg8::Cl => write!(f, "cl"),
            Reg8::Dl => write!(f, "dl"),
            Reg8::Bl => write!(f, "bl"),
            Reg8::Ah => write!(f, "ah"),
            Reg8::Ch => write!(f, "ch"),
            Reg8::Dh => write!(f, "dh"),
            Reg8::Bh => write!(f, "bh"),
        }
    }
}

impl TryFrom<u8> for Reg8 {
    type Error = CpuError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0b000 => Ok(Reg8::Al),
            0b001 => Ok(Reg8::Cl),
            0b010 => Ok(Reg8::Dl),
            0b011 => Ok(Reg8::Bl),
            0b100 => Ok(Reg8::Ah),
            0b101 => Ok(Reg8::Ch),
            0b110 => Ok(Reg8::Dh),
            0b111 => Ok(Reg8::Bh),
            _ => Err(CpuError::InvalidRegisterValue(value)),
        }
    }
}

impl Cpu {
    pub(crate) fn new() -> Self {
        Cpu {
            ax: 0x1234,
            bx: 0x1234,
            cx: 0x1234,
            dx: 0x1234,
            si: 0x1234,
            di: 0x1234,
            sp: 0x1234,
            bp: 0x1234,
            ip: 0x0,
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

    fn set_reg8(&mut self, reg: Reg8, value: u8) {
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
            Reg8::Al => self.ax & 0x00FF,
            Reg8::Bl => self.bx & 0x00FF,
            Reg8::Cl => self.cx & 0x00FF,
            Reg8::Dl => self.dx & 0x00FF,
            Reg8::Ah => (self.ax & 0xFF00) >> 8,
            Reg8::Bh => (self.bx & 0xFF00) >> 8,
            Reg8::Ch => (self.cx & 0xFF00) >> 8,
            Reg8::Dh => (self.dx & 0xFF00) >> 8,
        }) as u8
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
                let new_val = self.sub_imm8(Reg8::Al, operand);
                self.set_reg8(Reg8::Al, new_val);
            }
            0x3C => {
                // CMP AL - basically SUB AL that doesn't change AL
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("cmp al, {:X}h", operand));
                self.sub_imm8(Reg8::Al, operand);
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
            0x75 => {
                // Signed byte
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
            // MOVs
            0x88 => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let decoded = self.decode_mod_r_m8(memory, operand)?;

                print_instruction(byte, &format_args!("mov {}, {}", decoded.rm, decoded.reg));

                // Read the value in the register
                let value = self.get_reg8(decoded.reg);
                match decoded.rm {
                    RegisterOrMemoryOperand::Memory(memory_addr) => {
                        if let Some(mem_mut) = memory.get_mut(memory_addr as usize) {
                            *mem_mut = value;
                        } else {
                            return Err(CpuError::OutOfBoundsMemoryAccess(format!(
                                "Address {:#X}",
                                memory_addr
                            )));
                        }
                    }
                    RegisterOrMemoryOperand::Register(reg) => self.set_reg8(reg, value),
                }
            }
            0x8A => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let decoded = self.decode_mod_r_m8(memory, operand)?;

                print_instruction(byte, &format_args!("mov {}, {}", decoded.reg, decoded.rm));
                let value = match decoded.rm {
                    RegisterOrMemoryOperand::Memory(memory_addr) => {
                        if let Some(data) = memory.get(memory_addr as usize) {
                            *data
                        } else {
                            return Err(CpuError::OutOfBoundsMemoryAccess(format!(
                                "Address {:#X}",
                                memory_addr
                            )));
                        }
                    }
                    RegisterOrMemoryOperand::Register(reg) => self.get_reg8(reg),
                };
                self.set_reg8(decoded.reg, value);
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
        if let Some(mem_mut) = memory.get_mut(self.sp as usize..self.sp as usize + 2) {
            mem_mut[0] = word as u8;
            mem_mut[1] = (word >> 8) as u8;
            Ok(())
        } else {
            return Err(CpuError::OutOfBoundsMemoryAccess(
                "push_onto_stack".to_string(),
            ));
        }
    }

    fn pop_from_stack(&mut self, memory: &mut [u8]) -> Result<u16, CpuError> {
        let value;
        if let Some(mem) = memory.get(self.sp as usize..self.sp as usize + 2) {
            value = (mem[0] as u16) | ((mem[1] as u16) << 8);
        } else {
            return Err(CpuError::OutOfBoundsMemoryAccess(
                "pop_from_stack".to_string(),
            ));
        }
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
        // Combine the low and high bytes into a 16-bit value
        (lob as u16) | ((hob as u16) << 8)
    }

    fn sub_imm8(&mut self, reg: Reg8, operand: u8) -> u8 {
        let old_reg = self.get_reg8(reg);
        let new_al = old_reg.wrapping_sub(operand);

        // Carry flag: did subtraction require a borrow?
        self.set_flag(Flag::Carry, old_reg < operand);
        // Set or clear the zero flag
        self.set_flag(Flag::Zero, new_al == 0);
        // Overflow: (1) old_reg and operand have different signs, AND
        // (2) the result's sign differs from old_reg's sign
        self.set_flag(
            Flag::Overflow,
            ((old_reg & 0x80) != (operand & 0x80)) && ((new_al & 0x80) != (old_reg & 0x80)),
        );
        self.set_flag(Flag::Sign, new_al & (1 << 7) != 0);
        self.set_flag(Flag::Parity, new_al.count_ones() % 2 == 0);
        self.set_flag(Flag::AuxiliaryCarry, (old_reg & 0x0F) < (operand & 0x0F));

        new_al
    }

    pub(crate) fn trace(&self) {
        eprintln!("{:?}", self);
    }

    fn effective_address_base(&self, rm_field: u8) -> Result<u16, CpuError> {
        match rm_field {
            // 0b000 => {} // BX+SI
            // 0b001 => {} // BX+DI
            // 0b010 => {} // BP+SI
            // 0b011 => {} // BP+DI
            0b100 => Ok(self.si),
            0b101 => Ok(self.di),
            // 0b110 => {} // BP (except in exception handled separately)
            // 0b111 => {} // BX
            _ => Err(CpuError::UnsupportedRmField(rm_field)),
        }
    }

    fn decode_mod_r_m8(&mut self, memory: &[u8], operand: u8) -> Result<DecodedModRm8, CpuError> {
        let mod_field = operand >> 6;
        let reg_field = (operand & 0x3F) >> 3;
        let rm_field = operand & 0x7;

        let reg = Reg8::try_from(reg_field)?;
        let rm = match mod_field {
            0b00 => {
                // Memory without displacement
                if rm_field == 0b110 {
                    // Exception for direct memory [disp16]
                    todo!("[disp16]");
                }
                let base_address = self.effective_address_base(rm_field)?;
                Ok(RegisterOrMemoryOperand::Memory(base_address))
            }
            0b01 => {
                // Memory with an 8-bit displacement
                let base_address = self.effective_address_base(rm_field)?;
                // Get displacement
                let displacement = self.fetch_next_byte_and_advance_ip(memory);
                Ok(RegisterOrMemoryOperand::Memory(
                    base_address.wrapping_add_signed((displacement as i8) as i16),
                ))
            }
            // 0b10 => {
            //     // Memory with a 16-bit displacement
            //     panic!("I'm so sorry")
            // }
            0b11 => {
                // Register
                let rm_reg = Reg8::try_from(rm_field)?;
                Ok(RegisterOrMemoryOperand::Register(rm_reg))
            }
            _ => Err(CpuError::UnsupportedModField(mod_field)),
        }?;
        Ok(DecodedModRm8 { reg, rm })
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
            .field("flags", &format_args!("{:09b}", self.flags))
            .finish()
    }
}

fn print_instruction(op: u8, instruction: &dyn fmt::Debug) {
    eprintln!("{:#X} \x1b[96m({:?})\x1b[0m", op, instruction);
}

enum RegisterOrMemoryOperand {
    Register(Reg8),
    Memory(u16),
}

impl Display for RegisterOrMemoryOperand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Register(reg) => write!(f, "{}", reg),
            Self::Memory(addr) => write!(f, "[{:#X}]", addr),
        }
    }
}

struct DecodedModRm8 {
    reg: Reg8,
    rm: RegisterOrMemoryOperand,
}
