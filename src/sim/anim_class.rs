//! Scheduler-owned ordinary SHP animation objects.
//!
//! `AnimStore` owns animation storage while `world::LogicVector` owns live AI
//! order. This module implements the verified ordinary non-bouncer AnimClass
//! lifecycle: constructor/reveal, first-AI guard, start delay and its `Start`
//! edge, logic-frame timing, loops, reverse/ping-pong, Next, trailer, sound
//! identity, owner attachment, conceal, and deferred deletion. Its producers
//! are building slots and damage fires, tile and crate animations, combat
//! explosions, weapon and occupant muzzle flashes, parachute canopies,
//! teleport warps,
//! superweapon invokes, Lightning Storm bolts, bridge collapse explosions,
//! wakes and ore twinkles.
//!
//! ## Residuals — two `AnimClass::AI` arms are not built
//!
//! RESIDUAL (M11b) — **the bouncing-debris landing arm.** `AnimClass::AI
//! 0x00423E28..0x00423F37` runs when `AnimClass+0x194` (set by the constructor
//! for `Bouncer=` or `IsMeteor=`) is live and
//! `AnimClass::ProcessBounceResult @ 0x00423930` reports landed or gone. It
//! constructs the type's `ExpireAnim=` and calls `Apply_area_damage` with the
//! type's own `Damage=`/`Warhead=`/`DamageRadius=`, walks the impact cell's
//! occupant list for `ReceiveDamage`, and spawns `Spawns=` behind two
//! `RandomRanged` draws. `BounceState` (`src/sim/bounce.rs`) is ported but no
//! `AnimClass` hosts one.
//! - Trigger: every building death and every heavy-vehicle death, through
//!   `[General] MetallicDebris=` (20 stock `DBRIS*` types, all `Bouncer=yes`).
//! - Player effect: debris chunks neither damage what they land on
//!   (`Damage=10..20`, warhead `HE`, `DamageRadius=50..80`) nor leave their
//!   `TWLT026`/`TWLT036` landing explosions.
//! - Frequency: continuous in ordinary skirmish — the higher-weight of the two
//!   missing arms by a wide margin.
//! - Downstream risk: the constructor fork draws two or three RNG values per
//!   chunk on the scenario stream, so wiring it moves the lockstep stream and
//!   needs its own `SNAPSHOT_VERSION` move and fixture re-record.
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
//! - Trigger: the `FIRE3` fires a destroyed building leaves behind
//!   (`BuildingClass::DestructionEffects` loads the literal name at
//!   `0x00441AEC`). The `BURN-S/M/L` family is commented out of
//!   `[Animations]` and `INVISO` has no stock producer, so nothing else in
//!   stock reaches it.
//! - Player effect: `[FIRE3] Damage=.003` plus the `1.0` seed means exactly one
//!   point of `Fire2` area damage on the fire's first ticked frame and then
//!   none for another ~334 frames. Missing it costs one splash per burning
//!   building.
//! - Frequency: once per building death; magnitude 1.
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
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::intern::InternedId;
use crate::sim::occupancy::{RawCellOccupationGrid, infantry_raw_occupation_mask};
use crate::sim::timer::CdTimer;
use crate::sim::world::{LifecycleOutput, SimSoundEvent, Simulation};
use crate::util::fixed_math::SimFixed;
use crate::util::lepton::{BRIDGE_HEIGHT_DELTA_LEPTONS, ground_height_leptons};

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
const BUILDING_RENDER_ORIGIN_LEPTONS: i32 = 128;
const DAMAGE_FIRE_SLOT_COUNT: usize = 8;
// Retained with the verified multiplayer-feedback spawn seam until command
// feedback owns its production call site.
#[allow(dead_code)]
const MULTIPLAYER_FEEDBACK_Z_ADJUST: i32 = -5000;
#[allow(dead_code)]
const SYNC_EXEMPT_NATIVE_UNIQUE_ID: i32 = -2;

/// Pure YR `AnimClass_UpdateBouncePhysics` directional-frame projection.
pub fn directional_tumble_frame(running_frames: i32, bucket8: i32, global_frame: i32) -> i32 {
    let running = running_frames.max(1);
    running * ((-1 - bucket8) & 7) + (global_frame / 3).rem_euclid(running)
}

pub fn settled_bounce_frame(running_frames: i32) -> i32 {
    running_frames.wrapping_mul(8).wrapping_add(1)
}

/// `AnimClass_Update` @ 0x00423f37: landing consumes two inclusive rolls.
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    pub remap_color: Option<HouseColorIndex>,
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
    /// Native Anim+118 suppresses its independent draw; the building draws it.
    /// This is not the distinct Anim+CC Object-owner attachment.
    #[serde(skip)]
    pub building_slot: Option<(u64, u8)>,
    pub start_sound_active: bool,
    pub stop_sound_id: Option<InternedId>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnimStore(BTreeMap<AnimId, AnimObject>);

impl AnimStore {
    pub fn get(&self, id: AnimId) -> Option<&AnimObject> {
        self.0.get(&id)
    }

    pub(crate) fn get_mut(&mut self, id: AnimId) -> Option<&mut AnimObject> {
        self.0.get_mut(&id)
    }

    pub(crate) fn insert(&mut self, object: AnimObject) -> Option<AnimObject> {
        self.0.insert(object.stable_id, object)
    }

    pub(crate) fn remove(&mut self, id: AnimId) -> Option<AnimObject> {
        self.0.remove(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&AnimId, &AnimObject)> {
        self.0.iter()
    }

    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut AnimObject> {
        self.0.values_mut()
    }

    pub fn contains_key(&self, id: AnimId) -> bool {
        self.0.contains_key(&id)
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn key_at(&self, index: usize) -> Option<AnimId> {
        self.0.keys().nth(index).copied()
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
}

enum VisitAction {
    None,
    Destroy,
    DestroyAfterMakeInfantryClear,
    Next(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnimOccupationOperation {
    Mark,
    Clear,
}

fn apply_anim_raw_occupation(
    grid: &mut RawCellOccupationGrid,
    rx: u16,
    ry: u16,
    mask: u8,
    world_z: i32,
    ground_z: i32,
    live_structural_bridge: bool,
    operation: AnimOccupationOperation,
) {
    let reaches_deck = world_z >= ground_z.wrapping_add(BRIDGE_HEIGHT_DELTA_LEPTONS as i32);
    let use_deck = match operation {
        AnimOccupationOperation::Mark => reaches_deck && live_structural_bridge,
        // AnimClass::ClearCellOccupancy deliberately ignores Cell+0x140 bit
        // 0x100. This can leave a ground bit stale when a high animation was
        // marked after structural bridge state disappeared.
        AnimOccupationOperation::Clear => reaches_deck,
    };
    match (operation, use_deck) {
        (AnimOccupationOperation::Mark, false) => grid.mark_ground(rx, ry, mask),
        (AnimOccupationOperation::Mark, true) => grid.mark_deck(rx, ry, mask),
        (AnimOccupationOperation::Clear, false) => grid.clear_ground(rx, ry, mask),
        (AnimOccupationOperation::Clear, true) => grid.clear_deck(rx, ry, mask),
    }
}

impl Simulation {
    pub fn anim(&self, id: AnimId) -> Option<&AnimObject> {
        self.substrate
            .anims
            .get(id)
            .or_else(|| self.substrate.multiplayer_feedback_anims.get(id))
    }

    pub fn anims(&self) -> impl Iterator<Item = (&AnimId, &AnimObject)> {
        self.substrate
            .anims
            .iter()
            .chain(self.substrate.multiplayer_feedback_anims.iter())
    }

    pub fn multiplayer_feedback_anims(&self) -> impl Iterator<Item = (&AnimId, &AnimObject)> {
        self.substrate.multiplayer_feedback_anims.iter()
    }

    fn anim_mut_by_id(&mut self, id: AnimId) -> Option<&mut AnimObject> {
        if self.substrate.anims.contains_key(id) {
            self.substrate.anims.get_mut(id)
        } else {
            self.substrate.multiplayer_feedback_anims.get_mut(id)
        }
    }

    fn is_multiplayer_feedback_anim(&self, id: AnimId) -> bool {
        self.substrate.multiplayer_feedback_anims.contains_key(id)
    }

    fn apply_make_infantry_raw_occupation(
        &mut self,
        world: AnimWorldCoord,
        operation: AnimOccupationOperation,
    ) {
        let cell_x = world.x >> 8;
        let cell_y = world.y >> 8;
        let (Ok(rx), Ok(ry)) = (u16::try_from(cell_x), u16::try_from(cell_y)) else {
            // Native writes its shared dummy cell for out-of-map coordinates;
            // that dummy is not part of Rust's serialized map substrate.
            return;
        };
        let mask = infantry_raw_occupation_mask(
            SimFixed::from_num(world.x & 0xff),
            SimFixed::from_num(world.y & 0xff),
        );
        let (ground_z, live_structural_bridge) = self
            .resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.cell(rx, ry))
            .and_then(|cell| {
                ground_height_leptons(cell.level, cell.slope_type, world.x, world.y)
                    .ok()
                    .map(|ground_z| {
                        let live_structural_bridge = cell.bridge_facts.has_structural_bridge()
                            && self
                                .bridge_state
                                .as_ref()
                                .is_some_and(|state| state.is_bridge_walkable(rx, ry));
                        (ground_z, live_structural_bridge)
                    })
            })
            .unwrap_or((0, false));
        apply_anim_raw_occupation(
            &mut self.substrate.raw_cell_occupation,
            rx,
            ry,
            mask,
            world.z,
            ground_z,
            live_structural_bridge,
            operation,
        );
    }

    /// Construct the `AnimClass` a producer described by cell, sub-cell and
    /// height level.
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
    /// RESIDUAL — the impact Z arrives as the producer's height-level byte,
    /// not exact leptons: `ExplosionEffect` carries the byte while its paired
    /// `SmudgeSpawnRequest::Anim` carries the exact `world_z_leptons`. The
    /// store itself holds exact leptons, so this is the producer's to fix.
    /// - Trigger: any detonation whose impact Z is not a whole level — an
    ///   airburst, or a shot landing on a slope.
    /// - Player effect: the explosion sprite's height, and therefore its depth
    ///   sort against nearby objects, can be off by up to one height level.
    /// - Frequency: common.
    /// - Downstream risk: widening `ExplosionEffect` moves hashed anim
    ///   coordinates for every such detonation.
    pub(crate) fn spawn_combat_explosion_anim(
        &mut self,
        rules: &RuleSet,
        type_name: InternedId,
        rx: u16,
        ry: u16,
        sub_x: crate::util::fixed_math::SimFixed,
        sub_y: crate::util::fixed_math::SimFixed,
        z: u8,
    ) -> Option<AnimId> {
        let descriptor = AnimClassSpawnDescriptor {
            delay: 0,
            loop_count: 1,
            draw_flags: COMBAT_EXPLOSION_DRAW_FLAGS,
            z_adjust: COMBAT_EXPLOSION_Z_ADJUST,
            reverse: false,
            ..AnimClassSpawnDescriptor::new(type_name, rx, ry, sub_x, sub_y, z)
        };
        let world_coord = AnimWorldCoord::from_cell_sub_z(rx, ry, sub_x, sub_y, z);
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
        let type_name = self
            .interner
            .resolve(descriptor.type_name)
            .to_ascii_uppercase();
        let config = rules
            .art_registry
            .anim_runtime_config(&type_name)
            .cloned()
            .ok_or(AnimSpawnError::MissingType(descriptor.type_name))?;
        let (effective_end, effective_loop_end) = effective_bounds(&type_name, &config)?;
        let reverse = descriptor.reverse || config.reverse;
        let rate_reload = self.choose_anim_rate(&config);
        let frame_timer =
            CdTimer::started(self.session.binary_frame as i32, i32::from(rate_reload));
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
            native_unique_id: stable_id as i32,
            type_id: descriptor.type_name,
            world_coord,
            draw_flags: descriptor.draw_flags,
            z_adjust: descriptor.z_adjust,
            remap_color: None,
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
                rate_reload,
                frame_timer,
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
            start_sound_active: false,
            stop_sound_id,
        };
        // The insert must run in every build profile: wrapped in
        // `debug_assert!` it was compiled out of release binaries and no
        // scheduler anim ever existed in a shipped build.
        let previous = self.substrate.anims.insert(object);
        debug_assert!(previous.is_none());
        // Native registry insertion precedes Reveal, and Reveal precedes the
        // delay-zero constructor-time Start call.
        self.reveal_anim(stable_id);
        if descriptor.delay == 0 {
            self.anim_start(stable_id, &config);
        }
        Ok(stable_id)
    }

    /// Fresh-authored-load Anim constructor with an already-assigned native
    /// identity. The object is present in the Anim registry before the optional
    /// Scenario `RandomRate` draw, matching `AnimClass::Constructor`.
    pub(crate) fn spawn_load_anim_at_world(
        &mut self,
        art: &crate::rules::art_data::ArtRegistry,
        descriptor: AnimClassSpawnDescriptor,
        world_coord: AnimWorldCoord,
        native_unique_id: i32,
    ) -> Result<AnimId, AnimSpawnError> {
        let type_name = self
            .interner
            .resolve(descriptor.type_name)
            .to_ascii_uppercase();
        let config = art
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
            remap_color: None,
            world_coord,
            draw_flags: descriptor.draw_flags,
            z_adjust: descriptor.z_adjust,
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
            start_sound_active: false,
            stop_sound_id,
        };
        // The insert must run in every build profile: wrapped in
        // `debug_assert!` it was compiled out of release binaries and no
        // scheduler anim ever existed in a shipped build.
        let previous = self.substrate.anims.insert(object);
        debug_assert!(previous.is_none());

        let rate_reload = self.choose_anim_rate(&config);
        let frame_timer =
            CdTimer::started(self.session.binary_frame as i32, i32::from(rate_reload));
        let registered = self
            .substrate
            .anims
            .get_mut(stable_id)
            .expect("load Anim remains registered across RandomRate");
        registered.runtime.rate_reload = rate_reload;
        registered.runtime.frame_timer = frame_timer;

        self.reveal_anim(stable_id);
        if descriptor.delay == 0 {
            self.anim_start(stable_id, &config);
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
            let world = self.anim_absolute_coord(id);
            let start_sound_active = self
                .substrate
                .anims
                .get(id)
                .is_some_and(|anim| anim.start_sound_active);
            self.detach_anim_from_owner(id);
            if start_sound_active && let Some(world) = world {
                // `MapClass::InitCellAttributes @ 0x00568BB0` reaches the
                // scalar-deleting Anim destructor with StopSound forced null.
                self.sound_events.push(SimSoundEvent::AnimationStopped {
                    anim_id: id,
                    stop_sound_id: None,
                    world,
                });
            }
            self.conceal_anim(id);
            self.substrate.pending_delete.retain(|queued| *queued != id);
            let removed_anim = self.substrate.anims.remove(id);
            debug_assert!(removed_anim.is_some());
            removed += 1;
        }
        removed
    }

    // The move-feedback producer is not wired yet; keep the verified
    // sync-exempt allocation path available for that activation slice.
    #[allow(dead_code)]
    pub(crate) fn spawn_multiplayer_feedback_anim_at_world(
        &mut self,
        rules: &RuleSet,
        world_coord: AnimWorldCoord,
    ) -> Result<AnimId, AnimSpawnError> {
        let type_id = self.interner.intern(&rules.general.move_flash.name);
        let type_name = self.interner.resolve(type_id).to_ascii_uppercase();
        let config = rules
            .art_registry
            .anim_runtime_config(&type_name)
            .cloned()
            .ok_or(AnimSpawnError::MissingType(type_id))?;
        let (effective_end, effective_loop_end) = effective_bounds(&type_name, &config)?;
        let reverse = config.reverse;
        let rate_reload = self.choose_anim_rate(&config);
        let frame_timer =
            CdTimer::started(self.session.binary_frame as i32, i32::from(rate_reload));
        let stop_sound_id = config
            .stop_sound
            .as_deref()
            .map(|sound| self.interner.intern(sound));
        let stable_id = self.substrate.next_multiplayer_feedback_anim_id;
        self.substrate.next_multiplayer_feedback_anim_id = stable_id.wrapping_add(1);
        if self
            .substrate
            .multiplayer_feedback_anims
            .contains_key(stable_id)
        {
            return Err(AnimSpawnError::DuplicateId(stable_id));
        }

        let object = AnimObject {
            stable_id,
            native_unique_id: SYNC_EXEMPT_NATIVE_UNIQUE_ID,
            type_id,
            world_coord,
            draw_flags: TRAILER_DRAW_FLAGS,
            z_adjust: MULTIPLAYER_FEEDBACK_Z_ADJUST,
            remap_color: None,
            effective_end,
            effective_loop_end,
            runtime: AnimRuntime {
                current_frame: if reverse {
                    effective_loop_end.wrapping_sub(1)
                } else {
                    0
                },
                frame_step: if reverse { -1 } else { 1 },
                delay_remaining: 0,
                rate_reload,
                frame_timer,
                loop_remaining: native_loop_remaining(config.loop_count, 1),
                first_ai_guard: true,
                constructor_reverse: false,
                inactive: false,
                paused: false,
            },
            draw_runtime: AnimDrawRuntime::default(),
            use_cell_drawer: false,
            terrain_attached: false,
            in_logic_vector: false,
            owner_entity: None,
            building_slot: None,
            start_sound_active: false,
            stop_sound_id,
        };
        debug_assert!(
            self.substrate
                .multiplayer_feedback_anims
                .insert(object)
                .is_none()
        );
        self.anim_start(stable_id, &config);
        Ok(stable_id)
    }

    /// `CellClass::Get_Tiberium_Value @ 0x00485020` of the cell under a world
    /// coordinate. Native resolves the raw Location through
    /// `MapClass::Get_CellClass_At_Coord @ 0x00565730` and reads the shared
    /// dummy on a miss; VERA reads the grid cell for any coordinate inside the
    /// rectangular grid and the dummy's overlay pair outside it (the
    /// in-rectangle/outside-diamond difference is unreachable for retail art).
    fn anim_cell_tiberium_value(
        &self,
        world_coord: AnimWorldCoord,
        rules: &RuleSet,
        overlay_registry: &crate::map::overlay_types::OverlayTypeRegistry,
    ) -> i32 {
        let rx = u16::try_from(world_coord.x.div_euclid(LEPTONS_PER_CELL)).ok();
        let ry = u16::try_from(world_coord.y.div_euclid(LEPTONS_PER_CELL)).ok();
        let (overlay_id, overlay_data) = match (rx, ry, self.overlay_grid.as_ref()) {
            (Some(rx), Some(ry), Some(grid)) if rx < grid.width() && ry < grid.height() => {
                let cell = grid.cell(rx, ry);
                (cell.overlay_id, cell.overlay_data)
            }
            _ => self.effective_shared_cell_dummy().overlay_fields(),
        };
        crate::sim::ore_twinkle::tiberium_value(
            overlay_id,
            overlay_data,
            overlay_registry,
            &rules.tiberium_types,
        )
    }

    pub(crate) fn for_each_multiplayer_feedback_anim<F>(&mut self, mut body: F)
    where
        F: FnMut(&mut Simulation, AnimId),
    {
        let mut index = 0;
        while index < self.substrate.multiplayer_feedback_anims.len() {
            let Some(id) = self.substrate.multiplayer_feedback_anims.key_at(index) else {
                break;
            };
            body(self, id);
            index += 1;
        }
    }

    pub(crate) fn visit_anim(
        &mut self,
        id: AnimId,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) {
        // `AnimClass::GetCoords @ 0x00422BE0`, not the stored field: an
        // owner-attached anim stores an owner-relative delta.
        let Some(world_coord) = self.anim_absolute_coord(id) else {
            return;
        };
        let Some((type_id, first_guard, inactive)) = self.anim(id).map(|anim| {
            (
                anim.type_id,
                anim.runtime.first_ai_guard,
                anim.runtime.inactive,
            )
        }) else {
            return;
        };
        let type_name = self.interner.resolve(type_id).to_ascii_uppercase();
        let Some(config) = rules.art_registry.anim_runtime_config(&type_name).cloned() else {
            self.destroy_anim(id);
            return;
        };

        // `AnimClass::AI @ 0x00423AC0`, before the MakeInfantry `vtable+0xF0`
        // call, the bounce-landing block, and the trailer block: with
        // `AnimType+0x359 HideIfNoOre`, `AnimClass+0x19D` is rewritten every
        // tick from the anim coordinate's cell — hidden when the cell is
        // missing or `CellClass::Get_Tiberium_Value @ 0x00485020` is zero,
        // visible otherwise. Only drawing is suppressed; the AI keeps running.
        // Registry-less callers (fixtures) keep the current flag.
        if config.hide_if_no_ore
            && let Some(overlay_registry) = overlay_registry
        {
            let hidden = self.anim_cell_tiberium_value(world_coord, rules, overlay_registry) == 0;
            if let Some(anim) = self.anim_mut_by_id(id) {
                anim.draw_runtime.hidden = hidden;
            }
        }

        // AnimClass::AI performs this before its first-AI, inactive, delay,
        // visibility, and frame-timer gates. Repeated visits OR the same raw
        // bit; there is deliberately no contributor count.
        if config.make_infantry != -1 {
            self.apply_make_infantry_raw_occupation(world_coord, AnimOccupationOperation::Mark);
        }
        if inactive {
            self.destroy_anim(id);
            return;
        }

        if let Some(trailer_name) = config.trailer_anim.as_deref() {
            if trailer_cadence_matches(
                u64::from(self.session.binary_frame),
                config.trailer_seperation,
            ) && rules
                .art_registry
                .anim_runtime_config(trailer_name)
                .is_some()
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
            return;
        }

        // `AnimClass::AI @ 0x00423AC0` delay countdown: the visit that takes it
        // to zero calls `AnimClass::Start @ 0x00424CE0` (call site `0x004243A1`)
        // and returns. The field also holds the `RandomLoopDelay=` pause, so a
        // looping anim restarts, and replays its start sound, after each pause.
        {
            let Some(anim) = self.anim_mut_by_id(id) else {
                return;
            };
            if anim.runtime.delay_remaining > 0 {
                anim.runtime.delay_remaining -= 1;
                if anim.runtime.delay_remaining == 0 {
                    self.anim_start(id, &config);
                }
                return;
            }
        }

        let mut action = VisitAction::None;
        let mut random_loop_delay = None;
        let current_frame = self.session.binary_frame as i32;
        {
            let Some(anim) = self.anim_mut_by_id(id) else {
                return;
            };
            //42449B: power pause follows first-AI/delay gates and precedes timer advance.
            if anim.runtime.paused {
                return;
            }
            if anim.runtime.rate_reload == 0 {
                return;
            }
            if !anim.runtime.frame_timer.expired(current_frame) {
                return;
            }
            anim.runtime
                .frame_timer
                .start(current_frame, i32::from(anim.runtime.rate_reload));
            anim.runtime.current_frame = anim
                .runtime
                .current_frame
                .wrapping_add(anim.runtime.frame_step);

            match advance_anim_boundary(anim, &config) {
                AnimBoundary::Continue | AnimBoundary::Bounce => return,
                AnimBoundary::Loop => {
                    random_loop_delay = config.random_loop_delay;
                }
                AnimBoundary::Complete => {
                    action = if let Some(next) = config.next.clone() {
                        VisitAction::Next(next)
                    } else if config.make_infantry != -1 {
                        VisitAction::DestroyAfterMakeInfantryClear
                    } else {
                        VisitAction::Destroy
                    };
                }
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
            VisitAction::Destroy => self.destroy_anim(id),
            VisitAction::DestroyAfterMakeInfantryClear => {
                // Native clears before validating AnimToInfantry, resolving an
                // owner, allocating the infantry, or attempting Unlimbo. The
                // downstream factory/retry path belongs to the entity-runtime
                // implementation item; this Phase-3 slice owns its preceding
                // authoritative cell-byte transition.
                self.apply_make_infantry_raw_occupation(
                    world_coord,
                    AnimOccupationOperation::Clear,
                );
                self.destroy_anim(id);
            }
            VisitAction::Next(next) => self.switch_anim_type(id, &next, rules),
        }
    }

    pub(crate) fn destroy_anim(&mut self, id: AnimId) {
        let is_feedback = self.is_multiplayer_feedback_anim(id);
        let already_queued = if is_feedback {
            self.substrate
                .multiplayer_feedback_pending_delete
                .contains(&id)
        } else {
            self.substrate.pending_delete.contains(&id)
        };
        if already_queued {
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
        self.detach_anim_from_owner(id);
        if let Some(anim) = self.anim_mut_by_id(id) {
            anim.runtime.inactive = true;
            anim.start_sound_active = false;
        }
        self.sound_events.push(SimSoundEvent::AnimationStopped {
            anim_id: id,
            stop_sound_id: stop_sound,
            world,
        });
        if is_feedback {
            self.substrate.multiplayer_feedback_pending_delete.push(id);
        } else {
            self.conceal_anim(id);
            self.substrate.pending_delete.push(id);
        }
    }

    /// Building451A2C and ClearAnimSlot451E40 use scalar deletion: the old
    /// object is gone before the replacement pointer is installed. Keep this
    /// separate from an animation's ordinary deferred Destroy operation.
    pub(crate) fn scalar_delete_building_anim(&mut self, id: AnimId) {
        // Anim VT7E3354+20 ->426590 ->4228E0 releases sound handles but
        // never reaches Destroy4255B0 or its StopSound playback. The slot was
        // cleared by the caller before these synchronous destructor effects.
        let world = self.anim_absolute_coord(id);
        let sound_active = self.anim(id).is_some_and(|anim| anim.start_sound_active);
        self.detach_anim_from_owner(id);
        self.clear_building_anim_reference(id);
        if sound_active && let Some(world) = world {
            self.sound_events.push(SimSoundEvent::AnimationStopped {
                anim_id: id,
                stop_sound_id: None,
                world,
            });
        }
        self.conceal_anim(id);
        self.substrate.pending_delete.retain(|queued| *queued != id);
        self.substrate.anims.remove(id);
    }

    pub(crate) fn clear_building_anim_reference(&mut self, id: AnimId) {
        let slot = self
            .substrate
            .anims
            .get_mut(id)
            .and_then(|anim| anim.building_slot.take());
        if let Some((owner, slot)) = slot
            && let Some(entity) = self.substrate.entities.get_mut(owner)
            && entity.building_anim_slots[usize::from(slot)] == Some(id)
        {
            entity.building_anim_slots[usize::from(slot)] = None;
        }
    }

    /// Clear the owner link both ways. This is the `AnimClass::Destroy @
    /// 0x004255B0` order — owner callback (the owner's vtable `+0x60`, whose
    /// Techno/Object implementation is `FUN_00710410`; here the
    /// `damage_fire_anim_ids` slot clear) before the owner pointer itself.
    ///
    /// gamemd-derived: `AnimClass::SetOwnerObject @ 0x00424B50` — the attach
    /// half. Native stores an attached anim's coordinate RELATIVE to its owner
    /// and resolves it back on read, so the anim follows a moving owner for
    /// free. The disassembly is explicit about the sign and the axes: at
    /// `0x00424C16` it takes the anim's own `GetCoords` (vtable `+0x48`, with
    /// the owner pointer still null so this is the stored absolute), at
    /// `0x00424C37` it writes the owner into `Anim+0xCC`, at `0x00424C46` it
    /// takes the owner's `GetCoords`, then `SUB EBX,ECX` / `SUB EBP,EDI` /
    /// `SUB ECX,EDX` (`0x00424C51`, `0x00424C5F`, `0x00424C5B`) form
    /// `anim_abs - owner_abs` on X, Y and Z and hand that to `SetCoords`
    /// (vtable `+0x1B4`) at `0x00424C70`. The detach half at `0x00424BBD`
    /// mirrors it: read `GetCoords` while the owner is still set — so it comes
    /// back absolute — null `Anim+0xCC`, and store the absolute.
    ///
    /// Three native steps are deliberately not modelled, and it matters what
    /// each one actually is:
    /// - The owner's "has an anim attached" byte. Attach sets
    ///   `[owner+0x84] = 1` at `0x00424C30`; detach clears it at `0x00424BB6`,
    ///   but only when the `g_AnimClass_Array` scan at `0x00424B85` finds no
    ///   OTHER anim sharing that owner. Nothing in this engine reads
    ///   `Object+0x84`, so neither write has an observable consequence here.
    ///   The multi-slot case the scan exists for is already correct for a
    ///   different reason: [`Self::detach_anim_from_owner`] clears only the
    ///   slot equal to the departing anim.
    /// - The same guard also gates a virtual call on the OWNER,
    ///   `CALL [owner_vtable+0x17C]` at `0x00424BB0` — read directly, not
    ///   inferred: BuildingClass's primary vtable base is `0x007E3EBC` and
    ///   `+0x17C` there is `0x005F43C0`, whose body is a bare `RET`. The Unit,
    ///   Infantry and Aircraft vtables hold the same `0x005F43C0` at `+0x17C`
    ///   (read for the muzzle flash, the first non-building attach producer),
    ///   so the skipped call does nothing for any owner this engine attaches.
    /// - The `DisplayClass::RemoveFromLayer` / `Submit_Object` re-registration
    ///   pair either side of the pointer write (`0x004A9770` / `0x004A9720`).
    ///   Layer membership is rebuilt from `tactical_registration_order` every
    ///   frame in this engine rather than held as a persistent container, so
    ///   there is no registration to move.
    ///
    /// Returns `false` when the anim does not exist, or when the requested
    /// owner does not.
    pub(crate) fn set_anim_owner_object(&mut self, id: AnimId, new_owner: Option<u64>) -> bool {
        if self.anim(id).is_none() {
            return false;
        }

        // Detach half: resolve back to absolute *before* dropping the owner.
        if self.anim(id).and_then(|anim| anim.owner_entity).is_some() {
            let absolute = self
                .anim_absolute_coord(id)
                .expect("anim exists: checked at the head of this function");
            if let Some(anim) = self.anim_mut_by_id(id) {
                anim.owner_entity = None;
                anim.world_coord = absolute;
            }
        }

        // Attach half: the stored coordinate becomes the owner-relative delta.
        if let Some(owner_id) = new_owner {
            let Some(owner_coord) = self.anim_owner_coords(owner_id) else {
                return false;
            };
            if let Some(anim) = self.anim_mut_by_id(id) {
                anim.world_coord = AnimWorldCoord {
                    x: anim.world_coord.x.wrapping_sub(owner_coord.x),
                    y: anim.world_coord.y.wrapping_sub(owner_coord.y),
                    z: anim.world_coord.z.wrapping_sub(owner_coord.z),
                };
                anim.owner_entity = Some(owner_id);
            }
        }
        true
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
        let anim = self.anim(id)?;
        let Some(owner_coord) = anim
            .owner_entity
            .and_then(|owner| self.anim_owner_coords(owner))
        else {
            return Some(anim.world_coord);
        };
        Some(AnimWorldCoord {
            x: anim.world_coord.x.wrapping_add(owner_coord.x),
            y: anim.world_coord.y.wrapping_add(owner_coord.y),
            z: anim.world_coord.z.wrapping_add(owner_coord.z),
        })
    }

    /// The owner side of `AnimClass::GetCoords`: `ObjectClass::GetCoords` on
    /// the attached-to object.
    ///
    /// X and Y come from `object_center_coord_with_foundation`, the owner of
    /// `BuildingClass::GetCoords @ 0x00447AC0`'s `(W-1) * 128` / `(H-1) * 128`
    /// shift onto the foundation centre. Z is `object_world_z_leptons`, the
    /// object's actual height including locomotor altitude, so an anim attached
    /// to an airborne or elevated owner follows it. The attach/detach round
    /// trip is exact because the same value is subtracted and added back.
    pub(crate) fn anim_owner_coords(&self, owner_id: u64) -> Option<AnimWorldCoord> {
        let owner = self.substrate.entities.get(owner_id)?;
        let centre = crate::sim::movement::ground_pose::object_center_coord_with_foundation(
            owner,
            &owner.foundation,
        );
        Some(AnimWorldCoord {
            x: centre.x,
            y: centre.y,
            z: crate::sim::combat::object_world_z_leptons(owner, self.resolved_terrain.as_ref()),
        })
    }

    /// gamemd-derived: `AnimClass::SetOwnerObject @ 0x00424B50` detach half,
    /// plus this engine's damage-fire slot bookkeeping. See
    /// [`Self::set_anim_owner_object`] for the coordinate contract; the slot
    /// clear below is why the native `g_AnimClass_Array` shared-owner scan has
    /// nothing to guard here.
    pub(crate) fn detach_anim_from_owner(&mut self, id: AnimId) -> Option<u64> {
        let owner_id = self.anim(id).and_then(|anim| anim.owner_entity)?;
        if let Some(owner) = self.substrate.entities.get_mut(owner_id) {
            for slot in &mut owner.damage_fire_anim_ids {
                if *slot == Some(id) {
                    *slot = None;
                }
            }
        }
        self.set_anim_owner_object(id, None);
        Some(owner_id)
    }

    /// Owner expiry, dispatched from `ObjectClass::UnInit -> Detach_From_All_Lists`
    /// before the owner's conceal and alive clear. `AnimClass::Detach @
    /// 0x00425150` removes the anim from its display layer, calls the owner
    /// callback, clears the owner pointer, sets the detached marker
    /// `AnimClass+0x19B = 1`, and calls anim vtable `+0x124(0)`.
    ///
    /// `Detach` itself neither destroys nor deactivates — that is deferred into
    /// the marker. `AnimClass::AI @ 0x00423AC0` reloads `+0x19B` at `0x0042435F`
    /// and `JNZ 0x00424B38`, whose two instructions call anim vtable `+0xF8`
    /// (`AnimClass::Destroy`) and return. `runtime.inactive` is that marker: set
    /// here, checked at the head of `visit_anim`, which calls `destroy_anim` and
    /// returns. The `StopSound` frame and the pending-delete frame therefore
    /// land where native puts them, because `destroy_anim` owns both; draw
    /// suppression is owned separately by the `DisplayRemove` push below plus
    /// the `runtime.inactive` skip in
    /// `app/presentation/instances/overlays.rs`, mirroring native hiding
    /// through `DisplayClass::RemoveFromLayer` rather than through a `DrawIt`
    /// branch on `+0x19B`. The marker is serialized, matching native
    /// `SaveExtras`; it suppresses the trailer spawn, because `visit_anim`
    /// gates ahead of the trailer block exactly as native guards its trailer
    /// block on `+0x19B == 0` at `0x004242B0`; and the `Next=` transition
    /// clears it, matching native's `+0x19B = 0` write there.
    ///
    /// DRIFT (GSI-05.12) — the marker is checked earlier in the visit than
    /// native checks it, and the difference has no observable surface here yet.
    /// The gate sits 0x89F bytes into `AnimClass::AI`
    /// (`0x00423AC0`..`0x0042435F`), so native runs a prefix on a detached anim
    /// that `visit_anim` skips: `AnimClass::UpdateLoopingSound @ 0x00750D40`
    /// (entered on `Anim+0x198 == 0` and `AnimType+0x2F8 != -1`); `BounceAI`
    /// plus the `AnimType+0x354` `ObjectClass::AI` call; the bouncer-impact
    /// block on instance byte `+0x194`, which itself contains
    /// `AnimClass::Constructor` calls and an `Apply_area_damage` call; the
    /// `+0x19D` draw-suppression writers at `0x00423B5C`,
    /// `0x00423B7F`/`0x00423B88` and `0x00423BB8`/`0x00423BC1`; and the `+0x11B`
    /// frame-equality cleanup at `0x00423C03`..`0x00423C1D`. `visit_anim` runs
    /// only the make-infantry occupation mark before its own gate.
    ///
    /// Four of those five items are inert in gamemd itself for this trigger,
    /// and the fifth is inert only here, which is why moving the gate now would
    /// be a no-op rather than a fix:
    /// - `BounceAI` plus the `AnimType+0x354` `ObjectClass::AI` call never run.
    ///   `AnimType+0x354` is `IsFlamingGuy=` (`AnimTypeClass::ReadINI` read @
    ///   `0x004282E2`, store @ `0x004282FC`, key string @ `0x818448`), and
    ///   retail `artmd.ini` sets it on `[FLAMEGUY]` alone — never on
    ///   `FIRE01/02/03`, the anims this trigger detaches.
    /// - The `+0x11B` frame-equality cleanup at `0x00423C03`..`0x00423C1D` is
    ///   dead code in gamemd. The byte is never written non-zero anywhere in
    ///   the image: its only AnimClass writers are `AnimClass::Constructor @
    ///   0x00421F5F` and `@ 0x004227A7`, both storing `BL` inside zero-init
    ///   runs dominated by `XOR EBX,EBX` (`0x00421EB6` / `0x004225FC`), plus
    ///   the `= 0` at `AnimClass::AI @ 0x00423C1D`.
    /// - The bouncer-impact block needs a bounce simulation, and there is none:
    ///   `Bouncer=` is parsed into `art_data.rs`'s `bouncer` flag with no sim
    ///   consumer, and damage-fire anims are not bouncers in any case.
    /// - The `+0x19D` writes cannot reach a `DrawIt` even in native — the same
    ///   call destroys the anim a few instructions later.
    /// - `UpdateLoopingSound` is the one item that really runs in native:
    ///   `[FIRE01]`/`[FIRE02]` carry `StartSound=BuildingFireBig` and
    ///   `[FIRE03]` `StartSound=BuildingFireMed`, so `AnimType+0x2F8 != -1`
    ///   holds. It is pure maintenance of a loop handle — revalidate, recompute
    ///   volume and pan through `VocClass::CalcVolumeAndPan`, stop the loop when
    ///   the volume comes back non-positive — and this engine has no loop-handle
    ///   mechanism to maintain (recorded on `audio/sfx.rs`), so there is nothing
    ///   for the extra pass to act on.
    ///
    /// - Trigger: a building destroyed while its damage-fire anims are live —
    ///   in this engine, `expire_anim_owner_reference` is the only producer of
    ///   the marked state.
    /// - Player effect: none today; audible once loop handles exist, and what
    ///   that final pass emits is the corpus's own open item, OQ-09 in
    ///   `ANIMCLASS_DETACHEDOWNER_MARKER_0X19B_CONSUMERS_GHIDRA_REPORT.md`.
    /// - Frequency: every destroyed building that had reached a damage-fire
    ///   threshold, so several times in an ordinary skirmish — with zero
    ///   observable output until loop handles exist.
    /// - Downstream risk: this is sequenced, not open-ended. It becomes
    ///   observable exactly when `GSI-15.03`'s loop-handle mechanism lands, and
    ///   the gate must move in the same slice that lands it. Only
    ///   `UpdateLoopingSound` has to be re-argued when that slice arrives; the
    ///   other four are settled above.
    ///
    /// Separately unmodelled, and not a consequence of the ordering above: the
    /// `Rules+0x147C` occupied-cell path at `0x00424322`..`0x0042435E` is a
    /// second writer of the marker — native re-reads coords, resolves the cell
    /// through `MapClass::Get_CellClass_At_Coord @ 0x00565730`, tests it via
    /// `0x0047C520`, and sets `+0x19B = 1` at `0x00424358`. A third writer, the
    /// `AnimType+0x360` path entered at `0x004243C2` and writing at
    /// `0x00424427` sits after the gate. `AnimType+0x360` is
    /// `IsAnimatedTiberium=`, and that writer is the one native path which
    /// routinely runs a full prefix in the marked state — unmodelled here only
    /// because `art_data.rs`'s `is_animated_tiberium`/`hide_if_no_ore` have no
    /// `sim/` consumer. Neither writer has an analogue in this store.
    ///
    /// DRIFT (GSI-05.12, structural) — `runtime.inactive` doubles as this
    /// store's "ready for pending delete" predicate
    /// (`world/lifecycle.rs pending_object_is_ready`), so the marker and object
    /// liveness are one field where native keeps display-layer membership
    /// separate from both.
    /// - Trigger: an attached anim whose owner is torn down first: a
    ///   building's damage fire, or a muzzle flash whose firer dies within the
    ///   flash's few frames.
    /// - Player effect: the flash disappears with the firer instead of playing
    ///   out its last frames where it stood.
    /// - Frequency: common in any firefight; a handful of frames each.
    /// - Downstream risk: recorded because splitting the two fields is a
    ///   prerequisite for any future producer whose anim survives its owner;
    ///   nothing else depends on it.
    pub(crate) fn expire_anim_owner_reference(&mut self, id: AnimId, expired_id: u64) -> bool {
        if self.anim(id).and_then(|anim| anim.owner_entity) != Some(expired_id) {
            return false;
        }
        self.lifecycle_outputs
            .push(LifecycleOutput::DisplayRemove { stable_id: id });
        self.detach_anim_from_owner(id);
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
        anim.remap_color = remap_color;
        anim.z_adjust = z_adjust;
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

    pub(crate) fn update_building_damage_fire(&mut self, building_id: u64, rules: &RuleSet) {
        let Some((current, type_ref, position, prior_state, category)) =
            self.substrate.entities.get(building_id).map(|entity| {
                (
                    entity.health.current,
                    entity.type_ref(),
                    entity.position.clone(),
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
        if let Some(entity) = self.substrate.entities.get_mut(building_id) {
            entity.damage_fire_state_active = active;
        }
        if !active {
            self.clear_building_damage_fire_slots(building_id);
            return;
        }

        let type_count = rules.general.damage_fire_types.len();
        if type_count == 0 {
            return;
        }
        let mut type_index = self
            .scenario_rng
            .next_range_u32_inclusive(0, type_count.saturating_sub(1) as u32)
            as usize;
        let offsets = rules
            .art_registry
            .get(&image)
            .map(|entry| entry.damage_fire_offsets.clone())
            .unwrap_or_default();
        let (foundation_w, foundation_h) =
            crate::rules::foundation::foundation_dimensions(&foundation);
        let foundation_sum = i32::from(foundation_w).wrapping_add(i32::from(foundation_h));
        let base_x = i32::from(position.rx)
            .wrapping_mul(LEPTONS_PER_CELL)
            .wrapping_add(position.sub_x.to_num::<i32>())
            .wrapping_sub(BUILDING_RENDER_ORIGIN_LEPTONS);
        let base_y = i32::from(position.ry)
            .wrapping_mul(LEPTONS_PER_CELL)
            .wrapping_add(position.sub_y.to_num::<i32>())
            .wrapping_sub(BUILDING_RENDER_ORIGIN_LEPTONS);
        // The building's own Z, so the attached fire's stored delta is zero
        // in Z, as it always was.
        let base_z = self
            .anim_owner_coords(building_id)
            .map_or(0, |owner| owner.z);

        for slot in 0..DAMAGE_FIRE_SLOT_COUNT {
            let occupied = self
                .substrate
                .entities
                .get(building_id)
                .and_then(|entity| entity.damage_fire_anim_ids[slot]);
            if occupied.is_some() {
                return;
            }
            let Some(offset) = offsets.get(slot).copied() else {
                return;
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
                x: base_x.wrapping_add(offset.world_dx),
                y: base_y.wrapping_add(offset.world_dy),
                z: base_z,
            };
            let anim_id = self
                .spawn_anim_at_world(rules, descriptor, world)
                .expect("validated stock damage-fire animation must spawn");
            self.set_anim_owner_object(anim_id, Some(building_id));
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

    pub(crate) fn clear_building_damage_fire_slots(&mut self, building_id: u64) {
        for slot in 0..DAMAGE_FIRE_SLOT_COUNT {
            let anim_id = self
                .substrate
                .entities
                .get(building_id)
                .and_then(|entity| entity.damage_fire_anim_ids[slot]);
            let Some(anim_id) = anim_id else {
                continue;
            };
            self.destroy_anim(anim_id);
            if let Some(entity) = self.substrate.entities.get_mut(building_id) {
                entity.damage_fire_anim_ids[slot] = None;
            }
        }
    }

    fn choose_anim_rate(&mut self, config: &AnimTypeRuntimeConfig) -> u16 {
        let delay = config
            .random_rate_logic_frames
            .map_or(config.rate_logic_frames, |(a, b)| {
                self.scenario_rng
                    .next_range_u32_inclusive(u32::from(a), u32::from(b)) as u16
            });
        if config.normalized {
            self.session.game_options.normalized_anim_delay(delay)
        } else {
            delay
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
    /// The particle/scorch/crater half (`0x00424F00`) is not wired — see the
    /// module header.
    fn anim_start(&mut self, id: AnimId, config: &AnimTypeRuntimeConfig) {
        let sound_name = config
            .start_sound
            .as_ref()
            .or(config.report.as_ref())
            .cloned();
        let Some(sound_name) = sound_name else {
            return;
        };
        let sound_id = self.interner.intern(&sound_name);
        let Some(world) = self.anim_absolute_coord(id) else {
            return;
        };
        if let Some(anim) = self.anim_mut_by_id(id) {
            anim.start_sound_active = true;
        }
        self.sound_events.push(SimSoundEvent::AnimationStarted {
            anim_id: id,
            sound_id,
            world,
        });
    }

    fn switch_anim_type(&mut self, id: AnimId, next: &str, rules: &RuleSet) {
        let Some(config) = rules.art_registry.anim_runtime_config(next).cloned() else {
            self.destroy_anim(id);
            return;
        };
        let Ok((effective_end, effective_loop_end)) = effective_bounds(next, &config) else {
            self.destroy_anim(id);
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
        self.anim_start(id, &config);
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
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;

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
        let mut rules = RuleSet::from_ini(&rules_ini).expect("damage-fire rules");
        let art_ini = IniFile::from_str(
            "[TESTART]\nFoundation=4x4\nDamageFireOffset0=-24,-1\nDamageFireOffset1=64,36\n\n\
             [FIRE01]\nRate=450\nLoopCount=-1\nStartSound=BuildingFireBig\n\n\
             [FIRE02]\nRate=450\nLoopCount=-1\nStartSound=BuildingFireMed\n\n\
             [FIRE03]\nRate=450\nLoopCount=-1\nStartSound=BuildingFireSmall\n",
        );
        let mut art = ArtRegistry::from_ini(&art_ini);
        art.bind_anim_frame_count_for_test("FIRE01", 30);
        art.bind_anim_frame_count_for_test("FIRE02", 64);
        art.bind_anim_frame_count_for_test("FIRE03", 30);
        rules.merge_art_data(&art);
        rules.art_registry = art;

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
        rules.art_registry = art;
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

    #[test]
    fn native_anim_boundary_and_reset_vectors() {
        let golden: serde_json::Value =
            serde_json::from_str(include_str!("../../tools/anim_oracle/boundary.json")).unwrap();
        let rules = runtime_rules("[TEST]\nEnd=64\n", &[("TEST", 64)]);
        let mut sim = Simulation::new();
        let type_id = sim.interner.intern("TEST");
        let id = sim
            .spawn_anim_object(&rules, runtime_descriptor(type_id, 0))
            .unwrap();
        let template = sim.anim(id).unwrap().clone();
        let mut config = rules
            .art_registry
            .anim_runtime_config("TEST")
            .unwrap()
            .clone();
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
                .art_registry
                .anim_runtime_config("DEFAULT")
                .unwrap()
                .make_infantry,
            -1
        );
        assert_eq!(
            rules
                .art_registry
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
        let report = sim.interner.intern("EXPLOSION06");
        assert!(
            sim.sound_events.iter().any(|event| matches!(
                event,
                SimSoundEvent::AnimationStarted { anim_id, sound_id, .. }
                    if *anim_id == id && *sound_id == report
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

    /// A producer can name art that resolves to no INI section — retail has
    /// three (`MININUKE - ADDED 11/30` from `[CRNUKEWH] AnimList=`, plus
    /// `GTPOWEXP` and `TSTLEXP` from the faction power plants' `Explosion=`).
    /// Native mints a default AnimType, but `AnimTypeClass::ReadINI @
    /// 0x00427D00` bails on the absent section before the image loader, so
    /// `End` stays 0 and the anim dies unseen. VERA must decline the spawn
    /// rather than panic or block the shot. Full residual on
    /// `ArtRegistry::bind_anim_class_assets`.
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
            .spawn_load_anim_at_world(&rules.art_registry, first, world, 1_010_001)
            .expect("first authored load Anim");
        let keep_id = sim
            .spawn_anim_object(&rules, runtime_descriptor(keep_type, 0))
            .expect("ordinary retained Anim");
        let mut second = runtime_descriptor(load_type, 0);
        second.terrain_attached = true;
        second.use_cell_drawer = true;
        let second_id = sim
            .spawn_load_anim_at_world(&rules.art_registry, second, world, 1_010_002)
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
                .filter(|event| matches!(
                    event,
                    SimSoundEvent::AnimationStopped {
                        stop_sound_id: None,
                        ..
                    }
                ))
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
        sim.destroy_anim(id);
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
            7,
            8,
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
            7,
            8,
            0x10,
            416,
            0,
            false,
            AnimOccupationOperation::Clear,
        );
        assert_eq!(grid.deck_bits(7, 8), 0);

        apply_anim_raw_occupation(
            &mut grid,
            7,
            8,
            0x10,
            416,
            0,
            false,
            AnimOccupationOperation::Mark,
        );
        assert_eq!(grid.ground_bits(7, 8), 0x10);
        apply_anim_raw_occupation(
            &mut grid,
            7,
            8,
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
        sim.destroy_anim(id);
        assert!(sim.anim(id).unwrap().runtime.inactive);
        assert!(!sim.live_object_order_snapshot().contains(&id));
        assert_eq!(
            sim.sound_events
                .iter()
                .filter(|event| matches!(event, SimSoundEvent::AnimationStopped { .. }))
                .count(),
            1,
        );
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

        sim.for_each_live_object(|sim, id| sim.visit_anim(id, &rules, None));

        let order = sim.live_object_order_snapshot();
        assert_eq!(order.len(), 2);
        assert_eq!(order[0], parent);
        let child = sim.anim(order[1]).unwrap();
        assert_eq!(sim.interner.resolve(child.type_id), "CHILD");
        assert!(!child.runtime.first_ai_guard);
        assert_eq!(child.runtime.current_frame, 0);
    }

    #[test]
    fn multiplayer_feedback_uses_sync_exempt_registry_without_global_id_or_logic_membership() {
        let rules = runtime_rules("[RING]\nRate=900\nEnd=1\nLoopCount=1\n", &[("RING", 1)]);
        let mut sim = Simulation::new();
        let next_global_id = sim.substrate.next_stable_object_id;
        let id = sim
            .spawn_multiplayer_feedback_anim_at_world(
                &rules,
                AnimWorldCoord {
                    x: 512,
                    y: 768,
                    z: 32,
                },
            )
            .unwrap();

        assert_eq!(sim.substrate.next_stable_object_id, next_global_id);
        assert!(!sim.substrate.anims.contains_key(id));
        assert!(sim.live_object_order_snapshot().is_empty());
        let anim = sim.anim(id).unwrap();
        assert_eq!(anim.native_unique_id, SYNC_EXEMPT_NATIVE_UNIQUE_ID);
        assert_eq!(anim.z_adjust, MULTIPLAYER_FEEDBACK_Z_ADJUST);
        assert!(!anim.in_logic_vector);
        let hash_with_feedback = sim.state_hash();
        let feedback = sim.substrate.multiplayer_feedback_anims.remove(id).unwrap();
        assert_eq!(sim.state_hash(), hash_with_feedback);
        assert!(
            sim.substrate
                .multiplayer_feedback_anims
                .insert(feedback)
                .is_none()
        );

        sim.for_each_multiplayer_feedback_anim(|sim, id| sim.visit_anim(id, &rules, None));
        assert!(!sim.anim(id).unwrap().runtime.first_ai_guard);
        sim.session.binary_frame = 1;
        sim.for_each_multiplayer_feedback_anim(|sim, id| sim.visit_anim(id, &rules, None));
        assert!(sim.anim(id).unwrap().runtime.inactive);
        assert_eq!(sim.substrate.multiplayer_feedback_pending_delete, vec![id]);

        sim.process_pending_delete();
        assert!(sim.anim(id).is_none());
        assert!(sim.substrate.multiplayer_feedback_pending_delete.is_empty());
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
        assert_eq!(sim.anim(anim_id).unwrap().owner_entity, Some(building_id));

        assert!(sim.expire_anim_owner_reference(anim_id, building_id));
        assert!(
            sim.substrate
                .entities
                .get(building_id)
                .unwrap()
                .damage_fire_anim_ids[0]
                .is_none()
        );
        assert!(sim.anim(anim_id).unwrap().runtime.inactive);
        assert!(sim.live_object_order_snapshot().contains(&anim_id));
        assert!(!sim.substrate.pending_delete.contains(&anim_id));

        sim.visit_anim(anim_id, &rules, None);
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
        sim.anim_mut_by_id(anim_id).unwrap().runtime.inactive = true;
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
        assert_eq!(sim.detach_anim_from_owner(anim_id), Some(building_id));
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
        assert_eq!(first_anim.owner_entity, Some(building_id));
        assert_eq!(
            sim.interner.resolve(first_anim.type_id),
            type_names[expected_types[0]]
        );
        assert_eq!(first_anim.runtime.current_frame, expected_frames[0]);
        // `AnimClass::SetOwnerObject @ 0x00424B50` stores the coordinate
        // owner-relative; `GetCoords @ 0x00422BE0` resolves it back. The
        // absolute is what the draw and the sound see, and it is unchanged.
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
                x: 2450 - 3072,
                y: 2653 - 3072,
                z: 0
            },
            "stored coordinate is the owner-relative delta native writes"
        );
        assert_eq!(first_anim.z_adjust, -192);
        let second_anim = sim.anim(second).unwrap();
        assert_eq!(second_anim.owner_entity, Some(building_id));
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
                x: 3140 - 3072,
                y: 2594 - 3072,
                z: 0
            },
            "stored coordinate is the owner-relative delta native writes"
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
                .filter(|event| matches!(event, SimSoundEvent::AnimationStopped { .. }))
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
