//! Android entry point for the VERA20k client.
//!
//! Android never runs `main`. `android.app.NativeActivity` loads the cdylib and
//! resolves `android_main` by symbol (the manifest names the library through
//! `android.app.lib_name`), so the process starts here instead. Everything
//! platform-specific that has to happen before the first window — the working
//! directory, logging, panic reporting — runs in this order and then hands over
//! to `vera20k::entry`, the same start-up the desktop binary uses.
//!
//! One process gets one `android_main` call per activity creation; the activity
//! is destroyed and recreated on rotation, a locale change or a configuration
//! change the manifest does not absorb. `EventLoop` can be built only once per
//! process (winit keeps it in a static), so a second call must not reach
//! `EventLoop::builder()` — see the guard below. Killing the process on that
//! path is deliberate for the first milestone: a stale surface is a black
//! screen, while a restart is recoverable. P5 replaces it with real
//! suspend/resume handling.
#![cfg(target_os = "android")]

use android_activity::AndroidApp;
use winit::platform::android::EventLoopBuilderExtAndroid;

/// Resolved by `android-activity`'s loader trampoline; the name is the ABI.
///
/// Android calls the symbol with the activity's handle, which
/// `android-activity` turns into an `AndroidApp`.
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    // Android starts every process in "/" with no writable working directory.
    // The logger opens `logs/` beside it and the settings reader resolves
    // VERA.INI from HOME, so enter the app's private folder first: everything
    // below assumes a real, writable current directory.
    if let Some(dir) = app.internal_data_path() {
        if let Err(error) = std::env::set_current_dir(&dir) {
            eprintln!("cannot enter {}: {error}", dir.display());
        }
    } else {
        eprintln!("no internal data path; logs and settings will not persist");
    }

    let log_path = vera20k::entry::init_logging();
    vera20k::entry::log_banner(log_path.as_deref());

    // An activity recreation reaches this point a second time. Report it and
    // let the activity finish rather than building a second event loop.
    if !claim_event_loop() {
        log::error!("android_main called again; the activity finishes here");
        return;
    }

    let event_loop: winit::event_loop::EventLoop<()> = match winit::event_loop::EventLoop::builder()
        .with_android_app(app)
        .build()
    {
        Ok(event_loop) => event_loop,
        Err(error) => {
            log::error!("cannot create the Android event loop: {error}");
            return;
        }
    };

    // Android has no retail command line: the launcher starts an ordinary
    // interactive game. `-SPAWN`, replay and capture launches are desktop
    // automation and stay out of this path.
    let launch_mode = vera20k::app::frontend::launch::AppLaunchMode::Interactive(
        vera20k::app::frontend::startup_options::RetailStartupOptions::default(),
    );

    match vera20k::entry::run_with_loop(launch_mode, false, event_loop) {
        Ok(0) => {}
        // Android has no `-SPAWN` status to propagate; a non-zero one still
        // belongs in the log before the activity finishes.
        Ok(status) => log::warn!("VERA20k exited with status {status}"),
        Err(error) => log::error!("VERA20k stopped: {error:#}"),
    }
    log::logger().flush();
}

/// Claims this process's single event loop.
///
/// winit keeps its event loop in a static and panics on a second build, and a
/// panic inside the JNI callback aborts the process with no log line. The first
/// caller wins; every later one is told so the activity can finish quietly.
fn claim_event_loop() -> bool {
    use std::sync::OnceLock;
    static CLAIMED: OnceLock<()> = OnceLock::new();
    CLAIMED.set(()).is_ok()
}
