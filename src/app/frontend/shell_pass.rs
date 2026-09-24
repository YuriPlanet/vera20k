//! Shared paint helpers for the front-end shell renderers: CSF caption
//! lookup, the owner-draw button label clip, the software cursor sprite and
//! one RGB565-presented composition pass.

use std::borrow::Cow;

use crate::app::AppState;
use crate::render::batch::{BatchTexture, SpriteInstance};
use crate::render::shell_surface_present::SurfaceEffects;
use crate::render::shell_text::ShellTextDraw;
use crate::ui::shell::geom::RectPx;

/// CSF text for `key`; the key itself when no string table is loaded (a
/// missing label resolves to the CSF's `MISSING:'key'` text).
pub(crate) fn resolve_csf<'a>(state: &'a AppState, key: &'a str) -> Cow<'a, str> {
    state
        .process_assets
        .csf
        .as_ref()
        .map_or(Cow::Borrowed(key), |csf| csf.text(key))
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

/// Default software cursor in screen space (camera at the origin): the raw
/// pointer minus the hotspot, sized to the current animation frame, drawn
/// from the sequence's frame-0 texture. `None` without a software cursor.
pub(crate) fn software_cursor(
    state: &AppState,
    depth: f32,
) -> Option<(&BatchTexture, SpriteInstance)> {
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

#[cfg(test)]
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
