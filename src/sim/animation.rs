//! Sprite animation system — tracks per-entity animation state and frame timing.
//!
//! Manages animation sequences (stand, walk, attack, die) for SHP sprite entities.
//! Each entity with an `Animation` component has a current sequence, frame index,
//! and reached-frame counter. The `tick_animations()` function advances frames and
//! handles auto-transitions (e.g., stand ↔ walk based on MovementTarget).
//!
//! ## SHP frame layout
//! Infantry SHP files pack multiple sequences contiguously:
//! - Stand: 1 frame × 8 facings = frames 0–7
//! - Walk: 6 frames × 8 facings = frames 8–55
//! - Idle1: 15 frames (non-directional) = frames 56–70
//! - Die1: 15 frames (non-directional) = frames 86–100
//!
//! For directional sequences (facings > 1):
//!   `shp_frame = start + facing_index * frame_count + frame_within_sequence`
//! For non-directional sequences:
//!   `shp_frame = start + frame_within_sequence`
//!
//! ## Auto-transitions
//! - Entity gains MovementTarget → switch to Walk
//! - Entity loses MovementTarget → switch back to Stand
//! - Attack sequence finishes → switch to Stand (TransitionTo)
//! - Die sequence finishes → hold last frame (HoldLast)
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/components (MovementTarget, TypeRef) and
//!   re-exports the rules-owned sequence vocabulary below.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use std::collections::BTreeMap;

// Immutable sequence vocabulary and catalog construction are rules-owned
// (`rules::animation_sequence`, F04); sim re-exports the types so runtime
// consumers and serialized components keep their existing paths.
pub use crate::rules::animation_sequence::{
    FacingSlots, LoopMode, SequenceDef, SequenceKind, SequenceSet, ShpVehicleCadence,
    default_building_sequences, default_infantry_sequences,
};

/// Facing-to-frame-slot table for infantry bodies, transcribed from the
/// original engine's 32-dword table.
///
/// Indexed by [`infantry_facing_step32`]. The two trailing 7s are what make the
/// north arc wrap early: slot boundaries land 4/256 of a turn *before* the
/// octant centers rather than on them, so the arc for slot 7 is facing bytes
/// 236..=255 plus 0..=11, not the symmetric 240..=15. That bias is native
/// behavior, not an artifact of the transcription.
const INFANTRY_FACING_SLOT_TABLE: [u8; 32] = [
    7, 7, 6, 6, 6, 6, 5, 5, 5, 5, 4, 4, 4, 4, 3, 3, 3, 3, 2, 2, 2, 2, 1, 1, 1, 1, 0, 0, 0, 0, 7, 7,
];

/// Number of frame blocks an SHP vehicle body must declare for its facing
/// slots to be used at all. Any other count draws slot 0 for every facing.
const VEHICLE_FACING_SLOTS: u8 = 8;

/// Quantize a facing byte to the 32-step index into [`INFANTRY_FACING_SLOT_TABLE`].
///
/// The original computes this from the 16-bit facing as
/// `((facing16 >> 10) + 1) >> 1 & 0x1F`. Only bits 10..=15 of the 16-bit facing
/// survive that shift, and those are exactly bits 2..=7 of the facing byte, so
/// the byte carries every bit the native expression can see — matching it needs
/// no 16-bit plumbing. The `+ 1` before the final shift is a round-half-up, which
/// is why this is not the same as truncating the facing into eight buckets.
const fn infantry_facing_step32(facing: u8) -> usize {
    (((facing as u16 >> 2) + 1) >> 1) as usize & 0x1F
}

/// Facing byte to SHP vehicle frame-block slot.
///
/// The original computes `(((facing16 >> 12) + 1) >> 1) + 1 & 7`. As above, the
/// shift keeps only bits 12..=15 of the 16-bit facing, which are bits 4..=7 of
/// the byte. The inner `+ 1` rounds to the nearest octant; the outer `+ 1` is
/// what puts NW — not N — at frame 0 of every block.
const fn vehicle_facing_slot(facing: u8) -> u16 {
    ((((facing as u16 >> 4) + 1) >> 1) + 1) & 7
}

/// Facing byte to infantry frame-block slot (0..=7), counter-clockwise from
/// screen-north. Exposed for render-side fallbacks that have to pick a standing
/// pose without a `SequenceDef` in hand.
pub fn infantry_facing_slot(facing: u8) -> u16 {
    INFANTRY_FACING_SLOT_TABLE[infantry_facing_step32(facing)] as u16
}

/// Per-entity animation state component.
///
/// Attach to any entity that should animate. The `tick_animations()` system
/// reads and updates this each frame. The render loop reads `sequence` and
/// `frame_index` to select the correct SHP frame via `resolve_shp_frame()`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Animation {
    /// Currently playing sequence.
    pub sequence: SequenceKind,
    /// Current frame within the sequence (0 to frame_count - 1).
    pub frame_index: u16,
    /// Reached native frames accumulated since the last image advance.
    pub elapsed_frames: u16,
    /// True if a HoldLast sequence has reached its final frame.
    pub finished: bool,
}

impl Animation {
    /// Create a new Animation starting at frame 0 of the given sequence.
    pub fn new(sequence: SequenceKind) -> Self {
        Self {
            sequence,
            frame_index: 0,
            elapsed_frames: 0,
            finished: false,
        }
    }

    /// Switch to a different sequence, resetting frame and timing.
    /// No-op if already playing the requested sequence.
    pub fn switch_to(&mut self, sequence: SequenceKind) {
        if self.sequence != sequence {
            self.sequence = sequence;
            self.frame_index = 0;
            self.elapsed_frames = 0;
            self.finished = false;
        }
    }
}

/// Whether a sequence represents the infantry being in a prone stance.
///
/// This is a temporary stance proxy until the sim carries an explicit prone bit.
#[cfg(test)]
pub fn sequence_is_prone(sequence: SequenceKind) -> bool {
    matches!(
        sequence,
        SequenceKind::Prone
            | SequenceKind::Crawl
            | SequenceKind::FireProne
            | SequenceKind::SecondaryProne
            | SequenceKind::Down
    )
}

/// Whether a sequence is a one-shot infantry fire action.
pub fn sequence_is_fire_action(sequence: SequenceKind) -> bool {
    matches!(
        sequence,
        SequenceKind::Attack
            | SequenceKind::FireProne
            | SequenceKind::DeployedFire
            | SequenceKind::SecondaryFire
            | SequenceKind::SecondaryProne
            | SequenceKind::FireFly
            | SequenceKind::WetAttack
    )
}

fn sequence_set_for_type<'a>(
    sequences: &'a BTreeMap<String, SequenceSet>,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    type_name: &str,
) -> Option<&'a SequenceSet> {
    sequences.get(type_name).or_else(|| {
        let canonical = rules?.object(type_name)?.id.as_str();
        sequences.get(canonical)
    })
}

/// Compute the SHP frame index for a given sequence, facing, and animation frame.
///
/// For directional sequences (facings > 1):
///   `start_frame + facing_slot * facing_multiplier + frame_index`
/// For non-directional (facings == 1):
///   `start_frame + frame_index`
///
/// `facing` is the RA2 DirStruct byte (0–255, clockwise in cell space:
/// 0=N, 64=E, 128=S, 192=W). The facing-to-slot conversion is
/// family-specific — see [`FacingSlots`].
///
/// The `facings <= 1` early return stands in for the original's directional
/// test, which reads the facing multiplier rather than a facing count. The two
/// agree because every producer that emits a zero multiplier also emits
/// `facings == 1`, and a zero multiplier contributes nothing to the sum anyway.
pub fn resolve_shp_frame(def: &SequenceDef, facing: u8, frame_index: u16) -> u16 {
    let clamped: u16 = if def.frame_count > 0 {
        frame_index % def.frame_count
    } else {
        0
    };

    if def.facings <= 1 {
        return def.start_frame + clamped;
    }

    let facing_slot: u16 = match def.facing_slots {
        FacingSlots::InfantryTable => infantry_facing_slot(facing),
        // The vehicle draw path gates the whole slot computation on the body
        // declaring exactly 8 frame blocks; any other count draws block 0 for
        // every facing.
        FacingSlots::VehicleOctant if def.facings == VEHICLE_FACING_SLOTS => {
            vehicle_facing_slot(facing)
        }
        FacingSlots::VehicleOctant => 0,
    };

    def.start_frame + facing_slot * def.facing_multiplier + clamped
}

/// Advance the persistent Unit SHP body counter at FootClass's post-Process
/// point for the current absolute binary frame.
///
/// The moving branch deliberately does not guard a zero `WalkRate`: retail
/// feeds that raw signed value to IDIV, making zero invalid content. `IdleRate`
/// is different â€” zero is its documented branch-off switch and performs no
/// division. The counter itself is a native dword, so increment wraps.
pub(crate) fn tick_shp_vehicle_body_frame_counter(
    entity: &mut crate::sim::game_entity::GameEntity,
    cadence: ShpVehicleCadence,
    binary_frame: u32,
) {
    if entity.category != crate::map::entities::EntityCategory::Unit
        || entity.is_voxel
        || entity.dying
        || !entity.lifecycle.object_alive
        || entity.lifecycle.in_limbo
        || entity.object_is_falling_down != 0
    {
        return;
    }

    let Some(locomotor) = entity.locomotor.as_ref() else {
        return;
    };
    if locomotor.piggyback.is_some()
        || entity.deploy_state.is_some()
        || entity.is_warped_out()
        || entity.is_warping_in()
    {
        return;
    }

    let rate = if crate::sim::movement::ready_producer::is_moving_now_for(entity, binary_frame) {
        cadence.walk_rate
    } else {
        if cadence.idle_rate == 0 {
            return;
        }
        cadence.idle_rate
    };

    if (binary_frame as i32) % rate == 0 {
        entity.body_frame_counter = entity.body_frame_counter.wrapping_add(1);
    }
}

/// Resolve the ordinary UnitClass SHP body frame from the persistent Foot
/// counter. Firing and death counters intentionally stay on their existing
/// paths; this helper covers only the standing/walking draw branch.
pub fn resolve_shp_vehicle_body_frame(
    set: &SequenceSet,
    facing: u8,
    body_frame_counter: u32,
    locomotor_is_moving: bool,
) -> Option<u16> {
    let cadence = set.shp_vehicle_cadence()?;
    let sequence = if locomotor_is_moving || cadence.idle_rate != 0 {
        SequenceKind::Walk
    } else {
        SequenceKind::Stand
    };
    let def = set.get(&sequence)?;
    let frame_index = if sequence == SequenceKind::Walk && def.frame_count != 0 {
        (body_frame_counter % u32::from(def.frame_count)) as u16
    } else {
        0
    };
    Some(resolve_shp_frame(def, facing, frame_index))
}

/// Advance a single animation by one reached native gameplay frame.
///
/// Returns `Some(SequenceKind)` if the sequence completed with a `TransitionTo`
/// loop mode, indicating the caller should switch to that sequence. Otherwise None.
pub fn advance_animation(
    anim: &mut Animation,
    def: &SequenceDef,
    game_options: &crate::sim::game_options::GameOptions,
) -> Option<SequenceKind> {
    let frame_delay = if def.normalized {
        game_options.normalized_anim_delay(def.frame_delay)
    } else {
        def.frame_delay
    };
    if anim.finished || frame_delay == 0 || def.frame_count == 0 {
        return None;
    }

    anim.elapsed_frames = anim.elapsed_frames.saturating_add(1);

    while anim.elapsed_frames >= frame_delay {
        anim.elapsed_frames -= frame_delay;
        anim.frame_index += 1;

        if anim.frame_index >= def.frame_count {
            match def.loop_mode {
                LoopMode::Loop => {
                    anim.frame_index = 0;
                }
                LoopMode::HoldLast => {
                    anim.frame_index = def.frame_count.saturating_sub(1);
                    anim.finished = true;
                    anim.elapsed_frames = 0;
                    return None;
                }
                LoopMode::TransitionTo(next) => {
                    anim.frame_index = 0;
                    anim.elapsed_frames = 0;
                    return Some(next);
                }
            }
        }
    }

    None
}

/// Advance all animated entities by one reached native gameplay frame.
///
/// 1. Dying entities: skip auto-transitions, only advance death animation.
///    Returns IDs of dying entities whose death animation has finished.
/// 2. Auto-transitions: entities with MovementTarget switch to Walk;
///    entities without MovementTarget switch back to Stand.
/// 3. Attack transitions: stationary entities with attack_target switch to
///    Attack (or FireProne/DeployedFire depending on stance).
/// 4. Advances frame timing for each entity's current sequence.
///
/// `sequences` maps type_id → SequenceSet for frame timing lookup.
/// Entities whose type_id isn't in the map are skipped (no animation advance).
/// `completed_actions` collects each Infantry whose Doing sequence ended.
#[allow(clippy::too_many_arguments)]
fn tick_animations_impl(
    entities: &mut crate::sim::entity_store::EntityStore,
    sequences: &BTreeMap<String, SequenceSet>,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    game_options: &crate::sim::game_options::GameOptions,
    interner: &crate::sim::intern::StringInterner,
    binary_frame: u32,
    tick_dying: bool,
    completed_actions: &mut Vec<(u64, i32)>,
) -> Vec<u64> {
    let mut dying_finished: Vec<u64> = Vec::new();
    let keys: Vec<u64> = entities.keys_sorted();

    for &id in &keys {
        let Some(entity) = entities.get_mut(id) else {
            continue;
        };
        if entity.dying && !tick_dying {
            continue;
        }
        // The stage steps in TechnoClass::AI_Update (`0x006FAC4D`), which a
        // warped object never reaches (`GameEntity::ai_frozen`): its pose holds.
        if !entity.dying && entity.ai_frozen() {
            continue;
        }
        let type_ref = entity.type_ref();
        let Some(anim) = entity.animation.as_mut() else {
            // Dying entity with no animation → ready for despawn.
            if entity.dying {
                dying_finished.push(id);
            }
            continue;
        };

        // Dying entities: only advance the death animation, skip all transitions.
        if entity.dying {
            if anim.finished {
                dying_finished.push(id);
                continue;
            }
            let Some(seq_set) =
                sequence_set_for_type(sequences, rules, interner.resolve(type_ref))
            else {
                dying_finished.push(id);
                continue;
            };
            let Some(def) = seq_set.get(&anim.sequence) else {
                dying_finished.push(id);
                continue;
            };
            advance_animation(anim, def, game_options);
            if anim.finished {
                dying_finished.push(id);
            }
            continue;
        }

        let has_movement: bool = entity.movement_target.is_some();
        let shoveling = entity
            .mission_leaf
            .as_infantry()
            .is_some_and(|leaf| leaf.doing() == 38);
        let pending_fire_sequence = entity
            .attack_target
            .as_ref()
            .and_then(|attack| attack.pending_infantry_fire.map(|pending| pending.sequence));
        let has_fire_action = pending_fire_sequence.is_some();
        let preserving_fire_action = sequence_is_fire_action(anim.sequence) && !has_fire_action;

        // Look up this type's sequence definitions for transition checks.
        let seq_set = sequence_set_for_type(sequences, rules, interner.resolve(type_ref));

        // Deploy state takes priority over the standard Stand/Walk/Attack cascade.
        // The visual reflects the sim phase; DeployedFire is the auto-transition
        // when a Deployed unit gains an attack target (visual-only, matches stock YR).
        match entity.deploy_state {
            Some(crate::sim::deploy::DeployPhase::Deploying { .. }) => {
                anim.switch_to(SequenceKind::Deploy);
            }
            Some(crate::sim::deploy::DeployPhase::Undeploying { .. }) => {
                anim.switch_to(SequenceKind::Undeploy);
            }
            Some(crate::sim::deploy::DeployPhase::Deployed) => {
                if let Some(sequence) = pending_fire_sequence {
                    anim.switch_to(sequence);
                } else if anim.sequence != SequenceKind::DeployedFire {
                    anim.switch_to(SequenceKind::Deployed);
                }
            }
            None => {
                let runtime_prone = entity
                    .infantry
                    .as_ref()
                    .is_some_and(|infantry| infantry.is_prone);
                if !matches!(anim.sequence, SequenceKind::Down | SequenceKind::Up) && runtime_prone
                {
                    if let Some(set) = seq_set {
                        if let Some(sequence) = pending_fire_sequence {
                            anim.switch_to(sequence);
                        } else if has_movement {
                            if set.get(&SequenceKind::Crawl).is_some() {
                                anim.switch_to(SequenceKind::Crawl);
                            }
                        } else if !preserving_fire_action && set.get(&SequenceKind::Prone).is_some()
                        {
                            anim.switch_to(SequenceKind::Prone);
                        }
                    }
                }
                // A slave digging (Doing 0x26, `InfantryClass::Mission_Harvest @
                // 0x00522EF3`) shows Shovel until it moves off or its
                // Do_Action(0) ends the dig; the cascade below then walks it.
                if !runtime_prone {
                    if shoveling && !has_movement && anim.sequence == SequenceKind::Stand {
                        anim.switch_to(SequenceKind::Shovel);
                    } else if anim.sequence == SequenceKind::Shovel && (has_movement || !shoveling)
                    {
                        anim.switch_to(SequenceKind::Stand);
                    }
                }
                // Standard cascade for upright entities — preserved verbatim from prior logic.
                if !matches!(anim.sequence, SequenceKind::Down | SequenceKind::Up)
                    && !runtime_prone
                    && has_movement
                    && anim.sequence == SequenceKind::Stand
                {
                    anim.switch_to(SequenceKind::Walk);
                } else if !matches!(anim.sequence, SequenceKind::Down | SequenceKind::Up)
                    && !runtime_prone
                    && !has_movement
                    && matches!(anim.sequence, SequenceKind::Walk | SequenceKind::Crawl)
                {
                    anim.switch_to(SequenceKind::Stand);
                }
                if !matches!(anim.sequence, SequenceKind::Down | SequenceKind::Up)
                    && !runtime_prone
                    && has_fire_action
                    && !has_movement
                {
                    if let Some(sequence) = pending_fire_sequence {
                        anim.switch_to(sequence);
                    }
                }
            }
        }

        // Advance frame timing.
        let Some(set) = seq_set else {
            continue;
        };
        let Some(def) = set.get(&anim.sequence) else {
            continue;
        };

        // UnitClass SHP Stand/Walk images are selected from Foot's persistent
        // body counter at draw time. Do not advance a second relative clock or
        // reset cadence when the generic visual sequence changes.
        if entity.category == crate::map::entities::EntityCategory::Unit
            && !entity.is_voxel
            && set.shp_vehicle_cadence().is_some()
            && matches!(anim.sequence, SequenceKind::Stand | SequenceKind::Walk)
        {
            continue;
        }

        if let Some(next) = advance_animation(anim, def, game_options) {
            // gamemd-derived: `InfantryClass::DoType_Sequencer` @ 0x00520AE0
            // (0x00520CEB..0x00520D16) updates the completed action's facing
            // before dispatching its next/default action.
            if let Some(facing) = def.completion_facing {
                entity.facing = facing;
                if let Some(body_facing) = entity.body_facing.as_mut() {
                    body_facing.snap(u16::from(facing) << 8, binary_frame);
                }
            }
            // The Doing that installed this sequence has played to its end.
            if let Some(doing) = entity.mission_leaf.as_infantry().map(|leaf| leaf.doing())
                && doing == i32::from(crate::rules::infantry_sequence::action_id(anim.sequence))
            {
                completed_actions.push((id, doing));
            }
            anim.switch_to(next);
        }
    }

    dying_finished
}

#[cfg(test)]
pub fn tick_animations(
    entities: &mut crate::sim::entity_store::EntityStore,
    sequences: &BTreeMap<String, SequenceSet>,
    game_options: &crate::sim::game_options::GameOptions,
    interner: &crate::sim::intern::StringInterner,
    binary_frame: u32,
) -> Vec<u64> {
    tick_animations_impl(
        entities,
        sequences,
        None,
        game_options,
        interner,
        binary_frame,
        true,
        &mut Vec::new(),
    )
}

/// Advance only living entity animations. Dying animation completion is owned
/// by that object's live scheduler turn so UnInit can compact the LogicVector
/// before the scheduler cursor advances.
///
/// Returns each Infantry, in visit order, whose Doing played its sequence to
/// the end this frame, with that Doing: the stage end
/// `InfantryClass::DoType_Sequencer` dispatches on
/// (`Simulation::infantry_action_completed`).
pub(crate) fn tick_non_dying_animations(
    entities: &mut crate::sim::entity_store::EntityStore,
    sequences: &BTreeMap<String, SequenceSet>,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    game_options: &crate::sim::game_options::GameOptions,
    interner: &crate::sim::intern::StringInterner,
    binary_frame: u32,
) -> Vec<(u64, i32)> {
    let mut completed_actions = Vec::new();
    let _ = tick_animations_impl(
        entities,
        sequences,
        rules,
        game_options,
        interner,
        binary_frame,
        false,
        &mut completed_actions,
    );
    completed_actions
}

/// Advance one dying object's death sequence during its own scheduler turn.
/// Missing animation/type/sequence data takes the immediate-finish path used
/// by the prior app-owned completion fallback.
pub(crate) fn tick_dying_animation(
    entity: &mut crate::sim::game_entity::GameEntity,
    sequence_set: Option<&SequenceSet>,
    game_options: &crate::sim::game_options::GameOptions,
    _binary_frame: u32,
) -> bool {
    debug_assert!(entity.dying);
    let Some(anim) = entity.animation.as_mut() else {
        return true;
    };
    if anim.finished {
        return true;
    }
    let Some(seq_set) = sequence_set else {
        return true;
    };
    let Some(def) = seq_set.get(&anim.sequence) else {
        return true;
    };
    advance_animation(anim, def, game_options);
    anim.finished
}

/// Number of animation frames in oregath.shp per facing direction.
const HARVEST_OVERLAY_FRAMES: u16 = 15;

/// Advance all HarvestOverlay components by one reached native frame.
///
/// Cycles through the 15-frame oregath.shp animation for harvesters that are
/// actively gathering ore. When not visible, the overlay is skipped.
pub fn tick_harvest_overlays(entities: &mut crate::sim::entity_store::EntityStore) {
    let keys: Vec<u64> = entities.keys_sorted();
    for &id in &keys {
        let Some(entity) = entities.get_mut(id) else {
            continue;
        };
        let Some(overlay) = entity.harvest_overlay.as_mut() else {
            continue;
        };
        if !overlay.visible {
            continue;
        }
        overlay.elapsed_frames = 0;
        overlay.frame = (overlay.frame + 1) % HARVEST_OVERLAY_FRAMES;
    }
}

/// Advance all VoxelAnimation components by one reached native frame.
///
/// Cycles through HVA frames for voxel entities that have `playing == true`.
/// Frame wraps around to 0 when reaching frame_count (looping animation).
pub fn tick_voxel_animations(entities: &mut crate::sim::entity_store::EntityStore) {
    let keys: Vec<u64> = entities.keys_sorted();
    for &id in &keys {
        let Some(entity) = entities.get_mut(id) else {
            continue;
        };
        let Some(anim) = entity.voxel_animation.as_mut() else {
            continue;
        };
        if !anim.playing || anim.frame_count <= 1 || anim.frame_delay == 0 {
            continue;
        }
        anim.elapsed_frames = anim.elapsed_frames.saturating_add(1);
        while anim.elapsed_frames >= anim.frame_delay {
            anim.elapsed_frames -= anim.frame_delay;
            anim.frame = (anim.frame + 1) % anim.frame_count;
        }
    }
}

/// Map a warhead's `InfDeath=` to the death sequence, if it selects one at all.
///
/// gamemd-derived: the `InfantryClass` death handler's jump table at
/// `0x00518D58`, dispatched on `InfDeath - 1` over the range 0..9. The mapping
/// is EXCLUSIVE: `1` plays `Die1` and `2` plays `Die2` with no animation, while
/// `3..=10` spawn an animation and no sequence, and `0` or `> 10` do nothing at
/// all. `Die3`, `Die4` and `Die5` are unreachable — no arm reaches DoType
/// `0x0D`/`0x0E`/`0x0F`. Selection draws no RNG: `Do_Action` is called with
/// `randomStart = 0`, which is the parameter that gates the frame-start draw.
///
/// RESIDUAL (GSI-08.13) — four arms ahead of the table are not modelled, each
/// needing a field VERA does not carry: a paradropping infantryman forces
/// `InfDeath = 3` (`CurrentDoType == 33`); a kill by a building whose type sets
/// `+0x16BF` forces `5`, the Tesla-Coil skeleton death; a type with a non-empty
/// `DeathAnims` list (`+0xE7C`) spawns from that list and returns, taking NO
/// sequence; and `NotHuman=` (`+0xEAD`) forces `Die1`. Trigger: paradrops, any
/// Tesla kill, and the `NotHuman=` types. Player effect: a Tesla-Coiled
/// infantryman plays the ordinary death instead of the skeleton, and a
/// `NotHuman=` type plays a sequence it does not own. Frequency: Tesla Coils are
/// routine in a Soviet match; paradrops are per-support-power.
pub fn death_sequence_for_inf_death(inf_death: u8) -> Option<SequenceKind> {
    match inf_death {
        1 => Some(SequenceKind::Die1),
        2 => Some(SequenceKind::Die2),
        _ => None,
    }
}

/// Whether this `InfDeath=` spawns a death ANIMATION rather than a sequence.
///
/// gamemd-derived: same jump table, arms 3..=10. Exclusive with
/// [`death_sequence_for_inf_death`] by construction.
pub fn inf_death_spawns_anim(inf_death: u8) -> bool {
    (3..=10).contains(&inf_death)
}

#[cfg(test)]
#[path = "animation_tests.rs"]
mod tests;
