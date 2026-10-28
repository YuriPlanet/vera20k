//! Match owner (F12 `MatchState`): the aggregate for everything scoped to a
//! single running match — the authoritative `SimRuntime`, the app-side input,
//! presentation, audio, and diagnostics owners, plus the scenario identity and
//! pacing facts that accompany them.
//!
//! App-side only: nothing here is serialized, hashed, or read by the
//! deterministic simulation except through `SimRuntime` itself.

use crate::map::basic::BasicSection;

pub(crate) struct MatchState {
    pub(crate) startup: super::startup::MatchStartup,
    /// Live session owner: gamemd's `SessionClass` at `0x00A8B238` (who
    /// holds `GameMode`, participants, local participant, host). Written by
    /// the shell, the launch path and the saved-game restore; read by the app
    /// loop only. Read `GameMode` through [`Self::session_mode`].
    pub(crate) session: super::session::MatchSession,
    /// The in-match connection wait and its dialog 0xEA
    /// (`Wait_For_Players 0x00648710`).
    pub(crate) network_wait: super::network_match::NetworkMatchWait,
    /// Input state for ExecuteDoList's blocking checksum-error message box.
    /// The suspended frame and mismatch remain owned by Simulation.
    pub(crate) network_failure: Option<super::network_failure::NetworkFailureModal>,
    /// Physical byte transport and packet progress for an installed match.
    /// Session owns the accepted participant roster and checksum history.
    pub(crate) live_network: Option<crate::net::live_commands::LiveCommandSession>,
    /// LAN visit transport before the match, then discovery metadata while the
    /// same sockets are owned by live_network. Session owns admitted endpoints.
    pub(crate) lan_service: Option<crate::app::lan_service::LanService>,
    pub(crate) sim_runtime: Option<crate::sim::runtime::SimRuntime>,
    /// Match input owner (F12): camera, zoom, cursor, keys, hotkeys.
    pub(crate) input: crate::app::input::state::MatchInputState,
    /// Match presentation owner (F12), part 1: per-match atlases + cursor.
    pub(crate) match_presentation: crate::app::presentation::state::MatchPresentationState,
    /// Per-match audio owner (F11): sound event queue + EVA latches; resets
    /// on every match install and on leaving a match for the shell.
    pub(crate) match_audio: crate::app::match_audio::MatchAudioState,
    /// App-owned diagnostic recording (F10) — never inside the simulation, so
    /// no load/install path can silently drop an unflushed segment.
    pub(crate) match_diagnostics: crate::app::match_diagnostics::MatchDiagnosticsState,
    /// Read-only process measurements; never inputs to the simulation.
    #[cfg(feature = "telemetry-dashboard")]
    pub(crate) telemetry_timing: super::telemetry::TickTimingRing,
    /// Whole-match 1 Hz timeline; recorded whether or not a dashboard client
    /// is attached, so a late client can backfill.
    #[cfg(feature = "telemetry-dashboard")]
    pub(crate) telemetry_history: super::telemetry::MatchHistory,
    /// Report-grade 5 s samples; each one renders the same report as the live
    /// view, including the time before a client attached.
    #[cfg(feature = "telemetry-dashboard")]
    pub(crate) report_history: super::telemetry::ReportHistory,
    #[cfg(feature = "telemetry-dashboard")]
    pub(crate) telemetry_tick_rate: super::telemetry::TickRateMeter,
    pub(crate) map_basic: BasicSection,
    /// Resolved Full_Init mission strings; retained independently of the
    /// current CSF and outside deterministic simulation state.
    pub(crate) campaign_names: crate::rules::campaign_loading::CampaignScenarioNames,
    /// Parsed `[Ranking]` values used by the campaign victory page.
    pub(crate) map_ranking: crate::map::ranking::RankingSection,
    /// Exact source whose bytes produced the active parsed map.
    pub(crate) loaded_map_source: Option<crate::map::source::LoadedMapSource>,
    /// Deterministic digest of the parsed source map INI. `None` only for
    /// generated/fallback worlds without an authoritative source-map payload.
    pub(crate) loaded_map_hash: Option<u64>,
    /// App-owned wall-clock House Vox drain. HouseState owns pending flags
    /// and SavourDelay; ScenarioSession retains the admitted result request.
    pub(crate) scenario_outcome:
        Option<crate::app::match_runtime::scenario_exit::ScenarioOutcomeVoiceWait>,
    /// Active running-scenario audio teardown. While present the tactical
    /// frame remains visible but simulation is frozen; its destination is
    /// committed only after the retail fade/voice-wait sequence completes.
    pub(crate) scenario_exit: Option<crate::app::match_runtime::scenario_exit::ScenarioExitCascade>,
    /// `g_bScenarioRestartRequested` `0x008B41C0`: B6 Restart raised it and
    /// the termination router consumes it before the next simulation frame.
    pub(crate) scenario_restart_requested: bool,
    /// Match elapsed wall time for the retail score screen. App-local and never
    /// serialized, hashed, or read by deterministic simulation.
    pub(crate) scenario_elapsed_clock: crate::app::match_runtime::frame_pacer::ScenarioElapsedClock,
    /// Config-sourced input delay — copied to each new Simulation instance at game start.
    pub(crate) configured_input_delay_ticks: u64,
    /// Explicit development HUD/command preference. Only consulted when the
    /// live scenario has no current House; ordinary matches derive their
    /// identity from serialized `Simulation.session.current_house`.
    pub(crate) local_owner_override: Option<String>,
    /// Seeded empty-map sandbox keeps full map visibility while still locking control.
    pub(crate) sandbox_full_visibility: bool,
    /// The debug pause (`J` hotkey): the simulation is frozen
    /// without an in-scenario modal. Every modal transition ends it.
    pub(crate) debug_pause: bool,
    /// Effective simulation ticks per second — controls game speed.
    /// Default follows retail/YR skirmish stored game speed 1.
    pub(crate) sim_speed_tps: u32,
}

impl MatchState {
    /// Shared ScenarioPause684060 audio hold, independent of menu/debug pause.
    /// Main_Tick55D82C also gates offline world admission on its counter.
    pub(crate) fn presentation_paused(&self, screen: &crate::ui::game_screen::GameScreen) -> bool {
        presentation_pause_for_screen(
            screen,
            self.paused(),
            self.sim_runtime.as_ref().map(|rt| rt.simulation.session()),
        )
    }
    /// Native elapsed-clock intent excludes the relay hold and network
    /// menus: those menus never stop the score clock. Audio still combines
    /// every physical pause through presentation_paused.
    pub(crate) fn elapsed_clock_paused(&self, screen: &crate::ui::game_screen::GameScreen) -> bool {
        elapsed_clock_pause_for_screen(
            screen,
            self.session_mode(),
            self.match_presentation.in_game_menu.is_open(),
            self.match_presentation
                .radar_anim
                .as_ref()
                .is_some_and(|radar| radar.movie_pause()),
            self.sim_runtime
                .as_ref()
                .is_some_and(|runtime| runtime.simulation.session().scenario_paused()),
        )
    }

    pub(crate) fn paused(&self) -> bool {
        self.local_paused() || self.multiplayer_paused()
    }

    pub(crate) fn local_paused(&self) -> bool {
        self.match_presentation.in_game_menu.is_open()
            || self.debug_pause
            || self
                .match_presentation
                .radar_anim
                .as_ref()
                .is_some_and(|radar| radar.movie_pause())
    }

    /// The relay's marker applies after the entire barrier frame, including a
    /// retained post-Logic transaction. This is a view of network authority,
    /// not an app-owned pause flag or a Scenario pause credit.
    pub(crate) fn multiplayer_paused(&self) -> bool {
        self.live_network
            .as_ref()
            .and_then(|live| live.multiplayer_pause())
            .zip(self.sim_runtime.as_ref())
            .is_some_and(|(pause, runtime)| {
                multiplayer_pause_reached(
                    pause,
                    runtime.simulation.session().binary_frame(),
                    runtime.simulation.pending_network_wait(),
                )
            })
    }

    pub(crate) fn sync_multiplayer_pause_clock(&mut self, wall_ms: u64) {
        let paused = self.multiplayer_paused();
        self.scenario_elapsed_clock
            .set_multiplayer_paused(paused, wall_ms);
    }

    /// Main_Tick (`0x0055D360`) is not running. A LAN/WOL modal's pump
    /// `0x00623120` keeps calling it in GameMode 3/4, so what Main_Tick
    /// drives keeps going under the dialog: `MessageListClass::Manage`
    /// (`0x005D4430`, its only caller) ages the rows. Debug pause remains a
    /// local diagnostic gate; native offline Scenario62C gates all its holds.
    pub(crate) fn main_tick_frozen(&self) -> bool {
        self.sim_runtime
            .as_ref()
            .is_some_and(|rt| rt.simulation.pending_network_failure().is_some())
            || self.debug_pause
            || self.multiplayer_paused()
            || super::scenario_pause::blocks_main_tick(
                self.session_mode(),
                self.sim_runtime
                    .as_ref()
                    .is_some_and(|rt| rt.simulation.session().scenario_paused()),
            )
            || (self.match_presentation.in_game_menu.is_open() && !self.session_mode().is_network())
    }

    /// `GameMode` (`0x00A8B238`) from whichever side holds it: the installed
    /// world's saved ScenarioSession from install until the shell writes it
    /// again, otherwise the shell (see `MatchSession`).
    pub(crate) fn session_mode(&self) -> super::session::SessionMode {
        held_session_mode(
            &self.session,
            self.sim_runtime.as_ref().map(|rt| rt.simulation.session()),
        )
    }

    /// The live scenario's pinned player, including a successfully restored save.
    pub(crate) fn local_player_owner(&self) -> Option<&str> {
        self.sim_runtime
            .as_ref()
            .and_then(|runtime| scenario_local_owner(&runtime.simulation))
    }
}

fn multiplayer_pause_reached(
    pause: crate::net::live_commands::MultiplayerPauseStatus,
    next_frame: u32,
    transaction_pending: bool,
) -> bool {
    pause.available && pause.paused && next_frame > pause.frame && !transaction_pending
}

fn elapsed_clock_pause_for_screen(
    screen: &crate::ui::game_screen::GameScreen,
    mode: super::session::SessionMode,
    menu_open: bool,
    movie_paused: bool,
    scenario_paused: bool,
) -> bool {
    matches!(screen, crate::ui::game_screen::GameScreen::InGame)
        && (movie_paused
            || (menu_open && !mode.is_network())
            || super::scenario_pause::blocks_main_tick(mode, scenario_paused))
}

/// A retained runtime can outlive its tactical screen. This is a physical
/// audio/presentation query; it never retires saved simulation pause credits.
fn presentation_pause_for_screen(
    screen: &crate::ui::game_screen::GameScreen,
    local_pause: bool,
    scenario: Option<&crate::sim::scenario_session::ScenarioSession>,
) -> bool {
    matches!(screen, crate::ui::game_screen::GameScreen::InGame)
        && (local_pause || scenario.is_some_and(|session| session.scenario_paused()))
}

fn held_session_mode(
    session: &super::session::MatchSession,
    world: Option<&crate::sim::scenario_session::ScenarioSession>,
) -> super::session::SessionMode {
    session.shell_mode().unwrap_or_else(|| {
        world
            .expect("only an installed world holds GameMode")
            .game_mode()
    })
}

#[cfg(test)]
mod tests {
    use super::{
        elapsed_clock_pause_for_screen, held_session_mode, multiplayer_pause_reached,
        presentation_pause_for_screen,
    };
    use crate::app::match_runtime::session::{MatchSession, NetworkReturnContext, SessionMode};
    use crate::sim::world::Simulation;
    use crate::ui::game_screen::GameScreen;

    #[test]
    fn shared_pause_waits_for_the_complete_barrier_and_survives_pending_resume() {
        let mut pause = crate::net::live_commands::MultiplayerPauseStatus {
            available: true,
            paused: true,
            frame: 42,
            epoch: 1,
            pending: false,
        };
        assert!(!multiplayer_pause_reached(pause, 42, false));
        assert!(!multiplayer_pause_reached(pause, 43, true));
        assert!(multiplayer_pause_reached(pause, 43, false));
        pause.pending = true;
        assert!(multiplayer_pause_reached(pause, 43, false));
        pause.paused = false;
        assert!(!multiplayer_pause_reached(pause, 43, false));
    }

    #[test]
    fn game_mode_reads_from_its_one_holder() {
        let mut session = MatchSession::default();
        let mut world = Simulation::with_seed(0);
        world.install_session_mode(SessionMode::Skirmish);
        // Process start: the shell holds 0 even beside a retained world.
        assert_eq!(held_session_mode(&session, None), SessionMode::Campaign);
        assert_eq!(
            held_session_mode(&session, Some(world.session())),
            SessionMode::Campaign
        );
        session.begin_offline(SessionMode::Skirmish, None);
        session.hand_mode_to_world(world.session().game_mode());
        assert_eq!(
            held_session_mode(&session, Some(world.session())),
            SessionMode::Skirmish
        );
        // A shell write after the match takes it back from the finished world.
        session.enter_network(NetworkReturnContext::Wol);
        assert_eq!(
            held_session_mode(&session, Some(world.session())),
            SessionMode::Wol
        );
    }

    #[test]
    fn retained_pause_does_not_leak_into_result_or_shell_audio() {
        assert!(elapsed_clock_pause_for_screen(
            &GameScreen::InGame,
            SessionMode::Skirmish,
            true,
            false,
            false
        ));
        assert!(!elapsed_clock_pause_for_screen(
            &GameScreen::InGame,
            SessionMode::Wol,
            true,
            false,
            true
        ));
        assert!(elapsed_clock_pause_for_screen(
            &GameScreen::InGame,
            SessionMode::Wol,
            true,
            true,
            false
        ));
        let mut sim = Simulation::with_seed(0);
        sim.publish_scenario_started();
        sim.pause_scenario_for_test(2);
        assert!(presentation_pause_for_screen(
            &GameScreen::InGame,
            false,
            Some(sim.session())
        ));
        // Score entry retains ScenarioStarted until Continue; it is not the
        // presentation-lifetime boundary. Shell return retains the runtime too.
        for screen in [
            GameScreen::MissionResult {
                title: String::new(),
                detail: String::new(),
            },
            GameScreen::MainMenu,
            GameScreen::Loading,
        ] {
            assert!(
                !presentation_pause_for_screen(&screen, false, Some(sim.session())),
                "{screen:?}"
            );
        }
        // A failed load returning to the current world must retain the hold.
        assert!(sim.session().scenario_pause_timed());
        assert!(presentation_pause_for_screen(
            &GameScreen::InGame,
            false,
            Some(sim.session())
        ));
        sim.expire_scenario_pause_for_test();
        assert!(!presentation_pause_for_screen(
            &GameScreen::InGame,
            false,
            Some(sim.session())
        ));
        assert!(presentation_pause_for_screen(
            &GameScreen::InGame,
            true,
            Some(sim.session())
        ));
    }
}

/// Native current House A83D4C is saved67F802, loaded67F9F3 and swizzled67FA0E.
/// Do not retain a separate app owner across Simulation replacement or infer it
/// from selection/human flags. Save validation owns reference validity.
/// Evidence: docs/research/PHASE3_CURRENT_HOUSE_IDENTITY_GHIDRA_REPORT.md.
pub(crate) fn scenario_local_owner(simulation: &crate::sim::world::Simulation) -> Option<&str> {
    simulation
        .session()
        .current_house()
        .map(|owner| simulation.interner().resolve(owner))
}
