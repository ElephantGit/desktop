//! Splits an incrementally arriving byte stream into newline-delimited logical records while
//! keeping memory bounded.
//!
//! The framer never trusts the producer to emit a newline: once a record grows past the
//! configured limit it is flushed as an ordered sequence of fragments instead of being held
//! until a newline finally arrives. A consumer can therefore treat a `Line` as one complete
//! record and reassemble a fragmented one from its sequence number and index without ever
//! buffering more than the limit itself.

/// One record produced by [`BoundedLineFramer`]; never includes the terminating newline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineFrame {
    /// A whole newline-terminated record (or the trailing record at end of input) within limit.
    Line(Vec<u8>),
    /// One piece of a record that exceeded the limit before its newline arrived.
    ///
    /// Pieces of one oversized record share a `sequence`; `index` orders them and `last` marks
    /// the piece that carried the newline or the end of input.
    Fragment {
        sequence: u64,
        index: u32,
        last: bool,
        bytes: Vec<u8>,
    },
}

/// Incremental, bounded newline framer.
#[derive(Debug)]
pub struct BoundedLineFramer {
    max_record_bytes: usize,
    buffer: Vec<u8>,
    /// `Some` while the current record has already been partially flushed as fragments.
    overflow: Option<Overflow>,
    next_sequence: u64,
}

#[derive(Debug)]
struct Overflow {
    sequence: u64,
    next_index: u32,
}

impl BoundedLineFramer {
    /// Creates a framer that flushes any record reaching `max_record_bytes` as fragments.
    ///
    /// # Panics
    ///
    /// Panics when `max_record_bytes` is zero, which could never hold a record.
    pub fn new(max_record_bytes: usize) -> Self {
        assert!(max_record_bytes > 0, "record limit must be positive");
        Self {
            max_record_bytes,
            buffer: Vec::new(),
            overflow: None,
            next_sequence: 0,
        }
    }

    /// Feeds one chunk of bytes and returns every frame it completes, in order.
    ///
    /// Chunk boundaries carry no meaning: the same byte sequence produces the same frames however
    /// it is split, which is what makes the framer safe on top of pipe reads.
    pub fn push(&mut self, mut chunk: &[u8]) -> Vec<LineFrame> {
        let mut frames = Vec::new();
        while !chunk.is_empty() {
            match chunk.iter().position(|byte| *byte == b'\n') {
                Some(newline) => {
                    let (line, rest) = chunk.split_at(newline);
                    self.append_bounded(line, &mut frames);
                    frames.push(self.take_record(/*last*/ true));
                    chunk = &rest[1..];
                }
                None => {
                    self.append_bounded(chunk, &mut frames);
                    chunk = &[];
                }
            }
        }
        frames
    }

    /// Flushes whatever precedes end of input as a final frame, if anything was buffered.
    pub fn finish(&mut self) -> Option<LineFrame> {
        if self.buffer.is_empty() && self.overflow.is_none() {
            return None;
        }
        Some(self.take_record(/*last*/ true))
    }

    /// Appends bytes to the current record, emitting fragments whenever the limit is reached.
    ///
    /// A record that exactly fills the buffer is only flushed once more input proves it is not
    /// complete; otherwise a record of exactly the limit would be split for no reason.
    fn append_bounded(&mut self, mut bytes: &[u8], frames: &mut Vec<LineFrame>) {
        while !bytes.is_empty() {
            let room = self.max_record_bytes - self.buffer.len();
            let take = room.min(bytes.len());
            self.buffer.extend_from_slice(&bytes[..take]);
            bytes = &bytes[take..];
            if self.buffer.len() == self.max_record_bytes && !bytes.is_empty() {
                frames.push(self.take_record(/*last*/ false));
            }
        }
    }

    /// Drains the buffer into a `Line` or the next fragment of an oversized record.
    fn take_record(&mut self, last: bool) -> LineFrame {
        let bytes = std::mem::take(&mut self.buffer);
        let mut overflow = self.overflow.take();
        if overflow.is_none() && last {
            return LineFrame::Line(bytes);
        }
        let state = overflow.get_or_insert_with(|| {
            let sequence = self.next_sequence;
            self.next_sequence += 1;
            Overflow {
                sequence,
                next_index: 0,
            }
        });
        let frame = LineFrame::Fragment {
            sequence: state.sequence,
            index: state.next_index,
            last,
            bytes,
        };
        state.next_index += 1;
        if !last {
            self.overflow = overflow;
        }
        frame
    }
}
