//! The layers a [`Gui`]'s roots paint in, who made each node, and the
//! revisions that tell a layout what changed.
//!
//! Everything a host passes in or reads out is plain data: handles, integers,
//! strings and colours. No callbacks are kept and no GPU type appears, so a
//! bridge (a script or a sandboxed module) can forward the calls unchanged.
//!
//! The GUI is no security boundary. Such a bridge keeps its own handle table,
//! checks [`Gui::origin`] before a change, undoes its changes to others'
//! items, expects [`Gui::remove_origin`] to take others' nodes beneath its
//! own, and numbers the enums for its wire.

use crate::node::{NodeId, NodeKind, Prop, Refused};
use crate::origin::{Origin, others_share};

use super::Gui;

/// Bottom to top.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    Field,
    Panels,
    Overlay,
    /// Where a root made by plain [`Gui::create`] goes: the host's dialogs
    /// and chrome. Their input and paint order is the dialog stack's.
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

#[derive(Clone, Copy, Debug)]
pub(crate) struct Meta {
    pub origin: Origin,
    pub layer: Layer,
    /// The top-level node it is under (itself for one): nodes never move,
    /// so a change finds the revision to move without climbing.
    pub root: NodeId,
    /// A root's: the [`Gui::revision`] its subtree last changed at.
    pub revision: u64,
}

impl Gui {
    /// Changes whenever a node changes, so the host knows when to lay out
    /// again.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The revision `id`'s root last changed at: a layout over that root
    /// runs again only once it moves.
    pub fn root_revision(&self, id: NodeId) -> Result<u64, Refused> {
        self.tree.node(id)?;
        let root = self.meta.get(&id).ok_or(Refused::NotFound)?.root;
        Ok(self.meta.get(&root).ok_or(Refused::NotFound)?.revision)
    }

    /// A top-level node, above the layer's earlier ones.
    pub fn create_root(&mut self, layer: Layer, origin: Origin, kind: NodeKind) -> Result<NodeId, Refused> {
        self.admit(layer, origin)?;
        let id = self.tree.create(kind, None)?;
        self.revision += 1;
        self.meta.insert(id, Meta { origin, layer, root: id, revision: self.revision });
        self.roots[layer as usize].push(id);
        Ok(id)
    }

    /// A node made by `origin`: under `parent`, above its earlier children
    /// and in its layer; without one a root in [`Layer::Modal`].
    pub fn create_by(&mut self, kind: NodeKind, parent: Option<NodeId>, origin: Origin) -> Result<NodeId, Refused> {
        let Some(parent) = parent else {
            return self.create_root(Layer::Modal, origin, kind);
        };
        self.tree.node(parent)?;
        let Meta { layer, root, .. } = *self.meta.get(&parent).ok_or(Refused::NotFound)?;
        self.admit(layer, origin)?;
        let id = self.tree.create(kind, Some(parent))?;
        self.meta.insert(id, Meta { origin, layer, root, revision: 0 });
        self.touch(id);
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

    /// Moves the revision and that of `id`'s root.
    pub(crate) fn touch(&mut self, id: NodeId) {
        self.revision += 1;
        let Some(root) = self.meta.get(&id).map(|m| m.root) else { return };
        if let Some(meta) = self.meta.get_mut(&root) {
            meta.revision = self.revision;
        }
    }

    /// Back to the prop's default.
    pub fn clear(&mut self, id: NodeId, prop: Prop) -> Result<(), Refused> {
        if self.tree.clear(id, prop)? {
            self.touch(id);
        }
        Ok(())
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
        self.tree.node(id)?;
        self.remove_all(&[id]);
        Ok(())
    }

    /// Removes the nodes with their subtrees at once, in the order given;
    /// those already gone are passed over. A dialog whose root goes is
    /// popped, and a pointer that held a removed node lets go of it.
    pub fn remove_all(&mut self, ids: &[NodeId]) {
        if ids.is_empty() {
            return;
        }
        // The bookkeeping to drop, gathered while the tree still links it.
        let mut gone = std::mem::take(&mut self.gone);
        gone.clear();
        let mut layers = [false; 5];
        for &id in ids {
            let parent = match self.tree.node(id) {
                Ok(node) => node.parent,
                Err(_) => continue,
            };
            match parent {
                Some(parent) => self.touch(parent),
                None => {
                    if let Some(meta) = self.meta.get(&id) {
                        layers[meta.layer as usize] = true;
                    }
                }
            }
            let mut at = gone.len();
            gone.push(id);
            while at < gone.len() {
                if let Ok(children) = self.tree.children(gone[at]) {
                    gone.extend_from_slice(children);
                }
                at += 1;
            }
        }
        if gone.is_empty() {
            self.gone = gone;
            return;
        }
        match ids {
            [id] => {
                let _ = self.tree.remove(*id);
            }
            _ => self.tree.remove_all(ids),
        }
        self.revision += 1;
        for id in &gone {
            self.meta.remove(id);
        }
        self.gone = gone;
        let tree = &self.tree;
        for (roots, touched) in self.roots.iter_mut().zip(layers) {
            if touched {
                roots.retain(|id| tree.node(*id).is_ok());
            }
        }
        self.prune_dialogs();
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
        self.remove_all(&made);
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

    /// The node named `name` (ASCII case aside) among `origin`'s alone, the
    /// first in paint order, in any layer: a bridge's lookup that walks the
    /// origins in an order of its own (RFC 0001 section 10, "lookup by
    /// name"). It is the host's query: the bridge decides what its client
    /// may reach.
    pub fn find_by(&self, origin: Origin, name: &str) -> Option<NodeId> {
        let mut found = None;
        self.visit(|id| {
            if found.is_some() || self.meta.get(&id).is_none_or(|m| m.origin != origin) {
                return;
            }
            if self.tree.node(id).is_ok_and(|n| n.text_prop(Prop::Name).is_some_and(|n| n.eq_ignore_ascii_case(name))) {
                found = Some(id);
            }
        });
        found
    }

    /// `layer`'s top-level nodes, bottom to top.
    pub fn roots(&self, layer: Layer) -> &[NodeId] {
        &self.roots[layer as usize]
    }

    /// The node limit [`Gui::new`] was given; [`Origin::share`] of it is what
    /// an origin other than the host may hold.
    pub fn node_limit(&self) -> usize {
        self.tree.max_nodes()
    }

    /// Whether the node and every ancestor are `Visible` (unset is shown).
    pub fn shown(&self, id: NodeId) -> bool {
        self.tree.effective(id, Prop::Visible)
    }

    /// Whether the node and every ancestor are `Enabled` (unset is enabled).
    pub fn enabled(&self, id: NodeId) -> bool {
        self.tree.effective(id, Prop::Enabled)
    }

    /// The topmost shown node at `x`, `y` that `accept` takes, over
    /// `layers` given top first: in each, the later root over the earlier
    /// and a child over its parent, as they paint. A node hits in its
    /// `HitRect`, else its `Rect`; a hidden node's subtree never hits. This
    /// is no input routing: a bridge asks it which of its client's nodes a
    /// pointer is over (hover, click, a press outside).
    pub fn top_at(&self, layers: &[Layer], x: i32, y: i32, accept: impl Fn(NodeId) -> bool) -> Option<NodeId> {
        for &layer in layers {
            for &root in self.roots[layer as usize].iter().rev() {
                let mut hit = None;
                self.tree.walk_visible(root, |id| {
                    if self.tree.screen_hit_rect(id).is_ok_and(|r| r.contains(x, y)) && accept(id) {
                        hit = Some(id);
                    }
                });
                if hit.is_some() {
                    return hit;
                }
            }
        }
        None
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
}
