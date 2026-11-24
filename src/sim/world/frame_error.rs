//! Failure of a synchronous simulation receiver, before the frame commits.

/// The world may contain earlier writes from this frame, including writes made
/// by the failing receiver. Callers must stop this timeline; retrying the same
/// frame would replay callbacks and RNG. No rollback or successful frame output
/// is implied by this error.
#[derive(Debug, thiserror::Error)]
#[error(
    "simulation frame {tick} (native {binary_frame}) stopped in a synchronous receiver for entity {entity_id}: {cause}; prior world mutations remain and the frame did not commit"
)]
pub struct FrameAdvanceError {
    pub tick: u64,
    pub binary_frame: u32,
    pub entity_id: u64,
    pub cause: String,
}

impl FrameAdvanceError {
    pub(crate) fn new(tick: u64, binary_frame: u32, entity_id: u64, cause: String) -> Self {
        Self {
            tick,
            binary_frame,
            entity_id,
            cause,
        }
    }
}
