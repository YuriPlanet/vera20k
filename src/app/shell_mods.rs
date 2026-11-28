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
use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::shell_layout::mods_layout;
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::app::frontend::state::FrontendState;
use crate::app::frontend::state::{ModFilesBox, ShellMessageBoxPurpose};
use crate::app::match_runtime::state::MatchState;
use crate::app::mod_files::{Against, Checked, PageCheck, Step};
use crate::app::persistence::PersistenceState;
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::shell_route::ShellRoute;
use crate::i18n::{tr, tr_fill};
use crate::render::gpu::GpuContext;
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
    pub(super) fn open_mods_page(
        game_config: Option<&GameConfig>,
        gpu: &GpuContext,
        frontend: &mut FrontendState,
    ) {
        Self::destroy_current_shell_dialog(frontend);
        frontend.mods.page = Some(Self::installed_mods(game_config));
        Self::show_shell_page(frontend, ShellRoute::Mods, mods_shell::DIALOG);
        Self::mods_sync(gpu, frontend);
    }

    /// The row the page's list has selected.
    pub(super) fn mods_selected(controller: &DialogController) -> Option<usize> {
        family_list_selected(controller, mods_shell::DIALOG)
    }

    /// The window is in front again, perhaps from the folder that Open Mods
    /// Folder showed: list what `Mods` holds now. The picked row stays
    /// picked while its folder is there.
    pub(super) fn refresh_mods_page(
        game_config: Option<&GameConfig>,
        frontend: &mut FrontendState,
    ) {
        if crate::app::input::precedence::route_page(frontend)
            != crate::app::input::precedence::ScreenPage::Mods
        {
            return;
        }
        let rows = Self::installed_mods(game_config);
        let selected = Self::mods_selected(&frontend.shell_controller);
        let Some(page) = frontend.mods.page.as_mut() else {
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
        Self::update_mods_list(frontend, selected, false);
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
    fn mods_sync(gpu: &GpuContext, frontend: &mut FrontendState) -> ModsLayout {
        let layout = mods_layout(gpu);
        let mods = &frontend.mods;
        let in_use = mods.page.as_ref().map(|rows| {
            let selected = rows.iter().position(|row| mods.in_use(row));
            (rows.len(), selected)
        });
        let controller = &mut frontend.shell_controller;
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
    pub(super) fn handle_mods_wheel(gpu: &GpuContext, frontend: &mut FrontendState, lines: f32) {
        Self::mods_sync(gpu, frontend);
        let controller = &mut frontend.shell_controller;
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
        let layout = Self::mods_sync(&state.renderer.gpu, &mut state.frontend);
        let selected = Self::mods_selected(&state.frontend.shell_controller);
        let mods = &state.frontend.mods;
        #[cfg(target_os = "android")]
        let (picked, naming) = (
            Self::picked_folder(&state.frontend).is_some(),
            mods.name_dialog.is_some(),
        );
        let enabled = [
            (mods_shell::USE, mods.switch_to(selected).is_some()),
            (mods_shell::CHECK, mods.picked_is_mod(selected)),
            #[cfg(target_os = "android")]
            (mods_shell::RENAME, picked),
            #[cfg(target_os = "android")]
            (mods_shell::DELETE, picked),
            #[cfg(target_os = "android")]
            (mods_shell::OPEN_FOLDER, picked),
            #[cfg(target_os = "android")]
            (mods_shell::PIN, picked),
            #[cfg(target_os = "android")]
            (mods_shell::NAME_OK, naming),
            #[cfg(target_os = "android")]
            (mods_shell::NAME_CLOSE, naming),
        ];
        // Android: the name dialog's OK and Close sit on the box template's
        // buttons, over the page.
        #[cfg(target_os = "android")]
        let names = Self::name_dialog_parts(&state.renderer.gpu, &state.frontend);
        let rect_of = |id| {
            #[cfg(target_os = "android")]
            if let Some(rect) = names.rect_of(id) {
                return Some(rect);
            }
            layout.rect_of(id)
        };
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
            #[cfg(target_os = "android")]
            Some(id)
                if state.frontend.mods.name_dialog.is_some()
                    && id != mods_shell::NAME_OK
                    && id != mods_shell::NAME_CLOSE => {}
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
            Some(mods_shell::ADD) => {
                let selected =
                    Self::mods_selected(&state.frontend.shell_controller).and_then(|row| {
                        state
                            .frontend
                            .mods
                            .page
                            .as_ref()
                            .and_then(|rows| rows.get(row))
                            .and_then(|listed| listed.folder.clone())
                    });
                match selected {
                    Some(folder) => Self::ask_add_or_append(
                        &state.platform.window,
                        &state.renderer,
                        &mut state.frontend,
                        &state.process_assets,
                        &state.persistence.options_profile,
                        folder,
                    ),
                    None => Self::open_add_mod(
                        &state.platform.window,
                        &state.renderer,
                        &mut state.frontend,
                        &state.process_assets,
                        &state.persistence.options_profile,
                    ),
                }
            }
            #[cfg(target_os = "android")]
            Some(mods_shell::NAME_OK) => Self::answer_name_dialog(state, true),
            #[cfg(target_os = "android")]
            Some(mods_shell::NAME_CLOSE) => Self::answer_name_dialog(state, false),
            #[cfg(target_os = "android")]
            Some(mods_shell::RENAME) => {
                if let Some(folder) = Self::picked_folder(&state.frontend) {
                    Self::open_name_dialog(
                        &mut state.frontend,
                        crate::app::mods::NameDialog::Rename(folder),
                    );
                }
            }
            #[cfg(target_os = "android")]
            Some(mods_shell::DELETE) => Self::ask_delete_picked_mod(
                &state.platform.window,
                &state.renderer,
                &mut state.frontend,
                &state.process_assets,
                &state.persistence.options_profile,
            ),
            #[cfg(target_os = "android")]
            Some(mods_shell::PIN) => Self::pin_picked_mod(
                &state.platform,
                &state.renderer,
                &mut state.frontend,
                &state.process_assets,
                &state.persistence.options_profile,
            ),
            #[cfg(target_os = "android")]
            Some(mods_shell::OPEN_FOLDER) => Self::ask_export_picked_mod(
                &state.platform.window,
                &state.renderer,
                &mut state.frontend,
                &state.process_assets,
                &state.persistence.options_profile,
            ),
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
            // Android asks for the new mod's name first.
            #[cfg(target_os = "android")]
            Some(mods_shell::NEW) => {
                Self::open_name_dialog(&mut state.frontend, crate::app::mods::NameDialog::New)
            }
            #[cfg(not(target_os = "android"))]
            Some(mods_shell::NEW) => Self::make_new_mod(
                &state.platform,
                &state.renderer,
                &mut state.frontend,
                &state.process_assets,
                &state.persistence.options_profile,
                &tr("VERA:ModNewFolder"),
            ),
            Some(mods_shell::BACK) => {
                Self::leave_shell_dialog(state, ShellSlideKind::Mods, |state| {
                    App::close_mods_page(&mut state.frontend);
                    App::open_launcher_options_dialog(
                        &state.platform,
                        &state.renderer,
                        &mut state.frontend,
                        &state.process_assets,
                        &state.audio,
                        &state.persistence.options_profile,
                    );
                })
            }
            _ => {}
        }
    }

    /// Android: the folder of the row the list has selected, if it is a mod.
    #[cfg(target_os = "android")]
    fn picked_folder(frontend: &FrontendState) -> Option<String> {
        let selected = Self::mods_selected(&frontend.shell_controller)?;
        frontend.mods.page.as_ref()?.get(selected)?.folder.clone()
    }

    /// Android: what a Mods page action came to, with OK, over the page.
    #[cfg(target_os = "android")]
    fn mods_notice(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        text: String,
    ) {
        if frontend.shell_message_box.is_some() {
            Self::close_shell_message_box_from_controller(frontend);
        }
        let ok = process_assets.csf_label("TXT_OK", "OK");
        Self::open_shell_message_box(
            window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            ShellBox::notice(text, ok),
            ShellMessageBoxPurpose::ModNotice,
        );
    }

    /// Android: Delete Mod asks first, over the page.
    #[cfg(target_os = "android")]
    fn ask_delete_picked_mod(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
    ) {
        let Some(folder) = Self::picked_folder(frontend) else {
            return;
        };
        Self::open_mods_box(
            window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            tr("VERA:ModDeleteConfirm"),
            tr("VERA:ModDelete"),
            tr("VERA:Cancel"),
            ShellMessageBoxPurpose::ModDelete(folder),
        );
    }

    /// Android: the delete box's answer. Delete removes the folder (the app
    /// owns it) and its home-screen shortcuts, drops it as the next start's
    /// choice, and says what came of it.
    #[cfg(target_os = "android")]
    pub(super) fn answer_mod_delete(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        folder: &str,
        delete: bool,
    ) {
        platform.window.request_redraw();
        if !delete {
            return;
        }
        let Some(config) = platform.game_config.as_ref() else {
            return;
        };
        let dir = config.paths.mods_dir().join(folder);
        // The id goes with the folder: read it first.
        let id = crate::app::mods::mod_id(&dir).unwrap_or_default();
        let text = match std::fs::remove_dir_all(&dir) {
            Ok(()) => {
                log::info!("Mods: deleted {}", dir.display());
                if !crate::util::mod_actions::forget(folder, &id) {
                    log::warn!("Mods: the shortcuts of {folder} were not dropped");
                }
                if config.paths.mod_name() == Some(folder) {
                    let _ = Self::keep_mod_choice(
                        platform,
                        renderer,
                        frontend,
                        process_assets,
                        options_profile,
                        None,
                    );
                }
                tr_fill("VERA:ModDeleted", &[("mod", &folder)])
            }
            Err(error) => {
                log::warn!("Mods: could not delete {}: {error}", dir.display());
                tr("VERA:ModActionFailed")
            }
        };
        Self::refresh_mods_page(platform.game_config.as_ref(), frontend);
        Self::mods_notice(
            &platform.window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            text,
        );
    }

    /// Android: Export Mod asks whether to pack a ZIP or copy the folder.
    #[cfg(target_os = "android")]
    fn ask_export_picked_mod(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
    ) {
        let Some(folder) = Self::picked_folder(frontend) else {
            return;
        };
        let title = tr_fill("VERA:ModExportChoice", &[("mod", &folder)]);
        let shell_box = ShellBox::message(
            title,
            tr("VERA:ModExportZip"),
            tr("VERA:Cancel"),
            tr("VERA:ModExportFolder"),
        );
        let shell_box = crate::app::shell_mod_import::choice_box(shell_box);
        Self::open_shell_message_box(
            window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            shell_box,
            ShellMessageBoxPurpose::ModExportChoice(folder),
        );
    }

    /// Android: the Export Mod box's answer: ZIP, a folder copy or cancel.
    #[cfg(target_os = "android")]
    pub(crate) fn answer_mod_export(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        folder: &str,
        zip: bool,
    ) {
        let done = tr_fill("VERA:ModExported", &[("mod", &folder)]);
        let failed = tr("VERA:ModActionFailed");
        let opened = if zip {
            let progress = tr_fill("VERA:ModExporting", &[("mod", &folder)]);
            crate::util::mod_actions::export_zip(folder, &done, &failed, &progress)
        } else {
            crate::util::mod_actions::export(folder, &done, &failed)
        };
        if !opened {
            log::warn!("Mods: no file manager to export {folder}");
            Self::mods_notice(
                window,
                renderer,
                frontend,
                process_assets,
                options_profile,
                failed,
            );
        }
    }

    /// Android: Add Desktop Entry, a home-screen shortcut for the folder,
    /// keyed by the id it keeps across a rename.
    #[cfg(target_os = "android")]
    fn pin_picked_mod(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
    ) {
        let Some(folder) = Self::picked_folder(frontend) else {
            return;
        };
        let id = platform
            .game_config
            .as_ref()
            .and_then(|config| {
                crate::app::mods::ensure_mod_id(&config.paths.mods_dir().join(&folder))
            })
            .unwrap_or_default();
        if !crate::util::mod_actions::pin(&folder, &id) {
            log::warn!("Mods: the launcher took no shortcut for {folder}");
            Self::mods_notice(
                &platform.window,
                renderer,
                frontend,
                process_assets,
                options_profile,
                tr("VERA:ModActionFailed"),
            );
        }
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

    /// New Mod: a folder `name` in `Mods` with the original game's main INI
    /// files (`app::new_mod`), picked in the list, under a box that says what
    /// it holds and, on the desktop, offers to show it. Android names it in
    /// the page's name dialog first.
    fn make_new_mod(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        name: &str,
    ) {
        let Some(config) = platform.game_config.as_ref() else {
            return;
        };
        let paths = &config.paths;
        let mods_dir = paths.mods_dir();
        crate::app::mods::prepare_folder(&mods_dir);
        // The game folder with no mod on it, whatever this run plays: the
        // run's own archives may hold a mod's copies.
        let made = crate::assets::asset_manager::AssetManager::inspect_install(
            &paths.ra2_dir,
            process_assets.media_archive_mode(),
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
                    &platform.window,
                    renderer,
                    frontend,
                    process_assets,
                    options_profile,
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
        // Android: the folder gets the id a pinned shortcut finds it by.
        #[cfg(target_os = "android")]
        let _ = crate::app::mods::ensure_mod_id(&made.dir);
        Self::select_installed_mod(platform.game_config.as_ref(), frontend, &made.folder);
        let files = made.copied.join(", ");
        let text = tr_fill("VERA:ModMade", &[("mod", &made.folder), ("files", &files)]);
        // Android: no file manager opens the app's folders.
        #[cfg(target_os = "android")]
        Self::mods_notice(
            &platform.window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            text,
        );
        #[cfg(not(target_os = "android"))]
        let (open, close) = (tr("VERA:ModOpenItsFolder"), tr("VERA:Close"));
        #[cfg(not(target_os = "android"))]
        Self::open_mods_box(
            &platform.window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            text,
            open,
            close,
            ShellMessageBoxPurpose::ModMade(made.dir),
        );
    }

    /// Android: New Mod and Rename Mod ask for a name in a dialog of the page's
    /// own (`ui::mods_shell::NAME_EDIT`), so its edit keeps its text and focus
    /// across frames and the soft keyboard types into it (`app::input::ime`).
    #[cfg(target_os = "android")]
    fn open_name_dialog(frontend: &mut FrontendState, dialog: crate::app::mods::NameDialog) {
        use vera20k_gui::{Prop, Value};
        let text = match &dialog {
            crate::app::mods::NameDialog::New => String::new(),
            crate::app::mods::NameDialog::Rename(folder) => folder.clone(),
        };
        frontend.mods.name_dialog = Some(dialog);
        let controller = &mut frontend.shell_controller;
        let Some(nodes) = name_dialog_nodes(controller) else {
            return;
        };
        let gui = controller.gui_mut();
        let _ = gui.set(nodes.edit, Prop::Visible, Value::Bool(true));
        let _ = gui.set(nodes.edit, Prop::Text, Value::Text(text));
        let _ = gui.set_focus(Some(nodes.edit));
    }

    /// Android: the name dialog in window pixels, on the message box
    /// template's own dialog and buttons so it sits where the Mods page's boxes
    /// do, its panel in `PUDLGBGN.SHP`'s shape.
    #[cfg(target_os = "android")]
    fn name_dialog_parts(
        gpu: &GpuContext,
        frontend: &FrontendState,
    ) -> mods_shell::NameDialogRects {
        // Non-empty captions choose the two-button template (`0x120`), the one
        // Add Mod's and the Mods page's boxes use.
        let template = ShellBox::message(" ".into(), " ".into(), " ".into(), String::new());
        let parts = crate::app::frontend::shell_layout::shell_box_parts(gpu, &template);
        let aspect = frontend
            .skirmish_shell_chrome
            .as_ref()
            .and_then(|atlas| atlas.ra2_dialog_background)
            .map(|art| art.pixel_size[1] / art.pixel_size[0]);
        mods_shell::NameDialogRects::new(
            parts.dialog,
            parts.body,
            parts.button(0),
            parts.button(1),
            aspect,
        )
    }

    /// Android: keeps the name dialog's widgets on the page: OK and Close on
    /// the template's buttons, the edit off-screen (the overlay paints its
    /// field; the widget only takes keys), all hidden while it is closed.
    #[cfg(target_os = "android")]
    pub(crate) fn sync_name_dialog(gpu: &GpuContext, frontend: &mut FrontendState) {
        use vera20k_gui::{Prop, Value};
        let open = frontend.mods.name_dialog.is_some();
        let parts = Self::name_dialog_parts(gpu, frontend);
        let controller = &mut frontend.shell_controller;
        let Some(nodes) = name_dialog_nodes(controller) else {
            return;
        };
        let off_screen =
            crate::ui::shell::geom::RectPx::new(-4096, -4096, parts.ok.w.max(1), parts.ok.h.max(1));
        let gui = controller.gui_mut();
        for (node, rect) in [
            (nodes.edit, off_screen),
            (nodes.ok, parts.ok),
            (nodes.close, parts.close),
        ] {
            let _ = gui.set(node, Prop::Visible, Value::Bool(open));
            let _ = gui.set(node, Prop::Rect, Value::Rect(rect));
        }
        let _ = gui.set(nodes.ok, Prop::Enabled, Value::Bool(open));
        let _ = gui.set(nodes.close, Prop::Enabled, Value::Bool(open));
    }

    /// Add Mod with a mod selected: a new mod, or parts for that folder
    /// through the same picker.
    #[cfg(any(
        windows,
        target_os = "macos",
        target_os = "linux",
        target_os = "android"
    ))]
    fn ask_add_or_append(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        folder: String,
    ) {
        let title = tr_fill("VERA:ModAddChoice", &[("mod", &folder)]);
        let shell_box = ShellBox::message(
            title,
            tr("VERA:ModAddCreate"),
            tr("VERA:Cancel"),
            tr_fill("VERA:ModAppendTo", &[("mod", &folder)]),
        );
        let shell_box = crate::app::shell_mod_import::choice_box(shell_box);
        let purpose = ShellMessageBoxPurpose::ModAddChoice(folder);
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

    /// Android: a key pressed while the name dialog is up.
    #[cfg(target_os = "android")]
    pub(crate) fn name_dialog_key(state: &mut AppState, event: &winit::event::KeyEvent) {
        if !event.state.is_pressed() {
            return;
        }
        let winit::keyboard::PhysicalKey::Code(code) = event.physical_key else {
            return;
        };
        Self::name_dialog_input(state, Some(code), event.text.as_deref());
    }

    /// Android: a key or the input method's text for the name dialog: Enter
    /// answers OK, Escape Close, the rest types into its edit.
    #[cfg(target_os = "android")]
    pub(crate) fn name_dialog_input(
        state: &mut AppState,
        code: Option<winit::keyboard::KeyCode>,
        text: Option<&str>,
    ) {
        use winit::keyboard::KeyCode;
        match code {
            Some(KeyCode::Enter | KeyCode::NumpadEnter) => {
                return Self::answer_name_dialog(state, true);
            }
            Some(KeyCode::Escape) => return Self::answer_name_dialog(state, false),
            _ => {}
        }
        let shift = state.platform.live_modifiers.shift_key();
        for input in crate::ui::shell::controller::gui_inputs(code, shift, text) {
            state.frontend.shell_controller.on_input(input);
        }
        state.platform.window.request_redraw();
    }

    /// Android: OK makes the mod under the typed name (the default when it is
    /// blank) or renames the picked one; Close leaves everything as it was.
    #[cfg(target_os = "android")]
    pub(crate) fn answer_name_dialog(state: &mut AppState, ok: bool) {
        use vera20k_gui::{Prop, Value};
        let Some(dialog) = state.frontend.mods.name_dialog.take() else {
            return;
        };
        let controller = &mut state.frontend.shell_controller;
        let nodes = name_dialog_nodes(controller);
        let text = nodes
            .and_then(|nodes| match controller.gui().get(nodes.edit, Prop::Text) {
                Ok(Some(Value::Text(text))) => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let _ = controller.gui_mut().set_focus(None);
        state.platform.window.request_redraw();
        if !ok {
            return;
        }
        match dialog {
            crate::app::mods::NameDialog::New => {
                let typed = text.trim();
                let name = if typed.is_empty() {
                    tr("VERA:ModNewFolder")
                } else {
                    typed.to_owned()
                };
                Self::make_new_mod(
                    &state.platform,
                    &state.renderer,
                    &mut state.frontend,
                    &state.process_assets,
                    &state.persistence.options_profile,
                    &name,
                );
            }
            crate::app::mods::NameDialog::Rename(from) => Self::rename_mod(
                &state.platform,
                &state.renderer,
                &mut state.frontend,
                &state.process_assets,
                &state.persistence.options_profile,
                &from,
                &text,
            ),
        }
    }

    /// Android: renames the folder `from` to `typed`, keeps a shown `mod.ini`
    /// `Name=` and the next start's choice in step, and drops the shortcut
    /// that named the old folder (an id-keyed one follows the folder). A name
    /// that is not one plain folder name, or that another folder has, changes
    /// nothing and says so.
    #[cfg(target_os = "android")]
    fn rename_mod(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        from: &str,
        typed: &str,
    ) {
        let Some(config) = platform.game_config.as_ref() else {
            return;
        };
        let mods_dir = config.paths.mods_dir();
        let name = typed.trim();
        if name.is_empty() || name == from {
            return;
        }
        let from_dir = mods_dir.join(from);
        let taken = crate::util::case_insensitive_path::find(&mods_dir, name)
            .is_some_and(|found| found != from_dir);
        let renamed = if crate::app::new_mod::plain_name(name) != name || taken {
            Err(std::io::Error::other("not a free folder name"))
        } else {
            std::fs::rename(&from_dir, mods_dir.join(name))
        };
        if let Err(error) = renamed {
            log::warn!("Mods: could not rename {from} to {name}: {error}");
            Self::mods_notice(
                &platform.window,
                renderer,
                frontend,
                process_assets,
                options_profile,
                tr("VERA:ModRenameFailed"),
            );
            return;
        }
        log::info!("Mods: renamed {from} to {name}");
        set_mod_name(&mods_dir.join(name), name);
        let _ = crate::util::mod_actions::forget(from, "");
        if config.paths.mod_name() == Some(from) {
            let _ = Self::keep_mod_choice(
                platform,
                renderer,
                frontend,
                process_assets,
                options_profile,
                Some(name),
            );
        }
        Self::select_installed_mod(platform.game_config.as_ref(), frontend, name);
    }

    /// Android: paints the name dialog over the finished page, in the same
    /// pass as the message boxes, so its panel, buttons and text sit above the
    /// page's own text.
    #[cfg(target_os = "android")]
    pub(crate) fn draw_name_dialog_over(
        platform: &mut PlatformState,
        renderer: &RendererState,
        frontend: &FrontendState,
        match_state: &mut MatchState,
        persistence: &PersistenceState,
        encoder: &mut wgpu::CommandEncoder,
        destination: &wgpu::Texture,
    ) {
        use crate::app::frontend::shell_pass::{ShellComposition, TexturedDraw};
        use crate::render::shell_paint::{self, ModalButton, ModalDepths};
        use crate::render::shell_text::TextRect;
        use crate::ui::shell::geom::{RectPx, ShellAlign};
        let Some(dialog) = frontend.mods.name_dialog.as_ref() else {
            return;
        };
        let Some(atlas) = frontend.skirmish_shell_chrome.as_ref() else {
            return;
        };
        let scale = mods_layout(&renderer.gpu).page.scale;
        let parts = Self::name_dialog_parts(&renderer.gpu, frontend);
        let mut sprites = Vec::new();
        if let Some(art) = atlas.ra2_dialog_background {
            shell_paint::push_entry_sized(
                &mut sprites,
                art,
                parts.panel.x as f32,
                parts.panel.y as f32,
                [parts.panel.w as f32, parts.panel.h as f32],
                0.00070,
            );
        } else if let Some(white) = atlas.white_pixel {
            shell_paint::push_tinted_entry(
                &mut sprites,
                white,
                parts.panel,
                [0.03, 0.03, 0.04],
                0.00070,
            );
        }
        let frames = crate::app::frontend::skirmish_shell_render::type3_button_frames(atlas);
        let button = |rect| ModalButton {
            rect,
            pressed: false,
            enabled: true,
        };
        sprites.extend(shell_paint::paint_modal_sprites(
            None,
            frames,
            parts.panel,
            &[button(parts.ok), button(parts.close)],
            scale,
            ModalDepths {
                background: 0.00072,
                button: 0.00068,
                text: 0.00060,
            },
        ));
        let font = &renderer.bit_font;
        let enabled = shell_paint::SHELL_TEXT_RGB_ENABLED;
        let line = |text: &str, rect, align| {
            crate::render::shell_text::draw_in_rect_scaled(
                font,
                text,
                TextRect::of(rect),
                enabled,
                align,
                [0.0, 0.0],
                0.00060,
                scale.factor(),
            )
        };
        // Where the box's own text starts (`body`), as the Mods page's boxes
        // do: the hint on the first line, the field under it, one line plus
        // twice the text's top inset high.
        let body = parts.body;
        let line_h = (font.cell_height() * scale.factor()).round() as i32;
        let inset = (2.0 * scale.factor()).round() as i32;
        let field = RectPx::new(
            body.x,
            body.y + line_h + (line_h / 2).max(4),
            body.w,
            line_h + 2 * inset,
        );
        let mut text = Vec::new();
        let controller = &frontend.shell_controller;
        let node = controller
            .root_in(mods_shell::DIALOG)
            .and_then(|root| find_child(controller.gui(), root, mods_shell::NAME_EDIT));
        let edit_state = node
            .and_then(|node| controller.gui().edit_state(node))
            .unwrap_or_default();
        let caret = node.is_some_and(|node| controller.caret_visible(node));
        crate::app::frontend::skirmish_shell_render::push_new_edit(
            &mut sprites,
            &mut text,
            atlas,
            font,
            &edit_state,
            None,
            field,
            caret,
            enabled,
            0.00060,
            scale,
        );
        let hint = match dialog {
            crate::app::mods::NameDialog::New => tr("VERA:ModNewNameHint"),
            crate::app::mods::NameDialog::Rename(_) => tr("VERA:ModRenameHint"),
        };
        text.push(line(
            &hint,
            RectPx::new(body.x, body.y, body.w, line_h),
            ShellAlign::NONE,
        ));
        let centred = ShellAlign::H_CENTER | ShellAlign::V_CENTER;
        text.push(line(&tr("TXT_OK"), parts.ok, centred));
        text.push(line(&tr("VERA:Close"), parts.close, centred));
        let draws = [TexturedDraw {
            texture: &atlas.texture,
            instances: sprites,
        }];
        // The dialog's art covers the page, so the cursor it covered is drawn
        // again here, above the art.
        let cursor = crate::app::frontend::shell_pass::top_software_cursor(
            platform,
            renderer,
            frontend,
            match_state,
            persistence,
            shell_paint::CURSOR_DEPTH,
        );
        crate::app::frontend::shell_pass::encode_shell_pass_over_page(
            renderer,
            encoder,
            destination,
            "Mods Name Dialog",
            ShellComposition {
                draws: &draws,
                text: &text,
                cursor,
                effects: None,
                smooth_art: false,
            },
        );
    }

    /// Refresh the list and bring a mod we just added into view.
    pub(super) fn select_installed_mod(
        game_config: Option<&GameConfig>,
        frontend: &mut FrontendState,
        folder: &str,
    ) {
        Self::refresh_mods_page(game_config, frontend);
        let selected = frontend.mods.page.as_ref().and_then(|rows| {
            rows.iter()
                .position(|row| row.folder.as_deref() == Some(folder))
        });
        Self::update_mods_list(frontend, selected, true);
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
        if open && !crate::app::web_link::open_folder(dir) {
            Self::show_folder_is_here(
                window,
                renderer,
                frontend,
                process_assets,
                options_profile,
                dir,
            );
        }
    }

    /// The page is gone and the route is the main menu's again. Back builds
    /// Options over it next; a restart leaves from here.
    pub(super) fn close_mods_page(frontend: &mut FrontendState) {
        Self::destroy_current_shell_dialog(frontend);
        frontend.mods.page = None;
        frontend.shell_route = ShellRoute::MainMenu;
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
    pub(super) fn answer_mod_restart(
        platform: &PlatformState,
        renderer: &RendererState,
        diag: &DiagnosticsState,
        frontend: &mut FrontendState,
        match_state: &mut MatchState,
        process_assets: &ProcessAssets,
        audio: &mut AppAudioRuntime,
        persistence: &PersistenceState,
        folder: Option<String>,
        restart: bool,
    ) {
        if !restart {
            return;
        }
        if Self::keep_mod_choice(
            platform,
            renderer,
            frontend,
            process_assets,
            &persistence.options_profile,
            folder.as_deref(),
        ) {
            log::info!("Mods: restarting to use {folder:?}");
            crate::app::restart::ask();
            // As Exit Game leaves: from the empty backdrop, the page gone.
            Self::close_mods_page(frontend);
            Self::confirm_quit(
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
    }
}

/// Android: the name dialog's widgets under the Mods page's root, once the
/// page built them (`ui::mods_shell::nodes`).
#[cfg(target_os = "android")]
#[derive(Clone, Copy)]
struct NameDialogNodes {
    edit: vera20k_gui::NodeId,
    ok: vera20k_gui::NodeId,
    close: vera20k_gui::NodeId,
}

#[cfg(target_os = "android")]
fn name_dialog_nodes(controller: &DialogController) -> Option<NameDialogNodes> {
    let gui = controller.gui();
    let root = controller.root_in(mods_shell::DIALOG)?;
    Some(NameDialogNodes {
        edit: find_child(gui, root, mods_shell::NAME_EDIT)?,
        ok: find_child(gui, root, mods_shell::NAME_OK)?,
        close: find_child(gui, root, mods_shell::NAME_CLOSE)?,
    })
}

#[cfg(target_os = "android")]
fn find_child(
    gui: &vera20k_gui::Gui,
    root: vera20k_gui::NodeId,
    id: u16,
) -> Option<vera20k_gui::NodeId> {
    gui.tree()
        .children(root)
        .ok()?
        .iter()
        .copied()
        .find(|node| gui.id_of(*node) == Some(id.into()))
}

/// Android: `mod.ini`'s shown `Name=` follows a rename; a file without one
/// (or with only a commented `;Name=`) keeps showing the folder's name. Line
/// endings stay as the file has them.
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
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = String::with_capacity(text.len() + name.len());
    for line in text.lines() {
        if shown(line) {
            out.push_str("Name=");
            out.push_str(name);
        } else {
            out.push_str(line);
        }
        out.push_str(newline);
    }
    if let Err(error) = std::fs::write(&file, out) {
        log::warn!(
            "Mods: could not set the name in {}: {error}",
            file.display()
        );
    }
}
