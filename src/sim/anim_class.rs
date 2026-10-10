//! Scheduler-owned ordinary SHP animation objects.
//!
//! `AnimStore` owns animation storage while `world::LogicVector` owns live AI
//! order. This module implements the verified AnimClass lifecycle:
//! constructor/reveal, first-AI guard, start delay and its `Start` edge,
//! `Middle`'s scorch and crater, logic-frame timing, loops, reverse/ping-pong,
//! Next, trailer, sound identity, owner attachment, conceal, deferred
//! deletion, and the `Bouncer=` chunk: its constructor launch, its
//! `BounceClass` flight and its landing (`Simulation::anim_bounce_step`). Its
//! producers are building slots and damage fires, tile and crate animations,
//! combat explosions, death debris, weapon and occupant muzzle flashes,
//! parachute canopies, teleport warps, superweapon invokes, Lightning Storm
//! bolts, bridge collapse explosions, wakes and ore twinkles.
//!
//! ## Residuals
//!
//! The `IsMeteor=` constructor arm and a dry landing's `Spawns=`/`IsTiberium=`
//! work are recorded at [`anim_constructor_draws`] and
//! `Simulation::anim_bounce_step`; no death or weapon produces them.
//!
//! RESIDUAL (M11b) — **the per-frame damage arm.** `AnimClass::AI
//! 0x00424507..0x0042464C` accumulates the type's `Damage=` (parsed, see
//! `AnimTypeRuntimeConfig::damage`) into `AnimClass+0x188`, which the
//! constructor seeds to `1.0`, and calls `Apply_area_damage` with a
//! truncating `Math__ftol` whenever the accumulator reaches 1.0, subtracting
//! the emitted integer back as a double. The warhead is
//! `[CombatDamage] C4Warhead` when the anim's own section name is `INVISO`
//! (string at `0x008182F8`, NOT the neighbouring `RING1` at `0x008182F0`) and
//! `[CombatDamage] FlameDamage2` otherwise; a `TerrainClass` owner
//! (`What_Am_I` -> `0x24`) multiplies the per-frame value by 5.
//! - Trigger: the `FIRE3` anims `BuildingClass::DestructionEffects` starts,
//!   for an `Explodes=` building, in each cardinal neighbour cell whose own
//!   overlay has `Explodes=yes` (`0x00441A2B..0x00441B2E`; the neighbour's
//!   `+0x44` overlay tested at `0x00441A90..0x00441AC2`, name loaded at
//!   `0x00441AEC`).
//!   No stock overlay has `Explodes=yes`, the `BURN-S/M/L` family is commented
//!   out of `[Animations]` and `INVISO` has no stock producer, so stock play
//!   never reaches it.
//! - Player effect: `[FIRE3] Damage=.003` plus the `1.0` seed means exactly one
//!   point of `Fire2` area damage on the fire's first ticked frame and then
//!   none for another ~334 frames.
//! - Frequency: none in stock; magnitude 1.
//! - Downstream risk: low — it applies damage through the existing
//!   `Apply_area_damage` path and consumes no RNG.
//!   The `TerrainClass` x5 multiplier has no verified stock producer at all:
//!   its only source of a terrain-owned anim is `TerrainClass::Catch_Fire @
//!   0x0071C5B0`, which has no code caller in active YR.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::rules::art_data::AnimTypeRuntimeConfig;
use crate::rules::house_colors::HouseColorIndex;
use crate::rules::ruleset::RuleSet;
use crate::sim::bounce::{BounceOutcome, BounceState};
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::{RawCellKey, RawCellOccupationGrid, infantry_raw_occupation_mask};
use crate::sim::timer::CdTimer;
use crate::sim::touch_log::{TouchLog, Touched};
use crate::sim::world::{LifecycleOutput, SimSoundEvent, Simulation};
use crate::util::fixed_math::SimFixed;
use crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS;
use crate::util::native_x87::{NativeF64Bits, NativeX87Error, X87Chop53};

pub type AnimId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AnimWorldCoord {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl AnimWorldCoord {
    /// Decompose the absolute lepton coordinate into (cell, sub-cell,
    /// height level). The level is the floor of Z over the native level height
    /// and is for consumers that key on a level (depth rows, lighting); a
    /// screen position comes from the exact `z` field, which may sit between
    /// levels (a muzzle, an airburst).
    pub(crate) fn to_cell_sub_z(
        &self,
    ) -> (
        u16,
        u16,
        crate::util::fixed_math::SimFixed,
        crate::util::fixed_math::SimFixed,
        u8,
    ) {
        let rx = self
            .x
            .div_euclid(LEPTONS_PER_CELL)
            .clamp(0, i32::from(u16::MAX)) as u16;
        let ry = self
            .y
            .div_euclid(LEPTONS_PER_CELL)
            .clamp(0, i32::from(u16::MAX)) as u16;
        let sub_x =
            crate::util::fixed_math::SimFixed::from_num(self.x.rem_euclid(LEPTONS_PER_CELL));
        let sub_y =
            crate::util::fixed_math::SimFixed::from_num(self.y.rem_euclid(LEPTONS_PER_CELL));
        let z = self
            .z
            .div_euclid(LEVEL_HEIGHT_LEPTONS)
            .clamp(0, i32::from(u8::MAX)) as u8;
        (rx, ry, sub_x, sub_y, z)
    }

    /// Compose the absolute lepton coordinate a producer's (cell, sub-cell,
    /// height-level) triple names: `Level * LevelHeight`, the product the
    /// native level-keyed producers form (`MOVSX (Level); IMUL [0x00ABDE88]`
    /// in `MapClass::CollapseBridge_EW_Low`, `0x00575391`). A producer that
    /// knows its exact Z builds the coordinate directly instead.
    pub(crate) fn from_cell_sub_z(
        rx: u16,
        ry: u16,
        sub_x: crate::util::fixed_math::SimFixed,
        sub_y: crate::util::fixed_math::SimFixed,
        z: u8,
    ) -> Self {
        Self {
            x: i32::from(rx)
                .wrapping_mul(LEPTONS_PER_CELL)
                .wrapping_add(sub_x.to_num::<i32>()),
            y: i32::from(ry)
                .wrapping_mul(LEPTONS_PER_CELL)
                .wrapping_add(sub_y.to_num::<i32>()),
            z: i32::from(z).wrapping_mul(LEVEL_HEIGHT_LEPTONS),
        }
    }
}

/// `AnimClass` constructor `drawFlags` for a combat explosion.
///
/// gamemd-derived: `BulletClass::DetonateAtCoord` pushes the literal `0x2600`
/// at `0x00469C82`, and identically at `0x0046A2E5`. The individual bits are
/// UNCHECKED — only the `| 0x2000` that `AnimClass::DrawIt @ 0x0042304B` adds
/// before `CC_Draw_Shape`, and the `AnimClass::SaveExtras` round trip, were
/// read — so the word is carried verbatim, exactly as every other producer in
/// this engine carries it.
pub const COMBAT_EXPLOSION_DRAW_FLAGS: u32 = 0x2600;

/// `AnimClass` constructor `zAdjust` for a combat explosion.
///
/// gamemd-derived: the argument at `0x00469C81` is the return of
/// `0x0048ACE0`, whose whole body is `MOV EAX,0xFFFFFFF1; RET 0xC`
/// (`disassemble_bytes 0x0048ACE0`) — it takes the impact `CoordStruct` by
/// value and ignores it, always yielding `-15`.
pub const COMBAT_EXPLOSION_Z_ADJUST: i32 = -15;

const LEPTONS_PER_CELL: i32 = crate::util::lepton::LEPTONS_PER_CELL_I32;
/// The native level height. `AnimClass` coordinates are ordinary world
/// `CoordStruct` leptons, so a producer that places an anim by height level
/// multiplies by the level step, 104 (`util::lepton::LEPTONS_PER_LEVEL` records
/// the runtime captures of the per-module level globals; the image holds
/// zeroes). The one such producer read in the binary is MapClass's: the bridge
/// walkers form `Level * [0x00ABDE88]`, a Map-module scalar written only by
/// the static initialiser `0x005617E0` and otherwise read by other Map-module
/// code (shroud reveal, bridge edge tiles); its
/// value is taken from those captures, not re-proved here. No native 128 was
/// found, though the `AnimClass` constructor and `AI` bodies were not audited
/// for one. This store used to keep a private 128-per-level Z while half its
/// producers already wrote 104-frame leptons.
const LEVEL_HEIGHT_LEPTONS: i32 = crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS;
const TRAILER_DRAW_FLAGS: u32 = 0x600;
const DAMAGE_FIRE_SLOT_COUNT: usize = 8;

/// Pure YR `AnimClass_UpdateBouncePhysics` directional-frame projection.
#[cfg(test)]
pub fn directional_tumble_frame(running_frames: i32, bucket8: i32, global_frame: i32) -> i32 {
    let running = running_frames.max(1);
    running * ((-1 - bucket8) & 7) + (global_frame / 3).rem_euclid(running)
}

#[cfg(test)]
pub fn settled_bounce_frame(running_frames: i32) -> i32 {
    running_frames.wrapping_mul(8).wrapping_add(1)
}

/// `AnimClass_Update` @ 0x00423f37: landing consumes two inclusive rolls.
#[cfg(test)]
pub fn bounce_spawn_count(has_spawns: bool, spawn_count: i32, roll_a: i32, roll_b: i32) -> i32 {
    if !has_spawns || spawn_count <= 0 {
        return 0;
    }
    assert!((0..=spawn_count).contains(&roll_a));
    assert!((0..=spawn_count).contains(&roll_b));
    roll_a.wrapping_add(roll_b)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimDrawDetailInput {
    pub frame_rate_below_minimum: bool,
    pub type_detail_level: i32,
    pub game_detail_level: i32,
    pub hidden: bool,
    pub special_hidden: bool,
    pub type_special_hide: bool,
}

/// `AnimClass__DrawIt` @ 0x00422fd8: visibility gates precede flag selection.
/// The original422CA0..4238AF body never reads Anim+19B or Object+90;
/// Display membership or a retained Building slot supplies the draw receiver.
pub fn anim_draw_detail_visible(input: AnimDrawDetailInput) -> bool {
    !(input.frame_rate_below_minimum && input.type_detail_level > 1)
        && !input.hidden
        && input.type_detail_level <= input.game_detail_level
        && !(input.special_hidden && input.type_special_hide)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimTranslucencyInput {
    pub base_flags: u32,
    pub forced_translucent: bool,
    pub forced_uses_75: bool,
    pub translucency_detail_level: i32,
    pub game_detail_level: i32,
    pub translucent_ramp: bool,
    pub current_frame: i32,
    pub frame_count: i32,
    pub explicit_translucency: i32,
    pub instance_ramp: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimTranslucencyResult {
    pub draw: bool,
    pub flags: u32,
}

fn translucency_flag(level: i32) -> u32 {
    match level {
        25 => 2,
        50 => 4,
        75 => 6,
        _ => 0,
    }
}

/// `AnimClass__DrawIt` @ 0x00423061: preserve native 25/50/75 flag values.
pub fn anim_translucency_selection(input: AnimTranslucencyInput) -> AnimTranslucencyResult {
    let mut flags = input.base_flags;
    if input.forced_translucent {
        flags |= if input.forced_uses_75 { 6 } else { 4 };
        return AnimTranslucencyResult { draw: true, flags };
    }
    if input.translucency_detail_level > input.game_detail_level {
        return AnimTranslucencyResult { draw: true, flags };
    }
    if input.translucent_ramp {
        if input.instance_ramp >= 15 {
            return AnimTranslucencyResult { draw: false, flags };
        }
        let frame = i64::from(input.current_frame);
        let frame_count = i64::from(input.frame_count);
        flags |= if frame * 5 > frame_count * 3 {
            6
        } else if frame * 5 > frame_count * 2 {
            4
        } else if frame * 5 > frame_count {
            2
        } else {
            0
        };
        return AnimTranslucencyResult { draw: true, flags };
    }
    if input.explicit_translucency > 0 {
        return AnimTranslucencyResult {
            draw: input.instance_ramp < 15,
            flags: flags | translucency_flag(input.explicit_translucency),
        };
    }
    if input.instance_ramp > 15 {
        return AnimTranslucencyResult { draw: false, flags };
    }
    if input.instance_ramp > 5 {
        flags |= 4;
    }
    AnimTranslucencyResult { draw: true, flags }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AnimRuntime {
    pub current_frame: i32,
    pub frame_step: i32,
    pub delay_remaining: u16,
    pub rate_reload: u16,
    pub frame_timer: CdTimer,
    pub loop_remaining: u8,
    pub first_ai_guard: bool,
    pub constructor_reverse: bool,
    /// Native Anim+19B: request expiry on a later AI visit (owner expiry425196).
    /// UnInit4255B0 preserves this byte. Pending deletion and retained Logic/
    /// Display membership own retirement independently.
    pub inactive: bool,
    /// Anim+19E, pause/resume425260/425270. The absolute frame timer keeps running.
    #[serde(default)]
    pub paused: bool,
}

/// Per-instance `AnimClass::DrawIt` bytes that are independent of the art
/// type. They remain serialized simulation state because native effects may
/// set them between frames; presentation only reads this resolved input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AnimDrawRuntime {
    /// AnimClass `+0x19d`: unconditional draw suppression.
    pub hidden: bool,
    /// AnimClass `+0x199`: applies only with the unresolved type `+0x374` bit.
    pub special_hidden: bool,
    /// AnimClass `+0x178`: ramp/age input used by translucency selection.
    pub translucency_ramp: u8,
    /// AnimClass `+0x119`: force the type-selected 50/75% draw family.
    pub forced_translucent: bool,
    /// Type `+0x368` = art.ini `DoubleThick=`, consulted only inside the
    /// `+0x119` forced-translucency arm: `AnimClass::DrawIt` reads
    /// `0x0042306B MOV CL,[EAX+0x368]` and takes `OR EBX,0x6` when set,
    /// `OR EBX,0x4` when clear. `AnimTypeClass::ReadINI @ 0x00427D00` stores
    /// `DoubleThick=` at `param_1[0xda]` (= byte offset `0x368`), which closes
    /// the art-key mapping this comment previously left open. It stays a
    /// producer-supplied instance byte rather than a type read because nothing
    /// in this engine sets `+0x119` yet — the first producer that does should
    /// feed it from `AnimTypeRuntimeConfig::double_thick`.
    pub forced_uses_75: bool,
}

/// Retained Anim instance inputs to Display, independent of vector membership.
/// Constructor422131 copies type+340 to instance+104; SetOwner424B50 tests
/// Object+74 to gate only its normal-detach re-registration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct AnimDisplayState {
    marked_on_map: bool,
    y_sort_adjust: i32,
}

impl AnimDisplayState {
    pub(crate) fn y_sort_adjust(&self) -> i32 {
        self.y_sort_adjust
    }
}

/// The producer-selected AnimClass+0xD4 palette conversion. A fixed scheme
/// serves CellAnim; a deploy animation retains the house selected by
/// Techno705D70 at creation. Presentation resolves that identity through the
/// match's existing HouseColorMap, never through the owner's later state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AnimRemap {
    ColorScheme(HouseColorIndex),
    House(InternedId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnimObject {
    pub stable_id: AnimId,
    pub native_unique_id: i32,
    pub type_id: InternedId,
    /// World leptons — but **owner-relative whenever `owner_entity` is set**,
    /// exactly as `AnimClass::SetOwnerObject @ 0x00424B50` stores it. Read it
    /// through [`Simulation::anim_absolute_coord`] for a world position; read
    /// the field directly only where native reads the stored `ObjectClass`
    /// coordinate directly, which is the multiplayer sync checksum
    /// (`Compute_Game_Sync_Checksum @ 0x0064DAB0` folds `+0x9c`/`+0xa0`) and
    /// the state hash. All three axes are world leptons.
    pub world_coord: AnimWorldCoord,
    pub draw_flags: u32,
    pub z_adjust: i32,
    /// Optional ConvertClass palette selected by a producer after construction
    /// (for example OverlayClass CellAnim over a Tiberium cell).
    #[serde(default)]
    remap: Option<AnimRemap>,
    pub effective_end: i32,
    pub effective_loop_end: i32,
    pub runtime: AnimRuntime,
    pub draw_runtime: AnimDrawRuntime,
    /// AnimClass `+0x196`: use the containing CellClass draw/palette authority.
    #[serde(default)]
    pub use_cell_drawer: bool,
    /// AnimClass `+0x197`: created from a terrain tile animation descriptor.
    #[serde(default)]
    pub terrain_attached: bool,
    /// LogicClass membership is reconstructed from the serialized vector.
    /// ObjectClass::Save does not persist its local membership byte.
    #[serde(skip)]
    pub in_logic_vector: bool,
    pub owner_entity: Option<u64>,
    /// Derived reverse index of Building+55C's slot reference, rebuilt on load.
    /// Independent of both Anim+118 and the Anim+CC Object-owner attachment.
    #[serde(skip)]
    pub building_slot: Option<(u64, u8)>,
    /// Anim+118: ctor421F44 clears it; Building45199B sets it. It suppresses
    /// independent drawing and admits451B40, even after the slot is cleared.
    #[serde(default)]
    building_anim: bool,
    /// Derived reverse index of Building+5C8's damage-fire slot, rebuilt on
    /// load. It survives owner expiry until the Anim's own UnInit broadcast.
    #[serde(skip)]
    pub(crate) damage_fire_slot: Option<(u64, u8)>,
    /// Anim+179: constructor421FB0 clears it; normal AI completion424B31
    /// sets it before UnInit. Building451B40 distinguishes a completed
    /// retraction from an animation removed for another reason.
    #[serde(default)]
    completed: bool,
    pub start_sound_active: bool,
    pub stop_sound_id: Option<InternedId>,
    pub(crate) display: AnimDisplayState,
    /// `AnimClass+0x128`: a `Bouncer=` chunk's BounceClass body. Its presence
    /// is `+0x194` for the ported arm (the `IsMeteor=` arm is not built).
    #[serde(default)]
    pub bounce: Option<BounceState>,
    /// `AnimClass+0x180`, the owning House (`SetOwnerHouse @ 0x00424CA0`).
    /// Its expiry also ends the anim (`0x004251D5`). The PsiWarning
    /// visibility test reads it against the local player (`0x0043B4C0`).
    #[serde(default)]
    owner_house: Option<InternedId>,
    /// `AnimClass+0x17C`, the bullet whose expiry ends this anim
    /// (`0x00424C90` stores it; `AnimClass::PointerExpired @ 0x00425150`
    /// clears it and UnInits the anim). The nuclear missile's warning marker
    /// rides its missile this way.
    #[serde(default)]
    attached_bullet: Option<u64>,
}

impl AnimObject {
    pub fn remap(&self) -> Option<AnimRemap> {
        self.remap
    }

    pub(crate) fn completed(&self) -> bool {
        self.completed
    }

    pub fn is_building_anim(&self) -> bool {
        self.building_anim
    }

    pub(crate) fn mark_building_anim(&mut self) {
        self.building_anim = true;
    }

    #[cfg(test)]
    pub(crate) fn install_building_expiry_fixture(&mut self, marker: bool, completed: bool) {
        self.building_anim = marker;
        self.completed = completed;
    }

    /// Minimal unattached object for tests of storage, lifecycle and checksum
    /// consumers. Runtime producers use the animation constructor below.
    #[cfg(test)]
    pub(crate) fn new_unattached_for_test(
        stable_id: AnimId,
        native_unique_id: i32,
        type_id: InternedId,
        world_coord: AnimWorldCoord,
        runtime: AnimRuntime,
    ) -> Self {
        Self {
            stable_id,
            native_unique_id,
            type_id,
            world_coord,
            draw_flags: 0,
            z_adjust: 0,
            remap: None,
            effective_end: 1,
            effective_loop_end: 1,
            runtime,
            draw_runtime: AnimDrawRuntime::default(),
            use_cell_drawer: false,
            terrain_attached: false,
            in_logic_vector: false,
            owner_entity: None,
            building_slot: None,
            building_anim: false,
            damage_fire_slot: None,
            completed: false,
            start_sound_active: false,
            stop_sound_id: None,
            display: AnimDisplayState::default(),
            bounce: None,
            owner_house: None,
            attached_bullet: None,
        }
    }

    /// The owning House (`AnimClass+0x180`).
    pub fn owner_house(&self) -> Option<InternedId> {
        self.owner_house
    }

    /// The bullet whose expiry ends this anim (`AnimClass+0x17C`).
    #[cfg(test)]
    pub(crate) fn attached_bullet(&self) -> Option<u64> {
        self.attached_bullet
    }

    /// Preserve the former field order; the new damage-fire reverse index is
    /// derived from already-hashed Building slots and never enters this fold.
    pub(crate) fn hash_before_display(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        self.stable_id.hash(hasher);
        self.native_unique_id.hash(hasher);
        self.type_id.hash(hasher);
        self.world_coord.hash(hasher);
        self.draw_flags.hash(hasher);
        self.z_adjust.hash(hasher);
        self.remap.hash(hasher);
        self.effective_end.hash(hasher);
        self.effective_loop_end.hash(hasher);
        self.runtime.hash(hasher);
        self.draw_runtime.hash(hasher);
        self.use_cell_drawer.hash(hasher);
        self.terrain_attached.hash(hasher);
        self.in_logic_vector.hash(hasher);
        self.owner_entity.hash(hasher);
        self.building_slot.hash(hasher);
        self.start_sound_active.hash(hasher);
        self.stop_sound_id.hash(hasher);
        self.completed.hash(hasher);
        self.building_anim.hash(hasher);
        // Unset links add no bytes, preserving earlier streams.
        if self.owner_house.is_some() || self.attached_bullet.is_some() {
            b"anim-owner-house-bullet-v1".hash(hasher);
            self.owner_house.hash(hasher);
            self.attached_bullet.hash(hasher);
        }
    }
}

impl std::hash::Hash for AnimObject {
    fn hash<H: std::hash::Hasher>(&self, hasher: &mut H) {
        self.hash_before_display(hasher);
        self.display.hash(hasher);
    }
}

/// The owner centre (its GetCoords) an attached anim's stored coordinate is
/// relative to.
fn anim_owner_world_coords(
    owner: &crate::sim::game_entity::GameEntity,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> AnimWorldCoord {
    let centre = crate::sim::movement::ground_pose::object_get_coords(owner, terrain);
    AnimWorldCoord {
        x: centre.x,
        y: centre.y,
        z: centre.z,
    }
}

/// Anim GetYSort (422BC0 -> Object5F6BD0 -> GetCoords422BE0, plus the
/// retained instance+104): the absolute X + Y plus the adjust. An attached
/// anim stores an owner-relative coordinate, so its key is
/// [`anim_own_sort_term`] plus [`anim_owner_sort_term`] (wrapping, so the
/// terms add in any order).
pub(crate) fn anim_display_sort_key(
    anim: &AnimObject,
    entities: &crate::sim::entity_store::EntityStore,
) -> i32 {
    let owner = anim.owner_entity.and_then(|id| entities.get(id));
    anim_own_sort_term(anim).wrapping_add(owner.map_or(0, anim_owner_sort_term))
}

/// The anim's own part of its GetYSort: the stored coordinate's X + Y plus
/// the retained adjust. Type changes do not recopy the adjust.
pub(crate) fn anim_own_sort_term(anim: &AnimObject) -> i32 {
    anim.world_coord
        .x
        .wrapping_add(anim.world_coord.y)
        .wrapping_add(anim.display.y_sort_adjust())
}

/// What an attached anim's GetYSort adds for its owner: the X + Y of the
/// owner's centre ([`anim_owner_world_coords`], whose Z the key never reads).
pub(crate) fn anim_owner_sort_term(owner: &crate::sim::game_entity::GameEntity) -> i32 {
    let [x, y] = crate::sim::movement::ground_pose::object_center_xy(owner);
    x.wrapping_add(y)
}

pub(crate) fn anim_world_coords(
    anim: &AnimObject,
    entities: &crate::sim::entity_store::EntityStore,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> AnimWorldCoord {
    let Some(owner) = anim.owner_entity.and_then(|id| entities.get(id)) else {
        return anim.world_coord;
    };
    let owner = anim_owner_world_coords(owner, terrain);
    AnimWorldCoord {
        x: anim.world_coord.x.wrapping_add(owner.x),
        y: anim.world_coord.y.wrapping_add(owner.y),
        z: anim.world_coord.z.wrapping_add(owner.z),
    }
}

/// Animation objects by id. Every `&mut AnimObject` goes through the store,
/// which notes it in a touch log for the kept Ground display sort keys.
pub struct AnimStore {
    anims: BTreeMap<AnimId, AnimObject>,
    /// Which anims may have changed since [`Self::take_touched`]. Transient:
    /// never saved, compared or hashed.
    touched: TouchLog,
    /// Derived `(bullet, anim)` pairs of the anims whose `+0x17C` names a
    /// bullet, so a bullet's expiry finds them without a scan. Written with
    /// that field, rebuilt from the anims on load and clone; never saved,
    /// compared or hashed.
    bullet_links: std::collections::BTreeSet<(u64, AnimId)>,
}

impl AnimStore {
    pub fn get(&self, id: AnimId) -> Option<&AnimObject> {
        self.anims.get(&id)
    }

    pub(crate) fn get_mut(&mut self, id: AnimId) -> Option<&mut AnimObject> {
        let stored = self.anims.len();
        let anim = self.anims.get_mut(&id)?;
        self.touched.note(id, stored);
        Some(anim)
    }

    pub(crate) fn insert(&mut self, object: AnimObject) -> Option<AnimObject> {
        self.touched.note(object.stable_id, self.anims.len());
        if let Some(bullet) = object.attached_bullet {
            self.bullet_links.insert((bullet, object.stable_id));
        }
        let replaced = self.anims.insert(object.stable_id, object);
        self.unlink_bullet(replaced.as_ref());
        replaced
    }

    pub(crate) fn remove(&mut self, id: AnimId) -> Option<AnimObject> {
        let removed = self.anims.remove(&id);
        if removed.is_some() {
            self.touched.note(id, self.anims.len());
        }
        self.unlink_bullet(removed.as_ref());
        removed
    }

    /// Drop the index pair of an anim that left the store or was replaced.
    fn unlink_bullet(&mut self, gone: Option<&AnimObject>) {
        if let Some(anim) = gone
            && let Some(bullet) = anim.attached_bullet
            && self
                .anims
                .get(&anim.stable_id)
                .map(|kept| kept.attached_bullet)
                != Some(Some(bullet))
        {
            self.bullet_links.remove(&(bullet, anim.stable_id));
        }
    }

    /// Store `+0x17C` for `id` and keep the index with it.
    fn set_attached_bullet(&mut self, id: AnimId, bullet: Option<u64>) -> bool {
        let Some(anim) = self.get_mut(id) else {
            return false;
        };
        let previous = std::mem::replace(&mut anim.attached_bullet, bullet);
        if let Some(previous) = previous {
            self.bullet_links.remove(&(previous, id));
        }
        if let Some(bullet) = bullet {
            self.bullet_links.insert((bullet, id));
        }
        true
    }

    /// The anims whose `+0x17C` names `bullet`, in id order.
    fn attached_to(&self, bullet: u64) -> Vec<AnimId> {
        self.bullet_links
            .range((bullet, AnimId::MIN)..=(bullet, AnimId::MAX))
            .map(|&(_, id)| id)
            .collect()
    }

    fn with_links(anims: BTreeMap<AnimId, AnimObject>) -> Self {
        let bullet_links = anims
            .values()
            .filter_map(|anim| Some((anim.attached_bullet?, anim.stable_id)))
            .collect();
        Self {
            anims,
            touched: TouchLog::everything(),
            bullet_links,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&AnimId, &AnimObject)> {
        self.anims.iter()
    }

    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut AnimObject> {
        self.touched.note_all();
        self.anims.values_mut()
    }

    pub fn contains_key(&self, id: AnimId) -> bool {
        self.anims.contains_key(&id)
    }

    pub(crate) fn len(&self) -> usize {
        self.anims.len()
    }

    pub(crate) fn key_at(&self, index: usize) -> Option<AnimId> {
        self.anims.keys().nth(index).copied()
    }

    /// Take the touch log, leaving it empty. One reader: the kept Ground
    /// display sort keys.
    pub(crate) fn take_touched(&mut self) -> Touched {
        self.touched.take()
    }
}

impl Default for AnimStore {
    fn default() -> Self {
        Self::with_links(BTreeMap::new())
    }
}

impl Clone for AnimStore {
    /// A clone starts with an everything-touched log: whatever was derived
    /// from the original says nothing certain about the copy's future.
    fn clone(&self) -> Self {
        Self::with_links(self.anims.clone())
    }
}

impl PartialEq for AnimStore {
    fn eq(&self, other: &Self) -> bool {
        self.anims == other.anims
    }
}

impl Eq for AnimStore {}

impl std::fmt::Debug for AnimStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("AnimStore").field(&self.anims).finish()
    }
}

/// The saved form is the former newtype `AnimStore(map)`; the log is not saved.
impl Serialize for AnimStore {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_newtype_struct("AnimStore", &self.anims)
    }
}

impl<'de> Deserialize<'de> for AnimStore {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename = "AnimStore")]
        struct Saved(BTreeMap<AnimId, AnimObject>);
        let Saved(anims) = Saved::deserialize(deserializer)?;
        Ok(Self::with_links(anims))
    }
}

#[derive(Debug, Error)]
pub enum AnimSpawnError {
    #[error("animation type id {0} does not resolve to bound runtime metadata")]
    MissingType(InternedId),
    #[error("animation type [{0}] has no bound SHP frame count")]
    UnboundType(String),
    #[error("animation stable id {0} collided with an existing object")]
    DuplicateId(AnimId),
    #[error("bouncing animation [{0}] launched outside the verified x87 domain: {1}")]
    LaunchOutOfDomain(String, NativeX87Error),
}

enum VisitAction {
    None,
    Destroy,
    MakeInfantry,
    Next(String),
}

/// The facing a `MakeInfantry=` anim's infantryman is Unlimbo'd with
/// (`PUSH 0x60`, `0x00424A23`).
const MAKE_INFANTRY_UNLIMBO_FACING: u8 = 0x60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnimOccupationOperation {
    Mark,
    Clear,
}

/// One MakeInfantry anim's Mark or Clear on its cell's raw planes. At or above
/// the floor plus the deck (Anim `[0x0089A1B4]`), Mark takes the deck only
/// while the cell keeps its structural bridge flag. Both write the anim's owner
/// index, -1 (vtable `+0x38` = `0x00410490`, stored at `0x004262D0` /
/// `0x004262F5`); Clear resets it once `0x1C` is clear (`0x0042634F` /
/// `0x00426378`).
fn apply_anim_raw_occupation(
    grid: &mut RawCellOccupationGrid,
    key: RawCellKey,
    mask: u8,
    world_z: i32,
    ground_z: i32,
    live_structural_bridge: bool,
    operation: AnimOccupationOperation,
) {
    let reaches_deck = world_z >= ground_z.wrapping_add(BRIDGE_DECK_HEIGHT_LEPTONS);
    let use_deck = match operation {
        AnimOccupationOperation::Mark => reaches_deck && live_structural_bridge,
        // AnimClass::ClearCellOccupancy deliberately ignores Cell+0x140 bit
        // 0x100. This can leave a ground bit stale when a high animation was
        // marked after structural bridge state disappeared.
        AnimOccupationOperation::Clear => reaches_deck,
    };
    let layer = if use_deck {
        MovementLayer::Bridge
    } else {
        MovementLayer::Ground
    };
    let put = operation == AnimOccupationOperation::Mark;
    grid.write_occupant(key, layer, mask, None, put);
}

impl Simulation {
    pub fn anim(&self, id: AnimId) -> Option<&AnimObject> {
        self.substrate.anims.get(id)
    }

    pub fn anims(&self) -> impl Iterator<Item = (&AnimId, &AnimObject)> {
        self.substrate.anims.iter()
    }

    /// An anim's stage (`AnimClass+0xAC`) and its image's frame count (the
    /// type's GetImage, vt+0x9C, SHP header `+0x6`; 0 for an image that never
    /// bound), or `None` once the anim has left the store. The superweapons
    /// pace themselves by it: the Psychic Dominator's followed anim
    /// (`PsyDom::Process @ 0x0053AF40`) and the Lightning Storm's clouds
    /// (`LightningStorm::Process @ 0x0053A6C0`).
    pub(crate) fn anim_stage_and_frames(&self, id: AnimId, rules: &RuleSet) -> Option<(i32, i32)> {
        let anim = self.anim(id)?;
        let frames = rules
            .art()
            .anim_runtime_config(self.interner.resolve(anim.type_id))
            .and_then(|config| config.raw_shp_frame_count)
            .unwrap_or(0);
        Some((anim.runtime.current_frame, frames))
    }

    fn anim_mut_by_id(&mut self, id: AnimId) -> Option<&mut AnimObject> {
        self.substrate.anims.get_mut(id)
    }

    /// Map[coord] (`0x00565730`) for an anim's world XY, which truncates
    /// toward zero and wraps by row: the real cell, or `None` for the shared
    /// dummy cell off the map. Without resolved terrain, the truncated cell.
    fn anim_map_cell(&self, x: i32, y: i32) -> Option<(u16, u16)> {
        use crate::map::cell_index::NativeCellIdentity;
        use crate::util::lepton::lepton_to_cell_packed;

        let (rx, ry) = match self.resolved_terrain.as_ref() {
            Some(terrain) => {
                let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
                let cell = cells.lookup_world(x, y);
                let NativeCellIdentity::Real(_) = cell else {
                    return None;
                };
                cells.coord(cell)
            }
            None => (lepton_to_cell_packed(x), lepton_to_cell_packed(y)),
        };
        Some((u16::try_from(rx).ok()?, u16::try_from(ry).ok()?))
    }

    /// `AnimClass::MarkCellOccupancy` (vt+0xF0 = `0x00426270`) at the anim's
    /// stored Location, called directly: InfantryClass::ReceiveDamage's
    /// InfDeath 9 arm marks its new InfantryMutate anim (`0x00518B26`),
    /// whatever the anim's `MakeInfantry=`.
    pub(crate) fn anim_mark_cell_occupancy(&mut self, id: AnimId) {
        if let Some(location) = self.anim(id).map(|anim| anim.world_coord) {
            self.apply_make_infantry_raw_occupation(location, AnimOccupationOperation::Mark);
        }
    }

    /// `AnimClass::MarkCellOccupancy` (`0x00426270`) and `ClearCellOccupancy`
    /// (`0x00426300`) for a MakeInfantry anim at its raw Location: the sub-cell
    /// (`0x004810A0`), the cell through Map[coord] (`0x00565730`), whose miss
    /// is the shared dummy cell, and the floor there (`0x00578080`).
    fn apply_make_infantry_raw_occupation(
        &mut self,
        location: AnimWorldCoord,
        operation: AnimOccupationOperation,
    ) {
        let mask = infantry_raw_occupation_mask(
            SimFixed::from_num(location.x & 0xff),
            SimFixed::from_num(location.y & 0xff),
        );
        let (key, ground_z, live_structural_bridge) = match self.resolved_terrain.as_ref() {
            Some(terrain) => {
                let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
                let cell = cells.lookup_world(location.x, location.y);
                let point = crate::sim::components::DriveCoord {
                    x: location.x,
                    y: location.y,
                    z: location.z,
                };
                let ground_z =
                    crate::sim::movement::ground_pose::query_ground_height(&cells, point)
                        .unwrap_or(0);
                // The mark reads live Cell+140 bit100. A stamped side cell needs
                // no own overlay (constructor5FC380, bridge_constructor.json
                // cases8..11).
                let live_structural_bridge = cells.flags(cell) & 0x100 != 0;
                (
                    RawCellKey::from_native(terrain, cell),
                    ground_z,
                    live_structural_bridge,
                )
            }
            None => {
                let Some((rx, ry)) = self.anim_map_cell(location.x, location.y) else {
                    return;
                };
                (RawCellKey::Real(rx, ry), 0, false)
            }
        };
        apply_anim_raw_occupation(
            &mut self.substrate.raw_cell_occupation,
            key,
            mask,
            location.z,
            ground_z,
            live_structural_bridge,
            operation,
        );
    }

    /// Construct the `AnimClass` a producer described by cell, sub-cell and
    /// height level.
    #[cfg(test)]
    pub(crate) fn spawn_anim_object(
        &mut self,
        rules: &RuleSet,
        descriptor: AnimClassSpawnDescriptor,
    ) -> Result<AnimId, AnimSpawnError> {
        let world_coord = AnimWorldCoord::from_cell_sub_z(
            descriptor.rx,
            descriptor.ry,
            descriptor.sub_x,
            descriptor.sub_y,
            descriptor.z,
        );
        self.spawn_anim_at_world(rules, descriptor, world_coord)
    }

    /// Construct one combat explosion as a real `AnimClass` instance.
    ///
    /// gamemd-derived: the warhead's `AnimList=` pick reaches
    /// `AnimClass::AnimClass @ 0x00421EA0` from `BulletClass::DetonateAtCoord`
    /// at `0x00469C93` (and identically at `0x0046A2F6`) with the argument row
    /// `(type, &impactCoord, delay 0, loopCount 1, drawFlags 0x2600,
    /// zAdjust -15, reverse 0)`. Read as raw pushes over
    /// `0x00469C40..0x00469CA0`: `PUSH 0x0` (reverse) at `0x00469C65`, then the
    /// by-value `CoordStruct` the `RET 0xC` helper at `0x0048ACE0` consumes,
    /// then `PUSH EAX` (-15), `PUSH 0x2600`, `PUSH 0x1`, `PUSH 0x0`,
    /// `PUSH ECX` (coord), `PUSH EBX` (type).
    ///
    /// Because `delay` is zero, `AnimClass::AnimClass` calls `AnimClass::Start
    /// @ 0x00424CE0` before returning — that is where `Report=`/`StartSound=`
    /// is played, from the single `AnimTypeClass+0x2F8` slot. So an explosion's
    /// sound is a constructor-time effect, not a first-tick one.
    ///
    /// Returns `None` when the art type never bound (no art section or no SHP,
    /// see `ArtRegistry::bind_anim_class_assets`); native mints a
    /// default `AnimTypeClass` in that case whose `End` stays 0, so the anim
    /// retains its first-AI guard, expires on a later visit, and draws nothing.
    ///
    /// The anim is constructed at the producer's exact impact Z (`world_z`);
    /// `z` is only the level byte of the descriptor.
    pub(crate) fn spawn_combat_explosion_anim(
        &mut self,
        rules: &RuleSet,
        type_name: InternedId,
        rx: u16,
        ry: u16,
        sub_x: crate::util::fixed_math::SimFixed,
        sub_y: crate::util::fixed_math::SimFixed,
        z: u8,
        world_z: i32,
    ) -> Option<AnimId> {
        let descriptor = AnimClassSpawnDescriptor {
            delay: 0,
            loop_count: 1,
            draw_flags: COMBAT_EXPLOSION_DRAW_FLAGS,
            z_adjust: COMBAT_EXPLOSION_Z_ADJUST,
            reverse: false,
            ..AnimClassSpawnDescriptor::new(type_name, rx, ry, sub_x, sub_y, z)
        };
        let world_coord = AnimWorldCoord {
            z: world_z,
            ..AnimWorldCoord::from_cell_sub_z(rx, ry, sub_x, sub_y, z)
        };
        match self.spawn_anim_at_world(rules, descriptor, world_coord) {
            Ok(id) => Some(id),
            Err(error) => {
                log::debug!(
                    "combat explosion [{}] did not construct: {error}",
                    self.interner.resolve(type_name)
                );
                None
            }
        }
    }

    pub(crate) fn spawn_anim_at_world(
        &mut self,
        rules: &RuleSet,
        descriptor: AnimClassSpawnDescriptor,
        world_coord: AnimWorldCoord,
    ) -> Result<AnimId, AnimSpawnError> {
        self.spawn_anim_at_world_with_constructor(
            rules,
            descriptor,
            world_coord,
            AnimConstructorInput::Runtime,
        )
    }

    /// One port of Anim421EA0 for runtime and authored-load producers. An
    /// assigned identity skips only42203D; preconsumed death-debris input also
    /// retains draws already taken at that producer's native boundary.
    pub(crate) fn spawn_anim_at_world_with_constructor(
        &mut self,
        rules: &RuleSet,
        descriptor: AnimClassSpawnDescriptor,
        world_coord: AnimWorldCoord,
        constructor: AnimConstructorInput,
    ) -> Result<AnimId, AnimSpawnError> {
        // Anim42203D assigns before registry insertion and RandomRate4221F5.
        // Death debris may already have constructed at its pick/draw boundary.
        let native_unique_id = match constructor {
            AnimConstructorInput::AssignedIdentity(id) => id,
            AnimConstructorInput::Preconsumed(draws) => draws
                .native_unique_id
                .unwrap_or_else(|| self.next_native_runtime_id()),
            AnimConstructorInput::Runtime => self.next_native_runtime_id(),
        };
        let type_name = self
            .interner
            .resolve(descriptor.type_name)
            .to_ascii_uppercase();
        let config = rules
            .art()
            .anim_runtime_config(&type_name)
            .cloned()
            .ok_or(AnimSpawnError::MissingType(descriptor.type_name))?;
        let (effective_end, effective_loop_end) = effective_bounds(&type_name, &config)?;
        let reverse = descriptor.reverse || config.reverse;
        let stop_sound_id = config
            .stop_sound
            .as_deref()
            .map(|sound| self.interner.intern(sound));
        let stable_id = self.allocate_stable_id();
        if self.substrate.anims.contains_key(stable_id)
            || self.substrate.entities.contains(stable_id)
        {
            return Err(AnimSpawnError::DuplicateId(stable_id));
        }
        let object = AnimObject {
            stable_id,
            native_unique_id,
            type_id: descriptor.type_name,
            world_coord,
            draw_flags: descriptor.draw_flags,
            z_adjust: descriptor.z_adjust,
            remap: None,
            effective_end,
            effective_loop_end,
            runtime: AnimRuntime {
                current_frame: if reverse {
                    effective_loop_end.wrapping_sub(1)
                } else {
                    0
                },
                frame_step: if reverse { -1 } else { 1 },
                delay_remaining: descriptor.delay,
                rate_reload: 0,
                frame_timer: CdTimer::default(),
                loop_remaining: native_loop_remaining(config.loop_count, descriptor.loop_count),
                first_ai_guard: true,
                constructor_reverse: descriptor.reverse,
                inactive: false,
                paused: false,
            },
            draw_runtime: descriptor.draw_runtime,
            use_cell_drawer: descriptor.use_cell_drawer,
            terrain_attached: descriptor.terrain_attached,
            in_logic_vector: false,
            owner_entity: None,
            building_slot: None,
            building_anim: false,
            damage_fire_slot: None,
            completed: false,
            start_sound_active: false,
            stop_sound_id,
            display: AnimDisplayState {
                marked_on_map: false,
                y_sort_adjust: config.y_sort_adjust,
            },
            bounce: None,
            owner_house: None,
            attached_bullet: None,
        };
        // The insert must run in every build profile: wrapped in
        // `debug_assert!` it was compiled out of release binaries and no
        // scheduler anim ever existed in a shipped build.
        let previous = self.substrate.anims.insert(object);
        debug_assert!(previous.is_none());
        let draws = match constructor {
            AnimConstructorInput::Preconsumed(draws) => draws,
            AnimConstructorInput::Runtime | AnimConstructorInput::AssignedIdentity(_) => {
                match anim_constructor_draws(&config, world_coord, &mut self.scenario_rng) {
                    Ok(draws) => draws,
                    Err(error) => {
                        // VERA's explicit unsupported-arithmetic failure precedes
                        // Reveal. The original constructor ID remains spent.
                        self.substrate.anims.remove(stable_id);
                        return Err(AnimSpawnError::LaunchOutOfDomain(type_name, error));
                    }
                }
            }
        };
        let rate_reload = self.anim_rate(&config, draws.random_rate);
        let frame_timer =
            CdTimer::started(self.session.binary_frame as i32, i32::from(rate_reload));
        let registered = self
            .substrate
            .anims
            .get_mut(stable_id)
            .expect("new Anim registry entry");
        registered.runtime.rate_reload = rate_reload;
        registered.runtime.frame_timer = frame_timer;
        registered.bounce = draws.bounce;
        // Native registry insertion precedes Reveal, and Reveal precedes the
        // delay-zero constructor-time Start call.
        self.reveal_anim(stable_id, Some(rules));
        if descriptor.delay == 0 {
            self.anim_start(stable_id, &config, rules, None);
        }
        Ok(stable_id)
    }

    /// Exact final-Init scalar deletion selector for first-sweep tile Anims.
    /// Removal rechecks the compacted registry slot and never enters the
    /// ordinary Destroy/UnInit pending-delete path.
    pub(crate) fn scalar_delete_load_terrain_anims(&mut self) -> usize {
        let mut index = 0;
        let mut removed = 0;
        while let Some(id) = self.substrate.anims.key_at(index) {
            let terrain_attached = self
                .substrate
                .anims
                .get(id)
                .is_some_and(|anim| anim.terrain_attached);
            if !terrain_attached {
                index += 1;
                continue;
            }
            let start_sound_active = self
                .substrate
                .anims
                .get(id)
                .is_some_and(|anim| anim.start_sound_active);
            self.clear_damage_fire_anim_reference(id);
            self.release_anim_owner_reference(id);
            if start_sound_active {
                // `MapClass::InitCellAttributes @ 0x00568BB0` reaches the
                // scalar destructor4228E0: Release406060, no StopSound.
                self.sound_events
                    .push(SimSoundEvent::ObjectSoundReleased { owner: id });
            }
            self.conceal_anim(id);
            self.substrate.pending_delete.retain(|queued| *queued != id);
            let removed_anim = self.substrate.anims.remove(id);
            debug_assert!(removed_anim.is_some());
            removed += 1;
        }
        removed
    }

    /// `CellClass::Get_Tiberium_Value @ 0x00485020` of the anim's cell. Native
    /// finds it from the raw Location through Object vtable `+0x1BC`
    /// (`0x005F6960`): two Map[coord] lookups (`0x00565730`), whose miss is the
    /// shared dummy cell.
    fn anim_cell_tiberium_value(
        &self,
        location: AnimWorldCoord,
        rules: &RuleSet,
        overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    ) -> i32 {
        let grid_fields = |(rx, ry): (u16, u16)| {
            self.overlay_grid
                .as_ref()
                .filter(|grid| rx < grid.width() && ry < grid.height())
                .map(|grid| {
                    let cell = grid.cell(rx, ry);
                    (cell.overlay_id, cell.overlay_data)
                })
        };
        let (overlay_id, overlay_data) = match self.resolved_terrain.as_ref() {
            Some(terrain) => {
                let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
                let point = crate::sim::components::DriveCoord {
                    x: location.x,
                    y: location.y,
                    z: location.z,
                };
                match crate::sim::movement::ground_pose::query_object_cell(&cells, point) {
                    cell @ crate::map::cell_index::NativeCellIdentity::Real(_) => {
                        let (rx, ry) = cells.coord(cell);
                        grid_fields((rx as u16, ry as u16)).unwrap_or((None, 0))
                    }
                    crate::map::cell_index::NativeCellIdentity::Dummy => {
                        cells.dummy().overlay_fields()
                    }
                }
            }
            None => self
                .anim_map_cell(location.x, location.y)
                .and_then(grid_fields)
                .unwrap_or_else(|| self.effective_shared_cell_dummy().overlay_fields()),
        };
        crate::map::tiberium_cell::tiberium_value(
            overlay_id,
            overlay_data,
            overlay_registry,
            &rules.tiberium_types,
        )
    }

    pub(crate) fn visit_anim(
        &mut self,
        id: AnimId,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        // HideIfNoOre and the MakeInfantry mark read the stored Location
        // (`+0x9C`, `0x00423BD6..0x00423BFD`), not GetCoords: an owner-attached
        // anim stores an owner-relative delta there.
        let Some((type_id, first_guard, inactive, location)) = self.anim(id).map(|anim| {
            (
                anim.type_id,
                anim.runtime.first_ai_guard,
                anim.runtime.inactive,
                anim.world_coord,
            )
        }) else {
            return false;
        };
        let type_name = self.interner.resolve(type_id).to_ascii_uppercase();
        let Some(config) = rules.art().anim_runtime_config(&type_name).cloned() else {
            self.destroy_anim(id, rules);
            return false;
        };

        // `AnimClass::AI @ 0x00423AC0`, before the MakeInfantry `vtable+0xF0`
        // call, the bounce-landing block, and the trailer block: with
        // `AnimType+0x359 HideIfNoOre`, `AnimClass+0x19D` is rewritten every
        // tick from its Location's cell — hidden when
        // `CellClass::Get_Tiberium_Value @ 0x00485020` is zero, visible
        // otherwise. Only drawing is suppressed; the AI keeps running.
        // Registry-less callers (fixtures) keep the current flag.
        if config.hide_if_no_ore
            && let Some(overlay_registry) = overlay_registry
        {
            let hidden = self.anim_cell_tiberium_value(location, rules, overlay_registry) == 0;
            if let Some(anim) = self.anim_mut_by_id(id) {
                anim.draw_runtime.hidden = hidden;
            }
        }

        // AnimClass::AI performs this before its first-AI, inactive, delay,
        // visibility, and frame-timer gates. Repeated visits OR the same raw
        // bit; there is deliberately no contributor count.
        if config.make_infantry != -1 {
            self.apply_make_infantry_raw_occupation(location, AnimOccupationOperation::Mark);
        }
        if inactive {
            self.destroy_anim(id, rules);
            return false;
        }

        // `AnimClass::AI 0x00423C24`: a bouncing chunk flies its body before
        // the trailer and the first-AI guard; touching down ends it.
        if self.anim(id).is_some_and(|anim| anim.bounce.is_some())
            && let Some(bridge_state_changed) =
                self.anim_bounce_step(id, &config, rules, overlay_registry)
        {
            return bridge_state_changed;
        }
        // The trailer spawns at GetCoords after the body moved the anim.
        let Some(world_coord) = self.anim_absolute_coord(id) else {
            return false;
        };

        if let Some(trailer_name) = config.trailer_anim.as_deref() {
            if trailer_cadence_matches(
                u64::from(self.session.binary_frame),
                config.trailer_seperation,
            ) && rules.art().anim_runtime_config(trailer_name).is_some()
            {
                let trailer_type = self.interner.intern(trailer_name);
                let descriptor = AnimClassSpawnDescriptor {
                    type_name: trailer_type,
                    rx: 0,
                    ry: 0,
                    sub_x: crate::util::fixed_math::SIM_ZERO,
                    sub_y: crate::util::fixed_math::SIM_ZERO,
                    z: 0,
                    delay: 1,
                    loop_count: 1,
                    draw_flags: TRAILER_DRAW_FLAGS,
                    z_adjust: 0,
                    reverse: false,
                    use_cell_drawer: false,
                    terrain_attached: false,
                    draw_runtime: AnimDrawRuntime::default(),
                };
                // RESIDUAL: this unwrap relies on the binder having bound the
                // parent's whole `TrailerAnim=` closure. The strict binder
                // guarantees it; the tolerant one ends a chain at the first
                // type with no sprite, so a tolerantly bound parent with an
                // unbindable trailer would panic here. Trigger: modded art
                // only; no retail trailer chain starts from a tolerant root.
                self.spawn_anim_at_world(rules, descriptor, world_coord)
                    .expect("validated trailer closure must remain spawnable");
            }
        }

        if first_guard {
            if let Some(anim) = self.anim_mut_by_id(id) {
                anim.runtime.first_ai_guard = false;
            }
            return false;
        }

        // `AnimClass::AI @ 0x00423AC0` delay countdown: the visit that takes it
        // to zero calls `AnimClass::Start @ 0x00424CE0` (call site `0x004243A1`)
        // and returns. The field also holds the `RandomLoopDelay=` pause, so a
        // looping anim restarts, and replays its start sound, after each pause.
        {
            let Some(anim) = self.anim_mut_by_id(id) else {
                return false;
            };
            if anim.runtime.delay_remaining > 0 {
                anim.runtime.delay_remaining -= 1;
                if anim.runtime.delay_remaining == 0 {
                    self.anim_start(id, &config, rules, overlay_registry);
                }
                return false;
            }
        }

        let mut action = VisitAction::None;
        let mut random_loop_delay = None;
        let current_frame = self.session.binary_frame as i32;
        let (middle, boundary) = {
            let Some(anim) = self.anim_mut_by_id(id) else {
                return false;
            };
            //42449B: power pause follows first-AI/delay gates and precedes timer advance.
            if anim.runtime.paused {
                return false;
            }
            if anim.runtime.rate_reload == 0 {
                return false;
            }
            if !anim.runtime.frame_timer.expired(current_frame) {
                return false;
            }
            anim.runtime
                .frame_timer
                .start(current_frame, i32::from(anim.runtime.rate_reload));
            anim.runtime.current_frame = anim
                .runtime
                .current_frame
                .wrapping_add(anim.runtime.frame_step);
            // `0x0042465D..0x00424687`: the committed stage reaching the
            // type's middle frame (`+0x298`, zero meaning Start already ran
            // Middle) calls Middle unless the anim is bouncing (`+0x194`, the
            // constructor's byte, which a `Next=` switch keeps; the meteor arm
            // is not built, so its type flag stands in), before the boundary
            // tail.
            let middle = middle_frame(&config).is_some_and(|middle| {
                config.start.wrapping_add(anim.runtime.current_frame) == middle
            }) && !(anim.bounce.is_some() || config.is_meteor);
            (middle, advance_anim_boundary(anim, &config))
        };
        if middle {
            self.anim_middle(id, &config, rules, overlay_registry);
        }
        match boundary {
            AnimBoundary::Continue | AnimBoundary::Bounce => return false,
            AnimBoundary::Loop => {
                random_loop_delay = config.random_loop_delay;
            }
            AnimBoundary::Complete => {
                action = if let Some(next) = config.next.clone() {
                    VisitAction::Next(next)
                } else if config.make_infantry != -1 {
                    VisitAction::MakeInfantry
                } else {
                    VisitAction::Destroy
                };
            }
        }

        if let Some((low, high)) = random_loop_delay {
            let delay = self
                .scenario_rng
                .next_range_u32_inclusive(u32::from(low), u32::from(high))
                as u16;
            if let Some(anim) = self.anim_mut_by_id(id) {
                anim.runtime.delay_remaining = delay;
            }
        }
        match action {
            VisitAction::None => {}
            VisitAction::Destroy => {
                if let Some(anim) = self.anim_mut_by_id(id) {
                    anim.completed = true;
                }
                self.destroy_anim(id, rules);
            }
            VisitAction::MakeInfantry => {
                if self.anim_make_infantry(id, config.make_infantry, rules, overlay_registry) {
                    if let Some(anim) = self.anim_mut_by_id(id) {
                        anim.completed = true;
                    }
                    self.destroy_anim(id, rules);
                }
            }
            VisitAction::Next(next) => self.switch_anim_type(id, &next, rules, overlay_registry),
        }
        false
    }

    /// The end of a `MakeInfantry=` anim's last loop with no `Next=`
    /// (`AnimClass::AI @ 0x00423AC0`, `0x00424932..0x00424B30`), in order:
    /// 1. ClearCellOccupancy (vt+0xF4 = `0x00426300`) at the stored Location
    ///    (`0x00424963`).
    /// 2. A `MakeInfantry=` above `[General] AnimToInfantry=`'s count
    ///    (`Rules+0xCF4`, signed `JG` at `0x00424982`) creates nothing.
    /// 3. With no owner (`+0x180`) or a defeated one (`+0x1F5`), the first
    ///    house in HouseClass::Array order whose side is `Civilian` becomes
    ///    the owner ([`Simulation::civilian_side_house`],
    ///    `0x0042499C..0x004249D8`). Without one the owner stays, and with
    ///    no owner nothing is created (`0x004249E6`).
    /// 4. `AnimToInfantry=`'s entry's CreateObject (vt+0x8C) for the owner,
    ///    then Unlimbo (vt+0xD8) at the Location facing `0x60` (`0x00424A42`).
    /// 5. A refused Unlimbo steps the stage (`+0xAC`) back one and keeps the
    ///    anim (`0x00424B23`): its next completed frame tries again with a
    ///    new object. The refused one stays in limbo, as it does natively.
    /// 6. Placed where the anim's cell (GetCoords, `0x005657A0`) carries the
    ///    bridge flag (`+0x140 & 0x100`) and the Location is above the cell's
    ///    GetCoords Z (`0x00486840`), it moves onto the deck: Mark(UP),
    ///    OnBridge (`+0x8C`), Mark(DOWN) (`0x00424AA0..0x00424AE6`).
    /// 7. Hunt (`0xF`, Queue_Mission vt+0x1E8) unless the owner's IsHuman
    ///    (`+0x1EC`, `0x00424AF2`).
    ///
    /// Returns whether the anim ends; it then completes (`+0x179`) and
    /// UnInits (`0x00424B0A..0x00424B15`).
    ///
    /// Native execution: `tools/superweapon_oracle.py` section
    /// `make_infantry`, replayed in `superweapon::genetic_converter_tests`.
    ///
    /// RESIDUAL: a `MakeInfantry=` equal to the count, or below -1, reads
    /// outside the vector (`0x00424A05`); VERA creates nothing. Retail's only
    /// `MakeInfantry=` anim is GENDEATH (0), and `AnimToInfantry=` holds
    /// BRUTE (production reader).
    pub(crate) fn anim_make_infantry(
        &mut self,
        id: AnimId,
        make_infantry: i32,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        let Some((location, owner)) = self
            .anim(id)
            .map(|anim| (anim.world_coord, anim.owner_house))
        else {
            return true;
        };
        self.apply_make_infantry_raw_occupation(location, AnimOccupationOperation::Clear);
        let kinds = &rules.general.anim_to_infantry;
        if i64::from(make_infantry) > kinds.len() as i64 {
            return true;
        }
        let Some(kind) = usize::try_from(make_infantry)
            .ok()
            .and_then(|index| kinds.get(index))
        else {
            return true;
        };
        let defeated = |sim: &Self, house: InternedId| {
            sim.houses
                .get(&house)
                .is_some_and(|state| state.is_defeated)
        };
        let owner = if owner.is_none_or(|house| defeated(self, house)) {
            match self.civilian_side_house(rules) {
                Some(civilian) => {
                    self.set_anim_owner_house(id, civilian);
                    Some(civilian)
                }
                None => owner,
            }
        } else {
            owner
        };
        let Some(owner) = owner else {
            return true;
        };
        let owner_name = self.interner.resolve(owner).to_string();
        let (rx, ry, _, _, _) = location.to_cell_sub_z();
        let Some(infantry) =
            self.construct_object_limbo_at_height(kind, &owner_name, rx, ry, 0, 0, rules)
        else {
            log::debug!("MakeInfantry anim {id}: [{kind}] did not construct");
            return true;
        };
        let placed = self.reveal_constructed_object_at_coord_with_overlay_context(
            infantry,
            crate::sim::components::DriveCoord {
                x: location.x,
                y: location.y,
                z: location.z,
            },
            MAKE_INFANTRY_UNLIMBO_FACING,
            crate::sim::world::PlacementEvidence::EvaluateMark,
            rules,
            overlay_registry,
        );
        if placed.is_none() {
            if let Some(anim) = self.anim_mut_by_id(id) {
                anim.runtime.current_frame = anim.runtime.current_frame.wrapping_sub(1);
            }
            return false;
        }
        let at = self.anim_absolute_coord(id).unwrap_or(location);
        let lift = self.resolved_terrain.as_ref().is_some_and(|terrain| {
            let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
            let cell = cells.lookup_world(at.x, at.y);
            let crate::map::cell_index::NativeCellIdentity::Real(_) = cell else {
                return false;
            };
            let (cell_x, cell_y) = cells.coord(cell);
            cells.flags(cell) & 0x100 != 0
                && location.z
                    > crate::sim::projectile::cell_ground_coord(
                        Some(terrain),
                        cell_x as u16,
                        cell_y as u16,
                    )
                    .z
        });
        if lift {
            self.foot_mark_remove(infantry, Some(rules), overlay_registry);
            if let Some(entity) = self.substrate.entities.get_mut(infantry) {
                entity.on_bridge = true;
            }
            self.foot_mark_put(infantry, Some(rules), overlay_registry);
        }
        let human = self.houses.get(&owner).is_some_and(|house| house.is_human);
        if !human && let Some(entity) = self.substrate.entities.get_mut(infantry) {
            crate::sim::mission::authority::queue_entity_mission_deferred(
                entity,
                crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Hunt),
            );
        }
        true
    }

    pub(crate) fn destroy_anim(&mut self, id: AnimId, rules: &RuleSet) {
        self.destroy_anim_with_context(id, Some(rules));
    }

    pub(crate) fn destroy_anim_with_context(&mut self, id: AnimId, rules: Option<&RuleSet>) {
        if self.substrate.pending_delete.contains(&id) {
            return;
        }
        // `AnimClass::GetCoords @ 0x00422BE0` — the sound plays at the anim's
        // resolved world position, not its owner-relative stored delta.
        let Some(world) = self.anim_absolute_coord(id) else {
            return;
        };
        let Some(stop_sound) = self.anim(id).map(|anim| anim.stop_sound_id) else {
            return;
        };
        if self
            .anim(id)
            .is_some_and(|anim| anim.owner_entity.is_some())
        {
            if let Some(rules) = rules {
                self.detach_anim_from_owner(id, rules);
            } else {
                // An imminent physical owner retirement may have no Rules.
                // Preserve SetOwner(NULL)'s resolved coordinates and pointer
                // callback here. Its temporary Display resubmission is omitted:
                // this same operation conceals the Anim before another draw.
                self.release_anim_owner_reference(id);
                self.anim_mut_by_id(id)
                    .expect("the retiring animation still exists")
                    .world_coord = world;
            }
        }
        if let Some(anim) = self.anim_mut_by_id(id) {
            anim.start_sound_active = false;
        }
        // Original4255B0..425628 never writes19B; ObjectUnInit5F6625 clears
        // ObjectAlive after expiry/Limbo, then queues this physical object.
        // Executed MGUN/PIFF retirement receipts retain inactive0 while
        // removing Logic/Display and adding deferred deletion:
        // tools/spatial_oracle/anytown_damage/foot_missions.json,
        // ground_emission_receipt.
        // UnInit4255D5 calls Release406060, leaving a one-shot Report
        // playing; StopAndClear405D40 would cut it off. Original execution:
        // tools/rules_oracle/bridge_child_sound.{py,json,md}.
        self.sound_events
            .push(SimSoundEvent::ObjectSoundReleased { owner: id });
        if let Some(stop_sound_id) = stop_sound {
            // Native425618 plays StopSound after releasing the Report. The
            // preceding release removes this owner's handle, so the existing
            // stop/play consumer cannot interrupt the released Report.
            self.sound_events.push(SimSoundEvent::AnimationStopped {
                anim_id: id,
                stop_sound_id: Some(stop_sound_id),
                world,
            });
        }
        // Object::UnInit5F6616 broadcasts the Anim's expiry before Limbo;
        // Building44EA1A..44EA4F clears its matching damage-fire slot or
        // runs the building-slot transition before this Anim leaves Logic.
        self.clear_damage_fire_anim_reference(id);
        self.building_anim_pointer_expired(id, rules);
        // Limbo5F4D30→DetachAll5F4D61 announces again. The first callback
        // normally removed the reverse index; preserve the second visit for
        // native dead-building gates that retained the matched slot. Both
        // notifications precede Display5F4D79 and Logic5F4DD3 removal.
        self.building_anim_pointer_expired(id, rules);
        self.conceal_anim(id);
        self.substrate.pending_delete.push(id);
    }

    /// Building451A2C and ClearAnimSlot451E40 use scalar deletion: the old
    /// object is gone before the replacement pointer is installed. Keep this
    /// separate from an animation's ordinary deferred Destroy operation.
    pub(crate) fn scalar_delete_building_anim(&mut self, id: AnimId) {
        // Anim VT7E3354+20 ->426590 ->4228E0 releases sound handles but
        // never reaches UnInit4255B0 or its StopSound playback. The slot was
        // cleared by the caller before these synchronous destructor effects.
        let sound_active = self.anim(id).is_some_and(|anim| anim.start_sound_active);
        self.clear_damage_fire_anim_reference(id);
        self.release_anim_owner_reference(id);
        self.clear_building_anim_reference(id);
        if sound_active {
            self.sound_events
                .push(SimSoundEvent::ObjectSoundReleased { owner: id });
        }
        self.conceal_anim(id);
        self.substrate.pending_delete.retain(|queued| *queued != id);
        self.substrate.anims.remove(id);
    }

    pub(crate) fn clear_damage_fire_anim_reference(&mut self, id: AnimId) {
        let link = self
            .anim_mut_by_id(id)
            .and_then(|anim| anim.damage_fire_slot.take());
        if let Some((owner, slot)) = link
            && let Some(entity) = self.substrate.entities.get_mut(owner)
            && entity.damage_fire_anim_ids[usize::from(slot)] == Some(id)
        {
            entity.damage_fire_anim_ids[usize::from(slot)] = None;
        }
    }

    /// AnimClass::SetOwnerObject424B50. Normal detach preserves absolute
    /// coordinates and gates Remove/Submit on the entry Object+74 mark. Attach
    /// always removes, stores owner-relative coordinates, then submits Ground.
    /// The owner+17C callback is a bare RET for all currently supported Techno
    /// owners; Object+84's shared-owner flag has no consumer here.
    pub(crate) fn set_anim_owner_object(
        &mut self,
        id: AnimId,
        new_owner: Option<u64>,
        rules: &RuleSet,
    ) -> bool {
        let Some(anim) = self.anim(id) else {
            return false;
        };
        let marked = anim.display.marked_on_map;
        if anim.owner_entity.is_some() {
            if marked {
                self.substrate.display.remove(id);
            }
            let absolute = self.anim_absolute_coord(id).expect("live Anim");
            let anim = self.anim_mut_by_id(id).expect("live Anim");
            anim.owner_entity = None;
            anim.world_coord = absolute;
            if marked {
                self.submit_anim_display(id, Some(rules));
            }
        }
        if let Some(owner_id) = new_owner {
            let Some(owner_coord) = self.anim_owner_coords(owner_id) else {
                return false;
            };
            self.substrate.display.remove(id);
            let anim = self.anim_mut_by_id(id).expect("live Anim");
            anim.world_coord = AnimWorldCoord {
                x: anim.world_coord.x.wrapping_sub(owner_coord.x),
                y: anim.world_coord.y.wrapping_sub(owner_coord.y),
                z: anim.world_coord.z.wrapping_sub(owner_coord.z),
            };
            anim.owner_entity = Some(owner_id);
            self.submit_anim_display(id, Some(rules));
        }
        true
    }

    /// Anim GetLayer424CB0: attached -> Ground; missing type -> Air; otherwise
    /// the current type's layer.
    pub(crate) fn submit_anim_display(&mut self, id: AnimId, rules: Option<&RuleSet>) {
        if let Some(layer) = self.anim_display_layer(id, rules) {
            self.submit_object_display(id, layer, rules);
        } else {
            self.substrate.display.remove(id);
        }
    }

    pub(crate) fn anim_display_layer(
        &self,
        id: AnimId,
        rules: Option<&RuleSet>,
    ) -> Option<crate::sim::world::display_layers::DisplayLayer> {
        use crate::rules::art_data::AnimLayer;
        use crate::sim::world::display_layers::DisplayLayer;
        let anim = self.substrate.anims.get(id)?;
        if anim.owner_entity.is_some() {
            return Some(DisplayLayer::GROUND);
        }
        let config = rules.and_then(|rules| {
            rules
                .art()
                .anim_runtime_config(self.interner.resolve(anim.type_id))
        });
        match config.map(|config| config.layer) {
            None => Some(DisplayLayer::AIR),
            Some(AnimLayer::Ground) => Some(DisplayLayer::GROUND),
            Some(AnimLayer::Top) => Some(DisplayLayer::TOP),
            Some(AnimLayer::Other(index)) => {
                u8::try_from(index).ok().and_then(DisplayLayer::from_index)
            }
        }
    }

    pub(crate) fn mark_anim_display(&mut self, id: AnimId, marked: bool) {
        if let Some(anim) = self.anim_mut_by_id(id) {
            anim.display.marked_on_map = marked;
        }
    }

    /// gamemd-derived: `AnimClass::GetCoords @ 0x00422BE0` — with an owner at
    /// `Anim+0xCC` it returns `stored + owner->GetCoords()`, otherwise the
    /// stored coordinate unchanged. This is the only correct way to read an
    /// anim's world position: [`AnimObject::world_coord`] is owner-relative
    /// whenever `owner_entity` is set, exactly as native stores it.
    ///
    /// Returns `None` only when the anim does not exist.
    ///
    /// VERA-internal, gamemd equivalent UNREACHABLE: an anim naming an owner
    /// the store no longer holds is treated as unattached, so the stored
    /// coordinate is returned as-is. Native cannot produce that state —
    /// `AnimClass::Detach @ 0x00425150` runs from the owner's own uninit, via
    /// `ObjectClass::Detach_From_All_Lists`, before the pointer can dangle, and
    /// this engine mirrors it in `expire_anim_owner_reference`. The fallback
    /// exists so every caller agrees on one behaviour instead of one panicking
    /// and another silently dropping the anim; treating a dangling owner as no
    /// owner is also the only reading under which the stored coordinate means
    /// anything.
    pub fn anim_absolute_coord(&self, id: AnimId) -> Option<AnimWorldCoord> {
        Some(anim_world_coords(
            self.anim(id)?,
            &self.substrate.entities,
            self.resolved_terrain.as_ref(),
        ))
    }

    pub(crate) fn anim_owner_coords(&self, owner_id: u64) -> Option<AnimWorldCoord> {
        Some(anim_owner_world_coords(
            self.substrate.entities.get(owner_id)?,
            self.resolved_terrain.as_ref(),
        ))
    }

    /// Anim Destroy's owner callback (Techno710410 -> Object5F6DA0) clears
    /// other owner references, not Building damage-fire slots. Those clear
    /// only when this Anim broadcasts its own pointer expiry (44EA45).
    pub(crate) fn detach_anim_from_owner(&mut self, id: AnimId, rules: &RuleSet) -> Option<u64> {
        let owner = self.anim(id)?.owner_entity?;
        if let Some(entity) = self.substrate.entities.get_mut(owner) {
            // TechnoClass::Detach710443 clears the matching Techno+0x130.
            entity.expire_deploy_anim(id);
        }
        self.set_anim_owner_object(id, None, rules);
        Some(owner)
    }

    /// Destruction/expiry clear the reference without SetOwner's coordinate
    /// conversion or intermediate display submission (422961 / 425190).
    pub(crate) fn release_anim_owner_reference(&mut self, id: AnimId) -> Option<u64> {
        let owner = self.anim_mut_by_id(id)?.owner_entity.take()?;
        if let Some(entity) = self.substrate.entities.get_mut(owner) {
            entity.expire_deploy_anim(id);
        }
        Some(owner)
    }

    /// Anim PointerExpired425150 removes Display, calls owner+60, clears +CC,
    /// sets +19B and Mark(REMOVE). Stored relative coordinates remain unchanged.
    /// Normal SetOwner(NULL) would instead convert and potentially resubmit.
    /// Native comparisons: tools/spatial_oracle/display_anim_owner.json.
    ///
    /// Residual: native AI checks +19B at42435F after looping-sound, bounce and
    /// visibility work; `visit_anim` currently checks it earlier, after ore
    /// visibility/occupation.
    /// Owner expiry during combat therefore still needs that prefix audit when
    /// its missing effects land. Occupied-cell424358 and animated-tiberium424427
    /// writers of +19B remain unported. No claim of complete Anim AI parity.
    pub(crate) fn expire_anim_owner_reference(&mut self, id: AnimId, expired_id: u64) -> bool {
        if self.anim(id).and_then(|anim| anim.owner_entity) != Some(expired_id) {
            return false;
        }
        self.lifecycle_outputs
            .push(LifecycleOutput::DisplayRemove { stable_id: id });
        self.substrate.display.remove(id);
        self.release_anim_owner_reference(id);
        self.mark_anim_display(id, false);
        if let Some(anim) = self.anim_mut_by_id(id) {
            anim.runtime.inactive = true;
        }
        true
    }

    pub(crate) fn set_anim_frame_and_z_adjust(&mut self, id: AnimId, frame: i32, z_adjust: i32) {
        if let Some(anim) = self.anim_mut_by_id(id) {
            anim.runtime.current_frame = frame;
            anim.z_adjust = z_adjust;
        }
    }

    /// Apply OverlayClass's post-constructor CellAnim writes: `+0xD4` selects
    /// the Tiberium ConvertClass when present and `+0xFC` receives CellClass's
    /// current ground Z-adjust (`+0x10A`).
    pub(crate) fn set_cell_anim_draw_authority(
        &mut self,
        id: AnimId,
        remap_color: Option<HouseColorIndex>,
        z_adjust: i32,
    ) -> bool {
        let Some(anim) = self.anim_mut_by_id(id) else {
            return false;
        };
        anim.remap = remap_color.map(AnimRemap::ColorScheme);
        anim.z_adjust = z_adjust;
        true
    }

    /// A house's colour conversion as the anim's remap (`+0xD4`), frozen at
    /// the write; later disguise or owner changes must not recolor the anim.
    /// Unit deploy739C1A/739DFE store the one Techno705D70 returns
    /// (CustomPalette is not yet represented by this owner; stock SCHP uses
    /// the house conversion); InfantryClass::ReceiveDamage's InfDeath 9 arm
    /// the house's colour scheme's (`0x00518AB5..0x00518AC9`,
    /// `0x00518AE4..0x00518AF9`).
    pub(crate) fn set_anim_house_remap(&mut self, id: AnimId, house: InternedId) -> bool {
        let Some(anim) = self.anim_mut_by_id(id) else {
            return false;
        };
        anim.remap = Some(AnimRemap::House(house));
        true
    }

    /// Apply CellClass's producer-owned `AnimClass +0x100` write after the
    /// delay-zero constructor has already run `Middle`.
    pub(crate) fn set_terrain_anim_z_adjust_after_construction(
        &mut self,
        id: AnimId,
        z_adjust: i32,
    ) -> bool {
        let Some(anim) = self.anim_mut_by_id(id) else {
            return false;
        };
        if !anim.terrain_attached || anim.z_adjust != 0 {
            return false;
        }
        anim.z_adjust = z_adjust;
        true
    }

    /// A producer's plain post-construction `AnimClass +0x100` (ZAdjust)
    /// write, such as CaptureUnit's building ring (`0x00471F66`).
    pub(crate) fn set_anim_z_adjust(&mut self, id: AnimId, z_adjust: i32) -> bool {
        let Some(anim) = self.anim_mut_by_id(id) else {
            return false;
        };
        anim.z_adjust = z_adjust;
        true
    }

    /// A producer's plain Anim+19D store. Mirage's attached-ring tail
    /// `746A9C..746B19` changes drawing without suspending the Anim AI.
    pub(crate) fn set_anim_hidden(&mut self, id: AnimId, hidden: bool) -> bool {
        let Some(anim) = self.anim_mut_by_id(id) else {
            return false;
        };
        anim.draw_runtime.hidden = hidden;
        true
    }

    /// `AnimClass::SetOwnerHouse @ 0x00424CA0`: store `+0x180`.
    pub(crate) fn set_anim_owner_house(&mut self, id: AnimId, house: InternedId) -> bool {
        let Some(anim) = self.anim_mut_by_id(id) else {
            return false;
        };
        anim.owner_house = Some(house);
        true
    }

    /// `0x00424C90`: store `+0x17C`, the bullet whose expiry ends the anim.
    pub(crate) fn set_anim_attached_bullet(&mut self, id: AnimId, bullet: Option<u64>) -> bool {
        self.substrate.anims.set_attached_bullet(id, bullet)
    }

    /// `AnimClass::PointerExpired @ 0x00425150`'s `+0x17C` arm for a bullet
    /// leaving the game: each anim holding it clears the link and UnInits
    /// (`0x004251BE..0x004251D2`, vt+0xF8). Every producer attaches one anim
    /// per bullet, so the listener order the broadcast visits them in has no
    /// effect.
    ///
    /// The `+0x180` arm (the owning House expiring) is dormant: a House is
    /// never deleted during a match.
    pub(crate) fn expire_anim_attached_bullet(&mut self, bullet: u64, rules: Option<&RuleSet>) {
        for id in self.substrate.anims.attached_to(bullet) {
            self.substrate.anims.set_attached_bullet(id, None);
            self.destroy_anim_with_context(id, rules);
        }
    }

    /// `0x0043B4C0`, the test `AnimClass::AI` runs every frame for an anim
    /// whose type sets `PsiWarning=` (`AnimTypeClass+0x373`,
    /// `0x00423B29..0x00423B5C`) and stores negated as the hidden byte
    /// `+0x19D`: does `viewer` (native asks for the local player,
    /// `0x00A83D4C`) detect the anim? Not when it is allied with the anim's
    /// House (`HouseClass::IsAlliedWith @ 0x004F9A50`, null not allied).
    /// Otherwise, when one of its PsychicDetection buildings
    /// (`HouseClass+0x128`: placed buildings whose type's
    /// `PsychicDetectionRadius=` (`+0x170C`) is positive, added by
    /// `BuildingClass::Unlimbo` at `0x004414A0` and moved by
    /// `BuildingClass::ChangeOwner`) is operational (vt+0x350) and its
    /// GetCoords lies within the radius in leptons (`<< 8`, `jle` at
    /// `0x0043B5A9`) of the centre of the anim's cell (`MapClass::operator[]
    /// @ 0x00565730`, `CellClass::Get_Center_Coords @ 0x00480A30`), measured
    /// as `CoordStruct::Distance3D` (inline at `0x0043B547..0x0043B593`).
    /// The answer depends on the local player, so the presentation asks it
    /// in place of the hidden flag. Evidence: instruction reading only; no
    /// native run compares the distance's rounding at the radius edge.
    pub(crate) fn psi_warning_detected_by(
        &self,
        viewer: InternedId,
        anim: &AnimObject,
        rules: &RuleSet,
    ) -> bool {
        if anim.owner_house.is_some_and(|house| {
            crate::sim::combat::combat_weapon::is_ally_by_object(
                Some(&self.fog.alliances),
                &self.interner,
                viewer,
                house,
            )
        }) {
            return false;
        }
        let terrain = self.resolved_terrain.as_ref();
        let at = anim_world_coords(anim, &self.substrate.entities, terrain);
        let cell = crate::sim::projectile::cell_ground_coord(
            terrain,
            (at.x / 256) as u16,
            (at.y / 256) as u16,
        );
        self.substrate.entities.values().any(|entity| {
            if entity.owner() != viewer
                || entity.category != crate::map::entities::EntityCategory::Structure
                || !entity.lifecycle.object_alive
                || entity.lifecycle.in_limbo
            {
                return false;
            }
            let radius = rules
                .object(self.interner.resolve(entity.type_ref()))
                .map_or(0, |object| i32::from(object.psychic_detection_radius));
            if radius <= 0
                || self.building_operational_state(entity.stable_id(), rules) != Some(true)
            {
                return false;
            }
            let coords = crate::sim::movement::ground_pose::object_get_coords(entity, terrain);
            crate::util::native_x87::distance_3d_leptons(
                [coords.x, coords.y, coords.z],
                [cell.x, cell.y, cell.z],
            ) <= radius << 8
        })
    }

    pub(crate) fn update_building_damage_fire(&mut self, building_id: u64, rules: &RuleSet) {
        let Some((current, type_ref, position, prior_state, category)) =
            self.substrate.entities.get(building_id).map(|entity| {
                (
                    entity.health.current,
                    entity.type_ref(),
                    entity.position,
                    entity.damage_fire_state_active,
                    entity.category,
                )
            })
        else {
            return;
        };
        if category != crate::map::entities::EntityCategory::Structure {
            return;
        }
        let Some(object_type) = self.object_type(type_ref, rules) else {
            return;
        };
        let maximum = object_type.strength;
        let can_be_occupied = object_type.can_be_occupied;
        let image = object_type.image.clone();
        let foundation = object_type.foundation.clone();
        let ratio = if can_be_occupied {
            rules.general.condition_red
        } else {
            rules.general.condition_yellow
        };
        // Building43FC39..43FC84: CanBeOccupied selects red versus yellow;
        // TEST AH,0x41 includes unordered, with no HP/Strength positivity gate.
        let active = crate::sim::components::Health { current }.compare_ratio(maximum, ratio)
            != crate::util::native_x87::MaskedX87Ordering::Greater;
        if active == prior_state {
            return;
        }
        if !active {
            self.clear_building_damage_fire_slots(building_id, Some(rules));
        } else if !rules.general.damage_fire_types.is_empty() {
            let type_count = rules.general.damage_fire_types.len();
            let mut type_index = self
                .scenario_rng
                .next_range_u32_inclusive(0, type_count.saturating_sub(1) as u32)
                as usize;
            let offsets = rules
                .art()
                .get(&image)
                .map(|entry| entry.damage_fire_offsets.clone())
                .unwrap_or_default();
            let (foundation_w, foundation_h) =
                crate::rules::foundation::foundation_dimensions(&foundation);
            let foundation_sum = i32::from(foundation_w).wrapping_add(i32::from(foundation_h));
            let location = crate::sim::movement::ground_pose::object_location(
                self.substrate.entities.get(building_id).unwrap(),
                self.resolved_terrain.as_ref(),
            );
            let (base, _) = crate::sim::movement::ground_pose::building_render_order_parts(
                location, false, false,
            );

            for slot in 0..DAMAGE_FIRE_SLOT_COUNT {
                let occupied = self
                    .substrate
                    .entities
                    .get(building_id)
                    .and_then(|entity| entity.damage_fire_anim_ids[slot]);
                if occupied.is_some() {
                    break;
                }
                let Some(offset) = offsets.get(slot).copied() else {
                    break;
                };
                let fire_name = &rules.general.damage_fire_types[type_index].name;
                let fire_type = self.interner.intern(fire_name);
                let descriptor = AnimClassSpawnDescriptor {
                    type_name: fire_type,
                    rx: position.rx,
                    ry: position.ry,
                    sub_x: position.sub_x,
                    sub_y: position.sub_y,
                    z: position.z,
                    delay: 0,
                    loop_count: 1,
                    draw_flags: TRAILER_DRAW_FLAGS,
                    z_adjust: 0,
                    reverse: false,
                    use_cell_drawer: false,
                    terrain_attached: false,
                    draw_runtime: AnimDrawRuntime::default(),
                };
                let world = AnimWorldCoord {
                    x: base.x.wrapping_add(offset.world_dx),
                    y: base.y.wrapping_add(offset.world_dy),
                    z: base.z,
                };
                let anim_id = self
                    .spawn_anim_at_world(rules, descriptor, world)
                    .expect("validated stock damage-fire animation must spawn");
                // Original43C0D0/43C1EB retains only the Building5C8 slot.
                // The constructor leaves Anim+CC null: absolute coordinates
                // and the expiry listener do not require Object attachment.
                self.anim_mut_by_id(anim_id)
                    .expect("new damage fire")
                    .damage_fire_slot = Some((building_id, slot as u8));
                if let Some(entity) = self.substrate.entities.get_mut(building_id) {
                    entity.damage_fire_anim_ids[slot] = Some(anim_id);
                }

                let scaled = offset
                    .pixel_y
                    .wrapping_sub(foundation_sum.wrapping_mul(15))
                    .wrapping_mul(3);
                let z_adjust = (scaled >> 1).wrapping_sub(10).min(0);
                let effective_end = self
                    .substrate
                    .anims
                    .get(anim_id)
                    .map_or(0, |anim| anim.effective_end);
                let frame = if effective_end > 0 {
                    self.scenario_rng
                        .next_range_u32_inclusive(0, effective_end.wrapping_sub(1) as u32)
                        as i32
                } else {
                    0
                };
                self.set_anim_frame_and_z_adjust(anim_id, frame, z_adjust);
                type_index += 1;
                if type_index == type_count {
                    type_index = 0;
                }
            }
        }
        // Original43FC92 calls the producer (or43FCA4 recovery) before
        // 43FCBE stores5E8, including a producer's early slot/type exit.
        // Native replay: building_death_anims_joined.json, full target AI.
        if let Some(entity) = self.substrate.entities.get_mut(building_id) {
            entity.damage_fire_state_active = active;
        }
    }

    /// Recovery43FCA4 and destructor43BDE0 call Anim Destroy, not scalar delete.
    /// The destructor follows owner expiry, so its Anims no longer need Rules
    /// for SetOwner's intermediate display resubmission.
    pub(crate) fn clear_building_damage_fire_slots(
        &mut self,
        building_id: u64,
        rules: Option<&RuleSet>,
    ) {
        if !self.substrate.entities.contains(building_id) {
            return;
        }
        for slot in 0..DAMAGE_FIRE_SLOT_COUNT {
            let anim_id = self
                .substrate
                .entities
                .get(building_id)
                .and_then(|entity| entity.damage_fire_anim_ids[slot]);
            let Some(anim_id) = anim_id else {
                continue;
            };
            self.destroy_anim_with_context(anim_id, rules);
            if let Some(entity) = self.substrate.entities.get_mut(building_id) {
                entity.damage_fire_anim_ids[slot] = None;
            }
        }
    }

    fn choose_anim_rate(&mut self, config: &AnimTypeRuntimeConfig) -> u16 {
        let drawn = anim_random_rate(config, &mut self.scenario_rng);
        self.anim_rate(config, drawn)
    }

    /// The frame delay from `Rate=` or a drawn `RandomRate=` pick, normalized
    /// to the game speed for `Normalized=` types.
    fn anim_rate(&self, config: &AnimTypeRuntimeConfig, random_rate: Option<u16>) -> u16 {
        let delay = random_rate.unwrap_or(config.rate_logic_frames);
        if config.normalized {
            self.session.game_options.normalized_anim_delay(delay)
        } else {
            delay
        }
    }

    /// `AnimClass::ProcessBounceResult @ 0x00423930` and the landing arm of
    /// `AnimClass::AI` (`0x00423C3C..0x00424298`). Returns true when the chunk
    /// touched down, which always destroys it: Bounced and Stopped both take
    /// the landing arm.
    ///
    /// Each tick the body integrates (`BounceClass::Update`) and the anim's
    /// Location follows it, truncated (`vtable+0x1B4`, `0x00423AA8`). Bounced
    /// first builds `BounceAnim=` and hits every object in the landing cell
    /// within `DamageRadius=` (Manhattan leptons) directly; Stopped destroys
    /// the anim there (`0x00423976`), before the landing arm. Then:
    /// - in water (`CellClass+0xEC == 2`) below the deck plane (ground +
    ///   416): `Wake=` at the Location and the first `SplashList=` 3 leptons
    ///   up, no damage;
    /// - otherwise, only when the type names an `ExpireAnim=`: that anim
    ///   (flags `0x2600`, zAdjust -30), then `Apply_area_damage @ 0x00489280`
    ///   with `ftol(Damage=)` and the type's `Warhead=`, sourceless and
    ///   houseless, then the combat light (`0x0048A620`, not forced).
    ///
    /// RESIDUAL: the `Spawns=`/`SpawnCount=` burst (two `RandomRanged` draws,
    /// `0x00423F37..0x00423FC4`) and the `IsTiberium=` ring (`0x00423FC6..`)
    /// that follow a dry landing are not built. Trigger: METDEBRI and the
    /// CRYSTAL chunks (meteor showers); no stock `Bouncer=` death chunk sets
    /// either key.
    fn anim_bounce_step(
        &mut self,
        id: AnimId,
        config: &AnimTypeRuntimeConfig,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Option<bool> {
        let mut body = self.anim(id).and_then(|anim| anim.bounce)?;
        let outcome = self.anim_bounce_update(&mut body, rules);
        let position = body.position_leptons();
        if let Some(anim) = self.anim_mut_by_id(id) {
            anim.bounce = Some(body);
        }
        let mut bridge_state_changed = false;
        match outcome {
            BounceOutcome::Falling => {}
            BounceOutcome::Bounced => {
                bridge_state_changed |=
                    self.anim_bounce_contact(id, config, rules, overlay_registry, position);
            }
            BounceOutcome::Stopped => self.destroy_anim(id, rules),
        }
        if let Some(anim) = self.anim_mut_by_id(id) {
            anim.world_coord = AnimWorldCoord {
                x: position.x,
                y: position.y,
                z: position.z,
            };
        }
        if outcome == BounceOutcome::Falling {
            return None;
        }
        bridge_state_changed |= self.anim_bounce_landing(config, rules, overlay_registry, position);
        self.destroy_anim(id, rules);
        Some(bridge_state_changed)
    }

    /// The Bounced arm of `AnimClass::ProcessBounceResult` (`0x00423981..
    /// 0x00423A88`): `BounceAnim=` at GetCoords, then a direct
    /// `ReceiveDamage(ftol(Damage=), AdjustForZ(distance), Warhead=)` on each
    /// object of the landing cell's FirstObject list within `DamageRadius=`,
    /// measured `|dx| + |dy|` from the body to the object's GetCoords.
    fn anim_bounce_contact(
        &mut self,
        id: AnimId,
        config: &AnimTypeRuntimeConfig,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        position: glam::IVec3,
    ) -> bool {
        if let (Some(bounce_anim), Some(coord)) =
            (config.bounce_anim.as_deref(), self.anim_absolute_coord(id))
        {
            self.spawn_named_anim(rules, bounce_anim, coord, 0, BOUNCE_CONTACT_DRAW_FLAGS, 0);
        }
        // Map[coord] (`0x004239E5`); VERA keeps no objects on the dummy cell.
        let (Some(warhead_name), Some((rx, ry))) = (
            config.warhead.as_deref(),
            self.anim_map_cell(position.x, position.y),
        ) else {
            return false;
        };
        let damage = match X87Chop53::load_f64(config.damage).and_then(X87Chop53::ftol_i64) {
            Ok(damage) => damage as i32,
            Err(_) => return false,
        };
        let warhead_ref = self.interner.intern(warhead_name);
        use crate::sim::combat::combat_aoe::AreaDamageReceiver;
        use crate::sim::occupancy::CellObjectMember;
        let ground = MovementLayer::Ground;
        let mut current = self.cell_objects((rx, ry), ground).next();
        let mut bridge_state_changed = false;
        while let Some(target) = current {
            let center = match target {
                CellObjectMember::Entity(id) => self.substrate.entities.get(id).map(|entity| {
                    let [x, y] = crate::sim::movement::ground_pose::object_center_xy(entity);
                    (x, y)
                }),
                // Loaded Terrain objects retain their map-cell center. This
                // contact gate uses XY only (GetCoords5F65A0); it does not
                // substitute ground/deck height for the object's location.
                CellObjectMember::Terrain(id) => {
                    self.production.terrain_objects.get(&id).map(|tree| {
                        let coord = tree.world_coord();
                        (coord.x, coord.y)
                    })
                }
            };
            if let Some((x, y)) = center {
                let distance = position
                    .x
                    .wrapping_sub(x)
                    .wrapping_abs()
                    .wrapping_add(position.y.wrapping_sub(y).wrapping_abs());
                if distance <= config.damage_radius {
                    let distance_leptons = crate::util::native_x87::adjust_for_z_standard(distance);
                    let receiver = match target {
                        CellObjectMember::Entity(id) => AreaDamageReceiver::Entity(
                            crate::sim::combat::EntityDamageEvent::direct_receiver(
                                id,
                                damage,
                                distance_leptons,
                                crate::sim::combat::RAD_NO_ATTACKER,
                                None,
                                warhead_ref,
                                crate::sim::combat::ReceiverCallFlags {
                                    ignore_defenses: false,
                                    arg6: false,
                                },
                            ),
                        ),
                        CellObjectMember::Terrain(id) => {
                            let tree = &self.production.terrain_objects[&id];
                            AreaDamageReceiver::Terrain(crate::sim::combat::TerrainDamageEvent {
                                stable_id: id,
                                rx: tree.rx,
                                ry: tree.ry,
                                damage,
                                distance_leptons,
                                warhead_ref,
                                near_center_ic_isolation_eligible: false,
                            })
                        }
                    };
                    bridge_state_changed |= self
                        .commit_noncombat_aoe_receivers(rules, overlay_registry, &[receiver])
                        .bridge_state_changed;
                }
            }
            // 423A83 reads Object+30 AFTER ReceiveDamage. The native Terrain
            // lethal tail unlinks synchronously and clears that successor;
            // collecting a vector up front would incorrectly hit later objects.
            // Evidence: tools/spatial_oracle/terrain_debris_receiver.json.
            current = self.next_cell_object(target);
        }
        bridge_state_changed
    }

    /// The landing arm proper (`0x00423C4A..0x00423EF8`); see
    /// [`Self::anim_bounce_step`].
    fn anim_bounce_landing(
        &mut self,
        config: &AnimTypeRuntimeConfig,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        position: glam::IVec3,
    ) -> bool {
        let location = AnimWorldCoord {
            x: position.x,
            y: position.y,
            z: position.z,
        };
        let ground = crate::sim::projectile::projectile_ground_z(
            self.resolved_terrain.as_ref(),
            &self.effective_shared_cell_dummy(),
            crate::sim::projectile::ProjectileCoord::new(position.x, position.y, position.z),
        );
        let above_deck = position.z >= ground.wrapping_add(BRIDGE_DECK_HEIGHT_LEPTONS);
        if self.bounce_cell_is_water(position, rules) && !above_deck {
            let wake = rules.general.wake.name.clone();
            self.spawn_named_anim(rules, &wake, location, 0, BOUNCE_CONTACT_DRAW_FLAGS, 0);
            if let Some(splash) = rules.combat_damage.splash_list.first() {
                let splash_coord = AnimWorldCoord {
                    z: location.z.wrapping_add(BOUNCE_SPLASH_LIFT_LEPTONS),
                    ..location
                };
                self.spawn_named_anim(rules, splash, splash_coord, 0, BOUNCE_CONTACT_DRAW_FLAGS, 0);
            }
            return false;
        }
        let Some(expire) = config.expire_anim.as_deref() else {
            return false;
        };
        self.spawn_named_anim(
            rules,
            expire,
            location,
            0,
            BOUNCE_EXPIRE_DRAW_FLAGS,
            BOUNCE_EXPIRE_Z_ADJUST,
        );
        let Some(warhead_name) = config.warhead.as_deref() else {
            return false;
        };
        let (Some(warhead), Ok(damage)) = (
            rules.warhead(warhead_name),
            X87Chop53::load_f64(config.damage).and_then(X87Chop53::ftol_i64),
        ) else {
            return false;
        };
        let damage = damage as i32;
        let warhead_ref = self.interner.intern(warhead_name);
        let impact =
            crate::sim::projectile::ProjectileCoord::new(position.x, position.y, position.z);
        // Anim's Apply_area_damage call at0x423EAB completes its bridge
        // continuation before the combat-light call at0x423EF8.
        let bridge_state_changed = crate::sim::combat::world_receiver::apply_area_damage(
            self,
            rules,
            overlay_registry,
            impact,
            damage,
            warhead,
            (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
        );
        self.combat_light_requests
            .push(crate::sim::combat::CombatLightRequest {
                target_id: None,
                damage,
                warhead_ref,
                coord: impact,
                force_create: false,
                flags: 0,
            });
        bridge_state_changed
    }

    /// `new AnimClass(AnimTypes[FindIndex(name)], &coord, delay, 1, flags,
    /// zAdjust, 0)`: a landing chunk's follow-up anims and the rocket puffs of
    /// Rocket Process and the spawn manager's Boomer launch. A type that never
    /// bound constructs nothing.
    pub(crate) fn spawn_named_anim(
        &mut self,
        rules: &RuleSet,
        type_name: &str,
        coord: AnimWorldCoord,
        delay: u16,
        draw_flags: u32,
        z_adjust: i32,
    ) {
        let type_id = self.interner.intern(type_name);
        let (rx, ry, sub_x, sub_y, z) = coord.to_cell_sub_z();
        let descriptor = AnimClassSpawnDescriptor {
            delay,
            loop_count: 1,
            draw_flags,
            z_adjust,
            reverse: false,
            ..AnimClassSpawnDescriptor::new(type_id, rx, ry, sub_x, sub_y, z)
        };
        if let Err(error) = self.spawn_anim_at_world(rules, descriptor, coord) {
            log::debug!("anim [{type_name}] did not construct: {error}");
        }
    }

    /// Native `AnimClass::Start @ 0x00424CE0` — the anim's sound emitter.
    ///
    /// `AnimClass::Start @ 0x00424CE0`: it Marks, then
    /// `if (Anim+0x198 /* silent */ == 0 && AnimType+0x2F8 != -1)` takes the
    /// coordinate through vtable `+0x48` and plays it with `VocClass::PlayAt`,
    /// then calls `AnimClass::Middle @ 0x00424F00` when `AnimType+0x298 == 0`.
    /// `Middle` is `SpawnsParticle=` looped `NumParticles=` times, `Scorch=`,
    /// `Crater=`, `ForceBigCraters=`, and plays nothing. Callers: the
    /// constructor for a zero delay (`0x00422702`), and `AnimClass::AI` when
    /// the delay countdown reaches zero (`0x004243A1`) or a `Next=` type takes
    /// over (`0x00424925`).
    ///
    /// `AnimType+0x2F8` is one slot: `AnimTypeClass::ReadINI @ 0x00427D00`
    /// reads `Report=` into it only when `StartSound=` resolved to `-1`, which
    /// is what `start_sound.or(report)` reproduces.
    ///
    /// Constructor-time starts pass no overlay registry; no retail marking
    /// anim has fewer than two frames, so none marks from there.
    fn anim_start(
        &mut self,
        id: AnimId,
        config: &AnimTypeRuntimeConfig,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        let sound_name = config
            .start_sound
            .as_ref()
            .or(config.report.as_ref())
            .cloned();
        if let Some(sound_name) = sound_name
            && let Some(world) = self.anim_absolute_coord(id)
        {
            if let Some(anim) = self.anim_mut_by_id(id) {
                anim.start_sound_active = true;
            }
            self.sound_events.push(SimSoundEvent::AnimationStarted {
                anim_id: id,
                sound_id: sound_name,
                world,
            });
        }
        if middle_frame(config).is_none() {
            self.anim_middle(id, config, rules, overlay_registry);
        }
    }

    /// `AnimClass::Middle @ 0x00424F00`: marks the ground under the anim.
    ///
    /// The size is the type's middle-frame size (`ArtEntry::frame_width`,
    /// 30 x 30 without an image). Nothing lands when the anim stands 30
    /// leptons or more above the ground (`vtable+0x1C8`, `ObjectClass::
    /// GetHeight @ 0x005F5F40`, measured from the stored Location; anims
    /// never set OnBridge). The marks land at `GetCoords` (`vtable+0x48`).
    ///
    /// RESIDUAL: the `SpawnsParticle=`/`NumParticles=` loop
    /// (`0x00425008..0x0042504B`, `0x0062E430` at the raw Location) is not
    /// ported. Trigger: VIRUSD, the only retail type with the key.
    fn anim_middle(
        &mut self,
        id: AnimId,
        config: &AnimTypeRuntimeConfig,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        if !(config.scorch || config.crater) {
            return;
        }
        let (Some(stored), Some(coord)) = (
            self.anim(id).map(|anim| anim.world_coord),
            self.anim_absolute_coord(id),
        ) else {
            return;
        };
        let stored = crate::sim::projectile::ProjectileCoord::new(stored.x, stored.y, stored.z);
        let ground = crate::sim::projectile::projectile_ground_z(
            self.resolved_terrain.as_ref(),
            &self.effective_shared_cell_dummy(),
            stored,
        );
        if stored.z.wrapping_sub(ground) >= ANIM_MIDDLE_MAX_HEIGHT {
            return;
        }
        let type_name = self
            .anim(id)
            .map(|anim| self.interner.resolve(anim.type_id).to_ascii_uppercase());
        let (width, height) = type_name
            .as_deref()
            .and_then(|name| rules.art().get(name))
            .map_or((30, 30), |entry| {
                (i32::from(entry.frame_width), i32::from(entry.frame_height))
            });
        let marks = crate::sim::combat::smudge_dispatch::AnimMiddleMarks {
            scorch: config.scorch,
            crater: config.crater,
            force_big_craters: config.force_big_craters,
            width,
            height,
        };
        let coord = crate::sim::smudge_grid::SimCoord {
            x: coord.x,
            y: coord.y,
            z: coord.z,
        };
        self.commit_smudge_request_inline(
            rules,
            overlay_registry,
            crate::sim::combat::SmudgeSpawnRequest::AnimMiddle { coord, marks },
        );
    }

    fn switch_anim_type(
        &mut self,
        id: AnimId,
        next: &str,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        let Some(config) = rules.art().anim_runtime_config(next).cloned() else {
            self.destroy_anim(id, rules);
            return;
        };
        let Ok((effective_end, effective_loop_end)) = effective_bounds(next, &config) else {
            self.destroy_anim(id, rules);
            return;
        };
        let type_id = self.interner.intern(next);
        let rate_reload = self.choose_anim_rate(&config);
        let frame_timer =
            CdTimer::started(self.session.binary_frame as i32, i32::from(rate_reload));
        let stop_sound_id = config
            .stop_sound
            .as_deref()
            .map(|sound| self.interner.intern(sound));
        let constructor_reverse = self
            .anim(id)
            .is_some_and(|anim| anim.runtime.constructor_reverse);
        let reverse = constructor_reverse || config.reverse;
        if let Some(anim) = self.anim_mut_by_id(id) {
            anim.type_id = type_id;
            anim.effective_end = effective_end;
            anim.effective_loop_end = effective_loop_end;
            anim.stop_sound_id = stop_sound_id;
            anim.runtime.current_frame = if reverse {
                effective_loop_end.wrapping_sub(1)
            } else {
                0
            };
            anim.runtime.frame_step = if reverse { -1 } else { 1 };
            anim.runtime.delay_remaining = 0;
            anim.runtime.rate_reload = rate_reload;
            anim.runtime.frame_timer = frame_timer;
            anim.runtime.loop_remaining = native_loop_remaining(config.loop_count, 1);
            anim.runtime.first_ai_guard = false;
            anim.runtime.inactive = false;
        }
        self.anim_start(id, &config, rules, overlay_registry);
    }
}

fn effective_bounds(
    type_name: &str,
    config: &AnimTypeRuntimeConfig,
) -> Result<(i32, i32), AnimSpawnError> {
    if !config.art_body_read {
        return Ok((config.end, config.loop_end));
    }
    let raw = config
        .raw_shp_frame_count
        .ok_or_else(|| AnimSpawnError::UnboundType(type_name.to_string()))?;
    let effective_end = if config.end == -1 {
        if config.shadow { raw / 2 } else { raw }
    } else {
        config.end
    };
    let effective_loop_end = if config.loop_end == -1 {
        effective_end
    } else {
        config.loop_end
    };
    Ok((effective_end, effective_loop_end))
}

/// `AnimTypeClass+0x320` MaxZVel: no INI key reads it; the AnimType
/// constructor writes 3.5 (`0x00427627`).
const BOUNCER_MAX_Z_VEL: NativeF64Bits = NativeF64Bits::from_bits(0x400c_0000_0000_0000);
/// The gravity the Bouncer arm hands `BounceClass::Init` as two literal pushes
/// (`0x3FF66666`/`0x60000000`, the double 1.4).
const BOUNCER_GRAVITY: NativeF64Bits = NativeF64Bits::from_bits(0x3ff6_6666_6000_0000);
/// The `+ 1.0` bias of the Z divisor (`0x007E1718`).
const BOUNCER_Z_RANGE_BIAS: NativeF64Bits = NativeF64Bits::from_bits(0x3ff0_0000_0000_0000);
/// The body starts this far above the anim's coordinate.
const BOUNCER_LAUNCH_LIFT_LEPTONS: i32 = 10;
/// `AnimClass` constructor `drawFlags` for the anims a landing chunk makes:
/// `BounceAnim=`, `Wake=` and the splash (`0x600`), `ExpireAnim=` (`0x2600`,
/// zAdjust -30).
const BOUNCE_CONTACT_DRAW_FLAGS: u32 = 0x600;
const BOUNCE_EXPIRE_DRAW_FLAGS: u32 = 0x2600;
const BOUNCE_EXPIRE_Z_ADJUST: i32 = -30;
/// A water splash rises 3 leptons above the chunk (`ADD EDI, 3`, `0x00423DBE`).
const BOUNCE_SPLASH_LIFT_LEPTONS: i32 = 3;

/// The Scenario draws `AnimClass::AnimClass @ 0x00421EA0` takes, in order:
/// the `RandomRate=` pick (`0x004221F5`; none when the clamped bounds are
/// equal, as for every stock debris chunk), then the `Bouncer=` arm's launch
/// (`0x004224D9..0x00422648`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimConstructorDraws {
    /// Present when a producer has already crossed Anim42203D. Admission must
    /// retain that constructor identity rather than consume the cursor again.
    pub(crate) native_unique_id: Option<i32>,
    /// The drawn `RandomRate=` delay in logic frames, before normalization.
    pub random_rate: Option<u16>,
    pub bounce: Option<BounceState>,
}

/// Transient entry state for the single Anim constructor; never simulation
/// state. Fresh map loading may already own the Abstract ID, while death debris
/// may have crossed both identity and RNG boundaries before registry admission.
#[derive(Debug, Clone, Copy)]
pub(crate) enum AnimConstructorInput {
    Runtime,
    AssignedIdentity(i32),
    Preconsumed(AnimConstructorDraws),
}

/// Take the constructor's draws for an anim of `config` at `coord`.
///
/// RESIDUAL: the `IsMeteor=` arm (`0x004222FF`: three draws at `0x0042230B`,
/// `0x0042231F`, `0x004223C4`, and its per-tick gravity add) is not built; a
/// meteor type constructs as a plain anim. Trigger: meteor showers only
/// (scenario triggers); no death or weapon produces one.
pub(crate) fn anim_constructor_draws(
    config: &AnimTypeRuntimeConfig,
    coord: AnimWorldCoord,
    rng: &mut crate::sim::rng::SimRng,
) -> Result<AnimConstructorDraws, NativeX87Error> {
    let random_rate = anim_random_rate(config, rng);
    let bounce = if config.bouncer && !config.is_meteor {
        Some(bouncer_launch(config, coord, rng)?)
    } else {
        None
    };
    Ok(AnimConstructorDraws {
        native_unique_id: None,
        random_rate,
        bounce,
    })
}

fn anim_random_rate(
    config: &AnimTypeRuntimeConfig,
    rng: &mut crate::sim::rng::SimRng,
) -> Option<u16> {
    config
        .random_rate_logic_frames
        .map(|(low, high)| rng.next_range_u32_inclusive(u32::from(low), u32::from(high)) as u16)
}

/// The `Bouncer=` arm of `AnimClass::AnimClass` (`0x004224D9..0x00422648`):
/// three `Random__Next()` draws, Z then Y then X, each taken as `|draw| %
/// divisor` (`CDQ/XOR/SUB`, `IDIV`):
/// - `Velocity.Z = |a| % ftol(MaxZVel - MinZVel + 1.0) + MinZVel`
/// - `Velocity.Y = |b| % ftol(MaxXYVel + MaxXYVel) - MaxXYVel`, X likewise;
///
/// then `BounceClass::Init` from 10 leptons above the coordinate with the
/// type's `Elasticity=`, gravity 1.4 and no spin, which draws three more.
/// With the stock chunks' `MinZVel` above the fixed `MaxZVel` 3.5, the Z
/// divisor is negative (-20 large, -15 small).
///
/// Native execution: `tools/spatial_oracle/anim_bouncer_launch.py`.
fn bouncer_launch(
    config: &AnimTypeRuntimeConfig,
    coord: AnimWorldCoord,
    rng: &mut crate::sim::rng::SimRng,
) -> Result<BounceState, NativeX87Error> {
    use crate::sim::voxel_anim::raw_abs_modulo;

    let z_draw = rng.next_u32();
    let y_draw = rng.next_u32();
    let x_draw = rng.next_u32();
    let max_xy = X87Chop53::load_f64(config.max_xy_vel)?;
    let min_z = X87Chop53::load_f64(config.min_z_vel)?;
    let xy_divisor = X87Chop53::ftol_i64(X87Chop53::add(max_xy, max_xy))? as i32;
    let z_divisor = X87Chop53::ftol_i64(X87Chop53::add(
        X87Chop53::sub(X87Chop53::load_f64(BOUNCER_MAX_Z_VEL)?, min_z),
        X87Chop53::load_f64(BOUNCER_Z_RANGE_BIAS)?,
    ))? as i32;
    let velocity_z = X87Chop53::store_f32(X87Chop53::add(
        X87Chop53::load_i32(raw_abs_modulo(z_draw, z_divisor)),
        min_z,
    ))?;
    let velocity_y = X87Chop53::store_f32(X87Chop53::sub(
        X87Chop53::load_i32(raw_abs_modulo(y_draw, xy_divisor)),
        max_xy,
    ))?;
    let velocity_x = X87Chop53::store_f32(X87Chop53::sub(
        X87Chop53::load_i32(raw_abs_modulo(x_draw, xy_divisor)),
        max_xy,
    ))?;
    BounceState::init(
        glam::IVec3::new(
            coord.x,
            coord.y,
            coord.z.wrapping_add(BOUNCER_LAUNCH_LIFT_LEPTONS),
        ),
        config.elasticity,
        BOUNCER_GRAVITY,
        NativeF64Bits::POSITIVE_ZERO,
        [velocity_x, velocity_y, velocity_z],
        NativeF64Bits::POSITIVE_ZERO,
        rng,
    )
}

/// `AnimClass::Middle @ 0x00424F00`'s height gate (`0x00425057`).
pub(crate) const ANIM_MIDDLE_MAX_HEIGHT: i32 = 30;

/// The type's middle frame, `AnimTypeClass+0x298`: the raw SHP frame count
/// halved (`AnimTypeClass::Load_Image @ 0x00427B50`). None when it is zero,
/// where Start runs Middle instead.
fn middle_frame(config: &AnimTypeRuntimeConfig) -> Option<i32> {
    config
        .raw_shp_frame_count
        .map(|raw| raw / 2)
        .filter(|&middle| middle != 0)
}

fn native_loop_remaining(loop_count: i32, constructor_loop: i32) -> u8 {
    // gamemd-derived: `AnimClass::Constructor @ 0x00421EA0`, branch at
    // 0x004226BF. The constructor argument is compared as signed before its
    // low byte participates in the wrapping LoopCount multiplication.
    let constructor_factor = if constructor_loop > 1 {
        constructor_loop as u8
    } else {
        1
    };
    (loop_count as u8).wrapping_mul(constructor_factor).max(1)
}

fn trailer_cadence_matches(binary_frame: u64, separation: i32) -> bool {
    separation == 1 || (separation > 1 && (binary_frame as i32) % separation == 0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnimBoundary {
    Continue,
    Bounce,
    Loop,
    Complete,
}

/// Native AnimClass::AI committed-frame tail, 0x0042468C..0x004247B1.
/// Native boundary/reset goldens: tools/anim_oracle/boundary.json. The direction
/// flags select the reverse endpoint and reset; the signed frame step does not.
fn advance_anim_boundary(anim: &mut AnimObject, config: &AnimTypeRuntimeConfig) -> AnimBoundary {
    let stage = anim.runtime.current_frame;
    let last_loop = anim.runtime.loop_remaining <= 1;
    let loop_end = anim.effective_loop_end.wrapping_sub(config.start);

    // 0x0042468C..0x004246DC: ping-pong has its own lower-end equality test,
    // returns immediately after NEG, and does not consume a loop.
    if config.ping_pong
        && if last_loop {
            stage >= anim.effective_end || stage == 0
        } else {
            stage >= loop_end || stage == config.start
        }
    {
        anim.runtime.frame_step = anim.runtime.frame_step.wrapping_neg();
        return AnimBoundary::Bounce;
    }

    let reverse = config.reverse || anim.runtime.constructor_reverse;
    let upper_end = if last_loop {
        anim.effective_end
    } else {
        loop_end
    };
    // 0x0042470C..0x00424738: Shadow adds the forward LoopEnd-Start boundary
    // even on the final loop. It does not replace the ordinary End test.
    if !(stage >= upper_end
        || (config.shadow && !reverse && stage >= loop_end)
        || (reverse && stage <= 0))
    {
        return AnimBoundary::Continue;
    }
    if anim.runtime.loop_remaining != 0 && anim.runtime.loop_remaining != u8::MAX {
        anim.runtime.loop_remaining -= 1;
    }
    if anim.runtime.loop_remaining == 0 {
        return AnimBoundary::Complete;
    }
    // 0x0042477B..0x004247AB: reset from the two reverse flags, independently of
    // the current (possibly ping-pong-negated) frame step.
    anim.runtime.current_frame = if reverse {
        anim.effective_loop_end
    } else {
        config.loop_start.wrapping_sub(config.start)
    };
    AnimBoundary::Loop
}

#[cfg(test)]
mod long_tail_contract_tests {
    use super::*;

    #[test]
    fn yr_long_tail_vectors() {
        assert_eq!(directional_tumble_frame(5, 0, 9), 38);
        assert_eq!(directional_tumble_frame(5, 7, 14), 4);
        assert_eq!(settled_bounce_frame(5), 41);
        assert_eq!(bounce_spawn_count(true, 4, 2, 3), 5);
        assert_eq!(
            anim_translucency_selection(AnimTranslucencyInput {
                base_flags: 0,
                forced_translucent: false,
                forced_uses_75: false,
                translucency_detail_level: 1,
                game_detail_level: 2,
                translucent_ramp: true,
                current_frame: 7,
                frame_count: 10,
                explicit_translucency: 0,
                instance_ramp: 0,
            }),
            AnimTranslucencyResult {
                draw: true,
                flags: 6
            }
        );
    }
}

#[cfg(test)]
#[path = "anim_debris_tests.rs"]
mod debris_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;

    #[test]
    fn damage_fire_references_follow_original_owner_then_anim_expiry() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/anim_damage_fire_expiry.json",
        ))
        .unwrap();
        assert_eq!(rows.len(), 8);
        for row in rows {
            let slot = row["slot"].as_u64().unwrap() as usize;
            let (mut sim, rules, building_id) = damage_fire_fixture(false);
            let owner_coord = sim.anim_owner_coords(building_id).unwrap();
            let type_id = sim.interner.intern("FIRE01");
            let id = sim
                .spawn_anim_at_world(
                    &rules,
                    runtime_descriptor(type_id, 0),
                    AnimWorldCoord {
                        x: owner_coord.x + 128,
                        y: owner_coord.y + 256,
                        z: owner_coord.z + 300,
                    },
                )
                .unwrap();
            sim.set_anim_owner_object(id, Some(building_id), &rules);
            sim.substrate
                .entities
                .get_mut(building_id)
                .unwrap()
                .damage_fire_anim_ids[slot] = Some(id);
            sim.anim_mut_by_id(id).unwrap().damage_fire_slot = Some((building_id, slot as u8));
            sim.detach_all_pointer_expired(building_id, &rules, None);
            for phase in ["owner_expiry", "anim_expiry"] {
                if phase == "anim_expiry" {
                    sim.destroy_anim(id, &rules);
                }
                let expected = &row[phase];
                let anim = sim.anim(id).unwrap();
                assert_eq!(
                    anim.owner_entity.is_some(),
                    expected["owner"].as_bool().unwrap()
                );
                assert_eq!(
                    anim.display.marked_on_map,
                    expected["marked"].as_bool().unwrap()
                );
                assert_eq!(
                    anim.runtime.inactive,
                    expected["expired"].as_bool().unwrap()
                );
                assert_eq!(
                    [anim.world_coord.x, anim.world_coord.y, anim.world_coord.z],
                    std::array::from_fn::<_, 3, _>(
                        |i| expected["stored"][i].as_i64().unwrap() as i32
                    )
                );
                let building = sim.substrate.entities.get(building_id).unwrap();
                for (i, id) in building.damage_fire_anim_ids.iter().enumerate() {
                    assert_eq!(id.is_some(), expected["slots"][i].as_bool().unwrap());
                }
                assert_eq!(sim.substrate.display.layer_of(id), None);
            }
        }
    }

    #[test]
    fn building_uninit_expires_attached_fires_before_destructor_destroy_and_survives_save() {
        let (mut sim, rules, building_id) = damage_fire_fixture(false);
        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .health
            .current = 50;
        sim.update_building_damage_fire(building_id, &rules);
        let ids = sim
            .entities()
            .get(building_id)
            .unwrap()
            .damage_fire_anim_ids;
        // This is the explicit attached-owner expiry control, as in the
        // native eight-slot corpus. The ordinary fire producer leavesCC0.
        for id in ids.into_iter().flatten() {
            sim.set_anim_owner_object(id, Some(building_id), &rules);
        }
        let coords: Vec<_> = ids
            .iter()
            .flatten()
            .map(|id| (*id, sim.anim(*id).unwrap().world_coord))
            .collect();
        sim.sound_events.clear();
        sim.uninit_with_rules(building_id, &rules);
        assert!(
            sim.sound_events.is_empty(),
            "owner expiry does not call Anim Destroy"
        );
        assert_eq!(
            sim.entities()
                .get(building_id)
                .unwrap()
                .damage_fire_anim_ids,
            ids
        );
        for (id, coord) in &coords {
            let anim = sim.anim(*id).unwrap();
            assert_eq!(anim.world_coord, *coord);
            assert_eq!(anim.owner_entity, None);
            assert!(anim.runtime.inactive);
            assert!(!sim.substrate.pending_delete.contains(id));
            assert_eq!(sim.substrate.display.layer_of(*id), None);
        }
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let hash = sim.state_hash();
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "building-expiry", 0);
        sim = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        sim.restore_after_snapshot_load().unwrap();
        assert_eq!(sim.state_hash(), hash);
        for (slot, id) in ids.iter().enumerate() {
            if let Some(id) = id {
                assert_eq!(
                    sim.anim(*id).unwrap().damage_fire_slot,
                    Some((building_id, slot as u8))
                );
            }
        }
        sim.process_pending_delete();
        assert!(sim.entities().get(building_id).is_none());
        for (id, _) in coords {
            assert!(sim.anim(id).is_none());
            assert!(sim.sound_events.iter().any(|event| matches!(event,
                SimSoundEvent::ObjectSoundReleased { owner } if *owner == id)));
        }
    }

    #[test]
    fn animation_display_owner_histories_match_native_and_survive_save() {
        use crate::sim::snapshot::GameSnapshot;
        use crate::sim::world::display_layers::DisplayLayer;
        let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/display_anim_owner.json",
        ))
        .unwrap();
        assert_eq!(rows.len(), 24);
        for row in rows {
            let input = &row["input"];
            let layer_name = match input["layer"].as_i64().unwrap() {
                -1 => "None",
                0 => "Underground",
                1 => "Surface",
                2 => "Ground",
                3 => "Air",
                4 => "Top",
                _ => unreachable!(),
            };
            let rules = runtime_rules(
                &format!(
                    "[A]\nLayer={layer_name}\nYSortAdjust={}\n[A_NEXT]\nLayer={layer_name}\nYSortAdjust=999\n",
                    input["adjust"].as_i64().unwrap()
                ),
                &[("A", 10), ("A_NEXT", 10)],
            );
            let mut sim = Simulation::with_seed(0);
            let owner_id = sim.allocate_stable_id();
            let owner_name = sim.interner.intern("HOUSE");
            let type_ref = sim.interner.intern("UNIT");
            let mut owner = GameEntity::new_at_frame_zero_for_test(
                owner_id,
                10,
                10,
                0,
                0,
                owner_name,
                Health { current: 100 },
                type_ref,
                EntityCategory::Unit,
                0,
                5,
                false,
            );
            owner.position.sub_x = SimFixed::from_num(128);
            owner.position.sub_y = SimFixed::from_num(128);
            owner.position.exact_z_leptons = Some(0);
            sim.substrate.entities.insert(owner);
            let type_id = sim.interner.intern("A");
            let id = sim
                .spawn_anim_at_world(
                    &rules,
                    runtime_descriptor(type_id, 0),
                    AnimWorldCoord {
                        x: 2816,
                        y: 2944,
                        z: 300,
                    },
                )
                .unwrap();
            // Native corpus deliberately has type+340=999 and a different
            // retained instance+104. Exercise the real Next type transition.
            sim.switch_anim_type(id, "A_NEXT", &rules, None);
            assert_eq!(
                sim.anim(id).unwrap().display.y_sort_adjust,
                input["adjust"].as_i64().unwrap() as i32
            );
            sim.mark_anim_display(id, input["marked"].as_bool().unwrap());
            for observation in row["observations"].as_array().unwrap() {
                match observation["op"].as_str().unwrap() {
                    "submit" => sim.submit_anim_display(id, Some(&rules)),
                    "attach" => {
                        assert!(sim.set_anim_owner_object(id, Some(owner_id), &rules));
                    }
                    "detach" => {
                        assert!(sim.set_anim_owner_object(id, None, &rules));
                    }
                    "move_owner" => {
                        let owner = sim.substrate.entities.get_mut(owner_id).unwrap();
                        owner.position.rx = 3000 / 256;
                        owner.position.ry = 3100 / 256;
                        owner.position.sub_x = SimFixed::from_num(3000 % 256);
                        owner.position.sub_y = SimFixed::from_num(3100 % 256);
                        owner.position.exact_z_leptons = Some(700);
                    }
                    "expire" => {
                        assert!(sim.expire_anim_owner_reference(id, owner_id));
                    }
                    other => panic!("unexpected {other}"),
                }
                let expected = &observation["state"];
                let anim = sim.anim(id).unwrap();
                let coord = |key: &str| AnimWorldCoord {
                    x: expected[key][0].as_i64().unwrap() as i32,
                    y: expected[key][1].as_i64().unwrap() as i32,
                    z: expected[key][2].as_i64().unwrap() as i32,
                };
                assert_eq!(
                    anim.world_coord,
                    coord("stored"),
                    "{}: {observation}",
                    input["name"]
                );
                assert_eq!(sim.anim_absolute_coord(id), Some(coord("absolute")));
                assert_eq!(
                    anim.owner_entity.is_some(),
                    expected["owner"].as_bool().unwrap()
                );
                assert_eq!(
                    anim.display.marked_on_map,
                    expected["marked"].as_bool().unwrap()
                );
                assert_eq!(
                    anim.runtime.inactive,
                    expected["detached"].as_bool().unwrap()
                );
                assert_eq!(
                    anim_display_sort_key(anim, &sim.substrate.entities),
                    expected["y_sort"].as_i64().unwrap() as i32
                );
                let queried = expected["queried_layer"].as_i64().unwrap();
                let queried = u8::try_from(queried)
                    .ok()
                    .and_then(DisplayLayer::from_index);
                assert_eq!(sim.anim_display_layer(id, Some(&rules)), queried);
                for layer in 0..5 {
                    let expected_ids = if expected["layers"][layer] == 0 {
                        vec![]
                    } else {
                        vec![id]
                    };
                    assert_eq!(
                        sim.substrate
                            .display
                            .members(DisplayLayer::from_index(layer as u8).unwrap()),
                        expected_ids
                    );
                }
                let bytes = GameSnapshot::save(&sim, 0, 0, "anim-display", 0);
                let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
                restored.restore_after_snapshot_load().unwrap();
                assert_eq!(
                    restored.state_hash(),
                    sim.state_hash(),
                    "{}: save {observation}",
                    input["name"]
                );
                sim = restored;
            }
            sim.destroy_anim(id, &rules);
            assert_eq!(sim.substrate.display.layer_of(id), None);
            sim.process_pending_delete();
            assert!(sim.anim(id).is_none());
        }
    }

    #[test]
    fn gsi_13_04_signed_constructor_loop_preserves_negative_one_distinction() {
        assert_eq!(native_loop_remaining(0, 1), 1);
        assert_eq!(native_loop_remaining(1, 1), 1);
        assert_eq!(native_loop_remaining(2, 1), 2);
        assert_eq!(native_loop_remaining(-1, 1), u8::MAX);
        assert_eq!(native_loop_remaining(128, 2), 1);
        assert_eq!(native_loop_remaining(-1, -1), u8::MAX);
        assert_eq!(native_loop_remaining(-1, 255), 1);
    }

    #[test]
    fn trailer_zero_separation_never_divides_or_spawns() {
        assert!(!trailer_cadence_matches(0, 0));
        assert!(trailer_cadence_matches(7, 1));
        assert!(trailer_cadence_matches(6, 3));
        assert!(!trailer_cadence_matches(7, 3));
    }

    fn damage_fire_fixture(can_be_occupied: bool) -> (Simulation, RuleSet, u64) {
        damage_fire_fixture_with_strength(can_be_occupied, 100)
    }

    fn damage_fire_fixture_with_strength(
        can_be_occupied: bool,
        strength: i32,
    ) -> (Simulation, RuleSet, u64) {
        let rules_ini = IniFile::from_str(&format!(
            "[BuildingTypes]\n0=TESTBLD\n\n\
             [TESTBLD]\nStrength={strength}\nImage=TESTART\nCanBeOccupied={}\n\n\
             [General]\nDamageFireTypes=FIRE01,FIRE02,FIRE03\n\n\
             [AudioVisual]\nConditionYellow=50%\nConditionRed=25%\n",
            if can_be_occupied { "yes" } else { "no" },
        ));
        let art_ini = IniFile::from_str(
            "[TESTART]\nFoundation=4x4\nDamageFireOffset0=-24,-1\nDamageFireOffset1=64,36\n\n\
             [FIRE01]\nRate=450\nLoopCount=-1\nStartSound=BuildingFireBig\n\n\
             [FIRE02]\nRate=450\nLoopCount=-1\nStartSound=BuildingFireMed\n\n\
             [FIRE03]\nRate=450\nLoopCount=-1\nStartSound=BuildingFireSmall\n",
        );
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini)
            .expect("damage-fire rules and ART");
        let mut art = ArtRegistry::from_ini(&art_ini);
        art.bind_anim_frame_count_for_test("FIRE01", 30);
        art.bind_anim_frame_count_for_test("FIRE02", 64);
        art.bind_anim_frame_count_for_test("FIRE03", 30);
        rules.install_art_fixture(art);

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("A");
        let type_ref = sim.interner.intern("TESTBLD");
        for name in ["FIRE01", "FIRE02", "FIRE03"] {
            sim.interner.intern(name);
        }
        let id = sim.allocate_stable_id();
        let mut building = GameEntity::new_at_frame_zero_for_test(
            id,
            10,
            10,
            0,
            0,
            owner,
            Health { current: 100 },
            type_ref,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        building.foundation = "4x4".to_string();
        sim.substrate.entities.insert(building);
        sim.reveal(id);
        (sim, rules, id)
    }

    #[test]
    fn original_health_ratio_corpus_drives_damage_fire_update() {
        for row in crate::sim::health_ratio_fixture::rows() {
            for occupied in [false, true] {
                let (mut sim, mut rules, id) =
                    damage_fire_fixture_with_strength(occupied, row.input.strength);
                rules.general.condition_yellow = row.input.yellow();
                rules.general.condition_red = row.input.red();
                sim.substrate.entities.get_mut(id).unwrap().health.current = row.input.current;
                sim.update_building_damage_fire(id, &rules);
                let building = sim.substrate.entities.get(id).unwrap();
                let expected = if occupied {
                    row.output.damage_fire_occupied
                } else {
                    row.output.damage_fire_ordinary
                };
                assert_eq!(
                    building.damage_fire_state_active, expected,
                    "{row:?}, occupied={occupied}"
                );
                assert_eq!(
                    building.damage_fire_anim_ids.iter().flatten().count(),
                    if expected { 2 } else { 0 },
                    "{row:?}"
                );
            }
        }
    }

    fn runtime_rules(art_text: &str, frame_counts: &[(&str, i32)]) -> RuleSet {
        let mut rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nDamageFireTypes=\n\n[AudioVisual]\nConditionYellow=50%\nConditionRed=25%\n",
        ))
        .unwrap();
        let mut art = ArtRegistry::from_ini(&IniFile::from_str(art_text));
        for &(name, frames) in frame_counts {
            art.bind_anim_frame_count_for_test(name, frames);
        }
        rules.replace_art_registry_for_test(art);
        rules
    }

    fn runtime_descriptor(type_name: InternedId, delay: u16) -> AnimClassSpawnDescriptor {
        AnimClassSpawnDescriptor {
            type_name,
            rx: 0,
            ry: 0,
            sub_x: crate::util::fixed_math::SIM_ZERO,
            sub_y: crate::util::fixed_math::SIM_ZERO,
            z: 0,
            delay,
            loop_count: 1,
            draw_flags: TRAILER_DRAW_FLAGS,
            z_adjust: 0,
            reverse: false,
            use_cell_drawer: false,
            terrain_attached: false,
            draw_runtime: AnimDrawRuntime::default(),
        }
    }

    /// Techno710443..71044E clears the matching retained animation pointer,
    /// leaving Unit deployment state for the mission's next visit.
    #[test]
    fn deployment_attachment_expiry_clears_only_the_matching_pointer() {
        for detach_with_coords in [false, true] {
            let rules = runtime_rules("[DEPLOY]\nEnd=3\n", &[("DEPLOY", 3)]);
            let mut sim = Simulation::new();
            let owner_id = 100;
            let owner = sim.interner.intern("Americans");
            let type_ref = sim.interner.intern("UNIT");
            let mut entity = GameEntity::new_at_frame_zero_for_test(
                owner_id,
                10,
                10,
                0,
                0,
                owner,
                Health { current: 100 },
                type_ref,
                EntityCategory::Unit,
                0,
                5,
                true,
            );
            entity.set_unit_simple_deploy_for_test(false, true, false);
            sim.substrate.entities.insert(entity);
            let type_id = sim.interner.intern("DEPLOY");
            let other = sim
                .spawn_anim_object(&rules, runtime_descriptor(type_id, 0))
                .unwrap();
            let retained = sim
                .spawn_anim_object(&rules, runtime_descriptor(type_id, 0))
                .unwrap();
            for id in [other, retained] {
                sim.set_anim_owner_object(id, Some(owner_id), &rules);
            }
            sim.substrate
                .entities
                .get_mut(owner_id)
                .unwrap()
                .retain_deploy_anim(retained);
            for (id, expected) in [(other, Some(retained)), (retained, None)] {
                let cleared = if detach_with_coords {
                    sim.detach_anim_from_owner(id, &rules)
                } else {
                    sim.release_anim_owner_reference(id)
                };
                assert_eq!(cleared, Some(owner_id));
                assert_eq!(sim.anim(id).unwrap().owner_entity, None);
                let entity = sim.substrate.entities.get(owner_id).unwrap();
                assert_eq!(entity.deploy_anim(), expected);
                assert!(entity.unit_deploying());
                assert!(!entity.is_fully_deployed());
            }
        }
    }

    #[test]
    fn deployment_attachment_retirement_without_rules_keeps_absolute_sound_position() {
        let rules = runtime_rules("[DEPLOY]\nEnd=3\nStopSound=DeployDone\n", &[("DEPLOY", 3)]);
        let mut sim = Simulation::new();
        let owner_id = 100;
        let mut entity = GameEntity::test_default(owner_id, "UNIT", "Americans", 10, 10);
        entity.owner = sim.interner.intern("Americans");
        entity.type_ref = sim.interner.intern("UNIT");
        sim.substrate.entities.insert(entity);
        let type_id = sim.interner.intern("DEPLOY");
        let id = sim
            .spawn_anim_object(&rules, runtime_descriptor(type_id, 0))
            .unwrap();
        sim.set_anim_owner_object(id, Some(owner_id), &rules);
        let entity = sim.substrate.entities.get_mut(owner_id).unwrap();
        entity.retain_deploy_anim(id);
        entity.position.rx += 1;
        let absolute = sim.anim_absolute_coord(id).unwrap();
        assert_ne!(sim.anim(id).unwrap().world_coord, absolute);

        sim.destroy_anim_with_context(id, None);

        let anim = sim.anim(id).unwrap();
        assert_eq!(anim.world_coord, absolute);
        assert_eq!(anim.owner_entity, None);
        assert_eq!(
            sim.substrate.entities.get(owner_id).unwrap().deploy_anim(),
            None
        );
        assert!(sim.substrate.pending_delete.contains(&id));
        assert!(sim.sound_events.iter().any(|event| matches!(event,
            SimSoundEvent::AnimationStopped { anim_id, world, .. }
                if *anim_id == id && *world == absolute
        )));
    }

    #[test]
    fn native_anim_boundary_and_reset_vectors() {
        let golden: serde_json::Value =
            serde_json::from_str(crate::test_fixture::text("tools/anim_oracle/boundary.json"))
                .unwrap();
        let rules = runtime_rules("[TEST]\nEnd=64\n", &[("TEST", 64)]);
        let mut sim = Simulation::new();
        let type_id = sim.interner.intern("TEST");
        let id = sim
            .spawn_anim_object(&rules, runtime_descriptor(type_id, 0))
            .unwrap();
        let template = sim.anim(id).unwrap().clone();
        let mut config = rules.art().anim_runtime_config("TEST").unwrap().clone();
        let rows = golden["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 13_312);
        for row in rows {
            let name = row[0].as_str().unwrap();
            let bounds = golden["bounds"]
                .as_array()
                .unwrap()
                .iter()
                .find(|bounds| bounds[0].as_str() == Some(name))
                .unwrap();
            let mut anim = template.clone();
            config.start = bounds[1].as_i64().unwrap() as i32;
            config.loop_start = bounds[2].as_i64().unwrap() as i32;
            anim.effective_loop_end = bounds[3].as_i64().unwrap() as i32;
            anim.effective_end = bounds[4].as_i64().unwrap() as i32;
            anim.runtime.loop_remaining = row[1].as_u64().unwrap() as u8;
            anim.runtime.current_frame = row[2].as_i64().unwrap() as i32;
            config.shadow = row[3].as_u64().unwrap() != 0;
            config.reverse = row[4].as_u64().unwrap() != 0;
            anim.runtime.constructor_reverse = row[5].as_u64().unwrap() != 0;
            config.ping_pong = row[6].as_u64().unwrap() != 0;
            anim.runtime.frame_step = row[7].as_i64().unwrap() as i32;
            let expected = match row[8].as_str().unwrap() {
                "continue" => AnimBoundary::Continue,
                "bounce" => AnimBoundary::Bounce,
                "boundary_loop" => AnimBoundary::Loop,
                "boundary_terminal" => AnimBoundary::Complete,
                other => panic!("unknown native decision {other}"),
            };
            assert_eq!(advance_anim_boundary(&mut anim, &config), expected, "{row}");
            assert_eq!(
                anim.runtime.frame_step,
                row[9].as_i64().unwrap() as i32,
                "{row}"
            );
            assert_eq!(
                anim.runtime.loop_remaining,
                row[10].as_u64().unwrap() as u8,
                "{row}"
            );
            assert_eq!(
                anim.runtime.current_frame,
                row[11].as_i64().unwrap() as i32,
                "{row}"
            );
        }
    }

    #[test]
    fn combat_anim_shadow_endpoint_retires_in_runtime_and_survives_restore() {
        use crate::sim::runtime::{SimResources, SimRuntime};
        use crate::sim::snapshot::GameSnapshot;
        use crate::sim::world::TickLane;

        fn run(restore: bool) -> Vec<u64> {
            // Explicit ART override on the existing combat producer. Bounds
            // match native stock_FDHD rows; Rate=900/Normalized=no selects one
            // frame per visit for this fixture. This is not a claim that retail
            // NAMISL is produced by combat or that its Building host is bound.
            let rules = runtime_rules(
                "[TWLT036]\nStart=16\nLoopStart=0\nLoopEnd=32\nEnd=31\nShadow=yes\nRate=900\nNormalized=no\n\n\
                 [NEIGHBOR]\nEnd=64\nLoopCount=-1\nRate=900\nNormalized=no\n",
                &[("TWLT036", 64), ("NEIGHBOR", 64)],
            );
            // Native load resets Scenario RNG to seed zero. Keep this fixture
            // on that stream so endpoint continuation is comparable across load.
            let mut sim = Simulation::with_seed(0);
            sim.session.map_name = "anim-boundary".to_string();
            let explosion_type = sim.interner.intern("TWLT036");
            let neighbor_type = sim.interner.intern("NEIGHBOR");
            let explosion = sim
                .spawn_combat_explosion_anim(
                    &rules,
                    explosion_type,
                    7,
                    9,
                    crate::util::fixed_math::SIM_ZERO,
                    crate::util::fixed_math::SIM_ZERO,
                    0,
                    0,
                )
                .unwrap();
            let neighbor = sim
                .spawn_combat_explosion_anim(
                    &rules,
                    neighbor_type,
                    7,
                    9,
                    crate::util::fixed_math::SIM_ZERO,
                    crate::util::fixed_math::SIM_ZERO,
                    0,
                    0,
                )
                .unwrap();
            assert_eq!(sim.live_object_order_snapshot(), vec![explosion, neighbor]);
            assert_eq!(sim.anim(explosion).unwrap().runtime.rate_reload, 1);
            let mut resources = SimResources::empty();
            resources.rules = rules;
            let mut runtime = SimRuntime {
                simulation: sim,
                resources,
            };
            let mut hashes = Vec::new();
            for frame in 0..=17 {
                assert_eq!(runtime.simulation.session.binary_frame, frame);
                if restore && frame == 8 {
                    assert_eq!(
                        runtime.simulation.rng_state().scenario,
                        Simulation::with_seed(0).rng_state().scenario,
                        "endpoint fixture has consumed no Scenario RNG"
                    );
                    let before = runtime.simulation.state_hash();
                    let rules_hash = runtime.resources.rules.simulation_config_hash();
                    let bytes = GameSnapshot::save_validated(
                        &runtime.simulation,
                        0x1234,
                        rules_hash,
                        "Anim boundary fixture",
                        0,
                    );
                    let mut restored =
                        GameSnapshot::load_validated(&bytes, 0x1234, rules_hash, "anim-boundary")
                            .expect("valid live Anim snapshot")
                            .sim;
                    restored
                        .restore_after_snapshot_load()
                        .expect("live Anim references restore");
                    restored.retain_in_scenario_process_state_from(&runtime.simulation);
                    assert_eq!(restored.rng_state(), runtime.simulation.rng_state());
                    runtime.replace_simulation(restored);
                    assert_eq!(runtime.simulation.state_hash(), before);
                }
                let output = runtime
                    .advance_frame(&[], 67, TickLane::Ordinary)
                    .expect("fixture frame must complete");
                assert!(output.tick.frame_committed);
                if frame < 16 {
                    assert_eq!(
                        runtime
                            .simulation
                            .anim(explosion)
                            .unwrap()
                            .runtime
                            .current_frame,
                        frame as i32
                    );
                    assert_eq!(
                        runtime.simulation.live_object_order_snapshot(),
                        vec![explosion, neighbor]
                    );
                } else {
                    // Native golden stock_FDHD, loop1, stage16, Shadow1,
                    // reverse0/constructor_reverse0/pingpong0 is terminal.
                    // The actual live-vector pass removes the first object;
                    // its shifted neighbor is skipped until the next pass.
                    assert!(runtime.simulation.anim(explosion).is_none());
                    assert_eq!(
                        runtime.simulation.live_object_order_snapshot(),
                        vec![neighbor]
                    );
                }
                let neighbor_stage = if frame < 16 { frame } else { frame - 1 };
                assert_eq!(
                    runtime
                        .simulation
                        .anim(neighbor)
                        .unwrap()
                        .runtime
                        .current_frame,
                    neighbor_stage as i32
                );
                hashes.push(runtime.simulation.state_hash());
            }
            hashes
        }
        assert_eq!(run(false), run(true));
    }

    #[test]
    fn gsi_04_12_anim_make_infantry_ini_preserves_native_default_and_signed_value() {
        let rules = runtime_rules(
            "[DEFAULT]\nRate=900\nEnd=1\n\n[EXPLICIT]\nRate=900\nEnd=1\nMakeInfantry=-2\n",
            &[("DEFAULT", 1), ("EXPLICIT", 1)],
        );

        assert_eq!(
            rules
                .art()
                .anim_runtime_config("DEFAULT")
                .unwrap()
                .make_infantry,
            -1
        );
        assert_eq!(
            rules
                .art()
                .anim_runtime_config("EXPLICIT")
                .unwrap()
                .make_infantry,
            -2
        );
    }

    /// The whole point of routing combat explosions through `AnimStore`: the
    /// verified constructor row from `BulletClass::DetonateAtCoord 0x00469C93`,
    /// and the `Report=` that only `AnimClass::Start @ 0x00424CE0` can play.
    /// `AnimClass::AI @ 0x00423AC0`: a constructor delay keeps `Start` out of
    /// the constructor; the AI visit that counts the delay to zero calls
    /// `AnimClass::Start @ 0x00424CE0` (`0x004243A1`) and returns, so the
    /// `Report=` of a delayed anim plays exactly then, once.
    #[test]
    fn delayed_anim_plays_its_report_when_the_delay_expires() {
        let rules = runtime_rules(
            "[TWLT036]\nTranslucent=yes\nReport=Explosion06\nEnd=8\n",
            &[("TWLT036", 8)],
        );
        let mut sim = Simulation::new();
        let type_name = sim.interner.intern("TWLT036");
        let descriptor = AnimClassSpawnDescriptor {
            delay: 2,
            ..AnimClassSpawnDescriptor::new(
                type_name,
                7,
                9,
                crate::util::lepton::CELL_CENTER_LEPTON,
                crate::util::lepton::CELL_CENTER_LEPTON,
                0,
            )
        };
        let id = sim
            .spawn_anim_object(&rules, descriptor)
            .expect("bound art constructs");
        let starts = |sim: &Simulation| {
            sim.sound_events
                .iter()
                .filter(|event| {
                    matches!(event, SimSoundEvent::AnimationStarted { anim_id, .. } if *anim_id == id)
                })
                .count()
        };
        assert_eq!(
            starts(&sim),
            0,
            "a delayed constructor does not reach Start"
        );
        assert!(!sim.anim(id).unwrap().start_sound_active);

        // The first AI visit only clears the first-AI guard.
        sim.visit_anim(id, &rules, None);
        sim.visit_anim(id, &rules, None);
        assert_eq!(sim.anim(id).unwrap().runtime.delay_remaining, 1);
        assert_eq!(starts(&sim), 0);

        sim.visit_anim(id, &rules, None);
        assert_eq!(sim.anim(id).unwrap().runtime.delay_remaining, 0);
        assert_eq!(starts(&sim), 1, "the expiring visit runs Start");
        assert_eq!(
            sim.anim(id).unwrap().runtime.current_frame,
            0,
            "and returns before advancing a frame"
        );

        sim.visit_anim(id, &rules, None);
        assert_eq!(starts(&sim), 1, "Start runs once per delay");
    }

    #[test]
    fn combat_explosion_uses_the_verified_constructor_row_and_plays_report() {
        // TWLT036 is the stock AP-shell explosion: Report=Explosion06,
        // Translucent=yes, no Rate= (so one logic frame per image frame).
        let rules = runtime_rules(
            "[TWLT036]\nTranslucent=yes\nReport=Explosion06\nEnd=8\n",
            &[("TWLT036", 8)],
        );
        let mut sim = Simulation::new();
        let type_name = sim.interner.intern("TWLT036");

        let id = sim
            .spawn_combat_explosion_anim(
                &rules,
                type_name,
                7,
                9,
                crate::util::fixed_math::SimFixed::from_num(128),
                crate::util::fixed_math::SimFixed::from_num(64),
                3,
                3 * LEVEL_HEIGHT_LEPTONS,
            )
            .expect("bound explosion art constructs an AnimClass");

        let anim = sim.anim(id).expect("explosion is a registered AnimObject");
        assert_eq!(anim.draw_flags, COMBAT_EXPLOSION_DRAW_FLAGS);
        assert_eq!(anim.z_adjust, COMBAT_EXPLOSION_Z_ADJUST);
        assert_eq!(anim.runtime.delay_remaining, 0);
        assert!(!anim.runtime.constructor_reverse);
        assert_eq!(anim.runtime.current_frame, 0);
        assert_eq!(anim.runtime.loop_remaining, 1);
        assert_eq!(anim.owner_entity, None);

        // `delay == 0` means the constructor itself reached Start, so the
        // sound is already out before the first AI visit.
        assert!(anim.start_sound_active);
        assert!(
            sim.sound_events.iter().any(|event| matches!(
                event,
                SimSoundEvent::AnimationStarted { anim_id, sound_id, .. }
                    if *anim_id == id && sound_id == "EXPLOSION06"
            )),
            "explosion must emit its art `Report=`, got {:?}",
            sim.sound_events
        );

        // The level-keyed spawn coordinate decomposes back to the same cell,
        // sub-cell and level.
        let coord = sim.anim_absolute_coord(id).expect("absolute coordinate");
        assert_eq!(
            coord.to_cell_sub_z(),
            (
                7,
                9,
                crate::util::fixed_math::SimFixed::from_num(128),
                crate::util::fixed_math::SimFixed::from_num(64),
                3
            )
        );
    }

    /// A name with no registered AnimType declines the spawn rather than
    /// panic or block the shot. Retail's art-less names are registered: rules
    /// processing mints them as native does, and a minted type without an art
    /// section (`gtpowexp` and `tstlexp` from the power plants' `Explosion=`)
    /// keeps `End=0` because `AnimTypeClass::ReadINI @ 0x00427D00` bails
    /// before the image loader, so its anim is constructed and dies unseen
    /// (`ArtRegistry::bind_anim_class_assets`).
    #[test]
    fn combat_explosion_with_unbound_art_declines_instead_of_panicking() {
        let rules = runtime_rules("[TWLT036]\nEnd=8\n", &[("TWLT036", 8)]);
        let mut sim = Simulation::new();
        let missing = sim.interner.intern("MININUKE - ADDED 11/30");
        assert!(
            sim.spawn_combat_explosion_anim(
                &rules,
                missing,
                1,
                1,
                crate::util::fixed_math::SIM_ZERO,
                crate::util::fixed_math::SIM_ZERO,
                0,
                0,
            )
            .is_none()
        );
        assert_eq!(sim.substrate.anims.len(), 0);
        assert!(sim.sound_events.is_empty());
    }

    #[test]
    fn gsi_13_04_draw_and_terrain_attachment_state_roundtrip_and_hash() {
        let rules = runtime_rules("[DRAW]\nRate=900\nEnd=1\n", &[("DRAW", 1)]);
        let mut sim = Simulation::new();
        let mut descriptor = runtime_descriptor(sim.interner.intern("DRAW"), 0);
        descriptor.draw_runtime = AnimDrawRuntime {
            hidden: false,
            special_hidden: true,
            translucency_ramp: 6,
            forced_translucent: true,
            forced_uses_75: true,
        };
        descriptor.use_cell_drawer = true;
        descriptor.terrain_attached = true;
        let draw_runtime = descriptor.draw_runtime;
        let id = sim.spawn_anim_object(&rules, descriptor).unwrap();
        assert_eq!(sim.anim(id).unwrap().draw_runtime, draw_runtime);

        let serialized = bincode::serialize(&sim.substrate.anims).unwrap();
        let restored: AnimStore = bincode::deserialize(&serialized).unwrap();
        assert_eq!(restored.get(id).unwrap().draw_runtime, draw_runtime);
        assert!(restored.get(id).unwrap().use_cell_drawer);
        assert!(restored.get(id).unwrap().terrain_attached);

        let before = sim.state_hash();
        sim.anim_mut_by_id(id).unwrap().terrain_attached = false;
        assert_ne!(sim.state_hash(), before);
    }

    /// The store saves as the former derived newtype `AnimStore(map)` (the
    /// touch log is never saved), and every mutable hand-out is noted for the
    /// kept Ground sort keys.
    #[test]
    fn anim_store_saves_as_before_and_notes_every_hand_out() {
        #[derive(Serialize)]
        #[serde(rename = "AnimStore")]
        struct Former<'a>(&'a BTreeMap<AnimId, AnimObject>);
        let rules = runtime_rules("[DRAW]\nRate=900\nEnd=1\n", &[("DRAW", 1)]);
        let mut sim = Simulation::new();
        let descriptor = runtime_descriptor(sim.interner.intern("DRAW"), 0);
        let id = sim.spawn_anim_object(&rules, descriptor).unwrap();
        let store = &mut sim.substrate.anims;
        let former = Former(&store.anims);
        assert_eq!(
            bincode::serialize(&*store).unwrap(),
            bincode::serialize(&former).unwrap()
        );
        assert_eq!(
            serde_json::to_string(&*store).unwrap(),
            serde_json::to_string(&former).unwrap()
        );
        let bytes = bincode::serialize(&*store).unwrap();
        let restored: AnimStore = bincode::deserialize(&bytes).unwrap();
        assert_eq!(bincode::serialize(&restored).unwrap(), bytes);

        // Nothing has been derived from a fresh store yet.
        assert_eq!(store.take_touched(), Touched::All);
        assert!(store.get(id).is_some());
        assert_eq!(store.take_touched(), Touched::Ids(Vec::new()));
        store.get_mut(id).unwrap().z_adjust += 1;
        let removed = store.remove(id).unwrap();
        store.insert(removed);
        assert_eq!(store.take_touched(), Touched::Ids(vec![id]));
        assert_eq!(store.clone().take_touched(), Touched::All);
        let _ = store.values_mut().count();
        assert_eq!(store.take_touched(), Touched::All);
    }

    #[test]
    fn runtime_anim_retains_native_identity_and_spends_it_before_asset_failure() {
        let rules = runtime_rules("[LIVE]\nRate=900\nEnd=2\n", &[("LIVE", 2)]);
        let mut sim = Simulation::new();
        sim.native_unique_ids =
            Some(crate::sim::native_identity::NativeUniqueIdCursor::test_at_current_value(1000));
        let live = sim.interner.intern("LIVE");
        let missing = sim.interner.intern("UNBOUND");
        let rng = sim.scenario_rng.logical_state();
        assert!(
            sim.spawn_anim_object(&rules, runtime_descriptor(missing, 0))
                .is_err()
        );
        assert_eq!(sim.native_unique_ids.as_ref().unwrap().current_raw(), 1001);
        let id = sim
            .spawn_anim_object(&rules, runtime_descriptor(live, 0))
            .unwrap();
        assert_eq!(sim.anim(id).unwrap().native_unique_id, 1002);
        assert_ne!(id as i32, 1002, "stable handles have an independent owner");
        assert_eq!(sim.scenario_rng.logical_state(), rng);
    }

    #[test]
    fn preconsumed_anim_constructor_retains_identity_without_redrawing_or_reassigning() {
        let rules = runtime_rules(
            "[DEBRIS]\nRate=900\nRandomRate=50,150\nEnd=2\n",
            &[("DEBRIS", 2)],
        );
        let mut sim = Simulation::new();
        let type_id = sim.interner.intern("DEBRIS");
        let descriptor = runtime_descriptor(type_id, 1);
        let coord = AnimWorldCoord {
            x: 128,
            y: 128,
            z: 20,
        };
        let native_unique_id = sim.next_native_runtime_id();
        let mut draws = anim_constructor_draws(
            rules.art().anim_runtime_config("DEBRIS").unwrap(),
            coord,
            &mut sim.scenario_rng,
        )
        .unwrap();
        draws.native_unique_id = Some(native_unique_id);
        let after_draws = sim.scenario_rng.logical_state();
        let next_constructor = sim.next_native_runtime_id();
        let id = sim
            .spawn_anim_at_world_with_constructor(
                &rules,
                descriptor,
                coord,
                AnimConstructorInput::Preconsumed(draws),
            )
            .unwrap();
        assert_eq!(sim.anim(id).unwrap().native_unique_id, native_unique_id);
        assert_eq!(
            sim.native_unique_ids.as_ref().unwrap().current_raw() as i32,
            next_constructor
        );
        assert_eq!(sim.scenario_rng.logical_state(), after_draws);
    }

    #[test]
    fn authored_load_anim_retains_native_id_and_final_scalar_delete_rechecks_compaction() {
        let rules = runtime_rules(
            "[LOADTILE]\nRate=900\nEnd=2\nLoopCount=-1\nStartSound=TileStart\nStopSound=TileStop\n\n\
             [KEEP]\nRate=900\nEnd=2\nLoopCount=-1\n",
            &[("LOADTILE", 2), ("KEEP", 2)],
        );
        let mut sim = Simulation::new();
        let load_type = sim.interner.intern("LOADTILE");
        let keep_type = sim.interner.intern("KEEP");
        let world = AnimWorldCoord {
            x: 384,
            y: 640,
            z: 208,
        };
        let mut first = runtime_descriptor(load_type, 0);
        first.terrain_attached = true;
        first.use_cell_drawer = true;
        let first_id = sim
            .spawn_anim_at_world_with_constructor(
                &rules,
                first,
                world,
                AnimConstructorInput::AssignedIdentity(1_010_001),
            )
            .expect("first authored load Anim");
        let keep_id = sim
            .spawn_anim_object(&rules, runtime_descriptor(keep_type, 0))
            .expect("ordinary retained Anim");
        let mut second = runtime_descriptor(load_type, 0);
        second.terrain_attached = true;
        second.use_cell_drawer = true;
        let second_id = sim
            .spawn_anim_at_world_with_constructor(
                &rules,
                second,
                world,
                AnimConstructorInput::AssignedIdentity(1_010_002),
            )
            .expect("second authored load Anim");

        assert_eq!(sim.anim(first_id).unwrap().native_unique_id, 1_010_001);
        assert_eq!(sim.anim(second_id).unwrap().native_unique_id, 1_010_002);
        assert!(sim.anim(first_id).unwrap().start_sound_active);

        assert_eq!(sim.scalar_delete_load_terrain_anims(), 2);
        assert!(sim.anim(first_id).is_none());
        assert!(sim.anim(second_id).is_none());
        assert!(sim.anim(keep_id).is_some());
        assert_eq!(
            sim.sound_events
                .iter()
                .filter(|event| matches!(event, SimSoundEvent::ObjectSoundReleased { .. }))
                .count(),
            2,
            "final Init scalar deletion suppresses configured StopSound identity"
        );
    }

    #[test]
    fn gsi_04_12_anim_make_infantry_marks_before_first_ai_and_clears_on_natural_end() {
        let rules = runtime_rules(
            "[GENDEATH]\nRate=900\nEnd=1\nLoopCount=1\nMakeInfantry=0\n",
            &[("GENDEATH", 1)],
        );
        let mut sim = Simulation::new();
        let type_id = sim.interner.intern("GENDEATH");
        let mut descriptor = runtime_descriptor(type_id, 0);
        descriptor.rx = 3;
        descriptor.ry = 4;
        descriptor.sub_x = SimFixed::from_num(192);
        descriptor.sub_y = SimFixed::from_num(64);
        let id = sim.spawn_anim_object(&rules, descriptor).unwrap();

        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
        sim.visit_anim(id, &rules, None);
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(3, 4),
            0x04,
            "the first AI guard runs after MakeInfantry raw marking"
        );
        assert_eq!(sim.anim(id).unwrap().runtime.current_frame, 0);

        sim.session.binary_frame = 1;
        sim.visit_anim(id, &rules, None);

        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
        assert!(sim.substrate.pending_delete.contains(&id));
    }

    #[test]
    fn gsi_04_12_anim_make_infantry_next_and_early_destroy_leave_destructive_mark_stale() {
        let rules = runtime_rules(
            "[GENDEATH]\nRate=900\nEnd=1\nLoopCount=1\nMakeInfantry=0\nNext=PLAIN\n\n\
             [PLAIN]\nRate=900\nEnd=1\nLoopCount=1\n",
            &[("GENDEATH", 1), ("PLAIN", 1)],
        );
        let mut sim = Simulation::new();
        let gen_type = sim.interner.intern("GENDEATH");
        let plain = sim.interner.intern("PLAIN");
        let mut descriptor = runtime_descriptor(gen_type, 0);
        descriptor.rx = 5;
        descriptor.ry = 6;
        descriptor.sub_x = SimFixed::from_num(64);
        descriptor.sub_y = SimFixed::from_num(192);
        let id = sim.spawn_anim_object(&rules, descriptor).unwrap();

        sim.visit_anim(id, &rules, None);
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(5, 6), 0x08);
        sim.session.binary_frame = 1;
        sim.visit_anim(id, &rules, None);

        assert_eq!(sim.anim(id).unwrap().type_id, plain);
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(5, 6),
            0x08,
            "Next takes priority and performs no MakeInfantry clear"
        );
        sim.destroy_anim(id, &rules);
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(5, 6),
            0x08,
            "generic Anim destruction does not repair the raw byte"
        );
    }

    #[test]
    fn gsi_04_12_anim_make_infantry_mark_and_clear_keep_native_bridge_asymmetry() {
        let mut grid = RawCellOccupationGrid::new();

        apply_anim_raw_occupation(
            &mut grid,
            RawCellKey::Real(7, 8),
            0x10,
            416,
            0,
            true,
            AnimOccupationOperation::Mark,
        );
        assert_eq!(grid.ground_bits(7, 8), 0);
        assert_eq!(grid.deck_bits(7, 8), 0x10);
        apply_anim_raw_occupation(
            &mut grid,
            RawCellKey::Real(7, 8),
            0x10,
            416,
            0,
            false,
            AnimOccupationOperation::Clear,
        );
        assert_eq!(grid.deck_bits(7, 8), 0);

        apply_anim_raw_occupation(
            &mut grid,
            RawCellKey::Real(7, 8),
            0x10,
            416,
            0,
            false,
            AnimOccupationOperation::Mark,
        );
        assert_eq!(grid.ground_bits(7, 8), 0x10);
        apply_anim_raw_occupation(
            &mut grid,
            RawCellKey::Real(7, 8),
            0x10,
            416,
            0,
            false,
            AnimOccupationOperation::Clear,
        );
        assert_eq!(
            grid.ground_bits(7, 8),
            0x10,
            "height-only clear targets deck after a nonstructural ground mark"
        );
        assert_eq!(grid.deck_bits(7, 8), 0);
    }

    #[test]
    fn make_infantry_occupation_reads_constructor_side_flags_without_sprite_state() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/bridge_constructor.json",
        ))
        .unwrap();
        // Supply original5FC380's completed cell fields at the Anim reader
        // boundary. The constructor's common TMP fixture has physical level6.
        for case in &corpus["cases"].as_array().unwrap()[8..12] {
            let native = case["cells"]
                .as_array()
                .unwrap()
                .iter()
                .find(|cell| cell["flags"].as_u64().unwrap() & 0x100 != 0 && cell["overlay"] == -1)
                .unwrap();
            let x = native["coord"][0].as_u64().unwrap() as u16;
            let y = native["coord"][1].as_u64().unwrap() as u16;
            let mut terrain = crate::map::resolved_terrain::bridge_constructor_terrain();
            let cell = terrain.native_cell_identity((x as i16, y as i16));
            let flags = native["flags"].as_u64().unwrap() as u32;
            terrain.write_native_cell_flags(cell, flags);
            terrain.write_native_cell_state(cell, native["state"].as_u64().unwrap() as u8);
            assert_eq!(terrain.cell(x, y).unwrap().bridge_facts.overlay_id, None);
            let mut sim = Simulation::new();
            sim.resolved_terrain = Some(terrain);
            assert!(sim.bridge_state.is_none());
            let world = AnimWorldCoord {
                x: i32::from(x) * 256 + 128,
                y: i32::from(y) * 256 + 128,
                z: 1040,
            };
            // Original Infantry5217C0's center (128,128) occupies slot0:
            // walk_head_occupation.json raw[94]/[98], deck8 -> deck9.
            sim.apply_make_infantry_raw_occupation(world, AnimOccupationOperation::Mark);
            assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(x, y), 0);
            assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(x, y), 0x01);

            sim.resolved_terrain
                .as_mut()
                .unwrap()
                .write_native_cell_flags(cell, flags & !0x100);
            sim.apply_make_infantry_raw_occupation(world, AnimOccupationOperation::Clear);
            assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(x, y), 0);
            sim.apply_make_infantry_raw_occupation(world, AnimOccupationOperation::Mark);
            assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(x, y), 0x01);
            assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(x, y), 0);
        }
    }

    /// Map[coord] (`0x00565730`) truncates toward zero and wraps by row. A
    /// MakeInfantry anim less than a cell left of the map edge occupies cell
    /// 0 of its row; a cell further left it wraps to slot 511 of the row
    /// above, off this map, and past slot 511 it wraps to the next row.
    #[test]
    fn a_make_infantry_anim_left_of_the_map_edge_occupies_cell_zero() {
        const SIZE: u16 = 8;
        let mut sim = Simulation::new();
        sim.resolved_terrain = Some(
            crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
                SIZE,
                SIZE,
                (0..SIZE)
                    .flat_map(|y| {
                        (0..SIZE).map(move |x| crate::map::resolved_terrain::test_flat_cell(x, y))
                    })
                    .collect(),
            ),
        );
        // Sub-cell (128, 128) is slot 0.
        let world = AnimWorldCoord {
            x: -128,
            y: 5 * 256 + 128,
            z: 0,
        };
        sim.apply_make_infantry_raw_occupation(world, AnimOccupationOperation::Mark);
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(0, 5), 0x01);
        sim.apply_make_infantry_raw_occupation(world, AnimOccupationOperation::Clear);
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(0, 5), 0);

        assert_eq!(sim.anim_map_cell(-128 - 256, world.y), None);
        assert_eq!(sim.anim_map_cell(512 * 256 + 128, world.y), Some((0, 6)));
    }

    /// `AnimClass::MarkCellOccupancy` (`0x00426270`) stores the anim's owner
    /// index, -1, beside the bit it sets (`0x004262D0`), and
    /// `ClearCellOccupancy` (`0x00426300`) resets it only once `0x1C` is clear.
    /// Off the map both write the shared dummy cell's planes.
    #[test]
    fn a_make_infantry_anim_writes_owner_minus_one_on_a_real_or_dummy_cell() {
        const SIZE: u16 = 8;
        let mut sim = Simulation::new();
        sim.resolved_terrain = Some(
            crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
                SIZE,
                SIZE,
                (0..SIZE)
                    .flat_map(|y| {
                        (0..SIZE).map(move |x| crate::map::resolved_terrain::test_flat_cell(x, y))
                    })
                    .collect(),
            ),
        );
        let house = InternedId::from_index(41);
        let ground = MovementLayer::Ground;
        // An infantryman in slot 2 leaves its house beside the ground byte;
        // the anim at the centre marks slot 0.
        sim.substrate
            .raw_cell_occupation
            .mark_ground_infantry(3, 3, 1 << 2, house);
        let on_map = AnimWorldCoord {
            x: 3 * 256 + 128,
            y: 3 * 256 + 128,
            z: 0,
        };
        sim.apply_make_infantry_raw_occupation(on_map, AnimOccupationOperation::Mark);
        let raw = &sim.substrate.raw_cell_occupation;
        assert_eq!(raw.ground_bits(3, 3), 0x05);
        assert_eq!(raw.infantry_owner(3, 3, ground), None);
        sim.apply_make_infantry_raw_occupation(on_map, AnimOccupationOperation::Clear);
        let raw = &sim.substrate.raw_cell_occupation;
        assert_eq!(raw.ground_bits(3, 3), 0x04);
        assert_eq!(raw.infantry_owner(3, 3, ground), None);

        // A cell left of column 0 wraps to slot 511 of the row above: the
        // shared dummy on this map.
        sim.substrate.raw_cell_occupation.write_occupant(
            RawCellKey::Dummy,
            ground,
            1 << 3,
            Some(house),
            true,
        );
        let off_map = AnimWorldCoord {
            x: -384,
            y: 3 * 256 + 128,
            z: 0,
        };
        sim.apply_make_infantry_raw_occupation(off_map, AnimOccupationOperation::Mark);
        let raw = &sim.substrate.raw_cell_occupation;
        assert_eq!(raw.bits_at(RawCellKey::Dummy, ground), 0x09);
        assert_eq!(raw.owner_at(RawCellKey::Dummy, ground), None);
        sim.apply_make_infantry_raw_occupation(off_map, AnimOccupationOperation::Clear);
        let raw = &sim.substrate.raw_cell_occupation;
        assert_eq!(raw.bits_at(RawCellKey::Dummy, ground), 0x08);
        assert_eq!(raw.owner_at(RawCellKey::Dummy, ground), None);
    }

    /// `AnimClass::AnimClass @ 0x00421EA0`'s `RandomRate=` pick and `Bouncer=`
    /// launch (`tools/spatial_oracle/anim_bouncer_launch.py`, the whole
    /// constructor executed): the rate draw, the three velocity draws and
    /// Init's three, state for state, and the body's position, velocity,
    /// elasticity, gravity and clamp bits. `RandomRate=` goes through the
    /// production reader; the oracle writes the three doubles into the type
    /// directly, so they are set exactly (`ReadDouble` would round them
    /// through `%f`; the stock values are integers either way).
    #[test]
    fn bouncer_launch_matches_the_original() {
        let golden: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/anim_bouncer_launch.json",
        ))
        .unwrap();
        let rows = golden["ctor"].as_array().unwrap();
        assert!(rows.len() >= 150);
        let mut assigned_identity_rows = 0;
        for row in rows {
            let input = &row["input"];
            let rate = input["random_rate"]
                .as_array()
                .map(|pair| format!("RandomRate={},{}\n", pair[0], pair[1]))
                .unwrap_or_default();
            let art =
                ArtRegistry::from_ini(&IniFile::from_str(&format!("[T]\nBouncer=yes\n{rate}")));
            let mut config = art.anim_runtime_config("T").unwrap().clone();
            let exact =
                |key: &str| NativeF64Bits::from_bits(input[key].as_f64().unwrap().to_bits());
            config.elasticity = exact("elasticity");
            config.max_xy_vel = exact("max_xy");
            config.min_z_vel = exact("min_z");
            let coord = input["coord"].as_array().unwrap();
            let at = |i: usize| coord[i].as_i64().unwrap() as i32;
            let mut rng = crate::sim::rng::SimRng::new(input["seed"].as_u64().unwrap());
            assert_eq!(
                rng.native_state_hex(),
                row["rng_before"].as_str().unwrap(),
                "{input}"
            );
            let draws = anim_constructor_draws(
                &config,
                AnimWorldCoord {
                    x: at(0),
                    y: at(1),
                    z: at(2),
                },
                &mut rng,
            )
            .unwrap();
            assert_eq!(
                rng.native_state_hex(),
                row["rng_after"].as_str().unwrap(),
                "{input}"
            );
            let rate_pick = row["events"]
                .as_array()
                .unwrap()
                .iter()
                .find(|event| event["site"] == "0x004221F5")
                .map(|event| event["result"].as_u64().unwrap() as u16);
            assert_eq!(draws.random_rate, rate_pick, "{input}");
            let body = draws.bounce.expect("a Bouncer= type launches");
            let native = &row["bounce"];
            let bits = |key: &str, i: usize| native[key][i].as_u64().unwrap() as u32;
            for axis in 0..3 {
                assert_eq!(
                    body.position[axis].bits(),
                    bits("position_bits", axis),
                    "{input}"
                );
                assert_eq!(
                    body.velocity[axis].bits(),
                    bits("velocity_bits", axis),
                    "{input}"
                );
            }
            assert_eq!(
                body.elasticity.bits(),
                native["elasticity_bits"].as_u64().unwrap(),
                "{input}"
            );
            assert_eq!(
                body.gravity.bits(),
                native["gravity_bits"].as_u64().unwrap(),
                "{input}"
            );
            assert_eq!(
                body.angular_velocity_magnitude.bits(),
                native["clamp_bits"].as_u64().unwrap(),
                "{input}"
            );
            // Exercise the assigned-identity constructor through the real ART
            // reader wherever its %f conversion preserves the supplied native
            // doubles. The body above was compared to the executed constructor,
            // so the former fresh-load/live-slot entry must retain that same
            // body and full RNG state, rather than taking only RandomRate.
            if ["elasticity", "max_xy", "min_z"].iter().all(|key| {
                let value = input[*key].as_f64().unwrap();
                f64::from(value as f32) == value
            }) {
                assigned_identity_rows += 1;
                let rules = runtime_rules(
                    &format!(
                        "[T]\nBouncer=yes\n{rate}Elasticity={}\nMaxXYVel={}\nMinZVel={}\nEnd=2\n",
                        input["elasticity"], input["max_xy"], input["min_z"],
                    ),
                    &[("T", 2)],
                );
                let mut sim = Simulation::with_seed(input["seed"].as_u64().unwrap());
                let type_id = sim.interner.intern("T");
                let native_id = sim.next_native_runtime_id();
                let id = sim
                    .spawn_anim_at_world_with_constructor(
                        &rules,
                        runtime_descriptor(type_id, 0),
                        AnimWorldCoord {
                            x: at(0),
                            y: at(1),
                            z: at(2),
                        },
                        AnimConstructorInput::AssignedIdentity(native_id),
                    )
                    .unwrap();
                assert_eq!(
                    sim.scenario_rng.native_state_hex(),
                    row["rng_after"].as_str().unwrap(),
                    "{input}: assigned-identity constructor draws"
                );
                let anim = sim.anim(id).unwrap();
                assert_eq!(anim.native_unique_id, native_id);
                assert_eq!(
                    sim.native_unique_ids.as_ref().unwrap().current_raw() as i32,
                    native_id
                );
                assert_eq!(
                    anim.bounce,
                    Some(body),
                    "{input}: assigned-identity native body"
                );
            }
        }
        assert_eq!(assigned_identity_rows, 134);
    }

    /// A stock-shaped debris chunk (`DBRIS1LG` numbers, `MaxXYVel` cut to 0.5
    /// so it lands in its own cell) launched over a flat arena beside a tank:
    /// it flies its body, then on touching down (`BounceClass::Update`
    /// reports Stopped on flat ground) constructs its `ExpireAnim=`, blasts
    /// the tank through `Apply_area_damage` with its HE `Damage=`, and is
    /// destroyed. Its Location follows the body every tick.
    #[test]
    fn a_debris_chunk_lands_and_blasts_what_is_under_it() {
        let mut rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nDamageFireTypes=\n\
             [AudioVisual]\nConditionYellow=50%\nConditionRed=25%\n\
             [InfantryTypes]\n[VehicleTypes]\n0=TANK\n[AircraftTypes]\n[BuildingTypes]\n\
             [TANK]\nStrength=300\nArmor=heavy\nSpeed=4\n\
             [Warheads]\n0=HE\n\
             [HE]\nCellSpread=.5\nPercentAtMax=.5\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .unwrap();
        let mut art = ArtRegistry::from_ini(&IniFile::from_str(
            "[CHUNK]\nBouncer=yes\nElasticity=0.0\nMaxXYVel=0.5\nMinZVel=25.0\n\
             ExpireAnim=BOOM\nDamage=20\nDamageRadius=80\nWarhead=HE\n\
             LoopEnd=15\nLoopCount=-1\nRandomRate=220,600\n\
             [BOOM]\nRate=900\n",
        ));
        art.bind_anim_frame_count_for_test("CHUNK", 30);
        art.bind_anim_frame_count_for_test("BOOM", 17);
        rules.replace_art_registry_for_test(art);
        let mut sim = Simulation::with_seed(3);
        let americans = sim.interner.intern("Americans");
        sim.houses.insert(
            americans,
            crate::sim::house_state::HouseState::new(americans, 0, None, true, 0, 10),
        );
        sim.session.house_order.push(americans);
        crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        let tank = sim
            .spawn_object_at_height("TANK", "Americans", 8, 8, 0, 0, &rules)
            .unwrap();
        let chunk_type = sim.interner.intern("CHUNK");
        let center = crate::util::lepton::CELL_CENTER_LEPTON;
        let chunk = sim
            .spawn_anim_at_world(
                &rules,
                AnimClassSpawnDescriptor {
                    delay: 0,
                    loop_count: 1,
                    draw_flags: 0x600,
                    z_adjust: 0,
                    reverse: false,
                    ..AnimClassSpawnDescriptor::new(chunk_type, 8, 8, center, center, 0)
                },
                AnimWorldCoord {
                    x: 8 * 256 + 128,
                    y: 8 * 256 + 128,
                    z: 20,
                },
            )
            .unwrap();
        let launch = sim
            .anim(chunk)
            .unwrap()
            .bounce
            .expect("a Bouncer= chunk has a body");
        assert_eq!(
            launch.position_leptons().z,
            30,
            "10 leptons above its coordinate"
        );

        let mut landed_at = None;
        for frame in 1..200 {
            sim.session.binary_frame = frame;
            sim.visit_anim(chunk, &rules, None);
            let anim = sim.anim(chunk).unwrap();
            let body = anim.bounce.unwrap().position_leptons();
            assert_eq!(
                [anim.world_coord.x, anim.world_coord.y, anim.world_coord.z],
                [body.x, body.y, body.z]
            );
            if sim.substrate.pending_delete.contains(&chunk) {
                landed_at = Some(body);
                break;
            }
        }
        let landed_at = landed_at.expect("the chunk comes down");
        assert_eq!(landed_at.z, 0, "it rests on the flat ground");
        assert_eq!((landed_at.x >> 8, landed_at.y >> 8), (8, 8));
        let boom = sim.interner.intern("BOOM");
        assert_eq!(
            sim.anims()
                .filter(|(_, anim)| anim.type_id == boom)
                .map(|(_, anim)| [anim.world_coord.x, anim.world_coord.y, anim.world_coord.z])
                .collect::<Vec<_>>(),
            vec![[landed_at.x, landed_at.y, landed_at.z]]
        );
        let health = sim.substrate.entities.get(tank).unwrap().health.current;
        assert!(
            health < 300 && health >= 280,
            "HE splash of 20 at most, got {health}"
        );
        assert_eq!(sim.combat_light_requests.len(), 1);
    }

    /// A world whose flat 8x8 map takes a 1x1 crater and a 1x1 scorch.
    fn middle_world(art_text: &str, frames: i32) -> (RuleSet, Simulation) {
        let mut rules = runtime_rules(art_text, &[("MARK", frames)]);
        rules.smudge_types =
            crate::rules::smudge_type::SmudgeTypeRegistry::from_rules_ini(&IniFile::from_str(
                "[SmudgeTypes]\n0=CR1\n1=BURN1\n\
                 [CR1]\nCrater=yes\nWidth=1\nHeight=1\n\
                 [BURN1]\nBurn=yes\nWidth=1\nHeight=1\n",
            ));
        let mut sim = Simulation::new();
        let mut terrain = crate::map::resolved_terrain::test_flat_ground_grid(8);
        for ry in 0..8 {
            for rx in 0..8 {
                terrain.cell_mut(rx, ry).unwrap().accepts_smudge = true;
            }
        }
        sim.resolved_terrain = Some(terrain);
        sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(8, 8));
        sim.smudge_grid = Some(crate::sim::smudge_grid::SmudgeGrid::new(8, 8));
        (rules, sim)
    }

    fn spawn_mark(sim: &mut Simulation, rules: &RuleSet, world_z: i32) -> AnimId {
        let name = sim.interner.intern("MARK");
        let center = crate::util::lepton::CELL_CENTER_LEPTON;
        sim.spawn_combat_explosion_anim(rules, name, 4, 4, center, center, 0, world_z)
            .unwrap()
    }

    fn marks_placed(sim: &Simulation) -> usize {
        sim.smudge_grid.as_ref().unwrap().iter_occupied().count()
    }

    /// `AnimClass::AI 0x0042465D`: a 13-frame crater anim (S_CLSN22, the
    /// AP warhead's large AnimList entry) marks the ground once, on the
    /// visit that commits stage 6 (`13 / 2`), not at construction.
    #[test]
    fn a_crater_anim_marks_the_ground_at_its_middle_frame() {
        let (rules, mut sim) = middle_world("[MARK]\nRate=900\nCrater=yes\n", 13);
        let id = spawn_mark(&mut sim, &rules, 0);
        assert_eq!(
            marks_placed(&sim),
            0,
            "Start leaves a 13-frame anim to its middle frame"
        );
        let mut saw_middle = false;
        for frame in 1..40 {
            sim.session.binary_frame = frame;
            sim.visit_anim(id, &rules, None);
            let Some(anim) = sim.anim(id) else {
                break;
            };
            let stage = anim.runtime.current_frame;
            saw_middle |= stage == 6;
            assert_eq!(marks_placed(&sim), usize::from(stage >= 6), "stage {stage}");
        }
        assert!(saw_middle);
        assert_eq!(marks_placed(&sim), 1);
        assert_eq!(
            sim.smudge_grid.as_ref().unwrap().cell(4, 4).type_id,
            Some(0)
        );
    }

    /// `AnimClass::Middle 0x00425057`: an anim 30 leptons or more above the
    /// ground leaves no mark; 29 still marks.
    #[test]
    fn an_anim_thirty_leptons_up_leaves_no_mark() {
        for (world_z, expected) in [(30, 0), (29, 1)] {
            let (rules, mut sim) = middle_world("[MARK]\nRate=900\nScorch=yes\n", 13);
            let id = spawn_mark(&mut sim, &rules, world_z);
            for frame in 1..40 {
                sim.session.binary_frame = frame;
                sim.visit_anim(id, &rules, None);
            }
            assert_eq!(marks_placed(&sim), expected, "z {world_z}");
        }
    }

    /// `AnimClass::Start 0x00424D48`: a type without a middle frame (one raw
    /// frame, so `+0x298` is 0) marks at Start, before any AI visit.
    #[test]
    fn a_single_frame_anim_marks_at_start() {
        let (rules, mut sim) = middle_world("[MARK]\nRate=900\nScorch=yes\n", 1);
        spawn_mark(&mut sim, &rules, 0);
        assert_eq!(marks_placed(&sim), 1);
        assert_eq!(
            sim.smudge_grid.as_ref().unwrap().cell(4, 4).type_id,
            Some(1)
        );
    }

    #[test]
    fn delay_guard_and_rate_use_passive_frame_anchor() {
        let rules = runtime_rules("[TEST]\nRate=450\nEnd=3\nLoopCount=1\n", &[("TEST", 3)]);
        let mut sim = Simulation::new();
        let type_id = sim.interner.intern("TEST");
        let rng_before = sim.scenario_rng.logical_state();
        let id = sim
            .spawn_anim_object(&rules, runtime_descriptor(type_id, 1))
            .unwrap();

        sim.visit_anim(id, &rules, None); // constructor first-AI guard at frame 0
        sim.session.binary_frame = 1;
        sim.visit_anim(id, &rules, None); // delay 1 -> 0
        assert_eq!(sim.anim(id).unwrap().runtime.current_frame, 0);
        sim.session.binary_frame = 2;
        sim.visit_anim(id, &rules, None); // constructor-anchored timer is already due

        assert_eq!(sim.anim(id).unwrap().runtime.current_frame, 1);
        sim.visit_anim(id, &rules, None); // a second visit in the same frame cannot advance again
        assert_eq!(sim.anim(id).unwrap().runtime.current_frame, 1);
        assert_eq!(sim.scenario_rng.logical_state(), rng_before);
    }

    #[test]
    fn normalized_rate_uses_live_speed_after_random_rate_selection() {
        let rules = runtime_rules(
            "[TEST]\nRate=900\nRandomRate=180,225\nNormalized=yes\nEnd=3\nLoopCount=1\n",
            &[("TEST", 3)],
        );
        let mut sim = Simulation::new();
        sim.session.game_options.game_speed = 1;
        let type_id = sim.interner.intern("TEST");
        let expected_rng = sim.scenario_rng.clone();
        let expected_rate = sim.session.game_options.normalized_anim_delay(4);

        let id = sim
            .spawn_anim_object(&rules, runtime_descriptor(type_id, 0))
            .unwrap();
        let runtime = &sim.anim(id).unwrap().runtime;

        assert_eq!(runtime.rate_reload, expected_rate);
        assert_eq!(runtime.frame_timer.start_frame(), 0);
        assert_eq!(runtime.frame_timer.duration(), i32::from(expected_rate));
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state()
        );
    }

    #[test]
    fn stock_naweap_a_rate_normalizes_to_six_frames_at_speed_one() {
        let rules = runtime_rules(
            "[NAWEAP_A]\nRate=200\nNormalized=yes\nEnd=12\nLoopCount=-1\n",
            &[("NAWEAP_A", 12)],
        );
        let mut sim = Simulation::new();
        sim.session.game_options.game_speed = 1;
        let type_id = sim.interner.intern("NAWEAP_A");
        let id = sim
            .spawn_anim_object(&rules, runtime_descriptor(type_id, 0))
            .unwrap();

        assert_eq!(sim.anim(id).unwrap().runtime.rate_reload, 6);
        sim.visit_anim(id, &rules, None);
        sim.session.binary_frame = 5;
        sim.visit_anim(id, &rules, None);
        assert_eq!(sim.anim(id).unwrap().runtime.current_frame, 0);
        sim.session.binary_frame = 6;
        sim.visit_anim(id, &rules, None);
        assert_eq!(sim.anim(id).unwrap().runtime.current_frame, 1);
    }

    #[test]
    fn next_without_preintern_reuses_identity_runs_middle_and_destroy_is_idempotent() {
        let rules = runtime_rules(
            "[FIRST]\nRate=900\nEnd=2\nLoopCount=1\nNext=SECOND\nStartSound=FirstStart\n\n\
             [SECOND]\nRate=900\nEnd=2\nLoopCount=1\nReport=SecondReport\nStopSound=SecondStop\n",
            &[("FIRST", 2), ("SECOND", 2)],
        );
        let mut sim = Simulation::new();
        let first = sim.interner.intern("FIRST");
        assert!(sim.interner.get("SECOND").is_none());
        let id = sim
            .spawn_anim_object(&rules, runtime_descriptor(first, 0))
            .unwrap();
        assert!(matches!(
            sim.sound_events.as_slice(),
            [SimSoundEvent::AnimationStarted { anim_id, .. }] if *anim_id == id
        ));

        sim.visit_anim(id, &rules, None); // guard
        sim.session.binary_frame = 1;
        sim.visit_anim(id, &rules, None); // frame 1
        sim.session.binary_frame = 2;
        sim.visit_anim(id, &rules, None); // frame 2 -> SECOND in place + Middle
        let anim = sim.anim(id).unwrap();
        assert_eq!(sim.interner.resolve(anim.type_id), "SECOND");
        assert_eq!(anim.runtime.current_frame, 0);
        assert_eq!(
            sim.sound_events
                .iter()
                .filter(|event| matches!(event, SimSoundEvent::AnimationStarted { .. }))
                .count(),
            2,
        );

        sim.session.binary_frame = 3;
        sim.visit_anim(id, &rules, None); // SECOND frame 1 (Next does not restore guard)
        sim.session.binary_frame = 4;
        sim.visit_anim(id, &rules, None); // SECOND frame 2 -> destroy
        sim.destroy_anim(id, &rules);
        assert!(!sim.anim(id).unwrap().runtime.inactive);
        assert!(sim.substrate.pending_delete.contains(&id));
        assert!(!sim.live_object_order_snapshot().contains(&id));
        assert_eq!(
            sim.sound_events
                .iter()
                .filter(|event| matches!(event, SimSoundEvent::ObjectSoundReleased { .. }))
                .count(),
            1,
        );
    }

    #[test]
    fn destroy_releases_report_before_optional_stop_sound_once() {
        let rules = runtime_rules(
            "[A]\nReport=StartCue\nStopSound=EndCue\nEnd=2\nLoopCount=1\n",
            &[("A", 2)],
        );
        let mut sim = Simulation::new();
        let type_id = sim.interner.intern("A");
        let id = sim
            .spawn_anim_object(&rules, runtime_descriptor(type_id, 0))
            .unwrap();
        let world = sim.anim_absolute_coord(id).unwrap();
        let stop = sim.interner.intern("ENDCUE");
        sim.sound_events.clear();
        sim.destroy_anim(id, &rules);
        sim.destroy_anim(id, &rules);
        // Original4255D5 releases +1A0 before425618 plays the optional
        // StopSound. Repeated queued Destroy cannot stop the released cue.
        assert!(matches!(sim.sound_events.as_slice(), [
            SimSoundEvent::ObjectSoundReleased { owner },
            SimSoundEvent::AnimationStopped { anim_id, stop_sound_id: Some(sound), world: at },
        ] if *owner == id && *anim_id == id && *sound == stop && *at == world));
        // UnInit4255B0/UnInit5F65F0 retire the receiver without changing19B.
        // The retained physical identity cannot be revealed before the drain.
        assert!(!sim.anim(id).unwrap().runtime.inactive);
        assert_eq!(sim.substrate.pending_delete, vec![id]);
        assert!(!sim.anim(id).unwrap().in_logic_vector);
        assert_eq!(sim.display_layers().layer_of(id), None);
        assert!(!sim.reveal_anim(id, Some(&rules)));
        sim.process_pending_delete();
        assert!(sim.anim(id).is_none());
    }

    #[test]
    fn trailer_without_preintern_is_visited_and_guarded_in_same_live_walk() {
        let rules = runtime_rules(
            "[PARENT]\nRate=0\nEnd=2\nTrailerAnim=CHILD\nTrailerSeperation=1\n\n\
             [CHILD]\nRate=900\nEnd=2\nLoopCount=1\n",
            &[("PARENT", 2), ("CHILD", 2)],
        );
        let mut sim = Simulation::new();
        let parent_type = sim.interner.intern("PARENT");
        assert!(sim.interner.get("CHILD").is_none());
        let parent = sim
            .spawn_anim_object(&rules, runtime_descriptor(parent_type, 0))
            .unwrap();

        sim.for_each_live_object(|sim, id| {
            sim.visit_anim(id, &rules, None);
        });

        let order = sim.live_object_order_snapshot();
        assert_eq!(order.len(), 2);
        assert_eq!(order[0], parent);
        let child = sim.anim(order[1]).unwrap();
        assert_eq!(sim.interner.resolve(child.type_id), "CHILD");
        assert!(!child.runtime.first_ai_guard);
        assert_eq!(child.runtime.current_frame, 0);
    }

    #[test]
    fn owner_expiry_marks_anim_inactive_until_its_next_ai_visit() {
        let (mut sim, rules, building_id) = damage_fire_fixture(false);
        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .health
            .current = 50;
        sim.update_building_damage_fire(building_id, &rules);
        let anim_id = sim
            .substrate
            .entities
            .get(building_id)
            .unwrap()
            .damage_fire_anim_ids[0]
            .unwrap();
        sim.set_anim_owner_object(anim_id, Some(building_id), &rules);
        assert_eq!(sim.anim(anim_id).unwrap().owner_entity, Some(building_id));

        assert!(sim.expire_anim_owner_reference(anim_id, building_id));
        assert_eq!(
            sim.substrate
                .entities
                .get(building_id)
                .unwrap()
                .damage_fire_anim_ids[0],
            Some(anim_id),
            "native owner expiry retains the slot until the Anim's own expiry"
        );
        assert!(sim.anim(anim_id).unwrap().runtime.inactive);
        assert!(sim.live_object_order_snapshot().contains(&anim_id));
        assert!(!sim.substrate.pending_delete.contains(&anim_id));

        sim.visit_anim(anim_id, &rules, None);
        assert!(
            sim.entities()
                .get(building_id)
                .unwrap()
                .damage_fire_anim_ids[0]
                .is_none()
        );
        assert!(!sim.live_object_order_snapshot().contains(&anim_id));
        assert_eq!(sim.substrate.pending_delete, vec![anim_id]);
    }

    #[test]
    fn finalizer_defensively_clears_remaining_owner_slot() {
        let (mut sim, rules, building_id) = damage_fire_fixture(false);
        let fire_type = sim.interner.get("FIRE01").unwrap();
        let anim_id = sim
            .spawn_anim_at_world(
                &rules,
                runtime_descriptor(fire_type, 0),
                AnimWorldCoord { x: 0, y: 0, z: 0 },
            )
            .unwrap();
        sim.anim_mut_by_id(anim_id).unwrap().owner_entity = Some(building_id);
        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .damage_fire_anim_ids[0] = Some(anim_id);
        sim.rebuild_building_anim_slot_indices();
        // Supply the common post-UnInit retirement state through its existing
        // membership owner;19B remains the independent native expiry byte.
        sim.conceal_anim(anim_id);
        sim.substrate.pending_delete.push(anim_id);

        sim.process_pending_delete();

        assert!(sim.anim(anim_id).is_none());
        assert!(!sim.live_object_order_snapshot().contains(&anim_id));
        assert!(
            sim.substrate
                .entities
                .get(building_id)
                .unwrap()
                .damage_fire_anim_ids[0]
                .is_none()
        );
    }

    #[test]
    fn gsi_05_12_attached_anim_follows_a_moving_owner_and_detaches_absolute() {
        // The point of owner-relative storage: `AnimClass::GetCoords @
        // 0x00422BE0` adds the owner's live coordinate, so the anim tracks the
        // owner without anyone rewriting the anim. `SetOwnerObject @
        // 0x00424B50` then writes the resolved absolute back on detach, so the
        // anim stays where it was standing.
        let (mut sim, rules, building_id) = damage_fire_fixture(false);
        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .health
            .current = 50;
        sim.update_building_damage_fire(building_id, &rules);
        let anim_id = sim
            .substrate
            .entities
            .get(building_id)
            .unwrap()
            .damage_fire_anim_ids[0]
            .expect("slot zero");
        sim.set_anim_owner_object(anim_id, Some(building_id), &rules);
        let before = sim.anim_absolute_coord(anim_id).expect("attached anim");
        let stored_before = sim.anim(anim_id).unwrap().world_coord;

        // Move the owner one cell east and one cell south.
        {
            let owner = sim.substrate.entities.get_mut(building_id).unwrap();
            owner.position.rx += 1;
            owner.position.ry += 1;
        }

        assert_eq!(
            sim.anim(anim_id).unwrap().world_coord,
            stored_before,
            "the stored delta is untouched by owner movement"
        );
        assert_eq!(
            sim.anim_absolute_coord(anim_id).unwrap(),
            AnimWorldCoord {
                x: before.x + LEPTONS_PER_CELL,
                y: before.y + LEPTONS_PER_CELL,
                z: before.z
            },
            "the resolved coordinate follows the owner one cell on each axis"
        );

        let moved = sim.anim_absolute_coord(anim_id).unwrap();
        assert_eq!(
            sim.detach_anim_from_owner(anim_id, &rules),
            Some(building_id)
        );
        assert!(sim.anim(anim_id).unwrap().owner_entity.is_none());
        assert_eq!(
            sim.anim(anim_id).unwrap().world_coord,
            moved,
            "detach writes the resolved absolute back into the stored field"
        );
        assert_eq!(sim.anim_absolute_coord(anim_id).unwrap(), moved);
    }

    /// One Z frame: a producer's height level is `Level * 104` world leptons
    /// (`IMUL [0x00ABDE88]`, the ground height unit), and the level a consumer
    /// reads back is the floor over the same unit. The store used to keep a
    /// private 128-per-level scale next to producers that wrote 104-frame
    /// leptons, so those drew a level low on raised ground.
    #[test]
    fn anim_coordinates_use_the_native_level_height() {
        let level_three = AnimWorldCoord::from_cell_sub_z(
            4,
            5,
            SimFixed::from_num(128),
            SimFixed::from_num(64),
            3,
        );
        assert_eq!(
            level_three,
            AnimWorldCoord {
                x: 4 * 256 + 128,
                y: 5 * 256 + 64,
                z: 3 * 104
            }
        );
        assert_eq!(level_three.to_cell_sub_z().4, 3);
        // An exact-Z producer's coordinate one lepton under level 1 is still
        // level 0; at 104 it is level 1 (128 would have called both level 0).
        let under = AnimWorldCoord {
            z: 103,
            ..level_three
        };
        let at = AnimWorldCoord {
            z: 104,
            ..level_three
        };
        assert_eq!((under.to_cell_sub_z().4, at.to_cell_sub_z().4), (0, 1));
    }

    /// `AnimClass::GetCoords @ 0x00422BE0` adds the owner's live coordinate on
    /// every axis, so an anim attached to an owner that gains height rises with
    /// it. The owner term used to be the coarse level byte only.
    #[test]
    fn gsi_05_12_attached_anim_follows_the_owners_height() {
        let (mut sim, rules, building_id) = damage_fire_fixture(false);
        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .health
            .current = 50;
        sim.update_building_damage_fire(building_id, &rules);
        let anim_id = sim
            .substrate
            .entities
            .get(building_id)
            .unwrap()
            .damage_fire_anim_ids[0]
            .expect("slot zero");
        sim.set_anim_owner_object(anim_id, Some(building_id), &rules);
        let before = sim.anim_absolute_coord(anim_id).expect("attached anim");

        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .position
            .exact_z_leptons = Some(before.z + 250);

        assert_eq!(
            sim.anim_absolute_coord(anim_id).unwrap(),
            AnimWorldCoord {
                z: before.z + 250,
                ..before
            },
            "250 leptons is not a whole level: the exact height carries through"
        );
    }

    #[test]
    fn building_damage_fire_uses_exact_threshold_slots_coords_and_depth() {
        let (mut sim, rules, building_id) = damage_fire_fixture(false);
        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .health
            .current = 50;
        let mut expected_rng = sim.scenario_rng.clone();
        let start_type = expected_rng.next_range_u32_inclusive(0, 2) as usize;
        let type_names = ["FIRE01", "FIRE02", "FIRE03"];
        let frame_counts = [30_u32, 64, 30];
        let expected_types = [start_type, (start_type + 1) % type_names.len()];
        let expected_frames = expected_types
            .map(|index| expected_rng.next_range_u32_inclusive(0, frame_counts[index] - 1) as i32);
        sim.update_building_damage_fire(building_id, &rules);

        let building = sim.substrate.entities.get(building_id).unwrap();
        assert!(building.damage_fire_state_active);
        let first = building.damage_fire_anim_ids[0].expect("slot zero");
        let second = building.damage_fire_anim_ids[1].expect("slot one");
        assert!(
            building.damage_fire_anim_ids[2..]
                .iter()
                .all(Option::is_none)
        );
        let first_anim = sim.anim(first).unwrap();
        assert_eq!(first_anim.owner_entity, None);
        assert_eq!(
            sim.interner.resolve(first_anim.type_id),
            type_names[expected_types[0]]
        );
        assert_eq!(first_anim.runtime.current_frame, expected_frames[0]);
        // Original43C0D0 stores absolute coordinates and leaves ctor+CC0.
        // Its Building5C8 slot/listener is independent of Object attachment.
        assert_eq!(
            sim.anim_absolute_coord(first).unwrap(),
            AnimWorldCoord {
                x: 2450,
                y: 2653,
                z: 0
            }
        );
        assert_eq!(
            first_anim.world_coord,
            AnimWorldCoord {
                x: 2450,
                y: 2653,
                z: 0
            },
            "damage fires retain absolute coordinates"
        );
        assert_eq!(first_anim.z_adjust, -192);
        let second_anim = sim.anim(second).unwrap();
        assert_eq!(second_anim.owner_entity, None);
        assert_eq!(
            sim.interner.resolve(second_anim.type_id),
            type_names[expected_types[1]]
        );
        assert_eq!(second_anim.runtime.current_frame, expected_frames[1]);
        assert_eq!(
            sim.anim_absolute_coord(second).unwrap(),
            AnimWorldCoord {
                x: 3140,
                y: 2594,
                z: 0
            }
        );
        assert_eq!(
            second_anim.world_coord,
            AnimWorldCoord {
                x: 3140,
                y: 2594,
                z: 0
            },
            "damage fires retain absolute coordinates"
        );
        assert_eq!(second_anim.z_adjust, -136);
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state()
        );
        assert_eq!(
            sim.live_object_order_snapshot(),
            vec![building_id, first, second]
        );
    }

    #[test]
    fn unchanged_damage_fire_cache_consumes_no_rng_and_recovery_clears_slots() {
        let (mut sim, rules, building_id) = damage_fire_fixture(false);
        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .health
            .current = 50;
        sim.update_building_damage_fire(building_id, &rules);
        let rng_after_spawn = sim.scenario_rng.logical_state();
        let ids = sim
            .substrate
            .entities
            .get(building_id)
            .unwrap()
            .damage_fire_anim_ids;

        sim.update_building_damage_fire(building_id, &rules);
        assert_eq!(sim.scenario_rng.logical_state(), rng_after_spawn);
        assert_eq!(
            sim.substrate
                .entities
                .get(building_id)
                .unwrap()
                .damage_fire_anim_ids,
            ids
        );

        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .health
            .current = 51;
        sim.update_building_damage_fire(building_id, &rules);
        let building = sim.substrate.entities.get(building_id).unwrap();
        assert!(!building.damage_fire_state_active);
        assert!(building.damage_fire_anim_ids.iter().all(Option::is_none));
        assert_eq!(sim.live_object_order_snapshot(), vec![building_id]);
    }

    #[test]
    fn empty_fire_type_list_sets_cache_without_rng_or_slots() {
        let (mut sim, mut rules, building_id) = damage_fire_fixture(false);
        rules.general.damage_fire_types.clear();
        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .health
            .current = 50;
        let rng_before = sim.scenario_rng.logical_state();

        sim.update_building_damage_fire(building_id, &rules);

        let building = sim.substrate.entities.get(building_id).unwrap();
        assert!(building.damage_fire_state_active);
        assert!(building.damage_fire_anim_ids.iter().all(Option::is_none));
        assert_eq!(sim.scenario_rng.logical_state(), rng_before);
    }

    #[test]
    fn occupied_first_slot_stops_after_initial_type_roll() {
        let (mut sim, rules, building_id) = damage_fire_fixture(false);
        let fire_type = sim.interner.get("FIRE01").unwrap();
        let occupied_id = sim
            .spawn_anim_at_world(
                &rules,
                AnimClassSpawnDescriptor {
                    type_name: fire_type,
                    rx: 0,
                    ry: 0,
                    sub_x: crate::util::fixed_math::SIM_ZERO,
                    sub_y: crate::util::fixed_math::SIM_ZERO,
                    z: 0,
                    delay: 0,
                    loop_count: 1,
                    draw_flags: TRAILER_DRAW_FLAGS,
                    z_adjust: 0,
                    reverse: false,
                    use_cell_drawer: false,
                    terrain_attached: false,
                    draw_runtime: AnimDrawRuntime::default(),
                },
                AnimWorldCoord { x: 0, y: 0, z: 0 },
            )
            .unwrap();
        let building = sim.substrate.entities.get_mut(building_id).unwrap();
        building.damage_fire_anim_ids[0] = Some(occupied_id);
        building.health.current = 50;
        let mut expected_rng = sim.scenario_rng.clone();
        let _ = expected_rng.next_range_u32_inclusive(0, 2);

        sim.update_building_damage_fire(building_id, &rules);

        let slots = sim
            .substrate
            .entities
            .get(building_id)
            .unwrap()
            .damage_fire_anim_ids;
        assert_eq!(slots[0], Some(occupied_id));
        assert!(slots[1..].iter().all(Option::is_none));
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state()
        );
    }

    #[test]
    fn repaired_health_clears_owned_anims_and_stop_is_idempotent() {
        let (mut sim, rules, building_id) = damage_fire_fixture(false);
        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .health
            .current = 50;
        sim.update_building_damage_fire(building_id, &rules);
        sim.sound_events.clear();
        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .health
            .current = 100;

        sim.update_building_damage_fire(building_id, &rules);
        sim.update_building_damage_fire(building_id, &rules);

        let building = sim.substrate.entities.get(building_id).unwrap();
        assert!(!building.damage_fire_state_active);
        assert!(building.damage_fire_anim_ids.iter().all(Option::is_none));
        assert_eq!(
            sim.sound_events
                .iter()
                .filter(|event| matches!(event, SimSoundEvent::ObjectSoundReleased { .. }))
                .count(),
            2,
        );
    }

    #[test]
    fn occupiable_building_selects_condition_red_boundary() {
        let (mut sim, rules, building_id) = damage_fire_fixture(true);
        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .health
            .current = 26;
        sim.update_building_damage_fire(building_id, &rules);
        assert!(
            !sim.substrate
                .entities
                .get(building_id)
                .unwrap()
                .damage_fire_state_active
        );
        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .health
            .current = 25;
        sim.update_building_damage_fire(building_id, &rules);
        assert!(
            sim.substrate
                .entities
                .get(building_id)
                .unwrap()
                .damage_fire_state_active
        );
    }

    #[test]
    fn first_anim_visit_only_clears_guard() {
        let (mut sim, rules, building_id) = damage_fire_fixture(false);
        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .health
            .current = 50;
        sim.update_building_damage_fire(building_id, &rules);
        let anim_id = sim
            .substrate
            .entities
            .get(building_id)
            .unwrap()
            .damage_fire_anim_ids[0]
            .unwrap();
        let frame = sim.anim(anim_id).unwrap().runtime.current_frame;
        sim.visit_anim(anim_id, &rules, None);
        let anim = sim.anim(anim_id).unwrap();
        assert_eq!(anim.runtime.current_frame, frame);
        assert!(!anim.runtime.first_ai_guard);
    }

    #[test]
    fn anim_store_slots_scheduler_and_hash_roundtrip() {
        let (mut sim, rules, building_id) = damage_fire_fixture(false);
        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .health
            .current = 50;
        sim.update_building_damage_fire(building_id, &rules);
        assert!(sim.substrate.pending_delete.is_empty());
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // AnimStore/scheduler persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();
        let expected_order = sim.live_object_order_snapshot();
        let bytes = bincode::serialize(&sim).expect("serialize sim with AnimStore");
        let mut restored: Simulation = bincode::deserialize(&bytes).expect("deserialize AnimStore");
        restored.rebuild_logic_membership();
        assert_eq!(restored.live_object_order_snapshot(), expected_order);
        assert_eq!(restored.state_hash(), expected_hash);
        assert!(restored.sound_events.is_empty());
        assert_eq!(
            restored
                .substrate
                .entities
                .get(building_id)
                .unwrap()
                .damage_fire_anim_ids,
            sim.substrate
                .entities
                .get(building_id)
                .unwrap()
                .damage_fire_anim_ids,
        );
    }
}
