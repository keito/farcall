pub(crate) trait CpuValue: Copy {
    fn is_zero(self) -> bool;
    fn is_negative(self) -> bool;
    fn low_byte(self) -> u8;
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
}
