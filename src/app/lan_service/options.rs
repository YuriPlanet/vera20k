//! LAN5E3720/5DBB60 options and5E8EA0 roster adapters. The packet is a
//! projection: MatchSession retains participants, choices, map and readiness;
//! the existing HostSetup service performs mode/map admission and its effects.

use std::net::SocketAddrV4;

use anyhow::{Result, anyhow, ensure};

use crate::app::AppState;
use crate::app::match_runtime::session::{
    MatchSession, NetworkAiOptions, NetworkMapMetadata, NetworkRosterEntry, NetworkVacancy,
};
use crate::app::shell_network_setup::{HostOptions, NetworkSetupEvent};
use crate::net::lan_messages::setup::{self, LanGameOptions, LanOptionsAi, LanOptionsHuman};
use crate::net::lan_messages::{LanMessage, game_name_hash};
use crate::rules::ini_value::{crt_atoi, strtok};
use crate::skirmish_launch::{
    AiDifficulty, LaunchCountry, LaunchStartPosition, LaunchTeam, SkirmishAiSlot,
    SkirmishLaunchMode, SkirmishLocalSlot,
};
use crate::util::native_string::{narrow_utf16_c_string, narrow_widened, widen_bytes};

pub(super) fn encode_options(state: &AppState) -> Result<LanMessage> {
    let session = &state.match_state.session;
    ensure!(session.host(), "only the host publishes full LAN options");
    let options = projected_options(
        session,
        state
            .match_state
            .match_presentation
            .in_game_options
            .game_speed as i32,
        state
            .frontend
            .multiplayer_shell_runtime
            .cooperative_difficulty(),
    )?;
    let game = super::active_game_name(state).ok_or_else(|| anyhow!("no active LAN game"))?;
    let local = session
        .network_setup()
        .expect("projected setup")
        .local_player();
    Ok(LanMessage::options(
        &local.name,
        &options.encode(),
        super::color_value(local) as u32,
        game_name_hash(&game),
    )?)
}

pub(super) fn receive_options(
    state: &mut AppState,
    source: SocketAddrV4,
    message: &LanMessage,
) -> Result<()> {
    validate_host(state, source, message)?;
    let fields = message
        .options_fields()
        .ok_or_else(|| anyhow!("not a LAN options packet"))?;
    let game = super::active_game_name(state).ok_or_else(|| anyhow!("no active LAN game"))?;
    ensure!(
        fields.game_name_hash == game_name_hash(&game),
        "different LAN game"
    );
    let (options, current_speed, ai_options) = parse_options(
        &state.match_state.session,
        &state.frontend.skirmish_modes,
        fields.text,
    )?;
    crate::app::shell_network_setup::service_event(state, NetworkSetupEvent::HostSetup(options))?;
    state
        .match_state
        .session
        .accept_network_inactive_ai_options(ai_options)?;
    //5E4DE1 writes OptionsClass's applied speed independently of5E4E01's
    // selected setup speed. PregameSetup5E74AA copies the latter at launch;
    // the existing world-install projection then takes over this value.
    state
        .match_state
        .match_presentation
        .in_game_options
        .game_speed = current_speed as u32;
    Ok(())
}

pub(super) fn encode_roster(state: &AppState) -> Result<LanMessage> {
    let session = &state.match_state.session;
    ensure!(session.host(), "only the host publishes a LAN roster");
    let names = projected_roster(session)?;
    let names = std::array::from_fn(|index| names[index].as_str());
    Ok(LanMessage::roster(&super::local_name(state), &names)?)
}

pub(super) fn receive_roster(
    state: &mut AppState,
    source: SocketAddrV4,
    message: &LanMessage,
) -> Result<()> {
    validate_host(state, source, message)?;
    let names = message
        .roster_names()
        .ok_or_else(|| anyhow!("not a LAN roster packet"))?;
    //5E8EA3's empty input returns before clearing the old row bindings.
    if names.is_empty() {
        return Ok(());
    }
    let rows = parsed_roster(&state.match_state.session, &names)?;
    state.match_state.session.accept_network_roster(rows)?;
    state.platform.window.request_redraw();
    Ok(())
}

fn validate_host(state: &AppState, source: SocketAddrV4, message: &LanMessage) -> Result<()> {
    let host = state
        .match_state
        .lan_service
        .as_ref()
        .and_then(|service| service.admitted_host.as_ref())
        .ok_or_else(|| anyhow!("no admitted LAN host"))?;
    validate_host_endpoint(&state.match_state.session, host, source, &message.name())
}

fn validate_host_endpoint(
    session: &MatchSession,
    host: &(String, SocketAddrV4),
    source: SocketAddrV4,
    name: &str,
) -> Result<()> {
    ensure!(
        !session.host(),
        "host does not receive guest-authored setup"
    );
    ensure!(
        source == host.1 && name.eq_ignore_ascii_case(&host.0),
        "setup is not from the admitted LAN host"
    );
    if let Some(index) = session.network_endpoint_source(source) {
        ensure!(
            session.network_setup().expect("bound participant").humans[index]
                .name
                .eq_ignore_ascii_case(&host.0),
            "host endpoint belongs to another participant"
        );
    }
    Ok(())
}

fn projected_options(
    session: &MatchSession,
    current_speed: i32,
    cooperative_difficulty: i32,
) -> Result<LanGameOptions> {
    let launch = session
        .network_setup()
        .ok_or_else(|| anyhow!("no admitted setup"))?;
    let metadata = session
        .network_map_metadata()
        .ok_or_else(|| anyhow!("no selected map metadata"))?;
    let options = &launch.options;
    let humans = launch
        .humans
        .iter()
        .map(|human| LanOptionsHuman {
            name: human.name.clone(),
            country: super::country_value(human),
            color: super::color_value(human),
            start: start_value(human.slot.start_position),
            team: team_value(human.slot.team),
        })
        .collect();
    let ai = std::array::from_fn(|row| {
        let raw = session
            .network_ai_options(row as u8)
            .expect("eight setup rows");
        LanOptionsAi {
            difficulty: raw.difficulty,
            country: raw.country,
            color: raw.color,
            start: raw.start,
            team: raw.team,
        }
    });
    Ok(LanGameOptions {
        seed: session
            .network_start_seed()
            .ok_or_else(|| anyhow!("host seed is not initialized"))?,
        current_speed,
        game_speed: options.game_speed,
        credits: options.starting_credits,
        unit_count: options.unit_count,
        short_game: options.short_game,
        superweapons: options.super_weapons,
        build_off_ally: options.build_off_ally,
        mcv_redeploy: options.mcv_redeploy,
        crates: options.crates,
        mode_index: launch.mode.id,
        cooperative_difficulty,
        map_description: metadata.display_name.clone(),
        map_player_capacity: metadata.player_capacity,
        official_map: metadata.official,
        map_file_size: metadata.byte_length,
        scenario_filename: launch
            .selected_map_file
            .clone()
            .ok_or_else(|| anyhow!("no selected map filename"))?,
        map_digest: metadata.digest.clone(),
        humans,
        ai,
    })
}

///5E4DA6 uses comma-only strtok through the filename, colon-only for its
/// digest, then comma/colon for names and arrays. Widening bytes (not UTF-8)
/// preserves the packed map title for7B83E0 and the native narrow names.
fn parse_options(
    session: &MatchSession,
    modes: &[crate::skirmish_modes::SkirmishGameMode],
    bytes: &[u8],
) -> Result<(HostOptions, i32, [NetworkAiOptions; 8])> {
    let text = widen_bytes(bytes);
    let (header, tail) = text
        .split_once(':')
        .ok_or_else(|| anyhow!("options have no participant section"))?;
    let mut fields = strtok(header, &[',']);
    let mut next = || {
        fields
            .next()
            .ok_or_else(|| anyhow!("truncated LAN options header"))
    };
    let seed = crt_atoi(next()?) as u32;
    let current_speed = crt_atoi(next()?);
    let mut launch = session
        .network_setup()
        .ok_or_else(|| anyhow!("no admitted setup"))?
        .clone();
    launch.options.game_speed = crt_atoi(next()?);
    launch.options.starting_credits = crt_atoi(next()?);
    launch.options.unit_count = crt_atoi(next()?);
    launch.options.short_game = crt_atoi(next()?) != 0;
    launch.options.super_weapons = crt_atoi(next()?) != 0;
    launch.options.build_off_ally = crt_atoi(next()?) != 0;
    launch.options.mcv_redeploy = crt_atoi(next()?) != 0;
    launch.options.crates = crt_atoi(next()?) != 0;
    let mode_id = crt_atoi(next()?);
    let mode = crate::skirmish_modes::mode_by_id(modes, mode_id)
        .ok_or_else(|| anyhow!("unknown LAN game mode {mode_id}"))?;
    launch.mode = SkirmishLaunchMode::from_game_mode(mode);
    let cooperative_difficulty = crt_atoi(next()?);
    let description = setup::unpack_map_description(&narrow_widened(next()?));
    let map_metadata = NetworkMapMetadata {
        display_name: String::from_utf16_lossy(&description),
        player_capacity: crt_atoi(next()?),
        official: crt_atoi(next()?) != 0,
        byte_length: crt_atoi(next()?),
        digest: String::new(),
    };
    launch.selected_map_file = Some(next()?.to_owned());
    let digest = next()?.to_owned();
    ensure!(fields.next().is_none(), "extra LAN options header fields");
    let map_metadata = NetworkMapMetadata {
        digest,
        ..map_metadata
    };
    let mut fields = strtok(tail, &[',', ':']);
    loop {
        let name = fields
            .next()
            .ok_or_else(|| anyhow!("options have no AI sentinel"))?;
        if name == "@" {
            break;
        }
        let mut next_int = || {
            fields
                .next()
                .map(crt_atoi)
                .ok_or_else(|| anyhow!("truncated human options"))
        };
        let country = next_int()?;
        let color = next_int()?;
        let start = next_int()?;
        let team = next_int()?;
        //5E58FD..5E5A45 consumes three network-address fields; LAN does not
        //use them. Endpoint authority remains the received type3 envelope.
        for _ in 0..3 {
            fields
                .next()
                .ok_or_else(|| anyhow!("truncated human address fields"))?;
        }
        if let Some(human) = launch.humans.iter_mut().find(|human| {
            widen_bytes(&narrow_utf16_c_string(human.name.encode_utf16()))
                .eq_ignore_ascii_case(name)
        }) {
            human.slot = decoded_choices(&human.slot, country, color, start, team)?;
            human.set_observer(country == -3);
        }
    }
    let mut opponents = Vec::new();
    let mut ai_options = [NetworkAiOptions {
        difficulty: 0,
        country: 0,
        color: 0,
        start: 0,
        team: 0,
    }; 8];
    for row in 0..8u8 {
        let mut next_int = || {
            fields
                .next()
                .map(crt_atoi)
                .ok_or_else(|| anyhow!("truncated AI options"))
        };
        let difficulty = next_int()?;
        let country = next_int()?;
        let color = next_int()?;
        let start = next_int()?;
        let team = next_int()?;
        ai_options[usize::from(row)] = NetworkAiOptions {
            difficulty,
            country,
            color,
            start,
            team,
        };
        if country == -1 {
            continue;
        }
        let old = session.network_row_choices(row).expect("eight setup rows");
        let choices = decoded_choices(&old, country, color, start, team)?;
        opponents.push(SkirmishAiSlot {
            raw_slot_index: row,
            country: choices.country,
            country_random: choices.country_random,
            color_index: choices.color_index,
            color_random: choices.color_random,
            start_position: choices.start_position,
            team: choices.team,
            difficulty: decoded_difficulty(difficulty)?,
        });
    }
    ensure!(fields.next().is_none(), "extra LAN AI fields");
    launch.ai_player_count = opponents.len() as i32;
    launch.opponents = opponents;
    launch.rebuild_house_roster();
    let vacancies = std::array::from_fn(|row| {
        session
            .network_vacancy(row as u8)
            .unwrap_or(NetworkVacancy::Open)
    });
    Ok((
        HostOptions {
            launch,
            map_metadata,
            vacancies,
            observer_row: session.network_observer_row(),
            cooperative_difficulty,
            seed: Some(seed),
        },
        current_speed,
        ai_options,
    ))
}

fn decoded_choices(
    old: &SkirmishLocalSlot,
    country: i32,
    color: i32,
    start: i32,
    team: i32,
) -> Result<SkirmishLocalSlot> {
    let mut slot = old.clone();
    ensure!(
        country == -3 || country == -2 || (0..=i32::from(u16::MAX)).contains(&country),
        "country outside native registry domain"
    );
    slot.country_random = country == -2;
    if country >= 0 {
        slot.country = LaunchCountry::from_country_index(country as u32);
    } else if country == -2 {
        //696F99 clears the concrete country when SetCountry receives Random.
        //The AI-array parser5E5BAB..5E5C3C likewise replaces its word with -2; there
        //is no previous display-row country to retain in that representation.
        slot.country = LaunchCountry::America;
    }
    ensure!(
        color == -2 || (0..=i32::from(u8::MAX)).contains(&color),
        "color outside native slot domain"
    );
    slot.color_random = color == -2;
    if color >= 0 {
        slot.color_index = color as u8;
    } else {
        //696FC9 sets the concrete color to -1 for Random. The Rust slot
        //represents that sentinel with color_random; its unused u8 payload
        //has canonical value0, independent of whichever row was displayed.
        slot.color_index = 0;
    }
    slot.start_position = match start {
        -2 | -1 => LaunchStartPosition::Auto,
        0..=7 => LaunchStartPosition::Position(start as u8),
        _ => anyhow::bail!("start outside native waypoint domain"),
    };
    slot.team = match team {
        -2 | -1 => LaunchTeam::None,
        0..=255 => LaunchTeam::Team(team as u8),
        _ => anyhow::bail!("team outside native slot domain"),
    };
    Ok(slot)
}

fn start_value(start: LaunchStartPosition) -> i32 {
    match start {
        LaunchStartPosition::Auto => -2,
        LaunchStartPosition::Position(index) => i32::from(index),
    }
}
fn team_value(team: LaunchTeam) -> i32 {
    match team {
        LaunchTeam::None => -2,
        LaunchTeam::Team(index) => i32::from(index),
    }
}
fn decoded_difficulty(value: i32) -> Result<AiDifficulty> {
    match value {
        0 => Ok(AiDifficulty::Hard),
        1 => Ok(AiDifficulty::Normal),
        2 => Ok(AiDifficulty::Easy),
        _ => anyhow::bail!("invalid native AI difficulty"),
    }
}

fn projected_roster(session: &MatchSession) -> Result<[String; 8]> {
    let launch = session
        .network_setup()
        .ok_or_else(|| anyhow!("no admitted setup"))?;
    Ok(std::array::from_fn(|row| {
        if let Some(human) = launch
            .humans
            .iter()
            .find(|human| human.raw_slot_index == Some(row as u8))
        {
            human.name.clone()
        } else if let Some(ai) = session.network_display_ai(row as u8) {
            match ai {
                AiDifficulty::Easy => setup::EASY_AI,
                AiDifficulty::Normal => setup::MEDIUM_AI,
                AiDifficulty::Hard => setup::HARD_AI,
            }
            .to_owned()
        } else {
            match session
                .network_vacancy(row as u8)
                .unwrap_or(NetworkVacancy::Closed)
            {
                NetworkVacancy::Open => setup::OPEN,
                NetworkVacancy::Closed => setup::CLOSED,
                NetworkVacancy::OpenObserver => setup::OPEN_OBSERVER,
                NetworkVacancy::Waiting => setup::WAITING,
            }
            .to_owned()
        }
    }))
}

fn parsed_roster(session: &MatchSession, names: &[String]) -> Result<[NetworkRosterEntry; 8]> {
    ensure!(names.len() <= 8, "LAN roster exceeds eight rows");
    let launch = session
        .network_setup()
        .ok_or_else(|| anyhow!("no admitted setup"))?;
    let mut rows = [NetworkRosterEntry::Vacancy(NetworkVacancy::Open); 8];
    for (row, name) in names.iter().enumerate() {
        rows[row] = match name.as_str() {
            setup::OPEN => NetworkRosterEntry::Vacancy(NetworkVacancy::Open),
            setup::CLOSED => NetworkRosterEntry::Vacancy(NetworkVacancy::Closed),
            setup::OPEN_OBSERVER => NetworkRosterEntry::Vacancy(NetworkVacancy::OpenObserver),
            setup::EASY_AI => NetworkRosterEntry::Ai(AiDifficulty::Easy),
            setup::MEDIUM_AI => NetworkRosterEntry::Ai(AiDifficulty::Normal),
            setup::HARD_AI => NetworkRosterEntry::Ai(AiDifficulty::Hard),
            _ => launch
                .humans
                .iter()
                .position(|human| human.name == *name)
                .map_or(
                    NetworkRosterEntry::Vacancy(NetworkVacancy::Waiting),
                    NetworkRosterEntry::Human,
                ),
        };
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::match_runtime::session::{NetworkReturnContext, NetworkRowKind};
    use crate::skirmish_launch::SkirmishLaunchSession;
    use std::net::Ipv4Addr;

    fn fixture(host: bool) -> MatchSession {
        let profile: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.command-bar.example.json",
        ))
        .unwrap();
        let mut launch: SkirmishLaunchSession =
            serde_json::from_value(profile["launch"].clone()).unwrap();
        launch.opponents.clear();
        launch.ai_player_count = 0;
        launch.mode =
            SkirmishLaunchMode::from_game_mode(&crate::skirmish_modes::stock_skirmish_modes()[0]);
        launch.selected_map_file = Some("test.map".into());
        launch.humans.truncate(1);
        launch.humans[0].name = "Host".into();
        launch.humans[0].raw_slot_index = Some(0);
        launch.humans[0].slot.country_random = false;
        launch.humans[0].slot.country = LaunchCountry::America;
        launch.humans[0].slot.color_random = false;
        launch.humans[0].slot.color_index = 0;
        let mut guest = launch.humans[0].clone();
        guest.name = "Guest".into();
        guest.raw_slot_index = Some(1);
        guest.slot.color_index = 2;
        launch.humans.push(guest);
        launch.local_player_index = usize::from(!host);
        launch.options.game_speed = 4;
        launch.options.starting_credits = 9000;
        launch.rebuild_house_roster();
        let mut session = MatchSession::default();
        session.enter_network(NetworkReturnContext::LanLobby);
        session.accept_network_setup(launch, host, Some(7)).unwrap();
        session.set_network_start_seed(0xf1234567).unwrap();
        session
            .accept_network_map_metadata(
                NetworkMapMetadata {
                    display_name: "Høyde地".into(),
                    player_capacity: 8,
                    official: true,
                    byte_length: 123456,
                    digest: "0123456789abcdef0123456789abcdef".into(),
                },
                None,
                true,
            )
            .unwrap();
        session
    }

    #[test]
    fn full_options_preserve_native_text_and_apply_to_existing_participants() {
        let mut host = fixture(true);
        host.set_network_row_kind(3, NetworkRowKind::Ai(AiDifficulty::Easy))
            .unwrap();
        let mut choices = host.network_row_choices(3).unwrap();
        choices.country = LaunchCountry::Yuri;
        choices.country_random = false;
        choices.color_index = 5;
        choices.color_random = false;
        choices.start_position = LaunchStartPosition::Position(6);
        choices.team = LaunchTeam::Team(2);
        host.set_network_row_choices(3, choices).unwrap();
        let wire = projected_options(&host, 2, 1).unwrap();
        let packet =
            LanMessage::options("Host", &wire.encode(), 0, game_name_hash("Host")).unwrap();
        let mut guest = fixture(false);
        guest.set_network_player_ready(0, true).unwrap();
        let old_names: Vec<_> = guest
            .network_setup()
            .unwrap()
            .humans
            .iter()
            .map(|h| h.name.clone())
            .collect();
        let (parsed, current_speed, ai_options) = parse_options(
            &guest,
            &crate::skirmish_modes::stock_skirmish_modes(),
            packet.options_fields().unwrap().text,
        )
        .unwrap();
        assert_eq!(current_speed, 2);
        assert_eq!(
            ai_options[0].start, -2,
            "human row choices do not replace AI arrays"
        );
        assert_eq!(parsed.launch.options.game_speed, 4);
        assert_eq!(parsed.map_metadata.display_name, "Høyde地");
        assert_eq!(parsed.map_metadata.byte_length, 123456);
        assert_eq!(parsed.seed, Some(0xf1234567));
        assert_eq!(parsed.launch.opponents[0].raw_slot_index, 3);
        assert_eq!(parsed.launch.opponents[0].country, LaunchCountry::Yuri);
        assert_eq!(parsed.launch.opponents[0].difficulty, AiDifficulty::Easy);
        assert_eq!(
            parsed.launch.opponents[0].start_position,
            LaunchStartPosition::Position(6)
        );
        guest
            .accept_network_host_setup(
                parsed.launch,
                parsed.vacancies,
                parsed.map_metadata.player_capacity,
                parsed.observer_row,
                false,
            )
            .unwrap();
        assert_eq!(
            guest
                .network_setup()
                .unwrap()
                .humans
                .iter()
                .map(|h| h.name.clone())
                .collect::<Vec<_>>(),
            old_names
        );
        assert!(
            guest.network_player_ready(0),
            "full setup does not replace source acknowledgements"
        );
    }

    #[test]
    fn options_use_crt_tokens_and_reject_truncated_input_without_mutation() {
        let host = fixture(true);
        let wire = projected_options(&host, 3, 2).unwrap().encode();
        let text = widen_bytes(&wire).replacen("9000,", "9000credits,,", 1);
        let guest = fixture(false);
        let before = guest.clone();
        let (parsed, _, _) = parse_options(
            &guest,
            &crate::skirmish_modes::stock_skirmish_modes(),
            &narrow_widened(&text),
        )
        .unwrap();
        assert_eq!(parsed.launch.options.starting_credits, 9000);
        assert_eq!(parsed.seed, Some(0xf1234567));
        assert!(
            parse_options(
                &guest,
                &crate::skirmish_modes::stock_skirmish_modes(),
                b"1,2,3"
            )
            .is_err()
        );
        assert_eq!(guest, before);
    }

    #[test]
    fn roster_rebinds_rows_without_creating_nodes_and_late_type3_fills_waiting() {
        let mut guest = fixture(false);
        let host_address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1234);
        guest.bind_network_endpoint("Host", host_address).unwrap();
        guest.set_network_player_ready(0, true).unwrap();
        let names = [
            "Guest",
            setup::OPEN,
            "Late",
            setup::HARD_AI,
            "Host",
            setup::CLOSED,
            setup::OPEN,
            setup::OPEN_OBSERVER,
        ]
        .map(str::to_owned);
        guest
            .accept_network_roster(parsed_roster(&guest, &names).unwrap())
            .unwrap();
        let launch = guest.network_setup().unwrap();
        assert_eq!(launch.humans.len(), 2);
        assert_eq!(launch.humans[0].name, "Host");
        assert_eq!(launch.humans[0].raw_slot_index, Some(4));
        assert_eq!(launch.local_player_index, 1);
        assert_eq!(launch.local_player().raw_slot_index, Some(0));
        assert!(guest.network_player_ready(0));
        assert_eq!(guest.network_endpoint(0), Some(host_address));
        assert_eq!(guest.network_vacancy(2), Some(NetworkVacancy::Waiting));
        assert_eq!(projected_roster(&guest).unwrap()[2], setup::WAITING);
        let mut late = launch.humans[0].clone();
        late.name = "Late".into();
        late.raw_slot_index = Some(2);
        guest.add_network_participant(late).unwrap();
        assert_eq!(guest.network_setup().unwrap().humans.len(), 3);
        assert_eq!(guest.network_vacancy(2), None);
        assert_eq!(projected_roster(&guest).unwrap()[2], "Late");
    }

    #[test]
    fn duplicate_roster_is_atomic_and_host_endpoint_must_match() {
        let mut guest = fixture(false);
        let before = guest.clone();
        let names = ["Guest", "Host", "Host"].map(str::to_owned);
        assert!(
            guest
                .accept_network_roster(parsed_roster(&guest, &names).unwrap())
                .is_err()
        );
        assert_eq!(guest, before);
        let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1234);
        let host = ("Host".into(), address);
        assert!(validate_host_endpoint(&guest, &host, address, "Host").is_ok());
        assert!(
            validate_host_endpoint(
                &guest,
                &host,
                SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1236),
                "Host"
            )
            .is_err()
        );
        assert!(validate_host_endpoint(&guest, &host, address, "Guest").is_err());
        assert!(validate_host_endpoint(&fixture(true), &host, address, "Host").is_err());
    }

    #[test]
    fn join_and_peer_changes_share_human_and_ai_color_occupancy() {
        let mut host = fixture(true);
        assert!(host.network_color_in_use(2));
        assert!(!host.network_color_in_use(5));
        host.set_network_row_kind(3, NetworkRowKind::Ai(AiDifficulty::Hard))
            .unwrap();
        let mut choices = host.network_row_choices(3).unwrap();
        choices.color_index = 5;
        choices.color_random = false;
        host.set_network_row_choices(3, choices).unwrap();
        assert!(host.network_color_in_use(5));
    }

    #[test]
    fn inactive_array_words_survive_options_separately_from_human_choices() {
        let mut host = fixture(true);
        host.set_network_row_kind(3, NetworkRowKind::Ai(AiDifficulty::Easy))
            .unwrap();
        host.set_network_row_kind(3, NetworkRowKind::Open).unwrap();
        let mut human_choices = host.network_setup().unwrap().local_slot().clone();
        human_choices.start_position = LaunchStartPosition::Position(5);
        human_choices.team = LaunchTeam::Team(2);
        host.set_network_row_choices(0, human_choices).unwrap();
        let wire = projected_options(&host, 3, 0).unwrap();
        assert_eq!(wire.humans[0].start, 5);
        assert_eq!(wire.ai[0].start, -2);
        assert_eq!(
            wire.ai[3],
            LanOptionsAi {
                difficulty: -1,
                country: -1,
                color: -1,
                start: -1,
                team: -2
            }
        );
        let mut guest = fixture(false);
        let (options, _, arrays) = parse_options(
            &guest,
            &crate::skirmish_modes::stock_skirmish_modes(),
            &wire.encode(),
        )
        .unwrap();
        guest
            .accept_network_host_setup(
                options.launch,
                options.vacancies,
                options.map_metadata.player_capacity,
                options.observer_row,
                false,
            )
            .unwrap();
        guest.accept_network_inactive_ai_options(arrays).unwrap();
        assert_eq!(
            guest.network_ai_options(3).unwrap(),
            NetworkAiOptions {
                difficulty: -1,
                country: -1,
                color: -1,
                start: -1,
                team: -2
            }
        );
        assert_eq!(
            guest.network_setup().unwrap().humans[0].slot.start_position,
            LaunchStartPosition::Position(5)
        );
    }

    #[test]
    fn reordered_roster_and_options_keep_display_and_ai_arrays_independent() {
        let mut host = fixture(true);
        let mut launch = host.network_setup().unwrap().clone();
        launch.humans[1].raw_slot_index = Some(2);
        host.accept_network_setup(launch, true, Some(7)).unwrap();
        host.set_network_row_kind(1, NetworkRowKind::Ai(AiDifficulty::Hard))
            .unwrap();
        let options = projected_options(&host, 3, 0).unwrap().encode();
        let roster = projected_roster(&host).unwrap();
        let mut final_states = Vec::new();
        for roster_first in [false, true] {
            let mut guest = fixture(false);
            let apply_roster = |guest: &mut MatchSession| {
                guest
                    .accept_network_roster(parsed_roster(guest, &roster).unwrap())
                    .unwrap();
            };
            if roster_first {
                apply_roster(&mut guest);
                assert_eq!(guest.network_display_ai(1), Some(AiDifficulty::Hard));
                assert!(
                    guest.network_setup().unwrap().opponents.is_empty(),
                    "roster does not write gameplay AI arrays"
                );
            }
            let (parsed, _, raw_ai) = parse_options(
                &guest,
                &crate::skirmish_modes::stock_skirmish_modes(),
                &options,
            )
            .unwrap();
            guest
                .accept_network_host_setup(
                    parsed.launch,
                    parsed.vacancies,
                    parsed.map_metadata.player_capacity,
                    parsed.observer_row,
                    false,
                )
                .unwrap();
            guest.accept_network_inactive_ai_options(raw_ai).unwrap();
            if !roster_first {
                assert_eq!(
                    guest.network_setup().unwrap().humans[1].raw_slot_index,
                    Some(1)
                );
                assert_eq!(
                    guest.network_setup().unwrap().opponents[0].raw_slot_index,
                    1,
                    "options may precede the new display bindings"
                );
                assert_eq!(guest.network_display_ai(1), None);
                apply_roster(&mut guest);
            }
            assert_eq!(
                guest.network_setup().unwrap().humans[1].raw_slot_index,
                Some(2)
            );
            let ai = &guest.network_setup().unwrap().opponents[0];
            assert!(ai.country_random && ai.color_random);
            assert_eq!(ai.country.raw_index(), 0);
            assert_eq!(ai.color_index, 0);
            final_states.push(guest);
        }
        assert_eq!(final_states[0], final_states[1]);
    }

    #[test]
    fn lan_peer_policy_keeps_direct_team_and_shared_collision_checks() {
        let mut lan = fixture(true);
        let mut own = lan.network_setup().unwrap().local_slot().clone();
        own.start_position = LaunchStartPosition::Position(3);
        lan.set_network_row_choices(0, own.clone()).unwrap();
        let mut request = lan.network_setup().unwrap().humans[1].slot.clone();
        request.color_index = own.color_index;
        request.color_random = false;
        request.start_position = own.start_position;
        request.team = LaunchTeam::Team(7);
        request.country = LaunchCountry::Yuri;
        request.country_random = false;
        let mut wol = lan.clone();
        let result = lan.accept_lan_peer_choices(1, request.clone()).unwrap();
        assert!(result.options);
        assert!(!result.roster);
        let slot = &lan.network_setup().unwrap().humans[1].slot;
        assert!(slot.color_random);
        assert_eq!(slot.start_position, LaunchStartPosition::Auto);
        assert_eq!(slot.team, LaunchTeam::Team(7));
        assert_eq!(slot.country, LaunchCountry::Yuri);
        wol.accept_network_peer_choices(1, request, false, Some(&[LaunchCountry::America]))
            .unwrap();
        let slot = &wol.network_setup().unwrap().humans[1].slot;
        assert_eq!(slot.team, LaunchTeam::None);
        assert_eq!(slot.country, LaunchCountry::America);
    }
}
