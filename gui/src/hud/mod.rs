//! A retained heads-up display: layered node trees over a world the host
//! draws, plus status templates evaluated per object in that world
//! ([`world`]).
//!
//! Everything a host passes in or reads out is plain data: handles, integers,
//! strings and colours. No callbacks are kept and no GPU type appears, so a
//! bridge (a script or a sandboxed module) can forward the calls unchanged.
//!
//! The HUD is no security boundary. Such a bridge keeps its own handle table,
//! checks [`Hud::origin`] before a change, undoes its changes to others'
//! items, expects [`Hud::remove_origin`] to take others' nodes beneath its
//! own, and numbers the enums for its wire.
//!
//! A frame: the host's changes, [`Hud::layout`], [`Hud::paint`].

#[cfg(test)]
mod contract_tests;
#[cfg(test)]
mod client_bridge_tests;
mod layout;
mod paint;
pub mod world;

use std::collections::HashMap;

use crate::node::{NodeId, NodeKind, Prop, Refused, Tree, Value};
use crate::Rect;

pub use paint::{PaintCmd, PaintOp};

/// Empties `buffer` for the next frame, free of what the last frame's
/// commands borrowed. It keeps the allocation only where `A` and `B` have
/// one layout, as when they differ in lifetimes alone.
pub fn recycle<A, B>(mut buffer: Vec<A>) -> Vec<B> {
    buffer.clear();
    // An empty same-layout vector collects in place.
    buffer.into_iter().filter_map(|_| None).collect()
}

/// Bottom to top.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    Field,
    Panels,
    Overlay,
    Modal,
    /// The host's own emergency screens.
    System,
}

impl Layer {
    /// Only the host enters `Modal` and `System`.
    pub fn host_only(self) -> bool {
        self >= Layer::Modal
    }

    pub const ALL: [Layer; 5] = [Layer::Field, Layer::Panels, Layer::Overlay, Layer::Modal, Layer::System];
}

/// Who made a node, template or draw list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Origin {
    Host,
    Layout,
    Player,
    External(u16),
}

/// What others than the host share of a limit: a quarter, rounded up, is
/// the host's alone.
fn others_share(limit: usize) -> usize {
    limit - limit.div_ceil(4)
}

impl Origin {
    fn share(self, limit: usize) -> usize {
        if self == Origin::Host {
            limit
        } else {
            others_share(limit)
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Meta {
    pub origin: Origin,
    pub layer: Layer,
}

pub struct Hud {
    pub(crate) tree: Tree,
    pub(crate) meta: HashMap<NodeId, Meta>,
    /// Each layer's roots, bottom to top.
    pub(crate) roots: [Vec<NodeId>; 5],
    pub(crate) layout: layout::Layout,
    revision: u64,
}

impl Hud {
    pub fn new(max_nodes: usize) -> Self {
        Hud {
            tree: Tree::new(max_nodes),
            meta: HashMap::new(),
            roots: Default::default(),
            layout: Default::default(),
            revision: 0,
        }
    }

    /// Changes whenever a node changes, so the host knows when to lay out
    /// again.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    /// A top-level node, above the layer's earlier ones.
    pub fn create_root(&mut self, layer: Layer, origin: Origin, kind: NodeKind) -> Result<NodeId, Refused> {
        self.admit(layer, origin)?;
        let id = self.tree.create(kind, None)?;
        self.meta.insert(id, Meta { origin, layer });
        self.roots[layer as usize].push(id);
        self.revision += 1;
        Ok(id)
    }

    /// A child, above its earlier siblings, in its parent's layer.
    pub fn create(&mut self, kind: NodeKind, parent: NodeId, origin: Origin) -> Result<NodeId, Refused> {
        self.tree.node(parent)?;
        let layer = self.meta.get(&parent).ok_or(Refused::NotFound)?.layer;
        self.admit(layer, origin)?;
        let id = self.tree.create(kind, Some(parent))?;
        self.meta.insert(id, Meta { origin, layer });
        self.revision += 1;
        Ok(id)
    }

    /// Others than the host get three quarters of the node limit between them.
    fn admit(&self, layer: Layer, origin: Origin) -> Result<(), Refused> {
        if origin == Origin::Host {
            return Ok(());
        }
        if layer.host_only() {
            return Err(Refused::Denied);
        }
        if self.tree.live_count() >= others_share(self.tree.max_nodes()) {
            return Err(Refused::Limit);
        }
        Ok(())
    }

    /// Back to the prop's default.
    pub fn clear(&mut self, id: NodeId, prop: Prop) -> Result<(), Refused> {
        if self.tree.clear(id, prop)? {
            self.revision += 1;
        }
        Ok(())
    }

    /// Geometry takes effect at the next [`Self::layout`].
    pub fn set(&mut self, id: NodeId, prop: Prop, value: Value) -> Result<(), Refused> {
        if self.tree.get(id, prop)? == Some(&value) {
            return Ok(());
        }
        self.tree.set(id, prop, value)?;
        self.revision += 1;
        Ok(())
    }

    pub fn get(&self, id: NodeId, prop: Prop) -> Result<Option<&Value>, Refused> {
        self.tree.get(id, prop)
    }

    pub fn origin(&self, id: NodeId) -> Result<Origin, Refused> {
        self.tree.node(id)?;
        Ok(self.meta.get(&id).ok_or(Refused::NotFound)?.origin)
    }

    pub fn layer(&self, id: NodeId) -> Result<Layer, Refused> {
        self.tree.node(id)?;
        Ok(self.meta.get(&id).ok_or(Refused::NotFound)?.layer)
    }

    /// Removes the node and its subtree.
    pub fn remove(&mut self, id: NodeId) -> Result<(), Refused> {
        self.tree.remove(id)?;
        self.forget_removed();
        Ok(())
    }

    /// Removes the nodes with their subtrees at once, in the order given;
    /// those already gone are passed over.
    pub fn remove_all(&mut self, ids: &[NodeId]) {
        if !ids.is_empty() {
            self.tree.remove_all(ids);
            self.forget_removed();
        }
    }

    /// Removes the nodes `origin` made, with their subtrees. What others made
    /// beneath them goes too, so their handles to it become `Stale`.
    pub fn remove_origin(&mut self, origin: Origin) {
        // In paint order, not the map's, so the ids given out next are the same in every run.
        let mut made = Vec::new();
        self.visit(|id| {
            if self.meta.get(&id).is_some_and(|m| m.origin == origin) {
                made.push(id);
            }
        });
        if !made.is_empty() {
            self.tree.remove_all(&made);
            self.forget_removed();
        }
    }

    /// Drops the bookkeeping of nodes the tree no longer has.
    fn forget_removed(&mut self) {
        let tree = &self.tree;
        self.meta.retain(|id, _| tree.node(*id).is_ok());
        for roots in &mut self.roots {
            roots.retain(|id| tree.node(*id).is_ok());
        }
        self.revision += 1;
    }

    /// The node named `name` (ASCII case aside): `from`'s own first, then
    /// the host's, the layout's, the player's and the rest; the first in
    /// paint order among one origin's. Only the host finds what is in a
    /// host-only layer.
    pub fn find(&self, from: Origin, name: &str) -> Option<NodeId> {
        let mut best: Option<(bool, Origin, NodeId)> = None;
        self.visit(|id| {
            let Ok(node) = self.tree.node(id) else { return };
            if !node.text_prop(Prop::Name).is_some_and(|n| n.eq_ignore_ascii_case(name)) {
                return;
            }
            let Some(meta) = self.meta.get(&id) else { return };
            if meta.layer.host_only() && from != Origin::Host {
                return;
            }
            let foreign = meta.origin != from;
            if best.is_none_or(|(f, o, _)| (foreign, meta.origin) < (f, o)) {
                best = Some((foreign, meta.origin, id));
            }
        });
        best.map(|(_, _, id)| id)
    }

    /// Every node in paint order, parents before children. No recursion: a
    /// caller may nest as deep as the node limit.
    pub(crate) fn visit(&self, mut f: impl FnMut(NodeId)) {
        let mut stack: Vec<NodeId> = Vec::new();
        for roots in &self.roots {
            for root in roots {
                stack.push(*root);
                while let Some(id) = stack.pop() {
                    f(id);
                    if let Ok(children) = self.tree.children(id) {
                        stack.extend(children.iter().rev());
                    }
                }
            }
        }
    }

    /// The rect from the last [`Self::layout`]; none while hidden or removed,
    /// also if only since then.
    pub fn rect(&self, id: NodeId) -> Option<Rect> {
        if !self.tree.effective(id, Prop::Visible) {
            return None;
        }
        self.layout.placed(id).map(|p| p.rect)
    }
}
