//! Original Anim423930 -> Terrain71B920 comparisons. Physics is separately
//! compared in the joined bridge producer/flight corpus.
use super::*;
use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile, retail_ini_fixture::retail_ini};
use crate::sim::occupancy::CellObjectMember;

#[test]
fn debris_contact_matches_original_tree_damage_gates_radius_and_retirement() {
    let Some(retail) = retail_ini("rulesmd.ini") else {
        return;
    };
    let Some(art_ini) = retail_ini("artmd.ini") else {
        return;
    };
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/terrain_debris_receiver.json",
    ))
    .unwrap();
    let mut compared = 0;
    for row in native["cases"].as_array().unwrap() {
        let input = &row["case"];
        // The source corpus also bounds synthetic two-tree lists and a custom
        // SpawnsTiberium death with failed allocation. Neither is the loaded
        // single plain tree modeled here; do not claim those as Rust parity.
        if input["contact"] != true || input["second"] == true || input["spawns"] == true {
            continue;
        }
        let label = input["name"].as_str().unwrap();
        let mut ini = retail.clone();
        let wood = if input["wood"] == 0 { "no" } else { "yes" };
        let immune = if input["immune"] == 1 { "yes" } else { "no" };
        ini.merge(&IniFile::from_str(&format!(
            "[HE]\nWood={wood}\n[TREE01]\nImmune={immune}\n"
        )));
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art_ini).unwrap();
        rules.install_art_data(ArtRegistry::from_ini(&art_ini));
        let mut config = rules.art().anim_runtime_config("DBRIS1LG").unwrap().clone();
        let damage = input["damage"].as_i64().unwrap_or(20) as i32;
        config.damage = NativeF64Bits::from_bits(f64::from(damage).to_bits());
        config.damage_radius = input["radius"].as_i64().unwrap_or(80) as i32;
        if input["null_warhead"] == 1 {
            config.warhead = None;
        }
        let mut sim = Simulation::with_seed(1);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        crate::sim::terrain_spawn::construct_terrain_objects(
            &mut sim,
            &[crate::map::overlay::TerrainObject {
                rx: 10,
                ry: 20,
                name: "TREE01".into(),
            }],
            &rules,
            false,
        );
        let id = sim.production.terrain_object_cells[&(10, 20)];
        sim.production.terrain_objects.get_mut(&id).unwrap().health =
            input["health"].as_i64().unwrap() as i32;
        // Preserve unrelated raw bits to expose over-broad occupation clears.
        sim.substrate.raw_cell_occupation.mark_ground(10, 20, 0xFC);
        let before_rng = sim.scenario_rng.clone();
        let position = glam::IVec3::new(
            2688 + input["offset_x"].as_i64().unwrap_or(0) as i32,
            5248,
            208,
        );
        assert!(
            !sim.anim_bounce_contact(0, &config, &rules, None, position),
            "{label}"
        );
        let tree = &sim.production.terrain_objects[&id];
        assert_eq!(
            tree.health,
            row["health"].as_i64().unwrap() as i32,
            "{label}"
        );
        assert_eq!(tree.is_live(), row["alive"] == 1, "{label}");
        assert_eq!(tree.in_logic_vector, row["in_logic"] == 1, "{label}");
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(10, 20),
            row["occupation"].as_u64().unwrap() as u8,
            "{label}"
        );
        assert_eq!(
            sim.substrate
                .pending_delete
                .iter()
                .filter(|&&queued| queued == id)
                .count(),
            row["delete_count"].as_u64().unwrap() as usize,
            "{label}"
        );
        assert_eq!(
            sim.cell_objects(
                (10, 20),
                crate::sim::movement::locomotor::MovementLayer::Ground
            )
            .next(),
            (row["ground_head"] != 0).then_some(CellObjectMember::Terrain(id)),
            "{label}"
        );
        assert_eq!(
            serde_json::to_value(&sim.scenario_rng).unwrap(),
            serde_json::to_value(&before_rng).unwrap(),
            "{label}"
        );
        let continuation: Vec<u32> = (0..4).map(|_| sim.scenario_rng.next_u32()).collect();
        assert_eq!(serde_json::json!(continuation), row["next_rng"], "{label}");
        compared += 1;
    }
    assert_eq!(compared, 19);
}
