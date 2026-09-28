use std::time::{SystemTime, UNIX_EPOCH};

/// Supplies the wall-clock timestamps the runtime writes into session rows.
///
/// Unix milliseconds carry no offset, so the value is the same whichever local zone a host is
/// configured with; hosts therefore do not inject a clock of their own here.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SystemClock;

impl SystemClock {
    /// Returns the current Unix timestamp in milliseconds for persisted audit fields.
    pub(crate) fn now_timestamp_millis(self) -> i64 {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_millis() as i64,
            Err(_) => 0,
        }
    }
}
