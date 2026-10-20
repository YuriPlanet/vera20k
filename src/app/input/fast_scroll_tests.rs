//! Original-executable observations, never regenerated from the Rust port.
use super::{RightDragPoll, TacticalMouseState, right_drag_pan_step};
use crate::app::match_runtime::session::SessionMode;
use crate::app::match_runtime::sim_tick::{RuntimePassInputs, decide_runtime_pass};
use serde::Deserialize;

#[derive(Deserialize)]
struct Expected {
    crossed: bool,
    engaged: bool,
    motion: [i32; 2],
    band_box_cancel_points: Vec<[i32; 2]>,
}

#[derive(Deserialize)]
struct DragCase {
    name: String,
    anchor: [f32; 2],
    cursor: [f32; 2],
    viewport: [f32; 2],
    drag_metrics: [i32; 2],
    scroll_rate: u32,
    scroll_method: i32,
    crossed_before: bool,
    engaged_before: bool,
    band_box: bool,
    game_active: bool,
    tactical_active: bool,
    map_editor: bool,
    expected: Expected,
}

fn vectors() -> &'static serde_json::Value {
    crate::test_fixture::json("tools/input_oracle/fast_scroll.json")
}

fn mouse_for(case: &DragCase) -> TacticalMouseState {
    TacticalMouseState {
        captured: true,
        right_held: true,
        right_anchor: case.anchor.into(),
        right_threshold_crossed: case.crossed_before,
        right_pan_engaged: case.engaged_before,
        ..Default::default()
    }
}

fn check_poll(mouse: &mut TacticalMouseState, case: &DragCase) {
    let poll = mouse.poll_right_drag(case.cursor.into(), case.drag_metrics.into(), case.band_box);
    let motion = if poll == RightDragPoll::Pan {
        right_drag_pan_step(
            mouse.right_anchor,
            case.cursor.into(),
            case.viewport[0],
            case.viewport[1],
            case.scroll_rate,
        )
    } else {
        (0.0, 0.0)
    };
    assert_eq!(
        motion,
        (
            case.expected.motion[0] as f32,
            case.expected.motion[1] as f32
        ),
        "{}",
        case.name
    );
    assert_eq!(
        mouse.right_threshold_crossed, case.expected.crossed,
        "{}",
        case.name
    );
    assert_eq!(
        mouse.right_pan_engaged, case.expected.engaged,
        "{}",
        case.name
    );
    assert_eq!(
        poll == RightDragPoll::CancelBandBox,
        !case.expected.band_box_cancel_points.is_empty(),
        "{}",
        case.name
    );
}

#[test]
fn stock_right_drag_matches_original_executable() {
    let cases: Vec<DragCase> = serde_json::from_value(vectors()["cases"].clone()).unwrap();
    let mut checked = 0;
    for case in cases {
        // The live stock path is method 0 in an active tactical game. Alternate
        // warp methods/editor admission remain characterization, not Rust parity.
        if case.scroll_method != 0 || !case.game_active || !case.tactical_active || case.map_editor
        {
            continue;
        }
        check_poll(&mut mouse_for(&case), &case);
        checked += 1;
    }
    assert_eq!(checked, 738);
}

#[test]
fn held_pointer_and_return_to_anchor_match_native_sequence() {
    let data = vectors();
    for sequence in data["sequences"].as_array().unwrap() {
        let mut input = sequence["input"].clone();
        input["expected"] = sequence["frames"][0]["expected"].clone();
        let mut case: DragCase = serde_json::from_value(input).unwrap();
        let mut mouse = mouse_for(&case);
        for frame in sequence["frames"].as_array().unwrap() {
            case.cursor = serde_json::from_value(frame["cursor"].clone()).unwrap();
            case.expected = serde_json::from_value(frame["expected"].clone()).unwrap();
            check_poll(&mut mouse, &case);
        }
    }
}

#[test]
fn right_release_cancel_decision_matches_native_messages() {
    let data = vectors();
    let mut checked = 0;
    for row in data["message_cases"].as_array().unwrap() {
        if row["message"] != 0x205 {
            continue;
        }
        let mut mouse = TacticalMouseState {
            captured: row["captured"].as_bool().unwrap(),
            right_held: true,
            right_threshold_crossed: row["crossed_before"].as_bool().unwrap(),
            ..Default::default()
        };
        let expected = &row["expected"];
        let cancel = mouse.end_right_press();
        let calls = expected["calls"].as_array().unwrap();
        assert_eq!(
            cancel.is_some(),
            calls.iter().any(|call| call == "release_capture")
        );
        assert_eq!(
            cancel == Some(true),
            calls.iter().any(|call| call == "cancel_selection_or_mode")
        );
        assert_eq!(mouse.captured, expected["captured"].as_bool().unwrap());
        assert_eq!(
            mouse.right_threshold_crossed,
            expected["crossed"].as_bool().unwrap()
        );
        checked += 1;
    }
    assert_eq!(checked, 4);
}

#[test]
fn capture_and_left_button_priority_match_native_update() {
    let data = vectors();
    let mut checked = 0;
    for row in data["update_cases"].as_array().unwrap() {
        if !matches!(
            row["name"].as_str().unwrap(),
            "held_right" | "held_both" | "held_left" | "no_live_buttons" | "uncaptured_right"
        ) {
            continue;
        }
        let mut input = row.clone();
        input["expected"] = row["expected"]["drag"].clone();
        let case: DragCase = serde_json::from_value(input).unwrap();
        let mut mouse = mouse_for(&case);
        mouse.captured = row["captured"].as_bool().unwrap();
        mouse.left_held = row["left_held"].as_bool().unwrap();
        mouse.right_held = row["right_held"].as_bool().unwrap();
        check_poll(&mut mouse, &case);
        checked += 1;
    }
    assert_eq!(checked, 5);
}

#[test]
fn locked_physical_edges_drive_native_button_priority_after_unlock() {
    use winit::event::MouseButton;
    let data = vectors();
    let mut checked = 0;
    for row in data["update_cases"].as_array().unwrap() {
        let released_during_lock = match row["name"].as_str().unwrap() {
            "held_right" => true,
            "held_both" => false,
            _ => continue,
        };
        let mut input = row.clone();
        input["expected"] = row["expected"]["drag"].clone();
        let case: DragCase = serde_json::from_value(input).unwrap();
        let mut mouse = TacticalMouseState::default();
        if released_during_lock {
            assert!(mouse.begin_left_press());
            mouse.cancel_mouse_drag();
            mouse.record_physical_button(MouseButton::Left, false);
        } else {
            // A consumed cinematic/menu press updates physical state without
            // arming a gesture; new-view initialization must retain it too.
            mouse.record_physical_button(MouseButton::Left, true);
            mouse.reset_for_new_view();
        }
        assert!(!mouse.captured);
        assert_eq!(mouse.left_held(), !released_during_lock);
        mouse.record_physical_button(MouseButton::Right, true);
        mouse.begin_right_drag(case.anchor.into());
        check_poll(&mut mouse, &case);
        checked += 1;
    }
    assert_eq!(checked, 2);
}

#[test]
fn a_drag_between_offline_polls_remains_a_cancel_click() {
    let data = vectors();
    let mut checked = 0;
    for row in data["throttle_cases"].as_array().unwrap() {
        let mode = row["session_mode"].as_i64().unwrap() as i32;
        if !matches!(mode, 0 | 5) {
            continue;
        }
        let mut mouse = TacticalMouseState {
            right_held: true,
            ..Default::default()
        };
        mouse.begin_right_drag((400.0, 300.0));
        let decision = decide_runtime_pass(RuntimePassInputs {
            exact_step: false,
            window_active: true,
            focus_frozen: false,
            startup_admitted: true,
            frame_stepping: false,
            paused: false,
            scenario_paused: false,
            menu_open: false,
            session_mode: SessionMode::from_game_mode(mode),
            pacer_timing_admits: false,
            network_wait: false,
            network_dialog_open: false,
        });
        assert_eq!(
            usize::from(decision.scroll_input),
            row["expected"]["input_calls"].as_u64().unwrap() as usize
        );
        if decision.scroll_input {
            mouse.poll_right_drag((500.0, 300.0), (4, 4), false);
        }
        // Moving far and releasing during the throttle wait cannot cross a
        // latch which only the native admitted poll is allowed to write.
        assert_eq!(mouse.end_right_press(), Some(true), "{}", row["name"]);
        checked += 1;
    }
    assert_eq!(checked, 10);
}

#[derive(Clone, Deserialize)]
struct CameraSequence {
    name: String,
    viewport: [f32; 2],
    map_rect_width: i32,
    local_size: [i32; 4],
    initial_center_seed: [f32; 2],
    initial_center: [f32; 2],
    drag_anchor: [f32; 2],
    scroll_rate: u32,
    drag_metrics: [i32; 2],
    steps: Vec<serde_json::Value>,
}

/// The same final MapClass fields used by the production scroll clamp.
fn camera_fixture_clamp(sequence: &CameraSequence) -> impl Fn((f32, f32)) -> (f32, f32) + use<> {
    fixture_clamp(
        sequence.map_rect_width,
        sequence.local_size,
        sequence.viewport,
    )
}

/// Native clamp6D8640 reads these final normalized MapClass fields; radar's
/// enumerated tile extent is a separate consumer and must not supply the
/// camera's vertical headroom.
fn fixture_clamp(
    map_rect_width: i32,
    local_size: [i32; 4],
    viewport: [f32; 2],
) -> impl Fn((f32, f32)) -> (f32, f32) + use<> {
    let [left, top, width, height] = local_size;
    let bounds = crate::map::playfield::PlayfieldBounds::from_normalized_local_size(
        map_rect_width,
        left,
        top,
        width,
        height,
    );
    let viewport = viewport.into();
    move |point| super::clamp_camera_point_to_local_bounds(point, bounds, viewport, 1.0)
}

fn native_camera_to_world(center: [f32; 2], viewport: [f32; 2]) -> (f32, f32) {
    let bias = crate::util::lepton::absolute_leptons_to_screen(0, 0, 0);
    super::tactical_camera_top_left(
        (center[0] + bias.0, center[1] + bias.1),
        viewport[0],
        viewport[1],
        1.0,
    )
}

#[test]
fn pending_requests_commit_once_and_absolute_views_discard_them() {
    use super::{PendingCameraScroll, tactical_camera_top_left};
    let sequences: Vec<CameraSequence> =
        serde_json::from_value(vectors()["camera_sequences"].clone()).unwrap();
    assert_eq!(sequences.len(), 10);
    let mut commits = 0;
    for sequence in sequences {
        let clamp = camera_fixture_clamp(&sequence);
        // Original refresh0x6D8B30 converts center to top-left with viewport/2.
        // VERA's shared projection adds its world-row bias; use that owner to
        // obtain the offset, rather than duplicating its literal15 here.
        let world_bias = crate::util::lepton::absolute_leptons_to_screen(0, 0, 0);
        let native_to_world = |center: [f32; 2]| {
            tactical_camera_top_left(
                (center[0] + world_bias.0, center[1] + world_bias.1),
                sequence.viewport[0],
                sequence.viewport[1],
                1.0,
            )
        };
        let mut current = clamp(native_to_world(sequence.initial_center_seed));
        assert_eq!(
            current,
            native_to_world(sequence.initial_center),
            "{} initial clamp",
            sequence.name
        );
        let mut pending = PendingCameraScroll::default();
        let mut mouse_request = (0.0, 0.0);
        let mut mouse = TacticalMouseState {
            right_held: true,
            ..Default::default()
        };
        mouse.begin_right_drag(sequence.drag_anchor.into());
        for step in sequence.steps {
            match step["operation"].as_str().unwrap() {
                "scroll" => {
                    let direction = step["direction"].as_u64().unwrap() as usize;
                    let distance = step["distance"].as_f64().unwrap() as f32;
                    let (dx, dy) = super::OCTANT_DELTA[direction];
                    pending.request((dx * distance, dy * distance));
                }
                "drag" => {
                    let cursor: [f32; 2] = serde_json::from_value(step["cursor"].clone()).unwrap();
                    assert_eq!(
                        mouse.poll_right_drag(cursor.into(), sequence.drag_metrics.into(), false),
                        RightDragPoll::Pan
                    );
                    let delta = right_drag_pan_step(
                        sequence.drag_anchor.into(),
                        cursor.into(),
                        sequence.viewport[0],
                        sequence.viewport[1],
                        sequence.scroll_rate,
                    );
                    mouse_request.0 += delta.0;
                    mouse_request.1 += delta.1;
                }
                "commit" => {
                    current = std::mem::take(&mut pending).commit(current, mouse_request, &clamp);
                    mouse_request = (0.0, 0.0);
                    commits += 1;
                }
                "set_view" => {
                    let coord: [i32; 3] = serde_json::from_value(step["coord"].clone()).unwrap();
                    let world = crate::util::lepton::absolute_leptons_to_screen(
                        coord[0], coord[1], coord[2],
                    );
                    current = clamp(tactical_camera_top_left(
                        world,
                        sequence.viewport[0],
                        sequence.viewport[1],
                        1.0,
                    ));
                    pending.clear();
                    mouse_request = (0.0, 0.0);
                }
                operation => panic!("unexpected native camera operation {operation}"),
            }
            let expected: [f32; 2] =
                serde_json::from_value(step["expected"]["committed_center"].clone()).unwrap();
            assert_eq!(
                current,
                native_to_world(expected),
                "{} after {}",
                sequence.name,
                step["operation"]
            );
        }
        assert!(
            pending.is_idle(),
            "{} must leave no stale keyboard request",
            sequence.name
        );
    }
    assert_eq!(commits, 13);
}

#[test]
fn cinematic_focus_matches_native_frame_gates_projection_cleanup_and_restored_state() {
    use super::{CameraAiFrame, CameraFocus, PendingCameraScroll, tactical_camera_top_left};
    let mut sequences: Vec<CameraSequence> =
        serde_json::from_value(vectors()["focus_sequences"].clone()).unwrap();
    assert_eq!(sequences.len(), 22);
    // This native control records alternating inverted bounds. VERA centers
    // oversized views instead; camera_bounds_tests covers that deliberate
    // presentation difference without rewriting the executable observations.
    sequences.retain(|sequence| sequence.name != "focus_view_larger_than_local_rect");
    assert_eq!(sequences.len(), 21);
    let lifecycle: Vec<CameraSequence> =
        serde_json::from_value(vectors()["camera_lifecycle_sequences"].clone()).unwrap();
    assert_eq!(lifecycle.len(), 4);
    sequences.extend(lifecycle);
    let mut visits = 0;
    for sequence in sequences {
        let mut clamp = camera_fixture_clamp(&sequence);
        let native_to_world = |point| native_camera_to_world(point, sequence.viewport);
        let mut current = clamp(native_to_world(sequence.initial_center_seed));
        assert_eq!(current, native_to_world(sequence.initial_center));
        // These original controls supply last-AI=0 explicitly. The actual
        // Tactical constructor uses -1, covered by its lifecycle controls.
        let mut pending = PendingCameraScroll {
            last_ai_frame: 0,
            ..Default::default()
        };
        let mut frame = 0;
        let mut started = true;
        let mut replay_blocked = false;
        for step in &sequence.steps {
            let operation = step["operation"].as_str().unwrap();
            match operation {
                "constructor" => {
                    pending = PendingCameraScroll::default();
                    current = native_to_world([0.0, 0.0]);
                }
                "scroll" => {
                    let direction = step["direction"].as_u64().unwrap() as usize;
                    let distance = step["distance"].as_f64().unwrap() as f32;
                    let (dx, dy) = super::OCTANT_DELTA[direction];
                    pending.request((dx * distance, dy * distance));
                }
                "save_restore" => {
                    use crate::sim::snapshot::GameSnapshot;
                    let saved_view = pending.saved_view(
                        super::tactical_camera_center(
                            current,
                            sequence.viewport[0],
                            sequence.viewport[1],
                            1.0,
                        ),
                        1.0,
                    );
                    let mut sim = crate::sim::world::Simulation::new();
                    sim.session_mut_for_test().set_binary_frame_for_test(frame);
                    let bytes = GameSnapshot::save_validated_with_presentation(
                        &sim,
                        0,
                        0,
                        "native camera lifecycle",
                        0,
                        &[],
                        None,
                        Some(&saved_view),
                        None,
                        None,
                    );
                    let snapshot = GameSnapshot::load(&bytes).unwrap();
                    let restored = snapshot.tactical_view().unwrap();
                    super::validate_saved_tactical_view(restored).unwrap();
                    assert_eq!(snapshot.sim.session().binary_frame(), frame);
                    if let Some(rect) = step.get("local_size") {
                        let mut geometry = sequence.clone();
                        geometry.local_size = serde_json::from_value(rect.clone()).unwrap();
                        clamp = camera_fixture_clamp(&geometry);
                    }
                    pending = PendingCameraScroll::default();
                    pending.restore_saved_view(restored);
                    current = clamp(tactical_camera_top_left(
                        (
                            restored.view_center_world_pixels[0],
                            restored.view_center_world_pixels[1],
                        ),
                        sequence.viewport[0],
                        sequence.viewport[1],
                        1.0,
                    ));
                    assert_eq!(step["expected"]["stream"]["consumed"], 3612);
                }
                "focus" => {
                    let coord = serde_json::from_value(step["coord"].clone()).unwrap();
                    assert!(pending.focus_on(
                        current,
                        coord,
                        step["velocity"].as_i64().unwrap() as i32,
                        sequence.viewport.into(),
                        1.0,
                    ));
                }
                "commit" => {
                    frame = step["frame"].as_u64().unwrap() as u32;
                    current = pending.advance_ai(
                        current,
                        (0.0, 0.0),
                        CameraAiFrame {
                            frame,
                            scenario_started: started,
                            replay_blocked,
                        },
                        sequence.viewport.into(),
                        1.0,
                        &clamp,
                    );
                    if frame == 2 {
                        // This checks the camera owner's serialization only;
                        // production save/load integration has a separate gate.
                        pending =
                            serde_json::from_slice(&serde_json::to_vec(&pending).unwrap()).unwrap();
                    }
                }
                "scenario_started" => started = step["value"].as_bool().unwrap(),
                "replay_flags" => {
                    replay_blocked = step["value"].as_u64().unwrap() & 2 != 0;
                }
                "set_view" => {
                    let coord: [i32; 3] = serde_json::from_value(step["coord"].clone()).unwrap();
                    current = clamp(tactical_camera_top_left(
                        crate::util::lepton::absolute_leptons_to_screen(
                            coord[0], coord[1], coord[2],
                        ),
                        sequence.viewport[0],
                        sequence.viewport[1],
                        1.0,
                    ));
                    pending.clear();
                }
                unknown => panic!("unexpected camera operation {unknown}"),
            }
            let expected = &step["expected"];
            let center = serde_json::from_value(expected["committed_center"].clone()).unwrap();
            assert_eq!(
                current,
                native_to_world(center),
                "{} {operation} frame{frame}",
                sequence.name
            );
            let requested = serde_json::from_value(expected["requested_center"].clone()).unwrap();
            assert_eq!(
                (
                    current.0 + pending.world_pixels.0,
                    current.1 + pending.world_pixels.1
                ),
                native_to_world(requested),
                "{} requested {operation} frame{frame}",
                sequence.name,
            );
            let focus: CameraFocus = serde_json::from_value(expected["focus"].clone()).unwrap();
            assert_eq!(
                pending.focus, focus,
                "{} focus {operation} frame{frame}",
                sequence.name
            );
            assert_eq!(
                u64::from(pending.last_ai_frame),
                expected["focus"]["last_ai_frame"].as_u64().unwrap(),
                "{} cadence {operation} frame{frame}",
                sequence.name,
            );
            visits += 1;
        }
    }
    assert_eq!(visits, 1646);
}

#[test]
fn saved_camera_center_and_new_viewport_match_original_load_and_resize() {
    use super::{set_camera_position, tactical_camera_center, tactical_camera_top_left};
    let data = vectors();
    let cases = data["camera_save_load"].as_array().unwrap();
    assert_eq!(cases.len(), 13);
    let world_bias = crate::util::lepton::absolute_leptons_to_screen(0, 0, 0);
    let mut checked = 0;
    for row in cases {
        let source = &row["input"];
        // The map-editor no-clamp gate remains characterization: ordinary
        // production VERA has no editor-mode saved-game route.
        if source["map_editor"].as_u64().unwrap() != 0 {
            continue;
        }
        let name = source["name"].as_str().unwrap();
        let viewport: [u32; 2] = serde_json::from_value(source["viewport"].clone()).unwrap();
        let saved_center: [f32; 2] =
            serde_json::from_value(source["saved_center"].clone()).unwrap();
        let pending: [f32; 2] = serde_json::from_value(source["saved_requested"].clone()).unwrap();
        let local_size: [i32; 4] = serde_json::from_value(source["local_size"].clone()).unwrap();
        let map_rect_width = source["map_rect_width"].as_i64().unwrap() as i32;
        let clamp_in = |dimensions: [u32; 2]| {
            fixture_clamp(
                map_rect_width,
                local_size,
                [dimensions[0] as f32, dimensions[1] as f32],
            )
        };
        let center = (
            saved_center[0] + world_bias.0,
            saved_center[1] + world_bias.1,
        );
        let point = tactical_camera_top_left(center, viewport[0] as f32, viewport[1] as f32, 1.0);
        let mut input = crate::app::input::state::MatchInputState::new(Default::default());
        input.pending_camera_scroll.request(pending.into());
        set_camera_position(&mut input, clamp_in(viewport)(point));
        for (expected, dimensions) in [
            (&row["loaded"], viewport),
            (
                &row["resized"],
                serde_json::from_value(source["resize"].clone()).unwrap_or(viewport),
            ),
        ] {
            if expected.is_null() {
                continue;
            }
            if dimensions != viewport {
                let center = tactical_camera_center(
                    (input.camera_x, input.camera_y),
                    viewport[0] as f32,
                    viewport[1] as f32,
                    1.0,
                );
                let point = tactical_camera_top_left(
                    center,
                    dimensions[0] as f32,
                    dimensions[1] as f32,
                    1.0,
                );
                set_camera_position(&mut input, clamp_in(dimensions)(point));
            }
            let native_top_left: [f32; 2] =
                serde_json::from_value(expected["top_left"].clone()).unwrap();
            assert_eq!(
                (input.camera_x, input.camera_y),
                (
                    native_top_left[0] + world_bias.0,
                    native_top_left[1] + world_bias.1
                ),
                "{name}"
            );
            let native_center: [f32; 2] =
                serde_json::from_value(expected["committed"].clone()).unwrap();
            assert_eq!(
                tactical_camera_center(
                    (input.camera_x, input.camera_y),
                    dimensions[0] as f32,
                    dimensions[1] as f32,
                    1.0,
                ),
                (
                    native_center[0] + world_bias.0,
                    native_center[1] + world_bias.1
                ),
                "{name}"
            );
            assert_eq!(expected["committed"], expected["requested"], "{name}");
            assert!(input.pending_camera_scroll.is_idle(), "{name}");
            checked += 1;
        }
    }
    assert_eq!(checked, 13);
}
