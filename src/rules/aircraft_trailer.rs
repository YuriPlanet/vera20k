//! AircraftType's retained fixed-ART trailer and global-frame cadence.

/// AircraftType ctor41C8B0 stores null at+E00 (41C9B3) and3 at+E04
/// (41C9B9). ReadINI41CC20 replaces them only on an admitted Rules read,
/// using the current Rules Image's fixed ART section (41CCEF..41CD4E).
/// Save41CE90 includes both fields; Load41CE20 swizzles the animation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AircraftTrailer {
    animation: Option<String>,
    spawn_delay: i32,
}

impl Default for AircraftTrailer {
    fn default() -> Self {
        Self {
            animation: None,
            spawn_delay: 3,
        }
    }
}

impl AircraftTrailer {
    /// Resolved AnimType slot identity, including the registry's alias key.
    pub fn animation(&self) -> Option<&str> {
        self.animation.as_deref()
    }

    /// Signed ReadInt value; native's reached IDIV is not clamped at load.
    pub fn spawn_delay(&self) -> i32 {
        self.spawn_delay
    }

    pub(crate) fn apply_read(&mut self, animation: Option<Option<String>>, spawn_delay: i32) {
        if let Some(animation) = animation {
            self.animation = animation;
        }
        self.spawn_delay = spawn_delay;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::{
        ProcessedRulesLayers, RulesLayerStack, anim_type_runtime_key,
    };
    use crate::rules::ruleset::RuleSet;
    use crate::util::save_codec;

    #[test]
    fn aircraft_trailer_retains_admitted_art_reads_slot_identity_and_saved_history() {
        let root = |image: &str| {
            IniFile::from_str(&format!(
                "[AircraftTypes]\n0=PLANE\n1=COLD\n[VehicleTypes]\n0=TANK\n\
                 [Animations]\n0=TRAIL\n[PLANE]\nImage={image}\n\
                 [TANK]\nImage=FIRST\n"
            ))
        };
        let art = IniFile::from_str(
            "[TRAIL]\nRate=7\n[FIRST]\nTrailer=TRAIL\nSpawnDelay=-2\n\
             Image=DECOY\n[DECOY]\nTrailer=WRONG\nSpawnDelay=99\n\
             [EMPTY]\nTrailer=\nSpawnDelay=$invalid\n[NULL]\nTrailer=<none>\n",
        );
        let fields = |rules: &RuleSet, name: &str| {
            let trailer = rules.object(name).unwrap().aircraft_trailer();
            (
                trailer.animation().map(str::to_owned),
                trailer.spawn_delay(),
            )
        };
        let first = RulesLayerStack::new(root("FIRST"))
            .process_with_fixed_art(&art)
            .unwrap();
        let rules = RuleSet::from_processed_rules(&first).unwrap();
        assert_eq!(fields(&rules, "PLANE"), (Some("TRAIL".into()), -2));
        assert_eq!(fields(&rules, "COLD"), (None, 3));
        assert_eq!(fields(&rules, "TANK"), (None, 3));

        let loaded: ProcessedRulesLayers =
            save_codec::deserialize(&save_codec::serialize(&first).unwrap()).unwrap();
        let (_, trace) = loaded.into_ini_and_native_type_construction_trace();
        let retained = RulesLayerStack::new(root("EMPTY"))
            .process_with_fixed_art_and_registry_state(
                &art,
                trace.into_registry_state_discarding_events(),
            )
            .unwrap();
        let retained_rules = RuleSet::from_processed_rules(&retained).unwrap();
        let fresh = RuleSet::from_processed_rules(
            &RulesLayerStack::new(root("EMPTY"))
                .process_with_fixed_art(&art)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(fields(&retained_rules, "PLANE"), (Some("TRAIL".into()), -2));
        assert_eq!(fields(&fresh, "PLANE"), (None, 3));
        assert_eq!(retained_rules.source_ini_hash(), fresh.source_ini_hash());
        assert_ne!(
            retained_rules.simulation_config_hash(),
            fresh.simulation_config_hash()
        );

        let (_, trace) = retained.into_ini_and_native_type_construction_trace();
        let cleared = RulesLayerStack::new(root("NULL"))
            .process_with_fixed_art_and_registry_state(
                &art,
                trace.into_registry_state_discarding_events(),
            )
            .unwrap();
        assert_eq!(
            fields(&RuleSet::from_processed_rules(&cleared).unwrap(), "PLANE"),
            (None, -2)
        );
        let (_, trace) = cleared.into_ini_and_native_type_construction_trace();
        let reset = RulesLayerStack::new(root("EMPTY"))
            .process_with_fixed_art_and_registry_state(
                &art,
                trace
                    .into_registry_state_discarding_events()
                    .destructive_reset(),
            )
            .unwrap();
        assert_eq!(
            fields(&RuleSet::from_processed_rules(&reset).unwrap(), "PLANE"),
            (None, 3)
        );

        // Aircraft reads follow the Anim sweep: the full incoming name misses
        // its truncated explicit slot and creates an unread second AnimType.
        let prefix = "ABCDEFGHIJKLMNOPQRSTUVWX";
        let alias_root = IniFile::from_str(&format!(
            "[AircraftTypes]\n0=PLANE\n[PLANE]\nImage=BODY\n\
             [Animations]\n0={prefix}-TRAIL\n"
        ));
        let alias_art = IniFile::from_str(&format!(
            "[{prefix}]\nRate=7\n[BODY]\nTrailer={prefix}-TRAIL\n"
        ));
        let alias = RulesLayerStack::new(alias_root)
            .process_with_fixed_art(&alias_art)
            .unwrap();
        let matching: Vec<_> = alias
            .anim_type_art_read_states()
            .enumerate()
            .filter(|(_, (name, _))| *name == prefix)
            .collect();
        assert_eq!(matching.len(), 2);
        assert!(matching[0].1.1);
        assert!(!matching[1].1.1);
        let names: Vec<_> = alias
            .anim_type_art_read_states()
            .map(|(name, _)| name)
            .collect();
        let key = anim_type_runtime_key(names.iter().copied(), matching[1].0)
            .unwrap()
            .into_owned();
        let mut loaded: ProcessedRulesLayers =
            save_codec::deserialize(&save_codec::serialize(&alias).unwrap()).unwrap();
        loaded.restore_object_image_loads("TEMPERATE").unwrap();
        let rules = RuleSet::from_processed_rules(&loaded).unwrap();
        assert_eq!(fields(&rules, "PLANE"), (Some(key.clone()), 3));
        assert!(
            crate::rules::effect_asset_catalog::anim_class_roots(&rules)
                .contains(&key.to_ascii_uppercase())
        );
        let (_, trace) = loaded.into_ini_and_native_type_construction_trace();
        let absent = RulesLayerStack::new(IniFile::from_str("[OTHER]\nName=unrelated\n"))
            .process_with_fixed_art_and_registry_state(
                &alias_art,
                trace.into_registry_state_discarding_events(),
            )
            .unwrap();
        assert_eq!(
            fields(&RuleSet::from_processed_rules(&absent).unwrap(), "PLANE"),
            (Some(key), 3)
        );
        assert_eq!(
            absent
                .anim_type_art_read_states()
                .filter(|(name, _)| *name == prefix)
                .count(),
            2
        );
        assert!(serde_json::from_str::<AircraftTrailer>(r#"{"animation":null}"#).is_err());

        if let Some((root, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() {
            let rules = RuleSet::from_processed_rules(
                &RulesLayerStack::new(root)
                    .process_with_fixed_art(&art)
                    .unwrap(),
            )
            .unwrap();
            for name in ["V3ROCKET", "DMISL", "CMISL"] {
                assert_eq!(fields(&rules, name), (None, 2), "{name}");
            }
        }
    }
}
