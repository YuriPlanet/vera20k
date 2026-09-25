//! Damage math over caller-built views: the shared warhead numeric receiver,
//! the surrounding receiver stages and FireAt's damage build.
//!
//! ## Dependency rules
//! - sim/ submodule: uses shared util arithmetic and caller-resolved rule inputs.
//! - NEVER depends on render/ui/sidebar/audio/net. No EntityStore/GameEntity
//!   reach-in: callers extract inputs into the value-types below.
//! - The kernel, the defence divides and the attacker's damage build use native
//!   PC53/chop, binary32 spills and signed64 conversion's low32 through
//!   util/native_x87. A host `f64 as i32` is not that conversion.
//!
//! Original `489180` kernel comparisons live in spatial_oracle/estimated_damage;
//! the defence divides and the damage build in spatial_oracle/damage_build;
//! shared hardware primitive comparisons live in spatial_oracle/x87_masked_hardware.
//! These bounded comparisons do not establish complete receiver/attacker parity.
//! See docs/research/SHARED_WARHEAD_NUMERIC_COMPARISON.md and local citations.
//!
//! Ordered Apply_area_damage records use the receiver service live. Legacy
//! direct/radiation routes still arrive as precomputed damage amounts.

use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};

pub(crate) mod attacker;
pub(crate) mod estimate;
pub(crate) mod gates;
pub(crate) mod kernel;
pub(crate) mod receive;

/// 0..=10 armor class index (none..special_2). Newtype over u8 to stop
/// raw-int confusion with Verses/percent values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ArmorClass(pub u8);

/// `TechnoClass::ReceiveDamage @ 0x00701900`'s defence divisors for one
/// target, gathered by the caller (`0x00701939..0x007019E3`, see
/// [`receive::defence_divides`]).
#[derive(Debug, Clone, Copy)]
pub(crate) struct DefenceDivisors {
    /// `HouseClass::GetArmorMultForType @ 0x0050BD30`: the owner's HouseType
    /// float for the target's category (`ArmorInfantryMult=`, `ArmorUnitsMult=`,
    /// `ArmorAircraftMult=`, `ArmorBuildingsMult=`, or `ArmorDefensesMult=` for
    /// a `BuildCat=Combat` building). Difficulty and country `Armor=` never
    /// reach it: `House+0x1A0` has no damage reader.
    pub house_type_armor: NativeF32Bits,
    /// `Techno+0x158`, the per-object ArmorMultiplier.
    pub unit_armor: NativeF64Bits,
    /// `Rules+0x688` `VeteranArmor=` when the target's rank holds STRONGER.
    pub rank_armor: Option<NativeF64Bits>,
}

impl Default for DefenceDivisors {
    fn default() -> Self {
        Self {
            house_type_armor: NativeF32Bits::from_bits(1.0_f32.to_bits()),
            unit_armor: NativeF64Bits::ONE,
            rank_armor: None,
        }
    }
}

/// Receiver-side gate inputs (warhead bools + target flags + ally relationship),
/// gathered by the caller. Evaluated in gamemd's verified order (TechnoClass::
/// ReceiveDamage 0x00701900): the armor divides run first, then these gates.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ImmunityInputs {
    /// Concrete receiver ABI flag. It skips Techno defender transforms and
    /// the Object damage kernel/Immune gate, while later warhead gates retain
    /// their independently verified behavior.
    pub ignore_defenses: bool,
    pub attacker_present: bool,
    /// type+0xc8c set AND same WhatAmI AND same owner.
    pub type_immune: bool,
    /// vtable+0x1d4 warping out.
    pub warping_out: bool,
    /// vtable+0x160 active IronCurtain/ForceShield, after the native
    /// positive-sign and `ignoreDefenses` admission checks.
    pub invulnerable: bool,
    /// Bunker/garrison link blocks the hit (target in bunker AND warhead does
    /// NOT PenetratesBunker). NOT a wall check.
    pub bunker_blocked: bool,
    /// Warhead Radiation && target ImmuneToRadiation.
    pub radiation_immune: bool,
    /// Warhead PsychicDamage && target immune.
    pub psychic_immune: bool,
    /// Warhead Poison && target immune.
    pub poison_immune: bool,
    /// Warhead AffectsAllies (warhead+0x179, default TRUE).
    pub affects_allies: bool,
    /// Attacker owner IsAlliedWith target owner (AffectsAllies operand).
    pub attacker_is_allied: bool,
    /// Target owner IsAlliedWith sourceHouse (Psychedelic operand).
    pub source_house_is_allied: bool,
    /// Warhead Psychedelic/MindControl (warhead+0x16d).
    pub psychedelic: bool,
    /// Target ImmuneToPsionics.
    pub psionics_immune: bool,
    pub target_is_building: bool,
}

/// Caller-built target view — decouples the service from GameEntity.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TargetDamageView {
    pub armor: ArmorClass,
    pub current_hp: i32,
    /// ObjectType `Immune=` entry gate in ObjectClass::ReceiveDamage.
    pub object_immune: bool,
}

/// What the receiver-side gates decide before the kernel runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DamageGate {
    Pass,
    /// Short-circuit to 0 HP delta, no state change.
    Nullified,
    /// Active IronCurtain/ForceShield short-circuit. Kept distinct because
    /// TechnoClass emits its transient combat-light before returning.
    Invulnerable,
    /// 0 HP delta, return-code-1 marker (damaged, no HP) — mind control.
    MindControlled,
}

/// Health-state classification returned by the receiver pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DamageState {
    Unaffected,
    Damaged,
    Yellow,
    Red,
    Dead,
    /// ObjectAlive was already false after the positive HP write (native5).
    AlreadyDead,
}

/// Prepared receiver packet, before the Object HP transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DamageOutcome {
    /// Object entry admitted the packet before its kernel (which may yield0).
    pub apply_object_damage: bool,
    /// Prepared signed kernel result; Object owns the final cap and HP write.
    pub hp_delta: i32,
    /// Packet retained when Object entry is skipped (for example Immune).
    /// Accepted commits replace it with their actual mutated packet for anger.
    pub post_object_damage: Option<i32>,
    /// Accepted `Psychedelic=yes` receiver value. TechnoClass stores this
    /// signed distance-zero kernel result as its berserk timer and returns
    /// before ObjectClass mutates HP.
    pub psychedelic_value: Option<i32>,
    /// Exact ECX argument passed to `FUN_0048A620` for an IC/FS block:
    /// post-defender-transform damage shifted left once with x86 wrapping.
    pub invulnerability_impact_damage: Option<i32>,
    /// True iff the concrete Techno receiver delegated to ObjectClass and
    /// returned through TechnoClass's surviving-object postlude. The native
    /// hostile-hit latch lives in that postlude, so a zero HP delta can still
    /// be observable while an early Techno gate or Psychedelic return cannot.
    pub reached_survivor_postlude: bool,
}
