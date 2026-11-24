//! Hidden, no-input production tactical-capture checkpoint.
//!
//! This app-layer diagnostic enters through accepted skirmish startup, drives
//! ordinary simulation commands one exact production step at a time, observes
//! the real tactical renderer, and publishes one final swapchain readback. It
//! never grants gameplay state or certifies native pixel parity.

pub(crate) mod evidence;
pub(crate) mod integrity;
pub(crate) mod manifest;
pub(crate) mod placement;
pub(crate) mod profile;
pub(crate) mod script;
pub(crate) mod session;
