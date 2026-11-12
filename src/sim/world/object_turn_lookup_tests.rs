//! What an object's turn looks up in the entity store (VERA-only costs; the
//! turn's results are covered by the tests of each step).

use super::*;
use crate::sim::arena_fixture::{clear_ground_ini, rival_arena};

/// A tank and a power plant of one house on the flat arena.
fn scene() -> (Simulation, RuleSet, u64, u64) {
    let rules = RuleSet::from_ini(&clear_ground_ini(
        "[VehicleTypes]\n0=MTNK\n[BuildingTypes]\n0=GAPOWR\n\
         [MTNK]\nStrength=300\nSpeed=6\n[GAPOWR]\nStrength=750\n",
    ))
    .unwrap();
    let mut sim = rival_arena(&rules, 0x0B1);
    let tank = sim
        .spawn_object("MTNK", "Americans", 10, 10, 0, &rules)
        .expect("tank");
    let plant = sim
        .spawn_object("GAPOWR", "Americans", 16, 16, 0, &rules)
        .expect("power plant");
    sim.resolve_type_handles(&rules);
    (sim, rules, tank, plant)
}

/// The store's lookups by id during `turn`, and those its id index answered.
fn lookups_during(sim: &mut Simulation, turn: impl FnOnce(&mut Simulation)) -> (u64, u64) {
    let before = sim.substrate.entities.lookups();
    turn(sim);
    let after = sim.substrate.entities.lookups();
    (after.0 - before.0, after.1 - before.1)
}

/// A turn looks its own object up by id through the store's focused slot:
/// the id index answers next to none of an idle tank's lookups of itself.
#[test]
fn a_turn_finds_its_own_object_without_the_id_index() {
    let (mut sim, rules, tank, _) = scene();
    let (lookups, hashed) = lookups_during(&mut sim, |sim| {
        sim.advance_live_object_turn(tank, &rules, techno_ai::ObjectAiCtx::default())
            .unwrap();
    });
    assert!(
        hashed <= 8,
        "the id index answered {hashed} of the turn's {lookups} lookups"
    );
}

/// A tank's turn writes none of the state only a Building holds, so its
/// record keeps no building part, art or damage fires, before a save and
/// after a load. The power plant stores its part and its art, which load
/// back as saved.
#[test]
fn a_tanks_turn_and_a_load_store_no_building_state() {
    use crate::util::save_codec;
    let (mut sim, rules, tank, plant) = scene();
    for id in [tank, plant] {
        sim.advance_live_object_turn(id, &rules, techno_ai::ObjectAiCtx::default())
            .unwrap();
    }
    let reloaded = |id| {
        let entity = sim.substrate.entities.get(id).unwrap();
        let bytes = save_codec::serialize(entity).unwrap();
        let loaded: crate::sim::game_entity::GameEntity = save_codec::deserialize(&bytes).unwrap();
        (entity, loaded)
    };
    let (tank, loaded_tank) = reloaded(tank);
    for tank in [tank, &loaded_tank] {
        assert!(!tank.building.is_stored());
        assert!(!tank.building_art.is_stored());
        assert!(!tank.damage_fires.is_stored());
    }
    let (plant, loaded_plant) = reloaded(plant);
    assert!(plant.building.is_stored());
    assert_eq!(*loaded_plant.building, *plant.building);
    assert!(plant.building_art.is_stored());
    assert_eq!(*loaded_plant.building_art, *plant.building_art);
}

/// The record a turn walks stays small: state most objects lack sits
/// behind a pointer, and state a turn does not read each step is the seldom
/// part. A new field that pushes either past its ceiling belongs behind a
/// pointer or in the other part.
#[cfg(all(
    target_pointer_width = "64",
    not(any(feature = "ares", feature = "phobos", feature = "debug-tools"))
))]
#[test]
fn the_object_record_stays_under_its_ceiling() {
    use crate::sim::game_entity::{GameEntity, Seldom};
    let (main, seldom) = (
        std::mem::size_of::<GameEntity>(),
        std::mem::size_of::<Seldom>(),
    );
    assert!(
        main <= 1208 && seldom <= 816,
        "GameEntity is {main} bytes and Seldom {seldom}"
    );
}

/// The sound owner's view of a turn: what was delivered, and whose looping
/// sound was updated.
#[derive(Default)]
struct Heard {
    delivered: std::cell::Cell<usize>,
    updated: std::cell::RefCell<Vec<u64>>,
}

impl crate::sim::world::SoundBoundary for Heard {
    fn consume(&self, _: &Simulation, _: &RuleSet, events: &[crate::sim::world::SimSoundEvent]) {
        self.delivered.set(events.len());
    }

    fn delivered(&self) -> usize {
        self.delivered.get()
    }

    fn update_looping_sound(
        &self,
        _: &Simulation,
        owner: u64,
        _: crate::sim::anim_class::AnimWorldCoord,
    ) {
        self.updated.borrow_mut().push(owner);
    }
}

/// After its own AI a building's turn takes none of the Foot and class tails
/// but what reaches it: it still updates its looping sound and commits, which
/// delivers the smudge grid's pending dirt, and it looks itself up for
/// little else.
#[test]
fn a_buildings_turn_keeps_its_deliveries_and_takes_no_foot_tail() {
    // The same scene twice, with smudge dirt nothing has delivered yet: one
    // runs the building's AI alone, the other its whole turn.
    let prepared = || {
        let (mut sim, rules, _, plant) = scene();
        let mut smudges = crate::sim::smudge_grid::SmudgeGrid::new(32, 32);
        smudges.test_force_set(3, 4, Default::default());
        sim.smudge_grid = Some(smudges);
        sim.dirty_cells.clear_tactical_for_test();
        (sim, rules, plant)
    };
    let pending = |sim: &mut Simulation| sim.smudge_grid.as_mut().unwrap().drain_dirty();

    let (mut ai_only, rules, plant) = prepared();
    let heard = Heard::default();
    let ctx = techno_ai::ObjectAiCtx {
        effects: crate::sim::world::FrameEffects::for_sound(&heard),
        overlay_registry: None,
    };
    let (ai_lookups, _) = lookups_during(&mut ai_only, |sim| {
        sim.object_ai_visit_one_with_effects(plant, &rules, ctx);
    });
    assert!(heard.updated.borrow().is_empty());
    assert_eq!(pending(&mut ai_only), [(3, 4)]);

    let (mut sim, rules, plant) = prepared();
    let heard = Heard::default();
    let ctx = techno_ai::ObjectAiCtx {
        effects: crate::sim::world::FrameEffects::for_sound(&heard),
        overlay_registry: None,
    };
    let (turn_lookups, _) = lookups_during(&mut sim, |sim| {
        sim.advance_live_object_turn(plant, &rules, ctx).unwrap();
    });
    assert_eq!(*heard.updated.borrow(), [plant]);
    assert_eq!(heard.delivered.get(), sim.sound_events.len());
    assert!(pending(&mut sim).is_empty());
    assert!(sim.dirty_cells.tactical_for_test().contains(&(3, 4)));
    let tail = turn_lookups - ai_lookups;
    assert!(
        tail <= 12,
        "the building looked itself up {tail} times after its AI"
    );
}

/// A tank and an infantryman that hold no target take their fire visits
/// without a receiver transaction. Each visit still delivers the smudge
/// grid's pending dirt, as the commit of an empty transaction did, and the
/// infantryman's clears his firing latch.
#[test]
fn a_fire_visit_without_a_target_keeps_its_deliveries() {
    use crate::sim::combat::world_fire::FireVisit;
    let rules = RuleSet::from_ini(&clear_ground_ini(
        "[VehicleTypes]\n0=MTNK\n[InfantryTypes]\n0=E1\n\
         [MTNK]\nStrength=300\nSpeed=6\n[E1]\nStrength=125\nSpeed=4\n",
    ))
    .unwrap();
    let mut sim = rival_arena(&rules, 0x0B2);
    let tank = sim
        .spawn_object("MTNK", "Americans", 10, 10, 0, &rules)
        .expect("tank");
    let man = sim
        .spawn_object("E1", "Americans", 14, 14, 0, &rules)
        .expect("infantryman");
    sim.resolve_type_handles(&rules);
    sim.substrate
        .entities
        .get_mut(man)
        .unwrap()
        .mission_leaf
        .set_foot_firing_sequence(1);
    for (visit, cell) in [
        (FireVisit::UnitTarget(tank), (3, 4)),
        (FireVisit::InfantryTarget(man), (5, 6)),
    ] {
        let mut smudges = crate::sim::smudge_grid::SmudgeGrid::new(32, 32);
        smudges.test_force_set(cell.0, cell.1, Default::default());
        sim.smudge_grid = Some(smudges);
        sim.dirty_cells.clear_tactical_for_test();
        let receipt = sim.commit_fire_visit(
            visit,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert!(!receipt.bridge_state_changed && !receipt.structure_destroyed);
        assert!(sim.smudge_grid.as_mut().unwrap().drain_dirty().is_empty());
        assert!(sim.dirty_cells.tactical_for_test().contains(&cell));
    }
    let man = sim.substrate.entities.get(man).unwrap();
    assert_eq!(man.mission_leaf.foot_firing_sequence_latch(), 0);
}

/// Idle turns hand their object out for writing only when a timer comes
/// due (a mission handler re-arming its clock): each hand-out copies an
/// object a presentation frame still shares, so a step that writes nothing
/// must not take one.
#[test]
fn an_idle_turn_hands_its_object_out_only_on_a_due_timer() {
    let (mut sim, rules, tank, plant) = scene();
    let mut turn = |sim: &mut Simulation, id| {
        let before = sim.substrate.entities.hand_outs();
        sim.advance_live_object_turn(id, &rules, techno_ai::ObjectAiCtx::default())
            .unwrap();
        sim.substrate.entities.hand_outs() - before
    };
    let mut frame = |sim: &mut Simulation| {
        let next = sim.session.binary_frame() + 1;
        sim.session.set_binary_frame_for_test(next);
    };
    // Let construction, first missions and timers settle.
    for _ in 0..40 {
        turn(&mut sim, tank);
        turn(&mut sim, plant);
        frame(&mut sim);
    }
    let mut handed_out = [0u64; 2];
    let mut writing_turns = [0u32; 2];
    for _ in 0..60 {
        for (slot, id) in [tank, plant].into_iter().enumerate() {
            let count = turn(&mut sim, id);
            handed_out[slot] += count;
            writing_turns[slot] += u32::from(count != 0);
        }
        frame(&mut sim);
    }
    assert!(
        writing_turns[0] <= 6 && writing_turns[1] <= 6,
        "turns of 60 that handed the object out (tank, plant): {writing_turns:?}, \
         hand-outs {handed_out:?}"
    );
}
