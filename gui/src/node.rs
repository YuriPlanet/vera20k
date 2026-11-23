//! The retained node tree (docs/gui-library-api.md §2.1–§2.3).
//!
//! Rules: a failed change leaves the tree as it was; a removed id answers
//! `Stale`; only container kinds hold children; a prop takes one value type.

use std::collections::BTreeMap;
use std::ops::ControlFlow;

use crate::Rect;


#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId {
    idx: u32,
    generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    NotFound,
    Stale,
    WrongKind,
    Limit,
    /// The caller's origin may not do this ([`crate::Origin`]).
    Denied,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Panel,
    Dialog,
    Text,
    Button,
    Checkbox,
    Combo,
    List,
    Trackbar,
    Edit,
    /// A lone vertical scrollbar over rows its host lays out
    /// ([`crate::widget::scrollbar`]).
    ScrollBar,
    /// A picture named by `Prop::Art` ([`crate::Gui::paint`]).
    Image,
    /// A proportional bar, `Prop::Fill` percent full ([`crate::Gui::paint`]).
    Bar,
    /// An area the host fills itself (a map, a video), named by `Prop::Art`
    /// ([`crate::Gui::paint`]).
    External,
}

impl NodeKind {
    pub fn is_container(self) -> bool {
        matches!(self, NodeKind::Panel | NodeKind::Dialog)
    }

    pub fn is_focusable(self) -> bool {
        matches!(
            self,
            NodeKind::Button
                | NodeKind::Checkbox
                | NodeKind::Combo
                | NodeKind::List
                | NodeKind::Trackbar
                | NodeKind::Edit
                | NodeKind::ScrollBar
        )
    }
}

/// Property names (§2.3). Only the props the migrated screens use exist yet;
/// the rest of the §2.3 table is added with the screen that needs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Prop {
    // The props [`crate::Gui::paint`] and [`crate::Gui::find`] read.
    /// The node's name among its origin's nodes ([`crate::Gui::find`]).
    Name,
    /// 0..=100.
    Fill,
    /// A picture or surface name; the host resolves it.
    Art,
    /// The subtree is clipped to the node's rect.
    Clip,
    Background,
    /// The host's own integer id for the node (a dialog resource id).
    Id,
    Visible,
    Enabled,
    Text,
    Label,
    Tooltip,
    Rect,
    Toggled,
    Checked,
    Selected,
    Pos,
    Min,
    Max,
    Step,
    /// List, Combo: the rows' texts (§5.1, §5.3).
    Items,
    /// List, Combo: the row count when the host paints its own rows.
    RowCount,
    /// List, Combo popup: the first visible row.
    Top,
    /// Combo: the drop-down list is open.
    Open,
    /// Combo: rows the drop-down shows at once before it scrolls; 0 (the
    /// default) shows every row.
    VisibleRows,
    /// Edit: the most UTF-16 units typing admits (EM_LIMITTEXT); 0 or unset
    /// is unlimited.
    MaxLen,
    Password,
    /// Edit: typing admits only units below 0x100 (NewEdit's byte-only flag).
    ByteOnly,
    /// Edit: the most bytes the text takes in the host's code page
    /// ([`crate::Gui::set_byte_len`]): an ANSI edit's `EM_LIMITTEXT`. 0 or
    /// unset is unlimited.
    MaxBytes,
    /// Edit: every press selects the whole text (`EM_SETSEL 0, -1`).
    SelectOnFocus,
    /// Edit: the whole text is selected. Typing replaces it, Backspace and
    /// Delete remove it, a caret key drops it.
    AllSelected,
    /// Edit: characters typing refuses (NewEdit's reject table).
    Rejected,
    /// Edit: when set, the only characters typing admits.
    Allowed,
    /// Edit: caret position in chars.
    Caret,
    /// Text, Checkbox caption: the host's text alignment flags.
    Align,
    /// Trackbar: the value plaque at the right end (default on).
    ShowValue,
    /// Combo: a press anywhere on the face opens the list, not only on the
    /// arrow.
    OpenAnywhere,
    /// Combo: the drop-down rises to end at or above this y, but not above
    /// the window top (a list kept above its box's footer).
    PopupBottom,
    /// The area the pointer hits, when other than `Rect`. A checkbox with one
    /// takes a press anywhere in it, not only on its icon (a row whose
    /// caption toggles too).
    HitRect,
    Color,
    /// Combo: each row's colour swatch for the host to paint in place of its
    /// text (a palette index in decimal; empty for a text row).
    Swatches,
    /// Combo: px the drop-down grows to the right while its list scrolls
    /// (a narrow face whose list needs wider captions).
    PopupWiden,
    /// Combo: when the drop-down would end below this y it opens above the
    /// face instead (a long list low on the page).
    PopupFlipBelow,
    /// Combo: the open list's whole rect, when the host lays it out (the
    /// random-map dialog's lists hang flush under the face and may run past
    /// their rows); a press past the rows dismisses.
    PopupRect,
    /// List, Combo drop-down: rows this many px tall, not the shell's
    /// (a list at smaller text). With a List's `VisibleRows`, the scrollbar
    /// ranges over the tops that keep that many rows showing.
    RowHeight,
    /// Trackbar: hits and drags at the GUI's base scale fitted to this rect's
    /// size (`ShellScale::in_window`), not its dialog's scale (a bar drawn
    /// smaller than its page).
    ScaleScreen,
    /// The node hovers, but a press or release passes to the node under it
    /// (a cell whose list does not open lets the press reach the box under
    /// it).
    PressThrough,
    /// Combo: its open list neither shows nor takes input until cleared,
    /// and then shows again as it was (a list whose row moved away).
    PopupHidden,
    /// The text colour while the node is disabled, in place of its kind's.
    DisabledColor,
    /// Text, List rows: the caption's insets from the node's rect (or a
    /// row's) in base pixels, as `Rect { x: left, y: top, w: right, h: bottom }`.
    /// Edit: a framed field; [`crate::Gui::keep_caret_in_view`] measures its
    /// text area as the rect less the inset and the caret's 2 px.
    TextInset,
    /// List: a press still selects, but the selected row shows no fill
    /// (`LBS_NOSEL`, a chat transcript).
    NoSelectionFill,
    /// List: `LBS_MULTIPLESEL`: a press toggles its row in `SelectedRows`
    /// (and reports it as `ListSelected`); `Selected` stays as it is.
    MultiSelect,
    /// A press on the node may carry it (`UiEvent::DragStarted`); a press a
    /// drop-down takes may not.
    Draggable,
    /// Px the pointer moves from a `Draggable` node's press, on either
    /// axis, before the node is carried (0 or unset: the first move).
    DragThreshold,
    /// The node takes a carried node ([`crate::UiEvent::Dropped`]'s
    /// `target`); it need take no press.
    DropTarget,
    /// List with `MultiSelect`: the selected rows, each in decimal, in the
    /// order they were selected.
    SelectedRows,
    /// Button: Enter activates it (the dialog's IDOK). It need not show: a
    /// hidden one stands for a command no button carries.
    Default,
    /// Button: Escape activates it (the dialog's IDCANCEL); hidden as `Default`.
    Cancel,
    /// Whether Tab stops on the node (`WS_TABSTOP`); an `Edit` does unless
    /// cleared, other kinds only when set.
    TabStop,
    /// Edit: Tab gives it the focus without restarting its caret blink.
    KeepsBlink,
    /// A control that is no edit takes the dialog's focus on a press, off
    /// the edits ([`crate::Gui::focused_control`]): a window that reads the
    /// keys itself (a hotkey field), or one whose press moves the focus off
    /// it (a category list).
    FocusOnPress,
    /// Button: a left release over it activates it whatever the press did,
    /// off it or none (buttons that act on the release alone).
    ReleaseActivates,
    /// A press it took ends with the hover hit-tested again at the release
    /// point (events as a move there), not left until the next move: a
    /// trackbar or list that held the pointer.
    RehoverOnRelease,
    /// List: rows a wheel notch scrolls (3 unless set).
    WheelStep,
    /// Combo: while its list is open, the wheel anywhere picks the row next
    /// to the selection (the previous for a notch up), within the rows, and
    /// closes the list without `ComboClosed` (no close cue); `Changed` only
    /// when the selection moved.
    WheelPicks,
    /// A label: a left press on it while it is hovered, enabled or not,
    /// first gives the focus to the `Edit` with this host id in its dialog
    /// (as `Gui::set_focus`: no event, the caret keeps its blink phase);
    /// the press then goes on as any other (VERA's online settings' titles).
    FocusTarget,
    /// Edit: a left press anywhere else in its dialog, on a control or not
    /// and over an open drop-down too, takes its focus.
    BlurOnPress,
    /// Edit: NewEdit's multiline style. Enter appends CRLF to the end
    /// (`0x00614DEB`, past `MaxLen` and wherever the caret is, which stays)
    /// and reports `Submitted`; it goes on to the dialog's `Default` button
    /// only when the text is not empty once its trailing CR/LF go (the
    /// owner's 0x501 handler clicks Send then, `0xCF`/`0x11E`).
    Multiline,
    /// A node of the chrome layer ([`crate::Gui::push_chrome`]): the mouse
    /// buttons it takes, bit 1 the left and bit 2 the right (the left alone
    /// unless set). A press or release of a button it does not take passes
    /// on to the next node under the pointer.
    Buttons,
    /// Chrome button: it activates on the press edge (`Activated` for the
    /// left button, `RightActivated` for the right) and takes no capture,
    /// so its release passes on.
    PressActivates,
    /// Chrome button: a catcher the host handles itself. It takes and holds
    /// the pointer as a button does but activates nothing; the input it
    /// takes reports it in [`crate::InputResult::passthrough`], unconsumed.
    Passthrough,
    /// Chrome button: a release that activates it with the pointer inside
    /// its rect flips `Toggled` (1) or turns it on (2).
    ToggleOnRelease,
    /// Chrome button: nothing under it sees a press or release over it that
    /// it does not act on (a button it does not take, a release it holds no
    /// capture for); it consumes them and does nothing.
    Opaque,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i32),
    Text(String),
    Rect(Rect),
    Rgba(Rgba),
    Items(Vec<String>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ValueType {
    Bool,
    Int,
    Text,
    Rect,
    Rgba,
    Items,
}

impl Prop {
    fn value_type(self) -> ValueType {
        match self {
            Prop::Visible
            | Prop::Enabled
            | Prop::Toggled
            | Prop::Checked
            | Prop::Open
            | Prop::ShowValue
            | Prop::OpenAnywhere
            | Prop::Password
            | Prop::PressThrough
            | Prop::PopupHidden
            | Prop::NoSelectionFill
            | Prop::MultiSelect
            | Prop::Draggable
            | Prop::DropTarget
            | Prop::Default
            | Prop::Cancel
            | Prop::TabStop
            | Prop::KeepsBlink
            | Prop::FocusOnPress
            | Prop::ReleaseActivates
            | Prop::RehoverOnRelease
            | Prop::BlurOnPress
            | Prop::Multiline
            | Prop::PressActivates
            | Prop::Passthrough
            | Prop::Opaque
            | Prop::WheelPicks
            | Prop::SelectOnFocus
            | Prop::AllSelected
            | Prop::Clip
            | Prop::ByteOnly => ValueType::Bool,
            Prop::Items | Prop::Swatches | Prop::SelectedRows => ValueType::Items,
            Prop::Text
            | Prop::Label
            | Prop::Tooltip
            | Prop::Name
            | Prop::Art
            | Prop::Rejected
            | Prop::Allowed => {
                ValueType::Text
            }
            Prop::Rect | Prop::HitRect | Prop::PopupRect | Prop::ScaleScreen | Prop::TextInset => ValueType::Rect,
            Prop::Color | Prop::DisabledColor | Prop::Background => ValueType::Rgba,
            Prop::Id
            | Prop::Selected
            | Prop::Pos
            | Prop::Min
            | Prop::Max
            | Prop::Step
            | Prop::RowCount
            | Prop::Top
            | Prop::VisibleRows
            | Prop::DragThreshold
            | Prop::MaxLen
            | Prop::MaxBytes
            | Prop::Caret
            | Prop::Align
            | Prop::PopupBottom
            | Prop::PopupWiden
            | Prop::PopupFlipBelow
            | Prop::RowHeight
            | Prop::FocusTarget
            | Prop::Fill
            | Prop::Buttons
            | Prop::ToggleOnRelease
            | Prop::WheelStep => ValueType::Int,
        }
    }

    fn allowed_on(self, kind: NodeKind) -> bool {
        match self {
            Prop::Toggled
            | Prop::Default
            | Prop::Cancel
            | Prop::ReleaseActivates
            | Prop::Buttons
            | Prop::PressActivates
            | Prop::Passthrough
            | Prop::Opaque
            | Prop::ToggleOnRelease => kind == NodeKind::Button,
            Prop::TabStop => kind.is_focusable(),
            Prop::KeepsBlink | Prop::BlurOnPress | Prop::Multiline => kind == NodeKind::Edit,
            Prop::FocusOnPress => !kind.is_container() && kind != NodeKind::Edit,
            Prop::Checked => kind == NodeKind::Checkbox,
            Prop::OpenAnywhere
            | Prop::PopupBottom
            | Prop::Swatches
            | Prop::PopupWiden
            | Prop::PopupFlipBelow
            | Prop::PopupRect
            | Prop::WheelPicks => kind == NodeKind::Combo,
            Prop::NoSelectionFill | Prop::WheelStep | Prop::MultiSelect | Prop::SelectedRows => {
                kind == NodeKind::List
            }
            Prop::RowHeight => matches!(kind, NodeKind::List | NodeKind::Combo),
            Prop::ScaleScreen => kind == NodeKind::Trackbar,
            Prop::Min | Prop::Step => kind == NodeKind::Trackbar,
            Prop::Max => matches!(kind, NodeKind::Trackbar | NodeKind::ScrollBar),
            Prop::Fill => kind == NodeKind::Bar,
            Prop::Clip => kind.is_container(),
            _ => true,
        }
    }
}

impl Value {
    fn value_type(&self) -> ValueType {
        match self {
            Value::Bool(_) => ValueType::Bool,
            Value::Int(_) => ValueType::Int,
            Value::Text(_) => ValueType::Text,
            Value::Rect(_) => ValueType::Rect,
            Value::Rgba(_) => ValueType::Rgba,
            Value::Items(_) => ValueType::Items,
        }
    }
}

pub(crate) struct Node {
    pub kind: NodeKind,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub props: BTreeMap<Prop, Value>,
    /// Interaction state a widget keeps between inputs (scroll capture,
    /// double-click timing, caret blink); never set by the host.
    pub state: crate::widget::WidgetState,
}

impl Node {
    pub fn bool_prop(&self, prop: Prop, default: bool) -> bool {
        match self.props.get(&prop) {
            Some(Value::Bool(b)) => *b,
            _ => default,
        }
    }

    pub fn int_prop(&self, prop: Prop, default: i32) -> i32 {
        match self.props.get(&prop) {
            Some(Value::Int(v)) => *v,
            _ => default,
        }
    }

    /// A list's or combo's rows: its `Items`, else its `RowCount`.
    pub fn row_count(&self) -> usize {
        match self.props.get(&Prop::Items) {
            Some(Value::Items(items)) => items.len(),
            _ => self.int_prop(Prop::RowCount, 0).max(0) as usize,
        }
    }

    /// A list's or combo's selected row; `-1` or absent is none.
    pub fn selected(&self) -> Option<usize> {
        usize::try_from(self.int_prop(Prop::Selected, -1)).ok()
    }

    pub fn text_prop(&self, prop: Prop) -> Option<&str> {
        match self.props.get(&prop) {
            Some(Value::Text(s)) => Some(s),
            _ => None,
        }
    }

    pub fn rect(&self) -> Rect {
        match self.props.get(&Prop::Rect) {
            Some(Value::Rect(r)) => *r,
            _ => Rect::default(),
        }
    }
}

struct Slot {
    generation: u32,
    node: Option<Node>,
}

pub struct Tree {
    slots: Vec<Slot>,
    free: Vec<u32>,
    live: usize,
    max_nodes: usize,
}

impl Tree {
    pub fn new(max_nodes: usize) -> Self {
        Tree { slots: Vec::new(), free: Vec::new(), live: 0, max_nodes }
    }

    pub(crate) fn max_nodes(&self) -> usize {
        self.max_nodes
    }

    pub fn live_count(&self) -> usize {
        self.live
    }

    pub fn create(&mut self, kind: NodeKind, parent: Option<NodeId>) -> Result<NodeId, Refused> {
        if let Some(p) = parent {
            if !self.node(p)?.kind.is_container() {
                return Err(Refused::WrongKind);
            }
        }
        if self.live >= self.max_nodes {
            return Err(Refused::Limit);
        }
        let node = Node { kind, parent, children: Vec::new(), props: BTreeMap::new(), state: Default::default() };
        let id = match self.free.pop() {
            Some(idx) => {
                let slot = &mut self.slots[idx as usize];
                slot.node = Some(node);
                NodeId { idx, generation: slot.generation }
            }
            None => {
                let idx = self.slots.len() as u32;
                self.slots.push(Slot { generation: 0, node: Some(node) });
                NodeId { idx, generation: 0 }
            }
        };
        self.live += 1;
        if let Some(p) = parent {
            self.node_mut(p).expect("checked above").children.push(id);
        }
        Ok(id)
    }

    /// Removes the node and its subtree; every id inside becomes `Stale`.
    pub fn remove(&mut self, id: NodeId) -> Result<(), Refused> {
        if let Some(p) = self.node(id)?.parent {
            self.node_mut(p)?.children.retain(|c| *c != id);
        }
        self.remove_subtree(id);
        Ok(())
    }

    /// Removes the nodes with their subtrees, in the order given, going over
    /// each parent's children once.
    pub(crate) fn remove_all(&mut self, ids: &[NodeId]) {
        let mut parents = Vec::new();
        for &id in ids {
            let Ok(node) = self.node(id) else { continue };
            parents.extend(node.parent);
            self.remove_subtree(id);
        }
        parents.sort_unstable();
        parents.dedup();
        for parent in parents {
            let Ok(node) = self.node_mut(parent) else {
                continue;
            };
            let mut children = std::mem::take(&mut node.children);
            children.retain(|child| self.node(*child).is_ok());
            if let Ok(node) = self.node_mut(parent) {
                node.children = children;
            }
        }
    }

    /// No recursion: a caller may nest as deep as the node limit.
    fn remove_subtree(&mut self, id: NodeId) {
        let mut pending = vec![id];
        while let Some(id) = pending.pop() {
            let slot = &mut self.slots[id.idx as usize];
            let Some(node) = slot.node.take() else { continue };
            // A slot out of generations is retired: reused, it would revive a stale id.
            if let Some(next) = slot.generation.checked_add(1) {
                slot.generation = next;
                self.free.push(id.idx);
            }
            self.live -= 1;
            pending.extend(node.children.into_iter().rev());
        }
    }

    pub fn set(&mut self, id: NodeId, prop: Prop, value: Value) -> Result<(), Refused> {
        let node = self.node_mut(id)?;
        if prop.value_type() != value.value_type() || !prop.allowed_on(node.kind) {
            return Err(Refused::WrongKind);
        }
        node.props.insert(prop, value);
        Ok(())
    }

    /// Back to the prop's default; whether it was set.
    pub fn clear(&mut self, id: NodeId, prop: Prop) -> Result<bool, Refused> {
        Ok(self.node_mut(id)?.props.remove(&prop).is_some())
    }

    pub fn get(&self, id: NodeId, prop: Prop) -> Result<Option<&Value>, Refused> {
        Ok(self.node(id)?.props.get(&prop))
    }

    pub fn kind(&self, id: NodeId) -> Result<NodeKind, Refused> {
        Ok(self.node(id)?.kind)
    }

    /// Visits `id` and its subtree in tree order, each node before its
    /// children, until `f` breaks; the break's value.
    pub fn walk<B>(
        &self,
        id: NodeId,
        f: &mut impl FnMut(NodeId, NodeKind) -> ControlFlow<B>,
    ) -> ControlFlow<B> {
        let Ok(node) = self.node(id) else { return ControlFlow::Continue(()) };
        f(id, node.kind)?;
        for child in &node.children {
            self.walk(*child, f)?;
        }
        ControlFlow::Continue(())
    }

    pub fn children(&self, id: NodeId) -> Result<&[NodeId], Refused> {
        Ok(&self.node(id)?.children)
    }

    pub fn parent(&self, id: NodeId) -> Result<Option<NodeId>, Refused> {
        Ok(self.node(id)?.parent)
    }

    pub(crate) fn node(&self, id: NodeId) -> Result<&Node, Refused> {
        match self.slots.get(id.idx as usize) {
            None => Err(Refused::NotFound),
            Some(Slot { generation, node: Some(n) }) if *generation == id.generation => Ok(n),
            Some(_) => Err(Refused::Stale),
        }
    }

    pub(crate) fn node_mut(&mut self, id: NodeId) -> Result<&mut Node, Refused> {
        match self.slots.get_mut(id.idx as usize) {
            None => Err(Refused::NotFound),
            Some(Slot { generation, node: Some(n) }) if *generation == id.generation => Ok(n),
            Some(_) => Err(Refused::Stale),
        }
    }

    /// Visible and enabled through every ancestor (§8.9).
    pub(crate) fn effective(&self, id: NodeId, prop: Prop) -> bool {
        let mut cur = Some(id);
        while let Some(c) = cur {
            let Ok(n) = self.node(c) else { return false };
            if !n.bool_prop(prop, true) {
                return false;
            }
            cur = n.parent;
        }
        true
    }

    /// Screen rect: the node's `Rect`.
    pub fn screen_rect(&self, id: NodeId) -> Result<Rect, Refused> {
        Ok(self.node(id)?.rect())
    }

    /// Screen area the pointer hits: `HitRect` when set, else the `Rect`.
    pub fn screen_hit_rect(&self, id: NodeId) -> Result<Rect, Refused> {
        match self.node(id)?.props.get(&Prop::HitRect) {
            Some(Value::Rect(r)) => Ok(*r),
            _ => self.screen_rect(id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removed_id_is_stale_and_slot_reuse_does_not_revive_it() {
        let mut t = Tree::new(8);
        let root = t.create(NodeKind::Dialog, None).unwrap();
        let child = t.create(NodeKind::Button, Some(root)).unwrap();
        t.remove(root).unwrap();
        let again = t.create(NodeKind::Button, None).unwrap();
        assert_eq!(t.get(child, Prop::Text), Err(Refused::Stale));
        assert!(t.get(again, Prop::Text).is_ok());
        assert_eq!(t.live_count(), 1);
    }

    #[test]
    fn a_slot_out_of_generations_is_retired() {
        let mut t = Tree::new(1);
        let first = t.create(NodeKind::Button, None).unwrap();
        t.remove(first).unwrap();
        t.slots[0].generation = u32::MAX;
        let last = t.create(NodeKind::Button, None).unwrap();
        t.remove(last).unwrap();
        let next = t.create(NodeKind::Button, None).unwrap();
        assert_ne!(next.idx, last.idx);
        assert_eq!(t.get(last, Prop::Text), Err(Refused::Stale));
    }

    #[test]
    fn wrong_value_type_leaves_old_value() {
        let mut t = Tree::new(8);
        let b = t.create(NodeKind::Button, None).unwrap();
        t.set(b, Prop::Label, Value::Text("GUI:Ok".into())).unwrap();
        assert_eq!(t.set(b, Prop::Label, Value::Int(1)), Err(Refused::WrongKind));
        assert_eq!(t.get(b, Prop::Label).unwrap(), Some(&Value::Text("GUI:Ok".into())));
    }

    #[test]
    fn leaves_hold_no_children_and_toggled_is_button_only() {
        let mut t = Tree::new(8);
        let text = t.create(NodeKind::Text, None).unwrap();
        assert_eq!(t.create(NodeKind::Button, Some(text)), Err(Refused::WrongKind));
        assert_eq!(t.set(text, Prop::Toggled, Value::Bool(true)), Err(Refused::WrongKind));
    }

    #[test]
    fn limit_is_enforced() {
        let mut t = Tree::new(1);
        t.create(NodeKind::Panel, None).unwrap();
        assert_eq!(t.create(NodeKind::Panel, None), Err(Refused::Limit));
    }
}
