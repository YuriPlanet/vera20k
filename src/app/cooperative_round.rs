//! The LAN Cooperative continuation: Cooperative `+0xA0` at a mission's
//! end, the result dialog BC6 and the next mission over the same session.
//!
//! The progress record belongs to `MultiplayerShellRuntime`'s
//! `CooperativeProgressState`; the participants, the agreed seed and BC6's
//! readiness to `MatchSession`'s `CooperativeRound` phase. This module
//! orders their calls the way `Main_Game__Route_Scenario_Termination`
//! (`0x0055CFD0`), `GameExit__Victory`/`Defeat` (`0x00685670`/`0x00685DC0`),
//! `+0xA8` (`0x005C42D0`), BC6's loop (`0x0046FC00`) and `+0xAC`
//! (`0x005C46E0`) do.
//!
//! RESIDUAL: BC6's country, colour and start combos do not edit; their
//! guest record `0x33` (`0x0046E377`, `0x0046E4F3`, `0x0046E632`,
//! `0x0046ECD8`) and the master's `0x0046D8B0` reply are not ported.
//! RESIDUAL: the dialog's 20 s keepalive `0x0D` (`0x0046EADD`) is not sent;
//! a peer's unhandled records still reach the loop's tail.

use crate::app::frontend::skirmish_session::cooperative::CooperativeMissionRow;
use crate::app::match_runtime::session::{CooperativeRoundEvent, CooperativeRoundStep};
use crate::app::{App, AppState, GameScreen};
use crate::ui::coop_score_shell::{CoopScoreAction, CoopScoreModel, CoopScorePage};
use crate::ui::lan_shell::LanChatLine;

/// A network Cooperative match: the router calls the mode's `+0xA0`
/// before GameExit (`0x0055D0C5..0x0055D0E2`, GameMode 3 or 4).
pub(crate) fn applies(state: &AppState) -> bool {
    state.match_state.session_mode().is_network()
        && state
            .match_state
            .sim_runtime
            .as_ref()
            .and_then(|runtime| runtime.simulation.session().multiplayer_mode_class())
            == Some(crate::skirmish_modes::MpModeClass::Cooperative)
}

/// Cooperative `+0xA0` (`0x005C3710`) at the terminal edge, with the
/// mission's elapsed ticks. "Won" is read from the Houses, not the
/// outcome: some human House is not defeated. A win with an active record
/// adds the mission, writes coopsave.ini and posts the save message; then
/// the record's next stage, matched case-sensitively as `<token>.MAP`,
/// becomes the scenario (a defeat, or no match, keeps the current one).
/// RESIDUAL: the tail's writes (`[0xA8B538]`, `0x004A8BF0`, the score
/// history column) have no VERA counterpart.
pub(crate) fn mission_ended(state: &mut AppState, elapsed_ticks: i32) {
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|runtime| &runtime.simulation)
    else {
        return;
    };
    let won = sim.cooperative_humans_won();
    let rows: Vec<CooperativeMissionRow> = sim
        .cooperative_score_rows()
        .into_iter()
        .map(|row| CooperativeMissionRow {
            name: crate::app::presentation::house_ui::house_ui_name(state, row.owner)
                .map(|name| crate::util::native_string::narrow_utf16_c_string(name.encode_utf16()))
                .unwrap_or_default(),
            kills: row.kills as i32,
            built: row.built as i32,
            lost: row.losses as i32,
            score: row.score,
        })
        .collect();
    if won {
        let house = save_houses(state);
        // 0x0049B7E0 leaves a missing row uninitialised; it reads as zero.
        let missing = CooperativeMissionRow::default();
        let rows = [
            rows.first().unwrap_or(&missing),
            rows.get(1).unwrap_or(&missing),
        ];
        if let Some(saved) = state
            .frontend
            .multiplayer_shell_runtime
            .record_cooperative_win(rows, elapsed_ticks, house)
        {
            crate::app::input::messages::post_cooperative_save_result(state, saved);
        }
    }
    let next = state
        .frontend
        .multiplayer_shell_runtime
        .cooperative_next_scenario_token()
        .map(|token| format!("{token}.MAP"));
    let index = next.and_then(|file| {
        state
            .frontend
            .scenario_catalog
            .records()
            .iter()
            .position(|record| record.file_name == file)
    });
    if let Some(index) = index {
        state
            .frontend
            .multiplayer_shell_runtime
            .set_scenario_index(index);
    }
}

/// `0x0049C5A0`'s House1/Color1 (node 0, the local player) and
/// House2/Color2 (the partner): `0x00696F10`/`0x00696F20`, -2 for a Random
/// choice.
/// RESIDUAL: VERA's resolved launch keeps no chosen-Random flag
/// (`node+0x4F`), so a player who chose Random saves the drawn country
/// instead of `<random>`. Nothing reads it back: the saved-campaign
/// reader's houses have only uncalled getters (`0x0049D0C0`).
fn save_houses(state: &AppState) -> [(i32, i32); 2] {
    let Some(launch) = state.match_state.session.network_setup() else {
        return [(-1, -1); 2];
    };
    let value = |slot: &crate::skirmish_launch::SkirmishLocalSlot| {
        (
            if slot.country_random {
                -2
            } else {
                i32::from(slot.country.raw_index())
            },
            if slot.color_random {
                -2
            } else {
                i32::from(slot.color_index)
            },
        )
    };
    let partner = launch
        .humans
        .iter()
        .enumerate()
        .find(|(index, human)| *index != launch.local_player_index && !human.is_observer())
        .map_or((-1, -1), |(_, human)| value(&human.slot));
    [value(launch.local_slot()), partner]
}

/// GameExit's co-op arm after the outcome (`0x00685851` skips the score
/// dialog): `+0xA8` runs BC6 when the progress record is active; without
/// one it returns false and the shell follows (`+0xA4` shows nothing).
/// The dialog's rows are this mission's live statistics (`0x0046DA70`),
/// read while the world is still resident.
pub(crate) fn open_result(state: &mut AppState, victory: bool) {
    let Some(can_continue) = state
        .frontend
        .multiplayer_shell_runtime
        .cooperative_can_continue()
    else {
        App::leave_mission_result_screen(state);
        return;
    };
    // GameExit stopped Scenario+0x614; +0x61C holds the mission's ticks.
    let elapsed = state
        .match_state
        .scenario_elapsed_clock
        .stop_buckets(crate::app::match_runtime::frame_pacer::wall_clock_ms());
    let Some(model) = build_model(state, can_continue, elapsed) else {
        App::leave_mission_result_screen(state);
        return;
    };
    // GameExit__Victory's tail saves the carry-over record (0x004C6180)
    // before Start_Scenario; the defeat path saves none.
    let saved = victory
        .then(|| {
            state
                .match_state
                .sim_runtime
                .as_ref()
                .map(|runtime| runtime.simulation.save_campaign_carry_over())
        })
        .flatten();
    let next_map = state
        .frontend
        .scenario_catalog
        .records()
        .get(state.frontend.multiplayer_shell_runtime.scenario_index())
        .map(|record| record.file_name.clone());
    let rows = model.rows.len();
    let campaign_end = victory
        .then(|| build_campaign_end(state, model.side))
        .flatten();
    // BC6 keeps the match's route; the shared teardown then finds no match
    // transport to return.
    crate::app::lan_service::cooperative::hold_transport(state);
    App::retire_finished_scenario(state);
    if let Err(error) =
        state
            .match_state
            .session
            .enter_cooperative_round(next_map, can_continue, rows)
    {
        log::error!("Could not open the co-op result dialog: {error:#}");
        return_to_shell(state);
        return;
    }
    // The repaint's country pass 0x0046F6D0 against the next stage.
    let runtime = &state.frontend.multiplayer_shell_runtime;
    let admit = |slot: &mut crate::skirmish_launch::SkirmishLocalSlot| {
        let requested = if slot.country_random {
            -2
        } else {
            i32::from(slot.country.raw_index())
        };
        if runtime.cooperative_admits(requested) {
            return;
        }
        match runtime.cooperative_default_country() {
            Some(country) => {
                slot.country = country;
                slot.country_random = false;
            }
            None => slot.country_random = true,
        }
    };
    if let Err(error) = state
        .match_state
        .session
        .admit_cooperative_round_countries(admit)
    {
        log::warn!("co-op round countries: {error:#}");
    }
    // 0x0072D300 (Scenario+0x34B8's palette) and 0x0060CF00's SCRBK.
    state.frontend.campaign_score_art = state.process_assets.manager().and_then(|assets| {
        crate::render::main_menu_shell_chrome::build_campaign_score_art(
            &state.renderer.gpu,
            &state.renderer.batch_renderer,
            assets,
            model.side,
            crate::app::frontend::shell_layout::art_key_screen_width(state),
        )
    });
    state.frontend.score_page = None;
    state.frontend.score_art = None;
    state.frontend.campaign_score_page = None;
    state.frontend.coop_score_page = Some(CoopScorePage {
        model,
        ui: Default::default(),
        victory,
        saved,
        campaign_end,
    });
    state.frontend.screen = GameScreen::MissionResult {
        title: String::new(),
        detail: String::new(),
    };
    state
        .frontend
        .shell_controller
        .reset_to(crate::ui::coop_score_shell::COOP_SCORE_DIALOG, false);
}

/// The tail `0x0046EE17`'s texts: rows sorted by score (`0x0046DC90`), the
/// time `TXT_TIME_FORMAT_HOURS` over the mission's ticks, the next
/// mission and map names (`A8B322`, the scenario `+0xA0` selected), the
/// game type and `NAME:COOPDESC<campaign+1><map+1>`.
fn build_model(state: &AppState, can_continue: bool, elapsed: i32) -> Option<CoopScoreModel> {
    use crate::app::frontend::shell_pass::resolve_csf;
    let sim = &state.match_state.sim_runtime.as_ref()?.simulation;
    let computer = resolve_csf(state, "TXT_COMPUTER");
    let sources = crate::app::presentation::house_ui::HouseUiSources::from_state(state, &computer);
    let mut rows: Vec<_> = sim
        .cooperative_score_rows()
        .into_iter()
        .map(|row| crate::ui::score_shell::ScoreRow {
            name: crate::app::presentation::house_ui::house_ui_name(state, row.owner)
                .unwrap_or_default(),
            rgb: sources.text_rgb(sim, row.owner),
            kills: row.kills,
            losses: row.losses,
            built: row.built,
            score: row.score,
        })
        .collect();
    crate::ui::score_shell::sort_rows(&mut rows);
    // s = T/60 signed, then the 99:59:59 clamp (0x0046EE34..0x0046EEE4).
    let time_model = crate::ui::score_shell::ScoreScreenModel {
        elapsed_seconds: (elapsed / 60) as u32,
        ..Default::default()
    };
    let time = crate::ui::score_shell::format_elapsed(
        &time_model,
        &resolve_csf(state, "TXT_TIME_FORMAT_HOURS"),
    );
    let side = crate::app::input::commands::preferred_local_owner_name(state)
        .as_deref()
        .and_then(|owner| sim.interner().get(owner))
        .and_then(|owner| sim.houses().get(&owner))
        .map_or(0, |house| house.side_index());
    let runtime = &state.frontend.multiplayer_shell_runtime;
    let catalog = &state.frontend.scenario_catalog;
    let map_index = catalog
        .records()
        .get(runtime.scenario_index())
        .and_then(|record| {
            catalog
                .shell_maps()
                .iter()
                .position(|map| map.file_name.eq_ignore_ascii_case(&record.file_name))
        });
    let map_name = map_index
        .and_then(|index| catalog.shell_maps().get(index))
        .map(|map| map.display_name.clone())
        .unwrap_or_default();
    let mode_key = state
        .match_state
        .session
        .network_setup()
        .and_then(|launch| {
            crate::skirmish_modes::mode_by_id(&state.frontend.skirmish_modes, launch.mode.id)
        })
        .map_or_else(|| "GUI:None".to_owned(), |mode| mode.ui_name_key.clone());
    let description = runtime
        .cooperative_setup_view()
        .description
        .map(|(campaign, stage)| {
            resolve_csf(
                state,
                &format!("NAME:COOPDESC{}{}", campaign + 1, stage + 1),
            )
            .into_owned()
        })
        .unwrap_or_default();
    Some(CoopScoreModel {
        rows,
        time,
        next_mission: map_name.clone(),
        mode_label: resolve_csf(state, &mode_key).into_owned(),
        map_label: map_name,
        description,
        side,
        map_index,
        can_continue,
    })
}

/// Cooperative `+0xA4`'s dialog (`0x005C44E0` -> `0x00471420`) after the
/// last stage's win: the first two human Houses' names and colours
/// (`0x0049B7E0`) with the record's campaign tallies (`0x0049BCE0`), sorted
/// by `0x0046DC90`; the time `TXT_TIME_FORMAT_HOURS` over the record's
/// ticks; `GUI:CampaignLabel` over the campaign's number; the difficulty
/// `[0xA8EB68]` through `GUI:DifficultyString`; and the campaign's
/// resolved `CampaignName`.
/// RESIDUAL: a missing second human House leaves `0x0049B7E0`'s row
/// uninitialised; here it has an empty name in the shell's yellow.
fn build_campaign_end(
    state: &AppState,
    side: i32,
) -> Option<crate::ui::campaign_score_shell::CampaignEndModel> {
    use crate::app::frontend::shell_pass::resolve_csf;
    use crate::assets::csf_file::{CsfArg, format_csf};
    let sim = &state.match_state.sim_runtime.as_ref()?.simulation;
    let computer = resolve_csf(state, "TXT_COMPUTER");
    let sources = crate::app::presentation::house_ui::HouseUiSources::from_state(state, &computer);
    let houses: Vec<_> = sim
        .cooperative_score_rows()
        .into_iter()
        .take(2)
        .map(|row| {
            (
                crate::app::presentation::house_ui::house_ui_name(state, row.owner)
                    .unwrap_or_default(),
                sources.text_rgb(sim, row.owner),
            )
        })
        .collect();
    let first_name = houses
        .first()
        .map(|(name, _)| crate::util::native_string::narrow_utf16_c_string(name.encode_utf16()))
        .unwrap_or_default();
    let runtime = &state.frontend.multiplayer_shell_runtime;
    let (campaign, tallies) = runtime.cooperative_campaign_end(&first_name)?;
    let (mut rows, time) = match tallies {
        Some((tallies, time)) => {
            let rows = tallies
                .iter()
                .enumerate()
                .map(|(index, &[kills, built, lost, score])| {
                    let (name, rgb) = houses
                        .get(index)
                        .cloned()
                        .unwrap_or_else(|| (String::new(), [255, 255, 0]));
                    crate::ui::score_shell::ScoreRow {
                        name,
                        rgb,
                        kills: kills as u32,
                        losses: lost as u32,
                        built: built as u32,
                        score,
                    }
                })
                .collect();
            (rows, time)
        }
        None => (Vec::new(), 0),
    };
    crate::ui::score_shell::sort_rows(&mut rows);
    // s = T/60, then the 99:59:59 clamp (0x00470E3C..0x00470EB6).
    let time = crate::ui::score_shell::format_elapsed(
        &crate::ui::score_shell::ScoreScreenModel {
            elapsed_seconds: (time / 60) as u32,
            ..Default::default()
        },
        &resolve_csf(state, "TXT_TIME_FORMAT_HOURS"),
    );
    let difficulty = match runtime.cooperative_difficulty() {
        level @ 0..=2 => {
            let level = resolve_csf(
                state,
                [
                    "TXT_DIFFICULTY_EASY",
                    "TXT_DIFFICULTY_MEDIUM",
                    "TXT_DIFFICULTY_BRUTAL",
                ][level as usize],
            );
            format_csf(
                &resolve_csf(state, "GUI:DifficultyString"),
                &[CsfArg::Str(&level)],
            )
        }
        _ => resolve_csf(state, "GUI:Blank").into_owned(),
    };
    let campaign_name = usize::try_from(campaign)
        .ok()
        .and_then(|campaign| runtime.cooperative_campaign_name(campaign))
        .map(|key| App::csf_label(state, key, key))
        .unwrap_or_default();
    Some(crate::ui::campaign_score_shell::CampaignEndModel {
        side,
        campaign_name,
        campaign_number: format_csf(
            &resolve_csf(state, "GUI:CampaignLabel"),
            &[CsfArg::Int(i64::from(campaign) + 1)],
        ),
        difficulty,
        time,
        rows,
    })
}

/// One human row of BC6 (`0x0046F600`, `0x0046F6D0`, `0x0046F8F0`,
/// `0x0046FAC0`, `0x0046FA20`).
pub(crate) struct CoopSlotView {
    pub(crate) name: String,
    pub(crate) country: crate::ui::multiplayer_setup::SetupChoice,
    pub(crate) flag: Option<&'static str>,
    pub(crate) color: crate::ui::multiplayer_setup::SetupChoice,
    pub(crate) start: crate::ui::multiplayer_setup::SetupChoice,
    /// `0x89E000[slot]`.
    pub(crate) ready: bool,
    /// `0x0046DCC0`/`0x0046DD70`: only the local row, and only while it is
    /// not ready.
    pub(crate) enabled: bool,
}

/// The round as BC6 shows it this frame.
pub(crate) struct CoopScoreView {
    pub(crate) host: bool,
    pub(crate) slots: [Option<CoopSlotView>; 2],
    pub(crate) continue_enabled: bool,
    pub(crate) quit_enabled: bool,
    pub(crate) difficulty: i32,
}

pub(crate) fn view(state: &AppState) -> Option<CoopScoreView> {
    use crate::app::frontend::skirmish_shell_render::{
        flag_pcx_for_side_item_data, side_item_data_for_country,
    };
    use crate::app::shell_network_setup::view::choice;
    use crate::ui::skirmish_shell::{SkirmishComboItem, SkirmishCountryChoice};
    let session = &state.match_state.session;
    let round = session.cooperative_round()?;
    let launch = session.network_setup()?;
    let mut slots: [Option<CoopSlotView>; 2] = [None, None];
    for (index, human) in launch.humans.iter().enumerate() {
        let Some(row) = human.raw_slot_index.filter(|row| *row < 2) else {
            continue;
        };
        let slot = &human.slot;
        // 0x005C23B0 lists the stage's countries with `GUI:Random` (-2).
        let (country, country_value) = if slot.country_random {
            (
                crate::ui::multiplayer_setup::SetupChoice {
                    value: -2,
                    text: crate::app::frontend::shell_pass::resolve_csf(state, "GUI:Random")
                        .into_owned(),
                    swatch: None,
                },
                -2,
            )
        } else {
            (
                choice(
                    state,
                    SkirmishComboItem::Country(SkirmishCountryChoice::Country(slot.country)),
                ),
                side_item_data_for_country(slot.country),
            )
        };
        let color_item = if slot.color_random {
            SkirmishComboItem::ColorSentinel(-2)
        } else {
            SkirmishComboItem::Color(usize::from(slot.color_index))
        };
        slots[usize::from(row)] = Some(CoopSlotView {
            name: human.name.clone(),
            country,
            flag: flag_pcx_for_side_item_data(country_value),
            color: choice(state, color_item),
            start: choice(state, SkirmishComboItem::Start(slot.start_position)),
            ready: session.cooperative_round_slot_ready(index),
            enabled: index == launch.local_player_index && !round.local_ready(),
        });
    }
    Some(CoopScoreView {
        host: session.host(),
        slots,
        continue_enabled: session.cooperative_round_continue_enabled(),
        quit_enabled: round.quit_enabled(),
        difficulty: state
            .frontend
            .multiplayer_shell_runtime
            .cooperative_difficulty(),
    })
}

/// A BC6 control's intent.
pub(crate) fn apply_action(state: &mut AppState, action: CoopScoreAction) {
    let result = match action {
        CoopScoreAction::Continue => continue_pressed(state),
        // 0x0046E28C: 0x24 to every node, result 0.
        CoopScoreAction::Quit => state
            .match_state
            .session
            .quit_cooperative_round()
            .map(|()| CooperativeRoundStep::Exit(false)),
        CoopScoreAction::Chat(text) => {
            send_chat(state, text);
            Ok(CooperativeRoundStep::None)
        }
    };
    match result {
        Ok(step) => run_step(state, step),
        Err(error) => log::warn!("co-op result command refused: {error:#}"),
    }
    state.platform.window.request_redraw();
}

fn continue_pressed(state: &mut AppState) -> anyhow::Result<CooperativeRoundStep> {
    anyhow::ensure!(
        state
            .match_state
            .session
            .cooperative_round_continue_enabled(),
        "Continue is disabled"
    );
    let difficulty = state
        .frontend
        .multiplayer_shell_runtime
        .cooperative_difficulty();
    state
        .match_state
        .session
        .continue_cooperative_round(difficulty)
}

/// One decoded BC6 packet from a peer.
pub(crate) fn service_event(
    state: &mut AppState,
    event: CooperativeRoundEvent,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        state.frontend.coop_score_page.is_some(),
        "no co-op result dialog"
    );
    let difficulty = state
        .frontend
        .multiplayer_shell_runtime
        .cooperative_difficulty();
    let step = state
        .match_state
        .session
        .cooperative_round_event(event, difficulty)?;
    run_step(state, step);
    state.platform.window.request_redraw();
    Ok(())
}

fn run_step(state: &mut AppState, step: CooperativeRoundStep) {
    match step {
        CooperativeRoundStep::None => {}
        CooperativeRoundStep::Assign => {
            // Native cannot fail here. A refused assignment has sent no
            // begin; leaving with `0x24` returns every peer's Quit.
            if let Err(error) = assign(state) {
                log::error!("co-op round assignment failed: {error:#}");
                if let Err(error) = state.match_state.session.quit_cooperative_round() {
                    log::warn!("co-op round quit refused: {error:#}");
                }
                finish(state);
            }
        }
        CooperativeRoundStep::OptionsApplied { difficulty, .. } => state
            .frontend
            .multiplayer_shell_runtime
            .set_cooperative_difficulty(difficulty),
        CooperativeRoundStep::PeerLeft { source } => {
            // 0x005E2700(-1, GUI:PlayerLeftCoop, name).
            let name = participant_name(state, source);
            let format = crate::app::frontend::shell_pass::resolve_csf(state, "GUI:PlayerLeftCoop");
            let text = crate::assets::csf_file::format_csf(
                &format,
                &[crate::assets::csf_file::CsfArg::Str(&name)],
            );
            append_line(state, text, u32::MAX);
        }
        CooperativeRoundStep::Chat {
            source,
            text,
            color,
        } => {
            let name = participant_name(state, source);
            append_line(state, format!("{name}: {text}"), color);
        }
        CooperativeRoundStep::Exit(true) => begin_next_mission(state),
        CooperativeRoundStep::Exit(false) => finish(state),
    }
}

/// The master's all-ready (`0x0046FD47`): mode `+0x68` and
/// `SessionClass::ProcessRandomAssignments` (`0x0069B8C0`) on the gameplay
/// Scenario stream the match returned, then its own ack and `0x32`.
fn assign(state: &mut AppState) -> anyhow::Result<()> {
    let launch = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("no round roster"))?
        .clone();
    let map = state
        .frontend
        .scenario_catalog
        .shell_maps()
        .iter()
        .find(|map| {
            launch
                .selected_map_file
                .as_deref()
                .is_some_and(|file| file.eq_ignore_ascii_case(&map.file_name))
        })
        .ok_or_else(|| anyhow::anyhow!("the next mission is unavailable"))?;
    let preparation = state
        .frontend
        .multiplayer_shell_runtime
        .prepare_network_mode_start(&launch, map)?;
    state
        .match_state
        .session
        .apply_network_ai_preparation(preparation.declared_count, preparation.opponents)?;
    let launch = state
        .match_state
        .session
        .network_setup()
        .expect("the round keeps its roster")
        .clone();
    let resolved = state
        .frontend
        .multiplayer_shell_runtime
        .resolve_network_assignments(&launch)?;
    state
        .match_state
        .session
        .accept_network_assignments(resolved)?;
    let difficulty = state
        .frontend
        .multiplayer_shell_runtime
        .cooperative_difficulty();
    state
        .match_state
        .session
        .cooperative_round_assigned(difficulty)
}

/// Result 1: `+0xAC` keeps the agreed seed and takes the continuation
/// timing, then Start_Scenario loads the selected mission: the next one
/// after a win, the same one after a defeat.
/// RESIDUAL: Start_Scenario's retry loop (`0x00685C47`) has no VERA
/// counterpart; a failed load returns to the lobby.
/// RESIDUAL: the scenario movies around a continuation (Win, PostScore and
/// PreMapSelect after a victory, Lose after a defeat, then Intro/Brief and
/// Action on a victory) have no owner. No retail co-op map sets them.
fn begin_next_mission(state: &mut AppState) {
    let Some(page) = state.frontend.coop_score_page.take() else {
        return;
    };
    let game_speed = state
        .match_state
        .session
        .network_setup()
        .map_or(0, |launch| launch.options.game_speed);
    // LAN has no writer of the ping table 0xB7790C: every ping reads 0.
    let (max_ahead, latency_fudge) =
        crate::net::lockstep::cooperative_continuation_timing(game_speed, &[]);
    state.frontend.campaign_score_art = None;
    match state
        .match_state
        .session
        .freeze_cooperative_round(max_ahead, latency_fudge, page.saved)
    {
        Ok(startup) => crate::app::loading::pump::begin_loading(
            state,
            crate::app::loading::pump::LoadingRequest::network(startup),
        ),
        Err(error) => {
            log::error!("Could not continue the co-op campaign: {error:#}");
            return_to_shell(state);
        }
    }
}

/// Result 0: `+0xA4` shows the campaign-end dialog `0xBC7` only after the
/// last mission's win, then the shell. BC6's records have gone out first;
/// `0xBC7` services no network.
/// RESIDUAL: the session keeps its round phase until the shell resumes, so
/// the round's poll pops and drops packets that arrive while `0xBC7` is
/// open; native leaves them queued for the lobby. Lobby broadcasts repeat,
/// so nothing visible is lost.
fn finish(state: &mut AppState) {
    let campaign_end = state
        .frontend
        .coop_score_page
        .take()
        .and_then(|page| page.campaign_end);
    match campaign_end {
        Some(model) => {
            crate::app::lan_service::cooperative::release_transport(state);
            App::open_campaign_end_page(state, model);
        }
        None => return_to_shell(state),
    }
}

/// Main_Game's return after GameExit: the session toggles persist, then
/// the mode's shell (LAN: a new BB).
fn return_to_shell(state: &mut AppState) {
    crate::app::lan_service::cooperative::release_transport(state);
    leave_result(state);
}

/// `0xBC7`'s Continue (`0x6D1`, result 1) after its slide: the shell.
pub(crate) fn campaign_end_continue(state: &mut AppState) {
    state.frontend.campaign_score_page = None;
    leave_result(state);
}

fn leave_result(state: &mut AppState) {
    state.frontend.coop_score_page = None;
    state.frontend.campaign_score_art = None;
    App::persist_returned_session_toggles(state);
    state.frontend.screen = GameScreen::MainMenu;
    App::resume_shell_after_match(state);
}

/// BC6's chat (`0x540` Enter): a text over two characters echoes locally
/// as `"%s: %s"` in `[0x7EEF44]` (`0x0046E12C`) and goes to every other
/// node as `0x0C` with `[0x7EEF2C]` (`0x0046E126`) and this game's hash.
fn send_chat(state: &mut AppState, text: Vec<u16>) {
    let text = String::from_utf16_lossy(&text);
    let text = text.trim_end_matches(['\r', '\n']).to_owned();
    if text.encode_utf16().count() <= 2 {
        return;
    }
    let name = state
        .match_state
        .session
        .local_player_name()
        .unwrap_or_default()
        .to_owned();
    append_line(
        state,
        format!("{name}: {text}"),
        crate::ui::wol_chat::SAY_COLOR,
    );
    let local = state
        .match_state
        .session
        .network_setup()
        .map_or(0, |launch| launch.local_player_index);
    let recipients: Vec<usize> = state
        .match_state
        .session
        .network_connected_source_indices()
        .iter()
        .copied()
        .filter(|index| *index != local)
        .collect();
    state.match_state.session.request_chat(recipients, text);
}

fn participant_name(state: &AppState, source: usize) -> String {
    state
        .match_state
        .session
        .network_setup()
        .and_then(|launch| launch.humans.get(source))
        .map(|human| human.name.clone())
        .unwrap_or_default()
}

/// `0x005E2700` with a `COLORREF` (-1 the list's default) into BC6's list
/// `0x53F`.
fn append_line(state: &mut AppState, text: String, color: u32) {
    let layout = crate::app::input::coop_score::layout(state);
    let rect = layout
        .control_rect(crate::ui::coop_score_shell::CHAT_LOG)
        .expect("BC6 lays out its chat list");
    let scale = layout.page.scale;
    let font = &state.renderer.bit_font;
    let Some(page) = state.frontend.coop_score_page.as_mut() else {
        return;
    };
    let chat = &mut page.ui.chat;
    let visible = chat
        .list
        .geometry(rect, chat.lines.len(), scale)
        .visible_rows;
    chat.append_message(
        LanChatLine {
            text: text.encode_utf16().collect(),
            color,
        },
        scale.base_rect(rect).w,
        visible,
        |units| font.text_width(&String::from_utf16_lossy(units)) as i32,
    );
}
