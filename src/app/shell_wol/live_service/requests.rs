//! Outgoing lobby requests: the WOL service requests, chat, and the room
//! choices this game proposes, agrees and checks against the accepted ones.

use super::{
    LiveService, PendingChoices, RELAY_TIMEOUT, StartPreparation, WolProvider, WolServiceEvent,
    WolServiceRequest, answer_local_request, decode, end_room, fail_start, forget_sign_in, http,
    match_report, notice, service_event, settings_dir, sign_in_credential, wire,
};
use crate::app::AppState;
use crate::app::shell_network_setup::NetworkSetupRequest;
use crate::app::shell_network_setup::campaign_room;
use crate::i18n::tr;
use crate::ui::wol_chat::WolChatLine;
use crate::util::time::{Duration, Instant};
use anyhow::{Result, anyhow, ensure};
use serde_json::{Value, json};

pub(super) fn request_service(
    state: &mut AppState,
    service: &mut LiveService,
    request: WolServiceRequest,
) {
    match request {
        request @ (WolServiceRequest::EnsureApi | WolServiceRequest::FetchServers { .. }) => {
            answer_local_request(state, &request);
        }
        // The login reducer asks only native providers for stored credentials.
        WolServiceRequest::ListCredentials => {}
        WolServiceRequest::Connect {
            nickname,
            password,
            remember,
            generated_guest,
            ..
        } => {
            service.auth_attempt = service.auth_attempt.wrapping_add(1);
            let credential = if generated_guest {
                Some(http::Credential::GeneratedGuest)
            } else {
                sign_in_credential(settings_dir(state), service, &nickname, password, remember)
            };
            let Some(credential) = credential else {
                super::apply_response(
                    state,
                    service,
                    http::Response::SignInRefused {
                        attempt: service.auth_attempt,
                        message: tr("VERA:LobbySignInWithPassword"),
                        bad_credential: false,
                    },
                );
                return;
            };
            service.signing_in_remembered = matches!(credential, http::Credential::Remembered(_));
            // What this game changes from the original game, which a room
            // names when its rules differ from the host's.
            use crate::app::shell_network_setup::agreement;
            let changes = agreement::changes_header(&agreement::original_game_changes(
                state.frontend.mods.title(),
                &state.process_assets,
            ));
            service.sign_in(nickname, credential, changes);
        }
        WolServiceRequest::Account { attempt, request } => {
            let _ = service
                .requests
                .send(http::Request::Account { attempt, request });
        }
        WolServiceRequest::ForgetSignIn => forget_sign_in(settings_dir(state), service),
        WolServiceRequest::ListChannels { kind, .. } => {
            let _ = service.requests.send(http::Request::Channels(kind));
        }
        WolServiceRequest::ListMatches => {
            let _ = service.requests.send(http::Request::Matches {
                refused_options: None,
            });
        }
        WolServiceRequest::Watch { match_id } => {
            if super::lobby::bind_online_request(state, service.next_request) {
                service.command("watch", json!({"match_id":match_id}));
            }
        }
        WolServiceRequest::JoinChannel { channel, key } => {
            if super::lobby::bind_online_request(state, service.next_request) {
                let payload = json!({"channel":channel.name,"key":key});
                if super::lobby::switching_room(state) {
                    service.switching_room =
                        Some(service.retrying_command("join", payload, Instant::now()));
                } else {
                    service.command("join", payload);
                }
            }
        }
        WolServiceRequest::VisitLobby => {
            if super::lobby::bind_online_request(state, service.next_request) {
                service.command("visit_lobby", json!({}));
            }
        }
        WolServiceRequest::EndVisit => service.command("end_visit", json!({})),
        WolServiceRequest::VisitChat { line, to } => chat(state, service, line, to, true),
        WolServiceRequest::VisitEmote { text, to } => service.command(
            "chat",
            json!({"text":String::from_utf16_lossy(&text),"to":to,"action":true,"visit":true}),
        ),
        WolServiceRequest::CreateChannel {
            channel,
            key,
            ranked,
        } => {
            if super::lobby::bind_online_request(state, service.next_request) {
                service.command("create", create_payload(channel, key, ranked));
            }
        }
        WolServiceRequest::LeaveChannel => {
            end_room(state, service);
            service.leave_room(Instant::now());
        }
        WolServiceRequest::MatchReturned => {
            service.back_to_room = state.match_state.session.awaiting_wol_room_return();
            service.match_returned(match_report(state), Instant::now());
        }
        WolServiceRequest::Logout => {
            end_room(state, service);
            service.auth_attempt = service.auth_attempt.wrapping_add(1);
            service.begin_logout(Instant::now());
            crate::app::lan_service::close(state);
        }
        WolServiceRequest::Chat { line, to } => chat(state, service, line, to, false),
        WolServiceRequest::Emote { text, to } => service.command(
            "chat",
            json!({"text":String::from_utf16_lossy(&text),"to":to,"action":true}),
        ),
        WolServiceRequest::RequestLocales { names } => {
            service.command("locales", json!({"names":names}))
        }
        WolServiceRequest::SetLocale(locale) => {
            service.command("set_locale", json!({"locale":locale}))
        }
        WolServiceRequest::LocaleCount => service_event(
            state,
            WolServiceEvent::LocaleCount(crate::ui::wol_persona::LOCALE_COUNT),
        ),
        WolServiceRequest::ListBuddies => service.command("buddies", json!({})),
        WolServiceRequest::AddBuddy(name) => service.command("add_buddy", json!({"name":name})),
        WolServiceRequest::RemoveBuddy(name) => {
            service.command("remove_buddy", json!({"name":name}))
        }
        WolServiceRequest::Locate(name) => service.command("locate", json!({"name":name})),
        WolServiceRequest::SquadInfo { id } => service_event(
            state,
            WolServiceEvent::Squad {
                id,
                squad: Err(0x8000_4005),
            },
        ),
        WolServiceRequest::LadderRecords(names) => service.look_up_ladder(names, false),
        WolServiceRequest::HighScores(names) => service.look_up_ladder(names, true),
        WolServiceRequest::CancelWait => {
            end_room(state, service);
            service.auth_attempt = service.auth_attempt.wrapping_add(1);
            if let Some(visit) = state.frontend.wol.as_mut() {
                visit.authenticated = false;
                visit.connected = false;
                visit.requests.retain(|request| {
                    !matches!(
                        request,
                        WolServiceRequest::JoinChannel { .. }
                            | WolServiceRequest::CreateChannel { .. }
                            | WolServiceRequest::Watch { .. }
                            | WolServiceRequest::VisitLobby
                    )
                });
            }
            service.begin_logout(Instant::now());
        }
        WolServiceRequest::StoreCredential { .. }
        | WolServiceRequest::ForgetCredential { .. }
        | WolServiceRequest::SetLangFilter(_)
        | WolServiceRequest::SetFindPage { .. } => {}
        WolServiceRequest::RequestMatch { matchbot, request } => {
            request_match(service, matchbot, &request)
        }
        WolServiceRequest::Translate { text, target, line } => {
            service
                .translations
                .request(&service.requests, &text, target, line, Instant::now());
        }
    }
}

/// `create`'s payload. Only an unranked room says so, as a lobby from before
/// the choice refuses the field.
pub(super) fn create_payload(
    channel: crate::ui::wol_lobby::WolChannel,
    key: String,
    ranked: bool,
) -> Value {
    let mut payload = json!({"channel":wire::Channel::from(channel),"key":key});
    if !ranked {
        payload["ranked"] = json!(false);
    }
    payload
}

/// API+0x30's request: ordinary private chat to the matchbot. This game
/// has no transport fields to report, so RAT, ADR, NAT and PRT are 0.
pub(super) fn request_match(
    service: &mut LiveService,
    matchbot: String,
    request: &super::quick_match::MatchRequest,
) {
    let text = request.text(super::quick_match::MatchTransport::default());
    send_chat(
        service,
        None,
        WolChatLine::Say(text.encode_utf16().collect()),
        vec![matchbot],
        false,
    );
}

/// [`send_chat`], answering a page's result to its sink `0x007A9800`.
pub(super) fn chat(
    state: &mut AppState,
    service: &mut LiveService,
    line: WolChatLine,
    to: Vec<String>,
    visit: bool,
) {
    if let Some(result) = send_chat(service, state.frontend.wol.as_ref(), line, to, visit) {
        service_event(state, WolServiceEvent::PageResult(result));
    }
}

/// `visit` sends to the public lobby this player visits from their room.
/// A page (API+0xF8, or `/reply` to `[0xB77E30]`) returns its result: the
/// lobby delivers private chat only within one room and outside a match
/// (docs/lobby-protocol.md, `chat`), so a page goes, under the name the
/// room lists, to this player's channel or the lobby they visit, and fails
/// unsent for anyone neither lists.
/// RESIDUAL: success is answered on sending; a later refusal shows the
/// generic refused line after `TXT_PAGESENT`.
pub(super) fn send_chat(
    service: &mut LiveService,
    wol: Option<&super::WolVisit>,
    line: WolChatLine,
    mut to: Vec<String>,
    mut visit: bool,
) -> Option<Result<(), u32>> {
    let (text, page) = match line {
        WolChatLine::Say(text) => (text, None),
        WolChatLine::Page { name, text } => (text, Some(String::from_utf16_lossy(&name))),
        WolChatLine::Reply { text } => {
            let pager = wol
                .and_then(|wol| wol.last_pager.clone())
                .unwrap_or_default();
            (text, Some(pager))
        }
        WolChatLine::Locate { name } => {
            service.command("locate", json!({"name":String::from_utf16_lossy(&name)}));
            return None;
        }
        WolChatLine::Notice(_) => return None,
    };
    if let Some(name) = &page {
        let Some((name, visited)) = wol.and_then(|wol| super::lobby::page_target(wol, name)) else {
            return Some(Err(0x8000_4005));
        };
        to = vec![name.to_owned()];
        visit = visited;
    }
    let mut chat = json!({"text":String::from_utf16_lossy(&text),"to":to,"action":false});
    if visit {
        chat["visit"] = Value::Bool(true);
    }
    service.command("chat", chat);
    page.map(|_| Ok(()))
}

/// The map digests are the verified local source's. Without one the map
/// status is 0, which proves nothing: the lobby checks only the shape, and a
/// fixed impossible effective digest keeps it valid without claiming
/// unverified expanded content.
pub(super) fn local_agreement(state: &mut AppState, revision: u64) -> Result<Value> {
    let session = &state.match_state.session;
    let (map_digest, effective_map_digest, staging_digest) =
        if let Some(room) = session.campaign_room() {
            if campaign_room::local_map_verified(session) == Some(true) {
                let (map, effective) = campaign_room::online_agreement(room);
                (map.to_owned(), Some(effective.to_owned()), String::new())
            } else {
                (String::new(), None, String::new())
            }
        } else {
            let map = session
                .network_local_map_source()
                .and_then(|source| source.physical_identity())
                .map_or_else(String::new, |(_, digest)| digest.to_owned());
            let effective = session
                .network_local_map_effective_digest()
                .map(str::to_owned);
            let staging = crate::app::shell_network_setup::maps::staging_digest(state);
            (map, effective, staging)
        };
    let effective_map_digest = effective_map_digest.unwrap_or_else(|| "0".repeat(64));
    let agreement = crate::app::shell_network_setup::agreement::local(state)?;
    Ok(
        json!({"revision":revision,"version":agreement.version,"rules_digest":agreement.rules_digest,"map_digest":map_digest,"effective_map_digest":effective_map_digest,"staging_digest":staging_digest}),
    )
}

pub(super) fn local_map_status(state: &AppState, service: &LiveService) -> u8 {
    if let Some(verified) = campaign_room::local_map_verified(&state.match_state.session) {
        return u8::from(verified);
    }
    if !service.maps.current_verified(state) {
        return 0;
    }
    let Some(expected) = state.match_state.session.network_map_metadata() else {
        return 0;
    };
    let Some(file) = state
        .match_state
        .session
        .network_setup()
        .and_then(|launch| launch.selected_map_file.as_deref())
    else {
        return 0;
    };
    if crate::map::rmg::is_seed_selection(file)
        && state
            .match_state
            .session
            .network_random_map_preview()
            .is_none()
    {
        return 0;
    }
    u8::from(
        crate::map::source::valid_source_sha256(&expected.source_sha256)
            && state
                .match_state
                .session
                .network_local_map_source()
                .and_then(|source| source.physical_identity())
                == Some((
                    expected.byte_length as usize,
                    expected.source_sha256.as_str(),
                )),
    )
}

#[derive(Debug, thiserror::Error)]
#[error("room exceeds the map's {0} starts")]
pub(super) struct MapTooSmall(pub(super) i32);

pub(super) fn check_map_capacity(options: &wire::Options) -> Result<(), MapTooSmall> {
    if options.campaign.is_some() {
        return Ok(());
    }
    let launch = &options.launch;
    let needed = crate::app::shell_network_setup::starts_needed(
        launch,
        launch.options.teams.is_some(),
        crate::ui::multiplayer_setup::SetupFlavor::Wol,
    );
    if needed > options.map_metadata.player_capacity {
        return Err(MapTooSmall(options.map_metadata.player_capacity));
    }
    Ok(())
}

/// The host's options for the lobby. The map is the one its session
/// admitted when the host chose it, as LAN publishes it.
pub(super) fn options(state: &AppState) -> Result<wire::Options> {
    let session = &state.match_state.session;
    let mut launch = session
        .network_setup()
        .ok_or_else(|| anyhow!("no online setup"))?
        .clone();
    let campaign = campaign_room::publication(state)?;
    let map_metadata = if let Some(room) = &campaign {
        let (file, metadata) = campaign_room::online_map_metadata(state, room)?;
        launch.selected_map_file = Some(file);
        launch.mode = crate::skirmish_launch::SkirmishLaunchMode::stock(
            crate::skirmish_launch::BATTLE_MODE_ID,
        )
        .expect("stock lobby mode");
        let speed = launch.options.game_speed;
        launch.options = crate::skirmish_launch::SkirmishLaunchOptions::default();
        launch.options.game_speed = speed;
        launch.opponents.clear();
        launch.ai_player_count = 0;
        launch.use_live_house_rows();
        metadata
    } else {
        session
            .network_map_metadata()
            .cloned()
            .ok_or_else(|| anyhow!("no admitted map metadata"))?
    };
    let options = wire::Options {
        campaign: campaign.clone(),
        launch,
        map_metadata,
        random_map: if campaign.is_none() {
            crate::app::shell_network_setup::maps::host_generated_manifest(state)?
        } else {
            None
        },
        vacancies: std::array::from_fn(|row| {
            session
                .network_vacancy(row as u8)
                .unwrap_or(crate::app::match_runtime::session::NetworkVacancy::Closed)
        }),
        observer_row: session.network_observer_row(),
        cooperative_difficulty: state
            .frontend
            .multiplayer_shell_runtime
            .cooperative_difficulty(),
        cooperative_campaign: campaign
            .is_none()
            .then(|| cooperative_campaign(state))
            .flatten(),
        seed: Some(crate::app::shell_network_setup::resident_seed(state)),
        // Only a host's VERA room has one; a guest publishes no options.
        watching: state
            .frontend
            .network_setup_protocol
            .as_ref()
            .and_then(|protocol| protocol.published_watching(super::lobby_watches(state)))
            .map(str::to_owned),
    };
    check_map_capacity(&options)?;
    Ok(options)
}

/// The host's current `C0` text for its options, only in Cooperative: the
/// record `+0x60` would send each guest at that moment.
pub(super) fn cooperative_campaign(state: &AppState) -> Option<String> {
    let cooperative = state
        .match_state
        .session
        .network_setup()
        .is_some_and(|launch| launch.mode.class == crate::skirmish_modes::MpModeClass::Cooperative);
    cooperative
        .then(|| {
            state
                .frontend
                .multiplayer_shell_runtime
                .cooperative_campaign_text()
        })
        .flatten()
}

pub(super) fn canonical_options(options: &wire::Options) -> Result<Value> {
    let mut value = serde_json::to_value(options)?;
    value["launch"]["local_player_index"] = json!(0);
    Ok(value)
}

pub(super) fn local_choices_match(
    session: &crate::app::match_runtime::session::MatchSession,
    accepted: &Value,
) -> bool {
    let Some(local) = session.network_setup().map(|launch| launch.local_player()) else {
        return false;
    };
    let Ok(local) = serde_json::to_value(local) else {
        return false;
    };
    accepted_human_matches(accepted, &local)
}

pub(super) fn accepted_human_matches(accepted: &Value, human: &Value) -> bool {
    accepted["launch"]["humans"]
        .as_array()
        .is_some_and(|humans| humans.iter().any(|candidate| candidate == human))
}

/// Whether the accepted options hold a player's requested choices in
/// whatever row the host seated it: the host's `choices` takes no row, and a
/// guest's first choices leave before its seat is known. With Teams set, the
/// host's game puts every seated player on a team itself
/// (`MatchSession::admit_network_team`), so it answers with whatever team
/// it gave.
pub(super) fn accepted_choices_match(accepted: &Value, human: &Value) -> bool {
    let any_team = !accepted["launch"]["options"]["teams"].is_null();
    let seatless = |human: &Value| {
        let mut human = human.clone();
        if let Some(fields) = human.as_object_mut() {
            fields.remove("raw_slot_index");
            if any_team && let Some(slot) = fields.get_mut("slot").and_then(Value::as_object_mut) {
                slot.remove("team");
            }
        }
        human
    };
    let expected = seatless(human);
    accepted["launch"]["humans"]
        .as_array()
        .is_some_and(|humans| {
            humans
                .iter()
                .any(|candidate| seatless(candidate) == expected)
        })
}

pub(super) fn restore_accepted_options(state: &mut AppState, service: &LiveService) {
    let Some(accepted) = service.room.accepted_options.clone() else {
        return;
    };
    let result = decode::<wire::Options>(accepted)
        .and_then(|options| options.into_setup_events(&state.frontend.wol_account.nickname))
        .and_then(|events| {
            events
                .into_iter()
                .try_for_each(|event| crate::app::shell_network_setup::service_event(state, event))
        });
    if result.is_err() {
        notice(state, &tr("VERA:HostSettingsUnavailable"));
    }
}

/// Optimistic country/color controls cannot accept until the host's complete
/// server options echo the same local row. Controlled/native providers keep
/// their existing admission behavior.
pub(crate) fn local_choices_agreed(state: &AppState) -> bool {
    let Some(visit) = state
        .frontend
        .wol
        .as_ref()
        .filter(|visit| visit.provider == WolProvider::Online)
    else {
        return true;
    };
    visit.live_service.as_ref().is_some_and(|service| {
        service.room.pending_choices.is_none()
            && service
                .room
                .accepted_options
                .as_ref()
                .is_some_and(|accepted| local_choices_match(&state.match_state.session, accepted))
    })
}

pub(super) fn poll_start_preparation(
    state: &mut AppState,
    service: &mut LiveService,
) -> Result<()> {
    let Some(mut start) = service.start_preparation.take() else {
        return Ok(());
    };
    if Instant::now() >= start.deadline {
        fail_start(state, &tr("VERA:PlayersNotReadyInTime"));
        return Ok(());
    }
    if start.submitted_request.is_some()
        || !start.ready(&state.match_state.session, service.revision)
        || local_map_status(state, service) != 1
    {
        service.start_preparation = Some(start);
        return Ok(());
    }
    ensure!(
        canonical_options(&options(state)?)? == start.options,
        "prepared options changed"
    );
    let mut payload = local_agreement(state, service.revision)?;
    payload["users"] = json!(&start.users);
    start.submitted_request = Some(service.command_with_id("start", payload));
    start.deadline = Instant::now() + RELAY_TIMEOUT;
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        protocol.set_start_phase(crate::app::shell_network_setup::StartPhase::Submitted);
    }
    service.start_preparation = Some(start);
    Ok(())
}

pub(super) fn request_setup(
    state: &mut AppState,
    service: &mut LiveService,
    request: NetworkSetupRequest,
) -> Result<()> {
    match request {
        // Cooperative's C0 also travels in the host's options.
        NetworkSetupRequest::PublishOptions
        | NetworkSetupRequest::PublishRoster
        | NetworkSetupRequest::PublishCooperativeCampaign(_) => {
            service.publish_room_options(options(state)?);
        }
        NetworkSetupRequest::PlayerChoices {
            row,
            slot,
            observer,
        } => {
            let mut human = state
                .match_state
                .session
                .network_setup()
                .ok_or_else(|| anyhow!("no setup for player choices"))?
                .local_player()
                .clone();
            // Earlier full options can arrive before this queue drains. The
            // outgoing request, not that intervening UI echo, defines its ack.
            // A guest who starts or stops watching changed nothing here: the
            // host picks its row, or keeps it.
            let switch = human.is_observer() != observer;
            human.slot = slot.clone();
            human.set_observer(observer);
            let expected_human = serde_json::to_value(&human)?;
            human.slot.country_random = true;
            let expected_random = serde_json::to_value(human)?;
            let request_id = service.command_with_id(
                "choices",
                json!({"row":row,"slot":slot,"observer":observer}),
            );
            // A Quick Match host drops `R` (`0x007A8260`), so no options
            // answer it.
            let answered = !crate::app::shell_network_setup::quick_match(state);
            service.room.pending_choices = (answered
                && !switch
                && !service
                    .room
                    .accepted_options
                    .as_ref()
                    .is_some_and(|accepted| accepted_choices_match(accepted, &expected_human)))
            .then_some(PendingChoices {
                request_id,
                expected_human,
                expected_random,
                deadline: Instant::now() + RELAY_TIMEOUT,
            });
        }
        NetworkSetupRequest::Ready(requested) => {
            let ready = requested && local_map_status(state, service) == 1;
            let index = state
                .match_state
                .session
                .network_setup()
                .ok_or_else(|| anyhow!("no setup"))?
                .local_player_index;
            let withdrawn = state.match_state.session.network_player_ready(index);
            if requested && !ready {
                // The guest's automatic Accept waits for the verified map
                // instead of repeating this request every frame.
                crate::app::shell_network_setup::refuse_local_acceptance(state)?;
                if !withdrawn {
                    // Only the Quick Match guest's accept (`0x007A7F72`)
                    // leaves its node unaccepted. It withdraws nothing, so
                    // nothing is sent: the host starts on the guest's first
                    // `ready` (`0x0077EA53`) and drops the later ones.
                    return Ok(());
                }
            } else if !ready {
                state
                    .match_state
                    .session
                    .set_network_player_ready(index, false)?;
            }
            let mut payload = local_agreement(state, service.revision)?;
            payload["ready"] = json!(ready);
            let id = service.command_with_id("ready", payload);
            service.room.last_ready_request = Some((id, service.revision));
        }
        NetworkSetupRequest::MapStatus(status) => {
            let mut payload = local_agreement(state, service.revision)?;
            // Native RandMap admission assumes the pending seed/image
            // receive. Online status1 requires both verified resources.
            payload["status"] = json!(status.min(local_map_status(state, service)));
            service.command("map_status", payload);
        }
        NetworkSetupRequest::StartGame { users } => {
            if service.start_preparation.is_none() {
                service.room.published_options = None;
                let snapshot = canonical_options(&options(state)?)?;
                let revision = (service.room.accepted_options.as_ref() == Some(&snapshot))
                    .then_some(service.revision);
                let publication_request = if revision.is_none() {
                    Some(service.command_with_id("options", json!({"options":snapshot})))
                } else {
                    None
                };
                service.start_preparation = Some(StartPreparation {
                    users: StartPreparation::users_in_human_order(
                        &state.match_state.session,
                        users,
                    ),
                    options: snapshot,
                    revision,
                    publication_request,
                    submitted_request: None,
                    deadline: Instant::now() + Duration::from_secs(90),
                });
                if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
                    protocol
                        .set_start_phase(crate::app::shell_network_setup::StartPhase::Preparing);
                }
                notice(state, &tr("VERA:ConfirmingFinalSettings"));
            }
        }
        NetworkSetupRequest::Leave => {
            end_room(state, service);
            service.leave_room(Instant::now());
        }
        // Older lobbies take no keep_out, so only a host's own kick sends it.
        NetworkSetupRequest::Kick { name, keep_out } if keep_out => {
            service.command("kick", json!({"name":name,"keep_out":true}))
        }
        NetworkSetupRequest::Kick { name, .. } => service.command("kick", json!({"name":name})),
        NetworkSetupRequest::Advertise => {
            if let Some(channel) = super::lobby::republish_channel(state) {
                service.command("advertise", json!({"channel":wire::Channel::from(channel)}));
            }
        }
        // VERA's room: the host's Balance is `/balance` as typed.
        NetworkSetupRequest::Balance => {
            service.command("chat", json!({"text":"/balance","to":[],"action":false}))
        }
        NetworkSetupRequest::WolChat(line) => chat(state, service, line, Vec::new(), false),
        NetworkSetupRequest::WolEmote(text) => service.command(
            "chat",
            json!({"text":String::from_utf16_lossy(&text),"to":[],"action":true}),
        ),
        NetworkSetupRequest::RequestAcceptance | NetworkSetupRequest::ObserverSlot { .. } => {}
        _ => notice(state, &tr("VERA:SetupOperationUnavailable")),
    }
    Ok(())
}
