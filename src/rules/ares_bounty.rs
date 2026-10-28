//! Retained optional Ares bounty configuration, projected by the rules registry.
//! Source: installed Ares TypeExt reader1003F750..1003F833 and RulesExt
//! readers1002AAAB/1002AAC5/1002ADC6. No resource loading belongs here.

use std::hash::{Hash, Hasher};

use crate::{
    rules::ini_parser::{IniFile, IniSection},
    util::native_x87::NativeF64Bits,
};

/// TypeExt+4F4/+4F8/+4FC, +500 and nullable+501/+502. Constructor1003CB80
/// clears EAX;1003CDFC..1003CE0E zeroes the values and all flag bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct BountyTypeRules {
    values: [i32; 3],
    enabled: bool,
    display: Option<bool>,
}

impl BountyTypeRules {
    pub(crate) fn read_pass(&mut self, section: &IniSection) {
        // Rank reader1003F750..1003F833: common value first copies to all
        // ranks; then Rookie, Veteran and Elite override independently.
        // Missing/invalid Ares integer reads retain the current destination.
        if let Some(value) = section.read_ares_nullable_int("Bounty.Value", None) {
            self.values = [value; 3];
        }
        for (slot, key) in [
            "Bounty.RookieValue",
            "Bounty.VeteranValue",
            "Bounty.EliteValue",
        ]
        .into_iter()
        .enumerate()
        {
            self.values[slot] = section.read_ares_int(key, self.values[slot]);
        }
        self.enabled = section.read_ares_bool("Bounty", self.enabled);
        self.display = section.read_ares_nullable_bool("Bounty.Display", self.display);
    }

    pub(crate) fn values(&self) -> &[i32; 3] {
        &self.values
    }

    /// The incoming attacker's flag gates payout; victim values are separate.
    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn has_gameplay_configuration(&self) -> bool {
        self.enabled || self.values != [0; 3]
    }

    pub(crate) fn display(&self) -> Option<bool> {
        self.display
    }
}

// Visibility of the signed credit label cannot change simulation decisions.
impl Hash for BountyTypeRules {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.values.hash(state);
        self.enabled.hash(state);
    }
}

/// RulesExt retains these across ordinary Process passes. Destructive Rules
/// reconstruction resets them through the existing global-controls owner.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct BountyGlobalRules {
    enablers: Vec<String>,
    display: bool,
    display_delay: NativeF64Bits,
}

impl Default for BountyGlobalRules {
    fn default() -> Self {
        Self {
            // Constructor1002A4C7..1002A4D9 clears the vector and display byte.
            enablers: Vec::new(),
            display: false,
            // Constructor1002A449/1002A44F copies binary64 at1009A6E8 (0.02).
            display_delay: NativeF64Bits::from_bits(0x3f94_7ae1_47ae_147b),
        }
    }
}

impl BountyGlobalRules {
    pub(crate) fn read_before_type_data(&mut self, pass: &IniFile) {
        //1002ADB1..1002ADC6, Valueable<double>1000EB00, AudioVisual.
        if let Some(section) = pass.section("AudioVisual") {
            self.display_delay =
                section.read_ares_double_bits("DisplayCreditsDelay", self.display_delay);
        }
    }

    /// The registry supplies the BuildingType Find-only vector read10010850:
    /// General/BountyEnablers1002AAAB. Successful empty results clear it;
    /// failed or absent ReadString retains it. Order and duplicates survive.
    pub(crate) fn read_after_type_data(&mut self, pass: &IniFile, enablers: Option<Vec<String>>) {
        if let Some(enablers) = enablers {
            self.enablers = enablers;
        }
        //1002AAC4..1002AAD9 reads a separate AudioVisual bool.
        if let Some(section) = pass.section("AudioVisual") {
            self.display = section.read_ares_bool("BountyDisplay", self.display);
        }
    }

    pub(crate) fn enablers(&self) -> &[String] {
        &self.enablers
    }

    pub(crate) fn display(&self) -> bool {
        self.display
    }

    pub(crate) fn display_delay(&self) -> NativeF64Bits {
        self.display_delay
    }
}

impl Hash for BountyGlobalRules {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.enablers.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{
        native_processing::{RulesLayerKind, RulesLayerStack},
        ruleset::RuleSet,
    };

    #[test]
    fn bounty_readers_retain_layers_rank_overrides_and_noninherited_country_gate() {
        let root = "[Countries]\n0=PARENT\n1=CHILD\n[PARENT]\nGivesBounty=no\n\
            [CHILD]\nParentCountry=PARENT\n[VehicleTypes]\n0=HUNTER\n1=TARGET\n\
            [BuildingTypes]\n0=BANK\n[General]\nBountyEnablers=BANK,UNKNOWN,BANK\n\
            [AudioVisual]\nBountyDisplay=yes\nDisplayCreditsDelay=0.25\n\
            [HUNTER]\nBounty=yes\nBounty.Display=no\n[TARGET]\nBounty.Value=30\nBounty.EliteValue=70\n";
        let retained = |root: &str| {
            let mut layers = RulesLayerStack::new(IniFile::from_str(root));
            layers.push(
                RulesLayerKind::Scenario,
                IniFile::from_str(
                    "[HUNTER]\nBounty=invalid\nBounty.Display=invalid\n\
                 [TARGET]\nBounty.VeteranValue=-12\nBounty.EliteValue=invalid\n\
                 [PARENT]\nGivesBounty=invalid\n[General]\nBountyEnablers=\n\
                 [AudioVisual]\nDisplayCreditsDelay=invalid\nBountyDisplay=invalid\n",
                ),
            );
            let (_, trace) = layers
                .process()
                .unwrap()
                .into_ini_and_native_type_construction_trace();
            let processed = RulesLayerStack::new(IniFile::empty())
                .process_with_fixed_art_and_registry_state(
                    &IniFile::empty(),
                    trace.into_registry_state_discarding_events(),
                )
                .unwrap();
            RuleSet::from_processed_rules(&processed).unwrap()
        };
        let rules = retained(root);
        let hunter = rules.object("HUNTER").unwrap().bounty_rules();
        assert!(hunter.enabled());
        assert_eq!(hunter.display(), Some(false));
        let target = rules.object("TARGET").unwrap().bounty_rules();
        assert_eq!(target.values(), &[30, -12, 70]);
        assert!(!target.enabled() && target.display().is_none());
        assert!(!rules.country_gives_bounty("PARENT"));
        assert!(
            rules.country_gives_bounty("CHILD"),
            "parent copy does not copy GivesBounty"
        );
        assert_eq!(rules.bounty_rules().enablers(), &["BANK", "BANK"]);
        assert!(rules.object("UNKNOWN").is_none());
        assert_eq!(
            rules.bounty_rules().display_delay().bits(),
            0.25f64.to_bits()
        );
        let saved: BountyTypeRules =
            serde_json::from_slice(&serde_json::to_vec(target).unwrap()).unwrap();
        assert_eq!(&saved, target);
        let display_only = retained(
            &root
                .replace("BountyDisplay=yes", "BountyDisplay=no")
                .replace("Bounty.Display=no", "Bounty.Display=yes")
                .replace("Delay=0.25", "Delay=0.5"),
        );
        assert_eq!(
            rules.simulation_config_hash(),
            display_only.simulation_config_hash()
        );
        let value_change = retained(&root.replace("Bounty.Value=30", "Bounty.Value=31"));
        assert_ne!(
            rules.simulation_config_hash(),
            value_change.simulation_config_hash()
        );
    }
}
