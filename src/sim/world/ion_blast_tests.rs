//! UpdateAll against `tools/superweapon_oracle.json` `ion_blast_update` (the
//! original `0x0053D310` run natively over flagged blasts).

use super::*;
use serde_json::Value;

/// Each row's blasts, in vector order, update once: the ones left keep their
/// order, their frames advance, and a frame of 79 or more is removed.
#[test]
fn update_all_matches_native() {
    let oracle: Value =
        serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json")).unwrap();
    let rows = oracle["ion_blast_update"].as_array().unwrap();
    assert_eq!(rows.len(), 6);
    for row in rows {
        let mut sim = Simulation::new();
        // The X coordinate carries each blast's index in the row.
        for (index, frame) in row["frames"].as_array().unwrap().iter().enumerate() {
            sim.ion_blasts.push(IonBlast {
                coord: [index as i32, 0, 0],
                frame: frame.as_i64().unwrap() as i32,
                skip_forces: true,
            });
        }
        sim.update_ion_blasts(
            &RuleSet::from_ini(&crate::rules::ini_parser::IniFile::empty()).unwrap(),
            None,
            FrameEffects::default(),
        );
        let left: Vec<[i64; 2]> = row["left"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pair| [pair[0].as_i64().unwrap(), pair[1].as_i64().unwrap()])
            .collect();
        let ours: Vec<[i64; 2]> = sim
            .ion_blasts()
            .map(|(coord, frame)| [i64::from(coord[0]), i64::from(frame)])
            .collect();
        assert_eq!(ours, left, "{row}");
    }
}

use crate::map::{map_file::MapFile, waypoints::Waypoint};
use crate::rules::ini_parser::IniFile;
use crate::sim::superweapon::chronosphere_tests::{place, world_with};
use std::collections::HashMap;

fn rules() -> RuleSet {
    let art = IniFile::from_str(
        "[RING1]\nRate=1\nEnd=8\n[SPLASH0]\nRate=1\nEnd=8\n[SPLASH1]\nRate=1\nEnd=8\n[IONBEAM]\nRate=1\nEnd=8\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(
        "[General]\nIonBlast=RING1\nIonBeam=IONBEAM\n\
         [CombatDamage]\nIonCannonDamage=10\nIonCannonWarhead=ION\nSplashList=SPLASH0,SPLASH1\n\
         [Warheads]\n0=ION\n[ION]\nCellSpread=0.5\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [VehicleTypes]\n0=TANK\n[TANK]\nStrength=5000\nArmor=none\nSpeed=4\nCrewed=no\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
         [InfantryTypes]\n0=MAN\n[MAN]\nStrength=5000\nSpeed=4\nCrewed=no\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n"
    ), &art).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    for name in ["RING1", "IONBEAM", "SPLASH0", "SPLASH1"] {
        rules.bind_anim_frame_count_for_test(name, 32);
    }
    rules
}

fn waypoint(sim: &mut Simulation, cell: (i16, i16)) {
    sim.session.install_waypoints(&HashMap::from([(
        1,
        Waypoint {
            index: 1,
            rx: cell.0 as u16,
            ry: cell.1 as u16,
        },
    )]));
}

#[test]
fn counted_action94_uses_ground_then_real_frame_damage_and_cleanup() {
    let (rules, mut sim, _) = world_with(rules(), 64, &[((20, 20), 2)]);
    let victim = place(&mut sim, &rules, "TANK", (20, 20), 2, false, false);
    waypoint(&mut sim, (20, 20));
    let terrain = sim.resolved_terrain.as_mut().unwrap();
    let cell = terrain.native_cell_identity((20, 20));
    terrain.write_native_cell_flags(cell, 0x100);
    let before = sim.entities().get(victim).unwrap().health.current();
    let map = MapFile::from_bytes(
        b"[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
        [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n[Basic]\nNewINIFormat=4\n\
        [Triggers]\nT=Neutral,<none>,Ion,0,1,1,1,0\n[Events]\nT=1,13,0,0\n\
        [Actions]\nT=2,94,0,999,0,0,0,0,B,56,0,0,0,0,0,0,A\n\
        [Tags]\nTAG=0,Ion,T\n[VariableNames]\n0=Finished,0\n",
    )
    .unwrap();
    sim.initialize_trigger_definitions(
        &map,
        &crate::rules::team_ai_ini::TeamAiIniRegistry::default(),
        None,
        None,
    );
    sim.classify_map_triggers(crate::rules::ruleset::RuleSet::empty_for_test());
    sim.session.publish_started();
    sim.advance_triggers(&rules, None, FrameEffects::default());
    assert!(sim.trigger_local_is_set_for_test(0));
    assert_eq!(
        sim.ion_blasts().collect::<Vec<_>>(),
        [([5248, 5248, 208], 0)]
    );
    assert_eq!(sim.entities().get(victim).unwrap().health.current(), before);
    let (_, areas) = crate::sim::combat::receiver_fixture::trace_area_visits(|| {
        sim.advance_tick(&[], Some(&rules), None, None, 33);
    });
    assert_eq!(
        areas.iter().map(|v| v["coord"].clone()).collect::<Vec<_>>(),
        [
            serde_json::json!([5248, 5248, 624]),
            serde_json::json!([5248, 5248, 208])
        ]
    );
    assert!(
        areas
            .iter()
            .all(|v| v["damage"] == 10 && v["no_source"] == true && v["no_source_house"] == true)
    );
    assert!(sim.entities().get(victim).unwrap().health.current() < before);
    let anims: Vec<_> = sim
        .anims()
        .map(|(_, a)| {
            (
                sim.interner.resolve(a.type_id()).to_string(),
                [a.world_coord().x, a.world_coord().y, a.world_coord().z],
                a.draw_flags(),
                a.z_adjust(),
            )
        })
        .collect();
    assert_eq!(
        anims,
        [
            ("RING1".into(), [5248, 5248, 213], 0x600, 0),
            ("IONBEAM".into(), [5248, 5248, 213], 0x600, 0)
        ]
    );
    assert_eq!(sim.ion_blasts[0].frame, 1);
    sim.ion_blasts[0].frame = 79;
    sim.advance_tick(&[], Some(&rules), None, None, 33);
    assert_eq!(sim.ion_blasts().count(), 0);
}

#[test]
fn water_selects_last_splash_and_only_raw100_adds_a_deck_area() {
    for (land, flags, expected_anim, area_count) in [
        (2, 0, "SPLASH1", 1),
        (0, 0x400, "RING1", 1),
        (0, 0x100, "RING1", 2),
    ] {
        let (rules, mut sim, _) = world_with(rules(), 64, &[]);
        waypoint(&mut sim, (20, 20));
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        terrain.cell_mut_for_test(20, 20).unwrap().land_type = land;
        let cell = terrain.native_cell_identity((20, 20));
        terrain.write_native_cell_flags(cell, flags);
        assert!(sim.trigger_ion_blast(1));
        let (_, areas) = crate::sim::combat::receiver_fixture::trace_area_visits(|| {
            sim.update_ion_blasts(&rules, None, FrameEffects::default())
        });
        assert_eq!(areas.len(), area_count);
        let names: Vec<_> = sim
            .anims()
            .map(|(_, a)| sim.interner.resolve(a.type_id()))
            .collect();
        assert_eq!(names, [expected_anim, "IONBEAM"]);
    }
}

#[test]
fn positive_ripple_stops_and_lifts_shp_foot_before_the_voxel_gate() {
    let (rules, mut sim, _) = world_with(rules(), 64, &[]);
    let ids = [
        place(&mut sim, &rules, "TANK", (21, 20), 0, false, false),
        place(&mut sim, &rules, "MAN", (21, 20), 0, false, false),
    ];
    let sample = RippleFrames::shared().force_sample(7, 41);
    assert_eq!(sample, 15, "checked native-generated frame7 midline byte");
    for id in ids {
        let e = sim.substrate.entities.get_mut(id).unwrap();
        e.foot_speed
            .set_speed_fraction(crate::util::fixed_math::SIM_ONE);
        assert!(e.rocking.is_none());
    }
    sim.apply_ion_blast_forces(
        IonBlast {
            coord: [5248, 5248, 0],
            frame: 7,
            skip_forces: false,
        },
        &rules,
    );
    for id in ids {
        let e = sim.entities().get(id).unwrap();
        assert_eq!(
            e.foot_speed.applied_fraction(),
            crate::util::fixed_math::SIM_ZERO
        );
        assert_eq!(e.ion_blast_lift_pixels(), 4);
        assert!(e.rocking.is_none(), "the custom types have no loaded VXL");
    }
    sim.advance_tick(&[], Some(&rules), None, None, 33);
    assert!(
        ids.into_iter()
            .all(|id| sim.entities().get(id).unwrap().ion_blast_lift_pixels() == 0)
    );
}

#[test]
fn retail_loaded_tank_uses_the_shared_numeric_kernel_and_frame_table() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let mut rules = retail.rules;
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
    let roots = crate::rules::effect_asset_catalog::anim_class_roots(&rules);
    let ion_roots = vec![
        rules.general.ion_blast_anim.clone(),
        rules.general.ion_beam_anim.clone(),
    ];
    assert!(ion_roots.iter().all(|name| roots.contains(name)));
    // Exercise the production binder, including its native constructor-only
    // type admission. Successful binding need not imply a physical SHP.
    rules
        .bind_scheduler_anim_assets(&ion_roots, &assets, "TEM", "TEMPERATE")
        .expect("retail IonBlast/IonBeam roots bind through the production owner");
    rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
    rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
    assert_eq!(rules.general.ion_cannon_damage, 751);
    assert_eq!(rules.general.ion_blast_anim, "RING1");
    assert_eq!(rules.general.ion_beam_anim, "IONBEAM");
    assert_eq!(rules.bridge_warheads.ion_cannon_name, "IonCannonWH");
    assert!(rules.object_has_loaded_voxel(rules.type_handle("MTNK").unwrap()));
    let (rules, mut sim, _) = world_with(rules, 64, &[]);
    let id = place(&mut sim, &rules, "MTNK", (21, 20), 0, false, false);
    assert_eq!(
        sim.entities()
            .get(id)
            .unwrap()
            .body_facing_current(sim.session.binary_frame()),
        0
    );
    sim.apply_ion_blast_forces(
        IonBlast {
            coord: [5248, 5248, 0],
            frame: 7,
            skip_forces: false,
        },
        &rules,
    );
    let corpus: Vec<Value> =
        serde_json::from_str(crate::test_fixture::text("tools/ion_blast_forces.json")).unwrap();
    let native = corpus.iter().find(|r| r["input"]["frame"] == 7).unwrap();
    let e = sim.entities().get(id).unwrap();
    let expected: Vec<_> = native["angles"].as_array().unwrap()[..2]
        .iter()
        .map(|v| v.as_u64().unwrap() as u32)
        .collect();
    assert_eq!(&e.rocking.as_ref().unwrap().native_bits()[..2], expected);
    assert_eq!(
        e.ion_blast_lift_pixels(),
        native["lift"].as_i64().unwrap() as i8
    );
}

#[test]
fn pending_damaging_blasts_survive_save_and_hash_while_visual_blasts_do_not() {
    // deserialize_scenario_rng_reset intentionally reseeds to0 on native load.
    // Start this unrelated persistence comparison on the same cursor.
    let mut sim = Simulation::with_seed(0);
    let default_hash = sim.state_hash();
    sim.add_dominator_ion_blast([100, 200, 0]);
    assert_eq!(sim.state_hash(), default_hash);
    sim.ion_blasts.push(IonBlast {
        coord: [128, 128, 0],
        frame: 8,
        skip_forces: false,
    });
    let hash = sim.state_hash();
    assert_ne!(hash, default_hash);
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(restored.state_hash(), hash);
    assert_eq!(
        restored.ion_blasts().collect::<Vec<_>>(),
        [([128, 128, 0], 8)]
    );
    let rules = rules();
    sim.update_ion_blasts(&rules, None, FrameEffects::default());
    restored.update_ion_blasts(&rules, None, FrameEffects::default());
    assert_eq!(sim.state_hash(), restored.state_hash());
    let mut entity =
        crate::sim::game_entity::GameEntity::test_default(1, "TANK", "Americans", 20, 20);
    entity.lift_by_ion_blast(7);
    let restored: crate::sim::game_entity::GameEntity =
        crate::util::save_codec::deserialize(&crate::util::save_codec::serialize(&entity).unwrap())
            .unwrap();
    assert_eq!(restored.ion_blast_lift_pixels(), 7);
}

#[test]
fn ion_rules_use_current_defaults_case_and_registry_handoff() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    let fresh = RuleSet::from_ini(&IniFile::empty()).unwrap();
    assert_eq!(fresh.general.ion_cannon_damage, 700);
    assert!(
        fresh.general.ion_blast_anim.is_empty()
            && fresh.general.ion_beam_anim.is_empty()
            && fresh.bridge_warheads.ion_cannon_name.is_empty()
    );
    let invalid =
        RuleSet::from_ini(&IniFile::from_str("[CombatDamage]\nIonCannonDamage=?\n")).unwrap();
    assert_eq!(
        invalid.general.ion_cannon_damage, 0,
        "present nonnumeric ReadInt uses C atoi; it does not keep the default"
    );
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[General]\nIonBlast=RING\nIonBeam=BEAM\n[CombatDamage]\nIonCannonDamage=-7\nIonCannonWarhead=WH\n",
    ));
    layers.push(RulesLayerKind::Scenario,IniFile::from_str("[General]\nIonBeam=\n[CombatDamage]\nIonCannonDamage=\nioncannondamage=12\nIonCannonWarhead=\n"));
    let processed = layers.process().unwrap();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    assert_eq!(
        (
            rules.general.ion_cannon_damage,
            rules.general.ion_blast_anim.as_str(),
            rules.general.ion_beam_anim.as_str(),
            rules.bridge_warheads.ion_cannon_name.as_str()
        ),
        (-7, "RING", "BEAM", "WH")
    );
    let (_, trace) = processed.into_ini_and_native_type_construction_trace();
    let state = trace.into_registry_state_discarding_events();
    let bytes = crate::util::save_codec::serialize(&state).unwrap();
    let reset_state: crate::rules::native_processing::NativeRulesRegistryState =
        crate::util::save_codec::deserialize(&bytes).unwrap();
    let reset = RulesLayerStack::new(IniFile::empty())
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            reset_state.destructive_reset(),
        )
        .unwrap();
    let reset = RuleSet::from_processed_rules(&reset).unwrap();
    assert_eq!(reset.general.ion_cannon_damage, -7);
    assert!(
        reset.general.ion_blast_anim.is_empty()
            && reset.general.ion_beam_anim.is_empty()
            && reset.bridge_warheads.ion_cannon_name.is_empty(),
        "the existing Type deletion policy clears retired reference bindings"
    );
    let state = crate::util::save_codec::deserialize(&bytes).unwrap();
    let next = RulesLayerStack::new(IniFile::from_str(
        "[General]\nIonBeam=none\n[CombatDamage]\nFixture=1\n",
    ))
    .process_with_fixed_art_and_registry_state(&IniFile::empty(), state)
    .unwrap();
    let next = RuleSet::from_processed_rules(&next).unwrap();
    assert_eq!(next.general.ion_cannon_damage, -7);
    assert_eq!(next.general.ion_blast_anim, "RING");
    assert!(next.general.ion_beam_anim.is_empty());
    assert_eq!(next.bridge_warheads.ion_cannon_name, "WH");
    let next_hash = next.simulation_config_hash();
    let fresh_same_text = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nIonBeam=none\n[CombatDamage]\nFixture=1\n",
    ))
    .unwrap();
    assert_ne!(next_hash, fresh_same_text.simulation_config_hash());
}
