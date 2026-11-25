//! The boxes of the pieces drawn over the battlefield, in the frame.
//!
//! `ui::field_layout` owns the boxes' rules; this places them on the live
//! battlefield for the message list, the timers, the banner and Phobos'
//! superweapon column.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::app::presentation::state::MatchPresentationState;
use crate::ui::field_layout::{FieldLayout, FieldPart};

/// The local side's places for the pieces over the battlefield.
pub(crate) fn layout(presentation: &MatchPresentationState) -> FieldLayout {
    presentation.sidebar_layout_spec.field
}

/// Whether the side's layout has this piece.
pub(crate) fn shows(presentation: &MatchPresentationState, part: FieldPart) -> bool {
    layout(presentation).shows(part)
}

/// A piece's box in frame pixels (`x, y, width, height`): the whole
/// battlefield in the original interface.
pub(crate) fn rect(state: &AppState, part: FieldPart) -> [i32; 4] {
    let (x, y, width, height) = crate::app::input::camera::tactical_viewport_px(
        &state.platform,
        &state.renderer,
        &state.frontend.screen,
        &state.match_state.match_presentation,
        &state.persistence,
    );
    layout(&state.match_state.match_presentation)
        .part_rect(part, [x as i32, y as i32, width as i32, height as i32])
}

/// What an overlay laid out in a piece's own pixels (its box's corner at
/// 0, 0) adds to them for a screen-space lane: the world camera the UI
/// shader subtracts, plus the box's corner in the frame.
pub(crate) fn lane_camera(state: &AppState, part: FieldPart) -> [f32; 2] {
    let [x, y, _, _] = rect(state, part);
    [
        state.match_state.input.camera_x + x as f32,
        state.match_state.input.camera_y + y as f32,
    ]
}
