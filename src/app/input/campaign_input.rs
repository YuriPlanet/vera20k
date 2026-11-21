//! Presentation effects of original input46/47. ScenarioSession is the sole
//! gameplay lock owner; this leaf coordinates existing input/display owners.

use crate::app::AppState;

/// Native mirror A8ED9C is copied from Scenario+35A2. App consumers read the
/// session owner directly, including before its presentation effect drains.
pub(crate) fn is_locked(state: &AppState) -> bool {
    state
        .match_state
        .sim_runtime
        .runtime()
        .is_some_and(|runtime| {
            runtime.simulation.session().input_locked()
                || (runtime.simulation.session().scenario_paused()
                    && matches!(
                        state.match_state.session_mode(),
                        crate::app::match_runtime::session::SessionMode::Campaign
                            | crate::app::match_runtime::session::SessionMode::Skirmish
                    ))
        })
}

/// TActionClass::Execute 0x006DD8B0, case 46 (`0x006DED8B..0x006DEDD7`;
/// `0x006E2660` is an unreferenced copy), nulls the current placement object, type and
/// house and the active foundation (SetActiveFoundation 0x004A8BF0), then
/// SetRepairMode(0) 0x004AC8C0 and SetSellMode(0) 0x004AC660, which always
/// clear because the type is already null, and the Power, Planning and
/// Beacon bytes +0x11B2..+0x11B4. The pending foundation 0x0088097C and the
/// super pointer +0x11B8 stay armed.
pub(crate) fn cancel_placement_modes(
    input: &mut crate::app::input::state::MatchInputState,
    gadgets: &mut crate::ui::sidebar::gadget_flash::SidebarGadgetState,
) {
    input.clear_building_placement();
    gadgets.disable_cursor_modes();
}

/// Ordered effect emitted after the authoritative Scenario mutation. The
/// prior flag is carried by that owner: Unlock684290 restores cursor depth
/// only for prior byte1, including after repeated locks.
pub(crate) fn apply_lock_effect(state: &mut AppState, locked: bool, was_locked: bool) {
    if locked {
        state
            .match_state
            .match_presentation
            .in_game_gadgets
            .cancel_pointer_capture();
        state.match_state.input.reset_cursor_shape();
    }
    lock_input(&mut state.match_state.input, locked, was_locked);
    crate::app::presentation::sidebar_render::refresh_sidebar_projection(state);
}

/// The input half of a lock. Lock_User_Input 0x00684240 (and the pause's
/// inline copy at 0x00684060) cancels the drag through
/// ScrollClass::CancelMouseDrag 0x006938C0; armed placement, cursor and super
/// modes stay.
fn lock_input(
    input: &mut crate::app::input::state::MatchInputState,
    locked: bool,
    was_locked: bool,
) {
    if locked {
        input.cancel_tactical_drag();
    }
    input.cursor_visibility.apply_input_lock(locked, was_locked);
    input.clear_command_latches();
}

#[cfg(all(test, tests_app_input_a_c))]
mod tests {
    use crate::app::input::cursor::CursorVisibility;
    use crate::app::input::state::MatchInputState;
    use crate::app::types::{OrderMode, TargetingMode};
    use winit::keyboard::KeyCode;

    #[test]
    fn lock_cleanup_cancels_gestures_but_keeps_armed_modes_selection_and_held_keys() {
        let mut interner = crate::sim::intern::StringInterner::new();
        let owner = interner.intern("FixtureHouse");
        for building in [true, false] {
            let mut input = MatchInputState::new(
                crate::app::input::hotkeys::HotkeyBindings::from_ini_bytes(None),
            );
            if building {
                assert!(input.begin_building_placement("FixtureBuilding", 23, owner,));
            } else {
                input.targeting_mode = Some(TargetingMode::SuperWeapon("FixtureSuper".to_owned()));
            }
            input.building_placement_preview =
                Some(crate::sim::production::BuildingPlacementPreview {
                    rx: 10,
                    ry: 11,
                    width: 1,
                    height: 1,
                    valid: true,
                    in_build_area: true,
                    foundation_visible: true,
                    reason: None,
                    cells: vec![crate::sim::production::BuildingPlacementCell {
                        coord: (10, 11),
                        valid: true,
                        level: 0,
                        slope: 0,
                    }],
                    wall_autofill_cells: vec![],
                });
            assert!(input.begin_minimap_press(winit::event::MouseButton::Left));
            input.selection_state.begin_drag(10.0, 20.0);
            input.selection_state.update_drag(40.0, 50.0);
            assert!(input.tactical_mouse.begin_left_press());
            input.tactical_mouse.right_threshold_crossed = true;
            input.selection_order = vec![11, 12];
            input.follow_target = Some(12);
            input.camera_x = 30.0;
            input.camera_y = 60.0;
            input.keys_held.insert(KeyCode::ArrowLeft);
            input.keys_held.insert(KeyCode::KeyT);
            input
                .type_select
                .press(KeyCode::KeyT, std::time::Instant::now(), false);
            input.queued_order_mode = OrderMode::AttackMove;

            super::lock_input(&mut input, true, false);

            assert!(input.targeting_mode.is_some());
            assert!(input.building_placement_preview.is_some());
            assert!(!input.minimap_press_active());
            assert!(!input.selection_state.is_dragging());
            assert!(!input.tactical_mouse.captured);
            assert!(input.tactical_mouse.left_held());
            assert!(input.tactical_mouse.right_threshold_crossed);
            assert_eq!(input.selection_order, [11, 12]);
            assert_eq!(input.follow_target, Some(12));
            assert_eq!((input.camera_x, input.camera_y), (30.0, 60.0));
            assert!(input.keys_held.contains(&KeyCode::ArrowLeft));
            assert!(input.keys_held.contains(&KeyCode::KeyT));
            // TypeSelect's latch 0x00B0FE65 survives the lock.
            assert!(input.type_select.held());
            assert_eq!(input.queued_order_mode, OrderMode::Move);
        }
    }

    /// Deactivate683EB0/4AEAD0's teardown (the pause-overlay path): drop the
    /// tactical capture, band drag and minimap gesture, but keep an armed
    /// building placement / superweapon targeting and the physical button
    /// record — the overlay cancels a drag, never a pending placement.
    #[test]
    fn pause_teardown_cancels_drag_but_keeps_targeting_modes() {
        let mut interner = crate::sim::intern::StringInterner::new();
        let owner = interner.intern("FixtureHouse");
        for building in [true, false] {
            let mut input = MatchInputState::new(
                crate::app::input::hotkeys::HotkeyBindings::from_ini_bytes(None),
            );
            if building {
                assert!(input.begin_building_placement("FixtureBuilding", 23, owner,));
            } else {
                input.targeting_mode = Some(TargetingMode::SuperWeapon("FixtureSuper".to_owned()));
            }
            input.building_placement_preview =
                Some(crate::sim::production::BuildingPlacementPreview {
                    rx: 10,
                    ry: 11,
                    width: 1,
                    height: 1,
                    valid: true,
                    in_build_area: true,
                    foundation_visible: true,
                    reason: None,
                    cells: vec![crate::sim::production::BuildingPlacementCell {
                        coord: (10, 11),
                        valid: true,
                        level: 0,
                        slope: 0,
                    }],
                    wall_autofill_cells: vec![],
                });
            // All three latches the swallowed release used to strand.
            assert!(input.begin_minimap_press(winit::event::MouseButton::Left));
            input.selection_state.begin_drag(10.0, 20.0);
            input.selection_state.update_drag(40.0, 50.0);
            assert!(input.tactical_mouse.begin_left_press());

            input.cancel_tactical_drag();

            assert!(!input.minimap_press_active());
            assert!(!input.selection_state.is_dragging());
            assert!(!input.tactical_mouse.captured);
            assert!(
                input.tactical_mouse.left_held(),
                "physical button state survives — the OS still holds it",
            );
            assert!(
                input.targeting_mode.is_some(),
                "the armed cursor mode must survive the pause teardown",
            );
            assert!(input.building_placement_preview.is_some());
        }
    }

    #[test]
    fn native_input_histories_preserve_cursor_depth_and_scenario_prior_gate() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/trigger_action_values.json",
        ))
        .unwrap();
        let mut compared = 0;
        for row in rows
            .iter()
            .filter(|row| row["case_kind"] == "input_history")
        {
            let steps = row["history"].as_array().unwrap();
            let mut cursor = CursorVisibility::default();
            cursor.restore_level(steps[0]["before"]["show_level"].as_i64().unwrap() as i32);
            for step in steps {
                assert_eq!(
                    i64::from(cursor.level()),
                    step["before"]["show_level"].as_i64().unwrap()
                );
                cursor.apply_input_lock(step["kind"] == 46, step["before"]["scenario_locked"] == 1);
                assert_eq!(
                    i64::from(cursor.level()),
                    step["after"]["show_level"].as_i64().unwrap(),
                    "{}",
                    row["input"]["name"]
                );
                assert_eq!(
                    cursor.is_visible(),
                    step["after"]["show_level"].as_i64().unwrap() >= 0
                );
            }
            // Native LoadMission retains the pre-load client scalar.
            let mut restored = CursorVisibility::default();
            restored.restore_level(cursor.level());
            assert_eq!(restored, cursor);
            compared += 1;
        }
        assert_eq!(compared, 8);
    }

    #[test]
    fn leaving_the_menus_hides_the_pointer_only_while_the_game_is_locked() {
        // A cutscene hides the pointer; the menus show it; a load keeps
        // that depth; leaving reads the lock of the game now running.
        for (locked_on_leaving, visible) in [(false, true), (true, false)] {
            let mut cursor = CursorVisibility::default();
            cursor.apply_input_lock(true, false);
            cursor.show_for_menus();
            assert!(cursor.is_visible());
            cursor.leave_menus(locked_on_leaving);
            assert_eq!(cursor.is_visible(), visible);
        }
        // Resumed during the cutscene, its unlock still shows the pointer.
        let mut cursor = CursorVisibility::default();
        cursor.apply_input_lock(true, false);
        cursor.show_for_menus();
        cursor.leave_menus(true);
        cursor.apply_input_lock(false, true);
        assert!(cursor.is_visible());
    }

    /// TAction 46 (`0x006DED8B..0x006DEDD7`) drops the current placement and the cursor
    /// modes, but never the super pointer +0x11B8.
    #[test]
    fn taction_46_drops_placement_and_cursor_modes_but_keeps_the_super_pointer() {
        let mut interner = crate::sim::intern::StringInterner::new();
        let owner = interner.intern("FixtureHouse");
        for building in [true, false] {
            let mut input = MatchInputState::new(
                crate::app::input::hotkeys::HotkeyBindings::from_ini_bytes(None),
            );
            if building {
                assert!(input.begin_building_placement("FixtureBuilding", 23, owner));
            } else {
                input.targeting_mode = Some(TargetingMode::SuperWeapon("FixtureSuper".to_owned()));
            }
            let mut gadgets = crate::ui::sidebar::gadget_flash::SidebarGadgetState::new();
            gadgets.repair_mode_on = true;
            gadgets.place_beacon_mode_on = true;

            super::cancel_placement_modes(&mut input, &mut gadgets);

            assert_eq!(input.targeting_mode.is_some(), !building);
            assert!(!gadgets.repair_mode_on && !gadgets.place_beacon_mode_on);
        }
    }
}
