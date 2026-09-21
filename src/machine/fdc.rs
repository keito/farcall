use std::collections::VecDeque;

#[cfg(test)]
mod tests;

/// Emulates a Floppy Disk Controller
pub(super) struct Fdc {
    /// Digital Output Register
    dor: u8,
    interrupt_request: Option<PendingInterrupt>,
    result_bytes: VecDeque<u8>,
    command_state: CommandState,
    step_rate: u8,
    head_unload_time: u8,
    head_load_time: u8,
    non_dma: bool,
    read_data: Option<ReadData>,
}

enum PendingInterrupt {
    Reset,
    Recalibrated { drive: u8 },
}

enum CommandState {
    Idle,
    SpecifyFirst,
    SpecifySecond,
    RecalibrateDrive,
    ReadData,
}

struct ReadData {
    command: u8,
    parameters: [u8; 8],
    next: usize,
}

impl Fdc {
    pub(super) fn new() -> Self {
        Self {
            dor: 0,
            interrupt_request: None,
            result_bytes: VecDeque::new(),
            command_state: CommandState::Idle,
            step_rate: 0,
            head_unload_time: 0,
            head_load_time: 0,
            non_dma: false,
            read_data: None,
        }
    }

    pub(super) fn read(&mut self, port: u16) -> u8 {
        match port {
            // Idle vs result bytes available status
            0x3F4 => {
                if !matches!(self.command_state, CommandState::Idle) {
                    // Command busy bit set
                    0x90
                } else if self.result_bytes.is_empty() {
                    0x80
                } else {
                    0xD0
                }
            }
            // Result bytes from commands
            0x3F5 => self.result_bytes.pop_front().unwrap_or(0),
            _ => unreachable!(),
        }
    }

    fn sense_interrupt_status(&mut self) {
        // TODO: Fix the check here
        if let Some(interrupt_request) = self.interrupt_request.take() {
            match interrupt_request {
                // Ready-change status for drive A (ST0)
                PendingInterrupt::Reset => self.result_bytes.push_back(0xC0),
                // Seek completed
                PendingInterrupt::Recalibrated { drive } => {
                    self.result_bytes.push_back(0x20 | drive)
                }
            }

            // Cylinder 0
            self.result_bytes.push_back(0x00);
        } else {
            // Invalid command
            self.result_bytes.push_back(0x80);
        }
    }

    fn read_data(&mut self) {
        let read_data = self.read_data.as_ref().expect("expectorant");
        todo!(
            "read_data not impled. command: {}, parameters: {:?}",
            read_data.command,
            read_data.parameters
        );
    }

    pub(super) fn write(&mut self, port: u16, value: u8) {
        match port {
            0x3F2 => {
                if (self.dor & 0x4 == 0) && (value & 0x4 != 0) {
                    // Leaving Reset
                    self.interrupt_request = Some(PendingInterrupt::Reset);
                } else if (self.dor & 0x4 != 0) && (value & 0x4 == 0) {
                    // Entering Reset
                    self.result_bytes.clear();
                    self.command_state = CommandState::Idle;
                    self.interrupt_request = None;
                    self.read_data = None;
                }
                self.dor = value;
            }
            0x3F5 => {
                // Command bytes
                // Disallow new commands while there are results to be read
                if !self.result_bytes.is_empty() {
                    return;
                }
                match self.command_state {
                    CommandState::Idle => {
                        match value {
                            0x03 => self.command_state = CommandState::SpecifyFirst,
                            0x07 => self.command_state = CommandState::RecalibrateDrive,
                            0x08 => self.sense_interrupt_status(),
                            _ if value & 0x1F == 0x06 => {
                                self.read_data = Some(ReadData {
                                    command: value,
                                    parameters: [0; 8],
                                    next: 0,
                                });
                                self.command_state = CommandState::ReadData;
                            }
                            _ => {
                                // Unknown command
                                self.result_bytes.push_back(0x80);
                            }
                        };
                    }
                    CommandState::SpecifyFirst => {
                        self.step_rate = value >> 4;
                        self.head_unload_time = value & 0x0F;
                        self.command_state = CommandState::SpecifySecond;
                    }
                    CommandState::SpecifySecond => {
                        self.head_load_time = value >> 1;
                        self.non_dma = value & 1 != 0;
                        self.command_state = CommandState::Idle;
                    }
                    CommandState::RecalibrateDrive => {
                        // value is the drive to recalibrate
                        self.interrupt_request = Some(PendingInterrupt::Recalibrated {
                            drive: value & 0x03,
                        });
                        self.command_state = CommandState::Idle;
                    }
                    CommandState::ReadData => {
                        let Some(read_data) = self.read_data.as_mut() else {
                            panic!("ReadData state without read_data set");
                        };
                        read_data.parameters[read_data.next] = value;
                        if read_data.next == 7 {
                            self.command_state = CommandState::Idle;
                            self.read_data();
                        } else {
                            read_data.next += 1;
                        }
                    }
                };
            }
            _ => unreachable!(),
        }
    }

    /// The FDC's interrupt output pin
    pub(super) fn irq_output(&self) -> bool {
        // If we have an interrupt request and the DOR bit 3 (interrupt/DMA interface) is set
        self.interrupt_request.is_some() && (self.dor & 0x08 != 0)
    }
}
