//! Test-only Approach answers for the `harvest_attack_return` replay. That
//! native harness substitutes the live-target Approach vcall itself: during a
//! seam-armed dispatch step, execution that reaches the Approach receiver
//! (`0x007414E0`) executes a declared `mov eax, approach_return; ret 4`
//! outside the image and the caller resumes (`APPROACH_SEAM` in
//! tools/spatial_oracle/harvest_attack_return.py), recording one
//! `supplied_approach_return` event per interception. The declared answer
//! leaves the Target and destination untouched; a replay substitutes it on
//! the Approach owner ahead of its head and body. Nothing is substituted
//! unless a test installs the declared answers for the row it replays.

use std::cell::RefCell;

thread_local! {
    static APPROACH_RETURNS: RefCell<Vec<i32>> = const { RefCell::new(Vec::new()) };
}

/// Install one declared answer per Approach call the replayed step's native
/// packet intercepted, in call order.
pub(crate) fn install_approach_returns(returns: &[i32]) {
    APPROACH_RETURNS.with(|answers| {
        *answers.borrow_mut() = returns.to_vec();
    });
}

/// Remove the seam, answering how many declared answers were left unused.
pub(crate) fn finish_approach_returns() -> usize {
    APPROACH_RETURNS.with(|answers| std::mem::take(&mut *answers.borrow_mut()).len())
}

/// The declared answer for one Approach call; None when the row supplies no
/// answer for it, and the real body runs.
pub(crate) fn take_approach_return() -> Option<i32> {
    APPROACH_RETURNS.with(|answers| {
        let mut answers = answers.borrow_mut();
        (!answers.is_empty()).then(|| answers.remove(0))
    })
}
