use super::*;
#[cfg(feature = "phobos")]
use crate::rules::ini_parser::IniFile;
use crate::sim::command::Command;
use crate::sim::world::{FrameEffects, TickLane};

fn native_superweapon_range_controls() -> serde_json::Value {
    serde_json::from_str::<serde_json::Value>(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/radial_indicator.json",
    ))
    .unwrap()["superweapon_range"]
        .clone()
}

fn loop_control(name: &str) -> serde_json::Value {
    native_superweapon_range_controls()["loop_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|control| control["name"] == name)
        .unwrap()
        .clone()
}

fn bits_from_control(values: &[serde_json::Value]) -> Vec<u32> {
    values
        .iter()
        .map(|value| u32::from_str_radix(value.as_str().unwrap(), 16).unwrap())
        .collect()
}

fn state(rules: &RuleSet) -> Simulation {
    let mut sim = Simulation::with_seed(1);
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    for name in ["Americans", "British"] {
        let owner = sim.intern(name);
        sim.houses_mut_for_test()
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, true, 0, 10,
            ));
    }
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let house = sim.interner().get("Americans");
    sim.session_mut_for_test().set_current_house_for_test(house);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim
}

fn query(sim: &Simulation, rules: &RuleSet, ids: &[u64], ms: u64) -> Vec<RadialIndicatorDraw> {
    let before = (
        sim.state_hash(),
        sim.main_rng().native_state_hex(),
        sim.scenario_rng().native_state_hex(),
        sim.mapgen_rng().native_state_hex(),
    );
    let mut pulse = SuperweaponRangePulse::default();
    let result = build(
        Some(sim),
        Some(rules),
        ids,
        None,
        None,
        None,
        false,
        &mut pulse,
        &HouseColorMap::new(),
        SurfaceLineViewport {
            camera: [0; 2],
            clip: [0, 0, 1024, 768],
            z_origin_y: 0,
            zoom: 1.,
        },
        ms,
    );
    assert_eq!(
        before,
        (
            sim.state_hash(),
            sim.main_rng().native_state_hex(),
            sim.scenario_rng().native_state_hex(),
            sim.mapgen_rng().native_state_hex()
        )
    );
    result
}

fn target_query(
    sim: &Simulation,
    rules: &RuleSet,
    armed_super_weapon: Option<&str>,
    pulse: &mut SuperweaponRangePulse,
) -> Vec<RadialIndicatorDraw> {
    let colors = HouseColorMap::from([(
        "Americans".to_owned(),
        crate::rules::house_colors::HouseColorIndex(3),
    )]);
    build(
        Some(sim),
        Some(rules),
        &[],
        armed_super_weapon,
        None,
        Some((16, 16)),
        false,
        pulse,
        &colors,
        SurfaceLineViewport {
            camera: [0; 2],
            clip: [0, 0, 1024, 768],
            z_origin_y: 0,
            zoom: 1.0,
        },
        0,
    )
}

#[test]
fn selected_retail_superweapon_targeting_emits_native_range_loop() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    let nuke = rules.super_weapon("NukeSpecial").unwrap();
    assert_eq!(nuke.range, 7.0);
    assert_eq!(nuke.line_multiplier, 2);
    let sim = state(rules);
    let control = loop_control("nuke_special_retail_7_2");
    let first_draw = control["pulse_draws"][0].clone();
    let mut pulse = SuperweaponRangePulse::default();
    assert!(target_query(&sim, rules, None, &mut pulse).is_empty());
    let draws = target_query(&sim, rules, Some("NukeSpecial"), &mut pulse);
    let radii: Vec<_> = draws.iter().map(|draw| draw.radius.to_bits()).collect();
    let expected_radii = bits_from_control(control["radii_f32_bits"].as_array().unwrap());
    assert_eq!(radii, expected_radii, "native loop stores i/2 as f32");
    assert!(
        draws
            .iter()
            .all(|draw| { !draw.truncate_radius && !draw.pixel_radius && draw.fade && !draw.rays })
    );
    let base = crate::render::palette_light::house_color_rgb(
        &rules.house_color_ramps,
        crate::rules::house_colors::HouseColorIndex(3),
    );
    let native_factors: Vec<u32> = first_draw["caller_factors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|factor| factor.as_u64().unwrap() as u32)
        .collect();
    let expected_rgb: Vec<_> = native_factors
        .iter()
        .map(|&factor| crate::render::native_surface_format::interpolate_rgb8(base, [0; 3], factor))
        .collect();
    assert_eq!(
        draws.iter().map(|draw| draw.rgb).collect::<Vec<_>>(),
        expected_rgb,
        "each Nuke ring uses its native local pulse factor from House RGB"
    );
    assert_eq!(
        pulse,
        SuperweaponRangePulse {
            phase: 10,
            direction: 1
        }
    );
    assert_eq!(
        [pulse.phase, pulse.direction],
        [
            first_draw["phase_direction_after"][0].as_i64().unwrap() as i32,
            first_draw["phase_direction_after"][1].as_i64().unwrap() as i32,
        ]
    );

    // The native caller input is [33,67,101]; these literal output triples
    // pin the shared 0x6612C0 interpolation behavior independently of the
    // retail ramp selected by this fixture.
    let native_rgb: Vec<[u8; 3]> = first_draw["caller_rgb_after_adjust"]
        .as_array()
        .unwrap()
        .iter()
        .map(|rgb| {
            let channels = rgb.as_array().unwrap();
            [
                channels[0].as_u64().unwrap() as u8,
                channels[1].as_u64().unwrap() as u8,
                channels[2].as_u64().unwrap() as u8,
            ]
        })
        .collect();
    let native_source = [33, 67, 101];
    let native_rgb_from_shared_owner: Vec<_> = native_factors
        .iter()
        .map(|&factor| {
            crate::render::native_surface_format::interpolate_rgb8(native_source, [0; 3], factor)
        })
        .collect();
    assert_eq!(native_rgb_from_shared_owner, native_rgb);

    // An unarmed frame freezes the process-lived pulse. A later active draw
    // advances the outer phase once and restarts the ring loop from that state.
    assert!(target_query(&sim, rules, None, &mut pulse).is_empty());
    assert_eq!(
        pulse,
        SuperweaponRangePulse {
            phase: 10,
            direction: 1
        }
    );
    let next = target_query(&sim, rules, Some("NukeSpecial"), &mut pulse);
    assert_eq!(
        pulse,
        SuperweaponRangePulse {
            phase: 20,
            direction: 1
        }
    );
    let second_draw = control["pulse_draws"][1].clone();
    let second_factors: Vec<u32> = second_draw["caller_factors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|factor| factor.as_u64().unwrap() as u32)
        .collect();
    let expected_second_rgb: Vec<_> = second_factors
        .iter()
        .map(|&factor| crate::render::native_surface_format::interpolate_rgb8(base, [0; 3], factor))
        .collect();
    assert_eq!(
        next.iter().map(|draw| draw.rgb).collect::<Vec<_>>(),
        expected_second_rgb
    );
    assert_eq!(
        [pulse.phase, pulse.direction],
        [
            second_draw["phase_direction_after"][0].as_i64().unwrap() as i32,
            second_draw["phase_direction_after"][1].as_i64().unwrap() as i32,
        ]
    );
}

#[test]
fn superweapon_range_ring_requires_targeting_mode_and_positive_range() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    let sim = state(rules);
    let controls = native_superweapon_range_controls();
    for inactive in controls["pulse_controls"]["inactive"].as_array().unwrap() {
        let before = [
            inactive["phase_direction_before"][0].as_i64().unwrap() as i32,
            inactive["phase_direction_before"][1].as_i64().unwrap() as i32,
        ];
        let after = [
            inactive["phase_direction_after"][0].as_i64().unwrap() as i32,
            inactive["phase_direction_after"][1].as_i64().unwrap() as i32,
        ];
        let mut pulse = SuperweaponRangePulse {
            phase: before[0],
            direction: before[1],
        };
        let draws = if inactive["name"] == "no_selected_superweapon" {
            target_query(&sim, rules, None, &mut pulse)
        } else {
            let mut zero_range_rules = rules.clone();
            zero_range_rules
                .super_weapons
                .iter_mut()
                .find(|(id, _)| id.eq_ignore_ascii_case("NukeSpecial"))
                .unwrap()
                .1
                .range = 0.0;
            let zero_range_sim = state(&zero_range_rules);
            target_query(
                &zero_range_sim,
                &zero_range_rules,
                Some("NukeSpecial"),
                &mut pulse,
            )
        };
        assert!(draws.is_empty(), "{}", inactive["name"]);
        assert_eq!(
            [pulse.phase, pulse.direction],
            after,
            "{} preserves the native targeting pulse",
            inactive["name"]
        );
    }

    let mut pulse = SuperweaponRangePulse::default();
    let mut single_ring_rules = rules.clone();
    single_ring_rules
        .super_weapons
        .iter_mut()
        .find(|(id, _)| id.eq_ignore_ascii_case("IronCurtainSpecial"))
        .unwrap()
        .1
        .line_multiplier = 0;
    let single_ring_sim = state(&single_ring_rules);
    let single = target_query(
        &single_ring_sim,
        &single_ring_rules,
        Some("IronCurtainSpecial"),
        &mut pulse,
    );
    assert_eq!(single.len(), 1);
    let single_ring_control = &controls["pulse_controls"]["lm_below_one"];
    let native_call = &single_ring_control["radial_calls"][0];
    assert_eq!(single[0].radius.to_bits(), 0x3FB3_3333);
    assert_eq!(native_call["radius_f32_bits"], "3FB33333");
    let native_flags: Vec<u64> = native_call["pixel_radius_and_truncate_flags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|flag| flag.as_u64().unwrap())
        .collect();
    assert_eq!(native_flags, [0, 1], "fallback passes pixel=0, truncate=1");
    assert_eq!(single[0].pixel_radius, native_flags[0] != 0);
    assert_eq!(single[0].truncate_radius, native_flags[1] != 0);
    let native_helper_control = controls["primitive_radius_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|control| control["name"] == "retail_lm_below_one_single_ring")
        .unwrap();
    assert_eq!(
        native_helper_control["input_radius_f32_bits"],
        native_call["radius_f32_bits"]
    );
    assert_eq!(
        crate::render::radial_indicator::radius_pixels(single[0].radius, single[0].truncate_radius),
        Some(
            native_helper_control["output_radius_pixels"]
                .as_i64()
                .unwrap() as i32
        )
    );
    assert_eq!(
        pulse,
        SuperweaponRangePulse {
            phase: 10,
            direction: 1
        }
    );
    let factor = single_ring_control["caller_factors"][0].as_u64().unwrap() as u32;
    let base = crate::render::palette_light::house_color_rgb(
        &rules.house_color_ramps,
        crate::rules::house_colors::HouseColorIndex(3),
    );
    assert_eq!(
        single[0].rgb,
        crate::render::native_surface_format::interpolate_rgb8(base, [0; 3], factor)
    );
}

#[test]
fn retail_fractional_and_synthetic_loop_controls_reach_the_production_builder() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    let iron = rules.super_weapon("IronCurtainSpecial").unwrap();
    let iron_control = loop_control("iron_curtain_special_retail_1_4_3");
    assert_eq!(iron.range.to_bits(), 0x3FB3_3333);
    assert_eq!(iron.line_multiplier, 3);
    let sim = state(rules);
    let mut pulse = SuperweaponRangePulse::default();
    let draws = target_query(&sim, rules, Some("IronCurtainSpecial"), &mut pulse);
    assert_eq!(
        draws
            .iter()
            .map(|draw| draw.radius.to_bits())
            .collect::<Vec<_>>(),
        bits_from_control(iron_control["radii_f32_bits"].as_array().unwrap())
    );
    let iron_factors: Vec<u32> = iron_control["pulse_draws"][0]["caller_factors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|factor| factor.as_u64().unwrap() as u32)
        .collect();
    let iron_base = crate::render::palette_light::house_color_rgb(
        &rules.house_color_ramps,
        crate::rules::house_colors::HouseColorIndex(3),
    );
    assert_eq!(
        draws.iter().map(|draw| draw.rgb).collect::<Vec<_>>(),
        iron_factors
            .iter()
            .map(|&factor| {
                crate::render::native_surface_format::interpolate_rgb8(iron_base, [0; 3], factor)
            })
            .collect::<Vec<_>>()
    );

    let synthetic = loop_control("synthetic_2_8_5");
    let mut synthetic_rules = rules.clone();
    let nuke = synthetic_rules
        .super_weapons
        .iter_mut()
        .find(|(id, _)| id.eq_ignore_ascii_case("NukeSpecial"))
        .unwrap()
        .1;
    nuke.range = f32::from_bits(
        u32::from_str_radix(synthetic["range_f32_bits"].as_str().unwrap(), 16).unwrap(),
    );
    nuke.line_multiplier = synthetic["line_multiplier"].as_i64().unwrap() as i32;
    let synthetic_sim = state(&synthetic_rules);
    let mut synthetic_pulse = SuperweaponRangePulse::default();
    let synthetic_draws = target_query(
        &synthetic_sim,
        &synthetic_rules,
        Some("NukeSpecial"),
        &mut synthetic_pulse,
    );
    assert_eq!(
        synthetic_draws
            .iter()
            .map(|draw| draw.radius.to_bits())
            .collect::<Vec<_>>(),
        bits_from_control(synthetic["radii_f32_bits"].as_array().unwrap())
    );
}

#[test]
fn large_concentric_outer_ring_keeps_a_bounded_expanding_ring() {
    let patch =
        crate::rules::ini_parser::IniFile::from_str("[NAPSIS]\nPsychicDetectionRadius=100\n");
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "XMP03T4.MAP",
        &patch,
    ) else {
        return;
    };
    let rules = &retail.rules;
    let mut sim = state(rules);
    let sensor = sim
        .spawn_object("NAPSIS", "Americans", 16, 16, 0, rules)
        .unwrap();
    let draws = query(&sim, rules, &[sensor], 32);
    let expanding = draws
        .iter()
        .find(|draw| draw.pixel_radius)
        .expect("expanding ring");
    assert!(expanding.radius > 32.0 && expanding.radius < 1024.0);
    let clip = [
        expanding.center[0] - 100,
        expanding.center[1] - 100,
        200,
        200,
    ];
    let mut operations = Vec::new();
    for draw in draws {
        draw.draws(clip, |operation| operations.push(operation));
    }
    assert_eq!(
        operations.len(),
        2,
        "only the bounded expanding ring reaches drawing"
    );
}

#[test]
fn targeting_a_missing_cell_cannot_change_the_simulation_dummy() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    let mut sim = state(rules);
    let tank = sim
        .spawn_object("HTNK", "Americans", 12, 12, 0, rules)
        .unwrap();
    // A retained dummy target makes the dummy coordinate part of lockstep
    // state. This supplied prior isolates rendering from target acquisition.
    sim.entities_mut_for_test()
        .get_mut(tank)
        .unwrap()
        .set_archive_target(Some(crate::sim::combat::TargetKind::Cell(
            u16::MAX,
            u16::MAX,
        )));
    let dummy = sim.effective_shared_cell_dummy();
    dummy.stamp_coord(7, 9);
    dummy.set_level_slope(-2, 3);
    let before_dummy = dummy.snapshot();
    let before_hash = sim.state_hash();
    let save = || {
        crate::sim::snapshot::GameSnapshot::save(
            &sim,
            0,
            rules.source_ini_hash(),
            "range-cursor",
            0,
        )
    };
    let before_save = save();
    for cell in [(40, 40), (u16::MAX, u16::MAX)] {
        assert!(
            sim.resolved_terrain()
                .unwrap()
                .native_fixed_cell_index(cell.0 as i16, cell.1 as i16)
                .is_none()
        );
        let draws = build(
            Some(&sim),
            Some(rules),
            &[],
            Some("NukeSpecial"),
            None,
            Some(cell),
            false,
            &mut SuperweaponRangePulse::default(),
            &HouseColorMap::new(),
            SurfaceLineViewport {
                camera: [0; 2],
                clip: [0, 0, 1024, 768],
                z_origin_y: 0,
                zoom: 1.0,
            },
            0,
        );
        assert!(!draws.is_empty());
        assert_eq!(dummy.snapshot(), before_dummy);
        assert_eq!(sim.state_hash(), before_hash);
        assert_eq!(
            save(),
            before_save,
            "rendering must preserve all serialized state"
        );
    }
}

fn advance(sim: &mut Simulation, rules: &RuleSet) {
    sim.advance_app_frame(
        &[],
        rules,
        None,
        33,
        TickLane::Ordinary,
        FrameEffects::default(),
    )
    .unwrap();
}

#[test]
fn stock_selected_gap_uses_current_house_ledger_order_and_online_not_power() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    let mut sim = state(rules);
    let a = sim
        .spawn_object("GAGAP", "Americans", 16, 16, 64, rules)
        .unwrap();
    let b = sim
        .spawn_object("GAGAP", "Americans", 20, 16, 64, rules)
        .unwrap();
    let rival = sim
        .spawn_object("GAGAP", "British", 28, 28, 64, rules)
        .unwrap();
    let pillbox = sim
        .spawn_object("GAPILL", "Americans", 12, 12, 64, rules)
        .unwrap();
    for _ in 0..3 {
        advance(&mut sim, rules);
    }
    assert_eq!(sim.building_operational_state(a, rules), Some(false));
    assert!(sim.entities().get(a).unwrap().building_online());
    let first = query(&sim, rules, &[a, b], 0);
    let reversed = query(&sim, rules, &[b, a], 0);
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].center, reversed[1].center);
    assert_eq!(first[1].center, reversed[0].center);
    assert_ne!(first[0].center, first[1].center);
    assert!(query(&sim, rules, &[], 0).is_empty());
    assert!(
        query(&sim, rules, &[rival], 0).is_empty(),
        "another human house is not CurrentHouse"
    );
    assert!(
        query(&sim, rules, &[pillbox], 0).is_empty(),
        "weapon range alone does not grant HasRadialIndicator"
    );
    let house = sim.interner().get("British");
    sim.session_mut_for_test().set_current_house_for_test(house);
    assert!(query(&sim, rules, &[a], 0).is_empty());
    assert_eq!(query(&sim, rules, &[rival], 0).len(), 1);
}

#[test]
fn shared_concentric_consumer_uses_recorded_os_phase_and_native_admission() {
    use crate::render::radial_indicator::tests::native;
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    let mut sim = state(rules);
    let sensor = sim
        .spawn_object("NAPSIS", "Americans", 16, 16, 64, rules)
        .unwrap();
    let mut checked = 0;
    for row in native()["scenes"].as_array().unwrap() {
        if row["input"]["actors"][0]["type"] != "NAPSIS" {
            continue;
        }
        let actual = query(
            &sim,
            rules,
            &[sensor],
            row["input"]["time_get_time_ms"].as_u64().unwrap(),
        );
        let expected = row["radials"].as_array().unwrap();
        assert_eq!(actual.len(), expected.len(), "{}", row["input"]["name"]);
        for (a, e) in actual.iter().zip(expected) {
            assert!(!a.rays && !a.fade);
            assert_eq!(a.pixel_radius, e["pixel_radius"]);
            let radius = f32::from_bits(
                u32::from_str_radix(e["radius_f32_bits"].as_str().unwrap(), 16).unwrap(),
            );
            assert_eq!(a.radius, radius);
        }
        checked += 1;
    }
    assert_eq!(checked, 6);
}

#[test]
fn ordinary_temporal_warp_suppresses_gap_query_and_release_restores_it() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    let mut sim = state(rules);
    let gap = sim
        .spawn_object("GAGAP", "Americans", 16, 16, 64, rules)
        .unwrap();
    let legionnaire = sim
        .spawn_object("CLEG", "Americans", 16, 19, 64, rules)
        .unwrap();
    assert!(sim.apply_command(
        "Americans",
        &Command::Attack {
            attacker_id: legionnaire,
            target_id: gap
        },
        Some(rules)
    ));
    for _ in 0..120 {
        advance(&mut sim, rules);
        if !sim.entities().get(gap).unwrap().building_online() {
            break;
        }
    }
    assert!(
        !sim.entities().get(gap).unwrap().building_online(),
        "ordinary Temporal detonation reached the retained latch"
    );
    //An explicit prepared CurrentObjects prior isolates the query's online
    //gate even though ordinary warp also clears the player's selection.
    assert!(query(&sim, rules, &[gap], 0).is_empty());
    assert!(sim.apply_command(
        "Americans",
        &Command::Stop {
            entity_id: legionnaire
        },
        Some(rules)
    ));
    for _ in 0..30 {
        advance(&mut sim, rules);
    }
    assert!(sim.entities().get(gap).unwrap().building_online());
    assert_eq!(query(&sim, rules, &[gap], 0).len(), 1);
}

#[cfg(feature = "phobos")]
fn enhancer_rules(range: &str, amount: &str, global: bool) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[BuildingTypes]\n0=ENH\n1=GAPOWR\n2=RADAR\n\
         [ENH]\nStrength=100\nPowerPlantEnhancer.PowerPlants=GAPOWR\n\
         PowerPlantEnhancer.Range={range}\nPowerPlantEnhancer.Amount={amount}\n\
         [GAPOWR]\nStrength=100\n[RADAR]\nStrength=100\n\
         [AudioVisual]\nShowPowerPlantEnhancerRange={}\n",
        if global { "yes" } else { "no" }
    )))
    .expect("power plant enhancer rules")
}

/// Insert a finished Structure fixture with the simulation's own interned
/// type and owner handles; unlimbo admission is not what these display tests
/// measure.
#[cfg(feature = "phobos")]
fn insert_enhancer(
    sim: &mut Simulation,
    type_id: &str,
    owner: &str,
    cell: (u16, u16),
    online: bool,
) -> u64 {
    let mut entity = crate::sim::game_entity::GameEntity::test_default_of_category(
        1,
        type_id,
        owner,
        cell.0,
        cell.1,
        crate::map::entities::EntityCategory::Structure,
    );
    entity.set_type_ref_for_test(sim.intern(type_id));
    entity.set_owner_for_test(sim.intern(owner));
    entity.lifecycle.set_in_limbo_for_test(false);
    entity.lifecycle.set_object_alive_for_test(true);
    entity.set_building_online_for_test(online);
    sim.entities_mut_for_test().insert(entity);
    1
}

#[cfg(feature = "phobos")]
fn placement_query(
    sim: &Simulation,
    rules: &RuleSet,
    armed: &str,
    cursor: (u16, u16),
    show: bool,
) -> Vec<RadialIndicatorDraw> {
    let mut pulse = SuperweaponRangePulse::default();
    build(
        Some(sim),
        Some(rules),
        &[],
        None,
        Some(armed),
        Some(cursor),
        show,
        &mut pulse,
        &HouseColorMap::new(),
        SurfaceLineViewport {
            camera: [0; 2],
            clip: [0, 0, 1024, 768],
            z_origin_y: 0,
            zoom: 1.0,
        },
        0,
    )
}

#[cfg(feature = "phobos")]
#[test]
fn placement_draws_an_active_enhancers_range_at_the_enhancer_cell() {
    let rules = enhancer_rules("4", "10", true);
    let mut sim = state(&rules);
    insert_enhancer(&mut sim, "ENH", "Americans", (16, 16), true);
    let draws = placement_query(&sim, &rules, "GAPOWR", (18, 16), true);
    assert_eq!(draws.len(), 1);
    let draw = draws[0];
    assert_eq!(draw.radius, 4.0);
    assert!(draw.truncate_radius && !draw.pixel_radius);
    assert!(draw.fade && !draw.rays);
    assert_eq!(draw.frame, sim.session().binary_frame() as i32);
    assert_eq!(
        draw.rgb,
        crate::render::palette_light::house_color_rgb(
            &rules.house_color_ramps,
            crate::rules::house_colors::NO_REMAP
        )
    );

    // Both switches are caller-side: no client switch, no ring.
    assert!(placement_query(&sim, &rules, "GAPOWR", (18, 16), false).is_empty());
    // A placing type the enhancer does not name.
    assert!(placement_query(&sim, &rules, "RADAR", (18, 16), true).is_empty());
    // Outside the native 1.5 * range cell prefilter (14 > 6).
    assert!(placement_query(&sim, &rules, "GAPOWR", (30, 16), true).is_empty());
}

#[cfg(feature = "phobos")]
#[test]
fn placement_enhancer_rings_need_membership_range_and_the_current_player() {
    // A list with the default Amount/Factor is not registered as an enhancer.
    let inert = enhancer_rules("4", "0", true);
    let mut sim = state(&inert);
    insert_enhancer(&mut sim, "ENH", "Americans", (16, 16), true);
    assert!(placement_query(&sim, &inert, "GAPOWR", (18, 16), true).is_empty());

    // Range 0 disables the ring even for a registered enhancer.
    let zero_range = enhancer_rules("0", "10", true);
    let mut sim = state(&zero_range);
    insert_enhancer(&mut sim, "ENH", "Americans", (16, 16), true);
    assert!(placement_query(&sim, &zero_range, "GAPOWR", (18, 16), true).is_empty());

    // The native loop only walks the current player's enhancers.
    let rules = enhancer_rules("4", "10", true);
    let mut sim = state(&rules);
    insert_enhancer(&mut sim, "ENH", "British", (16, 16), true);
    assert!(placement_query(&sim, &rules, "GAPOWR", (18, 16), true).is_empty());

    // An offline enhancer is not drawn.
    let mut sim = state(&rules);
    insert_enhancer(&mut sim, "ENH", "Americans", (16, 16), false);
    assert!(placement_query(&sim, &rules, "GAPOWR", (18, 16), true).is_empty());
    sim.entities_mut_for_test()
        .get_mut(1)
        .unwrap()
        .set_building_online_for_test(true);
    assert_eq!(
        placement_query(&sim, &rules, "GAPOWR", (18, 16), true).len(),
        1
    );
}
