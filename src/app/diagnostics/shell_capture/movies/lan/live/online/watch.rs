//! `online-live-watch` within LiveRoundFlow: a third account watches the
//! configured host's running match (docs/Networking.md "Watching"). Only the
//! ordinary lobby mouse adapter and the watcher's own keys
//! (`match_runtime::watching::key`) make inputs; the lobby's list poll, the
//! grant, the map, the relay's stream, its catch-up and the exit run as in an
//! interactive game. A desync or a lost relay stops the match with its usual
//! message, which `LiveLanCapture::after_present` fails on.

use super::*;
use crate::app::match_runtime::watching;

/// Frames the watcher must play once it has caught up.
const FRAMES_AFTER_CATCH_UP: u32 = REQUIRED_FRAMES;
/// The host signs in, makes its room and loads before its match is listed,
/// and the lobby asks for the list every 30 s.
const LIST_WAIT: Duration = Duration::from_secs(600);
/// How long, beyond the grant's delay, the watched world may run no frame.
const STALL: Duration = Duration::from_secs(60);
/// Escape leaves at the next frame's network wait pass.
const RETURN_WAIT: Duration = Duration::from_secs(60);
/// Tab's stops at most: 30 players and the observer's sidebar.
const TAB_STOPS: usize = 31;
/// The observer's sidebar among the views Tab shows; every other is a House.
const OBSERVER_VIEW: &str = "the observer's sidebar";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// The settled lobby, until its watch list shows the host's match.
    List,
    /// Join pressed: the grant, the map, relay admission and loading.
    Joining,
    /// The watched match, through its catch-up and the frames after it.
    Playing,
    /// Tab went through the views; Escape comes a presented frame later.
    Followed,
    /// Escape pressed: back to the online lobby.
    Returning,
}

pub(super) struct WatchRun {
    step: Step,
    since: Instant,
    match_id: Option<String>,
    delay_seconds: Option<u32>,
    first_frame: Option<u32>,
    catch_up_seen: bool,
    caught_up_frame: Option<u32>,
    /// The latest native frame and when the world reached it.
    last_frame: Option<(u32, Instant)>,
    frames_after: u32,
    /// The view before Tab and each it showed, back to that one.
    tab: Option<(String, Vec<String>)>,
    exit_route: Vec<Value>,
}

impl WatchRun {
    fn advance(&mut self, step: Step) {
        self.step = step;
        self.since = Instant::now();
    }
}

impl LiveRoundFlow {
    pub(super) fn watch_from_lobby(&mut self, frame: u32) {
        self.watch = Some(WatchRun {
            step: Step::List,
            since: Instant::now(),
            match_id: None,
            delay_seconds: None,
            first_frame: None,
            catch_up_seen: false,
            caught_up_frame: None,
            last_frame: None,
            frames_after: 0,
            tab: None,
            exit_route: Vec::new(),
        });
        self.record(
            frame,
            "Online lobby settled; its own poll refreshes the watch list",
        );
        self.phase = RoundPhase::Watch;
    }

    pub(super) fn watch_step(
        &mut self,
        state: &mut AppState,
        rendered: PresentedShell,
        frame: u32,
    ) -> Result<()> {
        let mut run = self.watch.take().context("online watch run")?;
        let result = match run.step {
            Step::List => self.join_listed_match(&mut run, state, rendered, frame),
            Step::Joining => self.await_watched_match(&mut run, state, frame),
            Step::Playing => self.play_watched_match(&mut run, state, frame),
            Step::Followed => self.leave_watched_match(&mut run, state, frame),
            Step::Returning => self.watch_returned(&mut run, state, rendered, frame),
        };
        self.watch = Some(run);
        result
    }

    /// Join the host's running match once the lobby's poll lists it.
    fn join_listed_match(
        &mut self,
        run: &mut WatchRun,
        state: &mut AppState,
        rendered: PresentedShell,
        frame: u32,
    ) -> Result<()> {
        if !lobby_ready(state, rendered) {
            return Ok(());
        }
        let host = self.online_config().host.clone();
        let lobby = state
            .frontend
            .wol
            .as_ref()
            .and_then(|visit| visit.lobby())
            .context("online lobby")?;
        ensure!(
            lobby.ui.is_modern(),
            "only the modern online lobby lists matches to watch"
        );
        let matches = crate::app::shell_wol::lobby::watch_matches(state);
        let listed = lobby
            .ui
            .game_rows
            .iter()
            .enumerate()
            .find_map(|(row, game)| {
                let watch = matches.get(game.record).filter(|_| game.watch)?;
                watch
                    .names
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&host))
                    .then(|| (row, watch.match_id.clone()))
            });
        let Some((row, match_id)) = listed else {
            ensure!(
                run.since.elapsed() < LIST_WAIT,
                "the watch list showed no running match of {host} within {} s",
                LIST_WAIT.as_secs()
            );
            return Ok(());
        };
        if !join_game_row(state, row)? {
            self.record(frame, "Game list scrolled toward the host's running match");
            return Ok(());
        }
        self.record(
            frame,
            &format!("Running match {match_id} of {host} joined from the watch list"),
        );
        run.match_id = Some(match_id);
        run.advance(Step::Joining);
        Ok(())
    }

    /// The grant installs the match's setup, its map arrives and the relay
    /// admits the watcher; a refusal or failure shows a lobby box instead.
    fn await_watched_match(
        &mut self,
        run: &mut WatchRun,
        state: &AppState,
        frame: u32,
    ) -> Result<()> {
        if let Some(message) = state
            .frontend
            .wol
            .as_ref()
            .and_then(|visit| visit.lobby())
            .and_then(|lobby| lobby.message())
        {
            bail!("the watch did not start: {}", message.body);
        }
        if state.frontend.screen != GameScreen::InGame {
            return Ok(());
        }
        let delay = state
            .match_state
            .live_network
            .as_ref()
            .filter(|live| live.watching())
            .and_then(|live| live.relay_watch_delay())
            .context("the watched match runs without a watcher's relay route")?;
        let native = native_frame(state)?;
        run.delay_seconds = Some(delay);
        run.first_frame = Some(native);
        run.last_frame = Some((native, Instant::now()));
        self.record(
            frame,
            &format!(
                "Watch admitted {delay} s behind the players; gameplay from native frame {native}"
            ),
        );
        run.advance(Step::Playing);
        Ok(())
    }

    /// Until the delayed present the stream's frames run at speed; after it
    /// the world must keep running, and then Tab goes through the views.
    fn play_watched_match(
        &mut self,
        run: &mut WatchRun,
        state: &mut AppState,
        frame: u32,
    ) -> Result<()> {
        ensure!(
            state.frontend.screen == GameScreen::InGame && watching::active(state),
            "the watched match ended {} frames after its catch-up, before {FRAMES_AFTER_CATCH_UP}",
            run.frames_after
        );
        let native = native_frame(state)?;
        let (last, at) = run.last_frame.context("watched frame")?;
        if native != last {
            run.last_frame = Some((native, Instant::now()));
        } else {
            let limit =
                Duration::from_secs(u64::from(run.delay_seconds.unwrap_or_default())) + STALL;
            ensure!(
                at.elapsed() < limit,
                "the watched match ran no frame after native frame {native} for {} s",
                limit.as_secs()
            );
        }
        let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32;
        if watching::catching_up(state, now_ms) {
            if !run.catch_up_seen || run.caught_up_frame.is_some() {
                self.record(frame, &format!("Catching up at native frame {native}"));
            }
            run.catch_up_seen = true;
            run.caught_up_frame = None;
            return Ok(());
        }
        let caught_up = match run.caught_up_frame {
            Some(caught_up) => caught_up,
            None => {
                self.record(
                    frame,
                    &format!(
                        "{} at native frame {native}",
                        if run.catch_up_seen {
                            "Caught up"
                        } else {
                            "At the delayed present without catching up"
                        }
                    ),
                );
                run.caught_up_frame = Some(native);
                native
            }
        };
        run.frames_after = native.saturating_sub(caught_up);
        if run.frames_after < FRAMES_AFTER_CATCH_UP {
            return Ok(());
        }
        // Tab goes through the other players and the observer's sidebar back
        // to the player it started with.
        let start = shown_view(state)?;
        let mut views: Vec<String> = Vec::new();
        while views.last() != Some(&start) {
            ensure!(
                views.len() < TAB_STOPS,
                "Tab never came back to {start}: {}",
                views.join(" -> ")
            );
            press(state, KeyCode::Tab)?;
            let view = shown_view(state)?;
            ensure!(
                views.last().unwrap_or(&start) != &view,
                "Tab kept showing {view}"
            );
            views.push(view);
        }
        ensure!(
            views.iter().any(|view| view == OBSERVER_VIEW),
            "Tab never showed the observer's sidebar: {}",
            views.join(" -> ")
        );
        ensure!(
            views
                .iter()
                .any(|view| view != &start && view != OBSERVER_VIEW),
            "Tab followed nobody else; the match needs two players still in it"
        );
        self.record(
            frame,
            &format!(
                "Tab went {start} -> {} at native frame {native}",
                views.join(" -> ")
            ),
        );
        run.tab = Some((start, views));
        run.advance(Step::Followed);
        Ok(())
    }

    fn leave_watched_match(
        &mut self,
        run: &mut WatchRun,
        state: &mut AppState,
        frame: u32,
    ) -> Result<()> {
        let native = native_frame(state)?;
        press(state, KeyCode::Escape)?;
        run.exit_route
            .push(json!({"step":"Escape","render_frame":frame,"native_frame":native}));
        self.record(frame, "Escape pressed in the watched match");
        run.advance(Step::Returning);
        Ok(())
    }

    /// Escape returns to the online lobby, which Back then leaves as every
    /// online capture does.
    fn watch_returned(
        &mut self,
        run: &mut WatchRun,
        state: &mut AppState,
        rendered: PresentedShell,
        frame: u32,
    ) -> Result<()> {
        if !lobby_ready(state, rendered) {
            ensure!(
                run.since.elapsed() < RETURN_WAIT,
                "Escape did not return to the online lobby within {} s",
                RETURN_WAIT.as_secs()
            );
            return Ok(());
        }
        ensure!(
            state.match_state.live_network.is_none()
                && state.match_state.session.network_setup().is_none(),
            "the online lobby came back with the watched match's transport or setup"
        );
        run.exit_route
            .push(json!({"step":"online lobby","render_frame":frame}));
        self.record(frame, "Escape returned to the online lobby");
        let rect = crate::app::shell_wol::lobby::layout(state)
            .page
            .button_rect(wol_lobby::BACK)
            .context("WOL lobby Back button")?;
        click(state, rect);
        self.record(
            frame,
            "Final settled lobby Back clicked through ordinary input",
        );
        self.phase = RoundPhase::ExitSignIn;
        Ok(())
    }

    pub(super) fn watch_receipt(&self, state: &AppState, run: &WatchRun) -> Result<Value> {
        let (start, views) = run
            .tab
            .as_ref()
            .context("online watch capture is incomplete")?;
        let menu = self
            .menu_return
            .as_ref()
            .context("online watch capture did not return to the settled Main Menu")?;
        ensure!(self.ready(), "online watch capture is incomplete");
        ensure!(
            state.match_state.live_network.is_none(),
            "finished watch capture retained command transport"
        );
        let mut exit_route = run.exit_route.clone();
        exit_route.push(json!({"step":"Main Menu","render_frame":menu["render_frame"]}));
        let config = self.online_config();
        Ok(
            json!({"target":"online-live-watch","account":config.account,"host":config.host,
            "service_boundary":"normal WOL UI, deployed HTTPS lobby and its watch grant, the relay's delayed stream to a watcher, production loading and frame boundary",
            "scope":"a third account finds the host's running match in the lobby's watch list, loads it, catches up to the delayed present, plays 300 frames more without a stop, goes through the players and the observer's sidebar with Tab and leaves with Escape to the lobby and the settled Main Menu; no result or cross-client checksum claim beyond the watcher's own check",
            "watch":{"match_id":run.match_id,"delay_seconds":run.delay_seconds,
                "first_native_frame":run.first_frame,"catch_up_seen":run.catch_up_seen,
                "caught_up_native_frame":run.caught_up_frame,
                "frames_after_catch_up":run.frames_after,
                "tab_start":start,"tab_views":views,
                "exit_route":exit_route},
            "returned_to_menu":true,"menu_return":menu,"route":self.route}),
        )
    }
}

fn native_frame(state: &AppState) -> Result<u32> {
    Ok(state
        .match_state
        .sim_runtime
        .runtime()
        .context("watched world")?
        .simulation
        .session()
        .binary_frame())
}

/// What the watcher shows: the observer's sidebar, or the House it stands in
/// for (`current_house`).
fn shown_view(state: &AppState) -> Result<String> {
    let sim = &state
        .match_state
        .sim_runtime
        .runtime()
        .context("watched world")?
        .simulation;
    if sim.session().watcher_observes() {
        return Ok(OBSERVER_VIEW.to_owned());
    }
    let house = sim
        .session()
        .current_house()
        .context("the watcher follows no House")?;
    Ok(sim
        .interner()
        .try_resolve(house)
        .context("followed House name")?
        .to_owned())
}

/// A press and release through the watcher's key adapter, as the window's
/// key events reach it in a watched match.
fn press(state: &mut AppState, code: KeyCode) -> Result<()> {
    ensure!(
        state.frontend.screen == GameScreen::InGame && watching::active(state),
        "{code:?} reached no watched match"
    );
    for pressed in [true, false] {
        ensure!(
            watching::key(state, code, pressed, false),
            "{code:?} is no watcher's key"
        );
    }
    Ok(())
}
