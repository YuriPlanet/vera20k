//! Damage math over caller-built views: the shared warhead numeric receiver,
//! surrounding receiver stages and staged attacker transforms.
//!
//! ## Dependency rules
//! - sim/ submodule: uses shared util arithmetic and caller-resolved rule inputs.
//! - NEVER depends on render/ui/sidebar/audio/net. No EntityStore/GameEntity
//!   reach-in: callers extract inputs into the value-types below.
//! - The kernel uses native PC53/chop, binary32 spills and signed64 conversion's
//!   low32 through util/native_x87. A host `f64 as i32` is not that conversion.
//!   Surrounding defense/attacker stages still contain unmigrated host arithmetic.
//!
//! Original `489180` kernel comparisons live in spatial_oracle/estimated_damage;
//! shared hardware primitive comparisons live in spatial_oracle/x87_masked_hardware.
//! These bounded comparisons do not establish complete receiver/attacker parity.
//! See docs/research/SHARED_WARHEAD_NUMERIC_COMPARISON.md and local citations.
//!
//! Ordered Apply_area_damage records use the receiver service live. Legacy
//! direct/radiation routes still arrive as precomputed damage amounts.

pub(crate) mod attacker;
pub(crate) mod gates;
pub(crate) mod kernel;
pub(crate) mod receive;

/// 0..=10 armor class index (none..special_2). Newtype over u8 to stop
/// raw-int confusion with Verses/percent values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ArmorClass(pub u8);

/// Attacker (Fire_At) + defender (ReceiveDamage) modifiers, gathered by the
/// caller. All default 1.0 => no-op. Carried as f64 because gamemd applies each
/// as a double multiply/divide with an ftol truncation per stage.
///
/// Attacker stages are gamemd's verified Fire_At chain (not the pre-plan
/// guess): FirePower fold -> VeteranCombat -> Occupy -> TankBunker -> OpenTopped.
/// Each stage is gated in gamemd by a *condition flag*; the caller resolves the
/// flag into either the rules mult (stage active) or 1.0 (stage inactive), so
/// `fire_damage` can multiply unconditionally (ftol(d*1.0) == d).
#[derive(Debug, Clone, Copy)]
pub(crate) struct CombatMods {
    // --- Attacker side (Fire_At), folded/truncated in this order ---
    // These verified inputs remain staged until the authoritative fire path
    // adopts `attacker::fire_damage`; production currently reads only defender fields.
    /// Country FirePower mult (House+0x188).
    #[allow(dead_code)]
    pub attacker_country_firepower: f64,
    /// Per-unit Firepower mult (Techno+0x160); folded with country + base damage
    /// into ONE ftol stage.
    #[allow(dead_code)]
    pub attacker_unit_firepower: f64,
    /// VeteranCombat (Rules+0x670, ~1.1, double) when the attacker has the
    /// firepower vet/elite ability, else 1.0.
    #[allow(dead_code)]
    pub attacker_vet_combat: f64,
    /// Occupy/garrison damage mult (Rules+0xf40, float) when the attacker is an
    /// occupant firing from a garrisonable building, else 1.0.
    #[allow(dead_code)]
    pub attacker_occupy: f64,
    /// Tank-bunker mult (Rules+0xf4c) when the attacker is a tank-bunker
    /// occupant (this+0x2e4 link, non-building), else 1.0.
    #[allow(dead_code)]
    pub attacker_tank_bunker: f64,
    /// Open-topped transport mult (Rules+0xf58) when the attacker fires from an
    /// OpenTopped transport (this+0x82), else 1.0.
    #[allow(dead_code)]
    pub attacker_open_topped: f64,

    // --- Defender side (ReceiveDamage) — DIVIDE, each ftol-truncated ---
    /// Country armor mult (GetArmorMultForType(target)); larger => tougher.
    pub defender_country_armor: f64,
    /// Per-unit ArmorMultiplier (Techno+0x158); folded with country into ONE
    /// divide stage.
    pub defender_unit_armor: f64,
    /// VeteranArmor (Rules+0x688, ~1.5) when the target has the armor vet/elite
    /// ability, else 1.0.
    pub defender_vet_armor: f64,
}

impl Default for CombatMods {
    fn default() -> Self {
        Self {
            attacker_country_firepower: 1.0,
            attacker_unit_firepower: 1.0,
            attacker_vet_combat: 1.0,
            attacker_occupy: 1.0,
            attacker_tank_bunker: 1.0,
            attacker_open_topped: 1.0,
            defender_country_armor: 1.0,
            defender_unit_armor: 1.0,
            defender_vet_armor: 1.0,
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
    pub strength: i32,
    pub current_hp: i32,
    /// ObjectType `Immune=` entry gate in ObjectClass::ReceiveDamage.
    pub object_immune: bool,
    pub is_building: bool,
    pub can_c4: bool,
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
}

/// Result of the full receiver pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DamageOutcome {
    /// > 0 = damage to subtract; < 0 = heal.
    pub hp_delta: i32,
    /// Final signed value left in ObjectClass's `int *damage` packet. This is
    /// normally the HP delta, but an ObjectType `Immune=` early return leaves
    /// the transformed incoming value intact while applying no health change.
    /// TechnoClass uses this exact value for House anger feedback.
    pub post_object_damage: Option<i32>,
    pub state: DamageState,
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
