//! VERA20k — entry point.
//!
//! Runs the CLI-only switches (replay verification, retail data check, language
//! file check, the version, the headless spawn probe and random-map file), then delegates
//! start-up to [`vera20k::entry`], which owns the logging, panic reporting and
//! event-loop work. Android has no `main`; its cdylib (the `vera20k-android` crate in
//! `android/`) reaches the same start-up through `entry::run_with_loop`.
//!
//! Module declarations live in lib.rs so integration tests can import them.

// Release games are GUI applications. Keep the console for development builds;
// the file logger below retains diagnostics when Windows supplies no console.
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use anyhow::Result;
use std::process::ExitCode;

// On Windows the game allocates through mimalloc instead of the process heap
// (Cargo.toml, "Windows memory allocator"). Only this program: the tools and
// the tests keep the system allocator.
#[cfg(windows)]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> Result<ExitCode> {
    // Return through the platform runtime so Windows runs the C runtime's
    // shutdown hooks, including LLVM's profile writer.
    let status = u8::try_from(run()?)?;
    Ok(ExitCode::from(status))
}

fn run() -> Result<i32> {
    // Before any switch below simulates: the pool's workers take simulation
    // items and need the game's stack (`sim::parallel::worker_pool`).
    #[cfg(not(target_arch = "wasm32"))]
    vera20k::sim::parallel::start_worker_pool();
    // `-ra2` selects the original Red Alert 2 non-md INI roots. Record it
    // before any CLI switch below can load config, so every GameConfig::load
    // resolves the same roots. The saved edition is read with VERA.INI
    // (`GameConfig::use_ra2_mode`).
    {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        let refs: Vec<_> = args.iter().map(std::ffi::OsString::as_os_str).collect();
        vera20k::util::ra2_launch::configure(&refs);
    }
    if let Some(result) =
        vera20k::app::frontend::launch::try_replay_verify(std::env::args_os().skip(1))
    {
        return result;
    }
    if let Some(result) =
        vera20k::app::frontend::launch::try_game_data_check(std::env::args_os().skip(1))
    {
        return result;
    }
    if let Some(result) =
        vera20k::app::frontend::launch::try_language_check(std::env::args_os().skip(1))
    {
        return result;
    }
    // docs/spawn.md "Version": this build's version; JSON on stdout.
    if let Some(result) = vera20k::app::frontend::launch::try_version(std::env::args_os().skip(1)) {
        return result;
    }
    // docs/spawn.md: headless launch compatibility probe; JSON on stdout.
    if let Some(result) = vera20k::app::frontend::launch::try_probe(std::env::args_os().skip(1)) {
        return result;
    }
    // docs/spawn.md "Random maps": headless random-map file; JSON on stdout.
    if let Some(result) =
        vera20k::app::frontend::launch::try_generate_map(std::env::args_os().skip(1))
    {
        return result;
    }
    // A launch from the macOS app bundle or the Linux AppImage works in the
    // app's data folder. Enter it before the logger opens logs/ there.
    let app_bundle = vera20k::app::app_bundle::enter_data_folder();
    let log_path = vera20k::entry::init_logging();
    vera20k::entry::log_banner(log_path.as_deref());

    vera20k::entry::finish(vera20k::entry::run(app_bundle))
}
