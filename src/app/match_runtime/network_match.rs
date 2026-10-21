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
use crate::net::live_commands::{LiveWaitInfo, LiveWaitKind};
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;
use crate::ui::lan_shell::LanChatLine;
use crate::ui::shell::connection::ConnectionPage;

/// DestroyConnection's error argument (`EDX`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectionError {
    /// 0: the peer sent EXIT. `TXT_LEFT_GAME`, nothing queued.
    Left,
    /// 1: the connection was lost. `TXT_CONNECTION_LOST`; Flag_To_Die at
    /// frame 0, REMOVEPLAYER otherwise.
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
/// Returns early, with no line, when the House is not human (`0x005DA77E`);
/// a participant already disconnected has no connection record, so its
/// caller never reaches here twice. The line itself is posted by
/// [`post_connection_line`] once the simulation is installed.
pub(crate) fn destroy_connection(
    session: &mut MatchSession,
    sim: &mut Simulation,
    source_index: usize,
    error: ConnectionError,
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
    if !session.disconnect_network_participant(source_index, error.code()) {
        return None;
    }
    if error == ConnectionError::Lost {
        if sim.session().binary_frame() == 0 {
            sim.flag_house_to_die(owner);
        } else {
            // EVENT_REMOVEPLAYER from PlayerPtr with the House's index
            // (0x005DA898..0x005DA8F0); it kills the House on every peer.
            let local = sim.session().current_house();
            let index = sim.houses().order().iter().position(|id| *id == owner);
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
    // player who left (error 0). RESIDUAL: mode 4's ladder-stats call
    // (0x005DA921..) is backend.
    if error == ConnectionError::Left && sim.session().wol_tournament_game() {
        sim.flag_house_to_die(owner);
    }
    Some(ConnectionDestroyed {
        owner,
        key: error.csf_key(),
    })
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
/// DestroyConnection(house, 0) (`0x0064C9B1`).
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
            state
                .match_state
                .network_wait
                .received_votes
                .push_back((voter, target));
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

///Active LAN call arguments: initial647919..647954; ordinary64800E..648067.
fn live_wait_periods(info: LiveWaitInfo) -> (u32, u32) {
    let dialog = match info.kind {
        LiveWaitKind::Initial => 900,
        LiveWaitKind::Ordinary => {
            (info.response_time_at_entry.wrapping_mul(3) as i32).max(420) as u32
        }
    };
    (dialog, 2400)
}

/// One `Wait_For_Players` call.
#[derive(Debug, Clone)]
struct ConnectionWait {
    /// `local_6c0`: the entry bucket; `[0x00AFA418]` is the time since.
    entry: u32,
    ///None selects the established controlled-service path. Live timing is
    ///LAN-only; WOL router recovery remains a separate dependency.
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
    packet: &[u8],
) -> Option<NetworkMatchEvent> {
    if packet.len() != 455 || packet[..4] != 0x1c_u32.to_le_bytes() {
        return None;
    }
    let target = i32::from_le_bytes(packet[0x2f..0x33].try_into().ok()?);
    let voter = i32::from_le_bytes(packet[0x33..0x37].try_into().ok()?);
    Some(NetworkMatchEvent::KickVote {
        voter: house_source(session, sim, voter)?,
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
    let house = state.match_state.sim_runtime.as_ref().and_then(|runtime| {
        source_house(
            &state.match_state.session,
            &runtime.simulation,
            source_index,
        )
    });
    if let (Some(live), Some(house)) = (
        &mut state.match_state.live_network,
        house.and_then(|house| i8::try_from(house).ok()),
    ) {
        //64B240 compacts the progress row before5DA750 removes membership.
        //The live owner removes the reliable connection and progress together.
        live.remove_peer(house);
    }
    let Some(runtime) = state.match_state.sim_runtime.as_mut() else {
        return;
    };
    if let Some(destroyed) = destroy_connection(
        &mut state.match_state.session,
        &mut runtime.simulation,
        source_index,
        error,
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
        .take(crate::ui::shell::connection::CONNECTION_ROWS)
        .map(|source| participant_name(session, source))
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
    // This live owner is the direct-LAN path. WOL router timers and recovery
    // remain outside it; the existing controlled-service WOL path is retained.
    let live = mode == SessionMode::Lan && state.match_state.live_network.is_some();
    if live {
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
        if let Some(source) = lowest_record(state) {
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
    let nodes = nodes(&state.match_state.session);
    if row >= nodes.len() {
        return;
    }
    if row == 0 {
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
    state.match_state.session.request_kick_proposal(nodes[row]);
    add_vote(state, nodes[0], nodes[row]);
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
    fn live_wait_uses_reached_entry_and_signed_wrapping_response_delay() {
        let info = LiveWaitInfo {
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
            kind: LiveWaitKind::Ordinary,
            entered_at: 100,
            response_time_at_entry: 141,
        };
        let mut owner = NetworkMatchWait::default();
        owner.observe_live_wait(info);
        let wait = owner.wait.as_mut().unwrap();
        wait.votes.insert(1, vec![0]);
        wait.page = Some(ConnectionPage::default());
        wait.restart_timers(2500);
        owner.observe_live_wait(LiveWaitInfo {
            response_time_at_entry: 500,
            ..info
        });
        let wait = owner.wait.as_ref().unwrap();
        assert_eq!(wait.entry, 100);
        assert_eq!(wait.periods, (423, 2400));
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
            .accept_network_setup(launch, false, Some(7))
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
    fn global_kick_codec_uses_native_house_ids_and_existing_string_conversion() {
        let (session, sim) = vote_fixture();
        let packet = encode_kick_proposal(&session, &sim, 0).unwrap();
        assert_eq!(&packet[..4], &0x1c_u32.to_le_bytes());
        assert_eq!(&packet[4..7], b"G\0\0");
        assert_eq!(&packet[0x2f..0x33], &1_i32.to_le_bytes());
        assert_eq!(&packet[0x33..0x37], &0_i32.to_le_bytes());
        assert_eq!(
            decode_kick_proposal(&session, &sim, &packet),
            Some(NetworkMatchEvent::KickVote {
                voter: 1,
                target: 0
            })
        );
        assert!(decode_kick_proposal(&session, &sim, &packet[..54]).is_none());
        let mut invalid = packet;
        invalid[0x2f..0x33].copy_from_slice(&(-1_i32).to_le_bytes());
        assert!(decode_kick_proposal(&session, &sim, &invalid).is_none());
        invalid = packet;
        invalid[0] = 0x19;
        assert!(decode_kick_proposal(&session, &sim, &invalid).is_none());
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
