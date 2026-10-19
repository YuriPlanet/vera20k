//! Diagnostics owner (F12): process debug overlays plus the hidden
//! shell/tactical capture harnesses. Match-lifetime diagnostic replay lives
//! in `app::match_diagnostics`.

pub(crate) mod debug_overlays;
pub(crate) mod shell_capture;
pub(crate) mod state;
pub(crate) mod tactical_capture;
