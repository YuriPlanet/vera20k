//! Authored campaign room choices, verified against the installed series.
//! The accepted room is shared above the simulation; loading consumes its
//! authored launch and a local proof of the exact bytes.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::app::match_bootstrap::PreparedCampaignStart;
use crate::map::campaign_manifest::{CampaignManifest, CampaignStage};
use crate::map::source::{LoadedMapSource, is_logical_name, valid_source_sha256};
use crate::sim::campaign_carry_over::StageCarryOver;
use crate::sim::scenario_bootstrap::AuthoredCampaignStart;
use crate::skirmish_launch::{NETWORK_PLAYER_SLOT_COUNT, SkirmishLaunchSession};
use crate::util::file_system;

const CAMPAIGNS: &str = "campaigns";
const MANIFEST: &str = "campaign.ini";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CampaignRoom {
    series: String,
    stage: String,
    manifest_sha256: String,
    map_sha256: String,
    effective_map_digest: String,
    pub(crate) attempt: u32,
    player_limit: u8,
    difficulty: i32,
    playing_sources: Vec<PlayingSource>,
    carry_over: StageCarryOver,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayingSource {
    player: String,
    role: String,
}

impl CampaignRoom {
    pub(crate) fn carry_over(&self) -> &StageCarryOver {
        &self.carry_over
    }

    pub(crate) fn stage(&self) -> &str {
        &self.stage
    }

    pub(crate) fn player_names(&self) -> impl Iterator<Item = &str> {
        self.playing_sources.iter().map(|seat| seat.player.as_str())
    }

    pub(crate) fn player_limit(&self) -> u8 {
        self.player_limit
    }

    pub(crate) fn source(&self, player: &str) -> Option<i8> {
        self.playing_sources
            .iter()
            .position(|seat| seat.player.eq_ignore_ascii_case(player))
            .and_then(|index| i8::try_from(index).ok())
    }

    pub(crate) fn player(&self, source: i8) -> Option<&str> {
        self.playing_sources
            .get(usize::try_from(source).ok()?)
            .map(|seat| seat.player.as_str())
    }

    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            is_logical_name(&self.series)
                && is_logical_name(&self.stage)
                && valid_source_sha256(&self.manifest_sha256)
                && valid_source_sha256(&self.map_sha256)
                && valid_source_sha256(&self.effective_map_digest)
                && (0..=2).contains(&self.difficulty)
                && self.attempt != 0
                && (1..=NETWORK_PLAYER_SLOT_COUNT as u8).contains(&self.player_limit),
            "invalid campaign room identity"
        );
        ensure!(
            !self.playing_sources.is_empty()
                && self.playing_sources.len() <= NETWORK_PLAYER_SLOT_COUNT
                && self
                    .playing_sources
                    .iter()
                    .all(|seat| !seat.player.is_empty())
                && self.playing_sources.windows(2).all(|pair| {
                    pair[0].player.to_ascii_lowercase() < pair[1].player.to_ascii_lowercase()
                }),
            "invalid or noncanonical campaign source roster"
        );
        self.carry_over.validate()?;
        Ok(())
    }

    fn with_roster(mut self, launch: &SkirmishLaunchSession, stage: &CampaignStage) -> Self {
        let mut names = launch
            .humans
            .iter()
            .filter(|human| !human.is_observer())
            .map(|human| human.name.clone())
            .collect::<Vec<_>>();
        names.sort_by_cached_key(|name| name.to_ascii_lowercase());
        let previous = std::mem::take(&mut self.playing_sources);
        let mut assigned = BTreeSet::new();
        self.playing_sources = names
            .into_iter()
            .map(|player| {
                let role = previous
                    .iter()
                    .find(|seat| seat.player.eq_ignore_ascii_case(&player))
                    .and_then(|seat| {
                        stage
                            .roles
                            .iter()
                            .find(|role| role.name.eq_ignore_ascii_case(&seat.role))
                    })
                    .filter(|role| assigned.insert(role.name.to_ascii_lowercase()))
                    .map_or_else(String::new, |role| role.name.clone());
                PlayingSource { player, role }
            })
            .collect();
        for seat in &mut self.playing_sources {
            if seat.role.is_empty()
                && let Some(role) = stage
                    .roles
                    .iter()
                    .find(|role| assigned.insert(role.name.to_ascii_lowercase()))
            {
                seat.role.clone_from(&role.name);
            }
        }
        self
    }

    fn compile(
        &self,
        launch: &SkirmishLaunchSession,
        stage: &CampaignStage,
        stage_index: usize,
    ) -> Result<AuthoredCampaignStart> {
        self.validate()?;
        let count = self.playing_sources.len();
        ensure!(
            count >= usize::from(stage.min_players.unwrap_or(1))
                && count <= usize::from(self.player_limit)
                && launch.humans.len() == count
                && launch
                    .humans
                    .iter()
                    .all(|human| { !human.is_observer() && self.source(&human.name).is_some() }),
            "campaign playing roster does not meet stage bounds"
        );
        let mut stage = stage.clone();
        if stage.roles.is_empty() {
            ensure!(
                count == 1 && self.playing_sources[0].role.is_empty(),
                "a campaign stage without roles permits one player"
            );
        } else {
            let mut assigned = BTreeSet::new();
            stage.roles = self
                .playing_sources
                .iter()
                .map(|seat| {
                    stage
                        .roles
                        .iter()
                        .find(|role| role.name.eq_ignore_ascii_case(&seat.role))
                        .filter(|role| assigned.insert(role.name.to_ascii_lowercase()))
                        .cloned()
                        .with_context(|| format!("unavailable campaign role for {}", seat.player))
                })
                .collect::<Result<Vec<_>>>()?;
            stage
                .grants
                .retain(|grant| assigned.contains(&grant.role.to_ascii_lowercase()));
        }
        AuthoredCampaignStart::from_stage(
            &stage,
            stage_index,
            self.difficulty,
            self.carry_over.clone(),
        )
    }
}

pub(super) struct CampaignSeries {
    id: String,
    manifest: CampaignManifest,
}

fn campaign_roots(state: &AppState) -> Vec<PathBuf> {
    state
        .platform
        .game_config
        .as_ref()
        .map_or_else(Vec::new, |config| {
            crate::map::source::game_folders(&config.paths.ra2_dir)
                .into_iter()
                .rev()
                .map(|root| root.join(CAMPAIGNS))
                .collect()
        })
}

pub(super) fn discover(state: &AppState) -> Vec<CampaignSeries> {
    let mut series = Vec::new();
    let mut seen = BTreeSet::new();
    for root in campaign_roots(state) {
        let Ok(entries) = file_system::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(id) = entry.file_name().into_string() else {
                continue;
            };
            let series_path = entry.path();
            if !is_logical_name(&id)
                || !file_system::is_file(series_path.join(MANIFEST))
                || !seen.insert(id.to_ascii_lowercase())
            {
                continue;
            }
            let result = confined_manifest(&root, &series_path)
                .and_then(|path| Ok(CampaignManifest::load(path)?));
            match result {
                Ok(manifest) => series.push(CampaignSeries { id, manifest }),
                Err(error) => log::debug!("Campaign series {id} is unavailable: {error:#}"),
            }
        }
    }
    let locale = crate::i18n::selected_language();
    series.sort_by_cached_key(|entry| {
        (
            entry.manifest.title(&locale).to_lowercase(),
            entry.id.clone(),
        )
    });
    series
}

fn confined_manifest(root: &Path, series: &Path) -> Result<PathBuf> {
    let root = file_system::canonicalize(root)?;
    let series = file_system::canonicalize(series)?;
    ensure!(
        series != root && series.starts_with(&root),
        "campaign series escaped its library"
    );
    let path = file_system::canonicalize(series.join(MANIFEST))?;
    ensure!(
        path.starts_with(&series),
        "campaign manifest escaped its series"
    );
    Ok(path)
}

fn find_manifest(state: &AppState, id: &str) -> Result<(PathBuf, CampaignManifest)> {
    ensure!(is_logical_name(id), "invalid campaign series");
    for root in campaign_roots(state) {
        let Ok(entries) = file_system::read_dir(&root) else {
            continue;
        };
        let mut matches = entries.flatten().filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(id))
                && file_system::is_file(entry.path().join(MANIFEST))
        });
        if let Some(entry) = matches.next() {
            ensure!(matches.next().is_none(), "ambiguous campaign series {id}");
            let path = confined_manifest(&root, &entry.path())?;
            let manifest = CampaignManifest::load(&path)?;
            return Ok((path, manifest));
        }
    }
    anyhow::bail!("campaign series {id} is unavailable")
}

fn checked_manifest(state: &AppState, room: &CampaignRoom) -> Result<(PathBuf, CampaignManifest)> {
    room.validate()?;
    let (path, manifest) = find_manifest(state, &room.series)?;
    ensure!(
        manifest.digest == room.manifest_sha256,
        "campaign manifest differs from the host"
    );
    Ok((path, manifest))
}

pub(crate) fn load_manifest(state: &AppState, room: &CampaignRoom) -> Result<CampaignManifest> {
    checked_manifest(state, room).map(|(_, manifest)| manifest)
}

fn stage_map(
    state: &AppState,
    manifest_path: &Path,
    stage: &CampaignStage,
) -> Result<(String, LoadedMapSource, String)> {
    let series = manifest_path
        .parent()
        .context("campaign manifest has no series directory")?;
    let map = file_system::canonicalize(series.join(&stage.map))?;
    ensure!(map.starts_with(series), "campaign map escaped its series");
    let name = Path::new(&stage.map)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| is_logical_name(name))
        .context("campaign map has no logical name")?;
    let config = state
        .platform
        .game_config
        .as_ref()
        .context("campaign has no game configuration")?;
    let assets = state
        .process_assets
        .manager()
        .context("campaign has no game assets")?;
    let loaded = crate::map::source::load_network_map_ini(
        &config.paths.ra2_dir,
        map.to_str().context("campaign map path is not UTF-8")?,
        assets,
    )?;
    ensure!(
        matches!(&loaded.source, LoadedMapSource::Loose { path, .. } if path == &map),
        "campaign map resolved to another file"
    );
    crate::map::source::validate_network_map_content(&loaded.ini, false)?;
    let roster = crate::map::houses::parse_house_roster(&loaded.ini, &[], None);
    stage.validate_house_names(roster.houses.iter().map(|house| house.name.as_str()))?;
    let source = loaded.source;
    let effective = crate::map::source::effective_network_map_digest(&loaded.ini, false, false);
    let (_, digest) = source
        .physical_identity()
        .context("campaign map has no digest")?;
    ensure!(
        stage
            .map_sha256
            .as_deref()
            .is_none_or(|expected| expected == digest),
        "campaign map differs from the manifest"
    );
    Ok((name.to_owned(), source, effective))
}

fn stage_player_limit(stage: &CampaignStage) -> u8 {
    let declared = usize::from(stage.max_players.unwrap_or(u8::MAX));
    declared
        .min(stage.roles.len().max(1))
        .min(NETWORK_PLAYER_SLOT_COUNT) as u8
}

fn verified_map(
    state: &AppState,
    room: &CampaignRoom,
) -> Result<(CampaignManifest, usize, String, LoadedMapSource)> {
    let (path, manifest) = checked_manifest(state, room)?;
    let index = manifest
        .stage_index(&room.stage)
        .context("unknown campaign stage")?;
    let stage = &manifest.stages[index];
    ensure!(
        room.player_limit == stage_player_limit(stage),
        "campaign room player limit differs from its stage"
    );
    let (name, source, effective) = stage_map(state, &path, stage)?;
    ensure!(
        source
            .physical_identity()
            .is_some_and(|(_, digest)| digest == room.map_sha256),
        "campaign map differs from the host"
    );
    ensure!(
        effective == room.effective_map_digest,
        "campaign map includes differ from the host"
    );
    Ok((manifest, index, name, source))
}

pub(crate) fn verify_current(state: &AppState, room: &CampaignRoom) -> Result<()> {
    verified_map(state, room).map(|_| ())
}

pub(crate) fn prepare_current(
    state: &AppState,
    room: &CampaignRoom,
) -> Result<PreparedCampaignStart> {
    let (manifest, index, _, map_source) = verified_map(state, room)?;
    let map_name = format!(
        "{CAMPAIGNS}/{}/{}",
        room.series,
        manifest.stages[index].map.replace('\\', "/")
    );
    let session = &state.match_state.session;
    let launch = session
        .network_setup()
        .context("campaign requires a network setup")?;
    let authored = room.compile(launch, &manifest.stages[index], index)?;
    let local_source = if session.local_connection_watches() {
        None
    } else {
        Some(
            room.source(&launch.local_player().name)
                .context("local campaign role not admitted")?,
        )
    };
    Ok(PreparedCampaignStart {
        authored,
        map_name,
        map_source,
        map_effective_digest: room.effective_map_digest.clone(),
        local_source,
        game_speed: launch.options.game_speed,
    })
}

pub(crate) fn prepared_start(state: &AppState) -> Result<Option<PreparedCampaignStart>> {
    state
        .match_state
        .session
        .campaign_room()
        .map(|room| prepare_current(state, room))
        .transpose()
}

fn selected_room(
    state: &AppState,
    series: &str,
    id: &str,
    previous: Option<&CampaignRoom>,
    carry_over: StageCarryOver,
) -> Result<CampaignRoom> {
    let (path, manifest) = find_manifest(state, series)?;
    if let Some(previous) = previous {
        ensure!(
            previous.manifest_sha256 == manifest.digest,
            "campaign manifest changed"
        );
    }
    let stage = manifest.stage(id).context("unknown campaign stage")?;
    let (_, source, effective_map_digest) = stage_map(state, &path, stage)?;
    let (_, map_sha256) = source
        .physical_identity()
        .context("campaign map has no digest")?;
    let launch = state
        .match_state
        .session
        .network_setup()
        .context("campaign requires a network setup")?;
    let (playing_sources, difficulty, attempt) = if let Some(previous) = previous {
        (
            previous.playing_sources.clone(),
            previous.difficulty,
            previous
                .attempt
                .checked_add(1)
                .context("campaign attempt exhausted")?,
        )
    } else {
        (Vec::new(), 1, 1)
    };
    let room = CampaignRoom {
        series: series.to_owned(),
        stage: stage.id.clone(),
        manifest_sha256: manifest.digest.clone(),
        map_sha256: map_sha256.to_owned(),
        effective_map_digest,
        attempt,
        player_limit: stage_player_limit(stage),
        difficulty,
        playing_sources,
        carry_over,
    }
    .with_roster(launch, stage);
    room.validate()?;
    Ok(room)
}

pub(crate) fn prepare_stage(
    state: &AppState,
    id: &str,
    carry_over: StageCarryOver,
) -> Result<CampaignRoom> {
    let previous = state
        .match_state
        .session
        .campaign_room()
        .context("no active campaign room")?;
    selected_room(state, &previous.series, id, Some(previous), carry_over)
}

/// Publication rebinds names against the current admitted participants after
/// joins or leaves. Source IDs are assigned from this canonical projection.
pub(crate) fn publication(state: &AppState) -> Result<Option<CampaignRoom>> {
    let session = &state.match_state.session;
    let Some(room) = session.campaign_room() else {
        return Ok(None);
    };
    let manifest = load_manifest(state, room)?;
    let stage = manifest
        .stage(&room.stage)
        .context("unknown campaign stage")?;
    let launch = session
        .network_setup()
        .context("campaign has no playing roster")?;
    let room = room.clone().with_roster(launch, stage);
    room.validate()?;
    Ok(Some(room))
}

pub(crate) fn local_map_verified(
    session: &crate::app::match_runtime::session::MatchSession,
) -> Option<bool> {
    session.campaign_room()?;
    Some(
        session
            .network_setup()
            .is_some_and(|setup| session.network_map_status(setup.local_player_index) == Some(1)),
    )
}

pub(crate) fn report_local_files(state: &mut AppState, verified: bool) -> Result<()> {
    let session = &mut state.match_state.session;
    let local = session
        .network_setup()
        .context("campaign has no network setup")?
        .local_player_index;
    let status = u8::from(verified);
    session.set_network_map_status(local, status)?;
    if let Some(protocol) = &mut state.frontend.network_setup_protocol {
        protocol.accept_eligible = verified;
        if !session.host() && !protocol.external {
            protocol
                .requests
                .retain(|request| !matches!(request, super::NetworkSetupRequest::MapStatus(_)));
            protocol
                .requests
                .push(super::NetworkSetupRequest::MapStatus(status));
        }
    }
    if !verified {
        super::chat_notice(state, "VERA:CampaignRoomUnavailable", &[]);
    }
    Ok(())
}

pub(crate) fn online_map_metadata(
    state: &AppState,
    room: &CampaignRoom,
) -> Result<(
    String,
    crate::app::match_runtime::session::NetworkMapMetadata,
)> {
    let (manifest, index, name, source) = verified_map(state, room)?;
    let stage = &manifest.stages[index];
    let (bytes, _) = source
        .physical_identity()
        .context("campaign has no physical map")?;
    let capacity = stage_player_limit(stage);
    Ok((
        name,
        crate::app::match_runtime::session::NetworkMapMetadata {
            display_name: stage.title(&crate::i18n::selected_language()).to_owned(),
            player_capacity: i32::from(capacity),
            official: false,
            byte_length: i32::try_from(bytes)?,
            digest: String::new(),
            source_sha256: room.map_sha256.clone(),
            effective_digest: room.effective_map_digest.clone(),
        },
    ))
}

pub(crate) fn online_agreement(room: &CampaignRoom) -> (&str, &str) {
    (&room.map_sha256, &room.effective_map_digest)
}

pub(super) fn speed_and_options(state: &AppState) -> Result<i32> {
    state
        .match_state
        .session
        .network_setup()
        .map(|launch| launch.options.game_speed)
        .context("campaign has no setup")
}

fn committed_room(state: &mut AppState, room: Option<CampaignRoom>) -> Result<()> {
    let speed = speed_and_options(state)?;
    let campaign = room.is_some();
    state
        .match_state
        .session
        .accept_campaign_room(room, speed)?;
    super::invalidate_network_agreement(state)?;
    if campaign {
        report_local_files(state, true)?;
    }
    super::actions::queue(state, super::NetworkSetupRequest::PublishOptions);
    Ok(())
}

pub(super) fn set_difficulty(state: &mut AppState, difficulty: i32) -> Result<()> {
    let mut room = state
        .match_state
        .session
        .campaign_room()
        .context("no selected campaign")?
        .clone();
    if room.difficulty != difficulty {
        room.difficulty = difficulty;
        committed_room(state, Some(room))?;
    }
    Ok(())
}

pub(crate) fn resume_after_wol_result(state: &mut AppState, room: CampaignRoom) -> Result<()> {
    verify_current(state, &room)?;
    committed_room(state, Some(room))?;
    report_local_files(state, true)?;
    super::actions::request_campaign_start(state)
}

fn selected_series(state: &AppState) -> Option<&CampaignSeries> {
    let room = state.match_state.session.campaign_room()?;
    state
        .frontend
        .network_setup_protocol
        .as_ref()?
        .campaign_catalog
        .iter()
        .find(|entry| {
            entry.id.eq_ignore_ascii_case(&room.series)
                && entry.manifest.digest == room.manifest_sha256
        })
}

pub(super) fn visible_stage(state: &AppState) -> Option<&CampaignStage> {
    let room = state.match_state.session.campaign_room()?;
    selected_series(state)?.manifest.stage(&room.stage)
}

pub(super) fn stage_label(state: &AppState) -> Option<String> {
    let room = state.match_state.session.campaign_room()?;
    Some(visible_stage(state).map_or_else(
        || room.stage.clone(),
        |stage| stage.title(&crate::i18n::selected_language()).to_owned(),
    ))
}

pub(super) fn difficulty(state: &AppState) -> Option<i32> {
    state
        .match_state
        .session
        .campaign_room()
        .map(|room| room.difficulty)
}

fn choice(value: i32, text: String) -> crate::ui::multiplayer_setup::SetupChoice {
    crate::ui::multiplayer_setup::SetupChoice {
        value,
        text,
        swatch: None,
    }
}

pub(super) fn setup_selectors(state: &AppState) -> Vec<crate::ui::multiplayer_setup::SetupCombo> {
    use crate::ui::multiplayer_setup::{CAMPAIGN_SERIES, CAMPAIGN_STAGE, SetupCombo};

    let Some(protocol) = state.frontend.network_setup_protocol.as_ref() else {
        return Vec::new();
    };
    let entries = &protocol.campaign_catalog;
    let room = state.match_state.session.campaign_room();
    if (entries.is_empty() || !state.match_state.session.host()) && room.is_none() {
        return Vec::new();
    }
    let host = state.match_state.session.host();
    let locale = crate::i18n::selected_language();
    let mut choices = vec![choice(0, crate::i18n::tr("VERA:CampaignModeStock"))];
    choices.extend(entries.iter().enumerate().filter_map(|(index, entry)| {
        Some(choice(
            i32::try_from(index + 1).ok()?,
            entry.manifest.title(&locale).to_owned(),
        ))
    }));
    let selected = if let Some(room) = room {
        Some(
            entries
                .iter()
                .position(|entry| entry.id.eq_ignore_ascii_case(&room.series))
                .map_or_else(
                    || {
                        let fallback = choices.len();
                        choices.push(choice(-1, room.series.clone()));
                        fallback
                    },
                    |index| index + 1,
                ),
        )
    } else {
        Some(0)
    };
    let mut selectors = vec![SetupCombo {
        id: CAMPAIGN_SERIES,
        row: 0,
        enabled: host,
        visible: true,
        selected,
        choices,
        list_w: 0,
    }];
    if let Some(room) = room {
        let mut choices = selected_series(state).map_or_else(Vec::new, |entry| {
            entry
                .manifest
                .stages_by_title(&locale)
                .into_iter()
                .filter_map(|stage| {
                    let index = i32::try_from(entry.manifest.stage_index(&stage.id)?).ok()?;
                    Some(choice(index, stage.title(&locale).to_owned()))
                })
                .collect::<Vec<_>>()
        });
        let selected = selected_series(state)
            .and_then(|entry| entry.manifest.stage_index(&room.stage))
            .and_then(|stage| {
                choices
                    .iter()
                    .position(|choice| i32::try_from(stage).ok() == Some(choice.value))
            });
        let selected = selected.or_else(|| {
            choices.push(choice(-1, room.stage.clone()));
            Some(choices.len() - 1)
        });
        selectors.push(SetupCombo {
            id: CAMPAIGN_STAGE,
            row: 0,
            enabled: host,
            visible: true,
            selected,
            choices,
            list_w: 0,
        });
    }
    selectors
}

pub(super) fn role_choices(
    state: &AppState,
    name: &str,
) -> (
    Vec<crate::ui::multiplayer_setup::SetupChoice>,
    Option<usize>,
) {
    let room = state.match_state.session.campaign_room();
    let assigned = room.and_then(|room| {
        room.playing_sources
            .iter()
            .find(|seat| seat.player.eq_ignore_ascii_case(name))
    });
    let stage = visible_stage(state);
    let mut choices = stage.map_or_else(Vec::new, |stage| {
        stage
            .roles
            .iter()
            .enumerate()
            .filter_map(|(index, role)| {
                Some(choice(
                    i32::try_from(index).ok()?,
                    format!("{} ({})", role.name, role.house),
                ))
            })
            .collect::<Vec<_>>()
    });
    if let Some(index) = stage.and_then(|stage| {
        stage.roles.iter().position(|role| {
            assigned.is_some_and(|seat| role.name.eq_ignore_ascii_case(&seat.role))
        })
    }) {
        return (choices, Some(index));
    }
    let text = assigned.filter(|seat| !seat.role.is_empty()).map_or_else(
        || crate::i18n::tr("VERA:CampaignRoleMissing"),
        |seat| seat.role.clone(),
    );
    let index = choices.len();
    choices.push(choice(-1, text));
    (choices, Some(index))
}

pub(super) fn select(state: &mut AppState, control: u16, row: u8, value: i32) -> Result<()> {
    use crate::ui::multiplayer_setup::{CAMPAIGN_SERIES, CAMPAIGN_STAGE};

    ensure!(
        state.match_state.session.host(),
        "only the host assigns campaign roles"
    );
    if control == CAMPAIGN_SERIES && value == 0 {
        return committed_room(state, None);
    }
    if super::dialog(state).wol() && !crate::app::shell_wol::lobby_campaign_rooms(state) {
        super::chat_notice(state, "VERA:CampaignRoomOnlineUnsupported", &[]);
        return Ok(());
    }
    let next = (|| -> Result<Option<CampaignRoom>> {
        if control == CAMPAIGN_SERIES {
            let entry = state
                .frontend
                .network_setup_protocol
                .as_ref()
                .context("no setup")?
                .campaign_catalog
                .get(usize::try_from(value - 1)?)
                .context("unknown campaign series")?;
            let first = entry
                .manifest
                .stages
                .first()
                .context("campaign has no stages")?;
            return selected_room(state, &entry.id, &first.id, None, Default::default()).map(Some);
        }
        let room = state
            .match_state
            .session
            .campaign_room()
            .context("no selected campaign")?;
        let entry = selected_series(state).context("campaign manifest unavailable")?;
        let index = usize::try_from(value)?;
        if control == CAMPAIGN_STAGE {
            let stage = entry.manifest.stages.get(index).context("unknown stage")?;
            return selected_room(state, &entry.id, &stage.id, Some(room), Default::default())
                .map(Some);
        }
        let mut room = room.clone();
        let stage = entry
            .manifest
            .stage(&room.stage)
            .context("unknown campaign stage")?;
        let roster = state
            .match_state
            .session
            .network_setup()
            .context("no admitted campaign setup")?;
        let player = roster
            .humans
            .iter()
            .find(|human| human.raw_slot_index == Some(row))
            .context("unknown campaign participant")?;
        room = room.with_roster(roster, stage);
        let prior = room
            .source(&player.name)
            .and_then(|source| usize::try_from(source).ok())
            .context("campaign participant is not playing")?;
        let role = stage.roles.get(index).context("unknown campaign role")?;
        if let Some(existing) = room
            .playing_sources
            .iter()
            .position(|seat| seat.role.eq_ignore_ascii_case(&role.name))
        {
            let previous = room.playing_sources[prior].role.clone();
            room.playing_sources[existing].role = previous;
        }
        room.playing_sources[prior].role.clone_from(&role.name);
        Ok(Some(room))
    })();
    match next {
        Ok(room) => committed_room(state, room),
        Err(error) => {
            log::warn!("Campaign setup edit refused: {error:#}");
            super::chat_notice(state, "VERA:CampaignRoomUnavailable", &[]);
            Ok(())
        }
    }
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    #[test]
    fn room_compiles_shared_houses_and_source_scoped_grants_in_canonical_order() {
        let manifest = CampaignManifest::read(
            "campaigns/match/campaign.ini",
            &b"[Stage First]\nMap=stable.map\nMinPlayers=2\nMaxPlayers=2\n[Roles First]\nBob=Shared\nAlice=Shared\n[Grants First]\nBob.Third=FullControl\nAlice.Third=UnitsOnly\nAlice.Fourth=FullControl\n"[..],
        ).unwrap();
        let stage = &manifest.stages[0];
        let profile = crate::test_fixture::json("tools/map_observation.command-bar.example.json");
        let mut launch: SkirmishLaunchSession =
            serde_json::from_value(profile["launch"].clone()).unwrap();
        launch.humans[0].name = "ZHost".into();
        let mut guest = launch.humans[0].clone();
        guest.name = "AGuest".into();
        guest.raw_slot_index = Some(1);
        launch.humans.push(guest);
        let room = CampaignRoom {
            series: "match".into(),
            stage: "First".into(),
            manifest_sha256: manifest.digest.clone(),
            map_sha256: "a".repeat(64),
            effective_map_digest: "b".repeat(64),
            attempt: 1,
            player_limit: 2,
            difficulty: 2,
            playing_sources: vec![
                PlayingSource {
                    player: "AGuest".into(),
                    role: "Alice".into(),
                },
                PlayingSource {
                    player: "ZHost".into(),
                    role: "Bob".into(),
                },
            ],
            carry_over: Default::default(),
        };
        let start = room.compile(&launch, stage, 0).unwrap();
        assert_eq!(room.source("ZHOST"), Some(1));
        assert_eq!(start.roles()[0].name, "Alice");
        assert_eq!(start.roles()[1].name, "Bob");
        assert!(start.roles().iter().all(|role| role.house == "Shared"));
        assert_eq!(start.difficulty(), 2);
        launch.humans.pop();
        assert!(
            room.compile(&launch, stage, 0).is_err(),
            "minimum playing count"
        );
    }
}
