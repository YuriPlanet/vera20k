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

use super::block_index::HeldBlockSets;
use super::ground_pose;
use super::infantry_entry::InfantryEntryArgs;
use super::movement_tick::FootPathRequest;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone};
use crate::rules::mission_data::MissionType;
use crate::rules::overlay_types::OverlayTypeRegistry;
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
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;

/// AStar429F54's live Foot +1AC receiver: Infantry 0x0051BF90 or Unit
/// 0x0073F0A0, the one class port behind `Simulation::foot_can_enter`, with
/// the mover's own facts resolved once per search. The fifth native argument
/// is unused by both receivers; direction, signed path height and previous
/// Cell reach the same decision owner used by Walk, Drive/Ship and bridge
/// repair. The raw code drives admission and cost at 0x00429F5A..FED.
struct FootSearchEntry<'a> {
    terrain: Option<&'a crate::map::resolved_terrain::ResolvedTerrainGrid>,
    receiver: Result<crate::sim::world::FootEntryReceiver<'a>, String>,
    threat_coefficient: crate::util::native_x87::NativeF64Bits,
    markers: super::path_markers::DeferredBridgeMarkerSearch<'a>,
}

impl crate::sim::pathfinding::SearchFootEntry for FootSearchEntry<'_> {
    fn classify(&self, query: crate::sim::pathfinding::SearchEntryQuery) -> Result<u8, String> {
        let terrain = self.terrain.ok_or("Foot search requires map cells")?;
        let receiver = self.receiver.as_ref().map_err(Clone::clone)?;
        let previous = query
            .from
            .map(|coord| terrain.native_cell_identity((coord.0 as i16, coord.1 as i16)));
        let candidate = match query.candidate {
            crate::sim::pathfinding::SearchEntryCandidate::CopiedCoord(coord) => {
                terrain.native_cell_identity((coord.0 as i16, coord.1 as i16))
            }
            crate::sim::pathfinding::SearchEntryCandidate::RetainedNativeCell(cell) => cell,
        };
        receiver.can_enter(
            candidate,
            InfantryEntryArgs {
                direction: query.direction,
                height: query.path_height,
                previous_cell: previous,
            },
        )
    }
    fn hierarchy_house_threat(&self, padded_index: i32) -> Result<i32, String> {
        self.receiver
            .as_ref()
            .map_err(Clone::clone)?
            .hierarchy_house_threat(padded_index)
    }
    fn prepare_search_markers(
        &self,
    ) -> Result<
        Option<(
            u8,
            std::cell::Ref<'_, crate::sim::pathfinding::SearchMarkerOverlay>,
        )>,
        String,
    > {
        Ok(Some(self.markers.prepare()))
    }
    fn prepared_search_markers(
        &self,
    ) -> Option<std::cell::Ref<'_, crate::sim::pathfinding::SearchMarkerOverlay>> {
        self.markers.prepared()
    }
    fn finishing_threat_coefficient(
        &self,
    ) -> Result<crate::util::native_x87::NativeF64Bits, String> {
        Ok(self.threat_coefficient)
    }
    fn finishing_house_threat(&self, coord: (i16, i16)) -> Result<i32, String> {
        let terrain = self.terrain.ok_or("Foot finishing requires map cells")?;
        //56BCD0 first resolves a Cell, including fixed-stride aliases and
        // shared Dummy replacement. Keep that observable query even when the
        // product cannot depend on House+59F0: signed-i32 * exact +/-0 is zero.
        if self.threat_coefficient.bits() & 0x7fff_ffff_ffff_ffff == 0 {
            let _ = terrain.native_cell_identity(coord);
            Ok(0)
        } else {
            self.receiver
                .as_ref()
                .map_err(Clone::clone)?
                .house_threat(coord)
        }
    }
}

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

pub(super) fn coord_cell(coord: DriveCoord) -> (i16, i16) {
    (
        crate::util::lepton::lepton_to_cell_packed(coord.x),
        crate::util::lepton::lepton_to_cell_packed(coord.y),
    )
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
    /// A route of at least one direction was copied into Foot+5E0. AStar
    /// never returns a zero-cost route: a goal in the start cell is NULL
    /// (0x429BF3..0x429C0A), so 0x4D3E52..0x4D3E5F always copies.
    Route,
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
    /// Shared Foot+68A sound guard read by Walk75B085 and the Drive/Ship
    /// failure callers. The constructor clears it; native Load can retain a
    /// nonzero byte. No gameplay arming writer is established. Original
    /// 750920 receives Rules ScoldSound, centered pan0x2000, volume1 and0.
    /// Clearing is caller-owned: Drive4B3AC9/Ship6A3118 can recurse before
    /// their later clear. Evidence: tools/spatial_oracle/foot_scold_latch.
    pub(crate) fn play_foot_path_scold(&mut self, id: u64, rules: &RuleSet) {
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        if actor.navigation.path_runtime.scold_latch_raw() != 0
            && let Some(sound_id) = rules.general.scold_sound.clone()
        {
            self.sound_events
                .push(crate::sim::world::SimSoundEvent::VocCentered { sound_id });
        }
    }

    /// Run a suspended Walk no-queue request (Walk75AFC5, continuation
    /// 0x75AFD3); `held` is the owner block sets the pending pass holds.
    /// Drive/Ship requests are made inside their Process_Movement
    /// (`track_fresh`).
    pub(crate) fn run_foot_path_request(
        &mut self,
        request: &FootPathRequest,
        held: Option<&mut HeldBlockSets>,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<FootPathOutcome, String> {
        self.run_walk_path_request(request, held, rules, registry, frame_effects)
            .map(|resumed| {
                if resumed {
                    FootPathOutcome::Resume
                } else {
                    FootPathOutcome::Returned
                }
            })
    }

    /// `Find_Path(cell, 0, 0)` for a no-queue caller whose own movement
    /// timer is already armed; see [`FindPathResult`]. `held`: the owner
    /// block sets of the movement pass making the request, if any.
    pub(crate) fn foot_find_path(
        &mut self,
        request: &FootPathRequest,
        held: Option<&mut HeldBlockSets>,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
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
            match path {
                SuppliedPath::FoundUnchanged => return Ok(FindPathResult::Route),
                SuppliedPath::Found(words) => {
                    queue.directions = words;
                    queue.cursor = 0;
                    return Ok(FindPathResult::Route);
                }
                SuppliedPath::Failed => return Ok(FindPathResult::Failed),
                SuppliedPath::CoreNull => super::fresh_oracle_seam::arm_core_null(),
            }
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
        let goal =
            self.find_path_admitted_goal(id, request.destination, rules, registry, frame_effects)?;
        match self.search_foot_path(request, held, goal, rules, registry, frame_effects)? {
            Ok(()) => Ok(FindPathResult::Route),
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
                self.run_find_path_failed_receiver(id, rules, registry, frame_effects)?;
                self.finish_find_path_failure(id, goal, rules, registry, frame_effects)?;
                Ok(FindPathResult::Failed)
            }
        }
    }

    /// `+0x500` at 0x4D4044, whose return value Find_Path ignores. Infantry
    /// 0x0051DAF0 performs Do_Action and the current-cell answer before Foot
    /// 0x4D55C0; the Unit's and the Aircraft's slot IS 0x4D55C0, which only
    /// calls the active locomotor's Stop_Moving (+0x48,
    /// [`Self::locomotor_stop_moving`]).
    pub(crate) fn run_find_path_failed_receiver(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<(), String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired failed-path receiver")?;
        match actor.category {
            EntityCategory::Infantry => {
                self.infantry_stop_driver(id, rules, registry, frame_effects)
            }
            EntityCategory::Unit | EntityCategory::Aircraft => {
                self.locomotor_stop_moving(id, Some(rules), registry, frame_effects)
            }
            EntityCategory::Structure => {
                Err("Find_Path +0x500 receiver for this class is not represented".into())
            }
        }
    }

    /// Mark0, the core search, Mark1 and the +640 rewrite; a found route is
    /// installed with its 0x4D4003 reference cell. Search refusals return
    /// through the inner `Err`; an unavailable PathGrid stops the frame.
    fn search_foot_path(
        &mut self,
        request: &FootPathRequest,
        held: Option<&mut HeldBlockSets>,
        goal: DriveCoord,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<Result<(), CoreRefusal>, String> {
        let id = request.entity_id;
        let frame = self.session.binary_frame;
        if self.path_grid.is_none() {
            return Err("Find_Path core requires PathGrid; no native failure inferred".into());
        }
        self.foot_mark_remove(id, Some(rules), registry, frame_effects);
        #[cfg(test)]
        self.export_bridge_engineer_entry_inputs(id, rules, "mark0");
        let owner = request.owner();
        //These occupancy-derived inputs must see Mark0, including the actor's
        //removal: the kept plane and owner sets follow it through the touch
        //log instead of a whole-world rebuild per request.
        let mut borrowed = None;
        let lent = match held.and_then(|held| held.get_mut(&owner)) {
            Some(lent) => {
                self.movement_pass_cache.refresh_lent_block_set(
                    owner,
                    lent,
                    &mut self.substrate.entities,
                    self.resolved_terrain.as_ref(),
                    &self.house_alliances,
                    &self.interner,
                    Some(rules),
                );
                lent
            }
            None => borrowed.insert(self.movement_pass_cache.lend_block_set(
                owner,
                &mut self.substrate.entities,
                self.resolved_terrain.as_ref(),
                &self.house_alliances,
                &self.interner,
                Some(rules),
            )),
        };
        let snapshot = self.path_grid_snapshot();
        let grid = snapshot.as_deref().expect("checked above");
        self.movement_pass_cache.blocker_plane(
            &mut self.substrate.entities,
            grid,
            self.resolved_terrain.as_ref(),
            self.overlay_grid.as_ref(),
            &self.interner,
            Some(rules),
        );
        let counts = self.movement_pass_cache.current_blocker_plane();
        //The track_fresh_response oracle substitutes the whole AStar core
        //0x4CBBA0 for a CoreNull row, so no +1AC call happens inside it; the
        //replay must not run the search that would consume those answers.
        #[cfg(test)]
        let core_null = super::fresh_oracle_seam::take_core_null();
        #[cfg(not(test))]
        let core_null = false;
        let searched = if core_null {
            Err(super::movement_path::MovePathFailure::Search(
                crate::sim::pathfinding::zone_search::PathSearchFailure::CellSearchExhausted,
            ))
        } else {
            // Original42ACF0 runs at429C1A after FootMark0 (4D3E11),
            // before reconstruction and both finishing passes. Reuse its one
            // marker owner against the live ordered peer lists/raw bytes here;
            // the returned overlay stays alive through the finishing passes
            // and is discarded on success or failure before FootMark1.
            let mover_peer = super::path_markers::bridge_marker_peer(
                &self.substrate.entities,
                id,
                Some(rules),
                &self.interner,
            );
            let marker_context = super::path_markers::DeferredBridgeMarker {
                mover_id: id,
                mover: mover_peer.as_ref(),
                rules: Some(rules),
                interner: &self.interner,
                grid,
                terrain: self.resolved_terrain.as_ref(),
                playfield_bounds: self.playfield_bounds,
            }
            .reading(
                &self.substrate.entities,
                &self.substrate.raw_cell_occupation,
            );
            let actor = self
                .substrate
                .entities
                .get(id)
                .expect("live path requester");
            let marker_search = marker_context.defer_search(
                &self.substrate.occupancy,
                id,
                (actor.position.rx, actor.position.ry),
                actor.body_facing.current(frame),
                actor.on_bridge,
                request.urgency,
            );
            let entry = self
                .substrate
                .entities
                .get(id)
                .filter(|actor| {
                    matches!(
                        actor.category,
                        EntityCategory::Infantry | EntityCategory::Unit
                    )
                })
                .map(|_| FootSearchEntry {
                    terrain: self.resolved_terrain.as_ref(),
                    markers: marker_search,
                    receiver: self.foot_entry_receiver(id, rules, registry),
                    threat_coefficient: self
                        .substrate
                        .entities
                        .get(id)
                        .expect("live Foot")
                        .navigation
                        .path_threat_coefficient_for_team(
                            self.team_script_vm.member_avoids_threats(id),
                        ),
                });
            request.search(
                goal,
                &self.substrate.entities,
                super::PathfindingContext {
                    path_grid: Some(grid),
                    zone_grid: self.zone_grid.as_ref(),
                    resolved_terrain: self.resolved_terrain.as_ref(),
                    playfield_bounds: self.playfield_bounds,
                    blocker_neighbor_counts: Some(counts),
                },
                &self.terrain_costs,
                &lent.sets,
                entry
                    .as_ref()
                    .map(|entry| entry as &dyn crate::sim::pathfinding::SearchFootEntry),
            )
        };
        if let Some(lent) = borrowed {
            self.movement_pass_cache.give_back(owner, lent);
        }
        //4D3EAC restores Mark1 before inspecting the core result.
        self.foot_mark_put(id, Some(rules), registry, frame_effects);
        if let Err(super::movement_path::MovePathFailure::Search(
            crate::sim::pathfinding::zone_search::PathSearchFailure::CellEntryUnavailable(cause),
        )) = &searched
        {
            return Err(cause.clone());
        }
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
            Ok(path) => {
                debug_assert!(path.len() >= 2, "AStar routes leave the start cell");
                request.install_route(actor, path);
                Ok(Ok(()))
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
            Err(MovePathFailure::BridgeOnlyGoal) => {
                Ok(Err(CoreRefusal::VeraOnly("BridgeOnlyGoal")))
            }
            Err(MovePathFailure::MissingGrid) => {
                Err("Find_Path core requires PathGrid; no native failure inferred".into())
            }
            Err(MovePathFailure::Search(Search::CellEntryUnavailable(cause))) => Err(cause),
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
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
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
        let owner = actor.owner();
        if self.team_script_vm.team_for_member(id).is_some() {
            //0x4D40DA..0x4D4134: the actor leaves its team (Remove_Member
            //0x6EA870, idle order included) between its locomotor's +B4 and
            //+B8, which clear and set Drive+65 (0x4B4BE0/0x4B4BF0), so the
            //idle mode's END test refuses a piggybacked Drive. Ship's pair
            //(0x6A4210/0x6A4220) writes its own +65, which VERA's Ship END
            //does not read; Walk's (0x4B6650/0x4B6660) are empty.
            let drive_end = |sim: &mut Self, permitted: bool| {
                if let Some(loco) = sim
                    .substrate
                    .entities
                    .get_mut(id)
                    .filter(|actor| {
                        actor
                            .locomotor
                            .as_ref()
                            .is_some_and(|loco| loco.active_kind() == LocomotorKind::Drive)
                    })
                    .and_then(|actor| actor.locomotor.as_mut())
                {
                    loco.store_drive_end_permission(permitted);
                }
            };
            drive_end(self, false);
            self.leave_team(id, false, Some(rules), frame_effects);
            drive_end(self, true);
        }
        //0x4D413A: the class SetDestination(NULL, true): Infantry 0x51AA40 ->
        //Foot 0x4D94B0 -> Walk 0x75ADA0, or Unit 0x741970 -> Foot 0x4D94B0 ->
        //Drive 0x4AFE00 / Ship 0x69F510.
        self.assign_null_destination(id, Some(rules), registry, frame_effects);
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired failed Find_Path actor")?;
        //0x4D4149: the class target setter with NULL; the represented owner
        //serves Techno 0x6FCDB0 and Infantry 0x51B1F0 alike.
        represented_assign_target(actor, None);
        let human = self.owner_is_human(owner);
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
        frame_effects: FrameEffects<'_>,
    ) -> Result<DriveCoord, String> {
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Find_Path goal requires map cells")?;
        let target = ((destination.x / 256) as i16, (destination.y / 256) as i16);
        let cell = terrain.native_cell_identity(target);
        let answer = self.foot_can_enter(id, cell, InfantryEntryArgs::REPAIR, rules, registry)?;
        self.find_path_goal_for_answer(id, destination, answer, rules, registry, frame_effects)
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
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
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
                //0x4D3A30..0x4D3A57: a team member keeps a target within its
                //team's stray (TeamClass::Get_Stray 0x6F03B0), anyone else
                //within CloseEnough.
                let keep_within = match self.team_script_vm.team_for_member(id) {
                    Some((team_id, _)) => self.team_stray(team_id, rules),
                    None => rules.general.close_enough,
                };
                //0x4D3944..0x4D3A2B: |+48 coordinate - target centre| with z.
                let coord = ground_pose::position_world_coord(&actor.position);
                let centre = cell_centre(target);
                let distance =
                    native_xyz_distance(coord.x - centre.x, coord.y - centre.y, coord.z - centre.z);
                //0x4D3A9B: dist <= that distance keeps the target. IsTrain (+C94)
                //is set by no retail TechnoType (no IsTrain=yes in rulesmd).
                if distance <= keep_within {
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
                // 4D3C54..4D3C9C reads target flags before nearby flags,
                // then passes those two copied booleans to EstimateZoneCost.
                let terrain = self.resolved_terrain.as_ref().unwrap();
                let cells = NativeCellQuery::canonical(terrain);
                let target_bridge = cells.flags(cells.lookup(target)) & 0x100 != 0;
                let near_bridge = cells.flags(cells.lookup(near)) & 0x100 != 0;
                let cost =
                    self.estimate_zone_cost(id, near, target, near_bridge, target_bridge, rules)?;
                let direct = (i32::from(near.0) - i32::from(target.0))
                    .abs()
                    .max((i32::from(near.1) - i32::from(target.1)).abs());
                if cost > direct + 6 {
                    return Ok(destination);
                }
                self.redirect_find_path_destination(id, near, rules, registry, frame_effects)?;
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
                self.redirect_find_path_destination(id, near, rules, registry, frame_effects)?;
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
        let required_zone_id = zones.get_zone_id_native(
            terrain,
            (current.0 as u16, current.1 as u16),
            zone,
            actor.on_bridge,
        );
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

    /// `PathfinderClass::EstimateZoneCost` 0x42D170, shared by Find_Path and
    /// Approach_Target. Both native callers pass a Foot and movement override
    /// -1, so its type supplies the movement row. The caller owns its limit:
    /// Find_Path uses Chebyshev+6; Approach uses Chebyshev+8.
    ///
    /// The two CellStruct arguments and bridge booleans are already copied
    /// inputs. The body retains both GetCell results before projecting either
    /// one through 583180, preserving real aliases and shared-Dummy ordering.
    /// Evidence: tools/spatial_oracle/fv_cell_attack/{pursuit,zone_cost}.json.
    /// No RNG, timer, destination or detach effect belongs to this function.
    pub(crate) fn estimate_zone_cost(
        &self,
        id: u64,
        start: (i16, i16),
        goal: (i16, i16),
        start_bridge: bool,
        goal_bridge: bool,
        rules: &RuleSet,
    ) -> Result<i32, String> {
        use crate::sim::cell_rect::get_cellclass_fallback;
        use crate::sim::pathfinding::zone_build::find_high_bridge_record_index;
        use crate::sim::pathfinding::zone_hierarchy::{
            ZonePrecheckExclusions, ZonePrecheckOutcome, ZonePrecheckThreat, zone_precheck_flat,
        };
        use crate::sim::pathfinding::zone_search::native_path_entry::live_hierarchy_projection;

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
        let zones = self
            .zone_grid
            .as_ref()
            .ok_or("EstimateZoneCost requires zone topology")?;
        let lookup = |cell: (i16, i16)| {
            get_cellclass_fallback(Some(terrain), i32::from(cell.0), i32::from(cell.1))
        };
        let start_cell = lookup(start); //42D1A1
        let goal_cell = lookup(goal); //42D1B2, before the first projection
        let start_projection = live_hierarchy_projection(
            terrain,
            zones.bridge_records(),
            &start_cell,
            start_bridge,
            self.playfield_bounds,
        )?;
        let goal_projection = live_hierarchy_projection(
            terrain,
            zones.bridge_records(),
            &goal_cell,
            goal_bridge,
            self.playfield_bounds,
        )?;
        let hierarchy = zones
            .hierarchy_for(object.movement_zone)
            .ok_or("EstimateZoneCost requires the native movement row")?;
        let start_zone = zones
            .hierarchy_zone_at_native(0, start_projection)
            .ok_or("EstimateZoneCost requires the projected source zone")?;
        let goal_zone = zones
            .hierarchy_zone_at_native(0, goal_projection)
            .ok_or("EstimateZoneCost requires the projected destination zone")?;
        // 42D182 clears this operation's exclusion vectors before precheck.
        let coefficient = actor
            .navigation
            .path_threat_coefficient_for_team(self.team_script_vm.member_avoids_threats(id));
        let threat_lookup = |index| {
            self.houses
                .get(&actor.owner())
                .ok_or("EstimateZoneCost threat requires a live House")?
                .spatial_threat_at_padded_index(index)
        };
        let ZonePrecheckOutcome::Passed(path) = zone_precheck_flat(
            hierarchy,
            start_zone,
            goal_zone,
            object.movement_zone,
            &ZonePrecheckExclusions::default(),
            Some(ZonePrecheckThreat::new(coefficient, &threat_lookup)),
        )?
        else {
            return Ok(i32::MAX);
        };
        let packed = |cell: (i16, i16)| (cell.0 as u16, cell.1 as u16);
        let signed = |cell: (u16, u16)| (cell.0 as i16, cell.1 as i16);
        let direct = chebyshev(start, goal);
        let fine_path = &path.paths[0];
        let count = fine_path.len();
        let mut cost = 2 * (count as i32 - 1);

        if goal_bridge {
            if start_bridge {
                // Both copied bridge booleans true: the same nonnegative
                // high-record index bypasses every bridge-distance term.
                let goal_record =
                    find_high_bridge_record_index(zones.bridge_records(), 0, packed(goal), 3);
                let start_record =
                    find_high_bridge_record_index(zones.bridge_records(), 0, packed(start), 3);
                if goal_record.is_some() && goal_record == start_record {
                    return Ok(direct);
                }
            }
            let mut adjacent = (0, 0);
            if count >= 4 {
                adjacent = zones.bridge_cell_for_hierarchy_zone(
                    terrain,
                    packed(goal),
                    0,
                    fine_path[count - 2],
                    self.playfield_bounds,
                )?;
            }
            if adjacent == (0, 0) {
                adjacent = zones.bridge_cell_for_hierarchy_zone(
                    terrain,
                    packed(goal),
                    0,
                    fine_path[count - 1],
                    self.playfield_bounds,
                )?;
            }
            if adjacent == (0, 0) {
                adjacent = goal_projection;
            }
            if adjacent != (0, 0) {
                cost += chebyshev(goal, signed(adjacent));
            }
        }
        if start_bridge {
            let mut adjacent = (0, 0);
            if count >= 4 {
                adjacent = zones.bridge_cell_for_hierarchy_zone(
                    terrain,
                    packed(start),
                    0,
                    fine_path[1],
                    self.playfield_bounds,
                )?;
            }
            if adjacent == (0, 0) {
                adjacent = zones.bridge_cell_for_hierarchy_zone(
                    terrain,
                    packed(start),
                    0,
                    fine_path[0],
                    self.playfield_bounds,
                )?;
            }
            if adjacent == (0, 0) {
                adjacent = start_projection;
            }
            if adjacent != (0, 0) {
                cost += chebyshev(start, signed(adjacent));
            }
        };
        Ok(cost.max(direct))
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
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<(), String> {
        let move_info = self
            .resolve_move_info(id, Some(rules))
            .ok_or("Find_Path redirect requires the actor's move info")?;
        let target = (cell.0 as u16, cell.1 as u16);
        let requested = crate::sim::components::NavTargetRef::cell(target.0, target.1);
        if self.infantry_setter_receiver(id, rules) {
            // Original4D3CC7/4D3DF9 call the same class+480 as an order.
            // Carry the already-borrowed registry through its current-cell
            // and Stop_Driver decisions instead of composing a second tail.
            let _ = self.assign_infantry_destination(
                id,
                requested,
                move_info.speed,
                rules,
                registry,
                frame_effects,
            )?;
            return Ok(());
        }
        if self.unit_setter_receiver(id, Some(rules)) {
            // These same virtual calls reach Unit741970(target,1), including
            // its same-NavCom guard and queue ownership. Reuse that owner;
            // composing only its Foot/Drive tail bypassed those decisions.
            // Native class controls: track_destination.json.
            let _ = self.set_unit_destination(id, requested, rules, true, frame_effects);
            return Ok(());
        }
        let timing = super::DestinationTiming::new(
            self.session.binary_frame,
            rules.general.blockage_path_delay_ticks,
        );
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Find_Path redirect actor")?;
        super::movement_commands::clear_destination_path_head(actor);
        let coord = super::navcom::target_cell_coord(
            target.0,
            target.1,
            self.resolved_terrain
                .as_ref()
                .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                .as_ref(),
        );
        if !super::prepare_walk_destination(
            &mut self.substrate.entities,
            id,
            (
                crate::sim::components::NavTargetRef::cell(target.0, target.1),
                coord,
            ),
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
        Ok(self.member_team_is_moving_outside_playfield(id))
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
        if coord_cell(destination) == (0, 0) {
            return Ok(false);
        }
        self.foot_can_reach_navigation_cell(id, destination, rules)
    }

    /// Shared Foot navigation/layer producers followed by Map56D100. The
    /// direct Walk75B163 caller has neither Foot4D3810's invalid-MZ bypass
    /// nor its null-cell rejection; those remain in the wrapper above.
    pub(super) fn foot_can_reach_navigation_cell(
        &self,
        id: u64,
        destination: DriveCoord,
        rules: &RuleSet,
    ) -> Result<bool, String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("missing Foot navigation reachability actor")?;
        let object = rules
            .object(self.interner.resolve(actor.type_ref()))
            .ok_or("missing Foot navigation reachability type")?;
        let destination = coord_cell(destination);
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
        assert_eq!(crate::util::lepton::lepton_to_cell_packed(2624), 10);
        assert_eq!(crate::util::lepton::lepton_to_cell_packed(255), 0);
        assert_eq!(crate::util::lepton::lepton_to_cell_packed(-1), 0);
        assert_eq!(crate::util::lepton::lepton_to_cell_packed(-256), -1);
        assert_eq!(crate::util::lepton::lepton_to_cell_packed(-257), -1);
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

#[cfg(test)]
#[path = "foot_path_marker_integration_tests.rs"]
mod marker_integration_tests;

#[cfg(test)]
#[path = "foot_path_threat_integration_tests.rs"]
mod threat_integration_tests;
