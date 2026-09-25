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
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::MovementZone;
use crate::rules::ruleset::RuleSet;
use crate::sim::miner::miner_dock::{self, ContactAdmission};
use crate::sim::miner::{
    CargoBale, Miner, MinerConfig, MinerKind, MinerState, RefineryDockPhase, ResourceType,
};
use crate::sim::mission::authority::EntityReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::zone_map::{ZONE_INVALID, ZoneGrid};
use crate::sim::world::{SimSoundEvent, Simulation};
use crate::util::fixed_math::SimFixed;

use crate::sim::debug_event_log::DebugEventKind;
use crate::sim::intern::InternedId;

use crate::sim::production::foundation_dimensions;
use crate::util::lepton::{LEPTONS_PER_LEVEL, ground_height_leptons};
use crate::util::native_x87::{X87Chop53, sqrt_approx_f32};

/// Object-coordinate Z of one object in leptons: the terrain ground height for
/// its cell (level plus slope), the bridge deck offset when it stands on a
/// bridge, and any locomotor altitude.
///
/// A missing resolved-terrain grid, a cell outside it, or an unsupported slope
/// is a Rust-side resource gap rather than a game rule, so this degrades that
/// one object to its stored level height instead of refusing to answer — the
/// caller must still be able to reach a distance decision.
fn object_coordinate_z(
    sim: &Simulation,
    entity: &crate::sim::game_entity::GameEntity,
    x_leptons: i64,
    y_leptons: i64,
) -> i64 {
    let ground = sim
        .resolved_terrain
        .as_ref()
        .and_then(|terrain| terrain.cell(entity.position.rx, entity.position.ry))
        .and_then(|cell| {
            ground_height_leptons(
                cell.level,
                cell.slope_type,
                x_leptons as i32,
                y_leptons as i32,
            )
            .ok()
        })
        .map_or_else(
            || i64::from(entity.position.z) * LEPTONS_PER_LEVEL,
            i64::from,
        );
    ground
        + if entity.on_bridge {
            i64::from(crate::sim::map::bridge_topology::BRIDGE_DECK_HEIGHT_LEPTONS)
        } else {
            0
        }
        + entity
            .locomotor
            .as_ref()
            .map(|locomotor| locomotor.altitude.to_num::<i64>())
            .unwrap_or(0)
}

/// `BuildingClass::GetCoords @ 0x00447AC0` (object vtable +0x48) X/Y, read
/// from the disassembly 2026-09-05: `out.x = [this+0x9C] + (W-1)*128`,
/// `out.y = [this+0xA0] + (H-1)*128`, `out.z = [this+0xA4]` unchanged, with
/// `W = BuildingTypeClass::GetFoundationWidth @ 0x0045EC90` and
/// `H = GetFoundationHeight(0) @ 0x0045ECA0`. `+0x9C` is the NW foundation
/// cell's coordinate, so the result is the footprint centre: offset 0 for a
/// 1x1, (384, 256) leptons for a 4x3 refinery. Both the `FUN_004DEE80`
/// candidate ranking and the Mission_Harvest state-2 too-far test consume
/// this point, so they share this one formula. Z is not touched here: the
/// building's stored coordinate Z belongs to its NW cell, which is what
/// `object_coordinate_z` resolves for the entity.
fn building_get_coords_xy(
    entity: &crate::sim::game_entity::GameEntity,
    foundation_w: u16,
    foundation_h: u16,
) -> (i64, i64) {
    let x = i64::from(entity.position.rx) * 256
        + entity.position.sub_x.to_num::<i64>()
        + (i64::from(foundation_w.max(1)) - 1) * 128;
    let y = i64::from(entity.position.ry) * 256
        + entity.position.sub_y.to_num::<i64>()
        + (i64::from(foundation_h.max(1)) - 1) * 128;
    (x, y)
}

/// Native too-far test: `ftol(Sqrt_Approx(d²)) > threshold_cells * 256` is far;
/// `<=` (`JLE @ 0x0073EC19` / `JG @ 0x0073EE4B`) keeps the close radio path.
/// Used by both CMIN (`ChronoHarvTooFarDistance`, Rules+0xD7C) and HARV
/// (`HarvesterTooFarDistance`, Rules+0xD78); caller picks the kind-appropriate
/// threshold. Native shifts the raw Rules int (`SHL EDX,8`) with no clamp; the
/// `.max(1)` lives in `MinerConfig::from_general_rules` (VERA-internal, gamemd
/// equivalent UNCHECKED for a 0 threshold).
///
/// `UnitClass::Mission_Harvest @ 0x0073E5E0` state 2 subtracts the candidate's
/// `GetCoords` (vtable +0x48 = `BuildingClass::GetCoords @ 0x00447AC0`, the
/// foundation centre — see `building_get_coords_xy`) from the miner's
/// `GetCoords`, so the X/Y side is measured to the footprint centre, not the
/// NW cell. Z: native uses the building's stored coordinate Z (`+0xA4`,
/// unchanged by GetCoords), which is its NW/origin cell; Rust resolves the
/// refinery's `object_coordinate_z` at that same NW cell.
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
    rules: &RuleSet,
    miner_sid: u64,
    refinery_sid: u64,
    threshold_cells: u16,
) -> Option<bool> {
    let miner = sim.substrate.entities.get(miner_sid)?;
    let refinery = sim.substrate.entities.get(refinery_sid)?;
    if refinery.dying || refinery.health.current == 0 {
        return None;
    }

    let miner_x = i64::from(miner.position.rx) * 256 + miner.position.sub_x.to_num::<i64>();
    let miner_y = i64::from(miner.position.ry) * 256 + miner.position.sub_y.to_num::<i64>();
    let refinery_nw_x =
        i64::from(refinery.position.rx) * 256 + refinery.position.sub_x.to_num::<i64>();
    let refinery_nw_y =
        i64::from(refinery.position.ry) * 256 + refinery.position.sub_y.to_num::<i64>();
    // Same by-name lookup as `find_docking_bay`; a type the sim's interner
    // never produced (foreign-interner fixtures) degrades to a 1x1 footprint.
    let (w, h) = sim
        .interner
        .try_resolve(refinery.type_ref())
        .and_then(|name| rules.object_case_insensitive(name))
        .map(|obj| foundation_dimensions(&obj.foundation))
        .unwrap_or((1, 1));
    let (refinery_x, refinery_y) = building_get_coords_xy(refinery, w, h);
    let miner_z = object_coordinate_z(sim, miner, miner_x, miner_y);
    let refinery_z = object_coordinate_z(sim, refinery, refinery_nw_x, refinery_nw_y);

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
    let threshold = i64::from(threshold_cells) * 256;
    Some(distance > threshold)
}

#[cfg(test)]
mod gsi_04_03b_tests {
    use super::*;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::LocomotorState;

    fn empty_rules() -> RuleSet {
        RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str("")).expect("empty rules")
    }

    /// Mission_Harvest state 2 measures to `BuildingClass::GetCoords @
    /// 0x00447AC0` = the foundation centre. For a 4x3 refinery that is
    /// (+384, +256) leptons from the NW cell, which flips the 5-cell
    /// `HarvesterTooFarDistance` verdict on either side of the building.
    #[test]
    fn too_far_threshold_measures_to_the_foundation_centre() {
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[BuildingTypes]\n0=GAREFN\n[GAREFN]\nFoundation=4x3\nRefinery=yes\nDockUnload=yes\n",
        ))
        .expect("refinery rules");
        let mut sim = Simulation::new();
        let refinery_type = sim.interner.intern("GAREFN");
        let harv_type = sim.interner.intern("HARV");
        let mut refinery = GameEntity::test_default(2, "GAREFN", "Allies", 10, 10);
        refinery.type_ref = refinery_type;
        refinery.category = EntityCategory::Structure;
        sim.substrate.entities.insert(refinery);

        // East of the building: NW distance is sqrt(37) cells (far), centre
        // distance is 4.5 cells (x = 4224 vs centre 3072, dy = 0; within 5)
        // -> native keeps the narrow result.
        let mut east = GameEntity::test_default(1, "HARV", "Allies", 16, 11);
        east.type_ref = harv_type;
        sim.substrate.entities.insert(east);
        assert_eq!(
            return_exceeds_too_far_threshold(&sim, &rules, 1, 2, 5),
            Some(false),
            "east side: centre offset pulls a NW-far miner inside the threshold"
        );

        // West of the building: NW distance is sqrt(17) cells (near), centre
        // distance is sqrt(30.25) cells (beyond 5) -> native falls to wide.
        let mut west = GameEntity::test_default(3, "HARV", "Allies", 6, 11);
        west.type_ref = harv_type;
        sim.substrate.entities.insert(west);
        assert_eq!(
            return_exceeds_too_far_threshold(&sim, &rules, 3, 2, 5),
            Some(true),
            "west side: centre offset pushes a NW-near miner beyond the threshold"
        );

        // Without a known foundation the offset is zero (1x1 fallback).
        assert_eq!(
            return_exceeds_too_far_threshold(&sim, &empty_rules(), 3, 2, 5),
            Some(false)
        );
    }

    /// `Sqrt_Approx`'s 14-bit table rounds down, so with
    /// `HarvesterTooFarDistance=5` (1280 leptons) d = 1281 truncates to 1280
    /// (close) and only d = 1282 reads as far. 1x1 fallback, dy = dz = 0.
    #[test]
    fn too_far_threshold_follows_sqrt_approx_table_edge() {
        let mut sim = Simulation::new();
        let mut refinery = GameEntity::test_default(2, "GAREFN", "Allies", 0, 0);
        refinery.position.sub_x = SimFixed::from_num(0);
        refinery.position.sub_y = SimFixed::from_num(0);
        sim.substrate.entities.insert(refinery);
        let mut miner = GameEntity::test_default(1, "HARV", "Allies", 5, 0);
        miner.position.sub_x = SimFixed::from_num(1);
        miner.position.sub_y = SimFixed::from_num(0);
        sim.substrate.entities.insert(miner);

        assert_eq!(
            return_exceeds_too_far_threshold(&sim, &empty_rules(), 1, 2, 5),
            Some(false),
            "d = 1281 leptons: Sqrt_Approx yields 1280.9995, ftol 1280, close"
        );
        sim.substrate.entities.get_mut(1).unwrap().position.sub_x = SimFixed::from_num(2);
        assert_eq!(
            return_exceeds_too_far_threshold(&sim, &empty_rules(), 1, 2, 5),
            Some(true),
            "d = 1282 leptons is the first far distance"
        );
    }

    fn sloped_cell() -> ResolvedTerrainCell {
        ResolvedTerrainCell {
            rx: 0,
            ry: 0,
            source_tile_index: 0,
            source_sub_tile: 0,
            final_tile_index: 0,
            final_sub_tile: 0,
            is_wood_bridge_repair_tile: false,
            level: 0,
            filled_clear: true,
            tileset_index: Some(0),
            land_type: 0,
            yr_cell_land_type: 0,
            slope_type: 1,
            template_height: 0,
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: Default::default(),
            speed_costs: Default::default(),
            is_water: false,
            is_cliff_like: false,
            is_rough: false,
            is_road: false,
            height_in_pixels: 0,
            variant: 0,
            has_ramp: true,
            canonical_ramp: None,
            ground_walk_blocked: false,
            terrain_object_blocks: false,
            terrain_object_occupation: None,
            overlay_blocks: false,
            overlay_zone_type: None,
            outside_playfield: false,
            zone_type: 0,
            base_ground_walk_blocked: false,
            base_build_blocked: false,
            base_land_type: 0,
            base_yr_cell_land_type: 0,
            base_terrain_class: Default::default(),
            base_speed_costs: Default::default(),
            build_blocked: false,
            has_bridge_deck: false,
            bridge_walkable: false,
            bridge_transition: false,
            bridge_deck_level: 0,
            bridge_layer: None,
            bridge_facts: Default::default(),
            tube_index: None,
            radar_left: [0; 3],
            radar_right: [0; 3],
            accepts_smudge: true,
            allows_tiberium: false,
            has_damaged_data: false,
            bridgehead_anchor_class_at_load: None,
        }
    }

    #[test]
    fn gsi_04_03b_miner_return_distance_uses_terrain_bridge_and_altitude_z() {
        let mut sim = Simulation::new();
        let mut miner = GameEntity::test_default(1, "CMIN", "Allies", 0, 0);
        miner.position.sub_x = SimFixed::from_num(0);
        let mut refinery = GameEntity::test_default(2, "GAOREP", "Allies", 0, 0);
        refinery.position.sub_x = SimFixed::from_num(255);
        sim.substrate.entities.insert(miner);
        sim.substrate.entities.insert(refinery);
        sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(1, 1, vec![sloped_cell()]));

        assert_eq!(
            return_exceeds_too_far_threshold(&sim, &empty_rules(), 1, 2, 1),
            Some(true),
            "255 horizontal leptons plus the slope Z delta exceeds one cell"
        );

        sim.substrate.entities.get_mut(1).unwrap().position.sub_x = SimFixed::from_num(0);
        sim.substrate.entities.get_mut(2).unwrap().position.sub_x = SimFixed::from_num(0);
        assert_eq!(
            return_exceeds_too_far_threshold(&sim, &empty_rules(), 1, 2, 1),
            Some(false)
        );

        sim.substrate.entities.get_mut(1).unwrap().on_bridge = true;
        assert_eq!(
            return_exceeds_too_far_threshold(&sim, &empty_rules(), 1, 2, 1),
            Some(true),
            "OnBridge coordinate Z contributes the full deck offset"
        );

        let miner = sim.substrate.entities.get_mut(1).unwrap();
        miner.on_bridge = false;
        let mut locomotor = LocomotorState::for_test_kind(LocomotorKind::Fly);
        locomotor.altitude = SimFixed::from_num(300);
        miner.locomotor = Some(locomotor);
        assert_eq!(
            return_exceeds_too_far_threshold(&sim, &empty_rules(), 1, 2, 1),
            Some(true),
            "locomotor altitude contributes to raw object-coordinate Z"
        );
    }

    #[test]
    fn gsi_04_03b_miner_return_distance_falls_back_to_level_z_without_resolved_terrain() {
        let mut sim = Simulation::new();
        let mut miner = GameEntity::test_default(1, "CMIN", "Allies", 0, 0);
        miner.position.sub_x = SimFixed::from_num(0);
        miner.position.z = 0;
        let mut refinery = GameEntity::test_default(2, "GAOREP", "Allies", 0, 0);
        refinery.position.sub_x = SimFixed::from_num(255);
        refinery.position.z = 0;
        sim.substrate.entities.insert(miner);
        sim.substrate.entities.insert(refinery);
        assert!(sim.resolved_terrain.is_none());

        // Same pair as the terrain fixture above, which answers Some(true) from
        // the slope contribution. With no grid to resolve, each object degrades
        // to level-only Z: dz = 0, so 255 horizontal leptons stay inside one
        // cell — and the decision is still made rather than refused.
        assert_eq!(
            return_exceeds_too_far_threshold(&sim, &empty_rules(), 1, 2, 1),
            Some(false)
        );

        sim.substrate.entities.get_mut(2).unwrap().position.z = 3;
        assert_eq!(
            return_exceeds_too_far_threshold(&sim, &empty_rules(), 1, 2, 1),
            Some(true),
            "the fallback Z is position.z * LEPTONS_PER_LEVEL, not a dropped term"
        );
    }

    #[test]
    fn gsi_04_05_sequential_miner_process_reserves_head_before_next_process() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("AMERICANS");
        let type_ref = sim.interner.intern("HARV");
        for (entity_id, rx, facing) in [(1, 1, 0x40), (2, 3, 0xC0)] {
            let mut miner = GameEntity::test_default(entity_id, "HARV", "AMERICANS", rx, 2);
            miner.owner = owner;
            miner.type_ref = type_ref;
            miner.facing = facing;
            miner.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
            miner.drive_locomotion = Some(Default::default());
            sim.substrate.entities.insert(miner);
            assert!(matches!(
                sim.reveal(entity_id),
                crate::sim::world::RevealOutcome::Revealed { .. }
            ));
        }

        let grid = PathGrid::new(5, 5);
        let shared_head = (2, 2);
        issue_move_if_idle(
            &mut sim,
            None,
            &grid,
            1,
            shared_head,
            SimFixed::from_num(128),
            None,
        );
        assert!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .drive_locomotion
                .as_ref()
                .unwrap()
                .head_to
                .is_none(),
            "the helper issues an order; Process owns head admission"
        );
        sim.process_ground_locomotor_for_test(1, None, Some(&grid), None)
            .expect("the first miner Process commits its head reservation");

        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .and_then(|entity| entity.drive_locomotion.as_ref())
                .and_then(|drive| drive.occupation_head_to)
                .map(|head| (head.rx, head.ry)),
            Some(shared_head)
        );
        assert!(sim.substrate.cell_occupation.occupied_by_other(
            shared_head.0,
            shared_head.1,
            MovementLayer::Ground,
            2,
        ));

        issue_move_if_idle(
            &mut sim,
            None,
            &grid,
            2,
            shared_head,
            SimFixed::from_num(128),
            None,
        );
        sim.process_ground_locomotor_for_test(2, None, Some(&grid), None)
            .expect("the second miner Process observes the existing reservation");

        let second = sim.substrate.entities.get(2).expect("second miner");
        assert_ne!(
            second
                .drive_locomotion
                .as_ref()
                .and_then(|drive| drive.occupation_head_to)
                .map(|head| (head.rx, head.ry)),
            Some(shared_head),
            "the second miner must observe the first Process head mark immediately"
        );
        // Unit741970 names the reserved cell unchanged; only the second
        // miner's own Process refuses the reserved head (above).
        assert_eq!(
            second
                .movement_target
                .as_ref()
                .and_then(|movement| movement.final_goal),
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
/// on every dispatch, the search state's archive-consume and still-driving
/// returns, and any cursor outside the native handler's switch.
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
    pub(super) speed: SimFixed,
    pub(super) miner: Miner,
    /// FSM cursor working copy — decoded from `MissionCom::handler_state` at
    /// dispatch entry, committed back through it at dispatch commit.
    pub(super) state: MinerState,
    /// Handler return value: frames until the next dispatch, written into the
    /// mission dispatch timer by the commit (the native post-handler epilogue).
    pub(super) dispatch_delay: i32,
    /// Buffered miner state change events — flushed to entity at commit.
    pub(super) debug_events: Vec<(String, String)>,
    /// Buffered dock phase change events — flushed to entity at commit.
    pub(super) debug_dock_events: Vec<(String, String)>,
}

/// Build the dispatch snapshot for one live, non-dying, non-slave miner.
/// Returns `None` when the object is not a dispatchable miner.
pub(super) fn build_miner_snapshot(
    sim: &Simulation,
    rules: &RuleSet,
    id: u64,
) -> Option<MinerSnapshot> {
    let entity = sim.substrate.entities.get(id)?;
    // A Dying miner corpse (sold/captured this tick, awaiting the end-of-tick
    // drain) must not move, harvest, or deposit.
    if entity.dying {
        return None;
    }
    let miner = entity.miner.as_ref()?;
    // Slave Miners use their own system (slave_miner.rs) — never dispatched here.
    if miner.kind == MinerKind::Slave {
        return None;
    }
    // Use the authentic RA2 speed formula: Speed=4 → ~0.586 cells/sec.
    // `FootClass::GetCurrentSpeed @ 0x004DB1A0`: the miner's drive loop asks the
    // same getter every mover does, so a `FASTER` miner takes the multiply here.
    let obj = sim.object_type(entity.type_ref(), rules);
    let speed: SimFixed = crate::sim::combat::veterancy::entity_mover_speed_leptons_per_second(
        entity,
        obj,
        obj.map_or(4, |o| o.speed.max(1)),
        rules.general.veteran_speed,
    );
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
        rx: entity.position.rx,
        ry: entity.position.ry,
        speed,
        miner: miner.clone(),
        state: cursor.unwrap_or(MinerState::SearchOre),
        dispatch_delay: DISPATCH_NEXT_FRAME,
        debug_events: Vec::new(),
        debug_dock_events: Vec::new(),
    })
}

/// Commit one dispatched snapshot back to the entity: miner mutations, the
/// FSM cursor of record (`MissionCom::handler_state`), the post-handler
/// dispatch-timer epilogue (verified host shape: start = current frame,
/// delay = handler return), buffered debug events, and the render-side
/// harvest-visual flags (the former global-tick Phases 3/4/4b for one object).
pub(super) fn commit_miner_snapshot(sim: &mut Simulation, snap: &MinerSnapshot, now: u32) {
    let Some(entity) = sim.substrate.entities.get_mut(snap.entity_id) else {
        return;
    };
    entity.miner = Some(snap.miner.clone());
    entity.mission.set_handler_state(snap.state.cursor());
    entity
        .mission
        .write_dispatch_epilogue(now as i32, snap.dispatch_delay);
    for (from, to) in &snap.debug_events {
        entity.push_debug_event(
            sim.session.tick as u32,
            DebugEventKind::MinerStateChange {
                from: from.clone(),
                to: to.clone(),
            },
        );
    }
    for (from, to) in &snap.debug_dock_events {
        entity.push_debug_event(
            sim.session.tick as u32,
            DebugEventKind::DockPhaseChange {
                from: from.clone(),
                to: to.clone(),
            },
        );
    }
    // Drive VoxelAnimation + HarvestOverlay (oregath.shp) from the Harvest
    // cursor — render-side flags, never hashed.
    let is_harvesting: bool = snap.state == MinerState::Harvest;
    if let Some(ref mut va) = entity.voxel_animation {
        va.playing = is_harvesting;
        if !is_harvesting {
            va.frame = 0;
            va.elapsed_frames = 0;
        }
    }
    if let Some(ref mut ho) = entity.harvest_overlay {
        if is_harvesting && !ho.visible {
            ho.visible = true;
            ho.frame = 0;
            ho.elapsed_frames = 0;
        } else if !is_harvesting && ho.visible {
            ho.visible = false;
            ho.frame = 0;
            ho.elapsed_frames = 0;
        }
    }
}

/// Test-only mirror of the production Harvest dispatch walk: the same
/// per-entity dispatch (timer gate + epilogue) the host Unit arm performs, in
/// live-object order, with the legacy stable-id fallback for direct-insert
/// fixtures that never build a LogicVector.
#[cfg(test)]
pub(crate) fn tick_miners(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    path_grid: Option<&PathGrid>,
) {
    tick_miners_test_walk(
        sim,
        rules,
        config,
        path_grid,
        Some(crate::sim::tiberium::test_support::overlay_registry()),
    );
}

#[cfg(test)]
pub(super) fn tick_miners_test_walk(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) {
    let live_order = sim.live_object_order_snapshot();
    let keys: Vec<u64> = if live_order.is_empty() {
        sim.substrate.entities.keys_sorted()
    } else {
        live_order
    };
    for id in keys {
        super::harvest_mission::dispatch_harvest_for_object(
            sim,
            rules,
            config,
            path_grid,
            overlay_registry,
            id,
        );
    }
}

pub(super) fn process_miner(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    snap: &mut MinerSnapshot,
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
    //
    // The `Dock` cursor is exempt: past the state-3 hand-off its phases run
    // under native Mission_Enter / Mission_Deploy (the miner is no longer
    // dispatched through Mission_Harvest there), and those handlers carry no
    // dock-ownership preamble — a mind-controlled miner still finishes
    // unloading into the refinery it entered. VERA dispatches the Dock
    // cursor from this handler for structural reasons only.
    let legacy_dock = snap.state == MinerState::Dock && snap.miner.kind != MinerKind::War;
    if !legacy_dock && !house_owns_dock_instance(sim, rules, snap) {
        queue_guard_from_harvest(sim, snap);
        snap.dispatch_delay = DISPATCH_NEXT_FRAME;
        return;
    }

    let state_before = format!("{:?}", snap.state);
    match snap.state {
        MinerState::SearchOre => {
            handle_search_ore(sim, rules, config, path_grid, overlay_registry, snap)
        }
        MinerState::MoveToOre => {
            handle_move_to_ore(sim, rules, config, path_grid, overlay_registry, snap)
        }
        MinerState::Harvest => {
            handle_harvest(sim, rules, config, path_grid, overlay_registry, snap)
        }
        MinerState::ReturnToRefinery => {
            handle_return(sim, rules, config, path_grid, overlay_registry, snap);
            // Native return/finding-home state has no per-frame exit: every
            // dispatch leaves through the default Rate epilogue.
            arm_rate_epilogue(sim, rules, snap);
        }
        MinerState::Dock if snap.miner.kind == MinerKind::War => handle_handoff_war(sim, snap),
        MinerState::Dock => super::miner_dock_sequence::handle_dock_sequence(
            sim,
            rules,
            config,
            path_grid,
            overlay_registry,
            snap,
        ),
        MinerState::Unload => {
            // Legacy state — production code never enters this path. If we
            // encounter it (e.g., a save from before the FSM rewrite), fall
            // through to SearchOre. Outside the native handler's switch, so
            // it exits through the default epilogue.
            snap.state = MinerState::SearchOre;
            arm_rate_epilogue(sim, rules, snap);
        }
        MinerState::WaitNoOre => {
            if handle_going_to_idle(sim, rules, config, path_grid, overlay_registry, snap) {
                // Native state 4 has no `return 1` exit: every dispatch falls
                // into the default Rate epilogue (`0x0073EF97`).
                arm_rate_epilogue(sim, rules, snap);
            }
        }
        MinerState::ForcedReturn => {
            handle_forced_return(sim, rules, config, path_grid, overlay_registry, snap);
            // VERA-internal cursor; outside the native handler's switch, so
            // it exits through the default epilogue like any high cursor.
            arm_rate_epilogue(sim, rules, snap);
        }
    }
    let state_after = format!("{:?}", snap.state);
    if state_before != state_after {
        log::info!(
            "MINER {} state: {} → {} pos=({},{}) target_ore={:?} cargo={} timer={:?}",
            snap.entity_id,
            state_before,
            state_after,
            snap.rx,
            snap.ry,
            snap.miner.target_ore_cell,
            snap.miner.cargo.len(),
            snap.miner.harvest_timer,
        );
        snap.debug_events.push((state_before, state_after));
    }
}

// -- State handlers --

/// Build the combined scan filter — zone reachability AND cell occupancy.
///
/// Mirrors gamemd's `FootClass::Is_Cell_Harvestable`, which gates each
/// ring-1+ candidate cell through a zone-connectivity check plus a
/// per-cell `Can_Enter_Cell` call (cell occupancy: vehicles, terrain
/// objects, building footprints).
///
/// Returns `None` if no zone grid or anchor is available — caller falls
/// back to an unfiltered scan for this tick.
fn build_scan_filter<'a>(
    sim: &'a Simulation,
    path_grid: Option<&'a PathGrid>,
    snap: &MinerSnapshot,
) -> Option<Box<dyn Fn((u16, u16)) -> bool + 'a>> {
    let entity = sim.substrate.entities.get(snap.entity_id);
    let mz = entity
        .and_then(|e| e.locomotor.as_ref())
        .map(|loc| loc.movement_zone)
        .unwrap_or(MovementZone::Normal);
    let layer = entity
        .map(|e| e.movement_layer_or_ground())
        .unwrap_or(MovementLayer::Ground);
    let zone_grid = sim.zone_grid.as_ref()?;
    let anchor = effective_zone_cell(zone_grid, mz, snap.rx, snap.ry)?;
    let occupancy = &sim.substrate.occupancy;
    let self_id = snap.entity_id;

    Some(Box::new(move |ore_cell: (u16, u16)| {
        if !ore_reachable(zone_grid, mz, layer, anchor, ore_cell) {
            return false;
        }
        is_cell_path_clear_for_scan(occupancy, path_grid, ore_cell, self_id)
    }))
}

/// True if the cell has no static blocker (terrain object, building
/// footprint set in PathGrid) and no non-self vehicle/structure occupant
/// (OccupancyGrid). Infantry are not blockers.
///
/// Used by ring-1+ scan candidates only — ring 0 is always allowed (the
/// harvester is allowed to harvest its own cell even if it appears as a
/// blocker to itself).
pub(crate) fn is_cell_path_clear_for_scan(
    occupancy: &OccupancyGrid,
    path_grid: Option<&PathGrid>,
    cell: (u16, u16),
    self_id: u64,
) -> bool {
    if let Some(grid) = path_grid
        && !grid.is_walkable(cell.0, cell.1)
    {
        return false;
    }
    if let Some(occ) = occupancy.get(cell.0, cell.1) {
        let any_non_self_blocker = occ.blockers(MovementLayer::Ground).any(|id| id != self_id);
        if any_non_self_blocker {
            return false;
        }
    }
    true
}

fn handle_search_ore(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    snap: &mut MinerSnapshot,
) {
    // gamemd's Mission_Harvest state 0 checks full storage before scanning
    // ore, so a full miner that lost its refinery keeps trying to return.
    if snap.miner.is_full() {
        snap.miner.target_ore_cell = None;
        snap.state = MinerState::ReturnToRefinery;
        return;
    }

    // L10: the post-unload ore search is paced by the Mission_Harvest epilogue's
    // RandomRanged(0,2) jitter, armed at the state-4 dock exit. Wait it out so the
    // search resumes at exit_frame + jitter, not immediately. For every other
    // entry the harvest timer is long-elapsed (always due), so this is a no-op.
    if !snap.miner.harvest_timer.due(sim.session.binary_frame) {
        return;
    }

    /// Scan decision, computed under the scan filter's immutable `sim` borrow
    /// and committed after it drops (the epilogue draw needs `&mut sim`).
    enum ScanOutcome {
        /// Ghost-cell archive consumed — the native archive-target return.
        Archive((u16, u16)),
        /// Fresh ore target from the bounded scan.
        Found((u16, u16)),
        /// No reachable ore inside the scan radius.
        NoOre,
    }

    let outcome = {
        // Combined scan filter — zone reachability + cell occupancy.
        // Returns None if zone_grid / anchor is missing; caller falls back to
        // an unfiltered scan that tick.
        let scan_filter = build_scan_filter(sim, path_grid, snap);
        let filter_ref: Option<&dyn Fn((u16, u16)) -> bool> = scan_filter.as_deref();

        // Archive ghost-cell consumption: if `last_harvest_cell` is set,
        // drive straight to it and clear. The archive is written by
        // `save_archive_via_short_scan` when the miner becomes full.
        // Reachability is re-checked because the patch may have been walled
        // off between the save and the next cycle.
        let mut archive_hit = None;
        if let Some(archive) = snap.miner.last_harvest_cell {
            let archive_has_ore = resource_cell_present(sim, rules, overlay_registry, archive);
            let archive_reachable = filter_ref.is_none_or(|f| f(archive));
            if archive_has_ore && archive_reachable {
                archive_hit = Some(ScanOutcome::Archive(archive));
            } else {
                // Stale archive (depleted or unreachable) — drop it so we
                // don't keep retrying.
                snap.miner.last_harvest_cell = None;
            }
        }

        // Long-range bounded scan from the miner's current position
        // (TiberiumLongScan). Single scan with no separate short-scan
        // pre-pass — the search expands outward and picks the best cell
        // within radius. Used for both war miners and chrono miners.
        //
        // That radius is the whole search. gamemd's scan is hard-bounded by
        // it and breaks on the first ring with a hit; there is no second,
        // wider pass behind it, so a miss is a miss and the miss arm below —
        // not a cross-map drive — is what a player sees.
        //
        // Chrono miners DRIVE to ore, not warp: the destination-setting path
        // only keeps the Teleport locomotor when the miner already holds a
        // radio contact, which a miner heading out to ore never does, so it
        // swaps in a Drive piggyback. Only the inbound trip (ore → refinery)
        // uses the warp.
        archive_hit.unwrap_or_else(|| {
            search_local_resource(
                sim,
                rules,
                overlay_registry,
                (snap.rx, snap.ry),
                config.long_scan_radius,
                filter_ref,
            )
            .map_or(ScanOutcome::NoOre, ScanOutcome::Found)
        })
    };

    match outcome {
        ScanOutcome::Archive(cell) => {
            snap.miner.target_ore_cell = Some(cell);
            snap.state = MinerState::MoveToOre;
            snap.miner.last_harvest_cell = None;
            // The native archive-consume exit goes through the default Rate
            // epilogue, not the per-frame return.
            arm_rate_epilogue(sim, rules, snap);
        }
        ScanOutcome::Found(cell) => {
            snap.miner.target_ore_cell = Some(cell);
            snap.state = MinerState::MoveToOre;
            // A scan that answers the miner's own cell is gamemd's one
            // productive return with nothing to drive to: per-frame dispatch,
            // no epilogue draw. Every other hit sets the destination inside
            // this same dispatch and then falls through into the default Rate
            // epilogue — so the drive command and the epilogue's single
            // RandomRanged(0, 2) both belong to the scan dispatch, not to a
            // later one.
            if cell != (snap.rx, snap.ry) {
                if let Some(grid) = path_grid {
                    let _ = issue_stock_miner_drive_move_with_overlay_registry(
                        sim,
                        rules,
                        grid,
                        snap.entity_id,
                        cell,
                        overlay_registry,
                    );
                }
                arm_rate_epilogue(sim, rules, snap);
            }
        }
        ScanOutcome::NoOre => {
            // `0x0073E8EE..0x0073E91C`: no ore, no destination, no archive
            // writes state 4, `Techno+0x3D0 = 1` and, for a `Harvester=yes`
            // type, `House+0x242 = 1`, then returns the fixed 105-frame wait
            // directly — bypassing the Rate epilogue, so no RNG draw. What
            // runs when the wait expires is `handle_going_to_idle`.
            //
            // `+0x3D0` is not carried on the entity: its only in-handler
            // reader is the state-4 RepairBay probe, whose outcome the same
            // dispatch overwrites (see `handle_going_to_idle`), and state 4 is
            // reachable from this arm alone, where the byte is always 1. Its
            // other readers (`BuildingClass::MissionRepairAndProduce`
            // `0x0044C4BC`, AI-house only) are outside this lane.
            snap.state = MinerState::WaitNoOre;
            // `rescan_cooldown` is no longer read by the idle state (native
            // state 4 has no internal gate); it stays armed for snapshot/hash
            // continuity of the persisted `Miner` block.
            snap.miner.rescan_cooldown.arm(
                sim.session.binary_frame,
                u32::from(config.rescan_cooldown_ticks),
            );
            if let Some(house) = sim.houses.get_mut(&snap.owner) {
                // `MOV byte ptr [ECX+0x242], 1` at `0x0073E911`, gated on
                // `UnitType+0xE0E` (`Harvester=yes`) — true for every kind
                // dispatched here (slave hosts never reach this handler).
                house.harvester_no_ore = true;
            }
            snap.dispatch_delay = i32::from(config.rescan_cooldown_ticks);
        }
    }
}

fn handle_move_to_ore(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    snap: &mut MinerSnapshot,
) {
    let has_destination_or_movement =
        sim.substrate
            .entities
            .get(snap.entity_id)
            .is_some_and(|entity| {
                entity.navigation.nav_com.is_some() || entity.movement_target.is_some()
            });

    // Native Search_For_Tiberium_And_Move returns immediately for a non-null
    // owner NavCom before target validation, arrival, or scan; the handler
    // then exits through the default Rate epilogue (the still-driving
    // return). MovementTarget remains Rust's transitional second owner until
    // the broader Drive host is migrated.
    if has_destination_or_movement {
        arm_rate_epilogue(sim, rules, snap);
        return;
    }

    let Some(current_target) = snap.miner.target_ore_cell else {
        snap.state = MinerState::SearchOre;
        return;
    };

    // Check if current target has been depleted.
    let still_has_ore = resource_cell_present(sim, rules, overlay_registry, current_target);
    if !still_has_ore {
        snap.miner.target_ore_cell = None;
        snap.state = MinerState::SearchOre;
        return;
    }

    // Wait for any in-progress teleport to complete (chrono delay).
    // Must be checked BEFORE the arrival check — during ChronoDelay the
    // entity is already at the target position but still materializing
    // (50% translucent). Transitioning to Harvest during delay would skip
    // the warp-in visual.
    let has_teleport = sim
        .substrate
        .entities
        .get(snap.entity_id)
        .is_some_and(|e| e.teleport_state.is_some());
    if has_teleport {
        return;
    }

    // Rescan on re-entry, NOT per tick. gamemd's Mission_Harvest state 0
    // wraps its entire body — scan, cell lookup and destination write — in
    // the "no destination held" guard above; while a destination is held the
    // state never looks at ore. So this scan runs only on the dispatches that
    // get past that guard, i.e. after the drive ends (arrival or abort). A
    // *distant* destination going impassable therefore retargets nothing
    // until the miner's own movement reaches it. Exactly one candidate
    // fast-retarget path was checked and ruled out: the destination repair
    // inside the locomotor's path search, which runs from within a re-path
    // and nudges the destination by a cell rather than re-running the ore
    // scan. Whether any *other* mechanism reacts to that trigger is
    // UNCHECKED — no exhaustive sweep was run. Here the scan re-picks the
    // best cell from the harvester's current position; it is deterministic
    // given unchanged inputs, so a world that has not changed returns the
    // same cell and the assignment is a no-op.
    let new_target = {
        let scan_filter = build_scan_filter(sim, path_grid, snap);
        let filter_ref: Option<&dyn Fn((u16, u16)) -> bool> = scan_filter.as_deref();
        search_local_resource(
            sim,
            rules,
            overlay_registry,
            (snap.rx, snap.ry),
            config.long_scan_radius,
            filter_ref,
        )
    };
    let target = new_target.unwrap_or(current_target);
    if target != current_target {
        snap.miner.target_ore_cell = Some(target);
    }

    // Arrived?
    if (snap.rx, snap.ry) == target {
        snap.state = MinerState::Harvest;
        // This physical-arrival anchor is legacy Rust behavior; native initializes
        // the timer when search/move succeeds, a separately tracked acquisition-
        // timing drift. Retain +1 for the verified mission-before-timer observation.
        snap.miner.harvest_timer.arm(
            sim.session.binary_frame,
            u32::from(config.harvest_tick_interval) + 1,
        );
        return;
    }

    if let Some(grid) = path_grid {
        let _ = issue_stock_miner_drive_move_with_overlay_registry(
            sim,
            rules,
            grid,
            snap.entity_id,
            target,
            overlay_registry,
        );
    }
    // VERA-internal, gamemd equivalent UNCHECKED. This cursor has no native
    // counterpart at all, and the epilogue is armed here whether or not the
    // drive command was accepted. What the decompile actually shows is only
    // the accepted case: a destination that took reaches the default Rate
    // epilogue, because the handler's next test reads a now-non-null
    // destination slot. Whether a *refused* destination lands there too is
    // unchecked — VERA's mover can refuse where the native call cannot.
    arm_rate_epilogue(sim, rules, snap);
}

fn handle_harvest(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    snap: &mut MinerSnapshot,
) {
    // Frame-anchored gate (was a per-tick countdown).
    if !snap.miner.harvest_timer.due(sim.session.binary_frame) {
        return;
    }

    if snap.miner.is_full() {
        // Harvest_Ore_Tick checks full storage before Reduce_Tiberium, resets its
        // timer, and returns failure. Mission_Harvest then writes return state
        // before choosing the ghost/archive cell; state-2 work waits for the next
        // mission dispatch.
        snap.miner.harvest_timer.reset(sim.session.binary_frame);
        snap.state = MinerState::ReturnToRefinery;
        save_archive_via_short_scan(sim, rules, config, path_grid, overlay_registry, snap);
        return;
    }

    let cell = (snap.rx, snap.ry);
    let empty: u16 = snap
        .miner
        .capacity_bales
        .saturating_sub(snap.miner.cargo.len() as u16);

    // `UnitClass::Harvest_Ore_Tick` @ 0x0073D450 requests ONE density level
    // per harvest gate, not the free capacity. Disassembly 0x0073D556..
    // 0x0073D5A1: `FILD [type+0x800]` (Storage), `CALL GetTotalAmount`,
    // `FSUBR` (Storage - total), `FCOMP [0x007E2AC8]` (1.0f), `TEST AH,0x41` /
    // `JNZ` keeps the difference only when it is <= 1.0, otherwise
    // `FLD [0x007E2AC8]` loads 1.0; then `CALL Math__ftol`, `PUSH EAX`,
    // `CALL Reduce_Tiberium` @ 0x00480A80. So request = ftol(min(1.0,
    // Storage - total)). With integer cargo the difference is always >= 1 here
    // (the full check above already returned), so the request is exactly 1.
    // `FUN_00522E70` (slave harvest tick) uses the same min(1.0, ..) shape.
    let request: u16 = empty.min(1);

    // Shared CellClass::Reduce_Tiberium boundary: caller owns cargo insertion,
    // while the helper owns overlay/resource/dirty/queue side effects.
    let reduction =
        sim.reduce_tiberium_at_with_native_context(cell, request, Some(rules), overlay_registry);

    if reduction.removed_amount > 0 {
        let Some(resource_type) = reduction.resource_type else {
            return;
        };
        // RESIDUAL (VERA-internal, gamemd equivalent UNCHECKED beyond the
        // stock map data): native `StorageClass` (`UnitClass+0x33C`) keeps
        // four per-`TiberiumType` slots and `Mission_Unload` drains them by
        // `FindFirstNonEmptySlot` in index order 0..3, each slot valued by
        // its own type's `Value=`; VERA folds every overlay family into
        // `ResourceType::{Ore, Gem}` (`TiberiumTypeId` 0 / 1 values above).
        // Trigger: a map placing TIB2/TIB3 (Vinifera/Aboreus) overlays —
        // no stock skirmish map does. Effect: those bales are valued as
        // type 0 (Riparius) and drain in the Ore slot, so the unload runs
        // one 15-frame slot gate fewer than native. Frequency: zero on stock
        // maps. Downstream: none for the stock ruleset.
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

        // A positive extraction is success even when it fills storage. Native
        // Mission_Harvest remains in state 1 and observes fullness only at the
        // next helper gate: 9 * HarvesterLoadRate + 1 frame numbers under the
        // verified mission-before-timer order.
        snap.miner.harvest_timer.arm(
            sim.session.binary_frame,
            u32::from(config.harvest_tick_interval) + 1,
        );
        return;
    }

    // No bales extracted while not full. Run the caller-owned short continuation
    // scan; a hit moves toward the next patch, while a miss begins the existing
    // no-resource return path.

    // Short scan. The filter's closure captures `&sim`; scope it so the
    // immutable borrow drops before `begin_return` needs `&mut sim` below.
    let continuation_target = {
        let scan_filter = build_scan_filter(sim, path_grid, snap);
        let filter_ref: Option<&dyn Fn((u16, u16)) -> bool> = scan_filter.as_deref();
        search_local_resource(
            sim,
            rules,
            overlay_registry,
            (snap.rx, snap.ry),
            config.local_continuation_radius,
            filter_ref,
        )
    };
    if let Some(next_cell) = continuation_target {
        snap.miner.target_ore_cell = Some(next_cell);
        snap.state = MinerState::MoveToOre;
        return;
    }

    // Scan miss while not full → return to refinery, clear archive. Native
    // state 1 only writes Status 2 and returns 1 (`0x0073EAEE..0x0073EB0D`);
    // the Chrono Miner keeps the legacy same-dispatch selection.
    snap.miner.last_harvest_cell = None;
    if snap.miner.kind == MinerKind::War {
        snap.state = MinerState::ReturnToRefinery;
        return;
    }
    begin_return(sim, rules, config, path_grid, overlay_registry, snap);
}

/// Save a fresh ghost-cell archive by running a short-radius scan from
/// the miner's current position. The due full-failure caller invokes this only
/// after selecting Return, so the next `SearchOre` cycle can return directly to
/// a nearby still-productive patch. On scan miss, clears the archive.
fn save_archive_via_short_scan(
    sim: &Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    snap: &mut MinerSnapshot,
) {
    let scan_filter = build_scan_filter(sim, path_grid, snap);
    let filter_ref: Option<&dyn Fn((u16, u16)) -> bool> = scan_filter.as_deref();
    snap.miner.last_harvest_cell = search_local_resource(
        sim,
        rules,
        overlay_registry,
        (snap.rx, snap.ry),
        config.local_continuation_radius,
        filter_ref,
    );
}

/// `UnitClass::Mission_Harvest @ 0x0073E5E0` state 2 (FINDING_HOME) for a
/// War Miner, `0x0073EB2C..0x0073EE72`. Every exit is the caller's Rate
/// epilogue. Native evidence: tools/spatial_oracle/refinery_dock.json
/// `mission_harvest` rows.
///
/// - With a NavCom (still driving) it only waits (`0x0073EB5A`).
/// - The narrow `Find_Docking_Bay(Dock, 0, 0)` result within
///   `HarvesterTooFarDistance` gets HELLO; ROGER writes state 3
///   (`0x0073EB7E..0x0073EE68`).
/// - Otherwise the wide pass (`0x0073EC1F`) finds the dock to wait by: within
///   0x300 leptons the miner waits where it is (`0x0073ECD0`); farther out it
///   drives to the nearby passable cell around the dock's NW cell plus art
///   `QueueingCell=` (`0x0073ECDF..0x0073EDBB`), or clears its destination
///   when there is none.
///
/// VERA-internal: a player return order (`Command::MinerReturn`, the
/// ForcedReturn cursor) pins both passes to its refinery; the native order is
/// an Enter mission on the refinery, not yet represented.
fn handle_return_war(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    snap: &mut MinerSnapshot,
) {
    let id = snap.entity_id;
    if sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.navigation.nav_com.is_some())
    {
        return;
    }
    let pinned = snap
        .miner
        .forced_return
        .then_some(snap.miner.reserved_refinery)
        .flatten()
        .filter(|&bay| sim.substrate.entities.get(bay).is_some());
    let narrow = pinned.or_else(|| find_docking_bay(sim, rules, snap, false));
    if let Some(bay) = narrow
        && return_exceeds_too_far_threshold(sim, rules, id, bay, config.too_far_threshold_standard)
            == Some(false)
        && let Some(capacity) = refinery_dock_capacity_for_sid(sim, rules, bay)
        && miner_dock::hello(sim, id, bay, capacity) == ContactAdmission::Accepted
    {
        snap.state = MinerState::Dock;
        snap.miner.forced_return = false;
        snap.miner.reserved_refinery = None;
        return;
    }
    let Some(bay) = pinned.or_else(|| find_docking_bay(sim, rules, snap, true)) else {
        return;
    };
    if return_exceeds_too_far_threshold(sim, rules, id, bay, REFUSED_HELLO_STAGING_CELLS)
        != Some(true)
    {
        return;
    }
    match refinery_staging_cell(sim, rules, bay) {
        Some(cell) => {
            sim.set_unit_cell_destination(id, cell, rules);
        }
        None => {
            sim.assign_null_destination(id, Some(rules));
        }
    }
}

/// `MapClass::Find_Nearby_Passable_Cell @ 0x0056DC20` as Mission_Harvest
/// state 2 calls it at `0x0073ED75`: seeded at the dock's NW cell
/// (`Location / 256`) plus the low words of art `QueueingCell=`, with
/// SpeedType Wheel, no zone, MovementZone Normal, bridges allowed, no height
/// or occupancy check and no target cell (frame-modulo pick). `None` is the
/// (0, 0) no-cell answer.
fn refinery_staging_cell(sim: &Simulation, rules: &RuleSet, bay: u64) -> Option<(u16, u16)> {
    use crate::rules::locomotor_type::SpeedType;
    use crate::sim::find_nearby_cell::{
        NearbyAnchorGate, NearbyFootprint, NearbyQuery, PassabilityArgs, find_nearby_passable_cell,
        map_owned_radius_cap,
    };
    let building = sim.substrate.entities.get(bay)?;
    let queueing = sim
        .object_type(building.type_ref(), rules)
        .map_or([0, 0], |object| object.queueing_cell);
    let seed = (
        i32::from((building.position.rx as i16).wrapping_add(queueing[0] as i16)),
        i32::from((building.position.ry as i16).wrapping_add(queueing[1] as i16)),
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
            anchor_gate: NearbyAnchorGate::NativeHeightAware,
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
fn handle_handoff_war(sim: &mut Simulation, snap: &MinerSnapshot) {
    let now = sim.session.binary_frame;
    let _ = sim.mission_queue_exact(
        snap.entity_id,
        MissionId::from_known(MissionType::Enter),
        0,
        now,
        &EntityReadyInputProvider,
    );
}

fn handle_return(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    snap: &mut MinerSnapshot,
) {
    if snap.miner.kind == MinerKind::War {
        handle_return_war(sim, rules, config, snap);
        return;
    }
    let has_teleport = sim
        .substrate
        .entities
        .get(snap.entity_id)
        .is_some_and(|e| e.teleport_state.is_some());
    if has_teleport {
        return;
    }

    let Some(ref_sid) = snap.miner.reserved_refinery else {
        if let Some(rsid) = select_return_refinery(sim, rules, config, snap) {
            snap.miner.reserved_refinery = Some(rsid);
            if try_issue_chrono_far_return_teleport(sim, rules, config, path_grid, snap, rsid) {
                return;
            }
            if try_begin_close_return_radio(
                sim,
                rules,
                config,
                path_grid,
                overlay_registry,
                snap,
                rsid,
            ) {
                return;
            }
        }
        // No bay from either pass (the house still owns a dock instance, or
        // the preamble would already have queued Guard): native state 2 sets
        // no destination and leaves through the Rate epilogue, re-running the
        // whole selection on the next dispatch. Stay put.
        return;
    };

    let dock = refinery_dock_for_sid(sim, ref_sid)
        .filter(|_| miner_dock::same_house(sim, ref_sid, snap.entity_id));
    let Some(dock) = dock else {
        miner_dock::break_contact(sim, snap.entity_id, ref_sid);
        snap.miner.reserved_refinery = None;
        snap.miner.dock_queued = false;
        snap.miner.dock_phase = RefineryDockPhase::Approach;
        snap.miner.dock_enter_retry.clear();
        snap.miner.exit_cell = None;
        if snap.miner.is_full() {
            snap.miner.target_ore_cell = None;
            snap.state = MinerState::ReturnToRefinery;
        } else {
            snap.state = MinerState::SearchOre;
        }
        return;
    };

    let moving = sim
        .substrate
        .entities
        .get(snap.entity_id)
        .is_some_and(|entity| entity.movement_target.is_some());
    if !moving && try_issue_chrono_far_return_teleport(sim, rules, config, path_grid, snap, ref_sid)
    {
        return;
    }
    if try_begin_close_return_radio(
        sim,
        rules,
        config,
        path_grid,
        overlay_registry,
        snap,
        ref_sid,
    ) {
        return;
    }

    // Fallback contact test. With the close-return radio now covering every
    // kind inside its too-far distance and the far paths covering the rest,
    // this arm is reached only when the distance decision itself is
    // unavailable (refinery dying / off-grid coordinates); it is kept as the
    // VERA-internal safety net, gamemd equivalent UNCHECKED.
    let at_dock = (snap.rx, snap.ry) == dock;
    let contact = if snap.miner.kind == MinerKind::Chrono {
        at_dock
    } else {
        let stopped_close_enough =
            sim.substrate
                .entities
                .get(snap.entity_id)
                .is_some_and(|entity| {
                    entity.movement_target.is_none()
                        && is_within_close_enough(
                            (snap.rx, snap.ry),
                            dock,
                            rules.general.close_enough,
                        )
                });
        is_adjacent_or_at((snap.rx, snap.ry), dock) || stopped_close_enough
    };

    if contact {
        snap.state = MinerState::Dock;
        snap.miner.dock_phase = RefineryDockPhase::Approach;
        snap.miner.dock_enter_retry.clear();
        return;
    }

    if let Some(grid) = path_grid {
        issue_move_if_idle(
            sim,
            Some(rules),
            grid,
            snap.entity_id,
            dock,
            snap.speed,
            overlay_registry,
        );
    }
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
///    same dispatch. `Find_Docking_Bay @ 0x004DF040` → `FootClass::
///    Find_Nearest_Dock_Of_Type @ 0x004DEE80` is a pure scan (no radio, no
///    reservation; `g_MapEditorMode` untouched on this call), so neither the
///    probe nor the overwritten queue has an observable effect. Stock
///    `RepairBay=GADEPT,NADEPT,CAOUTP` exists, but the branch outcome is dead
///    either way.
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
/// Step 2's cell search is VERA-internal in detail (the native range/flag
/// arguments of `Find_Nearby_Passable_Cell` are not modelled; the same
/// `find_nearby_passable_cell_with_index` helper the return staging uses
/// stands in), gamemd equivalent UNCHECKED beyond the seed cell.
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
    config: &MinerConfig,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    snap: &mut MinerSnapshot,
) -> bool {
    let human = sim
        .houses
        .get(&snap.owner)
        .is_none_or(|house| house.is_controlled_by_human(sim.session.game_mode_nonzero));
    if !human {
        snap.state = MinerState::SearchOre;
        handle_search_ore(sim, rules, config, path_grid, overlay_registry, snap);
        return false;
    }
    if let Some(grid) = path_grid
        && let Some(refinery_sid) = refinery_building_in_cell(sim, rules, (snap.rx, snap.ry))
        && let Some(exit) = building_nearby_passable_cell(sim, rules, refinery_sid, grid)
    {
        issue_move_if_idle(
            sim,
            Some(rules),
            grid,
            snap.entity_id,
            exit,
            snap.speed,
            overlay_registry,
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

/// `HouseClass::CountOwnedInstances > 0` for at least one entry of the
/// harvester type's `Dock=` list, as the Harvest preamble loop tests it.
///
/// Reads the store's O(1) per-(owner, type) count
/// (`EntityStore::count_owned_of_type`), the analogue of the native per-house
/// per-type counter array `CountOwnedInstances @ 0x0049FAE0` indexes. Native
/// counts an instance from Unlimbo until Limbo, so a refinery still under
/// construction and a dying one both count; the Rust count spans store
/// insert..remove and so agrees on both (the earlier `!in_limbo && !dying &&
/// health > 0` scan excluded the dying case — a mismatch, now removed). A
/// `Dock=` name the interner has never seen has no instances. A miner whose
/// type is unknown to the rules cannot evaluate the list and reads as "owns
/// one" (fixture tolerance; no production type lacks a rules entry).
fn house_owns_dock_instance(sim: &Simulation, rules: &RuleSet, snap: &MinerSnapshot) -> bool {
    let Some(harvester) = rules.object_case_insensitive(sim.interner.resolve(snap.type_id)) else {
        return true;
    };
    harvester.dock.iter().any(|dock_type| {
        sim.interner.get(dock_type).is_some_and(|type_ref| {
            sim.substrate
                .entities
                .count_owned_of_type(snap.owner, type_ref)
                > 0
        })
    })
}

/// `Look_up_building_in_cell` for the state-4 refinery test: the structure
/// occupying `cell` whose type is `Refinery=yes` (BuildingType `+0x16BB`).
/// Any owner qualifies, as native tests only the type flag.
fn refinery_building_in_cell(sim: &Simulation, rules: &RuleSet, cell: (u16, u16)) -> Option<u64> {
    let occupancy = sim.substrate.occupancy.get(cell.0, cell.1)?;
    occupancy.blockers(MovementLayer::Ground).find(|&sid| {
        sim.substrate.entities.get(sid).is_some_and(|entity| {
            entity.category == EntityCategory::Structure
                && !entity.dying
                && sim
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|obj| obj.refinery)
        })
    })
}

/// `FUN_00703590`: `Find_Nearby_Passable_Cell` seeded at the building's
/// `GetCoords` cell (`BuildingClass::GetCoords @ 0x00447AC0` = NW +
/// `((W-1)*128, (H-1)*128)` leptons, cell = coord >> 8).
fn building_nearby_passable_cell(
    sim: &Simulation,
    rules: &RuleSet,
    building_sid: u64,
    grid: &PathGrid,
) -> Option<(u16, u16)> {
    let building = sim.substrate.entities.get(building_sid)?;
    let (w, h) = sim
        .object_type(building.type_ref(), rules)
        .map(|obj| foundation_dimensions(&obj.foundation))
        .unwrap_or((1, 1));
    let (x, y) = building_get_coords_xy(building, w, h);
    super::miner_dock_sequence::find_nearby_passable_cell_with_index(
        (x >> 8) as i32,
        (y >> 8) as i32,
        grid,
        Some(&sim.substrate.occupancy),
        super::miner_dock_sequence::EXIT_SEARCH_MAX_RADIUS,
        u64::from(sim.session.binary_frame),
    )
}

fn handle_forced_return(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    snap: &mut MinerSnapshot,
) {
    if snap.miner.kind == MinerKind::War {
        handle_return_war(sim, rules, config, snap);
        return;
    }
    let has_teleport = sim
        .substrate
        .entities
        .get(snap.entity_id)
        .is_some_and(|e| e.teleport_state.is_some());
    if has_teleport {
        return;
    }

    if snap.miner.reserved_refinery.is_none() {
        if let Some(rsid) = select_return_refinery(sim, rules, config, snap) {
            snap.miner.reserved_refinery = Some(rsid);
            if try_issue_chrono_far_return_teleport(sim, rules, config, path_grid, snap, rsid) {
                return;
            }
        } else {
            // VERA-internal cursor: keep retrying the selection on the Rate
            // cadence, the way native state 2 does with no bay.
            return;
        }
    }

    handle_return(sim, rules, config, path_grid, overlay_registry, snap);
}

// -- Helpers --

/// Extract one bale from a tiberium cell: one density level of the overlay.
pub(crate) fn extract_bale(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
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

/// Test-only bulk-drain primitive over `CellClass::Reduce_Tiberium`.
///
/// This is NOT the harvester's per-gate request: `Harvest_Ore_Tick`
/// @ 0x0073D450 asks `Reduce_Tiberium` for `ftol(min(1.0, Storage - total))`,
/// i.e. one density level per gate (see `handle_harvest`). This helper only
/// exercises `Reduce_Tiberium`'s clamp-to-cell-content behaviour for an
/// arbitrary request, the way area damage or a test fixture might issue one.
///
/// One call drains `min(empty_capacity_bales, cell_density_levels)` bales
/// in a single atomic mutation: one overlay density update (or removal).
/// Returns an empty Vec when the cell holds no tiberium or
/// `empty_capacity_bales == 0`.
#[cfg(test)]
pub(crate) fn extract_bales_max(
    sim: &mut Simulation,
    rules: &RuleSet,
    cell: (u16, u16),
    config: &MinerConfig,
    empty_capacity_bales: u16,
) -> Vec<CargoBale> {
    if empty_capacity_bales == 0 {
        return Vec::new();
    }
    let outcome = sim.reduce_tiberium_at_with_native_context(
        cell,
        empty_capacity_bales,
        Some(rules),
        Some(crate::sim::tiberium::test_support::overlay_registry()),
    );
    let Some(resource_type) = outcome.resource_type else {
        return Vec::new();
    };
    let value = match resource_type {
        ResourceType::Ore => config.ore_bale_value,
        ResourceType::Gem => config.gem_bale_value,
    };
    (0..outcome.removed_amount)
        .map(|_| CargoBale {
            resource_type,
            value,
        })
        .collect()
}

/// Begin the return-to-refinery sequence.
///
/// Miners inside their kind's "too far" threshold (CMIN:
/// `ChronoHarvTooFarDistance=50`, HARV: `HarvesterTooFarDistance=5`) keep the
/// normal refinery radio/contact path to the accepted dock cell. Miners beyond
/// that threshold use the far-return destination: the `QueueingCell` passable-cell
/// search result, not the pad/contact cell. CMIN warps to the staging cell;
/// HARV drives to it.
fn begin_return(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    snap: &mut MinerSnapshot,
) {
    if let Some(rsid) = select_return_refinery(sim, rules, config, snap) {
        // A miner holds one refinery contact at a time. HELLOing a new
        // refinery over a live contact would evict the old one from the
        // miner's single slot only, leaving the old refinery's slot taken
        // until this miner died.
        if let Some(previous) = snap.miner.reserved_refinery.filter(|&sid| sid != rsid) {
            miner_dock::break_contact(sim, snap.entity_id, previous);
        }
        snap.miner.reserved_refinery = Some(rsid);
        if try_issue_chrono_far_return_teleport(sim, rules, config, path_grid, snap, rsid) {
            return;
        }
        if try_begin_close_return_radio(sim, rules, config, path_grid, overlay_registry, snap, rsid)
        {
            return;
        }
        snap.state = MinerState::ReturnToRefinery;
    } else {
        // No bay this dispatch: native state 2 sets nothing and retries on
        // the next Rate-epilogue dispatch. (A house with no dock instance at
        // all never gets here — the preamble queues Guard first.)
        snap.state = MinerState::ReturnToRefinery;
    }
}

fn try_begin_close_return_radio(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    snap: &mut MinerSnapshot,
    ref_sid: u64,
) -> bool {
    // `UnitClass::Mission_Harvest @ 0x0073E5E0` state 2, the close-return
    // radio (decompiled 2026-09-05, HARV branch `0x0073EBB1..0x0073EE68`,
    // CMIN branch `0x0073EDE3..0x0073EE4B`): with no NavCom, the narrow
    // `Find_Docking_Bay(Dock, 0, 0)` result within the kind's too-far
    // distance (`HarvesterTooFarDistance` Rules+0xD78 for HARV,
    // `ChronoHarvTooFarDistance` Rules+0xD7C for a `Teleporter=` type, both
    // x256 leptons against the 3-D `GetCoords` distance) gets
    // `Transmit_Radio(HELLO=2, bay)` on THAT dispatch (`0x0073EE51`); a reply
    // of 1 writes state 3, whose next dispatch is `Queue_Mission(Enter, 0);
    // return 1`. So a war miner hands off to Mission_Enter up to 5 cells out,
    // never on adjacency. Both kinds share the shape; only the threshold
    // differs.
    //
    // Chrono Miner legacy path; the War Miner runs `handle_return_war`.
    if snap.miner.kind != MinerKind::Chrono {
        return false;
    }
    let threshold = config.too_far_threshold_chrono;

    match return_exceeds_too_far_threshold(sim, rules, snap.entity_id, ref_sid, threshold) {
        Some(false) => {}
        Some(true) | None => return false,
    }

    let Some(dock_capacity) = refinery_dock_capacity_for_sid(sim, rules, ref_sid) else {
        return false;
    };

    let admission = miner_dock::hello(sim, snap.entity_id, ref_sid, dock_capacity);

    if admission == ContactAdmission::Accepted {
        if let Some(entity) = sim.substrate.entities.get_mut(snap.entity_id) {
            entity.movement_target = None;
        }
        snap.state = MinerState::Dock;
        snap.miner.dock_queued = false;
        // G5: the accepted close-return HELLO queues Mission_Enter via the
        // Harvest epilogue; arm the retry so the first CAN_DOCK waits the
        // ~14-16f cadence (and draws the RandomRanged(0,2) the dispatch
        // consumes), instead of an always-due next-tick collapse.
        super::miner_dock_sequence::schedule_enter_retry(sim, rules, snap);
        snap.miner.dock_phase = RefineryDockPhase::MissionEnter;
        return true;
    }

    // HELLO refused (`0x0073EE68` falls through): the wide pass
    // `Find_Docking_Bay(Dock, 0, 1)` under `g_MapEditorMode++`, and when its
    // bay is farther than 0x300 leptons (`CMP EAX,0x300; JG` @ 0x0073ECD0)
    // OR the type is a Teleporter, the staging destination = bay NW cell +
    // `QueueingCell` (`BuildingType+0x1618/+0x161C`) through
    // `Find_Nearby_Passable_Cell`; otherwise no destination — the state
    // simply re-runs on the next Rate dispatch.
    //
    // **VERA-internal, gamemd equivalent UNCHECKED — staging seed.** Rust
    // seeds the staging cell from the already-selected narrow-pass `ref_sid`
    // where native re-runs `Find_Docking_Bay(Dock, 0, 1)` (the WIDE pass,
    // no free-contact-slot gate) and seeds from THAT bay. The two differ
    // only when the house owns two refineries of the Dock type within the
    // `ChronoHarvTooFarDistance` close radius and the nearer one's slot is
    // taken: native stages beside the other refinery, Rust beside the
    // refused one. Player effect: a Chrono Miner waiting beside the wrong
    // refinery for one Rate dispatch; the next HELLO retry re-selects.
    // Chrono keeps its existing Dock/Approach re-HELLO cadence and staging
    // drive (adjacency guard is VERA-internal; native sets the destination
    // unconditionally for a Teleporter). The War Miner runs the native state
    // 2 in `handle_return_war`.
    if let Some(entity) = sim.substrate.entities.get_mut(snap.entity_id) {
        entity.movement_target = None;
    }
    snap.state = MinerState::Dock;
    snap.miner.dock_queued = true;
    snap.miner.dock_enter_retry.clear();
    snap.miner.dock_phase = RefineryDockPhase::Approach;
    if let Some(staging) = chrono_return_staging_cell_for_sid(sim, rules, ref_sid, path_grid)
        && !is_adjacent_or_at((snap.rx, snap.ry), staging)
        && let Some(grid) = path_grid
    {
        issue_move_if_idle(
            sim,
            Some(rules),
            grid,
            snap.entity_id,
            staging,
            snap.speed,
            overlay_registry,
        );
    }

    true
}

/// `0x0073ECD0`: a non-Teleporter miner whose refused-HELLO wide-pass bay is
/// within `0x300` leptons (3 cells) gets no staging destination.
const REFUSED_HELLO_STAGING_CELLS: u16 = 3;

fn try_issue_chrono_far_return_teleport(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    path_grid: Option<&PathGrid>,
    snap: &MinerSnapshot,
    ref_sid: u64,
) -> bool {
    if snap.miner.kind != MinerKind::Chrono {
        return false;
    }

    if !return_exceeds_too_far_threshold(
        sim,
        rules,
        snap.entity_id,
        ref_sid,
        config.too_far_threshold_chrono,
    )
    .unwrap_or(false)
    {
        return false;
    }

    let Some(staging) = chrono_return_staging_cell_for_sid(sim, rules, ref_sid, path_grid) else {
        return false;
    };

    let issued = movement::set_destination_for_teleporter_entity(
        &mut sim.substrate.entities,
        path_grid,
        snap.entity_id,
        staging,
        snap.speed,
        false,
        None,
        None,
        None,
        sim.zone_grid.as_ref(),
        None,
        &rules.general,
        true,
        true,
        false,
        sim.playfield_bounds,
        sim.session.binary_frame,
    );
    if issued {
        emit_chrono_warp_sounds(sim, rules, snap.type_id, (snap.rx, snap.ry), staging);
    }
    issued
}

fn emit_chrono_warp_sounds(
    sim: &mut Simulation,
    rules: &RuleSet,
    type_id: InternedId,
    depart: (u16, u16),
    arrive: (u16, u16),
) {
    let obj = rules.object_case_insensitive(sim.interner.resolve(type_id));
    let chrono_out = obj
        .and_then(|o| o.chrono_out_sound.clone())
        .or_else(|| rules.general.chrono_out_sound.clone());
    let chrono_in = obj
        .and_then(|o| o.chrono_in_sound.clone())
        .or_else(|| rules.general.chrono_in_sound.clone());
    if let Some(name) = chrono_out {
        let sound_id = sim.interner.intern(&name);
        sim.sound_events.push(SimSoundEvent::ChronoTeleport {
            sound_id,
            rx: depart.0,
            ry: depart.1,
        });
    }
    if let Some(name) = chrono_in {
        let sound_id = sim.interner.intern(&name);
        sim.sound_events.push(SimSoundEvent::ChronoTeleport {
            sound_id,
            rx: arrive.0,
            ry: arrive.1,
        });
    }
}

/// Refinery selection for a full miner: `UnitClass::Mission_Harvest @
/// 0x0073E5E0` state 2 (FINDING_HOME), decompiled 2026-09-05.
///
/// Native ordering: a *narrow* `Find_Docking_Bay(Type->Dock, 0, wide=0)` pass
/// runs first, and its result is used only when it lies within the kind's
/// too-far distance (`HarvesterTooFarDistance` @ Rules+0xD78 for HARV,
/// `ChronoHarvTooFarDistance` @ Rules+0xD7C when the unit type's Teleporter
/// byte +0xCD4 is set; both x256 leptons, compared against the 3-D
/// `GetCoords` distance, where the building side is `BuildingClass::GetCoords
/// @ 0x00447AC0` = foundation centre — see `return_exceeds_too_far_threshold`)
/// — the miner then radios HELLO(2) to it. Otherwise
/// (candidate too far, HELLO refused, or no candidate) a *wide* pass runs
/// bracketed by `g_MapEditorMode++ / --` (`0x00A8E7AC`) with `wide=1`, and
/// that result is the far-return destination. The wide pass admits
/// refineries whose contact slots are full. Native then gates the drive on
/// distance (0x0073ECD0: `CMP EAX,0x300; JG`): a non-Teleporter miner already
/// within 768 leptons of the wide-pass result gets NO destination and idles in
/// state 2 until a contact slot frees; Rust has no 0x300 gate and its dock FSM
/// parks the miner nearby instead (VERA-internal, gamemd equivalent UNCHECKED
/// beyond the compare; player-visible effect is the idle spot, small). There is no Teleporter branch
/// inside selection: chrono miners run the same two passes, only the
/// threshold differs (0x0073E5E0 case 2, `+0xCD4` selects Rules+0xD7C).
///
/// VERA-internal residuals: (1) the chosen refinery stays in
/// `reserved_refinery` until the return completes, where gamemd re-runs both
/// passes on every state-2 dispatch, so a miner beside an occupied refinery
/// does not re-pick a second free one inside the close radius (visible only
/// with two refineries within HarvesterTooFarDistance); (2) the narrow
/// candidate is returned before HELLO — gamemd falls through to the wide pass
/// when HELLO(2) is refused, VERA's later `hello_or_wait` re-probes instead.
fn select_return_refinery(
    sim: &Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    snap: &MinerSnapshot,
) -> Option<u64> {
    let threshold = match snap.miner.kind {
        MinerKind::Chrono => config.too_far_threshold_chrono,
        MinerKind::War | MinerKind::Slave => config.too_far_threshold_standard,
    };
    if let Some(sid) = find_docking_bay(sim, rules, snap, false)
        && return_exceeds_too_far_threshold(sim, rules, snap.entity_id, sid, threshold)
            == Some(false)
    {
        return Some(sid);
    }
    find_docking_bay(sim, rules, snap, true)
}

/// One `FootClass::Find_Docking_Bay @ 0x004DF040` pass: for each `Dock=`
/// type in list order, `FUN_004DEE80` scans the miner's OWN house's
/// building list (`Owner @ TechnoClass+0x21C` → vector at House+0x6C, count
/// at +0x78). Allies are never candidates, so a miner never deposits into an
/// ally's wallet. Rust's `ids_for_owner` is ascending stable_id = creation
/// order, which matches the native vector order for buildings that never
/// changed hands (`DynamicVectorClass::Remove` shifts later entries down and
/// preserves relative order). Tie-only residual: a captured building is
/// natively removed from the old owner's list and appended to the new
/// owner's, so it sorts LAST among that house's refineries, while Rust keeps
/// it at its creation id. Order only decides equal-distance ties (strict `<`
/// below), so this is visible only when a captured refinery and an original
/// one sit at exactly the same centre distance; list-append semantics are
/// deliberately not modelled.
///
/// Per-candidate gates in native order:
/// - non-null and `+0x81 == 0` (`ObjectClass::InLimbo`), type == Dock type
///   (`+0x520`); the whole scan returns null when the house owns no instance
///   of that type (`HouseClass::CountOwnedInstances` at 0x004DEEA5);
/// - narrow pass only (`wide != 1`, 0x004DEF02): `FUN_0065ADF0` on the
///   building with the miner as argument (0x004DEF09) — a free `Contacts[]`
///   slot (`+0xE4`, count `+0xE8`) or the miner already tracked. `+0xE8` is
///   1 from the `RadioClass` ctor (0x0065A764) and then set by
///   `BuildingClass::Constructor` 0x0043BCBD..0x0043BCD0 to
///   `max([Type+0x1780] NumberOfDocks, 1)` via `Set_Contact_Count`; Rust
///   derives the slot capacity the same way (stock refineries are
///   `NumberOfDocks=1`);
/// - `MapClass::Can_Reach_Zone` from the miner's cell to the building's
///   `GetCoords` cell (skipped when `WhatAmI() == Aircraft(2)`, never a
///   miner) — see `refinery_zone_reachable`;
/// - `Receive_Radio(0xF)` must return 1 — see `refinery_accepts_can_load`;
/// - distance `FUN_005F6500`: `dx² + dy²` in leptons between both objects'
///   `GetCoords` (Z ignored); replace when `best == -1 || d < best` (strict,
///   so ties keep the earlier Dock type / earlier-created building) or when
///   the candidate is the primary factory (`TechnoClass+0x3D3`). VERA has no
///   primary designation for refineries, so that override is absent here.
/// [`find_docking_bay`] for `miner` as its state-2 dispatch calls it.
#[cfg(test)]
pub(super) fn find_docking_bay_for_test(
    sim: &Simulation,
    rules: &RuleSet,
    miner: u64,
    wide: bool,
) -> Option<u64> {
    let snap = build_miner_snapshot(sim, rules, miner)?;
    find_docking_bay(sim, rules, &snap, wide)
}

fn find_docking_bay(
    sim: &Simulation,
    rules: &RuleSet,
    snap: &MinerSnapshot,
    wide: bool,
) -> Option<u64> {
    let miner = sim.substrate.entities.get(snap.entity_id)?;
    let harvester = rules.object_case_insensitive(sim.interner.resolve(snap.type_id))?;
    let miner_x = i64::from(miner.position.rx) * 256 + miner.position.sub_x.to_num::<i64>();
    let miner_y = i64::from(miner.position.ry) * 256 + miner.position.sub_y.to_num::<i64>();
    let unit_mz = miner
        .locomotor
        .as_ref()
        .map(|loc| loc.movement_zone)
        .unwrap_or(MovementZone::Normal);

    let mut best: Option<(i64, u64)> = None;
    for dock_type in &harvester.dock {
        for &sid in sim.substrate.entities.ids_for_owner(snap.owner) {
            let Some(entity) = sim.substrate.entities.get(sid) else {
                continue;
            };
            if entity.category != EntityCategory::Structure {
                continue;
            }
            let e_type = sim.interner.resolve(entity.type_ref());
            if !e_type.eq_ignore_ascii_case(dock_type) || entity.lifecycle.in_limbo {
                continue;
            }
            // gamemd removes a dead building from the house list through
            // Limbo; VERA keeps the entity through its death animation.
            if entity.dying || entity.health.current == 0 {
                continue;
            }
            let Some(obj) = rules.object_case_insensitive(e_type) else {
                continue;
            };
            let capacity = obj.dock_contact_capacity() as usize;
            if !wide && !miner_dock::would_admit(sim, sid, snap.entity_id, capacity) {
                continue;
            }
            let (w, h) = foundation_dimensions(&obj.foundation);
            let dock = refinery_dock_cell(entity.position.rx, entity.position.ry);
            if !refinery_zone_reachable(sim, miner, unit_mz, (snap.rx, snap.ry), dock) {
                continue;
            }
            if !refinery_accepts_can_load(
                sim,
                harvester,
                unit_mz,
                entity,
                obj,
                snap.entity_id,
                capacity,
                wide,
            ) {
                continue;
            }
            // `BuildingClass::GetCoords @ 0x00447AC0`: foundation centre, the
            // same point the state-2 too-far test measures to.
            let (centre_x, centre_y) = building_get_coords_xy(entity, w, h);
            let dx = miner_x - centre_x;
            let dy = miner_y - centre_y;
            let dist_sq = dx * dx + dy * dy;
            match best {
                Some((d, _)) if dist_sq >= d => {}
                _ => best = Some((dist_sq, sid)),
            }
        }
    }
    best.map(|(_, sid)| sid)
}

/// `BuildingClass::Receive_Radio @ 0x0043C2D0` case 0xF (CAN_LOAD) as seen by
/// a `Harvester=yes` unit probing a `Refinery=yes` building (disassembly
/// 0x0043C2F8..0x0043C6EF, 2026-09-05). Returns true for native result 1.
///
/// - `HouseClass::Is_Ally` on the building owner → 0 (always passes here:
///   the scanner only offers own-house buildings);
/// - current mission Construction (0x12) or Selling (0x13) → 10;
/// - `+0x534 == 0` → 10. `+0x534` is the current BState, written by
///   `BuildingClass::GrandOpening @ 0x00447780` (`+0x538` is the queued one);
///   0 = BSTATE_CONSTRUCTION. Rust: `building_up` (construction) and
///   `building_down` (sell/deconstruct) cover both mission and BState gates;
/// - narrow pass (`g_MapEditorMode == 0`, 0x0043C35A): no free/own contact
///   slot (`FUN_0065ADF0`) → 10 unless the type is `UnitAbsorb=`/
///   `InfantryAbsorb=` (+0x16AE/+0x16AF, never a stock refinery). Same probe
///   the scanner already applied;
/// - unit `MovementZone != Amphibious(5)` and `Naval=` (TechnoType+0xCCE)
///   differs between unit and building → 10;
/// - unit `BalloonHover=` (TechnoType+0xD6A, `TechnoTypeClass::ReadINI`
///   0x00714DA9) → 10;
/// - `+0x660 == 0` → 10 (0x0043C422). Writers (instruction scan
///   `mov [..+0x660]`): `BuildingClass` ctor 0x0043B882 = 1, `ReadFromINI`
///   0x0044FC49 = 1, `GoOnline @ 0x00452260` = 1, `GoOffline @ 0x00452360`
///   = 0 (callers: `EventClass::Execute` 0x004C6D9A power toggle,
///   `TriggerAction::Execute` 0x006DDFB9, `ReadFromINI` 0x0044FD23), plus
///   0x004521C0 = 0 / 0x00452210 = 1 called only from `TemporalClass`
///   InitiateWarp and LetGo (now labelled TemporalGoOffline/Online). So it
///   is the player/trigger TogglePower latch plus a temporal-warp clear, not
///   house low power. VERA represents the warp's half
///   ([`crate::sim::game_entity::GameEntity::building_online`]); stock refineries are not toggleable;
/// - 0x0043C43B..0x0043C453: unless the type is `UnitAbsorb=`/`InfantryAbsorb=`
///   (+0x16AE/+0x16AF) the `JZ 0x0043C4F8` at 0x0043C453 jumps straight past
///   the absorber-only block, so for a refinery NEITHER the `CaptureManager`
///   test (`+0x2BC` → `FUN_004722C0`, 0x0043C4A0) NOR the passenger-count /
///   `SizeLimit=` block (0x0043C4C2: `[+0x114]+1 > Type+0x5E0`, `Size` vs
///   +0x388) is reached. They are not gates on this path and Rust models
///   neither;
/// - 0x0043C4F8..0x0043C64F, tested before the Refinery branch: +0x16AD → 1,
///   +0x16AB (`+0x0070FB50` unit probe then radio 0x23), +0x16A9 (WhatAmI 1/2
///   then radio 0x23), +0x16C2/+0x16C1 (only for `WhatAmI() == 0xF`), +0x16CB
///   (interface query through `[unit+0x4]`). All six bytes are zero for a
///   stock refinery type, so
///   control falls through to the Refinery test;
/// - `DockUnload=yes` (BuildingType+0x16B3; `Refinery=` is +0x16BB) and the unit is a UnitClass with
///   `Harvester=yes` (UnitType+0xE0E): return 1 when `g_MapEditorMode != 0`
///   (wide pass, 0x0043C675) or `+0x118 == 0` (0x0043C682). `+0x118` is
///   `PassengersClass::FirstPassenger` (`PassengersClass` at +0x114: case
///   0xE compares `[+0x114] + 1` against Type+0x5E0; `CargoClass::AddPassenger
///   @ 0x004733A0` is always entered via `LEA ECX,[this+0x114]`;
///   `TechnoClass` ctor zeroes +0x118 at 0x006F2B8D). None of AddPassenger's
///   15 call sites is a BuildingClass refinery path or the harvester unload
///   FSM (`UnitClass::Mission_Unload @ 0x0073D630`; its 0x0073DC78 call
///   re-adds a popped passenger to the unit's own cargo), so a stock
///   refinery's +0x118 stays 0 and this "bay" gate is INERT for stock play:
///   the narrow-pass occupancy gate is the `Contacts[]` probe alone. Rust
///   therefore models no bay-occupancy gate here;
/// - otherwise 0.
#[allow(clippy::too_many_arguments)]
fn refinery_accepts_can_load(
    sim: &Simulation,
    harvester: &crate::rules::object_type::ObjectType,
    unit_mz: MovementZone,
    refinery: &crate::sim::game_entity::GameEntity,
    refinery_type: &crate::rules::object_type::ObjectType,
    miner_sid: u64,
    capacity: usize,
    wide: bool,
) -> bool {
    if refinery.building_up.is_some() || refinery.building_down.is_some() {
        return false;
    }
    if !wide && !miner_dock::would_admit(sim, refinery.stable_id(), miner_sid, capacity) {
        return false;
    }
    if unit_mz != MovementZone::Amphibious && harvester.naval != refinery_type.naval {
        return false;
    }
    if harvester.balloon_hover {
        return false;
    }
    if !refinery.building_online() {
        return false;
    }
    refinery_type.dock_unload && harvester.harvester
}

/// `MapClass::Can_Reach_Zone` gate of the scanner `FUN_004DEE80`, called
/// natively with the unit type's MovementZone, the miner's cell and the
/// building's `GetCoords` cell. VERA's zone map marks building footprints
/// `ZONE_INVALID`, so the probe targets the refinery's dock cell and its 8
/// neighbours instead of the foundation centre — VERA-internal
/// approximation, gamemd equivalent UNCHECKED for the exact probed cell.
/// Without a zone grid or a valid miner anchor the gate is skipped, as the
/// ore-scan filter does.
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
        || ore_reachable(zone_grid, mz, layer, anchor, dock)
}

/// Resolve a refinery's dock cell from its stable_id.
fn refinery_dock_for_sid(sim: &Simulation, ref_sid: u64) -> Option<(u16, u16)> {
    let entity = sim.substrate.entities.get(ref_sid)?;
    if entity.dying || entity.health.current == 0 {
        return None;
    }
    Some(refinery_dock_cell(entity.position.rx, entity.position.ry))
}

fn refinery_dock_capacity_for_sid(
    sim: &Simulation,
    rules: &RuleSet,
    ref_sid: u64,
) -> Option<usize> {
    let entity = sim.substrate.entities.get(ref_sid)?;
    if entity.dying || entity.health.current == 0 {
        return None;
    }
    sim.object_type(entity.type_ref(), rules)
        .map(|o| o.dock_contact_capacity() as usize)
        .or(Some(1))
}

/// Chrono far-return staging cell from `QueueingCell`, then the same nearby
/// passable-cell search gamemd runs before assigning a teleport destination.
fn chrono_return_staging_cell_for_sid(
    sim: &Simulation,
    rules: &RuleSet,
    ref_sid: u64,
    path_grid: Option<&PathGrid>,
) -> Option<(u16, u16)> {
    let entity = sim.substrate.entities.get(ref_sid)?;
    let queueing = sim
        .object_type(entity.type_ref(), rules)
        .map_or([0, 0], |o| o.queueing_cell);
    let seed = super::miner_dock_sequence::refinery_queue_cell(
        entity.position.rx,
        entity.position.ry,
        queueing,
    );

    if let Some(grid) = path_grid {
        return super::miner_dock_sequence::find_nearby_passable_cell_with_index(
            seed.0 as i32,
            seed.1 as i32,
            grid,
            None,
            super::miner_dock_sequence::EXIT_SEARCH_MAX_RADIUS,
            u64::from(sim.session.binary_frame),
        );
    }

    Some(seed)
}

/// The dock pad a refinery at NW `(rx, ry)` sends its miner to
/// ([`crate::sim::radio::receive::dock_pad_cell`]).
pub(crate) fn refinery_dock_cell(rx: u16, ry: u16) -> (u16, u16) {
    crate::sim::radio::receive::dock_pad_cell(rx, ry)
}

/// 8-neighbor offsets in clockwise order starting from north. Used by the
/// effective-zone-cell probe and the ore-reachability check.
const ADJACENT_8: [(i32, i32); 8] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];

/// Return a cell whose zone serves as the harvester's reachability anchor.
///
/// The harvester's own cell may be on Tiberium (impassable in the path grid,
/// hence `ZONE_INVALID`); when so, probe its 8 neighbors and return the
/// first cell with a valid zone. Returns `None` if neither the harvester's
/// cell nor any neighbor has a valid zone — caller falls back to no-filter
/// behavior for that tick.
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
    for &(dx, dy) in &ADJACENT_8 {
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

/// True if any 8-neighbor of `ore_cell` is in the harvester's connected zone
/// component. Ore cells themselves are `ZONE_INVALID` because Tiberium is
/// blocked in the path grid (so A* doesn't path through ore fields), so we
/// probe the ore's neighbors instead — mirroring how a harvester actually
/// approaches an ore patch.
fn ore_reachable(
    zone_grid: &ZoneGrid,
    mz: MovementZone,
    layer: MovementLayer,
    harvester_zone_cell: (u16, u16),
    ore_cell: (u16, u16),
) -> bool {
    for &(dx, dy) in &ADJACENT_8 {
        let nx = (ore_cell.0 as i32) + dx;
        let ny = (ore_cell.1 as i32) + dy;
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

fn native_tiberium_context<'a>(
    sim: &'a Simulation,
    rules: &'a RuleSet,
    overlay_registry: Option<&'a crate::map::overlay_types::OverlayTypeRegistry>,
) -> Option<(
    &'a crate::sim::overlay_grid::OverlayGrid,
    &'a crate::map::overlay_types::OverlayTypeRegistry,
    &'a crate::rules::tiberium_type::TiberiumTypeRegistry,
)> {
    let grid = sim.overlay_grid.as_ref()?;
    let registry = overlay_registry?;
    (!rules.tiberium_types.is_empty()).then_some((grid, registry, &rules.tiberium_types))
}

pub(crate) fn resource_cell_present(
    sim: &Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    cell: (u16, u16),
) -> bool {
    if let Some((grid, registry, types)) = native_tiberium_context(sim, rules, overlay_registry) {
        return crate::sim::tiberium::tiberium_cell_view(grid, registry, types, cell).is_some();
    }
    false
}

pub(crate) fn search_local_resource(
    sim: &Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    center: (u16, u16),
    radius: u16,
    filter: Option<&dyn Fn((u16, u16)) -> bool>,
) -> Option<(u16, u16)> {
    let (grid, registry, types) = native_tiberium_context(sim, rules, overlay_registry)?;
    search_local_tiberium(grid, registry, types, center, radius, filter)
}

fn search_local_tiberium(
    grid: &crate::sim::overlay_grid::OverlayGrid,
    registry: &crate::map::overlay_types::OverlayTypeRegistry,
    types: &crate::rules::tiberium_type::TiberiumTypeRegistry,
    center: (u16, u16),
    radius: u16,
    filter: Option<&dyn Fn((u16, u16)) -> bool>,
) -> Option<(u16, u16)> {
    if crate::sim::tiberium::tiberium_cell_view(grid, registry, types, center).is_some() {
        return Some(center);
    }
    let cx = i32::from(center.0);
    let cy = i32::from(center.1);
    for ring in 1..i32::from(radius) {
        let mut best_in_ring: Option<(i32, (u16, u16))> = None;
        for col in -ring..=ring {
            for (nx, ny) in [
                (cx + col, cy - ring),
                (cx + col, cy + ring),
                (cx - ring, cy + col),
                (cx + ring, cy + col),
            ] {
                if nx < 0 || ny < 0 || nx > i32::from(u16::MAX) || ny > i32::from(u16::MAX) {
                    continue;
                }
                let cell = (nx as u16, ny as u16);
                if filter.is_some_and(|candidate_filter| !candidate_filter(cell)) {
                    continue;
                }
                let Some(view) =
                    crate::sim::tiberium::tiberium_cell_view(grid, registry, types, cell)
                else {
                    continue;
                };
                if best_in_ring.is_none_or(|(value, _)| view.nominal_value > value) {
                    best_in_ring = Some((view.nominal_value, cell));
                }
            }
        }
        if let Some((_, cell)) = best_in_ring {
            return Some(cell);
        }
    }
    None
}

/// Hand a selected stock-miner destination to the normal Drive command authority.
#[cfg(test)]
pub(crate) fn issue_stock_miner_drive_move(
    sim: &mut Simulation,
    rules: &RuleSet,
    grid: &PathGrid,
    entity_id: u64,
    target: (u16, u16),
) -> bool {
    issue_stock_miner_drive_move_with_overlay_registry(sim, rules, grid, entity_id, target, None)
}

fn issue_stock_miner_drive_move_with_overlay_registry(
    sim: &mut Simulation,
    rules: &RuleSet,
    grid: &PathGrid,
    entity_id: u64,
    target: (u16, u16),
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> bool {
    if target.0 >= grid.width() || target.1 >= grid.height() {
        return false;
    }
    let Some(info) = sim.resolve_move_info(entity_id, Some(rules)) else {
        return false;
    };

    // Unit741970's class refusals return before its Teleporter swap
    // (0x7423CD), and the Drive setter behind the swap accepts without a
    // route, so an activated Drive is never rolled back: a later Process
    // FindPath failure clears the destination and the idle restore gate
    // returns the Chrono Miner to Teleport.
    if info.is_teleporter && info.is_harvester {
        let Some(entity) = sim.substrate.entities.get_mut(entity_id) else {
            return false;
        };
        if !movement::can_accept_destination(entity) {
            return false;
        }
        movement::locomotor_owner::begin_drive_for_teleporter(entity, sim.session.binary_frame);
    }

    let terrain_costs = sim.terrain_costs.get(&info.speed_type);
    let blocker_neighbor_counts = movement::bump_crush::build_blocker_neighbor_counts_with_overlays(
        &sim.substrate.entities,
        grid.width(),
        grid.height(),
        sim.resolved_terrain.as_ref(),
        sim.overlay_grid.as_ref(),
        overlay_registry,
        &sim.interner,
        Some(rules),
    );
    let issued = movement::issue_move_command_with_layered(
        &mut sim.substrate.entities,
        grid,
        entity_id,
        target,
        info.speed,
        false,
        terrain_costs,
        None,
        sim.resolved_terrain.as_ref(),
        sim.zone_grid.as_ref(),
        None,
        Some(&blocker_neighbor_counts),
        sim.playfield_bounds,
        Some(&mut sim.substrate.cell_occupation),
        crate::sim::movement::DestinationTiming::from_rules(sim.session.binary_frame, rules.into()),
    );
    if !issued {
        return false;
    }

    if let Some(movement) = sim
        .substrate
        .entities
        .get_mut(entity_id)
        .and_then(|entity| entity.movement_target.as_mut())
    {
        movement.accel_factor = info.accel_factor;
        movement.decel_factor = info.decel_factor;
        movement.slowdown_distance = info.slowdown_distance;
    }
    true
}

/// Issue a move command only if the entity's retained destination is not
/// already this cell.
///
/// The destination is the NavCom the setter publishes: a Drive/Ship order
/// accepts without a route (the first Process requests it) and keeps NavCom
/// through the Foot+64C retry ladder, so the gate must not read the route.
/// A Find_Path redirect (code 6 FNPC, code 7) or the command-time redirect
/// of the remaining adapter locomotors publishes a different cell, and the
/// next call re-issues the original target (VERA-internal dock approach;
/// native Mission_Harvest's drive is not ported).
pub(crate) fn issue_move_if_idle(
    sim: &mut Simulation,
    rules: Option<&RuleSet>,
    grid: &PathGrid,
    entity_id: u64,
    target: (u16, u16),
    speed: SimFixed,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) {
    if target.0 >= grid.width() || target.1 >= grid.height() {
        return;
    }
    let already = sim.substrate.entities.get(entity_id).is_some_and(|e| {
        e.navigation.nav_com
            == Some(crate::sim::components::NavTargetRef::cell(
                target.0, target.1,
            ))
    });
    if !already {
        let blocker_neighbor_counts =
            movement::bump_crush::build_blocker_neighbor_counts_with_overlays(
                &sim.substrate.entities,
                grid.width(),
                grid.height(),
                sim.resolved_terrain.as_ref(),
                sim.overlay_grid.as_ref(),
                overlay_registry,
                &sim.interner,
                rules,
            );
        let _ = movement::issue_move_command_with_layered(
            &mut sim.substrate.entities,
            grid,
            entity_id,
            target,
            speed,
            false,
            None,
            None,
            sim.resolved_terrain.as_ref(),
            sim.zone_grid.as_ref(),
            None,
            Some(&blocker_neighbor_counts),
            sim.playfield_bounds,
            Some(&mut sim.substrate.cell_occupation),
            crate::sim::movement::DestinationTiming::from_rules(
                sim.session.binary_frame,
                rules.into(),
            ),
        );
    }
}

/// True if `pos` is at `target` or cardinally/diagonally adjacent (1 cell away).
/// Used for dock arrival checks — buildings occupy their cells, so miners
/// park adjacent to the refinery rather than on top of it.
fn is_adjacent_or_at(pos: (u16, u16), target: (u16, u16)) -> bool {
    let dx = (pos.0 as i32 - target.0 as i32).unsigned_abs();
    let dy = (pos.1 as i32 - target.1 as i32).unsigned_abs();
    dx <= 1 && dy <= 1
}

/// Movement can legitimately stop short when blocked but within
/// `[General] CloseEnough`; refinery return must treat that as contact so the
/// dock radio/enter sequence can take over instead of reissuing the same path.
fn is_within_close_enough(pos: (u16, u16), target: (u16, u16), close_enough: i32) -> bool {
    // Same metric the movement give-up test uses: `CoordStruct::Distance3D` @
    // `0x0041C380` against `Rules+0x1718`. A Manhattan sum here disagreed with
    // movement by up to √2 at exactly the Δ(2,1) geometry where the two now
    // both abort, so movement stopped while this said "not close enough" and the
    // return loop reissued the same path.
    let dx = (pos.0 as i64 - target.0 as i64).abs() * 256;
    let dy = (pos.1 as i64 - target.1 as i64).abs() * 256;
    crate::util::fixed_math::isqrt_i64(dx * dx + dy * dy) < i64::from(close_enough)
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
/// alive, on-map `OrePurifier=` structure. Shared by
/// [`count_purifiers_for_owner`] and the per-tick economy shadow
/// (`refresh_economy_shadow`) so both count the same buildings.
pub(crate) fn counts_as_purifier(
    sim: &Simulation,
    rules: &RuleSet,
    e: &crate::sim::game_entity::GameEntity,
) -> bool {
    // A Dying purifier corpse (sold/destroyed this tick) must not keep paying
    // its deposit bonus until the end-of-tick drain; a limbo'd one already
    // took the native Limbo `DEC`.
    !e.dying
        && !e.lifecycle.in_limbo
        && e.building_up.is_none()
        && e.category == EntityCategory::Structure
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
    // Apply the AI virtual bonus only when a HouseState explicitly says
    // the refinery's owner is non-human. Real games seed every house
    // through app init with the correct flag; tests/edge cases that fall
    // through to the credits_entry_for_owner auto-create get is_human=true
    // (the safer default) and therefore skip the AI bonus, as intended.
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
    use crate::map::overlay_types::OverlayTypeRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::mission::MissionType;

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
        rules.merge_art_data(&crate::rules::art_data::ArtRegistry::from_ini(
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
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
        );
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
            Health { current: 900 },
            type_ref,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        ge.lifecycle.in_limbo = false;
        sim.substrate.entities.insert(ge);
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
            Health { current: 600 },
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        ge.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Drive,
            ),
        );
        ge.drive_locomotion = Some(Default::default());
        ge.miner = Some(Miner::new(MinerKind::War, &MinerConfig::default(), 0));
        ge.mission.set_handler_state(MinerState::SearchOre.cursor());
        sim.substrate.entities.insert(ge);
        sim.substrate.next_stable_object_id = MINER_ID + 1;
    }

    fn seed_ore(sim: &mut Simulation, cell: (u16, u16)) {
        crate::sim::tiberium::test_support::place_stock_amount(sim, cell, ResourceType::Ore, 720);
    }

    fn ore_authority_rules() -> (RuleSet, OverlayTypeRegistry, u8) {
        let mut text = String::from(
            "[Tiberiums]\n0=Riparius\n[Riparius]\nImage=1\nValue=25\n[OverlayTypes]\n",
        );
        for slot in 0..=102 {
            if slot == 102 {
                text.push_str("102=TIB01\n");
            } else {
                text.push_str(&format!("{slot}=FILL{slot:03}\n"));
            }
        }
        text.push_str("[TIB01]\nTiberium=yes\n");
        let ini = IniFile::from_str(&text);
        let rules = RuleSet::from_ini(&ini).expect("ore authority rules");
        let registry = OverlayTypeRegistry::from_ini(&ini, None);
        let tib01 = registry.id_for_name("TIB01").expect("TIB01 slot");
        (rules, registry, tib01)
    }

    #[test]
    fn miner_queries_fail_closed_without_the_overlay_registry() {
        let (rules, registry, tib01) = ore_authority_rules();
        let config = MinerConfig::from_rules(&rules);
        let mut sim = Simulation::new();
        let mut overlay = crate::sim::overlay_grid::OverlayGrid::new(8, 8);
        overlay.place_overlay(4, 4, tib01, 0);
        overlay.take_dirty_cells();
        sim.overlay_grid = Some(overlay);

        assert!(!resource_cell_present(&sim, &rules, None, (4, 4)));
        assert_eq!(
            search_local_resource(&sim, &rules, None, (2, 2), 8, None),
            None,
            "without the overlay registry no cell can be classified as tiberium"
        );
        assert!(resource_cell_present(&sim, &rules, Some(&registry), (4, 4)));
        assert_eq!(
            search_local_resource(&sim, &rules, Some(&registry), (2, 2), 8, None,),
            Some((4, 4)),
        );
    }

    #[test]
    fn productive_scan_sets_the_destination_and_draws_the_epilogue_in_one_dispatch() {
        let rules = scan_rules();
        let config = MinerConfig::from_rules(&rules);
        let grid = PathGrid::new(64, 64);
        let mut sim = Simulation::new();
        spawn_search_miner(&mut sim, (10, 10));
        seed_ore(&mut sim, (10, 14));

        let scenario_before = sim.rng_state().scenario;
        tick_miners(&mut sim, &rules, &config, Some(&grid));

        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(
            entity.miner.as_ref().expect("miner").target_ore_cell,
            Some((10, 14))
        );
        assert!(
            entity.movement_target.is_some(),
            "gamemd sets the destination inside the scan dispatch"
        );
        assert_ne!(
            sim.rng_state().scenario,
            scenario_before,
            "the Rate epilogue's RandomRanged(0, 2) belongs to the scan dispatch"
        );
        let base = rules.mission_control.rate_frames(MissionType::Harvest) as i32;
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
        spawn_search_miner(&mut sim, (10, 10));
        seed_ore(&mut sim, (10, 10));

        let scenario_before = sim.rng_state().scenario;
        tick_miners(&mut sim, &rules, &config, Some(&grid));

        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert!(entity.movement_target.is_none(), "nothing to drive to");
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
        spawn_search_miner(&mut sim, (10, 10));
        // Well outside TiberiumLongScan — the only ore on the map, and gamemd's
        // bounded scan can never reach it.
        sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(512, 512));
        seed_ore(&mut sim, (400, 400));
        assert!(config.long_scan_radius < 300);

        let scenario_before = sim.rng_state().scenario;
        tick_miners(&mut sim, &rules, &config, Some(&grid));

        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.miner_state(), Some(MinerState::WaitNoOre));
        assert_eq!(
            entity.miner.as_ref().expect("miner").target_ore_cell,
            None,
            "no whole-map fallback target"
        );
        assert!(entity.movement_target.is_none(), "no cross-map drive");
        assert_eq!(
            entity.mission.dispatch_timer().delay(),
            i32::from(config.rescan_cooldown_ticks),
        );
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
        spawn_search_miner(&mut sim, (10, 10));
        let owner = sim.interner.intern("Americans");
        assert!(!sim.houses[&owner].harvester_no_ore);

        tick_miners(&mut sim, &rules, &config, Some(&grid));
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
        tick_miners(&mut sim, &rules, &config, Some(&grid));

        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.mission.queued().known(), Some(MissionType::Guard));
        assert_eq!(
            entity.miner_state(),
            Some(MinerState::WaitNoOre),
            "state 4 re-runs until Guard commences; no return to the scan"
        );
        assert_eq!(entity.miner.as_ref().expect("miner").target_ore_cell, None);
        assert!(
            entity.movement_target.is_none(),
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
        tick_miners(&mut sim, &rules, &config, Some(&grid));
        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.mission.current().known(), Some(MissionType::Guard));
        assert_eq!(entity.miner.as_ref().expect("miner").target_ore_cell, None);
        assert!(entity.movement_target.is_none());
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
        spawn_search_miner_without_refinery(&mut sim, (10, 10));
        spawn_owned_refinery(&mut sim, REFINERY_NW);
        let owner = sim.interner.intern("Americans");
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, false, 0, 10),
        );

        tick_miners(&mut sim, &rules, &config, Some(&grid));
        {
            let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
            assert_eq!(entity.miner_state(), Some(MinerState::WaitNoOre));
            assert_eq!(entity.mission.dispatch_timer().delay(), 105);
        }
        assert!(sim.houses[&owner].harvester_no_ore);

        // Second miss: back through the scan, another 105-frame wait, no
        // Guard queue.
        sim.session.binary_frame += 105;
        tick_miners(&mut sim, &rules, &config, Some(&grid));
        {
            let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
            assert_eq!(entity.mission.queued().known(), None, "no Guard park");
            assert_eq!(entity.miner_state(), Some(MinerState::WaitNoOre));
            assert_eq!(entity.mission.dispatch_timer().delay(), 105);
        }

        // Ore appears: the next re-scan finds it.
        seed_ore(&mut sim, (10, 14));
        sim.session.binary_frame += 105;
        tick_miners(&mut sim, &rules, &config, Some(&grid));
        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.mission.queued().known(), None);
        assert_eq!(entity.miner_state(), Some(MinerState::MoveToOre));
        assert_eq!(
            entity.miner.as_ref().expect("miner").target_ore_cell,
            Some((10, 14))
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
        spawn_search_miner_without_refinery(&mut sim, (10, 10));
        register_house(&mut sim);
        seed_ore(&mut sim, (10, 14));

        let scenario_before = sim.rng_state().scenario;
        tick_miners(&mut sim, &rules, &config, Some(&grid));

        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.mission.queued().known(), Some(MissionType::Guard));
        assert_eq!(entity.mission.dispatch_timer().delay(), DISPATCH_NEXT_FRAME);
        assert_eq!(
            entity.miner.as_ref().expect("miner").target_ore_cell,
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
        // Standing on the stock pad cell (NW + (3, 1)) inside the footprint.
        let pad = (REFINERY_NW.0 + 3, REFINERY_NW.1 + 1);
        spawn_search_miner(&mut sim, pad);

        tick_miners(&mut sim, &rules, &config, Some(&grid));
        sim.session.binary_frame += 105;
        tick_miners(&mut sim, &rules, &config, Some(&grid));

        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.mission.queued().known(), Some(MissionType::Guard));
        let goal = entity
            .movement_target
            .as_ref()
            .and_then(|m| m.final_goal.or_else(|| m.path.last().copied()))
            .expect("exit destination set from the refinery cell");
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
    /// (`Queue_Mission(mission, 0)` @ 0x004C73B9 natively; VERA assigns
    /// Harvest + the MoveToOre cursor), after which the Harvest dispatch gate
    /// re-engages.
    #[test]
    fn player_harvest_order_returns_a_parked_war_miner_to_work() {
        let rules = scan_rules();
        let config = MinerConfig::from_rules(&rules);
        let grid = PathGrid::new(64, 64);
        let mut sim = Simulation::new();
        spawn_search_miner(&mut sim, (10, 10));

        tick_miners(&mut sim, &rules, &config, Some(&grid));
        sim.session.binary_frame += 105;
        tick_miners(&mut sim, &rules, &config, Some(&grid));
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
            Some(&grid),
            &std::collections::BTreeMap::new(),
        );
        assert!(applied);
        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.mission.current().known(), Some(MissionType::Harvest));
        assert_eq!(entity.miner_state(), Some(MinerState::MoveToOre));

        // The Harvest handler dispatches again and drives to the ordered cell.
        sim.session.binary_frame += 1;
        tick_miners(&mut sim, &rules, &config, Some(&grid));
        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.mission.current().known(), Some(MissionType::Harvest));
        assert_eq!(
            entity
                .movement_target
                .as_ref()
                .and_then(|m| m.final_goal.or_else(|| m.path.last().copied())),
            Some((10, 14)),
            "the dispatch gate re-engaged and the MoveToOre cursor drove to the order"
        );
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
        // Refinery centre = (3072, 2944) leptons; cell (16, 11) is 1152
        // leptons (4.5 cells) east of it.
        spawn_returning_war_miner(&mut sim, (16, 11));

        tick_miners(&mut sim, &rules, &config, Some(&grid));

        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.miner_state(), Some(MinerState::Dock));
        assert_eq!(entity.radio_contacts.slot(0), Some(REFINERY_ID));
        assert!(
            crate::sim::miner::miner_dock::has_contact(&sim, REFINERY_ID, MINER_ID),
            "HELLO accepted on the same dispatch"
        );
        assert_eq!((entity.position.rx, entity.position.ry), (16, 11));
        assert!(
            !is_adjacent_or_at((16, 11), refinery_dock_for_sid(&sim, REFINERY_ID).unwrap()),
            "the hand-off happened without adjacency to the CAN_DOCK cell"
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
            Health { current: 300 },
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        ge.lifecycle.in_limbo = false;
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
        tick_miners(&mut sim, &rules, &config, Some(&grid));

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
            entity.navigation.nav_com,
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
        // Cell (14, 11): 640 leptons (2.5 cells) east of the centre.
        spawn_returning_war_miner(&mut sim, (14, 11));
        spawn_slot_holder(&mut sim);

        tick_miners(&mut sim, &rules, &config, Some(&grid));

        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.miner_state(), Some(MinerState::ReturnToRefinery));
        assert!(
            entity.movement_target.is_none(),
            "`CMP EAX,0x300; JG` not taken"
        );

        // Slot frees: the next dispatch's HELLO is accepted and hands off.
        crate::sim::miner::miner_dock::break_contact(&mut sim, BLOCKER_ID, REFINERY_ID);
        sim.session.binary_frame += 20;
        tick_miners(&mut sim, &rules, &config, Some(&grid));
        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.miner_state(), Some(MinerState::Dock));
        assert_eq!(entity.radio_contacts.slot(0), Some(REFINERY_ID));
    }
}
