//! The connection verify `0x130` ("Connecting to other players"): the
//! compact popup Start Game Now (`0x00794BA0`) pushes over C2/C9 after
//! OnGameStart, with proc `0x00793280`. It has no buttons, timer or sound;
//! one auto-checkbox per session node shows which peers answered, and the
//! negotiation's mismatch box `0xD0` may open over it.

use crate::ui::shell::button::ShellButtonInteraction;
use crate::ui::wol_shell::WolMessageBox;

/// `0x0084A1F0`: node `i`'s checkbox, stacked by `i` in the template.
pub const BOXES: [u16; 8] = [0x769, 0x761, 0x764, 0x762, 0x765, 0x768, 0x766, 0x767];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WolVerifyDialog {
    /// The node names in `[0xA8DA78]` order (`0x497`'s `0x4B2` captions);
    /// the boxes past them are destroyed.
    pub names: Vec<String>,
    /// `0xB75F54[i]`: node `i` answered.
    connected: [bool; 8],
    /// The boxes' own check state.
    pub checked: [bool; 8],
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
        names.truncate(BOXES.len());
        Self {
            names,
            connected: [false; 8],
            checked: [false; 8],
            mismatch: None,
            ok: ShellButtonInteraction::default(),
        }
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
        assert_eq!(dialog.checked, [false; 8]);
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
        assert_eq!(dialog.checked[5], false, "no box past the nodes");
    }
}
