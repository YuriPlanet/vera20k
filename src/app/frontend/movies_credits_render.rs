//! Render glue for the Movies & Credits children: movie list `0x129`, the
//! full-screen Play_Movie presentation and the Show_Credits roll.

use anyhow::Result;

use crate::app::AppState;
use crate::app::frontend::shell_transition::ButtonGroup;
use crate::render::batch::{BatchTexture, SpriteInstance};
use crate::render::main_menu_shell_chrome::{MainMenuShellChromeAtlas, MainMenuShellChromeEntry};
use crate::render::shell_paint::{
    self, ArtFit, ButtonPolicy, CHROME_DEPTH, CURSOR_DEPTH, PARENT_BACKGROUND_DEPTH, PaintButton,
    PaintLabel, SHELL_TEXT_RGB_ENABLED,
};
use crate::render::shell_surface_present::SurfaceEffects;
use crate::render::shell_text::{ShellAlign, ShellTextDraw};
use crate::ui::main_menu_shell::RectPx;
use crate::ui::movies_credits_shell::{
    MOVIE_LIST_CONTROL, MOVIE_LIST_PAGE, MOVIE_LIST_PROMPT_KEY, MOVIE_LIST_TOOLTIP_KEY,
    MovieListLayout, compute_movie_list_layout,
};
use crate::ui::shell::list::ROW_HEIGHT;

/// `0x129` right-panel buttons use the same native SDBTNANM policy as the
/// menu pages: press art, no hover flash.
const MOVIE_LIST_BUTTON_POLICY: ButtonPolicy = ButtonPolicy {
    art_fit: ArtFit::Native,
    hover_flash: false,
    art_sink_y: 0.0,
    disabled_dim: true,
};

/// Owner-draw ListBox frame colors (`0x00619230`), as RGB: light
/// `0xC5BEA7`, dark `0x807A68`, and their average `0xA29C87` at the two
/// corners where they meet.
const LIST_FRAME_LIGHT: [u8; 3] = [0xA7, 0xBE, 0xC5];
const LIST_FRAME_DARK: [u8; 3] = [0x68, 0x7A, 0x80];
const LIST_FRAME_CORNER: [u8; 3] = [0x87, 0x9C, 0xA2];
/// Selected-row fill `0x0000FF` (RGB red).
const LIST_SELECTED_FILL: [u8; 3] = [0xFF, 0x00, 0x00];
/// Row text inset from the row's left edge.
const LIST_TEXT_INSET: i32 = 2;

const LIST_DEPTH: f32 = CHROME_DEPTH - 0.00002;
const LIST_FILL_DEPTH: f32 = CHROME_DEPTH - 0.00003;

fn rgb(color: [u8; 3]) -> [f32; 3] {
    color.map(|channel| f32::from(channel) / 255.0)
}

fn resolve_csf<'a>(state: &'a AppState, key: &'static str) -> std::borrow::Cow<'a, str> {
    state
        .process_assets
        .csf
        .as_ref()
        .map(|csf| csf.text(key))
        .unwrap_or(std::borrow::Cow::Borrowed(key))
}

fn push_entry_crop(
    out: &mut Vec<SpriteInstance>,
    entry: MainMenuShellChromeEntry,
    entry_origin: (i32, i32),
    rect: RectPx,
    depth: f32,
) {
    // Clip the destination to the entry canvas, then map it to atlas UVs.
    let left = rect.x.max(entry_origin.0);
    let top = rect.y.max(entry_origin.1);
    let right = (rect.x + rect.w).min(entry_origin.0 + entry.pixel_size[0] as i32);
    let bottom = (rect.y + rect.h).min(entry_origin.1 + entry.pixel_size[1] as i32);
    if right <= left || bottom <= top {
        return;
    }
    let u_per_px = entry.uv_size[0] / entry.pixel_size[0];
    let v_per_px = entry.uv_size[1] / entry.pixel_size[1];
    out.push(SpriteInstance {
        position: [left as f32, top as f32],
        size: [(right - left) as f32, (bottom - top) as f32],
        uv_origin: [
            entry.uv_origin[0] + (left - entry_origin.0) as f32 * u_per_px,
            entry.uv_origin[1] + (top - entry_origin.1) as f32 * v_per_px,
        ],
        uv_size: [
            (right - left) as f32 * u_per_px,
            (bottom - top) as f32 * v_per_px,
        ],
        depth,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    });
}

fn push_solid(
    out: &mut Vec<SpriteInstance>,
    atlas: &MainMenuShellChromeAtlas,
    rect: RectPx,
    color: [u8; 3],
    depth: f32,
) {
    let Some(white) = atlas.white_pixel else {
        return;
    };
    if rect.w <= 0 || rect.h <= 0 {
        return;
    }
    out.push(SpriteInstance {
        position: [rect.x as f32, rect.y as f32],
        size: [rect.w as f32, rect.h as f32],
        uv_origin: white.uv_origin,
        uv_size: white.uv_size,
        depth,
        tint: rgb(color),
        alpha: 1.0,
        ..Default::default()
    });
}

/// The two offset frame rings around list window `w`, as measured in the
/// native capture. With `R = x + w` and `B = y + h`, the outer ring spans
/// `x-1..=R+1` by `y-1..=B+1` (light top/left, dark bottom/right) and the
/// inner ring spans `x..=R` by `y..=B` with the colors swapped. Each ring's
/// top-right and bottom-left corner takes the average color.
fn push_list_frame(out: &mut Vec<SpriteInstance>, atlas: &MainMenuShellChromeAtlas, w: RectPx) {
    let (left, top) = (w.x, w.y);
    let (right, bottom) = (w.x + w.w, w.y + w.h);
    for (ring, top_left, bottom_right) in [
        (1, LIST_FRAME_LIGHT, LIST_FRAME_DARK),
        (0, LIST_FRAME_DARK, LIST_FRAME_LIGHT),
    ] {
        let (x0, y0) = (left - ring, top - ring);
        let (x1, y1) = (right + ring, bottom + ring);
        // Top edge x0..x1-1, corner at x1; left edge y0..y1-1, corner at y1.
        push_solid(
            out,
            atlas,
            RectPx::new(x0, y0, x1 - x0, 1),
            top_left,
            LIST_DEPTH,
        );
        push_solid(
            out,
            atlas,
            RectPx::new(x0, y0, 1, y1 - y0),
            top_left,
            LIST_DEPTH,
        );
        push_solid(
            out,
            atlas,
            RectPx::new(x1, y0, 1, 1),
            LIST_FRAME_CORNER,
            LIST_DEPTH,
        );
        push_solid(
            out,
            atlas,
            RectPx::new(x0, y1, 1, 1),
            LIST_FRAME_CORNER,
            LIST_DEPTH,
        );
        // Right edge y0+1..=y1, bottom edge x0+1..=x1.
        push_solid(
            out,
            atlas,
            RectPx::new(x1, y0 + 1, 1, y1 - y0),
            bottom_right,
            LIST_DEPTH,
        );
        push_solid(
            out,
            atlas,
            RectPx::new(x0 + 1, y1, x1 - x0, 1),
            bottom_right,
            LIST_DEPTH,
        );
    }
}

/// List interior (`x+1, y+1, w-1, h-1`) and the rows it holds.
fn list_interior(list: RectPx) -> RectPx {
    RectPx::new(list.x + 1, list.y + 1, list.w - 1, list.h - 1)
}

fn list_row(interior: RectPx, visible_row: usize) -> RectPx {
    RectPx::new(
        interior.x,
        interior.y + visible_row as i32 * ROW_HEIGHT,
        interior.w,
        ROW_HEIGHT,
    )
}

fn visible_rows(interior: RectPx) -> usize {
    (interior.h / ROW_HEIGHT).max(0) as usize
}

struct TexturedDraw<'a> {
    texture: &'a BatchTexture,
    instances: Vec<SpriteInstance>,
}

/// One shell composition: textured sprite batches in order, then clipped
/// text, then the optional cursor, presented through the RGB565 presenter
/// with the optional 16-bit surface effects.
#[derive(Default)]
struct ShellComposition<'a> {
    draws: &'a [TexturedDraw<'a>],
    text: &'a [ShellTextDraw],
    cursor: Option<(&'a BatchTexture, SpriteInstance)>,
    effects: Option<SurfaceEffects>,
}

fn encode_shell_pass(
    state: &AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
    label: &str,
    composition: ShellComposition<'_>,
) {
    let ShellComposition {
        draws,
        text,
        cursor,
        effects,
    } = composition;
    let gpu = &state.renderer.gpu;
    let batch = &state.renderer.batch_renderer;
    batch.update_camera(
        gpu,
        gpu.config.width as f32,
        gpu.config.height as f32,
        0.0,
        0.0,
        1.0,
        crate::render::batch::DepthAxis::NONE,
    );
    let buffers: Vec<_> = draws
        .iter()
        .map(|draw| batch.create_instance_buffer(gpu, &draw.instances))
        .collect();
    let text_buffers: Vec<_> = text
        .iter()
        .map(|draw| batch.create_instance_buffer(gpu, &draw.instances))
        .collect();
    let cursor_buffer = cursor.as_ref().and_then(|(_, instance)| {
        batch.create_instance_buffer(gpu, std::slice::from_ref(instance))
    });
    let color = state.renderer.shell_surface_presenter.source_render_view();
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &color,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(crate::app::types::CLEAR_COLOR),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &state.renderer.depth_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    for (draw, buffer) in draws.iter().zip(&buffers) {
        if let Some((buffer, count)) = buffer.as_ref() {
            batch.draw_with_buffer_passthrough(&mut pass, draw.texture, buffer, *count);
        }
    }
    for (draw, buffer) in text.iter().zip(&text_buffers) {
        let Some((buffer, count)) = buffer.as_ref() else {
            continue;
        };
        pass.set_scissor_rect(
            draw.scissor.x,
            draw.scissor.y,
            draw.scissor.w,
            draw.scissor.h,
        );
        batch.draw_with_buffer_passthrough(
            &mut pass,
            state.renderer.bit_font.atlas(),
            buffer,
            *count,
        );
    }
    pass.set_scissor_rect(0, 0, gpu.config.width, gpu.config.height);
    if let (Some((texture, _)), Some((buffer, count))) = (cursor, cursor_buffer.as_ref()) {
        batch.draw_with_buffer_passthrough(&mut pass, texture, buffer, *count);
    }
    drop(pass);
    let presenter = &state.renderer.shell_surface_presenter;
    match effects {
        Some(effects) => {
            presenter.encode_present_with_effects(&gpu.queue, encoder, destination, effects)
        }
        None => presenter.encode_present(encoder, destination),
    }
}

fn shell_cursor(state: &AppState) -> Option<(&BatchTexture, SpriteInstance)> {
    let cursor = state
        .match_state
        .match_presentation
        .software_cursor
        .as_ref()?;
    let sequence = cursor.get(crate::app::types::CursorId::Default)?;
    let frame = crate::app::input::cursor::current_software_cursor_frame(sequence)?;
    let texture = &sequence.frames.first()?.texture;
    Some((
        texture,
        SpriteInstance {
            position: [
                state.match_state.input.cursor_x - sequence.hotspot[0],
                state.match_state.input.cursor_y - sequence.hotspot[1],
            ],
            size: [frame.width, frame.height],
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            depth: CURSOR_DEPTH,
            tint: [1.0, 1.0, 1.0],
            alpha: 1.0,
            ..Default::default()
        },
    ))
}

fn owner_draw_button_label_rect(rect: RectPx, pressed: bool) -> RectPx {
    let (dx, dy) = if pressed { (2, 5) } else { (0, 1) };
    RectPx::new(
        rect.x + dx,
        rect.y + dy,
        (rect.w - 2 - dx).max(0),
        (rect.h - dy).max(0),
    )
}

/// Sprites and labels for dialog `0x129`.
fn movie_list_composition<'a>(
    state: &'a AppState,
    atlas: &MainMenuShellChromeAtlas,
    layout: &MovieListLayout,
) -> (
    Vec<SpriteInstance>,
    Vec<SpriteInstance>,
    Vec<PaintLabel<'a>>,
) {
    let screen_w = layout.page.screen.w;
    let screen_h = layout.page.screen.h;
    let origin =
        crate::app::frontend::main_menu_shell_render::shell_background_origin(screen_w, screen_h);
    let (background, darkened) = if screen_w == 640 {
        (
            atlas.parent_background_640_mnscrns,
            atlas.parent_background_640_mnscrns_list,
        )
    } else {
        (
            atlas.parent_background_large_mnscrnl,
            atlas.parent_background_large_mnscrnl_list,
        )
    };
    let mut sprites = Vec::new();
    if let Some(entry) = background {
        push_entry_crop(
            &mut sprites,
            entry,
            origin,
            RectPx::new(
                origin.0,
                origin.1,
                entry.pixel_size[0] as i32,
                entry.pixel_size[1] as i32,
            ),
            PARENT_BACKGROUND_DEPTH,
        );
    }
    sprites.extend(shell_paint::paint_chrome(
        atlas,
        layout.page.right_panel,
        Some(layout.page.lower_strip),
        screen_w,
    ));

    let list = state.frontend.movie_list.as_ref();
    let interior = list_interior(layout.list);
    if let Some(entry) = darkened {
        push_entry_crop(
            &mut sprites,
            entry,
            origin,
            interior,
            LIST_FILL_DEPTH + 0.00001,
        );
    }
    push_list_frame(&mut sprites, atlas, layout.list);
    let rows = visible_rows(interior);
    let mut labels = Vec::new();
    if let Some(list) = list {
        for (visible, index) in (list.top..list.rows.len()).take(rows).enumerate() {
            let row = list_row(interior, visible);
            if list.selected == Some(index) {
                push_solid(
                    &mut sprites,
                    atlas,
                    row,
                    LIST_SELECTED_FILL,
                    LIST_FILL_DEPTH,
                );
            }
            labels.push(PaintLabel {
                text: resolve_csf(state, list.rows[index].label_key),
                rect: RectPx::new(
                    row.x + LIST_TEXT_INSET,
                    row.y,
                    row.w - LIST_TEXT_INSET,
                    row.h,
                ),
                align: ShellAlign::NONE,
                rgb: SHELL_TEXT_RGB_ENABLED,
                path_a_reveal: None,
            });
        }
    }

    let controller = &state.frontend.shell_controller;
    let active = controller.top_id() == Some(MOVIE_LIST_PAGE.dialog);
    let pressed = active.then(|| controller.pressed()).flatten();
    let hovered = active.then(|| controller.hovered()).flatten();
    let wave = state.frontend.shell_first_paint_slide.as_ref();
    let buttons: Vec<PaintButton> = layout
        .page
        .buttons
        .iter()
        .enumerate()
        .map(|(slot, button)| PaintButton {
            rect: button.rect,
            pressed: pressed == Some(button.id),
            hovered: hovered == Some(button.id),
            enabled: true,
            wave_frame: wave.map(|w| w.sdbtnanm_frame(slot as u32, ButtonGroup::A)),
        })
        .collect();
    let button_sprites = shell_paint::paint_buttons(
        atlas,
        &buttons,
        MOVIE_LIST_BUTTON_POLICY,
        std::time::Instant::now(),
        None,
    );
    for button in &layout.page.buttons {
        let Some(spec) = MOVIE_LIST_PAGE.button(button.id) else {
            continue;
        };
        labels.push(PaintLabel {
            text: resolve_csf(state, spec.csf_key),
            rect: owner_draw_button_label_rect(button.rect, pressed == Some(button.id)),
            align: ShellAlign(ShellAlign::H_CENTER.0 | ShellAlign::V_CENTER.0),
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: None,
        });
    }
    labels.push(PaintLabel {
        text: resolve_csf(state, MOVIE_LIST_PAGE.title_key),
        rect: layout.page.title,
        align: ShellAlign::H_CENTER,
        rgb: SHELL_TEXT_RGB_ENABLED,
        path_a_reveal: None,
    });
    labels.push(PaintLabel {
        text: resolve_csf(state, MOVIE_LIST_PROMPT_KEY),
        rect: layout.prompt,
        align: ShellAlign::H_CENTER,
        rgb: SHELL_TEXT_RGB_ENABLED,
        path_a_reveal: None,
    });
    let status_key = match hovered {
        Some(MOVIE_LIST_CONTROL) => Some(MOVIE_LIST_TOOLTIP_KEY),
        Some(id) => MOVIE_LIST_PAGE.button(id).map(|button| button.tooltip_key),
        None => None,
    };
    if let Some(key) = status_key {
        labels.push(PaintLabel {
            text: resolve_csf(state, key),
            rect: layout.page.status_help,
            align: ShellAlign::V_CENTER,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: None,
        });
    }
    (sprites, button_sprites, labels)
}

/// Paint dialog `0x129`. Returns `false` when the shell chrome is missing.
pub(crate) fn render_movie_list(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<bool> {
    let Some(atlas) = state.frontend.main_menu_shell_chrome.as_ref() else {
        return Ok(false);
    };
    let layout = compute_movie_list_layout(
        state.renderer.gpu.config.width,
        state.renderer.gpu.config.height,
    );
    let (sprites, button_sprites, labels) = movie_list_composition(state, atlas, &layout);
    let text = shell_paint::paint_labels(&state.renderer.bit_font, &labels);
    let draws = [
        TexturedDraw {
            texture: &atlas.texture,
            instances: sprites,
        },
        TexturedDraw {
            texture: &atlas.texture,
            instances: button_sprites,
        },
    ];
    encode_shell_pass(
        state,
        encoder,
        destination,
        "Movie List Shell",
        ShellComposition {
            draws: &draws,
            text: &text,
            cursor: shell_cursor(state),
            effects: None,
        },
    );
    Ok(true)
}

/// A cleared black frame: Play_Movie's closing clear (`arg1 == 1`) and the
/// end of Show_Credits, shown for the frame on which the presentation ends.
pub(crate) fn render_fullscreen_black(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) {
    encode_shell_pass(
        state,
        encoder,
        destination,
        "Shell black",
        ShellComposition::default(),
    );
}

/// Paint the active full-screen movie on black, without a cursor.
pub(crate) fn render_fullscreen_movie(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<()> {
    let Some(movie) = state.frontend.fullscreen_movie.as_ref() else {
        return Ok(());
    };
    let instance = movie.instance(
        state.renderer.gpu.config.width as i32,
        state.renderer.gpu.config.height as i32,
    );
    let draws = [TexturedDraw {
        texture: movie.surface().batch_texture(),
        instances: vec![instance],
    }];
    encode_shell_pass(
        state,
        encoder,
        destination,
        "Play_Movie",
        ShellComposition {
            draws: &draws,
            ..ShellComposition::default()
        },
    );
    Ok(())
}

/// Paint the credits roll on black with the 16-bit band fade, no cursor.
pub(crate) fn render_credits_roll(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<()> {
    let Some(session) = state.frontend.credits_roll.as_ref() else {
        return Ok(());
    };
    let font = &state.renderer.bit_font;
    let instances = session
        .roll
        .instances(font, state.renderer.gpu.config.width as i32);
    let text = [ShellTextDraw {
        instances,
        scissor: crate::render::shell_text::ScissorRect {
            x: 0,
            y: 0,
            w: state.renderer.gpu.config.width,
            h: state.renderer.gpu.config.height,
        },
    }];
    encode_shell_pass(
        state,
        encoder,
        destination,
        "Show_Credits",
        ShellComposition {
            text: &text,
            effects: Some(SurfaceEffects {
                fade_rows: crate::app::frontend::credits_roll::CREDITS_FADE_ROWS,
            }),
            ..ShellComposition::default()
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_interior_and_rows_match_the_native_capture() {
        let list = RectPx::new(120, 128, 399, 304);
        let interior = list_interior(list);
        // Selection fill measured at x 121..=518, y 129..=147.
        assert_eq!(interior, RectPx::new(121, 129, 398, 303));
        assert_eq!(list_row(interior, 0), RectPx::new(121, 129, 398, 19));
        assert_eq!(visible_rows(interior), 15);
    }
}
