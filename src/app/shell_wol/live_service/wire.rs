//! VERA HTTPS lobby payloads. These project the existing shell/session owners;
//! they do not create a second gameplay state. See docs/lobby-protocol.md.

use serde::{Deserialize, Serialize};

use crate::app::match_runtime::session::{NetworkMapMetadata, NetworkVacancy};
use crate::app::shell_network_setup::{HostOptions, NetworkSetupEvent};
use crate::app::shell_wol::{WolHighScoreRecord, WolLadderRecord};
use crate::ui::wol_lobby::{WolChannel, WolMember, WolRoomSummary};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Channel {
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
    #[serde(
        default,
        rename = "room_summary",
        deserialize_with = "room_summary",
        skip_serializing
    )]
    pub summary: Option<WolRoomSummary>,
}

// Optional display data must never prevent listing or joining a room with an
// older or newer lobby. The host's create/advertise requests never send it.
fn room_summary<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<WolRoomSummary>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    let Ok(mut summary) = serde_json::from_value::<WolRoomSummary>(value) else {
        return Ok(None);
    };
    let max = crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT;
    if summary.teams.as_ref().is_some_and(|teams| {
        teams.len() > usize::from(*crate::skirmish_launch::ROOM_TEAMS.end())
            || teams.iter().map(|&count| usize::from(count)).sum::<usize>() > max
            || (!teams.is_empty() && teams.iter().filter(|&&count| count > 0).count() < 2)
    }) || [summary.observers, summary.computers]
        .into_iter()
        .flatten()
        .any(|n| usize::from(n) > max)
    {
        return Ok(None);
    }
    summary.map = summary.map.filter(|name| {
        !name.trim().is_empty()
            && name.chars().count() <= 128
            && !name.chars().any(char::is_control)
    });
    Ok(Some(summary))
}

fn unknown_ping() -> i32 {
    -1
}

impl From<WolChannel> for Channel {
    fn from(channel: WolChannel) -> Self {
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
            summary: None,
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
            summary: channel.summary,
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
pub(super) struct Options {
    /// Authored campaign room record, sent only to a lobby that offers
    /// `campaign_rooms` (docs/lobby-protocol.md, HostOptions). The older
    /// fields carry only the lobby's participant/control and map proofs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub campaign: Option<crate::app::shell_network_setup::campaign_room::CampaignRoom>,
    pub launch: crate::skirmish_launch::SkirmishLaunchSession,
    pub map_metadata: NetworkMapMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub random_map: Option<crate::app::shell_network_setup::maps::GeneratedMapManifest>,
    pub vacancies: [NetworkVacancy; crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT],
    pub observer_row: Option<u8>,
    pub cooperative_difficulty: i32,
    /// Cooperative's campaign record, the `C0` text LAN sends as its own
    /// type `0x17` packet (`0x0049D680` through `0x005E3720`). Online it
    /// rides in the options, so every revision and a late join carry it.
    /// Absent in every other mode: lobbies before it refuse the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooperative_campaign: Option<String>,
    pub seed: Option<u32>,
    /// The room's Watching, `delayed` or `live` (docs/lobby-protocol.md
    /// "Watching"). Absent while it is off, which the lobby reads as off;
    /// lobbies before it refuse the field. Guests have no use for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watching: Option<String>,
}

impl Options {
    pub fn into_options(mut self, local: &str) -> anyhow::Result<HostOptions> {
        self.launch.local_player_index = self
            .launch
            .humans
            .iter()
            .position(|human| human.name.eq_ignore_ascii_case(local))
            .ok_or_else(|| anyhow::anyhow!("server roster omits the signed-in player"))?;
        Ok(HostOptions {
            launch: self.launch,
            map_metadata: self.map_metadata,
            vacancies: self.vacancies,
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
        let authored = self.campaign.take();
        let options = self.into_options(local)?;
        if let Some(authored) = authored {
            anyhow::ensure!(
                campaign.is_none(),
                "campaign room contains stock Co-op data"
            );
            return Ok(vec![NetworkSetupEvent::CampaignHostSetup {
                options,
                room: authored,
            }]);
        }
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
    // The operator's welcome, when one is set; the game has none of its own.
    #[serde(default)]
    pub motd: Option<String>,
    /// An account's password sign-in with `remember`: this computer's
    /// remembered sign-in (docs/lobby-protocol.md "Accounts").
    #[serde(default)]
    pub sign_in_token: Option<String>,
    /// What this lobby offers beyond protocol 4: `visit_lobby`
    /// (docs/lobby-protocol.md "Visiting the lobby").
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Deserialize)]
pub(super) struct ChannelList {
    pub kind: u32,
    pub channels: Vec<Channel>,
}

/// `GET /v1/matches` (docs/lobby-protocol.md "Watching"). The game keeps
/// only these fields of each match.
#[derive(Deserialize)]
pub(super) struct MatchList {
    pub matches: Vec<Match>,
}

#[derive(Deserialize)]
pub(super) struct Match {
    pub match_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub map: Option<String>,
    pub watchers: u32,
    pub max_watchers: u32,
    #[serde(default, deserialize_with = "listed_count")]
    pub observers: Option<u8>,
    #[serde(default, deserialize_with = "listed_count")]
    pub computers: Option<u8>,
    #[serde(default)]
    pub names: Vec<String>,
    #[serde(default, rename = "room_summary", deserialize_with = "room_summary")]
    pub summary: Option<WolRoomSummary>,
}

fn listed_count<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<u8>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(value
        .as_u64()
        .and_then(|n| u8::try_from(n).ok())
        .filter(|&n| usize::from(n) <= crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT))
}

impl From<Match> for crate::app::shell_wol::WolMatch {
    fn from(value: Match) -> Self {
        // Older watch lists carry these seat totals on the match itself.
        let summary = value.summary.or_else(|| {
            (value.observers.is_some() || value.computers.is_some()).then_some(WolRoomSummary {
                observers: value.observers,
                computers: value.computers,
                ..Default::default()
            })
        });
        Self {
            match_id: value.match_id,
            name: value.name.unwrap_or_default(),
            map: value.map.unwrap_or_default(),
            watchers: value.watchers,
            max_watchers: value.max_watchers,
            names: value.names,
            summary,
        }
    }
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

/// `watch_started` (docs/lobby-protocol.md "Watching"): the start every
/// player's game got, with the timing it agreed, and this watcher's grant.
#[derive(Deserialize)]
pub(super) struct WatchStarted {
    pub match_id: String,
    pub version: String,
    pub rules_digest: String,
    pub seed: u32,
    /// The room's tournament value (`[0xB779C4]`), and whether it is a
    /// Quick Co-op room: the start terms every player's game took.
    pub tournament: i32,
    pub quick_coop: bool,
    pub users: Vec<WatchedUser>,
    pub options: Options,
    pub pings: Vec<i32>,
    pub relay: WatchGrant,
}

#[derive(Deserialize)]
pub(super) struct WatchedUser {
    pub name: String,
    pub flags: u32,
    pub source_index: u8,
}

// Deliberately no Debug: a watch grant is a credential.
#[derive(Deserialize)]
pub(super) struct WatchGrant {
    pub address: std::net::SocketAddrV4,
    pub ticket: String,
    pub packet_key: String,
    pub payload_key: String,
    pub ordering_certificate: String,
}

/// The `ladder` event for the names a `ladder` command asked for, or the
/// rated players' tables the lobby sends unasked after a rated game, or none
/// when a season starts or ends: the running season's records, which a name
/// without a rated game there lacks, and the badge points of those that have
/// played one.
#[derive(Deserialize)]
pub(super) struct Ladder {
    /// The running season's number; `None` when none runs.
    pub season: Option<u32>,
    pub records: Vec<LadderRecord>,
    /// Absent from a lobby from before all-time badges.
    #[serde(default)]
    pub high_scores: Vec<HighScoreRecord>,
}

impl Ladder {
    /// The ladder table from the season's records, and the high-score table,
    /// whose points pick the badges (`0x00783B80`), from the badge points.
    pub(super) fn tables(&self) -> (Vec<WolLadderRecord>, Vec<WolHighScoreRecord>) {
        (
            self.records.iter().map(WolLadderRecord::from).collect(),
            self.high_scores
                .iter()
                .map(WolHighScoreRecord::from)
                .collect(),
        )
    }
}

/// A player's badge points, the points that pick the badge of their OS, and
/// their rank in the running season, 0 when unranked.
#[derive(Deserialize)]
pub(super) struct HighScoreRecord {
    pub name: String,
    pub points: u32,
    pub rank: u32,
}

/// A player's season record: `points` their OS rounded and `os` their OS to
/// the hundredth, which a lobby from before it leaves out.
#[derive(Deserialize)]
pub(super) struct LadderRecord {
    pub name: String,
    pub points: u32,
    #[serde(default)]
    pub os: Option<f64>,
    pub rank: u32,
    pub wins: u32,
    pub losses: u32,
    pub disconnects: u32,
}

/// The lobby's counts in the native record's signed fields.
fn signed(count: u32) -> i32 {
    i32::try_from(count).unwrap_or(i32::MAX)
}

impl From<&LadderRecord> for WolLadderRecord {
    fn from(record: &LadderRecord) -> Self {
        Self {
            name: record.name.clone(),
            wins: signed(record.wins),
            losses: signed(record.losses),
            points: signed(record.points),
            os_hundredths: record.os.map_or_else(
                || signed(record.points).saturating_mul(100),
                |os| (os * 100.0).round().max(0.0) as i32,
            ),
            rank: signed(record.rank),
            disconnects: signed(record.disconnects),
        }
    }
}

/// The co-op icon's campaign bits have no lobby source and stay clear.
impl From<&HighScoreRecord> for WolHighScoreRecord {
    fn from(record: &HighScoreRecord) -> Self {
        Self {
            name: record.name.clone(),
            points: record.points,
            rank: record.rank,
            coop: 0,
        }
    }
}

#[cfg(all(test, tests_app))]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::app::shell_network_setup::maps::{AssetMetadata, GeneratedMapManifest};
    use crate::map::source::TransferResource;
    use crate::skirmish_launch::{
        AiDifficulty, HostRules, LaunchCountry, LaunchHumanRole, LaunchStartPosition, LaunchTeam,
        ROOM_TEAMS, SkirmishAiSlot, SkirmishHumanSlot, SkirmishLaunchMode, SkirmishLaunchOptions,
        SkirmishLaunchSession, SkirmishLocalSlot,
    };

    #[test]
    fn public_room_summary_is_optional_and_never_sent_by_the_host() {
        let mut room = json!({"kind":41,"max_users":16,"users":8,"name":"Friday Night"});
        let old: Channel = serde_json::from_value(room.clone()).unwrap();
        assert!(WolChannel::from(old).summary.is_none());
        room["room_summary"] = json!({"map":"Heck Freezes Over","teams":[0,4,0,4],"observers":2});
        let record = WolChannel::from(serde_json::from_value::<Channel>(room.clone()).unwrap());
        let summary = record.summary.as_ref().unwrap();
        assert_eq!(summary.teams.as_deref(), Some([0, 4, 0, 4].as_slice()));
        assert_eq!(summary.map.as_deref(), Some("Heck Freezes Over"));
        assert_eq!(summary.observers, Some(2));
        assert!(
            serde_json::to_value(Channel::from(record))
                .unwrap()
                .get("room_summary")
                .is_none()
        );

        room["room_summary"]["teams"] = json!([30, 30]);
        let record = WolChannel::from(serde_json::from_value::<Channel>(room).unwrap());
        assert_eq!(record.name, "Friday Night");
        assert!(record.summary.is_none());

        let watch: Match = serde_json::from_value(json!({
            "match_id":"running","map":"Snow Valley","watchers":1,"max_watchers":10,
            "observers":2,"computers":1
        }))
        .unwrap();
        let watch = crate::app::shell_wol::WolMatch::from(watch);
        let seats = watch
            .summary
            .expect("older lobbies already list seated spectators and AI");
        assert_eq!(
            (seats.observers, seats.computers, seats.teams),
            (Some(2), Some(1), None)
        );
    }

    fn human(row: u8, team: LaunchTeam) -> SkirmishHumanSlot {
        SkirmishHumanSlot {
            raw_slot_index: Some(row),
            name: format!("Player{row}"),
            slot: SkirmishLocalSlot {
                country: LaunchCountry::ALL[usize::from(row) % 10],
                country_random: row % 4 == 3,
                // The original eight colours, then the extra ones to the last, 30.
                color_index: if row < 8 { row } else { row + 3 },
                color_random: false,
                start_position: LaunchStartPosition::Auto,
                team,
            },
            role: LaunchHumanRole::default(),
        }
    }

    fn map(file: &str, player_capacity: i32, official: bool, digest: String) -> NetworkMapMetadata {
        NetworkMapMetadata {
            display_name: file.into(),
            player_capacity,
            official,
            byte_length: TransferResource::Map.limit() as i32,
            digest,
            source_sha256: "5a".repeat(32),
            effective_digest: "e7".repeat(32),
        }
    }

    /// The host's options for stock mode `mode` on `map`, every row closed
    /// and the game's own House roster for its people and AI.
    fn room(
        mode: i32,
        map: NetworkMapMetadata,
        humans: Vec<SkirmishHumanSlot>,
        opponents: Vec<SkirmishAiSlot>,
        options: SkirmishLaunchOptions,
    ) -> Options {
        let launch = SkirmishLaunchSession::from_rows(
            SkirmishLaunchMode::stock(mode).unwrap(),
            Some(map.display_name.clone()),
            humans,
            0,
            opponents,
            options,
        );
        Options {
            launch,
            map_metadata: map,
            random_map: None,
            vacancies: [NetworkVacancy::Closed; crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT],
            observer_row: None,
            cooperative_difficulty: 1,
            cooperative_campaign: None,
            campaign: None,
            seed: Some(0),
            watching: None,
        }
    }

    /// The lobby checks the host's options against its own copy of their
    /// shape and limits (docs/lobby-protocol.md), and its tests check that
    /// copy against these rooms at the limits the game reaches.
    #[test]
    fn rooms_at_the_limits_send_the_pinned_options() {
        // Thirty people: 28 play on all fifteen teams of a 15-start map and
        // two spectators wait for a seat. Every option a room sends now is on.
        let mut people: Vec<_> = (0..28)
            .map(|row| human(row, LaunchTeam::Team(row % ROOM_TEAMS.end())))
            .collect();
        for row in 28..30 {
            let mut spectator = human(row, LaunchTeam::None);
            spectator.set_observer(true);
            spectator.slot.color_index = crate::rules::skirmish_colors::OBSERVER_PRIORITY;
            people.push(spectator);
        }
        let mut own = crate::rules::personal_rules::PersonalRules::default();
        own.set_terrain_deformation(true);
        own.set_superweapon_limit(i32::MAX);
        let options = SkirmishLaunchOptions {
            bans: crate::sim::game_options::RoomBans {
                yuri: true,
                france: true,
                spy: true,
                stolen_tech: true,
            },
            teams: Some(*ROOM_TEAMS.end()),
            queue: vec!["Player28".into(), "Player29".into()],
            host_rules: HostRules::from_own(&own),
            ..SkirmishLaunchOptions::default()
        };
        // A map without a [Digest] sends the reader's default.
        let digest = "No digest here mate. Nope.".to_owned();
        let mut teams = room(
            1,
            map("Fifteen.map", 15, false, digest),
            people,
            vec![],
            options,
        );
        teams.observer_row = Some(29);
        teams.seed = Some(u32::MAX);
        teams.watching = Some("live".into());

        // A generated map for thirty: its seed, preview and thirty starts.
        let mut stage = crate::map::preview::ScenarioStartStaging {
            width: 150,
            height: 150,
            start_x: -75,
            start_y: 0,
            count: 30,
            ..Default::default()
        };
        for start in 0..30 {
            let at = start as i32 * 5;
            stage.pairs[start] = (at, 150 - at);
            stage.cells[start] = (10 + at as i16, 160 - at as i16);
        }
        let start_staging = stage.to_w_text();
        assert!(crate::app::shell_network_setup::maps::checked_staging(&start_staging, 30).is_ok());
        let seed = crate::map::rmg::RmgOptions {
            num_players: 30,
            ..Default::default()
        };
        let ai = SkirmishAiSlot {
            driver_policy: None,
            raw_slot_index: 28,
            country: LaunchCountry::Yuri,
            country_random: false,
            color_index: 30,
            color_random: true,
            start_position: LaunchStartPosition::Position(29),
            team: LaunchTeam::None,
            difficulty: AiDifficulty::Hard,
        };
        let seed_map = map(
            crate::map::skirmish_scenarios::RANDMAP_SED,
            30,
            true,
            seed.native_digest(),
        );
        let people = vec![human(0, LaunchTeam::None), human(1, LaunchTeam::None)];
        let mut generated = room(1, seed_map, people, vec![ai], Default::default());
        generated.random_map = Some(GeneratedMapManifest {
            start_staging,
            preview: AssetMetadata {
                source_sha256: "b0".repeat(32),
                byte_length: TransferResource::Preview.limit(),
            },
        });
        generated.vacancies[2] = NetworkVacancy::Waiting;
        generated.vacancies[3..28].fill(NetworkVacancy::Open);
        generated.vacancies[29] = NetworkVacancy::OpenObserver;
        generated.observer_row = Some(29);

        // Cooperative: two players and the host's campaign record.
        let people = vec![human(0, LaunchTeam::None), human(1, LaunchTeam::None)];
        let digest = "2jmj7l5rSw0yVb/vlWAYkK/YBwk=".to_owned();
        let mut cooperative = room(
            3,
            map("Coop.map", 2, true, digest),
            people,
            vec![],
            Default::default(),
        );
        cooperative.cooperative_difficulty = 2;
        cooperative.cooperative_campaign =
            Some("C0,0,0,0,0,0,0,0,0,0,0,0,1,2,-1,Player0,Player1".into());

        crate::test_fixture::assert_sent_json(
            "tests/fixtures/lobby/options.json",
            &json!({
                "about": "The options a host's game sends (`options`, docs/lobby-protocol.md) \
                    for rooms at the limits the game reaches, keyed by room. The game's \
                    test in src/app/shell_wol/live_service/wire.rs writes this file; \
                    a lobby accepts each room.",
                "rooms": {
                    "fifteen_teams_of_thirty": teams,
                    "generated_map_for_thirty": generated,
                    "cooperative": cooperative,
                },
            }),
        );
    }
}
