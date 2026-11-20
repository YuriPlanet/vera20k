//! Scheduler-owned ordinary SHP animation objects.
//!
//! `AnimStore` owns animation storage while `world::LogicVector` owns live AI
//! order. This module implements the native-derived AnimClass lifecycle:
//! constructor/reveal, first-AI guard, start delay and its `Start` edge,
//! `Middle`'s scorch and crater, logic-frame timing, loops, reverse/ping-pong,
//! Next, trailer, sound identity, owner attachment, conceal, deferred
//! deletion, and the `Bouncer=` chunk: its constructor launch, its
//! `BounceClass` flight and its landing (`Simulation::anim_bounce_step`). Its
//! producers are building slots and damage fires, tile and crate animations,
//! combat explosions, death debris, weapon and occupant muzzle flashes,
//! parachute canopies, teleport warps, superweapon invokes, Lightning Storm
//! bolts, bridge collapse explosions, wakes, ore twinkles and TAction58 meteor
//! showers (including their landing children and ore).
//!
//! ## Residuals
//!
//! Ordinary damage runs after a due stage advance and before Middle/boundary
//! through the shared area receiver (AnimAI424513..424654). Authored Overlay
//! CellAnim provides a live FIRE3 producer; its stock ART Damage is .003.
//! No common unchanged-stock ordinary positive-Damage producer is established.
//! Terrain never burns in YR, so this needs no fire chain. `TerrainClass::
//! Catch_Fire @ 0x0071C5B0`, the only IsOnFire writer, sits in vtable slot
//! +0x178 (0x007F53A4) with no virtual call found. `Take_Damage @ 0x0071B920`
//! uninits a terrain object on a lethal hit, and `Finish_Fire_Death @
//! 0x0071C6B0` (from AnimClass 0x00422955/0x00424BB0) returns while IsOnFire is
//! clear. Ares 3.0 makes trees burn (`sim::terrain_fire`); its fire anims
//! are owned by the tree (`AnimObject::owner_terrain`).

use crate::sim::world::FrameEffects;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::art_data::AnimTypeRuntimeConfig;
use crate::rules::house_colors::HouseColorIndex;
use crate::rules::overlay_types::TiberiumImageError;
use crate::rules::ruleset::RuleSet;
use crate::sim::bounce::{BounceOutcome, BounceState};
use crate::sim::components::{AnimClassSpawnDescriptor, DriveCoord};
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::{RawCellKey, RawCellOccupationGrid, infantry_raw_occupation_mask};
use crate::sim::overlay_mark_host::{OverlayRecalc, RuntimeOverlayError};
use crate::sim::rng::SimRng;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::timer::CdTimer;
use crate::sim::touch_log::{TouchLogs, Touched};
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::{SimSoundEvent, Simulation};
use crate::util::fixed_math::SimFixed;
use crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS;
use crate::util::lookup_hash::LookupMap;
use crate::util::native_x87::{NativeF64Bits, NativeX87Error, X87Chop53};

pub type AnimId = u64;

mod flaming_guy;

#[cfg(all(test, tests_sim_a_b))]
#[path = "anim_animated_tiberium_tests.rs"]
mod animated_tiberium_tests;
#[cfg(all(test, tests_sim_a_b))]
#[path = "anim_vein_attack_tests.rs"]
mod vein_attack_tests;

#[cfg(all(test, tests_sim_a_b))]
#[path = "anim_firing_update_tests.rs"]
mod anim_firing_update_tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AnimWorldCoord {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl From<crate::sim::components::DriveCoord> for AnimWorldCoord {
    fn from(coord: crate::sim::components::DriveCoord) -> Self {
        Self {
            x: coord.x,
            y: coord.y,
            z: coord.z,
        }
    }
}

impl From<crate::sim::projectile::ProjectileCoord> for AnimWorldCoord {
    fn from(coord: crate::sim::projectile::ProjectileCoord) -> Self {
        Self {
            x: coord.x,
            y: coord.y,
            z: coord.z,
        }
    }
}

impl AnimWorldCoord {
    /// Decompose the absolute lepton coordinate into (cell, sub-cell,
    /// height level). The level is the floor of Z over the native level height
    /// and is for consumers that key on a level (depth rows, lighting); a
    /// screen position comes from the exact `z` field, which may sit between
    /// levels (a muzzle, an airburst).
    pub fn to_cell_sub_z(
        self,
    ) -> (
        u16,
        u16,
        crate::util::fixed_math::SimFixed,
        crate::util::fixed_math::SimFixed,
        u8,
    ) {
        let (rx, sub_x) = crate::util::lepton::split_lepton_axis(self.x);
        let (ry, sub_y) = crate::util::lepton::split_lepton_axis(self.y);
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
            x: crate::util::lepton::absolute_lepton_axis(rx, sub_x),
            y: crate::util::lepton::absolute_lepton_axis(ry, sub_y),
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

#[cfg(any(test, feature = "test-support"))]
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

/// `static_cast<int>(cells * Unsorted::LeptonsPerCell)`, the fire-spawn
/// distance conversion (`Ext/Anim/Body.cpp:369,371`): truncation toward zero.
fn leptons_of_cells(cells: f64) -> i32 {
    (cells * f64::from(crate::util::lepton::LEPTONS_PER_CELL_I32)) as i32
}

/// `AnimClass_Update` @ 0x00423f37: landing consumes two inclusive rolls.
pub(crate) fn bounce_spawn_count(
    has_spawns: bool,
    spawn_count: i32,
    roll_a: i32,
    roll_b: i32,
) -> i32 {
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
    /// Type `+0x368`, art.ini `DoubleThick=` (`AnimTypeClass::ReadINI @
    /// 0x00427D00`), read only by the `+0x119` arm: `OR EBX,0x6` when set,
    /// `OR EBX,0x4` when clear.
    pub double_thick: bool,
    pub translucency_detail_level: i32,
    pub game_detail_level: i32,
    pub translucent_ramp: bool,
    pub current_frame: i32,
    /// AnimType `End` (`+0x2C0`), which the `Translucent=` ladder divides
    /// (`0x004230AC`); see `AnimTypeRuntimeConfig::loaded_end`.
    pub end_frame: i32,
    /// Stock `Translucency=` integer level. A build without Phobos has no
    /// `Animatable`, so the key stays the plain value the draw arm maps.
    pub explicit_translucency: i32,
    /// Phobos `Translucency.Get(currentFrame / End)` for this frame
    /// (`Ext/Anim/Hooks.cpp:601-605`, hook `0x423183`). `None` is native
    /// `BlitterFlags::None`, which keeps the stock ladder.
    pub translucency_level: Option<u8>,
    /// Phobos `Translucency.Cloaked.Get(...)` for this frame (`:588-599`),
    /// consulted only while [`Self::cloaked_translucency_answers`].
    pub translucency_cloaked_level: Option<u8>,
    /// The cloaked track has values and the owner is Cloaking or Cloaked, so
    /// the plain track is not consulted even when the cloaked answer is
    /// `None` — native's `hasValue` (`:588-605`).
    pub cloaked_translucency_answers: bool,
    pub instance_ramp: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimTranslucencyResult {
    pub draw: bool,
    pub flags: u32,
}

impl AnimTranslucencyInput {
    /// The draw's translucency input for one frame, from the resolved anim-type
    /// config the draw already holds.
    ///
    /// `AnimClass_DrawIt_Translucency` (`0x423183`, `Ext/Anim/Hooks.cpp:583-605`)
    /// queries each track at `currentFrame / End`, `End` being the animation
    /// type's frame count
    /// ([`AnimTypeRuntimeConfig::loaded_end`](crate::rules::art_data::AnimTypeRuntimeConfig::loaded_end)).
    /// `End == 0` divides by zero exactly as native does: frame 0 yields
    /// `0/0` (`NaN`, which matches no keyframe, so the stock ladder runs) and
    /// any later frame yields infinity, which reads the last keyframe.
    pub fn new(
        config: Option<&crate::rules::art_data::AnimTypeRuntimeConfig>,
        base_flags: u32,
        current_frame: i32,
        shp_frame_count: i32,
        game_detail_level: i32,
        draw_runtime: AnimDrawRuntime,
        owner_cloaked: bool,
    ) -> Self {
        let end_frame = config.map_or(shp_frame_count, |value| value.loaded_end(shp_frame_count));
        let percentage = f64::from(current_frame) / f64::from(end_frame);
        // Without Phobos there is no cloaked track to gate.
        Self {
            base_flags,
            forced_translucent: draw_runtime.forced_translucent,
            double_thick: config.is_some_and(|value| value.double_thick),
            translucency_detail_level: config.map_or(0, |value| value.translucency_detail_level),
            game_detail_level,
            translucent_ramp: config.is_some_and(|value| value.translucent),
            current_frame,
            end_frame,
            explicit_translucency: config.map_or(0, |value| value.translucency),
            translucency_level: config.and_then(|value| value.phobos_translucency.get(percentage)),
            translucency_cloaked_level: config
                .and_then(|value| value.phobos_translucency_cloaked.get(percentage)),
            cloaked_translucency_answers: owner_cloaked
                && config.is_some_and(|value| value.phobos_translucency_cloaked.has_values()),
            instance_ramp: i32::from(draw_runtime.translucency_ramp),
        }
    }
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
    // `0x00423061..0x0042307A` ORs the `+0x119` bits and falls through.
    if input.forced_translucent {
        flags |= if input.double_thick { 6 } else { 4 };
    }
    if input.translucency_detail_level > input.game_detail_level {
        return AnimTranslucencyResult { draw: true, flags };
    }
    if input.translucent_ramp {
        if input.instance_ramp >= 15 {
            return AnimTranslucencyResult { draw: false, flags };
        }
        // `0x004230A8..0x004230FB`: the stage against End times the doubles
        // 0.6, 0.4 and 0.2 (`0x007E3558`, `0x007E3550`, `0x007E3548`) on the
        // x87 in chop-53. 0.6 rounds below three fifths, so with End a
        // multiple of 5 the stage at three fifths already takes 75%.
        use crate::util::native_x87::{NativeF64Bits, X87Chop53, X87Ordering};
        let stage = X87Chop53::load_i32(input.current_frame);
        let end = X87Chop53::load_i32(input.end_frame);
        let above = |factor: u64| {
            X87Chop53::load_f64(NativeF64Bits::from_bits(factor)).is_ok_and(|factor| {
                X87Chop53::compare(stage, X87Chop53::mul(end, factor)) == X87Ordering::Greater
            })
        };
        flags |= if above(0x3FE3_3333_3333_3333) {
            6
        } else if above(0x3FD9_9999_9999_999A) {
            4
        } else if above(0x3FC9_9999_9999_999A) {
            2
        } else {
            0
        };
        return AnimTranslucencyResult { draw: true, flags };
    }
    // Phobos `0x423183`: the airborne `Translucency` track replaces the stock
    // level. A cloaked Techno owner answers from `Translucency.Cloaked` and
    // never consults the plain track, even when that answer is `None`
    // (`hasValue`, `:588-605`). A `None` answer keeps the stock ladder below.
    {
        let level = if input.cloaked_translucency_answers {
            input.translucency_cloaked_level
        } else {
            input.translucency_level
        };
        if let Some(level) = level {
            return AnimTranslucencyResult {
                draw: input.instance_ramp < 15,
                flags: flags | translucency_flag(i32::from(level)),
            };
        }
    }
    // A build without Phobos has no `Animatable`: the authored integer is the
    // level, and any other value keeps the stock ladder.
    if input.explicit_translucency > 0 {
        return AnimTranslucencyResult {
            draw: input.instance_ramp < 15,
            flags: flags | translucency_flag(input.explicit_translucency),
        };
    }
    // `0x004231C1..0x004231EE`: a ramp of 1-5 draws at 25%, 6-15 at 50%,
    // and anything above 15 not at all.
    match input.instance_ramp {
        16.. => return AnimTranslucencyResult { draw: false, flags },
        6..=15 => flags |= 4,
        1..=5 => flags |= 2,
        _ => {}
    }
    AnimTranslucencyResult { draw: true, flags }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AnimRuntime {
    pub current_frame: i32,
    pub frame_step: i32,
    /// AnimClass +0x184, a signed int: the constructor's delay or a drawn
    /// `RandomLoopDelay=` pause.
    pub delay_remaining: i32,
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
    /// Anim+11A: a building's Temporal erase freezes its slot anims
    /// ([`AnimObject::set_temporal_erase`], [`AnimObject::temporal_pause`]).
    /// AI stops on it just as on +19E (`0x004244A9`); the frame timer keeps
    /// running.
    #[serde(default)]
    pub temporal_paused: bool,
}

/// Per-instance `AnimClass::DrawIt` bytes that are independent of the art
/// type. They remain serialized simulation state because native effects may
/// set them between frames; presentation only reads this resolved input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AnimDrawRuntime {
    /// AnimClass `+0x19d`: unconditional draw suppression.
    pub hidden: bool,
    /// AnimClass `+0x199`, the fogged byte. DrawIt skips the anim when it is
    /// set and the type's `ShouldFogRemove=` (`+0x374`, `0x004282DC`) is too
    /// (`0x00423014..0x00423026`). Presentation ports that gate with the fog
    /// query (`instances::overlays`), so nothing sets this field.
    pub special_hidden: bool,
    /// AnimClass `+0x178`: ramp/age input used by translucency selection.
    pub translucency_ramp: u8,
    /// AnimClass `+0x119`: draw at 50%, or 75% for a `DoubleThick=` type
    /// (`AnimClass::DrawIt` reads type `+0x368` at `0x0042306B`).
    pub forced_translucent: bool,
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
    /// Techno705D70's loaded `+0xDF0` group and house colour, frozen when
    /// the producer assigns the Convert. The palette name survives the
    /// source building's destruction and distinguishes simultaneous groups.
    ExtraBuildingScheme {
        house: InternedId,
        palette: InternedId,
    },
}

fn one_f64_bits() -> NativeF64Bits {
    NativeF64Bits::ONE
}

/// `Phobos AnimExt::FiringAnim_*` (`Ext/Anim/Body.h`): the muzzle anim a
/// `[WeaponType] Anim=` list produced keeps following its firer, and every
/// firing direction re-picks its own type (`Ext/Anim/Body.cpp:72-101`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FiringAnimState {
    /// `FiringAnim_Weapon`: the weapon whose `Anim=` list supplied the type.
    pub weapon: InternedId,
    /// `FiringAnim_WeaponIndex`: which barrel of the firer fired it.
    pub weapon_index: u8,
    /// `FiringAnim_BurstIndex`: the burst frame the muzzle came from; the
    /// anchor restores it only while it asks `GetFLH`.
    pub burst_index: u8,
    /// `FiringAnim_LastFacing`: the owner facing the current type was picked
    /// for; an unchanged facing does not re-pick.
    pub last_facing: u16,
    /// `FiringAnim_LastCoords`: the owner render coord the muzzle was last
    /// anchored to; an unchanged coord and facing does not re-anchor.
    pub last_coords: AnimWorldCoord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnimObject {
    stable_id: AnimId,
    native_unique_id: i32,
    type_id: InternedId,
    /// World leptons — but **owner-relative whenever `owner_entity` is set**,
    /// exactly as `AnimClass::SetOwnerObject @ 0x00424B50` stores it. Read it
    /// through [`Simulation::anim_absolute_coord`] for a world position; read
    /// the field directly only where native reads the stored `ObjectClass`
    /// coordinate directly: the multiplayer sync checksum
    /// (`Compute_Game_Sync_Checksum @ 0x0064DAB0` folds `+0x9c`/`+0xa0`),
    /// HideIfNoOre and post-load Tiberium conversion4261D0. All three axes
    /// are world leptons.
    world_coord: AnimWorldCoord,
    draw_flags: u32,
    z_adjust: i32,
    /// Optional ConvertClass palette selected by a producer after construction
    /// (for example OverlayClass CellAnim over a Tiberium cell).
    #[serde(default)]
    remap: Option<AnimRemap>,
    effective_end: i32,
    effective_loop_end: i32,
    runtime: AnimRuntime,
    draw_runtime: AnimDrawRuntime,
    /// Per-viewer snapshots of native Anim+19D, for a producer whose byte
    /// depends on the native client's PlayerPtr. Every ordinary hidden-byte
    /// writer replaces this with a uniform value.
    #[serde(default)]
    viewer_hidden: Option<AnimViewerHidden>,
    /// AnimClass `+0x196`: use the containing CellClass draw/palette authority.
    #[serde(default)]
    use_cell_drawer: bool,
    /// AnimClass `+0x197`: created from a terrain tile animation descriptor.
    #[serde(default)]
    terrain_attached: bool,
    /// Anim+198. Constructor421FF1 clears it; TAction416E2368 sets it
    /// only after constructor-time Start. Start424CF1/424D5F, AI423AC6
    /// and UnInit4255DA suppress effects, independently of drawing/Middle.
    #[serde(default)]
    no_effects: bool,
    /// Present only on a muzzle anim `Anim.Update` keeps following its firer
    /// (`Ext/Anim/Hooks.cpp:714-740`); every other anim leaves it None.
    #[serde(default)]
    firing_anim: Option<FiringAnimState>,
    /// Anim+188. Normal ctor421FA4/421FC8 seeds1; default ctor4227FD/
    ///42280F and Next424877/424885 seed0. Load425280 retains saved bytes.
    /// Binary64 keeps the reader's fractional precision and full accumulator
    /// range, including a NoEffects backlog, beyond SimFixed's16.16 range.
    /// Basic IEEE operations are deterministic; NaN stores are canonicalized.
    /// https://doc.rust-lang.org/std/primitive.f64.html
    /// Precision residual: round-to-nearest replaces native PC53/chop. Near
    /// an integer threshold a long-running fire can emit on a different visit.
    damage_accumulator: NativeF64Bits,
    /// Phobos `AnimExt::FirepowerMult` (`Ext/Anim/Body.h:41,62`, constructed
    /// 1.0): the invoker's firepower multiplier cached on the first damage
    /// round (`Ext/Anim/Hooks.cpp:119-128`); `Damage.ApplyFirepowerMult`
    /// scales each damage application by it.
    #[serde(default = "one_f64_bits")]
    phobos_firepower_mult: NativeF64Bits,
    /// LogicClass membership is reconstructed from the serialized vector.
    /// ObjectClass::Save does not persist its local membership byte.
    #[serde(skip)]
    in_logic_vector: bool,
    owner_entity: Option<u64>,
    /// Derived reverse index of Building+55C's slot reference, rebuilt on load.
    /// Independent of both Anim+118 and the Anim+CC Object-owner attachment.
    #[serde(skip)]
    building_slot: Option<(u64, u8)>,
    /// Anim+118: ctor421F44 clears it; Building45199B sets it. It suppresses
    /// independent drawing and admits451B40, even after the slot is cleared.
    #[serde(default)]
    building_anim: bool,
    /// Derived reverse index of Building+5C8's damage-fire slot, rebuilt on
    /// load. It survives owner expiry until the Anim's own UnInit broadcast.
    #[serde(skip)]
    damage_fire_slot: Option<(u64, u8)>,
    /// Anim+179: constructor421FB0 clears it; normal AI completion424B31
    /// sets it before UnInit. Building451B40 distinguishes a completed
    /// retraction from an animation removed for another reason.
    #[serde(default)]
    completed: bool,
    start_sound_active: bool,
    stop_sound_id: Option<InternedId>,
    display: AnimDisplayState,
    /// `AnimClass+0x128`: a `Bouncer=` or `IsMeteor=` chunk's BounceClass
    /// body, both built by [`anim_constructor_draws`]. Its presence is `+0x194`.
    #[serde(default)]
    bounce: Option<BounceState>,
    /// Anim+108/+114/+19A and Object falling words for IsFlamingGuy AI.
    #[serde(default)]
    flaming_guy: Option<flaming_guy::FlamingGuyState>,
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
    /// Phobos `AnimExt::Invoker` (`Ext/Anim/Body.h:28`): the Techno whose
    /// `SetInvoker` named this anim. `InvalidateTechnoPointers`
    /// (`Body.cpp:497`) clears it when that Techno is destroyed; VERA keeps
    /// the identity and a stable-id lookup answers absent instead.
    #[serde(default)]
    invoker: Option<u64>,
    /// Phobos `AnimExt::InvokerHouse` (`Ext/Anim/Body.h:29`).
    #[serde(default)]
    invoker_house: Option<InternedId>,
    /// Phobos `AnimExt::FromDeathUnit` (`Ext/Anim/Body.h`): the anim came
    /// from `AnimTypeExt::ProcessDestroyAnims` (`Ext/AnimType/Body.cpp:55`).
    #[serde(default)]
    from_death_unit: bool,
    /// Phobos `AnimExt::DeathUnitFacing`: the destroyed unit's body direction.
    #[serde(default)]
    death_unit_facing: Option<u8>,
    /// Phobos `AnimExt::DeathUnitHasTurret`.
    #[serde(default)]
    death_unit_has_turret: bool,
    /// Phobos `AnimExt::DeathUnitTurretFacing`.
    #[serde(default)]
    death_unit_turret_facing: Option<u8>,
    /// VERA-internal: the `CreateUnit` actual-effects arm already ran.
    #[serde(default)]
    create_unit_fired: bool,
    /// `AnimClass+0xCC` when it names a `TerrainClass`: a tree's fire, which
    /// only the Ares terrain fire attaches (`sim::terrain_fire`). VERA keeps
    /// a terrain owner apart from `owner_entity` and its stored coordinate
    /// absolute: a terrain object never moves, so native's owner-relative
    /// form adds the same constant back on every read.
    #[serde(default)]
    owner_terrain: Option<u64>,
    /// VERA-only: `Some` for a presentation anim
    /// ([`AnimConstructorInput::Presentation`]), holding the layer it draws in
    /// while revealed, where an ordinary anim would hold a Display slot. A
    /// presentation anim is local, so it is never saved, compared or hashed.
    #[serde(skip)]
    presentation: Option<Option<crate::sim::world::display_layers::DisplayLayer>>,
    /// VERA-only: a presentation anim's place in Display submission order,
    /// taken when it was last placed in its layer
    /// (`DisplayLayers::unshared_order`), where an ordinary anim's slot
    /// would hold it.
    #[serde(skip)]
    presentation_order: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct AnimViewerHidden {
    viewers: Vec<(InternedId, bool)>,
    other_hidden: bool,
}

impl AnimObject {
    /// Whether this is a presentation anim ([`AnimConstructorInput::Presentation`]).
    pub fn is_presentation(&self) -> bool {
        self.presentation.is_some()
    }

    /// The layer a revealed presentation anim draws in; `None` for an
    /// ordinary anim, which Display places, or a concealed one.
    pub fn presentation_layer(&self) -> Option<crate::sim::world::display_layers::DisplayLayer> {
        self.presentation.flatten()
    }

    /// A presentation anim's place in Display submission order
    /// (`Simulation::picture_draw_order`).
    pub fn presentation_order(&self) -> u64 {
        self.presentation_order
    }

    /// Read the same hidden byte for a particular client without making
    /// simulation state depend on which house this process is displaying.
    pub fn hidden_for(&self, viewer: Option<InternedId>) -> bool {
        if let Some(snapshot) = &self.viewer_hidden {
            return viewer
                .and_then(|viewer| snapshot.viewers.iter().find(|(id, _)| *id == viewer))
                .map_or(snapshot.other_hidden, |(_, hidden)| *hidden);
        }
        self.draw_runtime.hidden
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn draw_runtime_mut_for_test(&mut self) -> &mut AnimDrawRuntime {
        &mut self.draw_runtime
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_building_slot_for_test(&mut self, value: Option<(u64, u8)>) {
        self.building_slot = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_in_logic_vector_for_test(&mut self, value: bool) {
        self.in_logic_vector = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_start_sound_active_for_test(&mut self, value: bool) {
        self.start_sound_active = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_terrain_attached_for_test(&mut self, value: bool) {
        self.terrain_attached = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn runtime_mut_for_test(&mut self) -> &mut AnimRuntime {
        &mut self.runtime
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn runtime_frame_for_test(&self) -> i32 {
        self.runtime.current_frame
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn world_coord_mut_for_test(&mut self) -> &mut AnimWorldCoord {
        &mut self.world_coord
    }

    /// LogicClass membership: RegisterObject sets it (`0x0055BAC6`),
    /// UnregisterObject clears it (`0x0055BB27`). Only the logic vector's
    /// owner writes it, and a load rebuilds it from the saved vector.
    pub(crate) fn mark_logic_membership(&mut self, member: bool) {
        self.in_logic_vector = member;
    }

    /// Building43F738 moves a slot anim to its building's new coordinate;
    /// it neither reconstructs the anim nor resets its timers.
    pub(crate) fn move_with_building(&mut self, coord: AnimWorldCoord) {
        self.world_coord = coord;
    }

    /// The anim takes slot `slot` of `building` (Building+55C), with the
    /// slot config's `ZAdjust=`. The reverse index is derived from that slot.
    pub(crate) fn enter_building_slot(&mut self, building: u64, slot: u8, z_adjust: i32) {
        self.building_slot = Some((building, slot));
        self.z_adjust = z_adjust;
    }

    /// The reverse index of the building slot that holds this anim, rebuilt
    /// from Building+55C on load.
    pub(crate) fn index_building_slot(&mut self, building: u64, slot: u8) {
        self.building_slot = Some((building, slot));
    }

    /// The building slot leaves the anim: the index is cleared and returned.
    pub(crate) fn take_building_slot(&mut self) -> Option<(u64, u8)> {
        self.building_slot.take()
    }

    /// The reverse index of the Building+5C8 damage-fire slot that holds
    /// this anim, rebuilt on load.
    pub(crate) fn index_damage_fire_slot(&mut self, building: u64, slot: u8) {
        self.damage_fire_slot = Some((building, slot));
    }

    /// Drop both derived slot indexes before a rebuild, or when the
    /// building releases the anim.
    pub(crate) fn clear_slot_indexes(&mut self) {
        self.building_slot = None;
        self.damage_fire_slot = None;
    }

    /// Anim pause (`0x00425260`): only Anim+19E changes; the absolute frame
    /// timer keeps running.
    pub(crate) fn pause(&mut self) {
        self.runtime.paused = true;
    }

    /// Anim resume (`0x00425270`).
    pub(crate) fn resume(&mut self) {
        self.runtime.paused = false;
    }

    /// Anim+11A `UnderTemporal` ("temporal'd building's active anims"): freeze
    /// the frame advance without the erase's translucent draw (+119). Phobos
    /// `ShieldClass::AI_Temporal`/`OnlineCheck` write it and `TemporalCheck`
    /// clears it (`src/New/Entity/ShieldClass.cpp:398..437,585..604`); the AI
    /// stops on it exactly as on +19E (`0x004244A9`).
    pub(crate) fn set_under_temporal(&mut self, under: bool) {
        self.runtime.temporal_paused = under;
    }

    /// A building's Temporal erase on one slot anim: `0x004521C0` freezes it
    /// (+11A) and draws it translucent (+119); `0x00452210` clears both.
    ///
    /// RESIDUAL: the erase also saves the frame (+11C) and the release sets
    /// +11B, which AI (`0x00423C03..0x00423C1D`) clears once the frame comes
    /// back round to the saved one. Nothing else reads either, so neither is
    /// kept.
    pub(crate) fn set_temporal_erase(&mut self, erased: bool) {
        self.runtime.temporal_paused = erased;
        self.draw_runtime.forced_translucent = erased;
    }

    /// `0x00451B25`: a slot anim created while its building is being erased
    /// starts frozen (+11A), but opaque.
    pub(crate) fn temporal_pause(&mut self) {
        self.runtime.temporal_paused = true;
    }

    /// A building writes the anim's current frame: a slot successor keeps
    /// its predecessor's frame, and SiloDamage (`0x00450D81`) shows the
    /// storage tier.
    pub(crate) fn jump_to_frame(&mut self, frame: i32) {
        self.runtime.current_frame = frame;
    }

    /// A turret building aims its slot-9 anim (`0x004466C5..0x004466CB`,
    /// `0x00451286..0x0045128C`, `0x0044FE17..0x0044FE1D`): it writes the
    /// frame (`+0xAC`) and a zero frame step (`+0xC4`), so the stage timer
    /// keeps running without moving the frame.
    pub(crate) fn hold_at_frame(&mut self, frame: i32) {
        self.runtime.current_frame = frame;
        self.runtime.frame_step = 0;
    }

    /// Hold the current loop until the owning effect explicitly destroys it.
    /// Ares attached animation1005872F stores byte255 at Anim+195.
    pub(crate) fn hold_loops(&mut self) {
        self.runtime.loop_remaining = u8::MAX;
    }

    /// No loops remain: the anim ends after its current pass. The
    /// parachute release and the ChronoPlacement release use it.
    pub(crate) fn end_loops(&mut self) {
        self.runtime.loop_remaining = 0;
    }

    pub const fn stable_id(&self) -> AnimId {
        self.stable_id
    }

    pub const fn native_unique_id(&self) -> i32 {
        self.native_unique_id
    }

    pub const fn type_id(&self) -> InternedId {
        self.type_id
    }

    pub const fn world_coord(&self) -> AnimWorldCoord {
        self.world_coord
    }

    pub const fn draw_flags(&self) -> u32 {
        self.draw_flags
    }

    pub const fn z_adjust(&self) -> i32 {
        self.z_adjust
    }

    pub const fn effective_end(&self) -> i32 {
        self.effective_end
    }

    pub const fn effective_loop_end(&self) -> i32 {
        self.effective_loop_end
    }

    pub const fn runtime(&self) -> &AnimRuntime {
        &self.runtime
    }

    pub const fn draw_runtime(&self) -> AnimDrawRuntime {
        self.draw_runtime
    }

    pub const fn use_cell_drawer(&self) -> bool {
        self.use_cell_drawer
    }

    pub const fn terrain_attached(&self) -> bool {
        self.terrain_attached
    }

    pub const fn in_logic_vector(&self) -> bool {
        self.in_logic_vector
    }

    pub const fn owner_entity(&self) -> Option<u64> {
        self.owner_entity
    }

    pub const fn building_slot(&self) -> Option<(u64, u8)> {
        self.building_slot
    }

    pub const fn damage_fire_slot(&self) -> Option<(u64, u8)> {
        self.damage_fire_slot
    }

    pub const fn start_sound_active(&self) -> bool {
        self.start_sound_active
    }

    pub const fn stop_sound_id(&self) -> Option<InternedId> {
        self.stop_sound_id
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn display(&self) -> &AnimDisplayState {
        &self.display
    }

    pub const fn bounce(&self) -> Option<BounceState> {
        self.bounce
    }

    pub fn remap(&self) -> Option<AnimRemap> {
        self.remap
    }

    pub fn no_effects(&self) -> bool {
        self.no_effects
    }

    pub fn completed(&self) -> bool {
        self.completed
    }

    pub fn is_building_anim(&self) -> bool {
        self.building_anim
    }

    /// `Phobos AnimExt::UpdateAsFiringAnim` `pThis->Type = pNewType`
    /// (`Ext/Anim/Body.cpp:88-92`): a changed firing direction swaps the anim's
    /// own type on the spot. `AnimClass` keeps the frame counters, not the type,
    /// so the running frame, step and delay carry over while only the
    /// type-owned bounds (`Type->End`, `Type->LoopEnd`) are re-derived.
    /// `TechnoClass_Fire_Anim @ 0x6FF42B` stores the weapon, barrel and
    /// burst the muzzle came from; the per-frame pass then keeps the flash
    /// on the firer (`0x423B95`, `Ext/Anim/Hooks.cpp:714-740`).
    pub(crate) fn set_firing_anim(&mut self, state: FiringAnimState) {
        self.firing_anim = Some(state);
    }

    pub(crate) fn firing_anim(&self) -> Option<FiringAnimState> {
        self.firing_anim
    }

    /// `AnimClass::SetLocation` (`ObjectClass+0x9C..+0xA4`): Phobos
    /// `UpdateAsFiringAnim` writes `GetFLH - GetRenderCoords`
    /// (`Ext/Anim/Body.cpp:93..103`) while the muzzle anim is owner-attached,
    /// so the stored coordinate stays owner-relative.
    pub(crate) fn set_stored_location(&mut self, coord: AnimWorldCoord) {
        self.world_coord = coord;
    }

    pub(crate) fn retype_for_firing_anim(
        &mut self,
        type_id: InternedId,
        type_name: &str,
        config: &crate::rules::art_data::AnimTypeRuntimeConfig,
    ) -> Result<(), AnimSpawnError> {
        let (end, loop_end) = effective_bounds(type_name, config)?;
        self.type_id = type_id;
        self.effective_end = end;
        self.effective_loop_end = loop_end;
        Ok(())
    }

    /// Building45199B..4519A2 marks the slot animation and replaces its
    /// constructor YSortAdjust with the building slot's retained value.
    /// The Anim store touch invalidates the existing Ground key cache.
    /// Native reader/store/sort evidence: building_slot_replacement.display.json.
    pub(crate) fn mark_building_anim(&mut self, y_sort: i32) {
        self.building_anim = true;
        self.display.y_sort_adjust = y_sort;
    }

    /// Ares `BuildingExt::ExtData::UpdateFirewall @ 10013280` stores its
    /// own anims in `building`'s `slot` and sets only Anim+0x118: the
    /// constructor's ZAdjust and YSortAdjust stay.
    pub(crate) fn enter_ares_firewall_slot(&mut self, building: u64, slot: u8) {
        self.building_anim = true;
        self.building_slot = Some((building, slot));
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn install_building_expiry_fixture(&mut self, marker: bool, completed: bool) {
        self.building_anim = marker;
        self.completed = completed;
    }

    /// Minimal unattached object for tests of storage, lifecycle and checksum
    /// consumers. Runtime producers use the animation constructor below.
    #[cfg(any(test, feature = "test-support"))]
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
            firing_anim: None,
            runtime,
            draw_runtime: AnimDrawRuntime::default(),
            viewer_hidden: None,
            use_cell_drawer: false,
            terrain_attached: false,
            no_effects: false,
            damage_accumulator: NativeF64Bits::POSITIVE_ZERO,
            phobos_firepower_mult: NativeF64Bits::ONE,
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
            flaming_guy: None,
            owner_house: None,
            attached_bullet: None,
            invoker: None,
            invoker_house: None,
            from_death_unit: false,
            death_unit_facing: None,
            death_unit_has_turret: false,
            death_unit_turret_facing: None,
            create_unit_fired: false,
            owner_terrain: None,
            presentation: None,
            presentation_order: 0,
        }
    }

    /// The owning House (`AnimClass+0x180`).
    pub fn owner_house(&self) -> Option<InternedId> {
        self.owner_house
    }

    /// The invoker Techno (`AnimExt::Invoker`).
    pub fn invoker(&self) -> Option<u64> {
        self.invoker
    }

    /// The invoker House (`AnimExt::InvokerHouse`).
    pub fn invoker_house(&self) -> Option<InternedId> {
        self.invoker_house
    }

    /// Phobos `AnimExt::FromDeathUnit`: a destruction-animation receipt.
    pub fn from_death_unit(&self) -> bool {
        self.from_death_unit
    }

    /// Phobos `AnimExt::DeathUnitFacing`: the destroyed unit's body DirType.
    pub fn death_unit_facing(&self) -> Option<u8> {
        self.death_unit_facing
    }

    /// Phobos `AnimExt::DeathUnitHasTurret`.
    pub fn death_unit_has_turret(&self) -> bool {
        self.death_unit_has_turret
    }

    /// Phobos `AnimExt::DeathUnitTurretFacing`.
    pub fn death_unit_turret_facing(&self) -> Option<u8> {
        self.death_unit_turret_facing
    }

    /// VERA-internal one-shot latch for the `CreateUnit` actual-effects arm
    /// (native reaches it once per anim).
    pub(crate) fn create_unit_fired(&self) -> bool {
        self.create_unit_fired
    }

    pub(crate) fn set_create_unit_fired(&mut self) {
        self.create_unit_fired = true;
    }

    /// The bullet whose expiry ends this anim (`AnimClass+0x17C`).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn attached_bullet(&self) -> Option<u64> {
        self.attached_bullet
    }

    /// The ids `Simulation::anim_pointer_expired` compares: the owner object
    /// (`AnimClass+0xCC`) and the attached bullet (`+0x17C`). VERA-only: feeds
    /// the pointer-expiry listener index (`ExpiryListeners`); a field that
    /// callback compares must be named here too.
    pub(crate) fn expiry_names(&self) -> [Option<u64>; 2] {
        // An anim names at most one owner at `+0xCC`.
        let owner = self.owner_entity.or(self.owner_terrain);
        [owner, self.attached_bullet]
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
        if let Some(snapshot) = &self.viewer_hidden {
            "AnimViewerHidden".hash(hasher);
            snapshot.hash(hasher);
        }
        self.use_cell_drawer.hash(hasher);
        self.terrain_attached.hash(hasher);
        self.in_logic_vector.hash(hasher);
        self.owner_entity.hash(hasher);
        self.building_slot.hash(hasher);
        self.start_sound_active.hash(hasher);
        self.stop_sound_id.hash(hasher);
        self.completed.hash(hasher);
        self.building_anim.hash(hasher);
        // The retained native byte affects later Start/UnInit behavior.
        // False preserves pre-existing hashes and compatibility fixtures.
        if self.no_effects {
            b"anim-no-effects-v1".hash(hasher);
            self.no_effects.hash(hasher);
        }
        if self.damage_accumulator != NativeF64Bits::ONE {
            b"anim-damage-accumulator-v1".hash(hasher);
            self.damage_accumulator.hash(hasher);
        }
        if self.phobos_firepower_mult != NativeF64Bits::ONE {
            b"anim-phobos-firepower-mult-v1".hash(hasher);
            self.phobos_firepower_mult.hash(hasher);
        }
        // Unset links add no bytes, preserving earlier streams.
        if self.owner_house.is_some() || self.attached_bullet.is_some() {
            b"anim-owner-house-bullet-v1".hash(hasher);
            self.owner_house.hash(hasher);
            self.attached_bullet.hash(hasher);
        }
        if self.invoker.is_some() || self.invoker_house.is_some() {
            b"anim-invoker-v1".hash(hasher);
            self.invoker.hash(hasher);
            self.invoker_house.hash(hasher);
        }
        if self.from_death_unit
            || self.death_unit_facing.is_some()
            || self.death_unit_has_turret
            || self.death_unit_turret_facing.is_some()
        {
            b"anim-death-unit-v1".hash(hasher);
            self.from_death_unit.hash(hasher);
            self.death_unit_facing.hash(hasher);
            self.death_unit_has_turret.hash(hasher);
            self.death_unit_turret_facing.hash(hasher);
        }
        if let Some(state) = &self.flaming_guy {
            b"anim-flaming-guy-v1".hash(hasher);
            state.hash(hasher);
        }
        if let Some(terrain) = self.owner_terrain {
            b"anim-owner-terrain-v1".hash(hasher);
            terrain.hash(hasher);
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
/// which notes it in a touch log for each [`AnimTouchReader`].
pub struct AnimStore {
    anims: BTreeMap<AnimId, AnimObject>,
    /// VERA-only: how many stored anims name each owner at `+0xCC`
    /// (`owner_entity`), so [`Self::another_owned_by`] answers the Anim
    /// array scans without walking it. The anims' owners are the truth:
    /// [`Self::insert`] and [`Self::remove`] keep it, and
    /// [`Self::set_owner_entity`] is the owners' one writer. Rebuilt on load,
    /// never saved, compared or hashed; only looked up.
    owner_counts: LookupMap<u64, u32>,
    /// Which anims may have changed since each reader last took its log
    /// ([`Self::take_touched`]). Transient: never saved, compared or hashed.
    touched: TouchLogs<{ AnimTouchReader::COUNT }>,
    /// Which anims may draw differently since the presentation last took the
    /// log ([`Self::take_looks`]): stored, or given a new colour remap or, by
    /// [`Self::note_look`], a new type. The simulation never reads it; never
    /// saved, compared or hashed.
    looks: TouchLogs<1>,
    /// The local anims, which only this game runs and draws: a LAN or online
    /// MoveFlash, which `FootClass::ClickedAction_Cell` moves out of
    /// AnimClass::Array (`0x00A8E9A8`) into its own list (`0x00A83E00`,
    /// `0x004D7FD7..0x004D8062`). They take ids from their own range
    /// ([`is_local_anim_id`]). The by-id reads and writes find them; the
    /// Anim array's readers ([`Self::iter`], [`Self::len`], [`Self::key_at`],
    /// [`Self::values_mut`]), comparison, saves and the world hash leave them
    /// out. They have no owner: the owner writers refuse them.
    local: BTreeMap<AnimId, AnimObject>,
    /// The id the next local anim takes.
    next_local: AnimId,
    /// Local anims Destroy has retired, which the end-of-frame drain removes
    /// ([`Self::drain_local_deletes`]): the shared pending-delete queue is
    /// hashed, so they stay out of it.
    local_deletes: Vec<AnimId>,
}

/// How a world makes the effect anims (muzzle flashes, impacts, trailers)
/// that change nothing but their own picture: in native order, as a new world
/// does, or as presentation anims ([`AnimConstructorInput::Presentation`])
/// once [`Simulation::present_effect_anims`] has asked. The choice decides
/// the unique ids later objects take, so a save carries it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EffectAnims {
    presentation: bool,
}

/// The first local anim id ([`AnimStore`]'s `local`). Shared ids count up
/// from 1 ([`crate::sim::world::ObjectSubstrate::allocate_stable_id`]) and
/// never reach it, and it sits below the sound-handle owner tags
/// (`1 << 57` to `1 << 62`), so a local anim's sound owner stays its own.
const LOCAL_ANIM_ID_BASE: AnimId = 1 << 56;

/// Whether `id` names a local anim, one only this game runs and draws.
pub(crate) fn is_local_anim_id(id: u64) -> bool {
    id >= LOCAL_ANIM_ID_BASE
}

/// A product derived from the anims that re-derives from the touch log.
/// Each reads its own log, so one reader's take leaves the others' intact.
#[derive(Debug, Clone, Copy)]
pub(crate) enum AnimTouchReader {
    /// The pointer-expiry broadcast's listener index.
    ExpiryListeners = 0,
}

impl AnimTouchReader {
    const COUNT: usize = 1;
}

impl AnimStore {
    pub fn get(&self, id: AnimId) -> Option<&AnimObject> {
        if is_local_anim_id(id) {
            return self.local.get(&id);
        }
        self.anims.get(&id)
    }

    pub fn get_mut(&mut self, id: AnimId) -> Option<&mut AnimObject> {
        let stored = self.anims.len();
        let anim = if is_local_anim_id(id) {
            self.local.get_mut(&id)?
        } else {
            self.anims.get_mut(&id)?
        };
        self.touched.note(id, stored);
        Some(anim)
    }

    /// A house's colour conversion as the anim's remap (`+0xD4`), frozen at
    /// the write; later disguise or owner changes must not recolor the anim.
    /// Unit deploy739C1A/739DFE store the one Techno705D70 returns (its
    /// custom-palette arm is unported there: no retail deploying Unit sets
    /// art `Palette=`); InfantryClass::ReceiveDamage's InfDeath 9 arm
    /// the house's colour scheme's (`0x00518AB5..0x00518AC9`,
    /// `0x00518AE4..0x00518AF9`).
    pub fn set_anim_house_remap(&mut self, id: AnimId, house: InternedId) -> bool {
        self.set_anim_remap(id, AnimRemap::House(house))
    }

    /// Assign the producer's frozen Convert identity (`Anim+0xD4`). Custom
    /// groups also retain their resource name, independently of any owner.
    pub fn set_anim_remap(&mut self, id: AnimId, remap: AnimRemap) -> bool {
        let Some(anim) = self.get_mut(id) else {
            return false;
        };
        anim.remap = Some(remap);
        self.looks.note(id, self.anims.len());
        true
    }

    /// Drop the anim's Convert (`Anim+0xD4 = 0`): Ares
    /// InfantryClass_ReceiveDamage_InfantryVirus2 (10025F50, hook 0x51887B).
    pub(crate) fn clear_anim_remap(&mut self, id: AnimId) -> bool {
        let Some(anim) = self.get_mut(id) else {
            return false;
        };
        anim.remap = None;
        true
    }

    pub(crate) fn insert(&mut self, object: AnimObject) -> Option<AnimObject> {
        self.touched.note(object.stable_id, self.anims.len());
        self.looks.note(object.stable_id, self.anims.len());
        if is_local_anim_id(object.stable_id) {
            debug_assert!(object.owner_entity.is_none());
            return self.local.insert(object.stable_id, object);
        }
        count_owner(&mut self.owner_counts, object.owner_entity);
        let replaced = self.anims.insert(object.stable_id, object);
        if let Some(replaced) = &replaced {
            uncount_owner(&mut self.owner_counts, replaced.owner_entity);
        }
        replaced
    }

    pub(crate) fn remove(&mut self, id: AnimId) -> Option<AnimObject> {
        if is_local_anim_id(id) {
            let removed = self.local.remove(&id);
            if removed.is_some() {
                self.touched.note(id, self.anims.len());
            }
            return removed;
        }
        let removed = self.anims.remove(&id);
        if let Some(removed) = &removed {
            uncount_owner(&mut self.owner_counts, removed.owner_entity);
            self.touched.note(id, self.anims.len());
        }
        removed
    }

    /// Write `id`'s owner (`+0xCC`), returning the one it replaced, or `None`
    /// when `id` is not stored. The owners' one writer.
    pub(crate) fn set_owner_entity(
        &mut self,
        id: AnimId,
        owner: Option<u64>,
    ) -> Option<Option<u64>> {
        // A presentation anim follows its owner too. No Anim array scan
        // counts it ([`Self::another_owned_by`]): it raises no owner flag.
        if is_local_anim_id(id) {
            let anim = self.local.get_mut(&id)?;
            debug_assert!(anim.presentation.is_some() || owner.is_none());
            return Some(std::mem::replace(&mut anim.owner_entity, owner));
        }
        let stored = self.anims.len();
        let anim = self.anims.get_mut(&id)?;
        self.touched.note(id, stored);
        let replaced = std::mem::replace(&mut anim.owner_entity, owner);
        uncount_owner(&mut self.owner_counts, replaced);
        count_owner(&mut self.owner_counts, owner);
        Some(replaced)
    }

    /// Write `id`'s terrain owner (`+0xCC` naming a TerrainClass), returning
    /// the one it replaced, or `None` when `id` is not stored.
    pub(crate) fn set_owner_terrain(
        &mut self,
        id: AnimId,
        owner: Option<u64>,
    ) -> Option<Option<u64>> {
        let stored = self.anims.len();
        let anim = self.anims.get_mut(&id)?;
        self.touched.note(id, stored);
        Some(std::mem::replace(&mut anim.owner_terrain, owner))
    }

    /// Whether a stored anim other than `id` names the terrain object
    /// `owner`: the Anim array scan of SetOwnerObject424B79..424BB6 for a
    /// terrain owner. Only burning trees own anims, so it walks the store.
    pub(crate) fn another_owned_by_terrain(&self, id: AnimId, owner: u64) -> bool {
        self.anims
            .iter()
            .any(|(&other, anim)| other != id && anim.owner_terrain == Some(owner))
    }

    /// Ares terrain fire: the anims a terrain object owns, in array order.
    pub(crate) fn owned_by_terrain(&self, owner: u64) -> Vec<AnimId> {
        self.anims
            .iter()
            .filter(|(_, anim)| anim.owner_terrain == Some(owner))
            .map(|(&id, _)| id)
            .collect()
    }

    /// `AnimClass::ClearLoopsForOwner @ 0x00422B80`: every anim naming
    /// `owner` at `+0xCC` ends after its current loop (`+0x195 = 0`).
    pub(crate) fn clear_loops_for_terrain_owner(&mut self, owner: u64) {
        let stored = self.anims.len();
        for (&id, anim) in self.anims.iter_mut() {
            if anim.owner_terrain == Some(owner) {
                anim.end_loops();
                self.touched.note(id, stored);
            }
        }
    }

    /// The terrain object an anim is attached to (`+0xCC`).
    pub(crate) fn owner_terrain(&self, id: AnimId) -> Option<u64> {
        self.anims.get(&id).and_then(|anim| anim.owner_terrain)
    }

    /// Install a fixture's owner (`+0xCC`) on a stored anim.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_owner_entity_for_test(&mut self, id: AnimId, owner: Option<u64>) {
        self.set_owner_entity(id, owner)
            .expect("the fixture's anim is stored");
    }

    /// Whether a stored anim other than `id` names `owner` at `+0xCC`: the
    /// Anim array scan of SetOwnerObject424B79..424BB6 and the destructor
    ///42291A..42295B, which counts every retained entry (inactive,
    /// concealed and pending-delete ones too), answered from the owners'
    /// counts. Debug builds check it against that scan (the live-read check).
    pub(crate) fn another_owned_by(&self, id: AnimId, owner: u64) -> bool {
        let own = u32::from(self.get(id).and_then(|anim| anim.owner_entity) == Some(owner));
        let another = self
            .owner_counts
            .get(&owner)
            .is_some_and(|&count| count > own);
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled()
                || another
                    == self
                        .anims
                        .iter()
                        .any(|(&other, anim)| other != id && anim.owner_entity == Some(owner)),
            "the Anim owner counts disagree with the Anim array for owner {owner}"
        );
        another
    }

    /// Store the independently owned attached-Bullet pointer (+0x17C).
    fn set_attached_bullet(&mut self, id: AnimId, bullet: Option<u64>) -> bool {
        let Some(anim) = self.get_mut(id) else {
            return false;
        };
        anim.attached_bullet = bullet;
        true
    }

    fn from_anims(anims: BTreeMap<AnimId, AnimObject>) -> Self {
        let mut owner_counts = LookupMap::default();
        for anim in anims.values() {
            count_owner(&mut owner_counts, anim.owner_entity);
        }
        Self {
            anims,
            owner_counts,
            touched: TouchLogs::everything(),
            looks: TouchLogs::everything(),
            local: BTreeMap::new(),
            next_local: LOCAL_ANIM_ID_BASE,
            local_deletes: Vec::new(),
        }
    }

    /// The id for a new local anim.
    pub(crate) fn allocate_local_id(&mut self) -> AnimId {
        let id = self.next_local;
        self.next_local += 1;
        id
    }

    /// The local anims, in the order they were made (the local list's).
    pub(crate) fn local_ids(&self) -> Vec<AnimId> {
        self.local.keys().copied().collect()
    }

    /// The local anims and their ids, in the local list's order.
    pub(crate) fn local_iter(&self) -> impl Iterator<Item = (&AnimId, &AnimObject)> {
        self.local.iter()
    }

    /// Take presentation anim `id` out of its layer, as Display's remove
    /// takes an ordinary anim out of its vector.
    pub(crate) fn conceal_presentation(&mut self, id: AnimId) {
        if let Some(anim) = self.local.get_mut(&id)
            && anim.presentation.is_some()
        {
            anim.presentation = Some(None);
        }
    }

    /// The presentation anims, in the order they were made.
    pub(crate) fn presentation_iter(&self) -> impl Iterator<Item = (&AnimId, &AnimObject)> {
        self.local.iter().filter(|(_, anim)| anim.is_presentation())
    }

    /// Retire the local anim `id` for the end-of-frame drain.
    pub(crate) fn queue_local_delete(&mut self, id: AnimId) {
        if !self.local_deletes.contains(&id) {
            self.local_deletes.push(id);
        }
    }

    /// Whether Destroy has retired the local anim `id`.
    pub(crate) fn local_delete_queued(&self, id: AnimId) -> bool {
        self.local_deletes.contains(&id)
    }

    /// Remove the retired local anims. Their Destroy already released the
    /// sound and left Display; the destructor's `0x00422A69` takes each out
    /// of the local list.
    pub(crate) fn drain_local_deletes(&mut self) {
        for id in std::mem::take(&mut self.local_deletes) {
            self.remove(id);
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&AnimId, &AnimObject)> {
        self.anims.iter()
    }

    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut AnimObject> {
        self.touched.note_all();
        self.looks.note_all();
        self.anims.values_mut()
    }

    pub fn contains_key(&self, id: AnimId) -> bool {
        if is_local_anim_id(id) {
            return self.local.contains_key(&id);
        }
        self.anims.contains_key(&id)
    }

    pub(crate) fn len(&self) -> usize {
        self.anims.len()
    }

    pub(crate) fn key_at(&self, index: usize) -> Option<AnimId> {
        self.anims.keys().nth(index).copied()
    }

    /// Take `reader`'s touch log, leaving it empty.
    pub(crate) fn take_touched(&mut self, reader: AnimTouchReader) -> Touched {
        self.touched.take(reader as usize)
    }

    /// Take the look log, leaving it empty.
    pub(crate) fn take_looks(&mut self) -> Touched {
        self.looks.take(0)
    }

    /// Note that an in-place write gave `id` a new type.
    pub(crate) fn note_look(&mut self, id: AnimId) {
        self.looks.note(id, self.anims.len());
    }
}

impl Default for AnimStore {
    fn default() -> Self {
        Self::from_anims(BTreeMap::new())
    }
}

impl Clone for AnimStore {
    /// A clone starts with an everything-touched log: whatever was derived
    /// from the original says nothing certain about the copy's future. The
    /// local anims come along, so the presentation copy draws them.
    fn clone(&self) -> Self {
        Self {
            local: self.local.clone(),
            next_local: self.next_local,
            local_deletes: self.local_deletes.clone(),
            ..Self::from_anims(self.anims.clone())
        }
    }
}

/// The local anims are left out, as they are from saves and the hash.
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

/// The saved form is the former newtype `AnimStore(map)`; the log and the
/// local anims are not saved.
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
        Ok(Self::from_anims(anims))
    }
}

/// Count one more stored anim naming `owner`.
fn count_owner(counts: &mut LookupMap<u64, u32>, owner: Option<u64>) {
    if let Some(owner) = owner {
        *counts.entry(owner).or_default() += 1;
    }
}

/// Count one fewer stored anim naming `owner`, forgetting an owner none names.
fn uncount_owner(counts: &mut LookupMap<u64, u32>, owner: Option<u64>) {
    if let Some(owner) = owner
        && let std::collections::hash_map::Entry::Occupied(mut count) = counts.entry(owner)
    {
        *count.get_mut() -= 1;
        if *count.get() == 0 {
            count.remove();
        }
    }
}

#[derive(Debug, Error)]
pub enum AnimSpawnError {
    #[error(transparent)]
    Tiberium(#[from] TiberiumImageError),
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
pub(crate) enum AnimOccupationOperation {
    Mark,
    Clear,
}

/// One MakeInfantry anim's Mark or Clear on its cell's raw planes. At or above
/// the floor plus the deck (Anim `[0x0089A1B4]`), Mark takes the deck only
/// while the cell keeps its structural bridge flag. Both write the anim's owner
/// index, -1 (vtable `+0x38` = `0x00410490`, stored at `0x004262D0` /
/// `0x004262F5`); Clear resets it once `0x1C` is clear (`0x0042634F` /
/// `0x00426378`).
pub(crate) fn apply_anim_raw_occupation(
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

/// `AnimClass::MarkAllOccupationBits` / `UnmarkAllOccupationBits` with the
/// constructor's and actual-effects' `GetCell()->GetCoordsWithBridge()`
/// argument (`Ext/Anim/Hooks.AnimCreateUnit.cpp:36-80`): the coordinate's
/// plane follows the cell's live structural bridge flag alone; the anim's
/// height does not select a layer.
pub(crate) fn apply_anim_coords_occupation(
    grid: &mut RawCellOccupationGrid,
    key: RawCellKey,
    mask: u8,
    live_structural_bridge: bool,
    operation: AnimOccupationOperation,
) {
    let layer = if live_structural_bridge {
        MovementLayer::Bridge
    } else {
        MovementLayer::Ground
    };
    let put = operation == AnimOccupationOperation::Mark;
    grid.write_occupant(key, layer, mask, None, put);
}

/// A building's damage fires: the transition cache (`BuildingClass+0x5E8`),
/// distinct from the generic damaged-art state, and the eight fixed AnimClass
/// ownership slots (`+0x5C8..+0x5E4`). `BuildingClass::Update` calls the
/// producer (or the `0x0043FCA4` recovery) before storing the cache
/// (`0x0043FC92`, `0x0043FCBE`); an anim's expiry clears its slot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DamageFires {
    active: bool,
    anim_ids: [Option<AnimId>; 8],
}

impl DamageFires {
    /// The cached transition state.
    pub(crate) const fn is_active(&self) -> bool {
        self.active
    }

    /// The eight slots.
    pub(crate) const fn anim_ids(&self) -> &[Option<AnimId>; 8] {
        &self.anim_ids
    }

    /// Install a fixture's slot.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_slot_for_test(&mut self, slot: usize, id: Option<AnimId>) {
        self.anim_ids[slot] = id;
    }
}

/// An object that never lit a fire keeps no slots (`GameEntity::damage_fires`).
impl crate::util::sparse::SparseDefault for DamageFires {
    fn sparse_default() -> &'static Self {
        static DEFAULT: std::sync::LazyLock<DamageFires> =
            std::sync::LazyLock::new(DamageFires::default);
        &DEFAULT
    }
}

/// The fields `Simulation::visit_anim`'s prefix and gates read, copied in one
/// lookup of the anim.
#[derive(Clone, Copy)]
struct AnimGates {
    /// The stored Location (`+0x9C`), owner-relative on an attached anim.
    location: AnimWorldCoord,
    bouncing: bool,
    inactive: bool,
    first_ai_guard: bool,
    delay_remaining: i32,
    /// Paused (`+0x19E`) or Temporal-paused (`+0x11A`).
    paused: bool,
    rate_reload: u16,
    frame_timer: CdTimer,
}

impl AnimGates {
    fn of(anim: &AnimObject) -> Self {
        Self {
            location: anim.world_coord,
            bouncing: anim.bounce.is_some(),
            inactive: anim.runtime.inactive,
            first_ai_guard: anim.runtime.first_ai_guard,
            delay_remaining: anim.runtime.delay_remaining,
            paused: anim.runtime.paused || anim.runtime.temporal_paused,
            rate_reload: anim.runtime.rate_reload,
            frame_timer: anim.runtime.frame_timer,
        }
    }
}

impl Simulation {
    pub fn anim(&self, id: AnimId) -> Option<&AnimObject> {
        self.substrate.anims.get(id)
    }

    /// Phobos `AnimClass_DrawIt_Visibility @ 0x423061`
    /// (`Ext/Anim/Hooks.cpp:520-570`): the `RestrictVisibilityIfCloaked=`
    /// skip ([`Self::anim_owner_is_cloaked`], with the owner's alliance and
    /// the viewer's sensors) followed by the `VisibleTo` owner-house gate.
    /// `VisibleTo.ConsiderInvokerAsOwner` replaces the owner object's house
    /// with the anim's invoker (or invoker house) first.
    ///
    /// RESIDUAL: the cloak arm's trailer-anim fallback
    /// (`pExt->IsTechnoTrailerAnim` then `Invoker` when `OwnerObject` is not a
    /// techno) has no trailer-anim marker in VERA; such an anim keeps the
    /// non-techno answer.
    pub fn anim_visibility_skipped(
        &self,
        id: AnimId,
        rules: &RuleSet,
        viewer: Option<InternedId>,
    ) -> bool {
        use crate::rules::phobos_shields::AffectedHouse;
        let Some(anim) = self.anim(id) else {
            return false;
        };
        let Some(config) = rules
            .art()
            .anim_runtime_config(self.interner().resolve(anim.type_id()))
        else {
            return false;
        };
        let visible_to = config.phobos_visible_to.unwrap_or(AffectedHouse::ALL);
        if !config.phobos_restrict_visibility_if_cloaked && visible_to == AffectedHouse::ALL {
            return false;
        }
        let Some(viewer) = viewer.or_else(|| self.session().current_house()) else {
            return false;
        };
        if self.session().is_observer_house(viewer) {
            return false;
        }
        if config.phobos_restrict_visibility_if_cloaked
            && let Some(owner_id) = anim.owner_entity()
            && let Some(owner) = self.entities().get(owner_id)
            && matches!(owner.cloak.as_ref().map_or(0, |cloak| cloak.state()), 1 | 2)
            && !self
                .house_alliances()
                .is_allied(self.interner(), viewer, owner.owner())
            // `pTechno->GetCell()->Sensors_InclHouse(current)`: a sensor over
            // the owner's cell reveals it regardless of the alliance.
            && !self
                .fog()
                .has_sensor_for_house(viewer, owner.position.rx(), owner.position.ry())
        {
            return true;
        }
        // `pOwner = OwnerObject ? OwnerObject->GetOwningHouse() : pThis->Owner`.
        let mut owner = anim
            .owner_entity()
            .and_then(|owner_id| self.entities().get(owner_id))
            .map(|entity| entity.owner())
            .or_else(|| anim.owner_house());
        if config.phobos_visible_to_consider_invoker_as_owner {
            if let Some(invoker) = anim.invoker() {
                owner = self.entities().get(invoker).map(|entity| entity.owner());
            } else if let Some(house) = anim.invoker_house() {
                owner = Some(house);
            }
        }
        let Some(owner) = owner else {
            return false;
        };
        !crate::sim::combat::can_target_house(
            visible_to,
            viewer,
            owner,
            self.houses(),
            self.house_alliances(),
            self.interner(),
        )
    }

    /// The anim's entity owner is Cloaking (1) or Cloaked (2): the
    /// `Translucency.Cloaked` gate in `AnimClass_DrawIt_Translucency`
    /// (`0x423183`, `Ext/Anim/Hooks.cpp:573-615`).
    pub fn anim_owner_is_cloaked(&self, id: u64) -> bool {
        let Some(owner_id) = self.anim(id).and_then(AnimObject::owner_entity) else {
            return false;
        };
        self.entities().get(owner_id).is_some_and(|owner| {
            matches!(owner.cloak.as_ref().map_or(0, |cloak| cloak.state()), 1 | 2)
        })
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

    /// `AnimClass::MarkCellOccupancy` (vt+0xF0 = `0x00426270`) at the anim's
    /// stored Location, called directly: InfantryClass::ReceiveDamage's
    /// InfDeath 9 arm marks its new InfantryMutate anim (`0x00518B26`),
    /// whatever the anim's `MakeInfantry=`.
    pub(crate) fn anim_mark_cell_occupancy(
        substrate: &mut ObjectSubstrate,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        id: AnimId,
    ) {
        if let Some(location) = substrate.anims.get(id).map(|anim| anim.world_coord) {
            Self::apply_make_infantry_raw_occupation(
                substrate,
                resolved_terrain,
                location,
                AnimOccupationOperation::Mark,
            );
        }
    }

    /// `AnimClass::MarkCellOccupancy` (`0x00426270`) and `ClearCellOccupancy`
    /// (`0x00426300`) for a MakeInfantry anim at its raw Location: the sub-cell
    /// (`0x004810A0`), the cell through Map[coord] (`0x00565730`), whose miss
    /// is the shared dummy cell, and the floor there (`0x00578080`).
    fn apply_make_infantry_raw_occupation(
        substrate: &mut ObjectSubstrate,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        location: AnimWorldCoord,
        operation: AnimOccupationOperation,
    ) {
        let mask = infantry_raw_occupation_mask(
            SimFixed::from_num(location.x & 0xff),
            SimFixed::from_num(location.y & 0xff),
        );
        let (key, ground_z, live_structural_bridge) = match resolved_terrain.as_ref() {
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
                let Some((rx, ry)) = Self::map_coord_cell(resolved_terrain, location.x, location.y)
                else {
                    return;
                };
                (RawCellKey::Real(rx, ry), 0, false)
            }
        };
        apply_anim_raw_occupation(
            &mut substrate.raw_cell_occupation,
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
    #[cfg(any(test, feature = "test-support"))]
    pub fn spawn_anim_object(
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

    /// [`Self::spawn_anim_object`] for a muzzle flash, impact or trailer: it
    /// takes [`Self::effect_anim_constructor`], so after
    /// [`Self::present_effect_anims`] a picture-only type is a presentation
    /// anim, as in a match.
    #[cfg(any(test, feature = "test-support"))]
    pub fn spawn_effect_anim_object(
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
        let constructor = self.effect_anim_constructor(rules, descriptor.type_name);
        self.spawn_anim_at_world_with_constructor(
            rules,
            descriptor,
            world_coord,
            constructor,
            FrameEffects::default(),
        )
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
    /// Returns `None` if the type or its required runtime bounds never bound
    /// (see `ArtRegistry::bind_anim_class_assets`). A missing SHP is a bound
    /// native null image: its zero-frame animation still constructs and runs
    /// Start/Middle effects, then retires through the existing AI lifecycle.
    ///
    /// The anim is constructed at the producer's exact impact Z (`world_z`);
    /// `z` is only the level byte of the descriptor.
    pub fn spawn_combat_explosion_anim(
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
        self.spawn_combat_explosion_anim_with_z_adjust(
            rules,
            type_name,
            rx,
            ry,
            sub_x,
            sub_y,
            z,
            world_z,
            COMBAT_EXPLOSION_Z_ADJUST,
            None,
            true,
        )
    }

    /// [`Self::spawn_combat_explosion_anim`] with the Phobos warhead
    /// `AnimZAdjust` substituted for the stock combat row's -15 and the
    /// tail's invoker House installed after construction. Unless the caller
    /// `keeps` the anim, a picture-only type runs as a presentation anim
    /// ([`Self::effect_anim_constructor`]); [`Self::spawn_combat_explosion_anim`]
    /// returns an anim its callers may keep (the nuke and Ares' PreImpactAnim
    /// await theirs), so it always makes an ordinary one.
    pub(crate) fn spawn_combat_explosion_anim_with_z_adjust(
        &mut self,
        rules: &RuleSet,
        type_name: InternedId,
        rx: u16,
        ry: u16,
        sub_x: crate::util::fixed_math::SimFixed,
        sub_y: crate::util::fixed_math::SimFixed,
        z: u8,
        world_z: i32,
        z_adjust: i32,
        owner_house: Option<InternedId>,
        keeps: bool,
    ) -> Option<AnimId> {
        let descriptor = AnimClassSpawnDescriptor {
            delay: 0,
            loop_count: 1,
            draw_flags: COMBAT_EXPLOSION_DRAW_FLAGS,
            z_adjust,
            reverse: false,
            ..AnimClassSpawnDescriptor::new(type_name, rx, ry, sub_x, sub_y, z)
        };
        let world_coord = AnimWorldCoord {
            z: world_z,
            ..AnimWorldCoord::from_cell_sub_z(rx, ry, sub_x, sub_y, z)
        };
        let constructor = if keeps {
            AnimConstructorInput::Runtime
        } else {
            self.effect_anim_constructor(rules, type_name)
        };
        match self.spawn_anim_at_world_with_constructor(
            rules,
            descriptor,
            world_coord,
            constructor,
            FrameEffects::default(),
        ) {
            Ok(id) => {
                if let Some(house) = owner_house {
                    self.install_anim_owner_house(rules, type_name, id, house);
                }
                Some(id)
            }
            Err(AnimSpawnError::Tiberium(error)) => {
                log::warn!("combat explosion refused an invalid Tiberium Image start: {error}");
                None
            }
            Err(error) => {
                log::debug!(
                    "combat explosion [{}] did not construct: {error}",
                    self.interner.resolve(type_name)
                );
                None
            }
        }
    }

    /// `AnimExt::CreateRandomAnim`'s construction half: a new anim at
    /// `world_coord` with the `AnimClass` constructor defaults (`loop=1`,
    /// flags `0x600`) and `owner_house` installed through the same
    /// `SetAnimOwnerHouseKind(..., false, true)` tail the combat producer
    /// `SetAnimOwnerObject` (native `ownedObject=true`). RESIDUAL: a
    /// `MakeInfantry=`/`CreateUnit=` type re-selects its own owner kind here;
    /// this owner keeps the invoker House, like the existing combat producer.
    /// `invoker` is `CreateRandomAnim`'s `pTechno` under its `invoker` flag
    /// (the same object may or may not also be the `ownedObject`).
    pub(crate) fn spawn_owned_anim(
        &mut self,
        rules: &RuleSet,
        type_name: InternedId,
        world_coord: AnimWorldCoord,
        owner_house: InternedId,
        owner_object: Option<u64>,
        invoker: Option<u64>,
    ) -> Option<AnimId> {
        let (rx, ry, sub_x, sub_y, z) = world_coord.to_cell_sub_z();
        let descriptor = AnimClassSpawnDescriptor {
            draw_flags: 0x600,
            owner: owner_object,
            ..AnimClassSpawnDescriptor::new(type_name, rx, ry, sub_x, sub_y, z)
        };
        let id = self
            .spawn_anim_at_world(rules, descriptor, world_coord)
            .ok()?;
        self.install_anim_owner_house(rules, type_name, id, owner_house);
        if let Some(invoker) = invoker {
            self.substrate
                .set_anim_invoker(id, Some(invoker), Some(owner_house));
        }
        Some(id)
    }

    /// Phobos `VoxelAnimClass_AI_Trailer @ 0x74A70E`
    /// (`Ext/VoxelAnim/Hooks.cpp:47-67`): `GameCreate<AnimClass>(TrailerAnim,
    /// Location, 1, 1)` then `SetAnimOwnerHouseKind(OwnerHouse, nullptr,
    /// false, true)`. A null owner house leaves the anim unowned, as the
    /// default kind does.
    ///
    /// RESIDUAL: a `MakeInfantry=`/`CreateUnit=` trailer type re-selects its
    /// own owner kind in native, as [`Self::spawn_owned_anim`] records; this
    /// keeps the invoker house like the combat producer.
    pub(crate) fn spawn_trailer_anim(
        &mut self,
        rules: &RuleSet,
        type_name: InternedId,
        world_coord: AnimWorldCoord,
        owner_house: Option<InternedId>,
    ) -> Option<AnimId> {
        let (rx, ry, sub_x, sub_y, z) = world_coord.to_cell_sub_z();
        let descriptor = AnimClassSpawnDescriptor {
            delay: 1,
            draw_flags: TRAILER_DRAW_FLAGS,
            ..AnimClassSpawnDescriptor::new(type_name, rx, ry, sub_x, sub_y, z)
        };
        let id = self
            .spawn_anim_at_world(rules, descriptor, world_coord)
            .ok()?;
        if let Some(house) = owner_house {
            self.install_anim_owner_house(rules, type_name, id, house);
        }
        Some(id)
    }

    /// `AnimExt::CreateRandomAnim`'s name/random half
    /// (`Ext/Anim/Body.cpp:407-432`), shared by the Phobos consumers: an
    /// unresolvable `ValueableVector<AnimTypeClass*>` token never reached the
    /// native vector, so only names with a type take part in
    /// `RandomRanged(0, size - 1)`; an empty list draws nothing.
    pub(crate) fn resolve_random_anim_name<'n>(
        &mut self,
        rules: &RuleSet,
        names: &'n [String],
    ) -> Option<&'n str> {
        let anims: Vec<&str> = names
            .iter()
            .map(String::as_str)
            .filter(|name| rules.has_anim_type(name))
            .collect();
        if anims.is_empty() {
            return None;
        }
        let pick = self
            .scenario_rng
            .next_range_i32_inclusive(0, anims.len() as i32 - 1) as usize;
        Some(anims[pick])
    }

    /// [`Self::resolve_random_anim_name`] plus construction with the owner
    /// house and owner object.
    pub(crate) fn spawn_random_owned_anim(
        &mut self,
        rules: &RuleSet,
        names: &[String],
        world_coord: AnimWorldCoord,
        owner_house: InternedId,
        owner_object: Option<u64>,
        invoker: Option<u64>,
    ) -> Option<AnimId> {
        let name = self.resolve_random_anim_name(rules, names)?;
        let type_name = self.interner.intern(name);
        self.spawn_owned_anim(
            rules,
            type_name,
            world_coord,
            owner_house,
            owner_object,
            invoker,
        )
    }

    /// `AnimExt::SetAnimOwnerHouseKind(anim, house, None, false, true)`'s
    /// post-construction tail: install the owner House and, for a
    /// `MakeInfantry=` or remapping `CreateUnit=` type, its light conversion.
    pub(crate) fn install_anim_owner_house(
        &mut self,
        rules: &RuleSet,
        type_name: InternedId,
        id: AnimId,
        owner_house: InternedId,
    ) {
        self.substrate.set_anim_owner_house(id, owner_house);
        {
            let remaps = {
                let name = self.interner.resolve(type_name);
                rules.art().anim_runtime_config(name).is_some_and(|config| {
                    config.make_infantry > -1
                        || config
                            .phobos_create_unit
                            .as_ref()
                            .is_some_and(|settings| settings.remap_anim)
                })
            };
            let defeated = self
                .houses
                .get(&owner_house)
                .is_some_and(|state| state.is_defeated());
            if remaps && !defeated {
                self.substrate.anims.set_anim_house_remap(id, owner_house);
            }
        }
    }

    /// `0x6FF42B` marks the anim the muzzle block just built and counts it
    /// into `FiringAnimUpdateCount`; the anim also holds it for its own AI.
    pub(crate) fn attach_firing_anim(&mut self, id: AnimId, state: FiringAnimState) {
        if let Some(anim) = self.substrate.anims.get_mut(id) {
            anim.set_firing_anim(state);
            self.firing_anim_update_count = self.firing_anim_update_count.saturating_add(1);
        }
    }

    pub const fn firing_anim_update_count(&self) -> u32 {
        self.firing_anim_update_count
    }

    /// Phobos `AnimExt::~AnimExt` releases the owner count for a marked anim.
    pub(crate) fn release_firing_anim_count(&mut self, anim: &AnimObject) {
        if anim.firing_anim().is_some() {
            self.firing_anim_update_count = self.firing_anim_update_count.saturating_sub(1);
        }
    }

    /// Phobos `AnimExt::UpdateAsFiringAnim`
    /// (`Ext/Anim/Body.cpp:72..105`), called per anim by the early-AI hook
    /// `0x423B95` while `FiringAnimUpdateCount > 0`: a changed real facing
    /// re-picks the type from the weapon's `Anim=` list, and a changed facing
    /// or owner render coordinate re-anchors the anim at
    /// `GetFLH(weaponIndex) - GetRenderCoords`.
    ///
    /// `TechnoClass::GetRealFacing` (`0x708D70`) forwards to the turret-facing
    /// virtual (`vt+0x2A8`). The pass uses `fire_coordinate`'s aim facing as
    /// that value: the barrel facing when the object has one (and for
    /// aircraft), the body heading otherwise; `Fire_At` picks the muzzle type
    /// from the same facing.
    fn update_as_firing_anim(&mut self, id: AnimId, rules: &RuleSet) {
        let Some(anim) = self.substrate.anims.get(id) else {
            return;
        };
        let Some(mut state) = anim.firing_anim() else {
            return;
        };
        let Some(owner_id) = anim.owner_entity() else {
            return;
        };
        let current_type = anim.type_id();
        let Some(owner) = self.substrate.entities.get(owner_id) else {
            return;
        };
        if !matches!(
            owner.category(),
            EntityCategory::Infantry
                | EntityCategory::Unit
                | EntityCategory::Aircraft
                | EntityCategory::Structure
        ) {
            return;
        }
        let Some(obj) = self.interner.object_type(owner.type_ref(), rules) else {
            return;
        };
        let Some(weapon) = rules.weapon(self.interner.resolve(state.weapon)) else {
            return;
        };
        let source = crate::sim::combat::fire_coord::FireSource::of_entity(owner);
        let owner_coords = anim_owner_world_coords(owner, self.resolved_terrain.as_ref());
        let fire = crate::sim::combat::fire_coord::fire_coordinate(
            self,
            rules,
            &source,
            obj,
            i32::from(state.weapon_index),
            state.burst_index,
            crate::rules::flh::Flh::default(),
        );

        let facing_changed = fire.aim_facing16 != state.last_facing;
        if facing_changed {
            state.last_facing = fire.aim_facing16;
            let index = crate::sim::unit_simple_deploy::item_for_direction(
                weapon.anim.len(),
                fire.aim_facing16,
            );
            if let Some(name) = weapon.anim.get(index) {
                let type_id = self.interner.intern(name);
                if type_id != current_type
                    && let Some(config) = rules.art().anim_runtime_config(name)
                    && let Some(anim) = self.substrate.anims.get_mut(id)
                {
                    // A refused retype (unbound art) keeps the current type.
                    let _ = anim.retype_for_firing_anim(type_id, name, config);
                }
            }
        }

        if facing_changed || owner_coords != state.last_coords {
            state.last_coords = owner_coords;
            if let Some(anim) = self.substrate.anims.get_mut(id) {
                anim.set_stored_location(AnimWorldCoord {
                    x: fire.coord.x.wrapping_sub(owner_coords.x),
                    y: fire.coord.y.wrapping_sub(owner_coords.y),
                    z: fire.coord.z.wrapping_sub(owner_coords.z),
                });
            }
        }
        if let Some(anim) = self.substrate.anims.get_mut(id) {
            anim.set_firing_anim(state);
        }
    }

    pub fn spawn_anim_at_world(
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
            FrameEffects::default(),
        )
    }

    /// Original TAction41 PlayAnimAtWP6E2290, reached at6DECB0.
    /// Resolve8B4154[+90], use the shared signed waypoint/Ground/bridge
    /// query, then constructor421EA0 with delay0/loop1/flags600. Native
    /// sets+198 after the constructor returns, so its first Start effects
    /// remain admitted. Invalid Rust input safely fails instead of taking
    /// the original unchecked pointer access; stock operands are valid.
    ///
    /// Ares 3.0 ActionClass_PlayAnimAt 10025C20 (hook 0x6E232E) gives a
    /// `MakeInfantry=` type SetMakeInfOwner with the action's House as
    /// invoker, victim and killer.
    pub(crate) fn trigger_play_anim_at_waypoint(
        &mut self,
        rules: &RuleSet,
        anim_type_index: i32,
        waypoint: i32,
        house: Option<InternedId>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let Some(type_name) = rules.anim_type_runtime_key_by_index(anim_type_index) else {
            return false;
        };
        let Some([x, y, z]) = self.trigger_waypoint_coord(waypoint) else {
            return false;
        };
        let world_coord = AnimWorldCoord { x, y, z };
        let type_id = self.interner.intern(&type_name);
        let (rx, ry, sub_x, sub_y, level) = world_coord.to_cell_sub_z();
        let descriptor = AnimClassSpawnDescriptor {
            draw_flags: 0x600,
            ..AnimClassSpawnDescriptor::new(type_id, rx, ry, sub_x, sub_y, level)
        };
        let id = match self.spawn_anim_at_world_with_constructor(
            rules,
            descriptor,
            world_coord,
            AnimConstructorInput::Runtime,
            frame_effects,
        ) {
            Ok(id) => id,
            Err(AnimSpawnError::Tiberium(error)) => {
                log::warn!("waypoint animation refused an invalid Tiberium Image start: {error}");
                return false;
            }
            Err(_) => return false,
        };
        if rules.ares_form()
            && rules
                .art()
                .anim_runtime_config(&type_name)
                .is_some_and(|config| config.make_infantry > -1)
        {
            self.ares_set_make_inf_owner(id, type_id, house, house, house, rules);
        }
        self.substrate.set_anim_no_effects(id, true);
        true
    }

    /// The MoveFlash of `FootClass::ClickedAction_Cell`'s Move arm
    /// (`0x004D7EB5..0x004D7F98`): `AnimClass(Rules+0xBB4, coord, 0, 1,
    /// 0x600, 0, false)` at the clicked Cell's centre, on the ground
    /// (`0x00578080`) and raised by the bridge deck when the Cell at that
    /// coordinate (`0x00565730`) has flag 0x100. Campaign and skirmish
    /// (GameMode 0 and 5) keep it an ordinary anim. LAN and online games make
    /// it a local anim instead (`0x004D7F28..0x004D7F57`,
    /// `0x004D7FA6..0x004D8062`), which only the clicking game runs and
    /// draws. False when nothing is constructed.
    ///
    /// RESIDUAL: a local anim runs the whole AnimClass::AI, as gamemd's does.
    /// A MoveFlash type that damages, leaves a scorch or crater, spawns
    /// particles, a `TrailerAnim=` or a `Next=` anim or marks `MakeInfantry=`
    /// cells would change the clicking game alone, as in gamemd. A
    /// `Layer=Ground` one keeps its own list (`DisplayLayers::local_ground`),
    /// so it moves no shared member, but draws after every Ground member
    /// rather than at its sort key. Retail RING does none of these.
    pub(crate) fn spawn_move_flash(&mut self, rules: &RuleSet, cell: (u16, u16)) -> bool {
        if rules.general.move_flash.name.is_empty() {
            return false;
        }
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        // 0x004D7EC2..0x004D7EED: the signed Cell words, times 0x100, plus 0x80.
        let centre = crate::sim::cell_kernel::cell_center(
            crate::sim::cell_kernel::CellCoordinate {
                x: i32::from(cell.0 as i16),
                y: i32::from(cell.1 as i16),
            },
            0,
        );
        let Ok(mut z) = crate::sim::movement::ground_pose::query_ground_height(
            &cells,
            crate::sim::components::DriveCoord {
                x: centre.x,
                y: centre.y,
                z: 0,
            },
        ) else {
            return false;
        };
        if cells.flags(cells.lookup_world(centre.x, centre.y)) & 0x100 != 0 {
            z = z.wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
        }
        let world_coord = AnimWorldCoord {
            x: centre.x,
            y: centre.y,
            z,
        };
        let local = !self.session.game_mode().is_single_player();
        let name = &rules.general.move_flash.name;
        // `resolve_type_handles` interned it; a local flash never adds a
        // name, which the other games would lack.
        let type_id = if local {
            let Some(type_id) = self.interner.get(name) else {
                log::debug!("MoveFlash [{name}] was not interned with the type handles");
                return false;
            };
            type_id
        } else {
            self.interner.intern(name)
        };
        let (rx, ry, sub_x, sub_y, level) = world_coord.to_cell_sub_z();
        let descriptor = AnimClassSpawnDescriptor {
            draw_flags: 0x600,
            ..AnimClassSpawnDescriptor::new(type_id, rx, ry, sub_x, sub_y, level)
        };
        let constructor = if local {
            AnimConstructorInput::Local
        } else {
            AnimConstructorInput::Runtime
        };
        match self.spawn_anim_at_world_with_constructor(
            rules,
            descriptor,
            world_coord,
            constructor,
            FrameEffects::default(),
        ) {
            Ok(_) => true,
            Err(AnimSpawnError::Tiberium(error)) => {
                log::warn!("MoveFlash refused an invalid Tiberium Image start: {error}");
                false
            }
            Err(error) => {
                log::debug!("MoveFlash did not construct: {error}");
                false
            }
        }
    }

    /// Execute58 MeteorShower6E2C40..6E2E12 (Steam gamemd). Literal types
    /// are find-only; raw signed XY remainders precede each constructor, whose
    /// launch/Start draws are visible to the following selection. 842AFC holds
    /// [1,5,9,15]. No House, SuperWeapon, no-effects flag or per-piece delay.
    pub(crate) fn trigger_meteor_shower(
        &mut self,
        rules: &RuleSet,
        intensity: i32,
        waypoint: i32,
    ) -> bool {
        let Some(base) = usize::try_from(intensity)
            .ok()
            .and_then(|i| [1i32, 5, 9, 15].get(i).copied())
        else {
            return false;
        };
        let Some([x, y, z]) = self.trigger_waypoint_coord(waypoint) else {
            return false;
        };
        let count = base.wrapping_add(crate::sim::rng::abs_before_modulo(
            self.scenario_rng.next_u32(),
            3,
        ));
        let spread = count.wrapping_mul(70);
        for _ in 0..count {
            let dx = (self.scenario_rng.next_u32() as i32).wrapping_rem(spread);
            let dy = (self.scenario_rng.next_u32() as i32).wrapping_rem(spread);
            let mut coord = AnimWorldCoord {
                x: x.wrapping_add(dx),
                y: y.wrapping_add(dy),
                z,
            };
            coord.z = crate::sim::projectile::projectile_ground_z(
                self.resolved_terrain.as_ref(),
                &self.effective_shared_cell_dummy(),
                crate::sim::projectile::ProjectileCoord::new(coord.x, coord.y, coord.z),
            );
            let name = if crate::sim::rng::abs_before_modulo(self.scenario_rng.next_u32(), 2) == 0 {
                "METSMALL"
            } else {
                "METLARGE"
            };
            //422B20 is find-only. An orphan ART section is not a registered
            //AnimType; safe Rust refuses the native invalid array access.
            if !rules.anim_type_names.contains(name) {
                return false;
            }
            if !self.spawn_named_anim(rules, name, coord, 0, 0x600, 0) {
                return false;
            }
        }
        true
    }

    /// TAction8 DropZoneFlare6E1CC0: its tail waypoint, inline ground/bridge
    ///coordinate, Rules+147C, then Anim421EA0(delay0,loop1,flags600). The
    ///post-constructor +198 write leaves first Start and DropZone reveal live.
    pub(crate) fn trigger_drop_zone_flare(&mut self, rules: &RuleSet, waypoint: i32) -> bool {
        let Some(type_name) = rules.general.drop_zone_anim.as_deref() else {
            //Native would pass the null pointer to the constructor.
            return false;
        };
        let Some([x, y, z]) = self.trigger_waypoint_coord(waypoint) else {
            return false;
        };
        let world_coord = AnimWorldCoord { x, y, z };
        let type_id = self.interner.intern(type_name);
        let (rx, ry, sub_x, sub_y, level) = world_coord.to_cell_sub_z();
        let descriptor = AnimClassSpawnDescriptor {
            draw_flags: 0x600,
            ..AnimClassSpawnDescriptor::new(type_id, rx, ry, sub_x, sub_y, level)
        };
        let id = match self.spawn_anim_at_world(rules, descriptor, world_coord) {
            Ok(id) => id,
            Err(AnimSpawnError::Tiberium(error)) => {
                log::warn!("DropZone flare refused an invalid Tiberium Image start: {error}");
                return false;
            }
            Err(_) => return false,
        };
        self.substrate.set_anim_no_effects(id, true);
        true
    }

    /// Anim ctor42264D..4226BF: the bound DropZone type reveals twice for
    ///the current House. Rules+14BC is converted from leptons by signed /256;
    ///the existing Map5678E0 owner handles height, allies and notifications.
    /// Every native client reveals for its own PlayerPtr, so a network game
    /// reveals for each human House ([`Self::represented_player_maps`]).
    fn reveal_drop_zone_from_anim(&mut self, rules: &RuleSet, world: AnimWorldCoord) {
        let owners = self.represented_player_maps();
        // Preserve large positive inputs through the narrower Rust parameter;
        // Map567A3E/the shared reveal owner still applies the native cap10.
        let radius = (rules.general.drop_zone_radius / 256).min(i32::from(u16::MAX));
        let Ok(radius) = u16::try_from(radius) else {
            //Negative radii index before native's spiral table; keep the
            //existing RevealArea owner's safe supported-range boundary.
            return;
        };
        let (x, y) = crate::sim::movement::coord_cell(crate::sim::components::DriveCoord {
            x: world.x,
            y: world.y,
            z: world.z,
        });
        let config = self.sight_reveal_config(rules);
        for owner in owners {
            let clients = self.fog_viewers();
            let viewers = self.reveal_viewers.direct(
                self.house_alliances.as_map(),
                owner,
                &config,
                &self.interner,
                &clients,
            );
            for final_pass in [false, true] {
                let _ = self.reveal_area(
                    &viewers,
                    (x as u16, y as u16),
                    world.z,
                    radius,
                    None,
                    final_pass,
                    &config,
                    crate::sim::world::UninitContext::with_rules(rules),
                    crate::sim::vision::MapRevealProjection::Transient,
                    self.session.current_house(),
                );
            }
        }
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
        frame_effects: FrameEffects<'_>,
    ) -> Result<AnimId, AnimSpawnError> {
        // Anim42203D assigns before registry insertion and RandomRate4221F5.
        // Death debris may already have constructed at its pick/draw boundary.
        let native_unique_id = match constructor {
            AnimConstructorInput::AssignedIdentity(id) => id,
            AnimConstructorInput::Preconsumed(draws) => draws
                .native_unique_id
                .unwrap_or_else(|| self.next_native_runtime_id()),
            AnimConstructorInput::Runtime => self.next_native_runtime_id(),
            // ClickedAction_Cell sets Scenario+0x214 to -3 around the
            // constructor (0x004D7F42) and restores it after (0x004D7FCB), so
            // NextUniqueID's pre-increment (0x0068BCB0) gives the flash -2,
            // the id the sync checksum skips, and the counter stays put.
            AnimConstructorInput::Local | AnimConstructorInput::Presentation => {
                crate::sim::multiplayer_checksum::SYNC_EXEMPT_ANIM_ID
            }
        };
        let presentation = matches!(constructor, AnimConstructorInput::Presentation);
        let local = presentation || matches!(constructor, AnimConstructorInput::Local);
        // The MoveFlash alone runs in the clicking game only; a presentation
        // anim runs in every game alike, so it may name what it plays.
        let private = local && !presentation;
        let type_name = self
            .interner
            .resolve(descriptor.type_name)
            .to_ascii_uppercase();
        // Borrowed from the rules, which outlive the spawn: the record is
        // only read.
        let config = rules
            .art()
            .anim_runtime_config(&type_name)
            .ok_or(AnimSpawnError::MissingType(descriptor.type_name))?;
        let (effective_end, effective_loop_end) = effective_bounds(&type_name, config)?;
        let reverse = descriptor.reverse || config.reverse;
        // VERA-only: a local anim never adds a name to the interner, which the
        // other games would lack; a StopSound name not yet interned is left out.
        let stop_sound_id = config.stop_sound.as_deref().and_then(|sound| {
            if private {
                self.interner.get(sound)
            } else {
                Some(self.interner.intern(sound))
            }
        });
        if presentation {
            self.intern_presentation_chain(rules, config);
        }
        let stable_id = if local {
            self.substrate.anims.allocate_local_id()
        } else {
            self.substrate.allocate_stable_id()
        };
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
            // Anim42219E: a zero constructor argument reads Type+348;
            // explicit nonzero producer adjustments take precedence.
            z_adjust: if descriptor.z_adjust == 0 {
                config.z_adjust
            } else {
                descriptor.z_adjust
            },
            remap: None,
            effective_end,
            effective_loop_end,
            firing_anim: None,
            runtime: AnimRuntime {
                current_frame: if reverse {
                    effective_loop_end.wrapping_sub(1)
                } else {
                    0
                },
                frame_step: if reverse { -1 } else { 1 },
                delay_remaining: i32::from(descriptor.delay),
                rate_reload: 0,
                frame_timer: CdTimer::default(),
                loop_remaining: native_loop_remaining(config.loop_count, descriptor.loop_count),
                first_ai_guard: true,
                constructor_reverse: descriptor.reverse,
                inactive: false,
                paused: false,
                temporal_paused: false,
            },
            draw_runtime: descriptor.draw_runtime,
            viewer_hidden: None,
            use_cell_drawer: descriptor.use_cell_drawer,
            terrain_attached: descriptor.terrain_attached,
            no_effects: false,
            damage_accumulator: NativeF64Bits::ONE,
            phobos_firepower_mult: NativeF64Bits::ONE,
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
                // 0x004D7F57 writes -5000 to the MoveFlash type's YSortAdjust
                // (+0x340) before a local flash's constructor. RESIDUAL: gamemd
                // keeps that write on the type, so later anims of the type
                // sort with it too; VERA gives it to the flash alone. Retail
                // MoveFlash is RING, which nothing else makes, in Air.
                y_sort_adjust: if private { -5000 } else { config.y_sort_adjust },
            },
            bounce: None,
            flaming_guy: None,
            owner_house: None,
            attached_bullet: None,
            invoker: None,
            invoker_house: None,
            from_death_unit: false,
            death_unit_facing: None,
            death_unit_has_turret: false,
            death_unit_turret_facing: None,
            create_unit_fired: false,
            owner_terrain: None,
            presentation: presentation.then_some(None),
            presentation_order: 0,
        };
        // The insert must run in every build profile: wrapped in
        // `debug_assert!` it was compiled out of release binaries and no
        // scheduler anim ever existed in a shipped build.
        let previous = self.substrate.anims.insert(object);
        debug_assert!(previous.is_none());
        // Phobos `AnimClass_CTOR_CreateUnit_MarkOccupationBits @ 0x4226F0`.
        // A `CreateUnit=` naming no loaded unit type is null at load natively,
        // so it marks nothing.
        if config
            .phobos_create_unit
            .as_ref()
            .is_some_and(|settings| rules.object(&settings.unit_type).is_some())
        {
            Self::anim_mark_all_occupation_bits(
                &mut self.substrate,
                &self.resolved_terrain,
                world_coord,
            );
        }
        let draws = match constructor {
            AnimConstructorInput::Preconsumed(draws) => draws,
            AnimConstructorInput::Runtime
            | AnimConstructorInput::AssignedIdentity(_)
            | AnimConstructorInput::Local
            | AnimConstructorInput::Presentation => {
                // VERA-only: a local anim draws from a copy, so the other
                // games' Scenario RNG stays in step. gamemd draws from the
                // shared one; retail RING has no draw (no `RandomRate=`).
                let mut copy;
                let rng = if local {
                    copy = self.scenario_rng.clone();
                    &mut copy
                } else {
                    &mut self.scenario_rng
                };
                match anim_constructor_draws(config, world_coord, rng) {
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
        let rate_reload = Self::anim_rate(&self.session, config, draws.random_rate);
        let frame_timer =
            CdTimer::started(self.session.binary_frame() as i32, i32::from(rate_reload));
        let registered = self
            .substrate
            .anims
            .get_mut(stable_id)
            .expect("new Anim registry entry");
        registered.runtime.rate_reload = rate_reload;
        registered.runtime.frame_timer = frame_timer;
        registered.bounce = draws.bounce;
        // Meteor422433 moves Object.Location to the computed launch point
        // before Reveal/Start; ordinary Bouncer keeps the supplied location
        // while its physics starts ten leptons higher.
        if config.is_meteor
            && let Some(body) = registered.bounce
        {
            let start = body.position_leptons();
            registered.world_coord = AnimWorldCoord {
                x: start.x,
                y: start.y,
                z: start.z,
            };
        }
        let initial_world_coord = registered.world_coord;
        // Native registry insertion precedes Reveal, and Reveal precedes the
        // delay-zero constructor-time Start call. A producer's attach then
        // removes the Anim from Display and submits it to Ground again, so
        // Reveal's own submit is left out when nothing before the attach can
        // read or change Display. Start's mineral tail can construct children
        // and dispatch damage before the producer attaches its receiver.
        let start_reaches_display = descriptor.delay == 0
            && (config.tiberium_chain_reaction
                || (middle_frame(config).is_none()
                    && (config.scorch
                        || config.crater
                        || (config.spawns_particle.is_some() && config.num_particles > 0))));
        let attaches = descriptor
            .owner
            .is_some_and(|owner| self.anim_owner_coords(owner).is_some());
        self.reveal_anim(stable_id, rules, !attaches || start_reaches_display);
        //42264D..4226BF reveals DropZone before the constructor-time Start.
        if rules
            .general
            .drop_zone_anim
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(&type_name))
        {
            self.reveal_drop_zone_from_anim(rules, initial_world_coord);
        }
        if descriptor.delay == 0 {
            self.anim_start(stable_id, config, rules, None, frame_effects)?;
        }
        if let Some(owner) = descriptor.owner {
            self.set_anim_owner_object(stable_id, Some(owner), rules);
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
            // MapClass::InitCellAttributes568BB0 uses the same scalar
            // destructor4228E0: Release406060, without StopSound.
            self.scalar_delete_anim(id);
            removed += 1;
        }
        removed
    }

    /// One Map565730 lookup and Cell+44/+11E read. Live IsAnimatedTiberium
    /// AI424402 and post-load426224 use this same real/dummy field owner.
    fn anim_cell_overlay_fields(&self, location: AnimWorldCoord) -> (Option<u8>, u8) {
        let grid_fields = |(rx, ry): (u16, u16)| {
            self.overlay_grid
                .as_ref()
                .filter(|grid| rx < grid.width() && ry < grid.height())
                .map(|grid| {
                    let cell = grid.cell(rx, ry);
                    (cell.overlay_id, cell.overlay_data)
                })
        };
        match self.resolved_terrain.as_ref() {
            Some(terrain) => {
                let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
                match cells.lookup_world(location.x, location.y) {
                    cell @ crate::map::cell_index::NativeCellIdentity::Real(_) => {
                        let (rx, ry) = cells.coord(cell);
                        grid_fields((rx as u16, ry as u16)).unwrap_or((None, 0))
                    }
                    crate::map::cell_index::NativeCellIdentity::Dummy => {
                        cells.dummy().overlay_fields()
                    }
                }
            }
            None => Self::map_coord_cell(&self.resolved_terrain, location.x, location.y)
                .and_then(grid_fields)
                .unwrap_or_else(|| self.effective_shared_cell_dummy().overlay_fields()),
        }
    }

    /// Cell485020 value from raw Location via Object5F6960: two Map565730
    /// lookups. Keep its first lookup separate from the shared field read.
    fn anim_cell_tiberium_value(
        &self,
        location: AnimWorldCoord,
        rules: &RuleSet,
        overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    ) -> Result<i32, TiberiumImageError> {
        if let Some(terrain) = self.resolved_terrain.as_ref() {
            let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
            let _ = cells.lookup_world(location.x, location.y);
        }
        let (overlay_id, overlay_data) = self.anim_cell_overlay_fields(location);
        crate::map::tiberium_cell::tiberium_value(
            overlay_id,
            overlay_data,
            overlay_registry,
            &rules.tiberium_types,
        )
    }

    /// Anims_Refresh_Tiberium_LightConverts4261D0, after user-load lighting:
    /// all flagged stored Anims sample raw Location, including inactive and
    /// attached entries. Non-resource cells keep the prior Convert. This is not
    /// the live AI's virtual coordinate minus384, and does not alter timers,
    /// intensity, depth, attachment or RNG.
    pub(crate) fn refresh_animated_tiberium_after_load(
        &mut self,
        rules: &RuleSet,
        overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    ) -> Result<(), TiberiumImageError> {
        // Remap writes have no callbacks. Snapshot the selected coordinates
        // in storage order so the shared setter retains touch invalidation.
        let candidates: Vec<_> = self
            .anims()
            .filter(|(_, anim)| {
                rules
                    .art()
                    .anim_runtime_config(self.interner.resolve(anim.type_id))
                    .is_some_and(|config| config.is_animated_tiberium)
            })
            .map(|(&id, anim)| (id, anim.world_coord))
            .collect();
        for (id, location) in candidates {
            let (Some(overlay), _) = self.anim_cell_overlay_fields(location) else {
                continue;
            };
            let Some(kind) =
                overlay_registry.tiberium_type_for_overlay(&rules.tiberium_types, overlay)?
            else {
                continue;
            };
            let tiberium = rules
                .tiberium_types
                .get(kind)
                .ok_or(TiberiumImageError::MissingType(kind))?;
            if let Ok(index) = u8::try_from(tiberium.color_scheme() / 2) {
                assert!(self.substrate.anims.set_anim_remap(
                    id,
                    AnimRemap::ColorScheme(crate::rules::house_colors::HouseColorIndex(index)),
                ));
            }
        }
        Ok(())
    }

    pub fn visit_anim(
        &mut self,
        id: AnimId,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<bool, RuntimeOverlayError> {
        // One read serves the visit's prefix and gates: most visits end at a
        // gate, and each step that may write the anim re-reads it (VERA-only,
        // results unchanged).
        let Some(anim) = self.anim(id) else {
            return Ok(false);
        };
        let type_id = anim.type_id;
        let mut gates = AnimGates::of(anim);
        // Borrowed from the rules, not the world: the visit below mutates the
        // world while it reads the type.
        let Some(config) = rules
            .art()
            .anim_runtime_config(self.interner.resolve(type_id))
        else {
            self.destroy_anim(id, rules);
            return Ok(false);
        };

        //423B17/423B1E precede the ordinary first-AI, delay and timer gates.
        if config.is_flaming_guy {
            self.anim_flaming_guy_ai(id, &config, rules, overlay_registry);
            let Some(reread) = self.anim(id).map(AnimGates::of) else {
                return Ok(false);
            };
            gates = reread;
        }
        // HideIfNoOre and the MakeInfantry mark read the stored Location
        // (`+0x9C`, `0x00423BD6..0x00423BFD`), not GetCoords: an owner-attached
        // anim stores an owner-relative delta there.
        let location = gates.location;

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
            let value = self.anim_cell_tiberium_value(location, rules, overlay_registry)?;
            // Phobos' `AnimClass_AI_Early @ 0x423B95` replacement keeps the same
            // gate and compares instead: `GetContainedTiberiumValue() <=
            // abs(HideIfNoOre.Threshold)`. `wrapping_abs` keeps the native
            // two's-complement result for INT_MIN, where C++'s `abs` overflows.
            let hidden = value <= config.hide_if_no_ore_threshold.wrapping_abs();
            self.substrate.set_anim_hidden(id, hidden);
        }

        // AnimClass::AI performs this before its first-AI, inactive, delay,
        // visibility, and frame-timer gates. Repeated visits OR the same raw
        // bit; there is deliberately no contributor count.
        if config.make_infantry != -1 {
            Self::apply_make_infantry_raw_occupation(
                &mut self.substrate,
                &self.resolved_terrain,
                location,
                AnimOccupationOperation::Mark,
            );
        }
        // `AnimClass::AI 0x00423C24`: a bouncing chunk flies its body before
        // the trailer and the first-AI guard; touching down ends it.
        if gates.bouncing {
            if let Some(bridge_state_changed) =
                self.anim_bounce_step(id, &config, rules, overlay_registry, frame_effects)?
            {
                return Ok(bridge_state_changed);
            }
            let Some(reread) = self.anim(id).map(AnimGates::of) else {
                return Ok(false);
            };
            gates = reread;
        }

        // AnimAI4242A6/4242B0 gates only this producer on ObjectAlive and
        // inactive. Bounce above still runs before the retirement gate.
        if let Some(trailer_name) = config.trailer_anim.as_deref()
            && !gates.inactive
            && !self.substrate.pending_delete.contains_object(&id)
            && trailer_cadence_matches(
                u64::from(self.session.binary_frame()),
                config.trailer_seperation,
                rules.ares(),
            )
            && rules.art().anim_runtime_config(trailer_name).is_some()
        {
            // The trailer spawns at GetCoords after the body moved the anim.
            let Some(world_coord) = self.anim_absolute_coord(id) else {
                return Ok(false);
            };
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
                owner: None,
            };
            // The strict binder binds the parent's whole `TrailerAnim=`
            // closure; the tolerant one stops on a malformed image header, so
            // a mod's trailer can be unbindable here. It is then not spawned.
            let constructor = self.effect_anim_constructor(rules, trailer_type);
            match self.spawn_anim_at_world_with_constructor(
                rules,
                descriptor,
                world_coord,
                constructor,
                FrameEffects::default(),
            ) {
                Ok(_) => {}
                Err(AnimSpawnError::Tiberium(error)) => {
                    log::warn!(
                        "trailer animation refused an invalid Tiberium Image start: {error}"
                    );
                }
                Err(error) => log::warn!("trailer animation not spawned: {error}"),
            }
            let Some(reread) = self.anim(id).map(AnimGates::of) else {
                return Ok(false);
            };
            gates = reread;
        }

        //AnimAI424322..42435F: a DropZone marker ends over the first ground
        //Building at its current GetCoords, before first-AI/delay/timer gates.
        //NoEffects does not gate this test; a deck-only Building does not end it.
        if rules
            .general
            .drop_zone_anim
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(self.interner.resolve(type_id)))
            && self.anim_absolute_coord(id).is_some_and(|world| {
                Self::map_coord_cell(&self.resolved_terrain, world.x, world.y)
                    .and_then(|(x, y)| {
                        self.substrate.occupancy.first_building_on_layer(
                            x,
                            y,
                            crate::sim::movement::locomotor::MovementLayer::Ground,
                        )
                    })
                    .is_some()
            })
        {
            if let Some(anim) = self.substrate.anims.get_mut(id) {
                anim.runtime.inactive = true; //424358 writes+19B before UnInit.
                gates.inactive = true;
            }
        }

        // AnimAI42435F rereads after prefix callbacks. All inactive writers
        // share this retirement; a Bouncer may already have landed/retired.
        if gates.inactive {
            self.destroy_anim(id, rules);
            return Ok(false);
        }
        if gates.first_ai_guard {
            if let Some(anim) = self.substrate.anims.get_mut(id) {
                anim.runtime.first_ai_guard = false;
            }
            return Ok(false);
        }

        // `AnimClass::AI @ 0x00423AC0` delay countdown: the visit that takes it
        // to zero calls `AnimClass::Start @ 0x00424CE0` (call site `0x004243A1`)
        // and returns. The field also holds the `RandomLoopDelay=` pause, so a
        // looping anim restarts, and replays its start sound, after each pause.
        // `0x00424386..0x00424399` tests `!= 0` and decrements, so a negative
        // pause counts on down and holds the anim.
        if gates.delay_remaining != 0 {
            let Some(anim) = self.substrate.anims.get_mut(id) else {
                return Ok(false);
            };
            anim.runtime.delay_remaining = anim.runtime.delay_remaining.wrapping_sub(1);
            if anim.runtime.delay_remaining == 0 {
                self.anim_start(id, &config, rules, overlay_registry, frame_effects)?;
            }
            return Ok(false);
        }

        // AnimAI4243AE..4243B6 checks Object+90 after first-AI/delay.
        // FlamingGuyAI can retire this receiver before the ordinary tail.
        if self.substrate.pending_delete.contains_object(&id) {
            return Ok(false);
        }
        // Phobos `AnimClass_Update_VeinholeAttack` (hook `0x4243BC`,
        // `Misc/Hooks.VeinholeMonster.cpp:59-67`): an `IsVeins=` anim runs
        // the veinhole attack AI before the animated-tiberium check.
        if config.is_veins {
            self.vein_attack_ai(id, rules, overlay_registry, frame_effects)?;
        }
        // AnimAI4243BC..424427: compare the exact CellAnim type at virtual
        // GetCoords minus384 in XY. A mismatch writes inactive but falls
        // through this visit's frame/damage work; retirement is next visit.
        if config.is_animated_tiberium {
            let Some(mut location) = self.anim_absolute_coord(id) else {
                return Ok(false);
            };
            location.x = location.x.wrapping_sub(384);
            location.y = location.y.wrapping_sub(384);
            let (overlay, _) = self.anim_cell_overlay_fields(location);
            let matches = if let Some(overlay) = overlay {
                let registry = overlay_registry.unwrap_or_else(|| rules.overlay_registry());
                registry
                    .flags(overlay)
                    .ok_or(TiberiumImageError::MissingOverlay(i32::from(overlay)))?
                    .cell_anim
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(self.interner.resolve(type_id)))
            } else {
                false
            };
            if !matches && let Some(anim) = self.substrate.anims.get_mut(id) {
                anim.runtime.inactive = true;
            }
        }
        let mut action = VisitAction::None;
        let mut random_loop_delay = None;
        let current_frame = self.session.binary_frame() as i32;
        //42449B: power pause follows first-AI/delay gates and precedes timer advance;
        //4244A9: the Temporal erase pause (+11A) right after it.
        if gates.paused || gates.rate_reload == 0 || !gates.frame_timer.expired(current_frame) {
            return Ok(false);
        }
        {
            let Some(anim) = self.substrate.anims.get_mut(id) else {
                return Ok(false);
            };
            anim.runtime
                .frame_timer
                .start(current_frame, i32::from(anim.runtime.rate_reload));
            anim.runtime.current_frame = anim
                .runtime
                .current_frame
                .wrapping_add(anim.runtime.frame_step);
        }
        let bridge_state_changed =
            self.anim_frame_damage(id, &config, rules, overlay_registry, frame_effects);
        // AnimAI42464C checks only ObjectAlive after receiver callbacks.
        // Ordinary Anim retirement queues retained storage; scalar deletion
        // removes it. Owner expiry's inactive byte is a later-visit gate.
        if self.anim(id).is_none() || self.substrate.pending_delete.contains_object(&id) {
            return Ok(bridge_state_changed);
        }
        let middle = self.anim(id).is_some_and(|anim| {
            // `0x0042465D..0x00424687`: the committed stage reaching the
            // type's middle frame (`+0x298`, zero meaning Start already ran
            // Middle) calls Middle unless the anim is bouncing (`+0x194`, the
            // constructor's byte, which a `Next=` switch keeps), before the boundary
            // tail.
            middle_frame(&config).is_some_and(|middle| {
                config.start.wrapping_add(anim.runtime.current_frame) == middle
            }) && anim.bounce.is_none()
        });
        if middle {
            self.anim_middle(id, &config, rules, overlay_registry);
        }
        let Some(anim) = self.substrate.anims.get_mut(id) else {
            return Ok(bridge_state_changed);
        };
        let boundary = advance_anim_boundary(anim, &config);
        match boundary {
            AnimBoundary::Continue | AnimBoundary::Bounce => return Ok(bridge_state_changed),
            AnimBoundary::Loop => {
                random_loop_delay = config.random_loop_delay;
            }
            AnimBoundary::Complete => {
                // Phobos `AnimClass_AI_CreateUnit_ActualEffects @ 0x424932`.
                if let Some(settings) = config.phobos_create_unit.clone()
                    && !self.anim(id).is_some_and(|anim| anim.create_unit_fired())
                {
                    if let Some(anim) = self.substrate.anims.get_mut(id) {
                        anim.set_create_unit_fired();
                    }
                    self.anim_create_unit(id, &settings, rules, overlay_registry, frame_effects);
                }
                action = if let Some(next) = config.next.clone() {
                    VisitAction::Next(next)
                } else if config.make_infantry != -1 {
                    VisitAction::MakeInfantry
                } else {
                    VisitAction::Destroy
                };
            }
        }

        // `0x004247B1..0x004247E5`: unless both bounds are 0, Random_Ranged
        // (`0x0065C7E0`) on the Scenario stream draws the signed pause; equal
        // bounds draw nothing.
        if let Some((low, high)) = random_loop_delay {
            let delay = self.scenario_rng.next_range_i32_inclusive(low, high);
            if let Some(anim) = self.substrate.anims.get_mut(id) {
                anim.runtime.delay_remaining = delay;
            }
        }
        match action {
            VisitAction::None => {}
            VisitAction::Destroy => {
                self.anim_spawn_fire_anims(id, &config, rules);
                if let Some(anim) = self.substrate.anims.get_mut(id) {
                    anim.completed = true;
                }
                self.destroy_anim(id, rules);
            }
            VisitAction::MakeInfantry => {
                if self.anim_make_infantry(
                    id,
                    config.make_infantry,
                    rules,
                    overlay_registry,
                    frame_effects,
                ) {
                    self.anim_spawn_fire_anims(id, &config, rules);
                    if let Some(anim) = self.substrate.anims.get_mut(id) {
                        anim.completed = true;
                    }
                    self.destroy_anim(id, rules);
                }
            }
            VisitAction::Next(next) => {
                self.switch_anim_type(id, &next, rules, overlay_registry, frame_effects)?;
            }
        }
        Ok(bridge_state_changed)
    }

    /// AnimClass::AI424513..424647, after the stage/timer commit. Debit the
    /// retained fraction before GetCoords/AreaDamage can retire this emitter.
    /// The body adds no RNG request; receiver consequences keep their owners.
    /// Phobos `AnimExt::VeinAttackAI` (`Ext/Anim/Body.cpp:164-195`, hook
    /// `0x4243BC`): a `IsVeins=` anim on the fully grown weed overlay (index
    /// 126, data >= 0x30, flat cell) retires itself when a precondition fails
    /// and damages every low techno of its cell every other frame with
    /// `[General] VeinDamage=` and `[General] VeinholeWarhead=`.
    ///
    /// RESIDUAL: native passes a null warhead when `VeinholeWarhead=` is
    /// unset; a damage packet needs a live warhead, so an unset/unknown key
    /// deals no damage (retail leaves it unset with `VeinDamage=0`). The TS
    /// `VeinProof` ability has no YR type flag to read.
    fn vein_attack_ai(
        &mut self,
        id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<(), TiberiumImageError> {
        const WEED_OVERLAY_INDEX: u8 = 126;
        const FULLY_GROWN_FROM: u8 = 0x30;
        let Some(location) = self.anim_absolute_coord(id) else {
            return Ok(());
        };
        let (occupiers, retire) = {
            let Some(terrain) = self.resolved_terrain.as_ref() else {
                return Ok(());
            };
            let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
            let identity = cells.lookup_world(location.x, location.y);
            let (rx, ry) = cells.coord(identity);
            let slope = match identity {
                crate::map::cell_index::NativeCellIdentity::Real(index) => {
                    terrain.cells()[index].slope_type
                }
                crate::map::cell_index::NativeCellIdentity::Dummy => {
                    terrain.shared_cell_dummy().snapshot().slope_type
                }
            };
            let (overlay, overlay_data) = self.anim_cell_overlay_fields(location);
            let mut occupiers: Vec<u64> = Vec::new();
            if let Some(cell) = self.substrate.occupancy.get(rx as u16, ry as u16) {
                for layer in [
                    crate::sim::movement::locomotor::MovementLayer::Ground,
                    crate::sim::movement::locomotor::MovementLayer::Bridge,
                    crate::sim::movement::locomotor::MovementLayer::Air,
                    crate::sim::movement::locomotor::MovementLayer::Underground,
                ] {
                    for occupant in cell.iter_layer(layer) {
                        occupiers.push(occupant.entity_id);
                    }
                }
            }
            // `pOccupier->GetHeight() > 0` for the cell list head blocks the
            // anim from continuing.
            let blocked = occupiers.iter().any(|&occupier| {
                self.substrate.entities.get(occupier).is_some_and(|entity| {
                    crate::sim::movement::air_movement::current_fly_height(entity, Some(terrain))
                        > 0
                })
            });
            let retire = occupiers.is_empty()
                || blocked
                || overlay != Some(WEED_OVERLAY_INDEX)
                || overlay_data < FULLY_GROWN_FROM
                || slope != 0;
            (occupiers, retire)
        };
        // `pAnim->UnableToContinue = true`; the same next-visit retirement
        // byte the animated-tiberium mismatch writes.
        if retire && let Some(anim) = self.substrate.anims.get_mut(id) {
            anim.runtime.inactive = true;
        }
        if !self.session.binary_frame().is_multiple_of(2) {
            return Ok(());
        }
        let Some(warhead_name) = rules.phobos_globals.veinhole_warhead.as_deref() else {
            return Ok(());
        };

        let Some(warhead) = rules.warhead(warhead_name) else {
            return Ok(());
        };
        let warhead_ref = self.interner.intern(&warhead.id);
        let damage = rules.general.vein_damage;
        let terrain = self.resolved_terrain.as_ref();
        let targets: Vec<u64> = occupiers
            .into_iter()
            .filter(|&occupier| {
                let Some(entity) = self.substrate.entities.get(occupier) else {
                    return false;
                };
                if !matches!(
                    entity.category(),
                    crate::map::entities::EntityCategory::Structure
                        | crate::map::entities::EntityCategory::Unit
                        | crate::map::entities::EntityCategory::Infantry
                        | crate::map::entities::EntityCategory::Aircraft
                ) {
                    return false;
                }
                let Some(object) = self.object_type(entity.type_ref(), rules) else {
                    return false;
                };
                !object.immune_to_veins
                    && entity.health.current() > 0
                    && entity.lifecycle.object_alive()
                    && crate::sim::movement::air_movement::current_fly_height(entity, terrain) <= 5
            })
            .collect();
        for occupant in targets {
            let receiver = crate::sim::combat::EntityDamageEvent::direct_receiver(
                occupant,
                damage,
                0,
                crate::sim::combat::RAD_NO_ATTACKER,
                None,
                warhead_ref,
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses: false,
                    arg6: false,
                },
            );
            self.commit_direct_damage_receiver(rules, overlay_registry, receiver, effects);
        }
        Ok(())
    }

    fn anim_frame_damage(
        &mut self,
        id: AnimId,
        config: &AnimTypeRuntimeConfig,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let ares = rules.ares_form();
        let Some(anim) = self.anim(id) else {
            return false;
        };
        let damage = f64::from_bits(config.damage.bits());
        //424513..424532 rejects zero/negative/unordered, then instance+194.
        if !(damage > 0.0) || anim.bounce.is_some() {
            return false;
        }
        let terrain_owner = anim
            .owner_entity
            .is_some_and(|owner| self.terrain_objects.get(owner).is_some());
        // Ares AnimClass_Update_DamageDelay1000B1AF..1000B1E7 passes the
        // animation's House, falling back to an attached Techno's owner.
        // Neither owner is an attacking object; keep RAD_NO_ATTACKER below
        // unless `DealtByInvoker` fires.
        let source_house = if ares {
            anim.owner_house.or_else(|| {
                anim.owner_entity
                    .and_then(|owner| self.substrate.entities.get(owner))
                    .map(|owner| owner.owner())
            })
        } else {
            None
        };
        let no_effects = anim.no_effects;
        // Phobos' replaced body (`Ext/Anim/Hooks.cpp:46-47`) rejects the inert
        // flag trigger action 41 writes (`IsInert`,
        // `Ext/TAction/Hooks.cpp:118`) before the accumulator runs, where the
        // stock body gates after it.
        if no_effects {
            return false;
        }
        let type_id = anim.type_id;
        let accumulator = f64::from_bits(anim.damage_accumulator.bits());
        let anim_invoker = anim.invoker();
        let anim_invoker_house = anim.invoker_house();
        let anim_owner_entity = anim.owner_entity;
        let anim_owner_house = anim.owner_house;
        let anim_frame = anim.runtime.current_frame;
        // Rust does not specify arithmetic NaN payloads across platforms.
        let store = |sim: &mut Self, value: f64| {
            let bits = if value.is_nan() {
                0x7ff8_0000_0000_0000
            } else {
                value.to_bits()
            };
            sim.substrate.anims.get_mut(id).unwrap().damage_accumulator =
                NativeF64Bits::from_bits(bits);
        };
        // Phobos `Damage.DealtByInvoker` (`Ext/Anim/Hooks.cpp:101-135`): with
        // the type key or the `[CombatDamage] AnimDamage.DealtByInvoker`
        // global, the attacker becomes the stored invoker (a techno
        // `OwnerObject` when unset) and the damage house the invoker house or
        // the invoker's owner. RESIDUAL: the hook's `IsBuildingAnim ->
        // ParentBuilding` fallback has no VERA model.
        let dealt_by_invoker = config
            .damage_dealt_by_invoker
            .unwrap_or(rules.combat_damage.anim_damage_dealt_by_invoker);
        let anim_attacker = if dealt_by_invoker {
            let mut invoker = anim_invoker;
            if invoker.is_none() {
                // `abstract_cast<TechnoClass*, true>(pOwnerObject)`: the owner
                // object when it is a techno — a Terrain owner lives in the
                // terrain store and does not qualify.
                invoker = anim_owner_entity.and_then(|owner| {
                    self.terrain_objects
                        .get(owner)
                        .is_none()
                        .then_some(owner)
                        .filter(|&owner| self.substrate.entities.contains(owner))
                });
            }
            invoker.unwrap_or(crate::sim::combat::RAD_NO_ATTACKER)
        } else {
            crate::sim::combat::RAD_NO_ATTACKER
        };
        let anim_source_house = if dealt_by_invoker {
            match anim_invoker_house {
                Some(house) => Some(house),
                None => anim_invoker
                    .and_then(|invoker| self.substrate.entities.get(invoker).map(|e| e.owner())),
            }
            .or(anim_owner_house)
            .or_else(|| {
                anim_owner_entity
                    .and_then(|owner| self.substrate.entities.get(owner))
                    .map(|owner| owner.owner())
            })
        } else {
            source_house
        };
        // Phobos `Damage.ApplyFirepowerMult` (`Ext/Anim/Hooks.cpp:119-128`):
        // with the type key or the `[CombatDamage] AnimDamage.ApplyFirepowerMult`
        // global, an anim inside the `Damage.DealtByInvoker` block caches the
        // invoker's current firepower multiplier on its first damage round
        // (`Animation.Value == max(Damage.Delay-1, 1)`, after the applied>0
        // gate) and scales every application by it.
        let apply_firepower = config
            .damage_apply_firepower_mult
            .unwrap_or(rules.combat_damage.anim_damage_apply_firepower_mult)
            && dealt_by_invoker;
        let first_damage_round =
            anim_frame == std::cmp::max(config.ares_damage_delay.wrapping_sub(1), 1);
        let refresh_firepower = |sim: &mut Self| {
            if first_damage_round
                && let Some(multiplier) =
                    crate::sim::combat::world_fire::bullet_init_firepower_multiplier(
                        &sim.interner,
                        &sim.substrate,
                        &sim.houses,
                        rules,
                        anim_attacker,
                    )
                && let Some(anim) = sim.substrate.anims.get_mut(id)
            {
                anim.phobos_firepower_mult = multiplier;
            }
        };
        let fold_firepower = |sim: &mut Self, value: i32| -> i32 {
            if !apply_firepower || value <= 0 {
                return value;
            }
            refresh_firepower(sim);
            let cached = f64::from_bits(
                sim.substrate
                    .anims
                    .get(id)
                    .map_or(NativeF64Bits::ONE, |anim| anim.phobos_firepower_mult)
                    .bits(),
            );
            crate::sim::attach_effect::scale_i32(value, cached)
        };
        // Phobos `Damage.ApplyOncePerLoop` (`Ext/Anim/Hooks.cpp:56-66`): the
        // damage lands only on the loop's first frame —
        // `Animation.Value == max(Damage.Delay-1, 1)` — as
        // `round(Damage)` times the terrain multiplier, bypassing the
        // accumulator and the delay counter. RESIDUAL: Phobos's replaced body
        // would also fold the stock branches through `std::round`; VERA keeps
        // the native/Ares conversion for the stock paths.
        let once_per_loop = if config.damage_apply_once_per_loop {
            let delay = config.ares_damage_delay;
            if anim_frame != std::cmp::max(delay.wrapping_sub(1), 1) {
                return false;
            }
            let applied = phobos_anim_round(damage).wrapping_mul(if terrain_owner { 5 } else { 1 });
            if applied <= 0 {
                return false;
            }
            // Weapon= detonates a bullet of that weapon instead (0x42464C).
            if let Some(weapon) = config
                .ares_weapon
                .as_deref()
                .and_then(|name| rules.weapon(name))
            {
                let applied = fold_firepower(self, applied);
                return self.ares_anim_weapon_detonation(
                    id,
                    weapon,
                    applied,
                    anim_attacker,
                    rules,
                    overlay_registry,
                    effects,
                );
            }
            Some(applied)
        } else {
            None
        };
        let whole = if let Some(applied) = once_per_loop {
            applied
        } else {
            if ares {
                let increment = if terrain_owner { damage * 5.0 } else { damage };
                let accumulated = accumulator + increment;
                store(self, accumulated);
                if !(accumulated >= 1.0) || no_effects {
                    return false;
                }
                //4245AC..4245D5 consumes Math7C5F00's low signed32 result, including
                // masked conversion/overflow, and subtracts that signed integer.
                let whole = X87Chop53::ftol_f64_low_masked(accumulated);
                store(self, accumulated - f64::from(whole));
                whole
            } else {
                let multiplier = if terrain_owner { 5 } else { 1 };
                let delay = config.ares_damage_delay;
                let delayed = delay >= 1 && !(damage < 1.0);
                let counted = if delayed {
                    accumulator + 1.0
                } else {
                    f64::from(multiplier) * damage + accumulator
                };
                store(self, counted);
                let short = if delayed {
                    counted < f64::from(delay)
                } else {
                    counted < 1.0
                };
                if short {
                    return false;
                }
                let whole = if delayed {
                    ares_truncate(damage).wrapping_mul(multiplier)
                } else {
                    ares_truncate(counted)
                };
                if whole < 1 || no_effects {
                    return false;
                }
                store(
                    self,
                    if delayed {
                        0.0
                    } else {
                        counted - f64::from(whole)
                    },
                );
                // Weapon= detonates a bullet of that weapon instead (0x42464C).
                if let Some(weapon) = config
                    .ares_weapon
                    .as_deref()
                    .and_then(|name| rules.weapon(name))
                {
                    let whole = fold_firepower(self, whole);
                    return self.ares_anim_weapon_detonation(
                        id,
                        weapon,
                        whole,
                        crate::sim::combat::RAD_NO_ATTACKER,
                        rules,
                        overlay_registry,
                        effects,
                    );
                }
                whole
            }
        };
        let whole = fold_firepower(self, whole);
        //4245C3..424630 compares the own stored ID exactly, not Image.
        let warhead_name = if self.interner.resolve(type_id) == "INVISO" {
            Some(rules.bridge_warheads.c4_name.as_str())
        } else {
            rules.combat_damage.flame_damage2()
        };
        // Ares1000B179 first reads AnimType+330 (the existing art Warhead
        // reader); only a null pointer uses the stock INVISO/FlameDamage2
        // fallback. Phobos's replaced body keeps the native `pType->Warhead`
        // read, which the ART registry resolves the same way.
        let warhead_name = config.warhead.as_deref().or(warhead_name);
        let Some((name, warhead)) =
            warhead_name.and_then(|name| rules.warhead(name).map(|warhead| (name, warhead)))
        else {
            return false;
        };
        let Some(coord) = self.anim_absolute_coord(id) else {
            return false;
        };
        let warhead_ref = self.interner.intern(name);
        crate::sim::combat::world_receiver::apply_area_damage(
            self,
            rules,
            overlay_registry,
            crate::sim::projectile::ProjectileCoord::new(coord.x, coord.y, coord.z),
            whole,
            warhead,
            true,
            (anim_attacker, anim_source_house, warhead_ref),
            effects,
        )
        .bridge_state_changed
    }

    /// Ares AnimClass_Update_DamageDelay's `Weapon=` arm (1000B116..
    /// 1000B169): BulletTypeExt::CreateBullet of the weapon's Projectile at
    /// the anim's cell, no owner, the computed damage, the weapon's Warhead
    /// and Bright, given the weapon, Removed, detonated at the anim's
    /// GetCoords and Released. That is the bare `DetonateAtCoord`
    /// Fire_Death_Weapon makes (`ProjectileDetonationReason::DeathWeapon`).
    /// Returns whether a bridge changed.
    fn ares_anim_weapon_detonation(
        &mut self,
        id: AnimId,
        weapon: &crate::rules::weapon_type::WeaponType,
        damage: i32,
        attacker: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let (Some(warhead), Some(coord)) =
            (weapon.warhead.as_deref(), self.anim_absolute_coord(id))
        else {
            return false;
        };
        let impact = crate::sim::projectile::ProjectileCoord::new(coord.x, coord.y, coord.z);
        let (rx, ry) = (coord.x.div_euclid(256), coord.y.div_euclid(256));
        let target = match (u16::try_from(rx), u16::try_from(ry)) {
            (Ok(rx), Ok(ry)) => crate::sim::projectile::ProjectileTarget::looked_up_cell(
                self.resolved_terrain.as_ref(),
                rx,
                ry,
            ),
            _ => crate::sim::projectile::ProjectileTarget::DummyCell,
        };
        let payload = crate::sim::projectile::ProjectilePayload::new(
            damage,
            self.interner.intern(warhead),
            weapon
                .projectile
                .as_deref()
                .map(|id| self.interner.intern(id)),
            weapon.bright,
        )
        .with_installed_weapon(Some(self.interner.intern(&weapon.id)));
        let detonation = crate::sim::projectile::ProjectileDetonation {
            projectile_id: id,
            source_id: attacker,
            target,
            impact,
            payload,
            reason: crate::sim::projectile::ProjectileDetonationReason::DeathWeapon,
        };
        self.commit_logic_projectile_detonations(rules, overlay_registry, &[detonation], effects)
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
    ///    With Ares the refused one is UnInit and the anim ends (below).
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
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let Some((location, owner)) = self
            .anim(id)
            .map(|anim| (anim.world_coord, anim.owner_house))
        else {
            return true;
        };
        Self::apply_make_infantry_raw_occupation(
            &mut self.substrate,
            &self.resolved_terrain,
            location,
            AnimOccupationOperation::Clear,
        );
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
                .is_some_and(|state| state.is_defeated())
        };
        let owner = if owner.is_none_or(|house| defeated(self, house)) {
            match self.civilian_side_house(rules) {
                Some(civilian) => {
                    self.substrate.set_anim_owner_house(id, civilian);
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
        // Rules6707DC/67BB10 retains an InfantryType pointer; AnimAI4249F9
        // calls that type's creator, independent of other name registries.
        let Some(object) =
            rules.object_in_category(crate::rules::object_type::ObjectCategory::Infantry, kind)
        else {
            return true;
        };
        let Some(infantry) = self.construct_resolved_object_limbo_at_height(
            object,
            &owner_name,
            rx,
            ry,
            0,
            0,
            rules,
            frame_effects,
        ) else {
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
            frame_effects,
        );
        if placed.is_none() {
            // Ares 3.0 AnimClass_Update (1005C3E0, hook 0x424B23): the
            // refused infantryman is UnInit and the anim ends (TimeToDie,
            // UnInit) instead of trying again.
            if rules.ares_form() {
                self.uninit_with_context(
                    infantry,
                    crate::sim::world::UninitContext::new(rules, overlay_registry)
                        .with_effects(frame_effects),
                );
                return true;
            }
            if let Some(anim) = self.substrate.anims.get_mut(id) {
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
            self.foot_mark_remove(infantry, rules, overlay_registry, frame_effects);
            if let Some(entity) = self.substrate.entities.get_mut(infantry) {
                entity.on_bridge.store(true);
            }
            self.foot_mark_put(infantry, rules, overlay_registry, frame_effects);
        }
        let human = self
            .houses
            .get(&owner)
            .is_some_and(|house| house.is_human());
        if !human && let Some(entity) = self.substrate.entities.get_mut(infantry) {
            crate::sim::mission::authority::queue_entity_mission_deferred(
                entity,
                crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Hunt),
            );
        }
        true
    }

    pub(crate) fn destroy_anim(&mut self, id: AnimId, rules: &RuleSet) {
        self.destroy_anim_with_context(id, rules);
    }

    pub(crate) fn destroy_anim_with_context(&mut self, id: AnimId, rules: &RuleSet) {
        if self.substrate.pending_delete.contains_object(&id)
            || self.substrate.anims.local_delete_queued(id)
        {
            return;
        }
        // `AnimClass::GetCoords @ 0x00422BE0` — the sound plays at the anim's
        // resolved world position, not its owner-relative stored delta.
        let Some(world) = self.anim_absolute_coord(id) else {
            return;
        };
        let Some(stop_sound) = self
            .anim(id)
            .map(|anim| (!anim.no_effects()).then_some(anim.stop_sound_id).flatten())
        else {
            return;
        };
        // UnInit4255C3 calls owner+60 before SetOwner(NULL)4255CA. Keep
        // the shared attachment flag and every matching owner link in that
        // operation, including the ordinary non-Building path.
        self.detach_anim_from_owner(id, rules);
        if let Some(anim) = self.substrate.anims.get_mut(id) {
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
            .emit(SimSoundEvent::ObjectSoundReleased { owner: id });
        if let Some(stop_sound_id) = stop_sound {
            // Native425618 plays StopSound after releasing the Report. The
            // preceding release removes this owner's handle, so the existing
            // stop/play consumer cannot interrupt the released Report.
            self.sound_events.emit(SimSoundEvent::AnimationStopped {
                anim_id: id,
                stop_sound_id: Some(stop_sound_id),
                world,
            });
        }
        // Object::UnInit5F6616 broadcasts the Anim's expiry before Limbo;
        // Building44EA1A..44EA4F clears its matching damage-fire slot or
        // runs the building-slot transition before this Anim leaves Logic.
        self.notify_anim_pointer_expired(id);
        self.substrate.clear_damage_fire_anim_reference(id);
        self.building_anim_pointer_expired(id, rules);
        // Limbo5F4D30→DetachAll5F4D61 announces again. The first callback
        // normally removed the reverse index; preserve the second visit for
        // native dead-building gates that retained the matched slot. Both
        // notifications precede Display5F4D79 and Logic5F4DD3 removal.
        self.notify_anim_pointer_expired(id);
        self.building_anim_pointer_expired(id, rules);
        self.conceal_anim(id);
        // VERA-only: a local anim waits for the drain outside the shared,
        // hashed queue gamemd puts it in.
        if is_local_anim_id(id) {
            self.substrate.anims.queue_local_delete(id);
        } else {
            self.substrate.pending_delete.queue_object(id);
        }
    }

    /// Building451A2C, ClearAnimSlot451E40 and Infantry5185C3 use scalar
    /// deletion: the old object is gone before the caller continues. Keep this
    /// separate from an animation's ordinary deferred Destroy operation.
    pub(crate) fn scalar_delete_anim(&mut self, id: AnimId) {
        // Anim VT7E3354+20 ->426590 ->4228E0 releases sound handles but
        // never reaches UnInit4255B0 or its StopSound playback. The slot was
        // cleared by the caller before these synchronous destructor effects.
        let sound_active = self.anim(id).is_some_and(|anim| anim.start_sound_active);
        self.notify_anim_pointer_expired(id);
        self.substrate.clear_damage_fire_anim_reference(id);
        self.substrate.release_last_anim_owner_flag(id);
        self.substrate.release_anim_owner_reference(id);
        self.substrate.clear_building_anim_reference(id);
        if sound_active {
            self.sound_events
                .emit(SimSoundEvent::ObjectSoundReleased { owner: id });
        }
        self.conceal_anim(id);
        self.substrate.pending_delete.remove_object(id);
        // Scalar slot deletion451A2C ends its inherited FoggedOut199 flag;
        // the immutable FoggedObject record does not retain this Anim pointer.
        self.fog.forget_fogged_live_object(id);
        if let Some(anim) = self.substrate.anims.remove(id) {
            self.release_firing_anim_count(&anim);
        }
    }

    /// AnimClass::SetOwnerObject424B50. Normal detach preserves absolute
    /// coordinates and gates Remove/Submit on the entry Object+74 mark. Attach
    /// always removes, stores owner-relative coordinates, then submits Ground.
    /// The owner+17C callback is a bare RET5F43C0 for supported Techno owners.
    /// Last detach clears Object+84; every attach sets it before publishing CC.
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
        // A terrain owner (Ares terrain fire): the same detach, whose
        // last-anim callback is TerrainClass vt+0x17C, Finish_Fire_Death
        // (`0x00424BB0`). Its stored coordinate is already absolute.
        let owner_terrain = anim.owner_terrain;
        if let Some(terrain) = owner_terrain {
            if marked {
                self.substrate.display.remove(id);
            }
            let last = !self.substrate.anims.another_owned_by_terrain(id, terrain);
            if last {
                self.terrain_finish_fire_death(terrain, rules);
            }
            self.substrate.anims.set_owner_terrain(id, None);
            if marked {
                self.submit_anim_display(id, rules);
            }
        }
        let Some(anim) = self.anim(id) else {
            return false;
        };
        if anim.owner_entity.is_some() {
            if marked {
                self.substrate.display.remove(id);
            }
            self.substrate.release_last_anim_owner_flag(id);
            let absolute = self.anim_absolute_coord(id).expect("live Anim");
            self.substrate.anims.set_owner_entity(id, None);
            let anim = self.substrate.anims.get_mut(id).expect("live Anim");
            anim.world_coord = absolute;
            if marked {
                self.submit_anim_display(id, rules);
            }
        }
        if let Some(owner_id) = new_owner {
            let Some(owner_coord) = self.anim_owner_coords(owner_id) else {
                return false;
            };
            if !self.substrate.entities.contains(owner_id) {
                // A terrain owner: attach without the coordinate conversion
                // (see `AnimObject::owner_terrain`) or the Object+84 flag,
                // which no terrain reader asks.
                self.substrate.display.remove(id);
                self.substrate.anims.set_owner_terrain(id, Some(owner_id));
                self.submit_anim_display(id, rules);
                return true;
            }
            self.substrate.display.remove(id);
            // A presentation anim leaves the owner's flag alone
            // (`AnimConstructorInput::Presentation`).
            if !self.anim(id).is_some_and(AnimObject::is_presentation) {
                self.substrate
                    .entities
                    .get_mut(owner_id)
                    .expect("the resolved Anim owner remains stored")
                    .set_attached_anim_flag(true);
            }
            let anim = self.substrate.anims.get_mut(id).expect("live Anim");
            anim.world_coord = AnimWorldCoord {
                x: anim.world_coord.x.wrapping_sub(owner_coord.x),
                y: anim.world_coord.y.wrapping_sub(owner_coord.y),
                z: anim.world_coord.z.wrapping_sub(owner_coord.z),
            };
            self.substrate.anims.set_owner_entity(id, Some(owner_id));
            self.submit_anim_display(id, rules);
        }
        true
    }

    /// Anim GetLayer424CB0: attached -> Ground; missing type -> Air; otherwise
    /// the current type's layer. Phobos `Layer.UseObjectLayer` overrides an
    /// attached anim (owner's layer for true, the type's for false).
    pub(crate) fn submit_anim_display(&mut self, id: AnimId, rules: &RuleSet) {
        // A presentation anim keeps the layer it draws in instead of a slot.
        if self.anim(id).is_some_and(AnimObject::is_presentation) {
            let layer = self.anim_display_layer(id, rules);
            let order = self.substrate.display.unshared_order();
            if let Some(anim) = self.substrate.anims.get_mut(id) {
                anim.presentation = Some(layer);
                anim.presentation_order = order;
            }
            return;
        }
        if let Some(layer) = self.anim_display_layer(id, rules) {
            Self::submit_object_display(&mut self.substrate, id, layer);
        } else {
            self.substrate.display.remove(id);
        }
    }

    pub(crate) fn anim_display_layer(
        &self,
        id: AnimId,
        rules: &RuleSet,
    ) -> Option<crate::sim::world::display_layers::DisplayLayer> {
        use crate::rules::art_data::AnimLayer;
        use crate::sim::world::display_layers::DisplayLayer;
        let anim = self.substrate.anims.get(id)?;
        let config = rules
            .art()
            .anim_runtime_config(self.interner.resolve(anim.type_id));
        let type_layer =
            |config: Option<&crate::rules::art_data::AnimTypeRuntimeConfig>| match config
                .map(|config| config.layer)
            {
                None => Some(DisplayLayer::AIR),
                Some(AnimLayer::Ground) => Some(DisplayLayer::GROUND),
                Some(AnimLayer::Top) => Some(DisplayLayer::TOP),
                Some(AnimLayer::Other(index)) => {
                    u8::try_from(index).ok().and_then(DisplayLayer::from_index)
                }
            };
        let attached = anim.owner_entity.is_some() || anim.owner_terrain.is_some();
        if attached {
            // Phobos `AnimClass_InWhichLayer` (hook `0x424CB0`,
            // `Ext/Anim/Hooks.cpp:354-375`): a set `Layer.UseObjectLayer`
            // picks the owner object's layer (true) or the type's (false).
            if let Some(use_object_layer) =
                config.and_then(|config| config.phobos_layer_use_object_layer)
            {
                if !use_object_layer {
                    return type_layer(config);
                }
                // The hook reads OwnerObject only; a terrain-owned anim keeps
                // the stock attached layer.
                return match anim.owner_entity {
                    Some(owner) => self.entity_display_layer(owner, rules),
                    None => Some(DisplayLayer::GROUND),
                };
            }
            return Some(DisplayLayer::GROUND);
        }
        type_layer(config)
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
    /// this engine mirrors it in `anim_pointer_expired`. The fallback
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

    /// Intern now every name a presentation anim of type `config` may intern
    /// on a later visit: the types it can become or make (`Next=`,
    /// `TrailerAnim=`, `ExpireAnim=`, `BounceAnim=`) and their StopSounds.
    /// A game loaded or rejoined while the anim lives lacks the anim, so the
    /// interner must not wait for its visits to learn them.
    fn intern_presentation_chain(&mut self, rules: &RuleSet, config: &AnimTypeRuntimeConfig) {
        let children = |config: &AnimTypeRuntimeConfig| {
            [
                config.next.clone(),
                config.trailer_anim.clone(),
                config.expire_anim.clone(),
                config.bounce_anim.clone(),
            ]
            .into_iter()
            .flatten()
        };
        let mut pending: Vec<String> = children(config).collect();
        let mut seen: Vec<String> = Vec::new();
        while let Some(name) = pending.pop() {
            if seen.contains(&name) {
                continue;
            }
            self.interner.intern(&name);
            if let Some(child) = rules.art().anim_runtime_config(&name) {
                if let Some(sound) = child.stop_sound.as_deref() {
                    self.interner.intern(sound);
                }
                pending.extend(children(child));
            }
            seen.push(name);
        }
    }

    /// Whether presentation anim `id` rides an owner its UnInit has ended.
    pub(crate) fn presentation_anim_orphaned(&self, id: AnimId) -> bool {
        self.anim(id)
            .filter(|anim| anim.is_presentation())
            .and_then(|anim| anim.owner_entity)
            .is_some_and(|owner| {
                self.substrate
                    .entities
                    .get(owner)
                    .is_none_or(|entity| !entity.lifecycle.object_alive())
            })
    }

    /// The presentation anims ([`AnimConstructorInput::Presentation`]) that
    /// draw this frame, each with its layer, in the order they were made. The
    /// picture draws each where Display would hold it as an ordinary anim
    /// (`Simulation::picture_draw_order`).
    pub fn presentation_anims(
        &self,
    ) -> impl Iterator<Item = (AnimId, crate::sim::world::display_layers::DisplayLayer)> + '_ {
        self.substrate
            .anims
            .presentation_iter()
            .filter(|(id, _)| !self.substrate.anims.local_delete_queued(**id))
            .filter_map(|(&id, anim)| Some((id, anim.presentation_layer()?)))
    }

    /// Every Anim's [`Self::anim_absolute_coord`], in one walk of the store,
    /// the local anims last: the presentation places what it draws by these.
    pub fn anim_absolute_coords(&self) -> impl Iterator<Item = (AnimId, AnimWorldCoord)> + '_ {
        let terrain = self.resolved_terrain.as_ref();
        let anims = &self.substrate.anims;
        anims
            .iter()
            .chain(anims.local_iter())
            .map(move |(&id, anim)| {
                (
                    id,
                    anim_world_coords(anim, &self.substrate.entities, terrain),
                )
            })
    }

    pub(crate) fn anim_owner_coords(&self, owner_id: u64) -> Option<AnimWorldCoord> {
        // A terrain owner's centre is its GetCoords (Terrain VT+48 ->
        // Object5F65A0), for the Ares terrain fire.
        if !self.substrate.entities.contains(owner_id) {
            let centre = self.terrain_objects.get(owner_id)?.world_coord();
            return Some(AnimWorldCoord {
                x: centre.x,
                y: centre.y,
                z: centre.z,
            });
        }
        Some(anim_owner_world_coords(
            self.substrate.entities.get(owner_id)?,
            self.resolved_terrain.as_ref(),
        ))
    }

    /// Anim Destroy's owner callback (Techno710410 -> Object5F6DA0) clears
    /// other owner references, not Building damage-fire slots. Those clear
    /// only when this Anim broadcasts its own pointer expiry (44EA45).
    pub(crate) fn detach_anim_from_owner(&mut self, id: AnimId, rules: &RuleSet) -> Option<u64> {
        // A terrain owner's +60 is the Object base, which keeps no anim.
        if let Some(terrain) = self.substrate.anims.owner_terrain(id) {
            self.set_anim_owner_object(id, None, rules);
            return Some(terrain);
        }
        let owner = self.anim(id)?.owner_entity?;
        if let Some(entity) = self.substrate.entities.get_mut(owner) {
            entity.attached_anim_expired(id);
        }
        self.set_anim_owner_object(id, None, rules);
        Some(owner)
    }

    /// Phobos `DetachOnCloak=` (`Ext/AnimType/Body.cpp:112`, `Valueable<bool>`
    /// constructor `true`): `Detach_All(false)` on a cloaking object detaches
    /// every attached anim (`TechnoClass::Do_Cloak @ 0x703789` reaches vtable
    /// `+0xDC`), and the hook `AnimClass_Detach_Cloak @ 0x425174`
    /// (`Ext/Anim/Hooks.cpp:663-684`) suppresses that detach only for a type
    /// whose key reads `no`.
    pub(crate) fn detach_attached_anims_for_cloak(&mut self, owner_id: u64, rules: &RuleSet) {
        let ids: Vec<AnimId> = self
            .anims()
            .filter(|(_, anim)| anim.owner_entity() == Some(owner_id))
            .filter(|(_, anim)| {
                let name = self.interner.resolve(anim.type_id());
                {
                    anim_type_detach_on_cloak(rules, name)
                }
            })
            .map(|(&id, _)| id)
            .collect();
        for id in ids {
            self.detach_anim_from_owner(id, rules);
        }
    }

    /// Anim PointerExpired425150 removes Display, calls owner+60, clears +CC,
    /// sets +19B and Mark(REMOVE). Stored relative coordinates remain unchanged.
    /// Normal SetOwner(NULL) would instead convert and potentially resubmit.
    /// Its independent attached-Bullet arm clears +17C before UnInit (4251AD).
    /// Native comparisons: tools/spatial_oracle/display_anim_owner.json.
    ///
    /// AI checks +19B at42435F after its bounce/trailer/DropZone prefix. The
    /// same gate serves owner expiry, occupied-cell424358 and the later
    /// IsAnimatedTiberium424427 writer; the latter finishes its current visit.
    pub(crate) fn anim_pointer_expired(
        &mut self,
        id: AnimId,
        expired_id: u64,
        rules: &RuleSet,
    ) -> bool {
        let mut changed = false;
        let owner = self
            .anim(id)
            .and_then(|anim| anim.owner_entity.or(anim.owner_terrain));
        if owner == Some(expired_id) {
            self.substrate.display.remove(id);
            if let Some(entity) = self.substrate.entities.get_mut(expired_id) {
                entity.attached_anim_expired(id);
            }
            self.substrate.release_anim_owner_reference(id);
            self.substrate.mark_anim_display(id, false);
            if let Some(anim) = self.substrate.anims.get_mut(id) {
                anim.runtime.inactive = true;
            }
            changed = true;
        }
        // Independent later +17C arm4251AD..4251C8. Clear before UnInit;
        // the owner arm above does not suppress this callback.
        if self
            .anim(id)
            .is_some_and(|anim| anim.attached_bullet == Some(expired_id))
        {
            self.substrate.anims.set_attached_bullet(id, None);
            self.destroy_anim_with_context(id, rules);
            changed = true;
        }
        changed
    }

    /// Manual frame writer62979A..6297D7: restart the owned stage timer,
    /// write its reload rate (+C0 at6297B7), then set the requested frame.
    /// The animation remains in Logic and must retain this rate on expiry.
    pub(crate) fn set_anim_manual_frame(
        session: &ScenarioSession,
        substrate: &mut ObjectSubstrate,
        id: AnimId,
        frame: i32,
        duration: u16,
    ) {
        let current = session.binary_frame() as i32;
        if let Some(anim) = substrate.anims.get_mut(id) {
            anim.runtime.frame_timer = CdTimer::started(current, i32::from(duration));
            anim.runtime.rate_reload = duration;
            anim.runtime.current_frame = frame;
        }
    }

    /// Original43B5E0: caller supplies the target's GetCoords and the name;
    /// construct delay0/loop1/flags600, then SetBullet424C90, SetHouse424CA0
    /// and hidden+19D. PointerExpired425150 owns the eventual bullet detach.
    pub(crate) fn spawn_bullet_warning(
        &mut self,
        rules: &RuleSet,
        name: &str,
        world: AnimWorldCoord,
        bullet: u64,
        house: InternedId,
    ) -> Option<AnimId> {
        let type_name = self.interner.intern(name);
        let (rx, ry, sub_x, sub_y, z) = world.to_cell_sub_z();
        let descriptor = AnimClassSpawnDescriptor {
            draw_flags: 0x600,
            ..AnimClassSpawnDescriptor::new(type_name, rx, ry, sub_x, sub_y, z)
        };
        let id = match self.spawn_anim_at_world(rules, descriptor, world) {
            Ok(id) => id,
            Err(AnimSpawnError::Tiberium(error)) => {
                log::warn!("bullet warning refused an invalid Tiberium Image start: {error}");
                return None;
            }
            Err(_) => return None,
        };
        self.substrate.set_anim_attached_bullet(id, Some(bullet));
        self.substrate.set_anim_owner_house(id, house);
        self.substrate.set_anim_hidden(id, true);
        Some(id)
    }

    /// `0x0043B4C0`, the test `AnimClass::AI` runs every frame for an anim
    /// whose type sets `PsiWarning=` (`AnimTypeClass+0x373`,
    /// `0x00423B29..0x00423B5C`) and stores negated as the hidden byte
    /// `+0x19D`: does `viewer` (native asks for the local player,
    /// `0x00A83D4C`) detect the anim? Not when it is allied with the anim's
    /// House (`HouseClass::IsAlliedWith @ 0x004F9A50`, null not allied).
    /// Otherwise, when one of its PsychicDetection buildings
    /// ([`Simulation::psychic_detection_sites`]) is operational and its
    /// GetCoords lies within the radius in leptons (`<< 8`, `jle` at
    /// `0x0043B5A9`) of the centre of the anim's cell (`MapClass::operator[]
    /// @ 0x00565730`, `CellClass::Get_Center_Coords @ 0x00480A30`), measured
    /// as `CoordStruct::Distance3D` (inline at `0x0043B547..0x0043B593`).
    /// The answer depends on the local player, so the presentation asks it
    /// in place of the hidden flag. Evidence: instruction reading only; no
    /// native run compares the distance's rounding at the radius edge.
    pub fn psi_warning_detected_by(
        &self,
        viewer: InternedId,
        anim: &AnimObject,
        rules: &RuleSet,
    ) -> bool {
        if anim.owner_house.is_some_and(|house| {
            crate::sim::combat::combat_weapon::is_ally_by_object(
                Some(&self.house_alliances),
                &self.interner,
                viewer,
                house,
            )
        }) {
            return false;
        }
        let terrain = self.resolved_terrain.as_ref();
        let at = anim_world_coords(anim, &self.substrate.entities, terrain);
        // The lookup stamps the fallback CellClass on a miss, as native's
        // does. The presentation asks this, so the stamp stays private to
        // the query: drawing must not change the simulation.
        let cell = self.effective_shared_cell_dummy().detached(|| {
            crate::sim::projectile::cell_ground_coord(
                terrain,
                (at.x / 256) as u16,
                (at.y / 256) as u16,
            )
        });
        self.psychic_detection_sites(viewer, rules)
            .any(|(site, radius)| {
                crate::util::native_x87::distance_3d_leptons(
                    [site.x, site.y, site.z],
                    [cell.x, cell.y, cell.z],
                ) <= radius
            })
    }

    pub(crate) fn update_building_damage_fire(&mut self, building_id: u64, rules: &RuleSet) {
        let Some((current, type_ref, position, prior_state, category)) =
            self.substrate.entities.get(building_id).map(|entity| {
                (
                    entity.health.current(),
                    entity.type_ref(),
                    entity.position,
                    entity.damage_fires.active,
                    entity.category(),
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
        // Borrowed from the rules, which outlive this visit: the image and
        // foundation are read only when the fires start.
        let image = &object_type.image;
        let foundation = &object_type.foundation;
        let ratio = if can_be_occupied {
            rules.general.condition_red
        } else {
            rules.general.condition_yellow
        };
        // Building43FC39..43FC84: CanBeOccupied selects red versus yellow;
        // TEST AH,0x41 includes unordered, with no HP/Strength positivity gate.
        let active = crate::sim::combat::Health::new(current).compare_ratio(maximum, ratio)
            != crate::util::native_x87::MaskedX87Ordering::Greater;
        if active == prior_state {
            return;
        }
        if !active {
            self.clear_building_damage_fire_slots(building_id, rules);
        } else if !rules.general.damage_fire_types.is_empty() {
            let type_count = rules.general.damage_fire_types.len();
            let mut type_index = self
                .scenario_rng
                .next_range_u32_inclusive(0, type_count.saturating_sub(1) as u32)
                as usize;
            let offsets = rules
                .art()
                .get(image)
                .map(|entry| entry.damage_fire_offsets.clone())
                .unwrap_or_default();
            let (foundation_w, foundation_h) = foundation.dimensions();
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
                    .and_then(|entity| entity.damage_fires.anim_ids[slot]);
                if occupied.is_some() {
                    break;
                }
                let Some(offset) = offsets.get(slot).copied() else {
                    break;
                };
                // 43C124..43C137: an offset equal to the zero Point2D at
                // 89C848 (never written) ends the fires, as the reader's end
                // marker does; an explicit `DamageFireOffsetN=0,0` stops here.
                if offset.pixel_x == 0 && offset.pixel_y == 0 {
                    break;
                }
                let fire_name = &rules.general.damage_fire_types[type_index].name;
                let fire_type = self.interner.intern(fire_name);
                let descriptor = AnimClassSpawnDescriptor {
                    type_name: fire_type,
                    rx: position.rx(),
                    ry: position.ry(),
                    sub_x: position.sub_x(),
                    sub_y: position.sub_y(),
                    z: position.z(),
                    delay: 0,
                    loop_count: 1,
                    draw_flags: TRAILER_DRAW_FLAGS,
                    z_adjust: 0,
                    reverse: false,
                    use_cell_drawer: false,
                    terrain_attached: false,
                    draw_runtime: AnimDrawRuntime::default(),
                    owner: None,
                };
                let world = AnimWorldCoord {
                    x: base.x.wrapping_add(offset.world_dx),
                    y: base.y.wrapping_add(offset.world_dy),
                    z: base.z,
                };
                let anim_id = match self.spawn_anim_at_world(rules, descriptor, world) {
                    Ok(id) => id,
                    Err(AnimSpawnError::Tiberium(error)) => {
                        log::warn!(
                            "damage-fire animation refused an invalid Tiberium Image start: {error}"
                        );
                        break;
                    }
                    // A map or mod can list a `DamageFireTypes=` entry with
                    // no bound art. Nothing is written for this slot before
                    // the spawn, so the building keeps its earlier fires and
                    // gets no more this pass, as for the trailer above.
                    Err(error) => {
                        log::warn!("damage-fire animation not spawned: {error}");
                        break;
                    }
                };
                // Original43C0D0/43C1EB retains only the Building5C8 slot.
                // The constructor leaves Anim+CC null: absolute coordinates
                // and the expiry listener do not require Object attachment.
                self.substrate
                    .anims
                    .get_mut(anim_id)
                    .expect("new damage fire")
                    .damage_fire_slot = Some((building_id, slot as u8));
                if let Some(entity) = self.substrate.entities.get_mut(building_id) {
                    entity.damage_fires.anim_ids[slot] = Some(anim_id);
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
                self.substrate
                    .set_anim_frame_and_z_adjust(anim_id, frame, z_adjust);
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
            entity.damage_fires.active = active;
        }
    }

    /// Recovery43FCA4, destructor43BDE0 and Building Limbo 4458E4..445903
    /// call Anim UnInit4255B0 (deferred), not scalar delete, on each of the
    /// eight slots and null it; none of them writes5E8 here.
    /// The destructor follows owner expiry, so its Anims no longer need Rules
    /// for SetOwner's intermediate display resubmission.
    pub(crate) fn clear_building_damage_fire_slots(&mut self, building_id: u64, rules: &RuleSet) {
        if !self.substrate.entities.contains(building_id) {
            return;
        }
        for slot in 0..DAMAGE_FIRE_SLOT_COUNT {
            let anim_id = self
                .substrate
                .entities
                .get(building_id)
                .and_then(|entity| entity.damage_fires.anim_ids[slot]);
            let Some(anim_id) = anim_id else {
                continue;
            };
            self.destroy_anim_with_context(anim_id, rules);
            if let Some(entity) = self.substrate.entities.get_mut(building_id) {
                entity.damage_fires.anim_ids[slot] = None;
            }
        }
    }

    /// Phobos `AnimExt::ChangeAnimType` (`Ext/Anim/Body.cpp:198-243`), the
    /// retype one live anim takes when its cumulative `AttachEffectType`
    /// layer count picks another body
    /// (`New/Entity/AttachEffectClass.cpp:520-526`) instead of a destroy and
    /// re-create.
    ///
    /// In order: the fraction travelled through the old body
    /// (`Value / Type->End`, `1.0` for a zero-length body where native divides
    /// by zero), the new type's `End`/`LoopEnd` resolved when the art reader
    /// left them at -1, the type swap, `resetLoops` re-arming
    /// `RemainingIterations` as `static_cast<byte>(LoopCount)`, `Accum = 0`,
    /// `UnableToContinue = false`, `Reverse =` the new type's own byte, the
    /// fresh `Rate=`/`RandomRate=` pick (`0x004221F5`) normalized by
    /// `Normalized=`, and finally either a restart at the new body's first
    /// frame (`Value = Reverse ? End : Start`, then `AnimClass::Start`) or the
    /// proportional frame `int(End * fraction)`.
    ///
    /// VERA stores the stage offset the constructor writes rather than native's
    /// absolute `Value`, so the proportional arm is exact for the retail
    /// `Start=0` bodies and the restart arm re-arms the same offset a fresh
    /// construction would. A body with a nonzero `Start=` lands within one
    /// `Start` of native in both arms; no retail ART sets that key.
    pub(crate) fn change_anim_type(
        &mut self,
        id: AnimId,
        new_type: InternedId,
        reset_loops: bool,
        restart: bool,
        rules: &RuleSet,
    ) -> bool {
        let Some(object) = self.substrate.anims.get(id) else {
            return false;
        };
        // The native caller retypes only when the resolved name differs.
        if object.type_id == new_type {
            return false;
        }
        let old_end = object.effective_end;
        let value = object.runtime.current_frame;
        let fraction = if old_end == 0 {
            1.0
        } else {
            f64::from(value) / f64::from(old_end)
        };
        let name = self.interner.resolve(new_type).to_ascii_uppercase();
        let Some(config) = rules.art().anim_runtime_config(&name).cloned() else {
            return false;
        };
        let Ok((effective_end, effective_loop_end)) = effective_bounds(&name, &config) else {
            return false;
        };
        let reverse = config.reverse;
        let rate = Self::choose_anim_rate(&self.session, &mut self.scenario_rng, &config);
        let now = self.session.binary_frame() as i32;
        let stop_sound_id = config
            .stop_sound
            .as_deref()
            .map(|sound| self.interner.intern(sound));
        let frame = if restart {
            if reverse {
                effective_loop_end.wrapping_sub(1)
            } else {
                0
            }
        } else {
            (f64::from(effective_end) * fraction) as i32
        };
        let Some(object) = self.substrate.anims.get_mut(id) else {
            return false;
        };
        object.type_id = new_type;
        object.effective_end = effective_end;
        object.effective_loop_end = effective_loop_end;
        object.stop_sound_id = stop_sound_id;
        object.runtime.frame_step = if reverse { -1 } else { 1 };
        object.runtime.current_frame = frame;
        object.runtime.rate_reload = rate;
        object.runtime.frame_timer = CdTimer::started(now, i32::from(rate));
        if reset_loops {
            object.runtime.loop_remaining = config.loop_count as u8;
        }
        // `pAnim->Start()` in the restart arm is native `AnimClass::Start
        // @ 0x00424CE0` (start sound + `Middle`), not the frame-rate
        // `Animation.Start(rate, direction)` line above it.
        if restart {
            let _ = self.anim_start(id, &config, rules, None, FrameEffects::empty());
        }
        true
    }

    fn choose_anim_rate(
        session: &ScenarioSession,
        scenario_rng: &mut SimRng,
        config: &AnimTypeRuntimeConfig,
    ) -> u16 {
        let drawn = anim_random_rate(config, scenario_rng);
        Self::anim_rate(session, config, drawn)
    }

    /// The frame delay from `Rate=` or a drawn `RandomRate=` pick, normalized
    /// to the game speed for `Normalized=` types.
    fn anim_rate(
        session: &ScenarioSession,
        config: &AnimTypeRuntimeConfig,
        random_rate: Option<u16>,
    ) -> u16 {
        let delay = random_rate.unwrap_or(config.rate_logic_frames);
        if config.normalized {
            session.game_options().normalized_anim_delay(delay)
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
    fn anim_bounce_step(
        &mut self,
        id: AnimId,
        config: &AnimTypeRuntimeConfig,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<Option<bool>, RuntimeOverlayError> {
        let Some(mut body) = self.anim(id).and_then(|anim| anim.bounce) else {
            return Ok(None);
        };
        let outcome = self.anim_bounce_update(&mut body, rules);
        if config.is_meteor
            && let Err(error) = body.compensate_meteor_gravity()
        {
            log::warn!("meteor anim left the verified x87 domain: {error}");
            self.destroy_anim(id, rules);
            return Ok(Some(false));
        }
        let position = body.position_leptons();
        if let Some(anim) = self.substrate.anims.get_mut(id) {
            anim.bounce = Some(body);
        }
        let mut bridge_state_changed = false;
        match outcome {
            BounceOutcome::Falling => {}
            BounceOutcome::Bounced => {
                bridge_state_changed |= self.anim_bounce_contact(
                    id,
                    config,
                    rules,
                    overlay_registry,
                    position,
                    frame_effects,
                );
            }
            BounceOutcome::Stopped => self.destroy_anim(id, rules),
        }
        if let Some(anim) = self.substrate.anims.get_mut(id) {
            anim.world_coord = AnimWorldCoord {
                x: position.x,
                y: position.y,
                z: position.z,
            };
        }
        if outcome == BounceOutcome::Falling {
            return Ok(None);
        }
        bridge_state_changed |=
            self.anim_bounce_landing(config, rules, overlay_registry, position, frame_effects)?;
        self.destroy_anim(id, rules);
        Ok(Some(bridge_state_changed))
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
        _overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        position: glam::IVec3,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        if let (Some(bounce_anim), Some(coord)) =
            (config.bounce_anim.as_deref(), self.anim_absolute_coord(id))
        {
            self.spawn_named_anim(rules, bounce_anim, coord, 0, BOUNCE_CONTACT_DRAW_FLAGS, 0);
        }
        let Some(warhead_name) = config.warhead.as_deref() else {
            return false;
        };
        let damage = match X87Chop53::load_f64(config.damage).and_then(X87Chop53::ftol_i64) {
            Ok(damage) => damage as i32,
            Err(_) => return false,
        };
        // Ares 3.0 AnimClass_UpdateBounce_Damage (1005C410, hook 0x4239F0):
        // a negative DamageRadius or a Damage of zero damages nothing.
        if rules.ares() && (config.damage_radius < 0 || damage == 0) {
            return false;
        }
        let warhead_ref = self.interner.intern(warhead_name);
        self.apply_bounce_contact_damage(
            position,
            config.damage_radius,
            crate::sim::world::BounceContactDamage::Anim(damage),
            warhead_ref,
            rules,
            frame_effects,
        )
    }

    /// The landing arm proper (`0x00423C4A..0x00423EF8`); see
    /// [`Self::anim_bounce_step`].
    fn anim_bounce_landing(
        &mut self,
        config: &AnimTypeRuntimeConfig,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        position: glam::IVec3,
        frame_effects: FrameEffects<'_>,
    ) -> Result<bool, RuntimeOverlayError> {
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
        //Phobos `ExplodeOnWater=` (`Ext/AnimType/Body.cpp:99`,
        //`Ext/Anim/Body.cpp:252`): a water landing still takes the explosion
        //arm when the type sets the key.
        let explode_on_water = config.explode_on_water;
        if self.bounce_cell_is_water(position, rules) && !above_deck && !explode_on_water {
            //423CDB..423DE2: a meteor uses the LAST splash, without a Wake.
            //Phobos `SplashAnims=`/`SplashAnims.PickRandom=`
            //(`Ext/AnimType/Body.cpp:102-103`, `Ext/Anim/Hooks.cpp:236-241`):
            //the type's list, filtered by the animation registry, replaces
            //the stock `SplashList=`, and PickRandom draws
            //`RandomRanged(0, last)` before the Wake is created.
            let splash_names: Vec<&str> = match config.splash_anims.as_deref() {
                Some(names) => names
                    .iter()
                    .filter(|name| {
                        rules
                            .anim_type_names
                            .iter()
                            .any(|valid| valid.eq_ignore_ascii_case(name))
                    })
                    .map(String::as_str)
                    .collect(),
                None => rules
                    .combat_damage
                    .splash_list
                    .iter()
                    .map(String::as_str)
                    .collect(),
            };
            let splash = if splash_names.is_empty() {
                None
            } else if config.splash_anims_pick_random {
                let index = self
                    .scenario_rng
                    .next_range_u32_inclusive(0, (splash_names.len() - 1) as u32)
                    as usize;
                splash_names.get(index).copied()
            } else if config.is_meteor {
                splash_names.last().copied()
            } else {
                splash_names.first().copied()
            };
            if !config.is_meteor {
                let wake = rules.general.wake.name.clone();
                self.spawn_named_anim(rules, &wake, location, 0, BOUNCE_CONTACT_DRAW_FLAGS, 0);
            }
            if let Some(splash) = splash {
                let splash_coord = AnimWorldCoord {
                    z: location.z.wrapping_add(BOUNCE_SPLASH_LIFT_LEPTONS),
                    ..location
                };
                self.spawn_named_anim(rules, splash, splash_coord, 0, BOUNCE_CONTACT_DRAW_FLAGS, 0);
            }
            return Ok(false);
        }
        //423C4A..423EFD: the stock explosion prefix requires ExpireAnim.
        //The Spawns/IsTiberium continuation still runs without it or a Warhead.
        let mut bridge_state_changed = false;
        // Phobos `AnimExt::HandleDebrisImpact` (`Ext/Anim/Body.cpp:253-280`)
        // replaces the expiry block (`0x423CC7`): the warhead applies whenever
        // the type names one, before the ExpireAnim and without requiring it.
        // Stock `0x423DE7` nests both under the ExpireAnim slot and creates
        // the anim first, so retain that order without the feature.
        {
            bridge_state_changed |= self.bounce_landing_warhead(
                config,
                rules,
                overlay_registry,
                position,
                frame_effects,
            );
            if let Some(expire) = config.expire_anim.as_deref() {
                self.spawn_named_anim(
                    rules,
                    expire,
                    location,
                    0,
                    BOUNCE_EXPIRE_DRAW_FLAGS,
                    BOUNCE_EXPIRE_Z_ADJUST,
                );
            }
        }
        //423F37..423FC4: two inclusive draws precede the live constructor loop.
        //Each child completes constructor/Start before constructing the next.
        if let Some(child) = config.spawns.as_deref().filter(|_| config.spawn_count > 0) {
            let first = self
                .scenario_rng
                .next_range_i32_inclusive(0, config.spawn_count);
            let second = self
                .scenario_rng
                .next_range_i32_inclusive(0, config.spawn_count);
            let count = bounce_spawn_count(true, config.spawn_count, first, second);
            for _ in 0..count {
                self.spawn_named_anim(rules, child, location, 0, BOUNCE_CONTACT_DRAW_FLAGS, 0);
            }
        }
        if config.is_tiberium && !above_deck {
            self.anim_landing_tiberium(config, rules, overlay_registry, position, frame_effects)?;
        }
        Ok(bridge_state_changed)
    }

    /// The landing warhead: Phobos `WarheadTypeExt::DetonateAt` for a
    /// `Warhead.Detonate=yes` type (`Ext/Anim/Body.cpp:256-263`), else the
    /// stock `DamageArea` plus `FlashbangWarheadAt` pair (`0x423E75..0x423EFD`).
    fn bounce_landing_warhead(
        &mut self,
        config: &AnimTypeRuntimeConfig,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        position: glam::IVec3,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let (Some(warhead_name), Ok(damage)) = (
            config.warhead.as_deref(),
            X87Chop53::load_f64(config.damage).and_then(X87Chop53::ftol_i64),
        ) else {
            return false;
        };
        let Some(warhead) = rules.warhead(warhead_name) else {
            return false;
        };
        let damage = damage as i32;
        let warhead_ref = self.interner.intern(warhead_name);
        let impact =
            crate::sim::projectile::ProjectileCoord::new(position.x, position.y, position.z);
        let detonate_at = config.phobos_warhead_detonate;
        if detonate_at {
            // `WarheadTypeExt::DetonateAt` builds a bullet with no weapon and
            // explodes it at the landing coords instead of the stock damage
            // area.
            let packet = crate::sim::projectile::ProjectileDetonation {
                projectile_id: u64::MAX,
                source_id: crate::sim::combat::RAD_NO_ATTACKER,
                target: crate::sim::projectile::ProjectileTarget::None,
                impact,
                payload: crate::sim::projectile::ProjectilePayload::new(
                    damage,
                    warhead_ref,
                    None,
                    warhead.bright,
                ),
                reason: crate::sim::projectile::ProjectileDetonationReason::AnimWarheadDetonate,
            };
            return self.commit_logic_projectile_detonations(
                rules,
                overlay_registry,
                &[packet],
                frame_effects,
            );
        }
        let bridge_state_changed = crate::sim::combat::world_receiver::apply_area_damage(
            self,
            rules,
            overlay_registry,
            impact,
            damage,
            warhead,
            true,
            (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
            frame_effects,
        )
        .bridge_state_changed;
        if crate::sim::combat::combat_light_chance_admits(self, rules, warhead_ref) {
            self.combat_light_requests
                .emit(crate::sim::combat::CombatLightRequest::Impact {
                    target_id: None,
                    damage,
                    warhead_ref,
                    coord: impact,
                    force_create: false,
                    flags: 0,
                });
        }
        bridge_state_changed
    }

    /// AnimAI423FD0..424269. The radius gates a squared-distance loop, but
    /// the placement direction is ONLY outer_x&7 (89F688), not (x,y).
    /// Repeated visits and CanGerminate-before-null-SpawnType are intentional.
    fn anim_landing_tiberium(
        &mut self,
        config: &AnimTypeRuntimeConfig,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        position: glam::IVec3,
        frame_effects: FrameEffects<'_>,
    ) -> Result<(), RuntimeOverlayError> {
        use crate::sim::tiberium::{NewTiberiumAdmission, TiberiumPlacementObjectContext};
        let Some(registry) = registry else {
            return Ok(());
        };
        let radius = config.tiberium_spread_radius;
        let first = radius.wrapping_neg();
        if first > radius {
            return Ok(());
        }
        for x in first..=radius {
            for y in first..=radius {
                if crate::util::native_x87::sqrt_approx_length([x, y]) > radius {
                    continue;
                }
                let (dx, dy) = crate::sim::tiberium::ADJACENT_OFFSETS[(x & 7) as usize];
                let cell = (
                    (position.x / 256).wrapping_add(dx) as u16,
                    (position.y / 256).wrapping_add(dy) as u16,
                );
                let Some(base) = config
                    .tiberium_spawn_type
                    .as_deref()
                    .and_then(|name| registry.id_for_name(name))
                else {
                    continue;
                };
                let ramp_type_allows =
                    crate::sim::tiberium::overlay_tiberium_allows_ramps(rules, registry, base);
                let admitted = {
                    let (Some(terrain), Some(grid)) =
                        (self.resolved_terrain.as_ref(), self.overlay_grid.as_ref())
                    else {
                        return Ok(());
                    };
                    let objects = TiberiumPlacementObjectContext::new(
                        &self.substrate.entities,
                        &self.substrate.occupancy,
                        &self.substrate.raw_cell_occupation,
                        rules,
                        &self.interner,
                        self.terrain_objects.cell_index(),
                    );
                    crate::sim::tiberium::admit_new_tiberium_target(
                        grid,
                        self.terrain_objects.spawning_cells(),
                        NewTiberiumAdmission::runtime(terrain, objects),
                        cell,
                        ramp_type_allows,
                    )
                };
                let Some(cell) = admitted else {
                    continue;
                };
                let variant = self.scenario_rng.next_range_i32_inclusive(0, 3);
                if let Ok(selected) = u8::try_from(i32::from(base).wrapping_add(variant)) {
                    crate::sim::crates::construct_runtime_overlay(
                        self,
                        &rules.crate_rules,
                        rules,
                        registry,
                        cell,
                        selected,
                        OverlayRecalc::Immediate,
                        frame_effects,
                    )?;
                }
                //424140 executes even when Overlay Unlimbo refused the cell.
                let density = self.scenario_rng.next_range_i32_inclusive(0, 2) as u8;
                if let Some(grid) = self.overlay_grid.as_mut() {
                    //424155: like Place487190's post-constructor store, a
                    //literal Cell+11E write even when no overlay was installed.
                    grid.write_cell_data(cell.0, cell.1, density);
                }
                self.dirty_cells.mark_tactical_once(cell);
                //Anim's ore arm does not call AddToGrowthQueue or radar6551C0.
            }
        }
        Ok(())
    }

    /// `new AnimClass(AnimTypes[FindIndex(name)], &coord, delay, 1, flags,
    /// zAdjust, 0)`: a landing chunk's follow-up anims and the rocket puffs of
    /// Rocket Process and the spawn manager's Boomer launch. A type that never
    /// bound constructs nothing. This optional-effect boundary also reports
    /// and refuses an invalid Tiberium Image reached during the child's Start;
    /// the core constructor and the live Anim visitor return that error.
    pub(crate) fn spawn_named_anim(
        &mut self,
        rules: &RuleSet,
        type_name: &str,
        coord: AnimWorldCoord,
        delay: u16,
        draw_flags: u32,
        z_adjust: i32,
    ) -> bool {
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
        match self.spawn_anim_at_world(rules, descriptor, coord) {
            Ok(_) => true,
            Err(AnimSpawnError::Tiberium(error)) => {
                log::warn!(
                    "optional animation [{type_name}] refused an invalid Tiberium Image start: {error}"
                );
                false
            }
            Err(error) => {
                log::debug!("anim [{type_name}] did not construct: {error}");
                false
            }
        }
    }

    /// Phobos `AnimExt::SpawnFireAnims` (`Ext/Anim/Body.cpp:309-405`), the body
    /// of `DEFINE_HOOK(0x425060, AnimClass_Expire_ScorchFlamer, 0x6)`
    /// (`Ext/Anim/Hooks.cpp:693-703`): a `Flamer=` or `Scorch=` anim that
    /// completes runs its small fire loop, and a `Flamer=` one its large loop.
    ///
    /// Per iteration the native order is `RandomDouble()` for the chance,
    /// `RandomRanged(0, n-1)` when the anim vector holds more than one entry,
    /// then the spawned anim's constructor `RandomRanged(1, 2)` loop count.
    /// Chances and distances fall back to the last authored entry, then to the
    /// per-arm defaults; distances are cells scaled by 256 leptons.
    ///
    /// RESIDUAL: `MapClass::GetRandomCoordsNear` is not ported; the point
    /// reuses the recorded single-angle form (`sim::projectile::launch`). The
    /// `Chances`/`Distances` shorter-than-count arm reads past its vector in
    /// gamemd (a native bug); VERA reads the last entry instead.
    fn anim_spawn_fire_anims(
        &mut self,
        id: AnimId,
        config: &crate::rules::art_data::AnimTypeRuntimeConfig,
        rules: &RuleSet,
    ) {
        use crate::rules::phobos_anim_type_ext::LandTypeFlags;

        if !(config.flamer || config.scorch) {
            return;
        }
        let Some(coord) = self.anim(id).map(|anim| anim.world_coord()) else {
            return;
        };
        let land = self.resolved_terrain.as_ref().map(|terrain| {
            let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
            cells.land_type(cells.lookup_world(coord.x, coord.y))
        });
        let disallowed =
            config
                .phobos_fire_anim_disallowed_land_types
                .unwrap_or(if config.scorch {
                    LandTypeFlags::DEFAULT_DISALLOWED
                } else {
                    LandTypeFlags::NONE
                });
        if land
            .and_then(|index| crate::rules::terrain_rules::LandType::from_index(index as u8))
            .is_some_and(|land| disallowed.contains(land))
        {
            return;
        }
        let constrain = config.phobos_constrain_fire_anims_to_cell_spots;
        let attach = config
            .phobos_attach_fire_anims_to_parent
            .unwrap_or(config.scorch);
        // `SpawnAnim`'s `attach && pThis->OwnerObject` arm is the anim's
        // `SetOwnerObject`; VERA's spawned anims take the default owner.
        let _ = attach;
        let small_count = config
            .phobos_small_fire_count
            .unwrap_or(1 + i32::from(config.flamer));
        if config.flamer {
            self.fire_anim_loop(
                rules,
                coord,
                &config.phobos_small_fire_anims,
                &config.phobos_small_fire_chances,
                &config.phobos_small_fire_distances,
                small_count.clamp(0, i32::MAX as i32),
                rules.general.small_fire_anim.as_str(),
                (0.5, 1.0),
                (64, 160),
                constrain,
            );
            self.fire_anim_loop(
                rules,
                coord,
                &config.phobos_large_fire_anims,
                &config.phobos_large_fire_chances,
                &config.phobos_large_fire_distances,
                config.phobos_large_fire_count.clamp(0, i32::MAX as i32),
                rules.general.large_fire_anim.as_str(),
                (0.5, 0.5),
                (112, 112),
                constrain,
            );
        } else if config.scorch {
            self.fire_anim_loop(
                rules,
                coord,
                &config.phobos_small_fire_anims,
                &config.phobos_small_fire_chances,
                &config.phobos_small_fire_distances,
                small_count.clamp(0, i32::MAX as i32),
                rules.general.small_fire_anim.as_str(),
                (1.0, 1.0),
                (0, 0),
                constrain,
            );
        }
    }

    /// `AnimExt::SpawnFireAnims`'s `LoopAnims` lambda
    /// (`Ext/Anim/Body.cpp:342-377`).
    #[allow(clippy::too_many_arguments)]
    fn fire_anim_loop(
        &mut self,
        rules: &RuleSet,
        coord: AnimWorldCoord,
        anims: &[String],
        chances: &[crate::util::native_x87::NativeF64Bits],
        distances: &[crate::util::native_x87::NativeF64Bits],
        count: i32,
        default_anim: &str,
        default_chances: (f64, f64),
        default_distances: (i32, i32),
        constrain: bool,
    ) {
        for index in 0..count {
            let i = index as usize;
            let chance = if chances.len() > i {
                f64::from_bits(chances[i].bits())
            } else if let Some(last) = chances.last() {
                f64::from_bits(last.bits())
            } else if index == 0 {
                default_chances.0
            } else {
                default_chances.1
            };
            // `if (chance < random.RandomDouble()) continue;`
            if chance < self.scenario_rng.random_double() {
                continue;
            }
            let anim_type = if anims.len() > 1 {
                let pick = self
                    .scenario_rng
                    .next_range_i32_inclusive(0, anims.len() as i32 - 1)
                    as usize;
                anims[pick].clone()
            } else if let Some(first) = anims.first() {
                first.clone()
            } else {
                default_anim.to_owned()
            };
            if anim_type.is_empty() {
                continue;
            }
            let distance = if distances.len() > i {
                leptons_of_cells(f64::from_bits(distances[i].bits()))
            } else if let Some(last) = distances.last() {
                leptons_of_cells(f64::from_bits(last.bits()))
            } else if index == 0 {
                default_distances.0
            } else {
                default_distances.1
            };
            self.spawn_fire_anim(rules, &anim_type, coord, distance, constrain);
        }
    }

    /// `AnimExt::SpawnFireAnims`'s `SpawnAnim` lambda
    /// (`Ext/Anim/Body.cpp:317-340`).
    fn spawn_fire_anim(
        &mut self,
        rules: &RuleSet,
        type_name: &str,
        coord: AnimWorldCoord,
        distance: i32,
        constrain: bool,
    ) {
        let mut target = coord;
        if distance > 0 {
            let jittered = crate::sim::projectile::launch::random_coords_near(
                self.cell_grid(),
                crate::sim::projectile::ProjectileCoord {
                    x: coord.x,
                    y: coord.y,
                    z: coord.z,
                },
                distance,
                &mut self.scenario_rng,
            );
            target = AnimWorldCoord::from(jittered);
            if constrain {
                let (rx, ry, _, _, z) = target.to_cell_sub_z();
                let spot = crate::sim::movement::bump_crush::pick_infantry_sublocation(
                    self.resolved_terrain.as_ref(),
                    &self.substrate.raw_cell_occupation,
                    &self.substrate.occupancy,
                    &self.substrate.entities,
                    rules,
                    &self.interner,
                    &mut self.scenario_rng,
                    crate::sim::components::DriveCoord {
                        x: target.x,
                        y: target.y,
                        z: target.z,
                    },
                )
                .map(|(spot, _)| spot);
                let (sub_x, sub_y) = crate::util::lepton::subcell_lepton_offset(spot);
                target = AnimWorldCoord::from_cell_sub_z(rx, ry, sub_x, sub_y, z);
            }
        }
        let loop_count = self.scenario_rng.next_range_i32_inclusive(1, 2);
        let type_id = self.interner.intern(type_name);
        let (rx, ry, sub_x, sub_y, z) = target.to_cell_sub_z();
        let descriptor = crate::sim::components::AnimClassSpawnDescriptor {
            delay: 0,
            loop_count,
            draw_flags: 0x600,
            z_adjust: 0,
            reverse: false,
            ..crate::sim::components::AnimClassSpawnDescriptor::new(
                type_id, rx, ry, sub_x, sub_y, z,
            )
        };
        let _ = self.spawn_anim_at_world(rules, descriptor, target);
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
    /// reads `Report=` into it only when `StartSound=` resolved to `-1`
    /// (`0x00428344..0x004283BC`); `config.start_sound` holds that slot.
    ///
    /// Constructor-time starts pass no overlay registry; no retail marking
    /// anim has fewer than two frames, so none marks from there.
    fn anim_start(
        &mut self,
        id: AnimId,
        config: &AnimTypeRuntimeConfig,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<(), AnimSpawnError> {
        let ares = rules.ares_form();
        let sound_name = self.anim(id).and_then(|anim| {
            (!anim.no_effects())
                .then(|| config.start_sound.clone())
                .flatten()
        });
        if let Some(sound_name) = sound_name
            && let Some(world) = self.anim_absolute_coord(id)
        {
            if let Some(anim) = self.substrate.anims.get_mut(id) {
                anim.start_sound_active = true;
            }
            self.sound_events.emit(SimSoundEvent::AnimationStarted {
                anim_id: id,
                sound_id: sound_name,
                world,
            });
        } else if self.anim(id).is_some_and(|anim| anim.start_sound_active) {
            // Start424D32 clears the prior start handle when NoEffects is
            // set or the new type has no StartSound. A late TAction41 flag
            // never cancels the constructor's sound before this later Start.
            if let Some(world) = self.anim_absolute_coord(id) {
                self.sound_events.emit(SimSoundEvent::AnimationStopped {
                    anim_id: id,
                    stop_sound_id: None,
                    world,
                });
            }
            self.substrate
                .anims
                .get_mut(id)
                .expect("restarting animation remains present")
                .start_sound_active = false;
        }
        // Phobos `DetachedReport=` (`0x424CF1`, `Ext/Anim/Hooks.cpp:269-280`):
        // `VocClass::PlayAt` at the animation's own coordinate, with no
        // NoEffects gate in the hook.
        if let Some(sound_id) = config.detached_report.clone()
            && let Some(world) = self.anim_absolute_coord(id)
        {
            self.sound_events.emit(SimSoundEvent::VocAtCoord {
                sound_id,
                coordinate: DriveCoord {
                    x: world.x,
                    y: world.y,
                    z: world.z,
                },
                flags: 0,
            });
        }
        if middle_frame(config).is_none() {
            self.anim_middle(id, config, rules, overlay_registry);
        }

        // AnimClass::Start424D5F..424EF3, original gamemd instructions:
        // the constructor reaches this before TAction41's late NoEffects.
        // Middle may run callbacks, so read the live instance/type again.
        let Some(type_id) = self
            .anim(id)
            .filter(|anim| !anim.no_effects())
            .map(|anim| anim.type_id())
        else {
            return Ok(());
        };
        if !rules
            .art()
            .anim_runtime_config(self.interner.resolve(type_id))
            .is_some_and(|config| config.tiberium_chain_reaction)
        {
            return Ok(());
        }
        let Some(coord) = self.anim_absolute_coord(id) else {
            return Ok(());
        };
        let registry = rules.overlay_registry();
        let cell = {
            let Some(terrain) = self.resolved_terrain.as_ref() else {
                return Ok(());
            };
            let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
            match crate::sim::movement::ground_pose::query_object_cell(
                &cells,
                crate::sim::components::DriveCoord {
                    x: coord.x,
                    y: coord.y,
                    z: coord.z,
                },
            ) {
                receiver @ crate::map::cell_index::NativeCellIdentity::Real(_) => {
                    let (x, y) = cells.coord(receiver);
                    (x as u16, y as u16)
                }
                crate::map::cell_index::NativeCellIdentity::Dummy => {
                    if cells.dummy().overlay_fields().0.is_some_and(|overlay| {
                        registry.flags(overlay).is_some_and(|flags| flags.tiberium)
                    }) {
                        log::warn!(
                            "Anim.Start mineral-bearing shared Dummy mutation is unsupported"
                        );
                    }
                    return Ok(());
                }
            }
        };
        let resource = self
            .overlay_grid
            .as_ref()
            .filter(|grid| cell.0 < grid.width() && cell.1 < grid.height())
            .map(|grid| {
                crate::sim::tiberium::tiberium_cell_view(
                    grid,
                    registry,
                    &rules.tiberium_types,
                    cell,
                )
            })
            .transpose()?
            .flatten();
        let Some(resource) = resource else {
            return Ok(());
        };
        // Retain the selected class across removal; the cleared overlay no
        // longer identifies its Debris or Color (424DAA..424DC0).
        let mineral = rules
            .tiberium_types
            .get(resource.tiberium_type)
            .expect("resolved mineral type");
        Self::reduce_tiberium_at_with_native_context(
            &self.terrain_objects,
            &self.ore_growth_config,
            &mut self.ore_growth_state,
            &self.session,
            &mut self.scenario_rng,
            &self.substrate,
            &mut self.resolved_terrain,
            &mut self.overlay_grid,
            &mut self.dirty_cells,
            cell,
            i32::from(resource.overlay_data) + 1,
            rules,
            Some(registry),
        )?;
        // Reduce480A80 publishes Recalc's caches before the child constructor.
        // No zone repair belongs to that first Recalc.
        self.publish_recalculated_cells(rules, &[cell]);
        let debris = mineral.debris();
        let throw_debris = if ares {
            // Ares 3.0 10050D50 (hook 0x424DD3), after the Debris count test:
            // RandomRanged(0, 99) < Debris.Chance replaces the one-in-three draw.
            !debris.is_empty()
                && self.scenario_rng.next_range_i32_inclusive(0, 99)
                    < mineral.ares().debris_chance()
        } else {
            // 424DDF..424DF3 uses one signed raw draw. Only zero is consumed:
            // abs(raw % 3)==0 also matches wrapped abs(raw)%3==0 at INT_MIN.
            !debris.is_empty() && self.scenario_rng.next_raw_abs_modulo(3) == 0
        };
        if throw_debris {
            if let Some(mut child_coord) = self.anim_absolute_coord(id) {
                child_coord.z = child_coord.z.wrapping_add(10);
                let selected = self
                    .scenario_rng
                    .next_range_u32_inclusive(0, (debris.len() - 1) as u32)
                    as usize;
                let (rx, ry, sub_x, sub_y, level) = child_coord.to_cell_sub_z();
                let type_name = self.interner.intern(&debris[selected]);
                let descriptor = AnimClassSpawnDescriptor {
                    draw_flags: 0x600,
                    ..AnimClassSpawnDescriptor::new(type_name, rx, ry, sub_x, sub_y, level)
                };
                match self.spawn_anim_at_world_with_constructor(
                    rules,
                    descriptor,
                    child_coord,
                    AnimConstructorInput::Runtime,
                    frame_effects,
                ) {
                    Ok(child) => {
                        // 424E72..424E95 assigns Convert and retained Cell
                        // light AFTER all child constructor/Start callbacks.
                        if let Ok(entry) = u8::try_from(mineral.color_scheme() / 2) {
                            self.substrate.anims.set_anim_remap(
                                child,
                                AnimRemap::ColorScheme(
                                    crate::rules::house_colors::HouseColorIndex(entry),
                                ),
                            );
                        }
                        self.retain_anim_cell_light(child, Some(cell));
                    }
                    Err(error @ AnimSpawnError::Tiberium(_)) => return Err(error),
                    Err(error) => {
                        log::warn!("Anim.Start mineral Debris failed to construct: {error}")
                    }
                }
            }
        }
        // The damage centre is raw Object.Location, whereas the retained Cell
        // and child above use GetCoords. Zero damage still leaves the suffix.
        // RESIDUAL: Start returns nothing, so the bridge-change flag is dropped
        // and a bridge hit here skips that frame's bridge art refresh; only a
        // TiberiumExplosionDamage above retail's 0 reaches it.
        // Ares 3.0 10050DA0 (hook 0x424EC5): the mineral's ExplosionWarhead
        // and ExplosionDamage, else C4Warhead and TiberiumExplosionDamage.
        let (warhead_name, damage) = if ares {
            (
                mineral
                    .ares()
                    .explosion_warhead(Some(rules.bridge_warheads.c4_name.as_str())),
                mineral
                    .ares()
                    .explosion_damage(rules.combat_damage.tiberium_explosion_damage),
            )
        } else {
            (
                Some(rules.bridge_warheads.c4_name.as_str()),
                rules.combat_damage.tiberium_explosion_damage,
            )
        };
        if let Some(raw) = self.anim(id).map(|anim| anim.world_coord)
            && let Some(warhead_name) = warhead_name
            && let Some(warhead) = rules.warhead(warhead_name)
        {
            let warhead_ref = self.interner.intern(warhead_name);
            crate::sim::combat::world_receiver::apply_area_damage(
                self,
                rules,
                Some(registry),
                crate::sim::projectile::ProjectileCoord::new(raw.x, raw.y, raw.z),
                damage,
                warhead,
                false,
                (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
                frame_effects,
            );
        }
        // Start424EDA/424EE8/424EF3 is a distinct, unconditional second
        // Recalc -> AssignOrphaned -> hierarchy update, never a second Reduce.
        if let (Some(grid), Some(terrain)) =
            (self.overlay_grid.as_mut(), self.resolved_terrain.as_mut())
        {
            grid.recalculate_runtime_cell(terrain, registry, cell);
        }
        self.publish_cell_and_assign_orphaned_zone(rules, cell, None);
        Ok(())
    }

    /// `AnimClass::Middle @ 0x00424F00`: marks the ground under the anim.
    ///
    /// The size is the type's middle-frame size (`ArtEntry::frame_width`,
    /// 30 x 30 without an image). Nothing lands when the anim stands 30
    /// leptons or more above the ground (`vtable+0x1C8`, `ObjectClass::
    /// GetHeight @ 0x005F5F40`, measured from the stored Location; anims
    /// never set OnBridge). The marks land at `GetCoords` (`vtable+0x48`).
    ///
    /// Before the mark-height gate,425008..42504B sends SpawnsParticle to
    /// the shared GasCloudSys NumParticles times at the raw Location.
    fn anim_middle(
        &mut self,
        id: AnimId,
        config: &AnimTypeRuntimeConfig,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        if let Some(kind) = config
            .spawns_particle
            .as_deref()
            .and_then(|name| rules.p_type_id_by_name(name))
            && let Some(coord) = self.anim(id).map(|anim| anim.world_coord)
        {
            let spread = rules.ares_form() && config.ares_spawns_particle_range != [0, 0];
            if spread {
                self.ares_spawn_particles_around(id, kind, config, rules);
            } else {
                for _ in 0..config.num_particles {
                    self.spawn_gas_particle(
                        kind,
                        glam::IVec3::new(coord.x, coord.y, coord.z),
                        rules,
                    );
                }
            }
        }
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
        // 0x4250E1: the AnimType's Phobos key, else the `[General]` fallback.
        let destroy_tiberium = config
            .crater_destroy_tiberium
            .unwrap_or(rules.phobos_globals.anim_crater_destroy_tiberium);
        let marks = crate::sim::combat::smudge_dispatch::AnimMiddleMarks {
            scorch: config.scorch,
            crater: config.crater,
            force_big_craters: config.force_big_craters,
            width,
            height,
            destroy_tiberium,
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

    /// Ares 3.0 AnimClass_Expired_SpawnsParticle (1000AE40, hook 0x425002,
    /// resuming at 0x42504D): each of the NumParticles particles lands at
    /// `|RandomRanged(min, max)|` leptons from the anim's GetCoords, at an
    /// angle drawn within its share of the circle (`RandomRanged(1,
    /// 0x7FFFFFFF)` times 1/0x7FFFFFFF), keeping the anim's height above the
    /// floor (`0x00578080`). The arithmetic is the DLL's x87 code; its CRT
    /// cos/sin are libm's.
    fn ares_spawn_particles_around(
        &mut self,
        id: AnimId,
        kind: crate::rules::particle_type::ParticleTypeId,
        config: &AnimTypeRuntimeConfig,
        rules: &RuleSet,
    ) {
        use crate::util::native_x87::NativeF64Bits as Bits;
        let Some(coord) = self.anim_absolute_coord(id) else {
            return;
        };
        let floor = |sim: &Self, x: i32, y: i32, z: i32| {
            crate::sim::projectile::projectile_ground_z(
                sim.resolved_terrain.as_ref(),
                &sim.effective_shared_cell_dummy(),
                crate::sim::projectile::ProjectileCoord::new(x, y, z),
            )
        };
        let height = coord.z.wrapping_sub(floor(self, coord.x, coord.y, coord.z));
        let load = |value: f64| {
            X87Chop53::load_f64(Bits::from_bits(value.to_bits())).expect("finite constant")
        };
        let store = |value| f64::from_bits(X87Chop53::store_f64(value).map_or(0, Bits::bits));
        let count = config.num_particles;
        let step =
            X87Chop53::div(load(6.283185307179586), X87Chop53::load_i32(count)).map_or(0.0, store);
        let [minimum, maximum] = config.ares_spawns_particle_range;
        let mut angle = 0.0;
        for _ in 0..count {
            let distance = self
                .scenario_rng
                .next_range_i32_inclusive(minimum, maximum)
                .wrapping_abs();
            let value = self.scenario_rng.next_range_i32_inclusive(1, 0x7FFF_FFFF);
            let radians = store(X87Chop53::add(
                X87Chop53::mul(
                    X87Chop53::mul(X87Chop53::load_i32(value), load(4.656612873077393e-10)),
                    load(step),
                ),
                load(angle),
            ));
            let (cos, sin) = (libm::cos(radians), libm::sin(radians));
            let along = |trig: f64| {
                ares_truncate_x87(X87Chop53::mul(X87Chop53::load_i32(distance), load(trig)))
            };
            let x = along(cos).wrapping_add(coord.x);
            let y = coord.y.wrapping_sub(along(sin));
            let z = floor(self, x, y, coord.z).wrapping_add(height);
            self.spawn_gas_particle(kind, glam::IVec3::new(x, y, z), rules);
            angle = store(X87Chop53::add(load(angle), load(step)));
        }
    }

    fn switch_anim_type(
        &mut self,
        id: AnimId,
        next: &str,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<(), AnimSpawnError> {
        let Some(config) = rules.art().anim_runtime_config(next) else {
            self.destroy_anim(id, rules);
            return Ok(());
        };
        let Ok((effective_end, effective_loop_end)) = effective_bounds(next, config) else {
            self.destroy_anim(id, rules);
            return Ok(());
        };
        let type_id = self.interner.intern(next);
        let rate_reload = Self::choose_anim_rate(&self.session, &mut self.scenario_rng, config);
        let frame_timer =
            CdTimer::started(self.session.binary_frame() as i32, i32::from(rate_reload));
        let stop_sound_id = config
            .stop_sound
            .as_deref()
            .map(|sound| self.interner.intern(sound));
        // A remapped anim's new type is a new atlas key.
        self.substrate.anims.note_look(id);
        if let Some(anim) = self.substrate.anims.get_mut(id) {
            anim.type_id = type_id;
            anim.damage_accumulator = NativeF64Bits::POSITIVE_ZERO;
            anim.effective_end = effective_end;
            anim.effective_loop_end = effective_loop_end;
            anim.stop_sound_id = stop_sound_id;
            // 0x00424913..0x0042491F: Value (+0xAC) takes Next's Start as
            // written, though it is otherwise relative to Start, so the first
            // drawn frame is twice Start; the step (+0xC4) keeps the old
            // type's direction. Retail Next targets all have Start=0 and no
            // Reverse=, where this equals a fresh start.
            anim.runtime.current_frame = config.start;
            anim.runtime.delay_remaining = 0;
            anim.runtime.rate_reload = rate_reload;
            anim.runtime.frame_timer = frame_timer;
            anim.runtime.loop_remaining = native_loop_remaining(config.loop_count, 1);
            anim.runtime.first_ai_guard = false;
            anim.runtime.inactive = false;
            {
                // The one-shot CreateUnit latch belongs to the current type:
                // a `Next=` chain of two CreateUnit= types spawns each.
                anim.create_unit_fired = false;
            }
        }
        self.anim_start(id, config, rules, overlay_registry, frame_effects)
    }
}

impl crate::sim::world::ObjectSubstrate {
    /// Mutation of the retained Anim+198 byte belongs to its instance owner.
    pub(crate) fn set_anim_no_effects(&mut self, id: AnimId, no_effects: bool) {
        if let Some(anim) = self.anims.get_mut(id) {
            anim.no_effects = no_effects;
        }
    }
    pub(crate) fn clear_damage_fire_anim_reference(&mut self, id: AnimId) {
        let link = self
            .anims
            .get_mut(id)
            .and_then(|anim| anim.damage_fire_slot.take());
        if let Some((owner, slot)) = link
            && let Some(entity) = self.entities.get_mut(owner)
            && entity.damage_fires.anim_ids[usize::from(slot)] == Some(id)
        {
            entity.damage_fires.anim_ids[usize::from(slot)] = None;
        }
    }
    pub(crate) fn mark_anim_display(&mut self, id: AnimId, marked: bool) {
        if let Some(anim) = self.anims.get_mut(id) {
            anim.display.marked_on_map = marked;
        }
    }
    /// Retail gamemd.exe SetOwnerObject424B79..424BB6 and active-scenario
    /// destructor42291A..42295B clear Object+84 only for the last attached
    /// Anim. Scan the whole retained array, excluding only this Anim: inactive,
    /// concealed and pending-delete entries still count while CC names owner.
    /// Owner+17C is bare RET5F43C0 for supported Techno owners. Rust world
    /// disposal drops the stores; it does not simulate native gate0 exit teardown.
    pub(crate) fn release_last_anim_owner_flag(&mut self, id: AnimId) {
        // A presentation anim never raised the flag, and the scan below does
        // not count it.
        let Some(owner) = self
            .anims
            .get(id)
            .filter(|anim| !anim.is_presentation())
            .and_then(|anim| anim.owner_entity)
        else {
            return;
        };
        if !self.anims.another_owned_by(id, owner)
            && let Some(entity) = self.entities.get_mut(owner)
        {
            entity.set_attached_anim_flag(false);
        }
    }

    /// Clear only CC, without SetOwner's coordinate conversion or display
    /// submission (422961 / 425190). Callers own the distinct +60 and +84
    /// effects: PointerExpired clears CC without releasing the retained flag.
    pub(crate) fn release_anim_owner_reference(&mut self, id: AnimId) -> Option<u64> {
        // Runs in every game: only the Ares terrain fire gives an anim a
        // terrain owner, and without one this falls through to the stock clear.
        if let Some(Some(terrain)) = self.anims.set_owner_terrain(id, None) {
            return Some(terrain);
        }
        self.anims.set_owner_entity(id, None)?
    }
    pub(crate) fn set_anim_frame_and_z_adjust(&mut self, id: AnimId, frame: i32, z_adjust: i32) {
        if let Some(anim) = self.anims.get_mut(id) {
            anim.runtime.current_frame = frame;
            anim.z_adjust = z_adjust;
        }
    }
    /// Apply CellClass's producer-owned `AnimClass +0x100` write after the
    /// delay-zero constructor has already run `Middle`.
    pub(crate) fn set_terrain_anim_z_adjust_after_construction(
        &mut self,
        id: AnimId,
        z_adjust: i32,
    ) -> bool {
        let Some(anim) = self.anims.get_mut(id) else {
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
        let Some(anim) = self.anims.get_mut(id) else {
            return false;
        };
        anim.z_adjust = z_adjust;
        true
    }
    /// A producer's plain Anim+19D store. Mirage's attached-ring tail
    /// `746A9C..746B19` changes drawing without suspending the Anim AI.
    pub(crate) fn set_anim_hidden(&mut self, id: AnimId, hidden: bool) -> bool {
        let Some(anim) = self.anims.get_mut(id) else {
            return false;
        };
        anim.draw_runtime.hidden = hidden;
        anim.viewer_hidden = None;
        true
    }

    /// Anim+19D per viewer: `viewers` holds named houses' bytes and
    /// `other_hidden` every other viewer's. Ares common Launch100330DC writes
    /// it once, from the alliance relation at construction rather than a live
    /// visibility query; the disguised ring tail writes it every frame.
    pub(crate) fn set_anim_viewer_hidden(
        &mut self,
        id: AnimId,
        viewers: Vec<(InternedId, bool)>,
        other_hidden: bool,
    ) {
        if let Some(anim) = self.anims.get_mut(id) {
            anim.viewer_hidden = Some(AnimViewerHidden {
                viewers,
                other_hidden,
            });
        }
    }
    /// `AnimClass::SetOwnerHouse @ 0x00424CA0`: store `+0x180`.
    pub(crate) fn set_anim_owner_house(&mut self, id: AnimId, house: InternedId) -> bool {
        let Some(anim) = self.anims.get_mut(id) else {
            return false;
        };
        anim.owner_house = Some(house);
        true
    }

    /// Phobos `AnimTypeExt::ProcessDestroyAnims`' death-unit facts
    /// (`Ext/AnimType/Body.cpp:55-70`): `FromDeathUnit` and the inherited
    /// facing/turret facts a `CreateUnit` anim consumes.
    pub(crate) fn set_anim_death_facts(
        &mut self,
        id: AnimId,
        from_death_unit: bool,
        facing: Option<u8>,
        has_turret: bool,
        turret_facing: Option<u8>,
    ) -> bool {
        let Some(anim) = self.anims.get_mut(id) else {
            return false;
        };
        anim.from_death_unit = from_death_unit;
        anim.death_unit_facing = facing;
        anim.death_unit_has_turret = has_turret;
        anim.death_unit_turret_facing = turret_facing;
        true
    }

    /// Phobos `AnimExt::SetInvoker` (`Ext/Anim/Body.cpp:28-45`): store the
    /// invoker Techno and its House. RESIDUAL: the native per-Techno
    /// `AnimRefCount` and `InvalidateTechnoPointers` (clearing a destroyed
    /// invoker) are not modeled; a stable-id lookup answers absent instead.
    pub(crate) fn set_anim_invoker(
        &mut self,
        id: AnimId,
        invoker: Option<u64>,
        house: Option<InternedId>,
    ) -> bool {
        let Some(anim) = self.anims.get_mut(id) else {
            return false;
        };
        anim.invoker = invoker;
        anim.invoker_house = house;
        true
    }
    /// `0x00424C90`: store `+0x17C`, the bullet whose expiry ends the anim.
    pub(crate) fn set_anim_attached_bullet(&mut self, id: AnimId, bullet: Option<u64>) -> bool {
        self.anims.set_attached_bullet(id, bullet)
    }
}

/// Phobos `AnimTypeExt::DetachOnCloak` (`Ext/AnimType/Body.cpp:112`,
/// `Valueable<bool>` constructor `true`): one query for every consumer. A type
/// with no art entry keeps the constructor default.
pub(crate) fn anim_type_detach_on_cloak(rules: &RuleSet, type_name: &str) -> bool {
    rules
        .art()
        .anim_runtime_config(type_name)
        .is_none_or(|config| config.phobos_detach_on_cloak)
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
    /// A local anim ([`AnimStore`]'s `local`): the LAN and online MoveFlash
    /// of [`Simulation::spawn_move_flash`].
    Local,
    /// A presentation anim: a local anim of a type that changes nothing but
    /// its own picture ([`crate::rules::art_data::ArtRegistry::anim_presentation_only`]),
    /// which every game makes alike. It takes no Display slot, no Logic slot,
    /// no unique id and no Scenario draw, and nothing in the simulation holds
    /// it; the picture draws it where Display would hold it
    /// (`Simulation::picture_draw_order`).
    ///
    /// VERA-only sim-scale deviation, on in real matches
    /// ([`Simulation::present_effect_anims`]): gamemd keeps every muzzle flash and
    /// impact anim in Display (Ground's sorted vector) and Logic and gives each
    /// a unique id. Leaving them out changes the unique ids and the Ground order
    /// later objects take, and the state hash and sync checksum with them; a
    /// player sees the same picture. RESIDUAL: such an anim does not raise its
    /// owner's attached-Anim flag (Object+84, `SetOwnerObject @ 0x00424B50`),
    /// whose only reader caps the fall rate (`ObjectFall`); trigger: a Techno
    /// that starts falling within a few frames of firing; effect: it falls at
    /// the ordinary rate; frequency: rare (a bridge collapsing under a firing
    /// unit). It ends with its owner's UnInit, where gamemd detaches it in
    /// place; an owner that enters Limbo (a transport, a garrison) keeps it
    /// drawing until its own end, a few frames.
    Presentation,
}

/// Take the constructor's draws for an anim of `config` at `coord`.
///
/// IsMeteor4222FF takes the shared meteor launch prefix (three raw draws),
/// then Bounce4397E0's three axis draws, with no angular velocity. RandomRate
/// still precedes those calls; equal rate bounds make a request without a draw.
pub(crate) fn anim_constructor_draws(
    config: &AnimTypeRuntimeConfig,
    coord: AnimWorldCoord,
    rng: &mut crate::sim::rng::SimRng,
) -> Result<AnimConstructorDraws, NativeX87Error> {
    let random_rate = anim_random_rate(config, rng);
    let bounce = if config.is_meteor {
        let launch = crate::sim::bounce::meteor_launch(
            glam::IVec3::new(coord.x, coord.y, coord.z),
            // 4223D8: a literal lifetime that only places the start.
            70,
            config.min_z_vel,
            config.max_xy_vel,
            rng,
        )?;
        Some(BounceState::init(
            launch.start,
            config.elasticity,
            BOUNCER_GRAVITY,
            NativeF64Bits::POSITIVE_ZERO,
            launch.velocity,
            NativeF64Bits::POSITIVE_ZERO,
            rng,
        )?)
    } else if config.bouncer {
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
    use crate::sim::rng::abs_before_modulo;

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
        X87Chop53::load_i32(abs_before_modulo(z_draw, z_divisor)),
        min_z,
    ))?;
    let velocity_y = X87Chop53::store_f32(X87Chop53::sub(
        X87Chop53::load_i32(abs_before_modulo(y_draw, xy_divisor)),
        max_xy,
    ))?;
    let velocity_x = X87Chop53::store_f32(X87Chop53::sub(
        X87Chop53::load_i32(abs_before_modulo(x_draw, xy_divisor)),
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
    // AnimTypeLoad428855 calls generic LoadSHP, not bounds resolver427B50;
    // an unread type's newly resident header must not replace ctor Middle0.
    if !config.art_body_read {
        return None;
    }
    config
        .middle_frame
        .or_else(|| config.raw_shp_frame_count.map(|raw| raw / 2))
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

/// Ares.dll's float-to-int conversion (100772B0): toward zero, and the
/// integer indefinite (`i32::MIN`) for NaN or a value out of range.
fn ares_truncate(value: f64) -> i32 {
    X87Chop53::load_f64(NativeF64Bits::from_bits(value.to_bits()))
        .map_or(i32::MIN, ares_truncate_x87)
}

/// C++ `static_cast<int>(std::round(value))` in Phobos's replaced anim
/// damage body (`Ext/Anim/Hooks.cpp:60,75,91`): round half away from zero,
/// then the x87 masked conversion (the integer indefinite out of range).
fn phobos_anim_round(value: f64) -> i32 {
    let rounded = if value.is_nan() {
        value
    } else {
        (value.abs() + 0.5).floor().copysign(value)
    };
    X87Chop53::ftol_f64_low_masked(rounded)
}

fn ares_truncate_x87(value: crate::util::native_x87::X87Value) -> i32 {
    X87Chop53::ftol_i64(value)
        .ok()
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or(i32::MIN)
}

/// `0x004242CA..0x004242DF`: a separation of 1, else the signed frame count
/// modulo the separation (IDIV), so a negative separation also spawns.
/// Ares hooks 0x004242CA (`AnimClass_Update_FixIE_TrailerSeperation`,
/// Ares.dll `1000B210`): a separation below 1 jumps to 0x00424322 and spawns
/// none; 1 and above take the IDIV.
/// RESIDUAL: gamemd faults dividing by a zero `TrailerSeperation=`; VERA
/// spawns no trailer there.
fn trailer_cadence_matches(binary_frame: u64, separation: i32, ares: bool) -> bool {
    if ares && separation < 1 {
        return false;
    }
    separation == 1 || (separation != 0 && (binary_frame as i32).wrapping_rem(separation) == 0)
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

#[cfg(all(test, tests_sim_a_b))]
mod long_tail_contract_tests {
    use super::*;

    /// The answer a single-entry `Translucency` fallback track gives for an
    /// authored integer (`Utilities/Constructs.h:481-501`): only the three
    /// translucent stages survive, everything else is `BlitterFlags::None`.
    fn plain_level(value: i32) -> Option<u8> {
        match value {
            25 => Some(25),
            50 => Some(50),
            75 => Some(75),
            _ => None,
        }
    }

    /// `0x0042319A..0x004231BC`: only an exact `Translucency=` 25, 50 or 75
    /// selects a stage; any other value draws without one.
    /// `0x0042306B`: the `+0x119` arm ORs 75% for a `DoubleThick=` type,
    /// else 50%, and falls through to the type's own arms (`0x0042307D`).
    #[test]
    fn forced_translucency_reads_double_thick_and_falls_through() {
        let input = |double_thick, explicit_translucency, instance_ramp| AnimTranslucencyInput {
            base_flags: 0,
            forced_translucent: true,
            double_thick,
            translucency_detail_level: 0,
            game_detail_level: 2,
            translucent_ramp: false,
            current_frame: 0,
            end_frame: 10,
            explicit_translucency,
            translucency_level: plain_level(explicit_translucency),
            translucency_cloaked_level: None,
            cloaked_translucency_answers: false,
            instance_ramp,
        };
        for (double_thick, explicit, ramp, expected) in [
            (false, 0, 0, (true, 4)),
            (true, 0, 0, (true, 6)),
            (false, 25, 0, (true, 6)),
            (false, 0, 16, (false, 4)),
        ] {
            let result = anim_translucency_selection(input(double_thick, explicit, ramp));
            assert_eq!((result.draw, result.flags), expected);
        }
    }

    /// `0x004231C1..0x004231EE`: the `+0x178` bands a fading building
    /// writes into its slot anims.
    #[test]
    fn instance_ramp_bands_draw_at_25_then_50_then_not_at_all() {
        for (instance_ramp, expected) in [
            (0, (true, 0)),
            (1, (true, 2)),
            (5, (true, 2)),
            (6, (true, 4)),
            (15, (true, 4)),
            (16, (false, 0)),
        ] {
            let result = anim_translucency_selection(AnimTranslucencyInput {
                base_flags: 0,
                forced_translucent: false,
                double_thick: false,
                translucency_detail_level: 0,
                game_detail_level: 2,
                translucent_ramp: false,
                current_frame: 0,
                end_frame: 10,
                explicit_translucency: 0,
                translucency_level: None,
                translucency_cloaked_level: None,
                cloaked_translucency_answers: false,
                instance_ramp,
            });
            assert_eq!(
                (result.draw, result.flags),
                expected,
                "ramp {instance_ramp}"
            );
        }
    }

    /// `0x004230A8..0x004230FB`: End 15 (EXPLOMED, WAKE1, RING) reaches 75%
    /// at stage 9, because 15 times the 0.6 double chops below 9, while the
    /// 0.4 and 0.2 doubles sit above their fifths.
    #[test]
    fn translucent_ramp_takes_75_percent_at_three_fifths() {
        for (current_frame, flags) in [(3, 0), (4, 2), (6, 2), (7, 4), (9, 6)] {
            let result = anim_translucency_selection(AnimTranslucencyInput {
                base_flags: 0,
                forced_translucent: false,
                double_thick: false,
                translucency_detail_level: 0,
                game_detail_level: 2,
                translucent_ramp: true,
                current_frame,
                end_frame: 15,
                explicit_translucency: 0,
                translucency_level: None,
                translucency_cloaked_level: None,
                cloaked_translucency_answers: false,
                instance_ramp: 0,
            });
            assert_eq!(
                (result.draw, result.flags),
                (true, flags),
                "stage {current_frame}"
            );
        }
    }

    #[test]
    fn only_exact_fixed_translucency_values_select_a_stage() {
        for (explicit_translucency, flags) in [
            (25, 2),
            (50, 4),
            (75, 6),
            (1, 0),
            (26, 0),
            (100, 0),
            (0, 0),
            (-25, 0),
        ] {
            assert_eq!(
                anim_translucency_selection(AnimTranslucencyInput {
                    base_flags: 0,
                    forced_translucent: false,
                    double_thick: false,
                    translucency_detail_level: 0,
                    game_detail_level: 2,
                    translucent_ramp: false,
                    current_frame: 0,
                    end_frame: 10,
                    explicit_translucency,
                    translucency_level: plain_level(explicit_translucency),
                    translucency_cloaked_level: None,
                    cloaked_translucency_answers: false,
                    instance_ramp: 0,
                }),
                AnimTranslucencyResult { draw: true, flags },
                "Translucency={explicit_translucency}"
            );
        }
    }
}

#[cfg(all(test, tests_sim_a_b))]
#[path = "anim_debris_tests.rs"]
mod debris_tests;

#[cfg(all(test, any(tests_sim_a_b, tests_sim_c_g)))]
#[path = "anim_trigger_tests.rs"]
pub(in crate::sim) mod trigger_tests;

#[cfg(all(test, tests_sim_a_b))]
#[path = "anim_mineral_start_tests.rs"]
mod mineral_start_tests;

#[cfg(all(test, tests_sim_a_b))]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;

    /// Phobos `Translucency.Cloaked=` (`Ext/AnimType/Body.cpp:114`,
    /// `AnimClass_DrawIt_Translucency` hook `0x423183`,
    /// `Ext/Anim/Hooks.cpp:573-615`): while the anim's Techno owner is
    /// Cloaking or Cloaked the alternate level replaces `Translucency`; an
    /// animation without the key keeps the plain level.
    #[test]
    fn translucency_cloaked_overrides_on_a_cloaked_owner_only() {
        use crate::sim::world::Simulation;

        const ANIM: &str = "CLOAKTINT";
        const PLAIN: &str = "PLAINTINT";
        let ini = crate::sim::arena_fixture::clear_ground_ini(
            "[General]\nCloakingStages=6\n\
             [VehicleTypes]\n0=VICTIM\n\
             [VICTIM]\nStrength=1000\nArmor=heavy\nSpeed=4\nCloakable=yes\nCloakingSpeed=3\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("arena rules");
        let mut art = ArtRegistry::from_ini(&IniFile::from_str(&format!(
            "[{ANIM}]\nTranslucency=50\nTranslucency.Cloaked=25\n\
             [{PLAIN}]\nTranslucency=50\n"
        )));
        art.bind_anim_frame_count_for_test(ANIM, 8);
        art.bind_anim_frame_count_for_test(PLAIN, 8);
        rules.replace_art_registry_for_test(art);
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        let owner = sim
            .spawn_object("VICTIM", "Russians", 10, 10, 0, &rules)
            .expect("owner");
        let attach = |sim: &mut Simulation, name: &str| {
            let ty = sim.intern(name);
            let anim = sim
                .spawn_anim_at_world(
                    &rules,
                    AnimClassSpawnDescriptor::new(
                        ty,
                        16,
                        16,
                        crate::util::fixed_math::SIM_ZERO,
                        crate::util::fixed_math::SIM_ZERO,
                        0,
                    ),
                    AnimWorldCoord {
                        x: 10 * 256,
                        y: 10 * 256,
                        z: 0,
                    },
                )
                .expect("the art section spawns the anim");
            assert!(sim.set_anim_owner_object(anim, Some(owner), &rules));
            anim
        };
        let tinted = attach(&mut sim, ANIM);
        let plain = attach(&mut sim, PLAIN);
        assert_eq!(
            rules
                .art()
                .anim_runtime_config(ANIM)
                .unwrap()
                .phobos_translucency_cloaked
                .keyframes(),
            &[
                crate::rules::phobos_translucency_track::TranslucencyKeyframe {
                    percentage: 0.0,
                    level: 25,
                }
            ]
        );
        assert!(
            !rules
                .art()
                .anim_runtime_config(PLAIN)
                .unwrap()
                .phobos_translucency_cloaked
                .has_values(),
            "an absent Translucency.Cloaked leaves the cloaked track empty"
        );
        let select = |sim: &Simulation, anim: u64| {
            let config = rules
                .art()
                .anim_runtime_config(sim.interner().resolve(sim.anim(anim).unwrap().type_id()));
            crate::sim::anim_class::anim_translucency_selection(
                crate::sim::anim_class::AnimTranslucencyInput::new(
                    config,
                    0,
                    0,
                    8,
                    2,
                    crate::sim::anim_class::AnimDrawRuntime::default(),
                    sim.anim_owner_is_cloaked(anim),
                ),
            )
            .flags
        };
        assert_eq!(select(&sim, tinted), 4, "50% from Translucency");
        assert_eq!(select(&sim, plain), 4);
        let cloak = |sim: &mut Simulation, state: i32| {
            sim.substrate
                .entities
                .get_mut(owner)
                .unwrap()
                .cloak
                .as_mut()
                .expect("Cloakable=yes carries the cloak runtime")
                .set_state_for_test(state);
        };
        cloak(&mut sim, 1);
        assert_eq!(select(&sim, tinted), 2, "Cloaking uses the cloaked 25%");
        assert_eq!(select(&sim, plain), 4, "no key keeps Translucency");
        cloak(&mut sim, 2);
        assert_eq!(select(&sim, tinted), 2, "Cloaked uses the cloaked 25%");
        cloak(&mut sim, 0);
        assert_eq!(select(&sim, tinted), 4, "uncloaked returns to Translucency");
    }

    /// Phobos keyframeable `Translucency=` (`Ext/AnimType/Body.cpp:113`,
    /// `Animatable<TranslucencyLevel>`; `AnimClass_DrawIt_Translucency`
    /// `0x423183`, `Ext/Anim/Hooks.cpp:583-605`): the track answers at
    /// `currentFrame / End`, interpolation truncates, and a `None` answer
    /// keeps the stock instance ramp. A cloaked owner answers from
    /// `Translucency.Cloaked` and never from the plain track.
    #[test]
    fn translucency_keyframes_answer_per_frame_and_keep_the_stock_fallback() {
        const TRACK: &str = "TINTTRACK";
        let mut art = ArtRegistry::from_ini(&IniFile::from_str(&format!(
            "[{TRACK}]\nEnd=8\n\
             Translucency.Keyframe0.Percentage=0\n\
             Translucency.Keyframe0.Value=0\n\
             Translucency.Keyframe1.Percentage=1\n\
             Translucency.Keyframe1.Value=50\n\
             Translucency.Interpolation=linear\n\
             Translucency.Cloaked=75\n"
        )));
        art.bind_anim_frame_count_for_test(TRACK, 8);
        let config = art.anim_runtime_config(TRACK).unwrap();
        let select = |frame: i32, owner_cloaked: bool| {
            anim_translucency_selection(AnimTranslucencyInput::new(
                Some(config),
                0,
                frame,
                8,
                2,
                AnimDrawRuntime::default(),
                owner_cloaked,
            ))
        };
        for (frame, owner_cloaked, flags) in [
            (0, false, 0),
            (2, false, 0),
            (4, false, 2),
            (8, false, 4),
            (4, true, 6),
        ] {
            let result = select(frame, owner_cloaked);
            assert!(
                result.draw,
                "frame {frame} cloak {owner_cloaked} keeps drawing"
            );
            assert_eq!(result.flags, flags, "frame {frame} cloak {owner_cloaked}");
        }
    }

    /// Phobos `Layer.UseObjectLayer=` (`Ext/AnimType/Body.cpp:92`,
    /// `AnimClass_InWhichLayer` hook `0x424CB0`,
    /// `Ext/Anim/Hooks.cpp:354-375`): an attached anim takes the owner
    /// object's `InWhichLayer` when true, its own type layer when false, and
    /// the stock Ground when the key is unset.
    #[test]
    fn layer_use_object_layer_selects_between_the_owner_and_the_type() {
        use crate::sim::world::Simulation;
        use crate::sim::world::display_layers::DisplayLayer;

        let ini = crate::sim::arena_fixture::clear_ground_ini(
            "[VehicleTypes]\n0=OWNER\n[OWNER]\nStrength=1000\nArmor=heavy\nSpeed=4\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("arena rules");
        let mut art = ArtRegistry::from_ini(&IniFile::from_str(
            "[OWNERLAYER]\nLayer=Top\nLayer.UseObjectLayer=yes\n\
             [TYPELAYER]\nLayer=Top\nLayer.UseObjectLayer=no\n\
             [STOCK]\nLayer=Top\n",
        ));
        for name in ["OWNERLAYER", "TYPELAYER", "STOCK"] {
            art.bind_anim_frame_count_for_test(name, 8);
        }
        rules.replace_art_registry_for_test(art);
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        let owner = sim
            .spawn_object("OWNER", "Russians", 10, 10, 0, &rules)
            .expect("owner");
        assert_eq!(
            sim.entity_display_layer(owner, &rules),
            Some(DisplayLayer::GROUND)
        );
        let attach = |sim: &mut Simulation, name: &str| {
            let ty = sim.intern(name);
            let anim = sim
                .spawn_anim_at_world(
                    &rules,
                    AnimClassSpawnDescriptor::new(
                        ty,
                        16,
                        16,
                        crate::util::fixed_math::SIM_ZERO,
                        crate::util::fixed_math::SIM_ZERO,
                        0,
                    ),
                    AnimWorldCoord {
                        x: 10 * 256,
                        y: 10 * 256,
                        z: 0,
                    },
                )
                .expect("the art section spawns the anim");
            assert!(sim.set_anim_owner_object(anim, Some(owner), &rules));
            anim
        };
        let anim = attach(&mut sim, "OWNERLAYER");
        assert_eq!(
            sim.anim_display_layer(anim, &rules),
            Some(DisplayLayer::GROUND),
            "true takes the owner object's layer"
        );
        let anim = attach(&mut sim, "TYPELAYER");
        assert_eq!(
            sim.anim_display_layer(anim, &rules),
            Some(DisplayLayer::TOP),
            "false takes the type's layer"
        );
        let anim = attach(&mut sim, "STOCK");
        assert_eq!(
            sim.anim_display_layer(anim, &rules),
            Some(DisplayLayer::GROUND),
            "unset keeps the stock attached Ground"
        );
    }

    /// Phobos `RestrictVisibilityIfCloaked=` (`Ext/AnimType/Body.cpp:111`) at
    /// `AnimClass_DrawIt_Visibility @ 0x423061` (`Ext/Anim/Hooks.cpp:524-570`):
    /// the cloak state of the anim's techno owner, its alliance with the viewer
    /// and the viewer's sensors on the owner's cell decide the skip.
    #[test]
    fn a_cloaked_owners_anim_hides_only_from_unsensing_unallied_viewers() {
        use crate::sim::world::Simulation;

        const ANIM: &str = "CLOAKSPARK";
        const PLAIN: &str = "PLAINSPARK";
        let ini = crate::sim::arena_fixture::clear_ground_ini(
            "[General]\nCloakingStages=6\n\
             [VehicleTypes]\n0=VICTIM\n\
             [VICTIM]\nStrength=1000\nArmor=heavy\nSpeed=4\nCloakable=yes\nCloakingSpeed=3\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("arena rules");
        let mut art = ArtRegistry::from_ini(&IniFile::from_str(&format!(
            "[{ANIM}]\nRestrictVisibilityIfCloaked=yes\nLayer=Ground\n\
             [{PLAIN}]\nLayer=Ground\n"
        )));
        art.bind_anim_frame_count_for_test(ANIM, 8);
        art.bind_anim_frame_count_for_test(PLAIN, 8);
        rules.replace_art_registry_for_test(art);
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.session.set_map_width_for_test(32);
        sim.session.set_map_height_for_test(32);
        sim.fog.set_width_for_test(32);
        sim.fog.set_height_for_test(32);
        sim.intern_rule_type_ids(&rules);
        let target = sim
            .spawn_object("VICTIM", "Russians", 10, 10, 0, &rules)
            .expect("the arena places the cloakable victim");
        let americans = sim.intern("Americans");
        let russians = sim.intern("Russians");
        let attach = |sim: &mut Simulation, name: &str, owner: Option<u64>| {
            let ty = sim.intern(name);
            let anim = sim
                .spawn_anim_at_world(
                    &rules,
                    AnimClassSpawnDescriptor::new(
                        ty,
                        16,
                        16,
                        crate::util::fixed_math::SIM_ZERO,
                        crate::util::fixed_math::SIM_ZERO,
                        0,
                    ),
                    AnimWorldCoord {
                        x: 10 * 256,
                        y: 10 * 256,
                        z: 0,
                    },
                )
                .expect("the art section spawns the anim");
            sim.substrate.anims.set_owner_entity_for_test(anim, owner);
            anim
        };
        let cloak = |sim: &mut Simulation, state: i32| {
            sim.substrate
                .entities
                .get_mut(target)
                .unwrap()
                .cloak
                .as_mut()
                .expect("Cloakable=yes carries the cloak runtime")
                .set_state_for_test(state);
        };

        assert!(rules.art().anim_runtime_config(ANIM).is_some());
        let anim = attach(&mut sim, ANIM, Some(target));
        // An uncloaked owner draws, whatever the viewer.
        assert!(!sim.anim_visibility_skipped(anim, &rules, Some(americans)));

        cloak(&mut sim, 2);
        assert!(
            sim.anim_visibility_skipped(anim, &rules, Some(americans)),
            "an enemy viewer without a sensor loses the cloaked owner's anim"
        );
        cloak(&mut sim, 1);
        assert!(
            sim.anim_visibility_skipped(anim, &rules, Some(americans)),
            "Cloaking (1) hides like Cloaked (2)"
        );
        // The owner's own house is allied with itself.
        assert!(!sim.anim_visibility_skipped(anim, &rules, Some(russians)));

        // `Sensors_InclHouse` on the owner's cell reveals it to the enemy.
        let cell = {
            let entity = sim.entities().get(target).unwrap();
            (entity.position.rx(), entity.position.ry())
        };
        sim.fog_mut_for_test()
            .increment_sensor_at(americans, cell.0, cell.1);
        assert!(!sim.anim_visibility_skipped(anim, &rules, Some(americans)));

        // The key off, and an anim without a techno owner, both draw.
        let plain = attach(&mut sim, PLAIN, Some(target));
        assert!(!sim.anim_visibility_skipped(plain, &rules, Some(americans)));
        let free = attach(&mut sim, ANIM, None);
        assert!(!sim.anim_visibility_skipped(free, &rules, Some(americans)));
    }

    /// Phobos `VisibleTo` (`Ext/AnimType/Body.cpp:109`,
    /// `Valueable<AffectedHouse>` default All) and
    /// `VisibleTo.ConsiderInvokerAsOwner` at `AnimClass_DrawIt_Visibility`
    /// (`0x423061`, `Ext/Anim/Hooks.cpp:530-568`): the anim's owning house
    /// gates the draw, and the invoker's house replaces it when keyed.
    #[test]
    fn visible_to_filters_anim_draws_by_the_owner_or_invoker_house() {
        use crate::rules::phobos_shields::AffectedHouse;
        use crate::sim::world::Simulation;

        let ini = crate::sim::arena_fixture::clear_ground_ini(
            "[VehicleTypes]\n0=VICTIM\n1=INVOKER\n\
             [VICTIM]\nStrength=1000\nArmor=heavy\nSpeed=4\n\
             [INVOKER]\nStrength=1000\nArmor=heavy\nSpeed=4\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("arena rules");
        let mut art = ArtRegistry::from_ini(&IniFile::from_str(
            "[VISOWNER]\nVisibleTo=owner\n\
             [VISENEMY]\nVisibleTo=enemies\n\
             [VISINVOKER]\nVisibleTo=owner\nVisibleTo.ConsiderInvokerAsOwner=yes\n",
        ));
        for name in ["VISOWNER", "VISENEMY", "VISINVOKER"] {
            art.bind_anim_frame_count_for_test(name, 8);
        }
        rules.replace_art_registry_for_test(art);
        assert_eq!(
            rules
                .art()
                .anim_runtime_config("VISOWNER")
                .unwrap()
                .phobos_visible_to,
            Some(AffectedHouse::OWNER)
        );
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        let owner = sim
            .spawn_object("VICTIM", "Russians", 10, 10, 0, &rules)
            .expect("owner");
        let invoker = sim
            .spawn_object("INVOKER", "Americans", 12, 10, 0, &rules)
            .expect("invoker");
        let americans = sim.intern("Americans");
        let russians = sim.intern("Russians");
        let attach = |sim: &mut Simulation, name: &str| {
            let ty = sim.intern(name);
            let anim = sim
                .spawn_anim_at_world(
                    &rules,
                    AnimClassSpawnDescriptor::new(
                        ty,
                        16,
                        16,
                        crate::util::fixed_math::SIM_ZERO,
                        crate::util::fixed_math::SIM_ZERO,
                        0,
                    ),
                    AnimWorldCoord {
                        x: 10 * 256,
                        y: 10 * 256,
                        z: 0,
                    },
                )
                .expect("the art section spawns the anim");
            assert!(sim.set_anim_owner_object(anim, Some(owner), &rules));
            anim
        };
        let owner_only = attach(&mut sim, "VISOWNER");
        assert!(sim.anim_visibility_skipped(owner_only, &rules, Some(americans)));
        assert!(!sim.anim_visibility_skipped(owner_only, &rules, Some(russians)));
        let enemies_only = attach(&mut sim, "VISENEMY");
        assert!(sim.anim_visibility_skipped(enemies_only, &rules, Some(russians)));
        assert!(!sim.anim_visibility_skipped(enemies_only, &rules, Some(americans)));
        // The invoker's American house replaces the Russian owner: the
        // American viewer becomes the "owner", the Russian one an enemy.
        let invoker_owned = attach(&mut sim, "VISINVOKER");
        assert!(
            sim.substrate
                .set_anim_invoker(invoker_owned, Some(invoker), Some(americans))
        );
        assert!(!sim.anim_visibility_skipped(invoker_owned, &rules, Some(americans)));
        assert!(sim.anim_visibility_skipped(invoker_owned, &rules, Some(russians)));
        // Without the key the owner object's house wins even with an invoker.
        assert!(
            sim.substrate
                .set_anim_invoker(owner_only, Some(invoker), Some(americans))
        );
        assert!(!sim.anim_visibility_skipped(owner_only, &rules, Some(russians)));
        assert!(sim.anim_visibility_skipped(owner_only, &rules, Some(americans)));
    }

    /// Phobos' `AnimClass_AI_Early @ 0x423B95` replacement of the stock
    /// `HideIfNoOre` test: the anim's cell `Get_Tiberium_Value @ 0x00485020`
    /// hides it at or below `abs(HideIfNoOre.Threshold=)`, not only at zero.
    #[test]
    fn hide_if_no_ore_threshold_moves_the_value_cut_off() {
        use crate::sim::overlay_grid::OverlayGrid;
        use crate::sim::world::Simulation;

        let ini = IniFile::from_str(&format!(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n{}",
            crate::sim::tiberium::test_support::tiberium_rules_text(),
        ));
        let mut rules = RuleSet::from_ini(&ini).expect("twinkle rules");
        let mut art = ArtRegistry::from_ini(&IniFile::from_str(
            "[TWNK1]\nLoopCount=-1\nDetailLevel=2\nHideIfNoOre=true\n\
             HideIfNoOre.Threshold=-60\nRate=450\n",
        ));
        art.bind_anim_frame_count_for_test("TWNK1", 8);
        rules.replace_art_registry_for_test(art);
        let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);

        let hidden_over = |density: u8| {
            let mut sim = Simulation::new();
            sim.session.set_map_width_for_test(16);
            sim.session.set_map_height_for_test(16);
            let mut grid = OverlayGrid::new(16, 16);
            grid.place_overlay(8, 8, overlays.id_for_name("TIB01").unwrap(), density);
            *sim.overlay_grid_mut_for_test() = Some(grid);
            let type_id = sim.interner.intern("TWNK1");
            let id = sim
                .spawn_anim_at_world(
                    &rules,
                    AnimClassSpawnDescriptor {
                        loop_count: -1,
                        ..runtime_descriptor(type_id, 0)
                    },
                    AnimWorldCoord {
                        x: 8 * 256 + 128,
                        y: 8 * 256 + 128,
                        z: 0,
                    },
                )
                .expect("the twinkle constructs");
            sim.visit_anim(
                id,
                &rules,
                Some(&overlays),
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
            sim.anim(id)
                .expect("the looping twinkle survives its visit")
                .draw_runtime
                .hidden
        };

        // `TiberiumClass::Value` times `OverlayData + 1`, so 50 against the
        // threshold's 60 and 125 over it.
        assert!(hidden_over(1), "50 <= abs(-60) hides the twinkle");
        assert!(!hidden_over(4), "125 > 60 leaves it drawing");
    }

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
                .damage_fires
                .anim_ids[slot] = Some(id);
            sim.substrate.anims.get_mut(id).unwrap().damage_fire_slot =
                Some((building_id, slot as u8));
            sim.detach_all_pointer_expired(
                building_id,
                &rules,
                None,
                crate::sim::world::FrameEffects::default(),
            );
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
                for (i, id) in building.damage_fires.anim_ids.iter().enumerate() {
                    assert_eq!(id.is_some(), expected["slots"][i].as_bool().unwrap());
                }
                assert_eq!(sim.substrate.display.layer_of(id), None);
            }
        }
    }

    #[test]
    fn building_uninit_expires_attached_fires_then_limbo_uninits_them_and_survives_save() {
        let (mut sim, rules, building_id) = damage_fire_fixture(false);
        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .health
            .set_for_test(50);
        sim.update_building_damage_fire(building_id, &rules);
        let ids = sim
            .entities()
            .get(building_id)
            .unwrap()
            .damage_fires
            .anim_ids;
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
        sim.sound_events.clear_for_test();
        sim.uninit_with_rules(building_id, &rules);
        // ObjectUnInit5F6616 expires the owner first (Anim425150: owner
        // cleared, +19B set), then its Limbo call reaches Building Limbo
        // 4458E4, which UnInits each fire (Release4255D5) and nulls its slot
        // before TechnoLimbo6F6C6B releases the empty4A4 handle.
        assert!(matches!(
            sim.sound_events.as_slice().last(),
            Some(SimSoundEvent::GattlingLoopRelease { owner })
                if *owner == crate::sim::combat::gattling::gattling_sound_owner(building_id)
        ));
        assert!(
            sim.entities()
                .get(building_id)
                .unwrap()
                .damage_fires
                .anim_ids
                .iter()
                .all(Option::is_none)
        );
        for (id, coord) in &coords {
            let anim = sim.anim(*id).unwrap();
            assert_eq!(anim.world_coord, *coord);
            assert_eq!(anim.owner_entity, None);
            assert!(anim.runtime.inactive);
            assert_eq!(anim.damage_fire_slot, None);
            assert!(sim.substrate.pending_delete.contains_object(id));
            assert_eq!(sim.substrate.display.layer_of(*id), None);
            assert!(sim.sound_events.iter().any(|event| matches!(event,
                SimSoundEvent::ObjectSoundReleased { owner } if *owner == *id)));
        }
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let hash = sim.state_hash();
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "building-expiry", 0);
        sim = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        sim.restore_after_snapshot_load().unwrap();
        assert_eq!(sim.state_hash(), hash);
        sim.process_pending_delete();
        assert!(sim.entities().get(building_id).is_none());
        for (id, _) in coords {
            assert!(sim.anim(id).is_none());
        }
    }

    /// AnimClass::AI's Next= switch (`0x004247F3..0x00424925`) sets Value to
    /// the new type's Start (`0x0042491F`) and leaves the step as it was.
    #[test]
    fn next_takes_the_new_start_as_its_value_and_keeps_the_step() {
        let rules = runtime_rules(
            "[A]\nReverse=yes\nNext=A_NEXT\n[A_NEXT]\nStart=3\n",
            &[("A", 10), ("A_NEXT", 10)],
        );
        let mut sim = Simulation::with_seed(0);
        let type_id = sim.interner.intern("A");
        let id = sim
            .spawn_anim_at_world(
                &rules,
                runtime_descriptor(type_id, 0),
                AnimWorldCoord {
                    x: 2816,
                    y: 2944,
                    z: 0,
                },
            )
            .unwrap();
        assert_eq!(sim.anim(id).unwrap().runtime.frame_step, -1);
        sim.switch_anim_type(id, "A_NEXT", &rules, None, FrameEffects::default())
            .unwrap();
        let runtime = &sim.anim(id).unwrap().runtime;
        assert_eq!((runtime.current_frame, runtime.frame_step), (3, -1));
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
            let owner_id = sim.substrate.allocate_stable_id();
            let owner_name = sim.interner.intern("HOUSE");
            let type_ref = sim.interner.intern("UNIT");
            let mut owner = GameEntity::new_at_frame_zero_for_test(
                owner_id,
                10,
                10,
                0,
                0,
                owner_name,
                Health::new(100),
                type_ref,
                EntityCategory::Unit,
                0,
                5,
                false,
            );
            owner.position.set_sub_x_for_test(SimFixed::from_num(128));
            owner.position.set_sub_y_for_test(SimFixed::from_num(128));
            owner.position.set_exact_z_leptons_for_test(Some(0));
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
            sim.switch_anim_type(id, "A_NEXT", &rules, None, FrameEffects::default())
                .unwrap();
            assert_eq!(
                sim.anim(id).unwrap().display.y_sort_adjust,
                input["adjust"].as_i64().unwrap() as i32
            );
            sim.substrate
                .mark_anim_display(id, input["marked"].as_bool().unwrap());
            for observation in row["observations"].as_array().unwrap() {
                match observation["op"].as_str().unwrap() {
                    "submit" => sim.submit_anim_display(id, &rules),
                    "attach" => {
                        assert!(sim.set_anim_owner_object(id, Some(owner_id), &rules));
                    }
                    "detach" => {
                        assert!(sim.set_anim_owner_object(id, None, &rules));
                    }
                    "move_owner" => {
                        let owner = sim.substrate.entities.get_mut(owner_id).unwrap();
                        owner.position.set_rx_for_test(3000 / 256);
                        owner.position.set_ry_for_test(3100 / 256);
                        owner
                            .position
                            .set_sub_x_for_test(SimFixed::from_num(3000 % 256));
                        owner
                            .position
                            .set_sub_y_for_test(SimFixed::from_num(3100 % 256));
                        owner.position.set_exact_z_leptons_for_test(Some(700));
                    }
                    "expire" => {
                        assert!(sim.anim_pointer_expired(
                            id,
                            owner_id,
                            crate::rules::ruleset::RuleSet::empty_for_test()
                        ));
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
                assert_eq!(sim.anim_display_layer(id, &rules), queried);
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
    fn trailer_separation_spawns_on_signed_multiples_and_never_divides_by_zero() {
        assert!(!trailer_cadence_matches(0, 0, false));
        assert!(trailer_cadence_matches(7, 1, false));
        assert!(trailer_cadence_matches(6, 3, false));
        assert!(!trailer_cadence_matches(7, 3, false));
        assert!(trailer_cadence_matches(6, -3, false));
        assert!(!trailer_cadence_matches(7, -3, false));
        // Ares spawns no trailer below a separation of 1.
        assert!(!trailer_cadence_matches(6, -3, true));
        assert!(trailer_cadence_matches(6, 3, true));
    }

    fn damage_fire_fixture(can_be_occupied: bool) -> (Simulation, RuleSet, u64) {
        damage_fire_fixture_with_strength(can_be_occupied, 100)
    }

    fn damage_fire_fixture_with_strength(
        can_be_occupied: bool,
        strength: i32,
    ) -> (Simulation, RuleSet, u64) {
        damage_fire_fixture_with_offsets(
            can_be_occupied,
            strength,
            "DamageFireOffset0=-24,-1\nDamageFireOffset1=64,36\n",
        )
    }

    /// `offsets` are TESTART's `DamageFireOffsetN=` lines.
    fn damage_fire_fixture_with_offsets(
        can_be_occupied: bool,
        strength: i32,
        offsets: &str,
    ) -> (Simulation, RuleSet, u64) {
        let rules_ini = IniFile::from_str(&format!(
            "[BuildingTypes]\n0=TESTBLD\n\n\
             [TESTBLD]\nStrength={strength}\nImage=TESTART\nCanBeOccupied={}\n\n\
             [General]\nDamageFireTypes=FIRE01,FIRE02,FIRE03\n\n\
             [AudioVisual]\nConditionYellow=50%\nConditionRed=25%\n",
            if can_be_occupied { "yes" } else { "no" },
        ));
        let art_ini = IniFile::from_str(&format!(
            "[TESTART]\nFoundation=4x4\n{offsets}\n\
             [FIRE01]\nRate=450\nLoopCount=-1\nStartSound=BuildingFireBig\n\n\
             [FIRE02]\nRate=450\nLoopCount=-1\nStartSound=BuildingFireMed\n\n\
             [FIRE03]\nRate=450\nLoopCount=-1\nStartSound=BuildingFireSmall\n",
        ));
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
        let id = sim.substrate.allocate_stable_id();
        let mut building = GameEntity::new_at_frame_zero_for_test(
            id,
            10,
            10,
            0,
            0,
            owner,
            Health::new(100),
            type_ref,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        building
            .type_facts
            .set_foundation_for_test("4x4".to_string().into());
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
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .health
                    .set_for_test(row.input.current);
                sim.update_building_damage_fire(id, &rules);
                let building = sim.substrate.entities.get(id).unwrap();
                let expected = if occupied {
                    row.output.damage_fire_occupied
                } else {
                    row.output.damage_fire_ordinary
                };
                assert_eq!(
                    building.damage_fires.active, expected,
                    "{row:?}, occupied={occupied}"
                );
                assert_eq!(
                    building.damage_fires.anim_ids.iter().flatten().count(),
                    if expected { 2 } else { 0 },
                    "{row:?}"
                );
            }
        }
    }

    /// 43C124..43C137: an authored (0,0) offset ends the fires, as the
    /// reader's end marker does, so later offsets get none.
    #[test]
    fn a_zero_damage_fire_offset_ends_the_fires() {
        let (mut sim, rules, id) = damage_fire_fixture_with_offsets(
            false,
            100,
            "DamageFireOffset0=-24,-1\nDamageFireOffset1=0,0\nDamageFireOffset2=64,36\n",
        );
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .health
            .set_for_test(10);
        sim.update_building_damage_fire(id, &rules);
        let fires = &sim.substrate.entities.get(id).unwrap().damage_fires;
        assert!(fires.active);
        let slots: Vec<bool> = fires.anim_ids.iter().map(Option::is_some).collect();
        assert_eq!(
            slots,
            [true, false, false, false, false, false, false, false]
        );
    }

    /// Each `art_text` section is an AnimType, so the ordered Rules processor
    /// reads its fixed-ART Type references (`Next=`, `TrailerAnim=`, ...) as
    /// in production.
    fn runtime_rules(art_text: &str, frame_counts: &[(&str, i32)]) -> RuleSet {
        let art_ini = IniFile::from_str(art_text);
        let animations: String = art_ini
            .section_names()
            .iter()
            .enumerate()
            .map(|(index, name)| format!("{index}={name}\n"))
            .collect();
        // SOUNDMD knows every sound the art names.
        let sounds: Vec<&str> = art_ini
            .section_names()
            .iter()
            .filter_map(|name| art_ini.section(name))
            .flat_map(|section| {
                ["StartSound", "Report", "StopSound"].map(|key| section.get_for_test(key))
            })
            .flatten()
            .collect();
        let mut rules = RuleSet::from_ini_with_fixed_art_and_sounds_for_test(
            &IniFile::from_str(&format!(
                "[General]\nDamageFireTypes=\n\n[AudioVisual]\nConditionYellow=50%\nConditionRed=25%\n\n\
                 [Animations]\n{animations}"
            )),
            &art_ini,
            &sounds,
        )
        .unwrap();
        let mut art = ArtRegistry::from_ini(&art_ini);
        for &(name, frames) in frame_counts {
            art.bind_anim_frame_count_for_test(name, frames);
        }
        rules.install_art_fixture(art);
        rules
    }

    /// Phobos `DetachOnCloak=`: `Detach_All(false)` on a cloaking object
    /// detaches attached anims, and only the type that reads the key `no`
    /// stays attached (`Ext/AnimType/Body.cpp:112`, hook `0x425174`).
    #[test]
    fn detach_on_cloak_keeps_only_the_anim_type_that_asks_for_it() {
        let rules = runtime_rules(
            "[CLOAKKEEP]\nDetachOnCloak=no\nLayer=Ground\n[CLOAKNORM]\nLayer=Ground\n",
            &[("CLOAKKEEP", 8), ("CLOAKNORM", 8)],
        );
        let mut sim = Simulation::new();
        let owner_id = 100;
        let type_ref = sim.interner.intern("CLOAKOWNER");
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            owner_id,
            5,
            5,
            0,
            0,
            sim.interner.intern("Americans"),
            Health::new(100),
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            false,
        );
        entity.lifecycle.set_in_limbo_for_test(false);
        sim.substrate.entities.insert(entity);
        let keep_type = sim.interner.intern("CLOAKKEEP");
        let norm_type = sim.interner.intern("CLOAKNORM");
        let keep = sim
            .spawn_anim_object(&rules, runtime_descriptor(keep_type, 0))
            .unwrap();
        let norm = sim
            .spawn_anim_object(&rules, runtime_descriptor(norm_type, 0))
            .unwrap();
        for id in [keep, norm] {
            assert!(sim.set_anim_owner_object(id, Some(owner_id), &rules));
        }

        sim.detach_attached_anims_for_cloak(owner_id, &rules);

        assert_eq!(
            sim.anim(keep).unwrap().owner_entity,
            Some(owner_id),
            "DetachOnCloak=no keeps the attached anim"
        );
        assert_eq!(
            sim.anim(norm).unwrap().owner_entity,
            None,
            "the default detaches"
        );
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
            owner: None,
        }
    }

    /// A producer's `owner` attaches inside the constructor without Reveal's
    /// own Display submit. Destroy follows the shared SetOwner(NULL) detach
    /// and conceal sequence, leaving Display as those native calls do.
    #[test]
    fn an_owned_anim_leaves_display_as_construct_then_attach_does() {
        let rules = runtime_rules("[A]\nLayer=Ground\n", &[("A", 10)]);
        let coord = AnimWorldCoord {
            x: 2816,
            y: 2944,
            z: 0,
        };
        let scene = || {
            let mut sim = Simulation::with_seed(0);
            let house = sim.interner.intern("HOUSE");
            let unit = sim.interner.intern("UNIT");
            let ids: Vec<u64> = [(10, 10), (14, 3), (6, 9), (12, 12), (8, 15)]
                .into_iter()
                .map(|(rx, ry)| {
                    let id = sim.substrate.allocate_stable_id();
                    sim.substrate
                        .entities
                        .insert(GameEntity::new_at_frame_zero_for_test(
                            id,
                            rx,
                            ry,
                            0,
                            0,
                            house,
                            Health::new(100),
                            unit,
                            EntityCategory::Unit,
                            0,
                            5,
                            false,
                        ));
                    sim.resubmit_entity_display_for_test(id);
                    id
                })
                .collect();
            let type_id = sim.interner.intern("A");
            (sim, ids[3], type_id)
        };
        let layers = |sim: &Simulation| -> Vec<Vec<u64>> {
            (0..5)
                .map(|index| {
                    let layer = crate::sim::world::display_layers::DisplayLayer::from_index(index);
                    sim.substrate.display.members(layer.unwrap()).to_vec()
                })
                .collect()
        };

        let (mut native, owner, type_id) = scene();
        let id = native
            .spawn_anim_at_world(&rules, runtime_descriptor(type_id, 0), coord)
            .unwrap();
        assert!(native.set_anim_owner_object(id, Some(owner), &rules));
        let (mut sim, _, _) = scene();
        let descriptor = AnimClassSpawnDescriptor {
            owner: Some(owner),
            ..runtime_descriptor(type_id, 0)
        };
        assert_eq!(
            sim.spawn_anim_at_world(&rules, descriptor, coord).unwrap(),
            id
        );
        assert_eq!(sim.anim(id).unwrap().owner_entity, Some(owner));
        assert_eq!(
            sim.anim(id).unwrap().world_coord,
            native.anim(id).unwrap().world_coord
        );
        assert!(sim.anim(id).unwrap().display.marked_on_map);
        assert_eq!(layers(&sim), layers(&native));

        assert!(native.set_anim_owner_object(id, None, &rules));
        native.destroy_anim(id, &rules);
        sim.destroy_anim(id, &rules);
        assert_eq!(
            sim.anim(id).unwrap().world_coord,
            native.anim(id).unwrap().world_coord
        );
        assert_eq!(sim.anim(id).unwrap().owner_entity, None);
        assert_eq!(layers(&sim), layers(&native));
        assert_eq!(sim.substrate.display.layer_of(id), None);
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
                Health::new(100),
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
                if detach_with_coords {
                    assert_eq!(sim.detach_anim_from_owner(id, &rules), Some(owner_id));
                } else {
                    assert!(sim.anim_pointer_expired(id, owner_id, &rules));
                }
                assert_eq!(sim.anim(id).unwrap().owner_entity, None);
                let entity = sim.substrate.entities.get(owner_id).unwrap();
                assert_eq!(entity.deploy_anim(), expected);
                assert!(entity.unit_deploying());
                assert!(!entity.is_fully_deployed());
            }
        }
    }

    /// SetOwnerObject424B50, destructor4228E0 and PointerExpired425150 have
    /// distinct effects on the retained Object+84 byte and owner+60 links.
    #[test]
    fn attached_animation_transfer_and_expiry_preserve_shared_owner_state() {
        let rules = runtime_rules("[A]\nEnd=3\n", &[("A", 3)]);
        let mut sim = Simulation::new();
        sim.interner = crate::sim::intern::test_interner();
        for owner in [100, 101] {
            sim.substrate.entities.insert(GameEntity::test_default(
                owner,
                "UNIT",
                "Americans",
                10,
                10,
            ));
        }
        let type_id = sim.interner.intern("A");
        let first = sim
            .spawn_anim_object(&rules, runtime_descriptor(type_id, 0))
            .unwrap();
        let second = sim
            .spawn_anim_object(&rules, runtime_descriptor(type_id, 0))
            .unwrap();
        for anim in [first, second] {
            assert!(sim.set_anim_owner_object(anim, Some(100), &rules));
        }
        // The native scan includes an inactive Anim while its CC still names
        // this owner; filtering to Logic/Display/live entries clears too early.
        sim.substrate
            .anims
            .get_mut(second)
            .unwrap()
            .runtime
            .inactive = true;
        assert!(sim.set_anim_owner_object(first, Some(101), &rules));
        assert!(sim.substrate.entities.get(100).unwrap().is_parachuted());
        assert!(sim.substrate.entities.get(101).unwrap().is_parachuted());

        sim.scalar_delete_anim(second);
        assert!(sim.anim(second).is_none());
        assert!(!sim.substrate.entities.get(100).unwrap().is_parachuted());
        sim.substrate
            .entities
            .hold_parachute_animation(101, first)
            .unwrap();
        assert!(sim.anim_pointer_expired(first, 101, &rules));
        assert_eq!(sim.anim(first).unwrap().owner_entity, None);
        let owner = sim.substrate.entities.get(101).unwrap();
        assert_eq!(
            owner.parachute_animation(),
            None,
            "owner+60 ran before unlink"
        );
        assert!(owner.is_parachuted(), "PointerExpired retains Object+84");
        sim.scalar_delete_anim(first);
        assert!(sim.anim(first).is_none());
        assert!(
            sim.substrate.entities.get(101).unwrap().is_parachuted(),
            "a later destructor cannot reconstruct the already-cleared owner"
        );

        // GameEntity serde is the production snapshot payload. Preserve the
        // orphaned raw flag, then let ObjectAI consume it in a subsequent fall.
        let cap = rules.general.parachute_max_fall_rate;
        let owner = sim.substrate.entities.get_mut(101).unwrap();
        owner.set_fall_rate_for_test(cap);
        let bytes = serde_json::to_vec(owner).unwrap();
        let mut restored: GameEntity = serde_json::from_slice(&bytes).unwrap();
        restored.lifecycle.set_in_limbo_for_test(false);
        restored.lifecycle.set_cell_marked_for_test(false);
        restored.position.put_z(512);
        restored.begin_bomb_fall();
        *sim.substrate.entities.get_mut(101).unwrap() = restored;
        sim.object_ai_fall(101, &rules, None, FrameEffects::default())
            .unwrap();
        let owner = sim.substrate.entities.get(101).unwrap();
        assert!(owner.is_parachuted());
        assert_eq!(owner.parachute_animation(), None);
        assert_eq!(
            owner.fall_rate(),
            cap,
            "restored raw flag still selects canopy cap"
        );
    }

    /// The owners' counts answer SetOwnerObject424B79's and the destructor
    /// 42291A's Anim array scan through each owner write, a load, and a
    /// removal of an Anim that still names its owner.
    #[test]
    fn the_last_anim_naming_an_owner_releases_it_through_writes_loads_and_removals() {
        let rules = runtime_rules("[A]\nEnd=3\n", &[("A", 3)]);
        let mut sim = Simulation::new();
        sim.interner = crate::sim::intern::test_interner();
        for owner in [100, 101] {
            sim.substrate.entities.insert(GameEntity::test_default(
                owner,
                "UNIT",
                "Americans",
                10,
                10,
            ));
        }
        let type_id = sim.interner.intern("A");
        let [a, b, c] = [0; 3].map(|_| {
            sim.spawn_anim_object(&rules, runtime_descriptor(type_id, 0))
                .unwrap()
        });
        for (anim, owner) in [(a, 100), (b, 100), (c, 101)] {
            assert!(sim.set_anim_owner_object(anim, Some(owner), &rules));
        }
        let parachuted =
            |sim: &Simulation, owner| sim.substrate.entities.get(owner).unwrap().is_parachuted();
        sim.substrate.anims = crate::util::save_codec::deserialize(
            &crate::util::save_codec::serialize(&sim.substrate.anims).unwrap(),
        )
        .unwrap();
        assert!(sim.set_anim_owner_object(a, Some(101), &rules));
        assert!(parachuted(&sim, 100), "the loaded b still names 100");
        // PointerExpired's bare CC clear leaves b no longer naming 100.
        assert_eq!(sim.substrate.release_anim_owner_reference(b), Some(100));
        assert!(sim.set_anim_owner_object(c, Some(100), &rules));
        assert!(parachuted(&sim, 101), "a still names 101");
        assert!(sim.set_anim_owner_object(c, None, &rules));
        assert!(!parachuted(&sim, 100), "c was the last Anim naming 100");
        assert!(sim.set_anim_owner_object(b, Some(101), &rules));
        assert!(sim.substrate.anims.remove(b).is_some());
        sim.scalar_delete_anim(a);
        assert!(
            !parachuted(&sim, 101),
            "a was the last stored Anim naming 101"
        );
    }

    #[test]
    fn deployment_attachment_retirement_without_rules_keeps_absolute_sound_position() {
        let rules = runtime_rules("[DEPLOY]\nEnd=3\nStopSound=DeployDone\n", &[("DEPLOY", 3)]);
        let mut sim = Simulation::new();
        let owner_id = 100;
        let mut entity = GameEntity::test_default(owner_id, "UNIT", "Americans", 10, 10);
        entity.set_owner_for_test(sim.interner.intern("Americans"));
        entity.set_type_ref_for_test(sim.interner.intern("UNIT"));
        sim.substrate.entities.insert(entity);
        let type_id = sim.interner.intern("DEPLOY");
        let id = sim
            .spawn_anim_object(&rules, runtime_descriptor(type_id, 0))
            .unwrap();
        sim.set_anim_owner_object(id, Some(owner_id), &rules);
        let entity = sim.substrate.entities.get_mut(owner_id).unwrap();
        entity.retain_deploy_anim(id);
        entity.position.set_rx_for_test(entity.position.rx() + 1);
        let absolute = sim.anim_absolute_coord(id).unwrap();
        assert_ne!(sim.anim(id).unwrap().world_coord, absolute);

        sim.destroy_anim_with_context(id, crate::rules::ruleset::RuleSet::empty_for_test());

        let anim = sim.anim(id).unwrap();
        assert_eq!(anim.world_coord, absolute);
        assert_eq!(anim.owner_entity, None);
        assert_eq!(
            sim.substrate.entities.get(owner_id).unwrap().deploy_anim(),
            None
        );
        assert!(sim.substrate.pending_delete.contains_object(&id));
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
            sim.session
                .set_map_name_for_test("anim-boundary".to_string());
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
                resources: std::sync::Arc::new(resources),
            };
            let mut hashes = Vec::new();
            for frame in 0..=17 {
                assert_eq!(runtime.simulation.session.binary_frame(), frame);
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
                    restored.retain_process_state_for_load(
                        crate::sim::world::SnapshotProcessState::Live(&runtime.simulation),
                    );
                    assert_eq!(restored.rng_state(), runtime.simulation.rng_state());
                    runtime.replace_simulation(restored);
                    assert_eq!(runtime.simulation.state_hash(), before);
                }
                let output = runtime
                    .advance_frame(
                        &[],
                        67,
                        TickLane::Ordinary,
                        crate::sim::world::FrameEffects::default(),
                    )
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

    /// `AnimClass::AI @ 0x00423AC0`: the loop boundary stores the signed
    /// `RandomLoopDelay=` draw (`0x004247B1..0x004247E5`) and the countdown
    /// tests `!= 0` (`0x00424386`), so a negative pause holds the anim.
    #[test]
    fn a_negative_random_loop_delay_holds_the_anim() {
        let rules = runtime_rules(
            "[LOOPER]\nRate=900\nEnd=2\nLoopCount=-1\nRandomLoopDelay=-3,-3\n",
            &[("LOOPER", 2)],
        );
        let mut sim = Simulation::new();
        let type_name = sim.interner.intern("LOOPER");
        let id = sim
            .spawn_anim_object(
                &rules,
                AnimClassSpawnDescriptor {
                    loop_count: -1,
                    ..runtime_descriptor(type_name, 0)
                },
            )
            .expect("bound art constructs");
        for frame in 1..40 {
            sim.session.set_binary_frame_for_test(frame);
            sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
                .unwrap();
        }
        let delay = sim.anim(id).unwrap().runtime.delay_remaining;
        assert!(delay < -3, "the pause counts on down, got {delay}");
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
        sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
            .unwrap();
        sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
            .unwrap();
        assert_eq!(sim.anim(id).unwrap().runtime.delay_remaining, 1);
        assert_eq!(starts(&sim), 0);

        sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
            .unwrap();
        assert_eq!(sim.anim(id).unwrap().runtime.delay_remaining, 0);
        assert_eq!(starts(&sim), 1, "the expiring visit runs Start");
        assert_eq!(
            sim.anim(id).unwrap().runtime.current_frame,
            0,
            "and returns before advancing a frame"
        );

        sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
            .unwrap();
        assert_eq!(starts(&sim), 1, "Start runs once per delay");
    }

    /// Phobos `DetachedReport=` (`Ext/AnimType/Body.h:63,115`,
    /// `Ext/AnimType/Body.cpp:108`): `AnimClass::Start`'s hook `0x424CF1`
    /// (`Ext/Anim/Hooks.cpp:269-280`) plays a positional `VocClass::PlayAt` cue
    /// at the animation itself, alongside its own `Report=`.
    #[test]
    fn detached_report_plays_a_positional_cue_from_start() {
        let rules = runtime_rules(
            "[CUE]\nRate=900\nEnd=1\nReport=OwnCue\nDetachedReport=TestCue\n",
            &[("CUE", 1)],
        );
        let mut sim = Simulation::with_seed(0);
        let type_id = sim.interner.intern("CUE");
        let id = sim
            .spawn_anim_object(
                &rules,
                AnimClassSpawnDescriptor::new(
                    type_id,
                    7,
                    9,
                    crate::util::lepton::CELL_CENTER_LEPTON,
                    crate::util::lepton::CELL_CENTER_LEPTON,
                    0,
                ),
            )
            .expect("bound art constructs");
        let cues = sim
            .sound_events
            .iter()
            .filter_map(|event| match event {
                SimSoundEvent::VocAtCoord {
                    sound_id,
                    coordinate,
                    ..
                } => Some((sound_id.clone(), *coordinate)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let world = sim.anim_absolute_coord(id).unwrap();
        assert_eq!(
            cues,
            [(
                "TESTCUE".to_owned(),
                DriveCoord {
                    x: world.x,
                    y: world.y,
                    z: world.z
                }
            )],
            "the detached cue plays at the animation, next to its own Report="
        );
        assert!(
            sim.sound_events
                .iter()
                .any(|event| matches!(event, SimSoundEvent::AnimationStarted { anim_id, .. } if *anim_id == id)),
            "the ordinary Report= still plays"
        );
    }
    /// A presentation anim draws where gamemd's Display would hold it as an
    /// ordinary anim: on Ground by its GetYSort key (Anim422BC0, inserted
    /// by Ground551A90) among the members, not after all of them.
    #[test]
    fn a_ground_presentation_anim_sorts_among_the_ground_members() {
        use crate::sim::world::display_layers::DisplayLayer;
        let rules = runtime_rules("[PUFF]\nEnd=8\nLayer=Ground\n", &[("PUFF", 8)]);
        let zero = crate::util::fixed_math::SIM_ZERO;
        let mut sim = Simulation::new();
        sim.present_effect_anims();
        let type_id = sim.interner.intern("PUFF");
        let near = sim
            .spawn_combat_explosion_anim(&rules, type_id, 1, 1, zero, zero, 0, 0)
            .unwrap();
        let far = sim
            .spawn_combat_explosion_anim(&rules, type_id, 10, 10, zero, zero, 0, 0)
            .unwrap();
        // X+Y 2000 lies between the kept anims' cells 1,1 and 10,10.
        let point = crate::sim::projectile::ProjectileCoord::new(1000, 1000, 0);
        let packet = crate::sim::combat::detonation_anim::placed_effect(type_id, point);
        crate::sim::world::damage_consequences::admit_explosion_effect(&mut sim, &rules, packet);
        let (puff, layer) = sim.presentation_anims().next().unwrap();
        assert_eq!(layer, DisplayLayer::GROUND);
        let ground: Vec<u64> = sim
            .picture_draw_order(Some(&rules))
            .into_iter()
            .filter(|&(layer, _)| layer == DisplayLayer::GROUND)
            .map(|(_, id)| id)
            .collect();
        assert_eq!(ground, [near, puff, far]);
    }

    /// The impact packet keeps nothing, so a picture-only impact is a
    /// presentation anim; `spawn_combat_explosion_anim`'s callers may keep
    /// theirs (the nuke awaits its NUKEBALL), so it makes an ordinary one.
    #[test]
    fn only_an_unkept_picture_only_impact_is_a_presentation_anim() {
        let rules = runtime_rules("[PUFF]\nEnd=8\n", &[("PUFF", 8)]);
        let point = crate::sim::projectile::ProjectileCoord::new(1000, 1000, 0);
        // A new world makes it in native order: in the Anim array.
        let mut native = Simulation::new();
        let type_id = native.interner.intern("PUFF");
        let packet = crate::sim::combat::detonation_anim::placed_effect(type_id, point);
        crate::sim::world::damage_consequences::admit_explosion_effect(&mut native, &rules, packet);
        assert_eq!(native.anims().count(), 1, "in the Anim array, as gamemd's");
        assert_eq!(native.presentation_anims().count(), 0);

        let mut sim = Simulation::new();
        sim.present_effect_anims();
        let type_id = sim.interner.intern("PUFF");
        let packet = crate::sim::combat::detonation_anim::placed_effect(type_id, point);
        crate::sim::world::damage_consequences::admit_explosion_effect(&mut sim, &rules, packet);
        assert_eq!(sim.anims().count(), 0, "not in the Anim array");
        assert_eq!(sim.presentation_anims().count(), 1);

        let zero = crate::util::fixed_math::SIM_ZERO;
        let kept = sim
            .spawn_combat_explosion_anim(&rules, type_id, 3, 3, zero, zero, 0, 0)
            .unwrap();
        assert!(!sim.anim(kept).unwrap().is_presentation());
        assert_eq!(sim.anims().count(), 1);
    }

    #[test]
    fn signed_impact_packet_keeps_exact_xy_through_explosion_admission() {
        // Sticky= keeps it an ordinary anim (`ArtRegistry::anim_presentation_only`).
        let rules = runtime_rules("[TWLT036]\nEnd=8\nSticky=yes\n", &[("TWLT036", 8)]);
        for (x, y) in [(-257, -511), (-1, 1811), (-128_128, 5248), (2694, 5253)] {
            let mut sim = Simulation::new();
            let type_id = sim.interner.intern("TWLT036");
            let point = crate::sim::projectile::ProjectileCoord::new(x, y, 209);
            let packet = crate::sim::combat::detonation_anim::placed_effect(type_id, point);
            let packed = (packet.rx, packet.ry, packet.sub_x, packet.sub_y);
            crate::sim::world::damage_consequences::admit_explosion_effect(
                &mut sim, &rules, packet,
            );
            let ids = sim.anims().map(|(id, _)| *id).collect::<Vec<_>>();
            assert_eq!(
                ids.len(),
                1,
                "the real constructor admits the impact packet"
            );
            let coord = sim.anim_absolute_coord(ids[0]).unwrap();
            assert_eq!(coord, AnimWorldCoord { x, y, z: 209 });
            let (rx, ry, sub_x, sub_y, _) = coord.to_cell_sub_z();
            assert_eq!((rx, ry, sub_x, sub_y), packed);
        }
    }

    #[test]
    fn combat_explosion_z_adjust_and_owner_reach_the_anim() {
        let rules = runtime_rules("[TWLT036]\nEnd=8\n", &[("TWLT036", 8)]);
        let mut sim = Simulation::new();
        let type_id = sim.interner.intern("TWLT036");
        let house = sim.interner.intern("Americans");
        let id = sim
            .spawn_combat_explosion_anim_with_z_adjust(
                &rules,
                type_id,
                1,
                1,
                crate::util::fixed_math::SIM_ZERO,
                crate::util::fixed_math::SIM_ZERO,
                0,
                0,
                7,
                Some(house),
                true,
            )
            .unwrap();
        let anim = sim.anim(id).unwrap();
        assert_eq!(anim.z_adjust, 7);
        assert_eq!(anim.owner_house(), Some(house));
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
        };
        descriptor.use_cell_drawer = true;
        descriptor.terrain_attached = true;
        let draw_runtime = descriptor.draw_runtime;
        let id = sim.spawn_anim_object(&rules, descriptor).unwrap();
        assert_eq!(sim.anim(id).unwrap().draw_runtime, draw_runtime);

        let serialized = crate::util::save_codec::serialize(&sim.substrate.anims).unwrap();
        let restored: AnimStore = crate::util::save_codec::deserialize(&serialized).unwrap();
        assert_eq!(restored.get(id).unwrap().draw_runtime, draw_runtime);
        assert!(restored.get(id).unwrap().use_cell_drawer);
        assert!(restored.get(id).unwrap().terrain_attached);

        let before = sim.state_hash();
        sim.substrate.anims.get_mut(id).unwrap().terrain_attached = false;
        assert_ne!(sim.state_hash(), before);
    }

    /// The store saves as the former derived newtype `AnimStore(map)` (the
    /// touch log is never saved), and every mutable hand-out is noted for the
    /// pointer-expiry listener index.
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
            crate::util::save_codec::serialize(&*store).unwrap(),
            crate::util::save_codec::serialize(&former).unwrap()
        );
        assert_eq!(
            serde_json::to_string(&*store).unwrap(),
            serde_json::to_string(&former).unwrap()
        );
        let bytes = crate::util::save_codec::serialize(&*store).unwrap();
        let restored: AnimStore = crate::util::save_codec::deserialize(&bytes).unwrap();
        assert_eq!(
            crate::util::save_codec::serialize(&restored).unwrap(),
            bytes
        );

        // Nothing has been derived from a fresh store yet.
        let keys = AnimTouchReader::ExpiryListeners;
        assert_eq!(store.take_touched(keys), Touched::All);
        assert!(store.get(id).is_some());
        assert_eq!(store.take_touched(keys), Touched::Ids(Vec::new()));
        store.get_mut(id).unwrap().z_adjust += 1;
        let removed = store.remove(id).unwrap();
        store.insert(removed);
        assert_eq!(store.take_touched(keys), Touched::Ids(vec![id]));
        assert_eq!(store.clone().take_touched(keys), Touched::All);
        let _ = store.values_mut().count();
        assert_eq!(store.take_touched(keys), Touched::All);
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
                crate::sim::world::FrameEffects::default(),
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
                crate::sim::world::FrameEffects::default(),
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
                crate::sim::world::FrameEffects::default(),
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
        sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
            .unwrap();
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(3, 4),
            0x04,
            "the first AI guard runs after MakeInfantry raw marking"
        );
        assert_eq!(sim.anim(id).unwrap().runtime.current_frame, 0);

        sim.session.set_binary_frame_for_test(1);
        sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
            .unwrap();

        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
        assert!(sim.substrate.pending_delete.contains_object(&id));
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

        sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
            .unwrap();
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(5, 6), 0x08);
        sim.session.set_binary_frame_for_test(1);
        sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
            .unwrap();

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
            // Native -1 identity with the corpus state byte.
            let mut overlays =
                crate::sim::overlay_grid::OverlayGrid::new(terrain.width(), terrain.height());
            overlays.cell_mut_for_test(x, y).overlay_data = native["state"].as_u64().unwrap() as u8;
            let mut sim = Simulation::new();
            sim.resolved_terrain = Some(terrain);
            sim.overlay_grid = Some(overlays);
            assert!(sim.bridge_state.is_none());
            let world = AnimWorldCoord {
                x: i32::from(x) * 256 + 128,
                y: i32::from(y) * 256 + 128,
                z: 1040,
            };
            // Original Infantry5217C0's center (128,128) occupies slot0:
            // walk_head_occupation.json raw[94]/[98], deck8 -> deck9.
            Simulation::apply_make_infantry_raw_occupation(
                &mut sim.substrate,
                &sim.resolved_terrain,
                world,
                AnimOccupationOperation::Mark,
            );
            assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(x, y), 0);
            assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(x, y), 0x01);

            sim.resolved_terrain
                .as_mut()
                .unwrap()
                .write_native_cell_flags(cell, flags & !0x100);
            Simulation::apply_make_infantry_raw_occupation(
                &mut sim.substrate,
                &sim.resolved_terrain,
                world,
                AnimOccupationOperation::Clear,
            );
            assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(x, y), 0);
            Simulation::apply_make_infantry_raw_occupation(
                &mut sim.substrate,
                &sim.resolved_terrain,
                world,
                AnimOccupationOperation::Mark,
            );
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
        Simulation::apply_make_infantry_raw_occupation(
            &mut sim.substrate,
            &sim.resolved_terrain,
            world,
            AnimOccupationOperation::Mark,
        );
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(0, 5), 0x01);
        Simulation::apply_make_infantry_raw_occupation(
            &mut sim.substrate,
            &sim.resolved_terrain,
            world,
            AnimOccupationOperation::Clear,
        );
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(0, 5), 0);

        assert_eq!(
            Simulation::map_coord_cell(&sim.resolved_terrain, -128 - 256, world.y),
            None
        );
        assert_eq!(
            Simulation::map_coord_cell(&sim.resolved_terrain, 512 * 256 + 128, world.y),
            Some((0, 6))
        );
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
        Simulation::apply_make_infantry_raw_occupation(
            &mut sim.substrate,
            &sim.resolved_terrain,
            on_map,
            AnimOccupationOperation::Mark,
        );
        let raw = &sim.substrate.raw_cell_occupation;
        assert_eq!(raw.ground_bits(3, 3), 0x05);
        assert_eq!(raw.occupant_at(RawCellKey::Real(3, 3), ground).1, None);
        Simulation::apply_make_infantry_raw_occupation(
            &mut sim.substrate,
            &sim.resolved_terrain,
            on_map,
            AnimOccupationOperation::Clear,
        );
        let raw = &sim.substrate.raw_cell_occupation;
        assert_eq!(raw.ground_bits(3, 3), 0x04);
        assert_eq!(raw.occupant_at(RawCellKey::Real(3, 3), ground).1, None);

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
        Simulation::apply_make_infantry_raw_occupation(
            &mut sim.substrate,
            &sim.resolved_terrain,
            off_map,
            AnimOccupationOperation::Mark,
        );
        let raw = &sim.substrate.raw_cell_occupation;
        assert_eq!(raw.bits_at(RawCellKey::Dummy, ground), 0x09);
        assert_eq!(raw.occupant_at(RawCellKey::Dummy, ground).1, None);
        Simulation::apply_make_infantry_raw_occupation(
            &mut sim.substrate,
            &sim.resolved_terrain,
            off_map,
            AnimOccupationOperation::Clear,
        );
        let raw = &sim.substrate.raw_cell_occupation;
        assert_eq!(raw.bits_at(RawCellKey::Dummy, ground), 0x08);
        assert_eq!(raw.occupant_at(RawCellKey::Dummy, ground).1, None);
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
                    body.position()[axis].bits(),
                    bits("position_bits", axis),
                    "{input}"
                );
                assert_eq!(
                    body.velocity()[axis].bits(),
                    bits("velocity_bits", axis),
                    "{input}"
                );
            }
            assert_eq!(
                body.elasticity().bits(),
                native["elasticity_bits"].as_u64().unwrap(),
                "{input}"
            );
            assert_eq!(
                body.gravity().bits(),
                native["gravity_bits"].as_u64().unwrap(),
                "{input}"
            );
            assert_eq!(
                body.angular_velocity_magnitude().bits(),
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
                        crate::sim::world::FrameEffects::default(),
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

    /// Original full Anim421EA0 constructor controls, with a supplied genuine
    /// Random2Class state that makes one of the Z/Y/X draws INT_MIN. Shared
    /// abs_before_modulo must retain the native CDQ/XOR/SUB-before-IDIV sign.
    /// Frame count 2 is a declared fixture binding; no image/render claim.
    #[test]
    fn bouncer_launch_int_min_axes_match_the_original() {
        let golden: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/anim_bouncer_launch_intmin.json",
        ))
        .unwrap();
        let rows = golden["ctor"].as_array().unwrap();
        assert_eq!(rows.len(), 3);
        for row in rows {
            let input = &row["input"];
            let name = input["type"].as_str().unwrap();
            let rate = &input["random_rate"];
            let rules = runtime_rules(
                &format!(
                    "[{name}]\nBouncer=yes\nRandomRate={},{}\nElasticity={}\nMaxXYVel={}\nMinZVel={}\nEnd=2\n",
                    rate[0], rate[1], input["elasticity"], input["max_xy"], input["min_z"],
                ),
                &[(name, 2)],
            );
            let mut sim = Simulation::with_seed(0);
            // The recorded seed describes native setup, not this constrained
            // boundary. Import every logical RNG field without reseeding it.
            sim.scenario_rng = crate::sim::rng::SimRng::from_native_state_hex_for_test(
                row["rng_before"].as_str().unwrap(),
            );
            let type_id = sim.interner.intern(name);
            let native_id = sim.next_native_runtime_id();
            let at = |axis: usize| input["coord"][axis].as_i64().unwrap() as i32;
            let (id, raw) = crate::sim::rng::trace_draws(|| {
                sim.spawn_anim_at_world_with_constructor(
                    &rules,
                    runtime_descriptor(type_id, 0),
                    AnimWorldCoord {
                        x: at(0),
                        y: at(1),
                        z: at(2),
                    },
                    AnimConstructorInput::AssignedIdentity(native_id),
                    crate::sim::world::FrameEffects::default(),
                )
                .unwrap()
            });
            let actual_raw: Vec<u32> = raw
                .iter()
                .map(|draw| draw["value"].as_u64().unwrap() as u32)
                .collect();
            let native_raw: Vec<u32> = row["raw_draws"]
                .as_array()
                .unwrap()
                .iter()
                .map(|draw| draw.as_u64().unwrap() as u32)
                .collect();
            assert_eq!(actual_raw, native_raw, "{input}: all raw words in order");
            assert_eq!(
                raw.len() as u64,
                row["raw_draw_count"].as_u64().unwrap(),
                "{input}"
            );
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                row["rng_after"].as_str().unwrap(),
                "{input}"
            );

            let velocity_draws: Vec<u32> = row["velocity_draws"]
                .as_array()
                .unwrap()
                .iter()
                .map(|draw| draw.as_i64().unwrap() as i32 as u32)
                .collect();
            assert_eq!(
                &actual_raw[..3],
                velocity_draws.as_slice(),
                "{input}: Z,Y,X first"
            );
            let exceptional_draw = match input["name"].as_str().unwrap() {
                "int_min_z" => 0,
                "int_min_y" => 1,
                "int_min_x" => 2,
                name => panic!("unexpected axis control {name}"),
            };
            assert_eq!(actual_raw[exceptional_draw], i32::MIN as u32, "{input}");
            assert_eq!(
                velocity_draws
                    .iter()
                    .filter(|&&word| word == i32::MIN as u32)
                    .count(),
                1
            );
            let request_sites: Vec<_> = row["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|event| event["site"].as_str())
                .collect();
            assert_eq!(
                request_sites,
                [
                    "0x004221F5",
                    "0x00422552",
                    "0x00422564",
                    "0x00422579",
                    "0x00439857",
                    "0x00439875",
                    "0x00439897"
                ],
                "{input}"
            );

            let anim = sim.anim(id).unwrap();
            assert_eq!(anim.native_unique_id, native_id);
            let body = anim.bounce.expect("Bouncer=yes must construct its body");
            // Every stored field, including both quaternions, is compared to
            // the whole original constructor rather than a second launch port.
            let actual = serde_json::json!({
                "elasticity_bits": body.elasticity().bits(),
                "gravity_bits": body.gravity().bits(),
                "clamp_bits": body.angular_velocity_magnitude().bits(),
                "position_bits": body.position().map(|v| v.bits()),
                "velocity_bits": body.velocity().map(|v| v.bits()),
                "orientation_bits": body.orientation().map(|v| v.bits()),
                "rotation_bits": body.rotation().map(|v| v.bits()),
            });
            assert_eq!(actual, row["bounce"], "{input}: whole native Bounce state");
        }
    }

    /// A stock-shaped debris chunk (`DBRIS1LG` numbers, `MaxXYVel` cut to 0.5
    /// so it lands in its own cell) launched over a flat arena beside a tank:
    /// it flies its body, then on touching down (`BounceClass::Update`
    /// reports Stopped on flat ground) constructs its `ExpireAnim=`, blasts
    /// the tank through `Apply_area_damage` with its HE `Damage=`, and is
    /// destroyed. Its Location follows the body every tick.
    #[test]
    fn a_debris_chunk_lands_and_blasts_what_is_under_it() {
        let art_ini = IniFile::from_str(
            "[CHUNK]\nBouncer=yes\nElasticity=0.0\nMaxXYVel=0.5\nMinZVel=25.0\n\
             ExpireAnim=BOOM\nDamage=20\nDamageRadius=80\nWarhead=HE\n\
             LoopEnd=15\nLoopCount=-1\nRandomRate=220,600\n\
             [BOOM]\nRate=900\n",
        );
        // The ordered Rules processor reads CHUNK's `ExpireAnim=` and
        // `Warhead=` with its ART body.
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str(
                "[General]\nDamageFireTypes=\n\
                 [AudioVisual]\nConditionYellow=50%\nConditionRed=25%\n\
                 [InfantryTypes]\n[VehicleTypes]\n0=TANK\n[AircraftTypes]\n[BuildingTypes]\n\
                 [Animations]\n0=CHUNK\n1=BOOM\n\
                 [TANK]\nStrength=300\nArmor=heavy\nSpeed=4\n\
                 [Warheads]\n0=HE\n\
                 [HE]\nCellSpread=.5\nPercentAtMax=.5\n\
                 Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
            ),
            &art_ini,
        )
        .unwrap();
        let mut art = ArtRegistry::from_ini(&art_ini);
        art.bind_anim_frame_count_for_test("CHUNK", 30);
        art.bind_anim_frame_count_for_test("BOOM", 17);
        rules.install_art_fixture(art);
        let mut sim = Simulation::with_seed(3);
        let americans = sim.interner.intern("Americans");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                americans, 0, None, true, 0, 10,
            ));
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
            sim.session.set_binary_frame_for_test(frame);
            sim.visit_anim(
                chunk,
                &rules,
                None,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
            let anim = sim.anim(chunk).unwrap();
            let body = anim.bounce.unwrap().position_leptons();
            assert_eq!(
                [anim.world_coord.x, anim.world_coord.y, anim.world_coord.z],
                [body.x, body.y, body.z]
            );
            if sim.substrate.pending_delete.contains_object(&chunk) {
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
        let health = sim.substrate.entities.get(tank).unwrap().health.current();
        assert!(
            (280..300).contains(&health),
            "HE splash of 20 at most, got {health}"
        );
        assert_eq!(sim.combat_light_requests.len(), 1);
    }

    /// Phobos `AnimExt::HandleDebrisImpact` (`Ext/Anim/Body.cpp:245-307`) with
    /// `Warhead.Detonate=yes` (`Ext/AnimType/Body.cpp:100`): the landing
    /// routes the warhead through `WarheadTypeExt::DetonateAt`, a bullet
    /// detonation, instead of `MapClass::DamageArea`. A warhead with
    /// `DetonateOnAllMapObjects=yes` therefore reaches a tank parked away
    /// from the impact only under the key.
    #[test]
    fn the_warhead_detonate_key_routes_the_landing_through_detonate_at() {
        for (key, far_damaged) in [
            (
                "Warhead.Detonate=yes
",
                true,
            ),
            (
                "Warhead.Detonate=no
",
                false,
            ),
            ("", false),
        ] {
            let art_ini = IniFile::from_str(&format!(
                "[CHUNK]
Bouncer=yes
Elasticity=0.0
MaxXYVel=0.5
MinZVel=25.0
ExpireAnim=BOOM
Damage=20
DamageRadius=80
Warhead=HE
{key}LoopEnd=15
LoopCount=-1
RandomRate=220,600
[BOOM]
Rate=900
"
            ));
            let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
                &IniFile::from_str(
                    "[General]
DamageFireTypes=
[AudioVisual]
ConditionYellow=50%
ConditionRed=25%
[InfantryTypes]
[VehicleTypes]
0=TANK
[AircraftTypes]
[BuildingTypes]
[Animations]
0=CHUNK
1=BOOM
[TANK]
Strength=300
Armor=heavy
Speed=4
[Warheads]
0=HE
[HE]
CellSpread=.5
PercentAtMax=.5
DetonateOnAllMapObjects=yes
DetonateOnAllMapObjects.AffectsTarget=all
DetonateOnAllMapObjects.AffectsHouse=all
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
",
                ),
                &art_ini,
            )
            .unwrap();
            let he = rules.warhead("HE").expect("HE");
            assert!(
                he.detonate_on_all_map.claims_detonation(),
                "{key:?}: DOAM claims, target {:?}",
                he.detonate_on_all_map.affects_target
            );
            let mut art = ArtRegistry::from_ini(&art_ini);
            art.bind_anim_frame_count_for_test("CHUNK", 30);
            art.bind_anim_frame_count_for_test("BOOM", 17);
            rules.install_art_fixture(art);
            assert_eq!(
                rules
                    .art()
                    .anim_runtime_config("CHUNK")
                    .expect("CHUNK config")
                    .phobos_warhead_detonate,
                key.contains("=yes"),
                "{key:?}: the ART key"
            );
            let mut sim = Simulation::with_seed(3);
            let americans = sim.interner.intern("Americans");
            sim.houses
                .register(crate::sim::house_state::HouseState::new(
                    americans, 0, None, true, 0, 10,
                ));
            crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
            let near = sim
                .spawn_object_at_height("TANK", "Americans", 8, 8, 0, 0, &rules)
                .unwrap();
            let far = sim
                .spawn_object_at_height("TANK", "Americans", 1, 1, 0, 0, &rules)
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
            for frame in 1..200 {
                sim.session.set_binary_frame_for_test(frame);
                sim.visit_anim(
                    chunk,
                    &rules,
                    None,
                    crate::sim::world::FrameEffects::default(),
                )
                .unwrap();
                if sim.substrate.pending_delete.contains_object(&chunk) {
                    break;
                }
            }
            assert!(
                sim.substrate.pending_delete.contains_object(&chunk),
                "{key:?}: the chunk lands"
            );
            let near_health = sim.substrate.entities.get(near).unwrap().health.current();
            let far_health = sim.substrate.entities.get(far).unwrap().health.current();
            assert!(
                near_health < 300,
                "{key:?}: the landing blasts the near tank"
            );
            assert_eq!(
                far_health < 300,
                far_damaged,
                "{key:?}: the far tank at {far_health}"
            );
        }
    }

    /// A landing warhead applies with no `ExpireAnim=` at all.
    ///
    /// Phobos `AnimExt::HandleDebrisImpact` (`Ext/Anim/Body.cpp:253-271`)
    /// tests `pWarhead` before, and independently of, `pExpireAnim`; the stock
    /// block at `0x423DE7` nests both under the ExpireAnim slot. Both key
    /// states must damage the tank at the impact, while only
    /// `Warhead.Detonate=yes` reaches a tank away from it.
    #[test]
    fn a_warhead_applies_without_an_expire_anim() {
        for (key, far_damaged) in [("Warhead.Detonate=yes\n", true), ("", false)] {
            let art_ini = IniFile::from_str(&format!(
                "[CHUNK]
Bouncer=yes
Elasticity=0.0
MaxXYVel=0.5
MinZVel=25.0
Damage=20
DamageRadius=80
Warhead=HE
{key}LoopEnd=15
LoopCount=-1
RandomRate=220,600
"
            ));
            let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
                &IniFile::from_str(
                    "[General]
DamageFireTypes=
[AudioVisual]
ConditionYellow=50%
ConditionRed=25%
[InfantryTypes]
[VehicleTypes]
0=TANK
[AircraftTypes]
[BuildingTypes]
[Animations]
0=CHUNK
[TANK]
Strength=300
Armor=heavy
Speed=4
[Warheads]
0=HE
[HE]
CellSpread=.5
PercentAtMax=.5
DetonateOnAllMapObjects=yes
DetonateOnAllMapObjects.AffectsTarget=all
DetonateOnAllMapObjects.AffectsHouse=all
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
",
                ),
                &art_ini,
            )
            .unwrap();
            let he = rules.warhead("HE").expect("HE");
            assert!(he.detonate_on_all_map.claims_detonation());
            let mut art = ArtRegistry::from_ini(&art_ini);
            art.bind_anim_frame_count_for_test("CHUNK", 30);
            rules.install_art_fixture(art);
            let config = rules.art().anim_runtime_config("CHUNK").expect("CHUNK");
            assert_eq!(config.phobos_warhead_detonate, key.contains("=yes"));
            assert!(config.expire_anim.is_none(), "{key:?}: no ExpireAnim");
            let mut sim = Simulation::with_seed(3);
            let americans = sim.interner.intern("Americans");
            sim.houses
                .register(crate::sim::house_state::HouseState::new(
                    americans, 0, None, true, 0, 10,
                ));
            crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
            let near = sim
                .spawn_object_at_height("TANK", "Americans", 8, 8, 0, 0, &rules)
                .unwrap();
            let far = sim
                .spawn_object_at_height("TANK", "Americans", 1, 1, 0, 0, &rules)
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
            for frame in 1..200 {
                sim.session.set_binary_frame_for_test(frame);
                sim.visit_anim(
                    chunk,
                    &rules,
                    None,
                    crate::sim::world::FrameEffects::default(),
                )
                .unwrap();
                if sim.substrate.pending_delete.contains_object(&chunk) {
                    break;
                }
            }
            assert!(
                sim.substrate.pending_delete.contains_object(&chunk),
                "{key:?}: the chunk lands"
            );
            let near_health = sim.substrate.entities.get(near).unwrap().health.current();
            let far_health = sim.substrate.entities.get(far).unwrap().health.current();
            assert!(
                near_health < 300,
                "{key:?}: the landing warhead applies without an ExpireAnim"
            );
            assert_eq!(
                far_health < 300,
                far_damaged,
                "{key:?}: the far tank at {far_health}"
            );
        }
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
                terrain.cell_mut_for_test(rx, ry).unwrap().accepts_smudge = true;
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

    /// Phobos `AnimExt::SpawnFireAnims` (`Ext/Anim/Body.cpp:309-405`, hook
    /// `0x425060`): a completing `Scorch=` anim runs `SmallFireCount` iterations
    /// over its `SmallFireAnims=` vector, and `SmallFireChances=0` keeps every
    /// iteration's anim unspawned while still drawing.
    #[test]
    fn a_scorch_anim_spawns_its_small_fire_anims_when_it_completes() {
        fn spawned_fire_anims(count: i32, chances: &str) -> usize {
            let art = format!(
                "[MARK]\nRate=900\nScorch=yes\nSmallFireCount={count}\n\
                 SmallFireAnims=FIREFX\nSmallFireChances={chances}\nSmallFireDistances=0\n\
                 [FIREFX]\nRate=900\n"
            );
            let (mut rules, mut sim) = middle_world(&art, 13);
            // The spawned child needs a bound frame count, else
            // `spawn_anim_at_world` refuses to construct it.
            let mut art_registry = ArtRegistry::from_ini(&IniFile::from_str(&art));
            art_registry.bind_anim_frame_count_for_test("MARK", 13);
            art_registry.bind_anim_frame_count_for_test("FIREFX", 3);
            rules.install_art_fixture(art_registry);
            let mark = spawn_mark(&mut sim, &rules, 0);
            for frame in 1..60 {
                sim.session.set_binary_frame_for_test(frame);
                sim.visit_anim(
                    mark,
                    &rules,
                    None,
                    crate::sim::world::FrameEffects::default(),
                )
                .unwrap();
                if sim.substrate.pending_delete.contains_object(&mark) {
                    break;
                }
            }
            let mut spawned = 0;
            for (_, anim) in sim.anims() {
                if sim
                    .interner
                    .resolve(anim.type_id)
                    .eq_ignore_ascii_case("FIREFX")
                {
                    spawned += 1;
                }
            }
            spawned
        }

        // `SmallFireDistances=0` keeps every spawn at the scorch's own point,
        // so the only draws are the chance and the constructor loop count.
        assert_eq!(spawned_fire_anims(2, "1,1"), 2);
        assert_eq!(spawned_fire_anims(2, "0,0"), 0);
        assert_eq!(spawned_fire_anims(3, "1"), 3);
    }

    /// A `Flamer=` type runs the small loop `1 + Flamer` times (here the
    /// authored `SmallFireCount=1`) and then the large loop
    /// `LargeFireCount` times (`Ext/Anim/Body.cpp:391-399`).
    #[test]
    fn a_flamer_anim_runs_both_its_fire_loops() {
        let art = "[MARK]\nRate=900\nFlamer=yes\nSmallFireCount=1\n\
                   SmallFireAnims=FIREFX\nSmallFireChances=1\nSmallFireDistances=0\n\
                   LargeFireCount=2\nLargeFireAnims=FIRELG\nLargeFireChances=1,1\n\
                   LargeFireDistances=0\n\
                   [FIREFX]\nRate=900\n[FIRELG]\nRate=900\n";
        let (mut rules, mut sim) = middle_world(art, 13);
        let mut art_registry = ArtRegistry::from_ini(&IniFile::from_str(art));
        for (name, frames) in [("MARK", 13), ("FIREFX", 3), ("FIRELG", 3)] {
            art_registry.bind_anim_frame_count_for_test(name, frames);
        }
        rules.install_art_fixture(art_registry);
        let mark = spawn_mark(&mut sim, &rules, 0);
        for frame in 1..60 {
            sim.session.set_binary_frame_for_test(frame);
            sim.visit_anim(
                mark,
                &rules,
                None,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
            if sim.substrate.pending_delete.contains_object(&mark) {
                break;
            }
        }
        let mut small = 0;
        let mut large = 0;
        for (_, anim) in sim.anims() {
            let name = sim.interner.resolve(anim.type_id);
            if name.eq_ignore_ascii_case("FIREFX") {
                small += 1;
            } else if name.eq_ignore_ascii_case("FIRELG") {
                large += 1;
            }
        }
        assert_eq!((small, large), (1, 2));
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
            sim.session.set_binary_frame_for_test(frame);
            sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
                .unwrap();
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

    /// Phobos `Crater.DestroyTiberium=` (`Ext/Anim/Hooks.cpp:707`, hook
    /// `0x4250E1`) resolved at Middle through the AnimType ART key and the
    /// `[General] AnimCraterDestroyTiberium=` fallback: false leaves the
    /// cell's ore untouched, true still reduces it by 6.
    #[test]
    fn phobos_crater_destroy_tiberium_controls_the_middle_frame_ore_reduction() {
        use crate::sim::overlay_grid::OverlayGrid;

        for (art_key, global, density, expected_data) in [
            ("Crater.DestroyTiberium=no\n", true, 9, 9),
            ("", false, 9, 9),
            ("Crater.DestroyTiberium=yes\n", false, 9, 3),
            ("", true, 9, 3),
            // 6 covers or exceeds the density, so these rows exercise the
            // clear plus neighbour reseed whose RNG the skipped call owns.
            ("Crater.DestroyTiberium=no\n", true, 4, 4),
            ("Crater.DestroyTiberium=yes\n", false, 4, 0),
        ] {
            let ini = IniFile::from_str(&format!(
                "[General]\nAnimCraterDestroyTiberium={}\n\
                 [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n{}",
                if global { "yes" } else { "no" },
                crate::sim::tiberium::test_support::tiberium_rules_text(),
            ));
            let mut rules = RuleSet::from_ini(&ini).expect("rules");
            let mut art = ArtRegistry::from_ini(&IniFile::from_str(&format!(
                "[MARK]\nRate=900\nCrater=yes\n{art_key}"
            )));
            art.bind_anim_frame_count_for_test("MARK", 13);
            rules.replace_art_registry_for_test(art);
            rules.smudge_types =
                crate::rules::smudge_type::SmudgeTypeRegistry::from_rules_ini(&IniFile::from_str(
                    "[SmudgeTypes]\n0=CR1\n\
                     [CR1]\nCrater=yes\nWidth=1\nHeight=1\n",
                ));
            let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);

            let mut sim = Simulation::new();
            let mut terrain = crate::map::resolved_terrain::test_flat_ground_grid(8);
            for ry in 0..8 {
                for rx in 0..8 {
                    terrain.cell_mut_for_test(rx, ry).unwrap().accepts_smudge = true;
                }
            }
            sim.resolved_terrain = Some(terrain);
            let mut grid = OverlayGrid::new(8, 8);
            grid.place_overlay(4, 4, overlays.id_for_name("TIB01").unwrap(), density);
            sim.overlay_grid = Some(grid);
            sim.smudge_grid = Some(crate::sim::smudge_grid::SmudgeGrid::new(8, 8));

            let id = spawn_mark(&mut sim, &rules, 0);
            for frame in 1..40 {
                sim.session.set_binary_frame_for_test(frame);
                sim.visit_anim(
                    id,
                    &rules,
                    Some(&overlays),
                    crate::sim::world::FrameEffects::default(),
                )
                .unwrap();
            }
            assert_eq!(
                sim.overlay_grid.as_ref().unwrap().cell(4, 4).overlay_data,
                expected_data,
                "art_key={art_key:?} global={global} density={density}"
            );
        }
    }

    /// `AnimClass::Middle 0x00425057`: an anim 30 leptons or more above the
    /// ground leaves no mark; 29 still marks.
    #[test]
    fn an_anim_thirty_leptons_up_leaves_no_mark() {
        for (world_z, expected) in [(30, 0), (29, 1)] {
            let (rules, mut sim) = middle_world("[MARK]\nRate=900\nScorch=yes\n", 13);
            let id = spawn_mark(&mut sim, &rules, world_z);
            for frame in 1..40 {
                sim.session.set_binary_frame_for_test(frame);
                sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
                    .unwrap();
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

        sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
            .unwrap(); // constructor first-AI guard at frame 0
        sim.session.set_binary_frame_for_test(1);
        sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
            .unwrap(); // delay 1 -> 0
        assert_eq!(sim.anim(id).unwrap().runtime.current_frame, 0);
        sim.session.set_binary_frame_for_test(2);
        sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
            .unwrap(); // constructor-anchored timer is already due

        assert_eq!(sim.anim(id).unwrap().runtime.current_frame, 1);
        sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
            .unwrap(); // a second visit in the same frame cannot advance again
        assert_eq!(sim.anim(id).unwrap().runtime.current_frame, 1);
        assert_eq!(sim.scenario_rng.logical_state(), rng_before);
    }

    /// An anim waiting on its frame timer ends its visit at the gates without
    /// being handed out, so nothing re-derives from it; the visit that
    /// advances its frame hands it out.
    #[test]
    fn a_waiting_anims_visit_hands_it_out_to_no_one() {
        let rules = runtime_rules("[TEST]\nRate=450\nEnd=3\nLoopCount=1\n", &[("TEST", 3)]);
        let mut sim = Simulation::new();
        let type_id = sim.interner.intern("TEST");
        let id = sim
            .spawn_anim_object(&rules, runtime_descriptor(type_id, 1))
            .unwrap();
        let visit = |sim: &mut Simulation, frame: u32| {
            sim.session.set_binary_frame_for_test(frame);
            sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
                .unwrap();
            sim.substrate
                .anims
                .take_touched(AnimTouchReader::ExpiryListeners)
        };
        visit(&mut sim, 0); // constructor first-AI guard
        visit(&mut sim, 1); // delay 1 -> 0
        assert_eq!(visit(&mut sim, 2), Touched::Ids(vec![id]));
        assert_eq!(sim.anim(id).unwrap().runtime.current_frame, 1);
        // The timer the last visit restarted is not due in the same frame.
        assert_eq!(visit(&mut sim, 2), Touched::Ids(Vec::new()));
        assert_eq!(sim.anim(id).unwrap().runtime.current_frame, 1);
    }

    #[test]
    fn stock_naweap_a_rate_normalizes_to_six_frames_at_speed_one() {
        let rules = runtime_rules(
            "[NAWEAP_A]\nRate=200\nNormalized=yes\nEnd=12\nLoopCount=-1\n",
            &[("NAWEAP_A", 12)],
        );
        let mut sim = Simulation::new();
        sim.session.game_options_mut_for_test().game_speed = 1;
        let type_id = sim.interner.intern("NAWEAP_A");
        let id = sim
            .spawn_anim_object(&rules, runtime_descriptor(type_id, 0))
            .unwrap();

        assert_eq!(sim.anim(id).unwrap().runtime.rate_reload, 6);
        sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
            .unwrap();
        sim.session.set_binary_frame_for_test(5);
        sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
            .unwrap();
        assert_eq!(sim.anim(id).unwrap().runtime.current_frame, 0);
        sim.session.set_binary_frame_for_test(6);
        sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
            .unwrap();
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

        sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
            .unwrap(); // guard
        sim.session.set_binary_frame_for_test(1);
        sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
            .unwrap(); // frame 1
        sim.session.set_binary_frame_for_test(2);
        sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
            .unwrap(); // frame 2 -> SECOND in place + Middle
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

        sim.session.set_binary_frame_for_test(3);
        sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
            .unwrap(); // SECOND frame 1 (Next does not restore guard)
        sim.session.set_binary_frame_for_test(4);
        sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
            .unwrap(); // SECOND frame 2 -> destroy
        sim.destroy_anim(id, &rules);
        assert!(!sim.anim(id).unwrap().runtime.inactive);
        assert!(sim.substrate.pending_delete.contains_object(&id));
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
        sim.sound_events.clear_for_test();
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
        assert_eq!(sim.substrate.pending_delete.object_ids(), vec![id]);
        assert!(!sim.anim(id).unwrap().in_logic_vector);
        assert_eq!(sim.display_layers().layer_of(id), None);
        assert!(!sim.reveal_anim(id, &rules, true));
        sim.process_pending_delete();
        assert!(sim.anim(id).is_none());
    }

    #[test]
    fn trailer_without_preintern_is_visited_and_guarded_in_same_live_walk() {
        let rules = runtime_rules(
            // Sticky= keeps CHILD an ordinary anim (`ArtRegistry::anim_presentation_only`).
            "[PARENT]\nRate=0\nEnd=2\nTrailerAnim=CHILD\nTrailerSeperation=1\n\n\
             [CHILD]\nRate=900\nEnd=2\nLoopCount=1\nSticky=yes\n",
            &[("PARENT", 2), ("CHILD", 2)],
        );
        let mut sim = Simulation::new();
        let parent_type = sim.interner.intern("PARENT");
        assert!(sim.interner.get("CHILD").is_none());
        let parent = sim
            .spawn_anim_object(&rules, runtime_descriptor(parent_type, 0))
            .unwrap();

        sim.for_each_live_object(|sim, id| {
            sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
                .unwrap();
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
            .set_for_test(50);
        sim.update_building_damage_fire(building_id, &rules);
        let anim_id = sim
            .substrate
            .entities
            .get(building_id)
            .unwrap()
            .damage_fires
            .anim_ids[0]
            .unwrap();
        sim.set_anim_owner_object(anim_id, Some(building_id), &rules);
        assert_eq!(sim.anim(anim_id).unwrap().owner_entity, Some(building_id));

        assert!(sim.anim_pointer_expired(
            anim_id,
            building_id,
            crate::rules::ruleset::RuleSet::empty_for_test()
        ));
        assert_eq!(
            sim.substrate
                .entities
                .get(building_id)
                .unwrap()
                .damage_fires
                .anim_ids[0],
            Some(anim_id),
            "native owner expiry retains the slot until the Anim's own expiry"
        );
        assert!(sim.anim(anim_id).unwrap().runtime.inactive);
        assert!(sim.live_object_order_snapshot().contains(&anim_id));
        assert!(!sim.substrate.pending_delete.contains_object(&anim_id));

        sim.visit_anim(
            anim_id,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
        assert!(
            sim.entities()
                .get(building_id)
                .unwrap()
                .damage_fires
                .anim_ids[0]
                .is_none()
        );
        assert!(!sim.live_object_order_snapshot().contains(&anim_id));
        assert_eq!(sim.substrate.pending_delete.object_ids(), vec![anim_id]);
    }

    #[test]
    fn building_limbo_uninits_its_damage_fire_anims() {
        let (mut sim, rules, building_id) = damage_fire_fixture(false);
        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .health
            .set_for_test(50);
        sim.update_building_damage_fire(building_id, &rules);
        let fires = sim
            .substrate
            .entities
            .get(building_id)
            .unwrap()
            .damage_fires
            .anim_ids;
        let anim_id = fires[0].expect("slot zero");

        // The building is alive: only Building Limbo 4458E4 retires the fires.
        sim.techno_limbo_with_rules(building_id, &rules, None);

        let building = sim.substrate.entities.get(building_id).unwrap();
        assert!(building.is_object_alive());
        assert!(building.damage_fires.anim_ids.iter().all(Option::is_none));
        assert!(
            building.damage_fires.active,
            "Limbo leaves Building+5E8 as it was"
        );
        assert!(!sim.live_object_order_snapshot().contains(&anim_id));
        assert!(sim.substrate.pending_delete.contains_object(&anim_id));
    }

    #[test]
    fn pointer_expiry_runs_owner_and_attached_bullet_arms_independently() {
        let (mut sim, rules, owner) = damage_fire_fixture(false);
        let fire_type = sim.interner.get("FIRE01").unwrap();
        let anim = sim
            .spawn_anim_object(&rules, runtime_descriptor(fire_type, 0))
            .unwrap();
        sim.set_anim_owner_object(anim, Some(owner), &rules);
        sim.substrate.set_anim_attached_bullet(anim, Some(owner));
        // Pointer fields are independently compared at425164 and4251AD;
        // clearing the owner must not skip attached-Bullet UnInit.
        assert!(sim.anim_pointer_expired(anim, owner, &rules));
        let object = sim.anim(anim).unwrap();
        assert_eq!(object.owner_entity, None);
        assert_eq!(object.attached_bullet(), None);
        assert!(object.runtime.inactive);
        assert!(!object.in_logic_vector);
        assert!(sim.substrate.pending_delete.contains_object(&anim));
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
        sim.substrate
            .anims
            .set_owner_entity_for_test(anim_id, Some(building_id));
        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .damage_fires
            .anim_ids[0] = Some(anim_id);
        sim.substrate.rebuild_building_anim_slot_indices();
        // Supply the common post-UnInit retirement state through its existing
        // membership owner;19B remains the independent native expiry byte.
        sim.conceal_anim(anim_id);
        sim.substrate.pending_delete.queue_object(anim_id);

        sim.process_pending_delete();

        assert!(sim.anim(anim_id).is_none());
        assert!(!sim.live_object_order_snapshot().contains(&anim_id));
        assert!(
            sim.substrate
                .entities
                .get(building_id)
                .unwrap()
                .damage_fires
                .anim_ids[0]
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
            .set_for_test(50);
        sim.update_building_damage_fire(building_id, &rules);
        let anim_id = sim
            .substrate
            .entities
            .get(building_id)
            .unwrap()
            .damage_fires
            .anim_ids[0]
            .expect("slot zero");
        sim.set_anim_owner_object(anim_id, Some(building_id), &rules);
        let before = sim.anim_absolute_coord(anim_id).expect("attached anim");
        let stored_before = sim.anim(anim_id).unwrap().world_coord;

        // Move the owner one cell east and one cell south.
        {
            let owner = sim.substrate.entities.get_mut(building_id).unwrap();
            owner.position.set_rx_for_test(owner.position.rx() + 1);
            owner.position.set_ry_for_test(owner.position.ry() + 1);
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
            .set_for_test(50);
        sim.update_building_damage_fire(building_id, &rules);
        let anim_id = sim
            .substrate
            .entities
            .get(building_id)
            .unwrap()
            .damage_fires
            .anim_ids[0]
            .expect("slot zero");
        sim.set_anim_owner_object(anim_id, Some(building_id), &rules);
        let before = sim.anim_absolute_coord(anim_id).expect("attached anim");

        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .position
            .set_exact_z_leptons_for_test(Some(before.z + 250));

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
            .set_for_test(50);
        let mut expected_rng = sim.scenario_rng.clone();
        let start_type = expected_rng.next_range_u32_inclusive(0, 2) as usize;
        let type_names = ["FIRE01", "FIRE02", "FIRE03"];
        let frame_counts = [30_u32, 64, 30];
        let expected_types = [start_type, (start_type + 1) % type_names.len()];
        let expected_frames = expected_types
            .map(|index| expected_rng.next_range_u32_inclusive(0, frame_counts[index] - 1) as i32);
        sim.update_building_damage_fire(building_id, &rules);

        let building = sim.substrate.entities.get(building_id).unwrap();
        assert!(building.damage_fires.active);
        let first = building.damage_fires.anim_ids[0].expect("slot zero");
        let second = building.damage_fires.anim_ids[1].expect("slot one");
        assert!(
            building.damage_fires.anim_ids[2..]
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
            .set_for_test(50);
        sim.update_building_damage_fire(building_id, &rules);
        let rng_after_spawn = sim.scenario_rng.logical_state();
        let ids = sim
            .substrate
            .entities
            .get(building_id)
            .unwrap()
            .damage_fires
            .anim_ids;

        sim.update_building_damage_fire(building_id, &rules);
        assert_eq!(sim.scenario_rng.logical_state(), rng_after_spawn);
        assert_eq!(
            sim.substrate
                .entities
                .get(building_id)
                .unwrap()
                .damage_fires
                .anim_ids,
            ids
        );

        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .health
            .set_for_test(51);
        sim.update_building_damage_fire(building_id, &rules);
        let building = sim.substrate.entities.get(building_id).unwrap();
        assert!(!building.damage_fires.active);
        assert!(building.damage_fires.anim_ids.iter().all(Option::is_none));
        assert_eq!(sim.live_object_order_snapshot(), vec![building_id]);
    }

    #[test]
    fn unspawnable_damage_fire_type_leaves_the_slots_empty() {
        let (mut sim, mut rules, building_id) = damage_fire_fixture(false);
        for fire in &mut rules.general.damage_fire_types {
            fire.name = "NOSUCHFIRE".to_string();
        }
        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .health
            .set_for_test(50);

        sim.update_building_damage_fire(building_id, &rules);

        let building = sim.substrate.entities.get(building_id).unwrap();
        assert!(building.damage_fires.active);
        assert!(building.damage_fires.anim_ids.iter().all(Option::is_none));
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
            .set_for_test(50);
        let rng_before = sim.scenario_rng.logical_state();

        sim.update_building_damage_fire(building_id, &rules);

        let building = sim.substrate.entities.get(building_id).unwrap();
        assert!(building.damage_fires.active);
        assert!(building.damage_fires.anim_ids.iter().all(Option::is_none));
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
                    owner: None,
                },
                AnimWorldCoord { x: 0, y: 0, z: 0 },
            )
            .unwrap();
        let building = sim.substrate.entities.get_mut(building_id).unwrap();
        building.damage_fires.anim_ids[0] = Some(occupied_id);
        building.health.set_for_test(50);
        let mut expected_rng = sim.scenario_rng.clone();
        let _ = expected_rng.next_range_u32_inclusive(0, 2);

        sim.update_building_damage_fire(building_id, &rules);

        let slots = sim
            .substrate
            .entities
            .get(building_id)
            .unwrap()
            .damage_fires
            .anim_ids;
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
            .set_for_test(50);
        sim.update_building_damage_fire(building_id, &rules);
        sim.sound_events.clear_for_test();
        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .health
            .set_for_test(100);

        sim.update_building_damage_fire(building_id, &rules);
        sim.update_building_damage_fire(building_id, &rules);

        let building = sim.substrate.entities.get(building_id).unwrap();
        assert!(!building.damage_fires.active);
        assert!(building.damage_fires.anim_ids.iter().all(Option::is_none));
        assert_eq!(
            sim.sound_events
                .iter()
                .filter(|event| matches!(event, SimSoundEvent::ObjectSoundReleased { .. }))
                .count(),
            2,
        );
    }

    #[test]
    fn anim_store_slots_scheduler_and_hash_roundtrip() {
        let (mut sim, rules, building_id) = damage_fire_fixture(false);
        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .health
            .set_for_test(50);
        sim.update_building_damage_fire(building_id, &rules);
        assert!(sim.substrate.pending_delete.is_empty());
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // AnimStore/scheduler persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();
        let expected_order = sim.live_object_order_snapshot();
        let bytes = crate::util::save_codec::serialize(&sim).expect("serialize sim with AnimStore");
        let mut restored: Simulation =
            crate::util::save_codec::deserialize(&bytes).expect("deserialize AnimStore");
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
                .damage_fires
                .anim_ids,
            sim.substrate
                .entities
                .get(building_id)
                .unwrap()
                .damage_fires
                .anim_ids,
        );
    }
    fn waypoint_cue_flare_scene() -> (Simulation, RuleSet, crate::sim::intern::InternedId) {
        // Custom DropZone binding from the original connected controls. ART and
        // the five-frame SHP metadata are physical ARRWSE inputs; no GPU claim.
        let mut rules=RuleSet::from_ini(&IniFile::from_str(
            "[General]\nDropZoneAnim=ARRWSE\n[AudioVisual]\nDropZoneRadius=4\n[Animations]\n0=ARRWSE\n[BuildingTypes]\n0=BLOCK\n[BLOCK]\nStrength=100\n[Clear]\nBuildable=yes\nFoot=100%\nTrack=100%\n",
        )).unwrap();
        let mut art = ArtRegistry::from_ini(&IniFile::from_str(
            "[ARRWSE]\nLoopCount=2\nLoopEnd=4\nRate=450\nDemandLoad=true\nLayer=Top\nZAdjust=-300\nNext=none\n",
        ));
        art.bind_anim_frame_count_for_test("ARRWSE", 5);
        art.set_anim_sounds_for_test("ARRWSE", Some("Explosion06"), Some("ExplosionShard"));
        rules.replace_art_registry_for_test(art);
        let map=crate::map::map_file::MapFile::from_bytes(
            b"[Map]\nTheater=TEMPERATE\nSize=0,0,16,16\nLocalSize=0,0,16,16\n[IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n[Waypoints]\n59=10010\n123=6005\n[Triggers]\nCUE=Player,<none>,Cue,0,1,1,1,0\n[Events]\nCUE=1,8,0,0\n[Actions]\nCUE=1,8,0,123,0,0,0,0,BH\n[Tags]\nCUETAG=0,Cue,CUE\n",
        ).unwrap();
        let mut sim = Simulation::with_seed(31);
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        // MapRevealArea rejects the arena's default Size20 diamond edge at
        // (10,10). Install this scene's Size16 through the normal owner.
        sim.install_playfield_from_map_header(&map.header);
        sim.fog = crate::sim::vision::FogState::new(32, 32);
        let player = sim.interner.intern("Player");
        let other = sim.interner.intern("Other");
        for (id, human) in [(player, true), (other, false)] {
            sim.houses
                .register(crate::sim::house_state::HouseState::new(
                    id, 0, None, human, 0, 10,
                ));
        }
        sim.session.set_current_house_for_test(Some(player));
        sim.session.set_binary_frame_for_test(1000);
        sim.session.install_waypoints(&map.waypoints);
        sim.initialize_trigger_definitions(
            &map,
            &crate::rules::team_ai_ini::TeamAiIniRegistry::default(),
            None,
            None,
            None,
        );
        sim.classify_map_triggers(&rules);
        (sim, rules, player)
    }

    fn waypoint_cue_spring_flare(sim: &mut Simulation, rules: &RuleSet) -> AnimId {
        sim.advance_triggers(rules, None, crate::sim::world::FrameEffects::default());
        let anim = sim
            .substrate
            .anims
            .iter()
            .map(|(_, anim)| anim)
            .next()
            .expect("live Tag8 creates the flare");
        anim.stable_id
    }

    #[test]
    fn waypoint_cue_flare_uses_tail_slot_reveals_current_house_and_keeps_first_start() {
        let (mut sim, rules, player) = waypoint_cue_flare_scene();
        assert!(!sim.fog.is_cell_revealed(player, 10, 10));
        let id = waypoint_cue_spring_flare(&mut sim, &rules);
        let anim = sim.anim(id).unwrap();
        assert_eq!(
            anim.world_coord,
            AnimWorldCoord {
                x: 2688,
                y: 2688,
                z: 0
            }
        );
        assert_eq!(anim.draw_flags, 0x600);
        assert_eq!(anim.z_adjust, -300);
        assert!(anim.no_effects());
        assert!(anim.runtime.first_ai_guard);
        assert!(sim.sound_events.iter().any(|event|matches!(event,SimSoundEvent::AnimationStarted{anim_id,sound_id,..} if *anim_id==id && sound_id=="EXPLOSION06")),"first Start precedes the action's NoEffects store");
        assert!(sim.fog.is_cell_revealed(player, 10, 10));
        assert!(!sim.fog.is_cell_revealed(player, 20, 20));
        let other = sim.interner.get("Other").unwrap();
        assert!(!sim.fog.is_cell_revealed(other, 10, 10));
        sim.sound_events.clear_for_test();
        sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
            .unwrap();
        assert!(!sim.anim(id).unwrap().runtime.first_ai_guard);
        assert!(
            sim.sound_events.is_empty(),
            "NoEffects suppresses later Start/Stop work"
        );
    }

    #[test]
    fn waypoint_cue_flare_ground_building_ends_before_first_guard_and_deck_does_not() {
        use crate::sim::movement::locomotor::MovementLayer;
        for layer in [MovementLayer::Ground, MovementLayer::Bridge] {
            let (mut sim, rules, player) = waypoint_cue_flare_scene();
            if layer == MovementLayer::Bridge {
                let terrain = sim.resolved_terrain.as_mut().unwrap();
                let cell = terrain.native_cell_identity((10, 10));
                terrain.write_native_cell_flags(cell, 0x100);
            }
            let type_ref = sim.interner.intern("BLOCK");
            let mut building = GameEntity::new_at_frame_zero_for_test(
                100,
                10,
                10,
                0,
                0,
                player,
                Health::new(100),
                type_ref,
                EntityCategory::Structure,
                0,
                5,
                false,
            );
            building.health.set(0); //Native Cell47C520 has no Health/Alive admission.
            sim.substrate.entities.insert(building);
            sim.set_next_stable_object_id_for_test(101);
            sim.substrate.occupancy.add(
                10,
                10,
                100,
                layer,
                None,
                crate::sim::occupancy::CellListInsertion::AppendBuilding,
            );
            let id = waypoint_cue_spring_flare(&mut sim, &rules);
            assert_eq!(
                sim.anim(id).unwrap().world_coord.z,
                if layer == MovementLayer::Bridge {
                    416
                } else {
                    0
                }
            );
            sim.sound_events.clear_for_test();
            let rng = sim.rng_state();
            sim.visit_anim(id, &rules, None, crate::sim::world::FrameEffects::default())
                .unwrap();
            let anim = sim.anim(id).unwrap();
            if layer == MovementLayer::Ground {
                assert!(anim.runtime.inactive);
                assert!(
                    anim.runtime.first_ai_guard,
                    "424358 ends the flare before consuming19C"
                );
                assert!(sim.substrate.pending_delete.contains_object(&id));
                assert!(!sim.sound_events.iter().any(|event|matches!(event,SimSoundEvent::VocAtCoord{sound_id,..} | SimSoundEvent::AnimationStarted{sound_id,..} if sound_id=="EXPLOSIONSHARD")));
                sim.process_pending_delete();
                assert!(sim.anim(id).is_none());
            } else {
                assert!(!anim.runtime.inactive);
                assert!(!anim.runtime.first_ai_guard);
                assert!(!sim.substrate.pending_delete.contains_object(&id));
            }
            assert_eq!(sim.rng_state(), rng);
        }
    }
}

#[cfg(all(test, tests_sim_a_b))]
#[path = "anim_move_flash_tests.rs"]
mod move_flash_tests;

#[cfg(all(test, any(tests_sim_a_b, tests_sim_trigger, tests_sim_world_t_z)))]
#[path = "anim_meteor_tests.rs"]
pub(in crate::sim) mod meteor_tests;

#[cfg(all(test, tests_sim_a_b))]
#[path = "anim_damage_tests.rs"]
mod damage_tests;

impl Simulation {
        /// Run the effect anims that change nothing but their picture as
        /// presentation anims from now on ([`AnimConstructorInput::Presentation`]).
        /// Simulations start without, in native order, so a comparison with
        /// gamemd's unique ids, Logic order and anim rows holds; real matches,
        /// replays of them and sim-bench turn it on.
        pub fn present_effect_anims(&mut self) {
            self.effect_anims.presentation = true;
        }
}

impl Simulation {
        /// Whether picture-only effect anims run as presentation anims
        /// ([`Self::present_effect_anims`]).
        pub(crate) fn effect_anims_presented(&self) -> bool {
            self.effect_anims.presentation
        }
}

impl Simulation {
        /// The constructor a muzzle flash, an impact or a trailer of type
        /// `type_id` takes: a presentation anim when the match runs them
        /// ([`Self::present_effect_anims`]) and the type changes nothing but
        /// its picture ([`crate::rules::art_data::ArtRegistry::anim_presentation_only`]),
        /// else an ordinary one. Only producers that keep no hold on the anim ask.
        pub(crate) fn effect_anim_constructor(
            &self,
            rules: &RuleSet,
            type_id: InternedId,
        ) -> AnimConstructorInput {
            if self.effect_anims_presented()
                && rules
                    .art()
                    .anim_presentation_only(self.interner.resolve(type_id))
            {
                AnimConstructorInput::Presentation
            } else {
                AnimConstructorInput::Runtime
            }
        }
}
