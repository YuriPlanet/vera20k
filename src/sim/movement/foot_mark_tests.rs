use super::*;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::LocomotorState;

fn fixture(category: EntityCategory, kind: LocomotorKind) -> Simulation {
    let mut sim = Simulation::new();
    let mut entity = GameEntity::test_default(1, "ACTOR", "Americans", 4, 4);
    entity.category = category;
    entity.lifecycle.in_limbo = false;
    entity.lifecycle.cell_marked = false;
    entity.foot_occupation_enabled = true;
    entity.locomotor = Some(LocomotorState::for_test_kind(kind));
    sim.substrate.entities.insert(entity);
    sim.interner = crate::sim::intern::test_interner();
    sim
}

#[test]
fn concrete_foot_category_selects_raw_receiver_independently_of_locomotor() {
    for (category, kind, expected_mask) in [
        (EntityCategory::Unit, LocomotorKind::Walk, 0x20),
        (EntityCategory::Infantry, LocomotorKind::Drive, 0x01),
    ] {
        let mut sim = fixture(category, kind);
        sim.foot_mark_put(1, None, None, None);
        assert!(sim.substrate.occupancy.contains_entity(4, 4, 1));
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(4, 4),
            expected_mask
        );
        let entered = sim.substrate.entities.get(1).unwrap().occupancy_enter_order;
        sim.foot_mark_put(1, None, None, None);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().occupancy_enter_order,
            entered
        );
        sim.foot_mark_remove(1, None, None, None);
        assert!(!sim.substrate.occupancy.contains_entity(4, 4, 1));
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(4, 4), 0);
        assert!(!sim.substrate.entities.get(1).unwrap().lifecycle.cell_marked);
    }
}

#[test]
fn put_receiver_observes_mark_and_link_before_raw_and_can_change_live_enable() {
    let mut sim = fixture(EntityCategory::Unit, LocomotorKind::Drive);
    let mut called = false;
    sim.foot_mark_put_observed(1, None, None, None, &mut |sim, id| {
        called = true;
        assert!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .lifecycle
                .cell_marked
        );
        assert!(sim.substrate.occupancy.contains_entity(4, 4, id));
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(4, 4), 0);
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .foot_occupation_enabled = false;
    });
    assert!(called);
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(4, 4), 0);
    assert!(sim.substrate.occupancy.contains_entity(4, 4, 1));
    sim.foot_mark_remove(1, None, None, None);
    assert!(!sim.substrate.occupancy.contains_entity(4, 4, 1));
}
