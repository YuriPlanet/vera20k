//! LAN Cooperative across two App processes: the host uses the Soviet
//! campaign through the real chooser, both players take B6 Observe, the
//! lost mission opens BC6 on both, both press Continue, and the retried
//! mission loads over the same admission. Only user input is automated; the
//! campaign text, BC6's records and the next loading cross real UDP.
//!
//! The campaign-end variant starts on the saved last stage and wins it: at
//! native frame 150 both processes hold the simulation and flag every human
//! House to win (the map TAction's owner, the same mutation on both), BC6
//! opens with Quit only, both press it, and `0xBC7` opens. The guest then
//! presses its Continue back to the shell.
//!
//! The online variant plays the same chooser in a VERA room over the
//! deployed lobby and wins the first stage the same way. BC6's Continue
//! there leaves at once for the kept room, which comes back on the next
//! stage without `TXT_SAVE_CAMP_AVAILABLE` in either setup's chat; the host
//! starts it as a first start, and it runs over the relay.

use super::*;
use crate::app::diagnostics::shell_capture::movies::abort_return::AbortReturnInput;
use crate::app::match_runtime::sim_host::SimWrite;

/// Both players open the menu at this committed frame, so each B6 still
/// counts two live humans and offers Observe.
const OBSERVE_FRAME: u32 = 150;
/// Native frames the mission after BC6 must run.
const RETRY_FRAMES: u32 = 90;
/// Render frames BC6 stays open before Continue, as a player's reaction:
/// a peer still finishing the match reads round records as in-match
/// packets and drops them, as `0x0048D1E0` does.
const CONTINUE_DELAY: u32 = 60;
/// How long the kept room may take to seat both players on the next stage:
/// the partner's re-seat and the host's options cross the lobby's polls.
const ROOM_WAIT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoopPhase {
    OpenChooser,
    Chooser,
    ChooserReturn,
    Playing,
    Observing,
    AwaitResult,
    AwaitRoom,
    AwaitNext,
    Retry,
    AwaitCampaignEnd,
    AwaitShell,
    Done,
}

/// How the capture's mission ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CoopEnding {
    /// Both observe, the mission is lost and retried.
    Retry,
    /// The last stage is won and `0xBC7` opens; `continue_to_shell` presses
    /// its Continue.
    CampaignEnd { continue_to_shell: bool },
    /// Online: the first stage is won, BC6's Continue returns both players
    /// to the kept room on the next stage, and the host starts it there.
    Advance,
}

pub(super) struct CooperativeRun {
    phase: CoopPhase,
    /// The host takes Use Saved Campaign instead of the Soviet row.
    saved: bool,
    ending: CoopEnding,
    /// A WOL room over the deployed lobby instead of the LAN service.
    online: bool,
    campaign_end: Option<Value>,
    abort: Option<AbortReturnInput>,
    first_map: Option<String>,
    result: Option<Value>,
    /// The stage the kept room came back on.
    next_map: Option<String>,
    room_since: Option<Instant>,
    /// The mission after BC6: the retried one, or online the next stage.
    next_startup: Option<Value>,
    next_first_frame: u32,
    result_frame: Option<u32>,
}

impl CooperativeRun {
    pub(super) fn new(saved: bool, ending: CoopEnding, online: bool) -> Self {
        Self {
            phase: CoopPhase::OpenChooser,
            saved,
            ending,
            online,
            campaign_end: None,
            abort: None,
            first_map: None,
            result: None,
            next_map: None,
            room_since: None,
            next_startup: None,
            next_first_frame: 0,
            result_frame: None,
        }
    }

    pub(super) fn done(&self) -> bool {
        self.phase == CoopPhase::Done
    }

    /// Both processes flag every human House to win at the same frame.
    pub(super) fn flags_win(&self) -> bool {
        self.ending != CoopEnding::Retry
    }

    fn setup_settled(&self, state: &AppState, host: bool) -> bool {
        if self.online {
            super::online::online_setup_settled(state, host)
        } else {
            setup_settled(state, host)
        }
    }

    /// The host's BC: Choose Map, Cooperative, the Soviet row, Use Map. True
    /// once BC shows the campaign's first map.
    pub(super) fn prepare_host(
        &mut self,
        state: &mut AppState,
        rendered: PresentedShell,
        route: &mut Vec<Value>,
        frame: u32,
    ) -> Result<bool> {
        match self.phase {
            CoopPhase::OpenChooser
                if rendered == PresentedShell::NetworkSetup && self.setup_settled(state, true) =>
            {
                setup_button(state, crate::ui::multiplayer_setup::CHOOSE_MAP)?;
                self.phase = CoopPhase::Chooser;
            }
            CoopPhase::Chooser
                if rendered == PresentedShell::NetworkSetup
                    && MoviesCapture::slide_settled(state, ShellSlideKind::ChooseMap)
                    && state
                        .frontend
                        .skirmish_shell_state
                        .choose_map_modal
                        .as_ref()
                        .is_some_and(|modal| modal.statics.is_terminal()) =>
            {
                let labels = click_cooperative_mode(state)?;
                if self.saved {
                    let label =
                        crate::app::App::csf_label(state, "TXT_COOP_USE_SAVE", "TXT_COOP_USE_SAVE");
                    let row = labels
                        .iter()
                        .position(|row| *row == label)
                        .context("the saved-campaign row")?;
                    use_chooser_row(state, row)?;
                } else {
                    use_soviet_campaign_row(state, &labels)?;
                }
                route.push(json!({"render_frame":frame,"action":"co-op campaign row used","saved":self.saved,"rows":labels}));
                self.phase = CoopPhase::ChooserReturn;
            }
            CoopPhase::ChooserReturn
                if rendered == PresentedShell::NetworkSetup && self.setup_settled(state, true) =>
            {
                let launch = state.match_state.session.network_setup().context("BC")?;
                ensure!(
                    launch.mode.id == cooperative_mode(state)?.id,
                    "Use Map left Cooperative"
                );
                self.first_map = launch.selected_map_file.clone();
                route.push(
                    json!({"render_frame":frame,"action":"BC in Cooperative","map":self.first_map}),
                );
                self.phase = CoopPhase::Playing;
                return Ok(true);
            }
            _ => {}
        }
        Ok(false)
    }

    /// The guest learns the first map from the host's options.
    pub(super) fn match_loaded(&mut self, state: &AppState) {
        if self.phase == CoopPhase::OpenChooser {
            self.phase = CoopPhase::Playing;
        }
        if self.first_map.is_none() {
            self.first_map = state
                .match_state
                .session
                .network_setup()
                .and_then(|launch| launch.selected_map_file.clone());
        }
    }

    /// A winning variant holds the simulation at the win frame, so both
    /// processes flag the win before the same tick.
    pub(super) fn holds_simulation(&self, state: &AppState) -> bool {
        self.phase == CoopPhase::Playing
            && self.flags_win()
            && committed_frame(state).is_ok_and(|frame| frame >= OBSERVE_FRAME)
    }

    /// Online, BC6's Continue returns both players to the kept room. True
    /// once it seats both on Cooperative's stage after the won one. Retail's
    /// rematch `+0xAC` (`0x005C46E0`) seats them without a join, so the
    /// partner's return must not print `+0x94`'s saved-campaign notice.
    pub(super) fn room_returned(
        &mut self,
        state: &AppState,
        host: bool,
        route: &mut Vec<Value>,
        frame: u32,
    ) -> Result<bool> {
        if self.phase != CoopPhase::AwaitRoom || !self.setup_settled(state, host) {
            return Ok(false);
        }
        let launch = state
            .match_state
            .session
            .network_setup()
            .context("kept room")?;
        let next = launch.selected_map_file.clone();
        if launch.humans.len() == 2
            && launch.mode.id == cooperative_mode(state)?.id
            && next.is_some()
            && next != self.first_map
        {
            let notice =
                App::csf_label(state, "TXT_SAVE_CAMP_AVAILABLE", "TXT_SAVE_CAMP_AVAILABLE");
            let shown = state
                .frontend
                .multiplayer_setup
                .as_ref()
                .is_some_and(|setup| {
                    setup
                        .chat
                        .lines
                        .iter()
                        .any(|line| String::from_utf16_lossy(&line.text) == notice)
                });
            ensure!(
                !shown,
                "the kept room printed {notice:?} for the partner's return"
            );
            route.push(json!({"render_frame":frame,"action":"kept room back in Cooperative on the next stage","map":next}));
            self.next_map = next;
            self.phase = CoopPhase::AwaitNext;
            return Ok(true);
        }
        ensure!(
            self.room_since.get_or_insert_with(Instant::now).elapsed() < ROOM_WAIT,
            "the kept room did not come back on the next stage: mode {}, map {next:?}, {} players",
            launch.mode.id,
            launch.humans.len()
        );
        Ok(false)
    }

    /// Everything after the first mission loaded.
    pub(super) fn step(
        &mut self,
        state: &mut AppState,
        route: &mut Vec<Value>,
        frame: u32,
    ) -> Result<()> {
        match self.phase {
            CoopPhase::Playing => {
                if committed_frame(state)? < OBSERVE_FRAME {
                    return Ok(());
                }
                ensure!(
                    state
                        .match_state
                        .session
                        .network_setup()
                        .is_some_and(|launch| launch.mode.class
                            == crate::skirmish_modes::MpModeClass::Cooperative),
                    "the live match is not Cooperative"
                );
                if self.flags_win() {
                    let committed = committed_frame(state)?;
                    ensure!(
                        committed == OBSERVE_FRAME,
                        "the win frame was overshot: {committed}"
                    );
                    flag_humans_won(state)?;
                    route.push(json!({"render_frame":frame,"action":"every human House flagged to win","native_frame":committed}));
                    self.phase = CoopPhase::AwaitResult;
                    return Ok(());
                }
                self.abort = Some(AbortReturnInput::begin_network_observe(
                    state, route, frame,
                )?);
                self.phase = CoopPhase::Observing;
            }
            CoopPhase::Observing => {
                if self
                    .abort
                    .as_mut()
                    .context("Observe input")?
                    .after_present(state, route, frame)?
                {
                    route.push(json!({"render_frame":frame,"action":"Observe selected through B6; real DESTRUCT queued"}));
                    self.phase = CoopPhase::AwaitResult;
                }
            }
            CoopPhase::AwaitResult => {
                if !crate::app::input::coop_score::active(state) {
                    return Ok(());
                }
                let opened = *self.result_frame.get_or_insert(frame);
                if frame < opened + CONTINUE_DELAY {
                    return Ok(());
                }
                // Online, the relay transport ends with the mission.
                ensure!(
                    state.match_state.live_network.is_none()
                        && (self.online || state.match_state.lan_service.is_some()),
                    "BC6 kept the match's private channels"
                );
                let page = state
                    .frontend
                    .coop_score_page
                    .as_ref()
                    .context("BC6 page")?;
                let rows = page
                    .model
                    .rows
                    .iter()
                    .map(|row| row.name.clone())
                    .collect::<Vec<_>>();
                match self.ending {
                    CoopEnding::CampaignEnd { .. } => {
                        ensure!(
                            page.victory && !page.model.can_continue && page.campaign_end.is_some(),
                            "the last stage's win must offer only Quit and the campaign end"
                        );
                        self.result =
                            Some(json!({"render_frame":frame,"victory":page.victory,"rows":rows}));
                        press_score_button(state, crate::ui::coop_score_shell::QUIT)?;
                        route.push(json!({"render_frame":frame,"action":"BC6 opened after the last stage; Quit clicked"}));
                        self.phase = CoopPhase::AwaitCampaignEnd;
                    }
                    CoopEnding::Advance => {
                        ensure!(
                            page.victory && page.model.can_continue && page.campaign_end.is_none(),
                            "a won stage with stages left must offer Continue"
                        );
                        self.result =
                            Some(json!({"render_frame":frame,"victory":page.victory,"rows":rows}));
                        press_score_button(state, crate::ui::coop_score_shell::CONTINUE)?;
                        route.push(json!({"render_frame":frame,"action":"BC6 opened after the won stage; Continue clicked"}));
                        self.phase = CoopPhase::AwaitRoom;
                    }
                    CoopEnding::Retry => {
                        ensure!(
                            !page.victory && page.model.can_continue,
                            "both humans observing loses a mission that can be retried"
                        );
                        let next = state
                            .match_state
                            .session
                            .network_setup()
                            .and_then(|launch| launch.selected_map_file.clone());
                        ensure!(
                            next.is_some() && next == self.first_map,
                            "a defeat keeps the mission: {:?} -> {next:?}",
                            self.first_map
                        );
                        self.result = Some(json!({"render_frame":frame,"victory":page.victory,
                            "rows":rows,"next_map":next}));
                        press_score_button(state, crate::ui::coop_score_shell::CONTINUE)?;
                        ensure!(
                            state
                                .match_state
                                .session
                                .cooperative_round()
                                .is_some_and(|round| round.local_ready()),
                            "BC6 Continue did not mark the local player ready"
                        );
                        route.push(
                            json!({"render_frame":frame,"action":"BC6 opened; Continue clicked"}),
                        );
                        self.phase = CoopPhase::AwaitNext;
                    }
                }
            }
            CoopPhase::AwaitNext => {
                if state.frontend.screen != GameScreen::InGame {
                    return Ok(());
                }
                ensure!(
                    state.match_state.live_network.is_some(),
                    "the mission after BC6 has no real command transport"
                );
                let advance = self.ending == CoopEnding::Advance;
                if advance {
                    let loaded = state
                        .match_state
                        .session
                        .network_setup()
                        .and_then(|launch| launch.selected_map_file.clone());
                    ensure!(
                        loaded == self.next_map,
                        "the next stage loaded {loaded:?}, not the kept room's {:?}",
                        self.next_map
                    );
                }
                self.next_startup = Some(startup_receipt(state)?);
                self.next_first_frame = committed_frame(state)?;
                route.push(json!({"render_frame":frame,"action":if advance {
                    "the kept room's next stage loaded over the relay"
                } else {
                    "BC6 round over UDP; retried mission loaded"
                }}));
                self.phase = CoopPhase::Retry;
            }
            CoopPhase::AwaitCampaignEnd => {
                use crate::ui::campaign_score_shell::{CAMPAIGN_END_NAME, CampaignScoreDialog};
                let Some(page) = state.frontend.campaign_score_page.as_ref() else {
                    return Ok(());
                };
                ensure!(
                    page.dialog == CampaignScoreDialog::CampaignEnd,
                    "BC6 Quit opened another campaign score dialog"
                );
                if !MoviesCapture::slide_settled(state, ShellSlideKind::CampaignEnd)
                    || !page.reveals_terminal()
                {
                    return Ok(());
                }
                ensure!(
                    state.match_state.live_network.is_none()
                        && state.frontend.coop_score_page.is_none(),
                    "0xBC7 opened while BC6 or the match was still open"
                );
                self.campaign_end = Some(json!({"render_frame":frame,
                    "campaign":page.static_text(CAMPAIGN_END_NAME),
                    "statics":page.statics().iter().map(|text| json!({"id":format!("{:#x}",text.id),"text":text.text(),"rgb":text.rgb})).collect::<Vec<_>>(),
                    "cells":page.cells().iter().map(|cell| json!({"id":format!("{:#x}",cell.id),"text":cell.text,"rgb":cell.rgb})).collect::<Vec<_>>()}));
                if self.ending
                    != (CoopEnding::CampaignEnd {
                        continue_to_shell: true,
                    })
                {
                    route.push(json!({"render_frame":frame,"action":"0xBC7 settled"}));
                    self.phase = CoopPhase::Done;
                    return Ok(());
                }
                let page_layout = {
                    let (width, height) =
                        crate::app::frontend::shell_layout::shell_screen_size(state);
                    let fills = crate::app::frontend::shell_layout::fills_window(state);
                    page.layout(width, height, fills)
                };
                set_window_pointer_to_rect_center(state, page_layout.continue_button);
                App::handle_campaign_score_mouse_move(state);
                App::handle_campaign_score_mouse_down(state);
                App::handle_campaign_score_mouse_up(state);
                MoviesCapture::restore_neutral_pointer(state);
                ensure!(
                    crate::app::frontend::shell_transition::shell_exit_wave(
                        state,
                        ShellSlideKind::CampaignEnd,
                    )
                    .is_some(),
                    "0xBC7 Continue did not start its teardown slide"
                );
                route
                    .push(json!({"render_frame":frame,"action":"0xBC7 settled; Continue clicked"}));
                self.phase = CoopPhase::AwaitShell;
            }
            CoopPhase::AwaitShell => {
                if state.frontend.screen != GameScreen::MainMenu
                    || state.frontend.campaign_score_page.is_some()
                {
                    return Ok(());
                }
                route.push(json!({"render_frame":frame,"action":"0xBC7 closed; shell resumed"}));
                self.phase = CoopPhase::Done;
            }
            CoopPhase::Retry => {
                ensure!(
                    state.frontend.screen == GameScreen::InGame,
                    "the mission after BC6 left gameplay"
                );
                if committed_frame(state)? >= self.next_first_frame + RETRY_FRAMES {
                    route.push(json!({"render_frame":frame,"action":
                        if self.ending == CoopEnding::Advance { "next stage ran" } else { "retried mission ran" }}));
                    self.phase = CoopPhase::Done;
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn receipt(&self, state: &AppState) -> Result<Value> {
        ensure!(self.done(), "live co-op capture has not finished");
        if matches!(self.ending, CoopEnding::CampaignEnd { .. }) {
            return Ok(json!({"first_map":self.first_map,"result":self.result,
                "campaign_end":self.campaign_end,"screen":format!("{:?}",state.frontend.screen)}));
        }
        let sim = &state
            .match_state
            .sim_runtime
            .runtime()
            .context("simulation after BC6")?
            .simulation;
        let mut receipt = json!({"first_map":self.first_map,"result":self.result,
            "final_native_frame":sim.session().binary_frame(),
            "checksum_failure":format!("{:?}",sim.pending_network_failure())});
        if self.ending == CoopEnding::Advance {
            receipt["next_map"] = json!(self.next_map);
            receipt["next_startup"] = json!(self.next_startup);
            receipt["next_first_native_frame"] = json!(self.next_first_frame);
        } else {
            receipt["retry_startup"] = json!(self.next_startup);
            receipt["retry_first_native_frame"] = json!(self.next_first_frame);
        }
        Ok(receipt)
    }
}

/// Clicks one of BC6's buttons through its ordinary mouse input.
fn press_score_button(state: &mut AppState, control: u16) -> Result<()> {
    let rect = crate::app::input::coop_score::layout(state)
        .control_rect(control)
        .with_context(|| format!("BC6 control {control:#06x}"))?;
    set_window_pointer_to_rect_center(state, rect);
    crate::app::input::coop_score::cursor_moved(state);
    crate::app::input::coop_score::mouse(state, MouseButton::Left, true);
    crate::app::input::coop_score::mouse(state, MouseButton::Left, false);
    MoviesCapture::restore_neutral_pointer(state);
    Ok(())
}

fn committed_frame(state: &AppState) -> Result<u32> {
    Ok(state
        .match_state
        .sim_runtime
        .runtime()
        .context("live simulation")?
        .simulation
        .session()
        .binary_frame())
}

/// The map TAction's win (`HouseClass::Flag_To_Win` on every human House in
/// a network game) through the dev producer both processes run before the
/// same tick.
#[cfg(feature = "dev-overlays")]
fn flag_humans_won(state: &mut AppState) -> Result<()> {
    let runtime = state
        .match_state
        .sim_runtime
        .write_sim(SimWrite::Debug)
        .context("live simulation")?;
    ensure!(
        runtime.simulation.dev_trigger_current_house_outcome(
            &runtime.resources.rules,
            crate::sim::house_state::HouseOutcomeKind::Victory,
        ),
        "no human House took the win"
    );
    Ok(())
}

#[cfg(not(feature = "dev-overlays"))]
fn flag_humans_won(_: &mut AppState) -> Result<()> {
    bail!("the co-op campaign-end capture needs the dev-overlays build")
}
