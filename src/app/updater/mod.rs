//! The desktop updater.
//!
//! A game built by the Release workflow (`VERA20K_COMMIT`) and started from
//! its release package checks `latest.json` on a worker thread. A newer
//! release whose manifest verifies against the update key is offered once
//! the main menu is idle, in the shell's own message boxes: Update, Skip This
//! Version or Later, beside what changed (`notes`). Update downloads it with
//! progress in the box, checks its size and SHA-256, installs it and restarts
//! into it. A release this program cannot install, after an updater or key
//! change, is offered the same way as a download by hand: from vera20k.org,
//! or after a key change from the GitHub releases (`manifest::Offer`).
//! Nothing is asked during a match, and a check that fails, offline
//! included, stays silent. Vera Options' Check for Updates checks when the
//! player asks, offers a skipped version too, and says what it found, a
//! failure included. When the online lobby says this version is behind, the
//! game checks again and offers a newer release at the next idle main menu,
//! a skipped version too ([`lobby_outdated`]).
//! Publication: `.github/release/downloads.md`.

mod fetch;
mod install;
mod manifest;
pub(crate) mod notes;

use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use crate::app::App;
use crate::app::frontend::state::ShellMessageBoxPurpose;
use crate::app::state::AppState;
use crate::app::web_link::{DOWNLOAD_PAGE, open_in_browser};
use crate::ui::shell::modal::ShellBox;
use crate::util::time::{Duration, Instant};

use fetch::Progress;
use install::Package;
use manifest::{Offer, Release, Version};

/// How often a pending check or download is polled.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Set only for the Release workflow's update test (`tools/update_test.py`):
/// its runners have no retail game data, so no main menu to ask on, and the
/// test build updates as soon as it finds a release it can install.
const UNATTENDED: bool = option_env!("VERA20K_UPDATE_UNATTENDED").is_some();

/// Where [`Offer::NewKey`] sends the player: not vera20k.org, whose download
/// links follow the download server's `latest.json`.
const GITHUB_RELEASES: &str = "https://github.com/YuriPlanet/vera20k/releases";

/// The updater's single owner of its progress through one run of the game.
#[derive(Default)]
pub(crate) struct Updater {
    phase: Phase,
    /// The online lobby said this version is behind ([`lobby_outdated`]): a
    /// newer release waits for the idle main menu even when it was skipped.
    lobby_outdated: bool,
}

impl Updater {
    /// The lobby says this version is behind: a check that ended (nothing
    /// newer then, Later, Skip, a failure) runs again with `check`, and a
    /// check under way or a release waiting for the menu goes on.
    fn lobby_outdated(&mut self, check: impl FnOnce() -> Result<Phase, &'static str>) {
        self.lobby_outdated = true;
        if matches!(self.phase, Phase::Finished)
            && let Ok(phase) = check()
        {
            log::info!("Updater: the lobby says this version is behind; checking again");
            self.phase = phase;
        }
    }

    /// A newer release from a check that no box waits for: it waits for an
    /// idle main menu, unless the player skipped it and the lobby has not
    /// said this version is behind.
    fn found(&mut self, package: Package, offer: Offer, skipped: Option<Version>) {
        self.phase = if skipped == Some(offer.version()) && !self.lobby_outdated {
            log::info!("Updater: VERA20k {} was skipped", offer.version());
            Phase::Finished
        } else {
            Phase::Found { package, offer }
        };
    }
}

#[derive(Default)]
enum Phase {
    /// The first poll decides whether to check.
    #[default]
    NotStarted,
    Checking {
        package: Package,
        result: Receiver<anyhow::Result<Option<Offer>>>,
        /// Vera Options' Check for Updates is waiting in its box: the result
        /// shows there, and a skipped version is offered again.
        asked: bool,
    },
    /// A newer release, waiting for an idle main menu.
    Found { package: Package, offer: Offer },
    /// Its offer is on screen, or the menu is sliding out to show it.
    Offered { package: Package, offer: Offer },
    Downloading {
        release: Release,
        progress: Arc<Progress>,
        result: Receiver<anyhow::Result<Command>>,
    },
    /// No release, declined, failed or restarting: nothing more this run
    /// unless the lobby says this version is behind ([`lobby_outdated`]).
    Finished,
}

/// Advance the updater; the deadline keeps the event loop polling while a
/// worker runs.
pub(crate) fn poll(state: &mut AppState) -> Option<Instant> {
    match std::mem::take(&mut state.frontend.updater.phase) {
        Phase::NotStarted => {
            // A CnCNet-launched match quits at its end, with no main menu to
            // ask at.
            state.frontend.updater.phase = if state.frontend.external_launch {
                Phase::Finished
            } else {
                start(false).unwrap_or(Phase::Finished)
            };
            matches!(state.frontend.updater.phase, Phase::Checking { .. })
                .then(|| Instant::now() + POLL_INTERVAL)
        }
        Phase::Checking {
            package,
            result,
            asked,
        } => match result.try_recv() {
            Ok(Ok(Some(offer))) => {
                let what = match offer {
                    Offer::Update(_) => "is available",
                    Offer::Manual { .. } => "needs a download by hand",
                    Offer::NewKey { .. } => "is signed by another key",
                };
                log::info!("Updater: VERA20k {} {what}", offer.version());
                if asked {
                    state.frontend.updater.phase = Phase::Offered { package, offer };
                    // The checking box gives way to the offer.
                    if state.frontend.shell_message_box.is_some() {
                        App::close_shell_message_box_from_controller(state);
                    }
                    open_offer(state);
                    return None;
                }
                let skipped = Version::parse(&state.persistence.options_profile.skipped_update);
                state.frontend.updater.found(package, offer, skipped);
                poll(state)
            }
            Ok(Ok(None)) => {
                log::info!("Updater: no newer release");
                state.frontend.updater.phase = Phase::Finished;
                if asked {
                    let text =
                        crate::i18n::tr_fill("VERA:UpToDate", &[("version", &Version::current())]);
                    show_notice(state, text);
                }
                None
            }
            Ok(Err(error)) => {
                log::info!("Updater: check failed: {error:#}");
                state.frontend.updater.phase = Phase::Finished;
                if asked {
                    show_notice(state, crate::i18n::tr(CHECK_FAILED));
                }
                None
            }
            Err(TryRecvError::Empty) => {
                state.frontend.updater.phase = Phase::Checking {
                    package,
                    result,
                    asked,
                };
                Some(Instant::now() + POLL_INTERVAL)
            }
            Err(TryRecvError::Disconnected) => {
                state.frontend.updater.phase = Phase::Finished;
                if asked {
                    show_notice(state, crate::i18n::tr(CHECK_FAILED));
                }
                None
            }
        },
        Phase::Found {
            package,
            offer: Offer::Update(release),
        } if UNATTENDED => {
            download(state, package, release);
            Some(Instant::now() + POLL_INTERVAL)
        }
        Phase::Found { package, offer } => {
            if state.main_menu_at_rest() {
                state.frontend.updater.phase = Phase::Offered { package, offer };
                App::leave_shell_dialog(
                    state,
                    crate::app::frontend::shell_transition::ShellSlideKind::MainMenu,
                    crate::app::updater::open_offer,
                );
            } else {
                state.frontend.updater.phase = Phase::Found { package, offer };
            }
            None
        }
        Phase::Downloading {
            release,
            progress,
            result,
        } => match result.try_recv() {
            Ok(Ok(mut finish)) => {
                state.frontend.updater.phase = Phase::Finished;
                restart(state, &release, &mut finish);
                None
            }
            // Whatever stopped a download the player cancelled, say nothing.
            Ok(Err(_)) if progress.cancel.load(Ordering::Relaxed) => {
                state.frontend.updater.phase = Phase::Finished;
                None
            }
            Ok(Err(error)) => {
                log::warn!("Updater: updating to {} failed: {error:#}", release.version);
                state.frontend.updater.phase = Phase::Finished;
                show_failure(state, release.version, &error);
                None
            }
            Err(TryRecvError::Empty) => {
                if let Some(message_box) = state.frontend.shell_message_box.as_mut()
                    && matches!(message_box.purpose, ShellMessageBoxPurpose::UpdateProgress)
                {
                    message_box.shell_box.body = progress_text(
                        release.version,
                        progress.received.load(Ordering::Relaxed),
                        release.size,
                    );
                }
                state.frontend.updater.phase = Phase::Downloading {
                    release,
                    progress,
                    result,
                };
                Some(Instant::now() + POLL_INTERVAL)
            }
            Err(TryRecvError::Disconnected) => {
                state.frontend.updater.phase = Phase::Finished;
                None
            }
        },
        phase => {
            state.frontend.updater.phase = phase;
            notes::poll_scroll(state)
        }
    }
}

/// Start the check when this is a release build running from its package;
/// otherwise the label of what Check for Updates says instead.
fn start(asked: bool) -> Result<Phase, &'static str> {
    if option_env!("VERA20K_COMMIT").is_none_or(str::is_empty) {
        return Err("VERA:UpdateFromSource");
    }
    let (Some(platform), Some(package)) = (manifest::platform(), install::package()) else {
        return Err("VERA:UpdateNotInstalled");
    };
    let (sender, result) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("vera-update-check".into())
        .spawn(move || {
            let _ = sender.send(fetch::check(Version::current(), platform));
        });
    match spawned {
        Ok(_) => Ok(Phase::Checking {
            package,
            result,
            asked,
        }),
        Err(error) => {
            log::warn!("Updater: cannot start the check: {error}");
            Err(CHECK_FAILED)
        }
    }
}

/// The label of what Check for Updates says when the check fails, offline
/// included.
const CHECK_FAILED: &str = "VERA:UpdateCheckFailed";

/// Vera Options' Check for Updates, once its page has slid out: an offer
/// waiting for the main menu shows now, and otherwise a check (the one
/// under way, or a new one) answers in a box, offering a skipped version too.
pub(crate) fn open_check(state: &mut AppState) {
    // Over the empty shell backdrop, as the offer asks.
    App::destroy_current_shell_dialog(state);
    match std::mem::take(&mut state.frontend.updater.phase) {
        Phase::Found { package, offer } => {
            state.frontend.updater.phase = Phase::Offered { package, offer };
            open_offer(state);
        }
        Phase::Checking {
            package, result, ..
        } => {
            state.frontend.updater.phase = Phase::Checking {
                package,
                result,
                asked: true,
            };
            open_checking(state);
        }
        Phase::NotStarted | Phase::Finished => match start(true) {
            Ok(phase) => {
                state.frontend.updater.phase = phase;
                open_checking(state);
            }
            Err(reason) => {
                state.frontend.updater.phase = Phase::Finished;
                show_notice(state, crate::i18n::tr(reason));
            }
        },
        // A box of the updater's own is up already.
        phase @ (Phase::Offered { .. } | Phase::Downloading { .. }) => {
            state.frontend.updater.phase = phase;
        }
    }
}

fn open_checking(state: &mut AppState) {
    let cancel = state.csf_label("TXT_CANCEL", "Cancel");
    App::open_shell_message_box(
        state,
        ShellBox::notice(crate::i18n::tr("VERA:UpdateChecking"), cancel),
        ShellMessageBoxPurpose::UpdateCheck,
    );
}

/// The checking box's button, Esc or Enter: stop waiting. The check still
/// ends as the one at startup does, offering a newer release at the idle
/// main menu unless it was skipped and the lobby has not said this version
/// is behind.
pub(crate) fn cancel_check(state: &mut AppState) {
    if let Phase::Checking { asked, .. } = &mut state.frontend.updater.phase {
        *asked = false;
    }
}

/// The online lobby says this version is behind the newest release: its 426
/// at sign-in or to a signed-in request, or its new-release line. The lobby
/// lists only rooms of the player's own version and Quick Match pairs only
/// equal ones, so the game checks as Check for Updates does: a newer release
/// is offered at the next idle main menu, even a skipped one. A
/// CnCNet-launched match has no main menu to ask at.
pub(crate) fn lobby_outdated(state: &mut AppState) {
    if !state.frontend.external_launch {
        state.frontend.updater.lobby_outdated(|| start(false));
    }
}

/// The menu has slid out: ask over the empty shell backdrop, as Exit Game
/// does (`App::open_exit_confirm`).
pub(crate) fn open_offer(state: &mut AppState) {
    use crate::i18n::{tr, tr_fill};
    let Phase::Offered { offer, .. } = &state.frontend.updater.phase else {
        return;
    };
    let (question, confirm, skip, changes) = match offer {
        Offer::Update(release) => (
            "VERA:UpdateQuestion",
            "VERA:Update",
            Some("VERA:SkipVersion"),
            release.notes.as_slice(),
        ),
        Offer::Manual { notes, .. } => (
            "VERA:UpdateByHand",
            "VERA:Download",
            Some("VERA:SkipVersion"),
            notes.as_slice(),
        ),
        // Unverified: its version is not one to skip.
        Offer::NewKey { .. } => ("VERA:UpdateNewKey", "VERA:Download", None, &[][..]),
    };
    let shell_box = ShellBox::message(
        tr_fill(
            "VERA:UpdateAvailable",
            &[
                ("version", &offer.version()),
                ("current", &Version::current()),
                ("question", &tr(question)),
            ],
        ),
        tr(confirm),
        tr("VERA:Later"),
        skip.map(tr).unwrap_or_default(),
    );
    let notes = notes::ReleaseNotes::new(&state.renderer.bit_font, changes);
    App::destroy_current_shell_dialog(state);
    App::open_shell_message_box(state, shell_box, ShellMessageBoxPurpose::UpdateOffer(notes));
}

/// The offer's answer: 0 Update (or Download), 2 Skip This Version, 1 (or
/// Esc/Enter) Later.
pub(crate) fn answer_offer(state: &mut AppState, answer: usize) {
    let Phase::Offered { package, offer } = std::mem::take(&mut state.frontend.updater.phase)
    else {
        return;
    };
    state.frontend.updater.phase = Phase::Finished;
    match (answer, offer) {
        (0, Offer::Update(release)) => download(state, package, release),
        (0, Offer::Manual { .. }) => open_in_browser(DOWNLOAD_PAGE),
        (0, Offer::NewKey { .. }) => open_in_browser(GITHUB_RELEASES),
        (2, offer @ (Offer::Update(_) | Offer::Manual { .. })) => {
            let profile = &mut state.persistence.options_profile;
            profile.skipped_update = offer.version().to_string();
            if let Some(config) = state.platform.game_config.as_ref()
                && let Err(error) = profile.commit_skipped_update(config.paths.settings_dir())
            {
                log::warn!("Updater: cannot save the skipped version: {error}");
            }
        }
        _ => {}
    }
}

fn download(state: &mut AppState, package: Package, release: Release) {
    let progress = Arc::new(Progress::default());
    let (sender, result) = mpsc::channel();
    let worker = {
        let progress = Arc::clone(&progress);
        let release = release.clone();
        move || {
            let finish = install::check_replaceable(&package)
                .and_then(|()| install::download_folder())
                .and_then(|folder| {
                    let _ = std::fs::remove_dir_all(&folder);
                    fetch::download(&release, &folder, &progress)
                })
                .and_then(|installer| {
                    // Past this point the update is committed: Cancel no longer applies.
                    anyhow::ensure!(!progress.cancel.load(Ordering::Relaxed), fetch::Cancelled);
                    install::stage(&package, &installer)
                });
            let _ = sender.send(finish);
        }
    };
    if let Err(error) = std::thread::Builder::new()
        .name("vera-update-download".into())
        .spawn(worker)
    {
        show_failure(state, release.version, &anyhow::Error::from(error));
        return;
    }
    open_progress(state, &release, 0);
    state.frontend.updater.phase = Phase::Downloading {
        release,
        progress,
        result,
    };
}

fn open_progress(state: &mut AppState, release: &Release, received: u64) {
    let cancel = state.csf_label("TXT_CANCEL", "Cancel");
    App::open_shell_message_box(
        state,
        ShellBox::notice(
            progress_text(release.version, received, release.size),
            cancel,
        ),
        ShellMessageBoxPurpose::UpdateProgress,
    );
}

/// The progress box's button, Esc or Enter. It stops the download; the
/// worker's result still ends the update, so an install already under way
/// finishes and restarts, and its box stays up until then.
pub(crate) fn cancel_download(state: &mut AppState) {
    let Phase::Downloading {
        release, progress, ..
    } = &state.frontend.updater.phase
    else {
        return;
    };
    let received = progress.received.load(Ordering::Relaxed);
    if received >= release.size {
        let release = release.clone();
        open_progress(state, &release, received);
        return;
    }
    progress.cancel.store(true, Ordering::Relaxed);
    log::info!("Updater: download cancelled");
}

fn progress_text(version: Version, received: u64, size: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    if received >= size {
        return crate::i18n::tr_fill("VERA:UpdateInstalling", &[("version", &version)]);
    }
    crate::i18n::tr_fill(
        "VERA:UpdateDownloading",
        &[
            ("version", &version),
            ("received", &format!("{:.1}", received as f64 / MIB)),
            ("size", &format!("{:.1}", size as f64 / MIB)),
            ("percent", &(received * 100 / size.max(1))),
        ],
    )
}

/// The update is in place (or its installer is ready): start what finishes
/// it, then quit the way Exit Game does.
fn restart(state: &mut AppState, release: &Release, finish: &mut Command) {
    match finish.spawn() {
        Ok(child) => {
            // The macOS and Linux helper starts the new version when its
            // standard input ends, which this process's exit closes.
            std::mem::forget(child);
            log::info!("Updater: restarting into VERA20k {}", release.version);
            App::confirm_quit(state);
        }
        Err(error) => {
            log::warn!("Updater: cannot start {:?}: {error}", finish.get_program());
            show_failure(state, release.version, &anyhow::Error::from(error));
        }
    }
}

fn show_failure(state: &mut AppState, version: Version, error: &anyhow::Error) {
    let reason = error.downcast_ref::<install::Hint>().map_or_else(
        || crate::i18n::tr("VERA:UpdateTryLater"),
        |hint| hint.0.clone(),
    );
    show_notice(
        state,
        crate::i18n::tr_fill(
            "VERA:UpdateFailed",
            &[("version", &version), ("reason", &reason)],
        ),
    );
}

/// An OK box in place of the updater's open one: a failure, or what Check
/// for Updates found.
fn show_notice(state: &mut AppState, text: String) {
    App::open_shell_notice(state, text);
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    fn skipped_release() -> (Package, Offer, Option<Version>) {
        let version = Version::parse("99.0.0").unwrap();
        let offer = Offer::Manual {
            version,
            notes: Vec::new(),
        };
        (Package::AppImage(Default::default()), offer, Some(version))
    }

    /// The lobby's word that this version is behind checks again after the
    /// startup check ended, and its release is offered at the main menu
    /// although the player skipped it; without that word a skipped release
    /// stays silent.
    #[test]
    fn a_lobby_outdated_checks_again_and_offers_a_skipped_release() {
        let (package, offer, skipped) = skipped_release();
        let mut startup = Updater::default();
        startup.found(package.clone(), offer.clone(), skipped);
        assert!(matches!(startup.phase, Phase::Finished));

        let (_sender, result) = mpsc::channel();
        startup.lobby_outdated(|| {
            Ok(Phase::Checking {
                package: package.clone(),
                result,
                asked: false,
            })
        });
        assert!(matches!(
            startup.phase,
            Phase::Checking { asked: false, .. }
        ));
        startup.found(package, offer, skipped);
        assert!(matches!(startup.phase, Phase::Found { .. }));
    }
}
