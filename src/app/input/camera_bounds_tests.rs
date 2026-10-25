//! VERA viewport adaptation, distinct from the native small-map clamp.
use super::*;

#[test]
fn saved_zoom_respects_the_wheel_preference_without_moving_the_center_or_focus() {
    let viewport = (641, 481);
    let mut pending = PendingCameraScroll::default();
    assert!(pending.focus_on((100., 200.), [8192, 4096, 0], 2, (640., 480.), 0.8));
    pending.last_ai_frame = 42;
    let saved = pending.saved_view((2100., 1600.), ViewDistanceTier::Large.zoom());
    validate_saved_tactical_view(&saved).unwrap();

    for (enabled, expected_zoom) in [(false, 1.0), (true, saved.zoom)] {
        let mut input = crate::app::input::state::MatchInputState::new(Default::default());
        input.pending_camera_scroll.request((99., -80.));
        restore_camera_input(&mut input, &saved, viewport, enabled);
        assert_eq!(input.zoom_level, expected_zoom);
        assert_eq!(input.zoom_target, expected_zoom);
        let center = tactical_camera_center(
            (input.camera_x, input.camera_y),
            viewport.0 as f32,
            viewport.1 as f32,
            input.zoom_level,
        );
        assert_eq!(center, (2100., 1600.));
        assert_eq!(input.pending_camera_scroll.world_pixels, (0., 0.));
        assert_eq!(
            input
                .pending_camera_scroll
                .saved_view(center, input.zoom_level),
            crate::sim::snapshot::SavedTacticalView {
                zoom: expected_zoom,
                ..saved.clone()
            },
            "focus and last-AI-frame survive the player zoom override"
        );
    }
}

#[test]
fn oversized_view_is_centered_and_repeated_clamps_are_stable() {
    // Shifted world rectangle; the horizontal camera margin is still 30 px.
    let area = (130., 200., 1800., 1305.);
    for (viewport, zoom) in [
        ((2392., 1568.), 1.), // 2560x1600, normal sidebar and command bar
        ((2392., 1408.), 1.), // 2560x1440
        ((1196., 784.), 0.5),
        ((4784., 3136.), 2.),
    ] {
        for seed in [(-10000., -10000.), (0., 0.), (10000., 10000.)] {
            let mut point = clamp_camera_point_to_rect(seed, area, viewport, zoom);
            let center = tactical_camera_center(point, viewport.0, viewport.1, zoom);
            assert_eq!(center, (1000., 852.5));
            for _ in 0..60 {
                let next = clamp_camera_point_to_rect(point, area, viewport, zoom);
                assert_eq!(next, point, "idle frame must not move the camera");
                point = next;
            }
        }
    }
}

#[test]
fn only_the_oversized_axis_is_fixed_and_fitting_axes_still_scroll() {
    let area = (130., 200., 1800., 1305.);
    for (viewport, low, high) in [
        ((2392., 1000.), (-196., 200.), (-196., 505.)),
        ((1200., 1568.), (100., 68.5), (700., 68.5)),
        ((1800., 1305.), (100., 200.), (100., 200.)),
        ((1200., 1000.), (100., 200.), (700., 505.)),
    ] {
        assert_eq!(
            clamp_camera_point_to_rect((-10000., -10000.), area, viewport, 1.),
            low
        );
        assert_eq!(
            clamp_camera_point_to_rect((10000., 10000.), area, viewport, 1.),
            high
        );
        assert_eq!(clamp_camera_point_to_rect(low, area, viewport, 1.), low);
        assert_eq!(clamp_camera_point_to_rect(high, area, viewport, 1.), high);
    }
    assert_eq!(
        clamp_camera_point_to_rect((400., 350.), area, (1200., 1000.), 1.),
        (400., 350.)
    );
}

#[test]
fn oversized_local_bounds_keep_scroll_and_cinematic_focus_stable() {
    let bounds = crate::map::playfield::PlayfieldBounds::from_normalized_local_size(10, 0, 0, 4, 4);
    let viewport = (640., 480.);
    let clamp = |point| clamp_camera_point_to_local_bounds(point, bounds, viewport, 1.);
    let mut current = clamp((0., 0.));
    let centered = current;
    let mut pending = PendingCameraScroll::default();
    for direction in [ScrollDir::N, ScrollDir::E, ScrollDir::S, ScrollDir::W] {
        let delta = OCTANT_DELTA[scroll_dir_octant(direction)];
        pending.request((delta.0 * 21., delta.1 * 21.));
        current = pending.commit(current, (0., 0.), clamp);
        assert_eq!(current, centered);
        assert!(!requested_scroll_survives_clamp(
            centered, current, direction
        ));
        assert!(pending.is_idle());
    }
    assert!(pending.focus_on(current, [25600, 25600, 0], 4, viewport, 1.));
    for frame in 0..30 {
        current = pending.advance_ai(
            current,
            (0., 0.),
            CameraAiFrame {
                frame,
                scenario_started: true,
                replay_blocked: false,
            },
            viewport,
            1.,
            clamp,
        );
        assert_eq!(current, centered, "focus frame {frame}");
        assert_eq!(clamp(current), current, "render frame {frame}");
    }
    assert!(
        pending.is_idle(),
        "focus still completes while the camera is fixed"
    );
}
