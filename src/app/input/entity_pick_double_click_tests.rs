//! VERA right-click orders: double-clicking an object selects every
//! on-screen object of its type that the local player controls. The T key
//! keeps the map-wide fallback; the double-click never widens past the screen.

use super::*;
use crate::sim::combat::Health;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::world::Simulation;

struct World {
    sim: Simulation,
    /// The ids the screen shows, in encounter order.
    screen_order: Vec<u64>,
    /// The ids the drawn-object list holds, in encounter order.
    click_order: Vec<u64>,
}

/// A human `Russians` house and a computer `YuriCountry` house, with local
/// E1s on cells (10,10) and (12,10), a local E1 far off screen, a local HTNK
/// and an enemy E1. Object 3 is absent from `screen_order` only.
fn world() -> World {
    let mut sim = Simulation::new();
    let local = sim.intern("Russians");
    let enemy = sim.intern("YuriCountry");
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(local));
    sim.session_mut_for_test()
        .set_game_mode(ScenarioGameMode::from_legacy_nonzero(false));
    sim.houses_mut_for_test()
        .register(HouseState::new(local, 0, None, true, 0, 10));
    sim.houses_mut_for_test()
        .register(HouseState::new(enemy, 1, None, false, 0, 10));
    spawn(&mut sim, 1, 10, 10, local, "E1");
    spawn(&mut sim, 2, 12, 10, local, "E1");
    spawn(&mut sim, 3, 30, 30, local, "E1");
    spawn(&mut sim, 4, 14, 10, local, "HTNK");
    spawn(&mut sim, 5, 16, 10, enemy, "E1");
    World {
        sim,
        screen_order: vec![1, 2, 4, 5],
        click_order: vec![1, 2, 3, 4, 5],
    }
}

fn spawn(sim: &mut Simulation, sid: u64, rx: u16, ry: u16, owner: InternedId, type_id: &str) {
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        sid,
        rx,
        ry,
        0,
        0,
        owner,
        Health::new(100),
        sim.intern(type_id),
        EntityCategory::Unit,
        0,
        5,
        false,
    );
    entity.lifecycle.set_in_limbo_for_test(false);
    sim.entities_mut_for_test().insert(entity);
}

/// The production double-click path at the clicked object's own drawn point.
fn double_click(world: &World, clicked: u64, screen_order: &[u64]) -> Option<SelectionMutation> {
    let (x, y) = crate::render::locomotor_visual::screen_position(
        world.sim.entities().get(clicked).unwrap(),
    );
    compute_double_click_type_select_with_playfield(
        &world.sim,
        &world.click_order,
        screen_order,
        None,
        Some("Russians"),
        x,
        y,
        None,
        Some(world.sim.interner()),
        false,
        world.sim.resolved_terrain(),
    )
}

#[test]
fn double_click_selects_only_the_screens_local_members_of_the_clicked_type() {
    let world = world();
    let mutation = double_click(&world, 1, &world.screen_order).expect("the clicked E1 is local");
    assert!(mutation.clear, "the shortcut replaces the selection");
    // 2 is the other on-screen local E1; 4 is another type, 5 is the enemy's
    // E1 and 3 is the same type off screen.
    assert_eq!(mutation.select, vec![1, 2]);
    assert!(mutation.deselect.is_empty());
}

#[test]
fn double_click_has_no_map_wide_fallback_when_the_screen_shows_one_member() {
    let world = world();
    // The map still holds local object 3 of the same type, but the screen
    // pass sees only object 1: the T tap would widen to the map here, the
    // double-click must not.
    let mutation = double_click(&world, 1, &[1]).expect("the clicked E1 is local");
    assert_eq!(mutation.select, vec![1]);
}

#[test]
fn double_click_on_an_enemy_or_empty_ground_keeps_the_plain_click() {
    let world = world();
    assert_eq!(
        double_click(&world, 5, &world.screen_order),
        None,
        "an enemy object is not a local type-select seed"
    );
    let mutation = compute_double_click_type_select_with_playfield(
        &world.sim,
        &world.click_order,
        &world.screen_order,
        None,
        Some("Russians"),
        10_000.0,
        10_000.0,
        None,
        Some(world.sim.interner()),
        false,
        world.sim.resolved_terrain(),
    );
    assert_eq!(mutation, None, "ground keeps the ordinary click");
}
