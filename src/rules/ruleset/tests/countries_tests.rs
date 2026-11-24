//! Tests of object costs and the country and side rules.

use super::*;

/// P7: per-country IncomeMult parses to PPM, defaults to the neutral 1.0×, and looks
/// up case-insensitively. The hand-written Default must NOT be a derived zero.
#[test]
fn cost_of_matches_the_original_cost_virtuals() {
    let rows: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/cost_of.json",
    ))
    .unwrap();
    let bits = |value: &serde_json::Value| -> [NativeF32Bits; 5] {
        let bits: Vec<u32> = value
            .as_array()
            .unwrap()
            .iter()
            .map(|bits| bits.as_u64().unwrap() as u32)
            .collect();
        std::array::from_fn(|slot| NativeF32Bits::from_bits(bits[slot]))
    };
    let rows = rows.as_array().unwrap();
    for row in rows {
        let input = &row["input"];
        let cost = input["cost"].as_i64().unwrap();
        let techno = input["techno"].as_str();
        let own_cost = |kind: &str, other: i64| if techno == Some(kind) { cost } else { other };
        let free = input["free"].as_i64();
        let pads = &input["pads"];
        let key = |set: bool, line: &'static str| if set { line } else { "" };
        let separate = if input["separate_aircraft"] == 1 {
            "yes"
        } else {
            "no"
        };
        let ini = format!(
            "[General]\nSeparateAircraft={}\nPadAircraft=PAD1,PAD2\n\
                 [BuildingTypes]\n0=BUILDING\n[VehicleTypes]\n0=FREE\n\
                 [InfantryTypes]\n0=INF\n[AircraftTypes]\n0=PAD1\n1=PAD2\n\
                 [BUILDING]\nCost={cost}\n{}{}\n[FREE]\nCost={}\n[INF]\nCost={cost}\n\
                 [PAD1]\nCost={}\n{}\n[PAD2]\nCost={}\n",
            separate,
            key(free.is_some(), "FreeUnit=FREE\n"),
            key(input["build_cat"] == 5, "BuildCat=Combat"),
            own_cost("unit", free.unwrap_or(0)),
            own_cost("aircraft", pads[0].as_i64().unwrap()),
            key(input["docks"] == true, "Dock=BUILDING"),
            pads[1].as_i64().unwrap(),
        );
        let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
        let object = match techno {
            None => "BUILDING",
            Some("unit") => "FREE",
            Some("infantry") => "INF",
            Some(_) => "PAD1",
        };
        let factors = HouseCostFactors {
            country: bits(&input["country"]),
            factory_plant: bits(&input["plant"]),
        };
        let house = (input["house"] == true).then_some(&factors);
        assert_eq!(
            rules.cost_of(rules.object(object).unwrap(), house),
            row["cost"].as_i64().unwrap() as i32,
            "native Cost_Of row {input}"
        );
    }
    assert_eq!(rows.len(), 90);
}

#[test]
fn country_cost_mults_read_doubles_into_floats() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Americans\n1=Russians\n\
             [Americans]\nCostInfantryMult=.8\nCostUnitsMult=85%\n",
    ))
    .unwrap();
    let one = NativeF32Bits::ONE;
    assert_eq!(
        rules.country_cost_mults("americans"),
        [
            NativeF32Bits::from_bits(0.8_f32.to_bits()),
            // 0.85000000000000009 stored under the chop control word.
            NativeF32Bits::from_bits(0x3F59_9999),
            one,
            one,
            one,
        ]
    );
    assert_eq!(rules.country_cost_mults("Russians"), [one; 5]);
    assert_eq!(rules.country_cost_mults("Nowhere"), [one; 5]);
}

#[test]
fn country_speed_mults_read_doubles_into_floats() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Americans\n1=Russians\n\
             [Americans]\nSpeedUnitsMult=1.15\nSpeedAircraftMult=85%\n",
    ))
    .unwrap();
    let one = NativeF32Bits::ONE;
    assert_eq!(
        rules.country_speed_mults("americans"),
        [
            one,
            NativeF32Bits::from_bits(1.15_f32.to_bits()),
            NativeF32Bits::from_bits(0x3F59_9999),
        ]
    );
    assert_eq!(rules.country_speed_mults("Russians"), [one; 3]);
}

/// No retail `RULESMD.INI` country authors a speed bonus, through the
/// production reader (mode INIs and maps are not checked).
#[test]
fn retail_countries_author_no_speed_mults() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    assert!(!rules.countries.is_empty());
    for (name, country) in &rules.countries {
        assert_eq!(country.speed_mults, [NativeF32Bits::ONE; 3], "{name}");
    }
}

#[test]
fn country_income_mult_parses_and_defaults() {
    assert_eq!(
        CountryRules::default().income_mult,
        NativeF32Bits::ONE,
        "default IncomeMult is 1.0x (a derived Default would zero it and wipe income)"
    );

    let src = format!(
        "{}\n[Countries]\n0=Americans\n1=Russia\n[Americans]\nIncomeMult=1.2\n[Russia]\nFixtureOnly=1\n",
        make_test_rules()
    );
    let ini = IniFile::from_str(&src);
    let rules = RuleSet::from_ini(&ini).expect("Should parse");
    assert_eq!(rules.country_income_mult("Americans").bits(), 0x3F99_999A);
    assert_eq!(rules.country_income_mult("Russia"), NativeF32Bits::ONE);
    assert_eq!(rules.country_income_mult("Nonexistent"), NativeF32Bits::ONE);
    assert_eq!(rules.country_income_mult("americans").bits(), 0x3F99_999A);
}

#[test]
fn native_income_and_purifier_field_readers_preserve_observed_bits() {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/refinery_income.json");
    let rows = corpus["reader_rows"].as_array().unwrap();
    assert_eq!(rows.len(), 12);
    for row in rows {
        let input = &row["input"];
        let field = input["field"].as_str().unwrap();
        let section = input["section"].as_str().unwrap();
        let current = NativeF32Bits::from_bits(
            u32::from_str_radix(input["default_bits"].as_str().unwrap(), 16).unwrap(),
        );
        let expected = NativeF32Bits::from_bits(
            u32::from_str_radix(row["output"]["bits"].as_str().unwrap(), 16).unwrap(),
        );
        let mut ini = IniFile::from_str(
            "[Countries]\n0=SYNTHCOUNTRY\n[SYNTHCOUNTRY]\nFixtureOnly=1\n[General]\nFixtureOnly=1\n",
        );
        if let Some(prefix) = input["prefix_raw"].as_str() {
            ini.merge(&IniFile::from_str(&format!(
                "[{section}]\n{field}={prefix}\n"
            )));
        }
        if let Some(raw) = input["raw"].as_str() {
            ini.merge(&IniFile::from_str(&format!("[{section}]\n{field}={raw}\n")));
        }
        let rules = RuleSet::from_ini(&ini).unwrap();
        let actual = match field {
            "IncomeMult" => rules.country_income_mult("SYNTHCOUNTRY"),
            "PurifierBonus" => rules.general.purifier_bonus,
            _ => unreachable!(),
        };
        assert_eq!(actual, expected, "{} {}", field, input["name"]);
        // Also compare the actual retained-default reader input, not just
        // the immutable view reconstructed from the processed layers.
        let raw = input["raw"]
            .as_str()
            .map_or(String::new(), |raw| format!("{field}={raw}\n"));
        let direct = IniFile::from_str(&format!("[{section}]\nFixtureOnly=1\n{raw}"));
        assert_eq!(
            direct
                .section(section)
                .unwrap()
                .read_double_to_float(field, current),
            expected
        );
    }
}

#[test]
fn gsi_04_07_placement_wall_owner_parses_exact_key_and_defaults_true() {
    assert!(CountryRules::default().wall_owner);
    let ini = IniFile::from_str(
        "[Countries]\n0=Allowed\n1=Denied\n\
             [Allowed]\nWallOwner=yes\n\
             [Denied]\nWallOwner=no\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("country registry parses");
    assert!(rules.country_wall_owner("allowed"));
    assert!(!rules.country_wall_owner("DENIED"));
    assert!(rules.country_wall_owner("unknown"));
}

#[test]
fn ordered_country_side_source_identity_and_case_insensitive_lookup() {
    let ini = IniFile::from_str(
        "[Countries]\n9=Zulu\n2=Alpha\n7=Middle\n\
             [Sides]\nNorth=Alpha,Middle\nSouth=Zulu\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("country/side registries parse");

    assert_eq!(rules.country_index("zulu"), Some(CountryIdx(0)));
    assert_eq!(rules.country_index("ALPHA"), Some(CountryIdx(1)));
    assert_eq!(rules.country_index("Middle"), Some(CountryIdx(2)));
    assert_eq!(rules.country_name(CountryIdx(1)), Some("Alpha"));
    assert_eq!(rules.side_index("north"), Some(SideIdx(0)));
    assert_eq!(rules.side_index("SOUTH"), Some(SideIdx(1)));
    assert_eq!(rules.side_name(SideIdx(0)), Some("North"));
}

/// Phobos `ValueableIdx<ColorScheme>` in each rules pass: a name finds the
/// shade-count-1 scheme of a `[Colors]` entry loaded so far, `none` stores
/// -1, and an empty or unknown name keeps the earlier pass's index.
#[cfg(feature = "phobos")]
#[test]
fn timer_blink_color_scheme_resolves_in_each_rules_pass() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    let scheme = |map: &str| {
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[Colors]\nGold=43,239,255\nGrey=0,0,131\nLightGrey=0,0,240\n\
                 [AudioVisual]\nTimerBlinkColorScheme=grey\n",
        ));
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(map));
        RuleSet::from_rules_layers(&layers)
            .expect("rules read")
            .general
            .timer_blink_color_scheme
    };
    // `Grey`, entry 1, is runtime scheme 2.
    assert_eq!(scheme(""), 2);
    assert_eq!(scheme("[AudioVisual]\nTimerBlinkColorScheme=\n"), 2);
    assert_eq!(scheme("[AudioVisual]\nTimerBlinkColorScheme=Nope\n"), 2);
    assert_eq!(scheme("[AudioVisual]\nTimerBlinkColorScheme=none\n"), -1);
    // The map's own [Colors] entry is loaded before the read.
    assert_eq!(
        scheme("[Colors]\nTeal=120,255,200\n[AudioVisual]\nTimerBlinkColorScheme=Teal\n"),
        6
    );
}

#[cfg(feature = "phobos")]
#[test]
fn missing_cameo_defaults_to_xxicon_and_reads_audio_visual() {
    let defaults = RuleSet::from_ini(&IniFile::from_str("[General]\n")).expect("rules read");
    assert_eq!(defaults.missing_cameo(), "XXICON.SHP");

    let named = RuleSet::from_ini(&IniFile::from_str(
        "[AudioVisual]\nMissingCameo=CUSTOM.PCX\n",
    ))
    .expect("rules read");
    assert_eq!(named.missing_cameo(), "CUSTOM.PCX");

    // Empty is no readable name, so the constructor default stands.
    let empty = RuleSet::from_ini(&IniFile::from_str("[AudioVisual]\nMissingCameo=\n"))
        .expect("rules read");
    assert_eq!(empty.missing_cameo(), "XXICON.SHP");

    // `PhobosFixedString::Read` clears the name on `none` or `<none>`.
    let none = RuleSet::from_ini(&IniFile::from_str("[AudioVisual]\nMissingCameo=<None>\n"))
        .expect("rules read");
    assert_eq!(none.missing_cameo(), "");
}

#[cfg(feature = "phobos")]
#[test]
fn sidebar_gdi_positions_default_the_first_side_and_read_side_sections() {
    let ini = IniFile::from_str(
        "[Countries]\n0=Alpha\n1=Beta\n2=Gamma\n\
             [Sides]\nNorth=Alpha\nSouth=Beta\nThird=Gamma\n\
             [North]\nSidebar.GDIPositions=no\n\
             [Third]\nSidebar.GDIPositions=yes\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("side sections parse");
    assert!(!rules.side_gdi_positions(0), "explicit no");
    assert!(!rules.side_gdi_positions(1), "default non-first side");
    assert!(rules.side_gdi_positions(2), "explicit yes");
    assert!(!rules.side_gdi_positions(99), "missing side is NOD");
    assert!(!rules.side_gdi_positions(-1), "negative side is NOD");

    let defaults = RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Alpha\n1=Beta\n[Sides]\nNorth=Alpha\nSouth=Beta\n",
    ))
    .expect("default side positions parse");
    assert!(defaults.side_gdi_positions(0));
    assert!(!defaults.side_gdi_positions(1));
}

#[test]
fn country_color_retains_the_catalog_present_at_each_reached_read() {
    // ReadColor474A90 resolves only currently registered schemes. The
    // original House controls observe Americans' Gold as native3;
    // Country ctor51141C and whole-section admission establish the0
    // boundary for the two unread/unresolved members below.
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[Colors]\nLightGold=25,255,255\nGold=43,239,255\n\
             [Countries]\n0=Americans\n1=UnknownAtRead\n2=MissingBody\n\
             [Americans]\nColor=Gold\n\
             [UnknownAtRead]\nColor=AddedLater\n",
    ));
    layers.push(
        crate::rules::native_processing::RulesLayerKind::Scenario,
        IniFile::from_str("[Colors]\nAddedLater=0,0,0\n[Americans]\nColor=UnknownName\n"),
    );
    let rules = RuleSet::from_rules_layers(&layers).expect("ordered Color reads");
    assert_eq!(rules.country_color_scheme("americans"), 3);
    assert_eq!(rules.country_color_scheme("UnknownAtRead"), 0);
    assert_eq!(rules.country_color_scheme("MissingBody"), 0);
    assert_eq!(rules.scenario_country_name(None), Some("Americans"));
    assert_eq!(rules.scenario_country_name(Some("unknown")), None);
}

#[test]
fn country_color_catalog_survives_handoff_and_is_cleared_by_type_reset() {
    let first = RulesLayerStack::new(IniFile::from_str(
        "[Colors]\nLightGold=25,255,255\nGold=43,239,255\n\
             [Countries]\n0=Americans\n[Americans]\nColor=Gold\n",
    ))
    .process()
    .unwrap();
    let (_, trace) = first.into_ini_and_native_type_construction_trace();
    let second = RulesLayerStack::new(IniFile::from_str("[Americans]\nColor=UnknownName\n"))
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            trace.into_registry_state_discarding_events(),
        )
        .unwrap();
    let rules = RuleSet::from_processed_rules(&second).unwrap();
    assert_eq!(rules.country_color_scheme("Americans"), 3);
    assert_eq!(
        rules
            .color_schemes
            .iter()
            .map(|scheme| scheme.name.as_str())
            .collect::<Vec<_>>(),
        ["LightGold", "Gold"]
    );
    let (_, trace) = second.into_ini_and_native_type_construction_trace();
    let after_reset = RulesLayerStack::new(IniFile::from_str(
        "[Colors]\nNewAccent=0,0,0\n[Countries]\n0=Americans\n\
             [Americans]\nColor=UnknownName\n",
    ))
    .process_with_fixed_art_and_registry_state(
        &IniFile::empty(),
        trace
            .into_registry_state_discarding_events()
            .destructive_reset(),
    )
    .unwrap();
    let rules = RuleSet::from_processed_rules(&after_reset).unwrap();
    assert_eq!(rules.country_color_scheme("Americans"), 0);
    assert_eq!(
        rules
            .color_schemes
            .iter()
            .map(|scheme| scheme.name.as_str())
            .collect::<Vec<_>>(),
        ["NewAccent"]
    );
}

#[test]
fn trigger_house_type_owner_uses_alias_then_id_and_native_signed_special() {
    let ini = IniFile::from_str(
        "[Countries]\n0=First\n1=Second\n2=Third\n\
             [First]\nName=Shared Alias\n\
             [Second]\nName=Shared Alias\n\
             [Third]\nName=Third Alias\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("country aliases parse");

    assert_eq!(
        rules.trigger_house_type_index("shared alias"),
        Some(CountryIdx(0)),
        "duplicate aliases keep first registration order"
    );
    assert_eq!(
        rules.trigger_house_type_index("second"),
        Some(CountryIdx(1))
    );
    assert_eq!(
        rules.trigger_house_type_index("THIRD ALIAS"),
        Some(CountryIdx(2))
    );
    assert_eq!(
        rules.trigger_house_type_index("<none>"),
        Some(CountryIdx(0))
    );
    assert_eq!(rules.trigger_house_type_index("<random>"), None);
    assert_eq!(rules.trigger_house_type_index("missing"), None);
    assert_eq!(
        rules.scenario_country_name(Some("shared alias")),
        Some("First")
    );
    assert_eq!(rules.scenario_country_name(Some("second")), Some("Second"));
}

#[test]
fn gsi_02_13_ruleset_exposes_parsed_color_add_table() {
    let ini = IniFile::from_str(
        "[ColorAdd]\n\
             First=1,2,3\n\
             StrongGreen=0,63,0\n\
             BrightWhite=31,63,31\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("ColorAdd rules parse");

    assert_eq!(rules.color_add.entries()[0].name.as_deref(), Some("First"));
    assert_eq!(rules.color_add.entries()[0].rgb, [1, 2, 3]);
    assert_eq!(
        rules.color_add.entries()[1].name.as_deref(),
        Some("StrongGreen")
    );
    assert_eq!(rules.color_add.entries()[1].rgb, [0, 63, 0]);
    assert_eq!(rules.color_add.entries()[2].rgb, [31, 63, 31]);
    assert_eq!(
        rules.color_add.entries()[15],
        crate::rules::color_add::ColorAddEntry::default()
    );
}

#[test]
fn ordered_country_side_country_override_moves_and_allocates_side() {
    let ini = IniFile::from_str(
        "[Countries]\n0=Alpha\n1=Beta\n2=Gamma\n\
             [Sides]\nGDI=Alpha,Beta\nNod=Gamma\n\
             [Beta]\nSide=Nod\n\
             [Gamma]\nSide=FourthSide\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("country side overrides parse");

    assert_eq!(rules.country_side_index("Alpha"), Some(SideIdx(0)));
    assert_eq!(rules.country_side_index("beta"), Some(SideIdx(1)));
    assert_eq!(rules.country_side_index("GAMMA"), Some(SideIdx(2)));
    assert_eq!(rules.side_name(SideIdx(2)), Some("FourthSide"));
    assert_eq!(rules.side_index("fourthside"), Some(SideIdx(2)));
}

#[test]
fn ordered_country_side_members_do_not_allocate_unknown_country() {
    let ini = IniFile::from_str(
        "[Countries]\n0=Alpha\n\
             [Sides]\nGDI=Alpha,Beta\n\
             [Beta]\nSide=NewSide\nMultiplayPassive=yes\n",
    );
    let processed = RulesLayerStack::new(ini)
        .process()
        .expect("Side members process");
    let rules = RuleSet::from_processed_rules(&processed).expect("registered Country projects");

    assert_eq!(rules.country_index("Beta"), None);
    assert_eq!(rules.side_index("NewSide"), None);
    assert_eq!(rules.country_side_index("beta"), None);
    assert_eq!(rules.country_side_index("Alpha"), Some(SideIdx(0)));
    assert_eq!(processed.country_color_scheme("Beta"), None);
    assert_eq!(
        processed
            .native_type_construction_trace()
            .registry_state()
            .side_country_members("GDI"),
        Some([0].as_slice()),
        "ReadHouses4767C0 skips unknown Beta; its orphan body is never read",
    );
}

#[test]
fn ordered_country_side_failed_binding_keeps_the_live_vector_until_reset() {
    use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};

    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        IniFile::from_str("[Countries]\n0=Alpha\n"),
        None,
        IniFile::empty(),
        std::sync::Arc::default(),
        Default::default(),
    )
    .expect("cold Rules owner");
    let broken = IniFile::from_str("[Countries]\n0=Alpha\n[Sides]\nBroken=<random>\n");
    let empty_map = IniFile::empty();
    let error = owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(Some(&broken)),
            &empty_map,
        )
        .err()
        .expect("native Country[-2] binding is outside admitted Rules sources");
    assert!(matches!(
        error,
        RulesError::InvalidValue { section, key, value, .. }
            if section == "Sides" && key == "Broken" && value == "-2"
    ));

    // This next real owner transition resolves the retained Side, then
    // expands its actual signed vector. Losing the failed pass's registry
    // or replacing its vector with empty would incorrectly succeed here.
    let probe = IniFile::from_str("[Sides]\nProbe=Broken\n");
    let error = owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(Some(&probe)),
            &empty_map,
        )
        .err()
        .expect("a failed Process retains its actual Side vector");
    assert!(matches!(
        error,
        RulesError::InvalidValue { section, key, value, .. }
            if section == "Sides" && key == "Probe" && value == "-2"
    ));

    let repair = IniFile::from_str("[Sides]\nBroken=Alpha\n");
    let loaded = owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(Some(&repair)),
            &empty_map,
        )
        .expect("the corrected vector reaches the native destructive reset");
    let (rules, _, _, _) = loaded.into_parts();
    assert_eq!(rules.country_index("Alpha"), Some(CountryIdx(0)));
    assert_eq!(rules.country_side_index("Alpha"), None);
    assert_eq!(rules.side_index("Broken"), None);
    assert_eq!(rules.side_index("Probe"), None);
}

#[test]
fn test_load_ruleset() {
    let ini: IniFile = IniFile::from_str(&make_test_rules());
    let rules: RuleSet = RuleSet::from_ini(&ini).expect("Should parse");

    assert_eq!(rules.infantry_ids.len(), 2);
    assert_eq!(rules.vehicle_ids.len(), 1);
    assert_eq!(rules.aircraft_ids.len(), 0);
    assert_eq!(rules.building_ids.len(), 1);
    assert_eq!(rules.object_count(), 4); // E1, E2, MTNK, GAPOWR
    // ReadDouble scans a float and widens it; float fields store it back.
    let float = |value: f32| NativeF32Bits::from_bits(value.to_bits());
    assert_eq!(
        rules.production.build_speed,
        NativeF64Bits::from_bits(f64::from(0.75_f32).to_bits())
    );
    assert_eq!(rules.production.multiple_factory, float(0.7));
    assert_eq!(rules.production.low_power_penalty_modifier, float(1.25));
    assert_eq!(rules.production.min_low_power_production_speed, float(0.4));
    assert_eq!(rules.production.max_low_power_production_speed, float(0.85));
    assert_eq!(rules.bridge_rules.strength, 1000);
    assert!(rules.bridge_rules.destroyable_by_default);
}

/// `HouseClass @ 0x0050C0A0` takes the country's build-time multiplier by
/// WhatAmI: infantry `+0x134`, units `+0x138`, aircraft `+0x13C`, and a
/// building's `+0x140`, or `+0x144` when its BuildCat is 5, Combat
/// (`0x0050C0F0`). An absent key or an unknown country gives 1.0.
#[test]
fn country_build_time_mults_follow_the_native_slots() {
    let ini = IniFile::from_str(
        "[Countries]\n0=T\n1=U\n\
             [T]\nBuildTimeInfantryMult=0.5\nBuildTimeUnitsMult=0.75\n\
             BuildTimeAircraftMult=1.25\nBuildTimeBuildingsMult=1.5\n\
             BuildTimeDefensesMult=2.0\n\
             [U]\nBuildTimeUnitsMult=0.8\n\
             [InfantryTypes]\n0=I\n[VehicleTypes]\n0=V\n[AircraftTypes]\n0=A\n\
             [BuildingTypes]\n0=B\n1=D\n2=P\n\
             [I]\nStrength=1\n[V]\nStrength=1\n[A]\nStrength=1\n[B]\nStrength=1\n\
             [D]\nStrength=1\nBuildCat=Combat\n[P]\nStrength=1\nBuildCat=Power\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("country fixture parses");
    let float = |value: f32| NativeF32Bits::from_bits(value.to_bits());
    let mult = |country: &str, object: &str| {
        rules.country_build_time_mult_for_type(country, rules.object(object).unwrap())
    };
    for (object, expected) in [
        ("I", 0.5),
        ("V", 0.75),
        ("A", 1.25),
        ("B", 1.5),
        ("D", 2.0),
        ("P", 1.5),
    ] {
        assert_eq!(mult("T", object), float(expected), "{object}");
    }
    assert_eq!(mult("U", "V"), float(0.8));
    assert_eq!(mult("U", "I"), NativeF32Bits::ONE);
    assert_eq!(mult("Nowhere", "V"), NativeF32Bits::ONE);
}
