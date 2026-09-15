use std::fmt::{self, Display};

use crate::registers::{Reg8, Reg16, SegmentReg};

use super::{Cpu, CpuError};

#[derive(Clone)]
pub(crate) enum RegisterOrMemoryOperand<T> {
    Register(T),
    Memory(SegmentReg, u16),
}

impl<T: Display> Display for RegisterOrMemoryOperand<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Register(reg) => write!(f, "{}", reg),
            Self::Memory(seg_reg, addr) => {
                write!(f, "[{:}:{:#X}]", seg_reg, addr)
            }
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
    fn effective_address_base(&self, rm_field: u8) -> Result<(SegmentReg, u16), CpuError> {
        match rm_field {
            0b000 => Ok((SegmentReg::Ds, self.bx.wrapping_add(self.si))),
            0b001 => Ok((SegmentReg::Ds, self.bx.wrapping_add(self.di))),
            0b010 => Ok((SegmentReg::Ss, self.bp.wrapping_add(self.si))),
            0b011 => Ok((SegmentReg::Ss, self.bp.wrapping_add(self.di))),
            0b100 => Ok((SegmentReg::Ds, self.si)),
            0b101 => Ok((SegmentReg::Ds, self.di)),
            0b110 => Ok((SegmentReg::Ss, self.bp)),
            0b111 => Ok((SegmentReg::Ds, self.bx)),
            _ => Err(CpuError::UnsupportedRmField(rm_field)),
        }
    }

    fn decode_effective_address(
        &mut self,
        memory: &[u8],
        fields: ModRmFields,
    ) -> Result<(SegmentReg, u16), CpuError> {
        match fields.mod_field {
            0b00 => {
                // Memory without displacement
                if fields.rm_field == 0b110 {
                    // Special case for direct memory [disp16]
                    let word = self.fetch_next_word_and_advance_ip(memory);
                    Ok((SegmentReg::Ds, word))
                } else {
                    self.effective_address_base(fields.rm_field)
                }
            }
            0b01 => {
                // Memory with an 8-bit displacement
                let (seg_reg, base_address) = self.effective_address_base(fields.rm_field)?;
                let displacement = self.fetch_next_byte_and_advance_ip(memory);
                Ok((
                    seg_reg,
                    base_address.wrapping_add_signed((displacement as i8) as i16),
                ))
            }
            0b10 => {
                // Memory with a 16-bit displacement
                let (seg_reg, base_address) = self.effective_address_base(fields.rm_field)?;
                let displacement = self.fetch_next_word_and_advance_ip(memory);
                Ok((
                    seg_reg,
                    base_address.wrapping_add_signed(displacement as i16),
                ))
            }
            0b11 => unreachable!("Caller should handle 0b11"),
            _ => Err(CpuError::UnsupportedModField(fields.mod_field)),
        }
    }

    pub(super) fn decode_rm8_operand(
        &mut self,
        memory: &[u8],
        fields: ModRmFields,
    ) -> Result<RegisterOrMemoryOperand<Reg8>, CpuError> {
        match fields.mod_field {
            0b00 | 0b01 | 0b10 => {
                let (seg_reg, addr) = self.decode_effective_address(memory, fields)?;
                Ok(RegisterOrMemoryOperand::Memory(seg_reg, addr))
            }
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
            0b00 | 0b01 | 0b10 => {
                let (seg_reg, addr) = self.decode_effective_address(memory, fields)?;
                Ok(RegisterOrMemoryOperand::Memory(seg_reg, addr))
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
