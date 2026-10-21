//! Physical pointer histories through the production match-input owner.

use super::MatchInputState;
use crate::app::input::hotkeys::HotkeyBindings;
use crate::render::upscale_pass::{FrameShape, ZoomGeometry};

fn input() -> MatchInputState {
    MatchInputState::new(HotkeyBindings::default())
}

#[test]
fn reversing_motion_in_letterbox_bars_moves_the_tactical_cursor_immediately() {
    let cases = [
        // 1600x900's 4:3 frame starts at x=200 and ends at source x=1199.
        (1600, 900, 100, (100.0, 450.0), (104.0, 450.0), (4.0, 450.0)),
        (
            1600,
            900,
            100,
            (1500.0, 450.0),
            (1496.0, 450.0),
            (1195.0, 450.0),
        ),
        // A tall window exercises both horizontal letterbox bars.
        (600, 1000, 100, (300.0, 100.0), (300.0, 104.0), (300.0, 4.0)),
        (
            600,
            1000,
            100,
            (300.0, 900.0),
            (300.0, 896.0),
            (300.0, 445.0),
        ),
        // At 300%, six physical pixels must move two source pixels.
        (
            3840,
            2160,
            300,
            (3500.0, 1080.0),
            (3494.0, 1080.0),
            (957.0, 360.0),
        ),
    ];
    for (width, height, percent, edge, inward, expected) in cases {
        let zoom = ZoomGeometry::new(width, height, percent, FrameShape::Original);
        let mut input = input();
        input.set_cursor_from_window(edge, Some(zoom), true);
        input.set_cursor_from_window(inward, Some(zoom), true);
        assert_eq!(
            (input.cursor_x, input.cursor_y),
            expected,
            "physical history {edge:?} -> {inward:?}, zoom {percent}%"
        );
    }
}

#[test]
fn ordinary_motion_and_frontend_coordinates_keep_their_window_mapping() {
    let zoom = ZoomGeometry::new(1600, 1200, 200, FrameShape::Window);
    let mut input = input();
    input.set_cursor_from_window((400.0, 300.0), Some(zoom), true);
    assert_eq!((input.cursor_x, input.cursor_y), (200.0, 150.0));
    input.set_cursor_from_window((420.0, 312.0), Some(zoom), true);
    assert_eq!((input.cursor_x, input.cursor_y), (210.0, 156.0));
    input.sync_cursor_space(None);
    assert_eq!((input.cursor_x, input.cursor_y), (420.0, 312.0));
    input.set_cursor_from_window((100.0, 90.0), None, true);
    assert_eq!((input.cursor_x, input.cursor_y), (100.0, 90.0));
}

#[test]
fn a_released_cursor_keeps_absolute_mapping_in_letterbox_bars() {
    let zoom = ZoomGeometry::new(1600, 900, 100, FrameShape::Original);
    let mut input = input();
    input.set_cursor_from_window((100.0, 450.0), Some(zoom), false);
    input.set_cursor_from_window((104.0, 450.0), Some(zoom), false);
    assert_eq!((input.cursor_x, input.cursor_y), (0.0, 450.0));
    input.set_cursor_from_window((210.0, 450.0), Some(zoom), false);
    assert_eq!((input.cursor_x, input.cursor_y), (10.0, 450.0));
}

#[test]
fn focus_or_client_reentry_starts_with_an_absolute_position() {
    let zoom = ZoomGeometry::new(1600, 900, 100, FrameShape::Original);
    let mut input = input();
    input.set_cursor_from_window((100.0, 450.0), Some(zoom), true);
    input.set_cursor_from_window((104.0, 450.0), Some(zoom), true);
    input.reset_window_cursor_motion();
    input.set_cursor_from_window((500.0, 400.0), Some(zoom), true);
    assert_eq!((input.cursor_x, input.cursor_y), (300.0, 400.0));
}

#[test]
fn resizing_preserves_motion_and_leaving_a_match_restores_the_os_position() {
    let mut input = input();
    let before = ZoomGeometry::new(1600, 900, 100, FrameShape::Original);
    input.set_cursor_from_window((1500.0, 450.0), Some(before), true);
    input.set_cursor_from_window((1496.0, 450.0), Some(before), true);
    assert_eq!(input.cursor_x, 1195.0);

    let after = ZoomGeometry::new(1920, 1080, 100, FrameShape::Original);
    input.sync_cursor_space(Some(after));
    assert_eq!(input.cursor_x, 1155.0);
    input.set_cursor_from_window((1495.0, 450.0), Some(after), true);
    assert_eq!(input.cursor_x, 1154.0);
    input.sync_cursor_space(None);
    assert_eq!((input.cursor_x, input.cursor_y), (1495.0, 450.0));
}

#[test]
fn a_fresh_match_does_not_reuse_the_frontend_motion_anchor() {
    let mut input = input();
    input.set_cursor_from_window((300.0, 300.0), None, false);
    // Tactical install places the cursor in source space before publishing it.
    input.cursor_x = 100.0;
    input.cursor_y = 100.0;
    let zoom = ZoomGeometry::new(1600, 1200, 200, FrameShape::Window);
    input.sync_cursor_space(Some(zoom));
    assert_eq!((input.cursor_x, input.cursor_y), (100.0, 100.0));
    input.set_cursor_from_window((400.0, 400.0), Some(zoom), true);
    assert_eq!((input.cursor_x, input.cursor_y), (200.0, 200.0));
}

#[test]
fn capture_only_source_input_keeps_the_rendered_transition_fallback() {
    let mut input = input();
    input.cursor_zoom = Some(ZoomGeometry::new(1600, 1200, 200, FrameShape::Window));
    input.cursor_x = 200.0;
    input.cursor_y = 150.0;
    input.sync_cursor_space(None);
    assert_eq!((input.cursor_x, input.cursor_y), (400.0, 300.0));
}
