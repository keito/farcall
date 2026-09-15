use core::fmt::{self, Debug};
use std::fs;
use std::io::{self, Write};
use thiserror::Error;

#[cfg(test)]
mod tests;

pub struct Cpu {
    ax: u16,
    bx: u16,
    cx: u16,
    dx: u16,
    si: u16,
    ip: u16,
    flags: u16,
}

// Owns everything
pub struct Machine {
    cpu: Cpu,
    memory: Vec<u8>,
}

#[derive(Error, Debug)]
enum CpuError {
    #[error("Unsupported instruction: {0:X}")]
    UnsupportedInstruction(u8),
    #[error("Out of bounds memory access: {0}")]
    OutOfBoundsMemoryAccess(String),
}

enum CpuStepResult {
    Continue,
    Interrupt(u8),
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
enum Flag {
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

impl Cpu {
    fn new() -> Self {
        Cpu {
            ax: 0x1234,
            bx: 0x1234,
            cx: 0x1234,
            dx: 0x1234,
            si: 0x1234,
            ip: 0x0,
            flags: 0,
        }
    }

    fn set_flag(&mut self, flag: Flag, value: bool) {
        let mask = 1 << (flag as u8);
        self.flags = (self.flags & !mask) | ((value as u16) << (flag as u8));
    }

    fn set_reg8(&mut self, reg: Reg8, value: u8) {
        match reg {
            Reg8::Al => self.ax = (self.ax & 0xFF00) | value as u16,
            Reg8::Bl => self.bx = (self.bx & 0xFF00) | value as u16,
            Reg8::Cl => self.cx = (self.cx & 0xFF00) | value as u16,
            Reg8::Dl => self.dx = (self.dx & 0xFF00) | value as u16,
            _ => todo!("Not implemented arm of set_reg8"),
        }
    }

    /// Fetch, decode, and execute one instruction
    fn step(&mut self, memory: &mut [u8]) -> Result<CpuStepResult, CpuError> {
        let byte = self.fetch_next_byte_and_advance_ip(memory);
        let mut result = CpuStepResult::Continue;
        // Table 4-13
        match byte {
            // Arithmetic
            0x2C => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("sub al, {:X}h", operand));
                let old_al = lower_byte(self.ax);
                let new_al = old_al.wrapping_sub(operand);
                self.set_reg8(Reg8::Al, new_al);

                // Carry flag: did subtraction require a borrow?
                self.set_flag(Flag::Carry, old_al < operand);
                // Set or clear the zero flag
                self.set_flag(Flag::Zero, new_al == 0);
                // Overflow: (1) old_al and operand have different signs, AND
                // (2) the result's sign differs from old_al's sign
                self.set_flag(
                    Flag::Overflow,
                    ((old_al & 0x80) != (operand & 0x80)) && ((new_al & 0x80) != (old_al & 0x80)),
                );
                self.set_flag(Flag::Sign, new_al & (1 << 7) != 0);
                self.set_flag(Flag::Parity, new_al.count_ones() % 2 == 0);
                self.set_flag(Flag::AuxiliaryCarry, (old_al & 0x0F) < (operand & 0x0F));
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
            // Jumps
            0x75 => {
                // Signed byte
                let operand = self.fetch_next_byte_and_advance_ip(memory) as i8;
                print_instruction(byte, &format_args!("jnz short {:X}h", operand));
                // If zero flag is not set, then jump
                if self.flags & 0x40 == 0 {
                    self.ip = self.ip.wrapping_add_signed(operand as i16);
                }
            }
            // MOVs
            0x88 => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let mod_field = operand >> 6;
                let reg_field = (operand & 0x3F) >> 3;
                let rm_field = operand & 0x7;

                if mod_field != 0 {
                    // Unsupported mod
                    todo!("mods other than 00 not currently supported for 88");
                }
                let source = match reg_field {
                    0b000 => Reg8::Al,
                    _ => todo!("Sources other than 0b000 (AL) not supported for 88"),
                };
                if rm_field != 0b100 {
                    // Unsupported RM
                    todo!("r/m other than 0b100 ([SI]) not supported for 88");
                }

                print_instruction(byte, &format_args!("mov [si], {}", source));

                // Read the value in AL
                let value = lower_byte(self.ax);
                // Make sure we are not out of bounds
                if let Some(si_mut) = memory.get_mut(self.si as usize) {
                    *si_mut = value;
                } else {
                    return Err(CpuError::OutOfBoundsMemoryAccess("[si]".to_string()));
                }
            }
            0x8A => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                let mod_field = operand >> 6;
                let reg_field = (operand & 0x3F) >> 3;
                let rm_field = operand & 0x7;

                if mod_field != 0 {
                    // Unsupported mod
                    todo!("mods other than 00 not currently supported for 8A");
                }
                let dest = match reg_field {
                    0b000 => Reg8::Al,
                    _ => todo!("Destinations other than 0b000 (AL) not supported for 8A"),
                };
                if rm_field != 0b100 {
                    // Unsupported RM
                    todo!("r/m other than 0b100 ([SI]) not supported for 8A");
                }

                print_instruction(byte, &format_args!("mov {}, [si]", dest));

                // Read the value at SI
                if let Some(data) = memory.get(self.si as usize) {
                    self.ax = *data as u16 | (self.ax & 0xFF00);
                } else {
                    return Err(CpuError::OutOfBoundsMemoryAccess("[si]".to_string()));
                }
            }
            // B0-B7: move an 8-bit immediate into an 8-bit register
            0xB0 => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("mov al, {:X}h", operand));
                // Move the u8 operand into the low byte of AX
                self.ax = operand as u16 | (self.ax & 0xFF00);
            }
            0xB4 => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("mov ah, {:X}h", operand));
                // Move the u8 operand into the high byte of AX
                self.ax = ((operand as u16) << 8) | (self.ax & 0b11111111);
            }
            // B8-BF: move a 16-bit immediate into a 16-bit register
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
            0xBE => {
                let operand = self.fetch_next_word_and_advance_ip(memory);
                print_instruction(byte, &format_args!("mov si, {:X}h", operand));
                self.si = operand;
            }
            0xB9 => {
                let operand = self.fetch_next_word_and_advance_ip(memory);
                print_instruction(byte, &format_args!("mov cx, {:X}h", operand));
                self.cx = operand;
            }
            // Interrupts
            0xCD => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, &format_args!("int {:X}h", operand));
                result = CpuStepResult::Interrupt(operand);
            }
            _ => return Err(CpuError::UnsupportedInstruction(byte)),
        }
        self.trace();
        Ok(result)
    }

    fn fetch_next_byte_and_advance_ip(&mut self, memory: &[u8]) -> u8 {
        let byte = memory[self.ip as usize];
        self.ip += 1;
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

    fn trace(&self) {
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
            .field("ip", &format_args!("{:#06X}", self.ip))
            .field("flags", &format_args!("{:09b}", self.flags))
            .finish()
    }
}

#[derive(Error, Debug)]
enum MachineError {
    #[error("Not enough memory; offset {0} exceeds {1} bytes")]
    NotEnoughMemory(usize, usize),
    #[error("Unsupported interrupt: {0:X}")]
    UnsupportedInterrupt(u8),
    #[error("Unsupported service: {0:X}")]
    UnsupportedService(u8),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Write Service error: {0}")]
    WriteServiceError(String),
}

enum InterruptResult {
    ResumeExecution,
    Exit(u8),
}

enum MachineStepResult {
    Continue,
    Exit(u8),
}

impl Machine {
    /// Emulates the DOS write service
    fn write_service(&mut self) -> Result<InterruptResult, MachineError> {
        let num_bytes = self.cpu.cx as usize;
        let start_pos = self.cpu.dx as usize;
        let dest = self.cpu.bx;
        if dest != 1 {
            return Err(MachineError::WriteServiceError(
                "Unsupported destination in bx".to_string(),
            ));
        }

        if let Some(buffer) = self.memory.get(start_pos..start_pos + num_bytes) {
            io::stdout().write_all(b"\x1b[92m")?;
            io::stdout().write_all(buffer)?;
            io::stdout().write_all(b"\x1b[0m")?;
            // Successfully written bytes
            self.cpu.ax = num_bytes as u16;
            // Clear the carry flag (least significant bit of `flags`)
            self.cpu.set_flag(Flag::Carry, false);
            Ok(InterruptResult::ResumeExecution)
        } else {
            Err(MachineError::WriteServiceError(
                "Buffer out of bounds".to_string(),
            ))
        }
    }

    /// Emulates the DOS termination service
    fn termination_service(&mut self) -> Result<InterruptResult, MachineError> {
        Ok(InterruptResult::Exit((self.cpu.ax & 0xFF) as u8))
    }

    fn handle_interrupt(&mut self, interrupt: u8) -> Result<InterruptResult, MachineError> {
        match interrupt {
            0x21 => {
                // AH contains the service ID
                let service = ((self.cpu.ax & 0xFF00) >> 8) as u8;
                match service {
                    0x40 => self.write_service(),
                    0x4C => self.termination_service(),
                    _ => Err(MachineError::UnsupportedService(service)),
                }
            }
            _ => Err(MachineError::UnsupportedInterrupt(interrupt)),
        }
    }

    fn step(&mut self) -> anyhow::Result<MachineStepResult> {
        let result = self.cpu.step(&mut self.memory)?;
        match result {
            CpuStepResult::Continue => Ok(MachineStepResult::Continue),
            CpuStepResult::Interrupt(interrupt) => {
                eprintln!("Interrupted: {interrupt:X}");
                let interrupt_result = self.handle_interrupt(interrupt)?;
                self.cpu.trace();
                match interrupt_result {
                    InterruptResult::ResumeExecution => Ok(MachineStepResult::Continue),
                    InterruptResult::Exit(code) => Ok(MachineStepResult::Exit(code)),
                }
            }
        }
    }
}

pub fn main_loop(file_path: &str) -> anyhow::Result<u8> {
    let memory_size = 512;
    let begin_offset = 0x100;
    let mut machine = Machine {
        cpu: Cpu::new(),
        memory: vec![0; memory_size],
    };

    let data: Vec<u8> = fs::read(file_path)?;
    // Load binary into memory at offset 0x100
    if begin_offset + data.len() > memory_size {
        return Err(MachineError::NotEnoughMemory(begin_offset + data.len(), memory_size).into());
    }
    machine.memory[begin_offset..begin_offset + data.len()].copy_from_slice(&data);
    // Start executing at begin_offset
    machine.cpu.ip = begin_offset as u16;

    machine.cpu.trace();
    loop {
        let result = machine.step()?;
        if let MachineStepResult::Exit(code) = result {
            return Ok(code);
        }
    }
}

fn print_instruction(op: u8, instruction: &dyn fmt::Debug) {
    eprintln!("{:#X} \x1b[96m({:?})\x1b[0m", op, instruction);
}

fn lower_byte(word: u16) -> u8 {
    (word & 0xFF) as u8
}
