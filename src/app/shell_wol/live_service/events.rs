//! Lobby events (`apply_event`): what each pushed event does to the visit,
//! the room and the match session.

use super::poll_maps;
use super::{
    Chat, LiveService, RoomAgreement, StartDataMismatch, WolServiceEvent, begin_start,
    cancel_pending, canonical_options, decode, decode_relay_grant, decode_watch_grant, end_room,
    enter_timed_start, fail_start, fail_watch, lobby_names, lobby_says_outdated, local_agreement,
    local_map_status, maps, notice, prepare_watch, relay_uuid, service_event, wire,
};
use crate::app::AppState;
use crate::app::shell_wol::DecodedRelayGrant;
use crate::i18n::tr;
use crate::util::time::Instant;
use anyhow::{Context, Result, anyhow, ensure};
use base64::Engine;
use serde::Deserialize;
use serde_json::Value;
use std::net::SocketAddrV4;

/// Whether a spawn.ini `[Relay] Ticket` is a watch ticket (docs/spawn.md
/// "Watching"): the relay protocol's own parser decides, not a version byte
/// copied here.
pub(crate) fn relay_ticket_is_watch(ticket: &str) -> bool {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(ticket)
        .is_ok_and(|bytes| crate::net::udp::RelayWatchTicket::parse(&bytes).is_ok())
}

/// A watcher spawn.ini's `[Relay]` grant, admitted through the lobby path's
/// own watch decoder: the ticket and certificate are `match_id`'s. A watch
/// ticket names no source and no player of the match.
pub(crate) fn decode_external_watch_grant(
    address: SocketAddrV4,
    ticket: String,
    packet_key: String,
    payload_key: String,
    ordering_certificate: String,
    match_id: &str,
) -> Result<DecodedRelayGrant> {
    decode_watch_grant(
        &wire::WatchGrant {
            address,
            ticket,
            packet_key,
            payload_key,
            ordering_certificate,
        },
        match_id,
    )
}

pub(super) fn apply_event(
    state: &mut AppState,
    service: &mut LiveService,
    kind: &str,
    payload: Value,
) -> Result<()> {
    // A launcher's watcher hears only its renewals (docs/spawn.md "Watching").
    if service.spawned_watch.is_some() && kind != "watch_renewed" {
        return Ok(());
    }
    let local = state.frontend.wol_account.nickname.clone();
    match kind {
        "relay_renewed" => {
            #[derive(Deserialize)]
            struct Renewed {
                match_id: String,
                relay: wire::RelayGrant,
            }
            let renewed: Renewed = decode(payload)?;
            // A late reply from the previous match cannot replace a rematch.
            if service.match_id.as_ref() != Some(&renewed.match_id) {
                return Ok(());
            }
            let grant = decode_relay_grant(&renewed.relay, &renewed.match_id, &service.player_id)?;
            crate::app::lan_service::renew_online_relay(
                state,
                renewed.relay.address,
                &grant.ticket,
                grant.key,
                grant.payload_key,
            )?;
            service.relay_granted(crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32);
        }
        "watch_started" => {
            let id = payload["match_id"]
                .as_str()
                .context("missing watch match")?
                .to_owned();
            relay_uuid(&id)?;
            let awaited = super::lobby::awaits_watch(state, &id);
            if let Err(error) = service.receive_watch_start(&id, awaited, || {
                let start = prepare_watch(state, decode(payload)?)?;
                if let Some(lobby) = state
                    .frontend
                    .wol
                    .as_mut()
                    .and_then(|visit| visit.lobby_mut())
                {
                    lobby.watch_granted(&id);
                }
                Ok(start)
            }) {
                log::warn!("Could not watch the match: {error:#}");
                fail_watch(state, service, "VERA:WatchCouldNotStart");
            }
        }
        "watch_renewed" => {
            #[derive(Deserialize)]
            struct Renewed {
                match_id: String,
                relay: wire::WatchGrant,
            }
            let renewed: Renewed = decode(payload)?;
            let watched = service
                .watching
                .as_ref()
                .map(|watching| &watching.match_id)
                .or(service.spawned_watch.as_ref());
            if watched != Some(&renewed.match_id) {
                return Ok(());
            }
            let grant = decode_watch_grant(&renewed.relay, &renewed.match_id)?;
            crate::app::lan_service::renew_online_relay(
                state,
                renewed.relay.address,
                &grant.ticket,
                grant.key,
                grant.payload_key,
            )?;
            service.relay_granted(crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32);
        }
        // The public lobby this player visits from a game room, whose state
        // stays as it is (docs/lobby-protocol.md "Visiting the lobby").
        "joined" if payload["visit"].as_bool() == Some(true) => {
            let joined: wire::Joined = decode(payload)?;
            let members = joined
                .members
                .into_iter()
                .map(|member| member.into_member(&local))
                .collect();
            service_event(
                state,
                WolServiceEvent::VisitJoined(super::lobby::WolJoined {
                    channel: joined.channel.into(),
                    members,
                }),
            );
        }
        "joined" | "created" | "room_reopened" => {
            let joined: wire::Joined = decode(payload)?;
            service.revision = 0;
            service.room = RoomAgreement::default();
            service.start_preparation = None;
            service.left_for_match = false;
            service.member_ids = joined
                .members
                .iter()
                .map(|m| (m.name.to_ascii_lowercase(), m.player_id.clone()))
                .collect();
            let members = joined
                .members
                .into_iter()
                .map(|member| member.into_member(&local))
                .collect();
            let joined = super::lobby::WolJoined {
                channel: joined.channel.into(),
                members,
            };
            let event = match kind {
                "created" => WolServiceEvent::Created(Ok(joined.members)),
                "room_reopened" => WolServiceEvent::RoomReopened(joined),
                _ => WolServiceEvent::Joined(Ok(joined)),
            };
            service_event(state, event);
        }
        "channel_left" => {
            if service.room_closed() {
                crate::app::loading::pump::cancel_network_loading(
                    state,
                    &tr("VERA:OnlineRoomClosedWhileLoading"),
                );
            }
            end_room(state, service);
            service.room = RoomAgreement::default();
            if state
                .frontend
                .wol
                .as_ref()
                .is_some_and(|visit| visit.in_game)
            {
                // The kept room closed during the match: nothing to return to.
                state.match_state.session.take_network_room_return();
            }
            service_event(state, WolServiceEvent::ChannelLeft);
        }
        "member_joined" => {
            #[derive(Deserialize)]
            struct Payload {
                member: wire::Member,
                #[serde(default)]
                visit: bool,
            }
            let p: Payload = decode(payload)?;
            if p.visit {
                service_event(
                    state,
                    WolServiceEvent::VisitUserJoined(p.member.into_member(&local)),
                );
                return Ok(());
            }
            service.member_ids.insert(
                p.member.name.to_ascii_lowercase(),
                p.member.player_id.clone(),
            );
            service_event(
                state,
                WolServiceEvent::UserJoined(p.member.into_member(&local)),
            );
        }
        // The room's host left and the lobby made another player its host
        // (docs/lobby-protocol.md "Host handover"): what this game agreed
        // with the old host is gone, and the room opens again.
        "host_changed" => {
            #[derive(Deserialize)]
            struct Payload {
                member: wire::Member,
                channel: Option<wire::Channel>,
            }
            let p: Payload = decode(payload)?;
            service.room = RoomAgreement::default();
            service.start_preparation = None;
            maps::cancel(state, service);
            service_event(
                state,
                WolServiceEvent::HostChanged {
                    name: p.member.name,
                    channel: p.channel.map(Into::into),
                },
            );
        }
        "member_left" => {
            let name = payload["name"]
                .as_str()
                .ok_or_else(|| anyhow!("missing member"))?
                .to_owned();
            if payload["visit"].as_bool() == Some(true) {
                service_event(state, WolServiceEvent::VisitUserLeft(name));
                return Ok(());
            }
            service.member_ids.remove(&name.to_ascii_lowercase());
            let required = state
                .match_state
                .session
                .network_setup()
                .is_some_and(|launch| {
                    launch.humans.iter().enumerate().any(|(source, human)| {
                        human.name.eq_ignore_ascii_case(&name)
                            && state
                                .match_state
                                .session
                                .network_connected_source_indices()
                                .contains(&source)
                    })
                });
            if required {
                crate::app::loading::pump::cancel_network_loading(
                    state,
                    &tr("VERA:OnlinePlayerLeftWhileLoading"),
                );
            }
            service_event(state, WolServiceEvent::UserLeft(name));
        }
        "options" => {
            let mut p: wire::OptionsEvent = decode(payload).inspect_err(|_| {
                log::warn!("Online host options could not be decoded");
            })?;
            if p.revision < service.revision {
                return Ok(());
            }
            let changed = p.revision != service.revision || service.room.accepted_options.is_none();
            let canonical = canonical_options(&p.options)?;
            if let Some(start) = service.start_preparation.as_mut()
                && !start.accept_options(p.revision, &canonical)
                && start.revision.is_some()
            {
                service.start_preparation = None;
                fail_start(state, &tr("VERA:SettingsChangedWhileStarting"));
            }
            service.revision = p.revision;
            if service
                .room
                .pending_choices
                .as_ref()
                .is_some_and(|choices| choices.answered_by(&canonical))
            {
                service.room.pending_choices = None;
            }
            service.room.accepted_options = Some(canonical);
            let authored = p.options.campaign.is_some();
            if changed {
                service.room.last_ready_request = None;
                crate::app::shell_network_setup::invalidate_network_agreement(state)?;
            }
            let generated = p.options.random_map.clone();
            if !state.match_state.session.host() {
                // VERA: Quick Match plays the original game, whatever rules
                // its host sends.
                if super::lobby::quick_match_runner(state) {
                    p.options.launch.options.host_rules = Default::default();
                }
                let names = p
                    .options
                    .launch
                    .humans
                    .iter()
                    .map(|human| human.name.clone())
                    .collect::<Vec<_>>();
                state
                    .match_state
                    .session
                    .canonicalize_online_roster(&names)
                    .inspect_err(|_| {
                        log::warn!("Online host options roster was not admitted");
                    })?;
                let events = p.options.into_setup_events(&local).inspect_err(|_| {
                    log::warn!("Online host options had no admitted local player");
                })?;
                for event in events {
                    crate::app::shell_network_setup::service_event(state, event).inspect_err(
                        |_| {
                            log::warn!("Online host setup was not admitted");
                        },
                    )?;
                }
            }
            if authored {
                maps::cancel(state, service);
            } else {
                maps::received_options(state, service, generated)?;
                // VERA: a guest checks the map these options name before
                // reporting on it, or one that has it reports 0 first and the
                // host's room says it lacks the map. The host's own upload
                // still waits for the poll, after the lobby's `map_available`.
                if !state.match_state.session.host() {
                    poll_maps(state, service);
                }
            }
            // The owner is ready without a guest Accept button, but still proves its
            // own local map and rules to the same server admission gate.
            let agreement = local_agreement(state, service.revision)?;
            let status = local_map_status(state, service);
            service.publish_options_agreement(agreement, status, state.match_state.session.host());
        }
        "choices" => {
            #[derive(Deserialize)]
            struct Payload {
                from: String,
                slot: crate::skirmish_launch::SkirmishLocalSlot,
                observer: bool,
            }
            let p: Payload = decode(payload)?;
            service_event(
                state,
                WolServiceEvent::PlayerChoices {
                    from: p.from,
                    slot: p.slot,
                    observer: p.observer,
                },
            );
        }
        // The lobby's answer to the host's `/balance` (docs/lobby-protocol.md).
        "balance" => {
            #[derive(Deserialize)]
            struct Team {
                name: String,
                team: u8,
            }
            #[derive(Deserialize)]
            struct Payload {
                teams: Vec<Team>,
            }
            let p: Payload = decode(payload)?;
            let teams: Vec<_> = p
                .teams
                .into_iter()
                .map(|team| (team.name, team.team))
                .collect();
            if let Err(error) = crate::app::shell_network_setup::balance_teams(state, &teams) {
                log::warn!("lobby balance: {error:#}");
            }
        }
        "ready" => {
            if payload["revision"].as_u64() != Some(service.revision) {
                return Ok(());
            }
            let name = payload["from"]
                .as_str()
                .ok_or_else(|| anyhow!("missing player"))?
                .to_owned();
            let ready = payload["ready"]
                .as_bool()
                .ok_or_else(|| anyhow!("missing ready state"))?;
            service_event(
                state,
                WolServiceEvent::PlayerReady {
                    from: name,
                    value: i32::from(ready),
                },
            );
        }
        "map_status" => {
            if payload["revision"].as_u64() != Some(service.revision) {
                return Ok(());
            }
            let name = payload["from"]
                .as_str()
                .ok_or_else(|| anyhow!("missing player"))?
                .to_owned();
            let status = payload["status"]
                .as_u64()
                .filter(|value| *value <= 2)
                .ok_or_else(|| anyhow!("invalid map status"))? as u8;
            service_event(state, WolServiceEvent::MapStatus { from: name, status });
        }
        "map_available" => {
            let digest = payload["digest"]
                .as_str()
                .ok_or_else(|| anyhow!("missing map digest"))?;
            let total = payload["byte_length"]
                .as_u64()
                .ok_or_else(|| anyhow!("missing map length"))?;
            maps::available(state, service, digest, usize::try_from(total)?);
        }
        "chat" => {
            let p: Chat = decode(payload)?;
            let (text, localized) = p.shown_text();
            if p.visit {
                service_event(
                    state,
                    WolServiceEvent::VisitMessage {
                        text,
                        localized,
                        from: p.from.into_member(&local),
                        private: p.private,
                        action: p.action,
                        sent_at: p.sent_at,
                    },
                );
                return Ok(());
            }
            let options = p
                .hosted_pairing(&local)
                .and_then(|pairing| super::quick_match::online_start_options(state, pairing));
            if p.announces_release() {
                lobby_says_outdated(state);
            }
            let warning = p.warns_of_other_game();
            service_event(
                state,
                WolServiceEvent::Message {
                    from: p.from.into_member(&local),
                    text,
                    localized,
                    warning,
                    private: p.private,
                    action: p.action,
                    ansi: p.ansi,
                    options,
                    sent_at: p.sent_at,
                },
            );
        }
        "locales" => {
            #[derive(Deserialize)]
            struct Locale {
                name: String,
                locale: i32,
            }
            #[derive(Deserialize)]
            struct Payload {
                locales: Vec<Locale>,
            }
            let p: Payload = decode(payload)?;
            service_event(
                state,
                WolServiceEvent::UserLocales(Ok(p
                    .locales
                    .into_iter()
                    .map(|entry| (entry.name, entry.locale))
                    .collect())),
            );
        }
        "buddies" => {
            #[derive(Deserialize)]
            struct Payload {
                names: Vec<String>,
            }
            let p: Payload = decode(payload)?;
            service_event(state, WolServiceEvent::Buddies(Ok(p.names)));
        }
        "located" => {
            #[derive(Deserialize)]
            struct Payload {
                channel: Option<wire::Channel>,
            }
            let p: Payload = decode(payload)?;
            service_event(
                state,
                WolServiceEvent::Located(p.channel.map(Into::into).ok_or(0x8000_4005)),
            );
        }
        "ladder" => {
            let ladder = decode::<wire::Ladder>(payload)?;
            let (records, high_scores) = ladder.tables();
            service_event(state, WolServiceEvent::LadderSeason(ladder.season));
            service_event(state, WolServiceEvent::Ladder(records));
            service_event(state, WolServiceEvent::HighScores(Ok(high_scores)));
        }
        "kicked" => {
            #[derive(Deserialize)]
            struct Payload {
                channel: wire::Channel,
                user: wire::Member,
                by: wire::Member,
            }
            let p: Payload = decode(payload)?;
            service_event(
                state,
                WolServiceEvent::Kicked {
                    channel: p.channel.into(),
                    user: p.user.into_member(&local),
                    by: p.by.into_member(&local),
                },
            );
        }
        "game_started" => {
            let admitted = service.receive_lobby_start(payload, |service, start| {
                // The lobby ended a visit to the public lobby for the match.
                super::lobby::visit_ends(state);
                begin_start(state, service, start)
            });
            if let Err(error) = admitted {
                if !service.locally_refused_start {
                    return Err(error);
                }
                log::warn!("Online match start refused locally: {error:#}");
                let line = if error.is::<StartDataMismatch>() {
                    "VERA:MatchStartMismatch"
                } else if error.is::<std::io::Error>() {
                    "VERA:RelayConnectionFailed"
                } else {
                    "VERA:StartRefusedSetup"
                };
                if state.frontend.network_setup_protocol.is_some() {
                    fail_start(state, &tr(line));
                } else {
                    // A delayed callback after leaving its setup may finish
                    // only its match. Never leave the player's current room.
                    notice(state, &tr(line));
                }
            }
        }
        "start_confirmed" => {
            let id = payload["match_id"]
                .as_str()
                .ok_or_else(|| anyhow!("missing match"))?;
            let pending = service
                .pending
                .as_mut()
                .ok_or_else(|| anyhow!("no pending match"))?;
            ensure!(
                pending.match_id == id && pending.relay_acknowledged,
                "unadmitted start"
            );
            if !pending.probes_started {
                crate::app::lan_service::begin_online_latency_probe(state)?;
                pending.probes_started = true;
                pending.deadline = Instant::now() + service.relay_timeout;
            }
        }
        "timing_confirmed" => {
            #[derive(Deserialize)]
            struct TimingConfirmed {
                match_id: String,
                pings: Vec<i32>,
                relay: wire::RelayGrant,
                #[serde(default)]
                ranked: bool,
            }
            let timing: TimingConfirmed = decode(payload)?;
            let pending = service
                .pending
                .as_ref()
                .ok_or_else(|| anyhow!("no pending match"))?;
            ensure!(
                pending.match_id == timing.match_id && pending.latencies_reported,
                "timing was not measured for this start"
            );
            let grant = decode_relay_grant(&timing.relay, &timing.match_id, &service.player_id)?;
            crate::app::lan_service::renew_online_relay(
                state,
                timing.relay.address,
                &grant.ticket,
                grant.key,
                grant.payload_key,
            )?;
            if timing.ranked {
                log::info!("Online match {} is ranked", timing.match_id);
            }
            enter_timed_start(state, service, timing.match_id, &timing.pings)?;
        }
        "start_rejected" => {
            if std::mem::take(&mut service.locally_refused_start) {
                // Our finish (or a simultaneous peer cancellation) released
                // this start. Keep the reason this game already showed.
                return Ok(());
            }
            let reason = payload["message"].as_str().unwrap_or_default();
            let names = lobby_names(&payload);
            log::warn!(
                "The lobby refused the online start: {reason} {}",
                names.as_deref().unwrap_or_default()
            );
            if service.pending.is_none()
                && crate::app::shell_network_setup::verify(state).is_none()
                && state.frontend.network_setup_protocol.is_none()
            {
                // This game already failed the start and left the room: only
                // the lobby's names are new.
                if let Some(names) = names {
                    crate::app::shell_wol::add_problem_line(state, names);
                }
                return Ok(());
            }
            let line = tr(service.start_rejected_line(reason));
            let text = names.map_or_else(|| line.clone(), |names| format!("{line} {names}"));
            cancel_pending(state, service);
            fail_start(state, &text);
        }
        "error" => notice(state, &tr("VERA:OnlineServiceRefused")),
        _ => {}
    }
    Ok(())
}
