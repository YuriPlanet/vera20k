//! The retained node tree (docs/gui-library-api.md §2.1–§2.3).
//!
//! Rules: a failed change leaves the tree as it was; a removed id answers
//! `Stale`; only container kinds hold children; a prop takes one value type.

use std::collections::BTreeMap;

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
    NoSuchName,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Panel,
    Dialog,
    Row,
    Column,
    Text,
    Image,
    Button,
    Checkbox,
    Combo,
    List,
    Trackbar,
    ScrollBar,
    Edit,
    Custom,
}

impl NodeKind {
    pub fn is_container(self) -> bool {
        matches!(self, NodeKind::Panel | NodeKind::Dialog | NodeKind::Row | NodeKind::Column)
    }

    pub fn is_focusable(self) -> bool {
        matches!(
            self,
            NodeKind::Button
                | NodeKind::Checkbox
                | NodeKind::Combo
                | NodeKind::List
                | NodeKind::Trackbar
                | NodeKind::ScrollBar
                | NodeKind::Edit
        )
    }
}

/// Property names (§2.3). Only the props the migrated screens use exist yet;
/// the rest of the §2.3 table is added with the screen that needs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Prop {
    /// The host's own integer id for the node (a dialog resource id).
    Id,
    Visible,
    Enabled,
    Text,
    Label,
    Picture,
    Command,
    Tooltip,
    Name,
    Rect,
    OffsetX,
    OffsetY,
    Toggled,
    Checked,
    Selected,
    Pos,
    Min,
    Max,
    Step,
    /// Checkbox: toggle on the press instead of the release (§8.2).
    ActivateOnPress,
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
    /// List: a second press within the double-click limits only reports
    /// `ListDoubleClicked` (default on); off, every press selects.
    DoubleClick,
    /// Combo: a press anywhere on the face opens the list, not only on the
    /// arrow (VERA's own Options lists).
    OpenAnywhere,
    /// Combo: the drop-down rises to end at or above this y, but not above
    /// the window top (VERA's Esc menu keeps its list above the box's footer).
    PopupBottom,
    /// The area the pointer hits, when other than `Rect`. A checkbox with one
    /// takes a press anywhere in it, not only on its icon (VERA's Esc menu
    /// rows, whose caption toggles too).
    HitRect,
    Color,
    Font,
    /// List: `Rect` is the list's whole paint surface (no list-box grow);
    /// rows hit inside its frame, and a second press counts as a double-click
    /// only on a row (the WOL pages' owner-draw lists).
    ListWindow,
    /// Combo: each row's colour swatch for the host to paint in place of its
    /// text (a palette index in decimal; empty for a text row).
    Swatches,
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
            | Prop::ActivateOnPress
            | Prop::Open
            | Prop::ShowValue
            | Prop::DoubleClick
            | Prop::OpenAnywhere
            | Prop::ListWindow
            | Prop::Password
            | Prop::ByteOnly => ValueType::Bool,
            Prop::Items | Prop::Swatches => ValueType::Items,
            Prop::Text
            | Prop::Label
            | Prop::Picture
            | Prop::Command
            | Prop::Tooltip
            | Prop::Name
            | Prop::Rejected
            | Prop::Allowed => {
                ValueType::Text
            }
            Prop::Rect | Prop::HitRect => ValueType::Rect,
            Prop::Color => ValueType::Rgba,
            Prop::Id
            | Prop::OffsetX
            | Prop::OffsetY
            | Prop::Selected
            | Prop::Pos
            | Prop::Min
            | Prop::Max
            | Prop::Step
            | Prop::RowCount
            | Prop::Top
            | Prop::VisibleRows
            | Prop::MaxLen
            | Prop::Caret
            | Prop::Align
            | Prop::PopupBottom
            | Prop::Font => ValueType::Int,
        }
    }

    /// `Command` belongs to buttons only (§2.3).
    fn allowed_on(self, kind: NodeKind) -> bool {
        match self {
            Prop::Command | Prop::Toggled => kind == NodeKind::Button,
            Prop::Checked | Prop::ActivateOnPress => kind == NodeKind::Checkbox,
            Prop::DoubleClick | Prop::ListWindow => kind == NodeKind::List,
            Prop::OpenAnywhere | Prop::PopupBottom | Prop::Swatches => kind == NodeKind::Combo,
            Prop::Min | Prop::Max | Prop::Step => matches!(kind, NodeKind::Trackbar | NodeKind::ScrollBar),
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
    roots: Vec<NodeId>,
}

impl Tree {
    pub fn new(max_nodes: usize) -> Self {
        Tree { slots: Vec::new(), free: Vec::new(), live: 0, max_nodes, roots: Vec::new() }
    }

    pub fn live_count(&self) -> usize {
        self.live
    }

    /// Top-level nodes in creation order (drawn first to last).
    pub fn roots(&self) -> &[NodeId] {
        &self.roots
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
        match parent {
            Some(p) => self.node_mut(p).expect("checked above").children.push(id),
            None => self.roots.push(id),
        }
        Ok(id)
    }

    /// Removes the node and its subtree; every id inside becomes `Stale`.
    pub fn remove(&mut self, id: NodeId) -> Result<(), Refused> {
        let parent = self.node(id)?.parent;
        match parent {
            Some(p) => self.node_mut(p)?.children.retain(|c| *c != id),
            None => self.roots.retain(|c| *c != id),
        }
        self.remove_subtree(id);
        Ok(())
    }

    fn remove_subtree(&mut self, id: NodeId) {
        let slot = &mut self.slots[id.idx as usize];
        let Some(node) = slot.node.take() else { return };
        slot.generation = slot.generation.wrapping_add(1);
        self.free.push(id.idx);
        self.live -= 1;
        for c in node.children {
            self.remove_subtree(c);
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

    pub fn get(&self, id: NodeId, prop: Prop) -> Result<Option<&Value>, Refused> {
        Ok(self.node(id)?.props.get(&prop))
    }

    pub fn kind(&self, id: NodeId) -> Result<NodeKind, Refused> {
        Ok(self.node(id)?.kind)
    }

    pub fn children(&self, id: NodeId) -> Result<&[NodeId], Refused> {
        Ok(&self.node(id)?.children)
    }

    pub fn parent(&self, id: NodeId) -> Result<Option<NodeId>, Refused> {
        Ok(self.node(id)?.parent)
    }

    /// Depth-first search under `root` for a node whose `Name` is `name`.
    pub fn find(&self, root: NodeId, name: &str) -> Option<NodeId> {
        let node = self.node(root).ok()?;
        if node.text_prop(Prop::Name) == Some(name) {
            return Some(root);
        }
        node.children.iter().find_map(|c| self.find(*c, name))
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

    /// Sum of `OffsetX`/`OffsetY` over the node and its ancestors (§9.2).
    pub(crate) fn offset(&self, id: NodeId) -> (i32, i32) {
        let (mut dx, mut dy) = (0, 0);
        let mut cur = Some(id);
        while let Some(c) = cur {
            let Ok(n) = self.node(c) else { break };
            dx += n.int_prop(Prop::OffsetX, 0);
            dy += n.int_prop(Prop::OffsetY, 0);
            cur = n.parent;
        }
        (dx, dy)
    }

    /// Screen rect: the node's `Rect` moved by the offsets on its path.
    pub fn screen_rect(&self, id: NodeId) -> Result<Rect, Refused> {
        let r = self.node(id)?.rect();
        let (dx, dy) = self.offset(id);
        Ok(r.translate(dx, dy))
    }

    /// Screen area the pointer hits: `HitRect` when set, else the `Rect`.
    pub fn screen_hit_rect(&self, id: NodeId) -> Result<Rect, Refused> {
        match self.node(id)?.props.get(&Prop::HitRect) {
            Some(Value::Rect(r)) => {
                let (dx, dy) = self.offset(id);
                Ok(r.translate(dx, dy))
            }
            _ => self.screen_rect(id),
        }
    }

    /// Pre-order walk in draw order (parents before children, roots in order).
    pub(crate) fn draw_order(&self) -> Vec<NodeId> {
        let mut out = Vec::with_capacity(self.live);
        let mut stack: Vec<NodeId> = self.roots.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            out.push(id);
            if let Ok(n) = self.node(id) {
                stack.extend(n.children.iter().rev().copied());
            }
        }
        out
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
    fn wrong_value_type_leaves_old_value() {
        let mut t = Tree::new(8);
        let b = t.create(NodeKind::Button, None).unwrap();
        t.set(b, Prop::Label, Value::Text("GUI:Ok".into())).unwrap();
        assert_eq!(t.set(b, Prop::Label, Value::Int(1)), Err(Refused::WrongKind));
        assert_eq!(t.get(b, Prop::Label).unwrap(), Some(&Value::Text("GUI:Ok".into())));
    }

    #[test]
    fn leaves_hold_no_children_and_command_is_button_only() {
        let mut t = Tree::new(8);
        let text = t.create(NodeKind::Text, None).unwrap();
        assert_eq!(t.create(NodeKind::Button, Some(text)), Err(Refused::WrongKind));
        assert_eq!(t.set(text, Prop::Command, Value::Text("x".into())), Err(Refused::WrongKind));
    }

    #[test]
    fn limit_is_enforced() {
        let mut t = Tree::new(1);
        t.create(NodeKind::Panel, None).unwrap();
        assert_eq!(t.create(NodeKind::Panel, None), Err(Refused::Limit));
    }
}
