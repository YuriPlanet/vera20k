//! Data-only lifecycle requests emitted by simulation subsystems.
//!
//! Subsystems enqueue these allocation-free values instead of mutating object
//! storage directly. The world lifecycle authority drains them at its verified
//! commit point.

/// Why an object requested central UnInit processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UninitReason {
    /// A crusher completed the victim's same-tick crush sequence.
    Crush,
}

/// Ordered request for the world lifecycle authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LifecycleRequest {
    /// Run the common UnInit transaction for `stable_id`.
    Uninit {
        stable_id: u64,
        reason: UninitReason,
    },
}
