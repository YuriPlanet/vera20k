//! Full native46BEE0 retained ART state, then the production RuleSet projection.
use super::*;
use crate::rules::projectile_type::ProjectileType;
use crate::rules::ruleset::RuleSet;
use serde_json::Value;

fn native() -> &'static Value {
    crate::test_fixture::json("tools/projectile_oracle/bridge_render_art_state.json")
}

#[test]
fn projectile_color_matches_original_shared_reader_and_section_admission() {
    // Bullet46BFA5 and House511C22 call the same ReadColor474A90, with the
    // same ctor0 and missing-section guard. Reuse its consumed native controls.
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/input_oracle/campaign_start_houses.json",
    ))
    .unwrap();
    let controls = &native["color_controls"];
    let mut prefix = String::from(
        "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n[GUN]\nProjectile=Control\n[Colors]\n",
    );
    for scheme in controls["registry"].as_array().unwrap() {
        if scheme["shade_count"] == 1 {
            prefix.push_str(scheme["name"].as_str().unwrap());
            prefix.push_str("=0,0,0\n");
        }
    }
    let sections = |value: &Value| {
        IniFile::from_sections_for_test(value.as_object().unwrap().iter().map(|(name, values)| {
            let mut section = IniSection::new(name.clone());
            for (key, value) in values.as_object().unwrap() {
                section.set(key, value.as_str().unwrap());
            }
            section
        }))
    };
    for row in controls["country_rows"].as_array().unwrap() {
        let mut setup = IniFile::from_str(&prefix);
        setup.merge(&sections(&row["setup_sections"]));
        let mut layers = RulesLayerStack::new(setup);
        let current = row["physical_ini"]
            .as_str()
            .map(IniFile::from_str)
            .unwrap_or_else(|| sections(&row["sections"]));
        layers.push(RulesLayerKind::Scenario, current);
        let processed = layers.process_with_fixed_art(&IniFile::empty()).unwrap();
        // Constructor-only absent sections need not project a gameplay type;
        // query and project the real registry's retained owner in all cases.
        let (_, state, image) = processed
            .projectile_art_states()
            .find(|(name, _, _)| *name == "Control")
            .unwrap();
        let mut projectile =
            ProjectileType::from_ini_section("Control", &IniSection::new("Control".into()), None);
        state.apply_to(image, &mut projectile);
        assert_eq!(
            i64::from(projectile.color_scheme()),
            row["after"].as_i64().unwrap(),
            "original474A90 {row}"
        );
    }
}

#[test]
fn production_kirov_projectile_retains_default_lightgold_shade53() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    let kirov = rules.object("ZEP").unwrap();
    // Both normal and elite production weapon references use this same type.
    assert_eq!(kirov.primary(), Some("BlimpBomb"));
    assert_eq!(kirov.elite_primary(), Some("BlimpBombE"));
    for name in [kirov.primary().unwrap(), kirov.elite_primary().unwrap()] {
        let projectile = rules.weapon(name).unwrap().projectile.as_deref().unwrap();
        let kind = rules.projectile(projectile).unwrap();
        assert_eq!(kind.id, "BlimpBombP");
        assert_eq!(kind.voxel_image_load(), Some("ZBOMB"));
        assert_eq!(
            kind.color_scheme(),
            1,
            "{name}: ctor0 -> ReadColor missing -> shade53"
        );
        assert_eq!(
            rules.color_schemes[kind.color_scheme() as usize / 2].name,
            "LightGold"
        );
    }
}

#[test]
fn projectile_color_uses_current_registry_and_survives_layer_handoff() {
    let first = IniFile::from_str(
        "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n[GUN]\nProjectile=SHOT\n[Colors]\nFirst=0,0,0\n[SHOT]\nColor=Later\n",
    );
    let first = RulesLayerStack::new(first).process().unwrap();
    let (_, receipt) = first.into_ini_and_native_type_construction_trace();
    let mut registry = receipt.into_registry_state_discarding_events();
    for (body, expected) in [
        ("[Colors]\nLater=1,2,3\n", 0),
        ("[SHOT]\nName=present, Color absent\n", 1),
        ("[SHOT]\nColor=Later\n", 3),
        ("[Other]\nName=whole SHOT section absent\n", 3),
        ("[SHOT]\nColor=unknown\n", 3),
    ] {
        let processed = RulesLayerStack::new(IniFile::from_str(body))
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), registry)
            .unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        assert_eq!(
            rules.projectile("SHOT").unwrap().color_scheme(),
            expected,
            "{body}"
        );
        let (_, receipt) = processed.into_ini_and_native_type_construction_trace();
        // The existing serialized registry handoff must keep this new state.
        registry = serde_json::from_value(
            serde_json::to_value(receipt.into_registry_state_discarding_events()).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn projectile_voxel_load_retains_attempt_identity_independently_of_shp_and_inviso() {
    // Original46C40B..46C417 runs after the independent SHP gate, on current
    // Image even when empty. A false Voxel flag does not call LoadVoxelAssets.
    let art = IniFile::from_str("[V]\nVoxel=yes\n[S]\nVoxel=no\n");
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n[GUN]\nProjectile=SHOT\n",
    ));
    for (body, expected, expected_shp) in [
        ("[SHOT]\nImage=V\nInviso=yes\n", Some("V"), None),
        ("[SHOT]\nName=missing Image, still Voxel\n", Some(""), None),
        ("[SHOT]\nImage=S\n", Some(""), Some("S")),
        ("[SHOT]\nImage=V\n", Some("V"), Some("S")),
        ("[Other]\nName=absent SHOT\n", Some("V"), Some("S")),
    ] {
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(body));
        let processed = layers.process_with_fixed_art(&art).unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        let kind = rules.projectile("SHOT").unwrap();
        assert!(kind.inviso, "{body}");
        assert_eq!(kind.voxel_image_load(), expected, "{body}");
        assert_eq!(
            kind.image_loads.first().map(|load| load.image.as_str()),
            expected_shp,
            "base non-Voxel SHP load precedes Bullet Inviso: {body}"
        );
    }
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
    let image_load = actual.image_loads.first().map(|load| {
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
        ProjectileArtState::new().apply_to(
            &crate::rules::object_image::ObjectImageState::new(identity),
            &mut unread,
        );
        compare(
            &unread,
            &case["constructor"],
            &format!("{identity} constructor"),
        );
        assert!(
            unread.image_loads.is_empty(),
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
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    let corpus = native();
    let anim_roots = crate::rules::effect_asset_catalog::anim_class_roots(&rules);
    let mut compared = Vec::new();
    for row in corpus["retail"]["rows"].as_array().unwrap() {
        let identity = row["identity"].as_str().unwrap();
        // The oracle separately constructs every physical Weapon-reference
        // type, including dormant declarations outside the runtime registry.
        if let Some(projectile) = rules.projectile(identity) {
            compare(projectile, &row["rows"][0]["state"], identity);
            if let Some(trailer) = row["rows"][0]["state"]["trailer"].as_str() {
                assert!(
                    anim_roots
                        .iter()
                        .any(|root| root == &trailer.to_ascii_uppercase()),
                    "native {identity} Trailer={trailer} must reach the common animation binder"
                );
            }
            compared.push(identity);
        }
    }
    assert_eq!(
        compared.len(),
        rules.projectiles_iter().count(),
        "every production-registered type has an original reader comparison"
    );
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
                    &sim.session.map_name()
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
