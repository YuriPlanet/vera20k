//! Whole original CanAutoCloak6FBDC0 with real6F7780/SelectWeapon/InRange.
//!
//! Native actor placement, House/rank facts and retained timers are component
//! inputs. The companion retail regressions exercise ordinary Unlimbo, Attack
//! and advance_tick separately. This corpus does not compare native cloak
//! transitions, DetachAll, movement or the whole world.

use crate::rules::ini_parser::IniFile;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::sim::cloak_disguise::CloakRuntime;
use crate::sim::combat::TargetKind;
use crate::sim::rng::SimRng;
use crate::sim::timer::CdTimer;
use serde_json::{Value, json};

fn signed(value: &Value) -> i32 {
    i32::try_from(value.as_i64().expect("native signed dword")).unwrap()
}

#[test]
fn original_can_auto_cloak_matches_retail_inputs_and_retained_target_controls() {
    let native = crate::test_fixture::json("tools/spatial_oracle/cloak_target_gate.json");
    let map = crate::test_fixture::checkout_root()
        .join("tools/spatial_oracle/cloak_target_gate/ordinary-sub-cloak.map");
    let Some(retail) =
        crate::rules::retail_ini_fixture::retail_battle_rules_for_map(map.to_str().unwrap())
    else {
        return;
    };
    let rules = &retail.rules;
    let inputs = &native["inputs"]["final"];
    for name in ["SUB", "DEST"] {
        let object = rules.object(name).unwrap();
        let expected = &inputs["types"][name];
        assert_eq!(
            object.cloakable,
            expected["cloakable"].as_bool().unwrap(),
            "{name}"
        );
        assert_eq!(
            object.cloak_stop,
            expected["cloak_stop"].as_bool().unwrap(),
            "{name}"
        );
        assert_eq!(
            object.cloaking_speed,
            signed(&expected["cloaking_speed"]),
            "{name}"
        );
        assert_eq!(object.strength, signed(&expected["strength"]), "{name}");
        assert_eq!(
            crate::sim::combat::armor_index(&object.armor),
            expected["armor"].as_u64().unwrap() as usize,
            "{name}"
        );
        assert_eq!(object.naval, expected["naval"].as_bool().unwrap(), "{name}");
        assert_eq!(
            object.underwater,
            expected["underwater"].as_bool().unwrap(),
            "{name}"
        );
        assert_eq!(
            object.land_targeting,
            signed(&expected["land_targeting"]),
            "{name}"
        );
        for slot in 0..2 {
            assert_eq!(
                object.weapon_at(slot),
                expected["weapons"][slot].as_str(),
                "{name} slot{slot}"
            );
            assert_eq!(
                object.elite_weapon_at(slot),
                expected["elite_weapons"][slot].as_str(),
                "{name} elite{slot}"
            );
        }
    }
    for (name, expected) in inputs["weapons"].as_object().unwrap() {
        let weapon = rules.weapon(name).unwrap();
        assert_eq!(weapon.range_leptons, signed(&expected["range"]), "{name}");
        assert_eq!(
            weapon.minimum_range_leptons,
            signed(&expected["minimum_range"]),
            "{name}"
        );
        assert_eq!(weapon.rof, signed(&expected["rof"]), "{name}");
        assert_eq!(weapon.damage, signed(&expected["damage"]), "{name}");
        assert_eq!(
            weapon.decloak_to_fire,
            expected["decloak_to_fire"].as_bool().unwrap(),
            "{name}"
        );
        assert_eq!(
            weapon.projectile.as_deref(),
            expected["projectile"].as_str(),
            "{name}"
        );
        assert_eq!(
            weapon.warhead.as_deref(),
            expected["warhead"].as_str(),
            "{name}"
        );
    }
    for (name, expected) in inputs["projectiles"].as_object().unwrap() {
        let projectile = rules.projectile(name).unwrap();
        assert_eq!(projectile.aa, expected["aa"].as_bool().unwrap(), "{name}");
        assert_eq!(projectile.ag, expected["ag"].as_bool().unwrap(), "{name}");
        assert_eq!(
            projectile.arcing,
            expected["arcing"].as_bool().unwrap(),
            "{name}"
        );
        assert_eq!(
            projectile.image.as_deref().unwrap_or(""),
            expected["image"].as_str().unwrap(),
            "{name}"
        );
    }
    for (name, expected) in inputs["warheads"].as_object().unwrap() {
        for (index, verse) in rules.warhead(name).unwrap().verses_f64.iter().enumerate() {
            assert_eq!(
                verse
                    .to_le_bytes()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
                expected["verses_bits"][index].as_str().unwrap(),
                "{name} verse{index}",
            );
        }
    }
    assert_eq!(
        rules.general.cloaking_stages,
        signed(&inputs["general"]["cloaking_stages"])
    );
    let delay = retail
        .processed_rules
        .section("General")
        .unwrap()
        .read_double("CloakDelay", 0.0);
    assert_eq!(
        format!("{:016x}", delay.to_bits()),
        inputs["general"]["cloak_delay_bits"].as_str().unwrap()
    );
    let omitted = IniFile::from_str("[DEFAULTS]\n");
    let defaults = ObjectType::from_ini_section(
        "DEFAULTS",
        &omitted.section_or_empty("DEFAULTS"),
        ObjectCategory::Vehicle,
    );
    assert_eq!(
        defaults.cloaking_speed,
        signed(&native["inputs"]["constructor"]["types"]["SUB"]["cloaking_speed"])
    );
    let constructor_delay = f64::from_bits(
        u64::from_str_radix(
            native["inputs"]["constructor"]["general"]["cloak_delay_bits"]
                .as_str()
                .unwrap(),
            16,
        )
        .unwrap(),
    );
    assert_eq!(
        constructor_delay, 0.0,
        "original Rules constructor has no recloak delay"
    );
    assert_eq!(
        crate::rules::ruleset::RuleSet::from_ini(&omitted)
            .unwrap()
            .general
            .cloak_delay_frames,
        0
    );

    let rows = native["rows"].as_array().unwrap();
    assert!(!rows.is_empty());
    for row in rows {
        let input = &row["input"];
        let label = input["name"].as_str().unwrap();
        let mut sim = crate::sim::arena_fixture::rival_water_arena(rules, 31, 64);
        let mut terrain = sim.resolved_terrain.take().unwrap();
        for y in 0..64 {
            for x in 0..64 {
                terrain.cell_mut_for_test(x, y).unwrap().level =
                    native["inputs"]["world_boundary"]["flat_cell_level"]
                        .as_u64()
                        .unwrap() as u8;
            }
        }
        sim.install_resolved_terrain_for_new_map(terrain);
        assert!(sim.rebuild_dynamic_navigation(rules));
        for house in sim.houses.values_mut() {
            house.set_is_human_for_test(true);
        }
        let spawn = |sim: &mut super::Simulation, name, owner, xyz: &Value| {
            let id = sim
                .spawn_object(
                    name,
                    owner,
                    u16::try_from(signed(&xyz[0]) / 256).unwrap(),
                    u16::try_from(signed(&xyz[1]) / 256).unwrap(),
                    0,
                    rules,
                )
                .unwrap();
            let position = crate::sim::movement::ground_pose::position_world_coord(
                &sim.substrate.entities.get(id).unwrap().position,
            );
            assert_eq!(
                [position.x, position.y, position.z],
                [signed(&xyz[0]), signed(&xyz[1]), signed(&xyz[2])],
                "{label} component placement"
            );
            id
        };
        let source = spawn(&mut sim, "SUB", "Americans", &input["source_xyz"]);
        let target = spawn(&mut sim, "DEST", "Russians", &input["target_xyz"]);
        sim.session
            .set_binary_frame_for_test(u32::try_from(signed(&input["frame"])).unwrap());
        let actor = sim.substrate.entities.get_mut(source).unwrap();
        // Deserialize explicit native component inputs through the cloak
        // owner's existing persistence boundary; no new mutation API/state.
        let mut cloak = serde_json::to_value(CloakRuntime::new(signed(&input["frame"]))).unwrap();
        cloak["state"] = input["state"].clone();
        cloak["depth"] = input["progress"].clone();
        cloak["recloak_delay"] = json!({"start_frame": signed(&input["recloak"][0]), "duration": signed(&input["recloak"][1])});
        actor.cloak = Some(serde_json::from_value(cloak).unwrap());
        actor.rearm_timer.set_for_test(CdTimer::from_raw(
            signed(&input["rearm"][0]),
            signed(&input["rearm"][1]),
        ));
        actor.attack_target.set_for_test(
            input["target"]
                .as_bool()
                .unwrap()
                .then_some(TargetKind::Entity(target)),
        );
        let retained = actor.cloak;
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["scenario"].as_str().unwrap());
        sim.main_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["main"].as_str().unwrap())
                .into();
        sim.mapgen_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["mapgen"].as_str().unwrap());
        let rng = sim.rng_state();
        let facts = super::stock_cloak_tick_facts(&sim, source, rules, None).unwrap();
        assert_eq!(
            facts.can_auto_cloak,
            row["can_auto"].as_bool().unwrap(),
            "{label}"
        );
        if let Some(expected) = row["can_fire_at_target"].as_bool() {
            assert_eq!(
                sim.techno_can_fire_at_target(source, TargetKind::Entity(target), rules, None),
                expected,
                "{label} original range"
            );
        }
        assert_eq!(
            sim.substrate.entities.get(source).unwrap().cloak,
            retained,
            "{label} no timer/cloak writes"
        );
        assert_eq!(sim.rng_state(), rng, "{label} Rust query RNG");
        assert_eq!(
            json!({"scenario": sim.scenario_rng.native_state_hex(),
                "main": sim.main_rng.native_state_hex(),
                "mapgen": sim.mapgen_rng.native_state_hex()}),
            row["rng_after"],
            "{label} full native/Rust RNG comparison",
        );
        assert_eq!(
            row["rng_before"], row["rng_after"],
            "{label} all3 full native RNG states"
        );
        assert_eq!(row["writes"], json!([]), "{label} native query writes");
    }
}
