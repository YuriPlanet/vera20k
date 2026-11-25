//! Native bottom bar art and its one layout shared by drawing and gadgets.
//!
//! The bar lays out in the sidebar's layout space (`ui::sidebar::frame`), on
//! the strip under the battlefield, and draws into its own surface
//! (`presentation::sidebar_surface`); its gadgets take it in frame pixels.

use super::*;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::presentation::picture_hud::{ChromePieces, covering};
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::ui::sidebar::command_bar::{CommandBarLayout, spacer_pieces};

pub(crate) fn layout_and_slots(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> Option<(CommandBarLayout, Vec<Option<usize>>)> {
    let art = &current_sidebar_chrome(frontend, match_state)?.command_bar;
    let layout = layout_on_frame(
        art,
        crate::app::state::sidebar_frame(
            platform,
            persistence,
            renderer,
            &frontend.screen,
            &match_state.match_presentation.sidebar_layout_spec,
        ),
        match_state
            .match_presentation
            .sidebar_gadget_state
            .command_bar_closed,
    )?;
    let multiplayer = !match_state.session_mode().is_single_player();
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

/// The depth the bar's surface draws at.
const DEPTH: f32 = 0.00048;

/// The bar's pieces into `pieces`, in drawing order: its rail, caps and
/// tiles as its part 0, then each slot's button (command `i` is button
/// `i`) and the thumb (button 11) over the left cap. A piece clipped away,
/// or whose art is missing, draws nothing. `camera` is what the surface's
/// shader subtracts.
pub(crate) fn draw(
    pieces: &mut impl ChromePieces,
    art: &crate::render::sidebar_chrome::CommandBarArt<SidebarChromeEntry>,
    layout: CommandBarLayout,
    slots: &[Option<usize>],
    gadgets: &crate::ui::sidebar::gadget_flash::SidebarGadgetState,
    camera: [f32; 2],
) {
    pieces.part(0);
    // 6D0A20: native sidebar converter, frame0, clipped to the tactical-bottom
    // surface. The spacer reaches the left cap as `spacer_pieces` lays it
    // out: 1:1 and cropped as native up to 1080-pixel frames, partly
    // stretched in wider ones. Its NeedsRedraw gate (set by RequestRedraw
    // 6D0F00 for triggers and mission timers, RequestTimedRedraw 6D0EC0) only
    // skips unchanged frames; VERA draws the bar every frame.
    if let Some(spacer) = art.spacer {
        let cap_x = (layout.left_cap.x - layout.bounds.x).max(0.0) as u32;
        let rail = spacer_pieces(cap_x, spacer.pixel_size[0] as u32);
        for piece in rail.into_iter().flatten() {
            let piece = clipped(
                spacer.columns(piece.source_x as f32, piece.source_w as f32),
                [layout.bounds.x + piece.x as f32, layout.bounds.y],
                [piece.w as f32, spacer.pixel_size[1]],
                layout.bounds,
                camera,
            );
            if let Some((rect, instance)) = piece {
                pieces.picture(rect, instance);
            }
        }
    }
    let blit = |entry: Option<SidebarChromeEntry>, x: f32, y: f32| {
        entry.and_then(|entry| clipped(entry, [x, y], entry.pixel_size, layout.bounds, camera))
    };
    let mut picture = |piece: Option<(vera20k_gui::Rect, SpriteInstance)>| {
        if let Some((rect, instance)) = piece {
            pieces.picture(rect, instance);
        }
    };
    picture(blit(art.left_cap[0], layout.left_cap.x, layout.left_cap.y));
    for slot in 0..layout.tile_count {
        let rect = layout.slot(slot).unwrap();
        picture(blit(art.background, rect.x, rect.y));
    }
    if let Some(rect) = layout.filler() {
        let tile = art.background.map(|tile| tile.columns(0.0, rect.w));
        picture(blit(tile, rect.x, rect.y));
    }
    picture(blit(art.right_cap, layout.right_cap.x, layout.right_cap.y));
    for (slot, command) in slots.iter().copied().enumerate() {
        let Some(command) = command.filter(|&command| command < 11) else {
            continue;
        };
        let piece = layout.slot(slot).and_then(|rect| {
            let entry = art.buttons[command][usize::from(gadgets.command_pressed[command])];
            blit(entry, rect.x, rect.y)
        });
        pieces.button(command, piece);
    }
    let thumb = art.left_cap[usize::from(gadgets.command_thumb_pressed)];
    pieces.button(11, blit(thumb, layout.left_cap.x, layout.left_cap.y));
}

/// The bar's nodes in the order [`draw`] draws them, and how many: its
/// rail, caps and tiles, each slot's button, the thumb. A button without a
/// slot draws nothing.
pub(crate) fn paint_order(
    nodes: &crate::app::input::chrome_input::ChromePaintNodes,
    slots: &[Option<usize>],
) -> ([vera20k_gui::NodeId; 13], usize) {
    let mut order = [nodes.bar_parts[0]; 13];
    let mut drawn = 1;
    // `read_button_list` keeps one slot per command.
    for command in slots.iter().flatten().copied().filter(|&command| command < 11) {
        if drawn < 12 {
            order[drawn] = nodes.bar_buttons[command];
            drawn += 1;
        }
    }
    order[drawn] = nodes.bar_buttons[11];
    (order, drawn + 1)
}

/// The whole of `entry` drawn over `size` at `position`, then clipped to
/// `clip`, and the whole pixels it covers; at the canvas size it is a blit.
/// The quad is the bar's own arithmetic, so it lands where and as it always
/// has. None when clipped away.
fn clipped(
    entry: SidebarChromeEntry,
    position: [f32; 2],
    size: [f32; 2],
    clip: Rect,
    camera: [f32; 2],
) -> Option<(vera20k_gui::Rect, SpriteInstance)> {
    let left = position[0].max(clip.x);
    let top = position[1].max(clip.y);
    let right = (position[0] + size[0]).min(clip.x + clip.w);
    let bottom = (position[1] + size[1]).min(clip.y + clip.h);
    if right <= left || bottom <= top {
        return None;
    }
    Some((
        covering(left, top, right - left, bottom - top),
        SpriteInstance {
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
            depth: DEPTH,
            tint: [1.; 3],
            alpha: 1.,
            ..Default::default()
        },
    ))
}

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
        let mut out: Vec<SpriteInstance> = Vec::new();
        draw(&mut out, &art, layout, &[], &gadgets, [0.0; 2]);
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
            let mut out: Vec<SpriteInstance> = Vec::new();
            draw(&mut out, &art, layout, &[], &gadgets, [0.0; 2]);
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

    /// The bar drawn by its nodes on the chrome layer, its buttons by the
    /// nodes that take their clicks, draws the instances of its drawing in
    /// the same order, frame after frame: as its buttons press, as a slot's
    /// art goes missing, and closed.
    #[test]
    fn the_bars_nodes_draw_its_pieces_in_drawing_order() {
        let mut art = bar_art();
        for (command, frames) in art.buttons.iter_mut().enumerate() {
            let entry = |frame: f32| SidebarChromeEntry {
                uv_origin: [command as f32 / 16.0, frame / 4.0],
                uv_size: [52.0 / 1024.0, 1.0 / 32.0],
                pixel_size: [52.0, 32.0],
            };
            *frames = [Some(entry(0.0)), Some(entry(1.0))];
        }
        // Retail UIMD.INI [AdvancedCommandBar] ButtonList.
        let slots = vec![Some(0), Some(1), Some(3), Some(4), Some(6), Some(9)];
        let (mut gui, nodes) = crate::app::input::chrome_input::test_layer();
        let chrome = crate::app::input::chrome_input::paint_nodes_of(&nodes);
        let (order, count) = paint_order(&chrome, &slots);
        let mut hud = crate::app::presentation::picture_hud::ChromeHud::default();
        let mut frame = |gui: &mut vera20k_gui::Gui,
                         art: &CommandBarArt<SidebarChromeEntry>,
                         open: bool,
                         gadgets: &SidebarGadgetState| {
            let layout =
                CommandBarLayout::new([1024, 768], 168, [28, 32], [52, 32], [28, 32], open)
                    .unwrap();
            let camera = [3.5, -2.0];
            let mut expected: Vec<SpriteInstance> = Vec::new();
            draw(&mut expected, art, layout, &slots, gadgets, camera);
            {
                let mut pieces =
                    hud.frame(gui, chrome.tree, &chrome.bar_parts, &chrome.bar_buttons);
                draw(&mut pieces, art, layout, &slots, gadgets, camera);
            }
            let drawn = hud.finish(gui, &order[..count]).to_vec();
            assert_eq!(
                bytemuck::cast_slice::<SpriteInstance, u8>(&drawn),
                bytemuck::cast_slice::<SpriteInstance, u8>(&expected),
            );
            drawn.len()
        };
        let mut gadgets = SidebarGadgetState::default();
        let open = frame(&mut gui, &art, true, &gadgets);
        assert!(open > slots.len(), "the rail, the tiles, six buttons and the thumb");
        assert_eq!(frame(&mut gui, &art, true, &gadgets), open);
        gadgets.command_pressed[3] = true;
        gadgets.command_thumb_pressed = true;
        assert_eq!(frame(&mut gui, &art, true, &gadgets), open);
        art.buttons[4] = [None; 2];
        assert_eq!(frame(&mut gui, &art, true, &gadgets), open - 1);
        assert!(frame(&mut gui, &art, false, &gadgets) < open);
        assert_eq!(frame(&mut gui, &art, true, &gadgets), open - 1);
    }
}
