use std::collections::VecDeque;

pub(super) struct PcKeyboard {
    scan_code_queue: VecDeque<u8>,
}

impl PcKeyboard {
    pub(super) fn new() -> Self {
        Self {
            scan_code_queue: VecDeque::new(),
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
}
