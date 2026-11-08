//! A terminal session's retained output, addressed by absolute byte offset.
//!
//! Reading does not remove anything: a reader says where it wants to start, so two readers can coexist and a long log can be read again at
//! a different place. When the buffer outgrows its cap the oldest text is dropped, and a reader asking for an offset that is gone is told how
//! many bytes it missed instead of silently getting a later slice.

/// What a read returned.
#[derive(Debug, PartialEq, Eq)]
pub struct Chunk {
    pub data: String,
    /// Where the next read should start to continue from here.
    pub next: u64,
    /// Bytes between the requested offset and the first one still held; zero when nothing was lost.
    pub dropped: u64,
}

pub struct OutputBuffer {
    text: String,
    /// The absolute offset of the first byte of `text`.
    base: u64,
    cap: usize,
}

impl OutputBuffer {
    pub fn new(cap: usize) -> Self {
        Self { text: String::new(), base: 0, cap }
    }

    /// The offset one past the last byte written.
    pub fn end(&self) -> u64 {
        self.base + self.text.len() as u64
    }

    pub fn push(&mut self, data: &str) {
        self.text.push_str(data);
        // Compact only once the buffer is a whole cap past the limit, then drop back to it, so a build printing continuously does not move
        // the retained text on every chunk.
        if self.text.len() > self.cap * 2 {
            let mut cut = self.text.len() - self.cap;
            while cut < self.text.len() && !self.text.is_char_boundary(cut) {
                cut += 1;
            }
            self.text.drain(..cut);
            self.base += cut as u64;
        }
    }

    /// Everything from `from` to the end. An offset past the end reads nothing.
    pub fn read_from(&self, from: u64) -> Chunk {
        let dropped = self.base.saturating_sub(from);
        let start = from.max(self.base) - self.base;
        let mut at = (start as usize).min(self.text.len());
        while at < self.text.len() && !self.text.is_char_boundary(at) {
            at += 1;
        }
        Chunk { data: self.text[at..].to_string(), next: self.end(), dropped }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_does_not_consume_and_a_cursor_continues_where_it_left_off() {
        let mut b = OutputBuffer::new(1024);
        b.push("hello ");
        let first = b.read_from(0);
        assert_eq!((first.data.as_str(), first.next, first.dropped), ("hello ", 6, 0));
        b.push("world");
        assert_eq!(b.read_from(first.next).data, "world");
        // Another reader, or the same one again, still sees everything.
        assert_eq!(b.read_from(0).data, "hello world");
        assert_eq!(b.read_from(99).data, "");
    }

    #[test]
    fn dropped_head_is_reported_with_how_much_was_lost() {
        let mut b = OutputBuffer::new(10);
        for _ in 0..10 {
            b.push("0123456789");
        }
        let all = b.read_from(0);
        assert!(all.dropped > 0 && all.dropped == b.end() - all.data.len() as u64, "{all:?}");
        assert_eq!(all.next, 100);
        // Asking from a point still held loses nothing.
        let recent = b.read_from(95);
        assert_eq!((recent.dropped, recent.data.len()), (0, 5));
    }

    #[test]
    fn compaction_never_splits_a_character() {
        let mut b = OutputBuffer::new(7);
        for _ in 0..20 {
            b.push("héllo");
        }
        let c = b.read_from(0);
        assert!(c.data.chars().all(|ch| ch == 'h' || ch == 'é' || ch == 'l' || ch == 'o'));
        assert_eq!(b.end(), 20 * "héllo".len() as u64);
    }
}
