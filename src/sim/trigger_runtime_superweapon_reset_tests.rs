//! Execute133/134 original controls, sharing the retail/clock/RNG fixture.
//! Native memory-only animation and notification priors are not Rust state;
//! the production Chronosphere check below creates its held anim normally.

use super::*;

fn reset_corpus() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/trigger_superweapon_reset.json")
}

fn runtime_state(sim: &Simulation, rules: &RuleSet, owner: InternedId, id: InternedId) -> Value {
    let mut result = state(sim, owner, id);
    let instance = &sim.super_weapons[&owner][&id];
    let sw = rules.super_weapon(sim.interner.resolve(id)).unwrap();
    result["effective_recharge"] = json!(instance.recharge_time(sw));
    result["ready_tick"] = json!(instance.ready_tick());
    result["charge_drain_state"] = json!(instance.charge_drain_state());
    result
}

fn assert_runtime_state(actual: &Value, native: &Value, label: &str) {
    for (key, value) in actual.as_object().unwrap() {
        assert_eq!(value, &native[key], "{label}: {key}");
    }
}

fn reset_world(
    rules: &RuleSet,
    input: &Value,
    data: &Value,
) -> (Simulation, InternedId, InternedId, InternedId) {
    let mut sim = Simulation::with_seed(0);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.session
        .set_binary_frame_for_test(int(&input["frame"]) as u32);
    let owner = sim.interner.intern("IncomingHouse");
    let other = sim.interner.intern("UnrelatedHouse");
    for house in [owner, other] {
        sim.houses
            .register(HouseState::new(house, 0, None, false, 0, 10));
        crate::sim::superweapon::construct_house_supers(&mut sim, rules, house, false);
    }
    let id = sim.interner.get("NukeSpecial").unwrap();
    let instance = sim
        .super_weapons
        .get_mut(&owner)
        .unwrap()
        .get_mut(&id)
        .unwrap();
    instance.set_recharge_time(int(&input["custom_recharge"]));
    instance.set_charge_start_tick_for_test(int(&input["start"]));
    instance.set_charge_duration_for_test(int(&input["duration"]));
    instance.set_is_active_for_test(input["granted"].as_bool().unwrap());
    instance.set_is_ready_for_test(input["ready"].as_bool().unwrap());
    instance.set_is_suspended_for_test(input["held"].as_bool().unwrap());
    instance.set_ready_tick_for_test(int(&input["ready_tick"]));
    instance.set_charge_drain_state_for_test(int(&input["charge_drain_state"]));
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_preferred_super_weapon_target(cell(&input["preferred"]));
    sim.houses
        .get_mut(&other)
        .unwrap()
        .set_preferred_super_weapon_target(cell(&input["other_preferred"]));
    sim.super_weapons
        .get_mut(&other)
        .unwrap()
        .get_mut(&id)
        .unwrap()
        .set_recharge_time(int(&input["other_custom_recharge"]));
    seeded_rngs(&mut sim, data);
    (sim, owner, other, id)
}

fn action(raw: &str) -> ActionEntry {
    let ini = crate::rules::ini_parser::IniFile::from_str(&format!("[Actions]\nCASE=1,{raw}\n"));
    crate::map::actions::parse_actions(&ini)
        .remove("CASE")
        .unwrap()
        .entries()[0]
        .clone()
}

fn execute(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    entry: ActionEntry,
    owner_present: bool,
) -> bool {
    let resolved = RuntimeDefinitions::default().resolve_action(entry, None, None);
    let context = TriggerActionContext {
        live_index: None,
        incoming_house: owner_present.then_some(owner),
        input: TagEventInput::default(),
    };
    let mut effects = Vec::new();
    let (returned, draws) = crate::sim::rng::trace_draws(|| {
        sim.apply_live_trigger_action(
            &resolved,
            context,
            &mut effects,
            Some(rules),
            None,
            FrameEffects::default(),
        )
    });
    assert!(draws.is_empty() && effects.is_empty());
    returned
}

fn replay_row(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    other: InternedId,
    id: InternedId,
    row: &Value,
    compare_native_rng_prior: bool,
) {
    let input = &row["input"];
    let label = row["name"].as_str().unwrap();
    sim.session
        .set_binary_frame_for_test(int(&input["frame"]) as u32);
    assert_runtime_state(&runtime_state(sim, rules, owner, id), input, label);
    let unrelated = serde_json::to_value(&sim.super_weapons[&other][&id]).unwrap();
    let other_target = sim.houses[&other].preferred_super_weapon_target();
    let entry = action(row["read"]["raw"].as_str().unwrap());
    assert_eq!(json!(entry.kind), row["read"]["kind"]);
    assert_eq!(json!(entry.literal_value()), row["read"]["value"]);
    assert_eq!(json!(entry.auxiliary_value()), row["read"]["auxiliary"]);
    let incoming_rng = rng_hashes(sim);
    if compare_native_rng_prior {
        assert_eq!(incoming_rng, row["rng_before"], "{label}");
    }
    let returned = execute(
        sim,
        rules,
        owner,
        entry,
        input["owner_present"].as_bool().unwrap(),
    );
    assert_eq!(i32::from(returned), int(&row["returned"]), "{label}");
    assert_runtime_state(&runtime_state(sim, rules, owner, id), &row["output"], label);
    assert_eq!(
        serde_json::to_value(&sim.super_weapons[&other][&id]).unwrap(),
        unrelated,
        "{label}: other Super"
    );
    assert_eq!(
        sim.houses[&other].preferred_super_weapon_target(),
        other_target
    );
    assert_eq!(rng_hashes(sim), incoming_rng, "{label}: no RNG draws");
    if compare_native_rng_prior {
        assert_eq!(rng_hashes(sim), row["rng_after"], "{label}");
    }
    assert!(row["rng_calls"].as_array().unwrap().is_empty());
}

#[test]
fn superweapon_reset_dispatch_matches_original_controls() {
    let Some((rules, _)) = all03_inputs() else {
        return;
    };
    let data = reset_corpus();
    assert_eq!(data["retail_inputs"], corpus()["retail_inputs"]);
    for row in data["dispatch_controls"].as_array().unwrap() {
        let (mut sim, owner, other, id) = reset_world(&rules, &row["input"], data);
        replay_row(&mut sim, &rules, owner, other, id, row, true);
    }
}

#[test]
fn superweapon_reset_history_survives_snapshot_between_reset_and_clear() {
    use crate::sim::snapshot::GameSnapshot;
    let Some((rules, _)) = all03_inputs() else {
        return;
    };
    let data = reset_corpus();
    for sequence in data["sequence_controls"].as_array().unwrap() {
        let rows = sequence["rows"].as_array().unwrap();
        // First compare the entire uninterrupted original history, including
        // all three native RNG priors at every dispatch boundary.
        let (mut direct, owner, other, id) = reset_world(&rules, &rows[0]["input"], data);
        for row in rows {
            replay_row(&mut direct, &rules, owner, other, id, row, true);
        }
        let (mut sim, owner, other, id) = reset_world(&rules, &rows[0]["input"], data);
        let mut restored = false;
        for (step, row) in rows.iter().enumerate() {
            replay_row(&mut sim, &rules, owner, other, id, row, !restored);
            if step == 1 {
                let retained = serde_json::to_value(&sim.super_weapons).unwrap();
                let saved = GameSnapshot::save(&sim, 0, 0, "SuperReset", 0);
                sim = GameSnapshot::load(&saved).unwrap().sim;
                sim.restore_after_snapshot_load().unwrap();
                assert_eq!(serde_json::to_value(&sim.super_weapons).unwrap(), retained);
                // The existing load owner deliberately reseeds Scenario RNG
                // and restarts load-time trackers. Compare the retained Super
                // state and subsequent no-draw actions, not whole-world CRC or
                // the uninterrupted history's pre-load RNG seed.
                restored = true;
            }
        }
    }
}

#[test]
fn superweapon_reset_releases_a_production_chrono_placement_without_destroying_it() {
    let Some((mut rules, _)) = all03_inputs() else {
        return;
    };
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
    let art = crate::rules::retail_sources::select_ini(&assets, "ARTMD.INI").unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art.ini));
    let placement = rules.general.chrono_placement_anim.clone();
    let image = rules
        .art()
        .resolve_effective_image_id(&placement, &placement);
    let bytes = assets.get_ref(&format!("{image}.SHP")).unwrap();
    let frames = crate::assets::shp_file::ShpFile::frame_count_from_bytes(bytes).unwrap();
    // The lib fixture has no app asset loader. Bind the physical SHP header,
    // then create the animation through the existing normal launch owner.
    rules.bind_anim_frame_count_for_test(&placement, i32::from(frames));
    let data = reset_corpus();
    let (mut sim, owner, _, _) = reset_world(&rules, &data["dispatch_controls"][0]["input"], data);
    sim.session.set_map_width_for_test(32);
    sim.session.set_map_height_for_test(32);
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_grid(
        32,
        32,
        crate::map::resolved_terrain::test_flat_cell,
    ));
    crate::sim::arena_fixture::supply_native_map_keyed(&mut sim, "trigger-reset-placement");
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_player_control_for_test(true);
    let id = sim.interner.get("ChronoSphereSpecial").unwrap();
    let instance = sim
        .super_weapons
        .get_mut(&owner)
        .unwrap()
        .get_mut(&id)
        .unwrap();
    instance.set_is_active_for_test(true);
    instance.set_is_ready_for_test(true);
    instance.set_recharge_time(4500);
    assert!(sim.fire_super_weapon(
        &rules,
        owner,
        id,
        (10, 10),
        None,
        FrameEffects::default(),
        false
    ));
    let held = sim.super_placement_anims();
    assert_eq!(held.len(), 1);
    let anim = held[0].1;
    let loops = sim
        .substrate
        .anims
        .get(anim)
        .unwrap()
        .runtime()
        .loop_remaining;
    let before = serde_json::to_value(&sim.super_weapons[&owner][&id]).unwrap();
    let index = rules
        .super_weapon_order
        .iter()
        .position(|name| name == "ChronoSphereSpecial")
        .unwrap();
    assert!(execute(
        &mut sim,
        &rules,
        owner,
        action(&format!("133,11,{index},0,0,0,0,NA")),
        true
    ));
    let after = serde_json::to_value(&sim.super_weapons[&owner][&id]).unwrap();
    for (key, value) in before.as_object().unwrap() {
        if key != "custom_recharge_time" {
            assert_eq!(&after[key], value, "133 preserves {key}");
        }
    }
    assert_eq!(sim.super_placement_anims(), held);
    assert_eq!(
        sim.substrate
            .anims
            .get(anim)
            .unwrap()
            .runtime()
            .loop_remaining,
        loops
    );
    assert!(execute(
        &mut sim,
        &rules,
        owner,
        action(&format!("134,11,{index},0,0,0,0,NA")),
        true
    ));
    assert!(sim.super_placement_anims().is_empty());
    // Original Reset clears the loops byte and its pointer. It does not
    // synchronously destroy/detach the Anim; that existing lifecycle runs later.
    let expected = data["dispatch_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "134_g1_r1_h0_stopped0")
        .unwrap();
    assert_eq!(
        json!(
            sim.substrate
                .anims
                .get(anim)
                .unwrap()
                .runtime()
                .loop_remaining
        ),
        expected["output"]["animation_loops"]
    );
    assert!(!sim.super_weapons[&owner][&id].is_ready());
}
