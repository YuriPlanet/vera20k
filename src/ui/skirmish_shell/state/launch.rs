//! Launch session conversion for the skirmish shell.

use crate::map::scenario_menu::MapMenuEntry;
use crate::skirmish_launch::{
    LaunchStartPosition, LaunchTeam, LaunchValidationError, SkirmishAiSlot, SkirmishLaunchMode,
    SkirmishLaunchSession, SkirmishLocalSlot,
};
use crate::skirmish_modes::SkirmishGameMode;

use super::SkirmishShellState;

fn launch_start_position(
    slot: usize,
    start_position: LaunchStartPosition,
) -> Result<LaunchStartPosition, LaunchValidationError> {
    match start_position {
        LaunchStartPosition::Auto => Ok(LaunchStartPosition::Auto),
        // VERA's maps with more than eight starts list them up to
        // SKIRMISH_MAX_SLOT_COUNT, the domain the start reader owns.
        LaunchStartPosition::Position(position) => {
            LaunchStartPosition::from_shell_value(i32::from(position))
                .ok_or(LaunchValidationError::InvalidStartPosition { slot, position })
        }
    }
}

/// The shell's own Team list offers -2 (no team) and the native teams
/// (0..=[`NATIVE_TEAMS`](crate::skirmish_launch::NATIVE_TEAMS)-1), so every
/// team the shell holds is inside the team domain; a value outside it is no
/// team.
pub(super) fn shell_team(value: i32) -> LaunchTeam {
    LaunchTeam::from_shell_value(value).unwrap_or(LaunchTeam::None)
}

/// A picked colour has to be one the lobby lists. A Random row's stored
/// colour is only what it last showed and the draw replaces it, so one the
/// lobby no longer lists packs as the first colour instead of stopping Start.
fn launch_color_index(
    state: &SkirmishShellState,
    slot: usize,
    color_index: usize,
    claimed: bool,
) -> Result<u8, LaunchValidationError> {
    let listed = u8::try_from(color_index)
        .ok()
        .filter(|&color| state.lobby_colors.is_lobby_priority(i32::from(color)));
    match listed {
        Some(color) => Ok(color),
        None if !claimed => Ok(0),
        None => Err(LaunchValidationError::InvalidColorIndex { slot, color_index }),
    }
}

/// Whether Start skips the "at least two players" and "more than one team"
/// gates ([`launch_session`]'s `allow_single_team`): `[VERA] SoloSkirmish=`,
/// or a front end Ares plays in. Ares_AllowSinglePlay (hook `0x006AD0ED`,
/// Ares.dll `10003AD0`) jumps from after the capacity check over both
/// (`TXT_NEED_AT_LEAST_TWO_PLAYERS`, `TXT_CANNOT_ALLY`) to `0x006AD2BA`.
/// `frontend_rules` are the shell's Rules, whose
/// [`ares`](crate::rules::ruleset::RuleSet::ares) answers for the front end.
pub fn start_skips_side_gates(
    solo_skirmish: bool,
    frontend_rules: Option<&crate::rules::ruleset::RuleSet>,
) -> bool {
    solo_skirmish || frontend_rules.is_some_and(crate::rules::ruleset::RuleSet::ares)
}

/// Pack the current controls into a Start session, or refuse a match that has
/// fewer than two sides.
///
/// `allow_single_team` ([`start_skips_side_gates`]) accepts a match with no
/// opponent at all and one with every active AI on the local player's own
/// explicit team. Retail keeps both the "at least two players" and the "more
/// than one team" gates.
pub fn launch_session(
    state: &SkirmishShellState,
    maps: &[MapMenuEntry],
    modes: &[SkirmishGameMode],
    allow_single_team: bool,
) -> Result<SkirmishLaunchSession, LaunchValidationError> {
    let selected_map = maps
        .get(state.selected_map_idx)
        .ok_or(LaunchValidationError::NoSelectedMap)?;

    let active_count = state
        .opponents
        .iter()
        .filter(|opponent| opponent.is_active())
        .count();
    let requested_players = active_count + 1;
    let capacity = selected_map.player_capacity;
    if capacity < i32::try_from(requested_players).unwrap_or(i32::MAX) {
        return Err(LaunchValidationError::MapCapacityExceeded {
            capacity,
            requested_players,
        });
    }
    // Retail keeps its "at least two players" gate.
    if active_count == 0 && !allow_single_team {
        return Err(LaunchValidationError::NoEnabledOpponent);
    }
    // With no opponent active there is no team conflict to report.
    if state.player_team >= 0 && active_count > 0 && !allow_single_team {
        let local_team = state.player_team as u8;
        let all_active_ai_same_team = state
            .opponents
            .iter()
            .filter(|opponent| opponent.is_active())
            .all(|opponent| shell_team(opponent.team) == LaunchTeam::Team(local_team));
        if all_active_ai_same_team {
            return Err(LaunchValidationError::SameExplicitTeam { team: local_team });
        }
    }

    pack_launch_session_without_start_validation(state, maps, modes)
}

/// Pack the current controls into a launch-shaped copy without running the
/// Start-only capacity, minimum-player, or same-team validation gates.
///
/// Back follows the same native control-packing/randomization transaction as
/// Start, so app code needs this conversion even when no match will launch.
/// Structural conversion failures (missing map/mode or invalid typed values)
/// remain errors because there is no faithful `SkirmishLaunchSession` to return.
pub fn pack_launch_session_without_start_validation(
    state: &SkirmishShellState,
    maps: &[MapMenuEntry],
    modes: &[SkirmishGameMode],
) -> Result<SkirmishLaunchSession, LaunchValidationError> {
    let selected_map = maps
        .get(state.selected_map_idx)
        .ok_or(LaunchValidationError::NoSelectedMap)?;
    let selected_mode =
        state
            .selected_mode
            .resolve(modes)
            .ok_or(LaunchValidationError::NoSelectedMode {
                mode_id: state.selected_mode.id,
            })?;

    let local = SkirmishLocalSlot {
        country: state.player_country,
        country_random: state.player_country_random,
        color_index: launch_color_index(
            state,
            0,
            state.player_color_index,
            state.player_color_claimed,
        )?,
        color_random: !state.player_color_claimed,
        start_position: launch_start_position(0, state.player_start_position)?,
        team: shell_team(state.player_team),
    };

    let mut opponents = Vec::new();
    for (idx, opponent) in state.opponents.iter().enumerate() {
        let Some(difficulty) = opponent.row_type.difficulty() else {
            continue;
        };
        let slot = idx + 1;
        opponents.push(SkirmishAiSlot {
            raw_slot_index: (idx + 1) as u8,
            country: opponent.country,
            country_random: opponent.country_random,
            color_index: launch_color_index(
                state,
                slot,
                opponent.color_index,
                opponent.color_claimed,
            )?,
            color_random: !opponent.color_claimed,
            start_position: launch_start_position(slot, opponent.start_position)?,
            team: shell_team(opponent.team),
            difficulty,
        });
    }

    // Start from the per-match base seeded from `[MultiplayerDialogSettings]`
    // (stock defaults until then). The fields the setup dialog exposes as
    // widgets are overridden from the live shell state below; the remaining
    // base fields (tech level and the non-widget toggles) carry into the match.
    let mut options = state.launch_options_base.clone();
    options.starting_credits = state.credits();
    options.unit_count = state.unit_count;
    options.game_speed = state.game_speed;
    options.short_game = state.short_game;
    options.super_weapons = state.super_weapons;
    options.build_off_ally = state.build_off_ally;
    options.crates = state.crates;
    options.mcv_redeploy = state.mcv_redeploy;
    // Then SkirmishDialog's Start/Back packing (`0x006AD88F..0x006AD8A4`)
    // forces Bridge Destruction (`0x00A8B260`) on, and Fog of War, Harvester
    // Truce and MultiEngineer off. VERA's Game Options can turn those three
    // on and set Tech Level and Bases; with nothing turned on the session
    // holds the forced values ([`super::skirmish_session_options`]).
    options.bridges_destroyable = true;
    options.set_session_options(state.session);

    Ok(SkirmishLaunchSession::from_rows(
        SkirmishLaunchMode::from_game_mode(selected_mode),
        Some(selected_map.file_name.clone()),
        vec![crate::skirmish_launch::SkirmishHumanSlot {
            raw_slot_index: Some(0),
            name: state.player_name.clone(),
            slot: local,
            role: crate::skirmish_launch::LaunchHumanRole::from_observer(false),
        }],
        0,
        opponents,
        options,
    ))
}
