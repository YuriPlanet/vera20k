//! VERA HTTPS lobby payloads. These project the existing shell/session owners;
//! they do not create a second gameplay state. See docs/lobby-protocol.md.

use serde::{Deserialize, Serialize};

use crate::app::match_runtime::session::{NetworkMapMetadata, NetworkVacancy};
use crate::app::shell_network_setup::{HostOptions, NetworkSetupEvent};
use crate::ui::wol_lobby::{WolChannel, WolMember};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Channel {
    #[serde(default)]
    pub id: String,
    pub kind: u32,
    pub max_users: i32,
    pub users: i32,
    #[serde(default)]
    pub tournament: i32,
    #[serde(default)]
    pub flags: u32,
    #[serde(default)]
    pub game_flags: u32,
    #[serde(default = "unknown_ping")]
    pub ping: i32,
    pub name: String,
    #[serde(default)]
    pub ex_info: String,
}

fn unknown_ping() -> i32 {
    -1
}

impl From<WolChannel> for Channel {
    fn from(channel: WolChannel) -> Self {
        Self {
            id: String::new(),
            kind: channel.kind,
            max_users: channel.max_users,
            users: channel.users,
            tournament: channel.tournament,
            flags: channel.flags,
            game_flags: channel.game_flags,
            ping: channel.ping,
            name: channel.name,
            ex_info: channel.ex_info,
        }
    }
}

impl From<Channel> for WolChannel {
    fn from(channel: Channel) -> Self {
        Self {
            kind: channel.kind,
            max_users: channel.max_users,
            users: channel.users,
            tournament: channel.tournament,
            flags: channel.flags,
            game_flags: channel.game_flags,
            ping: channel.ping,
            name: channel.name,
            ex_info: channel.ex_info,
        }
    }
}

#[derive(Deserialize)]
pub(super) struct Member {
    pub player_id: String,
    pub name: String,
    #[serde(default)]
    pub flags: u32,
    #[serde(default)]
    pub squad: i32,
    #[serde(default)]
    pub clan_name: String,
    #[serde(default)]
    pub clan_tag: String,
}

impl Member {
    pub fn into_member(self, local: &str) -> WolMember {
        let flags = (self.flags & !32768)
            | if self.name.eq_ignore_ascii_case(local) {
                32768
            } else {
                0
            };
        WolMember {
            flags,
            squad: self.squad,
            name: self.name,
            clan_name: self.clan_name,
            clan_tag: self.clan_tag,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct MapMetadata {
    pub display_name: String,
    pub player_capacity: i32,
    pub official: bool,
    pub byte_length: i32,
    pub digest: String,
    pub source_sha256: String,
    pub effective_digest: String,
}

impl From<MapMetadata> for NetworkMapMetadata {
    fn from(value: MapMetadata) -> Self {
        Self {
            display_name: value.display_name,
            player_capacity: value.player_capacity,
            official: value.official,
            byte_length: value.byte_length,
            digest: value.digest,
            source_sha256: value.source_sha256,
            effective_digest: value.effective_digest,
        }
    }
}

impl From<NetworkMapMetadata> for MapMetadata {
    fn from(value: NetworkMapMetadata) -> Self {
        Self {
            display_name: value.display_name,
            player_capacity: value.player_capacity,
            official: value.official,
            byte_length: value.byte_length,
            digest: value.digest,
            source_sha256: value.source_sha256,
            effective_digest: value.effective_digest,
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Vacancy {
    Open,
    Closed,
    Observer,
    Waiting,
}

impl From<NetworkVacancy> for Vacancy {
    fn from(value: NetworkVacancy) -> Self {
        match value {
            NetworkVacancy::Open => Self::Open,
            NetworkVacancy::Closed => Self::Closed,
            NetworkVacancy::OpenObserver => Self::Observer,
            NetworkVacancy::Waiting => Self::Waiting,
        }
    }
}

impl From<Vacancy> for NetworkVacancy {
    fn from(value: Vacancy) -> Self {
        match value {
            Vacancy::Open => Self::Open,
            Vacancy::Closed => Self::Closed,
            Vacancy::Observer => Self::OpenObserver,
            Vacancy::Waiting => Self::Waiting,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Options {
    pub launch: crate::skirmish_launch::SkirmishLaunchSession,
    pub map_metadata: MapMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub random_map: Option<crate::app::shell_network_setup::maps::GeneratedMapManifest>,
    pub vacancies: [Vacancy; crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT],
    pub observer_row: Option<u8>,
    pub cooperative_difficulty: i32,
    /// Cooperative's campaign record, the `C0` text LAN sends as its own
    /// type `0x17` packet (`0x0049D680` through `0x005E3720`). Online it
    /// rides in the options, so every revision and a late join carry it.
    /// Absent in every other mode: lobbies before it refuse the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooperative_campaign: Option<String>,
    pub seed: Option<u32>,
}

impl Options {
    pub fn into_options(mut self, local: &str) -> anyhow::Result<HostOptions> {
        self.launch.local_player_index = self
            .launch
            .humans
            .iter()
            .position(|human| human.name.eq_ignore_ascii_case(local))
            .ok_or_else(|| anyhow::anyhow!("server roster omits the signed-in player"))?;
        self.launch.rebuild_house_roster();
        Ok(HostOptions {
            launch: self.launch,
            map_metadata: self.map_metadata.into(),
            vacancies: self.vacancies.map(Into::into),
            observer_row: self.observer_row,
            cooperative_difficulty: self.cooperative_difficulty,
            seed: self.seed,
        })
    }

    /// What a guest's options sink makes of one host options event: the
    /// host's `C0` first, as the native host sends it ahead of its options
    /// (`+0x94` at the join accept `0x007A6E5A`, Use Map's `+0x60` before
    /// its publish), so the campaign's country refresh never outlives the
    /// host's rows; then the options themselves.
    pub fn into_setup_events(mut self, local: &str) -> anyhow::Result<Vec<NetworkSetupEvent>> {
        let campaign = self.cooperative_campaign.take();
        let options = self.into_options(local)?;
        Ok(campaign
            .map(NetworkSetupEvent::CooperativeCampaign)
            .into_iter()
            .chain([NetworkSetupEvent::HostSetup(options)])
            .collect())
    }
}

#[derive(Deserialize)]
pub(super) struct Envelope {
    pub sequence: u64,
    pub kind: String,
    pub payload: serde_json::Value,
}

#[derive(Deserialize)]
pub(super) struct EventBatch {
    pub events: Vec<Envelope>,
    pub cursor: u64,
}

#[derive(Deserialize)]
pub(super) struct Session {
    pub player_id: String,
    pub nickname: String,
    pub token: String,
    pub expires_at: u64,
    // The operator's welcome, when one is set; the game's own otherwise.
    #[serde(default)]
    pub motd: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct ChannelList {
    pub kind: u32,
    pub channels: Vec<Channel>,
}

#[derive(Deserialize)]
pub(super) struct Joined {
    pub channel: Channel,
    pub members: Vec<Member>,
}

#[derive(Deserialize)]
pub(super) struct OptionsEvent {
    pub revision: u64,
    pub options: Options,
}

#[derive(Deserialize)]
pub(super) struct StartUser {
    pub player_id: String,
    pub name: String,
    pub flags: u32,
    pub source_index: u8,
}

// Deliberately no Debug: relay admission is a credential.
#[derive(Deserialize)]
pub(super) struct RelayGrant {
    pub address: std::net::SocketAddrV4,
    pub ticket: String,
    pub packet_key: String,
    pub payload_key: String,
    pub source_index: u8,
    pub ordering_certificate: String,
    // Present when the lobby homes the match on several regional relays.
    #[serde(default)]
    pub federation: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct Start {
    pub match_id: String,
    pub revision: u64,
    pub version: String,
    pub rules_digest: String,
    pub seed: u32,
    pub users: Vec<StartUser>,
    pub relay: RelayGrant,
}
