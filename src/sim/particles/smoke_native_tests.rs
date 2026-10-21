//! Consumes original execution, never Rust-produced expectations.
use super::*;
use crate::map::bridge_facts::{BRIDGE_FLAG_STRUCTURAL, BridgeCellFacts};
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
use crate::rules::particle_system_type::ParticleSystemTypeId;
use crate::rules::particle_type::ParticleTypeId;
use crate::sim::combat::Health;
use crate::sim::game_entity::GameEntity;
use crate::sim::native_identity::NativeUniqueIdCursor;
use crate::sim::snapshot::GameSnapshot;
use serde_json::Value;

fn corpus() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/damage_smoke.json")
}
fn rules(data: &Value, input: &Value) -> RuleSet {
    super::super::native_fixture::rules_from_layers(
        &data["physical_layers"],
        input,
        "[Particles]\n0=SmallGreySmoke\n1=LargeGreySmoke\n[ParticleSystems]\n0=SmallGreySSys\n1=BigGreySmokeSys\n",
    )
}
fn bits(value: &Value) -> NativeF32Bits {
    NativeF32Bits::from_bits(u32::from_str_radix(value.as_str().unwrap(), 16).unwrap())
}
fn coords(value: &Value) -> IVec3 {
    IVec3::from_array(serde_json::from_value(value.clone()).unwrap())
}
fn particle_type(rules: &RuleSet, value: &Value) -> ParticleTypeId {
    rules.p_type_id_by_name(value.as_str().unwrap()).unwrap()
}
fn sim(bridge: bool) -> Simulation {
    let mut sim = Simulation::new();
    sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(1000));
    let cells = (0..64)
        .flat_map(|y| {
            (0..64).map(move |x| {
                let mut cell = test_flat_cell(x, y);
                if bridge && x == 44 && y == 48 {
                    cell.bridge_facts = BridgeCellFacts {
                        raw_flags: BRIDGE_FLAG_STRUCTURAL,
                        ..BridgeCellFacts::default()
                    };
                }
                cell
            })
        })
        .collect();
    sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(64, 64, cells));
    sim
}
fn adopt_rng(sim: &mut Simulation, case: &Value) {
    sim.scenario_rng =
        SimRng::from_native_state_hex_for_test(case["scenario_before"].as_str().unwrap());
}
fn assert_particle(p: &Particle, expected: &Value) {
    assert_eq!(
        p.native_unique_id(),
        expected["native_id"].as_i64().unwrap() as i32
    );
    assert_eq!(p.coords, coords(&expected["coords"]));
    assert_eq!(
        p.lifetime_remaining as u16,
        expected["remaining_ec"].as_u64().unwrap() as u16
    );
    assert_eq!(p.velocity(), bits(&expected["velocity_bits"]));
    assert_eq!(p.animation_state, expected["state"].as_u64().unwrap() as u8);
    assert_eq!(
        p.translucency,
        expected["translucency"].as_u64().unwrap() as u8
    );
    assert_eq!(
        [p.drift_x, p.drift_y, p.drift_z],
        serde_json::from_value::<[i32; 3]>(expected["drift"].clone()).unwrap()
    );
    assert_eq!(p.marked_for_deletion, expected["marked"].as_bool().unwrap());
}
fn declared_particle(sim: &mut Simulation, rules: &RuleSet, before: &Value) -> Particle {
    let mut p = Particle::test_fixture(
        particle_type(rules, &before["type"]),
        coords(&before["coords"]),
    );
    p.stable_id = sim.substrate.allocate_stable_id();
    p.native_unique_id = before["native_id"].as_i64().unwrap() as i32;
    p.lifetime_remaining = before["remaining_ec"].as_u64().unwrap() as i16;
    p.velocity = bits(&before["velocity_bits"]);
    p.animation_state = before["state"].as_u64().unwrap() as u8;
    p.translucency = before["translucency"].as_u64().unwrap() as u8;
    let drift: [i32; 3] = serde_json::from_value(before["drift"].clone()).unwrap();
    [p.drift_x, p.drift_y, p.drift_z] = drift;
    p.marked_for_deletion = before["marked"].as_bool().unwrap();
    p.state_ai_advance = rules.particle_type(p.type_id).state_ai_advance;
    Simulation::reveal_particle(&mut sim.substrate, p.stable_id());
    p
}

#[test]
fn original_smoke_constructors_preserve_signed_delta_then_lifetime_rng() {
    let data = corpus();
    for case in data["constructors"].as_array().unwrap() {
        let rules = rules(&data, &case["input"]);
        let mut sim = sim(false);
        sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(
            case["native_id_cursor_before"].as_u64().unwrap() as u32,
        ));
        adopt_rng(&mut sim, case);
        let id = particle_type(&rules, &case["particle"]["type"]);
        let p = Particle::new(
            id,
            coords(&case["particle"]["coords"]),
            IVec3::ZERO,
            rules.particle_type(id),
            &mut sim,
        );
        assert_particle(&p, &case["particle"]);
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            case["scenario_after"].as_str().unwrap()
        );
    }
}

#[test]
fn original_smoke_dispatch_cadence_drift_speed_and_word_lifetime() {
    let data = corpus();
    for case in data["particle_ticks"].as_array().unwrap() {
        let rules = rules(&data, &case["input"]);
        let mut sim = sim(false);
        let mut p = declared_particle(&mut sim, &rules, &case["before"]);
        sim.session
            .set_binary_frame_for_test(case["input"]["frame"].as_u64().unwrap_or(40) as u32);
        adopt_rng(&mut sim, case);
        let main = sim.main_rng.native_state_hex();
        let mapgen = sim.mapgen_rng.native_state_hex();
        super::super::system_ai::tick_particle(&mut p, &mut sim, &rules);
        assert_particle(&p, &case["after"]);
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            case["scenario_after"].as_str().unwrap(),
            "{}",
            case["input"]["name"]
        );
        assert_eq!(sim.main_rng.native_state_hex(), main);
        assert_eq!(sim.mapgen_rng.native_state_hex(), mapgen);
    }
}

#[test]
fn original_smoke_move_uses_old_live_bridge_and_retains_move_death_coords() {
    let data = corpus();
    for case in data["movement"].as_array().unwrap() {
        let rules = rules(&data, &case["input"]);
        let mut sim = sim(case["input"]["bridge"] == true);
        let mut p = declared_particle(&mut sim, &rules, &case["before"]);
        adopt_rng(&mut sim, case);
        let ty = p.type_id;
        move_smoke(
            &mut p,
            rules.particle_type(ty),
            rules.general.wind_direction(),
            &sim,
        );
        assert_particle(&p, &case["after"]);
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            case["scenario_after"].as_str().unwrap()
        );
    }
}

fn install_system(
    sim: &mut Simulation,
    rules: &RuleSet,
    input: &Value,
    before: &Value,
) -> (u64, Option<u64>) {
    let owner = if input["owner"] == true {
        let stable = sim.substrate.allocate_stable_id();
        let house = sim.interner.intern("Neutral");
        let ty = sim.interner.intern("FixtureBuilding");
        let category = match input["owner_kind"].as_str().unwrap_or("building") {
            "unit" => EntityCategory::Unit,
            "infantry" => EntityCategory::Infantry,
            "aircraft" => EntityCategory::Aircraft,
            _ => EntityCategory::Structure,
        };
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            stable,
            44,
            48,
            0,
            0,
            house,
            Health::new(255),
            ty,
            category,
            0,
            0,
            false,
        );
        if !input["owner_coords"].is_null() {
            let c = coords(&input["owner_coords"]);
            entity
                .position
                .put_location(crate::sim::components::DriveCoord {
                    x: c.x,
                    y: c.y,
                    z: c.z,
                });
            if category == EntityCategory::Structure {
                entity.type_facts.set_foundation_for_test("2x2".into());
            }
        }
        sim.substrate.entities.insert(entity);
        Some(stable)
    } else {
        None
    };
    let ty = rules
        .ps_type_id_by_name(input["system_type"].as_str().unwrap_or("SmallGreySSys"))
        .unwrap();
    let id = sim
        .spawn_particle_system(
            ty,
            coords(&before["coords"]),
            None,
            owner,
            IVec3::ZERO,
            None,
            rules,
        )
        .unwrap();
    let mut system = sim.substrate.particle_systems.take_for_tick(id).unwrap();
    system.native_unique_id = before["native_id"].as_i64().unwrap() as i32;
    system.spawn_timer = bits(&before["accumulator_bits"]);
    system.lifetime = before["lifetime"].as_i64().unwrap() as i32;
    system.done_spawning = before["done"].as_bool().unwrap();
    for row in before["particles"].as_array().unwrap() {
        system.particles.push(declared_particle(sim, rules, row));
    }
    let cursor = system
        .particles
        .iter()
        .map(Particle::native_unique_id)
        .chain([system.native_unique_id()])
        .max()
        .unwrap() as u32;
    sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(cursor));
    sim.substrate.particle_systems.reinsert_after_tick(system);
    if let Some(owner) = owner {
        sim.substrate
            .entities
            .get_mut(owner)
            .unwrap()
            .damage_smoke_system_id
            .set_for_test(Some(id));
        if !input["moved_owner_coords"].is_null() {
            let c = coords(&input["moved_owner_coords"]);
            sim.substrate
                .entities
                .get_mut(owner)
                .unwrap()
                .position
                .put_location(crate::sim::components::DriveCoord {
                    x: c.x,
                    y: c.y,
                    z: c.z,
                });
        }
    }
    (id, owner)
}

#[test]
fn original_smoke_follows_owner_offset_leaves_trail_and_suppresses_tube_births() {
    let data = corpus();
    let following = crate::test_fixture::json("tools/spatial_oracle/damage_smoke_following.json");
    for case in following["cases"].as_array().unwrap() {
        let mut input = case["input"].clone();
        input["owner"] = Value::Bool(true);
        let rules = rules(&data, &input);
        let mut sim = sim(false);
        let (id, owner) = install_system(&mut sim, &rules, &input, &case["before"]);
        let owner = owner.unwrap();
        assert_eq!(
            sim.particle_systems().get(id).unwrap().offset,
            coords(&case["before"]["offset"]),
            "{} constructor offset",
            input["name"]
        );
        adopt_rng(&mut sim, case);
        for (step, expected) in input["steps"]
            .as_array()
            .unwrap()
            .iter()
            .zip(case["visits"].as_array().unwrap())
        {
            let entity = sim.substrate.entities.get_mut(owner).unwrap();
            let c = coords(&step["coords"]);
            entity
                .position
                .put_location(crate::sim::components::DriveCoord {
                    x: c.x,
                    y: c.y,
                    z: c.z,
                });
            entity.low_bridge_tube_state = (step["tube"] == true).then(|| {
                crate::sim::movement::tube_movement::LowBridgeTubeMovementState::for_test(
                    crate::map::tube_facts::TubeId(0),
                    0,
                    crate::sim::components::DriveCoord {
                        x: c.x,
                        y: c.y,
                        z: c.z,
                    },
                )
            });
            sim.session.set_binary_frame_for_test(0);
            assert!(super::super::system_ai::tick_particle_system(
                &mut sim,
                &rules,
                id,
                crate::sim::world::UninitContext::with_rules(&rules)
            ));
            assert_system(&sim, id, Some(owner), expected);
            assert_eq!(
                sim.particle_systems().get(id).unwrap().offset,
                coords(&expected["offset"])
            );
        }
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            case["scenario_after"].as_str().unwrap(),
            "{} RNG",
            input["name"]
        );
    }
}
fn assert_system(sim: &Simulation, id: u64, owner: Option<u64>, expected: &Value) {
    let sys = sim.particle_systems().get(id).unwrap();
    assert_eq!(sys.spawn_timer(), bits(&expected["accumulator_bits"]));
    assert_eq!(sys.coords, coords(&expected["coords"]));
    assert_eq!(sys.alive(), expected["alive"].as_bool().unwrap());
    assert_eq!(sys.in_limbo(), expected["limbo"].as_bool().unwrap());
    assert_eq!(sys.done_spawning, expected["done"].as_bool().unwrap());
    assert_eq!(sys.lifetime, expected["lifetime"].as_i64().unwrap() as i32);
    let particles = expected["particles"].as_array().unwrap();
    assert_eq!(sys.particles.len(), particles.len());
    for (p, row) in sys.particles.iter().zip(particles) {
        assert_particle(p, row);
        // Marked-by-Move remains in the native AIR list until next UnInit.
        assert_eq!(
            sim.substrate
                .display
                .layer_of(p.stable_id())
                .unwrap()
                .index(),
            3
        );
    }
    if let Some(owner) = owner {
        assert_eq!(
            sim.substrate
                .entities
                .get(owner)
                .unwrap()
                .damage_smoke_system_id
                .system()
                == Some(id),
            expected["owner_has_smoke"].as_bool().unwrap()
        );
    }
}

#[test]
fn original_smoke_system_birth_split_emission_cutoff_and_limbo() {
    let data = corpus();
    for case in data["system_ticks"].as_array().unwrap() {
        let rules = rules(&data, &case["input"]);
        let mut sim = sim(case["input"]["bridge"] == true);
        let (id, owner) = install_system(&mut sim, &rules, &case["input"], &case["before"]);
        adopt_rng(&mut sim, case);
        for visit in case["visits"].as_array().unwrap() {
            sim.session
                .set_binary_frame_for_test(visit["frame"].as_u64().unwrap() as u32);
            assert!(super::super::system_ai::tick_particle_system(
                &mut sim,
                &rules,
                id,
                crate::sim::world::UninitContext::with_rules(&rules)
            ));
            assert_system(&sim, id, owner, &visit["state"]);
        }
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            case["scenario_after"].as_str().unwrap(),
            "{}",
            case["input"]["name"]
        );
    }
}

#[test]
fn original_stored_smoke_accumulator_preserves_long_run_rng_boundaries() {
    let data = corpus();
    let case = &data["accumulator_walk"];
    let mut input = case["input"].clone();
    // ParticleType's factory answers -1 only for `<none>` (0x00645430); `none`
    // would bind a particle type of that name.
    input["type_overrides"] = serde_json::json!({"SmallGreySSys":{"HoldsWhat":"<none>"}});
    let rules = rules(&data, &input);
    assert_eq!(
        rules
            .particle_system_type(ParticleSystemTypeId(0))
            .holds_what,
        None
    );
    let mut sim = sim(false);
    let id = sim
        .spawn_particle_system(
            ParticleSystemTypeId(0),
            IVec3::new(11392, 12416, 0),
            None,
            None,
            IVec3::ZERO,
            None,
            &rules,
        )
        .unwrap();
    adopt_rng(&mut sim, case);
    let points = case["checkpoints"].as_array().unwrap();
    for frame in 0..=points.last().unwrap()["frame"].as_u64().unwrap() as u32 {
        sim.session.set_binary_frame_for_test(frame);
        super::super::system_ai::tick_particle_system(
            &mut sim,
            &rules,
            id,
            crate::sim::world::UninitContext::with_rules(&rules),
        );
        if let Some(point) = points.iter().find(|point| point["frame"] == frame) {
            assert_system(&sim, id, None, &point["state"]);
        }
    }
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        case["scenario_after"].as_str().unwrap()
    );
}

#[test]
fn smoke_scalar_and_air_authorities_survive_existing_snapshot_roundtrip() {
    let data = corpus();
    let following = crate::test_fixture::json("tools/spatial_oracle/damage_smoke_following.json");
    let case = &following["cases"][0];
    let mut input = case["input"].clone();
    input["owner"] = Value::Bool(true);
    let rules = rules(&data, &input);
    let mut sim = sim(false);
    let (id, owner) = install_system(&mut sim, &rules, &input, &case["before"]);
    adopt_rng(&mut sim, case);
    let owner_id = owner.unwrap();
    let c = coords(&input["steps"][0]["coords"]);
    sim.substrate
        .entities
        .get_mut(owner_id)
        .unwrap()
        .position
        .put_location(crate::sim::components::DriveCoord {
            x: c.x,
            y: c.y,
            z: c.z,
        });
    sim.session.set_binary_frame_for_test(0);
    super::super::system_ai::tick_particle_system(
        &mut sim,
        &rules,
        id,
        crate::sim::world::UninitContext::with_rules(&rules),
    );
    sim.session
        .set_map_name_for_test("damage-smoke-continuation".into());
    let bytes = GameSnapshot::save_validated(&sim, 17, 23, "Smoke continuation", 0);
    let mut restored = GameSnapshot::load_validated(&bytes, 17, 23, "damage-smoke-continuation")
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_system(&restored, id, owner, &case["visits"][0]);
    assert_eq!(
        restored.particle_systems().get(id).unwrap().offset,
        coords(&case["before"]["offset"])
    );
    // Existing load deliberately reseeds Scenario; this check changes no load
    // flow. Adopt the same tested continuation boundary for the gameplay owner.
    restored.scenario_rng =
        SimRng::from_native_state_hex_for_test(&sim.scenario_rng.native_state_hex());
    let c = coords(&input["steps"][1]["coords"]);
    for world in [&mut sim, &mut restored] {
        world
            .substrate
            .entities
            .get_mut(owner_id)
            .unwrap()
            .position
            .put_location(crate::sim::components::DriveCoord {
                x: c.x,
                y: c.y,
                z: c.z,
            });
    }
    super::super::system_ai::tick_particle_system(
        &mut sim,
        &rules,
        id,
        crate::sim::world::UninitContext::with_rules(&rules),
    );
    super::super::system_ai::tick_particle_system(
        &mut restored,
        &rules,
        id,
        crate::sim::world::UninitContext::with_rules(&rules),
    );
    assert_system(&restored, id, owner, &case["visits"][1]);
    let a = sim.particle_systems().get(id).unwrap();
    let b = restored.particle_systems().get(id).unwrap();
    assert_eq!(a.spawn_timer(), b.spawn_timer());
    assert_eq!(
        serde_json::to_value(a).unwrap(),
        serde_json::to_value(b).unwrap()
    );
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        restored.scenario_rng.native_state_hex()
    );
}
