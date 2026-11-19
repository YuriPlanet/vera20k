//! Phobos `WeaponTypeExt` keys that no other rules owner reads.
//!
//! Read in `WeaponTypeExt::LoadFromINIFile` order: `FeedbackWeapon`
//! (`Ext/WeaponType/Body.cpp:124`; `Valueable<WeaponTypeClass*>` declared
//! `Ext/WeaponType/Body.h:56`, constructed null `:151`), `VisualScatter`
//! (`Body.cpp:133`; `Valueable<bool>` `Body.h:63`, constructed `no` `:158`) and
//! `AttackNoThreatBuildings` (`Body.cpp:206`; `Nullable<bool>`
//! `Ext/WeaponType/Body.h:114`, constructed unset `:210`).
//!
//! Two of the three carry a `[General]`/`[AudioVisual]` fallback:
//! - `AttackNoThreatBuildings` has none of its own: the consumer supplies
//!   `[General] AutoTarget_NoThreatBuildings` for a human scanner and
//!   `[General] AutoTargetAI_NoThreatBuildings` for a computer one
//!   (`Ext/Techno/Hooks.cpp:1848-1858`, defaults `Ext/Rules/Body.h:931-932`).
//! - `VisualScatter`'s radius is the `[AudioVisual] VisualScatter.Min/Max`
//!   pair (`Ext/Rules/Body.cpp:325-326`, defaults 8/32 leptons
//!   `Ext/Rules/Body.h:758-759`), drawn per fired effect.
//!
//! `FeedbackWeapon` has no fallback: a weapon that does not name one fires
//! nothing extra.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::ruleset::RuleSet;

/// Phobos `AreaFireTarget` (`Utilities/Enum.h:171-177`): the aim choice for
/// an `AreaFire=` weapon. `Firer` is the native `self` spelling.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum AreaFireTarget {
    #[default]
    Base,
    Random,
    Firer,
}

impl AreaFireTarget {
    /// `read<AreaFireTarget>`'s name table (`Utilities/TemplateDef.h:952-977`);
    /// an unknown spelling keeps the previous value.
    pub fn parse(raw: &str) -> Option<Self> {
        if raw.eq_ignore_ascii_case("base") {
            Some(Self::Base)
        } else if raw.eq_ignore_ascii_case("random") {
            Some(Self::Random)
        } else if raw.eq_ignore_ascii_case("self") {
            Some(Self::Firer)
        } else {
            None
        }
    }
}

/// The `WeaponTypeExt` Phobos keys owned here.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PhobosWeaponExtras {
    /// `FeedbackWeapon=` (`Body.cpp:124`, read `<true>`): the weapon also
    /// detonated at the firer right after a shot
    /// (`Ext/Techno/Hooks.Firing.cpp:802-809`).
    pub feedback_weapon: Option<String>,
    /// `VisualScatter=` (`Body.cpp:133`), default `no`: the drawn laser, bolt
    /// or beam ends at a random point near the bullet target
    /// (`Ext/Techno/Hooks.WeaponEffects.cpp:220-248`).
    pub visual_scatter: bool,
    /// `AttackNoThreatBuildings=` (`Body.cpp:206`), `Nullable`: whether the
    /// scanner may auto-target an enemy building that is unarmed or poses no
    /// threat (`Ext/Techno/Hooks.cpp:1848-1858`).
    pub attack_no_threat_buildings: Option<bool>,
    /// `TurretRecoil.Suppress=` (`Body.cpp:138`, `Valueable<bool>` default
    /// `false`, `Ext/WeaponType/Body.h:68,163`): the fired weapon skips the
    /// unit recoil record (`Ext/Techno/Hooks.Firing.cpp:821-836`, hook
    /// `0x6FF0DD`).
    pub turret_recoil_suppress: bool,
    /// `FireOnce.ResetSequence=` (`Body.cpp:137`, `Valueable<bool>` default
    /// `true`, `Ext/WeaponType/Body.h:67,162`): `no` keeps an infantryman's
    /// firing sequence across the target loss its `FireOnce=` shot causes —
    /// the `0x6FF905` hook sets `SkipTargetChangeResetSequence`, and
    /// `InfantryClass_AssignTarget_FireOnce` (`0x51B20E`,
    /// `Ext/Techno/Hooks.Misc.cpp:845-866`) then skips the class
    /// target-change latch reset and idle `Do_Action`.
    pub fire_once_reset_sequence: bool,
    /// `OmniFire.TurnToTarget=` (`Ext/WeaponType/Body.cpp:136`,
    /// `Nullable<bool>` unset at `Ext/WeaponType/Body.h:66,161`): overrides
    /// `[General] OmniFire.TurnToTarget` for this weapon's omni-fire hull
    /// turn (`Ext/Unit/Hooks.Jumpjet.cpp:79`, hook `0x736E6E`).
    pub omni_fire_turn_to_target: Option<bool>,
    /// `AreaFire.Target=` (`Body.cpp:123`, `Valueable<AreaFireTarget>`
    /// default `Base`, `Ext/WeaponType/Body.h:55,150`): what an `AreaFire=`
    /// weapon aims at instead of the stock cell under the firer —
    /// `Random` picks a random eligible adjacent cell in range with one
    /// Scenario draw per candidate, `Firer` (native `self`) aims at the
    /// firer itself (`Ext/Techno/Hooks.Firing.cpp:759-796`, hook
    /// `0x6FE19A`).
    pub area_fire_target: AreaFireTarget,
    /// `ChargeTurret.Delays=` (`Body.cpp:135`, `ValueableVector<int>`,
    /// `Body.h:65`, default empty `:160`): per-burst-shot charge delays for a
    /// charge turret — the just-fired shot's list item overrides the saved
    /// `+2F8` denominator and starts the ext `ChargeTurretTimer`
    /// (`Ext/Techno/Hooks.Firing.cpp:936-976`); the real rearm keeps the
    /// native duration. Items `<= 0` keep the native value.
    pub charge_turret_delays: Vec<i32>,
    /// `Anim.Update=` (`Ext/WeaponType/Body.cpp:208`, `Nullable<bool>`):
    /// a muzzle anim this weapon's `Anim=` list produced keeps following
    /// its firer every frame (`Ext/Anim/Hooks.cpp:714-740`). Unset falls
    /// back to `[AudioVisual] FiringAnim.Update=` (`Ext/Rules/Body.cpp:548`).
    pub anim_update: Option<bool>,
}

impl Default for PhobosWeaponExtras {
    /// The constructor values, including `FireOnce.ResetSequence`'s `true`
    /// (`Ext/WeaponType/Body.h:162`), so an empty section equals the default.
    fn default() -> Self {
        Self {
            anim_update: None,
            feedback_weapon: None,
            visual_scatter: false,
            attack_no_threat_buildings: None,
            turret_recoil_suppress: false,
            fire_once_reset_sequence: true,
            omni_fire_turn_to_target: None,
            area_fire_target: AreaFireTarget::Base,
            charge_turret_delays: Vec::new(),
        }
    }
}

impl PhobosWeaponExtras {
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            feedback_weapon: section
                .read_name("FeedbackWeapon", 0x100)
                .map(str::to_owned),
            visual_scatter: section.read_ares_bool("VisualScatter", false),
            attack_no_threat_buildings: section
                .read_phobos_nullable_bool("AttackNoThreatBuildings", None),
            turret_recoil_suppress: section.read_bool("TurretRecoil.Suppress", false),
            fire_once_reset_sequence: section.read_bool("FireOnce.ResetSequence", true),
            omni_fire_turn_to_target: section
                .read_phobos_nullable_bool("OmniFire.TurnToTarget", None),
            area_fire_target: section.read_string_with(
                "AreaFire.Target",
                2048,
                AreaFireTarget::Base,
                |current, raw| AreaFireTarget::parse(raw).unwrap_or(current),
            ),
            charge_turret_delays: section
                .read_ares_int_list("ChargeTurret.Delays")
                .unwrap_or_default(),
            // `Ext/WeaponType/Body.cpp:208`, `Nullable<bool>`: unset
            // falls back to `[AudioVisual] FiringAnim.Update=`.
            anim_update: section.read_phobos_nullable_bool("Anim.Update", None),
        }
    }

    /// `AttackNoThreatBuildings.Get(human ? [General] AutoTarget_NoThreatBuildings
    /// : [General] AutoTargetAI_NoThreatBuildings)`
    /// (`Ext/Techno/Hooks.cpp:1852-1858`).
    pub fn attack_no_threat_buildings(&self, rules: &RuleSet, human: bool) -> bool {
        self.attack_no_threat_buildings.unwrap_or(if human {
            rules.phobos_globals.auto_target_no_threat_buildings
        } else {
            rules.phobos_globals.auto_target_ai_no_threat_buildings
        })
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_weapon_extras_tests.rs"]
mod tests;
