//! The connection verify `0x130` ("Connecting to other players"): the
//! compact popup Start Game Now (`0x00794BA0`) pushes over C2/C9 after
//! OnGameStart, with proc `0x00793280`. The native eight rows have no timer;
//! VERA extended rosters use the shared scrollbar interaction.
//! one auto-checkbox per session node shows which peers answered, and the
//! negotiation's mismatch box `0xD0` may open over it.

use crate::ui::shell::button::ShellButtonInteraction;
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::list::{ListMetrics, ListScrollInteraction, ShellListGeometry};
use crate::ui::shell::modal::WolVerifyLayout;
use crate::ui::wol_shell::WolMessageBox;
use std::ops::Range;
use crate::util::time::Instant;

/// `0x0084A1F0`: node `i`'s checkbox, stacked by `i` in the template.
pub const BOXES: [u16; 8] = [0x769, 0x761, 0x764, 0x762, 0x765, 0x768, 0x766, 0x767];

#[derive(Debug, Clone)]
pub struct WolVerifyDialog {
    /// The node names in `[0xA8DA78]` order (`0x497`'s `0x4B2` captions);
    /// native boxes past them are destroyed; the VERA viewport retains every node.
    pub names: Vec<String>,
    /// `0xB75F54[i]`: node `i` answered.
    connected: Vec<bool>,
    /// The boxes' own check state.
    pub checked: Vec<bool>,
    top: usize,
    scroll: ListScrollInteraction,
    /// `0xD0` with `TXT_SCENARIOS_DO_NOT_MATCH` over the dialog
    /// (`0x00794D28..0x00794D57`).
    pub mismatch: Option<WolMessageBox>,
    /// The mismatch box's OK ([`crate::ui::wol_shell::BOX_OK`]).
    pub ok: ShellButtonInteraction<u16>,
}

impl WolVerifyDialog {
    /// `0x497` (`0x00793362..0x0079341B`): the connected bytes clear and
    /// every remaining box is unchecked.
    pub fn open(mut names: Vec<String>) -> Self {
        names.truncate(crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT);
        let count = names.len();
        Self {
            names,
            connected: vec![false; count],
            checked: vec![false; count],
            top: 0,
            scroll: ListScrollInteraction::default(),
            mismatch: None,
            ok: ShellButtonInteraction::default(),
        }
    }

    /// The eight resource rows of Verify793280 remain exact for ordinary
    /// matches. VERA's extended roster uses the existing list/scroll owner;
    /// only this viewport offset is local, not another membership list.
    pub fn list_geometry(&self, layout: &WolVerifyLayout) -> Option<ShellListGeometry> {
        if self.names.len() <= BOXES.len() {
            return None;
        }
        let first = layout.boxes[0];
        let last = layout.boxes[BOXES.len() - 1];
        Some(ShellListGeometry::new(
            RectPx::new(
                first.x - 1,
                first.y - 1,
                first.w + 2,
                last.y + last.h - first.y + 2,
            ),
            self.names.len(),
            self.top,
            ListMetrics {
                row_h: first.h,
                ..ListMetrics::RETAIL
            },
        ))
    }

    pub fn visible_nodes(&self, layout: &WolVerifyLayout) -> Range<usize> {
        let Some(geometry) = self.list_geometry(layout) else {
            return 0..self.names.len();
        };
        let top = self.top.min(geometry.max_top);
        top..(top + geometry.visible_rows).min(self.names.len())
    }

    pub fn node_rect(&self, layout: &WolVerifyLayout, node: usize) -> Option<RectPx> {
        if !self.visible_nodes(layout).contains(&node) {
            return None;
        }
        Some(match self.list_geometry(layout) {
            Some(geometry) => geometry.row(node - self.top.min(geometry.max_top)),
            None => layout.boxes[node],
        })
    }

    pub fn pressed_scroll_part(&self) -> Option<crate::ui::shell::list::ListScrollPart> {
        self.scroll.pressed_part()
    }

    /// Return the logical checkbox under a real pointer press. A physical
    /// first row on a later viewport never acquires native node-zero behavior.
    pub fn pointer_button(
        &mut self,
        layout: &WolVerifyLayout,
        x: i32,
        y: i32,
        pressed: bool,
        now: Instant,
    ) -> Option<usize> {
        if !pressed {
            self.scroll.cancel();
            return None;
        }
        if let Some(geometry) = self.list_geometry(layout)
            && let Some(part) = geometry.scroll_part_at(x, y)
        {
            self.scroll.press(part, geometry, &mut self.top, y, now);
            return None;
        }
        let node = self.visible_nodes(layout).find(|&node| {
            crate::ui::skirmish_shell::checkbox_icon_rect(
                self.node_rect(layout, node).expect("visible node"),
                ShellScale::RETAIL,
            )
            .contains(x, y)
        })?;
        self.click(node);
        Some(node)
    }

    pub fn pointer_moved(&mut self, layout: &WolVerifyLayout, x: i32, y: i32) {
        if let Some(geometry) = self.list_geometry(layout) {
            self.scroll.pointer_moved(geometry, &mut self.top, x, y);
        }
    }

    pub fn poll_scroll(
        &mut self,
        layout: &WolVerifyLayout,
        x: i32,
        y: i32,
        now: Instant,
    ) -> (bool, Option<Instant>) {
        let Some(geometry) = self.list_geometry(layout) else {
            return (false, None);
        };
        let changed = self.scroll.poll(geometry, &mut self.top, x, y, now);
        (changed, self.scroll.repeat_at())
    }

    /// `0xB75F54[node] = 1` and `BM_SETCHECK(1)`: node 0 when the verify
    /// starts with connection pairs (`0x00794809..0x00794849`), a peer when
    /// its reply arrives (`0x00794568..0x007945B5`).
    pub fn connect(&mut self, node: usize) {
        if node < self.names.len() {
            self.connected[node] = true;
            self.checked[node] = true;
        }
    }

    /// A click on node `node`'s box (`0x007932D7..0x0079334D`): the
    /// auto-checkbox toggles, then the proc forces it to the byte at
    /// `[0xB757EB + id]`. Only `0x769` (node 0) reads its connected byte;
    /// every peer box reads bytes nothing sets, so it ends unchecked.
    pub fn click(&mut self, node: usize) {
        if node < self.names.len() {
            self.checked[node] = node == 0 && self.connected[0];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_hold_node_zero_and_clear_peers() {
        let mut dialog = WolVerifyDialog::open(vec!["Tester".into(), "Natasha".into()]);
        assert_eq!(dialog.checked, vec![false; 2]);
        dialog.click(0);
        assert!(!dialog.checked[0], "node 0 follows its unset byte");
        dialog.connect(0);
        dialog.connect(1);
        assert!(dialog.checked[0] && dialog.checked[1]);
        dialog.click(0);
        dialog.click(1);
        assert!(dialog.checked[0], "node 0 is held at its connected byte");
        assert!(!dialog.checked[1], "a peer box reads an unrelated byte");
        dialog.connect(5);
        assert_eq!(dialog.checked.get(5), None, "no state past the nodes");
    }
    #[test]
    fn thirty_connected_nodes_survive_scroll_and_clicks_use_logical_identity() {
        let mut dialog = WolVerifyDialog::open((0..30).map(|n| format!("Player{n}")).collect());
        let layout = crate::ui::shell::modal::wol_verify_layout(800, 600);
        let now = Instant::now();
        dialog.connect(0);
        dialog.connect(29); // A hidden real peer response must remain visible later.
        assert!(dialog.node_rect(&layout, 29).is_none());
        let geometry = dialog.list_geometry(&layout).unwrap();
        let bar = geometry.scrollbar.unwrap();
        // Use the ordinary scrollbar track, then its release, not a changed index.
        let y = bar.y + bar.h - geometry.metrics.arrow_h - 2;
        dialog.pointer_button(&layout, bar.x + 2, y, true, now);
        dialog.pointer_button(&layout, bar.x + 2, y, false, now);
        let visible = dialog.visible_nodes(&layout);
        assert!(visible.start > 0 && visible.contains(&29));
        assert_eq!(dialog.names.len(), 30);
        assert!(dialog.checked[29]);
        let first = visible.start;
        dialog.connect(first);
        let rect = dialog.node_rect(&layout, first).unwrap();
        let icon = crate::ui::skirmish_shell::checkbox_icon_rect(rect, ShellScale::RETAIL);
        assert_eq!(
            dialog.pointer_button(&layout, icon.x + 1, icon.y + 1, true, now),
            Some(first)
        );
        assert!(
            !dialog.checked[first],
            "scrolled first row is a remote peer, not node zero"
        );
        assert!(dialog.checked[0]);
        dialog.connect(29);
        assert!(dialog.checked[29]);
        // The real held-arrow path uses the shared500ms/25ms repeat owner.
        let up = (bar.x + 2, bar.y + 2);
        dialog.pointer_button(&layout, up.0, up.1, true, now);
        let first = dialog.visible_nodes(&layout).start;
        let (changed, deadline) = dialog.poll_scroll(
            &layout,
            up.0,
            up.1,
            now + std::time::Duration::from_millis(500),
        );
        assert!(changed && deadline.is_some());
        assert!(dialog.visible_nodes(&layout).start < first);
        dialog.pointer_button(&layout, up.0, up.1, false, now);
        assert_eq!(
            dialog
                .poll_scroll(&layout, up.0, up.1, now + std::time::Duration::from_secs(1))
                .1,
            None
        );
    }
}
