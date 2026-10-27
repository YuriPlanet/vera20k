//! Ares TypeExt reader1003F3F3 ->1003C650/Find10007EA0.

use crate::rules::ini_parser::IniFile;
use crate::rules::object_type::ObjectCategory;
use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
use crate::rules::sound_ini::{EvaRegistry, SoundRegistry};
use std::sync::Arc;

#[test]
fn script_conversion_retains_registered_identity_through_layers_save_and_hash() {
    let root = "[InfantryTypes]\n0=SOURCE\n1=TARGET\n2=OTHER\n3=CLEAR\n4=UNSET\n\
                [SOURCE]\nConvert.Script=TARGET\nTurretROT=4\nHasSpotlight=yes\n\
                Spotlight.StartHeight=520\nSpotlight.Distance=700\nSpotlight.AttachedTo=barrel\nSpotlight.DisableColor=yes\n\
                [CLEAR]\nConvert.Script=TARGET\n\
                [TARGET]\nStrength=100\n[OTHER]\nStrength=200\n[UNSET]\nStrength=100\n";
    let map = IniFile::from_str(
        "[SOURCE]\nConvert.Script=MISSING\nTurretROT=bad\nSpotlight.StartHeight=bad\n\
         Spotlight.Distance=-17\nSpotlight.AttachedTo=invalid\n[CLEAR]\nConvert.Script=<none>\n\
         [UNSET]\nConvert.Script=MISSING\n",
    );
    let load = |root: &str| {
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            IniFile::from_str(root),
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
    let source = rules.object("SOURCE").unwrap();
    let light = source.spotlight_rules();
    assert!(source.has_spotlight);
    assert_eq!(
        (light.start_height(), light.distance(), light.glow_flags()),
        (520, -17, 1)
    );
    assert!(light.follows_secondary());
    assert_eq!(
        rules
            .object("TARGET")
            .unwrap()
            .spotlight_rules()
            .start_height(),
        430
    );
    assert_eq!(rules.object("SOURCE").unwrap().secondary_turn_rate(), 4);
    let target = rules.object("SOURCE").unwrap().script_conversion().unwrap();
    assert_eq!(
        (target.category, target.id.as_str()),
        (ObjectCategory::Infantry, "TARGET")
    );
    assert!(rules.object("CLEAR").unwrap().script_conversion().is_none());
    assert!(rules.object("UNSET").unwrap().script_conversion().is_none());
    assert!(
        rules.object("MISSING").is_none(),
        "lookup must not allocate a type"
    );

    let (_, mut other) = load(&root.replace(
        "[SOURCE]\nConvert.Script=TARGET",
        "[SOURCE]\nConvert.Script=OTHER",
    ));
    // Source identity also includes layer history. Normalize that private
    // test input so this assertion isolates retained conversion bindings.
    other.source_ini_hash = rules.source_ini_hash();
    assert_ne!(
        rules.simulation_config_hash(),
        other.simulation_config_hash()
    );
    let (_, mut other_light) =
        load(&root.replace("Spotlight.StartHeight=520", "Spotlight.StartHeight=530"));
    other_light.source_ini_hash = rules.source_ini_hash();
    assert_ne!(
        rules.simulation_config_hash(),
        other_light.simulation_config_hash(),
        "retained light options are simulation inputs"
    );

    let saved = crate::util::save_codec::serialize(owner.saved_scenario_rules().unwrap()).unwrap();
    let restored = crate::util::save_codec::deserialize(&saved).unwrap();
    let restored = owner.project_saved_scenario_rules(&restored).unwrap();
    assert_eq!(
        rules.object("SOURCE").unwrap().script_conversion(),
        restored.object("SOURCE").unwrap().script_conversion()
    );
    assert_eq!(light, restored.object("SOURCE").unwrap().spotlight_rules());
}
