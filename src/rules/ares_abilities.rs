//! The seven TypeExt rank abilities read by Ares100405A0. The retained
//! processing registry owns these flags; individual effects only query them.

#[cfg(feature = "ares")]
use crate::util::native_x87::NativeF32Bits;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct PromotionAbilities {
    veteran: [bool; 7],
    elite: [bool; 7],
}

impl PromotionAbilities {
    #[cfg(feature = "ares")]
    pub(crate) fn read_pass(&mut self, section: &crate::rules::ini_parser::IniSection) {
        // TypeExt1003E4FA/1003E510 -> ReadAbilities100405A0. An absent or
        // empty ReadString2048 keeps the tier. A nonempty value clears all
        // seven bytes before comma-only tokenization and table lookup.
        Self::read_tier(section, "VeteranAbilities", &mut self.veteran);
        Self::read_tier(section, "EliteAbilities", &mut self.elite);
    }

    #[cfg(feature = "ares")]
    fn read_tier(section: &crate::rules::ini_parser::IniSection, key: &str, flags: &mut [bool; 7]) {
        const NAMES: [&str; 7] = [
            "empimmune",
            "radimmune",
            "protected_driver",
            "unwarpable",
            "poisonimmune",
            "psionicsimmune",
            "psionicweaponimmune",
        ]; // Installed string table1009CDC4.
        if let Some(tokens) = section.read_list(key, 2048) {
            *flags = NAMES.map(|name| tokens.iter().any(|token| token.eq_ignore_ascii_case(name)));
        }
    }

    pub(crate) fn emp_immune(&self, rank: u16) -> bool {
        rank >= 100 && self.veteran[0] || rank >= 200 && self.elite[0]
    }

    /// CanKillDriver1004358F..100435CE samples the raw binary32 XP, not the
    /// cached rank. Above two, either tier protects; below one neither does.
    #[cfg(feature = "ares")]
    pub(crate) fn driver_protected(&self, experience: NativeF32Bits) -> bool {
        let experience = f32::from_bits(experience.bits());
        if experience >= 2.0 {
            self.veteran[2] || self.elite[2]
        } else {
            experience >= 1.0 && self.veteran[2]
        }
    }
}

#[cfg(all(test, feature = "ares"))]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
    use crate::rules::sound_ini::{EvaRegistry, SoundRegistry};
    use std::sync::Arc;

    #[test]
    fn promotion_abilities_keep_tiers_through_rules_layers_saves_and_hash() {
        let root = "[VehicleTypes]\n0=TANK\n1=ELITE\n2=RESET\n\
                    [TANK]\nVeteranAbilities=EMPIMMUNE,protected_driver\n\
                    [ELITE]\nEliteAbilities=protected_driver\n\
                    [RESET]\nVeteranAbilities=protected_driver\n";
        let map = IniFile::from_str(
            "[TANK]\nVeteranAbilities=\n[RESET]\nVeteranAbilities=unknown_ability\n",
        );
        let load = |source: &str| {
            let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
                IniFile::from_str(source),
                None,
                IniFile::empty(),
                Arc::new(SoundRegistry::from_ini(&IniFile::empty())),
                Arc::new(EvaRegistry::from_ini(&IniFile::empty())),
            )
            .unwrap();
            let (rules, ..) = owner
                .load_scenario(NativeScenarioRulesPrefix::NonCampaign(None), &map)
                .unwrap()
                .into_parts();
            (owner, rules)
        };
        let (owner, rules) = load(root);
        let tank = rules.object("TANK").unwrap().promotion_abilities().unwrap();
        let elite = rules
            .object("ELITE")
            .unwrap()
            .promotion_abilities()
            .unwrap();
        assert!(tank.emp_immune(100));
        assert!(!tank.driver_protected(NativeF32Bits::from_bits(0.99f32.to_bits())));
        assert!(tank.driver_protected(NativeF32Bits::ONE));
        let elite_xp = NativeF32Bits::from_bits(2.0f32.to_bits());
        assert!(tank.driver_protected(elite_xp));
        assert!(!elite.driver_protected(NativeF32Bits::ONE));
        assert!(elite.driver_protected(elite_xp));
        assert!(
            !rules
                .object("RESET")
                .unwrap()
                .promotion_abilities()
                .unwrap()
                .driver_protected(elite_xp)
        );

        let saved =
            crate::util::save_codec::serialize(owner.saved_scenario_rules().unwrap()).unwrap();
        let restored = crate::util::save_codec::deserialize(&saved).unwrap();
        let restored = owner.project_saved_scenario_rules(&restored).unwrap();
        assert_eq!(
            Some(tank),
            restored.object("TANK").unwrap().promotion_abilities()
        );
        // Continue both histories with identical current INI text. The retained
        // registry, not a private source-hash override, must distinguish them.
        let continued = |source: &str| {
            use crate::rules::native_processing::RulesLayerStack;
            let (_, trace) = RulesLayerStack::new(IniFile::from_str(source))
                .process()
                .unwrap()
                .into_ini_and_native_type_construction_trace();
            let processed = RulesLayerStack::new(IniFile::empty())
                .process_with_fixed_art_and_registry_state(
                    &IniFile::empty(),
                    trace.into_registry_state_discarding_events(),
                )
                .unwrap();
            crate::rules::ruleset::RuleSet::from_processed_rules(&processed).unwrap()
        };
        let retained = continued(root);
        let changed = continued(&root.replace("protected_driver", "radimmune"));
        assert_eq!(changed.source_ini_hash(), retained.source_ini_hash());
        assert_ne!(
            retained.simulation_config_hash(),
            changed.simulation_config_hash()
        );
    }
}
