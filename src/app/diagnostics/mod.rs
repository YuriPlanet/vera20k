//! Diagnostics owner (F12): process debug overlays plus the hidden
//! shell/tactical capture harnesses. Match-lifetime diagnostic replay lives
//! in `app::match_diagnostics`.

pub(crate) mod core_load;
pub(crate) mod debug_overlays;
#[cfg(feature = "perf")]
pub(crate) mod perf_recorder;
pub(crate) mod performance_overlay;
pub(crate) mod shell_capture;
pub(crate) mod state;
pub(crate) mod tactical_capture;
pub(crate) mod thread_load;
