//! Process start-up shared by the desktop binary and the Android library.
//!
//! `main.rs` keeps the CLI-only switches (replay verification, data check and
//! the spawn probe), then hands the rest here. Android never runs `main`: the
//! cdylib's `android_main` reaches the same start-up through [`run_with_loop`],
//! which takes an event loop already bound to the activity (Android builds the
//! event loop from an `AndroidApp`, which only the JNI-facing crate owns).
//!
//! Both paths must stay in step: whatever a platform needs before the first
//! window (working directory, logging, panic reporting) belongs here, not in
//! `main.rs`, or Android silently skips it.

#[cfg(target_os = "android")]
use anyhow::Context;
use anyhow::Result;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use winit::event_loop::EventLoop;

use crate::app::frontend::launch::{AppLaunchMode, parse_launch_args};

/// Open the file logger, fall back to stderr, and install the panic hook.
///
/// Returns the log file path when a file logger opened. A platform without a
/// writable working directory still gets diagnostics on stderr.
pub fn init_logging() -> Option<PathBuf> {
    // A hidden map-picture run, which the menus start while the game is
    // running, keeps its own log beside the game's.
    let name = if crate::app::map_picture::is_invocation(std::env::args_os().skip(1)) {
        "map-picture"
    } else {
        "ra2"
    };
    let log_path = match crate::util::logging::init_file_logger(name) {
        Ok(path) => {
            eprintln!("Logging to {}", path.display());
            Some(path)
        }
        Err(err) => {
            eprintln!("Failed to initialize file logger: {err}");
            let _ = crate::util::logging::init_stderr_logger("info");
            None
        }
    };

    crate::util::logging::install_panic_hook(
        log_path.as_deref(),
        Some(crate::app::system_dialog::report_panic),
    );

    log_path
}

/// The start-up banner: build identity, compiled capabilities, log location.
pub fn log_banner(log_path: Option<&Path>) {
    log::info!(
        "VERA20k {} starting ({}/{})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    log::info!("Ares support compiled in: {}", crate::rules::ARES_BUILD);
    log::info!(
        "Phobos support compiled in: {}",
        crate::rules::PHOBOS_ENABLED
    );
    log::info!(
        "NPatch support compiled in: {}",
        crate::rules::NPATCH_ENABLED
    );
    log::info!("VeraX support compiled in: {}", crate::rules::VERAX_ENABLED);
    log::info!("NPBUG support compiled in: {}", crate::rules::NPBUG_ENABLED);
    if let Some(path) = log_path {
        log::info!("Log file: {}", path.display());
    }
}

/// Adopt the platform's writable data directory and make it the working
/// directory.
///
/// Android starts every process in `/` with no writable home, so its cdylib
/// calls this before logging opens `logs/`. Everything below that resolves a
/// relative or per-user path — the log directory, `config.toml`, `VERA.INI`,
/// saves and replays — then lands somewhere writable, and the settings folder
/// resolves to this same directory rather than to a `HOME` that does not exist.
#[cfg(target_os = "android")]
pub fn adopt_android_data_dir(dir: &Path) -> Result<()> {
    crate::util::platform_dir::set(dir.to_path_buf());
    std::fs::create_dir_all(dir)
        .with_context(|| format!("create the Android data directory {}", dir.display()))?;
    std::env::set_current_dir(dir)
        .with_context(|| format!("enter the Android data directory {}", dir.display()))?;
    Ok(())
}

/// Desktop start-up: parse the process arguments, then drive a fresh event loop.
///
/// Returns the process exit status: 0, or a `-SPAWN` launch's result status.
pub fn run(app_bundle: bool) -> Result<i32> {
    run_with_args(app_bundle, std::env::args_os().skip(1))
}

fn run_with_args(app_bundle: bool, args: impl IntoIterator<Item = OsString>) -> Result<i32> {
    let (mod_switch, args) = crate::app::mods::take_switch(args.into_iter().collect())?;
    let launch_mode = parse_launch_args(args)?;

    // Help must work without a display server or an OS event loop, including
    // when called from a non-main thread by a headless command-line tool.
    if matches!(launch_mode, AppLaunchMode::Usage) {
        return Ok(print_usage());
    }

    // gamemd's 1 ms Windows timer period, until the event loop returns.
    let _wall_clock_period = crate::app::match_runtime::frame_pacer::WallClockPeriod::hold();

    // Create the OS event loop. This drives the entire application:
    // window events, input, redraws, lifecycle events.
    let mut builder = EventLoop::builder();
    // A `-SPAWN` test run with its opt-in invisible window (tools/spawn/README.md)
    // and a hidden map-picture run also stay out of the Dock and menu bar.
    #[cfg(target_os = "macos")]
    if matches!(&launch_mode, AppLaunchMode::MapPicture(_))
        || matches!(&launch_mode, AppLaunchMode::Spawn(_))
            && crate::app::App::spawn_hidden_window_requested()
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder.with_activation_policy(ActivationPolicy::Prohibited);
    }
    let event_loop: EventLoop<()> = builder.build()?;

    run_with(launch_mode, app_bundle, event_loop, mod_switch)
}

fn print_usage() -> i32 {
    println!("{}", crate::app::frontend::startup_options::usage_text());
    0
}

/// Shared start-up once an event loop exists.
///
/// Android passes a loop built from its `AndroidApp`; the desktop passes the one
/// [`run`] built. Everything after this point is platform-neutral.
pub fn run_with_loop(
    launch_mode: AppLaunchMode,
    app_bundle: bool,
    event_loop: EventLoop<()>,
) -> Result<i32> {
    run_with(launch_mode, app_bundle, event_loop, None)
}

fn run_with(
    launch_mode: AppLaunchMode,
    app_bundle: bool,
    event_loop: EventLoop<()>,
    mod_switch: Option<crate::app::mods::ModSwitch>,
) -> Result<i32> {
    // A player started this launch and may have no console: a failure that
    // ends it shows a message box. Spawn, replay and capture launches come
    // from other programs and scripts, which must never wait on a click.
    if matches!(launch_mode, AppLaunchMode::Interactive(_)) {
        crate::app::system_dialog::enable_fatal_messages();
        // Those messages read in the system's language until the settings
        // folder's choice loads (`initialize`).
        crate::i18n::init(None, None);
    }

    // Callers supplying their own loop share the same help output. Desktop
    // startup handles this before creating its event loop.
    if matches!(launch_mode, AppLaunchMode::Usage) {
        return Ok(print_usage());
    }

    // Native claims its single-instance gate before it creates a window: a
    // second copy raises and restores the running one and exits, so the two
    // never both write VERA.INI on quit. Capture modes are automation-only and
    // are deliberately left ungated so parallel captures still run —
    // VERA-internal exemption, native has no capture mode.
    let _instance_guard = match &launch_mode {
        AppLaunchMode::Interactive(_) | AppLaunchMode::Spawn(_) | AppLaunchMode::Replay(_) => {
            match crate::util::single_instance::acquire(crate::app::WINDOW_TITLE) {
                crate::util::single_instance::SingleInstance::Acquired(guard) => Some(guard),
                crate::util::single_instance::SingleInstance::AlreadyRunning => {
                    if matches!(
                        &launch_mode,
                        AppLaunchMode::Spawn(_) | AppLaunchMode::Replay(_)
                    ) {
                        anyhow::bail!(
                            "cannot start an external session or replay while another VERA20k instance is running"
                        );
                    }
                    // Recorded gap: native also exits with a success status
                    // here, so a live-smoke harness that only checks the exit
                    // code reads "already running" as a clean run. Anything
                    // asserting on a window must assert on the window.
                    log::info!("VERA20k is already running — raised the existing window");
                    return Ok(0);
                }
            }
        }
        _ => None,
    };

    // Create the app and hand control to the event loop.
    // This blocks until the window is closed.
    let mut app: crate::app::App = match launch_mode {
        AppLaunchMode::Usage => {
            unreachable!("usage returned above")
        }
        AppLaunchMode::Interactive(options) => {
            // Pass the parsed process policy to its consumers: options,
            // audio and retained asset ownership.
            log::info!("Retail startup switches: {options:?}");
            let mut app = crate::app::App::new(options);
            if app_bundle {
                app.ask_for_game_folder_first();
            }
            app
        }
        AppLaunchMode::Spawn(options) => crate::app::App::new_spawn(options)?,
        AppLaunchMode::Replay(request) => crate::app::App::new_replay(request),
        // A hidden map-picture run is desktop automation the menus start; it
        // watches its parent's lifetime and never reaches Android.
        AppLaunchMode::MapPicture(request) => {
            crate::app::map_picture::watch_lifetime();
            crate::app::App::new_map_picture(request)
        }
        AppLaunchMode::ShellCapture(request) => {
            request.validate_runtime_environment()?;
            crate::app::App::new_shell_capture(request)
        }
        AppLaunchMode::TacticalCapture(request) => {
            request.validate_runtime_environment()?;
            crate::app::App::new_tactical_capture(request)
        }
    };
    app.set_mod_switch(mod_switch);
    app.set_sim_wake(event_loop.create_proxy());
    // A panic skips `exiting`, where the match replay is saved. winit stops
    // its loop and hands the panic back here (macOS and Windows catch it in
    // their callbacks; elsewhere it unwinds straight out), with `app` intact.
    // Save the replay, then let the panic end the process as before.
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        event_loop.run_app(&mut app)
    }));
    match ran {
        Ok(result) => result?,
        Err(panic) => {
            app.save_replay_after_panic();
            // resume_unwind skips the hook: a worker's panic gets its box here.
            crate::app::system_dialog::report_reraised_panic(&*panic);
            std::panic::resume_unwind(panic);
        }
    }
    // Written for failed launches too, before the failure returns below.
    let spawn_status = app.finish_spawn_launch();
    app.finish_capture()?;

    log::info!("VERA20k shut down cleanly");
    Ok(spawn_status.unwrap_or(0))
}

/// Flush the log, report a failed launch, and turn the status into the process
/// result. A non-zero `-SPAWN` status ends the process here. A copy that shut
/// down in the ordinary way starts the next one when the Mods page asked.
pub fn finish(result: Result<i32>) -> Result<()> {
    if let Err(error) = &result {
        log::error!("VERA20k exited with an error: {error:#}");
    }
    log::logger().flush();
    // docs/spawn.md: a `-SPAWN` launch exits with its result's status.
    match result? {
        0 => {
            // The Mods page's restart: this copy is done, the next may start.
            crate::app::restart::start_again_if_asked();
            Ok(())
        }
        status => std::process::exit(status),
    }
}

#[cfg(test)]
mod tests {
    use super::run_with_args;
    use std::ffi::OsString;

    #[test]
    fn native_help_exits_without_initializing_an_event_loop() {
        // The test runs off the main thread, where constructing an OS event
        // loop would fail even on a machine with a display server.
        assert_eq!(run_with_args(false, [OsString::from("-h")]).unwrap(), 0);
    }
}
