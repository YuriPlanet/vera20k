//! VERA's scrollbar beside the computer rows of a map with more starts than
//! the page shows rows ([`shown_player_rows`]), up to thirty. The player's own
//! row stays first; the rows under it show the map's computer rows from
//! [`SkirmishShellState::first_opponent`]. It is the shared list scrollbar
//! (`0x0061C690`) with its capture and arrow repeat, a row a step. The wheel
//! moves it too, a row a notch: no retail control on the page takes the wheel.

use crate::map::scenario_menu::MapMenuEntry;
use crate::ui::shell::list::{ListMetrics, ShellListGeometry};
use crate::util::time::Instant;

use super::super::layout::{RectPx, SkirmishShellLayout, shown_player_rows};
use super::SkirmishShellState;
use super::player_name::{last_first_opponent, selected_player_rows};

/// The scrollbar on a map with more than eight starts: the shared list over
/// the map's computer rows, all but the player's shown rows showing.
pub fn opponent_scroll_geometry(
    layout: &SkirmishShellLayout,
    state: &SkirmishShellState,
    maps: &[MapMenuEntry],
) -> Option<ShellListGeometry> {
    let rows = selected_player_rows(state, maps);
    let shown = shown_player_rows(rows);
    if rows <= shown {
        return None;
    }
    let metrics = ListMetrics::scaled(layout.scale);
    let rect = layout.opponent_scrollbar;
    let outer = RectPx::new(rect.x, rect.y, metrics.scrollbar_w + 2, rect.h);
    Some(ShellListGeometry::with_visible_rows(
        outer,
        rows - 1,
        shown - 1,
        state.first_opponent,
        metrics,
    ))
}

/// A press on the scrollbar: an arrow steps a row, the track jumps there and
/// the thumb captures for a drag. Returns whether the press was on it.
pub(super) fn press_opponent_scrollbar(
    state: &mut SkirmishShellState,
    layout: &SkirmishShellLayout,
    maps: &[MapMenuEntry],
    x: i32,
    y: i32,
    now: Instant,
) -> bool {
    let Some(geometry) = opponent_scroll_geometry(layout, state, maps) else {
        return false;
    };
    let Some(part) = geometry.scroll_part_at(x, y) else {
        return false;
    };
    state
        .opponent_scroll
        .press(part, geometry, &mut state.first_opponent, y, now);
    true
}

/// Pointer motion while the scrollbar holds the mouse: a held thumb follows
/// the pointer, a held arrow shows pressed only under it.
pub(super) fn move_opponent_scrollbar(
    state: &mut SkirmishShellState,
    layout: &SkirmishShellLayout,
    maps: &[MapMenuEntry],
    x: i32,
    y: i32,
) {
    if let Some(geometry) = opponent_scroll_geometry(layout, state, maps) {
        state
            .opponent_scroll
            .pointer_moved(geometry, &mut state.first_opponent, x, y);
    }
}

/// A held arrow's repeat with the pointer at `x`, `y`. Returns whether the
/// rows or the arrow's look changed.
pub fn poll_opponent_scroll(
    state: &mut SkirmishShellState,
    layout: &SkirmishShellLayout,
    maps: &[MapMenuEntry],
    x: i32,
    y: i32,
    now: Instant,
) -> bool {
    if !state.opponent_scroll.captured() {
        return false;
    }
    let Some(geometry) = opponent_scroll_geometry(layout, state, maps) else {
        return false;
    };
    state
        .opponent_scroll
        .poll(geometry, &mut state.first_opponent, x, y, now)
}

/// When a held arrow next repeats.
pub fn opponent_scroll_deadline(state: &SkirmishShellState) -> Option<Instant> {
    state.opponent_scroll.repeat_at()
}

/// Whether a press on the scrollbar holds the mouse until its release.
pub fn opponent_scroll_held(state: &SkirmishShellState) -> bool {
    state.opponent_scroll.captured()
}

/// The wheel: a notch moves the rows one row, wheel up toward the first.
/// Not while a combo's list is open, which takes no wheel. Returns whether
/// the rows moved.
pub(super) fn wheel_opponents(
    state: &mut SkirmishShellState,
    maps: &[MapMenuEntry],
    lines: f32,
) -> bool {
    if state.open_combo_dropdown.is_some() || state.opponent_scroll.captured() {
        return false;
    }
    let last = last_first_opponent(selected_player_rows(state, maps)) as i64;
    let top = (state.first_opponent as i64 - lines as i64).clamp(0, last) as usize;
    if top == state.first_opponent {
        return false;
    }
    state.first_opponent = top;
    true
}
