//! Full-screen B6, original4F1840/4F18B0 and resourceBEFBC0. Its init
//! (`0x004F18EE..0x004F1A29`) captions6C9 `GUI:Leave` and sets the secondary
//! action712 by GameMode: campaign `GUI:Restart` (result5), skirmish hidden,
//! LAN/WOL `GUI:Observe` (result4, Leave result3) with two or more live
//! humans while the local House is not defeated, otherwise hidden. The
//! ordinary question uses60B7A0.

use super::geom::{self, RectPx};
use super::in_game_shell::{self, InGameShellLayout};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbortButton {
    Leave,
    /// Control712 with whichever caption the mode gave it.
    Secondary,
    Resume,
}

impl AbortButton {
    pub fn label(self, secondary: AbortSecondary) -> (&'static str, &'static str) {
        match (self, secondary) {
            (Self::Leave, _) => ("GUI:Leave", "Quit"),
            // String 0x008240E4, written through msg 0x4B2 at 0x004F1969.
            (Self::Secondary, AbortSecondary::Restart) => ("GUI:Restart", "Restart"),
            // String 0x008240D8, written at 0x004F19FD.
            (Self::Secondary, _) => ("GUI:Observe", "Observe"),
            (Self::Resume, _) => ("GUI:ResumeMission", "Resume Mission"),
        }
    }

    /// `0x006040B0`'s B6 branch keys 6C9 and 712 on the result each holds in
    /// its owner-draw record (`+0xC0`, stored by `0x00603E40` at init): Leave
    /// 2 or 3 (`LeaveEx`), secondary 4 (Observe) or 5 (Restart).
    pub fn help(self, secondary: AbortSecondary) -> (&'static str, &'static str) {
        match (self, secondary) {
            (Self::Leave, AbortSecondary::Observe) => {
                ("STT:ConfirmExitButtonLeaveEx", "Abort and leave the game.")
            }
            (Self::Leave, _) => ("STT:ConfirmExitButtonLeave", "Abort and leave the game."),
            (Self::Secondary, AbortSecondary::Restart) => (
                "STT:ConfirmExitButtonRestart",
                "Restart the campaign mission.",
            ),
            (Self::Secondary, _) => (
                "STT:ConfirmExitButtonObserve",
                "Abort and observe the game.",
            ),
            (Self::Resume, _) => ("STT:ConfirmExitButtonResume", "Resume mission."),
        }
    }
}

/// What B6's init does with control712 for the running GameMode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AbortSecondary {
    /// Skirmish `ShowWindow(712, SW_HIDE)` (`0x004F197F..0x004F1A0E`), and
    /// LAN/WOL with fewer than two live humans or a defeated local House.
    #[default]
    Hidden,
    /// Campaign: caption `GUI:Restart`, result 5 (`0x004F194B..0x004F1975`).
    Restart,
    /// LAN/WOL: caption `GUI:Observe`, result 4; Leave becomes result 3
    /// (`0x004F19D4..0x004F1A09`).
    Observe,
}

impl AbortSecondary {
    /// B6 init's mode switch (`0x004F1938..0x004F1A0E`). `live_humans`
    /// counts Houses with `+0x1EC` set and `+0x1F5` clear, the local House
    /// included (`0x004F1993..0x004F19BD`).
    pub const fn for_mode(
        campaign_restartable: Option<bool>,
        network: bool,
        live_humans: usize,
        local_defeated: bool,
    ) -> Self {
        match campaign_restartable {
            Some(true) => Self::Restart,
            Some(false) => Self::Hidden,
            None if network && live_humans >= 2 && !local_defeated => Self::Observe,
            None => Self::Hidden,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AbortLayout {
    pub secondary_kind: AbortSecondary,
    pub leave: RectPx,
    /// Control712 when the mode shows it.
    pub secondary: Option<RectPx>,
    pub resume: RectPx,
    pub question: RectPx,
    pub footer: RectPx,
}

impl AbortLayout {
    pub fn button(self, button: AbortButton) -> Option<RectPx> {
        match button {
            AbortButton::Leave => Some(self.leave),
            AbortButton::Secondary => self.secondary,
            AbortButton::Resume => Some(self.resume),
        }
    }

    /// The shown buttons in resource order: 6C9, 712, then Resume686.
    pub fn buttons(self) -> impl Iterator<Item = (AbortButton, RectPx)> {
        [
            AbortButton::Leave,
            AbortButton::Secondary,
            AbortButton::Resume,
        ]
        .into_iter()
        .filter_map(move |button| Some((button, self.button(button)?)))
    }

    pub fn hit(self, x: i32, y: i32) -> Option<AbortButton> {
        self.buttons()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(button, _)| button)
    }
}

pub fn abort_layout(
    width: i32,
    height: i32,
    shell: InGameShellLayout,
    button_size: [i32; 2],
    secondary: AbortSecondary,
) -> AbortLayout {
    AbortLayout {
        secondary_kind: secondary,
        // 608CD0 B6 recognizes6C9/712/524;60B000 handles the upper action.
        leave: shell.button_rect(geom::dlu_rect(431, 122, 108, 23), button_size),
        // Resource control712 (431,149,108,23) is the next sidebar row.
        secondary: (secondary != AbortSecondary::Hidden)
            .then(|| shell.button_rect(geom::dlu_rect(431, 149, 108, 23), button_size)),
        resume: shell.lower_button_rect(width, button_size),
        // 60C3D1 selects complete60B7A0. Signed halves truncate toward zero.
        question: question_rect(width, height),
        footer: in_game_shell::status_line_rect(height),
    }
}

fn question_rect(width: i32, height: i32) -> RectPx {
    in_game_shell::ordinary_child_rect(geom::dlu_rect(99, 165, 230, 19), width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> serde_json::Value {
        serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/abort_shell_layout.json",
        ))
        .unwrap()
    }

    fn resource_rect(fixture: &serde_json::Value, control_id: u64) -> RectPx {
        let control = fixture["resource"]["controls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["control_id"] == control_id)
            .unwrap();
        let rect: Vec<i32> = control["dlu_rect"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap() as i32)
            .collect();
        geom::dlu_rect(rect[0], rect[1], rect[2], rect[3])
    }

    #[test]
    fn question_matches_original_resource_and_executed_placement() {
        let fixture = fixture();
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

    #[test]
    fn network_modes_offer_observe_only_to_an_undefeated_player_among_live_humans() {
        use AbortSecondary::*;
        assert_eq!(
            AbortSecondary::for_mode(Some(true), false, 1, false),
            Restart
        );
        assert_eq!(
            AbortSecondary::for_mode(Some(false), false, 1, false),
            Hidden
        );
        assert_eq!(AbortSecondary::for_mode(None, false, 4, false), Hidden);
        assert_eq!(AbortSecondary::for_mode(None, true, 2, false), Observe);
        assert_eq!(AbortSecondary::for_mode(None, true, 1, false), Hidden);
        assert_eq!(AbortSecondary::for_mode(None, true, 3, true), Hidden);
        assert_eq!(
            AbortButton::help(AbortButton::Leave, Observe).0,
            "STT:ConfirmExitButtonLeaveEx"
        );
        assert_eq!(
            AbortButton::help(AbortButton::Leave, Hidden).0,
            "STT:ConfirmExitButtonLeave"
        );
        assert_eq!(
            AbortButton::label(AbortButton::Secondary, Observe).0,
            "GUI:Observe"
        );
        assert_eq!(
            AbortButton::help(AbortButton::Secondary, Restart).0,
            "STT:ConfirmExitButtonRestart"
        );
    }

    /// Campaign shows712 one sidebar row below Leave6C9, from the retail
    /// resource rects; skirmish hides it and its area hits nothing.
    #[test]
    fn campaign_shows_restart_below_leave_and_skirmish_hides_it() {
        let fixture = fixture();
        let geometry: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/in_game_shell_geometry.json",
        ))
        .unwrap();
        let case = &geometry["cases"][0];
        let assets = &geometry["assets"][case["theme"].as_str().unwrap()];
        let art = |address: &str| {
            [
                assets[address]["width"].as_i64().unwrap() as i32,
                assets[address]["height"].as_i64().unwrap() as i32,
            ]
        };
        let shell = InGameShellLayout::new(
            800,
            600,
            super::super::in_game_shell::InGameShellSizes {
                background_small: art("00b0fad4"),
                background_medium: art("00b0fac8"),
                background_large: art("00b0fa50"),
                credits: art("00b0fb08"),
                top: art("00b0f9e0"),
                radar: art("00b0fa68"),
                side1: art("00b0fa70"),
                side2: art("00b0fafc"),
                side3: art("00b0fa8c"),
                addon: art("00b0fa48"),
                bottom_spacer: art("00b0fa3c"),
                left_cap: art("00b0fa90"),
                button_background: art("00b0faa8"),
                right_cap: art("00b0fabc"),
            },
        )
        .unwrap();
        let size = [125, 25];
        let campaign = abort_layout(800, 600, shell, size, AbortSecondary::Restart);
        let skirmish = abort_layout(800, 600, shell, size, AbortSecondary::Hidden);

        let leave = shell.button_rect(resource_rect(&fixture, 0x6C9), size);
        let restart = shell.button_rect(resource_rect(&fixture, 0x712), size);
        assert_eq!(campaign.leave, leave);
        assert_eq!(campaign.secondary, Some(restart));
        assert_eq!(restart.x, leave.x);
        assert_eq!(restart.y, leave.y + size[1]);
        assert_eq!(skirmish.secondary, None);
        let network = abort_layout(800, 600, shell, size, AbortSecondary::Observe);
        assert_eq!(network.secondary, campaign.secondary);

        let center = (restart.x + restart.w / 2, restart.y + restart.h / 2);
        assert_eq!(
            campaign.hit(center.0, center.1),
            Some(AbortButton::Secondary)
        );
        assert_eq!(skirmish.hit(center.0, center.1), None);
        assert_eq!(
            campaign.buttons().map(|(b, _)| b).collect::<Vec<_>>(),
            [
                AbortButton::Leave,
                AbortButton::Secondary,
                AbortButton::Resume
            ]
        );
        assert_eq!(
            skirmish.buttons().map(|(b, _)| b).collect::<Vec<_>>(),
            [AbortButton::Leave, AbortButton::Resume]
        );
    }
}
