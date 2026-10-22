//! Campaign defeat: the GameMode 0 tail of `GameExit::Defeat` `0x00685DC0`
//! once its audio teardown has run (the scenario exit cascade).
//!
//! Native order: GameMode 0 skips the score block entirely. Play_Movie
//! `0x005BF260` plays the Lose movie Scenario+0x1440 (`0x00686060..0x00686073`),
//! Shell__DrawEmptyBackdrop `0x0052FEC0` clears to the shell backdrop, and, with
//! replay bit 2 of `0x00A8D5F8` clear, ShellMessageBox__Run `0x005D3490` asks
//! `TXT_TO_REPLAY` with `TXT_YES`/`TXT_NO` (`0x00686092..0x006860E5`). Two
//! labels select template `0x120`. Result 0 (Yes) jumps to `0x006860F6`:
//! Start_Scenario(Scenario+0x125C, DL 0, Scenario+0x34CC) at `0x00686296`,
//! then the carry-over record `0x004C6210` (`0x0068639A`). Any other result
//! falls to `0x006860EE`; campaign's BL is 0, so `0x006863C3` clears
//! g_GameActive and the shell resumes.
//!
//! Residuals:
//! - VERA has no replay playback, so the bit-2 branch that skips the prompt is
//!   not modeled.
//! - The record restore's money and Scenario+0x35A0 are not
//!   carried (inert on retail data; see `sim::campaign_carry_over`).
//! Saved campaigns use the retained Scenario index and incoming carry-over
//! through the same retry path as freshly started campaigns.
//! Message-box keyboard admission maps Enter's IDOK and Escape's IDCANCEL
//! through callback `0x005D36A0` to result 1 (the shell).

use super::{App, AppState};
use crate::app::match_bootstrap::PreparedCampaignStartup;
use crate::ui::shell::modal::ModalResult;

/// What a lost campaign mission does after its audio teardown.
#[derive(Debug)]
pub(crate) struct CampaignDefeat {
    /// The same mission's Start_Scenario, when VERA can rebuild it.
    retry: Option<Box<PreparedCampaignStartup>>,
}

/// Where the `TXT_TO_REPLAY` answer goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReplayRoute {
    /// Yes (`0x5AE`, result 0): the same mission again.
    Restart,
    /// No (control 2 or IDOK, result 1): back to the shell.
    Shell,
}

/// `0x006860EA test eax,eax; jz 0x006860F6`: only result 0 restarts.
pub(crate) fn replay_route(result: ModalResult) -> ReplayRoute {
    if result == ModalResult::MessageBox(0) {
        ReplayRoute::Restart
    } else {
        ReplayRoute::Shell
    }
}

impl App {
    /// The defeat cascade has finished: read what the prompt needs while the
    /// lost Simulation is still resident, retire it, then play the Lose movie.
    pub(super) fn begin_campaign_defeat(state: &mut AppState) {
        let retry = Self::prepare_campaign_restart(state).map(Box::new);
        let movie = state
            .match_state
            .map_basic
            .lose
            .as_deref()
            .and_then(|name| {
                let movies = state.process_assets.native_rules()?.movies();
                movies.name(movies.find_index(name)).map(str::to_owned)
            });
        Self::close_finished_scenario(state);
        let defeat = CampaignDefeat { retry };
        match movie {
            Some(movie) => Self::start_fullscreen_movie(
                state,
                &movie,
                crate::app::frontend::fullscreen_movie::MovieReturn::CampaignDefeat(Box::new(
                    defeat,
                )),
            ),
            None => Self::open_campaign_replay_prompt(state, defeat),
        }
    }

    /// The Lose movie has returned: ask over the empty shell backdrop.
    pub(super) fn open_campaign_replay_prompt(state: &mut AppState, defeat: CampaignDefeat) {
        let csf = |key: &str, fallback: &str| Self::csf_label(state, key, fallback);
        let labels = crate::ui::main_menu_dialogs::ShellMessageBoxLabels::campaign_replay(&csf);
        Self::destroy_current_shell_dialog(state);
        state.frontend.shell_route = crate::app::shell_route::ShellRoute::MainMenu;
        Self::open_shell_message_box(
            state,
            crate::app::frontend::state::ShellMessageBox {
                labels,
                purpose: crate::app::frontend::state::ShellMessageBoxPurpose::CampaignReplay(
                    defeat.retry,
                ),
            },
        );
    }

    pub(super) fn answer_campaign_replay(
        state: &mut AppState,
        retry: Option<Box<PreparedCampaignStartup>>,
        route: ReplayRoute,
    ) {
        match (route, retry) {
            (ReplayRoute::Restart, Some(startup)) => {
                log::info!("Campaign defeat: replaying {}", startup.scenario());
                Self::start_campaign_scenario(
                    state,
                    crate::app::loading::pump::LoadingRequest::campaign(*startup),
                );
            }
            (ReplayRoute::Restart, None) => {
                log::warn!("Campaign defeat: no campaign start to replay; returning to the shell");
                Self::resume_shell_after_campaign_defeat(state);
            }
            (ReplayRoute::Shell, _) => Self::resume_shell_after_campaign_defeat(state),
        }
        state.platform.window.request_redraw();
    }

    /// `0x006863C3` clears g_GameActive; PrepareSession rebuilds the shell.
    fn resume_shell_after_campaign_defeat(state: &mut AppState) {
        Self::destroy_current_shell_dialog(state);
        state.frontend.shell_route = crate::app::shell_route::ShellRoute::MainMenu;
        Self::resume_shell_after_match(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::shell::modal::control;

    /// Yes sits in the OK slot (`0x5AE` -> 0) and restarts; No sits in
    /// Cancel's (control 2 -> 1) and, like Escape, returns to the shell.
    #[test]
    fn only_the_yes_slot_replays_the_mission() {
        let route = |control| replay_route(ModalResult::from_message_box_control(control).unwrap());
        assert_eq!(route(control::OK), ReplayRoute::Restart);
        assert_eq!(route(control::CANCEL), ReplayRoute::Shell);
        assert_eq!(
            replay_route(ModalResult::MessageBox(
                crate::ui::shell::modal::MESSAGE_BOX_DISMISSED
            )),
            ReplayRoute::Shell
        );
    }
}
