//! Declarative trees (docs/gui-library-api.md §6.1): a screen describes its
//! widgets once as data and `Gui::create_tree` builds them, all or nothing.

use std::ops::ControlFlow;

use crate::gui::Gui;
use crate::node::{NodeId, NodeKind, Prop, Refused, Value};

/// One node and its subtree.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeSpec {
    pub kind: NodeKind,
    pub props: Vec<(Prop, Value)>,
    pub children: Vec<NodeSpec>,
}

impl NodeSpec {
    pub fn new(kind: NodeKind) -> Self {
        NodeSpec { kind, props: Vec::new(), children: Vec::new() }
    }

    pub fn with(mut self, prop: Prop, value: Value) -> Self {
        self.props.push((prop, value));
        self
    }

    /// The host's control id (`Prop::Id`), e.g. a dialog resource id.
    pub fn id(self, id: i32) -> Self {
        self.with(Prop::Id, Value::Int(id))
    }

    /// A CSF key or literal caption.
    pub fn label(self, key: &str) -> Self {
        self.with(Prop::Label, Value::Text(key.to_owned()))
    }

    /// The help text the status line shows while the node is hovered.
    pub fn tooltip(self, key: &str) -> Self {
        self.with(Prop::Tooltip, Value::Text(key.to_owned()))
    }

    pub fn child(mut self, child: NodeSpec) -> Self {
        self.children.push(child);
        self
    }

    pub fn children(mut self, children: impl IntoIterator<Item = NodeSpec>) -> Self {
        self.children.extend(children);
        self
    }
}

impl Gui {
    /// Builds `spec` under `parent`. On any refusal nothing is left behind.
    pub fn create_tree(&mut self, spec: &NodeSpec, parent: Option<NodeId>) -> Result<NodeId, Refused> {
        let root = self.create(spec.kind, parent)?;
        match self.fill(root, spec) {
            Ok(()) => Ok(root),
            Err(e) => {
                let _ = self.remove(root);
                Err(e)
            }
        }
    }

    fn fill(&mut self, node: NodeId, spec: &NodeSpec) -> Result<(), Refused> {
        for (prop, value) in &spec.props {
            self.set(node, *prop, value.clone())?;
        }
        for child in &spec.children {
            let id = self.create(child.kind, Some(node))?;
            self.fill(id, child)?;
        }
        Ok(())
    }

    /// The first node under `root` (itself included, tree order) whose
    /// `Prop::Id` is `id`.
    pub fn find_id(&self, root: NodeId, id: i32) -> Option<NodeId> {
        self.tree()
            .walk(root, &mut |node, _| match self.get(node, Prop::Id).ok().flatten() == Some(&Value::Int(id)) {
                true => ControlFlow::Break(node),
                false => ControlFlow::Continue(()),
            })
            .break_value()
    }

    /// The node's `Prop::Id`.
    pub fn id_of(&self, node: NodeId) -> Option<i32> {
        match self.get(node, Prop::Id).ok().flatten() {
            Some(Value::Int(id)) => Some(*id),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_is_built_whole_and_found_by_id() {
        let mut g = Gui::new(8);
        let spec = NodeSpec::new(NodeKind::Dialog)
            .child(NodeSpec::new(NodeKind::Button).id(0x61F).label("GUI:Options"))
            .child(NodeSpec::new(NodeKind::Checkbox).id(0x5F5).with(Prop::Checked, Value::Bool(true)));
        let root = g.create_tree(&spec, None).unwrap();
        let cb = g.find_id(root, 0x5F5).unwrap();
        assert_eq!(g.get(cb, Prop::Checked).unwrap(), Some(&Value::Bool(true)));
        assert_eq!(g.id_of(g.find_id(root, 0x61F).unwrap()), Some(0x61F));
    }

    #[test]
    fn a_refused_prop_leaves_nothing_behind() {
        let mut g = Gui::new(8);
        let spec = NodeSpec::new(NodeKind::Dialog)
            .child(NodeSpec::new(NodeKind::Text).with(Prop::Toggled, Value::Bool(true)));
        assert_eq!(g.create_tree(&spec, None), Err(Refused::WrongKind));
        assert_eq!(g.tree().live_count(), 0);
    }
}
