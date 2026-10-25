//! The LAN Cooperative result dialog's round state: BC6's readiness bytes
//! `0x89E000[8]`, the local ready flag `0x89E05C`, the ack count
//! `0x89E058`, the begin flag and the loop's result `0x89E04C`, held
//! with the round's roster as one network phase.
//!
//! Bodies: the loop `CampaignScoreClass vt+4` (`0x0046FC00`) and
//! `Dialog0xBC6__DlgProc` (`0x0046DFD0`). Peers arrive as decoded
//! [`CooperativeRoundEvent`]s and every packet this side sends is a
//! [`NetworkLoadingRequest`]; `lan_service::cooperative` moves both.

use super::{MatchSession, NetworkLoadingRequest, NetworkPhase, NetworkSession};
use crate::skirmish_launch::{
    LaunchStartPosition, SkirmishAiSlot, SkirmishLaunchSession, SkirmishLocalSlot,
};
use std::sync::Arc;

/// BC6's round state, owned by the `CooperativeRound` phase. The row
/// status statics (`TXT_READY`/`TXT_WAITING`/`TXT_ABORTED` in
/// `0x70A`/`0x70B`) are template-hidden and never shown, so their texts are
/// not kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CooperativeRoundState {
    /// `0x89E000[8]`, by raw slot: empty slots start ready
    /// (`0x0046EF06..0x0046EF4A`), and every un-ready pass clears human
    /// slots only.
    ready: [bool; 8],
    /// `0x89E05C`.
    local_ready: bool,
    /// `0x89E058`, the master's ack count.
    acks: usize,
    /// The loop's local begin flag (`[ESP+0x13]`), set by `0x34`.
    begin: bool,
    /// Quit `0x5C0`'s enable state: the `0x32` arm disables it once the
    /// begin arrived (`0x0047086E`), and the `0x24` arm enables it again
    /// (`0x00470583`).
    quit_enabled: bool,
    /// `0x89E05D`: false on the campaign's last mission.
    can_continue: bool,
    /// `A8D580`, the human House count of the mission's rows.
    rows: usize,
    /// `0x89E04C` once the loop left: true runs the next mission.
    outcome: Option<bool>,
}

impl CooperativeRoundState {
    pub(crate) fn local_ready(&self) -> bool {
        self.local_ready
    }

    pub(crate) fn quit_enabled(&self) -> bool {
        self.outcome.is_none() && self.quit_enabled
    }

    pub(crate) fn outcome(&self) -> Option<bool> {
        self.outcome
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
    Begin,
    /// `0x32`.
    Options(CooperativeRoundOptions),
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
}

impl MatchSession {
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
    ) -> anyhow::Result<()> {
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
        let NetworkPhase::Frozen { startup, connected } = &network.phase else {
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
        launch.rebuild_house_roster();
        // 0x0046EF06: empty slots ready, every bound human slot not.
        let mut ready = [true; 8];
        for (index, human) in launch.humans.iter().enumerate() {
            if let Some(row) = human.raw_slot_index
                && connected.contains(&index)
            {
                ready[usize::from(row)] = false;
            }
        }
        let round = CooperativeRoundState {
            ready,
            local_ready: false,
            acks: 0,
            begin: false,
            quit_enabled: true,
            can_continue,
            rows,
            outcome: None,
        };
        network.phase = NetworkPhase::CooperativeRound {
            previous: Arc::clone(startup),
            connected: connected.clone(),
            launch,
            round,
        };
        network.loading_requests.clear();
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
        launch.rebuild_house_roster();
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
                .is_some_and(|row| round.ready[usize::from(row)]),
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
        match self.network.as_ref().map(|network| &network.phase) {
            Some(NetworkPhase::CooperativeRound {
                connected, round, ..
            }) => round.outcome.is_none() && connected.len() >= round.rows && !round.local_ready,
            _ => false,
        }
    }

    /// Continue `0x6D1` (`0x0046E743`) and the loop's slot-1 pass
    /// (`0x0046FD61`). The button disables itself once ready, so only the
    /// ready arm is reachable; `difficulty` is `[0xA8EB68]` for the
    /// master's `0x32`.
    pub(crate) fn continue_cooperative_round(
        &mut self,
        difficulty: i32,
    ) -> anyhow::Result<CooperativeRoundStep> {
        let host = self.host;
        let RoundParts {
            requests,
            connected: _,
            launch,
            round,
        } = round_parts(self)?;
        anyhow::ensure!(round.outcome.is_none(), "the result dialog has closed");
        let local = launch.local_player_index;
        round.local_ready = true;
        if let Some(row) = launch
            .humans
            .get(local)
            .and_then(|human| human.raw_slot_index)
        {
            round.ready[usize::from(row)] = true;
        }
        if host && round.can_continue {
            let options = round_options(launch, difficulty);
            requests.push(NetworkLoadingRequest::CooperativeOptions(options));
        }
        requests.push(NetworkLoadingRequest::CooperativeReady);
        if host && round.all_ready() {
            if !round.can_continue {
                round.outcome = Some(false);
                return Ok(CooperativeRoundStep::Exit(false));
            }
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
    pub(crate) fn cooperative_round_assigned(&mut self, difficulty: i32) -> anyhow::Result<()> {
        anyhow::ensure!(self.host, "only the master assigns the round");
        let RoundParts {
            requests,
            connected: _,
            launch,
            round,
        } = round_parts(self)?;
        requests.push(NetworkLoadingRequest::CooperativeBegin);
        round.acks += 1;
        let options = round_options(launch, difficulty);
        requests.push(NetworkLoadingRequest::CooperativeOptions(options));
        Ok(())
    }

    /// Quit `0x5C0` (`0x0046E28C`): `0x24` to every node and result 0.
    pub(crate) fn quit_cooperative_round(&mut self) -> anyhow::Result<()> {
        let RoundParts {
            requests, round, ..
        } = round_parts(self)?;
        if round.outcome.is_none() {
            round.outcome = Some(false);
            requests.push(NetworkLoadingRequest::CooperativeLeave);
        }
        Ok(())
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
            connected,
            launch,
            round,
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
                if let Some(row) = slot_of(launch, source) {
                    round.ready[row] = true;
                }
                if host && round.all_ready() && round.can_continue {
                    return Ok(CooperativeRoundStep::Assign);
                }
                Ok(CooperativeRoundStep::None)
            }
            CooperativeRoundEvent::PeerWaiting { source } => {
                if let Some(row) = slot_of(launch, source) {
                    round.ready[row] = false;
                }
                Ok(CooperativeRoundStep::None)
            }
            CooperativeRoundEvent::PeerLeft { source } => {
                if let Some(row) = slot_of(launch, source) {
                    round.ready[row] = false;
                }
                // The node is deleted (A8DA84--); its slot stays unready,
                // so all-ready can no longer pass. Continue is disabled
                // (by the connected count) and Quit enabled again.
                connected.retain(|index| *index != source);
                round.quit_enabled = true;
                Ok(CooperativeRoundStep::PeerLeft { source })
            }
            CooperativeRoundEvent::Begin => {
                round.begin = true;
                Ok(tail(requests, host, round))
            }
            CooperativeRoundEvent::Options(options) => {
                let changed = apply_round_options(launch, &options, difficulty)?;
                if changed {
                    round.local_ready = false;
                    clear_human_readiness(launch, round);
                }
                if round.begin {
                    round.quit_enabled = false;
                    requests.push(NetworkLoadingRequest::CooperativeAck);
                }
                Ok(CooperativeRoundStep::OptionsApplied {
                    difficulty: options.difficulty,
                    changed,
                })
            }
            CooperativeRoundEvent::Ack { source: _ } => {
                round.acks += 1;
                if round.acks == round.rows {
                    round.outcome = Some(true);
                    requests.push(NetworkLoadingRequest::CooperativeGo);
                    return Ok(CooperativeRoundStep::Exit(true));
                }
                Ok(tail(requests, host, round))
            }
            CooperativeRoundEvent::Go => {
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
            CooperativeRoundEvent::KeepAlive => Ok(tail(requests, host, round)),
        }
    }

    /// Cooperative `+0xAC` and the next Start_Scenario: a won (or retried)
    /// round freezes its roster as the next immutable start, with the same
    /// agreed seed (`g_RngSeed = mode+0x9C`, `0x005C4890`), the
    /// continuation's MaxAhead and LatencyFudge, no WOL terms
    /// (`0x005C46F5`) and the live membership.
    /// `saved` is a won mission's fresh carry-over record (`0x004C6180`); a
    /// defeat restores the stale one: the last continuation's, or the
    /// cleared record of `0x004C6140`.
    pub(crate) fn freeze_cooperative_round(
        &mut self,
        max_ahead_frames: i32,
        latency_fudge: i32,
        saved: Option<crate::sim::campaign_carry_over::CampaignCarryOver>,
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
        let carry_over = saved
            .or_else(|| previous.carry_over().copied())
            .unwrap_or(crate::sim::campaign_carry_over::CampaignCarryOver::CLEARED);
        let NetworkPhase::CooperativeRound {
            launch, connected, ..
        } = std::mem::replace(&mut network.phase, NetworkPhase::Lobby)
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
            connected,
        };
        self.latency_fudge = latency_fudge;
        Ok(crate::app::match_bootstrap::LoadingStartup::Network(
            startup,
        ))
    }
}

/// The round's fields, split from the visit so a step can queue requests
/// while it edits the round.
struct RoundParts<'a> {
    requests: &'a mut Vec<NetworkLoadingRequest>,
    connected: &'a mut Vec<usize>,
    launch: &'a mut SkirmishLaunchSession,
    round: &'a mut CooperativeRoundState,
}

fn round_parts(session: &mut MatchSession) -> anyhow::Result<RoundParts<'_>> {
    let NetworkSession {
        loading_requests,
        phase,
        ..
    } = session
        .network
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
    let NetworkPhase::CooperativeRound {
        connected,
        launch,
        round,
        ..
    } = phase
    else {
        anyhow::bail!("no co-op result dialog");
    };
    Ok(RoundParts {
        requests: loading_requests,
        connected,
        launch,
        round,
    })
}

/// The loop's tail `0x004702FD`: a master whose every slot is ready leaves
/// on the next packet, sending `0x31`.
fn tail(
    requests: &mut Vec<NetworkLoadingRequest>,
    host: bool,
    round: &mut CooperativeRoundState,
) -> CooperativeRoundStep {
    if host && round.all_ready() && round.can_continue {
        round.outcome = Some(true);
        requests.push(NetworkLoadingRequest::CooperativeGo);
        return CooperativeRoundStep::Exit(true);
    }
    CooperativeRoundStep::None
}

fn clear_human_readiness(launch: &SkirmishLaunchSession, round: &mut CooperativeRoundState) {
    for human in &launch.humans {
        if let Some(row) = human.raw_slot_index {
            round.ready[usize::from(row)] = false;
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
    candidate.rebuild_house_roster();
    *launch = candidate;
    Ok(changed)
}

#[cfg(test)]
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
        let mut launch = super::super::tests::network_fixture();
        launch.local_player_index = local;
        let mut session = MatchSession::default();
        session.enter_network(NetworkReturnContext::LanLobby);
        session
            .accept_network_setup(launch, local == 0, Some(7))
            .unwrap();
        session.set_network_start_seed(SEED).unwrap();
        session.prepare_network_timing(2, || 0).unwrap();
        let LoadingStartup::Network(startup) = session.freeze_network_start(SEED).unwrap() else {
            panic!("network start");
        };
        (session, startup)
    }

    #[test]
    fn master_round_begins_on_the_guests_ready_and_leaves_on_its_ack() {
        let (mut session, first) = frozen(0);
        session
            .enter_cooperative_round(Some("C2S02MD.MAP".into()), true, 2)
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
            session.continue_cooperative_round(1).unwrap(),
            CooperativeRoundStep::None
        );
        assert!(!session.cooperative_round_continue_enabled());
        let requests = session.take_network_loading_requests();
        assert!(matches!(
            requests.as_slice(),
            [
                NetworkLoadingRequest::CooperativeOptions(CooperativeRoundOptions {
                    difficulty: 1,
                    ..
                }),
                NetworkLoadingRequest::CooperativeReady,
            ]
        ));

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
            session.take_network_loading_requests().is_empty(),
            "the begin waits for the assignment"
        );
        session.cooperative_round_assigned(1).unwrap();
        let requests = session.take_network_loading_requests();
        assert!(matches!(
            requests.as_slice(),
            [
                NetworkLoadingRequest::CooperativeBegin,
                NetworkLoadingRequest::CooperativeOptions(_),
            ]
        ));
        assert_eq!(
            session
                .cooperative_round_event(CooperativeRoundEvent::Ack { source: 1 }, 1)
                .unwrap(),
            CooperativeRoundStep::Exit(true)
        );
        let LoadingStartup::Network(next) = session.freeze_cooperative_round(30, 2, None).unwrap()
        else {
            panic!("network start");
        };
        assert_eq!(
            session.take_network_loading_requests(),
            vec![NetworkLoadingRequest::CooperativeGo],
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
    fn master_leaves_on_any_packet_once_every_slot_is_ready() {
        let (mut session, _) = frozen(0);
        session.enter_cooperative_round(None, true, 3).unwrap();
        session.continue_cooperative_round(0).unwrap();
        assert_eq!(
            session
                .cooperative_round_event(CooperativeRoundEvent::PeerReady { source: 1 }, 0)
                .unwrap(),
            CooperativeRoundStep::Assign
        );
        session.cooperative_round_assigned(0).unwrap();
        // Two acks are not three rows; the tail still leaves.
        assert_eq!(
            session
                .cooperative_round_event(CooperativeRoundEvent::Ack { source: 1 }, 0)
                .unwrap(),
            CooperativeRoundStep::Exit(true)
        );
    }

    #[test]
    fn guest_applies_the_masters_options_and_acks_after_the_begin() {
        let (mut session, _) = frozen(1);
        session.enter_cooperative_round(None, true, 2).unwrap();
        session.continue_cooperative_round(0).unwrap();
        assert_eq!(
            session.take_network_loading_requests(),
            vec![NetworkLoadingRequest::CooperativeReady],
            "only the master sends options"
        );
        assert_eq!(
            session
                .cooperative_round_event(CooperativeRoundEvent::Begin, 0)
                .unwrap(),
            CooperativeRoundStep::None
        );
        let mut options = round_options(session.network_setup().unwrap(), 2);
        options.humans[0].1.color_index = 5;
        assert_eq!(
            session
                .cooperative_round_event(CooperativeRoundEvent::Options(options), 0)
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
            session.take_network_loading_requests(),
            vec![NetworkLoadingRequest::CooperativeAck]
        );
        assert_eq!(
            session
                .cooperative_round_event(CooperativeRoundEvent::Go, 2)
                .unwrap(),
            CooperativeRoundStep::Exit(true)
        );
        assert_eq!(session.cooperative_round().unwrap().outcome(), Some(true));
    }

    #[test]
    fn a_departed_peer_leaves_only_quit() {
        let (mut session, _) = frozen(0);
        session.enter_cooperative_round(None, true, 2).unwrap();
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
            session.take_network_loading_requests(),
            vec![NetworkLoadingRequest::CooperativeLeave]
        );
        assert_eq!(session.cooperative_round().unwrap().outcome(), Some(false));
        assert!(session.freeze_cooperative_round(30, 1, None).is_err());
    }

    #[test]
    fn a_master_leaving_after_the_begin_gives_the_guest_quit_back() {
        let (mut session, _) = frozen(1);
        session.enter_cooperative_round(None, true, 2).unwrap();
        session.continue_cooperative_round(0).unwrap();
        session
            .cooperative_round_event(CooperativeRoundEvent::Begin, 0)
            .unwrap();
        assert!(
            session.cooperative_round().unwrap().quit_enabled(),
            "0x34 alone leaves Quit enabled"
        );
        let options = round_options(session.network_setup().unwrap(), 0);
        session
            .cooperative_round_event(CooperativeRoundEvent::Options(options), 0)
            .unwrap();
        assert!(!session.cooperative_round().unwrap().quit_enabled());
        session
            .cooperative_round_event(CooperativeRoundEvent::PeerLeft { source: 0 }, 0)
            .unwrap();
        assert!(session.cooperative_round().unwrap().quit_enabled());
        assert!(!session.cooperative_round_continue_enabled());
    }

    #[test]
    fn the_last_mission_cannot_continue() {
        let (mut session, _) = frozen(1);
        session.enter_cooperative_round(None, false, 2).unwrap();
        assert_eq!(
            session.continue_cooperative_round(0).unwrap(),
            CooperativeRoundStep::Exit(false)
        );
    }
}
