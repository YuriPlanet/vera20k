//! The frozen campaign's ballot and next admission share NetworkPhase.
//! Presentation can choose a route and transport can deliver a record. Neither
//! can approve a transition, add a voter, or select new stage content.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::app::match_bootstrap::{LoadingStartup, PreparedCampaignStart};
use crate::app::shell_network_setup::campaign_room::CampaignRoom;
use crate::map::campaign_manifest::CampaignManifest;
use crate::net::global::GlobalMessage;
use crate::net::lan_messages::extended_setup::campaign::{Signal, StageChoice};
use crate::sim::campaign_carry_over::StageCarryOver;
use crate::sim::house_state::HouseOutcomeKind;
use crate::sim::world::Simulation;

use super::{MatchSession, NetworkPhase, NetworkReturnContext, NetworkRoomReturn};

const VOTE_TIMEOUT_MS: u64 = 120_000;
const HANDOFF_TIMEOUT_MS: u64 = 15_000;
const VOTE_RETRY_MS: u64 = 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StagePlan {
    pub(crate) choice: StageChoice,
    pub(crate) stage: String,
    pub(crate) title: String,
    pub(crate) carry: StageCarryOver,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CampaignResult {
    outcome: HouseOutcomeKind,
    plans: Vec<StagePlan>,
    ending_hash: u64,
    deadline_ms: u64,
    playing_sources: Vec<usize>,
    votes: BTreeMap<usize, StageChoice>,
    choice: Option<StageChoice>,
    expected_next: Option<[u8; 32]>,
    prepared: Option<(CampaignRoom, PreparedCampaignStart)>,
    accepted: BTreeSet<usize>,
    begun: BTreeSet<usize>,
    begun_receipts: Vec<GlobalMessage>,
    outgoing: Option<Signal>,
    last_vote_ms: Option<u64>,
    begin_sent: bool,
    waiting_for_receipt_delivery: bool,
    begin: bool,
    aborted: bool,
}

pub(crate) enum StageHandoff {
    Lan(LoadingStartup),
    Wol,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum StageProgress {
    Waiting,
    Commit,
    Abort,
}

impl CampaignResult {
    /// The verified manifest supplies all route targets. The completed world
    /// supplies carry-over for branches, while Retry reuses the entry record.
    pub(crate) fn capture(
        manifest: &CampaignManifest,
        room: &CampaignRoom,
        sim: &Simulation,
        outcome: HouseOutcomeKind,
        now_ms: u64,
        playing_sources: &[usize],
    ) -> anyhow::Result<Self> {
        use anyhow::Context;
        let stage = manifest
            .stage(room.stage())
            .context("current stage is absent")?;
        let locale = crate::i18n::selected_language();
        let mut plans = vec![StagePlan {
            choice: StageChoice::Retry,
            stage: stage.id.clone(),
            title: stage.title(&locale).to_owned(),
            carry: room.carry_over().clone(),
        }];
        let carry = sim.save_stage_carry_over(&stage.carry_over)?;
        // The parser resolves every edge before a stage is admitted. Retain
        // canonical branch order even when titles and interface languages differ.
        for (index, (label, &target)) in stage.routes.iter().enumerate() {
            if label == "retry" {
                continue;
            }
            let next = manifest
                .stages
                .get(target)
                .context("route target is absent")?;
            plans.push(StagePlan {
                choice: StageChoice::Route(u16::try_from(index)?),
                stage: next.id.clone(),
                title: next.title(&locale).to_owned(),
                carry: carry.clone(),
            });
        }
        Ok(Self::new(
            outcome,
            plans,
            sim.state_hash(),
            now_ms,
            playing_sources.to_vec(),
        ))
    }

    fn new(
        outcome: HouseOutcomeKind,
        plans: Vec<StagePlan>,
        ending_hash: u64,
        now_ms: u64,
        playing_sources: Vec<usize>,
    ) -> Self {
        Self {
            outcome,
            plans,
            ending_hash,
            deadline_ms: now_ms.saturating_add(VOTE_TIMEOUT_MS),
            playing_sources,
            votes: BTreeMap::new(),
            choice: None,
            expected_next: None,
            prepared: None,
            accepted: BTreeSet::new(),
            begun: BTreeSet::new(),
            begun_receipts: Vec::new(),
            outgoing: None,
            last_vote_ms: None,
            begin_sent: false,
            waiting_for_receipt_delivery: false,
            begin: false,
            aborted: false,
        }
    }

    pub(crate) fn outcome(&self) -> HouseOutcomeKind {
        self.outcome
    }

    pub(crate) fn plans(&self) -> &[StagePlan] {
        &self.plans
    }

    fn ready_to_commit(&self) -> bool {
        self.begin && !self.aborted
    }

    pub(crate) fn progress(&self, playing: &[usize], now_ms: u64) -> StageProgress {
        if self.aborted || playing != self.playing_sources {
            StageProgress::Abort
        } else if self.ready_to_commit() {
            // The receive pump admits the Begin before polling local time.
            StageProgress::Commit
        } else if now_ms >= self.deadline_ms {
            StageProgress::Abort
        } else {
            StageProgress::Waiting
        }
    }

    fn accepts(&self, source: usize, choice: StageChoice) -> bool {
        self.playing_sources.contains(&source)
            && self.plans.iter().any(|plan| plan.choice == choice)
            && !self.aborted
            && !self.begin
    }

    fn unanimity(&self) -> Option<StageChoice> {
        let choice = self.votes.values().next().copied()?;
        self.playing_sources
            .iter()
            .all(|source| self.votes.get(source) == Some(&choice))
            .then_some(choice)
    }

    pub(crate) fn plan_to_prepare(&self) -> Option<&StagePlan> {
        if self.prepared.is_some() {
            return None;
        }
        let choice = self.choice?;
        self.plans.iter().find(|plan| plan.choice == choice)
    }

    pub(crate) fn outgoing(&self, host: bool, local: usize, now_ms: u64) -> Option<Signal> {
        if !host
            && self.choice.is_none()
            && self
                .last_vote_ms
                .is_none_or(|sent| now_ms.saturating_sub(sent) >= VOTE_RETRY_MS)
        {
            // A reliable packet may have arrived while the host was still
            // finishing its result audio. Repeat the same ballot until the
            // host confirms the choice. Its source cannot change its vote.
            return self.votes.get(&local).copied().map(Signal::StageVote);
        }
        self.outgoing.or_else(|| {
            (host
                && self.prepared.is_some()
                && !self.begin
                && !self.begin_sent
                && !self.aborted
                && self
                    .playing_sources
                    .iter()
                    .all(|source| self.accepted.contains(source)))
            .then(|| self.choice.map(Signal::StageBegin))
            .flatten()
        })
    }

    fn record_vote(&mut self, source: usize, choice: StageChoice, host: bool) {
        if self.accepts(source, choice) && self.choice.is_none() {
            self.votes.entry(source).or_insert(choice);
            if host {
                self.choice = self.unanimity();
            }
        }
    }

    fn receive(
        &mut self,
        source: usize,
        from_host: bool,
        host: bool,
        local: usize,
        signal: Signal,
    ) -> bool {
        if !self.playing_sources.contains(&source) || self.begin {
            return false;
        }
        match signal {
            Signal::StageVote(choice) if host => self.record_vote(source, choice, true),
            Signal::StagePrepare {
                choice,
                ending,
                next,
            } if !host && from_host && self.votes.get(&local) == Some(&choice) => {
                if ending == self.ending_hash {
                    self.choice = Some(choice);
                    self.expected_next = Some(next);
                } else {
                    self.aborted = true;
                }
            }
            Signal::StageAccepted(choice)
                if host && self.prepared.is_some() && self.choice == Some(choice) =>
            {
                self.accepted.insert(source);
            }
            Signal::StageBegin(choice)
                if !host
                    && from_host
                    && self.prepared.is_some()
                    && self.outgoing.is_none()
                    && !self.waiting_for_receipt_delivery
                    && self.choice == Some(choice) =>
            {
                self.outgoing = Some(Signal::StageBegun(choice));
            }
            Signal::StageBegun(choice)
                if host && self.begin_sent && self.choice == Some(choice) =>
            {
                let registered = self.begun.insert(source);
                self.begin = self
                    .playing_sources
                    .iter()
                    .all(|source| self.begun.contains(source));
                return registered;
            }
            Signal::StageAbort if host || from_host => self.aborted = true,
            _ => {}
        }
        false
    }

    fn confirm_send(&mut self, signal: Signal, local: usize, now_ms: u64) {
        if self.outgoing == Some(signal) {
            self.outgoing = None;
        }
        match signal {
            Signal::StageVote(_) => self.last_vote_ms = Some(now_ms),
            Signal::StageBegin(_) => {
                self.begin_sent = true;
                self.begun.insert(local);
                self.begin = self
                    .playing_sources
                    .iter()
                    .all(|source| self.begun.contains(source));
            }
            Signal::StageBegun(_) => self.waiting_for_receipt_delivery = true,
            _ => {}
        }
    }

    fn verify_start(
        &mut self,
        room: CampaignRoom,
        startup: PreparedCampaignStart,
        local: usize,
        host: bool,
        now_ms: u64,
    ) -> anyhow::Result<()> {
        let plan = self
            .plan_to_prepare()
            .ok_or_else(|| anyhow::anyhow!("no admitted vote"))?;
        anyhow::ensure!(plan.stage == room.stage() && &plan.carry == room.carry_over());
        let choice = plan.choice;
        // This recipe includes both digests of the destination map, the
        // manifest, role grants through that manifest and the exact carry.
        let mut digest = crate::util::sha256::Sha256::new();
        digest.update(&serde_json::to_vec(&room)?);
        let next = digest.finalize();
        anyhow::ensure!(
            host || self.expected_next == Some(next),
            "the next stage differs from the host's admitted recipe"
        );
        self.prepared = Some((room, startup));
        self.accepted.insert(local);
        self.outgoing = Some(if host {
            Signal::StagePrepare {
                choice,
                ending: self.ending_hash,
                next,
            }
        } else {
            Signal::StageAccepted(choice)
        });
        // Guests remain receptive past the host's handoff deadline. A late
        // Begin is either consumed at this phase or refused by startup.
        let timeout = HANDOFF_TIMEOUT_MS * if host { 1 } else { 2 };
        self.deadline_ms = now_ms.saturating_add(timeout);
        Ok(())
    }
}

impl MatchSession {
    pub(crate) fn open_campaign_result(&mut self, result: CampaignResult) -> anyhow::Result<()> {
        anyhow::ensure!(!self.local_connection_watches(), "watchers do not vote");
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no campaign visit"))?;
        let NetworkPhase::Frozen {
            startup,
            connections,
            room_return,
        } = &network.phase
        else {
            anyhow::bail!("a campaign result follows its frozen start");
        };
        anyhow::ensure!(
            startup.authored_campaign().is_some(),
            "not a campaign start"
        );
        anyhow::ensure!(
            result.playing_sources == connections.playing_sources,
            "the result roster changed"
        );
        network.phase = NetworkPhase::CampaignResult {
            startup: Arc::clone(startup),
            connections: connections.clone(),
            result: Box::new(result),
            room_return: room_return.clone(),
        };
        Ok(())
    }

    pub(crate) fn campaign_result(&self) -> Option<&CampaignResult> {
        match &self.network.as_ref()?.phase {
            NetworkPhase::CampaignResult { result, .. } => Some(result),
            _ => None,
        }
    }

    fn campaign_result_mut(&mut self) -> Option<&mut CampaignResult> {
        match &mut self.network.as_mut()?.phase {
            NetworkPhase::CampaignResult { result, .. } => Some(result),
            _ => None,
        }
    }

    pub(crate) fn vote_for_campaign_stage(&mut self, choice: StageChoice) {
        let host = self.host();
        let Some(local) = super::super::network_match::local_source(self) else {
            return;
        };
        let Some(result) = self.campaign_result_mut() else {
            return;
        };
        if result.accepts(local, choice) && !result.votes.contains_key(&local) {
            result.record_vote(local, choice, host);
        }
    }

    pub(crate) fn receive_campaign_stage_signal(
        &mut self,
        from: usize,
        from_host: bool,
        signal: Signal,
        receipt: Option<GlobalMessage>,
    ) {
        let host = self.host();
        let Some(local) = super::super::network_match::local_source(self) else {
            return;
        };
        if let Some(result) = self.campaign_result_mut()
            && result.receive(from, from_host, host, local, signal)
            && let Some(receipt) = receipt
        {
            result.begun_receipts.push(receipt);
        }
    }

    pub(crate) fn take_campaign_stage_receipts(&mut self) -> Vec<GlobalMessage> {
        self.campaign_result_mut()
            .filter(|result| result.ready_to_commit())
            .map(|result| std::mem::take(&mut result.begun_receipts))
            .unwrap_or_default()
    }

    pub(crate) fn confirm_campaign_stage_send(&mut self, signal: Signal, now_ms: u64) {
        let Some(local) = super::super::network_match::local_source(self) else {
            return;
        };
        if let Some(result) = self.campaign_result_mut() {
            result.confirm_send(signal, local, now_ms);
        }
    }

    pub(crate) fn confirm_campaign_stage_delivery(&mut self) {
        if let Some(result) = self.campaign_result_mut()
            && result.waiting_for_receipt_delivery
        {
            result.begin = true;
        }
    }

    pub(crate) fn stage_prepared(
        &mut self,
        room: CampaignRoom,
        startup: PreparedCampaignStart,
        now_ms: u64,
    ) -> anyhow::Result<()> {
        let host = self.host();
        let local = super::super::network_match::local_source(self)
            .ok_or_else(|| anyhow::anyhow!("no local playing source"))?;
        let result = self
            .campaign_result_mut()
            .ok_or_else(|| anyhow::anyhow!("no campaign result"))?;
        result.verify_start(room, startup, local, host, now_ms)
    }

    pub(crate) fn commit_campaign_stage(&mut self) -> anyhow::Result<StageHandoff> {
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no campaign visit"))?;
        let NetworkPhase::CampaignResult { result, .. } = &network.phase else {
            anyhow::bail!("no campaign result");
        };
        anyhow::ensure!(
            result.ready_to_commit(),
            "campaign stage did not pass handoff"
        );
        let NetworkPhase::CampaignResult {
            startup,
            connections,
            mut result,
            room_return,
        } = std::mem::replace(&mut network.phase, NetworkPhase::lobby())
        else {
            unreachable!("checked phase");
        };
        let (room, prepared) = result.prepared.take().expect("committed campaign plan");
        match network.return_context {
            NetworkReturnContext::LanLobby => {
                let startup = Arc::new(
                    crate::app::match_bootstrap::PreparedNetworkStartup::new(
                        startup.launch_session().clone(),
                        startup.seed.value,
                        startup.mode(),
                        self.host,
                        startup.max_ahead_frames(),
                        super::WolStartTerms::default(),
                    )
                    .with_campaign(prepared),
                );
                network.campaign_room = Some(room);
                network.loading_reports.clear();
                network.phase = NetworkPhase::Frozen {
                    startup: Arc::clone(&startup),
                    connections,
                    room_return,
                };
                Ok(StageHandoff::Lan(LoadingStartup::Network(startup)))
            }
            NetworkReturnContext::Wol => {
                network.phase = NetworkPhase::Lobby {
                    room_return: NetworkRoomReturn::Wol {
                        cooperative: None,
                        campaign: Some(Box::new(room)),
                    },
                };
                Ok(StageHandoff::Wol)
            }
        }
    }
}

#[cfg(all(test, tests_app_match))]
mod tests {
    use super::*;

    fn manifest() -> CampaignManifest {
        CampaignManifest::read(
            "campaigns/test/campaign.ini",
            concat!(
                "[Stage Opening]\nMap=OPEN.MAP\n",
                "[Routes Opening]\nDetour=Detour\nWin=Finale\n",
                "[CarryOver Opening]\nHouse.Alpha=Credits\nHouse.Bravo=Credits\n",
                "[Stage Detour]\nMap=DETOUR.MAP\n",
                "[Stage Finale]\nMap=FINAL.MAP\n",
            )
            .as_bytes(),
        )
        .unwrap()
    }

    fn room(manifest: &CampaignManifest, stage: &str, carry: StageCarryOver) -> CampaignRoom {
        serde_json::from_value(serde_json::json!({
            "series": "test",
            "stage": stage,
            "manifest_sha256": manifest.digest,
            "map_sha256": "b".repeat(64),
            "effective_map_digest": "c".repeat(64),
            "attempt": 2,
            "player_limit": 2,
            "difficulty": 1,
            "playing_sources": [
                { "player": "Guest", "role": "Guest" },
                { "player": "Host", "role": "Host" }
            ],
            "carry_over": carry,
        }))
        .unwrap()
    }

    fn result() -> CampaignResult {
        CampaignResult::new(
            HouseOutcomeKind::Defeat,
            vec![StagePlan {
                choice: StageChoice::Retry,
                stage: "opening".to_owned(),
                title: "Opening".to_owned(),
                carry: StageCarryOver::default(),
            }],
            49,
            0,
            vec![0, 1],
        )
    }

    #[test]
    fn a_watcher_cannot_vote_or_change_the_ballot_after_it_leaves() {
        let mut result = result();
        result.record_vote(0, StageChoice::Retry, true);
        result.receive(2, false, true, 0, Signal::StageVote(StageChoice::Retry));
        assert!(result.plan_to_prepare().is_none());
        result.receive(2, false, true, 0, Signal::StageAbort);
        assert_eq!(
            result.progress(&[0, 1], VOTE_TIMEOUT_MS - 1),
            StageProgress::Waiting
        );
        result.receive(1, false, true, 0, Signal::StageVote(StageChoice::Retry));
        assert_eq!(result.plan_to_prepare().unwrap().stage, "opening");
        assert_eq!(
            result.progress(&[0], VOTE_TIMEOUT_MS - 1),
            StageProgress::Abort
        );
    }

    #[test]
    fn a_different_terminal_hash_aborts_the_campaign_handoff() {
        let mut result = result();
        let choice = StageChoice::Retry;
        result.record_vote(1, choice, false);
        result.receive(
            0,
            true,
            false,
            1,
            Signal::StagePrepare {
                choice,
                ending: 50,
                next: [7; 32],
            },
        );
        assert_eq!(result.progress(&[0, 1], 1), StageProgress::Abort);
        assert!(!result.ready_to_commit());
    }

    #[test]
    fn branch_handoff_keeps_each_house_and_a_begin_at_the_guest_deadline() {
        let manifest = manifest();
        let previous = room(&manifest, "Opening", StageCarryOver::default());
        let mut sim = Simulation::new();
        for (name, credits) in [("Alpha", 310), ("Bravo", 470)] {
            let owner = sim.intern(name);
            let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 0);
            house.economy_mut_for_test().set_credits_for_test(credits);
            sim.register_house(house);
        }
        // Slot order deliberately differs from the room coordinator.
        let mut host = CampaignResult::capture(
            &manifest,
            &previous,
            &sim,
            HouseOutcomeKind::Victory,
            0,
            &[0, 1],
        )
        .unwrap();
        let mut guest = host.clone();
        assert!(
            host.plans()[0].carry.is_empty(),
            "retry restores entry state"
        );
        assert_eq!(host.plans()[1].stage, "Detour");
        assert_eq!(host.plans()[2].stage, "Finale");
        let winning = host.plans()[2].clone();
        let houses = serde_json::to_value(&winning.carry).unwrap();
        assert_eq!(houses["houses"][0]["credits"], 310);
        assert_eq!(houses["houses"][1]["credits"], 470);

        let choice = winning.choice;
        host.record_vote(1, choice, true);
        guest.record_vote(0, choice, false);
        host.receive(0, false, true, 1, guest.outgoing(false, 0, 0).unwrap());
        assert_eq!(host.plan_to_prepare(), Some(&winning));
        let room = room(&manifest, &winning.stage, winning.carry);
        let start = PreparedCampaignStart {
            authored: crate::sim::scenario_bootstrap::AuthoredCampaignStart::from_stage(
                manifest.stage("Finale").unwrap(),
                2,
                1,
                room.carry_over().clone(),
            )
            .unwrap(),
            map_source: crate::map::source::LoadedMapSource::LegacyFallback {
                label: "FINAL.MAP".into(),
            },
            map_name: "FINAL.MAP".into(),
            map_effective_digest: "c".repeat(64),
            local_source: Some(1),
            game_speed: 3,
        };
        host.verify_start(room.clone(), start.clone(), 1, true, 10)
            .unwrap();
        let prepare = host.outgoing(true, 1, 10).unwrap();
        host.confirm_send(prepare, 1, 10);
        guest.receive(1, true, false, 0, prepare);
        let mut wrong_room = serde_json::to_value(&room).unwrap();
        wrong_room["effective_map_digest"] = serde_json::json!("d".repeat(64));
        let wrong_room = serde_json::from_value(wrong_room).unwrap();
        assert!(
            guest
                .verify_start(wrong_room, start.clone(), 0, false, 11)
                .is_err()
        );
        guest.verify_start(room, start, 0, false, 11).unwrap();
        assert_eq!(host.outgoing(true, 1, 11), None);
        let accepted = guest.outgoing(false, 0, 11).unwrap();
        guest.confirm_send(accepted, 0, 11);
        host.receive(0, false, true, 1, accepted);
        let begin = host.outgoing(true, 1, 12).unwrap();
        assert_eq!(begin, Signal::StageBegin(choice));
        host.confirm_send(begin, 1, 12);
        assert!(
            !host.ready_to_commit(),
            "begin must reach the guest before old transport retires"
        );
        guest.receive(1, true, false, 0, begin);
        let begun = guest.outgoing(false, 0, guest.deadline_ms - 1).unwrap();
        assert_eq!(begun, Signal::StageBegun(choice));
        guest.confirm_send(begun, 0, guest.deadline_ms - 1);
        assert!(!guest.ready_to_commit(), "guest still owes a receipt");
        host.receive(0, false, true, 1, begun);
        assert_eq!(
            host.progress(&[0, 1], host.deadline_ms),
            StageProgress::Commit
        );
        guest.begin = guest.waiting_for_receipt_delivery;
        assert_eq!(
            guest.progress(&[0, 1], guest.deadline_ms),
            StageProgress::Commit
        );
    }
}
