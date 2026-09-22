use super::BytePhase;

#[cfg(test)]
mod tests;

/// Emulates the Programmable Interval Timer (8253)
pub(super) struct Pit {
    counter0: Counter,
    counter1: Counter,
    counter2: Counter,
}

struct Counter {
    access_mode: AccessMode,
    bcd: bool,
    byte_phase: BytePhase,
    current: u32,
    latched_count: Option<u16>,
    mode: u8,
    output: bool,
    pending_reload: u16,
    reload: u32,
}

impl Counter {
    fn new() -> Self {
        Self {
            access_mode: AccessMode::LowThenHigh,
            byte_phase: BytePhase::Low,
            reload: 0,
            pending_reload: 0,
            current: 0,
            mode: 0,
            bcd: false,
            output: false,
            latched_count: None,
        }
    }

    fn advance(&mut self, ticks: u8) {
        // An uninitialized counter at 0 should not start
        if self.current == 0 {
            return;
        }

        match self.mode {
            2 => {
                for _ in 0..ticks {
                    self.current -= 1;
                    if self.current == 1 {
                        self.output = false;
                    } else if self.current == 0 {
                        self.output = true;
                        self.current = self.reload;
                    }
                }
            }
            3 => {
                for _ in 0..ticks {
                    // By checking if current is odd, we catch the first time an odd reload is ticked
                    // because after that, the subtraction below makes it even
                    if self.current % 2 != 0 {
                        if self.output {
                            // If at the start of the high half-period, subtract 1
                            self.current -= 1;
                        } else {
                            // If at the start of the low half-period, subtract 3
                            self.current -= 3;
                        }
                    } else {
                        self.current -= 2;
                    }
                    if self.current == 0 {
                        self.current = self.reload;
                        self.output = !self.output;
                    }
                }
            }
            _ => unreachable!(),
        }
    }

    fn read(&mut self) -> u8 {
        let Some(latched_count) = self.latched_count else {
            return 0;
        };

        match self.access_mode {
            AccessMode::LowThenHigh => match self.byte_phase {
                BytePhase::Low => {
                    self.byte_phase = BytePhase::High;
                    (latched_count & 0xFF) as u8
                }
                BytePhase::High => {
                    self.byte_phase = BytePhase::Low;
                    self.latched_count = None;
                    (latched_count >> 8) as u8
                }
            },
            AccessMode::LowOnly => {
                self.latched_count = None;
                (latched_count & 0xFF) as u8
            }
            AccessMode::HighOnly => {
                self.latched_count = None;
                (latched_count >> 8) as u8
            }
        }
    }

    fn write(&mut self, value: u8) {
        match self.access_mode {
            AccessMode::LowThenHigh => match self.byte_phase {
                BytePhase::Low => {
                    self.pending_reload = value as u16;
                    self.byte_phase = BytePhase::High;
                }
                BytePhase::High => {
                    self.update_reload(
                        ((self.pending_reload & 0x00FF) | ((value as u16) << 8)) as u32,
                    );
                    self.byte_phase = BytePhase::Low;
                }
            },
            AccessMode::LowOnly => self.update_reload(value as u32),
            AccessMode::HighOnly => self.update_reload((value as u32) << 8),
        }
    }

    fn update_reload(&mut self, value: u32) {
        self.reload = value;
        if self.reload == 0 {
            self.reload = 65_536;
        }
        if self.current == 0 {
            self.current = self.reload;
        }
    }
}

enum AccessMode {
    LowOnly,
    HighOnly,
    LowThenHigh,
}

impl Pit {
    pub(super) fn new() -> Self {
        Self {
            counter0: Counter::new(),
            counter1: Counter::new(),
            counter2: Counter::new(),
        }
    }

    pub(super) fn read(&mut self, port: u16) -> u8 {
        match port {
            0x40 => self.counter0.read(),
            0x41 => self.counter1.read(),
            0x42 => self.counter2.read(),
            _ => unreachable!(),
        }
    }

    pub(super) fn write(&mut self, port: u16, value: u8) {
        match port {
            0x40 => self.counter0.write(value),
            0x41 => self.counter1.write(value),
            0x42 => self.counter2.write(value),
            0x43 => {
                // Control word register
                // SC - Select Counter
                let counter = match value >> 6 {
                    0 => &mut self.counter0,
                    1 => &mut self.counter1,
                    2 => &mut self.counter2,
                    _ => unreachable!(),
                };
                // RL - Read/Load
                match (value >> 4) & 0b11 {
                    0b00 => {
                        // Ignore command if we still have a latched count stored
                        if counter.latched_count.is_some() {
                            return;
                        }
                        counter.latched_count = Some(if counter.current == 65_536 {
                            0
                        } else {
                            counter.current as u16
                        });
                        counter.byte_phase = BytePhase::Low;
                        return;
                    }
                    0b10 => counter.access_mode = AccessMode::HighOnly,
                    0b01 => counter.access_mode = AccessMode::LowOnly,
                    0b11 => {
                        counter.access_mode = AccessMode::LowThenHigh;
                        counter.byte_phase = BytePhase::Low;
                    }
                    _ => unreachable!(),
                }

                counter.mode = (value >> 1) & 0b111;
                match counter.mode {
                    2 | 3 => counter.output = true,
                    _ => todo!(
                        "Only modes 2 and 3 are supported: Requested mode {}",
                        counter.mode
                    ),
                }
                counter.bcd = value & 1 != 0;
                if counter.bcd {
                    todo!("BCD mode is not supported yet");
                }
                // Reset counter so it stops
                counter.current = 0;
            }
            _ => unreachable!(),
        }
    }

    pub(super) fn advance(&mut self, ticks: u8) {
        self.counter0.advance(ticks);
        self.counter1.advance(ticks);
        self.counter2.advance(ticks);
    }

    pub(super) fn output0(&self) -> bool {
        self.counter0.output
    }
}
