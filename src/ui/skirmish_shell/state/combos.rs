//! Combo-box item, dropdown, scrollbar, and selection helpers for the skirmish shell.

use crate::map::scenario_menu::MapMenuEntry;
use crate::skirmish_launch::{HOUSE_COLOR_COUNT, SKIRMISH_PLAYER_SLOT_COUNT};
use crate::ui::main_menu::{SkirmishCountry, StartPosition};

use super::super::layout::{
    COMBO_ARROW_RESERVE_W, COMBO_DROPDOWN_ROW_H, COMBO_DROPDOWN_SCROLLBAR_BUTTON_H,
    COMBO_DROPDOWN_SCROLLBAR_W, COMBO_FACE_H, RectPx, SkirmishShellLayout, combo_face_rect,
};
use super::super::scroll::ScrollModel;
use super::{
    DropdownScrollDragState, DropdownScrollbarPart, DropdownScrollbarPressState, OpenComboDropdown,
    SkirmishAiRowType, SkirmishComboId, SkirmishComboItem, SkirmishCountryChoice,
    SkirmishShellOpponent, SkirmishShellState, SkirmishShellUiSound, inactive_ai_team_default,
    player_row_visible,
};

pub fn combo_rect(layout: &SkirmishShellLayout, id: SkirmishComboId) -> Option<RectPx> {
    match id {
        SkirmishComboId::AiType(idx) => layout.rows.ai_type_combos.get(idx).copied(),
        SkirmishComboId::Side(row) => layout.rows.side_combos.get(row).copied(),
        SkirmishComboId::Color(row) => layout.color_combos.get(row).copied(),
        SkirmishComboId::Start(row) => layout.rows.start_combos.get(row).copied(),
        SkirmishComboId::Team(row) => layout.rows.team_combos.get(row).copied(),
    }
}

pub fn combo_dropdown_rect(
    state: &SkirmishShellState,
    layout: &SkirmishShellLayout,
    maps: &[MapMenuEntry],
    id: SkirmishComboId,
) -> Option<RectPx> {
    if !combo_enabled(state, maps, id) {
        return None;
    }
    let rect = combo_rect(layout, id)?;
    let item_count = combo_items(state, maps, id).len() as i32;
    if item_count == 0 {
        return None;
    }
    let max_rows = combo_dropdown_max_visible_rows(id);
    let visible_rows = if max_rows > 0 {
        item_count.min(max_rows)
    } else {
        item_count
    };
    Some(RectPx::new(
        rect.x,
        rect.y + COMBO_FACE_H + 1,
        rect.w,
        visible_rows * COMBO_DROPDOWN_ROW_H,
    ))
}

pub const fn combo_dropdown_max_visible_rows(id: SkirmishComboId) -> i32 {
    match id {
        SkirmishComboId::Side(_) => 7,
        SkirmishComboId::Color(_) | SkirmishComboId::Start(_) => 9,
        SkirmishComboId::AiType(_) | SkirmishComboId::Team(_) => 0,
    }
}

pub fn combo_dropdown_visible_row_count(
    state: &SkirmishShellState,
    maps: &[MapMenuEntry],
    id: SkirmishComboId,
) -> usize {
    let item_count = combo_items(state, maps, id).len();
    ScrollModel::combo(combo_dropdown_max_visible_rows(id)).visible_rows(item_count)
}

fn combo_dropdown_item_count(
    state: &SkirmishShellState,
    maps: &[MapMenuEntry],
    id: SkirmishComboId,
) -> usize {
    combo_items(state, maps, id).len()
}

pub fn combo_dropdown_needs_scrollbar(
    state: &SkirmishShellState,
    maps: &[MapMenuEntry],
    id: SkirmishComboId,
) -> bool {
    combo_dropdown_item_count(state, maps, id) > combo_dropdown_visible_row_count(state, maps, id)
}

pub(super) fn combo_dropdown_max_top_index(
    state: &SkirmishShellState,
    maps: &[MapMenuEntry],
    id: SkirmishComboId,
) -> usize {
    let item_count = combo_dropdown_item_count(state, maps, id);
    let visible_rows = combo_dropdown_visible_row_count(state, maps, id);
    ScrollModel::combo(combo_dropdown_max_visible_rows(id)).max_top_index(item_count, visible_rows)
}

fn normal_color_index(color_index: usize) -> usize {
    color_index.min(HOUSE_COLOR_COUNT - 1)
}

pub fn combo_dropdown_scrollbar_rect(
    state: &SkirmishShellState,
    layout: &SkirmishShellLayout,
    maps: &[MapMenuEntry],
    id: SkirmishComboId,
) -> Option<RectPx> {
    if !combo_dropdown_needs_scrollbar(state, maps, id) {
        return None;
    }
    let dropdown = combo_dropdown_rect(state, layout, maps, id)?;
    Some(RectPx::new(
        dropdown.x + dropdown.w - COMBO_DROPDOWN_SCROLLBAR_W,
        dropdown.y,
        COMBO_DROPDOWN_SCROLLBAR_W,
        dropdown.h,
    ))
}

pub fn combo_dropdown_content_rect(
    state: &SkirmishShellState,
    layout: &SkirmishShellLayout,
    maps: &[MapMenuEntry],
    id: SkirmishComboId,
) -> Option<RectPx> {
    let dropdown = combo_dropdown_rect(state, layout, maps, id)?;
    let width = if combo_dropdown_needs_scrollbar(state, maps, id) {
        dropdown.w - COMBO_DROPDOWN_SCROLLBAR_W
    } else {
        dropdown.w
    };
    Some(RectPx::new(
        dropdown.x,
        dropdown.y,
        width.max(0),
        dropdown.h,
    ))
}

pub fn combo_dropdown_scroll_thumb_rect(
    state: &SkirmishShellState,
    layout: &SkirmishShellLayout,
    maps: &[MapMenuEntry],
    id: SkirmishComboId,
) -> Option<RectPx> {
    let scrollbar = combo_dropdown_scrollbar_rect(state, layout, maps, id)?;
    let model = ScrollModel::combo(combo_dropdown_max_visible_rows(id));
    let visible_rows = combo_dropdown_visible_row_count(state, maps, id);
    let item_count = combo_dropdown_item_count(state, maps, id);
    let thumb_h = model.thumb_height(visible_rows, item_count, scrollbar.h);
    let max_top = combo_dropdown_max_top_index(state, maps, id);
    let open_top = state
        .open_combo_dropdown
        .filter(|open| open.id == id)
        .map(|open| open.top_index.min(max_top))
        .unwrap_or(0);
    let thumb_y = model.thumb_y(scrollbar, thumb_h, open_top, max_top);
    Some(RectPx::new(scrollbar.x, thumb_y, scrollbar.w, thumb_h))
}

pub(super) fn set_open_combo_top_index(
    state: &mut SkirmishShellState,
    maps: &[MapMenuEntry],
    id: SkirmishComboId,
    top_index: usize,
) {
    let max_top = combo_dropdown_max_top_index(state, maps, id);
    if let Some(open) = state
        .open_combo_dropdown
        .as_mut()
        .filter(|open| open.id == id)
    {
        open.top_index = top_index.min(max_top);
    }
}

pub(super) fn scroll_open_combo_by_rows(
    state: &mut SkirmishShellState,
    maps: &[MapMenuEntry],
    id: SkirmishComboId,
    rows: i32,
) -> bool {
    let Some(open) = state.open_combo_dropdown.filter(|open| open.id == id) else {
        return false;
    };
    let max_top = combo_dropdown_max_top_index(state, maps, id);
    let next = if rows < 0 {
        open.top_index.saturating_sub((-rows) as usize)
    } else {
        (open.top_index + rows as usize).min(max_top)
    };
    set_open_combo_top_index(state, maps, id, next);
    next != open.top_index
}

pub(super) fn top_index_from_thumb_y(
    state: &SkirmishShellState,
    layout: &SkirmishShellLayout,
    maps: &[MapMenuEntry],
    id: SkirmishComboId,
    mouse_y: i32,
    grab_offset_y: i32,
) -> Option<usize> {
    let scrollbar = combo_dropdown_scrollbar_rect(state, layout, maps, id)?;
    let thumb = combo_dropdown_scroll_thumb_rect(state, layout, maps, id)?;
    let max_top = combo_dropdown_max_top_index(state, maps, id);
    let model = ScrollModel::combo(combo_dropdown_max_visible_rows(id));
    Some(model.top_index_from_thumb_top(scrollbar, thumb.h, max_top, mouse_y - grab_offset_y))
}

pub(super) fn top_index_from_scrollbar_track_click(
    state: &SkirmishShellState,
    layout: &SkirmishShellLayout,
    maps: &[MapMenuEntry],
    id: SkirmishComboId,
    mouse_y: i32,
) -> Option<usize> {
    let scrollbar = combo_dropdown_scrollbar_rect(state, layout, maps, id)?;
    let thumb = combo_dropdown_scroll_thumb_rect(state, layout, maps, id)?;
    let max_top = combo_dropdown_max_top_index(state, maps, id);
    let model = ScrollModel::combo(combo_dropdown_max_visible_rows(id));
    Some(model.top_index_from_thumb_top(scrollbar, thumb.h, max_top, mouse_y - thumb.h / 2))
}

pub fn combo_items(
    state: &SkirmishShellState,
    maps: &[MapMenuEntry],
    id: SkirmishComboId,
) -> Vec<SkirmishComboItem> {
    match id {
        SkirmishComboId::AiType(_) => [
            SkirmishAiRowType::None,
            SkirmishAiRowType::Easy,
            SkirmishAiRowType::Normal,
            SkirmishAiRowType::Hard,
        ]
        .into_iter()
        .map(SkirmishComboItem::AiType)
        .collect(),
        SkirmishComboId::Side(_) => std::iter::once(SkirmishCountryChoice::Random)
            .chain(
                SkirmishCountry::ALL
                    .into_iter()
                    .map(SkirmishCountryChoice::Country),
            )
            .map(SkirmishComboItem::Country)
            .collect(),
        SkirmishComboId::Color(row) => {
            let selected = selected_color_claim(state, row);
            let mut items = vec![SkirmishComboItem::ColorSentinel(-2)];
            for color in 0..HOUSE_COLOR_COUNT {
                if selected == Some(color)
                    || color_claimed_by_other_row(state, row, color).is_none()
                {
                    items.push(SkirmishComboItem::Color(color));
                }
            }
            items
        }
        SkirmishComboId::Start(row) => {
            let capacity = maps
                .get(state.selected_map_idx)
                .map(|map| {
                    map.player_capacity
                        .clamp(0, SKIRMISH_PLAYER_SLOT_COUNT as i32) as usize
                })
                .unwrap_or(SKIRMISH_PLAYER_SLOT_COUNT)
                .min(SKIRMISH_PLAYER_SLOT_COUNT);
            let selected = selected_start_position(state, row);
            let mut items = vec![SkirmishComboItem::Start(StartPosition::Auto)];
            for position in 0..capacity {
                let start = StartPosition::Position(position as u8);
                if selected == Some(start) || !start_position_taken_by_other_row(state, row, start)
                {
                    items.push(SkirmishComboItem::Start(start));
                }
            }
            items
        }
        SkirmishComboId::Team(_) => {
            let values: &[i32] = if state.selected_mode_must_ally {
                &[0, 1, 2, 3]
            } else {
                &[-2, 0, 1, 2, 3]
            };
            values
                .iter()
                .copied()
                .map(SkirmishComboItem::Team)
                .collect()
        }
    }
}

pub fn selected_combo_item(
    state: &SkirmishShellState,
    id: SkirmishComboId,
) -> Option<SkirmishComboItem> {
    match id {
        SkirmishComboId::AiType(idx) => state
            .opponents
            .get(idx)
            .map(|opponent| SkirmishComboItem::AiType(opponent.row_type)),
        SkirmishComboId::Side(0) => {
            Some(SkirmishComboItem::Country(if state.player_country_random {
                SkirmishCountryChoice::Random
            } else {
                SkirmishCountryChoice::Country(state.player_country)
            }))
        }
        SkirmishComboId::Side(row) => state.opponents.get(row - 1).map(|opponent| {
            SkirmishComboItem::Country(if opponent.country_random {
                SkirmishCountryChoice::Random
            } else {
                SkirmishCountryChoice::Country(opponent.country)
            })
        }),
        SkirmishComboId::Color(0) => Some(if state.player_color_claimed {
            SkirmishComboItem::Color(normal_color_index(state.player_color_index))
        } else {
            SkirmishComboItem::ColorSentinel(-2)
        }),
        SkirmishComboId::Color(row) => state.opponents.get(row - 1).map(|opponent| {
            if opponent.color_claimed {
                SkirmishComboItem::Color(normal_color_index(opponent.color_index))
            } else {
                SkirmishComboItem::ColorSentinel(-2)
            }
        }),
        SkirmishComboId::Start(row) => {
            selected_start_position(state, row).map(SkirmishComboItem::Start)
        }
        SkirmishComboId::Team(0) => Some(SkirmishComboItem::Team(state.player_team)),
        SkirmishComboId::Team(row) => state
            .opponents
            .get(row - 1)
            .map(|opponent| SkirmishComboItem::Team(opponent.team)),
    }
}

pub fn selected_combo_item_index(
    state: &SkirmishShellState,
    maps: &[MapMenuEntry],
    id: SkirmishComboId,
) -> Option<usize> {
    let selected = selected_combo_item(state, id)?;
    combo_items(state, maps, id)
        .iter()
        .position(|item| *item == selected)
}

fn combo_player_row(id: SkirmishComboId) -> usize {
    match id {
        SkirmishComboId::AiType(idx) => idx + 1,
        SkirmishComboId::Side(row)
        | SkirmishComboId::Color(row)
        | SkirmishComboId::Start(row)
        | SkirmishComboId::Team(row) => row,
    }
}

pub fn combo_enabled(
    state: &SkirmishShellState,
    maps: &[MapMenuEntry],
    id: SkirmishComboId,
) -> bool {
    if !player_row_visible(state, maps, combo_player_row(id)) {
        return false;
    }
    match id {
        SkirmishComboId::AiType(_) => true,
        SkirmishComboId::Side(0) | SkirmishComboId::Color(0) | SkirmishComboId::Start(0) => true,
        // The whole Team column follows the selected mode's `AlliesAllowed`,
        // the local row included: the native refresh pass reads that byte once
        // and hands it to every one of the eight team controls. With allying
        // forbidden (stock Free For All and Cooperative) the column is dead, so
        // the player cannot put themselves or an AI on a team the mode
        // disallows.
        SkirmishComboId::Team(0) => state.selected_mode_allies_allowed,
        SkirmishComboId::Side(row) | SkirmishComboId::Color(row) | SkirmishComboId::Start(row) => {
            state
                .opponents
                .get(row.saturating_sub(1))
                .is_some_and(SkirmishShellOpponent::is_active)
        }
        // An opponent's team control needs both: the native pass reads the row's
        // AI-type item data and greys the team combo when the row is closed,
        // *and* ANDs in the same mode-wide `AlliesAllowed` byte.
        SkirmishComboId::Team(row) => {
            state.selected_mode_allies_allowed
                && state
                    .opponents
                    .get(row.saturating_sub(1))
                    .is_some_and(SkirmishShellOpponent::is_active)
        }
    }
}

fn selected_start_position(state: &SkirmishShellState, row: usize) -> Option<StartPosition> {
    if row == 0 {
        Some(state.player_start_position)
    } else {
        state
            .opponents
            .get(row - 1)
            .map(|opponent| opponent.start_position)
    }
}

fn start_position_taken_by_other_row(
    state: &SkirmishShellState,
    row: usize,
    start: StartPosition,
) -> bool {
    if state.player_start_position == start && row != 0 {
        return true;
    }
    state
        .opponents
        .iter()
        .enumerate()
        .any(|(idx, opponent)| idx + 1 != row && opponent.start_position == start)
}

fn selected_color_claim(state: &SkirmishShellState, row: usize) -> Option<usize> {
    if row == 0 {
        if state.player_color_claimed {
            Some(normal_color_index(state.player_color_index))
        } else {
            None
        }
    } else {
        state.opponents.get(row - 1).and_then(|opponent| {
            if opponent.color_claimed {
                Some(normal_color_index(opponent.color_index))
            } else {
                None
            }
        })
    }
}

fn color_claimed_by_other_row(
    state: &SkirmishShellState,
    row: usize,
    color: usize,
) -> Option<usize> {
    if row != 0
        && state.player_color_claimed
        && normal_color_index(state.player_color_index) == color
    {
        return Some(0);
    }
    state
        .opponents
        .iter()
        .enumerate()
        .find_map(|(idx, opponent)| {
            let opponent_row = idx + 1;
            if opponent_row != row
                && opponent.color_claimed
                && normal_color_index(opponent.color_index) == color
            {
                Some(opponent_row)
            } else {
                None
            }
        })
}

/// Release any row other than `row` that currently claims `color`. Called
/// before writing `row`'s new claim so the derived ownership model never
/// shows two rows holding the same color simultaneously. The evicted row
/// keeps its cached `color_index` so the player can see what they had if
/// they later re-pick from the now-shorter dropdown.
fn evict_other_color_claimants(state: &mut SkirmishShellState, row: usize, color: usize) {
    if row != 0
        && state.player_color_claimed
        && normal_color_index(state.player_color_index) == color
    {
        state.player_color_claimed = false;
    }
    for (idx, opponent) in state.opponents.iter_mut().enumerate() {
        let opponent_row = idx + 1;
        if opponent_row != row
            && opponent.color_claimed
            && normal_color_index(opponent.color_index) == color
        {
            opponent.color_claimed = false;
        }
    }
}

fn arrow_hit_rect(rect: RectPx) -> RectPx {
    let face = combo_face_rect(rect);
    RectPx::new(
        face.x + face.w - COMBO_ARROW_RESERVE_W,
        face.y,
        COMBO_ARROW_RESERVE_W,
        face.h,
    )
}

fn combo_hit_order() -> Vec<SkirmishComboId> {
    let mut ids = Vec::new();
    for row in (0..SKIRMISH_PLAYER_SLOT_COUNT).rev() {
        ids.push(SkirmishComboId::Team(row));
        ids.push(SkirmishComboId::Start(row));
        ids.push(SkirmishComboId::Color(row));
        ids.push(SkirmishComboId::Side(row));
        if row > 0 {
            ids.push(SkirmishComboId::AiType(row - 1));
        }
    }
    ids
}

fn combo_arrow_at(
    state: &SkirmishShellState,
    layout: &SkirmishShellLayout,
    maps: &[MapMenuEntry],
    x: i32,
    y: i32,
) -> Option<SkirmishComboId> {
    combo_hit_order().into_iter().find(|id| {
        combo_enabled(state, maps, *id)
            && combo_rect(layout, *id)
                .map(|rect| arrow_hit_rect(rect).contains(x, y))
                .unwrap_or(false)
    })
}

/// A click anywhere on the collapsed combo face — not just the arrow zone.
/// The owner-draw combo plays the open sound on any face click; only a click
/// inside the rightmost arrow zone actually toggles the dropdown open. Splitting
/// these two reachable areas lets us play the sound on a text-portion click
/// while still gating the open on the arrow zone.
fn combo_face_at(
    state: &SkirmishShellState,
    layout: &SkirmishShellLayout,
    maps: &[MapMenuEntry],
    x: i32,
    y: i32,
) -> Option<SkirmishComboId> {
    combo_hit_order().into_iter().find(|id| {
        combo_enabled(state, maps, *id)
            && combo_rect(layout, *id)
                .map(|rect| combo_face_rect(rect).contains(x, y))
                .unwrap_or(false)
    })
}

pub(super) fn apply_combo_selection(
    state: &mut SkirmishShellState,
    id: SkirmishComboId,
    item: SkirmishComboItem,
) {
    match (id, item) {
        (SkirmishComboId::AiType(idx), SkirmishComboItem::AiType(row_type)) => {
            let team_default = inactive_ai_team_default(state);
            if let Some(opponent) = state.opponents.get_mut(idx) {
                opponent.row_type = row_type;
                opponent.enabled = row_type.is_active();
                opponent.team = team_default;
                if let Some(difficulty) = row_type.difficulty() {
                    opponent.difficulty = difficulty;
                }
                // Release color claim on deactivate; activation does not auto-claim
                // even if the row was previously holding a color — another slot may
                // have grabbed it during the deactivation gap.
                opponent.color_claimed = row_type.is_active() && opponent.color_claimed;
                if !row_type.is_active() {
                    opponent.apply_inactive_combo_defaults(team_default);
                }
            }
        }
        (SkirmishComboId::Side(0), SkirmishComboItem::Country(choice)) => match choice {
            SkirmishCountryChoice::Random => {
                state.player_country_random = true;
            }
            SkirmishCountryChoice::Country(country) => {
                state.player_country_random = false;
                state.player_country = country;
            }
        },
        (SkirmishComboId::Side(row), SkirmishComboItem::Country(choice)) => {
            if let Some(opponent) = state.opponents.get_mut(row - 1) {
                match choice {
                    SkirmishCountryChoice::Random => {
                        opponent.country_random = true;
                    }
                    SkirmishCountryChoice::Country(country) => {
                        opponent.country_random = false;
                        opponent.country = country;
                    }
                }
            }
        }
        (SkirmishComboId::Color(0), SkirmishComboItem::Color(color)) => {
            let color = color.min(HOUSE_COLOR_COUNT - 1);
            evict_other_color_claimants(state, 0, color);
            state.player_color_index = color;
            state.player_color_claimed = true;
        }
        (SkirmishComboId::Color(row), SkirmishComboItem::Color(color)) => {
            let color = color.min(HOUSE_COLOR_COUNT - 1);
            evict_other_color_claimants(state, row, color);
            if let Some(opponent) = state.opponents.get_mut(row - 1) {
                opponent.color_index = color;
                opponent.color_claimed = true;
            }
        }
        (SkirmishComboId::Color(0), SkirmishComboItem::ColorSentinel(_)) => {
            state.player_color_claimed = false;
        }
        (SkirmishComboId::Color(row), SkirmishComboItem::ColorSentinel(_)) => {
            if let Some(opponent) = state.opponents.get_mut(row - 1) {
                opponent.color_claimed = false;
            }
        }
        (SkirmishComboId::Start(0), SkirmishComboItem::Start(start)) => {
            state.player_start_position = start;
        }
        (SkirmishComboId::Start(row), SkirmishComboItem::Start(start)) => {
            if let Some(opponent) = state.opponents.get_mut(row - 1) {
                opponent.start_position = start;
            }
        }
        (SkirmishComboId::Team(0), SkirmishComboItem::Team(team)) => {
            state.player_team = team;
        }
        (SkirmishComboId::Team(row), SkirmishComboItem::Team(team)) => {
            if let Some(opponent) = state.opponents.get_mut(row - 1) {
                opponent.team = team;
            }
        }
        _ => {}
    }
}

pub(super) fn handle_combo_mouse_down(
    state: &mut SkirmishShellState,
    layout: &SkirmishShellLayout,
    maps: &[MapMenuEntry],
    x: i32,
    y: i32,
) -> bool {
    if let Some(open) = state.open_combo_dropdown {
        if let Some(scrollbar) = combo_dropdown_scrollbar_rect(state, layout, maps, open.id) {
            if scrollbar.contains(x, y) {
                if y < scrollbar.y + COMBO_DROPDOWN_SCROLLBAR_BUTTON_H {
                    state.dropdown_scroll_press = Some(DropdownScrollbarPressState {
                        id: open.id,
                        part: DropdownScrollbarPart::UpArrow,
                    });
                    scroll_open_combo_by_rows(state, maps, open.id, -1);
                } else if y >= scrollbar.y + scrollbar.h - COMBO_DROPDOWN_SCROLLBAR_BUTTON_H {
                    state.dropdown_scroll_press = Some(DropdownScrollbarPressState {
                        id: open.id,
                        part: DropdownScrollbarPart::DownArrow,
                    });
                    scroll_open_combo_by_rows(state, maps, open.id, 1);
                } else if let Some(thumb) =
                    combo_dropdown_scroll_thumb_rect(state, layout, maps, open.id)
                {
                    if thumb.contains(x, y) {
                        state.dropdown_scroll_press = Some(DropdownScrollbarPressState {
                            id: open.id,
                            part: DropdownScrollbarPart::Thumb,
                        });
                        state.dropdown_scroll_drag = Some(DropdownScrollDragState {
                            id: open.id,
                            grab_offset_y: y - thumb.y,
                        });
                    } else if let Some(top_index) =
                        top_index_from_scrollbar_track_click(state, layout, maps, open.id, y)
                    {
                        state.dropdown_scroll_press = Some(DropdownScrollbarPressState {
                            id: open.id,
                            part: DropdownScrollbarPart::Track,
                        });
                        set_open_combo_top_index(state, maps, open.id, top_index);
                    }
                }
                return true;
            }
        }
        if let Some(dropdown) = combo_dropdown_rect(state, layout, maps, open.id) {
            if let Some(content) = combo_dropdown_content_rect(state, layout, maps, open.id) {
                if content.contains(x, y) {
                    let row = open.top_index + ((y - content.y) / COMBO_DROPDOWN_ROW_H) as usize;
                    if let Some(item) = combo_items(state, maps, open.id).get(row).copied() {
                        apply_combo_selection(state, open.id, item);
                    }
                    state.open_combo_dropdown = None;
                    state.dropdown_scroll_drag = None;
                    state.dropdown_scroll_press = None;
                    state.push_ui_sound(SkirmishShellUiSound::GuiComboCloseSound);
                    return true;
                }
            }
            if dropdown.contains(x, y) {
                // The click landed in popup chrome rather than a row; native
                // ComboDropWin consumes it without closing the popup.
                return true;
            }
        }
        // The open dropdown is a captured popup: any mouse-down outside its
        // content/scrollbar is delivered to the popup, which plays the close
        // sound and closes itself. Because the popup consumed the click, it
        // does NOT propagate to a second combo — switching combos takes two
        // clicks (this one closes, a later one opens). So even when the click
        // lands on another combo's arrow, we only close here.
        state.open_combo_dropdown = None;
        state.dropdown_scroll_drag = None;
        state.dropdown_scroll_press = None;
        state.push_ui_sound(SkirmishShellUiSound::GuiComboCloseSound);
        return true;
    }

    // A click anywhere on a collapsed combo face plays the open sound. Only a
    // click inside the rightmost arrow zone actually opens the dropdown.
    if let Some(id) = combo_face_at(state, layout, maps, x, y) {
        state.push_ui_sound(SkirmishShellUiSound::GuiComboOpenSound);
        if combo_arrow_at(state, layout, maps, x, y) == Some(id) {
            state.open_combo_dropdown = Some(OpenComboDropdown { id, top_index: 0 });
            state.dropdown_scroll_drag = None;
            state.dropdown_scroll_press = None;
        }
        return true;
    }

    false
}
