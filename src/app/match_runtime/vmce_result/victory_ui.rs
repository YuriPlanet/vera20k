//! VMCE's victory medal page and its statistics view.

use crate::app::frontend::credits_roll::CreditsReturn;
use crate::app::frontend::score_overlay::{
    CONTINUE_CONTROL, ENEMIES_DESTROYED_STATICS, ScoreAction, ScoreBody, ScoreButton, ScoreContent,
    ScoreHeader, ScoreLayout, ScoreOverlay,
};
use crate::app::{App, AppState};
use crate::i18n::tr;
use crate::render::shell_paint::SHELL_TEXT_RGB_ENABLED;
use crate::render::shell_text::ShellAlign;
use crate::ui::score_shell::{NEXT_PAGE, PREVIOUS_PAGE};
use crate::ui::shell::geom::{SDBTNANM_CELL_H, SDBTNANM_CELL_W_NARROW};

use super::statistics_ui::{ROWS_PER_PAGE, Statistics, body_rect, body_scale, label};

const LEAVE_ROOM: u16 = 0x7002;
const STATISTICS: u16 = 0x7003;
const STATISTICS_BACK: u16 = 0x7004;

#[derive(Debug)]
struct VictoryUi {
    rows: Vec<super::SeatResult>,
    statistics: Statistics,
    continuation: Continuation,
}

#[derive(Debug)]
pub(super) enum Continuation {
    Room,
    Credits(Option<String>),
}

pub(super) fn overlay(
    rows: Vec<super::SeatResult>,
    header: &ScoreHeader,
    continuation: Continuation,
) -> ScoreOverlay {
    ScoreOverlay::new(VictoryUi {
        rows,
        statistics: Statistics::new(header),
        continuation,
    })
}

impl ScoreContent for VictoryUi {
    fn buttons(&self, layout: &ScoreLayout, view: usize) -> Vec<ScoreButton> {
        let panel = layout.right_panel;
        let bottom = panel.tile_count.saturating_sub(1);
        let button = |id, row, caption, action| ScoreButton {
            id,
            rect: panel.row_cell(row, SDBTNANM_CELL_W_NARROW, SDBTNANM_CELL_H),
            caption,
            action,
        };
        if view == 0 {
            return vec![
                button(CONTINUE_CONTROL, 0, "GUI:Continue", ScoreAction::Continue),
                button(STATISTICS, 1, "VERA:VmceStatistics", ScoreAction::View(1)),
                button(LEAVE_ROOM, bottom, "VERA:VmceLeaveRoom", ScoreAction::Exit),
            ];
        }

        // Statistics pages follow the medal view, which always occupies view 0.
        let mut buttons = vec![button(
            STATISTICS_BACK,
            bottom,
            "VERA:VmceBack",
            ScoreAction::View(0),
        )];
        if view > 1 {
            buttons.push(button(
                PREVIOUS_PAGE,
                0,
                "VERA:Previous",
                ScoreAction::View(view - 1),
            ));
        }
        if view * ROWS_PER_PAGE < self.rows.len() {
            buttons.push(button(
                NEXT_PAGE,
                1,
                "VERA:Next",
                ScoreAction::View(view + 1),
            ));
        }
        buttons
    }

    fn body(&self, state: &AppState, layout: &ScoreLayout, view: usize) -> ScoreBody {
        let scale = body_scale(layout);
        let mut labels = Vec::new();
        if let Some(page) = view.checked_sub(1) {
            let rows = self
                .rows
                .chunks(ROWS_PER_PAGE)
                .nth(page)
                .unwrap_or_default();
            labels = self.statistics.labels(state, rows);
            for label in &mut labels {
                label.rect = body_rect(layout, scale, label.rect);
            }
            labels.push(label(
                tr("VERA:VmceStatistics"),
                layout.title,
                ShellAlign::H_CENTER,
                SHELL_TEXT_RGB_ENABLED,
            ));
        }
        ScoreBody {
            replaces_stock_body: view != 0,
            hidden_statics: if view == 0 {
                ENEMIES_DESTROYED_STATICS
            } else {
                &[]
            },
            labels,
            glyph_scale: scale.factor(),
        }
    }

    fn title(&self) -> Option<String> {
        Some(tr("VERA:VmceMissionWon"))
    }

    fn resume(&self, state: &mut AppState) -> bool {
        match &self.continuation {
            Continuation::Credits(text) => {
                App::start_credits_roll_with_text(state, CreditsReturn::LanRoom, text.as_deref());
            }
            Continuation::Room => App::return_to_lan_room_after_score(state, false),
        }
        true
    }
}
