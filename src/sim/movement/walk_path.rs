//! Foot4D3920 request ownership inside Walk75AEC0. The precheck is distinct
//! from AStar42C900's entry gate and runs before the wrapper removes occupancy.
//! Original caller comparisons: tools/spatial_oracle/walk_failed_path.
//!
//! Failure continuation (`FootClass::Find_Path` 0x4D4016..0x4D41F0) and the
//! Infantry receiver it calls first (`Infantry vtable +0x500` = 0x0051DAF0)
//! live here as well, together with the two target redirects that precede the
//! core search (`Can_Enter_Cell` codes 6 and 7 at 0x4D3A92 and 0x4D3CDD).

use super::ground_pose;
use super::infantry_entry::{InfantryEntryArgs, InfantryEntryClass};
use super::movement_tick::WalkPathRequest;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::locomotor_type::MovementZone;
use crate::rules::mission_data::MissionType;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_kernel::{native_xy_distance, native_xyz_distance};
use crate::sim::cell_rect::PlayfieldBounds;
use crate::sim::components::DriveCoord;
use crate::sim::find_nearby_cell::{
    NearbyAnchorGate, NearbyFootprint, NearbyQuery, PassabilityArgs, find_nearby_passable_cell,
    map_owned_radius_cap,
};
use crate::sim::mission::MissionId;
use crate::sim::mission::authority::EntityReadyInputProvider;
use crate::sim::mission::concrete_effects::represented_assign_target;
use crate::sim::pathfinding::zone_map::{ZoneGrid, ZoneQueryCell};
use crate::sim::pathfinding::{PathGrid, zone_search::PathSearchFailure};
use crate::sim::world::Simulation;

/// Values retained by Foot4D3810 after its virtual+4C and+320 calls. Both
/// packed coordinates are local copies; they do not alias CellClass Dummy.
struct FootZoneQuery {
    source: DriveCoord,
    current: DriveCoord,
    destination: (i16, i16),
    movement_zone: MovementZone,
    on_bridge: bool,
    in_tube: bool,
    allow_destination_fringe: bool,
}

fn query_foot_zone(
    cells: &NativeCellQuery<'_>,
    zones: &ZoneGrid,
    bounds: PlayfieldBounds,
    size: (i32, i32),
    query: &FootZoneQuery,
) -> Result<bool, String> {
    //4D38DB reads the destination's raw structural bit before +BC. The
    //earlier +320 return remains on the native stack as CanReach argument6.
    let destination_bridge = cells.flags(cells.lookup(query.destination)) & 0x100 != 0;
    let source_bridge = ground_pose::navigation_should_be_on_bridge(
        cells,
        query.source,
        query.current,
        query.on_bridge,
        query.in_tube,
    )?;
    zones
        .can_reach_native(
            cells,
            ZoneQueryCell::Copied(((query.source.x / 256) as i16, (query.source.y / 256) as i16)),
            ZoneQueryCell::Copied(query.destination),
            query.movement_zone,
            source_bridge,
            destination_bridge,
            query.allow_destination_fringe,
            bounds,
            size,
        )
        .ok_or_else(|| "Foot precheck requires native zone topology".into())
}

/// `ObjectClass::Get_Cell_Packed` 0x0041BEA0 and the inlined copies at
/// 0x4D404A..0x4D409E: signed leptons to a cell with truncation toward zero.
fn lepton_to_cell(leptons: i32) -> i16 {
    (leptons.wrapping_add((leptons >> 31) & 0xff) >> 8) as i16
}

fn coord_cell(coord: DriveCoord) -> (i16, i16) {
    (lepton_to_cell(coord.x), lepton_to_cell(coord.y))
}

/// `max(|dx|, |dy|)` over packed cell words, the shape at 0x4D3B7D..0x4D3BBD
/// and 0x4D404A..0x4D40A4.
fn chebyshev(a: (i16, i16), b: (i16, i16)) -> i32 {
    (i32::from(a.0) - i32::from(b.0))
        .abs()
        .max((i32::from(a.1) - i32::from(b.1)).abs())
}

fn cell_centre(cell: (i16, i16)) -> DriveCoord {
    DriveCoord {
        x: i32::from(cell.0) * 256 + 128,
        y: i32::from(cell.1) * 256 + 128,
        z: 0,
    }
}

/// Table 0x007E8BF0, indexed by `Type+0x5B4` at 0x4D3AC3 and 0x4D3D25: the
/// MovementZone row `Find_Path` hands FNPC and GetZoneID (13 dwords read from
/// the binary: 0,0,0,5,5,5,6,7,7,9,10,11,0).
fn find_path_search_zone(zone: MovementZone) -> Option<MovementZone> {
    Some(match zone {
        MovementZone::Normal
        | MovementZone::Crusher
        | MovementZone::Destroyer
        | MovementZone::CrusherAll => MovementZone::Normal,
        MovementZone::AmphibiousDestroyer
        | MovementZone::AmphibiousCrusher
        | MovementZone::Amphibious => MovementZone::Amphibious,
        MovementZone::Subterranean => MovementZone::Subterranean,
        MovementZone::Infantry | MovementZone::InfantryDestroyer => MovementZone::Infantry,
        MovementZone::Fly => MovementZone::Fly,
        MovementZone::Water => MovementZone::Water,
        MovementZone::WaterBeach => MovementZone::WaterBeach,
        MovementZone::Invalid => return None,
    })
}

/// Infantry 0x51DAF6..0x51DB44: the action the failed-path receiver requests
/// from `Do_Action` before it tests the current cell.
pub(crate) fn failed_path_requested_action(doing: i32, prone: bool) -> i32 {
    if (27..=30).contains(&doing) {
        28
    } else if prone {
        2
    } else {
        0
    }
}

/// `Do_Action` 0x0051D6F0 admission for the three receiver requests, after the
/// requested-sequence gate: an unchanged action refuses (0x51D90B), and an
/// established non-idle action refuses unless its record byte0 is
/// interruptible (0x51D934). `-1` always admits.
pub(crate) fn failed_path_do_action_admits(current: i32, requested: i32) -> bool {
    if requested == current {
        return false;
    }
    current == -1
        || crate::rules::infantry_sequence::action_record(current)
            .is_some_and(|record| record.interruptible)
}

impl Simulation {
    /// Synchronous75AFC5 -> Foot4D3920. A successful result resumes this same
    /// Process invocation; a failed result owns its cleanup and must never
    /// enter the arrival finalizer (which would snap the actor's exact XYZ).
    pub(crate) fn run_walk_path_request(
        &mut self,
        request: &WalkPathRequest,
        rules: Option<&RuleSet>,
        fallback: Option<&PathGrid>,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<bool, String> {
        let rules = rules.ok_or("Walk path request requires rules")?;
        let id = request.entity_id;
        let frame = self.session.binary_frame;
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Walk path requester")?;
        //75AF69..8F arms the caller timer. Foot4D393A clears one head for
        //append=false before its own+2CC precheck; backing suffix is retained.
        actor
            .navigation
            .path_runtime
            .start_movement(frame, self.path_delay_ticks, true);
        actor.navigation.path_replay.clear_live_head();
        let admitted = self.walk_path_zone_precheck(id, request.destination, rules)?;
        if !admitted {
            //4D3989..399C returns false before Mark, AStar or+500. The
            //locomotor's separate zone recheck still occurs after that return.
            return self
                .finish_failed_walk_process(id, rules, registry)
                .map(|()| false);
        }

        let goal = self.walk_path_admitted_goal(id, request.destination, rules, registry)?;
        match self.search_walk_path(request, goal, rules, fallback, registry)? {
            Ok(()) => Ok(true),
            Err(
                PathSearchFailure::NativeEntryRejected | PathSearchFailure::CellSearchExhausted,
            ) => {
                //4D4016..4D4041 arms PathDelay (Rules+1760 scaled, ftol), then
                //4D4044 calls the Infantry receiver+500, which Stops Walk
                //BEFORE the outer caller reads the retained destination.
                self.substrate
                    .entities
                    .get_mut(id)
                    .ok_or("retired Walk core receiver")?
                    .navigation
                    .path_runtime
                    .start_movement(frame, self.path_delay_ticks, true);
                self.run_infantry_failed_path_receiver(id, rules, registry)?;
                self.finish_walk_find_path_failure(id, goal, rules)?;
                self.finish_failed_walk_process(id, rules, registry)?;
                Ok(false)
            }
            Err(error) => Err(format!(
                "Walk core prerequisite/compatibility rejection: {error:?}; no native failure inferred"
            )),
        }
    }

    /// Mark0, the core search, Mark1 and the +640 rewrite; a found route is
    /// installed with the 0x75B2E2 retry reset. Only search failures return
    /// through the inner `Err`; unavailable Rust prerequisites stop the frame.
    fn search_walk_path(
        &mut self,
        request: &WalkPathRequest,
        goal: DriveCoord,
        rules: &RuleSet,
        fallback: Option<&PathGrid>,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<Result<(), PathSearchFailure>, String> {
        let id = request.entity_id;
        let frame = self.session.binary_frame;
        if self.path_grid.is_none() && fallback.is_none() {
            return Err("Walk core requires PathGrid; no native failure inferred".into());
        }
        self.walk_mark_remove(id, Some(rules), fallback, registry);
        let searched = {
            let grid = self.path_grid_snapshot();
            let grid = grid.as_deref().or(fallback);
            //These occupancy-derived inputs must see Mark0, including the
            //actor's removal. Reuse the single existing search implementation.
            let counts = grid.map(|grid| {
                super::bump_crush::build_blocker_neighbor_counts_with_overlays(
                    &self.substrate.entities,
                    grid.width(),
                    grid.height(),
                    self.resolved_terrain.as_ref(),
                    self.overlay_grid.as_ref(),
                    registry,
                    &self.interner,
                    Some(rules),
                )
            });
            request.search(
                goal,
                &self.substrate.entities,
                super::PathfindingContext {
                    wall_tables: Some(crate::sim::pathfinding::cell_entry::WallArmTables {
                        overlay_grid: self.overlay_grid.as_ref(),
                        overlay_registry: registry,
                        alliances: Some(&self.house_alliances),
                        interner: Some(&self.interner),
                    }),
                    path_grid: grid,
                    zone_grid: self.zone_grid.as_ref(),
                    resolved_terrain: self.resolved_terrain.as_ref(),
                    playfield_bounds: self.playfield_bounds,
                    blocker_neighbor_counts: counts.as_ref(),
                },
                &self.terrain_costs,
                &self.house_alliances,
                &self.interner,
                Some(rules),
            )
        };
        //4D3EAC restores Mark1 before inspecting the core result. Also restore
        //on unavailable Rust prerequisites, without inventing native failure.
        self.walk_mark_put(id, Some(rules), fallback, registry);
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Walk core receiver")?;
        actor.navigation.path_runtime.start_movement(frame, 0, true);
        match searched {
            Ok((path, layers)) => {
                if path.len() < 2 {
                    return Err(
                        "Walk core returned an unclassified singleton path; native +4 is cost, not count"
                            .into(),
                    );
                }
                request.install_route(actor, path, layers);
                //75B2DF..E2 is the success caller's retry reset. Existing
                //head production resumes at its ordinary shared owner.
                actor.navigation.path_runtime.retries_left = super::PATH_STUCK_INIT;
                Ok(Ok(()))
            }
            Err(super::movement_path::MovePathFailure::Search(failure)) => Ok(Err(failure)),
            Err(error) => Err(format!(
                "Walk core prerequisite/compatibility rejection: {error:?}; no native failure inferred"
            )),
        }
    }

    /// Infantry vtable +0x500 = 0x0051DAF0, reached from the `Find_Path`
    /// failure at 0x4D4044 (and from `Do_Action` 0x51D6F0 at zero health,
    /// which no live actor here has). Order: `Do_Action` request by Doing and
    /// the prone byte (+6DB), current-cell `Can_Enter_Cell` (+1AC) with the
    /// facing octant and the Techno height helper 0x5F5F00 (this+8C OnBridge
    /// plus the current cell's +11B level through vtable +0x1BC), the +6DC
    /// answer byte, then Foot 0x4D55C0 -> locomotor +0x48 (Walk Stop 0x75ADA0).
    pub(crate) fn run_infantry_failed_path_receiver(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<(), String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired failed-path receiver")?;
        let object = self
            .object_type(actor.type_ref(), rules)
            .ok_or("failed-path receiver requires the Infantry type")?;
        let type_id = object.id.clone();
        let movement_zone = object.movement_zone;
        let doing = actor
            .mission_leaf
            .as_infantry()
            .ok_or("failed-path receiver requires Infantry Doing")?
            .doing();
        let prone = actor
            .infantry
            .as_ref()
            .is_some_and(|infantry| infantry.is_prone);
        let on_bridge = actor.on_bridge;
        let coord = ground_pose::position_world_coord(&actor.position);
        //0x51DB68..0x51DB7A: the 16-bit facing (+388) becomes an octant through
        //`((facing >> 12) + 1) >> 1 & 7`; the stored 8-bit facing is its high byte.
        let direction = (((i32::from(actor.facing) >> 4) + 1) >> 1) & 7;
        let requested = failed_path_requested_action(doing, prone);
        self.apply_failed_path_do_action(
            id,
            doing,
            requested,
            &type_id,
            movement_zone,
            on_bridge,
            rules,
        )?;

        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("failed-path receiver requires map cells")?;
        let cells = NativeCellQuery::canonical(terrain);
        let cell = cells.lookup(coord_cell(coord));
        //0x5F5F00 (ECX = this Infantry, 0x51DB78): the current cell's signed
        //level byte (+11B via vtable +1BC) plus four when OnBridge (+8C).
        let height = i32::from(cells.ground_fields(cell).0 as i8) + if on_bridge { 4 } else { 0 };
        let answer = self.infantry_can_enter(
            id,
            cell,
            InfantryEntryArgs {
                direction,
                height,
                previous_cell: None,
            },
            rules,
            registry,
        )?;
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("failed-path receiver actor retired during Can_Enter_Cell")?;
        //0x51DBAC stores the zero answer byte; 0x51DBBE stores 1 for any other.
        actor
            .infantry
            .as_mut()
            .ok_or("failed-path receiver requires Infantry runtime state")?
            .cell_entry_blocked = answer.is_nonzero();
        //0x4D55C0 -> ILocomotion +0x48. Walk 0x75ADA0 clears the destination
        //and, with no paid head, the IsMoving byte.
        actor
            .locomotor
            .as_mut()
            .ok_or("failed-path receiver requires the Walk locomotor")?
            .stop_walk();
        Ok(())
    }

    /// Bounded `InfantryClass::Do_Action` 0x0051D6F0 for the receiver's
    /// requests 0, 2 and 28 with force and random-frame arguments 0. The
    /// requested-sequence count gate (Type+E3C record) precedes everything.
    /// Two remaps change the written action and carry side effects Rust does
    /// not own (the +6E8 wet reclassification with its sound request at
    /// 0x51D842..0x51D8B8, and the airborne Hover remap through vtable+0x54):
    /// an AmphibiousDestroyer type on a Water/Beach cell off a bridge, or a
    /// request 0 while airborne. Both leave Doing untouched here; Doing is
    /// otherwise not maintained by an InfantryClass::AI port and only its
    /// sim consumers (readiness, hut Scatter, the Walk null setter) read it.
    /// Frame (+F8), logical timer (+100..+10C) and image frame (+3E) have no
    /// Rust owner and stay a visual residual.
    #[allow(clippy::too_many_arguments)]
    fn apply_failed_path_do_action(
        &mut self,
        id: u64,
        current: i32,
        requested: i32,
        type_id: &str,
        movement_zone: MovementZone,
        on_bridge: bool,
        rules: &RuleSet,
    ) -> Result<(), String> {
        use crate::rules::animation_sequence::SequenceKind;
        let kind = match requested {
            0 => SequenceKind::Stand,
            2 => SequenceKind::Prone,
            28 => SequenceKind::Deployed,
            other => {
                return Err(format!(
                    "failed-path receiver requested action {other} outside 0x51DAF6..0x51DB44"
                ));
            }
        };
        //0x51D70F: a zero requested-sequence count refuses before any state.
        let has_sequence = rules
            .animation_sequence(type_id)
            .and_then(|set| set.get(&kind))
            .is_some_and(|sequence| sequence.frame_count != 0);
        if !has_sequence {
            return Ok(());
        }
        if movement_zone == MovementZone::AmphibiousDestroyer && !on_bridge {
            let actor = self
                .substrate
                .entities
                .get(id)
                .ok_or("retired Do_Action receiver")?;
            let land = self
                .resolved_terrain
                .as_ref()
                .and_then(|terrain| terrain.cell(actor.position.rx, actor.position.ry))
                .map(|cell| cell.yr_cell_land_type);
            //0x51D7C6: LandType Water(2)/Beach(6) remaps 0/2 -> 16 and writes +6E8.
            if matches!(land, Some(2) | Some(6)) {
                return Ok(());
            }
        }
        if !failed_path_do_action_admits(current, requested) {
            return Ok(());
        }
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Do_Action receiver")?;
        actor
            .mission_leaf
            .set_infantry_doing_verified(requested)
            .map_err(|error| format!("failed-path Do_Action wrote an invalid Doing: {error:?}"))?;
        Ok(())
    }

    /// `Find_Path` failure continuation 0x4D404A..0x4D41F0 after the receiver:
    /// a target within one cell (Chebyshev over the `+9C` cell) returns unless
    /// the target cell is structural while the actor is off a bridge
    /// (0x4D40AB..0x4D40D4); otherwise Team detachment, `SetDestination(NULL,1)`
    /// (+0x480), `Assign_Target(NULL)` (+0x3C8 = Infantry 0x51B1F0), then the
    /// House 0x50B730 split: human -> `Queue_Mission(Guard=5, 0)` (0x4D41D6),
    /// nonhuman -> `Queue_Mission(AreaGuard=11, 0)` and, with GameMode != 0,
    /// `Find_Passable_Cell_Near_Unit` 0x500200 -> `SetDestination(cell, 1)`.
    pub(crate) fn finish_walk_find_path_failure(
        &mut self,
        id: u64,
        goal: DriveCoord,
        rules: &RuleSet,
    ) -> Result<(), String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired failed Find_Path actor")?;
        let current = coord_cell(ground_pose::position_world_coord(&actor.position));
        //[ESP+0x1fac] at the tail's depth is the target parameter slot, the same
        //slot the code-6/7 redirects rewrote as [ESP+0x1fb0] one push deeper
        //(0x4D3CD1, 0x4D3E03) and Run_AStar read as its destination: a redirected
        //search that still fails measures against the redirected cell.
        let target = ((goal.x / 256) as i16, (goal.y / 256) as i16);
        if chebyshev(current, target) <= 1 {
            if actor.on_bridge {
                return Ok(());
            }
            let terrain = self
                .resolved_terrain
                .as_ref()
                .ok_or("Find_Path failure requires map cells")?;
            let cells = NativeCellQuery::canonical(terrain);
            if cells.flags(cells.lookup(target)) & 0x100 == 0 {
                return Ok(());
            }
        }
        if self.team_script_vm.team_for_member(id).is_some() {
            //0x4D40DA..0x4D4134: locomotor +B4, TeamClass::Remove_Member
            //0x6EA870, locomotor +B8. No Rust Team membership owner exists.
            return Err(
                "Walk failed-path Team detachment (TeamClass::Remove_Member 0x6EA870) is not represented"
                    .into(),
            );
        }
        let owner = actor.owner();
        //0x4D413A: Infantry 0x51AA40(NULL, true) -> Foot 0x4D94B0 -> Walk 0x75ADA0.
        self.set_walk_null_destination(id, Some(rules));
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired failed Find_Path actor")?;
        //0x4D4149: the Infantry target setter with NULL.
        represented_assign_target(actor, None);
        let human = self
            .houses
            .get(&owner)
            .is_some_and(|house| house.is_controlled_by_human(self.session.game_mode_nonzero));
        let mission = if human {
            MissionType::Guard
        } else {
            MissionType::AreaGuard
        };
        let now = self.session.binary_frame;
        // Queue's own guards decide; with commence 0 only a missing receiver
        // fails, and that receiver was just resolved.
        let _ = self.mission_queue_exact(
            id,
            MissionId::from_known(mission),
            0,
            now,
            &EntityReadyInputProvider,
        );
        //0x4D4174..0x4D41C2: with GameMode != 0 a nonhuman actor is relocated to
        //`Find_Passable_Cell_Near_Unit` 0x500200 (House 0x501AC0 variants over
        //the House+5498 radius, base-or-starting cell, RNG and the sin/cos
        //tables) through SetDestination(cell, 1). Residual, deliberately
        //non-stopping: the actor stays in place under AreaGuard. Trigger: an AI
        //or neutral House infantryman whose core search fails in skirmish.
        //Effect: no relocation walk. Frequency: rare without AI production.
        //Downstream: the House base projection (house_base.rs) is the pending
        //input of that owner.
        let _ = human;
        Ok(())
    }

    /// `Find_Path` 0x4D3944..0x4D3E0A before the core search: the target's
    /// `Can_Enter_Cell` answer selects a redirect. Code 6 beyond CloseEnough
    /// (Rules+1718; a Team's own value via 0x6F03B0) asks FNPC for a cell near
    /// the target, accepts it only when it lies closer to the target than the
    /// actor (0x4D3C39) and `EstimateZoneCost` 0x42D170 admits it (at most
    /// Chebyshev plus 6), then `SetDestination(cell, 1)` retargets the search.
    /// Code 7 with a Building in the target cell (0x47C520) redirects
    /// unconditionally.
    fn walk_path_admitted_goal(
        &mut self,
        id: u64,
        destination: DriveCoord,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<DriveCoord, String> {
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Walk goal requires map cells")?;
        let target = ((destination.x / 256) as i16, (destination.y / 256) as i16);
        let cell = terrain.native_cell_identity(target);
        let answer =
            self.infantry_can_enter(id, cell, InfantryEntryArgs::REPAIR, rules, registry)?;
        self.walk_path_goal_for_answer(id, destination, answer, rules)
    }

    /// The redirect selection of 0x4D3A92..0x4D3E0A for an already computed
    /// target `Can_Enter_Cell` answer.
    pub(crate) fn walk_path_goal_for_answer(
        &mut self,
        id: u64,
        destination: DriveCoord,
        answer: InfantryEntryClass,
        rules: &RuleSet,
    ) -> Result<DriveCoord, String> {
        let target = ((destination.x / 256) as i16, (destination.y / 256) as i16);
        let cell = self
            .resolved_terrain
            .as_ref()
            .ok_or("Walk goal requires map cells")?
            .native_cell_identity(target);
        match answer {
            InfantryEntryClass::Clear | InfantryEntryClass::SoftOther => Ok(destination),
            InfantryEntryClass::Obstructed6 => {
                let actor = self
                    .substrate
                    .entities
                    .get(id)
                    .ok_or("retired Walk goal actor")?;
                if self.team_script_vm.team_for_member(id).is_some() {
                    return Err(
                        "Find_Path CloseEnough for a Team member (0x6F03B0) is not represented"
                            .into(),
                    );
                }
                //0x4D3944..0x4D3A2B: |+48 coordinate - target centre| with z.
                let coord = ground_pose::position_world_coord(&actor.position);
                let centre = cell_centre(target);
                let distance =
                    native_xyz_distance(coord.x - centre.x, coord.y - centre.y, coord.z - centre.z);
                //0x4D3A9B: dist <= CloseEnough keeps the target. IsTrain (+C94)
                //is absent from every stock InfantryType (cell_entry.rs).
                if distance <= self.close_enough.to_num::<i32>() {
                    return Ok(destination);
                }
                let Some(near) = self.find_path_nearby_cell(id, target, rules)? else {
                    //0x4D3BC2: an invalid (0,0) FNPC cell keeps the target.
                    return Ok(destination);
                };
                //0x4D3BDD..0x4D3C3D: the FNPC cell must lie closer to the target
                //than the actor does (both x87 lepton distances, ftol).
                let near_centre = cell_centre(near);
                let between =
                    native_xy_distance(centre.x - near_centre.x, centre.y - near_centre.y);
                if between >= distance {
                    return Ok(destination);
                }
                if !self.find_path_zone_cost_admits(id, near, target, rules)? {
                    return Ok(destination);
                }
                self.redirect_walk_destination(id, near, rules)?;
                Ok(cell_centre(near))
            }
            InfantryEntryClass::Impassable7 => {
                //4D3CDD..CF4 only enters the code7 FNPC arm when the
                //target's ground object list contains a Building. A hard
                //answer without that Building still reaches the core goal.
                let coord = self
                    .resolved_terrain
                    .as_ref()
                    .unwrap()
                    .native_cell_coord(cell);
                if self
                    .substrate
                    .occupancy
                    .first_building_on_layer(
                        coord.0 as u16,
                        coord.1 as u16,
                        super::locomotor::MovementLayer::Ground,
                    )
                    .is_none()
                {
                    return Ok(destination);
                }
                //0x4D3DD8..0x4D3E03: the FNPC result is set and searched without
                //an invalid-cell test; only an FNPC failure over the whole map
                //radius would search cell (0,0), which no test fixture produces.
                let near = self.find_path_nearby_cell(id, target, rules)?.ok_or(
                    "Find_Path code-7 redirect found no passable cell near the Building target",
                )?;
                self.redirect_walk_destination(id, near, rules)?;
                Ok(cell_centre(near))
            }
        }
    }

    /// FNPC 0x56DC20 as `Find_Path` calls it (0x4D3B0A..0x4D3B76 and
    /// 0x4D3D6C..0x4D3DD8): seed = target cell, Type+67C speed, zone =
    /// GetZoneID(current cell, table row, OnBridge), the table row, OnBridge,
    /// footprint 1x1, height check on, occupancy check = the Subterranean
    /// obstacle flag (Type+D2C, never set for stock infantry), bridge cells
    /// allowed, nearest to the actor's current cell.
    fn find_path_nearby_cell(
        &self,
        id: u64,
        target: (i16, i16),
        rules: &RuleSet,
    ) -> Result<Option<(i16, i16)>, String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired FNPC requester")?;
        let object = self
            .object_type(actor.type_ref(), rules)
            .ok_or("Find_Path FNPC requires the Infantry type")?;
        if object.movement_zone == MovementZone::Subterranean {
            return Err(
                "Find_Path FNPC obstacle flag (Type+D2C, 0x486FF0) is not represented".into(),
            );
        }
        let zone = find_path_search_zone(object.movement_zone)
            .ok_or("Find_Path FNPC requires a valid MovementZone")?;
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Find_Path FNPC requires map cells")?;
        let zones = self
            .zone_grid
            .as_ref()
            .ok_or("Find_Path FNPC requires zone topology")?;
        let size = self
            .playfield_bounds
            .zip(self.playfield_size_height)
            .map(|(bounds, height)| (bounds.base, height))
            .or_else(|| self.bridge_state.as_ref()?.native_zone_source_size())
            .ok_or("Find_Path FNPC requires original Map Size")?;
        let current = coord_cell(ground_pose::position_world_coord(&actor.position));
        let current = (i32::from(current.0), i32::from(current.1));
        let required_zone_id = zones.get_zone_id_native(current, zone, actor.on_bridge);
        let cells = NativeCellQuery::canonical(terrain);
        let grid = self.path_grid_snapshot();
        Ok(find_nearby_passable_cell(
            (i32::from(target.0), i32::from(target.1)),
            &NearbyQuery {
                native_cells: Some(&cells),
                raw_occupation: Some(&self.substrate.raw_cell_occupation),
                passability: PassabilityArgs {
                    speed_type: object.speed_type,
                    required_zone_id,
                    movement_zone: zone,
                    bridge_aware_zone: actor.on_bridge,
                },
                footprint: NearbyFootprint::SINGLE,
                anchor_gate: NearbyAnchorGate::NativeHeightAware,
                allow_bridge_cells: true,
                check_height: true,
                check_occupancy: false,
                radius_cap: map_owned_radius_cap(size.0, size.1),
                target_cell: Some(current),
                path_grid: grid.as_deref(),
                resolved_terrain: Some(terrain),
                overlay_grid: self.overlay_grid.as_ref(),
                occupancy: Some(&self.substrate.occupancy),
                entities: Some(&self.substrate.entities),
                zone_grid: self.zone_grid.as_ref(),
                playfield_bounds: self.playfield_bounds,
            },
            self.session.binary_frame,
        )
        .filter(|cell| *cell != (0, 0))
        .map(|cell| (cell.0 as i16, cell.1 as i16)))
    }

    /// `PathfinderClass::EstimateZoneCost` 0x42D170 <= Chebyshev + 6, as the
    /// code-6 arm tests it (0x4D3C9C..0x4D3CAA). Zone_precheck 0x42C290 failure
    /// returns INT_MAX (never admitted); a passing precheck with both cells off
    /// structural terrain returns `max(Chebyshev, 2 * level-0 hops)`. Rust keeps
    /// the raw zone labels but no level graph: equal labels are taken as a
    /// zero-hop path (cost = Chebyshev, admitted), unequal labels as the
    /// precheck failure. Residual: a same-label pair whose level-0 zone path
    /// detours by more than (Chebyshev + 6) / 2 hops is admitted here where the
    /// original keeps the obstructed target; it needs a nearby cell separated
    /// from the target by an obstacle inside CloseEnough. The structural
    /// bridge terms (0x42D2C0..0x42D438) are not represented.
    fn find_path_zone_cost_admits(
        &self,
        id: u64,
        near: (i16, i16),
        target: (i16, i16),
        rules: &RuleSet,
    ) -> Result<bool, String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired zone-cost requester")?;
        let object = self
            .object_type(actor.type_ref(), rules)
            .ok_or("EstimateZoneCost requires the Infantry type")?;
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("EstimateZoneCost requires map cells")?;
        let cells = NativeCellQuery::canonical(terrain);
        let near_bridge = cells.flags(cells.lookup(near)) & 0x100 != 0;
        let target_bridge = cells.flags(cells.lookup(target)) & 0x100 != 0;
        //Residual, non-stopping: with a structural near or target cell the
        //original adds the bridge-adjacent zone-cell terms (0x42D2C0..0x42D438,
        //PathfinderClass +B8.. records) to the cost, which can refuse a redirect
        //this same-label test admits. Trigger: a code-6 target (a parked Unit
        //without NavCom) on or beside a bridge beyond CloseEnough. Effect: the
        //redirect is taken where the original keeps the obstructed target.
        let _ = (near_bridge, target_bridge);
        let zones = self
            .zone_grid
            .as_ref()
            .ok_or("EstimateZoneCost requires zone topology")?;
        let label = |cell: (i16, i16)| {
            zones.get_zone_id_native(
                (i32::from(cell.0), i32::from(cell.1)),
                object.movement_zone,
                false,
            )
        };
        let (Some(near_label), Some(target_label)) = (label(near), label(target)) else {
            return Err("EstimateZoneCost requires native zone labels for both cells".into());
        };
        Ok(near_label == target_label)
    }

    /// `SetDestination(GetCell(cell), 1)` from inside `Find_Path` (0x4D3CC7,
    /// 0x4D3DF9): the ordinary Infantry setter, which installs NavCom and the
    /// Walk destination without a search of its own.
    fn redirect_walk_destination(
        &mut self,
        id: u64,
        cell: (i16, i16),
        rules: &RuleSet,
    ) -> Result<(), String> {
        let move_info = self
            .resolve_move_info(id, Some(rules))
            .ok_or("Find_Path redirect requires the actor's move info")?;
        let grid = self.path_grid_snapshot();
        let grid = grid
            .as_deref()
            .ok_or("Find_Path redirect requires navigation")?;
        let speed_type = self
            .substrate
            .entities
            .get(id)
            .and_then(|actor| self.object_type(actor.type_ref(), rules))
            .map(|object| object.speed_type)
            .ok_or("Find_Path redirect requires the Infantry type")?;
        if !super::prepare_walk_cell_destination(
            &mut self.substrate.entities,
            grid,
            id,
            (cell.0 as u16, cell.1 as u16),
            move_info.speed,
            self.terrain_costs.get(&speed_type),
            self.resolved_terrain.as_ref(),
            self.zone_grid.as_ref(),
            self.playfield_bounds,
            &mut self.substrate.cell_occupation,
            super::DestinationTiming::new(
                self.session.binary_frame,
                rules.general.blockage_path_delay_ticks,
            ),
        ) {
            return Err(
                "Find_Path redirect destination was not accepted by the Walk setter".into(),
            );
        }
        Ok(())
    }

    fn finish_failed_walk_process(
        &mut self,
        id: u64,
        rules: &RuleSet,
        _registry: Option<&OverlayTypeRegistry>,
    ) -> Result<(), String> {
        //ESI points into the live Walk destination, not a copy retained before
        //FindPath. Infantry+500 can have cleared it, and House can replace it.
        let destination = self
            .substrate
            .entities
            .get(id)
            .and_then(|e| e.locomotor.as_ref())
            .and_then(|l| l.walk_destination())
            .unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
        if !self.walk_path_zone_precheck(id, destination, rules)? {
            self.set_walk_null_destination(id, Some(rules));
        } else {
            //Only the nonhuman relocation (0x4D41C2) leaves a live destination
            //after the receiver's Stop, and that arm stops earlier.
            return Err(
                "Walk failed-search retained-zone continuation awaits the nonhuman relocation owner"
                    .into(),
            );
        }
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired failed Walk actor")?;
        if actor
            .locomotor
            .as_ref()
            .is_some_and(|l| l.walk_destination().is_none() && l.step_head().is_none())
        {
            actor.movement_target = None;
        }
        Ok(())
    }

    /// Original Foot+2CC4D3810. Its MZ==-1 and Cell(0,0) exits precede
    /// navigation, Team, map and source-bridge queries.
    pub(crate) fn walk_path_zone_precheck(
        &self,
        id: u64,
        destination: DriveCoord,
        rules: &RuleSet,
    ) -> Result<bool, String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("missing Foot precheck actor")?;
        let object = rules
            .object(self.interner.resolve(actor.type_ref()))
            .ok_or("missing Foot precheck type")?;
        if object.movement_zone == MovementZone::Invalid {
            return Ok(true);
        }
        let destination = ((destination.x / 256) as i16, (destination.y / 256) as i16);
        if destination == (0, 0) {
            return Ok(false);
        }
        let source = self.infantry_navigation_coordinate(id)?;
        //4DA1D0: stock Infantry +3D4 has no positive producer (all writers
        //target Aircraft), and IsTrain+C94 is absent from the admitted stock
        //types. Do not infer either from airborne/locomotor state.
        let allow_destination_fringe = if !actor.in_playfield {
            false
        } else if actor.mission.effective().raw() == 4 {
            true
        } else if self.team_script_vm.team_for_member(id).is_some() {
            return Err("Foot precheck requires attached Team6EC300 state/effects".into());
        } else {
            false
        };
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Foot precheck requires map cells")?;
        let cells = NativeCellQuery::canonical(terrain);
        let zones = self
            .zone_grid
            .as_ref()
            .ok_or("Foot precheck requires zone topology")?;
        let bounds = self
            .playfield_bounds
            .ok_or("Foot precheck requires playfield bounds")?;
        let size = self
            .playfield_size_height
            .map(|height| (bounds.base, height))
            .or_else(|| self.bridge_state.as_ref()?.native_zone_source_size())
            .ok_or("Foot precheck requires original Map Size")?;
        query_foot_zone(
            &cells,
            zones,
            bounds,
            size,
            &FootZoneQuery {
                source,
                current: ground_pose::position_world_coord(&actor.position),
                destination,
                movement_zone: object.movement_zone,
                on_bridge: actor.on_bridge,
                in_tube: actor.low_bridge_tube_state.is_some(),
                allow_destination_fringe,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receiver_action_follows_0x51daf6_selection() {
        for doing in 27..=30 {
            assert_eq!(failed_path_requested_action(doing, true), 28);
        }
        assert_eq!(failed_path_requested_action(0, true), 2);
        assert_eq!(failed_path_requested_action(-1, false), 0);
        assert_eq!(failed_path_requested_action(3, false), 0);
    }

    #[test]
    fn do_action_refuses_unchanged_and_uninterruptible_actions() {
        // Oracle astar_null rows: Doing 0 with request 0 leaves frame/timer alone.
        assert!(!failed_path_do_action_admits(0, 0));
        assert!(failed_path_do_action_admits(-1, 0));
        // Walk (3) is interruptible in the 0x7EAF7C table; Die1 (11) is not.
        assert!(failed_path_do_action_admits(3, 0));
        assert!(!failed_path_do_action_admits(11, 0));
    }

    #[test]
    fn signed_lepton_cell_conversion_truncates_toward_zero() {
        assert_eq!(lepton_to_cell(2624), 10);
        assert_eq!(lepton_to_cell(255), 0);
        assert_eq!(lepton_to_cell(-1), 0);
        assert_eq!(lepton_to_cell(-256), -1);
        assert_eq!(lepton_to_cell(-257), -1);
    }

    #[test]
    fn search_zone_table_matches_0x7e8bf0() {
        let rows = [
            (MovementZone::Normal, MovementZone::Normal),
            (MovementZone::Destroyer, MovementZone::Normal),
            (MovementZone::AmphibiousDestroyer, MovementZone::Amphibious),
            (MovementZone::Amphibious, MovementZone::Amphibious),
            (MovementZone::Subterranean, MovementZone::Subterranean),
            (MovementZone::Infantry, MovementZone::Infantry),
            (MovementZone::InfantryDestroyer, MovementZone::Infantry),
            (MovementZone::Fly, MovementZone::Fly),
            (MovementZone::Water, MovementZone::Water),
            (MovementZone::WaterBeach, MovementZone::WaterBeach),
            (MovementZone::CrusherAll, MovementZone::Normal),
        ];
        for (zone, row) in rows {
            assert_eq!(find_path_search_zone(zone), Some(row), "{zone:?}");
        }
        assert_eq!(find_path_search_zone(MovementZone::Invalid), None);
    }
}
