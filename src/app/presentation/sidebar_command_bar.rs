//! Native bottom bar art and its one layout shared by drawing and gadgets.
//!
//! The bar lays out in the sidebar's layout space (`ui::sidebar::frame`), on
//! the strip under the battlefield, and draws into its own surface
//! (`presentation::sidebar_surface`); its gadgets take it in frame pixels.

use std::collections::HashMap;

use vera20k_gui::hud::{Hud, InputPolicy, Layer, Origin};
use vera20k_gui::{NodeId, NodeKind, Prop, Rect as GuiRect, Value};

use super::*;
use crate::render::hud_paint::{HudPaint, HudSpace, NodeDraw, Picture};
use crate::ui::sidebar::command_bar::{CommandBarLayout, spacer_pieces};

pub(crate) fn layout_and_slots(state: &AppState) -> Option<(CommandBarLayout, Vec<Option<usize>>)> {
    let art = &current_sidebar_chrome(state)?.command_bar;
    let layout = layout_on_frame(
        art,
        state.sidebar_frame(),
        state
            .match_state
            .match_presentation
            .sidebar_gadget_state
            .command_bar_closed,
    )?;
    let multiplayer = !state.match_state.session_mode().is_single_player();
    Some((layout, art.slots[usize::from(multiplayer)].clone()))
}

/// The bar a match shows on `frame`, in its layout space: along the strip the
/// frame gives it, beside the sidebar's column. `None` without a bar.
fn layout_on_frame(
    art: &crate::render::sidebar_chrome::CommandBarArt<SidebarChromeEntry>,
    frame: crate::ui::sidebar::SidebarFrame,
    closed: bool,
) -> Option<CommandBarLayout> {
    if !frame.has_bar() {
        return None;
    }
    let size = |entry: SidebarChromeEntry| entry.pixel_size.map(|v| v as u32);
    let (width, height) = frame.layout_size();
    // `new` takes the screen and what the column takes of its width.
    let beside = width.saturating_sub(frame.bar_surface_size().0);
    let bottom_strip_top = height as f32 - crate::ui::sidebar::BOTTOM_STRIP_HEIGHT_PX as f32;
    Some(
        CommandBarLayout::new(
            [width, height],
            beside,
            size(art.left_cap[0]?),
            size(art.background?),
            size(art.right_cap?),
            !closed,
        )?
        .flush_left()
        .moved(
            frame.bar_layout_left(),
            frame.strip_layout_top() - bottom_strip_top,
        ),
    )
}

/// The bar's pieces as a node tree, kept from frame to frame, and the
/// instances it draws: one picture node per piece, in drawing order, each
/// with its rect in the bar's layout space. A piece clipped away, or whose
/// art is missing, has no node.
pub(crate) struct CommandBarHud {
    hud: Hud,
    /// The picture nodes, in drawing order.
    nodes: Vec<NodeId>,
    /// How many of `nodes` this update has reached.
    used: usize,
    /// The tree's revision at its last layout.
    laid_out: u64,
    /// Each node's quad, as the bar's arithmetic places and clips it.
    draws: HashMap<NodeId, NodeDraw>,
    paint: HudPaint,
}

impl Default for CommandBarHud {
    fn default() -> Self {
        Self {
            hud: Hud::new(usize::MAX, InputPolicy::default()),
            nodes: Vec::new(),
            used: 0,
            laid_out: 0,
            draws: HashMap::new(),
            paint: HudPaint::default(),
        }
    }
}

/// The `Art` every piece's node names; which picture it is, the node's
/// [`NodeDraw`] says.
const PICTURE: &str = "picture";

/// The depth the bar's surface draws at.
const DEPTH: f32 = 0.00048;

impl CommandBarHud {
    /// The bar `state` shows, drawn: its instances, bottom to top.
    pub(crate) fn draw_for(&mut self, state: &AppState, atlas: &SidebarChromeAtlas) -> &[SpriteInstance] {
        let Some((layout, slots)) = layout_and_slots(state) else {
            self.forget_from(0);
            self.paint.build_pictures(&self.hud, SPACE, |_| NodeDraw::default());
            return self.paint.instances();
        };
        let camera = [
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
        ];
        self.draw(
            &atlas.command_bar,
            layout,
            &slots,
            &state.match_state.match_presentation.sidebar_gadget_state,
            camera,
        )
    }

    /// Bring the tree to the bar and draw it. `camera` is what the surface's
    /// shader subtracts.
    pub(crate) fn draw(
        &mut self,
        art: &crate::render::sidebar_chrome::CommandBarArt<SidebarChromeEntry>,
        layout: CommandBarLayout,
        slots: &[Option<usize>],
        gadgets: &crate::ui::sidebar::gadget_flash::SidebarGadgetState,
        camera: [f32; 2],
    ) -> &[SpriteInstance] {
        self.used = 0;
        self.draws.clear();
        // 6D0A20: native sidebar converter, frame0, clipped to the tactical-bottom
        // surface. The spacer reaches the left cap as `spacer_pieces` lays it
        // out: 1:1 and cropped as native up to 1080-pixel frames, partly
        // stretched in wider ones. Its NeedsRedraw gate (set by RequestRedraw
        // 6D0F00 for triggers and mission timers, RequestTimedRedraw 6D0EC0) only
        // skips unchanged frames; VERA draws the bar every frame.
        if let Some(spacer) = art.spacer {
            let cap_x = (layout.left_cap.x - layout.bounds.x).max(0.0) as u32;
            let pieces = spacer_pieces(cap_x, spacer.pixel_size[0] as u32);
            for piece in pieces.into_iter().flatten() {
                self.piece(
                    spacer.columns(piece.source_x as f32, piece.source_w as f32),
                    [layout.bounds.x + piece.x as f32, layout.bounds.y],
                    [piece.w as f32, spacer.pixel_size[1]],
                    layout.bounds,
                    camera,
                );
            }
        }
        let mut blit = |hud: &mut Self, entry: Option<SidebarChromeEntry>, x: f32, y: f32| {
            if let Some(entry) = entry {
                hud.piece(entry, [x, y], entry.pixel_size, layout.bounds, camera);
            }
        };
        blit(self, art.left_cap[0], layout.left_cap.x, layout.left_cap.y);
        for slot in 0..layout.tile_count {
            let rect = layout.slot(slot).unwrap();
            blit(self, art.background, rect.x, rect.y);
        }
        if let Some(rect) = layout.filler() {
            blit(
                self,
                art.background.map(|tile| tile.columns(0.0, rect.w)),
                rect.x,
                rect.y,
            );
        }
        blit(self, art.right_cap, layout.right_cap.x, layout.right_cap.y);
        for (slot, command) in slots.iter().copied().enumerate() {
            if let (Some(rect), Some(command)) = (layout.slot(slot), command) {
                blit(
                    self,
                    art.buttons[command][usize::from(gadgets.command_pressed[command])],
                    rect.x,
                    rect.y,
                );
            }
        }
        blit(
            self,
            art.left_cap[usize::from(gadgets.command_thumb_pressed)],
            layout.left_cap.x,
            layout.left_cap.y,
        );
        self.forget_from(self.used);
        if self.laid_out != self.hud.revision() {
            self.hud.layout(UNBOUNDED);
            self.laid_out = self.hud.revision();
        }
        let draws = &self.draws;
        self.paint.build_pictures(&self.hud, SPACE, |node| {
            draws.get(&node).copied().unwrap_or_default()
        });
        self.paint.instances()
    }

    /// The whole of `entry` drawn over `size` at `position`, then clipped to
    /// `clip`; at the canvas size it is a blit. The quad is the bar's own
    /// arithmetic, so it lands where and as it always has.
    fn piece(
        &mut self,
        entry: SidebarChromeEntry,
        position: [f32; 2],
        size: [f32; 2],
        clip: Rect,
        camera: [f32; 2],
    ) {
        let left = position[0].max(clip.x);
        let top = position[1].max(clip.y);
        let right = (position[0] + size[0]).min(clip.x + clip.w);
        let bottom = (position[1] + size[1]).min(clip.y + clip.h);
        if right <= left || bottom <= top {
            return;
        }
        let rect = GuiRect::new(
            left.floor() as i32,
            top.floor() as i32,
            ((right - left).ceil() as i32).max(1),
            ((bottom - top).ceil() as i32).max(1),
        );
        let Some(node) = self.node(rect) else {
            return;
        };
        let draw = NodeDraw {
            position: Some([camera[0] + left, camera[1] + top]),
            picture: Some(Picture {
                uv_origin: [
                    entry.uv_origin[0] + (left - position[0]) / size[0] * entry.uv_size[0],
                    entry.uv_origin[1] + (top - position[1]) / size[1] * entry.uv_size[1],
                ],
                uv_size: [
                    (right - left) / size[0] * entry.uv_size[0],
                    (bottom - top) / size[1] * entry.uv_size[1],
                ],
                pixel_size: [right - left, bottom - top],
            }),
            depth: DEPTH,
            ..Default::default()
        };
        self.draws.insert(node, draw);
    }

    /// The update's next node, last frame's where there is one.
    fn node(&mut self, rect: GuiRect) -> Option<NodeId> {
        let node = match self.nodes.get(self.used) {
            Some(&node) => node,
            None => {
                let node = self
                    .hud
                    .create_root(Layer::Panels, Origin::Host, NodeKind::Image)
                    .ok()?;
                let _ = self.hud.set(node, Prop::Art, Value::Text(PICTURE.to_owned()));
                self.nodes.push(node);
                node
            }
        };
        self.used += 1;
        if !matches!(self.hud.get(node, Prop::Rect), Ok(Some(Value::Rect(shown))) if *shown == rect) {
            let _ = self.hud.set(node, Prop::Rect, Value::Rect(rect));
        }
        Some(node)
    }

    fn forget_from(&mut self, kept: usize) {
        for &node in self.nodes.get(kept..).unwrap_or_default() {
            let _ = self.hud.remove(node);
        }
        self.nodes.truncate(kept);
    }
}

/// Where the tree lays out: everywhere, as the bar's arithmetic already
/// placed each piece.
const UNBOUNDED: GuiRect = GuiRect::new(i32::MIN / 2, i32::MIN / 2, i32::MAX, i32::MAX);

/// The bar's pixels are the surface's: the host places every quad itself.
const SPACE: HudSpace = HudSpace {
    origin: [0.0; 2],
    scale: 1.0,
};

#[cfg(all(test, tests_app_presentation))]
mod tests {
    use super::*;
    use crate::render::sidebar_chrome::CommandBarArt;
    use crate::ui::sidebar::gadget_flash::SidebarGadgetState;

    fn bar_art() -> CommandBarArt<SidebarChromeEntry> {
        let piece = |w: f32| SidebarChromeEntry {
            uv_origin: [0.0; 2],
            uv_size: [w / 1024.0, 1.0 / 32.0],
            pixel_size: [w, 32.0],
        };
        CommandBarArt {
            spacer: Some(piece(856.0)),
            left_cap: [Some(piece(28.0)); 3],
            right_cap: Some(piece(28.0)),
            background: Some(piece(52.0)),
            buttons: [[None; 2]; 11],
            slots: [Vec::new(), Vec::new()],
        }
    }

    /// The open bar at the reported 1920-wide frame, where gamemd's `72FC60`
    /// leaves 32 pixels of bare rail left of the cap: the cap takes the
    /// frame's edge, Team01's slot follows it, and the cap and tiles alone
    /// cover the strip, the leftover being the start of one more tile.
    #[test]
    fn open_bar_reaches_the_left_end_at_wide_frames() {
        let art = bar_art();
        let layout = layout_on_frame(
            &art,
            crate::ui::sidebar::SidebarFrame::native(1920, 1080),
            false,
        )
        .unwrap();
        assert_eq!(layout.left_cap.x, 0.0);
        assert_eq!(layout.slot(0).unwrap().x, 28.0);
        assert_eq!(layout.right_cap.x, 1724.0);
        let gadgets = SidebarGadgetState::default();
        let mut hud = CommandBarHud::default();
        let out = hud.draw(&art, layout, &[], &gadgets, [0.0; 2]).to_vec();
        let mut spans: Vec<_> = out
            .iter()
            .filter(|sprite| sprite.size[0] != 856.0)
            .map(|sprite| (sprite.position[0], sprite.position[0] + sprite.size[0]))
            .collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let reached = spans.iter().fold(0.0f32, |reached, &(left, right)| {
            assert!(left <= reached, "rail from {reached} to {left}");
            reached.max(right)
        });
        assert_eq!(reached, 1752.0);
        let filler = out
            .iter()
            .find(|sprite| sprite.position[0] == 1692.0)
            .unwrap();
        assert_eq!(
            (filler.size[0], filler.uv_origin[0], filler.uv_size[0]),
            (32.0, 0.0, 32.0 / 1024.0)
        );
    }

    /// The closed bar at the reported 1121x741 frame: the rail reaches the
    /// caps, where LSPACER's 856 pixels left 856..897 black, and its stretched
    /// part is the rail's right half. At 1080 wide, the widest frame whose
    /// rail native fills, it stays one 1:1 draw.
    #[test]
    fn closed_bar_rail_reaches_the_caps_at_wide_frames() {
        let art = bar_art();
        let closed_bar = |width, height| {
            let layout =
                CommandBarLayout::new([width, height], 168, [28, 32], [52, 32], [28, 32], false)
                    .unwrap();
            let gadgets = SidebarGadgetState::default();
            CommandBarHud::default()
                .draw(&art, layout, &[], &gadgets, [0.0; 2])
                .to_vec()
        };
        let wide = closed_bar(1121, 741);
        let mut spans: Vec<_> = wide
            .iter()
            .map(|sprite| (sprite.position[0], sprite.position[0] + sprite.size[0]))
            .collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let reached = spans.iter().fold(0.0f32, |reached, &(left, right)| {
            assert!(left <= reached, "black from {reached} to {left}");
            reached.max(right)
        });
        assert_eq!(reached, 953.0);
        let rail = wide
            .iter()
            .find(|sprite| sprite.position[0] == 428.0)
            .unwrap();
        assert_eq!(
            (rail.size[0], rail.uv_origin[0], rail.uv_size[0]),
            (469.0, 428.0 / 1024.0, 428.0 / 1024.0)
        );
        let native = closed_bar(1080, 768);
        assert_eq!(
            (native[0].position, native[0].size),
            ([0.0, 736.0], [856.0, 32.0])
        );
        assert_eq!(native[0].uv_size, art.spacer.unwrap().uv_size);
    }
}
