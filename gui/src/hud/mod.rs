//! A retained heads-up display: layered node trees over a world the host
//! draws, plus status templates and draw lists anchored in that world.
//!
//! Everything a host passes in or reads out is plain data: handles, integers,
//! strings and colours. No callbacks are kept and no GPU type appears, so a
//! bridge (a script or a sandboxed module) can forward the calls unchanged.
//!
//! The HUD is no security boundary. Such a bridge keeps its own handle table,
//! checks [`Hud::origin`] (and [`world::World::template_origin`]) before a
//! change, undoes its changes to others' items, and numbers the enums for its
//! wire. Events may name a node removed since they were queued.
//!
//! A frame: [`Hud::layout`], [`Hud::input`] against that layout,
//! [`Hud::drain_events`], the host's changes, [`Hud::layout`], [`Hud::paint`].

#[cfg(test)]
mod contract_tests;
mod input;
mod layout;
mod paint;
pub mod world;

use std::collections::HashMap;

use crate::node::{NodeId, NodeKind, Prop, Refused, Tree, Value};
use crate::Rect;

pub use input::{ClickOrder, HoverOrder, HudEvent, InputPolicy, Pointer, Response};
pub use layout::Hold;
pub use paint::{ButtonState, PaintCmd, PaintOp, batches};

/// Empties `buffer` for the next frame and keeps its allocation, free of what
/// the last frame's commands borrowed.
pub fn recycle<A, B>(mut buffer: Vec<A>) -> Vec<B> {
    buffer.clear();
    // An empty same-layout vector collects in place.
    buffer.into_iter().filter_map(|_| None).collect()
}

/// Bottom to top. A visible node in `Modal` blocks every layer beneath it.
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
    /// `Modal` and `System` take the whole pointer, so only the host enters them.
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
    pub(crate) input: input::State,
    pub world: world::World,
    revision: u64,
}

impl Hud {
    pub fn new(max_nodes: usize, policy: InputPolicy) -> Self {
        Hud {
            tree: Tree::new(max_nodes),
            meta: HashMap::new(),
            roots: Default::default(),
            layout: Default::default(),
            input: input::State::new(policy),
            world: Default::default(),
            revision: 0,
        }
    }

    /// Changes whenever a node changes; with [`world::World::revision`] it
    /// tells the host whether a still picture needs a redraw.
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
        let max = self.tree.max_nodes();
        if self.tree.live_count() >= max - max / 4 {
            return Err(Refused::Limit);
        }
        Ok(())
    }

    /// Back to the prop's default.
    pub fn clear(&mut self, id: NodeId, prop: Prop) -> Result<(), Refused> {
        if self.tree.clear(id, prop)? {
            self.revision += 1;
            self.forget_inactive();
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
        if matches!(prop, Prop::Visible | Prop::Enabled) {
            self.forget_inactive();
        }
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
        let tree = &self.tree;
        self.meta.retain(|id, _| tree.node(*id).is_ok());
        for roots in &mut self.roots {
            roots.retain(|id| tree.node(*id).is_ok());
        }
        self.revision += 1;
        self.forget_inactive();
        Ok(())
    }

    /// Removes all that `origin` made: its nodes with their subtrees, its
    /// templates and its draw lists.
    pub fn remove_origin(&mut self, origin: Origin) {
        let made: Vec<NodeId> = self.meta.iter().filter(|(_, m)| m.origin == origin).map(|(id, _)| *id).collect();
        let mut changed = false;
        for id in made {
            // An earlier removal may have taken it with a parent's subtree.
            changed |= self.tree.remove(id).is_ok();
        }
        if changed {
            let tree = &self.tree;
            self.meta.retain(|id, _| tree.node(*id).is_ok());
            for roots in &mut self.roots {
                roots.retain(|id| tree.node(*id).is_ok());
            }
            self.revision += 1;
            self.forget_inactive();
        }
        self.world.remove_origin(origin);
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

    /// The rect from the last [`Self::layout`]; none while hidden or removed.
    pub fn rect(&self, id: NodeId) -> Option<Rect> {
        self.tree.node(id).ok()?;
        self.layout.placed(id).map(|p| p.rect)
    }
}
