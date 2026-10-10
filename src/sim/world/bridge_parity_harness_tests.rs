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
//! (`ObjectClass::SetHeight` @ `0x005F5FA0`, read back by
//! `ObjectClass::GetHeight` @ `0x005F5F40`):
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
use std::collections::BTreeSet;

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

/// Synthetic theater tile index of the first high-bridge (BridgeSet) tile.
const HIGH_BRIDGE_SET_START: u16 = 1000;
/// BridgeSet slot 6: start and end sub-tile 4, walk direction 2 (east)
/// (`HIGH_BRIDGE_*` tables in `sim::bridge_state`).
const HIGH_BRIDGE_EW_OFFSET: u16 = 6;
const HIGH_BRIDGE_EW_SUB_TILE: u8 = 4;

/// Distinct structural deck cells the tank must actually stand on. Eight exist;
/// requiring six leaves room for the sub-cell curve to skip an end cell without
/// letting a fixture that barely touches the span pass.
const MIN_DISTINCT_DECK_CELLS: usize = 6;

// Schema171: retained timers/track ownership and signed-health/hash composition.
// All201 baseline/candidate positions and RNG states matched. See the PR415
// section of docs/research/TRACK_PROCESS_REPLAY_REGRESSION_NOTES.md.
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
// The fixture then had no native zone topology, so the first Process searched
// in the since-removed legacy inline lane (see the 2026-10-01 entry below).
// The pins moved with that state; the RNG stream pins, per-tick replay
// equality and route tripwires in this file were unchanged. The old values
// are in the commit that moved them.
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
// Schema208 folds every object's crash latch and its AI edge (Foot+0x425/
// +0x426) and a Fly's fall counter: composition only. Before(208) reproduces
// the v207 pin (the ore-field schema moved nothing here);
// per-tick replay and the route tripwires are unchanged.
// 2026-09-26 retired weapon identity (snapshot 212, composition only): the
// entity hash no longer folds `current_weapon_ref`, the weapon id of the last
// live selection, so this one step re-pins every projection in this test.
// Nothing here ever selects a weapon, so the field held the constructor's
// `None` throughout; a pre-212 projection folding `None` could have kept this
// file's historical pins, but the global harness's per-fire values cannot be
// rebuilt, so none was added. Ceremony: with only that fold line deleted from
// the previous tree, these pins fail and nothing else in the lib suite moves;
// their `left` values are pasted here, and the full retirement reproduces
// every one. The absolute RNG pin, per-tick record/replay equality and the
// route tripwires are unchanged: the only change to these pins is the removed
// fold. The new Before(208) equals the old Before(202) pin, not a paste slip:
// at the end no object holds a target or a cloak, all were built at frame 0,
// and schemas 204-207 add nothing here (no house, bullet, anim or miner), so
// the dropped `None` and schema 202's zero rearm timer are both eight zero
// bytes in one run of zero bytes, which the unframed stream cannot tell apart.
// Old values: the commit that moved them.
// Schema217 composition only for this fixture: the native constructor cursor,
// native object IDs and retained fallback-cell Land now enter the hash. The
// repair diagnostic reproduced the prior current pin with Before(217), and
// all earlier projections, replay/behavior checks and RNG receipts still passed.
// These Rust regression receipts do not establish native gameplay parity.
// Schema220 retires the two VERA rally copies (the house's `rally_point` and
// each building's `rally_target`; the rally is the factory's ArchiveTarget):
// composition only, as no rally is set here. Before(220) folds their empty
// values and reproduces the prior current pin; every earlier projection,
// per-tick replay and the RNG receipts are unchanged.
// 2026-09-28 one body facing (snapshot 237, composition only; #580): each
// object's hash folds its body FacingClass (`+0x388`) in place of the retired
// 8-bit facing mirror, turn target and optional turn interpolator, and a
// building's `+0x388` moves from the turret slot into it, in every projection.
// No schema can rebuild the mirror or the target, so this one step re-pins
// every projection in this test. Ceremony: on origin/main 99935d1d and on this
// change, a probe hash folding no facing, and one folding only each object's
// body heading word at the hashed frame (the old tree's interpolator, else its
// mirror), matched at all 200 ticks, RNG streams included (the probe patch was
// not committed): every other fold and every object's heading are unchanged, so
// the only change to these pins is the fold. Drive's never-written turn target
// (`DriveTurnState`) left the fold in the same step: with its four default
// fields folded back in their old place, this change reproduced every
// facing-only pin (final 0x9FD9_D4EE_7F06_E1ED), RNG streams included. Old
// values: the commit that moved them.
// 2026-09-29 retired ground move phase (snapshot 244, composition only; #726):
// the locomotor's VERA-only `GroundMovePhase` and its stashed twin leave the
// object and piggyback folds in every projection; no schema can rebuild the
// retired value, so this one step re-pins every projection in this test.
// Ceremony: on the parent commit with only those two folds removed, and on
// this change, a soft-assert probe of every pin in this test printed identical
// values, and per-tick replay and the RNG receipts passed at all 200 ticks
// (the probe patch was not committed): the only change to these pins is the
// fold. Old values: the commit that moved them.
// 2026-09-29 one locomotor enum (snapshot 248, composition only; #725):
// LocomotorKind keeps only the eight installable classes, so the active kind,
// the installed slot and the stash fold renumbered discriminants, and the
// dormant Tunnel and DropPod states leave the object and payload folds, in
// every projection. No schema rebuilds the old numbering, so this one step
// re-pins every projection in this test. Ceremony: on the parent commit and on
// this change, a probe printing every object's position, exact Z, health,
// mission, NavCom, attack and movement targets, locomotor kind and layer and
// all three RNG states matched at all 200 ticks (the probe patch was not
// committed). Old values: the commit that moved them.
// 2026-09-30 no cached GetCurrentSpeed (composition only; #844): the
// Foot owner's Rust-only `cached_current_speed` leaves the object fold.
// Ceremony: the parent commit with only that fold removed printed this
// exact value, as this change does, with the RNG pins above unchanged
// (the probe patch was not committed): the only change to this pin is
// the fold. Old value: the commit that moved it.
// 2026-09-30 native mission-site idle precedes the first Ready action; explicit
// GI ART supplies its actual sequence records. Captured actor and full-stream
// receipts in foot_bridge_layer.replay.json establish the changed inputs and
// draw sites. All 200 per-frame replay hashes, route/height/layer/occupancy gates
// and absolute RNG pins pass before this Rust regression hash is checked.
// This is a Rust replay pin, not native whole-world parity evidence.
// 2026-09-30 one locomotor object (snapshot 258, composition only; #680): the
// active locomotor and its piggyback stash hash through one fold of every
// LocomotorState field. The active fold gains BalloonHover, HoverAttack,
// SpeedType, MovementZone, the sub-cell destination and the Hover speed
// request and drops the installed slot, which is the stash's own kind; the
// stash drops its retired separators. Ceremony: the parent commit with only
// that fold changed printed this exact value, as this change does, with the
// RNG pins above unchanged (the probe patch was not committed): the only
// change to this pin is the fold. Old value: the commit that moved it.
// 2026-09-30 unread movement bookkeeping (snapshot 260, composition only;
// #685): the fold drops bridge_occupancy and the ground cell enter order;
// the enter-order counter (and so AirTracker order values) advances only
// for AirTracker entries; Foot+0x68B is write-1-only. Ceremony: the
// parent and this change, each with those five inputs removed from the
// hash, printed the same value, with the RNG pins above unchanged (the
// probe patch was not committed). Old value: the commit that moved it.
// Snapshot264: private native Stage and Infantry Doing/sequence timing.
// Complete before/candidate/final receipts in foot_bridge_layer.replay.json
// (techno_stage264_followup) attribute every changed actor field and retain
// all819 frame rows, all three RNG states and ordered raw draw callers.
// Incoming main preserves candidate state; its source-line changes remain
// recorded. This Rust replay pin does not establish native whole-world parity.
// Native Hover host (snapshot 269): the locomotor fold drops the three
// SimFixed Hover copies and hashes the whole HoverLocomotionClass object in
// place of its head. Ceremony: this change with the old composition (three
// zero words, head only; probe not committed) printed the previous pin, so
// no hashed state moved. Previous: 0x4477_C324_18ED_A27F.
// 2026-09-30 one FootClass::Mark owner (#922): Mark no longer writes the
// AircraftTracker, which native Mark never touches, so a ground object keeps
// its constructor-seeded enter order (its stable id) where the old lifecycle
// Mark reset it to 0 on every Foot and Building Mark. Ceremony, rerun after
// merging main: this change with only that reset restored printed the old
// value for all three replay pins (bridge, global, slice 6), with the RNG pins
// above unchanged (the probe patch was not committed). Previous:
// 0x6670_93CF_F88F_7008.
// 2026-10-01 legacy Drive/Ship lane removed (behavior): the Drive Process now
// always runs native Find_Path, whose zone precheck (Foot+2CC 0x004D3810 ->
// Map56D100, `ZoneGrid::can_reach_native`) refused this fixture: with no
// high-bridge tile no bridge record existed, so the two banks were separate
// zones and the tank idled on Guard. The fixture now supplies what a map load
// gives a high bridge: BridgeSet tiles on both bridgeheads, so the production
// record walk derives the one active record (15,20)-(24,20), plus Map Size
// and the zone rebuild over it; the crossing runs through native Find_Path.
// The absolute RNG pins, per-tick replay equality and every route/height/
// on_bridge tripwire pass unchanged. Previous: 0x14B8_172A_D6EA_A02C.
// 2026-10-01 Walk route owner (hash composition only): Walk keeps no
// MovementTarget route cells and no locomotor sub-cell destination, both of
// which the state hash folded. Ceremony: main and this change, each with only
// those fields dropped from the hash (probe not committed), printed the same
// value for all three replay pins (bridge, global, slice 6). Previous: 0xB592_E027_A510_DB45.
// Historical retained-path integration compared all201 main7412/candidate
// observations after omitting newly retained actor fields and RNG source
// locations; gameplay observations matched and only hash composition moved.
// That pre-#962/#963 candidate pin was 0xEB5C_F8B1_1E9B_7DC6. The bounded Rust
// receipt remains tools/spatial_oracle/astar_path_finishing_replay/receipt.json.
// Main963 retained-path composition: the same-binary control reproduces
// 0xCC11_DDD9_1928_916A by omitting only navigation history, House threat, Foot530
// and cached Techno508 hash feeds. All201 control/current observations
// match exactly except tick hashes, including full actor state and all
// three RNG streams/draws/caller positions. The temporary gate was removed.
// Rust-only receipt: tools/spatial_oracle/astar_path_finishing_replay/main963/receipt.json.
// 2026-10-02 Building body has one retained owner (hash composition):
// same-binary control restores only the former two absent Up/Down
// folds and reproduces the preceding pin. All819 control/current
// boundaries match after only tick hashes are omitted: full actors,
// commands, lifecycle/Logic and three RNG streams/draws/callers.
// Rust-only receipt: tools/spatial_oracle/building_construction_replay/receipt.json.
// Previous: 0x8DE16E7B3B6FE661. Native proof is the separate construction corpus.
// 2026-10-02 Engineer/House retained state (Rust-only hash composition):
// The same-binary four-feed control reproduces the preceding main pin.
// All819 complete current/control boundaries match except tick hashes,
// including House/Factory state and three full RNG streams/draws/callers.
// Scope and receipt: tools/spatial_oracle/engineer_repair_replay/receipt.json.
// 2026-10-02 Unit deployment/body ownership: snapshot280 replaces the legacy
// DeployPhase hash with Techno130/134, adds Unit6E0, and advances the existing
// Foot538 counter for voxel Units. Route, height and RNG tripwires above the
// final assertion remain unchanged. The same-binary hash control reproduces
// incoming E3FD9DD363C98F82; all 201 off/on boundaries differ only in tick hash.
// Receipt: tools/spatial_oracle/unit_simple_deploy_replay/receipt.json.
// This is Rust attribution; unit_simple_deploy separately pins native cadence.
// Shared Techno Door hash composition after main339b57d18 integration:
// one diagnostic binary restores incoming F6E02080D0BE29C1 when only the Door
// hash feed is omitted. Behavior and RNG assertions reach the final pin in
// both modes. The temporary control is removed; native expected values stay
// unchanged. Receipt: tools/spatial_oracle/anytown_damage/unit_unlimbo.md.
// 2026-10-02 IFV owner migration: u8 last-shot hashing becomes signed i32
// current-weapon hashing and the redundant None override feed is removed.
// All 201 recorded boundaries retain gameplay/RNG after guarded field migration;
// tools/spatial_oracle/ifv_turret_replay/receipt.json records this Rust attribution.
// Shared Door composition after main1009: same test binary, 201 complete
// off/on boundaries equal except tick_result.state_hash. Omitting only Door
// restores incoming main; the temporary control is removed. Bounded Rust
// attribution: tools/spatial_oracle/anytown_damage/unit_unlimbo.md.
// Snapshot286: the entity-level absent Teleport tag is removed; the complete
//class payload owns its fold. Restoring only that old0 tag recovers the prior
//hash exactly, with all existing pose/gameplay and absolute RNG tripwires intact.
// See tools/spatial_oracle/infantry_teleport_destination.md. Rust hash ratchet only.
// Snapshot288: the complete Drive/Ship payload owns its retained Option fold.
// Same-binary old-feed control recovers514E76C3355546F9; all201 complete rows
// match except state_hash, including all3 RNG states. Control removed.
// Rust attribution: tools/spatial_oracle/drive_instance_replay/other/bridge/receipt.json.
// Rocket locomotor port: the entity-level absent rocket_state tag is removed;
// the Rocket payload owns its fold. Restoring only that old 0 tag recovers
// incoming 4C4F372E184105B6 in the same test binary. Control removed.
// Snapshot303: HoverAttack, target_pad and pad_index leave the hash. A control
// binary (main with a hash that skips only those fields) gives this value in
// the same test, with every tripwire above green. Control removed.
// Snapshot306: the legacy CellClass counters, flags, occlusion caches and
// visibility marks leave the hash (the ground bits stay), and SightAdmission
// drops fog_of_war. A control binary (main with a hash that skips only those
// fields) gives this value in the same test, with every tripwire above green.
// Control removed.
// Snapshot307: the session's LocalSize copy leaves the hash. A control binary
// (main with a hash that skips only that tuple) gives this value in the same
// test, with every tripwire above green. Control removed.
// Snapshot318 hashes MoveSound's signed i32 countdown instead of u8. The
// fixture's vectors are empty, but qualifying Foot visits retain countdown3
// while inactive (4DAA87; foot_move_sound.json empty_qualifies). No sound is
// selected and no Main draw is paid. This remains a Rust regression pin.
// Snapshot320 retains native Techno constructor disguise identity/timer state
// for every actor, replacing the absent component. These fixtures acquire no
// disguise; their gameplay and absolute RNG pins remain the independent gates.
const BRIDGE_HARNESS_FINAL_HASH: u64 = 0xE598_15E1_D803_4937;

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
         [E1]\nImage=GI\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=M60\n\n\
         [MTNK]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
         [M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n\n\
         [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\n\
         [SA]\nVerses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%\n\n\
         [AP]\nVerses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%\n",
    )
}

fn bridge_rules() -> RuleSet {
    let ini = bridge_ini();
    // Explicit authored GI inputs use the production fixed-ART reader and binder;
    // zero-count constructor records do not admit native Ready/idle actions.
    let art = IniFile::from_str(crate::rules::retail_ini_fixture::GI_ART_EXCERPT);
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
    );
    rules
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
            let mut cell = ResolvedTerrainCell {
                level: path.ground_level,
                tileset_index: None,
                speed_costs: clear_costs,
                ground_walk_blocked: !path.ground_walkable,
                base_ground_walk_blocked: !path.ground_walkable,
                base_build_blocked: !path.ground_walkable,
                base_terrain_class: TerrainClass::Clear,
                base_speed_costs: clear_costs,
                // Production bridgeheads carry no deck: only 0x100 cells do.
                has_bridge_deck: path.bridge_walkable && flags & BRIDGE_FLAG_STRUCTURAL != 0,
                bridge_walkable: path.bridge_walkable,
                bridge_transition: path.transition,
                bridge_deck_level: path.bridge_deck_level,
                bridge_facts: BridgeCellFacts {
                    raw_flags: flags,
                    ..Default::default()
                },
                ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
            };
            // Both bridgeheads carry a theater high-bridge tile, as a map's
            // IsoMapPack5 does: the map-load record walk (ComputeBridgeZones
            // 0x0056D6E0, `record_scan::compute_bridge_endpoints`) starts at
            // the west head and ends at the east one.
            if ry == SPAN_Y && (rx == ENTRY_RAMP_X || rx == EXIT_RAMP_X) {
                cell.final_tile_index = i32::from(HIGH_BRIDGE_SET_START + HIGH_BRIDGE_EW_OFFSET);
                cell.final_sub_tile = HIGH_BRIDGE_EW_SUB_TILE;
            }
            cells.push(cell);
        }
    }
    let mut terrain = ResolvedTerrainGrid::from_cells(GRID_W, GRID_H, cells);
    terrain.test_set_high_bridge_set_starts(Some(HIGH_BRIDGE_SET_START), None);
    terrain
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
        structure_ai_sellable: false,
        structure_ai_repairable: false,
    }
}

/// Spawn order fixes stable ids: 1 = the crossing tank, 2 = a far-away Soviet
/// rifleman that exists only so neither house is defeated at once.
fn seed_bridge_scenario(sim: &mut Simulation, rules: &RuleSet) {
    sim.resolved_terrain = Some(bridge_resolved_terrain(&bridge_grid(), rules));
    // Storage dimensions are independent of the isometric Map Size diamond.
    // This narrow synthetic playfield retains the original route and distant
    // actor while supplying the mandatory mode-one constructor input.
    sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds::from_raw_local_size(
        16,
        64,
        [2, 2, 12, 56],
    ));
    // A Drive Process searches: the bridge records, zones and Map Size a map
    // load installs.
    sim.playfield_size_height = Some(64);
    sim.bridge_state = Some(
        crate::sim::bridge_state::BridgeRuntimeState::from_resolved_terrain_with_map_size(
            sim.resolved_terrain.as_ref().unwrap(),
            true,
            rules.bridge_rules.strength,
            (16, 64),
        ),
    );
    let records = sim.bridge_state.as_ref().unwrap().endpoint_records();
    assert_eq!(records.len(), 1, "one high-bridge record: {records:?}");
    assert!(records[0].is_high() && records[0].active);
    assert_eq!(
        (records[0].endpoint_a, records[0].endpoint_b),
        ((ENTRY_RAMP_X, SPAN_Y), (EXIT_RAMP_X, SPAN_Y))
    );
    assert!(sim.rebuild_dynamic_navigation(rules));
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
    terrain_level: i16,
    structural: bool,
}

impl CrossingFrame {
    /// `position.z == own cell's signed terrain level + (on_bridge ? 4 : 0)`.
    fn expected_z(&self) -> i16 {
        self.terrain_level
            + if self.on_bridge {
                crate::util::lepton::BRIDGE_DECK_HEIGHT_LEVELS as i16
            } else {
                0
            }
    }

    fn holds_invariant(&self) -> bool {
        i16::from(self.z as i8) == self.expected_z()
    }
}

#[test]
fn bridge_crossing_replay_is_deterministic_and_baseline_stable() {
    let rules = bridge_rules();
    let grid = bridge_grid();
    let script = bridge_script();

    // ---- Record pass: drive the crossing through the live advance_tick path. ----
    let mut rec = Simulation::with_seed(BRIDGE_HARNESS_SEED);
    let mut diagnostic = super::global_parity_harness_tests::replay_diagnostic_file("bridge");
    let mut seed = || seed_bridge_scenario(&mut rec, &rules);
    let (_, seed_draws) = if diagnostic.is_some() {
        crate::sim::rng::trace_draws(seed)
    } else {
        (seed(), Vec::new())
    };
    super::global_parity_harness_tests::record_replay_diagnostic(
        &mut diagnostic,
        &rec,
        None,
        &[],
        &seed_draws,
    );
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
        let mut advance = || {
            rec.advance_tick(
                &due,
                Some(&rules),
                Some(&grid),
                None,
                BRIDGE_HARNESS_TICK_MS,
            )
        };
        let (result, draws) = if diagnostic.is_some() {
            crate::sim::rng::trace_draws(advance)
        } else {
            (advance(), Vec::new())
        };
        super::global_parity_harness_tests::record_replay_diagnostic(
            &mut diagnostic,
            &rec,
            Some(&result),
            &due,
            &draws,
        );
        let entity = rec
            .substrate
            .entities
            .get(TANK_ID)
            .expect("the crossing tank must stay alive for the whole run");
        // Nodes of the route the first Find_Path installed: the start cell
        // plus one Foot+5E0 word per later cell.
        if order_accepted_path.is_none()
            && entity.movement_target.is_some()
            && !entity.navigation.path_replay.directions.is_empty()
        {
            order_accepted_path = Some(entity.navigation.path_replay.directions.len() + 1);
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
            "  tick {:4} cell {:?} z={} on_bridge={} terrain={} deck={} expect_z={}",
            frame.tick,
            frame.cell,
            frame.z,
            frame.on_bridge,
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

    // 3. Every deck frame is at deck height and flagged on-bridge — never
    //    dropped to the gorge floor underneath.
    for frame in frames.iter().filter(|f| f.structural) {
        assert!(
            frame.on_bridge,
            "on structural deck cell {:?} the tank was not marked on_bridge: {frame:?}",
            frame.cell
        );
        assert_eq!(
            i16::from(frame.z as i8),
            frame.terrain_level + crate::util::lepton::BRIDGE_DECK_HEIGHT_LEVELS as i16,
            "deck height broke at {:?}: z must be terrain + deck levels: {frame:?}",
            frame.cell
        );
        assert_ne!(
            i16::from(frame.z as i8),
            i16::from(GORGE_LEVEL as i8),
            "the tank dropped to the gorge floor under the span at {:?}: {frame:?}",
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

    let arrived = rec.substrate.entities.get(TANK_ID).unwrap();
    assert!(
        arrived.navigation.nav_com.is_none(),
        "arrival must clear NavCom"
    );
    assert!(
        arrived
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .unwrap()
            .destination()
            .is_none(),
        "arrival must clear the class destination"
    );
    assert!(
        arrived
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .unwrap()
            .head_to()
            .is_none(),
        "arrival must retire the paid track head"
    );
    assert!(
        arrived.movement_target.is_none(),
        "arrival must retire the completed path"
    );

    // ---- Replay pass: fresh sim, real ReplayRunner, tick-for-tick equality. ----
    let mut rep = Simulation::with_seed(BRIDGE_HARNESS_SEED);
    seed_bridge_scenario(&mut rep, &rules);
    let replayed = ReplayRunner::run_fixture_with_overlay_registry(
        &mut rep,
        &log,
        Some(&rules),
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
    println!(
        "[bridge parity] final_hash={:016X}",
        replayed.last().unwrap()
    );
    assert_eq!(
        (
            rep.scenario_rng.state(),
            rep.main_rng.state(),
            rep.mapgen_rng.state()
        ),
        (
            // Native mission-site idle precedes first Ready; explicit GI ART
            // supplies real action records. See foot_bridge_layer.replay.json.
            0xBC80_136B_FB95_59C6,
            0x9C68_CC8B_9F2C_82ED,
            0x1CE8_1848_7043_6163
        ),
        "bridge absolute RNG states changed",
    );
    let final_hash = *replayed.last().expect("at least one tick replayed");
    assert_eq!(
        final_hash, BRIDGE_HARNESS_FINAL_HASH,
        "committed bridge-harness baseline drifted. Do not paste the observed value \
         until the tripwires above are still green and you can say which behavior \
         moved; this constant is a Rust-vs-prior-Rust ratchet, not gamemd evidence"
    );
}
