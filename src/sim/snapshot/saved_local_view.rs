//! Saved local view facts, independent of window, renderer and input devices.
//!
//! Active-retail authorities: ScenarioSave689310/Load689470 (four cell
//! bookmarks), MiscSave67F82A/Load67FA2A (ordered selection),
//! MouseLoad5BE117..123 (Follow target) and the Display
//! cursor modes below. The committed Tactical center is the separate
//! `SavedTacticalView`. Original executed
//! bookmark controls live in tools/spatial_oracle/native_id_snapshot.json.
//! This record is VERA's local view payload, not a description of the native
//! SAV byte layout.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// Mouse5BE715/5BE092 preserves Power's semantic state in its raw display
/// stream. Geometry is derived; Sidebar NoInit6A4F8E/94 overwrites slide start
/// and duration. Flash keeps its raw signed timer words across process exit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedPowerBar {
    pub counts_surplus_output_drain: [i32; 3],
    pub cached_output: i32,
    pub cached_drain: i32,
    pub flashes_remaining: i32,
    pub flash_timer: [i32; 2],
    pub animating: bool,
}

/// Neutral transport for the client-owned RadarEvent array. Native
/// Save6607D0/Load660840 copy live records; PostLoad6851DF->660C50 clears
/// the separate Spacebar ring, so that ring is deliberately absent here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedRadarEvents {
    pub events: Vec<SavedRadarEvent>,
    pub last_advanced_frame: Option<u64>,
}

/// Animation scalars retain their binary32 bits without moving presentation
/// decisions into sim. The render owner validates and hydrates this record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedRadarEvent {
    pub event_type: crate::sim::radar::RadarEventType,
    pub cell: (u16, u16),
    pub radar_pixel: (i32, i32),
    pub created_frame: u64,
    pub radius: u32,
    pub rotation: u32,
    pub rotation_speed: u32,
    pub fade: u32,
    pub fade_speed: u32,
    pub expanding: bool,
    pub needs_draw: bool,
    pub phase_started_frame: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedLocalView {
    /// Raw signed CellStruct words preserved in the app's u16 bit-pattern form.
    view_bookmarks: [[u16; 2]; 4],
    selection_order: Vec<u64>,
    /// App projection of the serialized Simulation Select queue. No command
    /// payload is duplicated here; admission checks the existing queue owner.
    selection_order_pending: bool,
    follow_target: Option<u64>,
    modes: SavedCursorModes,
    /// The save coordinator attaches the Power owner's projection after the
    /// input owner supplies its own facts. App loads require this payload.
    power_bar: Option<SavedPowerBar>,
    radar_events: Option<SavedRadarEvents>,
    /// MouseClass::Save5BE6D0 (5BE715) / Load5BDF70 (5BE092) retain
    /// CreditsClass+4 (Mouse+5520).
    /// None preserves the client's not-yet-observed counter, rather than
    /// replacing an initialized display with the simulation's wallet balance.
    credits_counter: Option<i32>,
}

/// The Display's armed cursor modes. MouseLoad5BDF70 reads the whole Display
/// body at 5BE083 and swizzles the placement product and type at
/// 5BE101/5BE112, so Repair +11B0, Sell +11B1, the selected Super +11B8 and
/// the placement +11A4/+11A8/+11AC all survive a load.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedCursorModes {
    pub repair: bool,
    pub sell: bool,
    /// Retained VERA wire field, written false and ignored on restoration.
    /// Active YR PlanMgr at AC4CF4 is not saved. This is not an owner for the
    /// distinct legacy Display waypoint byte at +11B3.
    pub planning: bool,
    pub targeting: Option<SavedTargeting>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SavedTargeting {
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
pub enum SavedLocalViewError {
    #[error("saved selection repeats object {object_id}")]
    DuplicateSelectionReference { object_id: u64 },
    #[error("saved {field} references absent object {object_id}")]
    MissingEntityReference { field: &'static str, object_id: u64 },
    #[error("saved selection order does not match its simulation selection timeline")]
    SelectionTimelineMismatch,
    #[error("saved {field} names unknown {name}")]
    UnknownName { field: &'static str, name: String },
    #[error("saved local view has no Power bar state")]
    MissingPowerBar,
    #[error("saved Power bar has invalid {field}")]
    InvalidPowerState { field: &'static str },
}

impl SavedLocalView {
    pub fn new(
        view_bookmarks: [[u16; 2]; 4],
        selection_order: Vec<u64>,
        selection_order_pending: bool,
        follow_target: Option<u64>,
        modes: SavedCursorModes,
    ) -> Result<Self, SavedLocalViewError> {
        let saved = Self {
            view_bookmarks,
            selection_order,
            selection_order_pending,
            follow_target,
            modes,
            power_bar: None,
            radar_events: None,
            credits_counter: None,
        };
        saved.validate_structure()?;
        Ok(saved)
    }

    /// Also called after deserialization, before any app state is mutated.
    /// Object references and queued selection are validated against the new
    /// world by the input owner, after that world's own restore validation.
    pub fn validate_structure(&self) -> Result<(), SavedLocalViewError> {
        let mut selected = BTreeSet::new();
        for &object_id in &self.selection_order {
            if !selected.insert(object_id) {
                return Err(SavedLocalViewError::DuplicateSelectionReference { object_id });
            }
        }
        Ok(())
    }

    pub fn view_bookmarks(&self) -> [[u16; 2]; 4] {
        self.view_bookmarks
    }

    pub fn selection_order(&self) -> &[u64] {
        &self.selection_order
    }

    pub fn selection_order_pending(&self) -> bool {
        self.selection_order_pending
    }

    pub fn follow_target(&self) -> Option<u64> {
        self.follow_target
    }

    pub fn modes(&self) -> &SavedCursorModes {
        &self.modes
    }

    pub fn set_power_bar(&mut self, power_bar: SavedPowerBar) {
        self.power_bar = Some(power_bar);
    }

    pub fn take_power_bar(&mut self) -> Option<SavedPowerBar> {
        self.power_bar.take()
    }

    pub fn set_radar_events(&mut self, radar_events: SavedRadarEvents) {
        self.radar_events = Some(radar_events);
    }

    pub fn take_radar_events(&mut self) -> Option<SavedRadarEvents> {
        self.radar_events.take()
    }

    pub fn set_credits_counter(&mut self, displayed: Option<i32>) {
        self.credits_counter = displayed;
    }

    pub fn take_credits_counter(&mut self) -> Option<i32> {
        self.credits_counter.take()
    }

    pub fn into_local_input(self) -> (Vec<u64>, bool, Option<u64>) {
        (
            self.selection_order,
            self.selection_order_pending,
            self.follow_target,
        )
    }
}
