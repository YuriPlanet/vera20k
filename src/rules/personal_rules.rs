//! Optional VERA-specific source overrides, separate from player preferences.
//!
//! Compose before the existing Rules readers rather than adding a Process pass:
//! another pass can reset omitted fields and repeat native postprocessing.

use std::borrow::Cow;

use crate::rules::error::RulesError;
use crate::rules::ini_parser::IniFile;

#[derive(Clone, Debug)]
pub struct PersonalRules {
    patch: IniFile,
    override_map_rules: bool,
    /// The Options page's Terrain Deformation box (`[VERA] TerrainDeformation=`
    /// in VERA.INI). Each new scenario's root reads it, so it needs no restart.
    terrain_deformation: bool,
}

impl Default for PersonalRules {
    fn default() -> Self {
        Self {
            patch: IniFile::empty(),
            override_map_rules: false,
            terrain_deformation: false,
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
            terrain_deformation: false,
        })
    }

    pub fn set_terrain_deformation(&mut self, on: bool) {
        self.terrain_deformation = on;
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

    /// The selected root as each scenario reads it, unchanged while the
    /// Options switches are off. Terrain Deformation turns on
    /// `[CombatDamage] TerrainDeformation=` and gives the nuke's warhead each
    /// of `Deform=100%` and `DeformDepth=3` the root does not set, so a nuke
    /// digs a crater three levels deep: gamemd names that warhead `NUKE`
    /// (0x0081AF98), and retail sets neither. Map and mode rules still read
    /// after it and can turn it off.
    pub(crate) fn scenario_root<'a>(&self, root: &'a IniFile) -> Cow<'a, IniFile> {
        if !self.terrain_deformation {
            return Cow::Borrowed(root);
        }
        let mut options = String::from("[CombatDamage]\nTerrainDeformation=yes\n");
        if let Some(nuke) = root.section("NUKE") {
            let crater: String = [("Deform", "100%"), ("DeformDepth", "3")]
                .into_iter()
                .filter(|(key, _)| !nuke.is_present(key))
                .map(|(key, value)| format!("{key}={value}\n"))
                .collect();
            if !crater.is_empty() {
                options.push_str("[NUKE]\n");
                options.push_str(&crater);
            }
        }
        let mut root = root.clone();
        root.merge_source_sections(&IniFile::from_str(&options), true);
        Cow::Owned(root)
    }

    /// Network admission must compare the precedence policy and the Options
    /// switches as well as values. A comment-only file with the switches off
    /// has no effect, including on the compatibility hash.
    pub fn compatibility_crc(&self) -> Option<u32> {
        if self.is_empty() && !self.terrain_deformation {
            return None;
        }
        let policy = if self.override_map_rules { "yes" } else { "no" };
        let mut header = format!("[VERA]\nPersonalRulesVersion=1\nOverrideMapRules={policy}\n");
        if self.terrain_deformation {
            header.push_str("TerrainDeformation=yes\n");
        }
        let mut input = IniFile::from_str(&header);
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

    #[test]
    fn terrain_deformation_box_reaches_the_next_scenario_and_network_agreement() {
        let scenario = |root: &[u8], on: bool| {
            let mut owner = sources(root)
                .into_owner(Default::default(), Default::default())
                .unwrap();
            let digests = owner.multiplayer_source_digests();
            owner.set_terrain_deformation(on);
            let (rules, _, _, _) = owner
                .load_scenario(
                    NativeScenarioRulesPrefix::NonCampaign(None),
                    &IniFile::empty(),
                )
                .unwrap()
                .into_parts();
            let nuke = rules.warhead("NUKE").unwrap();
            (
                rules.combat_damage.terrain_deformation,
                (nuke.deform_f64, nuke.deform_depth),
                digests != owner.multiplayer_source_digests(),
                owner.personal_rules_compatibility_crc().is_some(),
            )
        };
        let root = b"[Warheads]\n0=NUKE\n[NUKE]\nVerses=100%\n";
        assert_eq!(scenario(root, false), (false, (0.0, 1), false, false));
        assert_eq!(scenario(root, true), (true, (1.0, 3), true, true));
        // Keys the root already sets, from a mod or VERARULES.INI, stay.
        let modded = b"[Warheads]\n0=NUKE\n[NUKE]\nDeform=25%\n";
        assert_eq!(scenario(modded, true).1, (0.25, 3));
        let modded = b"[Warheads]\n0=NUKE\n[NUKE]\nDeformDepth=2\n";
        assert_eq!(scenario(modded, true).1, (1.0, 2));
    }
}
