use thiserror::Error;

#[derive(Error, Debug)]
pub(crate) enum MemoryError {
    #[error("Out of bounds memory access: {0}")]
    OutOfBoundsAccess(String),
}

#[derive(Copy, Clone)]
pub(crate) struct PhysAddr(u32);

impl PhysAddr {
    pub(crate) fn new(segment: u16, offset: u16) -> Self {
        Self((((segment as u32) << 4) + (offset as u32)) & 0xFFFFF)
    }

    pub(crate) fn as_usize(&self) -> usize {
        self.0 as usize
    }
}

impl std::fmt::UpperHex for PhysAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::UpperHex::fmt(&self.0, f)
    }
}

pub(crate) fn write_byte(
    memory: &mut [u8],
    address: PhysAddr,
    value: u8,
) -> Result<(), MemoryError> {
    let Some(mem_mut) = memory.get_mut(address.as_usize()) else {
        return Err(MemoryError::OutOfBoundsAccess(format!("{:#X}", address)));
    };
    *mem_mut = value;
    Ok(())
}

pub(crate) fn read_byte(memory: &[u8], address: PhysAddr) -> Result<u8, MemoryError> {
    let Some(data) = memory.get(address.as_usize()) else {
        return Err(MemoryError::OutOfBoundsAccess(format!("{:#X}", address)));
    };
    Ok(*data)
}

pub(crate) fn write_word(
    memory: &mut [u8],
    address: PhysAddr,
    value: u16,
) -> Result<(), MemoryError> {
    let (byte1, byte2) = calc_word_addr(memory, address)?;
    let [low, high] = value.to_le_bytes();
    memory[byte1] = low;
    memory[byte2] = high;
    Ok(())
}

pub(crate) fn read_word(memory: &[u8], address: PhysAddr) -> Result<u16, MemoryError> {
    let (byte1, byte2) = calc_word_addr(memory, address)?;
    Ok(u16::from_le_bytes([memory[byte1], memory[byte2]]))
}

fn calc_word_addr(memory: &[u8], address: PhysAddr) -> Result<(usize, usize), MemoryError> {
    let byte1 = address.as_usize();
    // The second byte might wrap
    let byte2 = (byte1 + 1) & 0xFFFFF;
    if memory.get(byte1).is_none() || memory.get(byte2).is_none() {
        return Err(MemoryError::OutOfBoundsAccess(format!("{:#X}", address)));
    };
    Ok((byte1, byte2))
}
