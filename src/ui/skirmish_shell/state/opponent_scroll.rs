//! VERA's scrollbar beside the computer rows of a map with more starts than
//! the page shows rows ([`shown_player_rows`]), up to thirty. The player's own
//! row stays first; the rows under it show the map's computer rows from
//! [`SkirmishShellState::first_opponent`]. It is the shared list scrollbar
//! (`0x0061C690`), a row a step: the page's gui List over the computer rows
//! (`page::OPPONENT_SCROLL`) owns its capture, arrow repeat and the wheel,
//! a row a notch anywhere on the page (no retail control on it takes the
//! wheel).

use crate::map::scenario_menu::MapMenuEntry;
use vera20k_gui::shell::list::{ListMetrics, ShellListGeometry};

use super::super::layout::{RectPx, SkirmishShellLayout, shown_player_rows};
use super::SkirmishShellState;
use super::player_name::selected_player_rows;

/// The computer rows the scrollbar ranges over and how many of them show;
/// none while every row shows.
pub fn opponent_scroll_rows(
    state: &SkirmishShellState,
    maps: &[MapMenuEntry],
) -> Option<(usize, usize)> {
    let rows = selected_player_rows(state, maps);
    let shown = shown_player_rows(rows);
    (rows > shown).then(|| (rows - 1, shown - 1))
}

/// The scrollbar on a map with more than eight starts: the shared list over
/// the map's computer rows, all but the player's shown rows showing.
pub fn opponent_scroll_geometry(
    layout: &SkirmishShellLayout,
    state: &SkirmishShellState,
    maps: &[MapMenuEntry],
) -> Option<ShellListGeometry> {
    let (rows, shown) = opponent_scroll_rows(state, maps)?;
    let metrics = ListMetrics::scaled(layout.scale);
    let rect = layout.opponent_scrollbar;
    let outer = RectPx::new(rect.x, rect.y, metrics.scrollbar_w + 2, rect.h);
    Some(ShellListGeometry::with_visible_rows(
        outer,
        rows,
        shown,
        state.first_opponent,
        metrics,
    ))
}
