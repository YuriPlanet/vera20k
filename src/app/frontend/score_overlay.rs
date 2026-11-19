//! Optional presentation and controls for a standalone mission result.

use std::fmt::Debug;
use std::sync::Arc;

use crate::app::AppState;
use crate::render::shell_paint::PaintLabel;
use crate::ui::campaign_score_shell::{
    ENEMIES_DESTROYED_LABEL_STATIC, ENEMIES_DESTROYED_VALUE_STATIC,
};
use crate::ui::score_shell::{ROOM_RESULT_EXIT, ROOM_RESULT_PRIMARY};
use crate::ui::shell::geom::{RectPx, RightPanelRects, ShellScale, dialog_origin};
use crate::ui::shell::menu_page::MenuPageLayout;

pub(crate) use crate::ui::campaign_score_shell::{
    CONTINUE_BUTTON as CONTINUE_CONTROL, CampaignScoreScreenModel as ScoreHeader,
};

pub(crate) const ENEMIES_DESTROYED_STATICS: &[u16] = &[
    ENEMIES_DESTROYED_LABEL_STATIC,
    ENEMIES_DESTROYED_VALUE_STATIC,
];

/// Geometry shared by result content on the standalone and room score shells.
pub(crate) struct ScoreLayout {
    pub(crate) scale: ShellScale,
    pub(crate) screen: RectPx,
    pub(crate) origin: (i32, i32),
    pub(crate) right_panel: RightPanelRects,
    pub(crate) title: RectPx,
    pub(crate) primary_button: Option<RectPx>,
    pub(crate) exit_button: Option<RectPx>,
}

impl From<&crate::ui::campaign_score_shell::CampaignScoreLayout> for ScoreLayout {
    fn from(layout: &crate::ui::campaign_score_shell::CampaignScoreLayout) -> Self {
        Self {
            scale: layout.scale,
            screen: layout.screen,
            origin: if layout.scale.is_retail() {
                (layout.background.x, layout.background.y)
            } else {
                (0, 0)
            },
            right_panel: layout.right_panel,
            title: layout.title,
            primary_button: Some(layout.continue_button),
            exit_button: None,
        }
    }
}

impl From<&MenuPageLayout> for ScoreLayout {
    fn from(page: &MenuPageLayout) -> Self {
        Self {
            scale: page.scale,
            screen: page.screen,
            origin: if page.scale.is_retail() {
                dialog_origin(page.screen.w, page.screen.h)
            } else {
                (0, 0)
            },
            right_panel: page.right_panel,
            title: page.title,
            primary_button: page.button_rect(ROOM_RESULT_PRIMARY),
            exit_button: page.button_rect(ROOM_RESULT_EXIT),
        }
    }
}

/// An overlay without a native standalone page owns the room result body.
pub(crate) fn room_result_active(state: &AppState) -> bool {
    state.frontend.campaign_score_page.is_none()
        && state.frontend.score_overlay.is_some()
        && state.frontend.skirmish_shell_chrome.is_some()
}

pub(crate) fn room_result_layout(state: &AppState) -> MenuPageLayout {
    let (width, height) = super::shell_layout::shell_screen_size(state);
    crate::ui::score_shell::room_result_layout(
        width,
        height,
        super::shell_layout::fills_window(state),
    )
}

#[derive(Debug, Clone)]
pub(crate) struct ScoreOverlay {
    content: Arc<dyn ScoreContent>,
    view: usize,
}

impl ScoreOverlay {
    pub(crate) fn new(content: impl ScoreContent + 'static) -> Self {
        Self {
            content: Arc::new(content),
            view: 0,
        }
    }

    pub(crate) fn buttons(&self, layout: &ScoreLayout) -> Vec<ScoreButton> {
        self.content.buttons(layout, self.view)
    }

    pub(crate) fn button_enabled(&self, state: &AppState, button: u16) -> bool {
        self.content.button_enabled(state, button)
    }

    pub(crate) fn body(&self, state: &AppState, layout: &ScoreLayout) -> ScoreBody {
        self.content.body(state, layout, self.view)
    }

    pub(crate) fn title(&self) -> Option<String> {
        self.content.title()
    }

    pub(crate) fn map_preview(&self) -> Option<&ScoreMapPreview> {
        self.content.map_preview()
    }

    pub(crate) fn resume(&self, state: &mut AppState) -> bool {
        self.content.resume(state)
    }

    pub(crate) fn activate(&mut self, layout: &ScoreLayout, id: u16) -> Option<ScoreAction> {
        let action = self
            .buttons(layout)
            .into_iter()
            .find(|button| button.id == id)?
            .action;
        if let ScoreAction::View(view) = action {
            self.view = view;
        }
        Some(action)
    }
}

pub(crate) trait ScoreContent: Debug + Send + Sync {
    fn buttons(&self, layout: &ScoreLayout, view: usize) -> Vec<ScoreButton>;
    fn body(&self, state: &AppState, layout: &ScoreLayout, view: usize) -> ScoreBody;

    fn button_enabled(&self, _state: &AppState, _button: u16) -> bool {
        true
    }

    fn title(&self) -> Option<String> {
        None
    }

    fn map_preview(&self) -> Option<&ScoreMapPreview> {
        None
    }

    /// Runs after the finished scenario and result page have been retired.
    fn resume(&self, _state: &mut AppState) -> bool {
        false
    }
}

#[derive(Debug)]
pub(crate) struct ScoreMapPreview {
    pub(crate) scenario: crate::map::scenario_menu::LocalScenario,
    pub(crate) unavailable_label: &'static str,
}

pub(crate) struct ScoreButton {
    pub(crate) id: u16,
    pub(crate) rect: RectPx,
    pub(crate) caption: &'static str,
    pub(crate) action: ScoreAction,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ScoreAction {
    Continue,
    Exit,
    View(usize),
    Run(fn(&mut AppState)),
}

pub(crate) struct ScoreBody {
    /// A replacement body keeps SCRBK and the right panel.
    pub(crate) replaces_stock_body: bool,
    pub(crate) hidden_statics: &'static [u16],
    pub(crate) labels: Vec<PaintLabel<'static>>,
    pub(crate) glyph_scale: f32,
}
