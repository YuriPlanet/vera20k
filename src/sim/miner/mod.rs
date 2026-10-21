//! Ore miner types, configuration, and ECS component.
//!
//! Defines the Miner component (state machine), CargoBale (discrete resource
//! unit), MinerConfig (tunable defaults), and ResourceType. Attached to
//! harvester entities (CMIN = Chrono Miner, HARV = War Miner).
//!
//! ## Dependency rules
//! - Part of sim/ -- may depend on rules/ for data-driven miner detection.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

mod harvest_mission;
#[cfg(test)]
pub mod miner_dock;
pub(crate) mod miner_system;
pub(crate) mod ore_scan;
mod refinery_dock;
pub(crate) use refinery_dock::UnloadImage;

#[cfg(test)]
#[path = "outbound_drive_tests.rs"]
mod outbound_drive_tests;

#[cfg(test)]
#[path = "pad_nudge_tests.rs"]
mod pad_nudge_tests;

#[cfg(test)]
#[path = "no_ore_park_tests.rs"]
mod no_ore_park_tests;

#[cfg(test)]
#[path = "refinery_reach_tests.rs"]
mod refinery_reach_tests;

pub(crate) use self::harvest_mission::mission_harvest;
pub(crate) use self::miner_system::extract_bale;
pub(crate) use self::refinery_dock::{
    clear_unload_anim_for_destination, clear_unload_latch, drain_first_slot, mission_unload,
    pay_refinery_owner, per_cell_release_dock_contact,
};

use crate::rules::object_type::ObjectType;

/// Ore/gem presentation classification; not a native TiberiumClass identity.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ResourceType {
    Ore,
    Gem,
}

/// Which miner chassis this entity uses.
/// Determines movement behavior (drive vs chrono-teleport) and cargo capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MinerKind {
    /// Soviet War Miner (HARV): drives both ways, armed, large cargo.
    War,
    /// Allied Chrono Miner (CMIN): drives to ore, teleports back to refinery.
    Chrono,
    /// Yuri Slave Miner (SMIN): deploys into refinery (YAREFN), spawns slave infantry.
    /// Does not harvest directly — slaves harvest and deposit at the deployed building.
    Slave,
}

/// State machine for the miner harvest loop.
///
/// The decoded vocabulary of the Harvest mission cursor: the value of record
/// lives in `MissionCom::handler_state` and round-trips through
/// [`MinerState::cursor`] / [`MinerState::from_cursor`]. The discriminants are
/// explicit because they are the persisted/hashed cursor encoding.
/// `SearchOre = 0` deliberately coincides with the zeroed handler state every
/// mission transition writes, so a fresh Harvest assignment lands on the
/// FSM's initial state without a separate write.
///
/// The five states gamemd's Harvest handler holds keep gamemd's own cursor
/// numbering (`UnitClass::Mission_Harvest @ 0x0073E5E0`): `0` looking, `1`
/// cutting ore, `2` finding home, `3` the dock handoff, `4` going idle — the
/// handler never writes a cursor above `4`. Cursors `5..=7`, retired VERA
/// states (drive-to-ore, legacy unload, the old player return), decode as none.
///
/// Cursor `4` has one entry, as in gamemd: the bounded scan's miss with no
/// destination and no archive (`0x0073E8EA..0x0073E924`).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum MinerState {
    /// Looking for the nearest ore/gem cell to harvest.
    SearchOre = 0,
    /// Extracting bales from the current cell.
    Harvest = 1,
    /// Finding a refinery: HELLO the nearest free one in range, else wait by
    /// the nearest.
    ReturnToRefinery = 2,
    /// The dock handoff: queue Enter. The dock itself runs as the Enter and
    /// Unload missions (`refinery_dock`).
    Dock = 3,
    /// Parked because the bounded ore scan found nothing — gamemd's only
    /// entry into this cursor. Its dispatch queues Guard
    /// (`miner_system::handle_going_to_idle`).
    WaitNoOre = 4,
}

impl MinerState {
    /// Encode this state as the `MissionCom::handler_state` cursor dword.
    #[inline]
    pub fn cursor(self) -> u32 {
        self as u32
    }

    /// Decode a `MissionCom::handler_state` cursor dword. Unknown values yield
    /// `None`; callers decide the fallback (dispatch treats it as `SearchOre`
    /// with a debug assert — the only writers are the FSM commit and the
    /// zeroing mission transitions, so an unknown value is a logic error).
    pub fn from_cursor(raw: u32) -> Option<Self> {
        Some(match raw {
            0 => Self::SearchOre,
            1 => Self::Harvest,
            2 => Self::ReturnToRefinery,
            3 => Self::Dock,
            4 => Self::WaitNoOre,
            _ => return None,
        })
    }
}

/// One discrete cargo bale carried by a miner.
///
/// Each harvest tick pops one bale worth of resource from the map cell and
/// pushes it into the miner's cargo hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CargoBale {
    /// Native StorageClass slot. Grouping and payment use this identity;
    /// multiple types can have the same ore/gem presentation classification.
    pub tiberium_type: crate::rules::tiberium_type::TiberiumTypeId,
    /// Presentation only, derived from the harvested type's Image.
    pub resource_type: ResourceType,
    pub value: u16,
}

impl CargoBale {
    /// Unit Harvest_Ore_Tick73D450 and Infantry Harvest522E70 preserve the
    /// Cell's native Tiberium index when storing the reduction. Value comes
    /// from that TiberiumClass, not the many-to-one Image classification.
    /// Native executions: spatial_oracle/harvest_field.json ore_tick rows.
    fn from_reduction(
        reduction: crate::sim::tiberium::ReduceTiberiumOutcome,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Option<Self> {
        if reduction.removed_amount == 0 {
            return None;
        }
        Self::of_type(reduction.tiberium_type?, reduction.resource_type?, rules)
    }

    /// One bale of a native Tiberium index, valued from that TiberiumClass.
    pub fn of_type(
        tiberium_type: crate::rules::tiberium_type::TiberiumTypeId,
        resource_type: ResourceType,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Option<Self> {
        let ty = rules.tiberium_types.get(tiberium_type)?;
        Some(Self {
            tiberium_type,
            resource_type,
            // Retain the existing whole-bale/u16 credit representation.
            value: ty.value.clamp(0, i32::from(u16::MAX)) as u16,
        })
    }
}

/// Tunable configuration for the miner/refinery/resource system.
///
/// Ship with RA2-like defaults; override for balance mods.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MinerConfig {
    // -- Cargo capacities (in bales) --
    /// War Miner bale capacity (1000 / 25 = 40 bales for ore).
    pub war_miner_capacity: u16,
    /// Chrono Miner bale capacity (500 / 25 = 20 bales for ore).
    pub chrono_miner_capacity: u16,
}

impl Default for MinerConfig {
    fn default() -> Self {
        Self {
            // War Miner: 40 bales * 25 = 1000 ore, 40 * 50 = 2000 gems
            war_miner_capacity: 40,
            // Chrono Miner: 20 bales * 25 = 500 ore, 20 * 50 = 1000 gems
            chrono_miner_capacity: 20,
        }
    }
}

/// A harvester's hold and latches. `sim::miner` writes them; everyone else
/// reads.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Miner {
    kind: MinerKind,
    // NOTE: the FSM cursor (`state`) retired from this struct at the
    // substate-authority flip — `MissionCom::handler_state` is the cursor of
    // record. Read it via `GameEntity::miner_state()`.
    /// Retired: the refinery VERA's old player return order pinned. The
    /// order is now gamemd's MEGAMISSION Enter on the refinery
    /// (`Command::MegaMission`), and nothing writes this; a save from before
    /// may restore a stale id, which only the state hash reads. The field
    /// keeps its place in saves and the hash until the sim-scale layout
    /// change removes it.
    reserved_refinery: Option<u64>,
    /// Discrete cargo bales currently carried.
    cargo: Vec<CargoBale>,
    /// Maximum number of bales this miner can carry.
    capacity_bales: u16,
    /// Unit+0x6D1 unload-active latch.
    #[serde(default)]
    unload_active: bool,
    /// Unit+0x6D2, set while Mission_Harvest works an ore cell: state 0's
    /// scan hit and state 1's hop write 1, state 0's entry and a failed
    /// Harvest_Ore_Tick write 0 (`0x0073E75B`, `0x0073E87D`, `0x0073E99A`,
    /// `0x0073EB19`). `UnitClass::Draw` (`0x0073CEC0`) draws OREGATH
    /// from it while the locomotor is not moving.
    #[serde(default)]
    harvesting: bool,
}

impl Miner {
    /// Create a new miner in SearchOre state with the given kind and config.
    ///
    /// `obj_storage` is the per-unit `Storage=` value from rules.ini (in bales).
    /// When > 0 it overrides the kind-based default in `config`, matching gamemd
    /// reading TechnoTypeClass+0x800 directly as the harvester's max capacity.
    /// Pass 0 to use the kind default.
    pub fn new(kind: MinerKind, config: &MinerConfig, obj_storage: u16) -> Self {
        let capacity_bales = match kind {
            MinerKind::War => {
                if obj_storage > 0 {
                    obj_storage
                } else {
                    config.war_miner_capacity
                }
            }
            MinerKind::Chrono => {
                if obj_storage > 0 {
                    obj_storage
                } else {
                    config.chrono_miner_capacity
                }
            }
            // Slave Miners don't carry cargo — their slave infantry harvest instead.
            MinerKind::Slave => 0,
        };
        Self {
            kind,
            reserved_refinery: None,
            cargo: Vec::with_capacity(capacity_bales as usize),
            capacity_bales,
            unload_active: false,
            harvesting: false,
        }
    }

    /// The chassis.
    pub fn kind(&self) -> MinerKind {
        self.kind
    }

    /// The retired return-order refinery (see the field).
    pub fn reserved_refinery(&self) -> Option<u64> {
        self.reserved_refinery
    }

    /// The bales in the hold, oldest first.
    pub fn cargo(&self) -> &[CargoBale] {
        &self.cargo
    }

    /// The hold's capacity in bales.
    pub fn capacity_bales(&self) -> u16 {
        self.capacity_bales
    }

    /// Unit+0x6D1, the unload latch.
    pub fn unload_active(&self) -> bool {
        self.unload_active
    }

    /// Unit+0x6D2, the harvest latch.
    pub fn harvesting(&self) -> bool {
        self.harvesting
    }

    /// Install a fixture's hold.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_cargo_for_test(&mut self, cargo: Vec<CargoBale>) {
        self.cargo = cargo;
    }

    /// Install a fixture's unload latch.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_unload_active_for_test(&mut self, active: bool) {
        self.unload_active = active;
    }

    /// Install a fixture's harvest latch.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_harvesting_for_test(&mut self, harvesting: bool) {
        self.harvesting = harvesting;
    }

    /// True when cargo is at capacity.
    pub fn is_full(&self) -> bool {
        self.cargo.len() as u16 >= self.capacity_bales
    }

    /// Total credit value of all bales currently in the hold.
    #[cfg(any(test, feature = "test-support"))]
    pub fn cargo_value(&self) -> u32 {
        self.cargo.iter().map(|b| b.value as u32).sum()
    }
}

/// Determine the miner chassis from parsed rules data.
///
/// Detection priority:
/// 1. `Enslaves=` present → Slave Miner (SMIN). Does NOT have `Harvester=yes`.
/// 2. `Harvester=yes` + `Teleporter=yes` → Chrono Miner (CMIN).
/// 3. `Harvester=yes` → War Miner (HARV).
pub fn miner_kind_for_object(object: &ObjectType) -> Option<MinerKind> {
    // Slave Miner detected via Enslaves= (SMIN does NOT have Harvester=yes).
    if object.enslaves.is_some() {
        return Some(MinerKind::Slave);
    }

    if !object.harvester {
        return None;
    }

    if object.teleporter {
        Some(MinerKind::Chrono)
    } else {
        Some(MinerKind::War)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harvest_cursor_uses_the_native_numbering_for_the_states_gamemd_holds() {
        // gamemd's Harvest handler cursor: 0 looking, 1 cutting ore, 2 finding
        // home, 3 dock handoff, 4 going idle. Pinning the encoding is the whole
        // point of the field being comparable.
        assert_eq!(MinerState::SearchOre.cursor(), 0);
        assert_eq!(MinerState::Harvest.cursor(), 1);
        assert_eq!(MinerState::ReturnToRefinery.cursor(), 2);
        assert_eq!(MinerState::Dock.cursor(), 3);
        assert_eq!(MinerState::WaitNoOre.cursor(), 4);
    }

    #[test]
    fn miner_kind_detection_is_data_driven() {
        let mut war = ObjectType::from_ini_section(
            "MODHARV",
            crate::rules::ini_parser::IniFile::from_str("[MODHARV]\nHarvester=yes\n")
                .section("MODHARV")
                .expect("section"),
            crate::rules::object_type::ObjectCategory::Vehicle,
        );
        assert_eq!(miner_kind_for_object(&war), Some(MinerKind::War));

        war.teleporter = true;
        assert_eq!(miner_kind_for_object(&war), Some(MinerKind::Chrono));

        let non_harvester = ObjectType::from_ini_section(
            "E1",
            crate::rules::ini_parser::IniFile::from_str("[E1]\nFixtureOnly=1\n")
                .section("E1")
                .expect("section"),
            crate::rules::object_type::ObjectCategory::Infantry,
        );
        assert_eq!(miner_kind_for_object(&non_harvester), None);
    }
}
