//! Object type reader tests: retail and layered rules.ini reads of object
//! keys through `ObjectType::from_ini_section` and `RuleSet`.

#[test]
fn fly_bank_rules_read_stock_values_and_keep_each_pass_sentinels() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    for id in ["BPLN", "SPYP"] {
        let object = retail.rules.object(id).expect("stock aircraft type");
        assert_eq!(object.fly_pitch_speed(), 0.25, "{id} default PitchSpeed");
        assert_eq!(object.fly_pitch_angle_radians(), 0.0, "{id} PitchAngle");
        assert!(
            (object.fly_roll_angle_radians() - 30.0_f64.to_radians()).abs() < 1.0e-12,
            "{id} constructor RollAngle"
        );
    }

    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=BPLN\n[BPLN]\nPitchSpeed=0.5\nRollAngle=45\nPitchAngle=12\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[BPLN]\nRollAngle=-1\nPitchAngle=-1\n"),
    );
    let rules = RuleSet::from_rules_layers(&layers).expect("production layered rules");
    let object = rules.object("BPLN").expect("registered aircraft type");
    assert_eq!(object.fly_pitch_speed(), 0.5, "omitted later key retains");
    assert!(
        (object.fly_roll_angle_radians() - 45.0_f64.to_radians()).abs() < 1.0e-12,
        "later -1 retains the earlier angle"
    );
    assert!(
        (object.fly_pitch_angle_radians() - 12.0_f32.to_radians()).abs() < 1.0e-5,
        "later PitchAngle=-1 retains the earlier angle"
    );
}

/// `RollAngle=1e999` reads as infinity: the stored binary32 angle is zero,
/// not a panic in the fire or draw code that asks for it.
#[test]
fn an_infinite_roll_angle_stores_as_zero() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::RulesLayerStack;
    use crate::rules::ruleset::RuleSet;
    let layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]
0=BPLN
[BPLN]
RollAngle=1e999
",
    ));
    let rules = RuleSet::from_rules_layers(&layers).expect("production layered rules");
    let object = rules.object("BPLN").expect("registered aircraft type");
    assert!(object.fly_roll_angle_radians().is_infinite());
    assert_eq!(object.fly_roll_angle_f32(), 0.0);
}

#[test]
fn half_damage_smoke_location_retains_stock_battle_building_values() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let authored = retail
        .authored_rules
        .section("GAREFN")
        .expect("stock Allied refinery section");
    assert_eq!(
        authored.read_coord3("HalfDamageSmokeLocation1", [9, 9, 9]),
        [0, 0, 0]
    );
    assert!(!authored.is_present("HalfDamageSmokeLocation2"));

    let building = retail
        .rules
        .object("GAREFN")
        .expect("stock Allied refinery BuildingType");
    assert_eq!(building.category, super::ObjectCategory::Building);
    assert_eq!(building.half_damage_smoke_location1, [0, 0, 0]);
    assert_eq!(building.half_damage_smoke_location2, [0, 0, 0]);
}

#[test]
fn half_damage_smoke_location_retains_native_current_values_across_rules_layers() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;

    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[BuildingTypes]\n0=HUT\n[VehicleTypes]\n0=HUTVEH\n[HUT]\nHalfDamageSmokeLocation1=11,22,33\n\
         HalfDamageSmokeLocation2=-4,5,6\n[HUTVEH]\n\
         HalfDamageSmokeLocation1=91,92,93\nHalfDamageSmokeLocation2=-94,95,96\n",
    ));
    layers.push(
        RulesLayerKind::GameMode,
        IniFile::from_str("[HUT]\nHalfDamageSmokeLocation1=-7,8,9\n"),
    );
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[HUT]\nStrength=100\n"),
    );

    let rules = RuleSet::from_rules_layers(&layers).expect("production layered rules");
    let building = rules.object("HUT").expect("registered BuildingType");
    assert_eq!(building.half_damage_smoke_location1, [-7, 8, 9]);
    assert_eq!(building.half_damage_smoke_location2, [-4, 5, 6]);
    let vehicle = rules.object("HUTVEH").expect("registered VehicleType");
    assert_eq!(vehicle.category, super::ObjectCategory::Vehicle);
    assert_eq!(vehicle.half_damage_smoke_location1, [0; 3]);
    assert_eq!(vehicle.half_damage_smoke_location2, [0; 3]);
}

#[test]
fn invisible_in_game_forces_building_flags_and_retains_them_across_layers() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;

    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[BuildingTypes]\n0=HIDDEN\n[VehicleTypes]\n0=VISIBLE\n\
         [HIDDEN]\nInvisible=no\nInvisibleInGame=yes\nRadarVisible=yes\n\
         [VISIBLE]\nInvisible=no\nInvisibleInGame=yes\nRadarVisible=yes\n",
    ));
    let flags = |layers: &RulesLayerStack, name: &str| {
        let rules = RuleSet::from_rules_layers(layers).unwrap();
        let object = rules.object(name).unwrap();
        (
            object.invisible_in_game,
            object.invisible,
            object.radar_visible,
        )
    };
    assert_eq!(flags(&layers, "HIDDEN"), (true, true, false));
    assert_eq!(flags(&layers, "VISIBLE"), (true, false, true));

    // Clearing the building-only key does not undo earlier derived writes.
    // Invalid base-reader values retain those writes as their defaults.
    layers.push(
        RulesLayerKind::GameMode,
        IniFile::from_str(
            "[HIDDEN]\nInvisibleInGame=no\nInvisible=invalid\nRadarVisible=invalid\n",
        ),
    );
    assert_eq!(flags(&layers, "HIDDEN"), (false, true, false));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[HIDDEN]\nInvisible=no\nRadarVisible=yes\n"),
    );
    assert_eq!(flags(&layers, "HIDDEN"), (false, false, true));

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let lamp = retail.rules.object("INGALITE").unwrap();
    assert!(lamp.invisible_in_game && lamp.invisible);
    assert!(!lamp.radar_visible);
}

/// `RevealToAll=` through the production reader on the retail layers.
#[test]
#[ignore = "slow; run when changing what it covers"]
fn reveal_to_all_reads_the_seven_stock_types() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let mut types: Vec<_> = retail
        .rules
        .all_objects()
        .filter(|object| object.reveal_to_all)
        .map(|object| object.id.as_str())
        .collect();
    types.sort_unstable();
    assert_eq!(
        types,
        [
            "GACSPH", "GADUMY", "GAWEAT", "NAIRON", "NAMISL", "YAGNTC", "YAPPET"
        ]
    );
}

#[test]
fn combatant_type_default_and_retained_layer_reads_match_original_execution() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;
    use std::fmt::Write;

    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/input_oracle/selection_navigation.json",
    ))
    .unwrap();
    let family = &native["combatant_type_histories"];
    assert_eq!(family["key"], "IsSelectableCombatant");
    assert_eq!(family["field_offset"], 0xDBC);
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=AUTHORED\n[AUTHORED]\nStrength=100\n",
    ));
    assert!(
        !RuleSet::from_rules_layers(&layers)
            .unwrap()
            .object("AUTHORED")
            .unwrap()
            .is_selectable_combatant
    );
    for constructor in family["constructors"].as_array().unwrap() {
        assert_eq!(constructor["value_before"], 0xA5);
        assert_eq!(
            constructor["value_after"], false,
            "original {} constructor",
            constructor["type_id"]
        );
    }
    let mut previous = false;
    for row in family["authored_history"].as_array().unwrap() {
        assert_eq!(row["before"].as_bool().unwrap(), previous);
        let mut text = String::new();
        for (section, values) in row["sections"].as_object().unwrap() {
            writeln!(text, "[{section}]").unwrap();
            for (key, value) in values.as_object().unwrap() {
                writeln!(text, "{key}={}", value.as_str().unwrap()).unwrap();
            }
        }
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(&text));
        let rules = RuleSet::from_rules_layers(&layers).expect("native retained-reader history");
        previous = rules.object("AUTHORED").unwrap().is_selectable_combatant;
        assert_eq!(
            previous,
            row["after"].as_bool().unwrap(),
            "original {}",
            row["id"]
        );
        for read in row["reads"].as_array().unwrap() {
            assert_eq!(read["key"], "IsSelectableCombatant");
            assert_eq!(read["reader"], "0x5295f0");
        }
    }
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn combatant_retail_rules_mode_map_reads_match_original_execution() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/input_oracle/selection_navigation.json",
    ))
    .unwrap();
    let rows = native["combatant_type_histories"]["retail_histories"]
        .as_array()
        .unwrap();
    assert_eq!(rows.len(), 8);
    for row in rows {
        let name = row["type_id"].as_str().unwrap();
        assert_eq!(
            retail.rules.object(name).unwrap().is_selectable_combatant,
            row["final"].as_bool().unwrap(),
            "{name}: physical retail startup -> Battle -> XMP03T4 layers",
        );
    }
}

#[test]
fn flight_level_reader_and_fallback_match_original_executable() {
    #[derive(serde::Deserialize)]
    struct Row {
        raw: Option<String>,
        general: i32,
        stored: i32,
        effective: i32,
    }
    let rows: Vec<Row> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/flight_level.json",
    ))
    .unwrap();
    assert_eq!(rows.len(), 30);
    for row in rows {
        let mut ini = IniFile::from_str("[PLANE]\nStrength=100\n");
        if let Some(raw) = &row.raw {
            ini.projection_section_mut("PLANE").set("FlightLevel", raw);
        }
        let obj = ObjectType::from_ini_section(
            "PLANE",
            ini.section("PLANE").unwrap(),
            ObjectCategory::Aircraft,
        );
        assert_eq!(obj.flight_level, row.stored, "{:?}", row.raw);
        assert_eq!(
            obj.flight_level(row.general),
            row.effective,
            "{:?}",
            row.raw
        );
    }
}

#[test]
fn signed_dock_count_is_distinct_from_contact_capacity() {
    #[derive(serde::Deserialize)]
    struct Row {
        initial: i32,
        raw: Option<String>,
        count: i32,
        capacity: u32,
    }
    // Executed original ReadInteger call site and separate contact clamp.
    // Non-constructor defaults test the shared reader, not Rules pass lifetime.
    let rows: Vec<Row> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_dock_count_rules.json",
    ))
    .unwrap();
    for row in rows {
        // The native corpus supplies cached INI entries directly. Physical
        // INI loading drops empty values; preserve the reader boundary here.
        let mut ini = IniFile::from_str("[PAD]\nStrength=100\n");
        if let Some(raw) = &row.raw {
            ini.projection_section_mut("PAD").set("NumberOfDocks", raw);
        }
        let section = ini.section("PAD").unwrap();
        assert_eq!(
            section.read_int("NumberOfDocks", row.initial),
            row.count,
            "initial={} raw={:?}",
            row.initial,
            row.raw
        );
        let mut object = ObjectType::from_ini_section("PAD", section, ObjectCategory::Building);
        if row.initial == 1 {
            assert_eq!(object.number_of_docks, row.count, "{:?}", row.raw);
        } else {
            object.number_of_docks = row.count;
        }
        // VERA bounds the native capacity (`DOCK_CONTACT_LIMIT`).
        assert_eq!(
            object.dock_contact_capacity(),
            row.capacity.min(DOCK_CONTACT_LIMIT as u32),
            "{:?}",
            row.raw
        );
    }
}

#[test]
fn pip_scale_native_names_default_capacity_and_layer_retention() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;
    // Reader474940's file-backed table81B9B0, through ReadString32.
    for (raw, expected) in [
        ("Ammo", PipScale::Ammo),
        ("tIbErIuM", PipScale::Tiberium),
        ("Passengers", PipScale::Passengers),
        ("Power", PipScale::Power),
        ("mInDcOnTrOl", PipScale::MindControl),
        ("Unknown", PipScale::None),
        ("5", PipScale::None),
        ("MindControl,Ammo", PipScale::None),
    ] {
        let ini = IniFile::from_str(&format!("[MIND]\nPipScale={raw}\n"));
        let object = ObjectType::from_ini_section(
            "MIND",
            ini.section("MIND").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert_eq!(object.pip_scale, expected, "{raw}");
    }
    let ini = IniFile::from_str(&format!(
        "[MIND]\nPipScale=MindControl{}invalid\n",
        " ".repeat(20)
    ));
    assert_eq!(
        ObjectType::from_ini_section(
            "MIND",
            ini.section("MIND").unwrap(),
            ObjectCategory::Vehicle
        )
        .pip_scale,
        PipScale::MindControl,
        "copy truncates to31 bytes BEFORE trim/name lookup"
    );
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=MIND\n[MIND]\nPipScale=MindControl\n",
    ));
    for patch in ["Strength=500", "PipScale=", "PipScale=   ", "pipscale=Ammo"] {
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(&format!("[MIND]\n{patch}\n")),
        );
        assert_eq!(
            RuleSet::from_rules_layers(&layers)
                .unwrap()
                .object("MIND")
                .unwrap()
                .pip_scale,
            PipScale::MindControl,
            "absent/empty/exact-case key: {patch}"
        );
    }
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[MIND]\nPipScale=Unknown\n"),
    );
    assert_eq!(
        RuleSet::from_rules_layers(&layers)
            .unwrap()
            .object("MIND")
            .unwrap()
            .pip_scale,
        PipScale::None
    );
    let default_ini = IniFile::from_str("[MIND]\nStrength=500\n");
    assert_eq!(
        ObjectType::from_ini_section(
            "MIND",
            default_ini.section("MIND").unwrap(),
            ObjectCategory::Vehicle
        )
        .pip_scale,
        PipScale::None
    );
}

#[test]
fn retail_mastermind_uses_mind_control_pips_and_current_primary_capacity() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let object = fixture.rules.object("MIND").unwrap();
    assert_eq!(object.pip_scale, PipScale::MindControl);
    assert_eq!(
        fixture
            .rules
            .weapon(object.weapon_at(0).unwrap())
            .unwrap()
            .damage,
        3
    );
}

#[test]
fn vhp_scan_parser_preserves_native_names_token_and_default() {
    // Unknown first tokens keep the constructor's None.
    for (raw, expected) in [
        ("Strong", VhpScan::Strong),
        ("nOrMaL", VhpScan::Normal),
        ("Strong,Normal", VhpScan::Strong),
        (",,Normal", VhpScan::Normal),
        (" Normal ", VhpScan::Normal),
        ("Normal ,Strong", VhpScan::None),
        ("Weak", VhpScan::None),
        ("2", VhpScan::None),
    ] {
        let mut section = IniSection::new("TYPE".to_string());
        section.set("VHPScan", raw);
        assert_eq!(VhpScan::read_ini(&section), expected, "{raw}");
    }
    let ini = IniFile::from_str("[TYPE]\nStrength=100\n");
    assert_eq!(
        ObjectType::from_ini_section(
            "TYPE",
            ini.section("TYPE").unwrap(),
            ObjectCategory::Vehicle
        )
        .vhp_scan,
        VhpScan::None,
    );
}

#[test]
fn vhp_scan_layers_retain_prior_mode_and_ignore_preallocation_body() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=EXISTING\n[EXISTING]\nVHPScan=Strong\n[LATE]\nVHPScan=Strong\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str(
            "[VehicleTypes]\n0=LATE\n[EXISTING]\nVHPScan=Invalid\n[LATE]\nVHPScan=Invalid\n",
        ),
    );
    let rules = RuleSet::from_rules_layers(&layers).unwrap();
    assert_eq!(rules.object("EXISTING").unwrap().vhp_scan, VhpScan::Strong);
    assert_eq!(rules.object("LATE").unwrap().vhp_scan, VhpScan::None);
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[EXISTING]\nVHPScan=None\n[LATE]\nVHPScan=Normal,Strong\n"),
    );
    let rules = RuleSet::from_rules_layers(&layers).unwrap();
    assert_eq!(rules.object("EXISTING").unwrap().vhp_scan, VhpScan::None);
    assert_eq!(rules.object("LATE").unwrap().vhp_scan, VhpScan::Normal);
}

use super::*;
use crate::rules::ini_parser::IniFile;

#[test]
fn test_parse_vehicle() {
    // TechLevel and the weapon slots come from the ordered Rules owner.
    let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n\
         [MTNK]\nName=Grizzly Battle Tank\nCost=700\nStrength=300\n\
         Armor=heavy\nSpeed=6\nSight=6\nTechLevel=2\n\
         AccelerationFactor=.03\nDeaccelerationFactor=.002\n\
         Owner=Americans,Alliance\nRequiredHouses=Americans\n\
         Prerequisite=GAWEAP\nPrimary=105mm\n",
    ))
    .unwrap();
    let obj = rules.object("MTNK").unwrap();

    assert_eq!(obj.id, "MTNK");
    assert_eq!(obj.category, ObjectCategory::Vehicle);
    assert_eq!(obj.name, Some("Grizzly Battle Tank".to_string()));
    assert_eq!(obj.cost, 700);
    assert_eq!(obj.strength, 300);
    assert_eq!(obj.armor, "heavy");
    assert_eq!(obj.speed, 6);
    assert_eq!(obj.accel_factor.bits(), 0x3f9e_b851_e000_0000);
    assert_eq!(obj.decel_factor.bits(), 0x3f60_624d_e000_0000);
    assert_eq!(obj.tech_level, 2);
    assert_eq!(
        obj.build_time_multiplier,
        crate::util::native_x87::NativeF32Bits::ONE
    );
    assert_eq!(obj.owner, vec!["Americans", "Alliance"]);
    assert_eq!(obj.required_houses, vec!["Americans"]);
    assert!(obj.allowed_to_start_in_multiplayer);
    assert_eq!(obj.primary(), Some("105mm"));
    assert_eq!(obj.secondary(), None);
    assert_eq!(obj.image, "MTNK"); // Defaults to ID when Image= absent.
    assert_eq!(obj.build_cat, None);
    assert_eq!(obj.adjacent, 3);
    assert!(obj.base_normal);
    assert!(!obj.eligibile_for_ally_building);
    assert!(!obj.crewed);
}

#[test]
fn parses_miner_order_voices() {
    // Stock [CMIN] harvest ack: VoiceHarvest round-trips into the parsed
    // type so the order-ack dispatch can play it instead of the generic
    // move voice.
    let ini: IniFile = IniFile::from_str(
        "[CMIN]\nName=Chrono Miner\nVoiceMove=ChronoMinerMove\n\
         VoiceHarvest=ChronoMinerHarvest\nVoiceEnter=ChronoMinerReturn\n",
    );
    let mut obj = ObjectType::from_ini_section(
        "CMIN",
        ini.section("CMIN").unwrap(),
        ObjectCategory::Vehicle,
    );
    let sounds = SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n0=ChronoMinerHarvest\n1=ChronoMinerMove\n2=ChronoMinerReturn\n",
    ));
    obj.bind_deploy_voice_references(ini.section("CMIN"), &sounds);
    obj.bind_order_voice_references(ini.section("CMIN"), &sounds);
    assert_eq!(obj.voice_move(), ["ChronoMinerMove"]);
    assert_eq!(obj.voice_harvest(), Some("ChronoMinerHarvest"));
    assert_eq!(obj.voice_enter(), Some("ChronoMinerReturn"));

    // Absent keys default to None (e.g. a plain tank).
    let none_ini: IniFile = IniFile::from_str("[MTNK]\nName=Grizzly\n");
    let none_obj = ObjectType::from_ini_section(
        "MTNK",
        none_ini.section("MTNK").unwrap(),
        ObjectCategory::Vehicle,
    );
    assert_eq!(none_obj.voice_harvest(), None);
    assert_eq!(none_obj.voice_enter(), None);
}

/// Capture708DC0 consumes the fixed type slot before its Enter fallback.
/// Stock engineers register this cue in SOUNDMD.
#[test]
fn gsi_06_parses_voice_capture_slot() {
    let ini: IniFile = IniFile::from_str(
        "[ENGINEER]\nName=Engineer\nVoiceMove=EngAllMove\n\
         VoiceEnter=EngAllMove\nVoiceCapture=EngAllAttackCommand\n",
    );
    let mut obj = ObjectType::from_ini_section(
        "ENGINEER",
        ini.section("ENGINEER").unwrap(),
        ObjectCategory::Infantry,
    );
    let sounds = SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n0=EngAllMove\n1=EngAllAttackCommand\n",
    ));
    obj.bind_order_voice_references(ini.section("ENGINEER"), &sounds);
    assert_eq!(obj.voice_capture(), Some("EngAllAttackCommand"));

    // Absent key stays None, which is the retail "fall back to Enter" case.
    let none_ini: IniFile = IniFile::from_str("[E1]\nName=GI\n");
    let none_obj = ObjectType::from_ini_section(
        "E1",
        none_ini.section("E1").unwrap(),
        ObjectCategory::Infantry,
    );
    assert_eq!(none_obj.voice_capture(), None);
}

#[test]
fn test_parse_building() {
    let ini: IniFile = IniFile::from_str(
        "[GAPOWR]\nName=Power Plant\nCost=800\nStrength=750\n\
         Power=200\nFoundation=2x2\nArmor=wood\nBuildCat=Power\nCrewed=yes\n",
    );
    let section: &IniSection = ini.section("GAPOWR").unwrap();
    let obj: ObjectType = ObjectType::from_ini_section("GAPOWR", section, ObjectCategory::Building);

    assert_eq!(obj.power, 200);
    assert_eq!(obj.foundation, "1x1", "RULES Foundation is not an input");
    assert_eq!(obj.armor, "wood");
    assert_eq!(obj.build_cat, Some(BuildCategory::Power));
    assert_eq!(obj.adjacent, 3);
    assert!(obj.base_normal);
    assert!(!obj.eligibile_for_ally_building);
    assert!(obj.crewed);
}

#[test]
fn firestorm_wall_matches_original_native_reader() {
    #[derive(serde::Deserialize)]
    struct Row {
        raw: Option<String>,
        output: bool,
    }
    #[derive(serde::Deserialize)]
    struct Fixture {
        firestorm_wall: Vec<Row>,
    }
    let fixture: Fixture = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_body_rules.json",
    ))
    .unwrap();
    for row in fixture.firestorm_wall {
        let mut section = IniSection::new("TEST".to_string());
        if let Some(raw) = &row.raw {
            section.set("FirestormWall", raw);
        }
        let object = ObjectType::from_ini_section("TEST", &section, ObjectCategory::Building);
        assert_eq!(object.firestorm_wall, row.output, "{:?}", row.raw);
    }
}

#[test]
fn test_defaults_for_missing_keys() {
    let ini: IniFile = IniFile::from_str("[BARE]\nFixtureOnly=1\n");
    let section: &IniSection = ini.section("BARE").unwrap();
    let obj: ObjectType = ObjectType::from_ini_section("BARE", section, ObjectCategory::Infantry);

    assert_eq!(obj.cost, 0);
    assert_eq!(obj.strength, 0);
    assert_eq!(obj.armor, "none");
    assert_eq!(obj.speed, 0);
    assert_eq!(obj.accel_factor.bits(), 0x3f9e_b851_eb85_1eb8);
    assert_eq!(obj.decel_factor.bits(), 0x3f60_624d_d2f1_a9fc);
    assert_eq!(obj.walk_rate, 1);
    assert_eq!(obj.idle_rate, 0);
    assert_eq!(obj.sight, 0);
    assert_eq!(obj.tech_level, 255);
    assert_eq!(
        obj.build_time_multiplier,
        crate::util::native_x87::NativeF32Bits::ONE
    );
    assert!(obj.owner.is_empty());
    assert!(obj.required_houses.is_empty());
    assert_eq!(obj.primary(), None);
    assert_eq!(obj.image, "BARE");
    assert_eq!(obj.power, 0);
    assert_eq!(obj.foundation, "1x1");
    assert_eq!(obj.build_cat, None);
    assert_eq!(obj.adjacent, 3);
    assert!(obj.base_normal);
    assert!(!obj.eligibile_for_ally_building);
    assert!(!obj.crewed);
}

#[test]
fn considered_aircraft_uses_native_category_defaults_and_ini_overrides() {
    let ini = IniFile::from_str(
        "[VEH_DEFAULT]\nFixtureOnly=1\n\
         [AIR_DEFAULT]\nFixtureOnly=1\n\
         [VEH_EXPLICIT]\nConsideredAircraft=yes\n\
         [AIR_EXPLICIT]\nConsideredAircraft=no\n",
    );

    let parse = |id, category| ObjectType::from_ini_section(id, ini.section(id).unwrap(), category);

    assert!(!parse("VEH_DEFAULT", ObjectCategory::Vehicle).considered_aircraft);
    assert!(parse("AIR_DEFAULT", ObjectCategory::Aircraft).considered_aircraft);
    assert!(parse("VEH_EXPLICIT", ObjectCategory::Vehicle).considered_aircraft);
    assert!(!parse("AIR_EXPLICIT", ObjectCategory::Aircraft).considered_aircraft);
}

#[test]
fn test_parse_undeploys_into() {
    let rules = rules(
        "[BuildingTypes]\n0=GACNST\n1=MODDEPLOY\n\
         [GACNST]\nUndeploysInto=AMCV\nFoundation=4x4\n\
         [MODDEPLOY]\nUndeploysInto=MODUNIT\nFoundation=1x1\n",
    );
    let mut obj = rules.object("GACNST").unwrap().clone();
    assert_eq!(obj.undeploys_into, Some("AMCV".to_string()));
    assert_eq!(obj.deploys_into, None);
    assert!(
        obj.is_1x1_with_undeploy(),
        "constructor remains1x1 until ART reads"
    );
    // This gate consumes a loaded native field; ART input provenance and
    // every table dimension are tested by building_foundation_pass_matches_original_art_reads.
    obj.foundation = "4x4".into();
    assert!(!obj.is_1x1_with_undeploy());

    assert!(rules.object("MODDEPLOY").unwrap().is_1x1_with_undeploy());
}

/// Phobos `UnitTypeExt`'s three `OreGathering.*` vectors through the
/// production layered reader: an absent key retains the whole list, a
/// present one replaces it, and an `Anims=` token the AnimType array lacks
/// drops out instead of holding a null slot.
#[test]
#[cfg(feature = "phobos")]
fn ore_gathering_lists_replace_retain_and_drop_unknown_anims() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;

    let base = IniFile::from_str(
        "[VehicleTypes]\n0=HARV\n[Animations]\n0=GATHA\n\
         [HARV]\nHarvester=yes\nOreGathering.Anims=GATHA,NOPE,GATHB\n\
         OreGathering.Tiberiums=0,1\nOreGathering.FramesPerDir=10,20\n",
    );
    let rules = RuleSet::from_rules_layers(&RulesLayerStack::new(base))
        .expect("authored phobos gather lists");
    let gathering = rules.object("HARV").expect("HARV").ore_gathering();
    assert_eq!(gathering.anims, ["GATHA"], "the unknown token drops out");
    assert_eq!(gathering.tiberiums, [0, 1]);
    assert_eq!(gathering.frames_per_dir, [10, 20]);

    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=HARV\n[Animations]\n0=GATHA\n\
         [HARV]\nHarvester=yes\nOreGathering.Anims=GATHA\n\
         OreGathering.Tiberiums=0,1\nOreGathering.FramesPerDir=10,20\n",
    ));
    layers.push(
        RulesLayerKind::GameMode,
        IniFile::from_str("[HARV]\nOreGathering.Tiberiums=2\n"),
    );
    let rules = RuleSet::from_rules_layers(&layers).expect("layered phobos gather lists");
    let gathering = rules.object("HARV").expect("HARV").ore_gathering();
    assert_eq!(gathering.tiberiums, [2], "the named list replaces");
    assert_eq!(gathering.frames_per_dir, [10, 20], "the others retain");

    let stock = RuleSet::from_rules_layers(&RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=HARV\n[HARV]\nHarvester=yes\n",
    )))
    .expect("stock harvester rules");
    assert_eq!(
        stock.object("HARV").expect("HARV").ore_gathering(),
        &crate::rules::object_type::PhobosOreGathering::default(),
        "a type authoring none keeps every empty list"
    );
}

#[test]
fn parse_construction_yard_and_deploy_facing() {
    let ini: IniFile = IniFile::from_str("[GACNST]\nConstructionYard=yes\nDeployFacing=2\n");
    let section: &IniSection = ini.section("GACNST").unwrap();
    let obj = ObjectType::from_ini_section("GACNST", section, ObjectCategory::Building);
    assert!(obj.construction_yard);
    assert_eq!(obj.deploy_facing, 0x40);

    let default_ini = IniFile::from_str("[GAPOWR]\nFixtureOnly=1\n");
    let default_obj = ObjectType::from_ini_section(
        "GAPOWR",
        default_ini.section("GAPOWR").unwrap(),
        ObjectCategory::Building,
    );
    assert!(!default_obj.construction_yard);
    assert_eq!(default_obj.deploy_facing, 0x80);
}

/// The production reader: the ordered Rules processor reads the Type
/// references, `RuleSet` publishes them.
fn rules(text: &str) -> crate::rules::ruleset::RuleSet {
    crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(text)).unwrap()
}

/// `Dock=` tokens resolve to the stored IDs of their BuildingTypes, found
/// case-insensitively or allocated.
#[test]
fn test_parse_refinery_free_unit_and_dock() {
    let rules = rules(
        "[BuildingTypes]\n0=MODPROC\n[VehicleTypes]\n0=MODHARV\n\
         [MODPROC]\nRefinery=yes\nDockUnload=yes\nFreeUnit=MODHARV\n\
         [MODHARV]\nHarvester=yes\nDock=modproc,NAREFN\n",
    );

    let refinery = rules.object("MODPROC").unwrap();
    assert!(refinery.refinery);
    assert_eq!(refinery.free_unit, Some("MODHARV".to_string()));

    let harvester = rules.object("MODHARV").unwrap();
    assert!(harvester.harvester);
    // Ares reads Dock= Find-only (TechnoTypeClass_LoadFromINI_ListLength
    // 1005BF90, hook 0x7125DF): the unregistered NAREFN is dropped.
    let dock: &[&str] = if crate::rules::ares_switch::ares_way() {
        &["MODPROC"]
    } else {
        &["MODPROC", "NAREFN"]
    };
    assert_eq!(harvester.dock, dock);
}

#[test]
fn stock_cmin_rot_remains_5_after_harvester_parse() {
    let ini: IniFile = IniFile::from_str("[CMIN]\nHarvester=yes\nROT=5\n");
    let section: &IniSection = ini.section("CMIN").unwrap();
    let obj: ObjectType = ObjectType::from_ini_section("CMIN", section, ObjectCategory::Vehicle);

    assert!(obj.harvester);
    assert_eq!(obj.turret_rot, 5);
}

#[test]
fn rally_line_accepts_infantry_vehicle_factories_and_repair() {
    let ini = IniFile::from_str(
        "[GAPILE]\nFactory=InfantryType\n\
         [GAWEAP]\nFactory=UnitType\n\
         [GADEPT]\nUnitRepair=yes\n",
    );
    let barracks = ObjectType::from_ini_section(
        "GAPILE",
        ini.section("GAPILE").unwrap(),
        ObjectCategory::Building,
    );
    let factory = ObjectType::from_ini_section(
        "GAWEAP",
        ini.section("GAWEAP").unwrap(),
        ObjectCategory::Building,
    );
    let depot = ObjectType::from_ini_section(
        "GADEPT",
        ini.section("GADEPT").unwrap(),
        ObjectCategory::Building,
    );
    assert!(barracks.has_rally_line());
    assert!(factory.has_rally_line());
    assert!(depot.has_rally_line());
}

#[test]
fn barracks_flags_keep_native_defaults_parser_and_class_scope() {
    // Original ctor45E1C2/45E1C8/45E1CE and ReadBool5295F0:
    // missing/unrecognized -> current byte; only the first byte decides.
    for (raw, expected) in [
        ("", false),
        ("yes", true),
        ("Tjunk", true),
        ("1junk", true),
        ("no", false),
        ("false", false),
        ("0junk", false),
        ("on", false),
        ("off", false),
    ] {
        let ini = IniFile::from_str(&format!(
            "[TYPE]\nFactory=InfantryType\nGDIBarracks={raw}\nNODBarracks={raw}\nYuriBarracks={raw}\n"
        ));
        let section = ini.section("TYPE").unwrap();
        for category in [
            ObjectCategory::Building,
            ObjectCategory::Infantry,
            ObjectCategory::Vehicle,
            ObjectCategory::Aircraft,
        ] {
            let object = ObjectType::from_ini_section("TYPE", section, category);
            let native = category == ObjectCategory::Building && expected;
            assert_eq!(
                (
                    object.gdi_barracks(),
                    object.nod_barracks(),
                    object.yuri_barracks()
                ),
                (native, native, native),
                "{category:?} {raw:?}"
            );
        }
    }
    let ini = IniFile::from_str(
        "[TYPE]\nFactory=InfantryType\ngdibarracks=yes\nNodBarracks=yes\nYURIBARRACKS=yes\n",
    );
    let object = ObjectType::from_ini_section(
        "TYPE",
        ini.section("TYPE").unwrap(),
        ObjectCategory::Building,
    );
    assert_eq!(
        (
            object.gdi_barracks(),
            object.nod_barracks(),
            object.yuri_barracks()
        ),
        (false, false, false)
    );
}

#[test]
fn armory_flag_retains_native_defaults_across_reached_rules_layers() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;
    //45E157 default0;460B03/460B08 use the shared ReadBool5295F0
    //reader before the three barracks keys. Native ordinary GAPILE reads0.
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[BuildingTypes]\n0=BRKS\n[BRKS]\nArmory=yes\n",
    ));
    assert!(
        RuleSet::from_rules_layers(&layers)
            .unwrap()
            .object("BRKS")
            .unwrap()
            .armory()
    );
    layers.push(
        RulesLayerKind::LangRule,
        IniFile::from_str("[BRKS]\nArmory=malformed\n"),
    );
    assert!(
        RuleSet::from_rules_layers(&layers)
            .unwrap()
            .object("BRKS")
            .unwrap()
            .armory()
    );
    layers.push(
        RulesLayerKind::GameMode,
        IniFile::from_str("[BRKS]\nArmory=n\n"),
    );
    assert!(
        !RuleSet::from_rules_layers(&layers)
            .unwrap()
            .object("BRKS")
            .unwrap()
            .armory()
    );
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[BRKS]\narmory=yes\n"),
    );
    assert!(
        !RuleSet::from_rules_layers(&layers)
            .unwrap()
            .object("BRKS")
            .unwrap()
            .armory()
    );
    let ini = IniFile::from_str("[TYPE]\nArmory=yes\n");
    assert!(
        !ObjectType::from_ini_section(
            "TYPE",
            ini.section("TYPE").unwrap(),
            ObjectCategory::Infantry
        )
        .armory()
    );
    let empty = IniFile::from_str("[TYPE]\n");
    assert!(
        !ObjectType::from_ini_section(
            "TYPE",
            empty.section_or_empty("TYPE"),
            ObjectCategory::Building
        )
        .armory()
    );
}

#[test]
fn barracks_flags_keep_live_defaults_across_rules_layers() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[BuildingTypes]\n0=BRKS\n[BRKS]\nFactory=InfantryType\nGDIBarracks=yes\nNODBarracks=yes\n",
    ));
    for (kind, patch, expected) in [
        (
            RulesLayerKind::LangRule,
            "[BRKS]\nName=Fixture\nNODBarracks=malformed\n",
            (true, true, false),
        ),
        (
            RulesLayerKind::GameMode,
            "[BRKS]\nGDIBarracks=no\nYuriBarracks=yes\n",
            (false, true, true),
        ),
        (
            RulesLayerKind::Scenario,
            "[BRKS]\nGDIBarracks=t\nNODBarracks=off\nYuriBarracks=0\n",
            (true, true, false),
        ),
    ] {
        layers.push(kind, IniFile::from_str(patch));
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        let object = rules.object("BRKS").unwrap();
        assert_eq!(
            (
                object.gdi_barracks(),
                object.nod_barracks(),
                object.yuri_barracks()
            ),
            expected,
            "{kind:?}"
        );
    }
}

#[test]
fn retail_barracks_flags_match_whole_original_gapile_reader() {
    // Whole original BuildingType ctor/reader for the stock GAPILE and
    // GAPOWR in the sealed native input contract: all defaults false,
    // GAPILE GDIBarracks=yes; later selected mode/map have no override.
    let Some(ini) = retail_rules_ini() else {
        return;
    };
    let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).unwrap();
    for (id, expected) in [
        ("GAPILE", (true, false, false)),
        ("GAPOWR", (false, false, false)),
    ] {
        let object = rules.object(id).unwrap();
        assert_eq!(
            (
                object.gdi_barracks(),
                object.nod_barracks(),
                object.yuri_barracks()
            ),
            expected,
            "[{id}]"
        );
    }
}

/// `WeaponTypeClass::FindOrAllocate @ 0x00772FA0` answers null for `none`
/// and `<none>`, on both weapon branches.
#[test]
fn a_none_weapon_name_leaves_its_slot_empty() {
    let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=CMIN\n1=GUNNER\n\
         [CMIN]\nPrimary=none\nSecondary=<none>\nElitePrimary=NONE\n\
         [GUNNER]\nTurretCount=1\nWeaponCount=2\nWeapon1=none\nWeapon2=GUN\n",
    ))
    .unwrap();
    let miner = rules.object("CMIN").unwrap();
    assert!(miner.weapon_list.iter().all(Option::is_none));
    assert!(miner.elite_weapon_list.iter().all(Option::is_none));
    let gunner = rules.object("GUNNER").unwrap();
    assert_eq!(gunner.weapon_list[0], None);
    assert_eq!(gunner.weapon_list[1].as_deref(), Some("GUN"));
}

/// `Primary=` and `Weapon1=` are the same storage in gamemd, so whichever
/// ReadINI branch a type takes must land in the same fields.
///
/// gamemd-derived: `TechnoTypeClass::ReadINI`. The `Weapon%d` cursor starts
/// at `0x007128D6 LEA EDI,[EBP+0xA94]` and stores the base result at
/// `0x0071294A MOV [EDI-0x1FC],EAX`; on iteration 1 that address is
/// `0xA94 - 0x1FC` = `+0x898`, which is exactly where the `Primary=` block
/// stores at `0x007129F2`. `Secondary`/slot 1 is `+0x8B4` (`0x00712A17`),
/// elite slot 0 `+0xA94` (`0x00712A64`), elite slot 1 `+0xAB0`
/// (`0x00712A89`), stride `0x1C` (`0x00712992`).
#[test]
fn gsi_08_02_weapon_one_and_primary_share_one_field() {
    let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
        // Turret branch: Primary=/Secondary= are ignored, but the fields
        // they name are filled from Weapon1=/Weapon2=.
        "[VehicleTypes]\n0=SREF\n1=YAGGUN\n2=MTNK\n3=WIPED\n4=NOSLOTS\n\
         [SREF]\nTurretCount=4\nWeaponCount=1\nWeapon1=Comet\nEliteWeapon1=SuperComet\n\
         Weapon2=Ignored\n\
         [YAGGUN]\nTurretCount=1\nWeaponCount=6\nWeapon1=AGGattling\nWeapon2=AAGattCann\n\
         EliteWeapon2=AAGattlingE\n\
         [MTNK]\nPrimary=105mm\nSecondary=MachGun\nWeapon1=NeverRead\n\
         [WIPED]\nClearAllWeapons=yes\nPrimary=105mm\nSecondary=MachGun\n\
         [NOSLOTS]\nTurretCount=2\nWeaponCount=0\nWeapon1=NeverRead\nPrimary=NeverRead\n",
    ))
    .unwrap();
    let obj = |id: &str| rules.object(id).unwrap();

    // Prism Tank: `Weapon1=` IS the `Primary` field. `WeaponCount=1` stops
    // the loop, so `Weapon2=` never reaches slot 1.
    let sref = obj("SREF");
    assert_eq!(sref.primary(), Some("Comet"));
    assert_eq!(sref.elite_primary(), Some("SuperComet"));
    assert_eq!(sref.secondary(), None);

    // Gattling Cannon: both slots filled, including the elite array.
    let yaggun = obj("YAGGUN");
    assert_eq!(yaggun.primary(), Some("AGGattling"));
    assert_eq!(yaggun.secondary(), Some("AAGattCann"));
    assert_eq!(yaggun.elite_secondary(), Some("AAGattlingE"));

    // No turret: the `Primary=` block runs and `Weapon1=` is never read.
    let mtnk = obj("MTNK");
    assert_eq!(mtnk.primary(), Some("105mm"));
    assert_eq!(mtnk.secondary(), Some("MachGun"));
    assert!(mtnk.weapon_list[2..].iter().all(Option::is_none));

    // `ClearAllWeapons=` skips the whole `Primary=` block (`0x007129A5`).
    let wiped = obj("WIPED");
    assert_eq!(wiped.primary(), None);
    assert_eq!(wiped.secondary(), None);

    // `WeaponCount <= 0` skips the loop AND the `Primary=` block
    // (`0x007128C8` jumps to `0x00712A8F`).
    let noslots = obj("NOSLOTS");
    assert_eq!(noslots.primary(), None);
    assert!(noslots.weapon_list.iter().all(Option::is_none));
}

#[test]
fn weapon_count_stops_at_the_vera_bound() {
    let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=GUN\n[GUN]\nTurretCount=1\nWeaponCount=5000\nWeapon1=AG1\n",
    ))
    .unwrap();
    let gun = rules.object("GUN").unwrap();
    assert_eq!(gun.weapon_count, WEAPON_COUNT_LIMIT);
    assert_eq!(gun.weapon_list[0].as_deref(), Some("AG1"));
}

#[test]
fn weapon_slot_lists_parse_for_non_gunner_types_and_selector_flags_parse() {
    // Stock YAGGUN shape: IsGattling with six WeaponN/EliteWeaponN slots
    // and no Gunner=. Stock DEST shape: NavalTargeting=1.
    let text = "[YAGGUN]\nIsGattling=yes\nTurretCount=1\nWeaponCount=6\nWeapon1=AG1\n\
         EliteWeapon1=AG1E\nWeapon2=AA1\nWeapon6=AA3\nEliteWeapon6=AA3E\n\
         [DEST]\nNavalTargeting=1\nNaval=yes\n\
         [BSUB]\nLandTargeting=2\nUnderwater=yes\nUnnatural=yes\n\
         [SQD]\nOrganic=yes\nNavalTargeting=3\n\
         [GAPOWR]\nDrainable=yes\n[TESLA]\nOverpowerable=true\n";
    let ini = IniFile::from_str(text);
    // The weapon block comes from the ordered Rules owner.
    let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(&format!(
        "[BuildingTypes]\n0=YAGGUN\n{text}"
    )))
    .unwrap();
    let yaggun = rules.object("YAGGUN").unwrap();
    assert!(yaggun.is_gattling);
    assert_eq!(yaggun.turret_count, 1);
    assert_eq!(yaggun.weapon_count, 6);
    assert_eq!(yaggun.weapon_list[0].as_deref(), Some("AG1"));
    assert_eq!(yaggun.elite_weapon_list[0].as_deref(), Some("AG1E"));
    assert_eq!(yaggun.weapon_list[1].as_deref(), Some("AA1"));
    assert!(yaggun.weapon_list[2].is_none());
    assert_eq!(yaggun.weapon_list[5].as_deref(), Some("AA3"));
    assert_eq!(yaggun.elite_weapon_list[5].as_deref(), Some("AA3E"));
    let dest = ObjectType::from_ini_section(
        "DEST",
        ini.section("DEST").unwrap(),
        ObjectCategory::Vehicle,
    );
    assert_eq!(dest.naval_targeting, 1);
    assert_eq!(dest.land_targeting, 0);
    assert!(!dest.underwater);
    let bsub = ObjectType::from_ini_section(
        "BSUB",
        ini.section("BSUB").unwrap(),
        ObjectCategory::Vehicle,
    );
    assert_eq!(bsub.land_targeting, 2);
    assert!(bsub.underwater);
    assert!(bsub.unnatural);
    let sqd =
        ObjectType::from_ini_section("SQD", ini.section("SQD").unwrap(), ObjectCategory::Vehicle);
    assert!(sqd.organic);
    assert_eq!(sqd.naval_targeting, 3);
    let powr = ObjectType::from_ini_section(
        "GAPOWR",
        ini.section("GAPOWR").unwrap(),
        ObjectCategory::Building,
    );
    assert!(powr.drainable);
    assert!(!powr.overpowerable);
    let tesla = ObjectType::from_ini_section(
        "TESLA",
        ini.section("TESLA").unwrap(),
        ObjectCategory::Building,
    );
    assert!(tesla.overpowerable);
}

#[test]
fn test_size_defaults_by_category() {
    let ini = IniFile::from_str(
        "[INF]\nFixtureOnly=1\n[VEH]\nFixtureOnly=1\n\
         [AIR]\nFixtureOnly=1\n[BUILDING]\nFixtureOnly=1\n",
    );
    for (id, category) in [
        ("INF", ObjectCategory::Infantry),
        ("VEH", ObjectCategory::Vehicle),
        ("AIR", ObjectCategory::Aircraft),
        ("BUILDING", ObjectCategory::Building),
    ] {
        let section = ini.section(id).expect("fixture section");
        let object = ObjectType::from_ini_section(id, section, category);
        assert_eq!(
            object.size, 1.0,
            "[{id}] missing Size= must retain the native constructor's 1.0",
        );
        assert_eq!(
            object.size_limit, 0.0,
            "[{id}] missing SizeLimit= must retain the native constructor's 0.0",
        );
    }
}

#[test]
fn retail_mtnk_and_e1_sizes_read_through_battle_layers() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };

    assert_eq!(retail.rules.object("MTNK").unwrap().size, 3.0);
    assert_eq!(retail.rules.object("E1").unwrap().size, 1.0);
}

/// `AbilityClass::FindAbilityByName @ 0x0074FEFF` resolves every one of
/// the 18 native tokens to its table index; the parser (`FUN_00477640`)
/// sets one byte per recognised token, skips unknown ones, folds ASCII
/// case, and leaves the prior array alone when the key is absent.
#[test]
fn gsi_08_12_ability_lists_parse_the_full_native_token_table() {
    // The whole table is 163 bytes; the 0x80 ReadString copy keeps 127,
    // ending after FEARLESS, so the last four tokens go in a second list.
    let all = Ability::NAMES.join(",");
    assert_eq!(all[..127].rsplit(',').next(), Some("FEARLESS"));
    let tail = Ability::NAMES[14..].join(",");
    let ini = IniFile::from_str(&format!(
        "[X]\nVeteranAbilities={all}\nEliteAbilities=self_heal,BOGUS,Rof\n\
         [T]\nVeteranAbilities={tail}\n"
    ));
    let obj = ObjectType::from_ini_section("X", ini.section("X").unwrap(), ObjectCategory::Vehicle);
    let tail_obj =
        ObjectType::from_ini_section("T", ini.section("T").unwrap(), ObjectCategory::Vehicle);
    for (index, name) in Ability::NAMES.iter().enumerate() {
        let ability = Ability::from_ini_token(name).expect("native token");
        assert_eq!(
            ability as usize, index,
            "{name} sits at native index {index}"
        );
        assert_eq!(obj.veteran_abilities.has(ability), index < 14, "{name} cut");
        if index >= 14 {
            assert!(tail_obj.veteran_abilities.has(ability), "{name} parsed");
        }
    }
    assert!(obj.elite_abilities.has(Ability::SelfHeal));
    assert!(obj.elite_abilities.has(Ability::Rof));
    assert!(!obj.elite_abilities.has(Ability::Faster));
    assert_eq!(Ability::from_ini_token("BOGUS"), None);

    let absent = IniFile::from_str("[Y]\nFixtureOnly=1\n");
    let obj =
        ObjectType::from_ini_section("Y", absent.section("Y").unwrap(), ObjectCategory::Vehicle);
    assert_eq!(obj.veteran_abilities, AbilityFlags::default());
    assert!(!obj.self_healing, "SelfHealing= defaults off");
    let prior = AbilityFlags::from_abilities(&[Ability::Sight]);
    assert_eq!(
        AbilityFlags::parse(None, prior),
        prior,
        "absent key keeps the prior array"
    );
    assert_eq!(
        AbilityFlags::parse(Some(vec!["ROF"]), prior),
        AbilityFlags::from_abilities(&[Ability::Rof]),
        "a present key replaces the whole array"
    );

    let healing = IniFile::from_str("[Z]\nSelfHealing=yes\n");
    let obj =
        ObjectType::from_ini_section("Z", healing.section("Z").unwrap(), ObjectCategory::Vehicle);
    assert!(obj.self_healing);
}

#[test]
fn gate_flag_parses_from_ini() {
    let ini = IniFile::from_str(
        "[GAGATE_A]\nGate=yes\nFoundation=3x1\nDeployTime=.044\nGateCloseDelay=.2\n",
    );
    let section = ini.section("GAGATE_A").unwrap();
    let obj = ObjectType::from_ini_section("GAGATE_A", section, ObjectCategory::Building);
    assert!(obj.gate);
    assert_eq!(obj.deploy_time_ticks, 39);
    assert_eq!(obj.gate_close_delay_ticks, 180);
}

#[test]
fn bunkerable_defaults_true_for_vehicles_only() {
    let ini = IniFile::from_str("[TEST]\nFixtureOnly=1\n");
    let section = ini.section("TEST").unwrap();

    let vehicle = ObjectType::from_ini_section("TEST", section, ObjectCategory::Vehicle);
    let infantry = ObjectType::from_ini_section("TEST", section, ObjectCategory::Infantry);
    let aircraft = ObjectType::from_ini_section("TEST", section, ObjectCategory::Aircraft);
    let building = ObjectType::from_ini_section("TEST", section, ObjectCategory::Building);

    assert!(vehicle.bunkerable);
    assert!(!infantry.bunkerable);
    assert!(!aircraft.bunkerable);
    assert!(!building.bunkerable);
}

#[test]
fn gsi_05_14_debris_keys_are_case_exact_so_the_retail_lowercase_spelling_is_unread() {
    // A death spends a count drawn from [MinDebris, MaxDebris] on
    // DebrisTypes VoxelAnims and DebrisAnims SHP anims.
    let rules = rules(
        // Registered for Ares's Find-only debris lists.
        "[VoxelAnims]\n0=TIRE\n[Animations]\n0=DBRIS1LG\n1=DBRIS1SM\n2=DBRIS4LG\n\
         [BuildingTypes]\n0=GAPOWR\n1=NAPOWR\n\
         [GAPOWR]\n\
         MinDebris=4\nMaxDebris=9\n\
         DebrisTypes=TIRE\nDebrisMaximums=3\n\
         DebrisAnims=DBRIS1LG,DBRIS1SM,DBRIS4LG\n\
         [NAPOWR]\nMaxDebris=5\n\
         debristypes=TIRE\nDEBRISMAXIMUMS=3\nDebrisanims=DBRIS1LG\n",
    );
    let obj = rules.object("GAPOWR").unwrap();
    assert_eq!((obj.min_debris, obj.max_debris), (4, 9));
    assert_eq!(obj.debris_types, vec!["TIRE".to_string()]);
    assert_eq!(obj.debris_maximums, vec![3]);
    assert_eq!(obj.debris_anims.len(), 3);

    // The lowercase spelling must NOT be read. gamemd's `INIClass` hashes
    // the raw bytes of the key on both the store side
    // (`INIClass::LoadFromStraw @ 0x005260C9`) and the lookup side
    // (`CCINIClass::ReadInt @ 0x00527715`), and both binary searches
    // compare 32-bit CRCs only — there is no folding instruction anywhere
    // on the path, and `strtrim @ 0x00727CF0` strips whitespace and
    // nothing else. `0x0084439C` is the literal `MaxDebris`, so the 17
    // stock `[VehicleTypes]` that spell it `Maxdebris=3` are invisible to
    // the read and keep the `TechnoTypeClass::Constructor 0x00710FB7`
    // default of 0. That zero closes the whole debris block at
    // `TechnoClass::ReceiveDamage 0x00702293 JLE`, so the Rhino, the
    // Apocalypse, the Prism Tank, the Battle Fortress, the IFV and 9 more
    // buildable vehicles throw nothing on death in retail and take no RNG
    // draw for it. Reading them would be the divergence, not the fix.
    let ini: IniFile = IniFile::from_str("[NAPOWR]\nMaxdebris=7\nMindebris=2\n");
    let obj = ObjectType::from_ini_section(
        "NAPOWR",
        ini.section("NAPOWR").unwrap(),
        ObjectCategory::Building,
    );
    assert_eq!((obj.min_debris, obj.max_debris), (0, 0));

    // The same rule on the CSV siblings: no stock section mis-spells any
    // of these three, so this arm is wrong-in-principle only today, but it
    // goes live the moment a mod mis-spells one.
    let obj = rules.object("NAPOWR").unwrap();
    assert_eq!(obj.max_debris, 5);
    assert!(obj.debris_types.is_empty());
    assert!(obj.debris_maximums.is_empty());
    assert!(obj.debris_anims.is_empty());
}

/// `TechnoTypeClass::ReadINI @ 0x007125BD` floors `MinDebris` at 0, then
/// `0x007125D9` raises `MaxDebris` to meet it. The order matters: flooring
/// first is what stops a negative `MinDebris` from dragging `MaxDebris`
/// below zero and re-opening the block that `MaxDebris <= 0` closes.
///
/// Stock-unreachable — 0 sections author a negative `MinDebris` and 0
/// author an inverted span — so this is modded-rules territory, but it is
/// what makes `throw_death_debris`'s inverted-span branch well-defined.
#[test]
fn gsi_05_14_mindebris_floors_at_zero_then_raises_maxdebris() {
    let ini: IniFile = IniFile::from_str("[NAPOWR]\nMaxDebris=2\nMinDebris=-9\n");
    let obj = ObjectType::from_ini_section(
        "NAPOWR",
        ini.section("NAPOWR").unwrap(),
        ObjectCategory::Building,
    );
    // MinDebris floored to 0 first, so MaxDebris is left where it was.
    assert_eq!((obj.min_debris, obj.max_debris), (0, 2));

    let ini: IniFile = IniFile::from_str("[NAPOWR]\nMaxDebris=2\nMinDebris=7\n");
    let obj = ObjectType::from_ini_section(
        "NAPOWR",
        ini.section("NAPOWR").unwrap(),
        ObjectCategory::Building,
    );
    // Inverted span: MaxDebris rises to meet MinDebris.
    assert_eq!((obj.min_debris, obj.max_debris), (7, 7));

    let ini: IniFile = IniFile::from_str("[NAPOWR]\nMaxDebris=-4\nMinDebris=-9\n");
    let obj = ObjectType::from_ini_section(
        "NAPOWR",
        ini.section("NAPOWR").unwrap(),
        ObjectCategory::Building,
    );
    // A negative MinDebris must not drag MaxDebris up out of its own
    // negative: the floor lands on 0, and MaxDebris rises only to 0.
    assert_eq!((obj.min_debris, obj.max_debris), (0, 0));
}
/// `0x00713C37`: ReadString 0x80, then each `strtok` token as written to
/// `ParticleSystemTypeClass::FindOrAllocate` (`0x00644890`).
// Read the stock way: Ares reads these lists Find-only through ParseList
// (Misc/Bugfixes.Buffer.cpp).
#[test]
fn techno_type_parses_damage_particle_systems_csv() {
    let rules = rules(
        "[BuildingTypes]\n0=GAPOWR\n[GAPOWR]\n\
         DamageParticleSystems=BigGreySSys, SmallGreySSys ,SparkSys\n\
         DestroyParticleSystems=DebrisSmokeSys\n",
    );
    let obj = rules.object("GAPOWR").unwrap();

    assert_eq!(
        obj.damage_particle_systems,
        vec![
            "BigGreySSys".to_string(),
            " SmallGreySSys ".to_string(),
            "SparkSys".to_string(),
        ]
    );
    assert_eq!(
        obj.destroy_particle_systems,
        vec!["DebrisSmokeSys".to_string()]
    );
}

/// TechnoType+0xC8F: `InfantryTypeClass::ReadINI` clears it on every pass
/// (`0x005240BD`) and sets it only on a pass that reads the type's section
/// (`0x005243E7`); no other type writes it. Ares jumps past the clear.
#[test]
fn damage_sparks_need_the_infantry_section_in_the_last_rules_pass() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;
    let sparks = |map: &str| {
        let mut stack = RulesLayerStack::new(IniFile::from_str(
            "[InfantryTypes]\n0=CYB\n[VehicleTypes]\n0=VCYB\n\
             [CYB]\nCyborg=yes\n[VCYB]\nCyborg=yes\n",
        ));
        stack.push(RulesLayerKind::Scenario, IniFile::from_str(map));
        let rules = RuleSet::from_processed_rules(&stack.process().unwrap()).unwrap();
        let spark = |id| rules.object(id).unwrap().emits_damage_spark();
        (spark("CYB"), spark("VCYB"))
    };
    assert_eq!(
        sparks("[CYB]\nStrength=90\n[VCYB]\nStrength=90\n"),
        (true, false)
    );
    assert_eq!(sparks(""), (false, false));
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    assert_eq!(sparks(""), (true, false));
}

#[test]
fn techno_type_can_passive_acquire_defaults_yes_and_parses_the_misspelled_key() {
    // Absent key → yes (the gamemd TechnoType constructor default). The INI
    // spelling is "CanPassiveAquire"; the correctly-spelled variant is NOT a
    // key the original reads, so it must not turn the flag off.
    let plain = IniFile::from_str("[E1]\nFixtureOnly=1\n");
    let obj =
        ObjectType::from_ini_section("E1", plain.section("E1").unwrap(), ObjectCategory::Infantry);
    assert!(obj.can_passive_acquire, "CanPassiveAquire defaults to yes");

    let opted_out = IniFile::from_str("[DESO]\nCanPassiveAquire=no\n");
    let obj = ObjectType::from_ini_section(
        "DESO",
        opted_out.section("DESO").unwrap(),
        ObjectCategory::Infantry,
    );
    assert!(!obj.can_passive_acquire, "CanPassiveAquire=no parses false");

    let misspelled_the_other_way = IniFile::from_str("[DESO]\nCanPassiveAcquire=no\n");
    let obj = ObjectType::from_ini_section(
        "DESO",
        misspelled_the_other_way.section("DESO").unwrap(),
        ObjectCategory::Infantry,
    );
    assert!(
        obj.can_passive_acquire,
        "only the binary's misspelled key is read"
    );
}

#[test]
fn original_eight_refinery_smoke_frames_scalar_rows() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/refinery_smoke.json",
    ))
    .unwrap();
    let rows = corpus["frames_parser"].as_array().unwrap();
    assert_eq!(rows.len(), 8);
    for row in rows {
        // Feed the original ReadInt boundary directly. A physical empty
        // INI value is removed earlier by the separate lexical loader.
        let mut section = IniSection::new("B".to_owned());
        if let Some(raw) = row["raw"].as_str() {
            section.set("RefinerySmokeFrames", raw);
        }
        let object = ObjectType::from_ini_section("B", &section, ObjectCategory::Building);
        assert_eq!(
            object.refinery_smoke_frames,
            row["output"].as_i64().unwrap() as i32,
            "{row}"
        );
    }
}

#[test]
fn no_shadow_reader_retains_native_prior_value_across_rules_layers() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;

    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=GENERATED\n[GENERATED]\nNoShadow=yes\n",
    ));
    // ReadBool's original default branch preserves the prior byte for
    // invalid values; physical INI loading omits authored empty values.
    for (kind, patch, expected) in [
        (RulesLayerKind::LangRule, "NoShadow=off", true),
        (RulesLayerKind::GameMode, "NoShadow=", true),
        (RulesLayerKind::Scenario, "NoShadow=no", false),
        (RulesLayerKind::Scenario, "noshadow=yes", false),
    ] {
        layers.push(kind, IniFile::from_str(&format!("[GENERATED]\n{patch}\n")));
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(
            rules.object("GENERATED").unwrap().no_shadow,
            expected,
            "{patch}"
        );
    }
    let defaults = IniFile::from_str("[DEFAULTS]\nFixtureOnly=yes\n");
    assert!(
        !ObjectType::from_ini_section(
            "DEFAULTS",
            defaults.section("DEFAULTS").unwrap(),
            ObjectCategory::Vehicle,
        )
        .no_shadow
    );
}

#[test]
fn turret_spins_reads_constructor_default_and_rule_layer_overrides() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;

    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=DISK\n[DISK]\nTurret=yes\n",
    ));
    assert!(
        !RuleSet::from_rules_layers(&layers)
            .unwrap()
            .object("DISK")
            .unwrap()
            .turret_spins
    );

    for (kind, patch, expected) in [
        (RulesLayerKind::LangRule, "TurretSpins=yes", true),
        (RulesLayerKind::GameMode, "TurretSpins=malformed", true),
        (RulesLayerKind::Scenario, "TurretSpins=no", false),
        (RulesLayerKind::Scenario, "TurretSpins=yes", true),
    ] {
        layers.push(kind, IniFile::from_str(&format!("[DISK]\n{patch}\n")));
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(
            rules.object("DISK").unwrap().turret_spins,
            expected,
            "{patch}"
        );
    }
}

/// The production rules reader on the stock battle source stack. DISK is
/// the authored `TurretSpins=yes` type; an ordinary tank remains the
/// default-false control.
#[test]
fn retail_flying_disk_turret_spins_reads_from_battle_rules_layers() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    assert!(retail.rules.object("DISK").unwrap().turret_spins);
    assert!(!retail.rules.object("MTNK").unwrap().turret_spins);
}

/// The production reader on retail `rulesmd.ini`: `CrateBeneath=` and
/// `CrateBeneathIsMoney=` (BuildingTypeClass::ReadINI `0x00460EC1`,
/// `0x00460EDB`), defaulting to the constructor's false.
#[test]
fn retail_crate_beneath_reads() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    for (id, beneath, money) in [
        ("CAWASH01", true, true),
        ("CARUS07", true, false),
        ("GAPOWR", false, false),
    ] {
        let object = rules.object(id).unwrap();
        assert_eq!(
            (object.crate_beneath, object.crate_beneath_is_money),
            (beneath, money),
            "{id}"
        );
    }
}

#[test]
fn retail_cloaked_naval_and_vehicle_shadow_type_inputs() {
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::rules::ruleset::RuleSet;

    let Some((ini, art_ini)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let mut rules = RuleSet::from_ini(&ini).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art_ini));
    for (id, voxel) in [("SUB", true), ("DLPH", false)] {
        let object = rules.object(id).unwrap();
        assert!(!object.no_shadow, "{id}");
        assert!(object.cloakable, "{id}");
        assert_eq!(object.locomotor, LocomotorKind::Ship, "{id}");
        assert!(!object.has_turret, "{id}");
        assert_eq!(object.turret_count, 0, "{id}");
        let art = rules
            .art()
            .resolve_metadata_entry(id, &object.image)
            .expect("retail naval body art");
        assert_eq!(art.voxel, voxel, "{id}: native VXL/SHP caller distinction");
    }
    assert!(!rules.object("MTNK").unwrap().no_shadow);
    // Positive retail controls prevent a constructor-default-only port.
    for id in ["DNOA", "DNOB"] {
        assert!(rules.object(id).unwrap().no_shadow, "{id}");
    }
}

/// ReadSoundList keeps `strtok` tokens as written; empty fields drop.
#[test]
fn parses_ordered_death_sound_lists() {
    let ini = IniFile::from_str("[E1]\nVoiceDie= VoiceA,VoiceB, VoiceC \nDieSound= DieA,,DieB\n");
    let object =
        ObjectType::from_ini_section("E1", ini.section("E1").unwrap(), ObjectCategory::Infantry);

    assert_eq!(object.voice_die, ["VoiceA", "VoiceB", " VoiceC"]);
    assert_eq!(object.die_sounds, ["DieA", "DieB"]);
}

#[test]
fn crushable_defaults_yes_for_infantry_and_no_for_everything_else() {
    // The InfantryTypeClass constructor overwrites the ObjectTypeClass
    // default with 1, so a key-less infantry section (Conscript, Engineer,
    // Flak Trooper, ... in stock rulesmd) is crushable.
    let ini = IniFile::from_str("[E2]\nName=Conscript\n");
    let section = ini.section("E2").unwrap();
    assert!(ObjectType::from_ini_section("E2", section, ObjectCategory::Infantry).crushable);

    let ini = IniFile::from_str("[MTNK]\nSpeed=7\n");
    let section = ini.section("MTNK").unwrap();
    assert!(!ObjectType::from_ini_section("MTNK", section, ObjectCategory::Vehicle).crushable);

    // An explicit key still wins in both directions.
    let ini = IniFile::from_str("[DESO]\nCrushable=no\n");
    let section = ini.section("DESO").unwrap();
    assert!(!ObjectType::from_ini_section("DESO", section, ObjectCategory::Infantry).crushable);

    let ini = IniFile::from_str("[GASAND]\nCrushable=yes\n");
    let section = ini.section("GASAND").unwrap();
    assert!(ObjectType::from_ini_section("GASAND", section, ObjectCategory::Building).crushable);
}

/// TechnoType ctor711500/default and ReadINI7153DA..7153F5.
#[test]
fn crush_tilt_reader_preserves_native_true_default_and_explicit_no() {
    for (line, expected) in [
        ("", true),
        ("TiltsWhenCrushes=no", false),
        ("TiltsWhenCrushes=yes", true),
    ] {
        let ini = IniFile::from_str(&format!("[MTNK]\nSpeed=7\n{line}\n"));
        let object = ObjectType::from_ini_section(
            "MTNK",
            ini.section("MTNK").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert_eq!(object.tilts_when_crushes, expected);
    }
}

#[test]
fn absent_weight_keeps_the_techno_type_constructor_value() {
    // Original TechnoType710C66..710C71 writes 1.0. UnitType7470D0
    // does not replace it before its reader calls TechnoType712170.
    let ini = IniFile::from_str("[MTNK]\nStrength=300\n");
    let mtnk = ObjectType::from_ini_section(
        "MTNK",
        ini.section("MTNK").expect("MTNK section"),
        ObjectCategory::Vehicle,
    );
    assert_eq!(mtnk.weight, SimFixed::ONE);
}

/// Retail `rulesmd.ini`, read from the gitignored `ini/` corpus.
///
/// The golden is retail INI bytes, not a hand-written fixture, so these are
/// parity checks on the authored data rather than Rust-vs-Rust. `None` (the
/// test skips) when `ini/` is absent; see `retail_ini_fixture`.
#[cfg(test)]
fn retail_rules_ini() -> Option<IniFile> {
    crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini")
}

/// Retail `Trainable=` through the production reader: a BuildingType is
/// untrainable unless its section says so (constructor `0x0045E42E`), so a
/// garrisonable building and a Tank Bunker never rank while the Yuri
/// refinery (the one retail `Trainable=yes` building) and ordinary units
/// do.
#[test]
fn retail_trainable_defaults_by_category() {
    let Some(ini) = retail_rules_ini() else {
        return;
    };
    let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).expect("retail rules parse");
    for (id, trainable) in [
        ("CAGAS01", false),
        ("NABNKR", false),
        ("GAPILL", false),
        ("YAREFN", true),
        ("E1", true),
        ("HTNK", true),
    ] {
        assert_eq!(rules.object(id).unwrap().trainable, trainable, "[{id}]");
    }
}

/// The two stock sections that take their Jumpjet locomotor from the
/// `Locomotor=` GUID alone and never author `JumpJet=`.
///
/// gamemd reads all nine parameters for them anyway —
/// `TechnoTypeClass::ReadINI` `0x00715020`-`0x0071520F` has no branch — so
/// a retail Kirov and Floating Disc hover at their authored 750, not at the
/// constructor's 500 (`0x007115D3`). Gating the block on `JumpJet=yes`
/// dropped every one of these values.
#[test]
fn retail_kirov_and_disc_keep_their_jumpjet_block_without_the_flag() {
    let Some(ini) = retail_rules_ini() else {
        return;
    };

    for id in ["ZEP", "DISK"] {
        let section = ini.section(id).unwrap_or_else(|| panic!("[{id}] section"));
        assert!(
            section.get_for_test("JumpJet").is_none(),
            "[{id}] is only interesting because stock omits JumpJet="
        );
        let obj = ObjectType::from_ini_section(id, section, ObjectCategory::Vehicle);
        assert!(
            !obj.jumpjet,
            "[{id}] JumpJet= is absent, so the flag is false"
        );
        assert_eq!(
            obj.locomotor,
            LocomotorKind::Jumpjet,
            "[{id}] takes the Jumpjet locomotor from its CLSID"
        );
        assert_eq!(
            obj.jumpjet_params.height, 750,
            "[{id}] authors JumpjetHeight=750 and gamemd reads it"
        );
    }

    let zep = ObjectType::from_ini_section(
        "ZEP",
        ini.section("ZEP").expect("[ZEP]"),
        ObjectCategory::Vehicle,
    );
    assert_eq!(
        zep.jumpjet_params.speed,
        crate::util::fixed_math::SimFixed::from_num(5.0_f32)
    );
    assert_eq!(zep.jumpjet_params.climb, 6.0);
    assert_eq!(zep.jumpjet_params.crash, 12.0);
    assert!(zep.jumpjet_params.no_wobbles, "[ZEP] JumpjetNoWobbles=yes");
    // `[ZEP]` authors no deviation, so the constructor's 40 stands.
    assert_eq!(zep.jumpjet_params.deviation, 40);

    let disk = ObjectType::from_ini_section(
        "DISK",
        ini.section("DISK").expect("[DISK]"),
        ObjectCategory::Vehicle,
    );
    assert_eq!(
        disk.jumpjet_params.speed,
        crate::util::fixed_math::SimFixed::from_num(16.0_f32)
    );
    assert_eq!(disk.jumpjet_params.climb, 8.0);
    assert_eq!(disk.jumpjet_params.crash, 15.0);
    assert_eq!(disk.jumpjet_params.deviation, 15);
}

/// The six stock sections that do author `JumpJet=yes` must be untouched by
/// removing the gate: they all author `JumpjetHeight=500`, which is also the
/// constructor seed.
#[test]
fn retail_jumpjet_yes_sections_are_unchanged_by_dropping_the_gate() {
    let Some(ini) = retail_rules_ini() else {
        return;
    };
    // Categories as the stock registries list them: the two jumpjet
    // infantry under `[InfantryTypes]`, the four choppers/transports under
    // `[VehicleTypes]`. Category does not reach the jumpjet block, but
    // parsing a section under the wrong one invites a later reader to
    // misread the fixture.
    for (id, category) in [
        ("JUMPJET", ObjectCategory::Infantry),
        ("LUNR", ObjectCategory::Infantry),
        ("SHAD", ObjectCategory::Vehicle),
        ("HIND", ObjectCategory::Vehicle),
        ("SCHP", ObjectCategory::Vehicle),
        ("SCHD", ObjectCategory::Vehicle),
    ] {
        let section = ini.section(id).unwrap_or_else(|| panic!("[{id}] section"));
        let obj = ObjectType::from_ini_section(id, section, category);
        assert!(obj.jumpjet, "[{id}] authors JumpJet=yes");
        assert_eq!(obj.jumpjet_params.height, 500, "[{id}] authors 500");
    }
}

/// gamemd looks up `JumpjetTurnRate` (`0x008436D0`) and `JumpjetAccel`
/// (`0x00843680`) — lowercase `jet` — and `INIClass` compares CRC-32s of
/// the raw key bytes (`CRCEngine::AddData @ 0x004A1DE0`), so the capital-J
/// spelling stock uses is invisible to it. Every stock jumpjet section
/// therefore flies on the constructor's turn rate 4 (`0x007115AE`) and
/// acceleration 2.0 (`0x007115DD`).
///
/// Derived over the retail file rather than asserted from a list: the
/// section set is whatever authors a `Jumpjet*` key, and the two counts are
/// counted here.
#[test]
fn retail_never_authors_the_spelling_gamemd_reads_for_turn_rate_or_accel() {
    let Some(ini) = retail_rules_ini() else {
        return;
    };

    let mut native_spelling = 0usize;
    let mut ini_spelling = 0usize;
    let mut jumpjet_sections = 0usize;
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for name in ini.section_names() {
        if !seen.insert(name) {
            continue;
        }
        let Some(section) = ini.section(name) else {
            continue;
        };
        let authors_any = section
            .keys()
            .any(|k| k.to_ascii_lowercase().starts_with("jumpjet"));
        if !authors_any {
            continue;
        }
        jumpjet_sections += 1;
        for key in ["JumpjetTurnRate", "JumpjetAccel"] {
            if section.get_for_test(key).is_some() {
                native_spelling += 1;
            }
        }
        for key in ["JumpJetTurnRate", "JumpJetAccel"] {
            if section.get_for_test(key).is_some() {
                ini_spelling += 1;
            }
        }

        let obj = ObjectType::from_ini_section(name, section, ObjectCategory::Vehicle);
        assert_eq!(
            obj.jumpjet_params.turn_rate, 4,
            "[{name}] must keep the constructor turn rate"
        );
        assert_eq!(
            obj.jumpjet_params.accel, 2.0,
            "[{name}] must keep the constructor acceleration"
        );
    }

    assert_eq!(
        native_spelling, 0,
        "no stock section spells the keys the way gamemd looks them up"
    );
    assert_eq!(
        (jumpjet_sections, ini_spelling),
        (8, 16),
        "8 stock sections author Jumpjet* keys and all 8 carry both mis-cased keys"
    );
}

#[test]
fn techno_category_retains_stock_battle_values() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };

    assert_eq!(
        battle.rules.object("E1").expect("stock E1").techno_category,
        TechnoTypeCategory::Soldier
    );
    assert_eq!(
        battle
            .rules
            .object("MTNK")
            .expect("stock MTNK")
            .techno_category,
        TechnoTypeCategory::ArmoredFightingVehicle
    );
}

#[test]
fn techno_category_uses_native_names_and_current_default_per_pass() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;

    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let mut layers = RulesLayerStack::new((*battle.authored_rules).clone());
    layers.push(
        RulesLayerKind::GameMode,
        IniFile::from_str("[E1]\nCategory=Armored Fighting Vehicle\n[MTNK]\nCategory=AFV\n"),
    );
    let named = RuleSet::from_rules_layers(&layers).expect("production Category pass");
    assert_eq!(
        named.object("E1").expect("registered E1").techno_category,
        TechnoTypeCategory::ArmoredFightingVehicle
    );
    assert_eq!(
        named
            .object("MTNK")
            .expect("registered MTNK")
            .techno_category,
        TechnoTypeCategory::ArmoredFightingVehicle
    );

    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[E1]\nCost=1\n[MTNK]\nCategory=not-a-native-category\n"),
    );
    let missing_and_unknown =
        RuleSet::from_rules_layers(&layers).expect("production Category boundaries");
    assert_eq!(
        missing_and_unknown
            .object("E1")
            .expect("registered E1")
            .techno_category,
        TechnoTypeCategory::ArmoredFightingVehicle
    );
    assert_eq!(
        missing_and_unknown
            .object("MTNK")
            .expect("registered MTNK")
            .techno_category,
        TechnoTypeCategory::Unknown
    );

    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[E1]\nCategory=not-a-native-category\n[MTNK]\nCost=1\n"),
    );
    let unknown = RuleSet::from_rules_layers(&layers).expect("production layered Category");
    assert_eq!(
        unknown.object("E1").expect("registered E1").techno_category,
        TechnoTypeCategory::Unknown
    );
    assert_eq!(
        unknown
            .object("MTNK")
            .expect("registered MTNK")
            .techno_category,
        TechnoTypeCategory::Unknown
    );
}
