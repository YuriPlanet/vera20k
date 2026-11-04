//! VERA's per-map Skirmish setup; gamemd's `0x102` has no counterpart. Save
//! Setup keeps the selected map's player rows and options, and Load Setup
//! puts them back, so a map can be played again without setting every row.
//! The app keeps one setup per map file.
//!
//! Loading goes through the page's own combo rules: each saved choice is
//! picked as a click on its list would pick it, so a row gets only what the
//! page offers it. A colour or start an earlier row took, a start past the
//! map's or a team the mode does not list leaves the row as the load opened
//! it. Both reach every row the map seats, shown or not.

use std::ops::Range;

use crate::map::scenario_menu::MapMenuEntry;
use crate::skirmish_launch::LaunchStartPosition;

use super::super::layout::SkirmishTrackbarId;
use super::combos::{apply_combo_selection, combo_open};
use super::player_name::selected_player_rows;
use super::{
    SkirmishAiRowType, SkirmishComboId, SkirmishComboItem, SkirmishCountryChoice,
    SkirmishShellState, combo_items, game_speed_visual_position, selected_combo_item,
};

/// One row's Side, Color, Start and Team picks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkirmishSetupSeat {
    pub country: SkirmishCountryChoice,
    /// `None` is Random.
    pub color: Option<usize>,
    pub start: LaunchStartPosition,
    /// The Team combo's item data: -2 for none, 0..=3 for A..D.
    pub team: i32,
}

/// A computer row: its player type, then its seat while it plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkirmishSetupComputer {
    pub row_type: SkirmishAiRowType,
    pub seat: SkirmishSetupSeat,
}

/// A map's saved page: the player's row, the computer rows the map shows and
/// the options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkirmishSavedSetup {
    pub player: SkirmishSetupSeat,
    pub computers: Vec<SkirmishSetupComputer>,
    /// As `[Skirmish] GameSpeed` stores it (0 fastest), not the slider's position.
    pub game_speed: i32,
    pub credits: i32,
    pub unit_count: i32,
    pub short_game: bool,
    pub super_weapons: bool,
    pub build_off_ally: bool,
    pub crates: bool,
    pub mcv_redeploy: bool,
}

impl SkirmishSavedSetup {
    /// The page as it shows: what each combo shows and the options' values.
    pub fn capture(state: &SkirmishShellState, maps: &[MapMenuEntry]) -> Self {
        let computers = computer_rows(state, maps)
            .map(|idx| SkirmishSetupComputer {
                row_type: state.opponents[idx].row_type,
                seat: seat_shown(state, idx + 1),
            })
            .collect();
        Self {
            player: seat_shown(state, 0),
            computers,
            game_speed: state.game_speed,
            credits: state.credits(),
            unit_count: state.unit_count,
            short_game: state.short_game,
            super_weapons: state.super_weapons,
            build_off_ally: state.build_off_ally,
            crates: state.crates,
            mcv_redeploy: state.mcv_redeploy,
        }
    }

    /// Put the setup on the page.
    pub fn apply(&self, state: &mut SkirmishShellState, maps: &[MapMenuEntry]) {
        // Close every computer row and free the player's colour and start
        // first, so no claim left on the page blocks a saved one.
        for idx in 0..state.opponents.len() {
            apply_combo_selection(
                state,
                SkirmishComboId::AiType(idx),
                SkirmishComboItem::AiType(SkirmishAiRowType::None),
            );
        }
        apply_combo_selection(
            state,
            SkirmishComboId::Color(0),
            SkirmishComboItem::ColorSentinel(-2),
        );
        apply_combo_selection(
            state,
            SkirmishComboId::Start(0),
            SkirmishComboItem::Start(LaunchStartPosition::Auto),
        );

        pick_seat(state, maps, 0, self.player);
        let rows = computer_rows(state, maps).len();
        for (idx, computer) in self.computers.iter().enumerate().take(rows) {
            let row_type = SkirmishComboItem::AiType(computer.row_type);
            pick(state, maps, SkirmishComboId::AiType(idx), row_type);
            pick_seat(state, maps, idx + 1, computer.seat);
        }

        // A value outside its slider's range leaves the slider where it is.
        let bounds = state.trackbar_bounds;
        let fits = |id, value| {
            let range = bounds.range(id);
            (range.min..=range.max).contains(&value)
        };
        let speed = game_speed_visual_position(self.game_speed);
        if fits(SkirmishTrackbarId::GameSpeed0x529, speed) {
            state.game_speed = self.game_speed;
        }
        if fits(SkirmishTrackbarId::Credits0x511, self.credits) {
            state.starting_credits = self.credits;
        }
        if fits(SkirmishTrackbarId::UnitCount0x50c, self.unit_count) {
            state.unit_count = self.unit_count;
        }
        state.short_game = self.short_game;
        state.super_weapons = self.super_weapons;
        state.build_off_ally = self.build_off_ally;
        state.crates = self.crates;
        state.mcv_redeploy = self.mcv_redeploy;
    }
}

/// The computer rows the selected map seats, as `opponents` indices.
fn computer_rows(state: &SkirmishShellState, maps: &[MapMenuEntry]) -> Range<usize> {
    0..selected_player_rows(state, maps)
        .saturating_sub(1)
        .min(state.opponents.len())
}

/// What the row's Side, Color, Start and Team combos show.
fn seat_shown(state: &SkirmishShellState, row: usize) -> SkirmishSetupSeat {
    let shown = |id| selected_combo_item(state, id);
    SkirmishSetupSeat {
        country: match shown(SkirmishComboId::Side(row)) {
            Some(SkirmishComboItem::Country(choice)) => choice,
            _ => SkirmishCountryChoice::Random,
        },
        color: match shown(SkirmishComboId::Color(row)) {
            Some(SkirmishComboItem::Color(color)) => Some(color),
            _ => None,
        },
        start: match shown(SkirmishComboId::Start(row)) {
            Some(SkirmishComboItem::Start(start)) => start,
            _ => LaunchStartPosition::Auto,
        },
        team: match shown(SkirmishComboId::Team(row)) {
            Some(SkirmishComboItem::Team(team)) => team,
            _ => -2,
        },
    }
}

fn pick_seat(
    state: &mut SkirmishShellState,
    maps: &[MapMenuEntry],
    row: usize,
    seat: SkirmishSetupSeat,
) {
    let color = seat.color.map_or(
        SkirmishComboItem::ColorSentinel(-2),
        SkirmishComboItem::Color,
    );
    for (id, item) in [
        (
            SkirmishComboId::Side(row),
            SkirmishComboItem::Country(seat.country),
        ),
        (SkirmishComboId::Color(row), color),
        (
            SkirmishComboId::Start(row),
            SkirmishComboItem::Start(seat.start),
        ),
        (
            SkirmishComboId::Team(row),
            SkirmishComboItem::Team(seat.team),
        ),
    ] {
        pick(state, maps, id, item);
    }
}

/// Pick `item` as a click on the combo's list would: only on an open combo
/// that lists it.
fn pick(
    state: &mut SkirmishShellState,
    maps: &[MapMenuEntry],
    id: SkirmishComboId,
    item: SkirmishComboItem,
) {
    if combo_open(state, id) && combo_items(state, maps, id).contains(&item) {
        apply_combo_selection(state, id, item);
    }
}
