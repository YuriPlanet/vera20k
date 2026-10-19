//! Host command588's ordered validation and pre-GO setup (005DCF00..5DD5DB).
//! GO completion is a later service/timeout event, never the Start click.

use super::*;
use crate::assets::csf_file::{CsfArg, format_csf};
use crate::skirmish_launch::{LaunchHumanRole, LaunchTeam};
use crate::skirmish_modes::ModeStartEffect;
use std::time::{Duration, Instant};

fn message(state: &mut AppState, key: &str, args: &[CsfArg]) {
    let text = format_csf(&App::csf_label(state, key, key), args);
    crate::app::shell_lan::append_chat(
        state,
        crate::ui::lan_shell::LanChatLine {
            text: text.encode_utf16().collect(),
            color: u32::MAX,
        },
    );
}

pub(super) fn request_start(state: &mut AppState) -> anyhow::Result<()> {
    anyhow::ensure!(
        state.match_state.session.host(),
        "only the host starts a LAN game"
    );
    anyhow::ensure!(
        state
            .frontend
            .lan_setup_protocol
            .as_ref()
            .is_some_and(|protocol| !protocol.locked),
        "setup is locked"
    );
    super::super::remember_setup(state);
    let launch = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("no setup"))?
        .clone();
    if launch
        .humans
        .iter()
        .filter(|human| !human.is_observer())
        .count()
        < 2
    {
        message(state, "TXT_ONLY_ONE", &[]);
        return Ok(());
    }
    let mut missing_map = false;
    for index in 0..launch.humans.len() {
        if index != launch.local_player_index
            && state
                .match_state
                .session
                .network_map_status(index)
                .unwrap_or(0)
                == 0
        {
            let map_name = super::super::map_display_name(state)
                .unwrap_or_default()
                .to_owned();
            message(
                state,
                "GUI:HostNoMap",
                &[
                    CsfArg::Str(&launch.humans[index].name),
                    CsfArg::Str(&map_name),
                ],
            );
            missing_map = true;
        }
    }
    if missing_map {
        return Ok(());
    }
    let capacity = state
        .match_state
        .session
        .network_setup_capacity()
        .unwrap_or(0);
    let players = launch
        .humans
        .iter()
        .filter(|human| !human.is_observer())
        .count() as i32
        + launch.ai_player_count;
    let capacity_ok = capacity >= players;
    if !capacity_ok {
        message(
            state,
            "TXT_SCENARIO_TOO_SMALL",
            &[CsfArg::Int(i64::from(capacity))],
        );
    }
    let mode = crate::skirmish_modes::mode_by_id(&state.frontend.skirmish_modes, launch.mode.id)
        .ok_or_else(|| anyhow::anyhow!("unknown mode"))?
        .clone();
    // The virtual14 callback still runs after a failed capacity check and may
    // mutate roles or the retained Cooperative partner record.
    let roles: Vec<_> = launch
        .humans
        .iter()
        .map(|human| human.role.native_value())
        .collect();
    let mode_ok = match mode
        .class
        .validate_setup_start(launch.options.bases, &roles)
    {
        Ok(effect) => {
            match effect {
                ModeStartEffect::None => {}
                ModeStartEffect::FreeForAllSourceRoles => {
                    for (index, human) in launch.humans.iter().enumerate() {
                        if !human.is_observer() {
                            state.match_state.session.accept_network_human_role(
                                index,
                                LaunchHumanRole::Player {
                                    group: index as i32,
                                },
                            )?;
                        }
                    }
                }
                ModeStartEffect::CooperativePartners => {
                    state
                        .frontend
                        .multiplayer_shell_runtime
                        .bind_network_cooperative_partners(&launch);
                }
            }
            true
        }
        Err(key) => {
            if let Some(key) = key {
                message(state, key, &[]);
            }
            false
        }
    };
    if !capacity_ok || !mode_ok {
        return Ok(());
    }
    let launch = state
        .match_state
        .session
        .network_setup()
        .expect("mode callback keeps setup");
    // 5DD0EA: only a nonobserver human in raw row0 seeds the comparison.
    // An observer there bypasses this native gate; do not invent all-pairs.
    if let Some(first) = launch
        .humans
        .iter()
        .find(|human| human.raw_slot_index == Some(0) && !human.is_observer())
    {
        if let LaunchTeam::Team(team) = first.slot.team {
            let same_humans = launch
                .humans
                .iter()
                .filter(|human| human.raw_slot_index.is_some() && !human.is_observer())
                .all(|human| human.slot.team == LaunchTeam::Team(team));
            let same_ai = launch
                .opponents
                .iter()
                .all(|ai| ai.team == LaunchTeam::Team(team));
            if same_humans && same_ai && !mode.class.permits_all_allied() {
                message(state, "TXT_CANNOT_ALLY", &[]);
                return Ok(());
            }
        }
    }
    if (0..launch.humans.len()).any(|index| !state.match_state.session.network_player_ready(index))
    {
        queue(state, LanSetupRequest::RequestAcceptance);
        message(state, "GUI:HostGameStartHost", &[]);
        return Ok(());
    }
    let launch = state
        .match_state
        .session
        .network_setup()
        .expect("validated setup")
        .clone();
    let max_ahead_frames = state
        .match_state
        .session
        .network_max_ahead_frames()
        .ok_or_else(|| anyhow::anyhow!("host start has no admitted LAN protocol timing"))?;
    // Host5DD3BD invokes mode+68 before the sole69B8C0 assignment owner.
    // The callback's declared-count write can survive a false result while
    // existing AI rows remain, so commit its two effects independently.
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
        .ok_or_else(|| anyhow::anyhow!("selected scenario is unavailable"))?;
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
        .expect("mode callback retains setup")
        .clone();
    let resolved = state
        .frontend
        .multiplayer_shell_runtime
        .resolve_network_assignments(&launch)?;
    state
        .match_state
        .session
        .accept_network_assignments(resolved)?;
    let agreed_seed = state
        .match_state
        .session
        .network_start_seed()
        .ok_or_else(|| anyhow::anyhow!("host setup has no proposed seed"))?;
    queue(state, LanSetupRequest::PublishOptions);
    queue(state, LanSetupRequest::PublishRoster);
    // Native mode+64 follows the resolved options/name publication, before
    // preview disposal and GO. Only Cooperative consumes Scenario RNG here.
    let launch = state
        .match_state
        .session
        .network_setup()
        .expect("resolved setup");
    state
        .frontend
        .multiplayer_shell_runtime
        .complete_network_mode_assignments(launch)?;
    if launch.mode.class.uses_cooperative_setup() {
        let rounds = state
            .frontend
            .multiplayer_shell_runtime
            .cooperative_round_assignments()
            .to_vec();
        queue(
            state,
            LanSetupRequest::PublishCooperativeAssignments(rounds),
        );
    }
    queue(
        state,
        LanSetupRequest::Go {
            agreed_seed,
            max_ahead_frames,
        },
    );
    if let Some(protocol) = state.frontend.lan_setup_protocol.as_mut() {
        protocol.locked = true;
        // Native host completes even after its600-bucket GO reply wait times
        // out. Real queue/transfer service completion is supplied separately.
        protocol.go_deadline = Some(Instant::now() + Duration::from_millis(600 * 16));
    }
    state.frontend.skirmish_preview_texture = None;
    Ok(())
}
