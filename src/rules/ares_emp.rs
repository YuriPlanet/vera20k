//! Retained ordinary Ares EMP configuration. Installed Ares3.0 Bullet46920B
//! and Techno6FAF0D consumers; Type suffix716132 and WH suffix75DEA0 own
//! the actual reads. This module owns no gameplay timer or offline latch.
#[cfg(feature = "ares")]
use crate::rules::ini_parser::IniSection;
use crate::util::native_x87::NativeF64Bits;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct EmpTypeRules {
    modifier: NativeF64Bits,
    threshold: i32,
    immune: Option<bool>,
    sparkles: Option<Option<String>>,
    veteran_immune: bool,
    elite_immune: bool,
}
impl Default for EmpTypeRules {
    fn default() -> Self {
        Self {
            modifier: NativeF64Bits::ONE,
            threshold: -1,
            immune: None,
            sparkles: None,
            veteran_immune: false,
            elite_immune: false,
        }
    }
}
impl EmpTypeRules {
    pub(crate) fn modifier(&self) -> NativeF64Bits {
        self.modifier
    }
    pub(crate) fn threshold(&self) -> i32 {
        self.threshold
    }
    pub(crate) fn immune(&self) -> Option<bool> {
        self.immune
    }
    pub(crate) fn sparkles(&self) -> Option<Option<&str>> {
        self.sparkles.as_ref().map(|name| name.as_deref())
    }
    pub(crate) fn rank_immune(&self, rank: u16) -> bool {
        rank >= 100 && self.veteran_immune || rank >= 200 && self.elite_immune
    }
    #[cfg(feature = "ares")]
    pub(crate) fn read_pass(&mut self, section: &IniSection, sparkle: Option<Option<String>>) {
        self.immune = section.read_ares_nullable_bool("ImmuneToEMP", self.immune);
        self.modifier = section.read_ares_double_bits("EMP.Modifier", self.modifier);
        if let Some(sparkle) = sparkle {
            self.sparkles = Some(sparkle);
        }
        self.threshold = section.read_ares_emp_threshold("EMP.Threshold", self.threshold);
        // Installed suffix100405A0 clears that tier's seven extension flags
        // only after a nonempty ReadString2048; strtok has comma-only delimiters.
        if let Some(tokens) = section.read_list("VeteranAbilities", 2048) {
            self.veteran_immune = tokens
                .iter()
                .any(|name| name.eq_ignore_ascii_case("EMPIMMUNE"));
        }
        if let Some(tokens) = section.read_list("EliteAbilities", 2048) {
            self.elite_immune = tokens
                .iter()
                .any(|name| name.eq_ignore_ascii_case("EMPIMMUNE"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{
        ini_parser::IniFile,
        native_processing::{RulesLayerKind, RulesLayerStack},
        ruleset::RuleSet,
    };
    fn corpus() -> serde_json::Value {
        serde_json::from_str(crate::test_fixture::text(
            "tools/rules_oracle/ares_emp.json",
        ))
        .unwrap()
    }
    #[test]
    fn ares_emp_shared_readers_match_native_corpus() {
        for row in corpus()["readers"].as_array().unwrap() {
            let raw = row["fixture"]["raw"].as_str();
            let ini = IniFile::from_str(&format!(
                "[T]\n{}",
                raw.map_or(String::new(), |value| format!("Value={value}\n"))
            ));
            // Native missing/empty ReadString fixtures may have no retained
            // physical section body; exercise the reader with its empty view.
            let section = ini.section_or_empty("T");
            let prior = &row["fixture"]["prior"];
            let result = &row["result"];
            match row["kind"].as_str().unwrap() {
                "int" => assert_eq!(
                    section.read_ares_int("Value", prior.as_i64().unwrap() as i32),
                    result["value"].as_i64().unwrap() as i32,
                    "{row}"
                ),
                "threshold" => assert_eq!(
                    section.read_ares_emp_threshold("Value", prior.as_i64().unwrap() as i32),
                    result["value"].as_i64().unwrap() as i32,
                    "{row}"
                ),
                "double" => {
                    let bits = section.read_ares_double_bits(
                        "Value",
                        NativeF64Bits::from_bits(prior.as_f64().unwrap().to_bits()),
                    );
                    let hex: String = bits
                        .bits()
                        .to_le_bytes()
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect();
                    assert_eq!(hex, result["value_le_bits"].as_str().unwrap(), "{row}");
                }
                kind => panic!("unhandled native reader {kind}"),
            }
        }
    }
    #[cfg(feature = "ares")]
    #[test]
    fn ares_emp_native_bool_and_ability_readers() {
        for row in corpus()["flag_readers"].as_array().unwrap() {
            let kind = row["kind"].as_str().unwrap();
            let key = match kind {
                "enemies" => "AffectsEnemies",
                "owner" => "AffectsOwner",
                "abilities" => "VeteranAbilities",
                _ => panic!("unknown kind"),
            };
            let text = row["fixture"]["raw"]
                .as_str()
                .map_or(String::new(), |raw| format!("{key}={raw}\n"));
            let ini = IniFile::from_str(&format!("[T]\n{text}"));
            // Native missing/empty ReadString fixtures may have no retained
            // physical section body; exercise the reader with its empty view.
            let section = ini.section_or_empty("T");
            let expected = &row["result"]["bytes"];
            if kind == "abilities" {
                let mut config = EmpTypeRules::default();
                config.veteran_immune = true;
                config.read_pass(section, None);
                assert_eq!(
                    config.veteran_immune,
                    expected[0].as_i64().unwrap() != 0,
                    "{row}"
                );
            } else {
                let mut config = crate::rules::ares_warhead::AresWarheadRules::default();
                config.read_pass(section, None, None);
                if kind == "enemies" {
                    assert_eq!(
                        config.affects_enemies(),
                        expected[0].as_i64().unwrap() != 0,
                        "{row}"
                    );
                } else {
                    let expected = (expected[1].as_i64().unwrap() != 0)
                        .then(|| expected[0].as_i64().unwrap() != 0);
                    assert_eq!(config.affects_owner(), expected, "{row}");
                }
            }
        }
    }
    #[test]
    fn ares_emp_production_layers_retain_existing_anim_and_nullable_settings() {
        let root = IniFile::from_str(
            "[VehicleTypes]\n0=TANK\n[Warheads]\n0=EMPWH\n[Animations]\n0=SPARK\n[General]\nEMPulseSparkles=SPARK\n[TANK]\nImmuneToEMP=no\nEMP.Modifier=50%\nEMP.Threshold=inair\nEMP.Sparkles=SPARK\nVeteranAbilities=EMPIMMUNE\n[EMPWH]\nEMP.Duration=121\nEMP.Cap=120\nEMP.Sparkles=SPARK\nAffectsOwner=yes\n",
        );
        let scenario = "[TANK]\nEMP.Modifier=junk\nEMP.Sparkles=UNKNOWN\nVeteranAbilities=RADIMMUNE\nEliteAbilities=empimmune\n[EMPWH]\nEMP.Duration=\nEMP.Cap=0\n";
        let mut layers = RulesLayerStack::new(root.clone());
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(scenario));
        let rules = RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap();
        let object = rules.object("TANK").unwrap();
        if crate::rules::ARES_ENABLED {
            let config = object.emp_rules().unwrap();
            assert_eq!(
                config.modifier(),
                NativeF64Bits::from_bits(0.5f64.to_bits())
            );
            assert_eq!(config.immune(), Some(false));
            assert_eq!(config.threshold(), -1);
            assert_eq!(config.sparkles(), Some(Some("SPARK")));
            assert!(!config.rank_immune(100));
            assert!(config.rank_immune(200));
            let wh = rules.warhead("EMPWH").unwrap().ares_rules().unwrap();
            assert_eq!(wh.duration(), 121);
            assert_eq!(wh.cap(), 0);
            assert!(wh.can_affect_house(true, true, true, false));
            assert_eq!(rules.emp_sparkles(), Some("SPARK"));
            let hash = rules.simulation_config_hash();
            let mut null_layers = RulesLayerStack::new(root);
            null_layers.push(
                RulesLayerKind::Scenario,
                IniFile::from_str(&scenario.replace("EMP.Sparkles=UNKNOWN", "EMP.Sparkles=<none>")),
            );
            let null_rules =
                RuleSet::from_processed_rules(&null_layers.process().unwrap()).unwrap();
            assert_eq!(
                null_rules
                    .object("TANK")
                    .unwrap()
                    .emp_rules()
                    .unwrap()
                    .sparkles(),
                Some(None)
            );
            assert_ne!(null_rules.simulation_config_hash(), hash);
        } else {
            assert!(object.emp_rules().is_none());
            assert!(rules.warhead("EMPWH").unwrap().ares_rules().is_none());
        }
    }
}
