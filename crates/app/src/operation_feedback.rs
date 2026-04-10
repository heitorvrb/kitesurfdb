use std::time::{Duration, Instant};

pub const OP_TIMEOUT_SECS: u64 = 8;
pub const OP_TIMEOUT: Duration = Duration::from_secs(OP_TIMEOUT_SECS);
pub const SLOW_WARNING_MS: u128 = 3000;

pub fn slow_warning_message() -> String {
    format!(
        "This is taking longer than usual. The request will be cancelled after {OP_TIMEOUT_SECS}s."
    )
}

pub fn timeout_error_message(target: &str) -> String {
    format!("{target} cancelled after {OP_TIMEOUT_SECS}s because it took too long")
}

pub fn remaining_timeout(started_at: Instant) -> Option<Duration> {
    OP_TIMEOUT.checked_sub(started_at.elapsed())
}
