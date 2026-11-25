//! VERA's room's Quick Team Setup and Bulk Seat Actions: the host's two
//! lists in the team bar ([`crate::ui::team_tools`]), as the VERA
//! launcher's room has them. Each list's first item is its caption; an item
//! that cannot be had now is listed grey, with why. A pick goes through the
//! session's own seat and team owners, as the host's Teams count, row lists
//! and roster drags do.

use super::*;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::session::{NetworkRowKind, PeerChoicesChange};
use crate::app::match_runtime::state::MatchState;
use crate::app::process_assets::ProcessAssets;
use crate::skirmish_launch::{LaunchTeam, ROOM_TEAMS};
use crate::ui::multiplayer_setup::{SEAT_MENU, SetupChoice, SetupCombo, TEAM_MENU};
use crate::ui::team_tools::{
    self, PRESETS, SEAT_MENU_KEY, Seat, SeatAction, SeatCounts, TEAM_MENU_KEY, TeamPreset,
    TeamTool, ToolRefusal,
};
use crate::util::config::GameConfig;
use anyhow::Context;

/// The host's lists in VERA's room: Quick Team Setup in a room with teams
/// (`grouped`), Bulk Seat Actions wherever the team bar shows.
pub(super) fn combos(
    frontend: &FrontendState,
    match_state: &MatchState,
    grouped: bool,
    ai_allowed: bool,
    starting: bool,
) -> Vec<SetupCombo> {
    let session = &match_state.session;
    let Some(launch) = session.network_setup() else {
        return Vec::new();
    };
    if !super::dialog(frontend, match_state).modern || !session.host() {
        return Vec::new();
    }
    let menu = |id, key, items: Vec<SetupChoice>| SetupCombo {
        id,
        // No roster row: the host never drags it.
        row: u8::MAX,
        enabled: !starting,
        visible: true,
        selected: Some(0),
        choices: std::iter::once(choice(0, crate::i18n::tr(key), true))
            .chain(items)
            .collect(),
        list_w: 0,
    };
    let mut combos = Vec::new();
    if grouped {
        combos.push(menu(
            TEAM_MENU,
            TEAM_MENU_KEY,
            team_items(frontend, match_state, launch),
        ));
    }
    if grouped
        || super::view::room_size_shown(
            frontend.network_setup_protocol.as_ref(),
            &match_state.session,
        )
    {
        let items = team_tools::seat_actions(seat_counts(session, ai_allowed))
            .into_iter()
            .map(|(action, offered)| {
                choice(
                    action.code(),
                    team_tools::seat_action_text(action),
                    !offered,
                )
            })
            .collect();
        combos.push(menu(SEAT_MENU, SEAT_MENU_KEY, items));
    }
    combos
}

fn choice(value: i32, text: String, greyed: bool) -> SetupChoice {
    SetupChoice {
        value,
        text,
        swatch: None,
        greyed,
    }
}

/// Quick Team Setup's items: Team Starts and Team Colors while Teams is
/// set (Team Starts on a map the host has), online Balance Teams, then the
/// quick teams under their heading.
fn team_items(
    frontend: &FrontendState,
    match_state: &MatchState,
    launch: &SkirmishLaunchSession,
) -> Vec<SetupChoice> {
    let set = launch
        .options
        .teams
        .is_none()
        .then_some(ToolRefusal::TwoTeamsNeeded);
    let starts = set.or_else(|| {
        view::local_map_index(frontend, &match_state.session)
            .is_none()
            .then_some(ToolRefusal::Unavailable)
    });
    let mut tools = vec![(TeamTool::Starts, starts), (TeamTool::Colors, set)];
    if super::dialog(frontend, match_state).wol() {
        tools.push((TeamTool::Balance, None));
    }
    let item = |(tool, refusal): (TeamTool, Option<ToolRefusal>)| {
        choice(
            tool.code(),
            team_tools::team_tool_text(tool, refusal),
            refusal.is_some(),
        )
    };
    let heading = choice(-1, team_tools::presets_heading(), true);
    let presets =
        PRESETS.map(|preset| (TeamTool::Preset(preset), plan_preset(launch, preset).err()));
    tools
        .into_iter()
        .map(item)
        .chain(std::iter::once(heading))
        .chain(presets.into_iter().map(item))
        .collect()
}

/// The map's seats by what is in them: Open, Closed, and the room's AIs.
struct SeatRows {
    open: Vec<u8>,
    closed: Vec<u8>,
    ai: Vec<u8>,
}

fn seat_rows(session: &MatchSession) -> SeatRows {
    let seats = 0..session.network_player_row_limit();
    let with = |vacancy| {
        seats
            .clone()
            .filter(|row| session.network_vacancy(*row) == Some(vacancy))
            .collect()
    };
    SeatRows {
        open: with(NetworkVacancy::Open),
        closed: with(NetworkVacancy::Closed),
        ai: session
            .network_setup()
            .into_iter()
            .flat_map(|launch| &launch.opponents)
            .map(|ai| ai.raw_slot_index)
            .collect(),
    }
}

/// The host keeps two seats at least, in its Player lists and Close all.
pub(super) const KEPT_SEATS: usize = 2;

fn seat_counts(session: &MatchSession, ai_allowed: bool) -> SeatCounts {
    let rows = seat_rows(session);
    let kept = usize::from(session.network_player_row_limit()) - rows.closed.len();
    SeatCounts {
        open: rows.open.len(),
        closed: rows.closed.len(),
        ai: rows.ai.len(),
        closable: kept > KEPT_SEATS,
        ai_allowed,
    }
}

/// A quick team for the room: the Teams count it needs (`None`: Off) and
/// each seated row's team, or why it cannot be had. With Teams Off nobody
/// is on a team, unless the mode allies everyone on its default team, so
/// each person off a team stands alone, as on a team of their own: after
/// the AIs' teams when splitting the AIs. A quick team already in place is
/// refused, so picking it again leaves the guests' Accept.
fn plan_preset(
    launch: &SkirmishLaunchSession,
    preset: TeamPreset,
) -> Result<(Option<u8>, Vec<(u8, LaunchTeam)>), ToolRefusal> {
    let most = *ROOM_TEAMS.end();
    let teams = launch.options.teams;
    let rows = super::teams::seats(launch);
    let mut alone = match preset {
        TeamPreset::AiTeams(count) => count,
        _ => 0,
    };
    let seats: Vec<Seat> = rows
        .iter()
        .map(|&(row, team, _)| {
            let person = launch
                .humans
                .iter()
                .any(|human| human.raw_slot_index == Some(row) && !human.is_observer());
            if !person {
                return Seat::Ai(team);
            }
            if teams.is_some() || team != LaunchTeam::None {
                return Seat::Player(team);
            }
            alone += 1;
            Seat::Player(LaunchTeam::Team(alone - 1))
        })
        .collect();
    let planned = team_tools::preset_teams(preset, &seats, launch.mode.must_ally, most)?;
    if preset == TeamPreset::FreeForAll {
        return match teams {
            Some(_) => Ok((None, Vec::new())),
            None => Err(ToolRefusal::Unavailable),
        };
    }
    let moves: Vec<(u8, LaunchTeam)> = rows
        .iter()
        .zip(&seats)
        .zip(planned)
        .map(|((&(row, ..), seat), team)| {
            let (Seat::Player(own) | Seat::Ai(own)) = *seat;
            (row, team.unwrap_or(own))
        })
        .collect();
    let highest = moves
        .iter()
        .filter_map(|(_, team)| match team {
            LaunchTeam::Team(team) => Some(*team + 1),
            LaunchTeam::None => None,
        })
        .max()
        .unwrap_or(0);
    let count = teams.unwrap_or(0).max(highest).max(2);
    if count > most {
        return Err(ToolRefusal::NoFreeTeam);
    }
    let in_place = rows
        .iter()
        .zip(&moves)
        .all(|(&(_, on, _), &(_, team))| on == team);
    if teams == Some(count) && in_place {
        return Err(ToolRefusal::Unavailable);
    }
    Ok((Some(count), moves))
}

/// The host's pick from one of the lists, an item [`combos`] offered.
pub(super) fn select(
    game_config: Option<&GameConfig>,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &mut ProcessAssets,
    control: u16,
    value: i32,
) -> anyhow::Result<()> {
    match control {
        TEAM_MENU => match TeamTool::from_code(value).context("unknown Quick Team Setup item")? {
            TeamTool::Starts => {
                if super::teams::team_starts(
                    game_config,
                    frontend,
                    &mut match_state.session,
                    process_assets,
                    false,
                )? {
                    options_edited(frontend, match_state)?;
                }
                Ok(())
            }
            TeamTool::Colors => {
                if super::teams::team_colors(frontend, match_state)? {
                    options_edited(frontend, match_state)?;
                }
                Ok(())
            }
            TeamTool::Balance => {
                super::teams::balance(&mut frontend.network_setup_protocol);
                Ok(())
            }
            TeamTool::Preset(preset) => apply_preset(frontend, match_state, preset),
        },
        SEAT_MENU => apply_seat_action(
            frontend,
            match_state,
            SeatAction::from_code(value).context("unknown Bulk Seat Actions item")?,
        ),
        _ => anyhow::bail!("not a team bar list"),
    }
}

fn options_edited(
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
) -> anyhow::Result<()> {
    actions::host_option_edited(frontend, match_state)?;
    actions::queue(
        &mut frontend.network_setup_protocol,
        NetworkSetupRequest::PublishOptions,
    );
    Ok(())
}

/// A quick team: the Teams count it needs, then each seated row moved to
/// its team as the host's roster drag would move it.
fn apply_preset(
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    preset: TeamPreset,
) -> anyhow::Result<()> {
    let launch = match_state.session.network_setup().context("no setup")?;
    let (count, moves) = plan_preset(launch, preset)
        .map_err(|why| anyhow::anyhow!("quick team refused: {why:?}"))?;
    if match_state.session.network_teams() != count {
        super::teams::set_teams(
            &mut frontend.network_setup_protocol,
            &mut match_state.session,
            count,
        )?;
    }
    let mut change = PeerChoicesChange::default();
    let mut refused = Ok(());
    for (row, team) in moves {
        let on = super::teams::seats(match_state.session.network_setup().context("no setup")?)
            .into_iter()
            .find(|&(seat, ..)| seat == row)
            .map(|(_, on, _)| on);
        if on.is_none_or(|on| on == team) {
            continue;
        }
        match actions::put_on_team(frontend, match_state, row, team, false) {
            Ok(moved) => {
                change.roster |= moved.roster;
                change.options |= moved.options;
            }
            Err(error) => {
                refused = Err(error);
                break;
            }
        }
    }
    // Rows moved before a refused one are published too.
    actions::host_option_edited(frontend, match_state)?;
    super::service::publish_choices(frontend, match_state, change);
    refused
}

/// A Bulk Seat Actions item on the map's seats, each row changed as the
/// host's Player list would change it; Fill with AI's random difficulty is
/// drawn for each seat. Close all keeps two seats.
fn apply_seat_action(
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    action: SeatAction,
) -> anyhow::Result<()> {
    let session = &match_state.session;
    let rows = seat_rows(session);
    let mut kept = usize::from(session.network_player_row_limit()) - rows.closed.len();
    let changes: Vec<(u8, NetworkRowKind)> = match action {
        SeatAction::FillAi(difficulty) => rows
            .open
            .into_iter()
            .map(|row| {
                let difficulty = difficulty
                    .unwrap_or_else(crate::app::shell_skirmish_team_menus::random_difficulty);
                (row, NetworkRowKind::Ai(difficulty))
            })
            .collect(),
        SeatAction::CloseAll => rows
            .open
            .into_iter()
            .take_while(|_| {
                let close = kept > KEPT_SEATS;
                kept -= usize::from(close);
                close
            })
            .map(|row| (row, NetworkRowKind::Closed))
            .collect(),
        SeatAction::OpenClosed => rows
            .closed
            .into_iter()
            .map(|row| (row, NetworkRowKind::Open))
            .collect(),
        SeatAction::RemoveAi => rows
            .ai
            .into_iter()
            .map(|row| (row, NetworkRowKind::Open))
            .collect(),
    };
    for (row, kind) in changes {
        // Open seats, closed seats and AI rows hold no guest to kick.
        match_state.session.choose_network_row_kind(row, kind)?;
    }
    if super::dialog(frontend, match_state).wol() {
        actions::queue(
            &mut frontend.network_setup_protocol,
            NetworkSetupRequest::Advertise,
        );
    }
    actions::queue(
        &mut frontend.network_setup_protocol,
        NetworkSetupRequest::PublishOptions,
    );
    actions::queue(
        &mut frontend.network_setup_protocol,
        NetworkSetupRequest::PublishRoster,
    );
    Ok(())
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    /// A room with Teams Off, two people (rows 0 and 1) and two AIs (rows 2
    /// and 28): AIs in 2 teams puts the AIs on Teams A and B and each person
    /// on a team of their own after them, so Teams becomes four; Players vs
    /// AI needs two; Free-for-all is in place until Teams is set, as is
    /// Players vs AI once played. A mode that allies everyone keeps its
    /// people together on its default team, Team A, with Team A's AI.
    #[test]
    fn quick_teams_set_the_teams_count_they_need() {
        let rooms = crate::test_fixture::json("tests/fixtures/lobby/options.json");
        let mut launch: SkirmishLaunchSession =
            serde_json::from_value(rooms["rooms"]["generated_map_for_thirty"]["launch"].clone())
                .unwrap();
        let mut ai = launch.opponents[0].clone();
        ai.raw_slot_index = 2;
        launch.opponents.push(ai);
        let team = LaunchTeam::Team;
        assert_eq!(
            plan_preset(&launch, TeamPreset::AiTeams(2)),
            Ok((
                Some(4),
                vec![(0, team(2)), (1, team(3)), (2, team(0)), (28, team(1))]
            ))
        );
        assert_eq!(
            plan_preset(&launch, TeamPreset::PlayersVsAi),
            Ok((
                Some(2),
                vec![(0, team(0)), (1, team(0)), (2, team(1)), (28, team(1))]
            ))
        );
        assert_eq!(
            plan_preset(&launch, TeamPreset::AiTeams(3)),
            Err(ToolRefusal::FewAi(3))
        );
        assert_eq!(
            plan_preset(&launch, TeamPreset::FreeForAll),
            Err(ToolRefusal::Unavailable)
        );
        launch.options.teams = Some(2);
        for human in &mut launch.humans {
            human.slot.team = team(0);
        }
        for ai in &mut launch.opponents {
            ai.team = team(1);
        }
        assert_eq!(
            plan_preset(&launch, TeamPreset::PlayersVsAi),
            Err(ToolRefusal::Unavailable)
        );
        assert_eq!(
            plan_preset(&launch, TeamPreset::FreeForAll),
            Ok((None, Vec::new()))
        );
        launch.options.teams = None;
        launch.mode.must_ally = true;
        for ai in &mut launch.opponents {
            ai.team = team(0);
        }
        assert_eq!(
            plan_preset(&launch, TeamPreset::AiTeams(2)),
            Ok((
                Some(2),
                vec![(0, team(0)), (1, team(0)), (2, team(0)), (28, team(1))]
            ))
        );
    }
}
