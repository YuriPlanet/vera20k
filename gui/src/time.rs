//! Host clocks: `std::time` natively, `web-time` in the browser build.
pub use web_time::{Duration, Instant};

/// The shell's one-second alternation, on the host's UI clock in ms: whether
/// `now_ms` lies in the second of each two seconds from `start_ms`. NewEdit's
/// caret shows in the first; the owner-draw button's timer highlight
/// (`612B70`'s `4DC`, initially off) lights in the second.
pub fn blink_phase(start_ms: u64, now_ms: u64) -> bool {
    now_ms.saturating_sub(start_ms) / 1000 % 2 == 1
}
