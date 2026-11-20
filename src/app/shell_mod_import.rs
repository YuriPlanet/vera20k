//! Add Mod collects the parts of one mod, then prepares them off the UI thread.
//! Only the open dialog can make the finished folder visible in Mods.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use crate::app::frontend::state::{ShellMessageBox, ShellMessageBoxPurpose};
use crate::app::mod_import_picker;
use crate::i18n::{tr, tr_fill};
use crate::ui::main_menu_dialogs::ShellMessageBoxLabels;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::list::{ListBoxState, ListMetrics};
use crate::ui::shell::modal::{MessageBoxLayout, ModalResult};
use crate::util::mod_package::{ModImport, PreparedModImport};
use crate::util::time::{Duration, Instant};

use super::{App, AppState};

#[derive(Debug)]
pub(crate) enum Dialog {
    Select(Selection),
    Preparing(Preparation),
    Notice,
}

#[derive(Debug)]
pub(crate) struct Selection {
    plan: Option<ModImport>,
    // The plan owns the parts. These labels give the scrollable preview their order.
    rows: ListBoxState<String>,
    can_browse: bool,
    choosing_kind: bool,
    error: Option<String>,
}

impl Selection {
    fn new() -> Self {
        Self {
            plan: None,
            rows: ListBoxState::new(Vec::new(), None),
            can_browse: mod_import_picker::available(),
            choosing_kind: false,
            error: None,
        }
    }

    fn choosing_kind(&self) -> bool {
        !cfg!(target_os = "macos") && self.can_browse && (self.plan.is_none() || self.choosing_kind)
    }

    fn previewing(&self) -> bool {
        self.plan.is_some() && !self.choosing_kind()
    }

    fn add(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        let paths: Vec<_> = paths.into_iter().collect();
        if paths.is_empty() {
            return;
        }
        let selected = self.rows.selected.and_then(|index| {
            self.plan
                .as_ref()?
                .parts()
                .nth(index)
                .map(|part| part.to_owned())
        });
        let changed = match self.plan.as_mut() {
            Some(plan) => plan.add_paths(&paths),
            None => ModImport::plan(&paths).map(|plan| {
                self.plan = Some(plan);
                true
            }),
        };
        match changed {
            Ok(true) => {
                let index = selected.and_then(|selected| {
                    self.plan
                        .as_ref()?
                        .parts()
                        .position(|part| part == selected)
                });
                self.refresh_rows(index.unwrap_or(0));
                self.choosing_kind = false;
                self.error = None;
            }
            Ok(false) => {}
            Err(error) => self.selection_error(error),
        }
    }

    fn refresh_rows(&mut self, selected: usize) {
        let Some(plan) = self.plan.as_ref() else {
            return;
        };
        self.rows.rows = plan
            .parts()
            .filter_map(|path| path.file_name())
            .enumerate()
            .map(|(index, name)| format!("{}. {}", index + 1, name.to_string_lossy()))
            .collect();
        self.rows.selected = (selected < self.rows.rows.len()).then_some(selected);
        self.rows.scroll.cancel();
    }

    fn move_target(&self, id: u16) -> Option<(usize, usize)> {
        let from = self.rows.selected?;
        let to = match id {
            MOVE_UP => from.checked_sub(1)?,
            MOVE_DOWN => from + 1,
            _ => return None,
        };
        (to < self.rows.rows.len()).then_some((from, to))
    }

    fn move_selected(&mut self, id: u16) -> bool {
        let Some((from, to)) = self.move_target(id) else {
            return false;
        };
        let Some(plan) = self.plan.as_mut() else {
            return false;
        };
        if plan.move_part(from, to) {
            self.refresh_rows(to);
            true
        } else {
            false
        }
    }

    fn selection_error(&mut self, error: std::io::Error) {
        log::warn!("Mods: could not add parts: {error}");
        self.error = Some(tr_fill(
            "VERA:ModImportSelectionError",
            &[("error", &error)],
        ));
    }

    fn labels(&self) -> ShellMessageBoxLabels {
        let cancel = tr(if self.choosing_kind {
            "VERA:ModImportBack"
        } else {
            "VERA:Cancel"
        });
        let title = self.error.clone().unwrap_or_else(|| {
            tr(if self.previewing() {
                "VERA:ModImportOrder"
            } else if self.can_browse {
                "VERA:ModImportPrompt"
            } else {
                "VERA:ModImportDrop"
            })
        });
        let (confirm, third, cancel) = if self.choosing_kind() {
            (
                tr("VERA:ModImportZIPs"),
                tr("VERA:ModImportFolders"),
                cancel,
            )
        } else if self.previewing() {
            (tr("VERA:ModImportPlay"), tr("VERA:ModImportOnly"), cancel)
        } else if self.can_browse {
            (tr("VERA:ModImportBrowse"), String::new(), cancel)
        } else {
            (tr("VERA:ModsOpenFolder"), String::new(), cancel)
        };
        ShellMessageBoxLabels {
            title,
            confirm,
            third,
            cancel,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Preparation {
    play: bool,
    stop: Arc<AtomicBool>,
    result: Receiver<anyhow::Result<PreparedModImport>>,
}

impl Drop for Preparation {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn preview(state: &AppState) -> Option<&Selection> {
    let ShellMessageBoxPurpose::ModImport(dialog) =
        &state.frontend.shell_message_box.as_ref()?.purpose
    else {
        return None;
    };
    match dialog.as_ref() {
        Dialog::Select(selection) if selection.previewing() => Some(selection),
        _ => None,
    }
}

pub(crate) fn visible_parts(state: &AppState) -> Option<&ListBoxState<String>> {
    preview(state).map(|selection| &selection.rows)
}

const ADD_MORE: u16 = 0x7E37;
const MOVE_UP: u16 = 0x7E38;
const MOVE_DOWN: u16 = 0x7E39;

pub(crate) struct PreviewButton {
    pub(crate) id: u16,
    pub(crate) rect: RectPx,
    pub(crate) label: &'static str,
    pub(crate) enabled: bool,
}

/// Add and reorder buttons use the row above the install actions.
pub(crate) fn preview_buttons(
    state: &AppState,
    layout: MessageBoxLayout,
) -> impl Iterator<Item = PreviewButton> + '_ {
    preview(state).into_iter().flat_map(move |selection| {
        [
            (ADD_MORE, Some(layout.ok), "VERA:ModImportMore", true),
            (
                MOVE_UP,
                layout.third,
                "VERA:ModImportMoveUp",
                selection.move_target(MOVE_UP).is_some(),
            ),
            (
                MOVE_DOWN,
                layout.cancel,
                "VERA:ModImportMoveDown",
                selection.move_target(MOVE_DOWN).is_some(),
            ),
        ]
        .into_iter()
        .filter_map(move |(id, rect, label, enabled)| {
            if id == ADD_MORE && !selection.can_browse {
                return None;
            }
            Some(PreviewButton {
                id,
                rect: layout.scale.derive_rect(rect?, |rect| {
                    RectPx::new(rect.x, rect.y - rect.h - 6, rect.w, rect.h)
                }),
                label,
                enabled,
            })
        })
    })
}

/// Most of the panel is for the ordered parts. The actions are along its footer.
pub(crate) fn layout(base: MessageBoxLayout, has_parts: bool) -> MessageBoxLayout {
    const INSET: i32 = 24;
    let derive = |f: fn(RectPx) -> RectPx| base.scale.derive_rect(base.dialog, f);
    let body = derive(|dialog| {
        RectPx::new(
            dialog.x + INSET,
            dialog.y + INSET,
            dialog.w - 2 * INSET,
            dialog.h - 3 * INSET,
        )
    });
    let button = base.scale.base_rect(base.ok);
    let place = |slot| {
        base.scale.derive_rect(base.dialog, |dialog| {
            let x = match slot {
                0 => INSET,
                1 => (dialog.w - button.w) / 2,
                _ => dialog.w - INSET - button.w,
            };
            RectPx::new(
                dialog.x + x,
                dialog.y + dialog.h - INSET - button.h,
                button.w,
                button.h,
            )
        })
    };
    MessageBoxLayout {
        body: if has_parts {
            base.scale
                .derive_rect(body, |rect| RectPx::new(rect.x, rect.y, rect.w, 66))
        } else {
            body
        },
        ok: place(if base.cancel.is_some() { 0 } else { 2 }),
        third: base.third.map(|_| place(1)),
        cancel: base.cancel.map(|_| place(2)),
        ..base
    }
}

pub(crate) fn list_rect(layout: MessageBoxLayout) -> RectPx {
    layout.scale.derive_rect(layout.body, |body| {
        RectPx::new(
            body.x,
            body.y + body.h + 8,
            body.w - 1,
            7 * crate::ui::shell::list::ROW_HEIGHT + 1,
        )
    })
}

fn preview_mut(state: &mut AppState) -> Option<(&mut Selection, MessageBoxLayout)> {
    let kind = state.frontend.shell_message_box.as_ref()?.labels.kind();
    let layout = crate::app::frontend::shell_layout::shell_message_box_layout(state, kind);
    let ShellMessageBoxPurpose::ModImport(dialog) =
        &mut state.frontend.shell_message_box.as_mut()?.purpose
    else {
        return None;
    };
    match dialog.as_mut() {
        Dialog::Select(selection) if selection.previewing() => Some((selection, layout)),
        _ => None,
    }
}

pub(super) fn pointer_button(state: &mut AppState, pressed: bool) -> bool {
    let (x, y) = state.match_state.input.cursor_px();
    let Some((selection, layout)) = preview_mut(state) else {
        return false;
    };
    let parts = &mut selection.rows;
    if !pressed {
        parts.scroll.cancel();
        return false;
    }
    let list = list_rect(layout);
    let metrics = ListMetrics::scaled(layout.scale);
    if parts.scroll_press(list, metrics, x, y, Instant::now()) {
        return true;
    }
    let geometry = parts.geometry(list, metrics);
    let row_end = parts.rows.len().min(parts.top + geometry.visible_rows);
    if let Some(index) = geometry.row_at(row_end, parts.top, x, y) {
        parts.selected = Some(index);
        state.platform.window.request_redraw();
        return true;
    }
    false
}

fn reveal_selection(selection: &mut Selection, layout: MessageBoxLayout) {
    let parts = &mut selection.rows;
    if let Some(index) = parts.selected {
        let geometry = parts.geometry(list_rect(layout), ListMetrics::scaled(layout.scale));
        let first = index.saturating_sub(geometry.visible_rows.saturating_sub(1));
        parts.top = parts.top.clamp(first, index).min(geometry.max_top);
    }
}

pub(super) fn cursor_moved(state: &mut AppState) {
    let (x, y) = state.match_state.input.cursor_px();
    let Some((selection, layout)) = preview_mut(state) else {
        return;
    };
    let parts = &mut selection.rows;
    let top = parts.top;
    parts.scroll_pointer_moved(list_rect(layout), ListMetrics::scaled(layout.scale), x, y);
    if top != parts.top {
        state.platform.window.request_redraw();
    }
}

pub(super) fn wheel(state: &mut AppState, lines: f32) {
    let Some((selection, layout)) = preview_mut(state) else {
        return;
    };
    let parts = &mut selection.rows;
    let max_top = parts
        .geometry(list_rect(layout), ListMetrics::scaled(layout.scale))
        .max_top;
    parts.top = (parts.top as i32 - (lines * 3.0) as i32).clamp(0, max_top as i32) as usize;
    state.platform.window.request_redraw();
}

fn poll_scroll(state: &mut AppState) -> Option<Instant> {
    let (x, y) = state.match_state.input.cursor_px();
    let (selection, layout) = preview_mut(state)?;
    let parts = &mut selection.rows;
    let changed = parts.scroll_poll(
        list_rect(layout),
        ListMetrics::scaled(layout.scale),
        x,
        y,
        Instant::now(),
    );
    let next = parts.scroll.repeat_at();
    if changed {
        state.platform.window.request_redraw();
    }
    next
}

/// Keep Enter from cancelling Add Mod or accidentally cancelling a running import.
pub(super) fn enter(state: &AppState) -> Option<bool> {
    let ShellMessageBoxPurpose::ModImport(dialog) =
        &state.frontend.shell_message_box.as_ref()?.purpose
    else {
        return None;
    };
    Some(match dialog.as_ref() {
        Dialog::Select(_) | Dialog::Notice => true,
        Dialog::Preparing(_) => false,
    })
}

impl App {
    pub(super) fn open_add_mod(state: &mut AppState) {
        Self::show_import_selection(state, Selection::new());
    }

    fn show_import_selection(state: &mut AppState, selection: Selection) {
        let labels = selection.labels();
        Self::show_import_box(state, labels, Dialog::Select(selection));
        if let Some((selection, layout)) = preview_mut(state) {
            reveal_selection(selection, layout);
        }
    }

    fn show_import_box(state: &mut AppState, labels: ShellMessageBoxLabels, dialog: Dialog) {
        Self::open_shell_message_box(
            state,
            ShellMessageBox {
                labels,
                purpose: ShellMessageBoxPurpose::ModImport(Box::new(dialog)),
            },
        );
    }

    pub(super) fn mod_import_drop(state: &mut AppState, path: PathBuf) {
        let Some(message) = state.frontend.shell_message_box.as_mut() else {
            return;
        };
        let ShellMessageBoxPurpose::ModImport(dialog) = &mut message.purpose else {
            return;
        };
        let Dialog::Select(selection) = dialog.as_mut() else {
            return;
        };
        let old_kind = message.labels.kind();
        selection.add([path]);
        message.labels = selection.labels();
        if old_kind != message.labels.kind()
            && let Some(message) = state.frontend.shell_message_box.take()
        {
            Self::close_shell_message_box_from_controller(state);
            Self::open_shell_message_box(state, message);
        }
        if let Some((selection, layout)) = preview_mut(state) {
            reveal_selection(selection, layout);
        }
        state.platform.window.request_redraw();
    }

    pub(super) fn activate_mod_import_button(state: &mut AppState, id: u16) {
        let Some((selection, layout)) = preview_mut(state) else {
            return;
        };
        if id != ADD_MORE {
            if selection.move_selected(id) {
                reveal_selection(selection, layout);
                state.platform.window.request_redraw();
            }
            return;
        }
        let Some(ShellMessageBox {
            purpose: ShellMessageBoxPurpose::ModImport(dialog),
            ..
        }) = state.frontend.shell_message_box.take()
        else {
            return;
        };
        let Dialog::Select(mut selection) = *dialog else {
            return;
        };
        Self::close_shell_message_box_from_controller(state);
        if cfg!(target_os = "macos") {
            Self::browse_mod_parts(state, selection, false);
        } else {
            selection.choosing_kind = true;
            Self::show_import_selection(state, selection);
        }
    }

    pub(super) fn answer_mod_import(state: &mut AppState, dialog: Dialog, result: ModalResult) {
        state.platform.window.request_redraw();
        let Dialog::Select(mut selection) = dialog else {
            return;
        };
        if selection.previewing() {
            if let ModalResult::MessageBox(action @ (0 | 2)) = result
                && let Some(plan) = selection.plan.take()
            {
                Self::prepare_mod_import(state, plan, action == 0);
            }
            return;
        }
        match result {
            ModalResult::MessageBox(0) if selection.can_browse => {
                Self::browse_mod_parts(state, selection, false);
            }
            ModalResult::MessageBox(0) => Self::open_mods_folder(state),
            ModalResult::MessageBox(2) if selection.choosing_kind() => {
                Self::browse_mod_parts(state, selection, true);
            }
            ModalResult::MessageBox(1) if selection.choosing_kind => {
                selection.choosing_kind = false;
                Self::show_import_selection(state, selection);
            }
            _ => {}
        }
    }

    fn browse_mod_parts(state: &mut AppState, mut selection: Selection, folders: bool) {
        crate::app::input::hardware_cursor::give_back_arrow(state);
        crate::app::input::cursor::release_cursor_confinement(state);
        let window = &state.platform.window;
        #[cfg(not(target_os = "macos"))]
        let picked = if folders {
            mod_import_picker::pick_folders(window, &tr("VERA:ModImportFolders"))
        } else {
            mod_import_picker::pick(window, &tr("VERA:ModAdd"))
        };
        #[cfg(target_os = "macos")]
        let picked = {
            let _ = folders;
            mod_import_picker::pick(window, &tr("VERA:ModAdd"))
        };
        selection.choosing_kind = false;
        match picked {
            Ok(paths) => selection.add(paths),
            Err(error) => selection.selection_error(error),
        }
        Self::show_import_selection(state, selection);
    }

    fn prepare_mod_import(state: &mut AppState, plan: ModImport, play: bool) {
        let Some(config) = state.platform.game_config.as_ref() else {
            Self::mod_import_failed(
                state,
                anyhow::anyhow!("The game settings could not be read"),
            );
            return;
        };
        let mods = config.paths.mods_dir();
        let title = tr_fill("VERA:ModImportWorking", &[("mod", &plan.name())]);
        let stop = Arc::new(AtomicBool::new(false));
        let Some(result) = crate::app::worker::spawn("mod-import", stop.clone(), move |stop| {
            Ok(plan.prepare(&mods, stop)?)
        }) else {
            Self::mod_import_failed(state, anyhow::anyhow!("The import worker could not start"));
            return;
        };
        Self::show_import_box(
            state,
            ShellMessageBoxLabels {
                title,
                confirm: tr("VERA:Cancel"),
                third: String::new(),
                cancel: String::new(),
            },
            Dialog::Preparing(Preparation { play, stop, result }),
        );
    }

    pub(super) fn poll_mod_import(state: &mut AppState) -> Option<Instant> {
        let ShellMessageBoxPurpose::ModImport(dialog) =
            &state.frontend.shell_message_box.as_ref()?.purpose
        else {
            return None;
        };
        let Dialog::Preparing(preparation) = dialog.as_ref() else {
            return poll_scroll(state);
        };
        let result = match preparation.result.try_recv() {
            Err(TryRecvError::Empty) => return Some(Instant::now() + Duration::from_millis(100)),
            Err(TryRecvError::Disconnected) => Err(anyhow::anyhow!("The import worker stopped")),
            Ok(result) => result,
        };
        let play = preparation.play;
        Self::close_shell_message_box_from_controller(state);
        match result.and_then(|prepared| Ok(prepared.install()?)) {
            Ok(folder) => Self::mod_imported(state, folder, play),
            Err(error) => Self::mod_import_failed(state, error),
        }
        None
    }

    fn mod_imported(state: &mut AppState, folder: String, play: bool) {
        Self::select_installed_mod(state, &folder);
        if play && state.frontend.mods.switch_to().is_some() {
            Self::answer_mod_restart(state, Some(folder), true);
            return;
        }
        let row = state
            .frontend
            .mods
            .page
            .as_ref()
            .and_then(|page| page.selected.and_then(|row| page.rows.get(row)));
        let name = row.map_or_else(|| folder.clone(), |row| row.title().into_owned());
        let added = tr_fill("VERA:ModImported", &[("mod", &name)]);
        let title = if let Some(needs) = row.and_then(|row| row.needs.as_ref()) {
            format!(
                "{added}\n\n{}",
                tr_fill("VERA:ModNeeds", &[("needs", needs)])
            )
        } else {
            added
        };
        Self::mod_import_notice(state, title);
    }

    fn mod_import_failed(state: &mut AppState, error: anyhow::Error) {
        log::warn!("Mods: import failed: {error:#}");
        Self::mod_import_notice(state, tr_fill("VERA:ModImportFailed", &[("error", &error)]));
    }

    fn mod_import_notice(state: &mut AppState, title: String) {
        Self::show_import_box(
            state,
            ShellMessageBoxLabels {
                title,
                confirm: Self::csf_label(state, "TXT_OK", "OK"),
                cancel: String::new(),
                third: String::new(),
            },
            Dialog::Notice,
        );
    }
}
