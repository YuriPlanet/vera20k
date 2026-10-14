//! Ground movement tick — the per-tick state machine for all ground/bridge entities.
//!
//! Prepares and resumes one production object visit: rotation, speed inputs
//! and fresh head selection.
//! Retained Drive/Ship tracks, including tracks without a MovementTarget,
//! execute through track_host. The batch entry points are test adapters.
//!
//! Pass preparation is separate from ordinary mover advancement: entry work
//! executes once and returns owned scheduling/cache state. Point callbacks need
//! their own continuation inside the movement call, without repeating entry work.
//! Geometry, admission and occupation remain with their dedicated modules.
//!
//! ## Dependency rules
//! - Internal to sim/movement — called via re-export in mod.rs.

use std::collections::BTreeMap;

use crate::map::entities::EntityCategory;
use crate::map::houses::HouseAllianceMap;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
use crate::sim::cell_rect::PlayfieldBounds;
use crate::sim::entity_store::EntityStore;
use crate::sim::infantry;
#[cfg(test)]
use crate::sim::lifecycle_request::LifecycleRequest;
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::terrain_cost::TerrainCostGrid;
#[cfg(test)]
use crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig;
#[cfg(test)]
use crate::sim::pathfinding::zone_map::ZoneGrid;
use crate::sim::rng::SimRng;
use crate::sim::type_handle_table::TypeHandleTable;

use super::block_index::{HeldBlockSets, LentOwnerBlockSet, OwnerBlockIndex};
use super::bump_crush;
use super::locomotor::MovementLayer;
use super::movement_path::supports_layered_bridge_pathing;
use super::movement_step;
use super::tube_movement;
use super::{MovementTickStats, MoverSnapshot, PathfindingContext};
use crate::sim::occupancy::{CellOccupationGrid, OccupancyGrid, RawCellOccupationGrid};

/// Build a read-only snapshot of the mover's properties before entering the
/// inner movement loop. This avoids repeated `entities.get()` calls and keeps
/// the data available across the mutable/immutable borrow boundary.
pub(super) fn snapshot_mover(
    entities: &EntityStore,
    entity_id: u64,
    playfield_bounds: Option<crate::sim::cell_rect::PlayfieldBounds>,
    type_handles: Option<&TypeHandleTable>,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> Option<MoverSnapshot> {
    let e = entities.get(entity_id)?;
    Some(MoverSnapshot {
        category: e.category,
        speed_type: e.locomotor.as_ref().map(|l| l.speed_type),
        movement_zone: e
            .locomotor
            .as_ref()
            .map(|l| l.movement_zone)
            .unwrap_or(MovementZone::Normal),
        omni_crusher: e.omni_crusher,
        regular_crusher: e.regular_crusher,
        owner: e.owner(),
        on_bridge: e.on_bridge,
        locomotor: e.locomotor.clone(),
        allow_zone_hierarchy: playfield_bounds.is_none() || e.in_playfield,
        slave_deposit_cells: if e.slave.owner().is_some() {
            crate::sim::slave_deposit::slave_deposit_cells(entities, entity_id, &|owner| {
                type_handles
                    .zip(rules)
                    .and_then(|(handles, rules)| {
                        handles
                            .handle_for(owner.type_ref())
                            .map(|h| rules.object_by_handle(h))
                    })
                    .map(|kind| crate::rules::foundation::foundation_dimensions(&kind.foundation))
            })
        } else {
            [None, None]
        },
    })
}

/// The map inputs a movement pass reads. Its searches run at the Simulation
/// (`FootPathRequest`), which brings its own search context current.
#[derive(Clone, Copy)]
struct PassGrids<'a> {
    path_grid: Option<&'a PathGrid>,
    resolved_terrain: Option<&'a ResolvedTerrainGrid>,
    playfield_bounds: Option<PlayfieldBounds>,
}

/// Owned one-time mover inputs retained across a synchronous Foot path request.
/// Resuming does not repeat the Process timer/preparation prefix or count a new
/// mover visit. These are stack continuation values, not another path owner.
struct OrdinaryMoverVisit {
    snap: MoverSnapshot,
    walk_position_before_step: Option<crate::sim::components::Position>,
    prone_crawls: Option<bool>,
    /// Walk75AEC0 argument1: the outer call may retry a refused head once.
    walk_retry_allowed: bool,
}

/// Synchronous Walk75B690 request. The pending pass releases the mutable
/// mover before its live Foot+1AC query and the class-specific response.
/// A clear answer finishes fresh-head selection in that same Process; a
/// recursive refusal reuses the retained visit through FootPathRequest.
pub(crate) struct WalkAdmissionRequest {
    pub(crate) entity_id: u64,
    visit: OrdinaryMoverVisit,
}

impl WalkAdmissionRequest {
    #[cfg(test)]
    pub(super) fn for_response_test(
        entities: &EntityStore,
        entity_id: u64,
        allow_retry: bool,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Option<Self> {
        let mut request = FootPathRequest::track(
            entities,
            entity_id,
            crate::sim::components::DriveCoord { x: 0, y: 0, z: 0 },
            0,
            None,
            None,
            Some(rules),
        )?;
        request.visit.walk_retry_allowed = allow_retry;
        Some(Self {
            entity_id,
            visit: request.visit,
        })
    }

    pub(super) fn allows_retry(&self) -> bool {
        self.visit.walk_retry_allowed
    }

    pub(super) fn into_path_request(
        self,
        destination: crate::sim::components::DriveCoord,
        urgency: u8,
    ) -> FootPathRequest {
        let mut visit = self.visit;
        visit.walk_retry_allowed = false;
        FootPathRequest {
            entity_id: self.entity_id,
            destination,
            urgency,
            visit,
        }
    }
}

pub(crate) struct FootPathRequest {
    pub(crate) entity_id: u64,
    pub(crate) destination: crate::sim::components::DriveCoord,
    /// Find_Path's third argument: 0 for an ordinary request, 1 or 2 from the
    /// Drive/Ship code-2 ladder (0x4B3A0E), read by the edge cost (+3C).
    pub(crate) urgency: u8,
    visit: OrdinaryMoverVisit,
}

/// The retained no-queue request of a live Walk Foot with no paid head, whose
/// Foot+5E0 head is empty (native -1). Drive/Ship requests are made inside
/// their Process_Movement (`track_fresh`).
fn no_queue_path_request(
    entity: &crate::sim::game_entity::GameEntity,
) -> Option<crate::sim::components::DriveCoord> {
    if !entity
        .navigation
        .path_replay
        .remaining_directions()
        .is_empty()
    {
        return None;
    }
    let loco = entity.locomotor.as_ref()?;
    match loco.kind {
        LocomotorKind::Walk if loco.step_head().is_none() => loco.walk_destination(),
        _ => None,
    }
}

impl FootPathRequest {
    /// Resume the retained recursive visit in the decoder corpus after its
    ///supplied Find_Path. Production resumes through PendingMovementPass.
    #[cfg(test)]
    pub(crate) fn into_walk_admission_for_test(self) -> WalkAdmissionRequest {
        WalkAdmissionRequest {
            entity_id: self.entity_id,
            visit: self.visit,
        }
    }

    /// A Drive/Ship `Find_Path(cell, append, urgency)` issued inside
    /// Process_Movement (`track_fresh`): the no-queue arm, the code-2 ladder
    /// and the last-word extension.
    pub(crate) fn track(
        entities: &EntityStore,
        entity_id: u64,
        destination: crate::sim::components::DriveCoord,
        urgency: u8,
        playfield_bounds: Option<PlayfieldBounds>,
        type_handles: Option<&TypeHandleTable>,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) -> Option<Self> {
        let snap = snapshot_mover(entities, entity_id, playfield_bounds, type_handles, rules)?;
        Some(Self {
            entity_id,
            destination,
            urgency,
            visit: OrdinaryMoverVisit {
                snap,
                walk_position_before_step: None,
                prone_crawls: None,
                walk_retry_allowed: true,
            },
        })
    }

    /// The Simulation wrapper calls this only between its real Mark0/Mark1.
    /// `blocks` and the context's blocker plane are brought current after the
    /// actor left its cell; this reuses the single existing search.
    pub(super) fn search(
        &self,
        goal: crate::sim::components::DriveCoord,
        entities: &EntityStore,
        ctx: PathfindingContext<'_>,
        terrain_costs: &BTreeMap<SpeedType, TerrainCostGrid>,
        blocks: &super::block_index::OwnerBlockSet,
        foot_entry: Option<&dyn crate::sim::pathfinding::SearchFootEntry>,
    ) -> Result<Vec<(u16, u16)>, super::movement_path::MovePathFailure> {
        let snap = &self.visit.snap;
        let actor = entities.get(self.entity_id).expect("live suspended mover");
        let start = (actor.position.rx, actor.position.ry);
        // AStar 0x00429A90 takes the start height from the Foot's OnBridge
        // byte (+0x8C): the cell level, +4 on a bridge. No path layer.
        let layer = if actor.on_bridge {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        let goal = ((goal.x / 256) as u16, (goal.y / 256) as u16);
        let layered = snap
            .locomotor
            .as_ref()
            .zip(ctx.path_grid)
            .is_some_and(|(loco, grid)| {
                supports_layered_bridge_pathing(loco, grid, snap.on_bridge)
            });
        debug_assert!(
            ctx.blocker_neighbor_counts.is_some() == ctx.path_grid.is_some(),
            "path build on a pass that skipped the blocker plane; see pass_may_build_paths"
        );
        super::movement_path::find_move_path_with_marker_detailed(
            ctx,
            layered,
            start,
            layer,
            goal,
            snap.speed_type.and_then(|speed| terrain_costs.get(&speed)),
            Some(&blocks.0),
            Some(&blocks.0),
            Some(&blocks.0),
            snap.movement_zone,
            Some(snap.movement_zone),
            Some(&blocks.1),
            None,
            //The admitted search prepares markers and applies native42AECA
            //urgency downgrade through the live Foot provider.
            //One crush authority for every search; see `CrushCapability::of`.
            super::MoverPathFacts::from_snapshot(snap, self.urgency),
            snap.allow_zone_hierarchy,
            foot_entry,
        )
        // Foot+5E0 words carry no layer; the Process reads OnBridge.
        .map(|(path, _)| path)
    }

    pub(crate) fn owner(&self) -> crate::sim::intern::InternedId {
        self.visit.snap.owner
    }

    /// FootFindPath4D3920's success tail: the found route becomes the
    /// Foot+5E0 direction queue, and 4D4003 records the current Cell as its
    /// reference. Foot timer/latch/retry state has its own lifetime.
    ///
    /// Every Foot reads only that queue; the order adapter keeps no cells.
    pub(super) fn install_route(
        &self,
        actor: &mut crate::sim::game_entity::GameEntity,
        path: Vec<(u16, u16)>,
    ) {
        let current = (actor.position.rx, actor.position.ry);
        super::path_markers::install_path_replay(
            &mut actor.navigation.path_replay,
            current,
            &path,
            1,
        );
    }
}

/// Effects accumulated until the pass tail. Keeping these together preserves
/// crushed-victim exclusions and scatter deduplication across mover visits.
#[derive(Default)]
struct MovementPassEffects {
    stats: MovementTickStats,
    finished_entities: Vec<u64>,
    scatters: super::scatter::ScatterRequests,
    native_track: Option<super::track_process::TrackInvocation>,
    walk_per_cell: Option<(u64, crate::sim::components::DriveCoord)>,
    walk_boundary: Option<(u64, crate::sim::components::DriveCoord)>,
    foot_path_request: Option<FootPathRequest>,
    walk_admission_request: Option<WalkAdmissionRequest>,
    /// A Drive/Ship Unit's Process_Movement(&out, 1, 0), which the host runs
    /// at the Simulation (`track_fresh`).
    track_movement: Option<(u64, super::track_process::TrackFamily)>,
}

/// A Drive/Ship Unit: its Process_Movement is `track_fresh`.
fn native_track_route(
    entity: &crate::sim::game_entity::GameEntity,
) -> Option<super::track_process::TrackFamily> {
    if entity.category != EntityCategory::Unit {
        return None;
    }
    match entity.locomotor.as_ref()?.kind {
        LocomotorKind::Drive => Some(super::track_process::TrackFamily::Drive),
        LocomotorKind::Ship => Some(super::track_process::TrackFamily::Ship),
        _ => None,
    }
}

/// Where a mover visit enters [`advance_ordinary_mover`].
enum VisitEntry {
    /// The object turn's Process: the active-track dispatch (Drive 0x4B055A /
    /// Ship 0x69FC6A), else its Process_Movement.
    Process,
    /// Process_Movement(&out, 1, 0) again in the same Process after
    /// Process_Track(0) ended the track (Drive 0x4B0647 / Ship 0x69FCEE):
    /// no active-track dispatch and no new mover visit.
    AfterTrackEnd,
    /// Below the no-queue request, after Find_Path resumed head selection.
    FootPathResume(Box<OrdinaryMoverVisit>),
}

/// Run one ordinary mover visit. An early return ends this visit, including
/// its deferred-effect tail, exactly as the former outer-loop continue did.
/// Track callback suspension will resume below the one-time mover preparation,
/// rather than invoke this complete visit again.
fn advance_ordinary_mover(
    entities: &mut EntityStore,
    entity_id: u64,
    grids: PassGrids<'_>,
    occupancy: &mut OccupancyGrid,
    cell_occupation: &mut CellOccupationGrid,
    raw_cell_occupation: &mut RawCellOccupationGrid,
    native_frame: u32,
    interner: &mut crate::sim::intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    type_handles: Option<&TypeHandleTable>,
    houses: &std::collections::BTreeMap<
        crate::sim::intern::InternedId,
        crate::sim::house_state::HouseState,
    >,
    effects: &mut MovementPassEffects,
    entry: VisitEntry,
) {
    let PassGrids {
        path_grid,
        resolved_terrain,
        playfield_bounds,
    } = grids;
    let MovementPassEffects {
        stats,
        finished_entities,
        native_track,
        walk_per_cell,
        walk_boundary,
        foot_path_request,
        walk_admission_request,
        track_movement,
        ..
    } = effects;
    if matches!(entry, VisitEntry::Process) {
        let continuation = entities
            .get(entity_id)
            .is_some_and(|entity| super::track_head::active_track_family(entity).is_some());
        if continuation {
            // Drive4B055A..0576 dispatches an active descriptor directly to
            // Process_Track. A MovementTarget or exhausted route cannot route
            // it back through fresh selection/rotation and strand its cursor.
            let entity = entities.get(entity_id).expect("live retained track");
            *native_track = Some(super::track_process::TrackInvocation {
                entity_id,
                family: if entity
                    .locomotor
                    .as_ref()
                    .is_some_and(|l| l.kind == LocomotorKind::Ship)
                {
                    super::track_process::TrackFamily::Ship
                } else {
                    super::track_process::TrackFamily::Drive
                },
                apply_fresh_occupation: false,
                active_gate: true,
                retry: false,
            });
            stats.movers_total = stats.movers_total.saturating_add(1);
            return;
        }
    }
    let resumed_path_request = matches!(entry, VisitEntry::FootPathResume(_));
    let counts_visit = matches!(entry, VisitEntry::Process);
    let visit = if let VisitEntry::FootPathResume(visit) = entry {
        *visit
    } else {
        if counts_visit {
            stats.movers_total = stats.movers_total.saturating_add(1);
        }
        // Drive4B0A79 / Ship6A0142 (and the track-end continuation 4B0647):
        // Process_Movement runs at the Simulation; see `track_fresh`.
        if let Some(family) = entities.get(entity_id).and_then(native_track_route) {
            debug_assert!(track_movement.is_none(), "scoped Process has one request");
            *track_movement = Some((entity_id, family));
            return;
        }
        // The rest of this visit is the Walk Process.
        if !entities
            .get(entity_id)
            .and_then(|entity| entity.locomotor.as_ref())
            .is_some_and(|locomotor| locomotor.kind == LocomotorKind::Walk)
        {
            return;
        }

        // Snapshot mover data before entering the inner loop so we can release the
        // mutable borrow on `entities` when needed for crush/bump immutable lookups.
        let Some(snap) = snapshot_mover(entities, entity_id, playfield_bounds, type_handles, rules)
        else {
            return;
        };
        // Walk tests CanEnter at 0x75B690 before its paid SetCoords calls
        // (0x75BDC0/0x75C12E). A refused prospective step keeps exact XY.
        let walk_position_before_step = snap
            .locomotor
            .as_ref()
            .filter(|loco| loco.kind == crate::rules::locomotor_type::LocomotorKind::Walk)
            .and_then(|_| entities.get(entity_id).map(|entity| entity.position));
        let prone_crawls = entities.get(entity_id).and_then(|entity| {
            if !infantry::is_prone_for_damage(entity) {
                return None;
            }
            let rules = rules?;
            let obj = rules.object(interner.resolve(entity.type_ref()))?;
            Some(obj.crawls)
        });
        OrdinaryMoverVisit {
            snap,
            walk_position_before_step,
            prone_crawls,
            walk_retry_allowed: true,
        }
    };
    if !resumed_path_request
        && let Some(entity) = entities.get_mut(entity_id)
        && entity
            .locomotor
            .as_ref()
            .is_some_and(|l| l.step_head().is_none() && l.walk_destination().is_none())
    {
        //Walk75AEC3..75AF10: no head and no destination is the idle tail
        //(75BCE3); the empty route adapter retires with it.
        super::walk_step::finish_idle(entity);
        finished_entities.push(entity_id);
        return;
    }
    if !resumed_path_request
        && let Some(destination) = entities.get(entity_id).and_then(no_queue_path_request)
    {
        let entity = entities.get(entity_id).expect("same mover request");
        //Walk75AF3C..55 observes a frame-anchored exact-zero remainder. No
        //search, debt, occupancy preparation or second mover visit on this wait.
        if !entity
            .navigation
            .path_runtime
            .movement_timer
            .expired(native_frame as i32)
        {
            return;
        }
        debug_assert!(
            foot_path_request.is_none(),
            "scoped Process has one path request"
        );
        *foot_path_request = Some(FootPathRequest {
            entity_id,
            destination,
            urgency: 0,
            visit,
        });
        return;
    }
    let OrdinaryMoverVisit {
        snap,
        walk_position_before_step,
        prone_crawls,
        walk_retry_allowed,
    } = visit;
    // The mover is lifted out of the store for each scope below; the Walk head
    // preparation between them needs the whole store.
    {
        let Some(mut turn) = entities.take_turn(entity_id) else {
            return;
        };
        let entity = turn.entity();
        if entity
            .locomotor
            .as_ref()
            .is_some_and(|l| l.step_head().is_none())
        {
            if let Some(tube_id) = tube_movement::pending_path_tube_id(
                &entity.navigation.path_replay,
                &entity.position,
                entity.movement_layer_or_ground(),
                resolved_terrain,
            ) {
                let terrain = resolved_terrain.expect("tube admission resolved terrain");
                if tube_movement::begin_path_tube_step(
                    &mut entity.foot_occupation_enabled,
                    &mut entity.navigation.path_replay,
                    entity_id,
                    entity.category,
                    &mut entity.position,
                    &mut entity.locomotor,
                    &mut entity.low_bridge_tube_state,
                    &mut entity.lifecycle.cell_marked,
                    tube_id,
                    terrain,
                    occupancy,
                    cell_occupation,
                    raw_cell_occupation,
                )
                .is_ok()
                {
                    return;
                }
            }
            //75B690 requires the whole live Foot owner. Suspending here
            //releases this entity's mutable borrow before any admission
            //query. Canonical Clear goes straight to75C240, never through
            //the grid/cliff/occupancy adapters below.
            debug_assert!(walk_admission_request.is_none());
            *walk_admission_request = Some(WalkAdmissionRequest {
                entity_id,
                visit: OrdinaryMoverVisit {
                    snap,
                    walk_position_before_step,
                    prone_crawls,
                    walk_retry_allowed,
                },
            });
            return;
        }
    } // Release the admission borrow before live head/priority queries.
    let Some(mut turn) = entities.take_turn(entity_id) else {
        return;
    };
    let entity = turn.entity();

    // Steering / rotation: rotate in place, then move. ROT=0 means an
    // instant turn.
    if snap.category != EntityCategory::Infantry {
        match movement_step::handle_vehicle_rotation(&entity.body_facing, native_frame) {
            movement_step::RotationResult::StillRotating => return,
            movement_step::RotationResult::ReadyToMove => {}
        }
    }

    if let Some(loco) = entity.locomotor.as_mut() {
        loco.begin_walk_motion();
    }
    if let Some(head) = movement_step::completed_walk_head(&entity.position, &entity.locomotor) {
        *walk_per_cell = Some((entity_id, head));
        return;
    }
    let object = rules.and_then(|r| r.object(interner.resolve(entity.type_ref())));
    let adjusted_speed = super::foot_speed::adjusted_speed(
        entity,
        object,
        rules.map_or(1.0, |r| r.general.veteran_speed),
        houses,
    );
    super::walk_step::advance(
        entity,
        adjusted_speed,
        prone_crawls,
        native_frame,
        resolved_terrain,
        path_grid,
    );
    if let Some(coord) = movement_step::walk_boundary_crossing(&entity.position) {
        entity.position = *walk_position_before_step
            .as_ref()
            .expect("only Walk can suspend a boundary");
        *walk_boundary = Some((entity_id, coord));
        return;
    }
    // Walk clears the blocked latch when it makes a paid coordinate
    // step (0x75BFCD), not merely when FindPath succeeds.
    if let Some(before) = walk_position_before_step.as_ref()
        && super::ground_pose::position_world_xy(before)
            != super::ground_pose::position_world_xy(&entity.position)
    {
        entity.navigation.path_runtime.path_blocked = false;
    }
}

/// Derived movement inputs that survive across object turns.
///
/// The base contains retained wall/Foot bytes and derived terrain occupation.
/// Wall/terrain edits rebuild under their epochs. Foot writes replay the owner's
/// bounded cell-delta journal, rebuilding only if the reader fell behind. The
/// entity touch log maintains building contributions (and, in fixtures without an overlay grid, mobiles).
/// Debug checks compare against a fresh sum. No cache data is saved or hashed.
#[derive(Default)]
pub(crate) struct MovementPassCache {
    blocker: BlockerPlaneCache,
    /// Each owner's pathfinding block sets, kept current from the entity
    /// store's touch log instead of rebuilt from every entity per turn.
    block_index: OwnerBlockIndex,
}

#[derive(Default)]
struct BlockerPlaneCache {
    entry: Option<BlockerPlaneEntry>,
    /// How often the plane was rebuilt from the whole map and every entity.
    #[cfg(test)]
    world_rebuilds: usize,
}

struct BlockerPlaneEntry {
    key: BlockerPlaneKey,
    plane: crate::sim::pathfinding::BlockerNeighborCounts,
    sources: BTreeMap<u64, bump_crush::BlockerPlaneSource>,
    foot_revision: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct BlockerPlaneKey {
    terrain_epoch: Option<u64>,
    overlay_epoch: Option<u64>,
    width: u16,
    height: u16,
    /// A building's foundation size comes from its type. By address, never
    /// dereferenced.
    rules: Option<usize>,
}

impl MovementPassCache {
    /// Read the plane after `blocker_plane` brought it current. Splitting the
    /// refresh from this borrow lets the live Foot query read the Simulation
    /// without cloning the plane or owning a second admission cache.
    pub(crate) fn current_blocker_plane(&self) -> &crate::sim::pathfinding::BlockerNeighborCounts {
        &self
            .blocker
            .entry
            .as_ref()
            .expect("blocker plane refreshed before search")
            .plane
    }

    /// How often the block index had to read every entity: once at the start,
    /// and again only when something hands out the whole store mutably.
    #[cfg(test)]
    pub(crate) fn block_index_world_rebuilds(&self) -> usize {
        self.block_index.world_rebuilds
    }

    /// How often an owner's sets were built from every placement instead of
    /// brought current.
    #[cfg(test)]
    pub(crate) fn block_index_view_builds(&self) -> usize {
        self.block_index.view_builds
    }

    /// How often the blocker plane was rebuilt from the whole map and every
    /// entity.
    #[cfg(test)]
    pub(crate) fn blocker_plane_world_rebuilds(&self) -> usize {
        self.blocker.world_rebuilds
    }

    /// The kept plane brought current through the touch log. The synchronous
    /// Foot path request reads it between its Mark(0) and Mark(1), so the
    /// requester's own removal is included without a whole-world rebuild.
    pub(crate) fn blocker_plane(
        &mut self,
        entities: &mut EntityStore,
        grid: &PathGrid,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        overlay_grid: Option<&crate::sim::overlay_grid::OverlayGrid>,
        interner: &crate::sim::intern::StringInterner,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) -> &crate::sim::pathfinding::BlockerNeighborCounts {
        let touched = self.block_index.take_forwarded(entities);
        Self::blocker_plane_in(
            &mut self.blocker,
            touched,
            entities,
            grid,
            resolved_terrain,
            overlay_grid,
            interner,
            rules,
        )
    }

    /// Bring owner sets a pass holds current, e.g. after a synchronous Mark(0).
    pub(crate) fn refresh_lent_block_set(
        &mut self,
        owner: crate::sim::intern::InternedId,
        lent: &mut LentOwnerBlockSet,
        entities: &mut EntityStore,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        alliances: &HouseAllianceMap,
        interner: &crate::sim::intern::StringInterner,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) {
        self.block_index.refresh_lent(
            owner,
            lent,
            entities,
            resolved_terrain,
            alliances,
            interner,
            rules,
        );
    }

    /// Owner sets for a search whose pass holds none for this owner, or that
    /// runs outside any pass; [`Self::give_back`] returns them.
    pub(crate) fn lend_block_set(
        &mut self,
        owner: crate::sim::intern::InternedId,
        entities: &mut EntityStore,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        alliances: &HouseAllianceMap,
        interner: &crate::sim::intern::StringInterner,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) -> LentOwnerBlockSet {
        self.block_index.lend_current(
            owner,
            entities,
            resolved_terrain,
            alliances,
            interner,
            rules,
        )
    }

    pub(crate) fn give_back(
        &mut self,
        owner: crate::sim::intern::InternedId,
        lent: LentOwnerBlockSet,
    ) {
        self.block_index.give_back(owner, lent);
    }

    /// On the blocker field alone, so a pass can hold the plane while it
    /// updates the block index beside it.
    fn blocker_plane_in<'a>(
        blocker: &'a mut BlockerPlaneCache,
        touched: super::block_index::TouchedBacklog,
        entities: &EntityStore,
        grid: &PathGrid,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        overlay_grid: Option<&crate::sim::overlay_grid::OverlayGrid>,
        interner: &crate::sim::intern::StringInterner,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) -> &'a crate::sim::pathfinding::BlockerNeighborCounts {
        let key = BlockerPlaneKey {
            terrain_epoch: resolved_terrain.map(ResolvedTerrainGrid::mutation_epoch),
            overlay_epoch: overlay_grid.map(|grid| grid.blocker_plane_epoch()),
            width: grid.width(),
            height: grid.height(),
            rules: rules.map(|rules| std::ptr::from_ref(rules) as usize),
        };
        let retained_foot = overlay_grid.is_some();
        match blocker.entry.as_mut().filter(|entry| {
            entry.key == key
                && !touched.everything
                && overlay_grid.is_none_or(|grid| {
                    grid.foot_neighbor_changes_since(entry.foot_revision)
                        .is_some()
                })
        }) {
            Some(entry) => {
                if let Some(grid) = overlay_grid {
                    for (index, delta) in grid
                        .foot_neighbor_changes_since(entry.foot_revision)
                        .unwrap()
                    {
                        entry.plane.apply_retained_delta(index, delta);
                    }
                    entry.foot_revision = grid.foot_neighbor_revision();
                }
                for id in touched.ids {
                    let now = entities.get(id).and_then(|entity| {
                        bump_crush::blocker_plane_source(
                            entity,
                            resolved_terrain,
                            interner,
                            rules,
                            retained_foot,
                        )
                    });
                    if entry.sources.get(&id) == now.as_ref() {
                        continue;
                    }
                    if let Some(old) = entry.sources.remove(&id) {
                        old.remove_from(&mut entry.plane);
                    }
                    if let Some(now) = now {
                        now.add_to(&mut entry.plane);
                        entry.sources.insert(id, now);
                    }
                }
            }
            None => {
                let mut plane = bump_crush::blocker_plane_base(
                    grid.width(),
                    grid.height(),
                    resolved_terrain,
                    overlay_grid,
                );
                let mut sources = BTreeMap::new();
                for entity in entities.values() {
                    if let Some(source) = bump_crush::blocker_plane_source(
                        entity,
                        resolved_terrain,
                        interner,
                        rules,
                        retained_foot,
                    ) {
                        source.add_to(&mut plane);
                        sources.insert(entity.stable_id(), source);
                    }
                }
                blocker.entry = Some(BlockerPlaneEntry {
                    key,
                    plane,
                    sources,
                    foot_revision: overlay_grid.map_or(0, |grid| grid.foot_neighbor_revision()),
                });
                #[cfg(test)]
                {
                    blocker.world_rebuilds += 1;
                }
            }
        }
        let plane = &blocker
            .entry
            .as_ref()
            .expect("plane was just ensured")
            .plane;
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled()
                || *plane
                    == bump_crush::build_blocker_neighbor_counts_with_overlays(
                        entities,
                        grid.width(),
                        grid.height(),
                        resolved_terrain,
                        overlay_grid,
                        interner,
                        rules,
                    ),
            "the kept blocker plane diverged from a build of the whole world"
        );
        plane
    }
}

/// The visit's entry work, run once before the ordinary mover advances.
/// Resuming after a world callback must not repeat it: it runs Tube movement
/// and the idle Walk tail. Returns the owner of an object that still advances
/// as an ordinary mover.
fn prepare_movement_visit(
    entities: &mut EntityStore,
    entity_id: u64,
    grids: PassGrids<'_>,
    occupancy: &mut OccupancyGrid,
    cell_occupation: &mut CellOccupationGrid,
    raw_cell_occupation: &mut RawCellOccupationGrid,
    rng: &mut SimRng,
    native_frame: u32,
    interner: &crate::sim::intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    stats: &mut MovementTickStats,
    scatters: &mut super::scatter::ScatterRequests,
) -> Option<crate::sim::intern::InternedId> {
    let entity = entities.get_mut(entity_id)?;
    cell_occupation.reconcile_entity(entity, occupancy);
    // Active TubeMovement owns the entire object turn, including the visit in
    // which its final clears the payload.
    if entity.low_bridge_tube_state.is_some() {
        if let Some(terrain) = grids.resolved_terrain
            && tube_movement::tick_active_tube_object(
                entities,
                entity_id,
                terrain,
                grids.path_grid,
                occupancy,
                cell_occupation,
                raw_cell_occupation,
                rules,
                interner,
                rng,
                native_frame,
                scatters,
            )
        {
            stats.movers_total = stats.movers_total.saturating_add(1);
        }
        return None;
    }
    // Walk75AEC0 is the Process of every live Foot (FootClass::AI
    // 0x004DA806..0x004DA877) and branches on its own head (+0x28) and
    // destination (+0x1C): either one runs its step, neither the idle tail
    // 75BCE3. An order adapter does not decide it, so a caller that drops
    // the adapter beside a null setter (owner change, parasite release,
    // Temporal freeze) still finishes the paid head.
    let walk_route = entity.locomotor.as_ref().is_some_and(|loco| {
        loco.kind == LocomotorKind::Walk
            && (loco.step_head().is_some() || loco.walk_destination().is_some())
    });
    if entity.is_active() && entity.movement_target.is_none() && !walk_route {
        super::walk_step::finish_idle(entity);
    }
    // A Drive or Ship got here through the outer Process gates
    // (`track_turn`: Drive 0x4B08D1..0x4B0A69 / Ship 0x69FF98..0x6A0131),
    // which admit Process_Movement (0x4B0A79 / 0x6A0142) for a moving class
    // or a live path word whatever adapter it has: a Chrono Warp's
    // Force_Track leaves +34 in the Unit's own cell with none.
    let track_route = entity
        .locomotor
        .as_ref()
        .is_some_and(|loco| super::track_process::TrackFamily::from_kind(loco.kind).is_some());
    if entity.movement_target.is_none()
        && !walk_route
        && !track_route
        && super::track_head::active_track_family(entity).is_none()
    {
        return None;
    }
    let layer = entity.movement_layer_or_ground();
    (!matches!(layer, MovementLayer::Air | MovementLayer::Underground)).then(|| entity.owner())
}

#[cfg(test)]
pub(crate) fn tick_movement_with_grids(
    entities: &mut EntityStore,
    live_order: Option<&[u64]>,
    path_grid: Option<&PathGrid>,
    terrain_costs: &BTreeMap<SpeedType, TerrainCostGrid>,
    alliances: &HouseAllianceMap,
    occupancy: &mut OccupancyGrid,
    cell_occupation: &mut CellOccupationGrid,
    raw_cell_occupation: &mut RawCellOccupationGrid,
    rng: &mut SimRng,
    sim_tick: u64,
    native_frame: u32,
    zone_grid: Option<&ZoneGrid>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    playfield_bounds: Option<PlayfieldBounds>,
    terrain_speed_config: &TerrainSpeedConfig,
    interner: &mut crate::sim::intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    sound_events: &mut Vec<crate::sim::world::SimSoundEvent>,
    lifecycle_requests: &mut Vec<LifecycleRequest>,
) -> MovementTickStats {
    // This adapter owns only fixture assembly and result transfer. Runtime
    // state is moved, not mirrored; every entity reaches the production host.
    let mut sim = crate::sim::world::Simulation::new();
    sim.substrate.entities = std::mem::take(entities);
    sim.substrate.occupancy = std::mem::take(occupancy);
    sim.substrate.cell_occupation = std::mem::take(cell_occupation);
    sim.substrate.raw_cell_occupation = std::mem::take(raw_cell_occupation);
    sim.interner = std::mem::take(interner);
    sim.scenario_rng = rng.clone();
    sim.session.tick = sim_tick;
    sim.session.binary_frame = native_frame;
    sim.terrain_costs = terrain_costs.clone();
    sim.house_alliances = alliances.clone();
    sim.path_grid = path_grid.cloned().map(std::sync::Arc::new);
    sim.zone_grid = zone_grid.cloned();
    sim.resolved_terrain = resolved_terrain.cloned();
    sim.playfield_bounds = playfield_bounds;
    sim.terrain_speed_config = terrain_speed_config.clone();
    let order = live_order
        .map(<[u64]>::to_vec)
        .unwrap_or_else(|| sim.substrate.entities.keys_sorted());
    let mut stats = MovementTickStats::default();
    for id in order {
        // The full object turn applies these lifecycle requests before the
        // next object. Component fixtures retain them for inspection, so an
        // already requested victim must not receive another movement visit.
        if sim.pending_lifecycle_requests.iter().any(|request| {
            matches!(request, LifecycleRequest::Uninit { stable_id, .. } if *stable_id == id)
        }) {
            continue;
        }
        stats.merge(
            sim.process_ground_locomotor_stats_for_test(id, rules, None)
                .expect("fixture reached an unsupported production movement receiver"),
        );
    }
    *entities = sim.substrate.entities;
    *occupancy = sim.substrate.occupancy;
    *cell_occupation = sim.substrate.cell_occupation;
    *raw_cell_occupation = sim.substrate.raw_cell_occupation;
    *interner = sim.interner;
    *rng = sim.scenario_rng;
    sound_events.append(&mut sim.sound_events);
    lifecycle_requests.append(&mut sim.pending_lifecycle_requests);
    stats
}

pub(crate) struct PendingMovementPass {
    effects: MovementPassEffects,
    /// The mover owner's block sets the visit lends to its path requests.
    held_block_sets: HeldBlockSets,
}

/// Where the Process host re-enters the pending pass for the same mover.
pub(crate) enum MoverReentry {
    /// Find_Path resumed head selection (Walk after 0x75AFC5, Drive 0x4B32A1,
    /// Ship 0x6A28F1).
    FootPath(Box<FootPathRequest>),
    /// Process_Movement after Process_Track(0) ended the track in the same
    /// Process (Drive 0x4B0647 / Ship 0x69FCEE).
    AfterTrackEnd(u64),
}

impl PendingMovementPass {
    pub(crate) fn take_foot_path_request(&mut self) -> Option<FootPathRequest> {
        self.effects.foot_path_request.take()
    }

    pub(crate) fn take_walk_admission_request(&mut self) -> Option<WalkAdmissionRequest> {
        self.effects.walk_admission_request.take()
    }

    /// The Scatter calls this pass's steps queued since the last take.
    pub(crate) fn take_scatter_requests(&mut self) -> Vec<(u64, super::ScatterFlags)> {
        self.effects.scatters.take()
    }

    pub(crate) fn request_foot_path(&mut self, request: FootPathRequest) {
        debug_assert!(self.effects.foot_path_request.is_none());
        self.effects.foot_path_request = Some(request);
    }

    pub(crate) fn take_track_movement(
        &mut self,
    ) -> Option<(u64, super::track_process::TrackFamily)> {
        self.effects.track_movement.take()
    }

    /// The owner sets this pass holds, for the path requests it makes.
    pub(crate) fn held_block_sets(&mut self) -> &mut HeldBlockSets {
        &mut self.held_block_sets
    }

    /// A Drive/Ship Process_Movement that returned without head selection
    /// still reaches the outer Process_Track (0x4B0AAA / 0x6A0173).
    pub(crate) fn record_native_track(
        &mut self,
        invocation: super::track_process::TrackInvocation,
    ) {
        self.effects.native_track = Some(invocation);
    }

    /// Re-enter this pass for the same mover of its Process (see
    /// `MoverReentry`), reading the Simulation's current grids and state.
    pub(crate) fn reenter_mover(
        &mut self,
        sim: &mut crate::sim::world::Simulation,
        reentry: MoverReentry,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) {
        let grids = PassGrids {
            path_grid: sim.path_grid.as_deref(),
            resolved_terrain: sim.resolved_terrain.as_ref(),
            playfield_bounds: sim.playfield_bounds,
        };
        let (entity_id, entry) = match reentry {
            MoverReentry::FootPath(request) => (
                request.entity_id,
                VisitEntry::FootPathResume(Box::new(request.visit)),
            ),
            MoverReentry::AfterTrackEnd(entity_id) => (entity_id, VisitEntry::AfterTrackEnd),
        };
        advance_ordinary_mover(
            &mut sim.substrate.entities,
            entity_id,
            grids,
            &mut sim.substrate.occupancy,
            &mut sim.substrate.cell_occupation,
            &mut sim.substrate.raw_cell_occupation,
            sim.session.binary_frame,
            &mut sim.interner,
            rules,
            Some(&sim.type_handles),
            &sim.houses,
            &mut self.effects,
            entry,
        );
    }

    pub(crate) fn take_walk_boundary(
        &mut self,
    ) -> Option<(u64, crate::sim::components::DriveCoord)> {
        self.effects.walk_boundary.take()
    }

    pub(crate) fn take_walk_per_cell(
        &mut self,
    ) -> Option<(u64, crate::sim::components::DriveCoord)> {
        self.effects.walk_per_cell.take()
    }

    /// PerCell may retire this Foot or install another order. Finalization
    /// must use the surviving live path, never the pre-callback finished list.
    pub(crate) fn retain_walk_completion(&mut self, id: u64, entities: &EntityStore) {
        let complete = entities.get(id).is_some_and(|e| {
            e.lifecycle.object_alive
                && !e.lifecycle.in_limbo
                && !e.is_falling_down()
                && e.locomotor.as_ref().is_some_and(|loco| {
                    loco.walk_destination().is_none() && loco.step_head().is_none()
                })
                && e.movement_target.is_some()
        });
        self.effects
            .finished_entities
            .retain(|candidate| *candidate != id);
        if complete {
            self.effects.finished_entities.push(id);
        }
    }
    pub(crate) fn take_native_track(&mut self) -> Option<super::track_process::TrackInvocation> {
        self.effects.native_track.take()
    }
    pub(crate) fn record_track_movement(&mut self, moved: u32) {
        self.effects.stats.moved_steps = self.effects.stats.moved_steps.saturating_add(moved);
    }
}

/// One object's ground movement visit, up to its first world callback.
pub(crate) fn begin_movement(
    entities: &mut EntityStore,
    entity_id: u64,
    path_grid: Option<&PathGrid>,
    alliances: &HouseAllianceMap,
    occupancy: &mut OccupancyGrid,
    cell_occupation: &mut CellOccupationGrid,
    raw_cell_occupation: &mut RawCellOccupationGrid,
    rng: &mut SimRng,
    native_frame: u32,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    playfield_bounds: Option<PlayfieldBounds>,
    interner: &mut crate::sim::intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    type_handles: Option<&TypeHandleTable>,
    houses: &std::collections::BTreeMap<
        crate::sim::intern::InternedId,
        crate::sim::house_state::HouseState,
    >,
    caches: &mut MovementPassCache,
) -> PendingMovementPass {
    let grids = PassGrids {
        path_grid,
        resolved_terrain,
        playfield_bounds,
    };
    let mut effects = MovementPassEffects::default();
    let mut held_block_sets = HeldBlockSets::new();
    let mover_owner = prepare_movement_visit(
        entities,
        entity_id,
        grids,
        occupancy,
        cell_occupation,
        raw_cell_occupation,
        rng,
        native_frame,
        interner,
        rules,
        &mut effects.stats,
        &mut effects.scatters,
    );
    if let Some(owner) = mover_owner {
        // The owner's entity block sets for friendly-passable pathfinding,
        // as a build from the entities would give them now.
        let lent = caches.block_index.lend_current(
            owner,
            entities,
            resolved_terrain,
            alliances,
            interner,
            rules,
        );
        held_block_sets.insert(owner, lent);
        advance_ordinary_mover(
            entities,
            entity_id,
            grids,
            occupancy,
            cell_occupation,
            raw_cell_occupation,
            native_frame,
            interner,
            rules,
            type_handles,
            houses,
            &mut effects,
            VisitEntry::Process,
        );
    }
    PendingMovementPass {
        effects,
        held_block_sets,
    }
}

pub(crate) fn finish_movement_pass(
    pending: PendingMovementPass,
    entities: &mut EntityStore,
    caches: &mut MovementPassCache,
) -> MovementTickStats {
    let PendingMovementPass {
        effects,
        held_block_sets,
    } = pending;
    for (owner, lent) in held_block_sets {
        caches.block_index.give_back(owner, lent);
    }
    let MovementPassEffects {
        stats,
        finished_entities,
        ..
    } = effects;

    finalize_finished_entities(entities, &finished_entities);
    stats
}

// ---------------------------------------------------------------------------
// Post-loop helpers — extracted from tick_movement_with_grids
// ---------------------------------------------------------------------------

/// Retire the scheduling adapter of each Walk whose Process found neither a
/// head nor a destination. Walk's arrival (75BE42) already ran in its Process.
fn finalize_finished_entities(entities: &mut EntityStore, finished: &[u64]) {
    for &entity_id in finished {
        if let Some(entity) = entities.get_mut(entity_id) {
            entity.movement_target = None;
        }
    }
}

#[cfg(test)]
#[path = "track_chain_migration_tests.rs"]
mod track_chain_migration_tests;

#[cfg(test)]
mod pass_cache_tests {
    use super::*;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::test_interner;
    use crate::sim::occupancy::CellListInsertion;

    #[test]
    fn retained_foot_changes_replay_without_world_rebuild_or_position_reconstruction() {
        let grid = PathGrid::new(5, 5);
        let terrain = ResolvedTerrainGrid::from_cells(
            5,
            5,
            (0..5)
                .flat_map(|y| {
                    (0..5).map(move |x| crate::map::resolved_terrain::test_flat_cell(x, y))
                })
                .collect(),
        );
        let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(5, 5);
        let mut entities = EntityStore::new();
        let mut unit = GameEntity::test_default(7, "MTNK", "Americans", 2, 2);
        unit.lifecycle.cell_marked = true;
        entities.insert(unit);
        let interner = test_interner();
        let mut cache = MovementPassCache::default();
        overlays.adjust_foot_neighbor_source(Some(&terrain), (2, 2), true);
        assert_eq!(
            cache
                .blocker_plane(
                    &mut entities,
                    &grid,
                    Some(&terrain),
                    Some(&overlays),
                    &interner,
                    None
                )
                .count_at(1, 1),
            1,
            "do not double-count live Foot positions"
        );
        entities.get_mut(7).unwrap().position.rx = 4;
        assert_eq!(
            cache
                .blocker_plane(
                    &mut entities,
                    &grid,
                    Some(&terrain),
                    Some(&overlays),
                    &interner,
                    None
                )
                .count_at(1, 1),
            1,
            "physical flight preserves history"
        );
        overlays.adjust_foot_neighbor_source(Some(&terrain), (2, 2), false);
        overlays.adjust_foot_neighbor_source(Some(&terrain), (3, 2), true);
        let current = cache.blocker_plane(
            &mut entities,
            &grid,
            Some(&terrain),
            Some(&overlays),
            &interner,
            None,
        );
        assert_eq!(current.count_at(1, 1), 0);
        assert_eq!(current.count_at(4, 1), 1);
        assert_eq!(cache.blocker_plane_world_rebuilds(), 1);
        // A dormant reader can miss the bounded journal; rebuilding must
        // recover the authoritative wrapping bytes, not inferred occupancy.
        for _ in 0..260 {
            overlays.adjust_foot_neighbor_source(Some(&terrain), (3, 2), false);
            overlays.adjust_foot_neighbor_source(Some(&terrain), (2, 2), true);
        }
        let expected = bump_crush::build_blocker_neighbor_counts_with_overlays(
            &entities,
            5,
            5,
            Some(&terrain),
            Some(&overlays),
            &interner,
            None,
        );
        assert_eq!(
            cache.blocker_plane(
                &mut entities,
                &grid,
                Some(&terrain),
                Some(&overlays),
                &interner,
                None
            ),
            &expected
        );
        assert_eq!(cache.blocker_plane_world_rebuilds(), 2);
    }

    /// A marked object that starts dying stays on the occupancy grid. Its
    /// writer takes it mutably from the store, which is all the plane needs.
    #[test]
    fn the_kept_blocker_plane_follows_a_marked_object_that_starts_dying() {
        let grid = PathGrid::new(5, 5);
        let mut entities = EntityStore::new();
        let mut occupancy = OccupancyGrid::new();
        let mut unit = GameEntity::test_default(7, "MTNK", "Americans", 2, 2);
        unit.lifecycle.cell_marked = true;
        entities.insert(unit);
        occupancy.add(
            2,
            2,
            7,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let interner = test_interner();
        let mut cache = MovementPassCache::default();

        let before = cache
            .blocker_plane(&mut entities, &grid, None, None, &interner, None)
            .clone();
        assert_eq!(
            before.count_at(1, 1),
            1,
            "the marked unit is a neighbour source"
        );
        // Nothing touched: kept (and cross-checked in debug builds).
        let again = cache
            .blocker_plane(&mut entities, &grid, None, None, &interner, None)
            .clone();
        assert_eq!(again, before);

        // Death sequence: the object stays on the grid.
        entities.get_mut(7).unwrap().dying = true;
        let after = cache
            .blocker_plane(&mut entities, &grid, None, None, &interner, None)
            .clone();
        assert_eq!(
            after.count_at(1, 1),
            0,
            "a dying object is not a neighbour source"
        );
        assert_ne!(after, before);
        assert_eq!(
            cache.blocker_plane_world_rebuilds(),
            1,
            "only the first use read the whole world"
        );
    }

    /// A grid that retains its wall plane, as every production grid does, is
    /// read for that plane only: an ore write moves the grid's general epoch
    /// and must not rebuild the blocker plane; a wall-plane write must.
    #[test]
    fn ore_writes_do_not_rebuild_the_kept_blocker_plane() {
        let grid = PathGrid::new(5, 5);
        let mut entities = EntityStore::new();
        let interner = test_interner();
        let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(5, 5);
        let mut cache = MovementPassCache::default();
        let mut plane = |cache: &mut MovementPassCache,
                         overlays: &crate::sim::overlay_grid::OverlayGrid| {
            cache
                .blocker_plane(&mut entities, &grid, None, Some(overlays), &interner, None)
                .clone()
        };
        let before = plane(&mut cache, &overlays);
        let epoch = overlays.mutation_epoch();
        overlays.place_overlay(2, 2, 102, 5);
        assert_ne!(
            overlays.mutation_epoch(),
            epoch,
            "the ore write is a mutation"
        );
        assert_eq!(plane(&mut cache, &overlays), before);
        assert_eq!(cache.blocker_plane_world_rebuilds(), 1);

        overlays.seed_neighbor_counts_for_tests(0);
        let _ = plane(&mut cache, &overlays);
        assert_eq!(cache.blocker_plane_world_rebuilds(), 2);
    }

    /// Random edits through the store, the plane brought current after each
    /// batch and compared with a build of the whole world. Counts wrap, so a
    /// source taken out must leave exactly what was there before it went in.
    #[test]
    fn the_kept_blocker_plane_equals_a_whole_world_build_after_random_edits() {
        use crate::rules::ini_parser::IniFile;
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]
0=MTNK
[BuildingTypes]
0=GAPOWR
             [MTNK]
Speed=4
[GAPOWR]
Foundation=2x2
",
        ))
        .expect("rules");
        let grid = PathGrid::new(12, 12);
        let mut entities = EntityStore::new();
        let mut cache = MovementPassCache::default();
        for name in ["Americans", "MTNK", "GAPOWR"] {
            crate::sim::intern::test_intern(name);
        }
        let interner = test_interner();
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = |bound: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % bound
        };
        for step in 0..800 {
            let id = 1 + next(10);
            let cell = (next(12) as u16, next(12) as u16);
            match next(6) {
                0 => {
                    entities.remove(id);
                }
                1 | 2 => {
                    let building = next(3) == 0;
                    let kind = if building { "GAPOWR" } else { "MTNK" };
                    let mut entity =
                        GameEntity::test_default(id, kind, "Americans", cell.0, cell.1);
                    if building {
                        entity.category = crate::map::entities::EntityCategory::Structure;
                    }
                    entity.lifecycle.cell_marked = true;
                    entities.insert(entity);
                }
                edit => {
                    if let Some(entity) = entities.get_mut(id) {
                        match edit {
                            3 => (entity.position.rx, entity.position.ry) = cell,
                            4 => entity.dying = !entity.dying,
                            _ => entity.lifecycle.cell_marked = !entity.lifecycle.cell_marked,
                        }
                    }
                }
            }
            if step % 3 == 0 {
                let kept = cache
                    .blocker_plane(&mut entities, &grid, None, None, &interner, Some(&rules))
                    .clone();
                let built = bump_crush::build_blocker_neighbor_counts_with_overlays(
                    &entities,
                    12,
                    12,
                    None,
                    None,
                    &interner,
                    Some(&rules),
                );
                assert_eq!(kept, built, "step {step}");
            }
        }
        assert_eq!(cache.blocker_plane_world_rebuilds(), 1);
    }
}
