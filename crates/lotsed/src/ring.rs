//! Raw byte ring buffer per session, replayed to xterm.js on attach.

use std::collections::VecDeque;

pub struct Ring {
    buf: VecDeque<u8>,
    cap: usize,
}

impl Ring {
    pub fn new(cap: usize) -> Self {
        Self {
            buf: VecDeque::new(),
            cap: cap.max(1),
        }
    }

    pub fn push(&mut self, data: &[u8]) {
        if data.len() >= self.cap {
            self.buf.clear();
            self.buf.extend(&data[data.len() - self.cap..]);
            return;
        }
        let over = (self.buf.len() + data.len()).saturating_sub(self.cap);
        self.buf.drain(..over);
        self.buf.extend(data);
    }

    pub fn snapshot(&self) -> Vec<u8> {
        self.buf.iter().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_everything_under_cap() {
        let mut r = Ring::new(8);
        r.push(b"abc");
        r.push(b"de");
        assert_eq!(r.snapshot(), b"abcde");
    }

    #[test]
    fn drops_oldest_bytes_over_cap() {
        let mut r = Ring::new(4);
        r.push(b"abc");
        r.push(b"def");
        assert_eq!(r.snapshot(), b"cdef");
    }

    #[test]
    fn oversized_chunk_keeps_its_tail() {
        let mut r = Ring::new(3);
        r.push(b"zz");
        r.push(b"abcdefg");
        assert_eq!(r.snapshot(), b"efg");
    }
}
