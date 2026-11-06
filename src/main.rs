//! VERA20k — entry point.
//!
//! Runs the CLI-only switches (replay verification, retail data check and the
//! headless spawn probe), then delegates start-up to [`vera20k::entry`], which
//! owns the logging, panic reporting and event-loop work. Android has no
//! `main`; its cdylib (the `vera20k-android` crate in `android/`) reaches the
//! same start-up through `entry::run_with_loop`.
//!
//! Module declarations live in lib.rs so integration tests can import them.

// Release games are GUI applications. Keep the console for development builds;
// the file logger below retains diagnostics when Windows supplies no console.
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use anyhow::Result;

fn main() -> Result<()> {
    if let Some(result) =
        vera20k::app::frontend::launch::try_replay_verify(std::env::args_os().skip(1))
    {
        std::process::exit(result?);
    }
    if let Some(result) =
        vera20k::app::frontend::launch::try_game_data_check(std::env::args_os().skip(1))
    {
        std::process::exit(result?);
    }
    // docs/spawn.md: headless launch compatibility probe; JSON on stdout.
    if let Some(result) = vera20k::app::frontend::launch::try_probe(std::env::args_os().skip(1)) {
        std::process::exit(result?);
    }
    // A launch from the macOS app bundle or the Linux AppImage works in the
    // app's data folder. Enter it before the logger opens logs/ there.
    let app_bundle = vera20k::app::app_bundle::enter_data_folder();
    let log_path = vera20k::entry::init_logging();
    vera20k::entry::log_banner(log_path.as_deref());

    vera20k::entry::finish(vera20k::entry::run(app_bundle))
}
