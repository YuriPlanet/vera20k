//! Optional VERA-specific source overrides, separate from player preferences.
//!
//! Compose before the existing Rules readers rather than adding a Process pass:
//! another pass can reset omitted fields and repeat native postprocessing.

use crate::rules::error::RulesError;
use crate::rules::ini_parser::IniFile;

#[derive(Clone, Debug)]
pub struct PersonalRules {
    patch: IniFile,
    override_map_rules: bool,
}

impl Default for PersonalRules {
    fn default() -> Self {
        Self {
            patch: IniFile::empty(),
            override_map_rules: false,
        }
    }
}

impl PersonalRules {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, RulesError> {
        let mut patch = IniFile::from_bytes(bytes)?;
        let override_map_rules = patch
            .section_or_empty("VERA")
            .read_bool("OverrideMapRules", false);
        patch.remove_section("VERA");
        Ok(Self {
            patch,
            override_map_rules,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.patch.is_empty()
    }

    pub(crate) fn apply_root(&self, root: &mut IniFile) {
        if !self.is_empty() {
            root.merge_source_sections(&self.patch, true);
        }
    }

    pub(crate) fn apply_base_layer(&self, layer: &mut IniFile) {
        if !self.is_empty() {
            layer.merge_source_sections(&self.patch, false);
        }
    }

    pub(crate) fn apply_later_layer(&self, layer: &mut IniFile) {
        if self.override_map_rules {
            self.apply_base_layer(layer);
        }
    }

    /// Network admission must compare the precedence policy as well as values.
    /// A comment-only file has no effect, including on the compatibility hash.
    pub fn compatibility_crc(&self) -> Option<u32> {
        if self.is_empty() {
            return None;
        }
        let policy = if self.override_map_rules { "yes" } else { "no" };
        let mut input = IniFile::from_str(&format!(
            "[VERA]\nPersonalRulesVersion=1\nOverrideMapRules={policy}\n"
        ));
        input.merge_source_sections(&self.patch, true);
        Some(input.native_crc())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::asset_manager::AssetManager;
    use crate::map::source::test_support::TestDirectory;
    use crate::rules::native_processing::ProcessedRulesLayers;
    use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
    use crate::rules::retail_sources::RetailRulesSources;

    const ROOT: &[u8] = b"[VehicleTypes]\n0=TEST\n[TEST]\nStrength=100\nCost=500\nCost=600\nROOT=First\nROOT=Second\nSHOW=Show\n[Easy]\nFirePower=1.5\nArmor=.5\n[Difficult]\nFirePower=1.5\nArmor=.4\n[Probe]\nValue=First\n[Probe]\nValue=Second\n";

    fn sources(root: &[u8]) -> RetailRulesSources {
        let directory = TestDirectory::new("personal-rules");
        directory.write("rulesmd.ini", root);
        directory.write("langrule.ini", b"[TEST]\nStrength=125\n");
        directory.write("artmd.ini", b"[TEST]\nImage=TEST\n");
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        RetailRulesSources::select(&assets).unwrap()
    }

    fn owner(personal: &str) -> NativeRulesProcessOwner {
        sources(ROOT)
            .with_personal_rules(PersonalRules::from_bytes(personal.as_bytes()).unwrap())
            .into_owner(Default::default(), Default::default())
            .unwrap()
    }

    #[test]
    fn comment_only_profile_preserves_selected_sources_and_compatibility() {
        let root = b"[Probe]\nValue=First\n[Probe]\nValue=Second\n";
        let selected = sources(root);
        let original = selected
            .clone()
            .into_owner(Default::default(), Default::default())
            .unwrap();
        let comments = selected
            .with_personal_rules(
                PersonalRules::from_bytes(
                    b";[TEST]\n;Strength=200\n[VERA]\nOverrideMapRules=yes\n",
                )
                .unwrap(),
            )
            .into_owner(Default::default(), Default::default())
            .unwrap();
        assert_eq!(comments.personal_rules_compatibility_crc(), None);
        assert_eq!(
            original.selected_source_ini_hashes(),
            comments.selected_source_ini_hashes()
        );
        assert_eq!(
            original.selected_rules_root().native_crc(),
            comments.selected_rules_root().native_crc()
        );
        assert_eq!(
            original
                .selected_rules_root()
                .section("Probe")
                .unwrap()
                .read_string("Value", "", 32),
            comments
                .selected_rules_root()
                .section("Probe")
                .unwrap()
                .read_string("Value", "", 32)
        );
        assert!(comments.selected_rules_root().section("VERA").is_none());
    }

    #[test]
    fn base_overrides_preserve_later_pass_priority_and_original_sources() {
        let patch = "[TEST]\nStrength=200\nCost=250\nNewKey=yes\n[NewSection]\nNewKey=yes\n";
        let original = owner("");
        let mut customized = owner(patch);
        assert_eq!(
            customized
                .selected_rules_root()
                .section("TEST")
                .unwrap()
                .read_int("Strength", 0),
            200
        );
        for (section, key) in [("Probe", "Value"), ("TEST", "ROOT")] {
            assert_eq!(
                original
                    .selected_rules_root()
                    .section(section)
                    .unwrap()
                    .read_string(key, "", 32),
                customized
                    .selected_rules_root()
                    .section(section)
                    .unwrap()
                    .read_string(key, "", 32)
            );
        }
        assert!(
            customized
                .selected_rules_root()
                .section("TEST")
                .unwrap()
                .read_bool("NewKey", false)
        );
        assert!(
            customized
                .selected_rules_root()
                .section("NewSection")
                .unwrap()
                .read_bool("NewKey", false)
        );
        let loaded = customized
            .load_scenario(
                NativeScenarioRulesPrefix::NonCampaign(None),
                &IniFile::empty(),
            )
            .unwrap();
        assert_eq!(loaded.into_parts().0.object("TEST").unwrap().strength, 200);
        let mode = IniFile::from_str("[TEST]\nStrength=150\n");
        let map = IniFile::from_str("[TEST]\nStrength=175\n");
        let loaded = customized
            .load_scenario(NativeScenarioRulesPrefix::NonCampaign(Some(&mode)), &map)
            .unwrap();
        let (rules, _, _, _) = loaded.into_parts();
        let unit = rules.object("TEST").unwrap();
        assert_eq!(unit.strength, 175);
        assert_eq!(unit.cost, 250);
        assert_eq!(mode.section("TEST").unwrap().read_int("Strength", 0), 150);
        assert_eq!(map.section("TEST").unwrap().read_int("Strength", 0), 175);
    }

    #[test]
    fn forced_campaign_overrides_preserve_section_absence_and_saved_rules() {
        let patch = "[VERA]\nOverrideMapRules=yes\n[TEST]\nStrength=200\n[Easy]\nFirePower=2\n[Difficult]\nFirePower=2\n";
        let mut owner = owner(patch);
        let named = IniFile::from_str("[TEST]\nStrength=150\n");
        let map = IniFile::from_str("[TEST]\nStrength=175\n[Easy]\nCost=.9\n");
        let loaded = owner
            .load_scenario(NativeScenarioRulesPrefix::Campaign(Some(&named)), &map)
            .unwrap();
        let (rules, _, _, _) = loaded.into_parts();
        assert_eq!(rules.object("TEST").unwrap().strength, 200);
        assert_eq!(rules.general.difficulty_rows[0].firepower, 2.0);
        // The present Easy section still applies its own omitted-key defaults.
        assert_eq!(rules.general.difficulty_rows[0].armor, 1.0);
        // No new Difficult section was injected into the map to reset Armor.
        assert_eq!(rules.general.difficulty_rows[2].firepower, 2.0);
        assert_eq!(rules.general.difficulty_rows[2].armor, f64::from(0.4_f32));
        assert_eq!(map.section("TEST").unwrap().read_int("Strength", 0), 175);
        assert!(map.section("Difficult").is_none());
        let encoded =
            crate::util::save_codec::serialize(owner.saved_scenario_rules().unwrap()).unwrap();
        let saved: ProcessedRulesLayers = crate::util::save_codec::deserialize(&encoded).unwrap();
        let restored = owner.project_saved_scenario_rules(&saved).unwrap();
        assert_eq!(
            restored.simulation_config_hash(),
            rules.simulation_config_hash()
        );
        let base = PersonalRules::from_bytes(b"[TEST]\nStrength=200\n").unwrap();
        let force =
            PersonalRules::from_bytes(b"[VERA]\nOverrideMapRules=yes\n[TEST]\nStrength=200\n")
                .unwrap();
        assert_ne!(base.compatibility_crc(), force.compatibility_crc());
    }
}
