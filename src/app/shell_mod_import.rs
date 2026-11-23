//! Add Mod collects the parts of one mod, then prepares them off the UI thread.
//! Only the open dialog can make the finished folder visible in Mods.
//!
//! Its boxes are [`ShellBox`]es in VERA's [`Footer`] arrangement. While the
//! parts are previewed, More, Move Up and Move Down stand over the footer and
//! a `List` node of the box's dialog holds the ordered parts: the gui owns its
//! selection, top and scrollbar; [`Selection`] keeps them while a box is
//! rebuilt.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use crate::app::frontend::state::ShellMessageBoxPurpose;
use crate::app::mod_import_picker;
use crate::i18n::{tr, tr_fill};
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::modal::{BoxParts, Footer, ShellBox, control};
use crate::util::mod_package::{ModImport, PreparedModImport};
use crate::util::time::{Duration, Instant};
use vera20k_gui::shell::list::{ListMetrics, ListScrollPart, ROW_HEIGHT, ShellListGeometry};
use vera20k_gui::{NodeId, NodeKind, Prop, Value};

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
    rows: Vec<String>,
    /// The list's selected row and top, kept while its box is rebuilt.
    selected: Option<usize>,
    top: usize,
    can_browse: bool,
    choosing_kind: bool,
    error: Option<String>,
}

const ADD_MORE: u16 = 0x7E37;
const MOVE_UP: u16 = 0x7E38;
const MOVE_DOWN: u16 = 0x7E39;

/// Every Add Mod box: More over OK, the reorder buttons over the third
/// button and Cancel.
const FOOTER: &[(u16, u16)] = &[
    (ADD_MORE, control::OK),
    (MOVE_UP, control::THIRD),
    (MOVE_DOWN, control::CANCEL),
];

/// The parts list shows seven rows.
const LIST_ROWS: i32 = 7;
/// The row text's inset on each side, in 800x600 pixels.
const ROW_TEXT_INSET: i32 = 4;

impl Selection {
    fn new() -> Self {
        Self {
            plan: None,
            rows: Vec::new(),
            selected: None,
            top: 0,
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
        let selected = self.selected.and_then(|index| {
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
        self.rows = plan
            .parts()
            .filter_map(|path| path.file_name())
            .enumerate()
            .map(|(index, name)| format!("{}. {}", index + 1, name.to_string_lossy()))
            .collect();
        self.selected = (selected < self.rows.len()).then_some(selected);
    }

    fn move_target(&self, id: u16) -> Option<(usize, usize)> {
        let from = self.selected?;
        let to = match id {
            MOVE_UP => from.checked_sub(1)?,
            MOVE_DOWN => from + 1,
            _ => return None,
        };
        (to < self.rows.len()).then_some((from, to))
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

    /// The box: its question and buttons, and in the preview More and the
    /// reorder buttons, each greyed where it has nothing to do.
    fn shell_box(&self) -> ShellBox {
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
        let mut shell_box = import_box(ShellBox::message(title, confirm, cancel, third), self.previewing());
        if self.previewing() {
            if self.can_browse {
                shell_box = shell_box.button(ADD_MORE, tr("VERA:ModImportMore"));
            }
            for (id, label) in [
                (MOVE_UP, "VERA:ModImportMoveUp"),
                (MOVE_DOWN, "VERA:ModImportMoveDown"),
            ] {
                shell_box = shell_box
                    .button(id, tr(label))
                    .disabled(id, self.move_target(id).is_none());
            }
        }
        shell_box
    }
}

/// `shell_box` in Add Mod's arrangement, with room for the parts `list`.
fn import_box(shell_box: ShellBox, list: bool) -> ShellBox {
    shell_box.footer(Footer {
        list,
        raised: FOOTER,
    })
}

/// Whether `id` is one of the preview's own buttons, which act inside the
/// box ([`App::activate_mod_import_button`]).
pub(crate) fn is_preview_button(id: u16) -> bool {
    matches!(id, ADD_MORE | MOVE_UP | MOVE_DOWN)
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

fn preview_mut(state: &mut AppState) -> Option<&mut Selection> {
    let ShellMessageBoxPurpose::ModImport(dialog) =
        &mut state.frontend.shell_message_box.as_mut()?.purpose
    else {
        return None;
    };
    match dialog.as_mut() {
        Dialog::Select(selection) if selection.previewing() => Some(selection),
        _ => None,
    }
}

/// The parts list under the body, the body's width, seven rows high.
fn list_rect(layout: &BoxParts) -> RectPx {
    let list = layout.scale.derive_rect(layout.body, |body| {
        RectPx::new(
            body.x,
            body.y + body.h + 8,
            body.w - 1,
            LIST_ROWS * ROW_HEIGHT + 1,
        )
    });
    RectPx::new(list.x, list.y, list.w + 1, list.h + 1)
}

/// The open preview's box layout and dialog.
fn preview_box(state: &AppState) -> Option<(&Selection, BoxParts, DialogResourceId)> {
    let selection = preview(state)?;
    let shell_box = &state.frontend.shell_message_box.as_ref()?.shell_box;
    let layout = crate::app::frontend::shell_layout::shell_box_parts(state, shell_box);
    Some((selection, layout, shell_box.dialog))
}

/// The preview box's `List` node, if built.
fn list_node(state: &AppState, dialog: DialogResourceId) -> Option<NodeId> {
    let controller = &state.frontend.shell_controller;
    let gui = controller.gui();
    let children = gui.tree().children(controller.root_in(dialog)?).ok()?;
    children
        .iter()
        .copied()
        .find(|node| gui.tree().kind(*node) == Ok(NodeKind::List))
}

/// Builds the preview's parts list in its box once (with no id, so the
/// box's buttons still build), from the selection's rows, selected row and
/// top, the selected row in view; keeps it on its rect and row count.
pub(crate) fn sync(state: &mut AppState) -> Option<NodeId> {
    let (selection, layout, dialog) = preview_box(state)?;
    let rect = list_rect(&layout);
    let (rows, selected, top) = (selection.rows.len(), selection.selected, selection.top);
    let scale = layout.scale;
    let built = list_node(state, dialog);
    let controller = &mut state.frontend.shell_controller;
    // The list's metrics follow the box's scale, as its paint does.
    controller.set_scale_in(dialog, scale);
    let root = controller.root_in(dialog)?;
    let gui = controller.gui_mut();
    let int = |v: usize| Value::Int(i32::try_from(v).unwrap_or(i32::MAX));
    let node = match built {
        Some(node) => node,
        None => {
            let node = gui.create(NodeKind::List, Some(root)).ok()?;
            let _ = gui.set(node, Prop::RowCount, int(rows));
            let _ = gui.set(node, Prop::Rect, Value::Rect(rect));
            let _ = gui.set(node, Prop::Top, int(top));
            let _ = gui.set(node, Prop::Selected, selected.map_or(Value::Int(-1), int));
            if let Some(row) = selected {
                let _ = gui.ensure_row_visible(node, row);
            }
            node
        }
    };
    let _ = gui.set(node, Prop::RowCount, int(rows));
    let _ = gui.set(node, Prop::Rect, Value::Rect(rect));
    Some(node)
}

/// The selection's own change of its rows or selected row: the list takes
/// them and shows the selected row.
fn push_selection(state: &mut AppState) {
    let Some(node) = sync(state) else {
        return;
    };
    let Some(selection) = preview(state) else {
        return;
    };
    let int = |v: usize| Value::Int(i32::try_from(v).unwrap_or(i32::MAX));
    let selected = selection.selected;
    let gui = state.frontend.shell_controller.gui_mut();
    let _ = gui.set(node, Prop::Selected, selected.map_or(Value::Int(-1), int));
    if let Some(row) = selected {
        let _ = gui.ensure_row_visible(node, row);
    }
    follow_list(state);
}

/// After an input on the open preview: the selection keeps the list's
/// selected row and top, and the reorder buttons grey where they have
/// nothing to move.
pub(crate) fn follow_list(state: &mut AppState) {
    let Some((_, _, dialog)) = preview_box(state) else {
        return;
    };
    let view = list_node(state, dialog)
        .and_then(|node| state.frontend.shell_controller.gui().list_view(node));
    let Some(message) = state.frontend.shell_message_box.as_mut() else {
        return;
    };
    let ShellMessageBoxPurpose::ModImport(dialog) = &mut message.purpose else {
        return;
    };
    let Dialog::Select(selection) = dialog.as_mut() else {
        return;
    };
    if let Some(view) = view {
        selection.selected = view.selected;
        selection.top = view.top;
    }
    for id in [MOVE_UP, MOVE_DOWN] {
        let disabled = selection.move_target(id).is_none();
        if let Some(button) = message.shell_box.buttons.iter_mut().find(|b| b.id == id) {
            button.enabled = !disabled;
        }
    }
}

/// What the open preview's list paints: its geometry, its rows, top and
/// selected row, and the scrollbar part shown pushed.
pub(crate) struct PartsView<'a> {
    pub(crate) geometry: ShellListGeometry,
    pub(crate) rows: &'a [String],
    pub(crate) top: usize,
    pub(crate) selected: Option<usize>,
    pub(crate) pressed: Option<ListScrollPart>,
    /// Each row's text inset, in 800x600 pixels.
    pub(crate) text_inset: i32,
}

pub(crate) fn parts_view(state: &AppState) -> Option<PartsView<'_>> {
    let (selection, layout, dialog) = preview_box(state)?;
    let view = list_node(state, dialog)
        .and_then(|node| state.frontend.shell_controller.gui().list_view(node));
    let top = view.map_or(selection.top, |view| view.top);
    let selected = view.map_or(selection.selected, |view| view.selected);
    let metrics = ListMetrics::scaled(layout.scale);
    Some(PartsView {
        geometry: ShellListGeometry::new(list_rect(&layout), selection.rows.len(), top, metrics),
        rows: &selection.rows,
        top,
        selected,
        pressed: view.and_then(|view| view.pressed),
        text_inset: ROW_TEXT_INSET,
    })
}

/// The box's own input reaches the parts list through the gui; this only
/// makes sure the list is there first. Never takes the press itself.
pub(super) fn pointer_button(state: &mut AppState, _pressed: bool) -> bool {
    sync(state);
    false
}

/// The cursor drags a captured thumb and moves on or off a held arrow.
pub(super) fn cursor_moved(state: &mut AppState) {
    let Some(node) = sync(state) else {
        return;
    };
    let controller = &state.frontend.shell_controller;
    if controller
        .top_id()
        .and_then(|top| controller.pressed_node_in(top))
        != Some(node)
    {
        return;
    }
    let (x, y) = state.match_state.input.cursor_px();
    let controller = &mut state.frontend.shell_controller;
    controller.on_pointer_move(x, y);
    controller.drain_events();
    follow_list(state);
}

/// Three rows a wheel notch.
pub(super) fn wheel(state: &mut AppState, lines: f32) {
    let Some(node) = sync(state) else {
        return;
    };
    let controller = &mut state.frontend.shell_controller;
    let now = controller.now();
    let gui = controller.gui_mut();
    let _ = gui.set(node, Prop::WheelStep, Value::Int(1));
    gui.wheel_list(node, (lines * 3.0) as i32, now);
    gui.drain_events();
    follow_list(state);
    state.platform.window.request_redraw();
}

/// A held arrow's repeat; the deadline of the next one.
fn poll_scroll(state: &mut AppState) -> Option<Instant> {
    sync(state)?;
    let window = &state.platform.window;
    let next = state
        .frontend
        .shell_controller
        .poll_then(|| window.request_redraw());
    follow_list(state);
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
        let shell_box = selection.shell_box();
        Self::show_import_box(state, shell_box, Dialog::Select(selection));
        // A new box's list shows the selected row.
        sync(state);
    }

    /// `shell_box` for `dialog`, in place of any box already open.
    fn show_import_box(state: &mut AppState, shell_box: ShellBox, dialog: Dialog) {
        if state.frontend.shell_message_box.is_some() {
            Self::close_shell_message_box_from_controller(state);
        }
        let purpose = ShellMessageBoxPurpose::ModImport(Box::new(dialog));
        Self::open_shell_message_box(state, shell_box, purpose);
    }

    pub(super) fn mod_import_drop(state: &mut AppState, path: PathBuf) {
        follow_list(state);
        let Some(message) = state.frontend.shell_message_box.as_mut() else {
            return;
        };
        let ShellMessageBoxPurpose::ModImport(dialog) = &mut message.purpose else {
            return;
        };
        let Dialog::Select(selection) = dialog.as_mut() else {
            return;
        };
        selection.add([path]);
        let shell_box = selection.shell_box();
        let ids = |shell_box: &ShellBox| -> Vec<u16> {
            shell_box.buttons.iter().map(|button| button.id).collect()
        };
        // Other buttons make another box; the same ones keep it, and its list.
        if ids(&shell_box) == ids(&message.shell_box) {
            message.shell_box = shell_box;
            push_selection(state);
        } else if let Some(message) = state.frontend.shell_message_box.take() {
            Self::close_shell_message_box_from_controller(state);
            Self::open_shell_message_box(state, shell_box, message.purpose);
            sync(state);
        }
        state.platform.window.request_redraw();
    }

    pub(super) fn activate_mod_import_button(state: &mut AppState, id: u16) {
        follow_list(state);
        let Some(selection) = preview_mut(state) else {
            return;
        };
        if id != ADD_MORE {
            if selection.move_selected(id) {
                push_selection(state);
                state.platform.window.request_redraw();
            }
            return;
        }
        let Some(message) = state.frontend.shell_message_box.take() else {
            return;
        };
        let ShellMessageBoxPurpose::ModImport(dialog) = message.purpose else {
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

    /// The box's answer: 0 its first button, 1 Cancel (and Enter or
    /// Escape), 2 the third.
    pub(super) fn answer_mod_import(state: &mut AppState, dialog: Dialog, answer: usize) {
        state.platform.window.request_redraw();
        let Dialog::Select(mut selection) = dialog else {
            return;
        };
        if selection.previewing() {
            if let action @ (0 | 2) = answer
                && let Some(plan) = selection.plan.take()
            {
                Self::prepare_mod_import(state, plan, action == 0);
            }
            return;
        }
        match answer {
            0 if selection.can_browse => Self::browse_mod_parts(state, selection, false),
            0 => Self::open_mods_folder(state),
            2 if selection.choosing_kind() => Self::browse_mod_parts(state, selection, true),
            1 if selection.choosing_kind => {
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
        let shell_box = ShellBox::message(title, tr("VERA:Cancel"), String::new(), String::new());
        Self::show_import_box(
            state,
            import_box(shell_box, false),
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
        let selected = Self::mods_selected(state);
        if play && state.frontend.mods.switch_to(selected).is_some() {
            Self::answer_mod_restart(state, Some(folder), true);
            return;
        }
        let row = state
            .frontend
            .mods
            .page
            .as_ref()
            .and_then(|rows| selected.and_then(|row| rows.get(row)));
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
        let ok = state.csf_label("TXT_OK", "OK");
        Self::show_import_box(state, import_box(ShellBox::notice(title, ok), false), Dialog::Notice);
    }
}
