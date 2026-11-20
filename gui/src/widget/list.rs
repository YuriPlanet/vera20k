//! `List` behaviour: the shell's owner-draw list box (subclass `0x0061A948`
//! with its scrollbar child `0x0061C690`), driven through the shared list
//! substrate in `shell::list` exactly as the Movies and Mods pages drive it.
//!
//! The node's props hold what the host sees: `Rect` (the list window, the
//! scrollbar included), `Items` or `RowCount`, `Selected` (`-1` or absent =
//! none), `Top` and `DoubleClick` (default on: the second press of a
//! double-click only reports it; off, every press selects, as VERA's Mods
//! page does). The scrollbar capture and the double-click timing stay in the
//! node's `WidgetState::List`; the limits are the GUI's
//! ([`crate::Gui::set_double_click`]).

use std::sync::OnceLock;

use super::{Capture, Ctx, WidgetState};
use crate::Rect;
use crate::gui::{UiEvent, UiMs};
use crate::node::{Node, NodeId, Prop, Tree, Value};
use crate::shell::list::{
    ListBoxState, ListMetrics, ListScrollPart, ShellListGeometry, list_box_geometry,
};
use crate::time::{Duration, Instant};

#[derive(Debug)]
pub struct ListState {
    /// The shell list box. Its row count, top and selection are reloaded
    /// from the props before every input and written back after it.
    list: ListBoxState<()>,
    /// The pointer as the list last saw it: the arrow repeat steps the arrow
    /// under it (`0x0061D215..0x0061D2C8`).
    pointer: (i32, i32),
}

impl Default for ListState {
    fn default() -> Self {
        Self {
            list: ListBoxState::new(Vec::new(), None),
            pointer: (0, 0),
        }
    }
}

/// The substrate keeps its timers as `Instant`s; the GUI clock is `UiMs`.
/// One process-wide epoch maps between them.
fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

fn instant(ms: UiMs) -> Instant {
    epoch() + Duration::from_millis(ms)
}

fn ui_ms(at: Instant) -> UiMs {
    at.saturating_duration_since(epoch()).as_millis() as UiMs
}

/// What the host paints of a list: its rows, top, selection and the
/// scrollbar arrow drawn pushed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListView {
    pub rows: usize,
    pub top: usize,
    pub selected: Option<usize>,
    pub pressed: Option<ListScrollPart>,
}

impl ListView {
    /// The list box geometry in window `list` ([`list_box_geometry`]).
    pub fn geometry(&self, list: Rect, metrics: ListMetrics) -> ShellListGeometry {
        list_box_geometry(list, self.rows, self.top, metrics)
    }
}

fn row_count(n: &Node) -> usize {
    match n.props.get(&Prop::Items) {
        Some(Value::Items(items)) => items.len(),
        _ => n.int_prop(Prop::RowCount, 0).max(0) as usize,
    }
}

pub(crate) fn view(tree: &Tree, node: NodeId) -> Option<ListView> {
    let n = tree.node(node).ok()?;
    let pressed = match &n.state {
        WidgetState::List(state) => state.list.scroll.pressed_part(),
        _ => None,
    };
    Some(ListView {
        rows: row_count(n),
        top: n.int_prop(Prop::Top, 0).max(0) as usize,
        selected: usize::try_from(n.int_prop(Prop::Selected, -1)).ok(),
        pressed,
    })
}

/// Runs `f` on the node's list box with its props loaded, then writes the
/// top and selection back; a moved top is the user's scroll (`Scrolled`).
fn with_list<R>(
    ctx: &mut Ctx<'_>,
    node: NodeId,
    f: impl FnOnce(&mut ListState, Rect, ListMetrics, &mut Vec<UiEvent>) -> R,
) -> Option<R> {
    let rect = ctx.tree.screen_rect(node).ok()?;
    let metrics = ListMetrics::scaled(ctx.scale);
    let n = ctx.tree.node_mut(node).ok()?;
    let count = row_count(n);
    let top = n.int_prop(Prop::Top, 0).max(0) as usize;
    let selected = usize::try_from(n.int_prop(Prop::Selected, -1)).ok();
    let mut state = match std::mem::take(&mut n.state) {
        WidgetState::List(state) => state,
        _ => ListState::default(),
    };
    state.list.rows.resize(count, ());
    state.list.top = top;
    state.list.selected = selected;
    let result = f(&mut state, rect, metrics, ctx.events);
    let (new_top, new_selected) = (state.list.top, state.list.selected);
    let n = ctx.tree.node_mut(node).ok()?;
    n.state = WidgetState::List(state);
    if new_selected != selected {
        let row = new_selected.map_or(-1, |row| row as i32);
        n.props.insert(Prop::Selected, Value::Int(row));
    }
    if new_top != top {
        n.props.insert(Prop::Top, Value::Int(new_top as i32));
        ctx.events.push(UiEvent::Scrolled {
            node,
            top: new_top as i32,
        });
    }
    Some(result)
}

pub(crate) fn press(ctx: &mut Ctx<'_>, node: NodeId, x: i32, y: i32) -> Capture {
    let now = instant(ctx.now);
    let (ms, w, h) = ctx.double_click;
    let limits = (Duration::from_millis(ms), w, h);
    let double_click = ctx
        .tree
        .node(node)
        .is_ok_and(|n| n.bool_prop(Prop::DoubleClick, true));
    with_list(ctx, node, |s, rect, metrics, events| {
        s.pointer = (x, y);
        // The scrollbar child (0x0061C690) captures its own presses and
        // plays no sound: arrow step, track jump or thumb capture.
        if s.list.scroll_press(rect, metrics, x, y, now) {
            return Capture::Yes;
        }
        // CS_DBLCLKS: the second press only forwards LBN_DBLCLK
        // (0x0061A904..0x0061A945), wherever it landed; the host opens the
        // current selection.
        if double_click && s.list.is_double_click(now, x, y, limits) {
            if let Some(row) = s.list.selected {
                events.push(UiEvent::ListDoubleClicked {
                    node,
                    row: row as u32,
                });
            }
            return Capture::No;
        }
        // 0x0061A948: a press selects the row under it (the host plays
        // GenericClick); presses below the last row are ignored.
        if let Some(row) = s.list.row_at(rect, metrics, x, y) {
            s.list.selected = Some(row);
            events.push(UiEvent::ListSelected {
                node,
                row: row as u32,
            });
        }
        Capture::No
    })
    .unwrap_or(Capture::No)
}

/// A move while the scrollbar holds the pointer: the thumb follows it.
pub(crate) fn drag(ctx: &mut Ctx<'_>, node: NodeId, x: i32, y: i32) {
    with_list(ctx, node, |s, rect, metrics, _| {
        s.pointer = (x, y);
        s.list.scroll_pointer_moved(rect, metrics, x, y);
    });
}

/// The release ends the scrollbar's capture and its repeat (0x0061D2D3).
pub(crate) fn release(ctx: &mut Ctx<'_>, node: NodeId, _x: i32, _y: i32) {
    with_list(ctx, node, |s, _, _, _| s.list.scroll.cancel());
}

/// The wheel over the list; true when it scrolled. Three rows a notch, as
/// the Mods and Keyboard pages scroll.
pub(crate) fn wheel(ctx: &mut Ctx<'_>, node: NodeId, lines: i32) -> bool {
    with_list(ctx, node, |s, rect, metrics, _| {
        let max_top = s.list.geometry(rect, metrics).max_top as i64;
        let top = (s.list.top as i64 - i64::from(lines) * 3).clamp(0, max_top) as usize;
        let scrolled = top != s.list.top;
        s.list.top = top;
        scrolled
    })
    .unwrap_or(false)
}

/// Timers (scroll-arrow repeat): the next time the list wants a poll.
pub(crate) fn poll(ctx: &mut Ctx<'_>, node: NodeId) -> Option<UiMs> {
    let now = instant(ctx.now);
    with_list(ctx, node, |s, rect, metrics, _| {
        let (x, y) = s.pointer;
        s.list.scroll_poll(rect, metrics, x, y, now);
        s.list.scroll.repeat_at().map(ui_ms)
    })
    .flatten()
}

#[cfg(test)]
mod tests {
    use crate::Rect;
    use crate::gui::{DialogKind, Gui, InputEvent, MouseButton, UiEvent};
    use crate::node::{NodeId, NodeKind, Prop, Value};

    /// A retail 400x255 list at (10, 20): 13 rows show; with 60 rows the
    /// scrollbar sits at x 390..410 and the top ranges over 0..=47.
    fn list_page(rows: Value) -> (Gui, NodeId) {
        let mut g = Gui::new(8);
        let root = g.create(NodeKind::Dialog, None).unwrap();
        let list = g.create(NodeKind::List, Some(root)).unwrap();
        g.set(list, Prop::Rect, Value::Rect(Rect::new(10, 20, 400, 255)))
            .unwrap();
        let prop = if matches!(rows, Value::Int(_)) {
            Prop::RowCount
        } else {
            Prop::Items
        };
        g.set(list, prop, rows).unwrap();
        g.push_dialog(root, DialogKind::Page, None, None).unwrap();
        (g, list)
    }

    fn click(g: &mut Gui, x: i32, y: i32, now: u64) -> Vec<UiEvent> {
        g.input(InputEvent::PointerMove { x, y }, now);
        g.input(
            InputEvent::PointerDown {
                x,
                y,
                button: MouseButton::Left,
            },
            now,
        );
        g.input(
            InputEvent::PointerUp {
                x,
                y,
                button: MouseButton::Left,
            },
            now,
        );
        g.drain_events()
            .into_iter()
            .filter(|e| !matches!(e, UiEvent::Hovered { .. }))
            .collect()
    }

    #[test]
    fn second_press_in_time_opens_the_selection_and_rows_end_at_the_last() {
        let (mut g, list) = list_page(Value::Int(3));
        let row1 = 20 + 19 + 5;
        assert_eq!(
            click(&mut g, 50, row1, 0),
            vec![UiEvent::ListSelected { node: list, row: 1 }]
        );
        assert_eq!(
            click(&mut g, 52, row1 + 2, 300),
            vec![UiEvent::ListDoubleClicked { node: list, row: 1 }]
        );
        // The double-click ended the sequence: the next press selects again.
        assert_eq!(
            click(&mut g, 50, row1, 400),
            vec![UiEvent::ListSelected { node: list, row: 1 }]
        );
        // Below the last row: no selection change.
        assert_eq!(click(&mut g, 50, 20 + 19 * 5, 2000), vec![]);
        assert_eq!(g.get(list, Prop::Selected).unwrap(), Some(&Value::Int(1)));
    }

    #[test]
    fn with_double_click_off_every_press_selects() {
        let (mut g, list) = list_page(Value::Int(3));
        g.set(list, Prop::DoubleClick, Value::Bool(false)).unwrap();
        let row1 = 20 + 19 + 5;
        for now in [0, 300] {
            assert_eq!(
                click(&mut g, 50, row1, now),
                vec![UiEvent::ListSelected { node: list, row: 1 }]
            );
        }
    }

    #[test]
    fn held_down_arrow_captures_silently_then_repeats_after_500ms_until_release() {
        let (mut g, list) = list_page(Value::Items(vec![String::new(); 60]));
        let (x, y) = (400, 270);
        g.input(InputEvent::PointerMove { x, y }, 0);
        g.input(
            InputEvent::PointerDown {
                x,
                y,
                button: MouseButton::Left,
            },
            0,
        );
        assert_eq!(g.pressed(), Some(list));
        let events: Vec<_> = g
            .drain_events()
            .into_iter()
            .filter(|e| !matches!(e, UiEvent::Hovered { .. }))
            .collect();
        assert_eq!(events, vec![UiEvent::Scrolled { node: list, top: 1 }]);
        assert_eq!(g.poll(499), Some(500));
        assert_eq!(g.get(list, Prop::Top).unwrap(), Some(&Value::Int(1)));
        assert_eq!(g.poll(500), Some(525));
        assert_eq!(g.get(list, Prop::Top).unwrap(), Some(&Value::Int(2)));
        g.input(
            InputEvent::PointerUp {
                x,
                y,
                button: MouseButton::Left,
            },
            510,
        );
        assert_eq!(g.poll(600), None);
        assert_eq!(g.get(list, Prop::Top).unwrap(), Some(&Value::Int(2)));
    }

    #[test]
    fn wheel_scrolls_three_rows_a_notch_within_the_range() {
        let (mut g, list) = list_page(Value::Items(vec![String::new(); 60]));
        g.input(InputEvent::PointerMove { x: 50, y: 50 }, 0);
        g.drain_events();
        assert!(
            !g.input(InputEvent::Wheel { lines: 1 }, 0).consumed,
            "already at the top"
        );
        assert!(g.input(InputEvent::Wheel { lines: -1 }, 0).consumed);
        assert_eq!(
            g.drain_events(),
            vec![UiEvent::Scrolled { node: list, top: 3 }]
        );
        g.input(InputEvent::Wheel { lines: -100 }, 0);
        assert_eq!(g.get(list, Prop::Top).unwrap(), Some(&Value::Int(47)));
    }
}
