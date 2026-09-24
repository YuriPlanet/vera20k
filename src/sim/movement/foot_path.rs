//! `FootClass::Find_Path` 0x004D3920, shared by every ground Foot class.
//!
//! The no-queue callers (Walk 0x75AFC5, Drive 0x4B28A3, Ship 0x6A1EF3) arm
//! their own movement timer and then enter this body with `(cell, 0, 0)`.
//! Order: clear one path head (0x4D392A), the zone precheck `+0x2CC`
//! (0x4D397F, Foot 0x4D3810 for both classes), the target `Can_Enter_Cell`
//! redirects (codes 6 and 7 at 0x4D3A92 and 0x4D3CDD), Mark(0), the core
//! search, Mark(1), the `+0x640` rewrite, and on a core failure the PathDelay
//! re-arm, the class receiver `+0x500` and the continuation 0x4D404A..0x4D41F0.
//! Virtual calls reach the receiver's own class: Infantry and Unit differ in
//! `+0x1AC`, `+0x480`, `+0x500` and `+0x3C8`, never in this body.

use super::block_index::LentOwnerBlockSet;
use super::ground_pose;
use super::infantry_entry::InfantryEntryArgs;
use super::movement_tick::{FootPathCaller, FootPathRequest};
use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone};
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
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::zone_map::{ZoneGrid, ZoneQueryCell};
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
pub(super) fn lepton_to_cell(leptons: i32) -> i16 {
    (leptons.wrapping_add((leptons >> 31) & 0xff) >> 8) as i16
}

pub(super) fn coord_cell(coord: DriveCoord) -> (i16, i16) {
    (lepton_to_cell(coord.x), lepton_to_cell(coord.y))
}

/// `max(|dx|, |dy|)` over packed cell words, the shape at 0x4D3B7D..0x4D3BBD
/// and 0x4D404A..0x4D40A4.
pub(super) fn chebyshev(a: (i16, i16), b: (i16, i16)) -> i32 {
    (i32::from(a.0) - i32::from(b.0))
        .abs()
        .max((i32::from(a.1) - i32::from(b.1)).abs())
}

pub(super) fn cell_centre(cell: (i16, i16)) -> DriveCoord {
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

/// Why the core search produced no route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoreRefusal {
    /// AStar42C900 returned NULL: the raw-label entry reject 42CB22 or an
    /// exhausted cell search.
    Native,
    /// A VERA-only adapter refused where native has no counterpart.
    VeraOnly(&'static str),
}

/// The result of one `Find_Path(cell, 0, 0)` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FindPathResult {
    /// A route of at least one direction was copied into Foot+5E0.
    Route,
    /// The core returned a zero-cost route (start cell is the goal cell):
    /// 0x4D3E52..0x4D3E5F copies no word, yet Find_Path returns success.
    EmptyRoute,
    /// The precheck refused (0x4D3989, before Mark, AStar or +500), or the
    /// core returned NULL and the PathDelay re-arm, the class receiver +500
    /// and the continuation 0x4D404A..0x4D41F0 have run.
    Failed,
}

/// How the suspended Process continues after its no-queue request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FootPathOutcome {
    /// Head selection continues in the same Process invocation
    /// (Walk after 0x75AFC5, Drive 0x4B32A1, Ship 0x6A28F1).
    Resume,
    /// Process_Movement returned. The Drive/Ship outer Process still calls
    /// Process_Track (0x4B0AAA / 0x6A0173); Walk has no further call.
    Returned,
    /// The Foot is gone after Find_Path (Drive/Ship out byte, 0x4B28BE /
    /// 0x6A1F0E): nothing else runs for it this call.
    Deleted,
}

impl Simulation {
    /// Dispatch a suspended no-queue request to its locomotor caller. `lent`
    /// is the requester's owner block set held by the pending pass.
    pub(crate) fn run_foot_path_request(
        &mut self,
        request: &FootPathRequest,
        lent: Option<&mut LentOwnerBlockSet>,
        rules: Option<&RuleSet>,
        fallback: Option<&PathGrid>,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<FootPathOutcome, String> {
        match request.caller {
            FootPathCaller::Walk => self
                .run_walk_path_request(request, lent, rules, fallback, registry)
                .map(|resumed| {
                    if resumed {
                        FootPathOutcome::Resume
                    } else {
                        FootPathOutcome::Returned
                    }
                }),
            FootPathCaller::Track(_) => {
                self.run_track_path_request(request, lent, rules, fallback, registry)
            }
        }
    }

    /// `Find_Path(cell, 0, 0)` for a no-queue caller whose own movement
    /// timer is already armed; see [`FindPathResult`].
    pub(crate) fn foot_find_path(
        &mut self,
        request: &FootPathRequest,
        lent: Option<&mut LentOwnerBlockSet>,
        rules: &RuleSet,
        fallback: Option<&PathGrid>,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<FindPathResult, String> {
        let id = request.entity_id;
        let frame = self.session.binary_frame;
        #[cfg(test)]
        if let Some(path) = super::fresh_oracle_seam::supplied_path(
            (request.destination.x / 256, request.destination.y / 256),
            request.urgency,
        ) {
            use super::fresh_oracle_seam::SuppliedPath;
            let queue = &mut self
                .substrate
                .entities
                .get_mut(id)
                .ok_or("retired Find_Path requester")?
                .navigation
                .path_replay;
            return Ok(match path {
                SuppliedPath::Found(words) => {
                    queue.directions = words;
                    queue.cursor = 0;
                    FindPathResult::Route
                }
                SuppliedPath::Failed => FindPathResult::Failed,
            });
        }
        //4D392A..393A: append=false clears one head before the +2CC
        //precheck; the backing suffix is retained.
        self.substrate
            .entities
            .get_mut(id)
            .ok_or("retired Find_Path requester")?
            .navigation
            .path_replay
            .clear_live_head();
        if !self.foot_path_zone_precheck(id, request.destination, rules)? {
            //4D3989..399C returns false before Mark, AStar or +500.
            return Ok(FindPathResult::Failed);
        }
        let goal = self.find_path_admitted_goal(id, request.destination, rules, registry)?;
        match self.search_foot_path(request, lent, goal, rules, fallback, registry)? {
            Ok(true) => Ok(FindPathResult::Route),
            Ok(false) => Ok(FindPathResult::EmptyRoute),
            Err(refusal) => {
                if let CoreRefusal::VeraOnly(reason) = refusal {
                    //Residual: a VERA-only search refusal (BridgeOnlyGoal,
                    //the compatibility zone graph, a hierarchy cell without
                    //native topology) has no native counterpart; it takes the
                    //native NULL-core continuation below instead of aborting
                    //the frame. Trigger: a goal or corridor those adapters
                    //refuse. Effect: the Foot stops (Guard/AreaGuard) where
                    //AStar42C900 might have routed.
                    log::debug!("Find_Path core: VERA-only refusal {reason} for {id}");
                }
                //4D4016..4D4041 arms PathDelay (Rules+1760 scaled, ftol), then
                //4D4044 calls the class receiver +500 BEFORE the continuation
                //and before the outer caller reads the retained destination.
                self.substrate
                    .entities
                    .get_mut(id)
                    .ok_or("retired Find_Path core receiver")?
                    .navigation
                    .path_runtime
                    .start_movement(frame, rules.general.path_delay_ticks());
                self.run_find_path_failed_receiver(id, rules, registry)?;
                self.finish_find_path_failure(id, goal, rules)?;
                Ok(FindPathResult::Failed)
            }
        }
    }

    /// `+0x500` at 0x4D4044, whose return value Find_Path ignores. Infantry
    /// 0x0051DAF0 performs Do_Action and the current-cell answer before Foot
    /// 0x4D55C0; Unit's slot IS 0x4D55C0, which only calls the active
    /// locomotor's Stop_Moving (+0x48: Drive 0x4AFE00, Ship 0x69F510).
    pub(crate) fn run_find_path_failed_receiver(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<(), String> {
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired failed-path receiver")?;
        match actor.category {
            EntityCategory::Infantry => self.run_infantry_failed_path_receiver(id, rules, registry),
            EntityCategory::Unit => {
                if super::navcom::track_stop_moving(actor) {
                    Ok(())
                } else {
                    Err("Find_Path Unit +0x500 Stop for this locomotor is not represented".into())
                }
            }
            _ => Err("Find_Path +0x500 receiver for this class is not represented".into()),
        }
    }

    /// Mark0, the core search, Mark1 and the +640 rewrite; a found route is
    /// installed with its 0x4D4003 reference cell. `Ok(true)`: at least one
    /// direction; `Ok(false)`: the zero-cost route. Search refusals return
    /// through the inner `Err`; an unavailable PathGrid stops the frame.
    fn search_foot_path(
        &mut self,
        request: &FootPathRequest,
        lent: Option<&mut LentOwnerBlockSet>,
        goal: DriveCoord,
        rules: &RuleSet,
        fallback: Option<&PathGrid>,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<Result<bool, CoreRefusal>, String> {
        let id = request.entity_id;
        let frame = self.session.binary_frame;
        if self.path_grid.is_none() && fallback.is_none() {
            return Err("Find_Path core requires PathGrid; no native failure inferred".into());
        }
        self.foot_mark_remove(id, Some(rules), fallback, registry);
        let owner = request.owner();
        //These occupancy-derived inputs must see Mark0, including the actor's
        //removal: the kept plane and owner sets follow it through the touch
        //log instead of a whole-world rebuild per request.
        let mut borrowed = None;
        let blocks = match lent {
            Some(lent) => {
                self.movement_pass_cache.refresh_lent_block_set(
                    owner,
                    lent,
                    &mut self.substrate.entities,
                    &self.house_alliances,
                    &self.interner,
                    Some(rules),
                );
                lent.sets.clone()
            }
            None => {
                let lent = self.movement_pass_cache.lend_block_set(
                    owner,
                    &mut self.substrate.entities,
                    &self.house_alliances,
                    &self.interner,
                    Some(rules),
                );
                let sets = lent.sets.clone();
                borrowed = Some(lent);
                sets
            }
        };
        let snapshot = self.path_grid_snapshot();
        let grid = snapshot.as_deref().or(fallback).expect("checked above");
        let counts = self
            .movement_pass_cache
            .blocker_plane(
                &mut self.substrate.entities,
                grid,
                self.resolved_terrain.as_ref(),
                self.overlay_grid.as_ref(),
                registry,
                &self.interner,
                Some(rules),
            )
            .clone();
        let searched = request.search(
            goal,
            &self.substrate.entities,
            super::PathfindingContext {
                wall_tables: Some(crate::sim::pathfinding::cell_entry::WallArmTables {
                    overlay_grid: self.overlay_grid.as_ref(),
                    overlay_registry: registry,
                    alliances: Some(&self.house_alliances),
                    interner: Some(&self.interner),
                }),
                path_grid: Some(grid),
                zone_grid: self.zone_grid.as_ref(),
                resolved_terrain: self.resolved_terrain.as_ref(),
                playfield_bounds: self.playfield_bounds,
                blocker_neighbor_counts: Some(&counts),
            },
            &self.terrain_costs,
            &blocks,
        );
        if let Some(lent) = borrowed {
            self.movement_pass_cache.give_back(owner, lent);
        }
        //4D3EAC restores Mark1 before inspecting the core result.
        self.foot_mark_put(id, Some(rules), fallback, registry);
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Find_Path core receiver")?;
        //4D3EB2..3ECA: +640 = (Frame, 0) on success and failure alike.
        actor.navigation.path_runtime.start_movement(frame, 0);
        use super::movement_path::MovePathFailure;
        use crate::sim::pathfinding::zone_search::PathSearchFailure as Search;
        match searched {
            Ok((path, _)) if path.len() < 2 => {
                //4D3E52..5F skips the copy for a zero-cost route; 4D4003
                //still records the current Cell.
                let current = (actor.position.rx as i16, actor.position.ry as i16);
                actor.navigation.path_replay.reference_cell = Some(current);
                Ok(Ok(false))
            }
            Ok((path, layers)) => {
                request.install_route(actor, path, layers);
                Ok(Ok(true))
            }
            Err(MovePathFailure::Search(
                Search::NativeEntryRejected | Search::CellSearchExhausted,
            )) => Ok(Err(CoreRefusal::Native)),
            Err(MovePathFailure::Search(Search::MissingHierarchyCell)) => {
                Ok(Err(CoreRefusal::VeraOnly("MissingHierarchyCell")))
            }
            Err(MovePathFailure::Search(Search::CompatibilityZoneRejected)) => {
                Ok(Err(CoreRefusal::VeraOnly("CompatibilityZoneRejected")))
            }
            Err(MovePathFailure::Search(Search::CompatibilityCorridorExhausted)) => {
                Ok(Err(CoreRefusal::VeraOnly("CompatibilityCorridorExhausted")))
            }
            Err(MovePathFailure::BridgeOnlyGoal) => {
                Ok(Err(CoreRefusal::VeraOnly("BridgeOnlyGoal")))
            }
            Err(MovePathFailure::MissingGrid) => {
                Err("Find_Path core requires PathGrid; no native failure inferred".into())
            }
        }
    }

    /// `Find_Path` failure continuation 0x4D404A..0x4D41F0 after the receiver:
    /// a target within one cell (Chebyshev over the `+9C` cell) returns unless
    /// the target cell is structural while the actor is off a bridge
    /// (0x4D40AB..0x4D40D4); otherwise Team detachment, `SetDestination(NULL,1)`
    /// (+0x480: Infantry 0x51AA40, Unit 0x741970), `Assign_Target(NULL)`
    /// (+0x3C8: Infantry 0x51B1F0, Unit Techno 0x6FCDB0), then the
    /// House 0x50B730 split: human -> `Queue_Mission(Guard=5, 0)` (0x4D41D6),
    /// nonhuman -> `Queue_Mission(AreaGuard=11, 0)` and, with GameMode != 0,
    /// `Find_Passable_Cell_Near_Unit` 0x500200 -> `SetDestination(cell, 1)`.
    pub(crate) fn finish_find_path_failure(
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
            //Production creates no TeamClass instance yet (every create_team
            //caller is a test), so this arm has no production reach.
            return Err(
                "Find_Path failed-path Team detachment (TeamClass::Remove_Member 0x6EA870) is not represented"
                    .into(),
            );
        }
        let owner = actor.owner();
        //0x4D413A: the class SetDestination(NULL, true): Infantry 0x51AA40 ->
        //Foot 0x4D94B0 -> Walk 0x75ADA0, or Unit 0x741970 -> Foot 0x4D94B0 ->
        //Drive 0x4AFE00 / Ship 0x69F510.
        if actor.category == EntityCategory::Unit {
            self.set_unit_null_destination(id, Some(rules));
        } else {
            self.set_walk_null_destination(id, Some(rules));
        }
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired failed Find_Path actor")?;
        //0x4D4149: the class target setter with NULL; the represented owner
        //serves Techno 0x6FCDB0 and Infantry 0x51B1F0 alike.
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
        //or neutral House Foot whose core search fails in skirmish.
        //Effect: no relocation walk. Frequency: rare without AI production.
        //Downstream: the House base projection (house_base.rs) is the pending
        //input of that owner.
        let _ = human;
        Ok(())
    }

    /// `Find_Path` 0x4D3944..0x4D3E0A before the core search: the target's
    /// `Can_Enter_Cell` answer (the class +0x1AC with facing -1, level -1,
    /// no source and flag 1, 0x4D3A6F..0x4D3A8C) selects a redirect. Code 6
    /// beyond CloseEnough (Rules+1718; a Team member reads Stray +171C or
    /// RelaxedStray +1720 through 0x6F03B0) asks FNPC for a cell near
    /// the target, accepts it only when it lies closer to the target than the
    /// actor (0x4D3C39) and `EstimateZoneCost` 0x42D170 admits it (at most
    /// Chebyshev plus 6), then `SetDestination(cell, 1)` retargets the search.
    /// Code 7 with a Building in the target cell (0x47C520) redirects
    /// unconditionally.
    fn find_path_admitted_goal(
        &mut self,
        id: u64,
        destination: DriveCoord,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<DriveCoord, String> {
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Find_Path goal requires map cells")?;
        let target = ((destination.x / 256) as i16, (destination.y / 256) as i16);
        let cell = terrain.native_cell_identity(target);
        let answer = self.foot_can_enter(id, cell, InfantryEntryArgs::REPAIR, rules, registry)?;
        self.find_path_goal_for_answer(id, destination, answer, rules)
    }

    /// The redirect selection of 0x4D3A92..0x4D3E0A for an already computed
    /// raw target `Can_Enter_Cell` code: 6 and 7 have arms, every other code
    /// keeps the target.
    pub(crate) fn find_path_goal_for_answer(
        &mut self,
        id: u64,
        destination: DriveCoord,
        answer: u8,
        rules: &RuleSet,
    ) -> Result<DriveCoord, String> {
        let target = ((destination.x / 256) as i16, (destination.y / 256) as i16);
        let cell = self
            .resolved_terrain
            .as_ref()
            .ok_or("Find_Path goal requires map cells")?
            .native_cell_identity(target);
        match answer {
            6 => {
                let actor = self
                    .substrate
                    .entities
                    .get(id)
                    .ok_or("retired Find_Path goal actor")?;
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
                //is set by no retail TechnoType (no IsTrain=yes in rulesmd).
                if distance <= rules.general.close_enough {
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
                self.redirect_find_path_destination(id, near, rules)?;
                Ok(cell_centre(near))
            }
            7 => {
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
                self.redirect_find_path_destination(id, near, rules)?;
                Ok(cell_centre(near))
            }
            _ => Ok(destination),
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
            .ok_or("Find_Path FNPC requires the mover type")?;
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
            .ok_or("EstimateZoneCost requires the mover type")?;
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
    /// 0x4D3DF9): the class's ordinary cell setter, which installs NavCom and
    /// the locomotor destination without a search of its own (Infantry
    /// 0x51AA40 -> Walk 0x75ACB0; Unit 0x741970 -> Drive 0x4AFD40 / Ship
    /// 0x69F450).
    fn redirect_find_path_destination(
        &mut self,
        id: u64,
        cell: (i16, i16),
        rules: &RuleSet,
    ) -> Result<(), String> {
        let move_info = self
            .resolve_move_info(id, Some(rules))
            .ok_or("Find_Path redirect requires the actor's move info")?;
        let target = (cell.0 as u16, cell.1 as u16);
        let timing = super::DestinationTiming::new(
            self.session.binary_frame,
            rules.general.blockage_path_delay_ticks,
        );
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Find_Path redirect actor")?;
        let track = actor
            .locomotor
            .as_ref()
            .is_some_and(|loco| matches!(loco.kind, LocomotorKind::Drive | LocomotorKind::Ship));
        if actor.category == EntityCategory::Unit && track {
            super::movement_commands::prepare_track_destination(
                actor,
                target,
                None,
                move_info.speed,
                self.resolved_terrain.as_ref(),
                timing,
            );
            return Ok(());
        }
        if !super::prepare_walk_cell_destination(
            &mut self.substrate.entities,
            id,
            target,
            move_info.speed,
            self.resolved_terrain.as_ref(),
            timing,
        ) {
            return Err(
                "Find_Path redirect destination was not accepted by the Walk setter".into(),
            );
        }
        Ok(())
    }

    /// Foot+320/4DA1D0, shared by class admission and the path-zone precheck.
    /// Read retained3D5/3D4 and effective mission; fresh bounds or locomotor
    /// state cannot reconstruct these inputs. TypeC94 IsTrain is absent from
    /// stock retail types and remains outside the represented rules contract.
    /// Original complete-call evidence: tools/spatial_oracle/unit_entry_boundary.
    pub(crate) fn foot_allows_outside_playfield(&self, id: u64) -> Result<bool, String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("missing Foot edge receiver")?;
        if !actor.in_playfield {
            return Ok(false);
        }
        if actor.is_mission_only() || actor.mission.effective().raw() == 4 {
            return Ok(true);
        }
        if let Some((team_id, _)) = self.team_script_vm.team_for_member(id) {
            let team = self
                .team_script_vm
                .team(team_id)
                .ok_or("missing attached Team")?;
            let script = self
                .team_script_vm
                .script(team.script_id())
                .ok_or("missing attached ScriptType")?;
            //6EC300 returns false for every invalid cursor/non-action3,
            //independently of unrepresented Team7F. It must not perform a
            //waypoint lookup on these exits. Do not infer7F from completion,
            //refusal, suspension or script presence.
            if script
                .actions
                .get(team.cursor() as u32 as usize)
                .is_some_and(|action| action.action_id == 3)
            {
                return Err("Foot edge admission requires retained Team7F and action3 waypoint state/effects".into());
            }
        }
        Ok(false)
    }

    /// Original Foot+2CC4D3810. Its MZ==-1 and Cell(0,0) exits precede
    /// navigation, Team, map and source-bridge queries.
    pub(crate) fn foot_path_zone_precheck(
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
        let source = self.foot_navigation_coordinate(id)?;
        let allow_destination_fringe = self.foot_allows_outside_playfield(id)?;
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
