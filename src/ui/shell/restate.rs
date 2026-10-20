//! In-scenario mission restate briefing, RT_DIALOG 0x10C (in-scenario state
//! 9): `ScenarioClass::ShowMissionRestateBriefing` 0x0065F520, proc
//! 0x0065F710.
//!
//! Geometry: `ResizeShellChildControl` 0x0060C0C0 places 0x10C as it does
//! Diplomacy 0x73 — title 0x694 in the right-panel static, Return 0x423 as
//! the lower button (`0x006097A0` groups 0x73 and 0x10C), status line 0x695
//! through 0x0060B550, and the briefing text 0x6E7 through 0x0060B7A0
//! (0x0060C3D9). The template, that placement and the 0x006040B0 help keys
//! are pinned by `tools/storage_oracle/restate_shell_layout.py`.
//!
//! The text is a plain kind-0 static: left and top aligned, wrapped to its
//! rect and clipped at its height, with no scrolling (`OwnerDraw_Static`
//! 0x006153E0 → `BitText__DrawText` 0x00434CD0).

use super::button::ShellButtonInteraction;
use super::geom::{self, RectPx};
use super::in_game_shell::{self, InGameShellLayout};

pub const TITLE_LABEL: (&str, &str) = ("GUI:Briefing", "Briefing");
pub const RETURN_LABEL: (&str, &str) = ("GUI:Return", "Return");
/// The briefing text 0x6E7's template rect, in dialog units.
const CONTENT_DLU: RectPx = RectPx::new(61, 45, 301, 265);

/// The dialog's only owner-draw push button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestateButton {
    /// 0x423: the proc writes 0x423 to the result slot (0x0065F710), which
    /// ends the modal.
    Return,
}

/// A child the pointer can rest on, for the status-line help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestateControl {
    Return,
    Content,
}

impl RestateControl {
    pub fn help_key(self) -> Option<&'static str> {
        match self {
            Self::Return => Some("STT:BriefingButtonReturn"),
            Self::Content => Some("STT:BriefingLabelContent"),
        }
    }

    pub fn id(self) -> u16 {
        match self {
            Self::Return => 0x423,
            Self::Content => 0x6E7,
        }
    }
}

/// Pointer state while 0x10C is open: Return's capture and the child whose
/// help the status line shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RestateInteraction {
    pub button: ShellButtonInteraction<RestateButton>,
    pub hovered: Option<RestateControl>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestateLayout {
    pub title: RectPx,
    pub return_button: RectPx,
    pub status_line: RectPx,
    pub content: RectPx,
}

pub fn restate_layout(
    width: i32,
    height: i32,
    shell: InGameShellLayout,
    button_size: [i32; 2],
) -> RestateLayout {
    RestateLayout {
        title: in_game_shell::right_panel_title_rect(width),
        return_button: shell.lower_button_rect(width, button_size),
        status_line: in_game_shell::status_line_rect(height),
        content: in_game_shell::ordinary_child_rect(
            geom::dlu_rect(CONTENT_DLU.x, CONTENT_DLU.y, CONTENT_DLU.w, CONTENT_DLU.h),
            width,
            height,
        ),
    }
}

impl RestateLayout {
    pub fn control_rect(&self, control: RestateControl) -> RectPx {
        match control {
            RestateControl::Return => self.return_button,
            RestateControl::Content => self.content,
        }
    }

    pub fn control_at(&self, x: i32, y: i32) -> Option<RestateControl> {
        [RestateControl::Return, RestateControl::Content]
            .into_iter()
            .find(|control| self.control_rect(*control).contains(x, y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::shell::in_game_shell::allied_fixture_layout as shell;

    fn corpus() -> serde_json::Value {
        serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/restate_shell_layout.json",
        ))
        .unwrap()
    }

    #[test]
    fn the_briefing_text_lands_where_native_placed_it() {
        let corpus = corpus();
        let cases = corpus["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 3);
        for case in cases {
            assert_eq!(case["control_id"], u64::from(RestateControl::Content.id()));
            let (width, height) = (
                case["width"].as_i64().unwrap() as i32,
                case["height"].as_i64().unwrap() as i32,
            );
            let rect = restate_layout(width, height, shell(width, height), [125, 25]).content;
            assert_eq!(
                serde_json::json!([rect.x, rect.y, rect.w, rect.h]),
                case["rect"],
                "{width}x{height}"
            );
        }
    }

    #[test]
    fn title_return_and_status_line_share_the_diplomacy_placements() {
        use crate::ui::shell::diplomacy::diplomacy_layout;
        for (width, height) in [(640, 480), (800, 600), (1024, 768)] {
            let shell = shell(width, height);
            let layout = restate_layout(width, height, shell, [125, 25]);
            let diplomacy = diplomacy_layout(width, height, shell, [125, 25]);
            assert_eq!(layout.title, diplomacy.title);
            assert_eq!(layout.return_button, diplomacy.return_button);
            assert_eq!(layout.status_line, diplomacy.status_line);
        }
    }

    #[test]
    fn help_keys_and_captions_match_the_native_template() {
        let corpus = corpus();
        let control = |id: u16| {
            corpus["controls"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["control_id"] == u64::from(id))
                .unwrap()
                .clone()
        };
        for child in [RestateControl::Return, RestateControl::Content] {
            assert_eq!(
                control(child.id())["help"].as_str(),
                child.help_key(),
                "{child:?}"
            );
        }
        assert_eq!(control(0x694)["caption"], TITLE_LABEL.0);
        assert_eq!(control(0x423)["caption"], RETURN_LABEL.0);
        assert_eq!(
            control(0x6E7)["dlu_rect"],
            serde_json::json!([CONTENT_DLU.x, CONTENT_DLU.y, CONTENT_DLU.w, CONTENT_DLU.h])
        );
    }
}
