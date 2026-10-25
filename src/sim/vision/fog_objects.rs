//! Immutable drawing records owned by the fog snapshot store.
//!
//! Original FoggedObjectClass building constructor4D0EF0 records type/frame,
//! owner and location, not a live Building pointer. DrawAll4D1890 uses these
//! records after the original object has changed or ceased to exist.

use serde::{Deserialize, Serialize};

use crate::sim::components::DriveCoord;
use crate::sim::intern::InternedId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FoggedDrawKind {
    Building,
    Animation,
}

/// The meaningful fields of one native 16-byte drawing record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FoggedDrawRecord {
    pub kind: FoggedDrawKind,
    pub type_ref: InternedId,
    pub frame: i32,
    pub no_zshape: bool,
    pub z_adjust: i32,
}

/// Snapshot geometry is independent of any live entity or camera.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FrozenBuilding {
    pub owner: InternedId,
    pub location: DriveCoord,
    pub level: i32,
    pub visible: bool,
    pub records: Vec<FoggedDrawRecord>,
}
