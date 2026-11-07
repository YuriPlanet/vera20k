//! The menus' side of HD map previews: a shown map's kept picture, and the
//! one hidden run (`--map-picture`) that draws a missing one. Runs go one at
//! a time, only while the menus show with HD Map Previews on; while one
//! runs, the newest map shown waits for the next.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::{Context, Result};

use super::MapPictureRequest;
use crate::app::AppState;
use crate::map::preview::DecodedPreview;
use crate::map::skirmish_scenarios::SkirmishScenarioSource;
use crate::ui::game_screen::GameScreen;
use crate::util::time::{Duration, Instant};

/// A run longer than this is stopped; the run itself gives up sooner.
const RUN_TIMEOUT: Duration = Duration::from_secs(180);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
/// How long a quitting game waits for its run to end: on Windows a running
/// copy keeps the executable from being replaced by an update.
const EXIT_WAIT: Duration = Duration::from_secs(1);

/// What the menus know about map pictures during this run of the game.
#[derive(Default)]
pub(crate) struct MapPictures {
    /// Each shown map's SHA-256, by its name in lower case.
    identities: HashMap<String, Identity>,
    /// Maps whose picture could not be drawn, by SHA-256: not tried again
    /// until the game restarts.
    failed: HashSet<String>,
    /// The newest map shown without a picture.
    wanted: Option<MapPictureRequest>,
    running: Option<Running>,
    /// The pictures for the game folder (`super::generation_folder`), found on
    /// first use.
    folder: Option<PathBuf>,
}

struct Identity {
    /// A loose file's path, length and modification time, to notice it being
    /// replaced; None for a map in the game's archives.
    file: Option<(PathBuf, u64, Option<SystemTime>)>,
    sha256: String,
}

struct Running {
    sha256: String,
    child: std::process::Child,
    started: Instant,
}

/// The kept picture of the catalog's map `map_index`, or None while it has
/// none. A local map without one gets a run that draws it; a library map
/// without a file shows a picture only when one is kept.
pub(crate) fn picture_for_map(state: &mut AppState, map_index: usize) -> Option<DecodedPreview> {
    let folder = match &state.frontend.map_pictures.folder {
        Some(folder) => folder.clone(),
        None => {
            let paths = &state.platform.game_config.as_ref()?.paths;
            let folder = super::generation_folder(&paths.map_pictures_dir(), &paths.ra2_dir);
            state.frontend.map_pictures.folder = Some(folder.clone());
            folder
        }
    };
    let record = state.frontend.scenario_catalog.records().get(map_index)?;
    let (sha256, map) = match &record.source {
        SkirmishScenarioSource::Online(sha256) => (sha256.clone(), None),
        // A seed draws a new map each time; it has no fixed picture.
        _ if crate::map::rmg::is_seed_selection(&record.file_name) => return None,
        _ => {
            let map = record.file_name.clone();
            (map_sha256(state, &map)?, Some(map))
        }
    };
    let output = super::picture_path(&folder, &sha256);
    if let Some(picture) = super::read_png(&output) {
        return Some(picture);
    }
    if let Some(map) = map {
        want(
            &mut state.frontend.map_pictures,
            MapPictureRequest::for_menus(map, sha256, output),
        );
    }
    None
}

/// The SHA-256 of the file a game of `map` would load, read once per file.
fn map_sha256(state: &mut AppState, map: &str) -> Option<String> {
    let key = map.to_ascii_lowercase();
    if let Some(identity) = state.frontend.map_pictures.identities.get(&key)
        && identity
            .file
            .as_ref()
            .is_none_or(|(path, length, modified)| {
                file_stamp(path).is_some_and(|stamp| stamp == (*length, *modified))
            })
    {
        return Some(identity.sha256.clone());
    }
    let ra2_dir = &state.platform.game_config.as_ref()?.paths.ra2_dir;
    let source =
        crate::map::source::map_source_identity(ra2_dir, map, state.process_assets.manager()?)
            .inspect_err(|error| log::debug!("No map file for {map}'s picture: {error:#}"))
            .ok()?;
    let (_, sha256) = source.physical_identity()?;
    let identity = Identity {
        file: match &source {
            crate::map::source::LoadedMapSource::Loose { path, .. } => {
                let (length, modified) = file_stamp(path)?;
                Some((path.clone(), length, modified))
            }
            _ => None,
        },
        sha256: sha256.to_owned(),
    };
    let sha256 = identity.sha256.clone();
    state.frontend.map_pictures.identities.insert(key, identity);
    Some(sha256)
}

fn file_stamp(path: &std::path::Path) -> Option<(u64, Option<SystemTime>)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.len(), metadata.modified().ok()))
}

fn want(pictures: &mut MapPictures, request: MapPictureRequest) {
    if pictures.failed.contains(&request.sha256)
        || pictures
            .running
            .as_ref()
            .is_some_and(|running| running.sha256 == request.sha256)
    {
        return;
    }
    pictures.wanted = Some(request);
    start_wanted(pictures);
}

fn start_wanted(pictures: &mut MapPictures) {
    if pictures.running.is_some() {
        return;
    }
    let Some(request) = pictures.wanted.take() else {
        return;
    };
    match start(&request) {
        Ok(child) => {
            log::info!("Drawing the picture of {}", request.map);
            pictures.running = Some(Running {
                sha256: request.sha256,
                child,
                started: Instant::now(),
            });
        }
        Err(error) => {
            log::warn!("Could not start the picture of {}: {error:#}", request.map);
            pictures.failed.insert(request.sha256);
        }
    }
}

/// This executable in the hidden map-picture run, in the same folder and
/// with the same game configuration, silent.
fn start(request: &MapPictureRequest) -> Result<std::process::Child> {
    let executable = std::env::current_exe().context("find VERA20k's executable")?;
    let mut command = std::process::Command::new(executable);
    command
        .args(request.args())
        // The developer sandbox would start a different map at start-up.
        .env_remove("RA2_QUICKPLAY")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        // A console build would otherwise open a console window.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    Ok(command.spawn()?)
}

/// Take a finished run and start the next. A new picture replaces the map's
/// own preview on the screen showing it.
pub(crate) fn poll(state: &mut AppState) -> Option<Instant> {
    // A match and its loading get the machine to themselves, and unticking
    // HD Map Previews stops the drawing; a map stopped is drawn when next
    // shown.
    if !state.hd_map_previews() || state.frontend.screen != GameScreen::MainMenu {
        let pictures = &mut state.frontend.map_pictures;
        pictures.wanted = None;
        if let Some(running) = pictures.running.take() {
            end(running.child);
        }
        return None;
    }
    let pictures = &mut state.frontend.map_pictures;
    let running = pictures.running.as_mut()?;
    let (drawn, overdue) = match running.child.try_wait() {
        Ok(Some(status)) => (status.success(), false),
        Ok(None) if running.started.elapsed() < RUN_TIMEOUT => {
            return Some(Instant::now() + POLL_INTERVAL);
        }
        Ok(None) => (false, true),
        Err(error) => {
            log::warn!("Lost the map picture run: {error}");
            (false, false)
        }
    };
    let running = pictures.running.take().expect("checked above");
    if overdue {
        end(running.child);
    }
    if !drawn {
        log::warn!(
            "Could not draw the map picture {} (logs/map-picture.log has why)",
            running.sha256
        );
        pictures.failed.insert(running.sha256);
    }
    start_wanted(pictures);
    let next = pictures
        .running
        .is_some()
        .then(|| Instant::now() + POLL_INTERVAL);
    if drawn
        && state
            .frontend
            .skirmish_preview_texture
            .as_ref()
            .is_some_and(|texture| texture.hd_setting && texture.hd_picture.is_none())
    {
        state.frontend.skirmish_preview_texture = None;
    }
    next
}

/// Kill a run and reap it on a thread of its own: a run stuck in a GPU
/// driver call can be slow to die, and the menus must not wait for it.
fn end(mut child: std::process::Child) {
    let _ = child.kill();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

/// Stop the run as the game quits; dropping it would leave it running.
pub(crate) fn stop_for_exit(pictures: &mut MapPictures) {
    pictures.wanted = None;
    let Some(mut running) = pictures.running.take() else {
        return;
    };
    let _ = running.child.kill();
    let deadline = Instant::now() + EXIT_WAIT;
    while Instant::now() < deadline && matches!(running.child.try_wait(), Ok(None)) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
