//! Original Object GetHeight/low/high queries from retained physical XYZ.
use super::*;
use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
use crate::sim::game_entity::GameEntity;
use serde::Deserialize;

#[derive(Deserialize)]
struct Input {
    name: String,
    marked: u8,
    on_bridge: u8,
    xyz: [i32; 3],
    cell_level: u8,
}
#[derive(Deserialize)]
struct Row {
    input: Input,
    height: i32,
    low_flying: u8,
    high_flying: u8,
    dummy_xy: [i16; 2],
}
#[derive(Deserialize)]
struct Corpus {
    native_sha256: String,
    rows: Vec<Row>,
}

#[test]
fn native_object_flight_queries_use_live_ground_bridge_and_mark() {
    let corpus: Corpus = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/object_flight_height.json",
    ))
    .unwrap();
    assert_eq!(
        corpus.native_sha256,
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    assert_eq!(corpus.rows.len(), 24);
    for row in corpus.rows {
        let mut cells: Vec<_> = (0..25)
            .flat_map(|y| (0..25).map(move |x| test_flat_cell(x, y)))
            .collect();
        cells[20 * 25 + 10].level = row.input.cell_level;
        cells[20 * 25 + 10].bridge_facts.raw_flags = 0x100;
        let mut terrain = ResolvedTerrainGrid::from_cells(25, 25, cells);
        terrain.test_set_native_allocated_cells(&[(10, 20)]);
        let dummy = terrain.shared_cell_dummy();
        dummy.stamp_coord(111, -222);
        let [x, y, z] = row.input.xyz;
        let mut entity =
            GameEntity::test_default(1, "FV", "Americans", (x / 256) as u16, (y / 256) as u16);
        entity.position.sub_x = SimFixed::from_num(x % 256);
        entity.position.sub_y = SimFixed::from_num(y % 256);
        entity.position.exact_z_leptons = Some(z);
        entity.on_bridge = row.input.on_bridge != 0;
        entity.lifecycle.cell_marked = row.input.marked != 0;
        assert_eq!(
            current_fly_height(&entity, Some(&terrain)),
            row.height,
            "{} height",
            row.input.name
        );
        assert_eq!(
            is_low_flying(&entity, Some(&terrain), None),
            row.low_flying != 0,
            "{} low",
            row.input.name
        );
        assert_eq!(
            is_high_flying(&entity, Some(&terrain), None),
            row.high_flying != 0,
            "{} high",
            row.input.name
        );
        assert_eq!(
            dummy.snapshot().coord,
            (i32::from(row.dummy_xy[0]), i32::from(row.dummy_xy[1])),
            "{} dummy",
            row.input.name
        );
    }
}

/// Paradrop (`0x005F5940`) places the falling object at the drop coordinate.
/// Each frame `ObjectClass::AI`'s falling block moves its Location Z by the
/// FallRate (`0x005F3F2C..0x005F3F60`) until GetHeight (`0x005F5F40`) is at
/// most 0 (`0x005F3F6A`), then SetHeight(0) grounds it. The flight queries
/// read that height: IsInAir (`0x005F6B90`) holds from two levels up. On a
/// bridge GetHeight subtracts the deck (`0x005F5F86`), so the fall ends on it.
#[test]
fn a_paratroopers_fall_moves_its_location_z_to_the_ground_or_deck() {
    use crate::sim::movement::parachute_descent::begin_parachute_descent;
    use crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS;

    for (on_bridge, floor) in [(false, 0), (true, BRIDGE_DECK_HEIGHT_LEPTONS)] {
        let mut sim = crate::sim::world::Simulation::new();
        sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(
            25,
            25,
            (0..25)
                .flat_map(|y| (0..25).map(move |x| test_flat_cell(x, y)))
                .collect(),
        ));
        let mut paratrooper = GameEntity::test_default(1, "E1", "Americans", 10, 10);
        paratrooper.lifecycle.cell_marked = true;
        paratrooper.on_bridge = on_bridge;
        sim.substrate.entities.insert(paratrooper);
        assert!(begin_parachute_descent(
            &mut sim.substrate.entities,
            1,
            floor + 212
        ));
        fn entity(sim: &crate::sim::world::Simulation) -> &GameEntity {
            sim.substrate.entities.get(1).unwrap()
        }

        // FallRate 0, -1, -2: the object hangs a frame, then sinks to 209.
        for _ in 0..3 {
            assert!(!sim.advance_fall(1, -3, None, None));
        }
        let t = sim.resolved_terrain.clone();
        assert_eq!(current_fly_height(entity(&sim), t.as_ref()), 209);
        assert!(is_high_flying(entity(&sim), t.as_ref(), None));
        assert!(!sim.advance_fall(1, -3, None, None));
        assert_eq!(current_fly_height(entity(&sim), t.as_ref()), 206);
        assert!(is_low_flying(entity(&sim), t.as_ref(), None));

        let mut frames = 4;
        while !sim.advance_fall(1, -3, None, None) {
            frames += 1;
            assert!(frames < 100, "the fall must ground");
        }
        let landed = entity(&sim);
        assert!(!landed.is_falling_down());
        assert_eq!(landed.position.exact_z_leptons, Some(floor), "SetHeight(0)");
        assert_eq!(current_fly_height(landed, t.as_ref()), 0);
    }
}

/// A paratrooper stays in its cell's list as it falls: Walk's layer query
/// (`0x0075C7E0`) answers Ground at any height. `ObjectClass::AI`'s falling
/// block marks it again around each Z write (`0x005F3F46`, `0x005F3F58`), so
/// it is prepended ahead of an object that entered the cell after it.
#[test]
fn a_falling_paratrooper_is_marked_again_at_the_head_of_its_cell() {
    use crate::map::entities::EntityCategory;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::sim::movement::parachute_descent::begin_parachute_descent;
    use crate::sim::occupancy::CellObjectMember::Entity;

    let mut sim = crate::sim::world::Simulation::new();
    sim.interner = crate::sim::intern::test_interner();
    sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(
        25,
        25,
        (0..25)
            .flat_map(|y| (0..25).map(move |x| test_flat_cell(x, y)))
            .collect(),
    ));
    for id in [1, 2] {
        let mut infantry = GameEntity::test_default(id, "E1", "Americans", 10, 10);
        infantry.category = EntityCategory::Infantry;
        infantry.lifecycle.in_limbo = false;
        infantry.lifecycle.cell_marked = false;
        infantry.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
        infantry.position.exact_z_leptons = Some(0);
        sim.substrate.entities.insert(infantry);
        assert!(sim.foot_mark_put(id, None, None));
    }
    assert!(begin_parachute_descent(
        &mut sim.substrate.entities,
        1,
        1000
    ));
    let next = |sim: &crate::sim::world::Simulation, id| {
        sim.next_cell_object(crate::sim::occupancy::CellObjectMember::Entity(id))
    };
    assert_eq!(next(&sim, 2), Some(Entity(1)));

    assert!(!sim.advance_fall(1, -3, None, None));
    assert_eq!(next(&sim, 1), Some(Entity(2)));
    let paratrooper = sim.substrate.entities.get(1).unwrap();
    assert!(paratrooper.lifecycle.cell_marked);
    assert!(paratrooper.is_falling_down());
}
