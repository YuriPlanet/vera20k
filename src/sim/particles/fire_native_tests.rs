//! Native comparison: `tools/procedural_drawing_oracle/fire_particles.json`
//! runs the original ParticleSystem constructor (`0x0062DC50`) as FireAt
//! builds a Fire system, then live Logic through AI_Fire (`0x0062F9A0`), the
//! Fire particle AI (`0x0062CB10`), Move_Dispatch (`0x0062D5E0`) and
//! SpawnParticleWithInsert (`0x0062E4C0`) to retirement. The owner's facts
//! replay each case's script; the damage walk runs on real cell lists, and
//! its ReceiveDamage calls are recorded as the native stubs record theirs.

use super::fire::{FireHit, FireWorld, OwnerFacts, OwnerSweep};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::native_identity::NativeUniqueIdCursor;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::rng::SimRng;
use crate::sim::world::{Simulation, UninitContext};
use glam::IVec3;
use serde_json::{Value, json};
use std::collections::HashMap;

fn int(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

fn xyz(value: &Value) -> IVec3 {
    IVec3::new(int(&value[0]), int(&value[1]), int(&value[2]))
}

/// The case's owner script in place of the live owner, and every hit kept.
struct Replay<'a> {
    case: &'a Value,
    visit: usize,
    swept: bool,
    hits: Vec<(usize, FireHit)>,
}

impl FireWorld for Replay<'_> {
    fn owner(&mut self, _: &Simulation, _: &RuleSet, owner: Option<u64>) -> Option<OwnerFacts> {
        assert!(owner.is_some(), "FireAt's system keeps its owner");
        let script = &self.case["script"][self.visit - 1];
        let alive = script["alive"].as_bool().unwrap();
        self.swept = alive
            && script["has_target"].as_bool().unwrap()
            && script["rotating"].as_bool().unwrap();
        Some(OwnerFacts {
            alive,
            sweep: self.swept.then(|| OwnerSweep {
                target: xyz(&self.case["target"]),
                location: xyz(&self.case["owner"]),
                facing: script["facing"].as_u64().unwrap() as u16,
                flh: xyz(&self.case["flh"]),
            }),
        })
    }

    fn receive_damage(&mut self, _: &mut Simulation, _: &RuleSet, hit: FireHit) {
        self.hits.push((self.visit, hit));
    }
}

fn particle_state(p: &super::Particle) -> Value {
    json!({
        "native_id": p.native_unique_id(),
        "location": p.coords.to_array(),
        "step": p.prev_delta,
        "direction_bits": p.direction.map(|axis| axis.bits()),
        "speed_bits": p.velocity.bits(),
        "remaining": p.lifetime_remaining as u16,
        "damage_counter": p.damage_counter,
        "advance": p.state_ai_advance,
        "state": p.animation_state,
        "translucency": p.translucency,
        "marked": p.marked_for_deletion,
    })
}

#[test]
fn fire_system_matches_native_sweep_spawns_particles_and_damage_to_retirement() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/fire_particles.json",
    ))
    .unwrap();
    let type_id = rules.ps_type_id_by_name("FireStreamSys").unwrap();
    let first_frame = corpus["first_frame"].as_u64().unwrap() as u32;
    for case in corpus["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let mut sim = crate::sim::arena_fixture::rival_arena_with_size(rules, 1, 64);
        sim.session.set_binary_frame_for_test(first_frame);
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);

        // Real objects at the stubs' Locations, in the same cells.
        let place = |sim: &mut Simulation, at: &Value| {
            let at = xyz(at);
            let id = sim
                .spawn_object(
                    "MTNK",
                    "Americans",
                    (at.x >> 8) as u16,
                    (at.y >> 8) as u16,
                    0,
                    rules,
                )
                .unwrap();
            let location = crate::sim::movement::ground_pose::object_location(
                sim.substrate.entities.get(id).unwrap(),
                sim.resolved_terrain.as_ref(),
            );
            assert_eq!([location.x, location.y, location.z], at.to_array());
            id
        };
        let owner = place(&mut sim, &case["owner"]);
        let mut names = HashMap::from([(owner, "owner")]);
        for object in case["objects"].as_array().unwrap() {
            let id = place(&mut sim, &object["location"]);
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            if let Some(strength) = object.get("strength") {
                entity.health.set_for_test(int(strength));
            }
            if object.get("alive") == Some(&json!(false)) {
                entity.lifecycle.set_object_alive_for_test(false);
            }
            names.insert(id, object["name"].as_str().unwrap());
        }
        sim.set_logic_order_for_test(Vec::new());
        sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(1000));
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(case["initial_scenario_rng"].as_str().unwrap());
        let target = xyz(&case["target"]);
        let id = sim
            .spawn_particle_system(
                type_id,
                xyz(&case["flh"]),
                Some(TargetKind::Cell(
                    (target.x >> 8) as u16,
                    (target.y >> 8) as u16,
                )),
                Some(owner),
                IVec3::ZERO,
                None,
                rules,
            )
            .unwrap();

        let mut world = Replay {
            case,
            visit: 0,
            swept: false,
            hits: Vec::new(),
        };
        let mut systems = Vec::new();
        for visit in 1..=corpus["visits"].as_u64().unwrap() as usize {
            sim.session
                .set_binary_frame_for_test(first_frame + visit as u32);
            (world.visit, world.swept) = (visit, false);
            if sim
                .particle_systems()
                .get(id)
                .is_some_and(|system| system.in_logic_vector())
            {
                super::fire::tick_system_with(&mut sim, rules, id, &mut world);
                super::system_ai::finish_system_visit(
                    &mut sim,
                    id,
                    UninitContext::with_rules(rules),
                );
            }
            assert_eq!(
                world.swept,
                case["swept"][visit - 1].as_bool().unwrap(),
                "{name}: owner sweep read at visit {visit}"
            );
            let system = sim
                .particle_systems()
                .get(id)
                .filter(|system| system.in_logic_vector());
            assert_eq!(
                system.map_or(-1, |system| system.particles().len() as i64),
                case["live_counts"][visit - 1].as_i64().unwrap(),
                "{name}: live particles after visit {visit}"
            );
            if let Some(system) = system {
                let [x, y, z] = system.coords.to_array();
                let [tx, ty, tz] = system.target_coords.to_array();
                systems.push(json!([
                    x,
                    y,
                    z,
                    tx,
                    ty,
                    tz,
                    i32::from(system.done_spawning)
                ]));
            }
            if let Some(expected) = case["particles_after_visit"].get(visit.to_string()) {
                let actual: Vec<Value> = system
                    .map(|system| system.particles().iter().map(particle_state).collect())
                    .unwrap_or_default();
                assert_eq!(
                    Value::Array(actual),
                    *expected,
                    "{name}: particles after visit {visit}"
                );
            }
        }
        assert_eq!(
            Value::Array(systems),
            case["systems"],
            "{name}: system location, aim and done byte"
        );
        let warhead = sim.interner.intern("Fire");
        let hits: Vec<Value> = world
            .hits
            .iter()
            .map(|(visit, hit)| {
                assert_eq!(hit.warhead, warhead, "{name}: FireStream's Warhead");
                let CellObjectMember::Entity(receiver) = hit.receiver else {
                    panic!("{name}: only Technos stand in the cells");
                };
                json!([visit, names[&receiver], hit.damage, hit.distance])
            })
            .collect();
        assert_eq!(
            Value::Array(hits),
            case["hits"],
            "{name}: ReceiveDamage calls"
        );
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            case["final_scenario_rng"].as_str().unwrap(),
            "{name}: Scenario draws"
        );
    }
}
