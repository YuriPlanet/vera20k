//! The system's own dialogs, for what the game window cannot show: a startup
//! that never got a working window or renderer, and a panic. A Windows release
//! build has no console and the app bundle and AppImage none either, so
//! without these the game would vanish with only the log knowing why. The
//! first-launch game folder questions (`app_bundle`) use the same macOS alert
//! and Linux dialog tool.
//!
//! Windows shows `MessageBoxW`, macOS an `NSAlert` and Linux `zenity` or
//! `kdialog`, whichever is installed; without either, stderr is all there is.
//!
//! Their words come from VERA's language files (`crate::i18n`), in the
//! language `app::initialize` chose; a failure before that reads in English.

use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::thread::ThreadId;

/// Whether a fatal failure may wait on a click; see [`enable_fatal_messages`].
static FATAL_MESSAGES: AtomicBool = AtomicBool::new(false);
/// Set by the first fatal message, so a panic that brings down a second thread
/// does not stack another box over it.
static FATAL_MESSAGE_SHOWN: AtomicBool = AtomicBool::new(false);
/// The thread that turned fatal messages on: the game's main thread, whose
/// panic always ends the game. A worker's panic may not (the random map
/// dialog recovers from its generator's death), so it gets no box unless the
/// main thread re-raises it ([`report_reraised_panic`]).
static MAIN_THREAD: OnceLock<ThreadId> = OnceLock::new();

/// Let fatal failures show a message box. Only a launch a player is watching
/// turns this on; captures, `-SPAWN` test runs and tests leave it off so that
/// nothing unattended waits on a click.
/// Call it from the main thread.
pub fn enable_fatal_messages() {
    let _ = MAIN_THREAD.set(std::thread::current().id());
    FATAL_MESSAGES.store(true, Ordering::Release);
}

/// What the last worker thread's panic said, kept for the box the main
/// thread shows if that panic is re-raised there.
static WORKER_PANIC: Mutex<Option<String>> = Mutex::new(None);

/// The panic hook's notifier (`util::logging::install_panic_hook`). It runs
/// after the log and `except.txt` are written, and must not panic itself.
pub fn report_panic(info: &std::panic::PanicHookInfo<'_>) {
    let on_main = MAIN_THREAD.get() == Some(&std::thread::current().id());
    if let Some(what) = note_panic(on_main, info.to_string(), &WORKER_PANIC) {
        show_crash(what);
    }
}

/// The main thread is about to re-raise a panic it caught
/// (`entry::run_app`). `resume_unwind` does not run the panic hook, so a
/// panic that began on a worker (pooled sim work, the frame hand-off thread)
/// would end the game with no box; show it here. A panic that began on the
/// main thread already showed its box, and [`show_fatal`] shows only one.
pub fn report_reraised_panic(payload: &(dyn Any + Send)) {
    show_crash(reraised_panic_text(payload, &WORKER_PANIC));
}

/// A panic the hook saw: the text for a box now, on the main thread, or
/// `None` on a worker, whose text is kept in case the main thread re-raises
/// it. A worker's panic alone shows nothing: it may not end the game.
fn note_panic(on_main: bool, what: String, worker: &Mutex<Option<String>>) -> Option<String> {
    if on_main {
        return Some(what);
    }
    *worker.lock().unwrap_or_else(PoisonError::into_inner) = Some(what);
    None
}

/// The text for a re-raised panic: the worker's own report (it has the
/// source location) when it is this panic's, else the payload's message.
fn reraised_panic_text(payload: &(dyn Any + Send), worker: &Mutex<Option<String>>) -> String {
    let message = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("");
    worker
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .filter(|what| !message.is_empty() && what.contains(message))
        .unwrap_or_else(|| message.to_owned())
}

fn show_crash(mut what: String) {
    if let Some((end, _)) = what.char_indices().nth(600) {
        what.truncate(end);
        what.push('…');
    }
    show_fatal(
        &crate::i18n::tr("VERA:CrashTitle"),
        &crate::i18n::tr_fill("VERA:CrashText", &[("panic", &what), ("log", &log_note())]),
    );
}

/// Tell the player why startup failed, before the process exits.
pub(crate) fn report_startup_failure(error: &anyhow::Error) {
    if error
        .downcast_ref::<crate::render::gpu::GraphicsUnavailable>()
        .is_some()
    {
        show_fatal(
            &crate::i18n::tr("VERA:GraphicsFailedTitle"),
            &crate::i18n::tr_fill(
                "VERA:GraphicsFailedText",
                &[
                    ("advice", &crate::i18n::tr(GRAPHICS_ADVICE)),
                    ("log", &log_note()),
                    ("error", &format!("{error:#}")),
                ],
            ),
        );
    } else {
        show_fatal(
            &crate::i18n::tr("VERA:StartFailedTitle"),
            &format!("{error:#}\n\n{}", log_note()),
        );
    }
}

/// What the graphics failure advises on this system, as its text's label.
#[cfg(target_os = "windows")]
const GRAPHICS_ADVICE: &str = "VERA:GraphicsAdviceWindows";
#[cfg(target_os = "macos")]
const GRAPHICS_ADVICE: &str = "VERA:GraphicsAdviceMac";
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const GRAPHICS_ADVICE: &str = "VERA:GraphicsAdviceLinux";

fn log_note() -> String {
    match crate::util::logging::log_file() {
        Some(path) => crate::i18n::tr_fill("VERA:LogFileDetails", &[("path", &path.display())]),
        None => crate::i18n::tr("VERA:NoLogFile"),
    }
}

fn show_fatal(title: &str, message: &str) {
    if !FATAL_MESSAGES.load(Ordering::Acquire) || FATAL_MESSAGE_SHOWN.swap(true, Ordering::AcqRel) {
        return;
    }
    show_error(title, message);
}

/// Show an error box and wait until the player closes it.
#[cfg(target_os = "windows")]
fn show_error(title: &str, message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        ClipCursor, MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MessageBoxW, ShowCursor,
    };

    let wide = |text: &str| text.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (message, title) = (wide(message), wide(title));
    // SAFETY: both strings are NUL-terminated UTF-16 that outlive the calls; a
    // null rectangle and a null owner are documented arguments.
    unsafe {
        // The game may hold the pointer inside its window and hidden there
        // (`input::cursor`); the box needs it free and visible.
        ClipCursor(std::ptr::null());
        for _ in 0..64 {
            if ShowCursor(1) >= 0 {
                break;
            }
        }
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR | MB_TOPMOST | MB_SETFOREGROUND,
        );
    }
}

/// Show an error box and wait until the player closes it. AppKit runs it on
/// the main thread only; elsewhere the log and stderr have to do.
#[cfg(target_os = "macos")]
fn show_error(title: &str, message: &str) {
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        alert(
            mtm,
            objc2_app_kit::NSAlertStyle::Critical,
            title,
            message,
            &[],
        );
    }
}

/// Run an alert modally and return the button the player chose; no
/// `buttons` gives AppKit's own OK.
#[cfg(target_os = "macos")]
pub(crate) fn alert(
    mtm: objc2::MainThreadMarker,
    style: objc2_app_kit::NSAlertStyle,
    title: &str,
    message: &str,
    buttons: &[&str],
) -> objc2_app_kit::NSModalResponse {
    use objc2_app_kit::NSAlert;
    use objc2_foundation::NSString;

    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(style);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(message));
    for button in buttons {
        alert.addButtonWithTitle(&NSString::from_str(button));
    }
    alert.runModal()
}

/// Show an error box and wait until the player closes it.
#[cfg(target_os = "linux")]
fn show_error(title: &str, message: &str) {
    use std::process::Command;

    let shown = match DialogTool::installed() {
        Some(DialogTool::Zenity) => Command::new("zenity")
            .args(["--error", "--no-markup", "--title=VERA20k"])
            .arg(format!("--text={title}\n\n{message}"))
            .status()
            .is_ok(),
        Some(DialogTool::Kdialog) => Command::new("kdialog")
            .args(["--title", "VERA20k", "--error"])
            .arg(format!("{title}\n\n{message}"))
            .status()
            .is_ok(),
        None => false,
    };
    if !shown {
        eprintln!("{title}\n\n{message}");
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn show_error(title: &str, message: &str) {
    eprintln!("{title}\n\n{message}");
}

/// The desktop's dialog program: zenity (GNOME and most desktops) or kdialog
/// (KDE), run as a child process.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
pub(crate) enum DialogTool {
    Zenity,
    Kdialog,
}

#[cfg(target_os = "linux")]
impl DialogTool {
    /// The installed tool that suits this desktop: kdialog on KDE, else
    /// zenity, else kdialog.
    pub(crate) fn installed() -> Option<Self> {
        let installed = |program: &str| {
            std::env::var_os("PATH").is_some_and(|path| {
                std::env::split_paths(&path).any(|folder| folder.join(program).is_file())
            })
        };
        let kde = std::env::var("XDG_CURRENT_DESKTOP")
            .is_ok_and(|desktops| desktops.split(':').any(|desktop| desktop == "KDE"));
        if kde && installed("kdialog") {
            Some(Self::Kdialog)
        } else if installed("zenity") {
            Some(Self::Zenity)
        } else if installed("kdialog") {
            Some(Self::Kdialog)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_worker_panic_shows_its_box_only_when_the_main_thread_re_raises_it() {
        let worker = Mutex::new(None);
        let report = "panicked at src/sim/parallel.rs:13:5:\nzone build failed".to_owned();
        // On the worker: no box (the random-map generator's death is survived).
        assert_eq!(note_panic(false, report.clone(), &worker), None);
        // Re-raised on main with the same payload: the worker's full report.
        let payload: Box<dyn Any + Send> = Box::new("zone build failed".to_owned());
        assert_eq!(reraised_panic_text(&*payload, &worker), report);
        // A survived worker death is not shown for a later, different panic.
        assert_eq!(
            note_panic(false, "panicked: generator died".into(), &worker),
            None
        );
        let other: Box<dyn Any + Send> = Box::new("frame hand-off failed");
        assert_eq!(
            reraised_panic_text(&*other, &worker),
            "frame hand-off failed"
        );
        // A main-thread panic still shows at once.
        assert_eq!(note_panic(true, report.clone(), &worker), Some(report));
    }
}
