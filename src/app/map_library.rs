//! The online map library (docs/lobby-protocol.md): in an online room,
//! Choose Map lists the lobby's curated maps under its Online Maps row, and
//! Use Map on one asks to download it. The download is saved in VERA's own
//! Maps folder (never the game folder) as a loose `.yrm`, where it is an
//! ordinary map from then on, in every game type's list. Players star maps they like; the stars are kept in
//! `MapFavorites.txt` in the settings folder. VERA-only; gamemd has no map
//! library.
//!
//! The catalog is read once a run, in the background, when the game reaches
//! its menus. Each entry carries the map's own `[Basic]`, `[Preview]`,
//! `[PreviewPack]`, `[Waypoints]`, `[Header]` and `[RandomMap]` sections, so
//! its row is read by the same record reader as a local map's.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use anyhow::{Context, Result, ensure};

use super::state::AppState;
use crate::map::skirmish_scenarios::{
    SkirmishScenarioKind, SkirmishScenarioRecord, SkirmishScenarioSource,
};
use crate::ui::skirmish_shell::{ChooseMapPrompt, ChooseMapPromptKind, ChooseMapSelection};
use crate::util::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// One map the library offers.
#[derive(Debug, Clone)]
pub(crate) struct OnlineMap {
    pub(crate) sha256: String,
    /// `[Basic] Name=`.
    pub(crate) name: String,
    /// The `.yrm` name it is saved under.
    pub(crate) file_name: String,
    pub(crate) byte_length: usize,
    /// Its Choose Map row, read from the header (source ordinal unset).
    record: SkirmishScenarioRecord,
}

#[derive(Default)]
enum Phase {
    #[default]
    NotStarted,
    Fetching(Receiver<Result<Vec<OnlineMap>>>),
    Ready,
}

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
    phase: Phase,
    maps: Vec<OnlineMap>,
    download: Option<Download>,
    /// Starred maps by SHA-256, read when the catalog read starts.
    favorites: std::collections::BTreeSet<String>,
}

const FAVORITES_FILE: &str = "MapFavorites.txt";

impl MapLibrary {
    pub(crate) fn favorites(&self) -> &std::collections::BTreeSet<String> {
        &self.favorites
    }

    /// The library entry behind an Online Choose Map row.
    pub(crate) fn online_map(&self, record: &SkirmishScenarioRecord) -> Option<&OnlineMap> {
        let SkirmishScenarioSource::Online(sha256) = &record.source else {
            return None;
        };
        self.maps.iter().find(|map| &map.sha256 == sha256)
    }
}

/// Starts the catalog read, then takes the catalog and a finished download.
pub(crate) fn poll(state: &mut AppState) -> Option<Instant> {
    match std::mem::take(&mut state.frontend.map_library.phase) {
        Phase::NotStarted => {
            // A launcher-started match has no Choose Map to fill.
            let phase = if state.frontend.external_launch {
                Phase::Ready
            } else {
                state.frontend.map_library.favorites = read_favorites(state);
                start_catalog(state).map_or(Phase::Ready, Phase::Fetching)
            };
            state.frontend.map_library.phase = phase;
        }
        Phase::Fetching(result) => {
            let (phase, read) = take_catalog(result);
            state.frontend.map_library.phase = phase;
            match read {
                Some(Ok(maps)) => {
                    log::info!("The online map library lists {} maps", maps.len());
                    state.frontend.map_library.maps = maps;
                    append_online_records(state);
                }
                Some(Err(error)) => log::info!("The online map library is unavailable: {error:#}"),
                None => {}
            }
        }
        Phase::Ready => state.frontend.map_library.phase = Phase::Ready,
    }
    let library = &mut state.frontend.map_library;
    if let Some(download) = library.download.as_ref() {
        match download.result.try_recv() {
            Ok(result) => {
                let sha256 = library.download.take().map(|done| done.sha256);
                finish_download(state, sha256.unwrap_or_default(), result);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                let sha256 = library.download.take().map(|done| done.sha256);
                finish_download(
                    state,
                    sha256.unwrap_or_default(),
                    Err(anyhow::anyhow!("the download stopped")),
                );
            }
        }
    }
    if state.frontend.map_library.download.is_some() {
        // The download box's bar follows the bytes as they arrive.
        state.platform.window.request_redraw();
    }
    let library = &state.frontend.map_library;
    (matches!(library.phase, Phase::NotStarted | Phase::Fetching(_)) || library.download.is_some())
        .then(|| Instant::now() + POLL_INTERVAL)
}

/// One look at a running catalog read: the phase after it, and the read's
/// result once it ended. Only an unanswered read stays Fetching; every other
/// outcome is final for this run, so the catalog is read once.
fn take_catalog(
    result: Receiver<Result<Vec<OnlineMap>>>,
) -> (Phase, Option<Result<Vec<OnlineMap>>>) {
    match result.try_recv() {
        Ok(read) => (Phase::Ready, Some(read)),
        Err(TryRecvError::Empty) => (Phase::Fetching(result), None),
        Err(TryRecvError::Disconnected) => (Phase::Ready, None),
    }
}

/// Adds a Choose Map row for each library map that neither the game folder
/// nor VERA's maps folder holds a file of that name for. Also run after the
/// catalog is rebuilt.
pub(crate) fn append_online_records(state: &mut AppState) {
    let Some((ra2_dir, maps_dir)) = state.platform.game_config.as_ref().map(|config| {
        (
            config.paths.ra2_dir.clone(),
            config.paths.downloaded_maps_dir(),
        )
    }) else {
        return;
    };
    let maps = &state.frontend.map_library.maps;
    if maps.is_empty() {
        return;
    }
    append_rows(
        &mut state.frontend.scenario_catalog.records_mut(),
        maps,
        &[&ra2_dir, &maps_dir],
    );
}

fn append_rows(
    records: &mut Vec<SkirmishScenarioRecord>,
    maps: &[OnlineMap],
    folders: &[&std::path::Path],
) {
    // Each folder is listed once; names compare without case, as `find` does.
    let listed: Vec<std::path::PathBuf> = folders
        .iter()
        .flat_map(|folder| {
            crate::util::case_insensitive_path::native_listing(folder).unwrap_or_default()
        })
        .collect();
    let mut taken: std::collections::HashSet<String> = listed
        .iter()
        .filter_map(|path| path.file_name()?.to_str())
        .chain(records.iter().map(|record| record.file_name.as_str()))
        .map(str::to_ascii_lowercase)
        .collect();
    for map in maps {
        if !taken.insert(map.file_name.to_ascii_lowercase()) {
            continue;
        }
        let mut record = map.record.clone();
        record.source_ordinal = records.len();
        records.push(record);
    }
}

/// The running download's bytes received and its length.
pub(crate) fn download_progress(state: &AppState) -> Option<(u64, u64)> {
    state
        .frontend
        .map_library
        .download
        .as_ref()
        .map(|download| {
            let received = download.received.load(Ordering::Relaxed);
            (received as u64, download.length as u64)
        })
}

/// Whether this Choose Map shows the Online Maps row: online rooms only.
pub(crate) fn listed_in_choose_map(state: &AppState) -> bool {
    state.frontend.shell_route.wol()
}

/// Star or unstar a map, and keep the stars for later runs.
pub(crate) fn set_favorite(state: &mut AppState, sha256: String, starred: bool) {
    let favorites = &mut state.frontend.map_library.favorites;
    if starred {
        favorites.insert(sha256);
    } else {
        favorites.remove(&sha256);
    }
    let Some(path) = favorites_path(state) else {
        return;
    };
    let text: String = state
        .frontend
        .map_library
        .favorites
        .iter()
        .map(|sha| format!("{sha}\n"))
        .collect();
    if let Err(error) = std::fs::create_dir_all(path.parent().unwrap_or(&path))
        .and_then(|()| std::fs::write(&path, text))
    {
        log::warn!("Could not save the map favorites: {error}");
    }
}

fn favorites_path(state: &AppState) -> Option<std::path::PathBuf> {
    state
        .platform
        .game_config
        .as_ref()
        .map(|config| config.paths.settings_dir().join(FAVORITES_FILE))
}

/// The stars from earlier runs; a missing or unreadable file stars nothing.
fn read_favorites(state: &AppState) -> std::collections::BTreeSet<String> {
    favorites_path(state)
        .and_then(|path| crate::util::file_system::read(path).ok())
        .map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .lines()
                .map(str::trim)
                .filter(|sha| crate::map::source::valid_source_sha256(sha))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Use Map on an Online row: ask to download it first. False for a map
/// already saved.
pub(crate) fn prompt_download(state: &mut AppState, selection: ChooseMapSelection) -> bool {
    let online = selection
        .record_index
        .and_then(|index| state.frontend.scenario_catalog.records().get(index))
        .is_some_and(|record| state.frontend.map_library.online_map(record).is_some());
    if !online {
        return false;
    }
    set_prompt(state, selection, ChooseMapPromptKind::Download);
    true
}

/// Download in the box: fetch the map while the box waits.
pub(crate) fn start_download(state: &mut AppState, selection: ChooseMapSelection) {
    let Some(map) = selection
        .record_index
        .and_then(|index| state.frontend.scenario_catalog.records().get(index))
        .and_then(|record| state.frontend.map_library.online_map(record))
        .cloned()
    else {
        return;
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let received = Arc::new(AtomicUsize::new(0));
    let sha256 = map.sha256.clone();
    let length = map.byte_length;
    let counter = Arc::clone(&received);
    let Some(result) = spawn("vera-map-download", Arc::clone(&cancel), move |cancel| {
        fetch::map(&map.sha256, map.byte_length, cancel, &counter)
    }) else {
        set_prompt(state, selection, ChooseMapPromptKind::DownloadFailed);
        return;
    };
    cancel_download(state);
    state.frontend.map_library.download = Some(Download {
        sha256,
        result,
        cancel,
        received,
        length,
    });
    set_prompt(state, selection, ChooseMapPromptKind::Downloading);
}

/// Cancel in the box, or the box closing: drop a running download.
pub(crate) fn cancel_download(state: &mut AppState) {
    if let Some(download) = state.frontend.map_library.download.take() {
        download.cancel.store(true, Ordering::Relaxed);
    }
}

fn selection_sha256(state: &AppState, selection: ChooseMapSelection) -> Option<String> {
    let record = state
        .frontend
        .scenario_catalog
        .records()
        .get(selection.record_index?)?;
    match &record.source {
        SkirmishScenarioSource::Online(sha256) => Some(sha256.clone()),
        _ => None,
    }
}

fn set_prompt(state: &mut AppState, selection: ChooseMapSelection, kind: ChooseMapPromptKind) {
    if let Some(modal) = state
        .frontend
        .skirmish_shell_state
        .choose_map_modal
        .as_mut()
    {
        modal.message_box = Some(ChooseMapPrompt {
            selection,
            pressed: None,
            kind,
        });
    }
    state.platform.window.request_redraw();
}

/// A download ended: keep the map and go on with Use Map, or say it failed.
fn finish_download(state: &mut AppState, sha256: String, result: Result<Vec<u8>>) {
    // The box this download belongs to, unless the player closed it.
    let Some(prompt) = state
        .frontend
        .skirmish_shell_state
        .choose_map_modal
        .as_ref()
        .and_then(|modal| modal.message_box)
        .filter(|prompt| prompt.kind == ChooseMapPromptKind::Downloading)
        .filter(|prompt| {
            selection_sha256(state, prompt.selection).as_deref() == Some(sha256.as_str())
        })
    else {
        return;
    };
    let selection = prompt.selection;
    match result.and_then(|bytes| keep_download(state, selection, &bytes)) {
        Ok(()) => {
            if let Some(modal) = state
                .frontend
                .skirmish_shell_state
                .choose_map_modal
                .as_mut()
            {
                modal.message_box = None;
            }
            // The rest of Use Map, as for a map that was already here.
            if super::App::prompt_choose_map_eject(state, selection) {
                return;
            }
            super::App::leave_shell_dialog(
                state,
                crate::app::frontend::shell_transition::ShellExitThen::ChooseMapUse(selection),
            );
        }
        Err(error) => {
            log::warn!("Could not download the online map {sha256}: {error:#}");
            set_prompt(state, selection, ChooseMapPromptKind::DownloadFailed);
        }
    }
}

/// Saves a downloaded map in VERA's maps folder and turns its Online row
/// into the ordinary row of that file.
fn keep_download(state: &mut AppState, selection: ChooseMapSelection, bytes: &[u8]) -> Result<()> {
    let (ra2_dir, maps_dir) = state
        .platform
        .game_config
        .as_ref()
        .map(|config| {
            (
                config.paths.ra2_dir.clone(),
                config.paths.downloaded_maps_dir(),
            )
        })
        .context("no game folder")?;
    let index = selection.record_index.context("no map selected")?;
    let map = state
        .frontend
        .scenario_catalog
        .records()
        .get(index)
        .and_then(|record| state.frontend.map_library.online_map(record))
        .cloned()
        .context("the map left the library")?;
    let path =
        match crate::map::source::keep_received_map(&maps_dir, &ra2_dir, &map.file_name, bytes)? {
            Some(name) => maps_dir.join(name),
            // A file of that name arrived meanwhile: use it only if it is this map.
            None => {
                let path = crate::map::source::local_map_path(&ra2_dir, &map.file_name)
                    .context("the saved map is gone")?;
                let existing = crate::util::file_system::read(&path)?;
                ensure!(
                    crate::util::sha256::sha256_hex(&existing) == map.sha256,
                    "another map named {} is already saved",
                    map.file_name
                );
                path
            }
        };
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("the saved map has no file name")?
        .to_owned();
    let mut records = state.frontend.scenario_catalog.records_mut();
    let ordinal = records[index].source_ordinal;
    let record = crate::map::scenario_sources::loose_yrm_record(&path, name.clone(), ordinal)
        .context("the saved map does not read as a map")?;
    records[index] = record;
    log::info!("Downloaded the online map {} as {name}", map.name);
    Ok(())
}

/// Reads the catalog in the background.
fn start_catalog(state: &AppState) -> Option<Receiver<Result<Vec<OnlineMap>>>> {
    let cache = state
        .platform
        .game_config
        .as_ref()
        .map(|config| config.paths.settings_dir().to_path_buf())?;
    spawn(
        "vera-map-library",
        Arc::new(AtomicBool::new(false)),
        move |_| {
            let body = fetch::catalog(&cache)?;
            // A damaged cached copy is read again in full next time.
            read_catalog(&body).inspect_err(|_| fetch::forget(&cache))
        },
    )
}

/// One worker thread whose result arrives on the receiver. `None` where
/// threads cannot start, as in the browser build, which has no library yet.
fn spawn<T: Send + 'static>(
    name: &str,
    cancel: Arc<AtomicBool>,
    work: impl FnOnce(&AtomicBool) -> Result<T> + Send + 'static,
) -> Option<Receiver<Result<T>>> {
    if cfg!(all(target_arch = "wasm32", target_os = "unknown")) {
        return None;
    }
    let (output, input) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let _ = output.send(work(&cancel));
        })
        .ok()?;
    Some(input)
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
            // Only names the download can be saved under exactly.
            let named = crate::map::source::valid_source_sha256(&entry.sha256)
                && crate::map::source::kept_map_file_name(&entry.file_name)
                    .is_ok_and(|name| name == entry.file_name)
                && (1..=limit).contains(&entry.byte_length);
            if !named {
                return None;
            }
            let header = base64::engine::general_purpose::STANDARD
                .decode(&entry.header)
                .ok()?;
            let ini = crate::rules::ini_parser::IniFile::from_bytes(&header).ok()?;
            let record = SkirmishScenarioRecord::concrete_from_ini(
                0,
                SkirmishScenarioSource::Online(entry.sha256.clone()),
                &entry.file_name,
                &ini,
            );
            (!record.official && record.kind == SkirmishScenarioKind::ConcreteMap).then(|| {
                OnlineMap {
                    sha256: entry.sha256,
                    name: entry.name,
                    file_name: entry.file_name,
                    byte_length: entry.byte_length,
                    record,
                }
            })
        })
        .collect())
}

#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
mod fetch {
    //! The library's HTTPS reads, on worker threads only.

    use std::io::Read;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use anyhow::{Context, Result, bail, ensure};

    use std::time::Duration;

    const USER_AGENT: &str = concat!("VERA20k/", env!("CARGO_PKG_VERSION"), " map-library");
    /// A curated library of a few hundred maps is a few MB.
    const MAX_CATALOG_BYTES: u64 = 32 * 1024 * 1024;
    /// The last catalog and its ETag, so an unchanged one is not sent again.
    const CACHE_FILE: &str = "MapLibrary.json";
    const CACHE_TAG_FILE: &str = "MapLibrary.etag";

    pub(super) fn forget(cache: &Path) {
        let _ = std::fs::remove_file(cache.join(CACHE_TAG_FILE));
        let _ = std::fs::remove_file(cache.join(CACHE_FILE));
    }

    fn agent(base: &str) -> ureq::Agent {
        ureq::Agent::config_builder()
            .https_only(base.starts_with("https://"))
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .timeout_per_read(Some(Duration::from_secs(30)))
            .build()
            .into()
    }

    fn base() -> Result<String> {
        Ok(crate::app::shell_wol::lobby_base_url()?
            .trim_end_matches('/')
            .to_owned())
    }

    /// `GET /v1/library/catalog`, revalidated against the cached copy.
    pub(super) fn catalog(cache: &Path) -> Result<Vec<u8>> {
        let base = base()?;
        let url = format!("{base}/v1/library/catalog");
        let cached_tag = std::fs::read_to_string(cache.join(CACHE_TAG_FILE)).ok();
        let cached = std::fs::read(cache.join(CACHE_FILE)).ok();
        let mut request = agent(&base).get(&url).header("User-Agent", USER_AGENT);
        if let (Some(tag), Some(_)) = (&cached_tag, &cached) {
            request = request.header("If-None-Match", tag.trim());
        }
        let mut response = request
            .call()
            .with_context(|| format!("cannot reach {url}"))?;
        match response.status().as_u16() {
            304 => cached.context("the cached catalog is gone"),
            200 => {
                let body = read_limited(response.body_mut().as_reader(), MAX_CATALOG_BYTES, None)?;
                if let Some(tag) = response
                    .headers()
                    .get("ETag")
                    .and_then(|tag| tag.to_str().ok())
                {
                    // A failed write only costs the next run a full read.
                    let _ = std::fs::create_dir_all(cache)
                        .and_then(|()| std::fs::write(cache.join(CACHE_FILE), &body))
                        .and_then(|()| std::fs::write(cache.join(CACHE_TAG_FILE), tag));
                }
                Ok(body)
            }
            status => bail!("{url} answered {status}"),
        }
    }

    /// `GET /v1/library/maps/<sha256>`: the whole map, checked against its
    /// catalog length and SHA-256.
    pub(super) fn map(
        sha256: &str,
        length: usize,
        cancel: &AtomicBool,
        received: &AtomicUsize,
    ) -> Result<Vec<u8>> {
        let base = base()?;
        let url = format!("{base}/v1/library/maps/{sha256}");
        let mut response = agent(&base)
            .get(&url)
            .header("User-Agent", USER_AGENT)
            .call()
            .with_context(|| format!("cannot reach {url}"))?;
        ensure!(
            response.status() == 200,
            "{url} answered {}",
            response.status()
        );
        let bytes = read_limited(
            response.body_mut().as_reader(),
            length as u64,
            Some((cancel, received)),
        )?;
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

    #[cfg(test)]
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

/// The browser build has no map library yet.
#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
mod fetch {
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicUsize};

    use anyhow::{Result, bail};

    pub(super) fn catalog(_cache: &Path) -> Result<Vec<u8>> {
        bail!("the browser build has no map library")
    }

    pub(super) fn forget(_cache: &Path) {}

    pub(super) fn map(
        _sha256: &str,
        _length: usize,
        _cancel: &AtomicBool,
        _received: &AtomicUsize,
    ) -> Result<Vec<u8>> {
        bail!("the browser build has no map library")
    }
}

#[cfg(test)]
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
            entry('d', "Frozen_Lakes.map", header),
            entry('e', "con.yrm", header),
        ]});
        let maps = read_catalog(body.to_string().as_bytes()).unwrap();
        assert_eq!(maps.len(), 1);
        let record = &maps[0].record;
        assert_eq!(
            record.source,
            SkirmishScenarioSource::Online("a".repeat(64))
        );
        assert_eq!(record.display_name, "Frozen Lakes");
        assert_eq!(record.player_capacity, 4);
        assert_eq!(maps[0].file_name, "Frozen_Lakes.yrm");
    }

    #[test]
    fn a_finished_or_failed_catalog_read_is_not_started_again() {
        let (send, read) = std::sync::mpsc::channel();
        let (phase, result) = take_catalog(read);
        assert!(matches!(phase, Phase::Fetching(_)) && result.is_none());
        let Phase::Fetching(read) = phase else {
            unreachable!()
        };
        send.send(Err(anyhow::anyhow!("answered 404"))).unwrap();
        let (phase, result) = take_catalog(read);
        assert!(matches!(phase, Phase::Ready) && result.is_some_and(|read| read.is_err()));
        let (send, read) = std::sync::mpsc::channel::<Result<Vec<OnlineMap>>>();
        drop(send);
        assert!(matches!(take_catalog(read).0, Phase::Ready));
    }

    #[test]
    fn an_online_row_is_added_only_for_a_map_the_game_folder_does_not_hold() {
        let dir = std::env::temp_dir().join(format!("vera-map-library-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("frozen_lakes.YRM"), b"[Basic]\n").unwrap();
        let header = "[Basic]\nName=Frozen Lakes\n";
        let body = serde_json::json!({ "maps": [
            entry('a', "Frozen_Lakes.yrm", header),
            entry('b', "Twin_Rivers.yrm", "[Basic]\nName=Twin Rivers\n"),
        ]});
        let maps = read_catalog(body.to_string().as_bytes()).unwrap();
        let mut records = vec![maps[0].record.clone()];
        records[0].source = SkirmishScenarioSource::Synthetic;
        records[0].file_name = "Other.yrm".into();
        append_rows(&mut records, &maps, &[&dir]);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[1].display_name, "Twin Rivers");
        assert_eq!(records[1].source_ordinal, 1);
    }
}
