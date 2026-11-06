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
//! failure included.
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
use crate::app::frontend::state::{ShellMessageBox, ShellMessageBoxPurpose};
use crate::app::state::AppState;
use crate::app::web_link::{WEBSITE, open_in_browser};
use crate::ui::main_menu_dialogs::ShellMessageBoxLabels;
use crate::ui::shell::modal::ModalResult;
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

/// The updater's single owner of its progress through one run of the game.
#[derive(Default)]
pub(crate) struct Updater {
    phase: Phase,
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
    /// Nothing more this run: no release, declined, failed or restarting.
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
                if skipped == Some(offer.version()) {
                    log::info!("Updater: VERA20k {} was skipped", offer.version());
                    state.frontend.updater.phase = Phase::Finished;
                    return None;
                }
                state.frontend.updater.phase = Phase::Found { package, offer };
                poll(state)
            }
            Ok(Ok(None)) => {
                log::info!("Updater: no newer release");
                state.frontend.updater.phase = Phase::Finished;
                if asked {
                    let text = format!("VERA20K {} is up to date.", Version::current());
                    show_notice(state, text);
                }
                None
            }
            Ok(Err(error)) => {
                log::info!("Updater: check failed: {error:#}");
                state.frontend.updater.phase = Phase::Finished;
                if asked {
                    show_notice(state, CHECK_FAILED.to_owned());
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
                    show_notice(state, CHECK_FAILED.to_owned());
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
            if main_menu_idle(state) {
                state.frontend.updater.phase = Phase::Offered { package, offer };
                App::leave_shell_dialog(
                    state,
                    crate::app::frontend::shell_transition::ShellExitThen::UpdateOffer,
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
                    message_box.labels.title = progress_text(
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
/// otherwise what Check for Updates says instead.
fn start(asked: bool) -> Result<Phase, &'static str> {
    if option_env!("VERA20K_COMMIT").is_none_or(str::is_empty) {
        return Err(
            "This VERA20K was built from source, so it does not update itself.\n\n\
             Download releases from vera20k.org.",
        );
    }
    let (Some(platform), Some(package)) = (manifest::platform(), install::package()) else {
        return Err(
            "This copy is not an installed VERA20K release, so it does not update \
             itself.\n\nDownload releases from vera20k.org.",
        );
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

/// What Check for Updates says when the check fails, offline included.
const CHECK_FAILED: &str = "VERA20K could not check for updates.\n\nTry again later.";

/// Vera Options' Check for Updates, once its page has slid out: an offer
/// waiting for the main menu shows now, and otherwise a check (the one
/// under way, or a new one) answers in a box, offering a skipped version too.
pub(crate) fn open_check(state: &mut AppState) {
    // Over the empty shell backdrop, as the offer asks.
    crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
    crate::app::frontend::shell_transition::invalidate_main_menu_dialog_instance(state);
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
                show_notice(state, reason.to_owned());
            }
        },
        // A box of the updater's own is up already.
        phase @ (Phase::Offered { .. } | Phase::Downloading { .. }) => {
            state.frontend.updater.phase = phase;
        }
    }
}

fn open_checking(state: &mut AppState) {
    let cancel = App::csf_label(state, "TXT_CANCEL", "Cancel");
    App::open_shell_message_box(
        state,
        ShellMessageBox {
            labels: ShellMessageBoxLabels {
                title: "Checking for a newer VERA20K...".to_owned(),
                confirm: cancel,
                third: String::new(),
                cancel: String::new(),
            },
            purpose: ShellMessageBoxPurpose::UpdateCheck,
        },
    );
}

/// The checking box's button, Esc or Enter: stop waiting. The check still
/// ends as the one at startup does, offering a newer release at the idle
/// main menu unless it was skipped.
pub(crate) fn cancel_check(state: &mut AppState) {
    if let Phase::Checking { asked, .. } = &mut state.frontend.updater.phase {
        *asked = false;
    }
    state.platform.window.request_redraw();
}

/// The offer waits for the main menu at rest: its page takes input (not
/// sliding, no movie, page or error over it), past the splash, with no dialog.
fn main_menu_idle(state: &AppState) -> bool {
    use crate::app::input::precedence::{InputLayer, ScreenPage, input_layer};
    input_layer(state) == InputLayer::Screen(ScreenPage::MainMenu)
        && state.frontend.shell_route == crate::app::shell_route::ShellRoute::MainMenu
        && state.frontend.startup_splash.is_none()
        && !state.main_menu_dialog_open()
}

/// The menu has slid out: ask over the empty shell backdrop, as Exit Game
/// does (`App::open_exit_confirm`).
pub(crate) fn open_offer(state: &mut AppState) {
    let Phase::Offered { offer, .. } = &state.frontend.updater.phase else {
        return;
    };
    let (question, confirm, skip, changes) = match offer {
        Offer::Update(release) => (
            "Update now? VERA20K restarts when it is done.",
            "Update",
            "Skip Version",
            release.notes.as_slice(),
        ),
        Offer::Manual { notes, .. } => (
            "This copy cannot install it by itself. Download it from vera20k.org.",
            "Download",
            "Skip Version",
            notes.as_slice(),
        ),
        // Unverified: its version is not one to skip.
        Offer::NewKey { .. } => (
            "This copy cannot install it by itself. Download it from GitHub.",
            "Download",
            "",
            &[][..],
        ),
    };
    let labels = ShellMessageBoxLabels {
        title: format!(
            "VERA20K {} is available. You have {}.\n\n{question}",
            offer.version(),
            Version::current()
        ),
        confirm: confirm.to_owned(),
        third: skip.to_owned(),
        cancel: "Later".to_owned(),
    };
    let notes = notes::ReleaseNotes::new(&state.renderer.bit_font, changes);
    crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
    crate::app::frontend::shell_transition::invalidate_main_menu_dialog_instance(state);
    App::open_shell_message_box(
        state,
        ShellMessageBox {
            labels,
            purpose: ShellMessageBoxPurpose::UpdateOffer(notes),
        },
    );
}

/// The offer's answer: 0 Update (or Download), 2 Skip This Version, 1 (or
/// Esc/Enter) Later.
pub(crate) fn answer_offer(state: &mut AppState, result: ModalResult) {
    let Phase::Offered { package, offer } = std::mem::take(&mut state.frontend.updater.phase)
    else {
        return;
    };
    state.frontend.updater.phase = Phase::Finished;
    match (result, offer) {
        (ModalResult::MessageBox(0), Offer::Update(release)) => download(state, package, release),
        (ModalResult::MessageBox(0), Offer::Manual { .. }) => open_in_browser(WEBSITE),
        (ModalResult::MessageBox(0), Offer::NewKey { .. }) => {
            open_in_browser(crate::app::shell_wol::RELEASES_URL)
        }
        (ModalResult::MessageBox(2), offer @ (Offer::Update(_) | Offer::Manual { .. })) => {
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
    state.platform.window.request_redraw();
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
    let cancel = App::csf_label(state, "TXT_CANCEL", "Cancel");
    App::open_shell_message_box(
        state,
        ShellMessageBox {
            labels: ShellMessageBoxLabels {
                title: progress_text(release.version, received, release.size),
                confirm: cancel,
                third: String::new(),
                cancel: String::new(),
            },
            purpose: ShellMessageBoxPurpose::UpdateProgress,
        },
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
    state.platform.window.request_redraw();
}

fn progress_text(version: Version, received: u64, size: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    if received >= size {
        return format!("Installing VERA20K {version}...");
    }
    format!(
        "Downloading VERA20K {version}...\n\n{:.1} of {:.1} MB ({}%)",
        received as f64 / MIB,
        size as f64 / MIB,
        received * 100 / size.max(1)
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
    let reason = error.downcast_ref::<install::Hint>().map_or(
        "Try again later, or download it from vera20k.org.",
        |hint| hint.0,
    );
    show_notice(
        state,
        format!("VERA20K could not update to {version}.\n\n{reason}"),
    );
}

/// An OK box in place of the updater's open one: a failure, or what Check
/// for Updates found.
fn show_notice(state: &mut AppState, text: String) {
    if state.frontend.shell_message_box.is_some() {
        App::close_shell_message_box_from_controller(state);
    }
    let ok = App::csf_label(state, "TXT_OK", "OK");
    App::open_shell_message_box(
        state,
        ShellMessageBox {
            labels: ShellMessageBoxLabels {
                title: text,
                confirm: ok,
                third: String::new(),
                cancel: String::new(),
            },
            purpose: ShellMessageBoxPurpose::UpdateNotice,
        },
    );
}
