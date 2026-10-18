use super::*;
use crate::rules::jumpjet_params::JumpjetParams;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::LocomotorState;
use crate::util::fixed_math::SimFixed;

/// An unmarked, grounded object out of Limbo at cell (4, 4).
fn actor(id: u64, category: EntityCategory, kind: LocomotorKind) -> GameEntity {
    let mut entity = GameEntity::test_default(id, "ACTOR", "Americans", 4, 4);
    entity.set_category_for_test(category);
    entity.lifecycle.set_in_limbo_for_test(false);
    entity.lifecycle.set_cell_marked(false);
    entity.set_foot_occupation_enabled(true);
    entity.locomotor = Some(LocomotorState::for_test_kind(kind));
    entity.position.set_exact_z_leptons_for_test(Some(0));
    entity
}

fn fixture(category: EntityCategory, kind: LocomotorKind) -> Simulation {
    let mut sim = Simulation::new();
    sim.substrate.entities.insert(actor(1, category, kind));
    sim.interner = crate::sim::intern::test_interner();
    sim
}

fn tethered_infantry() -> Simulation {
    use crate::sim::radio::{self, RadioMessage, RadioPayload, RadioResponse};
    let mut sim = fixture(EntityCategory::Infantry, LocomotorKind::Walk);
    let mut building = GameEntity::test_default_of_category(
        2,
        "GAPILE",
        "Americans",
        4,
        4,
        EntityCategory::Structure,
    );
    building.set_type_ref_for_test(sim.intern("GAPILE"));
    building.lifecycle.set_in_limbo_for_test(false);
    building.lifecycle.set_cell_marked(true);
    sim.substrate.entities.insert(building);
    sim.substrate.occupancy.add(
        4,
        4,
        2,
        MovementLayer::Ground,
        None,
        CellListInsertion::from_category(EntityCategory::Structure),
    );
    sim.substrate.raw_cell_occupation.mark_ground(4, 4, 0x80);
    for message in [RadioMessage::Hello, RadioMessage::Tether] {
        assert_eq!(
            radio::transmit(
                &mut sim,
                1,
                2,
                message,
                RadioPayload::default(),
                None,
                crate::sim::world::FrameEffects::default()
            ),
            RadioResponse::Roger
        );
    }
    radio::take_transmit_log();
    radio::clear_test_trace();
    sim
}

/// P2 original Foot Mark0/1→Techno6F4A91→Object5F5374: the sender's +74
/// write precedes radio13, and the Pickup/PlaceDown list write follows it.
/// The stock dirty GAPILE Mark2 refuses without another presence transaction.
/// Executed witness: human-infantry-output-downstream-research,
/// mark-radio.native.json SHA256270a1321e3a5509df0eb58500883737f69af2172726da216f7ca107962391127.
#[test]
fn a_tethered_foot_marks_before_radio13_and_changes_its_list_after_the_receiver() {
    use crate::sim::radio::{self, RadioTestEvent};
    let mut sim = tethered_infantry();
    let rng_before = sim.rng_state();

    assert!(sim.foot_mark_put(1, None, None, crate::sim::world::FrameEffects::default()));
    assert_eq!(
        radio::take_test_trace(),
        vec![RadioTestEvent::ObjectMarkRefresh {
            receiver_sid: 2,
            sender_sid: Some(1),
            sender_cell_marked: Some(true),
            sender_cell_listed: Some(false),
            accepted: false,
        }]
    );
    assert!(sim.substrate.occupancy.contains_entity(4, 4, 1));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(4, 4), 0x80);

    assert!(sim.foot_mark_remove(1, None, None, crate::sim::world::FrameEffects::default()));
    assert_eq!(
        radio::take_test_trace(),
        vec![RadioTestEvent::ObjectMarkRefresh {
            receiver_sid: 2,
            sender_sid: Some(1),
            sender_cell_marked: Some(false),
            sender_cell_listed: Some(true),
            accepted: false,
        }]
    );
    assert!(!sim.substrate.occupancy.contains_entity(4, 4, 1));
    assert!(sim.substrate.occupancy.contains_entity(4, 4, 2));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(4, 4), 0x80);
    let log = radio::take_transmit_log();
    assert_eq!(log.len(), 2);
    assert!(log.iter().all(|event| {
        (event.sender_sid, event.msg, event.target_sid, event.reply) == (1, 13, 2, Some(1))
    }));
    for (id, contact) in [(1, 2), (2, 1)] {
        let entity = sim.substrate.entities.get(id).unwrap();
        assert_eq!(entity.radio_contacts.slot(0), Some(contact));
        assert_eq!(entity.tether.partner(), Some(contact));
    }
    assert_eq!(sim.rng_state(), rng_before);
}

/// Foot4D3789 returns1 for Mark2 before the Object/Techno gate. In particular,
/// it does not recursively send13 or use Mark(PUT) on an unmarked/limbo Foot.
#[test]
fn foot_mark_refresh_is_immediate_and_does_not_reenter_techno_mark() {
    use crate::sim::radio::{self, RadioMessage, RadioPayload, RadioResponse};
    for (limbo, marked) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut sim = tethered_infantry();
        let actor = sim.substrate.entities.get_mut(1).unwrap();
        actor.lifecycle.set_in_limbo_for_test(limbo);
        actor.lifecycle.set_cell_marked(marked);
        let before = serde_json::to_value(sim.substrate.entities.get(1).unwrap()).unwrap();
        let rng_before = sim.rng_state();
        assert!(sim.mark_entity_refresh(1));
        assert_eq!(
            radio::transmit(
                &mut sim,
                2,
                1,
                RadioMessage::AnimStop,
                RadioPayload::default(),
                None,
                crate::sim::world::FrameEffects::default(),
            ),
            RadioResponse::Roger
        );
        assert_eq!(
            serde_json::to_value(sim.substrate.entities.get(1).unwrap()).unwrap(),
            before
        );
        assert!(!sim.substrate.occupancy.contains_entity(4, 4, 1));
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(4, 4), 0x80);
        assert_eq!(radio::take_transmit_log().len(), 1);
        assert_eq!(sim.rng_state(), rng_before);
    }
}

/// Techno6F4A7D's rejected Object Mark does not transmit; Radio65ACB0 reads
/// only slot0, so a tethered Foot does not fall through a hole to slot1.
#[test]
fn a_refused_mark_and_a_null_first_contact_do_not_emit_radio13() {
    use crate::sim::radio;
    let mut sim = tethered_infantry();
    assert!(!sim.foot_mark_remove(1, None, None, crate::sim::world::FrameEffects::default()));
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .lifecycle
        .set_in_limbo_for_test(true);
    assert!(!sim.foot_mark_put(1, None, None, crate::sim::world::FrameEffects::default()));
    assert!(radio::take_transmit_log().is_empty());

    let actor = sim.substrate.entities.get_mut(1).unwrap();
    actor.lifecycle.set_in_limbo_for_test(false);
    actor.radio_contacts.set_capacity(2);
    assert_eq!(actor.radio_contacts.remove(2), Some(0));
    actor.radio_contacts.set_slot(1, 2);
    assert!(sim.foot_mark_put(1, None, None, crate::sim::world::FrameEffects::default()));
    assert!(radio::take_transmit_log().is_empty());
    assert!(radio::take_test_trace().is_empty());
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .radio_contacts
            .slot(1),
        Some(2)
    );
}

/// A Jumpjet cruising at its `JumpjetHeight=`: `0x0054B8D0` answers Top
/// while it is marked, Ground once +0x74 is clear.
fn cruising_jumpjet() -> Simulation {
    let mut sim = fixture(EntityCategory::Unit, LocomotorKind::Jumpjet);
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    let locomotor = entity.locomotor.as_mut().unwrap();
    locomotor
        .jumpjet_runtime_mut()
        .expect("jumpjet runtime")
        .link(&JumpjetParams {
            turn_rate: 4,
            speed: SimFixed::from_num(14),
            climb: 5.0,
            crash: 5.0,
            height: 500,
            accel: 2.0,
            wobbles: 0.15,
            deviation: 40,
            no_wobbles: false,
        });
    entity.position.set_exact_z_leptons_for_test(Some(500));
    sim
}

/// Add/RemoveContent return for Infantry (`0x0047E9EA` / `0x0047EAFE`)
/// before the Foot enable and the object's vt+0xF0/+0xF4: Unit `0x007441B0`
/// writes 0x20, Aircraft's ObjectClass `0x005F60A0` writes 0x40.
#[test]
fn mark_writes_the_unit_and_aircraft_raw_bits_and_never_an_infantrymans() {
    for (category, kind, expected_bits) in [
        (EntityCategory::Unit, LocomotorKind::Drive, 0x20),
        (EntityCategory::Aircraft, LocomotorKind::Fly, 0x40),
        (EntityCategory::Infantry, LocomotorKind::Walk, 0x00),
    ] {
        let mut sim = fixture(category, kind);
        sim.foot_mark_put(1, None, None, crate::sim::world::FrameEffects::default());
        assert!(sim.substrate.occupancy.contains_entity(4, 4, 1));
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(4, 4),
            expected_bits,
            "{category:?}"
        );
        sim.foot_mark_put(1, None, None, crate::sim::world::FrameEffects::default());
        assert_eq!(
            sim.substrate
                .occupancy
                .count_on_layer(4, 4, MovementLayer::Ground),
            1
        );
        sim.foot_mark_remove(1, None, None, crate::sim::world::FrameEffects::default());
        assert!(!sim.substrate.occupancy.contains_entity(4, 4, 1));
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(4, 4), 0);
        assert!(
            !sim.substrate
                .entities
                .get(1)
                .unwrap()
                .lifecycle
                .cell_marked()
        );
    }
}

/// Mark toggles +0x74 for an airborne Fly object, but `0x004CFCF0` answers
/// Top above height 0, so neither Pick_Up nor Place_Down runs
/// (`0x004D37A9..0x004D37AC`).
#[test]
fn an_airborne_fly_object_is_marked_without_joining_its_cell() {
    let mut sim = fixture(EntityCategory::Aircraft, LocomotorKind::Fly);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .position
        .set_exact_z_leptons_for_test(Some(600));
    assert!(sim.foot_mark_put(1, None, None, crate::sim::world::FrameEffects::default()));
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .lifecycle
            .cell_marked()
    );
    assert!(!sim.substrate.occupancy.contains_entity(4, 4, 1));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(4, 4), 0);
    assert!(sim.foot_mark_remove(1, None, None, crate::sim::world::FrameEffects::default()));
    assert!(
        !sim.substrate
            .entities
            .get(1)
            .unwrap()
            .lifecycle
            .cell_marked()
    );
}

/// Mark(UP) clears +0x74 (`0x005F5913`) before the layer query, which a
/// cruising Jumpjet then answers as Ground: its Pick_Up runs Unit REMOVE
/// (`0x00744210`), which picks the plane by height alone, the deck at this
/// height over level-0 ground. Mark(DOWN) sets +0x74 first, so the same
/// Jumpjet answers Top and joins nothing.
#[test]
fn a_cruising_jumpjet_clears_the_deck_bit_on_remove_and_joins_nothing_on_put() {
    let mut sim = cruising_jumpjet();
    // Level-0 ground under the owner for the receiver's height test.
    sim.path_grid = Some(std::sync::Arc::new(crate::sim::pathfinding::PathGrid::new(
        8, 8,
    )));
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .lifecycle
        .set_cell_marked(true);
    sim.substrate.raw_cell_occupation.mark_ground(4, 4, 0x20);
    sim.substrate.raw_cell_occupation.mark_deck(4, 4, 0x20);
    assert!(sim.foot_mark_remove(1, None, None, crate::sim::world::FrameEffects::default()));
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(4, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(4, 4), 0x20);

    assert!(sim.foot_mark_put(1, None, None, crate::sim::world::FrameEffects::default()));
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .lifecycle
            .cell_marked()
    );
    assert!(!sim.substrate.occupancy.contains_entity(4, 4, 1));
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(4, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(4, 4), 0x20);
}

/// `ObjectClass::Mark` refuses a Limbo object (`0x005F5854..0x005F585C`).
#[test]
fn a_limbo_object_is_refused() {
    let mut sim = fixture(EntityCategory::Unit, LocomotorKind::Drive);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .lifecycle
        .set_in_limbo_for_test(true);
    assert!(!sim.foot_mark_put(1, None, None, crate::sim::world::FrameEffects::default()));
    assert!(
        !sim.substrate
            .entities
            .get(1)
            .unwrap()
            .lifecycle
            .cell_marked()
    );
    assert!(!sim.substrate.occupancy.contains_entity(4, 4, 1));
}

/// Place_Down skips a Location outside the allocated cells
/// (`0x00568471..0x0056848E`); Mark still records +0x74.
#[test]
fn a_location_outside_the_map_is_marked_without_a_cell() {
    let mut sim = fixture(EntityCategory::Unit, LocomotorKind::Drive);
    let cells = (0..4)
        .flat_map(|y| {
            (0..4).map(move |x| crate::sim::world::common_raw_test_terrain_cell(x, y, 0, false))
        })
        .collect();
    sim.resolved_terrain =
        Some(crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(4, 4, cells));
    assert!(sim.foot_mark_put(1, None, None, crate::sim::world::FrameEffects::default()));
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .lifecycle
            .cell_marked()
    );
    assert!(!sim.substrate.occupancy.contains_entity(4, 4, 1));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(4, 4), 0);
}

#[test]
fn put_receiver_observes_mark_and_link_before_raw_and_can_change_live_enable() {
    let mut sim = fixture(EntityCategory::Unit, LocomotorKind::Drive);
    let mut called = false;
    sim.foot_mark_put_observed(
        1,
        None,
        None,
        &mut |sim, id| {
            called = true;
            assert!(
                sim.substrate
                    .entities
                    .get(id)
                    .unwrap()
                    .lifecycle
                    .cell_marked()
            );
            assert!(sim.substrate.occupancy.contains_entity(4, 4, id));
            assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(4, 4), 0);
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .set_foot_occupation_enabled(false);
        },
        crate::sim::world::FrameEffects::default(),
    );
    assert!(called);
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(4, 4), 0);
    assert!(sim.substrate.occupancy.contains_entity(4, 4, 1));
    sim.foot_mark_remove(1, None, None, crate::sim::world::FrameEffects::default());
    assert!(!sim.substrate.occupancy.contains_entity(4, 4, 1));
}

/// A landed Jumpjet answers Ground (`0x0054B8D0`), so its Mark links it into
/// its cell's list like any ground object. A list walk (Object `+0x30`, as
/// `next_cell_object` reads it) passes through it to the objects behind it.
#[test]
fn a_list_walk_passes_through_a_listed_landed_jumpjet() {
    use crate::sim::occupancy::CellObjectMember;
    let mut sim = fixture(EntityCategory::Unit, LocomotorKind::Drive);
    for (id, kind) in [(2, LocomotorKind::Jumpjet), (3, LocomotorKind::Drive)] {
        sim.substrate
            .entities
            .insert(actor(id, EntityCategory::Unit, kind));
    }
    for id in 1..=3 {
        assert!(sim.foot_mark_put(id, None, None, crate::sim::world::FrameEffects::default()));
    }
    assert!(sim.substrate.occupancy.contains_entity(4, 4, 2));
    let next = |sim: &Simulation, id| sim.next_cell_object(CellObjectMember::Entity(id));
    assert_eq!(next(&sim, 3), Some(CellObjectMember::Entity(2)));
    assert_eq!(next(&sim, 2), Some(CellObjectMember::Entity(1)));
}

/// Unit Mark writes a landed Jumpjet's 0x20 through `0x007441B0` as it lists
/// it.
#[test]
fn a_listed_landed_jumpjet_units_mark_writes_its_vehicle_bit() {
    let mut sim = fixture(EntityCategory::Unit, LocomotorKind::Jumpjet);
    assert!(sim.foot_mark_put(1, None, None, crate::sim::world::FrameEffects::default()));
    assert!(sim.substrate.occupancy.contains_entity(4, 4, 1));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(4, 4), 0x20);
}
