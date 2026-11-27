//! What changed in an offered release, listed under the offer's question.
//!
//! The publisher signs the release page's change list into the manifest
//! (`notes`). The offer shows it in the shell's owner-draw list (`0x00618D40`,
//! paint `0x00619230`) with its scrollbar (`0x0061C690`), the list Skirmish
//! and Westwood Online use, left of the button column. gamemd's message box
//! `0x121` has no list: its placement and caption are VERA's own.

use crate::app::frontend::state::ShellMessageBoxPurpose;
use crate::render::bit_font::BitFont;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::{RectPx, SHELL_BASE_H, SHELL_BASE_W};
use crate::ui::shell::modal::{BoxParts, control, menu_area, message_box_parts};
use crate::util::time::Instant;
use vera20k_gui::shell::list::{ListMetrics, ListScrollPart, ShellListGeometry};
use vera20k_gui::{NodeId, NodeKind, Prop, Value};

/// The label of the list's first row.
const CAPTION: &str = "VERA:WhatsNew";
/// Rows the list shows at once: from the lowest button's bottom up to below
/// the question.
const VISIBLE_ROWS: i32 = 8;
/// The gap between the list and the buttons, in 800x600 pixels.
const BUTTON_GAP: i32 = 10;
/// The rows' text inset, as Sound Options' list draws its rows.
const TEXT_INSET: i32 = 2;
const BULLET: &str = "\u{2022} ";

/// The change list's rows while the offer is open. Its top and scrollbar
/// are a `List` node's in the offer box's dialog ([`sync`]).
#[derive(Debug)]
pub(crate) struct ReleaseNotes {
    rows: Vec<Row>,
}

#[derive(Debug)]
struct Row {
    text: String,
    /// In 800x600 pixels: a change's further rows hang under its text.
    indent: i32,
}

impl ReleaseNotes {
    /// A caption, then `notes` wrapped to the list's width, a bullet before
    /// each change.
    pub(crate) fn new(font: &BitFont, notes: &[String]) -> Self {
        let indent = font.text_width(BULLET);
        let width = wrap_width().saturating_sub(indent);
        let mut rows = Vec::new();
        for note in notes {
            // GAME.FNT has no arrow; release pages write "Internet → Custom Match".
            let note = note.replace('\u{2192}', "->");
            for (index, text) in font.wrap_layout(&note, width).line_texts(&note).enumerate() {
                rows.push(match index {
                    0 => Row {
                        text: format!("{BULLET}{text}"),
                        indent: 0,
                    },
                    _ => Row {
                        text: text.to_owned(),
                        indent: indent as i32,
                    },
                });
            }
        }
        if !rows.is_empty() {
            let caption = Row {
                text: crate::i18n::tr(CAPTION),
                indent: 0,
            };
            rows.insert(0, caption);
        }
        Self { rows }
    }

    /// The list in the offer box's `layout` from row `top`; `None` without
    /// notes.
    pub(crate) fn geometry(&self, layout: &BoxParts, top: usize) -> Option<ShellListGeometry> {
        if self.rows.is_empty() {
            return None;
        }
        let metrics = ListMetrics::scaled(layout.scale);
        Some(ShellListGeometry::new(
            list_rect(layout, metrics),
            self.rows.len(),
            top,
            metrics,
        ))
    }

    /// Row `row`'s text and where it starts, in 800x600 pixels into the
    /// row: a change's further rows hang under its text. The text runs to
    /// the list's right edge: it was wrapped narrower, so a scaled rect's
    /// rounding never wraps it again ([`crate::app::frontend::skirmish_shell_render::gui_paint::GuiStyle::indent_of`]).
    pub(crate) fn row(&self, row: usize) -> Option<(&str, i32)> {
        let row = self.rows.get(row)?;
        Some((row.text.as_str(), TEXT_INSET + row.indent))
    }

    /// The rows' text as the list's `Items`, written once as it is built:
    /// the notes never change while the offer is open.
    fn items(&self) -> Vec<String> {
        self.rows.iter().map(|row| row.text.clone()).collect()
    }
}

/// Under the question, from its left edge to just short of the buttons, its
/// bottom level with the lowest button's.
fn list_rect(layout: &BoxParts, metrics: ListMetrics) -> RectPx {
    let lowest = layout.rect_of(control::CANCEL).unwrap_or(layout.button(0));
    let gap = (BUTTON_GAP as f32 * layout.scale.factor()).round() as i32;
    let h = VISIBLE_ROWS * metrics.row_h + 2;
    RectPx::new(
        layout.body.x,
        lowest.y + lowest.h - h,
        layout.button(0).x - gap - layout.body.x,
        h,
    )
}

/// The rows' text width in 800x600 pixels, beside a scrollbar.
fn wrap_width() -> u32 {
    let layout = message_box_parts(3, menu_area(SHELL_BASE_W, SHELL_BASE_H));
    let metrics = ListMetrics::RETAIL;
    (list_rect(&layout, metrics).w - 2 - metrics.scrollbar_w - TEXT_INSET).max(0) as u32
}

/// The open offer's change list, the box's layout and its dialog.
fn open_list<'a>(
    gpu: &crate::render::gpu::GpuContext,
    frontend: &'a crate::app::frontend::state::FrontendState,
) -> Option<(&'a ReleaseNotes, BoxParts, DialogResourceId)> {
    let message_box = frontend.shell_message_box.as_ref()?;
    let shell_box = &message_box.shell_box;
    let layout = crate::app::frontend::shell_layout::shell_box_parts(gpu, shell_box);
    match &message_box.purpose {
        ShellMessageBoxPurpose::UpdateOffer(notes) if !notes.rows.is_empty() => {
            Some((notes, layout, shell_box.dialog))
        }
        _ => None,
    }
}

/// The offer list's host id: past the box's 16-bit control ids, so the
/// box's rect sync leaves its window alone and no press names it (the movie
/// list's is `0x1_0000`, the Add Mod parts list's `0x1_0002`).
pub(crate) const LIST_ID: i32 = 0x1_0001;

/// The open offer's `List` node and its notes, if built.
pub(crate) fn node<'a>(
    gpu: &crate::render::gpu::GpuContext,
    frontend: &'a crate::app::frontend::state::FrontendState,
) -> Option<(NodeId, &'a ReleaseNotes)> {
    let (notes, _, dialog) = open_list(gpu, frontend)?;
    Some((list_node(&frontend.shell_controller, dialog)?, notes))
}

/// The offer box's `List` node, if built.
fn list_node(
    controller: &crate::ui::shell::controller::DialogController,
    dialog: DialogResourceId,
) -> Option<NodeId> {
    let gui = controller.gui();
    let children = gui.tree().children(controller.root_in(dialog)?).ok()?;
    children
        .iter()
        .copied()
        .find(|node| gui.tree().kind(*node) == Ok(NodeKind::List))
}

/// The open offer's list as it paints: its geometry, top and the scrollbar
/// arrow drawn pushed.
pub(crate) fn view(
    gpu: &crate::render::gpu::GpuContext,
    frontend: &crate::app::frontend::state::FrontendState,
) -> Option<(ShellListGeometry, usize, Option<ListScrollPart>)> {
    let (notes, layout, dialog) = open_list(gpu, frontend)?;
    let view = list_node(&frontend.shell_controller, dialog)
        .and_then(|node| frontend.shell_controller.gui().list_view(node));
    let top = view.map_or(0, |view| view.top);
    Some((
        notes.geometry(&layout, top)?,
        top,
        view.and_then(|view| view.pressed),
    ))
}

/// Builds the open offer's list in its box once, after the box's buttons
/// (its id counts as built), and keeps it on its rect; only its scrollbar
/// takes the pointer. The box's widgets take the press and release.
pub(crate) fn sync(
    gpu: &crate::render::gpu::GpuContext,
    frontend: &mut crate::app::frontend::state::FrontendState,
) -> Option<NodeId> {
    let (geometry, _, _) = view(gpu, frontend)?;
    let (notes, layout, dialog) = open_list(gpu, frontend)?;
    let scale = layout.scale;
    let node = list_node(&frontend.shell_controller, dialog);
    // The rows' text goes into the list once, when it is built.
    let items = node.is_none().then(|| notes.items());
    let shell_box = &frontend.shell_message_box.as_ref()?.shell_box;
    let controller = &mut frontend.shell_controller;
    // The list's metrics follow the box's scale, as its paint does.
    controller.set_scale_in(dialog, scale);
    let root = controller.root_in(dialog)?;
    let gui = controller.gui_mut();
    let node = match node {
        Some(node) => node,
        None => {
            crate::ui::shell::page::ensure_built(gui, root, || shell_box.nodes());
            let node = gui.create(NodeKind::List, Some(root)).ok()?;
            let _ = gui.set(node, Prop::Id, Value::Int(LIST_ID));
            // The rows draw top-left, as `texts` placed them.
            let _ = gui.set(node, Prop::Align, Value::Int(0));
            node
        }
    };
    if let Some(items) = items {
        let _ = gui.set(node, Prop::Items, Value::Items(items));
    }
    let _ = gui.set(node, Prop::Rect, Value::Rect(geometry.outer));
    let _ = gui.set(
        node,
        Prop::HitRect,
        Value::Rect(geometry.scrollbar.unwrap_or_default()),
    );
    Some(node)
}

/// The cursor drags a captured thumb and moves on or off a held arrow.
pub(crate) fn cursor_moved(
    gpu: &crate::render::gpu::GpuContext,
    frontend: &mut crate::app::frontend::state::FrontendState,
    input: &crate::app::input::state::MatchInputState,
) {
    let Some(node) = sync(gpu, frontend) else {
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
}

/// Three rows a wheel notch, as the other shell lists scroll. The open offer
/// takes every wheel turn.
pub(crate) fn wheel(
    gpu: &crate::render::gpu::GpuContext,
    frontend: &mut crate::app::frontend::state::FrontendState,
    lines: f32,
) -> bool {
    let Some(node) = sync(gpu, frontend) else {
        return false;
    };
    let controller = &mut frontend.shell_controller;
    let now = controller.now();
    let gui = controller.gui_mut();
    let _ = gui.set(node, Prop::WheelStep, Value::Int(1));
    gui.wheel_list(node, (lines * 3.0) as i32, now);
    gui.drain_events();
    true
}

/// A held arrow's repeat; the deadline of the next one.
pub(super) fn poll_scroll(
    platform: &crate::app::PlatformState,
    gpu: &crate::render::gpu::GpuContext,
    frontend: &mut crate::app::frontend::state::FrontendState,
) -> Option<Instant> {
    sync(gpu, frontend)?;
    let window = &platform.window;
    frontend
        .shell_controller
        .poll_then(|| window.request_redraw())
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    #[test]
    fn a_long_change_wraps_under_its_bullet_in_the_list_beside_the_buttons() {
        // Every glyph 5 pixels wide plus spacing; GAME.FNT's widths vary.
        let font = crate::render::bit_font::tests::make_test_font(&[], 4);
        let note = "Play online through Internet \u{2192} Custom Match: enter a username, \
                    host or join a game, and add AI opponents.";
        let notes = ReleaseNotes::new(&font, &[note.to_owned()]);
        let indent = font.text_width(BULLET) as i32;
        assert!(notes.rows.len() > 3);
        assert_eq!(notes.rows[0].text, crate::i18n::tr(CAPTION));
        assert!(notes.rows[1].text.starts_with("\u{2022} Play online"));
        assert!(notes.rows[2..].iter().all(|row| row.indent == indent));
        assert!(
            notes
                .rows
                .iter()
                .all(|row| row.indent as u32 + font.text_width(&row.text) <= wrap_width())
        );
        let words: Vec<&str> = notes.rows[1..]
            .iter()
            .map(|row| row.text.trim_start_matches(BULLET))
            .collect();
        assert_eq!(words.join(" "), note.replace('\u{2192}', "->"));
        // In the 451x326 box at 800x600: eight 19-pixel rows and the frame,
        // left of the buttons and level with the lowest.
        let layout = message_box_parts(3, menu_area(800, 600));
        let list = notes.geometry(&layout, 0).unwrap().outer;
        let (dialog, cancel) = (layout.dialog, layout.rect_of(control::CANCEL).unwrap());
        assert_eq!(
            (list.x - dialog.x, list.y - dialog.y, list.w, list.h),
            (60, 155, 241, 154)
        );
        assert_eq!(list.y + list.h, cancel.y + cancel.h);
        assert!(list.x + list.w < layout.button(0).x);
    }
}
