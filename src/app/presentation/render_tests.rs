use super::HoverTargetKind;
use crate::app::input::dispatch::CLICK_SELECT_RADIUS;
use crate::app::input::entity_pick::{
    compute_box_selection_snapshot, compute_click_selection_snapshot, hover_target_at_point,
};
use crate::map::entities::EntityCategory;
use crate::sim::combat::Health;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::test_intern;
use crate::sim::world::Simulation;

/// Spawn a mobile unit **at a cell**. Where it lands on screen is derived, so
/// tests take their click and box coordinates from [`screen_of`] rather than
/// inventing pixel values that no longer have anywhere to be stored.
fn spawn_mobile(store: &mut EntityStore, sid: u64, rx: u16, ry: u16, owner: &str, selected: bool) {
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        sid,
        rx,
        ry,
        0,
        0,
        test_intern(owner),
        Health::new(100),
        test_intern("E1"),
        EntityCategory::Unit,
        0,
        5,
        false,
    );
    entity.selection.set_selected_for_test(selected);
    // Revealed onto the map — the picker refuses limbo objects.
    entity.lifecycle.set_in_limbo_for_test(false);
    store.insert(entity);
}

/// Where an entity in `store` is drawn — the same answer the picking code gets.
fn screen_of(store: &EntityStore, sid: u64) -> (f32, f32) {
    crate::render::locomotor_visual::screen_position(store.get(sid).expect("entity"))
}

fn encounter_order(store: &EntityStore) -> Vec<u64> {
    store.values().map(|entity| entity.stable_id()).collect()
}

fn selected_order(store: &EntityStore) -> Vec<u64> {
    store
        .values()
        .filter(|entity| entity.selection.is_selected())
        .map(|entity| entity.stable_id())
        .collect()
}

#[test]
fn test_click_additive_toggles_membership() {
    let mut store = EntityStore::new();
    spawn_mobile(&mut store, 1, 10, 10, "Americans", true);
    spawn_mobile(&mut store, 2, 12, 10, "Americans", false);
    let (first_x, first_y) = screen_of(&store, 1);
    let (second_x, second_y) = screen_of(&store, 2);

    let added = compute_click_selection_snapshot(
        &store,
        &encounter_order(&store),
        &selected_order(&store),
        None,
        None,
        second_x,
        second_y,
        CLICK_SELECT_RADIUS,
        true,
        None,
        None,
        None,
    )
    .expect("snapshot");
    assert_eq!(added.select, vec![2]);

    let removed = compute_click_selection_snapshot(
        &store,
        &encounter_order(&store),
        &selected_order(&store),
        None,
        None,
        first_x,
        first_y,
        CLICK_SELECT_RADIUS,
        true,
        None,
        None,
        None,
    )
    .expect("snapshot");
    assert_eq!(removed.deselect, vec![1]);
}

#[test]
fn test_box_additive_adds_only_and_excludes_structures() {
    let mut store = EntityStore::new();
    spawn_mobile(&mut store, 1, 10, 10, "Americans", true);
    spawn_mobile(&mut store, 2, 12, 10, "Americans", true);
    spawn_mobile(&mut store, 3, 14, 10, "Americans", false);
    let mut building = GameEntity::new_at_frame_zero_for_test(
        4,
        11,
        11,
        0,
        0,
        test_intern("Americans"),
        Health::new(100),
        test_intern("GAPOWR"),
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    building.lifecycle.set_in_limbo_for_test(false);
    store.insert(building);

    // The box covers all four: the units span (0,315)..(120,375) and the
    // building sits at (0,345). The native band callback only ever calls
    // Select — a shift drag over units already in the group keeps them, so the
    // two selected units stay and the third joins.
    let snapshot = compute_box_selection_snapshot(
        &store,
        &encounter_order(&store),
        &encounter_order(&store),
        &selected_order(&store),
        None,
        None,
        -40.0,
        300.0,
        160.0,
        400.0,
        true,
        None,
        None,
        None,
    )
    .expect("snapshot");
    assert_eq!(snapshot.select, vec![3]);
}

fn spawn_structure(
    store: &mut EntityStore,
    sid: u64,
    rx: u16,
    ry: u16,
    type_id: &str,
    owner: &str,
) {
    let mut building = GameEntity::new_at_frame_zero_for_test(
        sid,
        rx,
        ry,
        0,
        0,
        test_intern(owner),
        Health::new(100),
        test_intern(type_id),
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    building.lifecycle.set_in_limbo_for_test(false);
    store.insert(building);
}

/// A box drawn around both units: `hi` is the id expected to survive the filter.
fn box_over_two(
    store: &EntityStore,
    a: u64,
    b: u64,
) -> Option<crate::app::input::entity_pick::SelectionMutation> {
    let (ax, ay) = screen_of(store, a);
    let (bx, by) = screen_of(store, b);
    compute_box_selection_snapshot(
        store,
        &encounter_order(store),
        &encounter_order(store),
        &selected_order(store),
        None,
        None,
        ax.min(bx) - 20.0,
        ay.min(by) - 20.0,
        ax.max(bx) + 20.0,
        ay.max(by) + 20.0,
        false,
        None,
        None,
        None,
    )
}

#[test]
fn test_box_skips_a_miner_docked_on_a_refinery() {
    let mut store = EntityStore::new();
    spawn_mobile(&mut store, 1, 10, 10, "Americans", false);
    spawn_mobile(&mut store, 2, 11, 10, "Americans", false);
    // The miner sits on the refinery's own cell while it unloads. The native
    // gate is the dock flag AND a building in that cell, so both halves hold.
    spawn_structure(&mut store, 3, 11, 10, "GAREFN", "Americans");
    store
        .get_mut(2)
        .expect("miner")
        .tether
        .set_for_test(Some(3));

    let snapshot = box_over_two(&store, 1, 2).expect("snapshot");
    assert_eq!(snapshot.select, vec![1]);
}

#[test]
fn test_box_keeps_a_vehicle_that_drove_clear_of_its_factory() {
    let mut store = EntityStore::new();
    spawn_mobile(&mut store, 1, 10, 10, "Americans", false);
    spawn_mobile(&mut store, 2, 14, 10, "Americans", false);
    // A vehicle leaving a war factory carries the same dock flag, but it is no
    // longer standing on the factory — the cell half of the native gate fails,
    // so it stays band-selectable. Without that half every freshly built
    // vehicle would drop out of a box for the whole exit sequence.
    spawn_structure(&mut store, 3, 11, 11, "GAWEAP", "Americans");
    store
        .get_mut(2)
        .expect("vehicle")
        .tether
        .set_for_test(Some(3));

    let snapshot = box_over_two(&store, 1, 2).expect("snapshot");
    assert_eq!(snapshot.select, vec![1, 2]);
}

#[test]
fn test_box_selection_excludes_selectable_no_types() {
    let mut store = EntityStore::new();
    spawn_mobile(&mut store, 1, 10, 10, "Americans", false);
    let mut plane = GameEntity::new_at_frame_zero_for_test(
        2,
        11,
        10,
        0,
        0,
        test_intern("Americans"),
        Health::new(100),
        test_intern("PDPLANE"),
        EntityCategory::Aircraft,
        0,
        5,
        true,
    );
    plane.lifecycle.set_in_limbo_for_test(false);
    store.insert(plane);

    let ini = crate::rules::ini_parser::IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\n\
         [VehicleTypes]\n\n\
         [AircraftTypes]\n0=PDPLANE\n\n\
         [BuildingTypes]\n\n\
         [E1]\nStrength=125\nArmor=flak\nSpeed=4\n\n\
         [PDPLANE]\nStrength=150\nArmor=light\nSpeed=16\nSelectable=no\n",
    );
    let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).expect("rules parse");
    let interner = crate::sim::intern::test_interner();

    // A box wide enough to cover both sprites — the paradrop plane overhead must
    // not be dragged into the group with the unit under it.
    let (ux, uy) = screen_of(&store, 1);
    let (px, py) = screen_of(&store, 2);
    let snapshot = compute_box_selection_snapshot(
        &store,
        &encounter_order(&store),
        &encounter_order(&store),
        &selected_order(&store),
        None,
        None,
        ux.min(px) - 20.0,
        uy.min(py) - 20.0,
        ux.max(px) + 20.0,
        uy.max(py) + 20.0,
        false,
        Some(&rules),
        None,
        Some(&interner),
    )
    .expect("snapshot");
    assert_eq!(snapshot.select, vec![1]);
}

#[test]
fn item83_band_selection_is_exact_local_even_for_a_discovered_nonlocal_unit() {
    let mut store = EntityStore::new();
    spawn_mobile(&mut store, 1, 10, 10, "Americans", false);
    spawn_mobile(&mut store, 2, 11, 10, "Soviet", false);

    let mut houses = crate::sim::houses::Houses::default();
    for (name, is_human) in [("Americans", true), ("Soviet", false)] {
        let id = test_intern(name);
        houses.register(crate::sim::house_state::HouseState::new(
            id, 0, None, is_human, 0, 10,
        ));
    }

    // A box over both admits only the exact local owner. House human/AI flags
    // are not the gate on this caller-specific path.
    let (mx, my) = screen_of(&store, 1);
    let (ex, ey) = screen_of(&store, 2);
    let interner = crate::sim::intern::test_interner();
    let snapshot = compute_box_selection_snapshot(
        &store,
        &encounter_order(&store),
        &encounter_order(&store),
        &selected_order(&store),
        None,
        Some("Americans"),
        mx.min(ex) - 20.0,
        my.min(ey) - 20.0,
        mx.max(ex) + 20.0,
        my.max(ey) + 20.0,
        false,
        None,
        Some(&houses),
        Some(&interner),
    )
    .expect("snapshot");
    assert_eq!(snapshot.select, vec![1]);
}

#[test]
fn test_hover_target_distinguishes_friendly_and_enemy_categories() {
    let mut sim = Simulation::new();
    let americans_id = sim.intern("Americans");
    let soviet_id = sim.intern("Soviet");
    let gapowr_id = sim.intern("GAPOWR");
    let e1_id = sim.intern("E1");

    // Hover coordinates come from the drawn position, which is the cell
    // centre — the point `screen_to_iso` resolves back to the right cell.
    let mut friendly = GameEntity::new_at_frame_zero_for_test(
        10,
        5,
        5,
        0,
        0,
        americans_id,
        Health::new(100),
        gapowr_id,
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    friendly.lifecycle.set_in_limbo_for_test(false);
    sim.entities_mut_for_test().insert(friendly);
    let (friendly_sx, friendly_sy) =
        crate::render::locomotor_visual::screen_position(sim.entities().get(10).expect("friendly"));

    let mut enemy = GameEntity::new_at_frame_zero_for_test(
        11,
        20,
        5,
        0,
        0,
        soviet_id,
        Health::new(100),
        e1_id,
        EntityCategory::Unit,
        0,
        5,
        false,
    );
    enemy.lifecycle.set_in_limbo_for_test(false);
    sim.entities_mut_for_test().insert(enemy);
    let (enemy_sx, enemy_sy) =
        crate::render::locomotor_visual::screen_position(sim.entities().get(11).expect("enemy"));
    // Provide empty height maps — structure picking now uses foundation-based hit testing.

    // This test classifies the two objects on open terrain. Reveal the actual
    // clicked cells, including the building anchor's projection, before the
    // shared ProcessClickCoords gate. Shroud rejection is tested separately.
    for (x, y) in [(friendly_sx, friendly_sy), (enemy_sx, enemy_sy)] {
        let (rx, ry) = crate::app::match_runtime::sim_tick::world_point_to_cell(x, y, None);
        sim.fog_mut_for_test()
            .mark_visible_for_owner(americans_id, rx, ry);
    }
    let friendly_hover =
        hover_target_at_point(&sim, friendly_sx, friendly_sy, "Americans", false, None)
            .expect("friendly hover");
    assert_eq!(friendly_hover.kind, HoverTargetKind::FriendlyStructure);
    assert_eq!(friendly_hover.stable_id, 10);

    let enemy_hover = hover_target_at_point(&sim, enemy_sx, enemy_sy, "Americans", false, None)
        .expect("enemy hover");
    assert_eq!(enemy_hover.kind, HoverTargetKind::EnemyUnit);
    assert_eq!(enemy_hover.stable_id, 11);
}
