//! Retained Building gap admission, separate from per-viewer cell receipts.
//!
//! Fresh Techno6F2B40 clears269/26C; Building43B740 clears6C8. Original
//! Building save/load preserves all three fields. See the Phase3 gap report.
use crate::sim::intern::InternedId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct GapGeneratorRuntime {
    ///269/26C belong to each represented native local player's client. A
    ///single viewer's SpySat bracket can change them before Building6C8.
    pub viewers: BTreeMap<InternedId, GapDeposit>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct GapDeposit {
    /// Techno+269: deposit admission, even when no hostile viewer is present.
    pub active: bool,
    /// Techno+26C: lazily copied signed Type+CD2, retained after removal.
    pub radius: i32,
}
