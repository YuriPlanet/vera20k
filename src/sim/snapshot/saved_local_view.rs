//! Saved local view facts, independent of window, renderer and input devices.
//!
//! Active-retail authorities: ScenarioSave689310/Load689470 (four cell
//! bookmarks), MiscSave67F82A/Load67FA2A (ordered selection), Techno+214 (one
//! group membership), MouseLoad5BE117..123 (Follow target) and the Display
//! cursor modes below. The committed Tactical center is the separate
//! `SavedTacticalView`. Original executed
//! bookmark controls live in tools/spatial_oracle/native_id_snapshot.json.
//! This record is VERA's local view payload, not a description of the native
//! SAV byte layout.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// Ten native team-selection slots; an object belongs to at most one slot.
pub(crate) const CONTROL_GROUP_SLOTS: usize = 10;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SavedLocalView {
    /// Raw signed CellStruct words preserved in the app's u16 bit-pattern form.
    view_bookmarks: [[u16; 2]; 4],
    selection_order: Vec<u64>,
    /// App projection of the serialized Simulation Select queue. No command
    /// payload is duplicated here; admission checks the existing queue owner.
    selection_order_pending: bool,
    control_groups: [Vec<u64>; CONTROL_GROUP_SLOTS],
    follow_target: Option<u64>,
    modes: SavedCursorModes,
}

/// The Display's armed cursor modes. MouseLoad5BDF70 reads the whole Display
/// body at 5BE083 and swizzles the placement product and type at
/// 5BE101/5BE112, so Repair +11B0, Sell +11B1, the selected Super +11B8 and
/// the placement +11A4/+11A8/+11AC all survive a load.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct SavedCursorModes {
    pub(crate) repair: bool,
    pub(crate) sell: bool,
    pub(crate) targeting: Option<SavedTargeting>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum SavedTargeting {
    /// The type, the factory's held product (absent after its pointer
    /// expired) and that House, by name.
    BuildingPlacement {
        type_id: String,
        product_id: Option<u64>,
        owner: String,
    },
    /// The selected superweapon's type.
    SuperWeapon(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum SavedLocalViewError {
    #[error("saved selection repeats object {object_id}")]
    DuplicateSelectionReference { object_id: u64 },
    #[error("saved control groups repeat object {object_id}")]
    DuplicateGroupReference { object_id: u64 },
    #[error("saved {field} references absent object {object_id}")]
    MissingEntityReference { field: &'static str, object_id: u64 },
    #[error("saved selection order does not match its simulation selection timeline")]
    SelectionTimelineMismatch,
    #[error("saved {field} names unknown {name}")]
    UnknownName { field: &'static str, name: String },
}

impl SavedLocalView {
    pub(crate) fn new(
        view_bookmarks: [[u16; 2]; 4],
        selection_order: Vec<u64>,
        selection_order_pending: bool,
        control_groups: [Vec<u64>; CONTROL_GROUP_SLOTS],
        follow_target: Option<u64>,
        modes: SavedCursorModes,
    ) -> Result<Self, SavedLocalViewError> {
        let saved = Self {
            view_bookmarks,
            selection_order,
            selection_order_pending,
            control_groups,
            follow_target,
            modes,
        };
        saved.validate_structure()?;
        Ok(saved)
    }

    /// Also called after deserialization, before any app state is mutated.
    /// Object references and queued selection are validated against the new
    /// world by the input owner, after that world's own restore validation.
    pub(crate) fn validate_structure(&self) -> Result<(), SavedLocalViewError> {
        let mut selected = BTreeSet::new();
        for &object_id in &self.selection_order {
            if !selected.insert(object_id) {
                return Err(SavedLocalViewError::DuplicateSelectionReference { object_id });
            }
        }
        let mut grouped = BTreeSet::new();
        for &object_id in self.control_groups.iter().flatten() {
            if !grouped.insert(object_id) {
                return Err(SavedLocalViewError::DuplicateGroupReference { object_id });
            }
        }
        Ok(())
    }

    pub(crate) fn view_bookmarks(&self) -> [[u16; 2]; 4] {
        self.view_bookmarks
    }

    pub(crate) fn selection_order(&self) -> &[u64] {
        &self.selection_order
    }

    pub(crate) fn selection_order_pending(&self) -> bool {
        self.selection_order_pending
    }

    pub(crate) fn control_groups(&self) -> &[Vec<u64>; CONTROL_GROUP_SLOTS] {
        &self.control_groups
    }

    pub(crate) fn follow_target(&self) -> Option<u64> {
        self.follow_target
    }

    pub(crate) fn modes(&self) -> &SavedCursorModes {
        &self.modes
    }

    pub(crate) fn into_local_input(
        self,
    ) -> (Vec<u64>, bool, [Vec<u64>; CONTROL_GROUP_SLOTS], Option<u64>) {
        (
            self.selection_order,
            self.selection_order_pending,
            self.control_groups,
            self.follow_target,
        )
    }
}
