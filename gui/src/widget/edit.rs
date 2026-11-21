//! `Edit` behaviour: the native single-line NewEdit (window proc
//! `0x00614B30`, [`crate::shell::new_edit::NewEdit`]) on the node's props.
//!
//! `Prop::Text` is the text and `Prop::Caret` the caret (in chars; unset is
//! the end, where a programmatic assignment leaves it). NewEdit keeps a caret
//! but no selection and places no caret on a press, so a press only takes the
//! focus; with `SelectOnFocus` every press selects the whole text
//! (`AllSelected`), the only selection there is. Typing runs
//! NewEdit's per-unit filter (control units, `ByteOnly`, `Rejected`,
//! `MaxLen`) plus `Allowed` and `MaxBytes`; Backspace/Delete/Left/Right/
//! Home/End edit; Enter submits and Escape cancels, both left for the
//! dialog's default and cancel buttons too (§8.5; a `Multiline` edit first
//! appends CRLF). The gui keeps the caret in view with the host's text
//! widths ([`keep_caret_in_view`]); the host keeps the paint and
//! screen-specific rules (it answers `TextChanged`).

use super::{Capture, Ctx, WidgetState};
use crate::gui::{Key, UiEvent, UiMs};
use crate::node::{Node, NodeId, Prop, Tree, Value};
use crate::shell::geom::ShellScale;
use crate::shell::new_edit::{self, NewEdit};

#[derive(Debug, Default)]
pub struct EditState {
    /// When the edit last took the focus: the caret blink's phase.
    pub focused_at: UiMs,
}

impl EditState {
    /// NewEdit's caret blink: shown in the first of each two seconds.
    pub fn caret_visible(&self, now: UiMs) -> bool {
        new_edit::caret_visible(self.focused_at, now)
    }
}

pub(crate) fn press(ctx: &mut Ctx<'_>, node: NodeId, _x: i32, _y: i32) -> Capture {
    let Ok(n) = ctx.tree.node_mut(node) else { return Capture::No };
    if *ctx.focus != Some(node) {
        *ctx.focus = Some(node);
        n.state = WidgetState::Edit(EditState { focused_at: ctx.now });
    }
    // A `SelectOnFocus` box selects the whole text on every press.
    if n.bool_prop(Prop::SelectOnFocus, false) {
        let end = n.text_prop(Prop::Text).unwrap_or("").chars().count();
        n.props.insert(Prop::Caret, Value::Int(i32::try_from(end).unwrap_or(i32::MAX)));
        n.props.insert(Prop::AllSelected, Value::Bool(end > 0));
    }
    Capture::No
}

pub(crate) fn drag(_ctx: &mut Ctx<'_>, _node: NodeId, _x: i32, _y: i32) {}

pub(crate) fn release(_ctx: &mut Ctx<'_>, _node: NodeId, _x: i32, _y: i32) {}

/// A key while the edit has the focus; true when consumed.
pub(crate) fn key(ctx: &mut Ctx<'_>, node: NodeId, key: Key) -> bool {
    let Some(mut edit) = load(ctx, node) else { return false };
    // A selection goes with the first edit or caret move.
    let caret_key = !matches!(key, Key::Enter | Key::Escape);
    if caret_key && take_selection(ctx, node) && matches!(key, Key::Backspace | Key::Delete) {
        edit.units.clear();
        edit.caret = 0;
        store(ctx, node, &edit);
        return true;
    }
    match key {
        Key::Enter => {
            // NewEdit's multiline Enter (`0x00614DEB`) appends CRLF and
            // notifies 0x501; its owner clicks the default button only for
            // a text that is not empty once trimmed.
            let multiline = ctx.tree.node(node).is_ok_and(|n| n.bool_prop(Prop::Multiline, false));
            if multiline {
                edit.append_enter_line_end();
                store(ctx, node, &edit);
            }
            let text = edit.display_text();
            ctx.events.push(UiEvent::Submitted { node, text });
            return multiline && edit.units.iter().all(|unit| matches!(unit, 10 | 13));
        }
        Key::Escape => {
            ctx.events.push(UiEvent::Cancelled { node });
            return false;
        }
        Key::Backspace => edit.backspace(),
        Key::Delete => edit.delete(),
        Key::Left => edit.left(),
        Key::Right => edit.right(),
        Key::Home => edit.home(),
        Key::End => edit.end(),
        _ => return false,
    }
    store(ctx, node, &edit);
    true
}

/// Typed text while the edit has the focus (IME commits included).
pub(crate) fn text(ctx: &mut Ctx<'_>, node: NodeId, text: &str) {
    let Some(mut edit) = load(ctx, node) else { return };
    // Typed text replaces the selection.
    if text.chars().any(|c| !c.is_control()) && take_selection(ctx, node) {
        edit.units.clear();
        edit.caret = 0;
    }
    let Ok(n) = ctx.tree.node(node) else { return };
    let limit = usize::try_from(n.int_prop(Prop::MaxLen, 0)).unwrap_or(0);
    let max_bytes = usize::try_from(n.int_prop(Prop::MaxBytes, 0)).unwrap_or(0);
    let byte_only = n.bool_prop(Prop::ByteOnly, false);
    let rejected: Vec<u16> = n.text_prop(Prop::Rejected).unwrap_or("").encode_utf16().collect();
    let allowed: Option<Vec<u16>> = n.text_prop(Prop::Allowed).map(|a| a.encode_utf16().collect());
    for ch in text.chars() {
        let mut units = [0; 2];
        let units = ch.encode_utf16(&mut units);
        if allowed.as_ref().is_some_and(|a| units.iter().any(|u| !a.contains(u))) {
            continue;
        }
        if max_bytes != 0 && (ctx.byte_len)(&edit.display_text()) + (ctx.byte_len)(ch.encode_utf8(&mut [0; 4])) > max_bytes {
            continue;
        }
        for unit in units.iter() {
            edit.insert_unit_filtered(*unit, limit, byte_only, &rejected);
        }
    }
    store(ctx, node, &edit);
}

/// Drops the whole-text selection; whether there was one.
fn take_selection(ctx: &mut Ctx<'_>, node: NodeId) -> bool {
    let Ok(n) = ctx.tree.node_mut(node) else { return false };
    n.props.remove(&Prop::AllSelected) == Some(Value::Bool(true))
}

/// An `Edit` node's props as a NewEdit: `Text`, `Caret` (chars; unset is
/// the end) as UTF-16 units, `Top` as the first shown unit, the `Password`
/// mask. Not focused.
pub(crate) fn read(n: &Node) -> NewEdit {
    let text = n.text_prop(Prop::Text).unwrap_or("");
    let chars = text.chars().count();
    let caret_chars = match n.props.get(&Prop::Caret) {
        Some(Value::Int(c)) => usize::try_from(*c).unwrap_or(0).min(chars),
        _ => chars,
    };
    let units: Vec<u16> = text.encode_utf16().collect();
    NewEdit {
        caret: text.chars().take(caret_chars).map(char::len_utf16).sum(),
        first_visible_unit: usize::try_from(n.int_prop(Prop::Top, 0)).unwrap_or(0).min(units.len()),
        units,
        password: n.bool_prop(Prop::Password, false),
        ..NewEdit::default()
    }
}

/// Writes a NewEdit's text, caret (as chars) and first shown unit to its
/// node's props, without an event.
pub(crate) fn write(tree: &mut Tree, node: NodeId, edit: &NewEdit) {
    let caret = String::from_utf16_lossy(&edit.units[..edit.caret.min(edit.units.len())]).chars().count();
    let _ = tree.set(node, Prop::Text, Value::Text(edit.display_text()));
    let _ = tree.set(node, Prop::Caret, Value::Int(i32::try_from(caret).unwrap_or(i32::MAX)));
    let _ = tree.set(node, Prop::Top, Value::Int(i32::try_from(edit.first_visible_unit).unwrap_or(i32::MAX)));
}

/// The base px NewEdit shows its text in, less the caret's: the rect at
/// `scale` less `push_new_edit`'s 3, or for a framed edit (`TextInset`)
/// less the inset and 2.
fn field_width(n: &Node, scale: ShellScale) -> u32 {
    let less = match n.props.get(&Prop::TextInset) {
        Some(Value::Rect(inset)) => inset.x + inset.w + 2,
        _ => 3,
    };
    u32::try_from((scale.base_rect(n.rect()).w - less).max(0)).unwrap_or(0)
}

/// NewEdit's horizontal scroll (`scroll_to_caret`, as
/// `NewEdit__DrawTextAndCaret` `0x00623880` paints from the first visible
/// unit): `Top` becomes the first unit from which the text up to the caret
/// fits the field, `measure` giving text widths in base px.
pub(crate) fn keep_caret_in_view(tree: &mut Tree, node: NodeId, scale: ShellScale, measure: &dyn Fn(&str) -> u32) {
    let Ok(n) = tree.node(node) else { return };
    let mut edit = read(n);
    edit.scroll_to_caret(field_width(n, scale), measure);
    let _ = tree.set(node, Prop::Top, Value::Int(i32::try_from(edit.first_visible_unit).unwrap_or(i32::MAX)));
}

/// The node's text and caret as the focused NewEdit typing works on (the
/// view's first shown unit is the host's).
fn load(ctx: &Ctx<'_>, node: NodeId) -> Option<NewEdit> {
    let n = ctx.tree.node(node).ok()?;
    Some(NewEdit { focused: true, first_visible_unit: 0, ..read(n) })
}

/// Writes the edit back; `TextChanged` when the text changed.
fn store(ctx: &mut Ctx<'_>, node: NodeId, edit: &NewEdit) {
    let text = edit.display_text();
    let caret = String::from_utf16_lossy(&edit.units[..edit.caret]).chars().count();
    let changed = ctx
        .tree
        .node(node)
        .is_ok_and(|n| n.text_prop(Prop::Text).unwrap_or("") != text);
    let _ = ctx.tree.set(node, Prop::Caret, Value::Int(i32::try_from(caret).unwrap_or(i32::MAX)));
    if changed {
        let _ = ctx.tree.set(node, Prop::Text, Value::Text(text.clone()));
        ctx.events.push(UiEvent::TextChanged { node, text });
    }
}
