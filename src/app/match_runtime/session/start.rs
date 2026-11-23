//! The network match start: resolved assignments, readiness and map status,
//! response-time and WOL timing, the start seed and frozen start, and the
//! requests (loading progress, chat, beacons, taunts) queued to send.

use super::NetworkMapAdmission;
use super::validate_network_rows;
use crate::skirmish_launch::SkirmishLaunchSession;
use std::sync::Arc;

use super::{
    ConnectionRoster, MatchSession, NetworkPhase, NetworkRoomReturn, NetworkSendRequest,
    NetworkSession, ParticipantAdmission, PauseEventLimiters, SessionMode, WolStartTerms,
    admit_network_start_roster,
};

impl MatchSession {
    /// Host-local assignment completion or a received resolved start. Source
    /// order/identity, options and map cannot change at this seam.
    pub(crate) fn accept_network_assignments(
        &mut self,
        mut resolved: SkirmishLaunchSession,
    ) -> anyhow::Result<()> {
        let current = self.editable_network_setup()?;
        anyhow::ensure!(
            current.humans.len() == resolved.humans.len()
                && current
                    .humans
                    .iter()
                    .zip(&resolved.humans)
                    .all(|(left, right)| left.name == right.name
                        && left.role == right.role
                        && left.raw_slot_index == right.raw_slot_index
                        && left.slot.start_position == right.slot.start_position
                        && left.slot.team == right.slot.team)
                && current.local_player_index == resolved.local_player_index
                && current.mode == resolved.mode
                && current.selected_map_file == resolved.selected_map_file
                && current.options == resolved.options
                && current.ai_player_count == resolved.ai_player_count
                && current.opponents.len() == resolved.opponents.len()
                && current
                    .opponents
                    .iter()
                    .zip(&resolved.opponents)
                    .all(|(left, right)| left.raw_slot_index == right.raw_slot_index
                        && left.difficulty == right.difficulty
                        && left.start_position == right.start_position
                        && left.team == right.team),
            "assignment completion cannot replace admitted setup identity"
        );
        crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(resolved.clone())
            .map_err(|error| anyhow::anyhow!("unresolved network assignments: {error}"))?;
        anyhow::ensure!(
            resolved.house_roster_is_consistent(),
            "inconsistent resolved House roster"
        );
        resolved.use_live_house_rows();
        *current = resolved;
        Ok(())
    }

    pub(crate) fn set_network_player_ready(
        &mut self,
        index: usize,
        ready: bool,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.local_connection_watches(),
            "watching has no readiness vote"
        );
        self.network_admission_mut(index)?.ready = ready;
        Ok(())
    }

    /// `005E9850` clears node+77 of every node not named `0xA8D610`. WOL
    /// calls it on host option edits (C2 `0078F9C1`/`0078FA97`) and whenever a
    /// user joins the game channel (`007A6625`). The host's Create copies its
    /// own name there (`0x78A0A9`); a WOL guest's join sink empties it
    /// (`0x7A620D`), so a guest clears every node, its own included. A LAN
    /// host calls it on a guest's preview request `0x1B` (`0x005E014B`);
    /// otherwise LAN peers retract acceptance through packet A0.
    pub(crate) fn clear_guest_readiness(&mut self) -> anyhow::Result<()> {
        let Some(NetworkSession {
            phase: NetworkPhase::Setup {
                launch, admission, ..
            },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        let keep = self.host.then_some(launch.local_player_index);
        for (index, entry) in admission.iter_mut().enumerate() {
            if Some(index) != keep {
                entry.ready = false;
            }
        }
        Ok(())
    }

    /// A new online options revision invalidates every remote agreement,
    /// even when a local optimistic edit made its echoed values compare equal.
    /// The host remains implicitly ready, as in clear_guest_readiness; each
    /// participant must publish fresh map/rules proof for this revision.
    pub(crate) fn invalidate_network_agreement(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.network.as_ref().is_some_and(|network| matches!(
                network.return_context.session_mode(),
                SessionMode::Lan | SessionMode::Wol
            )),
            "multiplayer agreement belongs to a network setup"
        );
        self.clear_guest_readiness()?;
        let Some(NetworkSession {
            phase: NetworkPhase::Setup {
                launch, admission, ..
            },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        for (index, entry) in admission.iter_mut().enumerate() {
            if index != launch.local_player_index {
                entry.map_status = 0;
            }
        }
        Ok(())
    }

    pub(crate) fn set_network_map_status(
        &mut self,
        index: usize,
        status: u8,
    ) -> anyhow::Result<()> {
        self.network_admission_mut(index)?.map_status = status;
        Ok(())
    }

    /// The host takes a participant's K (`0x5E39C0`) and learns whether it
    /// is news: the first K about this map, or a changed one. A native
    /// guest sends K once per map; VERA's online guests repeat theirs with
    /// every options revision, which says nothing new. Native admission
    /// (0x5E5500) has every guest report RandMap.Sed present, so an online
    /// guest's K0 for it, sent while the host's map is on its way, is not.
    pub(crate) fn report_network_map_status(
        &mut self,
        index: usize,
        status: u8,
    ) -> anyhow::Result<bool> {
        let file = self
            .network_setup()
            .and_then(|setup| setup.selected_map_file.as_deref());
        let always_present =
            self.network_map_metadata()
                .zip(file)
                .is_some_and(|(metadata, file)| {
                    metadata.admission(file, false) == NetworkMapAdmission::Present
                });
        let entry = self.network_admission_mut(index)?;
        entry.map_status = status;
        Ok(entry.reported_map_status.replace(status) != Some(status) && !always_present)
    }

    fn network_admission_mut(&mut self, index: usize) -> anyhow::Result<&mut ParticipantAdmission> {
        match self.network.as_mut().map(|network| &mut network.phase) {
            Some(NetworkPhase::Setup { admission, .. }) => admission
                .get_mut(index)
                .ok_or_else(|| anyhow::anyhow!("unknown participant")),
            _ => anyhow::bail!("network setup is not editable"),
        }
    }
}

impl MatchSession {
    pub(crate) fn network_player_ready(&self, index: usize) -> bool {
        match self.network.as_ref().map(|network| &network.phase) {
            Some(NetworkPhase::Setup { admission, .. }) => {
                admission.get(index).is_some_and(|entry| entry.ready)
            }
            _ => false,
        }
    }

    pub(crate) fn network_map_status(&self, index: usize) -> Option<u8> {
        match self.network.as_ref().map(|network| &network.phase) {
            Some(NetworkPhase::Setup { admission, .. }) => {
                admission.get(index).map(|entry| entry.map_status)
            }
            _ => None,
        }
    }

    /// Accept the final local response-time observations at host5DD48D or
    /// guest5DD2CD. The received host GO budget is not the guest's final timing.
    pub(crate) fn prepare_network_timing(
        &mut self,
        protocol: i32,
        response_time: impl FnMut() -> u32,
    ) -> anyhow::Result<()> {
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
        anyhow::ensure!(
            !matches!(network.phase, NetworkPhase::Frozen { .. }),
            "accepted start is immutable"
        );
        network.max_ahead_frames = Some(crate::net::lockstep::initial_multiplayer_max_ahead(
            protocol,
            response_time,
        ));
        self.latency_fudge = 0;
        Ok(())
    }

    pub(crate) fn latency_fudge(&self) -> i32 {
        self.latency_fudge
    }

    /// EVENT_LATENCYFUDGE's write (`0x004C7F4F`). The event itself, its
    /// broadcast and the remote-sender message `TXT_PLAYER_CHANGED_LATENCY`
    /// (`0x004C7F7D`) travel through the transport, outside this owner.
    pub(crate) fn set_latency_fudge(&mut self, value: i32) {
        self.latency_fudge = value;
    }

    pub(crate) fn pause_event_limiters_mut(&mut self) -> &mut PauseEventLimiters {
        &mut self.pause_event_limiters
    }
    /// WOL Start Game Now timing block (`0x794F6F..0x7951F0`): MaxAhead
    /// comes from the agreed game speed, identical for host and guest.
    /// Return the transport's outputs to its existing service owner. VERA's
    /// extended roster uses the same signed maximum across every peer ping.
    pub(crate) fn prepare_wol_timing(
        &mut self,
        pings: impl IntoIterator<Item = i32>,
        channel_owner: bool,
    ) -> anyhow::Result<crate::net::lockstep::WolStartTiming> {
        anyhow::ensure!(
            self.network_mode() == Some(SessionMode::Wol),
            "WOL timing needs a WOL visit"
        );
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
        let NetworkPhase::Setup { launch, .. } = &network.phase else {
            anyhow::bail!("WOL timing follows an admitted setup");
        };
        let timing =
            crate::net::lockstep::wol_start_timing(launch.options.game_speed, pings, channel_owner);
        network.max_ahead_frames = Some(timing.max_ahead);
        self.latency_fudge = timing.latency_fudge;
        Ok(timing)
    }

    /// OnGameStart's `[0xB779C4]` and Start Game Now's clan lists for the
    /// accepted WOL start.
    pub(crate) fn set_wol_start(&mut self, terms: WolStartTerms) {
        if let Some(network) = self.network.as_mut() {
            network.wol_start = terms;
        }
    }

    /// The WOL start terms: the setup's, then the frozen start's.
    pub(crate) fn network_wol_start(&self) -> Option<&WolStartTerms> {
        let network = self.network.as_ref()?;
        match &network.phase {
            NetworkPhase::Frozen { startup, .. } => Some(startup.wol_start()),
            _ => Some(&network.wol_start),
        }
    }

    pub(crate) fn network_max_ahead_frames(&self) -> Option<i32> {
        self.network.as_ref()?.max_ahead_frames
    }

    pub(crate) fn network_campaign_carry_over(
        &self,
    ) -> Option<crate::sim::campaign_carry_over::CampaignCarryOver> {
        let NetworkPhase::Frozen { startup, .. } = &self.network.as_ref()?.phase else {
            return None;
        };
        startup.carry_over().copied()
    }

    pub(crate) fn set_network_start_seed(&mut self, seed: u32) -> anyhow::Result<()> {
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
        anyhow::ensure!(
            !matches!(network.phase, NetworkPhase::Frozen { .. }),
            "accepted start is immutable"
        );
        network.proposed_seed = Some(seed);
        Ok(())
    }

    pub(crate) fn campaign_room(
        &self,
    ) -> Option<&crate::app::shell_network_setup::campaign_room::CampaignRoom> {
        self.network.as_ref()?.campaign_room.as_ref()
    }

    pub(crate) fn accept_campaign_room(
        &mut self,
        room: Option<crate::app::shell_network_setup::campaign_room::CampaignRoom>,
        game_speed: i32,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            u8::try_from(game_speed).is_ok_and(crate::sim::game_options::is_selectable_game_speed),
            "invalid campaign room speed"
        );
        if let Some(room) = &room {
            room.validate()?;
        }
        let Some(NetworkSession {
            campaign_room,
            phase: NetworkPhase::Setup { launch, .. },
            ..
        }) = &mut self.network
        else {
            anyhow::bail!("campaign room choices require an editable setup");
        };
        if room.is_some() {
            launch.options.game_speed = game_speed;
        }
        *campaign_room = room;
        Ok(())
    }

    pub(crate) fn network_start_seed(&self) -> Option<u32> {
        self.network.as_ref()?.proposed_seed
    }

    /// An accepted start freezes resolved host assignments and the agreed
    /// host seed. It never samples local time or resolves guest choices again.
    pub(crate) fn freeze_network_start(
        &mut self,
        agreed_seed: u32,
        campaign: Option<crate::app::match_bootstrap::PreparedCampaignStart>,
    ) -> anyhow::Result<crate::app::match_bootstrap::LoadingStartup> {
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no network visit is active"))?;
        let NetworkPhase::Setup { launch, .. } = &network.phase else {
            anyhow::bail!("network setup is not ready for start admission");
        };
        anyhow::ensure!(
            network.proposed_seed.is_none_or(|seed| seed == agreed_seed),
            "accepted host seed differs from admitted setup seed"
        );
        anyhow::ensure!(
            campaign.is_some() == network.campaign_room.is_some(),
            "campaign launch must be verified before freezing"
        );
        if campaign.is_none() {
            admit_network_start_roster(launch)?;
        } else {
            validate_network_rows(launch)?;
        }
        network.proposed_seed = Some(agreed_seed);
        let max_ahead_frames = network
            .max_ahead_frames
            .ok_or_else(|| anyhow::anyhow!("network start requires accepted timing"))?;
        let NetworkPhase::Setup {
            mut launch,
            watching_sources,
            ..
        } = std::mem::replace(&mut network.phase, NetworkPhase::lobby())
        else {
            unreachable!()
        };
        if let Some(prepared) = &campaign {
            // The connection adapter keeps shell names/rows. The actual world
            // is compiled from its authored recipe, so stock Co-op setup and
            // skirmish opponents cannot leak through the frozen transport.
            launch.mode = crate::skirmish_launch::SkirmishLaunchMode::stock(
                crate::skirmish_launch::BATTLE_MODE_ID,
            )
            .expect("stock transport mode");
            launch.selected_map_file = Some(prepared.map_name.clone());
            launch.opponents.clear();
            launch.ai_player_count = 0;
            launch.use_live_house_rows();
        }
        let startup = crate::app::match_bootstrap::PreparedNetworkStartup::new(
            launch,
            agreed_seed,
            network.return_context.session_mode(),
            self.host,
            max_ahead_frames,
            std::mem::take(&mut network.wol_start),
        )
        .with_local_map_source(
            network.local_map_source.clone(),
            network.local_map_effective_digest.clone(),
        )
        .with_random_map_preview(network.local_random_map_preview.clone());
        let startup = Arc::new(match campaign {
            Some(campaign) => startup.with_campaign(campaign),
            None => startup,
        });
        network.phase = NetworkPhase::Frozen {
            connections: ConnectionRoster {
                playing_sources: (0..startup.launch_session().humans.len()).collect(),
                watching_sources,
            },
            startup: Arc::clone(&startup),
            room_return: NetworkRoomReturn::Lobby,
            campaign_abort_pending: false,
        };
        Ok(crate::app::match_bootstrap::LoadingStartup::Network(
            startup,
        ))
    }

    /// Read-only identity gate for a synchronous loader resuming after normal
    /// network reducers ran. A retired/replaced admission cannot install its
    /// old finished result, even when its local progress already reached100.
    pub(crate) fn admits_network_start(
        &self,
        startup: &Arc<crate::app::match_bootstrap::PreparedNetworkStartup>,
    ) -> bool {
        matches!(&self.network,
            Some(NetworkSession { phase: NetworkPhase::Frozen { startup: admitted, .. }, .. })
                if Arc::ptr_eq(admitted, startup))
    }

    /// A scoped departure from a required player retires the frozen attempt
    /// before presentation can publish a finished local loader result. A
    /// viewer reads departures from the historical journal after restoration.
    pub(crate) fn retire_network_start_on_departure(&mut self, source: usize) -> bool {
        if self.local_connection_watches() {
            return false;
        }
        let Some(NetworkPhase::Frozen {
            startup,
            connections,
            ..
        }) = self.network.as_ref().map(|network| &network.phase)
        else {
            return false;
        };
        if source == startup.launch_session().local_player_index
            || !connections.playing_sources.contains(&source)
        {
            return false;
        }
        self.return_to_network_lobby();
        true
    }

    /// The loader borrows the same frozen owner as the shell. It cannot adopt
    /// a second mutable setup or rewrite LAN/WOL/host identity during loading.
    pub(crate) fn assert_network_start(
        &self,
        startup: &Arc<crate::app::match_bootstrap::PreparedNetworkStartup>,
    ) {
        assert!(
            self.admits_network_start(startup),
            "loading must consume the admitted immutable start"
        );
        assert_eq!(self.network_mode(), Some(startup.mode()));
        assert_eq!(self.host, startup.host());
    }

    pub(crate) fn take_network_send_requests(&mut self) -> Vec<NetworkSendRequest> {
        self.network.as_mut().map_or_else(Vec::new, |network| {
            std::mem::take(&mut network.send_requests)
        })
    }

    /// Loading reports keep their own queue, so the held loader can send them
    /// while the other requests wait for the ordinary service pass.
    pub(crate) fn take_network_loading_progress_reports(&mut self) -> Vec<i32> {
        self.network.as_mut().map_or_else(Vec::new, |network| {
            std::mem::take(&mut network.loading_reports)
        })
    }

    pub(crate) fn report_network_loading_progress(&mut self, percent: i32) {
        if self.local_connection_watches() {
            return;
        }
        if let Some(network) = self.network.as_mut()
            && matches!(network.phase, NetworkPhase::Frozen { .. })
        {
            network.loading_reports.push(percent);
        }
    }

    /// Queue a request the service sends while the match runs.
    fn queue_frozen_request(&mut self, request: NetworkSendRequest) {
        if let Some(network) = self.network.as_mut()
            && matches!(network.phase, NetworkPhase::Frozen { .. })
        {
            network.send_requests.push(request);
        }
    }

    /// 0x0064AAE0's kick proposal for participant `target_source`, sent to
    /// every other connected node.
    pub(crate) fn request_kick_proposal(&mut self, target_source: usize) {
        self.queue_frozen_request(NetworkSendRequest::ProposeKick { target_source });
    }

    /// One in-battle chat packet to `recipients`.
    pub(crate) fn request_chat(&mut self, recipients: Vec<usize>, text: String) {
        if let Some(network) = self.network.as_mut()
            && matches!(
                network.phase,
                NetworkPhase::Frozen { .. } | NetworkPhase::CooperativeRound { .. }
            )
        {
            network
                .send_requests
                .push(NetworkSendRequest::Chat { recipients, text });
        }
    }

    pub(crate) fn request_beacon(
        &mut self,
        record: crate::net::lan_messages::ingame::BeaconRecord,
    ) {
        self.queue_frozen_request(NetworkSendRequest::Beacon(record));
    }

    pub(crate) fn request_beacon_text(&mut self, house: i8, slot: i8, text: Vec<u16>) {
        self.queue_frozen_request(NetworkSendRequest::BeaconText { house, slot, text });
    }

    pub(crate) fn request_taunt(&mut self, recipients: Vec<usize>, byte: u8) {
        self.queue_frozen_request(NetworkSendRequest::Taunt { recipients, byte });
    }
}
