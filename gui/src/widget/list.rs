//! `List` behaviour: the shell's owner-draw list box (subclass `0x0061A948`
//! with its scrollbar child `0x0061C690`), driven through the shared list
//! substrate in `shell::list` exactly as the Movies and Mods pages drive it.
//!
//! The node's props hold what the host sees: `Rect` (the list's paint
//! surface, frame and scrollbar included), `Items` or `RowCount`, `Selected`
//! (`-1` or absent = none) and `Top`. The scrollbar capture and the
//! double-click timing stay in the node's `WidgetState::List`; the limits are
//! the GUI's ([`crate::Gui::set_double_click`]).

use super::{Capture, Ctx, WidgetState, instant, ui_ms};
use crate::Rect;
use crate::gui::{UiEvent, UiMs};
use crate::node::{Node, NodeId, Prop, Tree, Value};
use crate::shell::list::{
    ListMetrics, ListScrollInteraction, ListScrollPart, ShellListGeometry, is_double_click,
};
use crate::time::{Duration, Instant};

/// One owner-draw list box (the movie list `0x129`, VERA's Mods list). Its
/// row count, top and selection are reloaded from the props before every
/// input and written back after it.
#[derive(Debug, Default)]
pub struct ListState {
    rows: usize,
    /// Subclass current selection. Its LB_SETCURSEL handler
    /// (`0x0061A534..0x0061A5F5`) always returns 0, so a fresh process opens
    /// with nothing selected.
    selected: Option<usize>,
    top: usize,
    /// Previous press for the ListBox class's `CS_DBLCLKS` detection.
    last_press: Option<(Instant, i32, i32)>,
    /// Scrollbar capture and arrow repeat (`0x0061C690`).
    scroll: ListScrollInteraction,
    /// Rows its owner shows at once, when it lays them out itself
    /// ([`ShellListGeometry::with_visible_rows`]); `None` fits the window.
    visible_rows: Option<usize>,
    /// The pointer as the list last saw it: the arrow repeat steps the arrow
    /// under it (`0x0061D215..0x0061D2C8`).
    pointer: (i32, i32),
}

impl ListState {
    /// List and scrollbar geometry on the paint surface `list`.
    fn geometry(&self, list: Rect, metrics: ListMetrics) -> ShellListGeometry {
        match self.visible_rows {
            Some(rows) => ShellListGeometry::with_visible_rows(list, self.rows, rows, self.top, metrics),
            None => ShellListGeometry::new(list, self.rows, self.top, metrics),
        }
    }
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
    /// The list's geometry on its paint surface `list` (its `Rect`).
    pub fn geometry(&self, list: Rect, metrics: ListMetrics) -> ShellListGeometry {
        ShellListGeometry::new(list, self.rows, self.top, metrics)
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
        WidgetState::List(state) => state.scroll.pressed_part(),
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
    let mut metrics = ListMetrics::scaled(ctx.scale);
    let n = ctx.tree.node_mut(node).ok()?;
    if let Some(Value::Int(row_h)) = n.props.get(&Prop::RowHeight) {
        metrics.row_h = (*row_h).max(1);
    }
    let count = row_count(n);
    let top = n.int_prop(Prop::Top, 0).max(0) as usize;
    let selected = usize::try_from(n.int_prop(Prop::Selected, -1)).ok();
    let mut state = match std::mem::take(&mut n.state) {
        WidgetState::List(state) => state,
        _ => ListState::default(),
    };
    state.rows = count;
    state.top = top;
    state.selected = selected;
    state.visible_rows = match n.props.get(&Prop::VisibleRows) {
        Some(Value::Int(rows)) => usize::try_from(*rows).ok(),
        _ => None,
    };
    let result = f(&mut state, rect, metrics, ctx.events);
    let (new_top, new_selected) = (state.top, state.selected);
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

/// Drops the list's scrollbar hold.
pub(crate) fn cancel_scroll(tree: &mut Tree, node: NodeId) {
    if let Ok(n) = tree.node_mut(node)
        && let WidgetState::List(state) = &mut n.state
    {
        state.scroll.cancel();
    }
}

/// A press on another node ends the list's double-click sequence.
pub(crate) fn forget_press(tree: &mut Tree, node: NodeId) {
    if let Ok(n) = tree.node_mut(node)
        && let WidgetState::List(state) = &mut n.state
    {
        state.last_press = None;
    }
}

pub(crate) fn press(ctx: &mut Ctx<'_>, node: NodeId, x: i32, y: i32) -> Capture {
    let now = instant(ctx.now);
    let (ms, w, h) = ctx.double_click;
    let limits = (Duration::from_millis(ms), w, h);
    with_list(ctx, node, |s, rect, metrics, events| {
        s.pointer = (x, y);
        // The scrollbar child (0x0061C690) captures its own presses and
        // plays no sound: arrow step, track jump or thumb capture.
        let geometry = s.geometry(rect, metrics);
        if let Some(part) = geometry.scroll_part_at(x, y) {
            s.scroll.press(part, geometry, &mut s.top, y, now);
            return Capture::Yes;
        }
        // CS_DBLCLKS: the second press only forwards LBN_DBLCLK
        // (0x0061A904..0x0061A945), wherever it landed; the host opens the
        // current selection.
        if is_double_click(&mut s.last_press, now, x, y, limits) {
            if let Some(row) = s.selected {
                events.push(UiEvent::ListDoubleClicked { node, row: row as u32 });
            }
            return Capture::No;
        }
        // 0x0061A948: a press selects the row under it (the host plays
        // GenericClick); presses below the last row are ignored.
        if let Some(row) = geometry.row_at(s.rows, s.top, x, y) {
            s.selected = Some(row);
            events.push(UiEvent::ListSelected { node, row: row as u32 });
        }
        Capture::No
    })
    .unwrap_or(Capture::No)
}

/// A move while the scrollbar holds the pointer: the thumb follows it.
pub(crate) fn drag(ctx: &mut Ctx<'_>, node: NodeId, x: i32, y: i32) {
    with_list(ctx, node, |s, rect, metrics, _| {
        s.pointer = (x, y);
        let geometry = s.geometry(rect, metrics);
        s.scroll.pointer_moved(geometry, &mut s.top, x, y);
    });
}

/// The release ends the scrollbar's capture and its repeat (0x0061D2D3).
pub(crate) fn release(ctx: &mut Ctx<'_>, node: NodeId, _x: i32, _y: i32) {
    with_list(ctx, node, |s, _, _, _| s.scroll.cancel());
}

/// The wheel over the list; true when it scrolled. Three rows a notch, as
/// the Mods and Keyboard pages scroll.
pub(crate) fn wheel(ctx: &mut Ctx<'_>, node: NodeId, lines: i32) -> bool {
    with_list(ctx, node, |s, rect, metrics, _| {
        let max_top = s.geometry(rect, metrics).max_top as i64;
        let top = (s.top as i64 - i64::from(lines) * 3).clamp(0, max_top) as usize;
        let scrolled = top != s.top;
        s.top = top;
        scrolled
    })
    .unwrap_or(false)
}

/// Timers (scroll-arrow repeat): the next time the list wants a poll.
pub(crate) fn poll(ctx: &mut Ctx<'_>, node: NodeId) -> Option<UiMs> {
    let now = instant(ctx.now);
    with_list(ctx, node, |s, rect, metrics, _| {
        let (x, y) = s.pointer;
        if s.scroll.repeat_at().is_some() {
            let geometry = s.geometry(rect, metrics);
            s.scroll.poll(geometry, &mut s.top, x, y, now);
        }
        s.scroll.repeat_at().map(ui_ms)
    })
    .flatten()
}

#[cfg(test)]
mod tests {
    use crate::Rect;
    use crate::gui::{DialogKind, Gui, InputEvent, MouseButton, UiEvent};
    use crate::node::{NodeId, NodeKind, Prop, Value};

    /// A retail 400x255 list at (10, 20): 13 rows show; with 60 rows the
    /// scrollbar sits at x 389..409 and the top ranges over 0..=47.
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
        // Below the last row: no selection change, but a second press there
        // still opens the selection.
        assert_eq!(click(&mut g, 50, 20 + 19 * 5, 2000), vec![]);
        assert_eq!(
            click(&mut g, 51, 20 + 19 * 5, 2100),
            vec![UiEvent::ListDoubleClicked { node: list, row: 1 }]
        );
        assert_eq!(g.get(list, Prop::Selected).unwrap(), Some(&Value::Int(1)));
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
        // Losing the pointer lets go of a hold as the release does.
        g.input(InputEvent::PointerDown { x, y, button: MouseButton::Left }, 700);
        g.cancel_pointer();
        assert_eq!((g.poll(1300), g.list_view(list).unwrap().pressed), (None, None));
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
