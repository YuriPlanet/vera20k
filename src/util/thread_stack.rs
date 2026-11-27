//! The Android entry point's rule for a thread's stack: small stacks are
//! raised to what the game's biggest thread work asks, nothing is ever lowered,
//! and a caller that brought its own stack keeps it.

/// What the game's heaviest thread work asks for (`app::match_runtime::sim_host`,
/// `sim::snapshot`).
pub const ANDROID_MIN_STACK: usize = 8 * 1024 * 1024;

/// The size to request for a new thread, or `None` to keep its attribute as the
/// caller built it: a stack the caller provided (`pthread_attr_setstack`) is
/// never resized, and a thread that asked for more than the minimum keeps it.
pub fn android_stack(requested: usize, caller_stack: bool) -> Option<usize> {
    if caller_stack || requested >= ANDROID_MIN_STACK {
        None
    } else {
        Some(ANDROID_MIN_STACK)
    }
}

#[cfg(all(test, tests_util))]
mod tests {
    use super::{ANDROID_MIN_STACK, android_stack};

    #[test]
    fn small_stacks_are_raised_while_larger_and_caller_ones_are_kept() {
        assert_eq!(android_stack(2 * 1024 * 1024, false), Some(ANDROID_MIN_STACK));
        assert_eq!(android_stack(ANDROID_MIN_STACK, false), None);
        assert_eq!(android_stack(16 * 1024 * 1024, false), None);
        assert_eq!(android_stack(64 * 1024, true), None);
    }
}
