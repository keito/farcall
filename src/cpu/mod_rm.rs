use std::fmt::{self, Display};

use crate::registers::{Reg8, Reg16};

use super::{Cpu, CpuError};

pub(crate) enum RegisterOrMemoryOperand<T> {
    Register(T),
    Memory(u16),
}

impl<T: Display> Display for RegisterOrMemoryOperand<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Register(reg) => write!(f, "{}", reg),
            Self::Memory(addr) => write!(f, "[{:#X}]", addr),
        }
    }
}

pub(super) struct ModRmFields {
    pub(super) mod_field: u8,
    pub(super) reg_field: u8,
    pub(super) rm_field: u8,
}

pub(super) fn parse_mod_rm(operand: u8) -> ModRmFields {
    ModRmFields {
        mod_field: operand >> 6,
        reg_field: (operand & 0x3F) >> 3,
        rm_field: operand & 0x7,
    }
}

impl Cpu {
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

    pub(super) fn decode_rm8_operand(
        &mut self,
        memory: &[u8],
        fields: ModRmFields,
    ) -> Result<RegisterOrMemoryOperand<Reg8>, CpuError> {
        match fields.mod_field {
            0b00 => {
                // Memory without displacement
                if fields.rm_field == 0b110 {
                    // Exception for direct memory [disp16]
                    todo!("[disp16]");
                }
                let base_address = self.effective_address_base(fields.rm_field)?;
                Ok(RegisterOrMemoryOperand::Memory(base_address))
            }
            0b01 => {
                // Memory with an 8-bit displacement
                let base_address = self.effective_address_base(fields.rm_field)?;
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
                let rm_reg = Reg8::try_from(fields.rm_field)?;
                Ok(RegisterOrMemoryOperand::Register(rm_reg))
            }
            _ => Err(CpuError::UnsupportedModField(fields.mod_field)),
        }
    }

    pub(super) fn decode_rm16_operand(
        &mut self,
        memory: &[u8],
        fields: ModRmFields,
    ) -> Result<RegisterOrMemoryOperand<Reg16>, CpuError> {
        match fields.mod_field {
            0b00 => {
                if fields.rm_field == 0b110 {
                    // Special case where we fetch the next word and return Memory(offset)
                    let word = self.fetch_next_word_and_advance_ip(memory);
                    Ok(RegisterOrMemoryOperand::Memory(word))
                } else {
                    Err(CpuError::UnsupportedRmField(fields.rm_field))
                }
            }
            0b11 => {
                // Register
                let rm_reg = Reg16::try_from(fields.rm_field)?;
                Ok(RegisterOrMemoryOperand::Register(rm_reg))
            }
            _ => Err(CpuError::UnsupportedModField(fields.mod_field)),
        }
    }
}
