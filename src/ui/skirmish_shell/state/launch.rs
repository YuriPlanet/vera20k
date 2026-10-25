//! Launch session conversion for the skirmish shell.

use crate::map::scenario_menu::MapMenuEntry;
use crate::skirmish_launch::{
    HOUSE_COLOR_COUNT, LaunchStartPosition, LaunchTeam, LaunchValidationError, PreFillHouseRoster,
    SKIRMISH_PLAYER_SLOT_COUNT, SkirmishAiSlot, SkirmishLaunchMode, SkirmishLaunchSession,
    SkirmishLocalSlot,
};
use crate::skirmish_modes::{SkirmishGameMode, mode_by_id};

use super::SkirmishShellState;

fn launch_start_position(
    slot: usize,
    start_position: LaunchStartPosition,
) -> Result<LaunchStartPosition, LaunchValidationError> {
    match start_position {
        LaunchStartPosition::Auto => Ok(LaunchStartPosition::Auto),
        LaunchStartPosition::Position(position) if position < SKIRMISH_PLAYER_SLOT_COUNT as u8 => {
            Ok(LaunchStartPosition::Position(position))
        }
        LaunchStartPosition::Position(position) => {
            Err(LaunchValidationError::InvalidStartPosition { slot, position })
        }
    }
}

fn launch_color_index(slot: usize, color_index: usize) -> Result<u8, LaunchValidationError> {
    if color_index < HOUSE_COLOR_COUNT {
        Ok(color_index as u8)
    } else {
        Err(LaunchValidationError::InvalidColorIndex { slot, color_index })
    }
}

pub fn launch_session(
    state: &SkirmishShellState,
    maps: &[MapMenuEntry],
    modes: &[SkirmishGameMode],
    allow_solo: bool,
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
    // `[VERA] SoloSkirmish=yes` accepts a match with no opponent for an
    // undisturbed test game. Retail keeps its "at least two players" gate.
    if active_count == 0 && !allow_solo {
        return Err(LaunchValidationError::NoEnabledOpponent);
    }
    // With no opponent active there is no team conflict to report.
    if state.player_team >= 0 && active_count > 0 {
        let local_team = state.player_team as u8;
        let all_active_ai_same_team = state
            .opponents
            .iter()
            .filter(|opponent| opponent.is_active())
            .all(|opponent| {
                LaunchTeam::from_shell_value(opponent.team) == LaunchTeam::Team(local_team)
            });
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
        mode_by_id(modes, state.selected_mode_id).ok_or(LaunchValidationError::NoSelectedMode {
            mode_id: state.selected_mode_id,
        })?;

    let local = SkirmishLocalSlot {
        country: state.player_country,
        country_random: state.player_country_random,
        color_index: launch_color_index(0, state.player_color_index)?,
        color_random: !state.player_color_claimed,
        start_position: launch_start_position(0, state.player_start_position)?,
        team: LaunchTeam::from_shell_value(state.player_team),
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
            color_index: launch_color_index(slot, opponent.color_index)?,
            color_random: !opponent.color_claimed,
            start_position: launch_start_position(slot, opponent.start_position)?,
            team: LaunchTeam::from_shell_value(opponent.team),
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
    // forces four session bytes whatever the rules default: Fog of War
    // (`0x00A8B31F`) off, Bridge Destruction (`0x00A8B260`) on, Harvester
    // Truce (`0x00A8B31D`) off and MultiEngineer (`0x00A8B26C`) off. Only the
    // network setup dialogs offer the last three.
    options.fog_of_war = false;
    options.bridges_destroyable = true;
    options.harvester_truce = false;
    options.multi_engineer = false;

    let mut session = SkirmishLaunchSession {
        mode: SkirmishLaunchMode::from_game_mode(selected_mode),
        selected_map_file: Some(selected_map.file_name.clone()),
        humans: vec![crate::skirmish_launch::SkirmishHumanSlot {
            raw_slot_index: Some(0),
            name: state.player_name_edit.text.clone(),
            slot: local,
            role: crate::skirmish_launch::LaunchHumanRole::from_observer(false),
        }],
        local_player_index: 0,
        ai_player_count: opponents.len() as i32,
        opponents,
        pre_fill_house_roster: PreFillHouseRoster::from_compact_skirmish(0),
        options,
    };
    session.rebuild_house_roster();
    Ok(session)
}
