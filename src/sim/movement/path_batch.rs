//! Batched path searches: a frame's waiting Find_Path requests search side
//! by side.
//!
//! Two requests wait. A Foot with no path queue makes a no-queue request
//! (Walk 0x75AFC5, Drive 0x4B28A3, Ship 0x6A1EF3) whenever its movement timer
//! has run out: its first request after a new destination and each one after
//! a route runs out. A Foot whose next cell holds a moving friendly
//! (Can_Enter_Cell code 2) re-paths from its code-2 ladder (Drive/Ship
//! 0x4B3607..0x4B3A94, Walk 0x0075B8A9..0x0075B9F9) whenever that timer has
//! run out. With batching on, either registers the Foot and takes the
//! timer-still-running branch instead. At the start of the next frame's live
//! object pass every registration is searched on worker threads against the
//! world as it stands, and the Foot's request then runs its Find_Path with
//! the answer.
//!
//! Proposed sim-scale deviation from native order (update order and
//! same-frame effects), pending side-by-side play: a Foot's path search runs
//! one frame later, from the world at the start of the pass rather than at
//! its own turn, so a Foot starts along a new route one frame later, and its
//! movement timer (PathDelay) is armed one frame later, which shifts its
//! later requests too. A Foot that does not repeat its request that next
//! frame does not search at all: a blocked Foot whose blocker moved on keeps
//! its route; a no-queue request whose destination cleared is dropped. Two
//! visible cases: Infantry Scatter's immediate Process (0x51D478) waits, so
//! scattered infantry leave a frame later; a Drive/Ship Foot whose track
//! ends with no path left loses that frame's movement where native's timer
//! had run out. The admission retries' recursions (Walk's Process(0),
//! Drive/Ship's Process_Movement(0, 0)) never wait: their timer was just
//! expired. Hover's own timer-gated Find_Path is not batched. The batch
//! draws no random numbers; later draws follow the changed movement. Each
//! search reads only that frame-start world, so no result depends on the
//! thread count. Simulations start without batching; real matches and
//! sim-bench turn it on ([`Simulation::batch_path_searches`]).
//!
//! The batch runs Find_Path's own steps up to its redirect
//! ([`Simulation::find_path_target_answer`]) and the core search, without
//! their effects. A redirect retargets the Foot's NavCom, which the core
//! search reads, so a request that redirects is left to the Foot's own
//! Find_Path. That Find_Path still runs at the Foot's turn, in native order,
//! and takes the answer in place of its core search only when the request
//! and the Foot's own search inputs are the ones the answer was computed for
//! ([`FootSearchKey`]); otherwise it searches as usual.
//!
//! The batch searches before Mark0 lifts the Foot off its cell; see
//! [`MoverLift::Emulated`] for what the search emulates and drops. The
//! blocker plane needs an overlay grid, through which movers reach it by
//! their Foot+55C counters, which Mark0 leaves alone. Lookups off the map
//! stamp the shared Dummy, so each search detaches it
//! (`SharedCellDummy::detached`).
//!
//! Residuals: batched searches don't stamp the shared Dummy's coordinate, so
//! a later read that doesn't stamp first sees the last serial stamp. Mark0's
//! recalculation of the Foot's cell normally re-derives the same attributes;
//! the code-2 chain reads that cell's bridge bit and level before it. A
//! terrain object's multi-cell foundation test sees the Foot still in its
//! cell when the foundation covers it.

use std::collections::BTreeMap;

use super::foot_path::MoverLift;
use super::movement_path::MovePathFailure;
use super::movement_tick::FootPathRequest;
use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionId;
use crate::sim::pathfinding::zone_search::PathSearchFailure;
use crate::sim::pathfinding::{BlockerNeighborCounts, PathGrid};
use crate::sim::radio::Contacts;
use crate::sim::world::Simulation;

/// The path searches waiting for the next frame, and this frame's answers.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct PathBatch {
    /// Foots whose request waited this frame, and which request each repeats.
    pending: Vec<(u64, WaitingRequest)>,
    /// This live object pass's answers by Foot. A Foot with an entry no
    /// longer waits; Find_Path takes out the answer it uses. Empty outside
    /// the pass, so a request made between passes waits whether or not the
    /// game was saved in between.
    #[serde(skip)]
    ready: BTreeMap<u64, Option<BatchedSearch>>,
    /// Answers Find_Path used, by [`WaitingRequest`].
    #[cfg(test)]
    #[serde(skip)]
    used: [usize; 2],
    /// The Foots the latest pass answered.
    #[cfg(test)]
    #[serde(skip)]
    answered: Vec<u64>,
}

impl std::hash::Hash for PathBatch {
    /// The registrations, which carry into the next frame; answers live
    /// within one.
    fn hash<H: std::hash::Hasher>(&self, hasher: &mut H) {
        (self.pending.len() as u64).hash(hasher);
        for &(id, request) in &self.pending {
            id.hash(hasher);
            (request as u8).hash(hasher);
        }
    }
}

/// The Find_Path request a waiting Foot repeats at its next frame's turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum WaitingRequest {
    /// The no-queue request, `Find_Path(cell, 0, 0)` toward Walk's
    /// destination or Drive/Ship's track destination.
    NoQueue,
    /// A code-2 ladder's re-path, at the ladder's urgency.
    Code2Ladder,
}

#[derive(Debug, PartialEq)]
struct BatchedSearch {
    request: WaitingRequest,
    key: FootSearchKey,
    found: Result<Vec<(u16, u16)>, MovePathFailure>,
}

/// What a core search reads from its request and its mover besides the
/// rest of the world: an answer serves only a request with the same key.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FootSearchKey {
    location: (i32, i32, i32),
    start: (u16, u16),
    on_bridge: bool,
    facing: u16,
    goal: (i32, i32, i32),
    urgency: u8,
    category: EntityCategory,
    type_ref: InternedId,
    speed_type: Option<SpeedType>,
    movement_zone: MovementZone,
    crushers: (bool, bool),
    snapshot_on_bridge: bool,
    locomotor: Option<LocomotorKind>,
    allow_zone_hierarchy: bool,
    threat_coefficient: u64,
    owner: InternedId,
    veterancy: u16,
    in_playfield: bool,
    mission: MissionId,
    nav_com: Option<NavTargetRef>,
    attack_target: Option<TargetKind>,
    radio_contacts: Contacts,
}

/// The destination a code-2 ladder searches toward: Walk's destination,
/// zero without one, or Drive/Ship's track destination.
pub(super) fn code2_ladder_destination(actor: &GameEntity) -> Option<DriveCoord> {
    let loco = actor.locomotor.as_ref()?;
    match loco.kind {
        LocomotorKind::Walk => {
            Some(
                loco.walk_destination()
                    .unwrap_or(DriveCoord { x: 0, y: 0, z: 0 }),
            )
        }
        LocomotorKind::Drive | LocomotorKind::Ship => super::track_path::track_destination(actor),
        _ => None,
    }
}

impl Simulation {
    /// Batch the path searches from now on; see the module docs.
    pub fn batch_path_searches(&mut self) {
        self.path_batch.get_or_insert_with(PathBatch::default);
    }

    /// Whether `id`'s `request` waits for the next frame's batch, as if its
    /// movement timer still ran. It does, and registers, when batching is
    /// on, the batch can search this map, and `id` has no answer this frame.
    pub(super) fn defer_path_search(&mut self, id: u64, request: WaitingRequest) -> bool {
        if self.overlay_grid.is_none() || self.resolved_terrain.is_none() {
            return false;
        }
        let Some(batch) = self.path_batch.as_mut() else {
            return false;
        };
        if batch.ready.contains_key(&id) {
            return false;
        }
        batch.pending.push((id, request));
        true
    }

    /// The batched answer for `request`'s core search toward `goal`, if one
    /// was computed for this request.
    pub(super) fn take_batched_search(
        &mut self,
        request: &FootPathRequest,
        goal: DriveCoord,
    ) -> Option<Result<Vec<(u16, u16)>, MovePathFailure>> {
        let id = request.entity_id;
        if !self
            .path_batch
            .as_ref()?
            .ready
            .get(&id)
            .is_some_and(Option::is_some)
        {
            return None;
        }
        let key = self.foot_search_key(request, goal)?;
        let batch = self.path_batch.as_mut()?;
        let slot = batch.ready.get_mut(&id)?;
        if slot.as_ref()?.key != key {
            return None;
        }
        let answer = slot.take()?;
        #[cfg(test)]
        {
            batch.used[answer.request as usize] += 1;
        }
        Some(answer.found)
    }

    /// The start of the live object pass: search for the Foots registered
    /// last frame.
    pub(crate) fn answer_batched_path_searches(
        &mut self,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let Some(batch) = self.path_batch.as_mut() else {
            return;
        };
        batch.ready.clear();
        #[cfg(test)]
        batch.answered.clear();
        let mut pending = std::mem::take(&mut batch.pending);
        if pending.is_empty() {
            return;
        }
        // A Foot's first registration this frame stands.
        pending.sort_by_key(|&(id, _)| id);
        pending.dedup_by_key(|&mut (id, _)| id);
        let answers = match (rules, self.path_grid_snapshot()) {
            (Some(rules), Some(grid))
                if self.overlay_grid.is_some() && self.resolved_terrain.is_some() =>
            {
                self.search_batch(&pending, rules, registry, &grid)
            }
            _ => Vec::new(),
        };
        let mut answers = answers.into_iter();
        let batch = self.path_batch.as_mut().expect("batching is on");
        #[cfg(test)]
        {
            batch.answered = pending.iter().map(|&(id, _)| id).collect();
        }
        batch.ready = pending
            .into_iter()
            .map(|(id, _)| (id, answers.next().flatten()))
            .collect();
    }

    /// The end of the live object pass: its answers expire.
    pub(crate) fn finish_batched_path_searches(&mut self) {
        if let Some(batch) = self.path_batch.as_mut() {
            batch.ready.clear();
        }
    }

    fn search_batch(
        &mut self,
        pending: &[(u64, WaitingRequest)],
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        grid: &PathGrid,
    ) -> Vec<Option<BatchedSearch>> {
        // One blocker plane for every search. With an overlay grid movers
        // reach it through their Foot+55C counters, which Mark0 leaves alone,
        // so it is the plane each Find_Path would read after its Mark0.
        self.movement_pass_cache.blocker_plane(
            &mut self.substrate.entities,
            grid,
            self.resolved_terrain.as_ref(),
            self.overlay_grid.as_ref(),
            &self.interner,
            Some(rules),
        );
        let sim = &*self;
        let counts = sim.movement_pass_cache.current_blocker_plane();
        let dummy = sim
            .resolved_terrain
            .as_ref()
            .expect("checked by the caller")
            .shared_cell_dummy();
        // Main is shared with retained playback, which may draw on another
        // thread, so only tests check that the searches draw nothing.
        #[cfg(test)]
        let rng = sim.rng_state();
        let answers = crate::sim::parallel::map(pending, |&(id, request)| {
            dummy.detached(|| sim.batched_search(id, request, rules, registry, grid, counts))
        });
        #[cfg(test)]
        assert_eq!(rng, sim.rng_state(), "batched searches draw no RNG");
        answers
    }

    /// `id`'s search as its `request`'s Find_Path would run it now, without
    /// effects; `None` when it has nothing to answer.
    fn batched_search(
        &self,
        id: u64,
        request: WaitingRequest,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        grid: &PathGrid,
        counts: &BlockerNeighborCounts,
    ) -> Option<BatchedSearch> {
        let waiting = request;
        let request = match waiting {
            WaitingRequest::NoQueue => self.no_queue_request(id)?,
            WaitingRequest::Code2Ladder => self.code2_ladder_request(id)?,
        };
        let goal = request.destination;
        let answer = self
            .find_path_target_answer(id, goal, rules, registry)
            .ok()??;
        if self
            .find_path_redirect(id, goal, answer, rules)
            .ok()?
            .is_some()
        {
            return None;
        }
        let (found, read_own_cell) = self.foot_core_search(
            &request,
            goal,
            rules,
            registry,
            grid,
            counts,
            MoverLift::Emulated,
        );
        if read_own_cell
            || matches!(
                found,
                Err(MovePathFailure::MissingGrid
                    | MovePathFailure::Search(PathSearchFailure::CellEntryUnavailable(_)))
            )
        {
            return None;
        }
        Some(BatchedSearch {
            request: waiting,
            key: self.foot_search_key(&request, goal)?,
            found,
        })
    }

    /// The no-queue request `id` makes now: Walk's toward its destination,
    /// Drive/Ship's toward its track destination, at urgency 0.
    fn no_queue_request(&self, id: u64) -> Option<FootPathRequest> {
        let actor = self.substrate.entities.get(id)?;
        let destination = match actor.locomotor.as_ref()?.kind {
            LocomotorKind::Walk => actor.locomotor.as_ref()?.walk_destination()?,
            LocomotorKind::Drive | LocomotorKind::Ship => {
                super::track_path::track_destination(actor)?
            }
            _ => return None,
        };
        FootPathRequest::track(
            &self.substrate.entities,
            id,
            destination,
            0,
            self.playfield_bounds,
        )
    }

    /// The request `id`'s code-2 ladder makes now, at the ladder's urgency.
    fn code2_ladder_request(&self, id: u64) -> Option<FootPathRequest> {
        let actor = self.substrate.entities.get(id)?;
        let destination = code2_ladder_destination(actor)?;
        let urgency = actor
            .navigation
            .path_runtime
            .code2_urgency(self.session.binary_frame);
        FootPathRequest::track(
            &self.substrate.entities,
            id,
            destination,
            urgency,
            self.playfield_bounds,
        )
    }

    /// The key of `request`'s core search toward `goal` as the world stands.
    fn foot_search_key(
        &self,
        request: &FootPathRequest,
        goal: DriveCoord,
    ) -> Option<FootSearchKey> {
        let actor = self.substrate.entities.get(request.entity_id)?;
        let snap = request.snapshot();
        let location = super::ground_pose::position_world_coord(&actor.position);
        Some(FootSearchKey {
            location: (location.x, location.y, location.z),
            start: (actor.position.rx, actor.position.ry),
            on_bridge: actor.on_bridge,
            facing: actor.body_facing.current(self.session.binary_frame),
            goal: (goal.x, goal.y, goal.z),
            urgency: request.urgency,
            category: snap.category,
            type_ref: actor.type_ref(),
            speed_type: snap.speed_type,
            movement_zone: snap.movement_zone,
            crushers: (snap.regular_crusher, snap.omni_crusher),
            snapshot_on_bridge: snap.on_bridge,
            locomotor: snap.locomotor.as_ref().map(|loco| loco.kind),
            allow_zone_hierarchy: snap.allow_zone_hierarchy,
            threat_coefficient: self.foot_path_threat_coefficient(request.entity_id).bits(),
            owner: actor.owner(),
            veterancy: actor.veterancy.rank(),
            in_playfield: actor.in_playfield,
            mission: actor.mission.current(),
            nav_com: actor.navigation.nav_com,
            attack_target: actor.attack_target.as_ref().map(|attack| attack.target),
            radio_contacts: actor.radio_contacts.clone(),
        })
    }
}

#[cfg(test)]
#[path = "path_batch_tests.rs"]
mod tests;
