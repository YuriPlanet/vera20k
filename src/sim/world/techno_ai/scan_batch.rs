//! Compatibility with the retired frame-start target-scan setting.
//!
//! Foot::Mission_Hunt 0x004D5392 and Mission_AreaGuard 0x004D6F06 call
//! Retaliate_And_Scan 0x00709820 synchronously. Earlier object turns can
//! admit or change candidates, so a previous answer is not authoritative.
//! All consumers now use the live scan owner in target_scan. Candidate
//! scoring within one scan retains its existing parallel implementation.

use crate::sim::world::Simulation;

/// Old saves encode Option<ScanBatch>; every former payload field was
/// serde-skipped. Keep the same empty struct and field name so positional
/// decoding and the snapshot layout remain compatible. This is no longer
/// a simulation setting and has no hash contribution.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct ScanBatch {}

impl Simulation {
    /// Compatibility for callers that requested frame-start scans. Scans
    /// always observe the world at their object's turn now.
    #[deprecated(note = "target scans run during each object's turn")]
    pub fn batch_target_scans(&mut self) {
        self.scan_batch = None;
    }
}

#[cfg(test)]
#[path = "scan_batch_tests.rs"]
mod tests;
