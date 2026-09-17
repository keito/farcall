use crate::memory::{MemoryError, PhysAddr, read_byte, read_word, write_byte, write_word};

pub(crate) trait CpuValue: Copy {
    fn is_zero(self) -> bool;
    fn is_negative(self) -> bool;
    fn low_byte(self) -> u8;
    fn read(memory: &[u8], address: PhysAddr) -> Result<Self, MemoryError>;
    fn write(self, memory: &mut [u8], address: PhysAddr) -> Result<(), MemoryError>;
}

impl CpuValue for u8 {
    fn is_zero(self) -> bool {
        self == 0
    }

    fn is_negative(self) -> bool {
        self & (1 << 7) != 0
    }

    fn low_byte(self) -> u8 {
        self
    }

    fn read(memory: &[u8], address: PhysAddr) -> Result<Self, MemoryError> {
        read_byte(memory, address)
    }

    fn write(self, memory: &mut [u8], address: PhysAddr) -> Result<(), MemoryError> {
        write_byte(memory, address, self)
    }
}

impl CpuValue for u16 {
    fn is_zero(self) -> bool {
        self == 0
    }

    fn is_negative(self) -> bool {
        self & (1 << 15) != 0
    }

    fn low_byte(self) -> u8 {
        self as u8
    }

    fn read(memory: &[u8], address: PhysAddr) -> Result<Self, MemoryError> {
        read_word(memory, address)
    }

    fn write(self, memory: &mut [u8], address: PhysAddr) -> Result<(), MemoryError> {
        write_word(memory, address, self)
    }
}
