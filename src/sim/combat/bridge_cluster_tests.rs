//! Joined ordinary Bullet -> Apply_area_damage -> bridge -> cluster coverage.
//! Native vectors execute 468D80/4690B0/489280 and the original Scenario RNG;
//! only bridge driver results and rendering calls are supplied at the boundary.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::bridge_state::BridgeRuntimeState;
use crate::sim::projectile::ProjectileDetonationReason;
use crate::sim::world::Simulation;
use serde_json::Value;

fn corpus() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../tools/projectile_oracle/bridge_cluster_order.json"
    ))
    .unwrap()
}

fn rules(input: &Value) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[VehicleTypes]\n0=SHOOTER\n\
         [SHOOTER]\nStrength=100\nPrimary=SHOT\n\
         [SHOT]\nDamage={}\nProjectile=SHELL\nWarhead=WH\n\
         [SHELL]\nArcing=yes\nCluster={}\n\
         [WH]\nCellSpread=0\nWall={}\nMaxDebris=0\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        input["damage"].as_i64().unwrap(),
        input["cluster"].as_i64().unwrap(),
        if input["wall"].as_bool().unwrap() {
            "yes"
        } else {
            "no"
        },
    )))
    .unwrap()
}

fn world(rules: &RuleSet, input: &Value) -> Simulation {
    let cells = (0..64)
        .flat_map(|y| {
            (0..64).map(move |x| {
                let mut cell = crate::map::resolved_terrain::test_flat_cell(x, y);
                cell.level = input["level"].as_i64().unwrap() as u8;
                cell.final_tile_index = -1;
                cell
            })
        })
        .collect();
    let mut terrain = ResolvedTerrainGrid::from_cells(64, 64, cells);
    let anchor = terrain.native_cell_identity((9, 20));
    terrain.cell_mut(9, 20).unwrap().bridge_facts.overlay_id =
        Some(input["anchor_overlay"].as_u64().unwrap() as u8);
    // The oracle supplies false at the bridge driver boundary. Use the live
    // driver's no-transition state here, without a test hook or bridge mutation.
    // The wooden fallback has no anchor span and likewise returns no change.
    // This fixture compares admission/continuation, not valid bridge topology.
    terrain.write_native_cell_state(anchor, 255);
    for y in 17..24 {
        for x in 7..14 {
            if (x, y) == (10, 20) || (input["bridge_patch"].as_bool().unwrap() && (x, y) != (9, 20))
            {
                let cell = terrain.cell_mut(x, y).unwrap();
                cell.bridge_facts.raw_flags = input["flags"].as_u64().unwrap() as u32;
                cell.bridge_facts.native_anchor = Some(anchor);
            }
        }
    }
    let bridges = BridgeRuntimeState::from_resolved_terrain(
        &terrain,
        input["destroyable"].as_bool().unwrap(),
        input["strength"].as_i64().unwrap() as i32,
    );
    let mut sim = Simulation::with_seed(input["seed"].as_u64().unwrap());
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.install_resolved_terrain_for_new_map(terrain);
    sim.bridge_state = Some(bridges);
    sim.session.no_damage = input["no_damage"].as_bool().unwrap();
    sim
}

fn detonation(sim: &mut Simulation, input: &Value) -> ProjectileDetonation {
    ProjectileDetonation {
        projectile_id: 1,
        source_id: RAD_NO_ATTACKER,
        target: ProjectileTarget::Cell { rx: 10, ry: 20 },
        impact: ProjectileCoord::new(2688, 5248, input["impact_z"].as_i64().unwrap() as i32),
        payload: ProjectilePayload {
            base_damage: input["damage"].as_i64().unwrap() as i32,
            warhead: sim.interner.intern("WH"),
            weapon: sim.interner.intern("SHOT"),
        },
        reason: ProjectileDetonationReason::ReachedTarget,
    }
}

#[test]
fn ordinary_bridge_continuation_precedes_cluster_rng_like_native() {
    for row in corpus() {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let rules = rules(input);
        let mut sim = world(&rules, input);
        let mut shot = detonation(&mut sim, input);
        // This production seam starts at the resolved detonation coordinate.
        // Cell impact-ladder vectors separately cover the preceding 468D80 arm.
        if let Some(first) = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["kind"] == "detonate")
        {
            let coord = first["coord"].as_array().unwrap();
            shot.impact = ProjectileCoord::new(
                coord[0].as_i64().unwrap() as i32,
                coord[1].as_i64().unwrap() as i32,
                coord[2].as_i64().unwrap() as i32,
            );
        }
        let mut run = world_receiver::ReceiverRun::default();
        let commit = world_receiver::commit_projectiles(&mut sim, &mut run, &[shot], &rules, None);
        run.finish(&mut sim);
        assert!(!commit.effects.bridge_state_changed, "{name}");
        assert!(commit.effects.explosion_effects.is_empty(), "{name}");
        let dirty: Vec<_> = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["kind"] == "driver_false")
            .map(|event| {
                (
                    event["coord"][0].as_u64().unwrap() as u16,
                    event["coord"][1].as_u64().unwrap() as u16,
                )
            })
            .collect();
        assert_eq!(sim.tactical_dirty_cells, dirty, "{name}");
        let rng = sim.scenario_rng.logical_view();
        assert_eq!(
            serde_json::json!([rng.index_a, rng.index_b]),
            row["rng_indices"],
            "{name}"
        );
        let next: Vec<_> = (0..4).map(|_| sim.scenario_rng.next_u32()).collect();
        assert_eq!(serde_json::json!(next), row["next_rng"], "{name}");
    }
}

#[test]
fn nested_death_bridge_continuation_finishes_before_parent_area_returns() {
    let native: Value = serde_json::from_str(include_str!(
        "../../../tools/projectile_oracle/bridge_nested_draws.json"
    ))
    .unwrap();
    let mut input = corpus()
        .into_iter()
        .find(|row| row["input"]["name"] == "s1_strength_65536")
        .unwrap()["input"]
        .clone();
    input["seed"] = native["seed"].clone();
    let draws = native["draws"].as_array().unwrap();
    assert!(draws[0].as_i64().unwrap() < 29_000);
    assert!(draws[1].as_i64().unwrap() >= 29_000);

    // Control-flow evidence: 489280 dispatches each receiver synchronously;
    // 70266D -> 70D690 -> 4690B0 recursively enters the death weapon's area.
    // The inner bridge tail must finish before the parent's 489E87 tail.
    // This is a Rust nested-receiver regression using original scalar RNG
    // goldens, not an executable comparison of the full native death chain.
    for (outer_damage, expected_dirty) in [(29_000, vec![]), (31_000, vec![(10, 20)])] {
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[VehicleTypes]\n0=OUTER\n1=INNER\n\
             [OUTER]\nStrength=100\nArmor=heavy\nExplodes=yes\nDeathWeapon=OuterBlast\n\
             [INNER]\nStrength=100\nArmor=heavy\nExplodes=yes\nDeathWeapon=InnerBlast\n\
             [OuterBlast]\nDamage={outer_damage}\nWarhead=WH\n\
             [InnerBlast]\nDamage=-1\nWarhead=WH\n\
             [WH]\nCellSpread=0\nWall=yes\nMaxDebris=0\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
        )))
        .unwrap();
        let mut sim = world(&rules, &input);
        let mut units = Vec::new();
        for name in ["OUTER", "INNER"] {
            let id = sim
                .construct_object_limbo_at_height(name, "Americans", 10, 20, 0, 6, &rules)
                .unwrap();
            // Both objects are in the deck list at the identical coordinate,
            // so OUTER's zero-spread death blast includes only the live INNER.
            sim.substrate.entities.get_mut(id).unwrap().on_bridge = true;
            sim.reveal_entity_with_rules(id, &rules);
            assert!(
                sim.substrate
                    .entities
                    .get(id)
                    .unwrap()
                    .lifecycle
                    .cell_marked
            );
            units.push(id);
        }
        sim.scenario_rng = SimRng::new(native["seed"].as_u64().unwrap());
        sim.tactical_dirty_cells.clear();
        let warhead = sim.interner.intern("WH");
        let hit = EntityDamageEvent::area(units[0], 100, 0, RAD_NO_ATTACKER, None, warhead);
        let mut run = world_receiver::ReceiverRun::default();
        let (effects, _) = world_receiver::commit_area(
            &mut sim,
            &mut run,
            &[combat_aoe::AreaDamageReceiver::Entity(hit)],
            &rules,
            None,
        );
        assert_eq!(run.handled_deaths, units, "both recursive receivers ran");
        run.finish(&mut sim);
        assert!(!effects.bridge_state_changed);
        // INNER's signed -1 packet still draws first and cannot pass. OUTER
        // then compares the second native result:29000 fails,31000 succeeds.
        assert_eq!(sim.tactical_dirty_cells, expected_dirty, "{outer_damage}");
        let rng = sim.scenario_rng.logical_view();
        assert_eq!(
            serde_json::json!([rng.index_a, rng.index_b]),
            native["rng_indices"],
            "{outer_damage}"
        );
        let next: Vec<_> = (0..4).map(|_| sim.scenario_rng.next_u32()).collect();
        assert_eq!(
            serde_json::json!(next),
            native["next_rng"],
            "{outer_damage}"
        );
    }
}
