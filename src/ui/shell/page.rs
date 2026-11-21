//! What every shell screen does with its `vera20k-gui` widgets, done once:
//! keep their rects on the layout, turn the GUI's node events into the
//! shell's cues and into control-id events the screen acts on, and read the
//! status-line help of the hovered control. A screen describes its widgets
//! as a [`NodeSpec`] (each control carries its resource id in `Prop::Id`)
//! and matches [`PageEvent`]s by id.

use vera20k_gui::{Gui, NodeId, NodeKind, NodeSpec, Prop, UiEvent, Value};

use super::geom::RectPx;

/// The sounds the shell's owner-draw controls make; the app plays them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageCue {
    /// A press on an owner-draw button (`GUIMainButtonSound`).
    Button,
    /// A checkbox toggled (`GUICheckboxSound`).
    Checkbox,
    /// A press on a combo's face (`GUIComboOpenSound`, 0x006184A2).
    ComboOpen,
    /// A pick or dismissal in an open combo list (`GUIComboCloseSound`,
    /// 0x0060E4E9).
    ComboClose,
    /// A row picked in a list (`GenericClick`).
    ListClick,
}

/// One user action on a control, by resource id.
#[derive(Debug, Clone, PartialEq)]
pub enum PageEvent {
    Cue(PageCue),
    Activated(u16),
    Toggled(u16, bool),
    /// A combo's new selection.
    Picked(u16, i32),
    /// A trackbar's position; `final_` once the pointer lets go.
    Slid {
        id: u16,
        value: i32,
        final_: bool,
    },
    Selected(u16, u32),
    DoubleClicked(u16, u32),
    Scrolled(u16),
    TextChanged(u16, String),
    Submitted(u16, String),
    Cancelled(u16),
    /// A `Draggable` control was carried off its press.
    DragStarted(u16),
    /// A carried control was dropped, over the `DropTarget` control given.
    Dropped(u16, Option<u16>),
}

/// Builds `spec`'s children under the dialog root once; true when it built.
/// Buttons a box fed into the dialog carry no id and do not count.
pub fn ensure_built(gui: &mut Gui, root: NodeId, spec: impl FnOnce() -> NodeSpec) -> bool {
    let built = gui
        .tree()
        .children(root)
        .is_ok_and(|c| c.iter().any(|n| gui.id_of(*n).is_some()));
    if built {
        return false;
    }
    for child in spec().children {
        // A screen's own table: a refusal here is a programming error the
        // screen's tests catch, and the page then shows without that part.
        let _ = gui.create_tree(&child, Some(root));
    }
    true
}

/// Puts every control under `root` on its layout rect; a control without
/// one is hidden.
pub fn sync_rects(gui: &mut Gui, root: NodeId, rect_of: impl Fn(u16) -> Option<RectPx>) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if let Ok(children) = gui.tree().children(node) {
            stack.extend(children.iter().copied());
        }
        let Some(id) = gui.id_of(node).and_then(|id| u16::try_from(id).ok()) else {
            continue;
        };
        let rect = rect_of(id);
        let _ = gui.set(node, Prop::Rect, Value::Rect(rect.unwrap_or_default()));
        let _ = gui.set(node, Prop::Visible, Value::Bool(rect.is_some()));
    }
}

/// Sets `prop` on control `id` under `root`.
pub fn set(gui: &mut Gui, root: NodeId, id: u16, prop: Prop, value: Value) {
    if let Some(node) = gui.find_id(root, i32::from(id)) {
        let _ = gui.set(node, prop, value);
    }
}

/// Control `id`'s value of `prop` under `root`.
pub fn get(gui: &Gui, root: NodeId, id: u16, prop: Prop) -> Option<&Value> {
    gui.get(gui.find_id(root, i32::from(id))?, prop)
        .ok()
        .flatten()
}

/// Whether checkbox `id` under `root` is checked.
pub fn checked(gui: &Gui, root: NodeId, id: u16) -> bool {
    get(gui, root, id, Prop::Checked) == Some(&Value::Bool(true))
}

/// Control `id`'s integer `prop` (a trackbar's `Pos`, a list's `Selected`).
pub fn int(gui: &Gui, root: NodeId, id: u16, prop: Prop) -> Option<i32> {
    match get(gui, root, id, prop)? {
        Value::Int(value) => Some(*value),
        _ => None,
    }
}

/// A key, or typed text one character at a time, into the top dialog's
/// focused edit, for a screen that runs the edit's notifications itself:
/// `changed` runs after each change (an admitted character's `EN_CHANGE`).
pub fn edit_input(
    c: &mut super::controller::DialogController,
    event: vera20k_gui::InputEvent,
    mut changed: impl FnMut(&mut super::controller::DialogController),
) {
    for event in super::controller::per_char(event) {
        c.on_input(event);
        if c.drain_events()
            .iter()
            .any(|event| matches!(event, UiEvent::TextChanged { .. }))
        {
            changed(c);
        }
    }
}

/// [`hovered_help`] of the tree's hovered control, borrowed from the tree.
pub fn hovered_key(gui: &Gui) -> Option<&str> {
    match gui.get(gui.hovered()?, Prop::Tooltip).ok().flatten()? {
        Value::Text(key) => Some(key),
        _ => None,
    }
}

/// The status-line help of the hovered control (enable-unfiltered: a
/// disabled control still writes its help).
pub fn hovered_help(gui: &Gui, hovered: Option<NodeId>) -> Option<String> {
    match gui.get(hovered?, Prop::Tooltip).ok().flatten() {
        Some(Value::Text(key)) => Some(key.clone()),
        _ => None,
    }
}

/// A family page's right-panel buttons as nodes, each with its status help.
pub fn menu_buttons(spec: &super::menu_page::MenuPageSpec) -> impl Iterator<Item = NodeSpec> + '_ {
    spec.buttons().map(|b| {
        NodeSpec::new(NodeKind::Button)
            .id(i32::from(b.id))
            .tooltip(b.tooltip_key)
    })
}

/// The dialog rooted at `root`.
fn dialog_of(gui: &Gui, root: NodeId) -> Option<vera20k_gui::DialogId> {
    gui.dialogs().find(|d| gui.dialog_root(*d) == Some(root))
}

/// The node of the dialog rooted at `root` that shows pushed: captured and
/// still under the pointer (612B70), under another dialog too.
pub fn held_in(gui: &Gui, root: NodeId) -> Option<NodeId> {
    let dialog = dialog_of(gui, root)?;
    gui.pressed_in(dialog)
        .filter(|n| gui.hovered_in(dialog) == Some(*n))
}

/// The node a press captured in the dialog rooted at `root`, under the
/// pointer or not (the Skirmish family's buttons keep their pushed look).
pub fn captured_in(gui: &Gui, root: NodeId) -> Option<NodeId> {
    gui.pressed_in(dialog_of(gui, root)?)
}

/// The status help of the control hovered in the dialog rooted at `root`,
/// which may lie under another.
pub fn hovered_key_in(gui: &Gui, root: NodeId) -> Option<&str> {
    match gui
        .get(gui.hovered_in(dialog_of(gui, root)?)?, Prop::Tooltip)
        .ok()
        .flatten()?
    {
        Value::Text(key) => Some(key),
        _ => None,
    }
}

/// The GUI's events as cues and control-id events, in order: a control's
/// cue comes before what it did.
pub fn page_events(gui: &Gui, events: Vec<UiEvent>) -> Vec<PageEvent> {
    let mut out = Vec::with_capacity(events.len());
    let id = |node: NodeId| gui.id_of(node).and_then(|id| u16::try_from(id).ok());
    for event in events {
        match event {
            UiEvent::Pressed { node } => match gui.tree().kind(node) {
                Ok(NodeKind::Button) => out.push(PageEvent::Cue(PageCue::Button)),
                Ok(NodeKind::Combo) => out.push(PageEvent::Cue(PageCue::ComboOpen)),
                _ => {}
            },
            UiEvent::ComboOpened { .. } => out.push(PageEvent::Cue(PageCue::ComboOpen)),
            UiEvent::ComboClosed { .. } => out.push(PageEvent::Cue(PageCue::ComboClose)),
            UiEvent::Activated { node, .. } => out.extend(id(node).map(PageEvent::Activated)),
            UiEvent::Changed {
                node,
                value,
                final_,
            } => {
                let Some(id) = id(node) else { continue };
                match (gui.tree().kind(node), value) {
                    (Ok(NodeKind::Checkbox), Value::Bool(on)) => {
                        out.push(PageEvent::Cue(PageCue::Checkbox));
                        out.push(PageEvent::Toggled(id, on));
                    }
                    (Ok(NodeKind::Combo), Value::Int(row)) => out.push(PageEvent::Picked(id, row)),
                    (Ok(NodeKind::Trackbar | NodeKind::ScrollBar), Value::Int(value)) => {
                        out.push(PageEvent::Slid { id, value, final_ })
                    }
                    _ => {}
                }
            }
            UiEvent::ListSelected { node, row } => {
                out.push(PageEvent::Cue(PageCue::ListClick));
                out.extend(id(node).map(|id| PageEvent::Selected(id, row)));
            }
            UiEvent::ListDoubleClicked { node, row } => {
                out.extend(id(node).map(|id| PageEvent::DoubleClicked(id, row)))
            }
            UiEvent::Scrolled { node, .. } => out.extend(id(node).map(PageEvent::Scrolled)),
            UiEvent::TextChanged { node, text } => {
                out.extend(id(node).map(|id| PageEvent::TextChanged(id, text)))
            }
            UiEvent::Submitted { node, text } => {
                out.extend(id(node).map(|id| PageEvent::Submitted(id, text)))
            }
            UiEvent::Cancelled { node } => out.extend(id(node).map(PageEvent::Cancelled)),
            UiEvent::DragStarted { node } => out.extend(id(node).map(PageEvent::DragStarted)),
            UiEvent::Dropped { node, target } => {
                out.extend(id(node).map(|dragged| PageEvent::Dropped(dragged, target.and_then(id))))
            }
            _ => {}
        }
    }
    out
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use vera20k_gui::{DialogKind, InputEvent, MouseButton};

    #[test]
    fn a_checkbox_click_cues_before_it_reports_and_rects_follow_the_layout() {
        let mut gui = Gui::new(16);
        let root = gui.create(NodeKind::Dialog, None).unwrap();
        gui.push_dialog(root, DialogKind::Page).unwrap();
        ensure_built(&mut gui, root, || {
            NodeSpec::new(NodeKind::Dialog)
                .child(
                    NodeSpec::new(NodeKind::Checkbox)
                        .id(0x5F5)
                        .tooltip("STT:WOLOptionsPage"),
                )
                .child(NodeSpec::new(NodeKind::Button).id(1))
        });
        sync_rects(&mut gui, root, |id| {
            (id == 0x5F5).then_some(RectPx::new(0, 0, 18, 18))
        });
        let hidden = gui.find_id(root, 1).unwrap();
        assert_eq!(
            gui.get(hidden, Prop::Visible).unwrap(),
            Some(&Value::Bool(false))
        );
        for event in [
            InputEvent::PointerMove { x: 5, y: 5 },
            InputEvent::PointerDown {
                x: 5,
                y: 5,
                button: MouseButton::Left,
            },
            InputEvent::PointerUp {
                x: 5,
                y: 5,
                button: MouseButton::Left,
            },
        ] {
            gui.input(event, 0);
        }
        assert_eq!(
            hovered_help(&gui, gui.hovered()).as_deref(),
            Some("STT:WOLOptionsPage")
        );
        let events = gui.drain_events();
        assert_eq!(
            page_events(&gui, events),
            [
                PageEvent::Cue(PageCue::Checkbox),
                PageEvent::Toggled(0x5F5, true)
            ]
        );
        // The tree owns the value the screen reads back.
        assert!(checked(&gui, root, 0x5F5) && !checked(&gui, root, 1));
        assert_eq!(hovered_key(&gui), Some("STT:WOLOptionsPage"));
    }
}
