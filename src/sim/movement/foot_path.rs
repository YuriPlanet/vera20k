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

use super::ground_pose;
use super::infantry_entry::InfantryEntryArgs;
use super::movement_tick::FootPathRequest;
use crate::map::cell_index::NativeCellIdentity;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone};
use crate::rules::mission_data::MissionType;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_kernel::{cell_center, native_xy_distance, native_xyz_distance};
use crate::sim::cell_rect::PlayfieldBounds;
use crate::sim::components::DriveCoord;
use crate::sim::find_nearby_cell::{NearbyArgs, NearbyFootprint, PassabilityArgs};
use crate::sim::mission::MissionId;
use crate::sim::mission::authority::EntityReadyInputProvider;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::pathfinding::CODE2_CHAIN_MAX_HOPS;
use crate::sim::pathfinding::zone_map::{ZoneGrid, ZoneQueryCell};
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;
use crate::util::direction::DIRECTION_DELTAS;
use crate::util::direction_tables::dir_from_facing16;
use crate::util::fixed_math::SIM_ZERO;

/// AStar429F54's live Foot +1AC receiver: Infantry 0x0051BF90 or Unit
/// 0x0073F0A0, the one class port behind `Simulation::foot_can_enter`, with
/// the mover's own facts resolved once per search. The fifth native argument
/// is unused by both receivers; direction, signed path height and previous
/// Cell reach the same decision owner used by Walk, Drive/Ship and bridge
/// repair. The raw code drives admission and cost at 0x00429F5A..FED; a
/// code 2 is priced by the chain over the live cell lists
/// ([`Simulation::astar_code2_chain_jams`]).
struct FootSearchEntry<'a> {
    sim: &'a Simulation,
    receiver: Result<crate::sim::world::FootEntryReceiver<'a>, String>,
    threat_coefficient: crate::util::native_x87::NativeF64Bits,
    markers: super::path_markers::DeferredBridgeMarkerSearch<'a>,
    /// The mover, when Mark0 has not lifted it ([`MoverLift::Emulated`]):
    /// the code-2 chain skips it in cell lists.
    unlisted: Option<u64>,
    /// The unlifted mover's own cell. Lifting it would change any answer
    /// about that cell, so asking one marks the search.
    own_cell: Option<(i16, i16)>,
    read_own_cell: std::cell::Cell<bool>,
    /// A Unit whose type is `Passive=`.
    passive_unit: bool,
}

/// Whether Mark0 has lifted the searching mover off its cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MoverLift {
    /// Find_Path's own order: Mark0 (0x4D3E11) ran before the search.
    Marked,
    /// A batched path search ([`super::path_batch`]) runs before Mark0,
    /// which unlinks the mover from its cell's lists, clears its raw
    /// occupation bits and vehicle mark there and recalculates that cell.
    /// The search skips the mover in cell lists and reports whether it asked
    /// Can_Enter_Cell about the mover's own cell, whose other state Mark0
    /// changes; the batch drops such an answer.
    Emulated,
}

impl crate::sim::pathfinding::SearchFootEntry for FootSearchEntry<'_> {
    fn classify(&self, query: crate::sim::pathfinding::SearchEntryQuery) -> Result<u8, String> {
        let terrain = self
            .sim
            .resolved_terrain
            .as_ref()
            .ok_or("Foot search requires map cells")?;
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
        if let Some(own) = self.own_cell
            && let NativeCellIdentity::Real(_) = candidate
            && terrain.native_cell_coord(candidate) == own
        {
            self.read_own_cell.set(true);
        }
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
    fn code2_chain_jams(&self, cell: (u16, u16), layer: MovementLayer) -> Result<bool, String> {
        self.sim.astar_code2_chain_jams(cell, layer, self.unlisted)
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
    fn is_passive_unit(&self) -> bool {
        self.passive_unit
    }
    fn finishing_threat_coefficient(
        &self,
    ) -> Result<crate::util::native_x87::NativeF64Bits, String> {
        Ok(self.threat_coefficient)
    }
    fn finishing_house_threat(&self, coord: (i16, i16)) -> Result<i32, String> {
        let terrain = self
            .sim
            .resolved_terrain
            .as_ref()
            .ok_or("Foot finishing requires map cells")?;
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

impl Simulation {
    /// The urgency-0 code-2 chain of `AStar_compute_edge_cost @ 0x00429830`
    /// (`0x00429886..0x00429984`): whether the traffic ahead of `cell`'s
    /// `layer` list jams. It follows the first object of each list:
    /// - none clears, and a non-Foot (`+0x14` bit 2 clear) jams;
    /// - a Foot with a zero speed fraction (`+0x578`) heads for its path's
    ///   head word (`+0x5E0`, masked to a direction at `0x004298F1`), and one
    ///   without a head clears; any other Foot heads along its PrimaryFacing
    ///   (`+0x388`, `FacingClass::Current`) rounded to eight ways;
    /// - it moves to the AdjacentCell (`0x0089F688`) of the Foot's own cell
    ///   (vt+0x1B8): that cell's bridge list when the cell has a bridge
    ///   (`+0x140 & 0x100`) and the Foot is on a bridge (`+0x8C`) or its own
    ///   cell's level (vt+0x1BC, `+0x11B`) is more than two above the cell's,
    ///   else its ground list. The shared dummy of an off-map cell lists
    ///   nothing.
    ///
    /// After [`CODE2_CHAIN_MAX_HOPS`] Foots it jams, without testing the list
    /// the last one heads for. The lists skip `unlisted`, a mover Mark0 has
    /// not lifted yet ([`MoverLift::Emulated`]).
    pub(crate) fn astar_code2_chain_jams(
        &self,
        cell: (u16, u16),
        layer: MovementLayer,
        unlisted: Option<u64>,
    ) -> Result<bool, String> {
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("the code-2 chain requires map cells")?;
        let frame = self.session.binary_frame();
        let mut list = Some((cell, layer));
        for _ in 0..CODE2_CHAIN_MAX_HOPS {
            let Some(object) = list.and_then(|(cell, layer)| {
                self.cell_objects(cell, layer).find(|object| {
                    !matches!(*object, CellObjectMember::Entity(id) if Some(id) == unlisted)
                })
            }) else {
                return Ok(false);
            };
            // A terrain object is no Foot.
            let CellObjectMember::Entity(id) = object else {
                return Ok(true);
            };
            let foot = self
                .substrate
                .entities
                .get(id)
                .ok_or("a cell list names a retired object")?;
            if !foot.category().is_foot() {
                return Ok(true);
            }
            let direction = if foot.foot_speed.applied_fraction() == SIM_ZERO {
                let Some(&word) = foot.navigation.path_replay.remaining_directions().first() else {
                    return Ok(false);
                };
                word & 7
            } else {
                dir_from_facing16(foot.body_facing_current(frame))
            };
            let (dx, dy) = DIRECTION_DELTAS[usize::from(direction)];
            let own = (foot.position.rx() as i16, foot.position.ry() as i16);
            let next = terrain.native_cell_identity((
                own.0.wrapping_add(dx as i16),
                own.1.wrapping_add(dy as i16),
            ));
            let level = |cell| i32::from(terrain.native_cell_ground_fields(cell).0 as i8);
            let bridge = terrain.native_cell_flags(next) & 0x100 != 0
                && (foot.on_bridge() || level(terrain.native_cell_identity(own)) - level(next) > 2);
            list = match next {
                NativeCellIdentity::Real(_) => {
                    let (x, y) = terrain.native_cell_coord(next);
                    let layer = if bridge {
                        MovementLayer::Bridge
                    } else {
                        MovementLayer::Ground
                    };
                    Some(((x as u16, y as u16), layer))
                }
                NativeCellIdentity::Dummy => None,
            };
        }
        Ok(true)
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

pub(crate) fn coord_cell(coord: DriveCoord) -> (i16, i16) {
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
    /// 0x75AFD3). Drive/Ship requests are made inside their Process_Movement
    /// (`track_fresh`).
    pub(crate) fn run_foot_path_request(
        &mut self,
        request: &FootPathRequest,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<FootPathOutcome, String> {
        // A batched request waits for the next frame's batch as if the +640
        // timer still ran (`path_batch`, a proposed deviation): 75AF4B..75AF66
        // calls the owner's vt+0x548 and returns. So does the admission
        // retry's recursion, whose +640 the retry just expired, unless it is
        // the Foot's first request in a frame the batch answered.
        if self.defer_path_search(
            request.entity_id,
            super::path_batch::WaitingRequest::NoQueue,
        ) {
            self.walk_owner_clear_doing(request.entity_id)?;
            return Ok(FootPathOutcome::Returned);
        }
        self.run_walk_path_request(request, rules, registry, frame_effects)
            .map(|resumed| {
                if resumed {
                    FootPathOutcome::Resume
                } else {
                    FootPathOutcome::Returned
                }
            })
    }

    /// `Find_Path(cell, 0, 0)` for a no-queue caller whose own movement
    /// timer is already armed; see [`FindPathResult`].
    pub(crate) fn foot_find_path(
        &mut self,
        request: &FootPathRequest,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<FindPathResult, String> {
        let id = request.entity_id;
        let frame = self.session.binary_frame();
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
                    queue.set_directions_for_test(words);
                    queue.set_cursor(0);
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
        let Some(answer) =
            self.find_path_target_answer(id, request.destination, rules, registry)?
        else {
            //4D3989..399C returns false before Mark, AStar or +500.
            return Ok(FindPathResult::Failed);
        };
        let goal = self.find_path_goal_for_answer(
            id,
            request.destination,
            answer,
            rules,
            registry,
            frame_effects,
        )?;
        match self.search_foot_path(request, goal, rules, registry, frame_effects)? {
            Ok(()) => Ok(FindPathResult::Route),
            Err(refusal) => {
                if let CoreRefusal::VeraOnly(reason) = refusal {
                    //Residual: a VERA-only search refusal (the compatibility
                    //zone graph, a hierarchy cell without native topology)
                    //has no native counterpart; it takes the native NULL-core
                    //continuation below instead of aborting the frame.
                    //Trigger: a goal or corridor those adapters refuse.
                    //Effect: the Foot stops (Guard/AreaGuard) where
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
                self.foot_stop_driver(id, rules, registry, frame_effects)?;
                self.finish_find_path_failure(id, goal, rules, registry, frame_effects)?;
                Ok(FindPathResult::Failed)
            }
        }
    }

    /// Shared virtual `+0x500` receiver, called by FindPath4D4044,
    /// Infantry FireAt and first-Limbo4DB2FB. Infantry
    /// 0x0051DAF0 performs Do_Action and the current-cell answer before Foot
    /// 0x4D55C0; the Unit's and the Aircraft's slot IS 0x4D55C0, which only
    /// calls the active locomotor's Stop_Moving (+0x48,
    /// [`Self::locomotor_stop_moving`]).
    pub(crate) fn foot_stop_driver(
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
            .ok_or("retired Foot Stop_Driver receiver")?;
        match actor.category() {
            EntityCategory::Infantry => {
                self.infantry_stop_driver(id, rules, registry, frame_effects)
            }
            EntityCategory::Unit | EntityCategory::Aircraft => {
                self.locomotor_stop_moving(id, Some(rules), registry, frame_effects)
            }
            EntityCategory::Structure => {
                Err("Foot +0x500 receiver for this class is not represented".into())
            }
        }
    }

    /// Mark0, the core search, Mark1 and the +640 rewrite; a found route is
    /// installed with its 0x4D4003 reference cell. Search refusals return
    /// through the inner `Err`; an unavailable PathGrid stops the frame.
    fn search_foot_path(
        &mut self,
        request: &FootPathRequest,
        goal: DriveCoord,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<Result<(), CoreRefusal>, String> {
        let id = request.entity_id;
        let frame = self.session.binary_frame();
        if self.path_grid.is_none() {
            return Err("Find_Path core requires PathGrid; no native failure inferred".into());
        }
        self.foot_mark_remove(id, Some(rules), registry, frame_effects);
        #[cfg(test)]
        self.export_bridge_engineer_entry_inputs(id, rules, "mark0");
        let searched = match self.take_batched_search(request, goal) {
            Some(searched) => searched,
            None => {
                //The blocker plane must see Mark0, including the actor's
                //removal: the kept plane follows it through the touch log
                //instead of a whole-world rebuild per request.
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
                //The track_fresh_response oracle substitutes the whole AStar
                //core 0x4CBBA0 for a CoreNull row, so no +1AC call happens
                //inside it; the replay must not run the search that would
                //consume those answers.
                #[cfg(test)]
                let core_null = super::fresh_oracle_seam::take_core_null();
                #[cfg(not(test))]
                let core_null = false;
                if core_null {
                    Err(super::movement_path::MovePathFailure::Search(
                crate::sim::pathfinding::zone_search::PathSearchFailure::CellSearchExhausted,
            ))
                } else {
                    self.foot_core_search(
                        request,
                        goal,
                        rules,
                        registry,
                        grid,
                        counts,
                        MoverLift::Marked,
                        None,
                    )
                    .0
                }
            }
        };
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
            Err(MovePathFailure::MissingGrid) => {
                Err("Find_Path core requires PathGrid; no native failure inferred".into())
            }
            Err(MovePathFailure::Search(Search::CellEntryUnavailable(cause))) => Err(cause),
        }
    }

    /// The core search (AStar 0x0042C900 with its bridge markers and both
    /// finishing passes) for `request` toward `goal` on the current world,
    /// reading the blocker counts `counts` and a path batch's kept entry
    /// `facts`. The flag says whether an [`MoverLift::Emulated`] search asked
    /// about the mover's own cell.
    pub(super) fn foot_core_search(
        &self,
        request: &FootPathRequest,
        goal: DriveCoord,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        grid: &crate::sim::pathfinding::PathGrid,
        counts: &crate::sim::pathfinding::BlockerNeighborCounts,
        lift: MoverLift,
        facts: Option<&crate::sim::world::EntryFacts>,
    ) -> (
        Result<Vec<(u16, u16)>, super::movement_path::MovePathFailure>,
        bool,
    ) {
        let id = request.entity_id;
        let frame = self.session.binary_frame();
        // Original42ACF0 runs at429C1A after FootMark0 (4D3E11), before
        // reconstruction and both finishing passes. Reuse its one marker
        // owner against the live ordered peer lists/raw bytes here; the
        // returned overlay stays alive through the finishing passes and is
        // discarded on success or failure before FootMark1.
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
        let own_cell = (actor.position.rx(), actor.position.ry());
        let unlisted = (lift == MoverLift::Emulated).then_some(id);
        let marker_search = marker_context.defer_search(
            &self.substrate.occupancy,
            id,
            own_cell,
            actor.body_facing.current(frame),
            actor.on_bridge(),
            request.urgency,
            unlisted,
        );
        // A requester outside Infantry and Unit has no receiver, so its
        // search fails at the first class question.
        let entry = FootSearchEntry {
            sim: self,
            markers: marker_search,
            receiver: self.foot_entry_receiver(id, rules, registry, facts),
            threat_coefficient: self.foot_path_threat_coefficient(id),
            unlisted,
            own_cell: unlisted.map(|_| (own_cell.0 as i16, own_cell.1 as i16)),
            read_own_cell: std::cell::Cell::new(false),
            passive_unit: actor.category() == EntityCategory::Unit
                && self
                    .interner
                    .object_type(actor.type_ref(), rules)
                    .is_some_and(|object| object.passive),
        };
        let searched = request.search(
            goal,
            &self.substrate.entities,
            super::PathfindingContext {
                path_grid: Some(grid),
                zone_grid: self.zone_grid.as_ref(),
                resolved_terrain: self.resolved_terrain.as_ref(),
                playfield_bounds: self.playfield_bounds,
                blocker_neighbor_counts: Some(counts),
            },
            &entry,
        );
        (searched, entry.read_own_cell.get())
    }

    /// The live Foot's threat coefficient for its searches (Foot 0x4DC760),
    /// with its Team's AvoidThreats.
    pub(super) fn foot_path_threat_coefficient(
        &self,
        id: u64,
    ) -> crate::util::native_x87::NativeF64Bits {
        self.substrate
            .entities
            .get(id)
            .expect("live Foot")
            .navigation
            .path_threat_coefficient_for_team(self.team_script_vm.member_avoids_threats(id))
    }

    /// `Find_Path` failure continuation 0x4D404A..0x4D41F0 after the receiver:
    /// a target within one cell (Chebyshev over the `+9C` cell) returns unless
    /// the target cell is structural while the actor is off a bridge
    /// (0x4D40AB..0x4D40D4); otherwise Team detachment, `SetDestination(NULL,1)`
    /// (+0x480: Infantry 0x51AA40, Unit 0x741970), `Assign_Target(NULL)`
    /// (+0x3C8: Infantry 0x51B1F0, Unit Techno 0x6FCDB0), then the
    /// House 0x50B730 split: human -> `Queue_Mission(Guard=5, 0)` (0x4D41D6),
    /// nonhuman -> `Queue_Mission(AreaGuard=11, 0)` and, with GameMode != 0,
    /// its own House's post (`Find_Passable_Cell_Near_Unit` 0x500200,
    /// [`Self::house_return_cell`]) -> `SetDestination(cell, 1)`.
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
            if actor.on_bridge() {
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
        //4D4149 dispatches the concrete target setter, including an owned
        //Airstrike's cancellation before its TarCom changes.
        self.assign_target_represented(id, None, Some(rules), registry, frame_effects)
            .map_err(|_| "retired failed Find_Path actor")?;
        let human = self.owner_is_human(owner);
        let mission = if human {
            MissionType::Guard
        } else {
            MissionType::AreaGuard
        };
        let now = self.session.binary_frame();
        // Queue's own guards decide; with commence 0 only a missing receiver
        // fails, and that receiver was just resolved.
        let _ = self.mission_queue_exact(
            id,
            MissionId::from_known(mission),
            0,
            now,
            &EntityReadyInputProvider,
        );
        //0x4D4174..0x4D41C2: with GameMode ([0xA8B238]) nonzero, a nonhuman
        //actor asks its own House (ecx = +0x21C) for a post, House500200 at
        //0x4D4189, and unless that is the zero Cell (0x8B3D88) is sent there:
        //SetDestination(cell, 1) through vt+0x480.
        if !human
            && self.session.game_mode_nonzero()
            && let Some((rx, ry)) = self.house_return_cell(owner, id)
        {
            self.assign_destination_represented(
                id,
                Some(crate::sim::components::NavTargetRef::cell(rx, ry)),
                Some(rules),
                registry,
                frame_effects,
            )
            .map_err(|cause| format!("Find_Path failure post: {cause}"))?;
        }
        Ok(())
    }

    /// `Find_Path` 0x4D3920 up to its redirect, without effects: the +2CC
    /// zone precheck, `None` when it refuses (0x4D3989..0x4D399C), then the
    /// target's `Can_Enter_Cell` answer (the class +0x1AC with facing -1,
    /// level -1, no source and flag 1, 0x4D3A6F..0x4D3A8C), which selects the
    /// redirect ([`Self::find_path_goal_for_answer`]).
    pub(super) fn find_path_target_answer(
        &self,
        id: u64,
        destination: DriveCoord,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<Option<u8>, String> {
        if !self.foot_path_zone_precheck(id, destination, rules)? {
            return Ok(None);
        }
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Find_Path goal requires map cells")?;
        let target = ((destination.x / 256) as i16, (destination.y / 256) as i16);
        let cell = terrain.native_cell_identity(target);
        self.foot_can_enter(id, cell, InfantryEntryArgs::REPAIR, rules, registry)
            .map(Some)
    }

    /// The redirect selection of 0x4D3A92..0x4D3E0A for an already computed
    /// raw target `Can_Enter_Cell` code: 6 and 7 have arms, every other code
    /// keeps the target. A redirect's `SetDestination(cell, 1)` runs before
    /// the search.
    pub(crate) fn find_path_goal_for_answer(
        &mut self,
        id: u64,
        destination: DriveCoord,
        answer: u8,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<DriveCoord, String> {
        let Some(near) = self.find_path_redirect(id, destination, answer, rules)? else {
            return Ok(destination);
        };
        self.redirect_find_path_destination(id, near, rules, registry, frame_effects)?;
        Ok(DriveCoord::from(cell_center(near.into(), 0)))
    }

    /// The cell [`Self::find_path_goal_for_answer`] retargets the search to,
    /// if its code-6 or code-7 arm redirects; it changes nothing. Code 6
    /// beyond CloseEnough (Rules+1718; a Team member reads Stray +171C or
    /// RelaxedStray +1720 through 0x6F03B0) asks FNPC for a cell near the
    /// target, accepts it only when it lies closer to the target than the
    /// actor (0x4D3C39) and `EstimateZoneCost` 0x42D170 admits it (at most
    /// Chebyshev plus 6). Code 7 with a Building in the target cell
    /// (0x47C520) redirects unconditionally.
    pub(crate) fn find_path_redirect(
        &self,
        id: u64,
        destination: DriveCoord,
        answer: u8,
        rules: &RuleSet,
    ) -> Result<Option<(i16, i16)>, String> {
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
                let centre = cell_center(target.into(), 0);
                let distance =
                    native_xyz_distance(coord.x - centre.x, coord.y - centre.y, coord.z - centre.z);
                //0x4D3A9B: dist <= that distance keeps the target. IsTrain (+C94)
                //is set by no retail TechnoType (no IsTrain=yes in rulesmd).
                if distance <= keep_within {
                    return Ok(None);
                }
                let Some(near) = self.find_path_nearby_cell(id, target, rules)? else {
                    //0x4D3BC2: an invalid (0,0) FNPC cell keeps the target.
                    return Ok(None);
                };
                //0x4D3BDD..0x4D3C3D: the FNPC cell must lie closer to the target
                //than the actor does (both x87 lepton distances, ftol).
                let near_centre = cell_center(near.into(), 0);
                let between =
                    native_xy_distance(centre.x - near_centre.x, centre.y - near_centre.y);
                if between >= distance {
                    return Ok(None);
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
                    return Ok(None);
                }
                Ok(Some(near))
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
                    return Ok(None);
                }
                //0x4D3DD8..0x4D3E03: the FNPC result is set and searched without
                //an invalid-cell test; only an FNPC failure over the whole map
                //radius would search cell (0,0), which no test fixture produces.
                let near = self.find_path_nearby_cell(id, target, rules)?.ok_or(
                    "Find_Path code-7 redirect found no passable cell near the Building target",
                )?;
                Ok(Some(near))
            }
            _ => Ok(None),
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
        self.map_size_diamond()
            .ok_or("Find_Path FNPC requires original Map Size")?;
        let current = coord_cell(ground_pose::position_world_coord(&actor.position));
        let current = (i32::from(current.0), i32::from(current.1));
        let required_zone_id = zones.get_zone_id_native(
            terrain,
            (current.0 as u16, current.1 as u16),
            zone,
            actor.on_bridge(),
        );
        Ok(self
            .nearby_passable_cell(
                (i32::from(target.0), i32::from(target.1)),
                NearbyArgs {
                    check_height: true,
                    target_cell: Some(current),
                    ..NearbyArgs::plain(
                        PassabilityArgs {
                            speed_type: object.speed_type,
                            required_zone_id,
                            movement_zone: zone,
                            bridge_aware_zone: actor.on_bridge(),
                        },
                        NearbyFootprint::SINGLE,
                    )
                },
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
    /// 0x4D3DF9): the class setter vt+0x480 an order reaches
    /// ([`Simulation::assign_destination_represented`]; Infantry 0x51AA40 ->
    /// Walk 0x75ACB0, Unit 0x741970 -> Drive 0x4AFD40 / Ship 0x69F450), which
    /// installs NavCom and the locomotor destination without a search of its
    /// own.
    fn redirect_find_path_destination(
        &mut self,
        id: u64,
        cell: (i16, i16),
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<(), String> {
        let requested = crate::sim::components::NavTargetRef::cell(cell.0 as u16, cell.1 as u16);
        self.assign_destination_represented(
            id,
            Some(requested),
            Some(rules),
            registry,
            frame_effects,
        )
        .map_err(|cause| format!("Find_Path redirect: {cause}"))
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
        if !actor.in_playfield() {
            return Ok(false);
        }
        if actor.is_mission_only() || actor.mission.effective().raw() == 4 {
            return Ok(true);
        }
        Ok(self.member_team_is_moving_outside_playfield(id))
    }

    /// `FootClass::IsInSameZoneAs @ 0x004DBA50` (vt+0x2C4): the target's
    /// coordinate for this mover (its vt+0x4C with the mover as requester,
    /// `0x004DBA63`), cut to its cell toward zero, whose centre at height 0
    /// takes the zone pre-check ([`Self::foot_path_zone_precheck`],
    /// vt+0x2CC at `0x004DBAB8`).
    pub(crate) fn foot_is_in_same_zone_as(
        &self,
        id: u64,
        target: crate::sim::combat::TargetKind,
        rules: &RuleSet,
    ) -> Result<bool, String> {
        let coord = super::navcom::nav_target_coordinate(
            target.into(),
            Some(id),
            self,
            self.resolved_terrain.as_ref(),
            Some((rules, &self.interner)),
        )?;
        let centre = DriveCoord::from(cell_center(coord_cell(coord).into(), 0));
        self.foot_path_zone_precheck(id, centre, rules)
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
        let object = self
            .interner
            .object_type(actor.type_ref(), rules)
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
        let object = self
            .interner
            .object_type(actor.type_ref(), rules)
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
            .map_size_diamond()
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
                on_bridge: actor.on_bridge(),
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

    /// `AStar @ 0x0042CAD6` takes the zone hierarchy only for a mover in the
    /// playfield (Techno+0x3D5), which the Foot search snapshots with its
    /// request. With the zones split between start and goal, a mover outside
    /// the playfield still finds the flat route; one inside it is refused at
    /// the hierarchy.
    #[test]
    fn foot_search_takes_the_zone_hierarchy_only_inside_the_playfield() {
        for in_playfield in [false, true] {
            let (mut sim, rules, registry) =
                crate::sim::world::entry_test_fixture::fixture_with_rules(
                    "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=400\nSpeed=6\nSpeedType=Track\nMovementZone=Normal\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n",
                );
            let (start, goal_cell) = ((13, 15), (17, 15));
            let id = sim
                .spawn_object("MTNK", "Americans", start.0, start.1, 0, &rules)
                .unwrap();
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .set_in_playfield(in_playfield);
            let mut split = sim.path_grid.as_deref().unwrap().clone();
            for y in 0..split.height() {
                split.set_blocked(15, y, true);
            }
            let zones = ZoneGrid::following_path_grid(&split);
            assert!(!zones.can_reach(
                MovementZone::Normal,
                start,
                MovementLayer::Ground,
                goal_cell,
                MovementLayer::Ground,
            ));
            sim.zone_grid = Some(zones);
            let goal = DriveCoord::cell(goal_cell.0, goal_cell.1, 0);
            let request =
                FootPathRequest::track(&sim.substrate.entities, id, goal, 0, sim.playfield_bounds)
                    .unwrap();
            let found = sim
                .search_foot_path(
                    &request,
                    goal,
                    &rules,
                    Some(&registry),
                    FrameEffects::default(),
                )
                .unwrap();
            let route = sim
                .substrate
                .entities
                .get(id)
                .unwrap()
                .navigation
                .path_replay
                .route_cells();
            assert_eq!(found.is_ok(), !in_playfield, "in_playfield={in_playfield}");
            assert_eq!(
                route.last() == Some(&goal_cell),
                !in_playfield,
                "in_playfield={in_playfield}: {route:?}"
            );
        }
    }

    /// The failure arm's House split (`0x004D4156..0x004D41C2`): a human's
    /// Foot queues Guard and stays. A computer's queues Area Guard and, in a
    /// nonzero game mode, is sent to the post its own House draws
    /// (House500200 at `0x004D4189`, two Scenario draws); in game mode 0 it
    /// stays and draws nothing.
    #[test]
    fn a_failed_search_sends_a_computer_foot_to_its_house_post() {
        use crate::sim::components::NavTargetRef;
        use crate::sim::mission::MissionDispatchTimer;
        use crate::sim::mission::state::MissionTestFixture;

        for (human, skirmish) in [(true, true), (false, false), (false, true)] {
            let world = || {
                let (mut sim, rules, registry) =
                    crate::sim::world::entry_test_fixture::fixture_with_rules(
                        "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=400\nSpeed=6\nSpeedType=Track\nMovementZone=Normal\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n",
                    );
                sim.session.set_game_mode(
                    crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(skirmish),
                );
                let owner = sim.intern("Americans");
                let mut house =
                    crate::sim::house_state::HouseState::new(owner, 0, None, human, 0, 10);
                house.base_center = Some((16, 16));
                sim.houses.register(house);
                let id = sim
                    .spawn_object("MTNK", "Americans", 13, 15, 0, &rules)
                    .unwrap();
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .mission
                    .apply_test_fixture(MissionTestFixture {
                        current: MissionId::from_known(MissionType::Move),
                        suspended: MissionId::NONE,
                        queued: MissionId::NONE,
                        movement_bypass_latch: 0,
                        handler_state: 0,
                        mission_start_frame: 0,
                        ai_counter: 0,
                        dispatch_timer: MissionDispatchTimer::at_frame(0),
                    });
                (sim, rules, registry, id)
            };
            let (mut twin, _, _, twin_id) = world();
            let owner = twin.substrate.entities.get(twin_id).unwrap().owner();
            let post = twin
                .house_return_cell(owner, twin_id)
                .expect("a post near the base");
            let (mut sim, rules, registry, id) = world();
            sim.finish_find_path_failure(
                id,
                DriveCoord::cell(28, 28, 0),
                &rules,
                Some(&registry),
                FrameEffects::default(),
            )
            .unwrap();
            let sent = !human && skirmish;
            let e = sim.substrate.entities.get(id).unwrap();
            assert_eq!(
                e.mission.queued().known(),
                Some(if human {
                    MissionType::Guard
                } else {
                    MissionType::AreaGuard
                }),
                "human {human}, skirmish {skirmish}"
            );
            assert_eq!(
                e.navigation.nav_com(),
                sent.then(|| NavTargetRef::cell(post.0, post.1)),
                "human {human}, skirmish {skirmish}"
            );
            assert_eq!(
                sim.rng_state() == twin.rng_state(),
                sent,
                "only the post draws: human {human}, skirmish {skirmish}"
            );
        }
    }
}

#[cfg(test)]
#[path = "foot_path_marker_integration_tests.rs"]
mod marker_integration_tests;

#[cfg(test)]
#[path = "foot_path_threat_integration_tests.rs"]
mod threat_integration_tests;

#[cfg(test)]
#[path = "foot_path_code2_chain_tests.rs"]
mod code2_chain_tests;
