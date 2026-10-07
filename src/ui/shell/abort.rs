//! Offline Skirmish B6, original4F1840/4F18B0 and resourceBEFBC0.
//! Mode5 hides secondary action712; the ordinary question uses60B7A0.

use super::geom::{self, RectPx};
use super::in_game_shell::InGameShellLayout;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbortButton {
    Leave,
    Resume,
}

impl AbortButton {
    pub const ALL: [Self; 2] = [Self::Leave, Self::Resume];

    pub fn label(self) -> (&'static str, &'static str) {
        match self {
            Self::Leave => ("GUI:Leave", "Quit"),
            Self::Resume => ("GUI:ResumeMission", "Resume Mission"),
        }
    }

    pub fn help(self) -> (&'static str, &'static str) {
        match self {
            Self::Leave => ("STT:ConfirmExitButtonLeave", "Abort and leave the game."),
            Self::Resume => ("STT:ConfirmExitButtonResume", "Resume mission."),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AbortLayout {
    pub leave: RectPx,
    pub resume: RectPx,
    pub question: RectPx,
    pub footer: RectPx,
}

impl AbortLayout {
    pub fn button(self, button: AbortButton) -> RectPx {
        match button {
            AbortButton::Leave => self.leave,
            AbortButton::Resume => self.resume,
        }
    }

    pub fn hit(self, x: i32, y: i32) -> Option<AbortButton> {
        AbortButton::ALL
            .into_iter()
            .find(|button| self.button(*button).contains(x, y))
    }
}

pub fn abort_layout(
    width: i32,
    height: i32,
    shell: InGameShellLayout,
    button_size: [i32; 2],
) -> AbortLayout {
    AbortLayout {
        // 608CD0 B6 recognizes6C9/712/524;60B000 handles the upper action.
        leave: shell.button_rect(geom::dlu_rect(431, 122, 108, 23), button_size),
        resume: RectPx::new(
            width - 147,
            shell.side3.y - button_size[1],
            button_size[0],
            button_size[1],
        ),
        // 60C3D1 selects complete60B7A0. Signed halves truncate toward zero.
        question: question_rect(width, height),
        footer: RectPx::new(10, height - 21, 455, 20),
    }
}

fn question_rect(width: i32, height: i32) -> RectPx {
    let raw = geom::dlu_rect(99, 165, 230, 19);
    RectPx::new(
        (raw.x + (width - 800) / 2).max(0),
        (raw.y + (height - 600) / 2).max(0),
        raw.w,
        raw.h,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn question_matches_original_resource_and_executed_placement() {
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/abort_shell_layout.json",
        ))
        .unwrap();
        let question = fixture["resource"]["controls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["control_id"] == 0xffff)
            .unwrap();
        assert_eq!(question["caption"], "GUI:AskAbortMission");
        assert_eq!(question["dlu_rect"], serde_json::json!([99, 165, 230, 19]));
        for case in fixture["cases"].as_array().unwrap() {
            let native: Vec<i32> = case["rect"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap() as i32)
                .collect();
            assert_eq!(
                question_rect(
                    case["width"].as_i64().unwrap() as i32,
                    case["height"].as_i64().unwrap() as i32
                ),
                RectPx::new(native[0], native[1], native[2], native[3])
            );
        }
    }
}
