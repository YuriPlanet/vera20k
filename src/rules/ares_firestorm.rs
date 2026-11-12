//! Optional Ares firestorm wall configuration (`docs/ares-firestorm.md`):
//! RulesExt's firestorm anims, warhead and damage coefficient, which
//! `RulesExt::LoadAfterTypeData` (installed Ares `1002AAF0`, then
//! `RulesExt::ExtData::LoadAfterTypeData @ 1002A800`) reads. The
//! BuildingType `Firestorm.Wall=` key is read with the other BuildingTypeExt
//! keys (`rules::ares_building_type`); `sim::ares_firestorm` consumes both.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, audio/, etc.

use crate::rules::ini_parser::IniFile;
use crate::util::native_x87::NativeF64Bits;

/// RulesExt `+0xF0..+0x108`. The constructor (`1002A360`) clears the four
/// anims and the warhead (unset) and stores a 0.0 coefficient.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct FirestormGlobalRules {
    /// `[AudioVisual] FirestormActiveAnim` (`+0xF0`): the anim a wall
    /// section shows while its owner's firestorm is up.
    active_anim: Option<String>,
    /// `[AudioVisual] FirestormIdleAnim` (`+0xF4`): the random flicker of a
    /// powered section.
    idle_anim: Option<String>,
    /// `[AudioVisual] FirestormGroundAnim` (`+0xF8`): where a victim lower
    /// than 100 leptons burns.
    ground_anim: Option<String>,
    /// `[AudioVisual] FirestormAirAnim` (`+0xFC`): where a higher one burns.
    air_anim: Option<String>,
    /// `[CombatDamage] FirestormWarhead` (`+0x100`, Nullable): unset uses
    /// `C4Warhead=`; `Some(None)` is an explicit `none`.
    warhead: Option<Option<String>>,
    /// `[General] DamageToFirestormDamageCoefficient` (`+0x108`,
    /// Valueable<double>): the share of a hit on a powered section taken
    /// off the firestorm's drain timer.
    damage_coefficient: NativeF64Bits,
}

impl Default for FirestormGlobalRules {
    fn default() -> Self {
        Self {
            active_anim: None,
            idle_anim: None,
            ground_anim: None,
            air_anim: None,
            warhead: None,
            damage_coefficient: NativeF64Bits::POSITIVE_ZERO,
        }
    }
}

/// The registry's Find-only lookups of one pass, in `1002AAF0`'s order: the
/// four default anims (`AnimTypeClass::Find` of `GAFSDF_A`, `FSIDLE`,
/// `FSGRND`, `FSAIR`, only for the rulesmd.ini pass), then the four keyed
/// anims and the warhead (`detail::read<AnimTypeClass*> @ 1000EB20`,
/// `Nullable<WarheadTypeClass*> @ 10008D00`). `None` keeps a field;
/// `Some(None)` is a blank (`none`) value.
#[derive(Debug, Default)]
pub(crate) struct FirestormReferences {
    pub(crate) default_anims: Option<[Option<String>; 4]>,
    pub(crate) anims: [Option<Option<String>>; 4],
    pub(crate) warhead: Option<Option<String>>,
}

impl FirestormGlobalRules {
    /// `1002AAF0`: on the rulesmd.ini pass the four anims take their Find
    /// defaults first; then `1002A800`'s reads, after the TogglePower keys
    /// and before BountyEnablers.
    pub(crate) fn read_after_type_data(&mut self, pass: &IniFile, references: FirestormReferences) {
        let _ares = crate::rules::ares_switch::ares_reads();
        if let Some([active, idle, ground, air]) = references.default_anims {
            self.active_anim = active;
            self.idle_anim = idle;
            self.ground_anim = ground;
            self.air_anim = air;
        }
        let [active, idle, ground, air] = references.anims;
        for (field, read) in [
            (&mut self.active_anim, active),
            (&mut self.idle_anim, idle),
            (&mut self.ground_anim, ground),
            (&mut self.air_anim, air),
        ] {
            if let Some(name) = read {
                *field = name;
            }
        }
        if let Some(warhead) = references.warhead {
            self.warhead = Some(warhead);
        }
        if let Some(section) = pass.section("General") {
            // Retail [General] already sets the TS key.
            self.damage_coefficient = crate::rules::ares_switch::not_ares_read(|| {
                section.read_ares_double_bits(
                    "DamageToFirestormDamageCoefficient",
                    self.damage_coefficient,
                )
            });
        }
    }

    pub(crate) fn active_anim(&self) -> Option<&str> {
        self.active_anim.as_deref()
    }

    pub(crate) fn idle_anim(&self) -> Option<&str> {
        self.idle_anim.as_deref()
    }

    /// `FirestormGroundAnim` for a victim below 100 leptons
    /// (`ImmolateVictim`, inlined in `10013280`), else `FirestormAirAnim`.
    pub(crate) fn victim_anim(&self, height: i32) -> Option<&str> {
        if height < 100 {
            self.ground_anim.as_deref()
        } else {
            self.air_anim.as_deref()
        }
    }

    /// `FirestormWarhead.Get(C4Warhead)`: `None` for an explicit `none`.
    pub(crate) fn warhead<'a>(&'a self, c4_warhead: &'a str) -> Option<&'a str> {
        match &self.warhead {
            None => Some(c4_warhead),
            Some(warhead) => warhead.as_deref(),
        }
    }

    pub(crate) fn damage_coefficient(&self) -> NativeF64Bits {
        self.damage_coefficient
    }
}

/// The four default names of `1002AAF0`, in its order.
pub(crate) const FIRESTORM_DEFAULT_ANIMS: [&str; 4] = ["GAFSDF_A", "FSIDLE", "FSGRND", "FSAIR"];

/// The four anim keys of `1002A800`, in its order (`[AudioVisual]`).
pub(crate) const FIRESTORM_ANIM_KEYS: [&str; 4] = [
    "FirestormActiveAnim",
    "FirestormIdleAnim",
    "FirestormGroundAnim",
    "FirestormAirAnim",
];

#[cfg(test)]
mod tests {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;

    fn rules(rulesmd: &str, scenario: &str) -> RuleSet {
        let mut layers = RulesLayerStack::new(IniFile::from_str(rulesmd));
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(scenario));
        let processed = layers.process_with_fixed_art(&IniFile::empty()).unwrap();
        RuleSet::from_processed_rules(&processed).unwrap()
    }

    /// `1002AAF0`: the four anims take their `AnimTypeClass::Find` defaults
    /// on the rulesmd.ini pass only, after its type data; `1002A800` reads
    /// the keys on every pass with Find-only pointers (`none` clears, an
    /// unknown name keeps), the warhead as a Nullable and the coefficient as
    /// a double.
    #[test]
    fn firestorm_rules_take_rulesmd_find_defaults_then_every_pass_reads_the_keys() {
        let rules = rules(
            "[Animations]\n0=FSIDLE\n1=FSGRND\n2=BURN\n[Warheads]\n0=FIRE\n\
             [AudioVisual]\nFirestormGroundAnim=none\n\
             [General]\nDamageToFirestormDamageCoefficient=0.5\n",
            "[Animations]\n3=GAFSDF_A\n[AudioVisual]\nFirestormAirAnim=BURN\n\
             FirestormIdleAnim=MISSING\n[CombatDamage]\nFirestormWarhead=FIRE\n",
        );
        let globals = rules.ares_firestorm_globals();
        assert_eq!(
            globals.active_anim(),
            None,
            "GAFSDF_A is only registered by a later pass"
        );
        assert_eq!(globals.idle_anim(), Some("FSIDLE"), "an unknown name keeps");
        assert_eq!(globals.victim_anim(0), None, "none clears the default");
        assert_eq!(globals.victim_anim(100), Some("BURN"));
        assert_eq!(globals.warhead("C4"), Some("FIRE"));
        assert_eq!(f64::from_bits(globals.damage_coefficient().bits()), 0.5);
        let unset = self::rules("[General]\n", "");
        assert_eq!(unset.ares_firestorm_globals().warhead("C4"), Some("C4"));
    }
}
