//! A HUD of atlas pictures whose host already knows each quad.
//!
//! The sidebar's chrome, the bottom command bar, the observer strip and
//! Phobos' column are runs of pictures from one atlas, each placed by the
//! original's own arithmetic. Here they become picture nodes of a retained
//! `vera20k-gui` tree, one per piece, in drawing order and with the piece's
//! rect in its layout space, so the tree owns which pieces stand and their
//! order. Each node's instance is the one its host made, handed to the
//! adapter unchanged, so every instance is what the host drew before.
//!
//! The sidebar's chrome and the command bar draw into the match's chrome
//! layer ([`ChromeHud`]): their parts are its nodes and each button is drawn
//! by the very node that takes its clicks (`input::chrome_input`), so a
//! button is one node, with the rect it covers on its surface (`Rect`) and
//! the rect the cursor hits (`HitRect`). The tree's order is the pointer's
//! (the original gadget list's); the drawing's is the layout's, which the
//! frame paints the parts in.

use vera20k_gui::{Gui, Layer, NodeId, Origin, Rect};

use crate::render::batch::SpriteInstance;
use crate::render::hud_paint::{HudSpace, NodeDraw};
use crate::render::retained_hud::{Rest, RetainedHud};

/// The pieces' tree, kept from frame to frame, and the instances it draws.
pub(crate) struct PictureHud {
    nodes: RetainedHud,
}

impl Default for PictureHud {
    fn default() -> Self {
        Self {
            nodes: RetainedHud::new(Layer::Panels, Origin::Host, usize::MAX, 1, Rest::Remove),
        }
    }
}

/// The host places every quad itself.
const SPACE: HudSpace = HudSpace {
    origin: [0.0; 2],
    scale: 1.0,
};

impl PictureHud {
    /// Start a frame: the pieces [`Self::picture`] adds from here replace
    /// the last frame's.
    pub(crate) fn begin(&mut self) {
        self.nodes.begin();
    }

    /// Start the part `name` (its `veraui.ini` key, the node's `Name`): the
    /// pictures added from here until the next part are its children, so
    /// the part can be found by its name and hidden or restyled as one. A
    /// part draws nothing itself; its pictures keep their places and order.
    pub(crate) fn part(&mut self, name: &'static str) {
        self.nodes.part(Some(name));
    }

    /// The frame's next piece: `rect` in the host's layout space, drawn as
    /// `instance`.
    pub(crate) fn picture(&mut self, rect: Rect, instance: SpriteInstance) {
        let draw = NodeDraw {
            instance: Some(instance),
            ..Default::default()
        };
        self.nodes.picture(rect, draw);
    }

    /// End the frame: drop the pieces it no longer has and draw the rest,
    /// bottom to top.
    pub(crate) fn finish(&mut self) -> &[SpriteInstance] {
        self.nodes.finish(SPACE, None).instances()
    }
}

/// Where a chrome drawing puts its pieces, in drawing order: the pictures of
/// its parts and its buttons, each named by its index among the drawing's
/// parts and buttons (the sidebar's `SidebarPart as usize`, the command
/// bar's command).
pub(crate) trait ChromePieces {
    /// Start the part `part`: the pictures from here until the next part
    /// are its.
    fn part(&mut self, part: usize);
    /// The next picture: `rect` in the layout space, drawn as `instance`.
    fn picture(&mut self, rect: Rect, instance: SpriteInstance);
    /// The button `button` is next, drawn as `piece` (`rect` in the layout
    /// space and its instance); without one it draws nothing this frame.
    fn button(&mut self, button: usize, piece: Option<(Rect, SpriteInstance)>);
}

/// The pieces' instances alone, in drawing order: what a chrome's nodes
/// must draw.
#[cfg(test)]
impl ChromePieces for Vec<SpriteInstance> {
    fn part(&mut self, _: usize) {}

    fn picture(&mut self, _: Rect, instance: SpriteInstance) {
        self.push(instance);
    }

    fn button(&mut self, _: usize, piece: Option<(Rect, SpriteInstance)>) {
        self.extend(piece.map(|(_, instance)| instance));
    }
}

/// A chrome drawn from the match's chrome layer: its parts and buttons are
/// that layer's nodes ([`crate::app::input::chrome_input::ChromePaintNodes`]),
/// its pictures nodes of those parts kept from frame to frame.
pub(crate) struct ChromeHud {
    nodes: RetainedHud,
}

impl Default for ChromeHud {
    fn default() -> Self {
        Self {
            nodes: RetainedHud::lent(Origin::Host, 1, Rest::Remove),
        }
    }
}

/// A frame of a [`ChromeHud`]: the chrome's `parts` and `buttons` by index.
pub(crate) struct ChromeFrame<'a> {
    nodes: &'a mut RetainedHud,
    parts: &'a [NodeId],
    buttons: &'a [NodeId],
}

impl ChromeHud {
    /// Start a frame in `gui`, the match's GUI, whose chrome layer was built
    /// as `tree`: it is lent to the frame until [`Self::finish`].
    pub(crate) fn frame<'a>(
        &'a mut self,
        gui: &mut Gui,
        tree: u64,
        parts: &'a [NodeId],
        buttons: &'a [NodeId],
    ) -> ChromeFrame<'a> {
        self.nodes.begin_in(gui, tree);
        ChromeFrame {
            nodes: &mut self.nodes,
            parts,
            buttons,
        }
    }

    /// End the frame: drop the pictures it no longer has, draw the parts and
    /// buttons `from`, one after the other bottom to top (the drawing's
    /// order, not the tree's, which is the pointer's), and hand the tree
    /// back to `gui`.
    pub(crate) fn finish(&mut self, gui: &mut Gui, from: &[NodeId]) -> &[SpriteInstance] {
        self.nodes.finish_in(gui, from, SPACE, None).instances()
    }
}

impl ChromePieces for ChromeFrame<'_> {
    fn part(&mut self, part: usize) {
        self.nodes.part_node(self.parts.get(part).copied());
    }

    fn picture(&mut self, rect: Rect, instance: SpriteInstance) {
        let draw = NodeDraw {
            instance: Some(instance),
            ..Default::default()
        };
        self.nodes.picture(rect, draw);
    }

    fn button(&mut self, button: usize, piece: Option<(Rect, SpriteInstance)>) {
        let Some(&node) = self.buttons.get(button) else {
            return;
        };
        let (rect, draw) = match piece {
            Some((rect, instance)) => (
                Some(rect),
                NodeDraw {
                    instance: Some(instance),
                    ..Default::default()
                },
            ),
            None => (None, NodeDraw::default()),
        };
        self.nodes.adopt(node, rect, draw);
    }
}

/// The whole pixels a piece covers in its layout space: from the floor of
/// its corner to the ceiling of its far edge.
pub(crate) fn covering(x: f32, y: f32, w: f32, h: f32) -> Rect {
    let (left, top) = (x.floor(), y.floor());
    Rect::new(
        left as i32,
        top as i32,
        ((x + w).ceil() - left) as i32,
        ((y + h).ceil() - top) as i32,
    )
}

#[cfg(all(test, tests_app_presentation))]
mod tests {
    use super::*;

    fn quad(x: f32) -> SpriteInstance {
        SpriteInstance {
            position: [x, 2.5],
            size: [3.0, 4.0],
            uv_origin: [0.125, 0.25],
            uv_size: [0.0625, 0.5],
            depth: 0.00048,
            tint: [1.0; 3],
            alpha: 1.0,
            ..Default::default()
        }
    }

    #[test]
    fn pieces_draw_their_quads_in_order_and_a_shorter_frame_drops_the_rest() {
        let mut hud = PictureHud::default();
        hud.begin();
        for x in [5.0, 1.0, 9.5] {
            hud.picture(covering(x, 2.5, 3.0, 4.0), quad(x));
        }
        let drawn: Vec<_> = hud.finish().iter().map(|q| q.position[0]).collect();
        assert_eq!(drawn, [5.0, 1.0, 9.5]);
        let first = hud.finish()[0];
        assert_eq!((first.size, first.depth, first.alpha), ([3.0, 4.0], 0.00048, 1.0));
        assert_eq!(hud.nodes.rects()[2], Rect::new(9, 2, 4, 5));
        hud.begin();
        hud.picture(covering(7.0, 0.0, 1.0, 1.0), quad(7.0));
        assert_eq!(hud.finish().len(), 1);
        assert_eq!(hud.nodes.rects(), [Rect::new(7, 0, 1, 1)]);
    }

    #[test]
    fn parts_name_their_pictures_without_moving_them_and_hide_as_one() {
        let mut hud = PictureHud::default();
        let frame = |hud: &mut PictureHud, with_middle: bool| -> Vec<f32> {
            hud.begin();
            hud.part("Top");
            hud.picture(covering(1.0, 0.0, 3.0, 4.0), quad(1.0));
            hud.picture(covering(2.0, 0.0, 3.0, 4.0), quad(2.0));
            if with_middle {
                hud.part("Middle");
                hud.picture(covering(3.0, 0.0, 3.0, 4.0), quad(3.0));
            }
            hud.part("Power");
            hud.picture(covering(4.0, 0.0, 3.0, 4.0), quad(4.0));
            hud.finish().iter().map(|q| q.position[0]).collect()
        };
        assert_eq!(frame(&mut hud, true), [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(frame(&mut hud, false), [1.0, 2.0, 4.0]);
        assert!(hud.nodes.find("Middle").is_none());
        assert_eq!(frame(&mut hud, true), [1.0, 2.0, 3.0, 4.0]);
        let top = hud.nodes.find("Top").unwrap();
        hud.nodes
            .hud_mut()
            .set(top, vera20k_gui::Prop::Visible, vera20k_gui::Value::Bool(false))
            .unwrap();
        assert_eq!(frame(&mut hud, true), [3.0, 4.0], "a hidden part hides its pictures");
    }

    /// A chrome button is one node: drawn where its picture covers, it
    /// takes the click where the cursor hits it, and a frame that draws
    /// nothing of it, or leaves it out, keeps it on the pointer.
    #[test]
    fn a_chrome_button_that_draws_nothing_still_takes_its_click() {
        use crate::app::input::chrome_input::{ID_REPAIR, edge, paint_nodes_of, test_layer};
        use crate::ui::sidebar::SidebarPart;
        use vera20k_gui::{MouseButton, Prop, Value};
        let (mut gui, nodes) = test_layer();
        let chrome = paint_nodes_of(&nodes);
        let repair = nodes.repair;
        gui.set(repair, Prop::HitRect, Value::Rect(Rect::new(10, 10, 20, 20)))
            .unwrap();
        gui.set(repair, Prop::Enabled, Value::Bool(true)).unwrap();
        let mut hud = ChromeHud::default();
        let index = SidebarPart::Repair as usize;
        let mut frame = |gui: &mut Gui, piece: Option<Option<(Rect, SpriteInstance)>>| {
            {
                let parts = &chrome.sidebar_parts;
                let mut pieces = hud.frame(gui, chrome.tree, parts, parts);
                if let Some(piece) = piece {
                    pieces.part(index);
                    pieces.button(index, piece);
                }
            }
            hud.finish(gui, &[repair]).len()
        };
        let drawn = Some((covering(100.0, 50.0, 3.0, 4.0), quad(100.0)));
        assert_eq!(frame(&mut gui, Some(drawn)), 1);
        assert_eq!(gui.get(repair, Prop::Rect).unwrap(), Some(&Value::Rect(Rect::new(100, 50, 3, 4))));
        for nothing in [Some(None), None] {
            assert_eq!(frame(&mut gui, nothing), 0, "it draws nothing");
            assert!(edge(&mut gui, 15, 15, MouseButton::Left, true).1.is_empty());
            assert_eq!(
                edge(&mut gui, 15, 15, MouseButton::Left, false).1,
                [(ID_REPAIR, false)],
                "and still takes its click where the cursor hits it"
            );
            assert_ne!(gui.chrome_hit(101, 51), Some(repair), "not where it drew");
        }
        assert_eq!(frame(&mut gui, Some(drawn)), 1);
    }

    #[test]
    fn a_host_area_draws_the_hosts_instance_and_goes_with_it() {
        let mut area = AreaHud::default();
        let drawn = area.draw("minimap", Some((covering(16.5, 49.0, 140.0, 108.0), quad(16.5))));
        assert_eq!(drawn.len(), 1);
        assert_eq!(bytemuck::bytes_of(&drawn[0]), bytemuck::bytes_of(&quad(16.5)));
        assert!(area.draw("minimap", None).is_empty());
        assert_eq!(area.draw("minimap", Some((Rect::new(0, 0, 0, 0), quad(3.0))))[0].position[0], 3.0);
    }
}

/// One host area as an `External` node: a part the host draws itself (the
/// radar's minimap) placed and named in the tree, so it can be found,
/// hidden or measured like any other part, while its pixels stay the
/// host's. The host hands over what it drew there as one instance.
pub(crate) struct AreaHud {
    nodes: RetainedHud,
}

impl Default for AreaHud {
    fn default() -> Self {
        Self {
            nodes: RetainedHud::new(Layer::Panels, Origin::Host, 1, 1, Rest::Remove),
        }
    }
}

impl AreaHud {
    /// Show the area `name` over `rect` (layout pixels) drawn as `instance`,
    /// or no area; the instances to draw this frame.
    pub(crate) fn draw(
        &mut self,
        name: &'static str,
        area: Option<(Rect, SpriteInstance)>,
    ) -> &[SpriteInstance] {
        self.nodes.begin();
        if let Some((rect, instance)) = area {
            let draw = NodeDraw {
                instance: Some(instance),
                ..Default::default()
            };
            self.nodes.area(name, rect, draw);
        }
        self.nodes.finish(SPACE, None).instances()
    }
}
