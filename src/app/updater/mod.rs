//! The desktop updater.
//!
//! A game built by the Release workflow (`VERA20K_COMMIT`) and started from
//! its release package checks `latest.json` on a worker thread from the end
//! of startup ([`launch`]). A newer release whose manifest verifies against
//! the update key is offered before the main menu first shows, which waits
//! for the check up to [`MENU_HOLD`], or else once the main menu is idle, in
//! the shell's own message boxes: Update, Skip This Version or Later, beside
//! what changed (`notes`). Update downloads it with
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
use crate::app::PlatformState;
use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::state::FrontendState;
use crate::app::frontend::state::ShellMessageBoxPurpose;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::AppState;
use crate::app::web_link::{DOWNLOAD_PAGE, open_in_browser};
use crate::ui::shell::modal::ShellBox;
use crate::util::time::{Duration, Instant};

use fetch::Progress;
use install::Package;
use manifest::{Offer, Release, Version};

/// How often a pending check or download is polled.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// VERA: how long the main menu's first entry waits for the startup check
/// over the empty backdrop ([`Updater::holds_main_menu`]). The check answers
/// in well under a second when online; a slower one lets the menu slide in,
/// and a release it finds later waits for the idle menu.
const MENU_HOLD: Duration = Duration::from_secs(2);

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
    /// The run's first check, at the end of startup: the main menu has not
    /// shown yet, so it waits while the check runs, at most [`MENU_HOLD`].
    fn begin(&mut self, mut phase: Phase, now: Instant) {
        if let Phase::Checking { menu_hold, .. } = &mut phase {
            *menu_hold = Some(now + MENU_HOLD);
        }
        self.phase = phase;
    }

    /// VERA: the main menu's first entry waits for the startup check: no
    /// slide-in, the empty backdrop and no shell input, so a newer release is
    /// asked about before the menu slides in rather than after it slid in
    /// and out again.
    pub(crate) fn holds_main_menu(&self) -> bool {
        matches!(
            self.phase,
            Phase::Checking {
                menu_hold: Some(_),
                ..
            }
        )
    }

    /// The startup check still runs at `now`: the hold ends once its time is
    /// up. Returns when to poll next.
    fn wait_for_check(&mut self, now: Instant) -> Instant {
        let next = now + POLL_INTERVAL;
        if let Phase::Checking { menu_hold, .. } = &mut self.phase {
            match *menu_hold {
                Some(until) if now < until => return next.min(until),
                _ => *menu_hold = None,
            }
        }
        next
    }

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

    /// A newer release from a check that no box waits for: it waits for the
    /// main menu, unless the player skipped it and the lobby has not said
    /// this version is behind.
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
    /// Until startup ends ([`launch`]); shell captures and hidden runs never
    /// check.
    #[default]
    NotStarted,
    Checking {
        package: Package,
        result: Receiver<anyhow::Result<Option<Offer>>>,
        /// Vera Options' Check for Updates is waiting in its box: the result
        /// shows there, and a skipped version is offered again.
        asked: bool,
        /// Until when the main menu's first entry waits for this check: the
        /// startup check only ([`Updater::begin`]).
        menu_hold: Option<Instant>,
    },
    /// A newer release, waiting for the main menu: before it first shows, or
    /// once it is idle.
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

/// Start the run's check as startup ends, before the shell's first frame, in
/// a player's run (not a shell capture or hidden run): the main menu's first
/// entry waits for it ([`Updater::holds_main_menu`]). A CnCNet-launched match
/// quits at its end, with no main menu to ask at.
pub(crate) fn launch(frontend: &mut crate::app::frontend::state::FrontendState) {
    let phase = if frontend.external_launch {
        Phase::Finished
    } else {
        start(false).unwrap_or(Phase::Finished)
    };
    frontend.updater.begin(phase, Instant::now());
}

/// Advance the updater; the deadline keeps the event loop polling while a
/// worker runs.
pub(crate) fn poll(state: &mut AppState) -> Option<Instant> {
    let held = state.frontend.updater.holds_main_menu();
    let next = advance(state);
    // The held menu draws nothing that moves: draw the frame that shows the
    // offer's backdrop or starts the slide-in.
    if held && !state.frontend.updater.holds_main_menu() {
        state.platform.window.request_redraw();
    }
    next
}

fn advance(state: &mut AppState) -> Option<Instant> {
    match std::mem::take(&mut state.frontend.updater.phase) {
        Phase::Checking {
            package,
            result,
            asked,
            menu_hold,
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
                        App::close_shell_message_box_from_controller(&mut state.frontend);
                    }
                    open_offer(
                        &state.platform.window,
                        &state.renderer,
                        &mut state.frontend,
                        &state.process_assets,
                        &state.persistence.options_profile,
                    );
                    return None;
                }
                let skipped = Version::parse(&state.persistence.options_profile.skipped_update);
                state.frontend.updater.found(package, offer, skipped);
                advance(state)
            }
            Ok(Ok(None)) => {
                log::info!("Updater: no newer release");
                state.frontend.updater.phase = Phase::Finished;
                if asked {
                    let text =
                        crate::i18n::tr_fill("VERA:UpToDate", &[("version", &Version::current())]);
                    show_notice(
                        &state.platform.window,
                        &state.renderer,
                        &mut state.frontend,
                        &state.process_assets,
                        &state.persistence.options_profile,
                        text,
                    );
                }
                None
            }
            Ok(Err(error)) => {
                log::info!("Updater: check failed: {error:#}");
                state.frontend.updater.phase = Phase::Finished;
                if asked {
                    show_notice(
                        &state.platform.window,
                        &state.renderer,
                        &mut state.frontend,
                        &state.process_assets,
                        &state.persistence.options_profile,
                        crate::i18n::tr(CHECK_FAILED),
                    );
                }
                None
            }
            Err(TryRecvError::Empty) => {
                state.frontend.updater.phase = Phase::Checking {
                    package,
                    result,
                    asked,
                    menu_hold,
                };
                Some(state.frontend.updater.wait_for_check(Instant::now()))
            }
            Err(TryRecvError::Disconnected) => {
                state.frontend.updater.phase = Phase::Finished;
                if asked {
                    show_notice(
                        &state.platform.window,
                        &state.renderer,
                        &mut state.frontend,
                        &state.process_assets,
                        &state.persistence.options_profile,
                        crate::i18n::tr(CHECK_FAILED),
                    );
                }
                None
            }
        },
        Phase::Found {
            package,
            offer: Offer::Update(release),
        } if UNATTENDED => {
            download(
                &state.platform.window,
                &state.renderer,
                &mut state.frontend,
                &state.process_assets,
                &state.persistence.options_profile,
                package,
                release,
            );
            Some(Instant::now() + POLL_INTERVAL)
        }
        Phase::Found { package, offer } => {
            // Before the menu shows, the slide-out below is skipped and the
            // offer opens at once.
            if crate::app::state::main_menu_at_rest(
                &state.platform,
                &state.renderer,
                &state.frontend,
                &state.match_state,
                &state.persistence,
            ) || crate::app::state::main_menu_not_shown(&state.frontend)
            {
                state.frontend.updater.phase = Phase::Offered { package, offer };
                App::leave_shell_dialog(
                    state,
                    crate::app::frontend::shell_transition::ShellSlideKind::MainMenu,
                    |state| {
                        crate::app::updater::open_offer(
                            &state.platform.window,
                            &state.renderer,
                            &mut state.frontend,
                            &state.process_assets,
                            &state.persistence.options_profile,
                        )
                    },
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
                restart(
                    &state.platform,
                    &state.renderer,
                    &state.diag,
                    &mut state.frontend,
                    &mut state.match_state,
                    &state.process_assets,
                    &mut state.audio,
                    &state.persistence,
                    &release,
                    &mut finish,
                );
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
                show_failure(
                    &state.platform.window,
                    &state.renderer,
                    &mut state.frontend,
                    &state.process_assets,
                    &state.persistence.options_profile,
                    release.version,
                    &error,
                );
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
            notes::poll_scroll(&state.platform, &state.renderer.gpu, &mut state.frontend)
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
            menu_hold: None,
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
pub(crate) fn open_check(
    window: &winit::window::Window,
    renderer: &crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    process_assets: &crate::app::process_assets::ProcessAssets,
    options_profile: &crate::app::persistence::options_profile::RetailOptionsProfile,
) {
    // Over the empty shell backdrop, as the offer asks.
    App::destroy_current_shell_dialog(frontend);
    match std::mem::take(&mut frontend.updater.phase) {
        Phase::Found { package, offer } => {
            frontend.updater.phase = Phase::Offered { package, offer };
            open_offer(window, renderer, frontend, process_assets, options_profile);
        }
        Phase::Checking {
            package, result, ..
        } => {
            // The player reached Options, so the menu has shown.
            frontend.updater.phase = Phase::Checking {
                package,
                result,
                asked: true,
                menu_hold: None,
            };
            open_checking(window, renderer, frontend, process_assets, options_profile);
        }
        Phase::NotStarted | Phase::Finished => match start(true) {
            Ok(phase) => {
                frontend.updater.phase = phase;
                open_checking(window, renderer, frontend, process_assets, options_profile);
            }
            Err(reason) => {
                frontend.updater.phase = Phase::Finished;
                show_notice(
                    window,
                    renderer,
                    frontend,
                    process_assets,
                    options_profile,
                    crate::i18n::tr(reason),
                );
            }
        },
        // A box of the updater's own is up already.
        phase @ (Phase::Offered { .. } | Phase::Downloading { .. }) => {
            frontend.updater.phase = phase;
        }
    }
}

fn open_checking(
    window: &winit::window::Window,
    renderer: &crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    process_assets: &crate::app::process_assets::ProcessAssets,
    options_profile: &crate::app::persistence::options_profile::RetailOptionsProfile,
) {
    let cancel = process_assets.csf_label("TXT_CANCEL", "Cancel");
    App::open_shell_message_box(
        window,
        renderer,
        frontend,
        process_assets,
        options_profile,
        ShellBox::notice(crate::i18n::tr("VERA:UpdateChecking"), cancel),
        ShellMessageBoxPurpose::UpdateCheck,
    );
}

/// The checking box's button, Esc or Enter: stop waiting. The check still
/// ends as the one at startup does, offering a newer release at the idle
/// main menu unless it was skipped and the lobby has not said this version
/// is behind.
pub(crate) fn cancel_check(frontend: &mut crate::app::frontend::state::FrontendState) {
    if let Phase::Checking { asked, .. } = &mut frontend.updater.phase {
        *asked = false;
    }
}

/// The online lobby says this version is behind the newest release: its 426
/// at sign-in or to a signed-in request, or its new-release line. The lobby
/// lists only rooms of the player's own version and Quick Match pairs only
/// equal ones, so the game checks as Check for Updates does: a newer release
/// is offered at the next idle main menu, even a skipped one. A
/// CnCNet-launched match has no main menu to ask at.
pub(crate) fn lobby_outdated(frontend: &mut crate::app::frontend::state::FrontendState) {
    if !frontend.external_launch {
        frontend.updater.lobby_outdated(|| start(false));
    }
}

/// The menu has slid out, or has not shown yet: ask over the empty shell
/// backdrop, as Exit Game does (`App::open_exit_confirm`).
pub(crate) fn open_offer(
    window: &winit::window::Window,
    renderer: &crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    process_assets: &crate::app::process_assets::ProcessAssets,
    options_profile: &crate::app::persistence::options_profile::RetailOptionsProfile,
) {
    use crate::i18n::{tr, tr_fill};
    let Phase::Offered { offer, .. } = &frontend.updater.phase else {
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
    let notes = notes::ReleaseNotes::new(&renderer.bit_font, changes);
    App::destroy_current_shell_dialog(frontend);
    App::open_shell_message_box(
        window,
        renderer,
        frontend,
        process_assets,
        options_profile,
        shell_box,
        ShellMessageBoxPurpose::UpdateOffer(notes),
    );
}

/// The offer's answer: 0 Update (or Download), 2 Skip This Version, 1 (or
/// Esc/Enter) Later.
pub(crate) fn answer_offer(
    platform: &crate::app::PlatformState,
    renderer: &crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    process_assets: &crate::app::process_assets::ProcessAssets,
    options_profile: &mut crate::app::persistence::options_profile::RetailOptionsProfile,
    answer: usize,
) {
    let Phase::Offered { package, offer } = std::mem::take(&mut frontend.updater.phase) else {
        return;
    };
    frontend.updater.phase = Phase::Finished;
    match (answer, offer) {
        (0, Offer::Update(release)) => download(
            &platform.window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            package,
            release,
        ),
        (0, Offer::Manual { .. }) => open_in_browser(DOWNLOAD_PAGE),
        (0, Offer::NewKey { .. }) => open_in_browser(GITHUB_RELEASES),
        (2, offer @ (Offer::Update(_) | Offer::Manual { .. })) => {
            let profile = options_profile;
            profile.skipped_update = offer.version().to_string();
            if let Some(config) = platform.game_config.as_ref()
                && let Err(error) = profile.commit_skipped_update(config.paths.settings_dir())
            {
                log::warn!("Updater: cannot save the skipped version: {error}");
            }
        }
        _ => {}
    }
}

fn download(
    window: &winit::window::Window,
    renderer: &crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    process_assets: &crate::app::process_assets::ProcessAssets,
    options_profile: &crate::app::persistence::options_profile::RetailOptionsProfile,
    package: Package,
    release: Release,
) {
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
        show_failure(
            window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            release.version,
            &anyhow::Error::from(error),
        );
        return;
    }
    open_progress(
        window,
        renderer,
        frontend,
        process_assets,
        options_profile,
        &release,
        0,
    );
    frontend.updater.phase = Phase::Downloading {
        release,
        progress,
        result,
    };
}

fn open_progress(
    window: &winit::window::Window,
    renderer: &crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    process_assets: &crate::app::process_assets::ProcessAssets,
    options_profile: &crate::app::persistence::options_profile::RetailOptionsProfile,
    release: &Release,
    received: u64,
) {
    let cancel = process_assets.csf_label("TXT_CANCEL", "Cancel");
    App::open_shell_message_box(
        window,
        renderer,
        frontend,
        process_assets,
        options_profile,
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
pub(crate) fn cancel_download(
    window: &winit::window::Window,
    renderer: &crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    process_assets: &crate::app::process_assets::ProcessAssets,
    options_profile: &crate::app::persistence::options_profile::RetailOptionsProfile,
) {
    let Phase::Downloading {
        release, progress, ..
    } = &frontend.updater.phase
    else {
        return;
    };
    let received = progress.received.load(Ordering::Relaxed);
    if received >= release.size {
        let release = release.clone();
        open_progress(
            window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            &release,
            received,
        );
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
fn restart(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &PersistenceState,
    release: &Release,
    finish: &mut Command,
) {
    match finish.spawn() {
        Ok(child) => {
            // The macOS and Linux helper starts the new version when its
            // standard input ends, which this process's exit closes.
            std::mem::forget(child);
            log::info!("Updater: restarting into VERA20k {}", release.version);
            App::confirm_quit(
                platform,
                renderer,
                diag,
                frontend,
                match_state,
                process_assets,
                audio,
                persistence,
            );
        }
        Err(error) => {
            log::warn!("Updater: cannot start {:?}: {error}", finish.get_program());
            show_failure(
                &platform.window,
                renderer,
                frontend,
                process_assets,
                &persistence.options_profile,
                release.version,
                &anyhow::Error::from(error),
            );
        }
    }
}

fn show_failure(
    window: &winit::window::Window,
    renderer: &crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    process_assets: &crate::app::process_assets::ProcessAssets,
    options_profile: &crate::app::persistence::options_profile::RetailOptionsProfile,
    version: Version,
    error: &anyhow::Error,
) {
    let reason = error.downcast_ref::<install::Hint>().map_or_else(
        || crate::i18n::tr("VERA:UpdateTryLater"),
        |hint| hint.0.clone(),
    );
    show_notice(
        window,
        renderer,
        frontend,
        process_assets,
        options_profile,
        crate::i18n::tr_fill(
            "VERA:UpdateFailed",
            &[("version", &version), ("reason", &reason)],
        ),
    );
}

/// An OK box in place of the updater's open one: a failure, or what Check
/// for Updates found.
fn show_notice(
    window: &winit::window::Window,
    renderer: &crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    process_assets: &crate::app::process_assets::ProcessAssets,
    options_profile: &crate::app::persistence::options_profile::RetailOptionsProfile,
    text: String,
) {
    App::open_shell_notice(
        window,
        renderer,
        frontend,
        process_assets,
        options_profile,
        text,
    );
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
                menu_hold: None,
            })
        });
        assert!(matches!(
            startup.phase,
            Phase::Checking { asked: false, .. }
        ));
        startup.found(package, offer, skipped);
        assert!(matches!(startup.phase, Phase::Found { .. }));
    }

    /// The startup check holds the main menu's first entry while it runs, up
    /// to MENU_HOLD and no longer; a later check, as the lobby's word starts,
    /// holds nothing.
    #[test]
    fn only_the_startup_check_holds_the_first_menu_and_only_briefly() {
        let checking = || {
            let (_sender, result) = mpsc::channel();
            Phase::Checking {
                package: Package::AppImage(Default::default()),
                result,
                asked: false,
                menu_hold: None,
            }
        };
        let start = Instant::now();
        let mut updater = Updater::default();
        updater.begin(checking(), start);
        assert!(updater.holds_main_menu());
        let before = start + MENU_HOLD - Duration::from_millis(1);
        assert_eq!(updater.wait_for_check(before), start + MENU_HOLD);
        assert!(updater.holds_main_menu());
        updater.wait_for_check(start + MENU_HOLD);
        assert!(!updater.holds_main_menu());

        updater.phase = Phase::Finished;
        updater.lobby_outdated(|| Ok(checking()));
        assert!(matches!(updater.phase, Phase::Checking { .. }));
        assert!(!updater.holds_main_menu());
    }
}
