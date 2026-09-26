//! Cell target +58 through the FireAt launch owner. The expected coordinates
//! come from original FireAt and Bullet::Fire with the real CellClass getters,
//! tools/spatial_oracle/bridge_damage_admission.py (`aim_cases`).

use super::*;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::projectile::ProjectileCoord;
use crate::sim::world::Simulation;
use serde_json::Value;

fn aim_cases() -> Vec<Value> {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/bridge_damage_admission.json"
    ))
    .unwrap();
    corpus["aim_cases"].as_array().unwrap().clone()
}

fn expected_coord(row: &Value, key: &str) -> ProjectileCoord {
    let values = row[key].as_array().unwrap();
    ProjectileCoord::new(
        values[0].as_i64().unwrap() as i32,
        values[1].as_i64().unwrap() as i32,
        values[2].as_i64().unwrap() as i32,
    )
}

#[test]
fn cell_launch_aim_matches_original_fireat_and_bullet_fire() {
    let entities = EntityStore::new();
    for row in aim_cases() {
        let input = &row["input"];
        let mut cells = (0..32)
            .flat_map(|y| (0..32).map(move |x| crate::map::resolved_terrain::test_flat_cell(x, y)))
            .collect::<Vec<_>>();
        let cell = &mut cells[20 * 32 + 10];
        cell.level = input["level"].as_i64().unwrap() as u8;
        cell.slope_type = input["slope"].as_u64().unwrap() as u8;
        cell.bridge_facts.raw_flags = input["flags"].as_u64().unwrap() as u32;
        // Derived walkability is deliberately absent: native reads raw 0x100.
        let terrain = ResolvedTerrainGrid::from_cells(32, 32, cells);
        let z = attack_world_z_leptons(TargetKind::Cell(10, 20), &entities, Some(&terrain));
        let expected = expected_coord(&row, "fireat_aim");
        assert_eq!(z, expected.z, "FireAt {input}");
        assert_eq!(
            crate::sim::projectile::cell_target_coord(Some(&terrain), 10, 20),
            expected_coord(&row, "bullet_frozen_target"),
            "Bullet::Fire {input}"
        );
    }
}

#[test]
fn retail_grizzly_forcefire_freezes_the_native_cell_aim() {
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let Some(art) = crate::rules::retail_ini_fixture::retail_ini("artmd.ini") else {
        return;
    };
    let mut rules = RuleSet::from_ini(&ini).expect("retail rules");
    rules.merge_art_data(&crate::rules::art_data::ArtRegistry::from_ini(&art));
    assert_eq!(
        rules.object("MTNK").unwrap().primary.as_deref(),
        Some("105mm")
    );
    assert_eq!(
        rules.weapon("105mm").unwrap().projectile.as_deref(),
        Some("Cannon")
    );

    for row in aim_cases().into_iter().filter(|row| {
        row["input"]["level"] == 2
            && row["input"]["slope"] == 0
            && matches!(row["input"]["flags"].as_u64(), Some(0x100 | 0x400))
    }) {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
        );
        sim.session.house_order.push(owner);
        let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        for (x, y) in [(10, 16), (10, 20)] {
            terrain.cell_mut(x, y).unwrap().level = 2;
        }
        terrain.cell_mut(10, 20).unwrap().bridge_facts.raw_flags =
            row["input"]["flags"].as_u64().unwrap() as u32;
        let heights = std::collections::BTreeMap::from([((10, 16), 2)]);
        let grizzly = sim
            .spawn_object("MTNK", "Americans", 10, 16, 128, &rules, &heights)
            .expect("retail Grizzly");
        sim.resolve_type_handles(&rules);
        sim.queue_command(CommandEnvelope::new(
            owner,
            sim.session.tick + 1,
            Command::ForceAttackCell {
                attacker_id: grizzly,
                target_rx: 10,
                target_ry: 20,
            },
        ));
        let expected = expected_coord(&row, "bullet_frozen_target");
        let mut launched = false;
        for _ in 0..200 {
            let commands = sim.take_due_commands();
            sim.advance_tick(&commands, Some(&rules), &heights, Some(&grid), None, 67);
            if let Some((_, shell)) = sim.projectiles.iter().find(|(_, p)| p.source_id == grizzly) {
                assert_eq!(
                    shell.launch_target, expected,
                    "retail shot {}",
                    row["input"]
                );
                assert_eq!(shell.last_target_position, expected);
                launched = true;
                break;
            }
        }
        assert!(launched, "retail Grizzly never launched: {}", row["input"]);
    }
}
