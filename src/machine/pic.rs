#[cfg(test)]
mod tests;

/// Emulates the Programmable Interrupt Controller (PIC) 8259A
pub(super) struct Pic {
    irq_lines: u8,   // physical input levels
    irr: u8,         // pending requests
    isr: u8,         // requests being serviced
    imr: u8,         // masked interrupts
    vector_base: u8, // base address of vectors
    state: PicState,
    read_selector: ReadSelector,
}

#[derive(PartialEq)]
enum PicState {
    ExpectIcw2,
    ExpectIcw4,
    Normal,
}

#[derive(PartialEq)]
enum ReadSelector {
    ReturnIrr,
    ReturnIsr,
}

impl Pic {
    pub(super) fn new() -> Self {
        Self {
            irq_lines: 0,
            irr: 0,
            isr: 0,
            imr: 0,
            vector_base: 0x08,
            state: PicState::Normal,
            read_selector: ReadSelector::ReturnIrr,
        }
    }

    pub(super) fn read(&self, port: u16) -> u8 {
        match port {
            0x20 => {
                if self.read_selector == ReadSelector::ReturnIrr {
                    self.irr
                } else {
                    self.isr
                }
            }
            0x21 => self.imr,
            _ => 0,
        }
    }

    pub(super) fn write(&mut self, port: u16, value: u8) {
        match port {
            0x20 => {
                if value & 0x10 != 0 {
                    // Initialization (ICW1)
                    // TODO: value is ICW1
                    self.state = PicState::ExpectIcw2;
                    self.imr = 0;
                    self.read_selector = ReadSelector::ReturnIrr;
                    return;
                }

                if value & 0x08 != 0 {
                    // OCW3: read selection, polling, special mask mode
                    match value {
                        0xA => {
                            self.read_selector = ReadSelector::ReturnIrr;
                        }
                        0xB => {
                            self.read_selector = ReadSelector::ReturnIsr;
                        }
                        _ => (),
                    };
                } else {
                    // OCW2: EOI and priority control
                    match value >> 5 {
                        0b001 => {
                            self.end_interrupt(None);
                        }
                        0b011 => {
                            // value is 60h | irq
                            self.end_interrupt(Some(value & 0x07));
                        }
                        _ => (),
                    };
                }
            }
            0x21 => match self.state {
                PicState::Normal => {
                    self.imr = value;
                }
                PicState::ExpectIcw2 => {
                    self.vector_base = value;
                    self.state = PicState::ExpectIcw4;
                }
                PicState::ExpectIcw4 => {
                    // TODO: value is operating mode
                    self.state = PicState::Normal;
                }
            },
            _ => (),
        }
    }

    /// Called by devices to set their interrupt request line low or high
    pub(super) fn set_irq_line(&mut self, line: u8, state: bool) {
        let mask = 1 << line;
        let old_state = self.irq_lines & mask != 0;

        if state {
            self.irq_lines |= mask;
        } else {
            self.irq_lines &= !mask;
        }

        // Trigger on rising edge
        if state && !old_state {
            self.irr |= mask;
        }
    }

    /// Called by Machine to see if there is a pending interrupt
    pub(super) fn interrupt_pending(&self) -> bool {
        self.next_pending_irq().is_some()
    }

    /// Returns the highest-priority pending IRQ
    fn next_pending_irq(&self) -> Option<u8> {
        for i in 0..8 {
            let mask = 1 << i;
            // If an interrupt is being serviced, block this and lower-priority
            if self.isr & mask != 0 {
                return None;
            }
            // If this interrupt is requested, return it unless it is masked
            if self.irr & mask != 0 && (self.imr & mask) == 0 {
                return Some(i);
            }
        }
        None
    }

    /// Called by the CPU to acknowledge an interrupt and receive the interrupt vector address.
    pub(super) fn acknowledge(&mut self) -> u8 {
        let Some(irq) = self.next_pending_irq() else {
            panic!("Acknowledge called when no pending IRQ")
        };

        self.irr &= !(1 << irq);
        self.isr |= 1 << irq;

        (self.vector_base & 0xF8) | irq
    }

    /// Called on End Of Interrupt, optionally with an IRQ
    fn end_interrupt(&mut self, irq: Option<u8>) {
        if let Some(irq) = irq {
            self.isr &= !(1 << irq);
        } else {
            // Non-specific EOI: clear the highest-priority in-service bit
            for i in 0..8 {
                let mask = 1 << i;
                if self.isr & mask != 0 {
                    self.isr &= !mask;
                    break;
                }
            }
        }
    }
}
