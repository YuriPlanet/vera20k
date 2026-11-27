//! Starting a lobby match: the timed and restored starts, watching another
//! player's match, relay admission, spawned match processes and the relay poll.

use super::WATCH_ADMISSION_TIMEOUT;
use super::{
    DecodedRelayGrant, EXIT_CLEANUP_TIMEOUT, LiveService, PendingStart, SpawnedVisit, WatchStart,
    WolProvider, decode_relay_grant, decode_watch_grant, end_room, http, local_agreement,
    local_map_status, put_service, wire,
};
use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::shell_network_setup::NetworkSetupEvent;
use crate::app::shell_network_setup::campaign_room;
use crate::app::shell_wol::WolVisit;
use crate::app::{AppState, PlatformState};
use crate::net::reliable::{LanConnectionConfig, LanConnections, RetryTiming};
use crate::net::udp::{LanUdpTransport, UdpBindConfig};
use crate::util::config::GameConfig;
use crate::util::time::Instant;
use anyhow::{Result, anyhow, ensure};
use serde_json::json;
use std::net::{Ipv4Addr, SocketAddrV4};
use winit::window::Window;

/// The agreed timing is known and the current grant is fresh: leave the online
/// barriers for the loading that follows `Negotiated`.
pub(super) fn enter_timed_start(
    state: &mut AppState,
    service: &mut LiveService,
    match_id: String,
    pings: &[i32],
) -> Result<()> {
    let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32;
    service.relay_granted(now_ms);
    if service.spawned_watch.is_some() {
        // A launcher's watch ticket may have spent most of its life waiting
        // for the relay to let it in: renew it at once.
        service.relay_renewal_due = Some(now_ms);
    }
    // A new match: no earlier one's result may stand for it.
    crate::app::App::clear_match_result(&mut state.frontend);
    crate::app::lan_service::apply_online_timing(&mut state.match_state, pings)?;
    let count = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow!("missing setup"))?
        .humans
        .len();
    for node in 1..count {
        crate::app::shell_network_setup::service_event(
            state,
            NetworkSetupEvent::PeerConnected { node },
        )?;
    }
    crate::app::shell_network_setup::service_event(
        state,
        NetworkSetupEvent::Negotiated { result: 0 },
    )?;
    // A watched match is no match of this player's: nothing finishes it.
    if service.watching.is_none() && service.spawned_watch.is_none() {
        service.hold_match(Some(match_id));
    }
    service.pending = None;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(super) struct StartDataMismatch(&'static str);

pub(super) fn begin_start(
    state: &mut AppState,
    service: &mut LiveService,
    start: wire::Start,
) -> Result<()> {
    ensure!(
        !state.match_state.session.host()
            || service
                .start_preparation
                .as_ref()
                .is_some_and(|request| request.submitted_request.is_some()),
        "host start was not submitted for this preparation"
    );
    let agreement = local_agreement(
        &state.frontend,
        state.platform.game_config.as_ref(),
        &state.match_state.session,
        &mut state.process_assets,
        service.revision,
    )?;
    ensure!(
        agreement["version"].as_str() == Some(&start.version),
        StartDataMismatch("client version differs")
    );
    ensure!(
        agreement["rules_digest"].as_str() == Some(&start.rules_digest),
        StartDataMismatch("rules differ")
    );
    ensure!(
        local_map_status(
            &state.match_state.session,
            state.frontend.wol.as_ref(),
            service
        ) == 1,
        StartDataMismatch("local map differs")
    );
    let names = start
        .users
        .iter()
        .map(|user| user.name.clone())
        .collect::<Vec<_>>();
    state
        .match_state
        .session
        .canonicalize_online_roster(&names)?;
    let launch = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow!("no setup"))?;
    ensure!(
        start.users.len() == launch.humans.len(),
        "membership differs"
    );
    for (index, user) in start.users.iter().enumerate() {
        ensure!(
            usize::from(user.source_index) == index,
            "noncanonical source index"
        );
        ensure!(
            service.member_ids.get(&user.name.to_ascii_lowercase()) == Some(&user.player_id),
            "unknown player identity"
        );
    }
    let hosts: Vec<_> = start
        .users
        .iter()
        .filter(|user| user.flags & 1 != 0)
        .collect();
    ensure!(hosts.len() == 1, "start requires one admitted host");
    let host_name = hosts[0].name.clone();
    let local = start
        .users
        .iter()
        .find(|user| user.player_id == service.player_id)
        .ok_or_else(|| anyhow!("local user absent"))?;
    ensure!(
        local.source_index == start.relay.source_index,
        "relay grant belongs to another player"
    );
    ensure!(
        launch.local_player_index == usize::from(local.source_index),
        "setup source order differs"
    );
    ensure!(
        (local.flags & 1 != 0) == state.match_state.session.host(),
        "host identity differs"
    );
    for (index, user) in start.users.iter().enumerate() {
        ensure!(
            launch.humans[index].name.eq_ignore_ascii_case(&user.name),
            "setup source order differs"
        );
    }
    let grant = decode_relay_grant(&start.relay, &start.match_id, &service.player_id)?;
    admit_relay_start(
        state,
        service,
        RelayStart {
            match_id: start.match_id,
            host_name,
            users: start
                .users
                .into_iter()
                .map(|user| (user.name, user.flags))
                .collect(),
            seed: start.seed,
            address: start.relay.address,
            gateway: None,
            grant,
            resume_pings: None,
        },
    )
}

/// The lobby's `watch_started` (docs/lobby-protocol.md "Watching"): this
/// game sets up the start every player's game set up, standing in the seat
/// [`followed_user`] picks. The match's map then arrives as a guest's would,
/// and [`poll_watch`] asks the relay to let this game in.
pub(super) fn prepare_watch(
    frontend: &mut FrontendState,
    game_config: Option<&GameConfig>,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    started: wire::WatchStarted,
) -> Result<WatchStart> {
    let grant = decode_watch_grant(&started.relay, &started.match_id)?;
    for (index, user) in started.users.iter().enumerate() {
        ensure!(
            usize::from(user.source_index) == index,
            "noncanonical source index"
        );
    }
    let hosts: Vec<_> = started
        .users
        .iter()
        .filter(|user| user.flags & 1 != 0)
        .collect();
    ensure!(hosts.len() == 1, "start requires one admitted host");
    let host_name = hosts[0].name.clone();
    let followed = followed_user(&started.users, &started.options.launch.humans)
        .ok_or_else(|| anyhow!("the match has no player to follow"))?
        .to_owned();
    let names: Vec<_> = started.users.iter().map(|user| user.name.clone()).collect();
    let room = started.options.campaign.clone();
    if let Some(room) = &room {
        campaign_room::verify_current(game_config, process_assets, room)?;
    }
    let options = started.options.into_options(&followed)?;
    let speed = options.launch.options.game_speed;
    crate::app::shell_network_setup::install_watch_setup(
        frontend,
        match_state,
        options,
        crate::app::match_runtime::session::NetworkRoom {
            tournament: started.tournament != 0,
            clan: false,
        },
    )?;
    match_state.session.canonicalize_online_roster(&names)?;
    if let Some(room) = room {
        match_state
            .session
            .accept_campaign_room(Some(room), speed)?;
        campaign_room::report_local_files(renderer, frontend, match_state, process_assets, true)?;
    }
    Ok(WatchStart {
        version: started.version,
        rules_digest: started.rules_digest,
        host_name,
        users: started
            .users
            .into_iter()
            .map(|user| (user.name, user.flags))
            .collect(),
        seed: started.seed,
        address: started.relay.address,
        grant,
        pings: started.pings,
        // The lobby lists no clan room, whose start splits its
        // players by squad.
        terms: crate::app::match_runtime::session::WolStartTerms {
            tournament: started.tournament,
            squad_lists: Default::default(),
            quick_coop: started.quick_coop,
        },
    })
}

/// The user whose seat a watcher takes: the first in source order who plays
/// rather than observes, or, where only AI play and everyone observes, the
/// first one, whose seat shows the observer's sidebar and the whole map.
pub(super) fn followed_user<'a>(
    users: &'a [wire::WatchedUser],
    humans: &[crate::skirmish_launch::SkirmishHumanSlot],
) -> Option<&'a str> {
    let plays = |user: &&wire::WatchedUser| {
        humans
            .iter()
            .any(|human| human.name.eq_ignore_ascii_case(&user.name) && !human.is_observer())
    };
    users
        .iter()
        .find(plays)
        .or_else(|| {
            users
                .first()
                .filter(|_| humans.iter().all(|human| human.is_observer()))
        })
        .map(|user| user.name.as_str())
}

/// A watch whose map is here asks its relay to let this game in, as a
/// returning player's game does, with the timing every player's game agreed.
pub(super) fn poll_watch(state: &mut AppState, service: &mut LiveService) -> Result<()> {
    if service.pending.is_some()
        || service
            .watching
            .as_ref()
            .is_none_or(|watching| watching.start.is_none())
    {
        return Ok(());
    }
    ensure!(!service.maps.failed(), "the match's map did not arrive");
    if local_map_status(
        &state.match_state.session,
        state.frontend.wol.as_ref(),
        service,
    ) != 1
    {
        return Ok(());
    }
    let watching = service.watching.as_mut().expect("checked watch");
    let start = watching.start.take().expect("checked start");
    let match_id = watching.match_id.clone();
    let agreement = crate::app::shell_network_setup::agreement::local(
        state.platform.game_config.as_ref(),
        &state.match_state.session,
        &mut state.process_assets,
    )?;
    ensure!(
        agreement.version == start.version && agreement.rules_digest == start.rules_digest,
        "this game's version or rules differ from the match's"
    );
    admit_relay_start(
        state,
        service,
        RelayStart {
            match_id,
            host_name: start.host_name,
            users: start.users,
            seed: start.seed,
            address: start.address,
            gateway: None,
            grant: start.grant,
            resume_pings: Some(start.pings),
        },
    )?;
    // The match's own terms, not those of the lobby this game is in.
    state.match_state.session.set_wol_start(start.terms);
    Ok(())
}

/// A watch that could not begin: the lobby frees its place, the setup and
/// any relay route close, and the lobby says why.
pub(super) fn fail_watch(
    frontend: &mut FrontendState,
    window: &Window,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    service: &mut LiveService,
    key: &str,
) {
    cancel_watch_start(frontend, window, match_state, service);
    super::lobby::watch_failed(frontend, process_assets, key);
}

pub(super) fn cancel_watch_start(
    frontend: &mut FrontendState,
    window: &Window,
    match_state: &mut MatchState,
    service: &mut LiveService,
) {
    service.stop_watching();
    end_room(frontend, window, match_state, service);
    if match_state.session.network_setup().is_some() || frontend.network_setup_protocol.is_some() {
        crate::app::shell_network_setup::abandon_for_wol_runner(frontend, match_state);
    }
}

/// One admitted relay start, from the lobby's `game_started` or an external
/// launch's `[Relay]` section: identity checks are the caller's.
struct RelayStart {
    match_id: String,
    host_name: String,
    /// Canonical source order: name and lobby flags (bit 0 room owner).
    users: Vec<(String, u32)>,
    seed: u32,
    address: SocketAddrV4,
    /// A spawn launch's `[Relay] GatewayUrl`; the built-in lobby uses the compiled one.
    gateway: Option<String>,
    grant: DecodedRelayGrant,
    /// A spawn launch returning to a match under way: its agreed timing.
    resume_pings: Option<Vec<i32>>,
}

/// The state right after `game_started`: the authenticated relay socket,
/// the bound roster and seed, and the start handshake's verify. Relay
/// admission, the latency barrier and `timing_confirmed` follow in
/// [`poll_relay`] and [`apply_event`].
fn admit_relay_start(
    state: &mut AppState,
    service: &mut LiveService,
    start: RelayStart,
) -> Result<()> {
    let grant = start.grant;
    let connections = LanConnections::new(
        LanConnectionConfig {
            max_peers: crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT - 1,
            ..Default::default()
        },
        RetryTiming {
            retry_delta: 15,
            max_retries: u32::MAX,
            timeout: 600,
        },
    );
    let any = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0);
    let mut transport = LanUdpTransport::bind(
        UdpBindConfig {
            primary: any,
            alternate: any,
        },
        connections,
    )?;
    transport.set_relay_gateway(start.gateway);
    transport.enable_relay(start.address, &grant.ticket, grant.key, grant.payload_key)?;
    if let Some(roster) = &grant.federation {
        transport.set_federation_roster(roster)?;
    }
    let certificate = crate::net::udp::OrderingCertificate::parse(&grant.ordering_certificate)?;
    ensure!(
        certificate.sources_mask == (1u32 << start.users.len()) - 1,
        "gameplay clock roster differs"
    );
    transport.set_ordering_certificate(grant.ordering_certificate)?;
    transport.start_io_pump(crate::app::match_runtime::frame_pacer::wall_clock_bucket)?;
    let names: Vec<_> = start.users.iter().map(|(name, _)| name.clone()).collect();
    bind_relay_roster(&mut state.match_state.session, &names, start.seed)?;
    // Unknown startup latency uses the native sentinel while the verify dialog
    // is open. Real peer ACK measurements replace it before Negotiated.
    let pings = vec![-1; start.users.len()];
    crate::app::shell_network_setup::service_event(
        state,
        NetworkSetupEvent::GameStarted {
            users: start
                .users
                .into_iter()
                .map(|(name, flags)| crate::app::shell_network_setup::WolStartUser { name, flags })
                .collect(),
            pings,
        },
    )?;
    crate::app::shell_network_setup::service_event(
        state,
        NetworkSetupEvent::VerifyStarted { pairs: true },
    )?;
    service.pending = Some(PendingStart {
        match_id: start.match_id,
        host_name: start.host_name,
        transport: Some(transport),
        deadline: Instant::now() + service.relay_timeout,
        relay_acknowledged: false,
        relay_reported: false,
        probes_started: false,
        latencies_reported: false,
        resume_pings: start.resume_pings,
        report: None,
    });
    Ok(())
}

/// Every canonical source's relay peer identity and the agreed seed.
pub(super) fn bind_relay_roster(
    session: &mut crate::app::match_runtime::session::MatchSession,
    names: &[String],
    seed: u32,
) -> Result<()> {
    for (source, name) in names.iter().enumerate() {
        session.bind_network_endpoint(
            name,
            crate::net::udp::relay_peer_endpoint(u8::try_from(source)?)?,
        )?;
    }
    session.set_network_start_seed(seed)
}

/// Enter the match an external launcher formed (docs/spawn.md): the online
/// visit and setup the lobby path holds right after `game_started`, with the
/// launcher's session, grant and seed. Everything after it is the lobby's.
pub(crate) fn begin_spawned_match(
    state: &mut AppState,
    launch: crate::skirmish_launch::SkirmishLaunchSession,
    relay: crate::app::frontend::cncnet_spawn::RelaySpawn,
    seed: u32,
) -> Result<()> {
    let mut service = LiveService::open(Some(&relay.service_url))?;
    service.spawned = true;
    service.relay_timeout = relay.admission_timeout;
    service.player_id = relay.player_id;
    let (mut resume_pings, resume_cursor) = relay.resume.map_or((None, None), |resume| {
        (Some(resume.pings), Some(resume.cursor))
    });
    if relay.watch {
        // docs/spawn.md "Watching": the match is under way, so the relay's
        // admission is the start, as a lobby watcher's (`poll_watch`). The
        // timing vector only sets retry timing, which a watcher's sends
        // never use; no ping is measured or reported.
        service.spawned_watch = Some(relay.match_id.clone());
        resume_pings = Some(vec![0; launch.humans.len()]);
        // The lobby grants a watch once the match plays, which can be
        // before its players loaded and the relay journals it: until then
        // the relay leaves a watcher's CONFIRM unanswered and the handshake
        // tries again, for as long as players may load.
        service.relay_timeout = service.relay_timeout.max(WATCH_ADMISSION_TIMEOUT);
    }
    // The launcher's bearer replaces `/v1/sessions`; nothing else is opened.
    service
        .requests
        .send(http::Request::AdoptSession {
            token: relay.token,
            resume: resume_cursor.map(|cursor| (relay.match_id.clone(), cursor)),
            watch: relay.watch,
        })
        .map_err(|_| anyhow!("the match service worker stopped"))?;
    let users: Vec<_> = launch
        .humans
        .iter()
        .enumerate()
        .map(|(source, human)| (human.name.clone(), u32::from(source == 0)))
        .collect();
    let host_name = users[0].0.clone();
    crate::app::shell_network_setup::install_external_setup(
        state.platform.game_config.as_ref(),
        &mut state.frontend,
        &mut state.match_state,
        &state.process_assets,
        launch,
        crate::app::match_runtime::session::NetworkReturnContext::Wol,
    )?;
    let mut visit = super::WolVisit::open(false);
    visit.provider = WolProvider::Online;
    visit.page = super::WolPage::Backdrop { wait: None };
    visit.spawned = Some(SpawnedVisit::default());
    state.frontend.wol = Some(visit);
    let admitted = admit_relay_start(
        state,
        &mut service,
        RelayStart {
            match_id: relay.match_id,
            host_name,
            users,
            seed,
            address: relay.address,
            gateway: relay.gateway_url,
            grant: relay.grant,
            resume_pings,
        },
    );
    // Keep this visit's own worker even on failure: no lobby worker may
    // take its place while the process exits.
    put_service(&mut state.frontend, service);
    admitted
}

/// Browser engine exit: retire the external launch's online visit through
/// the ordinary cleanup owner, so an unacknowledged `finish` still settles
/// before notifying the host and stopping the event loop.
#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
pub(crate) fn discard_spawned_visit(frontend: &mut crate::app::frontend::state::FrontendState) {
    if !crate::app::shell_wol::spawned_visit(frontend.wol.as_ref()) {
        return;
    }
    let Some(mut visit) = frontend.wol.take() else {
        return;
    };
    if let Some(service) = visit.live_service.take() {
        frontend.online_cleanup.retire(service, Instant::now());
    }
}

pub(super) fn spawned_visit_mut(wol: Option<&mut WolVisit>) -> Option<&mut SpawnedVisit> {
    wol?.spawned.as_mut()
}

/// The external launch's match returned or failed: report it, then quit.
/// False when this launch has no spawned online visit.
pub(crate) fn end_spawned_match(wol: Option<&mut WolVisit>) -> bool {
    let Some(spawned) = spawned_visit_mut(wol) else {
        return false;
    };
    spawned
        .exit_deadline
        .get_or_insert_with(|| Instant::now() + EXIT_CLEANUP_TIMEOUT);
    true
}

/// `finish` with the result after a match, else `leave` for a failed
/// admission; the process quits on its acknowledgement or the deadline.
pub(super) fn poll_spawned_exit(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &PersistenceState,
    service: &mut LiveService,
) {
    let result = crate::app::App::match_result(frontend)
        .unwrap_or(crate::app::cncnet::ExternalMatchResult::Quit);
    let opponents_left = frontend.external_progress.opponents_left;
    let Some(spawned) = spawned_visit_mut(frontend.wol.as_mut()) else {
        return;
    };
    let Some(deadline) = spawned.exit_deadline.filter(|_| !spawned.quit) else {
        return;
    };
    let now = Instant::now();
    if !std::mem::replace(&mut spawned.cleanup_begun, true) {
        if service.spawned_watch.is_some() {
            // A watch is no match of this player's: it finishes and leaves
            // nothing (docs/spawn.md "Watching").
        } else if service.match_id.is_some() {
            service.finish_result = Some(result.as_str());
            service.finish_opponents_left = opponents_left;
            service.begin_finish(now);
        } else {
            service.begin_leave(now);
        }
    }
    if (service.finishing.is_none() && service.leaving.is_none()) || now >= deadline {
        spawned.quit = true;
        crate::app::App::start_quit_cascade(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
        );
    }
}

/// A returning player and a spectator have the match's timing already. The
/// spectator stays in the watch wait until its local start really succeeded.
fn enter_restored_start(
    state: &mut AppState,
    service: &mut LiveService,
    match_id: String,
    pings: &[i32],
) -> Result<()> {
    crate::app::lan_service::mark_online_restore(&mut state.match_state)?;
    let start = enter_timed_start(state, service, match_id, pings);
    if service.watching.is_none() {
        return start;
    }
    match start {
        Ok(()) => {
            super::lobby::watch_admitted(&mut state.frontend, &mut state.match_state.session);
        }
        Err(error) => {
            log::warn!("Could not start the watched match: {error:#}");
            fail_watch(
                &mut state.frontend,
                &state.platform.window,
                &mut state.match_state,
                &state.process_assets,
                service,
                "VERA:WatchCouldNotStart",
            );
        }
    }
    Ok(())
}

pub(super) fn poll_relay(state: &mut AppState, service: &mut LiveService) -> Result<()> {
    let mut commands = Vec::new();
    // The caller fails the start first once its deadline passes
    // (`LiveService::expired_admission`).
    let Some(pending) = service.pending.as_mut() else {
        return Ok(());
    };
    if let Some(transport) = pending.transport.as_mut() {
        let now = crate::app::match_runtime::frame_pacer::frame_bucket(
            crate::app::match_runtime::frame_pacer::wall_clock_ms(),
        );
        // Only the deadline fails admission. A receive error such as Windows'
        // 10054 (ICMP port unreachable while the relay restarts) or a passing
        // ENOBUFS/ENETUNREACH clears while the handshake keeps retrying.
        transport.service(now);
        if transport.relay_ready() {
            let transport = pending.transport.take().expect("ready pending relay");
            crate::app::lan_service::install_online_transport(
                &mut state.match_state,
                transport,
                &pending.host_name,
            )?;
            pending.relay_acknowledged = true;
            let match_id = pending.match_id.clone();
            // A returning player, or a watcher: the match is under way.
            if let Some(pings) = pending.resume_pings.take() {
                return enter_restored_start(state, service, match_id, &pings);
            }
            commands.push(("relay_ready", json!({"match_id":match_id})));
        }
    }
    if pending.probes_started && !pending.latencies_reported {
        if let Some(pings) = crate::app::lan_service::online_peer_round_trips(&state.match_state) {
            let pings: Vec<_> = pings.into_iter().map(|(source_index, round_trip_ms)|
                json!({"source_index":source_index,"round_trip_ms":round_trip_ms})).collect();
            let match_id = pending.match_id.clone();
            pending.latencies_reported = true;
            commands.push(("latency_ready", json!({"match_id":match_id,"pings":pings})));
        }
    }
    for (operation, payload) in commands {
        service.send_report(operation, payload, Instant::now());
    }
    service.poll_report(Instant::now());
    Ok(())
}
