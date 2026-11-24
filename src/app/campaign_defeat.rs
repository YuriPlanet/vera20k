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
//! Saved campaigns use the retained Scenario index and incoming carry-over
//! through the same retry path as freshly started campaigns.
//! Message-box keyboard admission maps Enter's IDOK and Escape's IDCANCEL
//! through callback `0x005D36A0` to result 1 (the shell).

use super::{App, AppState};
use crate::app::match_bootstrap::PreparedCampaignStartup;

/// What a lost campaign mission does after its audio teardown.
#[derive(Debug)]
pub(crate) struct CampaignDefeat {
    /// The same mission's Start_Scenario, when VERA can rebuild it.
    retry: Option<Box<PreparedCampaignStartup>>,
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
        let csf = |key: &str, fallback: &str| state.csf_label(key, fallback);
        let shell_box = crate::ui::main_menu_dialogs::campaign_replay(&csf);
        Self::destroy_current_shell_dialog(state);
        state.frontend.shell_route = crate::app::shell_route::ShellRoute::MainMenu;
        Self::open_shell_message_box(
            state,
            shell_box,
            crate::app::frontend::state::ShellMessageBoxPurpose::CampaignReplay(defeat.retry),
        );
    }

    /// `0x006860EA test eax,eax; jz 0x006860F6`: only Yes (answer 0)
    /// restarts; No, Escape and Enter (1) return to the shell.
    pub(super) fn answer_campaign_replay(
        state: &mut AppState,
        retry: Option<Box<PreparedCampaignStartup>>,
        answer: usize,
    ) {
        match (answer == 0, retry) {
            (true, Some(startup)) => {
                log::info!("Campaign defeat: replaying {}", startup.scenario());
                Self::start_campaign_scenario(
                    state,
                    crate::app::loading::pump::LoadingRequest::campaign(*startup),
                );
            }
            (true, None) => {
                log::warn!("Campaign defeat: no campaign start to replay; returning to the shell");
                Self::resume_shell_after_campaign_defeat(state);
            }
            (false, _) => Self::resume_shell_after_campaign_defeat(state),
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
