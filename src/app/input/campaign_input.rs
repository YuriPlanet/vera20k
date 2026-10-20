//! Presentation effects of original input46/47. ScenarioSession is the sole
//! gameplay lock owner; this leaf coordinates existing input/display owners.

use crate::app::AppState;

/// Native mirror A8ED9C is copied from Scenario+35A2. App consumers read the
/// session owner directly, including before its presentation effect drains.
pub(crate) fn is_locked(state: &AppState) -> bool {
    state
        .match_state
        .sim_runtime
        .as_ref()
        .is_some_and(|runtime| {
            runtime.simulation.session().input_locked()
                || (runtime.simulation.session().scenario_paused()
                    && matches!(
                        state.match_state.session.mode(),
                        crate::app::match_runtime::session::SessionMode::Campaign
                            | crate::app::match_runtime::session::SessionMode::Skirmish
                    ))
        })
}

/// Ordered effect emitted after the authoritative Scenario mutation. The
/// prior flag is carried by that owner: Unlock684290 restores cursor depth
/// only for prior byte1, including after repeated locks.
pub(crate) fn apply_lock_effect(state: &mut AppState, locked: bool, was_locked: bool) {
    if locked {
        state
            .match_state
            .input
            .cancel_tactical_gesture_and_targeting();
        state
            .match_state
            .match_presentation
            .sidebar_gadget_state
            .disable_selection_modes(false);
        state
            .match_state
            .match_presentation
            .in_game_gadgets
            .cancel_pointer_capture();
        crate::app::input::cursor::reset_cursor_shape();
    }
    state
        .match_state
        .input
        .cursor_visibility
        .apply_input_lock(locked, was_locked);
    state.match_state.input.clear_command_latches();
    crate::app::presentation::sidebar_render::refresh_sidebar_projection(state);
}

#[cfg(test)]
mod tests {
    use crate::app::input::cursor::CursorVisibility;
    use crate::app::input::state::MatchInputState;
    use crate::app::types::{OrderMode, TargetingMode};
    use winit::keyboard::KeyCode;

    #[test]
    fn lock_cleanup_cancels_gestures_and_targets_without_clearing_selection_or_held_keys() {
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
            input.minimap_dragging = true;
            input.selection_state.begin_drag(10.0, 20.0);
            input.selection_state.update_drag(40.0, 50.0);
            assert!(input.tactical_mouse.begin_left_press());
            input.tactical_mouse.right_threshold_crossed = true;
            input.selection_order = vec![11, 12];
            input.control_groups[3] = vec![11, 12];
            input.follow_target = Some(12);
            input.camera_x = 30.0;
            input.camera_y = 60.0;
            input.keys_held.insert(KeyCode::ArrowLeft);
            input.keys_held.insert(KeyCode::KeyT);
            input
                .type_select
                .press(KeyCode::KeyT, std::time::Instant::now(), false);
            input.queued_order_mode = OrderMode::AttackMove;

            input.cancel_tactical_gesture_and_targeting();
            input.clear_command_latches();

            assert!(input.targeting_mode.is_none());
            assert!(input.building_placement_preview.is_none());
            assert!(!input.minimap_dragging);
            assert!(!input.selection_state.is_dragging());
            assert!(!input.tactical_mouse.captured);
            assert!(input.tactical_mouse.left_held());
            assert!(input.tactical_mouse.right_threshold_crossed);
            assert_eq!(input.selection_order, [11, 12]);
            assert_eq!(input.control_groups[3], [11, 12]);
            assert_eq!(input.follow_target, Some(12));
            assert_eq!((input.camera_x, input.camera_y), (30.0, 60.0));
            assert!(input.keys_held.contains(&KeyCode::ArrowLeft));
            assert!(input.keys_held.contains(&KeyCode::KeyT));
            assert!(!input.type_select.held());
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
            input.minimap_dragging = true;
            input.selection_state.begin_drag(10.0, 20.0);
            input.selection_state.update_drag(40.0, 50.0);
            assert!(input.tactical_mouse.begin_left_press());

            input.cancel_tactical_drag();

            assert!(!input.minimap_dragging);
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
}
