//! Full native46BEE0 retained ART state, then the production RuleSet projection.
use super::*;
use crate::rules::projectile_type::ProjectileType;
use crate::rules::ruleset::RuleSet;
use serde_json::Value;

fn native() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/bridge_render_art_state.json",
    ))
    .unwrap()
}

fn compare(actual: &ProjectileType, expected: &Value, context: &str) {
    assert_eq!(
        actual.image.as_deref().unwrap_or(""),
        expected["image"].as_str().unwrap(),
        "{context} Image"
    );
    for (name, value) in [
        ("voxel", actual.voxel),
        ("theater", actual.theater),
        ("new_theater", actual.new_theater),
        ("inverse_rotates", actual.rotates),
        ("flat", actual.flat),
        ("anim_palette", actual.anim_palette),
        ("inviso", actual.inviso),
    ] {
        assert_eq!(value, expected[name].as_bool().unwrap(), "{context} {name}");
    }
    for (name, value) in [
        ("anim_low", actual.anim_low),
        ("anim_high", actual.anim_high),
        ("anim_rate", actual.anim_rate),
        ("spawn_delay", actual.spawn_delay),
    ] {
        assert_eq!(
            i64::from(value),
            expected[name].as_i64().unwrap(),
            "{context} {name}"
        );
    }
    assert_eq!(
        actual.trailer.as_deref(),
        expected["trailer"].as_str(),
        "{context} Trailer"
    );
    let image_load = actual.image_load.as_ref().map(|load| {
        serde_json::json!({
            "image": load.image, "theater": load.theater, "new_theater": load.new_theater,
        })
    });
    assert_eq!(
        image_load,
        expected["image_load"]
            .as_object()
            .map(|_| expected["image_load"].clone()),
        "{context} admitted image load"
    );
}

#[test]
fn retained_projectile_art_matches_original_full_readers_across_layers_and_owner_handoff() {
    let corpus = native();
    let cases = corpus["controls"].as_array().unwrap();
    assert_eq!(cases.len(), 22);
    for case in cases {
        let identity = case["identity"].as_str().unwrap();
        let art = IniFile::from_str(case["art"].as_str().unwrap());
        let mut registry = NativeRulesRegistryState::default();
        let empty = crate::rules::ini_parser::IniSection::new(identity.to_owned());
        let mut unread = ProjectileType::from_ini_section(identity, &empty, None);
        ProjectileArtState::new(identity).apply_to(&mut unread);
        compare(
            &unread,
            &case["constructor"],
            &format!("{identity} constructor"),
        );
        assert!(
            unread.image_load.is_none(),
            "constructor Image text cannot load an asset"
        );
        let mut stack: Option<RulesLayerStack> = None;
        for (index, row) in case["rows"].as_array().unwrap().iter().enumerate() {
            // The real earlier Animation sweep calls D's original missing ART
            // reader, as in the native witness. D is also a retail registered
            // type; its cache effect is not a Python/manual-reset expectation.
            let prefix = if index == 0 {
                "[General]\nMetallicDebris=D\n[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n[GUN]\nProjectile=SHOT\n"
            } else {
                ""
            };
            let prefix = prefix.replace("Projectile=SHOT", &format!("Projectile={identity}"));
            let pass = IniFile::from_str(&format!("{prefix}{}", row["rules"].as_str().unwrap()));
            if let Some(layers) = &mut stack {
                let layers: &mut RulesLayerStack = layers;
                layers.push(RulesLayerKind::Scenario, pass.clone());
            } else {
                stack = Some(RulesLayerStack::new(pass.clone()));
            }
            let processed = stack
                .as_ref()
                .unwrap()
                .process_with_fixed_art(&art)
                .unwrap();
            let rules = RuleSet::from_processed_rules(&processed).unwrap();
            let context = format!("{} pass{index}", case["name"]);
            compare(rules.projectile(identity).unwrap(), &row["state"], &context);

            let continued = RulesLayerStack::new(pass)
                .process_with_fixed_art_and_registry_state(&art, registry)
                .unwrap();
            let rules = RuleSet::from_processed_rules(&continued).unwrap();
            compare(
                rules.projectile(identity).unwrap(),
                &row["state"],
                &format!("{context} handoff"),
            );
            let (_, receipt) = continued.into_ini_and_native_type_construction_trace();
            registry = receipt.into_registry_state_discarding_events();
        }
    }
}

#[test]
fn production_retail_projectile_art_matches_original_referenced_type_readers() {
    use crate::rules::retail_ini_fixture::retail_ini;
    let Some(rules_ini) = retail_ini("rulesmd.ini") else {
        return;
    };
    let Some(art_ini) = retail_ini("artmd.ini") else {
        return;
    };
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini).unwrap();
    let corpus = native();
    let mut compared = Vec::new();
    for row in corpus["retail"]["rows"].as_array().unwrap() {
        let identity = row["identity"].as_str().unwrap();
        // The oracle separately constructs every physical Weapon-reference
        // type, including dormant declarations outside the runtime registry.
        if let Some(projectile) = rules.projectile(identity) {
            compare(projectile, &row["rows"][0]["state"], identity);
            compared.push(identity);
        }
    }
    assert!(compared.contains(&"Cannon"));
    assert!(
        compared.contains(&"AAHeatSeeker2"),
        "actual rotating family"
    );
    assert!(
        compared.contains(&"JUMP"),
        "actual rotating parasite family"
    );
    eprintln!(
        "native ART compared {} production types: {compared:?}",
        compared.len()
    );
}

#[test]
fn effective_projectile_art_changes_configuration_hash_and_rejects_old_restore() {
    use crate::sim::snapshot::{GameSnapshot, SnapshotError};
    let ini = IniFile::from_str(
        "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n[GUN]\nProjectile=SHOT\n[SHOT]\nImage=IMAGE\n",
    );
    let make = |body: &str| {
        RuleSet::from_ini_with_fixed_art_for_test(
            &ini,
            &IniFile::from_str(&format!("[IMAGE]\n{body}")),
        )
        .unwrap()
    };
    let original = make("");
    let sim = crate::sim::world::Simulation::new();
    let bytes =
        GameSnapshot::save_validated(&sim, 11, original.simulation_config_hash(), "Bullet ART", 0);
    for field in [
        "Voxel=yes",
        "Theater=yes",
        "NewTheater=yes",
        "Rotates=yes",
        "Flat=yes",
        "AnimPalette=yes",
        "AnimLow=1",
        "AnimHigh=2",
        "AnimRate=3",
        "SpawnDelay=4",
        "Trailer=TRAIL",
    ] {
        let changed = make(field);
        assert_eq!(original.source_ini_hash(), changed.source_ini_hash());
        assert_ne!(
            original.simulation_config_hash(),
            changed.simulation_config_hash(),
            "{field}"
        );
        assert!(
            matches!(
                GameSnapshot::load_validated(
                    &bytes,
                    11,
                    changed.simulation_config_hash(),
                    &sim.session.map_name
                ),
                Err(SnapshotError::RulesMismatch { .. })
            ),
            "{field}"
        );
    }
    assert_eq!(
        original.simulation_config_hash(),
        make("Image=UNREAD_REDIRECT\nRotates=no\nAnimLow=0\nSpawnDelay=3").simulation_config_hash()
    );
}
