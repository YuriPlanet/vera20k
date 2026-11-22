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
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
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
        ShpFrameDefinition::Generic(def) => resolve_generic_shp_frame(def, facing, frame_index),
    }
}

fn resolve_generic_shp_frame(def: &SequenceDef, facing: u8, frame_index: i32) -> i32 {
    let slot = match def.facing_slots {
        FacingSlots::InfantryTable => infantry_facing_slot(facing),
        FacingSlots::VehicleOctant if def.facings == VEHICLE_FACING_SLOTS => {
            vehicle_facing_slot(facing)
        }
        FacingSlots::VehicleOctant => 0,
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

/// One SHP Unit block's image: its start, the bucket times the block's
/// frames, and the step within it (`0x0073C65B..0x0073C71D`).
fn vehicle_block_frame(def: &SequenceDef, slot: i32, frame_index: i32) -> i32 {
    resolve_shp_frame_parts(
        i32::from(def.start_frame),
        i32::from(def.frame_count),
        i32::from(def.facing_multiplier),
        slot,
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

    /// Whether the countdown moves: every value but the resting -1, which
    /// [`tick_unit_firing_counter`] leaves as it is.
    pub(crate) const fn counting(self) -> bool {
        self.value != -1
    }

    #[cfg(any(test, feature = "test-support"))]
    /// Install an original comparison or persistence fixture's raw counter.
    pub fn set_for_test(&mut self, value: i32) {
        self.value = value;
    }
}

/// UnitClass+6D8, the art `DeathFrames=` counter. The constructor stores -1
/// (`0x00735416`); a lethal hit on a type with `DeathFrames=` above zero
/// starts it at 0 (`UnitClass::ReceiveDamage`, `0x00737DB1..0x00737DBF`), and
/// `UnitClass::AI` counts it (`0x0073635B..0x0073637D`) until the count passes
/// `MaxDeathCounter=`. While it runs the Unit's vt+0x37C (`0x00746C90`)
/// answers yes and its SHP body draws death frames (`0x0073C6AD`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct UnitDeathCounter {
    value: i32,
}

impl Default for UnitDeathCounter {
    fn default() -> Self {
        Self { value: -1 }
    }
}

impl UnitDeathCounter {
    pub const fn get(self) -> i32 {
        self.value
    }

    /// Every value but the constructor's -1.
    pub(crate) const fn running(self) -> bool {
        self.value != -1
    }

    /// `0x00737DB1..0x00737DBF`: the first lethal hit starts the count, a
    /// later one leaves it. Returns whether this hit started it.
    pub(crate) fn start(&mut self) -> bool {
        if self.value != -1 {
            return false;
        }
        self.value = 0;
        true
    }

    /// `0x00736366..0x0073637D`: store the next count and answer whether the
    /// one before it had reached `max` (a signed compare).
    pub(crate) fn advance(&mut self, max: i32) -> bool {
        let old = self.value;
        self.value = old.wrapping_add(1);
        max <= old
    }

    #[cfg(any(test, feature = "test-support"))]
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
///
/// `air_rate` is the type's Ares `AirRate` (0 in a stock game).
pub(crate) fn tick_foot_body_frame_counter(
    entity: &mut crate::sim::game_entity::GameEntity,
    rules: crate::sim::movement::SpeedRules<'_>,
    cadence: FootBodyCadence,
    hover_attack: bool,
    deploy_to_land: bool,
    air_rate: i32,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    binary_frame: u32,
) {
    if foot_body_frame_due(
        entity,
        rules,
        cadence,
        hover_attack,
        deploy_to_land,
        air_rate,
        terrain,
        binary_frame,
    ) {
        entity.body_frame_counter.advance();
    }
}

/// Whether [`tick_foot_body_frame_counter`] advances `entity`'s counter at
/// `binary_frame`; otherwise it changes nothing.
pub(crate) fn foot_body_frame_due(
    entity: &crate::sim::game_entity::GameEntity,
    rules: crate::sim::movement::SpeedRules<'_>,
    cadence: FootBodyCadence,
    hover_attack: bool,
    deploy_to_land: bool,
    air_rate: i32,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    binary_frame: u32,
) -> bool {
    if !entity.category().is_foot()
        || entity.death.is_dying()
        || !entity.lifecycle.object_alive()
        || entity.lifecycle.in_limbo()
        || entity.is_falling_down()
    {
        return false;
    }

    if entity.locomotor.is_none()
        || entity.lift.swap_active()
        || entity.is_warped_out()
        || entity.is_warping_in()
    {
        return false;
    }
    // Ares 3.0 FootClass_Update_AnimRate (100493D0, hook 0x4DA8B2), past the
    // same warp and locomotor-swap refusals: a nonzero `AirRate` alone
    // decides an airborne (GetHeight > 0) Foot's step.
    if air_rate != 0 && crate::sim::movement::air_movement::current_fly_height(entity, terrain) > 0
    {
        return (binary_frame as i32).wrapping_rem(air_rate) == 0;
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
    (walking && (binary_frame as i32) % cadence.walk_rate == 0)
        || (cadence.idle_rate != 0 && !moving && (binary_frame as i32) % cadence.idle_rate == 0)
        // GetHeight (vt+0x1C8) at `0x004DA9CD`.
        || (deploy_to_land
            && crate::sim::movement::air_movement::current_fly_height(entity, terrain) > 0)
}

/// The facing bucket an SHP Unit's body draws (`UnitClass::Draw_Body_And_Turret`
/// `0x0073C613..0x0073C64B`), from the actual type's `Facings=` (UnitType
/// `+0xE3C`) and PrimaryFacing's current 16-bit facing. A disguised Unit
/// (vt+0xC4) draws bucket 0.
///
/// Stock gamemd buckets only an eight-facing type:
/// `(((facing >> 12) + 1) >> 1) + 1 & 7`; any other count draws bucket 0.
/// Ares 3.0 `UnitClass_DrawSHP_FacingsA` (hook `0x0073C613`, Ares.dll
/// `0x10047FE0`, resuming at `0x0073C64B`) buckets any count of at least 8:
/// with `n = floor(log2(Facings))`, `(((facing + 0x2000) >> (15 - n)) + 1) >> 1`
/// masked to `2^n - 1`, which is the stock value for 8. A count below 8, or
/// not positive, draws bucket 0. From `n = 16` the hook neither shifts nor
/// masks. `facing` is PrimaryFacing's current value: stock reads it through
/// FacingClass::Current (`0x004C93D0`, called at `0x0073C636`); the Ares hook
/// through its own inline copy (`0x10048064..0x10048102`,
/// [`FacingClass::current_ares`](crate::sim::movement::FacingClass::current_ares)).
pub fn shp_vehicle_facing_slot(facings: i32, facing: u16, disguised: bool, ares: bool) -> i32 {
    if disguised {
        return 0;
    }
    if !ares {
        return if facings == i32::from(VEHICLE_FACING_SLOTS) {
            i32::from(vehicle_facing_slot((facing >> 8) as u8))
        } else {
            0
        };
    }
    let count = u32::try_from(facings).unwrap_or(0);
    crate::util::direction_tables::ares_facing_bucket(count, facing) as i32
}

/// Unit73C64B..73C725: an active firing counter precedes Stand/Walk and
/// advances one image per two counter steps, in descending image order,
/// with no bound
/// ([`ShpVehicleFireFrames::image`](crate::rules::animation_sequence::ShpVehicleFireFrames::image)).
/// The persistent Foot counter continues to own Stand/Walk. A still body
/// with no `IdleRate=` and a running death counter draws its death image
/// instead of Stand (`0x0073C6AD..0x0073C6D3`), and one whose Foot
/// occupation enable is clear draws its bare bucket. `set` is the type whose
/// frame layout the draw reads (`UnitType+0x6C4`, or the Ares water type,
/// see `UnitClass_DrawSHP_ChangeType1`), and `slot` its facing bucket
/// ([`shp_vehicle_facing_slot`]); each block multiplies the bucket by its
/// own frame count (`0x0073C65B..0x0073C71D`).
pub fn resolve_shp_vehicle_body_frame(
    set: &SequenceSet,
    slot: i32,
    body_frame_counter: u32,
    firing_counter: i32,
    death_counter: i32,
    locomotor_is_moving: bool,
    occupation_enabled: bool,
) -> Option<u16> {
    let cadence = set.shp_vehicle_cadence()?;
    if firing_counter >= 0 {
        return u16::try_from(set.shp_vehicle_fire()?.image(slot, firing_counter)).ok();
    }
    let sequence = if locomotor_is_moving || cadence.idle_rate != 0 {
        SequenceKind::Walk
    } else if death_counter >= 0 {
        return u16::try_from(set.shp_vehicle_death()?.image(death_counter)).ok();
    } else if !occupation_enabled {
        // `0x0073C6D5..0x0073C6DD`: with the Foot occupation enable
        // (`+0x6B6`) clear, the image is the bare facing bucket.
        return u16::try_from(slot).ok();
    } else {
        SequenceKind::Stand
    };
    let def = set.get(&sequence)?;
    let frame_index = if sequence == SequenceKind::Walk && def.frame_count != 0 {
        (body_frame_counter % u32::from(def.frame_count)) as u16
    } else {
        0
    };
    u16::try_from(vehicle_block_frame(def, slot, i32::from(frame_index))).ok()
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
    // Only a Unit or an Aircraft is built with an Animation
    // (`world_spawn::construction`), and an Infantry leaves below whatever it
    // holds, so the frame-end step reads those two classes alone (VERA-only,
    // results unchanged). The whole-store walk also reports dying objects
    // that hold none.
    let animated = |category| {
        matches!(
            category,
            crate::map::entities::EntityCategory::Unit
                | crate::map::entities::EntityCategory::Aircraft
        )
    };
    debug_assert!(
        tick_dying
            || !crate::sim::touch_log::live_read_check_enabled()
            || entities
                .values_of_category(crate::map::entities::EntityCategory::Structure)
                .all(|entity| entity.animation.is_none()),
        "a Structure holds an Animation the frame-end step would not advance"
    );
    entities.for_each_entry_mut(
        |category| tick_dying || animated(category),
        |entry| {
            let id = entry.id();
            let entity = entry.get();
            if entity.death.is_dying() && !tick_dying {
                return;
            }
            // The stage steps in TechnoClass::AI_Update (`0x006FAC4D`), which a
            // warped object never reaches (`GameEntity::ai_frozen`): its pose holds.
            if !entity.death.is_dying() && entity.ai_frozen() {
                return;
            }
            // Its Doing's sequence, which its actions restart and whose stage
            // steps in its own turn (`sim::movement::infantry_action`); this
            // frame-end clock leaves it.
            if crate::sim::movement::infantry_action::doing_owns_sequence(entity) {
                return;
            }
            if entity.animation.is_none() {
                // Dying entity with no animation → ready for despawn.
                if entity.death.is_dying() {
                    dying_finished.push(id);
                }
                return;
            }
            let entity = entry.into_mut();
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
                    return;
                }
                let Some(seq_set) =
                    sequence_set_for_type(sequences, rules, interner.resolve(type_ref))
                else {
                    dying_finished.push(id);
                    return;
                };
                let Some(def) = seq_set.get(&anim.sequence) else {
                    dying_finished.push(id);
                    return;
                };
                advance_animation(anim, def, game_options);
                if anim.finished {
                    dying_finished.push(id);
                }
                return;
            }

            // Infantry never enters this cascade: Doing/Stage is its only
            // sequence clock. Motion does not switch the sequence either: the Unit
            // SHP draw picks Walk or Stand by the locomotor's Is_Moving (vt+0x10 at
            // `0x0073C696`, `resolve_shp_vehicle_body_frame`).
            let seq_set = sequence_set_for_type(sequences, rules, interner.resolve(type_ref));

            // Advance frame timing.
            let Some(set) = seq_set else {
                return;
            };
            let Some(def) = set.get(&anim.sequence) else {
                return;
            };

            // UnitClass SHP Stand/Walk images are selected from Foot's persistent
            // body counter at draw time. Do not advance a second relative clock or
            // reset cadence when the generic visual sequence changes.
            if category == crate::map::entities::EntityCategory::Unit
                && set.shp_vehicle_cadence().is_some()
                && matches!(anim.sequence, SequenceKind::Stand | SequenceKind::Walk)
            {
                return;
            }

            if let Some(next) = advance_animation(anim, def, game_options) {
                if let Some(facing) = def.completion_facing {
                    snap_completion_facing(&mut entity.body_facing, facing, binary_frame);
                }
                anim.switch_to(next);
            }
        },
    );

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

#[cfg(all(test, tests_sim_a_b))]
#[path = "animation_tests.rs"]
mod tests;
