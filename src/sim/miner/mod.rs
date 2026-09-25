//! Ore miner types, configuration, and ECS component.
//!
//! Defines the Miner component (state machine), CargoBale (discrete resource
//! unit), MinerConfig (tunable defaults), and ResourceType. Attached to
//! harvester entities (CMIN = Chrono Miner, HARV = War Miner).
//!
//! ## Dependency rules
//! - Part of sim/ -- may depend on rules/ for data-driven miner detection.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

mod exit_cell_search;
mod harvest_mission;
pub mod miner_dock;
pub(crate) mod miner_system;
pub(crate) mod ore_scan;
mod refinery_dock;

#[cfg(test)]
#[path = "miner_tests.rs"]
mod miner_tests;

#[cfg(test)]
#[path = "outbound_drive_tests.rs"]
mod outbound_drive_tests;

// Generic nearby-passable-cell search, reused by the tank-bunker exit placement.
pub(crate) use self::exit_cell_search::find_nearby_passable_cell_with_index;
pub(crate) use self::harvest_mission::dispatch_harvest_for_object;
pub(crate) use self::miner_system::extract_bale;
pub(crate) use self::refinery_dock::{
    clear_unload_latch, mission_enter, mission_unload, native_dock_miner, pay_refinery_owner,
    per_cell_dock_now, per_cell_release_dock_contact, tick_stage,
};

use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::GeneralRules;
use crate::sim::mission::MissionTimer;

/// Which kind of resource a map cell or cargo bale contains.
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
/// handler never writes a cursor above `4`. VERA's one extra cursor,
/// [`MinerState::ForcedReturn`], is numbered above that ceiling.
///
/// Residual: gamemd reaches cursor `4` from exactly one place, the bounded
/// scan's miss. VERA also parks there when the miner can reach no refinery at
/// all, so the cursor is *entered* more often than native's even though it is
/// never mis-numbered. Both entries take the same re-search exit; what gamemd
/// does with its own entry, and why VERA does not, is recorded on
/// `miner_system::handle_going_to_idle`.
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
    /// entry into this cursor. VERA also parks here when the miner can reach
    /// no refinery (VERA-internal, gamemd equivalent UNCHECKED).
    WaitNoOre = 4,
    /// Player issued a manual return order. VERA-internal. (Cursors 5 and 6,
    /// the retired drive-to-ore and legacy unload states, are unused.)
    ForcedReturn = 7,
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
            7 => Self::ForcedReturn,
            _ => return None,
        })
    }

    /// Highest cursor gamemd's Harvest handler ever writes. Cursors above it
    /// are VERA-internal states with no native counterpart.
    pub const NATIVE_CURSOR_CEILING: u32 = 4;
}

/// One discrete cargo bale carried by a miner.
///
/// Each harvest tick pops one bale worth of resource from the map cell and
/// pushes it into the miner's cargo hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CargoBale {
    pub resource_type: ResourceType,
    pub value: u16,
}

/// Tunable configuration for the miner/refinery/resource system.
///
/// Ship with RA2-like defaults; override for balance mods.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MinerConfig {
    // -- Bale values --
    /// Credits per ore bale.
    pub ore_bale_value: u16,
    /// Credits per gem bale.
    pub gem_bale_value: u16,

    // -- Cargo capacities (in bales) --
    /// War Miner bale capacity (1000 / 25 = 40 bales for ore).
    pub war_miner_capacity: u16,
    /// Chrono Miner bale capacity (500 / 25 = 20 bales for ore).
    pub chrono_miner_capacity: u16,

    // -- Timing (in sim ticks at 15Hz = RA2 game frames) --
    /// Whole-frame dump gate: the unload accumulator advances one frame per
    /// unloading tick and a resource slot drains once it reaches this value.
    /// Default 15 = ceil(HarvesterDumpRate(0.016) × 900) = ceil(14.4). Because
    /// the accumulator is integer-stepped, storing the ceiling reproduces
    /// gamemd's `rate × 900 <= accumulator` crossing exactly — no float in the
    /// gate, no tenths-rounding drift for modded rates.
    pub unload_tick_interval: u16,
}

impl Default for MinerConfig {
    fn default() -> Self {
        Self {
            ore_bale_value: 25,
            gem_bale_value: 50,
            // War Miner: 40 bales * 25 = 1000 ore, 40 * 50 = 2000 gems
            war_miner_capacity: 40,
            // Chrono Miner: 20 bales * 25 = 500 ore, 20 * 50 = 1000 gems
            chrono_miner_capacity: 20,
            // HarvesterDumpRate=0.016 × 900 = 14.4 frames/gate; the integer
            // accumulator crosses at ceil(14.4) = 15. The whole slot drains per
            // gate (one ore gate + one gem gate, ~15 frames each).
            unload_tick_interval: 15,
        }
    }
}

impl MinerConfig {
    /// Create a MinerConfig from parsed `[General]` rules data.
    ///
    /// Replaces hardcoded defaults with data-driven values from rules.ini.
    /// Bale values and capacities stay at defaults (not exposed in [General]).
    pub fn from_general_rules(general: &GeneralRules) -> Self {
        // HarvesterDumpRate is a double in gamemd (default 0.016). The dump gate
        // is `rate × 900 <= accumulator`; ruleset already stored ceil(rate × 900)
        // as a whole-frame threshold, so the gate stays integer-exact here.
        let unload_interval = general.harvester_dump_frames.max(1);

        Self {
            unload_tick_interval: unload_interval,
            ..Self::default()
        }
    }

    /// Create a `MinerConfig` from a full `RuleSet`, sourcing per-bale credit
    /// values from the `[Tiberiums]` registry so ore/gem payout tracks the mod's
    /// `Value=` overrides instead of hardcoded constants.
    ///
    /// Ore payout comes from the first `[Tiberiums]` entry (Riparius/ore image)
    /// and gem payout from the second (Cruentus/gem image), matching the native
    /// tiberium-type ordering. A registry entry that is absent or non-positive
    /// keeps the stock default (25 ore / 50 gem), so stock output stays
    /// byte-identical.
    pub fn from_rules(rules: &crate::rules::ruleset::RuleSet) -> Self {
        let defaults = Self::default();
        let bale_value = |id: crate::rules::tiberium_type::TiberiumTypeId, fallback: u16| -> u16 {
            rules
                .tiberium_types
                .get(id)
                .map(|t| t.value)
                .filter(|&v| v > 0)
                .map(|v| v.min(u16::MAX as i32) as u16)
                .unwrap_or(fallback)
        };
        Self {
            ore_bale_value: bale_value(
                crate::rules::tiberium_type::TiberiumTypeId(0),
                defaults.ore_bale_value,
            ),
            gem_bale_value: bale_value(
                crate::rules::tiberium_type::TiberiumTypeId(1),
                defaults.gem_bale_value,
            ),
            ..Self::from_general_rules(&rules.general)
        }
    }
}

/// ECS component: miner state machine and cargo hold.
///
/// Attached to harvester entities alongside Position, Owner, TypeRef, etc.
/// The miner_system tick reads and mutates this each frame.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Miner {
    pub kind: MinerKind,
    // NOTE: the FSM cursor (`state`) retired from this struct at the
    // substate-authority flip — `MissionCom::handler_state` is the cursor of
    // record. Read it via `GameEntity::miner_state()`.
    /// The refinery a player return order (`Command::MinerReturn`) pins
    /// Mission_Harvest state 2 to. VERA-internal: the native order is an Enter
    /// mission on the refinery.
    pub reserved_refinery: Option<u64>,
    /// Discrete cargo bales currently carried.
    pub cargo: Vec<CargoBale>,
    /// Maximum number of bales this miner can carry.
    pub capacity_bales: u16,
    /// Whether the player issued a manual return order.
    pub forced_return: bool,
    /// Unit+0x6D1 unload-active latch.
    #[serde(default)]
    pub unload_active: bool,
    /// Unit+0x6D2, set while Mission_Harvest works an ore cell: state 0's
    /// scan hit and state 1's hop write 1, state 0's entry and a failed
    /// Harvest_Ore_Tick write 0 (`0x0073E75B`, `0x0073E87D`, `0x0073E99A`,
    /// `0x0073EB19`). `UnitClass::DrawExtras` (`0x0073CEC0`) draws OREGATH
    /// from it while the locomotor is not moving.
    #[serde(default)]
    pub harvesting: bool,
    /// Unit+0xF8 StageClass value: Mission_Harvest state 1's step count and
    /// the unload's dump counter. `refinery_dock::tick_stage` (TechnoClass::AI
    /// `0x006FABC4`) adds the step (the constructor's 1) each time the timer
    /// expires while the rate is nonzero.
    #[serde(default)]
    pub stage_value: i32,
    /// Unit+0x100/+0x108, the StageClass timer.
    #[serde(default)]
    pub stage_timer: MissionTimer,
    /// Unit+0x10C, the StageClass rate; 0 stops the tick.
    #[serde(default)]
    pub stage_rate: u32,
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
            forced_return: false,
            unload_active: false,
            harvesting: false,
            stage_value: 0,
            stage_timer: MissionTimer::default(),
            stage_rate: 0,
        }
    }

    /// True when cargo is at capacity.
    pub fn is_full(&self) -> bool {
        self.cargo.len() as u16 >= self.capacity_bales
    }

    /// How many of the 5 UI pips should be filled.
    /// Each pip = 20% of capacity, rounded down.
    pub fn cargo_pips(&self) -> u8 {
        if self.capacity_bales == 0 {
            return 0;
        }
        let ratio = (self.cargo.len() as u32 * 5) / self.capacity_bales as u32;
        (ratio as u8).min(5)
    }

    /// Total credit value of all bales currently in the hold.
    #[cfg(test)]
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
    fn vera_internal_cursors_sit_above_the_native_ceiling() {
        for state in [MinerState::ForcedReturn] {
            assert!(
                state.cursor() > MinerState::NATIVE_CURSOR_CEILING,
                "{state:?} must not collide with a cursor gamemd writes"
            );
        }
    }

    #[test]
    fn harvest_cursor_round_trips_and_rejects_out_of_vocabulary_values() {
        for state in [
            MinerState::SearchOre,
            MinerState::Harvest,
            MinerState::ReturnToRefinery,
            MinerState::Dock,
            MinerState::WaitNoOre,
            MinerState::ForcedReturn,
        ] {
            assert_eq!(MinerState::from_cursor(state.cursor()), Some(state));
        }
        assert_eq!(MinerState::from_cursor(5), None, "retired drive-to-ore cursor");
        assert_eq!(MinerState::from_cursor(6), None, "retired legacy unload cursor");
        assert_eq!(MinerState::from_cursor(8), None);
        assert_eq!(MinerState::from_cursor(u32::MAX), None);
    }

    #[test]
    fn default_config_war_miner_ore_payout() {
        let cfg = MinerConfig::default();
        // War Miner full ore: capacity * ore_bale_value = 40 * 25 = 1000
        assert_eq!(
            cfg.war_miner_capacity as u32 * cfg.ore_bale_value as u32,
            1000
        );
    }

    #[test]
    fn default_config_war_miner_gem_payout() {
        let cfg = MinerConfig::default();
        // War Miner full gems: 40 * 50 = 2000
        assert_eq!(
            cfg.war_miner_capacity as u32 * cfg.gem_bale_value as u32,
            2000
        );
    }

    #[test]
    fn default_config_chrono_miner_ore_payout() {
        let cfg = MinerConfig::default();
        // Chrono Miner full ore: 20 * 25 = 500
        assert_eq!(
            cfg.chrono_miner_capacity as u32 * cfg.ore_bale_value as u32,
            500
        );
    }

    #[test]
    fn default_config_chrono_miner_gem_payout() {
        let cfg = MinerConfig::default();
        // Chrono Miner full gems: 20 * 50 = 1000
        assert_eq!(
            cfg.chrono_miner_capacity as u32 * cfg.gem_bale_value as u32,
            1000
        );
    }

    #[test]
    fn cargo_pips_shows_five_steps() {
        let cfg = MinerConfig::default();
        let mut miner = Miner::new(MinerKind::War, &cfg, 0);
        assert_eq!(miner.cargo_pips(), 0);
        // Fill 20% (8 of 40 bales)
        for _ in 0..8 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            });
        }
        assert_eq!(miner.cargo_pips(), 1);
        // Fill 40%
        for _ in 0..8 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            });
        }
        assert_eq!(miner.cargo_pips(), 2);
        // Fill 100%
        while !miner.is_full() {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            });
        }
        assert_eq!(miner.cargo_pips(), 5);
    }

    #[test]
    fn miner_kind_detection_is_data_driven() {
        let mut war = ObjectType::from_ini_section(
            "MODHARV",
            &crate::rules::ini_parser::IniFile::from_str("[MODHARV]\nHarvester=yes\n")
                .section("MODHARV")
                .expect("section"),
            crate::rules::object_type::ObjectCategory::Vehicle,
        );
        assert_eq!(miner_kind_for_object(&war), Some(MinerKind::War));

        war.teleporter = true;
        assert_eq!(miner_kind_for_object(&war), Some(MinerKind::Chrono));

        let non_harvester = ObjectType::from_ini_section(
            "E1",
            &crate::rules::ini_parser::IniFile::from_str("[E1]\nFixtureOnly=1\n")
                .section("E1")
                .expect("section"),
            crate::rules::object_type::ObjectCategory::Infantry,
        );
        assert_eq!(miner_kind_for_object(&non_harvester), None);
    }

    #[test]
    fn from_rules_sources_bale_values_from_tiberiums() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;

        // Full-skeleton ruleset (so from_ini succeeds) with modded ore/gem Value=.
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             0=HARV\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GAREFN\n\
             [HARV]\n\
             Name=War Miner\n\
             Harvester=yes\n\
             [GAREFN]\n\
             Name=Ore Refinery\n\
             Refinery=yes\nDockUnload=yes\n\
             [Tiberiums]\n\
             0=Riparius\n\
             1=Cruentus\n\
             [Riparius]\n\
             Name=Tiberium Riparius\n\
             Image=1\n\
             Value=77\n\
             [Cruentus]\n\
             Name=Tiberium Cruentus\n\
             Image=2\n\
             Value=88\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("rules");
        let cfg = MinerConfig::from_rules(&rules);
        assert_eq!(
            cfg.ore_bale_value, 77,
            "ore bale value comes from [Riparius] Value="
        );
        assert_eq!(
            cfg.gem_bale_value, 88,
            "gem bale value comes from [Cruentus] Value="
        );
    }

    #[test]
    fn from_rules_empty_tiberiums_keeps_stock_defaults() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;

        // No [Tiberiums] section -> stock byte-identical 25/50.
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             0=HARV\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GAREFN\n\
             [HARV]\n\
             Name=War Miner\n\
             Harvester=yes\n\
             [GAREFN]\n\
             Name=Ore Refinery\n\
             Refinery=yes\nDockUnload=yes\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("rules");
        let cfg = MinerConfig::from_rules(&rules);
        assert_eq!(cfg.ore_bale_value, 25);
        assert_eq!(cfg.gem_bale_value, 50);
    }
}
