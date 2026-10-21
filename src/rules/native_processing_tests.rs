//! Ordered native processing and constructor-oracle regression tests.

use super::*;
use crate::util::save_codec;

#[test]
fn nominal_type_reader_matches_executed_physical_history() {
    use crate::rules::ruleset::RuleSet;
    let corpus = crate::test_fixture::json("tools/spatial_oracle/nominal_building_survivor.json");
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[InfantryTypes]\n0=CONTROL\n[CONTROL]\nStrength=50\n",
    ));
    assert!(
        !RuleSet::from_processed_rules(&layers.process().unwrap())
            .unwrap()
            .object("CONTROL")
            .unwrap()
            .nominal()
    );
    for row in corpus["reader_history"]["rows"].as_array().unwrap() {
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(row["physical_ini"].as_str().unwrap()),
        );
        let rules = RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap();
        assert_eq!(
            rules.object("CONTROL").unwrap().nominal(),
            row["after"].as_u64().unwrap() != 0,
            "{}",
            row["physical_ini"]
        );
    }
}

///71592A reads exact-case ART ShadowIndex with the current field as default,
///after each retained Rules Image read. Neither retail zero nor final-image
///defaults can replace the previous section's selected value.
#[test]
fn voxel_shadow_index_retains_art_image_history_and_save_state() {
    use crate::rules::ruleset::RuleSet;
    let art = IniFile::from_str(
        "[FIRST]\nShadowIndex=2tail\n[OTHER]\nShadowindex=9\n[NEGATIVE]\nShadowIndex=-1\n",
    );
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[AircraftTypes]\n0=JET\n[JET]\nImage=FIRST\nShadowIndex=99\n",
    ));
    let read = |processed: &ProcessedRulesLayers| {
        RuleSet::from_processed_rules(processed)
            .unwrap()
            .object("JET")
            .unwrap()
            .voxel_shadow_index()
    };
    assert_eq!(read(&layers.process_with_fixed_art(&art).unwrap()), 2);
    layers.push(
        RulesLayerKind::GameMode,
        IniFile::from_str("[JET]\nImage=OTHER\n"),
    );
    assert_eq!(read(&layers.process_with_fixed_art(&art).unwrap()), 2);
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[JET]\nImage=NEGATIVE\n"),
    );
    let processed = layers.process_with_fixed_art(&art).unwrap();
    assert_eq!(read(&processed), -1);
    let (_, receipt) = processed.into_ini_and_native_type_construction_trace();
    let retained = receipt.into_registry_state_discarding_events();
    let retained = save_codec::deserialize(&save_codec::serialize(&retained).unwrap()).unwrap();
    let next = RulesLayerStack::new(IniFile::from_str(
        "[AircraftTypes]\n0=JET\n[JET]\nImage=OTHER\n",
    ))
    .process_with_fixed_art_and_registry_state(&art, retained)
    .unwrap();
    assert_eq!(read(&next), -1);
    let cold = RulesLayerStack::new(IniFile::from_str(
        "[AircraftTypes]\n0=JET\n[JET]\nShadowIndex=99\n",
    ))
    .process_with_fixed_art(&art)
    .unwrap();
    assert_eq!(
        read(&cold),
        0,
        "constructor zero and no Rules ShadowIndex read"
    );
}

#[test]
fn repair_inputs_retain_native_layer_defaults_through_reset_and_save() {
    use crate::rules::ruleset::RuleSet;

    // ReadGeneral670DE9/670E30/670E57 use current Rules scalars as defaults.
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[General]\nIRepairStep=20\nURepairRate=.25\nIRepairRate=.001\n",
    ));
    layers.push(
        RulesLayerKind::GameMode,
        IniFile::from_str("[General]\nIRepairStep=-3tail\nIRepairRate=.5\n"),
    );
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[General]\nIRepairStep=\nURepairRate=\nIRepairRate=\n"),
    );
    let processed = layers.process().unwrap();
    let assert_values = |rules: &RuleSet| {
        assert_eq!(rules.general.infantry_repair_step, -3);
        assert_eq!(rules.general.unit_repair_rate, 0.25);
        assert_eq!(rules.general.infantry_repair_rate, 0.5);
    };
    assert_values(&RuleSet::from_processed_rules(&processed).unwrap());
    let (_, trace) = processed.into_ini_and_native_type_construction_trace();
    let state = trace.into_registry_state_discarding_events();
    let state: NativeRulesRegistryState =
        save_codec::deserialize(&save_codec::serialize(&state).unwrap()).unwrap();
    let next = RulesLayerStack::new(IniFile::from_str("[General]\nName=Next scene\n"))
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), state.destructive_reset())
        .unwrap();
    let retained = RuleSet::from_processed_rules(&next).unwrap();
    assert_values(&retained);
    let cold = RuleSet::from_ini(&IniFile::from_str("[General]\nName=Next scene\n")).unwrap();
    assert_eq!(retained.source_ini_hash(), cold.source_ini_hash());
    assert_ne!(
        retained.simulation_config_hash(),
        cold.simulation_config_hash()
    );
}

/// Rules+580 is a signed retained ReadInt/current-default, not stock300.
#[test]
fn maximum_cheer_rate_preserves_native_defaults_layers_and_rules_lifetime() {
    use crate::rules::ruleset::{GeneralRules, RuleSet};
    let publish =
        |processed: &ProcessedRulesLayers| RuleSet::from_processed_rules(processed).unwrap();
    assert_eq!(GeneralRules::default().maximum_cheer_rate(), 1);
    let mut layers = RulesLayerStack::new(IniFile::from_str("[General]\n"));
    assert_eq!(
        publish(&layers.process().unwrap())
            .general
            .maximum_cheer_rate(),
        1
    );
    for (kind, text, expected) in [
        (
            RulesLayerKind::LangRule,
            "[General]\nMaximumCheerRate=300\n",
            300,
        ),
        (
            RulesLayerKind::GameMode,
            "[General]\nMaximumCheerRate=-17tail\n",
            -17,
        ),
        (
            RulesLayerKind::Scenario,
            "[General]\nMaximumCheerRate=\n",
            -17,
        ),
        (
            RulesLayerKind::Scenario,
            "[General]\nMaximumcheerRate=99\n",
            -17,
        ),
        (
            RulesLayerKind::Scenario,
            "[General]\nMaximumCheerRate=0\n",
            0,
        ),
    ] {
        layers.push(kind, IniFile::from_str(text));
        assert_eq!(
            publish(&layers.process().unwrap())
                .general
                .maximum_cheer_rate(),
            expected,
            "{text}"
        );
    }
    let (_, trace) = layers
        .process()
        .unwrap()
        .into_ini_and_native_type_construction_trace();
    let state = trace.into_registry_state_discarding_events();
    let mut retained: NativeRulesRegistryState =
        save_codec::deserialize(&save_codec::serialize(&state).unwrap()).unwrap();
    for reset in [false, true] {
        if reset {
            retained = retained.destructive_reset();
        }
        let next = RulesLayerStack::new(IniFile::from_str("[General]\nName=Next scene\n"))
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), retained)
            .unwrap();
        assert_eq!(publish(&next).general.maximum_cheer_rate(), 0);
        let (_, trace) = next.into_ini_and_native_type_construction_trace();
        retained = trace.into_registry_state_discarding_events();
    }
    let continued = |value: i32| {
        let first = RulesLayerStack::new(IniFile::from_str(&format!(
            "[General]\nPrismSupportModifier=0\nMaximumCheerRate={value}\n"
        )))
        .process()
        .unwrap();
        let (_, trace) = first.into_ini_and_native_type_construction_trace();
        let next = RulesLayerStack::new(IniFile::from_str(
            "[General]\nPrismSupportModifier=0\nName=Same current sources\n",
        ))
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            trace.into_registry_state_discarding_events(),
        )
        .unwrap();
        publish(&next)
    };
    let a = continued(1);
    let b = continued(2);
    assert_eq!(a.source_ini_hash(), b.source_ini_hash());
    assert_ne!(a.simulation_config_hash(), b.simulation_config_hash());
}

#[test]
fn retail_battle_cheer_rate_uses_the_production_layered_reader() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    assert_eq!(retail.rules.general.maximum_cheer_rate(), 300);
    assert_eq!(retail.rules.general.cheer_sound(), Some("Cheer"));
}

/// Rules ctor667371/66737B/667385 initializes +14F8/+14FC/+1500, then
/// ReadGeneral uses the retained integers as ReadInt5276D0 defaults. These
/// are instruction-derived reader expectations, not native timing goldens.
#[test]
fn survivor_divisors_follow_native_defaults_and_retained_rules_passes() {
    use crate::rules::ruleset::{GeneralRules, RuleSet};

    let values = |rules: &GeneralRules| {
        [
            rules.allied_survivor_divisor,
            rules.soviet_survivor_divisor,
            rules.third_survivor_divisor,
        ]
    };
    let published = |processed: &ProcessedRulesLayers| {
        values(&RuleSet::from_processed_rules(processed).unwrap().general)
    };
    assert_eq!(values(&GeneralRules::default()), [200, 100, 300]);
    let mut layers = RulesLayerStack::new(IniFile::empty());
    assert_eq!(published(&layers.process().unwrap()), [200, 100, 300]);
    let irrelevant = RulesLayerStack::new(IniFile::from_str("[General]\nName=Defaults\n"));
    assert_eq!(published(&irrelevant.process().unwrap()), [200, 100, 300]);
    for (kind, text, expected) in [
        (
            RulesLayerKind::LangRule,
            "[General]\nAlliedSurvivorDivisor=500\nSovietSurvivorDivisor=250\nThirdSurvivorDivisor=750\n",
            [500, 250, 750],
        ),
        (
            RulesLayerKind::GameMode,
            "[General]\nSovietSurvivorDivisor=-17\n",
            [500, -17, 750],
        ),
        (
            RulesLayerKind::Scenario,
            "[General]\nAlliedSurvivorDivisor=\nSovietSurvivorDivisor=$nothex\nThirdSurvivorDivisor=invalid\n",
            [500, -17, 0],
        ),
        (
            RulesLayerKind::Scenario,
            "[General]\nAlliedsurvivorDivisor=99\nSovietSurvivorDivisor=12tail\n",
            [500, 12, 0],
        ),
        (
            RulesLayerKind::Scenario,
            "[General]\nAlliedSurvivorDivisor=0\nSovietSurvivorDivisor=noth\n",
            [0, 12, 0],
        ),
    ] {
        layers.push(kind, IniFile::from_str(text));
        assert_eq!(published(&layers.process().unwrap()), expected, "{text}");
    }

    let processed = layers.process().unwrap();
    let (_, trace) = processed.into_ini_and_native_type_construction_trace();
    let state = trace.into_registry_state_discarding_events();
    let mut retained: NativeRulesRegistryState =
        save_codec::deserialize(&save_codec::serialize(&state).unwrap()).unwrap();
    for reset in [false, true] {
        if reset {
            retained = retained.destructive_reset();
        }
        let next = RulesLayerStack::new(IniFile::from_str("[General]\nName=Next scene\n"))
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), retained)
            .unwrap();
        assert_eq!(published(&next), [0, 12, 0]);
        let (_, trace) = next.into_ini_and_native_type_construction_trace();
        retained = trace.into_registry_state_discarding_events();
    }
}

#[test]
fn survivor_divisor_histories_change_simulation_config_hash_independently() {
    use crate::rules::ruleset::RuleSet;

    let continued = |divisors: [i32; 3]| {
        let initial = RulesLayerStack::new(IniFile::from_str(&format!(
            "[General]\nPrismSupportModifier=0\nAlliedSurvivorDivisor={}\n\
             SovietSurvivorDivisor={}\nThirdSurvivorDivisor={}\n",
            divisors[0], divisors[1], divisors[2]
        )))
        .process()
        .unwrap();
        let (_, trace) = initial.into_ini_and_native_type_construction_trace();
        let next = RulesLayerStack::new(IniFile::from_str(
            "[General]\nPrismSupportModifier=0\nName=Next scene\n",
        ))
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            trace.into_registry_state_discarding_events(),
        )
        .unwrap();
        RuleSet::from_processed_rules(&next).unwrap()
    };
    let baseline = continued([200, 100, 300]);
    let mut omitted = Vec::new();
    for (index, key) in [
        "AlliedSurvivorDivisor",
        "SovietSurvivorDivisor",
        "ThirdSurvivorDivisor",
    ]
    .into_iter()
    .enumerate()
    {
        let mut divisors = [200, 100, 300];
        divisors[index] += 1;
        let changed = continued(divisors);
        assert_eq!(baseline.source_ini_hash(), changed.source_ini_hash());
        assert_eq!(
            baseline.general.prism_support,
            changed.general.prism_support
        );
        assert_eq!(
            [
                changed.general.allied_survivor_divisor,
                changed.general.soviet_survivor_divisor,
                changed.general.third_survivor_divisor,
            ],
            divisors
        );
        if baseline.simulation_config_hash() == changed.simulation_config_hash() {
            omitted.push(key);
        }
    }
    assert!(omitted.is_empty(), "effective hash omits {omitted:?}");
}

/// Instruction-derived reader regression: Building460F87 keeps the current
/// triple on missing/blank keys; only a reached Building body can read it.
#[test]
fn target_coord_offset_retains_only_reached_building_rules_passes() {
    use crate::rules::ruleset::RuleSet;
    let art = IniFile::from_str("[HUT]\nTargetCoordOffset=900,900,900\n");
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[BuildingTypes]\n0=HUT\n[HUT]\nTargetCoordOffset=9,12,15\n\
         [VehicleTypes]\n0=TANK\n[TANK]\nTargetCoordOffset=100,100,100\n\
         [LATE]\nTargetCoordOffset=800,800,800\n",
    ));
    let get = |processed: &ProcessedRulesLayers, name: &str| {
        RuleSet::from_processed_rules(processed)
            .unwrap()
            .object(name)
            .unwrap()
            .target_coord_offset()
    };
    assert_eq!(
        get(&layers.process_with_fixed_art(&art).unwrap(), "HUT"),
        [9, 12, 15]
    );
    assert_eq!(
        get(&layers.process_with_fixed_art(&art).unwrap(), "TANK"),
        [0; 3]
    );
    for (kind, text, expected) in [
        (
            RulesLayerKind::LangRule,
            "[HUT]\ntargetcoordoffset=1,1,1\n",
            [9, 12, 15],
        ),
        (
            RulesLayerKind::GameMode,
            "[HUT]\nTargetCoordOffset=-7,20,4\n",
            [-7, 20, 4],
        ),
        (
            RulesLayerKind::Scenario,
            "[BuildingTypes]\n0=LATE\n[HUT]\nTargetCoordOffset=\n",
            [-7, 20, 4],
        ),
    ] {
        layers.push(kind, IniFile::from_str(text));
        assert_eq!(
            get(&layers.process_with_fixed_art(&art).unwrap(), "HUT"),
            expected
        );
    }
    let processed = layers.process_with_fixed_art(&art).unwrap();
    assert_eq!(
        get(&processed, "LATE"),
        [0; 3],
        "not allocated at the earlier body"
    );
    let (_, trace) = processed.into_ini_and_native_type_construction_trace();
    let state = trace.into_registry_state_discarding_events();
    let restored = save_codec::deserialize(&save_codec::serialize(&state).unwrap()).unwrap();
    let tail = RulesLayerStack::new(IniFile::from_str(
        "[BuildingTypes]\n0=HUT\n[HUT]\nStrength=123\n",
    ));
    let retained = tail
        .process_with_fixed_art_and_registry_state(&art, restored)
        .unwrap();
    assert_eq!(get(&retained, "HUT"), [-7, 20, 4]);

    let reset = tail
        .process_with_fixed_art_and_registry_state(&art, state.destructive_reset())
        .unwrap();
    assert_eq!(get(&reset, "HUT"), [0; 3]);
}

#[test]
fn target_coord_offset_hashes_effective_retained_value() {
    use crate::rules::ruleset::RuleSet;
    let tail = RulesLayerStack::new(IniFile::from_str("[HUT]\nStrength=123\n"));
    let continued = |offset: &str| {
        let prior = RulesLayerStack::new(IniFile::from_str(&format!(
            "[BuildingTypes]\n0=HUT\n[HUT]\nTargetCoordOffset={offset}\n"
        )))
        .process()
        .unwrap();
        let (_, trace) = prior.into_ini_and_native_type_construction_trace();
        let processed = tail
            .process_with_fixed_art_and_registry_state(
                &IniFile::empty(),
                trace.into_registry_state_discarding_events(),
            )
            .unwrap();
        RuleSet::from_processed_rules(&processed).unwrap()
    };
    let first = continued("10,20,30");
    let second = continued("10,20,31");
    assert_eq!(first.source_ini_hash(), second.source_ini_hash());
    assert_ne!(
        first.simulation_config_hash(),
        second.simulation_config_hash()
    );
}

/// Rules constructor +7AC=-1 and ReadGeneral66F2AF..66F2CF's prior-value
/// ReadInt. Reset clears type registries, not this scalar. The original
/// constructor is consumed from the Smoke corpus; the remaining assertions
/// exercise the production reader/registry handoff, not an AI wind index.
#[test]
fn smoke_wind_reader_retains_layer_history_and_registry_reset() {
    use crate::rules::ruleset::RuleSet;
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/damage_smoke.json",
    ))
    .unwrap();
    let mut layers = RulesLayerStack::new(IniFile::empty());
    assert_eq!(
        RuleSet::from_rules_layers(&layers)
            .unwrap()
            .general
            .wind_direction(),
        native["wind_constructor"].as_i64().unwrap() as i32
    );
    for (text, expected) in [
        ("[General]\nWindDirection=1\n", 1),
        ("[General]\nOther=4\n", 1),
        // ReadInt defaults only on absence; present junk takes atoi's0.
        ("[General]\nWindDirection=oops\n", 0),
        ("[General]\nWindDirection=-7\n", -7),
        ("[General]\nWindDirection=17\n", 17),
    ] {
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(text));
        assert_eq!(
            RuleSet::from_rules_layers(&layers)
                .unwrap()
                .general
                .wind_direction(),
            expected
        );
    }
    let processed = layers.process().unwrap();
    let (_, receipt) = processed.into_ini_and_native_type_construction_trace();
    let reset = receipt
        .into_registry_state_discarding_events()
        .destructive_reset();
    let continued = RulesLayerStack::new(IniFile::from_str("[General]\nOther=9\n"))
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), reset)
        .unwrap();
    assert_eq!(continued.wind_direction(), 17);
}

/// Rust regression for RulesClass666163/66616D defaults and
/// ReadGeneral66F30E..66F350 current-value defaults. Exercise the production
/// pass reader, its RuleSet publication and the process-resident registry
/// handoff; these are instruction-derived expectations, not native goldens.
#[test]
fn falling_rate_limits_retain_layer_and_scene_reader_history() {
    use crate::rules::ruleset::RuleSet;
    let values = |processed: &ProcessedRulesLayers| {
        let rules = RuleSet::from_processed_rules(processed).unwrap();
        let published = (
            rules.general.parachute_max_fall_rate,
            rules.general.no_parachute_max_fall_rate,
        );
        assert_eq!(published, processed.fall_rate_controls());
        published
    };
    let mut layers = RulesLayerStack::new(IniFile::empty());
    assert_eq!(values(&layers.process().unwrap()), (-3, -100));
    for (kind, text, expected) in [
        (
            RulesLayerKind::LangRule,
            "[General]\nParachuteMaxFallRate=-7\nNoParachuteMaxFallRate=-123\n",
            (-7, -123),
        ),
        (
            RulesLayerKind::GameMode,
            "[General]\nParachuteMaxFallRate=-2\n",
            (-2, -123),
        ),
        (
            RulesLayerKind::Scenario,
            "[General]\nParachuteMaxFallRate=\nNoParachuteMaxFallRate=\n",
            (-2, -123),
        ),
        (
            RulesLayerKind::Scenario,
            "[General]\nParachutemaxFallRate=99\nNoparachuteMaxFallRate=99\n",
            (-2, -123),
        ),
        (
            RulesLayerKind::Scenario,
            "[General]\nNoParachuteMaxFallRate=invalid\n",
            (-2, 0),
        ),
        (
            RulesLayerKind::Scenario,
            "[General]\nParachuteMaxFallRate=12tail\nNoParachuteMaxFallRate=-2147483648\n",
            (12, i32::MIN),
        ),
    ] {
        layers.push(kind, IniFile::from_str(text));
        assert_eq!(values(&layers.process().unwrap()), expected, "{text}");
    }
    let processed = layers.process().unwrap();
    let (_, receipt) = processed.into_ini_and_native_type_construction_trace();
    let mut registry = receipt.into_registry_state_discarding_events();
    for reset in [false, true] {
        if reset {
            registry = registry.destructive_reset();
        }
        let continued = RulesLayerStack::new(IniFile::from_str("[General]\nName=Next scene\n"))
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), registry)
            .unwrap();
        assert_eq!(values(&continued), (12, i32::MIN));
        let fresh = RulesLayerStack::new(IniFile::from_str("[General]\nName=Next scene\n"))
            .process_with_fixed_art(&IniFile::empty())
            .unwrap();
        let retained_rules = RuleSet::from_processed_rules(&continued).unwrap();
        let fresh_rules = RuleSet::from_processed_rules(&fresh).unwrap();
        assert_eq!(
            retained_rules.source_ini_hash(),
            fresh_rules.source_ini_hash()
        );
        assert_ne!(
            retained_rules.simulation_config_hash(),
            fresh_rules.simulation_config_hash(),
            "identical current INIs must not hide retained fall-rate differences"
        );
        let (_, receipt) = continued.into_ini_and_native_type_construction_trace();
        registry = receipt.into_registry_state_discarding_events();
    }
}

/// Retail RulesMD followed by production mode/map readers, rather than text
/// matching or manual constants in the simulation. The retail source authors
/// the same -3/-100 values as RulesClass's constructor defaults.
#[test]
fn retail_falling_rate_limits_survive_mode_and_map_layers() {
    use crate::rules::ruleset::RuleSet;
    let Some((ini, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let mut layers = RulesLayerStack::new(ini);
    let read = |layers: &RulesLayerStack| {
        let processed = layers.process_with_fixed_art(&art).unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        (
            rules.general.parachute_max_fall_rate,
            rules.general.no_parachute_max_fall_rate,
        )
    };
    assert_eq!(read(&layers), (-3, -100));
    layers.push(RulesLayerKind::LangRule, IniFile::empty());
    layers.push(
        RulesLayerKind::GameMode,
        IniFile::from_str("[General]\nNoParachuteMaxFallRate=-61\n"),
    );
    assert_eq!(read(&layers), (-3, -61));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[General]\nParachuteMaxFallRate=-5\n"),
    );
    assert_eq!(read(&layers), (-5, -61));
}

/// Rules ctor665EDE and retained ReadGeneral67181C..67183C, compared with
/// original5276D0 reads. Physical-empty lines and deliberately stored-empty
/// cache values have different admission; the shared INI owner preserves it.
#[test]
fn native_trigger_reveal_radius_retains_original_reader_history() {
    use crate::rules::ruleset::RuleSet;
    let corpus: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_action_values.json",
    ))
    .unwrap();
    let native = &corpus
        .iter()
        .find(|row| row["case_kind"] == "trigger_reveal")
        .unwrap()["radius_reader"];
    let integer = |value: &serde_json::Value| value.as_i64().unwrap() as i32;
    let mut layers = RulesLayerStack::new(IniFile::empty());
    assert_eq!(
        RuleSet::from_rules_layers(&layers)
            .unwrap()
            .reveal_trigger_radius(),
        integer(&native["constructor"])
    );
    let rows = native["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 22);
    for row in rows {
        let name = row["name"].as_str().unwrap();
        assert_eq!(
            RuleSet::from_rules_layers(&layers)
                .unwrap()
                .reveal_trigger_radius(),
            integer(&row["prior"]),
            "{name}: retained reader default"
        );
        let pass = if let Some(text) = row["physical_text"].as_str() {
            IniFile::from_str(text)
        } else {
            // Supplied cached entries are the same seam as the original
            // comparison, including its explicit stored-empty control.
            let mut ini = IniFile::empty();
            for (section, entries) in row["sections"].as_object().unwrap() {
                for (key, value) in entries.as_object().unwrap() {
                    ini.projection_section_mut(section)
                        .set(key, value.as_str().unwrap());
                }
            }
            ini
        };
        layers.push(RulesLayerKind::Scenario, pass);
        assert_eq!(
            RuleSet::from_rules_layers(&layers)
                .unwrap()
                .reveal_trigger_radius(),
            integer(&row["value"]),
            "{name}: original signed unclamped read"
        );
        let read = &row["reads"][0];
        assert_eq!(read["section"], native["section"]);
        assert_eq!(read["key"], native["key"]);
        assert_eq!(read["default"], row["prior"]);
    }
    assert_eq!(native["rng_before"], native["rng_after"]);
    assert!(native["rng_calls"].as_array().unwrap().is_empty());
}

#[test]
fn campaign_difficulty_process_layers_match_original_nine_scalar_outputs() {
    use crate::rules::ruleset::{DifficultyRules, RuleSet};
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/house_difficulty_readers.json",
    ))
    .unwrap();
    let Some(root_bytes) = crate::rules::retail_ini_fixture::retail_ini_bytes("rulesmd.ini") else {
        return;
    };
    let Some(art_bytes) = crate::rules::retail_ini_fixture::retail_ini_bytes("artmd.ini") else {
        return;
    };
    assert_eq!(
        crate::util::sha256::sha256_hex(&root_bytes),
        native["rulesmd_sha256"]
    );
    let art = IniFile::from_bytes(&art_bytes).unwrap();
    let rows = native["layers"].as_array().unwrap();
    assert_eq!(rows.len(), 8);
    fn recorded_row(row: &serde_json::Value) -> DifficultyRules {
        let bits = |key: &str| {
            f64::from_bits(u64::from_str_radix(row["biases"][key].as_str().unwrap(), 16).unwrap())
        };
        DifficultyRules {
            firepower: bits("firepower"),
            ground_speed: bits("groundspeed"),
            air_speed: bits("airspeed"),
            armor: bits("armor"),
            rof: bits("rof"),
            cost: bits("cost"),
            build_time: bits("build_time"),
            repair_delay: bits("repair_delay"),
            build_delay: bits("build_delay"),
        }
    }
    fn row_bits(row: DifficultyRules) -> [u64; 9] {
        [
            row.firepower,
            row.ground_speed,
            row.air_speed,
            row.armor,
            row.rof,
            row.cost,
            row.build_time,
            row.repair_delay,
            row.build_delay,
        ]
        .map(f64::to_bits)
    }
    let names = ["Easy", "Normal", "Difficult"];
    // The native reader fixture starts with explicit current fields. Seed
    // those observed inputs on the sole Process owner, then retain its state
    // through the same eight layers; do not synthesize decimal INI defaults.
    let mut registry = NativeRulesRegistryState {
        rules_difficulty_rows: names.map(|name| recorded_row(&rows[0]["before"][name])),
        ..NativeRulesRegistryState::default()
    };
    for row in rows {
        assert_eq!(
            registry.rules_difficulty_rows.map(row_bits),
            names.map(|name| row_bits(recorded_row(&row["before"][name]))),
            "{}: retained input",
            row["name"]
        );
        let pass = if row["name"] == "physical_rules" {
            IniFile::from_bytes(&root_bytes).unwrap()
        } else {
            IniFile::from_sections_for_test(row["sections"].as_object().unwrap().iter().map(
                |(name, entries)| {
                    let mut section = IniSection::new(name.clone());
                    for (key, value) in entries.as_object().unwrap() {
                        section.set(key, value.as_str().unwrap());
                    }
                    section
                },
            ))
        };
        let processed = RulesLayerStack::new(pass)
            .process_with_fixed_art_and_registry_state(&art, registry)
            .unwrap();
        let expected = names.map(|name| row_bits(recorded_row(&row["after"][name])));
        assert_eq!(
            processed.difficulty_rows().map(row_bits),
            expected,
            "{}: live Process fields",
            row["name"]
        );
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        assert_eq!(
            rules.general.difficulty_rows.map(row_bits),
            expected,
            "{}: production projection",
            row["name"]
        );
        registry = processed
            .into_ini_and_native_type_construction_trace()
            .1
            .into_registry_state_discarding_events();
    }
}

#[test]
fn building_foundation_pass_matches_original_art_reads() {
    let oracle: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/engineer_bridge_cursor_caller.json",
    ))
    .unwrap();
    let rows = oracle["foundation_reads"].as_array().unwrap();
    assert_eq!(rows.len(), 80);
    for row in rows {
        let input = &row["input"];
        let type_id = input["type_id"].as_str().unwrap();
        let mut fixed_art = IniFile::empty();
        for (name, entries) in input["art_entries"].as_object().unwrap() {
            let mut section = IniSection::new(name.to_owned());
            for (key, value) in entries.as_object().unwrap() {
                section.set(key, value.as_str().unwrap());
            }
            fixed_art.replace_first_section(section);
        }
        // The native corpus supplies a current +EF0 field, including nonzero
        // retained-state controls. Seed that exact fixture on the sole registry
        // owner; the production Process pass performs both actual ART reads.
        let registered = RulesLayerStack::new(IniFile::from_str(&format!(
            "[BuildingTypes]\n0={type_id}\n"
        )))
        .process()
        .unwrap();
        let mut registry = registered
            .into_ini_and_native_type_construction_trace()
            .1
            .into_registry_state_discarding_events();
        registry
            .families
            .get_mut(&RulesTypeFamily::Building)
            .unwrap()[0]
            .building_foundation = input["initial_foundation_id"].as_u64().unwrap() as u8;
        let mut raw = IniSection::new(type_id.to_owned());
        raw.set("Image", input["image"].as_str().unwrap());
        if let Some(poison) = input["rules_foundation_poison"].as_str() {
            raw.set("Foundation", poison);
        }
        if input["undeploy_pointer"].as_bool().unwrap() {
            raw.set("UndeploysInto", "TESTUNIT");
        }
        let mut pass = IniFile::empty();
        pass.replace_first_section(raw);
        let processed = RulesLayerStack::new(pass)
            .process_with_fixed_art_and_registry_state(&fixed_art, registry)
            .unwrap();
        assert_eq!(
            processed.building_foundation_states().collect::<Vec<_>>(),
            vec![(
                type_id,
                row["output"]["foundation_id"].as_u64().unwrap() as u8
            )],
            "{}",
            input["name"]
        );
        let mut rules = crate::rules::ruleset::RuleSet::from_processed_rules(&processed).unwrap();
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&fixed_art));
        let object = rules.object(type_id).unwrap();
        assert_eq!(
            crate::rules::foundation::foundation_id(&object.foundation),
            row["output"]["foundation_id"].as_u64().unwrap() as u8,
            "{}: projected and installed native field",
            input["name"]
        );
        assert_eq!(
            crate::rules::foundation::foundation_dimensions(&object.foundation),
            (
                row["output"]["width"].as_u64().unwrap() as u16,
                row["output"]["height"].as_u64().unwrap() as u16,
            )
        );
        assert_eq!(
            object.is_1x1_with_undeploy(),
            row["output"]["is_1x1_with_undeploy"].as_bool().unwrap(),
            "{}: Undeploy gate consumes the native field",
            input["name"]
        );
    }
}

#[test]
fn building_foundation_reads_only_reached_bodies_and_retains_rules_image() {
    let art = IniFile::from_str(
        "[FIRST]\nFoundation=2x2\nImage=VISUAL\n\
         [VISUAL]\nFoundation=4x4\n[SECOND]\nFoundation=3x3\n",
    );
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[BuildingTypes]\n0=HUT\n[HUT]\nImage=FIRST\nFoundation=4x4\n",
    ));
    assert_eq!(
        layers
            .process_with_fixed_art(&art)
            .unwrap()
            .building_foundation_states()
            .collect::<Vec<_>>(),
        [("HUT", 3)],
        "Rules Foundation and ART Image redirects are not Foundation sources"
    );
    layers.push(
        RulesLayerKind::LangRule,
        IniFile::from_str("[HUT]\nStrength=100\n"),
    );
    assert_eq!(
        layers
            .process_with_fixed_art(&art)
            .unwrap()
            .building_foundation_states()
            .collect::<Vec<_>>(),
        [("HUT", 3)],
        "missing Rules Image uses its previous 25-byte field"
    );
    layers.push(
        RulesLayerKind::GameMode,
        IniFile::from_str("[HUT]\nImage=SECOND\n"),
    );
    assert_eq!(
        layers
            .process_with_fixed_art(&art)
            .unwrap()
            .building_foundation_states()
            .collect::<Vec<_>>(),
        [("HUT", 6)]
    );
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[HUT]\nImage=MISSING\n"),
    );
    let processed = layers.process_with_fixed_art(&art).unwrap();
    assert_eq!(
        processed.building_foundation_states().collect::<Vec<_>>(),
        [("HUT", 6)]
    );

    let late_art = IniFile::from_str("[LATE]\nFoundation=4x4\n");
    let mut late = RulesLayerStack::new(IniFile::from_str("[LATE]\nImage=LATE\n"));
    late.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[BuildingTypes]\n0=LATE\n"),
    );
    assert_eq!(
        late.process_with_fixed_art(&late_art)
            .unwrap()
            .building_foundation_states()
            .collect::<Vec<_>>(),
        [("LATE", 0)],
        "a late constructor does not reread an earlier orphan Rules body or ART"
    );
}

#[test]
fn building_foundation_art_install_preserves_reached_retained_and_constructor_state() {
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ruleset::RuleSet;

    let art = IniFile::from_str("[FIRST]\nFoundation=2x2\n[LATE]\nFoundation=4x4\n");
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[BuildingTypes]\n0=HUT\n[HUT]\nImage=FIRST\nUndeploysInto=MCV\n\
         [LATE]\nImage=LATE\n",
    ));
    let reached = layers.process_with_fixed_art(&art).unwrap();
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[BuildingTypes]\n0=HUT\n1=LATE\n[HUT]\nImage=MISSING\n"),
    );
    let retained_and_late = layers.process_with_fixed_art(&art).unwrap();
    for (processed, installation_art, expected) in [
        (&reached, &art, vec![("HUT", "2x2")]),
        (
            &retained_and_late,
            &art,
            vec![("HUT", "2x2"), ("LATE", "1x1")],
        ),
        // Binding metadata is not another ReadINI pass, even if its ART cache
        // differs. Only the process-resident native owner writes Foundation.
        (
            &reached,
            &IniFile::from_str("[FIRST]\nFoundation=4x4\n"),
            vec![("HUT", "2x2")],
        ),
    ] {
        let mut rules = RuleSet::from_processed_rules(processed).unwrap();
        rules.install_art_data(ArtRegistry::from_ini(installation_art));
        for (name, foundation) in expected {
            let object = rules.object(name).unwrap();
            assert_eq!(
                object.foundation, foundation,
                "{name}: installation reread Foundation"
            );
            if name == "HUT" {
                assert!(!object.is_1x1_with_undeploy());
            }
        }
    }
}

#[test]
fn building_foundation_survives_registry_handoff_and_resets_with_owner() {
    let art = IniFile::from_str("[FIRST]\nFoundation=2x2\n");
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[BuildingTypes]\n0=HUT\n[HUT]\nImage=FIRST\n",
    ))
    .process_with_fixed_art(&art)
    .unwrap();
    let (_, receipt) = processed.into_ini_and_native_type_construction_trace();
    let continued = RulesLayerStack::new(IniFile::from_str("[HUT]\nImage=MISSING\n"))
        .process_with_fixed_art_and_registry_state(
            &art,
            receipt.into_registry_state_discarding_events(),
        )
        .unwrap();
    assert_eq!(
        continued.building_foundation_states().collect::<Vec<_>>(),
        [("HUT", 3)]
    );
    let (_, receipt) = continued.into_ini_and_native_type_construction_trace();
    let reset = RulesLayerStack::new(IniFile::from_str(
        "[BuildingTypes]\n0=HUT\n[HUT]\nImage=MISSING\n",
    ))
    .process_with_fixed_art_and_registry_state(
        &art,
        receipt
            .into_registry_state_discarding_events()
            .destructive_reset(),
    )
    .unwrap();
    assert_eq!(
        reset.building_foundation_states().collect::<Vec<_>>(),
        [("HUT", 0)]
    );
}

/// Each row is the next original ReadGeneral call on the same RulesClass.
/// Exercise the production pass owner and its RuleSet publication, not a second
/// list parser. The native corpus also checks ctor emptiness and factory order.
#[test]
fn bridge_animation_lists_match_original_retained_vectors() {
    use crate::rules::ruleset::RuleSet;
    let oracle: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/bridge_anim_lists.json",
    ))
    .unwrap();
    let mut layers = RulesLayerStack::new(IniFile::empty());
    let initial = RuleSet::from_rules_layers(&layers).unwrap();
    assert!(initial.general.metallic_debris.is_empty());
    assert!(initial.bridge_rules.explosions.is_empty());
    for (index, row) in oracle["rows"].as_array().unwrap().iter().enumerate() {
        let key = row["key"].as_str().unwrap();
        let mut pass = IniFile::empty();
        if let Some(raw) = row["raw"].as_str() {
            // Set allows cached empty/whitespace inputs that the physical file
            // loader drops. ReadString must retain the vector for either path.
            let mut general = IniSection::new("General".to_owned());
            general.set(key, raw);
            pass.replace_first_section(general);
        }
        layers.push(RulesLayerKind::Scenario, pass);
        let processed = layers.process().unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        let actual = if key == "MetallicDebris" {
            assert_eq!(rules.general.metallic_debris, processed.metallic_debris());
            &rules.general.metallic_debris
        } else {
            assert_eq!(rules.bridge_rules.explosions, processed.bridge_explosions());
            &rules.bridge_rules.explosions
        };
        let expected = row["result"]["names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(actual, &expected, "native sequential row {index}: {row}");
    }
}

/// MetallicDebris and BridgeExplosions are RulesClass vectors, which ReadGeneral
/// replaces only on a nonempty read: a Process call that continues the live
/// state and omits them keeps them, and their AnimTypes stay registered.
#[test]
fn bridge_animation_vectors_survive_a_continued_process_with_their_types() {
    use crate::rules::ruleset::RuleSet;
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[General]\nMetallicDebris=ONE,ONE\nBridgeExplosions=TWO\n",
    ))
    .process()
    .unwrap();
    let debris = processed.metallic_debris().to_vec();
    let explosions = processed.bridge_explosions().to_vec();
    assert!(!debris.is_empty() && !explosions.is_empty());
    let (_, receipt) = processed.into_ini_and_native_type_construction_trace();
    let continued = RulesLayerStack::new(IniFile::empty())
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            receipt.into_registry_state_discarding_events(),
        )
        .unwrap();
    let rules = RuleSet::from_processed_rules(&continued).unwrap();
    assert_eq!(rules.general.metallic_debris, debris);
    assert_eq!(rules.bridge_rules.explosions, explosions);
    assert!(rules.anim_type_names.iter().any(|name| name == "ONE"));
    assert!(rules.anim_type_names.iter().any(|name| name == "TWO"));
}

#[test]
fn retail_bridge_animation_vectors_match_original_reader_and_unread_d() {
    use crate::rules::{art_data::ArtRegistry, ruleset::RuleSet};
    let Some((ini, fixed_art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let oracle: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/bridge_anim_lists.json",
    ))
    .unwrap();
    let processed = RulesLayerStack::new(ini)
        .process_with_fixed_art(&fixed_art)
        .unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    for (key, actual) in [
        ("MetallicDebris", &rules.general.metallic_debris),
        ("BridgeExplosions", &rules.bridge_rules.explosions),
    ] {
        let row = oracle["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["file"] == "RULESMD.INI" && row["key"] == key)
            .unwrap();
        let expected = row["result"]["names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(actual, &expected);
    }
    assert_eq!(rules.general.metallic_debris.len(), 15);
    assert_eq!(
        rules.general.metallic_debris.last().map(String::as_str),
        Some("D")
    );
    assert!(
        rules
            .anim_type_art_read_states()
            .iter()
            .any(|(name, read)| name == "D" && !read)
    );
    assert!(fixed_art.section("D").is_none());
    rules.install_art_data(ArtRegistry::from_ini(&fixed_art));
    let d = rules
        .art()
        .anim_runtime_config("D")
        .expect("registered unread D");
    assert!(!d.art_body_read);
    assert!(!d.bouncer);
    assert_eq!(d.end, 0);
    assert_eq!(d.loop_end, 0);
    assert_eq!(d.rate_logic_frames, 1);
    assert_eq!(d.raw_shp_frame_count, None);
}

#[test]
fn projectile_flat_matches_original_art_reader_and_retained_defaults() {
    use crate::rules::ruleset::RuleSet;
    let cases: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/flat_art.json",
    ))
    .unwrap();
    let cases = cases.as_array().unwrap();
    assert_eq!(cases.len(), 38);
    for case in cases {
        let prior = case["prior"].as_bool().unwrap();
        let mut art = format!(
            "[PRIOR]\nFlat={}\n[{}]\nImage=UNREAD_REDIRECT\n",
            if prior { "yes" } else { "no" },
            case["art_section"].as_str().unwrap()
        );
        if let Some(flat) = case["flat"].as_str() {
            art.push_str(&format!("Flat={flat}\n"));
        }
        // A redirected ART body must not become the source of Flat.
        art.push_str(&format!(
            "[UNREAD_REDIRECT]\nFlat={}\n",
            if prior { "no" } else { "yes" }
        ));
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n\
             [GUN]\nProjectile=SHOT\n[SHOT]\nImage=PRIOR\n",
        ));
        let mut pass = format!("[SHOT]\nROT=1\nFlat={}\n", if prior { "no" } else { "yes" });
        if let Some(image) = case["image"].as_str() {
            pass.push_str(&format!("Image={image}\n"));
        }
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(&pass));
        let processed = layers
            .process_with_fixed_art(&IniFile::from_str(&art))
            .unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        assert_eq!(
            rules.projectile("SHOT").unwrap().flat,
            case["effective_flat"].as_bool().unwrap(),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn projectile_flat_survives_registry_handoff_and_resets_with_its_owner() {
    use crate::rules::ruleset::RuleSet;
    let art = IniFile::from_str("[YES]\nFlat=yes\n");
    let root = IniFile::from_str(
        "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n\
         [GUN]\nProjectile=SHOT\n[SHOT]\nImage=YES\n",
    );
    let processed = RulesLayerStack::new(root.clone())
        .process_with_fixed_art(&art)
        .unwrap();
    let (_, receipt) = processed.into_ini_and_native_type_construction_trace();
    let continued = RulesLayerStack::new(IniFile::from_str("[SHOT]\nROT=1\n"))
        .process_with_fixed_art_and_registry_state(
            &art,
            receipt.into_registry_state_discarding_events(),
        )
        .unwrap();
    assert!(
        RuleSet::from_processed_rules(&continued)
            .unwrap()
            .projectile("SHOT")
            .unwrap()
            .flat
    );
    assert!(
        continued
            .native_type_construction_trace()
            .events()
            .is_empty()
    );
    let (_, receipt) = continued.into_ini_and_native_type_construction_trace();
    let mut no_image_root = root;
    no_image_root.replace_first_section(
        IniFile::from_str("[SHOT]\nROT=1\n")
            .section("SHOT")
            .unwrap()
            .clone(),
    );
    let reset = RulesLayerStack::new(no_image_root)
        .process_with_fixed_art_and_registry_state(
            &art,
            receipt
                .into_registry_state_discarding_events()
                .destructive_reset(),
        )
        .unwrap();
    assert!(
        !RuleSet::from_processed_rules(&reset)
            .unwrap()
            .projectile("SHOT")
            .unwrap()
            .flat
    );
}

#[test]
fn anim_art_read_receipt_distinguishes_live_sweep_from_late_allocation() {
    // ReadTypeData 0x00679A5D..0x00679A82 reloads the live Anim count, then
    // proceeds to Techno readers; AudioVisual runs still later in Process.
    let root = IniFile::from_str(
        "[Animations]\n0=ROOT\n1=MISSING\n2=EMPTY\n\
         [General]\nDamageFireTypes=EARLY\n\
         [VehicleTypes]\n0=UNIT\n[UNIT]\nExplosion=LATE\n\
         [AudioVisual]\nSmoke=AFTER\n",
    );
    let art = IniFile::from_str(
        "[ROOT]\nNext=CHILD\n[CHILD]\nTrailerAnim=TAIL\n\
         [TAIL]\nRate=1\n[EARLY]\nRate=1\n[LATE]\nRate=1\n\
         [AFTER]\nRate=1\n[EMPTY]\n",
    );
    let mut layers = RulesLayerStack::new(root);
    let first = layers.process_with_fixed_art(&art).unwrap();
    assert_eq!(
        first.anim_type_art_read_states().collect::<Vec<_>>(),
        vec![
            ("ROOT", true),
            ("MISSING", false),
            ("EMPTY", false),
            ("EARLY", true),
            ("CHILD", true),
            ("TAIL", true),
            ("LATE", false),
            ("AFTER", false),
        ],
    );
    assert!(
        first
            .ini()
            .section("Animations")
            .unwrap()
            .registry_ids()
            .contains(&"LATE")
    );
    assert!(
        art.section("LATE").is_some(),
        "ART presence is not a read receipt"
    );
    assert!(
        art.section("EMPTY").is_none(),
        "native lexical loading drops empty sections"
    );

    layers.push(RulesLayerKind::Scenario, IniFile::empty());
    let next = layers.process_with_fixed_art(&art).unwrap();
    assert_eq!(
        next.anim_type_art_read_states().collect::<Vec<_>>(),
        vec![
            ("ROOT", true),
            ("MISSING", false),
            ("EMPTY", false),
            ("EARLY", true),
            ("CHILD", true),
            ("TAIL", true),
            ("LATE", true),
            ("AFTER", true),
        ],
    );
    assert_eq!(
        first.native_type_construction_trace().events(),
        next.native_type_construction_trace().events(),
        "body reads do not spend new constructor IDs",
    );
}

#[test]
fn anim_art_read_receipt_survives_handoff_and_follows_registry_reset() {
    let root = IniFile::from_str("[Animations]\n0=READ\n1=MISSING\n");
    let art = IniFile::from_str("[READ]\nRate=1\n");
    let processed = RulesLayerStack::new(root)
        .process_with_fixed_art(&art)
        .unwrap();
    let (_, receipt) = processed.into_ini_and_native_type_construction_trace();
    let state = receipt.into_registry_state_discarding_events();
    assert_eq!(
        state.anim_type_art_read_states().collect::<Vec<_>>(),
        vec![("READ", true), ("MISSING", false)],
    );
    let continued = RulesLayerStack::new(IniFile::empty())
        .process_with_fixed_art_and_registry_state(&art, state)
        .unwrap();
    assert_eq!(
        continued.anim_type_art_read_states().collect::<Vec<_>>(),
        vec![("READ", true), ("MISSING", false)],
    );
    assert!(
        continued
            .native_type_construction_trace()
            .events()
            .is_empty()
    );
    let (_, receipt) = continued.into_ini_and_native_type_construction_trace();
    let reset = receipt
        .into_registry_state_discarding_events()
        .destructive_reset();
    assert_eq!(reset.anim_type_art_read_states().count(), 0);
    let after_reset = RulesLayerStack::new(IniFile::empty())
        .process_with_fixed_art_and_registry_state(&art, reset)
        .unwrap();
    assert_eq!(after_reset.anim_type_art_read_states().count(), 0);
}

#[test]
fn anim_art_read_receipt_preserves_stored_identity_and_duplicate_slots() {
    let root = IniFile::from_str(
        "[General]\nDamageFireTypes=FIRST, SPACED,ABCDEFGHIJKLMNOPQRSTUVWXY,ABCDEFGHIJKLMNOPQRSTUVWXY\n",
    );
    let art = IniFile::from_str(
        "[FIRST]\nRate=1\n[ SPACED]\nRate=1\n[ABCDEFGHIJKLMNOPQRSTUVWX]\nRate=1\n",
    );
    let processed = RulesLayerStack::new(root)
        .process_with_fixed_art(&art)
        .unwrap();
    assert_eq!(
        processed.anim_type_art_read_states().collect::<Vec<_>>(),
        vec![
            ("FIRST", true),
            (" SPACED", true),
            ("ABCDEFGHIJKLMNOPQRSTUVWX", true),
            ("ABCDEFGHIJKLMNOPQRSTUVWX", true),
        ],
        "the receipt is ordered per type, not a trimmed or deduplicated root set",
    );
}

fn process_ini_passes(root: IniFile, later: IniFile) -> ProcessedRulesLayers {
    let mut layers = RulesLayerStack::new(root);
    layers.push(RulesLayerKind::Scenario, later);
    layers.process().expect("ordered synthetic Rules passes")
}

/// Native type-registry passes find-or-allocate every listed value from each
/// later rules layer, preserving entry order independently of the key text.
#[test]
fn map_overrides_union_type_registries_by_value() {
    let rules = IniFile::from_str("[VehicleTypes]\n0=MTNK\n[Animations]\n0=RING1\n");
    let map = IniFile::from_str("[VehicleTypes]\n0=EVILTANK\n[Animations]\n0=EVILANIM\n");
    let rules = process_ini_passes(rules, map).into_projection_discarding_native_receipt();
    assert_eq!(
        rules.section("VehicleTypes").unwrap().registry_ids(),
        vec!["MTNK", "EVILTANK"]
    );
    assert_eq!(
        rules.section("Animations").unwrap().registry_ids(),
        vec!["RING1", "EVILANIM"]
    );
}

fn process_rules_passes(root: &str, later: &str) -> ProcessedRulesLayers {
    let mut layers = RulesLayerStack::new(IniFile::from_str(root));
    layers.push(RulesLayerKind::Scenario, IniFile::from_str(later));
    layers.process().expect("synthetic Rules passes process")
}

#[test]
fn native_type_construction_trace_preserves_first_new_process_and_lazy_order() {
    let processed = process_rules_passes(
        "[Countries]\n0=Americans\n\
         [Sides]\nGDI=Americans,French\n\
         [SuperWeaponTypes]\n0=SW1\n1=SW2\n\
         [VehicleTypes]\n0=TANK\n\
         [Particles]\n0=SmokeParticle\n\
         [TANK]\nPrimary=Gun\n\
         [Gun]\nProjectile=Shell\n",
        "[Countries]\n0=americans\n1=British\n\
         [Sides]\nGDI=British\nNod=Russians\n\
         [SuperWeaponTypes]\n0=sw1\n2=SW3\n\
         [VehicleTypes]\n0=tank\n1=NEW\n\
         [Particles]\n1=SparkParticle\n\
         [NEW]\nPrimary=gun\n",
    );

    let actual = processed
        .native_type_construction_trace()
        .events()
        .iter()
        .map(|event| (event.family(), event.native_stored_id()))
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        vec![
            (NativeTypeConstructorFamily::HouseType, "Americans"),
            (NativeTypeConstructorFamily::Side, "GDI"),
            (NativeTypeConstructorFamily::SuperWeaponType, "SW1"),
            (NativeTypeConstructorFamily::SuperWeaponType, "SW2"),
            (NativeTypeConstructorFamily::UnitType, "TANK"),
            (NativeTypeConstructorFamily::WeaponType, "Gun"),
            (NativeTypeConstructorFamily::BulletType, "Shell"),
            (NativeTypeConstructorFamily::HouseType, "British"),
            (NativeTypeConstructorFamily::Side, "Nod"),
            (NativeTypeConstructorFamily::SuperWeaponType, "SW3"),
            (NativeTypeConstructorFamily::UnitType, "NEW"),
        ]
    );
    assert_eq!(
        processed
            .native_type_construction_trace()
            .allocated_super_weapon_type_count(),
        3
    );
    assert_eq!(
        processed
            .ini()
            .section("Sides")
            .unwrap()
            .get_for_test("GDI"),
        Some("British"),
        "Side constructor identity is the key, while the compatibility value remains the membership list"
    );
    assert!(
        actual
            .iter()
            .all(|(_, name)| !name.eq_ignore_ascii_case("SmokeParticle")
                && !name.eq_ignore_ascii_case("SparkParticle")),
        "ParticleType constructors do not call AssignUniqueID"
    );
}

#[test]
fn native_type_registry_compares_full_input_but_stores_and_emits_24_bytes() {
    let prefix = "abcdefghijklmnopqrstuvwx";
    let long = format!("{prefix}y");
    assert_eq!(prefix.len(), 0x18);
    assert_eq!(long.len(), 0x19);
    let rules = format!(
        "[VehicleTypes]\n0={prefix}\n1={}\n2={long}\n3={long}\n",
        prefix.to_ascii_uppercase()
    );
    let processed = RulesLayerStack::new(IniFile::from_str(&rules))
        .process()
        .expect("long native IDs process");

    let unit_events = processed
        .native_type_construction_trace()
        .events()
        .iter()
        .filter(|event| event.family() == NativeTypeConstructorFamily::UnitType)
        .map(|event| event.native_stored_id())
        .collect::<Vec<_>>();
    assert_eq!(unit_events, vec![prefix, prefix, prefix]);
    assert_eq!(
        processed
            .ini()
            .section("VehicleTypes")
            .expect("rebuilt Unit registry")
            .registry_ids(),
        vec![prefix, prefix, prefix]
    );
}

#[test]
fn side_registry_and_house_side_lookup_do_not_apply_generic_none_sentinels() {
    let explicit = RulesLayerStack::new(IniFile::from_str(
        "[Sides]\nnone=Americans\n<NONE>=British\nNONE=French\n",
    ))
    .process()
    .expect("Side registry processes");
    let explicit_events = explicit
        .native_type_construction_trace()
        .events()
        .iter()
        .map(|event| (event.family(), event.native_stored_id()))
        .collect::<Vec<_>>();
    assert_eq!(
        explicit_events,
        vec![
            (NativeTypeConstructorFamily::Side, "none"),
            (NativeTypeConstructorFamily::Side, "<NONE>"),
        ]
    );
    assert!(
        explicit_events
            .iter()
            .all(|(family, _)| *family != NativeTypeConstructorFamily::HouseType)
    );

    let lazy = RulesLayerStack::new(IniFile::from_str(
        "[Countries]\n0=House\n[House]\nSide=<none>\n",
    ))
    .process()
    .expect("House Side lookup processes");
    assert_eq!(
        lazy.native_type_construction_trace()
            .events()
            .iter()
            .map(|event| (event.family(), event.native_stored_id()))
            .collect::<Vec<_>>(),
        vec![
            (NativeTypeConstructorFamily::HouseType, "House"),
            (NativeTypeConstructorFamily::Side, "<none>"),
        ]
    );
}

#[test]
fn constructor_lists_collapse_empty_fields_without_trimming_individual_tokens() {
    let list = IniFile::from_str(
        "[General]\nDamageFireTypes=FIRST, SECOND ,,FIRST,,,none,<NoNe>, none , THIRD\n",
    );
    assert_eq!(
        list.section_or_empty("General")
            .read_list("DamageFireTypes", 0x80),
        Some(vec![
            "FIRST", " SECOND ", "FIRST", "none", "<NoNe>", " none ", " THIRD",
        ])
    );
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[BuildingTypes]\n0=FIRST\n\
         [General]\nDamageFireTypes=FIRST, SECOND ,,FIRST,,,none,<NoNe>, none , THIRD\n\
         PrerequisitePower=FIRST,FIRST,,none\n",
    ))
    .process()
    .expect("native list fixture processes");
    let anim_ids = processed
        .native_type_construction_trace()
        .events()
        .iter()
        .filter(|event| event.family() == NativeTypeConstructorFamily::AnimType)
        .map(|event| event.native_stored_id())
        .collect::<Vec<_>>();

    assert_eq!(anim_ids, vec!["FIRST", " SECOND ", " none ", " THIRD"]);
    assert_eq!(
        processed.prerequisite_lists()[PrerequisiteGroup::Power.index()],
        [Prerequisite::Building(0), Prerequisite::Building(0)],
        "native strtok vectors retain repeated entries",
    );
}

fn native_type_family_oracle_code(family: NativeTypeConstructorFamily) -> u8 {
    match family {
        NativeTypeConstructorFamily::HouseType => 0,
        NativeTypeConstructorFamily::Side => 1,
        NativeTypeConstructorFamily::OverlayType => 2,
        NativeTypeConstructorFamily::SuperWeaponType => 3,
        NativeTypeConstructorFamily::WarheadType => 4,
        NativeTypeConstructorFamily::SmudgeType => 5,
        NativeTypeConstructorFamily::TerrainType => 6,
        NativeTypeConstructorFamily::BuildingType => 7,
        NativeTypeConstructorFamily::UnitType => 8,
        NativeTypeConstructorFamily::AircraftType => 9,
        NativeTypeConstructorFamily::InfantryType => 10,
        NativeTypeConstructorFamily::AnimType => 11,
        NativeTypeConstructorFamily::VoxelAnimType => 12,
        NativeTypeConstructorFamily::ParticleSystemType => 13,
        NativeTypeConstructorFamily::WeaponType => 14,
        NativeTypeConstructorFamily::BulletType => 15,
    }
}

fn extend_native_type_event_oracle_hash(
    mut hash: u64,
    events: &[NativeTypeConstructionEvent],
) -> u64 {
    for event in events {
        for byte in std::iter::once(native_type_family_oracle_code(event.family()))
            .chain(event.native_stored_id().bytes())
            .chain(std::iter::once(0xff))
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

fn native_type_event_oracle_hash(events: &[NativeTypeConstructionEvent]) -> u64 {
    extend_native_type_event_oracle_hash(0xcbf29ce484222325_u64, events)
}

#[test]
fn retail_rulesmd_artmd_constructor_trace_matches_verified_base_oracle() {
    let Some((rules, fixed_art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let processed = RulesLayerStack::new(rules)
        .process_with_fixed_art(&fixed_art)
        .expect("stock base Rules pass processes");
    let events = processed.native_type_construction_trace().events();

    assert_eq!(events.len(), 1_975);
    assert_eq!(native_type_event_oracle_hash(events), 0x24516fbd1a096a12);

    let explicit_boundary = events[1_699..1_704]
        .iter()
        .map(|event| (event.family(), event.native_stored_id()))
        .collect::<Vec<_>>();
    assert_eq!(
        explicit_boundary,
        vec![
            (NativeTypeConstructorFamily::AnimType, "D"),
            (NativeTypeConstructorFamily::AnimType, "WCLBOLT2"),
            (NativeTypeConstructorFamily::UnitType, "VISC_LRG"),
            (NativeTypeConstructorFamily::UnitType, "VISC_SML"),
            (NativeTypeConstructorFamily::WeaponType, "Vulcan2"),
        ],
        "stock AI adds nothing; General first emits the cap-truncated MetallicDebris token D, then its other four fresh constructors",
    );

    let assert_ordered = |expected: &[(NativeTypeConstructorFamily, &str)]| {
        let mut cursor = 0;
        for &(family, identity) in expected {
            let relative = events[cursor..]
                .iter()
                .position(|event| event.family() == family && event.native_stored_id() == identity)
                .unwrap_or_else(|| {
                    panic!("missing ordered retail constructor {family:?} {identity}")
                });
            cursor += relative + 1;
        }
    };
    assert_ordered(&[
        (NativeTypeConstructorFamily::AnimType, "SMOKEY2"),
        (NativeTypeConstructorFamily::AnimType, "gtpowexp"),
        (NativeTypeConstructorFamily::AnimType, "tstlexp"),
        (NativeTypeConstructorFamily::AnimType, "CAWA15DM"),
        (NativeTypeConstructorFamily::AnimType, "CACH06DM"),
        (NativeTypeConstructorFamily::AnimType, "YURICNTL"),
        (NativeTypeConstructorFamily::AnimType, "APMUZZLE"),
        (NativeTypeConstructorFamily::AnimType, "BBBLELRG"),
        (NativeTypeConstructorFamily::AnimType, "MINDANIMR"),
        (NativeTypeConstructorFamily::AnimType, "xxxx"),
        (NativeTypeConstructorFamily::BulletType, "NukeUp"),
        (NativeTypeConstructorFamily::BulletType, "NukeDown"),
    ]);
    assert_ordered(&[
        (NativeTypeConstructorFamily::WarheadType, "PrismWarhead"),
        (NativeTypeConstructorFamily::WarheadType, "DummyWarhead"),
        (NativeTypeConstructorFamily::WarheadType, "ApocAPE"),
        (NativeTypeConstructorFamily::WarheadType, "RHINAPE"),
        (NativeTypeConstructorFamily::WarheadType, "GRIZAPE"),
        (NativeTypeConstructorFamily::WarheadType, "Special"),
        (NativeTypeConstructorFamily::WarheadType, "KTSTLEXP"),
        (NativeTypeConstructorFamily::WarheadType, "UltraAPE"),
        (NativeTypeConstructorFamily::WarheadType, "Shock"),
        (NativeTypeConstructorFamily::WarheadType, "MagneShakeWH"),
        (NativeTypeConstructorFamily::WarheadType, "HollowPoint4"),
    ]);

    let event_index = |family, identity| {
        events
            .iter()
            .position(|event| event.family() == family && event.native_stored_id() == identity)
            .unwrap_or_else(|| panic!("missing retail constructor {family:?} {identity}"))
    };
    let building_tail = event_index(NativeTypeConstructorFamily::AnimType, "CACH06DM");
    let bullet_art_head = event_index(NativeTypeConstructorFamily::AnimType, "BBBLELRG");
    for weapon_anim in ["APMUZZLE", "YURICNTL"] {
        let index = event_index(NativeTypeConstructorFamily::AnimType, weapon_anim);
        assert!(
            building_tail < index && index < bullet_art_head,
            "Weapon-body Anim {weapon_anim} must be constructed after Building bodies and before Bullet Art",
        );
    }
    let post_reader_head = event_index(NativeTypeConstructorFamily::AnimType, "MINDANIMR");
    for warhead_anim in ["APOCEXP", "MININUKE - added 11/30"] {
        let index = event_index(NativeTypeConstructorFamily::AnimType, warhead_anim);
        assert!(
            bullet_art_head < index && index < post_reader_head,
            "Warhead-body Anim {warhead_anim} must be constructed after Bullet bodies and before post-Type readers",
        );
    }
    assert!(events.iter().all(|event| {
        event.family() != NativeTypeConstructorFamily::AnimType
            || event.native_stored_id() != "DURASMOKE"
    }));
}

#[test]
fn retail_cold_start_and_noncampaign_prepass_match_verified_native_oracles() {
    let Some((rules, fixed_art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let (startup, startup_boundaries) = process_native_rules_cold_start_inner(
        NativeRulesRegistryState::default(),
        &rules,
        &fixed_art,
        None,
    )
    .expect("stock cold startup processes");

    assert_eq!(startup_boundaries, [1, 608, 612, 1_014, 1_070]);
    assert_eq!(startup.event_count(), 1_070);
    assert_eq!(
        native_type_event_oracle_hash(startup.events()),
        0x408b802af3a4cfce
    );
    assert_eq!(
        startup.events()[..3]
            .iter()
            .map(|event| (event.family(), event.native_stored_id()))
            .collect::<Vec<_>>(),
        vec![
            (NativeTypeConstructorFamily::AnimType, "xxxx"),
            (NativeTypeConstructorFamily::AnimType, "TWLT100"),
            (NativeTypeConstructorFamily::AnimType, "ELECTRO"),
        ]
    );
    assert_eq!(
        startup.events()[608..612]
            .iter()
            .map(|event| (event.family(), event.native_stored_id()))
            .collect::<Vec<_>>(),
        vec![
            (NativeTypeConstructorFamily::AnimType, "SMOKEY2"),
            (NativeTypeConstructorFamily::WarheadType, "HE"),
            (NativeTypeConstructorFamily::OverlayType, "TIB2_01"),
            (NativeTypeConstructorFamily::WarheadType, "TankOGas"),
        ]
    );
    for (family, expected) in [
        (NativeTypeConstructorFamily::AnimType, 613),
        (NativeTypeConstructorFamily::BuildingType, 402),
        (NativeTypeConstructorFamily::WeaponType, 31),
        (NativeTypeConstructorFamily::OverlayType, 9),
        (NativeTypeConstructorFamily::UnitType, 7),
        (NativeTypeConstructorFamily::ParticleSystemType, 5),
        (NativeTypeConstructorFamily::WarheadType, 2),
        (NativeTypeConstructorFamily::InfantryType, 1),
    ] {
        assert_eq!(startup.registry_state().family_len(family), expected);
    }
    for family in [
        NativeTypeConstructorFamily::HouseType,
        NativeTypeConstructorFamily::Side,
        NativeTypeConstructorFamily::SuperWeaponType,
        NativeTypeConstructorFamily::SmudgeType,
        NativeTypeConstructorFamily::TerrainType,
        NativeTypeConstructorFamily::AircraftType,
        NativeTypeConstructorFamily::VoxelAnimType,
        NativeTypeConstructorFamily::BulletType,
    ] {
        assert_eq!(startup.registry_state().family_len(family), 0);
    }
    assert_eq!(startup.allocated_super_weapon_type_count(), 0);
    assert_eq!(
        startup
            .registry_state()
            .families
            .get(&RulesTypeFamily::Particle)
            .map_or(0, Vec::len),
        1,
        "VirusCloud1 is a retained Particle but spends no native Type ID",
    );

    let startup_hash = native_type_event_oracle_hash(startup.events());
    let startup_state = startup.into_registry_state_discarding_events();
    let (prepass, prepass_boundaries) =
        process_native_noncampaign_rules_prepass_inner(startup_state, &rules);
    assert_eq!(prepass_boundaries, [14, 46, 51]);
    assert_eq!(prepass.event_count(), 51);
    assert_eq!(
        native_type_event_oracle_hash(prepass.events()),
        0x45b8b69cd005937d
    );
    assert_eq!(
        extend_native_type_event_oracle_hash(startup_hash, prepass.events()),
        0x026859d66424f324
    );
    assert_eq!(prepass.allocated_super_weapon_type_count(), 0);
    for (family, expected) in [
        (NativeTypeConstructorFamily::HouseType, 14),
        (NativeTypeConstructorFamily::Side, 5),
        (NativeTypeConstructorFamily::OverlayType, 9),
        (NativeTypeConstructorFamily::SuperWeaponType, 0),
        (NativeTypeConstructorFamily::WarheadType, 4),
        (NativeTypeConstructorFamily::SmudgeType, 0),
        (NativeTypeConstructorFamily::TerrainType, 5),
        (NativeTypeConstructorFamily::BuildingType, 402),
        (NativeTypeConstructorFamily::UnitType, 12),
        (NativeTypeConstructorFamily::AircraftType, 5),
        (NativeTypeConstructorFamily::InfantryType, 11),
        (NativeTypeConstructorFamily::AnimType, 615),
        (NativeTypeConstructorFamily::VoxelAnimType, 3),
        (NativeTypeConstructorFamily::ParticleSystemType, 5),
        (NativeTypeConstructorFamily::WeaponType, 31),
        (NativeTypeConstructorFamily::BulletType, 0),
    ] {
        assert_eq!(prepass.registry_state().family_len(family), expected);
    }
}

#[test]
fn native_startup_prepass_repeat_and_reset_keep_each_phase_on_one_registry_owner() {
    let root = IniFile::from_str(
        "[AudioVisual]\nSmoke=SEED\n\
         [Animations]\n0=A\n\
         [BuildingTypes]\n0=B\n\
         [Countries]\n0=HOUSE\n\
         [General]\nDamageFireTypes=SEED,PREANIM\n\
         [A]\nNext=RULES_ANIM_BODY_IGNORED\n\
         [B]\nPrimary=W\nExplosion=BA\nFreeUnit=U\nSecretBuilding=B2\n\
         [B2]\nPrimary=W2\n\
         [W]\nWarhead=TOO_LATE_WEAPON_BODY\n\
         [U]\nPrimary=TOO_LATE_UNIT_BODY\n\
         [BA]\nNext=TOO_LATE_ANIM_BODY\n\
         [HOUSE]\nVeteranInfantry=INF\nVeteranUnits=HUNIT\nVeteranAircraft=HAIR\nSide=HSIDE\n",
    );
    let fixed_art = IniFile::from_str(
        "[A]\nNext=A2\n\
         [A2]\nWarhead=ANIMWH\n\
         [B]\nToOverlay=BOV\n\
         [B2]\nToOverlay=B2OV\n\
         [BA]\nNext=TOO_LATE_ANIM_BODY\n",
    );
    let (startup, boundaries) = process_native_rules_cold_start_inner(
        NativeRulesRegistryState::default(),
        &root,
        &fixed_art,
        None,
    )
    .expect("synthetic startup processes");
    assert_eq!(boundaries, [1, 2, 4, 5, 12]);
    assert_eq!(
        startup.events()[..4]
            .iter()
            .map(|event| (event.family(), event.native_stored_id()))
            .collect::<Vec<_>>(),
        vec![
            (NativeTypeConstructorFamily::AnimType, "SEED"),
            (NativeTypeConstructorFamily::AnimType, "A"),
            (NativeTypeConstructorFamily::AnimType, "A2"),
            (NativeTypeConstructorFamily::WarheadType, "ANIMWH"),
        ],
        "AudioVisual reads Smoke twice but allocates once; the live fixed-Art Anim loop reaches A2",
    );
    let startup_ids = startup
        .events()
        .iter()
        .map(NativeTypeConstructionEvent::native_stored_id)
        .collect::<Vec<_>>();
    for expected in ["W", "BA", "U", "B2", "BOV", "W2", "B2OV"] {
        assert!(
            startup_ids.contains(&expected),
            "missing startup event {expected}"
        );
    }
    for forbidden in [
        "RULES_ANIM_BODY_IGNORED",
        "TOO_LATE_WEAPON_BODY",
        "TOO_LATE_UNIT_BODY",
        "TOO_LATE_ANIM_BODY",
    ] {
        assert!(!startup_ids.contains(&forbidden));
    }

    let startup_state = startup.into_registry_state_discarding_events();
    let (repeat, repeat_boundaries) =
        process_native_rules_cold_start_inner(startup_state, &root, &fixed_art, None)
            .expect("direct startup repeat processes retained state");
    assert_eq!(repeat_boundaries, [0, 0, 1, 1, 1]);
    assert_eq!(
        repeat
            .events()
            .iter()
            .map(|event| (event.family(), event.native_stored_id()))
            .collect::<Vec<_>>(),
        vec![(NativeTypeConstructorFamily::AnimType, "TOO_LATE_ANIM_BODY")],
        "the repeat's live Anim sweep must reach BA, which the prior Building sweep allocated after the first Anim sweep",
    );

    let retained_state = repeat.into_registry_state_discarding_events();
    let (prepass, prepass_boundaries) =
        process_native_noncampaign_rules_prepass_inner(retained_state, &root);
    assert_eq!(
        prepass_boundaries,
        if crate::rules::ARES_ENABLED {
            [1, 2, 3]
        } else {
            [1, 2, 6]
        }
    );
    assert_eq!(
        prepass
            .events()
            .iter()
            .map(|event| (event.family(), event.native_stored_id()))
            .collect::<Vec<_>>(),
        if crate::rules::ARES_ENABLED {
            vec![
                (NativeTypeConstructorFamily::HouseType, "HOUSE"),
                (NativeTypeConstructorFamily::AnimType, "PREANIM"),
                (NativeTypeConstructorFamily::Side, "HSIDE"),
            ]
        } else {
            vec![
                (NativeTypeConstructorFamily::HouseType, "HOUSE"),
                (NativeTypeConstructorFamily::AnimType, "PREANIM"),
                (NativeTypeConstructorFamily::InfantryType, "INF"),
                (NativeTypeConstructorFamily::UnitType, "HUNIT"),
                (NativeTypeConstructorFamily::AircraftType, "HAIR"),
                (NativeTypeConstructorFamily::Side, "HSIDE"),
            ]
        },
        "prepass must retain lookup state and read each House body in Infantry/Unit/Aircraft/Side order",
    );

    let reset_state = prepass
        .into_registry_state_discarding_events()
        .destructive_reset();
    let processed = RulesLayerStack::new(root)
        .process_with_fixed_art_and_registry_state(&fixed_art, reset_state)
        .expect("post-reset full Process succeeds");
    let post_reset_events = processed.native_type_construction_trace().events();
    assert!(post_reset_events.iter().any(|event| {
        event.family() == NativeTypeConstructorFamily::HouseType
            && event.native_stored_id() == "HOUSE"
    }));
    assert!(post_reset_events.iter().any(|event| {
        event.family() == NativeTypeConstructorFamily::AnimType && event.native_stored_id() == "A"
    }));
}

#[test]
fn fixed_art_drives_constructors_without_entering_rules_content_or_bodies() {
    let rules = IniFile::from_str("[Animations]\n0=ROOT\n[ROOT]\nRate=7\n");
    let without_art = RulesLayerStack::new(rules.clone())
        .process_with_fixed_art(&IniFile::empty())
        .expect("empty fixed Art processes");
    let with_art = RulesLayerStack::new(rules)
        .process_with_fixed_art(&IniFile::from_str("[ROOT]\nNext=TAIL\n"))
        .expect("populated fixed Art processes");

    assert_eq!(without_art.content_hash(), with_art.content_hash());
    assert_eq!(
        with_art
            .ini()
            .section("ROOT")
            .expect("Rules body")
            .get_for_test("Next"),
        None,
        "standalone Art keys must not merge into the Rules body projection",
    );
    assert_eq!(
        without_art
            .native_type_construction_trace()
            .events()
            .iter()
            .map(|event| event.native_stored_id())
            .collect::<Vec<_>>(),
        vec!["ROOT"],
    );
    assert_eq!(
        with_art
            .native_type_construction_trace()
            .events()
            .iter()
            .map(|event| event.native_stored_id())
            .collect::<Vec<_>>(),
        vec!["ROOT", "TAIL"],
    );
}

/// Retail `[KTSTLEXP]` is an Animation and the elite Kirov bomb's warhead.
/// The AnimType reads only ART, so its empty Rules body must not replace the
/// warhead's keys in the projection.
#[test]
fn an_animation_keeps_the_rules_body_of_a_warhead_of_the_same_name() {
    use crate::rules::ruleset::RuleSet;
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[Warheads]\n0=BOOM\n[Animations]\n0=BOOM\n\
         [BOOM]\nCellSpread=2\nWall=yes\nProneDamage=70%\n",
    ))
    .process()
    .unwrap();
    let body = processed.ini().section("BOOM").expect("Rules body");
    assert_eq!(body.get_for_test("CellSpread"), Some("2"));
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    let warhead = rules.warhead("BOOM").unwrap();
    assert_eq!(warhead.cell_spread_f64, 2.0);
    assert!(warhead.wall);
    assert_eq!(warhead.prone_damage_f64.to_bits(), 0x3fe6_6666_6666_6666);
}

#[test]
fn fixed_art_and_member_major_type_data_follow_live_native_order() {
    let rules = IniFile::from_str(
        "[BuildingTypes]\n0=BLD\n\
         [VehicleTypes]\n0=UNIT\n\
         [AircraftTypes]\n0=PLANE\n\
         [Animations]\n0=ROOT\n\
         [BLD]\nFreeUnit=FREE\nSecretInfantry=SECINF\nSecretUnit=SECUNIT\nSecretBuilding=SECBLD\nImage=BLDART\n\
         [PLANE]\nImage=PLANEART\n\
         [UNIT]\nPrimary=W1\nSecondary=W2\n\
         [W1]\nAnim=A1,A2\nAssaultAnim=A3\nOccupantAnim=A4\nOpenToppedAnim=A5\nAttachedParticleSystem=PS1\nWarhead=WH1\nProjectile=P1\n\
         [W2]\nAnim=B1,B2\nAssaultAnim=B3\nOccupantAnim=B4\nOpenToppedAnim=B5\nAttachedParticleSystem=PS2\nWarhead=WH2\nProjectile=P2\n\
         [P1]\nImage=P1ART\nAirburstWeapon=LATEW1\nShrapnelWeapon=LATEW2\n\
         [P2]\nImage=P2ART\n",
    );
    let fixed_art = IniFile::from_str(
        "[ROOT]\nNext=ROOT2\n\
         [ROOT2]\nTrailerAnim=ROOT3\n\
         [BLDART]\nToOverlay=BLDOVL\n\
         [PLANEART]\nTrailer=PLANETR\n\
         [PLANETR]\nNext=TOO_LATE\n\
         [P1ART]\nTrailer=P1TR\n\
         [P2ART]\nTrailer=P2TR\n",
    );
    let processed = RulesLayerStack::new(rules)
        .process_with_fixed_art(&fixed_art)
        .expect("fixed-Art TypeData fixture processes");
    let actual = processed
        .native_type_construction_trace()
        .events()
        .iter()
        .map(|event| (event.family(), event.native_stored_id()))
        .collect::<Vec<_>>();

    assert_eq!(
        actual,
        vec![
            (NativeTypeConstructorFamily::BuildingType, "BLD"),
            (NativeTypeConstructorFamily::UnitType, "UNIT"),
            (NativeTypeConstructorFamily::AircraftType, "PLANE"),
            (NativeTypeConstructorFamily::AnimType, "ROOT"),
            (NativeTypeConstructorFamily::AnimType, "ROOT2"),
            (NativeTypeConstructorFamily::AnimType, "ROOT3"),
            (NativeTypeConstructorFamily::UnitType, "FREE"),
            (NativeTypeConstructorFamily::InfantryType, "SECINF"),
            (NativeTypeConstructorFamily::UnitType, "SECUNIT"),
            (NativeTypeConstructorFamily::BuildingType, "SECBLD"),
            (NativeTypeConstructorFamily::OverlayType, "BLDOVL"),
            (NativeTypeConstructorFamily::AnimType, "PLANETR"),
            (NativeTypeConstructorFamily::WeaponType, "W1"),
            (NativeTypeConstructorFamily::WeaponType, "W2"),
            (NativeTypeConstructorFamily::AnimType, "A1"),
            (NativeTypeConstructorFamily::AnimType, "A2"),
            (NativeTypeConstructorFamily::AnimType, "A3"),
            (NativeTypeConstructorFamily::AnimType, "A4"),
            (NativeTypeConstructorFamily::AnimType, "A5"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS1"),
            (NativeTypeConstructorFamily::WarheadType, "WH1"),
            (NativeTypeConstructorFamily::BulletType, "P1"),
            (NativeTypeConstructorFamily::AnimType, "B1"),
            (NativeTypeConstructorFamily::AnimType, "B2"),
            (NativeTypeConstructorFamily::AnimType, "B3"),
            (NativeTypeConstructorFamily::AnimType, "B4"),
            (NativeTypeConstructorFamily::AnimType, "B5"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS2"),
            (NativeTypeConstructorFamily::WarheadType, "WH2"),
            (NativeTypeConstructorFamily::BulletType, "P2"),
            (NativeTypeConstructorFamily::AnimType, "P1TR"),
            (NativeTypeConstructorFamily::WeaponType, "LATEW1"),
            (NativeTypeConstructorFamily::WeaponType, "LATEW2"),
            (NativeTypeConstructorFamily::AnimType, "P2TR"),
        ]
    );
    assert!(actual.iter().all(|(_, id)| *id != "TOO_LATE"));
}

#[test]
fn techno_weapon_bank_uses_effective_native_gates_and_elite_slot_order() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=TURRETS\n1=ORDINARY\n2=CLEARED\n\
         [TURRETS]\nTurretCount=2\nWeaponCount=2\nPrimary=WRONG\nWeapon1=W1\nEliteWeapon1=E1\nWeapon2=W2\nEliteWeapon2=E2\n\
         [ORDINARY]\nPrimary=P\nSecondary=S\nElitePrimary=EP\nEliteSecondary=ES\n\
         [CLEARED]\nTurretCount=0\nClearAllWeapons=yes\nPrimary=ALSO_WRONG\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str(
            "[TURRETS]\nWeaponCount=1\nWeapon1=MAPW1\nEliteWeapon1=MAPE1\n\
             [ORDINARY]\nTurretCount=1\nWeaponCount=0\nPrimary=MAP_WRONG\n",
        ),
    );
    let processed = layers.process().expect("Techno gate fixture processes");
    let weapon_ids = processed
        .native_type_construction_trace()
        .events()
        .iter()
        .filter(|event| event.family() == NativeTypeConstructorFamily::WeaponType)
        .map(|event| event.native_stored_id())
        .collect::<Vec<_>>();

    assert_eq!(
        weapon_ids,
        vec![
            "W1", "E1", "W2", "E2", "P", "S", "EP", "ES", "MAPW1", "MAPE1"
        ]
    );
    assert!(
        weapon_ids
            .iter()
            .all(|id| !matches!(*id, "WRONG" | "ALSO_WRONG" | "MAP_WRONG"))
    );
}

#[test]
fn post_type_readers_preserve_crate_through_tiberium_constructor_order() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[CrateRules]\nWoodCrateImg=O1\nCrateImg=O2\nWaterCrateImg=O3\nUnitCrateType=U1\n\
         [CombatDamage]\nScorches=S0\nScorches1=S1\nScorches2=S2\nScorches3=S3\nScorches4=S4\nSplashList=A0\n\
         FlameDamage=W0\nFlameDamage2=W1\nC4Warhead=W2\nCrushWarhead=W3\nV3Warhead=W4\nDMislWarhead=W5\nV3EliteWarhead=W6\nDMislEliteWarhead=W7\nCMislWarhead=W8\nCMislEliteWarhead=W9\nIvanWarhead=W10\n\
         DeathWeapon=DW\nDrainAnimationType=A1\nControlledAnimationType=A2\nPermaControlledAnimationType=A3\nIonCannonWarhead=W11\n\
         DefaultLargeGreySmokeSystem=PS0\nDefaultSmallGreySmokeSystem=PS1\nDefaultSparkSystem=PS2\nDefaultLargeRedSmokeSystem=PS3\nDefaultSmallRedSmokeSystem=PS4\nDefaultDebrisSmokeSystem=PS5\nDefaultFireStreamSystem=PS6\nDefaultTestParticleSystem=PS7\nDefaultRepairParticleSystem=PS8\n\
         [Radiation]\nRadSiteWarhead=RW\n\
         [AudioVisual]\nDropPodPuff=AV0\nVeinAttack=AV1\nDig=AV2\nAtmosphereEntry=AV3\nTreeFire=AV4,AV5\nOnFire=AV6,AV7\nSmoke=ABCDEFGHIJKLMNOPQRSTUVWXY\nSmallFire=AV8\nLargeFire=AV9\n\
         [SpecialWeapons]\nNukeWarhead=NW\nNukeProjectile=NP\nNukeDown=ND\nMutateWarhead=MW\nMutateExplosionWarhead=MEW\nEMPulseWarhead=EW\nEMPulseProjectile=EP\n\
         [NW]\nParticle=SPS\nAnimList=SAN\nDebrisTypes=SVX\n\
         [Tiberiums]\n0=TIB\n[TIB]\nDebris=TDA,TDB\n",
    ))
    .process()
    .expect("post-reader fixture processes");
    let actual = processed
        .native_type_construction_trace()
        .events()
        .iter()
        .map(|event| (event.family(), event.native_stored_id()))
        .collect::<Vec<_>>();

    assert_eq!(
        actual,
        vec![
            (NativeTypeConstructorFamily::OverlayType, "O1"),
            (NativeTypeConstructorFamily::OverlayType, "O2"),
            (NativeTypeConstructorFamily::OverlayType, "O3"),
            (NativeTypeConstructorFamily::UnitType, "U1"),
            (NativeTypeConstructorFamily::SmudgeType, "S0"),
            (NativeTypeConstructorFamily::SmudgeType, "S1"),
            (NativeTypeConstructorFamily::SmudgeType, "S2"),
            (NativeTypeConstructorFamily::SmudgeType, "S3"),
            (NativeTypeConstructorFamily::SmudgeType, "S4"),
            (NativeTypeConstructorFamily::AnimType, "A0"),
            (NativeTypeConstructorFamily::WarheadType, "W0"),
            (NativeTypeConstructorFamily::WarheadType, "W1"),
            (NativeTypeConstructorFamily::WarheadType, "W2"),
            (NativeTypeConstructorFamily::WarheadType, "W3"),
            (NativeTypeConstructorFamily::WarheadType, "W4"),
            (NativeTypeConstructorFamily::WarheadType, "W5"),
            (NativeTypeConstructorFamily::WarheadType, "W6"),
            (NativeTypeConstructorFamily::WarheadType, "W7"),
            (NativeTypeConstructorFamily::WarheadType, "W8"),
            (NativeTypeConstructorFamily::WarheadType, "W9"),
            (NativeTypeConstructorFamily::WarheadType, "W10"),
            (NativeTypeConstructorFamily::WeaponType, "DW"),
            (NativeTypeConstructorFamily::AnimType, "A1"),
            (NativeTypeConstructorFamily::AnimType, "A2"),
            (NativeTypeConstructorFamily::AnimType, "A3"),
            (NativeTypeConstructorFamily::WarheadType, "W11"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS0"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS1"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS2"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS3"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS4"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS5"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS6"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS7"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS8"),
            (NativeTypeConstructorFamily::WarheadType, "RW"),
            (NativeTypeConstructorFamily::AnimType, "AV0"),
            (NativeTypeConstructorFamily::AnimType, "AV1"),
            (NativeTypeConstructorFamily::AnimType, "AV2"),
            (NativeTypeConstructorFamily::AnimType, "AV3"),
            (NativeTypeConstructorFamily::AnimType, "AV4"),
            (NativeTypeConstructorFamily::AnimType, "AV5"),
            (NativeTypeConstructorFamily::AnimType, "AV6"),
            (NativeTypeConstructorFamily::AnimType, "AV7"),
            (
                NativeTypeConstructorFamily::AnimType,
                "ABCDEFGHIJKLMNOPQRSTUVWX",
            ),
            (
                NativeTypeConstructorFamily::AnimType,
                "ABCDEFGHIJKLMNOPQRSTUVWX",
            ),
            (NativeTypeConstructorFamily::AnimType, "AV8"),
            (NativeTypeConstructorFamily::AnimType, "AV9"),
            (NativeTypeConstructorFamily::WarheadType, "NW"),
            (NativeTypeConstructorFamily::BulletType, "NP"),
            (NativeTypeConstructorFamily::BulletType, "ND"),
            (NativeTypeConstructorFamily::WarheadType, "MW"),
            (NativeTypeConstructorFamily::WarheadType, "MEW"),
            (NativeTypeConstructorFamily::WarheadType, "EW"),
            (NativeTypeConstructorFamily::BulletType, "EP"),
            (NativeTypeConstructorFamily::ParticleSystemType, "SPS"),
            (NativeTypeConstructorFamily::AnimType, "SAN"),
            (NativeTypeConstructorFamily::VoxelAnimType, "SVX"),
            (NativeTypeConstructorFamily::AnimType, "TDA"),
            (NativeTypeConstructorFamily::AnimType, "TDB"),
        ]
    );
}

#[test]
fn ai_and_general_constructor_sites_cover_the_verified_20_and_89_order() {
    use std::fmt::Write as _;

    const AI_KEYS: &[&str] = &[
        "BuildConst",
        "BuildPower",
        "BuildRefinery",
        "BuildBarracks",
        "BuildTech",
        "BuildWeapons",
        "AlliedBaseDefenses",
        "SovietBaseDefenses",
        "ThirdBaseDefenses",
        "BuildDefense",
        "BuildPDefense",
        "BuildAA",
        "BuildHelipad",
        "BuildRadar",
        "ConcreteWalls",
        "NSGates",
        "EWGates",
        "BuildNavalYard",
        "BuildDummy",
        "NeutralTechBuildings",
    ];
    const GENERAL_SITES: &[(&str, NativeTypeConstructorFamily)] = &[
        ("DamageFireTypes", NativeTypeConstructorFamily::AnimType),
        ("OreTwinkle", NativeTypeConstructorFamily::AnimType),
        ("BarrelExplode", NativeTypeConstructorFamily::AnimType),
        ("BarrelDebris", NativeTypeConstructorFamily::VoxelAnimType),
        (
            "BarrelParticle",
            NativeTypeConstructorFamily::ParticleSystemType,
        ),
        ("NukeTakeOff", NativeTypeConstructorFamily::AnimType),
        ("Wake", NativeTypeConstructorFamily::AnimType),
        ("DropPod", NativeTypeConstructorFamily::AnimType),
        ("DeadBodies", NativeTypeConstructorFamily::AnimType),
        ("MetallicDebris", NativeTypeConstructorFamily::AnimType),
        ("BridgeExplosions", NativeTypeConstructorFamily::AnimType),
        ("IonBlast", NativeTypeConstructorFamily::AnimType),
        ("IonBeam", NativeTypeConstructorFamily::AnimType),
        ("WeatherConClouds", NativeTypeConstructorFamily::AnimType),
        ("WeatherConBolts", NativeTypeConstructorFamily::AnimType),
        (
            "WeatherConBoltExplosion",
            NativeTypeConstructorFamily::AnimType,
        ),
        ("DominatorWarhead", NativeTypeConstructorFamily::WarheadType),
        ("DominatorFirstAnim", NativeTypeConstructorFamily::AnimType),
        ("DominatorSecondAnim", NativeTypeConstructorFamily::AnimType),
        ("ChronoPlacement", NativeTypeConstructorFamily::AnimType),
        ("ChronoBeam", NativeTypeConstructorFamily::AnimType),
        ("ChronoBlast", NativeTypeConstructorFamily::AnimType),
        ("ChronoBlastDest", NativeTypeConstructorFamily::AnimType),
        ("WarpIn", NativeTypeConstructorFamily::AnimType),
        ("WarpOut", NativeTypeConstructorFamily::AnimType),
        ("WarpAway", NativeTypeConstructorFamily::AnimType),
        (
            "IronCurtainInvokeAnim",
            NativeTypeConstructorFamily::AnimType,
        ),
        (
            "ForceShieldInvokeAnim",
            NativeTypeConstructorFamily::AnimType,
        ),
        ("WeaponNullifyAnim", NativeTypeConstructorFamily::AnimType),
        ("ChronoSparkle1", NativeTypeConstructorFamily::AnimType),
        ("InfantryExplode", NativeTypeConstructorFamily::AnimType),
        ("FlamingInfantry", NativeTypeConstructorFamily::AnimType),
        ("InfantryHeadPop", NativeTypeConstructorFamily::AnimType),
        ("InfantryNuked", NativeTypeConstructorFamily::AnimType),
        ("InfantryVirus", NativeTypeConstructorFamily::AnimType),
        ("InfantryBrute", NativeTypeConstructorFamily::AnimType),
        ("InfantryMutate", NativeTypeConstructorFamily::AnimType),
        ("Behind", NativeTypeConstructorFamily::AnimType),
        ("MoveFlash", NativeTypeConstructorFamily::AnimType),
        ("Parachute", NativeTypeConstructorFamily::AnimType),
        ("BombParachute", NativeTypeConstructorFamily::AnimType),
        ("DropZoneAnim", NativeTypeConstructorFamily::AnimType),
        ("EMPulseSparkles", NativeTypeConstructorFamily::AnimType),
        ("LargeVisceroid", NativeTypeConstructorFamily::UnitType),
        ("SmallVisceroid", NativeTypeConstructorFamily::UnitType),
        ("DropPodWeapon", NativeTypeConstructorFamily::WeaponType),
        (
            "ExplosiveVoxelDebris",
            NativeTypeConstructorFamily::VoxelAnimType,
        ),
        (
            "TireVoxelDebris",
            NativeTypeConstructorFamily::VoxelAnimType,
        ),
        (
            "ScrapVoxelDebris",
            NativeTypeConstructorFamily::VoxelAnimType,
        ),
        ("RepairBay", NativeTypeConstructorFamily::BuildingType),
        ("GDIGateOne", NativeTypeConstructorFamily::BuildingType),
        ("GDIGateTwo", NativeTypeConstructorFamily::BuildingType),
        ("NodGateOne", NativeTypeConstructorFamily::BuildingType),
        ("NodGateTwo", NativeTypeConstructorFamily::BuildingType),
        ("WallTower", NativeTypeConstructorFamily::BuildingType),
        ("Shipyard", NativeTypeConstructorFamily::BuildingType),
        ("GDIPowerPlant", NativeTypeConstructorFamily::BuildingType),
        ("NodRegularPower", NativeTypeConstructorFamily::BuildingType),
        (
            "NodAdvancedPower",
            NativeTypeConstructorFamily::BuildingType,
        ),
        ("ThirdPowerPlant", NativeTypeConstructorFamily::BuildingType),
        (
            "PrerequisiteProcAlternate",
            NativeTypeConstructorFamily::UnitType,
        ),
        ("BaseUnit", NativeTypeConstructorFamily::UnitType),
        ("HarvesterUnit", NativeTypeConstructorFamily::UnitType),
        ("PadAircraft", NativeTypeConstructorFamily::AircraftType),
        ("Paratrooper", NativeTypeConstructorFamily::InfantryType),
        ("SecretInfantry", NativeTypeConstructorFamily::InfantryType),
        ("SecretUnits", NativeTypeConstructorFamily::UnitType),
        ("SecretBuildings", NativeTypeConstructorFamily::BuildingType),
        ("AlliedDisguise", NativeTypeConstructorFamily::InfantryType),
        ("SovietDisguise", NativeTypeConstructorFamily::InfantryType),
        ("ThirdDisguise", NativeTypeConstructorFamily::InfantryType),
        ("Engineer", NativeTypeConstructorFamily::InfantryType),
        ("Technician", NativeTypeConstructorFamily::InfantryType),
        ("Pilot", NativeTypeConstructorFamily::InfantryType),
        ("AlliedCrew", NativeTypeConstructorFamily::InfantryType),
        ("SovietCrew", NativeTypeConstructorFamily::InfantryType),
        ("ThirdCrew", NativeTypeConstructorFamily::InfantryType),
        ("AmerParaDropInf", NativeTypeConstructorFamily::InfantryType),
        ("AllyParaDropInf", NativeTypeConstructorFamily::InfantryType),
        ("SovParaDropInf", NativeTypeConstructorFamily::InfantryType),
        ("YuriParaDropInf", NativeTypeConstructorFamily::InfantryType),
        ("AnimToInfantry", NativeTypeConstructorFamily::InfantryType),
        ("LightningWarhead", NativeTypeConstructorFamily::WarheadType),
        ("PrismType", NativeTypeConstructorFamily::BuildingType),
        ("V3RocketType", NativeTypeConstructorFamily::AircraftType),
        ("DMislType", NativeTypeConstructorFamily::AircraftType),
        ("CMislType", NativeTypeConstructorFamily::AircraftType),
        (
            "VeinholeTypeClass",
            NativeTypeConstructorFamily::TerrainType,
        ),
        (
            "DefaultMirageDisguises",
            NativeTypeConstructorFamily::TerrainType,
        ),
    ];

    assert_eq!(AI_KEYS.len(), 20);
    assert_eq!(GENERAL_SITES.len(), 89);
    let mut rules = String::from("[AI]\n");
    let mut expected = Vec::new();
    for (index, key) in AI_KEYS.iter().enumerate() {
        let id = format!("AI{index:02}");
        writeln!(rules, "{key}={id}").unwrap();
        expected.push((NativeTypeConstructorFamily::BuildingType, id));
    }
    rules.push_str("[General]\n");
    for (index, &(key, family)) in GENERAL_SITES.iter().enumerate() {
        let id = format!("G{index:02}");
        writeln!(rules, "{key}={id}").unwrap();
        expected.push((family, id));
    }
    rules.push_str("ParaDropPlane=SHOULD_NOT_EXIST\n");

    let processed = RulesLayerStack::new(IniFile::from_str(&rules))
        .process()
        .expect("all AI/General sites process");
    let actual = processed
        .native_type_construction_trace()
        .events()
        .iter()
        .map(|event| (event.family(), event.native_stored_id().to_string()))
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
    assert!(actual.iter().all(|(_, id)| id != "SHOULD_NOT_EXIST"));
}

#[test]
fn warhead_particle_system_and_voxel_children_obey_passed_family_timing() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[Warheads]\n0=WH\n\
         [VoxelAnims]\n0=VX\n\
         [Particles]\n0=PART\n\
         [ParticleSystems]\n0=PS\n\
         [WH]\nParticle=WHPS\nAnimList=WHA\nDebrisTypes=WHVX\n\
         [PART]\nWarhead=PARTWH\n\
         [PARTWH]\nAnimList=TOO_LATE_WARHEAD_BODY\n\
         [PS]\nBehavesLike=Smoke\n\
         [WHPS]\nHoldsWhat=WHOLD\n\
         [VX]\nBounceAnim=XBA\nExpireAnim=XEA\nTrailerAnim=XTA\nWarhead=XWH\nAttachedSystem=XPS\n\
         [WHVX]\nBounceAnim=YBA\nExpireAnim=YEA\nTrailerAnim=YTA\nWarhead=YWH\nAttachedSystem=YPS\n\
         [XPS]\nHoldsWhat=TOO_LATE_PARTICLE_SYSTEM_BODY\n",
    ))
    .process()
    .expect("late-family timing fixture processes");
    let actual = processed
        .native_type_construction_trace()
        .events()
        .iter()
        .map(|event| (event.family(), event.native_stored_id()))
        .collect::<Vec<_>>();

    assert_eq!(
        actual,
        vec![
            (NativeTypeConstructorFamily::WarheadType, "WH"),
            (NativeTypeConstructorFamily::VoxelAnimType, "VX"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS"),
            (NativeTypeConstructorFamily::ParticleSystemType, "WHPS"),
            (NativeTypeConstructorFamily::AnimType, "WHA"),
            (NativeTypeConstructorFamily::VoxelAnimType, "WHVX"),
            (NativeTypeConstructorFamily::WarheadType, "PARTWH"),
            (NativeTypeConstructorFamily::AnimType, "XBA"),
            (NativeTypeConstructorFamily::AnimType, "XEA"),
            (NativeTypeConstructorFamily::AnimType, "XTA"),
            (NativeTypeConstructorFamily::WarheadType, "XWH"),
            (NativeTypeConstructorFamily::ParticleSystemType, "XPS"),
            (NativeTypeConstructorFamily::AnimType, "YBA"),
            (NativeTypeConstructorFamily::AnimType, "YEA"),
            (NativeTypeConstructorFamily::AnimType, "YTA"),
            (NativeTypeConstructorFamily::WarheadType, "YWH"),
            (NativeTypeConstructorFamily::ParticleSystemType, "YPS"),
        ]
    );
    // [PS] omits HoldsWhat: the empty default reaches ParticleType's factory
    // (0x00645430), which constructs an empty-named type.
    assert_eq!(
        processed
            .ini()
            .section("Particles")
            .expect("Particle registry")
            .registry_ids(),
        vec!["PART", "", "WHOLD"]
    );
    assert!(actual.iter().all(|(_, id)| {
        !matches!(
            *id,
            "TOO_LATE_WARHEAD_BODY" | "TOO_LATE_PARTICLE_SYSTEM_BODY"
        )
    }));
}

#[test]
fn negative_tiberium_slot_is_an_explicit_safe_load_error() {
    let error = RulesLayerStack::new(IniFile::from_str("[Tiberiums]\n-1=BadSlot\n"))
        .process()
        .expect_err("native negative pre-array slot must not be accepted");
    assert!(matches!(
        error,
        RulesError::InvalidValue {
            ref section,
            ref key,
            ..
        } if section == "Tiberiums" && key == "-1"
    ));
}

#[test]
fn crate_rules_constructor_and_stock_fields_keep_native_bits() {
    let defaults = RulesLayerStack::new(IniFile::from_str(""))
        .process()
        .expect("crate rules process");
    assert_eq!(defaults.crate_rules().minimum, 1);
    assert_eq!(defaults.crate_rules().maximum, 255);
    assert_eq!(defaults.crate_rules().regen.bits(), 10.0_f64.to_bits());
    assert_eq!(defaults.crate_rules().wood_crate_img, None);
    assert_eq!(defaults.crate_rules().crate_img, None);
    assert_eq!(defaults.crate_rules().water_crate_img, None);

    let stock = RulesLayerStack::new(IniFile::from_str(
        "[CrateRules]\nCrateMinimum=1\nCrateMaximum=255\nCrateRegen=3\n\
         WoodCrateImg=CRATE\nCrateImg=CRATE\nWaterCrateImg=WCRATE\n",
    ))
    .process()
    .expect("crate rules process");
    assert_eq!(stock.crate_rules().minimum, 1);
    assert_eq!(stock.crate_rules().maximum, 255);
    assert_eq!(stock.crate_rules().regen.bits(), 3.0_f64.to_bits());
    assert_eq!(stock.crate_rules().wood_crate_img.as_deref(), Some("CRATE"));
    assert_eq!(stock.crate_rules().crate_img.as_deref(), Some("CRATE"));
    assert_eq!(
        stock.crate_rules().water_crate_img.as_deref(),
        Some("WCRATE")
    );
}

#[test]
fn crate_rules_retain_per_pass_and_preserve_signed_values() {
    let processed = process_rules_passes(
        "[CrateRules]\nCrateMinimum=-7\nCrateMaximum=-12\nCrateRegen=3\n\
         WoodCrateImg=WOOD\nCrateImg=COMMON\nWaterCrateImg=WATER\n",
        "[CrateRules]\nCrateMaximum=2\nWaterCrateImg=none\n",
    );
    let rules = processed.crate_rules();
    assert_eq!(rules.minimum, -7, "missing later key retains signed value");
    assert_eq!(rules.maximum, 2);
    assert_eq!(rules.regen.bits(), 3.0_f64.to_bits());
    assert_eq!(rules.wood_crate_img.as_deref(), Some("WOOD"));
    assert_eq!(rules.crate_img.as_deref(), Some("COMMON"));
    assert_eq!(rules.water_crate_img, None);

    let absent = process_rules_passes(
        "[CrateRules]\nCrateMinimum=9\nCrateMaximum=3\nCrateImg=FIRST\n",
        "[General]\nBuildSpeed=.7\n",
    );
    assert_eq!(absent.crate_rules().minimum, 9);
    assert_eq!(absent.crate_rules().maximum, 3);
    assert_eq!(absent.crate_rules().crate_img.as_deref(), Some("FIRST"));
}

#[test]
fn crate_rule_images_allocate_and_alias_by_overlay_identity() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[CrateRules]\nWoodCrateImg=AliasCrate\nCrateImg=aliascrate\n\
         WaterCrateImg=NewWater\n",
    ))
    .process()
    .expect("crate rules process");
    let overlays = processed
        .ini()
        .section("OverlayTypes")
        .expect("crate references allocate overlays");
    assert_eq!(overlays.registry_ids(), vec!["AliasCrate", "NewWater"]);

    let registry =
        crate::rules::overlay_types::OverlayTypeRegistry::from_ini(processed.ini(), None);
    assert_eq!(
        registry.id_for_name("AliasCrate"),
        registry.id_for_name("aliascrate")
    );
    assert_ne!(
        registry.id_for_name("AliasCrate"),
        registry.id_for_name("NewWater")
    );
}

#[test]
fn crate_rule_images_retain_the_stored_id_of_the_type_they_allocate() {
    let exact = "A".repeat(127);
    let over = "B".repeat(128);
    let processed = RulesLayerStack::new(IniFile::from_str(&format!(
        "[CrateRules]\nWoodCrateImg={exact}\nCrateImg={over}\nWaterCrateImg={over}Z\n"
    )))
    .process()
    .expect("crate rules process");

    // ReadString128 bounds each read, but every Type constructor stores only
    // the native 24-byte ID and Find compares the full input against that
    // stored prefix. A 127-byte name therefore misses every lookup and
    // constructs a duplicate stored ID per reference (see
    // `native_type_registry_compares_full_input_but_stores_and_emits_24_bytes`),
    // and each field keeps the Type the factory returned.
    let stored_a = "A".repeat(0x18);
    let stored_b = "B".repeat(0x18);
    let crate_rules = processed.crate_rules();
    assert_eq!(
        crate_rules.wood_crate_img.as_deref(),
        Some(stored_a.as_str())
    );
    assert_eq!(crate_rules.crate_img.as_deref(), Some(stored_b.as_str()));
    assert_eq!(
        crate_rules.water_crate_img.as_deref(),
        Some(stored_b.as_str())
    );
    assert_eq!(
        processed
            .ini()
            .section("OverlayTypes")
            .expect("truncated references allocate")
            .registry_ids(),
        vec![stored_a.as_str(), stored_b.as_str(), stored_b.as_str()],
    );
}

#[test]
fn crate_rules_direct_and_one_layer_entry_points_agree() {
    let ini = IniFile::from_str(
        "[CrateRules]\nCrateMinimum=-2\nCrateMaximum=6\nCrateRegen=3\n\
         WoodCrateImg=WOOD\nCrateImg=COMMON\nWaterCrateImg=WATER\n",
    );
    let direct = crate::rules::ruleset::RuleSet::from_ini(&ini).expect("direct rules");
    let layered =
        crate::rules::ruleset::RuleSet::from_rules_layers(&RulesLayerStack::new(ini.clone()))
            .expect("layered rules");
    assert_eq!(direct.crate_rules, layered.crate_rules);
    assert_eq!(direct.source_ini_hash(), layered.source_ini_hash());
}

#[test]
fn later_malformed_weapon_bool_preserves_current_field_default() {
    use crate::rules::weapon_type::WeaponType;

    let processed = process_rules_passes(
        "[VehicleTypes]\n0=TANK\n[TANK]\nPrimary=Gun\n[Gun]\nRevealOnFire=yes\n",
        "[Gun]\nRevealOnFire=maybe\n",
    );
    let section = processed.ini().section("Gun").expect("allocated Gun body");

    assert_eq!(section.get_for_test("RevealOnFire"), Some("maybe"));
    assert!(WeaponType::from_ini_section("Gun", section).reveal_on_fire);
}

#[test]
fn later_allocated_type_does_not_read_earlier_orphan_body() {
    let processed = process_rules_passes(
        "[LATE]\nStrength=900\nCost=700\n",
        "[VehicleTypes]\n0=LATE\n",
    );
    let late = processed.ini().section("LATE").expect("allocated body");
    assert_eq!(late.get_for_test("Strength"), None);
    assert_eq!(late.get_for_test("Cost"), None);
}

#[test]
fn existing_type_keeps_prior_fields_and_applies_current_body() {
    let processed = process_rules_passes(
        "[VehicleTypes]\n0=EARLY\n[EARLY]\nStrength=100\nCost=700\n",
        "[EARLY]\nStrength=250\n",
    );
    let early = processed.ini().section("EARLY").expect("EARLY body");
    assert_eq!(early.get_for_test("Strength"), Some("250"));
    assert_eq!(early.get_for_test("Cost"), Some("700"));
}

#[test]
fn tiberium_pass_reuses_numeric_slot_and_ignores_replacement_identity() {
    let processed = process_rules_passes(
        "[Tiberiums]\n0=Riparius\n[Riparius]\nImage=1\nValue=25\n",
        "[Tiberiums]\n0=Cruentus\n[Riparius]\nImage=4\n[Cruentus]\nImage=2\n",
    );

    assert_eq!(
        processed.ini().section("Tiberiums").unwrap().registry_ids(),
        vec!["Riparius"]
    );
    let riparius = processed.ini().section("Riparius").unwrap();
    assert_eq!(riparius.get_for_test("Image"), Some("4"));
    assert_eq!(riparius.get_for_test("Value"), Some("25"));
    assert!(processed.ini().section("Cruentus").is_some());
}

#[test]
fn tiberium_out_of_range_slot_appends_one_live_type() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[Tiberiums]\n7=Riparius\n0=Cruentus\n[Riparius]\nImage=1\n[Cruentus]\nImage=2\n",
    ))
    .process()
    .expect("Tiberium pass processes");

    assert_eq!(
        processed.ini().section("Tiberiums").unwrap().registry_ids(),
        vec!["Riparius"]
    );
    assert_eq!(
        processed
            .ini()
            .section("Riparius")
            .and_then(|section| section.get_for_test("Image")),
        Some("1")
    );
}

#[test]
fn new_type_reads_its_same_pass_body() {
    let processed = process_rules_passes(
        "[VehicleTypes]\n0=EARLY\n[EARLY]\nStrength=100\n",
        "[VehicleTypes]\n0=LATE\n[LATE]\nStrength=250\n",
    );
    assert_eq!(
        processed
            .ini()
            .section("LATE")
            .and_then(|section| section.get_for_test("Strength")),
        Some("250")
    );
}

#[test]
fn ordered_pass_registry_union_is_case_insensitive_by_value() {
    let processed = process_rules_passes(
        "[VehicleTypes]\nFirst=MTNK\n",
        "[VehicleTypes]\n0=mtnk\nAgain=HTNK\n",
    );
    assert_eq!(
        processed
            .ini()
            .section("VehicleTypes")
            .unwrap()
            .registry_ids(),
        vec!["MTNK", "HTNK"]
    );
}

#[test]
fn gsi_05_01_type_allocation_rejects_native_none_sentinels() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=none\n1=<NONE>\n2= NONE \n3=\t<none>\t\n4=NONE_TANK\n",
    ))
    .process()
    .expect("native none fixture processes");

    assert_eq!(
        processed
            .ini()
            .section("VehicleTypes")
            .expect("rebuilt vehicle registry")
            .registry_ids(),
        vec!["NONE_TANK"]
    );

    let mut processor = RulesPassProcessor::default();
    assert_eq!(
        processor.find_or_allocate(RulesTypeFamily::Vehicle, ""),
        None
    );
    assert_eq!(
        processor.find_or_allocate(RulesTypeFamily::Vehicle, "none"),
        None
    );
    assert_eq!(
        processor.find_or_allocate(RulesTypeFamily::Vehicle, "<NONE>"),
        None
    );
}

#[test]
fn later_pass_missing_scalar_key_preserves_live_value() {
    let processed = process_rules_passes(
        "[General]\nBuildSpeed=.7\nFlightLevel=1500\n",
        "[General]\nBuildSpeed=.58\n",
    );
    let general = processed.ini().section("General").unwrap();
    assert_eq!(general.get_for_test("BuildSpeed"), Some(".58"));
    assert_eq!(general.get_for_test("FlightLevel"), Some("1500"));
}

#[test]
fn later_general_section_without_damage_fire_types_preserves_live_list() {
    let processed = process_rules_passes(
        "[General]\nDamageFireTypes=FIRE01,FIRE02\nBuildSpeed=.7\n",
        "[General]\nBuildSpeed=.58\n",
    );
    let general = processed.ini().section("General").unwrap();
    assert_eq!(
        general.get_for_test("DamageFireTypes"),
        Some("FIRE01,FIRE02")
    );
    assert_eq!(general.get_for_test("BuildSpeed"), Some(".58"));
}

#[test]
fn general_prerequisite_groups_are_lookup_only() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[General]\nPrerequisitePower=MAPPOWR\n[MAPPOWR]\nStrength=750\n",
    ))
    .process()
    .expect("prerequisite lookup fixture processes");

    assert!(
        processed
            .ini()
            .section("BuildingTypes")
            .unwrap()
            .registry_ids()
            .is_empty()
    );
    assert!(processed.prerequisite_lists()[PrerequisiteGroup::Power.index()].is_empty());
}

#[test]
fn general_prerequisite_groups_keep_only_registered_buildings() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[BuildingTypes]\n0=GAPOWR\n[General]\nPrerequisitePower=gapowr,MISSING\n",
    ))
    .process()
    .expect("prerequisite registration fixture processes");

    assert_eq!(
        processed.prerequisite_lists()[PrerequisiteGroup::Power.index()],
        [Prerequisite::Building(0)]
    );
}

#[test]
fn prerequisite_proc_alternate_allocates_unit_before_same_pass_body_sweep() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[General]\nPrerequisiteProcAlternate=SMIN\n[SMIN]\nStrength=2000\n",
    ))
    .process()
    .expect("ProcAlternate fixture processes");

    assert_eq!(
        processed
            .ini()
            .section("VehicleTypes")
            .unwrap()
            .registry_ids(),
        vec!["SMIN"]
    );
    assert_eq!(
        processed
            .ini()
            .section("SMIN")
            .and_then(|section| section.get_for_test("Strength")),
        Some("2000")
    );
}

#[test]
fn barrel_particle_allocates_particle_system_before_same_pass_body_sweep() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[General]\nBarrelParticle=BarrelSys\n[BarrelSys]\nHoldsWhat=SmokePart\n[SmokePart]\nDamage=5\n",
    ))
    .process()
    .expect("particle-system fixture processes");

    assert_eq!(
        processed
            .ini()
            .section("ParticleSystems")
            .unwrap()
            .registry_ids(),
        vec!["BarrelSys"]
    );
    assert_eq!(
        processed.ini().section("Particles").unwrap().registry_ids(),
        vec!["SmokePart"]
    );
    assert_eq!(
        processed
            .ini()
            .section("BarrelSys")
            .and_then(|section| section.get_for_test("HoldsWhat")),
        Some("SmokePart")
    );
    assert_eq!(
        processed
            .ini()
            .section("SmokePart")
            .and_then(|section| section.get_for_test("Damage")),
        None,
        "HoldsWhat allocates the particle after the particle body sweep"
    );
}

#[test]
fn special_weapons_created_warhead_reads_same_pass_body() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[SpecialWeapons]\nMutateWarhead=FreshMutate\n\
         [FreshMutate]\nCellSpread=3\nVerses=100%,80%\n",
    ))
    .process()
    .expect("SpecialWeapons Warhead fixture processes");

    assert_eq!(
        processed
            .ini()
            .section("FreshMutate")
            .and_then(|section| section.get_for_test("CellSpread")),
        Some("3")
    );
}

#[test]
fn special_weapons_created_projectile_waits_for_next_pass_body_sweep() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[SpecialWeapons]\nNukeProjectile=FreshNuke\n[FreshNuke]\nImage=NUKE\n",
    ))
    .process()
    .expect("SpecialWeapons Bullet fixture processes");

    assert_eq!(
        processed
            .ini()
            .section("FreshNuke")
            .and_then(|section| section.get_for_test("Image")),
        None
    );
}

#[test]
fn combat_damage_allocates_late_smudge_and_animation_references() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[CombatDamage]\n\
         Scorches=BurnA,BurnB\n\
         Scorches1=BurnC\n\
         Scorches2=BurnD\n\
         Scorches3=BurnE\n\
         Scorches4=BurnF\n\
         SplashList=SplashA,SplashB\n\
         DrainAnimationType=DrainAnim\n\
         ControlledAnimationType=MindAnim\n\
         PermaControlledAnimationType=MindAnimR\n\
         [BurnA]\nWidth=3\n",
    ))
    .process()
    .expect("CombatDamage fixture processes");

    assert_eq!(
        processed
            .ini()
            .section("SmudgeTypes")
            .unwrap()
            .registry_ids(),
        vec!["BurnA", "BurnB", "BurnC", "BurnD", "BurnE", "BurnF"]
    );
    assert_eq!(
        processed
            .ini()
            .section("Animations")
            .unwrap()
            .registry_ids(),
        vec!["SplashA", "SplashB", "DrainAnim", "MindAnim", "MindAnimR"]
    );
    assert_eq!(
        processed
            .ini()
            .section("BurnA")
            .and_then(|section| section.get_for_test("Width")),
        None,
        "late CombatDamage allocations wait for the next type-data pass"
    );
}

#[test]
fn rules_hash_preserves_pass_boundaries() {
    let single = RulesLayerStack::new(IniFile::from_str("[General]\nBuildSpeed=.58\n"));
    let mut layered = RulesLayerStack::new(IniFile::from_str("[General]\nBuildSpeed=.7\n"));
    layered.push(
        RulesLayerKind::LangRule,
        IniFile::from_str("[General]\nBuildSpeed=.58\n"),
    );
    assert_eq!(
        single
            .process()
            .expect("single Rules pass processes")
            .ini()
            .section("General")
            .unwrap()
            .get_for_test("BuildSpeed"),
        layered
            .process()
            .expect("layered Rules passes process")
            .ini()
            .section("General")
            .unwrap()
            .get_for_test("BuildSpeed")
    );
    assert_ne!(single.content_hash(), layered.content_hash());
}

#[test]
fn projectiles_and_sides_keep_ordinary_compatibility_projection_overlay() {
    let processed = process_rules_passes(
        "[Projectiles]\n0=KEEP\n[Sides]\nGDI=Americans\n",
        "[Projectiles]\n0=REPLACE\n[Sides]\nGDI=French\n",
    );
    assert_eq!(
        processed
            .ini()
            .section("Projectiles")
            .unwrap()
            .get_for_test("0"),
        Some("REPLACE")
    );
    assert_eq!(
        processed
            .ini()
            .section("Sides")
            .unwrap()
            .get_for_test("GDI"),
        Some("French")
    );
}

/// Sections not proven to be native registries keep ordinary overlay behavior.
#[test]
fn existing_unlisted_numbered_section_receives_ordinary_overlay() {
    let rules = IniFile::from_str("[FutureTypes]\n0=KEEP\n");
    let map = IniFile::from_str("[FutureTypes]\n0=EVIL\n");
    let rules = process_ini_passes(rules, map).into_projection_discarding_native_receipt();
    assert_eq!(
        rules.section("FutureTypes").unwrap().get_for_test("0"),
        Some("EVIL")
    );
}

/// Registry processing uses every entry value, including entries whose keys
/// are not numeric.
#[test]
fn map_registry_pass_uses_every_entry_in_source_order() {
    let rules =
        IniFile::from_str("[Particles]\n30=FireStream\n[ParticleSystems]\n10=GasCloudSys\n");
    let map = IniFile::from_str(
        "[Particles]\n30=EvilFire\nName=oops\n[ParticleSystems]\n10=EvilSys\nStray=1\n",
    );
    let rules = process_ini_passes(rules, map).into_projection_discarding_native_receipt();
    assert_eq!(
        rules.section("Particles").unwrap().registry_ids(),
        vec!["FireStream", "EvilFire", "oops"]
    );
    assert_eq!(
        rules.section("ParticleSystems").unwrap().registry_ids(),
        vec!["GasCloudSys", "EvilSys", "1"]
    );
}

/// Empty-value semantics: a map line `BuildSpeed=` with no value is
/// "key absent" in the original (its INI writer gates out empty/NULL values
/// and never stores an entry, so the readers keep the value already on the
/// field). It must NOT clobber the value from an earlier rules pass — clobbering it
/// with `""` would reset the field to the hardcoded Rust default at parse time.
#[test]
fn map_overrides_skip_empty_valued_keys() {
    let rules = IniFile::from_str(
        "[General]\nBuildSpeed=.7\nFlightLevel=1500\n[CombatDamage]\nC4Delay=.03\n",
    );
    let map = IniFile::from_str("[General]\nBuildSpeed=\n[CombatDamage]\nC4Delay=.06\n");
    let rules = process_ini_passes(rules, map).into_projection_discarding_native_receipt();
    assert_eq!(
        rules.section("General").unwrap().get_for_test("BuildSpeed"),
        Some(".7"),
        "empty map value must leave the merged value intact"
    );
    assert_eq!(
        rules
            .section("CombatDamage")
            .unwrap()
            .get_for_test("C4Delay"),
        Some(".06")
    );
}

/// `[Colors]` is a find-or-create registry: an existing case-insensitive
/// identity keeps its first HSV value, while a new name allocates.
#[test]
fn map_colors_keep_existing_identity_and_allocate_new_name() {
    let rules = IniFile::from_str("[Colors]\nGold=42,252,252\nDarkRed=0,151,239\n");
    let map = IniFile::from_str("[Colors]\ngold=1,2,3\nNeonPink=12,200,255\n");
    let rules = process_ini_passes(rules, map).into_projection_discarding_native_receipt();
    assert_eq!(
        rules.section("Colors").unwrap().get_for_test("Gold"),
        Some("42,252,252")
    );
    assert_eq!(
        rules.section("Colors").unwrap().get_for_test("NeonPink"),
        Some("12,200,255")
    );
}

/// Both stateful readers run through the normal ordered Process owner. Expected
/// identities come from original reader execution, not a second Rust parser.
#[test]
fn secret_lab_readers_match_original_constructor_retention_null_and_case_controls() {
    use crate::rules::ruleset::RuleSet;
    use serde_json::{Value, json};
    let native: Value =
        serde_json::from_str(crate::test_fixture::text("tools/can_build_secret_lab.json")).unwrap();
    for section in ["retained_history", "scalar_history"] {
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            // Original reader fixture supplies registered uppercase identities
            // before any lookup, including U0/B1 not named by its first pass.
            "[InfantryTypes]\n0=I0\n1=I1\n[VehicleTypes]\n0=U0\n1=U1\n\
             [BuildingTypes]\n0=B0\n1=B1\n2=LAB\n[LAB]\nStrength=100\n",
        ));
        let rows = std::iter::once((&native["readers"]["constructor"], None)).chain(
            native["readers"][section]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| (&row["state"], Some(row))),
        );
        for (expected, row) in rows {
            if let Some(row) = row {
                let mut ini = IniFile::empty();
                for (name, entries) in row["sections"].as_object().unwrap() {
                    let mut section = IniSection::new(name.to_owned());
                    for (key, value) in entries.as_object().unwrap() {
                        section.set(key, value.as_str().unwrap());
                    }
                    // Native skips absent sections; an empty [LAB] here is a
                    // reached body only if entries exist, as in physical INIs.
                    if !entries.as_object().unwrap().is_empty() {
                        ini.replace_first_section(section);
                    }
                }
                layers.push(RulesLayerKind::Scenario, ini);
            }
            let processed = layers.process().unwrap();
            let rules = RuleSet::from_processed_rules(&processed).unwrap();
            let lab = rules.object("LAB").unwrap().secret_lab();
            let lists = rules.general.secret_lab_choices();
            let overrides = lab.overrides_for_test();
            assert_eq!(
                json!({
                    "secret_lab": lab.enabled(),
                    "overrides": { "infantry": overrides[0], "unit": overrides[1], "building": overrides[2] },
                    "lists": { "infantry": lists[0], "unit": lists[1], "building": lists[2] },
                    "total": lists.iter().map(Vec::len).sum::<usize>(),
                }),
                *expected,
                "{section}: {row:?}"
            );
        }
    }
}

#[test]
fn secret_lab_stock_rules_and_mode_use_native_lists_and_unoverridden_lab() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let native = crate::test_fixture::json("tools/can_build_secret_lab.json");
    let expected = &native["readers"]["physical_layers"][0]["state"];
    let lists = retail.rules.general.secret_lab_choices();
    for (slot, key) in ["infantry", "unit", "building"].into_iter().enumerate() {
        assert_eq!(
            serde_json::to_value(&lists[slot]).unwrap(),
            expected["lists"][key]
        );
    }
    let lab = retail.rules.object("CASLAB").unwrap().secret_lab();
    assert!(lab.enabled());
    assert_eq!(lab.production_override(), None);
}

/// Original full Rules ctor and66E5D5/66B385 read-block controls, Steam
///gamemd: pointer identity/empty retention and signed ReadRange.
#[test]
fn waypoint_cue_drop_zone_rules_keep_native_defaults_layers_and_saved_state() {
    use crate::rules::ruleset::RuleSet;
    let publish = |p: &ProcessedRulesLayers| RuleSet::from_processed_rules(p).unwrap();
    let mut layers = RulesLayerStack::new(IniFile::empty());
    let initial = publish(&layers.process().unwrap());
    assert_eq!(initial.general.drop_zone_anim, None);
    assert_eq!(initial.general.drop_zone_radius, 1024);
    for (text, name, radius) in [
        (
            "[General]\nDropZoneAnim=BEACON\n[AudioVisual]\nDropZoneRadius=4\n",
            Some("BEACON"),
            1024,
        ),
        (
            "[General]\nDropZoneAnim=\n[AudioVisual]\nDropZoneRadius=\n",
            Some("BEACON"),
            1024,
        ),
        ("[AudioVisual]\nDropZoneRadius=2.75\n", Some("BEACON"), 704),
        ("[AudioVisual]\nDropZoneRadius=-1\n", Some("BEACON"), 704),
        ("[AudioVisual]\nDropZoneRadius=-0.5\n", Some("BEACON"), -128),
        (
            "[General]\ndropzoneanim=OTHER\n[AudioVisual]\ndropzoneradius=12\n",
            Some("BEACON"),
            -128,
        ),
        ("[General]\nDropZoneAnim=none\n", None, -128),
        ("[General]\nName=Missing keys\n", None, -128),
        ("[General]\nDropZoneAnim=beacon\n", Some("BEACON"), -128),
        ("[General]\nDropZoneAnim=<none>\n", None, -128),
    ] {
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(text));
        let current = publish(&layers.process().unwrap());
        assert_eq!(current.general.drop_zone_anim.as_deref(), name, "{text}");
        assert_eq!(current.general.drop_zone_radius, radius, "{text}");
    }
    let first = RulesLayerStack::new(IniFile::from_str(
        "[General]\nDropZoneAnim=BEACON\n[AudioVisual]\nDropZoneRadius=2.75\n",
    ))
    .process()
    .unwrap();
    let (_, trace) = first.into_ini_and_native_type_construction_trace();
    let saved = save_codec::serialize(&trace.into_registry_state_discarding_events()).unwrap();
    let retained: NativeRulesRegistryState = save_codec::deserialize(&saved).unwrap();
    let next_layers = RulesLayerStack::new(IniFile::from_str("[General]\nName=Same sources\n"));
    let continued = next_layers
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            save_codec::deserialize(&saved).unwrap(),
        )
        .unwrap();
    let continued = publish(&continued);
    assert_eq!(continued.general.drop_zone_anim.as_deref(), Some("BEACON"));
    assert_eq!(continued.general.drop_zone_radius, 704);
    let cold = publish(&next_layers.process().unwrap());
    assert_ne!(
        continued.simulation_config_hash(),
        cold.simulation_config_hash()
    );
    let reset = next_layers
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), retained.destructive_reset())
        .unwrap();
    let reset = publish(&reset);
    assert_eq!(
        reset.general.drop_zone_anim, None,
        "retired type references are cleared"
    );
    assert_eq!(
        reset.general.drop_zone_radius, 704,
        "Rules numeric field survives Type reset"
    );
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    // The original reader on physical RULESMD's exact keys yields BEACON/1024.
    // The production fixture also reads optional LANGRULE, Battle, Hills and ARTMD.
    assert_eq!(
        retail.rules.general.drop_zone_anim.as_deref(),
        Some("BEACON")
    );
    assert_eq!(retail.rules.general.drop_zone_radius, 1024);
}

#[test]
fn spotlight_rules_keep_constructor_bits_native_parser_and_retained_pass_defaults() {
    use crate::rules::ruleset::{RuleSet, SpotlightRules};
    let mut layers = RulesLayerStack::new(IniFile::from_str("[General]\n"));
    assert_eq!(
        RuleSet::from_processed_rules(&layers.process().unwrap())
            .unwrap()
            .general
            .spotlight,
        SpotlightRules::default()
    );
    layers.push(RulesLayerKind::GameMode,IniFile::from_str("[General]\nSpotlightMovementRadius=-123tail\nSpotlightSpeed=.015\nSpotlightAngle=.5\nSpotlightRadius=201\n"));
    layers.push(RulesLayerKind::Scenario,IniFile::from_str("[General]\nSpotlightMovementRadius=bad\nSpotlightSpeed=bad\nSpotlightRadius=\nSpotlightangle=8\n"));
    let processed = layers.process().unwrap();
    let fields = RuleSet::from_processed_rules(&processed)
        .unwrap()
        .general
        .spotlight;
    // Present malformed numeric strings read zero; only absent/empty keys keep defaults.
    assert_eq!(fields.movement_radius, 0);
    assert_eq!(fields.radius, 201);
    assert_eq!(fields.speed.bits(), 0);
    assert_eq!(fields.angle.bits(), 0x3fe0000000000000);
    let (_, trace) = processed.into_ini_and_native_type_construction_trace();
    let retained = trace
        .into_registry_state_discarding_events()
        .destructive_reset();
    let next = RulesLayerStack::new(IniFile::from_str("[General]\n"))
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), retained)
        .unwrap();
    assert_eq!(
        RuleSet::from_processed_rules(&next)
            .unwrap()
            .general
            .spotlight,
        fields
    );
}
#[test]
fn spotlight_retail_inputs_use_the_production_layered_reader() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let r = retail.rules.general.spotlight;
    assert_eq!(
        (r.movement_radius, r.location_radius, r.radius),
        (2000, 1000, 175)
    );
    assert_eq!(r.speed.bits(), 0x3f8eb851e0000000);
    assert_eq!(r.acceleration.bits(), 0x3f647ae140000000);
    assert_eq!(r.angle.bits(), 0x3fe0000000000000);
}

#[test]
fn spotlight_type_flag_retains_the_native_current_default_across_rules_layers() {
    use crate::rules::ruleset::RuleSet;
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[BuildingTypes]\n0=LIGHT\n[LIGHT]\nHasSpotlight=yes\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[LIGHT]\nHasSpotlight=invalid\n"),
    );
    assert!(
        RuleSet::from_processed_rules(&layers.process().unwrap())
            .unwrap()
            .object("LIGHT")
            .unwrap()
            .has_spotlight
    );
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[LIGHT]\nHasSpotlight=no\n"),
    );
    assert!(
        !RuleSet::from_processed_rules(&layers.process().unwrap())
            .unwrap()
            .object("LIGHT")
            .unwrap()
            .has_spotlight
    );
}
