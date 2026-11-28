//! The one retained-node adapter over a `vera20k-gui` HUD whose host already
//! knows where each piece lands.
//!
//! The battlefield's text overlays, the sidebar's chrome, the command bar,
//! the minimap's area, the layout's added parts and the performance panel
//! each hand over a frame of pieces (fills, texts, pictures, host areas) in
//! drawing order. Here each piece is a node of a tree kept from frame to
//! frame: the frame's `n`th piece reuses last frame's `n`th node where that
//! is of its kind, under the same part, else it and every node after it
//! are made anew, as a newer root is on top. A node changes only where its
//! piece does, so a still frame costs a compare per piece and no layout.
//! The host hands each node its exact place on the target ([`NodeDraw`]),
//! so the instances are the ones the host drew itself.
//!
//! A HUD may also draw into a tree it does not own: the match's GUI, whose
//! chrome layer holds the sidebar's and the command bar's buttons, lends
//! itself to a frame ([`RetainedHud::begin_in`], [`RetainedHud::finish_in`]).
//! There the parts are the chrome's own nodes ([`RetainedHud::part_node`]),
//! a button is drawn by its own node ([`RetainedHud::adopt`]), and the frame
//! paints those parts alone, in the order it drew them.

use vera20k_gui::{Gui, Layer, NodeId, NodeKind, Origin, Prop, Rect, Rgba, Value};

use crate::render::bit_font::BitFont;
use crate::render::hud_paint::{HudPaint, HudSpace, NodeDraw};

/// Where the tree lays out: everywhere, as the host placed every piece and
/// what is off screen is the lanes' to clip.
const UNBOUNDED: Rect = Rect::new(i32::MIN / 2, i32::MIN / 2, i32::MAX, i32::MAX);

/// The `Art` every picture node names, and every chrome button that draws
/// its own picture; which picture it is, its [`NodeDraw`] says.
pub(crate) const PICTURE: &str = "picture";

/// What becomes of the nodes a frame no longer reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rest {
    /// Removed with their subtrees.
    Remove,
    /// Hidden, to show again when a frame reaches them.
    Hide,
}

/// The pieces' tree, kept from frame to frame, and the instances it draws.
pub(crate) struct RetainedHud {
    hud: Gui,
    /// The layer every root goes into and who makes the nodes.
    layer: Layer,
    origin: Origin,
    rest: Rest,
    /// Runs of nodes, each with its own order, so that a host can add to
    /// two (the panel's boxes under all its text) at once. A run after the
    /// first starts with a part of its own.
    runs: Vec<Run>,
    /// The run the next piece goes into.
    run: usize,
    /// The tree's revision at its last layout.
    laid_out: Option<u64>,
    /// The viewport of the tree's last layout, which a lent tree's other
    /// users may have changed.
    viewport: Option<Rect>,
    /// The nodes to remove at once, kept for its allocation.
    removed: Vec<NodeId>,
    paint: HudPaint,
    /// Whether the tree is lent to each frame ([`Self::lent`]); `hud` holds
    /// an empty one between frames.
    lent: bool,
    /// Which lent tree the entries are nodes of.
    tree: Option<u64>,
}

#[derive(Default)]
struct Run {
    /// The part and piece nodes, in drawing order.
    entries: Vec<Entry>,
    /// How many of `entries` this frame has reached.
    used: usize,
    /// The part this frame's pieces go into, from [`RetainedHud::part`].
    part: Option<NodeId>,
}

/// One node and what it was last set to, so that a still piece costs a
/// compare.
struct Entry {
    node: NodeId,
    kind: NodeKind,
    /// A part's or an area's name.
    name: Option<&'static str>,
    /// The part it is in.
    parent: Option<NodeId>,
    shown: bool,
    /// A node of the lent tree's own ([`RetainedHud::adopt`]): never removed
    /// nor hidden here.
    adopted: bool,
    rect: Option<Rect>,
    color: Option<Rgba>,
    text: String,
    /// What the tree cannot hold of it this frame.
    draw: NodeDraw,
}

impl RetainedHud {
    /// A tree of at most `limit` nodes, its roots in `layer` and made by
    /// `origin`, with `runs` runs (at least one).
    pub(crate) fn new(layer: Layer, origin: Origin, limit: usize, runs: usize, rest: Rest) -> Self {
        Self {
            hud: Gui::new(limit),
            layer,
            origin,
            rest,
            runs: (0..runs.max(1)).map(|_| Run::default()).collect(),
            run: 0,
            laid_out: None,
            viewport: None,
            removed: Vec::new(),
            paint: HudPaint::default(),
            lent: false,
            tree: None,
        }
    }

    /// A HUD that draws into a tree lent to each frame ([`Self::begin_in`]),
    /// its nodes made by `origin`, with `runs` runs. Every piece of it goes
    /// into a part of that tree ([`Self::part_node`]); one outside any is
    /// refused.
    pub(crate) fn lent(origin: Origin, runs: usize, rest: Rest) -> Self {
        Self {
            lent: true,
            ..Self::new(Layer::Panels, origin, 0, runs, rest)
        }
    }

    /// Start a frame in `gui`, lent to it until [`Self::finish_in`] hands it
    /// back: whoever reads `gui` meanwhile finds an empty tree. `tree` names
    /// the nodes the frame draws into (a chrome built anew is another): the
    /// last frame's nodes, of an older one, are forgotten without a change.
    pub(crate) fn begin_in(&mut self, gui: &mut Gui, tree: u64) {
        debug_assert!(self.lent, "a HUD with a tree of its own");
        std::mem::swap(&mut self.hud, gui);
        if self.tree != Some(tree) {
            for run in &mut self.runs {
                run.entries.clear();
            }
            self.laid_out = None;
            self.tree = Some(tree);
        }
        self.begin();
    }

    /// Start a frame: the pieces added from here replace the last frame's,
    /// into the first run and no part.
    pub(crate) fn begin(&mut self) {
        for run in &mut self.runs {
            run.used = 0;
            run.part = None;
        }
        self.run = 0;
    }

    /// Add the next pieces to `run`.
    pub(crate) fn select(&mut self, run: usize) {
        self.run = run;
    }

    /// Start a part (`name` its `veraui.ini` key and the node's `Name`, if
    /// it has one) in the run: the pieces added from here until the next
    /// part are its children, so the part can be found by its name and
    /// hidden or restyled as one. A part is a root and draws nothing
    /// itself; its pieces keep their places and order. Refused, the pieces
    /// are roots.
    pub(crate) fn part(&mut self, name: Option<&'static str>) {
        debug_assert!(!self.lent, "a lent tree's parts are its own");
        self.runs[self.run].part = None;
        let part = self
            .next(NodeKind::Panel, name, None, NodeDraw::default())
            .map(|at| self.runs[self.run].entries[at].node);
        self.runs[self.run].part = part;
    }

    /// Start the part `part`, a node of the lent tree, in the run: the
    /// pieces added from here until the next part are its children. None
    /// refuses them.
    pub(crate) fn part_node(&mut self, part: Option<NodeId>) {
        self.runs[self.run].part = part;
    }

    /// The frame's next piece is `node`, the lent tree's own (a button of
    /// the chrome, which takes the pointer there): moved to `rect` (none
    /// keeps its place) and drawn as `draw`, which a frame that draws
    /// nothing of it leaves at its default. The node stays in its tree and
    /// shown whatever the frames do; this HUD changes its `Rect` alone.
    pub(crate) fn adopt(&mut self, node: NodeId, rect: Option<Rect>, draw: NodeDraw) {
        let run = self.run;
        let used = self.runs[run].used;
        let same = self.runs[run]
            .entries
            .get(used)
            .is_some_and(|entry| entry.adopted && entry.node == node);
        if !same {
            self.forget_from(run, used);
            self.runs[run].entries.push(Entry {
                node,
                kind: NodeKind::Button,
                name: None,
                parent: None,
                shown: true,
                adopted: true,
                rect: None,
                color: None,
                text: String::new(),
                draw,
            });
        }
        self.runs[run].used += 1;
        let entry = &mut self.runs[run].entries[used];
        entry.draw = draw;
        if let Some(rect) = rect {
            let rect = Rect::new(rect.x, rect.y, rect.w.max(1), rect.h.max(1));
            if entry.rect != Some(rect) {
                entry.rect = Some(rect);
                let _ = self.hud.set(entry.node, Prop::Rect, Value::Rect(rect));
            }
        }
    }

    /// The frame's next box: `rect` (pixels in the tree) filled with
    /// `color`, drawn as `draw` says.
    pub(crate) fn fill(&mut self, rect: Rect, color: Rgba, draw: NodeDraw) {
        if let Some(at) = self.next(NodeKind::Panel, None, Some(rect), draw) {
            self.color(at, Prop::Background, color);
        }
    }

    /// The frame's next text: `text` in `color` from `rect`'s corner,
    /// drawn as `draw` says.
    pub(crate) fn text(&mut self, rect: Rect, text: &str, color: Rgba, draw: NodeDraw) {
        let Some(at) = self.next(NodeKind::Text, None, Some(rect), draw) else {
            return;
        };
        self.color(at, Prop::Color, color);
        let entry = &mut self.runs[self.run].entries[at];
        if entry.text != text {
            text.clone_into(&mut entry.text);
            let _ = self
                .hud
                .set(entry.node, Prop::Text, Value::Text(text.to_owned()));
        }
    }

    /// The frame's next picture over `rect`, its picture or instance in
    /// `draw`.
    pub(crate) fn picture(&mut self, rect: Rect, draw: NodeDraw) {
        self.next(NodeKind::Image, None, Some(rect), draw);
    }

    /// The frame's next host area `name` (its `Name` and `Art`) over
    /// `rect`, what the host drew there in `draw`.
    pub(crate) fn area(&mut self, name: &'static str, rect: Rect, draw: NodeDraw) {
        self.next(NodeKind::External, Some(name), Some(rect), draw);
    }

    /// End the frame: drop the nodes it no longer has, lay out where the
    /// tree changed and draw the rest through `space`, the text with `font`
    /// at its raster of now (none draws no text).
    pub(crate) fn finish(&mut self, space: HudSpace, font: Option<&BitFont>) -> &HudPaint {
        debug_assert!(!self.lent, "a lent tree is handed back by finish_in");
        self.settle();
        // The paint list names the shown nodes in the runs' order.
        let runs = &self.runs;
        let mut at = 0;
        self.paint
            .build(&self.hud, space, font, |node| seek(runs, &mut at, node));
        &self.paint
    }

    /// [`Self::finish`] of a frame begun by [`Self::begin_in`]: drawn from
    /// the subtrees of `from` alone, one after the other in that order (the
    /// parts the frame drew into, not the rest of the lent tree, whose own
    /// order is its input's), and the tree handed back to `gui`.
    pub(crate) fn finish_in(
        &mut self,
        gui: &mut Gui,
        from: &[NodeId],
        space: HudSpace,
        font: Option<&BitFont>,
    ) -> &HudPaint {
        self.settle();
        let runs = &self.runs;
        let mut at = 0;
        self.paint
            .build_from(&self.hud, Some(from), space, font, |node| {
                seek(runs, &mut at, node)
            });
        std::mem::swap(&mut self.hud, gui);
        &self.paint
    }

    /// Drop or hide the nodes the frame no longer has, and lay out where
    /// the tree changed or another user laid it out elsewhere.
    fn settle(&mut self) {
        for run in 0..self.runs.len() {
            let used = self.runs[run].used;
            match self.rest {
                Rest::Remove => self.forget_from(run, used),
                Rest::Hide => {
                    for entry in &mut self.runs[run].entries[used..] {
                        if entry.shown && !entry.adopted {
                            entry.shown = false;
                            let _ = self.hud.set(entry.node, Prop::Visible, Value::Bool(false));
                        }
                    }
                }
            }
        }
        let revision = self.hud.revision();
        if self.laid_out != Some(revision) || self.viewport != Some(self.hud.viewport()) {
            self.hud.layout(UNBOUNDED);
            self.laid_out = Some(revision);
            self.viewport = Some(self.hud.viewport());
        }
    }

    /// The last [`Self::finish`]'s instances.
    pub(crate) fn painted(&self) -> &HudPaint {
        &self.paint
    }

    /// The node named `name`, while this frame has it.
    #[cfg(test)]
    pub(crate) fn find(&self, name: &str) -> Option<NodeId> {
        self.hud.find(self.origin, name)
    }

    /// The tree, for a test that hides or restyles a part.
    #[cfg(test)]
    pub(crate) fn hud_mut(&mut self) -> &mut Gui {
        &mut self.hud
    }

    /// The pieces' rects, bottom to top.
    #[cfg(test)]
    pub(crate) fn rects(&self) -> Vec<Rect> {
        self.runs
            .iter()
            .flat_map(|run| &run.entries[..run.used])
            .filter_map(|entry| entry.rect)
            .collect()
    }

    /// The frame's next node of `kind` (named `name`) in the run's part,
    /// at `rect`: last frame's where it is one, else a new one in its
    /// place and those after it. Its rect is never empty, which the HUD
    /// would not paint. Its index in the run's entries.
    fn next(
        &mut self,
        kind: NodeKind,
        name: Option<&'static str>,
        rect: Option<Rect>,
        draw: NodeDraw,
    ) -> Option<usize> {
        let run = self.run;
        let (used, parent) = (self.runs[run].used, self.runs[run].part);
        // A lent tree takes pieces inside its parts alone.
        if self.lent && parent.is_none() {
            return None;
        }
        let same = self.runs[run].entries.get(used).is_some_and(|entry| {
            !entry.adopted && entry.kind == kind && entry.name == name && entry.parent == parent
        });
        if !same {
            self.forget_from(run, used);
            let node = match parent {
                Some(part) => self.hud.create_by(kind, Some(part), self.origin).ok()?,
                None => self.hud.create_root(self.layer, self.origin, kind).ok()?,
            };
            if let Some(name) = name {
                let _ = self.hud.set(node, Prop::Name, Value::Text(name.to_owned()));
            }
            let art = match kind {
                NodeKind::Image => Some(PICTURE),
                NodeKind::External => name,
                _ => None,
            };
            if let Some(art) = art {
                let _ = self.hud.set(node, Prop::Art, Value::Text(art.to_owned()));
            }
            self.runs[run].entries.push(Entry {
                node,
                kind,
                name,
                parent,
                shown: true,
                adopted: false,
                rect: None,
                color: None,
                text: String::new(),
                draw,
            });
        }
        self.runs[run].used += 1;
        let entry = &mut self.runs[run].entries[used];
        entry.draw = draw;
        if !entry.shown {
            entry.shown = true;
            let _ = self.hud.set(entry.node, Prop::Visible, Value::Bool(true));
        }
        if let Some(rect) = rect {
            let rect = Rect::new(rect.x, rect.y, rect.w.max(1), rect.h.max(1));
            if entry.rect != Some(rect) {
                entry.rect = Some(rect);
                let _ = self.hud.set(entry.node, Prop::Rect, Value::Rect(rect));
            }
        }
        Some(used)
    }

    fn color(&mut self, at: usize, prop: Prop, color: Rgba) {
        let entry = &mut self.runs[self.run].entries[at];
        if entry.color != Some(color) {
            entry.color = Some(color);
            let _ = self.hud.set(entry.node, prop, Value::Rgba(color));
        }
    }

    /// Remove the run's nodes from `kept` on, at once. A removed part takes
    /// its pieces with it; theirs are then gone. An adopted node is only
    /// forgotten: it stays in its tree, and draws nothing until a frame
    /// adopts it again ([`seek`] gives it the default draw).
    fn forget_from(&mut self, run: usize, kept: usize) {
        let entries = &mut self.runs[run].entries;
        if kept >= entries.len() {
            return;
        }
        self.removed.clear();
        self.removed.extend(
            entries[kept..]
                .iter()
                .filter(|entry| !entry.adopted)
                .map(|entry| entry.node),
        );
        entries.truncate(kept);
        self.hud.remove_all(&self.removed);
    }
}

/// The draw of `node`, looked for from `at`, where the last one was: the
/// paint list names the shown nodes in the runs' order, so each is found
/// in a step or a few. A node no run has draws as the HUD says.
fn seek(runs: &[Run], at: &mut usize, node: NodeId) -> NodeDraw {
    let total: usize = runs.iter().map(|run| run.used).sum();
    for step in 0..total {
        let index = (*at + step) % total;
        let entry = entry_at(runs, index);
        if entry.node == node {
            *at = index;
            return entry.draw;
        }
    }
    NodeDraw::default()
}

/// The `index`th shown entry of the runs, one after the other.
fn entry_at(runs: &[Run], mut index: usize) -> &Entry {
    for run in runs {
        if index < run.used {
            return &run.entries[index];
        }
        index -= run.used;
    }
    unreachable!("an index below the runs' total")
}

/// The nearest opaque colour to a tint, for a node's prop.
pub(crate) fn rounded(tint: [f32; 3]) -> Rgba {
    let [r, g, b] = tint.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8);
    Rgba(r, g, b, 255)
}

/// An opaque colour from whole 255ths.
pub(crate) fn rgb(channels: [u8; 3]) -> Rgba {
    Rgba(channels[0], channels[1], channels[2], 255)
}
