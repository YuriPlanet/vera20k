//! The skirmish shell's combo rows: their items, selections and what a pick
//! does to the session. The widgets themselves are [`super::super::page`]'s.

use crate::map::scenario_menu::MapMenuEntry;
use crate::skirmish_launch::{
    HOUSE_COLOR_COUNT, LaunchCountry, LaunchStartPosition, SKIRMISH_MAX_SLOT_COUNT,
    SKIRMISH_PLAYER_SLOT_COUNT,
};

use super::super::layout::{RectPx, SEAT_MENU, SkirmishShellLayout, TEAM_MENU};
use super::{
    SkirmishAiRowType, SkirmishComboId, SkirmishComboItem, SkirmishCountryChoice,
    SkirmishShellOpponent, SkirmishShellState, inactive_ai_team_default, player_row_visible,
};

/// The control rect of `id`'s player row; `None` while VERA's scrollbar
/// shows other computer rows.
pub fn combo_rect(
    layout: &SkirmishShellLayout,
    state: &SkirmishShellState,
    id: SkirmishComboId,
) -> Option<RectPx> {
    match id {
        SkirmishComboId::TeamMenu => return layout.statics.get(TEAM_MENU),
        SkirmishComboId::SeatMenu => return layout.statics.get(SEAT_MENU),
        _ => {}
    }
    let slot = state.row_slot(combo_player_row(id)?, layout.shown_rows)?;
    match id {
        SkirmishComboId::AiType(_) => layout.rows.ai_type_combos.get(slot.checked_sub(1)?),
        SkirmishComboId::Side(_) => layout.rows.side_combos.get(slot),
        SkirmishComboId::Color(_) => layout.color_combos.get(slot),
        SkirmishComboId::Start(_) => layout.rows.start_combos.get(slot),
        SkirmishComboId::Team(_) => layout.rows.team_combos.get(slot),
        SkirmishComboId::TeamMenu | SkirmishComboId::SeatMenu => None,
    }
    .copied()
}

pub const fn combo_dropdown_max_visible_rows(id: SkirmishComboId) -> i32 {
    match id {
        SkirmishComboId::Side(_) => 7,
        SkirmishComboId::Color(_) | SkirmishComboId::Start(_) => 9,
        SkirmishComboId::AiType(_) | SkirmishComboId::TeamMenu | SkirmishComboId::SeatMenu => 0,
        // VERA: past eight starts the Team list runs A to O, so it scrolls
        // at nine rows instead of running over the status line. gamemd's
        // five rows never reach the cap.
        SkirmishComboId::Team(_) => 9,
    }
}

/// A row's colour as the lobby shows it: a listed priority is itself, and
/// anything else falls back onto YR's eight.
fn normal_color_index(state: &SkirmishShellState, color_index: usize) -> usize {
    listed_color_index(state.lobby_colors, color_index)
}

fn listed_color_index(
    colors: crate::rules::skirmish_colors::SkirmishColorMode,
    color_index: usize,
) -> usize {
    if i32::try_from(color_index).is_ok_and(|color| colors.is_lobby_priority(color)) {
        color_index
    } else {
        color_index.min(HOUSE_COLOR_COUNT - 1)
    }
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
        SkirmishComboId::Side(_) => state.country_catalog.items().to_vec(),
        SkirmishComboId::Color(row) => available_color_choices(
            state.lobby_colors,
            selected_color_claim(state, row),
            state
                .lobby_colors
                .lobby_priorities()
                .map(usize::from)
                .filter(|color| color_claimed_by_other_row(state, row, *color).is_some()),
        ),
        SkirmishComboId::Start(row) => {
            // VERA lists a map's starts up to SKIRMISH_MAX_SLOT_COUNT.
            let capacity = maps
                .get(state.selected_map_idx)
                .map(|map| map.player_capacity.clamp(0, SKIRMISH_MAX_SLOT_COUNT as i32) as usize)
                .unwrap_or(SKIRMISH_PLAYER_SLOT_COUNT);
            available_start_choices(
                selected_start_position(state, row),
                (0..SKIRMISH_MAX_SLOT_COUNT)
                    .map(|index| LaunchStartPosition::Position(index as u8))
                    .filter(|start| start_position_taken_by_other_row(state, row, *start)),
                capacity,
            )
        }
        SkirmishComboId::Team(_) => team_items(state, maps),
        SkirmishComboId::TeamMenu => super::team_menus::team_menu_items(state, maps),
        SkirmishComboId::SeatMenu => super::team_menus::seat_menu_items(state, maps),
    }
}

/// Shared4E3A00 country catalog: registered source order, Multiplay enabled,
/// and a nonempty HouseType UIName. Random is the first entry. The catalog
/// has no index cap, which Ares 3.0 HTExt_Unlimit1..5 (1001DD70..1001DDB0,
/// hooks 0x4E3792, 0x4E3A9C, 0x4E3F31, 0x4E412C, 0x4E41A7) lift from the
/// shell's 0..9 country range checks; the status line is the Country's
/// `MenuText.Status` (LoadPlayerCountryString 1001DF10, hook 0x4E38D8).
pub fn country_choices(rules: &crate::rules::ruleset::RuleSet) -> Vec<SkirmishComboItem> {
    let mut countries: Vec<(i32, usize, LaunchCountry)> = rules
        .cooperative_country_candidates()
        .enumerate()
        .filter(|(_, (id, _))| {
            rules.country_multiplay(id)
                && rules
                    .country_display_name_sources(id)
                    .0
                    .is_some_and(|key| !key.is_empty())
        })
        .filter_map(|(index, (id, _))| {
            let list_index = rules.country_list_index(id);
            (list_index >= 0).then(|| {
                (
                    list_index,
                    index,
                    LaunchCountry::from_country_index(index as u32),
                )
            })
        })
        .collect();
    // Ares ListIndex, then the Country registry index. Without Ares every
    // Country's is the common 100, so this preserves source order.
    countries.sort_by_key(|&(list_index, index, _)| (list_index, index));
    std::iter::once(SkirmishCountryChoice::Random)
        .chain(
            countries
                .into_iter()
                .map(|(_, _, country)| SkirmishCountryChoice::Country(country)),
        )
        .map(SkirmishComboItem::Country)
        .collect()
}
/// Shared4E4770/4E48E0 color catalog. A row retains its current claim while
/// other occupied rows remove those colors from its available entries. The
/// colours are the priorities `colors` lists: YR's eight, then VERA's extras.
pub fn available_color_choices(
    colors: crate::rules::skirmish_colors::SkirmishColorMode,
    selected: Option<usize>,
    claimed: impl IntoIterator<Item = usize>,
) -> Vec<SkirmishComboItem> {
    let claimed: Vec<_> = claimed.into_iter().collect();
    std::iter::once(SkirmishComboItem::ColorSentinel(-2))
        .chain(
            colors
                .lobby_priorities()
                .map(usize::from)
                .filter(|color| selected == Some(*color) || !claimed.contains(color))
                .map(SkirmishComboItem::Color),
        )
        .collect()
}
/// Shared4E50C0 start catalog; capacity comes from the selected scenario owner.
pub fn available_start_choices(
    selected: Option<LaunchStartPosition>,
    claimed: impl IntoIterator<Item = LaunchStartPosition>,
    capacity: usize,
) -> Vec<SkirmishComboItem> {
    let claimed: Vec<_> = claimed.into_iter().collect();
    std::iter::once(SkirmishComboItem::Start(LaunchStartPosition::Auto))
        .chain(
            (0..capacity.min(crate::skirmish_launch::SKIRMISH_MAX_SLOT_COUNT))
                .map(|index| LaunchStartPosition::Position(index as u8))
                .filter(|start| selected == Some(*start) || !claimed.contains(start))
                .map(SkirmishComboItem::Start),
        )
        .collect()
}
/// Shared4E5B60 team catalog: None (-2) unless the mode must ally, then
/// teams `0..count`. gamemd lists [`NATIVE_TEAMS`], A to D.
///
/// [`NATIVE_TEAMS`]: crate::skirmish_launch::NATIVE_TEAMS
pub fn team_choices(must_ally: bool, count: u8) -> Vec<SkirmishComboItem> {
    let none = (!must_ally).then_some(-2);
    none.into_iter()
        .chain(0..i32::from(count))
        .map(SkirmishComboItem::Team)
        .collect()
}

/// Every row's Team combo shows this list, and the page's labels answer for
/// it: a map past gamemd's eight starts reaches [`SKIRMISH_TEAMS`], so the
/// labels must name E..O rather than stop at D.
///
/// [`SKIRMISH_TEAMS`]: crate::skirmish_launch::SKIRMISH_TEAMS
pub fn team_items(state: &SkirmishShellState, maps: &[MapMenuEntry]) -> Vec<SkirmishComboItem> {
    team_choices(
        state.selected_mode_must_ally,
        skirmish_team_count(state, maps),
    )
}

/// The teams a skirmish Team list offers: gamemd's A to D, and VERA's
/// [`SKIRMISH_TEAMS`] on a map past gamemd's eight starts.
///
/// [`SKIRMISH_TEAMS`]: crate::skirmish_launch::SKIRMISH_TEAMS
fn skirmish_team_count(state: &SkirmishShellState, maps: &[MapMenuEntry]) -> u8 {
    if super::selected_player_rows(state, maps) > SKIRMISH_PLAYER_SLOT_COUNT {
        crate::skirmish_launch::SKIRMISH_TEAMS
    } else {
        crate::skirmish_launch::NATIVE_TEAMS
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
            SkirmishComboItem::Color(normal_color_index(state, state.player_color_index))
        } else {
            SkirmishComboItem::ColorSentinel(-2)
        }),
        SkirmishComboId::Color(row) => state.opponents.get(row - 1).map(|opponent| {
            if opponent.color_claimed {
                SkirmishComboItem::Color(normal_color_index(state, opponent.color_index))
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
        // A menu shows its caption, not a choice.
        SkirmishComboId::TeamMenu | SkirmishComboId::SeatMenu => None,
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

/// The player row `id` belongs to; none for VERA's menus under the rows.
pub(super) fn combo_player_row(id: SkirmishComboId) -> Option<usize> {
    match id {
        SkirmishComboId::AiType(idx) => Some(idx + 1),
        SkirmishComboId::Side(row)
        | SkirmishComboId::Color(row)
        | SkirmishComboId::Start(row)
        | SkirmishComboId::Team(row) => Some(row),
        SkirmishComboId::TeamMenu | SkirmishComboId::SeatMenu => None,
    }
}

pub fn combo_enabled(
    state: &SkirmishShellState,
    maps: &[MapMenuEntry],
    id: SkirmishComboId,
) -> bool {
    combo_player_row(id).is_none_or(|row| player_row_visible(state, maps, row))
        && combo_open(state, id)
}

/// Whether the combo takes a pick wherever its row shows: the row's player
/// type and the mode's `AlliesAllowed` decide.
pub(crate) fn combo_open(state: &SkirmishShellState, id: SkirmishComboId) -> bool {
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
        // Teams follow the same gate; the seats are always the player's.
        SkirmishComboId::TeamMenu => state.selected_mode_allies_allowed,
        SkirmishComboId::SeatMenu => true,
    }
}

fn selected_start_position(state: &SkirmishShellState, row: usize) -> Option<LaunchStartPosition> {
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
    start: LaunchStartPosition,
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
            Some(normal_color_index(state, state.player_color_index))
        } else {
            None
        }
    } else {
        state.opponents.get(row - 1).and_then(|opponent| {
            if opponent.color_claimed {
                Some(normal_color_index(state, opponent.color_index))
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
        && normal_color_index(state, state.player_color_index) == color
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
                && normal_color_index(state, opponent.color_index) == color
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
        && normal_color_index(state, state.player_color_index) == color
    {
        state.player_color_claimed = false;
    }
    let colors = state.lobby_colors;
    for (idx, opponent) in state.opponents.iter_mut().enumerate() {
        let opponent_row = idx + 1;
        if opponent_row != row
            && opponent.color_claimed
            && listed_color_index(colors, opponent.color_index) == color
        {
            opponent.color_claimed = false;
        }
    }
}

pub(crate) fn apply_combo_selection(
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
            let color = normal_color_index(state, color);
            evict_other_color_claimants(state, 0, color);
            state.player_color_index = color;
            state.player_color_claimed = true;
        }
        (SkirmishComboId::Color(row), SkirmishComboItem::Color(color)) => {
            let color = normal_color_index(state, color);
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
        (
            SkirmishComboId::TeamMenu,
            SkirmishComboItem::TeamTool {
                tool,
                refusal: None,
            },
        ) => {
            state.menu_pick = Some(super::MenuPick::Team(tool));
        }
        (
            SkirmishComboId::SeatMenu,
            SkirmishComboItem::SeatAction {
                action,
                enabled: true,
            },
        ) => {
            state.menu_pick = Some(super::MenuPick::Seats(action));
        }
        _ => {}
    }
}
