//! Retained BuildingType inputs for installing a building on another building.
//!
//! Active-retail gamemd.exe (Steam):
//! constructor45DD90 seeds the host's empty string at45E41C, level-1 at45E1FA,
//! and capacity0 at45DF31. ReadINI45FE50 reads the Rules section, not ART:
//! PowersUpBuilding460E17..460E65, PowersUpToLevel460E6C..460E80, then
//! Upgrades464727..46473D after the intervening fixed-ART animation reads.

use crate::rules::ini_parser::IniSection;

/// BuildingType F4C +44*slot. Constructor45E3BE..45E414 clears names and
/// coordinates;4647CD..464912 retains empty names and current integer defaults.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BuildingPowerUpArt {
    normal_name: String,
    damaged_name: String,
    location: [i32; 3],
    y_sort: i32,
}

impl BuildingPowerUpArt {
    pub fn normal_name(&self) -> &str {
        &self.normal_name
    }
    pub fn damaged_name(&self) -> Option<&str> {
        (!self.damaged_name.is_empty()).then_some(self.damaged_name.as_str())
    }
    pub const fn location(&self) -> [i32; 3] {
        self.location
    }
    pub const fn y_sort(&self) -> i32 {
        self.y_sort
    }

    /// Shared reader for retained Type state and immutable ART declarations.
    pub(super) fn read_pass(&mut self, section: &IniSection, slot: usize) {
        let prefix = format!("PowerUp{}", slot + 1);
        for (suffix, current) in [
            ("Anim", &mut self.normal_name),
            ("DamagedAnim", &mut self.damaged_name),
        ] {
            let value = section.read_string(&format!("{prefix}{suffix}"), "", 0x10);
            if !value.is_empty() {
                *current = value;
            }
        }
        for (suffix, current) in ["LocXX", "LocYY", "LocZZ"]
            .into_iter()
            .zip(&mut self.location)
        {
            *current = section.read_int(&format!("{prefix}{suffix}"), *current);
        }
        self.y_sort = section.read_int(&format!("{prefix}YSort"), self.y_sort);
    }
}

/// The live ProcessedType owns mutation; ObjectType exposes its projection.
/// Host names remain text until installation, as in the native 24-byte field.
/// Reading a host does not allocate another BuildingType.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BuildingUpgradeRules {
    host: String,
    level: i32,
    slots: i32,
    /// One authority for both fixed-ART reads and installation's name writes.
    #[serde(default)]
    power_up_art: [BuildingPowerUpArt; 3],
}

impl Default for BuildingUpgradeRules {
    fn default() -> Self {
        Self {
            host: String::new(),
            level: -1,
            slots: 0,
            power_up_art: Default::default(),
        }
    }
}

impl BuildingUpgradeRules {
    pub fn host(&self) -> Option<&str> {
        (!self.host.is_empty()).then_some(self.host.as_str())
    }

    /// Signed PowersUpToLevel, without a reader-side clamp.
    pub const fn level(&self) -> i32 {
        self.level
    }

    /// Signed Upgrades, without a reader-side clamp.
    pub const fn slots(&self) -> i32 {
        self.slots
    }

    pub fn normal_art_name(&self, slot: u8) -> Option<&str> {
        let name = self.power_up_art(slot)?.normal_name();
        (!name.is_empty()).then_some(name)
    }

    pub fn power_up_art(&self, slot: u8) -> Option<&BuildingPowerUpArt> {
        self.power_up_art.get(usize::from(slot))
    }

    /// The retirement transaction has validated this exact BuildingType and
    /// zero-based slot. The Simulation supplies the already-bounded name.
    pub(crate) fn set_normal_art_name(&mut self, slot: u8, name: String) {
        self.power_up_art[usize::from(slot)].normal_name = name;
    }

    /// Call only for an admitted, exact-case BuildingType Rules section.
    pub(crate) fn read_pass(&mut self, section: &IniSection, fixed_art: &IniSection) {
        //460E1B: capacity24 includes the terminating byte. Missing/empty
        // retains the previous field; `none` is literal text, not a factory.
        let host = section.read_string("PowersUpBuilding", "", 0x18);
        if !host.is_empty() {
            self.host = host;
        }
        self.level = section.read_int("PowersUpToLevel", self.level);
        self.slots = section.read_int("Upgrades", self.slots);
        //46473D..4647F4: only positive Upgrades reaches PowerUp1..N;
        // ReadString16 returns zero for absent/empty text and skips the copy.
        // Read only the three ordinary slots represented by this mechanism;
        // the signed Upgrades value itself remains unclamped.
        for (slot, record) in self
            .power_up_art
            .iter_mut()
            .enumerate()
            .take(self.slots.max(0) as usize)
        {
            record.read_pass(fixed_art, slot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::{ProcessedRulesLayers, RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;

    fn continue_process(processed: ProcessedRulesLayers, pass: &str) -> ProcessedRulesLayers {
        continue_process_with_art(processed, pass, "")
    }

    fn continue_process_with_art(
        processed: ProcessedRulesLayers,
        pass: &str,
        fixed_art: &str,
    ) -> ProcessedRulesLayers {
        let (_, trace) = processed.into_ini_and_native_type_construction_trace();
        let state = trace.into_registry_state_discarding_events();
        let saved = crate::util::save_codec::serialize(&state).unwrap();
        let restored = crate::util::save_codec::deserialize(&saved).unwrap();
        RulesLayerStack::new(IniFile::from_str(pass))
            .process_with_fixed_art_and_registry_state(&IniFile::from_str(fixed_art), restored)
            .unwrap()
    }

    #[test]
    fn runtime_normal_names_retain_empty_reads_until_a_reached_copy_or_type_reset() {
        use crate::rules::object_type::ObjectCategory;

        let initial = "[BuildingTypes]\n0=HOST\n[HOST]\nImage=ALT\nUpgrades=3\nTechLevel=4\n";
        let processed = RulesLayerStack::new(IniFile::from_str(initial))
            .process()
            .unwrap();
        let mut state = processed
            .into_ini_and_native_type_construction_trace()
            .1
            .into_registry_state_discarding_events();
        state
            .consume_outgoing_type_runtime_fields(
                [(ObjectCategory::Building, "HOST".to_owned(), 7)],
                [
                    ("HOST".to_owned(), 0, "RUNTIME".to_owned()),
                    ("HOST".to_owned(), 2, "THIRD".to_owned()),
                ],
                [],
            )
            .unwrap();
        assert!(
            state
                .consume_outgoing_type_runtime_fields(
                    [(ObjectCategory::Building, "HOST".to_owned(), 99)],
                    [
                        ("HOST".to_owned(), 0, "INVALID".to_owned()),
                        ("HOST".to_owned(), 3, "INVALID".to_owned())
                    ],
                    [],
                )
                .is_err()
        );
        let mut processed = RulesLayerStack::new(IniFile::empty())
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), state)
            .unwrap();
        for art in [
            "",
            "[HOST]\nPowerUp1Anim=WRONG_IMAGE\n[ALT]\nPowerUp1Anim=\n",
        ] {
            processed = continue_process_with_art(processed, "[HOST]\nStrength=100\n", art);
            let rules = RuleSet::from_processed_rules(&processed).unwrap();
            let host = rules.object("HOST").unwrap();
            assert_eq!(host.tech_level, 7, "invalid retirement is atomic");
            assert_eq!(host.building_upgrade().normal_art_name(0), Some("RUNTIME"));
        }
        processed = continue_process_with_art(
            processed,
            "[HOST]\nUpgrades=0\n",
            "[ALT]\nPowerUp1Anim=NOT_REACHED\n",
        );
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        assert_eq!(
            rules
                .object("HOST")
                .unwrap()
                .building_upgrade()
                .normal_art_name(0),
            Some("RUNTIME")
        );
        processed = continue_process_with_art(
            processed,
            "[HOST]\nUpgrades=1\n",
            "[ALT]\nPowerUp1Anim=FIXED\nPowerUp3Anim=NOT_REACHED\n",
        );
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        let host = rules.object("HOST").unwrap().building_upgrade();
        assert_eq!(host.normal_art_name(0), Some("FIXED"));
        assert_eq!(host.normal_art_name(2), Some("THIRD"));
        let reset = processed
            .into_ini_and_native_type_construction_trace()
            .1
            .into_registry_state_discarding_events()
            .destructive_reset();
        let fresh = RulesLayerStack::new(IniFile::from_str(initial))
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), reset)
            .unwrap();
        let rules = RuleSet::from_processed_rules(&fresh).unwrap();
        assert_eq!(
            rules
                .object("HOST")
                .unwrap()
                .building_upgrade()
                .normal_art_name(2),
            None
        );
    }

    #[test]
    fn exact_rules_keys_retain_prior_values_across_admitted_layers() {
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[BuildingTypes]\n0=UPGRADE\n[UPGRADE]\nStrength=100\n",
        ));
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(
            rules.object("UPGRADE").unwrap().building_upgrade(),
            &BuildingUpgradeRules::default()
        );
        layers.push(
            RulesLayerKind::LangRule,
            IniFile::from_str(
                "[UPGRADE]\nPowersUpBuilding=abcdefghijklmnopqrstuvwXYZ\n\
                 PowersUpToLevel=$2\nUpgrades=-2\n",
            ),
        );
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str(
                "[UPGRADE]\nPowersUpBuilding=\npowersuptolevel=9\nUpgrades=\nStrength=101\n",
            ),
        );
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[Other]\nFixture=1\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        let upgrade = rules.object("UPGRADE").unwrap().building_upgrade();
        assert_eq!(upgrade.host(), Some("abcdefghijklmnopqrstuvw"));
        assert_eq!(upgrade.level(), 2);
        assert_eq!(upgrade.slots(), -2);

        //460E3E tests only string length: an authored `none` is retained text.
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[UPGRADE]\nPowersUpBuilding=none\nPowersUpToLevel=0\nUpgrades=4\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        let upgrade = rules.object("UPGRADE").unwrap().building_upgrade();
        assert_eq!(
            (upgrade.host(), upgrade.level(), upgrade.slots()),
            (Some("none"), 0, 4)
        );
    }

    #[test]
    fn retained_power_up_records_follow_image_changes_and_capacity_gate() {
        let processed = RulesLayerStack::new(IniFile::from_str(
            "[BuildingTypes]\n0=HOST\n[HOST]\nImage=A\nUpgrades=1\n",
        ))
        .process_with_fixed_art(&IniFile::from_str(
            "[A]\nPowerUp1Anim=NORMAL\nPowerUp1DamagedAnim=OLD\nPowerUp1LocXX=-24\n\
             PowerUp1LocYY=-12\nPowerUp1LocZZ=-42\nPowerUp1YSort=50\n\
             PowerUp2Anim=NOT_REACHED\nPowerUp2LocXX=999\n",
        ))
        .unwrap();
        let initial = RuleSet::from_processed_rules(&processed).unwrap();
        let initial = initial.object("HOST").unwrap().building_upgrade().clone();
        let first = initial.power_up_art(0).unwrap();
        assert_eq!(first.normal_name(), "NORMAL");
        assert_eq!(first.damaged_name(), Some("OLD"));
        assert_eq!(first.location(), [-24, -12, -42]);
        assert_eq!(first.y_sort(), 50);
        assert_eq!(
            initial.power_up_art(1).unwrap(),
            &BuildingPowerUpArt::default()
        );

        let processed = continue_process_with_art(
            processed,
            "[HOST]\nImage=B\n",
            "[B]\nPowerUp1Anim=\nPowerUp1DamagedAnim=\nPowerUp2LocXX=9\n",
        );
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        assert_eq!(rules.object("HOST").unwrap().building_upgrade(), &initial);
        let processed = continue_process_with_art(
            processed,
            "[HOST]\nUpgrades=0\n",
            "[B]\nPowerUp1Anim=NOT_REACHED\nPowerUp1LocXX=100\n",
        );
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        assert_eq!(
            rules
                .object("HOST")
                .unwrap()
                .building_upgrade()
                .power_up_art(0),
            Some(first)
        );
        let processed = continue_process_with_art(
            processed,
            "[HOST]\nUpgrades=2\n",
            "[B]\nPowerUp2Anim=SECOND\nPowerUp2LocXX=9\n",
        );
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        let upgrade = rules.object("HOST").unwrap().building_upgrade();
        assert_eq!(upgrade.power_up_art(0), Some(first));
        assert_eq!(upgrade.power_up_art(1).unwrap().normal_name(), "SECOND");
        assert_eq!(upgrade.power_up_art(1).unwrap().location(), [9, 0, 0]);
        assert_eq!(
            upgrade.power_up_art(2).unwrap(),
            &BuildingPowerUpArt::default()
        );
    }

    #[test]
    fn host_text_does_not_allocate_or_read_an_orphan_building() {
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[BuildingTypes]\n0=UPGRADE\n[UPGRADE]\nPowersUpBuilding=HOST\n\
             [HOST]\nPower=500\nUpgrades=3\n",
        ));
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert!(rules.object("HOST").is_none());
        assert_eq!(
            rules.object("UPGRADE").unwrap().building_upgrade().host(),
            Some("HOST")
        );
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[BuildingTypes]\n0=HOST\n[HOST]\nStrength=200\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        let host = rules.object("HOST").unwrap();
        assert_eq!(host.power, 0);
        assert_eq!(host.building_upgrade().slots(), 0);
    }

    #[test]
    fn saved_registry_projects_retained_inputs_and_hashes_them() {
        let prior = RulesLayerStack::new(IniFile::from_str(
            "[BuildingTypes]\n0=UPGRADE\n[UPGRADE]\nPowersUpBuilding=HOST\n\
             PowersUpToLevel=3\nUpgrades=2\n",
        ))
        .process()
        .unwrap();
        let next = "[UPGRADE]\nStrength=100\n";
        let retained = RuleSet::from_processed_rules(&continue_process(prior, next)).unwrap();
        let cold = RulesLayerStack::new(IniFile::from_str("[BuildingTypes]\n0=UPGRADE\n"))
            .process()
            .unwrap();
        let cold = RuleSet::from_processed_rules(&continue_process(cold, next)).unwrap();
        let upgrade = retained.object("UPGRADE").unwrap().building_upgrade();
        assert_eq!(
            (upgrade.host(), upgrade.level(), upgrade.slots()),
            (Some("HOST"), 3, 2)
        );
        assert_eq!(retained.source_ini_hash(), cold.source_ini_hash());
        assert_ne!(
            retained.simulation_config_hash(),
            cold.simulation_config_hash()
        );
    }

    #[test]
    fn retail_power_plants_declare_capacity_without_upgrade_types() {
        let Some((rules, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
            return;
        };
        let processed = RulesLayerStack::new(rules)
            .process_with_fixed_art(&art)
            .unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        for id in ["GAPOWR", "YAPOWR"] {
            assert_eq!(
                rules.object(id).unwrap().building_upgrade().slots(),
                2,
                "{id}"
            );
        }
        assert!(
            rules
                .all_objects()
                .all(|object| object.building_upgrade().host().is_none())
        );
    }
}
