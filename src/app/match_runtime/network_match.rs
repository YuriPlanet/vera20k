//! The in-match side of a LAN/WOL connection: a peer's departure
//! (`Execute_DoList`'s remote EXIT branch `0x0064C91B`) and
//! `Network__DestroyConnection 0x005DA750`, which the loading timeout and
//! the departure share.
//!
//! It also owns the in-match stall: `Wait_For_Players 0x00648710` (the
//! `0x00648067` call) and its connection dialog 0xEA (proc `0x0064AE50`).
//!
//! The live LAN command owner supplies its reached wait and peer progress.
//! Controlled service events remain available for diagnostics. Both paths
//! use the same dialog, vote and connection-removal owners here.

use std::collections::{BTreeMap, VecDeque};

use crate::app::AppState;
use crate::app::match_runtime::session::{MatchSession, SessionMode, WallBucketTimer};
use crate::net::live_commands::{LiveCommandMode, LiveWaitInfo, LiveWaitKind};
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;
use crate::ui::lan_shell::LanChatLine;
use crate::ui::shell::connection::{ConnectionPage, ConnectionRow};

/// The simulation owns FPS/MaxAhead; the live channel owns sampled peer lag.
/// Presentation and the frame pacer consume the same combined interval.
pub(crate) fn frame_budget(state: &AppState) -> Option<(u16, u32)> {
    if let Some(fps) = state
        .match_state
        .live_network
        .as_ref()
        .and_then(|live| live.fixed_frames_per_second())
    {
        return Some((fps, 0));
    }
    let session = state.match_state.sim_runtime.as_ref()?.simulation.session();
    let timing = session.network_command_timing()?;
    let delay = state.match_state.live_network.as_ref().map_or(0, |live| {
        live.pacing_delay_millis(session.network_max_ahead_frames().unwrap_or(0))
    });
    Some((timing.frames_per_second(), delay))
}

/// DestroyConnection's error argument (`EDX`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectionError {
    /// 0: the peer left (global Departure or executed EXIT).
    /// `TXT_LEFT_GAME`, nothing queued by DestroyConnection itself.
    Left,
    /// 1: the connection was lost. `TXT_CONNECTION_LOST`; Flag_To_Die at
    /// frame 0, REMOVEPLAYER otherwise. Installed VERA timing also needs
    /// synchronized source retirement when a live frame-zero vote removes it.
    Lost,
}

impl ConnectionError {
    fn code(self) -> i32 {
        match self {
            Self::Left => 0,
            Self::Lost => 1,
        }
    }

    fn csf_key(self) -> &'static str {
        match self {
            Self::Left => "TXT_LEFT_GAME",
            Self::Lost => "TXT_CONNECTION_LOST",
        }
    }
}

/// The line DestroyConnection posts for a destroyed connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConnectionDestroyed {
    pub(crate) owner: InternedId,
    pub(crate) key: &'static str,
}

/// What a controlled peer did during the match, decoded from its packets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NetworkMatchEvent {
    /// Participant `source_index` sent EXIT: its event executes at the
    /// current frame as if received.
    PeerExit { source_index: usize },
    /// Participant `source_index`'s frames stop arriving after the current
    /// frame: the next frame waits for it.
    PeerStalled { source_index: usize },
    /// Its frames arrive again.
    PeerResumed { source_index: usize },
    /// Kick proposal packet `0x1C` from `voter` against `target`, both
    /// participant indices; queued like `[0x00A8BB28]`.
    KickVote { voter: usize, target: usize },
    /// Participant `source_index`'s EVENT_ALLY toward the House at
    /// registration index `house_index`: it executes at the current frame
    /// as if received.
    PeerAlly {
        source_index: usize,
        house_index: i32,
    },
    /// Chat packet `0x0C` from participant `source_index`.
    Chat { source_index: usize, text: String },
    /// Taunt packet `0x1D`; the receiver reads only its taunt byte.
    Taunt { byte: u8 },
}

/// `Network__DestroyConnection 0x005DA750` for participant `source_index`.
/// Returns early, with no line, when the House is not human (`0x005DA78F`);
/// a participant already disconnected has no connection record, so its
/// caller never reaches here twice. The line itself is posted by
/// [`post_connection_line`] once the simulation is installed.
pub(crate) fn destroy_connection(
    session: &mut MatchSession,
    sim: &mut Simulation,
    source_index: usize,
    error: ConnectionError,
    live: Option<&mut crate::net::live_commands::LiveCommandSession>,
) -> Option<ConnectionDestroyed> {
    let owner = sim
        .interner()
        .get(&session.network_source_owner(source_index)?)?;
    if !sim
        .houses()
        .get(&owner)
        .is_some_and(|house| house.is_human())
    {
        return None;
    }
    let live = match live {
        Some(live) => Some((
            live,
            i8::try_from(source_house(session, sim, source_index)?).ok()?,
        )),
        None => None,
    };
    // VERA startup agreement: native649148 tallies a mutable node count,
    // and5DA86A..875 flags frame-zero loss locally. An installed match cannot
    // change its certified set/world before release. Abort that startup and
    // retain the same shared state; before-install loading stays native.
    let startup_pending = live
        .as_ref()
        .is_some_and(|(live, _)| live.startup_pending());
    // In relay-ordered play the committed departure already carried the shared
    // EVENT_REMOVEPLAYER; only this client's connection and line remain.
    let sequenced = live
        .as_ref()
        .is_some_and(|(live, _)| live.uses_sequenced_lockstep());
    if !session.disconnect_network_participant(source_index, error.code()) {
        return None;
    }
    log::warn!(
        "Network connection destroyed: source={source_index}, house={:?}, reason={}, native_frame={}, startup_pending={startup_pending}",
        sim.houses().index_of(owner),
        error.code(),
        sim.session().binary_frame(),
    );
    if let Some((live, house)) = live {
        log_live_progress(session, sim, live, "destroy_connection");
        //5DA85D removes the private connection after the Human guard, before
        //the reason-specific effects. Progress shares that connection's life.
        live.remove_peer(house);
    }
    if error == ConnectionError::Lost && !startup_pending && !sequenced {
        let frame = sim.session().binary_frame();
        if frame == 0 && sim.session().network_command_timing().is_none() {
            sim.flag_house_to_die(owner);
        }
        // VERA UDP/relay adaptation: native5DA86A..5DA87A only flags at zero.
        // A vote can remove a source after live timing was installed but
        // before the initial frame commits. Reuse the existing shared event
        // so its timing row retires through DoList, never a local peer list.
        if frame != 0 || sim.session().network_command_timing().is_some() {
            // EVENT_REMOVEPLAYER from PlayerPtr with the House's index
            // (0x005DA898..0x005DA8F0); it kills the House on every peer.
            let local = sim.session().current_house();
            let index = sim.houses().index_of(owner);
            if let (Some(local), Some(index)) = (local, index) {
                let local = sim.interner().resolve(local).to_string();
                crate::app::input::commands::schedule_command_in_sim(
                    sim,
                    &local,
                    crate::sim::command::Command::RemovePlayer {
                        house_index: index as i32,
                    },
                );
            }
        }
    }
    // 0x005DA8F7..0x005DA90F: a WOL tournament game kills the House of a
    // player who left (error 0). An executed EXIT ran this in the sim's EXIT
    // block. RESIDUAL: mode 4's ladder-stats call (0x005DA921..) is backend.
    if !startup_pending
        && error == ConnectionError::Left
        && sim.session().wol_tournament_game()
        && !sim
            .houses()
            .get(&owner)
            .is_some_and(|house| house.left_game())
    {
        sim.flag_house_to_die(owner);
    }
    Some(ConnectionDestroyed {
        owner,
        key: error.csf_key(),
    })
}

/// ProcessGlobalPackets48D81A identifies a type10 Departure by the active
/// private connection's exact endpoint (540FA0/541000/53EE70), not its name.
pub(crate) fn receive_departure(
    session: &mut MatchSession,
    sim: &mut Simulation,
    live: &mut crate::net::live_commands::LiveCommandSession,
    wait: &mut NetworkMatchWait,
    endpoint: std::net::SocketAddrV4,
) -> Option<ConnectionDestroyed> {
    // In sequenced play only the relay's committed departure can mutate
    // gameplay membership. Global shell traffic has no simulation authority.
    if live.uses_sequenced_lockstep() {
        return None;
    }
    let house = live.connections().peer_id(endpoint)?;
    let source = house_source(session, sim, house)?;
    let destroyed = destroy_connection(session, sim, source, ConnectionError::Left, Some(live))?;
    wait.stalled.retain(|peer| peer.source_index != source);
    Some(destroyed)
}

/// DestroyConnection's line (`0x005DA7A9..0x005DA83E`): the template with the
/// House's UI name, in its HouseType's `Color=` scheme (`[[House+0x34]+0xC0]`).
pub(crate) fn post_connection_line(state: &mut AppState, destroyed: ConnectionDestroyed) {
    crate::app::input::messages::post_house_line(
        state,
        destroyed.key,
        destroyed.owner,
        crate::app::input::messages::HouseLineColor::Country,
    );
}

/// The executed EXIT events of remote senders this frame: each runs
/// DestroyConnection(house, 0) (`0x0064C9B1`). Remote sources the relay ended
/// for silence run DestroyConnection(house, 1) after their EVENT_REMOVEPLAYER.
pub(crate) fn drain_departures(state: &mut AppState) {
    let Some(runtime) = state.match_state.sim_runtime.as_mut() else {
        return;
    };
    let departed = runtime.simulation.take_departed_owners();
    for owner in departed {
        let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
            return;
        };
        let sim = &runtime.simulation;
        let name = sim.interner().resolve(owner).to_string();
        let Some(source) = state.match_state.session.network_owner_source(&name) else {
            continue;
        };
        kick(state, source, ConnectionError::Left);
    }
    let lost = state
        .match_state
        .live_network
        .as_mut()
        .map(|live| live.take_lost_houses())
        .unwrap_or_default();
    for house in lost {
        let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
            return;
        };
        let Some(source) = house_source(
            &state.match_state.session,
            &runtime.simulation,
            i32::from(house),
        ) else {
            continue;
        };
        kick(state, source, ConnectionError::Lost);
    }
}

/// Deliver one controlled in-match event.
pub(crate) fn service_event(state: &mut AppState, event: NetworkMatchEvent) -> anyhow::Result<()> {
    match event {
        NetworkMatchEvent::PeerExit { source_index } => {
            let owner = state
                .match_state
                .session
                .network_source_owner(source_index)
                .ok_or_else(|| anyhow::anyhow!("no participant {source_index}"))?;
            anyhow::ensure!(
                state
                    .match_state
                    .session
                    .network_connected_source_indices()
                    .contains(&source_index),
                "participant {source_index} is not connected"
            );
            let runtime = state
                .match_state
                .sim_runtime
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("no running match"))?;
            let sim = &mut runtime.simulation;
            let owner = sim
                .interner()
                .get(&owner)
                .ok_or_else(|| anyhow::anyhow!("received event has no registered owner"))?;
            sim.queue_command(crate::sim::command::CommandEnvelope::new(
                owner,
                sim.session().tick(),
                crate::sim::command::Command::ExitMatch,
            ));
        }
        NetworkMatchEvent::PeerStalled { source_index } => {
            let session = &state.match_state.session;
            anyhow::ensure!(
                session
                    .network_connected_source_indices()
                    .contains(&source_index)
                    && local_source(session) != Some(source_index),
                "participant {source_index} is not a connected peer"
            );
            let frame: u64 = state
                .match_state
                .sim_runtime
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("no running match"))?
                .simulation
                .session()
                .binary_frame()
                .into();
            let wait = &mut state.match_state.network_wait;
            if !wait
                .stalled
                .iter()
                .any(|peer| peer.source_index == source_index)
            {
                wait.stalled.push(StalledPeer {
                    source_index,
                    frame,
                });
            }
        }
        NetworkMatchEvent::PeerResumed { source_index } => {
            state
                .match_state
                .network_wait
                .stalled
                .retain(|peer| peer.source_index != source_index);
        }
        NetworkMatchEvent::KickVote { voter, target } => {
            state.match_state.network_wait.queue_vote(voter, target);
        }
        NetworkMatchEvent::PeerAlly {
            source_index,
            house_index,
        } => {
            let owner = state
                .match_state
                .session
                .network_source_owner(source_index)
                .ok_or_else(|| anyhow::anyhow!("no participant {source_index}"))?;
            let runtime = state
                .match_state
                .sim_runtime
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("no running match"))?;
            let sim = &mut runtime.simulation;
            let owner = sim
                .interner()
                .get(&owner)
                .ok_or_else(|| anyhow::anyhow!("received event has no registered owner"))?;
            sim.queue_command(crate::sim::command::CommandEnvelope::new(
                owner,
                sim.session().tick(),
                crate::sim::command::Command::Ally { house_index },
            ));
        }
        NetworkMatchEvent::Chat { source_index, text } => {
            crate::app::input::chat::receive(state, source_index, &text);
        }
        NetworkMatchEvent::Taunt { byte } => crate::app::input::taunt::receive(state, byte),
    }
    Ok(())
}

/// A connected peer whose frames stopped after `frame`. Its record keeps
/// that last frame while the others run ahead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StalledPeer {
    source_index: usize,
    frame: u64,
}

/// The in-match rows of the `Wait_For_Players` timing table
/// `0x00837340 + 16·GameMode`: +8 dialog delay (a floor under
/// `3·latency`, which is transport) and +C timeout.
fn wait_periods(mode: SessionMode) -> (u32, u32) {
    match mode {
        SessionMode::Wol => (720, 7200),
        _ => (420, 2400),
    }
}

/// ProcessNetwork's original call arguments: initial647919..647954;
/// ordinary64800E..648067. Both index GameMode's row at837340: WOL's
/// +4/+8/+C are1800/720/7200, LAN's are900/420/2400. Source: gamemd.exe.
pub(crate) fn live_wait_periods(info: LiveWaitInfo) -> (u32, u32) {
    let (initial, floor, timeout) = match info.mode {
        LiveCommandMode::Lan => (900, 420, 2400),
        LiveCommandMode::Wol => (1800, 720, 7200),
    };
    let dialog = match info.kind {
        LiveWaitKind::Initial => initial,
        LiveWaitKind::Ordinary => {
            (info.response_time_at_entry.wrapping_mul(3) as i32).max(floor) as u32
        }
    };
    (dialog, timeout)
}

/// One `Wait_For_Players` call.
#[derive(Debug, Clone)]
struct ConnectionWait {
    /// `local_6c0`: the entry bucket; `[0x00AFA418]` is the time since.
    entry: u32,
    ///None selects the established controlled-service path.
    live_kind: Option<LiveWaitKind>,
    periods: (u32, u32),
    /// `local_6f4`: opens 0xEA the first time it expires.
    dialog_timer: WallBucketTimer,
    /// `local_700`: kicks the lowest record when it expires.
    timeout_timer: WallBucketTimer,
    /// `[0x00B04898]`: the last countdown second written.
    seconds: u32,
    /// `[0x00A8BB40]`/`[0x00A8BB60]`: voters by target participant.
    votes: BTreeMap<usize, Vec<usize>>,
    /// The result cell `[0x00B04468] == 2` (Quit Game).
    leave: bool,
    /// The open dialog (`bVar2`).
    page: Option<ConnectionPage>,
    ///Derived paint snapshot only. Peer timing and frame authority stay in
    ///CommandExchange; this cache refreshes with the dialog's repaint.
    bars: BTreeMap<usize, (u32, u32)>,
}

impl ConnectionWait {
    fn new(entry: u32, periods: (u32, u32), live_kind: Option<LiveWaitKind>) -> Self {
        Self {
            entry,
            live_kind,
            periods,
            dialog_timer: WallBucketTimer::start(entry, periods.0),
            timeout_timer: WallBucketTimer::start(entry, periods.1),
            seconds: 0,
            votes: BTreeMap::new(),
            leave: false,
            page: None,
            bars: BTreeMap::new(),
        }
    }

    fn restart_timers(&mut self, now: u32) {
        self.dialog_timer = WallBucketTimer::start(now, self.periods.0);
        self.timeout_timer = WallBucketTimer::start(now, self.periods.1);
    }
}

/// The match's connection-wait owner: stalled peers, received kick
/// proposals and the current wait.
#[derive(Debug, Clone, Default)]
pub(crate) struct NetworkMatchWait {
    stalled: Vec<StalledPeer>,
    /// `[0x00A8BB28]`: one is handled per dialog pass; opening clears them.
    received_votes: VecDeque<(usize, usize)>,
    wait: Option<ConnectionWait>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitTimeout {
    InitialFailure,
    DropPeer,
}

impl NetworkMatchWait {
    /// One dialog pass consumes one queued vote, so a repeated proposal is
    /// queued once: a peer resending its vote cannot push others' back.
    fn queue_vote(&mut self, voter: usize, target: usize) {
        if !self.received_votes.contains(&(voter, target)) {
            self.received_votes.push_back((voter, target));
        }
    }

    fn observe_live_wait(&mut self, info: LiveWaitInfo) {
        if self
            .wait
            .as_ref()
            .is_none_or(|wait| wait.live_kind != Some(info.kind) || wait.entry != info.entered_at)
        {
            self.wait = Some(ConnectionWait::new(
                info.entered_at,
                live_wait_periods(info),
                Some(info.kind),
            ));
        }
    }

    fn timeout_action(&self, now: u32) -> Option<WaitTimeout> {
        let wait = self.wait.as_ref()?;
        (wait.timeout_timer.remaining(now) == 0).then_some(
            if wait.live_kind == Some(LiveWaitKind::Initial) {
                WaitTimeout::InitialFailure
            } else {
                WaitTimeout::DropPeer
            },
        )
    }

    fn interrupt_initial_wait(&mut self, sim: &mut Simulation) -> bool {
        self.wait = None;
        sim.interrupt_network_wait(crate::sim::world::NetworkInterruption::ConnectionFailed)
    }

    /// A wait is running: no simulation frame may commit.
    pub(crate) fn waiting(&self) -> bool {
        self.wait.is_some()
    }

    /// 0xEA is open: the tactical view takes no input and is not drawn.
    pub(crate) fn dialog_open(&self) -> bool {
        self.page().is_some()
    }

    pub(crate) fn page(&self) -> Option<&ConnectionPage> {
        self.wait.as_ref().and_then(|wait| wait.page.as_ref())
    }

    pub(crate) fn page_mut(&mut self) -> Option<&mut ConnectionPage> {
        self.wait.as_mut().and_then(|wait| wait.page.as_mut())
    }

    ///The last painted64A7B0 lag. Live values derive from FRAMESYNC timing;
    ///controlled-service fixtures retain their explicit stalled-peer model.
    pub(crate) fn bar_lag(&self, source_index: usize) -> u32 {
        if let Some(wait) = self.wait.as_ref().filter(|wait| wait.live_kind.is_some()) {
            return wait.bars.get(&source_index).map_or(0, |bar| bar.0);
        }
        let Some(page) = self.page() else {
            return 0;
        };
        if self
            .stalled
            .iter()
            .any(|peer| peer.source_index == source_index)
        {
            page.painted_elapsed.min(u64::from(u32::MAX)) as u32
        } else {
            0
        }
    }

    pub(crate) fn bar_timing(&self, source_index: usize) -> (u32, u32) {
        if let Some(wait) = self.wait.as_ref().filter(|wait| wait.live_kind.is_some()) {
            return wait.bars.get(&source_index).copied().unwrap_or((0, 0));
        }
        let lag = self.bar_lag(source_index);
        (lag, (u64::from(lag) * 16 * 60 / 1000) as u32)
    }
}

/// What one runtime pass of the wait decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WaitPass {
    /// No wait, or it ended this pass: frames run.
    Running,
    /// Frames wait for a peer.
    Waiting,
    /// Quit Game: result 8, GameActive cleared without an EXIT event
    /// (`0x0064806C..0x006482A6`).
    Leave,
}

pub(crate) fn local_source(session: &MatchSession) -> Option<usize> {
    session
        .network_setup()
        .map(|launch| launch.local_player_index)
}

/// The connection nodes `[0x00A8DA78]` (count `[0x00A8DA84]`): the local
/// player first, then the connected peers in participant order.
pub(crate) fn nodes(session: &MatchSession) -> Vec<usize> {
    let Some(local) = local_source(session) else {
        return Vec::new();
    };
    std::iter::once(local)
        .chain(
            session
                .network_connected_source_indices()
                .iter()
                .copied()
                .filter(|index| *index != local),
        )
        .collect()
}

pub(crate) fn participant_name(session: &MatchSession, source_index: usize) -> String {
    session
        .network_setup()
        .and_then(|launch| launch.humans.get(source_index))
        .map(|human| human.name.clone())
        .unwrap_or_default()
}

pub(crate) fn source_house(session: &MatchSession, sim: &Simulation, source: usize) -> Option<i32> {
    let name = session.network_source_owner(source)?;
    let owner = sim.interner().get(&name)?;
    sim.houses()
        .order()
        .iter()
        .position(|candidate| *candidate == owner)
        .and_then(|index| i32::try_from(index).ok())
}

pub(crate) fn house_source(session: &MatchSession, sim: &Simulation, house: i32) -> Option<usize> {
    let owner = sim.houses().order().get(usize::try_from(house).ok()?)?;
    session.network_owner_source(sim.interner().resolve(*owner))
}

/// Numeric projections only: never format transports, endpoints, errors or
/// packet bodies. The peer vector is bounded by the admitted match roster.
/// UDP counters cover the retained aggregate since the last live poll, not
/// lifetime traffic; the existing report consumer/reset semantics are unchanged.
pub(crate) fn log_live_progress(
    session: &MatchSession,
    sim: &Simulation,
    live: &crate::net::live_commands::LiveCommandSession,
    point: &'static str,
) {
    if !log::log_enabled!(log::Level::Info) {
        return;
    }
    let peers: Vec<_> = live
        .peers()
        .iter()
        .map(|peer| {
            let house = peer.house_id();
            (
                house_source(session, sim, i32::from(house)),
                house,
                peer.frame(),
                peer.advertised_commands(),
                peer.received_commands(),
                live.queued_send_count(house),
                live.peer_response_time(house),
            )
        })
        .collect();
    let report = live.service_report();
    let udp_counts = report.map(|report| {
        (
            report.received_datagrams,
            report.sent_datagrams,
            report.malformed_datagrams,
            report.rejected_connections,
            report.rejected_global,
            report.discarded_global_sends,
            report.send_would_block,
            report.failed_peer,
        )
    });
    let io_failure = report.and_then(|report| report.first_io_error.as_ref());
    log::info!(
        "Network progress: point={point}, local_source={:?}, native_frame={}, phase={:?}, wait={:?}, sent_commands={}, retained_send={:?}, frame_wait={:?}, local_commands={}, synchronized_commands={}, mirror_allowance={}, udp_poll_received_sent_malformed_private_rejected_global_rejected_discarded_sends_would_block_failed_peer={udp_counts:?}, udp_poll_io_failure={}, udp_poll_os_error={:?}, peers_source_house_frame_advertised_received_send_queued_rtt_buckets={peers:?}",
        local_source(session),
        sim.session().binary_frame(),
        live.phase(),
        live.wait_info(),
        live.sent_commands(),
        live.retained_send(),
        live.frame_wait(),
        sim.pending_local_command_count(),
        sim.synchronized_network_command_count(),
        sim.network_mirror_allowance(),
        io_failure.is_some(),
        io_failure.and_then(|failure| failure.error.raw_os_error()),
    );
}

///Global NET_KICK proposal64AAE0: a fixed455-byte packet, independent of
///EventClass FRAMEINFO (which happens to share opcode0x1C).
pub(crate) fn encode_kick_proposal(
    session: &MatchSession,
    sim: &Simulation,
    target_source: usize,
) -> Option<[u8; 455]> {
    let local = local_source(session)?;
    let voter = source_house(session, sim, local)?;
    let target = source_house(session, sim, target_source)?;
    let name = crate::util::native_string::narrow_utf16_c_string(
        participant_name(session, local).encode_utf16(),
    );
    let mut packet = [0; 455];
    packet[..4].copy_from_slice(&0x1c_u32.to_le_bytes());
    //64ABE7..64ABF6:7350C0 followed by strncpy(...,20). Unread padding
    //is initialized here; only the two House fields feed64AD10.
    let length = name.len().min(20);
    packet[4..4 + length].copy_from_slice(&name[..length]);
    packet[0x2f..0x33].copy_from_slice(&target.to_le_bytes());
    packet[0x33..0x37].copy_from_slice(&voter.to_le_bytes());
    Some(packet)
}

///48D58A/64AC90 queue a global proposal;649120 later reads target+2F and
///voter+33. The shared global demux owns product/source filtering and calls
///service_event with this result; this function never drains global traffic.
pub(crate) fn decode_kick_proposal(
    session: &MatchSession,
    sim: &Simulation,
    sender: std::net::SocketAddrV4,
    packet: &[u8],
) -> Option<NetworkMatchEvent> {
    if packet.len() != 455 || packet[..4] != 0x1c_u32.to_le_bytes() {
        return None;
    }
    let target = i32::from_le_bytes(packet[0x2f..0x33].try_into().ok()?);
    let voter = house_source(
        session,
        sim,
        i32::from_le_bytes(packet[0x33..0x37].try_into().ok()?),
    )?;
    // VERA transport security: 649120 trusts the voter field, so one peer
    // could vote as every other node and reach the nodeCount - 1 kick
    // tally alone. Only the sender's own authenticated endpoint votes.
    if session.network_endpoint_source(sender) != Some(voter) {
        return None;
    }
    Some(NetworkMatchEvent::KickVote {
        voter,
        target: house_source(session, sim, target)?,
    })
}

fn capture_live_bars(state: &mut AppState, elapsed: u32, now_millis: u32) {
    let (Some(live), Some(runtime)) = (
        state.match_state.live_network.as_ref(),
        state.match_state.sim_runtime.as_ref(),
    ) else {
        return;
    };
    let bars = live
        .peers()
        .iter()
        .filter_map(|peer| {
            let source = house_source(
                &state.match_state.session,
                &runtime.simulation,
                i32::from(peer.house_id()),
            )?;
            Some((source, peer.bar_timing(elapsed, now_millis)))
        })
        .collect();
    if let Some(wait) = &mut state.match_state.network_wait.wait {
        wait.bars = bars;
    }
}

fn csf(state: &AppState, key: &str) -> String {
    crate::app::App::csf_label(state, key, key)
}

/// Kick_Player_Now `0x0064B240`: drop the record and run DestroyConnection.
/// RESIDUAL: its stats-only House+0x208/+0x247 writes and the WOL
/// statistics block are not ported.
fn kick(state: &mut AppState, source_index: usize, error: ConnectionError) {
    state
        .match_state
        .network_wait
        .stalled
        .retain(|peer| peer.source_index != source_index);
    let Some(runtime) = state.match_state.sim_runtime.as_mut() else {
        return;
    };
    if let Some(destroyed) = destroy_connection(
        &mut state.match_state.session,
        &mut runtime.simulation,
        source_index,
        error,
        state.match_state.live_network.as_mut(),
    ) {
        post_connection_line(state, destroyed);
    }
}

/// The list's wrap width and visible rows at the current shell size.
fn list_metrics(state: &AppState) -> (i32, usize) {
    let (width, height) = crate::app::frontend::skirmish_shell_render::in_game_shell_size(state);
    let rect = crate::ui::shell::connection::list_rect(width, height);
    let page = state.match_state.network_wait.page();
    let (count, top) = page.map_or((0, 0), |page| (page.lines.len(), page.list.top_index));
    let geometry = crate::ui::shell::list::ShellListGeometry::new(
        rect,
        count,
        top,
        crate::ui::shell::list::ListMetrics::RETAIL,
    );
    (rect.w, geometry.visible_rows)
}

/// `0x005E2820` into list 0x651 with colour −1.
fn append_line(state: &mut AppState, text: &str) {
    let (width, visible) = list_metrics(state);
    let font = &state.renderer.bit_font;
    if let Some(page) = state.match_state.network_wait.page_mut() {
        crate::ui::lan_shell::append_list_message(
            &mut page.lines,
            &mut page.list,
            LanChatLine {
                text: text.encode_utf16().collect(),
                color: u32::MAX,
            },
            width,
            visible,
            |units| font.text_width(&String::from_utf16_lossy(units)) as i32,
        );
    }
}

/// The line tail of 0x0064AAE0 and 0x0064AD10.
fn append_line_and_show_last(state: &mut AppState, text: &str) {
    append_line(state, text);
    let (_, visible) = list_metrics(state);
    if let Some(page) = state.match_state.network_wait.page_mut() {
        page.trim_and_show_last(visible);
    }
}

/// The record with the lowest last frame (`0x00648E3C`, `0x0064B410`):
/// peers whose frames still arrive are ahead of every stalled one.
fn lowest_record(state: &AppState) -> Option<usize> {
    if let Some(live) = state.match_state.live_network.as_ref() {
        let house = live.lowest_frame_peer()?.house_id();
        return house_source(
            &state.match_state.session,
            &state.match_state.sim_runtime.as_ref()?.simulation,
            i32::from(house),
        );
    }
    lowest_record_of(
        &nodes(&state.match_state.session),
        &state.match_state.network_wait.stalled,
    )
}

/// Over the records (every node but the local one, in node order); the
/// first of equal frames wins.
fn lowest_record_of(nodes: &[usize], stalled: &[StalledPeer]) -> Option<usize> {
    let mut best: Option<(u64, usize)> = None;
    for &source in nodes.iter().skip(1) {
        let frame = stalled
            .iter()
            .find(|peer| peer.source_index == source)
            .map_or(u64::MAX, |peer| peer.frame);
        if best.is_none_or(|(lowest, _)| frame < lowest) {
            best = Some((frame, source));
        }
    }
    best.map(|(_, source)| source)
}

/// The first dialog pass (`0x00648C40..0x006490D4`): create 0xEA with the
/// node names, clear the vote tables and the received proposals, and fill
/// list 0x651. `0x005E2820` adds nothing for the empty strings between.
fn open_dialog(state: &mut AppState, elapsed: u64) {
    let session = &state.match_state.session;
    let rows = nodes(session)
        .into_iter()
        .map(|source_index| ConnectionRow {
            source_index,
            name: participant_name(session, source_index),
        })
        .collect();
    let wol = state.match_state.session_mode() == SessionMode::Wol;
    let reconnecting = lowest_record(state)
        .map(|source| participant_name(&state.match_state.session, source))
        .unwrap_or_default();
    let network_wait = &mut state.match_state.network_wait;
    network_wait.received_votes.clear();
    if let Some(wait) = network_wait.wait.as_mut() {
        wait.votes.clear();
        wait.page = Some(ConnectionPage {
            rows,
            painted_elapsed: elapsed,
            ..Default::default()
        });
    }
    if state
        .match_state
        .network_wait
        .wait
        .as_ref()
        .is_some_and(|wait| wait.live_kind == Some(LiveWaitKind::Initial))
    {
        append_line(state, &csf(state, "TXT_WAITING_FOR_CONNECTIONS"));
        return;
    }
    let mut lines = vec![crate::assets::csf_file::format_csf(
        &csf(state, "TXT_RECONNECTING_TO"),
        &[crate::assets::csf_file::CsfArg::Str(&reconnecting)],
    )];
    if wol {
        lines.push(format!(
            "{} {} {}",
            csf(state, "TXT_RECONNECT_HELP3"),
            csf(state, "TXT_RECONNECT_HELP3B"),
            csf(state, "TXT_RECONNECT_HELP3C")
        ));
    }
    lines.push(format!(
        "{} {}",
        csf(state, "TXT_RECONNECT_HELP2"),
        csf(
            state,
            if wol {
                "TXT_RECONNECT_HELP2B"
            } else {
                "TXT_RECONNECT_HELP4"
            }
        )
    ));
    lines.push(csf(state, "TXT_RECONNECT_HELP5"));
    lines.push(csf(state, "TXT_RECONNECT_HELP1"));
    for line in lines {
        append_line(state, &line);
    }
}

/// `0x0064AD10(voter, target)`: count one vote per voter and list it.
fn add_vote(state: &mut AppState, voter: usize, target: usize) {
    let Some(wait) = state.match_state.network_wait.wait.as_mut() else {
        return;
    };
    let voters = wait.votes.entry(target).or_default();
    if voters.contains(&voter) {
        return;
    }
    voters.push(voter);
    let session = &state.match_state.session;
    let line = crate::assets::csf_file::format_csf(
        &csf(state, "TXT_RECONNECT_KICK_RECEIVED"),
        &[
            crate::assets::csf_file::CsfArg::Str(&participant_name(session, voter)),
            crate::assets::csf_file::CsfArg::Str(&participant_name(session, target)),
        ],
    );
    append_line_and_show_last(state, &line);
}

/// The kick tally (`0x006491A0..0x00649226`): a node with
/// `votes >= nodeCount − 1` is kicked with error 1; when it is the local
/// node, every other node is dropped with error 0 instead.
fn tally_votes(state: &mut AppState) {
    let votes = match state.match_state.network_wait.wait.as_ref() {
        Some(wait) => wait.votes.clone(),
        None => return,
    };
    for (source, error) in tally_kicks(nodes(&state.match_state.session), &votes) {
        kick(state, source, error);
    }
}

/// The kicks of one tally pass over the live node list. Each kick removes
/// its node from the list while the index walks on, so the node that moves
/// into its place waits for the next pass, as native does.
fn tally_kicks(
    mut nodes: Vec<usize>,
    votes: &BTreeMap<usize, Vec<usize>>,
) -> Vec<(usize, ConnectionError)> {
    let mut kicks = Vec::new();
    let mut index = 0;
    while index < nodes.len() {
        let source = nodes[index];
        if votes.get(&source).map_or(0, Vec::len) + 1 >= nodes.len() {
            if index == 0 {
                // "Outvoted player is me".
                let mut other = 1;
                while other < nodes.len() {
                    kicks.push((nodes.remove(other), ConnectionError::Left));
                    other += 1;
                }
                return kicks;
            }
            kicks.push((nodes.remove(index), ConnectionError::Lost));
        }
        index += 1;
    }
    kicks
}

/// One pass of `Wait_For_Players` for the in-match stall, before the
/// runtime decides whether a frame may run.
/// RESIDUAL: `0xEA`'s WM_TIMER 1 in a WOL tournament game
/// (`0x0064AFDD..0x0064B0AA`) quits after repeated 5 s/10 s stalls without a
/// WOL ping reply (`[0xB77CE0]`, NetUtil `0x007A411D`); the ping service is
/// backend, so VERA keeps waiting.
pub(crate) fn service_wait(state: &mut AppState, now_ms: u64) -> WaitPass {
    let now = super::frame_pacer::frame_bucket(now_ms);
    if state.match_state.sim_runtime.is_none()
        || !state.match_state.session_mode().is_network()
        || super::network_failure::pending(state)
    {
        state.match_state.network_wait.wait = None;
        return WaitPass::Running;
    }
    let mode = state.match_state.session_mode();
    // Both physical routes feed the same live progress owner. Online relay
    // framing cannot substitute a controlled-service progress row for it.
    let live = state.match_state.live_network.is_some();
    if live {
        let frame = state
            .match_state
            .sim_runtime
            .as_ref()
            .map_or(0, |rt| rt.simulation.session().binary_frame());
        if state
            .match_state
            .live_network
            .as_ref()
            .is_some_and(|live| exit_awaits_unreachable_relay(live, now_ms as u32, frame))
        {
            log::warn!("The relay is unreachable; leaving without the queued EXIT");
            state.match_state.network_wait.wait = None;
            return WaitPass::Leave;
        }
        let info = state
            .match_state
            .live_network
            .as_ref()
            .and_then(|live| live.wait_info());
        let Some(info) = info else {
            state.match_state.network_wait.wait = None;
            return WaitPass::Running;
        };
        state.match_state.network_wait.observe_live_wait(info);
    } else {
        let connected = state.match_state.session.network_connected_source_indices();
        let network_wait = &mut state.match_state.network_wait;
        network_wait
            .stalled
            .retain(|peer| connected.contains(&peer.source_index));
        if network_wait
            .wait
            .as_ref()
            .is_some_and(|wait| wait.live_kind.is_some())
        {
            network_wait.wait = None;
        }
        if network_wait.wait.is_none() {
            if network_wait.stalled.is_empty() {
                return WaitPass::Running;
            }
            network_wait.wait = Some(ConnectionWait::new(now, wait_periods(mode), None));
        }
    }
    let wait = state
        .match_state
        .network_wait
        .wait
        .as_ref()
        .expect("wait started");
    if wait.dialog_timer.remaining(now) == 0 {
        let elapsed = now.wrapping_sub(wait.entry);
        let opening = wait.page.is_none();
        if opening {
            open_dialog(state, u64::from(elapsed));
        }
        let wait = state
            .match_state
            .network_wait
            .wait
            .as_mut()
            .expect("wait remains");
        if wait.leave {
            state.match_state.network_wait.wait = None;
            return WaitPass::Leave;
        }
        let seconds = wait.timeout_timer.remaining(now) / 60;
        let changed = opening || seconds != wait.seconds;
        let repaint = opening || (changed && seconds % 2 == 0);
        if changed {
            wait.seconds = seconds;
            if let Some(page) = wait.page.as_mut() {
                if repaint {
                    page.painted_elapsed = u64::from(elapsed);
                }
            }
            let text = crate::assets::csf_file::format_csf(
                &csf(state, "TXT_TIME_ALLOWED"),
                &[crate::assets::csf_file::CsfArg::Int(i64::from(seconds))],
            );
            if let Some(page) = state.match_state.network_wait.page_mut() {
                page.countdown = Some(text);
            }
        }
        if live && repaint {
            //648C06..648C36 snapshots peer timing and wait age for64A7B0.
            capture_live_bars(state, elapsed, now_ms as u32);
        }
        if let Some((voter, target)) = state.match_state.network_wait.received_votes.pop_front() {
            add_vote(state, voter, target);
        }
        tally_votes(state);
    }
    if let Some(timeout) = state.match_state.network_wait.timeout_action(now) {
        if timeout == WaitTimeout::InitialFailure {
            //649237/649931..649A16: initial expiry returns7; it never kicks.
            let interrupted = if let Some(runtime) = state.match_state.sim_runtime.as_mut() {
                state
                    .match_state
                    .network_wait
                    .interrupt_initial_wait(&mut runtime.simulation)
            } else {
                false
            };
            if interrupted {
                super::network_failure::show(state);
            }
            return WaitPass::Running;
        }
        //64B410 chooses the first minimum signed peer frame. Recovery restarts
        //all three countdowns but retains entry, page, votes and peer+14.
        let source = lowest_record(state);
        let runtime = state.match_state.sim_runtime.as_ref();
        let house = source.and_then(|source| {
            source_house(&state.match_state.session, &runtime?.simulation, source)
        });
        let wait = state
            .match_state
            .network_wait
            .wait
            .as_ref()
            .expect("expired wait");
        log::warn!(
            "Network ordinary wait expired: now_bucket={now}, entered_bucket={}, deadline_bucket={:?}, period_buckets={}, remaining_buckets={}, native_frame={:?}, source={source:?}, house={house:?}, reason={}",
            wait.entry,
            wait.timeout_timer.deadline_bucket(),
            wait.periods.1,
            wait.timeout_timer.remaining(now),
            runtime.map(|runtime| runtime.simulation.session().binary_frame()),
            ConnectionError::Lost.code(),
        );
        if let (Some(runtime), Some(live)) = (runtime, state.match_state.live_network.as_ref()) {
            log_live_progress(
                &state.match_state.session,
                &runtime.simulation,
                live,
                "ordinary_wait_expired",
            );
        }
        if let Some(source) = source {
            kick(state, source, ConnectionError::Lost);
        }
        if let Some(wait) = state.match_state.network_wait.wait.as_mut() {
            wait.restart_timers(now);
        }
        if live && let Some(live) = state.match_state.live_network.as_mut() {
            live.restart_wait_probe(now);
        }
    }
    let present = if live {
        state.match_state.live_network.as_ref().is_none_or(|live| {
            live.peers().is_empty()
                || live.wait_info().is_none()
                || live.frame_wait().is_some_and(|wait| wait.ready())
        })
    } else {
        let connected = state.match_state.session.network_connected_source_indices();
        connected.len() <= 1
            || !state
                .match_state
                .network_wait
                .stalled
                .iter()
                .any(|peer| connected.contains(&peer.source_index))
    };
    if present {
        state.match_state.network_wait.wait = None;
        return WaitPass::Running;
    }
    WaitPass::Waiting
}

/// VERA relay adaptation: Abort's EXIT reaches the other players only once
/// the relay sequences it, none is submitted before the relay starts the
/// match, and silence never fails a relay match. While a confirmed EXIT waits
/// on an unreachable relay, the player leaves by Quit Game's route instead;
/// the returned transport's relay Leave departs this source in the others'
/// timeline. Dialog 0xEA stays off relay waits: its timeout and votes remove
/// peers locally (`0x0064B410`), which only the relay may do in a relay match.
fn exit_awaits_unreachable_relay(
    live: &crate::net::live_commands::LiveCommandSession,
    now_millis: u32,
    frame: u32,
) -> bool {
    live.exit_requested()
        && live.recovery_status(now_millis, frame)
            == Some(crate::net::live_commands::NetworkRecoveryStatus::Reconnecting)
}

/// Quit Game 2 (`0x0064AE50` WM_COMMAND 2): the result cell becomes 2 and
/// the next dialog pass leaves.
pub(crate) fn request_leave(state: &mut AppState) {
    if let Some(wait) = state.match_state.network_wait.wait.as_mut() {
        wait.leave = true;
    }
}

/// A row button (`0x0064AAE0`): the local row explains itself; in a WOL
/// tournament game another row is refused with `TXT_CANT_KICK`
/// (`0x0064AB6A..0x0064AB98`); otherwise it sends the proposal to every
/// other node and counts the local vote.
pub(crate) fn propose_kick(state: &mut AppState, row: usize) {
    let Some(target) = state
        .match_state
        .network_wait
        .page()
        .and_then(|page| page.row_source(row))
    else {
        return;
    };
    let Some(local) = local_source(&state.match_state.session) else {
        return;
    };
    if target == local {
        let line = csf(state, "TXT_RECONNECT_KICK_SELF");
        append_line_and_show_last(state, &line);
        return;
    }
    if state
        .match_state
        .sim_runtime
        .as_ref()
        .is_some_and(|runtime| runtime.simulation.session().wol_tournament_game())
    {
        let line = csf(state, "TXT_CANT_KICK");
        append_line_and_show_last(state, &line);
        return;
    }
    // The initial names stay fixed while nodes can disappear during a wait.
    // A disconnected row is inert; it can never nominate its former successor.
    if !state
        .match_state
        .session
        .network_connected_source_indices()
        .contains(&target)
    {
        return;
    }
    state.match_state.session.request_kick_proposal(target);
    add_vote(state, local, target);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Table 0x00837340 rows 3 (LAN) and 4 (WOL), +8 and +C.
    #[test]
    fn in_match_wait_uses_the_native_timing_rows() {
        assert_eq!(wait_periods(SessionMode::Lan), (420, 2400));
        assert_eq!(wait_periods(SessionMode::Wol), (720, 7200));
    }

    /// The countdown is the timeout's remaining buckets /60: 33 when 0xEA
    /// opens on a LAN stall, 0 once the timeout fires.
    #[test]
    fn countdown_seconds_count_sixty_bucket_steps_of_the_timeout() {
        let (dialog_after, timeout) = wait_periods(SessionMode::Lan);
        let entry = 1_000;
        let dialog = WallBucketTimer::start(entry, dialog_after);
        let timer = WallBucketTimer::start(entry, timeout);
        assert_ne!(dialog.remaining(entry + dialog_after - 1), 0);
        assert_eq!(dialog.remaining(entry + dialog_after), 0);
        assert_eq!(timer.remaining(entry + dialog_after) / 60, 33);
        assert_eq!(timer.remaining(entry + timeout - 1) / 60, 0);
        assert_ne!(timer.remaining(entry + timeout - 1), 0);
        assert_eq!(timer.remaining(entry + timeout), 0);
    }

    #[test]
    fn online_wait_uses_online_initial_and_ordinary_deadlines() {
        let info = LiveWaitInfo {
            mode: LiveCommandMode::Wol,
            kind: LiveWaitKind::Initial,
            entered_at: 100,
            response_time_at_entry: 0,
        };
        let mut wait = NetworkMatchWait::default();
        wait.observe_live_wait(info);
        assert_eq!(wait.wait.as_ref().unwrap().dialog_timer.remaining(1899), 1);
        assert_eq!(wait.timeout_action(7299), None);
        assert_eq!(wait.timeout_action(7300), Some(WaitTimeout::InitialFailure));
        let ordinary = LiveWaitInfo {
            kind: LiveWaitKind::Ordinary,
            ..info
        };
        assert_eq!(live_wait_periods(ordinary), (720, 7200));
        assert_eq!(
            live_wait_periods(LiveWaitInfo {
                response_time_at_entry: 241,
                ..ordinary
            }),
            (723, 7200)
        );
    }

    #[test]
    fn live_wait_uses_reached_entry_and_signed_wrapping_response_delay() {
        let info = LiveWaitInfo {
            mode: LiveCommandMode::Lan,
            kind: LiveWaitKind::Initial,
            entered_at: 100,
            response_time_at_entry: u32::MAX,
        };
        assert_eq!(live_wait_periods(info), (900, 2400));
        let mut wait = NetworkMatchWait::default();
        wait.observe_live_wait(info);
        // First UI service may be late; deadlines remain anchored to the
        // already reached live wait, not the first UI observation.
        assert_eq!(wait.wait.as_ref().unwrap().dialog_timer.remaining(999), 1);
        assert_eq!(wait.wait.as_ref().unwrap().dialog_timer.remaining(1000), 0);
        assert_eq!(wait.timeout_action(2499), None);
        assert_eq!(wait.timeout_action(2500), Some(WaitTimeout::InitialFailure));
        let ordinary = LiveWaitInfo {
            mode: LiveCommandMode::Lan,
            kind: LiveWaitKind::Ordinary,
            ..info
        };
        assert_eq!(live_wait_periods(ordinary), (420, 2400));
        assert_eq!(
            live_wait_periods(LiveWaitInfo {
                response_time_at_entry: 141,
                ..ordinary
            }),
            (423, 2400)
        );
        wait.observe_live_wait(ordinary);
        assert_eq!(wait.timeout_action(2500), Some(WaitTimeout::DropPeer));
    }

    #[test]
    fn ordinary_recovery_retains_entry_page_votes_and_original_arguments() {
        let info = LiveWaitInfo {
            mode: LiveCommandMode::Lan,
            kind: LiveWaitKind::Ordinary,
            entered_at: 100,
            response_time_at_entry: 141,
        };
        let mut owner = NetworkMatchWait::default();
        owner.observe_live_wait(info);
        let wait = owner.wait.as_mut().unwrap();
        wait.votes.insert(1, vec![0]);
        wait.page = Some(ConnectionPage::default());
        assert_eq!(wait.timeout_timer.deadline_bucket(), Some(2500));
        wait.restart_timers(2500);
        owner.observe_live_wait(LiveWaitInfo {
            response_time_at_entry: 500,
            ..info
        });
        let wait = owner.wait.as_ref().unwrap();
        assert_eq!(wait.entry, 100);
        assert_eq!(wait.periods, (423, 2400));
        assert_eq!(wait.timeout_timer.deadline_bucket(), Some(4900));
        assert!(wait.page.is_some());
        assert_eq!(wait.votes.get(&1), Some(&vec![0]));
        assert_eq!(wait.dialog_timer.remaining(2922), 1);
        assert_eq!(wait.dialog_timer.remaining(2923), 0);
        assert_eq!(owner.timeout_action(4899), None);
        assert_eq!(owner.timeout_action(4900), Some(WaitTimeout::DropPeer));
    }

    #[test]
    fn initial_timeout_hands_the_retained_frame_to_the_existing_failure_tail() {
        struct Waiting(std::cell::RefCell<crate::sim::command_queue::FrameChecksumState>);
        impl crate::sim::world::NetworkCommandBoundary for Waiting {
            fn exchange_commands(
                &self,
                _: crate::sim::world::NetworkQueueContext,
                _: &mut crate::sim::world::NetworkCommandQueues<'_>,
            ) -> Result<
                Option<crate::sim::world::NetworkQueuePass>,
                crate::sim::world::NetworkCommandError,
            > {
                Ok(Some(crate::sim::world::NetworkQueuePass::Waiting))
            }

            fn record_frame_checksum(&self, frame: u32, checksum: u32) {
                self.0.borrow_mut().record_frame_checksum(frame, checksum);
            }

            fn compare_frame_info(
                &self,
                record: &crate::sim::command::CommandRecord,
                current_frame: i32,
            ) -> Result<bool, crate::sim::world::NetworkCommandError> {
                crate::sim::world::NetworkCommandBoundary::compare_frame_info(
                    &self.0,
                    record,
                    current_frame,
                )
            }
        }
        let mut sim = Simulation::new();
        let human = sim.intern("Human");
        sim.houses_mut_for_test()
            .register(crate::sim::house_state::HouseState::new(
                human, 0, None, true, 0, 10,
            ));
        sim.session_mut_for_test().set_game_mode(SessionMode::Lan);
        let rules = crate::rules::ruleset::RuleSet::empty_for_test();
        let waiting = Waiting(Default::default());
        let output = sim
            .advance_app_frame(
                &[],
                rules,
                None,
                33,
                crate::sim::world::TickLane::Ordinary,
                crate::sim::world::FrameEffects::empty().with_network_commands(&waiting),
            )
            .unwrap();
        assert!(output.network_waiting);
        assert!(!output.tick.frame_committed);
        let mut owner = NetworkMatchWait::default();
        owner.observe_live_wait(LiveWaitInfo {
            mode: LiveCommandMode::Lan,
            kind: LiveWaitKind::Initial,
            entered_at: 100,
            response_time_at_entry: 0,
        });
        assert_eq!(
            owner.timeout_action(2500),
            Some(WaitTimeout::InitialFailure)
        );
        assert!(owner.interrupt_initial_wait(&mut sim));
        assert!(!owner.waiting());
        assert!(!sim.pending_network_wait());
        assert_eq!(
            sim.pending_network_interruption(),
            Some(crate::sim::world::NetworkInterruption::ConnectionFailed,)
        );
        let output = sim
            .resume_after_network_failure(rules, None, crate::sim::world::FrameEffects::empty())
            .unwrap();
        assert!(output.tick.frame_committed);
        assert_eq!(sim.session().binary_frame(), 1);
        assert!(!sim.quit_requested());
        assert!(sim.take_executed_exit_owner().is_none());
    }

    fn vote_fixture() -> (MatchSession, Simulation) {
        let profile: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.command-bar.example.json",
        ))
        .unwrap();
        let mut launch: crate::skirmish_launch::SkirmishLaunchSession =
            serde_json::from_value(profile["launch"].clone()).unwrap();
        launch.opponents.clear();
        launch.ai_player_count = 0;
        let mut guest = launch.local_player().clone();
        guest.name = "G\u{0100}uest".into();
        guest.raw_slot_index = Some(1);
        guest.slot.color_index = 2;
        launch.humans.push(guest);
        launch.local_player_index = 1;
        let mut session = MatchSession::default();
        session.enter_network(super::super::session::NetworkReturnContext::LanLobby);
        session
            .accept_network_setup(launch, false, Some(7), false)
            .unwrap();
        let mut sim = Simulation::new();
        // Deliberately distinct participant and native House ordering.
        for (house, source) in [1, 0].into_iter().enumerate() {
            let name = session.network_source_owner(source).unwrap();
            let owner = sim.intern(&name);
            sim.houses_mut_for_test()
                .register(crate::sim::house_state::HouseState::new(
                    owner,
                    house as u8,
                    None,
                    true,
                    0,
                    10,
                ));
        }
        (session, sim)
    }

    #[test]
    fn a_repeated_kick_proposal_is_queued_once() {
        let mut wait = NetworkMatchWait::default();
        for _ in 0..3 {
            wait.queue_vote(1, 0);
        }
        wait.queue_vote(2, 0);
        assert_eq!(wait.received_votes, [(1, 0), (2, 0)]);
    }

    #[test]
    fn global_kick_codec_uses_native_house_ids_and_existing_string_conversion() {
        use std::net::{Ipv4Addr, SocketAddrV4};
        let (mut session, sim) = vote_fixture();
        let voter = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30121);
        let other = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30122);
        for (source, endpoint) in [(1, voter), (0, other)] {
            let name = session.network_source_owner(source).unwrap();
            session.bind_network_endpoint(&name, endpoint).unwrap();
        }
        let packet = encode_kick_proposal(&session, &sim, 0).unwrap();
        assert_eq!(&packet[..4], &0x1c_u32.to_le_bytes());
        assert_eq!(&packet[4..7], b"G\0\0");
        assert_eq!(&packet[0x2f..0x33], &1_i32.to_le_bytes());
        assert_eq!(&packet[0x33..0x37], &0_i32.to_le_bytes());
        assert_eq!(
            decode_kick_proposal(&session, &sim, voter, &packet),
            Some(NetworkMatchEvent::KickVote {
                voter: 1,
                target: 0
            })
        );
        // Another node cannot cast source 1's vote.
        assert!(decode_kick_proposal(&session, &sim, other, &packet).is_none());
        assert!(decode_kick_proposal(&session, &sim, voter, &packet[..54]).is_none());
        let mut invalid = packet;
        invalid[0x2f..0x33].copy_from_slice(&(-1_i32).to_le_bytes());
        assert!(decode_kick_proposal(&session, &sim, voter, &invalid).is_none());
        invalid = packet;
        invalid[0] = 0x19;
        assert!(decode_kick_proposal(&session, &sim, voter, &invalid).is_none());
    }

    #[test]
    fn global_departure_uses_active_endpoint_and_leaves_lan_house_alive() {
        use crate::net::reliable::{LanConnectionConfig, LanConnections, RetryTiming};
        use crate::net::udp::{LanUdpTransport, UdpBindConfig};
        use std::net::{Ipv4Addr, SocketAddrV4};
        let (mut session, mut sim) = vote_fixture();
        let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30123);
        let name = session.network_setup().unwrap().humans[0].name.clone();
        session.bind_network_endpoint(&name, endpoint).unwrap();
        session.prepare_network_timing(2, || 0).unwrap();
        session.freeze_network_start(19).unwrap();
        sim.session_mut_for_test().set_game_mode(SessionMode::Lan);
        let mut connections = LanConnections::new(
            LanConnectionConfig::default(),
            RetryTiming {
                retry_delta: 20,
                max_retries: u32::MAX,
                timeout: 160,
            },
        );
        // Fixture source0 is native House1; the local source1 is House0.
        connections.add_peer(1, endpoint).unwrap();
        let transport = LanUdpTransport::bind(
            UdpBindConfig {
                primary: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
                alternate: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
            },
            connections,
        )
        .unwrap();
        let mut live = crate::net::live_commands::LiveCommandSession::new(
            crate::net::live_commands::StartupAgreement::new(
                [(0, 1), (1, 0)],
                [0, 1],
                19,
                10,
                30,
                std::num::NonZeroU32::new(2).unwrap(),
                sim.state_hash(),
            )
            .unwrap(),
            transport,
            0,
            19,
            std::num::NonZeroU32::new(2).unwrap(),
            LiveCommandMode::Lan,
        )
        .unwrap();
        let mut wait = NetworkMatchWait::default();
        wait.stalled.push(StalledPeer {
            source_index: 0,
            frame: 3,
        });
        let wrong_port = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30124);
        assert!(
            receive_departure(&mut session, &mut sim, &mut live, &mut wait, wrong_port).is_none()
        );
        let remote = sim.houses().order()[1];
        sim.houses_mut_for_test()
            .get_mut(&remote)
            .unwrap()
            .set_is_human_for_test(false);
        assert!(
            receive_departure(&mut session, &mut sim, &mut live, &mut wait, endpoint).is_none()
        );
        assert_eq!(live.peers().len(), 1);
        assert_eq!(session.network_connected_source_indices(), &[0, 1]);
        assert_eq!(wait.stalled.len(), 1);
        sim.houses_mut_for_test()
            .get_mut(&remote)
            .unwrap()
            .set_is_human_for_test(true);
        let removed =
            receive_departure(&mut session, &mut sim, &mut live, &mut wait, endpoint).unwrap();
        assert_eq!(removed.key, "TXT_LEFT_GAME");
        assert_eq!(removed.owner, sim.houses().order()[1]);
        assert_eq!(session.network_connected_source_indices(), &[1]);
        assert!(live.connections().peers().next().is_none());
        assert!(live.peers().is_empty());
        assert!(wait.stalled.is_empty());
        let house = sim.houses().get(&removed.owner).unwrap();
        assert!(house.is_human());
        assert!(!house.is_to_die());
        assert!(!house.is_defeated());
        assert_eq!(sim.network_command_queues().local_len(), 0);
        assert!(sim.take_executed_exit_owner().is_none());
        // The frozen roster keeps its binding; only the live connection is
        // eligible for another Departure.
        assert_eq!(session.network_endpoint_source(endpoint), Some(0));
        assert!(
            receive_departure(&mut session, &mut sim, &mut live, &mut wait, endpoint).is_none()
        );
    }

    #[test]
    fn frame_zero_loss_is_native_before_install_and_aborts_uncertified_startup() {
        use crate::net::reliable::{LanConnectionConfig, LanConnections, RetryTiming};
        use crate::net::udp::{LanUdpTransport, UdpBindConfig};
        use std::net::{Ipv4Addr, SocketAddrV4};
        use std::num::{NonZeroU8, NonZeroU32};

        for installed in [false, true] {
            let (mut session, mut sim) = vote_fixture();
            let launch = session.network_setup().unwrap().clone();
            session.enter_network(super::super::session::NetworkReturnContext::Wol);
            session
                .accept_network_setup(launch, false, None, false)
                .unwrap();
            session.prepare_wol_timing([0; 2], false).unwrap();
            let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30125);
            let remote_name = session.network_setup().unwrap().humans[0].name.clone();
            session
                .bind_network_endpoint(&remote_name, endpoint)
                .unwrap();
            session.freeze_network_start(19).unwrap();
            let local = sim.houses().order()[0];
            let remote = sim.houses().order()[1];
            sim.session_mut_for_test().set_game_mode(SessionMode::Wol);
            sim.session_mut_for_test()
                .set_current_house_for_test(Some(local));
            sim.install_network_timing(session.network_max_ahead_frames().unwrap());
            if installed {
                sim.network_command_queues()
                    .install_timing(1, NonZeroU8::new(5).unwrap(), [0, 1]);
            }
            let timing = sim.session().network_command_timing().cloned();
            let mut live = installed.then(|| {
                let mut connections = LanConnections::new(
                    LanConnectionConfig::default(),
                    RetryTiming::LAN_GUEST_COMPLETED_GLOBAL,
                );
                connections.add_peer(1, endpoint).unwrap();
                let bind = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0);
                let transport = LanUdpTransport::bind(
                    UdpBindConfig {
                        primary: bind,
                        alternate: bind,
                    },
                    connections,
                )
                .unwrap();
                crate::net::live_commands::LiveCommandSession::new(
                    crate::net::live_commands::StartupAgreement::new(
                        [(0, 1), (1, 0)],
                        [0, 1],
                        19,
                        10,
                        30,
                        NonZeroU32::new(5).unwrap(),
                        sim.state_hash(),
                    )
                    .unwrap(),
                    transport,
                    0,
                    19,
                    NonZeroU32::new(5).unwrap(),
                    LiveCommandMode::Wol,
                )
                .unwrap()
            });
            assert_eq!(sim.session().binary_frame(), 0);
            let removed = destroy_connection(
                &mut session,
                &mut sim,
                0,
                ConnectionError::Lost,
                live.as_mut(),
            )
            .unwrap();
            assert_eq!(removed.owner, remote);
            assert_eq!(removed.key, "TXT_CONNECTION_LOST");
            assert_eq!(session.network_connected_source_indices(), &[1]);
            if let Some(live) = live.as_ref() {
                assert!(live.peers().is_empty());
                assert!(live.connections().peers().next().is_none());
            }
            assert_eq!(sim.houses()[&remote].is_to_die(), !installed);
            assert_eq!(sim.session().network_command_timing(), timing.as_ref());
            let records = sim
                .network_command_queues()
                .local_records()
                .unwrap()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>();
            if !installed {
                assert!(
                    records.is_empty(),
                    "loading before timing installation stays native"
                );
                continue;
            }
            assert_eq!(sim.network_command_queues().timing_host_house(), Some(1));
            assert!(
                records.is_empty(),
                "uncertified source loss cannot mutate shared admission"
            );
            let error = live
                .as_mut()
                .unwrap()
                .poll(
                    &mut sim.network_command_queues(),
                    crate::sim::world::NetworkQueueContext {
                        frame: 0,
                        checksum: 7,
                    },
                    10,
                    100,
                    1600,
                    0,
                    0,
                )
                .unwrap_err();
            assert!(matches!(
                error,
                crate::net::live_commands::LiveCommandError::Startup(
                    crate::net::live_commands::StartupError::Aborted
                )
            ));
        }
    }

    /// A started two-player WOL relay match whose relay never answers.
    fn relay_fixture() -> (
        MatchSession,
        Simulation,
        crate::net::live_commands::LiveCommandSession,
    ) {
        use crate::net::reliable::{LanConnectionConfig, LanConnections, RetryTiming};
        use crate::net::udp::test_support::{SecretKey, derive_packet_key};
        use crate::net::udp::{
            LanUdpTransport, OrderingCertificate, RelayTicket, UdpBindConfig, relay_peer_endpoint,
        };
        use std::net::{Ipv4Addr, SocketAddrV4};
        use std::num::{NonZeroU8, NonZeroU32};

        let (mut session, mut sim) = vote_fixture();
        let launch = session.network_setup().unwrap().clone();
        session.enter_network(super::super::session::NetworkReturnContext::Wol);
        session
            .accept_network_setup(launch, false, None, false)
            .unwrap();
        session.prepare_wol_timing([0; 2], false).unwrap();
        let remote_name = session.network_setup().unwrap().humans[0].name.clone();
        let endpoint = relay_peer_endpoint(0).unwrap();
        session
            .bind_network_endpoint(&remote_name, endpoint)
            .unwrap();
        session.freeze_network_start(19).unwrap();
        let local = sim.houses().order()[0];
        sim.session_mut_for_test().set_game_mode(SessionMode::Wol);
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(local));
        sim.install_network_timing(session.network_max_ahead_frames().unwrap());
        sim.network_command_queues()
            .install_timing(1, NonZeroU8::new(1).unwrap(), [0, 1]);
        let key = SecretKey::new([5; 32]);
        let ticket = RelayTicket {
            room: [3; 16],
            source: 1,
            expires: u64::MAX / 2,
            nonce: [2; 16],
            player: [2; 16],
            generation: 1,
        }
        .sign(&key)
        .unwrap();
        let mut connections = LanConnections::new(
            LanConnectionConfig::default(),
            RetryTiming::LAN_GUEST_COMPLETED_GLOBAL,
        );
        connections.add_peer(1, endpoint).unwrap();
        let bind = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0);
        let mut transport = LanUdpTransport::bind(
            UdpBindConfig {
                primary: bind,
                alternate: bind,
            },
            connections,
        )
        .unwrap();
        transport
            .enable_relay(
                SocketAddrV4::new(Ipv4Addr::LOCALHOST, 9),
                &ticket,
                *derive_packet_key(&key, &ticket).expose_for_https(),
                [6; 32],
            )
            .unwrap();
        let certificate = OrderingCertificate {
            room: [3; 16],
            sources_mask: 0b11,
            tick_hz: 30,
            expires: u64::MAX / 2,
        }
        .sign(&key)
        .unwrap();
        transport
            .set_ordering_certificate(certificate.to_vec())
            .unwrap();
        let mut live = crate::net::live_commands::LiveCommandSession::new(
            crate::net::live_commands::StartupAgreement::new(
                [(0, 1), (1, 0)],
                [0, 1],
                19,
                10,
                30,
                NonZeroU32::new(1).unwrap(),
                sim.state_hash(),
            )
            .unwrap(),
            transport,
            0,
            19,
            NonZeroU32::new(1).unwrap(),
            LiveCommandMode::Wol,
        )
        .unwrap();
        assert!(live.uses_sequenced_lockstep());
        live.mark_sequenced_started_for_test();
        (session, sim, live)
    }

    /// A confirmed Abort's EXIT needs the relay, also when it was queued
    /// before the relay fell silent; after three silent seconds it leaves.
    #[test]
    fn a_queued_exit_leaves_only_once_the_relay_is_unreachable() {
        let (_, _, mut live) = relay_fixture();
        live.service_transport(0);
        assert!(!exit_awaits_unreachable_relay(&live, 3_001, 0));
        live.keep_running_for_exit();
        assert!(!exit_awaits_unreachable_relay(&live, 3_000, 0));
        assert!(exit_awaits_unreachable_relay(&live, 3_001, 0));
    }

    /// Relay-ordered play: the committed departure already carried the shared
    /// EVENT_REMOVEPLAYER, so DestroyConnection retires only this client's
    /// connection and posts its line.
    #[test]
    fn relay_connection_loss_only_retires_the_local_connection() {
        let (mut session, mut sim, mut live) = relay_fixture();
        let remote = sim.houses().order()[1];
        let removed = destroy_connection(
            &mut session,
            &mut sim,
            0,
            ConnectionError::Lost,
            Some(&mut live),
        )
        .unwrap();
        assert_eq!(removed.owner, remote);
        assert_eq!(removed.key, "TXT_CONNECTION_LOST");
        assert_eq!(session.network_connected_source_indices(), &[1]);
        assert!(live.connections().peers().next().is_none());
        assert!(!sim.houses()[&remote].is_to_die());
        assert!(
            sim.network_command_queues()
                .local_records()
                .unwrap()
                .is_empty(),
            "a local EVENT_REMOVEPLAYER cannot enter the relay stream"
        );
    }

    #[test]
    fn asymmetric_initial_vote_aborts_without_certifying_different_worlds() {
        use crate::net::live_commands::{
            LiveCommandError, LiveCommandSession, StartupAgreement, StartupError,
        };
        use crate::net::reliable::{Delivery, LanConnectionConfig, LanConnections, RetryTiming};
        use crate::net::udp::{LanUdpTransport, UdpBindConfig};
        use crate::sim::world::{NetworkQueueContext, NetworkQueuePass};
        use std::net::{Ipv4Addr, SocketAddrV4};
        use std::num::NonZeroU32;
        use std::time::{Duration, Instant};
        let bind = || {
            LanUdpTransport::bind(
                UdpBindConfig {
                    primary: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
                    alternate: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
                },
                LanConnections::new(
                    LanConnectionConfig::default(),
                    RetryTiming {
                        retry_delta: 0,
                        max_retries: u32::MAX,
                        timeout: u32::MAX,
                    },
                ),
            )
            .unwrap()
        };
        let mut host = bind();
        let mut first = bind();
        let mut second = bind();
        let endpoints = [
            host.primary_addr().unwrap(),
            first.primary_addr().unwrap(),
            second.primary_addr().unwrap(),
        ];
        let fixture = |local_source: usize| {
            let (session, mut sim) = vote_fixture();
            let mut launch = session.network_setup().unwrap().clone();
            let mut third = launch.humans[0].clone();
            third.name = "Third".into();
            third.raw_slot_index = Some(2);
            third.slot.color_index = 3;
            launch.humans.push(third);
            launch.local_player_index = local_source;
            let mut session = MatchSession::default();
            session.enter_network(super::super::session::NetworkReturnContext::Wol);
            session
                .accept_network_setup(launch, false, None, false)
                .unwrap();
            let name = session.network_source_owner(2).unwrap();
            let owner = sim.intern(&name);
            sim.houses_mut_for_test()
                .register(crate::sim::house_state::HouseState::new(
                    owner, 2, None, true, 0, 10,
                ));
            for source in 0..3 {
                let name = session.network_setup().unwrap().humans[source].name.clone();
                session
                    .bind_network_endpoint(&name, endpoints[source])
                    .unwrap();
            }
            session.prepare_wol_timing([0; 3], false).unwrap();
            session.freeze_network_start(19).unwrap();
            sim.session_mut_for_test().set_game_mode(SessionMode::Wol);
            let local = *sim
                .houses()
                .order()
                .get(if local_source == 1 { 0 } else { 2 })
                .unwrap();
            sim.session_mut_for_test()
                .set_current_house_for_test(Some(local));
            sim.install_network_timing(10);
            sim.network_command_queues().install_timing(
                1,
                std::num::NonZeroU8::new(5).unwrap(),
                [0, 1, 2],
            );
            (session, sim)
        };
        let (mut a, mut wa) = fixture(1);
        let (b, mut wb) = fixture(2);
        assert_eq!(wa.state_hash(), wb.state_hash());
        let original_hash = wa.state_hash();
        first.connections_mut().add_peer(1, endpoints[0]).unwrap();
        first.connections_mut().add_peer(2, endpoints[2]).unwrap();
        second.connections_mut().add_peer(1, endpoints[0]).unwrap();
        second.connections_mut().add_peer(0, endpoints[1]).unwrap();
        host.connections_mut().add_peer(2, endpoints[2]).unwrap();
        let rate = NonZeroU32::new(5).unwrap();
        let agreement = StartupAgreement::new(
            [(0, 1), (1, 0), (2, 2)],
            [0, 1, 2],
            19,
            10,
            30,
            rate,
            original_hash,
        )
        .unwrap();
        let mut first =
            LiveCommandSession::new(agreement.clone(), first, 0, 19, rate, LiveCommandMode::Wol)
                .unwrap();
        let mut second =
            LiveCommandSession::new(agreement, second, 2, 19, rate, LiveCommandMode::Wol).unwrap();
        let probe = crate::net::command_exchange::CommandExchange::new([0, 2])
            .unwrap()
            .frame_probe(
                1,
                0,
                10,
                19,
                crate::net::lockstep::NetworkSendPolicy::FrameSendRate2 {
                    frame_send_rate: rate,
                },
            )
            .unwrap();
        // Original source0 answers only source2, then disappears. Source1's
        // ordinary first poll sends its real probe and reliable HELLO to2.
        assert!(
            host.connections_mut()
                .send_to(2, &probe, Delivery::Unreliable)
        );
        host.service(1);
        let context = NetworkQueueContext {
            frame: 0,
            checksum: 7,
        };
        assert_eq!(
            first
                .poll(
                    &mut wa.network_command_queues(),
                    context,
                    10,
                    100,
                    1600,
                    0,
                    0
                )
                .unwrap(),
            NetworkQueuePass::Waiting
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        while second.peers().iter().any(|peer| peer.frame() < 0) {
            assert_eq!(
                second
                    .poll(
                        &mut wb.network_command_queues(),
                        context,
                        10,
                        101,
                        1616,
                        0,
                        0
                    )
                    .unwrap(),
                NetworkQueuePass::Waiting
            );
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        // The native node-count tally is asymmetric when the reverse vote is
        // delayed. It is the real production decision, not an injected result.
        let kicked = tally_kicks(vec![1, 0, 2], &votes(&[(0, &[1, 2])]));
        assert_eq!(kicked, vec![(0, ConnectionError::Lost)]);
        assert!(tally_kicks(vec![2, 0, 1], &votes(&[(0, &[2])])).is_empty());
        for (source, error) in kicked {
            destroy_connection(&mut a, &mut wa, source, error, Some(&mut first)).unwrap();
        }
        assert_eq!(a.network_connected_source_indices(), &[1, 2]);
        assert_eq!(b.network_connected_source_indices(), &[0, 1, 2]);
        assert_eq!(wa.state_hash(), original_hash);
        assert_eq!(wb.state_hash(), original_hash);
        assert!(matches!(
            first.poll(
                &mut wa.network_command_queues(),
                context,
                10,
                102,
                1632,
                0,
                0
            ),
            Err(LiveCommandError::Startup(StartupError::Aborted))
        ));
        assert_eq!(
            second
                .poll(
                    &mut wb.network_command_queues(),
                    context,
                    10,
                    102,
                    1632,
                    0,
                    0
                )
                .unwrap(),
            NetworkQueuePass::Waiting
        );
        assert!(second.startup_pending());
        assert_eq!(second.sent_commands(), 0);
        assert_eq!(wa.network_command_queues().local_len(), 0);
    }

    fn votes(entries: &[(usize, &[usize])]) -> BTreeMap<usize, Vec<usize>> {
        entries
            .iter()
            .map(|(target, voters)| (*target, voters.to_vec()))
            .collect()
    }

    /// Two nodes: one local vote reaches nodeCount − 1.
    #[test]
    fn a_single_vote_kicks_the_only_peer_with_error_one() {
        assert_eq!(
            tally_kicks(vec![0, 1], &votes(&[(1, &[0])])),
            [(1, ConnectionError::Lost)]
        );
        assert!(tally_kicks(vec![0, 1, 2], &votes(&[(1, &[0])])).is_empty());
    }

    /// A kick shifts the live list under the walking index: the node that
    /// takes the kicked one's place is skipped until the next pass.
    #[test]
    fn a_kick_skips_the_node_that_moves_into_its_place() {
        let votes = votes(&[(1, &[0, 2, 3]), (2, &[0, 1, 3])]);
        assert_eq!(
            tally_kicks(vec![0, 1, 2, 3], &votes),
            [(1, ConnectionError::Lost)]
        );
        assert_eq!(
            tally_kicks(vec![0, 2, 3], &votes),
            [(2, ConnectionError::Lost)]
        );
    }

    /// The outvoted local node drops the others with error 0, with the same
    /// skip, and stops the pass.
    #[test]
    fn an_outvoted_local_node_drops_every_other_with_error_zero() {
        let votes = votes(&[(0, &[1, 2]), (2, &[0, 1])]);
        assert_eq!(
            tally_kicks(vec![0, 1, 2], &votes),
            [(1, ConnectionError::Left)]
        );
        assert_eq!(
            tally_kicks(vec![0, 2], &votes),
            [(2, ConnectionError::Left)]
        );
    }

    #[test]
    fn the_lowest_record_is_the_earliest_stalled_peer_then_the_first() {
        let stalled = |source_index, frame| StalledPeer {
            source_index,
            frame,
        };
        assert_eq!(
            lowest_record_of(&[0, 1, 2], &[stalled(1, 12), stalled(2, 10)]),
            Some(2)
        );
        assert_eq!(
            lowest_record_of(&[0, 1, 2], &[stalled(1, 10), stalled(2, 10)]),
            Some(1)
        );
        assert_eq!(lowest_record_of(&[0, 1, 2], &[]), Some(1));
        assert_eq!(lowest_record_of(&[0], &[]), None);
    }
}
