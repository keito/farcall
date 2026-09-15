use crate::cpu::CpuError;
use std::fmt;

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub(crate) enum Reg8 {
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

#[allow(dead_code)]
pub(crate) enum Reg16 {
    Ax = 0b000,
    Cx = 0b001,
    Dx = 0b010,
    Bx = 0b011,
    Sp = 0b100,
    Bp = 0b101,
    Si = 0b110,
    Di = 0b111,
}

impl fmt::Display for Reg16 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reg16::Ax => write!(f, "ax"),
            Reg16::Cx => write!(f, "cx"),
            Reg16::Dx => write!(f, "dx"),
            Reg16::Bx => write!(f, "bx"),
            Reg16::Sp => write!(f, "sp"),
            Reg16::Bp => write!(f, "bp"),
            Reg16::Si => write!(f, "si"),
            Reg16::Di => write!(f, "di"),
        }
    }
}

impl TryFrom<u8> for Reg16 {
    type Error = CpuError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0b000 => Ok(Reg16::Ax),
            0b001 => Ok(Reg16::Cx),
            0b010 => Ok(Reg16::Dx),
            0b011 => Ok(Reg16::Bx),
            0b100 => Ok(Reg16::Sp),
            0b101 => Ok(Reg16::Bp),
            0b110 => Ok(Reg16::Si),
            0b111 => Ok(Reg16::Di),
            _ => Err(CpuError::InvalidRegisterValue(value)),
        }
    }
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub(crate) enum SegmentReg {
    Es = 0b000,
    Cs = 0b001,
    Ss = 0b010,
    Ds = 0b011,
}

impl fmt::Display for SegmentReg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SegmentReg::Es => write!(f, "es"),
            SegmentReg::Cs => write!(f, "cs"),
            SegmentReg::Ss => write!(f, "ss"),
            SegmentReg::Ds => write!(f, "ds"),
        }
    }
}

impl TryFrom<u8> for SegmentReg {
    type Error = CpuError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0b000 => Ok(SegmentReg::Es),
            0b001 => Ok(SegmentReg::Cs),
            0b010 => Ok(SegmentReg::Ss),
            0b011 => Ok(SegmentReg::Ds),
            _ => Err(CpuError::InvalidRegisterValue(value)),
        }
    }
}
