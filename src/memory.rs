use thiserror::Error;

#[cfg(test)]
mod tests;

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

    pub(crate) fn wrapping_add(&self, rhs: u16) -> PhysAddr {
        Self((self.0 + (rhs as u32)) & 0xFFFFF)
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

pub(crate) fn read_buffer(
    memory: &[u8],
    address: PhysAddr,
    length: usize,
) -> Result<Vec<u8>, MemoryError> {
    let mut cur_byte = address;
    let mut bytes = Vec::<u8>::with_capacity(length);
    while bytes.len() < length {
        let byte = read_byte(memory, cur_byte)?;
        bytes.push(byte);
        cur_byte = cur_byte.wrapping_add(1);
    }
    Ok(bytes)
}

pub(crate) fn validate_buffer(
    memory: &[u8],
    address: PhysAddr,
    num_bytes: usize,
) -> Result<(), MemoryError> {
    let memory_len = memory.len();
    let mut cur_byte = address;
    // Validate every destination address
    for _ in 0..num_bytes {
        if cur_byte.as_usize() >= memory_len {
            return Err(MemoryError::OutOfBoundsAccess(format!("{:#X}", address)));
        };
        cur_byte = cur_byte.wrapping_add(1);
    }
    Ok(())
}

pub(crate) fn write_buffer(
    memory: &mut [u8],
    address: PhysAddr,
    buffer: &[u8],
) -> Result<(), MemoryError> {
    validate_buffer(memory, address, buffer.len())?;

    // Actually perform the writes
    let mut cur_byte = address;
    for i in 0..buffer.len() {
        write_byte(memory, cur_byte, buffer[i])?;
        cur_byte = cur_byte.wrapping_add(1);
    }

    Ok(())
}

fn calc_word_addr(memory: &[u8], address: PhysAddr) -> Result<(usize, usize), MemoryError> {
    let byte1 = address.as_usize();
    // The second byte might wrap
    let byte2 = address.wrapping_add(1).as_usize();
    if memory.get(byte1).is_none() || memory.get(byte2).is_none() {
        return Err(MemoryError::OutOfBoundsAccess(format!("{:#X}", address)));
    };
    Ok((byte1, byte2))
}
