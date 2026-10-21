//! Sprite animation system — tracks per-entity animation state and frame timing.
//!
//! Manages animation sequences (stand, walk, attack, die) for SHP sprite entities.
//! Each entity with an `Animation` component has a current sequence, frame index,
//! and reached-frame counter. The `tick_animations()` function advances frames and
//! follows each completed sequence's transition.
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
//! - Attack sequence finishes → switch to Stand (TransitionTo)
//! - Die sequence finishes → hold last frame (HoldLast)
//!
//! Motion switches no sequence: Infantry's Doing/Stage owns its own, and the
//! Unit SHP draw picks Walk or Stand by the locomotor's Is_Moving.
//!
//! ## Dependency rules
//! - Part of sim/ — reads entity, movement and rules state, and re-exports the
//!   rules-owned sequence vocabulary below.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use std::collections::BTreeMap;

// Immutable sequence vocabulary and catalog construction are rules-owned
// (`rules::animation_sequence`, F04); sim re-exports the types so runtime
// consumers and serialized components keep their existing paths.
pub use crate::rules::animation_sequence::{
    FacingSlots, FootBodyCadence, LoopMode, SequenceDef, SequenceKind, SequenceSet,
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
fn infantry_facing_step32(facing: u8) -> usize {
    usize::from(crate::util::direction_tables::step32_from_facing16(
        u16::from(facing) << 8,
    ))
}

/// Facing byte to SHP vehicle frame-block slot.
///
/// The original computes `(((facing16 >> 12) + 1) >> 1) + 1 & 7`. As above, the
/// shift keeps only bits 12..=15 of the 16-bit facing, which are bits 4..=7 of
/// the byte. The inner `+ 1` rounds to the nearest octant; the outer `+ 1` is
/// what puts NW — not N — at frame 0 of every block.
fn vehicle_facing_slot(facing: u8) -> u16 {
    u16::from((crate::util::direction::direction_from_facing(facing) + 1) & 7)
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
/// Only this module writes it. Its native counterpart has not been traced
/// (#1593).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Animation {
    /// Currently playing sequence.
    sequence: SequenceKind,
    /// Current frame within the sequence (0 to frame_count - 1).
    /// Infantry draws from its private native Stage and Doing instead.
    frame_index: u16,
    /// Reached native frames accumulated since the last image advance.
    elapsed_frames: u16,
    /// True if a HoldLast sequence has reached its final frame.
    finished: bool,
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

    /// The playing sequence.
    pub fn sequence(&self) -> SequenceKind {
        self.sequence
    }

    /// The frame within the sequence.
    pub fn frame_index(&self) -> u16 {
        self.frame_index
    }

    /// Reached native frames since the last image advance.
    pub fn elapsed_frames(&self) -> u16 {
        self.elapsed_frames
    }

    /// A HoldLast sequence reached its last frame.
    pub fn finished(&self) -> bool {
        self.finished
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

    /// Install a fixture's sequence.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_sequence_for_test(&mut self, sequence: SequenceKind) {
        self.sequence = sequence;
    }

    /// Install a fixture's frame.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_frame_index_for_test(&mut self, frame_index: u16) {
        self.frame_index = frame_index;
    }

    /// Install a fixture's elapsed frames.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_elapsed_frames_for_test(&mut self, elapsed_frames: u16) {
        self.elapsed_frames = elapsed_frames;
    }

    /// Install a fixture's HoldLast end.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_finished_for_test(&mut self, finished: bool) {
        self.finished = finished;
    }
}

fn sequence_set_for_type<'a>(
    sequences: &'a BTreeMap<String, SequenceSet>,
    rules: &crate::rules::ruleset::RuleSet,
    type_name: &str,
) -> Option<&'a SequenceSet> {
    sequences.get(type_name).or_else(|| {
        let canonical = rules.object(type_name)?.id.as_str();
        sequences.get(canonical)
    })
}

/// Borrowed frame data from the rules owner. Infantry keeps its signed native
/// record and Doing identity; generic Unit/Building definitions retain their
/// existing presentation vocabulary. This view owns no mutable state.
pub enum ShpFrameDefinition<'a> {
    Infantry(&'a crate::rules::infantry_sequence::InfantrySequenceEntry),
    Generic(&'a SequenceDef),
}

impl<'a> From<&'a crate::rules::infantry_sequence::InfantrySequenceEntry>
    for ShpFrameDefinition<'a>
{
    fn from(value: &'a crate::rules::infantry_sequence::InfantrySequenceEntry) -> Self {
        Self::Infantry(value)
    }
}

impl<'a> From<&'a SequenceDef> for ShpFrameDefinition<'a> {
    fn from(value: &'a SequenceDef) -> Self {
        Self::Generic(value)
    }
}

/// Select a signed SHP frame. Native Infantry518D80, arithmetic518E08..1F
/// and518F3B..88: clamp the signed count to1, divide full Stage, then add
/// the positive facing stride and start with DWORD wrapping. Native keeps
/// EAX signed through DrawIt5195DA; asset consumers check the frame boundary.
///
/// The caller owns selected type, Doing and body/Jumpjet target facing. Native
/// evidence: tools/spatial_oracle/anytown_damage/foot_missions.json,
/// draw_stage_modulo_receipt and infantry_frame_selection_receipt.
pub fn resolve_shp_frame<'a>(
    def: impl Into<ShpFrameDefinition<'a>>,
    facing: u8,
    frame_index: i32,
) -> i32 {
    match def.into() {
        ShpFrameDefinition::Infantry(record) => resolve_shp_frame_parts(
            record.start_frame,
            record.frames_per_facing,
            record.facings,
            i32::from(infantry_facing_slot(facing)),
            frame_index,
        ),
        ShpFrameDefinition::Generic(def) => {
            resolve_generic_shp_frame(def, facing, frame_index, false)
        }
    }
}

fn resolve_generic_shp_frame(
    def: &SequenceDef,
    facing: u8,
    frame_index: i32,
    suppress_facing: bool,
) -> i32 {
    let slot = if suppress_facing {
        0
    } else {
        match def.facing_slots {
            FacingSlots::InfantryTable => infantry_facing_slot(facing),
            FacingSlots::VehicleOctant if def.facings == VEHICLE_FACING_SLOTS => {
                vehicle_facing_slot(facing)
            }
            FacingSlots::VehicleOctant => 0,
        }
    };
    resolve_shp_frame_parts(
        i32::from(def.start_frame),
        i32::from(def.frame_count),
        if def.facings <= 1 {
            0
        } else {
            i32::from(def.facing_multiplier)
        },
        i32::from(slot),
        frame_index,
    )
}

fn resolve_shp_frame_parts(
    start: i32,
    count: i32,
    stride: i32,
    slot: i32,
    frame_index: i32,
) -> i32 {
    let within = frame_index % count.max(1);
    let direction = if stride > 0 {
        slot.wrapping_mul(stride)
    } else {
        0
    };
    start.wrapping_add(direction).wrapping_add(within)
}

/// `FootClass+0x538`, the persistent body-animation counter. Unit SHP and
/// Unit/Aircraft voxel drawing reduce it against the selected model's frame count.
/// MoveSound also consumes its delta; this is serialized and hashed state. It
/// advances on absolute binary-frame cadence in `FootClass::AI`
/// (`0x004DA886..0x004DAA01`, [`tick_foot_body_frame_counter`]) and never
/// resets on a visual transition or deployment. It is a native dword, so the
/// increment wraps.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct BodyFrameCounter {
    value: u32,
}

impl BodyFrameCounter {
    /// The counter.
    pub const fn get(self) -> u32 {
        self.value
    }

    /// `FootClass::AI`'s increment.
    fn advance(&mut self) {
        self.value = self.value.wrapping_add(1);
    }

    /// Install a fixture's counter.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_for_test(&mut self, value: u32) {
        self.value = value;
    }
}

/// UnitClass+6C0. Original7353E0 constructs -1; successful Unit741421
/// seeds the signed-byte ART FiringFrames countdown. Unit7363C9 advances it
/// before Foot AI. A limboed parasite has no Logic visit and retains the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct UnitFiringCounter {
    value: i32,
}

impl Default for UnitFiringCounter {
    fn default() -> Self {
        Self { value: -1 }
    }
}

impl UnitFiringCounter {
    /// Native signed countdown; -1 selects the non-firing body branch.
    pub const fn get(self) -> i32 {
        self.value
    }

    fn advance(&mut self) {
        self.value = self.value.wrapping_sub(1).max(-1);
    }

    #[cfg(any(test, feature = "test-support"))]
    /// Install an original comparison or persistence fixture's raw counter.
    pub fn set_for_test(&mut self, value: i32) {
        self.value = value;
    }
}

/// Admitted Unit7363C9..7363DE, before Hunt/Ready/Foot AI. Entry lifecycle,
/// Tube and Temporal gates belong to the existing Unit AI caller.
pub(crate) fn tick_unit_firing_counter(entity: &mut crate::sim::game_entity::GameEntity) {
    entity.unit_firing_counter.advance();
}

/// Successful Unit741410..741421, after the base FireAt/LimboLaunch tail.
/// Stock DRON retains native FiringSyncFrame0/1=-1. The sync-gated primary
/// arm remains unrepresented with the existing fire-error reader; this writer
/// covers the represented, unsynchronized Unit types, not that dormant arm.
pub(crate) fn commit_unit_firing_counter(
    entity: &mut crate::sim::game_entity::GameEntity,
    firing_frames: i8,
) {
    if firing_frames > 0 {
        entity.unit_firing_counter.value = i32::from(firing_frames) * 2 - 1;
    }
}

/// Advance the persistent Foot body counter at FootClass's post-Process
/// point for the current absolute binary frame.
///
/// The moving branch deliberately does not guard a zero `WalkRate`: retail
/// feeds that raw signed value to IDIV, making zero invalid content. `IdleRate`
/// is different — zero is its documented branch-off switch and performs no
/// division. The counter itself is a native dword, so increment wraps.
///
/// The same Foot entry is reached by Unit73647B, Infantry51BC9F and
/// Aircraft414DA3. Only the HoverAttack arm asks for a Unit; the ordinary
/// moving/idle/DeployToLand arms serve every Foot. Original concrete-class
/// controls: tools/spatial_oracle/unit_simple_deploy.json.
pub(crate) fn tick_foot_body_frame_counter(
    entity: &mut crate::sim::game_entity::GameEntity,
    rules: crate::sim::movement::SpeedRules<'_>,
    cadence: FootBodyCadence,
    hover_attack: bool,
    deploy_to_land: bool,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    binary_frame: u32,
) {
    if !entity.category().is_foot()
        || entity.death.is_dying()
        || !entity.lifecycle.object_alive()
        || entity.lifecycle.in_limbo()
        || entity.is_falling_down()
    {
        return;
    }

    if entity.locomotor.is_none()
        || entity.lift.swap_active()
        || entity.is_warped_out()
        || entity.is_warping_in()
    {
        return;
    }
    // Foot4DA886..4DAA01: the same+538 feeds SHP and voxel drawing.
    // HoverAttack is Type+390, not TurretSpins. Native deploy6E0 only
    // excludes that hover-attack walk arm; it does not suppress IdleRate.
    let moving = crate::sim::movement::motion_query::is_moving_now(entity, rules, binary_frame);
    let walking = moving
        || (entity.category() == crate::map::entities::EntityCategory::Unit
            && entity.attack_target.target().is_some()
            && hover_attack
            && !entity.is_fully_deployed());
    let due = (walking && (binary_frame as i32) % cadence.walk_rate == 0)
        || (cadence.idle_rate != 0 && !moving && (binary_frame as i32) % cadence.idle_rate == 0)
        // GetHeight (vt+0x1C8) at `0x004DA9CD`.
        || (deploy_to_land
            && crate::sim::movement::air_movement::current_fly_height(entity, terrain) > 0);
    if due {
        entity.body_frame_counter.advance();
    }
}

/// Unit73C64B..73C725: an active firing counter precedes Stand/Walk and
/// advances one image per two counter steps, in descending image order.
/// The persistent Foot counter continues to own Stand/Walk. Death drawing
/// remains outside this admitted live-body branch.
pub fn resolve_shp_vehicle_body_frame(
    set: &SequenceSet,
    facing: u8,
    body_frame_counter: u32,
    firing_counter: i32,
    locomotor_is_moving: bool,
    disguised: bool,
) -> Option<u16> {
    let cadence = set.shp_vehicle_cadence()?;
    if firing_counter >= 0 {
        let def = set.get(&SequenceKind::Attack)?;
        let frame_index = firing_counter / 2;
        // The native seed is within its type's fire block. Do not wrap an
        // incompatible/invalid retained counter onto an unrelated atlas frame.
        if frame_index >= i32::from(def.frame_count) {
            return None;
        }
        return u16::try_from(resolve_generic_shp_frame(
            def,
            facing,
            frame_index,
            disguised,
        ))
        .ok();
    }
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
    // Unit73C654..73C681 only computes the facing bucket when the actual
    // type has eight facings AND the object is not disguised. Slot zero is
    // not equivalent to raw facing zero in the vehicle direction table.
    u16::try_from(resolve_generic_shp_frame(
        def,
        facing,
        i32::from(frame_index),
        disguised,
    ))
    .ok()
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
/// 1. Dying entities: only advance the death animation. Returns IDs of
///    dying entities whose death animation has finished.
/// 2. Advances frame timing for each entity's current sequence, following a
///    completed sequence's `TransitionTo`.
///
/// `sequences` maps type_id → SequenceSet for frame timing lookup.
/// Entities whose type_id isn't in the map are skipped (no animation advance).
fn tick_animations_impl(
    entities: &mut crate::sim::entity_store::EntityStore,
    sequences: &BTreeMap<String, SequenceSet>,
    rules: &crate::rules::ruleset::RuleSet,
    game_options: &crate::sim::game_options::GameOptions,
    interner: &crate::sim::intern::StringInterner,
    binary_frame: u32,
    tick_dying: bool,
) -> Vec<u64> {
    let mut dying_finished: Vec<u64> = Vec::new();
    let keys: Vec<u64> = entities.keys_sorted();

    for &id in &keys {
        let Some(entity) = entities.get(id) else {
            continue;
        };
        if entity.death.is_dying() && !tick_dying {
            continue;
        }
        // The stage steps in TechnoClass::AI_Update (`0x006FAC4D`), which a
        // warped object never reaches (`GameEntity::ai_frozen`): its pose holds.
        if !entity.death.is_dying() && entity.ai_frozen() {
            continue;
        }
        // Its Doing's sequence, which its actions restart and whose stage
        // steps in its own turn (`sim::movement::infantry_action`); this
        // frame-end clock leaves it.
        if crate::sim::movement::infantry_action::doing_owns_sequence(entity) {
            continue;
        }
        if entity.animation.is_none() {
            // Dying entity with no animation → ready for despawn.
            if entity.death.is_dying() {
                dying_finished.push(id);
            }
            continue;
        }
        let entity = entities
            .get_mut(id)
            .expect("an animated entity was just read");
        let type_ref = entity.type_ref();
        let category = entity.category();
        let anim = entity
            .animation
            .as_mut()
            .expect("an animated entity was just read");

        // Dying entities: only advance the death animation, skip all transitions.
        if entity.death.is_dying() {
            if anim.finished {
                dying_finished.push(id);
                continue;
            }
            let Some(seq_set) = sequence_set_for_type(sequences, rules, interner.resolve(type_ref))
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

        // Infantry never enters this cascade: Doing/Stage is its only
        // sequence clock. Motion does not switch the sequence either: the Unit
        // SHP draw picks Walk or Stand by the locomotor's Is_Moving (vt+0x10 at
        // `0x0073C696`, `resolve_shp_vehicle_body_frame`).
        let seq_set = sequence_set_for_type(sequences, rules, interner.resolve(type_ref));

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
        if category == crate::map::entities::EntityCategory::Unit
            && set.shp_vehicle_cadence().is_some()
            && matches!(anim.sequence, SequenceKind::Stand | SequenceKind::Walk)
        {
            continue;
        }

        if let Some(next) = advance_animation(anim, def, game_options) {
            if let Some(facing) = def.completion_facing {
                snap_completion_facing(&mut entity.body_facing, facing, binary_frame);
            }
            anim.switch_to(next);
        }
    }

    dying_finished
}

#[cfg(any(test, feature = "test-support"))]
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
        crate::rules::ruleset::RuleSet::empty_for_test(),
        game_options,
        interner,
        binary_frame,
        true,
    )
}

/// `InfantryClass::DoType_Sequencer @ 0x00520AE0` (`0x00520CEB..0x00520D16`):
/// a completed action's facing hint turns the body before the next or default
/// action, through `FacingClass::UpdateFacing` (`FacingClass::snap`).
/// Takes the body facing, which the cascade borrows beside its animation.
pub(crate) fn snap_completion_facing(
    body_facing: &mut crate::sim::movement::FacingClass,
    facing: u8,
    binary_frame: u32,
) {
    body_facing.snap(u16::from(facing) << 8, binary_frame);
}

/// Advance only living entity animations. Dying animation completion is owned
/// by that object's live scheduler turn so UnInit can compact the LogicVector
/// before the scheduler cursor advances.
///
pub(crate) fn tick_non_dying_animations(
    entities: &mut crate::sim::entity_store::EntityStore,
    sequences: &BTreeMap<String, SequenceSet>,
    rules: &crate::rules::ruleset::RuleSet,
    game_options: &crate::sim::game_options::GameOptions,
    interner: &crate::sim::intern::StringInterner,
    binary_frame: u32,
) {
    let _ = tick_animations_impl(
        entities,
        sequences,
        rules,
        game_options,
        interner,
        binary_frame,
        false,
    );
}

/// Advance one dying object's death sequence during its own scheduler turn.
/// Missing animation/type/sequence data takes the immediate-finish path used
/// by the prior app-owned completion fallback.
/// VERA-only: steps a sequence the `TerminalLifetime` dying mark keeps in the
/// store, which native lacks (see that record); the assert checks the mark.
pub(crate) fn tick_dying_animation(
    entity: &mut crate::sim::game_entity::GameEntity,
    sequence_set: Option<&SequenceSet>,
    game_options: &crate::sim::game_options::GameOptions,
    _binary_frame: u32,
) -> bool {
    debug_assert!(entity.death.is_dying());
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
/// RESIDUAL (GSI-08.13) — three arms ahead of the table are not modelled, each
/// needing a field VERA does not carry: a kill by a building whose type sets
/// `+0x16BF` forces `5`; a type with a non-empty `DeathAnims` list (`+0xE7C`)
/// spawns from that list and returns, taking NO sequence; and `NotHuman=`
/// (`+0xEAD`) forces `Die1`. `+0x16BF` is `LaserFence=` (BuildingTypeClass::
/// ReadINI, `0x00460AA9`), which retail `rulesmd.ini` sets nowhere, so that
/// arm is dormant. Trigger: the `DeathAnims=` and `NotHuman=` types. Player
/// effect: such a type plays a death it does not own. The paradropping
/// infantryman's arm ahead of them is `world::infantry_death`'s
/// `PARADROP_INF_DEATH`.
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
