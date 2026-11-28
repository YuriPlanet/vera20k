//! The online map library (docs/lobby-protocol.md): in an online room,
//! Choose Map lists the lobby's curated maps under its Online Maps row, and
//! Use Map on one asks to download it. The download is saved in VERA's own
//! Maps folder (never the game folder) as a loose `.map`, where it is an
//! ordinary map from then on, in every game type's list. Players star maps they like, online or not; the stars are kept in
//! `MapFavorites.txt` in the settings folder and list under Choose Map's
//! Favorites row. VERA-only; gamemd has no map
//! library.
//!
//! The catalog is read in the background when the player goes online, and
//! kept for the rest of the run once it arrives. A failed read is tried once
//! more during that visit, and again on the next. Each entry carries the map's own `[Basic]`, `[Preview]`,
//! `[PreviewPack]`, `[Waypoints]`, `[Header]` and `[RandomMap]` sections, so
//! its row is read by the same record reader as a local map's.

use crate::app::shell_route::ShellRoute;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use anyhow::{Context, Result};

use crate::map::skirmish_scenarios::{
    SkirmishScenarioKind, SkirmishScenarioRecord, SkirmishScenarioSource,
};
use crate::map::source::{KeptMap, KeptMapFiles};
use crate::ui::skirmish_shell::online_map_folders::OnlineMapFolders;
use crate::ui::skirmish_shell::{ChooseMapPrompt, ChooseMapPromptKind, ChooseMapSelection};
use crate::util::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// One map the library offers.
#[derive(Debug, Clone)]
pub(crate) struct OnlineMap {
    pub(crate) sha256: String,
    /// `[Basic] Name=`.
    pub(crate) name: String,
    /// The local `.map` basename derived from its catalog name.
    pub(crate) file_name: String,
    pub(crate) byte_length: usize,
    /// Its Choose Map row, read from the header (source ordinal unset).
    record: SkirmishScenarioRecord,
    /// The catalog's copy of the map's header lines, for its preview.
    header: Arc<[u8]>,
}

enum Phase<T> {
    /// Not read during this online visit: read on going online.
    NotStarted,
    Fetching {
        result: Receiver<Result<T>>,
        retried: bool,
    },
    /// The read failed; `retry_at` is the visit's one more try.
    Failed { retry_at: Option<Instant> },
    /// The catalog arrived, or this run needs none: final.
    Ready,
}

impl<T> Default for Phase<T> {
    fn default() -> Self {
        Self::NotStarted
    }
}

impl<T> Phase<T> {
    /// The catalog and category feed share a retry policy but run independently.
    fn poll(
        &mut self,
        online: bool,
        start: impl FnOnce() -> Option<Receiver<Result<T>>>,
    ) -> Option<Result<T>> {
        let retried = match self {
            Self::NotStarted if online => false,
            Self::Failed { .. } if !online => {
                *self = Self::NotStarted;
                return None;
            }
            Self::Failed { retry_at: Some(at) } if Instant::now() >= *at => true,
            Self::Fetching { result, retried } => {
                let result = match result.try_recv() {
                    Ok(result) => result,
                    Err(TryRecvError::Empty) => return None,
                    Err(TryRecvError::Disconnected) => {
                        Err(anyhow::anyhow!("the library worker stopped"))
                    }
                };
                *self = if result.is_ok() {
                    Self::Ready
                } else {
                    Self::failed(*retried)
                };
                return Some(result);
            }
            _ => return None,
        };
        *self = start().map_or(Self::Ready, |result| Self::Fetching { result, retried });
        None
    }

    fn failed(retried: bool) -> Self {
        Self::Failed {
            retry_at: (!retried).then(|| Instant::now() + CATALOG_RETRY),
        }
    }

    fn next(&self, next_poll: Instant, online: bool) -> Option<Instant> {
        match self {
            Self::Fetching { .. } => Some(next_poll),
            Self::Failed { retry_at } if online => *retry_at,
            _ => None,
        }
    }
}

/// How long a failed catalog read waits before its one retry in a visit.
const CATALOG_RETRY: Duration = Duration::from_secs(30);

struct Download {
    sha256: String,
    result: Receiver<Result<Vec<u8>>>,
    cancel: Arc<AtomicBool>,
    /// Bytes received so far, of `length`, for the box's progress bar.
    received: Arc<AtomicUsize>,
    length: usize,
}

/// The library's catalog and the one download Choose Map may run.
#[derive(Default)]
pub(crate) struct MapLibrary {
    phase: Phase<Vec<OnlineMap>>,
    categories: Phase<HashMap<String, Vec<String>>>,
    folders: OnlineMapFolders,
    maps: Vec<OnlineMap>,
    download: Option<Download>,
    /// Starred maps by `favorite_key`, read when Choose Map opens.
    favorites: std::collections::BTreeSet<String>,
}

const FAVORITES_FILE: &str = "MapFavorites.txt";

impl MapLibrary {
    pub(crate) fn folders(&self) -> OnlineMapFolders {
        self.folders.clone()
    }

    /// The library entry behind an Online Choose Map row.
    pub(crate) fn online_map(&self, record: &SkirmishScenarioRecord) -> Option<&OnlineMap> {
        let SkirmishScenarioSource::Online(sha256) = &record.source else {
            return None;
        };
        self.maps.iter().find(|map| &map.sha256 == sha256)
    }
}

/// Starts the catalog read once the player is online, then takes the
/// catalog and a finished download.
pub(crate) fn poll(
    platform: &crate::app::PlatformState,
    renderer: &crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    process_assets: &crate::app::process_assets::ProcessAssets,
) -> Option<Instant> {
    let online = in_online_play(frontend) && !frontend.external_launch;
    let catalog = frontend
        .map_library
        .phase
        .poll(online, || start_catalog(platform));
    let categories = frontend.map_library.categories.poll(online, || {
        crate::app::worker::spawn(
            "vera-map-categories",
            Arc::new(AtomicBool::new(false)),
            |_| fetch::categories(),
        )
    });
    let refresh = catalog.is_some() || categories.is_some();
    match catalog {
        Some(Ok(maps)) => {
            log::info!("The online map library lists {} maps", maps.len());
            frontend.map_library.maps = maps;
            append_online_records(platform, frontend, process_assets);
        }
        Some(Err(error)) => log::info!("The online map library is unavailable: {error:#}"),
        None => {}
    }
    match categories {
        Some(Ok(memberships)) => frontend.map_library.folders.set_memberships(memberships),
        Some(Err(error)) => log::info!("The online map categories are unavailable: {error:#}"),
        None => {}
    }
    if refresh {
        refresh_choose_map(platform, renderer, frontend);
    }
    // A download belongs to Choose Map's box: it stops when the chooser
    // goes, however it went.
    if frontend.skirmish_shell_state.choose_map_modal.is_none() {
        cancel_download(frontend);
    }
    let library = &mut frontend.map_library;
    if let Some(download) = library.download.as_ref() {
        match download.result.try_recv() {
            Ok(result) => {
                let sha256 = library.download.take().map(|done| done.sha256);
                finish_download(
                    platform,
                    renderer,
                    frontend,
                    process_assets,
                    sha256.unwrap_or_default(),
                    result,
                );
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                let sha256 = library.download.take().map(|done| done.sha256);
                finish_download(
                    platform,
                    renderer,
                    frontend,
                    process_assets,
                    sha256.unwrap_or_default(),
                    Err(anyhow::anyhow!("the download stopped")),
                );
            }
        }
    }
    if frontend.map_library.download.is_some() {
        // The download box's bar follows the bytes as they arrive.
        platform.window.request_redraw();
    }
    let library = &frontend.map_library;
    let next = Instant::now() + POLL_INTERVAL;
    [
        library.download.as_ref().map(|_| next),
        library.phase.next(next, online),
        library.categories.next(next, online),
    ]
    .into_iter()
    .flatten()
    .min()
}

/// What an empty Online Maps list says.
pub(crate) fn empty_online_maps_text(
    frontend: &crate::app::frontend::state::FrontendState,
) -> String {
    crate::i18n::tr(match frontend.map_library.phase {
        Phase::NotStarted | Phase::Fetching { .. } => "VERA:OnlineMapsLoading",
        Phase::Failed { .. } => "VERA:OnlineMapsUnavailable",
        Phase::Ready => "VERA:OnlineMapsEmpty",
    })
}

/// An open Choose Map shows the catalog's rows as they are now.
fn refresh_choose_map(
    platform: &crate::app::PlatformState,
    renderer: &crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
) {
    let layout = crate::app::frontend::shell_layout::choose_map_layout(renderer, &frontend.screen);
    if let Some(modal) = frontend.skirmish_shell_state.choose_map_modal.as_mut() {
        modal.set_online_categories(frontend.map_library.folders());
        modal.refresh(
            &frontend.skirmish_modes,
            frontend.scenario_catalog.records(),
            &layout,
        );
    }
    platform.window.request_redraw();
}

/// Adds missing library rows and presents maps already on disk under their
/// playable local names. Also run when the scenario catalog is rebuilt.
pub(crate) fn append_online_records(
    platform: &crate::app::PlatformState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    process_assets: &crate::app::process_assets::ProcessAssets,
) {
    let Some(config) = platform.game_config.as_ref() else {
        return;
    };
    if frontend.map_library.maps.is_empty() {
        return;
    }
    let ra2_dir = config.paths.ra2_dir.clone();
    let maps_dir = config.paths.downloaded_maps_dir();
    let folders: Vec<&Path> = crate::map::source::mod_dir()
        .into_iter()
        .chain([ra2_dir.as_path(), maps_dir.as_path()])
        .collect();
    read_favorites(platform, frontend);
    let library = &mut frontend.map_library;
    if append_rows(
        &mut frontend.scenario_catalog.records_mut(),
        &library.maps,
        &folders,
        &mut library.favorites,
        &mut library.folders,
        process_assets.manager(),
    ) {
        save_favorites(platform, frontend);
    }
}

fn append_rows(
    records: &mut Vec<SkirmishScenarioRecord>,
    maps: &[OnlineMap],
    folders: &[&Path],
    favorites: &mut BTreeSet<String>,
    categories: &mut OnlineMapFolders,
    assets: Option<&crate::assets::asset_manager::AssetManager>,
) -> bool {
    categories.reset_installed();
    let files = KeptMapFiles::scan(folders);
    // A selected client install supplies its own scenario list. Only ordinary
    // map discovery adopts an unrelated loose file without a player choosing it.
    let discover_loose = assets
        .and_then(|assets| assets.client_install().zip(assets.mod_dir()))
        .is_none();
    let mut taken: HashMap<String, usize> = records
        .iter()
        .enumerate()
        .map(|(index, record)| (record.file_name.to_lowercase(), index))
        .collect();
    let mut starred = false;
    for map in maps {
        if let Some(kept) = files.existing(map.byte_length, &map.sha256) {
            // A loose file copied since the last scan can be the first copy
            // this catalog sees. Register it through the normal scenario reader.
            let key = kept.name.to_lowercase();
            if discover_loose
                && !taken.contains_key(&key)
                && let Some(record) = crate::map::scenario_sources::loose_map_record(
                    &kept.path,
                    kept.name.clone(),
                    records.len(),
                    assets,
                )
            {
                taken.insert(key.clone(), records.len());
                records.push(record);
            }
            if let Some(&index) = taken.get(&key)
                && !records[index].is_online()
            {
                categories.record_installed(&records[index].file_name, &map.sha256);
                starred |= carry_favorite(favorites, &map.sha256, &records[index]);
                continue;
            }
        }
        let names = crate::map::source::kept_map_names(&map.file_name, &map.sha256);
        if let Some(name) = names
            .into_iter()
            .find(|name| !files.contains_name(name) && !taken.contains_key(&name.to_lowercase()))
        {
            taken.insert(name.to_lowercase(), records.len());
            let mut record = map.record.clone();
            record.file_name = name;
            record.source_ordinal = records.len();
            records.push(record);
        }
    }
    starred
}

fn local_row(records: &[SkirmishScenarioRecord], name: &str) -> Option<usize> {
    let name = name.to_lowercase();
    records
        .iter()
        .position(|record| !record.is_online() && record.file_name.to_lowercase() == name)
}

fn carry_favorite(
    favorites: &mut BTreeSet<String>,
    digest: &str,
    record: &SkirmishScenarioRecord,
) -> bool {
    if favorites.contains(digest)
        && let Some(key) = crate::ui::skirmish_shell::favorite_key(record)
    {
        favorites.remove(digest);
        favorites.insert(key);
        return true;
    }
    false
}

/// The catalog header of an online row (`record_index` into the scenario
/// catalog).
fn catalog_header(
    frontend: &crate::app::frontend::state::FrontendState,
    record_index: usize,
) -> Option<&[u8]> {
    let record = frontend.scenario_catalog.records().get(record_index)?;
    match record.source {
        SkirmishScenarioSource::Bundled => crate::map::source::bundled_map_bytes(&record.file_name),
        _ => Some(&frontend.map_library.online_map(record)?.header),
    }
}

/// A bundled or downloadable map supplies its own preview bytes to the chooser.
pub(crate) fn catalog_preview(
    frontend: &crate::app::frontend::state::FrontendState,
    record_index: usize,
) -> Option<crate::map::preview::DecodedPreview> {
    let ini =
        crate::rules::ini_parser::IniFile::from_bytes(catalog_header(frontend, record_index)?)
            .ok()?;
    crate::map::preview::decode_preview_image_from_ini(&ini)
        .ok()
        .flatten()
}

/// The briefing of the catalog row `record_index` (`map::basic::
/// briefing_text`): an online row's from its catalog header, which keeps the
/// map's whole `[Basic]` section, a loose map's from its file and the active
/// string table. The retail maps in mixfiles carry none.
pub(crate) fn briefing(
    platform: &crate::app::PlatformState,
    frontend: &crate::app::frontend::state::FrontendState,
    process_assets: &crate::app::process_assets::ProcessAssets,
    record_index: usize,
) -> Option<String> {
    let record = frontend.scenario_catalog.records().get(record_index)?;
    match record.source {
        SkirmishScenarioSource::Online(_) => {
            crate::map::basic::briefing_text(catalog_header(frontend, record_index)?)
        }
        SkirmishScenarioSource::Bundled => crate::map::basic::briefing_text_with_csf(
            catalog_header(frontend, record_index)?,
            process_assets.csf.as_ref(),
        ),
        SkirmishScenarioSource::ArchiveMap => crate::map::basic::briefing_text_with_csf(
            process_assets.manager()?.get_ref(&record.file_name)?,
            process_assets.csf.as_ref(),
        ),
        SkirmishScenarioSource::LooseMap(_) => {
            let path = crate::map::source::local_map_path(
                crate::app::state::ra2_dir(platform, process_assets)?,
                &record.file_name,
            )
            .ok()?;
            let bytes = crate::util::file_system::read(path).ok()?;
            crate::map::basic::briefing_text_with_csf(&bytes, process_assets.csf.as_ref())
        }
        _ => None,
    }
}

/// The running download's bytes received and its length.
pub(crate) fn download_progress(
    frontend: &crate::app::frontend::state::FrontendState,
) -> Option<(u64, u64)> {
    frontend.map_library.download.as_ref().map(|download| {
        let received = download.received.load(Ordering::Relaxed);
        (received as u64, download.length as u64)
    })
}

/// Whether this Choose Map shows the Online Maps row: online game rooms
/// only, which are network setup routes of a Westwood Online session, in a
/// build that can read the library.
pub(crate) fn listed_in_choose_map(
    frontend: &crate::app::frontend::state::FrontendState,
    match_state: &crate::app::match_runtime::state::MatchState,
) -> bool {
    crate::app::worker::THREADS
        && frontend.shell_route == ShellRoute::NetworkSetup
        && match_state.session_mode() == crate::app::match_runtime::session::SessionMode::Wol
}

/// Whether the player is in VERA20k Online: its pages, lobby or a room.
fn in_online_play(frontend: &crate::app::frontend::state::FrontendState) -> bool {
    frontend.wol.is_some()
}

/// Star or unstar a map, and keep the stars for later runs. The library
/// owns the stars; an open Choose Map gets a fresh copy.
pub(crate) fn set_favorite(
    platform: &crate::app::PlatformState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    key: String,
    starred: bool,
) {
    let favorites = &mut frontend.map_library.favorites;
    if starred {
        favorites.insert(key);
    } else {
        favorites.remove(&key);
    }
    save_favorites(platform, frontend);
}

fn save_favorites(
    platform: &crate::app::PlatformState,
    frontend: &mut crate::app::frontend::state::FrontendState,
) {
    if let Some(modal) = frontend.skirmish_shell_state.choose_map_modal.as_mut() {
        modal.favorites = frontend.map_library.favorites.clone();
    }
    let Some(path) = favorites_path(platform) else {
        return;
    };
    let text: String = frontend
        .map_library
        .favorites
        .iter()
        .map(|key| format!("{key}\n"))
        .collect();
    if let Err(error) = crate::util::file_system::create_dir_all(path.parent().unwrap_or(&path))
        .and_then(|()| crate::util::file_system::write(&path, text))
    {
        log::warn!("Could not save the map favorites: {error}");
    }
}

fn favorites_path(platform: &crate::app::PlatformState) -> Option<std::path::PathBuf> {
    platform
        .game_config
        .as_ref()
        .map(|config| config.paths.settings_dir().join(FAVORITES_FILE))
}

/// The stars from earlier runs, read as Choose Map opens; a missing or
/// unreadable file stars nothing. A line is an online map's SHA-256 or
/// another map's lower-case file name.
pub(crate) fn read_favorites(
    platform: &crate::app::PlatformState,
    frontend: &mut crate::app::frontend::state::FrontendState,
) -> std::collections::BTreeSet<String> {
    let favorites: std::collections::BTreeSet<String> = favorites_path(platform)
        .and_then(|path| crate::util::file_system::read(path).ok())
        .map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .lines()
                .map(str::trim)
                .filter(|key| valid_favorite_key(key))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    frontend.map_library.favorites = favorites.clone();
    favorites
}

/// A star's key as [`crate::ui::skirmish_shell::favorite_key`] writes it:
/// one short line with no upper-case ASCII or control characters.
fn valid_favorite_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 260
        && !key
            .chars()
            .any(|ch| ch.is_control() || ch.is_ascii_uppercase())
}

/// Use Map on an Online row: ask to download it first. False for a map
/// already saved.
pub(crate) fn prompt_download(
    platform: &crate::app::PlatformState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    selection: ChooseMapSelection,
) -> bool {
    let online = selection
        .record_index
        .and_then(|index| frontend.scenario_catalog.records().get(index))
        .is_some_and(|record| frontend.map_library.online_map(record).is_some());
    if !online {
        return false;
    }
    set_prompt(platform, frontend, selection, ChooseMapPromptKind::Download);
    true
}

/// Download in the box: fetch the map while the box waits.
pub(crate) fn start_download(
    platform: &crate::app::PlatformState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    selection: ChooseMapSelection,
) {
    let Some(map) = selection
        .record_index
        .and_then(|index| frontend.scenario_catalog.records().get(index))
        .and_then(|record| frontend.map_library.online_map(record))
        .cloned()
    else {
        return;
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let received = Arc::new(AtomicUsize::new(0));
    let sha256 = map.sha256.clone();
    let length = map.byte_length;
    let counter = Arc::clone(&received);
    let Some(result) =
        crate::app::worker::spawn("vera-map-download", Arc::clone(&cancel), move |cancel| {
            fetch::map(&map.sha256, map.byte_length, cancel, &counter)
        })
    else {
        set_prompt(
            platform,
            frontend,
            selection,
            ChooseMapPromptKind::DownloadFailed,
        );
        return;
    };
    cancel_download(frontend);
    frontend.map_library.download = Some(Download {
        sha256,
        result,
        cancel,
        received,
        length,
    });
    set_prompt(
        platform,
        frontend,
        selection,
        ChooseMapPromptKind::Downloading,
    );
}

/// Cancel in the box, or the box closing: drop a running download.
pub(crate) fn cancel_download(frontend: &mut crate::app::frontend::state::FrontendState) {
    if let Some(download) = frontend.map_library.download.take() {
        download.cancel.store(true, Ordering::Relaxed);
    }
}

fn set_prompt(
    platform: &crate::app::PlatformState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    selection: ChooseMapSelection,
    kind: ChooseMapPromptKind,
) {
    if let Some(modal) = frontend.skirmish_shell_state.choose_map_modal.as_mut() {
        modal.message_box = Some(ChooseMapPrompt { selection, kind });
    }
    platform.window.request_redraw();
}

/// A download ended: keep the map and say so, or say it failed. The box
/// stays until the player answers it.
fn finish_download(
    platform: &crate::app::PlatformState,
    renderer: &crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    process_assets: &crate::app::process_assets::ProcessAssets,
    sha256: String,
    result: Result<Vec<u8>>,
) {
    // The box this download belongs to, unless the player closed it.
    let Some(prompt) = frontend
        .skirmish_shell_state
        .choose_map_modal
        .as_ref()
        .and_then(|modal| modal.message_box)
        .filter(|prompt| prompt.kind == ChooseMapPromptKind::Downloading)
    else {
        return;
    };
    // Only one download can own this box. The row may have changed while the
    // bytes were in flight, for example after receiving the same map in a room.
    let result = result
        .map_err(|error| (ChooseMapPromptKind::DownloadFailed, error))
        .and_then(|bytes| {
            keep_download(
                platform,
                renderer,
                frontend,
                process_assets,
                &sha256,
                &bytes,
            )
            .map_err(|error| (ChooseMapPromptKind::SaveFailed, error))
        });
    let mut selection = prompt.selection;
    let kind = match result {
        Ok(index) => {
            selection.record_index = Some(index);
            ChooseMapPromptKind::Downloaded
        }
        Err((kind, error)) => {
            log::warn!("Could not download the online map {sha256}: {error:#}");
            kind
        }
    };
    set_prompt(platform, frontend, selection, kind);
}

/// Select the actual kept file, including an existing copy with a different name.
fn keep_download(
    platform: &crate::app::PlatformState,
    renderer: &crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    process_assets: &crate::app::process_assets::ProcessAssets,
    sha256: &str,
    bytes: &[u8],
) -> Result<usize> {
    let map = frontend
        .map_library
        .maps
        .iter()
        .find(|map| map.sha256 == sha256)
        .context("the map left the library")?;
    let file_name = map.file_name.clone();
    let name = map.name.clone();
    let (path, index) = keep_map_and_list(
        platform,
        renderer,
        frontend,
        process_assets,
        &file_name,
        bytes,
    )?;
    log::info!("Downloaded the online map {name} as {}", path.display());
    Ok(index)
}

/// Keeps a downloaded or received map in VERA's Maps folder
/// ([`crate::map::source::keep_received_map`]) and lists it in Choose Map:
/// the library row for these bytes becomes the kept file's row, and a file
/// that no row names yet gets one. Returns the kept file's path.
pub(crate) fn keep_map(
    platform: &crate::app::PlatformState,
    renderer: &crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    process_assets: &crate::app::process_assets::ProcessAssets,
    host_file: &str,
    bytes: &[u8],
) -> Result<std::path::PathBuf> {
    keep_map_and_list(
        platform,
        renderer,
        frontend,
        process_assets,
        host_file,
        bytes,
    )
    .map(|(path, _)| path)
}

fn keep_map_and_list(
    platform: &crate::app::PlatformState,
    renderer: &crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    process_assets: &crate::app::process_assets::ProcessAssets,
    host_file: &str,
    bytes: &[u8],
) -> Result<(std::path::PathBuf, usize)> {
    let (ra2_dir, maps_dir) = platform
        .game_config
        .as_ref()
        .map(|config| {
            (
                config.paths.ra2_dir.clone(),
                config.paths.downloaded_maps_dir(),
            )
        })
        .context("no game folder")?;
    let sha256 = crate::util::sha256::sha256_hex(bytes);
    let records = frontend.scenario_catalog.records();
    let online = records.iter().position(
        |record| matches!(&record.source, SkirmishScenarioSource::Online(sha) if sha == &sha256),
    );
    let prompted = online.is_some_and(|index| {
        frontend
            .skirmish_shell_state
            .choose_map_modal
            .as_ref()
            .and_then(|modal| modal.message_box)
            .is_some_and(|prompt| {
                prompt.selection.record_index == Some(index)
                    && matches!(
                        prompt.kind,
                        ChooseMapPromptKind::Download | ChooseMapPromptKind::Downloading
                    )
            })
    });
    let kept = crate::map::source::keep_received_map_with_names(
        &maps_dir,
        &ra2_dir,
        host_file,
        bytes,
        |name| {
            let key = name.to_lowercase();
            records.iter().enumerate().any(|(index, record)| {
                Some(index) != online && record.file_name.to_lowercase() == key
            })
        },
    )?;
    let mut records = frontend.scenario_catalog.records_mut();
    let (index, removed) = list_kept_file(&mut records, online, &kept, process_assets.manager())?;
    if frontend
        .map_library
        .maps
        .iter()
        .any(|map| map.sha256 == sha256)
    {
        frontend
            .map_library
            .folders
            .record_installed(&records[index].file_name, &sha256);
    }
    let carried_star = carry_favorite(
        &mut frontend.map_library.favorites,
        &sha256,
        &records[index],
    );
    drop(records);
    if let Some(removed) = removed {
        // An existing local row won. Point references to the old Online row
        // at that local row and shift the indices after the removed row.
        let remap = |old: usize| {
            if old == removed {
                index
            } else {
                old - usize::from(old > removed)
            }
        };
        let runtime = &mut frontend.multiplayer_shell_runtime;
        runtime.set_scenario_index(remap(runtime.scenario_index()));
        if let Some(page) = frontend.coop_score_page.as_mut() {
            page.model.map_index = page.model.map_index.map(remap);
        }
        let shell = &mut frontend.skirmish_shell_state;
        shell.selected_map_idx = remap(shell.selected_map_idx);
        if let Some(modal) = shell.choose_map_modal.as_mut() {
            modal.saved_selection.record_index = modal.saved_selection.record_index.map(remap);
            for row in &mut modal.filtered_record_indices {
                *row = remap(*row);
            }
            if let Some(prompt) = modal.message_box.as_mut() {
                prompt.selection.record_index = prompt.selection.record_index.map(remap);
            }
            // A Use Map waiting for the chooser's slide-out keeps its map.
            if let Some(selection) = modal.use_selection.as_mut() {
                selection.record_index = selection.record_index.map(remap);
            }
        }
    }
    if online.is_some() {
        // The same slot now has local briefing/preview data, or subsequent
        // slots shifted. The enlarged preview shares this index-keyed texture.
        frontend.skirmish_preview_texture = None;
    }
    if carried_star {
        save_favorites(platform, frontend);
    }
    // A room transfer can supply the bytes while the player is confirming
    // the download or waiting for it. Both prompts can now use the local row.
    if prompted
        && let Some(prompt) = frontend
            .skirmish_shell_state
            .choose_map_modal
            .as_mut()
            .and_then(|modal| modal.message_box.as_mut())
    {
        prompt.kind = ChooseMapPromptKind::Downloaded;
        prompt.selection.record_index = Some(index);
        cancel_download(frontend);
    }
    refresh_choose_map(platform, renderer, frontend);
    Ok((kept.path, index))
}

/// The local row after a keep, and any superseded Online row that was removed.
fn list_kept_file(
    records: &mut Vec<SkirmishScenarioRecord>,
    online: Option<usize>,
    kept: &KeptMap,
    assets: Option<&crate::assets::asset_manager::AssetManager>,
) -> Result<(usize, Option<usize>)> {
    if let Some(index) = local_row(records, &kept.name) {
        if let Some(online) = online {
            records.remove(online);
            return Ok((index - usize::from(online < index), Some(online)));
        }
        return Ok((index, None));
    }
    let index = online.unwrap_or(records.len());
    let ordinal = online.map_or(index, |index| records[index].source_ordinal);
    let record = crate::map::scenario_sources::loose_map_record(
        &kept.path,
        kept.name.clone(),
        ordinal,
        assets,
    )
    .context("the kept map does not read as a map")?;
    if online.is_some() {
        records[index] = record;
    } else {
        records.push(record);
    }
    Ok((index, None))
}

/// Reads the catalog in the background.
fn start_catalog(platform: &crate::app::PlatformState) -> Option<Receiver<Result<Vec<OnlineMap>>>> {
    let cache = platform
        .game_config
        .as_ref()
        .map(|config| config.paths.settings_dir().to_path_buf())?;
    crate::app::worker::spawn(
        "vera-map-library",
        Arc::new(AtomicBool::new(false)),
        move |_| {
            let body = fetch::catalog(&cache)?;
            // A damaged cached copy is read again in full next time.
            read_catalog(&body).inspect_err(|_| fetch::forget(&cache))
        },
    )
}

#[derive(serde::Deserialize)]
struct CatalogBody {
    maps: Vec<CatalogEntry>,
}

#[derive(serde::Deserialize)]
struct CatalogEntry {
    sha256: String,
    name: String,
    file_name: String,
    byte_length: usize,
    /// Base64 of the map's chooser sections.
    header: String,
}

/// The catalog's maps that read as Choose Map rows. Entries with a bad
/// identity or file name, retail maps and unreadable headers are skipped.
fn read_catalog(body: &[u8]) -> Result<Vec<OnlineMap>> {
    use base64::Engine;
    let catalog: CatalogBody = serde_json::from_slice(body).context("unreadable map catalog")?;
    let limit = crate::map::source::TransferResource::Map.limit();
    Ok(catalog
        .maps
        .into_iter()
        .filter_map(|entry| {
            if !crate::map::source::valid_source_sha256(&entry.sha256)
                || !(1..=limit).contains(&entry.byte_length)
                || entry.file_name.contains(['/', '\\'])
                || !crate::map::source::is_custom_map_name(Path::new(&entry.file_name))
            {
                return None;
            }
            let file_name = crate::map::source::kept_map_file_name(&entry.file_name).ok()?;
            let header = base64::engine::general_purpose::STANDARD
                .decode(&entry.header)
                .ok()?;
            let ini = crate::rules::ini_parser::IniFile::from_bytes(&header).ok()?;
            let record = SkirmishScenarioRecord::concrete_from_ini(
                0,
                SkirmishScenarioSource::Online(entry.sha256.clone()),
                &file_name,
                &ini,
            );
            (!record.official && record.kind == SkirmishScenarioKind::ConcreteMap).then(|| {
                OnlineMap {
                    sha256: entry.sha256,
                    name: entry.name,
                    file_name,
                    byte_length: entry.byte_length,
                    record,
                    header: header.into(),
                }
            })
        })
        .collect())
}

mod fetch {
    //! The library's HTTPS reads, on worker threads only.

    use std::io::Read;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use anyhow::{Context, Result, bail, ensure};

    use crate::app::shell_wol::lobby_get;

    /// A curated library of a few hundred maps is a few MB.
    const MAX_CATALOG_BYTES: u64 = 32 * 1024 * 1024;
    /// The last catalog and its ETag, so an unchanged one is not sent again.
    const CACHE_FILE: &str = "MapLibrary.json";
    const CACHE_TAG_FILE: &str = "MapLibrary.etag";

    pub(super) fn forget(cache: &Path) {
        let _ = std::fs::remove_file(cache.join(CACHE_TAG_FILE));
        let _ = std::fs::remove_file(cache.join(CACHE_FILE));
    }

    /// `GET /v1/library/catalog`, revalidated against the cached copy.
    pub(super) fn catalog(cache: &Path) -> Result<Vec<u8>> {
        const PATH: &str = "/v1/library/catalog";
        let cached_tag = std::fs::read_to_string(cache.join(CACHE_TAG_FILE)).ok();
        let cached = std::fs::read(cache.join(CACHE_FILE)).ok();
        let tag = cached_tag.as_deref().filter(|_| cached.is_some());
        let answer = lobby_get(PATH, tag.map(str::trim))?;
        match answer.status {
            304 => cached.context("the cached catalog is gone"),
            200 => {
                let body = read_limited(answer.body, MAX_CATALOG_BYTES, None)?;
                if let Some(tag) = answer.etag {
                    // A failed write only costs the next run a full read.
                    let _ = std::fs::create_dir_all(cache)
                        .and_then(|()| std::fs::write(cache.join(CACHE_FILE), &body))
                        .and_then(|()| std::fs::write(cache.join(CACHE_TAG_FILE), tag));
                }
                Ok(body)
            }
            status => bail!("{PATH} answered {status}"),
        }
    }

    /// Optional presentation metadata from the owner of the website gallery.
    pub(super) fn categories() -> Result<std::collections::HashMap<String, Vec<String>>> {
        #[derive(serde::Deserialize)]
        struct Feed {
            categories: std::collections::HashMap<String, Vec<String>>,
        }
        let Some(answer) = crate::app::shell_wol::gallery_categories_get()? else {
            return Ok(Default::default());
        };
        ensure!(
            answer.status == 200,
            "the map category feed answered {}",
            answer.status
        );
        let bytes = read_limited(answer.body, 1024 * 1024, None)?;
        let feed: Feed = serde_json::from_slice(&bytes).context("unreadable map categories")?;
        Ok(feed.categories)
    }

    /// `GET /v1/library/maps/<sha256>`: the whole map, checked against its
    /// catalog length and SHA-256.
    pub(super) fn map(
        sha256: &str,
        length: usize,
        cancel: &AtomicBool,
        received: &AtomicUsize,
    ) -> Result<Vec<u8>> {
        let path = format!("/v1/library/maps/{sha256}");
        let answer = lobby_get(&path, None)?;
        ensure!(answer.status == 200, "{path} answered {}", answer.status);
        let bytes = read_limited(answer.body, length as u64, Some((cancel, received)))?;
        ensure!(
            bytes.len() == length && crate::util::sha256::sha256_hex(&bytes) == sha256,
            "the downloaded map differs from the library's"
        );
        Ok(bytes)
    }

    /// Reads at most `limit` bytes; a map download can be cancelled and
    /// counts what has arrived.
    fn read_limited(
        mut source: impl Read,
        limit: u64,
        download: Option<(&AtomicBool, &AtomicUsize)>,
    ) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        let mut buffer = vec![0; 64 * 1024];
        loop {
            if download.is_some_and(|(cancel, _)| cancel.load(Ordering::Relaxed)) {
                bail!("the download was cancelled");
            }
            let count = source.read(&mut buffer)?;
            if count == 0 {
                return Ok(bytes);
            }
            bytes.extend_from_slice(&buffer[..count]);
            ensure!(bytes.len() as u64 <= limit, "the answer is too large");
            if let Some((_, received)) = download {
                received.store(bytes.len(), Ordering::Relaxed);
            }
        }
    }

    #[cfg(all(test, tests_app))]
    mod tests {
        use super::*;

        #[test]
        fn a_map_download_counts_what_has_arrived() {
            let cancel = AtomicBool::new(false);
            let received = AtomicUsize::new(0);
            let body = vec![7; 70_000];
            let read = read_limited(&body[..], 70_000, Some((&cancel, &received))).unwrap();
            assert_eq!(read.len(), 70_000);
            assert_eq!(received.load(Ordering::Relaxed), 70_000);
        }
    }
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use base64::Engine;

    fn entry(sha: char, file_name: &str, header: &str) -> serde_json::Value {
        serde_json::json!({
            "sha256": sha.to_string().repeat(64),
            "name": "Frozen Lakes",
            "file_name": file_name,
            "players": 4,
            "byte_length": 1000,
            "header": base64::engine::general_purpose::STANDARD.encode(header),
        })
    }

    #[test]
    fn the_catalog_reads_each_header_as_a_choose_map_row_and_skips_retail_or_unsafe_entries() {
        let header = "[Basic]\nName=Frozen Lakes\n[Waypoints]\n0=1\n1=2\n2=3\n3=4\n";
        let body = serde_json::json!({ "revision": 3, "maps": [
            entry('a', "Frozen_Lakes.yrm", header),
            entry('b', "Retail.yrm", "[Basic]\nName=Retail\nOfficial=yes\n"),
            entry('c', "../Escape.yrm", header),
            entry('d', "Frozen_Lakes.MAP", header),
            entry('f', "Coast.MpR", header),
            entry('0', "Mixed.YrM", header),
            entry('1', r"Maps\Escape.map", header),
            entry('2', "Escape.exe", header),
            entry('e', "con.yrm", header),
        ]});
        let maps = read_catalog(body.to_string().as_bytes()).unwrap();
        assert_eq!(
            maps.iter()
                .map(|map| map.file_name.as_str())
                .collect::<Vec<_>>(),
            [
                "Frozen_Lakes.map",
                "Frozen_Lakes.map",
                "Coast.map",
                "Mixed.map"
            ]
        );
        let record = &maps[0].record;
        assert_eq!(
            record.source,
            SkirmishScenarioSource::Online("a".repeat(64))
        );
        assert_eq!(record.display_name, "Frozen Lakes");
        assert_eq!(record.player_capacity, 4);
    }

    #[test]
    fn a_failed_catalog_read_tries_once_more_in_the_visit() {
        assert!(matches!(
            Phase::<()>::failed(false),
            Phase::Failed { retry_at: Some(_) }
        ));
        assert!(matches!(
            Phase::<()>::failed(true),
            Phase::Failed { retry_at: None }
        ));
    }

    #[test]
    fn an_online_row_is_added_for_each_map_the_player_does_not_have() {
        let dir = std::env::temp_dir().join(format!("vera-map-library-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Another map under Frozen Lakes' name, and Twin Rivers itself.
        std::fs::write(dir.join("frozen_lakes.MAP"), b"[Basic]\nName=Mine\n").unwrap();
        let twin = b"[Basic]\nName=Twin Rivers\nGameMode=navalwar\n[Map]\nTheater=TEMPERATE\n";
        std::fs::write(dir.join("Renamed.MPR"), twin).unwrap();
        let mut have = entry('b', "Twin_Rivers.yrm", "[Basic]\nName=Twin Rivers\n");
        have["sha256"] = crate::util::sha256::sha256_hex(twin).into();
        have["byte_length"] = twin.len().into();
        let body = serde_json::json!({ "maps": [
            entry('a', "Frozen_Lakes.yrm", "[Basic]\nName=Frozen Lakes\n"),
            have,
            entry('c', "Arctic.yrm", "[Basic]\nName=Arctic\n"),
        ]});
        let maps = read_catalog(body.to_string().as_bytes()).unwrap();
        // A row that is no file, as a MIX map's is, holds Arctic's name.
        let mut records = vec![maps[0].record.clone()];
        records[0].source = SkirmishScenarioSource::Synthetic;
        records[0].file_name = "Arctic.map".into();
        let mut favorites = BTreeSet::from([maps[1].sha256.clone()]);
        let mut folders = OnlineMapFolders::default();
        assert!(append_rows(
            &mut records,
            &maps,
            &[&dir],
            &mut favorites,
            &mut folders,
            None
        ));
        assert_eq!(favorites, BTreeSet::from(["renamed.mpr".to_string()]));
        assert!(folders.includes(&records[2], None));
        folders.set_memberships(HashMap::from([(
            maps[1].sha256.clone(),
            vec!["navalwar".into()],
        )]));
        assert!(folders.includes(&records[2], Some("navalwar")));
        assert!(!folders.includes(&records[0], Some("navalwar")));
        assert_eq!(records[2].game_modes, ["navalwar"]);
        std::fs::remove_dir_all(&dir).unwrap();
        let rows: Vec<_> = records[1..]
            .iter()
            .map(|record| (record.display_name.as_str(), record.file_name.as_str()))
            .collect();
        assert_eq!(
            rows,
            [
                ("Frozen Lakes", "Frozen_Lakes-aaaaaaaa.map"),
                ("Twin Rivers", "Renamed.MPR"),
                ("Arctic", "Arctic-cccccccc.map"),
            ]
        );
        assert_eq!(records[1].source_ordinal, 1);
    }

    #[test]
    fn a_client_catalog_only_adopts_an_unlisted_local_map_on_an_explicit_keep() {
        use crate::assets::{asset_manager::AssetManager, client_install::ClientInstall};
        use crate::util::{mod_definition::ResourceConfig, mod_package::ModFolder};

        let dir = crate::map::source::test_support::TestDirectory::new("client-library");
        let map = "[Basic]\nName=Shore\nGameMode=navalwar\n[Map]\nTheater=TEMPERATE\n";
        dir.write("My Shore.map", map.as_bytes());
        let mut offered = entry('a', "Shore.yrm", map);
        offered["sha256"] = crate::util::sha256::sha256_hex(map.as_bytes()).into();
        offered["byte_length"] = map.len().into();
        let body = serde_json::json!({ "maps": [offered] });
        let maps = read_catalog(body.to_string().as_bytes()).unwrap();
        let assets =
            AssetManager::from_loose_root_for_test(dir.path()).with_mod_sources(&ResourceConfig {
                mod_folder: Some(ModFolder::loose(dir.path().to_path_buf())),
                client_install: Some(Arc::new(ClientInstall {
                    maps: vec![],
                    modes: vec![],
                    theme: Default::default(),
                })),
                ..Default::default()
            });
        let mut records = vec![];
        append_rows(
            &mut records,
            &maps,
            &[dir.path()],
            &mut BTreeSet::new(),
            &mut OnlineMapFolders::default(),
            Some(&assets),
        );
        assert_eq!(records.len(), 1);
        assert!(records[0].is_online());
        let kept = KeptMap {
            path: dir.path().join("My Shore.map"),
            name: "My Shore.map".into(),
        };
        assert_eq!(
            list_kept_file(&mut records, Some(0), &kept, Some(&assets)).unwrap(),
            (0, None)
        );
        assert!(!records[0].is_online());
        assert_eq!(records[0].file_name, "My Shore.map");
    }

    #[test]
    fn a_download_selects_the_existing_loose_row_if_its_copy_arrived_under_another_name() {
        let dir = crate::map::source::test_support::TestDirectory::new("download-row");
        let map = "[Basic]\nName=Shore\nGameMode=navalwar\n[Map]\nTheater=TEMPERATE\n";
        dir.write("My Shore.mpr", map.as_bytes());
        let mut offered = entry('a', "Shore.yrm", map);
        offered["sha256"] = crate::util::sha256::sha256_hex(map.as_bytes()).into();
        let body = serde_json::json!({ "maps": [offered] });
        let online = read_catalog(body.to_string().as_bytes()).unwrap().remove(0);
        let kept = KeptMap {
            path: dir.path().join("My Shore.mpr"),
            name: "My Shore.mpr".into(),
        };
        let local =
            crate::map::scenario_sources::loose_map_record(&kept.path, kept.name.clone(), 1, None)
                .unwrap();
        let mut records = vec![online.record, local];
        let (selected, removed) = list_kept_file(&mut records, Some(0), &kept, None).unwrap();
        assert_eq!((selected, removed, records.len()), (0, Some(0), 1));
        assert_eq!(records[selected].file_name, "My Shore.mpr");
        assert_eq!(records[selected].game_modes, ["navalwar"]);
    }
}
