//! VERA's Mods page (`ui::mods_shell`), opened by the Mods button of Options:
//! the list of what is in the `Mods` folder, and the buttons that switch to
//! one after a restart, or open the folder. gamemd has no such page. The list,
//! the choice and its place in config.toml are `app::mods`'.

use crate::util::time::Instant;

use super::{App, AppState};
use crate::app::frontend::shell_transition::ShellExitThen;
use crate::app::frontend::state::{ShellMessageBox, ShellMessageBoxPurpose};
use crate::app::shell_route::ShellRoute;
use crate::ui::mods_shell::{self, ModsLayout};
use crate::ui::shell::list::{ListBoxState, ListMetrics};

impl App {
    fn installed_mods(state: &AppState) -> Vec<crate::app::mods::Listed> {
        state
            .platform
            .game_config
            .as_ref()
            .map(|config| crate::app::mods::installed(&config.paths))
            .unwrap_or_default()
    }

    /// The Mods button of Options, once Options has closed: list what `Mods`
    /// holds now, with what this run uses selected and in view.
    pub(super) fn open_mods_page(state: &mut AppState) {
        Self::destroy_current_shell_dialog(state);
        let rows = Self::installed_mods(state);
        let selected = rows.iter().position(|row| state.frontend.mods.in_use(row));
        let mut page = ListBoxState::new(rows, selected);
        let layout = Self::mods_layout(state);
        let geometry = page.geometry(layout.list, ListMetrics::scaled(layout.page.scale));
        page.top = selected
            .unwrap_or(0)
            .saturating_sub(geometry.visible_rows.saturating_sub(1))
            .min(geometry.max_top);
        state.frontend.mods.page = Some(page);
        state.frontend.shell_route = ShellRoute::Mods;
        state
            .frontend
            .shell_controller
            .reset_to(mods_shell::DIALOG, false);
    }

    /// The window is in front again, perhaps from the folder that Open Mods
    /// Folder showed: list what `Mods` holds now. The picked row stays
    /// picked while its folder is there.
    pub(super) fn refresh_mods_page(state: &mut AppState) {
        if !Self::mods_page_active(state) {
            return;
        }
        let rows = Self::installed_mods(state);
        let layout = Self::mods_layout(state);
        let Some(page) = state.frontend.mods.page.as_mut() else {
            return;
        };
        if page.rows == rows {
            return;
        }
        let picked = page
            .selected
            .and_then(|row| page.rows.get(row))
            .map(|row| row.folder.clone());
        page.selected = picked.and_then(|folder| rows.iter().position(|row| row.folder == folder));
        page.rows = rows;
        let geometry = page.geometry(layout.list, ListMetrics::scaled(layout.page.scale));
        page.top = page.top.min(geometry.max_top);
        state.platform.window.request_redraw();
    }

    pub(super) fn mods_page_active(state: &AppState) -> bool {
        state.frontend.screen == crate::ui::game_screen::GameScreen::MainMenu
            && state.frontend.shell_route.mods()
            && state.frontend.mods.page.is_some()
    }

    fn mods_layout(state: &AppState) -> ModsLayout {
        crate::app::frontend::shell_layout::mods_layout(state)
    }

    /// The page's buttons, then its list: a hover target for status help,
    /// never a button press. Use This Mod is disabled while the selection is
    /// nothing a restart would change to.
    fn mods_feed(state: &mut AppState) -> Vec<crate::ui::shell::layout::LaidOutControl> {
        let layout = Self::mods_layout(state);
        let nothing_to_use = state.frontend.mods.switch_to().is_none();
        let controller = &mut state.frontend.shell_controller;
        controller.ensure_active(mods_shell::DIALOG, false);
        controller.set_disabled(mods_shell::USE, nothing_to_use);
        let mut feed = Self::menu_page_button_feed(&layout.page);
        feed.push(crate::ui::shell::layout::LaidOutControl {
            id: mods_shell::LIST,
            rect: layout.list,
        });
        feed
    }

    fn mods_button_feed(state: &mut AppState) -> Vec<crate::ui::shell::layout::LaidOutControl> {
        Self::mods_feed(state)
            .into_iter()
            .filter(|control| control.id != mods_shell::LIST)
            .collect()
    }

    pub(super) fn handle_mods_mouse_move(state: &mut AppState) {
        let feed = Self::mods_feed(state);
        let (x, y) = state.match_state.input.cursor_px();
        state.frontend.shell_controller.on_pointer_move(x, y, &feed);
        let layout = Self::mods_layout(state);
        if let Some(page) = state.frontend.mods.page.as_mut() {
            page.scroll_pointer_moved(layout.list, ListMetrics::scaled(layout.page.scale), x, y);
        }
        // Every hover message repaints the status line (0x00615EF7).
        state.frontend.shell_status_line.repaint();
    }

    /// Scrollbar arrow auto-repeat; returns the next wake deadline.
    pub(super) fn poll_mods_scroll(state: &mut AppState) -> Option<Instant> {
        if !Self::mods_page_active(state) {
            return None;
        }
        let (x, y) = state.match_state.input.cursor_px();
        let layout = Self::mods_layout(state);
        let page = state.frontend.mods.page.as_mut()?;
        if page.scroll_poll(
            layout.list,
            ListMetrics::scaled(layout.page.scale),
            x,
            y,
            Instant::now(),
        ) {
            state.platform.window.request_redraw();
        }
        page.scroll.repeat_at()
    }

    /// The wheel moves the list three rows a notch, as on the Keyboard page.
    pub(super) fn handle_mods_wheel(state: &mut AppState, lines: f32) {
        let layout = Self::mods_layout(state);
        let Some(page) = state.frontend.mods.page.as_mut() else {
            return;
        };
        let geometry = page.geometry(layout.list, ListMetrics::scaled(layout.page.scale));
        page.top =
            (page.top as i32 - (lines * 3.0) as i32).clamp(0, geometry.max_top as i32) as usize;
        state.platform.window.request_redraw();
    }

    pub(super) fn handle_mods_mouse_down(state: &mut AppState) {
        let (x, y) = state.match_state.input.cursor_px();
        let layout = Self::mods_layout(state);
        let metrics = ListMetrics::scaled(layout.page.scale);
        if let Some(page) = state.frontend.mods.page.as_mut()
            && layout.list.contains(x, y)
        {
            // As in the movie list: the scrollbar takes its own presses
            // without a sound, and a press on a row selects it.
            if page.scroll_press(layout.list, metrics, x, y, Instant::now()) {
                return;
            }
            if let Some(row) = page.row_at(layout.list, metrics, x, y) {
                page.selected = Some(row);
                Self::play_generic_click_sound(state);
            }
            return;
        }
        let buttons = Self::mods_button_feed(state);
        state
            .frontend
            .shell_controller
            .on_pointer_down(x, y, &buttons);
        if state.frontend.shell_controller.pressed().is_some() {
            Self::play_main_menu_button_sound(state);
        }
    }

    pub(super) fn handle_mods_mouse_up(state: &mut AppState) {
        if let Some(page) = state.frontend.mods.page.as_mut() {
            page.scroll.cancel();
        }
        let buttons = Self::mods_button_feed(state);
        let (x, y) = state.match_state.input.cursor_px();
        match state
            .frontend
            .shell_controller
            .on_pointer_up(x, y, &buttons)
        {
            Some(mods_shell::USE) => Self::use_picked_mod(state),
            Some(mods_shell::OPEN_FOLDER) => Self::open_mods_folder(state),
            Some(mods_shell::BACK) => Self::leave_shell_dialog(state, ShellExitThen::ModsBack),
            _ => {}
        }
    }

    /// Open Mods Folder. Where no file manager starts, say where the folder
    /// is.
    fn open_mods_folder(state: &mut AppState) {
        let opened = state
            .platform
            .game_config
            .as_ref()
            .map(|config| crate::app::mods::open_folder(&config.paths));
        if let Some(Err(folder)) = opened {
            Self::open_shell_notice(
                state,
                crate::i18n::tr_fill("VERA:ModsFolderIsHere", &[("folder", &folder.display())]),
            );
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
        let mods = &state.frontend.mods;
        let Some(row) = mods.switch_to() else {
            return;
        };
        let folder = row.folder.clone();
        if mods.in_use(row) {
            Self::keep_mod_choice(state, folder.as_deref());
            return;
        }
        let title = match &row.folder {
            Some(_) => crate::i18n::tr_fill("VERA:ModRestartUse", &[("mod", &row.title())]),
            None => crate::i18n::tr("VERA:ModRestartOriginal"),
        };
        Self::open_shell_message_box(
            state,
            ShellMessageBox {
                labels: crate::ui::main_menu_dialogs::ShellMessageBoxLabels {
                    title,
                    confirm: crate::i18n::tr("VERA:ModRestart"),
                    third: String::new(),
                    cancel: crate::i18n::tr("VERA:Cancel"),
                },
                purpose: ShellMessageBoxPurpose::ModRestart(folder),
            },
        );
    }

    /// Keep `folder`, or the original game, as the choice for the next start.
    /// False when config.toml could not be written: nothing changes, and a
    /// notice names the file.
    fn keep_mod_choice(state: &mut AppState, folder: Option<&str>) -> bool {
        let kept = match state.platform.game_config.as_ref() {
            Some(config) => state.frontend.mods.keep_choice(&config.paths, folder),
            None => Err(std::io::Error::other("no config was read")),
        };
        state.platform.window.request_redraw();
        let Err(error) = kept else {
            return true;
        };
        log::warn!("Mods: the choice was not saved: {error}");
        let file = crate::util::config::GameConfig::path()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        Self::open_shell_notice(
            state,
            crate::i18n::tr_fill("VERA:ModChoiceNotSaved", &[("file", &file)]),
        );
        false
    }

    /// The restart question's answer. Restart keeps the choice for the next
    /// start, then leaves the way Exit Game does; the game starts again once
    /// this copy is gone (`app::restart`).
    pub(super) fn answer_mod_restart(state: &mut AppState, folder: Option<String>, restart: bool) {
        if !restart {
            state.platform.window.request_redraw();
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
