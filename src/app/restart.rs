//! Starting the game again once this copy has shut down, for a change that
//! only a fresh start applies: the active mod or game edition.
//!
//! The copy that is leaving first saves its settings, lets go of its files and
//! closes its log, as on any Exit Game; starting the next copy is the last
//! thing it does ([`start_again_if_asked`], from `entry::finish`). So the two
//! never write VERA.INI against each other, and Windows' one-copy gate
//! (`util::single_instance`) is open again. The updater's restart
//! (`updater::install`) is its own: it also puts the new version in place.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy)]
#[repr(u8)]
enum Restart {
    Mod = 1,
    Edition,
}

/// The request survives until `entry::finish`, after the app state is gone.
static ASKED: AtomicU8 = AtomicU8::new(0);

/// Start the game again when this copy has shut down. The caller then leaves
/// the way Exit Game does.
pub(crate) fn ask() {
    ASKED.store(Restart::Mod as u8, Ordering::Relaxed);
}

/// Use the saved edition instead of repeating a launch's `-ra2` override.
pub(crate) fn ask_for_edition() {
    ASKED.store(Restart::Edition as u8, Ordering::Relaxed);
}

/// The last act of a copy that shut down in the ordinary way: start the next
/// one, when [`ask`] was called. A failure only goes to the log; the choice
/// that asked for the restart is kept, and the next start by hand uses it.
pub(crate) fn start_again_if_asked() {
    // Android starts and ends an app by its own rules.
    let restart = match ASKED.swap(0, Ordering::Relaxed) {
        x if x == Restart::Mod as u8 => Restart::Mod,
        x if x == Restart::Edition as u8 => Restart::Edition,
        _ => return,
    };
    if cfg!(target_os = "android") {
        return;
    }
    let started = std::env::current_exe().and_then(|executable| {
        command(
            restart,
            std::env::args_os().skip(1).collect(),
            crate::app::app_bundle::package_path(),
            executable,
        )
        .spawn()
    });
    match started {
        Ok(_) => log::info!("Starting VERA20k again"),
        Err(error) => log::warn!("Could not start VERA20k again: {error}"),
    }
    log::logger().flush();
}

/// Relaunch with the unrelated arguments intact. Only drop the switch that
/// would override the choice just saved.
/// `package` is the app bundle or AppImage this copy runs from, if any.
fn command(
    restart: Restart,
    args: Vec<OsString>,
    package: Option<PathBuf>,
    executable: PathBuf,
) -> Command {
    let args = match restart {
        Restart::Mod => {
            crate::app::mods::take_switch(args).map_or_else(|_| Vec::new(), |(_, rest)| rest)
        }
        Restart::Edition => args
            .into_iter()
            .filter(|arg| !arg.eq_ignore_ascii_case("-ra2"))
            .collect(),
    };
    let mut command = match package {
        // The app bundle: a new copy, brought forward, while this one is
        // still on its way out.
        Some(bundle) if bundle.extension().is_some_and(|kind| kind == "app") => {
            let mut open = Command::new("/usr/bin/open");
            open.arg("-n").arg(bundle);
            if !args.is_empty() {
                open.arg("--args");
            }
            open
        }
        Some(image) => crate::app::app_bundle::app_image_start(&image),
        None => Command::new(executable),
    };
    command.args(args);
    command
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    #[test]
    fn edition_restart_removes_only_the_ra2_override() {
        let args = ["-Ra2", "--mod", "Winter War", "-WIN", "-ra2"]
            .map(Into::into)
            .to_vec();
        let next = command(Restart::Edition, args, None, "/games/vera20k".into());
        assert_eq!(
            next.get_args().collect::<Vec<_>>(),
            ["--mod", "Winter War", "-WIN"]
        );
    }

    /// A copy started with `--mod` must not hand the switch on: it would win
    /// over the choice the Mods page just kept.
    #[test]
    fn the_next_copy_starts_as_this_one_did_without_its_mod_switch() {
        let args: Vec<OsString> = ["--mod", "Winter War", "-WIN"].map(Into::into).to_vec();
        let plain = command(Restart::Mod, args.clone(), None, "/games/vera20k".into());
        assert_eq!(plain.get_program(), "/games/vera20k");
        assert_eq!(plain.get_args().collect::<Vec<_>>(), ["-WIN"]);

        let bundle = "/Applications/VERA20K.app";
        let opened = command(Restart::Mod, args, Some(bundle.into()), "/unused".into());
        assert_eq!(opened.get_program(), "/usr/bin/open");
        assert_eq!(
            opened.get_args().collect::<Vec<_>>(),
            ["-n", bundle, "--args", "-WIN"]
        );

        let image = command(
            Restart::Mod,
            Vec::new(),
            Some("/games/VERA20K.AppImage".into()),
            "/unused".into(),
        );
        assert_eq!(image.get_program(), "/games/VERA20K.AppImage");
        assert!(
            image
                .get_envs()
                .any(|(name, value)| name == "APPIMAGE" && value.is_none())
        );
    }
}
