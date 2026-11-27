//! The LAN Cooperative result dialog's readiness and handoff live with the
//! frozen connection roster in MatchSession. The native BC6 exchange retains
//! its Begin, Options, Ack and Go records. VERA commits only after each
//! playing connection has acknowledged, and retires a stalled handoff.
//!
//! Bodies: the loop `CampaignScoreClass vt+4` (`0x0046FC00`) and
//! `Dialog0xBC6__DlgProc` (`0x0046DFD0`). Peers arrive as decoded
//! [`CooperativeRoundEvent`]s and every packet this side sends is a
//! [`NetworkSendRequest`]; `lan_service::cooperative` moves both.

use super::{
    ConnectionRoster, CooperativeResume, MatchSession, NetworkPhase, NetworkReturnContext,
    NetworkRoomReturn, NetworkSendRequest, NetworkSession, WallBucketTimer,
};
use crate::net::lan_messages::cooperative::ROWS;
use crate::sim::campaign_carry_over::CampaignCarryOver;
use crate::skirmish_launch::{
    LaunchStartPosition, SkirmishAiSlot, SkirmishLaunchSession, SkirmishLocalSlot,
};
use std::sync::Arc;

/// BC6's reliable handoff uses a wall-clock wait independent of Logic time.
const ROUND_START_WAIT: u32 = 600;

#[derive(Debug, Clone, PartialEq, Eq)]
enum RoundStart {
    Voting,
    Assigning,
    HostAwaiting {
        pending: Vec<usize>,
        deadline: WallBucketTimer,
    },
    GuestAwaiting {
        acknowledged: bool,
        deadline: WallBucketTimer,
    },
}

/// BC6's round state, owned by the `CooperativeRound` phase. The row
/// status statics (`TXT_READY`/`TXT_WAITING`/`TXT_ABORTED` in
/// `0x70A`/`0x70B`) are template-hidden and never shown, so their texts are
/// not kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CooperativeRoundState {
    /// `0x89E000[8]`, by raw slot: empty slots start ready
    /// (`0x0046EF06..0x0046EF4A`), and every un-ready pass clears human
    /// slots only. Start admission keeps every human row below [`ROWS`].
    ready: [bool; ROWS],
    /// `0x89E05C`.
    local_ready: bool,
    start: RoundStart,
    /// `0x89E05D`: false on the campaign's last mission.
    can_continue: bool,
    /// `A8D580`, the human House count of the mission's rows.
    rows: usize,
    next_scenario: usize,
    /// `0x89E04C` once the loop left: true runs the next mission.
    outcome: Option<bool>,
    /// The won mission's single existing record (0x004C6180). A defeat
    /// leaves this empty and reuses the previous admission on retry.
    saved: Option<CampaignCarryOver>,
}

impl CooperativeRoundState {
    pub(crate) fn local_ready(&self) -> bool {
        self.local_ready
    }

    pub(crate) fn quit_enabled(&self) -> bool {
        self.outcome.is_none()
            && !matches!(
                self.start,
                RoundStart::GuestAwaiting {
                    acknowledged: true,
                    ..
                }
            )
    }

    fn all_ready(&self) -> bool {
        self.ready.iter().all(|ready| *ready)
    }
}

/// The master's options packet `0x32` (`0x0046D4A0`) as the transport
/// decodes it: the difficulty byte `+0x2F`, each human slot's start,
/// country and colour (`+0x30`/`+0x50`/`+0x70`), and the AI arrays
/// (`+0x90..+0x110`) as the AI rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CooperativeRoundOptions {
    pub(crate) difficulty: i32,
    /// `(raw slot, choices)` of every human row.
    pub(crate) humans: Vec<(u8, SkirmishLocalSlot)>,
    pub(crate) ai_player_count: i32,
    pub(crate) opponents: Vec<SkirmishAiSlot>,
}

/// A decoded BC6 packet from participant `source`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CooperativeRoundEvent {
    /// `0x26`.
    PeerReady { source: usize },
    /// `0x25`.
    PeerWaiting { source: usize },
    /// `0x24`.
    PeerLeft { source: usize },
    /// `0x34`, the master's begin.
    Begin { now: u32 },
    /// `0x32`.
    Options {
        options: CooperativeRoundOptions,
        now: u32,
    },
    /// `0x23`, a guest's ack of the begin.
    Ack { source: usize },
    /// `0x31`, the master's go.
    Go,
    /// `0x0C` of this game (its `+0x114` hash matched), with the line
    /// colour at `+0x110`.
    Chat {
        source: usize,
        text: String,
        color: u32,
    },
    /// `0x0D`, the 20 s keepalive: only the loop's tail reads it.
    KeepAlive,
}

/// What the app must do after a round step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CooperativeRoundStep {
    None,
    /// The master's all-ready (`0x0046FD47`): run mode `+0x68` and
    /// `0x0069B8C0`, then [`MatchSession::cooperative_round_assigned`],
    /// which queues `0x34` and `0x32`.
    Assign,
    /// A `0x32` was applied; `difficulty` is its byte, and `changed` cleared
    /// every human's readiness.
    OptionsApplied {
        difficulty: i32,
        changed: bool,
    },
    /// `0x24`: the row shows `TXT_ABORTED`; post `GUI:PlayerLeftCoop`.
    PeerLeft {
        source: usize,
    },
    /// `0x0C`: post the line in the packet's colour.
    Chat {
        source: usize,
        text: String,
        color: u32,
    },
    /// The loop left with `0x89E04C`.
    Exit(bool),
    /// The admission deadline expired and a scoped Departure was queued.
    TimedOut,
    /// VERA: Continue on WOL left with result 0 for the room the players
    /// return to, which its host sets to the next stage.
    ContinueInRoom,
}

impl MatchSession {
    /// If the post-match departure was lost, the continuation still needs
    /// every playing connection that committed the handoff. First starts
    /// keep the native timeout path, which can retire an absent player.
    pub(crate) fn retire_cooperative_start_on_timeout(&mut self, timed_out: &[usize]) -> bool {
        let Some(NetworkPhase::Frozen { startup, .. }) =
            self.network.as_ref().map(|network| &network.phase)
        else {
            return false;
        };
        // Only freeze_cooperative_round installs a network carry-over, even
        // for a retry that restores the cleared record.
        if startup.mode() != super::SessionMode::Lan || startup.carry_over().is_none() {
            return false;
        }
        timed_out
            .iter()
            .any(|&source| self.retire_network_start_on_departure(source))
    }

    /// GameExit's co-op arm enters BC6 from the finished match
    /// (`0x0046FC00` setup and the 0x497 init `0x0046EBA7`). The round
    /// takes the frozen roster with `next_map_file` (`+0xA0`'s
    /// SelectScenario) and every human's and AI's start reset to Random
    /// (`0x00696FF0(-2)`, `A8B2DC[i] = -2`). `rows` is the mission's human
    /// House count.
    pub(crate) fn enter_cooperative_round(
        &mut self,
        next_map_file: Option<String>,
        can_continue: bool,
        rows: usize,
        next_scenario: usize,
        saved: Option<CampaignCarryOver>,
    ) -> anyhow::Result<()> {
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
        let NetworkPhase::Frozen {
            startup,
            connections,
            room_return,
            ..
        } = &network.phase
        else {
            anyhow::bail!("the co-op round follows a frozen match");
        };
        let mut launch = startup.launch_session().clone();
        if let Some(file) = next_map_file {
            launch.selected_map_file = Some(file);
        }
        for human in &mut launch.humans {
            human.slot.start_position = LaunchStartPosition::Auto;
        }
        // RESIDUAL: the 0x497 init (`0x0046EBA7`) also writes every AI
        // country and colour to -1; only the starts reset here. Rows below
        // the next map's NumCoopHumanStartSpots keep their last values in
        // `prepare_ai`, which changes the `0x0069B8C0` draws only when that
        // count grows between missions. Every retail co-op map sets 2.
        for ai in &mut launch.opponents {
            ai.start_position = LaunchStartPosition::Auto;
        }
        launch.use_live_house_rows();
        // 0x0046EF06: empty slots ready, every bound human slot not.
        let mut ready = [true; ROWS];
        for (index, human) in launch.humans.iter().enumerate() {
            if let Some(row) = human.raw_slot_index
                && connections.playing_sources.contains(&index)
                && let Some(slot) = ready.get_mut(usize::from(row))
            {
                *slot = false;
            }
        }
        let round = CooperativeRoundState {
            ready,
            local_ready: false,
            start: RoundStart::Voting,
            can_continue,
            rows,
            next_scenario,
            outcome: None,
            saved,
        };
        network.phase = NetworkPhase::CooperativeRound {
            previous: Arc::clone(startup),
            connections: connections.clone(),
            launch,
            round,
            room_return: room_return.clone(),
        };
        network.send_requests.clear();
        network.loading_reports.clear();
        Ok(())
    }

    /// The repaint's country pass `0x0046F6D0`: each human slot's country
    /// goes through `admit`, which may replace a country the next stage
    /// does not list (`SetCountry`, a state write).
    pub(crate) fn admit_cooperative_round_countries(
        &mut self,
        admit: impl Fn(&mut SkirmishLocalSlot),
    ) -> anyhow::Result<()> {
        let RoundParts { launch, .. } = round_parts(self)?;
        for human in &mut launch.humans {
            admit(&mut human.slot);
        }
        Ok(())
    }

    /// The master's bound human slots begin unready, and `0x89E000` is a
    /// slot array: whether `source_index`'s slot is ready.
    pub(crate) fn cooperative_round_slot_ready(&self, source_index: usize) -> bool {
        match self.network.as_ref().map(|network| &network.phase) {
            Some(NetworkPhase::CooperativeRound { launch, round, .. }) => launch
                .humans
                .get(source_index)
                .and_then(|human| human.raw_slot_index)
                .and_then(|row| round.ready.get(usize::from(row)))
                .copied()
                .unwrap_or(false),
            _ => false,
        }
    }

    pub(crate) fn cooperative_round(&self) -> Option<&CooperativeRoundState> {
        match &self.network.as_ref()?.phase {
            NetworkPhase::CooperativeRound { round, .. } => Some(round),
            _ => None,
        }
    }

    /// `0x0046FB50`: Continue is enabled while every row's player is still
    /// connected and the local player is not ready.
    pub(crate) fn cooperative_round_continue_enabled(&self) -> bool {
        if self.local_connection_watches() {
            return false;
        }
        match self.network.as_ref().map(|network| &network.phase) {
            Some(NetworkPhase::CooperativeRound {
                connections,
                launch,
                round,
                ..
            }) => {
                round.outcome.is_none()
                    && matches!(round.start, RoundStart::Voting)
                    && !round.local_ready
                    && connections.playing_sources.len() >= round.rows
                    && connections
                        .playing_sources
                        .contains(&launch.local_player_index)
            }
            _ => false,
        }
    }

    /// Continue `0x6D1` (`0x0046E743`) and the loop's slot-1 pass
    /// (`0x0046FD61`). The button disables itself once ready, so only the
    /// ready arm is reachable. BC6's unimplemented choice editors leave
    /// nothing to publish on a vote. Send the options once after assignment,
    /// so a reordered `0x32` always acknowledges that assignment.
    /// VERA: a WOL match's relay ends with its mission, so BC6's records
    /// have no transport and `+0xAC` no session to continue. A round with a
    /// stage left leaves at once with result 0 for the players' room.
    pub(crate) fn continue_cooperative_round(&mut self) -> anyhow::Result<CooperativeRoundStep> {
        let host = self.host;
        let online = self
            .network
            .as_ref()
            .is_some_and(|network| network.return_context == NetworkReturnContext::Wol);
        anyhow::ensure!(
            self.cooperative_round_continue_enabled(),
            "the co-op round is not accepting this vote"
        );
        let RoundParts {
            requests,
            launch,
            round,
            room_return,
            ..
        } = round_parts(self)?;
        anyhow::ensure!(round.outcome.is_none(), "the result dialog has closed");
        if online && round.can_continue {
            if let NetworkRoomReturn::Wol { cooperative, .. } = room_return {
                *cooperative = Some(CooperativeResume {
                    mode_id: launch.mode.id,
                    scenario: round.next_scenario,
                });
            }
            round.outcome = Some(false);
            return Ok(CooperativeRoundStep::ContinueInRoom);
        }
        let local = launch.local_player_index;
        round.local_ready = true;
        if let Some(row) = launch
            .humans
            .get(local)
            .and_then(|human| human.raw_slot_index)
            .and_then(|row| round.ready.get_mut(usize::from(row)))
        {
            *row = true;
        }
        requests.push(NetworkSendRequest::CooperativeReady);
        if host && round.all_ready() {
            if !round.can_continue {
                round.outcome = Some(false);
                return Ok(CooperativeRoundStep::Exit(false));
            }
            round.start = RoundStart::Assigning;
            return Ok(CooperativeRoundStep::Assign);
        }
        if !round.can_continue {
            round.outcome = Some(false);
            return Ok(CooperativeRoundStep::Exit(false));
        }
        Ok(CooperativeRoundStep::None)
    }

    /// The master's all-ready tail after mode `+0x68` and `0x0069B8C0`
    /// changed the round's roster: the begin `0x34`, its own ack and the
    /// options `0x32`. Native sends `0x34` before the assignment, which
    /// cannot fail there; VERA queues it only once the assignment
    /// succeeded, so a refused one sends no begin. Both leave in the same
    /// order before any packet is read.
    pub(crate) fn cooperative_round_assigned(
        &mut self,
        difficulty: i32,
        now: u32,
    ) -> anyhow::Result<CooperativeRoundStep> {
        anyhow::ensure!(self.host, "only the master assigns the round");
        let RoundParts {
            requests,
            connections,
            launch,
            round,
            ..
        } = round_parts(self)?;
        anyhow::ensure!(
            round.start == RoundStart::Assigning,
            "no co-op handoff to assign"
        );
        let pending: Vec<_> = connections
            .playing_sources
            .iter()
            .copied()
            .filter(|source| *source != launch.local_player_index)
            .collect();
        if pending.is_empty() {
            round.outcome = Some(true);
            return Ok(CooperativeRoundStep::Exit(true));
        }
        requests.push(NetworkSendRequest::CooperativeBegin);
        round.start = RoundStart::HostAwaiting {
            pending,
            deadline: WallBucketTimer::start(now, ROUND_START_WAIT),
        };
        let options = round_options(launch, difficulty);
        requests.push(NetworkSendRequest::CooperativeOptions(options));
        Ok(CooperativeRoundStep::None)
    }

    /// Quit `0x5C0` (`0x0046E28C`): `0x24` to every node and result 0.
    pub(crate) fn quit_cooperative_round(&mut self) -> anyhow::Result<()> {
        let RoundParts {
            requests, round, ..
        } = round_parts(self)?;
        if round.outcome.is_none() {
            round.outcome = Some(false);
            let request = if round.start == RoundStart::Voting {
                NetworkSendRequest::CooperativeLeave
            } else {
                NetworkSendRequest::CooperativeAbort
            };
            requests.push(request);
        }
        Ok(())
    }

    pub(crate) fn poll_cooperative_round_start(&mut self, now: u32) -> CooperativeRoundStep {
        let Ok(RoundParts {
            requests, round, ..
        }) = round_parts(self)
        else {
            return CooperativeRoundStep::None;
        };
        let deadline = match round.start {
            RoundStart::HostAwaiting { deadline, .. }
            | RoundStart::GuestAwaiting { deadline, .. } => deadline,
            RoundStart::Voting | RoundStart::Assigning => return CooperativeRoundStep::None,
        };
        if round.outcome.is_some() || deadline.remaining(now) != 0 {
            return CooperativeRoundStep::None;
        }
        round.outcome = Some(false);
        requests.push(NetworkSendRequest::CooperativeAbort);
        CooperativeRoundStep::TimedOut
    }

    /// One packet through the loop's dispatch (`0x0047009F`).
    /// `difficulty` is the current `[0xA8EB68]`, which `0x32` compares.
    pub(crate) fn cooperative_round_event(
        &mut self,
        event: CooperativeRoundEvent,
        difficulty: i32,
    ) -> anyhow::Result<CooperativeRoundStep> {
        let host = self.host;
        let RoundParts {
            requests,
            connections,
            launch,
            round,
            ..
        } = round_parts(self)?;
        if round.outcome.is_some() {
            return Ok(CooperativeRoundStep::None);
        }
        let slot_of = |launch: &SkirmishLaunchSession, source: usize| {
            launch
                .humans
                .get(source)
                .and_then(|human| human.raw_slot_index)
                .map(usize::from)
        };
        match event {
            CooperativeRoundEvent::PeerReady { source } => {
                if round.start != RoundStart::Voting
                    || !connections.playing_sources.contains(&source)
                {
                    return Ok(CooperativeRoundStep::None);
                }
                if let Some(ready) =
                    slot_of(launch, source).and_then(|row| round.ready.get_mut(row))
                {
                    *ready = true;
                }
                if host
                    && round.local_ready
                    && connections.playing_sources.len() >= round.rows
                    && round.all_ready()
                    && round.can_continue
                {
                    round.start = RoundStart::Assigning;
                    return Ok(CooperativeRoundStep::Assign);
                }
                Ok(CooperativeRoundStep::None)
            }
            CooperativeRoundEvent::PeerWaiting { source } => {
                if round.start == RoundStart::Voting
                    && connections.playing_sources.contains(&source)
                    && let Some(ready) =
                        slot_of(launch, source).and_then(|row| round.ready.get_mut(row))
                {
                    *ready = false;
                }
                Ok(CooperativeRoundStep::None)
            }
            CooperativeRoundEvent::PeerLeft { source } => {
                if !connections.playing_sources.contains(&source) {
                    return Ok(CooperativeRoundStep::None);
                }
                if let Some(ready) =
                    slot_of(launch, source).and_then(|row| round.ready.get_mut(row))
                {
                    *ready = false;
                }
                // The node is deleted (A8DA84--); its slot stays unready,
                // so all-ready can no longer pass. Continue is disabled
                // (by the connected count) and Quit enabled again.
                connections.playing_sources.retain(|index| *index != source);
                round.start = RoundStart::Voting;
                Ok(CooperativeRoundStep::PeerLeft { source })
            }
            CooperativeRoundEvent::Begin { now } => {
                if !host
                    && round.start == RoundStart::Voting
                    && round.local_ready
                    && round.can_continue
                {
                    round.start = RoundStart::GuestAwaiting {
                        acknowledged: false,
                        deadline: WallBucketTimer::start(now, ROUND_START_WAIT),
                    };
                }
                Ok(CooperativeRoundStep::None)
            }
            CooperativeRoundEvent::Options { options, now } => {
                if host {
                    return Ok(CooperativeRoundStep::None);
                }
                // Begin and Options use the reliable global datagram queue,
                // which can deliver Options first. An authenticated options
                // packet for our vote also proves the host began the handoff.
                let begin_on_options =
                    round.start == RoundStart::Voting && round.local_ready && round.can_continue;
                let changed = apply_round_options(launch, &options, difficulty)?;
                if begin_on_options {
                    round.start = RoundStart::GuestAwaiting {
                        acknowledged: false,
                        deadline: WallBucketTimer::start(now, ROUND_START_WAIT),
                    };
                }
                if changed {
                    round.local_ready = false;
                    clear_human_readiness(launch, &connections.playing_sources, round);
                }
                if let RoundStart::GuestAwaiting { acknowledged, .. } = &mut round.start {
                    *acknowledged = true;
                    requests.push(NetworkSendRequest::CooperativeAck);
                }
                Ok(CooperativeRoundStep::OptionsApplied {
                    difficulty: options.difficulty,
                    changed,
                })
            }
            CooperativeRoundEvent::Ack { source } => {
                if let RoundStart::HostAwaiting { pending, .. } = &mut round.start
                    && let Some(index) = pending.iter().position(|candidate| *candidate == source)
                {
                    pending.remove(index);
                    if !pending.is_empty() {
                        return Ok(CooperativeRoundStep::None);
                    }
                    round.outcome = Some(true);
                    requests.push(NetworkSendRequest::CooperativeGo);
                    return Ok(CooperativeRoundStep::Exit(true));
                }
                Ok(CooperativeRoundStep::None)
            }
            CooperativeRoundEvent::Go => {
                if !matches!(
                    round.start,
                    RoundStart::GuestAwaiting {
                        acknowledged: true,
                        ..
                    }
                ) {
                    return Ok(CooperativeRoundStep::None);
                }
                round.outcome = Some(true);
                Ok(CooperativeRoundStep::Exit(true))
            }
            CooperativeRoundEvent::Chat {
                source,
                text,
                color,
            } => Ok(CooperativeRoundStep::Chat {
                source,
                text,
                color,
            }),
            CooperativeRoundEvent::KeepAlive => Ok(CooperativeRoundStep::None),
        }
    }

    /// Cooperative `+0xAC` and the next Start_Scenario: a won (or retried)
    /// round freezes its roster as the next immutable start, with the same
    /// agreed seed (`g_RngSeed = mode+0x9C`, `0x005C4890`), the
    /// continuation's MaxAhead and LatencyFudge, no WOL terms
    /// (`0x005C46F5`) and the live membership.
    /// A won mission's fresh carry-over record is captured at `0x004C6180`. A
    /// defeat restores the stale one: the last continuation's, or the
    /// cleared record of `0x004C6140`.
    pub(crate) fn freeze_cooperative_round(
        &mut self,
        max_ahead_frames: i32,
        latency_fudge: i32,
    ) -> anyhow::Result<crate::app::match_bootstrap::LoadingStartup> {
        let host = self.host;
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
        let NetworkPhase::CooperativeRound {
            previous,
            launch,
            round,
            ..
        } = &network.phase
        else {
            anyhow::bail!("no co-op round to continue");
        };
        anyhow::ensure!(round.outcome == Some(true), "the round did not continue");
        super::admit_network_start_roster(launch)?;
        let seed = previous.seed.value;
        let mode = previous.mode();
        let carry_over = round
            .saved
            .or_else(|| previous.carry_over().copied())
            .unwrap_or(crate::sim::campaign_carry_over::CampaignCarryOver::CLEARED);
        let NetworkPhase::CooperativeRound {
            launch,
            connections,
            room_return,
            ..
        } = std::mem::replace(&mut network.phase, NetworkPhase::lobby())
        else {
            unreachable!()
        };
        let startup = Arc::new(
            crate::app::match_bootstrap::PreparedNetworkStartup::new(
                launch,
                seed,
                mode,
                host,
                max_ahead_frames,
                super::WolStartTerms::default(),
            )
            .with_carry_over(carry_over),
        );
        network.max_ahead_frames = Some(max_ahead_frames);
        network.wol_start = super::WolStartTerms::default();
        network.phase = NetworkPhase::Frozen {
            startup: Arc::clone(&startup),
            connections,
            room_return,
            campaign_abort_pending: false,
        };
        self.staged_tiberium = crate::sim::scenario_session::TiberiumFlags::NETWORK_PREGAME;
        self.latency_fudge = latency_fudge;
        Ok(crate::app::match_bootstrap::LoadingStartup::Network(
            startup,
        ))
    }
}

/// The round's fields, split from the visit so a step can queue requests
/// while it edits the round.
struct RoundParts<'a> {
    requests: &'a mut Vec<NetworkSendRequest>,
    connections: &'a mut ConnectionRoster,
    launch: &'a mut SkirmishLaunchSession,
    round: &'a mut CooperativeRoundState,
    room_return: &'a mut NetworkRoomReturn,
}

fn round_parts(session: &mut MatchSession) -> anyhow::Result<RoundParts<'_>> {
    let NetworkSession {
        send_requests,
        phase,
        ..
    } = session
        .network
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
    let NetworkPhase::CooperativeRound {
        connections,
        launch,
        round,
        room_return,
        ..
    } = phase
    else {
        anyhow::bail!("no co-op result dialog");
    };
    Ok(RoundParts {
        requests: send_requests,
        connections,
        launch,
        round,
        room_return,
    })
}

fn clear_human_readiness(
    launch: &SkirmishLaunchSession,
    playing_sources: &[usize],
    round: &mut CooperativeRoundState,
) {
    for source in playing_sources {
        if let Some(ready) = launch
            .humans
            .get(*source)
            .and_then(|human| human.raw_slot_index)
            .and_then(|row| round.ready.get_mut(usize::from(row)))
        {
            *ready = false;
        }
    }
}

/// `0x0046D4A0`'s payload from the round roster.
fn round_options(launch: &SkirmishLaunchSession, difficulty: i32) -> CooperativeRoundOptions {
    CooperativeRoundOptions {
        difficulty,
        humans: launch
            .humans
            .iter()
            .filter_map(|human| Some((human.raw_slot_index?, human.slot.clone())))
            .collect(),
        ai_player_count: launch.ai_player_count,
        opponents: launch.opponents.clone(),
    }
}

/// `0x0046D650`: the master's values replace every bound slot's start,
/// country and colour (the local slot's too) and the AI rows. True when
/// anything, the difficulty included, changed.
fn apply_round_options(
    launch: &mut SkirmishLaunchSession,
    options: &CooperativeRoundOptions,
    difficulty: i32,
) -> anyhow::Result<bool> {
    // Validated on a copy, committed on success: a refused change leaves the
    // session as it was.
    let mut candidate = launch.clone();
    let mut changed = options.difficulty != difficulty;
    for (row, choices) in &options.humans {
        let Some(human) = candidate
            .humans
            .iter_mut()
            .find(|human| human.raw_slot_index == Some(*row))
        else {
            continue;
        };
        let slot = &mut human.slot;
        let before = slot.clone();
        slot.start_position = choices.start_position;
        slot.country = choices.country;
        slot.country_random = choices.country_random;
        slot.color_index = choices.color_index;
        slot.color_random = choices.color_random;
        changed |= *slot != before;
    }
    changed |= candidate.opponents != options.opponents
        || candidate.ai_player_count != options.ai_player_count;
    candidate.opponents = options.opponents.clone();
    candidate.ai_player_count = options.ai_player_count;
    super::validate_network_rows(&candidate)?;
    candidate.opponents.sort_by_key(|ai| ai.raw_slot_index);
    *launch = candidate;
    Ok(changed)
}

#[cfg(all(test, tests_app_match))]
mod tests {
    use super::*;
    use crate::app::match_bootstrap::LoadingStartup;
    use crate::app::match_runtime::session::NetworkReturnContext;

    const SEED: u32 = 0x1234_abcd;

    /// A frozen two-player LAN start; `local` is the local source index.
    fn frozen(
        local: usize,
    ) -> (
        MatchSession,
        Arc<crate::app::match_bootstrap::PreparedNetworkStartup>,
    ) {
        frozen_in(NetworkReturnContext::LanLobby, local)
    }

    fn frozen_in(
        context: NetworkReturnContext,
        local: usize,
    ) -> (
        MatchSession,
        Arc<crate::app::match_bootstrap::PreparedNetworkStartup>,
    ) {
        let mut launch = super::super::tests::network_fixture();
        launch.local_player_index = local;
        let mut session = MatchSession::default();
        session.enter_network(context);
        session
            .accept_network_setup(launch, local == 0, Some(7), false)
            .unwrap();
        session.set_network_start_seed(SEED).unwrap();
        session.prepare_network_timing(2, || 0).unwrap();
        let LoadingStartup::Network(startup) = session.freeze_network_start(SEED, None).unwrap()
        else {
            panic!("network start");
        };
        (session, startup)
    }

    #[test]
    fn a_watcher_cannot_enable_or_submit_the_followed_sources_vote() {
        let (mut session, _) = frozen_in(NetworkReturnContext::Wol, 1);
        session
            .enter_cooperative_round(Some("C2S02MD.MAP".into()), true, 2, 1, None)
            .unwrap();
        assert!(session.cooperative_round_continue_enabled());
        let endpoint = std::net::SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, 1237);
        session.admit_local_network_watcher(endpoint).unwrap();
        assert!(!session.cooperative_round_continue_enabled());
        assert!(session.continue_cooperative_round().is_err());
        assert!(session.take_network_send_requests().is_empty());
        assert!(session.disconnect_network_watcher(endpoint));
        assert!(session.cooperative_round_continue_enabled());
    }

    #[test]
    fn a_solo_lan_host_continues_when_its_assignment_completes() {
        let mut launch = super::super::tests::network_fixture();
        launch.humans.truncate(1);
        launch.local_player_index = 0;
        let mut session = MatchSession::default();
        session.enter_network(NetworkReturnContext::LanLobby);
        session
            .accept_network_setup(launch, true, Some(7), false)
            .unwrap();
        session.prepare_network_timing(2, || 0).unwrap();
        session.freeze_network_start(SEED, None).unwrap();
        session
            .enter_cooperative_round(Some("C2S02MD.MAP".into()), true, 1, 1, None)
            .unwrap();
        assert_eq!(
            session.continue_cooperative_round().unwrap(),
            CooperativeRoundStep::Assign
        );
        session.take_network_send_requests();
        assert_eq!(
            session.cooperative_round_assigned(1, 100).unwrap(),
            CooperativeRoundStep::Exit(true)
        );
        assert!(session.take_network_send_requests().is_empty());
        assert!(session.freeze_cooperative_round(30, 2).is_ok());
    }

    #[test]
    fn master_round_begins_on_the_guests_ready_and_leaves_on_its_ack() {
        let (mut session, first) = frozen(0);
        session
            .enter_cooperative_round(Some("C2S02MD.MAP".into()), true, 2, 0, None)
            .unwrap();
        let launch = session.network_setup().unwrap();
        assert_eq!(launch.selected_map_file.as_deref(), Some("C2S02MD.MAP"));
        assert!(
            launch
                .humans
                .iter()
                .all(|human| human.slot.start_position == LaunchStartPosition::Auto)
        );
        assert!(session.cooperative_round_continue_enabled());

        assert_eq!(
            session.continue_cooperative_round().unwrap(),
            CooperativeRoundStep::None
        );
        assert!(!session.cooperative_round_continue_enabled());
        assert_eq!(
            session.take_network_send_requests(),
            vec![NetworkSendRequest::CooperativeReady]
        );

        // A keepalive before all-ready reaches the tail with nothing to do.
        assert_eq!(
            session
                .cooperative_round_event(CooperativeRoundEvent::KeepAlive, 1)
                .unwrap(),
            CooperativeRoundStep::None
        );
        assert_eq!(
            session
                .cooperative_round_event(CooperativeRoundEvent::PeerReady { source: 1 }, 1)
                .unwrap(),
            CooperativeRoundStep::Assign
        );
        assert!(
            session.take_network_send_requests().is_empty(),
            "the begin waits for the assignment"
        );
        session.cooperative_round_assigned(1, 100).unwrap();
        let requests = session.take_network_send_requests();
        assert!(matches!(
            requests.as_slice(),
            [
                NetworkSendRequest::CooperativeBegin,
                NetworkSendRequest::CooperativeOptions(_),
            ]
        ));
        assert_eq!(
            session
                .cooperative_round_event(CooperativeRoundEvent::Ack { source: 1 }, 1)
                .unwrap(),
            CooperativeRoundStep::Exit(true)
        );
        let LoadingStartup::Network(next) = session.freeze_cooperative_round(30, 2).unwrap() else {
            panic!("network start");
        };
        assert_eq!(
            session.take_network_send_requests(),
            vec![NetworkSendRequest::CooperativeGo],
            "the go outlives the freeze for the transport"
        );
        session.assert_network_start(&next);
        assert_eq!(next.seed.value, first.seed.value);
        assert_eq!(next.max_ahead_frames(), 30);
        assert_eq!(
            next.carry_over(),
            Some(&crate::sim::campaign_carry_over::CampaignCarryOver::CLEARED),
            "a first continuation with no saved record restores the cleared one"
        );
        assert_eq!(session.latency_fudge(), 2);
        assert_eq!(
            next.launch_session().selected_map_file.as_deref(),
            Some("C2S02MD.MAP")
        );
        assert_eq!(session.network_connected_source_indices(), &[0, 1]);
    }

    #[test]
    fn watcher_departure_during_the_retry_vote_keeps_the_playing_quorum() {
        let (mut host, mut guest) = voting_peers();
        let watcher = std::net::SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, 1237);
        let Some(NetworkPhase::CooperativeRound { connections, .. }) =
            host.network.as_mut().map(|visit| &mut visit.phase)
        else {
            panic!("cooperative round");
        };
        connections
            .watching_sources
            .push(super::super::ConnectionWatcher {
                endpoint: watcher,
                local: false,
            });

        assert_eq!(
            host.cooperative_round_event(CooperativeRoundEvent::PeerReady { source: 1 }, 0)
                .unwrap(),
            CooperativeRoundStep::Assign
        );
        assert!(host.disconnect_network_watcher(watcher));
        assert_eq!(host.network_connected_source_indices(), &[0, 1]);
        host.cooperative_round_assigned(0, 100).unwrap();
        receive_assigned_options(&mut host, &mut guest, 105, false);

        // Irrelevant packets and observers' stale ACKs cannot commit before
        // the genuine guest has acknowledged the applied assignment.
        for event in [
            CooperativeRoundEvent::KeepAlive,
            CooperativeRoundEvent::Ack { source: 2 },
        ] {
            assert_eq!(
                host.cooperative_round_event(event, 0).unwrap(),
                CooperativeRoundStep::None
            );
        }
        assert_eq!(
            host.cooperative_round_event(CooperativeRoundEvent::Ack { source: 1 }, 0)
                .unwrap(),
            CooperativeRoundStep::Exit(true)
        );
        assert_eq!(
            host.take_network_send_requests(),
            vec![NetworkSendRequest::CooperativeGo]
        );
        assert_eq!(
            guest
                .cooperative_round_event(CooperativeRoundEvent::Go, 0)
                .unwrap(),
            CooperativeRoundStep::Exit(true)
        );
        assert!(host.freeze_cooperative_round(30, 1).is_ok());
        let LoadingStartup::Network(guest_start) = guest.freeze_cooperative_round(30, 1).unwrap()
        else {
            panic!("guest started the next load");
        };
        guest.admit_local_network_watcher(watcher).unwrap();
        assert!(!guest.retire_network_start_on_departure(0));
        assert!(guest.admits_network_start(&guest_start));
    }

    #[test]
    fn guest_timeout_after_host_go_retires_both_loading_admissions() {
        let (mut host, mut guest) = ready_to_vote_peers();
        assert_eq!(
            guest.continue_cooperative_round().unwrap(),
            CooperativeRoundStep::None
        );
        assert_eq!(
            guest.take_network_send_requests(),
            vec![NetworkSendRequest::CooperativeReady]
        );
        assert_eq!(
            host.cooperative_round_event(CooperativeRoundEvent::PeerReady { source: 1 }, 0)
                .unwrap(),
            CooperativeRoundStep::None
        );
        assert_eq!(
            host.continue_cooperative_round().unwrap(),
            CooperativeRoundStep::Assign
        );
        assert_eq!(
            host.take_network_send_requests(),
            vec![NetworkSendRequest::CooperativeReady],
            "before assignment, the host cannot send options that the voted guest would ACK"
        );
        host.cooperative_round_assigned(0, 100).unwrap();
        receive_assigned_options(&mut host, &mut guest, 105, true);
        assert_eq!(
            host.cooperative_round_event(CooperativeRoundEvent::Ack { source: 1 }, 0)
                .unwrap(),
            CooperativeRoundStep::Exit(true)
        );
        assert_eq!(
            host.take_network_send_requests(),
            vec![NetworkSendRequest::CooperativeGo]
        );
        let LoadingStartup::Network(host_start) = host.freeze_cooperative_round(30, 1).unwrap()
        else {
            panic!("host started the next load");
        };

        // The guest did not receive Go. Expiry wins locally and emits the
        // scoped departure the loader authenticates against this roster.
        assert_eq!(
            guest.poll_cooperative_round_start(105 + ROUND_START_WAIT),
            CooperativeRoundStep::TimedOut
        );
        assert_eq!(
            guest.take_network_send_requests(),
            vec![NetworkSendRequest::CooperativeAbort]
        );
        assert_eq!(
            guest
                .cooperative_round_event(CooperativeRoundEvent::Go, 0)
                .unwrap(),
            CooperativeRoundStep::None
        );
        assert!(guest.freeze_cooperative_round(30, 1).is_err());
        assert!(host.admits_network_start(&host_start));
        // If the scoped Departure is also lost, the host's ordinary loading
        // deadline must retire the whole attempt instead of starting solo.
        let mut host_without_departure = host.clone();
        assert!(!host_without_departure.retire_cooperative_start_on_timeout(&[0, 2]));
        assert!(host_without_departure.retire_cooperative_start_on_timeout(&[1]));
        assert!(!host_without_departure.admits_network_start(&host_start));
        assert!(host.retire_network_start_on_departure(1));
        assert!(!host.admits_network_start(&host_start));
        assert!(!host.retire_network_start_on_departure(1));
    }

    fn voting_peers() -> (MatchSession, MatchSession) {
        let (mut host, mut guest) = ready_to_vote_peers();
        for peer in [&mut host, &mut guest] {
            assert_eq!(
                peer.continue_cooperative_round().unwrap(),
                CooperativeRoundStep::None
            );
            peer.take_network_send_requests();
        }
        (host, guest)
    }

    fn ready_to_vote_peers() -> (MatchSession, MatchSession) {
        let (mut host, _) = frozen(0);
        let (mut guest, _) = frozen(1);
        for peer in [&mut host, &mut guest] {
            peer.enter_cooperative_round(None, true, 2, 0, None)
                .unwrap();
        }
        (host, guest)
    }

    fn receive_assigned_options(
        host: &mut MatchSession,
        guest: &mut MatchSession,
        now: u32,
        options_first: bool,
    ) {
        let requests = host.take_network_send_requests();
        let [
            NetworkSendRequest::CooperativeBegin,
            NetworkSendRequest::CooperativeOptions(options),
        ] = requests.as_slice()
        else {
            panic!("expected a begin and assigned options");
        };
        let begin = (
            CooperativeRoundEvent::Begin { now },
            CooperativeRoundStep::None,
        );
        let options = (
            CooperativeRoundEvent::Options {
                options: options.clone(),
                now,
            },
            CooperativeRoundStep::OptionsApplied {
                difficulty: 0,
                changed: false,
            },
        );
        for (event, step) in if options_first {
            [options, begin]
        } else {
            [begin, options]
        } {
            assert_eq!(guest.cooperative_round_event(event, 0).unwrap(), step);
        }
        assert_eq!(
            guest.take_network_send_requests(),
            vec![NetworkSendRequest::CooperativeAck]
        );
    }

    #[test]
    fn guest_applies_the_masters_options_and_acks_after_the_begin() {
        let (mut session, _) = frozen(1);
        session
            .enter_cooperative_round(None, true, 2, 0, None)
            .unwrap();
        session.continue_cooperative_round().unwrap();
        assert_eq!(
            session.take_network_send_requests(),
            vec![NetworkSendRequest::CooperativeReady],
            "only the master sends options"
        );
        assert_eq!(
            session
                .cooperative_round_event(CooperativeRoundEvent::Begin { now: 100 }, 0)
                .unwrap(),
            CooperativeRoundStep::None
        );
        let mut options = round_options(session.network_setup().unwrap(), 2);
        options.humans[0].1.color_index = 5;
        assert_eq!(
            session
                .cooperative_round_event(CooperativeRoundEvent::Options { options, now: 101 }, 0)
                .unwrap(),
            CooperativeRoundStep::OptionsApplied {
                difficulty: 2,
                changed: true
            }
        );
        let round = session.cooperative_round().unwrap();
        assert!(!round.local_ready(), "a changed 0x32 unreadies everyone");
        assert_eq!(
            session.network_setup().unwrap().humans[0].slot.color_index,
            5
        );
        assert_eq!(
            session.take_network_send_requests(),
            vec![NetworkSendRequest::CooperativeAck]
        );
        assert_eq!(
            session
                .cooperative_round_event(CooperativeRoundEvent::Go, 2)
                .unwrap(),
            CooperativeRoundStep::Exit(true)
        );
        assert_eq!(session.cooperative_round().unwrap().outcome, Some(true));
    }

    #[test]
    fn a_departed_peer_leaves_only_quit() {
        let (mut session, _) = frozen(0);
        session
            .enter_cooperative_round(None, true, 2, 0, None)
            .unwrap();
        assert_eq!(
            session
                .cooperative_round_event(CooperativeRoundEvent::PeerLeft { source: 1 }, 0)
                .unwrap(),
            CooperativeRoundStep::PeerLeft { source: 1 }
        );
        assert!(!session.cooperative_round_continue_enabled());
        assert_eq!(session.network_connected_source_indices(), &[0]);
        session.quit_cooperative_round().unwrap();
        assert_eq!(
            session.take_network_send_requests(),
            vec![NetworkSendRequest::CooperativeLeave]
        );
        assert_eq!(session.cooperative_round().unwrap().outcome, Some(false));
        assert!(session.freeze_cooperative_round(30, 1).is_err());
    }

    #[test]
    fn a_master_leaving_after_the_begin_gives_the_guest_quit_back() {
        let (mut session, _) = frozen(1);
        session
            .enter_cooperative_round(None, true, 2, 0, None)
            .unwrap();
        session.continue_cooperative_round().unwrap();
        session
            .cooperative_round_event(CooperativeRoundEvent::Begin { now: 100 }, 0)
            .unwrap();
        assert!(
            session.cooperative_round().unwrap().quit_enabled(),
            "0x34 alone leaves Quit enabled"
        );
        let options = round_options(session.network_setup().unwrap(), 0);
        session
            .cooperative_round_event(CooperativeRoundEvent::Options { options, now: 101 }, 0)
            .unwrap();
        assert!(!session.cooperative_round().unwrap().quit_enabled());
        session
            .cooperative_round_event(CooperativeRoundEvent::PeerLeft { source: 0 }, 0)
            .unwrap();
        assert!(session.cooperative_round().unwrap().quit_enabled());
        assert!(!session.cooperative_round_continue_enabled());
    }

    /// A WOL relay carries no BC6 record: Continue leaves for the room at
    /// once, and the last mission keeps BC6's own result 0.
    #[test]
    fn an_online_round_continues_in_its_room() {
        let (mut session, _) = frozen_in(NetworkReturnContext::Wol, 0);
        session.set_network_room_return(NetworkRoomReturn::Wol {
            cooperative: None,
            campaign: None,
        });
        let mode_id = session.network_setup().unwrap().mode.id;
        session
            .enter_cooperative_round(Some("C2S02MD.MAP".into()), true, 2, 1, None)
            .unwrap();
        assert_eq!(
            session.continue_cooperative_round().unwrap(),
            CooperativeRoundStep::ContinueInRoom
        );
        assert_eq!(session.cooperative_round().unwrap().outcome, Some(false));
        assert!(session.take_network_send_requests().is_empty());
        assert!(!session.awaiting_wol_room_return());
        session.return_to_network_lobby();
        assert!(session.awaiting_wol_room_return());
        assert_eq!(
            session.take_network_room_return(),
            NetworkRoomReturn::Wol {
                cooperative: Some(CooperativeResume {
                    mode_id,
                    scenario: 1
                }),
                campaign: None,
            }
        );
        assert!(!session.awaiting_wol_room_return());

        let (mut last, _) = frozen_in(NetworkReturnContext::Wol, 0);
        last.enter_cooperative_round(None, false, 2, 0, None)
            .unwrap();
        assert_eq!(
            last.continue_cooperative_round().unwrap(),
            CooperativeRoundStep::Exit(false)
        );
    }

    #[test]
    fn a_cooperative_start_refuses_a_human_past_the_rounds_rows() {
        let freeze = |row| {
            let mut launch = super::super::tests::network_fixture();
            launch.mode.class = crate::skirmish_modes::MpModeClass::Cooperative;
            launch.local_player_index = 0;
            launch.humans[1].raw_slot_index = Some(row);
            let mut session = MatchSession::default();
            session.enter_network(NetworkReturnContext::LanLobby);
            session.accept_network_setup(launch, true, None, false)?;
            session.set_network_start_seed(SEED)?;
            session.prepare_network_timing(2, || 0)?;
            session.freeze_network_start(SEED, None).map(|_| session)
        };
        let mut session = freeze(7).unwrap();
        session
            .enter_cooperative_round(None, true, 2, 0, None)
            .unwrap();
        assert!(freeze(8).is_err());
    }

    #[test]
    fn the_last_mission_cannot_continue() {
        let (mut session, _) = frozen(1);
        session
            .enter_cooperative_round(None, false, 2, 0, None)
            .unwrap();
        assert_eq!(
            session.continue_cooperative_round().unwrap(),
            CooperativeRoundStep::Exit(false)
        );
    }

    /// A report left from the last load must not count this side as loaded
    /// in the next mission's load.
    #[test]
    fn the_round_entry_drops_a_report_left_from_the_last_load() {
        let (mut session, _) = frozen(1);
        session.report_network_loading_progress(100);
        session
            .enter_cooperative_round(None, true, 2, 0, None)
            .unwrap();
        assert!(session.take_network_loading_progress_reports().is_empty());
    }
}
