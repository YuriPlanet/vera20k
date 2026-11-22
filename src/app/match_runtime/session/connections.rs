//! A running network match's participants: source owners and senders,
//! disconnects, local and remote watchers, and the return to the room
//! (lobby return, WOL room return, LAN rejoin, cooperative rematch seat).

use crate::skirmish_launch::SkirmishHumanSlot;
use std::net::SocketAddrV4;

use super::{
    ConnectionWatcher, LanRoomRejoin, MatchSession, NetworkPhase, NetworkReturnContext,
    NetworkRoomReturn,
};

impl MatchSession {
    /// The frozen launch's owner key of the House network participant
    /// `source_index` controls, its own or one it shares.
    pub(crate) fn network_source_owner(&self, source_index: usize) -> Option<String> {
        if self.campaign_room().is_some() {
            let source = usize::try_from(self.network_source_sender(source_index)?).ok()?;
            return self
                .network_authored_start()?
                .primary_houses
                .get(source)
                .cloned();
        }
        let launch = self.network_setup()?;
        crate::sim::scenario_bootstrap::normalized_launch_slots(launch)
            .into_iter()
            .find(|slot| slot.is_controlled_by(source_index))
            .map(|slot| slot.owner_name)
    }

    /// The network participants controlling the House with owner key `owner`.
    pub(crate) fn network_owner_sources(&self, owner: &str) -> Vec<usize> {
        let Some(launch) = self.network_setup() else {
            return Vec::new();
        };
        if self.campaign_room().is_some() {
            return (0..launch.humans.len())
                .filter(|&index| {
                    self.network_source_owner(index)
                        .is_some_and(|house| house.eq_ignore_ascii_case(owner))
                })
                .collect();
        }
        crate::sim::scenario_bootstrap::normalized_launch_slots(launch)
            .into_iter()
            .find(|slot| slot.owner_name == owner)
            .map_or_else(Vec::new, |slot| {
                slot.controllers
                    .iter()
                    .map(|controller| controller.human_index)
                    .collect()
            })
    }

    /// Network participant `source_index`'s command sender: the header byte
    /// of its records and its timing row. That is its House's index unless
    /// players share Houses.
    pub(crate) fn network_source_sender(&self, source_index: usize) -> Option<i8> {
        let launch = self.network_setup()?;
        if let Some(room) = self.campaign_room() {
            return room.source(&launch.humans.get(source_index)?.name);
        }
        crate::sim::scenario_bootstrap::normalized_launch_slots(launch)
            .iter()
            .flat_map(|slot| &slot.controllers)
            .find(|controller| controller.human_index == source_index)
            .map(|controller| controller.source)
    }

    /// The network participant sending as `sender`.
    pub(crate) fn network_sender_source(&self, sender: i8) -> Option<usize> {
        let launch = self.network_setup()?;
        if let Some(room) = self.campaign_room() {
            let name = room.player(sender)?;
            return launch
                .humans
                .iter()
                .position(|human| human.name.eq_ignore_ascii_case(name));
        }
        crate::sim::scenario_bootstrap::normalized_launch_slots(launch)
            .iter()
            .flat_map(|slot| &slot.controllers)
            .find(|controller| controller.source == sender)
            .map(|controller| controller.human_index)
    }

    /// VERA shared control: whether another connected participant controls
    /// the House participant `source_index` controls.
    pub(crate) fn network_teammate_connected(&self, source_index: usize) -> bool {
        let Some(launch) = self.network_setup() else {
            return false;
        };
        let connected = self.network_connected_source_indices();
        if self.campaign_room().is_some() {
            return self
                .network_source_owner(source_index)
                .is_some_and(|owner| {
                    self.network_owner_sources(&owner)
                        .into_iter()
                        .any(|source| source != source_index && connected.contains(&source))
                });
        }
        crate::sim::scenario_bootstrap::normalized_launch_slots(launch)
            .iter()
            .find(|slot| slot.is_controlled_by(source_index))
            .is_some_and(|slot| {
                slot.controllers.iter().any(|controller| {
                    controller.human_index != source_index
                        && connected.contains(&controller.human_index)
                })
            })
    }

    /// DestroyConnection's node removal (`0x005DAA8A`, `--[0xA8B54C]`): the
    /// participant leaves the live membership. Its private connection
    /// (`IPXManager::Delete_Connection 0x00540F00`, called at `0x005DA85D`)
    /// is removed by `network_match::destroy_connection`. False when it is
    /// the local player or no longer connected.
    pub(crate) fn disconnect_network_participant(&mut self, source_index: usize) -> bool {
        let Some(network) = self.network.as_mut() else {
            return false;
        };
        let (startup, connections) = match &mut network.phase {
            NetworkPhase::Frozen {
                startup,
                connections,
                ..
            }
            | NetworkPhase::CampaignResult {
                startup,
                connections,
                ..
            } => (startup, connections),
            _ => return false,
        };
        if source_index == startup.launch_session().local_player_index
            && !connections
                .watching_sources
                .iter()
                .any(|watcher| watcher.local)
        {
            return false;
        }
        let playing = &mut connections.playing_sources;
        let Some(position) = playing.iter().position(|index| *index == source_index) else {
            return false;
        };
        playing.remove(position);
        true
    }

    pub(crate) fn network_connected_source_indices(&self) -> &[usize] {
        match self.network.as_ref().map(|network| &network.phase) {
            Some(
                NetworkPhase::Frozen { connections, .. }
                | NetworkPhase::CampaignResult { connections, .. }
                | NetworkPhase::CooperativeRound { connections, .. },
            ) => &connections.playing_sources,
            _ => &[],
        }
    }

    /// The setup roster before launch, and the surviving playing sources
    /// when preparing a continuation from the frozen launch.
    pub(crate) fn campaign_launch_humans(&self) -> impl Iterator<Item = &SkirmishHumanSlot> {
        let playing = self
            .campaign_result()
            .map(|_| self.network_connected_source_indices());
        self.network_setup()
            .into_iter()
            .flat_map(|launch| launch.humans.iter().enumerate())
            .filter(move |(index, _)| playing.is_none_or(|sources| sources.contains(index)))
            .map(|(_, human)| human)
    }

    /// The authenticated relay route's local connection is a viewer. The
    /// launch still identifies the House it initially follows.
    pub(crate) fn admit_local_network_watcher(
        &mut self,
        endpoint: SocketAddrV4,
    ) -> anyhow::Result<()> {
        let Some(watching) = self.network_watching_sources_mut() else {
            anyhow::bail!("watch admission requires an active setup or match");
        };
        anyhow::ensure!(
            !watching.iter().any(|source| source.local),
            "local watch already admitted"
        );
        watching.push(ConnectionWatcher {
            endpoint,
            local: true,
        });
        Ok(())
    }

    pub(crate) fn local_connection_watches(&self) -> bool {
        match self.network.as_ref().map(|network| &network.phase) {
            Some(NetworkPhase::Setup {
                watching_sources, ..
            }) => watching_sources,
            Some(
                NetworkPhase::Frozen { connections, .. }
                | NetworkPhase::CampaignResult { connections, .. }
                | NetworkPhase::CooperativeRound { connections, .. },
            ) => &connections.watching_sources,
            _ => return false,
        }
        .iter()
        .any(|source| source.local)
    }

    pub(crate) fn disconnect_network_watcher(&mut self, endpoint: SocketAddrV4) -> bool {
        let Some(watching) = self.network_watching_sources_mut() else {
            return false;
        };
        let Some(index) = watching
            .iter()
            .position(|source| source.endpoint == endpoint)
        else {
            return false;
        };
        watching.remove(index);
        true
    }

    fn network_watching_sources_mut(&mut self) -> Option<&mut Vec<ConnectionWatcher>> {
        match &mut self.network.as_mut()?.phase {
            NetworkPhase::Setup {
                watching_sources, ..
            } => Some(watching_sources),
            NetworkPhase::Frozen { connections, .. }
            | NetworkPhase::CampaignResult { connections, .. }
            | NetworkPhase::CooperativeRound { connections, .. } => {
                Some(&mut connections.watching_sources)
            }
            NetworkPhase::Lobby { .. } => None,
        }
    }

    /// Match exit or failed loading returns to the active service lobby while
    /// retaining its connection/return context. A later setup is a new admission.
    pub(crate) fn return_to_network_lobby(&mut self) -> Option<NetworkReturnContext> {
        let network = self.network.as_mut()?;
        let room_return = network.phase.room_return_mut().map(std::mem::take);
        network.phase = NetworkPhase::Lobby {
            room_return: room_return.unwrap_or_default(),
        };
        network.campaign_room = None;
        network.endpoints.clear();
        network.send_requests.clear();
        network.loading_reports.clear();
        network.map_metadata = None;
        network.local_map_source = None;
        network.local_map_effective_digest = None;
        network.local_random_map_preview = None;
        network.proposed_seed = None;
        network.max_ahead_frames = None;
        Some(network.return_context)
    }

    /// Attach the choice to the admitted start only after freeze succeeds.
    pub(crate) fn set_network_room_return(&mut self, choice: NetworkRoomReturn) {
        if let Some(
            NetworkPhase::Frozen { room_return, .. }
            | NetworkPhase::CampaignResult { room_return, .. },
        ) = self.network.as_mut().map(|network| &mut network.phase)
        {
            *room_return = choice;
        }
    }

    pub(crate) fn awaiting_wol_room_return(&self) -> bool {
        matches!(
            self.network.as_ref().map(|network| &network.phase),
            Some(NetworkPhase::Lobby {
                room_return: NetworkRoomReturn::Wol { .. }
            })
        )
    }

    pub(crate) fn take_network_room_return(&mut self) -> NetworkRoomReturn {
        self.network
            .as_mut()
            .and_then(|network| network.phase.room_return_mut())
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// The shell has returned to discovery. Retain the single rejoin intent
    /// here until the old room is listed, the guest leaves, or it expires.
    pub(crate) fn wait_for_lan_room(
        &mut self,
        host: String,
        game_id: u64,
        now: crate::util::time::Instant,
    ) {
        if let Some(NetworkPhase::Lobby { room_return }) =
            self.network.as_mut().map(|network| &mut network.phase)
        {
            *room_return = NetworkRoomReturn::LanGuest {
                host,
                game_id,
                until: Some(now + crate::util::time::Duration::from_secs(120)),
            };
        }
    }

    pub(crate) fn poll_lan_room_rejoin(
        &mut self,
        now: crate::util::time::Instant,
        room_open: impl FnOnce(u64) -> bool,
    ) -> Option<LanRoomRejoin> {
        let NetworkPhase::Lobby { room_return } = &mut self.network.as_mut()?.phase else {
            return None;
        };
        let NetworkRoomReturn::LanGuest {
            host,
            game_id,
            until: Some(until),
        } = room_return
        else {
            return None;
        };
        let outcome = if now >= *until {
            LanRoomRejoin::Expired(std::mem::take(host))
        } else if room_open(*game_id) {
            LanRoomRejoin::Join(*game_id)
        } else {
            return None;
        };
        *room_return = NetworkRoomReturn::Lobby;
        Some(outcome)
    }

    pub(crate) fn await_cooperative_rematch_seat(&mut self) {
        if let Some(NetworkPhase::Setup {
            cooperative_rematch_seat,
            ..
        }) = self.network.as_mut().map(|network| &mut network.phase)
        {
            *cooperative_rematch_seat = true;
        }
    }

    pub(crate) fn take_cooperative_rematch_seat(&mut self) -> bool {
        match self.network.as_mut().map(|network| &mut network.phase) {
            Some(NetworkPhase::Setup {
                cooperative_rematch_seat,
                ..
            }) => std::mem::take(cooperative_rematch_seat),
            _ => false,
        }
    }
}
