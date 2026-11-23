//! The connection verify `0x130` ("Connecting to other players"): the
//! compact popup Start Game Now (`0x00794BA0`) pushes over C2/C9 after
//! OnGameStart, with proc `0x00793280`: one auto-checkbox per session node
//! shows which peers answered, and the negotiation's mismatch box `0xD0` may
//! open over it. The native eight rows have no scrollbar; past them VERA's
//! extended rosters scroll the boxes with a list's. The boxes, that list and
//! the box's OK are `vera20k-gui` widgets in the dialog's own tree.

use vera20k_gui::{NodeKind, NodeSpec, Prop, Value};

use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::modal::ShellBox;
use crate::ui::shell::modal::WolVerifyLayout;
use crate::ui::shell::page::{self, PageEvent};
use crate::ui::wol_shell::BOX_OK;
use crate::util::time::Instant;
use std::ops::Range;
use vera20k_gui::shell::list::{ListMetrics, ListScrollPart, ShellListGeometry};

/// VERA's room shows `0x130` only once the connection check has taken this
/// long, so a quick check does not flash a box before the loading screen.
pub const MODERN_SHOW_DELAY: std::time::Duration = std::time::Duration::from_secs(1);

/// `0x0084A1F0`: node `i`'s checkbox, stacked by `i` in the template.
pub const BOXES: [u16; 8] = [0x769, 0x761, 0x764, 0x762, 0x765, 0x768, 0x766, 0x767];

const DIALOG: DialogResourceId = DialogResourceId(0x0130);
/// The list whose scrollbar moves the boxes past eight nodes.
const LIST: u16 = 0x0200;
/// Node `node`'s box widget (by node, not by template id: past eight nodes
/// the template has none).
const fn node_box(node: usize) -> u16 {
    0x0100 + node as u16
}

#[derive(Debug)]
pub struct WolVerifyDialog {
    /// The node names in `[0xA8DA78]` order (`0x497`'s `0x4B2` captions);
    /// native boxes past them are destroyed; the VERA viewport retains every node.
    pub names: Vec<String>,
    /// `0xB75F54[i]`: node `i` answered.
    connected: Vec<bool>,
    /// `0xD0` with `TXT_SCENARIOS_DO_NOT_MATCH` over the dialog
    /// (`0x00794D28..0x00794D57`).
    pub mismatch: Option<ShellBox>,
    ui: DialogController,
    opened: Instant,
}

impl WolVerifyDialog {
    /// `0x497` (`0x00793362..0x0079341B`): the connected bytes clear and
    /// every remaining box is unchecked.
    pub fn open(mut names: Vec<String>) -> Self {
        names.truncate(crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT);
        let count = names.len();
        let mut ui = DialogController::default();
        ui.push(DIALOG);
        let boxes =
            (0..count).map(|node| NodeSpec::new(NodeKind::Checkbox).id(i32::from(node_box(node))));
        let list = NodeSpec::new(NodeKind::List)
            .id(i32::from(LIST))
            .with(Prop::RowCount, Value::Int(count as i32));
        let ok = NodeSpec::new(NodeKind::Button).id(i32::from(BOX_OK));
        // The first node under the pointer takes it: the boxes over the list.
        let spec = NodeSpec::new(NodeKind::Dialog)
            .children(boxes)
            .child(ok)
            .child(list);
        if let Some(root) = ui.root_in(DIALOG) {
            page::ensure_built(ui.gui_mut(), root, || spec);
        }
        Self {
            names,
            connected: vec![false; count],
            mismatch: None,
            ui,
            opened: Instant::now(),
        }
    }

    /// Whether the dialog paints at `now`: always in the native dialogs,
    /// and in VERA's room (`modern`) after [`MODERN_SHOW_DELAY`] or with
    /// its mismatch box up.
    pub fn shown(&self, modern: bool, now: Instant) -> bool {
        !modern
            || self.mismatch.is_some()
            || now.saturating_duration_since(self.opened) >= MODERN_SHOW_DELAY
    }

    fn list(&self) -> Option<vera20k_gui::ListView> {
        self.ui.gui().list_view(self.ui.control(DIALOG, LIST)?)
    }

    /// The eight resource rows of Verify793280 remain exact for ordinary
    /// matches; VERA's extended roster scrolls them as a list of box rows.
    pub fn list_geometry(&self, layout: &WolVerifyLayout) -> Option<ShellListGeometry> {
        if self.names.len() <= BOXES.len() {
            return None;
        }
        let first = layout.boxes[0];
        let last = layout.boxes[BOXES.len() - 1];
        // Template checkbox windows are slightly taller than their pitch.
        // Derive a uniform pitch from the scaled row starts, not the check
        // window height. The shared list uses a device-pixel inset per edge.
        let gaps = (BOXES.len() - 1) as i32;
        let (row_h, inner_h) = if layout.scale.is_unscaled() {
            // Preserve the native-size extended viewport as it was.
            (first.h, last.y + last.h - first.y)
        } else {
            let pitch = ((last.y - first.y + gaps / 2) / gaps).max(1);
            (pitch, pitch * BOXES.len() as i32)
        };
        let outer = RectPx::new(first.x - 1, first.y - 1, first.w + 2, inner_h + 2);
        let metrics = ListMetrics {
            row_h,
            ..ListMetrics::scaled(layout.scale)
        };
        let top = self.list().map_or(0, |view| view.top);
        Some(ShellListGeometry::new(
            outer,
            self.names.len(),
            top,
            metrics,
        ))
    }

    pub fn visible_nodes(&self, layout: &WolVerifyLayout) -> Range<usize> {
        let Some(geometry) = self.list_geometry(layout) else {
            return 0..self.names.len();
        };
        let top = self.list().map_or(0, |view| view.top).min(geometry.max_top);
        top..(top + geometry.visible_rows).min(self.names.len())
    }

    pub fn node_rect(&self, layout: &WolVerifyLayout, node: usize) -> Option<RectPx> {
        let visible = self.visible_nodes(layout);
        if !visible.contains(&node) {
            return None;
        }
        Some(match self.list_geometry(layout) {
            Some(geometry) => geometry.row(node - visible.start),
            None => layout.boxes[node],
        })
    }

    /// The scrollbar arrow drawn pushed.
    pub fn pressed_scroll_part(&self) -> Option<ListScrollPart> {
        self.list()?.pressed
    }

    /// Whether the mismatch box's OK shows pushed (captured and under the
    /// pointer, 612B70).
    pub fn ok_pressed(&self) -> bool {
        let gui = self.ui.gui();
        let root = self.ui.root_in(DIALOG);
        root.and_then(|root| page::held_in(gui, root))
            .is_some_and(|node| gui.id_of(node) == Some(i32::from(BOX_OK)))
    }

    /// Puts the widgets on `layout`: the box's OK on `ok` while the
    /// mismatch box is up, else the visible boxes and the list.
    fn sync(&mut self, layout: &WolVerifyLayout, ok: RectPx) {
        let Some(root) = self.ui.root_in(DIALOG) else {
            return;
        };
        self.ui.set_scale(layout.scale);
        let boxed = self.mismatch.is_some();
        let list = self.list_geometry(layout);
        if let Some(geometry) = list {
            let row_h = Value::Int(geometry.metrics.row_h);
            page::set(self.ui.gui_mut(), root, LIST, Prop::RowHeight, row_h);
        }
        let boxes: Vec<_> = (0..self.names.len())
            .map(|node| self.node_rect(layout, node))
            .collect();
        let gui = self.ui.gui_mut();
        page::sync_rects(gui, root, |id| match id {
            BOX_OK => boxed.then_some(ok),
            _ if boxed => None,
            LIST => list.map(|geometry| geometry.outer),
            id => boxes
                .get(usize::from(id.wrapping_sub(node_box(0))))
                .copied()
                .flatten(),
        });
    }

    /// One pointer message at (`x`, `y`) (`down`: `None` moves): the cues
    /// of what it hit (a list press's GenericClick the dialog never plays)
    /// and the mismatch box's OK once clicked. A box's icon toggles on the
    /// press (`0x006163A0`), then [`Self::click`] forces it.
    pub fn pointer(
        &mut self,
        layout: &WolVerifyLayout,
        ok: RectPx,
        (x, y): (i32, i32),
        down: Option<bool>,
    ) -> Vec<PageEvent> {
        self.sync(layout, ok);
        match down {
            None => self.ui.on_pointer_move(x, y),
            Some(true) => self.ui.on_pointer_down(x, y),
            Some(false) => self.ui.on_pointer_up(x, y),
        }
        let events = self.ui.drain_events();
        let events = page::page_events(self.ui.gui(), events);
        for event in &events {
            if let PageEvent::Toggled(id, _) = event {
                self.click(usize::from(id.wrapping_sub(node_box(0))));
            }
        }
        self.sync(layout, ok);
        events
    }

    /// The scrollbar's arrow repeat: whether the boxes moved, and the next
    /// deadline.
    pub fn poll_scroll(&mut self) -> (bool, Option<Instant>) {
        let next = self.ui.poll();
        (!self.ui.drain_events().is_empty(), next)
    }

    /// The boxes' check state, as their widgets hold it.
    pub fn checked(&self) -> Vec<bool> {
        (0..self.names.len())
            .map(|node| self.ui.flag(DIALOG, node_box(node), Prop::Checked, false))
            .collect()
    }

    fn set_checked(&mut self, node: usize, on: bool) {
        self.ui
            .set_prop(DIALOG, node_box(node), Prop::Checked, Value::Bool(on));
    }

    /// `0xB75F54[node] = 1` and `BM_SETCHECK(1)`: node 0 when the verify
    /// starts with connection pairs (`0x00794809..0x00794849`), a peer when
    /// its reply arrives (`0x00794568..0x007945B5`).
    pub fn connect(&mut self, node: usize) {
        if node < self.names.len() {
            self.connected[node] = true;
            self.set_checked(node, true);
        }
    }

    /// A click on node `node`'s box (`0x007932D7..0x0079334D`): the
    /// auto-checkbox toggles, then the proc forces it to the byte at
    /// `[0xB757EB + id]`. Only `0x769` (node 0) reads its connected byte;
    /// every peer box reads bytes nothing sets, so it ends unchecked.
    pub fn click(&mut self, node: usize) {
        if node < self.names.len() {
            self.set_checked(node, node == 0 && self.connected[0]);
        }
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;

    #[test]
    fn a_vera_room_shows_the_check_only_once_it_takes_a_second() {
        let dialog = WolVerifyDialog::open(vec!["Tester".into(), "Natasha".into()]);
        let opened = dialog.opened;
        assert!(dialog.shown(false, opened));
        assert!(!dialog.shown(true, opened + std::time::Duration::from_millis(999)));
        assert!(dialog.shown(true, opened + MODERN_SHOW_DELAY));
    }

    #[test]
    fn clicks_hold_node_zero_and_clear_peers() {
        let mut dialog = WolVerifyDialog::open(vec!["Tester".into(), "Natasha".into()]);
        assert_eq!(dialog.checked(), vec![false; 2]);
        dialog.click(0);
        assert!(!dialog.checked()[0], "node 0 follows its unset byte");
        dialog.connect(0);
        dialog.connect(1);
        assert!(dialog.checked()[0] && dialog.checked()[1]);
        dialog.click(0);
        dialog.click(1);
        assert!(dialog.checked()[0], "node 0 is held at its connected byte");
        assert!(!dialog.checked()[1], "a peer box reads an unrelated byte");
        dialog.connect(5);
        assert_eq!(dialog.checked().get(5), None, "no state past the nodes");
    }
    #[test]
    fn thirty_connected_nodes_survive_scroll_and_clicks_use_logical_identity() {
        for (w, h) in [(320, 240), (800, 600), (1024, 768), (1600, 1200)] {
            assert_roster_pointer_at_size(w, h);
        }
    }

    fn assert_roster_pointer_at_size(w: i32, h: i32) {
        use crate::ui::shell::{geom, modal::wol_verify_layout};

        let mut dialog = WolVerifyDialog::open((0..30).map(|n| format!("Player{n}")).collect());
        let scale = geom::shell_scale_for_window(w, h);
        let layout = wol_verify_layout(w, h, scale);
        let ok = RectPx::default();
        let click = |dialog: &mut WolVerifyDialog, at| {
            dialog.pointer(&layout, ok, at, Some(true));
            dialog.pointer(&layout, ok, at, Some(false))
        };
        dialog.connect(0);
        dialog.connect(29); // A hidden real peer response must remain visible later.
        assert!(dialog.node_rect(&layout, 29).is_none());
        let geometry = dialog.list_geometry(&layout).unwrap();
        let bar = geometry.scrollbar.unwrap();
        if !scale.is_unscaled() {
            assert_eq!(geometry.visible_rows, BOXES.len());
        }
        assert_eq!(bar.w, ListMetrics::scaled(scale).scrollbar_w);
        // The ordinary scrollbar track, then its release.
        click(
            &mut dialog,
            (bar.x + 2, bar.y + bar.h - geometry.metrics.arrow_h - 2),
        );
        let visible = dialog.visible_nodes(&layout);
        assert!(visible.start > 0 && visible.contains(&29));
        assert!(dialog.checked()[29]);
        let last = dialog.node_rect(&layout, 29).unwrap();
        click(&mut dialog, (last.x + 1, last.y + 1));
        assert!(!dialog.checked()[29], "the last rendered row is clickable");
        let first = visible.start;
        dialog.connect(first);
        let rect = dialog.node_rect(&layout, first).unwrap();
        let events = click(&mut dialog, (rect.x + 1, rect.y + 1));
        assert!(
            events.is_empty(),
            "the release does nothing more: {events:?}"
        );
        assert!(
            !dialog.checked()[first],
            "scrolled first row is a remote peer, not node zero"
        );
        assert!(dialog.checked()[0]);
        // A held arrow repeats after 500 ms, the shared scrollbar's.
        let up = (bar.x + 2, bar.y + 2);
        dialog.pointer(&layout, ok, up, Some(true));
        let (_, deadline) = dialog.poll_scroll();
        assert!(deadline.is_some());
        dialog.pointer(&layout, ok, up, Some(false));
        assert_eq!(dialog.poll_scroll().1, None);
    }
}
