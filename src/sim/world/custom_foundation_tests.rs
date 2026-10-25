//! Custom foundations through the existing construct/Unlimbo/Mark/restore owners.

use super::Simulation;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::snapshot::GameSnapshot;
use std::collections::BTreeMap;

#[test]
fn custom_foundation_mark_navigation_outline_restore_and_uninit() {
    for row in crate::rules::foundation::custom_tests::native_rows() {
        let rules = crate::rules::foundation::custom_tests::rules(&row);
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x568411);
        sim.rebuild_dynamic_navigation(&rules);
        let id = sim
            .spawn_object("CUSTOM_TEST", "Americans", 20, 20, 0, &rules)
            .expect("synthetic custom Building through production Unlimbo");
        let entity = sim.entities().get(id).unwrap();
        assert!(entity.lifecycle.cell_marked());
        assert_eq!(
            entity.type_facts.foundation(),
            &rules.object("CUSTOM_TEST").unwrap().foundation
        );
        let occupied: Vec<_> = row["installed"]["cells"]
            .as_array()
            .unwrap()
            .iter()
            .take_while(|p| p[0] != 32767 || p[1] != 32767)
            .map(|p| {
                (
                    (20 + p[0].as_i64().unwrap()) as u16,
                    (20 + p[1].as_i64().unwrap()) as u16,
                )
            })
            .collect();
        assert!(sim.rebuild_dynamic_navigation(&rules));
        for y in 18..26 {
            for x in 18..25 {
                assert_eq!(
                    sim.path_grid_snapshot().unwrap().is_walkable(x, y),
                    !occupied.contains(&(x, y)),
                    "{} cell{x},{y}",
                    row["name"]
                );
                assert_eq!(
                    sim.entities()
                        .get(id)
                        .unwrap()
                        .footprint_contains_cell((x, y)),
                    occupied.contains(&(x, y)),
                    "presentation membership reads the same native occupied cells"
                );
                let present = sim
                    .cell_objects((x, y), MovementLayer::Ground)
                    .any(|member| {
                        matches!(member,
                        crate::sim::occupancy::CellObjectMember::Entity(found) if found == id)
                    });
                assert_eq!(present, occupied.contains(&(x, y)));
            }
        }
        // ART Height=3 supplies the original caller's max(Height-1,1)=2.
        // The native witness explicitly provides depth2 to the same Ares hook.
        assert_eq!(
            rules
                .object("CUSTOM_TEST")
                .unwrap()
                .hidden_occupancy
                .occupy_height,
            3
        );
        let expected: BTreeMap<_, _> = row["mark"]["after_add"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                (
                    (p[0].as_u64().unwrap() as u16, p[1].as_u64().unwrap() as u16),
                    p[2].as_u64().unwrap() as u32,
                )
            })
            .collect();
        let actual: BTreeMap<_, _> = sim
            .substrate
            .hidden_occupation
            .entries()
            .map(|(x, y, count)| ((x, y), count))
            .collect();
        assert_eq!(actual, expected);

        let terrain = sim.resolved_terrain.as_ref().unwrap().clone();
        let bytes = GameSnapshot::save(&sim, 0, 0, "custom-foundation.map", 0);
        let mut restored: Simulation = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        restored.install_resolved_terrain_for_new_map(terrain);
        restored.rebuild_dynamic_navigation(&rules);
        assert_eq!(
            restored.entities().get(id).unwrap().type_facts.foundation(),
            &rules.object("CUSTOM_TEST").unwrap().foundation
        );
        for &(x, y) in &occupied {
            assert!(!restored.path_grid_snapshot().unwrap().is_walkable(x, y));
        }
        restored.uninit_with_rules(id, &rules);
        restored.flush_pending_delete();
        restored.rebuild_dynamic_navigation(&rules);
        assert!(restored.entities().get(id).is_none());
        assert_eq!(restored.substrate.hidden_occupation.entry_count(), 0);
        for &(x, y) in &occupied {
            assert!(restored.path_grid_snapshot().unwrap().is_walkable(x, y));
            assert!(
                !restored
                    .cell_objects((x, y), MovementLayer::Ground)
                    .any(|member| matches!(member,
                    crate::sim::occupancy::CellObjectMember::Entity(found) if found == id))
            );
        }
    }
}
