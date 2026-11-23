//! Widget behaviour behind `Gui::input`. `Gui` owns hit-testing, capture and
//! the dialog stack; each module here owns one kind's reaction to a press, a
//! captured move, a release, the wheel, keys and its timers. Everything a
//! widget changes goes through `Ctx` (props in the tree, events out).
//!
//! The behaviour is gamemd's owner-draw shell controls; each module cites the
//! native functions it follows.

pub(crate) mod combo;
pub(crate) mod edit;
pub(crate) mod list;
pub(crate) mod scrollbar;

use std::sync::OnceLock;

use crate::gui::{Key, UiEvent, UiMs};
use crate::node::{NodeId, Tree};
use crate::shell::geom::ShellScale;
use crate::time::{Duration, Instant};

/// What a widget sees of the GUI while it handles one input.
pub(crate) struct Ctx<'a> {
    pub tree: &'a mut Tree,
    pub events: &'a mut Vec<UiEvent>,
    /// The dialog's device-to-authored mapping.
    pub scale: ShellScale,
    pub now: UiMs,
    /// The host's double-click limits: milliseconds, width, height.
    pub double_click: (UiMs, i32, i32),
    /// The host's code-page length of a text (`MaxBytes`).
    pub byte_len: fn(&str) -> usize,
    /// The dialog's open popup (a combo's drop-down list): while set, every
    /// pointer input of the dialog goes to it first.
    pub popup: &'a mut Option<NodeId>,
    /// The dialog's keyboard focus (an `Edit`).
    pub focus: &'a mut Option<NodeId>,
}

/// The substrate keeps its timers as `Instant`s; the GUI clock is `UiMs`.
/// One process-wide epoch maps between them.
fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

pub(crate) fn instant(ms: UiMs) -> Instant {
    epoch() + Duration::from_millis(ms)
}

pub(crate) fn ui_ms(at: Instant) -> UiMs {
    at.saturating_duration_since(epoch()).as_millis() as UiMs
}

/// Per-node interaction state, kept in the tree beside the props.
#[derive(Debug, Default)]
pub enum WidgetState {
    #[default]
    None,
    List(list::ListState),
    Combo(combo::ComboState),
    Edit(edit::EditState),
}

/// Whether a press takes the pointer capture (later moves and the release
/// come back to the same node).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Capture {
    No,
    Yes,
}

/// Keys the widgets read; `Gui` routes them to the focused node.
pub(crate) fn is_edit_key(key: Key) -> bool {
    !matches!(key, Key::Tab | Key::BackTab)
}
