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
        // front drain of a `Copy` element only moves the head index: no per-chunk memmove
        self.buf.drain(..over);
        self.buf.extend(data);
    }

    pub fn snapshot(&self) -> Vec<u8> {
        // two slice copies (the deque may wrap), not one push per byte
        let (head, tail) = self.buf.as_slices();
        let mut out = Vec::with_capacity(self.buf.len());
        out.extend_from_slice(head);
        out.extend_from_slice(tail);
        out
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
    fn replay_is_byte_identical_after_the_deque_wraps() {
        let cap = 1000;
        let mut r = Ring::new(cap);
        let mut all = Vec::new();
        for i in 0..500u32 {
            let chunk: Vec<u8> = (0..(i % 97 + 1)).map(|j| (i + j) as u8).collect();
            r.push(&chunk);
            all.extend_from_slice(&chunk);
            let want = &all[all.len().saturating_sub(cap)..];
            assert_eq!(r.snapshot(), want, "after chunk {i}");
        }
        assert_eq!(r.snapshot().len(), cap);
    }

    #[test]
    fn oversized_chunk_keeps_its_tail() {
        let mut r = Ring::new(3);
        r.push(b"zz");
        r.push(b"abcdefg");
        assert_eq!(r.snapshot(), b"efg");
    }
}
