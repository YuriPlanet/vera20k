//! Shared paint helpers for the front-end shell renderers: CSF caption
//! lookup, the owner-draw button label clip, the software cursor sprite and
//! one RGB565-presented composition pass.

use crate::app::AppState;
use crate::render::batch::{BatchTexture, SpriteInstance};
use crate::render::shell_surface_present::SurfaceEffects;
use crate::render::shell_text::ShellTextDraw;
use crate::ui::shell::geom::RectPx;

impl AppState {
    /// [`Self::csf_label`] with the key itself standing in while no table is
    /// loaded (a missing label resolves to the CSF's `MISSING:'key'` text).
    pub(crate) fn csf_text(&self, key: &str) -> String {
        self.csf_label(key, key)
    }

    /// A label in the language in use ([`crate::i18n::label`]): VERA's own
    /// (`VERA:`) come from its language files, the game's from the loaded
    /// table, and `fallback` stands in while none is loaded.
    pub(crate) fn csf_label(&self, key: &str, fallback: &str) -> String {
        crate::i18n::label(self.process_assets.csf.as_ref(), key, fallback)
    }
}

/// Native owner-draw button label clip: unpressed `(x, y+1, w-2, h-1)`,
/// pressed `(x+2, y+5, w-4, h-5)`.
pub(crate) fn owner_draw_button_label_rect(rect: RectPx, pressed: bool) -> RectPx {
    let (dx, dy) = if pressed { (2, 5) } else { (0, 1) };
    RectPx::new(
        rect.x + dx,
        rect.y + dy,
        (rect.w - 2 - dx).max(0),
        (rect.h - dy).max(0),
    )
}

/// The cursor a page draws. A message box over the page draws it above itself
/// instead ([`top_software_cursor`]).
pub(crate) fn software_cursor(
    state: &AppState,
    depth: f32,
) -> Option<(&BatchTexture, SpriteInstance)> {
    use crate::app::input::precedence::{ModalPage, modal_page};
    if modal_page(state) == Some(ModalPage::ShellMessageBox) {
        return None;
    }
    top_software_cursor(state, depth)
}

/// Default software cursor in screen space (camera at the origin): the raw
/// pointer minus the hotspot, sized to the current animation frame and grown
/// with a fitted menu's art, drawn from the sequence's frame-0 texture. `None`
/// without a software cursor.
pub(crate) fn top_software_cursor(
    state: &AppState,
    depth: f32,
) -> Option<(&BatchTexture, SpriteInstance)> {
    // Outside the client area, or over an inactive window, the system arrow
    // owns the pointer; mouse.shp would otherwise stay painted next to it.
    if !state.platform.owns_pointer() {
        return None;
    }
    let cursor = state
        .match_state
        .match_presentation
        .software_cursor
        .as_ref()?;
    // Shell screens (menu, skirmish setup, score) only ever show the default
    // arrow, which is a single static frame.
    let sequence = cursor.get(crate::app::types::CursorId::Default)?;
    let frame = sequence.frames.get(
        state
            .match_state
            .input
            .software_cursor_frame_index(crate::app::types::CursorId::Default, sequence)?,
    )?;
    if crate::app::input::hardware_cursor::takes_cursor(
        state,
        crate::app::types::CursorId::Default,
        0,
    ) {
        return None;
    }
    let texture = &sequence.frames.first()?.texture;
    let scale = crate::app::frontend::shell_layout::menu_cursor_scale(state);
    Some((
        texture,
        SpriteInstance {
            position: [
                state.match_state.input.cursor_x - sequence.hotspot[0] * scale,
                state.match_state.input.cursor_y - sequence.hotspot[1] * scale,
            ],
            size: [frame.width * scale, frame.height * scale],
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            depth,
            tint: [1.0, 1.0, 1.0],
            alpha: 1.0,
            ..Default::default()
        },
    ))
}

/// Sprites that share one texture.
pub(crate) struct TexturedDraw<'a> {
    pub(crate) texture: &'a BatchTexture,
    pub(crate) instances: Vec<SpriteInstance>,
}

/// One shell composition: textured sprite batches in order, then clipped
/// text, then the optional cursor, presented through the RGB565 presenter
/// with the optional 16-bit surface effects.
#[derive(Default)]
pub(crate) struct ShellComposition<'a> {
    pub(crate) draws: &'a [TexturedDraw<'a>],
    pub(crate) text: &'a [ShellTextDraw],
    pub(crate) cursor: Option<(&'a BatchTexture, SpriteInstance)>,
    pub(crate) effects: Option<SurfaceEffects>,
    /// `draws` go through VERA's online client's smoothing
    /// ([`crate::render::batch::BatchRenderer::draw_chrome`]).
    pub(crate) smooth_art: bool,
}

/// Encode `composition` into the shell source surface and present it to
/// `destination`.
pub(crate) fn encode_shell_pass(
    state: &AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
    label: &str,
    composition: ShellComposition<'_>,
) {
    encode_composition(
        state,
        encoder,
        destination,
        label,
        composition,
        wgpu::LoadOp::Clear(crate::app::types::CLEAR_COLOR),
    );
}

/// Encode `composition` over what the shell source surface already holds and
/// present the result to `destination`.
pub(crate) fn encode_shell_pass_over_page(
    state: &AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
    label: &str,
    composition: ShellComposition<'_>,
) {
    encode_composition(
        state,
        encoder,
        destination,
        label,
        composition,
        wgpu::LoadOp::Load,
    );
}

fn encode_composition(
    state: &AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
    label: &str,
    composition: ShellComposition<'_>,
    color_load: wgpu::LoadOp<wgpu::Color>,
) {
    let ShellComposition {
        draws,
        text,
        cursor,
        effects,
        smooth_art,
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
                load: color_load,
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
        multiview_mask: None,
    });
    for (draw, buffer) in draws.iter().zip(&buffers) {
        if let Some((buffer, count)) = buffer.as_ref() {
            batch.draw_chrome(&mut pass, draw.texture, buffer, *count, smooth_art);
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
    drop(pass);
    let presenter = &state.renderer.shell_surface_presenter;
    presenter.stage_cursor_free_composition(
        &state.renderer.retail_screenshot_frame_cache,
        &gpu.device,
        encoder,
        (gpu.config.width, gpu.config.height),
    );
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Shell cursor after retained composition"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &color,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &state.renderer.depth_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let (Some((texture, _)), Some((buffer, count))) = (cursor, cursor_buffer.as_ref()) {
        batch.draw_with_buffer_passthrough(&mut pass, texture, buffer, *count);
    }
    drop(pass);
    match effects {
        Some(effects) => {
            presenter.encode_present_with_effects(&gpu.queue, encoder, destination, effects)
        }
        None => presenter.encode_present(encoder, destination),
    }
}

/// Sprite batches drawn in order over a black clear onto `view` through the
/// passthrough pipeline at the window's pixel camera, with no shell surface:
/// the status card and the startup splash, which must draw without the
/// shell's chrome or the app state.
pub(crate) fn encode_sprite_pass(
    gpu: &crate::render::gpu::GpuContext,
    batch: &crate::render::batch::BatchRenderer,
    encoder: &mut wgpu::CommandEncoder,
    targets: (&wgpu::TextureView, &wgpu::TextureView),
    label: &str,
    draws: &[TexturedDraw<'_>],
) {
    let (width, height) = (gpu.config.width as f32, gpu.config.height as f32);
    let flat = crate::render::batch::DepthAxis::NONE;
    batch.update_camera(gpu, width, height, 0.0, 0.0, 1.0, flat);
    let buffers: Vec<_> = draws
        .iter()
        .map(|draw| batch.create_instance_buffer(gpu, &draw.instances))
        .collect();
    let (view, depth) = targets;
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        // The passthrough pipeline declares Depth32Float even though it
        // compares Always and does not write depth.
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    for (draw, buffer) in draws.iter().zip(&buffers) {
        if let Some((buffer, count)) = buffer {
            batch.draw_with_buffer_passthrough(&mut pass, draw.texture, buffer, *count);
        }
    }
}

#[cfg(all(test, tests_app_frontend))]
mod tests {
    use super::*;

    #[test]
    fn owner_draw_label_clip_matches_native_up_and_pressed_rects() {
        let button = RectPx::new(644, 199, 156, 42);
        assert_eq!(
            owner_draw_button_label_rect(button, false),
            RectPx::new(644, 200, 154, 41)
        );
        assert_eq!(
            owner_draw_button_label_rect(button, true),
            RectPx::new(646, 204, 152, 37)
        );
    }
}
