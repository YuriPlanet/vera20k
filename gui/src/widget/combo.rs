//! `Combo`: gamemd's owner-draw combo box (`OwnerDraw_ComboBox @ 0x00617250`)
//! and its drop-down window (`ComboDropWin @ 0x0060D540`), on the shell
//! substrate in [`crate::shell::combobox`].
//!
//! Any press on an enabled face plays the open cue; only the arrow zone opens
//! the list ([`combo_arrow_hit_rect`]). The open list is the dialog's popup and
//! takes every pointer input: its scrollbar takes a press silently, a row or
//! anywhere else closes it with the close cue.
//!
//! Node data: `Rect` (the control rect; the face is its top
//! [`COMBO_FACE_H`](crate::shell::combobox::COMBO_FACE_H) px), `Items` or
//! `RowCount`, `Selected`, `VisibleRows` (0 = every row). The widget writes
//! `Open`, `Top` and, on a pick, `Selected`.

use super::{Capture, Ctx, WidgetState, instant, ui_ms};
use crate::gui::{Key, UiEvent};
use crate::node::{Node, NodeId, Prop, Tree, Value};
use crate::shell::combobox::{ComboPopup, combo_arrow_hit_rect, combo_face_rect};
use crate::shell::geom::ShellScale;
use crate::shell::list::{ListScrollInteraction, ListScrollPart};

#[derive(Debug, Default)]
pub struct ComboState {
    /// The list is open.
    open: bool,
    top_index: usize,
    /// 60E262: the row under the pointer, kept after it leaves the rows.
    hot_row: Option<usize>,
    scroll: ListScrollInteraction,
    /// The pointer as the open list last saw it: the arrow repeat steps the
    /// arrow under it.
    pointer: (i32, i32),
}

impl ComboState {
    /// 6181DA initializes top0; the current selection supplies the initial
    /// hot row, even when that row lies below the first visible page.
    fn open(&mut self, selected: Option<usize>) {
        self.open = true;
        self.top_index = 0;
        self.hot_row = selected;
        self.scroll.cancel();
    }

    fn close(&mut self) {
        self.open = false;
        self.scroll.cancel();
    }
}

/// An open drop-down as the host paints it: the same geometry the pointer
/// hits, the hot row and the scrollbar part shown pushed.
#[derive(Debug, Clone, Copy)]
pub struct ComboDropDown {
    pub popup: ComboPopup,
    /// 60E262: the row under the pointer, kept after it leaves the rows; the
    /// selection when the list opened.
    pub hot_row: Option<usize>,
    pub pressed_part: Option<ListScrollPart>,
    /// The scrollbar part a press holds until the release.
    pub captured_part: Option<ListScrollPart>,
}

/// The open drop-down of combo `node` on a dialog scaled by `scale`.
pub(crate) fn drop_down(tree: &Tree, node: NodeId, scale: ShellScale) -> Option<ComboDropDown> {
    let WidgetState::Combo(st) = &tree.node(node).ok()?.state else {
        return None;
    };
    if !st.open {
        return None;
    }
    Some(ComboDropDown {
        popup: popup_at(tree, node, scale, st.top_index)?,
        hot_row: st.hot_row,
        pressed_part: st.scroll.pressed_part(),
        captured_part: st.scroll.captured_part(),
    })
}

fn row_count(n: &Node) -> usize {
    match n.props.get(&Prop::Items) {
        Some(Value::Items(items)) => items.len(),
        _ => n.int_prop(Prop::RowCount, 0).max(0) as usize,
    }
}

fn selected(n: &Node) -> Option<usize> {
    usize::try_from(n.int_prop(Prop::Selected, -1)).ok()
}

/// The list hung under the face (`face.y + face.h + 1`), raised to end by
/// `PopupBottom` when set; none without rows.
fn popup_at(tree: &Tree, node: NodeId, scale: ShellScale, top: usize) -> Option<ComboPopup> {
    let n = tree.node(node).ok()?;
    let total = row_count(n);
    if total == 0 {
        return None;
    }
    let cap = n.int_prop(Prop::VisibleRows, 0).max(0) as usize;
    let mut face = combo_face_rect(tree.screen_rect(node).ok()?, scale);
    if cap > 0 && total > cap {
        face.w += n.int_prop(Prop::PopupWiden, 0);
    }
    let mut popup = ComboPopup::new(face, scale, total, cap, top);
    if let Some(Value::Int(row_h)) = n.props.get(&Prop::RowHeight) {
        popup = popup.with_row_height(*row_h);
    }
    if let Some(Value::Int(bottom)) = n.props.get(&Prop::PopupBottom) {
        popup.rect.y = popup.rect.y.min((bottom - popup.rect.h).max(0));
    }
    if let Some(Value::Int(limit)) = n.props.get(&Prop::PopupFlipBelow)
        && popup.rect.y + popup.rect.h > *limit
    {
        popup.rect.y = face.y - 1 - popup.rect.h;
    }
    if let Some(Value::Rect(rect)) = n.props.get(&Prop::PopupRect) {
        popup.rect = *rect;
    }
    Some(popup)
}

fn state_mut(tree: &mut Tree, node: NodeId) -> Option<&mut ComboState> {
    let n = tree.node_mut(node).ok()?;
    if !matches!(n.state, WidgetState::Combo(_)) {
        n.state = WidgetState::Combo(ComboState::default());
    }
    match &mut n.state {
        WidgetState::Combo(st) => Some(st),
        _ => None,
    }
}

/// Mirrors the substrate's first row into `Top`, reporting a scroll.
fn sync_top(ctx: &mut Ctx<'_>, node: NodeId, before: usize) {
    let Some(top) = state_mut(ctx.tree, node).map(|st| st.top_index) else {
        return;
    };
    let _ = ctx.tree.set(node, Prop::Top, Value::Int(top as i32));
    if top != before {
        ctx.events.push(UiEvent::Scrolled {
            node,
            top: top as i32,
        });
    }
}

/// Closes the list with the close cue (0x0060E4E9..0x0060E500).
fn close(ctx: &mut Ctx<'_>, node: NodeId) {
    if let Some(st) = state_mut(ctx.tree, node) {
        st.close();
    }
    *ctx.popup = None;
    let _ = ctx.tree.set(node, Prop::Open, Value::Bool(false));
    ctx.events.push(UiEvent::ComboClosed { node });
}

/// Lets go of the open list's held scrollbar part for the host, which lost
/// the pointer; the list stays open.
pub(crate) fn release_scroll(tree: &mut Tree, node: NodeId) {
    if let Some(st) = state_mut(tree, node) {
        st.scroll.cancel();
    }
}

/// Closes the list for the host: no cue, no event.
pub(crate) fn dismiss(tree: &mut Tree, node: NodeId) {
    if let Some(st) = state_mut(tree, node) {
        st.close();
    }
    let _ = tree.set(node, Prop::Open, Value::Bool(false));
}

/// Opens the list for the host at first row `top`, the selection lit: no
/// cue, no event.
pub(crate) fn open_for_host(tree: &mut Tree, node: NodeId, top: usize) {
    let selected = tree.node(node).ok().and_then(selected);
    if let Some(st) = state_mut(tree, node) {
        st.open(selected);
        st.top_index = top;
    }
    let _ = tree.set(node, Prop::Open, Value::Bool(true));
    let _ = tree.set(node, Prop::Top, Value::Int(top as i32));
}

pub(crate) fn press(ctx: &mut Ctx<'_>, node: NodeId, x: i32, y: i32) -> Capture {
    let (Ok(rect), Ok(n)) = (ctx.tree.screen_rect(node), ctx.tree.node(node)) else {
        return Capture::No;
    };
    let selected = selected(n);
    let anywhere = n.bool_prop(Prop::OpenAnywhere, false);
    let can_open = popup_at(ctx.tree, node, ctx.scale, 0).is_some();
    let scale = ctx.scale;
    let Some(st) = state_mut(ctx.tree, node) else {
        return Capture::No;
    };
    if can_open && (anywhere || combo_arrow_hit_rect(rect, scale).contains(x, y)) {
        st.open(selected);
        *ctx.popup = Some(node);
        let _ = ctx.tree.set(node, Prop::Open, Value::Bool(true));
        let _ = ctx.tree.set(node, Prop::Top, Value::Int(0));
        ctx.events.push(UiEvent::ComboOpened { node });
    } else {
        // 0x006184A2..0x006184BA: the open cue plays on any face press.
        ctx.events.push(UiEvent::Pressed { node });
    }
    Capture::No
}

/// The face never holds the pointer (the open list does), so nothing to do.
pub(crate) fn release(_ctx: &mut Ctx<'_>, _node: NodeId, _x: i32, _y: i32) {}

/// The open list for popup input, or none when the node or its rows went
/// away (the list then closes).
fn open_popup(ctx: &mut Ctx<'_>, node: NodeId) -> Option<ComboPopup> {
    let top = match &ctx.tree.node(node).ok()?.state {
        WidgetState::Combo(st) if st.open => Some(st.top_index),
        _ => None,
    };
    let popup = top.and_then(|top| popup_at(ctx.tree, node, ctx.scale, top));
    if popup.is_none() {
        close(ctx, node);
    }
    popup
}

/// A press while this combo's list is the dialog's popup (ComboDropWin
/// 0x0060E0C0): the scrollbar takes it silently; a row or anywhere else
/// closes the list, a row then selecting.
pub(crate) fn popup_press(ctx: &mut Ctx<'_>, node: NodeId, x: i32, y: i32) {
    let Some(popup) = open_popup(ctx, node) else {
        *ctx.popup = None;
        return;
    };
    let now = ctx.now;
    let Some(st) = state_mut(ctx.tree, node) else {
        return;
    };
    let before = st.top_index;
    st.pointer = (x, y);
    let now = instant(now);
    if let Some(part) = popup.scroll_part_at(x, y) {
        st.scroll.press(part, popup.scroll_geometry(), &mut st.top_index, y, now);
        sync_top(ctx, node, before);
        return;
    }
    let pick = popup.row_at(x, y);
    close_on(ctx, node, pick);
}

/// Closes the list with the close cue, then selects `pick`: CBN_SELCHANGE
/// on every pick, the current row too.
fn close_on(ctx: &mut Ctx<'_>, node: NodeId, pick: Option<usize>) {
    close(ctx, node);
    if let Some(row) = pick {
        let value = Value::Int(row as i32);
        let _ = ctx.tree.set(node, Prop::Selected, value.clone());
        ctx.events.push(UiEvent::Changed {
            node,
            value,
            final_: true,
        });
    }
}

/// A key on the open list, for a host whose list takes the keyboard: Up and
/// Down move the hot row (from the selection) within the rows, keeping it
/// shown; Enter picks it and Escape closes, both with the close cue.
pub(crate) fn key(ctx: &mut Ctx<'_>, node: NodeId, key: Key) {
    let Some(popup) = open_popup(ctx, node) else {
        *ctx.popup = None;
        return;
    };
    let selected = ctx.tree.node(node).ok().and_then(selected);
    let Some(st) = state_mut(ctx.tree, node) else {
        return;
    };
    let step = match key {
        Key::Up => -1,
        Key::Down => 1,
        Key::Enter => {
            let pick = st.hot_row.or(selected);
            return close_on(ctx, node, pick);
        }
        Key::Escape => return close(ctx, node),
        _ => return,
    };
    let last = popup.total.saturating_sub(1);
    let row = st.hot_row.or(selected).unwrap_or(0).saturating_add_signed(step).min(last);
    st.hot_row = Some(row);
    let before = st.top_index;
    st.top_index = st.top_index.min(row).max((row + 1).saturating_sub(popup.visible_rows));
    sync_top(ctx, node, before);
}

pub(crate) fn popup_move(ctx: &mut Ctx<'_>, node: NodeId, x: i32, y: i32) {
    let Some(popup) = open_popup(ctx, node) else {
        *ctx.popup = None;
        return;
    };
    let Some(st) = state_mut(ctx.tree, node) else {
        return;
    };
    let before = st.top_index;
    st.pointer = (x, y);
    popup.track_hot_row(&mut st.hot_row, x, y);
    st.scroll.pointer_moved(popup.scroll_geometry(), &mut st.top_index, x, y);
    sync_top(ctx, node, before);
}

/// The wheel over the open list: `rows` rows on, kept in
/// range, the hot row following the pointer.
pub(crate) fn wheel(ctx: &mut Ctx<'_>, node: NodeId, rows: i32, x: i32, y: i32) {
    let Some(mut popup) = open_popup(ctx, node) else {
        *ctx.popup = None;
        return;
    };
    let Some(st) = state_mut(ctx.tree, node) else {
        return;
    };
    let before = st.top_index;
    let max = popup.total - popup.visible_rows;
    popup.first = popup.first.saturating_add_signed(rows as isize).min(max);
    st.top_index = popup.first;
    popup.track_hot_row(&mut st.hot_row, x, y);
    sync_top(ctx, node, before);
}

/// The wheel on a `WheelPicks` combo's open list, wherever the pointer is
/// (the combo's own wheel step: CBN_SELCHANGE without the close cue): the
/// list closes with no event, and the row next to the selection (the
/// previous for `lines` > 0, the next for `lines` < 0), within the rows,
/// becomes selected, reported as `Changed` when it moved.
pub(crate) fn wheel_pick(ctx: &mut Ctx<'_>, node: NodeId, lines: i32) {
    dismiss(ctx.tree, node);
    *ctx.popup = None;
    let Ok(n) = ctx.tree.node(node) else { return };
    let last = row_count(n).saturating_sub(1);
    let Some(selected) = selected(n) else { return };
    let row = selected.saturating_add_signed(-(lines.signum() as isize)).min(last);
    if row == selected {
        return;
    }
    let value = Value::Int(row as i32);
    let _ = ctx.tree.set(node, Prop::Selected, value.clone());
    ctx.events.push(UiEvent::Changed { node, value, final_: true });
}

/// The open list's scroll-arrow repeat (`0x0061D215..0x0061D2C8`): the next
/// time it wants a poll.
pub(crate) fn poll(ctx: &mut Ctx<'_>, node: NodeId) -> Option<crate::gui::UiMs> {
    let popup = open_popup(ctx, node)?;
    let now = ctx.now;
    let st = state_mut(ctx.tree, node)?;
    let before = st.top_index;
    let (x, y) = st.pointer;
    st.scroll.poll(popup.scroll_geometry(), &mut st.top_index, x, y, instant(now));
    let next = st.scroll.repeat_at().map(ui_ms);
    sync_top(ctx, node, before);
    next
}

pub(crate) fn popup_release(ctx: &mut Ctx<'_>, node: NodeId, _x: i32, _y: i32) {
    release_scroll(ctx.tree, node);
}

#[cfg(test)]
mod tests {
    use crate::node::{NodeId, NodeKind, Prop, Value};
    use crate::{DialogKind, Gui, InputEvent, Key, MouseButton, Rect, UiEvent};

    /// A 120x24 combo at (10, 20) with `rows` items, `cap` shown at once.
    fn page_with_combo(rows: usize, cap: i32) -> (Gui, NodeId) {
        let mut g = Gui::new(8);
        let root = g.create(NodeKind::Dialog, None).unwrap();
        let c = g.create(NodeKind::Combo, Some(root)).unwrap();
        g.set(c, Prop::Rect, Value::Rect(Rect::new(10, 20, 120, 24)))
            .unwrap();
        let items = (0..rows).map(|i| format!("row {i}")).collect();
        g.set(c, Prop::Items, Value::Items(items)).unwrap();
        g.set(c, Prop::Selected, Value::Int(0)).unwrap();
        g.set(c, Prop::VisibleRows, Value::Int(cap)).unwrap();
        g.push_dialog(root, DialogKind::Page).unwrap();
        (g, c)
    }

    fn click(g: &mut Gui, x: i32, y: i32) -> Vec<UiEvent> {
        g.input(InputEvent::PointerMove { x, y }, 0);
        g.input(
            InputEvent::PointerDown {
                x,
                y,
                button: MouseButton::Left,
            },
            0,
        );
        g.input(
            InputEvent::PointerUp {
                x,
                y,
                button: MouseButton::Left,
            },
            0,
        );
        g.drain_events()
            .into_iter()
            .filter(|e| !matches!(e, UiEvent::Hovered { .. }))
            .collect()
    }

    #[test]
    fn keys_move_the_hot_row_within_the_rows_and_enter_picks_it() {
        let (mut g, c) = page_with_combo(3, 0);
        assert!(!g.key_popup(Key::Down, 0), "a closed list leaves the keys");
        click(&mut g, 129, 25);
        for _ in 0..3 {
            assert!(g.key_popup(Key::Down, 0));
        }
        assert_eq!(g.combo_drop_down(c).unwrap().hot_row, Some(2));
        g.key_popup(Key::Up, 0);
        g.drain_events();
        g.key_popup(Key::Enter, 0);
        assert_eq!(
            g.drain_events(),
            vec![
                UiEvent::ComboClosed { node: c },
                UiEvent::Changed { node: c, value: Value::Int(1), final_: true },
            ]
        );
        click(&mut g, 129, 25);
        g.key_popup(Key::Escape, 0);
        assert_eq!(g.drain_events(), vec![UiEvent::ComboClosed { node: c }]);
        assert_eq!(g.get(c, Prop::Selected).unwrap(), Some(&Value::Int(1)));
    }

    /// `WheelPicks`: a notch anywhere steps the selection within the rows
    /// and closes the list with no `ComboClosed`; at an end, nothing moves.
    #[test]
    fn a_wheel_picks_list_steps_the_selection_and_closes_silently() {
        let (mut g, c) = page_with_combo(3, 0);
        g.set(c, Prop::WheelPicks, Value::Bool(true)).unwrap();
        click(&mut g, 129, 25);
        g.input(InputEvent::PointerMove { x: 500, y: 500 }, 0);
        g.drain_events();
        assert!(g.input(InputEvent::Wheel { lines: -1 }, 0).consumed);
        assert_eq!(
            g.drain_events(),
            vec![UiEvent::Changed { node: c, value: Value::Int(1), final_: true }]
        );
        assert_eq!(g.popup(), None);
        click(&mut g, 129, 25);
        g.input(InputEvent::Wheel { lines: 1 }, 0);
        g.input(InputEvent::Wheel { lines: 0 }, 0);
        click(&mut g, 129, 25);
        g.drain_events();
        g.input(InputEvent::Wheel { lines: 2 }, 0);
        assert_eq!(g.drain_events(), vec![], "already the first row");
        assert_eq!((g.popup(), g.get(c, Prop::Selected).unwrap()), (None, Some(&Value::Int(0))));
    }

    #[test]
    fn a_dialog_losing_the_pointer_lets_go_of_its_lists_held_arrow() {
        let (mut g, c) = page_with_combo(5, 2);
        click(&mut g, 129, 25);
        let bar = g.combo_drop_down(c).unwrap().popup.scrollbar().unwrap();
        let (x, y) = (bar.x + 2, bar.y + bar.h - 2);
        g.input(InputEvent::PointerMove { x, y }, 0);
        g.input(InputEvent::PointerDown { x, y, button: MouseButton::Left }, 0);
        assert!(g.combo_drop_down(c).unwrap().captured_part.is_some());
        let dialog = g.dialogs().last().unwrap();
        g.drop_pointer_in(dialog);
        let list = g.combo_drop_down(c).expect("the list stays open");
        assert_eq!((list.captured_part, list.pressed_part), (None, None));
    }

    #[test]
    fn only_the_arrow_zone_opens_and_a_row_pick_closes_and_selects() {
        let (mut g, c) = page_with_combo(3, 0);
        // 0x006184CD: `x > right - 0x14` with `right` = 130 exclusive.
        assert_eq!(
            click(&mut g, 130 - 0x14, 25),
            vec![UiEvent::Pressed { node: c }]
        );
        assert_eq!(g.popup(), None);
        assert_eq!(
            click(&mut g, 130 - 0x13, 25),
            vec![UiEvent::ComboOpened { node: c }]
        );
        assert_eq!(g.popup(), Some(c));
        let row = g.combo_drop_down(c).unwrap().popup.row_rect(2).unwrap();
        assert_eq!(
            row.y,
            20 + 24 + 1 + 2 * 23,
            "rows hang one pixel under the face"
        );
        assert_eq!(
            click(&mut g, row.x + 1, row.y + 1),
            vec![
                UiEvent::ComboClosed { node: c },
                UiEvent::Changed {
                    node: c,
                    value: Value::Int(2),
                    final_: true
                },
            ]
        );
        assert_eq!(g.popup(), None);
        assert_eq!(g.get(c, Prop::Open).unwrap(), Some(&Value::Bool(false)));
        assert!(g.combo_drop_down(c).is_none());
    }

    #[test]
    fn open_list_takes_every_press_scrollbar_silently() {
        let (mut g, c) = page_with_combo(9, 4);
        click(&mut g, 125, 25);
        let dd = g.combo_drop_down(c).unwrap();
        let bar = dd
            .popup
            .scrollbar()
            .expect("nine rows over a cap of four scroll");
        // The down arrow steps one row and keeps the list open, no cue.
        let events = click(&mut g, bar.x + 2, bar.y + bar.h - 2);
        assert_eq!(events, vec![UiEvent::Scrolled { node: c, top: 1 }]);
        assert_eq!(g.get(c, Prop::Top).unwrap(), Some(&Value::Int(1)));
        // The pick of the already-selected row still reports it
        // (CBN_SELCHANGE runs the owner's handler again).
        g.set(c, Prop::Selected, Value::Int(1)).unwrap();
        let row = g.combo_drop_down(c).unwrap().popup.row_rect(1).unwrap();
        assert_eq!(
            click(&mut g, row.x + 1, row.y + 1),
            vec![
                UiEvent::ComboClosed { node: c },
                UiEvent::Changed { node: c, value: Value::Int(1), final_: true },
            ]
        );
        // A press on the face while open dismisses and goes no further.
        click(&mut g, 125, 25);
        assert_eq!(
            click(&mut g, 125, 25),
            vec![UiEvent::ComboClosed { node: c }]
        );
        assert_eq!(g.popup(), None);
    }

    #[test]
    fn open_list_leaves_hover_to_the_screen_beside_it_and_the_host_closes_it_silently() {
        let (mut g, c) = page_with_combo(3, 0);
        let root = g.dialog_root(g.top_dialog().unwrap()).unwrap();
        let b = g.create(NodeKind::Button, Some(root)).unwrap();
        g.set(b, Prop::Rect, Value::Rect(Rect::new(200, 0, 20, 20)))
            .unwrap();
        click(&mut g, 125, 25);
        g.input(InputEvent::PointerMove { x: 205, y: 5 }, 0);
        assert_eq!(g.hovered(), Some(b));
        let row = g.combo_drop_down(c).unwrap().popup.row_rect(1).unwrap();
        g.input(InputEvent::PointerMove { x: row.x + 1, y: row.y + 1 }, 0);
        assert_eq!(g.hovered(), None, "the list covers what is under it");
        g.drain_events();
        assert!(g.close_popup());
        assert!(g.drain_events().is_empty());
        assert_eq!(g.popup(), None);
        assert_eq!(g.get(c, Prop::Open).unwrap(), Some(&Value::Bool(false)));
        assert!(!g.close_popup());
    }

    #[test]
    fn open_anywhere_opens_from_the_face_and_popup_bottom_raises_the_list() {
        let (mut g, c) = page_with_combo(3, 0);
        g.set(c, Prop::OpenAnywhere, Value::Bool(true)).unwrap();
        g.set(c, Prop::PopupBottom, Value::Int(100)).unwrap();
        assert_eq!(click(&mut g, 12, 25), vec![UiEvent::ComboOpened { node: c }]);
        let list = g.combo_drop_down(c).unwrap().popup.rect;
        assert_eq!((list.y, list.h), (100 - 3 * 23, 3 * 23), "raised to end at 100");
        // Removing the combo forgets its open list.
        g.remove(c).unwrap();
        assert_eq!(g.popup(), None);
    }

    #[test]
    fn hot_row_starts_at_the_selection_and_survives_leaving_the_rows() {
        let (mut g, c) = page_with_combo(3, 0);
        g.set(c, Prop::Selected, Value::Int(1)).unwrap();
        click(&mut g, 125, 25);
        assert_eq!(g.combo_drop_down(c).unwrap().hot_row, Some(1));
        let row = g.combo_drop_down(c).unwrap().popup.row_rect(2).unwrap();
        g.input(
            InputEvent::PointerMove {
                x: row.x + 1,
                y: row.y + 1,
            },
            0,
        );
        assert_eq!(g.combo_drop_down(c).unwrap().hot_row, Some(2));
        g.input(InputEvent::PointerMove { x: 500, y: 500 }, 0);
        assert_eq!(g.combo_drop_down(c).unwrap().hot_row, Some(2));
    }
}
