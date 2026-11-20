//! The graphics API a start asks wgpu for, on Windows, which has two. A
//! driver that loses the device before startup finishes fails every start
//! the same way: a GeForce GTX 950M's Vulkan driver, too old to name its
//! version, did one second after GPU init on every VERA version. Where a
//! driver offers Vulkan and DirectX 12, wgpu takes Vulkan (`GpuContext::new`).
//! So a start that dies of a lost device records the API it was on, beside
//! VERA.INI, and the next start asks for the other API alone; the box that
//! closes the game says so (`system_dialog::graphics_lost_message`). A start
//! that finishes takes its API out of the record, so wgpu's `WGPU_BACKEND`
//! override, which wins over this choice, tries a repaired driver once and,
//! when it works, for good. A loss after startup, in play, changes nothing
//! here (`app::graphics_lost`).
//!
//! A driver can also hang instead of failing: an Intel HD 620's 2017 Vulkan
//! driver never returned from creating the device or the swapchain, so the
//! window stayed black until the player killed it, twenty-five starts in a
//! row, and no loss was ever reported. So a start also leaves a marker from
//! the moment it has chosen its adapter (`starting`) until startup finishes;
//! the next start reads a leftover marker as that API's failure.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::util::file_system;

/// The record beside VERA.INI: one line per API that lost the device at a
/// start, `<api> <vendor:device> <adapter name>`, after a comment that says
/// what it is.
const RECORD: &str = "GraphicsLostAtStart.txt";
const RECORD_HEADER: &str = "# VERA20k lost its graphics device, or never finished starting, on these\n\
                             # graphics APIs, so it starts on the other one. Delete this file to try them\n\
                             # again; WGPU_BACKEND=vulkan or dx12 overrides it for one start.\n";
/// The marker a start leaves beside VERA.INI from its adapter choice until
/// startup finishes: one line as the record holds it. Left behind, it says
/// the start hung or was killed before "GPU initialized" or the first menu.
const STARTING: &str = "GraphicsStarting.txt";
const UNFINISHED: &str = "(never finished starting)";

/// Windows' two APIs, in wgpu's own order of preference.
const WINDOWS_APIS: [wgpu::Backend; 2] = [wgpu::Backend::Vulkan, wgpu::Backend::Dx12];

/// Where [`backends`] found the record, for [`record_loss`] and
/// [`start_finished`].
static SETTINGS_DIR: OnceLock<PathBuf> = OnceLock::new();
/// Startup has finished ([`start_finished`]): a loss from now on is play's.
static STARTED: AtomicBool = AtomicBool::new(false);

/// The APIs this start asks wgpu for: the one `[VERA] GraphicsAPI=` names
/// (`forced`), else the Windows API the record leaves when the other failed
/// at a start, else wgpu's primary set.
pub(crate) fn backends(
    settings_dir: Option<&Path>,
    forced: Option<wgpu::Backend>,
) -> wgpu::Backends {
    let Some(dir) = settings_dir else {
        return forced.map_or(wgpu::Backends::PRIMARY, wgpu::Backends::from);
    };
    let _ = SETTINGS_DIR.set(dir.to_owned());
    if let Some(api) = forced {
        // The record still learns of a start that never finished, for the
        // day the key is removed again.
        let _ = note_unfinished_in(dir);
        log::info!("VERA.INI asks for {} ([VERA] GraphicsAPI=)", api_name(api));
        return wgpu::Backends::from(api);
    }
    if !cfg!(target_os = "windows") {
        return wgpu::Backends::PRIMARY;
    }
    if let Some(api) = note_unfinished_in(dir) {
        log::warn!(
            "The last start never finished on {}: the driver hung or the game was closed before its first menu",
            api_name(api)
        );
    }
    let failed = read_failed(dir);
    match other_api(&failed) {
        Some(api) => {
            log::warn!(
                "An earlier start failed on {}; this one draws through {} ({} says so; delete it to try again)",
                failed
                    .iter()
                    .map(|api| api_name(*api))
                    .collect::<Vec<_>>()
                    .join(" and "),
                api_name(api),
                dir.join(RECORD).display()
            );
            wgpu::Backends::from(api)
        }
        None => wgpu::Backends::PRIMARY,
    }
}

/// The start has chosen its adapter and is about to ask the driver for a
/// device (`GpuContext::new`): leave the marker that [`backends`] reads as
/// this API's failure if startup never finishes.
pub(crate) fn starting(info: &wgpu::AdapterInfo) {
    if !cfg!(target_os = "windows") {
        return;
    }
    if let Some(dir) = SETTINGS_DIR.get() {
        starting_in(dir, info.backend, &adapter_line(info));
    }
}

/// Startup failed or died for a reason that is not the graphics API's (a
/// missing file, a panic on a working device): the marker goes, so the next
/// start keeps its API.
pub(crate) fn forget_start() {
    if let Some(dir) = SETTINGS_DIR.get() {
        forget_start_in(dir);
    }
}

/// Startup has finished on the process's device
/// (`render::gpu::device_identity`): its API works here, so it leaves the
/// record and the start marker. A loss from now on is play's.
pub(crate) fn start_finished() {
    STARTED.store(true, Ordering::Release);
    if let (Some(dir), Some(identity)) = (SETTINGS_DIR.get(), crate::render::gpu::device_identity())
    {
        forget_start_in(dir);
        finish_in(dir, identity.backend);
    }
}

/// The game is closing because its device was lost. Before startup finished,
/// the loss goes into the record with its API, and the result is the API the
/// next start uses instead, for the box that closes the game. `None` after
/// startup, without a settings directory, off Windows, and once both APIs
/// have failed.
pub(crate) fn record_loss() -> Option<wgpu::Backend> {
    if STARTED.load(Ordering::Acquire) || !cfg!(target_os = "windows") {
        return None;
    }
    let dir = SETTINGS_DIR.get()?;
    let identity = crate::render::gpu::device_identity()?;
    // The loss is the record's now; the marker would only repeat it.
    forget_start_in(dir);
    record_loss_in(
        dir,
        identity.backend,
        &format!(
            "{:04x}:{:04x} {}",
            identity.vendor, identity.device, identity.name
        ),
    )
}

fn adapter_line(info: &wgpu::AdapterInfo) -> String {
    format!("{:04x}:{:04x} {}", info.vendor, info.device, info.name)
}

fn starting_in(dir: &Path, api: wgpu::Backend, adapter: &str) {
    let path = dir.join(STARTING);
    if let Err(error) = file_system::write(&path, format!("{} {adapter}\n", api.to_str())) {
        // Without the marker a hung start is not told apart from a working one.
        log::warn!("Could not write {}: {error}", path.display());
    }
}

fn forget_start_in(dir: &Path) {
    let path = dir.join(STARTING);
    match file_system::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => log::warn!("Could not remove {}: {error}", path.display()),
    }
}

/// A marker the last start left behind goes into the record as its API's
/// failure; the API, when there was one.
fn note_unfinished_in(dir: &Path) -> Option<wgpu::Backend> {
    let line = file_system::read_to_string(dir.join(STARTING)).ok()?;
    forget_start_in(dir);
    let line = line.trim();
    let api = api_of(line)?;
    let mut lines = read_lines(dir);
    if !lines.iter().any(|known| api_of(known) == Some(api)) {
        lines.push(format!("{line} {UNFINISHED}"));
        write_lines(dir, &lines);
    }
    Some(api)
}

/// The API's name as the player knows it.
pub(crate) fn api_name(api: wgpu::Backend) -> &'static str {
    match api {
        wgpu::Backend::Vulkan => "Vulkan",
        wgpu::Backend::Dx12 => "DirectX 12",
        other => other.to_str(),
    }
}

/// The API the next start asks for alone: the Windows API that has not lost
/// the device at a start, while the other has.
fn other_api(failed: &[wgpu::Backend]) -> Option<wgpu::Backend> {
    if failed.is_empty() {
        return None;
    }
    WINDOWS_APIS
        .iter()
        .copied()
        .find(|api| !failed.contains(api))
}

fn record_loss_in(dir: &Path, api: wgpu::Backend, adapter: &str) -> Option<wgpu::Backend> {
    let mut lines = read_lines(dir);
    if !lines.iter().any(|line| api_of(line) == Some(api)) {
        lines.push(format!("{} {adapter}", api.to_str()));
        write_lines(dir, &lines);
    }
    let failed: Vec<wgpu::Backend> = lines.iter().filter_map(|line| api_of(line)).collect();
    other_api(&failed)
}

fn finish_in(dir: &Path, api: wgpu::Backend) {
    let lines = read_lines(dir);
    let kept: Vec<String> = lines
        .iter()
        .filter(|line| api_of(line) != Some(api))
        .cloned()
        .collect();
    if kept.len() != lines.len() {
        log::info!(
            "Startup finished on {}: it leaves {}",
            api_name(api),
            dir.join(RECORD).display()
        );
        write_lines(dir, &kept);
    }
}

fn read_failed(dir: &Path) -> Vec<wgpu::Backend> {
    read_lines(dir)
        .iter()
        .filter_map(|line| api_of(line))
        .collect()
}

/// The record's API lines, without its comment.
fn read_lines(dir: &Path) -> Vec<String> {
    file_system::read_to_string(dir.join(RECORD))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

fn write_lines(dir: &Path, lines: &[String]) {
    let path = dir.join(RECORD);
    let result = if lines.is_empty() {
        match file_system::remove_file(&path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    } else {
        file_system::write(&path, format!("{RECORD_HEADER}{}\n", lines.join("\n")))
    };
    if let Err(error) = result {
        // Without the record the next start repeats this one's choice.
        log::warn!("Could not write {}: {error}", path.display());
    }
}

fn api_of(line: &str) -> Option<wgpu::Backend> {
    match line.split_whitespace().next()? {
        "vulkan" => Some(wgpu::Backend::Vulkan),
        "dx12" => Some(wgpu::Backend::Dx12),
        _ => None,
    }
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use wgpu::Backend::{Dx12, Vulkan};

    fn record_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vera-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_start_lost_on_vulkan_sends_the_next_one_to_directx12() {
        let dir = record_dir("startup-backend-loss");
        assert_eq!(other_api(&[]), None);
        assert_eq!(
            record_loss_in(&dir, Vulkan, "10de:139a GeForce GTX 950M"),
            Some(Dx12)
        );
        assert_eq!(read_failed(&dir), [Vulkan]);
        // The same loss again adds no line.
        assert_eq!(
            record_loss_in(&dir, Vulkan, "10de:139a GeForce GTX 950M"),
            Some(Dx12)
        );
        assert_eq!(read_lines(&dir).len(), 1);
        let text = std::fs::read_to_string(dir.join(RECORD)).unwrap();
        assert!(text.starts_with("# VERA20k"), "{text}");
        assert!(
            text.contains("\nvulkan 10de:139a GeForce GTX 950M\n"),
            "{text}"
        );
        // DirectX 12 failing as well leaves no API to switch to.
        assert_eq!(
            record_loss_in(&dir, Dx12, "10de:139a NVIDIA GeForce GTX 950M"),
            None
        );
        assert_eq!(read_failed(&dir), [Vulkan, Dx12]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_start_that_never_finished_fails_its_api() {
        let dir = record_dir("startup-backend-unfinished");
        // A finished start leaves no marker and blames nothing.
        starting_in(&dir, Vulkan, "8086:5916 Intel(R) HD Graphics 620");
        forget_start_in(&dir);
        assert_eq!(note_unfinished_in(&dir), None);
        assert_eq!(read_failed(&dir), []);
        // One that hung in the driver leaves it, and the next start reads it.
        starting_in(&dir, Vulkan, "8086:5916 Intel(R) HD Graphics 620");
        assert_eq!(note_unfinished_in(&dir), Some(Vulkan));
        assert!(!dir.join(STARTING).exists());
        assert_eq!(read_failed(&dir), [Vulkan]);
        assert_eq!(other_api(&read_failed(&dir)), Some(Dx12));
        let text = std::fs::read_to_string(dir.join(RECORD)).unwrap();
        assert!(
            text.contains(
                "\nvulkan 8086:5916 Intel(R) HD Graphics 620 (never finished starting)\n"
            ),
            "{text}"
        );
        // Read once: a second start finds no marker.
        assert_eq!(note_unfinished_in(&dir), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_start_that_finishes_takes_its_api_out_of_the_record() {
        let dir = record_dir("startup-backend-finish");
        record_loss_in(&dir, Vulkan, "10de:139a GeForce GTX 950M");
        record_loss_in(&dir, Dx12, "10de:139a NVIDIA GeForce GTX 950M");
        finish_in(&dir, Dx12);
        assert_eq!(read_failed(&dir), [Vulkan]);
        // Finishing on an API the record never named changes nothing.
        finish_in(&dir, Dx12);
        assert_eq!(read_failed(&dir), [Vulkan]);
        // The last API out takes the file with it.
        finish_in(&dir, Vulkan);
        assert!(!dir.join(RECORD).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
