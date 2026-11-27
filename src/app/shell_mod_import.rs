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
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::modal::{BoxParts, Footer, ShellBox, control};
use crate::util::mod_package::{ModImport, PreparedModAppend, PreparedModImport};
use crate::util::time::{Duration, Instant};
use vera20k_gui::shell::list::ROW_HEIGHT;
use vera20k_gui::{NodeId, NodeKind, Prop, Value};

use super::App;
use crate::app::PlatformState;
use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::state::FrontendState;
use crate::app::frontend::state::ShellMessageBox;
use crate::app::input::state::MatchInputState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::render::bit_font::BitFont;
use crate::render::gpu::GpuContext;
use crate::ui::shell::controller::DialogController;
use winit::window::Window;

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
    /// The text width the list's `Items` were last cut to from `rows`;
    /// `None` once the rows change ([`sync`] writes them again).
    listed: Option<u32>,
    /// The list's selected row and top, kept while its box is rebuilt.
    selected: Option<usize>,
    top: usize,
    can_browse: bool,
    choosing_kind: bool,
    /// The selected installed mod when adding parts.
    append: Option<String>,
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
            listed: None,
            selected: None,
            top: 0,
            can_browse: mod_import_picker::available(),
            choosing_kind: false,
            append: None,
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
        self.listed = None;
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
            if let Some(mod_name) = self.append.as_deref() {
                if self.previewing() {
                    return tr_fill("VERA:ModAppendPrompt", &[("mod", &mod_name)]);
                }
                if self.can_browse {
                    return tr_fill("VERA:ModAppendChoose", &[("mod", &mod_name)]);
                }
            }
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
        } else if self.previewing() && self.append.is_some() {
            (tr("VERA:ModAppend"), String::new(), cancel)
        } else if self.previewing() {
            (tr("VERA:ModImportPlay"), tr("VERA:ModImportOnly"), cancel)
        } else if self.can_browse {
            (tr("VERA:ModImportBrowse"), String::new(), cancel)
        } else {
            (tr("VERA:ModsOpenFolder"), String::new(), cancel)
        };
        let mut shell_box = import_box(
            ShellBox::message(title, confirm, cancel, third),
            self.previewing(),
        );
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

/// Add Mod's box arrangement for a box outside the picker (the Mods page's
/// New-or-append question), so both share one layout: three buttons on the
/// footer row, the same body area.
pub(super) fn choice_box(shell_box: ShellBox) -> ShellBox {
    import_box(shell_box, false)
}

/// Add Mod's footer, with room for the parts list when shown.
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
    result: Receiver<anyhow::Result<Prepared>>,
}

#[derive(Debug)]
enum Prepared {
    New(PreparedModImport),
    Append {
        folder: String,
        files: PreparedModAppend,
    },
}

impl Drop for Preparation {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn preview(message_box: Option<&ShellMessageBox>) -> Option<&Selection> {
    let ShellMessageBoxPurpose::ModImport(dialog) = &message_box?.purpose else {
        return None;
    };
    match dialog.as_ref() {
        Dialog::Select(selection) if selection.previewing() => Some(selection),
        _ => None,
    }
}

fn preview_mut(message_box: &mut Option<ShellMessageBox>) -> Option<&mut Selection> {
    let ShellMessageBoxPurpose::ModImport(dialog) = &mut message_box.as_mut()?.purpose else {
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
    // Android: More/Up/Down stand above the footer buttons; the list stops
    // short of them so its backing cannot cover their captions.
    #[cfg(target_os = "android")]
    let list = {
        let bottom = layout
            .rect_of(ADD_MORE)
            .map(|raised| raised.y - 6)
            .unwrap_or(list.y + list.h);
        RectPx::new(
            list.x,
            list.y,
            list.w,
            (bottom - list.y - 1).max(ROW_HEIGHT),
        )
    };
    RectPx::new(list.x, list.y, list.w + 1, list.h + 1)
}

/// The open preview's box layout and dialog.
fn preview_box<'a>(
    gpu: &GpuContext,
    frontend: &'a FrontendState,
) -> Option<(&'a Selection, BoxParts, DialogResourceId)> {
    let selection = preview(frontend.shell_message_box.as_ref())?;
    let shell_box = &frontend.shell_message_box.as_ref()?.shell_box;
    let layout = crate::app::frontend::shell_layout::shell_box_parts(gpu, shell_box);
    Some((selection, layout, shell_box.dialog))
}

/// The preview box's `List` node, if built.
fn list_node(controller: &DialogController, dialog: DialogResourceId) -> Option<NodeId> {
    let gui = controller.gui();
    let children = gui.tree().children(controller.root_in(dialog)?).ok()?;
    children
        .iter()
        .copied()
        .find(|node| gui.tree().kind(*node) == Ok(NodeKind::List))
}

/// The parts list's host id: past the box's 16-bit control ids, so the
/// box's rect sync leaves its window alone and no press names it (the
/// update offer's list is `0x1_0001`).
pub(crate) const LIST_ID: i32 = 0x1_0002;

/// The open preview's parts list node, if built.
pub(crate) fn node(gpu: &GpuContext, frontend: &FrontendState) -> Option<NodeId> {
    let (_, _, dialog) = preview_box(gpu, frontend)?;
    list_node(&frontend.shell_controller, dialog)
}

/// Builds the preview's parts list in its box once, after the box's
/// buttons (its id counts as built), from the selection's rows, selected
/// row and top, the selected row in view; keeps it on its rect and its
/// rows' text cut to the row.
pub(crate) fn sync(renderer: &RendererState, frontend: &mut FrontendState) -> Option<NodeId> {
    let (selection, layout, dialog) = preview_box(&renderer.gpu, frontend)?;
    let rect = list_rect(&layout);
    let (rows, selected, top) = (selection.rows.len(), selection.selected, selection.top);
    let scale = layout.scale;
    let built = list_node(&frontend.shell_controller, dialog);
    let shell_box = &frontend.shell_message_box.as_ref()?.shell_box;
    let controller = &mut frontend.shell_controller;
    // The list's metrics follow the box's scale, as its paint does.
    controller.set_scale_in(dialog, scale);
    let root = controller.root_in(dialog)?;
    let gui = controller.gui_mut();
    let int = |v: usize| Value::Int(i32::try_from(v).unwrap_or(i32::MAX));
    let node = match built {
        Some(node) => node,
        None => {
            crate::ui::shell::page::ensure_built(gui, root, || shell_box.nodes());
            let node = gui.create(NodeKind::List, Some(root)).ok()?;
            let _ = gui.set(node, Prop::Id, Value::Int(LIST_ID));
            // Each row's text inset from both ends.
            let inset = RectPx::new(ROW_TEXT_INSET, 0, ROW_TEXT_INSET, 0);
            let _ = gui.set(node, Prop::TextInset, Value::Rect(inset));
            // Blank rows until [`list_rows`] writes their text below.
            let _ = gui.set(node, Prop::Items, Value::Items(vec![String::new(); rows]));
            let _ = gui.set(node, Prop::Rect, Value::Rect(rect));
            let _ = gui.set(node, Prop::Top, int(top));
            let _ = gui.set(node, Prop::Selected, selected.map_or(Value::Int(-1), int));
            if let Some(row) = selected {
                let _ = gui.ensure_row_visible(node, row);
            }
            node
        }
    };
    let _ = gui.set(node, Prop::Rect, Value::Rect(rect));
    let width = row_text_width(gui, node, scale);
    list_rows(&renderer.bit_font, frontend, node, width, built.is_none());
    Some(node)
}

/// Every row's text width in 800x600 pixels: the first row's, less the
/// list's text insets.
fn row_text_width(gui: &vera20k_gui::Gui, node: NodeId, scale: ShellScale) -> u32 {
    gui.list_geometry(node).map_or(0, |geometry| {
        let row = geometry.row(0);
        let inset = match gui.get(node, Prop::TextInset) {
            Ok(Some(Value::Rect(inset))) => *inset,
            _ => RectPx::default(),
        };
        let text = scale.derive_rect(row, |row| {
            RectPx::new(row.x + inset.x, row.y, row.w - inset.x - inset.w, row.h)
        });
        scale.base_rect(text).w.max(0) as u32
    })
}

/// The selection's rows into the list's `Items`, each cut to `width`: only
/// when the list is new, the rows changed or the width did, so a steady
/// frame copies no text.
fn list_rows(
    bit_font: &BitFont,
    frontend: &mut FrontendState,
    node: NodeId,
    width: u32,
    new: bool,
) {
    let font = bit_font;
    let Some(message) = frontend.shell_message_box.as_mut() else {
        return;
    };
    let ShellMessageBoxPurpose::ModImport(dialog) = &mut message.purpose else {
        return;
    };
    let Dialog::Select(selection) = dialog.as_mut() else {
        return;
    };
    if !new && selection.listed == Some(width) {
        return;
    }
    selection.listed = Some(width);
    let items = (selection.rows.iter())
        .map(|name| font.truncate_to_width(name, width).into_owned())
        .collect();
    let gui = frontend.shell_controller.gui_mut();
    let _ = gui.set(node, Prop::Items, Value::Items(items));
}

/// The selection's own change of its rows or selected row: the list takes
/// them and shows the selected row.
fn push_selection(renderer: &RendererState, frontend: &mut FrontendState) {
    let Some(node) = sync(renderer, frontend) else {
        return;
    };
    let Some(selection) = preview(frontend.shell_message_box.as_ref()) else {
        return;
    };
    let int = |v: usize| Value::Int(i32::try_from(v).unwrap_or(i32::MAX));
    let selected = selection.selected;
    let gui = frontend.shell_controller.gui_mut();
    let _ = gui.set(node, Prop::Selected, selected.map_or(Value::Int(-1), int));
    if let Some(row) = selected {
        let _ = gui.ensure_row_visible(node, row);
    }
    follow_list(&renderer.gpu, frontend);
}

/// After an input on the open preview: the selection keeps the list's
/// selected row and top, and the reorder buttons grey where they have
/// nothing to move.
pub(crate) fn follow_list(gpu: &GpuContext, frontend: &mut FrontendState) {
    let Some((_, _, dialog)) = preview_box(gpu, frontend) else {
        return;
    };
    let view = list_node(&frontend.shell_controller, dialog)
        .and_then(|node| frontend.shell_controller.gui().list_view(node));
    let Some(message) = frontend.shell_message_box.as_mut() else {
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

/// The box's own input reaches the parts list through the gui; this only
/// makes sure the list is there first. Never takes the press itself.
pub(super) fn pointer_button(
    renderer: &RendererState,
    frontend: &mut FrontendState,
    _pressed: bool,
) -> bool {
    sync(renderer, frontend);
    false
}

/// The cursor drags a captured thumb and moves on or off a held arrow.
pub(super) fn cursor_moved(
    renderer: &RendererState,
    frontend: &mut FrontendState,
    input: &MatchInputState,
) {
    let Some(node) = sync(renderer, frontend) else {
        return;
    };
    let controller = &frontend.shell_controller;
    if controller
        .top_id()
        .and_then(|top| controller.pressed_node_in(top))
        != Some(node)
    {
        return;
    }
    let (x, y) = input.cursor_px();
    let controller = &mut frontend.shell_controller;
    controller.on_pointer_move(x, y);
    controller.drain_events();
    follow_list(&renderer.gpu, frontend);
}

/// Three rows a wheel notch.
pub(super) fn wheel(
    window: &Window,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    lines: f32,
) {
    let Some(node) = sync(renderer, frontend) else {
        return;
    };
    let controller = &mut frontend.shell_controller;
    let now = controller.now();
    let gui = controller.gui_mut();
    let _ = gui.set(node, Prop::WheelStep, Value::Int(1));
    gui.wheel_list(node, (lines * 3.0) as i32, now);
    gui.drain_events();
    follow_list(&renderer.gpu, frontend);
    window.request_redraw();
}

/// A held arrow's repeat; the deadline of the next one.
fn poll_scroll(
    window: &Window,
    renderer: &RendererState,
    frontend: &mut FrontendState,
) -> Option<Instant> {
    sync(renderer, frontend)?;
    let next = frontend
        .shell_controller
        .poll_then(|| window.request_redraw());
    follow_list(&renderer.gpu, frontend);
    next
}

/// Keep Enter from cancelling Add Mod or accidentally cancelling a running import.
pub(super) fn enter(message_box: Option<&ShellMessageBox>) -> Option<bool> {
    let ShellMessageBoxPurpose::ModImport(dialog) = &message_box?.purpose else {
        return None;
    };
    Some(match dialog.as_ref() {
        Dialog::Select(_) | Dialog::Notice => true,
        Dialog::Preparing(_) => false,
    })
}

impl App {
    pub(super) fn open_add_mod(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
    ) {
        Self::show_import_selection(
            window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            Selection::new(),
        );
    }

    /// Add parts to an installed mod's folder: the Mods page's Add Mod asks
    /// first when a mod is selected, since a file manager cannot extend a mod
    /// inside the app's own storage on Android and a desktop mod's folder is
    /// the same operation.
    pub(super) fn open_append_import(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        folder: String,
    ) {
        let mut selection = Selection::new();
        selection.append = Some(folder);
        Self::show_import_selection(
            window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            selection,
        );
    }

    /// Android: a ZIP or folder the system file manager copied in joins the
    /// selection, and its box is rebuilt with the preview's buttons. A part
    /// that arrives after Add Mod closed is deleted.
    #[cfg(target_os = "android")]
    fn drain_picked_part(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
    ) {
        let Some(path) = crate::util::mod_actions::take_picked() else {
            return;
        };
        let Some(shell) = frontend.shell_message_box.take() else {
            crate::util::mod_actions::clear_picked();
            return;
        };
        let ShellMessageBoxPurpose::ModImport(dialog) = shell.purpose else {
            frontend.shell_message_box = Some(shell);
            crate::util::mod_actions::clear_picked();
            return;
        };
        let Dialog::Select(mut selection) = *dialog else {
            // An import is running from an earlier part: it keeps its box,
            // and its end clears the copies.
            frontend.shell_message_box = Some(ShellMessageBox {
                purpose: ShellMessageBoxPurpose::ModImport(dialog),
                ..shell
            });
            return;
        };
        selection.add(vec![path]);
        // The old box's tree has no More/Up/Down button nodes: drop its dialog
        // so the box built below makes a fresh one with them.
        Self::close_shell_message_box_from_controller(frontend);
        Self::show_import_selection(
            window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            selection,
        );
    }

    fn show_import_selection(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        selection: Selection,
    ) {
        let shell_box = selection.shell_box();
        Self::show_import_box(
            window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            shell_box,
            Dialog::Select(selection),
        );
        // A new box's list shows the selected row.
        sync(renderer, frontend);
    }

    /// `shell_box` for `dialog`, in place of any box already open.
    fn show_import_box(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        shell_box: ShellBox,
        dialog: Dialog,
    ) {
        if frontend.shell_message_box.is_some() {
            Self::close_shell_message_box_from_controller(frontend);
        }
        let purpose = ShellMessageBoxPurpose::ModImport(Box::new(dialog));
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

    pub(super) fn mod_import_drop(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        path: PathBuf,
    ) {
        follow_list(&renderer.gpu, frontend);
        let Some(message) = frontend.shell_message_box.as_mut() else {
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
            push_selection(renderer, frontend);
        } else if let Some(message) = frontend.shell_message_box.take() {
            Self::close_shell_message_box_from_controller(frontend);
            Self::open_shell_message_box(
                window,
                renderer,
                frontend,
                process_assets,
                options_profile,
                shell_box,
                message.purpose,
            );
            sync(renderer, frontend);
        }
        window.request_redraw();
    }

    pub(super) fn activate_mod_import_button(
        platform: &mut PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        id: u16,
    ) {
        follow_list(&renderer.gpu, frontend);
        let Some(selection) = preview_mut(&mut frontend.shell_message_box) else {
            return;
        };
        if id != ADD_MORE {
            if selection.move_selected(id) {
                push_selection(renderer, frontend);
                platform.window.request_redraw();
            }
            return;
        }
        let Some(message) = frontend.shell_message_box.take() else {
            return;
        };
        let ShellMessageBoxPurpose::ModImport(dialog) = message.purpose else {
            return;
        };
        let Dialog::Select(mut selection) = *dialog else {
            return;
        };
        Self::close_shell_message_box_from_controller(frontend);
        if cfg!(target_os = "macos") {
            Self::browse_mod_parts(
                platform,
                renderer,
                frontend,
                process_assets,
                options_profile,
                selection,
                false,
            );
        } else {
            selection.choosing_kind = true;
            Self::show_import_selection(
                &platform.window,
                renderer,
                frontend,
                process_assets,
                options_profile,
                selection,
            );
        }
    }

    /// The box's answer: 0 its first button, 1 Cancel (and Enter or
    /// Escape), 2 the third.
    pub(super) fn answer_mod_import(
        platform: &mut PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        dialog: Dialog,
        answer: usize,
    ) {
        platform.window.request_redraw();
        // Android: an answer that closes Add Mod without starting an import or
        // a new pick deletes what the pickers copied in.
        #[cfg(target_os = "android")]
        {
            let goes_on = match &dialog {
                Dialog::Select(selection) if selection.previewing() => {
                    matches!(answer, 0 | 2) && selection.plan.is_some()
                }
                // The arms below: browse, back to the preview, browse folders.
                Dialog::Select(selection) => match answer {
                    0 => selection.can_browse,
                    1 => selection.choosing_kind,
                    2 => selection.choosing_kind(),
                    _ => false,
                },
                Dialog::Preparing(_) | Dialog::Notice => false,
            };
            if !goes_on {
                crate::util::mod_actions::clear_picked();
            }
        }
        let Dialog::Select(mut selection) = dialog else {
            return;
        };
        if selection.previewing() {
            if let action @ (0 | 2) = answer
                && let Some(plan) = selection.plan.take()
            {
                Self::prepare_mod_import(
                    platform,
                    renderer,
                    frontend,
                    process_assets,
                    options_profile,
                    plan,
                    action == 0,
                    selection.append.clone(),
                );
            }
            return;
        }
        match answer {
            0 if selection.can_browse => Self::browse_mod_parts(
                platform,
                renderer,
                frontend,
                process_assets,
                options_profile,
                selection,
                false,
            ),
            0 => Self::open_mods_folder(
                platform,
                renderer,
                frontend,
                process_assets,
                options_profile,
            ),
            2 if selection.choosing_kind() => Self::browse_mod_parts(
                platform,
                renderer,
                frontend,
                process_assets,
                options_profile,
                selection,
                true,
            ),
            1 if selection.choosing_kind => {
                selection.choosing_kind = false;
                Self::show_import_selection(
                    &platform.window,
                    renderer,
                    frontend,
                    process_assets,
                    options_profile,
                    selection,
                );
            }
            _ => {}
        }
    }

    fn browse_mod_parts(
        platform: &mut PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        mut selection: Selection,
        folders: bool,
    ) {
        crate::app::input::hardware_cursor::give_back_arrow(platform);
        crate::app::input::cursor::release_cursor_confinement(platform);
        // Android picks through the system file manager, which answers later:
        // the part joins the selection in `drain_picked_part`.
        #[cfg(target_os = "android")]
        {
            let failed = tr("VERA:ModActionFailed");
            let opened = if folders {
                crate::util::mod_actions::pick_folder(&failed)
            } else {
                crate::util::mod_actions::pick_zip(&failed)
            };
            if !opened {
                selection.selection_error(std::io::Error::other("no file manager"));
            }
        }
        #[cfg(not(target_os = "android"))]
        {
            let window = &platform.window;
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
            match picked {
                Ok(paths) => selection.add(paths),
                Err(error) => selection.selection_error(error),
            }
        }
        selection.choosing_kind = false;
        Self::show_import_selection(
            &platform.window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            selection,
        );
    }

    fn prepare_mod_import(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        plan: ModImport,
        play: bool,
        append: Option<String>,
    ) {
        let Some(config) = platform.game_config.as_ref() else {
            Self::mod_import_failed(
                &platform.window,
                renderer,
                frontend,
                process_assets,
                options_profile,
                anyhow::anyhow!("The game settings could not be read"),
            );
            return;
        };
        let mods = config.paths.mods_dir();
        let title = match append.as_deref() {
            Some(mod_name) => tr_fill("VERA:ModAppendPrompt", &[("mod", &mod_name)]),
            None => tr_fill("VERA:ModImportWorking", &[("mod", &plan.name())]),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let Some(result) = crate::app::worker::spawn("mod-import", stop.clone(), move |stop| {
            Ok(match append {
                Some(folder) => Prepared::Append {
                    files: plan.prepare_append(&mods, &folder, stop)?,
                    folder,
                },
                None => Prepared::New(plan.prepare(&mods, stop)?),
            })
        }) else {
            Self::mod_import_failed(
                &platform.window,
                renderer,
                frontend,
                process_assets,
                options_profile,
                anyhow::anyhow!("The import worker could not start"),
            );
            return;
        };
        let shell_box = ShellBox::message(title, tr("VERA:Cancel"), String::new(), String::new());
        Self::show_import_box(
            &platform.window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            import_box(shell_box, false),
            Dialog::Preparing(Preparation { play, stop, result }),
        );
    }

    pub(super) fn poll_mod_import(
        platform: &PlatformState,
        renderer: &RendererState,
        diag: &DiagnosticsState,
        frontend: &mut FrontendState,
        match_state: &mut MatchState,
        process_assets: &ProcessAssets,
        audio: &mut AppAudioRuntime,
        persistence: &PersistenceState,
    ) -> Option<Instant> {
        #[cfg(target_os = "android")]
        Self::drain_picked_part(
            &platform.window,
            renderer,
            frontend,
            process_assets,
            &persistence.options_profile,
        );
        let ShellMessageBoxPurpose::ModImport(dialog) =
            &frontend.shell_message_box.as_ref()?.purpose
        else {
            return None;
        };
        let Dialog::Preparing(preparation) = dialog.as_ref() else {
            let next = poll_scroll(&platform.window, renderer, frontend);
            // Android: the system file manager's copy lands with no event of
            // its own, so the box looks for it a few times a second.
            #[cfg(target_os = "android")]
            let next = next.or_else(|| Some(Instant::now() + Duration::from_millis(250)));
            return next;
        };
        let result = match preparation.result.try_recv() {
            Err(TryRecvError::Empty) => return Some(Instant::now() + Duration::from_millis(100)),
            Err(TryRecvError::Disconnected) => Err(anyhow::anyhow!("The import worker stopped")),
            Ok(result) => result,
        };
        let play = preparation.play;
        Self::close_shell_message_box_from_controller(frontend);
        // Android: the picked copies are installed or failed; either way they
        // are done with.
        #[cfg(target_os = "android")]
        crate::util::mod_actions::clear_picked();
        match result.and_then(|prepared| match prepared {
            Prepared::New(files) => Ok((files.install()?, None)),
            Prepared::Append { folder, files } => Ok((folder, Some(files.publish()?))),
        }) {
            Ok((folder, Some(count))) => {
                Self::select_installed_mod(platform.game_config.as_ref(), frontend, &folder);
                Self::mod_import_notice(
                    &platform.window,
                    renderer,
                    frontend,
                    process_assets,
                    &persistence.options_profile,
                    tr_fill(
                        "VERA:ModAppended",
                        &[("mod", &folder), ("count", &count.to_string())],
                    ),
                );
            }
            Ok((folder, None)) => Self::mod_imported(
                platform,
                renderer,
                diag,
                frontend,
                match_state,
                process_assets,
                audio,
                persistence,
                folder,
                play,
            ),
            Err(error) => Self::mod_import_failed(
                &platform.window,
                renderer,
                frontend,
                process_assets,
                &persistence.options_profile,
                error,
            ),
        }
        None
    }

    fn mod_imported(
        platform: &PlatformState,
        renderer: &RendererState,
        diag: &DiagnosticsState,
        frontend: &mut FrontendState,
        match_state: &mut MatchState,
        process_assets: &ProcessAssets,
        audio: &mut AppAudioRuntime,
        persistence: &PersistenceState,
        folder: String,
        play: bool,
    ) {
        // Android: the folder gets the id a pinned shortcut finds it by.
        #[cfg(target_os = "android")]
        if let Some(config) = platform.game_config.as_ref() {
            let _ = crate::app::mods::ensure_mod_id(&config.paths.mods_dir().join(&folder));
        }
        Self::select_installed_mod(platform.game_config.as_ref(), frontend, &folder);
        let selected = Self::mods_selected(&frontend.shell_controller);
        if play && frontend.mods.switch_to(selected).is_some() {
            Self::answer_mod_restart(
                platform,
                renderer,
                diag,
                frontend,
                match_state,
                process_assets,
                audio,
                persistence,
                Some(folder),
                true,
            );
            return;
        }
        let row = frontend
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
        Self::mod_import_notice(
            &platform.window,
            renderer,
            frontend,
            process_assets,
            &persistence.options_profile,
            title,
        );
    }

    fn mod_import_failed(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        error: anyhow::Error,
    ) {
        log::warn!("Mods: import failed: {error:#}");
        Self::mod_import_notice(
            window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            tr_fill("VERA:ModImportFailed", &[("error", &error)]),
        );
    }

    fn mod_import_notice(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        title: String,
    ) {
        let ok = process_assets.csf_label("TXT_OK", "OK");
        Self::show_import_box(
            window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            import_box(ShellBox::notice(title, ok), false),
            Dialog::Notice,
        );
    }
}
