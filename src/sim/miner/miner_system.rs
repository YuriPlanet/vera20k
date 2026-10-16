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
use crate::sim::mission::authority::EntityReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::pathfinding::zone_map::{ZONE_INVALID, ZoneGrid};
use crate::sim::radio::{self, RadioMessage, RadioPayload, RadioResponse};
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;
use crate::util::direction_tables::CELL_DELTAS;

use crate::sim::debug_event_log::DebugEventKind;
use crate::sim::intern::InternedId;

use crate::sim::movement::ground_pose::{object_get_coords, object_world_z_leptons};
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

#[cfg(test)]
mod gsi_04_03b_tests {
    use super::*;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::util::fixed_math::SimFixed;

    /// Mission_Harvest state 2 measures to `BuildingClass::GetCoords @
    /// 0x00447AC0` = the foundation centre. For a 4x3 refinery that is
    /// (+384, +256) leptons from the NW cell, which flips the 5-cell
    /// `HarvesterTooFarDistance` verdict on either side of the building.
    #[test]
    fn too_far_threshold_measures_to_the_foundation_centre() {
        let mut sim = Simulation::new();
        let refinery_type = sim.interner.intern("GAREFN");
        let harv_type = sim.interner.intern("HARV");
        let mut refinery = GameEntity::test_default_of_category(
            2,
            "GAREFN",
            "Allies",
            10,
            10,
            EntityCategory::Structure,
        );
        refinery.set_type_ref_for_test(refinery_type);
        // Construction stamps the type's `Foundation=4x3`.
        refinery
            .type_facts
            .set_foundation_for_test("4x3".to_string());
        sim.substrate.entities.insert(refinery);

        // East of the building: NW distance is sqrt(37) cells (far), centre
        // distance is 4.5 cells (x = 4224 vs centre 3072, dy = 0; within 5)
        // -> native keeps the narrow result.
        let mut east = GameEntity::test_default(1, "HARV", "Allies", 16, 11);
        east.set_type_ref_for_test(harv_type);
        sim.substrate.entities.insert(east);
        assert_eq!(
            return_exceeds_too_far_threshold(&sim, 1, 2, 5),
            Some(false),
            "east side: centre offset pulls a NW-far miner inside the threshold"
        );

        // West of the building: NW distance is sqrt(17) cells (near), centre
        // distance is sqrt(30.25) cells (beyond 5) -> native falls to wide.
        let mut west = GameEntity::test_default(3, "HARV", "Allies", 6, 11);
        west.set_type_ref_for_test(harv_type);
        sim.substrate.entities.insert(west);
        assert_eq!(
            return_exceeds_too_far_threshold(&sim, 3, 2, 5),
            Some(true),
            "west side: centre offset pushes a NW-near miner beyond the threshold"
        );
    }

    /// `Sqrt_Approx`'s 14-bit table rounds down, so with
    /// `HarvesterTooFarDistance=5` (1280 leptons) d = 1281 truncates to 1280
    /// (close) and only d = 1282 reads as far. 1x1 fallback, dy = dz = 0.
    #[test]
    fn too_far_threshold_follows_sqrt_approx_table_edge() {
        let mut sim = Simulation::new();
        let mut refinery = GameEntity::test_default(2, "GAREFN", "Allies", 0, 0);
        refinery.position.set_sub_x_for_test(SimFixed::from_num(0));
        refinery.position.set_sub_y_for_test(SimFixed::from_num(0));
        sim.substrate.entities.insert(refinery);
        let mut miner = GameEntity::test_default(1, "HARV", "Allies", 5, 0);
        miner.position.set_sub_x_for_test(SimFixed::from_num(1));
        miner.position.set_sub_y_for_test(SimFixed::from_num(0));
        sim.substrate.entities.insert(miner);

        assert_eq!(
            return_exceeds_too_far_threshold(&sim, 1, 2, 5),
            Some(false),
            "d = 1281 leptons: Sqrt_Approx yields 1280.9995, ftol 1280, close"
        );
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .position
            .set_sub_x_for_test(SimFixed::from_num(2));
        assert_eq!(
            return_exceeds_too_far_threshold(&sim, 1, 2, 5),
            Some(true),
            "d = 1282 leptons is the first far distance"
        );
    }

    fn sloped_cell() -> ResolvedTerrainCell {
        ResolvedTerrainCell {
            filled_clear: true,
            slope_type: 1,
            terrain_class: Default::default(),
            has_ramp: true,
            accepts_smudge: true,
            ..crate::map::resolved_terrain::test_flat_cell(0, 0)
        }
    }

    #[test]
    fn gsi_04_03b_miner_return_distance_uses_terrain_bridge_and_altitude_z() {
        let mut sim = Simulation::new();
        let mut miner = GameEntity::test_default(1, "CMIN", "Allies", 0, 0);
        miner.position.set_sub_x_for_test(SimFixed::from_num(0));
        let mut refinery = GameEntity::test_default(2, "GAOREP", "Allies", 0, 0);
        refinery
            .position
            .set_sub_x_for_test(SimFixed::from_num(255));
        sim.substrate.entities.insert(miner);
        sim.substrate.entities.insert(refinery);
        sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(1, 1, vec![sloped_cell()]));

        assert_eq!(
            return_exceeds_too_far_threshold(&sim, 1, 2, 1),
            Some(true),
            "255 horizontal leptons plus the slope Z delta exceeds one cell"
        );

        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .position
            .set_sub_x_for_test(SimFixed::from_num(0));
        sim.substrate
            .entities
            .get_mut(2)
            .unwrap()
            .position
            .set_sub_x_for_test(SimFixed::from_num(0));
        assert_eq!(return_exceeds_too_far_threshold(&sim, 1, 2, 1), Some(false));

        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .set_on_bridge(true);
        assert_eq!(
            return_exceeds_too_far_threshold(&sim, 1, 2, 1),
            Some(true),
            "OnBridge coordinate Z contributes the full deck offset"
        );

        let miner = sim.substrate.entities.get_mut(1).unwrap();
        miner.set_on_bridge(false);
        miner.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Fly));
        sim.set_object_height_unmarked(1, 300);
        assert_eq!(
            return_exceeds_too_far_threshold(&sim, 1, 2, 1),
            Some(true),
            "the object's height contributes to its coordinate Z"
        );
    }

    #[test]
    fn gsi_04_03b_miner_return_distance_falls_back_to_level_z_without_resolved_terrain() {
        let mut sim = Simulation::new();
        let mut miner = GameEntity::test_default(1, "CMIN", "Allies", 0, 0);
        miner.position.set_sub_x_for_test(SimFixed::from_num(0));
        miner.position.set_z_for_test(0);
        let mut refinery = GameEntity::test_default(2, "GAOREP", "Allies", 0, 0);
        refinery
            .position
            .set_sub_x_for_test(SimFixed::from_num(255));
        refinery.position.set_z_for_test(0);
        sim.substrate.entities.insert(miner);
        sim.substrate.entities.insert(refinery);
        assert!(sim.resolved_terrain.is_none());

        // Same pair as the terrain fixture above, which answers Some(true) from
        // the slope contribution. With no grid to resolve, each object degrades
        // to level-only Z: dz = 0, so 255 horizontal leptons stay inside one
        // cell — and the decision is still made rather than refused.
        assert_eq!(return_exceeds_too_far_threshold(&sim, 1, 2, 1), Some(false));

        sim.substrate
            .entities
            .get_mut(2)
            .unwrap()
            .position
            .set_z_for_test(3);
        assert_eq!(
            return_exceeds_too_far_threshold(&sim, 1, 2, 1),
            Some(true),
            "the fallback Z is the stored level, not a dropped term"
        );
    }

    #[test]
    fn gsi_04_05_sequential_miner_process_reserves_head_before_next_process() {
        let mut sim = Simulation::new();
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[VehicleTypes]\n0=HARV\n[HARV]\nSpeed=6\n",
            ))
            .unwrap();
        let grid = crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let owner = sim.interner.intern("AMERICANS");
        let type_ref = sim.interner.intern("HARV");
        for (entity_id, rx, facing) in [(1, 1, 0x40u8), (2, 3, 0xC0)] {
            let mut miner = GameEntity::test_default(entity_id, "HARV", "AMERICANS", rx, 2);
            miner.set_owner_for_test(owner);
            miner.set_type_ref_for_test(type_ref);
            miner.body_facing.snap(u16::from(facing) << 8, 0);
            miner.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
            miner
                .locomotor
                .as_mut()
                .unwrap()
                .ensure_installed_track_state();
            sim.substrate.entities.insert(miner);
            assert!(matches!(
                sim.reveal(entity_id),
                crate::sim::world::RevealOutcome::Revealed { .. }
            ));
        }

        let shared_head = (2, 2);
        assign_miner_cell_destination(
            &mut sim,
            &rules,
            1,
            shared_head,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .locomotor
                .as_ref()
                .and_then(|loco| loco.selected_drive_runtime())
                .and_then(|runtime| runtime.retained())
                .unwrap()
                .head_to()
                .is_none(),
            "the helper issues an order; Process owns head admission"
        );
        sim.process_ground_locomotor_for_test(1, Some(&rules), Some(&grid), None)
            .expect("the first miner Process commits its head reservation");

        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .and_then(|entity| entity
                    .locomotor
                    .as_ref()
                    .and_then(|loco| loco.selected_drive_runtime())
                    .and_then(|runtime| runtime.retained()))
                .and_then(|drive| drive.occupation_head_to())
                .map(|head| (head.rx, head.ry)),
            Some(shared_head)
        );
        assert!(sim.substrate.cell_occupation.occupied_by_other(
            shared_head.0,
            shared_head.1,
            MovementLayer::Ground,
            2,
        ));

        assign_miner_cell_destination(
            &mut sim,
            &rules,
            2,
            shared_head,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        sim.process_ground_locomotor_for_test(2, Some(&rules), Some(&grid), None)
            .expect("the second miner Process observes the existing reservation");

        let second = sim.substrate.entities.get(2).expect("second miner");
        assert_ne!(
            second
                .locomotor
                .as_ref()
                .and_then(|loco| loco.selected_drive_runtime())
                .and_then(|runtime| runtime.retained())
                .and_then(|drive| drive.occupation_head_to())
                .map(|head| (head.rx, head.ry)),
            Some(shared_head),
            "the second miner must observe the first Process head mark immediately"
        );
        // Unit741970 names the reserved cell unchanged; only the second
        // miner's own Process refuses the reserved head (above).
        assert_eq!(
            crate::sim::movement::movement_goal_cell(second),
            Some(shared_head)
        );
        assert!(sim.substrate.occupancy.contains_entity(1, 2, 1));
        assert!(sim.substrate.occupancy.contains_entity(3, 2, 2));
        assert_eq!(
            sim.substrate
                .occupancy
                .count_on_layer(2, 2, MovementLayer::Ground),
            0
        );
    }
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
pub(super) fn arm_rate_epilogue(sim: &mut Simulation, rules: &RuleSet, snap: &mut MinerSnapshot) {
    snap.dispatch_delay = sim.mission_rate_epilogue_for(
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
    /// Buffered miner state change events — flushed to entity at commit.
    pub(super) debug_events: Vec<(String, String)>,
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
        debug_events: Vec::new(),
    })
}

/// Commit one dispatched snapshot back to the entity: miner mutations, the
/// FSM cursor of record (`MissionCom::handler_state`, which
/// `UnitClass::Mission_Harvest @ 0x0073E5E0` stores at
/// `0x0073E714..0x0073EE68`) and buffered debug events.
pub(super) fn commit_miner_snapshot(sim: &mut Simulation, snap: &MinerSnapshot) {
    let Some(entity) = sim.substrate.entities.get_mut(snap.entity_id) else {
        return;
    };
    entity.miner = Some(snap.miner.clone());
    entity.mission.set_handler_state(snap.state.cursor());
    for (from, to) in &snap.debug_events {
        entity.push_debug_event(
            sim.session.tick as u32,
            DebugEventKind::MinerStateChange {
                from: from.clone(),
                to: to.clone(),
            },
        );
    }
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
pub(crate) fn unit_ai_clear_harvesting(sim: &mut Simulation, id: u64) {
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
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

/// Test-only mirror of the production Harvest dispatch walk: the same
/// per-entity dispatch (timer gate + epilogue) the host Unit arm performs, in
/// live-object order, with the legacy stable-id fallback for direct-insert
/// fixtures that never build a LogicVector.
#[cfg(test)]
pub(crate) fn tick_miners(sim: &mut Simulation, rules: &RuleSet, config: &MinerConfig) {
    tick_miners_test_walk(
        sim,
        rules,
        config,
        Some(crate::sim::tiberium::test_support::overlay_registry()),
    );
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn tick_miners_test_walk(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) {
    let live_order = sim.live_object_order_snapshot();
    let keys: Vec<u64> = if live_order.is_empty() {
        sim.substrate.entities.keys_sorted()
    } else {
        live_order
    };
    for id in keys {
        crate::sim::world::dispatch_foot_mission(
            sim,
            id,
            rules,
            crate::sim::world::ObjectAiCtx {
                miner_config: Some(config),
                overlay_registry,
                ..Default::default()
            },
        );
    }
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
        queue_guard_from_harvest(sim, snap);
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
            arm_rate_epilogue(sim, rules, snap);
        }
        MinerState::Dock => handle_handoff(sim, snap),
        MinerState::WaitNoOre => {
            if handle_going_to_idle(sim, rules, overlay_registry, snap, frame_effects) {
                // Native state 4 has no `return 1` exit: every dispatch falls
                // into the default Rate epilogue (`0x0073EF97`).
                arm_rate_epilogue(sim, rules, snap);
            }
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
        snap.debug_events.push((state_before, state_after));
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
        sim.set_unit_null_destination(id, Some(rules), None, frame_effects);
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
        arm_stage(sim, snap.entity_id, sim.session.binary_frame, 2);
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
                house.harvester_no_ore = true;
            }
            snap.dispatch_delay = NO_ORE_DELAY;
            return;
        };
        assign_archive_destination(sim, rules, id, archive, overlay_registry, frame_effects);
    }
    arm_rate_epilogue(sim, rules, snap);
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
fn arm_stage(sim: &mut Simulation, id: u64, now: u32, rate: u32) {
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
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
    let now = sim.session.binary_frame;
    if sim
        .substrate
        .entities
        .get(snap.entity_id)
        .is_some_and(|entity| entity.native_stage().rate() == 0)
    {
        arm_stage(sim, snap.entity_id, now, load_rate(rules));
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
    let now = sim.session.binary_frame;
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
        arm_stage(sim, id, now, 0);
        return false;
    }
    let request = snap
        .miner
        .capacity_bales
        .saturating_sub(snap.miner.cargo.len() as u16)
        .min(1);
    let reduction =
        sim.reduce_tiberium_at_with_native_context(cell, request, Some(rules), overlay_registry);
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
    arm_stage(sim, id, now, load_rate(rules));
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
            crate::sim::movement::foot_stop_moving(entity);
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
            None,
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
            sim.assign_null_destination(id, Some(rules), None, frame_effects);
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
    use crate::sim::find_nearby_cell::{
        NearbyFootprint, NearbyQuery, PassabilityArgs, find_nearby_passable_cell,
        map_owned_radius_cap,
    };
    let building = sim.substrate.entities.get(bay)?;
    let queueing = sim
        .object_type(building.type_ref(), rules)
        .map_or([0, 0], |object| object.queueing_cell);
    let seed = (
        i32::from((building.position.rx() as i16).wrapping_add(queueing[0] as i16)),
        i32::from((building.position.ry() as i16).wrapping_add(queueing[1] as i16)),
    );
    let (width, height) = sim
        .playfield_bounds
        .zip(sim.playfield_size_height)
        .map(|(bounds, height)| (bounds.base, height))?;
    let grid = sim.path_grid_snapshot();
    find_nearby_passable_cell(
        seed,
        &NearbyQuery {
            native_cells: None,
            raw_occupation: Some(&sim.substrate.raw_cell_occupation),
            passability: PassabilityArgs {
                speed_type: SpeedType::Wheel,
                required_zone_id: None,
                movement_zone: MovementZone::Normal,
                bridge_aware_zone: false,
            },
            footprint: NearbyFootprint::SINGLE,
            allow_bridge_cells: true,
            check_height: false,
            check_occupancy: false,
            radius_cap: map_owned_radius_cap(width, height),
            target_cell: None,
            path_grid: grid.as_deref(),
            resolved_terrain: sim.resolved_terrain.as_ref(),
            overlay_grid: sim.overlay_grid.as_ref(),
            occupancy: Some(&sim.substrate.occupancy),
            entities: Some(&sim.substrate.entities),
            zone_grid: sim.zone_grid.as_ref(),
            playfield_bounds: sim.playfield_bounds,
        },
        sim.session.binary_frame,
    )
    .filter(|&cell| cell != (0, 0))
}

/// Mission_Harvest state 3 (`0x0073EE8A`): `Queue_Mission(Enter, 0); return
/// 1`. UnitClass::AI's Ready/Commence step promotes it.
fn handle_handoff(sim: &mut Simulation, snap: &MinerSnapshot) {
    let now = sim.session.binary_frame;
    let _ = sim.mission_queue_exact(
        snap.entity_id,
        MissionId::from_known(MissionType::Enter),
        0,
        now,
        &EntityReadyInputProvider,
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
/// (`Command::HarvestCell` / `MinerReturn`, EventClass MEGAMISSION →
/// `Queue_Mission(mission, 0)` at `0x004C73B9`). A human war miner therefore
/// parks on Guard until re-ordered — native behaviour.
///
/// Step 2 calls the existing `Simulation::techno_nearby_location` owner
/// (703590), including its movement-zone/GetZone/FNPC arguments. Pending
/// entry controls execute that original helper against supplied uniform
/// zones; they do not establish arbitrary map-zone construction. The
/// destination goes through the Unit setter, so a Chrono Miner drives it.
///
/// **Non-human houses take a VERA-internal bridge instead, gamemd equivalent
/// = AI lane (`AI_Choose_Unit` 0x004FEB7B / `Mission_Guard` arm ii)
/// UNCHECKED.** Natively an AI miner parked on Guard is brought back by
/// `UnitClass::Mission_Guard` arm (ii), which re-queues Harvest only while
/// `House+0x242` is clear — and the scan miss that led here has just set it,
/// with no clearing writer. What keeps a native AI economy alive after that
/// is house/team logic (`AI_Choose_Unit` reading `+0x242`, team recruitment),
/// none of which VERA runs yet (`sim/ai.rs` never issues `HarvestCell`/
/// `MinerReturn` and skips harvesters). Until that lane lands, an AI-house
/// miner does not park: state 4 drops straight back into the state-0 scan,
/// which on a miss re-arms the same fixed 105-frame wait (the pre-Guard-park
/// perpetual re-scan cadence, `House+0x242` still written native-true on
/// every miss). Returns whether the caller must run the Rate epilogue —
/// false on the bridge, whose exit is the scan's own.
fn handle_going_to_idle(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    snap: &mut MinerSnapshot,
    frame_effects: FrameEffects<'_>,
) -> bool {
    let human = sim
        .houses
        .get(&snap.owner)
        .is_none_or(|house| house.is_controlled_by_human(sim.session.game_mode_nonzero));
    if !human {
        snap.state = MinerState::SearchOre;
        harvest_looking(sim, rules, overlay_registry, snap, frame_effects);
        return false;
    }
    if let Some(refinery_sid) = refinery_building_in_cell(sim, rules, (snap.rx, snap.ry))
        && let Some(exit) = sim
            .techno_nearby_location(snap.entity_id, Some(refinery_sid), rules)
            .and_then(|(x, y)| Some((u16::try_from(x).ok()?, u16::try_from(y).ok()?)))
    {
        assign_miner_cell_destination(
            sim,
            rules,
            snap.entity_id,
            exit,
            overlay_registry,
            frame_effects,
        );
    }
    queue_guard_from_harvest(sim, snap);
    true
}

/// `Queue_Mission(Guard, 0)` from inside the Harvest handler (preamble and
/// state 4). No Commence: the host's Ready-to-Commence step promotes it.
fn queue_guard_from_harvest(sim: &mut Simulation, snap: &MinerSnapshot) {
    let now = sim.session.binary_frame;
    let _ = sim.mission_queue_exact(
        snap.entity_id,
        MissionId::from_known(MissionType::Guard),
        0,
        now,
        &EntityReadyInputProvider,
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
        house.tracking.owns_any_building(
            harvester
                .dock
                .iter()
                .filter_map(|name| sim.interner.get(name)),
        )
    })
}

/// `Look_up_building_in_cell` for the state-4 refinery test: the structure
/// occupying `cell` whose type is `Refinery=yes` (BuildingType `+0x16BB`).
/// Any owner qualifies, as native tests only the type flag.
fn refinery_building_in_cell(sim: &Simulation, rules: &RuleSet, cell: (u16, u16)) -> Option<u64> {
    let occupancy = sim.substrate.occupancy.get(cell.0, cell.1)?;
    occupancy.blockers(MovementLayer::Ground).find(|&sid| {
        sim.substrate.entities.get(sid).is_some_and(|entity| {
            entity.category() == EntityCategory::Structure
                && !entity.death.is_dying()
                && sim
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|obj| obj.refinery)
        })
    })
}

// -- Helpers --

/// Extract one bale from a tiberium cell: one density level of the overlay.
pub(crate) fn extract_bale(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    cell: (u16, u16),
    config: &MinerConfig,
) -> Option<CargoBale> {
    let outcome =
        sim.reduce_tiberium_at_with_native_context(cell, 1, Some(rules), overlay_registry);
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
///   0x004DEF20) — see `refinery_zone_reachable`;
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
    let (owner, own_x, own_y, own_cell, unit_mz, aircraft) = {
        let entity = sim.substrate.entities.get(requester)?;
        let coords = object_get_coords(entity, sim.resolved_terrain.as_ref());
        (
            entity.owner(),
            coords.x,
            coords.y,
            (entity.position.rx(), entity.position.ry()),
            entity
                .locomotor
                .as_ref()
                .map(|loc| loc.movement_zone())
                .unwrap_or(MovementZone::Normal),
            entity.category() == EntityCategory::Aircraft,
        )
    };
    // `0x004DEE9B..0x004DEEAC`: a type of which the house tracks no
    // instance (`+0x5500`, `JZ`) finds nothing, whatever the list holds.
    let type_ref = sim.interner.get(dock_type)?;
    if sim
        .houses
        .get(&owner)?
        .tracking
        .owned_count(EntityCategory::Structure, type_ref)
        == 0
    {
        return None;
    }
    // CAN_LOAD runs synchronously; retain the House's native scan order
    // without holding its projection across radio owner calls.
    let buildings = sim.houses.get(&owner)?.base_projection.buildings().to_vec();
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
        if !aircraft {
            let dock = refinery_dock_cell(entity.position.rx(), entity.position.ry());
            if !refinery_zone_reachable(
                sim,
                sim.substrate.entities.get(requester)?,
                unit_mz,
                own_cell,
                dock,
            ) {
                continue;
            }
        }
        if crate::sim::radio::transmit(
            sim,
            requester,
            sid,
            crate::sim::radio::RadioMessage::CanEnter,
            crate::sim::radio::RadioPayload::docking_query(ignore_dock_capacity),
            Some(rules),
            frame_effects,
        ) != crate::sim::radio::RadioResponse::Roger
        {
            continue;
        }
        let entity = sim
            .substrate
            .entities
            .get(sid)
            .expect("read-only CAN_LOAD retains candidate");
        // `BuildingClass::GetCoords @ 0x00447AC0`: foundation centre, the
        // same point the state-2 too-far test measures to.
        let centre = object_get_coords(entity, sim.resolved_terrain.as_ref());
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

/// `MapClass::Can_Reach_Zone` gate of the scanner `FUN_004DEE80`, called
/// natively with the unit type's MovementZone, the miner's cell and the
/// building's `GetCoords` cell. VERA's zone map marks building footprints
/// `ZONE_INVALID`, so the probe targets the refinery's dock cell and its 8
/// neighbours instead of the foundation centre — VERA-internal
/// approximation, gamemd equivalent UNCHECKED for the exact probed cell.
/// Without a zone grid or a valid miner anchor the gate is skipped.
fn refinery_zone_reachable(
    sim: &Simulation,
    miner: &crate::sim::game_entity::GameEntity,
    mz: MovementZone,
    miner_cell: (u16, u16),
    dock: (u16, u16),
) -> bool {
    let Some(zone_grid) = sim.zone_grid.as_ref() else {
        return true;
    };
    let Some(anchor) = effective_zone_cell(zone_grid, mz, miner_cell.0, miner_cell.1) else {
        return true;
    };
    let layer = miner.movement_layer_or_ground();
    zone_grid.can_reach(mz, anchor, layer, dock, layer)
        || neighbour_reachable(zone_grid, mz, layer, anchor, dock)
}

/// The dock pad a refinery at NW `(rx, ry)` sends its miner to
/// ([`crate::sim::radio::receive::dock_pad_cell`]).
pub(crate) fn refinery_dock_cell(rx: u16, ry: u16) -> (u16, u16) {
    crate::sim::radio::receive::dock_pad_cell(rx, ry)
}

/// Return a cell whose zone serves as the harvester's anchor for the
/// refinery-dock zone gate.
///
/// The harvester's own cell may be on Tiberium (impassable in the path grid,
/// hence `ZONE_INVALID`); when so, probe its 8 neighbors and return the
/// first cell with a valid zone. Returns `None` if neither the harvester's
/// cell nor any neighbor has a valid zone — the gate is then skipped.
fn effective_zone_cell(
    zone_grid: &ZoneGrid,
    mz: MovementZone,
    rx: u16,
    ry: u16,
) -> Option<(u16, u16)> {
    let zone_map = zone_grid.map_for(mz)?;
    if zone_map.zone_at(rx, ry, MovementLayer::Ground) != ZONE_INVALID {
        return Some((rx, ry));
    }
    for &(dx, dy) in &CELL_DELTAS {
        let nx = (rx as i32) + dx;
        let ny = (ry as i32) + dy;
        if nx < 0 || ny < 0 || nx > u16::MAX as i32 || ny > u16::MAX as i32 {
            continue;
        }
        let (nx, ny) = (nx as u16, ny as u16);
        if zone_map.zone_at(nx, ny, MovementLayer::Ground) != ZONE_INVALID {
            return Some((nx, ny));
        }
    }
    None
}

/// True if any 8-neighbor of `cell` is in the harvester's connected zone
/// component: the refinery-dock gate probes the dock cell's neighbours, as
/// VERA's zone map marks building footprint cells `ZONE_INVALID`.
fn neighbour_reachable(
    zone_grid: &ZoneGrid,
    mz: MovementZone,
    layer: MovementLayer,
    harvester_zone_cell: (u16, u16),
    cell: (u16, u16),
) -> bool {
    for &(dx, dy) in &CELL_DELTAS {
        let nx = (cell.0 as i32) + dx;
        let ny = (cell.1 as i32) + dy;
        if nx < 0 || ny < 0 || nx > u16::MAX as i32 || ny > u16::MAX as i32 {
            continue;
        }
        let (nx, ny) = (nx as u16, ny as u16);
        if zone_grid.can_reach(mz, harvester_zone_cell, layer, (nx, ny), layer) {
            return true;
        }
    }
    false
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
        Some(rules),
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
    if house.is_human || !sim.session.game_mode_nonzero {
        return real;
    }
    let table = rules.general.ai_virtual_purifiers;
    let virtual_count = table[house.difficulty.table_index()];
    real + virtual_count
}

#[cfg(test)]
mod harvest_scan_dispatch_tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::miner::MinerKind;
    use crate::sim::mission::MissionType;
    use crate::sim::pathfinding::PathGrid;

    const MINER_ID: u64 = 1;

    fn scan_rules() -> RuleSet {
        let ini = IniFile::from_str(&format!(
            "{}{}",
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             0=HARV\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GAREFN\n\
             [HARV]\n\
             Name=War Miner\n\
             Speed=4\n\
             Sight=5\n\
             Harvester=yes\n\
             Dock=GAREFN\n\
             [GAREFN]\n\
             Name=Ore Refinery\n\
             Foundation=4x3\n\
             Refinery=yes\nDockUnload=yes\n",
            crate::sim::tiberium::test_support::tiberium_rules_text(),
        ));
        let mut rules = RuleSet::from_ini(&ini).expect("scan rules");
        // The retail art section (ARTMD GAREFN) carries `QueueingCell=4,1`.
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(
            &IniFile::from_str("[GAREFN]\nFoundation=4x3\nQueueingCell=4,1\n"),
        ));
        rules
    }

    const REFINERY_ID: u64 = 2;
    /// NW cell of the fixture refinery (4x3 footprint, occupancy marked).
    const REFINERY_NW: (u16, u16) = (50, 50);

    /// A War Miner parked at `cell` with the Harvest cursor on the search
    /// state, plus the owned refinery the Harvest preamble requires (a house
    /// with no `Dock=` instance is queued straight onto Guard) and the
    /// owning house registered so `House+0x242` has somewhere to land.
    fn spawn_search_miner(sim: &mut Simulation, cell: (u16, u16)) {
        spawn_search_miner_without_refinery(sim, cell);
        spawn_owned_refinery(sim, REFINERY_NW);
        register_house(sim);
    }

    fn register_house(sim: &mut Simulation) {
        let owner = sim.interner.intern("Americans");
        sim.houses.entry(owner).or_insert_with(|| {
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10)
        });
    }

    fn spawn_owned_refinery(sim: &mut Simulation, nw: (u16, u16)) {
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("GAREFN");
        let mut ge = GameEntity::new_at_frame_zero_for_test(
            REFINERY_ID,
            nw.0,
            nw.1,
            0,
            0,
            owner,
            Health::new(900),
            type_ref,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        ge.lifecycle.set_in_limbo_for_test(false);
        // Construction stamps the type's `Foundation=4x3`.
        ge.type_facts.set_foundation_for_test("4x3".to_string());
        sim.substrate.entities.insert(ge);
        // Unlimbo's House+0x68 append.
        register_house(sim);
        sim.append_house_base_building_for_test(REFINERY_ID);
        // The class constructor's `Add_Tracking`, which a direct insert skips.
        sim.update_house_tracking(
            REFINERY_ID,
            crate::sim::house_tracking::HouseTracking::add_tracking,
        );
        for y in nw.1..nw.1 + 3 {
            for x in nw.0..nw.0 + 4 {
                sim.substrate.occupancy.add(
                    x,
                    y,
                    REFINERY_ID,
                    MovementLayer::Ground,
                    None,
                    crate::sim::occupancy::CellListInsertion::AppendBuilding,
                );
                // Original Building OccupyDown453DB0 sets Cell+124 bit80.
                // The shared Nearby703590/FNPC owner reads this raw plane.
                sim.substrate.raw_cell_occupation.mark_ground(x, y, 0x80);
            }
        }
        if sim.substrate.next_stable_object_id <= REFINERY_ID {
            sim.substrate.next_stable_object_id = REFINERY_ID + 1;
        }
    }

    fn spawn_search_miner_without_refinery(sim: &mut Simulation, cell: (u16, u16)) {
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("HARV");
        let mut ge = GameEntity::new_at_frame_zero_for_test(
            MINER_ID,
            cell.0,
            cell.1,
            0,
            0,
            owner,
            Health::new(600),
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        // This prepared Harvest scene represents an Unlimboed actor; the
        // player re-order check takes native actor admission before retasking.
        ge.lifecycle.set_in_limbo_for_test(false);
        ge.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Drive,
            ),
        );
        ge.locomotor
            .as_mut()
            .unwrap()
            .ensure_installed_track_state();
        ge.miner = Some(Miner::new(MinerKind::War, &MinerConfig::default(), 0));
        ge.mission.set_handler_state(MinerState::SearchOre.cursor());
        sim.substrate.entities.insert(ge);
        // Direct insertion skips Unlimbo's class idle/commencement. A Harvest
        // fixture must supply that committed selector; the Miner component
        // alone no longer bypasses MissionClass dispatch.
        sim.mission_assign_exact(
            MINER_ID,
            crate::sim::mission::MissionId::from_known(MissionType::Harvest),
            sim.session.binary_frame,
        )
        .unwrap();
        sim.substrate.next_stable_object_id = MINER_ID + 1;
        // A playfield holding the 64x64 fixture (Is_Cell_Harvestable's first gate).
        sim.playfield_bounds
            .get_or_insert(crate::map::playfield::PlayfieldBounds {
                base: 0,
                off_fc: -64,
                off_100: -1,
                off_104: 128,
                off_108: 65,
            });
        sim.playfield_size_height.get_or_insert(64);
    }

    /// Six bales on a flat map (the scan reads the ore cell's LandType).
    fn seed_ore(sim: &mut Simulation, cell: (u16, u16)) {
        sim.resolved_terrain
            .get_or_insert_with(|| crate::map::resolved_terrain::test_flat_ground_grid(64));
        if sim.zone_grid.is_none() {
            let terrain = sim.resolved_terrain.as_ref().unwrap();
            let path = PathGrid::from_resolved_terrain(terrain);
            sim.zone_grid = Some(
                crate::sim::pathfinding::zone_map::ZoneGrid::build_with_native_map_context(
                    &path,
                    terrain,
                    &[],
                    sim.map_size_diamond(),
                    sim.playfield_bounds,
                ),
            );
        }
        crate::sim::tiberium::test_support::place_tiberium_on_map(sim, cell, ResourceType::Ore, 6);
    }

    #[test]
    fn productive_scan_sets_the_destination_and_draws_the_epilogue_in_one_dispatch() {
        let rules = scan_rules();
        let config = MinerConfig::from_rules(&rules);
        let grid = PathGrid::new(64, 64);
        let mut sim = Simulation::new();
        sim.install_fixture_path_grid(Some(&grid));
        spawn_search_miner(&mut sim, (10, 10));
        seed_ore(&mut sim, (10, 14));

        let scenario_before = sim.rng_state().scenario;
        tick_miners(&mut sim, &rules, &config);

        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(
            entity.navigation.nav_com(),
            Some(crate::sim::components::NavTargetRef::cell(10, 14))
        );
        assert!(
            crate::sim::movement::motion_query::is_moving(&entity) == Some(true),
            "gamemd sets the destination inside the scan dispatch"
        );
        assert_ne!(
            sim.rng_state().scenario,
            scenario_before,
            "the Rate epilogue's RandomRanged(0, 2) belongs to the scan dispatch"
        );
        let base = rules.mission_control.rate_frames(MissionType::Harvest);
        let delay = entity.mission.dispatch_timer().delay();
        assert!(
            (base..=base + crate::sim::mission::authority::RATE_EPILOGUE_JITTER_MAX_FRAMES as i32)
                .contains(&delay),
            "a productive scan returns the Rate epilogue, not the per-frame return \
             (delay {delay}, base {base})"
        );
    }

    #[test]
    fn scan_answering_the_miners_own_cell_returns_per_frame_and_draws_nothing() {
        let rules = scan_rules();
        let config = MinerConfig::from_rules(&rules);
        let grid = PathGrid::new(64, 64);
        let mut sim = Simulation::new();
        sim.install_fixture_path_grid(Some(&grid));
        spawn_search_miner(&mut sim, (10, 10));
        seed_ore(&mut sim, (10, 10));

        let scenario_before = sim.rng_state().scenario;
        tick_miners(&mut sim, &rules, &config);

        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert!(
            crate::sim::movement::motion_query::is_moving(&entity) != Some(true),
            "nothing to drive to"
        );
        assert_eq!(
            sim.rng_state().scenario,
            scenario_before,
            "gamemd's own-cell return bypasses the Rate epilogue"
        );
        assert_eq!(entity.mission.dispatch_timer().delay(), DISPATCH_NEXT_FRAME);
    }

    #[test]
    fn scan_miss_parks_the_miner_instead_of_driving_to_the_far_side_of_the_map() {
        let rules = scan_rules();
        let config = MinerConfig::from_rules(&rules);
        let grid = PathGrid::new(512, 512);
        let mut sim = Simulation::new();
        sim.install_fixture_path_grid(Some(&grid));
        spawn_search_miner(&mut sim, (10, 10));
        // Well outside TiberiumLongScan — the only ore on the map, and gamemd's
        // bounded scan can never reach it.
        sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(512, 512));
        seed_ore(&mut sim, (400, 400));
        assert!(rules.general.tiberium_long_scan >> 8 < 300);

        let scenario_before = sim.rng_state().scenario;
        tick_miners(&mut sim, &rules, &config);

        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.miner_state(), Some(MinerState::WaitNoOre));
        assert_eq!(
            entity.navigation.nav_com(),
            None,
            "no whole-map fallback target"
        );
        assert!(
            crate::sim::movement::motion_query::is_moving(&entity) != Some(true),
            "no cross-map drive"
        );
        assert_eq!(entity.mission.dispatch_timer().delay(), NO_ORE_DELAY);
        assert_eq!(
            sim.rng_state().scenario,
            scenario_before,
            "the miss return bypasses the Rate epilogue"
        );
    }

    /// `0x0073E5E0` state 0 miss → state 4 → 105 frames → `Queue_Mission(Guard)`
    /// (`House+0x242` written on the miss), never a re-scan of its own. Once
    /// promoted, the Harvest handler is no longer dispatched for the miner.
    #[test]
    fn scan_miss_waits_105_frames_then_queues_guard_and_latches_the_house() {
        let rules = scan_rules();
        let config = MinerConfig::from_rules(&rules);
        let grid = PathGrid::new(64, 64);
        let mut sim = Simulation::new();
        sim.install_fixture_path_grid(Some(&grid));
        spawn_search_miner(&mut sim, (10, 10));
        let owner = sim.interner.intern("Americans");
        assert!(!sim.houses[&owner].harvester_no_ore);

        tick_miners(&mut sim, &rules, &config);
        {
            let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
            assert_eq!(entity.miner_state(), Some(MinerState::WaitNoOre));
            assert_eq!(entity.mission.dispatch_timer().delay(), 105);
            assert_eq!(entity.mission.queued().known(), None);
        }
        assert!(
            sim.houses[&owner].harvester_no_ore,
            "`MOV [House+0x242], 1` at 0x0073E911 lands on the scan miss"
        );

        // Ore appears inside the scan radius while the miner waits: gamemd's
        // state 4 never looks.
        seed_ore(&mut sim, (10, 14));
        sim.session.binary_frame += 105;
        let scenario_before = sim.rng_state().scenario;
        tick_miners(&mut sim, &rules, &config);

        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.mission.queued().known(), Some(MissionType::Guard));
        assert_eq!(
            entity.miner_state(),
            Some(MinerState::WaitNoOre),
            "state 4 re-runs until Guard commences; no return to the scan"
        );
        assert_eq!(entity.navigation.nav_com(), None);
        assert!(
            crate::sim::movement::motion_query::is_moving(&entity) != Some(true),
            "not on a refinery cell: no exit drive"
        );
        assert_ne!(
            sim.rng_state().scenario,
            scenario_before,
            "state 4 always leaves through the Rate epilogue draw"
        );

        // The host's Ready-to-Commence step promotes the queued Guard.
        let now = sim.session.binary_frame;
        sim.mission_host_promote(MINER_ID, now, &rules);
        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.mission.current().known(), Some(MissionType::Guard));

        // On Guard the Harvest dispatch declines the miner: the ore stays
        // untouched and no destination appears.
        sim.session.binary_frame += 200;
        tick_miners(&mut sim, &rules, &config);
        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.mission.current().known(), Some(MissionType::Guard));
        assert_eq!(entity.navigation.nav_com(), None);
        assert!(crate::sim::movement::motion_query::is_moving(&entity) != Some(true));
        assert!(
            sim.houses[&owner].harvester_no_ore,
            "no clearing writer exists for House+0x242"
        );
    }

    /// VERA-internal AI bridge (gamemd equivalent = AI lane, UNCHECKED): a
    /// non-human house's miner never parks. The state-4 dispatch re-enters
    /// the state-0 scan, so ore that appeared during the wait is taken, and a
    /// second miss re-arms the same 105-frame wait with `House+0x242` still
    /// written native-true.
    #[test]
    fn ai_house_miner_keeps_rescanning_after_a_no_ore_miss() {
        let rules = scan_rules();
        let config = MinerConfig::from_rules(&rules);
        let grid = PathGrid::new(64, 64);
        let mut sim = Simulation::new();
        sim.install_fixture_path_grid(Some(&grid));
        // A computer house, made before the refinery counts into it.
        let owner = sim.interner.intern("Americans");
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, false, 0, 10),
        );
        spawn_search_miner_without_refinery(&mut sim, (10, 10));
        spawn_owned_refinery(&mut sim, REFINERY_NW);

        tick_miners(&mut sim, &rules, &config);
        {
            let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
            assert_eq!(entity.miner_state(), Some(MinerState::WaitNoOre));
            assert_eq!(entity.mission.dispatch_timer().delay(), 105);
        }
        assert!(sim.houses[&owner].harvester_no_ore);

        // Second miss: back through the scan, another 105-frame wait, no
        // Guard queue.
        sim.session.binary_frame += 105;
        tick_miners(&mut sim, &rules, &config);
        {
            let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
            assert_eq!(entity.mission.queued().known(), None, "no Guard park");
            assert_eq!(entity.miner_state(), Some(MinerState::WaitNoOre));
            assert_eq!(entity.mission.dispatch_timer().delay(), 105);
        }

        // Ore appears: the next re-scan finds it.
        seed_ore(&mut sim, (10, 14));
        sim.session.binary_frame += 105;
        tick_miners(&mut sim, &rules, &config);
        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.mission.queued().known(), None);
        assert_eq!(
            entity.miner_state(),
            Some(MinerState::SearchOre),
            "state 0 holds through the drive"
        );
        assert_eq!(
            entity.navigation.nav_com(),
            Some(crate::sim::components::NavTargetRef::cell(10, 14))
        );
        assert!(
            sim.houses[&owner].harvester_no_ore,
            "the bridge does not invent a clearing writer"
        );
    }

    /// Preamble: no owned instance of any `Dock=` type → `Queue_Mission(Guard,
    /// 0); return 1` before the state switch, ore or no ore.
    #[test]
    fn house_without_a_dock_instance_queues_guard_before_scanning() {
        let rules = scan_rules();
        let config = MinerConfig::from_rules(&rules);
        let grid = PathGrid::new(64, 64);
        let mut sim = Simulation::new();
        sim.install_fixture_path_grid(Some(&grid));
        spawn_search_miner_without_refinery(&mut sim, (10, 10));
        register_house(&mut sim);
        seed_ore(&mut sim, (10, 14));

        let scenario_before = sim.rng_state().scenario;
        tick_miners(&mut sim, &rules, &config);

        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.mission.queued().known(), Some(MissionType::Guard));
        assert_eq!(entity.mission.dispatch_timer().delay(), DISPATCH_NEXT_FRAME);
        assert_eq!(
            entity.navigation.nav_com(),
            None,
            "the switch (and its scan) is never entered"
        );
        assert_eq!(
            sim.rng_state().scenario,
            scenario_before,
            "return 1, no draw"
        );
        let owner = sim.interner.intern("Americans");
        assert!(!sim.houses[&owner].harvester_no_ore);
    }

    /// State 4 on a refinery cell: `Set_Destination(FUN_00703590(building))`
    /// before the Guard queue — the miner drives off the pad it unloaded on.
    #[test]
    fn idle_tail_drives_a_miner_off_the_refinery_cell_before_guard() {
        let rules = scan_rules();
        let config = MinerConfig::from_rules(&rules);
        let grid = PathGrid::new(64, 64);
        let mut sim = Simulation::new();
        sim.install_fixture_path_grid(Some(&grid));
        // FNPC reads CellClass identity and raw Building occupation. A detached
        // PathGrid resolves the shared dummy instead of these placed cells.
        sim.install_resolved_terrain_for_new_map(
            crate::map::resolved_terrain::test_flat_ground_grid(64),
        );
        // Standing on the stock pad cell (NW + (3, 1)) inside the footprint.
        let pad = (REFINERY_NW.0 + 3, REFINERY_NW.1 + 1);
        spawn_search_miner(&mut sim, pad);

        tick_miners(&mut sim, &rules, &config);
        sim.session.binary_frame += 105;
        tick_miners(&mut sim, &rules, &config);

        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.mission.queued().known(), Some(MissionType::Guard));
        let goal =
            drive_destination_cell(entity).expect("exit destination set from the refinery cell");
        let inside = (REFINERY_NW.0..REFINERY_NW.0 + 4).contains(&goal.0)
            && (REFINERY_NW.1..REFINERY_NW.1 + 3).contains(&goal.1);
        assert!(
            !inside,
            "exit cell {goal:?} must be a passable cell outside the footprint"
        );
    }

    /// The production re-order path: a war miner parked on Guard goes back to
    /// work only through a player order. `Command::HarvestCell` (the right
    /// click on ore) is the MEGAMISSION Harvest assignment
    /// (`Queue_Mission(mission, 0)` @ 0x004C73B9, promoted at the host's next
    /// Ready/Commence) with the clicked cell as its destination
    /// (`0x004C747C`), after which the Harvest dispatch gate re-engages on
    /// state 0.
    #[test]
    fn player_harvest_order_returns_a_parked_war_miner_to_work() {
        let rules = scan_rules();
        let config = MinerConfig::from_rules(&rules);
        let grid = PathGrid::new(64, 64);
        let mut sim = Simulation::new();
        sim.install_fixture_path_grid(Some(&grid));
        spawn_search_miner(&mut sim, (10, 10));

        tick_miners(&mut sim, &rules, &config);
        sim.session.binary_frame += 105;
        tick_miners(&mut sim, &rules, &config);
        let now = sim.session.binary_frame;
        sim.mission_host_promote(MINER_ID, now, &rules);
        assert_eq!(
            sim.substrate
                .entities
                .get(MINER_ID)
                .expect("miner")
                .mission
                .current()
                .known(),
            Some(MissionType::Guard)
        );

        seed_ore(&mut sim, (10, 14));
        let applied = sim.apply_command(
            "Americans",
            &crate::sim::command::Command::HarvestCell {
                entity_id: MINER_ID,
                target_rx: 10,
                target_ry: 14,
            },
            Some(&rules),
        );
        assert!(applied);
        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(
            entity.mission.current().known(),
            Some(MissionType::Guard),
            "Queue_Mission leaves Guard current"
        );
        assert_eq!(
            entity.mission.queued(),
            crate::sim::mission::MissionId::from_known(MissionType::Harvest)
        );
        assert_eq!(
            entity.navigation.nav_com(),
            Some(crate::sim::components::NavTargetRef::cell(10, 14)),
            "the order hands the clicked cell to the class setter"
        );
        // The host's next Ready/Commence starts Harvest at state 0.
        let now = sim.session.binary_frame;
        sim.mission_host_promote(MINER_ID, now, &rules);
        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.mission.current().known(), Some(MissionType::Harvest));
        assert_eq!(entity.miner_state(), Some(MinerState::SearchOre));

        // The Harvest handler dispatches again and drives to the ordered cell.
        sim.session.binary_frame += 1;
        tick_miners(&mut sim, &rules, &config);
        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.mission.current().known(), Some(MissionType::Harvest));
        assert_eq!(
            drive_destination_cell(entity),
            Some((10, 14)),
            "the dispatch gate re-engaged and the miner drives to the order"
        );
    }

    /// The Drive locomotor's authoritative destination cell (+34).
    fn drive_destination_cell(entity: &GameEntity) -> Option<(u16, u16)> {
        let coord = entity
            .locomotor
            .as_ref()
            .and_then(|loco| loco.selected_drive_runtime())
            .and_then(|runtime| runtime.retained())?
            .destination()?;
        Some(((coord.x / 256) as u16, (coord.y / 256) as u16))
    }

    /// A full War Miner at `cell` with the finding-home cursor, its refinery at
    /// (10, 10): the fixture for the state-2 return contact.
    fn spawn_returning_war_miner(sim: &mut Simulation, cell: (u16, u16)) {
        spawn_search_miner_without_refinery(sim, cell);
        spawn_owned_refinery(sim, (10, 10));
        register_house(sim);
        let entity = sim.substrate.entities.get_mut(MINER_ID).expect("miner");
        entity
            .mission
            .set_handler_state(MinerState::ReturnToRefinery.cursor());
        let miner = entity.miner.as_mut().expect("miner");
        let capacity = miner.capacity_bales;
        miner.cargo = (0..capacity)
            .map(|_| CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            })
            .collect();
    }

    /// State 2, HARV: the narrow bay within `HarvesterTooFarDistance` gets
    /// HELLO on the same dispatch (`0x0073EE51`) and the accepted reply hands
    /// off to the Enter sequence 4.5 cells out — before any adjacency.
    #[test]
    fn war_miner_hello_at_four_cells_hands_off_to_enter_before_adjacency() {
        let rules = scan_rules();
        let config = MinerConfig::from_rules(&rules);
        let grid = PathGrid::new(64, 64);
        let mut sim = Simulation::new();
        sim.install_fixture_path_grid(Some(&grid));
        // Refinery centre = (3072, 2944) leptons; cell (16, 11) is 1152
        // leptons (4.5 cells) east of it.
        spawn_returning_war_miner(&mut sim, (16, 11));

        tick_miners(&mut sim, &rules, &config);

        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.miner_state(), Some(MinerState::Dock));
        assert_eq!(entity.radio_contacts.slot(0), Some(REFINERY_ID));
        assert!(
            crate::sim::miner::miner_dock::has_contact(&sim, REFINERY_ID, MINER_ID),
            "HELLO accepted on the same dispatch"
        );
        assert_eq!((entity.position.rx(), entity.position.ry()), (16, 11));
        let pad = sim
            .substrate
            .entities
            .get(REFINERY_ID)
            .map(|refinery| refinery_dock_cell(refinery.position.rx(), refinery.position.ry()))
            .unwrap();
        assert!(
            pad.0.abs_diff(16) > 1,
            "the hand-off happened without adjacency to the pad {pad:?}"
        );
    }

    const BLOCKER_ID: u64 = 99;

    /// A live non-miner vehicle that can hold the refinery's contact slot
    /// (the dead-reservation sweep keeps contacts only for live objects).
    fn spawn_slot_holder(sim: &mut Simulation) {
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("MTNK");
        let mut ge = GameEntity::new_at_frame_zero_for_test(
            BLOCKER_ID,
            30,
            30,
            0,
            0,
            owner,
            Health::new(300),
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        ge.lifecycle.set_in_limbo_for_test(false);
        sim.substrate.entities.insert(ge);
        assert!(
            crate::sim::miner::miner_dock::test_support::dock_test_hello(
                sim,
                REFINERY_ID,
                BLOCKER_ID
            )
        );
    }

    /// Refused HELLO, HARV farther than 0x300 leptons from the bay: drive to
    /// the `QueueingCell` staging cell and stay in state 2 (retry every Rate
    /// dispatch).
    #[test]
    fn refused_hello_beyond_three_cells_stages_the_war_miner_and_keeps_state_two() {
        let rules = scan_rules();
        let config = MinerConfig::from_rules(&rules);
        let grid = PathGrid::new(64, 64);
        let mut sim = Simulation::new();
        spawn_returning_war_miner(&mut sim, (16, 11));
        // Another object holds the refinery's single contact slot.
        spawn_slot_holder(&mut sim);

        sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
            base: 0,
            off_fc: -64,
            off_100: -1,
            off_104: 128,
            off_108: 65,
        });
        sim.playfield_size_height = Some(64);
        sim.path_grid = Some(std::sync::Arc::new(grid.clone()));
        tick_miners(&mut sim, &rules, &config);

        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.miner_state(), Some(MinerState::ReturnToRefinery));
        assert!(!crate::sim::miner::miner_dock::has_contact(
            &sim,
            REFINERY_ID,
            MINER_ID
        ));
        // 4.5 cells out: Assign_Destination(FNPC(NW + QueueingCell 4,1)); the
        // seed (14, 11) is free, so the search answers it.
        assert_eq!(
            entity.navigation.nav_com(),
            Some(crate::sim::components::NavTargetRef::cell(14, 11))
        );
    }

    /// Refused HELLO, HARV within 0x300 leptons: no destination at all; the
    /// state re-runs (and re-HELLOs) on the next Rate dispatch.
    #[test]
    fn refused_hello_within_three_cells_sets_no_destination() {
        let rules = scan_rules();
        let config = MinerConfig::from_rules(&rules);
        let grid = PathGrid::new(64, 64);
        let mut sim = Simulation::new();
        sim.install_fixture_path_grid(Some(&grid));
        // Cell (14, 11): 640 leptons (2.5 cells) east of the centre.
        spawn_returning_war_miner(&mut sim, (14, 11));
        spawn_slot_holder(&mut sim);

        tick_miners(&mut sim, &rules, &config);

        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.miner_state(), Some(MinerState::ReturnToRefinery));
        assert!(
            crate::sim::movement::motion_query::is_moving(&entity) != Some(true),
            "`CMP EAX,0x300; JG` not taken"
        );

        // Slot frees: the next dispatch's HELLO is accepted and hands off.
        crate::sim::miner::miner_dock::break_contact(
            &mut sim,
            BLOCKER_ID,
            REFINERY_ID,
            crate::sim::world::FrameEffects::default(),
        );
        sim.session.binary_frame += 20;
        tick_miners(&mut sim, &rules, &config);
        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.miner_state(), Some(MinerState::Dock));
        assert_eq!(entity.radio_contacts.slot(0), Some(REFINERY_ID));
    }
}
