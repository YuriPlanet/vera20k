//! Native TubeClass movement for explicit map tubes.
//!
//! gamemd: `UnitClass::TubeMovement` 0x007359F0, which reads the cell's tube
//! through `CellClass::GetTubeAtCell` 0x00484F20 and, on exit, drives
//! `FacingClass::UpdateFacing` from the record's +0x2C direction field.
//!
//! Automatic tunnel/low-bridge records have a zero-length path. They are
//! predicate and zone metadata only: the retail producer divides by path
//! length, so a zero-step shell must never become active movement state.

use super::infantry_entry::InfantryEntryArgs;
use super::track_process::TrackFamily;
use super::{MovementTickStats, PerCellReason, walk_head};
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainGrid};
use crate::map::tube_facts::{TubeFact, TubeId, TubeSource};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_kernel;
use crate::sim::components::{DriveCoord, NavTargetRef, Position};
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::StringInterner;
use crate::sim::movement::ScatterFlags;
use crate::sim::movement::ground_pose;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::world::{FrameAdvanceError, FrameEffects, Simulation};
use crate::util::fixed_math::{SIM_ONE, SIM_ZERO};
use crate::util::native_x87::{NativeF64Bits, X87Chop53, distance_3d_leptons};

/// FootClass-owned active TubeMovement payload.
///
/// `Some` also means the mover is absent from CellClass object lists and raw
/// occupation, but inside an exit: the Infantry exit lists and marks the
/// infantryman (`0x0051BA22`, `0x0051BA45`) and runs its self-Scatter before
/// it clears the tube (`0x0051BA8D`), as
/// [`Simulation::infantry_tube_exit`] does. Immutable path bytes remain in
/// the map-owned [`TubeFact`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct LowBridgeTubeMovementState {
    tube_id: TubeId,
    cursor: u8,
    target: DriveCoord,
}

impl LowBridgeTubeMovementState {
    /// The tube (FootClass+0x684).
    pub fn tube_id(&self) -> TubeId {
        self.tube_id
    }

    /// The step along the tube's path.
    #[cfg(test)]
    pub fn cursor(&self) -> u8 {
        self.cursor
    }

    /// A fixture's transit.
    #[cfg(any(test, feature = "test-support"))]
    pub fn for_test(tube_id: TubeId, cursor: u8, target: DriveCoord) -> Self {
        Self {
            tube_id,
            cursor,
            target,
        }
    }
}

/// What a tube check finds for a path head word of 8 ([`tube_word`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TubeWord {
    /// The Cell's tube index (`+0x116`) is a live tube.
    Enter(TubeId),
    /// The index is out of range: -1, or past the last tube.
    NoTube,
}

/// The tube check of the path head word (Foot `+0x5E0`), Walk's
/// (`0x0075B302..0x0075B356`) and Drive's (`0x004B1297`,
/// `0x004B1302..0x004B1322`; Ship twin `0x006A095F`, `0x006A09CA..0x006A09EA`):
/// a word of 8 reads the tube index (`+0x116`) of the Cell holding the
/// Location (Walk `0x0075B338`, Drive's GetCell vt+0x1BC `0x005F6960`), on any
/// layer. `None` when the word is not a tube word.
///
/// RESIDUAL: an index past 127 (the `+0x684` byte) or an automatic
/// zero-length record (module doc) is not entered, and the head falls
/// through to the admission or the paid track. Native enters it, then
/// truncates the byte or divides by the zero path length (Walk `0x0075B53D`,
/// Drive `0x004B1474`). Trigger: a tube word at such a Cell. Effect: VERA
/// takes the word's step where native misreads the tube or faults.
/// Frequency: not measured; no stock-map tube census exists.
fn tube_word(
    path_replay: &crate::sim::components::FootPathQueue,
    position: &Position,
    terrain: &ResolvedTerrainGrid,
) -> Option<TubeWord> {
    if path_replay.remaining_directions().first()
        != Some(&crate::util::direction::TUBE_STEP_DIRECTION)
    {
        return None;
    }
    let Some((tube_id, tube)) = terrain
        .cell(position.rx(), position.ry())
        .and_then(|cell| cell.tube_index)
        .and_then(|tube_id| Some((tube_id, terrain.tube(tube_id)?)))
    else {
        return Some(TubeWord::NoTube);
    };
    (tube_id.0 <= i8::MAX as u16 && tube.source == TubeSource::ExplicitMap && tube.path_len() > 0)
        .then_some(TubeWord::Enter(tube_id))
}

/// The exit Cell's centre at Z 0, the head each entry stores (Walk
/// `0x0075B3CA..0x0075B3D2`, Drive `0x004B1332..0x004B135A`, Ship
/// `0x006A09FA..0x006A0A22`).
fn tube_exit_centre(tube: &TubeFact) -> DriveCoord {
    DriveCoord {
        x: i32::from(tube.exit.0 as i16) * 256 + 128,
        y: i32::from(tube.exit.1 as i16) * 256 + 128,
        z: 0,
    }
}

/// The first tube target's Z (Walk `0x0075B4E6..0x0075B544`, Drive
/// `0x004B13FB..0x004B147A`, Ship `0x006A0AC3..0x006A0B42`): the floor under
/// the Location (`0x00578080`) plus one step's share of the rise to the exit
/// centre's floor (path length `+0x1C0`).
///
/// A Location VERA holds without an exact Z takes the one SetHeight(0)
/// gives it, the floor plus the deck when OnBridge, so the steps measure
/// from it.
fn tube_first_z(
    position: &mut Position,
    on_bridge: bool,
    tube: &TubeFact,
    terrain: &ResolvedTerrainGrid,
) -> Result<i32, String> {
    let steps = i32::try_from(tube.path_len()).map_err(|_| "tube path too long")?;
    let [x, y] = ground_pose::position_world_xy(position);
    let cells = NativeCellQuery::canonical(terrain);
    let ground = ground_pose::query_ground_height(&cells, DriveCoord { x, y, z: 0 })?;
    let exit = ground_pose::query_ground_height(&cells, tube_exit_centre(tube))?;
    if position.exact_z_leptons().is_none() {
        position.put_z(ground_pose::z_at_height(ground, 0, on_bridge));
    }
    Ok(ground.wrapping_add(exit.wrapping_sub(ground) / steps))
}

/// Walk's first tube target (`0x0075B40C..0x0075B544`): the Location moved
/// by the first path step (the next Cell's centre less the entry Cell's,
/// `0x005657A0`, `0x00480A30`), at [`tube_first_z`].
fn walk_tube_target(
    position: &mut Position,
    on_bridge: bool,
    tube: &TubeFact,
    terrain: &ResolvedTerrainGrid,
) -> Result<DriveCoord, String> {
    let (dx, dy) = crate::util::direction::cell_delta_unchecked(tube.path_steps[0] as u8);
    let [x, y] = ground_pose::position_world_xy(position);
    Ok(DriveCoord {
        x: x.wrapping_add(dx.wrapping_mul(256)),
        y: y.wrapping_add(dy.wrapping_mul(256)),
        z: tube_first_z(position, on_bridge, tube, terrain)?,
    })
}

/// Drive's and Ship's first tube target (Drive `0x004B1390..0x004B13F8`,
/// Ship `0x006A0A58..0x006A0AC0`): the centre of the Cell one step from the
/// tube's entry (`0x005657A0`, `0x00480A30`), at [`tube_first_z`].
fn track_tube_target(
    position: &mut Position,
    on_bridge: bool,
    tube: &TubeFact,
    terrain: &ResolvedTerrainGrid,
) -> Result<DriveCoord, String> {
    let (dx, dy) = crate::util::direction::cell_delta_unchecked(tube.path_steps[0] as u8);
    let cell_x = (tube.entry.0 as i16).wrapping_add(dx as i16);
    let cell_y = (tube.entry.1 as i16).wrapping_add(dy as i16);
    Ok(DriveCoord {
        x: i32::from(cell_x) * 256 + 128,
        y: i32::from(cell_y) * 256 + 128,
        z: tube_first_z(position, on_bridge, tube, terrain)?,
    })
}

impl Simulation {
    /// Walk's fresh-head tube check (`0x0075B302..0x0075B599`), ahead of the
    /// admission the Process resumes at `0x0075B59C`
    /// ([`Simulation::run_walk_admission_request`]). A path head word of 8
    /// ([`tube_word`]):
    /// - at a live tube, enters it ([`Self::walk_tube_entry`]);
    /// - at no tube (`0x0075B34A`, `0x0075B356`), clears the head word and
    ///   the Walk head (`0x0075B554..0x0075B57A`), then calls Walk's
    ///   Stop_Moving (ILocomotion `+0x48`, `0x0075B57D`) and the class
    ///   setter's null destination (vt+0x480(NULL, 1), `0x0075B58C`).
    ///
    /// Answers whether the word was a tube word; either arm ends the Process.
    pub(crate) fn walk_tube_head(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return Ok(false);
        };
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Walk tube owner")?;
        let Some(word) = tube_word(actor.navigation.path_replay(), &actor.position, terrain) else {
            return Ok(false);
        };
        match word {
            TubeWord::Enter(tube_id) => {
                self.walk_tube_entry(id, tube_id, rules, registry, effects)?;
            }
            TubeWord::NoTube => {
                if let Some(actor) = self.substrate.entities.get_mut(id) {
                    actor.navigation.path_replay_mut().clear_live_head();
                    if let Some(locomotor) = actor.locomotor.as_mut() {
                        locomotor.set_step_head(None);
                    }
                }
                self.walk_stop_moving(id, Some(rules), effects)?;
                self.assign_null_destination(id, Some(rules), registry, effects);
            }
        }
        Ok(true)
    }

    /// Walk's tube entry (`0x0075B35C..0x0075B54E`): Mark(UP)
    /// (`0x0075B363`) and the raw receiver's REMOVE at the Location (vt+0xF4,
    /// `0x0075B391`), the Walk head at the exit Cell's centre with Z 0
    /// (`0x0075B3CA..0x0075B3D2`), the path shift
    /// (`0x0075B3E0..0x0075B3EB`), then the tube, cursor 0
    /// (`0x0075B3FC..0x0075B405`) and its first target ([`walk_tube_target`]).
    /// The target reads only the Location and the floor, which the Mark and
    /// the raw receiver leave alone, so it is taken first.
    fn walk_tube_entry(
        &mut self,
        id: u64,
        tube_id: TubeId,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<(), String> {
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Walk tube entry requires map cells")?;
        let tube = terrain
            .tube(tube_id)
            .ok_or("Walk tube entry lost its tube")?;
        let head = tube_exit_centre(tube);
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Walk tube owner")?;
        let on_bridge = actor.on_bridge();
        let target = walk_tube_target(&mut actor.position, on_bridge, tube, terrain)?;
        let location = ground_pose::position_world_coord(&actor.position);
        self.foot_mark_remove(id, Some(rules), registry, effects);
        self.object_raw_receiver_at(id, location, false);
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Walk tube owner")?;
        if let Some(locomotor) = actor.locomotor.as_mut() {
            locomotor.set_step_head(Some(head));
        }
        actor.navigation.path_replay_mut().consume(1);
        actor.low_bridge_tube_state = Some(LowBridgeTubeMovementState {
            tube_id,
            cursor: 0,
            target,
        });
        Ok(())
    }

    /// Drive's and Ship's Process_Track tube arm (Drive
    /// `0x004B1297..0x004B148E`, Ship `0x006A095F..0x006A0B5D`), after the
    /// speed prefix: a path head word of 8 ([`tube_word`]) with no selected
    /// track (`+0x58` of -1, `0x004B12A8`). The owner's Mark(UP)
    /// (vt+0x124(0), `0x004B12BA`) runs first, then a set HeadTo (`+0x40`)
    /// clears with the track's valid byte (`+0x63`,
    /// `0x004B12C0..0x004B12FF`). Then:
    /// - at a live tube, HeadTo becomes the exit Cell's centre at Z 0
    ///   (`0x004B1352`), the path shifts (`0x004B135D..0x004B1373`), the
    ///   tube and cursor 0 are stored (`0x004B1380..0x004B1389`) with the
    ///   first target ([`track_tube_target`]), and the valid byte sets
    ///   (`0x004B1480`);
    /// - at no tube (`0x004B1498`), the head word clears and HeadTo stays
    ///   null, while the Mark(UP) stands.
    ///
    /// Either way the selector retires (`0x004B1484`, `0x004B14A3`) and
    /// Process_Track returns false (`0x004B148C`, `0x004B25FC`).
    ///
    /// Answers whether the arm ran. A Unit takes no raw REMOVE here: its
    /// Mark(UP) clears the vehicle bit while `+0x6B6` holds, and the arm
    /// leaves `+0x6B6` as it is for the exit's Mark(DOWN)
    /// ([`Self::unit_tube_exit`]).
    pub(crate) fn track_tube_head(
        &mut self,
        id: u64,
        family: TrackFamily,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return Ok(false);
        };
        let Some(actor) = self.substrate.entities.get_mut(id) else {
            return Ok(false);
        };
        if actor
            .locomotor
            .as_ref()
            .and_then(|loco| loco.track_progress(family))
            .is_none_or(|progress| progress.turn_index() != -1)
        {
            return Ok(false);
        }
        let Some(word) = tube_word(actor.navigation.path_replay(), &actor.position, terrain) else {
            return Ok(false);
        };
        // The target reads only the Location and the floor, which Mark(UP)
        // leaves alone, so it is taken first.
        let entry = match word {
            TubeWord::Enter(tube_id) => {
                let tube = terrain
                    .tube(tube_id)
                    .ok_or("Drive tube entry lost its tube")?;
                let on_bridge = actor.on_bridge();
                let target = track_tube_target(&mut actor.position, on_bridge, tube, terrain)?;
                Some((tube_id, tube_exit_centre(tube), target))
            }
            TubeWord::NoTube => None,
        };
        self.foot_mark_remove(id, rules, registry, effects);
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Drive tube owner")?;
        let locomotor = actor
            .locomotor
            .as_mut()
            .ok_or("Drive tube entry lost its locomotor")?;
        if locomotor.track_head(family).is_some() {
            locomotor.store_track_head(family, None);
            locomotor.store_track_valid(family, false);
        }
        let mut progress = locomotor
            .track_progress(family)
            .ok_or("Drive tube entry lost its track state")?;
        match entry {
            Some((tube_id, head, target)) => {
                locomotor.store_track_head(family, Some(head));
                actor.navigation.path_replay_mut().consume(1);
                actor.low_bridge_tube_state = Some(LowBridgeTubeMovementState {
                    tube_id,
                    cursor: 0,
                    target,
                });
                locomotor.store_track_valid(family, true);
            }
            None => actor.navigation.path_replay_mut().clear_live_head(),
        }
        progress.retire_selector();
        locomotor.store_track_progress(family, progress);
        Ok(true)
    }
}

/// What one tube step did ([`tick_active_tube_object`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TubeTurn {
    /// The object holds no tube.
    Inactive,
    /// The object moved along its tube.
    Moved,
    /// A Unit at its tube's end found the exit Cell taken
    /// ([`Simulation::tube_exit_blocked`]).
    Blocked,
    /// A Unit stands at its tube's end with the exit Cell free
    /// ([`Simulation::unit_tube_exit`]).
    UnitExit,
    /// An infantryman stands at its tube's end
    /// ([`Simulation::infantry_tube_exit`]).
    InfantryExit,
}

/// One step of an active Unit/Infantry TubeMovement. The exits that read the
/// whole Simulation are left to [`Simulation::tube_object_turn`].
fn tick_active_tube_object(
    entities: &mut EntityStore,
    entity_id: u64,
    terrain: &ResolvedTerrainGrid,
    occupancy: &OccupancyGrid,
    rules: Option<&RuleSet>,
    interner: &StringInterner,
) -> TubeTurn {
    let Some(entity) = entities.get(entity_id) else {
        return TubeTurn::Inactive;
    };
    let Some(mut state) = entity.low_bridge_tube_state else {
        return TubeTurn::Inactive;
    };
    let Some(tube) = terrain.tube(state.tube_id) else {
        return TubeTurn::Moved;
    };
    let category = entity.category();
    let speed = rules
        .and_then(|rules| interner.object_type(entity.type_ref(), rules))
        .map_or(0, |object| {
            crate::util::fixed_math::ra2_speed_to_leptons_per_frame(object.speed)
        });
    let budget = if category == EntityCategory::Unit {
        speed.wrapping_mul(3) / 2
    } else {
        speed
    };
    let z_step = live_z_step(terrain, tube).unwrap_or(0);

    if usize::from(state.cursor) >= tube.path_len() {
        return tube_end(entity, occupancy);
    }

    // 0x00735A2F..0x00735A47: GetCoords (vt+0x48) less the target (+0x568),
    // through Distance3D.
    let current = ground_pose::object_get_coords(entity, Some(terrain));
    let distance = distance_3d_leptons(
        [current.x, current.y, current.z],
        [state.target.x, state.target.y, state.target.z],
    );
    if distance > budget {
        let raw_step = tube.path_steps[usize::from(state.cursor)];
        let next = advance_amount(current, state.target, budget, raw_step, z_step);
        ground_pose::foot_set_location(entities, entity_id, next, rules, interner);
        return TubeTurn::Moved;
    }

    state.cursor = state.cursor.saturating_add(1);
    if let Some(entity) = entities.get_mut(entity_id) {
        entity.low_bridge_tube_state = Some(state);
    }
    ground_pose::foot_set_location(entities, entity_id, state.target, rules, interner);
    if usize::from(state.cursor) >= tube.path_len() {
        return entities
            .get(entity_id)
            .map_or(TubeTurn::Moved, |entity| tube_end(entity, occupancy));
    }

    let next_raw = tube.path_steps[usize::from(state.cursor)];
    let (dx, dy) = crate::util::direction::cell_delta_unchecked(next_raw as u8);
    let old_target = state.target;
    state.target.x = state.target.x.wrapping_add(dx.wrapping_mul(256));
    state.target.y = state.target.y.wrapping_add(dy.wrapping_mul(256));
    state.target.z = state.target.z.wrapping_add(z_step);
    let leftover = budget.wrapping_sub(distance);
    let next = advance_amount(old_target, state.target, leftover, next_raw, z_step);
    if let Some(entity) = entities.get_mut(entity_id) {
        entity.low_bridge_tube_state = Some(state);
    }
    ground_pose::foot_set_location(entities, entity_id, next, rules, interner);
    TubeTurn::Moved
}

/// The tube's end (Unit `0x00735E36`, Infantry `0x0051B798`). An
/// infantryman's exit is [`Simulation::infantry_tube_exit`]. A Unit's is
/// [`Simulation::unit_tube_exit`] unless any object on the ground list
/// (`+0xE4`) of the Cell holding the Location blocks it
/// (`0x00735E5F..0x00735E6E`, [`Simulation::tube_exit_blocked`]).
fn tube_end(entity: &GameEntity, occupancy: &OccupancyGrid) -> TubeTurn {
    if entity.category() == EntityCategory::Infantry {
        return TubeTurn::InfantryExit;
    }
    if occupancy
        .get(entity.position.rx(), entity.position.ry())
        .is_some_and(|cell| cell.iter_layer(MovementLayer::Ground).next().is_some())
    {
        TubeTurn::Blocked
    } else {
        TubeTurn::UnitExit
    }
}

/// What one active tube object turn did ([`Simulation::tube_object_turn`]).
#[derive(Debug, Default)]
pub(crate) struct TubeObjectTurn {
    pub(crate) movement: MovementTickStats,
    pub(crate) bridge_state_changed: bool,
    /// The exit ran `Per_Cell_Process(2)`.
    pub(crate) per_cell_ran: bool,
}

impl Simulation {
    /// The TubeMovement the class AI runs in place of the rest of its turn
    /// while the object holds a tube (`+0x684`): `UnitClass` `0x007359F0`
    /// (called at `0x007363B0`) and `InfantryClass` `0x0051B350` (called at
    /// `0x0051BAC2`). It takes a step along the tube, or at its end the exit,
    /// which runs Per_Cell_Process(2) (Unit `0x0073603F`, before its speed and
    /// facing; Infantry `0x0051BA9B`, last). No locomotor Process runs: one
    /// the exit calls (the Infantry self-Scatter's, `0x0051D478`) is the
    /// locomotor's alone and reads no tube.
    ///
    /// The owed destination setter ([`Simulation::complete_pending_order`])
    /// runs first, as at a Process entry.
    pub(crate) fn tube_object_turn(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<TubeObjectTurn, FrameAdvanceError> {
        let mut turn = TubeObjectTurn::default();
        self.complete_pending_order(id, rules, registry, effects);
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return Ok(turn);
        };
        let step = tick_active_tube_object(
            &mut self.substrate.entities,
            id,
            terrain,
            &self.substrate.occupancy,
            rules,
            &self.interner,
        );
        let exit = match step {
            TubeTurn::Inactive => return Ok(turn),
            TubeTurn::Moved => Ok(false),
            TubeTurn::Blocked => self.tube_exit_blocked(id, rules, registry, effects),
            TubeTurn::UnitExit => self.unit_tube_exit(id, rules, registry, effects),
            TubeTurn::InfantryExit => self.infantry_tube_exit(id, rules, registry, effects),
        };
        turn.movement.movers_total = 1;
        turn.bridge_state_changed = exit.map_err(|cause| {
            FrameAdvanceError::new(self.session.tick(), self.session.binary_frame(), id, cause)
        })?;
        // Only a completed exit leaves the tube, and each runs its
        // Per_Cell_Process(2).
        turn.per_cell_ran = self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.low_bridge_tube_state.is_none());
        Ok(turn)
    }

    /// A tube exit its Cell refuses. Each Unit or Infantry on the ground
    /// list (`+0xE4`) of the Cell holding the Location whose locomotor is not
    /// moving ([`tube_exit_scatters`]) takes `Scatter(null, 1, 1)`: the list
    /// is copied first and each copy tested as the loop reaches it (Unit
    /// `0x00735E74..0x00735F62`, Infantry `0x0051B7E6..0x0051B8F4`). Then the
    /// object's speed becomes 0 (Unit `0x00735F6A`, Infantry `0x0051B8FC`)
    /// and it stays in the tube, to try again on its next turn. Without rules
    /// no Scatter admits, so none runs.
    ///
    /// Answers whether an Infantry Scatter's immediate Process changed bridge
    /// state.
    fn tube_exit_blocked(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        let occupants = self
            .substrate
            .entities
            .get(id)
            .and_then(|actor| {
                self.substrate
                    .occupancy
                    .get(actor.position.rx(), actor.position.ry())
            })
            .map_or_else(Vec::new, |cell| cell.snapshot_layer(MovementLayer::Ground));
        let mut bridge_state_changed = false;
        if let Some(rules) = rules {
            for blocker in occupants {
                if self
                    .substrate
                    .entities
                    .get(blocker)
                    .is_some_and(tube_exit_scatters)
                {
                    bridge_state_changed |= self.scatter(
                        blocker,
                        None,
                        ScatterFlags::new(true, true),
                        rules,
                        registry,
                        effects,
                    )?;
                }
            }
        }
        stop_blocked_mover(&mut self.substrate.entities, id);
        Ok(bridge_state_changed)
    }

    /// `UnitClass` TubeMovement's exit (`0x00735FA1..0x007360AA`), once the
    /// last step put the unit at its target and the Cell holding its
    /// Location has an empty ground list ([`tube_end`]). SetLocation
    /// (vt+0x1B4, `0x00735FEC`) puts it at the exit Cell's centre at the
    /// target's Z (`+0x570`), it leaves the tube (`+0x684 = -1`,
    /// `0x00735FF8`), and Mark(DOWN) lists it (`0x00735FFF`). A NavCom
    /// naming that Cell scatters the unit itself (`Scatter(null, 1, 1)`,
    /// `0x00736005..0x00736028`: EBX still holds the Cell from
    /// `0x00735E64`). Then it latches `+0x68B` (`0x00736038`), runs
    /// Per_Cell_Process(2) (`0x0073603F`), takes speed 1 (`0x0073604F`) and
    /// faces along the exit Cell's tube (`0x00736055..0x007360A5`). Without
    /// rules no Scatter admits, so none runs.
    ///
    /// Mark(DOWN) writes the raw vehicle bit while `+0x6B6` holds: the Drive
    /// entry left it as it was ([`Self::track_tube_head`]).
    ///
    /// Answers whether Per_Cell_Process changed bridge state.
    fn unit_tube_exit(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Unit tube exit requires map cells")?;
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired tube mover")?;
        let state = actor
            .low_bridge_tube_state
            .ok_or("Unit tube exit without a tube")?;
        let tube = terrain
            .tube(state.tube_id)
            .ok_or("Unit tube exit lost its tube")?;
        let at = (actor.position.rx(), actor.position.ry());
        let exit = DriveCoord::cell(tube.exit.0, tube.exit.1, state.target.z);
        let level = {
            let cells = NativeCellQuery::canonical(terrain);
            cells.ground_fields(cells.lookup_world(exit.x, exit.y)).0
        };
        self.foot_set_location_marked(id, exit, rules, registry, effects);
        if let Some(actor) = self.substrate.entities.get_mut(id) {
            // VERA's level cache follows the Location.
            let on_bridge = actor.on_bridge();
            actor.position.store_cell_level(level, on_bridge);
            actor.low_bridge_tube_state = None;
        }
        self.foot_mark_put(id, rules, registry, effects);
        let navcom_names_cell = self.substrate.entities.get(id).is_some_and(|actor| {
            matches!(actor.navigation.nav_com(), Some(NavTargetRef::Cell { rx, ry }) if (rx, ry) == at)
        });
        if let (true, Some(rules)) = (navcom_names_cell, rules) {
            self.scatter(
                id,
                None,
                ScatterFlags::new(true, true),
                rules,
                registry,
                effects,
            )?;
        }
        if let Some(actor) = self.substrate.entities.get_mut(id) {
            actor.runtime_bridge_transition.latch();
        }
        let bridge_state_changed = self
            .per_cell_process(id, PerCellReason::Arrival, rules, registry, effects)
            .map_err(|error| error.cause)?;
        if let Some(actor) = self.substrate.entities.get_mut(id) {
            actor.foot_speed.set_speed_fraction(SIM_ONE);
        }
        if let Some(terrain) = self.resolved_terrain.as_ref() {
            update_unit_final_facing(
                &mut self.substrate.entities,
                id,
                terrain,
                self.session.binary_frame(),
            );
        }
        Ok(bridge_state_changed)
    }

    /// `InfantryClass` TubeMovement's exit (`0x0051B798..0x0051BAA8`), once
    /// the last step put the infantryman at its target. It asks its own
    /// `Can_Enter_Cell` (`+0x1AC`) at the Cell holding its Location
    /// (`0x0051B7BE`), with direction -1, height 0, no previous cell and
    /// mode 1 (`0x0051B7C3..0x0051B7D8`).
    ///
    /// - Refused, it scatters the stationary Units and Infantry there, sets
    ///   its own speed to 0 and stays in the tube
    ///   ([`Self::tube_exit_blocked`]).
    /// - Admitted, PlaceInfantryInCell (`0x00481180`, `0x0051B967`) picks a
    ///   spot on the Cell's ground plane, and SetLocation (`0x0051B99C`)
    ///   puts it there at the floor (`0x00578080`, `0x0051B98A`). Then come
    ///   the locomotor's Force_Immediate_Destination(null) (`0x0051B9F4`)
    ///   and Stop_Movement_Animation (`0x0051BA16`), Mark(DOWN)
    ///   (`0x0051BA22`) and the raw receiver (vt+0xF0, `0x0051BA45`). A
    ///   NavCom naming the Cell scatters the infantryman itself
    ///   (`0x0051BA4B..0x0051BA6F`); otherwise its speed becomes 1
    ///   (`0x0051BA81`). Last, it leaves the tube (`+0x684 = -1`,
    ///   `0x0051BA8D`), latches `+0x68B` (`0x0051BA94`) and runs
    ///   Per_Cell_Process(2) (`0x0051BA9B`).
    ///
    /// The exit does not test PlaceInfantryInCell's answer. Without a spot
    /// it answers the zeroed coordinate at `0x0089E778` (`0x004812EC`,
    /// `0x004813CC`; zeroed by `0x0047B300`), so the infantryman goes to the
    /// floor at (0, 0). That needs an occupant the `+0x1AC` admits but the
    /// placement refuses, such as a vehicle the infantryman is entering.
    ///
    /// The Walk head the entry stored at the exit Cell's centre
    /// ([`Self::walk_tube_entry`]) is what Force_Immediate_Destination(null)
    /// releases: FindSubCellDest clears that spot's raw bit (`0x0075C2A0`).
    ///
    /// Answers whether its Scatter's immediate Process or Per_Cell_Process
    /// changed bridge state.
    fn infantry_tube_exit(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        let rules = rules.ok_or("Infantry tube exit requires rules")?;
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Infantry tube exit requires map cells")?;
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired tube mover")?;
        let location = ground_pose::position_world_coord(&actor.position);
        let at = (actor.position.rx(), actor.position.ry());
        let cell = NativeCellQuery::canonical(terrain).lookup_world(location.x, location.y);
        let args = InfantryEntryArgs {
            direction: -1,
            height: 0,
            previous_cell: None,
        };
        if self.foot_can_enter(id, cell, args, rules, registry)? != 0 {
            return self.tube_exit_blocked(id, Some(rules), registry, effects);
        }

        let out = self
            .place_infantry_in_ground_cell(rules, location)
            .map_or(DriveCoord { x: 0, y: 0, z: 0 }, |spot| {
                walk_head::selected_head(location, spot, 0, false)
            });
        let (exit, level) = {
            let cells = NativeCellQuery::canonical(
                self.resolved_terrain
                    .as_ref()
                    .ok_or("Infantry tube exit requires map cells")?,
            );
            let exit = DriveCoord {
                z: ground_pose::query_ground_height(&cells, out)?,
                ..out
            };
            (
                exit,
                cells.ground_fields(cells.lookup_world(exit.x, exit.y)).0,
            )
        };
        ground_pose::foot_set_location(
            &mut self.substrate.entities,
            id,
            exit,
            Some(rules),
            &self.interner,
        );
        if let Some(actor) = self.substrate.entities.get_mut(id) {
            // VERA's spot and level caches follow the Location: the spot is
            // the one the raw receiver reads from it.
            actor.sub_cell.hold(cell_kernel::infantry_preferred_spot(
                cell_kernel::CellQueryPoint {
                    x: exit.x,
                    y: exit.y,
                },
            ));
            let on_bridge = actor.on_bridge();
            actor.position.store_cell_level(level, on_bridge);
        }
        self.foot_force_immediate_destination(id, None, Some(rules), registry, effects)?;
        if let Some(locomotor) = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|actor| actor.locomotor.as_mut())
        {
            locomotor.stop_movement_animation();
        }
        self.foot_mark_put(id, Some(rules), registry, effects);
        if let Some(actor) = self.substrate.entities.get(id) {
            let location = ground_pose::position_world_coord(&actor.position);
            self.object_raw_receiver_at(id, location, true);
        }

        let navcom_names_cell = self.substrate.entities.get(id).is_some_and(|actor| {
            matches!(actor.navigation.nav_com(), Some(NavTargetRef::Cell { rx, ry }) if (rx, ry) == at)
        });
        let mut bridge_state_changed = false;
        if navcom_names_cell {
            bridge_state_changed = self.scatter(
                id,
                None,
                ScatterFlags::new(true, true),
                rules,
                registry,
                effects,
            )?;
        } else if let Some(actor) = self.substrate.entities.get_mut(id) {
            actor.foot_speed.set_speed_fraction(SIM_ONE);
        }
        if let Some(actor) = self.substrate.entities.get_mut(id) {
            actor.low_bridge_tube_state = None;
            actor.runtime_bridge_transition.latch();
        }
        bridge_state_changed |= self
            .per_cell_process(id, PerCellReason::Arrival, Some(rules), registry, effects)
            .map_err(|error| error.cause)?;
        Ok(bridge_state_changed)
    }
}

/// Whether a blocked tube exit scatters `blocker`: a Unit or Infantry
/// (`WhatAmI` 1 or 0xF) whose locomotor is not moving (ILocomotion +0x10).
/// Unit `0x00735EF9..0x00735F31`, Infantry `0x0051B88E..0x0051B8C6`. A Foot
/// without a locomotor raises a COM error natively (`0x00735F19`); here it is
/// skipped.
fn tube_exit_scatters(blocker: &GameEntity) -> bool {
    matches!(
        blocker.category(),
        EntityCategory::Unit | EntityCategory::Infantry
    ) && blocker.locomotor.is_some()
        && super::motion_query::is_moving(blocker) != Some(true)
}

/// The blocked exit's speed 0 (Unit `0x00735F6A`, Infantry `0x0051B8FC`):
/// vt+0x544 is `FootClass::SetSpeedFraction` (`0x004D3710`) for both, which
/// writes the Foot's applied fraction alone.
fn stop_blocked_mover(entities: &mut EntityStore, entity_id: u64) {
    if let Some(entity) = entities.get_mut(entity_id) {
        entity.foot_speed.set_speed_fraction(SIM_ZERO);
    }
}

/// `UnitClass::TubeMovement` (`0x007359F0`)'s exit facing (`0x00736055..0x007360A5`):
/// GetTubeAtCell (`0x00484F20`) at the exit Cell, then its direction (`+0x2C`).
fn update_unit_final_facing(
    entities: &mut EntityStore,
    entity_id: u64,
    terrain: &ResolvedTerrainGrid,
    native_frame: u32,
) {
    let Some(entity) = entities.get_mut(entity_id) else {
        return;
    };
    let Some(tube) = terrain
        .cell(entity.position.rx(), entity.position.ry())
        .and_then(|cell| cell.tube_index)
        .and_then(|tube_id| terrain.tube(tube_id))
    else {
        return;
    };
    let q32 = tube.direction.wrapping_shl(13).wrapping_sub(0x6001) & 0xffff_e000_u32 as i32;
    entity.body_facing.snap(q32 as u16, native_frame);
}

fn live_z_step(terrain: &ResolvedTerrainGrid, tube: &TubeFact) -> Option<i32> {
    let count = i32::try_from(tube.path_len()).ok()?;
    if count == 0 {
        return None;
    }
    let entry = floor_at_cell_center(terrain, tube.entry)?;
    let exit = floor_at_cell_center(terrain, tube.exit)?;
    Some(exit.wrapping_sub(entry) / count)
}

/// The floor height (`0x00578080`) at a cell's centre, as the live Z step
/// reads it at the tube's entry and exit (`0x00735B05`, `0x00735B16`).
fn floor_at_cell_center(terrain: &ResolvedTerrainGrid, cell: (u16, u16)) -> Option<i32> {
    let center = DriveCoord::cell(cell.0, cell.1, 0);
    ground_pose::ground_surface_z_at([center.x, center.y], false, Some(terrain), None)
}

fn advance_amount(
    base: DriveCoord,
    target: DriveCoord,
    amount: i32,
    raw_step: i32,
    z_step: i32,
) -> DriveCoord {
    let facing = crate::util::direction_tables::facing16_from_delta(
        target.x.wrapping_sub(base.x),
        target.y.wrapping_sub(base.y),
    );
    // `0x00735C53..0x00735CC0` (and `0x00735D56..0x00735DC1`) are the Walk
    // step's instructions (`0x0075C067..0x0075C0CB`): the stored radians of
    // the facing word, `SinFromTable`/`CosFromTable`, times the amount, added
    // to the coordinate and cut by ftol.
    let [x, y] = crate::util::native_trig::facing_step_world_xy([base.x, base.y], facing, amount);
    let amount_x87 = X87Chop53::load_i32(amount);

    let nominal_bits = if raw_step & 1 == 0 {
        256.0_f64.to_bits()
    } else {
        0x4076_a09e_667f_3bcd
    };
    let nominal = X87Chop53::load_f64(NativeF64Bits::from_bits(nominal_bits))
        .expect("nominal tube segment length is finite");
    let ratio = X87Chop53::div(amount_x87, nominal).expect("nominal length is nonzero");
    let z_delta = X87Chop53::mul(ratio, X87Chop53::load_i32(z_step));
    let z = X87Chop53::ftol_i64(X87Chop53::add(X87Chop53::load_i32(base.z), z_delta))
        .expect("tube Z fits i32") as i32;
    DriveCoord { x, y, z }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::map::tube_facts::TubeSource;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::sim::occupancy::VEHICLE_OCCUPATION_BIT;

    fn flat_cell(rx: u16, ry: u16, tube_index: Option<TubeId>) -> ResolvedTerrainCell {
        ResolvedTerrainCell {
            tube_index,
            ..crate::map::resolved_terrain::test_loader_clear_cell(rx, ry)
        }
    }

    fn explicit_terrain(path: Vec<i32>) -> ResolvedTerrainGrid {
        let exit = path.len() as u16;
        tube_terrain(path, exit)
    }

    /// Four clear cells in a row. The tube enters at (0, 0), follows `path`
    /// and records its exit at (`exit`, 0).
    fn tube_terrain(path: Vec<i32>, exit: u16) -> ResolvedTerrainGrid {
        let mut cells = Vec::new();
        for x in 0..=3 {
            cells.push(flat_cell(x, 0, (x == 0).then_some(TubeId(0))));
        }
        ResolvedTerrainGrid::from_cells_with_tubes(
            4,
            1,
            cells,
            vec![TubeFact {
                entry: (0, 0),
                exit: (exit, 0),
                direction: 2,
                path_steps: path,
                source: TubeSource::ExplicitMap,
            }],
        )
    }

    fn unit(id: u64) -> GameEntity {
        unit_of(id, "MTNK")
    }

    fn unit_of(id: u64, kind: &str) -> GameEntity {
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            id,
            0,
            0,
            0,
            0,
            crate::sim::intern::test_intern("Americans"),
            Health::new(100),
            crate::sim::intern::test_intern(kind),
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        entity.lifecycle.set_in_limbo_for_test(false);
        entity.lifecycle.set_cell_marked_for_test(true);
        entity.locomotor = Some(LocomotorState::for_test_kind(
            crate::rules::locomotor_type::LocomotorKind::Drive,
        ));
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .ensure_installed_track_state();
        entity
    }

    /// One walking infantryman (id 1, `Americans` `E1`) at the end of
    /// [`explicit_terrain`]'s two-step tube: at in-cell (64, 192) of the
    /// exit (2, 0), on Clear ground with a Foot speed row, its movement
    /// animation running and its Walk head where the entry put it.
    fn infantry_at_tube_end() -> (Simulation, RuleSet) {
        use crate::rules::ini_parser::IniFile;
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=APC\n[AircraftTypes]\n[BuildingTypes]\n\
             [E1]\nStrength=100\nSpeed=4\n[APC]\nStrength=100\nSpeed=4\n",
        ))
        .expect("rules");
        let mut terrain = explicit_terrain(vec![2, 2]);
        for x in 0..=3 {
            terrain.cell_mut_for_test(x, 0).unwrap().speed_costs.foot = Some(100);
        }
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let kind = sim.interner.intern("E1");
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            1,
            2,
            0,
            0,
            0,
            owner,
            Health::new(100),
            kind,
            EntityCategory::Infantry,
            0,
            5,
            true,
        );
        entity.lifecycle.set_in_limbo_for_test(false);
        entity.lifecycle.set_cell_marked_for_test(false);
        let spot = DriveCoord {
            x: 2 * 256 + 64,
            y: 192,
            z: 0,
        };
        entity.position.put_location(spot);
        let mut walk = crate::sim::movement::locomotor::LocomotorState::for_test_kind(
            crate::rules::locomotor_type::LocomotorKind::Walk,
        );
        walk.set_step_head(Some(spot));
        walk.begin_walk_motion();
        // The Walk entry's head: the exit Cell's centre at Z 0.
        walk.set_step_head(Some(DriveCoord::cell(2, 0, 0)));
        entity.locomotor = Some(walk);
        entity.low_bridge_tube_state = Some(LowBridgeTubeMovementState {
            tube_id: TubeId(0),
            cursor: 2,
            target: spot,
        });
        sim.substrate.entities.insert(entity);
        sim.resolved_terrain = Some(terrain);
        (sim, rules)
    }

    /// Infantry leaves a tube at the spot PlaceInfantryInCell picks
    /// (`0x0051B967`), at the floor there (`0x00578080` at `0x0051B98A`),
    /// through SetLocation (`0x0051B99C`). Its movement animation stops
    /// (`0x0051BA16`), Mark(DOWN) lists it (`0x0051BA22`), the raw receiver
    /// marks its spot (`0x0051BA45`), its speed becomes 1 (`0x0051BA81`),
    /// it latches `+0x68B` (`0x0051BA94`) and runs Per_Cell_Process(2)
    /// (`0x0051BA9B`), whose Techno tail promotes it into the playfield
    /// (`0x006F511A`).
    /// The floor is native's: `tools/ramp_height_vectors.json`
    /// `ramp_1_sub_64_192` (level 0, ramp 1, sub-cell (64, 192)) is 26.
    #[test]
    fn infantry_leaves_a_tube_at_the_floor_of_its_spot() {
        let (mut sim, rules) = infantry_at_tube_end();
        sim.playfield_bounds = Some(crate::sim::arena_fixture::OPEN_PLAYFIELD);
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .set_in_playfield(false);
        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut_for_test(2, 0)
            .unwrap()
            .slope_type = 1;
        let turn = sim
            .tube_object_turn(1, Some(&rules), None, FrameEffects::default())
            .expect("tube turn");
        assert!(turn.per_cell_ran);
        assert_eq!(turn.movement.movers_total, 1);
        let owner = sim.substrate.entities.get(1).unwrap();
        assert!(owner.low_bridge_tube_state.is_none());
        assert_eq!(
            owner.locomotor.as_ref().unwrap().walk_animation_moving(),
            Some(false)
        );
        assert_eq!(owner.sub_cell(), Some(3));
        assert_eq!(
            ground_pose::position_world_coord(&owner.position),
            DriveCoord {
                x: 2 * 256 + 64,
                y: 192,
                z: 26
            }
        );
        assert!(owner.lifecycle.cell_marked());
        assert!(sim.substrate.occupancy.contains_entity(2, 0, 1));
        assert_eq!(
            sim.substrate.raw_cell_occupation.bits_at(
                crate::sim::occupancy::RawCellKey::Real(2, 0),
                MovementLayer::Ground
            ),
            1 << 3
        );
        assert_eq!(owner.foot_speed.applied_fraction(), SIM_ONE);
        assert!(owner.runtime_bridge_transition.pending_mismatch());
        assert!(owner.in_playfield());
    }

    /// The infantryman of [`infantry_at_tube_end`], moved to spot 3 of cell
    /// `(rx, 0)` before any tube, listed with its raw spot marked, walking
    /// east to cell (3, 0) with the path `[8, 2]`: a tube word, then a step.
    fn infantry_walking_into(rx: u16) -> (Simulation, RuleSet) {
        let (mut sim, rules) = infantry_at_tube_end();
        crate::sim::arena_fixture::supply_native_map(&mut sim);
        let spot = DriveCoord {
            x: i32::from(rx) * 256 + 64,
            y: 192,
            z: 0,
        };
        let infantry = sim.substrate.entities.get_mut(1).unwrap();
        infantry.low_bridge_tube_state = None;
        infantry.locomotor.as_mut().unwrap().set_step_head(None);
        infantry.position.put_location(spot);
        infantry.set_sub_cell(Some(3));
        infantry.navigation.set_path_replay_for_test(
            crate::sim::components::FootPathQueue::for_test(vec![8, 2], 0, Some((0, 0))),
        );
        infantry
            .navigation
            .set_nav_com_for_test(Some(NavTargetRef::Cell { rx: 3, ry: 0 }));
        infantry
            .locomotor
            .as_mut()
            .unwrap()
            .set_walk_destination(Some(DriveCoord::cell(3, 0, 0)));
        sim.add_entity_occupancy(1);
        sim.object_raw_receiver_at(1, spot, true);
        (sim, rules)
    }

    /// The Walk head the entry stores at the exit Cell's centre
    /// (`0x0075B3CA..0x0075B3D2`) is the one the exit's
    /// Force_Immediate_Destination(null) releases (`0x0051B9F4`): its
    /// FindSubCellDest clears that spot's raw bit (`0x0075C2A0`), here a
    /// friend's. The infantryman walks in, along the tube and out at the
    /// spot PlaceInfantryInCell picks.
    #[test]
    fn the_exit_releases_the_head_the_entry_stored() {
        let (mut sim, rules) = infantry_walking_into(0);
        let owner = sim.interner.intern("Americans");
        let kind = sim.interner.intern("E1");
        let mut friend = GameEntity::new_at_frame_zero_for_test(
            2,
            2,
            0,
            0,
            0,
            owner,
            Health::new(100),
            kind,
            EntityCategory::Infantry,
            0,
            5,
            true,
        );
        let centre = DriveCoord::cell(2, 0, 0);
        friend.position.put_location(centre);
        friend.set_sub_cell(Some(0));
        friend.locomotor = Some(LocomotorState::for_test_kind(
            crate::rules::locomotor_type::LocomotorKind::Walk,
        ));
        sim.substrate.entities.insert(friend);
        sim.add_entity_occupancy(2);
        sim.object_raw_receiver_at(2, centre, true);
        let exit_bits = |sim: &Simulation| {
            sim.substrate.raw_cell_occupation.bits_at(
                crate::sim::occupancy::RawCellKey::Real(2, 0),
                MovementLayer::Ground,
            )
        };
        assert_eq!(exit_bits(&sim), 1);

        sim.process_ground_locomotor_for_test(1, Some(&rules), None, None)
            .expect("Walk Process");
        let mut turns = 0;
        while sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .low_bridge_tube_state
            .is_some()
        {
            sim.tube_object_turn(1, Some(&rules), None, FrameEffects::default())
                .expect("tube turn");
            turns += 1;
            assert!(turns < 1000, "the tube's two steps end");
        }
        let infantry = sim.substrate.entities.get(1).unwrap();
        assert_eq!(infantry.sub_cell(), Some(3));
        assert_eq!(infantry.locomotor.as_ref().unwrap().step_head(), None);
        assert_eq!(exit_bits(&sim), 1 << 3, "the centre's bit is released");
    }

    /// A head of 8 at a Cell without a tube takes the out-of-range arm
    /// (`0x0075B551..0x0075B599`): the head word clears, Walk's Stop_Moving
    /// drops its destination and the class setter's null destination its
    /// NavCom. The infantryman stays where it is, listed.
    #[test]
    fn a_tube_word_without_a_tube_stops_the_infantryman() {
        let (mut sim, rules) = infantry_walking_into(1);
        sim.process_ground_locomotor_for_test(1, Some(&rules), None, None)
            .expect("Walk Process");
        let infantry = sim.substrate.entities.get(1).unwrap();
        assert!(infantry.low_bridge_tube_state.is_none());
        assert!(
            infantry
                .navigation
                .path_replay()
                .remaining_directions()
                .is_empty()
        );
        let walk = infantry.locomotor.as_ref().unwrap();
        assert_eq!(walk.step_head(), None);
        assert_eq!(walk.walk_destination(), None);
        assert_eq!(infantry.navigation.nav_com(), None);
        assert!(sim.substrate.occupancy.contains_entity(1, 0, 1));
        assert_eq!(
            ground_pose::position_world_coord(&infantry.position),
            DriveCoord {
                x: 256 + 64,
                y: 192,
                z: 0
            }
        );
    }

    /// The raw vehicle bit of cell `(rx, 0)`'s ground plane.
    fn vehicle_bits(sim: &Simulation, rx: u16) -> u8 {
        sim.substrate.raw_cell_occupation.bits_at(
            crate::sim::occupancy::RawCellKey::Real(rx, 0),
            MovementLayer::Ground,
        ) & VEHICLE_OCCUPATION_BIT
    }

    /// [`unit_driving_on`] the two-step tube of [`explicit_terrain`].
    fn unit_driving_into(rx: u16) -> (Simulation, RuleSet) {
        unit_driving_on(rx, explicit_terrain(vec![2, 2]))
    }

    /// One Drive unit (id 1, `Americans` `APC`, mission Move) at the centre
    /// of cell `(rx, 0)` of `terrain`, listed with its raw vehicle bit, no
    /// track selected and its last head still set, driving east to cell
    /// (3, 0) with the path `[8, 2]`.
    fn unit_driving_on(rx: u16, terrain: ResolvedTerrainGrid) -> (Simulation, RuleSet) {
        use crate::rules::ini_parser::IniFile;
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n0=APC\n[AircraftTypes]\n[BuildingTypes]\n\
             [APC]\nStrength=100\nSpeed=4\n",
        ))
        .expect("rules");
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let kind = sim.interner.intern("APC");
        let mut vehicle = unit(1);
        vehicle.set_owner_for_test(owner);
        vehicle.set_type_ref_for_test(kind);
        vehicle.mission.set_current_for_test(
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Move),
            0,
        );
        vehicle.lifecycle.set_cell_marked_for_test(false);
        vehicle.position.put_location(DriveCoord::cell(rx, 0, 0));
        vehicle.navigation.set_path_replay_for_test(
            crate::sim::components::FootPathQueue::for_test(vec![8, 2], 0, Some((rx as i16, 0))),
        );
        vehicle
            .navigation
            .set_nav_com_for_test(Some(NavTargetRef::Cell { rx: 3, ry: 0 }));
        let locomotor = vehicle.locomotor.as_mut().unwrap();
        locomotor.store_track_head(TrackFamily::Drive, Some(DriveCoord::cell(rx, 0, 0)));
        locomotor.store_track_valid(TrackFamily::Drive, true);
        sim.substrate.entities.insert(vehicle);
        sim.resolved_terrain = Some(terrain);
        crate::sim::arena_fixture::supply_native_map(&mut sim);
        sim.add_entity_occupancy(1);
        assert!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .lifecycle
                .cell_marked()
        );
        assert_ne!(vehicle_bits(&sim, rx), 0);
        (sim, rules)
    }

    /// Process_Track's tube arm (`0x004B1297..0x004B148E`): Mark(UP)
    /// unlists the unit and clears its raw vehicle bit (`0x004B12BA`),
    /// HeadTo becomes the exit Cell's centre at Z 0 with the valid byte set
    /// (`0x004B1352`, `0x004B1480`), the path shifts and keeps the reference
    /// cell (`0x004B1362`), the selector stays retired (`0x004B1484`), and
    /// the first target is the centre of the entry Cell's neighbour along
    /// the path (`0x004B1390..0x004B147A`): the unit stands off its Cell's
    /// centre, where Walk's target, its Location a cell on, would differ.
    /// Nothing writes `+0x6B6`, the NavCom or the destination.
    #[test]
    fn a_tube_word_takes_a_drive_unit_into_the_tube() {
        let (mut sim, rules) = unit_driving_into(0);
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .position
            .put_location(DriveCoord {
                x: 60,
                y: 200,
                z: 0,
            });
        assert_eq!(
            sim.track_tube_head(
                1,
                TrackFamily::Drive,
                Some(&rules),
                None,
                FrameEffects::default()
            ),
            Ok(true)
        );
        let vehicle = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            vehicle.low_bridge_tube_state,
            Some(LowBridgeTubeMovementState {
                tube_id: TubeId(0),
                cursor: 0,
                target: DriveCoord::cell(1, 0, 0),
            })
        );
        let locomotor = vehicle.locomotor.as_ref().unwrap();
        assert_eq!(
            locomotor.track_head(TrackFamily::Drive),
            Some(DriveCoord::cell(2, 0, 0))
        );
        assert_eq!(locomotor.track_valid(TrackFamily::Drive), Some(true));
        assert_eq!(
            locomotor
                .track_progress(TrackFamily::Drive)
                .unwrap()
                .turn_index(),
            -1
        );
        assert!(!vehicle.lifecycle.cell_marked());
        assert!(!sim.substrate.occupancy.contains_entity(0, 0, 1));
        assert_eq!(vehicle_bits(&sim, 0), 0);
        assert!(vehicle.foot_occupation_enabled.get());
        assert_eq!(vehicle.navigation.path_replay().remaining_directions(), [2]);
        assert_eq!(
            vehicle.navigation.path_replay().reference_cell(),
            Some((0, 0))
        );
        assert_eq!(
            vehicle.navigation.nav_com(),
            Some(NavTargetRef::Cell { rx: 3, ry: 0 })
        );
    }

    /// The unit of [`unit_driving_into`] at the end of the tube: at the exit
    /// Cell (2, 0)'s centre, unlisted, its cursor past the path, with HeadTo
    /// at that centre as the entry stored it.
    fn unit_at_tube_end() -> (Simulation, RuleSet) {
        let (mut sim, rules) = unit_driving_into(2);
        sim.remove_entity_occupancy(1);
        let vehicle = sim.substrate.entities.get_mut(1).unwrap();
        vehicle.navigation.set_path_replay_for_test(
            crate::sim::components::FootPathQueue::for_test(vec![8, 2], 1, Some((0, 0))),
        );
        vehicle.low_bridge_tube_state = Some(LowBridgeTubeMovementState {
            tube_id: TubeId(0),
            cursor: 2,
            target: DriveCoord::cell(2, 0, 0),
        });
        assert!(!vehicle.lifecycle.cell_marked());
        assert_eq!(vehicle_bits(&sim, 2), 0);
        (sim, rules)
    }

    /// The Unit exit (`0x00735FA1..0x007360AA`): SetLocation at the exit
    /// Cell's centre (`0x00735FEC`), Mark(DOWN) lists it with its raw
    /// vehicle bit (`0x00735FFF`; `+0x6B6` held), the `+0x68B` latch
    /// (`0x00736038`), Per_Cell_Process(2) (`0x0073603F`), whose Techno
    /// tail promotes it into the playfield (`0x006F511A`), and speed 1
    /// (`0x0073604F`). Its NavCom names another cell, so it keeps it.
    #[test]
    fn a_unit_leaves_a_tube_through_mark() {
        let (mut sim, rules) = unit_at_tube_end();
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .set_in_playfield(false);
        let turn = sim
            .tube_object_turn(1, Some(&rules), None, FrameEffects::default())
            .expect("tube turn");
        assert!(turn.per_cell_ran);
        let vehicle = sim.substrate.entities.get(1).unwrap();
        assert!(vehicle.low_bridge_tube_state.is_none());
        assert_eq!(
            ground_pose::position_world_coord(&vehicle.position),
            DriveCoord::cell(2, 0, 0)
        );
        assert!(vehicle.lifecycle.cell_marked());
        assert!(sim.substrate.occupancy.contains_entity(2, 0, 1));
        assert_ne!(vehicle_bits(&sim, 2), 0);
        assert!(vehicle.runtime_bridge_transition.pending_mismatch());
        assert!(vehicle.in_playfield());
        assert_eq!(vehicle.foot_speed.applied_fraction(), SIM_ONE);
        assert_eq!(
            vehicle.navigation.nav_com(),
            Some(NavTargetRef::Cell { rx: 3, ry: 0 })
        );
    }
}
