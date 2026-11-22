//! VERA's Mods page (`ui::mods_shell`), opened by the Mods button of Options:
//! the list of what is in the `Mods` folder, and the buttons that switch to
//! one after a restart, open the folder, check a mod's files against its
//! file list, or make a new mod from the original game's files. gamemd has no
//! such page. The list, the choice and its place in config.toml are
//! `app::mods`'; the file list is `app::mod_files`'; a new mod's folder is
//! `app::new_mod`'s.

use crate::util::time::Instant;

use super::{App, AppState};
use crate::app::frontend::shell_layout::mods_layout;
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::app::frontend::state::{ModFilesBox, ShellMessageBoxPurpose};
use crate::app::mod_files::{Against, Checked, PageCheck, Step};
use crate::app::shell_route::ShellRoute;
use crate::i18n::{tr, tr_fill};
use crate::ui::mods_shell::{self, ModsLayout};
use crate::ui::movies_credits_shell::{
    family_list_node, family_list_selected, find_family_list, set_family_list,
};
use crate::ui::shell::modal::ShellBox;

impl App {
    fn installed_mods(state: &AppState) -> Vec<crate::app::mods::Listed> {
        let config = state.platform.game_config.as_ref();
        config
            .map(|config| crate::app::mods::installed(&config.paths))
            .unwrap_or_default()
    }

    /// The Mods button of Options, once Options has closed: list what `Mods`
    /// holds now, with what this run uses selected and in view.
    pub(super) fn open_mods_page(state: &mut AppState) {
        Self::destroy_current_shell_dialog(state);
        state.frontend.mods.page = Some(Self::installed_mods(state));
        Self::show_shell_page(state, ShellRoute::Mods, mods_shell::DIALOG);
        Self::mods_sync(state);
    }

    /// The row the page's list has selected.
    pub(super) fn mods_selected(state: &AppState) -> Option<usize> {
        family_list_selected(&state.frontend.shell_controller, mods_shell::DIALOG)
    }

    /// The window is in front again, perhaps from the folder that Open Mods
    /// Folder showed: list what `Mods` holds now. The picked row stays
    /// picked while its folder is there.
    pub(super) fn refresh_mods_page(state: &mut AppState) {
        if crate::app::input::precedence::route_page(state)
            != crate::app::input::precedence::ScreenPage::Mods
        {
            return;
        }
        let rows = Self::installed_mods(state);
        let selected = Self::mods_selected(state);
        let Some(page) = state.frontend.mods.page.as_mut() else {
            return;
        };
        if *page == rows {
            return;
        }
        let picked = selected
            .and_then(|row| page.get(row))
            .map(|row| row.folder.clone());
        let selected = picked.and_then(|folder| rows.iter().position(|row| row.folder == folder));
        *page = rows;
        // The top stays where it was, within the new range.
        Self::update_mods_list(state, selected, false);
    }

    /// The page's rows and `selected` into its `List` node. With `reveal`
    /// the list shows the selected row as a new list does (from the first
    /// row, scrolled into view); otherwise its top stays where it was,
    /// within the new range.
    fn update_mods_list(state: &mut AppState, selected: Option<usize>, reveal: bool) {
        let Some(rows) = state.frontend.mods.page.as_ref().map(Vec::len) else {
            return;
        };
        let controller = &mut state.frontend.shell_controller;
        let Some(node) = find_family_list(controller, mods_shell::DIALOG) else {
            return;
        };
        let gui = controller.gui_mut();
        let current = gui.list_view(node).map_or(0, |view| view.top);
        set_family_list(gui, node, rows, selected, if reveal { 0 } else { current });
        match (reveal, selected) {
            (true, Some(row)) => {
                let _ = gui.ensure_row_visible(node, row);
            }
            (true, None) => {}
            (false, _) => {
                let last = gui.list_geometry(node).map_or(0, |list| list.max_top);
                if current > last {
                    let _ = gui.set(
                        node,
                        vera20k_gui::Prop::Top,
                        vera20k_gui::Value::Int(last as i32),
                    );
                }
            }
        }
    }

    /// Binds the controller to the page and keeps its `List` node, which a
    /// new instance builds with the row in use selected and in view (a hover
    /// target for status help, never a button press); the layout.
    fn mods_sync(state: &mut AppState) -> ModsLayout {
        let layout = mods_layout(state);
        let mods = &state.frontend.mods;
        let in_use = mods.page.as_ref().map(|rows| {
            let selected = rows.iter().position(|row| mods.in_use(row));
            (rows.len(), selected)
        });
        let controller = &mut state.frontend.shell_controller;
        controller.ensure_active(mods_shell::DIALOG);
        controller.set_scale(layout.page.scale);
        if let Some((rows, selected)) = in_use {
            let built = find_family_list(controller, mods_shell::DIALOG).is_none();
            let list = (rows, selected, 0);
            let node = family_list_node(controller, mods_shell::DIALOG, layout.list, list);
            // A new instance scrolls the row in use into view.
            if built && let (Some(node), Some(row)) = (node, selected) {
                let _ = controller.gui_mut().ensure_row_visible(node, row);
            }
        }
        layout
    }

    /// The wheel moves the list three rows a notch, as on the Keyboard page,
    /// wherever the pointer is: the list takes the rows a turn makes.
    pub(super) fn handle_mods_wheel(state: &mut AppState, lines: f32) {
        Self::mods_sync(state);
        let controller = &mut state.frontend.shell_controller;
        let Some(node) = find_family_list(controller, mods_shell::DIALOG) else {
            return;
        };
        let now = controller.now();
        let gui = controller.gui_mut();
        let _ = gui.set(
            node,
            vera20k_gui::Prop::WheelStep,
            vera20k_gui::Value::Int(1),
        );
        gui.wheel_list(node, (lines * 3.0) as i32, now);
        gui.drain_events();
    }

    /// One pointer message (`down`: `None` moves), as in the movie list; the
    /// page checks for no double-click. Use This Mod is disabled while the
    /// selection is nothing a restart would change to, Check Files while it
    /// is no mod. Every hover message repaints the status line (0x00615EF7).
    pub(super) fn handle_mods_mouse(state: &mut AppState, down: Option<bool>) {
        let layout = Self::mods_sync(state);
        let selected = Self::mods_selected(state);
        let mods = &state.frontend.mods;
        let enabled = [
            (mods_shell::USE, mods.switch_to(selected).is_some()),
            (mods_shell::CHECK, mods.picked_is_mod(selected)),
        ];
        let rect_of = |id| layout.page.button_rect(id);
        let nodes = || mods_shell::PAGE.nodes();
        match Self::screen_page_pointer(state, mods_shell::DIALOG, nodes, rect_of, &enabled, down) {
            Some(mods_shell::USE) => Self::use_picked_mod(state),
            #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
            Some(mods_shell::ADD) => Self::open_add_mod(state),
            Some(mods_shell::OPEN_FOLDER) => Self::open_mods_folder(state),
            Some(mods_shell::CHECK) => Self::check_picked_mod(state),
            Some(mods_shell::NEW) => Self::make_new_mod(state),
            Some(mods_shell::BACK) => {
                Self::leave_shell_dialog(state, ShellSlideKind::Mods, |state| {
                    App::close_mods_page(state);
                    App::open_launcher_options_dialog(state);
                })
            }
            _ => {}
        }
    }

    /// Open Mods Folder. Where no file manager starts, say where the folder
    /// is.
    pub(super) fn open_mods_folder(state: &mut AppState) {
        let opened = state
            .platform
            .game_config
            .as_ref()
            .map(|config| crate::app::mods::open_folder(&config.paths));
        if let Some(Err(folder)) = opened {
            Self::show_folder_is_here(state, &folder);
        }
    }

    /// One of the page's boxes, in place of the one before it.
    fn open_mods_box(
        state: &mut AppState,
        text: String,
        confirm: String,
        cancel: String,
        purpose: ShellMessageBoxPurpose,
    ) {
        if state.frontend.shell_message_box.is_some() {
            Self::close_shell_message_box_from_controller(state);
        }
        let shell_box = ShellBox::message(text, confirm, cancel, String::new());
        Self::open_shell_message_box(state, shell_box, purpose);
    }

    /// What Check Files found, or what came of writing the list, with OK.
    fn open_mod_files_found(state: &mut AppState, text: String) {
        let ok = state.csf_label("TXT_OK", "OK");
        let purpose = ShellMessageBoxPurpose::ModFiles(ModFilesBox::Found);
        Self::open_mods_box(state, text, ok, String::new(), purpose);
    }

    /// Says where `folder` is, where no file manager starts.
    fn show_folder_is_here(state: &mut AppState, folder: &std::path::Path) {
        let text = tr_fill("VERA:ModsFolderIsHere", &[("folder", &folder.display())]);
        Self::open_shell_notice(state, text);
    }

    /// Check Files: read the picked mod's files on a thread of their own, and
    /// wait under a box whose button stops the wait ([`Self::poll_mod_files`]
    /// shows the answer).
    fn check_picked_mod(state: &mut AppState) {
        let selected = Self::mods_selected(state);
        let picked = state
            .platform
            .game_config
            .as_ref()
            .and_then(|config| state.frontend.mods.picked_mod(&config.paths, selected));
        let Some((title, dir)) = picked else {
            return;
        };
        // The folder went while the page was open.
        let started = dir
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
            .and_then(|dir| crate::app::mod_files::start(title.clone(), dir));
        match started {
            Ok(check) => {
                state.frontend.mods.files_check = Some(check);
                let text = tr_fill("VERA:ModChecking", &[("mod", &title)]);
                let purpose = ShellMessageBoxPurpose::ModFiles(ModFilesBox::Reading);
                Self::open_mods_box(state, text, tr("VERA:Cancel"), String::new(), purpose);
            }
            Err(error) => {
                log::warn!("Mods: {title}'s files cannot be read: {error}");
                Self::open_mod_files_found(
                    state,
                    tr_fill("VERA:ModFilesNotRead", &[("mod", &title)]),
                );
            }
        }
    }

    /// Check Files' reading, while it runs. Its answer takes the waiting
    /// box's place: every file as listed, or what differs with the offer to
    /// write the folder's files as the mod's list. Returns when to look
    /// again.
    pub(super) fn poll_mod_files(state: &mut AppState) -> Option<Instant> {
        use std::sync::mpsc::TryRecvError;
        /// How often the reading thread's answer is looked for.
        const LOOK_AGAIN: crate::util::time::Duration =
            crate::util::time::Duration::from_millis(100);
        let check = state.frontend.mods.files_check.take()?;
        let Step::Reading { result, .. } = &check.step else {
            state.frontend.mods.files_check = Some(check);
            return None;
        };
        let checked = match result.try_recv() {
            Err(TryRecvError::Empty) => {
                state.frontend.mods.files_check = Some(check);
                return Some(Instant::now() + LOOK_AGAIN);
            }
            Ok(Ok(checked)) => Some(checked),
            Ok(Err(error)) => {
                log::warn!("Mods: {} could not be read: {error}", check.dir.display());
                None
            }
            Err(TryRecvError::Disconnected) => None,
        };
        let PageCheck { title, dir, .. } = check;
        let Some(Checked { found, against }) = checked else {
            Self::open_mod_files_found(state, tr_fill("VERA:ModFilesNotRead", &[("mod", &title)]));
            return None;
        };
        let (count, code) = (found.len(), found.code());
        let found_text = |key| tr_fill(key, &[("mod", &title), ("count", &count), ("code", &code)]);
        let (text, offer) = match &against {
            Against::Same => (found_text("VERA:ModFilesSame"), None),
            Against::NoList => (found_text("VERA:ModFilesNoList"), Some("VERA:ModFilesMake")),
            Against::BadList => (
                found_text("VERA:ModFilesListBad"),
                Some("VERA:ModFilesMake"),
            ),
            Against::Differs(differences) => (
                tr_fill(
                    "VERA:ModFilesDiffer",
                    &[
                        ("mod", &title),
                        ("changed", &differences.changed.len()),
                        ("missing", &differences.missing.len()),
                        ("extra", &differences.extra.len()),
                        ("code", &code),
                    ],
                ),
                Some("VERA:ModFilesUpdate"),
            ),
        };
        match offer {
            None => Self::open_mod_files_found(state, text),
            Some(write_key) => {
                state.frontend.mods.files_check = Some(PageCheck {
                    title,
                    dir,
                    step: Step::Offered(found),
                });
                let purpose = ShellMessageBoxPurpose::ModFiles(ModFilesBox::Offer);
                Self::open_mods_box(state, text, tr(write_key), tr("VERA:Close"), purpose);
            }
        }
        None
    }

    /// The answer to one of Check Files' boxes. The waiting box's button
    /// stops the reading; the offer's first button writes the folder's files
    /// as the mod's list, and says what came of it.
    pub(super) fn answer_mod_files(state: &mut AppState, which: ModFilesBox, confirmed: bool) {
        let Some(check) = state.frontend.mods.files_check.take() else {
            return;
        };
        match (which, check.step) {
            (ModFilesBox::Reading, Step::Reading { stop, .. }) => {
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            (ModFilesBox::Offer, Step::Offered(found)) if confirmed => {
                let text = match found.write(&check.dir) {
                    Ok(list) => {
                        log::info!("Mods: wrote {}", list.display());
                        tr_fill(
                            "VERA:ModFilesMade",
                            &[
                                ("mod", &check.title),
                                ("count", &found.len()),
                                ("code", &found.code()),
                            ],
                        )
                    }
                    Err(error) => {
                        let list = check.dir.join(crate::app::mod_files::LIST_NAME);
                        log::warn!("Mods: could not write {}: {error}", list.display());
                        tr_fill("VERA:ModFilesNotMade", &[("file", &list.display())])
                    }
                };
                Self::open_mod_files_found(state, text);
            }
            _ => {}
        }
    }

    /// New Mod: a folder in `Mods` with the original game's main INI files
    /// (`app::new_mod`), picked in the list, under a box that says what it
    /// holds and offers to show it.
    fn make_new_mod(state: &mut AppState) {
        let Some(config) = state.platform.game_config.as_ref() else {
            return;
        };
        let paths = &config.paths;
        let mods_dir = paths.mods_dir();
        crate::app::mods::prepare_folder(&mods_dir);
        // The game folder with no mod on it, whatever this run plays: the
        // run's own archives may hold a mod's copies.
        let made = crate::assets::asset_manager::AssetManager::inspect_install(
            &paths.ra2_dir,
            state.process_assets.media_archive_mode(),
        )
        .map_err(std::io::Error::other)
        .and_then(|(original, _)| {
            crate::app::new_mod::make(&mods_dir, &tr("VERA:ModNewFolder"), |file| {
                original.get(file)
            })
        });
        let made = match made {
            Ok(made) => made,
            Err(error) => {
                log::warn!("Mods: no new mod made in {}: {error}", mods_dir.display());
                Self::open_shell_notice(
                    state,
                    tr_fill("VERA:ModNotMade", &[("folder", &mods_dir.display())]),
                );
                return;
            }
        };
        log::info!(
            "Mods: made {} with {}",
            made.dir.display(),
            made.copied.join(", ")
        );
        Self::select_installed_mod(state, &made.folder);
        let files = made.copied.join(", ");
        let text = tr_fill("VERA:ModMade", &[("mod", &made.folder), ("files", &files)]);
        let (open, close) = (tr("VERA:ModOpenItsFolder"), tr("VERA:Close"));
        Self::open_mods_box(
            state,
            text,
            open,
            close,
            ShellMessageBoxPurpose::ModMade(made.dir),
        );
    }

    /// Refresh the list and bring a mod we just added into view.
    pub(super) fn select_installed_mod(state: &mut AppState, folder: &str) {
        Self::refresh_mods_page(state);
        let selected = state.frontend.mods.page.as_ref().and_then(|rows| {
            rows.iter()
                .position(|row| row.folder.as_deref() == Some(folder))
        });
        Self::update_mods_list(state, selected, true);
    }

    /// New Mod's box: Open Folder shows the new mod's folder, or says where it
    /// is when no file manager starts.
    pub(super) fn answer_mod_made(state: &mut AppState, dir: &std::path::Path, open: bool) {
        if open && !crate::app::web_link::open_folder(dir) {
            Self::show_folder_is_here(state, dir);
        }
    }

    /// The page is gone and the route is the main menu's again. Back builds
    /// Options over it next; a restart leaves from here.
    pub(super) fn close_mods_page(state: &mut AppState) {
        Self::destroy_current_shell_dialog(state);
        state.frontend.mods.page = None;
        state.frontend.shell_route = ShellRoute::MainMenu;
    }

    /// Use This Mod. A mod is read once, at start-up, so a change to another
    /// row needs a restart: ask first. The row this run plays already (after
    /// a `--mod` switch, or when the kept choice could not be loaded) needs
    /// none: it only becomes the choice for the next start.
    fn use_picked_mod(state: &mut AppState) {
        let selected = Self::mods_selected(state);
        let mods = &state.frontend.mods;
        let Some(row) = mods.switch_to(selected) else {
            return;
        };
        let folder = row.folder.clone();
        if mods.in_use(row) {
            Self::keep_mod_choice(state, folder.as_deref());
            return;
        }
        let title = match &row.folder {
            Some(_) => tr_fill("VERA:ModRestartUse", &[("mod", &row.title())]),
            None => tr("VERA:ModRestartOriginal"),
        };
        let (restart, cancel) = (tr("VERA:ModRestart"), tr("VERA:Cancel"));
        let purpose = ShellMessageBoxPurpose::ModRestart(folder);
        Self::open_mods_box(state, title, restart, cancel, purpose);
    }

    /// Keep `folder`, or the original game, as the choice for the next start.
    /// False when config.toml could not be written: nothing changes, and a
    /// notice names the file.
    fn keep_mod_choice(state: &mut AppState, folder: Option<&str>) -> bool {
        let kept = match state.platform.game_config.as_ref() {
            Some(config) => state.frontend.mods.keep_choice(&config.paths, folder),
            None => Err(std::io::Error::other("no config was read")),
        };
        let Err(error) = kept else {
            return true;
        };
        log::warn!("Mods: the choice was not saved: {error}");
        let file = crate::util::config::GameConfig::path()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        Self::open_shell_notice(state, tr_fill("VERA:ModChoiceNotSaved", &[("file", &file)]));
        false
    }

    /// The restart question's answer. Restart keeps the choice for the next
    /// start, then leaves the way Exit Game does; the game starts again once
    /// this copy is gone (`app::restart`).
    pub(super) fn answer_mod_restart(state: &mut AppState, folder: Option<String>, restart: bool) {
        if !restart {
            return;
        }
        if Self::keep_mod_choice(state, folder.as_deref()) {
            log::info!("Mods: restarting to use {folder:?}");
            crate::app::restart::ask();
            // As Exit Game leaves: from the empty backdrop, the page gone.
            Self::close_mods_page(state);
            Self::confirm_quit(state);
        }
    }
}
