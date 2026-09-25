//! Bridge-crossing replay golden — determinism + baseline ratchet for the one
//! scenario no committed fixture in this repo covered: a ground vehicle driving
//! across a **high bridge** under the live `advance_tick` path.
//!
//! `global_parity_harness_tests.rs` is the model for the shape (record a
//! `ReplayLog`, replay it through `ReplayRunner` on a fresh `Simulation`, assert
//! tick-for-tick hash equality plus a committed final baseline). Every "bridge"
//! in that harness is the *mission* bridge — a code concept — so a regression in
//! deck height, the `on_bridge` flag, or the bridge occupancy layer sails
//! straight through the default `--lib` suite. This file closes that gap.
//!
//! Scope note: the fixture stamps a synthetic span into matching path and resolved terrain rather than
//! loading a retail map, so it runs with no assets and belongs in the DEFAULT
//! suite. The retail-map half of the same question — does this hold on real
//! stamped map data — is `sim::movement::movement_bridge_retail_tests`, which is
//! `#[ignore]`d because it needs `RA2_DIR`.
//!
//! The height invariant asserted on every deck frame is the one recorded on
//! `resolve_cell_transition_bridge_state` in `sim::movement::movement_bridge`
//! (`FootClass::Set_Height_On_Bridge` @ `0x005F5FA0`, read back by
//! `ObjectClass::GetHeight` @ `0x005F5F30`):
//!
//! ```text
//! position.z == own cell's signed terrain level + (on_bridge ? 4 : 0)
//! ```
//!
//! No new gamemd claim is made here; this fixture only pins the Rust behavior
//! those already-recorded citations describe.

use super::*;
use crate::map::entities::{EntityCategory, MapEntity};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::pathfinding::PathGrid;
use crate::sim::replay::{ReplayHeader, ReplayLog, ReplayRunner};
use std::collections::{BTreeMap, BTreeSet};

const BRIDGE_HARNESS_SEED: u64 = 0x0B21_D6E5_C0DE;
const BRIDGE_HARNESS_TICKS: u64 = 200;
const BRIDGE_HARNESS_TICK_MS: u32 = 67;

const GRID_W: u16 = 64;
const GRID_H: u16 = 64;

/// The span runs west-to-east along this row.
const SPAN_Y: u16 = 20;
/// Plateau (approach) terrain level on both banks.
const APPROACH_LEVEL: u8 = 4;
/// Terrain level of the gorge the span crosses — the riverbed the tank must
/// never sit on while it is flagged on-bridge.
const GORGE_LEVEL: u8 = 0;
/// `FootClass::Set_Height_On_Bridge`'s deck term, in levels. Same number as
/// `sim::movement::movement_occupancy::BRIDGE_DECK_LEVEL_DELTA`, which is
/// `pub(super)` to the movement module and therefore not nameable from here.
const DECK_LEVEL_DELTA: i16 = 4;

/// Start cell: plain plateau ground, one step before the entry ramp.
const APPROACH_A_X: u16 = 14;
/// Entry ramp: a bridgehead **transition** cell that is deliberately NOT
/// structural. `set_cell_for_test` ties `bridge_structural` to `bridge_walkable`
/// and so cannot express it; `set_bridge_cell_decoupled_for_test` can. It has to
/// be non-structural because the runtime Exit arm fires on
/// `src structural && !dst structural` — a structural exit ramp would leave the
/// tank flagged on-bridge after it had already driven off the span.
const ENTRY_RAMP_X: u16 = 15;
/// First and last structural deck cell (inclusive) — eight cells over the gorge.
const DECK_FIRST_X: u16 = 16;
const DECK_LAST_X: u16 = 23;
/// Exit ramp, mirror of the entry ramp.
const EXIT_RAMP_X: u16 = 24;
/// Destination: plain plateau ground on the far bank.
const APPROACH_B_X: u16 = 25;

/// Distinct structural deck cells the tank must actually stand on. Eight exist;
/// requiring six leaves room for the sub-cell curve to skip an end cell without
/// letting a fixture that barely touches the span pass.
const MIN_DISTINCT_DECK_CELLS: usize = 6;

// Historical schema166 receipts: signed health cannot reconstruct the old
// current/max fold. These values record provenance, not current projections.
mod schema166_receipt {
    /// Committed final-hash baseline for the recorded bridge crossing.
    ///
    /// **This is a Rust-vs-prior-Rust regression ratchet, NOT gamemd parity
    /// evidence.** AGENTS.md is explicit that replay fixtures and Rust-derived
    /// hashes are regression ratchets; only machine-derived goldens (binary
    /// emulation, live capture, retail bytes) are parity references. What this
    /// constant proves is that the committed crossing — path, per-tick positions,
    /// `on_bridge`, `bridge_occupancy`, deck heights, and every other hashed field —
    /// did not move without someone noticing.
    ///
    /// Captured from the first green run of this fixture. Re-baseline at most once
    /// per behavior-bearing change, with a one-line documented reason, and only
    /// after the coverage tripwires below still pass — a baseline over a tank that
    /// stopped crossing is worthless.
    ///
    /// It has its own name on purpose: `GLOBAL_HARNESS_FINAL_HASH` and the other
    /// committed goldens are shared, coordination-gated constants and are not
    /// touched by this file.
    /// Re-baselined for TechnoClass::TechnoClass @ 0x006F2B90: the two authored
    /// Technos now consume and persist the raw Scenario words written at
    /// 0x006F3254. Path nodes, visited cells, bridge tripwires, and record/replay
    /// equality remain exact; this is a Rust regression ratchet, not parity evidence.
    /// Re-baselined 2026-08-30 for GSI-04.03 Drive/Ship slope payload ownership.
    /// The ordered path, all 12 visited cells, bridge height/occupation tripwires,
    /// all three RNG streams, and tick-for-tick replay remain exact; only the
    /// hash composition moved from retired body-rocking bytes to the locomotor.
    /// Re-baselined 2026-08-30 for v107's Spark shared-dummy tag plus level/slope
    /// folds. The path, bridge tripwires, and RNG streams remain byte-identical.
    /// Re-baselined 2026-08-30 for v110's unconditional ordered BasePlan authority.
    /// The dedicated pre-v110 probe below reproduces the prior baseline exactly;
    /// path, bridge tripwires, RNG streams, and tick-for-tick replay remain exact.
    /// Re-baselined 2026-09-01 for v114's unconditional raw 256-slot crate authority.
    /// The dedicated pre-v114 probe reproduces the prior current baseline exactly;
    /// the same crossing, tripwires, streams, and replay equality remain exact.
    /// Re-baselined 2026-09-01 for v115's retained wall-neighbor count authority mode and
    /// shared-dummy overlay identity/state folds. The dedicated pre-v115 probe reproduces the
    /// prior current baseline exactly; this fixture builds a legacy `None`-count grid, so only
    /// current-schema composition moved.
    // Re-baselined 2026-09-02 for the native tiberium queue store (OQ-38, bridge transaction 3
    // slice D): every class now carries the native entry array, float min-heap, capacity, and
    // `native_rect`, rebuilds walk `CellIterator` order, and spread admission applies the
    // `FirstObject` occupier gate. This is behavior-bearing on every fixture with ore, so the
    // historical probes move as well; the RNG stream tuple and tick-for-tick record/replay
    // equality remain exact.
    // 2026-09-02 veterancy effects (GSI-08.12): `TechnoClass+0x13C` is now written by the
    // first `AI_Update` promotion sample (-1 -> GetVeterancyLevel code), so every live
    // object's hashed `veterancy_rank_cache` moved. Composition of the hash is unchanged
    // and the RNG stream pins held (FINAL_STREAM_STATES), so no cadence or draw moved.
    // Merge 2026-09-02: main's veterancy re-baseline and its queue-store re-baseline
    // both moved these constants, so the three historical probes below are main's
    // composed measurement, unchanged by this branch.
    // 2026-09-08 ramp-height writer: final entity 1 gains exact Z Some(416).
    // Clearing ONLY that field reproduces every parent (588f4079) hash probe; final
    // entity/RNG comparison has no other differences. Route/deck and per-tick
    // replay checks remain active. RAMP_UNIT_HEIGHT_GHIDRA_REPORT.md records scope.
    const BRIDGE_HARNESS_PRE_BASE_PLAN_V110_HASH: u64 = 0xCAE2_8471_3B63_14DB;
    const BRIDGE_HARNESS_PRE_CRATE_AUTHORITY_V114_HASH: u64 = 0x7BEE_3E9D_B148_B70A;
    const BRIDGE_HARNESS_PRE_WALL_RUNTIME_V115_HASH: u64 = 0x3931_1CCA_29F3_5EE9;
    // Re-baselined 2026-09-02 for v117's disguise-detect folds (FogState's
    // `CellClass+0xAC[house]` counter plane and the cached `DetectDisguiseRange=`
    // deposit radius). The dedicated pre-v117 probe reproduces main's committed
    // current baseline exactly; this fixture stamps no disguise circle, so only
    // current-schema composition moved.
    const BRIDGE_HARNESS_PRE_DISGUISE_DETECT_V117_HASH: u64 = 0xCDA7_50B4_C529_175D;
    // Baselined 2026-09-06 for the v135 credit-income folds (GSI-09.01): every
    // entity folds its dead ProduceCash timer and two `None` drain-link halves.
    // The dedicated pre-v135 probe reproduces the prior committed final exactly
    // and every older probe plus the three RNG streams are unchanged, so this is
    // composition-only (no derrick, DrainWeapon or capture in this fixture).
    const BRIDGE_HARNESS_PRE_CREDIT_INCOME_V135_HASH: u64 = 0xC1FF_A576_23F4_E576;
    const BRIDGE_HARNESS_PRE_INFANTRY_TERMINAL_V136_HASH: u64 = 0x3C96_2071_1743_915F;
    // v136 adds the Infantry terminal-policy fold. The pre-v136 assertion below
    // reproduces the ramp branch's current baseline exactly; older probes and RNG pins
    // are unchanged. This is Rust hash-composition provenance, not native parity.
    // 2026-09-08 integration of main 33f36d79 with ramp branch 5f41f2ea:
    // the pre-v136 probe and all older probes pass, isolating this new current
    // value to main's retained InfantryTerminal fold; route and replay checks hold.
    // v142 adds retained shroud knowledge/admissions, Map240 and Foot sight clocks.
    // The dedicated pre-v142 assertion below retains this fixture's prior current
    // pin; older probes and existing replay/stream checks remain unchanged.
    // Receipt: .local/shroud-current-sight-full-v1.log (composition-only candidates).
    // 2026-09-13 ordinary TrackProcess host: reviewed current-cursor payment,
    // residual and synchronous arrival timing; historical projections also include
    // migrated Foot owners. See docs/research/TRACK_PROCESS_REPLAY_REGRESSION_NOTES.md
    // for baseline/candidate observations and native scope. These are Rust pins.
    const BRIDGE_HARNESS_FINAL_HASH_PRE_CELL_MEMBERSHIP_V159: u64 = 0xBACE_D587_09DA_4C4B;
    // Schema159 adds actual ordered Cell membership and exact Sight0 metadata.
    // The immediately preceding composition is asserted below against the old
    // current pin; all historical, replay/path and RNG tripwires remain intact.
    // This is a Rust hash-composition ratchet, not a new native golden.
    const BRIDGE_HARNESS_FINAL_HASH_PRE_FOOT_PATH_RUNTIME_V160: u64 = 0x8145_80FE_2764_0AFA;
    // Schema160 moves the two Foot path timers, blocked latch and dword retry count
    // into NavigationState::path_runtime and hashes that owner instead of the former
    // positional MovementTarget fields. The immediately preceding composition is
    // asserted below against the old current pin; every older probe, replay/path
    // and RNG tripwire remains intact. Rust hash-composition ratchet, not a native golden.
    // Re-pinned 2026-09-15 for the live bridge repair integration: raw infantry
    // occupation owners are the mark-time House index (InfantryClass::
    // MarkCellOccupancy 0x005217C0 -> Infantry virtual +0x38 -> House+0x30), no
    // longer the Rust entity id, which re-encodes the raw occupation fold under
    // every schema and moves every projection at once. Composition-only proof:
    // the owner-excluded probe asserted below equals main 595e3a88's value for
    // this fixture (receipt .local/harness-repin-20260915/owner-probe.txt).
    const BRIDGE_HARNESS_FINAL_HASH: u64 = 0xF199_544D_CF92_A3DA;
    const RAW_OWNER_EXCLUDED_V161: u64 = 0xA076_0B34_D476_12F6;
    const PRE_SUSTAINED_SIGHT_V142: u64 = 0x5FEF_1F84_890C_4984;
}

// Schema171: retained timers/track ownership and signed-health/hash composition.
// All201 baseline/candidate positions and RNG states matched. See the PR415
// section of docs/research/TRACK_PROCESS_REPLAY_REGRESSION_NOTES.md.
const BRIDGE_HARNESS_FINAL_HASH_PRE_RETIRED_TIBERIUM_STATE_V174: u64 = 878034761769452076;
// Schema174 removes folds instead of adding them: OreGrowthState's node-era
// scanner cursor, candidate lists and sample counters, and ProductionState's
// fallback ore overlay id. The pre-174 projection folds the values those fields
// held IN THIS FIXTURE (zero, empty, None): its node-era scan never advanced,
// and it never calls the spawner seeding, the one path that set the fallback
// id. It is not a general reconstruction; a scenario finalized by the map
// loader held Some(first TIB* id). The projection must still equal the previous
// current pin, asserted below. Rust hash-composition ratchet, not a native golden.
const BRIDGE_HARNESS_FINAL_HASH_PRE_CRATE_SPEED_V181: u64 = 6938168863139048597;
// v181 folds Foot+580, including default1.0. The pre-181 assertion below
// reproduces the previous whole fixture hash; path and RNG pins are unchanged.
const BRIDGE_HARNESS_FINAL_HASH_PRE_DISPLAY_LAYERS_V182: u64 = 7561855454504905019;
// Snapshot182 adds ordered display vectors. The pre-182 projection below
// must reproduce the previous whole-fixture hash, including all RNG/state.
// Schema186 removes the always-None release-tail byte. No aircraft participate;
// pre186 below must reproduce the preceding full hash, with route/RNG unchanged.
// Schema187: the pre187 projection below preserves the preceding full pin.
// Schema189 folds retained Techno+3D4; Before(189) below reproduces v188.
// v190 adds saved Foot neighbor history. Before(190) reproduces the full v189
// fixture; its legacy grid has no retained plane. Route/RNG pins are unchanged.
// 2026-09-23 Drive/Ship Process path request (behavior, not composition):
// Drive/Ship orders from every producer (player, pursuit, rally, miners) are
// accepted by the Unit setter without an order-time A*, PowerOn or redirect.
// This fixture has no native zone topology, so the first Process searches in
// the legacy inline lane, not the Find_Path owner. The pins move with that
// state; the RNG stream pins, per-tick replay equality and route tripwires in
// this file are unchanged. The old values are in the commit that moved them.
// 2026-09-23 Drive/Ship same-call track-end continuation (behavior): when
// Process_Track(0) ends a track the same Process runs Process_Movement and
// Process_Track(1) (residual-only budget; its speed prefix runs again).
// First divergence from main: tick 30, the tank's first track end, now
// continuing into the next track in that Process. Measured against main: the
// speed prefix runs in both Process_Track calls of a track-end frame, so the
// early track ends (accelerating or cruising) leave the tank up to 4 leptons
// ahead; at the last one (tick 131), braking toward its destination, it
// brakes one step more, falls up to 21 leptons behind and arrives at tick 168
// instead of 165. The scenario-RNG draws are the same sequence, moved with
// the arrival's Guard mission, so the absolute RNG state pin changes too. Old
// values: the commit that moved them.
// Schema202 folds the object's rearm timer (TechnoClass+0x2EC, started at the
// construction frame as the constructor does) in place of the AttackTarget
// cooldown/burst-delay counters and the cloak copy: composition only.
// Before(202) reproduces the v201 pin; per-tick replay, the RNG stream pins and
// the route tripwires are unchanged.
// 2026-09-25 combat chain 1 (behavior, snapshot 204): first divergence from
// 947c7044 is frame 0. The map-placed infantryman now enters Guard on Unlimbo
// (InfantryClass::Enter_Idle_Mode 0x0051CBA0), so its Mission_Guard runs from
// frame 0 and draws its RandomRanged(0, 2) cadence; the Scenario stream is
// that draw ahead from then on. Main and mapgen streams, every entity's cell
// and health match 947c7044 on all 200 ticks (traced). Schema 204 adds the
// house ROF bias and bullet OnBridge folds. Old values: the commit that moved
// them.
// 2026-09-25 combat chain 2 (behavior, snapshot 205), traced against d02452cd:
// the vehicle now takes its idle mission on Unlimbo (UnitClass::Enter_Idle_Mode
// 0x00738970, Guard), so its Mission_Guard cadence draws from frame 0 and it
// starts its move one frame earlier: every cell is entered one tick sooner and
// the final cell and health are unchanged; main/mapgen streams unchanged. With
// that hook disabled every old pin reproduces. Old values: the moving commit.
// Schema207 folds every object's crash latch and its AI edge (Foot+0x425/
// +0x426) and a Fly's fall counter: composition only. Before(207) reproduces
// the v206 pin; per-tick replay and the route tripwires are unchanged.
const BRIDGE_HARNESS_FINAL_HASH: u64 = 0x8186_0D2B_AC68_26D5;
const BRIDGE_HARNESS_FINAL_HASH_PRE_AIRCRAFT_CRASH_V207: u64 = 0x9D26_63FE_1130_9E06;
const BRIDGE_HARNESS_FINAL_HASH_PRE_REARM_TIMER_V202: u64 = 0xD647_5869_EE05_41D7;
const BRIDGE_HARNESS_FINAL_HASH_PRE_AIRCRAFT_RELEASE_V186: u64 = 4350841866948950648;

fn bridge_ini() -> IniFile {
    // One armed ground vehicle and one distant infantryman on a second house, so
    // no side is defeated on frame one. Their ranges keep them out of each
    // other's reach for the whole run.
    IniFile::from_str(
        "[Clear]\nFoot=100%\nTrack=100%\nWheel=100%\nHover=100%\nFloat=0%\nAmphibious=100%\nBuildable=yes\n\n\
         [InfantryTypes]\n0=E1\n\n\
         [VehicleTypes]\n0=MTNK\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n\n\
         [E1]\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=M60\n\n\
         [MTNK]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
         [M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n\n\
         [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\n\
         [SA]\nVerses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%\n\n\
         [AP]\nVerses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%\n",
    )
}

fn bridge_rules() -> RuleSet {
    RuleSet::from_ini(&bridge_ini()).expect("bridge harness rules should parse")
}

/// Is `x` a gorge column — the low ground the span crosses?
fn is_gorge_column(x: u16) -> bool {
    (DECK_FIRST_X..=DECK_LAST_X).contains(&x)
}

/// Stamp the span into a fresh grid.
///
/// Geometry, west to east on row [`SPAN_Y`]:
///
/// ```text
///   ..14  |  15   | 16 .. 23  |  24   | 25 ..
///  approach  entry   8 deck      exit    far
///  plateau   ramp    cells       ramp    approach
///  level 4   lvl 4   level 0     lvl 4   level 4
/// ```
///
/// Everything off the row in columns 16..=23 is the gorge: level 0 and
/// **blocked**, so the span is the only crossing and A* cannot route around it.
/// Both banks are level 4, which is also what makes the crossing legal: the
/// runtime Enter predicate fires only on `dst_level == src_level - 4` AND
/// `dst.has_structural_bridge()`, so 0 == 4 - 4 on a structural deck cell.
fn bridge_grid() -> PathGrid {
    let mut grid = PathGrid::new(GRID_W, GRID_H);
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if is_gorge_column(x) {
                // Gorge floor: low ground, impassable, no bridge.
                grid.set_cell_for_test(x, y, GORGE_LEVEL, false, false);
                grid.set_blocked(x, y, true);
            } else {
                // Plateau: ordinary level-4 ground on both banks.
                grid.set_cell_for_test(x, y, APPROACH_LEVEL, false, false);
            }
        }
    }

    // Both ramps: bridge-walkable transition cells at plateau level that are NOT
    // structural (see ENTRY_RAMP_X).
    for ramp_x in [ENTRY_RAMP_X, EXIT_RAMP_X] {
        grid.set_bridge_cell_decoupled_for_test(
            ramp_x,
            SPAN_Y,
            APPROACH_LEVEL,
            false, // not structural
            true,  // bridge-walkable
            APPROACH_LEVEL,
            true, // bridgehead/transition
        );
    }

    // The deck itself: structural, bridge-walkable, over the gorge, with the
    // stored deck value at plateau level. The transition flag matches what the
    // map stamper writes for Anchor/Forward1/Opposite deck slots.
    for x in DECK_FIRST_X..=DECK_LAST_X {
        grid.set_bridge_cell_decoupled_for_test(
            x,
            SPAN_Y,
            GORGE_LEVEL,
            true, // structural deck
            true, // bridge-walkable
            APPROACH_LEVEL,
            true,
        );
    }

    grid
}

/// Native Cell486840 height must agree with the path/spawn levels. In
/// particular, the far-bank destination is ground level4 (416 leptons),
/// so terminal owner-Z comparison can complete the actual navigation order.
fn bridge_resolved_terrain(
    grid: &PathGrid,
    rules: &RuleSet,
) -> crate::map::resolved_terrain::ResolvedTerrainGrid {
    use crate::map::bridge_facts::{
        BRIDGE_FLAG_STRUCTURAL, BRIDGE_FLAG_TRANSITION, BridgeCellFacts,
    };
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::rules::terrain_rules::{LandType, TerrainClass};
    let clear_costs = rules
        .terrain_rules
        .semantics_for_land_type(LandType::Clear.as_index())
        .expect("fixture authors Clear terrain costs")
        .speed_costs;
    let mut cells = Vec::with_capacity(usize::from(GRID_W) * usize::from(GRID_H));
    for ry in 0..GRID_H {
        for rx in 0..GRID_W {
            let path = grid.cell(rx, ry).unwrap();
            let flags = if path.bridge_structural {
                BRIDGE_FLAG_STRUCTURAL
            } else {
                0
            } | if path.transition {
                BRIDGE_FLAG_TRANSITION
            } else {
                0
            };
            cells.push(ResolvedTerrainCell {
                rx,
                ry,
                source_tile_index: 0,
                source_sub_tile: 0,
                final_tile_index: 0,
                final_sub_tile: 0,
                is_wood_bridge_repair_tile: false,
                level: path.ground_level,
                filled_clear: false,
                tileset_index: None,
                land_type: 0,
                yr_cell_land_type: 0,
                slope_type: 0,
                template_height: 0,
                render_offset_x: 0,
                render_offset_y: 0,
                terrain_class: TerrainClass::Clear,
                speed_costs: clear_costs,
                is_water: false,
                is_cliff_like: false,
                is_rough: false,
                is_road: false,
                accepts_smudge: false,
                allows_tiberium: false,
                height_in_pixels: 0,
                variant: 0,
                has_ramp: false,
                canonical_ramp: None,
                ground_walk_blocked: !path.ground_walkable,
                terrain_object_blocks: false,
                terrain_object_occupation: None,
                overlay_blocks: false,
                overlay_zone_type: None,
                outside_playfield: false,
                zone_type: 0,
                base_ground_walk_blocked: !path.ground_walkable,
                base_build_blocked: !path.ground_walkable,
                base_land_type: 0,
                base_yr_cell_land_type: 0,
                base_terrain_class: TerrainClass::Clear,
                base_speed_costs: clear_costs,
                build_blocked: !path.ground_walkable,
                has_bridge_deck: path.bridge_walkable,
                bridge_walkable: path.bridge_walkable,
                bridge_transition: path.transition,
                bridge_deck_level: path.bridge_deck_level,
                bridge_layer: None,
                bridge_facts: BridgeCellFacts {
                    raw_flags: flags,
                    ..Default::default()
                },
                tube_index: None,
                radar_left: [0; 3],
                radar_right: [0; 3],
                has_damaged_data: false,
                bridgehead_anchor_class_at_load: None,
            });
        }
    }
    ResolvedTerrainGrid::from_cells(GRID_W, GRID_H, cells)
}

/// Terrain heights matching the grid, so a spawned object starts at its cell's
/// real level rather than 0.
fn bridge_heights() -> BTreeMap<(u16, u16), u8> {
    let mut heights = BTreeMap::new();
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            // Terrain only — the deck is not terrain, so the cells under the
            // span carry the gorge floor here exactly like the rest of the gorge.
            let level = if is_gorge_column(x) {
                GORGE_LEVEL
            } else {
                APPROACH_LEVEL
            };
            heights.insert((x, y), level);
        }
    }
    heights
}

fn unit(owner: &str, type_id: &str, cx: u16, cy: u16, cat: EntityCategory) -> MapEntity {
    MapEntity {
        owner: owner.to_string(),
        type_id: type_id.to_string(),
        health: 256,
        cell_x: cx,
        cell_y: cy,
        facing: 64,
        category: cat,
        sub_cell: 0,
        veterancy: 0,
        high: false,
        mission: None,
        recruitable_a: true,
        recruitable_b: true,
        structure_upgrades: [None, None, None],
    }
}

/// Spawn order fixes stable ids: 1 = the crossing tank, 2 = a far-away Soviet
/// rifleman that exists only so neither house is defeated at once.
fn seed_bridge_scenario(sim: &mut Simulation, rules: &RuleSet, heights: &BTreeMap<(u16, u16), u8>) {
    sim.resolved_terrain = Some(bridge_resolved_terrain(&bridge_grid(), rules));
    // Storage dimensions are independent of the isometric Map Size diamond.
    // This narrow synthetic playfield retains the original route and distant
    // actor while supplying the mandatory mode-one constructor input.
    sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds::from_raw_local_size(
        16,
        64,
        [2, 2, 12, 56],
    ));
    for (rx, ry) in (APPROACH_A_X..=APPROACH_B_X)
        .map(|rx| (rx, SPAN_Y))
        .chain(std::iter::once((58, 58)))
    {
        assert!(
            crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                (i32::from(rx), i32::from(ry)),
                sim.playfield_bounds,
                sim.resolved_terrain.as_ref(),
            ),
            "fixture actor/route cell ({rx},{ry}) must be in the native playfield"
        );
    }
    // Exercise the complete same admission used by normal Unit construction:
    // configured bounds/terrain, authored speed row, occupants and raw masks.
    assert!(
        crate::sim::production::produced_unit_unlimbo_entry_at_resolved_cell(
            sim,
            rules,
            "Americans",
            "MTNK",
            TANK_ID,
            0,
            (APPROACH_A_X, SPAN_Y),
            None,
        )
        .exact_zero_layer()
        .is_some(),
        "fixture must satisfy normal Unit admission"
    );
    sim.spawn_from_map(
        &[
            unit(
                "Americans",
                "MTNK",
                APPROACH_A_X,
                SPAN_Y,
                EntityCategory::Unit,
            ), // 1
            unit("Soviet", "E1", 58, 58, EntityCategory::Infantry), // 2
        ],
        Some(rules),
        heights,
    );
    assert!(
        sim.substrate.entities.get(TANK_ID).is_some(),
        "fixture tank must pass normal resolved-terrain constructor admission"
    );
}

const TANK_ID: u64 = 1;

/// One scripted order: drive the tank from the near approach to the far one.
fn bridge_script() -> Vec<(u64, Command)> {
    vec![(
        2,
        Command::Move {
            entity_id: TANK_ID,
            target_rx: APPROACH_B_X,
            target_ry: SPAN_Y,
            queue: false,
            group_id: None,
        },
    )]
}

fn due_commands(sim: &Simulation, script: &[(u64, Command)], tick: u64) -> Vec<CommandEnvelope> {
    let owner = sim.interner.get("Americans").expect("Americans interned");
    script
        .iter()
        .filter(|(t, _)| *t == tick + 1)
        .map(|(t, c)| CommandEnvelope::new(owner, *t, c.clone()))
        .collect()
}

/// One committed frame of observed mover state.
#[derive(Debug, Clone, Copy)]
struct CrossingFrame {
    tick: u64,
    cell: (u16, u16),
    z: u8,
    on_bridge: bool,
    occupancy_deck: Option<u8>,
    terrain_level: i16,
    structural: bool,
}

impl CrossingFrame {
    /// `position.z == own cell's signed terrain level + (on_bridge ? 4 : 0)`.
    fn expected_z(&self) -> i16 {
        self.terrain_level + if self.on_bridge { DECK_LEVEL_DELTA } else { 0 }
    }

    fn holds_invariant(&self) -> bool {
        i16::from(self.z as i8) == self.expected_z()
    }
}

#[test]
fn bridge_crossing_replay_is_deterministic_and_baseline_stable() {
    let rules = bridge_rules();
    let heights = bridge_heights();
    let grid = bridge_grid();
    let script = bridge_script();

    // ---- Record pass: drive the crossing through the live advance_tick path. ----
    let mut rec = Simulation::with_seed(BRIDGE_HARNESS_SEED);
    seed_bridge_scenario(&mut rec, &rules, &heights);
    let mut log = ReplayLog::new(ReplayHeader {
        version: 1,
        pixel_conversion_bounds: rec.session.pixel_conversion_bounds,
        tick_hz: 15,
        seed: BRIDGE_HARNESS_SEED,
        map_name: "bridge_parity_harness".to_string(),
        rules_hash: 0,
    });

    let mut frames: Vec<CrossingFrame> = Vec::with_capacity(BRIDGE_HARNESS_TICKS as usize);
    let mut order_accepted_path: Option<usize> = None;
    for tick in 0..BRIDGE_HARNESS_TICKS {
        let due = due_commands(&rec, &script, tick);
        let result = rec.advance_tick(
            &due,
            Some(&rules),
            &heights,
            Some(&grid),
            None,
            BRIDGE_HARNESS_TICK_MS,
        );
        let entity = rec
            .substrate
            .entities
            .get(TANK_ID)
            .expect("the crossing tank must stay alive for the whole run");
        if order_accepted_path.is_none() {
            order_accepted_path = entity.movement_target.as_ref().map(|t| t.path.len());
        }
        let cell = (entity.position.rx, entity.position.ry);
        let facts = grid
            .cell(cell.0, cell.1)
            .copied()
            .unwrap_or_else(|| panic!("the tank left the path grid at {cell:?}"));
        frames.push(CrossingFrame {
            tick,
            cell,
            z: entity.position.z,
            on_bridge: entity.on_bridge,
            occupancy_deck: entity.bridge_occupancy.map(|occ| occ.deck_level),
            terrain_level: facts.signed_level(),
            structural: facts.has_structural_bridge(),
        });
        log.record_tick(tick, due, result.state_hash);
    }

    // ---- Coverage tripwires. These are the point of the fixture: without them
    // the baseline below would happily pin a tank that never moved. ----
    let visited: Vec<(u16, u16)> = {
        let mut seen: Vec<(u16, u16)> = Vec::new();
        for frame in &frames {
            if seen.last() != Some(&frame.cell) {
                seen.push(frame.cell);
            }
        }
        seen
    };
    println!(
        "[bridge parity] ordered path nodes: {order_accepted_path:?}; cells visited: {visited:?}"
    );
    // One line per cell the tank entered, not per frame: the run is ~200 frames
    // and a failure report has to stay readable.
    let mut previous: Option<(u16, u16)> = None;
    for frame in &frames {
        if previous == Some(frame.cell) {
            continue;
        }
        previous = Some(frame.cell);
        println!(
            "  tick {:4} cell {:?} z={} on_bridge={} occ_deck={:?} terrain={} deck={} expect_z={}",
            frame.tick,
            frame.cell,
            frame.z,
            frame.on_bridge,
            frame.occupancy_deck,
            frame.terrain_level,
            frame.structural,
            frame.expected_z(),
        );
    }

    assert!(
        order_accepted_path.is_some(),
        "the ordinary Command::Move onto the span was never turned into a path — \
         a player could not cross at all. Cells visited: {visited:?}"
    );

    // 1. The Enter predicate must have fired: on_bridge became true at some tick.
    assert!(
        frames.iter().any(|f| f.on_bridge),
        "on_bridge never became true — BridgeStateUpdate::Set never fired, so the \
         tank did not enter the span. Cells visited: {visited:?}"
    );

    // 2. The tank stood on a real run of the deck, not just its first cell.
    let deck_cells: BTreeSet<(u16, u16)> = frames
        .iter()
        .filter(|f| f.structural)
        .map(|f| f.cell)
        .collect();
    assert!(
        deck_cells.len() >= MIN_DISTINCT_DECK_CELLS,
        "the tank stood on only {} distinct structural deck cell(s), need at least \
         {MIN_DISTINCT_DECK_CELLS}: {deck_cells:?}. Cells visited: {visited:?}",
        deck_cells.len(),
    );

    // 3. Every deck frame is at deck height, flagged on-bridge, and agrees with
    //    its own BridgeOccupancy — never dropped to the gorge floor underneath.
    for frame in frames.iter().filter(|f| f.structural) {
        assert!(
            frame.on_bridge,
            "on structural deck cell {:?} the tank was not marked on_bridge: {frame:?}",
            frame.cell
        );
        assert_eq!(
            i16::from(frame.z as i8),
            frame.terrain_level + DECK_LEVEL_DELTA,
            "deck height broke at {:?}: z must be terrain + {DECK_LEVEL_DELTA}: {frame:?}",
            frame.cell
        );
        assert_ne!(
            i16::from(frame.z as i8),
            i16::from(GORGE_LEVEL as i8),
            "the tank dropped to the gorge floor under the span at {:?}: {frame:?}",
            frame.cell
        );
        assert_eq!(
            frame.occupancy_deck,
            Some(frame.z),
            "BridgeOccupancy.deck_level disagrees with position.z at {:?}: {frame:?}",
            frame.cell
        );
    }

    // 4. The invariant holds on the off-bridge frames too — approaches and ramps.
    let violations: Vec<&CrossingFrame> = frames.iter().filter(|f| !f.holds_invariant()).collect();
    assert!(
        violations.is_empty(),
        "position.z left the native height model on {} frame(s); first: {:?}",
        violations.len(),
        violations.first(),
    );

    // 5. The tank finished the crossing: it reached the far approach and the
    //    Exit arm cleared on_bridge again.
    let last = frames.last().copied().expect("at least one frame recorded");
    assert_eq!(
        last.cell,
        (APPROACH_B_X, SPAN_Y),
        "the tank never reached the far approach; it ended at {:?}. Cells visited: {visited:?}",
        last.cell
    );
    assert!(
        !last.on_bridge,
        "the tank is still flagged on_bridge on the far approach: {last:?}"
    );
    assert_eq!(
        last.occupancy_deck, None,
        "BridgeOccupancy survived the Exit transition: {last:?}"
    );

    let arrived = rec.substrate.entities.get(TANK_ID).unwrap();
    assert!(
        arrived.navigation.nav_com.is_none(),
        "arrival must clear NavCom"
    );
    assert!(
        arrived
            .drive_locomotion
            .as_ref()
            .unwrap()
            .destination
            .is_none(),
        "arrival must clear the class destination"
    );
    assert!(
        arrived.drive_locomotion.as_ref().unwrap().head_to.is_none(),
        "arrival must retire the paid track head"
    );
    assert!(
        arrived.movement_target.is_none(),
        "arrival must retire the completed path"
    );

    // ---- Replay pass: fresh sim, real ReplayRunner, tick-for-tick equality. ----
    let mut rep = Simulation::with_seed(BRIDGE_HARNESS_SEED);
    seed_bridge_scenario(&mut rep, &rules, &heights);
    let replayed = ReplayRunner::run_fixture_with_overlay_registry(
        &mut rep,
        &log,
        Some(&rules),
        &heights,
        Some(&grid),
        None,
        BRIDGE_HARNESS_TICK_MS,
    );
    assert_eq!(
        replayed.len(),
        log.ticks.len(),
        "replay tick count must match record"
    );
    for (i, h) in replayed.iter().enumerate() {
        assert_eq!(
            *h, log.ticks[i].state_hash,
            "intra-run determinism: replay tick {i} hash must equal the recorded hash"
        );
    }

    // Actual replay/RNG gates remain; pre169 recipes cannot recover old health.
    assert_eq!(
        rec.scenario_rng.logical_state(),
        rep.scenario_rng.logical_state()
    );
    assert_eq!(rec.main_rng.logical_state(), rep.main_rng.logical_state());
    assert_eq!(
        rec.mapgen_rng.logical_state(),
        rep.mapgen_rng.logical_state()
    );
    assert_eq!(
        (
            rep.scenario_rng.state(),
            rep.main_rng.state(),
            rep.mapgen_rng.state()
        ),
        (
            0x1E0D_2428_10B3_F39F,
            0x9C68_CC8B_9F2C_82ED,
            0x1CE8_1848_7043_6163
        ),
        "bridge absolute RNG states changed",
    );
    let final_hash = *replayed.last().expect("at least one tick replayed");
    assert_eq!(
        rep.state_hash_with_schema(super::hash_schema::HashSchema::Before(174)),
        BRIDGE_HARNESS_FINAL_HASH_PRE_RETIRED_TIBERIUM_STATE_V174,
        "the pre-174 composition must reproduce the previous current pin"
    );
    assert_eq!(
        rep.state_hash_with_schema(super::hash_schema::HashSchema::Before(181)),
        BRIDGE_HARNESS_FINAL_HASH_PRE_CRATE_SPEED_V181,
        "excluding only Foot+580 must preserve the pre-181 fixture"
    );
    assert_eq!(
        rep.state_hash_with_schema(super::hash_schema::HashSchema::Before(182)),
        BRIDGE_HARNESS_FINAL_HASH_PRE_DISPLAY_LAYERS_V182,
        "excluding only display vectors must preserve the pre-182 fixture"
    );
    assert_eq!(
        rep.state_hash_with_schema(super::hash_schema::HashSchema::Before(186)),
        BRIDGE_HARNESS_FINAL_HASH_PRE_AIRCRAFT_RELEASE_V186,
        "restoring only the absent release-tail fold must reproduce the prior fixture"
    );
    assert_eq!(
        rep.state_hash_with_schema(super::hash_schema::HashSchema::Before(187)),
        13132899633947150139,
        "schema187 only replaces zero remaining-shot fields with the retained index in this fixture"
    );
    assert_eq!(
        rep.state_hash_with_schema(super::hash_schema::HashSchema::Before(189)),
        17836007346589956114,
        "v189 adds only the retained Techno+3D4 hash fold"
    );
    assert_eq!(
        rep.state_hash_with_schema(super::hash_schema::HashSchema::Before(190)),
        16757600261337163915,
        "v190 changes only the Foot neighbor-history hash composition in this fixture"
    );
    assert_eq!(
        rep.state_hash_with_schema(super::hash_schema::HashSchema::Before(202)),
        BRIDGE_HARNESS_FINAL_HASH_PRE_REARM_TIMER_V202,
        "v202 only folds the object's rearm timer in place of the target's counters"
    );
    assert_eq!(
        rep.state_hash_with_schema(super::hash_schema::HashSchema::Before(207)),
        BRIDGE_HARNESS_FINAL_HASH_PRE_AIRCRAFT_CRASH_V207,
        "v207 only folds the crash latch, its AI edge and the Fly fall counter"
    );
    assert_eq!(
        final_hash, BRIDGE_HARNESS_FINAL_HASH,
        "committed bridge-harness baseline drifted. Do not paste the observed value \
         until the tripwires above are still green and you can say which behavior \
         moved; this constant is a Rust-vs-prior-Rust ratchet, not gamemd evidence"
    );
}
