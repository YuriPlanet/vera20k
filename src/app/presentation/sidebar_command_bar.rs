//! Native bottom bar art and its one layout shared by drawing and gadgets.
//!
//! The bar lays out in the sidebar's layout space (`ui::sidebar::frame`), on
//! the strip under the battlefield, and draws into its own surface
//! (`presentation::sidebar_surface`); its gadgets take it in frame pixels.

use super::*;
use crate::ui::sidebar::command_bar::CommandBarLayout;

pub(crate) fn layout_and_slots(state: &AppState) -> Option<(CommandBarLayout, Vec<Option<usize>>)> {
    let art = &current_sidebar_chrome(state)?.command_bar;
    let size = |entry: SidebarChromeEntry| entry.pixel_size.map(|v| v as u32);
    let (width, height) = state.sidebar_frame().layout_size();
    let layout = CommandBarLayout::new(
        [width, height],
        crate::ui::sidebar::SIDEBAR_WIDTH_PX,
        size(art.left_cap[0]?),
        size(art.background?),
        size(art.right_cap?),
        !state
            .match_state
            .match_presentation
            .sidebar_gadget_state
            .command_bar_closed,
    )?;
    use crate::app::match_runtime::session::SessionMode;
    let multiplayer = !matches!(
        state.match_state.session.mode(),
        SessionMode::Campaign | SessionMode::Skirmish
    );
    Some((layout, art.slots[usize::from(multiplayer)].clone()))
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
    let mut draw = |entry: Option<SidebarChromeEntry>, x: f32, y: f32| {
        if let Some(entry) = entry {
            append_clipped(out, entry, [x, y], layout.bounds, camera);
        }
    };
    // 6D0A20: native sidebar converter, frame0, clipped to the tactical-bottom
    // surface. Source pixels remain1:1; the wide spacer is cropped, not scaled.
    draw(art.spacer, 0., layout.bounds.y);
    draw(art.left_cap[0], layout.left_cap.x, layout.left_cap.y);
    for slot in 0..layout.tile_count {
        let rect = layout.slot(slot).unwrap();
        draw(art.background, rect.x, rect.y);
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
    let left = position[0].max(clip.x);
    let top = position[1].max(clip.y);
    let right = (position[0] + entry.pixel_size[0]).min(clip.x + clip.w);
    let bottom = (position[1] + entry.pixel_size[1]).min(clip.y + clip.h);
    if right <= left || bottom <= top {
        return;
    }
    out.push(SpriteInstance {
        position: [camera[0] + left, camera[1] + top],
        size: [right - left, bottom - top],
        uv_origin: [
            entry.uv_origin[0] + (left - position[0]) / entry.pixel_size[0] * entry.uv_size[0],
            entry.uv_origin[1] + (top - position[1]) / entry.pixel_size[1] * entry.uv_size[1],
        ],
        uv_size: [
            (right - left) / entry.pixel_size[0] * entry.uv_size[0],
            (bottom - top) / entry.pixel_size[1] * entry.uv_size[1],
        ],
        depth: 0.00048,
        tint: [1.; 3],
        alpha: 1.,
        ..Default::default()
    });
}
