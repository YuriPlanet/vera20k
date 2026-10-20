//! Ground movement tick — the per-tick state machine for all ground/bridge entities.
//!
//! Prepares and resumes one production object visit: rotation, speed inputs
//! and fresh head selection.
//! Retained Drive/Ship tracks execute through track_host. The batch entry
//! points are test adapters.
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
#[cfg(any(test, feature = "test-support"))]
use crate::map::houses::HouseAllianceMap;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone};
use crate::sim::cell_rect::PlayfieldBounds;
use crate::sim::entity_store::{EntityStore, TouchReader};
use crate::sim::infantry;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::lifecycle_request::LifecycleRequest;
use crate::sim::pathfinding::PathGrid;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::pathfinding::zone_map::ZoneGrid;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::rng::SimRng;
use crate::sim::touch_log::Touched;

use super::bump_crush;
use super::locomotor::MovementLayer;
use super::movement_step;
use super::{MovementTickStats, MoverSnapshot, PathfindingContext};
#[cfg(any(test, feature = "test-support"))]
use crate::sim::occupancy::{OccupancyGrid, RawCellOccupationGrid};

/// Build a read-only snapshot of the mover's properties before entering the
/// inner movement loop. This avoids repeated `entities.get()` calls and keeps
/// the data available across the mutable/immutable borrow boundary.
///
/// VERA-only: a per-visit copy of the mover's class, locomotor, SpeedType,
/// MovementZone, crusher flags and playfield facts. Nothing writes
/// it back; the entity's records stay the source of truth.
pub(super) fn snapshot_mover(
    entities: &EntityStore,
    entity_id: u64,
    playfield_bounds: Option<crate::sim::cell_rect::PlayfieldBounds>,
) -> Option<MoverSnapshot> {
    let e = entities.get(entity_id)?;
    Some(MoverSnapshot {
        category: e.category(),
        speed_type: e.locomotor.as_ref().map(|l| l.speed_type()),
        movement_zone: e
            .locomotor
            .as_ref()
            .map(|l| l.movement_zone())
            .unwrap_or(MovementZone::Normal),
        omni_crusher: e.mover_facts.omni_crusher(),
        regular_crusher: e.mover_facts.regular_crusher(),
        locomotor: e.locomotor.clone(),
        allow_zone_hierarchy: playfield_bounds.is_none() || e.in_playfield(),
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

/// Synchronous Walk fresh-head request: the tube word (75B302) and then
/// Can_Enter_Cell (75B690). The pending pass releases the mutable mover
/// before either, since both need the whole live Foot. A tube word or a
/// clear answer finishes fresh-head selection in that same Process; a
/// recursive refusal reuses the retained visit through FootPathRequest.
pub(crate) struct WalkAdmissionRequest {
    pub(crate) entity_id: u64,
    visit: OrdinaryMoverVisit,
}

impl WalkAdmissionRequest {
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn for_response_test(
        entities: &EntityStore,
        entity_id: u64,
        allow_retry: bool,
    ) -> Option<Self> {
        let mut request = FootPathRequest::track(
            entities,
            entity_id,
            crate::sim::components::DriveCoord { x: 0, y: 0, z: 0 },
            0,
            None,
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
/// Foot+5E0 head is empty (native -1): `WalkLocomotionClass::ProcessMovement`
/// (`0x0075AEC0`), whose request is the call at `0x0075AFC5`. Drive/Ship
/// requests are made inside their Process_Movement (`track_fresh`).
fn no_queue_path_request(
    entity: &crate::sim::game_entity::GameEntity,
) -> Option<crate::sim::components::DriveCoord> {
    if !entity
        .navigation
        .path_replay()
        .remaining_directions()
        .is_empty()
    {
        return None;
    }
    let loco = entity.locomotor.as_ref()?;
    match loco.active_kind() {
        LocomotorKind::Walk if loco.step_head().is_none() => loco.walk_destination(),
        _ => None,
    }
}

impl FootPathRequest {
    /// Resume the retained recursive visit in the decoder corpus after its
    ///supplied Find_Path. Production resumes through PendingMovementPass.
    #[cfg(any(test, feature = "test-support"))]
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
    ) -> Option<Self> {
        let snap = snapshot_mover(entities, entity_id, playfield_bounds)?;
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

    /// The mover facts this request's search reads, as snapshotted.
    pub(super) fn snapshot(&self) -> &MoverSnapshot {
        &self.visit.snap
    }

    /// Find_Path's core search (`Simulation::foot_core_search`) calls this
    /// between its Mark0/Mark1, or before Mark0 for a batched path search
    /// whose `foot_entry` emulates the lift (`foot_path::MoverLift`). The
    /// context's blocker plane is current; this reuses the single existing
    /// search. `foot_entry` answers every class question and walks every
    /// code-2 chain.
    pub(super) fn search(
        &self,
        goal: crate::sim::components::DriveCoord,
        entities: &EntityStore,
        ctx: PathfindingContext<'_>,
        foot_entry: &dyn crate::sim::pathfinding::SearchFootEntry,
    ) -> Result<Vec<(u16, u16)>, super::movement_path::MovePathFailure> {
        let snap = &self.visit.snap;
        let actor = entities.get(self.entity_id).expect("live suspended mover");
        let start = (actor.position.rx(), actor.position.ry());
        // AStar 0x00429A90 takes the start height from the Foot's OnBridge
        // byte (+0x8C): the cell level, +4 on a bridge. No path layer.
        let layer = if actor.on_bridge() {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        let goal = ((goal.x / 256) as u16, (goal.y / 256) as u16);
        debug_assert!(
            ctx.blocker_neighbor_counts.is_some() == ctx.path_grid.is_some(),
            "path build on a pass that skipped the blocker plane; see pass_may_build_paths"
        );
        super::movement_path::find_move_path(
            ctx,
            start,
            layer,
            goal,
            snap.movement_zone,
            Some(snap.movement_zone),
            None,
            //The admitted search prepares markers and applies native42AECA
            //urgency downgrade through the live Foot provider.
            self.urgency,
            snap.allow_zone_hierarchy,
            foot_entry,
        )
        // Foot+5E0 words carry no layer; the Process reads OnBridge.
        .map(|(path, _)| path)
    }

    /// FootFindPath4D3920's success tail: the found route becomes the
    /// Foot+5E0 direction queue, and 4D4003 records the current Cell as its
    /// reference. Foot timer/latch/retry state has its own lifetime.
    ///
    /// Every Foot reads only that queue.
    pub(super) fn install_route(
        &self,
        actor: &mut crate::sim::game_entity::GameEntity,
        path: Vec<(u16, u16)>,
    ) {
        let current = (actor.position.rx(), actor.position.ry());
        actor
            .navigation
            .path_replay_mut()
            .install(current, &path, 1);
    }
}

/// Effects accumulated until the pass tail. Keeping these together preserves
/// crushed-victim exclusions across mover visits.
#[derive(Default)]
struct MovementPassEffects {
    stats: MovementTickStats,
    native_track: Option<super::track_process::TrackInvocation>,
    walk_per_cell: Option<(u64, crate::sim::components::DriveCoord)>,
    walk_boundary: Option<(u64, crate::sim::components::DriveCoord)>,
    foot_path_request: Option<FootPathRequest>,
    walk_admission_request: Option<WalkAdmissionRequest>,
    /// A Drive/Ship Unit's Process_Movement(&out, 1, 0), which the host runs
    /// at the Simulation (`track_fresh`).
    track_movement: Option<(u64, super::track_process::TrackFamily)>,
}

/// A Drive/Ship Unit: its Process_Movement is `track_fresh`. The active
/// locomotor's Process decides it (`DriveLocomotionClass::Process`
/// `0x004B0500`, `ShipLocomotionClass::Process` `0x0069FC10`).
fn native_track_route(
    entity: &crate::sim::game_entity::GameEntity,
) -> Option<super::track_process::TrackFamily> {
    if entity.category() != EntityCategory::Unit {
        return None;
    }
    match entity.locomotor.as_ref()?.active_kind() {
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
    native_frame: u32,
    interner: &mut crate::sim::intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    houses: &crate::sim::houses::Houses,
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
            // Process_Track. An exhausted route cannot route it back through
            // fresh selection/rotation and strand its cursor.
            let entity = entities.get(entity_id).expect("live retained track");
            *native_track = Some(super::track_process::TrackInvocation {
                entity_id,
                family: if entity
                    .locomotor
                    .as_ref()
                    .is_some_and(|l| l.active_kind() == LocomotorKind::Ship)
                {
                    super::track_process::TrackFamily::Ship
                } else {
                    super::track_process::TrackFamily::Drive
                },
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
            .is_some_and(|locomotor| locomotor.active_kind() == LocomotorKind::Walk)
        {
            return;
        }

        // Snapshot mover data before entering the inner loop so we can release the
        // mutable borrow on `entities` when needed for crush/bump immutable lookups.
        let Some(snap) = snapshot_mover(entities, entity_id, playfield_bounds) else {
            return;
        };
        // Walk tests CanEnter at 0x75B690 before its paid SetCoords calls
        // (0x75BDC0/0x75C12E). A refused prospective step keeps exact XY.
        let walk_position_before_step = snap
            .locomotor
            .as_ref()
            .filter(|loco| loco.active_kind() == crate::rules::locomotor_type::LocomotorKind::Walk)
            .and_then(|_| entities.get(entity_id).map(|entity| entity.position));
        let prone_crawls = entities.get(entity_id).and_then(|entity| {
            if !infantry::is_prone_for_damage(entity) {
                return None;
            }
            let rules = rules?;
            let obj = interner.object_type(entity.type_ref(), rules)?;
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
        && let Some(destination) = entities.get(entity_id).and_then(no_queue_path_request)
    {
        let entity = entities.get(entity_id).expect("same mover request");
        //Walk75AF3C..55 observes a frame-anchored exact-zero remainder. No
        //search, debt, occupancy preparation or second mover visit on this
        //wait: 75AF57..75AF66 calls the owner's vt+0x548 and returns.
        if !entity
            .navigation
            .path_runtime()
            .movement_timer()
            .expired(native_frame as i32)
        {
            if let Some(entity) =
                entities.get_mut_if(entity_id, super::walk_admission::walk_doing_to_clear)
            {
                super::walk_admission::clear_walk_doing(entity);
            }
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
    let Some(entity) = entities.get_mut(entity_id) else {
        return;
    };
    if entity
        .locomotor
        .as_ref()
        .is_some_and(|l| l.step_head().is_none())
    {
        //75B302 (the tube word) and 75B690 (Can_Enter_Cell) require the
        //whole live Foot owner. Suspending here releases this entity's
        //mutable borrow before either. Canonical Clear goes straight to
        //75C240, never through the grid/cliff/occupancy adapters below.
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
    let object = rules.and_then(|r| interner.object_type(entity.type_ref(), r));
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
        entity.position.copy_from(
            *walk_position_before_step
                .as_ref()
                .expect("only Walk can suspend a boundary"),
        );
        *walk_boundary = Some((entity_id, coord));
        return;
    }
    // Walk clears the blocked latch when it makes a paid coordinate
    // step (0x75BFCD), not merely when FindPath succeeds.
    if let Some(before) = walk_position_before_step.as_ref()
        && super::ground_pose::position_world_xy(before)
            != super::ground_pose::position_world_xy(&entity.position)
    {
        entity.navigation.path_runtime_mut().clear_blockage();
    }
}

/// Derived movement inputs that survive across object turns.
///
/// The base contains retained wall/Foot bytes and derived terrain occupation.
/// Terrain edits rebuild under their epoch. Foot writes replay the owner's
/// bounded cell-delta journal; after a wall edit, or when the reader fell
/// behind the journal, the retained bytes' difference from the copy the plane
/// last took applies instead (the counts wrap, so the sum is exact either
/// way). The entity touch log maintains building contributions (and, in
/// fixtures without an overlay grid, mobiles).
/// Debug checks compare against a fresh sum. No cache data is saved or hashed.
#[derive(Default)]
pub(crate) struct MovementPassCache {
    blocker: BlockerPlaneCache,
}

#[derive(Default)]
struct BlockerPlaneCache {
    entry: Option<BlockerPlaneEntry>,
    /// How often the plane was rebuilt from the whole map and every entity.
    #[cfg(any(test, feature = "test-support"))]
    world_rebuilds: usize,
}

struct BlockerPlaneEntry {
    key: BlockerPlaneKey,
    plane: crate::sim::pathfinding::BlockerNeighborCounts,
    sources: BTreeMap<u64, bump_crush::BlockerPlaneSource>,
    /// The overlay grid's retained wall/Foot bytes as `plane` holds them,
    /// with the Foot journal revision and wall epoch they are current at.
    retained: Vec<u8>,
    foot_revision: u64,
    wall_epoch: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct BlockerPlaneKey {
    terrain_epoch: Option<u64>,
    /// Whether an overlay grid retains the wall/Foot bytes: with one, only
    /// buildings are entity sources.
    retained_foot: bool,
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

    /// How often the blocker plane was rebuilt from the whole map and every
    /// entity.
    #[cfg(any(test, feature = "test-support"))]
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
        let blocker = &mut self.blocker;
        let key = BlockerPlaneKey {
            terrain_epoch: resolved_terrain.map(ResolvedTerrainGrid::mutation_epoch),
            retained_foot: overlay_grid.is_some(),
            width: grid.width(),
            height: grid.height(),
            rules: rules.map(|rules| std::ptr::from_ref(rules) as usize),
        };
        let retained_foot = key.retained_foot;
        let touched = match entities.take_touched(TouchReader::BlockerPlane) {
            Touched::All => None,
            Touched::Ids(ids) => Some(ids),
        };
        match blocker
            .entry
            .as_mut()
            .filter(|entry| entry.key == key && touched.is_some())
        {
            Some(entry) => {
                if let Some(grid) = overlay_grid {
                    let journal = (entry.wall_epoch == grid.blocker_plane_epoch())
                        .then(|| grid.foot_neighbor_changes_since(entry.foot_revision))
                        .flatten();
                    match journal {
                        Some(changes) => {
                            for (index, delta) in changes {
                                entry.plane.apply_retained_delta(index, delta);
                                entry.retained[index] = entry.retained[index].wrapping_add(delta);
                            }
                        }
                        None => entry.plane.apply_retained_difference(
                            &mut entry.retained,
                            grid.retained_neighbor_counts(),
                        ),
                    }
                    entry.foot_revision = grid.foot_neighbor_revision();
                    entry.wall_epoch = grid.blocker_plane_epoch();
                }
                for id in touched.into_iter().flatten() {
                    // With the retained Foot bytes only a building is a
                    // source (`blocker_plane_source`), and a stored object's
                    // category never changes: a touched object of another
                    // category without a source still has none. The category
                    // column says so without loading the entity.
                    if retained_foot
                        && entities
                            .category_of(id)
                            .is_some_and(|category| category != EntityCategory::Structure)
                        && !entry.sources.contains_key(&id)
                    {
                        continue;
                    }
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
                    retained: overlay_grid
                        .map_or_else(Vec::new, |grid| grid.retained_neighbor_counts().to_vec()),
                    foot_revision: overlay_grid.map_or(0, |grid| grid.foot_neighbor_revision()),
                    wall_epoch: overlay_grid.map_or(0, |grid| grid.blocker_plane_epoch()),
                });
                #[cfg(any(test, feature = "test-support"))]
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
/// Resuming after a world callback must not repeat it: it runs the idle
/// Walk tail. Returns whether the object still advances as an ordinary
/// mover.
fn prepare_movement_visit(entities: &mut EntityStore, entity_id: u64) -> bool {
    let Some(entity) = entities.get_mut(entity_id) else {
        return false;
    };
    // Walk75AEC0 is the Process of every live Foot (FootClass::AI
    // 0x004DA806..0x004DA877) and branches on its own head (+0x28) and
    // destination (+0x1C): either one runs its step, neither the idle tail
    // 75BCE3 (Walk75AEC3..75AF10).
    let walk_route = entity.locomotor.as_ref().is_some_and(|loco| {
        loco.active_kind() == LocomotorKind::Walk
            && (loco.step_head().is_some() || loco.walk_destination().is_some())
    });
    if entity.is_active() && !walk_route {
        super::walk_step::finish_idle(entity);
    }
    // A Drive or Ship got here through the outer Process gates
    // (`track_turn`: Drive 0x4B08D1..0x4B0A69 / Ship 0x69FF98..0x6A0131),
    // which admit Process_Movement (0x4B0A79 / 0x6A0142) for a moving class
    // or a live path word.
    let track_route = entity.locomotor.as_ref().is_some_and(|loco| {
        super::track_process::TrackFamily::from_kind(loco.active_kind()).is_some()
    });
    if !walk_route && !track_route {
        return false;
    }
    let layer = entity.movement_layer_or_ground();
    !matches!(layer, MovementLayer::Air | MovementLayer::Underground)
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn tick_movement_with_grids(
    entities: &mut EntityStore,
    live_order: Option<&[u64]>,
    path_grid: Option<&PathGrid>,
    alliances: &HouseAllianceMap,
    occupancy: &mut OccupancyGrid,
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
    sim.substrate.raw_cell_occupation = std::mem::take(raw_cell_occupation);
    sim.interner = std::mem::take(interner);
    sim.scenario_rng = rng.clone();
    sim.session.set_tick_for_test(sim_tick);
    sim.session.set_binary_frame_for_test(native_frame);
    *sim.house_alliances_mut_for_test() = alliances.clone();
    sim.path_grid = path_grid.cloned().map(std::sync::Arc::new);
    sim.zone_grid = zone_grid.cloned();
    sim.resolved_terrain = resolved_terrain.cloned();
    sim.set_playfield_bounds_for_test(playfield_bounds);
    sim.set_terrain_speed_config_for_test(terrain_speed_config.clone());
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
    *raw_cell_occupation = sim.substrate.raw_cell_occupation;
    *interner = sim.interner;
    *rng = sim.scenario_rng;
    sound_events.append(&mut sim.sound_events);
    lifecycle_requests.append(&mut sim.pending_lifecycle_requests);
    stats
}

pub(crate) struct PendingMovementPass {
    effects: MovementPassEffects,
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

    pub(crate) fn request_foot_path(&mut self, request: FootPathRequest) {
        debug_assert!(self.effects.foot_path_request.is_none());
        self.effects.foot_path_request = Some(request);
    }

    pub(crate) fn take_track_movement(
        &mut self,
    ) -> Option<(u64, super::track_process::TrackFamily)> {
        self.effects.track_movement.take()
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
            playfield_bounds: sim.playfield_bounds(),
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
            sim.session.binary_frame(),
            &mut sim.interner,
            rules,
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
    native_frame: u32,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    playfield_bounds: Option<PlayfieldBounds>,
    interner: &mut crate::sim::intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    houses: &crate::sim::houses::Houses,
) -> PendingMovementPass {
    let grids = PassGrids {
        path_grid,
        resolved_terrain,
        playfield_bounds,
    };
    let mut effects = MovementPassEffects::default();
    let ordinary_mover = prepare_movement_visit(entities, entity_id);
    if ordinary_mover {
        advance_ordinary_mover(
            entities,
            entity_id,
            grids,
            native_frame,
            interner,
            rules,
            houses,
            &mut effects,
            VisitEntry::Process,
        );
    }
    PendingMovementPass { effects }
}

pub(crate) fn finish_movement_pass(pending: PendingMovementPass) -> MovementTickStats {
    pending.effects.stats
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
        unit.lifecycle.set_cell_marked_for_test(true);
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
        entities.get_mut(7).unwrap().position.set_rx_for_test(4);
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
        // A dormant reader can miss the bounded journal; the retained bytes'
        // difference must recover the authoritative wrapping bytes, not
        // inferred occupancy, without reading the whole world.
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
        assert_eq!(cache.blocker_plane_world_rebuilds(), 1);
    }

    /// A marked object that starts dying stays on the occupancy grid. Its
    /// writer takes it mutably from the store, which is all the plane needs.
    #[test]
    fn the_kept_blocker_plane_follows_a_marked_object_that_starts_dying() {
        let grid = PathGrid::new(5, 5);
        let mut entities = EntityStore::new();
        let mut occupancy = OccupancyGrid::new();
        let mut unit = GameEntity::test_default(7, "MTNK", "Americans", 2, 2);
        unit.lifecycle.set_cell_marked_for_test(true);
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
        entities.get_mut(7).unwrap().death.set_dying_for_test(true);
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
    /// and must not rebuild the blocker plane; a wall-plane write reaches it
    /// as the retained bytes' difference, without reading the whole world.
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

        overlays.seed_neighbor_counts_for_tests(3);
        assert_eq!(plane(&mut cache, &overlays).count_at(2, 2), 3);
        assert_eq!(cache.blocker_plane_world_rebuilds(), 1);
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
                        entity
                            .set_category_for_test(crate::map::entities::EntityCategory::Structure);
                    }
                    entity.lifecycle.set_cell_marked_for_test(true);
                    entities.insert(entity);
                }
                edit => {
                    if let Some(entity) = entities.get_mut(id) {
                        match edit {
                            3 => {
                                entity.position.set_rx_for_test(cell.0);
                                entity.position.set_ry_for_test(cell.1);
                            }
                            4 => entity.death.set_dying_for_test(!entity.death.is_dying()),
                            _ => entity
                                .lifecycle
                                .set_cell_marked_for_test(!entity.lifecycle.cell_marked()),
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
