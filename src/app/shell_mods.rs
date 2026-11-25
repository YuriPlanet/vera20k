//! VERA's Mods page (`ui::mods_shell`), opened by the Mods button of Options:
//! the list of what is in the `Mods` folder, and the buttons that switch to
//! one after a restart, open the folder, check a mod's files against its
//! file list, or make a new mod from the original game's files. gamemd has no
//! such page. The list, the choice and its place in config.toml are
//! `app::mods`'; the file list is `app::mod_files`'; a new mod's folder is
//! `app::new_mod`'s.

use crate::util::time::Instant;

use super::{App, AppState};
use crate::app::PlatformState;
use crate::app::frontend::shell_layout::mods_layout;
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::app::frontend::state::FrontendState;
use crate::app::frontend::state::{ModFilesBox, ShellMessageBoxPurpose};
use crate::app::mod_files::{Against, Checked, PageCheck, Step};
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::shell_route::ShellRoute;
use crate::i18n::{tr, tr_fill};
use crate::ui::mods_shell::{self, ModsLayout};
use crate::ui::movies_credits_shell::{
    family_list_node, family_list_selected, find_family_list, set_family_list,
};
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::modal::ShellBox;
use crate::util::config::GameConfig;
use winit::window::Window;

impl App {
    fn installed_mods(game_config: Option<&GameConfig>) -> Vec<crate::app::mods::Listed> {
        let config = game_config;
        config
            .map(|config| crate::app::mods::installed(&config.paths))
            .unwrap_or_default()
    }

    /// The Mods button of Options, once Options has closed: list what `Mods`
    /// holds now, with what this run uses selected and in view.
    pub(super) fn open_mods_page(state: &mut AppState) {
        Self::destroy_current_shell_dialog(state);
        state.frontend.mods.page = Some(Self::installed_mods(state.platform.game_config.as_ref()));
        Self::show_shell_page(&mut state.frontend, ShellRoute::Mods, mods_shell::DIALOG);
        Self::mods_sync(state);
    }

    /// The row the page's list has selected.
    pub(super) fn mods_selected(controller: &DialogController) -> Option<usize> {
        family_list_selected(controller, mods_shell::DIALOG)
    }

    /// The window is in front again, perhaps from the folder that Open Mods
    /// Folder showed: list what `Mods` holds now. The picked row stays
    /// picked while its folder is there.
    pub(super) fn refresh_mods_page(state: &mut AppState) {
        if crate::app::input::precedence::route_page(&state.frontend)
            != crate::app::input::precedence::ScreenPage::Mods
        {
            return;
        }
        let rows = Self::installed_mods(state.platform.game_config.as_ref());
        let selected = Self::mods_selected(&state.frontend.shell_controller);
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
        Self::update_mods_list(&mut state.frontend, selected, false);
    }

    /// The page's rows and `selected` into its `List` node. With `reveal`
    /// the list shows the selected row as a new list does (from the first
    /// row, scrolled into view); otherwise its top stays where it was,
    /// within the new range.
    fn update_mods_list(frontend: &mut FrontendState, selected: Option<usize>, reveal: bool) {
        let Some(rows) = frontend.mods.page.as_ref().map(Vec::len) else {
            return;
        };
        let controller = &mut frontend.shell_controller;
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
        let layout = mods_layout(&state.renderer.gpu);
        let mods = &state.frontend.mods;
        let in_use = mods.page.as_ref().map(|rows| {
            let selected = rows.iter().position(|row| mods.in_use(row));
            (rows.len(), selected)
        });
        let controller = &mut state.frontend.shell_controller;
        controller.ensure_active(mods_shell::DIALOG);
        controller.set_scale(layout.page.scale);
        if let Some((rows, selected)) = in_use {
            // The buttons first: the list's id counts as built.
            if let Some(root) = controller.root_in(mods_shell::DIALOG) {
                crate::ui::shell::page::ensure_built(controller.gui_mut(), root, mods_shell::nodes);
            }
            let built = find_family_list(controller, mods_shell::DIALOG).is_none();
            let list = (rows, selected, 0);
            let node = family_list_node(controller, mods_shell::DIALOG, layout.list, list);
            if let Some(node) = node {
                let id = vera20k_gui::Value::Int(mods_shell::LIST_ID);
                let gui = controller.gui_mut();
                crate::ui::shell::page::set_if_changed(gui, node, vera20k_gui::Prop::Id, id);
            }
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
        let selected = Self::mods_selected(&state.frontend.shell_controller);
        let mods = &state.frontend.mods;
        let enabled = [
            (mods_shell::USE, mods.switch_to(selected).is_some()),
            (mods_shell::CHECK, mods.picked_is_mod(selected)),
            #[cfg(target_os = "android")]
            (mods_shell::RENAME, Self::picked_folder(state).is_some()),
            #[cfg(target_os = "android")]
            (mods_shell::DELETE, Self::picked_folder(state).is_some()),
            #[cfg(target_os = "android")]
            (
                mods_shell::OPEN_FOLDER,
                Self::picked_folder(state).is_some(),
            ),
            #[cfg(target_os = "android")]
            (mods_shell::PIN, Self::picked_folder(state).is_some()),
            #[cfg(target_os = "android")]
            (mods_shell::NAME_OK, Self::name_dialog_open()),
            #[cfg(target_os = "android")]
            (mods_shell::NAME_CLOSE, Self::name_dialog_open()),
        ];
        let rect_of = |id| layout.rect_of(id);
        let nodes = mods_shell::nodes;
        match Self::screen_page_pointer(
            &mut state.frontend,
            &state.match_state,
            &state.process_assets,
            &mut state.audio.sfx_player,
            mods_shell::DIALOG,
            nodes,
            rect_of,
            &enabled,
            down,
        ) {
            Some(mods_shell::USE) => Self::use_picked_mod(
                &state.platform,
                &state.renderer,
                &mut state.frontend,
                &state.process_assets,
                &state.persistence.options_profile,
            ),
            #[cfg(any(
                windows,
                target_os = "macos",
                target_os = "linux",
                target_os = "android"
            ))]
            Some(mods_shell::ADD) => Self::open_add_mod(state),
            #[cfg(target_os = "android")]
            Some(mods_shell::NAME_OK) => Self::answer_name_dialog(state, true),
            #[cfg(target_os = "android")]
            Some(mods_shell::NAME_CLOSE) => Self::answer_name_dialog(state, false),
            #[cfg(target_os = "android")]
            Some(mods_shell::RENAME) => Self::open_rename_dialog(state),
            #[cfg(target_os = "android")]
            Some(mods_shell::DELETE) => Self::delete_picked_mod(state),
            #[cfg(target_os = "android")]
            Some(mods_shell::PIN) => Self::pin_picked_mod(state),
            #[cfg(target_os = "android")]
            Some(mods_shell::OPEN_FOLDER) => Self::export_picked_mod(state),
            #[cfg(not(target_os = "android"))]
            Some(mods_shell::OPEN_FOLDER) => Self::open_mods_folder(
                &state.platform,
                &state.renderer,
                &mut state.frontend,
                &state.process_assets,
                &state.persistence.options_profile,
            ),
            Some(mods_shell::CHECK) => Self::check_picked_mod(
                &state.platform,
                &state.renderer,
                &mut state.frontend,
                &state.process_assets,
                &state.persistence.options_profile,
            ),
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

    /// The folder of the row the list has selected, if it is a mod.
    #[cfg(target_os = "android")]
    fn picked_folder(state: &AppState) -> Option<String> {
        let selected = Self::mods_selected(&state.frontend.shell_controller)?;
        state
            .frontend
            .mods
            .page
            .as_ref()?
            .get(selected)?
            .folder
            .clone()
    }

    /// Android: Delete Mod. The app owns the folder, so it removes it here and
    /// says what came of it; the page stays on the list it now shows.
    #[cfg(target_os = "android")]
    pub(super) fn delete_picked_mod(state: &mut AppState) {
        let Some(folder) = Self::picked_folder(state) else {
            return;
        };
        let dir = state
            .platform
            .game_config
            .as_ref()
            .map(|config| config.paths.mods_dir().join(&folder));
        let text = match dir.map(|dir| (std::fs::remove_dir_all(&dir), dir)) {
            Some((Ok(()), dir)) => {
                log::info!("Mods: deleted {}", dir.display());
                let id = crate::app::mods::mod_id(&dir).unwrap_or_default();
                if !crate::util::mod_actions::forget(&folder, &id) {
                    log::warn!("Mods: no shortcut hook to forget {folder}");
                }
                tr_fill("VERA:ModDeleted", &[("mod", &folder)])
            }
            Some((Err(error), dir)) => {
                log::warn!("Mods: could not delete {}: {error}", dir.display());
                tr("VERA:ModActionFailed").to_string()
            }
            None => tr("VERA:ModActionFailed").to_string(),
        };
        Self::refresh_mods_page(state);
        Self::open_shell_notice(
            &state.platform.window,
            &state.renderer,
            &mut state.frontend,
            &state.process_assets,
            &state.persistence.options_profile,
            text,
        );
    }

    /// Android: Export Mod, through the system file manager.
    #[cfg(target_os = "android")]
    pub(super) fn export_picked_mod(state: &mut AppState) {
        let Some(folder) = Self::picked_folder(state) else {
            return;
        };
        if !crate::util::mod_actions::export(&folder) {
            log::warn!("Mods: no export hook for {folder}");
            Self::mod_action_failed(state);
        }
    }

    /// Android: Add Desktop Entry, a home-screen shortcut for this folder.
    #[cfg(target_os = "android")]
    pub(super) fn pin_picked_mod(state: &mut AppState) {
        let Some(folder) = Self::picked_folder(state) else {
            return;
        };
        let dir = state
            .platform
            .game_config
            .as_ref()
            .map(|config| config.paths.mods_dir().join(&folder));
        let id = dir
            .as_deref()
            .and_then(crate::app::mods::ensure_mod_id)
            .unwrap_or_default();
        if !crate::util::mod_actions::pin(&folder, &id) {
            log::warn!("Mods: no shortcut hook for {folder}");
            Self::mod_action_failed(state);
        }
    }

    /// Android: Import Mod, a folder the system file manager copies in.
    #[cfg(target_os = "android")]
    pub(super) fn import_mod(state: &mut AppState) {
        if !crate::util::mod_actions::import() {
            log::warn!("Mods: no import hook");
            Self::mod_action_failed(state);
        }
    }

    /// The one line a failed platform action shows.
    #[cfg(target_os = "android")]
    fn mod_action_failed(state: &mut AppState) {
        Self::open_shell_notice(
            &state.platform.window,
            &state.renderer,
            &mut state.frontend,
            &state.process_assets,
            &state.persistence.options_profile,
            tr("VERA:ModActionFailed").to_string(),
        );
    }

    /// Open Mods Folder. Where no file manager starts, say where the folder
    /// is.
    pub(super) fn open_mods_folder(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
    ) {
        let opened = platform
            .game_config
            .as_ref()
            .map(|config| crate::app::mods::open_folder(&config.paths));
        if let Some(Err(folder)) = opened {
            Self::show_folder_is_here(
                &platform.window,
                renderer,
                frontend,
                process_assets,
                options_profile,
                &folder,
            );
        }
    }

    /// One of the page's boxes, in place of the one before it.
    fn open_mods_box(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        text: String,
        confirm: String,
        cancel: String,
        purpose: ShellMessageBoxPurpose,
    ) {
        if frontend.shell_message_box.is_some() {
            Self::close_shell_message_box_from_controller(frontend);
        }
        let shell_box = ShellBox::message(text, confirm, cancel, String::new());
        Self::open_shell_message_box(
            window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            shell_box,
            purpose,
        );
    }

    /// What Check Files found, or what came of writing the list, with OK.
    fn open_mod_files_found(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        text: String,
    ) {
        let ok = process_assets.csf_label("TXT_OK", "OK");
        let purpose = ShellMessageBoxPurpose::ModFiles(ModFilesBox::Found);
        Self::open_mods_box(
            window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            text,
            ok,
            String::new(),
            purpose,
        );
    }

    /// Says where `folder` is, where no file manager starts.
    fn show_folder_is_here(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        folder: &std::path::Path,
    ) {
        let text = tr_fill("VERA:ModsFolderIsHere", &[("folder", &folder.display())]);
        Self::open_shell_notice(
            window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            text,
        );
    }

    /// Check Files: read the picked mod's files on a thread of their own, and
    /// wait under a box whose button stops the wait ([`Self::poll_mod_files`]
    /// shows the answer).
    fn check_picked_mod(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
    ) {
        let selected = Self::mods_selected(&frontend.shell_controller);
        let picked = platform
            .game_config
            .as_ref()
            .and_then(|config| frontend.mods.picked_mod(&config.paths, selected));
        let Some((title, dir)) = picked else {
            return;
        };
        // The folder went while the page was open.
        let started = dir
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
            .and_then(|dir| crate::app::mod_files::start(title.clone(), dir));
        match started {
            Ok(check) => {
                frontend.mods.files_check = Some(check);
                let text = tr_fill("VERA:ModChecking", &[("mod", &title)]);
                let purpose = ShellMessageBoxPurpose::ModFiles(ModFilesBox::Reading);
                Self::open_mods_box(
                    &platform.window,
                    renderer,
                    frontend,
                    process_assets,
                    options_profile,
                    text,
                    tr("VERA:Cancel"),
                    String::new(),
                    purpose,
                );
            }
            Err(error) => {
                log::warn!("Mods: {title}'s files cannot be read: {error}");
                Self::open_mod_files_found(
                    &platform.window,
                    renderer,
                    frontend,
                    process_assets,
                    options_profile,
                    tr_fill("VERA:ModFilesNotRead", &[("mod", &title)]),
                );
            }
        }
    }

    /// Check Files' reading, while it runs. Its answer takes the waiting
    /// box's place: every file as listed, or what differs with the offer to
    /// write the folder's files as the mod's list. Returns when to look
    /// again.
    pub(super) fn poll_mod_files(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
    ) -> Option<Instant> {
        use std::sync::mpsc::TryRecvError;
        /// How often the reading thread's answer is looked for.
        const LOOK_AGAIN: crate::util::time::Duration =
            crate::util::time::Duration::from_millis(100);
        let check = frontend.mods.files_check.take()?;
        let Step::Reading { result, .. } = &check.step else {
            frontend.mods.files_check = Some(check);
            return None;
        };
        let checked = match result.try_recv() {
            Err(TryRecvError::Empty) => {
                frontend.mods.files_check = Some(check);
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
            Self::open_mod_files_found(
                window,
                renderer,
                frontend,
                process_assets,
                options_profile,
                tr_fill("VERA:ModFilesNotRead", &[("mod", &title)]),
            );
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
            None => Self::open_mod_files_found(
                window,
                renderer,
                frontend,
                process_assets,
                options_profile,
                text,
            ),
            Some(write_key) => {
                frontend.mods.files_check = Some(PageCheck {
                    title,
                    dir,
                    step: Step::Offered(found),
                });
                let purpose = ShellMessageBoxPurpose::ModFiles(ModFilesBox::Offer);
                Self::open_mods_box(
                    window,
                    renderer,
                    frontend,
                    process_assets,
                    options_profile,
                    text,
                    tr(write_key),
                    tr("VERA:Close"),
                    purpose,
                );
            }
        }
        None
    }

    /// The answer to one of Check Files' boxes. The waiting box's button
    /// stops the reading; the offer's first button writes the folder's files
    /// as the mod's list, and says what came of it.
    pub(super) fn answer_mod_files(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        which: ModFilesBox,
        confirmed: bool,
    ) {
        let Some(check) = frontend.mods.files_check.take() else {
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
                Self::open_mod_files_found(
                    window,
                    renderer,
                    frontend,
                    process_assets,
                    options_profile,
                    text,
                );
            }
            _ => {}
        }
    }

    /// New Mod: a folder in `Mods` with the original game's main INI files
    /// (`app::new_mod`), picked in the list, under a box that says what it
    /// holds and offers to show it.
    fn make_new_mod(state: &mut AppState) {
        #[cfg(target_os = "android")]
        Self::open_name_dialog(state);
        #[cfg(not(target_os = "android"))]
        Self::make_mod_named(state, &tr("VERA:ModNewFolder"), true);
    }

    /// Makes a mod named `name` (the default name when it is blank), selects it
    /// and, when `report`, says what it made.
    fn make_mod_named(state: &mut AppState, name: &str, report: bool) {
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
            crate::app::new_mod::make(&mods_dir, name, |file| original.get(file))
        });
        let made = match made {
            Ok(made) => made,
            Err(error) => {
                log::warn!("Mods: no new mod made in {}: {error}", mods_dir.display());
                Self::open_shell_notice(
                    &state.platform.window,
                    &state.renderer,
                    &mut state.frontend,
                    &state.process_assets,
                    &state.persistence.options_profile,
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
        if let Some(id) = crate::app::mods::ensure_mod_id(&made.dir) {
            log::info!("Mods: {} has id {id}", made.folder);
        }
        Self::select_installed_mod(state, &made.folder);
        if !report {
            return;
        }
        let files = made.copied.join(", ");
        let text = tr_fill("VERA:ModMade", &[("mod", &made.folder), ("files", &files)]);
        #[cfg(target_os = "android")]
        Self::open_shell_notice(
            &state.platform.window,
            &state.renderer,
            &mut state.frontend,
            &state.process_assets,
            &state.persistence.options_profile,
            text,
        );
        #[cfg(not(target_os = "android"))]
        Self::open_mods_box(
            &state.platform.window,
            &state.renderer,
            &mut state.frontend,
            &state.process_assets,
            &state.persistence.options_profile,
            text,
            tr("VERA:ModOpenItsFolder"),
            tr("VERA:Close"),
            ShellMessageBoxPurpose::ModMade(made.dir),
        );
    }

    /// Android: New Mod's dialog keys: Enter makes the mod, Escape closes it,
    /// anything else types into its edit.
    #[cfg(target_os = "android")]
    pub(crate) fn name_dialog_key(state: &mut AppState, event: &winit::event::KeyEvent) {
        use winit::keyboard::PhysicalKey;
        if !event.state.is_pressed() {
            return;
        }
        let code = match event.physical_key {
            PhysicalKey::Code(code) => code,
            PhysicalKey::Unidentified(_) => return,
        };
        match code {
            winit::keyboard::KeyCode::Enter | winit::keyboard::KeyCode::NumpadEnter => {
                Self::answer_name_dialog(state, true);
                return;
            }
            winit::keyboard::KeyCode::Escape => {
                Self::answer_name_dialog(state, false);
                return;
            }
            _ => {}
        }
        let shift = event.text.as_deref().is_none_or(|_| false)
            || state.platform.live_modifiers.shift_key();
        let inputs =
            crate::ui::shell::controller::gui_inputs(Some(code), shift, event.text.as_deref());
        for input in inputs {
            state.frontend.shell_controller.on_input(input);
        }
        state.platform.window.request_redraw();
    }

    /// Android: New Mod's hidden edit node, whose live state holds the typed
    /// name and its caret.
    #[cfg(target_os = "android")]
    fn name_edit_node(state: &AppState) -> Option<vera20k_gui::NodeId> {
        let controller = &state.frontend.shell_controller;
        let root = controller.root_in(mods_shell::DIALOG)?;
        let gui = controller.gui();
        gui.tree()
            .children(root)
            .ok()?
            .iter()
            .copied()
            .find(|node| gui.id_of(*node) == Some(mods_shell::NAME_EDIT.into()))
    }

    /// Android: paints New Mod's dialog over the finished page, in the same
    /// pass as the message boxes, so its panel, buttons and text sit above the
    /// page's own text. Its edit is a hidden GUI control that only takes keys.
    #[cfg(target_os = "android")]
    pub(crate) fn draw_name_dialog_over(
        state: &AppState,
        encoder: &mut wgpu::CommandEncoder,
        destination: &wgpu::Texture,
    ) {
        use crate::app::frontend::shell_pass::{ShellComposition, TexturedDraw};
        use crate::render::shell_paint::{self, ModalButton, ModalDepths};
        use crate::render::shell_text::TextRect;
        use crate::ui::shell::geom::ShellAlign;
        if !Self::name_dialog_open() {
            return;
        }
        let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
            return;
        };
        let layout = mods_layout(&state.renderer.gpu);
        let scale = layout.page.scale;
        let (panel, prompt, edit, ok, close) = mods_shell::name_dialog_rects(scale);
        let mut sprites = Vec::new();
        if let Some(art) = atlas.ra2_dialog_background {
            shell_paint::push_entry_sized(
                &mut sprites,
                art,
                panel.x as f32,
                panel.y as f32,
                [panel.w as f32, panel.h as f32],
                0.00070,
            );
        } else if let Some(white) = atlas.white_pixel {
            shell_paint::push_tinted_entry(&mut sprites, white, panel, [0.03, 0.03, 0.04], 0.00070);
        }
        let frames = crate::app::frontend::skirmish_shell_render::type3_button_frames(atlas);
        sprites.extend(shell_paint::paint_modal_sprites(
            None,
            frames,
            panel,
            &[
                ModalButton {
                    rect: ok,
                    pressed: false,
                    enabled: true,
                },
                ModalButton {
                    rect: close,
                    pressed: false,
                    enabled: true,
                },
            ],
            scale,
            ModalDepths {
                background: 0.00072,
                button: 0.00068,
                text: 0.00060,
            },
        ));
        let font = &state.renderer.bit_font;
        let line = |text: &str, rect, rgb, align| {
            crate::render::shell_text::draw_in_rect_scaled(
                font,
                text,
                TextRect::of(rect),
                rgb,
                align,
                [0.0, 0.0],
                0.00060,
                scale.factor(),
            )
        };
        let mut text = Vec::new();
        // Where the box's own text starts (`body`), as the Mods page's boxes
        // do: the hint on the first line, the field under it, its height one
        // line plus twice the text's top inset, and the name centred in it.
        let body = mods_shell::name_body_rect(scale);
        let line_h = (font.cell_height() * scale.factor()).round() as i32;
        let inset = (2.0 * scale.factor()).round() as i32;
        let prompt_rect = crate::ui::shell::geom::RectPx::new(body.x, body.y, body.w, line_h);
        let field = crate::ui::shell::geom::RectPx::new(
            body.x,
            body.y + line_h + (line_h / 2).max(4),
            body.w,
            line_h + 2 * inset,
        );
        let _ = edit;
        let node = Self::name_edit_node(state);
        let edit_state = node
            .and_then(|node| state.frontend.shell_controller.gui().edit_state(node))
            .unwrap_or_default();
        let caret = node.is_some_and(|node| state.frontend.shell_controller.caret_visible(node));
        crate::app::frontend::skirmish_shell_render::push_new_edit(
            &mut sprites,
            &mut text,
            atlas,
            font,
            &edit_state,
            None,
            field,
            caret,
            crate::render::shell_paint::SHELL_TEXT_RGB_ENABLED,
            0.00060,
            scale,
        );
        let hint = if Self::name_dialog_renames() {
            crate::i18n::tr("VERA:ModRenameHint")
        } else {
            crate::i18n::tr("VERA:ModNewNameHint")
        };
        text.push(line(
            &hint,
            prompt_rect,
            crate::render::shell_paint::SHELL_TEXT_RGB_ENABLED,
            ShellAlign::NONE,
        ));
        let enabled = crate::render::shell_paint::SHELL_TEXT_RGB_ENABLED;
        text.push(line(
            &tr("TXT_OK"),
            ok,
            enabled,
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
        ));
        text.push(line(
            &tr("VERA:Close"),
            close,
            enabled,
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
        ));
        let draws = [TexturedDraw {
            texture: &atlas.texture,
            instances: sprites,
        }];
        crate::app::frontend::shell_pass::encode_shell_pass_over_page(
            &state.renderer,
            encoder,
            destination,
            "Mods Name Dialog",
            ShellComposition {
                draws: &draws,
                text: &text,
                cursor: None,
                effects: None,
                smooth_art: false,
            },
        );
    }

    /// Android: whether New Mod's name dialog is up.
    #[cfg(target_os = "android")]
    pub(crate) fn name_dialog_open() -> bool {
        name_dialog::OPEN.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Android: shows New Mod's name dialog over the page.
    #[cfg(target_os = "android")]
    pub(crate) fn open_name_dialog(state: &mut AppState) {
        name_dialog::MODE.store(1, std::sync::atomic::Ordering::Relaxed);
        name_dialog::OPEN.store(true, std::sync::atomic::Ordering::Relaxed);
        state.platform.window.request_redraw();
    }

    /// Android: whether the dialog renames the picked mod rather than making
    /// one.
    #[cfg(target_os = "android")]
    pub(crate) fn name_dialog_renames() -> bool {
        name_dialog::MODE.load(std::sync::atomic::Ordering::Relaxed) == 2
    }

    /// Android: renames the picked mod through the same dialog New Mod uses.
    #[cfg(target_os = "android")]
    pub(crate) fn open_rename_dialog(state: &mut AppState) {
        let Some(folder) = Self::picked_folder(state) else {
            return;
        };
        if let Ok(mut from) = name_dialog::RENAME_FROM.lock() {
            *from = folder;
        }
        name_dialog::MODE.store(2, std::sync::atomic::Ordering::Relaxed);
        name_dialog::OPEN.store(true, std::sync::atomic::Ordering::Relaxed);
        state.platform.window.request_redraw();
    }

    /// Android: places the name dialog, hides it when closed and gives its edit
    /// the keyboard when it opens. The page's own dialog keeps these widgets,
    /// so typing survives a touch anywhere.
    #[cfg(target_os = "android")]
    pub(crate) fn sync_name_dialog(state: &mut AppState) {
        use vera20k_gui::{Prop, Value};
        let open = Self::name_dialog_open();
        let layout = mods_layout(&state.renderer.gpu);
        let scale = layout.page.scale;
        let dialog = mods_shell::DIALOG;
        let controller = &mut state.frontend.shell_controller;
        controller.set_scale_in(dialog, scale);
        let Some(root) = controller.root_in(dialog) else {
            return;
        };
        let nodes = name_dialog::find(controller.gui(), root);
        let Some(nodes) = nodes else {
            return;
        };
        let (_, prompt, edit, ok, close) = mods_shell::name_dialog_rects(scale);
        let _ = prompt;
        let gui = controller.gui_mut();
        // The edit takes keys only: the overlay paints the field, so its own
        // widget is kept off-screen and never flashes before the overlay.
        let off_screen =
            crate::ui::shell::geom::RectPx::new(-4096, -4096, edit.w.max(1), edit.h.max(1));
        for (node, rect) in [
            (nodes.prompt, off_screen),
            (nodes.edit, off_screen),
            (nodes.ok, ok),
            (nodes.close, close),
        ] {
            let _ = gui.set(node, Prop::Visible, Value::Bool(open));
            let _ = gui.set(node, Prop::Rect, Value::Rect(rect));
        }
        let _ = gui.set(nodes.ok, Prop::Enabled, Value::Bool(open));
        let _ = gui.set(nodes.close, Prop::Enabled, Value::Bool(open));
        if open && !name_dialog::CLEARED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            let text = if Self::name_dialog_renames() {
                name_dialog::RENAME_FROM
                    .lock()
                    .map(|from| from.clone())
                    .unwrap_or_default()
            } else {
                String::new()
            };
            let _ = gui.set(nodes.edit, Prop::Text, Value::Text(text));
            let _ = gui.set_focus(Some(nodes.edit));
        }
        if !open {
            name_dialog::CLEARED.store(false, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Android: renames the picked mod's folder, keeps its `mod.ini` name in
    /// step and carries the choice and the launcher shortcut with it.
    #[cfg(target_os = "android")]
    pub(crate) fn rename_picked_mod(state: &mut AppState, from: &str, typed: &str) {
        let Some(config) = state.platform.game_config.as_ref() else {
            return;
        };
        let paths = &config.paths;
        let mods_dir = paths.mods_dir();
        let name = crate::app::new_mod::plain_name(typed).trim().to_owned();
        if name.is_empty() || name == from {
            return;
        }
        let from_dir = mods_dir.join(from);
        if !from_dir.is_dir() {
            return;
        }
        let taken = crate::util::case_insensitive_path::find(&mods_dir, &name)
            .is_some_and(|found| found != from_dir);
        if taken {
            Self::open_shell_notice(
                &state.platform.window,
                &state.renderer,
                &mut state.frontend,
                &state.process_assets,
                &state.persistence.options_profile,
                tr("VERA:ModRenameFailed"),
            );
            return;
        }
        let to = mods_dir.join(&name);
        if let Err(error) = std::fs::rename(&from_dir, &to) {
            log::warn!("Mods: could not rename {from} to {name}: {error}");
            Self::open_shell_notice(
                &state.platform.window,
                &state.renderer,
                &mut state.frontend,
                &state.process_assets,
                &state.persistence.options_profile,
                tr("VERA:ModRenameFailed"),
            );
            return;
        }
        Self::set_mod_name(&to, &name);
        if paths.mod_name() == Some(from) {
            let _ = state.frontend.mods.keep_choice(paths, Some(&name));
        }
        log::info!("Mods: renamed {from} to {name}");
        Self::select_installed_mod(state, &name);
    }

    /// Android: `mod.ini`'s `Name=` follows a rename: the one it shows, or a
    /// new one in its `[Mod]` section (the starter file's shape). A commented
    /// `;Name=` stays a comment; the folder name is used then.
    #[cfg(target_os = "android")]
    fn set_mod_name(dir: &std::path::Path, name: &str) {
        let file = dir.join("mod.ini");
        let Ok(text) = std::fs::read_to_string(&file) else {
            return;
        };
        let shown = |line: &str| {
            let line = line.trim_start();
            !line.starts_with(';') && line.to_ascii_lowercase().starts_with("name=")
        };
        if !text.lines().any(shown) {
            return;
        }
        let mut out = String::with_capacity(text.len() + name.len());
        for line in text.lines() {
            if shown(line) {
                out.push_str(&format!("Name={name}\n"));
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }
        if let Err(error) = std::fs::write(&file, out) {
            log::warn!(
                "Mods: could not set the name in {}: {error}",
                file.display()
            );
        }
    }

    /// Android: OK makes the mod named in the row (the default when blank),
    /// Close leaves it alone.
    #[cfg(target_os = "android")]
    pub(crate) fn answer_name_dialog(state: &mut AppState, ok: bool) {
        use vera20k_gui::Prop;
        name_dialog::OPEN.store(false, std::sync::atomic::Ordering::Relaxed);
        let dialog = mods_shell::DIALOG;
        let controller = &mut state.frontend.shell_controller;
        let text = controller
            .root_in(dialog)
            .and_then(|root| name_dialog::find(controller.gui(), root))
            .and_then(|nodes| match controller.gui().get(nodes.edit, Prop::Text) {
                Ok(Some(vera20k_gui::Value::Text(text))) => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let renames = Self::name_dialog_renames();
        let from = name_dialog::RENAME_FROM
            .lock()
            .map(|from| from.clone())
            .unwrap_or_default();
        name_dialog::MODE.store(0, std::sync::atomic::Ordering::Relaxed);
        state.platform.window.request_redraw();
        if !ok {
            return;
        }
        if renames {
            Self::rename_picked_mod(state, &from, &text);
            return;
        }
        let name = text.trim().to_owned();
        let name = if name.is_empty() {
            tr("VERA:ModNewFolder")
        } else {
            name
        };
        // The mod's own notice box reports it, as it always has.
        Self::make_mod_named(state, &name, true);
    }

    /// Refresh the list and bring a mod we just added into view.
    pub(super) fn select_installed_mod(state: &mut AppState, folder: &str) {
        Self::refresh_mods_page(state);
        let selected = state.frontend.mods.page.as_ref().and_then(|rows| {
            rows.iter()
                .position(|row| row.folder.as_deref() == Some(folder))
        });
        Self::update_mods_list(&mut state.frontend, selected, true);
    }

    /// New Mod's box: Open Folder shows the new mod's folder, or says where it
    /// is when no file manager starts.
    pub(super) fn answer_mod_made(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        dir: &std::path::Path,
        open: bool,
    ) {
        if !open {
            return;
        }
        #[cfg(not(target_os = "android"))]
        if !crate::app::web_link::open_folder(dir) {
            Self::show_folder_is_here(
                window,
                renderer,
                frontend,
                process_assets,
                options_profile,
                dir,
            );
        }
        #[cfg(target_os = "android")]
        let _ = (
            window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            dir,
        );
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
    fn use_picked_mod(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
    ) {
        let selected = Self::mods_selected(&frontend.shell_controller);
        let mods = &frontend.mods;
        let Some(row) = mods.switch_to(selected) else {
            return;
        };
        let folder = row.folder.clone();
        if mods.in_use(row) {
            Self::keep_mod_choice(
                platform,
                renderer,
                frontend,
                process_assets,
                options_profile,
                folder.as_deref(),
            );
            return;
        }
        let title = match &row.folder {
            Some(_) => tr_fill("VERA:ModRestartUse", &[("mod", &row.title())]),
            None => tr("VERA:ModRestartOriginal"),
        };
        let (restart, cancel) = (tr("VERA:ModRestart"), tr("VERA:Cancel"));
        let purpose = ShellMessageBoxPurpose::ModRestart(folder);
        Self::open_mods_box(
            &platform.window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            title,
            restart,
            cancel,
            purpose,
        );
    }

    /// Keep `folder`, or the original game, as the choice for the next start.
    /// False when config.toml could not be written: nothing changes, and a
    /// notice names the file.
    fn keep_mod_choice(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        folder: Option<&str>,
    ) -> bool {
        let kept = match platform.game_config.as_ref() {
            Some(config) => frontend.mods.keep_choice(&config.paths, folder),
            None => Err(std::io::Error::other("no config was read")),
        };
        let Err(error) = kept else {
            return true;
        };
        log::warn!("Mods: the choice was not saved: {error}");
        let file = crate::util::config::GameConfig::path()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        Self::open_shell_notice(
            &platform.window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            tr_fill("VERA:ModChoiceNotSaved", &[("file", &file)]),
        );
        false
    }

    /// The restart question's answer. Restart keeps the choice for the next
    /// start, then leaves the way Exit Game does; the game starts again once
    /// this copy is gone (`app::restart`).
    pub(super) fn answer_mod_restart(state: &mut AppState, folder: Option<String>, restart: bool) {
        if !restart {
            return;
        }
        if Self::keep_mod_choice(
            &state.platform,
            &state.renderer,
            &mut state.frontend,
            &state.process_assets,
            &state.persistence.options_profile,
            folder.as_deref(),
        ) {
            log::info!("Mods: restarting to use {folder:?}");
            crate::app::restart::ask();
            // As Exit Game leaves: from the empty backdrop, the page gone.
            Self::close_mods_page(state);
            Self::confirm_quit(state);
        }
    }
}

/// Android: New Mod's name dialog' open flag and whether its edit was cleared
/// since it opened.
#[cfg(target_os = "android")]
mod name_dialog {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicU8};
    use vera20k_gui::{Gui, NodeId};

    pub(super) static OPEN: AtomicBool = AtomicBool::new(false);
    pub(super) static CLEARED: AtomicBool = AtomicBool::new(false);
    /// 0 closed, 1 New Mod, 2 rename.
    pub(super) static MODE: AtomicU8 = AtomicU8::new(0);
    pub(super) static RENAME_FROM: Mutex<String> = Mutex::new(String::new());

    /// New Mod's dialog widgets under the page's root, once the page built
    /// them. They sit flat in the page's dialog so its painter draws them.
    pub(super) struct Nodes {
        pub(super) prompt: NodeId,
        pub(super) edit: NodeId,
        pub(super) ok: NodeId,
        pub(super) close: NodeId,
    }

    pub(super) fn find(gui: &Gui, root: NodeId) -> Option<Nodes> {
        let child = |id: i32| {
            gui.tree()
                .children(root)
                .ok()?
                .iter()
                .copied()
                .find(|node| gui.id_of(*node) == Some(id))
        };
        Some(Nodes {
            prompt: child(super::mods_shell::NAME_PROMPT.into())?,
            edit: child(super::mods_shell::NAME_EDIT.into())?,
            ok: child(super::mods_shell::NAME_OK.into())?,
            close: child(super::mods_shell::NAME_CLOSE.into())?,
        })
    }
}
