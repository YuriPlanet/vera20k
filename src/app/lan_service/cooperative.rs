//! The LAN Cooperative result dialog BC6 over the match's global route.
//!
//! BC6's loop (`0x0046FC00`) keeps servicing the global manager `0xA8E9C0`
//! that carried the match: its records go with ack to every other node
//! (`0x00541260`, nodes from 1), and it reads only its own kinds. Here the
//! private channels end with the match, but the socket keeps its LAN scope
//! and the admitted setup, so the round's records reach the same peers and
//! the next mission loads over the same admission. The round state belongs
//! to `MatchSession`'s `CooperativeRound` phase; this module only moves its
//! records.

use super::*;
use crate::app::match_runtime::session::{
    CooperativeRoundEvent, CooperativeRoundOptions, NetworkAiOptions, NetworkSendRequest,
};
use crate::net::lan_messages::cooperative::{
    CooperativeAiWords, CooperativeHumanWords, CooperativeOptionsRecord, CooperativeSignal,
};
use crate::skirmish_launch::{
    LaunchCountry, LaunchStartPosition, LaunchTeam, SkirmishAiSlot, SkirmishLaunchSession,
    SkirmishLocalSlot,
};

/// BC6's exit to the shell: its last records (Quit's `0x24`) go out first,
/// then the visit leaves the match scope as [`return_transport`] does.
/// RESIDUAL: the `0x24` gets one transmission here; native's global manager
/// keeps retrying it from the shell. A lost one leaves the partner on BC6
/// until it quits too.
pub(crate) fn release_transport(state: &mut AppState) {
    if state.match_state.session_mode() != SessionMode::Lan {
        return;
    }
    drain_match_requests(state);
    let now = super::super::match_runtime::frame_pacer::frame_bucket(
        super::super::match_runtime::frame_pacer::wall_clock_ms(),
    );
    service_transport(&state.platform, &mut state.match_state, now);
    if let Some(service) = state.match_state.lan_service.as_mut() {
        if let Some(transport) = service.transport.as_mut() {
            transport.connections_mut().reset();
            transport.clear_lan_scope();
        }
        service.clear_setup();
    }
}

/// One service pass while the round is open; false outside it.
pub(super) fn poll(state: &mut AppState, now: u32) -> bool {
    if state.match_state.live_network.is_some()
        || state.match_state.session.cooperative_round().is_none()
    {
        return false;
    }
    crate::app::cooperative_round::poll_start_deadline(state, now);
    if state.match_state.session.cooperative_round().is_none() {
        return false;
    }
    loop {
        let Some(incoming) = state
            .match_state
            .lan_service
            .as_mut()
            .and_then(|service| service.transport.as_mut()?.pop_global_received())
        else {
            break;
        };
        if let Err(error) = receive(state, incoming, now) {
            log::debug!("Ignored co-op round packet: {error:#}");
        }
        service_transport(&state.platform, &mut state.match_state, now);
        // Go or Quit closed the round: later packets (the next mission's
        // loading) belong to the ordinary service.
        if state.match_state.session.cooperative_round().is_none() {
            return false;
        }
    }
    drain_match_requests(state);
    true
}

/// The loop's dispatch (`0x0047009F`) for a packet from a round participant.
fn receive(state: &mut AppState, incoming: GlobalMessage, now: u32) -> anyhow::Result<()> {
    anyhow::ensure!(
        incoming.product_id == LAN_PRODUCT_ID,
        "different LAN product"
    );
    let source = state
        .match_state
        .session
        .network_endpoint_source(incoming.source)
        .filter(|source| {
            state
                .match_state
                .session
                .network_connected_source_indices()
                .contains(source)
        })
        .ok_or_else(|| anyhow::anyhow!("not a round participant"))?;
    let from_host = !state.match_state.session.host()
        && state
            .match_state
            .lan_service
            .as_ref()
            .and_then(|service| service.admitted_host.as_ref())
            .is_some_and(|(_, endpoint)| *endpoint == incoming.source);
    let event = if let Some(signal) = CooperativeSignal::decode(&incoming.payload) {
        match signal {
            CooperativeSignal::Ack => CooperativeRoundEvent::Ack { source },
            CooperativeSignal::Left => CooperativeRoundEvent::PeerLeft { source },
            CooperativeSignal::Waiting => CooperativeRoundEvent::PeerWaiting { source },
            CooperativeSignal::Ready => CooperativeRoundEvent::PeerReady { source },
            CooperativeSignal::Go if from_host => CooperativeRoundEvent::Go,
            CooperativeSignal::Begin if from_host => CooperativeRoundEvent::Begin { now },
            CooperativeSignal::Go | CooperativeSignal::Begin => anyhow::bail!("not the round host"),
        }
    } else {
        let message = LanMessage::decode(&incoming.payload)?;
        if message.kind() == LanMessageKind::Departure {
            let source = state
                .match_state
                .lan_service
                .as_ref()
                .and_then(|service| {
                    service
                        .loading_departure_source(
                            &state.match_state.session,
                            incoming.source,
                            &message,
                        )
                        .ok()
                        .flatten()
                })
                .ok_or_else(|| anyhow::anyhow!("not an admitted departure"))?;
            CooperativeRoundEvent::PeerLeft { source }
        } else if let Some(record) = CooperativeOptionsRecord::decode(&message) {
            anyhow::ensure!(from_host, "not the round host");
            let launch = state
                .match_state
                .session
                .network_setup()
                .ok_or_else(|| anyhow::anyhow!("no round roster"))?;
            CooperativeRoundEvent::Options {
                options: round_options(launch, &record)?,
                now,
            }
        } else if let Some(chat) = message.in_game_chat_fields() {
            let game = super::active_game_name(&state.match_state).unwrap_or_default();
            let Some(event) = chat_event(source, &chat, &game) else {
                return Ok(());
            };
            event
        } else {
            // Every other kind, the 20 s `0x0D` among them, falls through
            // to the loop's tail (`0x004702FD`).
            CooperativeRoundEvent::KeepAlive
        }
    };
    crate::app::cooperative_round::service_event(state, event)
}

/// `0x0C` (`0x004706E3`): only this game's hash posts a line, in the
/// packet's colour `+0x110`.
fn chat_event(
    source: usize,
    chat: &crate::net::lan_messages::ingame::InGameChat<'_>,
    game: &str,
) -> Option<CooperativeRoundEvent> {
    (chat.game_name_hash == crate::net::lan_messages::game_name_hash(game)).then(|| {
        CooperativeRoundEvent::Chat {
            source,
            text: chat.text(),
            color: chat.color,
        }
    })
}

/// A queued round record to every other node.
pub(super) fn send(
    frontend: &crate::app::frontend::state::FrontendState,
    match_state: &mut crate::app::match_runtime::state::MatchState,
    request: NetworkSendRequest,
) {
    if match_state.session_mode() != SessionMode::Lan {
        return;
    }
    if request == NetworkSendRequest::CooperativeAbort {
        let now = super::super::match_runtime::frame_pacer::wall_clock_bucket();
        if let Some(service) = match_state.lan_service.as_mut()
            && let Err(error) = service.queue_departure(&match_state.session, now)
        {
            log::warn!("Co-op handoff departure did not queue: {error:#}");
        }
        return;
    }
    let name = local_name(frontend, &match_state.session);
    let peers: Vec<_> = super::super::match_runtime::network_match::nodes(&match_state.session)
        .into_iter()
        .skip(1)
        .collect();
    let signal = match request {
        NetworkSendRequest::CooperativeReady => CooperativeSignal::Ready,
        NetworkSendRequest::CooperativeLeave => CooperativeSignal::Left,
        NetworkSendRequest::CooperativeBegin => CooperativeSignal::Begin,
        NetworkSendRequest::CooperativeAck => CooperativeSignal::Ack,
        NetworkSendRequest::CooperativeGo => CooperativeSignal::Go,
        NetworkSendRequest::CooperativeOptions(options) => {
            if let Ok(message) = options_record(&options).encode(&name) {
                send_sources(match_state, message.as_bytes(), peers);
            }
            return;
        }
        _ => return,
    };
    if let Ok(bytes) = signal.encode(&name) {
        send_sources(match_state, &bytes, peers);
    }
}

/// `0x0046D4A0`: each raw slot's human words, `-1` where no human is bound,
/// and the AI rows. The round keeps no inactive AI words; those rows go out
/// as a fresh setup's (country `-1`, which every reader skips).
fn options_record(options: &CooperativeRoundOptions) -> CooperativeOptionsRecord {
    let humans = std::array::from_fn(|row| {
        options
            .humans
            .iter()
            .find(|(slot, _)| usize::from(*slot) == row)
            .map_or(
                CooperativeHumanWords {
                    start: -1,
                    country: -1,
                    color: -1,
                },
                |(_, choices)| CooperativeHumanWords {
                    start: choices.start_position.shell_value(),
                    country: choices.country_shell_value(false),
                    color: choices.color_shell_value(),
                },
            )
    });
    let ai = std::array::from_fn(|row| {
        let words = options
            .opponents
            .iter()
            .find(|ai| usize::from(ai.raw_slot_index) == row)
            .map_or(NetworkAiOptions::INITIAL_INACTIVE, NetworkAiOptions::of);
        CooperativeAiWords {
            country: words.country,
            color: words.color,
            difficulty: words.difficulty,
            start: words.start,
            team: words.team,
        }
    });
    CooperativeOptionsRecord {
        difficulty: options.difficulty as u8,
        humans,
        ai,
    }
}

/// The receiver's view of a `0x32` (`0x0046D650` applies it): every bound
/// human row's words, and every AI row whose country is set. The record
/// carries no AI count, so the receiver's own stays.
fn round_options(
    launch: &SkirmishLaunchSession,
    record: &CooperativeOptionsRecord,
) -> anyhow::Result<CooperativeRoundOptions> {
    let mut humans = Vec::new();
    for human in &launch.humans {
        let Some(row) = human.raw_slot_index else {
            continue;
        };
        let words = record.humans[usize::from(row)];
        if words.country == -1 {
            continue;
        }
        let choices = super::picks::decoded_pick(
            &human.slot,
            words.country,
            words.color,
            words.start,
            human.slot.team.shell_value(),
        )?;
        humans.push((row, choices));
    }
    let mut opponents = Vec::new();
    for (row, words) in record.ai.iter().enumerate() {
        if words.country == -1 {
            continue;
        }
        // Every field of this placeholder is overwritten: an AI row's
        // country is never the observer's -3.
        let placeholder = SkirmishLocalSlot {
            country: LaunchCountry::America,
            country_random: true,
            color_index: 0,
            color_random: true,
            start_position: LaunchStartPosition::Auto,
            team: LaunchTeam::None,
        };
        let choices = super::picks::decoded_pick(
            &placeholder,
            words.country,
            words.color,
            words.start,
            words.team,
        )?;
        opponents.push(SkirmishAiSlot {
            raw_slot_index: row as u8,
            country: choices.country,
            country_random: choices.country_random,
            color_index: choices.color_index,
            color_random: choices.color_random,
            start_position: choices.start_position,
            team: choices.team,
            difficulty: options::decoded_difficulty(words.difficulty)?,
        });
    }
    Ok(CooperativeRoundOptions {
        difficulty: i32::from(record.difficulty),
        humans,
        ai_player_count: launch.ai_player_count,
        opponents,
    })
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use crate::skirmish_launch::AiDifficulty;

    fn round_roster() -> SkirmishLaunchSession {
        let profile: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.command-bar.example.json",
        ))
        .unwrap();
        let mut launch: SkirmishLaunchSession =
            serde_json::from_value(profile["launch"].clone()).unwrap();
        let mut guest = launch.local_player().clone();
        guest.name = "Guest".into();
        guest.raw_slot_index = Some(1);
        launch.humans.push(guest);
        launch
    }

    /// BC6's partner line keeps the sender's `0xF0F0`; another game's
    /// `0x0C` posts nothing.
    #[test]
    fn chat_posts_this_games_lines_in_the_packet_colour() {
        let message = LanMessage::in_game_chat("Host", 0xF0F0, "Host's game", "gg").unwrap();
        let chat = message.in_game_chat_fields().unwrap();
        assert_eq!(
            chat_event(1, &chat, "Host's game"),
            Some(CooperativeRoundEvent::Chat {
                source: 1,
                text: "gg".into(),
                color: 0xF0F0,
            })
        );
        assert_eq!(chat_event(1, &chat, "Other game"), None);
    }

    /// The master's `0x32` reaches the guest as the same round options: the
    /// human rows by raw slot and the AI rows with their difficulty.
    #[test]
    fn options_cross_the_wire_unchanged() {
        let mut launch = round_roster();
        // 0x32 has no AI count: the receiver keeps its own.
        launch.ai_player_count = 1;
        let mut guest_choices = launch.humans[1].slot.clone();
        guest_choices.country_random = true;
        guest_choices.color_index = 2;
        guest_choices.color_random = false;
        guest_choices.start_position = LaunchStartPosition::Position(1);
        let options = CooperativeRoundOptions {
            difficulty: 2,
            humans: vec![(0, launch.humans[0].slot.clone()), (1, guest_choices)],
            ai_player_count: 1,
            opponents: vec![SkirmishAiSlot {
                raw_slot_index: 3,
                country: LaunchCountry::from_country_index(2),
                country_random: false,
                color_index: 5,
                color_random: false,
                start_position: LaunchStartPosition::Position(4),
                team: LaunchTeam::Team(1),
                difficulty: AiDifficulty::Hard,
            }],
        };
        let message = options_record(&options).encode("Host").unwrap();
        let record = CooperativeOptionsRecord::decode(&message).unwrap();
        let mut received = round_options(&launch, &record).unwrap();
        // A random country's placeholder is not on the wire.
        received.humans[1].1.country = options.humans[1].1.country;
        assert_eq!(received, options);
    }
}
