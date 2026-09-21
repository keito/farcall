#[cfg(test)]
mod tests;

/// Emulates the Programmable Interval Timer (8253)
pub(super) struct Pit {
    counter0: Counter,
    counter1: Counter,
    counter2: Counter,
}

struct Counter {
    reload: u32,
    pending_reload: u16,
    current: u32,
    state: CounterState,
    mode: u8,
    bcd: bool,
    output: bool,
}

impl Counter {
    fn new() -> Self {
        Self {
            reload: 0,
            pending_reload: 0,
            current: 0,
            state: CounterState::Undefined,
            mode: 0,
            bcd: false,
            output: false,
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

    fn write(&mut self, value: u8) {
        match self.state {
            CounterState::Undefined => panic!("Undef"),
            CounterState::ExpectLeastSignificantByte => {
                self.pending_reload = value as u16;
                self.state = CounterState::ExpectMostSignificantByte;
            }
            CounterState::ExpectMostSignificantByte => {
                self.update_reload(((self.pending_reload & 0x00FF) | ((value as u16) << 8)) as u32);
                self.state = CounterState::ExpectLeastSignificantByte;
            }
            CounterState::LeastSignificantByteOnly => self.update_reload(value as u32),
            CounterState::MostSignificantByteOnly => self.update_reload((value as u32) << 8),
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

enum CounterState {
    Undefined,
    MostSignificantByteOnly,
    LeastSignificantByteOnly,
    ExpectLeastSignificantByte,
    ExpectMostSignificantByte,
}

impl Pit {
    pub(super) fn new() -> Self {
        Self {
            counter0: Counter::new(),
            counter1: Counter::new(),
            counter2: Counter::new(),
        }
    }

    pub(super) fn read(&self, port: u16) -> u8 {
        match port {
            _ => todo!(),
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
                    0b00 => todo!("Counter latching operation"),
                    0b10 => counter.state = CounterState::MostSignificantByteOnly,
                    0b01 => counter.state = CounterState::LeastSignificantByteOnly,
                    0b11 => counter.state = CounterState::ExpectLeastSignificantByte,
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
