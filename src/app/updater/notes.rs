//! What changed in an offered release, listed under the offer's question.
//!
//! The publisher signs the release page's change list into the manifest
//! (`notes`). The offer shows it in the shell's owner-draw list (`0x00618D40`,
//! paint `0x00619230`) with its scrollbar (`0x0061C690`), the list Skirmish
//! and Westwood Online use, left of the button column. gamemd's message box
//! `0x121` has no list: its placement and caption are VERA's own.

use crate::app::frontend::state::ShellMessageBoxPurpose;
use crate::app::state::AppState;
use crate::render::bit_font::BitFont;
use crate::ui::shell::geom::{RectPx, SHELL_BASE_H, SHELL_BASE_W};
use crate::ui::shell::list::{
    ListMetrics, ListScrollInteraction, ListScrollPart, ShellListGeometry,
};
use crate::ui::shell::modal::{MessageBoxLayout, ModalKind};
use crate::util::time::Instant;

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

/// The change list's rows and scroll position while the offer is open.
#[derive(Debug)]
pub(crate) struct ReleaseNotes {
    rows: Vec<Row>,
    top: usize,
    scroll: ListScrollInteraction,
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
            for (index, line) in font.wrap_layout(&note, width).lines.iter().enumerate() {
                let text = &note[line.start_byte..line.end_byte];
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
        Self {
            rows,
            top: 0,
            scroll: ListScrollInteraction::default(),
        }
    }

    /// The list in the offer box's `layout`; `None` without notes.
    pub(crate) fn geometry(&self, layout: &MessageBoxLayout) -> Option<ShellListGeometry> {
        if self.rows.is_empty() {
            return None;
        }
        let metrics = ListMetrics::scaled(layout.scale);
        Some(ShellListGeometry::new(
            list_rect(layout, metrics),
            self.rows.len(),
            self.top,
            metrics,
        ))
    }

    pub(crate) fn top(&self) -> usize {
        self.top
    }

    pub(crate) fn pressed_part(&self) -> Option<ListScrollPart> {
        self.scroll.pressed_part()
    }

    /// Each visible row's text with the rect it is drawn in. The rect runs to
    /// the list's right edge: the text was wrapped narrower, so a scaled
    /// rect's rounding never wraps it again.
    pub(crate) fn texts(&self, geometry: &ShellListGeometry, scale: f32) -> Vec<(&str, RectPx)> {
        let right = geometry.outer.x + geometry.outer.w - 1;
        self.rows
            .iter()
            .skip(self.top)
            .take(geometry.visible_rows)
            .enumerate()
            .map(|(visible, row)| {
                let rect = geometry.row(visible);
                let x = rect.x + ((TEXT_INSET + row.indent) as f32 * scale).round() as i32;
                (row.text.as_str(), RectPx::new(x, rect.y, right - x, rect.h))
            })
            .collect()
    }
}

/// Under the question, from its left edge to just short of the buttons, its
/// bottom level with the lowest button's.
fn list_rect(layout: &MessageBoxLayout, metrics: ListMetrics) -> RectPx {
    let lowest = layout.cancel.unwrap_or(layout.ok);
    let gap = (BUTTON_GAP as f32 * layout.scale.factor()).round() as i32;
    let h = VISIBLE_ROWS * metrics.row_h + 2;
    RectPx::new(
        layout.body.x,
        lowest.y + lowest.h - h,
        layout.ok.x - gap - layout.body.x,
        h,
    )
}

/// The rows' text width in 800x600 pixels, beside a scrollbar.
fn wrap_width() -> u32 {
    let layout = crate::ui::shell::modal::message_box_layout(
        ModalKind::ThreeButton,
        SHELL_BASE_W,
        SHELL_BASE_H,
    );
    let metrics = ListMetrics::RETAIL;
    (list_rect(&layout, metrics).w - 2 - metrics.scrollbar_w - TEXT_INSET).max(0) as u32
}

/// The open offer's change list and the box's layout.
fn open_list(state: &mut AppState) -> Option<(&mut ReleaseNotes, MessageBoxLayout)> {
    let kind = state.frontend.shell_message_box.as_ref()?.labels.kind();
    let layout = crate::app::frontend::shell_layout::shell_message_box_layout(state, kind);
    match &mut state.frontend.shell_message_box.as_mut()?.purpose {
        ShellMessageBoxPurpose::UpdateOffer(notes) if !notes.rows.is_empty() => {
            Some((notes, layout))
        }
        _ => None,
    }
}

/// A left press on the scrollbar scrolls the list and is not the buttons';
/// any release ends the scrollbar's capture.
pub(crate) fn pointer_button(state: &mut AppState, pressed: bool) -> bool {
    let (x, y) = state.match_state.input.cursor_px();
    let Some((notes, layout)) = open_list(state) else {
        return false;
    };
    if !pressed {
        notes.scroll.cancel();
        return false;
    }
    let geometry = notes.geometry(&layout).expect("the list has rows");
    let Some(part) = geometry.scroll_part_at(x, y) else {
        return false;
    };
    notes
        .scroll
        .press(part, geometry, &mut notes.top, y, Instant::now());
    state.platform.window.request_redraw();
    true
}

/// The cursor drags a captured thumb and moves on or off a held arrow.
pub(crate) fn cursor_moved(state: &mut AppState) {
    let (x, y) = state.match_state.input.cursor_px();
    let Some((notes, layout)) = open_list(state) else {
        return;
    };
    let before = (notes.top, notes.pressed_part());
    let geometry = notes.geometry(&layout).expect("the list has rows");
    notes.scroll.pointer_moved(geometry, &mut notes.top, x, y);
    if (notes.top, notes.pressed_part()) != before {
        state.platform.window.request_redraw();
    }
}

/// Three rows a wheel notch, as the other shell lists scroll. The open offer
/// takes every wheel turn.
pub(crate) fn wheel(state: &mut AppState, lines: f32) -> bool {
    let Some((notes, layout)) = open_list(state) else {
        return false;
    };
    let max_top = notes.geometry(&layout).expect("the list has rows").max_top;
    let top = (notes.top as i32 - (lines * 3.0) as i32).clamp(0, max_top as i32) as usize;
    if top != notes.top {
        notes.top = top;
        state.platform.window.request_redraw();
    }
    true
}

/// A held arrow's repeat; the deadline of the next one.
pub(super) fn poll_scroll(state: &mut AppState) -> Option<Instant> {
    let (x, y) = state.match_state.input.cursor_px();
    let (notes, layout) = open_list(state)?;
    let geometry = notes.geometry(&layout).expect("the list has rows");
    let changed = notes
        .scroll
        .poll(geometry, &mut notes.top, x, y, Instant::now());
    let next = notes.scroll.repeat_at();
    if changed {
        state.platform.window.request_redraw();
    }
    next
}

#[cfg(test)]
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
        let layout = crate::ui::shell::modal::message_box_layout(ModalKind::ThreeButton, 800, 600);
        let list = notes.geometry(&layout).unwrap().outer;
        let (dialog, cancel) = (layout.dialog, layout.cancel.unwrap());
        assert_eq!(
            (list.x - dialog.x, list.y - dialog.y, list.w, list.h),
            (60, 155, 241, 154)
        );
        assert_eq!(list.y + list.h, cancel.y + cancel.h);
        assert!(list.x + list.w < layout.ok.x);
    }
}
