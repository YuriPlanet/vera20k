//! The in-match side of a LAN/WOL connection: a peer's departure
//! (`Execute_DoList`'s remote EXIT branch `0x0064C91B`) and
//! `Network__DestroyConnection 0x005DA750`, which the loading timeout and
//! the departure share.
//!
//! It also owns the in-match stall: `Wait_For_Players 0x00648710` (the
//! `0x00648067` call) and its connection dialog 0xEA (proc `0x0064AE50`).
//!
//! The transport is outside VERA: a controlled service delivers what a
//! peer's packets would have decided ([`NetworkMatchEvent`]), and the
//! session's request queue carries what the transport is asked to do.

use std::collections::{BTreeMap, VecDeque};

use crate::app::AppState;
use crate::app::match_runtime::session::{MatchSession, SessionMode};
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
    /// Chat packet `0x0C` from participant `source_index`.
    Chat { source_index: usize, text: String },
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
    // Error 0 outside WOL changes nothing more (0x005DA8F7).
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
        let Some(runtime) = state.match_state.sim_runtime.as_mut() else {
            return;
        };
        let sim = &mut runtime.simulation;
        let name = sim.interner().resolve(owner).to_string();
        let Some(source) = state.match_state.session.network_owner_source(&name) else {
            continue;
        };
        if let Some(destroyed) = destroy_connection(
            &mut state.match_state.session,
            sim,
            source,
            ConnectionError::Left,
        ) {
            post_connection_line(state, destroyed);
        }
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
            crate::app::input::commands::schedule_command_in_sim(
                &mut runtime.simulation,
                &owner,
                crate::sim::command::Command::ExitMatch,
            )
            .ok_or_else(|| anyhow::anyhow!("EXIT for {owner} was not queued"))?;
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
        NetworkMatchEvent::Chat { source_index, text } => {
            crate::app::input::chat::receive(state, source_index, &text);
        }
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

/// A native countdown timer in `WallClock__Get16msBucket` buckets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BucketTimer {
    start: u64,
    period: u64,
}

impl BucketTimer {
    fn start(now: u64, period: u64) -> Self {
        Self { start: now, period }
    }

    fn remaining(self, now: u64) -> u64 {
        self.period.saturating_sub(now.saturating_sub(self.start))
    }

    fn expired(self, now: u64) -> bool {
        self.remaining(now) == 0
    }
}

/// The in-match rows of the `Wait_For_Players` timing table
/// `0x00837340 + 16·GameMode`: +8 dialog delay (a floor under
/// `3·latency`, which is transport) and +C timeout.
fn wait_periods(mode: SessionMode) -> (u64, u64) {
    match mode {
        SessionMode::Wol => (720, 7200),
        _ => (420, 2400),
    }
}

/// One `Wait_For_Players` call.
#[derive(Debug, Clone)]
struct ConnectionWait {
    /// `local_6c0`: the entry bucket; `[0x00AFA418]` is the time since.
    entry: u64,
    /// `local_6f4`: opens 0xEA the first time it expires.
    dialog_timer: BucketTimer,
    /// `local_700`: kicks the lowest record when it expires.
    timeout_timer: BucketTimer,
    /// `[0x00B04898]`: the last countdown second written.
    seconds: u64,
    /// `[0x00A8BB40]`/`[0x00A8BB60]`: voters by target participant.
    votes: BTreeMap<usize, Vec<usize>>,
    /// The result cell `[0x00B04468] == 2` (Quit Game).
    leave: bool,
    /// The open dialog (`bVar2`).
    page: Option<ConnectionPage>,
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

impl NetworkMatchWait {
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

    /// A node's bar lag in buckets (0x0064A7B0): the wait's age at the
    /// dialog's last full repaint for a peer it waits for, 0 for a peer whose
    /// packets still arrive (its record is refreshed by each one).
    pub(crate) fn bar_lag(&self, source_index: usize) -> u32 {
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
    let wol = session.mode() == SessionMode::Wol;
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
pub(crate) fn service_wait(state: &mut AppState, now_ms: u64) -> WaitPass {
    let now = now_ms >> 4;
    let session = &state.match_state.session;
    if !session.mode().is_network() || state.match_state.sim_runtime.is_none() {
        state.match_state.network_wait.wait = None;
        return WaitPass::Running;
    }
    let connected = session.network_connected_source_indices().to_vec();
    let network_wait = &mut state.match_state.network_wait;
    network_wait
        .stalled
        .retain(|peer| connected.contains(&peer.source_index));
    if network_wait.wait.is_none() {
        if network_wait.stalled.is_empty() {
            return WaitPass::Running;
        }
        let (dialog_after, timeout) = wait_periods(session.mode());
        network_wait.wait = Some(ConnectionWait {
            entry: now,
            dialog_timer: BucketTimer::start(now, dialog_after),
            timeout_timer: BucketTimer::start(now, timeout),
            seconds: 0,
            votes: BTreeMap::new(),
            leave: false,
            page: None,
        });
    }
    let wait = network_wait.wait.as_ref().expect("wait started");
    if wait.dialog_timer.expired(now) {
        let elapsed = now.saturating_sub(wait.entry);
        let opening = wait.page.is_none();
        if opening {
            open_dialog(state, elapsed);
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
        if opening || seconds != wait.seconds {
            wait.seconds = seconds;
            if let Some(page) = wait.page.as_mut() {
                // The countdown block posts WM_PAINT on even seconds.
                if seconds % 2 == 0 {
                    page.painted_elapsed = elapsed;
                }
            }
            let text = crate::assets::csf_file::format_csf(
                &csf(state, "TXT_TIME_ALLOWED"),
                &[crate::assets::csf_file::CsfArg::Int(seconds as i64)],
            );
            if let Some(page) = state.match_state.network_wait.page_mut() {
                page.countdown = Some(text);
            }
        }
        if let Some((voter, target)) = state.match_state.network_wait.received_votes.pop_front() {
            add_vote(state, voter, target);
        }
        tally_votes(state);
    }
    let timed_out = state
        .match_state
        .network_wait
        .wait
        .as_ref()
        .is_some_and(|wait| wait.timeout_timer.expired(now));
    if timed_out {
        // 0x0064B410: LAN/WOL kick the lowest record with error 1 and the
        // wait restarts its timers with the dialog left open.
        if let Some(source) = lowest_record(state) {
            kick(state, source, ConnectionError::Lost);
        }
        let (dialog_after, timeout) = wait_periods(state.match_state.session.mode());
        if let Some(wait) = state.match_state.network_wait.wait.as_mut() {
            wait.dialog_timer = BucketTimer::start(now, dialog_after);
            wait.timeout_timer = BucketTimer::start(now, timeout);
        }
    }
    // All frames present, or the local player alone (`0x00649BAC`): close
    // 0xEA and continue.
    let connected = state.match_state.session.network_connected_source_indices();
    let network_wait = &mut state.match_state.network_wait;
    let present = !network_wait
        .stalled
        .iter()
        .any(|peer| connected.contains(&peer.source_index));
    if present || connected.len() <= 1 {
        network_wait.wait = None;
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

/// A row button (`0x0064AAE0`): the local row explains itself; another row
/// sends the proposal to every other node and counts the local vote.
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
    // RESIDUAL: WOL tournament games answer TXT_CANT_KICK; VERA has no
    // tournament flag.
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
        let dialog = BucketTimer::start(entry, dialog_after);
        let timer = BucketTimer::start(entry, timeout);
        assert!(!dialog.expired(entry + dialog_after - 1));
        assert!(dialog.expired(entry + dialog_after));
        assert_eq!(timer.remaining(entry + dialog_after) / 60, 33);
        assert_eq!(timer.remaining(entry + timeout - 1) / 60, 0);
        assert!(!timer.expired(entry + timeout - 1));
        assert!(timer.expired(entry + timeout));
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
