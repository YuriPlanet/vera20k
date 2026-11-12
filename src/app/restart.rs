//! Starting the game again once this copy has shut down, for a change that
//! only a fresh start applies: the Mods page's choice (`app::mods`).
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
use std::sync::atomic::{AtomicBool, Ordering};

static ASKED: AtomicBool = AtomicBool::new(false);

/// Start the game again when this copy has shut down. The caller then leaves
/// the way Exit Game does.
pub(crate) fn ask() {
    ASKED.store(true, Ordering::Relaxed);
}

/// The last act of a copy that shut down in the ordinary way: start the next
/// one, when [`ask`] was called. A failure only goes to the log; the choice
/// that asked for the restart is kept, and the next start by hand uses it.
pub(crate) fn start_again_if_asked() {
    // Android starts and ends an app by its own rules.
    if !ASKED.load(Ordering::Relaxed) || cfg!(target_os = "android") {
        return;
    }
    let started = std::env::current_exe().and_then(|executable| {
        command(
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

/// What starts this program the way it was started (`args`), without its
/// `--mod` or `--no-mod` switch: the choice just kept in config.toml decides.
/// `package` is the app bundle or AppImage this copy runs from, if any.
fn command(args: Vec<OsString>, package: Option<PathBuf>, executable: PathBuf) -> Command {
    let args = crate::app::mods::take_switch(args).map_or_else(|_| Vec::new(), |(_, rest)| rest);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A copy started with `--mod` must not hand the switch on: it would win
    /// over the choice the Mods page just kept.
    #[test]
    fn the_next_copy_starts_as_this_one_did_without_its_mod_switch() {
        let args: Vec<OsString> = ["--mod", "Winter War", "-WIN"].map(Into::into).to_vec();
        let plain = command(args.clone(), None, "/games/vera20k".into());
        assert_eq!(plain.get_program(), "/games/vera20k");
        assert_eq!(plain.get_args().collect::<Vec<_>>(), ["-WIN"]);

        let bundle = "/Applications/VERA20K.app";
        let opened = command(args, Some(bundle.into()), "/unused".into());
        assert_eq!(opened.get_program(), "/usr/bin/open");
        assert_eq!(
            opened.get_args().collect::<Vec<_>>(),
            ["-n", bundle, "--args", "-WIN"]
        );

        let image = command(
            Vec::new(),
            Some("/home/player/VERA20K.AppImage".into()),
            "/unused".into(),
        );
        assert_eq!(image.get_program(), "/home/player/VERA20K.AppImage");
        assert!(
            image
                .get_envs()
                .any(|(name, value)| name == "APPIMAGE" && value.is_none())
        );
    }
}
