//! The legacy defeat table and its Retry, Return to Room and Leave Room actions.

use crate::app::frontend::score_overlay::{
    ScoreAction, ScoreBody, ScoreButton, ScoreContent, ScoreHeader, ScoreLayout, ScoreMapPreview,
    ScoreOverlay,
};
use crate::app::{App, AppState};
use crate::map::scenario_menu::LocalScenario;
use crate::render::shell_paint::SHELL_TEXT_RGB_ENABLED;
use crate::render::shell_text::ShellAlign;
use crate::ui::score_shell::{NEXT_PAGE, PREVIOUS_PAGE, ROOM_RESULT_EXIT, ROOM_RESULT_PRIMARY};
use crate::ui::shell::geom::{RectPx, SDBTNANM_CELL_H, SDBTNANM_CELL_W_NARROW};

use super::SeatResult;
use super::retry;
use super::statistics_ui::{ROWS_PER_PAGE, Statistics, body_rect, body_scale, label};

const RETURN_ROOM: u16 = 0x7005;

#[derive(Debug)]
struct DefeatUi {
    rows: Vec<SeatResult>,
    statistics: Statistics,
    can_vote: bool,
    map: Option<ScoreMapPreview>,
}

pub(super) fn overlay(
    rows: Vec<SeatResult>,
    header: &ScoreHeader,
    can_vote: bool,
    scenario: Option<LocalScenario>,
) -> ScoreOverlay {
    ScoreOverlay::new(DefeatUi {
        rows,
        statistics: Statistics::new(header),
        can_vote,
        map: scenario.map(|scenario| ScoreMapPreview {
            scenario,
            unavailable_label: "VERA:VmceNoPreview",
        }),
    })
}

impl ScoreContent for DefeatUi {
    fn buttons(&self, layout: &ScoreLayout, view: usize) -> Vec<ScoreButton> {
        let (Some(retry), Some(exit)) = (layout.primary_button, layout.exit_button) else {
            return Vec::new();
        };
        let mut buttons = vec![
            ScoreButton {
                id: ROOM_RESULT_PRIMARY,
                rect: retry,
                caption: "VERA:VmceRetry",
                action: ScoreAction::Run(retry::retry_pressed),
            },
            ScoreButton {
                id: RETURN_ROOM,
                rect: layout
                    .right_panel
                    .row_cell(2, SDBTNANM_CELL_W_NARROW, SDBTNANM_CELL_H),
                caption: "VERA:VmceReturnRoom",
                action: ScoreAction::Run(retry::return_pressed),
            },
            ScoreButton {
                id: ROOM_RESULT_EXIT,
                rect: exit,
                caption: "VERA:VmceLeaveRoom",
                action: ScoreAction::Run(retry::leave_pressed),
            },
        ];
        if view > 0 {
            buttons.push(ScoreButton {
                id: PREVIOUS_PAGE,
                rect: layout
                    .right_panel
                    .row_cell(3, SDBTNANM_CELL_W_NARROW, SDBTNANM_CELL_H),
                caption: "VERA:Previous",
                action: ScoreAction::View(view - 1),
            });
        }
        if (view + 1) * ROWS_PER_PAGE < self.rows.len() {
            buttons.push(ScoreButton {
                id: NEXT_PAGE,
                rect: layout
                    .right_panel
                    .row_cell(4, SDBTNANM_CELL_W_NARROW, SDBTNANM_CELL_H),
                caption: "VERA:Next",
                action: ScoreAction::View(view + 1),
            });
        }
        buttons
    }

    fn button_enabled(&self, state: &AppState, button: u16) -> bool {
        match button {
            ROOM_RESULT_PRIMARY => self.can_vote && retry::can_vote(state),
            RETURN_ROOM | ROOM_RESULT_EXIT => retry::can_leave(state),
            _ => true,
        }
    }

    fn title(&self) -> Option<String> {
        Some(crate::i18n::tr("VERA:VmceMissionFailed"))
    }

    fn map_preview(&self) -> Option<&ScoreMapPreview> {
        self.map.as_ref()
    }

    fn body(&self, state: &AppState, layout: &ScoreLayout, view: usize) -> ScoreBody {
        let first = view * ROWS_PER_PAGE;
        let rows = &self.rows[first..self.rows.len().min(first + ROWS_PER_PAGE)];
        let scale = body_scale(layout);
        let mut labels = self.statistics.labels(state, rows);
        if let Some(status) = retry::status(state) {
            labels.push(label(
                status,
                RectPx::new(40, 274, 560, 22),
                ShellAlign::NONE,
                SHELL_TEXT_RGB_ENABLED,
            ));
        }
        for label in &mut labels {
            label.rect = body_rect(layout, scale, label.rect);
        }
        ScoreBody {
            replaces_stock_body: true,
            hidden_statics: &[],
            labels,
            glyph_scale: scale.factor(),
        }
    }

    fn resume(&self, state: &mut AppState) -> bool {
        App::return_to_lan_room_after_score(state, true);
        true
    }
}

#[cfg(all(test, tests_app_match))]
mod tests {
    use super::*;

    #[test]
    fn defeat_has_three_separate_room_actions_at_1024_by_768() {
        let page = crate::ui::score_shell::room_result_layout(1024, 768, true);
        let layout = ScoreLayout::from(&page);
        let score = overlay(Vec::new(), &ScoreHeader::default(), true, None);
        let buttons = score.buttons(&layout);
        let [retry, return_to_room, leave] = buttons.as_slice() else {
            panic!("expected Retry, Return to Room and Leave Room");
        };
        assert_eq!(retry.id, ROOM_RESULT_PRIMARY);
        assert_eq!(return_to_room.id, RETURN_ROOM);
        assert_eq!(leave.id, ROOM_RESULT_EXIT);
        assert!(retry.rect.y + retry.rect.h <= return_to_room.rect.y);
        assert!(return_to_room.rect.y + return_to_room.rect.h <= leave.rect.y);
    }
}
