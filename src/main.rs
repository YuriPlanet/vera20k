//! VERA20k — entry point.
//!
//! Initializes process diagnostics, then hands the winit event loop to App.
//! Application logic lives in the app facade and its modules.
//!
//! Module declarations live in lib.rs so integration tests can import them.

// Release games are GUI applications. Keep the console for development builds;
// the file logger below retains diagnostics when Windows supplies no console.
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use anyhow::Result;
use winit::event_loop::EventLoop;

fn main() -> Result<()> {
    // A launch from the macOS app bundle works in the app's data folder. Enter
    // it before the logger opens logs/ there.
    let app_bundle = vera20k::app::app_bundle::enter_data_folder();
    let log_path = match vera20k::util::logging::init_file_logger("ra2") {
        Ok(path) => {
            eprintln!("Logging to {}", path.display());
            Some(path)
        }
        Err(err) => {
            eprintln!("Failed to initialize file logger: {err}");
            let _ = vera20k::util::logging::init_stderr_logger("info");
            None
        }
    };

    vera20k::util::logging::install_panic_hook(log_path.as_deref());

    log::info!(
        "VERA20k {} starting ({}/{})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    log::info!("Ares support compiled in: {}", vera20k::rules::ARES_ENABLED);
    if let Some(path) = &log_path {
        log::info!("Log file: {}", path.display());
    }

    let result = run(app_bundle);
    if let Err(error) = &result {
        log::error!("VERA20k exited with an error: {error:#}");
    }
    log::logger().flush();
    result
}

fn run(app_bundle: bool) -> Result<()> {
    let launch_mode =
        vera20k::app::frontend::launch::parse_launch_args(std::env::args_os().skip(1))?;

    // A help switch terminates before anything else is created, matching the
    // native switch parser returning a failure that makes WinMain bail.
    if matches!(
        launch_mode,
        vera20k::app::frontend::launch::AppLaunchMode::Usage
    ) {
        println!("{}", vera20k::app::frontend::startup_options::usage_text());
        return Ok(());
    }

    // Native claims its single-instance gate before it creates a window: a
    // second copy raises and restores the running one and exits, so the two
    // never both write RA2MD.INI on quit. Capture modes are automation-only and
    // are deliberately left ungated so parallel captures still run —
    // VERA-internal exemption, native has no capture mode.
    let _instance_guard = match &launch_mode {
        vera20k::app::frontend::launch::AppLaunchMode::Interactive(_)
        | vera20k::app::frontend::launch::AppLaunchMode::Spawn(_) => {
            match vera20k::util::single_instance::acquire(vera20k::app::WINDOW_TITLE) {
                vera20k::util::single_instance::SingleInstance::Acquired(guard) => Some(guard),
                vera20k::util::single_instance::SingleInstance::AlreadyRunning => {
                    if matches!(
                        &launch_mode,
                        vera20k::app::frontend::launch::AppLaunchMode::Spawn(_)
                    ) {
                        anyhow::bail!(
                            "cannot start the CnCNet session while another VERA20k instance is running"
                        );
                    }
                    // Recorded gap: native also exits with a success status
                    // here, so a live-smoke harness that only checks the exit
                    // code reads "already running" as a clean run. Anything
                    // asserting on a window must assert on the window.
                    log::info!("VERA20k is already running — raised the existing window");
                    return Ok(());
                }
            }
        }
        _ => None,
    };

    // Create the OS event loop. This drives the entire application:
    // window events, input, redraws, lifecycle events.
    let event_loop: EventLoop<()> = EventLoop::builder().build()?;

    // Create the app and hand control to the event loop.
    // This blocks until the window is closed.
    let mut app: vera20k::app::App = match launch_mode {
        vera20k::app::frontend::launch::AppLaunchMode::Usage => {
            unreachable!("usage returned above")
        }
        vera20k::app::frontend::launch::AppLaunchMode::Interactive(options) => {
            // Pass the parsed process policy to its consumers: options,
            // audio and retained asset ownership.
            log::info!("Retail startup switches: {options:?}");
            let mut app = vera20k::app::App::new(options);
            if app_bundle {
                app.ask_for_game_folder_first();
            }
            app
        }
        vera20k::app::frontend::launch::AppLaunchMode::Spawn(options) => {
            vera20k::app::App::new_spawn(options)?
        }
        vera20k::app::frontend::launch::AppLaunchMode::ShellCapture(request) => {
            request.validate_runtime_environment()?;
            vera20k::app::App::new_shell_capture(request)
        }
        vera20k::app::frontend::launch::AppLaunchMode::TacticalCapture(request) => {
            request.validate_runtime_environment()?;
            vera20k::app::App::new_tactical_capture(request)
        }
    };
    event_loop.run_app(&mut app)?;
    app.finish_capture()?;

    log::info!("VERA20k shut down cleanly");
    Ok(())
}
