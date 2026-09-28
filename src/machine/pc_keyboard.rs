use std::collections::VecDeque;

const TRANSFER_COUNTDOWN_STEPS: usize = 1000;

pub(super) struct PcKeyboard {
    scan_code_queue: VecDeque<u8>,
    transfer_steps_remaining: Option<usize>,
}

impl PcKeyboard {
    pub(super) fn new() -> Self {
        Self {
            scan_code_queue: VecDeque::new(),
            transfer_steps_remaining: None,
        }
    }

    pub(super) fn enqueue(&mut self, scan_code: u8, pressed: bool) {
        let code = if pressed { scan_code } else { scan_code | 0x80 };
        self.scan_code_queue.push_back(code);
    }

    pub(super) fn peek(&self) -> Option<u8> {
        self.scan_code_queue.front().copied()
    }

    pub(super) fn pop_front(&mut self) -> Option<u8> {
        self.scan_code_queue.pop_front()
    }

    pub(super) fn advance_countdown(&mut self) {
        if let Some(countdown) = self.transfer_steps_remaining {
            self.transfer_steps_remaining = Some(countdown.saturating_sub(1));
        } else {
            self.transfer_steps_remaining = Some(TRANSFER_COUNTDOWN_STEPS);
        }
    }

    pub(super) fn is_transfer_ready(&self) -> bool {
        matches!(self.transfer_steps_remaining, Some(0))
    }

    pub(super) fn reset_transfer_countdown(&mut self) {
        self.transfer_steps_remaining = None;
    }
}
