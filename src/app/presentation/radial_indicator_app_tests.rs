use super::*;
use crate::sim::command::Command;
use crate::sim::world::{FrameEffects, TickLane};

fn state(rules: &RuleSet) -> Simulation {
    let mut sim = Simulation::with_seed(1);
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    for name in ["Americans", "British"] {
        let owner = sim.interner.intern(name);
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
        );
        sim.session.house_order.push(owner);
    }
    sim.session.game_mode_nonzero = true;
    sim.session.current_house = sim.interner.get("Americans");
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim
}

fn query(sim: &Simulation, rules: &RuleSet, ids: &[u64], ms: u64) -> Vec<RadialIndicatorDraw> {
    let before = (
        sim.state_hash(),
        sim.main_rng.native_state_hex(),
        sim.scenario_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
    );
    let result = build(
        Some(sim),
        Some(rules),
        ids,
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
            sim.main_rng.native_state_hex(),
            sim.scenario_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex()
        )
    );
    result
}

fn advance(sim: &mut Simulation, rules: &RuleSet) {
    sim.advance_app_frame(
        &[],
        Some(rules),
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
    sim.session.current_house = sim.interner.get("British");
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
            ) as i32;
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
