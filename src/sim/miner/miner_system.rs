//! Miner state machine — the Harvest mission handler body
//! (SearchOre→Harvest→Return→Unload loop).
//!
//! Since the handler absorption, each live miner is dispatched individually
//! from the per-object AI host (the Unit arm of `techno_ai_shell`, the
//! Mission_Dispatch position): snapshot the miner, run one FSM step, commit
//! the mutations plus the dispatch epilogue back to the entity. The FSM
//! cursor of record is `MissionCom::handler_state`; the snapshot carries a
//! decoded working copy in `MinerSnapshot::state`.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/miner, sim/miner_dock, sim/components,
//!   sim/movement, sim/pathfinding, rules/.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone};
use crate::rules::ruleset::RuleSet;
use crate::sim::miner::{CargoBale, Miner, MinerConfig, MinerState, ResourceType};
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::radio::{self, RadioMessage, RadioPayload, RadioResponse};
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;

use crate::sim::intern::InternedId;

use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::sim::dirty_cells::DirtyCells;
use crate::sim::movement::ground_pose::{object_get_coords, object_world_z_leptons};
use crate::sim::ore_growth::OreGrowthConfig;
use crate::sim::ore_growth::OreGrowthState;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::rng::SimRng;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::terrain_object::TerrainObjectStore;
use crate::sim::world::ObjectSubstrate;
use crate::util::native_x87::{X87Chop53, sqrt_approx_f32};

/// Native too-far test: `ftol(Sqrt_Approx(d²)) > threshold_cells * 256` is far;
/// `<=` (`JLE @ 0x0073EC19` / `JG @ 0x0073EE4B`) keeps the close radio path.
/// Used by both CMIN (`ChronoHarvTooFarDistance`, Rules+0xD7C) and HARV
/// (`HarvesterTooFarDistance`, Rules+0xD78); caller picks the kind-appropriate
/// threshold. Native shifts the raw Rules int (`SHL EDX,8`) with no clamp, and
/// so does this port.
///
/// `UnitClass::Mission_Harvest @ 0x0073E5E0` state 2 subtracts the candidate's
/// `GetCoords` (vtable +0x48 = `BuildingClass::GetCoords @ 0x00447AC0`, the
/// foundation centre, [`object_get_coords`]) from the miner's `GetCoords`, so
/// the X/Y side is measured to the footprint centre, not the NW cell. Z:
/// native uses the building's stored coordinate Z (`+0xA4`, unchanged by
/// GetCoords), the floor at its Location.
///
/// Distance, read from the disassembly at `0x0073EBB1..0x0073EC19` (HARV) and
/// `0x0073EDE3..0x0073EE4B` (CMIN), identical in both: the three integer
/// differences are `FILD`ed first and squared on the x87 stack, summed as
/// `(dz*dz + dy*dy) + dx*dx`, `FSTP double`, then `Sqrt_Approx @ 0x004CAC40`
/// (`FSTP float` under the chop control word, 14-bit mantissa table at
/// `0x008650BC`) and `ftol @ 0x007C5F00` (`FISTP qword`, truncating). The
/// table lookup rounds down, so the integer verdict is NOT `d² > (T*256)²`:
/// d = 1281 -> 1280.9995 -> 1280 (close), 1282 far; d = 12801 -> 12800.64 ->
/// 12800 (close), 12802 far. Rust reproduces that exact sequence.
fn return_exceeds_too_far_threshold(
    sim: &Simulation,
    miner_sid: u64,
    refinery_sid: u64,
    threshold_cells: i32,
) -> Option<bool> {
    let miner = sim.substrate.entities.get(miner_sid)?;
    let refinery = sim.substrate.entities.get(refinery_sid)?;
    if refinery.death.is_dying() || refinery.health.current() == 0 {
        return None;
    }

    let miner_x = i64::from(miner.position.rx()) * 256 + miner.position.sub_x().to_num::<i64>();
    let miner_y = i64::from(miner.position.ry()) * 256 + miner.position.sub_y().to_num::<i64>();
    // Both GetCoords (vt+0x48): the refinery's foundation centre
    // (`0x00447AC0`) at its Location's Z.
    let terrain = sim.resolved_terrain.as_ref();
    let refinery_coord = object_get_coords(refinery, terrain);
    let (refinery_x, refinery_y) = (i64::from(refinery_coord.x), i64::from(refinery_coord.y));
    let miner_z = i64::from(object_world_z_leptons(miner, terrain));
    let refinery_z = i64::from(refinery_coord.z);

    let dx = X87Chop53::load_i32(i32::try_from(miner_x - refinery_x).ok()?);
    let dy = X87Chop53::load_i32(i32::try_from(miner_y - refinery_y).ok()?);
    let dz = X87Chop53::load_i32(i32::try_from(miner_z - refinery_z).ok()?);
    // `(dz*dz + dy*dy) + dx*dx` in the native x87 operand order, then the
    // table sqrt and the truncating ftol. Every step is finite for map-sized
    // coordinates; a domain error is treated as "no decision" like a missing
    // entity rather than panicking the sim.
    let distance_sq = X87Chop53::add(
        X87Chop53::add(X87Chop53::mul(dz, dz), X87Chop53::mul(dy, dy)),
        X87Chop53::mul(dx, dx),
    );
    let root = sqrt_approx_f32(distance_sq).ok()?;
    let distance = X87Chop53::ftol_i64(X87Chop53::load_f32(root).ok()?).ok()?;
    // `SHL EDX,8` then a signed `JG` (0x0073EC14 / 0x0073EE46).
    Some(distance > i64::from(threshold_cells.wrapping_shl(8)))
}

/// The per-frame handler return. Native Mission_Harvest returns it from the
/// harvesting state (all paths), the enter/dock state, and the productive
/// search paths (ore found and moved toward); every other exit goes through
/// the default `[Harvest] Rate` epilogue or the fixed no-ore wait below.
pub(super) const DISPATCH_NEXT_FRAME: i32 = 1;

/// Install the native default handler epilogue as the dispatch delay:
/// `ftol([Harvest] Rate × 900)` plus one `RandomRanged(0, 2)` drawn on the
/// scenario stream ([`Simulation::mission_rate_epilogue_for`]). Paths that
/// take it: the return/finding-home state on every dispatch, the idle state
/// on every dispatch, the search state whenever the miner is left driving
/// (an archive or scan-hit destination, or one it already held), and any
/// cursor outside the native handler's switch.
pub(super) fn arm_rate_epilogue(
    scenario_rng: &mut SimRng,
    substrate: &ObjectSubstrate,
    rules: &RuleSet,
    snap: &mut MinerSnapshot,
) {
    snap.dispatch_delay = Simulation::mission_rate_epilogue_for(
        scenario_rng,
        substrate,
        rules,
        snap.entity_id,
        crate::sim::mission::MissionType::Harvest,
    );
}

/// Snapshot of one miner entity for one Harvest dispatch.
pub(super) struct MinerSnapshot {
    pub(super) entity_id: u64,
    pub(super) owner: InternedId,
    pub(super) type_id: InternedId,
    pub(super) rx: u16,
    pub(super) ry: u16,
    pub(super) miner: Miner,
    /// FSM cursor working copy — decoded from `MissionCom::handler_state` at
    /// dispatch entry, committed back through it at dispatch commit.
    pub(super) state: MinerState,
    /// Handler return value: frames until the next dispatch, written into the
    /// common mission dispatcher after the handler effects are committed.
    pub(super) dispatch_delay: i32,
}

/// Build the dispatch snapshot for one live, non-dying, non-slave miner.
/// Returns `None` when the object is not a dispatchable miner.
pub(super) fn build_miner_snapshot(sim: &Simulation, id: u64) -> Option<MinerSnapshot> {
    let entity = sim.substrate.entities.get(id)?;
    // A Dying miner corpse (sold/captured this tick, awaiting the end-of-tick
    // drain) must not move, harvest, or deposit.
    if entity.death.is_dying() {
        return None;
    }
    // A Slave Miner's own Mission_Harvest is HandleReturnedSlaves
    // (`0x0073E5E9`, chain 6); its slaves harvest through `slave_manager`.
    if !entity.is_harvester() {
        return None;
    }
    let miner = entity.miner.as_ref()?;
    let cursor = MinerState::from_cursor(entity.mission.handler_state());
    debug_assert!(
        cursor.is_some(),
        "miner {} carries an out-of-vocabulary Harvest cursor {:#x}",
        id,
        entity.mission.handler_state(),
    );
    Some(MinerSnapshot {
        entity_id: id,
        owner: entity.owner(),
        type_id: entity.type_ref(),
        rx: entity.position.rx(),
        ry: entity.position.ry(),
        miner: miner.clone(),
        state: cursor.unwrap_or(MinerState::SearchOre),
        dispatch_delay: DISPATCH_NEXT_FRAME,
    })
}

/// Commit one dispatched snapshot back to the entity: miner mutations and the
/// FSM cursor of record (`MissionCom::handler_state`, which
/// `UnitClass::Mission_Harvest @ 0x0073E5E0` stores at
/// `0x0073E714..0x0073EE68`).
pub(super) fn commit_miner_snapshot(substrate: &mut ObjectSubstrate, snap: &MinerSnapshot) {
    let Some(entity) = substrate.entities.get_mut(snap.entity_id) else {
        return;
    };
    entity.miner = Some(snap.miner.clone());
    entity.mission.set_handler_state(snap.state.cursor());
}

/// Whether the unit's mission is Harvest, as `UnitClass::AI` asks it at
/// `0x007365CD`: `MissionClass::Get_Mission` (`0x005B3040`), the current
/// mission, else the queued one.
pub(crate) fn native_mission_is_harvest(entity: &crate::sim::game_entity::GameEntity) -> bool {
    entity.mission.effective().known() == Some(MissionType::Harvest)
}

/// `UnitClass::AI` once `FootClass::AI` returns (`0x007365BB..0x007365D8`):
/// a live unit whose mission is not Harvest clears Unit+0x6D2, every frame.
/// Mission_Move, Mission_Patrol and Mission_Repair also clear it on entry
/// (`0x00740A99`, `0x00740B1A`, `0x00740F10`); they run inside FootClass::AI
/// and nothing reads the byte in between, so this clear covers them.
pub(crate) fn unit_ai_clear_harvesting(substrate: &mut ObjectSubstrate, id: u64) {
    let Some(entity) = substrate.entities.get_mut(id) else {
        return;
    };
    if entity.death.is_dying()
        || entity.category() != EntityCategory::Unit
        || native_mission_is_harvest(entity)
        || !entity.miner.as_ref().is_some_and(|miner| miner.harvesting)
    {
        return;
    }
    if let Some(miner) = entity.miner.as_mut() {
        miner.harvesting = false;
    }
}

/// `UnitClass::AI`'s stuck-harvester nudge (`0x0073681D..0x0073697B`), its
/// last block, every frame. A `Harvester=` Unit (`+0xE0E`) whose House no
/// human controls (`0x0050B730`), whose mission began more than 300 frames
/// ago (`+0xC0`), whose locomotor is not moving (ILocomotion `+0x10`) and
/// which stands on its refinery's dock Cell, the `Refinery=` building's
/// (`+0x16BB`) Cell plus (3, 1) of the first Building in its own Cell
/// (`0x0047C520`), drives off: it says OVER_OUT to its contact when it has
/// one (`0x0065AE30`, vt+0x274(3)), takes the Cell NearbyLocation finds
/// near the refinery (`0x00703590`) as its destination (vt+0x480(cell, 1))
/// and queues Move to commence now (vt+0x1E8(2, 1)).
///
/// A normal unload is safe: DOCK_NOW queues Unload (`0x0043C788`), which
/// restarts the mission clock, and this block follows the frame's second
/// Commence. Before DOCK_NOW a docking miner is still on Enter, whose clock
/// started when Harvest handed over (state 3).
///
/// RESIDUAL (timing, not ported logic): a computer miner whose Enter began
/// more than 300 frames before it stands still on the pad, in the turn
/// before DOCK_NOW, is sent off and docks again. This needs VERA's Enter,
/// warp and queue timing to match native to match retail. Trigger: a Chrono
/// Miner warping from far off (`ChronoHarvTooFarDistance=50`), or a War
/// Miner waiting behind others. Frequency: unmeasured.
pub(crate) fn unit_ai_nudge_off_refinery_pad(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let frame = sim.session.binary_frame();
    if entity.category() != EntityCategory::Unit
        || !sim
            .object_type(entity.type_ref(), rules)
            .is_some_and(|object| object.harvester)
        || sim
            .houses
            .get(&entity.owner())
            .is_none_or(|house| house.is_controlled_by_human(sim.session.game_mode_nonzero()))
        || (frame as i32).wrapping_sub(entity.mission.mission_start_frame() as i32) <= 300
        || crate::sim::movement::motion_query::is_moving(entity) != Some(false)
    {
        return;
    }
    let (rx, ry) = (entity.position.rx(), entity.position.ry());
    let in_contact = !entity.radio_contacts.is_empty();
    let Some(refinery) = sim
        .substrate
        .occupancy
        .first_building_on_layer(rx, ry, MovementLayer::Ground)
        .filter(|&building| {
            sim.substrate
                .entities
                .get(building)
                .is_some_and(|building| {
                    sim.object_type(building.type_ref(), rules)
                        .is_some_and(|object| object.refinery)
                        && (
                            building.position.rx().wrapping_add(3),
                            building.position.ry().wrapping_add(1),
                        ) == (rx, ry)
                })
        })
    else {
        return;
    };
    if in_contact {
        radio::transmit_to_contact(sim, id, RadioMessage::Break, rules, frame_effects);
    }
    drive_off_refinery(sim, rules, id, refinery, overlay_registry, frame_effects);
    let now = sim.session.binary_frame();
    let _ = sim.mission_queue_exact(
        id,
        MissionId::from_known(MissionType::Move),
        1,
        now,
        &crate::sim::mission::authority::LiveReadyInputProvider { rules },
    );
}

pub(super) fn process_miner(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    snap: &mut MinerSnapshot,
    frame_effects: FrameEffects<'_>,
) {
    // Mission_Harvest73E5E0 reaches its dock/type gates and state switch even
    // while Drive retains a track. The state's non-null NavCom branch owns
    // the still-driving Rate/RNG epilogue73EF77; a generic track guard here
    // would suppress that dispatch and advance the scenario stream wrongly.
    // `UnitClass::Mission_Harvest @ 0x0073E5E0` preamble (decompiled
    // 2026-09-05): after the non-harvester (`return 0x1C2`) and slave-host
    // exits, the handler walks the type's `Dock=` list (`Type+0x3EC`, count
    // `+0x3F8`) and enters its state switch only for the FIRST dock type with
    // `HouseClass::CountOwnedInstances > 0`. When no entry has one — the
    // house lost its last refinery, or the type lists no `Dock=` at all — it
    // falls out of the loop to `Queue_Mission(Guard, 0); return 1`. The
    // `IsControlledByHuman` test at the head only decides whether the
    // `+0x3F8 == 0` case takes the same Guard queue early or reaches it
    // through the empty loop; both houses end on Guard, so the human test is
    // inert and not modelled.
    if !house_owns_dock_instance(sim, rules, snap) {
        queue_guard_from_harvest(sim, snap, rules);
        snap.dispatch_delay = DISPATCH_NEXT_FRAME;
        return;
    }

    let state_before = format!("{:?}", snap.state);
    match snap.state {
        MinerState::SearchOre => harvest_looking(sim, rules, overlay_registry, snap, frame_effects),
        MinerState::Harvest => {
            harvest_cutting(sim, rules, config, overlay_registry, snap, frame_effects)
        }
        // Native return/finding-home state has no per-frame exit: every
        // dispatch leaves through the default Rate epilogue.
        MinerState::ReturnToRefinery => {
            handle_return(sim, rules, snap, frame_effects);
            arm_rate_epilogue(&mut sim.scenario_rng, &sim.substrate, rules, snap);
        }
        MinerState::Dock => handle_handoff(sim, snap, rules),
        MinerState::WaitNoOre => {
            handle_going_to_idle(sim, rules, overlay_registry, snap, frame_effects);
            // Native state 4 has no `return 1` exit: every dispatch falls
            // into the default Rate epilogue (`0x0073EF97`).
            arm_rate_epilogue(&mut sim.scenario_rng, &sim.substrate, rules, snap);
        }
    }
    let state_after = format!("{:?}", snap.state);
    if state_before != state_after {
        log::info!(
            "MINER {} state: {} → {} pos=({},{}) cargo={} stage={}",
            snap.entity_id,
            state_before,
            state_after,
            snap.rx,
            snap.ry,
            snap.miner.cargo.len(),
            sim.substrate
                .entities
                .get(snap.entity_id)
                .map_or(0, |entity| entity.native_stage().value()),
        );
    }
}

// -- State handlers --

/// `UnitClass::Mission_Harvest @ 0x0073E5E0` state 0 (LOOKING),
/// `0x0073E6F1..0x0073E92C`. Native evidence:
/// tools/spatial_oracle/harvest_field.json `harvest` rows `s0_*`.
///
/// - A full miner (Storage% >= 1.0, `0x0073E706`) goes home: state 2,
///   next frame.
/// - The archived ore cell (Techno+0x218) becomes the destination through
///   the class setter and the archive clears (`0x0073E72A..0x0073E750`); the
///   scan's flag argument drops to 0.
/// - Unit+0x6D2 clears (`0x0073E75B`). An active Teleport holding a NavCom
///   takes the NULL destination (`0x0073E793..0x0073E83E`).
/// - `Search_For_Tiberium_And_Move(TiberiumLongScan)` (`0x0073E864`). A
///   miner already on its best cell harvests: Unit+0x6D2 set, the StageClass
///   armed at the literal rate 2, state 1, next frame (`0x0073E87D..
///   0x0073E8BD`).
/// - A miss with no NavCom and no archive parks (state 4, House+0x242, the
///   fixed 105 frames, no draw; `0x0073E8EA..0x0073E91C`). A miss with an
///   archive re-takes it as the destination (`0x0073E8D7`; the clear above
///   leaves this arm dead). Driving, or just sent off, it leaves through the
///   Rate epilogue.
///
/// RESIDUAL: the Weeder= short search (`0x0073E76C`), dormant (no retail
/// Weeder=); an archive that is an object (the base-defence responder's
/// post) is not re-taken as a destination: VERA assigns cell archives only.
fn harvest_looking(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    snap: &mut MinerSnapshot,
    frame_effects: FrameEffects<'_>,
) {
    let id = snap.entity_id;
    if snap.miner.is_full() {
        snap.state = MinerState::ReturnToRefinery;
        return;
    }
    let archive = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| entity.archive_target());
    if let Some(archive) = archive {
        assign_archive_destination(sim, rules, id, archive, overlay_registry, frame_effects);
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.set_archive_target(None);
        }
    }
    snap.miner.harvesting = false;
    let teleport_with_nav = sim.substrate.entities.get(id).is_some_and(|entity| {
        entity.navigation.nav_com().is_some()
            && entity
                .locomotor
                .as_ref()
                .is_some_and(|loco| loco.active_kind() == LocomotorKind::Teleport)
    });
    if teleport_with_nav {
        sim.set_unit_null_destination(id, rules, None, frame_effects);
    }
    let range = super::ore_scan::scan_cells(rules.general.tiberium_long_scan);
    if super::ore_scan::search_for_tiberium_and_move(
        sim,
        rules,
        overlay_registry,
        id,
        range,
        frame_effects,
    ) {
        snap.miner.harvesting = true;
        arm_stage(
            &mut sim.substrate,
            snap.entity_id,
            sim.session.binary_frame(),
            2,
        );
        snap.state = MinerState::Harvest;
        return;
    }
    let driving = sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.navigation.nav_com().is_some());
    if !driving {
        let archive = sim
            .substrate
            .entities
            .get(id)
            .and_then(|entity| entity.archive_target());
        let Some(archive) = archive else {
            // `+0x3D0` is not carried on the entity: its only in-handler
            // reader is the state-4 RepairBay probe, whose outcome the same
            // dispatch overwrites (see `handle_going_to_idle`), and state 4 is
            // reachable from this arm alone, where the byte is always 1. Its
            // other readers (`BuildingClass::MissionRepairAndProduce`
            // `0x0044C4BC`, AI-house only) are outside this lane.
            snap.state = MinerState::WaitNoOre;
            if let Some(house) = sim.houses.get_mut(&snap.owner) {
                // `MOV byte ptr [ECX+0x242], 1` at `0x0073E911`, gated on
                // `UnitType+0xE0E` (`Harvester=yes`) — true for every kind
                // dispatched here (slave hosts never reach this handler).
                house.note_harvester_no_ore();
            }
            snap.dispatch_delay = NO_ORE_DELAY;
            return;
        };
        assign_archive_destination(sim, rules, id, archive, overlay_registry, frame_effects);
    }
    arm_rate_epilogue(&mut sim.scenario_rng, &sim.substrate, rules, snap);
}

/// The state-0 no-ore return (`return 0x69` at `0x0073E91C`): frames until
/// the GOING-TO-IDLE state dispatches. Not an INI value.
const NO_ORE_DELAY: i32 = 0x69;

/// `vt+0x480(ArchiveTarget, 1)`: the class setter towards an archived cell.
fn assign_archive_destination(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    archive: crate::sim::combat::TargetKind,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    if let crate::sim::combat::TargetKind::Cell(x, y) = archive {
        assign_miner_cell_destination(sim, rules, id, (x, y), overlay_registry, frame_effects);
    }
}

/// Arm the Unit+0xF8 StageClass: value 0, timer and rate `rate` from `now`.
fn arm_stage(substrate: &mut ObjectSubstrate, id: u64, now: u32, rate: u32) {
    if let Some(entity) = substrate.entities.get_mut(id) {
        entity.restart_native_stage(0, now as i32, rate as i32);
    }
}

/// `HarvesterLoadRate=` as the StageClass rate (Rules+0x1520).
fn load_rate(rules: &RuleSet) -> u32 {
    rules.general.harvester_load_rate as u32
}

/// `UnitClass::Mission_Harvest @ 0x0073E5E0` state 1 (HARVESTING),
/// `0x0073E931..0x0073EB2B`; every exit is the next frame. Native evidence:
/// tools/spatial_oracle/harvest_field.json `harvest` rows `s1_*`.
///
/// - An unarmed StageClass (rate 0) is armed at `HarvesterLoadRate`
///   (`0x0073E93B`). Until the stage reaches 9 nothing else runs
///   (`0x0073E96F`); the TechnoClass::AI stage tick after this dispatch
///   advances it, so a cut comes `9 * rate + 1` frames after an arming.
/// - `Harvest_Ore_Tick` ([`harvest_ore_tick`]); success stays here.
/// - Failure clears Unit+0x6D2. A full harvester (Storage% == 1.0,
///   `0x0073E9B9`) goes home: state 2, and the archive becomes the best cell
///   `Scan_For_Tiberium(TiberiumShortScan)` finds, or none (`0x0073E9E4..
///   0x0073EA7B`).
/// - Otherwise `Search_For_Tiberium_And_Move(TiberiumShortScan)`: a miss
///   with no NavCom clears the archive and goes home (`0x0073EAEE`); a hit
///   stays here with Unit+0x6D2 set (`0x0073EB0E`). The stage is not
///   re-armed, so a miner that hops to the next cell cuts on arrival.
fn harvest_cutting(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    snap: &mut MinerSnapshot,
    frame_effects: FrameEffects<'_>,
) {
    let now = sim.session.binary_frame();
    if sim
        .substrate
        .entities
        .get(snap.entity_id)
        .is_some_and(|entity| entity.native_stage().rate() == 0)
    {
        arm_stage(&mut sim.substrate, snap.entity_id, now, load_rate(rules));
    }
    if sim
        .substrate
        .entities
        .get(snap.entity_id)
        .is_none_or(|entity| entity.native_stage().value() < 9)
    {
        return;
    }
    if harvest_ore_tick(sim, rules, config, overlay_registry, snap) {
        return;
    }
    snap.miner.harvesting = false;
    let id = snap.entity_id;
    let range = super::ore_scan::scan_cells(rules.general.tiberium_short_scan);
    if snap.miner.cargo.len() == usize::from(snap.miner.capacity_bales) {
        snap.state = MinerState::ReturnToRefinery;
        let archive = super::ore_scan::scan_for_tiberium(sim, rules, overlay_registry, id, range)
            .map(|(x, y)| crate::sim::combat::TargetKind::Cell(x, y));
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.set_archive_target(archive);
        }
        return;
    }
    let found = super::ore_scan::search_for_tiberium_and_move(
        sim,
        rules,
        overlay_registry,
        id,
        range,
        frame_effects,
    );
    let driving = sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.navigation.nav_com().is_some());
    if !found && !driving {
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.set_archive_target(None);
        }
        snap.state = MinerState::ReturnToRefinery;
        return;
    }
    snap.state = MinerState::Harvest;
    snap.miner.harvesting = true;
}

/// [`harvest_ore_tick`] on `id` alone, as the oracle calls it.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn harvest_ore_tick_for_test(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    id: u64,
) -> bool {
    let mut snap = build_miner_snapshot(sim, id).expect("a dispatchable miner");
    let config = MinerConfig::from_rules(rules);
    let ok = harvest_ore_tick(sim, rules, &config, overlay_registry, &mut snap);
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.miner = Some(snap.miner);
    }
    ok
}

/// `UnitClass::Harvest_Ore_Tick @ 0x0073D450`. Native evidence:
/// tools/spatial_oracle/harvest_field.json `ore_tick` rows.
///
/// - A mover holding a NavCom answers true and touches nothing.
/// - Not `Harvester=`, full (Storage% >= 1.0) or off Tiberium land: the
///   StageClass resets (value 0, rate 0, timer at now for 0) and it answers
///   false.
/// - Otherwise it asks `CellClass::Reduce_Tiberium` for
///   `ftol(min(1.0, Storage - total))` — one level, integer cargo keeping a
///   whole level free below full — and stores what it removed; a positive
///   removal re-arms the stage at `HarvesterLoadRate` and answers true. A
///   density-0 cell clears for nothing: false, stage untouched.
///
/// RESIDUAL: the Weeder= branch (`0x0073D4F4`, `HarvesterLoadRate * 3`),
/// dormant (no retail Weeder=). Cargo is integer bales per Ore/Gem kind where
/// native keeps float `StorageClass` slots per tiberium type (see the Unload
/// payout residual).
fn harvest_ore_tick(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    snap: &mut MinerSnapshot,
) -> bool {
    let id = snap.entity_id;
    let now = sim.session.binary_frame();
    if sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.navigation.nav_com().is_some())
    {
        return true;
    }
    let cell = (snap.rx, snap.ry);
    let harvester = sim
        .object_type(snap.type_id, rules)
        .is_some_and(|object| object.harvester);
    if !harvester
        || snap.miner.is_full()
        || !super::ore_scan::cell_is_tiberium_land(sim, overlay_registry, cell)
    {
        arm_stage(&mut sim.substrate, id, now, 0);
        return false;
    }
    let request = snap
        .miner
        .capacity_bales
        .saturating_sub(snap.miner.cargo.len() as u16)
        .min(1);
    let reduction = Simulation::reduce_tiberium_at_with_native_context(
        &sim.terrain_objects,
        &sim.ore_growth_config,
        &mut sim.ore_growth_state,
        &sim.session,
        &mut sim.scenario_rng,
        &sim.substrate,
        &mut sim.resolved_terrain,
        &mut sim.overlay_grid,
        &mut sim.dirty_cells,
        cell,
        request,
        rules,
        overlay_registry,
    );
    let Some(resource_type) = reduction
        .resource_type
        .filter(|_| reduction.removed_amount > 0)
    else {
        return false;
    };
    let value = match resource_type {
        ResourceType::Ore => config.ore_bale_value,
        ResourceType::Gem => config.gem_bale_value,
    };
    snap.miner
        .cargo
        .extend((0..reduction.removed_amount).map(|_| CargoBale {
            resource_type,
            value,
        }));
    arm_stage(&mut sim.substrate, id, now, load_rate(rules));
    true
}

/// `UnitClass::Mission_Harvest @ 0x0073E5E0` state 2 (FINDING_HOME),
/// `0x0073EB2C..0x0073EE72`. Every exit is the caller's Rate epilogue. The
/// Teleporter byte (`Type+0xCD4`, loaded at `0x0073E6DE`) changes three steps.
/// Native evidence: tools/spatial_oracle/refinery_dock.json and
/// cmin_dock.json `mission_harvest` rows.
///
/// - With a NavCom (still driving) a War Miner only waits (`0x0073EB5A`). A
///   Teleporter first asks the narrow `Find_Docking_Bay(Dock, 0, 0)`
///   (`0x0073EB3A`): a bay drops NavCom and NavComAux raw (`0x004DF0D0`; no
///   locomotor Stop, so the Drive keeps its destination) and the state runs
///   on in the same dispatch.
/// - The narrow result within `HarvesterTooFarDistance` (Rules+0xD78), for a
///   Teleporter `ChronoHarvTooFarDistance` (+0xD7C), gets HELLO; ROGER writes
///   state 3 (`0x0073EB7E..0x0073EE68`).
/// - Otherwise the wide pass (`0x0073EC1F`) finds the dock to wait by. A War
///   Miner within 0x300 leptons of it waits where it is; a Teleporter never
///   does (`0x0073ECD0`). The miner is sent to the nearby passable cell around
///   the dock's NW cell plus art `QueueingCell=` (`0x0073ECDF..0x0073EDBB`),
///   or its destination is cleared when there is none. Neither destination
///   has a radio contact, so a Teleporter's arm makes it drive.
fn handle_return(
    sim: &mut Simulation,
    rules: &RuleSet,
    snap: &mut MinerSnapshot,
    frame_effects: FrameEffects<'_>,
) {
    let id = snap.entity_id;
    let teleporter = sim
        .object_type(snap.type_id, rules)
        .is_some_and(|object| object.teleporter);
    let driving = sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.navigation.nav_com().is_some());
    if driving && !teleporter {
        return;
    }
    // The narrow pass takes only a bay with a free contact slot
    // (`FUN_0065ADF0` at `0x004DEF09`), so a driving Teleporter keeps its
    // NavCom while every bay is busy.
    let narrow = find_docking_bay(sim, rules, snap.entity_id, false, false, frame_effects);
    if driving {
        if narrow.is_none() {
            return;
        }
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.navigation.stop_moving();
        }
    }
    let too_far = if teleporter {
        rules.general.chrono_harv_too_far_distance
    } else {
        rules.general.harvester_too_far_distance
    };
    if let Some(bay) = narrow
        && return_exceeds_too_far_threshold(sim, id, bay, too_far) == Some(false)
        && radio::transmit(
            sim,
            id,
            bay,
            RadioMessage::Hello,
            RadioPayload::default(),
            rules,
            frame_effects,
        ) == RadioResponse::Roger
    {
        snap.state = MinerState::Dock;
        return;
    }
    let Some(bay) = find_docking_bay(sim, rules, snap.entity_id, true, true, frame_effects) else {
        return;
    };
    if !teleporter
        && return_exceeds_too_far_threshold(sim, id, bay, STAGING_MIN_CELLS) != Some(true)
    {
        return;
    }
    match refinery_staging_cell(sim, rules, bay) {
        Some(cell) => {
            sim.set_unit_destination(
                id,
                crate::sim::components::NavTargetRef::cell(cell.0, cell.1),
                rules,
                true,
                frame_effects,
            );
        }
        None => {
            sim.assign_null_destination(id, rules, None, frame_effects);
        }
    }
}

/// `0x0073ECD0`: a War Miner whose wide-pass dock is within `0x300` leptons
/// (3 cells) gets no staging destination.
const STAGING_MIN_CELLS: i32 = 3;

/// `MapClass::Find_Nearby_Passable_Cell @ 0x0056DC20` as Mission_Harvest
/// state 2 calls it at `0x0073ED75`: seeded at the dock's NW cell
/// (`Location / 256`) plus the low words of art `QueueingCell=`, with
/// SpeedType Wheel, no zone, MovementZone Normal, bridges allowed, no height
/// or occupancy check and no target cell (frame-modulo pick). `None` is the
/// (0, 0) no-cell answer.
fn refinery_staging_cell(sim: &Simulation, rules: &RuleSet, bay: u64) -> Option<(u16, u16)> {
    use crate::rules::locomotor_type::SpeedType;
    use crate::sim::find_nearby_cell::{NearbyArgs, NearbyFootprint, PassabilityArgs};
    let building = sim.substrate.entities.get(bay)?;
    let queueing = sim
        .object_type(building.type_ref(), rules)
        .map_or([0, 0], |object| object.queueing_cell);
    let seed = (
        i32::from((building.position.rx() as i16).wrapping_add(queueing[0] as i16)),
        i32::from((building.position.ry() as i16).wrapping_add(queueing[1] as i16)),
    );
    sim.nearby_passable_cell(
        seed,
        NearbyArgs::plain(
            PassabilityArgs {
                speed_type: SpeedType::Wheel,
                required_zone_id: None,
                movement_zone: MovementZone::Normal,
                bridge_aware_zone: false,
            },
            NearbyFootprint::SINGLE,
        ),
    )
    .filter(|&cell| cell != (0, 0))
}

/// Mission_Harvest state 3 (`0x0073EE8A`): `Queue_Mission(Enter, 0); return
/// 1`. UnitClass::AI's Ready/Commence step promotes it.
fn handle_handoff(sim: &mut Simulation, snap: &MinerSnapshot, rules: &RuleSet) {
    let now = sim.session.binary_frame();
    let _ = sim.mission_queue_exact(
        snap.entity_id,
        MissionId::from_known(MissionType::Enter),
        0,
        now,
        &LiveReadyInputProvider { rules },
    );
}

/// `UnitClass::Mission_Harvest @ 0x0073E5E0` state 4 (GOING-TO-IDLE), the
/// cursor the scan-miss return parks in — gamemd's only entry into it. The
/// miss return carries the whole 105-frame wait as the dispatch delay, so the
/// body has no internal gate; the wait expiring *is* this dispatch. Body
/// (decompiled 2026-09-05, `0x0073EEA6..0x0073EF71`):
///
/// 1. `if Techno+0x3D0` (always 1 here, set by the miss return):
///    `Find_Docking_Bay(Rules+0x850 [General]RepairBay=, 0, 1)` →
///    `Queue_Mission(Hunt 0xF, 0)` when null else `Queue_Mission(Repair 0x14,
///    0)`. **Inert, EXCLUDED**: `MissionClass::Queue_Mission @ 0x005B35E0`
///    only overwrites `QueuedMission` (no Commence — `commence_now` is 0 and
///    the guard `current == Wait && mission == Guard || current == Selling`
///    never holds on Harvest), and step 3 below queues Guard over it in the
///    same dispatch. The shared `Find_Docking_Bay @ 0x004DF040` scanner
///    sends CAN_LOAD queries; stock UnitRepair queries only inspect admission
///    and do not reserve a slot or draw RNG. This overwritten repair/hunt
///    query remains outside the ordinary deposit/resume comparison. Stock
///    `RepairBay=GADEPT,NADEPT,CAOUTP` supplies the candidate types.
/// 2. `Look_up_building_in_cell(own cell)`: a building whose type has
///    `+0x16BB` (`Refinery=`) or `+0x16BC` (weeder dock, no stock type) set →
///    `Set_Destination(FUN_00703590(building))` — `Find_Nearby_Passable_Cell`
///    seeded at the building's `GetCoords` cell (foundation centre) for the
///    unit's movement zone. Ownership is not tested.
/// 3. `Queue_Mission(Guard 5, 0)`; fall into the Rate epilogue.
///
/// The queued Guard is promoted by the per-object AI host's
/// Ready-to-Commence step; while the miner is still driving off the refinery
/// the promotion defers and this state simply re-runs (re-queueing Guard is
/// idempotent). Once on Guard the Harvest handler is no longer dispatched;
/// what brings a miner back is `UnitClass::Mission_Guard @ 0x00740810` (the
/// chrono arms in `techno_ai/mission_handlers.rs`) or a player order
/// (MEGAMISSION Harvest or Enter, EventClass MEGAMISSION →
/// `Queue_Mission(mission, 0)` at `0x004C73B9`). A human war miner therefore
/// parks on Guard until re-ordered. State 4 tests no owner, so a computer
/// house's miner parks too. Mission_Guard's AI arm (`0x00740881..
/// 0x00740942`) re-queues Harvest only while `House+0x242` is clear
/// (`0x00740922`), and the miss that led here has just set it, with no
/// clearing writer (`HouseState::harvester_no_ore`). What still moves a
/// parked computer miner back to Harvest is an arm without that test:
/// Enter_Idle_Mode's harvester arm (`0x00738BD8`) after a later stop,
/// AreaGuard (`0x004D6D26`) or RUN_AWAY (`0x00737A98`).
///
/// Step 2 calls the existing `Simulation::techno_nearby_location` owner
/// (703590), including its movement-zone/GetZone/FNPC arguments. Pending
/// entry controls execute that original helper against supplied uniform
/// zones; they do not establish arbitrary map-zone construction. The
/// destination goes through the Unit setter, so a Chrono Miner drives it.
fn handle_going_to_idle(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    snap: &mut MinerSnapshot,
    frame_effects: FrameEffects<'_>,
) {
    // 73EF1F selects the first ground Building before its type flags; no
    // dying filter or later matching refinery can replace that identity.
    if let Some(refinery_sid) = sim
        .substrate
        .occupancy
        .first_building_on_layer(snap.rx, snap.ry, MovementLayer::Ground)
        .filter(|&sid| {
            sim.substrate.entities.get(sid).is_some_and(|building| {
                sim.object_type(building.type_ref(), rules)
                    .is_some_and(|kind| kind.refinery)
            })
        })
    {
        drive_off_refinery(
            sim,
            rules,
            snap.entity_id,
            refinery_sid,
            overlay_registry,
            frame_effects,
        );
    }
    queue_guard_from_harvest(sim, snap, rules);
}

/// vt+0x480(cell, 1) to the Cell NearbyLocation finds near `refinery`
/// (`0x00703590`), as Mission_Harvest state 4 (`0x0073EF1F..`) and the
/// stuck-harvester nudge (`0x0073690A..0x0073696D`) send a miner off its pad.
///
/// RESIDUAL: native always calls the setter with `Map[cell]`; VERA skips it
/// when NearbyLocation finds no Cell or one outside the path grid
/// ([`assign_miner_cell_destination`]). On a miss FNPC answers the zeroed
/// (0,0) (`0x00ABD480`), outside the map diamond, so `Get_CellClass`
/// (`0x005657A0`) hands back the shared dummy cell (`0x00ABDC50`) and Unit
/// `Set_Destination` (`0x00741970`) makes it the NavCom with Move queued.
/// Trigger: no unoccupied passable same-zone Cell within the search's ring
/// cap (`0x0056DCEF`) of the refinery's centre. Frequency: none seen.
fn drive_off_refinery(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    refinery: u64,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    if let Some(exit) = sim
        .techno_nearby_location(id, Some(refinery), rules)
        .and_then(|(x, y)| Some((u16::try_from(x).ok()?, u16::try_from(y).ok()?)))
    {
        assign_miner_cell_destination(sim, rules, id, exit, overlay_registry, frame_effects);
    }
}

/// `Queue_Mission(Guard, 0)` from inside the Harvest handler (preamble and
/// state 4). No Commence: the host's Ready-to-Commence step promotes it.
fn queue_guard_from_harvest(sim: &mut Simulation, snap: &MinerSnapshot, rules: &RuleSet) {
    let now = sim.session.binary_frame();
    let _ = sim.mission_queue_exact(
        snap.entity_id,
        MissionId::from_known(MissionType::Guard),
        0,
        now,
        &LiveReadyInputProvider { rules },
    );
}

/// The Harvest preamble's loop (`0x0073E67F..0x0073E6B1`): the house owns
/// a tracked instance of an entry of the harvester type's `Dock=` list
/// ([`HouseTracking::owns_any_building`]). A `Dock=` name the interner has
/// never seen has no instances, and a missing house (fixtures only) owns
/// none. A miner whose type is unknown to the rules cannot evaluate the list
/// and reads as "owns one" (fixture tolerance; no production type lacks a
/// rules entry).
///
/// [`HouseTracking::owns_any_building`]: crate::sim::house_tracking::HouseTracking::owns_any_building
fn house_owns_dock_instance(sim: &Simulation, rules: &RuleSet, snap: &MinerSnapshot) -> bool {
    let Some(harvester) = sim.interner.object_type(snap.type_id, rules) else {
        return true;
    };
    sim.houses.get(&snap.owner).is_some_and(|house| {
        house.tracking().owns_any_building(
            harvester
                .dock
                .iter()
                .filter_map(|name| sim.interner.get(name)),
        )
    })
}

// -- Helpers --

/// Extract one bale from a tiberium cell: one density level of the overlay.
pub(crate) fn extract_bale(
    terrain_objects: &TerrainObjectStore,
    ore_growth_config: &OreGrowthConfig,
    ore_growth_state: &mut OreGrowthState,
    session: &ScenarioSession,
    scenario_rng: &mut SimRng,
    substrate: &ObjectSubstrate,
    resolved_terrain: &mut Option<ResolvedTerrainGrid>,
    overlay_grid: &mut Option<OverlayGrid>,
    dirty_cells: &mut DirtyCells,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    cell: (u16, u16),
    config: &MinerConfig,
) -> Option<CargoBale> {
    let outcome = Simulation::reduce_tiberium_at_with_native_context(
        terrain_objects,
        ore_growth_config,
        ore_growth_state,
        session,
        scenario_rng,
        substrate,
        resolved_terrain,
        overlay_grid,
        dirty_cells,
        cell,
        1,
        rules,
        overlay_registry,
    );
    if outcome.removed_amount == 0 {
        return None;
    }
    let resource_type = outcome.resource_type?;
    let value = match resource_type {
        ResourceType::Ore => config.ore_bale_value,
        ResourceType::Gem => config.gem_bale_value,
    };
    Some(CargoBale {
        resource_type,
        value,
    })
}

/// One `FootClass::Find_Docking_Bay @ 0x004DF040` pass for a miner or an
/// aircraft (`AircraftClass::Find_Docking_Bay @ 0x0041BBD0` calls it): each
/// `Dock=` type in list order goes to the scanner ([`scan_docking_type`]),
/// and the pass keeps a type's answer when it has none yet, when that
/// answer is strictly nearer than the best so far, when the best is -1 or
/// when the answer is a primary factory (0x004DF08E..0x004DF0A7).
///
/// `wide` controls the scanner prefilter; `ignore_dock_capacity` is the
/// explicit native A8E7AC receiver context. Harvest73EC1F supplies both for
/// its wide pass. They are distinct: a wide scan without that context still
/// runs ordinary CAN_LOAD capacity admission.
pub(crate) fn find_docking_bay(
    sim: &mut Simulation,
    rules: &RuleSet,
    requester: u64,
    wide: bool,
    ignore_dock_capacity: bool,
    frame_effects: FrameEffects<'_>,
) -> Option<u64> {
    let type_id = sim.substrate.entities.get(requester)?.type_ref();
    let requester_type = sim.interner.object_type(type_id, rules)?;
    let mut result: Option<u64> = None;
    let mut best = -1_i32;
    for dock_type in &requester_type.dock {
        if let Some((found, nearest)) = scan_docking_type(
            sim,
            rules,
            requester,
            dock_type,
            wide,
            ignore_dock_capacity,
            frame_effects,
        ) && (result.is_none()
            || nearest < best
            || best == -1
            || sim.production.is_primary_factory(found))
        {
            result = Some(found);
            best = nearest;
        }
    }
    result
}

/// `FootClass::Find_Docking_Bay(type, 0, 0)` (`0x004DEE50`, Foot vt+0x530):
/// the narrow scanner for one `Dock=` type, its distance dropped. Its caller
/// is the damaged computer miner's retreat
/// (`sim::combat::unit_damage_response`).
pub(crate) fn find_docking_bay_of_type(
    sim: &mut Simulation,
    rules: &RuleSet,
    requester: u64,
    dock_type: &str,
    frame_effects: FrameEffects<'_>,
) -> Option<u64> {
    scan_docking_type(
        sim,
        rules,
        requester,
        dock_type,
        false,
        false,
        frame_effects,
    )
    .map(|(dock, _)| dock)
}

/// The scanner `FUN_004DEE80` (Foot vt+0x52C) for one `Dock=` type: the
/// nearest of the requester's OWN house's buildings of that type, walked in
/// the house's building list order (`Owner @ TechnoClass+0x21C` → House+0x68,
/// items `+0x6C`, count `+0x78`; VERA's `HouseBaseState::buildings`), with
/// its squared distance. Allies are never candidates, so a miner never
/// deposits into an ally's wallet.
///
/// Per-candidate gates in native order:
/// - non-null and `+0x81 == 0` (`ObjectClass::InLimbo`), type == Dock type
///   (`+0x520`); the whole scan returns null when the house owns no instance
///   of that type (`HouseClass::CountOwnedInstances` at 0x004DEEA5);
/// - narrow pass only (`wide != 1`, 0x004DEF02): `FUN_0065ADF0` on the
///   building with the requester as argument (0x004DEF09) — a free
///   `Contacts[]` slot (`+0xE4`, count `+0xE8`) or the requester already
///   tracked. `+0xE8` is 1 from the `RadioClass` ctor (0x0065A764) and then
///   set by `BuildingClass::Constructor` 0x0043BCBD..0x0043BCD0 to
///   `max([Type+0x1780] NumberOfDocks, 1)` via `Set_Contact_Count`; Rust
///   installs these slots in the shared constructor, and admission reads
///   them through `Contacts::has_free_or` (actual-slot oracle controls);
/// - `MapClass::Can_Reach_Zone` from the requester's cell to the building's
///   `GetCoords` cell, skipped for an Aircraft (`WhatAmI() == 2`,
///   0x004DEF20) — see [`dock_zone_reachable`];
/// - `Receive_Radio(0xF)` must return 1 — the shared building radio receiver;
/// - distance `FUN_005F6500`: `dx² + dy²` in leptons between both objects'
///   `GetCoords` (Z ignored), 32-bit wrapping `IMUL`/`ADD`; the candidate
///   replaces the best when the best is still -1, when it is strictly
///   nearer (signed `JL`, so ties keep the earlier building) or when it is a
///   primary factory (`TechnoClass+0x3D3`, 0x004DF004).
fn scan_docking_type(
    sim: &mut Simulation,
    rules: &RuleSet,
    requester: u64,
    dock_type: &str,
    wide: bool,
    ignore_dock_capacity: bool,
    frame_effects: FrameEffects<'_>,
) -> Option<(u64, i32)> {
    let (owner, own_x, own_y, aircraft) = {
        let entity = sim.substrate.entities.get(requester)?;
        let coords = object_get_coords(entity, sim.resolved_terrain.as_ref());
        (
            entity.owner(),
            coords.x,
            coords.y,
            entity.category() == EntityCategory::Aircraft,
        )
    };
    // `0x004DEE9B..0x004DEEAC`: a type of which the house tracks no
    // instance (`+0x5500`, `JZ`) finds nothing, whatever the list holds.
    let type_ref = sim.interner.get(dock_type)?;
    if sim
        .houses
        .get(&owner)?
        .tracking()
        .owned_count(EntityCategory::Structure, type_ref)
        == 0
    {
        return None;
    }
    // CAN_LOAD runs synchronously; retain the House's native scan order
    // without holding its projection across radio owner calls.
    let buildings = sim
        .houses
        .get(&owner)?
        .base_projection()
        .buildings()
        .to_vec();
    let mut found: Option<u64> = None;
    let mut nearest = -1_i32;
    for &sid in &buildings {
        let Some(entity) = sim.substrate.entities.get(sid) else {
            continue;
        };
        let e_type = sim.interner.resolve(entity.type_ref());
        if !e_type.eq_ignore_ascii_case(dock_type) || entity.lifecycle.in_limbo() {
            continue;
        }
        // gamemd removes a dead building from the house list through
        // Limbo; VERA keeps the entity through its death animation.
        if entity.death.is_dying() || entity.health.current() == 0 {
            continue;
        }
        if !wide && !entity.radio_contacts.has_free_or(requester) {
            continue;
        }
        // `BuildingClass::GetCoords @ 0x00447AC0`: foundation centre, the
        // point the zone gate and the distance measure to, and the same point
        // the state-2 too-far test measures to.
        let centre = object_get_coords(entity, sim.resolved_terrain.as_ref());
        if !aircraft && !dock_zone_reachable(sim, rules, requester, centre) {
            continue;
        }
        if crate::sim::radio::transmit(
            sim,
            requester,
            sid,
            crate::sim::radio::RadioMessage::CanEnter,
            crate::sim::radio::RadioPayload::docking_query(ignore_dock_capacity),
            rules,
            frame_effects,
        ) != crate::sim::radio::RadioResponse::Roger
        {
            continue;
        }
        let dx = own_x.wrapping_sub(centre.x);
        let dy = own_y.wrapping_sub(centre.y);
        let distance = dy.wrapping_mul(dy).wrapping_add(dx.wrapping_mul(dx));
        if nearest == -1 || distance < nearest || sim.production.is_primary_factory(sid) {
            found = Some(sid);
            nearest = distance;
        }
    }
    found.map(|found| (found, nearest))
}

/// The `MapClass::Can_Reach_Zone @ 0x0056D100` gate of the scanner
/// `FUN_004DEE80` (`0x004DEF29..0x004DEFDD`): from the cell of the
/// requester's `vt+0x4C` coordinate on its ShouldBeOnBridge layer, with its
/// type's MovementZone ([`harvest_reach`], the request Is_Cell_Harvestable
/// makes), to the cell of the building's GetCoords (`building`), with no
/// destination bridge and no fringe shortcut. Native re-queries the source
/// for each candidate. A world without native zone topology, or a requester
/// without a navigation coordinate, skips the gate.
///
/// [`harvest_reach`]: crate::sim::miner::ore_scan::harvest_reach
fn dock_zone_reachable(
    sim: &Simulation,
    rules: &RuleSet,
    requester: u64,
    building: crate::sim::components::DriveCoord,
) -> bool {
    let (Some(terrain), Some(reach)) = (
        sim.resolved_terrain.as_ref(),
        crate::sim::miner::ore_scan::harvest_reach(sim, rules, requester),
    ) else {
        return true;
    };
    // `CDQ; AND EDX,0xFF; ADD; SAR 8` (`0x004DEF3B..0x004DEF5A`).
    let cell = crate::util::lepton::lepton_to_cell_packed;
    reach
        .reaches(
            sim,
            &crate::map::resolved_terrain::NativeCellQuery::canonical(terrain),
            (cell(building.x), cell(building.y)),
        )
        .unwrap_or(true)
}

/// A miner's `vt+0x480(cell, 1)` through the class setter: Search_For_Tiberium
/// (`0x004DD086`) and Mission_Harvest state 4's `Set_Destination`. For a Unit
/// that is `0x741970`, which returns on an unchanged NavCom and whose
/// Teleporter arm gives a Chrono Miner out of radio contact a Drive. The
/// setter is void; a cell outside the path grid is not ordered.
pub(crate) fn assign_miner_cell_destination(
    sim: &mut Simulation,
    rules: &RuleSet,
    entity_id: u64,
    target: (u16, u16),
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    let Some(grid) = sim.path_grid() else {
        return;
    };
    if target.0 >= grid.width() || target.1 >= grid.height() {
        return;
    }
    if let Err(cause) = sim.assign_destination_represented(
        entity_id,
        Some(crate::sim::components::NavTargetRef::cell(
            target.0, target.1,
        )),
        rules,
        overlay_registry,
        frame_effects,
    ) {
        log::warn!("Miner destination {entity_id}: {cause}");
    }
}

/// Count completed, alive Ore Purifier buildings owned by `owner`
/// (case-insensitive) — the native `House+0x538C` counter.
///
/// Used by the deposit-bonus formula in `phase_unloading` and by the Slave
/// Miner deposit path. The bonus is `count × PurifierBonus × amount`, so
/// every real purifier stacks the bonus linearly.
///
/// Native writers of `House+0x538C` (gamemd.exe, read 2026-09-06):
/// - `BuildingClass::OnConstructionComplete` 0x0044636C..0x0044637C:
///   `Type+0x16CC` (`OrePurifier=`) → `INC [House+0x538C]` — only when the
///   build-up finishes, so a purifier still in its construction anim pays
///   nothing;
/// - `BuildingClass::ChangeOwner` 0x00448260: `DEC` on the old owner at
///   0x00448AC2 and `INC` on the new owner at 0x004491EB (the same
///   register-with-house tail that appends the building to the house's
///   per-category vectors), so a captured purifier moves with the house;
/// - `BuildingClass::Limbo` 0x00445925: `DEC` when the building leaves the
///   map (sale end, destruction).
///
/// Rust: `building_up.is_some()` is the construction anim (native
/// `BSTATE_CONSTRUCTION` before `OnConstructionComplete`), `dying` is the
/// Limbo'd corpse; a selling building (`building_down`) still counts, as the
/// native `DEC` only lands at Limbo.
pub(crate) fn count_purifiers_for_owner(sim: &Simulation, rules: &RuleSet, owner: &str) -> i32 {
    sim.substrate
        .entities
        .values()
        .filter(|e| {
            counts_as_purifier(sim, rules, e)
                && sim.interner.resolve(e.owner()).eq_ignore_ascii_case(owner)
        })
        .count() as i32
}

/// The owner-independent half of the `House+0x538C` predicate: a completed,
/// alive, on-map `OrePurifier=` structure, for [`count_purifiers_for_owner`].
pub(crate) fn counts_as_purifier(
    sim: &Simulation,
    rules: &RuleSet,
    e: &crate::sim::game_entity::GameEntity,
) -> bool {
    // A Dying purifier corpse (sold/destroyed this tick) must not keep paying
    // its deposit bonus until the end-of-tick drain; a limbo'd one already
    // took the native Limbo `DEC`.
    !e.death.is_dying()
        && !e.lifecycle.in_limbo()
        && !e.building_up()
        && e.category() == EntityCategory::Structure
        && sim
            .object_type(e.type_ref(), rules)
            .is_some_and(|obj| obj.ore_purifier)
}

/// Effective purifier count used in the deposit bonus formula.
///
/// Returns `real_purifiers + AI_virtual_purifiers`, where the AI term is
/// `general.ai_virtual_purifiers[refinery_owner.difficulty]` for non-human
/// houses. Both terms are sourced from the refinery's owner — credit
/// destination is a separate concern.
///
/// Native (`0x0073E3D5..0x0073E408`) adds the virtual term only when the
/// owner's `House+0x1EC` (is-human) is clear AND `[0x00A8B238]` (Session
/// GameMode, the global `HouseClass::IsControlledByHuman @ 0x0050B730` also
/// reads) is nonzero: never in the campaign
/// (tools/spatial_oracle/refinery_dock.json `unload_gate_ai_*` rows).
pub(crate) fn effective_purifier_count(
    sim: &Simulation,
    rules: &RuleSet,
    refinery_owner: &str,
) -> i32 {
    let real = count_purifiers_for_owner(sim, rules, refinery_owner);
    // The AI virtual bonus needs the owner's house to say it is a computer.
    let Some(house) =
        crate::sim::house_state::house_state_for_owner(&sim.houses, refinery_owner, &sim.interner)
    else {
        return real;
    };
    if house.is_human() || !sim.session.game_mode_nonzero() {
        return real;
    }
    let table = rules.general.ai_virtual_purifiers;
    let virtual_count = table[house.difficulty().table_index()];
    real + virtual_count
}
