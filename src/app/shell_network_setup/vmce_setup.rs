//! Local VMCE setup actions on the shared LAN chooser and room.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, ensure};

use crate::app::AppState;
use crate::app::match_bootstrap::PreparedNetworkStartup;
use crate::app::match_runtime::session::{
    LocalScenarioLobby, LocalScenarioObserver, LocalScenarioPlayer, LocalScenarioRules,
    LocalScenarioSource, MatchSession, NetworkReturnContext,
};
use crate::assets::asset_manager::AssetManager;
use crate::assets::csf_file::CsfFile;
use crate::map::scenario_menu::{
    LocalScenario, LocalScenarioKey, LocalScenarioMenuEntry, LocalScenarioRoleAppearance,
    LocalScenarioRoleVisual,
};
use crate::map::vmce::{self, DiscoveredVmceMap, OpenedVmceStage, VmceMapId, VmceStageOptions};
use crate::net::lan_messages::SessionScope;
use crate::sim::campaign_carry_over::AuthoredCampaignCarryOver;
use crate::sim::house_state::HouseDifficulty;
use crate::skirmish_launch::{NETWORK_PLAYER_SLOT_COUNT, SkirmishLaunchOptions};
use crate::ui::multiplayer_setup::{
    DIFFICULTY, RoomBox, ScenarioRoleDetails, ScenarioRoomRow, ScenarioRoomView, SetupAction,
    SetupChoice, SetupColumn, SetupCombo, SetupDialog, SetupRowIcon, SetupTrackbar, SetupView,
    roster_list_width,
};
use crate::ui::shell::trackbar::TrackbarRange;
use crate::ui::skirmish_shell::ChooseMapModalLayout;

use super::NetworkSetupRequest;

#[path = "vmce_setup/room_return.rs"]
mod room_return;
pub(crate) use room_return::{ReturningGuestRoles, RoomReturn, restore_admitted_role};

const MODE_KEY: &str = "vmce";
const MODE_LABEL: &str = "VMCE";

pub(crate) fn lan_mode_label() -> String {
    crate::i18n::tr("VERA:VmceModeName")
}

pub(crate) fn lan_filter_label() -> String {
    crate::i18n::tr("VERA:VmceOnlyRooms")
}

fn maps_root(state: &AppState) -> Option<PathBuf> {
    state
        .platform
        .game_config
        .as_ref()
        .map(|config| config.paths.downloaded_maps_dir())
}

fn can_choose(state: &AppState) -> bool {
    state.frontend.shell_route.network_setup()
        && can_choose_in_room(&state.match_state.session)
        && state
            .frontend
            .network_setup_protocol
            .as_ref()
            .is_some_and(|protocol| protocol.editable() && !protocol.external)
}

fn can_choose_in_room(session: &MatchSession) -> bool {
    session.host()
        && session.network_return_context() == Some(NetworkReturnContext::LanLobby)
        && session.network_setup_capacity().is_some()
        && session.network_setup().is_some_and(|launch| {
            launch.opponents.is_empty() && !launch.local_player().is_observer()
        })
}

fn menu_entry(
    map: DiscoveredVmceMap,
    base: Option<&CsfFile>,
    available: bool,
) -> LocalScenarioMenuEntry {
    let series = map.series_display_title(base).to_owned();
    let stage = map
        .stage_display_title(base)
        .map_or_else(|| stage_number(map.stage_number), str::to_owned);
    let players = player_range(map.min_players, map.max_players);
    let entry = if map.is_entry {
        format!(" · {}", crate::i18n::tr("VERA:VmceEntryStage"))
    } else {
        String::new()
    };
    LocalScenarioMenuEntry {
        selection: LocalScenario {
            mode_label: MODE_LABEL,
            title: format!("{series} / {stage}"),
            key: scenario_key(&map.id),
            presentation: None,
        },
        group: map.id.series,
        group_title: series,
        label: format!("{:02} · {stage}{entry} · {players}", map.stage_number),
        stage_title: stage,
        players,
        unavailable: (!available).then(|| {
            crate::i18n::tr(if map.path.is_file() {
                "VERA:VmceInvalidStage"
            } else {
                "VERA:VmceMissingStage"
            })
        }),
        no_preview: crate::i18n::tr("VERA:VmceNoPreview"),
    }
}

fn stage_number(number: usize) -> String {
    crate::i18n::tr_fill("VERA:VmceStageNumber", &[("number", &number)])
}

fn stage_selection(stage: &OpenedVmceStage, base: Option<&CsfFile>) -> LocalScenario {
    let presentation = stage.presentation(base, stage_number(stage.record.stage_number));
    selected_display(stage, presentation)
}

fn selected_display(
    stage: &OpenedVmceStage,
    presentation: crate::map::scenario_menu::LocalScenarioPresentation,
) -> LocalScenario {
    LocalScenario {
        mode_label: MODE_LABEL,
        title: format!(
            "{} / {}",
            presentation.series_title, presentation.stage_title
        ),
        key: scenario_key(&stage.record.id),
        presentation: Some(Arc::new(presentation)),
    }
}

/// Authored presentation uses the same cold rules and map layers as startup.
struct RoomRuleSources<'a> {
    assets: &'a AssetManager,
    rules: &'a crate::rules::process_owner::NativeRulesProcessOwner,
}

impl<'a> RoomRuleSources<'a> {
    fn for_room(assets: &'a crate::app::process_assets::ProcessAssets) -> Option<Self> {
        Some(Self {
            assets: assets.manager()?,
            rules: assets.native_rules()?,
        })
    }

    fn bind(&self, selection: &mut LocalScenario, map: &crate::rules::ini_parser::IniFile) {
        let Some(display) = selection.presentation.as_mut().and_then(Arc::get_mut) else {
            return;
        };
        match self.rules.scenario_presentation_rules(None, map) {
            Ok(rules) => {
                for role in &mut display.role_appearances {
                    role.resolved_visual = Some(resolve_role_visual(&rules, role));
                }
            }
            Err(error) => {
                log::warn!("VMCE stage role visuals could not read effective rules: {error:#}")
            }
        }
    }
}

pub(crate) fn role_title(selection: &LocalScenario, role: &str, country: Option<&str>) -> String {
    let number = role
        .strip_prefix("Player")
        .and_then(|index| index.parse::<usize>().ok())
        .filter(|index| *index > 0);
    let title = number
        .and_then(|number| {
            selection
                .presentation
                .as_ref()?
                .role_titles
                .get(number - 1)?
                .as_deref()
        })
        .map_or_else(
            || {
                number.map_or_else(
                    || crate::i18n::tr("VERA:VmceRole"),
                    |number| crate::i18n::tr_fill("VERA:VmceRoleNumber", &[("number", &number)]),
                )
            },
            str::to_owned,
        );
    if let Some(country) =
        country.filter(|country| !country.is_empty() && *country != title.as_str())
    {
        format!("{title} · {country}")
    } else {
        title
    }
}

fn scenario_key(id: &VmceMapId) -> LocalScenarioKey {
    LocalScenarioKey {
        source: MODE_KEY,
        path: format!("{}/{}", id.series, id.stage),
    }
}

fn selected_scenario(session: &MatchSession) -> Option<&LocalScenario> {
    session
        .local_network_scenario()
        .filter(|map| map.key.source == MODE_KEY)
}

fn discover(root: &Path, base: Option<&CsfFile>) -> Vec<LocalScenarioMenuEntry> {
    let discovery = vmce::discover(root);
    for issue in discovery.errors {
        log::warn!("VMCE {}: {:#}", issue.path.display(), issue.reason);
    }
    let mut maps: Vec<_> = discovery
        .maps
        .into_iter()
        .map(|map| (map, true))
        .chain(discovery.unavailable.into_iter().map(|map| (map, false)))
        .collect();
    maps.sort_by(|(a, _), (b, _)| {
        (&a.id.series, !a.is_entry, a.stage_number).cmp(&(
            &b.id.series,
            !b.is_entry,
            b.stage_number,
        ))
    });
    maps.into_iter()
        .map(|(map, available)| menu_entry(map, base, available))
        .collect()
}

pub(crate) fn add_choose_map_rows(state: &mut AppState, layout: &ChooseMapModalLayout) {
    if !can_choose(state) {
        return;
    }
    let maps = maps_root(state).map_or_else(Vec::new, |root| {
        discover(&root, state.process_assets.csf.as_ref())
    });
    let current = selected_scenario(&state.match_state.session).map(|map| &map.key);
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        protocol.scenario_chooser_preview = None;
    }
    if let Some(modal) = state
        .frontend
        .skirmish_shell_state
        .choose_map_modal
        .as_mut()
    {
        modal.add_local_scenarios(
            MODE_LABEL,
            crate::i18n::tr("VERA:VmceModeName"),
            maps,
            current,
            layout,
        );
    }
}

pub(crate) fn chooser_media(
    state: &mut AppState,
    selection: &LocalScenario,
) -> Option<(
    Arc<crate::map::scenario_menu::LocalScenarioPresentation>,
    Vec<Option<[u8; 3]>>,
)> {
    let protocol = state.frontend.network_setup_protocol.as_ref()?;
    if protocol
        .scenario_chooser_preview
        .as_ref()
        .is_none_or(|cached| cached.key != selection.key)
    {
        let preview = maps_root(state)
            .zip(map_id(&selection.key).ok())
            .and_then(|(root, id)| vmce::open_stage(&root, &id).ok())
            .map_or_else(
                || selection.clone(),
                |stage| stage_selection(&stage, state.process_assets.csf.as_ref()),
            );
        state
            .frontend
            .network_setup_protocol
            .as_mut()?
            .scenario_chooser_preview = Some(preview);
    }
    let presentation = state
        .frontend
        .network_setup_protocol
        .as_ref()?
        .scenario_chooser_preview
        .as_ref()?
        .presentation
        .clone()?;
    let colors = presentation
        .role_appearances
        .iter()
        .map(|role| role_appearance(state, role).rgb)
        .collect();
    Some((presentation, colors))
}

pub(crate) fn choose(state: &mut AppState, key: &LocalScenarioKey) {
    if let Err(error) = select(state, key) {
        log::warn!("Could not accept VMCE map selection: {error:#}");
        notice(
            state,
            &format!(
                "VMCE could not open the selected map. Check the series files and choose again. {error:#}"
            ),
        );
    }
}

fn notice(state: &mut AppState, text: &str) {
    crate::app::shell_lan::append_chat(
        state,
        crate::ui::lan_shell::LanChatLine {
            text: text.encode_utf16().collect(),
            color: u32::MAX,
        },
    );
}

fn map_id(key: &LocalScenarioKey) -> Result<VmceMapId> {
    ensure!(key.source == MODE_KEY, "unknown chooser source");
    let (series, stage) = key
        .path
        .split_once('/')
        .context("missing series or stage")?;
    let id = VmceMapId {
        series: series.to_owned(),
        stage: stage.to_owned(),
    };
    id.validate()?;
    Ok(id)
}

pub(crate) enum StageContinuation {
    Next {
        key: LocalScenarioKey,
        funds: Vec<(String, String)>,
    },
    Credits(Option<String>),
    SameStage,
}

pub(crate) fn stage_continuation(
    state: &AppState,
    current: &LocalScenarioKey,
    configuration_sha256: &str,
    terminal_globals: &[bool; crate::map::triggers::SCENARIO_GLOBAL_COUNT],
) -> Result<StageContinuation> {
    let root = maps_root(state).context("no configured Maps directory")?;
    let current = map_id(current)?;
    Ok(
        match vmce::stage_continuation(&root, &current, configuration_sha256, terminal_globals)? {
            vmce::VmceContinuation::Next { id, funds } => StageContinuation::Next {
                key: scenario_key(&id),
                funds,
            },
            vmce::VmceContinuation::Credits(text) => StageContinuation::Credits(text),
            vmce::VmceContinuation::SameStage => StageContinuation::SameStage,
        },
    )
}

/// Verify the same authored input on this machine before acknowledging reload.
pub(crate) fn verify_replay_source(
    state: &AppState,
    startup: &PreparedNetworkStartup,
) -> Result<()> {
    let authored = startup
        .authored_scenario()
        .context("no frozen authored map")?;
    let key = &authored.selection.key;
    let root = maps_root(state).context("no configured Maps directory")?;
    let stage = vmce::open_stage(&root, &map_id(key)?)?;
    ensure!(
        stage.manifest_sha256 == authored.configuration_sha256,
        "the scenario manifest changed since the match started"
    );
    ensure!(
        stage.map.source.physical_identity().is_some()
            && stage.map.source.physical_identity()
                == startup
                    .local_map_source()
                    .and_then(|source| source.physical_identity()),
        "the current map changed since the match started"
    );
    let assets = state
        .process_assets
        .manager()
        .context("the game assets are unavailable")?;
    let digest = checked_map_digest(stage.map, assets, key)?;
    ensure!(
        startup.effective_map_digest() == Some(digest.as_str()),
        "the effective map changed"
    );
    Ok(())
}

fn select(state: &mut AppState, key: &LocalScenarioKey) -> Result<()> {
    ensure!(
        can_choose(state),
        "VMCE requires an editable local LAN room"
    );
    let root = maps_root(state).context("no configured Maps directory")?;
    let visual_rules = RoomRuleSources::for_room(&state.process_assets);
    accept_selection(
        &mut state.match_state.session,
        &root,
        key,
        state.process_assets.csf.as_ref(),
        visual_rules.as_ref(),
    )?;
    crate::app::lan_service::invalidate_setup_options(state)?;
    state.frontend.random_map_retention.select_map("");
    state.frontend.skirmish_preview_texture = None;
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        protocol.invalidate_received_preview(true);
        protocol.accept_eligible = false;
        protocol.scenario_return_roles = None;
        protocol.republish_setup();
    }
    Ok(())
}

fn accept_selection(
    session: &mut MatchSession,
    root: &Path,
    key: &LocalScenarioKey,
    base: Option<&CsfFile>,
    visual_rules: Option<&RoomRuleSources<'_>>,
) -> Result<()> {
    ensure!(
        can_choose_in_room(session),
        "VMCE requires a playing LAN host and no lobby AI"
    );
    let stage = vmce::open_stage(root, &map_id(key)?)?;
    let source = stage_source(&stage);
    let selection = if let Some(rules) = visual_rules {
        let effective = checked_map_ini(stage_input(&stage), rules.assets, key)?;
        room_stage_selection(&stage, base, &effective.ini, Some(rules))
    } else {
        stage_selection(&stage, base)
    };
    let lobby = stage_lobby(&stage.options, stage.primary_houses)?;
    session.select_local_network_scenario(selection, source, lobby)
}

fn stage_source(stage: &OpenedVmceStage) -> LocalScenarioSource {
    LocalScenarioSource {
        configuration_sha256: stage.manifest_sha256.clone(),
        map: stage.map.source.clone(),
    }
}

fn stage_input(stage: &OpenedVmceStage) -> crate::map::source::LoadedMapIni {
    crate::map::source::LoadedMapIni {
        ini: stage.map.ini.clone(),
        source: stage.map.source.clone(),
    }
}

fn room_stage_selection(
    stage: &OpenedVmceStage,
    base: Option<&CsfFile>,
    effective: &crate::rules::ini_parser::IniFile,
    visual_rules: Option<&RoomRuleSources<'_>>,
) -> LocalScenario {
    let presentation =
        stage.presentation_from_map(base, stage_number(stage.record.stage_number), effective);
    let mut selection = selected_display(stage, presentation);
    if let Some(rules) = visual_rules {
        rules.bind(&mut selection, effective);
    }
    selection
}

fn stage_lobby(options: &VmceStageOptions, roles: Vec<String>) -> Result<LocalScenarioLobby> {
    ensure!(
        (1..=NETWORK_PLAYER_SLOT_COUNT).contains(&options.min_players)
            && (1..=roles.len()).contains(&options.max_players)
            && roles.len() <= NETWORK_PLAYER_SLOT_COUNT
            && roles.iter().enumerate().all(|(index, house)| {
                !house.trim().is_empty()
                    && !house.chars().any(char::is_control)
                    && roles[..index]
                        .iter()
                        .all(|previous| !previous.eq_ignore_ascii_case(house))
            }),
        "invalid VMCE stage settings"
    );
    Ok(LocalScenarioLobby {
        minimum_players: u8::try_from(options.min_players)?,
        capacity: u8::try_from(options.max_players)?,
        roles,
        allows_observers: options.allow_observers,
        rules: LocalScenarioRules {
            options: SkirmishLaunchOptions {
                short_game: false,
                crates: false,
                ally_change_allowed: options.allow_allies_change,
                ..Default::default()
            },
            ai_difficulty: ai_difficulty(options.lobby_difficulty)?,
            difficulty_locked: options.lock_difficulty,
            alliance_admission:
                crate::sim::house_diplomacy::PlayerAllianceAdmission::RoomParticipants,
            auto_enemy_selection: options.auto_enemy_selection,
        },
    })
}

fn ai_difficulty(lobby_row: i32) -> Result<HouseDifficulty> {
    // The authored menu and map triggers use easiest-first rows. House
    // difficulty tables are hardest-first.
    2_i32
        .checked_sub(lobby_row)
        .and_then(HouseDifficulty::from_native)
        .context("VMCE difficulty must be between 0 and 2")
}

fn room_settings(lobby: &LocalScenarioLobby) -> VmceStageOptions {
    let rules = &lobby.rules;
    VmceStageOptions {
        min_players: usize::from(lobby.minimum_players),
        max_players: usize::from(lobby.capacity),
        allow_allies_change: rules.options.ally_change_allowed,
        allow_observers: lobby.allows_observers,
        auto_enemy_selection: rules.auto_enemy_selection,
        lobby_difficulty: 2 - rules.ai_difficulty.table_index() as i32,
        lock_difficulty: rules.difficulty_locked,
    }
}

fn permits_room_settings(stage: &VmceStageOptions, accepted: &VmceStageOptions) -> bool {
    let mut permitted = stage.clone();
    if !stage.lock_difficulty {
        permitted.lobby_difficulty = accepted.lobby_difficulty;
    }
    permitted == *accepted
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioSeat {
    row: u8,
    name: String,
    role: Option<u8>,
    observer: bool,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScenarioOptions {
    mode: String,
    series: String,
    stage: String,
    manifest_sha256: String,
    map_sha256: String,
    effective_digest: String,
    seed: u32,
    settings: VmceStageOptions,
    roles: Vec<String>,
    seats: Vec<ScenarioSeat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    carry_over: Option<AuthoredCampaignCarryOver>,
}

#[derive(Debug, thiserror::Error)]
#[error("Host options disagree with the settings and roles in the installed VMCE stage")]
struct StageOptionsMismatch;

pub(crate) enum LocalMapCheck {
    Verified {
        source: LocalScenarioSource,
        selection: LocalScenario,
    },
    Unavailable(anyhow::Error),
}

pub(crate) fn publish_options(
    state: &AppState,
) -> Result<Option<(ScenarioOptions, LocalScenarioSource)>> {
    let session = &state.match_state.session;
    let Some(selection) = selected_scenario(session) else {
        return Ok(None);
    };
    let root = maps_root(state).context("no configured Maps directory")?;
    let assets = state
        .process_assets
        .manager()
        .context("the game assets are unavailable")?;
    options_from_selection(session, &root, &selection.key, assets).map(Some)
}

fn checked_map_digest(
    map: crate::map::source::LoadedMapIni,
    assets: &AssetManager,
    key: &LocalScenarioKey,
) -> Result<String> {
    let effective = checked_map_ini(map, assets, key)?;
    // Authored stages are maps. The Ares argument affects generated seeds only.
    Ok(crate::map::source::effective_network_map_digest(
        &effective.ini,
        false,
        false,
    ))
}

fn checked_map_ini(
    map: crate::map::source::LoadedMapIni,
    assets: &AssetManager,
    key: &LocalScenarioKey,
) -> Result<crate::map::source::LoadedMapIni> {
    let effective = crate::map::source::resolve_network_map_ini(map, assets, false, &key.path)?;
    crate::map::source::validate_network_map_content(&effective.ini, false)?;
    Ok(effective)
}

fn options_from_selection(
    session: &MatchSession,
    root: &Path,
    key: &LocalScenarioKey,
    assets: &AssetManager,
) -> Result<(ScenarioOptions, LocalScenarioSource)> {
    ensure!(session.host(), "the host publishes VMCE options");
    let id = map_id(key)?;
    // Recheck the selected files. Room settings come from the accepted setup,
    // including any permitted edits since those files were selected.
    let stage = vmce::open_stage(root, &id)?;
    let source = stage_source(&stage);
    session.verify_local_scenario_source(key, &source)?;
    let settings = room_settings(
        session
            .local_scenario_lobby()
            .context("no admitted VMCE setup")?,
    );
    ensure!(
        permits_room_settings(&stage.options, &settings),
        StageOptionsMismatch
    );
    let (_, map_sha256) = source
        .map
        .physical_identity()
        .context("missing map digest")?;
    let effective_digest = checked_map_digest(stage.map, assets, key)?;
    let options = ScenarioOptions {
        mode: MODE_KEY.to_owned(),
        series: id.series,
        stage: id.stage,
        manifest_sha256: source.configuration_sha256.clone(),
        map_sha256: map_sha256.to_owned(),
        effective_digest,
        seed: session
            .network_start_seed()
            .context("VMCE options have no admitted LAN seed")?,
        settings,
        roles: stage.primary_houses,
        seats: scenario_seats(session)?,
        carry_over: session.local_scenario_carry_over().cloned(),
    };
    options.validate()?;
    Ok((options, source))
}

fn scenario_seats(session: &MatchSession) -> Result<Vec<ScenarioSeat>> {
    let launch = session
        .network_setup()
        .context("the room has no accepted setup")?;
    let mut seats = launch
        .humans
        .iter()
        .enumerate()
        .map(|(source, human)| {
            let row = human
                .raw_slot_index
                .with_context(|| format!("{} has no accepted VMCE lobby seat", human.name))?;
            Ok(ScenarioSeat {
                row,
                name: human.name.clone(),
                role: session.local_scenario_role(source),
                observer: human.is_observer(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    seats.sort_unstable_by_key(|seat| seat.row);
    Ok(seats)
}

impl ScenarioOptions {
    fn picks(&self) -> Vec<(String, u8)> {
        self.seats
            .iter()
            .filter_map(|seat| Some((seat.name.clone(), seat.role?)))
            .collect()
    }

    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(self.mode == MODE_KEY, "unknown authored LAN mode");
        VmceMapId {
            series: self.series.clone(),
            stage: self.stage.clone(),
        }
        .validate()?;
        ensure!(
            crate::map::source::valid_source_sha256(&self.manifest_sha256)
                && crate::map::source::valid_source_sha256(&self.map_sha256)
                && crate::map::source::valid_source_sha256(&self.effective_digest),
            "invalid VMCE source digest"
        );
        ensure!(
            self.settings.min_players <= self.settings.max_players,
            "the VMCE minimum exceeds the available LAN seats"
        );
        let lobby = stage_lobby(&self.settings, self.roles.clone())?;
        ensure!(
            self.carry_over
                .as_ref()
                .is_none_or(|record| record.matches_roles(&self.roles)),
            "VMCE carry-over does not match the selected roles"
        );
        ensure!(
            self.seats.iter().any(|seat| !seat.observer)
                && self.seats.iter().enumerate().all(|(index, seat)| {
                    lobby.admits_row(seat.row, seat.observer)
                        && (!seat.observer || seat.role.is_none())
                        && self.seats[..index].iter().all(|previous| {
                            previous.row < seat.row
                                && !previous.name.eq_ignore_ascii_case(&seat.name)
                        })
                }),
            "invalid VMCE lobby seats"
        );
        lobby.validate_picks(&self.picks())
    }

    fn selection(&self) -> LocalScenario {
        LocalScenario {
            key: LocalScenarioKey {
                source: MODE_KEY,
                path: format!("{}/{}", self.series, self.stage),
            },
            title: self.series.clone(),
            mode_label: MODE_LABEL,
            presentation: None,
        }
    }

    pub(crate) fn validate_seat_names<'a>(
        &self,
        names: impl Iterator<Item = &'a str> + Clone,
    ) -> Result<()> {
        ensure!(
            names.clone().count() == self.seats.len()
                && self.seats.iter().all(|seat| {
                    names
                        .clone()
                        .any(|admitted| admitted.eq_ignore_ascii_case(&seat.name))
                }),
            "the LAN roster does not match the VMCE seats and role choices"
        );
        Ok(())
    }

    pub(crate) fn install(
        &self,
        session: &mut MatchSession,
        local_selection: Option<LocalScenario>,
    ) -> Result<()> {
        let launch = session.network_setup().context("no admitted VMCE room")?;
        ensure!(
            launch
                .humans
                .iter()
                .all(|human| self.seats.iter().any(|seat| {
                    seat.name.eq_ignore_ascii_case(&human.name)
                        && seat.observer == human.is_observer()
                        && human.raw_slot_index.is_none_or(|row| seat.row == row)
                })),
            "the VMCE player or observer seats differ from the admitted roster"
        );
        let selection = local_selection.unwrap_or_else(|| self.selection());
        let key = selection.key.clone();
        session.accept_remote_local_scenario(
            selection,
            stage_lobby(&self.settings, self.roles.clone())?,
            &self.picks(),
            self.seed,
        )?;
        session.set_local_scenario_carry_over(&key, self.carry_over.clone())
    }

    pub(crate) fn apply(&self, state: &mut AppState, check: LocalMapCheck) -> Result<()> {
        let local_selection = match &check {
            LocalMapCheck::Verified { selection, .. } => Some(selection.clone()),
            LocalMapCheck::Unavailable(_) => None,
        };
        let key = self.selection().key;
        let selection_changed =
            selected_scenario(&state.match_state.session).map(|map| &map.key) != Some(&key);
        self.install(&mut state.match_state.session, local_selection)?;
        if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
            if selection_changed {
                protocol.scenario_ready_intent = None;
            }
            protocol.invalidate_received_preview(selection_changed);
            protocol.accept_eligible = false;
        }
        // The same stage path can be republished with new approved map bytes.
        state.frontend.skirmish_preview_texture = None;
        if let Some(ui) = state.frontend.multiplayer_setup.as_mut()
            && (matches!(ui.open_box, Some(RoomBox::RoleDetails(_)))
                || selection_changed && ui.open_box == Some(RoomBox::Briefing))
        {
            ui.show_box(None);
        }
        match check {
            LocalMapCheck::Verified { source, .. } => {
                crate::app::lan_service::verify_setup_agreement(state)?;
                self.accept_map(state, source)?;
                if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
                    protocol.finish_return_verification(Some(&key));
                }
            }
            LocalMapCheck::Unavailable(error) => {
                log::warn!(
                    "VMCE {} local map verification refused: {error:#}",
                    self.stage
                );
                let reason = error
                    .downcast_ref::<vmce::VmceStageFileError>()
                    .map_or_else(|| error.to_string(), ToString::to_string);
                notice(
                    state,
                    &format!(
                        "VMCE: {reason}. Check the local series files, then ask the host to reselect this stage."
                    ),
                );
                if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
                    protocol.finish_return_verification(None);
                    protocol.requests.push(NetworkSetupRequest::MapStatus(0));
                }
            }
        }
        state.platform.window.request_redraw();
        Ok(())
    }

    pub(crate) fn check_guest_map(&self, state: &AppState) -> Result<LocalMapCheck> {
        let local = (|| {
            let root = maps_root(state).context("the local Maps directory is unavailable")?;
            let assets = state
                .process_assets
                .manager()
                .context("the game assets are unavailable")?;
            let visual_rules = RoomRuleSources::for_room(&state.process_assets);
            self.verify_map_from(
                &root,
                assets,
                state.process_assets.csf.as_ref(),
                visual_rules.as_ref(),
            )
        })();
        match local {
            Ok((source, selection)) => Ok(LocalMapCheck::Verified { source, selection }),
            Err(error) if error.is::<StageOptionsMismatch>() => Err(error),
            Err(error) => Ok(LocalMapCheck::Unavailable(error)),
        }
    }

    fn verify_map_from(
        &self,
        root: &Path,
        assets: &AssetManager,
        base: Option<&CsfFile>,
        visual_rules: Option<&RoomRuleSources<'_>>,
    ) -> Result<(LocalScenarioSource, LocalScenario)> {
        let key = self.selection().key;
        let stage = self.open_matching_stage(root, &key)?;
        let source = stage_source(&stage);
        let effective = checked_map_ini(stage_input(&stage), assets, &key)?;
        let digest = crate::map::source::effective_network_map_digest(&effective.ini, false, false);
        self.verify_effective_digest(&digest)?;
        let selection = room_stage_selection(&stage, base, &effective.ini, visual_rules);
        Ok((source, selection))
    }

    fn open_matching_stage(&self, root: &Path, key: &LocalScenarioKey) -> Result<OpenedVmceStage> {
        let stage = vmce::open_stage_with_identities(
            root,
            &map_id(key)?,
            Some((&self.manifest_sha256, &self.map_sha256)),
        )?;
        ensure!(
            permits_room_settings(&stage.options, &self.settings)
                && stage.primary_houses == self.roles,
            StageOptionsMismatch
        );
        Ok(stage)
    }

    fn verify_effective_digest(&self, digest: &str) -> Result<()> {
        ensure!(
            digest == self.effective_digest,
            "The effective VMCE map rules differ from the host's"
        );
        Ok(())
    }

    pub(crate) fn accept_map(
        &self,
        state: &mut AppState,
        source: LocalScenarioSource,
    ) -> Result<()> {
        super::service::accept_local_scenario_map(
            state,
            &self.selection().key,
            source,
            self.effective_digest.clone(),
        )
    }
}

/// The wire pair `[selection digest, role or null]` fits in Type 18's text
/// before the LAN session proof, even for a nested stage path.
#[derive(serde::Serialize, serde::Deserialize)]
struct RoleChoice(String, Option<u8>);

fn role_selection(key: &LocalScenarioKey) -> String {
    crate::util::sha256::sha256_hex(key.path.as_bytes())
}

pub(crate) fn receive_choice(state: &mut AppState, source: usize, text: &[u8]) -> Result<bool> {
    ensure!(
        state
            .frontend
            .network_setup_protocol
            .as_ref()
            .is_some_and(|protocol| protocol.editable()),
        "VMCE setup is locked"
    );
    accept_role_choice(&mut state.match_state.session, source, text)
}

fn accept_role_choice(session: &mut MatchSession, source: usize, text: &[u8]) -> Result<bool> {
    let map = selected_scenario(session).context("no selected VMCE map")?;
    let RoleChoice(selection, role) = serde_json::from_slice(text)?;
    ensure!(
        selection == role_selection(&map.key),
        "VMCE map selection changed"
    );
    session.accept_local_scenario_role(source, role)
}

fn local_role_choice(session: &MatchSession, row: u8, value: i32) -> Result<(usize, Option<u8>)> {
    let launch = session
        .network_setup()
        .context("the room has no accepted setup")?;
    let source = launch.local_player_index;
    ensure!(
        launch
            .humans
            .get(source)
            .and_then(|human| human.raw_slot_index)
            == Some(row),
        "you can only change your own VMCE role"
    );
    let role = (value != -1).then(|| u8::try_from(value)).transpose()?;
    session.validate_local_scenario_role(source, role)?;
    Ok((source, role))
}

fn select_role(state: &mut AppState, control: u16, value: i32) -> Result<()> {
    let view = view(state).context("no setup")?;
    ensure!(!view.locked, "VMCE setup is locked");
    let (column, _) = SetupColumn::decode(control).context("unknown setup control")?;
    ensure!(column == SetupColumn::Team, "not a role control");
    let row = view.combo(control).context("no role at this seat")?.row;
    let session = &mut state.match_state.session;
    let (source, role) = local_role_choice(session, row, value)?;
    let request = if session.host() {
        if !session.accept_local_scenario_role(source, role)? {
            return Ok(());
        }
        crate::app::lan_service::invalidate_setup_options(state)?;
        NetworkSetupRequest::PublishOptions
    } else {
        let key = &selected_scenario(session)
            .context("no selected VMCE map")?
            .key;
        let scope = state
            .match_state
            .lan_service
            .as_ref()
            .and_then(crate::app::lan_service::LanService::setup_scope)
            .context("the host has not published VMCE options")?;
        NetworkSetupRequest::AuthoredChoices {
            scope,
            text: serde_json::to_vec(&RoleChoice(role_selection(key), role))?,
        }
    };
    state
        .frontend
        .network_setup_protocol
        .as_mut()
        .context("no setup")?
        .requests
        .push(request);
    state.platform.window.request_redraw();
    Ok(())
}

fn request_start(state: &mut AppState) -> Result<()> {
    ensure!(
        can_choose(state),
        "this room needs a playing LAN host and no lobby AI"
    );
    let session = &state.match_state.session;
    checked_player_count(session)?;
    if session
        .network_setup()
        .is_some_and(|launch| launch.humans.len() > 1)
    {
        ensure!(
            peers_ready(session),
            "wait for each player to verify the map and Ready"
        );
        let scope = state
            .match_state
            .lan_service
            .as_ref()
            .and_then(crate::app::lan_service::LanService::setup_scope)
            .context("the room has no accepted options revision")?;
        crate::app::lan_service::prepare_host_timing(state)?;
        let ahead = state
            .match_state
            .session
            .network_max_ahead_frames()
            .context("the room has no accepted LAN timing")?;
        return super::actions::queue_lan_start(state, ahead, Some(scope));
    }
    let selection = selected_scenario(&state.match_state.session).context("choose a map first")?;
    let key = selection.key.clone();
    let root = maps_root(state).context("no configured Maps directory")?;
    crate::app::lan_service::prepare_local_start(state)?;
    let assets = state
        .process_assets
        .manager()
        .context("the game assets are unavailable")?;
    let startup = admit_start(&mut state.match_state.session, &root, &key, assets)?;
    let authored = startup
        .authored_scenario()
        .context("missing authored start")?;
    let player = startup
        .authored_local_player()
        .context("missing local authored player")?;
    log::debug!(
        "VMCE froze {} in House {} for {} with manifest sha256 {}, seed {} and shared Houses {:?}",
        player.role,
        player.house,
        authored.selection.key.path,
        authored.configuration_sha256,
        startup.seed.value,
        player.shared_control_houses
    );
    crate::app::App::leave_shell_dialog(
        state,
        super::shell_transition::ShellExitThen::PreparedNetworkStart {
            dialog: super::dialog(state),
            startup,
        },
    );
    Ok(())
}

fn admit_start(
    session: &mut MatchSession,
    root: &Path,
    key: &LocalScenarioKey,
    assets: &AssetManager,
) -> Result<Arc<PreparedNetworkStartup>> {
    ensure!(
        can_choose_in_room(session),
        "this room needs a playing LAN host and no lobby AI"
    );
    let stage = vmce::open_stage(root, &map_id(key)?)?;
    let start = stage_start(session, key.clone(), stage, assets)?;
    ensure!(
        start.players.len() == 1,
        "network start needs accepted options and map status from every player"
    );
    start.freeze(session)
}

struct StageStart {
    key: LocalScenarioKey,
    source: LocalScenarioSource,
    effective_digest: String,
    players: Vec<LocalScenarioPlayer>,
    observers: Vec<LocalScenarioObserver>,
}

impl StageStart {
    fn freeze(self, session: &mut MatchSession) -> Result<Arc<PreparedNetworkStartup>> {
        session.freeze_authored_network_start(
            &self.key,
            self.source,
            self.effective_digest,
            self.players,
            self.observers,
        )
    }
}

fn stage_start(
    session: &MatchSession,
    key: LocalScenarioKey,
    stage: OpenedVmceStage,
    assets: &AssetManager,
) -> Result<StageStart> {
    let players = resolve_start_players(session, &stage)?;
    let observers = resolve_observer_houses(session, &stage)?;
    let source = stage_source(&stage);
    session.verify_local_scenario_source(&key, &source)?;
    let settings = room_settings(
        session
            .local_scenario_lobby()
            .context("no admitted VMCE setup")?,
    );
    ensure!(
        permits_room_settings(&stage.options, &settings),
        StageOptionsMismatch
    );
    let effective_digest = checked_map_digest(stage.map, assets, &key)?;
    Ok(StageStart {
        key,
        source,
        effective_digest,
        players,
        observers,
    })
}

fn peers_ready(session: &MatchSession) -> bool {
    session.network_setup().is_some_and(|launch| {
        (0..launch.humans.len()).all(|source| {
            session.network_map_status(source) == Some(1)
                && (source == launch.local_player_index || session.network_player_ready(source))
        })
    })
}

impl ScenarioOptions {
    fn verify_start_session(&self, session: &MatchSession) -> Result<()> {
        ensure!(
            selected_scenario(session).is_some_and(|selected| selected.key == self.selection().key)
                && session.network_start_seed() == Some(self.seed),
            "the selected VMCE stage or seed changed since options were accepted"
        );
        ensure!(
            session
                .local_scenario_lobby()
                .is_some_and(|lobby| room_settings(lobby) == self.settings),
            "the VMCE settings changed since options were accepted"
        );
        let seats = scenario_seats(session)?;
        ensure!(
            seats.len() == self.seats.len()
                && seats.iter().zip(&self.seats).all(|(local, agreed)| {
                    local.row == agreed.row
                        && local.name.eq_ignore_ascii_case(&agreed.name)
                        && local.role == agreed.role
                        && local.observer == agreed.observer
                }),
            "the VMCE roster or role choices changed since options were accepted"
        );
        let local = session
            .network_setup()
            .context("no accepted VMCE setup")?
            .local_player_index;
        ensure!(
            if session.host() {
                peers_ready(session)
            } else {
                session.network_map_status(local) == Some(1) && session.network_player_ready(local)
            },
            "the current VMCE setup has not been verified and readied"
        );
        Ok(())
    }
}

fn revalidate_lan_start(state: &mut AppState, scope: SessionScope) -> Result<StageStart> {
    let accepted = crate::app::lan_service::authored_start_options(state, scope)?;
    let session = &state.match_state.session;
    accepted.verify_start_session(session)?;
    let key = accepted.selection().key;
    let root = maps_root(state).context("no configured Maps directory")?;
    let assets = state
        .process_assets
        .manager()
        .context("the game assets are unavailable")?;
    let stage = accepted.open_matching_stage(&root, &key)?;
    let start = stage_start(session, key, stage, assets)?;
    accepted.verify_effective_digest(&start.effective_digest)?;
    ensure!(
        session.network_local_map_effective_digest() == Some(start.effective_digest.as_str()),
        "the VMCE map is no longer verified for this revision"
    );
    crate::app::lan_service::verify_setup_agreement(state)?;
    Ok(start)
}

pub(crate) fn verify_lan_start(state: &mut AppState, scope: SessionScope) -> Result<()> {
    revalidate_lan_start(state, scope).map(|_| ())
}

pub(super) fn freeze_lan_start(
    state: &mut AppState,
    scope: SessionScope,
) -> Result<Arc<PreparedNetworkStartup>> {
    revalidate_lan_start(state, scope)?.freeze(&mut state.match_state.session)
}

pub(crate) fn refuse_lan_start(state: &mut AppState, error: &anyhow::Error) -> Result<()> {
    log::warn!("VMCE start refused before GO: {error:#}");
    state
        .frontend
        .network_setup_protocol
        .as_mut()
        .context("no LAN setup")?
        .cancel_lan_go();
    crate::app::lan_service::invalidate_setup_options(state)?;
    notice(
        state,
        &format!(
            "VMCE could not start. {error:#}. Restore the series files and choose the map again."
        ),
    );
    Ok(())
}

pub(super) fn refuse_lan_handoff(state: &mut AppState, error: &anyhow::Error) {
    super::service::fail_go_handoff_with_message(
        state,
        format!(
            "VMCE rejected the start. {error:#}. Check the local series files before returning to the room."
        ),
    );
}

fn resolve_start_players(
    session: &MatchSession,
    stage: &OpenedVmceStage,
) -> Result<Vec<LocalScenarioPlayer>> {
    let player_count = checked_player_count(session)?;
    let launch = session
        .network_setup()
        .context("the room has no accepted setup")?;
    ensure!(launch.opponents.is_empty(), "VMCE does not use lobby AI");
    let seed = session
        .network_start_seed()
        .context("VMCE start has no admitted LAN seed")?;
    let mut picks = launch
        .humans
        .iter()
        .enumerate()
        .filter(|(_, human)| !human.is_observer())
        .map(|(source, human)| {
            let row = human
                .raw_slot_index
                .filter(|row| usize::from(*row) < stage.options.max_players)
                .with_context(|| format!("{} has no accepted VMCE lobby seat", human.name))?;
            Ok((row, session.local_scenario_role(source).map(usize::from)))
        })
        .collect::<Result<Vec<_>>>()?;
    // Each process can keep its own node first. The host's lobby rows give
    // both sides the same assignment order while preserving source indices.
    picks.sort_unstable_by_key(|(row, _)| *row);

    let mut remaining = (0..stage.primary_houses.len()).collect::<Vec<_>>();
    for &(_, pick) in &picks {
        if let Some(role) = pick {
            let available = remaining
                .iter()
                .position(|&candidate| candidate == role)
                .context("an explicit VMCE role is missing or already occupied")?;
            remaining.remove(available);
        }
    }
    if player_count > 1 {
        // The VMCE multiplayer shuffle uses a private xorshift32 stream. A
        // solo Random pick keeps the first authored role as before.
        let mut random = if seed == 0 { 0x9E37_79B9 } else { seed };
        for index in (1..remaining.len()).rev() {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            remaining.swap(index, random as usize % (index + 1));
        }
    }
    let mut available = remaining.into_iter();
    picks
        .into_iter()
        .map(|(raw_slot_index, pick)| {
            let role = pick
                .or_else(|| available.next())
                .context("no VMCE role is available for the lobby seat")?;
            Ok(LocalScenarioPlayer {
                raw_slot_index,
                role: format!("Player{}", role + 1),
                house: stage
                    .primary_houses
                    .get(role)
                    .context("unknown authored role")?
                    .clone(),
                shared_control_houses: stage
                    .shared_control_houses
                    .get(role)
                    .context("missing authored role grants")?
                    .clone(),
            })
        })
        .collect()
}

fn resolve_observer_houses(
    session: &MatchSession,
    stage: &OpenedVmceStage,
) -> Result<Vec<LocalScenarioObserver>> {
    let launch = session.network_setup().context("no admitted VMCE room")?;
    let lobby = session
        .local_scenario_lobby()
        .context("no admitted VMCE stage")?;
    let seats = launch.humans.iter().filter(|human| human.is_observer());
    let mut rows = seats
        .map(|human| {
            human
                .raw_slot_index
                .filter(|&row| lobby.admits_row(row, true))
        })
        .collect::<Option<Vec<_>>>()
        .context("an observer has no admitted VMCE seat")?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    ensure!(
        stage.house_names.len() + rows.len() <= i8::MAX as usize + 1,
        "the VMCE map has no room in its House array for the observers"
    );
    rows.sort_unstable();
    let mut used = stage
        .house_names
        .iter()
        .map(|name| name.to_ascii_uppercase())
        .collect::<std::collections::HashSet<_>>();
    rows.into_iter()
        .map(|raw_slot_index| {
            let house = (0..=stage.house_names.len())
                .map(|suffix| format!("Observer{raw_slot_index}_{suffix}"))
                .find(|name| used.insert(name.to_ascii_uppercase()))
                .context("could not name the observer House")?;
            Ok(LocalScenarioObserver {
                raw_slot_index,
                house,
            })
        })
        .collect()
}

fn checked_player_count(session: &MatchSession) -> Result<usize> {
    let lobby = session
        .local_scenario_lobby()
        .context("choose a map first")?;
    let launch = session
        .network_setup()
        .context("the room has no accepted setup")?;
    let count = launch
        .humans
        .iter()
        .filter(|human| !human.is_observer())
        .count();
    let allowed = lobby.player_count_range();
    ensure!(
        count >= *allowed.start(),
        "the selected stage requires at least {} players and the room has {count}",
        allowed.start()
    );
    ensure!(
        count <= *allowed.end(),
        "the selected stage allows at most {} players and the room has {count}",
        allowed.end()
    );
    Ok(count)
}

pub(super) fn automatic_guest_ready(state: &mut AppState) -> Option<bool> {
    let selected = selected_scenario(&state.match_state.session);
    let protocol = state.frontend.network_setup_protocol.as_mut()?;
    if selected.is_none() {
        // Returning to stock play ends this stage's acceptance even if the
        // same local stage is selected again later in the room.
        protocol.scenario_ready_intent = None;
    }
    selected.map(|selected| protocol.scenario_ready_intent.as_ref() == Some(&selected.key))
}

fn guest_ready_action(state: &mut AppState) -> Result<()> {
    let session = &state.match_state.session;
    let key = selected_scenario(session)
        .context("no local stage")?
        .key
        .clone();
    ensure!(!session.host(), "the host starts the room");
    let protocol = state
        .frontend
        .network_setup_protocol
        .as_mut()
        .context("no setup")?;
    ensure!(protocol.editable(), "VMCE setup is locked");
    if protocol.scenario_ready_intent.as_ref() == Some(&key) {
        protocol.scenario_ready_intent = None;
        super::service::invalidate_local_acceptance(state)?;
    } else {
        super::actions::accept(state)?;
        if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
            protocol.scenario_ready_intent = Some(key);
        }
    }
    state.platform.window.request_redraw();
    Ok(())
}

pub(super) fn view(state: &AppState) -> Option<SetupView> {
    let protocol = state.frontend.network_setup_protocol.as_ref()?;
    let locked = protocol.locked;
    let session = &state.match_state.session;
    let lobby = session.local_scenario_lobby()?;
    let selection = selected_scenario(session)?;
    let dialog = super::dialog(state);
    let listing = super::view::room_listing(state)?;
    let mut view = project_view(
        session,
        dialog,
        locked,
        listing.first_row..listing.first_row + listing.page_len,
        &crate::app::App::csf_label(state, "GUI:Random", "Random"),
        &crate::app::App::csf_label(state, "GUI:Observer", "Observer"),
    )?;
    view.pages = listing.pages;
    view.roster_scroll = listing.roster_scroll;
    let waiting = crate::app::shell_lan::waiting_for_room(state);
    let cancel_ready = protocol.scenario_ready_intent.as_ref() == Some(&selection.key);
    view.accept_enabled =
        !waiting && !session.host() && (cancel_ready || super::view::accept_enabled(state));
    view.start_requested = protocol.start_requested;
    view.open_box = state
        .frontend
        .multiplayer_setup
        .as_ref()
        .and_then(|ui| ui.open_box)
        .filter(|open| !matches!(open, RoomBox::GameOptions));
    let role_ui: Vec<_> = selection
        .presentation
        .as_ref()
        .map_or_else(Vec::new, |presentation| {
            presentation
                .role_appearances
                .iter()
                .map(|role| role_appearance(state, role))
                .collect()
        });
    for combo in &mut view.combos {
        if waiting {
            combo.enabled = false;
        }
        combo.list_w = roster_list_width(&combo.choices, |text| {
            state.renderer.bit_font.text_width(text)
        });
    }
    let (key, fallback) = match lobby.rules.ai_difficulty {
        HouseDifficulty::Easy => ("TXT_Easy", "Easy"),
        HouseDifficulty::Normal => ("TXT_Normal", "Normal"),
        HouseDifficulty::Hard => ("TXT_Hard", "Hard"),
    };
    let difficulty = crate::app::App::csf_label(state, key, fallback);
    view.difficulty_label = Some(crate::assets::csf_file::format_csf(
        &crate::app::App::csf_label(state, "GUI:DifficultyString", "Difficulty: %s"),
        &[crate::assets::csf_file::CsfArg::Str(&difficulty)],
    ));
    view.scenario_room = Some(room_presentation(
        state,
        selection,
        lobby,
        &role_ui,
        &view,
        waiting,
        cancel_ready,
    )?);
    if let Some(display) = selection.presentation.as_ref() {
        view.mode_label.clone_from(&display.series_title);
        view.map_label.clone_from(&display.stage_title);
    } else {
        view.mode_label = crate::i18n::tr("VERA:VmceModeName");
    }
    view.preview_hidden = waiting;
    Some(view)
}

fn role_appearance(
    state: &AppState,
    appearance: &LocalScenarioRoleAppearance,
) -> LocalScenarioRoleVisual {
    appearance.resolved_visual.clone().unwrap_or_else(|| {
        state
            .rules()
            .map_or_else(LocalScenarioRoleVisual::default, |rules| {
                resolve_role_visual(rules, appearance)
            })
    })
}

fn resolve_role_visual(
    rules: &crate::rules::ruleset::RuleSet,
    appearance: &LocalScenarioRoleAppearance,
) -> LocalScenarioRoleVisual {
    let id = rules
        .scenario_country_name(appearance.country.as_deref())
        .or(appearance.country.as_deref());
    let flag = id.and_then(|country| {
        rules.country_flag_surface(country).or_else(|| {
            let index = rules.trigger_house_type_index(country)?;
            let item = crate::app::frontend::skirmish_shell_render::side_item_data_for_country(
                crate::skirmish_launch::LaunchCountry::from_country_index(u32::from(index.0)),
            );
            crate::app::frontend::skirmish_shell_render::flag_pcx_for_side_item_data(item)
                .map(str::to_owned)
        })
    });
    let scheme = appearance
        .color
        .as_deref()
        .and_then(|key| crate::rules::color_scheme::scheme_entry_by_name(&rules.color_schemes, key))
        .or_else(|| {
            usize::try_from(rules.country_color_scheme(id?))
                .ok()
                .map(|raw| raw / 2)
        });
    let color = scheme.and_then(|index| {
        rules.color_schemes.get(index)?;
        Some(crate::rules::house_colors::house_text_rgb(
            &rules.color_schemes,
            crate::rules::house_colors::HouseColorIndex(u8::try_from(index).ok()?),
        ))
    });
    LocalScenarioRoleVisual { flag, rgb: color }
}

fn room_presentation(
    state: &AppState,
    selection: &LocalScenario,
    lobby: &LocalScenarioLobby,
    role_ui: &[LocalScenarioRoleVisual],
    view: &SetupView,
    waiting: bool,
    cancel_ready: bool,
) -> Option<ScenarioRoomView> {
    let session = &state.match_state.session;
    let launch = session.network_setup()?;
    let stage = selection.presentation.as_ref();
    let tr = crate::i18n::tr;
    let rows = (view.first_row..view.first_row + view.page_len)
        .map(|row| {
            let Some((index, player)) = launch
                .humans
                .iter()
                .enumerate()
                .find(|(_, human)| human.raw_slot_index.map(usize::from) == Some(row))
            else {
                return ScenarioRoomRow {
                    name: tr("VERA:VmceOpenSeat"),
                    status: String::new(),
                    flag: None,
                    color: None,
                    details: None,
                };
            };
            let role = session
                .local_scenario_role(index)
                .map(usize::from)
                .or_else(|| (!player.is_observer() && lobby.roles.len() == 1).then_some(0));
            let appearance = role.and_then(|index| role_ui.get(index));
            let details = role.and_then(|index| {
                let role = stage?.role_appearances.get(index)?;
                Some(role_details(
                    selection,
                    index,
                    role,
                    lobby.rules.options.tech_level,
                ))
            });
            let status = if row == 0 {
                tr("VERA:VmceHostStatus")
            } else if player.is_observer() {
                crate::app::App::csf_label(state, "GUI:Observer", "Observer")
            } else if waiting {
                String::new()
            } else if session.network_player_ready(index) {
                tr("VERA:VmceReadyStatus")
            } else if session.network_map_status(index) == Some(1) {
                tr("VERA:VmceNotReadyStatus")
            } else if index == launch.local_player_index && selection.presentation.is_none() {
                tr("VERA:VmceMapUnavailable")
            } else {
                tr("VERA:VmceVerifyStatus")
            };
            ScenarioRoomRow {
                name: player.name.clone(),
                status,
                flag: appearance.and_then(|appearance| appearance.flag.clone()),
                color: appearance.and_then(|appearance| appearance.rgb),
                details,
            }
        })
        .collect();
    let mut rules = vec![
        tr(if lobby.rules.options.ally_change_allowed {
            "VERA:VmceAlliesAllowed"
        } else {
            "VERA:VmceAlliesLocked"
        }),
        tr("VERA:VmceVictoryTriggers"),
    ];
    if lobby.rules.difficulty_locked {
        rules.push(tr("VERA:VmceMapLocksDifficulty"));
    }
    Some(ScenarioRoomView {
        rows,
        caption: "VERA:VmceModeName",
        players_heading: "VERA:VmceRoomPlayers",
        chat_heading: "VERA:VmceRoomChat",
        rules_heading: "VERA:VmceRoomRules",
        rules,
        waiting: waiting.then(|| tr("VERA:VmceWaitHostReturn")),
        unavailable_preview: "VERA:VmceNoPreview",
        briefing_label: "VERA:VmceBriefing",
        briefing: stage
            .and_then(|stage| stage.briefing.clone())
            .unwrap_or_else(|| tr("VERA:VmceNoBriefing")),
        details_label: "VERA:VmceRoleDetails",
        accept_label: if cancel_ready {
            "VERA:VmceCancelReady"
        } else {
            "VERA:VmceReady"
        },
        choose_label: "VERA:VmceChooseStage",
        leave_label: "VERA:VmceLeaveRoom",
        presentation: selection.presentation.clone(),
        role_colors: role_ui.iter().map(|role| role.rgb).collect(),
        description_page: state
            .frontend
            .multiplayer_setup
            .as_ref()
            .map_or(0, |ui| ui.description_page),
        description_pages: state
            .frontend
            .multiplayer_setup
            .as_ref()
            .map_or(0, |ui| ui.description_pages),
    })
}

fn role_details(
    selection: &LocalScenario,
    index: usize,
    role: &LocalScenarioRoleAppearance,
    default_tech: i32,
) -> ScenarioRoleDetails {
    let opening = role.opening;
    let tech = opening.tech_level.unwrap_or(default_tech);
    let metric =
        |key, value: &dyn std::fmt::Display| crate::i18n::tr_fill(key, &[("value", value)]);
    let mut lines = vec![
        metric("VERA:VmceStartingFunds", &opening.funds),
        metric("VERA:VmceTechLevel", &tech),
        metric("VERA:VmcePlacedUnits", &opening.units),
        metric("VERA:VmcePlacedBuildings", &opening.buildings),
        String::new(),
    ];
    if role.shared_forces.is_empty() {
        lines.push(crate::i18n::tr("VERA:VmceNoSharedControl"));
    } else {
        lines.push(crate::i18n::tr("VERA:VmceSharedForces"));
        lines.extend(role.shared_forces.iter().enumerate().map(|(index, title)| {
            let name = title.clone().unwrap_or_else(|| {
                crate::i18n::tr_fill("VERA:VmceSharedForceNumber", &[("number", &(index + 1))])
            });
            format!("• {name}")
        }));
    }
    ScenarioRoleDetails {
        title: role_title(selection, &format!("Player{}", index + 1), None),
        body: lines.join("\n"),
    }
}

fn player_range(min: usize, max: usize) -> String {
    let count = if min == max {
        min.to_string()
    } else {
        format!("{min}–{max}")
    };
    crate::i18n::tr_fill("VERA:VmceStagePlayers", &[("count", &count)])
}

fn project_view(
    session: &MatchSession,
    dialog: SetupDialog,
    locked: bool,
    rows: Range<usize>,
    random_label: &str,
    observer_label: &str,
) -> Option<SetupView> {
    let map = selected_scenario(session)?;
    let launch = session.network_setup()?;
    let lobby = session.local_scenario_lobby()?;
    let mut view = SetupView::local_scenario(
        dialog,
        locked,
        session.local_scenario_row_count()?,
        map.mode_label,
        &map.title,
    );
    view.first_row = rows.start;
    view.page_len = rows.len();
    view.choose_map_enabled = can_choose_in_room(session);
    view.start_enabled = view.choose_map_enabled
        && checked_player_count(session).is_ok()
        && (launch.humans.len() == 1 || peers_ready(session));
    view.team_header = true;
    let rules = &lobby.rules;
    view.trackbars.push(SetupTrackbar {
        id: DIFFICULTY,
        value: 2 - rules.ai_difficulty.table_index() as i32,
        range: TrackbarRange {
            min: 0,
            max: 2,
            step: 1,
        },
        enabled: session.host() && !rules.difficulty_locked,
        visible: true,
    });
    let choices: Vec<_> = std::iter::once(SetupChoice {
        value: -1,
        text: random_label.to_owned(),
        swatch: None,
    })
    .chain(lobby.roles.iter().enumerate().map(|(role, _)| SetupChoice {
        value: role as i32,
        text: role_title(map, &format!("Player{}", role + 1), None),
        swatch: None,
    }))
    .collect();
    for (row, raw) in rows.enumerate() {
        let raw = raw as u8;
        let Some((source, player)) = launch
            .humans
            .iter()
            .enumerate()
            .find(|(_, human)| human.raw_slot_index == Some(raw))
        else {
            continue;
        };
        let own = session.local_scenario_role(source).map_or(-1, i32::from);
        let role_choices: Vec<_> = if player.is_observer() {
            vec![SetupChoice {
                value: -1,
                text: observer_label.to_owned(),
                swatch: None,
            }]
        } else {
            choices
                .iter()
                .filter(|choice| {
                    choice.value == own
                        || choice.value < 0
                        || !launch.humans.iter().enumerate().any(|(index, _)| {
                            index != source
                                && session.local_scenario_role(index).map(i32::from)
                                    == Some(choice.value)
                        })
                })
                .cloned()
                .collect()
        };
        let displayed = if !player.is_observer() && choices.len() == 2 {
            0
        } else {
            own
        };
        view.combos.extend([
            SetupCombo {
                id: SetupColumn::Player.control(row),
                row: raw,
                enabled: false,
                visible: true,
                selected: Some(0),
                choices: vec![SetupChoice {
                    value: 0,
                    text: player.name.clone(),
                    swatch: None,
                }],
                list_w: 0,
            },
            SetupCombo {
                id: SetupColumn::Team.control(row),
                row: raw,
                enabled: !player.is_observer()
                    && source == launch.local_player_index
                    && choices.len() > 2
                    && role_choices.len() > 1,
                visible: true,
                selected: role_choices
                    .iter()
                    .position(|choice| choice.value == displayed),
                choices: role_choices,
                list_w: 0,
            },
        ]);
        view.row_icons[row] = Some(SetupRowIcon {
            country: None,
            host: session.host() && source == launch.local_player_index,
            accepted: session.network_player_ready(source),
            badge: None,
            local: source == launch.local_player_index,
        });
    }
    Some(view)
}

fn edit_room_rules(session: &mut MatchSession, action: &SetupAction) -> Result<bool> {
    ensure!(session.host(), "only the host may edit VMCE settings");
    let mut rules = session
        .local_scenario_lobby()
        .context("no editable VMCE setup")?
        .rules
        .clone();
    match *action {
        SetupAction::Trackbar {
            control: DIFFICULTY,
            value,
        } => {
            ensure!(!rules.difficulty_locked, "this stage locks its difficulty");
            rules.ai_difficulty = ai_difficulty(value)?;
        }
        _ => return Ok(false),
    }
    session.set_local_scenario_rules(rules)
}

fn handle_room_edit(state: &mut AppState, action: &SetupAction) -> Result<()> {
    ensure!(can_choose(state), "VMCE setup is locked");
    if edit_room_rules(&mut state.match_state.session, action)? {
        crate::app::lan_service::invalidate_setup_options(state)?;
        state
            .frontend
            .network_setup_protocol
            .as_mut()
            .context("no setup")?
            .requests
            .push(NetworkSetupRequest::PublishOptions);
    }
    Ok(())
}

pub(super) fn handle_action(state: &mut AppState, action: &SetupAction) -> bool {
    if selected_scenario(&state.match_state.session).is_none() {
        return false;
    }
    match action {
        SetupAction::Start => {
            if let Err(error) = request_start(state) {
                log::warn!("VMCE start refused: {error:#}");
                notice(
                    state,
                    &format!(
                        "VMCE could not start. {error:#}. Check the series settings and choose the map again."
                    ),
                );
            }
            true
        }
        SetupAction::ChooseMap => !can_choose(state),
        SetupAction::Accept => {
            if let Err(error) = guest_ready_action(state) {
                notice(state, &format!("VMCE preparation refused. {error:#}"));
            }
            true
        }
        SetupAction::Select { control, value } => {
            if let Err(error) = select_role(state, *control, *value) {
                notice(state, &format!("VMCE role choice declined. {error:#}"));
            }
            true
        }
        SetupAction::Trackbar {
            control: DIFFICULTY,
            ..
        } => {
            if let Err(error) = handle_room_edit(state, action) {
                notice(state, &format!("VMCE setting declined. {error:#}"));
            }
            true
        }
        SetupAction::Back
        | SetupAction::Chat(_)
        | SetupAction::OpenBox(_)
        | SetupAction::RosterPage { .. } => false,
        _ => true,
    }
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use crate::map::skirmish_scenarios::{SkirmishScenarioRecord, SkirmishScenarioSource};
    use crate::map::source::test_support::TestDirectory;
    use crate::skirmish_launch::{SkirmishLaunchMode, SkirmishLaunchSession};
    use crate::ui::multiplayer_setup::{CHOOSE_MAP, CRATES, SHORT_GAME, START, SetupFlavor};
    use crate::ui::skirmish_shell::{
        ChooseMapContext, ChooseMapModalState, compute_choose_map_modal_layout,
    };

    const STAGE: &str = "minimal_stage01.map";
    const SERIES: &str = "VMCE/Fixture";
    const MANIFEST: &str = include_str!("../../../tests/fixtures/vmce/campaign.ini");
    const MAP: &str = include_str!("../../../tests/fixtures/vmce/minimal_stage01.map");
    const LAN_STAGE: &str = "shared_move_stage02.map";
    const LAN_MANIFEST: &str =
        include_str!("../../../tests/fixtures/vmce/shared_move/campaign.ini");
    const LAN_MAP: &str =
        include_str!("../../../tests/fixtures/vmce/shared_move/shared_move_stage02.map");

    fn fixture_directory(stage: &str, manifest: &str, map: &str) -> TestDirectory {
        let root = TestDirectory::new("vmce-room");
        std::fs::create_dir_all(root.path().join(SERIES)).unwrap();
        for (file, contents) in [("campaign.ini", manifest), (stage, map)] {
            root.write(&format!("{SERIES}/{file}"), contents.as_bytes());
        }
        root
    }

    fn installed_lan_stage(root: &TestDirectory) -> LocalScenarioKey {
        let stage_path = format!("Fixture/{LAN_STAGE}");
        discover(root.path(), None)
            .into_iter()
            .find(|entry| entry.selection.key.path == stage_path)
            .expect("installed LAN fixture stage")
            .selection
            .key
    }

    fn host_room(stock: &str) -> MatchSession {
        let profile: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.command-bar.example.json",
        ))
        .unwrap();
        let mut launch: SkirmishLaunchSession =
            serde_json::from_value(profile["launch"].clone()).unwrap();
        launch.humans.truncate(1);
        launch.local_player_index = 0;
        launch.opponents.clear();
        launch.ai_player_count = 0;
        launch.selected_map_file = Some(stock.to_owned());
        let mut session = MatchSession::default();
        session.enter_network(NetworkReturnContext::LanLobby);
        session
            .accept_network_setup(launch, true, None, true)
            .unwrap();
        session.set_network_start_seed(42).unwrap();
        session
    }

    #[test]
    fn publishes_exact_second_stage_settings_from_the_hosts_selected_source() {
        use crate::util::sha256::sha256_hex;

        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let manifest = LAN_MANIFEST.replace(
            "[Campaign]",
            "[Campaign]\nLockOptions=no\nShortGame=yes\nCrates=yes\nVictoryMode=Elimination\nDifficulty=2",
        );
        let manifest =
            manifest.replace("Player2=HouseCharlie", "Player2=HouseCharlie\nDifficulty=0");
        let root = fixture_directory(LAN_STAGE, &manifest, LAN_MAP);
        let key = installed_lan_stage(&root);
        let mut session = host_room("stock.map");
        accept_selection(&mut session, root.path(), &key, None, None).unwrap();
        let (options, _) = options_from_selection(&session, root.path(), &key, &assets).unwrap();
        let wire = serde_json::to_vec(&options).unwrap();
        let wire_settings =
            &serde_json::from_slice::<serde_json::Value>(&wire).unwrap()["settings"];
        assert_eq!(wire_settings["lobby_difficulty"], 0);
        for retired in ["lock_options", "short_game", "crates", "victory_mode"] {
            assert!(wire_settings.get(retired).is_none(), "{retired}");
        }
        let options: ScenarioOptions = serde_json::from_slice(&wire).unwrap();
        options.validate().unwrap();
        assert_eq!(options.mode, MODE_KEY);
        assert_eq!(
            (options.series.as_str(), options.stage.as_str()),
            ("Fixture", LAN_STAGE)
        );
        assert_eq!(options.manifest_sha256, sha256_hex(manifest.as_bytes()));
        assert_eq!(options.map_sha256, sha256_hex(LAN_MAP.as_bytes()));
        assert_eq!(options.seed, 42);
        assert_eq!(
            (options.settings.min_players, options.settings.max_players),
            (2, 2)
        );
        assert_eq!(options.roles, ["HouseAlpha", "HouseCharlie"]);
        assert!(!edit_room_rules(&mut session, &SetupAction::Toggle { control: CRATES }).unwrap());
        let difficulty = SetupAction::Trackbar {
            control: DIFFICULTY,
            value: 2,
        };
        assert!(edit_room_rules(&mut session, &difficulty).unwrap());
        assert!(!edit_room_rules(&mut session, &difficulty).unwrap());
        let (edited, _) = options_from_selection(&session, root.path(), &key, &assets).unwrap();
        assert_eq!(edited.settings.lobby_difficulty, 2);
        edited
            .verify_map_from(root.path(), &assets, None, None)
            .unwrap();

        let changed = format!("{LAN_MAP}\n; unaccepted file edit\n");
        root.write(&format!("{SERIES}/{LAN_STAGE}"), changed.as_bytes());
        assert!(options_from_selection(&session, root.path(), &key, &assets).is_err());
        accept_selection(&mut session, root.path(), &key, None, None).unwrap();
        assert_eq!(
            options_from_selection(&session, root.path(), &key, &assets)
                .unwrap()
                .0
                .map_sha256,
            sha256_hex(changed.as_bytes())
        );
    }

    #[test]
    fn guest_checks_its_exact_stage_files_and_effective_map_before_admission() {
        use vmce::VmceStageFileError as FileError;

        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let host_files = fixture_directory(LAN_STAGE, LAN_MANIFEST, LAN_MAP);
        let guest_files = TestDirectory::new("vmce-guest-stage");
        std::fs::create_dir_all(guest_files.path().join(SERIES)).unwrap();
        let manifest = format!("{SERIES}/campaign.ini");
        let map = format!("{SERIES}/{LAN_STAGE}");
        let key = installed_lan_stage(&host_files);
        let mut host = host_room("stock.map");
        accept_selection(&mut host, host_files.path(), &key, None, None).unwrap();
        let (options, _) = options_from_selection(&host, host_files.path(), &key, &assets).unwrap();
        let verify = || options.verify_map_from(guest_files.path(), &assets, None, None);
        let check_error = |expected| {
            let error = verify().unwrap_err();
            assert!(
                error.downcast_ref::<FileError>().is_some_and(|actual| {
                    std::mem::discriminant(actual) == std::mem::discriminant(&expected)
                }),
                "{error:#}"
            );
        };
        check_error(FileError::MissingManifest);
        guest_files.write(&manifest, b"invalid changed manifest");
        check_error(FileError::ManifestMismatch);
        guest_files.write(&manifest, LAN_MANIFEST.as_bytes());
        check_error(FileError::MissingMap);
        guest_files.write(&map, b"invalid changed map");
        check_error(FileError::MapMismatch);
        guest_files.write(&map, LAN_MAP.as_bytes());
        let (verified, _) = verify().unwrap();
        assert!(
            matches!(&verified.map, crate::map::source::LoadedMapSource::Loose { path, .. }
            if path.starts_with(guest_files.path().canonicalize().unwrap()))
        );

        let mut different_rules = options.clone();
        different_rules.effective_digest = "0".repeat(64);
        assert!(
            different_rules
                .verify_map_from(guest_files.path(), &assets, None, None)
                .unwrap_err()
                .to_string()
                .contains("effective VMCE map rules differ")
        );
        let mut conflicting_options = options.clone();
        conflicting_options.settings.auto_enemy_selection =
            !conflicting_options.settings.auto_enemy_selection;
        assert!(
            conflicting_options
                .verify_map_from(guest_files.path(), &assets, None, None)
                .unwrap_err()
                .to_string()
                .contains("Host options disagree")
        );
        let mut different_lock = options;
        different_lock.settings.lock_difficulty = !different_lock.settings.lock_difficulty;
        assert!(
            different_lock
                .verify_map_from(guest_files.path(), &assets, None, None)
                .unwrap_err()
                .downcast_ref::<StageOptionsMismatch>()
                .is_some()
        );
    }

    #[test]
    fn lan_role_choices_belong_to_the_admitted_sender_and_cannot_duplicate() {
        use crate::net::lan_messages::{LanMessage, LanSessionProof, SessionScope};

        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let root = fixture_directory(LAN_STAGE, LAN_MANIFEST, LAN_MAP);
        let key = installed_lan_stage(&root);
        let mut host = host_room("stock.map");
        accept_selection(&mut host, root.path(), &key, None, None).unwrap();
        assert!(host.network_allows_peers());
        assert_eq!(host.network_join_row(false), Some((1, false)));
        assert!(host.network_join_row(true).is_none());
        let mut remote = host.network_setup().unwrap().local_player().clone();
        remote.raw_slot_index = Some(1);
        remote.name = "Guest".to_owned();
        let guest_source = host.add_network_participant(remote).unwrap();
        assert!(host.network_join_row(false).is_none());
        host.accept_local_scenario_role(0, Some(0)).unwrap();

        // Keep the guest's existing admitted roster, with its own source at
        // index 0. The host snapshot identifies the picks by admitted name.
        let mut guest_launch = host.network_setup().unwrap().clone();
        guest_launch.humans.swap(0, 1);
        let mut guest = MatchSession::default();
        guest.enter_network(NetworkReturnContext::LanLobby);
        guest
            .accept_network_setup(guest_launch, false, None, true)
            .unwrap();
        options_from_selection(&host, root.path(), &key, &assets)
            .unwrap()
            .0
            .install(&mut guest, None)
            .unwrap();
        let view = project_view(
            &guest,
            SetupDialog {
                host: false,
                flavor: SetupFlavor::Lan,
                modern: true,
            },
            false,
            0..2,
            "Random",
            "Observer",
        )
        .unwrap();
        assert!(!view.enabled(SetupColumn::Team.control(0)));
        assert!(view.enabled(SetupColumn::Team.control(1)));
        assert!(local_role_choice(&guest, 0, 1).is_err());
        assert!(local_role_choice(&guest, 1, 0).is_err());
        assert!(local_role_choice(&guest, 1, 2).is_err());

        let proof = LanSessionProof {
            scope: SessionScope {
                epoch: [1; 16],
                revision: 1,
            },
            join_nonce: [2; 16],
            admission_nonce: [3; 16],
            version: "test".into(),
            rules_digest: String::new(),
        };
        let receive_choice = |host: &mut MatchSession, role| {
            let text = serde_json::to_vec(&RoleChoice(role_selection(&key), role)).unwrap();
            let sent = LanMessage::player_choices("Guest", &text)
                .unwrap()
                .with_session_proof(&proof)
                .unwrap();
            let received = LanMessage::decode(sent.as_bytes()).unwrap();
            assert_eq!(received.session_proof().unwrap(), Some(proof.clone()));
            let payload = received.player_choices_text().unwrap();
            assert!(payload == text, "scoped LAN packet changed the role choice");
            accept_role_choice(host, guest_source, payload)
        };
        for role in [0, 2] {
            assert!(receive_choice(&mut host, Some(role)).is_err());
            assert_eq!(host.local_scenario_role(guest_source), None);
        }
        assert!(receive_choice(&mut host, Some(1)).unwrap());
        assert_eq!(host.local_scenario_role(0), Some(0));
        let (options, _) = options_from_selection(&host, root.path(), &key, &assets).unwrap();
        options.install(&mut guest, None).unwrap();
        assert_eq!(guest.local_scenario_role(0), Some(1));
        assert_eq!(guest.local_scenario_role(1), Some(0));
        assert!(receive_choice(&mut host, None).unwrap());
        assert_eq!(host.local_scenario_role(guest_source), None);
    }

    #[test]
    fn start_roles_agree_for_reversed_source_order_and_the_host_seed() {
        let root = fixture_directory(LAN_STAGE, LAN_MANIFEST, LAN_MAP);
        let assets = AssetManager::from_loose_root_for_test(root.path());
        let key = installed_lan_stage(&root);
        let stage = vmce::open_stage(root.path(), &map_id(&key).unwrap()).unwrap();
        let mut host = host_room("stock.map");
        accept_selection(&mut host, root.path(), &key, None, None).unwrap();
        let mut peer = host.network_setup().unwrap().local_player().clone();
        peer.name = "Guest".into();
        peer.raw_slot_index = Some(1);
        let guest_source = host.add_network_participant(peer).unwrap();

        let mut guest_launch = host.network_setup().unwrap().clone();
        guest_launch.humans.swap(0, 1);
        let mut guest = MatchSession::default();
        guest.enter_network(NetworkReturnContext::LanLobby);
        guest
            .accept_network_setup(guest_launch, false, None, true)
            .unwrap();
        // A previous stock selection may have left a different seed.
        guest.set_network_start_seed(1).unwrap();

        let compare = |host: &MatchSession, guest: &mut MatchSession| {
            let (options, _) = options_from_selection(host, root.path(), &key, &assets).unwrap();
            let received: ScenarioOptions =
                serde_json::from_slice(&serde_json::to_vec(&options).unwrap()).unwrap();
            received.install(guest, None).unwrap();
            assert_eq!(guest.network_start_seed(), host.network_start_seed());
            let host_players = resolve_start_players(host, &stage).unwrap();
            assert_eq!(resolve_start_players(guest, &stage).unwrap(), host_players);
            host_players
        };

        let random = compare(&host, &mut guest);
        assert_ne!(random[0].house, random[1].house);
        host.set_network_start_seed(43).unwrap();
        assert_ne!(compare(&host, &mut guest), random);

        host.set_network_start_seed(42).unwrap();
        host.accept_local_scenario_role(guest_source, Some(1))
            .unwrap();
        let mixed = compare(&host, &mut guest);
        let seats = mixed
            .iter()
            .map(|player| {
                (
                    player.raw_slot_index,
                    player.role.as_str(),
                    player.house.as_str(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            seats,
            [(0, "Player1", "HouseAlpha"), (1, "Player2", "HouseCharlie")]
        );
        assert_eq!(mixed[0].shared_control_houses, ["HouseBravo"]);
        assert!(mixed[1].shared_control_houses.is_empty());

        let (admitted, _) = options_from_selection(&host, root.path(), &key, &assets).unwrap();
        guest.set_network_map_status(0, 1).unwrap();
        guest.set_network_player_ready(0, true).unwrap();
        admitted.verify_start_session(&guest).unwrap();
        use crate::app::match_runtime::session::NetworkRosterEntry;
        guest
            .accept_network_roster([NetworkRosterEntry::Human(0), NetworkRosterEntry::Human(1)])
            .unwrap();
        let error = admitted.verify_start_session(&guest).unwrap_err();
        assert!(error.to_string().contains("roster or role choices changed"));
    }

    #[test]
    fn observer_rows_follow_the_stage_policy_without_satisfying_player_limits() {
        let root = fixture_directory(LAN_STAGE, LAN_MANIFEST, LAN_MAP);
        let key = installed_lan_stage(&root);
        let mut host = host_room("stock.map");
        accept_selection(&mut host, root.path(), &key, None, None).unwrap();
        assert_eq!(host.network_join_row(true), None);

        let manifest = LAN_MANIFEST.replace("[Campaign]", "[Campaign]\nAllowObservers=yes");
        root.write(&format!("{SERIES}/campaign.ini"), manifest.as_bytes());
        accept_selection(&mut host, root.path(), &key, None, None).unwrap();
        let (row, observer) = host.network_join_row(true).unwrap();
        assert_eq!((row, observer), (2, true));
        let mut guest = host.network_setup().unwrap().local_player().clone();
        guest.name = "Spectator".into();
        guest.raw_slot_index = Some(row);
        guest.set_observer(true);
        let observer_source = host.add_network_participant(guest.clone()).unwrap();
        assert!(
            host.accept_local_scenario_role(observer_source, Some(1))
                .is_err()
        );
        assert!(checked_player_count(&host).is_err());

        guest.name = "Player Two".into();
        guest.raw_slot_index = Some(1);
        guest.set_observer(false);
        host.add_network_participant(guest).unwrap();
        assert_eq!(checked_player_count(&host).unwrap(), 2);
        assert_eq!(host.network_join_row(false), None);
        assert_eq!(host.local_scenario_row_count(), Some(3));
        let picks = scenario_seats(&host).unwrap();
        assert_eq!(picks.len(), 3);
        assert!(picks[2].observer && picks[2].role.is_none());
    }

    #[test]
    fn local_lan_scenario_owns_its_view_and_closes_the_room_until_stock_is_selected() {
        let manifest = MANIFEST
            .replace("ScenarioCount=1", "ScenarioCount=2\nScenario2=missing.map")
            + "\n[Scenario:missing.map]\nPlayerCount=1\nPlayer1=HouseAlpha\n";
        let root = fixture_directory(STAGE, &manifest, MAP);
        let modes = crate::skirmish_modes::stock_skirmish_modes();
        let stock_mode = modes
            .iter()
            .find(|mode| mode.map_filter == "standard")
            .unwrap();
        let stock = "stock.map";
        let records = [SkirmishScenarioRecord::concrete_from_ini(
            0,
            SkirmishScenarioSource::Synthetic,
            stock,
            &crate::rules::ini_parser::IniFile::from_str("[Basic]\nName=Stock map\n"),
        )];
        let layout = compute_choose_map_modal_layout(800, 600);
        let mut chooser = ChooseMapModalState::open(
            ChooseMapContext::Network {
                observer: false,
                tournament_room: false,
                clan_room: false,
            },
            stock_mode.id,
            Some(0),
            &modes,
            &records,
            &layout,
        );
        chooser.add_favorites_row(Default::default());
        chooser.add_local_scenarios(
            MODE_LABEL,
            MODE_LABEL.to_owned(),
            discover(root.path(), None),
            None,
            &layout,
        );
        let row = chooser
            .mode_rows()
            .iter()
            .copied()
            .find(|id| chooser.local_scenario_label(*id) == Some(MODE_LABEL))
            .unwrap();
        assert!(chooser.select_mode(row, &modes, &records, &layout));
        let key = chooser.accept_local_scenario().unwrap().clone();
        assert_eq!(key.path, format!("Fixture/{STAGE}"));
        assert!(chooser.accept_selection().is_none());
        assert!(
            chooser
                .toggle_favorite(&modes, &records, &layout)
                .unwrap()
                .1
        );
        assert!(chooser.select_mode(
            crate::ui::skirmish_shell::FAVORITES_ROW,
            &modes,
            &records,
            &layout
        ));
        assert_eq!(chooser.accept_local_scenario(), Some(&key));
        assert!(chooser.accept_selection().is_none());
        assert!(
            !chooser
                .toggle_favorite(&modes, &records, &layout)
                .unwrap()
                .1
        );
        assert!(chooser.accept_local_scenario().is_none());

        assert!(chooser.select_mode(row, &modes, &records, &layout));
        let missing = LocalScenarioKey {
            source: MODE_KEY,
            path: "Fixture/missing.map".into(),
        };
        let missing_row = chooser.local_scenario_row(&missing).unwrap();
        chooser.highlighted_filtered_index = Some(missing_row);
        assert!(chooser.local_scenario_disabled_row(missing_row));
        assert!(
            !chooser.button_enabled(crate::ui::skirmish_shell::ChooseMapModalButton::UseMap0x6c5)
        );
        assert!(chooser.accept_local_scenario().is_none());

        let mut session = host_room(stock);
        session
            .select_network_map(
                SkirmishLaunchMode::from_game_mode(stock_mode),
                stock.into(),
                2,
            )
            .unwrap();
        session.set_network_teams(Some(2)).unwrap();
        session.set_network_people_limit(3).unwrap();
        let stock_team = session.network_setup().unwrap().local_player().slot.team;
        assert!(matches!(
            stock_team,
            crate::skirmish_launch::LaunchTeam::Team(_)
        ));
        assert_eq!(session.network_teams(), Some(2));
        let mut stock_options = session.network_setup().unwrap().options.clone();
        stock_options.crates = true;
        session.set_network_options(stock_options).unwrap();
        session
            .set_network_vacancy(
                1,
                crate::app::match_runtime::session::NetworkVacancy::Closed,
            )
            .unwrap();
        accept_selection(&mut session, root.path(), &key, None, None).unwrap();
        assert_eq!(session.local_network_scenario().unwrap().key, key);
        assert!(!session.network_allows_peers());
        let mut guest = session.network_setup().unwrap().local_player().clone();
        guest.name = "Guest".into();
        guest.raw_slot_index = Some(1);
        assert!(session.add_network_participant(guest.clone()).is_err());
        let view = project_view(
            &session,
            SetupDialog {
                host: true,
                flavor: SetupFlavor::Lan,
                modern: true,
            },
            false,
            0..1,
            "Random",
            "Observer",
        )
        .unwrap();
        assert_eq!(view.row_count, 1);
        assert_eq!(view.map_label, "Fixture / VMCE Minimal Stage 01");
        assert!(!view.team_bar());
        assert_eq!(view.trackbars.len(), 1);
        assert_eq!(view.trackbars[0].id, DIFFICULTY);
        assert_eq!(view.trackbars[0].value, 1);
        assert!(view.enabled(DIFFICULTY));
        assert!(!view.enabled(SHORT_GAME) && !view.enabled(CRATES));
        assert!(view.enabled(crate::ui::multiplayer_setup::GAME_OPTIONS));
        assert!(view.enabled(CHOOSE_MAP) && view.enabled(START));
        assert!(!view.checks[2]);
        assert!(
            edit_room_rules(
                &mut session,
                &SetupAction::Trackbar {
                    control: DIFFICULTY,
                    value: 2
                }
            )
            .unwrap()
        );
        session.accept_local_scenario_role(0, Some(0)).unwrap();

        assert!(chooser.select_mode(stock_mode.id, &modes, &records, &layout));
        let choice = chooser.accept_selection().unwrap();
        let record = &records[choice.record_index.unwrap()];
        assert_eq!(choice.mode.id, stock_mode.id);
        session
            .select_network_map(
                SkirmishLaunchMode::from_game_mode(stock_mode),
                record.file_name.clone(),
                record.player_capacity,
            )
            .unwrap();
        assert!(session.local_network_scenario().is_none());
        assert!(session.local_scenario_role(0).is_none());
        assert!(session.network_setup().unwrap().options.crates);
        assert!(session.network_allows_peers());
        assert_eq!(session.network_free_row(false), Some(1));
        assert_eq!(session.network_teams(), Some(2));
        assert_eq!(session.network_people_limit(), Some(3));
        assert_eq!(
            session.network_setup().unwrap().local_player().slot.team,
            stock_team
        );
        assert_eq!(
            session
                .network_setup()
                .unwrap()
                .selected_map_file
                .as_deref(),
            Some(stock)
        );
        assert_eq!(session.network_start_seed(), Some(42));
        assert!(session.add_network_participant(guest).is_ok());
        assert!(can_choose_in_room(&session));
        assert!(accept_selection(&mut session, root.path(), &key, None, None).is_err());
    }

    #[test]
    fn solo_start_freezes_the_authored_seat_and_effective_options() {
        let root = fixture_directory(STAGE, MANIFEST, MAP);
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let manifest_path = format!("{SERIES}/campaign.ini");
        let map_path = format!("{SERIES}/{STAGE}");
        let map = MAP
            .replacen(
                "[Basic]",
                "[Basic]\nMinPlayers=3\nShortGame=yes\nCrates=yes\nAllowAlliesChange=yes",
                1,
            )
            .replace("0=HouseAlpha", "0=HouseAlpha\n1=HouseBravo")
            .replace(
                "[Waypoints]",
                "[HouseBravo]\nCountry=Russians\n\n[Waypoints]",
            );
        let series_minimum = MANIFEST
            .replace("[Campaign]", "[Campaign]\nMinPlayers=2\nCrates=no")
            .replace("PlayerCount=1", "PlayerCount=2\nPlayer2=HouseBravo");
        let scenario_override = series_minimum.replace(
            "Player1=HouseAlpha",
            "Player1=housealpha\nMinPlayers=1\nShortGame=no\nDifficulty=0\nLockDifficulty=yes",
        );
        root.write(&map_path, map.as_bytes());
        root.write(&manifest_path, series_minimum.as_bytes());

        let mut session = host_room("stock.map");
        let key = discover(root.path(), None).remove(0).selection.key;
        accept_selection(&mut session, root.path(), &key, None, None).unwrap();
        let starts = |session: &MatchSession| {
            project_view(
                session,
                SetupDialog {
                    host: true,
                    flavor: SetupFlavor::Lan,
                    modern: true,
                },
                false,
                0..1,
                "Random",
                "Observer",
            )
            .unwrap()
            .enabled(START)
        };
        assert!(!starts(&session));
        let error = admit_start(&mut session, root.path(), &key, &assets).unwrap_err();
        assert!(
            error.to_string().contains("requires at least 2 players"),
            "{error:#}"
        );
        assert!(session.local_scenario_player().is_none());
        assert!(can_choose_in_room(&session));

        root.write(&manifest_path, scenario_override.as_bytes());
        accept_selection(&mut session, root.path(), &key, None, None).unwrap();
        assert!(starts(&session));
        let toggle = SetupAction::Toggle {
            control: SHORT_GAME,
        };
        let change = SetupAction::Trackbar {
            control: DIFFICULTY,
            value: 2,
        };
        assert!(!edit_room_rules(&mut session, &toggle).unwrap());
        assert!(edit_room_rules(&mut session, &change).is_err());
        let startup = admit_start(&mut session, root.path(), &key, &assets).unwrap();
        let seat = startup.authored_local_player().unwrap();
        assert_eq!(seat.role, "Player1");
        assert_eq!(seat.house, "HouseAlpha");
        assert_eq!(startup.seed.value, 42);
        assert!(session.admits_network_start(&startup));
        assert!(!session.network_allows_peers());
        assert!(!can_choose_in_room(&session));
        assert!(session.set_network_start_seed(43).is_err());
        assert!(edit_room_rules(&mut session, &change).is_err());
        assert_eq!(
            startup.authored_scenario().unwrap().ai_difficulty,
            HouseDifficulty::Easy
        );
        let options = &startup.launch_session().options;
        assert!(!options.short_game && !options.crates && options.ally_change_allowed);
    }

    #[test]
    fn raw_manifest_and_current_map_changes_require_reselection() {
        use crate::util::sha256::sha256_hex;
        let root = fixture_directory(STAGE, MANIFEST, MAP);
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let manifest_path = format!("{SERIES}/campaign.ini");
        let current = format!("{SERIES}/{STAGE}");
        let later = format!("{SERIES}/later.map");
        let manifest = MANIFEST.replace("ScenarioCount=1", "ScenarioCount=2\nScenario2=later.map")
            + "\n[Scenario:later.map]\nPlayerCount=1\nPlayer1=HouseAlpha\n";
        root.write(&manifest_path, manifest.as_bytes());
        root.write(&later, MAP.as_bytes());
        let mut session = host_room("stock.map");
        let key = discover(root.path(), None).remove(0).selection.key;
        accept_selection(&mut session, root.path(), &key, None, None).unwrap();
        let changed_manifest = manifest + "\n; raw manifest change\n";
        let changed_map = format!("{MAP}\n; raw map change\n");
        for (path, bytes, expected) in [
            (
                &manifest_path,
                changed_manifest.as_bytes(),
                "manifest changed",
            ),
            (&current, changed_map.as_bytes(), "current map changed"),
        ] {
            root.write(path, bytes);
            let error = admit_start(&mut session, root.path(), &key, &assets).unwrap_err();
            assert!(error.to_string().contains(expected), "{error:#}");
            assert!(can_choose_in_room(&session));
            assert_eq!(session.network_start_seed(), Some(42));
            assert!(session.local_scenario_player().is_none());
            accept_selection(&mut session, root.path(), &key, None, None).unwrap();
        }

        root.write(&later, b"invalid later map");
        let startup = admit_start(&mut session, root.path(), &key, &assets).unwrap();
        assert!(session.admits_network_start(&startup));
        root.write(
            &manifest_path,
            b"later manifest mutation while startup retains the accepted values",
        );
        let authored = startup.authored_scenario().unwrap();
        assert_eq!(authored.selection.key, key);
        assert_eq!(
            authored.configuration_sha256,
            sha256_hex(changed_manifest.as_bytes())
        );
        assert_eq!(
            startup.local_map_source().unwrap().physical_identity(),
            Some((
                changed_map.len(),
                sha256_hex(changed_map.as_bytes()).as_str()
            )),
        );
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    #[test]
    #[ignore = "slow; requires a desktop GPU surface and retail assets"]
    fn frozen_stage_loads_roster_and_control_grants_through_the_lan_loader() {
        // Use the same stack budget as the game's loading thread.
        let load = std::thread::Builder::new()
            .stack_size(8 << 20)
            .spawn(load_frozen_stage_with_retail_assets)
            .unwrap();
        if let Err(panic) = load.join() {
            std::panic::resume_unwind(panic);
        }
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    #[allow(
        deprecated,
        reason = "test creates a hidden surface outside an app event loop"
    )]
    fn load_frozen_stage_with_retail_assets() {
        use crate::app::loading::pump::LoadingRequest;
        use crate::app::match_bootstrap::LoadingStartup;
        use crate::assets::asset_manager::MediaArchiveMode;
        use crate::render::{batch::BatchRenderer, gpu::GpuContext};
        use crate::sim::command::{Command, CommandRecord};
        use crate::sim::runtime::{SimResources, SimRuntime};
        use crate::sim::world::{FrameEffects, Simulation, TickLane};
        use crate::sim::{pathfinding, scenario_session::ScenarioGameMode};
        #[cfg(target_os = "windows")]
        use winit::platform::windows::EventLoopBuilderExtWindows;
        #[cfg(target_os = "linux")]
        use winit::platform::x11::EventLoopBuilderExtX11;

        let Some((ra2_dir, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        const SHARED_STAGE: &str = "shared_move_stage01.map";
        const SHARED_MAP: &str =
            include_str!("../../../tests/fixtures/vmce/shared_move/shared_move_stage01.map");
        let manifest = include_str!("../../../tests/fixtures/vmce/shared_move/campaign.ini")
            .replace(
                "Player1=HouseAlpha",
                "Player1=HouseAlpha\nAllowAlliesChange=yes\nCrates=yes",
            );
        let root = fixture_directory(SHARED_STAGE, &manifest, SHARED_MAP);
        let manifest_path = format!("{SERIES}/campaign.ini");
        let map_path = format!("{SERIES}/{SHARED_STAGE}");
        let mut session = host_room("stock.map");
        let key = discover(root.path(), None).remove(0).selection.key;
        accept_selection(&mut session, root.path(), &key, None, None).unwrap();
        let startup = admit_start(&mut session, root.path(), &key, &assets).unwrap();
        assert_eq!(
            startup
                .authored_local_player()
                .unwrap()
                .shared_control_houses,
            ["HouseBravo"]
        );

        let event_loop = winit::event_loop::EventLoop::builder()
            .with_any_thread(true)
            .build()
            .unwrap();
        let window = Arc::new(
            event_loop
                .create_window(
                    winit::window::Window::default_attributes()
                        .with_visible(false)
                        .with_inner_size(winit::dpi::PhysicalSize::new(800, 600)),
                )
                .unwrap(),
        );
        let gpu = GpuContext::new(window, event_loop.owned_display_handle(), true).unwrap();
        let batch = BatchRenderer::new(&gpu);
        let mut process_assets =
            crate::app::process_assets::ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false);
        process_assets
            .initialize_sources_if_needed(&assets)
            .unwrap();
        let mut load = || {
            LoadingRequest::network(LoadingStartup::Network(Arc::clone(&startup)))
                .load_fresh_map_for_test(
                    ra2_dir.clone(),
                    &mut assets,
                    &mut process_assets,
                    &gpu,
                    &batch,
                )
        };

        root.write(
            &map_path,
            format!("{SHARED_MAP}\n; changed after Start\n").as_bytes(),
        );
        let error = load().err().expect("the frozen map rejects changed bytes");
        assert!(
            error.to_string().contains("network map bytes changed"),
            "{error:#}"
        );

        root.write(&map_path, SHARED_MAP.as_bytes());
        root.write(
            &manifest_path,
            manifest
                .replace("Player1Control=yes", "Player1Control=no")
                .as_bytes(),
        );
        let result = load().unwrap();
        let LoadingStartup::Network(installed_startup) = &result.scenario.startup else {
            panic!("the loader did not retain LAN identity");
        };
        assert!(Arc::ptr_eq(&startup, installed_startup));
        assert_eq!(
            result.scenario.map_source.physical_identity(),
            startup.local_map_source().unwrap().physical_identity()
        );
        let sim = result.scenario.simulation.as_ref().unwrap();
        let owner = sim.session().current_house().unwrap();
        assert_eq!(sim.interner().resolve(owner), "HouseAlpha");
        assert!(sim.houses()[&owner].is_controlled_by_human(true));
        assert_eq!(
            sim.houses()
                .values()
                .filter(|house| house.is_human())
                .count(),
            1
        );
        assert_eq!(sim.session().game_mode(), ScenarioGameMode::Lan);
        assert_eq!(sim.session().seed(), 42);
        assert_eq!(sim.session().house_control_grants(), &[(0, 1)]);
        let options = sim.session().game_options();
        assert!(
            !options.short_game
                && options.ally_change_allowed
                && !options.crates
                && options.ai_players == 0
        );
        assert!(
            sim.crate_authority()
                .slots()
                .iter()
                .all(|slot| slot.is_empty()),
            "legacy Crates=yes must leave authored startup free of random crates"
        );
        assert_eq!(sim.entities().len(), 3);
        let grid = sim.path_grid().unwrap();
        for (name, unit_type, from, to) in [
            ("HouseAlpha", "MTNK", (32, 32), (36, 36)),
            ("HouseBravo", "HTNK", (30, 32), (30, 36)),
            ("HouseCharlie", "FV", (34, 32), (38, 32)),
        ] {
            let house = sim.interner().get(name).unwrap();
            assert!(
                sim.house_alliances()
                    .is_allied(sim.interner(), owner, house)
            );
            let unit = sim
                .entities()
                .values()
                .find(|unit| unit.owner() == house)
                .unwrap();
            assert_eq!(sim.interner().resolve(unit.type_ref()), unit_type);
            assert_eq!((unit.position.rx(), unit.position.ry()), from);
            assert!(pathfinding::find_path(grid, from, to).is_some(), "{name}");
        }
        assert_eq!(
            std::fs::read(root.path().join(map_path)).unwrap(),
            SHARED_MAP.as_bytes()
        );

        let charlie = sim.interner().get("HouseCharlie").unwrap();
        let actor = sim
            .entities()
            .values()
            .find(|unit| unit.owner() == charlie)
            .unwrap();
        let entity_id = actor.stable_id();
        let record = sim
            .encode_megamission_record(
                owner,
                &Command::Move {
                    entity_id,
                    target_rx: 38,
                    target_ry: 32,
                },
            )
            .unwrap();
        let mut runtime = SimRuntime {
            simulation: result.scenario.simulation.unwrap(),
            resources: Arc::new(SimResources {
                rules: result.scenario.rules.unwrap(),
                terrain_template: result.scenario.resolved_terrain,
            }),
        };
        assert!(
            runtime
                .simulation
                .network_command_queues()
                .admit_received(record.clone())
        );
        let applied = std::cell::Cell::new(None);
        let observe = |_: &Simulation, delivered: &CommandRecord, accepted| {
            assert_eq!(delivered.payload(), record.payload());
            assert_eq!(delivered.house_id(), 0);
            assert_eq!(applied.replace(Some(accepted)), None);
        };
        runtime
            .advance_frame(
                &[],
                33,
                TickLane::Ordinary,
                FrameEffects::default().with_command_applied(&observe),
            )
            .unwrap();
        assert_eq!(applied.get(), Some(false));
        let actor = runtime.simulation.entities().get(entity_id).unwrap();
        assert_eq!(actor.navigation.nav_com(), None);
        assert_eq!((actor.position.rx(), actor.position.ry()), (34, 32));
    }
}
