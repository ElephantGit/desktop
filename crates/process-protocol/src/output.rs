/// Explicit per-run policy; capture limits are byte counts, independently applied to each pipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputPolicy {
    Discard,
    Capture {
        stdout_limit: usize,
        stderr_limit: usize,
    },
}

/// Streams have independent ordering, limits and EOF facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// Pipe completion says nothing about process exit, cleanup, or durable storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputState {
    Open,
    Eof,
    Failed(String),
}

/// An in-memory retained prefix. Truncation remains visible even after EOF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputRead {
    pub bytes: Vec<u8>,
    pub retained: usize,
    pub truncated: bool,
    pub state: OutputState,
}
