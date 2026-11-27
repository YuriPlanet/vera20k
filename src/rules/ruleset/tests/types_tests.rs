//! Tests of the type registries, name and handle lookups, ART merging and
//! the bounds on out-of-range rules numbers.

use super::*;

/// Ares TechnoTypeClass::Find (10007EA0): the first TechnoType whose ID
/// matches ignoring ASCII case, none for `none`/`<none>` or an unknown
/// name; indexed once, the answers of the scan. `Dup` and `DUP` sit in two
/// arrays: the vehicle comes first in the native order and wins.
#[test]
fn ares_find_techno_type_matches_ignoring_case() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]
0=E1
1=DUP
[VehicleTypes]
0=MTNK
1=Dup
[AircraftTypes]
[BuildingTypes]
0=GAPOWR
[E1]
Strength=125
[MTNK]
Strength=300
[GAPOWR]
Strength=750
",
    ))
    .unwrap();
    let find = |name| {
        rules
            .ares_find_techno_type(name)
            .map(|object| object.id.as_str())
    };
    assert_eq!(find("mtnk"), Some("MTNK"));
    assert_eq!(find("E1"), Some("E1"));
    assert_eq!(find("gaPowr"), Some("GAPOWR"));
    assert_eq!(find("none"), None);
    assert_eq!(find("<none>"), None);
    assert_eq!(find("HTNK"), None);
    assert_eq!(find("dup"), Some("Dup"));
}

#[test]
fn ares_emp_ai_global_mission_uses_production_layers_and_config_hash() {
    use crate::rules::{
        mission_data::MissionType,
        native_processing::{RulesLayerKind, RulesLayerStack},
    };
    for ares_way in [false, true] {
        let _ares = crate::rules::ares_switch::read_ares_way(ares_way);
        let root = "[VehicleTypes]\n0=TANK\n[TANK]\nStrength=400\n[CombatDamage]\nEMPAIRecoverMission=Guard\n";
        let make = |language: &str| {
            let mut layers = RulesLayerStack::new(IniFile::from_str(root));
            layers.push(
                RulesLayerKind::LangRule,
                IniFile::from_str(&format!("[CombatDamage]\nEMPAIRecoverMission={language}\n")),
            );
            layers.push(
                RulesLayerKind::GameMode,
                IniFile::from_str("[CombatDamage]\nEMPAIRecoverMission=junk\n"),
            );
            layers.push(
                RulesLayerKind::Scenario,
                IniFile::from_str("[CombatDamage]\nEMPAIRecoverMission=\n"),
            );
            RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap()
        };
        let rules = make("Area Guard");
        let expected = ares_way.then_some(MissionType::AreaGuard);
        assert_eq!(rules.ares_emp_ai_recover_mission(), expected);
        let retained_hash = rules.simulation_config_hash();
        let mut changed = make("Hunt");
        // Source identity deliberately includes layer history, even when the
        // final merged text agrees. Normalize only this private test input to
        // isolate the retained extension state in the compatibility hash.
        assert_ne!(rules.source_ini_hash(), changed.source_ini_hash());
        changed.source_ini_hash = rules.source_ini_hash;
        assert_eq!(
            changed.ares_emp_ai_recover_mission(),
            ares_way.then_some(MissionType::Hunt)
        );
        assert_eq!(retained_hash == changed.simulation_config_hash(), !ares_way);
        let fresh = RuleSet::from_processed_rules(
            &RulesLayerStack::new(IniFile::from_str(
                "[VehicleTypes]\n0=TANK\n[TANK]\nStrength=400\n",
            ))
            .process()
            .unwrap(),
        )
        .unwrap();
        assert!(
            fresh.ares_emp_ai_recover_mission().is_none(),
            "new RulesClass constructor"
        );
    }
}
#[test]
fn infantry_occupy_pip_uses_each_admitted_rules_section_pass() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    // Original Infantry5240D1..524125 calls ReadPip4748A0 on every
    // admitted section, even when the pip key is missing. Executed native
    // section controls: passenger_pips.json/read_pip (also Pip3→4→5→7).
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[E1]\nPip=white\nOccupyPip=PersonBlue\n",
    ));
    for (patch, expected) in [
        ("[Unrelated]\nStrength=1\n", 11),
        ("[E1]\nStrength=125\n", 1),
        ("[E1]\nStrength=126\n", 2),
        ("[E1]\nStrength=127\n", 3),
    ] {
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(patch));
        let processed = layers.process().expect("ordered native rules passes");
        let rules = RuleSet::from_processed_rules(&processed).expect("projected types");
        assert_eq!(
            rules.object("E1").unwrap().occupy_pip(),
            expected,
            "ReadPip must use the retained field as a table index after {patch:?}"
        );
    }
}

fn detail_native() -> &'static serde_json::Value {
    crate::test_fixture::json("tools/rules_oracle/weapon_laser.json")
}

fn detail_state(detail: DetailRules) -> serde_json::Value {
    serde_json::json!({
        "min_frame_rate_normal": detail.min_frame_rate_normal,
        "min_frame_rate_movie": detail.min_frame_rate_movie,
        "buffer_zone_width": detail.buffer_zone_width,
    })
}

fn detail_cached_ini(sections: &serde_json::Value) -> IniFile {
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

#[test]
fn detail_rules_match_original_constructor_and_signed_readers() {
    let native = detail_native();
    assert_eq!(
        detail_state(DetailRules::default()),
        native["detail"]["constructor"]
    );
    let rows = native["detail"]["scalar_controls"].as_array().unwrap();
    assert_eq!(rows.len(), 15);
    for row in rows {
        let ini = detail_cached_ini(&row["sections"]);
        let rules = RuleSet::from_ini(&ini).unwrap();
        assert_eq!(detail_state(rules.general.detail), row["state"], "{row}");
    }
}

#[test]
fn detail_rules_retain_state_through_cold_reads_process_handoffs_and_type_reset() {
    use crate::rules::native_processing::{
        NativeRulesRegistryState, RulesLayerStack, process_native_rules_cold_start,
    };

    let native = detail_native();
    let rows = native["detail"]["retained_history"].as_array().unwrap();
    assert_eq!(rows.len(), 10);
    let mut retained = NativeRulesRegistryState::default();
    for row in rows {
        let pass = detail_cached_ini(&row["sections"]);
        if row["kind"] == "audio_visual" {
            retained = process_native_rules_cold_start(
                retained,
                &pass,
                &IniFile::empty(),
                None,
                &crate::rules::sound_ini::EvaRegistry::default(),
                &crate::rules::sound_ini::SoundRegistry::default(),
            )
            .unwrap()
            .into_registry_state_discarding_events();
        } else if row["kind"] == "type_reset_and_process" {
            retained = retained.destructive_reset();
        }
        // The empty Process after a cold read exposes retained RulesClass
        // fields without replaying the authored AudioVisual keys.
        let pass = if row["kind"] == "audio_visual" {
            IniFile::empty()
        } else {
            pass
        };
        let processed = RulesLayerStack::new(pass)
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), retained)
            .unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        assert_eq!(detail_state(rules.general.detail), row["state"], "{row}");
        let (_, trace) = processed.into_ini_and_native_type_construction_trace();
        retained = trace.into_registry_state_discarding_events();
    }
}

#[test]
fn retained_detail_rules_change_configuration_identity() {
    use crate::rules::native_processing::RulesLayerStack;
    let mut identities = Vec::new();
    for initial in [15, 31] {
        let first = RulesLayerStack::new(IniFile::from_str(&format!(
            "[AudioVisual]\nDetailMinFrameRateNormal={initial}\n"
        )))
        .process()
        .unwrap();
        let (_, trace) = first.into_ini_and_native_type_construction_trace();
        let processed = RulesLayerStack::new(IniFile::empty())
            .process_with_fixed_art_and_registry_state(
                &IniFile::empty(),
                trace.into_registry_state_discarding_events(),
            )
            .unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        assert_eq!(rules.general.detail.min_frame_rate_normal, initial);
        identities.push((rules.source_ini_hash(), rules.simulation_config_hash()));
    }
    assert_eq!(identities[0].0, identities[1].0);
    assert_ne!(identities[0].1, identities[1].1);
}

#[test]
fn gsi_05_14_voxel_anim_registry_indexes_the_stock_list_order() {
    // `[VoxelAnims]` is a numbered registry like `[Particles]`; ids are
    // list order and `DebrisTypes=`/`Spawns=` resolve by name.
    let ini = crate::rules::ini_parser::IniFile::from_str(
        "[VoxelAnims]
1=PIECE
2=TIRE
3=METEOR01
\
             [PIECE]
Duration=75
Elasticity=0
\
             [TIRE]
Duration=150
Elasticity=0.8
\
             [METEOR01]
IsMeteor=yes
Spawns=PEBBLE
SpawnCount=3
",
    );
    let rules = RuleSet::from_ini(&ini).expect("voxel anim rules parse");
    // METEOR01's `Spawns=PEBBLE` goes through `VoxelAnimTypeClass::
    // FindOrCreate @ 0x0074BB20`, which appends the unlisted PEBBLE.
    assert_eq!(rules.voxel_anim_type_count(), 4);
    assert_eq!(rules.voxel_anim_type_id_by_name("PEBBLE").unwrap().0, 3);
    let piece = rules
        .voxel_anim_type_id_by_name("piece")
        .expect("case-insensitive lookup");
    assert_eq!(piece.0, 0);
    assert_eq!(rules.voxel_anim_type(piece).duration, 75);
    let meteor = rules.voxel_anim_type_id_by_name("METEOR01").unwrap();
    assert_eq!(meteor.0, 2);
    let meteor = rules.voxel_anim_type(meteor);
    assert!(meteor.is_meteor);
    assert_eq!(meteor.spawns.as_deref(), Some("PEBBLE"));
    assert_eq!(meteor.spawn_count, 3);
}

/// A type's and a warhead's `DebrisTypes=`, here from a map layer, read as
/// the `[VoxelAnims]` ids their names resolve to, in list order and in any
/// case, from the ids the set resolved at build; a copy another set never
/// resolved reads its names.
#[test]
fn debris_types_read_the_ids_their_names_resolve_to() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VoxelAnims]\n0=PIECE\n1=TIRE\n[PIECE]\nDuration=75\n[TIRE]\nDuration=150\n\
         [VehicleTypes]\n0=TRUCK\n[TRUCK]\nStrength=100\n[Warheads]\n0=WH\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str(
            "[TRUCK]\nDebrisTypes=tire,Piece,TIRE\nDebrisMaximums=1,1,1\n\
             [WH]\nDebrisTypes=piece\n",
        ),
    );
    let rules = RuleSet::from_rules_layers(&layers).expect("layered debris rules parse");
    let by_name = |names: &[String]| -> Vec<_> {
        names
            .iter()
            .map(|name| rules.voxel_anim_type_id_by_name(name))
            .collect()
    };
    let (piece, tire) = (
        rules.voxel_anim_type_id_by_name("PIECE"),
        rules.voxel_anim_type_id_by_name("TIRE"),
    );
    let truck = rules.object("TRUCK").unwrap();
    let debris = rules.object_debris_types(truck);
    assert!(matches!(debris, std::borrow::Cow::Borrowed(_)));
    assert_eq!(*debris, [tire, piece, tire]);
    assert_eq!(*debris, *by_name(&truck.debris_types));
    let warhead = rules.warhead("WH").unwrap();
    let debris = rules.warhead_debris_types(warhead);
    assert!(matches!(debris, std::borrow::Cow::Borrowed(_)));
    assert_eq!(*debris, [piece]);
    let other = RuleSet::from_rules_layers(&layers).unwrap();
    assert_eq!(*other.object_debris_types(truck), [tire, piece, tire]);
    assert_eq!(*other.warhead_debris_types(warhead), [piece]);
}

/// `[Radiation] RadSiteWarhead=` reads the warhead its name resolves to, in
/// any case, from a later layer too.
#[test]
fn radiation_site_warhead_reads_the_warhead_its_name_names() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[Radiation]\nRadSiteWarhead=RadSite\n[Warheads]\n0=RadSite\n1=Hot\n",
    ));
    let rules = RuleSet::from_rules_layers(&layers).unwrap();
    assert!(std::ptr::eq(
        rules.radiation_site_warhead().unwrap(),
        rules.warhead("RADSITE").unwrap()
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[Radiation]\nRadSiteWarhead=hot\n"),
    );
    let rules = RuleSet::from_rules_layers(&layers).unwrap();
    assert_eq!(rules.radiation_site_warhead().unwrap().id, "Hot");
}

#[test]
fn test_object_lookup() {
    let ini: IniFile = IniFile::from_str(&make_test_rules());
    let art = IniFile::from_str("[GAPOWR]\nFoundation=2x2\n");
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("Should parse");

    let e1: &ObjectType = rules.object("E1").expect("E1 exists");
    assert_eq!(e1.cost, 200);
    assert_eq!(e1.strength, 125);
    assert_eq!(e1.category, ObjectCategory::Infantry);
    assert_eq!(e1.primary(), Some("M60"));
    assert_eq!(
        e1.build_time_multiplier,
        NativeF32Bits::from_bits(1.15_f32.to_bits())
    );

    let mtnk: &ObjectType = rules.object("MTNK").expect("MTNK exists");
    assert_eq!(mtnk.cost, 700);
    assert_eq!(mtnk.category, ObjectCategory::Vehicle);
    assert_eq!(mtnk.secondary(), Some("MachGun"));

    let gapowr: &ObjectType = rules.object("GAPOWR").expect("GAPOWR exists");
    assert_eq!(gapowr.power, 200);
    assert_eq!(gapowr.foundation, "2x2");
}

#[test]
fn test_weapon_and_warhead_loading() {
    let ini: IniFile = IniFile::from_str(&make_test_rules());
    let rules: RuleSet = RuleSet::from_ini(&ini).expect("Should parse");

    // Weapons referenced by objects should be loaded.
    let m60: &WeaponType = rules.weapon("M60").expect("M60 exists");
    assert_eq!(m60.damage, 25);
    assert_eq!(m60.warhead, Some("SA".to_string()));

    let cannon: &WeaponType = rules.weapon("105mm").expect("105mm exists");
    assert_eq!(cannon.damage, 65);
    assert_eq!(cannon.warhead, Some("AP".to_string()));
    assert_eq!(cannon.burst, 2);
    assert_eq!(cannon.projectile, Some("InvisibleLow".to_string()));

    // Burst defaults to 1 when not specified.
    assert_eq!(m60.burst, 1);

    // Projectiles referenced by weapons should be loaded.
    assert_eq!(rules.projectile_count(), 1);
    let proj = rules
        .projectile("InvisibleLow")
        .expect("InvisibleLow exists");
    assert!(!proj.aa);
    assert!(proj.ag);

    // Warheads referenced by weapons should be loaded.
    let sa: &WarheadType = rules.warhead("SA").expect("SA exists");
    assert_eq!(
        f64::from_bits(
            sa.interaction(crate::rules::armor::armor_index("none"))
                .multiplier()
                .bits()
        ),
        1.0
    );
    assert_eq!(
        f64::from_bits(
            sa.interaction(crate::rules::armor::armor_index("heavy"))
                .multiplier()
                .bits()
        ),
        0.25
    );

    let ap: &WarheadType = rules.warhead("AP").expect("AP exists");
    assert!(
        (f64::from_bits(
            ap.interaction(crate::rules::armor::armor_index("wood"))
                .multiplier()
                .bits()
        ) - 0.6)
            .abs()
            < 1e-9
    );
}

#[test]
fn registry_only_warhead_loads_through_rules_layer_stack() {
    let mut layers = RulesLayerStack::new(IniFile::from_str("[General]\n"));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str(
            "[Warheads]\n\
                 0=RegistryOnlyWH\n\
                 [RegistryOnlyWH]\n\
                 CellSpread=2\n\
                 PercentAtMax=.5\n\
                 Verses=100%,90%,80%,70%,60%,50%,40%,30%,20%,10%,0%\n",
        ),
    );

    let rules = RuleSet::from_rules_layers(&layers).expect("layered registry-only rules parse");
    assert_eq!(
        rules.weapon_count(),
        0,
        "the fixture must not smuggle the warhead in through a weapon"
    );
    assert_eq!(rules.warhead_count(), 1);

    let lower = rules
        .warhead("registryonlywh")
        .expect("scenario-created registry warhead resolves case-insensitively");
    let mixed = rules
        .warhead("RegistryOnlyWh")
        .expect("mixed-case registry warhead lookup resolves");
    assert!(std::ptr::eq(lower, mixed));
    assert_eq!(lower.id, "RegistryOnlyWH");
    assert_eq!(lower.percent_at_max_f64, 0.5);
    let expected = [1.0, 0.9, 0.8, 0.7, 0.6, 0.5, 0.4, 0.3, 0.2, 0.1, 0.0];
    for (name, expected) in crate::rules::armor::RETAIL_ARMOR_NAMES
        .into_iter()
        .zip(expected)
    {
        let actual = f64::from_bits(
            lower
                .interaction(crate::rules::armor::armor_index(name))
                .multiplier()
                .bits(),
        );
        assert!((actual - expected).abs() < 1e-9);
    }
}

#[test]
fn refinery_helpers_are_data_driven_and_case_insensitive() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
             [VehicleTypes]\n\
             0=MODHARV\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=MODPROC\n\
             1=FAKEREF\n\
             [MODHARV]\n\
             Harvester=yes\n\
             Dock=modproc\n\
             [MODPROC]\n\
             Refinery=yes\nDockUnload=yes\n\
             FreeUnit=modharv\n\
             [FAKEREF]\n\
             Name=Fake Refinery\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("Should parse");

    assert!(rules.object("modproc").is_some_and(|obj| obj.refinery));
    assert!(!rules.object("FAKEREF").is_some_and(|obj| obj.refinery));
    assert_eq!(rules.building_free_unit("MODPROC"), Some("MODHARV"));
    assert!(rules.harvester_can_dock_at("modharv", "MODPROC"));
    assert!(!rules.harvester_can_dock_at("modharv", "GAREFN"));
}

/// `BuildingTypeClass::ReadINI @ 0x0045FE50` reads `FreeUnit` through
/// `UnitTypeClass::Find_Or_Allocate`, so an unregistered name constructs an
/// empty UnitType instead of being dropped (see
/// `RULES_PROCESS_NATIVE_ID_CONSTRUCTOR_CHRONOLOGY_REINVESTIGATION_GHIDRA_REPORT.md`).
#[test]
fn refinery_free_unit_allocates_missing_target_like_native() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=MODPROC\n\
             [MODPROC]\n\
             Refinery=yes\nDockUnload=yes\n\
             FreeUnit=UNKNOWN\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("Should parse");

    assert!(rules.object("MODPROC").is_some_and(|obj| obj.refinery));
    assert_eq!(rules.building_free_unit("MODPROC"), Some("UNKNOWN"));
    assert!(
        rules.type_handle("UNKNOWN").is_some(),
        "native Find_Or_Allocate constructs the referenced UnitType"
    );
}

#[test]
fn harvester_scan_radii_parsed_from_general() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [General]\n\
             TiberiumShortScan=10\n\
             TiberiumLongScan=6.5\n\
             HarvesterLoadRate=3\n\
             SlaveMinerShortScan=12\n\
             SlaveMinerSlaveScan=20\n\
             SlaveMinerLongScan=55\n\
             SlaveMinerScanCorrection=5\n\
             SlaveMinerKickFrameDelay=200\n\
             HarvesterTooFarDistance=8\n\
             MaximumQueuedObjects=$1D\n\
             ChronoHarvTooFarDistance=40\n\
             ApproachTargetResetMultiplier=1.5\n\
             PurifierBonus=.30\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("Should parse");
    // ReadRange: cells x 256, chopped.
    assert_eq!(rules.general.tiberium_short_scan, 10 * 256);
    assert_eq!(rules.general.tiberium_long_scan, 1664);
    assert_eq!(rules.general.harvester_load_rate, 3);
    assert_eq!(rules.general.slave_miner_short_scan, 12 * 256);
    assert_eq!(rules.general.slave_miner_slave_scan, 20 * 256);
    assert_eq!(rules.general.slave_miner_long_scan, 55 * 256);
    assert_eq!(rules.general.slave_miner_scan_correction, 5 * 256);
    assert_eq!(rules.general.slave_miner_kick_frame_delay, 200);
    // ReadInt stops at the decimal point.
    assert_eq!(rules.general.approach_target_reset_multiplier, 1);
    assert_eq!(rules.general.harvester_too_far_distance, 8);
    assert_eq!(
        rules.general.maximum_queued_objects, 29,
        "ReadInt takes `$` hex"
    );
    assert_eq!(rules.general.chrono_harv_too_far_distance, 40);
    assert_eq!(rules.general.purifier_bonus.bits(), 0x3E99_999A);
}

#[test]
fn harvester_scan_radii_use_defaults_when_missing() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [General]\n\
             FixtureOnly=1\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("Should parse");
    // The RulesClass constructor's leptons (`0x00667638`, `0x00667642`).
    assert_eq!(rules.general.tiberium_short_scan, 0x600);
    assert_eq!(rules.general.tiberium_long_scan, 0x2000);
    assert_eq!(rules.general.harvester_load_rate, 2);
    assert_eq!(rules.general.slave_miner_short_scan, 0x500);
    assert_eq!(rules.general.slave_miner_slave_scan, 0x1000);
    assert_eq!(rules.general.slave_miner_long_scan, 0x5000);
    assert_eq!(rules.general.slave_miner_scan_correction, 0x300);
    assert_eq!(rules.general.slave_miner_kick_frame_delay, 0x7FFF_FFFF);
    assert_eq!(rules.general.approach_target_reset_multiplier, 1);
    assert_eq!(rules.general.harvester_too_far_distance, 5);
    assert_eq!(rules.general.maximum_queued_objects, 5);
    assert_eq!(rules.general.chrono_harv_too_far_distance, 50);
    assert_eq!(rules.general.purifier_bonus.bits(), 0x3E80_0000);
}

/// ReadDouble -> FSTP float retains the exact stored binary32, including
/// fractions that neither whole percent nor PPM represent exactly.
#[test]
fn purifier_bonus_keeps_fractional_precision() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [General]\n\
             PurifierBonus=.333\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("Should parse");
    assert_eq!(rules.general.purifier_bonus.bits(), 0x3EAA_7EFA);
    assert_ne!(rules.general.purifier_bonus.bits(), 0x3EA8_F5C2);
}

#[test]
fn from_ini_loads_tibtre_terrain_object_types() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [TerrainTypes]\n1=TIBTRE01\n2=TREE01\n\
             [TIBTRE01]\nSpawnsTiberium=yes\nIsAnimated=yes\n\
             AnimationRate=3\nAnimationProbability=.003\n\
             [TREE01]\nIsAnimated=no\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("rules parse");
    let t = rules
        .terrain_object_type_case_insensitive("tibtre01")
        .expect("TIBTRE01 should be parsed");
    assert!(t.spawns_tiberium);
    assert_eq!(t.animation_probability.bits(), 0x3b44_9ba6);
    // TREE01 also parsed but with default flags.
    let tree = rules
        .terrain_object_type_case_insensitive("TREE01")
        .expect("TREE01 should be parsed");
    assert!(!tree.spawns_tiberium);
}

fn make_particle_test_rules(extra: &str) -> String {
    // Minimal rules that load into a RuleSet — empty registries for the
    // unit categories so RuleSet::from_ini doesn't reject the input.
    format!(
        "\
[General]
BuildSpeed=0.75
MultipleFactory=0.7
LowPowerPenaltyModifier=1.25
MinLowPowerProductionSpeed=0.4
MaxLowPowerProductionSpeed=0.85

[InfantryTypes]
[VehicleTypes]
[AircraftTypes]
[BuildingTypes]

{extra}",
    )
}

#[test]
fn factory_resolves_next_particle_regardless_of_order() {
    let extra = "\
[Particles]
1=ChainEnd
2=ChainStart

[ChainStart]
NextParticle=ChainEnd
BehavesLike=Gas

[ChainEnd]
BehavesLike=Gas
";
    let ini = IniFile::from_str(&make_particle_test_rules(extra));
    let rs = RuleSet::from_ini(&ini).unwrap();
    let start_id = rs.p_type_id_by_name("ChainStart").unwrap();
    let end_id = rs.p_type_id_by_name("ChainEnd").unwrap();
    assert_eq!(rs.particle_type(start_id).next_particle, Some(end_id));
    assert_eq!(rs.particle_type(end_id).next_particle, None);
}

#[test]
fn factory_resolves_holds_what() {
    let extra = "\
[Particles]
1=Smoke1

[ParticleSystems]
1=BigSmoke

[BigSmoke]
HoldsWhat=Smoke1
BehavesLike=Smoke

[Smoke1]
BehavesLike=Smoke
";
    let ini = IniFile::from_str(&make_particle_test_rules(extra));
    let rs = RuleSet::from_ini(&ini).unwrap();
    let s = rs.ps_type_id_by_name("BigSmoke").unwrap();
    let p = rs.p_type_id_by_name("Smoke1").unwrap();
    assert_eq!(rs.particle_system_type(s).holds_what, Some(p));
}

#[test]
fn next_particle_allocates_a_missing_section_type() {
    let extra = "\
[Particles]
1=GhostRef

[GhostRef]
NextParticle=DoesNotExist
BehavesLike=Gas
";
    let ini = IniFile::from_str(&make_particle_test_rules(extra));
    let rs = RuleSet::from_ini(&ini).unwrap();
    let id = rs.p_type_id_by_name("GhostRef").unwrap();
    let next = rs.particle_type(id).next_particle.unwrap();
    assert_eq!(rs.particle_type(next).name, "DoesNotExist");
    assert_eq!(
        rs.particle_type(next).behaves_like,
        crate::rules::particle_type::ParticleBehavesLike::Unknown
    );
}

#[test]
fn p_type_id_by_name_is_case_insensitive() {
    let extra = "\
[Particles]
1=GasCloud1

[GasCloud1]
BehavesLike=Gas
";
    let ini = IniFile::from_str(&make_particle_test_rules(extra));
    let rs = RuleSet::from_ini(&ini).unwrap();
    let a = rs.p_type_id_by_name("GasCloud1").unwrap();
    let b = rs.p_type_id_by_name("GASCLOUD1").unwrap();
    let c = rs.p_type_id_by_name("gascloud1").unwrap();
    assert_eq!(a, b);
    assert_eq!(b, c);
}

#[test]
fn combat_damage_defaults_load_from_ini() {
    let extra = "\
[Particles]
1=Fp

[ParticleSystems]
1=FireStreamSys

[FireStreamSys]
BehavesLike=Fire

[Fp]
BehavesLike=Fire

[CombatDamage]
DefaultFireStreamSystem=FireStreamSys
DefaultSparkSystem=SparkSys
";
    let ini = IniFile::from_str(&make_particle_test_rules(extra));
    let rs = RuleSet::from_ini(&ini).unwrap();
    assert_eq!(
        rs.combat_damage.default_fire_stream_system.as_deref(),
        Some("FireStreamSys")
    );
    assert_eq!(
        rs.combat_damage.default_spark_system.as_deref(),
        Some("SparkSys")
    );
    // Other slots stay None when the key isn't present.
    assert!(rs.combat_damage.default_repair_particle_system.is_none());
}

#[test]
fn combat_damage_defaults_when_section_absent() {
    let ini = IniFile::from_str(&make_particle_test_rules(""));
    let rs = RuleSet::from_ini(&ini).unwrap();
    assert!(rs.combat_damage.default_fire_stream_system.is_none());
    assert!(rs.combat_damage.default_spark_system.is_none());
}

#[test]
fn damage_fire_thresholds_use_the_same_authored_double_as_other_health_readers() {
    let ini = IniFile::from_str(
        "[General]\nDamageFireTypes=FIRE01\n[AudioVisual]\nConditionYellow=49%\nConditionRed=13%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("native thresholds are not stock-only");
    let section = ini.section("AudioVisual").unwrap();
    assert_eq!(
        rules.general.condition_yellow,
        section.read_double("ConditionYellow", 0.5)
    );
    assert_eq!(
        rules.general.condition_red,
        section.read_double("ConditionRed", GeneralRules::default().condition_red)
    );
}

#[test]
fn empty_particles_section_leaves_registries_empty() {
    // Pre-existing rules without [Particles]/[ParticleSystems] still parse.
    let ini = IniFile::from_str(&make_particle_test_rules(""));
    let rs = RuleSet::from_ini(&ini).unwrap();
    assert_eq!(rs.particle_type_count(), 0);
    assert_eq!(rs.particle_system_type_count(), 0);
    assert_eq!(rs.p_type_id_by_name("Anything"), None);
    assert_eq!(rs.ps_type_id_by_name("Anything"), None);
}

#[test]
fn paradrop_weapon_rof_reaches_resolved_weapon() {
    // Verifies the Task 5 grounding question: does [ParaDropWeapon] ROF=130
    // flow through the weapon parser into rules.weapon("ParaDropWeapon").rof?
    // The parser only reads weapon sections referenced from an ObjectType's
    // Primary= / Secondary=, so we need a minimal aircraft entry that points
    // to ParaDropWeapon.
    let text = "\
[AircraftTypes]
1=PDPLANE

[PDPLANE]
Primary=ParaDropWeapon
Strength=400
Speed=15
Image=PDPLANE

[ParaDropWeapon]
Damage=60
ROF=130
Range=1
Projectile=Invisible
";
    let ini = IniFile::from_str(text);
    let rs = RuleSet::from_ini(&ini).expect("rules parse");
    let weapon = rs
        .weapon("ParaDropWeapon")
        .expect("ParaDropWeapon must reach the weapon registry");
    assert_eq!(weapon.rof, 130);
}

#[test]
fn merge_art_propagates_add_remove_occupy() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(&make_test_rules()));
    layers.push(
        crate::rules::native_processing::RulesLayerKind::Scenario,
        IniFile::from_str(
            "[BuildingTypes]\n0=GAREFN\n\
             [GAREFN]\nName=Refinery\nCost=2000\nFoundation=4x3\n",
        ),
    );
    let art_text = "[GAREFN]\nFoundation=4x3\nCanHideThings=no\nOccupyHeight=4\nAddOccupy1=-1,0\nAddOccupy2=-1,-1\nRemoveOccupy1=3,1\n";
    let art_ini: IniFile = IniFile::from_str(art_text);
    let mut rules =
        RuleSet::from_processed_rules(&layers.process_with_fixed_art(&art_ini).unwrap())
            .expect("rules parse");
    let art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini);
    rules.install_art_data(art);
    let obj = rules.object("GAREFN").expect("GAREFN");
    assert_eq!(obj.hidden_occupancy.add_occupy[0], Some((-1, 0)));
    assert_eq!(obj.hidden_occupancy.add_occupy[1], Some((-1, -1)));
    assert_eq!(obj.hidden_occupancy.remove_occupy[0], Some((3, 1)));
    assert!(!obj.hidden_occupancy.can_hide_things);
    assert_eq!(obj.hidden_occupancy.occupy_height, 4);
    assert!(!rules.art_registry.can_hide_things("GAREFN"));
    assert_eq!(rules.art_registry.occupy_height("GAREFN"), 4);
}

#[test]
fn native_discharge_retail_gi_binding_retains_independent_zero_defaults() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/infantry_discharge_rules.json",
    ))
    .unwrap();
    let physical = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["case"] == "physical_GI")
        .unwrap();
    let expected: [i32; 4] = serde_json::from_value(physical["after"].clone()).unwrap();
    let Some((retail_rules, retail_art)) = crate::rules::retail_ini_fixture::retail_rules_and_art()
    else {
        return;
    };
    let mut rules = RuleSet::from_processed_rules(
        &RulesLayerStack::new(retail_rules)
            .process_with_fixed_art(&retail_art)
            .unwrap(),
    )
    .expect("retail rules parse");
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&retail_art));
    let infantry = rules.object("E1").expect("retail E1");
    assert_eq!(infantry.image, "GI");
    assert_eq!(
        [
            infantry.fire_up_frame,
            infantry.fire_prone_frame,
            infantry.secondary_fire_frame,
            infantry.secondary_prone_frame,
        ],
        expected,
        "original5246BE..52473A on physical ARTMD.INI[GI]"
    );
}

#[test]
fn native_discharge_signed_art_fields_reach_infantry_type() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/infantry_discharge_rules.json",
    ))
    .unwrap();
    let row = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["case"] == "all_independent")
        .unwrap();
    let expected: [i32; 4] = serde_json::from_value(row["after"].clone()).unwrap();
    let mut layers = RulesLayerStack::new(IniFile::from_str(&make_test_rules()));
    layers.push(
        crate::rules::native_processing::RulesLayerKind::Scenario,
        IniFile::from_str("[E1]\nImage=GI\n"),
    );
    let mut art_text = "[GI]\nFixtureOnly=1\n".to_owned();
    for (key, raw) in row["layers"][0].as_object().unwrap() {
        art_text.push_str(&format!("{key}={}\n", raw.as_str().unwrap()));
    }
    let art_ini = IniFile::from_str(&art_text);
    let mut rules =
        RuleSet::from_processed_rules(&layers.process_with_fixed_art(&art_ini).unwrap())
            .expect("rules parse");
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art_ini));
    let infantry = rules.object("E1").expect("E1");
    assert_eq!(
        [
            infantry.fire_up_frame,
            infantry.fire_prone_frame,
            infantry.secondary_fire_frame,
            infantry.secondary_prone_frame,
        ],
        expected,
        "original independent signed DWORDs survive ART projection"
    );
}

#[test]
fn merge_art_propagates_infantry_crawls_without_building_side_effects() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(&make_test_rules()));
    layers.push(
        crate::rules::native_processing::RulesLayerKind::Scenario,
        IniFile::from_str(
            "[E1]\nName=GI\nImage=GI\nStrength=125\nArmor=flak\nSpeed=4\n\
             [GAPOWR]\nName=Power\nStrength=750\nArmor=wood\nFoundation=2x2\n",
        ),
    );
    let art_ini = IniFile::from_str(
        "[GI]\nCrawls=yes\nFireUp=2\nFireProne=3\nSecondaryFire=4\nSecondaryProne=5\n\n[GAPOWR]\nFoundation=2x2\nCrawls=yes\nFireUp=9\n",
    );
    let mut rules =
        RuleSet::from_processed_rules(&layers.process_with_fixed_art(&art_ini).unwrap())
            .expect("rules parse");
    let art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini);
    rules.install_art_data(art);

    let infantry = rules.object("E1").expect("E1");
    assert!(infantry.crawls);
    assert_eq!(infantry.fire_up_frame, 2);
    assert_eq!(infantry.fire_prone_frame, 3);
    assert_eq!(infantry.secondary_fire_frame, 4);
    assert_eq!(infantry.secondary_prone_frame, 5);
    let building = rules.object("GAPOWR").expect("GAPOWR");
    assert!(!building.crawls);
    assert_eq!(building.fire_up_frame, 0);
    assert_eq!(building.foundation, "2x2");
}

/// An art section without `Crawls=`, or no art section, keeps the
/// constructor's true (`0x00523788`): the read's default is the type's
/// current value (`0x005246A1..0x005246D0`).
#[test]
fn an_art_section_without_crawls_keeps_the_constructor_value() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(&make_test_rules()));
    layers.push(
        crate::rules::native_processing::RulesLayerKind::Scenario,
        IniFile::from_str("[E1]\nName=GI\nImage=GI\nStrength=125\nArmor=flak\nSpeed=4\n"),
    );
    let art_ini = IniFile::from_str("[GI]\nFireUp=2\n");
    let mut rules =
        RuleSet::from_processed_rules(&layers.process_with_fixed_art(&art_ini).unwrap())
            .expect("rules parse");
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art_ini));
    let infantry = rules.object("E1").expect("E1");
    assert!(infantry.crawls);
    assert_eq!(infantry.fire_up_frame, 2, "the GI art section was read");
    assert!(rules.object("E2").expect("E2").crawls, "no art section");
}

/// A later pass whose Image names an art section without the keys keeps
/// what the earlier pass read: each read's default is the type's current
/// value (`0x005246A1..0x0052472D`), not the constructor's.
#[test]
fn a_later_image_without_the_art_keys_keeps_the_earlier_reads() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(&make_test_rules()));
    layers.push(
        crate::rules::native_processing::RulesLayerKind::Scenario,
        IniFile::from_str("[E1]\nImage=GIB\n"),
    );
    let art_ini = IniFile::from_str("[E1]\nCrawls=no\nFireUp=2\n[GIB]\nFireProne=3\n");
    let mut rules =
        RuleSet::from_processed_rules(&layers.process_with_fixed_art(&art_ini).unwrap())
            .expect("rules parse");
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art_ini));
    let infantry = rules.object("E1").expect("E1");
    assert_eq!(infantry.image, "GIB");
    assert!(!infantry.crawls);
    assert_eq!((infantry.fire_up_frame, infantry.fire_prone_frame), (2, 3));
}

#[test]
fn wall_overlay_first_match_order_survives_registry_move() {
    // F04: the overlay registry moved from map to rules. Pin the two
    // orders wall selling depends on: `[OverlayTypes]` declaration order
    // IS the overlay ID, and `first_building_type_for_overlay` returns
    // the FIRST registered building even when a later one also matches.
    let overlay_ini = IniFile::from_str("[OverlayTypes]\n0=GASAND\n1=GAWALL\n2=CAWALL\n");
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&overlay_ini, None);
    assert_eq!(overlays.id_for_name("GAWALL"), Some(1));
    assert_eq!(overlays.id_for_name("CAWALL"), Some(2));

    let rules_ini = IniFile::from_str(
        "[BuildingTypes]\n0=GAWALL2\n1=GAWALL3\n[GAWALL2]\nCost=1\n[GAWALL3]\nCost=1\n",
    );
    let mut rules: RuleSet = RuleSet::from_ini(&rules_ini).expect("rules parse");
    for object in rules.object_list.iter_mut() {
        object.to_overlay = Some("GAWALL".to_string());
    }
    let first = rules
        .first_building_type_for_overlay(1, &overlays)
        .expect("first registered building wins");
    assert_eq!(first.id, "GAWALL2");
    assert!(
        rules
            .first_building_type_for_overlay(2, &overlays)
            .is_none()
    );
}

#[test]
fn resolved_rule_handles_stay_null_without_combat_damage_keys() {
    use crate::sim::intern::StringInterner;
    use crate::sim::type_handle_table::ResolvedRuleHandles;
    let ini: IniFile = IniFile::from_str(&make_test_rules());
    let rules: RuleSet = RuleSet::from_ini(&ini).expect("rules parse");
    let mut interner = StringInterner::default();
    let handles = ResolvedRuleHandles::resolve(&rules, &mut interner);
    // The RulesClass constructor leaves the three warheads null
    // (`0x00666B52`, `0x00666B58`, `0x00666BD5`) and the test rules.ini
    // has no `[CombatDamage]` keys, so no named warhead matches them.
    assert_eq!(interner.resolve(handles.ion_cannon), "");
    assert_eq!(interner.resolve(handles.c4), "");
    assert_eq!(interner.resolve(handles.crush), "");
    assert!(!handles.is_crush(interner.intern("Crush")));
}

#[test]
fn resolved_rule_handles_honor_combat_damage_overrides() {
    use crate::sim::intern::StringInterner;
    use crate::sim::type_handle_table::ResolvedRuleHandles;
    let rules_text = format!(
        "{}\n[CombatDamage]\nIonCannonWarhead=CustomIon\nC4Warhead=CustomC4\nCrushWarhead=CustomCrush\n",
        make_test_rules()
    );
    let ini: IniFile = IniFile::from_str(&rules_text);
    let rules: RuleSet = RuleSet::from_ini(&ini).expect("rules parse");
    let mut interner = StringInterner::default();
    let handles = ResolvedRuleHandles::resolve(&rules, &mut interner);
    assert_eq!(interner.resolve(handles.ion_cannon), "CustomIon");
    assert_eq!(interner.resolve(handles.c4), "CustomC4");
    assert_eq!(interner.resolve(handles.crush), "CustomCrush");
}

#[test]
fn retail_rulesmd_c4_flags_parse_correctly() {
    let ini = IniFile::from_str(
        "[CombatDamage]\nC4Delay=0.03\n\
             [InfantryTypes]\n\
             0=GHOST\n1=TANY\n2=PTROOP\n3=E1\n4=ENGINEER\n5=CCOMAND\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=CAMISC01\n1=CAMISC02\n2=CAMISC06\n3=AMMOCRAT\n\
             4=GAPILE\n5=NAHAND\n6=GAREFN\n\
             [GHOST]\nC4=yes\n\
             [TANY]\nC4=yes\n\
             [PTROOP]\nC4=yes\n\
             [E1]\nFixtureOnly=1\n\
             [ENGINEER]\nFixtureOnly=1\n\
             [CCOMAND]\nFixtureOnly=1\n\
             [CAMISC01]\nCanC4=no\n\
             [CAMISC02]\nCanC4=no\n\
             [CAMISC06]\nCanC4=no\n\
             [AMMOCRAT]\nCanC4=no\n\
             [GAPILE]\nFixtureOnly=1\n\
             [NAHAND]\nFixtureOnly=1\n\
             [GAREFN]\nFixtureOnly=1\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("parse C4 stock-contract fixture");

    // C4-capable units must have c4=true.
    for unit in &["GHOST", "TANY", "PTROOP"] {
        let obj = rules
            .object(unit)
            .unwrap_or_else(|| panic!("no [{}]", unit));
        assert!(obj.c4, "[{}] must have c4=true (C4=yes in INI)", unit);
    }
    // Non-C4 infantry must have c4=false.
    for unit in &["E1", "ENGINEER", "CCOMAND"] {
        if let Some(obj) = rules.object(unit) {
            assert!(!obj.c4, "[{}] must have c4=false", unit);
        }
    }

    // CanC4-opt-out buildings — verified by direct grep of ini/rulesmd.ini
    // for `^CanC4=no`. Four sections match: CAMISC01, CAMISC02, CAMISC06,
    // AMMOCRAT. (The plan originally listed CAMSC09/CAMSC10 in error;
    // the retail INI does not set the flag on either.)
    for bld in &["CAMISC01", "CAMISC02", "CAMISC06", "AMMOCRAT"] {
        let obj = rules.object(bld).unwrap_or_else(|| panic!("no [{}]", bld));
        assert!(
            !obj.can_c4,
            "[{}] must have can_c4=false (CanC4=no in INI)",
            bld
        );
    }
    // Normal buildings inherit can_c4=true.
    for bld in &["GAPILE", "NAHAND", "GAREFN"] {
        if let Some(obj) = rules.object(bld) {
            assert!(obj.can_c4, "[{}] must have can_c4=true (default)", bld);
        }
    }

    // Retail C4Delay=.03 is a 26-frame fuse.
    assert_eq!(rules.c4_delay_frames(), 26);
}

#[test]
fn type_lookups_are_case_insensitive() {
    // Parity: the original engine resolves type names case-insensitively
    // (stricmp-style find-or-allocate). Use a hermetic rules graph and prove every
    // type accessor resolves a stored name regardless of case, to the same entry.
    let ini_text = format!(
        "{}\n[SuperWeaponTypes]\n0=FixtureSW\n[FixtureSW]\nType=MultiMissile\n",
        make_test_rules()
    );
    let ini = IniFile::from_str(&ini_text);
    let rules = RuleSet::from_ini(&ini).expect("parse case-lookup fixture");

    // Concrete, readable anchor from the fixture.
    assert!(rules.object("MTNK").is_some());
    assert!(
        rules.object("mtnk").is_some(),
        "lowercase must resolve (gamemd parity)"
    );
    assert!(rules.object("Mtnk").is_some(), "mixed case must resolve");
    assert_eq!(
        rules.object("mtnk").map(|o| o as *const _),
        rules.object("MTNK").map(|o| o as *const _),
        "all casings resolve to the same object",
    );

    // Property over each of the five type maps: take a real stored key and
    // assert both the upper- and lower-cased forms resolve to the same entry.
    // Toggling case guarantees at least one form differs from the stored key,
    // so the case-fold scan path (not just exact match) is exercised.
    fn check_ci<'a, M, V, S>(
        map: &'a HashMap<String, M, S>,
        lookup: impl Fn(&str) -> Option<&'a V>,
        label: &str,
    ) where
        V: 'a,
    {
        let key = map
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| panic!("{label} map empty"));
        let canon = lookup(&key).map(|v| v as *const V);
        assert!(canon.is_some(), "{label} '{key}' must resolve as stored");
        assert_eq!(
            lookup(&key.to_ascii_lowercase()).map(|v| v as *const V),
            canon,
            "{label} '{key}' lowercase must resolve to same entry"
        );
        assert_eq!(
            lookup(&key.to_ascii_uppercase()).map(|v| v as *const V),
            canon,
            "{label} '{key}' uppercase must resolve to same entry"
        );
    }

    check_ci(&rules.object_index, |k| rules.object(k), "object");
    check_ci(&rules.weapons, |k| rules.weapon(k), "weapon");
    check_ci(&rules.warheads, |k| rules.warhead(k), "warhead");
    check_ci(&rules.projectiles, |k| rules.projectile(k), "projectile");
    check_ci(
        &rules.super_weapons,
        |k| rules.super_weapon(k),
        "super_weapon",
    );
}

#[test]
fn a_weapons_warhead_and_projectile_answer_as_their_names_in_any_rule_set() {
    // GUN names its types in another case than their sections.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=GI\n[GI]\nStrength=100\nPrimary=GUN\n\
             [GUN]\nWarhead=wh\nProjectile=shot\n[WH]\n[SHOT]\n",
    ))
    .unwrap();
    // OTHER's only types take the indexes GUN's handles hold, so a handle
    // read in the wrong set names one of them.
    let other = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=GI\n[GI]\nStrength=100\nPrimary=OTHER\n\
             [OTHER]\nWarhead=AP\nProjectile=Cannon\n[AP]\n[Cannon]\n",
    ))
    .unwrap();
    let copy = rules.clone().with_fresh_serial();
    for owner in [&rules, &copy] {
        let gun = owner.weapon("GUN").unwrap();
        for set in [&rules, &copy] {
            let warhead = set.weapon_warhead(gun).unwrap();
            assert!(std::ptr::eq(warhead, set.warhead("WH").unwrap()));
            let projectile = set.weapon_projectile(gun).unwrap();
            assert!(std::ptr::eq(projectile, set.projectile("SHOT").unwrap()));
        }
        assert!(other.weapon_warhead(gun).is_none());
        assert!(other.weapon_projectile(gun).is_none());
    }
}

#[test]
fn naval_base_rules_preserve_source_order_defaults_and_buildconst_identity() {
    let ini = IniFile::from_str(
        "[General]\n\
             Shipyard=GAYARD,nayard,YAYARD\n\
             BuildConst=WRONG\n\
             AINavalYardAdjacency=-7\n\
             [AI]\nBuildConst=,gacnst,,NACNST,gacnst,none,<NoNe>,YACNST,,\n\
             BuildTech=GAYARD,YAYARD\n\
             [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GAYARD\n1=NAYARD\n2=YAYARD\n3=GACNST\n4=NACNST\n5=YACNST\n6=WRONG\n\
             [GAYARD]\nFoundation=4x4\n\
             [NAYARD]\nFoundation=4x4\nAIBasePlanningSide=1\n\
             [YAYARD]\nFoundation=4x4\nAIBasePlanningSide=-3\n\
             [GACNST]\nFoundation=4x4\n\
             [NACNST]\nFoundation=4x4\n\
             [YACNST]\nFoundation=4x4\n\
             [WRONG]\nFoundation=4x4\n",
    );
    let art = IniFile::from_str(
        "[GAYARD]\nFoundation=4x4\n[NAYARD]\nFoundation=4x4\n\
             [YAYARD]\nFoundation=4x4\n[GACNST]\nFoundation=4x4\n\
             [NACNST]\nFoundation=4x4\n[YACNST]\nFoundation=4x4\n\
             [WRONG]\nFoundation=4x4\n",
    );
    let rules =
        RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("naval base rules and ART");

    assert_eq!(rules.shipyard_types, ["GAYARD", "NAYARD", "YAYARD"]);
    assert_eq!(
        rules.build_const_types,
        ["GACNST", "NACNST", "GACNST", "YACNST"],
        "ReadAI keeps the order and duplicates of the Types it found, without null sentinels"
    );
    assert_eq!(rules.build_tech_types, ["GAYARD", "YAYARD"]);
    assert_eq!(rules.ai_naval_yard_adjacency, -7);
    assert_eq!(rules.object("GAYARD").unwrap().ai_base_planning_side, -1);
    assert_eq!(rules.object("NAYARD").unwrap().ai_base_planning_side, 1);
    assert_eq!(rules.object("YAYARD").unwrap().ai_base_planning_side, -3);
    for id in ["GACNST", "NACNST", "YACNST"] {
        assert!(
            rules
                .object_in_category(ObjectCategory::Building, &id.to_ascii_lowercase())
                .unwrap()
                .build_const_eligible,
            "{id}"
        );
    }
    for id in ["GAYARD", "NAYARD", "YAYARD"] {
        let object = rules.object(id).unwrap();
        assert!(!object.build_const_eligible, "{id}");
        assert_eq!(object.foundation.dimensions(), (4, 4));
    }
    assert!(
        !rules.object("WRONG").unwrap().build_const_eligible,
        "the poisoned General key must not contribute membership"
    );

    let defaults = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n0=YARD\n[YARD]\nFoundation=1x1\n",
        ))
        .expect("constructor defaults");
    assert_eq!(defaults.ai_naval_yard_adjacency, 20);
    assert_eq!(defaults.object("YARD").unwrap().ai_base_planning_side, -1);
    assert!(defaults.build_const_types.is_empty());
    assert!(!defaults.object("YARD").unwrap().build_const_eligible);
}

// Read the stock way: Ares reads these lists Find-only through ParseList
// (Misc/Bugfixes.Buffer.cpp).
#[test]
fn build_const_readai_does_not_trim_individual_tokens() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[AI]\nBuildConst=GACNST, NACNST\n\
             [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n0=GACNST\n1=NACNST\n\
             [GACNST]\nFoundation=1x1\n\
             [NACNST]\nFoundation=1x1\n",
    ))
    .expect("BuildConst whitespace rules");

    // BuildingType FindOrAllocate (`0x004653C0`) compares whole IDs, so
    // " NACNST" allocates its own type rather than aliasing NACNST.
    assert_eq!(rules.build_const_types, ["GACNST", " NACNST"]);
    assert!(rules.object("GACNST").unwrap().build_const_eligible);
    assert!(rules.object(" NACNST").unwrap().build_const_eligible);
    assert!(
        !rules.object("NACNST").unwrap().build_const_eligible,
        "the internally space-prefixed token must not alias registered NACNST"
    );
}

// Read the stock way: Ares reads these lists Find-only through ParseList
// (Misc/Bugfixes.Buffer.cpp).
#[test]
fn build_const_readai_uses_native_127_source_byte_prefix() {
    let oversized = format!("GACNST,{},LATE", "X".repeat(120));
    assert_eq!(oversized.as_bytes()[127], b',');
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[AI]\nBuildConst={oversized}\n\
             [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n0=GACNST\n1=LATE\n\
             [GACNST]\nFoundation=1x1\n\
             [LATE]\nFoundation=1x1\n"
    )))
    .expect("oversized BuildConst rules");
    // The oversized token names the BuildingType its factory allocated,
    // stored as its first 24 bytes (`AbstractType` `0x00410800`).
    let allocated = "X".repeat(24);
    assert_eq!(rules.build_const_types, ["GACNST", allocated.as_str()]);
    assert!(!rules.object("LATE").unwrap().build_const_eligible);

    // `IniFile::from_bytes` widens 0xE9 to U+00E9, whose Rust UTF-8
    // encoding is two bytes. It is nevertheless one native source byte at
    // payload index 126; the comma at source byte 127 and LATE stay outside
    // the copied buffer without a UTF-8 boundary slice or panic.
    let mut bytes = b"[AI]\nBuildConst=GACNST,".to_vec();
    bytes.extend(std::iter::repeat_n(b'X', 119));
    bytes.push(0xE9);
    bytes.extend_from_slice(
        b",LATE\n[InfantryTypes]\n0=DUMMY\n[DUMMY]\nStrength=1\n\
              [BuildingTypes]\n0=GACNST\n1=LATE\n\
              [GACNST]\nFoundation=1x1\n[LATE]\nFoundation=1x1\n",
    );
    let ini = IniFile::from_bytes(&bytes).expect("byte-domain BuildConst rules");
    let stored = ini
        .section("AI")
        .unwrap()
        .get_for_test("BuildConst")
        .unwrap();
    assert_eq!(stored.chars().nth(126), Some(char::from(0xE9)));
    assert_eq!(stored.chars().nth(127), Some(','));
    let rules = RuleSet::from_ini(&ini).expect("byte-domain BuildConst RuleSet");
    assert_eq!(rules.build_const_types, ["GACNST", allocated.as_str()]);
    assert!(!rules.object("LATE").unwrap().build_const_eligible);
}

// Read the stock way: Ares reads these lists Find-only through ParseList
// (Misc/Bugfixes.Buffer.cpp).
#[test]
fn recalc_type_lists_share_native_parser_and_registered_family_resolution() {
    let ini = IniFile::from_str(
        "[General]\n\
             HarvesterUnit=harv,,none,HARV,<none>, AUNIT\n\
             BuildPower=POISON\n\
             [AI]\n\
             BuildConst=con,CON\n\
             BuildPower=pow,none,POW\n\
             BuildRefinery=ref\n\
             BuildBarracks=bar\n\
             BuildTech=tech, TECH\n\
             BuildWeapons=weap\n\
             BuildRadar=rad,<none>,RAD\n\
             NSGates=con,,none,CON\nEWGates=pow,<none>,POW\n\
             [VehicleTypes]\n0=HARV\n1=AUNIT\n\
             [BuildingTypes]\n0=CON\n1=POW\n2=REF\n3=BAR\n4=TECH\n5=WEAP\n6=RAD\n7=POISON\n\
             [HARV]\nStrength=1\n[AUNIT]\nStrength=1\n\
             [CON]\nFoundation=1x1\n[POW]\nFoundation=1x1\n\
             [REF]\nFoundation=1x1\n[BAR]\nFoundation=1x1\n\
             [TECH]\nFoundation=1x1\n[WEAP]\nFoundation=1x1\n\
             [RAD]\nFoundation=1x1\n[POISON]\nFoundation=1x1\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("all Recalc type lists");

    assert_eq!(rules.build_const_types, ["CON", "CON"]);
    assert_eq!(rules.build_power_types, ["POW", "POW"]);
    assert_eq!(rules.build_refinery_types, ["REF"]);
    assert_eq!(rules.build_barracks_types, ["BAR"]);
    assert_eq!(
        rules.build_tech_types,
        ["TECH", " TECH"],
        "the internally space-prefixed token is not trimmed; it allocates its own type"
    );
    assert_eq!(rules.build_weapons_types, ["WEAP"]);
    assert_eq!(rules.build_radar_types, ["RAD", "RAD"]);
    assert_eq!(rules.ns_gate_types, ["CON", "CON"]);
    assert_eq!(rules.ew_gate_types, ["POW", "POW"]);
    assert_eq!(
        rules.harvester_unit_types,
        ["HARV", "HARV", " AUNIT"],
        "sentinels are omitted, duplicates retained, and a space-prefixed Unit token \
             allocates its own UnitType (`0x007480D0`)"
    );
    assert!(rules.object("CON").unwrap().build_const_eligible);
    assert!(!rules.object("POISON").unwrap().build_const_eligible);
}

#[test]
fn recalc_type_lists_preserve_prior_layer_when_later_key_is_missing_or_empty() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[General]\nHarvesterUnit=HARV\n\
             [AI]\nBuildConst=CON\nBuildPower=POW\nBuildTech=TECH\n\
             NSGates=CON\nEWGates=POW\n\
             [VehicleTypes]\n0=HARV\n[HARV]\nStrength=1\n\
             [BuildingTypes]\n0=CON\n1=POW\n2=TECH\n3=NEWPOW\n\
             [CON]\nFoundation=1x1\n[POW]\nFoundation=1x1\n\
             [TECH]\nFoundation=1x1\n[NEWPOW]\nFoundation=1x1\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str(
            "[General]\nHarvesterUnit=\nUnrelated=1\n\
                 [AI]\nBuildConst=\nBuildPower=NEWPOW\nNSGates=\n",
        ),
    );
    let rules = RuleSet::from_rules_layers(&layers).expect("layered Recalc lists");
    assert_eq!(rules.harvester_unit_types, ["HARV"]);
    assert_eq!(rules.build_const_types, ["CON"]);
    assert_eq!(rules.build_power_types, ["NEWPOW"]);
    assert_eq!(rules.build_tech_types, ["TECH"]);
    assert_eq!(rules.ns_gate_types, ["CON"]);
    assert_eq!(rules.ew_gate_types, ["POW"]);
}

#[test]
fn recalc_difficulty_vectors_parse_exact_lengths_and_layer_defaults() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[General]\n\
             AISlaveMinerNumber=4,3\n\
             AIExtraRefineries=2,1,0, -1\n\
             AlliedBaseDefenseCounts=25\n\
             SovietBaseDefenseCounts=25,22,6\n\
             ThirdBaseDefenseCounts=25,22,6\n\
             [BuildingTypes]\n0=DUMMY\n[DUMMY]\nFoundation=1x1\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[General]\nAISlaveMinerNumber=\nAIExtraRefineries=9,,7\nUnrelated=1\n"),
    );
    let rules = RuleSet::from_rules_layers(&layers).expect("difficulty vectors");
    assert_eq!(rules.ai_slave_miner_number, [4, 3]);
    assert_eq!(rules.ai_extra_refineries, [9, 7]);
    assert_eq!(rules.allied_base_defense_counts, [25]);
    assert_eq!(rules.soviet_base_defense_counts, [25, 22, 6]);
    assert_eq!(rules.third_base_defense_counts, [25, 22, 6]);

    let missing = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=DUMMY\n[DUMMY]\nFoundation=1x1\n",
    ))
    .expect("missing vectors");
    assert!(missing.ai_slave_miner_number.is_empty());
    assert!(missing.ai_extra_refineries.is_empty());
    assert!(missing.allied_base_defense_counts.is_empty());
}

#[test]
fn ra2_launch_supplies_native_hardcoded_difficulty_vectors_when_ini_omits_them() {
    // ra2_launch is process-global; serialize against other tests that toggle
    // it and always restore the value we found.

    let ini = "[BuildingTypes]\n0=DUMMY\n[DUMMY]\nFoundation=1x1\n";
    crate::util::ra2_launch::set_override(true);
    let ra2 = RuleSet::from_ini(&IniFile::from_str(ini)).expect("ra2 vectors");
    assert_eq!(ra2.ai_slave_miner_number, [4, 3, 2]);
    assert_eq!(ra2.ai_extra_refineries, [2, 1, 0]);
    assert_eq!(ra2.third_base_defense_counts, [25, 22, 6]);
    // Allied/Soviet counts are authored in RA2 rules.ini; a bare fixture that
    // omits them must stay empty (no fabricated fallback).
    assert!(ra2.allied_base_defense_counts.is_empty());

    // A present value wins over the RA2 fallback.
    let ini_with = "[General]\nAISlaveMinerNumber=7,8\n\
                    [BuildingTypes]\n0=DUMMY\n[DUMMY]\nFoundation=1x1\n";
    let ra2_present = RuleSet::from_ini(&IniFile::from_str(ini_with)).expect("ra2 present");
    assert_eq!(ra2_present.ai_slave_miner_number, [7, 8]);

    crate::util::ra2_launch::set_override(false);
    let yr = RuleSet::from_ini(&IniFile::from_str(ini)).expect("yr vectors");
    assert!(yr.ai_slave_miner_number.is_empty());
    assert!(yr.ai_extra_refineries.is_empty());
    assert!(yr.third_base_defense_counts.is_empty());

    crate::util::ra2_launch::clear_override();
}

/// Slice 8 acceptance: the sim TypeHandleTable resolves every interned type id
/// (no orphans) and does so case-insensitively (htnk vs [HTNK]).
#[test]
fn type_handle_table_completeness_and_casing() {
    let ini = IniFile::from_str(&make_test_rules());
    let rules = RuleSet::from_ini(&ini).expect("fixture parses");
    let mut interner = crate::sim::intern::StringInterner::new();
    // Mirror Simulation::intern_rule_type_ids (the production interning
    // pass) so the table sees every registry id.
    let ids: Vec<_> = rules
        .infantry_ids
        .iter()
        .chain(&rules.vehicle_ids)
        .chain(&rules.aircraft_ids)
        .chain(&rules.building_ids)
        .map(|name| interner.intern(name))
        .collect();
    interner.bind_types(&rules);

    for id in ids {
        assert!(
            interner.type_handle(id, &rules).is_some(),
            "{} has no object type",
            interner.resolve(id),
        );
    }

    // Casing: a lowercased reference resolves to the same handle as the stored
    // uppercase id. The interner is case-insensitive, so the interning pass ("MTNK")
    // and a later get("mtnk") share one id.
    let mtnk_lower = interner
        .get("mtnk")
        .expect("MTNK interned case-insensitively");
    assert_eq!(
        interner.type_handle(mtnk_lower, &rules),
        rules.type_handle("MTNK")
    );
    assert!(
        rules.object("mtnk").is_some(),
        "lowercased object() resolves"
    );
}

/// Helper: parse a (rules.ini, art.ini) pair into a merged RuleSet for
/// pad-merge tests. Keeps a minimal scaffolding (one BuildingType) so
/// `RuleSet::from_ini` does not reject the input.
fn parse_rules_with_art(building_section: &str, art_ini: &str) -> RuleSet {
    let rules_str = format!(
        "[General]\n\
             BuildSpeed=1\n\
             MultipleFactory=1\n\
             LowPowerPenaltyModifier=1\n\
             MinLowPowerProductionSpeed=1\n\
             MaxLowPowerProductionSpeed=1\n\
             [InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GAAIRC\n\
             {}",
        building_section,
    );
    let rules_ini = IniFile::from_str(&rules_str);
    let art_ini_parsed = IniFile::from_str(art_ini);
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini_parsed)
        .expect("rules parse");
    let art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini_parsed);
    rules.install_art_data(art);
    rules
}

#[test]
fn merge_pads_zero_pads_missing_indices() {
    // NumberOfDocks=4 but art only has DockingOffset0,1.
    // Merge must produce pads.len() == 4 with indices 2,3 zero-init.
    let rules = parse_rules_with_art(
        "[GAAIRC]\nName=Airforce\nCost=1000\nStrength=1000\nNumberOfDocks=4\n",
        "[GAAIRC]\n\
             DockingOffset0=0,-128,0\n\
             DockingOffset1=0,128,0\n",
    );
    let obj = rules.object("GAAIRC").expect("obj");
    assert_eq!(obj.pads.len(), 4, "pads sized to NumberOfDocks");
    assert_eq!(obj.pads[0].lepton_offset, (0, -128, 0));
    assert_eq!(obj.pads[1].lepton_offset, (0, 128, 0));
    assert_eq!(
        obj.pads[2].lepton_offset,
        (0, 0, 0),
        "missing index 2 zero-init"
    );
    assert_eq!(
        obj.pads[3].lepton_offset,
        (0, 0, 0),
        "missing index 3 zero-init"
    );
}

#[test]
fn merge_pads_truncates_excess_offsets() {
    // NumberOfDocks=2 but art has 4 offsets. Truncate.
    let rules = parse_rules_with_art(
        "[GAAIRC]\nName=Airforce\nCost=1000\nStrength=1000\nNumberOfDocks=2\n",
        "[GAAIRC]\n\
             DockingOffset0=0,0,0\n\
             DockingOffset1=128,0,0\n\
             DockingOffset2=256,0,0\n\
             DockingOffset3=384,0,0\n",
    );
    let obj = rules.object("GAAIRC").expect("obj");
    assert_eq!(obj.pads.len(), 2, "truncated to NumberOfDocks=2");
    assert_eq!(obj.pads[0].lepton_offset, (0, 0, 0));
    assert_eq!(obj.pads[1].lepton_offset, (128, 0, 0));
}

#[test]
fn a_huge_dock_count_stops_at_the_contact_bound() {
    let rules = parse_rules_with_art(
        "[GAAIRC]\nName=Airforce\nCost=1000\nStrength=1000\nNumberOfDocks=100000\n",
        "[GAAIRC]\nDockingOffset0=0,-128,0\n",
    );
    let obj = rules.object("GAAIRC").expect("obj");
    let limit = crate::rules::object_type::DOCK_CONTACT_LIMIT;
    assert_eq!(obj.pads.len(), limit as usize);
    assert_eq!(obj.dock_contact_capacity(), limit as u32);
}

#[test]
fn out_of_range_rules_numbers_saturate_instead_of_stopping_the_game() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nTunnelSpeed=1e999\nShipSinkingWeight=-1e999\n\
             [Radiation]\nRadLightFactor=1e999\n\
             [AudioVisual]\nDirectRockingCoefficient=1e999\n\
             [VehicleTypes]\n0=TANK\n\
             [TANK]\nWeight=1e999\nPitchAngle=1e999\nJumpjetSpeed=40000\n\
             [VoxelAnims]\n0=PIECE\n[PIECE]\nMinAngularVelocity=1e999\n",
    ))
    .unwrap();
    assert_eq!(rules.general.tunnel_speed, SimFixed::MAX);
    assert_eq!(rules.general.ship_sinking_weight, SimFixed::MIN);
    assert_eq!(rules.radiation.light_factor, SimFixed::MAX);
    let tank = rules.object("TANK").unwrap();
    assert_eq!(tank.weight, SimFixed::MAX);
    assert_eq!(tank.jumpjet_params.speed, SimFixed::MAX);
    // The infinite spin keeps the constructor value.
    let piece = rules.voxel_anim_type(rules.voxel_anim_type_id_by_name("PIECE").unwrap());
    let default = crate::rules::voxel_anim_type::VoxelAnimType::with_defaults("PIECE");
    assert_eq!(piece.min_angular_velocity, default.min_angular_velocity);
}

#[test]
fn a_huge_debris_count_stops_at_the_debris_bound() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=TANK\n[TANK]\nMaxDebris=2147483647\nMinDebris=2147483647\n\
             [Warheads]\n0=WH\n[WH]\nMaxDebris=2147483647\n",
    ))
    .unwrap();
    let limit = crate::rules::object_type::DEBRIS_LIMIT;
    let tank = rules.object("TANK").unwrap();
    assert_eq!((tank.min_debris, tank.max_debris), (limit, limit));
    assert_eq!(rules.warhead("WH").unwrap().max_debris, limit);
}

#[test]
fn a_huge_ai_base_spacing_stops_at_the_spacing_bound() {
    let spacing = |value: &str| {
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[AI]\nAIBaseSpacing={value}\n[BuildingTypes]\n0=PLAIN\n[PLAIN]\nName=Plain\n"
        )))
        .unwrap();
        let plain = rules.object("PLAIN").unwrap().base_reservation_spacing;
        (rules.ai_base_spacing, plain)
    };
    let limit = AI_BASE_SPACING_LIMIT;
    assert_eq!(spacing("2147483647"), (limit, Some(limit)));
    assert_eq!(spacing("-2147483647"), (-limit, Some(-limit)));
}

#[test]
fn a_huge_spawn_count_stops_at_the_spawn_bound() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[AircraftTypes]\n0=HORNET\n[VehicleTypes]\n0=CARRIER\n\
             [HORNET]\nStrength=100\n[CARRIER]\nSpawns=HORNET\nSpawnsNumber=2147483647\n",
    ))
    .unwrap();
    let limit = crate::rules::object_type::SPAWNS_NUMBER_LIMIT;
    assert_eq!(rules.object("CARRIER").unwrap().spawns_number, limit);
}

#[test]
fn a_huge_slave_count_stops_at_the_slave_bound() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=SLAV\n[VehicleTypes]\n0=SMIN\n\
             [SLAV]\nStrength=100\n[SMIN]\nEnslaves=SLAV\nSlavesNumber=2147483647\n",
    ))
    .unwrap();
    let limit = crate::rules::object_type::SLAVES_NUMBER_LIMIT;
    assert_eq!(rules.object("SMIN").unwrap().slaves_number, limit);
}

#[test]
fn sides_past_one_byte_are_not_allocated() {
    let sides: String = (0..257).map(|i| format!("S{i}=Americans\n")).collect();
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!("[Sides]\n{sides}"))).unwrap();
    assert_eq!(rules.allocated_sides().count(), 256);
    assert!(rules.side_index("S256").is_none());
}

#[test]
fn gsi_04_05_reservation_ai_base_spacing_default_signed_and_writer_gates() {
    let default_rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str("[BuildingTypes]\n0=PLAIN\n[PLAIN]\nName=Plain\n"),
        &IniFile::from_str("[PLAIN]\nFoundation=2x2\n"),
    )
    .expect("default AI spacing rules");
    assert_eq!(default_rules.ai_base_spacing, 1);
    assert_eq!(
        default_rules
            .object("PLAIN")
            .unwrap()
            .base_reservation_spacing,
        Some(1)
    );

    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(
            "[AI]\nAIBaseSpacing=-3\n\
             [BuildingTypes]\n\
             0=PLAIN\n1=GATHER\n2=UNDEPLOY\n3=UNDEPLOYGATHER\n4=UNDEPLOYONE\n\
             [PLAIN]\nFoundation=2x2\n\
             [GATHER]\nFoundation=2x2\nResourceGatherer=yes\n\
             [UNDEPLOY]\nFoundation=2x2\nUndeploysInto=MCV\n\
             [UNDEPLOYGATHER]\nFoundation=2x2\nUndeploysInto=MCV\nResourceGatherer=yes\n\
             [UNDEPLOYONE]\nFoundation=1x1\nUndeploysInto=MCV\n",
        ),
        &IniFile::from_str(
            "[PLAIN]\nFoundation=2x2\n[GATHER]\nFoundation=2x2\n\
             [UNDEPLOY]\nFoundation=2x2\n[UNDEPLOYGATHER]\nFoundation=2x2\n\
             [UNDEPLOYONE]\nFoundation=1x1\n",
        ),
    )
    .expect("signed AI spacing rules");
    assert_eq!(rules.ai_base_spacing, -3);
    assert_eq!(
        rules.object("PLAIN").unwrap().base_reservation_spacing,
        Some(-3)
    );
    assert_eq!(
        rules.object("GATHER").unwrap().base_reservation_spacing,
        Some(-3)
    );
    assert_eq!(
        rules.object("UNDEPLOY").unwrap().base_reservation_spacing,
        Some(-3)
    );
    assert_eq!(
        rules
            .object("UNDEPLOYGATHER")
            .unwrap()
            .base_reservation_spacing,
        None
    );
    assert_eq!(
        rules
            .object("UNDEPLOYONE")
            .unwrap()
            .base_reservation_spacing,
        None
    );
}

#[test]
fn gsi_04_11_infantry_death_anim_bindings_use_general_and_animation_order() {
    let ini = IniFile::from_str(
        "[General]\n\
             InfantryExplode=EX3\n\
             FlamingInfantry=EX4\n\
             InfantryHeadPop=EX6\n\
             InfantryNuked=EX7\n\
             InfantryVirus=EX8\n\
             InfantryMutate=EX9\n\
             InfantryBrute=EX10\n\
             [Animations]\n\
             99=FIRST_DECLARED\n\
             2=SECOND_DECLARED\n",
    );
    let parsed = RuleSet::from_ini(&ini).unwrap().general;
    assert_eq!(parsed.infantry_death_anim(0), None);
    assert_eq!(parsed.infantry_death_anim(1), None);
    assert_eq!(parsed.infantry_death_anim(2), None);
    assert_eq!(parsed.infantry_death_anim(3), Some("EX3"));
    assert_eq!(parsed.infantry_death_anim(4), Some("EX4"));
    // Slot 5 is the second declared Animation, which no General key names.
    assert_eq!(
        GeneralRules::from_ini(&ini).infantry_death_anim(5),
        Some("SECOND_DECLARED")
    );
    assert_eq!(parsed.infantry_death_anim(6), Some("EX6"));
    assert_eq!(parsed.infantry_death_anim(7), Some("EX7"));
    assert_eq!(parsed.infantry_death_anim(8), Some("EX8"));
    assert_eq!(parsed.infantry_death_anim(9), Some("EX9"));
    assert_eq!(parsed.infantry_death_anim(10), Some("EX10"));

    let defaults = GeneralRules::default();
    assert_eq!(defaults.infantry_death_anim(3), None);
    assert_eq!(defaults.infantry_death_anim(5), Some("ELECTRO"));
    assert_eq!(defaults.infantry_death_anim(10), None);
}

/// Original-RA2 `[AI]` ore-scan radii parse to the authored values; YR data
/// has no such keys and keeps the native constructor default of 0.
#[test]
fn ra2_tiberium_scan_radii_parse_from_the_ai_section() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[AI]\nTiberiumNearScan=6\nTiberiumFarScan=48\n\
         [BuildingTypes]\n0=DUMMY\n[DUMMY]\nFoundation=1x1\n",
    ))
    .expect("scan radii");
    assert_eq!(rules.tiberium_near_scan, 6);
    assert_eq!(rules.tiberium_far_scan, 48);

    let missing = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=DUMMY\n[DUMMY]\nFoundation=1x1\n",
    ))
    .expect("missing scan radii");
    assert_eq!(missing.tiberium_near_scan, 0);
    assert_eq!(missing.tiberium_far_scan, 0);
}
