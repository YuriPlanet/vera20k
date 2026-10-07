//! Original speed readers and Process chronology through the production owner.
use super::*;
use crate::rules::ruleset::RuleSet;
use serde_json::Value;

fn cached_ini(sections: &Value) -> IniFile {
    let mut ini = IniFile::empty();
    for (name, keys) in sections.as_object().unwrap() {
        let mut section = IniSection::new(name.clone());
        for (key, value) in keys.as_object().unwrap() {
            section.set(key, value.as_str().unwrap());
        }
        ini.replace_first_section(section);
    }
    ini
}

fn scalar_corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/weapon_speed.json",
    ))
    .unwrap()
}

#[test]
fn retained_weapon_speed_matches_original_reader_controls() {
    let native = scalar_corpus();
    for row in native["controls"]["cached_reader_controls"]
        .as_array()
        .unwrap()
    {
        let mut processor = RulesPassProcessor::default();
        let index = processor
            .find_or_allocate(RulesTypeFamily::Weapon, "SpeedProbe")
            .unwrap();
        processor.family_mut(RulesTypeFamily::Weapon)[index]
            .weapon
            .speed = row["supplied_retained_default"].as_i64().unwrap() as i32;
        let mut section = IniSection::new("SpeedProbe".into());
        section.set("AmbientDamage", "1");
        if let Some(raw) = row["raw"].as_str() {
            if raw == "speed-key-case" {
                section.set("speed", "40");
            } else {
                section.set("Speed", raw);
            }
        }
        let mut pass = IniFile::empty();
        pass.replace_first_section(section);
        processor.apply_pass(&pass, &IniFile::empty()).unwrap();
        assert_eq!(
            processor.families[&RulesTypeFamily::Weapon][index]
                .weapon
                .speed,
            row["native_dword"].as_i64().unwrap() as i32,
            "{row}"
        );
    }
}

#[test]
fn full_process_speed_uses_prior_gravity_across_absent_sections_and_owner_handoff() {
    use crate::sim::projectile::launch::{LaunchSpeedProjectile, weapon_launch_speed};
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/weapon_speed_order.json",
    ))
    .unwrap();
    let mut first_pass_hashes = Vec::new();
    for sequence in native["sequences"].as_array().unwrap() {
        let mut state = NativeRulesRegistryState::default();
        assert_eq!(
            state.rules_gravity,
            sequence["constructor"]["gravity"].as_i64().unwrap() as i32
        );
        if let Some(gravity) = sequence["cold_audio_visual"]["authored_gravity"].as_i64() {
            let cold = IniFile::from_str(&format!("[AudioVisual]\nGravity={gravity}\n"));
            state = process_native_rules_cold_start(state, &cold, &IniFile::empty(), None)
                .unwrap()
                .into_registry_state_discarding_events();
        }
        for (index, row) in sequence["rows"].as_array().unwrap().iter().enumerate() {
            let mut pass = cached_ini(&row["cached_sections"]);
            if index == 0 {
                // Discover the oracle's preconstructed weapon through an actual
                // production consumer; no expected speed comes from this prefix.
                pass.merge(&IniFile::from_str(
                    "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=OrderProbe\n",
                ));
            }
            let processed = RulesLayerStack::new(pass)
                .process_with_fixed_art_and_registry_state(&IniFile::empty(), state)
                .unwrap();
            let rules = RuleSet::from_processed_rules(&processed).unwrap();
            let weapon = rules.weapon("OrderProbe").unwrap();
            assert_eq!(
                weapon.speed,
                row["result_speed"].as_i64().unwrap() as i32,
                "{row}"
            );
            assert_eq!(
                rules.general.gravity,
                row["result_gravity"].as_i64().unwrap() as i32,
                "{row}"
            );
            let projectile = weapon
                .projectile
                .as_deref()
                .and_then(|id| rules.projectile(id));
            assert_eq!(
                weapon_launch_speed(
                    weapon.speed,
                    projectile.map(|projectile| LaunchSpeedProjectile {
                        rot: projectile.rot,
                        floater: projectile.floater
                    }),
                    rules.general.gravity,
                    500
                ),
                row["get_speed_500"].as_i64().unwrap() as i32,
                "{row}"
            );
            if index == 0 {
                first_pass_hashes.push((rules.source_ini_hash(), rules.simulation_config_hash()));
            }
            let (_, receipt) = processed.into_ini_and_native_type_construction_trace();
            state = receipt.into_registry_state_discarding_events();
        }
    }
    assert_eq!(first_pass_hashes[0].0, first_pass_hashes[1].0);
    assert_ne!(
        first_pass_hashes[0].1, first_pass_hashes[1].1,
        "identical source stacks with distinct retained speeds must reject restore"
    );
}

#[test]
fn type_reset_keeps_rules_gravity_but_discards_weapon_values() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n[GUN]\nSpeed=40\n[AudioVisual]\nGravity=12\n",
    ))
    .process()
    .unwrap();
    let (_, trace) = processed.into_ini_and_native_type_construction_trace();
    let reset = trace
        .into_registry_state_discarding_events()
        .destructive_reset();
    assert_eq!(reset.rules_gravity, 12);
    assert!(reset.families.is_empty());
    let processed = RulesLayerStack::new(IniFile::empty())
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), reset)
        .unwrap();
    assert_eq!(
        RuleSet::from_processed_rules(&processed)
            .unwrap()
            .general
            .gravity,
        12
    );
}

#[test]
fn retail_ifv_speed_passes_production_cold_start_and_scenario_rules_owner() {
    use crate::rules::process_owner::NativeRulesProcessOwner;
    let Some((root, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        root,
        None,
        art,
        std::sync::Arc::default(),
    )
    .unwrap();
    let (rules, _, _, _) = owner
        .load_noncampaign_scenario(None, &IniFile::empty())
        .unwrap()
        .into_parts();
    let native = scalar_corpus();
    let base = &native["physical_ifv"]["layers"][0];
    let weapon = rules.weapon("HoverMissile").unwrap();
    assert_eq!(
        weapon.speed,
        base["postpass_speed"].as_i64().unwrap() as i32
    );
    assert_eq!(
        weapon.range_leptons,
        base["range_leptons"].as_i64().unwrap() as i32
    );
    let projectile = rules.projectile("AAHeatSeeker2").unwrap();
    assert_eq!(projectile.rot, base["type"]["rot"].as_i64().unwrap() as i32);
    assert_eq!(
        projectile.acceleration,
        base["type"]["acceleration"].as_i64().unwrap() as i32
    );
    assert_eq!(
        rules.general.gravity,
        base["gravity"].as_i64().unwrap() as i32
    );
}
