//! A HUD of atlas pictures whose host already knows each quad.
//!
//! The sidebar's chrome and the bottom command bar are runs of pictures from
//! one atlas, each placed by the original's own arithmetic. Here they become
//! picture nodes of a retained `vera20k-gui` tree, one per piece, in drawing
//! order and with the piece's rect in its layout space, so the tree owns
//! which pieces stand, their order and their hit rects. Each node's
//! instance is the one its host made, handed to the adapter unchanged, so
//! every instance is what the host drew before.

use vera20k_gui::{Layer, Origin, Rect};

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
