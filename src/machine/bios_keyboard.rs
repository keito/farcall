use std::collections::VecDeque;

pub(super) struct BiosKeyboard {
    queue: VecDeque<BiosKeyStroke>,
}

impl BiosKeyboard {
    pub(super) fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    pub(crate) fn enqueue(&mut self, key: BiosKeyStroke) {
        self.queue.push_back(key)
    }

    pub(crate) fn pop(&mut self) -> Option<BiosKeyStroke> {
        self.queue.pop_front()
    }

    pub(crate) fn peek(&self) -> Option<&BiosKeyStroke> {
        self.queue.front()
    }

    pub(crate) fn clear(&mut self) {
        self.queue.clear();
    }
}

pub struct BiosKeyStroke {
    pub ascii: Option<u8>,
    pub scan_code: u8,
}
