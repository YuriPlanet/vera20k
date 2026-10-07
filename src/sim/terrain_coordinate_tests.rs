//! Native Terrain placement coordinates retained through map changes and saves.
use super::*;
use crate::map::overlay::TerrainObject;
use crate::rules::retail_ini_fixture::retail_ini;
use crate::sim::combat::combat_aoe::{
    AoEAirImpact, AoELayerContext, AreaDamageReceiver, TerrainCollectionView,
    apply_aoe_damage_with_terrain,
};
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::Simulation;
use crate::util::lepton::CELL_CENTER_LEPTON;
use std::hash::{Hash, Hasher};

#[test]
fn bridge_neighbor_terrain_retains_native_surface_coords_through_damage_and_restore() {
    let Some(ini) = retail_ini("rulesmd.ini") else {
        return;
    };
    let rules = RuleSet::from_ini(&ini).unwrap();
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/terrain_coordinate.json",
    ))
    .unwrap();
    for row in corpus["cases"].as_array().unwrap() {
        let input = &row["input"];
        // Authored Terrain construction always supplies inputZ0. The extra
        // native above-ground input characterizes a different caller domain.
        if input["input_z"] != 0 {
            continue;
        }
        let mut sim = Simulation::with_seed(1);
        let mut grid = crate::map::resolved_terrain::test_flat_ground_grid(32);
        let cell = grid.cell_mut(10, 20).unwrap();
        cell.level = input["level"].as_i64().unwrap() as i8 as u8;
        cell.slope_type = input["slope"].as_u64().unwrap() as u8;
        cell.bridge_facts.raw_flags = input["flags"].as_u64().unwrap() as u32;
        sim.install_resolved_terrain_for_new_map(grid);
        crate::sim::terrain_spawn::construct_terrain_objects(
            &mut sim,
            &[TerrainObject {
                rx: 10,
                ry: 20,
                name: "TREE01".into(),
            }],
            &rules,
            false,
        );
        let id = sim.production.terrain_object_cells[&(10, 20)];
        let coord = sim.production.terrain_objects[&id].world_coord();
        assert_eq!(
            serde_json::json!([coord.x, coord.y, coord.z]),
            row["placement"],
            "{input}"
        );
        let mut higher = sim.production.terrain_objects[&id].clone();
        higher.world_z_leptons += 1;
        let hash = |tree: &TerrainObjectState| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            tree.hash(&mut hasher);
            hasher.finish()
        };
        assert_ne!(hash(&higher), hash(&sim.production.terrain_objects[&id]));

        let cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(10, 20)
            .unwrap();
        cell.level = 7;
        cell.slope_type = 0;
        cell.bridge_facts.raw_flags = 0;
        assert_eq!(
            serde_json::json!([
                coord.x,
                coord.y,
                sim.production.terrain_objects[&id].world_coord().z
            ]),
            row["after_ground_change"],
            "{input}"
        );
        let changed_grid = sim.resolved_terrain.as_ref().unwrap().clone();
        let bytes = GameSnapshot::save(&sim, 0, 0, "terrain-coordinate", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        restored.rebuild_caches_after_load(
            changed_grid,
            crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            &rules,
        );
        assert_eq!(
            restored.production.terrain_objects[&id].world_coord(),
            coord,
            "{input}"
        );

        // Consumer regression: an impact at the native retained coordinate
        // must still find the object after the ground under it has moved.
        let warhead = rules.warhead("HE").unwrap();
        let warhead_ref = restored.interner.intern("HE");
        let occupancy = OccupancyGrid::new();
        let result = apply_aoe_damage_with_terrain(
            &mut restored.substrate.entities,
            10,
            20,
            20,
            warhead,
            &rules,
            &mut restored.interner,
            (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
            AoELayerContext {
                occupancy: Some(&occupancy),
                terrain: restored.resolved_terrain.as_mut(),
                air_impact: Some(AoEAirImpact {
                    sub_x: CELL_CENTER_LEPTON,
                    sub_y: CELL_CENTER_LEPTON,
                    z_leptons: coord.z,
                }),
                ..AoELayerContext::default()
            },
            Some(TerrainCollectionView {
                objects: &restored.production.terrain_objects,
                cells: &restored.production.terrain_object_cells,
            }),
        );
        let terrain_hit = result
            .receivers
            .iter()
            .find_map(|receiver| match receiver {
                AreaDamageReceiver::Terrain(hit) if hit.stable_id == id => Some(hit),
                _ => None,
            })
            .expect("area collector must use the retained object coordinate");
        assert_eq!(terrain_hit.distance_leptons, 0, "{input}");
    }
}
