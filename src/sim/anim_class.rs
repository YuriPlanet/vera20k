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

#[cfg(test)]
use crate::sim::world::FrameEffects;
#[cfg(test)]
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::rules::overlay_types::TiberiumImageError;
#[cfg(test)]
use crate::rules::ruleset::RuleSet;
#[cfg(test)]
use crate::sim::bounce::BounceState;
#[cfg(test)]
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::{RawCellKey, RawCellOccupationGrid};
#[cfg(test)]
use crate::sim::touch_log::Touched;
#[cfg(test)]
use crate::sim::world::SimSoundEvent;
#[cfg(test)]
use crate::sim::world::Simulation;
#[cfg(test)]
use crate::util::fixed_math::SimFixed;
#[cfg(test)]
use crate::util::native_x87::NativeF64Bits;
use crate::util::native_x87::NativeX87Error;

pub type AnimId = u64;

mod ai;
mod bounce;
mod damage_fire;
mod flaming_guy;
mod lifecycle;
mod object;
mod spawn;
mod store;

#[cfg(test)]
pub(crate) use ai::ANIM_MIDDLE_MAX_HEIGHT;
pub(crate) use damage_fire::DamageFires;
#[cfg(test)]
pub(crate) use object::anim_own_sort_term;
pub(crate) use object::{AnimDisplayState, anim_display_sort_key, anim_world_coords};
pub use object::{AnimDrawRuntime, AnimObject, AnimRemap, AnimRuntime};
pub use spawn::AnimConstructorDraws;
pub(crate) use spawn::{AnimConstructorInput, anim_constructor_draws};
pub use store::AnimStore;
pub(crate) use store::{AnimTouchReader, EffectAnims, is_local_anim_id};

#[cfg(all(test, tests_sim_a_b))]
#[path = "anim_animated_tiberium_tests.rs"]
mod animated_tiberium_tests;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnimOccupationOperation {
    Mark,
    Clear,
}

/// One MakeInfantry anim's Mark or Clear on its cell's raw planes. At or above
/// the resolved deck, Mark takes the deck only while the cell keeps its bridge
/// flag. The native fallback uses Anim `[0x0089A1B4]`. Both write the anim's owner
/// index, -1 (vtable `+0x38` = `0x00410490`, stored at `0x004262D0` /
/// `0x004262F5`); Clear resets it once `0x1C` is clear (`0x0042634F` /
/// `0x00426378`).
fn apply_anim_raw_occupation(
    grid: &mut RawCellOccupationGrid,
    key: RawCellKey,
    mask: u8,
    world_z: i32,
    deck_z: i32,
    live_structural_bridge: bool,
    operation: AnimOccupationOperation,
) {
    let reaches_deck = world_z >= deck_z;
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

#[cfg(all(test, tests_sim_a_b))]
mod long_tail_contract_tests;

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
mod tests;

#[cfg(all(test, tests_sim_a_b))]
#[path = "anim_move_flash_tests.rs"]
mod move_flash_tests;

#[cfg(all(test, any(tests_sim_a_b, tests_sim_trigger, tests_sim_world_t_z)))]
#[path = "anim_meteor_tests.rs"]
pub(in crate::sim) mod meteor_tests;

#[cfg(all(test, tests_sim_a_b))]
#[path = "anim_damage_tests.rs"]
mod damage_tests;
