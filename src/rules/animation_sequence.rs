//! Rules-owned immutable animation-sequence vocabulary and catalog construction.
//!
//! `SequenceKind`/`SequenceDef`/`SequenceSet` describe the art.ini frame
//! layout for SHP-bodied object families; `build_animation_sequence_catalog`
//! assembles the immutable per-type catalog from registered rules types.
//! Animation *runtime* — per-entity `Animation` state, frame resolution, and
//! ticking — stays in `sim::animation`, which re-exports these types for its
//! consumers.
//!
//! Float never appears here; all timing is integer frame counts.
//!
//! ## Dependency rules
//! - Part of rules/ and depends only on rules siblings (ruleset, sequence
//!   parsers) plus std/serde.
//! - No dependency on sim/ or runtime scheduling.

use std::collections::BTreeMap;

/// Standard number of facing directions for infantry animations.
const INFANTRY_FACINGS: u8 = 8;

/// Default reached native frames per standing image.
const DEFAULT_STAND_FRAME_DELAY: u16 = 1;

/// Default reached native frames per walk-cycle image.
const DEFAULT_WALK_FRAME_DELAY: u16 = 3;

/// Default reached native frames per idle-fidget image.
const DEFAULT_IDLE_FRAME_DELAY: u16 = 3;

/// Default reached native frames per death image.
const DEFAULT_DIE_FRAME_DELAY: u16 = 1;

/// Named animation sequence types.
///
/// Each corresponds to a range of frames in the SHP file, defined
/// by a `SequenceDef`. An entity plays one sequence at a time.
///
/// Maps to art.ini sequence keys: Ready/Guard → Stand, FireUp → Attack, etc.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SequenceKind {
    /// Standing alert pose. Default state for idle entities (Ready/Guard in INI).
    Stand,
    /// Walking movement cycle. Active while entity has a MovementTarget.
    Walk,
    /// Standing still while prone (Prone= in INI).
    Prone,
    /// Moving while prone (Crawl= in INI).
    Crawl,
    /// Firing primary weapon while standing (FireUp= in INI). Transitions to Stand.
    Attack,
    /// Firing primary weapon while prone (FireProne= in INI). Transitions to Stand.
    FireProne,
    /// Transition from standing to prone (Down= in INI). Transitions to Prone.
    Down,
    /// Transition from prone to standing (Up= in INI). Transitions to Stand.
    Up,
    /// Random fidget animation while idle (Idle1= in INI).
    Idle1,
    /// Second idle fidget variant (Idle2= in INI).
    Idle2,
    /// Death animation variant 1. Plays once, holds last frame.
    Die1,
    /// Death animation variant 2. Plays once, holds last frame.
    Die2,
    /// Death animation variant 3. Plays once, holds last frame.
    Die3,
    /// Death animation variant 4. Plays once, holds last frame.
    Die4,
    /// Death animation variant 5. Plays once, holds last frame.
    Die5,
    /// Victory/celebration animation (Cheer= in INI).
    Cheer,
    /// Parachute landing animation (Paradrop= in INI).
    Paradrop,
    /// Panicked running (Panic= in INI). Uses Walk-like timing.
    Panic,
    /// Transition from standing to deployed stance (Deploy= in INI).
    Deploy,
    /// Transition from deployed back to standing (Undeploy= in INI).
    Undeploy,
    /// Standing in deployed stance (Deployed= in INI, e.g., GI sandbags).
    Deployed,
    /// Firing while deployed (DeployedFire= in INI).
    DeployedFire,
    /// Idle fidget while deployed (DeployedIdle= in INI).
    DeployedIdle,
    /// Firing secondary weapon while standing (SecondaryFire= in INI, YR only).
    SecondaryFire,
    /// Firing secondary weapon while prone (SecondaryProne= in INI, YR only).
    SecondaryProne,
    /// Swimming movement cycle (Swim= in INI, e.g., Tanya in water).
    Swim,
    /// Flying movement cycle (Fly= in INI, e.g., Rocketeer).
    Fly,
    /// Firing while flying (FireFly= in INI).
    FireFly,
    /// Hovering in place (Hover= in INI).
    Hover,
    /// Treading water / ground cycle (Tread= in INI).
    Tread,
    /// Firing while swimming (WetAttack= in INI).
    WetAttack,
    /// Idle fidget while swimming variant 1 (WetIdle1= in INI).
    WetIdle1,
    /// Idle fidget while swimming variant 2 (WetIdle2= in INI).
    WetIdle2,
}

/// How a sequence behaves when it reaches its last frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum LoopMode {
    /// Restart from frame 0 when reaching the end (walk, stand).
    Loop,
    /// Play once and freeze on the last frame (death animations).
    HoldLast,
    /// Play once then switch to a different sequence (attack → stand).
    TransitionTo(SequenceKind),
}

/// Which native facing-to-frame-slot rule a sequence follows.
///
/// The original engine has exactly two of these — one per SHP-bodied object
/// family — and they are separate code paths, not independent knobs. They
/// differ on *both* axes at once: which way the frame blocks rotate, and where
/// the quantizer puts its slot boundaries. Frame block 0 is screen-north
/// (cell NW) in both.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum FacingSlots {
    /// Infantry bodies: a 32-entry lookup table indexed by the facing
    /// quantized to 32 steps. Slots run **counter-clockwise** from
    /// screen-north — NW=0, W=1, SW=2, S=3, SE=4, E=5, NE=6, N=7.
    #[default]
    InfantryTable,
    /// SHP vehicle bodies: the facing rounded to the nearest octant, then
    /// advanced one slot. Slots run **clockwise** from screen-north —
    /// NW=0, N=1, NE=2, E=3, SE=4, S=5, SW=6, W=7.
    VehicleOctant,
}

/// Definition of one animation sequence within an SHP file.
///
/// Describes the frame range, timing, and looping behavior for a named
/// sequence. Multiple `SequenceDef`s grouped into a `SequenceSet` define
/// all animations available for one object type.
///
/// ## Frame index formula
/// For directional sequences:
///   `start_frame + facing_slot * facing_multiplier + frame_within_sequence`
/// For non-directional (facings == 1):
///   `start_frame + frame_within_sequence`
///
/// ## Facing convention
/// RA2's DirStruct byte (0–255) is clockwise in *cell* space (0=N, 64=E,
/// 128=S, 192=W), but SHP frame blocks start at screen-north, which is cell
/// NW. `facing_slots` selects which of the two native conversions applies.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SequenceDef {
    /// First SHP frame index for this sequence.
    pub start_frame: u16,
    /// Number of animation frames per facing direction.
    pub frame_count: u16,
    /// Number of facing directions (1 = non-directional, 8 = infantry standard).
    pub facings: u8,
    /// Frame stride between facings — the offset applied per facing increment.
    /// From art.ini's 3rd sequence field. Typically equals `frame_count` for
    /// contiguous packing. If 0, the animation is facing-independent.
    pub facing_multiplier: u16,
    /// Reached native frames per image. Lower = faster animation.
    pub frame_delay: u16,
    /// Whether native game-speed normalization applies to this action delay.
    pub normalized: bool,
    /// Facing byte snapped when this definition completes and dispatches its
    /// transition. `None` is native record value -1 (no completion update).
    #[serde(default)]
    pub completion_facing: Option<u8>,
    /// Behavior when the sequence reaches its final frame.
    pub loop_mode: LoopMode,
    /// Which native facing-to-slot rule converts the facing byte into a frame
    /// block index. Infantry and SHP vehicles use different ones.
    pub facing_slots: FacingSlots,
}

/// Collection of sequence definitions for one object type.
///
/// Maps `SequenceKind` → `SequenceDef`. Not all kinds need to be present;
/// entities hold their current frame if the active sequence is missing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SequenceSet {
    sequences: BTreeMap<SequenceKind, SequenceDef>,
    /// Signed42-action InfantryType records. Generic SHP definitions are only
    /// a presentation projection; action admission never reads narrowed counts.
    #[serde(default)]
    infantry_actions: Option<Vec<crate::rules::infantry_sequence::InfantrySequenceEntry>>,
    /// Derived type configuration for UnitClass SHP bodies. This is not
    /// per-entity animation state; the persistent native state is the Foot
    /// body-frame counter on `GameEntity`.
    #[serde(default)]
    shp_vehicle_cadence: Option<ShpVehicleCadence>,
}

/// Rules-owned divisors consumed by the SHP Unit body-counter path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ShpVehicleCadence {
    pub walk_rate: i32,
    pub idle_rate: i32,
}

impl ShpVehicleCadence {
    pub const fn native_defaults() -> Self {
        Self {
            walk_rate: 1,
            idle_rate: 0,
        }
    }
}

impl Default for ShpVehicleCadence {
    fn default() -> Self {
        Self::native_defaults()
    }
}

impl SequenceSet {
    /// Create an empty SequenceSet.
    pub fn new() -> Self {
        Self {
            sequences: BTreeMap::new(),
            infantry_actions: None,
            shp_vehicle_cadence: None,
        }
    }

    pub(crate) fn set_infantry_actions(
        &mut self,
        actions: Vec<crate::rules::infantry_sequence::InfantrySequenceEntry>,
    ) {
        assert_eq!(actions.len(), 42);
        self.infantry_actions = Some(actions);
    }

    #[cfg(test)]
    pub(crate) fn infantry_action(
        &self,
        action: i32,
    ) -> Option<&crate::rules::infantry_sequence::InfantrySequenceEntry> {
        self.infantry_actions
            .as_ref()?
            .get(usize::try_from(action).ok()?)
    }

    /// Mark this as a UnitClass SHP frame layout and attach its rules-owned
    /// cadence. The frame blocks themselves remain art-owned `SequenceDef`s.
    pub fn set_shp_vehicle_cadence(&mut self, cadence: ShpVehicleCadence) {
        self.shp_vehicle_cadence = Some(cadence);
    }

    pub fn shp_vehicle_cadence(&self) -> Option<ShpVehicleCadence> {
        self.shp_vehicle_cadence
    }

    /// Add a sequence definition.
    pub fn insert(&mut self, kind: SequenceKind, def: SequenceDef) {
        self.sequences.insert(kind, def);
    }

    /// Look up a sequence definition by kind.
    pub fn get(&self, kind: &SequenceKind) -> Option<&SequenceDef> {
        self.sequences.get(kind)
    }

    /// Number of defined sequences.
    pub fn len(&self) -> usize {
        self.sequences.len()
    }

    /// Whether no sequences are defined.
    pub fn is_empty(&self) -> bool {
        self.sequences.is_empty()
    }
}

/// Build the immutable per-type animation catalog used by authoritative world ticks.
///
/// The catalog is built from every registered rules type, not the currently
/// spawned entity set. Production and scripted spawns therefore cannot depend
/// on an app-side atlas refresh before their animation or death timing advances.
pub(crate) fn build_animation_sequence_catalog(
    rules: &crate::rules::ruleset::RuleSet,
    infantry_sequences: Option<&crate::rules::infantry_sequence::InfantrySequenceRegistry>,
) -> BTreeMap<String, SequenceSet> {
    let mut catalog = BTreeMap::new();

    for type_id in &rules.infantry_ids {
        let Some(object) = rules.object(type_id) else {
            continue;
        };
        let sequence_name = rules
            .art_registry
            .resolve_metadata_entry(&object.id, &object.image)
            .and_then(|entry| entry.sequence.as_deref());
        let sequence_set = sequence_name
            .and_then(|name| infantry_sequences?.get(&name.to_ascii_uppercase()))
            .map(crate::rules::infantry_sequence::build_sequence_set)
            .unwrap_or_else(default_infantry_sequences);
        catalog.insert(object.id.clone(), sequence_set);
    }

    for type_id in &rules.building_ids {
        let Some(object) = rules.object(type_id) else {
            continue;
        };
        catalog.insert(object.id.clone(), default_building_sequences());
    }

    for type_id in rules.vehicle_ids.iter().chain(&rules.aircraft_ids) {
        let Some(object) = rules.object(type_id) else {
            continue;
        };
        let Some(art) = rules
            .art_registry
            .resolve_metadata_entry(&object.id, &object.image)
        else {
            continue;
        };
        if art.voxel || (art.walk_frames.is_none() && art.firing_frames.is_none()) {
            continue;
        }
        let cadence = ShpVehicleCadence {
            walk_rate: object.walk_rate,
            idle_rate: object.idle_rate,
        };
        catalog.insert(
            object.id.clone(),
            crate::rules::shp_vehicle_sequence::build_shp_vehicle_sequences(art, cadence),
        );
    }

    catalog
}

/// Create the default infantry sequence set matching RA2's standard frame layout.
///
/// Based on RA2's standard infantry sequence defaults:
/// - Stand: frames 0–7 (1 frame × 8 facings)
/// - Walk: frames 8–55 (6 frames × 8 facings)
/// - Idle1: frames 56–70 (15 frames, non-directional)
/// - Idle2: frames 71–85 (15 frames, non-directional)
/// - Die1: frames 86–100 (15 frames, non-directional)
/// - Die2: frames 101–115 (15 frames, non-directional)
pub fn default_infantry_sequences() -> SequenceSet {
    let mut set: SequenceSet = SequenceSet::new();
    // InfantryType52392C constructs all 42 action records with zero counts.
    // The legacy SHP layout below is a presentation fallback, never gameplay
    // admission for an unbound/missing Sequence section.
    set.set_infantry_actions(vec![
        crate::rules::infantry_sequence::InfantrySequenceEntry::default(); 42
    ]);

    set.insert(
        SequenceKind::Stand,
        SequenceDef {
            start_frame: 0,
            frame_count: 1,
            facings: INFANTRY_FACINGS,
            facing_multiplier: 1,
            frame_delay: DEFAULT_STAND_FRAME_DELAY,
            normalized: false,
            completion_facing: None,
            loop_mode: LoopMode::Loop,
            facing_slots: FacingSlots::InfantryTable,
        },
    );
    set.insert(
        SequenceKind::Walk,
        SequenceDef {
            start_frame: 8,
            frame_count: 6,
            facings: INFANTRY_FACINGS,
            facing_multiplier: 6,
            frame_delay: DEFAULT_WALK_FRAME_DELAY,
            normalized: false,
            completion_facing: None,
            loop_mode: LoopMode::Loop,
            facing_slots: FacingSlots::InfantryTable,
        },
    );
    set.insert(
        SequenceKind::Idle1,
        SequenceDef {
            start_frame: 56,
            frame_count: 15,
            facings: 1,
            facing_multiplier: 0,
            frame_delay: DEFAULT_IDLE_FRAME_DELAY,
            normalized: true,
            completion_facing: None,
            loop_mode: LoopMode::TransitionTo(SequenceKind::Stand),
            facing_slots: FacingSlots::InfantryTable,
        },
    );
    set.insert(
        SequenceKind::Idle2,
        SequenceDef {
            start_frame: 71,
            frame_count: 15,
            facings: 1,
            facing_multiplier: 0,
            frame_delay: DEFAULT_IDLE_FRAME_DELAY,
            normalized: true,
            completion_facing: None,
            loop_mode: LoopMode::TransitionTo(SequenceKind::Stand),
            facing_slots: FacingSlots::InfantryTable,
        },
    );
    set.insert(
        SequenceKind::Die1,
        SequenceDef {
            start_frame: 86,
            frame_count: 15,
            facings: 1,
            facing_multiplier: 0,
            frame_delay: DEFAULT_DIE_FRAME_DELAY,
            normalized: false,
            completion_facing: None,
            loop_mode: LoopMode::HoldLast,
            facing_slots: FacingSlots::InfantryTable,
        },
    );
    set.insert(
        SequenceKind::Die2,
        SequenceDef {
            start_frame: 101,
            frame_count: 15,
            facings: 1,
            facing_multiplier: 0,
            frame_delay: DEFAULT_DIE_FRAME_DELAY,
            normalized: false,
            completion_facing: None,
            loop_mode: LoopMode::HoldLast,
            facing_slots: FacingSlots::InfantryTable,
        },
    );

    set
}

/// Create default building sequence set (single idle frame, no animation).
///
/// Buildings typically use frame 0 as their idle state. Animated buildings
/// (e.g., power plant, radar dome) will need per-type overrides later.
pub fn default_building_sequences() -> SequenceSet {
    let mut set: SequenceSet = SequenceSet::new();

    set.insert(
        SequenceKind::Stand,
        SequenceDef {
            start_frame: 0,
            frame_count: 1,
            facings: 1,
            facing_multiplier: 0,
            frame_delay: DEFAULT_STAND_FRAME_DELAY,
            normalized: false,
            completion_facing: None,
            loop_mode: LoopMode::Loop,
            facing_slots: FacingSlots::InfantryTable,
        },
    );

    set
}
