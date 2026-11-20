//! `Edit` behaviour: the native single-line NewEdit (window proc
//! `0x00614B30`, [`crate::shell::new_edit::NewEdit`]) on the node's props.
//!
//! `Prop::Text` is the text and `Prop::Caret` the caret (in chars; unset is
//! the end, where a programmatic assignment leaves it). NewEdit keeps a caret
//! but no selection and places no caret on a press, so a press only takes the
//! focus. Typing runs NewEdit's per-unit filter (control units, `ByteOnly`,
//! `Rejected`, `MaxLen`) plus `Allowed`; Backspace/Delete/Left/Right/Home/End
//! edit; Enter submits and Escape cancels, both left for the dialog's
//! default and cancel buttons too (§8.5). The host keeps the paint, the
//! horizontal scroll and screen-specific rules (it answers `TextChanged`).

use super::{Capture, Ctx, WidgetState};
use crate::gui::{Key, UiEvent, UiMs};
use crate::node::{NodeId, Prop, Value};
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
    if *ctx.focus != Some(node) {
        *ctx.focus = Some(node);
        if let Ok(n) = ctx.tree.node_mut(node) {
            n.state = WidgetState::Edit(EditState { focused_at: ctx.now });
        }
    }
    Capture::No
}

pub(crate) fn drag(_ctx: &mut Ctx<'_>, _node: NodeId, _x: i32, _y: i32) {}

pub(crate) fn release(_ctx: &mut Ctx<'_>, _node: NodeId, _x: i32, _y: i32) {}

/// A key while the edit has the focus; true when consumed.
pub(crate) fn key(ctx: &mut Ctx<'_>, node: NodeId, key: Key) -> bool {
    let Some(mut edit) = load(ctx, node) else { return false };
    match key {
        Key::Enter => {
            let text = edit.display_text();
            ctx.events.push(UiEvent::Submitted { node, text });
            return false;
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
    let Ok(n) = ctx.tree.node(node) else { return };
    let limit = usize::try_from(n.int_prop(Prop::MaxLen, 0)).unwrap_or(0);
    let byte_only = n.bool_prop(Prop::ByteOnly, false);
    let rejected: Vec<u16> = n.text_prop(Prop::Rejected).unwrap_or("").encode_utf16().collect();
    let allowed: Option<Vec<u16>> = n.text_prop(Prop::Allowed).map(|a| a.encode_utf16().collect());
    for unit in text.encode_utf16() {
        if allowed.as_ref().is_some_and(|a| !a.contains(&unit)) {
            continue;
        }
        edit.insert_unit_filtered(unit, limit, byte_only, &rejected);
    }
    store(ctx, node, &edit);
}

/// The node's text and caret as a NewEdit (caret in UTF-16 units).
fn load(ctx: &Ctx<'_>, node: NodeId) -> Option<NewEdit> {
    let n = ctx.tree.node(node).ok()?;
    let text = n.text_prop(Prop::Text).unwrap_or("");
    let chars = text.chars().count();
    let caret_chars = match n.props.get(&Prop::Caret) {
        Some(Value::Int(c)) => usize::try_from(*c).unwrap_or(0).min(chars),
        _ => chars,
    };
    let caret = text.chars().take(caret_chars).map(char::len_utf16).sum();
    Some(NewEdit {
        units: text.encode_utf16().collect(),
        caret,
        password: n.bool_prop(Prop::Password, false),
        focused: true,
        ..NewEdit::default()
    })
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
