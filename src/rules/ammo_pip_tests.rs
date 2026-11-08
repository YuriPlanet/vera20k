//! TechnoType's shared signed Ammo/InitialAmmo/PipWrap reader projections.
use super::{ObjectCategory, ObjectType};
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::ruleset::RuleSet;

fn native() -> &'static serde_json::Value {
    crate::test_fixture::json("tools/procedural_drawing_oracle/ammo_pips.json")
}

#[test]
fn retail_ammo_configuration_matches_original_type_readers() {
    let empty = IniSection::new("ORCA".to_owned());
    let constructor = ObjectType::from_ini_section("ORCA", &empty, ObjectCategory::Aircraft);
    let expected = &native()["readers"]["type_constructors"]["ORCA"];
    assert_eq!(
        constructor.pip_wrap(),
        expected["pip_wrap"].as_i64().unwrap() as i32
    );
    assert_eq!(
        constructor.initial_ammo,
        expected["initial_ammo"].as_i64().unwrap() as i32
    );
    assert_eq!(constructor.ammo, expected["ammo"].as_i64().unwrap() as i32);
    assert_eq!(
        constructor.pip_scale as i32,
        expected["pip_scale"].as_i64().unwrap() as i32
    );
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let layers = native()["readers"]["layers"].as_array().unwrap();
    let files: Vec<_> = layers
        .iter()
        .map(|layer| layer["file"].as_str().unwrap())
        .collect();
    assert_eq!(
        files,
        [
            "RULESMD.INI",
            "LANGRULE.INI",
            "MPBattleMD.ini",
            "XMP03T4.MAP"
        ]
    );
    for (name, expected) in native()["readers"]["type_results"].as_object().unwrap() {
        let object = retail.rules.object(name).unwrap();
        assert_eq!(
            object.pip_wrap(),
            expected["pip_wrap"].as_i64().unwrap() as i32,
            "{name}"
        );
        assert_eq!(
            object.initial_ammo,
            expected["initial_ammo"].as_i64().unwrap() as i32,
            "{name}"
        );
        assert_eq!(
            object.ammo,
            expected["ammo"].as_i64().unwrap() as i32,
            "{name}"
        );
        assert_eq!(
            object.pip_scale as i32,
            expected["pip_scale"].as_i64().unwrap() as i32,
            "{name}"
        );
        assert_eq!(
            object.pip_max(&retail.rules),
            expected["pip_max"].as_i64().unwrap() as i32,
            "{name}"
        );
    }
}

#[test]
fn signed_ammo_readers_keep_current_defaults_through_production_passes() {
    // Constructor710D40/7110E6/7110EC and ReadInt714188/71475B/714775.
    // ReadInt owns decimal/hex/empty semantics; these exercise projection
    // and admitted/missing-key passes, rather than adding another parser.
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[AircraftTypes]\n0=ORCA\n[ORCA]\nPipWrap=-1\nInitialAmmo=-2\nAmmo=2147483647\n",
    ));
    for (kind, patch, expected) in [
        (
            RulesLayerKind::LangRule,
            "[OTHER]\nPipWrap=99\n",
            (-1, -2, i32::MAX),
        ),
        (
            RulesLayerKind::GameMode,
            "[ORCA]\npipwrap=12\nStrength=150\n",
            (-1, -2, i32::MAX),
        ),
        (
            RulesLayerKind::Scenario,
            "[ORCA]\nPipWrap=$80000000\nInitialAmmo=-2147483648\nAmmo=-1\n",
            (i32::MIN, i32::MIN, -1),
        ),
        // Physical empty values are omitted by the INI loader, so these
        // admitted fields retain their current values.
        (
            RulesLayerKind::Scenario,
            "[ORCA]\nPipWrap=\nInitialAmmo=\nAmmo=\n",
            (i32::MIN, i32::MIN, -1),
        ),
        (
            RulesLayerKind::Scenario,
            "[ORCA]\nPipWrap=invalid\nInitialAmmo=invalid\nAmmo=invalid\n",
            (0, 0, 0),
        ),
    ] {
        layers.push(kind, IniFile::from_str(patch));
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        let object = rules.object("ORCA").unwrap();
        assert_eq!(
            (object.pip_wrap(), object.initial_ammo, object.ammo),
            expected,
            "{kind:?}: {patch}"
        );
    }
}

/// Ares TechnoTypeClass_GetPipMax_MindControl (1004B910, hook 0x7162B0):
/// a MindControl pip scale counts the first MindControl weapon's Damage,
/// here the secondary's, where stock reads the primary's.
#[cfg(feature = "ares")]
#[test]
fn ares_mind_control_pips_take_the_mind_control_weapon() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=PSI\n[PSI]\nPipScale=MindControl\nPrimary=Gun\nSecondary=Mind\n\
         [Gun]\nDamage=25\nWarhead=AP\n[Mind]\nDamage=3\nWarhead=MC\n\
         [AP]\nVerses=100%\n[MC]\nMindControl=yes\n",
    ))
    .unwrap();
    assert_eq!(rules.object("PSI").unwrap().pip_max(&rules), 3);
}
