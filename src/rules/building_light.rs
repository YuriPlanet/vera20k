//! Retained BuildingType radius-light inputs, before LightSource construction.
//!
//! Constructor45DE12..45DE33 and reader460C8C..460DCA own these integers.
//! Each admitted type section re-reads all five keys. Crucially, the four
//! ReadDouble defaults use signed integer division by1000, not a fractional
//! conversion. A sound-only map override therefore clears a stock .01 lamp.
//! Native execution and its limits: tools/spatial_oracle/building_body_rules.py
//! `--lights`, building_light_fields.json and its metadata sidecar.

use crate::rules::ini_parser::IniSection;
use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};

/// ProcessedType owns mutation during Rules passes; ObjectType receives an
/// immutable projection. LightSource consumers use these units directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BuildingLightRules {
    visibility_leptons: i32,
    intensity_units: i32,
    tint_units: [i32; 3],
}

impl Default for BuildingLightRules {
    fn default() -> Self {
        Self {
            visibility_leptons: 5000,
            intensity_units: 0,
            // Native45DE1C loads0xF4240 before storing all three channels.
            // This is1000.0 on the reader's scale, not a guessed unity tint.
            tint_units: [1_000_000; 3],
        }
    }
}

impl BuildingLightRules {
    pub const fn visibility_leptons(self) -> i32 {
        self.visibility_leptons
    }

    pub const fn intensity_units(self) -> i32 {
        self.intensity_units
    }

    pub const fn tint_units(self) -> [i32; 3] {
        self.tint_units
    }

    /// Call only for a reached, exact-case BuildingType Rules section.
    pub(crate) fn read_pass(&mut self, section: &IniSection) {
        self.visibility_leptons = section.read_int("LightVisibility", self.visibility_leptons);
        self.intensity_units = read_units(section, "LightIntensity", self.intensity_units);
        for (field, key) in
            self.tint_units
                .iter_mut()
                .zip(["LightRedTint", "LightGreenTint", "LightBlueTint"])
        {
            *field = read_units(section, key, *field);
        }
    }
}

fn read_units(section: &IniSection, key: &str, current: i32) -> i32 {
    //460CAC/460CB4 IMUL+SAR implements signed /1000 before FILD. The
    // ReadDouble's widened value is multiplied by1000, then biased by .1
    // (7E3860). Binary64 suffices for the executed retail/control corpus;
    // arbitrary extreme mod-value rounding is not certified. Reuse only the
    // existing ftol conversion for its low-dword/invalid semantics, rather
    // than Rust's saturating float cast or a second x87 arithmetic model.
    let value = section.read_double(key, f64::from(current / 1000));
    X87::ftol_i32_low_masked(X87::load_f64(NativeF64Bits::from_bits(
        (value * 1000.0 + 0.1).to_bits(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;

    fn fields(light: BuildingLightRules) -> [i32; 5] {
        let [red, green, blue] = light.tint_units();
        [
            light.visibility_leptons(),
            light.intensity_units(),
            red,
            green,
            blue,
        ]
    }

    fn golden_fields(value: &serde_json::Value) -> [i32; 5] {
        std::array::from_fn(|i| value[i].as_i64().unwrap() as i32)
    }

    #[test]
    fn building_light_fields_match_native_repeated_read_history() {
        let corpus = crate::test_fixture::json("tools/spatial_oracle/building_light_fields.json");
        assert_eq!(
            fields(BuildingLightRules::default()),
            golden_fields(&corpus["constructor"])
        );
        let root = || RulesLayerStack::new(IniFile::from_str("[BuildingTypes]\n0=LAMP\n"));
        let mut layers = root();
        let mut prior = fields(BuildingLightRules::default());
        for row in corpus["rows"].as_array().unwrap() {
            if row["reset_constructor"].as_bool() == Some(true) {
                layers = root();
                prior = fields(BuildingLightRules::default());
            }
            assert_eq!(
                prior,
                golden_fields(&row["before"]),
                "{} prior",
                row["name"]
            );
            let mut text = String::new();
            if row["section_present"].as_bool().unwrap() {
                text.push_str("[LAMP]\nFixtureOnly=1\n");
                for (key, value) in row["authored_keys"].as_object().unwrap() {
                    text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
                }
            } else {
                text.push_str("[Other]\nFixtureOnly=1\n");
            }
            layers.push(RulesLayerKind::Scenario, IniFile::from_str(&text));
            let rules = RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap();
            prior = fields(rules.object("LAMP").unwrap().building_light());
            assert_eq!(
                prior,
                golden_fields(&row["after"]),
                "{} result",
                row["name"]
            );
        }
    }

    #[test]
    fn building_light_retained_integers_survive_registry_restore() {
        let corpus = crate::test_fixture::json("tools/spatial_oracle/building_light_fields.json");
        let override_row = corpus["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "explicit_light_override")
            .unwrap();
        let mut source = "[BuildingTypes]\n0=LAMP\n[LAMP]\n".to_owned();
        for (key, value) in override_row["authored_keys"].as_object().unwrap() {
            source.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
        }
        let processed = RulesLayerStack::new(IniFile::from_str(&source))
            .process()
            .unwrap();
        let (_, trace) = processed.into_ini_and_native_type_construction_trace();
        let state = trace.into_registry_state_discarding_events();
        let saved = crate::util::save_codec::serialize(&state).unwrap();
        let loaded = crate::util::save_codec::deserialize(&saved).unwrap();
        let next = RulesLayerStack::new(IniFile::from_str("[LAMP]\nWorkingSound=ambient\n"))
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), loaded)
            .unwrap();
        let rules = RuleSet::from_processed_rules(&next).unwrap();
        let expected = corpus["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "empty_authored_keys_are_omitted")
            .unwrap();
        assert_eq!(
            fields(rules.object("LAMP").unwrap().building_light()),
            golden_fields(&expected["after"])
        );
    }

    #[test]
    fn retail_sound_only_map_pass_clears_only_the_overridden_lamps() {
        let Some((root, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
            return;
        };
        let Some(mode) = crate::rules::retail_ini_fixture::retail_ini("mpbattlemd.ini") else {
            return;
        };
        let mut layers = RulesLayerStack::new(root);
        layers.push(RulesLayerKind::GameMode, mode);
        let before =
            RuleSet::from_processed_rules(&layers.process_with_fixed_art(&art).unwrap()).unwrap();
        let lamps = [
            ("INREDLMP", "stock_green"),
            ("INGRNLMP", "stock_green"),
            ("INBLULMP", "stock_green"),
            ("INYELWLAMP", "stock_green"),
            ("TEMDAYLAMP", "stock_green"),
            ("SNODAYLAMP", "stock_snow_day"),
        ];
        let corpus = crate::test_fixture::json("tools/spatial_oracle/building_light_fields.json");
        let stock_green = &corpus["rows"][0]["after"];
        assert_eq!(
            fields(before.object("INGRNLMP").unwrap().building_light()),
            golden_fields(stock_green)
        );
        // Exact map key shape from Storm. Fixed ART and the selected Battle
        // layer are real retail inputs, rather than a merged synthetic INI.
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(
                "[INREDLMP]\nWorkingSound=_Amb_BirdsTemperate1\n\
             [INGRNLMP]\nWorkingSound=_Amb_BirdsTemperate2\n\
             [INBLULMP]\nWorkingSound=_Amb_BirdsPark\n\
             [INYELWLAMP]\nWorkingSound=_Amb_BirdsJungle1\n\
             [TEMDAYLAMP]\nWorkingSound=_Amb_BirdsBeach\n\
             [SNODAYLAMP]\nWorkingSound=_Amb_BirdsHarbor\n",
            ),
        );
        let after =
            RuleSet::from_processed_rules(&layers.process_with_fixed_art(&art).unwrap()).unwrap();
        for (id, native_case) in lamps {
            let expected = corpus["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["name"] == native_case)
                .unwrap();
            assert_eq!(
                before
                    .object(id)
                    .unwrap()
                    .building_light()
                    .intensity_units(),
                expected["after"][1].as_i64().unwrap() as i32,
                "{id} stock"
            );
            assert_eq!(
                after.object(id).unwrap().building_light().intensity_units(),
                0,
                "{id} sound-only map section"
            );
        }
        let snow_day = corpus["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "stock_snow_day")
            .unwrap();
        assert_eq!(
            fields(before.object("SNODAYLAMP").unwrap().building_light()),
            golden_fields(&snow_day["after"])
        );
        assert_eq!(
            fields(after.object("INGRNLMP").unwrap().building_light()),
            golden_fields(&corpus["rows"][2]["after"])
        );
        assert_eq!(
            before.object("GALITE").unwrap().building_light(),
            after.object("GALITE").unwrap().building_light(),
            "a map without GALITE's section preserves its light"
        );
    }
}
