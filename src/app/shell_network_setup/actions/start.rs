//! Host command 588's ordered validation and commit: BC (005DCF00..5DD5DB)
//! ends in GO, C2 (0x7905D0..0x790BEB) in a StartGame request. GO and the
//! server's OnGameStart complete later, never the Start click.

use super::*;
use crate::assets::csf_file::CsfArg;
use crate::skirmish_modes::ModeStartEffect;

use super::super::chat_notice as message;

pub(super) fn request_start(state: &mut AppState) -> anyhow::Result<()> {
    let wol = super::super::dialog(state).wol();
    anyhow::ensure!(
        state.match_state.session.host(),
        "only the host starts a network game"
    );
    anyhow::ensure!(
        state
            .frontend
            .network_setup_protocol
            .as_ref()
            .is_some_and(|protocol| !protocol.locked),
        "setup is locked"
    );
    // C2 ignores Start while its StartGame request is pending (00B77DEC).
    if state
        .frontend
        .network_setup_protocol
        .as_ref()
        .is_some_and(|protocol| protocol.start_in_progress())
    {
        return Ok(());
    }
    super::super::remember_setup(state);
    let launch = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("no setup"))?
        .clone();
    let humans = launch
        .humans
        .iter()
        .filter(|human| !human.is_observer())
        .count() as i32;
    let watch_ai = state.match_state.session.network_several_observers();
    if !has_minimum_players(&launch, watch_ai) {
        message(state, "TXT_ONLY_ONE", &[]);
        return Ok(());
    }
    // C2 0x790698: WOL forbids AI players in a game with observers. VERA's
    // own room lets them play while people watch.
    if wol
        && !state.match_state.session.network_several_observers()
        && humans < launch.humans.len() as i32
        && launch.ai_player_count != 0
    {
        message(state, "TXT_NO_AI_OBSERVATION", &[]);
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
    let mode = crate::skirmish_modes::mode_by_id(&state.frontend.skirmish_modes, launch.mode.id)
        .ok_or_else(|| anyhow::anyhow!("unknown mode"))?
        .clone();
    let capacity = state
        .match_state
        .session
        .network_setup_capacity()
        .unwrap_or(0);
    // A room sharing bases seats a team's players on one start where the
    // map runs out (`teams::team_starts`), so its map needs one for each
    // team with players; otherwise one for each person and AI.
    let shares = state.match_state.session.network_shares_bases();
    let needed = |ai: i32| {
        if shares {
            super::super::teams::bases_needed(&launch)
        } else {
            humans + ai
        }
    };
    if wol {
        // C2 0x79071E..0x7908D5: allies, readiness, then capacity. VERA's
        // explicit AI rows share the map with human players, even though
        // only humans belong to the network/relay roster.
        if all_allied(state) {
            message(state, "TXT_CANNOT_ALLY", &[]);
            return Ok(());
        }
        if !all_ready(state) {
            return Ok(());
        }
        if capacity < needed(launch.opponents.len() as i32) {
            message(
                state,
                "TXT_SCENARIO_TOO_SMALL",
                &[CsfArg::Int(i64::from(capacity))],
            );
            return Ok(());
        }
        // VERA online/LAN clients use the shared thirty-player admission
        // owner; the original C2 six-user service limit does not apply.
        if launch.humans.len() > crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT {
            message(state, "TXT_TOO_MANY_PLAYERS", &[]);
            return Ok(());
        }
        if !mode_start(state, &mode)? {
            return Ok(());
        }
        // 0x79096A..0x790B18: a clan room starts only with two squads of
        // equal size; the observer row's node is not counted.
        if state.match_state.session.network_room().clan {
            let observer = state
                .match_state
                .session
                .network_observer_row()
                .and_then(|row| {
                    launch
                        .humans
                        .iter()
                        .find(|human| human.raw_slot_index == Some(row) && human.is_observer())
                })
                .map(|human| human.name.clone());
            if !crate::app::shell_wol::lobby::clan_teams_even(state, observer.as_deref()) {
                message(state, "TXT_CLAN_NOSTART", &[]);
                return Ok(());
            }
        }
    } else {
        let capacity_ok = capacity >= needed(launch.ai_player_count);
        if !capacity_ok {
            message(
                state,
                "TXT_SCENARIO_TOO_SMALL",
                &[CsfArg::Int(i64::from(capacity))],
            );
        }
        // The virtual14 callback still runs after a failed capacity check
        // and may mutate roles or the retained Cooperative partner record.
        let mode_ok = mode_start(state, &mode)?;
        if !capacity_ok || !mode_ok {
            return Ok(());
        }
        if all_allied(state) {
            message(state, "TXT_CANNOT_ALLY", &[]);
            return Ok(());
        }
        if !all_ready(state) {
            return Ok(());
        }
    }
    let launch = state
        .match_state
        .session
        .network_setup()
        .expect("validated setup")
        .clone();
    if !wol {
        crate::app::lan_service::prepare_host_timing(state)?;
    }
    let max_ahead_frames = if wol {
        // Start Game Now derives WOL timing after OnGameStart.
        None
    } else {
        Some(
            state
                .match_state
                .session
                .network_max_ahead_frames()
                .ok_or_else(|| anyhow::anyhow!("host start has no admitted LAN protocol timing"))?,
        )
    };
    // mode+68 before the sole 69B8C0 assignment owner (BC 5DD3BD, C2
    // 0x790B38). The callback's declared-count write can survive a false
    // result while existing AI rows remain, so commit its two effects
    // independently.
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
    // VERA's room with Teams set gives each player still on a Random start
    // one by team before the assignment's farthest free start would.
    super::super::teams::team_starts(state, true)?;
    let launch = state
        .match_state
        .session
        .network_setup()
        .expect("mode callback retains setup")
        .clone();
    let banned =
        crate::app::shell_network_setup::bans::banned_countries(state, launch.options.bans);
    let resolved = state
        .frontend
        .multiplayer_shell_runtime
        .resolve_network_assignments(&launch, &banned)?;
    state
        .match_state
        .session
        .accept_network_assignments(resolved)?;
    if wol {
        // C2: slot names 77DB90 before options 5E32D0(1,1).
        queue(state, NetworkSetupRequest::PublishRoster);
        queue(state, NetworkSetupRequest::PublishOptions);
    } else {
        queue(state, NetworkSetupRequest::PublishOptions);
        queue(state, NetworkSetupRequest::PublishRoster);
    }
    // Native mode+64 follows the resolved options/name publication, before
    // preview disposal and GO. Only Cooperative consumes Scenario RNG here.
    // C2 then calls mode+0x24 (0x790B62), a no-op except MPCooperative's
    // 5C1CB0 flag for its later 5C2680 save step: not ported.
    let launch = state
        .match_state
        .session
        .network_setup()
        .expect("resolved setup");
    state
        .frontend
        .multiplayer_shell_runtime
        .complete_network_mode_assignments(launch)?;
    let users: Vec<String> = launch
        .humans
        .iter()
        .map(|human| human.name.clone())
        .collect();
    if launch.mode.class.uses_cooperative_setup() {
        let rounds = state
            .frontend
            .multiplayer_shell_runtime
            .cooperative_round_assignments()
            .to_vec();
        queue(
            state,
            NetworkSetupRequest::PublishCooperativeAssignments(rounds),
        );
    }
    let Some(max_ahead_frames) = max_ahead_frames else {
        // C2 0x790BB9: API+0x48 StartGame with every channel user, then
        // 00B77E28/00B77DEC until OnGameStart answers.
        queue(state, NetworkSetupRequest::StartGame { users });
        if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
            protocol.wol_start_pending = true;
        }
        return Ok(());
    };
    let agreed_seed = state
        .match_state
        .session
        .network_start_seed()
        .ok_or_else(|| anyhow::anyhow!("host setup has no proposed seed"))?;
    let launch = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("no setup"))?;
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        queue_lan_go(protocol, launch, agreed_seed, max_ahead_frames);
        // Native's offer blocks the dialog until it ends, so Start cannot
        // follow a pending one; GO drops it.
        protocol.drop_preview_offer();
    }
    state.frontend.skirmish_preview_texture = None;
    Ok(())
}

fn queue_lan_go(
    protocol: &mut NetworkSetupProtocol,
    launch: &SkirmishLaunchSession,
    agreed_seed: u32,
    max_ahead_frames: i32,
) {
    let pending = (0..launch.humans.len())
        .filter(|source| *source != launch.local_player_index)
        .collect();
    protocol.requests.push(NetworkSetupRequest::Go {
        agreed_seed,
        max_ahead_frames,
    });
    protocol.locked = true;
    // The transport starts the native drain only after all GO packets
    // have entered its existing global send queue.
    protocol.lan_go = Some(super::super::LanGoWait::HostSending { pending });
}

/// A direct spawn host's start (docs/spawn.md): the launcher already
/// validated the roster, teams and map, so only BC's tail runs: LAN start
/// timing, then GO to every admitted guest. GO's replies, drain and
/// completion are the ordinary LAN owners'.
pub(crate) fn begin_external_lan_go(state: &mut AppState) -> anyhow::Result<()> {
    anyhow::ensure!(
        super::super::external(state) && !super::super::dialog(state).wol(),
        "no direct spawn setup"
    );
    anyhow::ensure!(
        state.match_state.session.host(),
        "only the direct host sends GO"
    );
    anyhow::ensure!(
        state
            .frontend
            .network_setup_protocol
            .as_ref()
            .is_some_and(|protocol| !protocol.locked),
        "setup is locked"
    );
    crate::app::lan_service::prepare_host_timing(state)?;
    let max_ahead_frames = state
        .match_state
        .session
        .network_max_ahead_frames()
        .ok_or_else(|| anyhow::anyhow!("host start has no admitted LAN protocol timing"))?;
    let agreed_seed = state
        .match_state
        .session
        .network_start_seed()
        .ok_or_else(|| anyhow::anyhow!("direct setup has no seed"))?;
    let launch = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("no setup"))?;
    log::info!(
        "Direct spawn: every guest answered; GO with MaxAhead {max_ahead_frames}, seed {agreed_seed}"
    );
    let protocol = state
        .frontend
        .network_setup_protocol
        .as_mut()
        .expect("checked setup");
    queue_lan_go(protocol, launch, agreed_seed, max_ahead_frames);
    Ok(())
}

/// VERA's shared LAN/online admission allows a human to play the ordinary
/// skirmish AI without creating another network participant. Observers and
/// a stale declared AI count cannot stand in for an actual opponent slot,
/// and players sharing a House count once. VERA's own room (`watch_ai`)
/// also starts with everyone watching while two or more AI play.
fn has_minimum_players(launch: &SkirmishLaunchSession, watch_ai: bool) -> bool {
    let houses = (0..launch.humans.len())
        .filter(|&index| !launch.humans[index].is_observer() && launch.house_leader(index) == index)
        .count();
    (houses >= 1 || watch_ai) && houses + launch.opponents.len() >= 2
}

fn all_allied(state: &AppState) -> bool {
    state
        .match_state
        .session
        .network_setup()
        .expect("validated setup")
        .all_allied()
}

/// Readiness (BC; C2 0x7907E0 from the WOL user records): an unready node
/// asks every peer to accept and prints the host line.
fn all_ready(state: &mut AppState) -> bool {
    let count = state
        .match_state
        .session
        .network_setup()
        .expect("validated setup")
        .humans
        .len();
    if (0..count).all(|index| state.match_state.session.network_player_ready(index)) {
        return true;
    }
    queue(state, NetworkSetupRequest::RequestAcceptance);
    message(state, "GUI:HostGameStartHost", &[]);
    false
}

/// mode+14 (BC and C2 0x79091D): its effect commits, an error key prints.
fn mode_start(
    state: &mut AppState,
    mode: &crate::skirmish_modes::SkirmishGameMode,
) -> anyhow::Result<bool> {
    let launch = state
        .match_state
        .session
        .network_setup()
        .expect("validated setup")
        .clone();
    let roles: Vec<_> = launch
        .humans
        .iter()
        .map(|human| human.role.native_value())
        .collect();
    match mode
        .class
        .validate_setup_start(launch.options.bases, &roles)
    {
        Ok(ModeStartEffect::None) => {}
        Ok(ModeStartEffect::FreeForAllSourceRoles) => {
            for (index, role) in launch.free_for_all_source_roles() {
                state
                    .match_state
                    .session
                    .accept_network_human_role(index, role)?;
            }
        }
        Ok(ModeStartEffect::CooperativePartners) => {
            state
                .frontend
                .multiplayer_shell_runtime
                .bind_network_cooperative_partners(&launch);
        }
        Err(key) => {
            if let Some(key) = key {
                message(state, key, &[]);
            }
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    fn fixture() -> SkirmishLaunchSession {
        let profile = crate::test_fixture::json("tools/map_observation.command-bar.example.json");
        serde_json::from_value(profile["launch"].clone()).unwrap()
    }

    #[test]
    fn one_human_can_start_against_an_actual_ai_slot() {
        let mut launch = fixture();
        assert_eq!(launch.humans.len(), 1);
        assert_eq!(launch.opponents.len(), 1);
        assert!(has_minimum_players(&launch, false));

        launch.opponents.clear();
        assert_eq!(launch.ai_player_count, 1);
        assert!(
            !has_minimum_players(&launch, false),
            "a declared count without an AI House is not an opponent"
        );
    }

    #[test]
    fn observers_do_not_replace_playing_humans_or_opponents() {
        let mut launch = fixture();
        launch.humans[0].set_observer(true);
        assert!(!has_minimum_players(&launch, false));
        // VERA's room starts with everyone watching two or more AI.
        assert!(!has_minimum_players(&launch, true));
        let mut second = launch.opponents[0].clone();
        second.raw_slot_index += 1;
        launch.opponents.push(second);
        assert!(!has_minimum_players(&launch, false));
        assert!(has_minimum_players(&launch, true));

        let mut player = fixture().local_player().clone();
        player.name = "Guest".into();
        player.raw_slot_index = Some(2);
        launch.humans.push(player);
        launch.opponents.clear();
        assert!(!has_minimum_players(&launch, false));

        // VERA shared control: two players on one start are one House.
        launch.humans[0].set_observer(false);
        assert!(!has_minimum_players(&launch, false));
    }
}
