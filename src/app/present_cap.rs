//! Presentation frame cap.
//!
//! The window presents with `PresentMode::Fifo`, so the compositor returns one
//! frame per display refresh. A 144 Hz panel therefore renders 144 frames a
//! second for a simulation that advances a few ticks per second, which heats the
//! machine for no gameplay benefit. Holding the frame slot is what a 60 Hz
//! display did in retail.
//!
//! Focused play is capped at 60 fps by default (`VERA_FPS_CAP` overrides it, `0`
//! disables the cap). An unfocused window drops to 15 fps
//! (`VERA_FPS_CAP_IDLE`), because the player is not looking at it.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

thread_local! {
    /// Last presentation instant for this thread; the render callback runs on the
    /// event-loop thread only.
    static LAST: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
}

/// Frames per second granted to presentation; `None` means uncapped.
fn cap_hz(focused: bool) -> Option<u32> {
    static FOCUSED: OnceLock<Option<u32>> = OnceLock::new();
    static UNFOCUSED: OnceLock<Option<u32>> = OnceLock::new();
    if focused {
        *FOCUSED.get_or_init(|| {
            let cap = parse_cap(std::env::var("VERA_FPS_CAP").ok().as_deref(), 60);
            match cap {
                Some(hz) => log::info!("presentation cap: {hz} fps"),
                None => log::info!("presentation cap: disabled"),
            }
            cap
        })
    } else {
        *UNFOCUSED.get_or_init(|| {
            let cap = parse_cap(std::env::var("VERA_FPS_CAP_IDLE").ok().as_deref(), 15);
            match cap {
                Some(hz) => log::info!("presentation cap (unfocused): {hz} fps"),
                None => log::info!("presentation cap (unfocused): disabled"),
            }
            cap
        })
    }
}

/// Parse a cap override: unset or empty keeps `default`, `0` means uncapped.
fn parse_cap(raw: Option<&str>, default: u32) -> Option<u32> {
    match raw {
        None | Some("") => Some(default),
        Some(text) => match text.parse::<u32>() {
            Ok(0) => None,
            Ok(hz) => Some(hz),
            Err(_) => Some(default),
        },
    }
}

/// Sleep until the next presentation slot is due.
///
/// A late frame resets the slot instead of accumulating debt, so a slow render
/// never busy-loops to catch up.
pub(crate) fn pace_frame(focused: bool) {
    let Some(hz) = cap_hz(focused) else {
        return;
    };
    let slot = Duration::from_secs_f64(1.0 / f64::from(hz));
    if let Some(last) = LAST.with(std::cell::Cell::get) {
        let deadline = last + slot;
        let now = Instant::now();
        if now < deadline {
            std::thread::sleep(deadline - now);
        }
    }
    // The slot starts after the sleep, not before it: setting this first made
    // frames alternate short/long and overshoot the cap by ~50%.
    LAST.with(|cell| cell.set(Some(Instant::now())));
}

#[cfg(test)]
mod tests {
    use super::parse_cap;

    #[test]
    fn unset_or_empty_keeps_the_given_default() {
        assert_eq!(parse_cap(None, 60), Some(60));
        assert_eq!(parse_cap(Some(""), 15), Some(15));
    }

    #[test]
    fn zero_disables_the_cap_and_garbage_keeps_the_default() {
        assert_eq!(parse_cap(Some("0"), 60), None);
        assert_eq!(parse_cap(Some("144"), 60), Some(144));
        assert_eq!(parse_cap(Some("abc"), 15), Some(15));
    }
}
