//! Source-ordered `[ColorAdd]` rules data retained for later rendering consumers.
//!
//! The three channel values are raw RGB565 magnitudes, not display RGB bytes.
//! This module assigns no consumer meaning to the fixed native slots.

use crate::rules::ini_parser::{IniFile, IniSection};

/// Triples reserved before RulesClass's four AudioVisual color selectors.
pub const COLOR_ADD_SLOT_COUNT: usize = 16;

/// One source entry in the fixed `[ColorAdd]` table.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct ColorAddEntry {
    /// Exact key spelling retained from the source section; `None` marks an unused tail slot.
    pub name: Option<String>,
    /// Raw RGB565 channel magnitudes. These values are deliberately not normalized.
    pub rgb: [u8; 3],
}

/// Rules-owned fixed `[ColorAdd]` table in source declaration order.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ColorAddTable {
    slots: [ColorAddEntry; COLOR_ADD_SLOT_COUNT],
}

impl Default for ColorAddTable {
    fn default() -> Self {
        // RulesClass665650 initializes all16 triples to black before Process.
        // Original prefilled constructor control: tools/berserk_color.json.
        Self {
            slots: std::array::from_fn(|_| ColorAddEntry::default()),
        }
    }
}

impl ColorAddTable {
    /// Parse at most 16 source entries, leaving the unused tail zero-filled.
    #[cfg(any(test, feature = "test-support"))]
    pub fn from_ini(ini: &IniFile) -> Self {
        let mut table = Self::default();
        table.apply_pass(ini);
        table
    }

    /// ReadColorAdd66D480, Process668C03..668C8B and Reset668B13..668B8E
    /// enumerate the current section from slot 0, retaining its current tail.
    /// Full Reset clears the table before LANG and that final root loop.
    /// Names are diagnostic; native consumers use only slot indexes.
    /// RESIDUAL: native enumeration has no 16-entry clamp and overwrites
    /// adjacent Rules bytes on longer custom sections; VERA truncates to the
    /// actual table to preserve memory safety. Stock has 14 entries. Consumed
    /// original controls, including entry 17 overwriting LaserTargetColor:
    /// tools/berserk_color.json.
    pub(crate) fn apply_pass(&mut self, ini: &IniFile) {
        let Some(section) = ini.section("ColorAdd") else {
            return;
        };
        // The original loop reuses ReadColorRGB474B50's scanf destinations.
        // RESIDUAL: before its first successful scan these are uninitialized
        // native stack words. VERA starts them at 0 for deterministic recovery;
        // subsequent incomplete scans retain native per-channel carry.
        let mut scan_locals = [0; 3];
        for (slot, key) in self.slots.iter_mut().zip(section.keys()) {
            *slot = ColorAddEntry {
                name: Some(key.to_string()),
                rgb: section.read_color_rgb(key, [0; 3], Some(&mut scan_locals)),
            };
        }
    }

    /// Read-only source order; mutations remain with the retained rules owner.
    #[cfg(any(test, feature = "test-support"))]
    pub fn entries(&self) -> &[ColorAddEntry; COLOR_ADD_SLOT_COUNT] {
        &self.slots
    }

    /// The colour word a draw ORs into its pixels for the entry at `index`, as
    /// `BuildingClass_DrawBody` converts it (`0x0043D44F..0x0043D4E9`; its other
    /// readers repeat the conversion): the RGB565 arm of the pixel format
    /// `0x004BBC90` reports (format 2, the active retail one) falls through the
    /// other two, so the word is `R<<11 | G<<5 | B`, `R<<11 | (G>>1)<<6 | B` and
    /// `R<<10 | (G>>1)<<5 | B` together, of which the blitters OR the low 16
    /// bits (`0x00494CD9`). Executed: `tools/superweapon_oracle.py`
    /// `building_colour_word`.
    ///
    /// RESIDUAL: an index outside the table reads the Rules bytes after it
    /// natively; VERA answers 0. Retail's indexes are all inside it.
    pub fn rgb565_word(&self, index: i32) -> u16 {
        let Some(entry) = usize::try_from(index).ok().and_then(|i| self.slots.get(i)) else {
            return 0;
        };
        let [r, g, b] = entry.rgb.map(u32::from);
        let word =
            (r << 11 | g << 5 | b) | (r << 11 | (g >> 1) << 6 | b) | (r << 10 | (g >> 1) << 5 | b);
        word as u16
    }
}

/// ReadAudioVisual6691E0's signed ColorAdd indexes (Rules+18A4..18B0).
/// The live registry owns their process lifetime; RuleSet receives a projection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ColorAddSelectors {
    laser_target_color: i32,
    iron_curtain_color: i32,
    berserk_color: i32,
    force_shield_color: i32,
}

impl ColorAddSelectors {
    /// Constructor6678AA/6678B0/6678B6/6678C1 initializes all four to0.
    /// AudioVisual66B818..66B891 reads exact-case keys in this order with
    /// each retained field as its ReadInt5276D0 default; there is no clamp.
    /// Original constructor/reader controls: tools/berserk_color.json.
    pub(crate) fn read_pass(&mut self, section: &IniSection) {
        self.laser_target_color = section.read_int("LaserTargetColor", self.laser_target_color);
        self.iron_curtain_color = section.read_int("IronCurtainColor", self.iron_curtain_color);
        self.berserk_color = section.read_int("BerserkColor", self.berserk_color);
        self.force_shield_color = section.read_int("ForceShieldColor", self.force_shield_color);
    }

    pub(crate) fn laser_target_color(self) -> i32 {
        self.laser_target_color
    }
    pub(crate) fn iron_curtain_color(self) -> i32 {
        self.iron_curtain_color
    }
    pub(crate) fn berserk_color(self) -> i32 {
        self.berserk_color
    }
    pub(crate) fn force_shield_color(self) -> i32 {
        self.force_shield_color
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native_rules() -> serde_json::Value {
        serde_json::from_str(crate::test_fixture::text("tools/berserk_color.json")).unwrap()
    }

    fn native_ini_sections(
        sections: &serde_json::Value,
        color_add_key_order: &serde_json::Value,
    ) -> IniFile {
        let mut ini = IniFile::empty();
        for (name, entries) in sections.as_object().unwrap() {
            let mut section = IniSection::new(name.clone());
            let keys: Vec<_> = if name == "ColorAdd" {
                color_add_key_order
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|key| key.as_str().unwrap())
                    .collect()
            } else {
                entries
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect()
            };
            for key in keys {
                // set deliberately retains stored-empty cache controls. The
                // physical native/Rust INI loaders drop those values instead.
                section.set(key, entries[key].as_str().unwrap());
            }
            ini.replace_first_section(section);
        }
        ini
    }

    fn native_ini(row: &serde_json::Value) -> IniFile {
        native_ini_sections(&row["sections"], &row["color_add_key_order"])
    }

    fn native_state(fields: &serde_json::Value) -> (ColorAddTable, ColorAddSelectors) {
        let mut table = ColorAddTable::default();
        for (slot, rgb) in table
            .slots
            .iter_mut()
            .zip(fields["color_add"].as_array().unwrap())
        {
            slot.rgb = std::array::from_fn(|i| rgb[i].as_u64().unwrap() as u8);
        }
        let indexes = &fields["selectors"];
        let read = |key: &str| indexes[key].as_i64().unwrap() as i32;
        (
            table,
            ColorAddSelectors {
                laser_target_color: read("LaserTargetColor"),
                iron_curtain_color: read("IronCurtainColor"),
                berserk_color: read("BerserkColor"),
                force_shield_color: read("ForceShieldColor"),
            },
        )
    }

    fn assert_native_state(
        table: &ColorAddTable,
        indexes: ColorAddSelectors,
        expected: &serde_json::Value,
    ) {
        let (native, native_indexes) = native_state(expected);
        assert_eq!(
            table.slots.each_ref().map(|slot| slot.rgb),
            native.slots.each_ref().map(|slot| slot.rgb)
        );
        assert_eq!(indexes, native_indexes);
    }

    fn assert_native_rules(rules: &crate::rules::ruleset::RuleSet, expected: &serde_json::Value) {
        assert_native_state(
            &rules.color_add,
            ColorAddSelectors {
                laser_target_color: rules.general.laser_target_color,
                iron_curtain_color: rules.general.iron_curtain_color,
                berserk_color: rules.general.berserk_color,
                force_shield_color: rules.general.force_shield_color,
            },
            expected,
        );
    }

    #[test]
    fn successive_scenario_resets_match_original_color_lifetime() {
        use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};

        // Whole original Reset6686C0 reaches root Process668A27, zero16 at
        // 668A34..668A56, LANG668B05, root-only ColorAdd668B13..668B8E,
        // then mode668BAA. The preceding scenario's map seeds every slot and
        // selector through the production owner before that next reset.
        let fixture = native_rules();
        let history = &fixture["rules"]["reset_history"];
        let seed = native_ini_sections(
            &history["seed_sections"],
            &history["seed_color_add_key_order"],
        );
        let mut compared = 0;
        for row in history["cases"].as_array().unwrap() {
            let lang_file = &row["lang_file"];
            let lang_accepted = lang_file["read"]
                .as_array()
                .unwrap()
                .iter()
                .any(|read| read["status"] == 1);
            // Inputs here are already-selected INI objects. Native file
            // rejection and the separate campaign mode gate are not replayed
            // by this test of the ordinary noncampaign rules lifetime.
            if row["game_mode"] != 5 || (!lang_file["absent"].as_bool().unwrap() && !lang_accepted)
            {
                continue;
            }
            let root = native_ini_sections(&row["root_sections"], &row["root_color_add_key_order"]);
            let lang =
                lang_accepted.then(|| IniFile::from_str(lang_file["ascii"].as_str().unwrap()));
            let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
                root,
                lang,
                IniFile::empty(),
                std::sync::Arc::default(),
                Default::default(),
            )
            .unwrap();
            let (first, _, _, _) = owner
                .load_scenario(NativeScenarioRulesPrefix::NonCampaign(None), &seed)
                .unwrap()
                .into_parts();
            assert_native_rules(&first, &row["before"]);
            let mode = (!row["mode_sections"].is_null()).then(|| {
                native_ini_sections(&row["mode_sections"], &row["mode_color_add_key_order"])
            });
            let (second, _, _, _) = owner
                .load_scenario(
                    NativeScenarioRulesPrefix::NonCampaign(mode.as_ref()),
                    &IniFile::empty(),
                )
                .unwrap()
                .into_parts();
            assert_native_rules(&second, &row["after"]);
            compared += 1;
        }
        assert_eq!(compared, 8);
    }

    #[test]
    fn constructor_and_all_reached_color_passes_match_original() {
        let fixture = native_rules();
        let rules = &fixture["rules"];
        assert_native_state(
            &ColorAddTable::default(),
            ColorAddSelectors::default(),
            &rules["constructor"],
        );
        let rows = rules["physical"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|file| {
                file.get("stages")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
            })
            .chain(rules["controls"].as_array().unwrap());
        let mut compared = 0;
        for row in rows {
            let (mut table, mut indexes) = native_state(&row["before"]);
            let ini = native_ini(row);
            table.apply_pass(&ini);
            if row["phase"] != "reset_root" {
                if let Some(section) = ini.section("AudioVisual") {
                    indexes.read_pass(section);
                }
            }
            assert_native_state(&table, indexes, &row["after"]);
            compared += 1;
        }
        assert_eq!(compared, 16);
    }

    #[test]
    fn color_add_scan_locals_match_original_including_empty_and_partial_entries() {
        let fixture = native_rules();
        let rows = fixture["rules"]["malformed_rgb"].as_array().unwrap();
        assert_eq!(rows.len(), 21);
        for row in rows {
            let ini = native_ini(row);
            let section = ini.section("ColorAdd").unwrap();
            let mut locals =
                std::array::from_fn(|i| row["first_scanf_local_prior"][i].as_i64().unwrap() as i32);
            for (index, key) in section.keys().enumerate() {
                let actual = section.read_color_rgb(key, [0; 3], Some(&mut locals));
                let expected: [u8; 3] = std::array::from_fn(|i| {
                    row["after"]["color_add"][index][i].as_u64().unwrap() as u8
                });
                assert_eq!(
                    actual, expected,
                    "{} {} entry{index}",
                    row["phase"], row["name"]
                );
            }
            // Original474BDC packs green into the red scratch word's second
            // byte after scanning. Only the low bytes feed returned channels;
            // empty entries leave those reusable channels untouched.
            if let Some(scan) = row["scans"].as_array().unwrap().last() {
                for i in 0..3 {
                    assert_eq!(
                        locals[i] as u8,
                        scan["after"][i].as_u64().unwrap() as u8,
                        "{} {} channel{i}",
                        row["phase"],
                        row["name"]
                    );
                }
            }
        }
    }

    #[test]
    fn longer_custom_color_add_sections_keep_the_native_overflow_explicit() {
        let fixture = native_rules();
        let row = &fixture["rules"]["overflow17"];
        let (mut table, indexes) = native_state(&row["before"]);
        table.apply_pass(&native_ini(row));
        let (expected_table, overflow_indexes) = native_state(&row["after"]);
        assert_eq!(
            table.slots.each_ref().map(|slot| slot.rgb),
            expected_table.slots.each_ref().map(|slot| slot.rgb)
        );
        // Safe deterministic recovery: native writes the17th entry across
        // LaserTargetColor's low3 bytes. VERA retains that selector instead.
        assert_ne!(
            indexes.laser_target_color(),
            overflow_indexes.laser_target_color()
        );
    }

    #[test]
    fn retained_color_defaults_survive_serialization_handoff() {
        use crate::rules::native_processing::RulesLayerStack;
        use crate::rules::ruleset::RuleSet;
        let root = IniFile::from_str(
            "[ColorAdd]\nA=1,2,3\nB=4,5,6\nC=7,8,9\n[AudioVisual]\nLaserTargetColor=4\nIronCurtainColor=9\nBerserkColor=4\nForceShieldColor=6\n",
        );
        let processed = RulesLayerStack::new(root).process().unwrap();
        let saved: crate::rules::native_processing::ProcessedRulesLayers =
            crate::util::save_codec::deserialize(
                &crate::util::save_codec::serialize(&processed).unwrap(),
            )
            .unwrap();
        let (_, trace) = saved.into_ini_and_native_type_construction_trace();
        let registry = trace.into_registry_state_discarding_events();
        let continued = RulesLayerStack::new(IniFile::from_str("[AudioVisual]\nberserkcolor=99\n"))
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), registry)
            .unwrap();
        let rules = RuleSet::from_processed_rules(&continued).unwrap();
        assert_eq!(rules.general.berserk_color, 4);
        assert_eq!(rules.general.iron_curtain_color, 9);
        assert_eq!(rules.general.laser_target_color, 4);
        assert_eq!(rules.general.force_shield_color, 6);
        assert_eq!(rules.color_add.slots[1].rgb, [4, 5, 6]);
    }

    #[test]
    fn later_rules_color_add_replaces_prefix_and_retains_tail() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[ColorAdd]\nFirst=1,2,3\nSecond=4,5,6\nThird=7,8,9\n",
        ));
        // ReadColorAdd66D480 and Process668C03..668C8B restart at index0
        // for this section; they retain all slots after its last entry.
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[ColorAdd]\nSecond=0,0,24\n"),
        );
        layers.push(RulesLayerKind::Scenario, IniFile::empty());
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(rules.color_add.slots[0].rgb, [0, 0, 24]);
        assert_eq!(rules.color_add.slots[1].rgb, [4, 5, 6]);
        assert_eq!(rules.color_add.slots[2].rgb, [7, 8, 9]);
    }
}
