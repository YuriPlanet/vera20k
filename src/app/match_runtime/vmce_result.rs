//! VMCE settlement at the simulation's committed terminal outcome.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::app::AppState;
use crate::app::presentation::house_ui::HouseUiSources;
use crate::app::shell_lan::LanRematch;
use crate::app::shell_network_setup::local_scenarios::{StageContinuation, stage_continuation};
use crate::sim::house_state::HouseOutcomeKind;
use crate::skirmish_launch::SkirmishHumanSlot;
use crate::ui::score_shell::{ScoreRow, ScoreScreenModel};

use super::scenario_exit::ScenarioExitDestination;
use super::session::LocalScenarioPlayer;
use super::sim_tick::build_score_header_from_sources;

#[path = "vmce_result/defeat_ui.rs"]
mod defeat_ui;
#[path = "vmce_result/retry.rs"]
mod retry;
#[path = "vmce_result/statistics_ui.rs"]
mod statistics_ui;
#[path = "vmce_result/victory_ui.rs"]
mod victory_ui;

pub(crate) use retry::{RetryRound, poll_result, receive_result};

#[derive(Debug)]
pub(super) struct SeatResult {
    pub(super) row: u8,
    pub(super) house: String,
    pub(super) outcome: Option<HouseOutcomeKind>,
    pub(super) score: ScoreRow,
}

/// Admit exactly the starting participants independently of the role table.
/// Both the host and the guest project in lobby row order, even though their
/// launch source order is local-first. Departures cannot shrink this list.
fn primary_seats<'a>(
    humans: &'a [SkirmishHumanSlot],
    assignments: &'a [LocalScenarioPlayer],
) -> Result<Vec<(&'a SkirmishHumanSlot, &'a LocalScenarioPlayer)>> {
    let roles: BTreeMap<_, _> = assignments
        .iter()
        .map(|player| (player.raw_slot_index, player))
        .collect();
    ensure!(
        roles.len() == assignments.len(),
        "duplicate assigned result seat"
    );
    let mut seen = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut seats = Vec::with_capacity(roles.len());
    for human in humans.iter().filter(|human| !human.is_observer()) {
        let row = human
            .raw_slot_index
            .context("result participant has no admitted seat")?;
        ensure!(seen.insert(row), "duplicate admitted result seat");
        ensure!(
            names.insert(human.name.to_ascii_lowercase()),
            "duplicate result name"
        );
        let role = roles.get(&row).context("missing result seat assignment")?;
        seats.push((human, *role));
    }
    ensure!(
        !seats.is_empty() && seats.len() == roles.len(),
        "result assignments differ from the admitted roster"
    );
    seats.sort_unstable_by_key(|(_, role)| role.raw_slot_index);
    Ok(seats)
}

pub(super) fn settle(state: &AppState, outcome: HouseOutcomeKind) -> Result<Vec<SeatResult>> {
    let start = state
        .match_state
        .session
        .frozen_authored_start()
        .context("no admitted VMCE start")?;
    let launch = start.launch_session();
    let authored = start.authored_scenario().context("no frozen VMCE roles")?;
    let roles = &authored.players;
    let sim = &state
        .match_state
        .sim_runtime
        .runtime()
        .context("no completed simulation")?
        .simulation;
    let terminal = sim
        .terminal_score_snapshot()
        .context("no terminal House statistics")?;
    let local = sim
        .session()
        .current_house()
        .context("no local result House")?;
    let colors = HouseUiSources::from_state(state, "");
    let connected: BTreeSet<_> = state
        .match_state
        .session
        .network_connected_source_indices()
        .iter()
        .filter_map(|&source| start.authored_owner(source))
        .filter_map(|name| sim.interner().get(name))
        .collect();
    let mut primary_houses = BTreeSet::new();
    let mut results = Vec::new();
    for (human, role) in primary_seats(&launch.humans, roles)? {
        let owner = sim
            .interner()
            .get(&role.house)
            .context("missing result House")?;
        let house = sim
            .houses()
            .get(&owner)
            .context("missing result House state")?;
        ensure!(
            primary_houses.insert(owner),
            "duplicate primary result House"
        );
        if house.left_game() || !connected.contains(&owner) {
            continue;
        }
        let outcome = if owner == local {
            Some(outcome)
        } else {
            sim.authored_house_outcome(owner)
        };
        let (kills, losses, built, score) =
            match terminal.rows.iter().find(|row| row.owner == owner) {
                Some(row) => (row.kills, row.losses, row.built, row.score),
                // The base score page excludes passive Houses from its scoring
                // and RNG. An admitted seat still gets its counters at this edge.
                None if house.multiplay_passive() => {
                    let stats = house.stats();
                    (stats.kills(), stats.losses(), stats.built(), 0)
                }
                None => anyhow::bail!("missing terminal score for {}", role.house),
            };
        results.push(SeatResult {
            row: role.raw_slot_index,
            house: role.house.clone(),
            outcome,
            score: ScoreRow {
                name: human.name.clone(),
                rgb: colors.text_rgb(sim, owner),
                kills,
                losses,
                built,
                score,
            },
        });
    }
    Ok(results)
}

pub(super) fn destination(
    state: &mut AppState,
    outcome: HouseOutcomeKind,
    title: &str,
    detail: &str,
    elapsed_score_ticks: i32,
    score_model: impl FnOnce(&AppState, Vec<ScoreRow>) -> ScoreScreenModel,
) -> Option<ScenarioExitDestination> {
    let session = &state.match_state.session;
    let start = session.frozen_authored_start()?;
    let authored = start.authored_scenario()?;
    let rows = match settle(state, outcome) {
        Ok(rows) => rows,
        Err(error) => {
            log::error!("VMCE settlement could not read the terminal match: {error:#}");
            return None;
        }
    };
    if let Some(LanRematch::Guest { host, .. }) = &state.frontend.lan_rematch
        && let Some(source) = start
            .launch_session()
            .humans
            .iter()
            .position(|player| player.name.eq_ignore_ascii_case(host))
    {
        let sim = &state.match_state.sim_runtime.runtime()?.simulation;
        let host_left = start
            .authored_owner(source)
            .and_then(|owner| sim.interner().get(owner))
            .and_then(|house| sim.houses().get(&house))
            .is_some_and(|house| house.left_game());
        if host_left || !session.network_connected_source_indices().contains(&source) {
            state.frontend.lan_rematch = None;
        }
    }
    // This callback runs at the committed terminal frame before teardown,
    // with no further simulation ticks. Read the branch flags at the same
    // boundary as the terminal score. The host applies the resolved choice
    // through normal selection when the room reopens.
    let mut continuation = victory_ui::Continuation::Room;
    if outcome == HouseOutcomeKind::Victory {
        if let Some(LanRematch::Host { carry_over, .. }) = &mut state.frontend.lan_rematch {
            *carry_over = None;
        }
        let sim = &state.match_state.sim_runtime.runtime()?.simulation;
        let terminal_globals = sim.global_variables();
        match stage_continuation(
            state,
            &authored.selection.key,
            &authored.configuration_sha256,
            &terminal_globals,
        ) {
            Ok(StageContinuation::Next { key, funds }) => {
                if let Some(LanRematch::Host {
                    preselected,
                    carry_over,
                    ..
                }) = &mut state.frontend.lan_rematch
                {
                    let roles = funds.into_iter().filter_map(|(source, destination)| {
                        let player = authored
                            .players
                            .iter()
                            .find(|player| player.house.eq_ignore_ascii_case(&source))?;
                        Some((player.house.as_str(), destination))
                    });
                    *carry_over = Some(sim.capture_authored_stage_carry_over(roles));
                    *preselected = Some(key);
                }
            }
            Ok(StageContinuation::Credits(text)) => {
                continuation = victory_ui::Continuation::Credits(text);
                if let Some(LanRematch::Host { preselected, .. }) = &mut state.frontend.lan_rematch
                {
                    *preselected = Some(authored.selection.key.clone());
                }
            }
            Ok(StageContinuation::SameStage) => {}
            Err(error) => log::warn!("Keeping the current VMCE stage on return: {error:#}"),
        }
    }
    for row in &rows {
        log::debug!(
            "VMCE result row {}: {} ({}) {:?}",
            row.row,
            row.score.name,
            row.house,
            row.outcome
        );
    }
    let (title, detail) = (title.to_owned(), detail.to_owned());
    let score = score_model(state, rows.iter().map(|row| row.score.clone()).collect());
    let ranking = &state.match_state.map_ranking;
    let defaults = crate::map::ranking::RankingSection::default();
    let unconfigured = [
        defaults.under_par_title,
        defaults.under_par_message,
        defaults.over_par_title,
        defaults.over_par_message,
    ];
    // Both result pages use the map's normal par time.
    let mut model = build_score_header_from_sources(
        elapsed_score_ticks,
        ranking.par_time_medium_ticks,
        ranking,
        rows.iter().map(|row| (true, row.score.kills)),
        score.side,
        |key| {
            if let Some(text) = authored
                .selection
                .presentation
                .as_ref()
                .and_then(|presentation| presentation.report_strings.get(key))
            {
                text.clone()
            } else if unconfigured
                .iter()
                .any(|default| default.eq_ignore_ascii_case(key))
            {
                String::new()
            } else {
                state
                    .process_assets
                    .csf
                    .as_ref()
                    .and_then(|csf| csf.get(key))
                    .unwrap_or_default()
                    .to_owned()
            }
        },
    );
    model.theme = score.theme;
    Some(match outcome {
        HouseOutcomeKind::Victory => {
            if model.rank.trim().is_empty() {
                model.rank = crate::i18n::tr("VERA:VmceReportTitle");
            }
            if model.commendation.trim().is_empty() {
                model.commendation = crate::i18n::tr("VERA:VmceReportComplete");
            }
            let overlay = victory_ui::overlay(rows, &model, continuation);
            ScenarioExitDestination::LanStandaloneScore {
                title,
                detail,
                model,
                overlay,
            }
        }
        HouseOutcomeKind::Defeat => {
            let can_vote = rows
                .iter()
                .all(|row| row.outcome == Some(HouseOutcomeKind::Defeat))
                && state
                    .match_state
                    .session
                    .network_setup()
                    .is_some_and(|launch| !launch.local_player().is_observer());
            let overlay =
                defeat_ui::overlay(rows, &model, can_vote, Some(authored.selection.clone()));
            ScenarioExitDestination::LanRoomScore {
                title,
                detail,
                side: model.side,
                theme: model.theme,
                overlay,
            }
        }
    })
}

#[cfg(all(test, tests_app_match))]
mod tests {
    use super::*;
    use crate::skirmish_launch::{LaunchHumanRole, SkirmishLaunchSession};

    #[test]
    fn result_seats_use_the_original_roster_even_when_a_role_is_missing() {
        let source: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.command-bar.example.json",
        ))
        .unwrap();
        let launch: SkirmishLaunchSession =
            serde_json::from_value(source["launch"].clone()).unwrap();
        let human = |row, name: &str, role| SkirmishHumanSlot {
            raw_slot_index: Some(row),
            name: name.to_owned(),
            slot: launch.humans[0].slot.clone(),
            role,
        };
        let playing = LaunchHumanRole::default();
        let humans = [
            human(1, "Guest", playing),
            human(2, "Observer", LaunchHumanRole::Observer),
            human(0, "Host", playing),
        ];
        let roles = [
            LocalScenarioPlayer {
                raw_slot_index: 1,
                role: "Player1".into(),
                house: "PrimaryA".into(),
                shared_control_houses: vec!["SharedB".into()],
            },
            LocalScenarioPlayer {
                raw_slot_index: 0,
                role: "Player2".into(),
                house: "PrimaryC".into(),
                shared_control_houses: Vec::new(),
            },
        ];
        let projected: Vec<_> = primary_seats(&humans, &roles)
            .unwrap()
            .into_iter()
            .map(|(seat, role)| (seat.name.as_str(), role.house.as_str()))
            .collect();
        assert_eq!(projected, [("Host", "PrimaryC"), ("Guest", "PrimaryA")]);
        assert!(primary_seats(&humans, &roles[1..]).is_err());
    }
}
