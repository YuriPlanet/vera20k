//! Native bottom bar art and its one layout shared by drawing and gadgets.
//!
//! The bar lays out in the sidebar's layout space (`ui::sidebar::frame`), on
//! the strip under the battlefield, and draws into its own surface
//! (`presentation::sidebar_surface`); its gadgets take it in frame pixels.

use super::*;
use crate::ui::sidebar::command_bar::{CommandBarLayout, spacer_pieces};

pub(crate) fn layout_and_slots(state: &AppState) -> Option<(CommandBarLayout, Vec<Option<usize>>)> {
    let art = &current_sidebar_chrome(state)?.command_bar;
    let frame = state.sidebar_frame();
    let layout = layout_on_frame(
        art,
        frame.layout_size(),
        frame.column_width(),
        state
            .match_state
            .match_presentation
            .sidebar_gadget_state
            .command_bar_closed,
    )?;
    let multiplayer = !state.match_state.session_mode().is_single_player();
    Some((layout, art.slots[usize::from(multiplayer)].clone()))
}

/// The bar a match shows on a frame of that many sidebar pixels.
fn layout_on_frame(
    art: &crate::render::sidebar_chrome::CommandBarArt<SidebarChromeEntry>,
    (width, height): (u32, u32),
    sidebar_width: u32,
    closed: bool,
) -> Option<CommandBarLayout> {
    let size = |entry: SidebarChromeEntry| entry.pixel_size.map(|v| v as u32);
    Some(
        CommandBarLayout::new(
            [width, height],
            sidebar_width,
            size(art.left_cap[0]?),
            size(art.background?),
            size(art.right_cap?),
            !closed,
        )?
        .flush_left(),
    )
}

pub(super) fn append(state: &AppState, atlas: &SidebarChromeAtlas, out: &mut Vec<SpriteInstance>) {
    let Some((layout, slots)) = layout_and_slots(state) else {
        return;
    };
    let camera = [
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
    ];
    append_prepared(
        out,
        &atlas.command_bar,
        layout,
        &slots,
        &state.match_state.match_presentation.sidebar_gadget_state,
        camera,
    );
}

pub(crate) fn append_prepared(
    out: &mut Vec<SpriteInstance>,
    art: &crate::render::sidebar_chrome::CommandBarArt<SidebarChromeEntry>,
    layout: CommandBarLayout,
    slots: &[Option<usize>],
    gadgets: &crate::ui::sidebar::gadget_flash::SidebarGadgetState,
    camera: [f32; 2],
) {
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
            append_scaled(
                out,
                spacer.columns(piece.source_x as f32, piece.source_w as f32),
                [layout.bounds.x + piece.x as f32, layout.bounds.y],
                [piece.w as f32, spacer.pixel_size[1]],
                layout.bounds,
                camera,
            );
        }
    }
    let mut draw = |entry: Option<SidebarChromeEntry>, x: f32, y: f32| {
        if let Some(entry) = entry {
            append_clipped(out, entry, [x, y], layout.bounds, camera);
        }
    };
    draw(art.left_cap[0], layout.left_cap.x, layout.left_cap.y);
    for slot in 0..layout.tile_count {
        let rect = layout.slot(slot).unwrap();
        draw(art.background, rect.x, rect.y);
    }
    if let Some(rect) = layout.filler() {
        draw(
            art.background.map(|tile| tile.columns(0.0, rect.w)),
            rect.x,
            rect.y,
        );
    }
    draw(art.right_cap, layout.right_cap.x, layout.right_cap.y);
    for (slot, command) in slots.iter().copied().enumerate() {
        if let (Some(rect), Some(command)) = (layout.slot(slot), command) {
            draw(
                art.buttons[command][usize::from(gadgets.command_pressed[command])],
                rect.x,
                rect.y,
            );
        }
    }
    draw(
        art.left_cap[usize::from(gadgets.command_thumb_pressed)],
        layout.left_cap.x,
        layout.left_cap.y,
    );
}

fn append_clipped(
    out: &mut Vec<SpriteInstance>,
    entry: SidebarChromeEntry,
    position: [f32; 2],
    clip: Rect,
    camera: [f32; 2],
) {
    append_scaled(out, entry, position, entry.pixel_size, clip, camera);
}

/// The whole of `entry` drawn over `size` at `position`, then clipped as
/// [`append_clipped`] clips; at the canvas size it is that blit.
fn append_scaled(
    out: &mut Vec<SpriteInstance>,
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
    out.push(SpriteInstance {
        position: [camera[0] + left, camera[1] + top],
        size: [right - left, bottom - top],
        uv_origin: [
            entry.uv_origin[0] + (left - position[0]) / size[0] * entry.uv_size[0],
            entry.uv_origin[1] + (top - position[1]) / size[1] * entry.uv_size[1],
        ],
        uv_size: [
            (right - left) / size[0] * entry.uv_size[0],
            (bottom - top) / size[1] * entry.uv_size[1],
        ],
        depth: 0.00048,
        tint: [1.; 3],
        alpha: 1.,
        ..Default::default()
    });
}

#[cfg(test)]
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
        let layout = layout_on_frame(&art, (1920, 1080), 168, false).unwrap();
        assert_eq!(layout.left_cap.x, 0.0);
        assert_eq!(layout.slot(0).unwrap().x, 28.0);
        assert_eq!(layout.right_cap.x, 1724.0);
        let mut out = Vec::new();
        let gadgets = SidebarGadgetState::default();
        append_prepared(&mut out, &art, layout, &[], &gadgets, [0.0; 2]);
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
            let mut out = Vec::new();
            let gadgets = SidebarGadgetState::default();
            append_prepared(&mut out, &art, layout, &[], &gadgets, [0.0; 2]);
            out
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
