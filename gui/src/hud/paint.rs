//! The ordered paint list the host draws.

use crate::Rect;
use crate::node::{Node, NodeId, NodeKind, Prop, Rgba, Value};

use super::Hud;

const WHITE: Rgba = Rgba(255, 255, 255, 255);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintOp<'a> {
    Fill {
        rect: Rect,
        color: Rgba,
    },
    Image {
        rect: Rect,
        art: &'a str,
    },
    /// `key` is a string-table key or literal text; the host resolves it.
    Text {
        rect: Rect,
        key: &'a str,
        color: Option<Rgba>,
        align: i32,
        enabled: bool,
    },
    External {
        rect: Rect,
        key: &'a str,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaintCmd<'a> {
    pub node: NodeId,
    /// None is the whole viewport.
    pub clip: Option<Rect>,
    pub op: PaintOp<'a>,
}

impl Hud {
    /// Appends the last layout's commands to `out`, bottom to top, the order
    /// to draw them in.
    ///
    /// A node gives, in this order and before its children: its
    /// `Background`; its picture (`Image`, `Button`), filled part (`Bar`) or
    /// host area (`External`); its `Text`, or without one its `Label`
    /// (`Text`, `Button`, `Dialog`). What is empty or wholly outside its
    /// clip or the viewport is left out, so text needs a rect with a size,
    /// and so is a fill of alpha 0.
    pub fn paint<'a>(&'a self, out: &mut Vec<PaintCmd<'a>>) {
        let viewport = self.layout.viewport();
        for placed in self.layout.nodes() {
            let (id, rect, clip) = (placed.id, placed.rect, placed.clip);
            let area = clip.map_or(viewport, |clip| clip.intersect(viewport));
            if !shows(rect, area) {
                continue;
            }
            // Removed since the layout.
            let Ok(node) = self.tree.node(id) else {
                continue;
            };
            let mut push = |op| out.push(PaintCmd { node: id, clip, op });

            if let Some(color) = color_prop(node, Prop::Background).filter(has_alpha) {
                push(PaintOp::Fill { rect, color });
            }
            match node.kind {
                NodeKind::Image | NodeKind::Button => {
                    if let Some(art) = node.text_prop(Prop::Art).filter(|art| !art.is_empty()) {
                        push(PaintOp::Image { rect, art });
                    }
                }
                NodeKind::Bar => {
                    let percent = i64::from(node.int_prop(Prop::Fill, 0).clamp(0, 100));
                    let w = (i64::from(rect.w) * percent / 100) as i32;
                    let filled = Rect::new(rect.x, rect.y, w, rect.h);
                    let color = color_prop(node, Prop::Color).unwrap_or(WHITE);
                    if shows(filled, area) && has_alpha(&color) {
                        push(PaintOp::Fill {
                            rect: filled,
                            color,
                        });
                    }
                }
                NodeKind::External => {
                    let key = node.text_prop(Prop::Art).unwrap_or("");
                    push(PaintOp::External { rect, key });
                }
                _ => {}
            }
            if matches!(
                node.kind,
                NodeKind::Text | NodeKind::Button | NodeKind::Dialog
            ) {
                let key = node
                    .text_prop(Prop::Text)
                    .or_else(|| node.text_prop(Prop::Label));
                if let Some(key) = key.filter(|key| !key.is_empty()) {
                    push(PaintOp::Text {
                        rect,
                        key,
                        color: text_color(node, placed.enabled),
                        align: node.int_prop(Prop::Align, 0),
                        enabled: placed.enabled,
                    });
                }
            }
        }
    }
}

fn shows(rect: Rect, area: Rect) -> bool {
    let seen = rect.intersect(area);
    seen.w > 0 && seen.h > 0
}

fn has_alpha(color: &Rgba) -> bool {
    color.3 > 0
}

fn color_prop(node: &Node, prop: Prop) -> Option<Rgba> {
    match node.props.get(&prop) {
        Some(Value::Rgba(color)) => Some(*color),
        _ => None,
    }
}

fn text_color(node: &Node, enabled: bool) -> Option<Rgba> {
    let disabled = if enabled {
        None
    } else {
        color_prop(node, Prop::DisabledColor)
    };
    disabled.or_else(|| color_prop(node, Prop::Color))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hud::{Layer, Origin};

    const VIEW: Rect = Rect::new(0, 0, 100, 100);
    const BOX: Rect = Rect::new(10, 10, 40, 20);
    const INK: Rgba = Rgba(10, 20, 30, 255);
    const FADED: Rgba = Rgba(40, 50, 60, 128);

    fn hud() -> Hud {
        Hud::new(64)
    }

    fn root(hud: &mut Hud, kind: NodeKind, rect: Rect) -> NodeId {
        let id = hud.create_root(Layer::Panels, Origin::Host, kind).unwrap();
        hud.set(id, Prop::Rect, Value::Rect(rect)).unwrap();
        id
    }

    fn child(hud: &mut Hud, parent: NodeId, kind: NodeKind, rect: Rect) -> NodeId {
        let id = hud.create(kind, parent, Origin::Host).unwrap();
        hud.set(id, Prop::Rect, Value::Rect(rect)).unwrap();
        id
    }

    fn text(hud: &mut Hud, id: NodeId, prop: Prop, v: &str) {
        hud.set(id, prop, Value::Text(v.into())).unwrap();
    }

    fn color(hud: &mut Hud, id: NodeId, prop: Prop, v: Rgba) {
        hud.set(id, prop, Value::Rgba(v)).unwrap();
    }

    fn painted(hud: &mut Hud) -> Vec<PaintCmd<'_>> {
        hud.layout(VIEW);
        let mut out = Vec::new();
        hud.paint(&mut out);
        out
    }

    fn name<'a>(op: &PaintOp<'a>) -> &'a str {
        match *op {
            PaintOp::Fill { .. } => "fill",
            PaintOp::Image { art, .. } => art,
            PaintOp::Text { key, .. } | PaintOp::External { key, .. } => key,
        }
    }

    fn label(key: &str, color: Option<Rgba>, enabled: bool) -> PaintOp<'_> {
        PaintOp::Text {
            rect: BOX,
            key,
            color,
            align: 0,
            enabled,
        }
    }

    #[test]
    fn layers_paint_bottom_to_top() {
        let names = ["field", "panels", "overlay", "modal", "system"];
        let mut hud = hud();
        for (layer, key) in Layer::ALL.into_iter().zip(names).rev() {
            let id = hud
                .create_root(layer, Origin::Host, NodeKind::Text)
                .unwrap();
            hud.set(id, Prop::Rect, Value::Rect(BOX)).unwrap();
            text(&mut hud, id, Prop::Text, key);
        }
        let order: Vec<&str> = painted(&mut hud).iter().map(|cmd| name(&cmd.op)).collect();
        assert_eq!(order, names);
    }

    #[test]
    fn text_stays_beneath_a_later_image() {
        let mut hud = hud();
        let dialog = root(&mut hud, NodeKind::Dialog, VIEW);
        color(&mut hud, dialog, Prop::Background, INK);
        text(&mut hud, dialog, Prop::Text, "heading");
        let below = child(&mut hud, dialog, NodeKind::Button, BOX);
        color(&mut hud, below, Prop::Background, FADED);
        text(&mut hud, below, Prop::Art, "plate");
        text(&mut hud, below, Prop::Label, "first");
        let cover = child(&mut hud, dialog, NodeKind::Image, BOX);
        text(&mut hud, cover, Prop::Art, "cover");
        let order: Vec<&str> = painted(&mut hud).iter().map(|cmd| name(&cmd.op)).collect();
        assert_eq!(
            order,
            ["fill", "heading", "fill", "plate", "first", "cover"]
        );
    }

    #[test]
    fn text_comes_before_label() {
        let mut hud = hud();
        let id = root(&mut hud, NodeKind::Text, BOX);
        text(&mut hud, id, Prop::Label, "label");
        assert_eq!(painted(&mut hud)[0].op, label("label", None, true));
        text(&mut hud, id, Prop::Text, "text");
        assert_eq!(painted(&mut hud)[0].op, label("text", None, true));
        text(&mut hud, id, Prop::Text, "");
        assert!(painted(&mut hud).is_empty());
    }

    #[test]
    fn a_disabled_ancestor_disables_the_text() {
        let mut hud = hud();
        let panel = root(&mut hud, NodeKind::Panel, VIEW);
        let button = child(&mut hud, panel, NodeKind::Button, BOX);
        text(&mut hud, button, Prop::Art, "plate");
        text(&mut hud, button, Prop::Text, "go");
        color(&mut hud, button, Prop::Color, INK);
        color(&mut hud, button, Prop::DisabledColor, FADED);

        let image = PaintOp::Image {
            rect: BOX,
            art: "plate",
        };
        let cmds = painted(&mut hud);
        assert_eq!(cmds[0].op, image);
        assert_eq!(cmds[1].op, label("go", Some(INK), true));

        hud.set(panel, Prop::Enabled, Value::Bool(false)).unwrap();
        let cmds = painted(&mut hud);
        assert_eq!(cmds[0].op, image);
        assert_eq!(cmds[1].op, label("go", Some(FADED), false));
    }

    #[test]
    fn a_bar_fills_its_percent_from_the_left() {
        for (percent, w) in [(i32::MIN, 0), (0, 0), (25, 10), (100, 40), (i32::MAX, 40)] {
            let mut hud = hud();
            let bar = root(&mut hud, NodeKind::Bar, BOX);
            color(&mut hud, bar, Prop::Background, INK);
            hud.set(bar, Prop::Fill, Value::Int(percent)).unwrap();
            let ops: Vec<PaintOp<'_>> = painted(&mut hud).iter().map(|cmd| cmd.op).collect();
            let mut expected = vec![PaintOp::Fill {
                rect: BOX,
                color: INK,
            }];
            if w > 0 {
                expected.push(PaintOp::Fill {
                    rect: Rect::new(10, 10, w, 20),
                    color: WHITE,
                });
            }
            assert_eq!(ops, expected, "{percent}");
        }
    }

    #[test]
    fn a_fill_of_alpha_zero_is_left_out() {
        let mut hud = hud();
        let bar = root(&mut hud, NodeKind::Bar, BOX);
        hud.set(bar, Prop::Fill, Value::Int(50)).unwrap();
        color(&mut hud, bar, Prop::Background, Rgba(1, 2, 3, 0));
        color(&mut hud, bar, Prop::Color, Rgba(1, 2, 3, 0));
        assert!(painted(&mut hud).is_empty());
    }

    #[test]
    fn culling_cuts_the_clip_to_the_viewport() {
        let mut hud = hud();
        let window = root(&mut hud, NodeKind::Panel, Rect::new(50, 50, 200, 200));
        hud.set(window, Prop::Clip, Value::Bool(true)).unwrap();
        // Within the clip, beyond the viewport.
        child(
            &mut hud,
            window,
            NodeKind::External,
            Rect::new(120, 60, 5, 5),
        );
        let shown = child(
            &mut hud,
            window,
            NodeKind::External,
            Rect::new(90, 90, 20, 20),
        );
        let nodes: Vec<NodeId> = painted(&mut hud).iter().map(|cmd| cmd.node).collect();
        assert_eq!(nodes, [shown]);
    }

    #[test]
    fn an_external_is_keyed_by_its_art() {
        let mut hud = hud();
        let id = root(&mut hud, NodeKind::External, BOX);
        assert_eq!(
            painted(&mut hud)[0].op,
            PaintOp::External { rect: BOX, key: "" }
        );
        text(&mut hud, id, Prop::Art, "surface");
        assert_eq!(
            painted(&mut hud)[0].op,
            PaintOp::External {
                rect: BOX,
                key: "surface"
            }
        );
    }

    #[test]
    fn empty_and_unseen_nodes_are_culled() {
        let mut hud = hud();
        root(&mut hud, NodeKind::External, Rect::new(100, 0, 5, 5));
        root(&mut hud, NodeKind::External, Rect::new(-5, 0, 5, 5));
        root(&mut hud, NodeKind::External, Rect::new(5, 5, 0, 5));
        root(&mut hud, NodeKind::Image, BOX);
        let window = root(&mut hud, NodeKind::Panel, Rect::new(0, 0, 20, 20));
        hud.set(window, Prop::Clip, Value::Bool(true)).unwrap();
        child(&mut hud, window, NodeKind::External, Rect::new(20, 0, 5, 5));
        // The bar reaches into the clip, its filled part does not.
        let bar = child(&mut hud, window, NodeKind::Bar, Rect::new(-30, 0, 40, 5));
        hud.set(bar, Prop::Fill, Value::Int(50)).unwrap();
        let shown = child(
            &mut hud,
            window,
            NodeKind::External,
            Rect::new(19, 19, 5, 5),
        );

        let cmds = painted(&mut hud);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].node, shown);
        assert_eq!(cmds[0].clip, Some(Rect::new(0, 0, 20, 20)));
    }

    #[test]
    fn paint_appends_and_skips_what_was_removed() {
        let mut hud = hud();
        let gone = root(&mut hud, NodeKind::External, BOX);
        let kept = root(&mut hud, NodeKind::External, BOX);
        hud.layout(VIEW);
        hud.remove(gone).unwrap();
        let before = PaintCmd {
            node: gone,
            clip: None,
            op: label("before", None, true),
        };
        let mut out = vec![before];
        hud.paint(&mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], before);
        assert_eq!(out[1].node, kept);
    }
}
