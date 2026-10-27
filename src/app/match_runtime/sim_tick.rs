//! In-game update phase — advances fixed-step simulation, triggers, path grids, and atlases.
//!
//! Camera control lives in `input::camera`. Building animations, damage fires, sidebar
//! UI tick, and sound playback live in `presentation::building_anim`.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

#[cfg(feature = "telemetry-dashboard")]
use crate::util::time::Instant;
use std::collections::HashSet;
use std::path::Path;

use crate::app::AppState;
use crate::app::input::commands::{preferred_local_owner, preferred_local_owner_name};
use crate::app::match_runtime::session::SessionMode;

use crate::app::types::SIM_TICK_MS;
use crate::map::terrain;
use crate::render::sprite_atlas;
use crate::render::unit_atlas;
use crate::sim::production;
use crate::sim::replay::ScenarioServiceInput;
use crate::sim::trigger_runtime::TriggerEffect;
use crate::sim::world::{FrameEffects, LifecycleOutput, SimFrameOutput, TickLane};
use crate::ui::game_screen::GameScreen;

/// Directory for Rust-only deterministic diagnostic logs.
const REPLAYS_DIR: &str = "replays";

/// Persist and consume the in-memory deterministic diagnostic log.
///
/// The log lives on the app-owned diagnostics owner (F10) and is appended
/// every recorded tick. Call this at every segment-closing lifecycle point —
/// match teardown, new match install, in-scenario load before commit, and app
/// exit — so every timeline leaves a rich command+hash trace for desync
/// diagnosis. This JSON artifact is separate from the fixed native recording
/// stream in `sim::replay`. No-op when nothing was recorded. A successful
/// write consumes the log so repeated teardown hooks do not duplicate it; any
/// failure restores it for a later retry. Writes
/// `replays/replay_tick{tick}_{unix_secs}.json`, taking an incrementing
/// `_n` suffix instead of replacing a segment already published under the
/// same `(tick, unix second)`.
pub(crate) fn flush_replay_log(state: &mut AppState) {
    let (session_tick, now) = replay_flush_facts(state);
    match state
        .match_state
        .match_diagnostics
        .flush_to(session_tick, Path::new(REPLAYS_DIR), now)
    {
        Ok(Some(flush)) => log::info!(
            "Deterministic diagnostic log flushed: {} ticks -> {}",
            flush.tick_count,
            flush.path.display()
        ),
        Ok(None) => {}
        Err(e) => log::error!("Diagnostic-log flush failed: {e}"),
    }
}

/// Timeline-boundary close (new match install, in-scenario load): a write
/// failure discards the segment instead of retaining it, so the next
/// timeline can never append under the previous timeline's header.
pub(crate) fn close_replay_segment_for_new_timeline(state: &mut AppState) {
    let (session_tick, now) = replay_flush_facts(state);
    match state
        .match_state
        .match_diagnostics
        .close_segment_for_new_timeline(session_tick, Path::new(REPLAYS_DIR), now)
    {
        Ok(Some(flush)) => log::info!(
            "Deterministic diagnostic log flushed: {} ticks -> {}",
            flush.tick_count,
            flush.path.display()
        ),
        Ok(None) => {}
        Err(e) => {
            log::error!("Diagnostic-log close failed at timeline boundary; segment discarded: {e}")
        }
    }
}

fn replay_flush_facts(state: &AppState) -> (u64, u64) {
    let session_tick = state
        .match_state
        .sim_runtime
        .as_ref()
        .map_or(0, |rt| rt.simulation.session().tick());
    let now = crate::util::time::SystemTime::now()
        .duration_since(crate::util::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    (session_tick, now)
}

/// Route an admitted native result after its in-House Vox wait and callbacks.
/// The pending wait is serviced through the suspended frame's client boundary.
pub(crate) fn drive_local_player_outcome_exit(state: &mut AppState, wall_ms: u64) {
    if !matches!(state.frontend.screen, GameScreen::InGame)
        || state.match_state.scenario_exit.is_some()
    {
        return;
    }
    let Some(owner) = state.match_state.local_player_owner() else {
        return;
    };
    let outcome = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
        .filter(|sim| sim.pending_outcome_wait().is_none())
        .and_then(|sim| {
            sim.interner()
                .get(owner)
                .and_then(|owner| sim.ready_outcome_for_owner(owner))
        });
    let Some(outcome) = outcome.map(|outcome| outcome.kind) else {
        return;
    };
    log::info!(
        "Match end ready for local player '{owner}': {}",
        crate::app::match_runtime::scenario_exit::outcome_title(outcome)
    );
    //685670/685DC0 clear5BF450 before either outcome fade.
    crate::app::radar_movie::retire(state, wall_ms);
    crate::app::chrono_screen::retire(state, wall_ms);
    state.frontend.finished_game_count = state.frontend.finished_game_count.saturating_add(1);
    let elapsed_score_ticks = state
        .match_state
        .scenario_elapsed_clock
        .stop_buckets(wall_ms);
    let elapsed_seconds = elapsed_score_ticks / 60;
    // 0x0055D0C5: the router runs the mode's +0xA0 before GameExit.
    let cooperative = crate::app::cooperative_round::applies(state);
    if cooperative {
        crate::app::cooperative_round::mission_ended(state, elapsed_score_ticks);
    }
    let title = crate::app::match_runtime::scenario_exit::outcome_title(outcome).to_string();
    let detail = crate::app::match_runtime::scenario_exit::outcome_detail(outcome).to_string();
    let destination = crate::app::match_runtime::scenario_exit::scenario_exit_destination(
        state.match_state.session_mode(),
        outcome,
        state.match_state.map_basic.skip_score.unwrap_or(false),
        cooperative,
        title,
        detail,
        || build_campaign_score_screen_model(state, elapsed_score_ticks),
        || build_score_screen_model(state, elapsed_seconds),
    );
    // Native campaign victories enter the standalone dialog `0x103`; campaign
    // defeats take a separate prompt path. Only GameMode!=0 takes the generic
    // `ScoreDialog__Run 0x5C9720` / SCORE-theme handoff.
    state.match_state.scenario_exit = Some(
        crate::app::match_runtime::scenario_exit::ScenarioExitCascade::start(wall_ms, destination),
    );
}

fn build_campaign_score_screen_model(
    state: &AppState,
    elapsed_score_ticks: i32,
) -> crate::ui::campaign_score_shell::CampaignScoreScreenModel {
    let difficulty = state
        .match_state
        .sim_runtime
        .as_ref()
        .and_then(|runtime| {
            runtime
                .simulation
                .session()
                .campaign_house_difficulties()
                // Retain the raw campaign selector index: GameExit copies
                // OptionsClass::Difficulty into Scenario+0x60C (0=Easy,
                // 1=Medium, 2=Hard) before the standalone score lookup.
                .map(|(player, _)| player as i32)
        })
        .unwrap_or(state.persistence.options_profile.difficulty);
    let houses = state.match_state.sim_runtime.as_ref().map(|runtime| {
        runtime
            .simulation
            .houses()
            .values()
            .map(|house| (house.player_control(), house.stats().kills()))
            .collect::<Vec<_>>()
    });
    let side = state
        .match_state
        .sim_runtime
        .as_ref()
        .and_then(|runtime| {
            runtime
                .simulation
                .session()
                .current_house()
                .and_then(|owner| runtime.simulation.houses().get(&owner))
                .map(|house| house.side_index())
        })
        .unwrap_or(0);
    let mut model = build_campaign_score_screen_model_from_sources(
        elapsed_score_ticks,
        difficulty,
        &state.match_state.map_ranking,
        houses.unwrap_or_default(),
        side,
        |key| resolve_campaign_score_label(state, key),
    );
    model.theme = state
        .process_assets
        .side_score_theme(
            side,
            crate::rules::ruleset::ScoreTheme::Campaign {
                over_par: model.over_par,
            },
        )
        .map(str::to_owned);
    model
}

fn build_campaign_score_screen_model_from_sources(
    elapsed_score_ticks: i32,
    difficulty: i32,
    ranking: &crate::map::ranking::RankingSection,
    houses: impl IntoIterator<Item = (bool, u32)>,
    side: i32,
    mut resolve_label: impl FnMut(&str) -> String,
) -> crate::ui::campaign_score_shell::CampaignScoreScreenModel {
    let mut model = crate::ui::campaign_score_shell::CampaignScoreScreenModel {
        side,
        ..Default::default()
    };
    let elapsed_score_seconds = elapsed_score_ticks / 60;
    model.your_time = format_campaign_score_time(elapsed_score_seconds);
    let par_score_ticks = ranking.par_time_for_difficulty(difficulty);
    let par_score_seconds = par_score_ticks / 60;
    model.par_time = format_campaign_score_time(par_score_seconds);
    // Native `0x6C9231..33` divides both Scenario+0x61C and ParTime by 60,
    // then selects the over-par strings only when elapsed is strictly greater.
    model.over_par = elapsed_score_seconds > par_score_seconds;
    let (rank_key, commendation_key) = if elapsed_score_seconds <= par_score_seconds {
        (&ranking.under_par_title, &ranking.under_par_message)
    } else {
        (&ranking.over_par_title, &ranking.over_par_message)
    };
    model.rank = resolve_label(rank_key);
    model.commendation = resolve_label(commendation_key);
    // Native 0x6C92C1 sums all four Killed*Types arrays for every HouseClass
    // with PlayerControl set. MatchStatistics is the existing per-house total
    // owner; `player_control` is the matching +0x1ED bit.
    let enemies_destroyed = houses
        .into_iter()
        .fold(0u32, |sum, (player_control, kills)| {
            if player_control {
                sum.wrapping_add(kills)
            } else {
                sum
            }
        });
    model.enemies_destroyed = enemies_destroyed.to_string();
    model
}

fn resolve_campaign_score_label(state: &AppState, native_key: &str) -> String {
    crate::app::frontend::shell_pass::resolve_csf(state, native_key).into_owned()
}

fn format_campaign_score_time(total_seconds: i32) -> String {
    let hours = total_seconds / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

#[cfg(test)]
mod campaign_score_model_tests {
    use super::build_campaign_score_screen_model_from_sources;
    use crate::assets::csf_file::test_csf;
    use crate::map::ranking::parse_ranking_section;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::house_state::MatchStatistics;

    #[test]
    fn campaign_score_model_uses_ranking_difficulty_native_buckets_csf_and_player_control_kills() {
        let ranking = parse_ranking_section(&IniFile::from_str(
            "[Ranking]\nParTimeEasy=00:00:01\nParTimeMedium=00:00:02\nParTimeHard=00:00:15\nUnderParTitle=GUI:FastRank\nUnderParMessage=GUI:FastMessage\nOverParTitle=GUI:SlowRank\nOverParMessage=GUI:SlowMessage\n",
        ));
        let csf = test_csf(&[
            ("GUI:FastRank", "Gold"),
            ("GUI:FastMessage", "Under par"),
            ("GUI:SlowRank", "Silver"),
            ("GUI:SlowMessage", "Over par"),
        ]);
        // Production supplies the existing per-house MatchStatistics::kills()
        // projection; these represent two PlayerControl houses plus an AI.
        let player_a = MatchStatistics::from_totals_for_test(3, 2, 0, 0, 0);
        let player_b = MatchStatistics::from_totals_for_test(4, 1, 0, 0, 0);
        let computer = MatchStatistics::from_totals_for_test(90, 10, 0, 0, 0);
        let houses = [
            (true, player_a.kills()),
            (true, player_b.kills()),
            (false, computer.kills()),
        ];
        let resolve = |key: &str| csf.text(key).into_owned();

        // Scenario+0x60C raw 2 selects ParTimeHard. Elapsed=959 and Par=900
        // both divide to the same native 60-tick bucket, so this is UnderPar.
        let equal_bucket =
            build_campaign_score_screen_model_from_sources(959, 2, &ranking, houses, 0, resolve);
        assert_eq!(equal_bucket.par_time, "00:00:15");
        assert_eq!(equal_bucket.your_time, "00:00:15");
        assert_eq!(equal_bucket.rank, "Gold");
        assert_eq!(equal_bucket.commendation, "Under par");
        assert_eq!(equal_bucket.enemies_destroyed, "10");

        let over_bucket =
            build_campaign_score_screen_model_from_sources(960, 2, &ranking, houses, 0, |key| {
                csf.text(key).into_owned()
            });
        assert_eq!(over_bucket.your_time, "00:00:16");
        assert_eq!(over_bucket.rank, "Silver");
        assert_eq!(over_bucket.commendation, "Over par");
        assert_eq!(over_bucket.enemies_destroyed, "10");
    }
}

/// The dialog's rows from the sim's snapshot, in its order (`0x005C9D7A`).
fn resolve_score_rows(
    sim: &crate::sim::world::Simulation,
    snapshot: &crate::sim::score::TerminalScoreSnapshot,
    sources: &crate::app::presentation::house_ui::HouseUiSources<'_>,
    country_name: impl Fn(crate::sim::intern::InternedId) -> Option<String>,
) -> Vec<crate::ui::score_shell::ScoreRow> {
    let mut rows: Vec<_> = snapshot
        .rows
        .iter()
        .map(|raw| crate::ui::score_shell::ScoreRow {
            name: sources.ui_name(sim, raw.owner, raw.country, &country_name),
            rgb: sources.text_rgb(sim, raw.owner),
            kills: raw.kills,
            losses: raw.losses,
            built: raw.built,
            score: raw.score,
        })
        .collect();
    crate::ui::score_shell::sort_rows(&mut rows);
    rows
}

/// The 0x108 title: the proc writes `GUI:SkirmishScore` for GameMode 5 and
/// `GUI:MultiplayerScore` otherwise (`0x005C9BDF..0x005C9C34`).
fn score_title_key(mode: SessionMode) -> &'static str {
    if mode == SessionMode::Skirmish {
        "GUI:SkirmishScore"
    } else {
        "GUI:MultiplayerScore"
    }
}

/// Resolve the end-of-match score presentation from a sim-owned raw snapshot.
///
/// Simulation owns contender admission, raw statistics, displayed-score bonus
/// calculation, and its Scenario RNG draws. This app helper resolves names,
/// colours, the local side, the title, elapsed wall time, and display order;
/// the traversal itself is `sim::score`'s.
fn build_score_screen_model(
    state: &AppState,
    elapsed_seconds: i32,
) -> crate::ui::score_shell::ScoreScreenModel {
    use crate::ui::score_shell::ScoreScreenModel;

    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return ScoreScreenModel::default();
    };
    let Some(raw_snapshot) = sim.terminal_score_snapshot() else {
        log::error!("Natural match exit reached the score screen without a sim snapshot");
        return ScoreScreenModel::default();
    };
    let computer = crate::app::frontend::shell_pass::resolve_csf(state, "TXT_COMPUTER");
    let rows = resolve_score_rows(
        sim,
        raw_snapshot,
        &crate::app::presentation::house_ui::HouseUiSources::from_state(state, &computer),
        |country| crate::app::presentation::house_ui::country_display_name(state, sim, country),
    );
    // Scenario +0x34B8: the side of session player 0's country, the local
    // player in a skirmish (0x0068779A..0x0068782D).
    let local_owner = crate::app::input::commands::preferred_local_owner_name(state)
        .as_deref()
        .and_then(|owner| sim.interner().get(owner));
    let local_house = local_owner.and_then(|owner| sim.houses().get(&owner));
    let side = local_house.map_or(0, |house| house.side_index());
    // `Global_DrawScoreScreen_ScoreTheme` (0x1002F4B0, patch site 0x5C9B75)
    // selects the side's win or lose theme on the local player's
    // HouseClass+0x1F5 defeated flag; the literal it otherwise pushes is
    // 0x0082FEBC "SCORE". It keeps that literal when the local player is the
    // Observer house (`Player [0xA83D4C] == Observer [0xAC1198]`,
    // 0x1002F4B8..0x1002F4C7).
    let observing = local_owner.is_some() && local_owner == sim.session().observer_house();
    let theme = (!observing)
        .then(|| {
            state.process_assets.side_score_theme(
                side,
                crate::rules::ruleset::ScoreTheme::Multiplayer {
                    defeated: local_house.is_some_and(|house| house.is_defeated()),
                },
            )
        })
        .flatten()
        .map(str::to_owned);

    ScoreScreenModel {
        title_key: score_title_key(state.match_state.session_mode()),
        game_number: state.frontend.finished_game_count,
        // The clock performs native's signed division first. The existing UI
        // model is unsigned and applies the native 99:59:59 ceiling, so keep
        // the pathological rollover representation local to this boundary.
        elapsed_seconds: elapsed_seconds as u32,
        side,
        theme,
        rows,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeAdvanceMode {
    WallClock { now_ms: u64 },
    ExactOneStep,
}

/// App-local evidence that one tactical diagnostic pump advanced exactly one
/// gameplay frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ExactStepReceipt {
    pub tick_before: u64,
    pub tick_after: u64,
    pub binary_frame_before: u32,
    pub binary_frame_after: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ExactStepError {
    #[error(
        "exact tactical step requires an accepted match, installed campaign or committed saved world"
    )]
    MissingAdmittedWorld,
    #[error("exact tactical step requires the InGame screen")]
    ScreenNotInGame,
    #[error("exact tactical step requires a live simulation")]
    SimulationMissing,
    #[error("exact tactical step advanced {actual} ticks instead of exactly one")]
    TickDelta { actual: u64 },
    #[error("exact tactical step advanced gameplay frame by {actual} instead of exactly one")]
    FrameDelta { actual: u32 },
}

/// One `AudioSystem::Pump @ 0x00406F70` service pass. It runs once per pass
/// on the shell, menu, wait and network paths, and at Main_Tick's own points
/// in a running offline match ([`main_tick_owns_audio_pump`]).
///
/// Native reaches the pump from `Network_ServiceLoop @ 0x0048D080`, not from
/// the game frame, so `SoundSystem::UpdateTick @ 0x004041D0` keeps reaping
/// finished events, enforcing `Limit=`, ranking and servicing the EVA queue
/// during a pause, an open menu, a modal dialog and a loading screen. The
/// `> 33 ms` gate lives inside `SfxPlayer::pump`, as it does in native. The
/// EVA *dequeue* is the one part a pause does stop: `VoxClass::PlayNextQueued
/// @ 0x00752780` gates its whole body on `DAT_00b1d428 == 0`, which
/// `VoxClass::PauseEVA @ 0x007535B0` raises.
///
/// Pause is the explicit `GamePause::Enter @ 0x00406F00` path: suspend every
/// event, stop the playing channels, and pause the EVA/speech stream.
///
/// `MatchState::paused` **is** VERA's in-game-menu state: it reads
/// `InGameMenuState::is_open()`, and apart from the `J` debug pause and the
/// fullscreen movie below no other pause reaches the SFX player. That is not
/// a divergence from gamemd; it is
/// exactly gamemd's pause. `State_Machine @ 0x0048C8B0` brackets its entire
/// dialog switch — case 5 `OptionsClass::ShowInGameDialog`, case 8
/// `Show_Diplomacy_Menu`, case 9 `ScenarioClass::ShowMissionRestateBriefing`
/// — between `StateMachine::EnterPause @ 0x00683EB0`, whose first statement
/// is `GamePause::Enter`, and `StateMachine::ExitPause @ 0x00683FB0`, whose
/// last is `GamePause::Exit`. `get_function_callers` on `0x00683EB0` returns
/// only `State_Machine`, and on `0x00406F00` only that path plus
/// `ScenarioPause::Enter @ 0x00684060` (the nuke-flash / in-game-movie /
/// trigger route, not a menu).
///
/// So opening the in-game menu in gamemd is a full game pause: the SFX
/// channels stop and the EVA stream is cut mid-word. Passing `paused`
/// straight into `set_paused` is the correct behaviour — do **not** add a
/// "menu open is not really a pause" carve-out here; that would be a
/// regression against the binary.
///
/// Movies are separate owners: every movie takes the hold of
/// `Audio__PauseForMovie @ 0x005BF580` (guard `0x00ABF35C`: EVA and the
/// stream players pause, the masters halve), and only full-screen
/// `Play_Movie @ 0x005BED40` also suspends the sound events (`0x00406EA0`,
/// resumed by `0x00406EC0`). The movie routes apply both through
/// `AppAudioRuntime`; the SFX player combines their reasons with this pause.
pub(crate) fn pump_audio_service(state: &mut AppState, now_ms: u64) {
    crate::app::presentation::building_anim::refresh_coordinate_sound_gains(state);
    // The process audio service remains live while the loading job holds the
    // asset-manager lease. Retained simulations still own Main on the shell;
    // only the process before its first scenario uses the frontend cursor.
    //Full-screen5BED40 acquires its own shared movie hold synchronously.
    //It does not also call the scenario/menu pause path684140.
    let paused = state
        .match_state
        .presentation_paused(&state.frontend.screen);
    let assets = crate::app::loading::pump::audio_service_asset_manager(
        &state.process_assets,
        state.frontend.loading_session.as_ref(),
    );
    let mut main = crate::app::state::process_main_draws(
        state.match_state.sim_runtime.as_mut(),
        &mut state.frontend.frontend_main_rng,
    );
    state.audio.service_audio(
        now_ms,
        paused,
        assets,
        state.process_assets.audio_catalog(),
        &mut main,
    );
}

/// The per-pass audio work outside the pump, for the passes whose pump belongs
/// to Main_Tick: the pause state, the master fades and the device queues.
/// Native applies the pause synchronously (`GamePause::Enter @ 0x00406F00` /
/// `Exit`) and ticks the master InterpGroups on its own audio thread
/// (`0x004095B0`), neither through the pump.
pub(crate) fn service_audio_outputs(state: &mut AppState, now_ms: u64) {
    let paused = state
        .match_state
        .presentation_paused(&state.frontend.screen);
    state.audio.service_audio_outputs(now_ms, paused);
}

/// Whether this pass's audio pump belongs to Main_Tick. In campaign and
/// skirmish (`g_GameMode` 0 and 5) `Main::ThrottleFrame @ 0x0055E160` only
/// sleeps, so `Network_ServiceLoop @ 0x0048D080`, and with it
/// `AudioSystem::Pump`, runs once per `Main_Tick @ 0x0055D360`: after Logic
/// (`0x0055DE4A`) or on the scenario-pause path (`0x0055D835`). An offline
/// window parked by focus loss (`0x0055D39B..0x0055D3AE`: `Sleep(500)` and
/// the Win32 message pump `0x005D4D50`) runs neither, so it is not pumped.
/// Network modes also pump in the throttle spin, and the in-game menu's
/// modal pump (`0x00623120`) and the exit and outcome waits pump from their
/// own loops, so those passes keep the per-pass pump.
pub(crate) fn main_tick_owns_audio_pump(state: &AppState, simulation_allowed: bool) -> bool {
    let match_state = &state.match_state;
    audio_pump_follows_main_tick(
        match_state.session_mode(),
        state.frontend.screen == GameScreen::InGame
            && simulation_allowed
            && match_state.startup.admits_ordinary_tick(),
        match_state.match_presentation.in_game_menu.is_open()
            || match_state.debug_pause
            || match_state.scenario_exit.is_some()
            || match_state.scenario_outcome.is_some(),
    )
}

/// [`main_tick_owns_audio_pump`]'s decision: an admitted match in
/// `g_GameMode` 0 or 5, on a pass no loop of its own pumps.
fn audio_pump_follows_main_tick(mode: SessionMode, match_admitted: bool, own_loop: bool) -> bool {
    mode.is_single_player() && match_admitted && !own_loop
}

/// Pure modal-pump decision: should the simulation advance one frame while a
/// modal dialog is open? Mirrors the pump body: advance only on the network
/// branch (LAN/WOL), only when neither service-only blocker is set, and only
/// when no fixed tick is already in progress. Offline campaign/skirmish freeze
/// the world; message, input, and repaint still run in the surrounding loop.
/// Pure and total, so it is unit-tested without an `AppState`. The live
/// app-layer consumer is `decide_runtime_pass`, which composes it with the
/// pacer timing answer and the remaining freeze predicates to gate the
/// one-frame admission inside `advance_in_game_runtime`.
pub fn modal_pump_should_advance_sim(
    mode: SessionMode,
    service_only_blocked: bool,
    reentrancy_in_progress: bool,
) -> bool {
    mode.is_network() && !service_only_blocked && !reentrancy_in_progress
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WallClockServiceAdmission {
    simulation: bool,
    tactical_mutation: bool,
}

/// Map the app's two pause sources onto their distinct production effects.
/// A real in-scenario modal follows the native service pump for both simulation
/// and tactical-view mutation. VERA's developer pause has no native modal, so
/// it stops simulation only and leaves ordinary per-render camera/placement
/// updates admitted.
///
/// gamemd-derived: modal service pump, verified `FUN_00623120 @ 0x00623120`.
fn wall_clock_service_admission(
    paused: bool,
    modal_open: bool,
    mode: SessionMode,
    service_only_blocked: bool,
    reentrancy_in_progress: bool,
) -> WallClockServiceAdmission {
    let tactical_mutation = !modal_open
        || modal_pump_should_advance_sim(mode, service_only_blocked, reentrancy_in_progress);
    WallClockServiceAdmission {
        simulation: if modal_open {
            tactical_mutation
        } else {
            !paused
        },
        tactical_mutation,
    }
}

/// Raw per-pass predicates for one in-game runtime advance. The wall-clock and
/// exact-step entries fill every field from live state, so the freeze matrix
/// can be exercised from the same predicates production consults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimePassInputs {
    pub(crate) exact_step: bool,
    /// The window owns the foreground; gates input polling.
    pub(crate) window_active: bool,
    /// The deactivated window freezes the world
    /// (`AppState::focus_freeze_active`).
    pub(crate) focus_frozen: bool,
    pub(crate) startup_admitted: bool,
    pub(crate) frame_stepping: bool,
    pub(crate) paused: bool,
    pub(crate) scenario_paused: bool,
    pub(crate) menu_open: bool,
    pub(crate) session_mode: SessionMode,
    /// The frame pacer's pure timing answer (`should_admit` with no pause
    /// block). Pause/menu service blocking is applied by the decision, not by
    /// the pacer consult.
    pub(crate) pacer_timing_admits: bool,
    /// `Wait_For_Players` is waiting for a peer's frames: no frame commits.
    pub(crate) network_wait: bool,
    /// Its dialog 0xEA is open: the wait loop stops servicing tactical input
    /// and drawing (`0x006497EC` needs `!bVar2`).
    pub(crate) network_dialog_open: bool,
}

/// The admission outcome of one runtime pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimePassDecision {
    pub(crate) run_sim: bool,
    pub(crate) tick_lane: TickLane,
    pub(crate) admitted_by_pacer: bool,
    pub(crate) tactical_mutation: bool,
    pub(crate) scroll_input: bool,
}

/// Decide one in-game runtime pass from raw predicates. This is the single
/// freeze contract for simulation admission and therefore displayed-credit
/// cadence: paused/menu redraws, focus-frozen windows, missing startup receipts,
/// and closed pacer windows all resolve to `run_sim: false`, and a committed
/// network-modal frame keeps its non-`Ordinary` lane. `sidebar_credit_gate_matrix`
/// pins the matrix through this function.
pub(crate) fn decide_runtime_pass(inputs: RuntimePassInputs) -> RuntimePassDecision {
    if inputs.exact_step && !inputs.network_wait {
        // Exact diagnostic stepping is an explicit request, not a wall-clock
        // modal pump: it advances exactly one Ordinary frame even while the
        // app is paused, and never records pacer admission.
        return RuntimePassDecision {
            run_sim: true,
            tick_lane: TickLane::Ordinary,
            admitted_by_pacer: false,
            tactical_mutation: true,
            scroll_input: true,
        };
    }
    // The session owner (`MatchSession`) carries no network service-only
    // blocker yet; the in-game network service of a later networking stage
    // supplies their combined state.
    let service = wall_clock_service_admission(
        inputs.paused,
        inputs.menu_open,
        inputs.session_mode,
        false,
        false,
    );
    let pacer_admitted = service.simulation && inputs.pacer_timing_admits;
    let run_sim = !inputs.focus_frozen
        && !super::scenario_pause::blocks_main_tick(inputs.session_mode, inputs.scenario_paused)
        && inputs.startup_admitted
        && !inputs.network_wait
        && (inputs.frame_stepping || pacer_admitted);
    let tick_lane = if inputs.paused && inputs.session_mode.is_network() {
        TickLane::NetworkModal
    } else {
        TickLane::Ordinary
    };
    RuntimePassDecision {
        run_sim,
        tick_lane,
        admitted_by_pacer: run_sim && !inputs.frame_stepping && pacer_admitted,
        tactical_mutation: service.tactical_mutation && !inputs.network_dialog_open,
        // Main_Tick polls GScreen input at 0x55D8AB. ThrottleFrame's offline
        // modes 0/5 skip its extra poll at 0x55E253, so a redraw during that
        // wait must neither scroll nor latch a right drag. Native executions:
        // tools/input_oracle/fast_scroll.json (throttle_cases).
        // Retain VERA's developer-pause camera and existing network policy;
        // no network session can be launched yet, so the latter is not parity.
        scroll_input: inputs.window_active
            && inputs.startup_admitted
            && service.tactical_mutation
            && !inputs.network_dialog_open
            && (run_sim
                || (inputs.paused && !inputs.menu_open)
                || inputs.session_mode.is_network()),
    }
}

/// `main_tick_pumps_audio` ([`main_tick_owns_audio_pump`]) runs the audio
/// service at Main_Tick's own points instead of once per pass.
pub(crate) fn advance_in_game_runtime(
    state: &mut AppState,
    now_ms: u64,
    main_tick_pumps_audio: bool,
) -> bool {
    let startup_admitted = state.match_state.startup.admits_ordinary_tick();
    if !startup_admitted {
        log::error!("Accepted match tick blocked: matching Rust L0 receipt is absent");
        // The whole pass is skipped: nothing below may run un-receipted. The
        // runtime decision re-checks the same predicate so the freeze contract
        // stays complete even without this call-site return.
        return false;
    }

    advance_in_game_runtime_mode(
        state,
        RuntimeAdvanceMode::WallClock { now_ms },
        startup_admitted,
        main_tick_pumps_audio,
    )
}

/// Advance exactly one production simulation step for the hidden tactical
/// checkpoint.
///
/// This is stricter than the ordinary app pump: sandbox admission is not
/// sufficient, the screen must be `InGame`, and the live simulation clock must
/// move by exactly one gameplay frame. The local pacer is re-anchored after
/// the step so a prior wall-clock interval cannot leak into normal admission.
pub(crate) fn advance_in_game_runtime_exact_step(
    state: &mut AppState,
) -> Result<ExactStepReceipt, ExactStepError> {
    let admitted = state.match_state.startup.admits_exact_step();
    if !admitted {
        return Err(ExactStepError::MissingAdmittedWorld);
    }
    if state.frontend.screen != GameScreen::InGame {
        return Err(ExactStepError::ScreenNotInGame);
    }
    let (tick_before, binary_frame_before) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
        .map(|sim| (sim.session().tick(), sim.session().binary_frame()))
        .ok_or(ExactStepError::SimulationMissing)?;

    advance_in_game_runtime_mode(state, RuntimeAdvanceMode::ExactOneStep, admitted, false);
    let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
    state.platform.frame_pacer.reanchor(now_ms);

    let (tick_after, binary_frame_after) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
        .map(|sim| (sim.session().tick(), sim.session().binary_frame()))
        .ok_or(ExactStepError::SimulationMissing)?;
    let receipt = ExactStepReceipt {
        tick_before,
        tick_after,
        binary_frame_before,
        binary_frame_after,
    };
    validate_exact_step_receipt(receipt)?;
    Ok(receipt)
}

fn validate_exact_step_receipt(receipt: ExactStepReceipt) -> Result<(), ExactStepError> {
    let tick_delta = receipt.tick_after.saturating_sub(receipt.tick_before);
    if tick_delta != 1 {
        return Err(ExactStepError::TickDelta { actual: tick_delta });
    }
    let frame_delta = receipt
        .binary_frame_after
        .wrapping_sub(receipt.binary_frame_before);
    if frame_delta != 1 {
        return Err(ExactStepError::FrameDelta {
            actual: frame_delta,
        });
    }
    Ok(())
}

fn advance_in_game_runtime_mode(
    state: &mut AppState,
    mode: RuntimeAdvanceMode,
    startup_admitted: bool,
    main_tick_pumps_audio: bool,
) -> bool {
    let replaying = state.match_state.match_diagnostics.is_replay_playback();
    // ExecuteDoList64CD0C is a blocking message box inside this frame. Even
    // exact-step diagnostics must not admit another Logic pass while it waits.
    if super::network_failure::pending(state) {
        super::network_failure::show(state);
        return false;
    }
    if crate::app::App::house_outcome_wait_pending(state) {
        // House4F8649/4F8772 services48D080 inside this Main_Tick;
        // it never re-enters55D7C2's timed-pause/Chrono/input prelude.
        return advance_one_simulation_frame(
            state,
            TickLane::Ordinary,
            None,
            FrameResume::OutcomeWait,
        );
    }
    crate::app::input::commands::reconcile_paid_super_weapon_cursor(state);
    let frame_stepping = matches!(mode, RuntimeAdvanceMode::WallClock { .. })
        && state.diag.debug_frame_step_requested;
    let resume_wait = state
        .match_state
        .sim_runtime
        .as_ref()
        .is_some_and(|rt| rt.simulation.pending_network_wait());
    let pacer_timing_admits = match mode {
        RuntimeAdvanceMode::WallClock { now_ms } => {
            let game_speed = state
                .match_state
                .sim_runtime
                .as_ref()
                .map(|rt| &rt.simulation)
                .map_or_else(
                    || {
                        state
                            .match_state
                            .match_presentation
                            .in_game_options
                            .game_speed
                            .min(6) as u8
                    },
                    |sim| sim.session().game_options().game_speed.clamp(0, 6) as u8,
                );
            // Pure timing consult; pause/menu blocking belongs to the decision.
            let network_fps = super::network_match::frame_budget(state);
            if let Some(live) = state
                .match_state
                .live_network
                .as_ref()
                .filter(|live| live.uses_sequenced_lockstep())
            {
                if live.has_terminal_network_error() {
                    true
                } else if let Some(runtime) = state.match_state.sim_runtime.as_ref() {
                    let frame = runtime.simulation.session().binary_frame();
                    let frontier = live.committed_frontier();
                    if let Some(timing) = live.sequenced_timing().filter(|_| {
                        !replaying
                            && !frame_stepping
                            && !state.match_state.paused()
                            && !state.focus_freeze_active()
                            && !runtime.simulation.session().scenario_paused()
                    }) {
                        let recovering = live.recovery_status(now_ms as u32, frame).is_some();
                        state
                            .platform
                            .frame_pacer
                            .online_frame_decision(
                                crate::util::time::Instant::now(),
                                frame,
                                frontier,
                                timing,
                                resume_wait,
                                recovering,
                            )
                            .admits()
                    } else {
                        super::frame_pacer::LocalFramePacer::admits_canonical_frame(frame, frontier)
                    }
                } else {
                    false
                }
            } else {
                state
                    .platform
                    .frame_pacer
                    .should_admit(now_ms, game_speed, network_fps, false)
            }
        }
        RuntimeAdvanceMode::ExactOneStep => false,
    };
    let wall_ms = match mode {
        RuntimeAdvanceMode::WallClock { now_ms } => now_ms,
        RuntimeAdvanceMode::ExactOneStep => crate::app::match_runtime::frame_pacer::wall_clock_ms(),
    };
    let service_live = matches!(mode, RuntimeAdvanceMode::WallClock { .. })
        && !replaying
        && !state.match_state.match_presentation.in_game_menu.is_open()
        && !state.match_state.debug_pause
        && !state.focus_freeze_active();
    if service_live {
        super::scenario_pause::poll(state, wall_ms);
    }
    let scenario_paused = state
        .match_state
        .sim_runtime
        .as_ref()
        .is_some_and(|rt| rt.simulation.session().scenario_paused());
    if service_live
        && super::scenario_pause::blocks_main_tick(
            state.match_state.session_mode(),
            scenario_paused,
        )
        && pacer_timing_admits
    {
        // Main_Tick55D830..55D859 services53B560 and returns without Logic.
        // A Finish here admits the world only on the following pass. Its
        // Network_ServiceLoop (`0x0055D835`) comes first.
        if main_tick_pumps_audio {
            pump_audio_service(state, wall_ms);
        }
        super::scenario_pause::apply_service(state, ScenarioServiceInput::StepChronoScreen);
        state.platform.frame_pacer.record_admitted_frame(wall_ms);
    }
    let mut decision = decide_runtime_pass(RuntimePassInputs {
        exact_step: matches!(mode, RuntimeAdvanceMode::ExactOneStep),
        window_active: state.platform.window_active,
        focus_frozen: state.focus_freeze_active(),
        startup_admitted,
        frame_stepping,
        paused: state.match_state.paused(),
        scenario_paused: scenario_paused && !replaying,
        menu_open: state.match_state.match_presentation.in_game_menu.is_open(),
        session_mode: state.match_state.session_mode(),
        pacer_timing_admits,
        network_wait: state.match_state.network_wait.waiting(),
        network_dialog_open: state.match_state.network_wait.dialog_open(),
    });
    if resume_wait {
        // Native Wait_For_Players is inside the already admitted MainTick.
        // Pacing and pause cannot re-admit that frame. Before0xEA opens,
        // Wait_For_Players6497F1..64984D still services tactical input; the
        // existing dialog gate alone suppresses that presentation work.
        decision.run_sim = !state
            .match_state
            .live_network
            .as_ref()
            .is_some_and(|live| live.uses_sequenced_lockstep())
            || pacer_timing_admits;
        decision.tick_lane = TickLane::Ordinary;
        decision.admitted_by_pacer = matches!(mode, RuntimeAdvanceMode::WallClock { .. });
    }
    if replaying {
        decision.run_sim &= state.match_state.match_diagnostics.playback_can_advance();
        decision.tactical_mutation = false;
        // Camera remains available when the last recorded frame is frozen.
        decision.scroll_input = state.platform.window_active && !state.focus_freeze_active();
    }
    if frame_stepping {
        state.diag.debug_frame_step_requested = false;
        if let RuntimeAdvanceMode::WallClock { now_ms } = mode {
            state.platform.frame_pacer.reanchor(now_ms);
        }
    }

    // Sample the gesture once, before simulation. Apply its request at the
    // native Tactical__AI/pre-follow seam, after this frame's bounds writers.
    let mouse_scroll = decision
        .scroll_input
        .then(|| crate::app::input::camera::poll_mouse_scroll(state));
    if decision.run_sim {
        let tick_lane = decision.tick_lane;
        let resume = if resume_wait {
            FrameResume::NetworkWait
        } else {
            FrameResume::None
        };
        if decision.admitted_by_pacer && !resume_wait {
            // MainTick arms its timer before Logic; time already spent in
            // Wait_For_Players must not restart that interval on completion.
            state.platform.frame_pacer.record_admitted_frame(wall_ms);
        }
        let frame_committed = advance_one_simulation_frame(state, tick_lane, mouse_scroll, resume);
        if crate::app::App::house_outcome_wait_pending(state) {
            return false;
        }
        if super::network_failure::pending(state) {
            super::network_failure::show(state);
            return false;
        }
        #[cfg(feature = "telemetry-dashboard")]
        if frame_committed && state.diag.telemetry_active() {
            state.match_state.telemetry_tick_rate.record(Instant::now());
        }
        crate::app::presentation::sidebar_render::advance_sidebar_credits_after_frame(
            state,
            frame_committed,
            tick_lane,
        );
        if frame_committed {
            // House4F92E2's strip pass reaches this frame as a pending
            // recheck; membership must not wait for the next player input.
            let recheck = state
                .match_state
                .match_presentation
                .sidebar_projection
                .cameo_recheck_pending();
            // Native Strip AI6A8CF3 requests redraw throughout the authored
            // flash window, even with no input. Reuse the one projection owner
            // only in these windows; ordinary idle ticks do not enumerate cameos.
            let flashing = state
                .match_state
                .sim_runtime
                .as_ref()
                .is_some_and(|runtime| {
                    state
                        .match_state
                        .match_presentation
                        .sidebar_projection
                        .cameo_redraw_requested(runtime.simulation.session().binary_frame() as i32)
                });
            // StripDraw6A9540 asks each entry's availability and reads its
            // Factory while drawing, so the drawn state follows the simulation
            // even when no pass, input or flash window rebuilt the projection
            // this frame.
            if recheck || flashing {
                // The rebuilt view takes the same drawn state.
                crate::app::presentation::sidebar_render::refresh_sidebar_projection(state);
            } else {
                crate::app::presentation::sidebar_render::refresh_sidebar_cameo_draw_state(state);
            }
            #[cfg(feature = "phobos")]
            crate::app::presentation::sw_sidebar::refresh_draw_state(state);
        }
    } else if let Some(request) = mouse_scroll {
        crate::app::input::camera::commit_camera_scroll(state, request);
    }

    // MainThrottle55E33B publishes Logic visits per60 wall-clock buckets,
    // including the path where the pacer admitted no simulation frame.
    let wall_ms = state.radar_presentation_ms();
    state
        .match_state
        .match_presentation
        .detail
        .borrow_mut()
        .throttle_tail(wall_ms);

    // Ordered native source/global operations were applied with the frame
    // output. Reconcile the detail option and any explicit tool mutations.
    refresh_cell_lighting(state);

    crate::app::presentation::building_anim::update_radar_state(state);
    crate::app::presentation::building_anim::update_power_bar_anim(state, wall_ms);
    crate::app::presentation::sidebar_gadgets::update_sidebar_gadget_state(state);
    // Per-frame gadget idle tick (G22 rows 2/3 drag-off/drag-back tracking).
    crate::app::input::gadget_input::idle_tick(state);
    // The audio service is NOT gated on the simulation. gamemd's
    // `AudioSystem::Pump @ 0x00406F70` hangs off `Network_ServiceLoop @
    // 0x0048D080`, whose callers include `Main_Tick` itself (offline, the
    // running match's: `main_tick_owns_audio_pump`), the network modes'
    // `Main::ThrottleFrame @ 0x0055E160`, `ProcessModalServicePump @
    // 0x00623120`, the House and GameExit waits and `ShellDialog::RunUntilResult
    // @ 0x0060D380`, so the sound arbiter, the EVA queue and `ThemeClass::AI`
    // keep running behind a pause, an open menu and a loading screen alike.
    // Pause is expressed separately, by suspending the events and stopping the
    // playing channels (`GamePause::Enter @ 0x00406F00`).
    // `ThemeClass::AI` itself runs from `pump_audio_service` on every screen;
    // only `Main_Tick @ 0x0055D360`'s scenario-running head rule (retained ==
    // -1 -> `Queue_Song(-2)`; `InGameMusic == 0` -> `Stop(1)` + `Queue(-3)`)
    // belongs to the in-game frame.
    let music_now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
    crate::app::presentation::building_anim::drain_sound_events(state);
    let in_game_music = state.persistence.options_profile.in_game_music;
    state.audio.main_tick_theme(in_game_music, music_now_ms);
    // Main_Tick's Network_ServiceLoop after Logic (`0x0055DE4A`). Logic's
    // sounds reach the arbiter in the drain above, so they start in this
    // same Main_Tick, as native's direct submissions do.
    if decision.run_sim && main_tick_pumps_audio {
        pump_audio_service(state, music_now_ms);
    }
    if decision.scroll_input {
        crate::app::input::camera::queue_keyboard_scroll(state);
    }
    if decision.tactical_mutation || replaying {
        crate::app::input::camera::animate_zoom(state);
    }
    if decision.tactical_mutation {
        update_building_placement_preview(state);
    }
    decision.run_sim
}

/// Tick simulation: advance movement and animation systems.
fn should_record_replay_tick(
    tick_result: &crate::sim::world::TickResult,
    due_commands: &[crate::sim::command::CommandEnvelope],
    network_failure: bool,
) -> bool {
    !network_failure
        && (tick_result.frame_committed
            || !due_commands.is_empty()
            || tick_result.terminal_score_finalized)
}

/// Complete the exact frame suspended by ExecuteDoList's message box. Reuse
/// every output consumer below; neither input admission nor Logic runs again.
pub(crate) fn resume_after_network_failure(state: &mut AppState) -> bool {
    advance_one_simulation_frame(state, TickLane::Ordinary, None, FrameResume::ChecksumModal)
}

pub(crate) fn leave_network_wait(state: &mut AppState) -> bool {
    advance_one_simulation_frame(state, TickLane::Ordinary, None, FrameResume::NetworkLeave)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameResume {
    None,
    ChecksumModal,
    NetworkWait,
    NetworkLeave,
    OutcomeWait,
}

fn advance_one_simulation_frame(
    state: &mut AppState,
    tick_lane: TickLane,
    mouse_scroll: Option<(f32, f32)>,
    resume: FrameResume,
) -> bool {
    let replaying = state.match_state.match_diagnostics.is_replay_playback();
    let resuming = resume != FrameResume::None;
    // MainTick55D3DA..55DE3A: a House outcome modal can suspend Logic before
    // Queue_AI. Keep its start with the live channel until that boundary, then
    // take it once; subsequent network-wait polls do not add samples.
    if !resuming && let Some(live) = state.match_state.live_network.as_mut() {
        live.begin_processing(super::frame_pacer::wall_clock_ms() as u32);
    }
    let mut refresh_atlases_after_tick = false;
    // Trigger definitions are runtime-bound (F07), so a live runtime is the
    // only activity source; the old trigger-only fixture mode is unrepresentable.
    let runtime_active = state.match_state.sim_runtime.is_some();
    if !runtime_active {
        return false;
    }
    // Input QueueVoice requests precede this Logic pass. This shared consumer
    // only admits queued requests; positional redrive/EVA service remain at
    // their existing owners, and voices wait for actual Techno-head markers.
    if !resuming {
        crate::app::presentation::building_anim::drain_pending_sound_events(state);
    }
    let mut frame_committed = state.match_state.sim_runtime.is_none();

    if let Some(rt) = state.match_state.sim_runtime.as_ref() {
        let _ = state.match_state.match_diagnostics.get_or_begin_replay(rt);
        state.match_state.match_diagnostics.retain_replay_startup(
            rt,
            state.match_state.session.network_setup(),
            state.match_state.loaded_map_source.as_ref(),
            state.match_state.loaded_map_hash,
            state
                .match_state
                .live_network
                .as_ref()
                .is_some_and(|live| live.dispatches_every_frame()),
        );
    }

    for _ in 0..1 {
        let unit_voice_owners = state
            .audio
            .sfx_player
            .as_ref()
            .map(|sfx| sfx.pending_unit_voice_owners())
            .unwrap_or_default();
        // Compute local owner before mutable borrow of simulation.
        let local_owner_for_fog = preferred_local_owner_name(state);
        // Cache local owner name before mutable sim borrow (avoids borrow conflict).
        let local_owner_name = crate::app::input::commands::preferred_local_owner_name(state);
        let sound_listener = crate::app::presentation::building_anim::sound_listener(state);
        let session_mode = state.match_state.session_mode();
        let logic_frame_rate = state
            .match_state
            .match_presentation
            .detail
            .borrow()
            .frame_rate();
        let other_local_pause = state.match_state.paused();
        let rules_owner = state.process_assets.native_rules();
        let initial_save = if !resuming && tick_lane == TickLane::Ordinary {
            crate::app::persistence::commands::prepare_initial_campaign_save(state, rules_owner)
        } else {
            None
        };
        let mut drained_lifecycle_outputs: Vec<LifecycleOutput> = Vec::new();
        let mut drained_combat_lights = Vec::new();
        let mut frame_overlay_updates = Vec::new();
        let mut frame_overlay_removals: Vec<(u16, u16)> = Vec::new();
        let mut trigger_effects: Vec<TriggerEffect> = Vec::new();
        let mut camera_logic_frame: Option<u32> = None;
        let mut hud_messages = Vec::new();
        // Carried out of the sim borrow so the census can read `state` freely below.
        let mut census_tick: Option<u64> = None;
        if let Some(rt) = state.match_state.sim_runtime.as_mut() {
            let sim = &mut rt.simulation;
            let native_frame = sim.session().binary_frame();
            if !resuming {
                sim.prepare_unit_voice_visits(unit_voice_owners);
            }
            // Normal play hashes its state once every PLAY_HASH_INTERVAL
            // frames and on the frame that ends the match, for the replay log.
            sim.hash_state_every(if replaying {
                std::num::NonZeroU64::MIN
            } else {
                crate::sim::replay::PLAY_HASH_INTERVAL
            });
            // Report points need the ledger whether or not a dashboard is
            // attached; `publish_if_due` only runs with a client.
            #[cfg(feature = "telemetry-dashboard")]
            sim.set_frame_profiling(true);
            // Delay-zero AnimClass construction can emit StartSound during the
            // final map-load sweep. Keep it until this first tactical drain;
            // `drain(..)` below still consumes every event exactly once.
            let due_commands = if !replaying && !resuming && tick_lane == TickLane::Ordinary {
                sim.take_due_commands()
            } else {
                Vec::new()
            };
            // Recorded whether or not a client is attached: the history ring
            // backfills a dashboard that connects mid-match.
            #[cfg(feature = "telemetry-dashboard")]
            let telemetry_advance_started = Instant::now();
            // F07: the runtime is the one production frame API — bound
            // resources, no caller-substitutable inputs.
            let mut output = {
                let (replay_playback, observations) = state
                    .match_state
                    .match_diagnostics
                    .playback_and_observations_mut();
                let command_observations = std::cell::RefCell::new(observations);
                let observing_commands = command_observations.borrow().is_some();
                let command_applied = |sim: &crate::sim::world::Simulation,
                                       record: &crate::sim::command::CommandRecord,
                                       applied: bool| {
                    if let Some(log) = command_observations.borrow_mut().as_deref_mut() {
                        log.record(sim, record, applied);
                    }
                };
                let sidebar = std::cell::RefCell::new(
                    &mut state.match_state.match_presentation.sidebar_projection,
                );
                #[cfg(feature = "phobos")]
                let sw_sidebar_expanded = state.match_state.match_presentation.sw_sidebar.expanded;
                let production_visits = sidebar.borrow().production_factory_visits(&rt.simulation);
                let clear_production_request = |type_id| {
                    sidebar.borrow_mut().clear_production_request(type_id);
                };
                let elapsed_clock =
                    std::cell::RefCell::new(&mut state.match_state.scenario_elapsed_clock);
                let initial_save = std::cell::RefCell::new((initial_save, &mut state.persistence));
                let before_logic = |sim: &crate::sim::world::Simulation| {
                    let mut save = initial_save.borrow_mut();
                    let (prepared, persistence) = &mut *save;
                    if let Some(Ok(prepared)) = prepared.as_mut() {
                        // Strip6A8F42 clears requests before MainTick55DBCD.
                        // Preparation preceded the input prefix; snapshot the
                        // same sidebar owner again at the actual save boundary.
                        let mut sidebar = sidebar.borrow_mut();
                        if let Some(owner) = sim.session().current_house() {
                            sidebar.reconcile_factory_links(sim, owner);
                        }
                        let order = sidebar.saved_order();
                        #[cfg(feature = "phobos")]
                        let order = order.map(|mut order| {
                            order.set_sw_sidebar_expanded(sw_sidebar_expanded);
                            order
                        });
                        prepared.refresh_sidebar_order(order);
                    }
                    crate::app::persistence::commands::initial_campaign_save(
                        prepared.take(),
                        sim,
                        persistence,
                        &mut elapsed_clock.borrow_mut(),
                    );
                };
                let playback = match (
                    state.audio.sfx_player.as_mut(),
                    state.process_assets.manager(),
                    state.process_assets.audio_catalog(),
                ) {
                    (Some(sfx), Some(assets), Some(catalog)) => Some((
                        sfx,
                        crate::app::presentation::building_anim::SoundPlaybackInputs {
                            assets,
                            catalog,
                            listener: sound_listener,
                        },
                    )),
                    _ => None,
                };
                let minimap = &mut state.match_state.match_presentation.minimap;
                let mut admit_radar = |request, tick, rules: &crate::rules::ruleset::RuleSet| {
                    minimap.as_mut().is_some_and(|minimap| {
                        minimap.admit_radar_event(request, tick, Some(rules))
                    })
                };
                let has_radar = &mut state.match_state.match_presentation.has_radar;
                let radar_anim = &mut state.match_state.match_presentation.radar_anim;
                let mut set_tactical_radar = |available| {
                    *has_radar = available;
                    radar_anim
                        .as_mut()
                        .and_then(|radar| radar.set_has_radar(available))
                };
                let boundary = super::frame_effects::FrameClientBoundary::new(
                    playback,
                    local_owner_name.as_deref(),
                    &mut state.match_state.match_audio.sound_events,
                    &mut admit_radar,
                )
                .with_tactical_radar(&mut set_tactical_radar)
                .with_planning(
                    &mut state.match_state.input.planning,
                    &mut state.match_state.input.selection_state,
                )
                .with_network_session(&mut state.match_state.session)
                .with_live_network(state.match_state.live_network.as_mut())
                .with_logic_frame_rate(logic_frame_rate)
                .with_scenario_pause(
                    &mut state.match_state.match_presentation.scenario_pause,
                    &elapsed_clock,
                    session_mode,
                    other_local_pause,
                )
                .with_outcome_wait(&mut state.match_state.scenario_outcome);
                let effects = FrameEffects::for_sound(&boundary)
                    .with_tactical_radar(&boundary)
                    .with_local_planning(&boundary)
                    .with_network_commands(&boundary)
                    .with_production_strip(&production_visits, &clear_production_request)
                    .with_before_logic(&before_logic);
                // Recorded completion owns outcome timing during playback;
                // a local audio wait cannot add a frame or change its hash.
                let effects = if replaying {
                    effects
                } else {
                    effects.with_outcome_wait(&boundary)
                };
                let effects = if observing_commands {
                    effects.with_command_applied(&command_applied)
                } else {
                    effects
                };
                if let Some(playback) = replay_playback {
                    match playback.advance(rt, effects) {
                        Ok(Some(output)) => output,
                        Ok(None) => return false,
                        Err(error) => {
                            let message = format!("Replay failed: {error:#}");
                            log::error!("{message}");
                            state.frontend.external_launch_error = Some(message);
                            return false;
                        }
                    }
                } else {
                    match resume {
                        FrameResume::ChecksumModal => rt.resume_after_network_failure(effects),
                        FrameResume::NetworkWait => rt.resume_network_wait(effects),
                        FrameResume::NetworkLeave => rt.leave_network_wait(effects),
                        FrameResume::OutcomeWait => rt.resume_outcome_wait(effects),
                        FrameResume::None => {
                            rt.advance_frame(&due_commands, SIM_TICK_MS, tick_lane, effects)
                        }
                    }
                    .expect("simulation frame failed; prior world mutations remain")
                }
            };
            state
                .match_state
                .match_presentation
                .screen_shake
                .apply_updates(&output.screen_shake_updates);
            if output.outcome_waiting {
                // The simulation retains this frame's non-sound facts until
                // the same House suffix finishes. Its sound prefix has already
                // reached the synchronous boundary above exactly once.
                return false;
            }
            #[cfg(feature = "telemetry-dashboard")]
            {
                let tick_ms = telemetry_advance_started.elapsed().as_secs_f32() * 1000.0;
                let tick = rt.simulation.session().tick();
                let entities = rt.simulation.entities().len();
                let houses = rt.simulation.houses().len();
                let batch = rt.simulation.path_batch_stats();
                state.match_state.telemetry_timing.push(tick_ms);
                state
                    .match_state
                    .telemetry_history
                    .record_tick(tick, tick_ms, entities, houses, batch);
                // A report point closes the profiling window, so it covers the
                // five seconds since the previous point rather than the whole
                // match.
                if crate::app::match_runtime::telemetry::ReportHistory::due(tick) {
                    let speed = rt
                        .simulation
                        .session()
                        .game_options()
                        .game_speed
                        .clamp(0, 6) as u8;
                    let mut timing = state.match_state.telemetry_timing.summarize(
                        state.platform.frame_pacer.budget_ms(
                            speed,
                            rt.simulation
                                .session()
                                .network_command_timing()
                                .map(|timing| {
                                    (
                                        timing.frames_per_second(),
                                        state.match_state.live_network.as_ref().map_or(0, |live| {
                                            live.pacing_delay_millis(
                                                rt.simulation
                                                    .session()
                                                    .network_max_ahead_frames()
                                                    .unwrap_or(0),
                                            )
                                        }),
                                    )
                                }),
                        ),
                    );
                    timing.fps = state.diag.frame_timer.fps();
                    timing.tick_hz = state
                        .match_state
                        .telemetry_tick_rate
                        .rate(crate::util::time::Instant::now());
                    let mut point = rt.simulation.report_point();
                    point.timing = Some(timing);
                    point.render = state.diag.render_timing.report();
                    if let Some(report) = point.render {
                        state.diag.render_report_cache = Some(report);
                    }
                    state.match_state.report_history.push(point);
                    rt.simulation.reset_frame_profile();
                    state.diag.render_timing = Default::default();
                }
            }
            let admitted_commands = output.take_admitted_commands();
            let SimFrameOutput {
                tick: tick_result,
                trigger_effects: frame_trigger_effects,
                lifecycle_outputs: frame_lifecycle_outputs,
                overlay_updates,
                overlay_removals,
                sound_events: frame_sound_events,
                sound_events_delivered,
                combat_lights,
                lighting_events,
                multiplayer_checksum,
                network_failure,
                network_waiting,
                outcome_waiting,
                ..
            } = output;
            // Queue_AI already published this checksum to the process session
            // before in-frame FRAMEINFO comparisons. Keep the output for other
            // consumers without recording it a second time here.
            let _ = multiplayer_checksum;
            camera_logic_frame = frame_lifecycle_outputs
                .iter()
                .any(|output| matches!(output, LifecycleOutput::LogicVisit))
                .then_some(native_frame);
            let resources = &rt.resources;
            if let Some(terrain) = resources.terrain_template.as_ref() {
                state
                    .match_state
                    .match_presentation
                    .lighting
                    .apply_events(terrain, &lighting_events);
            }
            let sim = &mut rt.simulation;
            trigger_effects = frame_trigger_effects;
            frame_overlay_updates = overlay_updates;
            frame_overlay_removals = overlay_removals;
            frame_committed = tick_result.frame_committed;
            if tick_result.frame_committed {
                drained_combat_lights =
                    crate::app::presentation::combat_lights::materialize_combat_lights(
                        combat_lights,
                        Some(&resources.rules),
                        sim.interner(),
                        state
                            .match_state
                            .match_presentation
                            .in_game_options
                            .detail_level,
                        &mut state.match_state.match_presentation.detail.borrow_mut(),
                    );
            }
            // Parity capture, if requested. The sim has already finalized all
            // authoritative animation and particle work, so this observes the
            // exact state covered by the returned frame hash.
            if tick_result.frame_committed
                && let Some(sink) = state.diag.parity_digest_sink.as_mut()
            {
                let digest = sim.parity_digest();
                if let Err(error) = sink.write(&digest) {
                    // A failing diagnostic must never take the game down with it.
                    log::error!("parity digest write failed, disabling capture: {error}");
                    state.diag.parity_digest_sink = None;
                }
            }
            census_tick = tick_result.frame_committed.then_some(tick_result.tick);
            drained_lifecycle_outputs = frame_lifecycle_outputs;
            // Mark the local owner's fog view prepared; render dirty-gates on its
            // generation.
            // F10: sim owns the write; the app only names the owner.
            if let Some(owner) = &local_owner_for_fog {
                if sim.session().tick() == 1 {
                    log::info!("Fog view prepared for local owner: '{}'", owner);
                }
                sim.prepare_fog_view_for(owner);
            }
            // The radar event array is the local client's (RadarClass). A
            // match without a minimap renderer has no such client, so nothing
            // is admitted and the radar-gated EVA lines stay silent.
            let radar_frame = sim.session().tick();
            super::super_selection::follow_selection_writes(
                &mut state.match_state.input.targeting_mode,
                &frame_sound_events,
                sim,
                &resources.rules,
                local_owner_name.as_deref(),
            );
            hud_messages = super::sound_dispatch::hud_messages(
                &frame_sound_events,
                sim.interner(),
                local_owner_name.as_deref(),
            );
            assert!(
                sound_events_delivered <= frame_sound_events.len(),
                "frame delivered more sound facts than it produced"
            );
            let minimap = &mut state.match_state.match_presentation.minimap;
            let mut admit_radar = |request: crate::sim::radar::RadarEventRequest| {
                minimap.as_mut().is_some_and(|minimap| {
                    minimap.admit_radar_event(request, radar_frame, Some(&resources.rules))
                })
            };
            super::sound_dispatch::dispatch_sim_sound_events(
                frame_sound_events.into_iter().skip(sound_events_delivered),
                sim,
                &resources.rules,
                local_owner_name.as_deref(),
                &mut admit_radar,
                &mut state.match_state.match_audio.sound_events,
            );
            super::super_selection::hide_placement_anims(
                &mut state.match_state.match_presentation.hidden_super_anims,
                sim,
                super::super_selection::chrono_warp_selected(
                    state.match_state.input.targeting_mode.as_ref(),
                    &resources.rules,
                ),
                local_owner_name.as_deref(),
                &mut state.match_state.match_audio.sound_events,
            );
            if tick_result.destroyed_structure {
                refresh_atlases_after_tick = true;
            }
            if tick_result.bridge_state_changed {
                refresh_atlases_after_tick = true;
            }
            if tick_result.ownership_changed {
                refresh_atlases_after_tick = true;
            }
            if tick_result.spawned_entities {
                refresh_atlases_after_tick = true;
            }
            // Both terminal routes belong to the deterministic stream even
            // though Main_Tick skips its frame commit. EventClass EXIT carries
            // a command; natural win/loss carries the one-shot score/RNG latch.
            if should_record_replay_tick(
                &tick_result,
                &admitted_commands,
                network_failure.is_some() || network_waiting || outcome_waiting,
            ) && let Some(log) = &mut state.match_state.match_diagnostics.replay_log
            {
                log.record_tick(tick_result.tick, admitted_commands, tick_result.state_hash);
            }
        }
        if frame_committed {
            state
                .match_state
                .match_presentation
                .combat_lights
                .commit_frame(drained_combat_lights);
        }
        crate::app::input::dispatch::reconcile_selection_order_after_sim(
            state,
            &drained_lifecycle_outputs,
        );
        // Authored Team/trigger Focus and lock effects precede TacticalAI in this Logic visit.
        // Coordinates were resolved by simulation before its final hash.
        apply_trigger_effects(state, &trigger_effects);
        // Scroll_Map (0x4A9840 -> 0x6D8530) accumulates mouse requests;
        // Tactical__AI commits/clamps them at 0x6D26F9..0x6D2770, before
        // follow's SetView (0x55B6F3) overwrites both requested/current view.
        // Applying here also uses any LocalSize changes from this frame.
        if let Some(frame) = camera_logic_frame {
            crate::app::input::camera::advance_camera_ai(
                state,
                mouse_scroll.unwrap_or((0.0, 0.0)),
                frame,
            );
        } else if let Some(request) = mouse_scroll {
            crate::app::input::camera::commit_camera_scroll(state, request);
        }
        // Native drives the follow camera from the tail of
        // LogicClass__PerTickUpdate 0x0055B6B8, after every object has updated,
        // so the view lands on this tick's position rather than the last one's.
        crate::app::input::camera::update_follow_camera(state, &drained_lifecycle_outputs);
        // Rendering is rebuilt from lifecycle facts every frame. Replay the
        // app-owned transactions in native emission order for state that has a
        // direct attachment or retained audio handle.
        let local_house = preferred_local_owner_name(state).and_then(|owner| {
            state
                .match_state
                .sim_runtime
                .as_ref()?
                .simulation
                .interner()
                .get(&owner)
        });
        // House4F8B08..4F8B2B: this frame's House rung is the native caller.
        let placement_armed = state.armed_building_type().is_some();
        if let Some(rt) = state.match_state.sim_runtime.as_ref() {
            crate::app::presentation::sidebar_gadgets::exit_sell_mode_without_buildings(
                &mut state.match_state.match_presentation.sidebar_gadget_state,
                &rt.simulation,
                local_house,
                placement_armed,
            );
        }
        for output in drained_lifecycle_outputs {
            if let Some(cursor_clear) = state
                .match_state
                .match_presentation
                .sidebar_projection
                .apply_lifecycle_output(&output, local_house)
            {
                state
                    .match_state
                    .input
                    .apply_lifecycle_output(&cursor_clear, local_house);
            }
            if state
                .match_state
                .input
                .apply_lifecycle_output(&output, local_house)
            {
                update_building_placement_preview(state);
            }
            match output {
                LifecycleOutput::OptionsRequested => {
                    crate::app::App::enter_in_game_menu_state(
                        state,
                        crate::ui::pause_menu::InGameMenuState::Menu,
                    );
                }
                LifecycleOutput::LatencyFudgeChanged { value, .. } => {
                    state.match_state.session.set_latency_fudge(value);
                }
                LifecycleOutput::PlanningNodeDeleted {
                    owner,
                    node_id,
                    next_node,
                } => {
                    state.match_state.input.planning.node_deleted(
                        node_id,
                        if Some(owner) == local_house {
                            next_node
                        } else {
                            None
                        },
                    );
                }
                LifecycleOutput::SidebarConstructionOption { .. }
                | LifecycleOutput::SidebarRecalculated { .. }
                | LifecycleOutput::ProductionBeginAttempted { .. }
                | LifecycleOutput::ProductionFactoryLinked { .. }
                | LifecycleOutput::ProductionFactoryAbandoning { .. }
                | LifecycleOutput::ProductionFactoryUnlinked { .. }
                | LifecycleOutput::BuildingPlacementRetry { .. }
                | LifecycleOutput::BuildingPlacementSucceeded { .. }
                | LifecycleOutput::BuildingProductionAbandoned { .. }
                | LifecycleOutput::FollowReleased { .. }
                | LifecycleOutput::ObjectSelected { .. } => {}
                LifecycleOutput::ObjectPointerExpired { stable_id } => {
                    state
                        .match_state
                        .input
                        .category_navigation
                        .pointer_expired(stable_id);
                }
                LifecycleOutput::LogicVisit => {
                    state
                        .match_state
                        .match_presentation
                        .detail
                        .borrow_mut()
                        .record_logic_visit();
                }
                LifecycleOutput::LaserUpdate { frame } => {
                    state.match_state.match_presentation.lasers.update(frame);
                }
                LifecycleOutput::LaserCreated(birth) => {
                    let runtime = state.match_state.sim_runtime.as_ref();
                    let presentation = &mut state.match_state.match_presentation;
                    let colors = &presentation.house_color_map;
                    presentation.lasers.create(birth, |owner| {
                        let Some(runtime) = runtime else {
                            return [0; 3];
                        };
                        let name = runtime.simulation.interner().resolve(owner);
                        // CreateHouses initializes the two literal special
                        // houses without ComputeRemap; their +56FC stays0.
                        if runtime.simulation.session().game_mode_nonzero()
                            && (name.eq_ignore_ascii_case("Neutral")
                                || name.eq_ignore_ascii_case("Special"))
                        {
                            return [0; 3];
                        }
                        let index = colors.get(name).copied().or_else(|| {
                            colors
                                .iter()
                                .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
                                .map(|(_, &color)| color)
                        });
                        index.map_or([0; 3], |index| {
                            crate::render::palette_light::house_laser_rgb(
                                &runtime.resources.rules.house_color_ramps,
                                index,
                            )
                        })
                    });
                }
                LifecycleOutput::ElectricBoltCreated(birth) => {
                    state
                        .match_state
                        .match_presentation
                        .electric_bolts
                        .create(birth);
                }
                LifecycleOutput::RadiationBeamCreated(birth) => {
                    state
                        .match_state
                        .match_presentation
                        .radiation_beams
                        .create(birth);
                }
                LifecycleOutput::LineTrailConstructed { stable_id, style } => {
                    let presentation = &mut state.match_state.match_presentation;
                    presentation.line_trails.attach(
                        stable_id,
                        style.color,
                        style.decrement,
                        presentation.in_game_options.detail_level as i32,
                    );
                }
                LifecycleOutput::LineTrailDetached { stable_id } => {
                    state
                        .match_state
                        .match_presentation
                        .line_trails
                        .detach(stable_id);
                }
                // Attached anims are simulation objects; the store detaches
                // them itself.
                LifecycleOutput::DetachAttachedAnims { .. } => {}
                // Unlimbo's Display admission can introduce a model without
                // TickResult::spawned_entities (e.g. Action7's ALL01 ORCA
                // teams). Resolve it before their first ordinary body draw;
                // crash-pose rendering must not be the first path to load it.
                // In-place conversion keeps its ID and Display membership,
                // so its new model needs the same coverage/growth check.
                LifecycleOutput::RevealDisplay { .. }
                | LifecycleOutput::DisplayRemove { .. }
                | LifecycleOutput::ObjectTypeChanged { .. } => {
                    refresh_atlases_after_tick = true;
                }
                LifecycleOutput::DirtyTacticalRect { .. }
                | LifecycleOutput::ClearDrawnState { .. }
                | LifecycleOutput::ClearRedraw { .. } => {}
            }
        }
        // Black-cell census, on a schedule rather than a keypress: nobody hits a debug key
        // at the exact moment they notice the artifact. Spread-out ticks give a time series,
        // which separates "still exploring" from "these cells will never be revealed" — a
        // shrinking count is normal, a count that plateaus with a stubborn remainder is the
        // bug. Runs after the sim borrow ends so it can read the atlas and terrain grid.
        // Early sample, then once a minute for as long as the match runs. A fixed set of
        // early ticks missed the symptom entirely: the interesting state is a mostly-explored
        // map with holes left in it, which only appears well into a session.
        if census_tick.is_some_and(|tick| tick == 150 || (tick > 150 && tick % 900 == 0)) {
            crate::app::input::dispatch::report_black_cell_causes(state);
        }

        for message in hud_messages {
            // MPlayer_Defeated's local branch deletes the player's beacons
            // and leaves beacon mode (0x004FC1C6, 0x004FC1FA), then sets the
            // chat mask (0x004FC23E) before it posts its line (0x004FC271).
            if let crate::app::input::messages::HudMessage::PlayerDefeated(house) = message
                && state
                    .match_state
                    .sim_runtime
                    .as_ref()
                    .and_then(|runtime| runtime.simulation.session().current_house())
                    == Some(house)
            {
                super::beacons::local_player_defeated(state);
                state
                    .match_state
                    .match_presentation
                    .chat
                    .local_player_defeated();
            }
            crate::app::input::messages::post_hud_message(state, message);
        }

        // Simulation has already finalized identity, passability, navigation,
        // and the returned hash. The app only updates its render-side list.
        // Removals first: a cell can be erased and re-marked in one frame, and
        // the upsert must win.
        if !frame_overlay_removals.is_empty() {
            let dropped = state
                .match_state
                .match_presentation
                .overlays
                .remove_cells(&frame_overlay_removals);
            if dropped != 0 {
                log::trace!("Dropped {dropped} erased overlay cells from state.overlays");
            }
        }
        if !frame_overlay_updates.is_empty() {
            upsert_occupied_overlay_render_entries(state, frame_overlay_updates);
        }
    }

    // AnimClass producers can install a new palette after construction without
    // changing any entity identity. Unit deploy Anim+D4 (739C1A/739DFE) is one
    // such producer: its first ordinary draw needs the selected-scheme atlas
    // frames even when the tick reports no entity spawn or ownership change.
    if refresh_atlases_after_tick
        || (frame_committed
            && state
                .match_state
                .sim_runtime
                .as_ref()
                .is_some_and(|runtime| {
                    !sprite_atlas::atlas_covers_anim_remaps(
                        state.match_state.match_presentation.sprite_atlas.as_ref(),
                        &runtime.simulation,
                        &state.match_state.match_presentation.house_color_map,
                    ) || !sprite_atlas::atlas_covers_sprite_keys(
                        state.match_state.match_presentation.sprite_atlas.as_ref(),
                        &sprite_atlas::collect_fogged_sprite_keys(
                            &runtime.simulation,
                            &runtime.resources.rules,
                            &state.match_state.match_presentation.house_color_map,
                        ),
                        Some(&runtime.resources.rules),
                    )
                }))
    {
        refresh_entity_atlases(state);
    }
    frame_committed
}

/// Refresh the presentation-owned light transaction from the committed runtime.
fn refresh_cell_lighting(state: &mut AppState) {
    let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
        return;
    };
    let Some(terrain) = runtime.resources.terrain_template.as_ref() else {
        return;
    };
    let presentation = &mut state.match_state.match_presentation;
    presentation.lighting.refresh(
        terrain,
        &runtime.simulation,
        presentation.in_game_options.detail_level,
    );
}

pub(crate) fn apply_trigger_effects(state: &mut AppState, effects: &[TriggerEffect]) {
    let mut refresh_sidebar = false;
    for effect in effects {
        match effect {
            TriggerEffect::ScenarioPause {
                duration,
                outermost,
            } => {
                let now = crate::app::match_runtime::frame_pacer::wall_clock_ms();
                super::scenario_pause::enter(state, *duration, *outermost, now);
            }
            TriggerEffect::ScenarioResume => {
                let now = crate::app::match_runtime::frame_pacer::wall_clock_ms();
                super::scenario_pause::resume(state, now);
            }
            TriggerEffect::TalkBubble {
                actor,
                value,
                duration,
            } => {
                let now = crate::app::match_runtime::frame_pacer::wall_clock_ms();
                state
                    .match_state
                    .match_presentation
                    .talk_bubble
                    .set(*actor, *value, *duration, now);
            }
            TriggerEffect::MusicTheme { request } => {
                let wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
                state.audio.queue_theme(request, wall_ms);
            }
            TriggerEffect::ChronoScreen { command } => {
                crate::app::chrono_screen::apply(state, *command);
            }
            TriggerEffect::RadarMovie {
                index,
                pause_simulation,
            } => {
                crate::app::radar_movie::enqueue(state, *index, *pause_simulation);
            }
            TriggerEffect::FullscreenMovie { index } => {
                let name = state
                    .process_assets
                    .native_rules()
                    .and_then(|rules| rules.movies().name(*index))
                    .map(str::to_owned);
                if let Some(name) = name {
                    crate::app::App::start_fullscreen_movie(
                        state,
                        &name,
                        crate::app::frontend::fullscreen_movie::MovieReturn::RunningScenario,
                    );
                }
            }
            // Consumed by the frame's existing bridge/terrain refresh.
            TriggerEffect::BridgeStateChanged => {}
            TriggerEffect::HomeCamera { coord } => {
                crate::app::input::camera::apply_home_camera(state, *coord);
            }
            TriggerEffect::CameraAtCoord { coord, velocity } => {
                crate::app::input::camera::apply_trigger_camera(state, *coord, *velocity);
            }
            TriggerEffect::InputLockChanged { locked, was_locked } => {
                crate::app::input::campaign_input::apply_lock_effect(state, *locked, *was_locked);
            }
            TriggerEffect::RecentEventCell { cell } => {
                if let Some(minimap) = state.match_state.match_presentation.minimap.as_mut() {
                    minimap.remember_event_cell(*cell);
                }
            }
            TriggerEffect::LocalizedMessage { csf_label } => {
                crate::app::input::messages::post_trigger_message(state, csf_label);
            }
            TriggerEffect::SelectSidebarTab { index } => {
                refresh_sidebar = true;
                let presentation = &mut state.match_state.match_presentation;
                if let Some(tab) = presentation.sidebar_projection.selectable_tab(*index) {
                    // Trigger6E4100 bypasses the player-input lock, retaining
                    // each strip's independent scroll state.
                    presentation.active_sidebar_tab = tab;
                }
            }
            TriggerEffect::FlashSidebarCameo {
                type_id,
                duration,
                frame,
            } => {
                refresh_sidebar = true;
                if let Some(runtime) = state.match_state.sim_runtime.as_ref() {
                    state
                        .match_state
                        .match_presentation
                        .sidebar_projection
                        .flash_cameo(
                            *type_id,
                            *duration,
                            *frame,
                            runtime.simulation.interner(),
                            &runtime.resources.rules,
                        );
                }
            }
        }
    }
    // Publish the tab and initial flash phase in this same ordered batch;
    // input events are not required to expose trigger-authored UI changes.
    if refresh_sidebar {
        crate::app::presentation::sidebar_render::refresh_sidebar_projection(state);
    }
}

pub(crate) fn update_building_placement_preview(state: &mut AppState) {
    let Some(type_id) = state.armed_building_type() else {
        state.match_state.input.building_placement_preview = None;
        return;
    };
    if state
        .match_state
        .input
        .building_placement_issuer()
        .is_none()
    {
        // Product-only pointer expiry retains the current type/foundation;
        // it is neither an abandon nor a permission to emit another PLACE.
        return;
    }
    let owner: String = preferred_local_owner(state).unwrap_or_else(|| "Americans".to_string());
    let (Some(sim), Some(rules)) = (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        state.rules(),
    ) else {
        state.match_state.input.building_placement_preview = None;
        return;
    };
    let (fw, fh, foundation_str) = rules
        .object(type_id)
        .map(|obj| {
            let (w, h) = obj.foundation.dimensions();
            (w, h, obj.foundation.clone())
        })
        .unwrap_or((1, 1, crate::rules::foundation::Foundation::default()));
    // Log at info level once per type_id change so it's visible in console.
    static LAST_LOG: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let hash: u64 = type_id
        .as_bytes()
        .iter()
        .fold(0u64, |acc, &b| acc.wrapping_mul(31).wrapping_add(b as u64));
    if LAST_LOG.swap(hash, std::sync::atomic::Ordering::Relaxed) != hash {
        log::info!(
            "Placement preview: type={} foundation=\"{}\" → {}x{}",
            type_id,
            foundation_str,
            fw,
            fh,
        );
    }
    let cursor_cell = screen_point_to_world_cell(
        state,
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    // UpdateFoundationCenter4A91B0 uses center + the type's active offset.
    // Resolve it before admission/ground/shroud, through the shared owner
    // also used by a fresh LeftUp; rendering consumes the resulting Cells.
    let (rx, ry) = foundation_str.placement_origin_at_cursor(cursor_cell);
    state.match_state.input.building_placement_preview =
        production::placement_preview_for_owner_with_overlays(
            sim,
            rules,
            &owner,
            type_id,
            rx,
            ry,
            state.overlay_registry(),
        );
}

/// Refresh entity atlases after entities spawn, die, change type/owner or leave
/// the display.
///
/// This runs on every such event, so the check is per object model, never per
/// sprite key: a refresh happens only when a voxel model or an object (type,
/// house colour) appears that the atlas has not collected. The refresh then
/// renders only those sprites and appends them to a growth page.
/// Reuses the process asset manager instead of creating a new one (avoids re-opening
/// all MIX archives from disk).
pub(crate) fn refresh_entity_atlases(state: &mut AppState) {
    let Some(rt) = state.match_state.sim_runtime.as_ref() else {
        return;
    };
    let sim = &rt.simulation;
    let bound_rules = Some(&rt.resources.rules);
    let Some(asset_manager) = state.process_assets.manager() else {
        log::warn!("Atlas refresh skipped: no asset manager available");
        return;
    };

    // Check if the unit atlas lacks a voxel model the world now draws.
    let unit_rebuild = !unit_atlas::atlas_covers_world(
        state.match_state.match_presentation.unit_atlas.as_ref(),
        sim.entities(),
        bound_rules,
        Some(sim.interner()),
    );

    // Check if the sprite atlas lacks an object (type, house colour), an
    // AnimClass colour remap or the harvest overlay the world now draws.
    let extra_buildings: Vec<&str> =
        crate::app::frontend::skirmish::deployable_building_types(bound_rules);
    let anim_remap_keys = sprite_atlas::collect_anim_remap_base_keys(
        sim,
        &state.match_state.match_presentation.house_color_map,
    );
    let frozen_keys = sprite_atlas::collect_fogged_sprite_keys(
        sim,
        &rt.resources.rules,
        &state.match_state.match_presentation.house_color_map,
    );
    let sprite_rebuild = !sprite_atlas::atlas_covers_world(
        state.match_state.match_presentation.sprite_atlas.as_ref(),
        sim.entities(),
        &state.match_state.match_presentation.house_color_map,
        &extra_buildings,
        &anim_remap_keys,
        Some(sim.interner()),
    ) || !sprite_atlas::atlas_covers_sprite_keys(
        state.match_state.match_presentation.sprite_atlas.as_ref(),
        &frozen_keys,
        bound_rules,
    );

    if !unit_rebuild && !sprite_rebuild {
        return;
    }

    // Growth pages must key on the palettes the load-time pages used.
    let Some((palette, cell_palette)) = crate::map::theater::load_theater_unit_and_iso_palettes(
        asset_manager,
        &state.match_state.match_presentation.theater_name,
    ) else {
        log::warn!("Atlas refresh skipped: theater palettes not found");
        return;
    };

    if unit_rebuild {
        log::info!("Refreshing the unit atlas: new voxel models");
        let existing = state.match_state.match_presentation.unit_atlas.take();
        if let Some(new_unit_atlas) = unit_atlas::build_unit_atlas(
            &state.renderer.gpu.device,
            &state.renderer.gpu.queue,
            &state.renderer.batch_renderer,
            sim.entities(),
            asset_manager,
            bound_rules,
            existing,
            Some(sim.interner()),
        ) {
            state.match_state.match_presentation.unit_atlas = Some(new_unit_atlas);
        }
    }

    if sprite_rebuild {
        log::info!("Refreshing the sprite atlas: new SHP object types or colour remaps");
        let existing = state.match_state.match_presentation.sprite_atlas.take();
        let cell_drawer_type_ids: HashSet<String> = sim
            .resolved_terrain()
            .into_iter()
            .flat_map(|terrain| terrain.tile_animations())
            .map(|anim| anim.anim_name.to_ascii_uppercase())
            .collect();
        if let Some(new_sprite_atlas) = sprite_atlas::build_sprite_atlas(
            &state.renderer.gpu.device,
            &state.renderer.gpu.queue,
            &state.renderer.batch_renderer,
            sim.entities(),
            asset_manager,
            &palette,
            &state.match_state.match_presentation.theater_ext,
            &state.match_state.match_presentation.theater_name,
            bound_rules,
            bound_rules.map(|rules| rules.art()),
            &state.match_state.match_presentation.house_color_map,
            &extra_buildings,
            &anim_remap_keys,
            &cell_drawer_type_ids,
            &frozen_keys,
            Some(&cell_palette),
            existing,
            Some(sim.interner()),
        ) {
            state.match_state.match_presentation.sprite_atlas = Some(new_sprite_atlas);
        }
    }
}

/// Upsert authoritative occupied-overlay entries into `state.overlays`.
///
/// Background: the overlay renderer iterates `state.overlays`, the static list
/// loaded from the map's `[OverlayPack]`. Sim-side mutations that create new
/// overlay cells (TIBTRE ore spawn, ore_growth spread) update `OverlayGrid`
/// but never touched `state.overlays`, so the new cells were invisible even
/// though their sim state and pathfinding were correct. A coordinate can also
/// be cleared and later receive a different overlay variant; the renderer only
/// accepts live data when the cached identity matches. This sync therefore
/// inserts absent coordinates and updates identity plus frame in place for
/// existing coordinates. Candidates may be one frame's delta or the full live
/// occupied set returned after snapshot restoration.
///
/// Cleared cached entries are render-inert because the renderer treats live
/// `OverlayGrid` state as authoritative; a later occupied update or post-load
/// snapshot replaces their stale identity. Unrelated entries retain their order
/// and fields.
pub(crate) fn upsert_occupied_overlay_render_entries(
    state: &mut AppState,
    candidates: Vec<crate::map::overlay::OverlayEntry>,
) {
    let synced = state
        .match_state
        .match_presentation
        .overlays
        .upsert_occupied(candidates);
    if synced != 0 {
        log::trace!(
            "Synced {} occupied cells from OverlayGrid to state.overlays",
            synced
        );
    }
}

// The slot-retaining coordinate upsert lives on OverlayRenderIndex (F08);
// its contract tests moved with it. Tests here drive the index directly.
#[cfg(test)]
fn upsert_overlay_entries(
    existing: &mut Vec<crate::map::overlay::OverlayEntry>,
    candidates: Vec<crate::map::overlay::OverlayEntry>,
) -> usize {
    let mut index = crate::app::presentation::overlay_index::OverlayRenderIndex::default();
    index.replace_from_source(std::mem::take(existing));
    let synced = index.upsert_occupied(candidates);
    *existing = index.as_slice().to_vec();
    synced
}

pub(crate) fn screen_point_to_world(state: &AppState, screen_x: f32, screen_y: f32) -> (f32, f32) {
    screen_point_to_world_with_camera(
        (screen_x, screen_y),
        (
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
        ),
        state.match_state.input.zoom_level,
    )
}

/// Undo the current view transform before resolving a tactical click. Keep
/// subpixels here: the shared tactical inverse owns pixel quantization.
pub(crate) fn screen_point_to_world_with_camera(
    screen: (f32, f32),
    camera: (f32, f32),
    zoom: f32,
) -> (f32, f32) {
    // Screen pixel / zoom = world offset from camera top-left.
    (screen.0 / zoom + camera.0, screen.1 / zoom + camera.1)
}

/// Borrow current real-cell levels and flags for the inverse. There is no app
/// projection to invalidate after collapse, tile changes or world replacement.
/// Native 6D674C resolves the current cell before reading level/0x100/0x800; see
/// tools/bridge_click_state_oracle/README.md.
pub(crate) fn tactical_cells(
    sim: &crate::sim::world::Simulation,
) -> Option<&dyn terrain::TacticalCellLookup> {
    sim.resolved_terrain()
        .map(|terrain| terrain as &dyn terrain::TacticalCellLookup)
}

/// Shared owner for world-space point -> map-cell resolution in the app layer.
///
/// Any app code that already has world coordinates should use this instead of
/// re-calling the tactical inverse inline.
pub(crate) fn world_point_to_cell(
    world_x: f32,
    world_y: f32,
    cells: Option<&dyn terrain::TacticalCellLookup>,
) -> (u16, u16) {
    let inverse = terrain::screen_to_cell_tactical_inverse(
        world_x,
        world_y,
        terrain::TacticalInverseContext {
            cells,
            viewport_offset_x: 0.0,
            viewport_offset_y: 0.0,
        },
    );
    let (iso_rx, iso_ry) = match inverse {
        terrain::TacticalInverseResult::Cell { rx, ry }
        | terrain::TacticalInverseResult::Fallback { rx, ry } => (rx, ry),
    };
    (
        // Current Rust app callers expect a concrete in-map cell. Keep this
        // clamp isolated here until off-map sentinel behavior is modeled.
        iso_rx.round().max(0.0) as u16,
        iso_ry.round().max(0.0) as u16,
    )
}

/// Shared owner for screen-space cursor -> map-cell resolution in the app layer.
///
/// This is the entry point UI/input code should use when starting from screen
/// coordinates and the current camera.
pub(crate) fn screen_point_to_world_cell(
    state: &AppState,
    screen_x: f32,
    screen_y: f32,
) -> (u16, u16) {
    let (world_x, world_y) = screen_point_to_world(state, screen_x, screen_y);
    world_point_to_cell(
        world_x,
        world_y,
        state
            .sim_view()
            .and_then(|view| tactical_cells(view.simulation())),
    )
}

pub(crate) fn nearest_walkable_cell(
    grid: &crate::sim::pathfinding::PathGrid,
    start: (u16, u16),
    max_radius: u16,
) -> Option<(u16, u16)> {
    grid.nearest_walkable_any_layer(start.0, start.1, max_radius)
}

pub(crate) fn clamp_cell_to_grid(
    grid: &crate::sim::pathfinding::PathGrid,
    cell: (u16, u16),
) -> (u16, u16) {
    let max_x = grid.width().saturating_sub(1);
    let max_y = grid.height().saturating_sub(1);
    (cell.0.min(max_x), cell.1.min(max_y))
}

#[cfg(test)]
mod tests {
    use super::{
        ExactStepError, ExactStepReceipt, upsert_overlay_entries, validate_exact_step_receipt,
        world_point_to_cell,
    };
    use crate::map::overlay::OverlayEntry;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
    use crate::rules::terrain_rules::{LandType, SpeedCostProfile};
    use crate::sim::intern::StringInterner;
    use crate::sim::terrain_object::{
        TerrainObjectState, mark_terrain_occupation, unmark_terrain_occupation,
    };
    use std::collections::BTreeMap;

    fn entry(rx: u16, ry: u16, overlay_id: u8, frame: u8) -> OverlayEntry {
        OverlayEntry {
            rx,
            ry,
            overlay_id,
            frame,
        }
    }

    #[test]
    fn gsi_04_10_dynamic_navigation_rebuild_refreshes_the_path_grid() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n",
            ))
            .unwrap();
        let speed_costs = SpeedCostProfile {
            foot: Some(100),
            track: Some(100),
            wheel: Some(100),
            float: Some(100),
            amphibious: Some(100),
            float_beach: Some(100),
            hover: Some(100),
        };
        let terrain = ResolvedTerrainGrid::from_cells(
            1,
            1,
            vec![ResolvedTerrainCell {
                tileset_index: None,
                land_type: LandType::Clear.as_index(),
                speed_costs,
                zone_type: zone_class::GROUND,
                base_land_type: LandType::Clear.as_index(),
                base_speed_costs: speed_costs,
                ..crate::map::resolved_terrain::test_flat_cell(0, 0)
            }],
        );
        let mut sim = crate::sim::world::Simulation::new();
        *sim.resolved_terrain_mut_for_test() = Some(terrain);
        let mut interner = StringInterner::default();
        let tree = {
            let mut terrain = TerrainObjectState::for_test(1, interner.intern("TREE01"), 0, 0);
            terrain.set_health_for_test(10);
            terrain.set_max_health_for_test(10);
            terrain.set_occupation_bits_for_test(7);
            terrain
        };
        mark_terrain_occupation(&tree, sim.resolved_terrain_mut_for_test().as_mut());

        assert!(sim.rebuild_dynamic_navigation(&rules));
        assert!(!sim.path_grid().expect("terrain").is_walkable(0, 0));

        unmark_terrain_occupation(&tree, sim.resolved_terrain_mut_for_test().as_mut());
        assert!(sim.rebuild_dynamic_navigation(&rules));

        assert!(sim.path_grid().expect("terrain").is_walkable(0, 0));
    }

    #[test]
    fn upsert_updates_existing_and_inserts_absent_coordinates() {
        let mut existing = vec![entry(5, 5, 2, 0)];
        let candidates = vec![entry(5, 5, 2, 3), entry(6, 6, 2, 0)];
        assert_eq!(upsert_overlay_entries(&mut existing, candidates), 2);
        assert_eq!(existing.len(), 2);
        assert_eq!(existing[0].frame, 3);
        assert_eq!((existing[1].rx, existing[1].ry), (6, 6));
    }

    #[test]
    fn upsert_reuses_coordinate_within_candidate_list() {
        let mut existing: Vec<OverlayEntry> = Vec::new();
        let candidates = vec![entry(7, 7, 2, 0), entry(7, 7, 2, 5), entry(8, 8, 2, 0)];
        assert_eq!(upsert_overlay_entries(&mut existing, candidates), 3);
        assert_eq!(existing.len(), 2);
        assert_eq!((existing[0].rx, existing[0].ry), (7, 7));
        assert_eq!(existing[0].frame, 5);
        assert_eq!((existing[1].rx, existing[1].ry), (8, 8));
    }

    #[test]
    fn upsert_empty_inputs() {
        let mut existing: Vec<OverlayEntry> = Vec::new();
        let candidates: Vec<OverlayEntry> = Vec::new();
        assert_eq!(upsert_overlay_entries(&mut existing, candidates), 0);
        assert!(existing.is_empty());
    }

    #[test]
    fn upsert_identical_existing_entries_is_a_noop() {
        let mut existing = vec![entry(1, 1, 2, 0), entry(2, 2, 3, 5)];
        let candidates = existing.clone();
        assert_eq!(upsert_overlay_entries(&mut existing, candidates), 0);
    }

    #[test]
    fn gsi_04_09_render_handoff_replaces_regerminated_overlay_variant() {
        let old_variant = entry(4, 4, 2, 7);
        let untouched = entry(5, 4, 99, 3);
        let mut render_entries = vec![old_variant.clone(), untouched.clone()];
        let authoritative = vec![entry(4, 4, 3, 1)];

        assert_eq!(
            upsert_overlay_entries(&mut render_entries, authoritative),
            1
        );
        assert_eq!(render_entries.len(), 2);
        assert_eq!((render_entries[0].rx, render_entries[0].ry), (4, 4));
        assert_eq!(render_entries[0].overlay_id, 3);
        assert_eq!(render_entries[0].frame, 1);
        assert_eq!(render_entries[1].overlay_id, untouched.overlay_id);
        assert_eq!(render_entries[1].frame, untouched.frame);
    }

    #[test]
    fn exact_receipt_accepts_one_wrapping_gameplay_frame() {
        let receipt = ExactStepReceipt {
            tick_before: 9,
            tick_after: 10,
            binary_frame_before: u32::MAX,
            binary_frame_after: 0,
        };
        assert_eq!(validate_exact_step_receipt(receipt), Ok(()));
    }

    #[test]
    fn exact_receipt_rejects_zero_or_multiple_frame_advances() {
        let receipt = |tick_after, binary_frame_after| ExactStepReceipt {
            tick_before: 10,
            tick_after,
            binary_frame_before: 20,
            binary_frame_after,
        };

        assert_eq!(
            validate_exact_step_receipt(receipt(10, 21)),
            Err(ExactStepError::TickDelta { actual: 0 })
        );
        assert_eq!(
            validate_exact_step_receipt(receipt(11, 20)),
            Err(ExactStepError::FrameDelta { actual: 0 })
        );
        assert_eq!(
            validate_exact_step_receipt(receipt(11, 22)),
            Err(ExactStepError::FrameDelta { actual: 2 })
        );
    }

    #[test]
    fn world_point_to_cell_round_trips_ground_iso_anchor() {
        let (rx, ry, z) = (10_u16, 5_u16, 4_u8);
        let (world_x, world_y) = (150.0, 180.0);
        let mut height_map = BTreeMap::new();
        for hx in 8..=12 {
            for hy in 3..=7 {
                height_map.insert((hx, hy), z);
            }
        }

        assert_eq!(
            world_point_to_cell(
                world_x,
                world_y,
                Some(&crate::map::terrain::TacticalCellFixture {
                    levels: &height_map,
                    bridges: None
                })
            ),
            (rx, ry)
        );
    }

    #[test]
    fn world_point_to_cell_forwards_bridge_lookup() {
        let (world_x, world_y) = (150.0, 180.0);
        let height_map = BTreeMap::new();
        let bridge_cells = BTreeMap::from([(
            (10, 5),
            crate::map::terrain::TacticalBridgeCell {
                structural: true,
                direction_zero: true,
            },
        )]);
        let expected = crate::map::terrain::screen_to_cell_tactical_inverse(
            world_x,
            world_y,
            crate::map::terrain::TacticalInverseContext {
                cells: Some(&crate::map::terrain::TacticalCellFixture {
                    levels: &height_map,
                    bridges: Some(&bridge_cells),
                }),
                viewport_offset_x: 0.0,
                viewport_offset_y: 0.0,
            },
        );
        let (expected_rx, expected_ry) = match expected {
            crate::map::terrain::TacticalInverseResult::Cell { rx, ry }
            | crate::map::terrain::TacticalInverseResult::Fallback { rx, ry } => (rx, ry),
        };

        assert_eq!(
            world_point_to_cell(
                world_x,
                world_y,
                Some(&crate::map::terrain::TacticalCellFixture {
                    levels: &height_map,
                    bridges: Some(&bridge_cells)
                })
            ),
            (
                expected_rx.round().max(0.0) as u16,
                expected_ry.round().max(0.0) as u16,
            )
        );
    }

    #[test]
    fn world_point_to_cell_clamps_negative_results_to_zero() {
        let height_map = BTreeMap::new();
        assert_eq!(
            world_point_to_cell(
                -500.0,
                -500.0,
                Some(&crate::map::terrain::TacticalCellFixture {
                    levels: &height_map,
                    bridges: None
                })
            ),
            (0, 0)
        );
    }
}

#[cfg(test)]
#[path = "camera_cadence_tests.rs"]
mod camera_cadence_tests;

#[cfg(test)]
mod modal_pump_tests {
    use super::{
        SessionMode, audio_pump_follows_main_tick, modal_pump_should_advance_sim,
        resolve_score_rows, score_title_key, should_record_replay_tick,
        wall_clock_service_admission,
    };

    /// 0x005C9BE7: only GameMode 5 retitles 0x108.
    #[test]
    fn only_skirmish_retitles_the_score_dialog() {
        assert_eq!(score_title_key(SessionMode::Skirmish), "GUI:SkirmishScore");
        for mode in [SessionMode::Lan, SessionMode::Wol, SessionMode::Other] {
            assert_eq!(score_title_key(mode), "GUI:MultiplayerScore");
        }
    }

    #[test]
    fn empty_natural_terminal_score_frame_is_recorded() {
        let tick = crate::sim::world::TickResult {
            tick: 10,
            frame_committed: false,
            executed_commands: 0,
            state_hash: Some(123),
            terminal_score_finalized: true,
            spawned_entities: false,
            destroyed_structure: false,
            ownership_changed: false,
            bridge_state_changed: false,
        };

        assert!(should_record_replay_tick(&tick, &[], false));
        // A suspended prefix may already carry an outcome, but the complete
        // admitted journal and final hash arrive only after Stop resumes it.
        assert!(!should_record_replay_tick(&tick, &[], true));
        assert!(!should_record_replay_tick(
            &crate::sim::world::TickResult {
                terminal_score_finalized: false,
                ..tick
            },
            &[],
            false
        ));
    }

    #[test]
    fn gsi_01_03_debug_pause_does_not_impersonate_an_offline_modal() {
        let debug_pause =
            wall_clock_service_admission(true, false, SessionMode::Skirmish, false, false);
        assert!(!debug_pause.simulation);
        assert!(debug_pause.tactical_mutation);

        let actual_modal =
            wall_clock_service_admission(true, true, SessionMode::Skirmish, false, false);
        assert!(!actual_modal.simulation);
        assert!(!actual_modal.tactical_mutation);
    }

    #[test]
    fn gsi_01_03_tactical_view_mutators_follow_modal_service_admission() {
        // No modal means the ordinary app loop owns camera/placement updates;
        // modal-pump blockers have no authority on that path.
        for mode in [
            SessionMode::Campaign,
            SessionMode::Skirmish,
            SessionMode::Lan,
            SessionMode::Wol,
        ] {
            assert_eq!(
                wall_clock_service_admission(false, false, mode, true, true),
                super::WallClockServiceAdmission {
                    simulation: true,
                    tactical_mutation: true,
                }
            );
        }

        // The active offline Menu, Abort and Options callers all enter the
        // service-only return: their frozen battlefield cannot pan or rebuild a
        // placement preview underneath the dialog.
        for mode in [SessionMode::Campaign, SessionMode::Skirmish] {
            assert_eq!(
                wall_clock_service_admission(true, true, mode, false, false),
                super::WallClockServiceAdmission {
                    simulation: false,
                    tactical_mutation: false,
                }
            );
        }

        // Preserve the existing network-modal branch, including both native
        // reasons it falls back to service-only work.
        for mode in [SessionMode::Lan, SessionMode::Wol] {
            let admitted = super::WallClockServiceAdmission {
                simulation: true,
                tactical_mutation: true,
            };
            let denied = super::WallClockServiceAdmission {
                simulation: false,
                tactical_mutation: false,
            };
            assert_eq!(
                wall_clock_service_admission(true, true, mode, false, false),
                admitted
            );
            assert_eq!(
                wall_clock_service_admission(true, true, mode, true, false),
                denied
            );
            assert_eq!(
                wall_clock_service_admission(true, true, mode, false, true),
                denied
            );
        }
    }

    #[test]
    fn offline_matches_leave_the_audio_pump_to_main_tick() {
        // g_GameMode 0 and 5 only: Main::ThrottleFrame 0x0055E160 only sleeps.
        assert!(audio_pump_follows_main_tick(
            SessionMode::Campaign,
            true,
            false
        ));
        assert!(audio_pump_follows_main_tick(
            SessionMode::Skirmish,
            true,
            false
        ));
        assert!(!audio_pump_follows_main_tick(SessionMode::Lan, true, false));
        assert!(!audio_pump_follows_main_tick(
            SessionMode::Other,
            true,
            false
        ));
        // The in-game menu's modal pump 0x00623120 services its own passes.
        assert!(!audio_pump_follows_main_tick(
            SessionMode::Skirmish,
            true,
            true
        ));
    }

    #[test]
    fn only_network_modes_advance_behind_a_modal() {
        // {3 LAN, 4 WOL} advance; {0 campaign, 5 skirmish} + Other freeze.
        assert!(modal_pump_should_advance_sim(
            SessionMode::Lan,
            false,
            false
        ));
        assert!(modal_pump_should_advance_sim(
            SessionMode::Wol,
            false,
            false
        ));
        assert!(!modal_pump_should_advance_sim(
            SessionMode::Campaign,
            false,
            false
        ));
        assert!(!modal_pump_should_advance_sim(
            SessionMode::Skirmish,
            false,
            false
        ));
        assert!(!modal_pump_should_advance_sim(
            SessionMode::Other,
            false,
            false
        ));
    }

    #[test]
    fn reentrancy_guard_blocks_advance_even_on_network() {
        // The native reentrancy guard: a fixed tick already in progress means the
        // pump skips advancing, even on the network branch.
        assert!(!modal_pump_should_advance_sim(
            SessionMode::Lan,
            false,
            true
        ));
        assert!(!modal_pump_should_advance_sim(
            SessionMode::Wol,
            false,
            true
        ));
    }

    #[test]
    fn service_only_blockers_prevent_network_modal_advance() {
        assert!(!modal_pump_should_advance_sim(
            SessionMode::Lan,
            true,
            false
        ));
        assert!(!modal_pump_should_advance_sim(
            SessionMode::Wol,
            true,
            false
        ));
    }

    #[test]
    fn pumped_tick_delta_is_zero_offline_and_n_on_network() {
        // C2 acceptance at the decision level: the pump decision drives whether the
        // world's fixed tick advances per pumped frame. `tick` stands in for
        // `sim.session.tick`; the full headless-World assertion (incl. "no
        // battlefield recomposite offline") lands with the live `service_tick` swap.
        const FRAMES: u64 = 7;
        let pumped = |mode: SessionMode| -> u64 {
            let mut tick = 0u64;
            for _ in 0..FRAMES {
                if modal_pump_should_advance_sim(mode, false, false) {
                    tick += 1; // one fixed step advanced this pumped frame
                }
            }
            tick
        };
        // Offline freezes: zero advance over N pumped frames.
        assert_eq!(pumped(SessionMode::Skirmish), 0);
        assert_eq!(pumped(SessionMode::Campaign), 0);
        // Network advances once per pumped frame.
        assert_eq!(pumped(SessionMode::Lan), FRAMES);
        assert_eq!(pumped(SessionMode::Wol), FRAMES);
    }

    #[test]
    fn pumped_world_tick_freezes_offline_and_advances_on_network() {
        use crate::sim::world::Simulation;

        // C2 acceptance with a real headless World: drive `advance_tick` exactly
        // when the pump decision is true, and assert `session.tick` motion.
        // `advance_tick` commits one tick per call (no entities/rules needed).
        const FRAMES: u64 = 7;
        let pumped_world_delta = |mode: SessionMode| -> u64 {
            let mut sim = Simulation::new();
            let start = sim.session().tick();
            for _ in 0..FRAMES {
                if modal_pump_should_advance_sim(mode, false, false) {
                    // `tick_ms` does not affect the asserted tick delta; a literal
                    // matches the sim-test style and avoids the const dependency.
                    sim.advance_tick(&[], None, None, None, 33);
                }
            }
            sim.session().tick() - start
        };

        // Offline modes freeze the world behind the modal: zero tick advance.
        assert_eq!(pumped_world_delta(SessionMode::Skirmish), 0);
        assert_eq!(pumped_world_delta(SessionMode::Campaign), 0);
        // Network modes advance one fixed tick per pumped frame (dead code this
        // build; proves the contract for when multiplayer lands).
        assert_eq!(pumped_world_delta(SessionMode::Lan), FRAMES);
        assert_eq!(pumped_world_delta(SessionMode::Wol), FRAMES);
    }

    #[test]
    fn retail_score_row_colours_match_the_retail_still() {
        // Retail still at 800x600: the Easy AI (DarkRed) and the local player
        // (DarkBlue, Color=2) rows, in RGB565 units.
        let Some(rules) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let schemes = crate::rules::color_scheme::parse_color_schemes(&rules);
        for (name, units) in [("DarkRed", [31, 6, 3]), ("DarkBlue", [4, 26, 26])] {
            let entry = crate::rules::color_scheme::scheme_entry_by_name(&schemes, name)
                .unwrap_or_else(|| panic!("[Colors] {name}"));
            let rgb = crate::rules::house_colors::house_text_rgb(
                &schemes,
                crate::rules::house_colors::HouseColorIndex(entry as u8),
            );
            assert_eq!([rgb[0] >> 3, rgb[1] >> 2, rgb[2] >> 3], units, "{name}");
        }
    }

    #[test]
    fn score_rows_follow_the_houses_names_colours_and_the_native_order() {
        use crate::sim::house_state::HouseState;
        // Retail still: the local player (DarkBlue) and one Easy AI (DarkRed),
        // both at score 0; the dialog lists "Computer" first.
        let schemes = crate::rules::color_scheme::parse_color_schemes(
            &crate::rules::ini_parser::IniFile::from_str(
                "[Colors]\nDarkRed=0,230,255\nDarkBlue=153,214,212\n",
            ),
        );
        let mut sim = crate::sim::world::Simulation::with_seed(1);
        let player = sim.intern("Player");
        let computer = sim.intern("Computer1");
        let mut player_house = HouseState::new(player, 0, None, true, 0, 10);
        player_house
            .set_ui_name_for_test(crate::sim::house_state::HouseUiName::handle("[New Player]"));
        let mut computer_house = HouseState::new(computer, 1, None, false, 0, 10);
        computer_house.set_ui_name_for_test(crate::sim::house_state::HouseUiName::Computer);
        sim.houses_mut_for_test().register(player_house);
        sim.houses_mut_for_test().register(computer_house);
        let row = |owner, losses| crate::sim::score::TerminalScoreRowSnapshot {
            owner,
            country: None,
            survived: owner == computer,
            kills: 0,
            losses,
            built: 0,
            raw_score: 0,
            score: 0,
        };
        let snapshot = crate::sim::score::TerminalScoreSnapshot {
            rows: vec![row(player, 15), row(computer, 0)],
        };
        let colors = crate::map::houses::HouseColorMap::from([
            (
                "Player".to_string(),
                crate::rules::house_colors::HouseColorIndex(1),
            ),
            (
                "Computer1".to_string(),
                crate::rules::house_colors::HouseColorIndex(0),
            ),
        ]);
        let rows = resolve_score_rows(
            &sim,
            &snapshot,
            &crate::app::presentation::house_ui::HouseUiSources {
                csf: None,
                computer: "Computer",
                schemes: &schemes,
                colors: &colors,
            },
            |_| None,
        );
        let shown: Vec<(&str, [u8; 3], u32)> = rows
            .iter()
            .map(|row| (row.name.as_str(), row.rgb, row.losses))
            .collect();
        assert_eq!(
            shown,
            [
                ("Computer", [255, 25, 25], 0),
                ("[New Player]", [34, 105, 212], 15)
            ]
        );
        // A House still holding its type's UIName shows its country, else
        // its House name.
        sim.houses_mut_for_test()
            .get_mut(&player)
            .unwrap()
            .set_ui_name_for_test(crate::sim::house_state::HouseUiName::TypeName);
        let rows = resolve_score_rows(
            &sim,
            &snapshot,
            &crate::app::presentation::house_ui::HouseUiSources {
                csf: None,
                computer: "Computer",
                schemes: &schemes,
                colors: &colors,
            },
            |_| None,
        );
        assert_eq!(rows[1].name, "Player");
        // A map `UIName=` key resolves through the string table, then the
        // field keeps 20 characters (500CD7..500CEC).
        sim.houses_mut_for_test()
            .get_mut(&player)
            .unwrap()
            .set_ui_name_for_test(crate::sim::house_state::HouseUiName::Csf(
                "Name:AVeryLongHouseUiNameKey".into(),
            ));
        let rows = resolve_score_rows(
            &sim,
            &snapshot,
            &crate::app::presentation::house_ui::HouseUiSources {
                csf: None,
                computer: "Computer",
                schemes: &schemes,
                colors: &colors,
            },
            |_| None,
        );
        assert_eq!(rows[1].name, "Name:AVeryLongHouseU");
    }
}
