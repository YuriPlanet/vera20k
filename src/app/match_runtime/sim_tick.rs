//! In-game update phase — advances fixed-step simulation, triggers, path grids, and atlases.
//!
//! Camera control lives in `input::camera`. Building animations, damage fires, sidebar
//! UI tick, and sound playback live in `presentation::building_anim`.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

#[cfg(feature = "telemetry-dashboard")]
use crate::util::time::Instant;
use std::path::Path;

use crate::app::AppState;
use crate::app::PlatformState;
use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::state::FrontendState;
use crate::app::input::commands::{preferred_local_owner, preferred_local_owner_name};
use crate::app::match_runtime::session::SessionMode;
use crate::app::match_runtime::sim_host::SimHost;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::presentation::state::MatchPresentationState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;

use crate::app::types::SIM_TICK_MS;
use crate::map::terrain;
use crate::sim::production;
use crate::sim::replay::ReplayLog;
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
    let can_finish_queue = state.match_state.live_network.is_some();
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    let can_finish_queue = can_finish_queue && state.frontend.graphics_lost.is_none();
    let active = &mut state.match_state;
    let session_tick = match replay_flush_facts(
        &mut active.sim_runtime,
        active.match_diagnostics.replay_log.as_mut(),
        active.frame_stopped,
        can_finish_queue,
    ) {
        ReplayFlush::Sealed(tick) => tick,
        ReplayFlush::QueueHandoff(pending) => {
            // The worker stopped before the network queue. Use the same
            // live completion as an ordinary pass before sealing.
            finish_threaded_output(state, pending, None);
            let active = &mut state.match_state;
            seal_replay_log(
                &active.sim_runtime,
                active.match_diagnostics.replay_log.as_mut(),
                active.frame_stopped,
            )
        }
    };
    write_replay_log(&mut state.match_state.match_diagnostics, session_tick);
}

/// A panicked frame may have written only a prefix. Keep the recorded
/// diagnostic journal without resuming that frame or sealing its world.
pub(crate) fn preserve_replay_after_panic(match_state: &mut MatchState) {
    let session_tick = match_state
        .sim_runtime
        .runtime()
        .map_or(0, |rt| rt.simulation.session().tick());
    write_replay_log(&mut match_state.match_diagnostics, session_tick);
}

fn write_replay_log(
    match_diagnostics: &mut crate::app::match_diagnostics::MatchDiagnosticsState,
    session_tick: u64,
) {
    match match_diagnostics.flush_to(session_tick, Path::new(REPLAYS_DIR), replay_flush_time()) {
        Ok(Some(flush)) => log::info!(
            "Deterministic diagnostic log flushed: {} ticks -> {}",
            flush.tick_count,
            flush.path.display()
        ),
        Ok(None) => {}
        Err(e) => log::error!("Diagnostic-log flush failed: {e}"),
    }
}

/// Capture the initial world before either a frame or a skirmish cheat can change it.
pub(crate) fn prepare_replay_recording(
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
) {
    if let Some(rt) = match_state.sim_runtime.runtime() {
        use crate::sim::replay::ReplayLaunchInput;
        let campaign = match_state.session.network_authored_start();
        let launch = campaign
            .map(|campaign| ReplayLaunchInput::MapHouses(&campaign.authored))
            .or_else(|| {
                match_state
                    .session
                    .network_setup()
                    .or_else(|| match_state.startup.offline_launch())
                    .map(ReplayLaunchInput::Slots)
            });
        let _ = match_state.match_diagnostics.get_or_begin_replay(rt);
        match_state.match_diagnostics.retain_replay_startup(
            rt,
            launch,
            match_state.loaded_map_source.as_ref(),
            match_state.loaded_map_hash,
            crate::sim::replay::ReplayStartOptions {
                every_frame_network: match_state
                    .live_network
                    .as_ref()
                    .is_some_and(|live| live.dispatches_every_frame()),
                quick_coop_starts: match_state
                    .session
                    .network_wol_start()
                    .is_some_and(|start| start.quick_coop),
                personal_rules: process_assets
                    .native_rules()
                    .map(|owner| owner.personal_rules().clone())
                    .unwrap_or_default(),
                campaign_carry_over: match_state.session.network_campaign_carry_over(),
            },
        );
    }
}

/// Timeline-boundary close (new match install, in-scenario load): a write
/// failure discards the segment instead of retaining it, so the next
/// timeline can never append under the previous timeline's header.
pub(crate) fn close_replay_segment_for_new_timeline(state: &mut AppState) {
    // The client can continue in this scenario after a developer closes its
    // replay. Deliver the frame's presentation and network tail as usual.
    finish_threaded_tick(state, None);
    let active = &mut state.match_state;
    let session_tick = seal_replay_log(
        &active.sim_runtime,
        active.match_diagnostics.replay_log.as_mut(),
        active.frame_stopped,
    );
    match state
        .match_state
        .match_diagnostics
        .close_segment_for_new_timeline(session_tick, Path::new(REPLAYS_DIR), replay_flush_time())
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

enum ReplayFlush {
    Sealed(u64),
    QueueHandoff(ThreadedTick),
}

fn replay_flush_facts(
    host: &mut super::sim_host::SimHost,
    mut replay_log: Option<&mut ReplayLog>,
    mut frame_stopped: bool,
    can_finish_queue: bool,
) -> ReplayFlush {
    // A lifecycle write may already have reclaimed the world. The result
    // still belongs in the journal before the endpoint is read from it.
    if let Some(pending) = take_threaded_tick(host, replay_log.as_deref_mut()) {
        if can_finish_queue
            && pending
                .output
                .as_ref()
                .is_ok_and(|output| output.network_waiting)
        {
            return ReplayFlush::QueueHandoff(pending);
        }
        frame_stopped |= pending.output.is_err();
    }
    ReplayFlush::Sealed(seal_replay_log(host, replay_log, frame_stopped))
}

fn seal_replay_log(
    host: &super::sim_host::SimHost,
    replay_log: Option<&mut ReplayLog>,
    frame_stopped: bool,
) -> u64 {
    let Some(rt) = host.home_runtime() else {
        return 0;
    };
    if !frame_stopped && let Some(log) = replay_log {
        log.finish(&rt.simulation);
    }
    rt.simulation.session().tick()
}

fn replay_flush_time() -> u64 {
    crate::util::time::SystemTime::now()
        .duration_since(crate::util::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Route an admitted native result after its in-House Vox wait and callbacks.
/// The pending wait is serviced through the suspended frame's client boundary.
pub(crate) fn drive_local_player_outcome_exit(state: &mut AppState, wall_ms: u64) {
    if !matches!(state.frontend.screen, GameScreen::InGame)
        || state.match_state.scenario_exit.is_some()
        || state
            .match_state
            .match_presentation
            .narrative_sync
            .pending()
    {
        return;
    }
    let Some(owner) = state.match_state.local_player_owner().map(str::to_owned) else {
        return;
    };
    let outcome = state
        .match_state
        .sim_runtime
        .runtime()
        .map(|rt| &rt.simulation)
        .filter(|sim| sim.pending_outcome_wait().is_none())
        .and_then(|sim| {
            sim.interner()
                .get(&owner)
                .and_then(|owner| sim.ready_outcome_for_owner(owner))
        });
    let Some(outcome) = outcome.map(|outcome| outcome.kind) else {
        return;
    };
    // docs/spawn.md "Watching": a launcher's watcher is none of the players,
    // so the House it stands in deciding ends the watch with no verdict or
    // score page of that player's, and its launch with no result.
    if state.frontend.external_launch && super::watching::active(&state.match_state) {
        log::info!("The watched match is decided; the watcher leaves it");
        crate::app::App::leave_network_wait_to_main_menu(state);
        return;
    }
    // The loser gets the last word. The simulation published this result on
    // its own frame, the same one a headless or replay run publishes, and no
    // further frame runs while it waits. With `Custom End Game Movie` on, only
    // what follows here waits for the losing commander's clip: the card, the
    // teardown and the score page.
    if crate::app::commander_feedback::owe_the_verdict_a_clip(
        &state.frontend,
        &mut state.match_state,
        &state.persistence.options_profile,
        wall_ms,
    ) {
        return;
    }
    log::info!(
        "Match end ready for local player '{owner}': {}",
        crate::app::match_runtime::scenario_exit::outcome_title(outcome)
    );
    crate::app::App::record_match_result(
        &mut state.frontend,
        &state.match_state,
        match outcome {
            crate::sim::house_state::HouseOutcomeKind::Victory => {
                crate::app::cncnet::ExternalMatchResult::Won
            }
            crate::sim::house_state::HouseOutcomeKind::Defeat => {
                crate::app::cncnet::ExternalMatchResult::Lost
            }
        },
    );
    //685670/685DC0 clear5BF450 before either outcome fade. With `Custom End
    // Game Movie` on, this route waited for the losing commander's clip, so
    // the verdict below is the first thing said after the loser. With the
    // switch off, this cuts a clip still playing.
    crate::app::radar_movie::retire(
        &state.platform,
        &state.renderer,
        &state.diag,
        &state.frontend,
        &mut state.match_state,
        &state.process_assets,
        &mut state.audio,
        &state.persistence,
        wall_ms,
    );
    crate::app::chrono_screen::retire(
        &state.frontend.screen,
        &mut state.match_state,
        &mut state.audio,
        wall_ms,
    );
    // The result's own line, when a clip was still in its way.
    //
    // The simulation emitted it on the kill frame (`Flag_To_Win 0x004FCBA9` /
    // `Flag_To_Lose 0x004FCDA1`). With `Custom End Game Movie` on it was not
    // delivered there, because the clip that kill earns would have cleared it
    // (`RadarMovieCommand::Announce` -> `clear_eva_queues` + `suspend_eva`).
    // The clip layer says it as soon as no clip is in the way; one still
    // playing when the result was published leaves it to here. Either way it
    // is said once. A clip retired in its prelude leaves its EVA suspend for
    // RadarClass::AI's next pass, so that pass's resume is taken first.
    if crate::app::commander_feedback::take_withheld_verdict_line(
        &mut state.match_state.match_presentation,
    ) {
        crate::app::radar_movie::resume_eva_outside_prelude(&mut state.audio);
        play_outcome_eva(
            &state.match_state.sim_runtime,
            &state.process_assets,
            &mut state.audio,
            outcome,
        );
    }
    state.frontend.finished_game_count = state.frontend.finished_game_count.saturating_add(1);
    let elapsed_score_ticks = state
        .match_state
        .scenario_elapsed_clock
        .stop_buckets(wall_ms);
    let elapsed_seconds = elapsed_score_ticks / 60;
    // 0x0055D0C5: the router runs the mode's +0xA0 before GameExit.
    let cooperative = crate::app::cooperative_round::applies(&state.match_state);
    if cooperative {
        crate::app::cooperative_round::mission_ended(
            &state.platform,
            &state.renderer,
            &state.diag,
            &mut state.frontend,
            &mut state.match_state,
            &state.process_assets,
            &mut state.audio.sfx_player,
            &state.persistence,
            elapsed_score_ticks,
        );
    }
    // The match's own verdict presentation (`[EndGameScreens]`): the country's
    // banner picture on the battlefield and its banner music start together,
    // and both answer to the in-game switch. This reads the banner half only;
    // the score page reads its own `Score*` half (`build_score_screen_model`).

    let entry = local_end_game_entry(&state.frontend, &state.match_state, outcome);
    let banner = entry.as_ref().and_then(|entry| entry.banner_art());
    // Tactical SetScreenText (6D4DB0) precedes the wait and GameExit.
    // VERA can service both before drawing. Hold a visible skirmish on the
    // battlefield if the last presented picture omitted this verdict.
    // A custom country's entry still holds even if it has no picture.
    let custom_banner = state.persistence.options_profile.custom_end_game_banner && entry.is_some();
    let visible_skirmish = state.match_state.session_mode() == SessionMode::Skirmish
        && !state.platform.window_hidden
        && crate::app::presentation::outcome_banner::visible_verdict(&state.match_state)
            == Some(outcome);
    let hold_banner = state
        .match_state
        .match_presentation
        .outcome_banner_presentation
        .hold_on_exit(outcome, custom_banner, visible_skirmish);
    log::info!(
        "Match end: verdict entry={} card={} hold_banner={hold_banner}",
        entry.is_some(),
        banner.is_some(),
    );
    // The theme belongs to the verdict, not to the card. It starts here, after
    // any clip the result waited for: a playing clip holds the music, and a
    // held stream refuses a new song (`PlayFile @ 0x00407B60`). One beat later
    // the score page asks for its own `Score*` half, which is a no-op when
    // this name is already playing.
    start_verdict_theme(
        &state.frontend,
        &state.match_state,
        &state.process_assets,
        &mut state.audio,
        &state.persistence.options_profile,
        outcome,
        wall_ms,
    );
    let title = crate::app::match_runtime::scenario_exit::outcome_title(outcome);
    let detail = crate::app::match_runtime::scenario_exit::outcome_detail(outcome);
    let destination = if state.match_state.session.network_authored_start().is_some() {
        super::scenario_exit::ScenarioExitDestination::AuthoredCampaignResult(outcome)
    } else {
        super::scenario_exit::scenario_exit_destination(
            state.match_state.session_mode(),
            outcome,
            state.match_state.map_basic.skip_score.unwrap_or(false),
            cooperative,
            title,
            detail,
            || {
                build_campaign_score_screen_model(
                    &state.match_state,
                    &state.process_assets,
                    &state.persistence.options_profile,
                    elapsed_score_ticks,
                )
            },
            || {
                build_score_screen_model(
                    &state.frontend,
                    &state.match_state,
                    &state.process_assets,
                    &state.persistence.options_profile,
                    elapsed_seconds,
                    false,
                )
            },
        )
    };
    // Native campaign victories enter the standalone dialog `0x103`; campaign
    // defeats take a separate prompt path. Only GameMode!=0 takes the generic
    // `ScoreDialog__Run 0x5C9720` / SCORE-theme handoff.
    let cascade =
        crate::app::match_runtime::scenario_exit::ScenarioExitCascade::start_holding_banner(
            wall_ms,
            destination,
            hold_banner,
        )
        // The half the beat draws is decided here, once, and never re-derived:
        // `complete_outcome` clears the House's Won flag the moment the sim's
        // own Vox wait ends, which is long before this router runs. Native
        // keeps the choice in its screen text object for the same reason.
        .with_verdict(outcome);
    // The card has been up since the result was published. If a clip played
    // under it, that beat is already spent and the fade may start as soon as
    // the clip ends; a verdict with no clip still gets the full beat from
    // here.
    let cascade = if custom_banner
        && let Some(card_at) = crate::app::commander_feedback::verdict_card_shown_at_ms(
            &state.match_state.match_presentation,
        ) {
        cascade.with_banner_hold_until(
            card_at.saturating_add(crate::app::match_runtime::scenario_exit::BANNER_HOLD_MS),
        )
    } else {
        cascade
    };
    state.match_state.scenario_exit = Some(cascade);
}

/// The local player's own `[EndGameScreens]` entry for the result just
/// admitted: its picture and music, or `None` when the rules set names nothing
/// for this country and half.
fn local_end_game_entry<'a>(
    frontend: &'a FrontendState,
    match_state: &'a MatchState,
    outcome: crate::sim::house_state::HouseOutcomeKind,
) -> Option<crate::rules::end_game::EndGamePresentation<'a>> {
    let sim = &match_state.sim_runtime.runtime()?.simulation;
    // A mission owns its own result screens; the match card is a match's.
    if sim.session().uses_campaign_world() {
        return None;
    }
    let house = sim
        .session()
        .current_house()
        .and_then(|owner| sim.houses().get(&owner))?;
    let kind = match outcome {
        crate::sim::house_state::HouseOutcomeKind::Victory => {
            crate::rules::end_game::EndGameKind::Victory
        }
        crate::sim::house_state::HouseOutcomeKind::Defeat => {
            crate::rules::end_game::EndGameKind::Defeat
        }
    };
    let country = sim.resolve(house.country()?);
    crate::app::state::active_rules(frontend, match_state)?
        .end_game_screens()
        .presentation(country, kind)
}

/// The battlefield's own theme for the result (`[EndGameScreens] *Music=`).
///
/// It runs when the verdict is taken, after any clip the result waited for,
/// one beat before the score page asks for its own `Score*` half.
pub(crate) fn start_verdict_theme(
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    profile: &RetailOptionsProfile,
    outcome: crate::sim::house_state::HouseOutcomeKind,
    wall_ms: u64,
) {
    if !profile.custom_end_game_banner {
        return;
    }
    let theme = local_end_game_entry(frontend, match_state, outcome)
        .and_then(|entry| entry.banner_music().map(str::to_owned));
    let Some(theme) = theme else {
        return;
    };
    let Some(assets) = process_assets.manager() else {
        return;
    };
    audio.play_theme(&theme, assets, wall_ms);
    log::info!("Match end: the battlefield theme switches to '{theme}'");
}

/// The result's own EVA line, spoken through the shared `VoxClass` owner.
///
/// `HouseClass::Flag_To_Win 0x004FCBA9` / `Flag_To_Lose 0x004FCDA1` say it with
/// `EDX = -1`; [`crate::app::match_runtime::sound_dispatch::outcome_eva_event`]
/// owns that selection for the simulation's event, and this replays the same
/// selection when the frame's delivery withheld the original for a clip
/// ([`crate::app::commander_feedback::withholds_verdict_line`]).
pub(crate) fn play_outcome_eva(
    sim_runtime: &SimHost,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    outcome: crate::sim::house_state::HouseOutcomeKind,
) {
    let campaign = sim_runtime
        .runtime()
        .is_none_or(|runtime| runtime.simulation.session().uses_campaign_world());
    let event = crate::app::match_runtime::sound_dispatch::outcome_eva_event(outcome, campaign);
    let (Some(sfx), Some(catalog), Some(assets)) = (
        audio.sfx_player.as_mut(),
        process_assets.audio_catalog(),
        process_assets.manager(),
    ) else {
        return;
    };
    if sfx.play_eva(event, None, catalog.eva(), assets, catalog.index()) {
        log::info!("Match end: the verdict speaks '{event}'");
    } else {
        log::warn!("Match end: '{event}' was not admitted; the verdict stays silent");
    }
}

fn build_campaign_score_screen_model(
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    profile: &RetailOptionsProfile,
    elapsed_score_ticks: i32,
) -> crate::ui::campaign_score_shell::CampaignScoreScreenModel {
    let difficulty = match_state
        .sim_runtime
        .runtime()
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
        .unwrap_or(profile.difficulty);
    let houses = match_state.sim_runtime.runtime().map(|runtime| {
        runtime
            .simulation
            .houses()
            .values()
            .map(|house| (house.player_control(), house.stats().kills()))
            .collect::<Vec<_>>()
    });
    let side = match_state
        .sim_runtime
        .runtime()
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
        &match_state.map_ranking,
        houses.unwrap_or_default(),
        side,
        |key| resolve_campaign_score_label(process_assets, key),
    );
    model.theme = process_assets
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

fn resolve_campaign_score_label(process_assets: &ProcessAssets, native_key: &str) -> String {
    process_assets.csf_text(native_key)
}

fn format_campaign_score_time(total_seconds: i32) -> String {
    let hours = total_seconds / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

#[cfg(all(test, tests_app_match))]
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
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    options_profile: &RetailOptionsProfile,
    elapsed_seconds: i32,
    aborted: bool,
) -> crate::ui::score_shell::ScoreScreenModel {
    use crate::ui::score_shell::ScoreScreenModel;

    let Some(sim) = match_state.sim_runtime.runtime().map(|rt| &rt.simulation) else {
        return ScoreScreenModel::default();
    };
    // A natural exit reads the snapshot the terminal edge latched. An aborted
    // match never reached that edge, so it reads the live rows instead: the
    // page still shows what the player earned, and leaving consumes none of
    // the Scenario RNG draws the latch owns.
    let snapshot = if aborted {
        sim.live_score_snapshot()
    } else {
        let Some(snapshot) = sim.terminal_score_snapshot() else {
            log::error!("Natural match exit reached the score screen without a sim snapshot");
            return ScoreScreenModel::default();
        };
        snapshot.clone()
    };
    let raw_snapshot = &snapshot;
    let computer = process_assets.csf_text("TXT_COMPUTER");
    let rows = resolve_score_rows(
        sim,
        raw_snapshot,
        &crate::app::presentation::house_ui::HouseUiSources::from_state(
            frontend,
            match_state,
            &process_assets.csf,
            &computer,
        ),
        |country| {
            crate::app::presentation::house_ui::country_display_name(
                frontend,
                match_state,
                &process_assets.csf,
                sim,
                country,
            )
        },
    );
    // Scenario +0x34B8: the side of session player 0's country, the local
    // player in a skirmish (0x0068779A..0x0068782D).
    let local_owner = crate::app::input::commands::preferred_local_owner_name(match_state)
        .as_deref()
        .and_then(|owner| sim.interner().get(owner));
    let local_house = local_owner.and_then(|owner| sim.houses().get(&owner));
    let side = local_house.map_or(0, |house| house.side_index());
    // Leaving the match is a defeat: the player ended it, so the lose half of
    // the side's theme and of `[EndGameScreens]` is the one that fits. A match
    // that ended by itself keeps its own flag.
    let defeated = aborted || local_house.is_some_and(|house| house.is_defeated());
    // `Global_DrawScoreScreen_ScoreTheme` (0x1002F4B0, patch site 0x5C9B75)
    // selects the side's win or lose theme on the local player's
    // HouseClass+0x1F5 defeated flag; the literal it otherwise pushes is
    // 0x0082FEBC "SCORE". It keeps that literal when the local player is the
    // Observer house (`Player [0xA83D4C] == Observer [0xAC1198]`,
    // 0x1002F4B8..0x1002F4C7).
    let observing = local_owner.is_some_and(|owner| sim.session().is_observer_house(owner));
    let theme = (!observing)
        .then(|| {
            process_assets.side_score_theme(
                side,
                crate::rules::ruleset::ScoreTheme::Multiplayer { defeated },
            )
        })
        .flatten()
        .map(str::to_owned);
    // `[EndGameScreens]`: the local country's own music and picture, when the
    // rules set names one. The music replaces the side's theme; the picture
    // replaces the side art in `open_score_page`. The defeat flag that picks
    // the half is the same one the native theme selection reads, so an observer
    // — which has no result of its own — keeps the retail page.
    //
    // This is the *score page's* switch (`[VERA] CustomEndGameScore=`,
    // Options' Custom End Game Score Screen): its background and its theme
    // move together. The battlefield verdict — a picture over the tactical
    // view a beat earlier — answers to its own switch and its own keys
    // (`VictoryArt=`/`VictoryMusic=`), resolved in
    // `drive_local_player_outcome_exit`, so a player can keep one and drop the
    // other.
    //
    // The score page reads the `Score*` half of the same entry, and borrows
    // nothing from the banner half: that half is a *battlefield* picture (the
    // RA3 end-game cards are banners there too), so showing it behind the
    // score columns is exactly the mix-up this split undoes. A rules set that
    // wrote only the banner half therefore leaves `background` empty and
    // `theme` on the side's, which is the retail page.
    let custom_score = options_profile.custom_end_game_score;
    let (country_music, background) = (!observing)
        .then(|| {
            let house = local_house?;
            let kind = if defeated {
                crate::rules::end_game::EndGameKind::Defeat
            } else {
                crate::rules::end_game::EndGameKind::Victory
            };
            let country = sim.resolve(house.country()?);
            let presentation = crate::app::state::active_rules(frontend, match_state)?
                .end_game_screens()
                .presentation(country, kind)?;
            let background = if custom_score {
                presentation
                    .score_art()
                    .map(|art| crate::ui::score_shell::ScoreBackground {
                        shape: art.shape,
                        palette: art.palette,
                    })
            } else {
                None
            };
            let music = if custom_score {
                presentation.score_music().map(str::to_owned)
            } else {
                None
            };
            Some((music, background))
        })
        .flatten()
        .unwrap_or_default();

    ScoreScreenModel {
        title_key: score_title_key(match_state.session_mode()),
        game_number: frontend.finished_game_count,
        // The clock performs native's signed division first. The existing UI
        // model is unsigned and applies the native 99:59:59 ceiling, so keep
        // the pathological rollover representation local to this boundary.
        elapsed_seconds: elapsed_seconds as u32,
        side,
        // The country's own music outranks the side's theme; the country's
        // picture replaces the side art in `open_score_page`.
        theme: country_music.or(theme),
        background,
        rows,
    }
}

/// The score page for a match the player left on purpose.
///
/// Retail returns to the shell; the score a skirmish has earned up to that
/// point is still meaningful, and the games this mode follows (RA3, SC2, AoE2)
/// show it after a resign. The rows come from [`Simulation::live_score_snapshot`]
/// — the raw counters, no survivor bonus — because an abandoned match has no
/// terminal edge to latch and leaving must not advance the Scenario RNG.
pub(crate) fn build_aborted_score_screen_model(
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    options_profile: &RetailOptionsProfile,
    elapsed_seconds: i32,
) -> crate::ui::score_shell::ScoreScreenModel {
    build_score_screen_model(
        frontend,
        match_state,
        process_assets,
        options_profile,
        elapsed_seconds,
        true,
    )
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
/// `MatchState::paused` includes VERA's in-game-menu state, the developer
/// pause and the synchronized online pause. The menu uses gamemd's pause:
/// `State_Machine @ 0x0048C8B0` brackets its entire
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
pub(crate) fn pump_audio_service(
    platform: &crate::app::PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &crate::app::persistence::PersistenceState,
    now_ms: u64,
) {
    // `Play_Movie @ 0x005BED40`'s Bink loop (`0x00432C70`) never reaches
    // `Network_ServiceLoop`, so a full-screen movie runs no pump: Theme AI
    // does not move on past the held song, and only the outputs run.
    if frontend.fullscreen_movie.is_some() {
        service_audio_outputs(&frontend.screen, match_state, audio, now_ms);
        return;
    }
    crate::app::presentation::building_anim::refresh_coordinate_sound_gains(
        platform,
        renderer,
        &frontend.screen,
        match_state,
        process_assets,
        &mut audio.sfx_player,
        persistence,
    );
    // The process audio service remains live while the loading job holds the
    // asset-manager lease. Retained simulations still own Main on the shell;
    // only the process before its first scenario uses the frontend cursor.
    //Full-screen5BED40 acquires its own shared movie hold synchronously.
    //It does not also call the scenario/menu pause path684140.
    let paused = match_state.presentation_paused(&frontend.screen);
    let assets = crate::app::loading::pump::audio_service_asset_manager(
        process_assets,
        frontend.loading_session.as_ref(),
    );
    let mut main = crate::app::state::process_main_draws(&frontend.frontend_main_rng);
    audio.service_audio(
        now_ms,
        paused,
        assets,
        process_assets.audio_catalog(),
        &mut main,
    );
}

/// The per-pass audio work outside the pump, for the passes whose pump belongs
/// to Main_Tick: the pause state, the master fades and the device queues.
/// Native applies the pause synchronously (`GamePause::Enter @ 0x00406F00` /
/// `Exit`) and ticks the master InterpGroups on its own audio thread
/// (`0x004095B0`), neither through the pump.
pub(crate) fn service_audio_outputs(
    screen: &GameScreen,
    match_state: &MatchState,
    audio: &mut AppAudioRuntime,
    now_ms: u64,
) {
    let paused = match_state.presentation_paused(screen);
    audio.service_audio_outputs(now_ms, paused);
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
/// own loops, so those passes keep the per-pass pump. So does a published
/// result the exit router leaves waiting for a commander clip: no Main_Tick
/// runs while it waits (`App::service_match_runtime_once`).
pub(crate) fn main_tick_owns_audio_pump(
    screen: &GameScreen,
    match_state: &MatchState,
    simulation_allowed: bool,
) -> bool {
    audio_pump_follows_main_tick(
        match_state.session_mode(),
        *screen == GameScreen::InGame
            && simulation_allowed
            && match_state.startup.admits_ordinary_tick(),
        match_state.match_presentation.in_game_menu.is_open()
            || match_state.debug_pause
            || match_state.scenario_exit.is_some()
            || match_state.scenario_outcome.is_some()
            || crate::app::App::local_outcome_exit_ready(match_state),
    )
}

/// [`main_tick_owns_audio_pump`]'s decision: an admitted match in
/// `g_GameMode` 0 or 5, on a pass no loop of its own pumps.
fn audio_pump_follows_main_tick(mode: SessionMode, match_admitted: bool, own_loop: bool) -> bool {
    mode.is_single_player() && match_admitted && !own_loop
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
/// `ProcessModalServicePump @ 0x00623120` runs Main_Tick behind a modal only
/// outside `g_GameMode` 0 and 5; offline, it services messages and repaints
/// with the world frozen. Its other blockers, the lobby byte `0x00A8D60E`,
/// the counter `0x00A8DAB4` and Main_Tick's reentrancy byte `0x00ABCD58`
/// (read by `0x0055CBF0`), stay clear here: VERA's in-game modal pass never
/// runs inside Main_Tick, event execution or the lobby.
fn wall_clock_service_admission(
    paused: bool,
    modal_open: bool,
    mode: SessionMode,
) -> WallClockServiceAdmission {
    let tactical_mutation = !modal_open || mode.is_network();
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
    /// (`app::state::focus_freeze_active`).
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
    /// Main_Tick's input poll, which writes the right-drag latch.
    pub(crate) scroll_input: bool,
    /// The camera may take the scroll steps its own clock owes this pass
    /// (`input::camera`): `scroll_input`'s lifecycle without the pacer's timing.
    pub(crate) camera_scroll: bool,
    /// Game frames are running, whatever this pass's timing. Only then do
    /// Follow and a cinematic pan rewrite the view.
    pub(crate) frames_running: bool,
    /// GScreen Input 4F4320 reaches Sidebar Action 6A7780 this pass.
    /// Main_Tick polls it (55D8AB) only with the window active (A8ED80) and no
    /// in-game menu (A8EDA0), online too, and returns before it while the
    /// scenario pauses (55D82E..55D877); Wait_For_Players polls it until
    /// dialog 0xEA opens (6497EC). Unlike `scroll_input`, no VERA policy
    /// widens it.
    pub(crate) sidebar_action: bool,
}

/// Decide one in-game runtime pass from raw predicates. This is the single
/// freeze contract for simulation admission and Sidebar Action's visits:
/// paused/menu redraws, focus-frozen windows, missing startup receipts, and
/// closed pacer windows all resolve to `run_sim: false`, and a committed
/// network-modal frame keeps its non-`Ordinary` lane. `sidebar_action_gate_matrix`
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
            camera_scroll: true,
            frames_running: true,
            sidebar_action: true,
        };
    }
    let service =
        wall_clock_service_admission(inputs.paused, inputs.menu_open, inputs.session_mode);
    let pacer_admitted = service.simulation && inputs.pacer_timing_admits;
    let scenario_blocks =
        super::scenario_pause::blocks_main_tick(inputs.session_mode, inputs.scenario_paused);
    let frame_gate =
        !inputs.focus_frozen && !scenario_blocks && inputs.startup_admitted && !inputs.network_wait;
    let run_sim = frame_gate && (inputs.frame_stepping || pacer_admitted);
    let tick_lane = if inputs.paused && inputs.session_mode.is_network() {
        TickLane::NetworkModal
    } else {
        TickLane::Ordinary
    };
    let input_gate = inputs.window_active
        && inputs.startup_admitted
        && service.tactical_mutation
        && !inputs.network_dialog_open;
    // VERA's developer-pause camera and the existing network policy.
    let input_between_frames =
        (inputs.paused && !inputs.menu_open) || inputs.session_mode.is_network();
    let scroll_input = input_gate && (run_sim || input_between_frames);
    RuntimePassDecision {
        run_sim,
        tick_lane,
        admitted_by_pacer: run_sim && !inputs.frame_stepping && pacer_admitted,
        tactical_mutation: service.tactical_mutation && !inputs.network_dialog_open,
        // Main_Tick polls GScreen input at 0x55D8AB. ThrottleFrame's offline
        // modes 0/5 skip its extra poll at 0x55E253, so a redraw during that
        // wait must not latch a right drag. Native executions:
        // tools/input_oracle/fast_scroll.json (throttle_cases).
        scroll_input,
        // Scroll steps run between game frames too. A scenario pause is the
        // stop that remains: Main_Tick returns before input (0x55D830..0x55D859).
        camera_scroll: input_gate && (!scenario_blocks || input_between_frames),
        frames_running: frame_gate && !inputs.paused,
        sidebar_action: inputs.window_active
            && inputs.startup_admitted
            && !inputs.menu_open
            && !inputs.network_dialog_open
            && !scenario_blocks,
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
    // A tick an earlier pass sent to the simulation thread finishes first,
    // so the step is counted on the live world, not the published frame.
    finish_threaded_frame(state);
    let (tick_before, binary_frame_before) = state
        .match_state
        .sim_runtime
        .runtime()
        .map(|rt| &rt.simulation)
        .map(|sim| (sim.session().tick(), sim.session().binary_frame()))
        .ok_or(ExactStepError::SimulationMissing)?;

    advance_in_game_runtime_mode(state, RuntimeAdvanceMode::ExactOneStep, admitted, false);
    let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
    state.platform.frame_pacer.reanchor(now_ms);

    let (tick_after, binary_frame_after) = state
        .match_state
        .sim_runtime
        .runtime()
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
    state
        .match_state
        .sync_multiplayer_pause_clock(crate::app::match_runtime::frame_pacer::wall_clock_ms());
    // ExecuteDoList64CD0C is a blocking message box inside this frame. Even
    // exact-step diagnostics must not admit another Logic pass while it waits.
    if super::network_failure::pending(&state.frontend.screen, &state.match_state) {
        super::network_failure::show(
            &state.platform,
            &mut state.frontend.shell_controller,
            &mut state.match_state,
        );
        return false;
    }
    if crate::app::App::house_outcome_wait_pending(&state.match_state.sim_runtime) {
        // House4F8649/4F8772 services48D080 inside this Main_Tick;
        // it never re-enters55D7C2's timed-pause/Chrono/input prelude.
        let completed = advance_one_simulation_frame(
            state,
            TickLane::Ordinary,
            None,
            FrameResume::OutcomeWait,
            false,
        );
        state
            .match_state
            .sync_multiplayer_pause_clock(crate::app::match_runtime::frame_pacer::wall_clock_ms());
        return completed;
    }
    crate::app::input::commands::reconcile_paid_super_weapon_cursor(&mut state.match_state);
    // VERA's canonical pause barrier is authoritative even for diagnostic
    // exact stepping. The derived predicate becomes true only after the
    // entire barrier frame, including a retained network transaction, finishes.
    // Transport and UI continue in the outer service loop.
    if state.match_state.multiplayer_paused()
        || state
            .match_state
            .match_presentation
            .narrative_sync
            .pending()
    {
        return false;
    }
    if !replaying {
        crate::app::input::build_repeat::top_up(
            &state.platform,
            &state.renderer,
            &state.diag,
            &state.frontend,
            &mut state.match_state,
            &state.process_assets,
            &mut state.audio.sfx_player,
            &state.persistence,
        );
    }
    let frame_stepping = matches!(mode, RuntimeAdvanceMode::WallClock { .. })
        && state.diag.debug_frame_step_requested;
    let resume_wait = state
        .match_state
        .sim_runtime
        .runtime()
        .is_some_and(|rt| rt.simulation.pending_network_wait());
    // Locally paced frames keep their due time; see `admitted_frame_start_ms`.
    let mut paced_frame_start_ms = None;
    let pacer_timing_admits = match mode {
        RuntimeAdvanceMode::WallClock { now_ms } => {
            // Pure timing consult; pause/menu blocking belongs to the decision.
            let (game_speed, network_fps) = local_pacing_inputs(&state.match_state);
            if let Some(live) = state
                .match_state
                .live_network
                .as_ref()
                .filter(|live| live.uses_sequenced_lockstep())
            {
                if live.has_terminal_network_error() {
                    true
                } else if let Some(runtime) = state.match_state.sim_runtime.runtime() {
                    let frame = runtime.simulation.session().binary_frame();
                    let inputs = online_pacing_inputs(
                        &state.platform,
                        &state.diag,
                        &state.match_state,
                        frame,
                        now_ms,
                    );
                    state.platform.frame_pacer.admits_online_frame(
                        crate::util::time::Instant::now(),
                        frame,
                        live.committed_frontier(),
                        inputs,
                        resume_wait,
                    )
                } else {
                    false
                }
            } else {
                let pacer = &state.platform.frame_pacer;
                paced_frame_start_ms =
                    Some(pacer.admitted_frame_start_ms(now_ms, game_speed, network_fps));
                pacer.should_admit(now_ms, game_speed, network_fps, false)
            }
        }
        RuntimeAdvanceMode::ExactOneStep => false,
    };
    let wall_ms = match mode {
        RuntimeAdvanceMode::WallClock { now_ms } => now_ms,
        RuntimeAdvanceMode::ExactOneStep => crate::app::match_runtime::frame_pacer::wall_clock_ms(),
    };
    let frame_start_ms = paced_frame_start_ms.unwrap_or(wall_ms);
    let service_live = matches!(mode, RuntimeAdvanceMode::WallClock { .. })
        && !replaying
        && !state.match_state.match_presentation.in_game_menu.is_open()
        && !state.match_state.debug_pause
        && !crate::app::state::focus_freeze_active(&state.platform, &state.match_state);
    if service_live {
        super::scenario_pause::poll(
            &state.platform,
            &state.renderer,
            &state.diag,
            &state.frontend,
            &mut state.match_state,
            &state.process_assets,
            &mut state.audio,
            &state.persistence,
            wall_ms,
        );
    }
    let scenario_paused = state
        .match_state
        .sim_runtime
        .runtime()
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
            pump_audio_service(
                &state.platform,
                &state.renderer,
                &state.frontend,
                &state.match_state,
                &state.process_assets,
                &mut state.audio,
                &state.persistence,
                wall_ms,
            );
        }
        super::scenario_pause::apply_chrono_screen_step(state);
        state
            .platform
            .frame_pacer
            .record_admitted_frame(frame_start_ms);
        // The early return still draws the tactical view (0x0055D84F).
        state
            .match_state
            .match_presentation
            .tactical_composite
            .note_main_tick();
    }
    let mut decision = decide_runtime_pass(RuntimePassInputs {
        exact_step: matches!(mode, RuntimeAdvanceMode::ExactOneStep),
        window_active: state.platform.window_active,
        focus_frozen: crate::app::state::focus_freeze_active(&state.platform, &state.match_state),
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
        let can_advance = state.match_state.match_diagnostics.playback_can_advance();
        decision.run_sim &= can_advance;
        decision.frames_running &= can_advance;
        decision.tactical_mutation = false;
        // Camera remains available when the last recorded frame is frozen.
        decision.scroll_input = state.platform.window_active
            && !crate::app::state::focus_freeze_active(&state.platform, &state.match_state);
        decision.camera_scroll = decision.scroll_input;
    }
    if frame_stepping {
        state.diag.debug_frame_step_requested = false;
        if let RuntimeAdvanceMode::WallClock { now_ms } = mode {
            state.platform.frame_pacer.reanchor(now_ms);
        }
    }

    // Scrolling runs on the camera's own clock so its speed never follows the
    // game speed. A diagnostic step is one exact poll and reads no wall clock.
    let scroll_steps = match mode {
        RuntimeAdvanceMode::WallClock { .. } => crate::app::input::camera::take_scroll_steps(
            &mut state.match_state.input,
            decision.camera_scroll,
            decision.frames_running,
            decision.run_sim,
        ),
        RuntimeAdvanceMode::ExactOneStep if decision.scroll_input => {
            crate::app::input::camera::ScrollSteps::Whole
        }
        RuntimeAdvanceMode::ExactOneStep => crate::app::input::camera::ScrollSteps::None,
    };
    // Sample the gesture once, before simulation. Apply its request at the
    // native Tactical__AI/pre-follow seam, after this frame's bounds writers.
    let mouse_scroll = crate::app::input::camera::sample_mouse_scroll(
        &state.platform,
        &state.renderer,
        &state.frontend,
        &mut state.match_state,
        &state.persistence,
        decision.scroll_input,
        scroll_steps,
    );
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
            state
                .platform
                .frame_pacer
                .record_admitted_frame(frame_start_ms);
        }
        // Which frames go to the simulation thread: [`frame_ticks_inline`]
        // (docs/sim-thread.md).
        let (game_speed, network_fps) = local_pacing_inputs(&state.match_state);
        let pacer = &state.platform.frame_pacer;
        let hashes_state = !replaying
            && state
                .match_state
                .sim_runtime
                .runtime()
                .is_some_and(|rt| rt.simulation.next_frame_hashes_state());
        let now = crate::util::time::Instant::now();
        // Online, the relay's frontier and the online pacer admit the next
        // frame (the catch-up loop in `App::service_match_runtime`).
        let another_pass_follows = match state
            .match_state
            .live_network
            .as_ref()
            .filter(|live| live.uses_sequenced_lockstep() && !live.has_terminal_network_error())
        {
            // A frame step has cleared its request above; it ticks inline
            // whatever this answers.
            Some(live) => state.match_state.sim_runtime.runtime().is_some_and(|rt| {
                let next_frame = rt.simulation.session().binary_frame().wrapping_add(1);
                pacer.another_online_pass_follows(
                    now,
                    next_frame,
                    live.committed_frontier(),
                    online_pacing_inputs(
                        &state.platform,
                        &state.diag,
                        &state.match_state,
                        next_frame,
                        wall_ms,
                    ),
                )
            }),
            None => pacer.another_pass_follows(
                now,
                crate::app::match_runtime::frame_pacer::wall_clock_ms(),
                game_speed,
                network_fps,
            ),
        };
        let inline = frame_ticks_inline(
            frame_stepping || matches!(mode, RuntimeAdvanceMode::ExactOneStep),
            super::sim_host::sim_thread_always(),
            hashes_state,
            another_pass_follows,
            state.match_state.sim_runtime.lend_cost(),
            pacer.expected_frame_cost(),
        );
        let debug_frames =
            if !frame_stepping && !replaying && debug_speed_allowed(&state.match_state) {
                state.platform.frame_pacer.debug_speed().saturating_sub(1)
            } else {
                0
            };
        let mut frame_committed = advance_one_simulation_frame(
            state,
            tick_lane,
            mouse_scroll,
            resume,
            inline || debug_frames > 0,
        );
        // Debug speed runs further frames in this paced pass, each through
        // the same frame transaction; the pacer interval is unchanged.
        for frame in 1..=debug_frames {
            // A stopped frame's world must not run again.
            if state.match_state.frame_stopped
                || state.match_state.scenario_outcome.is_some()
                || state.match_state.scenario_exit.is_some()
                || crate::app::App::local_outcome_exit_ready(&state.match_state)
                || crate::app::App::house_outcome_wait_pending(&state.match_state.sim_runtime)
            {
                break;
            }
            frame_committed |= advance_one_simulation_frame(
                state,
                tick_lane,
                None,
                FrameResume::None,
                inline || frame < debug_frames,
            );
        }
        // One Main_Tick, one tactical composite (RenderFrame 0x0055D8F2),
        // however many frames debug speed runs in this pass. A frame on the
        // simulation thread has nothing to show yet: its composite is noted
        // when it finishes ([`finish_threaded_frame`]), so its picture is
        // drawn once, not once now and again then.
        if !state.match_state.sim_runtime.is_lent() {
            state
                .match_state
                .match_presentation
                .tactical_composite
                .note_main_tick();
        }
        state
            .match_state
            .sync_multiplayer_pause_clock(crate::app::match_runtime::frame_pacer::wall_clock_ms());
        if crate::app::App::house_outcome_wait_pending(&state.match_state.sim_runtime) {
            return false;
        }
        if super::network_failure::pending(&state.frontend.screen, &state.match_state) {
            super::network_failure::show(
                &state.platform,
                &mut state.frontend.shell_controller,
                &mut state.match_state,
            );
            return false;
        }
        // A frame sent to the simulation thread has no result yet: the pass
        // that finishes it does this ([`finish_threaded_frame`]).
        if frame_committed || !state.match_state.sim_runtime.is_lent() {
            after_frame(
                &state.platform,
                &state.renderer,
                &state.diag,
                &state.frontend,
                &mut state.match_state,
                &state.process_assets,
                &mut state.audio,
                &state.persistence,
                frame_committed,
            );
        }
    } else if let Some(request) = mouse_scroll {
        crate::app::input::camera::commit_camera_scroll(
            &state.platform,
            &state.renderer,
            &state.frontend.screen,
            &mut state.match_state,
            &state.persistence,
            request,
        );
    }
    after_simulation_pass(
        &state.platform,
        &state.renderer,
        &state.diag,
        &state.frontend,
        &mut state.match_state,
        &state.process_assets,
        &mut state.audio,
        &state.persistence,
        decision,
        main_tick_pumps_audio,
        scroll_steps,
        replaying,
    )
}

/// The client work that follows one frame's result, committed or not.
fn after_frame(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &PersistenceState,
    frame_committed: bool,
) {
    // What the frame sent client modules waits for the next render frame
    // (`ui_modules`); taken first, before a draw captures the frame again.
    if frame_committed {
        crate::app::ui_modules::collect_sim_outbox(match_state);
    }
    #[cfg(feature = "telemetry-dashboard")]
    if frame_committed && diag.telemetry_active() {
        match_state.telemetry_tick_rate.record(Instant::now());
    }
    if frame_committed {
        // House4F92E2's strip pass reaches this frame as a pending
        // recheck; membership must not wait for the next player input.
        let recheck = match_state
            .match_presentation
            .sidebar_projection
            .model
            .cameo_recheck_pending();
        // Native Strip AI6A8CF3 requests redraw throughout the authored
        // flash window, even with no input. Reuse the one projection owner
        // only in these windows; ordinary idle ticks do not enumerate cameos.
        let flashing = match_state.sim_runtime.runtime().is_some_and(|runtime| {
            match_state
                .match_presentation
                .sidebar_projection
                .model
                .cameo_redraw_requested(runtime.simulation.session().binary_frame() as i32)
        });
        // StripDraw6A9540 asks each entry's availability and reads its
        // Factory while drawing, so the drawn state follows the simulation
        // even when no pass, input or flash window rebuilt the projection
        // this frame.
        if recheck || flashing {
            // The rebuilt view takes the same drawn state.
            crate::app::presentation::sidebar_render::refresh_sidebar_projection(
                platform,
                renderer,
                diag,
                frontend,
                match_state,
                process_assets,
                audio,
                persistence,
            );
        } else {
            crate::app::presentation::sidebar_render::refresh_sidebar_cameo_draw_state(match_state);
        }
        #[cfg(feature = "phobos")]
        crate::app::presentation::sw_sidebar::refresh_draw_state(match_state);
    }
}

/// Finish the frame the simulation thread ran since the last pass: its tail,
/// then the client work that follows a frame. Every pass starts with it, so
/// pause service, exit routing, menus and saves read a world whose last tick
/// the client has fully taken. Nothing happens when no tick is out.
pub(crate) fn finish_threaded_frame(state: &mut AppState) {
    let Some(frame_committed) = finish_threaded_tick(state, None) else {
        return;
    };
    // As after an inline frame: an outcome wait or a failure holds the rest.
    if crate::app::App::house_outcome_wait_pending(&state.match_state.sim_runtime)
        || super::network_failure::pending(&state.frontend.screen, &state.match_state)
    {
        return;
    }
    after_frame(
        &state.platform,
        &state.renderer,
        &state.diag,
        &state.frontend,
        &mut state.match_state,
        &state.process_assets,
        &mut state.audio,
        &state.persistence,
        frame_committed,
    );
    // What the pass that admitted the frame left to its result: the
    // Main_Tick's tactical composite, and its audio pump after Logic
    // (`0x0055DE4A`), so the tick's sounds start now and not when the next
    // frame is admitted.
    state
        .match_state
        .match_presentation
        .tactical_composite
        .note_main_tick();
    crate::app::presentation::building_anim::drain_sound_events(
        &state.platform,
        &state.renderer,
        &state.frontend.screen,
        &mut state.match_state,
        &state.process_assets,
        &mut state.audio.sfx_player,
        &state.persistence,
    );
    if main_tick_owns_audio_pump(&state.frontend.screen, &state.match_state, true) {
        pump_audio_service(
            &state.platform,
            &state.renderer,
            &state.frontend,
            &state.match_state,
            &state.process_assets,
            &mut state.audio,
            &state.persistence,
            super::frame_pacer::wall_clock_ms(),
        );
    }
}

/// A pass that finds a tick still on the simulation thread
/// (docs/sim-thread.md): it admits no frame, as a pass the pacer does not
/// admit, and takes nothing from the world, reading the frame published for
/// the tick. The camera takes the steps its own clock owes, so scrolling
/// keeps the picture rate however long the tick takes, and the per-pass
/// presentation services run.
pub(crate) fn service_beside_tick(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &PersistenceState,
) {
    if frontend.screen != GameScreen::InGame {
        return;
    }
    let replaying = match_state.match_diagnostics.is_replay_playback();
    let scenario_paused = match_state
        .sim_runtime
        .runtime()
        .is_some_and(|rt| rt.simulation.session().scenario_paused());
    let decision = decide_runtime_pass(RuntimePassInputs {
        exact_step: false,
        window_active: platform.window_active,
        focus_frozen: crate::app::state::focus_freeze_active(platform, match_state),
        startup_admitted: match_state.startup.admits_ordinary_tick(),
        frame_stepping: false,
        paused: match_state.paused(),
        scenario_paused: scenario_paused && !replaying,
        menu_open: match_state.match_presentation.in_game_menu.is_open(),
        session_mode: match_state.session_mode(),
        pacer_timing_admits: false,
        network_wait: match_state.network_wait.waiting(),
        network_dialog_open: match_state.network_wait.dialog_open(),
    });
    let scroll_steps = crate::app::input::camera::take_scroll_steps(
        &mut match_state.input,
        decision.camera_scroll,
        decision.frames_running,
        false,
    );
    if let Some(request) = crate::app::input::camera::sample_mouse_scroll(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
        decision.scroll_input,
        scroll_steps,
    ) {
        crate::app::input::camera::commit_camera_scroll(
            platform,
            renderer,
            &frontend.screen,
            match_state,
            persistence,
            request,
        );
    }
    // A held key release has not reached the keys yet: an arrow it lets go
    // must not keep scrolling until the tick is back.
    let keyboard_steps = if platform.window_events_held {
        crate::app::input::camera::ScrollSteps::None
    } else {
        scroll_steps
    };
    after_simulation_pass(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
        decision,
        false,
        keyboard_steps,
        replaying,
    );
}

/// Move the simulation's pending radar-terrain marks into the minimap's
/// backlog. Without a minimap they stay with the simulation.
pub(crate) fn drain_radar_terrain_marks(match_state: &mut super::state::MatchState) {
    let Some(minimap) = match_state.match_presentation.minimap.as_mut() else {
        return;
    };
    if let Some(cells) = match_state.sim_runtime.take_radar_terrain_marks() {
        minimap.queue_radar_terrain_cells(cells);
    }
}

/// What every pass does after its simulation step, whether or not a frame
/// ran.
fn after_simulation_pass(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &PersistenceState,
    decision: RuntimePassDecision,
    main_tick_pumps_audio: bool,
    scroll_steps: crate::app::input::camera::ScrollSteps,
    replaying: bool,
) -> bool {
    // MainThrottle55E33B publishes Logic visits per60 wall-clock buckets,
    // including the path where the pacer admitted no simulation frame.
    let wall_ms = crate::app::diagnostics::state::radar_presentation_ms(diag, match_state);
    match_state
        .match_presentation
        .detail
        .borrow_mut()
        .throttle_tail(wall_ms);

    // Ordered native source/global operations were applied with the frame
    // output; they alone relight cells. Follow the detail option.
    refresh_cell_lighting(match_state);

    let sidebar_visits = crate::app::presentation::sidebar_gadgets::current_sidebar_action_visits(
        match_state,
        decision.sidebar_action,
    );
    crate::app::presentation::building_anim::update_power_bar_anim(
        platform,
        renderer,
        &frontend.screen,
        match_state,
        persistence,
        wall_ms,
        sidebar_visits,
    );
    crate::app::presentation::sidebar_render::advance_sidebar_credits(
        frontend,
        match_state,
        sidebar_visits,
    );
    crate::app::presentation::sidebar_gadgets::update_sidebar_gadget_state(
        frontend,
        match_state,
        sidebar_visits,
    );
    // Every unlocked pass republishes the complete sidebar projection,
    // including the retained House power pair the tooltip driver reads.
    if match_state
        .match_presentation
        .sidebar_projection
        .view()
        .is_some()
        && !crate::app::input::campaign_input::is_locked(match_state)
    {
        crate::app::presentation::sidebar_render::republish_sidebar_projection(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
        );
        crate::app::input::chrome_input::refresh_pointer(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            persistence,
        );
    }
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
    crate::app::presentation::building_anim::drain_sound_events(
        platform,
        renderer,
        &frontend.screen,
        match_state,
        process_assets,
        &mut audio.sfx_player,
        persistence,
    );
    let in_game_music = persistence.options_profile.in_game_music;
    audio.main_tick_theme(in_game_music, music_now_ms);
    // Main_Tick's Network_ServiceLoop after Logic (`0x0055DE4A`). Logic's
    // sounds reach the arbiter in the drain above, so they start in this
    // same Main_Tick, as native's direct submissions do. A frame on the
    // simulation thread has delivered none yet: its pump runs when it
    // finishes ([`finish_threaded_frame`]).
    if decision.run_sim && main_tick_pumps_audio && !match_state.sim_runtime.is_lent() {
        pump_audio_service(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            music_now_ms,
        );
    }
    crate::app::input::camera::queue_keyboard_scroll(match_state, scroll_steps);
    if decision.tactical_mutation || replaying {
        crate::app::input::camera::animate_zoom(
            platform,
            renderer,
            &frontend.screen,
            match_state,
            persistence,
        );
    }
    if decision.tactical_mutation {
        update_building_placement_preview(
            platform,
            renderer,
            frontend,
            match_state,
            persistence,
            FoundationCentre::Tactical,
        );
    }
    decision.run_sim
}

/// Whether an admitted frame ticks on this thread instead of going to the
/// simulation thread (docs/sim-thread.md). A frame goes only when that saves
/// this thread time: one expected to cost less here than the last lent one
/// did (`lend_cost`: its copy, drop and any wait) ticks inline. A frame that
/// another pass of the service follows (`another_pass_follows`) ticks inline
/// too unless lending it is measured cheaper. A frame that hashes its state
/// for the replay log always goes: it costs far more than the frames around
/// it, whose cost the estimate follows, and its tick is known before it runs.
fn frame_ticks_inline(
    must_tick_inline: bool,
    always_lend: bool,
    hashes_state: bool,
    another_pass_follows: bool,
    lend_cost: Option<std::time::Duration>,
    expected_cost: std::time::Duration,
) -> bool {
    let lending_is_cheaper = lend_cost.is_some_and(|lent| expected_cost > lent);
    let worth_lending = always_lend || hashes_state || lend_cost.is_none() || lending_is_cheaper;
    must_tick_inline
        || !worth_lending
        || (another_pass_follows && !(lending_is_cheaper || hashes_state))
}

/// How often an offline match hashes its state for the replay log: every
/// 300th tick, about five seconds at GameSpeed 1, and after the frame that
/// ends the match. Only the replay log's diagnostic checks read it, and
/// replay verification compares only the ticks the log recorded; on a large
/// map with many houses one hash costs more than a frame's budget. Network
/// matches keep [`crate::sim::replay::PLAY_HASH_INTERVAL`].
const OFFLINE_REPLAY_HASH_INTERVAL: std::num::NonZeroU64 = std::num::NonZeroU64::new(300).unwrap();

/// The replay-log hash interval of normal play: offline or `network`.
fn play_hash_interval(network: bool) -> std::num::NonZeroU64 {
    if network {
        crate::sim::replay::PLAY_HASH_INTERVAL
    } else {
        OFFLINE_REPLAY_HASH_INTERVAL
    }
}

/// Game speed and network FPS for the local frame timer.
/// Relay-sequenced play uses the online pacer.
pub(crate) fn local_pacing_inputs(match_state: &MatchState) -> (u8, Option<u16>) {
    let game_speed = match_state
        .sim_runtime
        .runtime()
        .map(|rt| &rt.simulation)
        .map_or_else(
            || {
                crate::sim::game_options::clamp_game_speed(
                    match_state.match_presentation.in_game_options.game_speed as i32,
                )
            },
            |sim| {
                crate::sim::game_options::clamp_game_speed(sim.session().game_options().game_speed)
            },
        );
    (game_speed, super::network_match::frame_budget(match_state))
}

/// The online pacer's inputs for canonical `frame`: the relay's timing and
/// whether `frame` is being recovered. `None` while the match is held
/// (paused, focus-frozen, scenario-paused, replaying or frame-stepping) or
/// before the relay's timing arrives; the committed frontier alone then
/// admits frames (`LocalFramePacer::admits_canonical_frame`). The pass, the
/// lend decision, the catch-up service and the render continuation all ask
/// this; each keeps its own policy for a terminal network error.
pub(crate) fn online_pacing_inputs(
    platform: &crate::app::PlatformState,
    diag: &crate::app::diagnostics::state::DiagnosticsState,
    match_state: &MatchState,
    frame: u32,
    now_ms: u64,
) -> Option<(crate::net::live_commands::SequencedTiming, bool)> {
    let live = match_state.live_network.as_ref()?;
    let session = match_state.sim_runtime.runtime()?.simulation.session();
    if match_state.paused()
        || crate::app::state::focus_freeze_active(platform, match_state)
        || session.scenario_paused()
        || match_state.match_diagnostics.is_replay_playback()
        || diag.debug_frame_step_requested
    {
        return None;
    }
    let timing = live.sequenced_timing()?;
    Some((timing, live.recovery_status(now_ms as u32, frame).is_some()))
}

/// Debug speed is limited to an offline Skirmish; online, LAN and campaign
/// play keep one frame per paced interval.
pub(crate) fn debug_speed_allowed(match_state: &MatchState) -> bool {
    match_state.session_mode() == super::session::SessionMode::Skirmish
        && match_state.live_network.is_none()
}

fn should_record_replay_tick(tick_result: &crate::sim::world::TickResult) -> bool {
    // The frame tail always emits a hash on a completed terminal frame,
    // including Cooperative's result without a score snapshot. Suspended
    // prefixes leave both the frame commit and hash for the resumed tail.
    tick_result.frame_committed || tick_result.state_hash.is_some()
}

fn record_replay_frame(log: Option<&mut ReplayLog>, output: &mut SimFrameOutput) {
    if let Some(log) = log
        && should_record_replay_tick(&output.tick)
    {
        log.record_tick(
            output.tick.tick,
            output.take_admitted_commands(),
            output.tick.state_hash,
        );
    }
}

/// Complete the exact frame suspended by ExecuteDoList's message box. Reuse
/// every output consumer below; neither input admission nor Logic runs again.
pub(crate) fn resume_after_network_failure(state: &mut AppState) -> bool {
    advance_one_simulation_frame(
        state,
        TickLane::Ordinary,
        None,
        FrameResume::ChecksumModal,
        false,
    )
}

pub(crate) fn leave_network_wait(state: &mut AppState) -> bool {
    advance_one_simulation_frame(
        state,
        TickLane::Ordinary,
        None,
        FrameResume::NetworkLeave,
        false,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameResume {
    None,
    ChecksumModal,
    NetworkWait,
    NetworkLeave,
    OutcomeWait,
    /// Queue_AI of a network frame whose Logic ran on the simulation thread
    /// (`DeferredClientEffects::hands_off_queue`). It resumes as a network
    /// wait does; the thread's part already counted the frame's cost.
    QueueHandoff,
}

impl FrameResume {
    /// Run the frame this resume names with the caller's effects.
    fn run(
        self,
        rt: &mut crate::sim::runtime::SimRuntime,
        due_commands: &[crate::sim::command::CommandEnvelope],
        tick_lane: TickLane,
        effects: &mut FrameEffects<'_>,
    ) -> Result<SimFrameOutput, crate::sim::world::FrameAdvanceError> {
        match self {
            Self::ChecksumModal => rt.resume_after_network_failure(effects),
            Self::NetworkWait | Self::QueueHandoff => rt.resume_network_wait(effects),
            Self::NetworkLeave => rt.leave_network_wait(effects),
            Self::OutcomeWait => rt.resume_outcome_wait(effects),
            Self::None => rt.advance_frame(due_commands, SIM_TICK_MS, tick_lane, effects),
        }
    }
}

/// House+56F9, the House colour an Ares `Beam.IsHouseColor` birth keeps
/// (TechnoClass_FireRadBeam1, Ares.dll 10056010), through the shared
/// colour-map lookup the other House+56F9 readers use.
fn house_rgb(
    runtime: Option<&crate::sim::runtime::SimRuntime>,
    colors: &crate::map::houses::HouseColorMap,
    owner: crate::sim::intern::InternedId,
) -> [u8; 3] {
    let Some(runtime) = runtime else {
        return [0; 3];
    };
    let name = runtime.simulation.interner().resolve(owner);
    let index = colors
        .get(name)
        .copied()
        .unwrap_or(crate::rules::house_colors::NO_REMAP);
    crate::render::palette_light::house_color_rgb(&runtime.resources.rules.house_color_ramps, index)
}

/// House+56FC, the laser color a `LaserColor::House` or `Beam.IsHouseColor`
/// birth resolves through.
fn house_laser_rgb(
    runtime: Option<&crate::sim::runtime::SimRuntime>,
    colors: &crate::map::houses::HouseColorMap,
    owner: crate::sim::intern::InternedId,
) -> [u8; 3] {
    let Some(runtime) = runtime else {
        return [0; 3];
    };
    let name = runtime.simulation.interner().resolve(owner);
    // CreateHouses gives the two literal special houses InitColor
    // (0x688320/0x688373, which writes +56F9 only) but no ComputeRemap, so
    // the constructor's white +56FC..+56FE (0x4F5C2E..0x4F5C54) stays.
    if !runtime.simulation.session().uses_campaign_world()
        && (name.eq_ignore_ascii_case("Neutral") || name.eq_ignore_ascii_case("Special"))
    {
        return [0xFF; 3];
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
}

fn advance_one_simulation_frame(
    state: &mut AppState,
    tick_lane: TickLane,
    mouse_scroll: Option<(f32, f32)>,
    resume: FrameResume,
    // Whether the frame must end inside the call: an exact step or a debug
    // frame step never goes to the simulation thread.
    inline: bool,
) -> bool {
    // docs/sim-thread.md phase 4: the tick the simulation thread ran since
    // the last pass finishes first, so this frame starts from its results.
    // That tail takes this pass's camera scroll.
    let threaded_finished = finish_threaded_tick(state, mouse_scroll);
    let mouse_scroll = if threaded_finished.is_some() {
        None
    } else {
        mouse_scroll
    };
    if threaded_finished.is_some()
        && (crate::app::App::house_outcome_wait_pending(&state.match_state.sim_runtime)
            || super::network_failure::pending(&state.frontend.screen, &state.match_state)
            || state.match_state.frame_stopped
            || pending_network_wait(&state.match_state.sim_runtime))
    {
        // The next pass resumes that frame inline.
        return threaded_finished.unwrap_or(false);
    }
    let replaying = state.match_state.match_diagnostics.is_replay_playback();
    let resuming = resume != FrameResume::None;
    // MainTick55D3DA..55DE3A: a House outcome modal can suspend Logic before
    // Queue_AI. Keep its start with the live channel until that boundary, then
    // take it once; subsequent network-wait polls do not add samples.
    if !resuming && let Some(live) = state.match_state.live_network.as_mut() {
        live.begin_processing(super::frame_pacer::wall_clock_ms() as u32);
    }
    // With `Custom End Game Movie` on, the result's own EVA line waits until
    // no commander clip is in its way. See
    // [`super::FrameClientBoundary::with_deferred_verdict`].
    let defer_verdict_to_clip = crate::app::commander_feedback::withholds_verdict_line(
        &state.frontend,
        &mut state.match_state,
        &state.persistence.options_profile,
    );
    // Trigger definitions are runtime-bound (F07), so a live runtime is the
    // only activity source; the old trigger-only fixture mode is unrepresentable.
    let runtime_active = state.match_state.sim_runtime.is_installed();
    if !runtime_active {
        return threaded_finished.unwrap_or(false);
    }
    // Input QueueVoice requests precede this Logic pass. This shared consumer
    // only admits queued requests; positional redrive/EVA service remain at
    // their existing owners, and voices wait for actual Techno-head markers.
    if !resuming {
        crate::app::presentation::building_anim::drain_pending_sound_events(
            &state.platform,
            &state.renderer,
            &state.frontend.screen,
            &mut state.match_state,
            &state.process_assets,
            &mut state.audio.sfx_player,
            &state.persistence,
        );
    }

    prepare_replay_recording(&mut state.match_state, &state.process_assets);

    let unit_voice_owners = state
        .audio
        .sfx_player
        .as_ref()
        .map(|sfx| sfx.pending_unit_voice_owners())
        .unwrap_or_default();
    // Compute local owner before mutable borrow of simulation.
    let local_owner_for_fog = preferred_local_owner_name(&state.match_state);
    // Cache local owner name before mutable sim borrow (avoids borrow conflict).
    let local_owner_name =
        crate::app::input::commands::preferred_local_owner_name(&state.match_state);
    let sound_listener = crate::app::presentation::building_anim::sound_listener(
        &state.platform,
        &state.renderer,
        &state.frontend.screen,
        &state.match_state,
        &state.persistence,
    );
    let session_mode = state.match_state.session_mode();
    let other_local_pause = state.match_state.local_paused();
    let rules_owner = state.process_assets.native_rules();
    let initial_save = if !resuming && tick_lane == TickLane::Ordinary {
        crate::app::persistence::commands::prepare_initial_campaign_save(
            &state.platform,
            &state.renderer,
            &state.frontend.screen,
            &state.match_state,
            &state.persistence,
            &state.audio,
            &state.diag,
            rules_owner,
        )
    } else {
        None
    };
    // VERA-only accessibility rows. The frame's synchronously delivered
    // sound prefix is played by FrameClientBoundary, whose pass also fills
    // this sink; the frame tail handles the suffix. Both must be merged so
    // an EVA row is never lost just because its voice played early.
    let detailed_eva_text = state.persistence.options_profile.detailed_eva_text;
    let prefix_eva_text = Vec::new();
    let fps_panel_shown = state.renderer.performance_overlay.is_visible();
    let observing_commands = state.match_state.match_diagnostics.observes_commands();
    // docs/sim-thread.md: a network frame lent to the simulation thread runs
    // there up to Queue_AI. A restore replays many frames per pass, each
    // waiting for the last, so it stays inline.
    let queue_handoff = state.match_state.sim_runtime.thread_enabled()
        && !inline
        && resume == FrameResume::None
        && tick_lane == TickLane::Ordinary
        && !replaying
        && !observing_commands
        && initial_save.is_none()
        && state
            .match_state
            .live_network
            .as_ref()
            .is_some_and(|live| !live.restoring_match());
    // docs/sim-thread.md: a frame that can run without the lent client
    // owners takes what it reads from them now and records its writes.
    let deferred = (queue_handoff
        || super::deferred_effects::DeferredClientEffects::applies(
            &state.match_state,
            replaying,
            observing_commands,
            initial_save.is_some(),
        ))
    .then(|| {
        super::deferred_effects::DeferredClientEffects::take(
            state.match_state.input.planning.active(),
            state.audio.sfx_player.as_mut(),
            &state.process_assets,
            &state.match_state.scenario_outcome,
            defer_verdict_to_clip,
            queue_handoff,
        )
    });
    // An ordinary frame runs on the simulation thread; drawing reads its last
    // published frame.
    let threaded = deferred.is_some()
        && !inline
        && resume == FrameResume::None
        && tick_lane == TickLane::Ordinary
        && state.match_state.sim_runtime.thread_enabled();
    if threaded {
        // The frame published for this tick shows every cell ticked so far;
        // their radar-terrain marks go to the minimap with it, since no draw
        // takes them while the tick is out (`App::drain_radar_terrain_marks`).
        drain_radar_terrain_marks(&mut state.match_state);
    }
    let hash_interval = play_hash_interval(state.match_state.live_network.is_some());
    let slow = crate::app::cncnet::SpawnSlowPc::of(&state.frontend);
    // A spawn test switch counts a tick this thread runs itself apart from
    // the service around it (`spawn_frame_cost`).
    let inline_cost = crate::app::spawn_frame_cost::begin(&state.frontend.external_progress);
    // A lent tick publishes the frame the last read captured: until `lend`
    // below, this dispatch writes nothing a frame records.
    let parts = if threaded {
        state.match_state.sim_runtime.lend_parts()
    } else {
        state.match_state.sim_runtime.tick_parts()
    };
    let Some((rt, frame_checksums)) = parts else {
        return threaded_finished.unwrap_or(false);
    };
    let sim = &mut rt.simulation;
    let native_frame = sim.session().binary_frame();
    if !resuming {
        sim.prepare_unit_voice_visits(unit_voice_owners);
    }
    // Normal play hashes its state once every `play_hash_interval` frames
    // and on the frame that ends the match, for the replay log.
    sim.hash_state_every(if replaying {
        std::num::NonZeroU64::MIN
    } else {
        hash_interval
    });
    // Report points need the ledger whether or not a dashboard is
    // attached; `publish_if_due` only runs with a client. Elsewhere
    // the performance panel's Most CPU row reads it while the panel shows.
    sim.set_frame_profiling(
        cfg!(feature = "telemetry-dashboard") || cfg!(feature = "perf") || fps_panel_shown,
    );
    // Delay-zero AnimClass construction can emit StartSound during the
    // final map-load sweep. Keep it until this first tactical drain;
    // `drain(..)` below still consumes every event exactly once.
    // A lent frame takes its due commands on the simulation thread, after
    // its frame is published: taken here, the published world would have a
    // selection or control group neither queued nor applied.
    let due_commands = if !threaded && !replaying && !resuming && tick_lane == TickLane::Ordinary {
        sim.take_due_commands()
    } else {
        Vec::new()
    };
    // Recorded whether or not a client is attached: the history ring
    // backfills a dashboard that connects mid-match.
    // A lent tick spends them on the simulation thread, the unreported
    // part before its cost is timed and the rest inside it.
    if !threaded {
        crate::app::cncnet::SpawnSlowPc::spend(slow.unreported);
    }
    let advance_started = crate::util::time::Instant::now();
    if !threaded {
        crate::app::cncnet::SpawnSlowPc::spend(slow.tick);
    }
    let production_visits = state
        .match_state
        .match_presentation
        .sidebar_projection
        .model
        .production_factory_visits(&rt.simulation);
    let mut finish = FrameFinish {
        native_frame,
        local_owner_for_fog,
        local_owner_name,
        defer_verdict_to_clip,
        detailed_eva_text,
        prefix_eva_text,
        deferred,
    };
    if threaded {
        let job = move |rt: &mut crate::sim::runtime::SimRuntime| -> super::sim_host::SimJobResult {
            crate::app::cncnet::SpawnSlowPc::spend(slow.unreported);
            let started = crate::util::time::Instant::now();
            crate::app::cncnet::SpawnSlowPc::spend(slow.tick);
            let logic_started_ms = super::frame_pacer::wall_clock_ms();
            let deferred = finish
                .deferred
                .as_mut()
                .expect("a threaded tick defers its client effects");
            let due_commands = rt.simulation.take_due_commands();
            let output = run_deferred_tick(
                rt,
                deferred,
                &production_visits,
                &due_commands,
                tick_lane,
                FrameResume::None,
            );
            Box::new(ThreadedTick {
                finish,
                output,
                tick_cost: started.elapsed(),
                logic_ms: (logic_started_ms, super::frame_pacer::wall_clock_ms()),
            })
        };
        // The passes and draws beside the tick show the placement preview and
        // the cursor's kept answers, which ask the live world. Answered now,
        // with the world home, they are one tick late at most however many
        // ticks in a row are lent; no draw may find the world home between.
        update_building_placement_preview(
            &state.platform,
            &state.renderer,
            &state.frontend,
            &mut state.match_state,
            &state.persistence,
            FoundationCentre::Tactical,
        );
        crate::app::input::cursor::answer_kept_questions(
            &state.platform,
            &state.renderer,
            &state.frontend,
            &state.match_state,
            &state.process_assets,
            &state.persistence,
        );
        state.match_state.sim_runtime.lend(Box::new(job));
        if let Some(request) = mouse_scroll {
            crate::app::input::camera::commit_camera_scroll(
                &state.platform,
                &state.renderer,
                &state.frontend.screen,
                &mut state.match_state,
                &state.persistence,
                request,
            );
        }
        return threaded_finished.unwrap_or(false);
    }
    // F07: the runtime is the one production frame API — bound
    // resources, no caller-substitutable inputs.
    let mut output = if let Some(deferred) = &mut finish.deferred {
        match run_deferred_tick(
            rt,
            deferred,
            &production_visits,
            &due_commands,
            tick_lane,
            resume,
        ) {
            Ok(output) => output,
            Err(error) => {
                stop_failed_frame(&mut state.match_state.frame_stopped, false, &error);
                return threaded_finished.unwrap_or(false);
            }
        }
    } else {
        let (replay_playback, observations) = state
            .match_state
            .match_diagnostics
            .playback_and_observations_mut();
        let mut observations = observations;
        let observing_commands = observations.is_some();
        let mut command_applied = |sim: &crate::sim::world::Simulation,
                                   record: &crate::sim::command::CommandRecord,
                                   applied: bool| {
            if let Some(log) = observations.as_deref_mut() {
                log.record(sim, record, applied);
            }
        };
        let active_tab = crate::app::persistence::commands::saved_active_tab(
            state.match_state.match_presentation.active_sidebar_tab,
        );
        let mut prepared = initial_save;
        let persistence = &mut state.persistence;
        let mut campaign_save =
            |sim: &crate::sim::world::Simulation,
             sidebar: &mut crate::app::sidebar_projection::SidebarModel,
             elapsed_clock: &mut super::frame_pacer::ScenarioElapsedClock| {
                if let Some(Ok(prepared)) = prepared.as_mut() {
                    // Strip6A8F42 clears requests before MainTick55DBCD.
                    // Preparation preceded the input prefix; snapshot the
                    // same sidebar owner again at the actual save boundary.
                    if let Some(owner) = sim.session().current_house() {
                        sidebar.reconcile_factory_links(sim, owner);
                    }
                    let order = sidebar
                        .saved_order()
                        .map(|order| order.with_active_tab(active_tab));
                    prepared.refresh_sidebar_order(order);
                }
                crate::app::persistence::commands::initial_campaign_save(
                    prepared.take(),
                    sim,
                    persistence,
                    elapsed_clock,
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
            minimap
                .as_mut()
                .is_some_and(|minimap| minimap.admit_radar_event(request, tick, Some(rules)))
        };
        let has_radar = &mut state.match_state.match_presentation.has_radar;
        let radar_anim = &mut state.match_state.match_presentation.radar_anim;
        let mut set_tactical_radar =
            |available: bool,
             sfx: Option<&mut crate::audio::sfx::SfxPlayer>,
             rules: &crate::rules::ruleset::RuleSet| {
                crate::app::presentation::building_anim::set_tactical_radar(
                    has_radar,
                    radar_anim.as_mut(),
                    sfx,
                    rules,
                    available,
                )
            };
        // Only Ares bounty or ProduceCashDisplay sums emit text, so this runs in every game.
        let flying_credits = &state.match_state.match_presentation.flying_credits;
        let mut queue_bounty =
            |sim: &crate::sim::world::Simulation,
             rules: &crate::rules::ruleset::RuleSet,
             text: crate::sim::bounty::BountyText| {
                flying_credits.borrow_mut().admit(sim, rules, text);
            };
        let mut boundary = super::frame_effects::FrameClientBoundary::new(
            playback,
            finish.local_owner_name.as_deref(),
            &mut state.match_state.match_audio.sound_events,
            &mut admit_radar,
        )
        .with_tactical_radar(&mut set_tactical_radar)
        .with_planning(
            &mut state.match_state.input.planning,
            &mut state.match_state.input.selection_state,
        )
        .with_latency_fudge(state.match_state.session.latency_fudge())
        .with_frame_checksums(frame_checksums)
        .with_live_network(state.match_state.live_network.as_mut())
        .with_scenario_pause(
            &mut state.match_state.match_presentation.scenario_pause,
            &mut state.match_state.scenario_elapsed_clock,
            session_mode,
            other_local_pause,
        )
        .with_sidebar(
            &mut state
                .match_state
                .match_presentation
                .sidebar_projection
                .model,
            Some(&mut campaign_save),
        )
        .with_outcome_wait(&mut state.match_state.scenario_outcome)
        .with_replay_playback(replaying)
        .with_deferred_verdict(defer_verdict_to_clip)
        .with_eva_text(detailed_eva_text.then_some(&mut finish.prefix_eva_text))
        .with_quiet_sounds(&mut state.match_state.match_audio.quiet);
        let effects = FrameEffects::for_client(&mut boundary)
            .with_production_strip(&production_visits)
            .with_bounty_text(&mut queue_bounty);
        let mut effects = if observing_commands {
            effects.with_command_applied(&mut command_applied)
        } else {
            effects
        };
        if let Some(playback) = replay_playback {
            match playback.advance(rt, &mut effects) {
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
            let network = rt.simulation.session().game_mode().is_network();
            match resume.run(rt, &due_commands, tick_lane, &mut effects) {
                Ok(advanced) => advanced,
                Err(error) => {
                    stop_failed_frame(&mut state.match_state.frame_stopped, network, &error);
                    return false;
                }
            }
        }
    };
    // The thread's part of a handed-off frame counted its cost; the exchange
    // adds no second sample.
    let tick_cost = (resume != FrameResume::QueueHandoff).then(|| advance_started.elapsed());
    crate::app::spawn_frame_cost::end_inline_tick(
        &mut state.frontend.external_progress,
        inline_cost,
    );
    if let Some(tick_cost) = tick_cost {
        state
            .platform
            .frame_pacer
            .record_frame_cost(tick_cost, false);
        crate::app::spawn_frame_cost::tick(&mut state.frontend.external_progress, tick_cost, false);
    }
    record_replay_frame(
        state.match_state.match_diagnostics.replay_log.as_mut(),
        &mut output,
    );
    let committed = finish_simulation_frame(state, finish, output, tick_cost, mouse_scroll);
    threaded_finished.unwrap_or(false) | committed
}

/// What a frame's tail needs from before its tick.
struct FrameFinish {
    native_frame: u32,
    local_owner_for_fog: Option<String>,
    local_owner_name: Option<String>,
    defer_verdict_to_clip: bool,
    detailed_eva_text: bool,
    /// VERA-only EVA rows the frame's synchronous sound prefix posted.
    prefix_eva_text: Vec<crate::app::input::messages::HudMessage>,
    deferred: Option<super::deferred_effects::DeferredClientEffects>,
}

/// A tick the simulation thread ran, back with what its tail needs.
struct ThreadedTick {
    finish: FrameFinish,
    output: Result<SimFrameOutput, crate::sim::world::FrameAdvanceError>,
    tick_cost: crate::util::time::Duration,
    /// When the job started and ended (`wall_clock_ms`): for a network
    /// frame, the Logic visit that Queue_AI measures.
    logic_ms: (u64, u64),
}

fn take_threaded_tick(
    host: &mut super::sim_host::SimHost,
    replay_log: Option<&mut ReplayLog>,
) -> Option<ThreadedTick> {
    let result = host.take_job_result()?;
    let mut tick = *result
        .downcast::<ThreadedTick>()
        .expect("the simulation thread runs only frame ticks");
    if let Ok(output) = &mut tick.output {
        record_replay_frame(replay_log, output);
    }
    Some(tick)
}

/// VERA-only: a failed frame must not crash the game. A peer's malformed
/// event reaches here in a LAN/online match, and mod or map data the
/// simulation refuses (an unknown `MovementZone=`, say) reaches here in any
/// match. The frame did not commit, so this timeline never runs again; the
/// box's Stop returns to the shell, which saves the replay. A checksum
/// mismatch never arrives here (it suspends the frame instead), so neither
/// the log nor the box (`network_failure::vera_body`) calls this a desync.
fn stop_failed_frame(frame_stopped: &mut bool, network: bool, error: &dyn std::fmt::Display) {
    if network {
        log::error!("Network match stopped by an error, not a desync: {error}");
    } else {
        log::error!("Match stopped by a simulation error: {error}");
    }
    *frame_stopped = true;
}

/// Finish the tick the simulation thread ran, waiting for it if it still
/// runs. `None` when no tick was out; else whether it committed.
fn finish_threaded_tick(state: &mut AppState, mouse_scroll: Option<(f32, f32)>) -> Option<bool> {
    let active = &mut state.match_state;
    let tick = take_threaded_tick(
        &mut active.sim_runtime,
        active.match_diagnostics.replay_log.as_mut(),
    )?;
    Some(finish_threaded_output(state, tick, mouse_scroll))
}

fn finish_threaded_output(
    state: &mut AppState,
    tick: ThreadedTick,
    mouse_scroll: Option<(f32, f32)>,
) -> bool {
    let ThreadedTick {
        finish,
        output,
        tick_cost,
        logic_ms: (logic_started_ms, logic_finished_ms),
    } = tick;
    let network = finish
        .deferred
        .as_ref()
        .is_some_and(|deferred| deferred.hands_off_queue());
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            stop_failed_frame(&mut state.match_state.frame_stopped, network, &error);
            return false;
        }
    };
    // MainTick55D3DA/55DE0D times the Logic visit up to Queue_AI. It ran on
    // the simulation thread, not until this thread resumes the exchange,
    // whose own finish then finds the sample taken.
    if output.network_waiting
        && let Some(live) = state.match_state.live_network.as_mut()
    {
        live.begin_processing(logic_started_ms as u32);
        live.finish_processing(logic_finished_ms as u32);
    }
    state
        .platform
        .frame_pacer
        .record_frame_cost(tick_cost, true);
    crate::app::spawn_frame_cost::tick(&mut state.frontend.external_progress, tick_cost, true);
    let committed = finish_simulation_frame(state, finish, output, Some(tick_cost), mouse_scroll);
    // The draw left this tick's radar-terrain marks with it
    // (`App::drain_radar_terrain_marks`); they go to the minimap now.
    drain_radar_terrain_marks(&mut state.match_state);
    // A network frame stopped at Queue_AI on the simulation thread; its
    // exchange, dispatch and commit continue here, with the live session.
    if !pending_network_wait(&state.match_state.sim_runtime) {
        return committed;
    }
    advance_one_simulation_frame(
        state,
        TickLane::Ordinary,
        None,
        FrameResume::QueueHandoff,
        true,
    ) | committed
}

fn pending_network_wait(sim_runtime: &SimHost) -> bool {
    sim_runtime
        .runtime()
        .is_some_and(|rt| rt.simulation.pending_network_wait())
}

/// A frame whose client writes are recorded ([`super::deferred_effects`]):
/// it borrows no app state, so it can run on the simulation thread.
fn run_deferred_tick(
    rt: &mut crate::sim::runtime::SimRuntime,
    deferred: &mut super::deferred_effects::DeferredClientEffects,
    production_visits: &[production::ProductionFactoryVisit],
    due_commands: &[crate::sim::command::CommandEnvelope],
    tick_lane: TickLane,
    resume: FrameResume,
) -> Result<SimFrameOutput, crate::sim::world::FrameAdvanceError> {
    // Sound plays from the frame tail; replay, the initial save and command
    // logs never reach this path, and a network frame stops at Queue_AI.
    let mut effects = FrameEffects::for_client(deferred).with_production_strip(production_visits);
    resume.run(rt, due_commands, tick_lane, &mut effects)
}

/// Collect the HUD facts carried by this visit's sound batch, independently
/// of whether its audio was already delivered by the synchronous boundary.
fn frame_hud_messages(
    output: &SimFrameOutput,
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    local_owner: Option<&str>,
) -> Vec<crate::app::input::messages::HudMessage> {
    let events = &output.sound_events;
    let mut messages = super::sound_dispatch::hud_messages(events, sim.interner(), local_owner);
    if rules.ares_form() {
        messages.extend(super::sound_dispatch::ares_superweapon_messages(
            events,
            sim,
            rules,
            local_owner,
        ));
    }
    // The sim fills these labels only when Ares plays.
    messages.extend(super::sound_dispatch::ares_tech_capture_messages(
        events,
        sim,
        local_owner,
    ));
    messages
}

/// The frame tail on the main thread: the client takes the tick's output.
fn finish_simulation_frame(
    state: &mut AppState,
    finish: FrameFinish,
    mut output: SimFrameOutput,
    tick_cost: Option<crate::util::time::Duration>,
    mouse_scroll: Option<(f32, f32)>,
) -> bool {
    let FrameFinish {
        native_frame,
        local_owner_for_fog,
        local_owner_name,
        defer_verdict_to_clip,
        detailed_eva_text,
        mut prefix_eva_text,
        deferred,
    } = finish;
    let mut frame_committed = false;
    let mut drained_lifecycle_outputs: Vec<LifecycleOutput> = Vec::new();
    let mut drained_combat_lights = Vec::new();
    let mut frame_overlay_updates = Vec::new();
    let mut frame_overlay_removals: Vec<(u16, u16)> = Vec::new();
    let mut trigger_effects: Vec<TriggerEffect> = Vec::new();
    let mut camera_logic_frame: Option<u32> = None;
    let mut hud_messages = Vec::new();
    // Carried out of the sim borrow so the census can read `state` freely below.
    let mut census_tick: Option<u64> = None;
    // Read before the world is borrowed for writing below.
    #[cfg(feature = "telemetry-dashboard")]
    let telemetry_budget_ms =
        super::network_match::game_frame_ms(&state.platform, &state.match_state);
    {
        if let Some((rt, _)) = state.match_state.sim_runtime.tick_parts() {
            state
                .match_state
                .match_presentation
                .screen_shake
                .apply_updates(&output.screen_shake_updates);
            hud_messages = frame_hud_messages(
                &output,
                &rt.simulation,
                &rt.resources.rules,
                local_owner_name.as_deref(),
            );
            if output.outcome_waiting {
                // House4F860B/4F8732 pauses after MPlayer_Defeated has
                // already posted its text (4FC271/4FC34F). The sim handed
                // off this sound batch, which the resumed House won't return.
                hud_messages.extend(prefix_eva_text.drain(..));
                if let Some(deferred) = deferred {
                    // A deferred frame has no synchronous boundary: the
                    // sound facts of this visit, which the simulation has
                    // already handed over, are delivered here instead.
                    let sim = &rt.simulation;
                    let rules = &rt.resources.rules;
                    let radar_frame = sim.session().tick();
                    let minimap = &mut state.match_state.match_presentation.minimap;
                    let mut admit_radar = |request: crate::sim::radar::RadarEventRequest| {
                        minimap.as_mut().is_some_and(|minimap| {
                            minimap.admit_radar_event(request, radar_frame, Some(rules))
                        })
                    };
                    let delivered = output.sound_events_delivered;
                    let quiet = &mut state.match_state.match_audio.quiet;
                    super::sound_dispatch::dispatch_sim_sound_events(
                        std::mem::take(&mut output.sound_events)
                            .into_iter()
                            .skip(delivered)
                            .filter(|event| {
                                !(defer_verdict_to_clip
                                    && matches!(
                                        event,
                                        crate::sim::world::SimSoundEvent::MatchOutcome { .. }
                                    ))
                                    && quiet.admit(event, radar_frame)
                            }),
                        sim,
                        rules,
                        local_owner_name.as_deref(),
                        &mut admit_radar,
                        &mut state.match_state.match_audio.sound_events,
                        detailed_eva_text.then_some(&mut hud_messages),
                    );
                    // Its client writes, the outcome wait it polled among
                    // them, still reach their owners.
                    deferred.apply(&mut state.match_state, &mut state.audio.sfx_player);
                }
                for message in hud_messages {
                    crate::app::input::messages::post_hud_message(
                        &state.platform,
                        &state.renderer,
                        &state.diag,
                        &state.frontend,
                        &mut state.match_state,
                        &state.process_assets,
                        &mut state.audio.sfx_player,
                        &state.persistence,
                        message,
                    );
                }
                return false;
            }
            // A network frame resumed after the simulation thread's Logic has
            // its cost from that thread's part; the exchange adds no sample.
            if let Some(tick_cost) = tick_cost {
                let tick_ms = tick_cost.as_secs_f32() * 1000.0;
                // Capture the committed tick's SIM snapshot while the runtime
                // is home (this is the tick tail). The perf recorder reads this
                // at present time, when the runtime may be lent to the SIM
                // thread and `home_runtime()` is None.
                #[cfg(feature = "perf")]
                {
                    use crate::app::diagnostics::perf_recorder::PerfTickSnapshot;
                    let sim = rt.simulation.frame_profile().last_frame();
                    let path = rt.simulation.path_batch_stats();
                    state.diag.perf_last_tick = Some(PerfTickSnapshot {
                        tick: rt.simulation.session().tick(),
                        binary_frame: rt.simulation.session().binary_frame(),
                        objects: rt.simulation.entities().len(),
                        houses: rt.simulation.houses().len(),
                        sim,
                        path,
                    });
                }
                state.match_state.network_quality.record_tick(tick_ms);
                // The relay's pace reports carry the simulation's own cost.
                if let Some(live) = state.match_state.live_network.as_mut() {
                    live.record_tick_cost(tick_cost);
                }
                #[cfg(feature = "telemetry-dashboard")]
                {
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
                        let mut timing = state
                            .match_state
                            .telemetry_timing
                            .summarize(telemetry_budget_ms);
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
            }
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
            // The objects whose look the tick changed, for the atlas check at
            // the tail.
            state
                .match_state
                .match_presentation
                .atlas_check
                .note(sim.take_looks());
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
            // VERA-only accessibility text: collect the EVA rows only when the
            // option is on. A disabled switch passes None, so the dispatch
            // never builds a fact and no rules/CSF lookup follows.
            let mut eva_text_messages = Vec::new();
            let quiet = &mut state.match_state.match_audio.quiet;
            super::sound_dispatch::dispatch_sim_sound_events(
                frame_sound_events
                    .into_iter()
                    .skip(sound_events_delivered)
                    // The same rules as the frame boundary's, for the facts
                    // that arrived after that boundary's last pass.
                    .filter(|event| {
                        !(defer_verdict_to_clip
                            && matches!(
                                event,
                                crate::sim::world::SimSoundEvent::MatchOutcome { .. }
                            ))
                            && quiet.admit(event, radar_frame)
                    }),
                sim,
                &resources.rules,
                local_owner_name.as_deref(),
                &mut admit_radar,
                &mut state.match_state.match_audio.sound_events,
                detailed_eva_text.then_some(&mut eva_text_messages),
            );
            // Frame order: the synchronously delivered prefix precedes the
            // frame-tail suffix. Both passes follow the same voice gates.
            hud_messages.extend(prefix_eva_text.drain(..));
            hud_messages.extend(eva_text_messages);
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
        }
        // The client writes a deferred frame recorded, in order, now that the
        // world is no longer borrowed.
        if let Some(deferred) = deferred {
            deferred.apply(&mut state.match_state, &mut state.audio.sfx_player);
        }
        if frame_committed {
            state
                .match_state
                .match_presentation
                .combat_lights
                .commit_frame(drained_combat_lights);
        }
        crate::app::input::dispatch::reconcile_selection_order_after_sim(
            &mut state.match_state,
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
                &state.platform,
                &state.renderer,
                &state.frontend.screen,
                &mut state.match_state,
                &state.persistence,
                mouse_scroll.unwrap_or((0.0, 0.0)),
                frame,
            );
        } else if let Some(request) = mouse_scroll {
            crate::app::input::camera::commit_camera_scroll(
                &state.platform,
                &state.renderer,
                &state.frontend.screen,
                &mut state.match_state,
                &state.persistence,
                request,
            );
        }
        // Native drives the follow camera from the tail of
        // LogicClass__PerTickUpdate 0x0055B6B8, after every object has updated,
        // so the view lands on this tick's position rather than the last one's.
        crate::app::input::camera::update_follow_camera(
            &state.platform,
            &state.renderer,
            &state.frontend.screen,
            &mut state.match_state,
            &state.persistence,
            &drained_lifecycle_outputs,
        );
        // Rendering is rebuilt from lifecycle facts every frame. Replay the
        // app-owned transactions in native emission order for state that has a
        // direct attachment or retained audio handle.
        let local_house = preferred_local_owner_name(&state.match_state).and_then(|owner| {
            state
                .match_state
                .sim_runtime
                .runtime()?
                .simulation
                .interner()
                .get(&owner)
        });
        // House4F8B08..4F8B2B: this frame's House rung is the native caller.
        let placement_armed = state.match_state.input.armed_building_type().is_some();
        if let Some(rt) = state.match_state.sim_runtime.runtime() {
            crate::app::presentation::sidebar_gadgets::exit_sell_mode_without_buildings(
                &mut state.match_state.match_presentation.sidebar_gadget_state,
                &rt.simulation,
                local_house,
                placement_armed,
            );
        }
        let ares = crate::app::state::active_rules(&state.frontend, &state.match_state)
            .is_some_and(crate::rules::ruleset::RuleSet::ares);
        for output in drained_lifecycle_outputs {
            if let Some(cursor_clear) = state
                .match_state
                .match_presentation
                .sidebar_projection
                .model
                .apply_lifecycle_output(&output, local_house, ares)
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
                update_building_placement_preview(
                    &state.platform,
                    &state.renderer,
                    &state.frontend,
                    &mut state.match_state,
                    &state.persistence,
                    FoundationCentre::Cursor,
                );
            }
            match output {
                LifecycleOutput::OptionsRequested => {
                    crate::app::App::enter_in_game_menu_state(
                        &mut state.platform,
                        &state.renderer,
                        &mut state.frontend,
                        &mut state.match_state,
                        &state.process_assets,
                        &mut state.audio,
                        &mut state.persistence,
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
                LifecycleOutput::PlanningAppendRefused { owner, message } => {
                    crate::app::input::planning_mode::append_output(
                        &state.platform,
                        &state.renderer,
                        &state.diag,
                        &state.frontend,
                        &mut state.match_state,
                        &state.process_assets,
                        &state.persistence,
                        Some(owner) == local_house,
                        message,
                        false,
                    );
                }
                LifecycleOutput::PlanningFramePassQueued { owner, message } => {
                    crate::app::input::planning_mode::append_output(
                        &state.platform,
                        &state.renderer,
                        &state.diag,
                        &state.frontend,
                        &mut state.match_state,
                        &state.process_assets,
                        &state.persistence,
                        Some(owner) == local_house,
                        message,
                        true,
                    );
                }
                // MPlayer_Defeated's local branch deletes PlayerPtr's beacons,
                // closes the command bar and leaves beacon mode (0x004FC1C6,
                // 0x004FC1D9, 0x004FC1FA), then sets the chat mask
                // (0x004FC23E) before the line (0x004FC271) this frame posts.
                // Any defeat that ends the match reopens the bar (0x004FC591).
                LifecycleOutput::MultiplayerHouseDefeated {
                    owner,
                    match_decided,
                } => {
                    if Some(owner) == local_house {
                        super::beacons::local_player_defeated(
                            &state.platform,
                            &state.renderer,
                            &state.diag,
                            &state.frontend,
                            &mut state.match_state,
                            &state.process_assets,
                            &mut state.audio.sfx_player,
                            &state.persistence,
                        );
                        let presentation = &mut state.match_state.match_presentation;
                        presentation
                            .sidebar_gadget_state
                            .close_command_bar_at_local_defeat();
                        presentation.chat.local_player_defeated();
                    }
                    state
                        .match_state
                        .match_presentation
                        .sidebar_gadget_state
                        .reopen_command_bar_at_match_end(match_decided);
                }
                LifecycleOutput::SidebarConstructionOption { .. }
                | LifecycleOutput::SidebarRecalculated { .. }
                | LifecycleOutput::ProductionBeginAttempted { .. }
                | LifecycleOutput::ProductionFactoryLinked { .. }
                | LifecycleOutput::ProductionBuildingReady { .. }
                | LifecycleOutput::ProductionFactoryAbandoning { .. }
                | LifecycleOutput::ProductionFactoryUnlinked { .. }
                | LifecycleOutput::BuildingPlacementRetry { .. }
                | LifecycleOutput::BuildingPlacementSucceeded { .. }
                | LifecycleOutput::BuildingProductionAbandoned { .. }
                | LifecycleOutput::ConstructionYardCaptured { .. }
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
                    let runtime = state.match_state.sim_runtime.runtime();
                    let presentation = &mut state.match_state.match_presentation;
                    let colors = &presentation.house_color_map;
                    presentation
                        .lasers
                        .create(birth, |owner| house_laser_rgb(runtime, colors, owner));
                }
                LifecycleOutput::ElectricBoltCreated(birth) => {
                    state
                        .match_state
                        .match_presentation
                        .electric_bolts
                        .create(birth);
                }
                LifecycleOutput::RadiationBeamCreated(birth) => {
                    let runtime = state.match_state.sim_runtime.runtime();
                    let presentation = &mut state.match_state.match_presentation;
                    let colors = &presentation.house_color_map;
                    presentation
                        .radiation_beams
                        .create(birth, |owner, kind| match kind {
                            // Only a birth fired while Ares plays asks for House.
                            crate::sim::combat::radiation_beam::BeamHouseColor::House => {
                                house_rgb(runtime, colors, owner)
                            }
                            _ => house_laser_rgb(runtime, colors, owner),
                        });
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
                LifecycleOutput::DirtyTacticalRect { .. }
                | LifecycleOutput::ClearDrawnState { .. }
                | LifecycleOutput::ClearRedraw { .. } => {}
            }
        }
        // The sidebar's next membership pass reads the House pass's AddCameo
        // attempts from the frame: ask them before it is captured, also when
        // the next tick is lent with it.
        let asked = state
            .match_state
            .match_presentation
            .sidebar_projection
            .model
            .asked_types(&[]);
        state.match_state.sim_runtime.ask_sidebar_types(&asked);
        // Main_Tick runs PlanMgr_Process_Frame after the frame's events
        // (0x0055DEA4); its input tail follows their lifecycle outputs.
        if frame_committed {
            crate::app::input::planning_mode::frame_pass(
                &state.platform,
                &state.renderer,
                &state.diag,
                &state.frontend,
                &mut state.match_state,
                &state.process_assets,
                &mut state.audio.sfx_player,
                &state.persistence,
            );
            // VERA ferry: the routes' orders, as ordinary input for a later
            // frame.
            crate::app::input::ferry::frame_pass(&mut state.match_state);
            // VERA Auto Repair: Repair orders for damaged buildings, the same
            // way.
            crate::app::input::auto_repair::frame_pass(
                &mut state.match_state,
                &state.persistence.options_profile,
            );
            // VERA primary rotation: the factory groups' turns.
            crate::app::input::primary_rotation::frame_pass(&mut state.match_state);
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
            crate::app::input::dispatch::report_black_cell_causes(&state.match_state);
        }

        for message in hud_messages {
            crate::app::input::messages::post_hud_message(
                &state.platform,
                &state.renderer,
                &state.diag,
                &state.frontend,
                &mut state.match_state,
                &state.process_assets,
                &mut state.audio.sfx_player,
                &state.persistence,
                message,
            );
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
            upsert_occupied_overlay_render_entries(
                &mut state.match_state.match_presentation,
                frame_overlay_updates,
            );
        }
    }

    // VERA relation colours follow the frame's alliances: an alliance made or
    // broken recolours that house before the atlas check below. Team colours
    // are fixed for the match, but sharing this refresh keeps one path.
    if frame_committed
        && (state.match_state.match_presentation.relation_colors
            || state.match_state.match_presentation.team_colors)
    {
        state.match_state.refresh_relation_colors();
    }
    // The atlases are checked against what the tick changed, before its first
    // draw: a new object, owner, type or disguise, an anim's new colour remap
    // (Unit deploy Anim+D4, 739C1A/739DFE, installs one after construction),
    // a new FoggedObject record. A miss, or a refresh asked beside a lent
    // tick (`refresh_entity_atlases`), runs the full refresh here.
    if state.match_state.match_presentation.atlas_refresh_owed
        || atlas_check_finds_miss(&mut state.match_state)
    {
        refresh_entity_atlases(
            &state.renderer,
            &mut state.match_state,
            &state.process_assets,
        );
    }
    frame_committed
}

/// Check the entity atlases against the looks changed since the last check
/// ([`EntityAtlasCheck`]): whether they may lack something the world draws.
/// A world away on the simulation thread is checked once it is back.
///
/// [`EntityAtlasCheck`]: crate::render::entity_atlas_growth::EntityAtlasCheck
fn atlas_check_finds_miss(match_state: &mut MatchState) -> bool {
    if match_state.sim_runtime.is_lent() {
        return false;
    }
    let Some(runtime) = match_state.sim_runtime.runtime() else {
        return false;
    };
    let presentation = &mut match_state.match_presentation;
    let house_colors = presentation
        .relation_color_map
        .as_ref()
        .unwrap_or(&presentation.house_color_map);
    let miss = presentation.atlas_check.finds_miss(
        &runtime.simulation,
        Some(&runtime.resources.rules),
        presentation.sprite_atlas.as_ref(),
        presentation.unit_atlas.as_ref(),
        house_colors,
    );
    if !miss {
        presentation
            .atlas_check
            .checked(&runtime.simulation, house_colors);
    }
    miss
}

/// The presentation-owned light transaction's pass work
/// ([`MatchLighting::after_pass`](crate::app::presentation::lighting::MatchLighting::after_pass)).
fn refresh_cell_lighting(match_state: &mut MatchState) {
    let Some(runtime) = match_state.sim_runtime.runtime() else {
        return;
    };
    let Some(terrain) = runtime.resources.terrain_template.as_ref() else {
        return;
    };
    let presentation = &mut match_state.match_presentation;
    presentation.lighting.after_pass(
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
                super::scenario_pause::enter(
                    &mut state.match_state,
                    &mut state.audio,
                    *duration,
                    *outermost,
                    now,
                );
            }
            TriggerEffect::ScenarioResume => {
                let now = crate::app::match_runtime::frame_pacer::wall_clock_ms();
                super::scenario_pause::resume(
                    &state.frontend.screen,
                    &mut state.match_state,
                    &mut state.audio,
                    now,
                );
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
                crate::app::chrono_screen::apply(
                    &state.frontend.screen,
                    &mut state.match_state,
                    &state.process_assets,
                    &mut state.audio,
                    *command,
                );
            }
            TriggerEffect::RadarMovie {
                index,
                pause_simulation,
            } => {
                if !(*pause_simulation
                    && super::narrative_sync::queue(
                        &mut state.match_state,
                        super::narrative_sync::Clip::Radar(*index),
                    ))
                {
                    crate::app::radar_movie::enqueue(
                        &mut state.match_state,
                        &mut state.persistence.options_profile,
                        &state.process_assets,
                        &state.renderer,
                        *index,
                        *pause_simulation,
                    );
                }
            }
            TriggerEffect::FullscreenMovie { index } => {
                if super::narrative_sync::queue(
                    &mut state.match_state,
                    super::narrative_sync::Clip::Fullscreen(*index),
                ) {
                    continue;
                }
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
            TriggerEffect::EvaVoice { index } => {
                if let (Some(sfx), Some(catalog)) = (
                    state.audio.sfx_player.as_mut(),
                    state.process_assets.audio_catalog(),
                ) {
                    sfx.set_trigger_eva_voice(*index, catalog.eva());
                }
            }
            TriggerEffect::HomeCamera { coord } => {
                crate::app::input::camera::apply_home_camera(
                    &state.platform,
                    &mut state.renderer,
                    &state.frontend,
                    &mut state.match_state,
                    &state.persistence,
                    *coord,
                );
            }
            TriggerEffect::CameraAtCoord { coord, velocity } => {
                crate::app::input::camera::apply_trigger_camera(
                    &state.platform,
                    &state.renderer,
                    &state.frontend.screen,
                    &mut state.match_state,
                    &state.persistence,
                    *coord,
                    *velocity,
                );
            }
            TriggerEffect::InputLockChanged { locked, was_locked } => {
                crate::app::input::campaign_input::apply_lock_effect(
                    &state.platform,
                    &state.renderer,
                    &state.diag,
                    &state.frontend,
                    &mut state.match_state,
                    &state.process_assets,
                    &mut state.audio,
                    &state.persistence,
                    *locked,
                    *was_locked,
                );
            }
            TriggerEffect::CancelPlacementModes => {
                // Main_Tick runs Logic, where action 46 fires (0x0055DC9E),
                // before the frame's events (0x00647260 at 0x0055DE40 ->
                // Execute_DoList 0x0064C380). A PLACE that fails in that frame
                // restores its placement (HousePlace 0x004FB395..0x004FB49A)
                // after this clear; the BuildingPlacementRetry outputs, applied
                // after these effects, keep that order.
                crate::app::input::campaign_input::cancel_placement_modes(
                    &mut state.match_state.input,
                    &mut state.match_state.match_presentation.sidebar_gadget_state,
                );
            }
            TriggerEffect::RecentEventCell { cell } => {
                if let Some(minimap) = state.match_state.match_presentation.minimap.as_mut() {
                    minimap.remember_event_cell(*cell);
                }
            }
            TriggerEffect::LocalizedMessage { csf_label } => {
                crate::app::input::messages::post_trigger_message(
                    &state.platform,
                    &state.renderer,
                    &state.diag,
                    &state.frontend,
                    &mut state.match_state,
                    &state.process_assets,
                    &mut state.audio.sfx_player,
                    &state.persistence,
                    csf_label,
                );
            }
            TriggerEffect::ExtModuleStopped { reason } => {
                let text = crate::i18n::tr_fill("VERA:ModuleStopped", &[("reason", reason)]);
                let name = crate::i18n::tr("VERA:ModuleSender");
                crate::app::input::messages::post_chat_line(
                    &state.platform,
                    &state.renderer,
                    &state.diag,
                    &state.frontend,
                    &mut state.match_state,
                    &state.process_assets,
                    &mut state.audio.sfx_player,
                    &state.persistence,
                    &name,
                    &text,
                    [1.0, 0.4, 0.4],
                );
            }
            TriggerEffect::ExtModuleNotice { text } => {
                let name = crate::i18n::tr("VERA:ModuleSender");
                crate::app::input::messages::post_chat_line(
                    &state.platform,
                    &state.renderer,
                    &state.diag,
                    &state.frontend,
                    &mut state.match_state,
                    &state.process_assets,
                    &mut state.audio.sfx_player,
                    &state.persistence,
                    &name,
                    text,
                    [1.0, 0.8, 0.4],
                );
            }
            TriggerEffect::SelectSidebarTab { index } => {
                refresh_sidebar = true;
                let presentation = &mut state.match_state.match_presentation;
                if let Some(tab) = presentation.sidebar_projection.model.selectable_tab(*index) {
                    // Trigger6E4100 bypasses the player-input lock, retaining
                    // each strip's independent scroll state.
                    presentation.active_sidebar_tab = tab;
                }
            }
            TriggerEffect::FlashSidebarCameo {
                category,
                type_index,
                duration,
                frame,
            } => {
                refresh_sidebar = true;
                if let Some(runtime) = state.match_state.sim_runtime.runtime() {
                    state
                        .match_state
                        .match_presentation
                        .sidebar_projection
                        .model
                        .flash_cameo(
                            *category,
                            *type_index,
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
        crate::app::presentation::sidebar_render::refresh_sidebar_projection(
            &state.platform,
            &state.renderer,
            &state.diag,
            &state.frontend,
            &mut state.match_state,
            &state.process_assets,
            &mut state.audio,
            &state.persistence,
        );
    }
}

/// Where `DisplayClass::UpdateFoundationCenter @ 0x004A91B0` takes an armed
/// building's foundation centre from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FoundationCentre {
    /// The Cell under the cursor wherever it is. The Home view
    /// (`0x0073181A`), the four view bookmarks (`0x005373C3`, `0x005374B3`,
    /// `0x005375A3`, `0x00537693`) and the radar event jump (`0x00734200`)
    /// pass the null centre, which means it (`0x004A91E2..0x004A92B1`); a
    /// placement retry passes the cursor's `ProcessClickCoords` Cell
    /// (`0x004FB3F8..0x004FB484`).
    Cursor,
    /// The tactical gadget's call each frame (`DisplayClass::Action @
    /// 0x004AAC10`, at `0x004AAD04`), which only the battlefield's region
    /// reaches: off it, over the sidebar or the radar, the centre stays.
    Tactical,
    /// A given Cell: placement start's factory Cell
    /// (`HouseClass::Begin_Building_Placement @ 0x004FB840`, at `0x004FB8FE`).
    Cell((u16, u16)),
}

impl FoundationCentre {
    /// The centre Cell this call takes, or `None` where the latched centre
    /// (`has_centre`) stays; `cursor` answers the Cell under the cursor.
    fn cell(
        self,
        has_centre: bool,
        over_battlefield: bool,
        cursor: impl FnOnce() -> (u16, u16),
    ) -> Option<(u16, u16)> {
        match self {
            Self::Cell(cell) => Some(cell),
            Self::Tactical if has_centre && !over_battlefield => None,
            Self::Cursor | Self::Tactical => Some(cursor()),
        }
    }
}

/// Re-centre the foundation ([`FoundationCentre`]) through its latch
/// ([`crate::app::input::state::FoundationLatch`]) and refresh the armed
/// building's preview there.
pub(crate) fn update_building_placement_preview(
    platform: &crate::app::PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    persistence: &crate::app::persistence::PersistenceState,
    centre: FoundationCentre,
) {
    let cursor_cell = |match_state: &MatchState| {
        screen_point_to_world_cell(
            match_state,
            match_state.input.cursor_x,
            match_state.input.cursor_y,
        )
    };
    let Some(type_id) = match_state.input.armed_building_type() else {
        match_state.input.building_placement_preview = None;
        // The world copy beside a lent tick has no Cell heights to pick with.
        if !match_state.sim_runtime.is_lent()
            && let Some(cell) = centre.cell(
                match_state.input.foundation_latch.origin().is_some(),
                crate::app::input::camera::cursor_over_battlefield(
                    platform,
                    renderer,
                    &frontend.screen,
                    match_state,
                    persistence,
                ),
                || cursor_cell(match_state),
            )
        {
            match_state.input.foundation_latch.follow(cell);
        }
        return;
    };
    if match_state.input.building_placement_issuer().is_none() {
        // Product-only pointer expiry retains the current type/foundation;
        // it is neither an abandon nor a permission to emit another PLACE.
        // RESIDUAL: `0x004A91B0` reads only the foundation (+0x117C), not the
        // product (+0x11A4), so natively the grid keeps following the cursor
        // after the product expires; here it stays where it was.
        return;
    }
    // The owner's buildings bound the build area (`is_within_build_area`),
    // and the world copy beside a lent tick has no objects: the preview
    // answered before the tick left stands until it is back.
    if match_state.sim_runtime.is_lent() {
        return;
    }
    let owner = preferred_local_owner(match_state);
    let (Some(owner), Some(sim), Some(rules)) = (
        owner,
        match_state.sim_runtime.runtime().map(|rt| &rt.simulation),
        crate::app::state::active_rules(frontend, match_state),
    ) else {
        match_state.input.building_placement_preview = None;
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
    // Process-wide, like the log it throttles.
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
    let mut latch = match_state.input.foundation_latch;
    // UpdateFoundationCenter4A91B0 takes the cursor's Cell from
    // `RadarClass::Click_Cell_Calc @ 0x00653760` (`0x004A9251`): the
    // tactical ScreenPixelToCell answer this shared inverse ports.
    // UpdateFoundationCenter4A91B0 uses center + the type's active offset.
    // Resolve it before admission/ground/shroud, through the shared owner
    // also used by a fresh LeftUp; rendering consumes the resulting Cells.
    let origin = centre
        .cell(
            latch.origin().is_some(),
            crate::app::input::camera::cursor_over_battlefield(
                platform,
                renderer,
                &frontend.screen,
                match_state,
                persistence,
            ),
            || cursor_cell(match_state),
        )
        .map(|cell| foundation_str.placement_origin_at_cursor(cell));
    let overlays = crate::app::state::active_overlay_registry(frontend, match_state);
    let preview = latch.place(origin, |(rx, ry)| {
        production::placement_preview_for_owner_with_overlays(
            sim, rules, &owner, type_id, rx, ry, overlays,
        )
    });
    match_state.input.foundation_latch = latch;
    match_state.input.building_placement_preview = preview;
}

/// Admit the voxel models and grow the SHP atlas when the atlas check finds a
/// changed look they lack, and again after a growth publishes. A new voxel
/// model's pictures are drawn the first time a frame shows them
/// ([`crate::render::unit_atlas::admit_world_models`]); the SHP raster runs on
/// a worker ([`EntityAtlasGrowth::start`]) and [`publish_entity_atlas_growth`]
/// completes it. Reuses the process asset manager instead of creating a new
/// one (avoids re-opening all MIX archives from disk).
///
/// [`EntityAtlasGrowth::start`]: crate::render::entity_atlas_growth::EntityAtlasGrowth::start
pub(crate) fn refresh_entity_atlases(
    renderer: &RendererState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
) {
    // The refresh asks the live world's objects, which the world copy beside
    // a lent tick lacks: a refresh asked then (a growth published beside the
    // tick) waits for the tick's tail.
    let lent = match_state.sim_runtime.is_lent();
    match_state.match_presentation.atlas_refresh_owed = lent;
    if lent {
        return;
    }
    let Some(rt) = match_state.sim_runtime.runtime() else {
        return;
    };
    let Some(asset_manager) = process_assets.manager() else {
        log::warn!("Atlas refresh skipped: no asset manager available");
        return;
    };
    let presentation = &mut match_state.match_presentation;
    crate::render::unit_atlas::admit_world_and_image_models(
        &mut presentation.unit_atlas,
        rt.simulation.entities(),
        rt.simulation.ext_module_images(),
        asset_manager,
        Some(&rt.resources.rules),
        Some(rt.simulation.interner()),
        renderer.gpu.device.limits().max_texture_dimension_2d,
    );
    let house_colors = presentation
        .relation_color_map
        .as_ref()
        .unwrap_or(&presentation.house_color_map);
    presentation.atlas_growth.start(
        &crate::render::entity_atlas_growth::GrowthSource {
            sim: &rt.simulation,
            rules: Some(&rt.resources.rules),
            asset_manager,
            house_colors,
            theater_name: &presentation.theater_name,
            theater_ext: &presentation.theater_ext,
            max_texture_dimension_2d: renderer.gpu.device.limits().max_texture_dimension_2d,
        },
        presentation.sprite_atlas.as_ref(),
    );
    // A growth this found running checks the whole world again once it
    // publishes (`publish_entity_atlas_growth`).
    presentation
        .atlas_check
        .checked(&rt.simulation, house_colors);
}

/// Publish the finished atlas growths before a picture is judged, then start
/// the growths the world still needs. `wait` blocks until none runs, so
/// captures draw complete atlases and never depend on worker timing.
pub(crate) fn publish_entity_atlas_growth(
    renderer: &RendererState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    wait: bool,
) {
    loop {
        let gpu = &renderer.gpu;
        let presentation = &mut match_state.match_presentation;
        if !presentation.atlas_growth.publish(
            &gpu.device,
            &gpu.queue,
            &renderer.batch_renderer,
            &mut presentation.sprite_atlas,
            wait,
        ) {
            return;
        }
        refresh_entity_atlases(renderer, match_state, process_assets);
        if !wait {
            return;
        }
    }
}

/// Upsert authoritative occupied-overlay entries into `presentation.overlays`.
///
/// Background: the overlay renderer iterates `presentation.overlays`, the static list
/// loaded from the map's `[OverlayPack]`. Sim-side mutations that create new
/// overlay cells (TIBTRE ore spawn, ore_growth spread) update `OverlayGrid`
/// but never touched `presentation.overlays`, so the new cells were invisible even
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
    presentation: &mut MatchPresentationState,
    candidates: Vec<crate::map::overlay::OverlayEntry>,
) {
    let synced = presentation.overlays.upsert_occupied(candidates);
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

pub(crate) fn screen_point_to_world(
    input: &crate::app::input::state::MatchInputState,
    screen_x: f32,
    screen_y: f32,
) -> (f32, f32) {
    screen_point_to_world_with_camera(
        (screen_x, screen_y),
        (input.camera_x, input.camera_y),
        input.zoom_level,
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
    match_state: &MatchState,
    screen_x: f32,
    screen_y: f32,
) -> (u16, u16) {
    let (world_x, world_y) = screen_point_to_world(&match_state.input, screen_x, screen_y);
    world_point_to_cell(
        world_x,
        world_y,
        match_state
            .sim_view()
            .and_then(|view| tactical_cells(view.simulation())),
    )
}

#[cfg(all(test, tests_app_match))]
mod tests {
    use super::{
        ExactStepError, ExactStepReceipt, frame_ticks_inline, upsert_overlay_entries,
        validate_exact_step_receipt, world_point_to_cell,
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
    fn large_skirmish_outcome_wait_keeps_defeat_notices() {
        use crate::app::input::messages::HudMessage;
        use crate::rules::ini_parser::IniFile;
        use crate::sim::house_state::{HouseOutcomeKind, HouseState};
        use crate::sim::intern::InternedId;
        use crate::sim::runtime::SimRuntime;
        use crate::sim::scenario_session::ScenarioGameMode;
        use crate::sim::world::{FrameEffects, OutcomeWaitBoundary, Simulation, TickLane};

        struct HoldVoice;
        impl OutcomeWaitBoundary for HoldVoice {
            fn wait_complete(&mut self, _: InternedId, _: HouseOutcomeKind) -> bool {
                false
            }
        }

        // Make the last elimination and the start of the voice wait share a frame.
        let patch = IniFile::from_str("[AudioVisual]\nSavourDelay=0\n");
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
            "Hills.mmx",
            &patch,
        ) else {
            return;
        };
        let rules = retail.rules;
        assert_eq!(rules.general.savour_delay_minutes, 0.0);
        let mut sim = Simulation::with_seed(0x004F_C34F);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let last_slot = crate::skirmish_launch::SKIRMISH_MAX_SLOT_COUNT - 1;
        let owners: Vec<_> = (0..last_slot)
            .map(|slot| {
                let owner = sim.intern(&format!("Computer{}", slot + 1));
                sim.houses_mut_for_test()
                    .register(HouseState::new(owner, 0, None, false, 0, 10));
                owner
            })
            .collect();
        let local = sim.intern("Player");
        sim.houses_mut_for_test()
            .register(HouseState::new(local, 0, None, true, 0, 10));
        let session = sim.session_mut_for_test();
        session.set_game_mode(ScenarioGameMode::Skirmish);
        session.set_current_house_for_test(Some(local));
        session.set_tick_for_test(20_000);
        session.set_binary_frame_for_test(20_000);
        sim.spawn_object("GACNST", "Player", 10, 10, 0, &rules)
            .expect("the player holds a construction yard");

        let mut runtime = SimRuntime::from_simulation(sim);
        runtime.resources_mut().rules = rules;
        let first = runtime
            .advance_frame(
                &[],
                67,
                TickLane::Ordinary,
                &mut FrameEffects::empty().with_outcome_wait(&mut HoldVoice),
            )
            .unwrap();
        assert!(first.outcome_waiting);
        let messages = |output, runtime: &SimRuntime| {
            super::frame_hud_messages(
                output,
                &runtime.simulation,
                &runtime.resources.rules,
                Some("Player"),
            )
        };
        let defeats: Vec<_> = messages(&first, &runtime)
            .into_iter()
            .filter_map(|message| match message {
                HudMessage::PlayerDefeated(house) => Some(house),
                _ => None,
            })
            .collect();
        let last = runtime
            .resume_outcome_wait(&mut FrameEffects::empty())
            .unwrap();
        assert!(
            messages(&last, &runtime)
                .into_iter()
                .all(|message| !matches!(message, HudMessage::PlayerDefeated(_)))
        );
        let verdict = runtime
            .simulation
            .ready_outcome_for_owner(local)
            .unwrap()
            .kind;
        assert_eq!(verdict, HouseOutcomeKind::Victory);
        let mut banner =
            crate::app::presentation::outcome_banner::OutcomeBannerPresentation::default();
        assert_eq!(
            (defeats.len(), banner.hold_on_exit(verdict, false, true)),
            (owners.len(), true),
            "each defeat notice survives, and a verdict that had no draw gets a hold",
        );
        assert_eq!(defeats, owners);
        banner.record_presented(Some(verdict));
        assert!(!banner.hold_on_exit(verdict, false, true));
    }

    /// Online Neutral and Special keep the constructor's white laser colour.
    #[test]
    fn neutral_and_special_lasers_stay_white_online() {
        let mut sim = crate::sim::world::Simulation::with_seed(1);
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        let neutral = sim.interner_mut_for_test().intern("Neutral");
        let runtime = crate::sim::runtime::SimRuntime {
            simulation: sim,
            resources: std::sync::Arc::new(crate::sim::runtime::SimResources::empty()),
        };
        let colors = crate::map::houses::HouseColorMap::default();
        assert_eq!(
            super::house_laser_rgb(Some(&runtime), &colors, neutral),
            [0xFF; 3]
        );
    }

    /// A catch-up frame goes to the simulation thread when lending is
    /// measured cheaper, a frame that hashes goes whatever the estimate
    /// says, and the rest tick here as before.
    #[test]
    fn costly_frames_go_to_the_simulation_thread() {
        let ms = |n| Some(std::time::Duration::from_millis(n));
        // (hashes, another pass follows, last lent cost, expected cost)
        let inline = |hashes, follows, lent, expected: Option<_>| {
            frame_ticks_inline(false, false, hashes, follows, lent, expected.unwrap())
        };
        // Another pass follows: lent when the last lent frame cost less.
        assert!(!inline(false, true, ms(1), ms(4)));
        assert!(inline(false, true, ms(4), ms(1)));
        // A frame that hashes goes, even with lending dearer.
        assert!(!inline(true, true, ms(4), ms(1)));
        // The last frame of a pass, lending dearer: here, as before.
        assert!(inline(false, false, ms(4), ms(1)));
        // Frame stepping and exact steps always tick here.
        assert!(frame_ticks_inline(
            true,
            false,
            true,
            false,
            None,
            ms(1).unwrap()
        ));
    }

    #[test]
    fn gsi_04_10_dynamic_navigation_rebuild_refreshes_the_path_grid() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n",
            ))
            .unwrap();
        let speed_costs = SpeedCostProfile {
            foot: Some(crate::util::native_x87::NativeF32Bits::ONE),
            track: Some(crate::util::native_x87::NativeF32Bits::ONE),
            wheel: Some(crate::util::native_x87::NativeF32Bits::ONE),
            float: Some(crate::util::native_x87::NativeF32Bits::ONE),
            amphibious: Some(crate::util::native_x87::NativeF32Bits::ONE),
            float_beach: Some(crate::util::native_x87::NativeF32Bits::ONE),
            hover: Some(crate::util::native_x87::NativeF32Bits::ONE),
            winged: Some(crate::util::native_x87::NativeF32Bits::ONE),
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
    fn high_terrain_picking_round_trips_camera_and_zoom() {
        let cells = (0..32)
            .flat_map(|ry| {
                (0..32).map(move |rx| {
                    let mut cell = ResolvedTerrainCell::clear_for_test(rx, ry);
                    cell.level = 16;
                    cell
                })
            })
            .collect();
        let terrain = ResolvedTerrainGrid::from_cells(32, 32, cells);
        let camera = (-93.5, 80.25);
        let zoom = 1.5;
        let (x, y) = crate::map::terrain::iso_to_screen(16, 16, 16);
        let screen = ((x + 30.0 - camera.0) * zoom, (y + 15.0 - camera.1) * zoom);
        let world = super::screen_point_to_world_with_camera(screen, camera, zoom);
        assert_eq!(
            world_point_to_cell(world.0, world.1, Some(&terrain)),
            (16, 16)
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

#[cfg(all(test, tests_app_match))]
#[path = "camera_cadence_tests.rs"]
mod camera_cadence_tests;

#[cfg(all(test, tests_app_match))]
#[path = "modal_pump_tests.rs"]
mod modal_pump_tests;
