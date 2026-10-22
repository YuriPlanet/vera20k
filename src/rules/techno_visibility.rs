//! Retained TechnoType visibility bytes and BuildingType's visibility postpass.
//!
//! Techno ctor7113D8/DE and Building ctor45E206 clear these three bytes.
//! ReadINI714A97..714ACC reads the current Invisible/RadarVisible defaults;
//! Building460DEB..460E10 then reads InvisibleInGame and, when true, forces
//! Invisible=true and RadarVisible=false. Clearing InvisibleInGame in a later
//! pass does not undo those writes. A final merged-key OR loses that history.

use crate::rules::ini_parser::IniSection;

#[derive(Debug, Default, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub(crate) struct TechnoVisibility {
    pub invisible: bool,
    pub radar_visible: bool,
    pub invisible_in_game: bool,
}

impl TechnoVisibility {
    pub(crate) fn read_techno_pass(&mut self, section: &IniSection) {
        self.invisible = section.read_bool("Invisible", self.invisible);
        self.radar_visible = section.read_bool("RadarVisible", self.radar_visible);
    }

    pub(crate) fn read_building_pass(&mut self, section: &IniSection) {
        self.invisible_in_game = section.read_bool("InvisibleInGame", self.invisible_in_game);
        if self.invisible_in_game {
            self.invisible = true;
            self.radar_visible = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::object_type::ObjectType;
    use crate::rules::ruleset::RuleSet;

    fn fields(object: &ObjectType) -> [bool; 3] {
        [
            object.invisible,
            object.radar_visible,
            object.invisible_in_game,
        ]
    }

    #[test]
    fn building_visibility_history_survives_clearing_the_key_and_registry_restore() {
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[BuildingTypes]\n0=HIDDEN\n[VehicleTypes]\n0=DOT\n\
             [HIDDEN]\nInvisible=no\nRadarVisible=yes\nInvisibleInGame=yes\n\
             [DOT]\nInvisible=no\nRadarVisible=yes\nInvisibleInGame=yes\n",
        ));
        let rules = RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap();
        assert_eq!(fields(rules.object("HIDDEN").unwrap()), [true, false, true]);
        assert_eq!(fields(rules.object("DOT").unwrap()), [false, true, false]);

        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[HIDDEN]\nInvisibleInGame=no\n"),
        );
        let processed = layers.process().unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        assert_eq!(
            fields(rules.object("HIDDEN").unwrap()),
            [true, false, false]
        );
        let (_, trace) = processed.into_ini_and_native_type_construction_trace();
        let state = trace.into_registry_state_discarding_events();
        let saved = crate::util::save_codec::serialize(&state).unwrap();
        let loaded = crate::util::save_codec::deserialize(&saved).unwrap();
        let restored = RulesLayerStack::new(IniFile::from_str("[Unrelated]\nX=1\n"))
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), loaded)
            .unwrap();
        let retained_rules = RuleSet::from_processed_rules(&restored).unwrap();
        assert_eq!(
            fields(retained_rules.object("HIDDEN").unwrap()),
            [true, false, false]
        );

        let (_, trace) = restored.into_ini_and_native_type_construction_trace();
        let reset = RulesLayerStack::new(IniFile::from_str(
            "[HIDDEN]\nInvisible=no\nRadarVisible=yes\n",
        ))
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            trace.into_registry_state_discarding_events(),
        )
        .unwrap();
        let rules = RuleSet::from_processed_rules(&reset).unwrap();
        assert_eq!(
            fields(rules.object("HIDDEN").unwrap()),
            [false, true, false]
        );
        let (_, trace) = reset.into_ini_and_native_type_construction_trace();
        let reset = RulesLayerStack::new(IniFile::from_str("[Unrelated]\nX=1\n"))
            .process_with_fixed_art_and_registry_state(
                &IniFile::empty(),
                trace.into_registry_state_discarding_events(),
            )
            .unwrap();
        let reset_rules = RuleSet::from_processed_rules(&reset).unwrap();
        assert_eq!(
            retained_rules.source_ini_hash(),
            reset_rules.source_ini_hash()
        );
        assert_ne!(
            retained_rules.simulation_config_hash(),
            reset_rules.simulation_config_hash(),
            "equal current source text must not hide different retained visibility"
        );
    }
}
