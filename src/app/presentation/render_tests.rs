use super::HoverTargetKind;
use crate::app::input::dispatch::CLICK_SELECT_RADIUS;
use crate::app::input::entity_pick::{
    compute_box_selection_snapshot, compute_click_selection_snapshot, hover_target_at_point,
    pick_enemy_target_stable_id,
};
use crate::app::presentation::sidebar_render::sync_targeting_mode;
use crate::map::entities::EntityCategory;
use crate::map::houses::HouseAllianceMap;
use crate::sim::components::Health;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::test_intern;
use crate::sim::production::ReadyBuildingView;
use crate::sim::vision::FogState;
use crate::sim::world::Simulation;
use std::collections::{BTreeMap, BTreeSet};

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
        Health { current: 100 },
        test_intern("E1"),
        EntityCategory::Unit,
        0,
        5,
        false,
    );
    entity.selected = selected;
    // Revealed onto the map — the picker refuses limbo objects.
    entity.lifecycle.in_limbo = false;
    store.insert(entity);
}

/// Where an entity in `store` is drawn — the same answer the picking code gets.
fn screen_of(store: &EntityStore, sid: u64) -> (f32, f32) {
    crate::render::locomotor_visual::screen_position(store.get(sid).expect("entity"))
}

fn encounter_order(store: &EntityStore) -> Vec<u64> {
    store.values().map(|entity| entity.stable_id).collect()
}

fn selected_order(store: &EntityStore) -> Vec<u64> {
    store
        .values()
        .filter(|entity| entity.selected)
        .map(|entity| entity.stable_id)
        .collect()
}

fn allied_fog_with_visible_cells(
    local_owner: &str,
    allied_owner: &str,
    visible_cells: &[(u16, u16)],
) -> FogState {
    let mut alliances = HouseAllianceMap::default();
    let allied_names = BTreeSet::from([
        local_owner.to_ascii_uppercase(),
        allied_owner.to_ascii_uppercase(),
    ]);
    alliances.insert(local_owner.to_ascii_uppercase(), allied_names.clone());
    alliances.insert(allied_owner.to_ascii_uppercase(), allied_names);

    let mut by_owner = BTreeMap::new();
    let mut visibility = crate::sim::vision::OwnerVisibility::new(64, 64);
    for &(rx, ry) in visible_cells {
        visibility.mark_visible(rx, ry);
    }
    by_owner.insert(crate::sim::intern::test_intern(allied_owner), visibility);

    FogState {
        width: 64,
        height: 64,
        by_owner,
        alliances,
        ..Default::default()
    }
}

#[test]
fn test_click_replace_selects_only_target() {
    let mut store = EntityStore::new();
    // Two cells apart is 67px, comfortably outside CLICK_SELECT_RADIUS, so a
    // click on one cannot also catch the other.
    spawn_mobile(&mut store, 1, 10, 10, "Americans", true);
    spawn_mobile(&mut store, 2, 12, 10, "Americans", false);
    let (cx, cy) = screen_of(&store, 2);

    let empty_heights: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let snapshot = compute_click_selection_snapshot(
        &store,
        &encounter_order(&store),
        &selected_order(&store),
        None,
        None,
        cx,
        cy,
        CLICK_SELECT_RADIUS,
        false,
        None,
        None,
        &empty_heights,
        None,
        None,
    )
    .expect("snapshot");
    assert!(snapshot.clear);
    assert_eq!(snapshot.select, vec![2]);
}

#[test]
fn test_click_additive_toggles_membership() {
    let mut store = EntityStore::new();
    spawn_mobile(&mut store, 1, 10, 10, "Americans", true);
    spawn_mobile(&mut store, 2, 12, 10, "Americans", false);
    let (first_x, first_y) = screen_of(&store, 1);
    let (second_x, second_y) = screen_of(&store, 2);

    let empty_heights: BTreeMap<(u16, u16), u8> = BTreeMap::new();
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
        &empty_heights,
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
        &empty_heights,
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
        Health { current: 100 },
        test_intern("GAPOWR"),
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    building.lifecycle.in_limbo = false;
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

#[test]
fn test_empty_box_leaves_the_selection_alone() {
    let mut store = EntityStore::new();
    spawn_mobile(&mut store, 1, 10, 10, "Americans", true);

    // gamemd clears the selection only when the rectangle caught a drawn
    // object; an empty rectangle queues no selection change at all and the
    // release falls through to the ordinary click action.
    let snapshot = compute_box_selection_snapshot(
        &store,
        &encounter_order(&store),
        &encounter_order(&store),
        &selected_order(&store),
        None,
        None,
        300.0,
        300.0,
        340.0,
        340.0,
        false,
        None,
        None,
        None,
    );
    assert_eq!(snapshot, None);
}

#[test]
fn test_box_over_only_ineligible_objects_still_clears_the_selection() {
    let mut store = EntityStore::new();
    spawn_mobile(&mut store, 1, 10, 10, "Americans", true);
    spawn_mobile(&mut store, 2, 30, 30, "Soviet", false);

    let mut houses: BTreeMap<crate::sim::intern::InternedId, crate::sim::house_state::HouseState> =
        BTreeMap::new();
    for (name, is_human) in [("Americans", true), ("Soviet", false)] {
        let id = test_intern(name);
        houses.insert(
            id,
            crate::sim::house_state::HouseState::new(id, 0, None, is_human, 0, 10),
        );
    }

    // The rectangle holds one AI-owned tank and nothing else. The native
    // "did the box catch anything" test has no owner filter, so the box counts
    // as non-empty, the clear runs, and the per-object filter then admits
    // nobody — the selection ends up empty rather than untouched.
    let (ex, ey) = screen_of(&store, 2);
    let interner = crate::sim::intern::test_interner();
    let snapshot = compute_box_selection_snapshot(
        &store,
        &encounter_order(&store),
        &encounter_order(&store),
        &selected_order(&store),
        None,
        Some("Americans"),
        ex - 20.0,
        ey - 20.0,
        ex + 20.0,
        ey + 20.0,
        false,
        None,
        Some(&houses),
        Some(&interner),
    )
    .expect("snapshot");
    assert!(snapshot.clear && snapshot.select.is_empty());
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
        Health { current: 100 },
        test_intern(type_id),
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    building.lifecycle.in_limbo = false;
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
    store.get_mut(2).expect("miner").dock_entered_with = Some(3);

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
    store.get_mut(2).expect("vehicle").dock_entered_with = Some(3);

    let snapshot = box_over_two(&store, 1, 2).expect("snapshot");
    assert_eq!(snapshot.select, vec![1, 2]);
}

/// The drawn-object list the native emptiness test walks is only partly
/// visibility-filtered: mobile objects register from inside their own draw
/// routine, so the shroud hides them from it, while buildings are appended in
/// bulk from the building array with no visibility test at all.
#[test]
fn test_band_emptiness_ignores_a_shrouded_unit_but_not_a_shrouded_building() {
    use crate::app::input::entity_pick::band_rect_contains_drawn_object;

    let mut alliances = HouseAllianceMap::default();
    alliances.insert(
        "AMERICANS".to_string(),
        BTreeSet::from(["AMERICANS".to_string()]),
    );
    let mut by_owner = BTreeMap::new();
    // The local player has explored nothing.
    by_owner.insert(
        test_intern("Americans"),
        crate::sim::vision::OwnerVisibility::new(64, 64),
    );
    let fog = FogState {
        width: 64,
        height: 64,
        by_owner,
        alliances,
        ..Default::default()
    };

    // One store per case, both populated before the interner snapshot is taken
    // (it is a clone of the shared test interner, so every name has to be in it).
    let mut unit_only = EntityStore::new();
    spawn_mobile(&mut unit_only, 1, 20, 20, "Soviet", false);
    let mut building_only = EntityStore::new();
    spawn_structure(&mut building_only, 2, 20, 20, "GAPOWR", "Soviet");
    let interner = crate::sim::intern::test_interner();

    let (ux, uy) = screen_of(&unit_only, 1);
    let rect = (ux - 20.0, uy - 20.0, ux + 20.0, uy + 20.0);
    assert!(
        !band_rect_contains_drawn_object(
            &unit_only,
            &encounter_order(&unit_only),
            Some(&fog),
            Some("Americans"),
            rect.0,
            rect.1,
            rect.2,
            rect.3,
            Some(&interner),
        ),
        "a shrouded enemy unit is not in the drawn-object list"
    );
    assert!(
        band_rect_contains_drawn_object(
            &building_only,
            &encounter_order(&building_only),
            Some(&fog),
            Some("Americans"),
            rect.0,
            rect.1,
            rect.2,
            rect.3,
            Some(&interner),
        ),
        "a building is registered in bulk, shroud or no shroud"
    );
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
        Health { current: 100 },
        test_intern("PDPLANE"),
        EntityCategory::Aircraft,
        0,
        5,
        true,
    );
    plane.lifecycle.in_limbo = false;
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

    let mut houses: BTreeMap<crate::sim::intern::InternedId, crate::sim::house_state::HouseState> =
        BTreeMap::new();
    for (name, is_human) in [("Americans", true), ("Soviet", false)] {
        let id = test_intern(name);
        houses.insert(
            id,
            crate::sim::house_state::HouseState::new(id, 0, None, is_human, 0, 10),
        );
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
fn test_click_selection_allows_visible_allied_units_for_local_owner() {
    let mut store = EntityStore::new();
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        7,
        11,
        10,
        0,
        0,
        test_intern("British"),
        Health { current: 100 },
        test_intern("E1"),
        EntityCategory::Unit,
        0,
        5,
        false,
    );
    entity.lifecycle.in_limbo = false;
    store.insert(entity);
    let (cx, cy) = screen_of(&store, 7);

    let fog = allied_fog_with_visible_cells("Americans", "British", &[(11, 10)]);

    let empty_heights: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let snapshot = compute_click_selection_snapshot(
        &store,
        &encounter_order(&store),
        &selected_order(&store),
        Some(&fog),
        Some("Americans"),
        cx,
        cy,
        CLICK_SELECT_RADIUS,
        false,
        None,
        None,
        &empty_heights,
        None,
        None,
    )
    .expect("snapshot");

    assert_eq!(snapshot.select, vec![7]);
}

#[test]
fn test_pick_enemy_target_ignores_hidden_entities() {
    let mut sim = Simulation::new();
    let soviet_id = sim.interner.intern("Soviet");
    let e1_id = sim.interner.intern("E1");

    let hidden = GameEntity::new_at_frame_zero_for_test(
        2,
        10,
        10,
        0,
        0,
        soviet_id,
        Health { current: 100 },
        e1_id,
        EntityCategory::Unit,
        0,
        5,
        false,
    );
    sim.entities_mut().insert(hidden);
    let (hx, hy) =
        crate::render::locomotor_visual::screen_position(sim.entities().get(2).expect("hidden"));

    let empty_heights: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let picked_hidden =
        pick_enemy_target_stable_id(&sim, hx, hy, "Americans", false, None, &empty_heights, None);
    assert!(
        picked_hidden.is_none(),
        "Hidden enemy must not be targetable"
    );

    let visible = GameEntity::new_at_frame_zero_for_test(
        3,
        11,
        10,
        0,
        0,
        soviet_id,
        Health { current: 100 },
        e1_id,
        EntityCategory::Unit,
        0,
        5,
        false,
    );
    sim.entities_mut().insert(visible);
    let (vx, vy) =
        crate::render::locomotor_visual::screen_position(sim.entities().get(3).expect("visible"));
    sim.fog
        .mark_visible_for_owner(crate::sim::intern::test_intern("Americans"), 11, 10);

    let picked_visible =
        pick_enemy_target_stable_id(&sim, vx, vy, "Americans", false, None, &empty_heights, None);
    assert_eq!(picked_visible, Some(3));

    let still_hidden =
        pick_enemy_target_stable_id(&sim, hx, hy, "Americans", false, None, &empty_heights, None);
    assert_ne!(still_hidden, Some(2));
}

#[test]
fn test_hover_target_distinguishes_friendly_and_enemy_categories() {
    let mut sim = Simulation::new();
    let americans_id = sim.interner.intern("Americans");
    let soviet_id = sim.interner.intern("Soviet");
    let gapowr_id = sim.interner.intern("GAPOWR");
    let e1_id = sim.interner.intern("E1");

    // Hover coordinates come from the drawn position, which is the cell
    // centre — the point `screen_to_iso` resolves back to the right cell.
    let friendly = GameEntity::new_at_frame_zero_for_test(
        10,
        5,
        5,
        0,
        0,
        americans_id,
        Health { current: 100 },
        gapowr_id,
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    sim.entities_mut().insert(friendly);
    let (friendly_sx, friendly_sy) =
        crate::render::locomotor_visual::screen_position(sim.entities().get(10).expect("friendly"));

    let enemy = GameEntity::new_at_frame_zero_for_test(
        11,
        20,
        5,
        0,
        0,
        soviet_id,
        Health { current: 100 },
        e1_id,
        EntityCategory::Unit,
        0,
        5,
        false,
    );
    sim.entities_mut().insert(enemy);
    let (enemy_sx, enemy_sy) =
        crate::render::locomotor_visual::screen_position(sim.entities().get(11).expect("enemy"));
    sim.fog
        .mark_visible_for_owner(crate::sim::intern::test_intern("Americans"), 20, 5);

    // Provide empty height maps — structure picking now uses foundation-based hit testing.
    let empty_heights: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let friendly_hover = hover_target_at_point(
        &sim,
        friendly_sx,
        friendly_sy,
        "Americans",
        false,
        None,
        &empty_heights,
        None,
    )
    .expect("friendly hover");
    assert_eq!(friendly_hover.kind, HoverTargetKind::FriendlyStructure);
    assert_eq!(friendly_hover.stable_id, 10);

    let enemy_hover = hover_target_at_point(
        &sim,
        enemy_sx,
        enemy_sy,
        "Americans",
        false,
        None,
        &empty_heights,
        None,
    )
    .expect("enemy hover");
    assert_eq!(enemy_hover.kind, HoverTargetKind::EnemyUnit);
    assert_eq!(enemy_hover.stable_id, 11);
}

#[test]
fn test_ready_buildings_do_not_auto_arm_placement() {
    let mut armed: Option<crate::app::types::TargetingMode> = None;
    let mut preview = None;
    let ready = vec![ReadyBuildingView {
        type_id: crate::sim::intern::test_intern("GAPOWR"),
        display_name: "Power Plant".to_string(),
        queue_category: crate::sim::production::ProductionCategory::Building,
    }];

    sync_targeting_mode(&mut armed, &mut preview, &ready, &[], None);

    assert!(
        armed.is_none(),
        "ready building should not auto-arm placement"
    );
    assert!(preview.is_none());
}

#[test]
fn test_invalid_armed_building_clears_when_not_ready() {
    let mut armed = Some(crate::app::types::TargetingMode::BuildingPlacement(
        "GAPOWR".to_string(),
    ));
    let mut preview = Some(crate::sim::production::BuildingPlacementPreview {
        type_id: crate::sim::intern::test_intern("GAPOWR"),
        rx: 5,
        ry: 5,
        width: 2,
        height: 2,
        valid: false,
        reason: None,
        cell_valid: vec![false; 4],
        wall_autofill_cells: Vec::new(),
    });

    sync_targeting_mode(&mut armed, &mut preview, &[], &[], None);

    assert!(armed.is_none());
    assert!(preview.is_none());
}

#[test]
fn test_sw_armed_preserved_when_ready() {
    use crate::sim::superweapon::SuperWeaponView;
    let mut armed = Some(crate::app::types::TargetingMode::SuperWeapon(
        "LightningStormSpecial".to_string(),
    ));
    let mut preview = None;
    let sw = SuperWeaponView {
        type_id: crate::sim::intern::test_intern("LightningStormSpecial"),
        display_name: "LightningStormSpecial".to_string(),
        progress: 1.0,
        is_ready: true,
        is_online: true,
        sidebar_image: Some("INTICON".to_string()),
        kind: crate::rules::superweapon_type::SuperWeaponKind::LightningStorm,
    };

    sync_targeting_mode(&mut armed, &mut preview, &[], &[sw], None);

    assert!(armed.is_some(), "armed SW should be preserved while ready");
}

#[test]
fn test_sw_armed_cleared_when_not_ready() {
    use crate::sim::superweapon::SuperWeaponView;
    let mut armed = Some(crate::app::types::TargetingMode::SuperWeapon(
        "LightningStormSpecial".to_string(),
    ));
    let mut preview = None;
    let sw = SuperWeaponView {
        type_id: crate::sim::intern::test_intern("LightningStormSpecial"),
        display_name: "LightningStormSpecial".to_string(),
        progress: 0.5,
        is_ready: false, // Charging, not yet ready.
        is_online: true,
        sidebar_image: Some("INTICON".to_string()),
        kind: crate::rules::superweapon_type::SuperWeaponKind::LightningStorm,
    };

    sync_targeting_mode(&mut armed, &mut preview, &[], &[sw], None);

    assert!(armed.is_none(), "armed SW should clear when not ready");
}

#[test]
fn test_sw_armed_cleared_when_view_gone() {
    let mut armed = Some(crate::app::types::TargetingMode::SuperWeapon(
        "LightningStormSpecial".to_string(),
    ));
    let mut preview = None;

    // No SW views — granting building destroyed.
    sync_targeting_mode(&mut armed, &mut preview, &[], &[], None);

    assert!(
        armed.is_none(),
        "armed SW should clear when view disappears"
    );
}
