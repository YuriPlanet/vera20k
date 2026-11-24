//! The gameplay transport boundary (native 697B70): moving the setup socket into
//! the live command owner, returning it afterwards, and sending raw bytes to sources.

use crate::app::match_runtime::sim_host::SimWrite;
use crate::net::global::GlobalDestination;
use crate::net::live_commands::LiveCommandMode;
use crate::net::reliable::RetryTiming;
use crate::net::udp::LanUdpTransport;

use super::{AppState, SessionMode, direct, gameplay_connections};

/// Session697B70 binds source nodes to the actual constructed House table.
/// The same physical transport used for discovery/loading enters the command
/// owner; no private peer is inferred from a colour or UI row.
pub(crate) fn install_gameplay(state: &mut AppState) -> anyhow::Result<()> {
    if !matches!(
        state.match_state.session_mode(),
        SessionMode::Lan | SessionMode::Wol
    ) || (state.platform.capture_client_size.is_some()
        && state.match_state.lan_service.is_none())
    {
        return Ok(());
    }
    if state.match_state.session_mode() == SessionMode::Wol {
        anyhow::ensure!(
            state
                .match_state
                .lan_service
                .as_ref()
                .and_then(|service| service.transport.as_ref())
                .is_some_and(LanUdpTransport::relay_ready),
            "online startup requires a connected relay"
        );
    }
    anyhow::ensure!(
        state.match_state.lan_service.is_some(),
        "LAN transport was not opened"
    );
    // VERA: the rules digest leaves the original game's terrain files out. A
    // match whose map names a tile the original game lacks read this game
    // folder's, so its start names them (`ProcessAssets::match_terrain_digest`).
    // Asked before the agreement, which loads a theater for a seed's map.
    let terrain = state
        .process_assets
        .match_terrain_digest()
        .map_err(anyhow::Error::msg)?;
    // VERA: the start every player's game certifies names its build and
    // rules too, so a launcher's relay match, which no lobby compared,
    // refuses games that would desync.
    let mut multiplayer = super::shell_network_setup::agreement::local(state)?;
    // VERA: and whether Ares plays in the match as it loaded, which the
    // room's digest leaves to the start (`agreement::start_rules`).
    let rules = &state
        .match_state
        .sim_runtime
        .runtime()
        .ok_or_else(|| anyhow::anyhow!("LAN startup has no constructed world"))?
        .resources
        .rules;
    multiplayer.rules_digest = super::shell_network_setup::agreement::start_rules(
        multiplayer.rules_digest,
        terrain.as_deref(),
        rules,
    );
    // VERA: and what the simulation took from this game's picture and model
    // files, which no room compares (`RuleSet::file_inputs_hash`): a
    // building's unfolding or an animation's length that differs between two
    // games would take their battles apart in mid-match.
    let file_inputs = rules.file_inputs_hash();
    // The refusal's box names three files; the log names them all.
    let picture_files = super::shell_network_setup::agreement::match_picture_files(state);
    if !picture_files.is_empty() {
        log::info!(
            "Game files that change what this start certifies of pictures and models: {}",
            picture_files.join(", ")
        );
    }
    super::shell_network_setup::agreement::log_rules_parts(state);
    let sim = &state
        .match_state
        .sim_runtime
        .runtime()
        .ok_or_else(|| anyhow::anyhow!("LAN startup has no constructed world"))?
        .simulation;
    let startup_transport = state
        .match_state
        .lan_service
        .as_ref()
        .and_then(|service| service.transport.as_ref())
        .ok_or_else(|| anyhow::anyhow!("LAN startup transport is already owned by a match"))?;
    let (local_house, seed, connections) = gameplay_connections(
        &state.match_state.session,
        sim,
        startup_transport.retry_timing(),
    )?;
    let host_house = if state.match_state.session.host() {
        local_house
    } else {
        let host_name = state
            .match_state
            .lan_service
            .as_ref()
            .and_then(|service| service.admitted_host.as_ref().map(|host| host.0.as_str()))
            .ok_or_else(|| anyhow::anyhow!("gameplay has no admitted host identity"))?;
        let source = state
            .match_state
            .session
            .network_setup()
            .and_then(|launch| {
                launch
                    .humans
                    .iter()
                    .position(|human| human.name.eq_ignore_ascii_case(host_name))
            })
            .ok_or_else(|| anyhow::anyhow!("host is absent from the admitted roster"))?;
        i8::try_from(
            super::match_runtime::network_match::source_house(
                &state.match_state.session,
                sim,
                source,
            )
            .ok_or_else(|| anyhow::anyhow!("host has no constructed House"))?,
        )
        .map_err(|_| anyhow::anyhow!("host House index cannot be encoded by the command channel"))?
    };
    // The loading wait may already have retired a source684596. Initialize
    // timing from MatchSession's accepted connected roster before the first
    // shared frame; `gameplay_connections` built the private connections from
    // the same roster.
    let source_houses = state
        .match_state
        .session
        .network_connected_source_indices()
        .iter()
        .map(|&source| {
            let house = super::match_runtime::network_match::source_house(
                &state.match_state.session,
                sim,
                source,
            )
            .ok_or_else(|| anyhow::anyhow!("connected source has no constructed House"))?;
            i8::try_from(house)
                .map_err(|_| anyhow::anyhow!("connected House index cannot be encoded"))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let full_houses = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("gameplay has no frozen original admission"))?
        .humans
        .iter()
        .enumerate()
        .map(|(source, human)| {
            let row = if state.match_state.session.campaign_room().is_some() {
                state
                    .match_state
                    .session
                    .network_source_sender(source)
                    .and_then(|sender| u8::try_from(sender).ok())
            } else {
                human.raw_slot_index
            }
            .ok_or_else(|| anyhow::anyhow!("admitted human has no shared setup row"))?;
            let house = super::match_runtime::network_match::source_house(
                &state.match_state.session,
                sim,
                source,
            )
            .ok_or_else(|| anyhow::anyhow!("admitted source has no constructed House"))?;
            Ok((
                row,
                i8::try_from(house)
                    .map_err(|_| anyhow::anyhow!("original House cannot be encoded"))?,
            ))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let active_rows = state
        .match_state
        .session
        .network_connected_source_indices()
        .iter()
        .map(|&source| {
            let session = &state.match_state.session;
            let row = if session.campaign_room().is_some() {
                session
                    .network_source_sender(source)
                    .and_then(|sender| u8::try_from(sender).ok())
            } else {
                session
                    .network_setup()
                    .and_then(|launch| launch.humans.get(source))
                    .and_then(|human| human.raw_slot_index)
            };
            row.ok_or_else(|| anyhow::anyhow!("active human has no shared setup row"))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let mode = if state.match_state.session_mode() == SessionMode::Wol {
        LiveCommandMode::Wol
    } else {
        LiveCommandMode::Lan
    };
    let online_timing = if mode == LiveCommandMode::Wol {
        Some(
            state
                .match_state
                .lan_service
                .as_ref()
                .and_then(|service| service.online_timing)
                .ok_or_else(|| anyhow::anyhow!("online startup has no agreed measured timing"))?,
        )
    } else {
        None
    };
    // Freeze shared setup-row/House bindings and loading-survivor set exactly
    // once. The existing canonical world hash includes the installed shared
    // timing rows; no first probe, vote or local clock can replace this value.
    let ordering_clock =
        crate::net::live_commands::relay_clock(mode, startup_transport, full_houses.len())?;
    let frame_send_rate = ordering_clock.map_or_else(
        || {
            online_timing.map_or(const { std::num::NonZeroU8::new(5).unwrap() }, |timing| {
                timing.frame_send_rate
            })
        },
        |_| std::num::NonZeroU8::MIN,
    );
    let rate = std::num::NonZeroU32::from(frame_send_rate);
    let sim = &mut state
        .match_state
        .sim_runtime
        .write_sim(SimWrite::Network)
        .expect("constructed world")
        .simulation;
    sim.network_command_queues()
        .install_timing(host_house, frame_send_rate, source_houses);
    if let Some(clock) = ordering_clock {
        sim.network_command_queues()
            .configure_sequenced_clock(clock.tick_hz);
    }
    let publisher = sim.network_timing_host_house();
    let agreement = crate::net::live_commands::StartupAgreement::new(
        full_houses,
        active_rows,
        seed,
        sim.session()
            .network_max_ahead_frames()
            .expect("installed network MaxAhead"),
        sim.session()
            .network_command_timing()
            .expect("installed timing")
            .frames_per_second(),
        rate,
        sim.state_hash(),
        file_inputs,
        &multiplayer.version,
        &multiplayer.rules_digest,
    )?;
    // The lobby keeps a report of at most 64 start parts (`start_hash` in
    // docs/lobby-protocol.md) and drops a longer one whole.
    const _: () = {
        const fn parts<T, const N: usize>(_: fn(&T) -> [(&'static str, u64); N]) -> usize {
            N
        }
        assert!(
            parts(crate::net::live_commands::StartupAgreement::report_parts)
                + parts(crate::sim::world::Simulation::state_hash_owners)
                <= 64
        );
    };
    if ordering_clock.is_some() {
        let parts = agreement
            .report_parts()
            .into_iter()
            .chain(sim.state_hash_owners())
            .collect();
        state.match_state.network_quality.record_start_parts(parts);
    }
    if state.match_state.session_mode() == SessionMode::Lan {
        let service = state
            .match_state
            .lan_service
            .as_mut()
            .ok_or_else(super::closed_visit)?;
        let scope = service
            .setup_scope()
            .ok_or_else(|| anyhow::anyhow!("LAN gameplay has no final accepted scope"))?;
        service
            .transport
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("LAN gameplay lost its transport"))?
            .set_lan_scope(scope)?;
    }
    let mut transport = state
        .match_state
        .lan_service
        .as_mut()
        .ok_or_else(super::closed_visit)?
        .transport
        .take()
        .ok_or_else(|| anyhow::anyhow!("LAN startup transport is already owned by a match"))?;
    *transport.connections_mut() = connections;
    // MainPrepareSession52EA18/52EA4F selects these after CreateConnections.
    // Keep validation above transactional: a failed binding retains the socket.
    if online_timing.is_none() {
        let (private, global) =
            RetryTiming::lan_gameplay(|| transport.global().mean_response_time());
        transport.set_retry_timing(private, true);
        transport.global_mut().set_timing(global);
    }
    // LAN5DD2DF/5DD49F is independent of game speed. WOL794F6F uses
    // the same agreed timing owner; the manifest binds that initial cadence.
    let mut live = crate::net::live_commands::LiveCommandSession::new(
        agreement,
        transport,
        local_house,
        seed,
        rate,
        mode,
        ordering_clock,
    )?;
    if state
        .match_state
        .lan_service
        .as_ref()
        .is_some_and(|service| service.online_restore)
    {
        live.begin_match_restore();
    }
    if publisher == Some(local_house)
        && let Some(timing) = online_timing
    {
        live.configure_precalculated_timing(timing.precalc_max_ahead, timing.precalc_frame_rate);
    }
    state.match_state.live_network = Some(live);
    Ok(())
}

/// End private channels on leaving gameplay, retaining the LAN visit's same
/// socket pair for BB. It is dropped only when that visit itself ends.
pub(crate) fn return_transport(state: &mut AppState) {
    if let Some(report) = crate::app::match_runtime::network_quality::close(state) {
        crate::app::shell_wol::keep_network_report(state, &report);
    }
    let online = state.match_state.session_mode() == SessionMode::Wol;
    let Some(live) = state.match_state.live_network.take() else {
        return;
    };
    if live.watching()
        && let Ok(endpoint) = live.primary_addr()
    {
        state
            .match_state
            .session
            .disconnect_network_watcher(endpoint);
    }
    if let Some(service) = state.match_state.lan_service.as_mut() {
        let mut transport = live.into_transport();
        transport.connections_mut().reset();
        if online {
            transport.retire_relay();
        } else {
            transport.clear_lan_scope();
        }
        service.transport = Some(transport);
        service.clear_setup();
    }
}

pub(super) fn send_bytes(
    state: &mut AppState,
    bytes: &[u8],
    destination: GlobalDestination,
    reliable: bool,
) -> bool {
    if let Some(live) = state.match_state.live_network.as_mut() {
        live.send_global(bytes, reliable, destination)
    } else {
        state
            .match_state
            .lan_service
            .as_mut()
            .is_some_and(|service| service.send(bytes, destination, reliable))
    }
}

pub(super) fn send_sources(
    state: &mut AppState,
    bytes: &[u8],
    sources: impl IntoIterator<Item = usize>,
) {
    for source in sources {
        if let Some(endpoint) = state.match_state.session.network_endpoint(source) {
            send_bytes(state, bytes, direct(endpoint), true);
        }
    }
}
