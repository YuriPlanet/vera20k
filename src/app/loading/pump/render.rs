//! Loading frame drawing: layout, atlas instances, text, encoding and presentation.
//!
//! The parent owns attempt state and progress policy. This child exposes only
//! complete drawing operations; layout helpers remain private.

use super::{LoadingProgressState, NativeLoadingScreenState};
use crate::app::loading::composition::{
    LoadingCompositionSnapshot, MmpbRegionRect, loading_base_origin,
};
use crate::app::loading::progress_row::{
    LoadingProgressRowLayout, LoadingProgressRowSnapshot, layout_standard_skirmish_progress_row,
};
use crate::app::renderer_state::RendererState;
use crate::render::batch::{BatchRenderer, SpriteInstance};
use crate::render::bit_font::BitFont;
use crate::render::draw_state::DrawState;
use crate::render::gpu::GpuContext;
use crate::render::loading_screen_chrome::{LoadingScreenAtlas, LoadingScreenEntry};
use crate::render::shell_surface_present::ShellSurfacePresenter;
use crate::render::shell_text::{ScissorRect, ShellAlign, ShellTextDraw, TextRect, draw_in_rect};
use crate::rules::color_scheme::scheme_entry_for_priority;

const BACKGROUND_DEPTH: f32 = 0.90;
const PREVIEW_DEPTH: f32 = 0.80;
const MARKER_DEPTH: f32 = 0.70;
const TEXT_BACKING_DEPTH: f32 = 0.60;
const TEXT_DEPTH: f32 = 0.50;
const TEXT_BACKING_ALPHA: f32 = 159.0 / 255.0;
const TEXT_BACKING_PADDING: f32 = 2.0;
/// Solid backing fill (G3) sits just behind the bar so the bar draws over it.
const SOLID_FILL_DEPTH: f32 = 0.20;
const PROGRESS_DEPTH: f32 = 0.10;
/// Side icon (G4) draws above the background, at the bar's depth.
const SIDE_ICON_DEPTH: f32 = 0.10;
/// Row label follows the bar and country insignia.
const ROW_LABEL_DEPTH: f32 = 0.05;

/// A black frame with no cursor: gamemd fills its hidden surface black and
/// blits it between the closed shell and the loading screen
/// (`0x0052E64C..0x0052E69E`) and again in the game mode after the load
/// (`0x00683E07..0x00683E1C`).
fn encode_blank(
    presenter: &ShellSurfacePresenter,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) {
    let target = presenter.source_render_view();
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Loading Blank"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(crate::app::types::CLEAR_COLOR),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    presenter.encode_present(encoder, destination);
}

/// The blank between the closed shell and the loading screen, in the frame
/// loop.
pub(super) fn encode_blank_loading_frame(
    renderer: &RendererState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) {
    encode_blank(&renderer.shell_surface_presenter, encoder, destination);
}

/// Present the blank at once, outside the frame loop.
pub(super) fn present_blank(
    gpu: &GpuContext,
    presenter: &ShellSurfacePresenter,
) -> anyhow::Result<()> {
    let output = gpu
        .surface
        .get_current_texture()
        .map_err(|e| anyhow::anyhow!("blank frame surface texture: {e}"))?;
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Game Mode Blank"),
        });
    encode_blank(presenter, &mut encoder, &output.texture);
    gpu.queue.submit(std::iter::once(encoder.finish()));
    output.present();
    Ok(())
}

/// Encode the prepared native frame into the caller's existing frame submission.
pub(super) fn encode_native_loading_frame(
    renderer: &RendererState,
    native: &NativeLoadingScreenState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let Some(atlas) = native.atlas.as_ref() else {
        return Err(anyhow::anyhow!(
            "native Skirmish loading atlas was not available for render"
        ));
    };
    let target = renderer.shell_surface_presenter.source_render_view();

    let frame_plan = build_native_loading_frame_plan(
        &renderer.bit_font,
        atlas,
        native.composition.as_ref(),
        &native.progress_row,
        &native.progress,
        native.backing_rgb,
        native.text_rgb,
        [renderer.gpu.config.width, renderer.gpu.config.height],
    );
    let instances = frame_plan.instances;
    let text_draws = frame_plan.text_draws;

    renderer.batch_renderer.update_camera(
        &renderer.gpu,
        renderer.gpu.config.width as f32,
        renderer.gpu.config.height as f32,
        0.0,
        0.0,
        1.0,
        crate::render::batch::DepthAxis::NONE,
    );
    let Some((buffer, count)) = renderer
        .batch_renderer
        .create_instance_buffer(&renderer.gpu, &instances)
    else {
        return Err(anyhow::anyhow!(
            "native Skirmish loading instances could not be uploaded"
        ));
    };
    let backing_buffers = text_draws
        .iter()
        .map(|draw| {
            renderer
                .batch_renderer
                .create_instance_buffer(&renderer.gpu, &draw.backing)
        })
        .collect::<Vec<_>>();
    let text_buffers = text_draws
        .iter()
        .map(|draw| {
            renderer
                .batch_renderer
                .create_instance_buffer(&renderer.gpu, &draw.text.instances)
        })
        .collect::<Vec<_>>();

    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Native Loading Screen"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(crate::app::types::CLEAR_COLOR),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &renderer.depth_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    renderer
        .batch_renderer
        .draw_with_buffer_passthrough(&mut pass, &atlas.texture, &buffer, count);
    for ((draw, backing_buffer), text_buffer) in text_draws
        .iter()
        .zip(backing_buffers.iter())
        .zip(text_buffers.iter())
    {
        if let Some((buffer, count)) = backing_buffer.as_ref() {
            renderer.batch_renderer.draw_with_buffer_passthrough(
                &mut pass,
                &atlas.texture,
                buffer,
                *count,
            );
        }
        let Some((buffer, count)) = text_buffer.as_ref() else {
            continue;
        };
        let Some(scissor) = clamp_loading_scissor(
            draw.text.scissor,
            renderer.gpu.config.width,
            renderer.gpu.config.height,
        ) else {
            continue;
        };
        pass.set_scissor_rect(scissor.x, scissor.y, scissor.w, scissor.h);
        renderer.batch_renderer.draw_with_buffer_passthrough(
            &mut pass,
            renderer.bit_font.atlas(),
            buffer,
            *count,
        );
    }
    pass.set_scissor_rect(0, 0, renderer.gpu.config.width, renderer.gpu.config.height);
    drop(pass);
    renderer
        .shell_surface_presenter
        .encode_present(encoder, destination);
    Ok(())
}

struct NativeLoadingTextDraw {
    backing: Vec<SpriteInstance>,
    text: ShellTextDraw,
}

struct NativeLoadingFramePlan {
    instances: Vec<SpriteInstance>,
    text_draws: Vec<NativeLoadingTextDraw>,
}

fn native_loading_row_layout(
    font: &BitFont,
    atlas: &LoadingScreenAtlas,
    progress: &LoadingProgressState,
    render_size: [u32; 2],
) -> Option<LoadingProgressRowLayout> {
    if progress.current_value() == 0.0 {
        return None;
    }
    Some(layout_standard_skirmish_progress_row(
        render_size,
        [
            atlas.progress_frame0.pixel_size[0] as i32,
            atlas.progress_frame0.pixel_size[1] as i32,
        ],
        atlas
            .side_icon
            .map(|icon| [icon.pixel_size[0] as i32, icon.pixel_size[1] as i32]),
        font.cell_height() as i32,
    ))
}

fn build_native_loading_text_draws(
    font: &BitFont,
    atlas: &LoadingScreenAtlas,
    composition: Option<&LoadingCompositionSnapshot>,
    row: &LoadingProgressRowSnapshot,
    row_layout: Option<&LoadingProgressRowLayout>,
    text_rgb: [f32; 3],
    row_rgb: [f32; 3],
) -> Vec<NativeLoadingTextDraw> {
    let mut draws = Vec::with_capacity(5);
    if let Some(composition) = composition {
        if let Some(text) = composition.text.country_name.as_deref() {
            draws.push(build_native_loading_text_draw(
                font,
                atlas,
                text,
                composition.text_rects.country_name,
                text_rgb,
                ShellAlign::H_RIGHT,
                true,
                TEXT_DEPTH,
            ));
        }
        if let Some(text) = composition.text.special_unit.as_deref() {
            draws.push(build_native_loading_text_draw(
                font,
                atlas,
                text,
                composition.text_rects.special_unit,
                [0.0, 0.0, 0.0],
                ShellAlign::NONE,
                false,
                TEXT_DEPTH,
            ));
        }
        if let Some(text) = composition.text.load_brief.as_deref() {
            draws.push(build_native_loading_text_draw(
                font,
                atlas,
                text,
                composition.text_rects.load_brief,
                text_rgb,
                ShellAlign::NONE,
                true,
                TEXT_DEPTH,
            ));
        }
        if let Some(text) = composition.text.loading.as_deref() {
            draws.push(build_native_loading_text_draw(
                font,
                atlas,
                text,
                composition.text_rects.loading,
                text_rgb,
                ShellAlign::NONE,
                true,
                TEXT_DEPTH,
            ));
        }
    }
    if let Some(layout) = row_layout
        && !row.label.is_empty()
        && layout.label_rect.w > 0
        && layout.label_rect.h > 0
    {
        draws.push(build_native_loading_text_draw(
            font,
            atlas,
            &row.label,
            layout.label_rect,
            row_rgb,
            ShellAlign::NONE,
            false,
            ROW_LABEL_DEPTH,
        ));
    }
    draws
}

#[allow(clippy::too_many_arguments)]
fn build_native_loading_text_draw(
    font: &BitFont,
    atlas: &LoadingScreenAtlas,
    text: &str,
    rect: crate::ui::shell::geom::RectPx,
    color: [f32; 3],
    align: ShellAlign,
    with_backing: bool,
    depth: f32,
) -> NativeLoadingTextDraw {
    let width = rect.w.max(0) as u32;
    let height = rect.h.max(0) as u32;
    let text_rect = TextRect {
        x: rect.x,
        y: rect.y,
        w: width,
        h: height,
    };
    let text_draw = draw_in_rect(font, text, text_rect, color, align, [0.0, 0.0], depth);
    let mut backing = Vec::new();
    if with_backing && !text_draw.instances.is_empty() {
        let layout = font.wrap_layout(text, width);
        let aligned_x = if align.contains(ShellAlign::H_RIGHT) && layout.width < width {
            rect.x + (width - layout.width) as i32
        } else if align.contains(ShellAlign::H_CENTER) && layout.width < width {
            rect.x + ((width - layout.width) / 2) as i32
        } else {
            rect.x
        };
        push_entry_tinted(
            &mut backing,
            atlas.solid_texel,
            [
                aligned_x as f32 - TEXT_BACKING_PADDING,
                rect.y as f32 - TEXT_BACKING_PADDING,
            ],
            [
                layout.width as f32 + TEXT_BACKING_PADDING * 2.0,
                layout.height.min(height) as f32 + TEXT_BACKING_PADDING * 2.0,
            ],
            TEXT_BACKING_DEPTH,
            [0.0, 0.0, 0.0],
        );
        if let Some(instance) = backing.last_mut() {
            instance.alpha = TEXT_BACKING_ALPHA;
        }
    }
    NativeLoadingTextDraw {
        backing,
        text: text_draw,
    }
}

fn clamp_loading_scissor(
    scissor: ScissorRect,
    render_width: u32,
    render_height: u32,
) -> Option<ScissorRect> {
    let x = scissor.x.min(render_width);
    let y = scissor.y.min(render_height);
    let w = scissor.w.min(render_width.saturating_sub(x));
    let h = scissor.h.min(render_height.saturating_sub(y));
    (w > 0 && h > 0).then_some(ScissorRect { x, y, w, h })
}

/// Build the full native loading-screen instance list (background, solid backing
/// fill, clipped progress bar, side icon) shared by the per-frame render path and
/// the synchronous-repaint sink.
fn build_native_loading_instances(
    atlas: &LoadingScreenAtlas,
    composition: Option<&LoadingCompositionSnapshot>,
    progress: &LoadingProgressState,
    backing_rgb: [f32; 3],
    row_layout: Option<&LoadingProgressRowLayout>,
    base_origin: [i32; 2],
) -> Vec<SpriteInstance> {
    let mut instances = Vec::with_capacity(12);
    // The art hangs off the same base origin as the progress row and the text
    // layers, so an oversized window centers all three together.
    push_entry(
        &mut instances,
        atlas.background,
        [base_origin[0] as f32, base_origin[1] as f32],
        BACKGROUND_DEPTH,
    );

    if let Some(composition) = composition {
        if let (Some(prepared), Some(preview_entry)) = (composition.preview.as_ref(), atlas.preview)
        {
            // gamemd blits the source preview into the fitted destination rect,
            // resampling the whole image. The aspect fit has already chosen a
            // destination that preserves the source ratio, so both axes scale;
            // clipping either one here would cut the map's edge off.
            push_entry_scaled(
                &mut instances,
                preview_entry,
                [
                    (prepared.region.x + prepared.fit.pad_x) as f32,
                    (prepared.region.y + prepared.fit.pad_y) as f32,
                ],
                [prepared.fit.width as f32, prepared.fit.height as f32],
                PREVIEW_DEPTH,
                [1.0; 3],
            );
        }
        // Markers only exist alongside a preview, and they are cropped to that
        // preview's region for the same reason gamemd composes them into a
        // region-sized surface before blitting it.
        if let Some(prepared) = composition.preview.as_ref() {
            for marker in &composition.markers {
                let color_key = scheme_entry_for_priority(i32::from(marker.color_priority)) as u8;
                let Some(entry) = atlas.mmpb_markers.get(&color_key).copied() else {
                    continue;
                };
                push_entry_clipped(
                    &mut instances,
                    entry,
                    [marker.anchor.screen_x, marker.anchor.screen_y],
                    MARKER_DEPTH,
                    prepared.region,
                );
            }
        }
    }

    // The LS renderer's compose-only state owns no progress row. The selected-map
    // path advances to 3 before the first confirmed display blit.
    if progress.current_value() == 0.0 {
        return instances;
    }

    let Some(row_layout) = row_layout else {
        return instances;
    };
    let bar_w = atlas.progress_frame0.pixel_size[0];
    let bar_h = atlas.progress_frame0.pixel_size[1];
    let bar_origin = [
        row_layout.bar_origin[0] as f32,
        row_layout.bar_origin[1] as f32,
    ];

    // G3: solid backing fill — full bar frame rect (W x H), filled with the
    // player scheme's `[Colors]` HSV→RGB color, drawn BEFORE the clipped bar so
    // the bar covers it.
    push_entry_tinted(
        &mut instances,
        atlas.solid_texel,
        bar_origin,
        [bar_w, bar_h],
        SOLID_FILL_DEPTH,
        backing_rgb,
    );

    // G2: clipped progress span. The session atlas already contains the
    // player's 16-shade remap, so preserve its per-pixel colors.
    let progress_width = progress.fill_width_gamemd_ftol_positive_domain(bar_w as u32);
    if progress_width > 0 {
        push_progress_fill(
            &mut instances,
            atlas.progress_frame0,
            bar_origin,
            progress_width as f32,
            PROGRESS_DEPTH,
        );
    }

    // Country insignia follows the progress span. The atlas has already applied
    // the verified RGB-magenta key.
    if let (Some(icon), Some(icon_origin)) = (atlas.side_icon, row_layout.icon_origin) {
        push_entry(
            &mut instances,
            icon,
            [icon_origin[0] as f32, icon_origin[1] as f32],
            SIDE_ICON_DEPTH,
        );
    }

    instances
}

#[allow(clippy::too_many_arguments)]
fn build_native_loading_frame_plan(
    font: &BitFont,
    atlas: &LoadingScreenAtlas,
    composition: Option<&LoadingCompositionSnapshot>,
    progress_row: &LoadingProgressRowSnapshot,
    progress: &LoadingProgressState,
    backing_rgb: [f32; 3],
    text_rgb: [f32; 3],
    render_size: [u32; 2],
) -> NativeLoadingFramePlan {
    let row_layout = native_loading_row_layout(font, atlas, progress, render_size);
    let instances = build_native_loading_instances(
        atlas,
        composition,
        progress,
        backing_rgb,
        row_layout.as_ref(),
        loading_base_origin(render_size),
    );
    let text_draws = build_native_loading_text_draws(
        font,
        atlas,
        composition,
        progress_row,
        row_layout.as_ref(),
        text_rgb,
        backing_rgb,
    );
    NativeLoadingFramePlan {
        instances,
        text_draws,
    }
}

/// Acquire a surface frame, render the native loading screen, and present it.
///
/// Used by the synchronous-repaint sink to mirror gamemd's per-milestone
/// `WM_PAINT`. All wgpu ops take `&self`, so only shared references are needed.
/// Returns an error on acquire/upload failure; the caller treats it as non-fatal.
pub(super) fn present_native_loading(
    gpu: &GpuContext,
    presenter: &ShellSurfacePresenter,
    depth_view: &wgpu::TextureView,
    batch: &BatchRenderer,
    font: &BitFont,
    atlas: &LoadingScreenAtlas,
    composition: Option<&LoadingCompositionSnapshot>,
    progress_row: &LoadingProgressRowSnapshot,
    progress: &LoadingProgressState,
    backing_rgb: [f32; 3],
    text_rgb: [f32; 3],
    render_size: [u32; 2],
) -> anyhow::Result<()> {
    let output = gpu
        .surface
        .get_current_texture()
        .map_err(|e| anyhow::anyhow!("loading repaint surface texture: {e}"))?;
    let view = presenter.source_render_view();
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Native Loading Repaint"),
        });

    let frame_plan = build_native_loading_frame_plan(
        font,
        atlas,
        composition,
        progress_row,
        progress,
        backing_rgb,
        text_rgb,
        render_size,
    );
    let instances = frame_plan.instances;
    let text_draws = frame_plan.text_draws;
    batch.update_camera(
        gpu,
        gpu.config.width as f32,
        gpu.config.height as f32,
        0.0,
        0.0,
        1.0,
        crate::render::batch::DepthAxis::NONE,
    );
    let Some((buffer, count)) = batch.create_instance_buffer(gpu, &instances) else {
        return Err(anyhow::anyhow!(
            "loading repaint instances could not be uploaded"
        ));
    };
    let backing_buffers = text_draws
        .iter()
        .map(|draw| batch.create_instance_buffer(gpu, &draw.backing))
        .collect::<Vec<_>>();
    let text_buffers = text_draws
        .iter()
        .map(|draw| batch.create_instance_buffer(gpu, &draw.text.instances))
        .collect::<Vec<_>>();

    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Native Loading Repaint"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(crate::app::types::CLEAR_COLOR),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        batch.draw_with_buffer_passthrough(&mut pass, &atlas.texture, &buffer, count);
        for ((draw, backing_buffer), text_buffer) in text_draws
            .iter()
            .zip(backing_buffers.iter())
            .zip(text_buffers.iter())
        {
            if let Some((buffer, count)) = backing_buffer.as_ref() {
                batch.draw_with_buffer_passthrough(&mut pass, &atlas.texture, buffer, *count);
            }
            let Some((buffer, count)) = text_buffer.as_ref() else {
                continue;
            };
            let Some(scissor) =
                clamp_loading_scissor(draw.text.scissor, gpu.config.width, gpu.config.height)
            else {
                continue;
            };
            pass.set_scissor_rect(scissor.x, scissor.y, scissor.w, scissor.h);
            batch.draw_with_buffer_passthrough(&mut pass, font.atlas(), buffer, *count);
        }
        pass.set_scissor_rect(0, 0, gpu.config.width, gpu.config.height);
    }

    presenter.encode_present(&mut encoder, &output.texture);
    gpu.queue.submit(std::iter::once(encoder.finish()));
    output.present();
    Ok(())
}

fn push_entry(
    out: &mut Vec<SpriteInstance>,
    entry: LoadingScreenEntry,
    position: [f32; 2],
    depth: f32,
) {
    push_entry_scaled(out, entry, position, entry.pixel_size, depth, [1.0; 3]);
}

/// Push a quad that resamples the whole source into `size`.
///
/// This is the ordinary scaling blit: the full atlas slot is sampled across the
/// destination rect, so a destination smaller than the source squashes the image
/// instead of cutting pieces off it.
fn push_entry_scaled(
    out: &mut Vec<SpriteInstance>,
    entry: LoadingScreenEntry,
    position: [f32; 2],
    size: [f32; 2],
    depth: f32,
    tint: [f32; 3],
) {
    out.push(SpriteInstance {
        position,
        size,
        uv_origin: entry.uv_origin,
        uv_size: entry.uv_size,
        depth,
        tint,
        alpha: 1.0,
        draw_state: DrawState::default(),
        ..Default::default()
    });
}

/// Push a quad cropped to the preview region, dropping it when nothing is left.
///
/// gamemd composes the start markers into a surface exactly the size of the
/// preview region and blits that surface, so a marker whose nudge pushes it past
/// an edge is cut off there instead of spilling onto the loading art.
fn push_entry_clipped(
    out: &mut Vec<SpriteInstance>,
    entry: LoadingScreenEntry,
    position: [i32; 2],
    depth: f32,
    clip: MmpbRegionRect,
) {
    let width = entry.pixel_size[0] as i32;
    let height = entry.pixel_size[1] as i32;
    if width <= 0 || height <= 0 {
        return;
    }
    let left = position[0].max(clip.x);
    let top = position[1].max(clip.y);
    let right = (position[0] + width).min(clip.x + clip.width);
    let bottom = (position[1] + height).min(clip.y + clip.height);
    if right <= left || bottom <= top {
        return;
    }

    let visible = [(right - left) as f32, (bottom - top) as f32];
    let cropped = [(left - position[0]) as f32, (top - position[1]) as f32];
    out.push(SpriteInstance {
        position: [left as f32, top as f32],
        size: visible,
        uv_origin: [
            entry.uv_origin[0] + entry.uv_size[0] * cropped[0] / entry.pixel_size[0],
            entry.uv_origin[1] + entry.uv_size[1] * cropped[1] / entry.pixel_size[1],
        ],
        uv_size: [
            entry.uv_size[0] * visible[0] / entry.pixel_size[0],
            entry.uv_size[1] * visible[1] / entry.pixel_size[1],
        ],
        depth,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        draw_state: DrawState::default(),
        ..Default::default()
    });
}

fn push_entry_tinted(
    out: &mut Vec<SpriteInstance>,
    entry: LoadingScreenEntry,
    position: [f32; 2],
    size: [f32; 2],
    depth: f32,
    tint: [f32; 3],
) {
    push_entry_scaled(out, entry, position, size, depth, tint);
}

/// Push the progress bar's filled span: `PROGBARM.SHP` frame 0 revealed from the
/// left, full height.
///
/// This is the one loading-screen layer that is *clipped* rather than scaled —
/// the bar sweeps by uncovering more of the same frame, so the U axis is cut at
/// the fill width while the V axis stays whole. Every other layer scales.
fn push_progress_fill(
    out: &mut Vec<SpriteInstance>,
    entry: LoadingScreenEntry,
    position: [f32; 2],
    fill_width: f32,
    depth: f32,
) {
    out.push(SpriteInstance {
        position,
        size: [fill_width, entry.pixel_size[1]],
        uv_origin: entry.uv_origin,
        uv_size: [
            entry.uv_size[0] * (fill_width / entry.pixel_size[0]).clamp(0.0, 1.0),
            entry.uv_size[1],
        ],
        depth,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        draw_state: DrawState::default(),
        ..Default::default()
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic `mmpb.shp` frame-0 atlas slot: 12x12 pixels somewhere inside a
    /// shared atlas, so cropping has to move both the UV origin and the UV size.
    fn marker_entry() -> LoadingScreenEntry {
        LoadingScreenEntry {
            uv_origin: [0.25, 0.5],
            uv_size: [0.1, 0.2],
            pixel_size: [12.0, 12.0],
        }
    }

    #[test]
    fn a_preview_wider_than_its_region_is_squashed_whole_not_cropped() {
        use crate::app::loading::composition::{aspect_fit_preview, mmpb_region_rect};

        // A stock map whose projected preview overruns the 800-wide region: the
        // fit picks a destination narrower than the source, which is exactly the
        // case the bar's left-to-right U clamp used to silently crop.
        let region = mmpb_region_rect(800);
        let fit = aspect_fit_preview(region, 400, 200).expect("valid fit");
        assert!(fit.width < 400, "fixture must exercise a downscale");

        let entry = LoadingScreenEntry {
            uv_origin: [0.5, 0.25],
            uv_size: [0.4, 0.2],
            pixel_size: [400.0, 200.0],
        };
        let mut instances = Vec::new();
        push_entry_scaled(
            &mut instances,
            entry,
            [(region.x + fit.pad_x) as f32, (region.y + fit.pad_y) as f32],
            [fit.width as f32, fit.height as f32],
            PREVIEW_DEPTH,
            [1.0; 3],
        );

        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].size, [fit.width as f32, fit.height as f32]);
        // The whole source is sampled on both axes; nothing is cut off.
        assert_eq!(instances[0].uv_origin, entry.uv_origin);
        assert_eq!(instances[0].uv_size, entry.uv_size);
    }

    #[test]
    fn the_progress_bar_is_the_only_layer_revealed_by_clipping_u() {
        let entry = LoadingScreenEntry {
            uv_origin: [0.0, 0.0],
            uv_size: [0.4, 0.05],
            pixel_size: [400.0, 10.0],
        };
        let mut instances = Vec::new();

        push_progress_fill(&mut instances, entry, [24.0, 332.0], 100.0, PROGRESS_DEPTH);

        assert_eq!(instances.len(), 1);
        // A quarter of the frame is uncovered: a quarter of U, all of V.
        assert_eq!(instances[0].size, [100.0, 10.0]);
        assert_eq!(instances[0].uv_size, [0.4_f32 * 0.25, 0.05]);
    }

    #[test]
    fn markers_inside_the_preview_region_draw_uncropped() {
        let clip = MmpbRegionRect::new(499, 379, 216, 166);
        let mut instances = Vec::new();

        push_entry_clipped(
            &mut instances,
            marker_entry(),
            [520, 400],
            MARKER_DEPTH,
            clip,
        );

        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].position, [520.0, 400.0]);
        assert_eq!(instances[0].size, [12.0, 12.0]);
        assert_eq!(instances[0].uv_origin, [0.25, 0.5]);
        assert_eq!(instances[0].uv_size, [0.1, 0.2]);
    }

    #[test]
    fn markers_overhanging_the_preview_region_are_cut_off_at_its_edge() {
        let clip = MmpbRegionRect::new(499, 379, 216, 166);
        let mut instances = Vec::new();

        // 4 px past the region's right edge and 3 px above its top edge.
        push_entry_clipped(
            &mut instances,
            marker_entry(),
            [707, 376],
            MARKER_DEPTH,
            clip,
        );

        assert_eq!(instances.len(), 1);
        let instance = instances[0];
        assert_eq!(instance.position, [707.0, 379.0]);
        assert_eq!(instance.size, [8.0, 9.0]);
        // The three cropped top rows advance the UV origin; the four cropped
        // right columns are simply never sampled.
        assert_eq!(instance.uv_origin, [0.25_f32, 0.5 + 0.2 * 3.0 / 12.0]);
        assert_eq!(instance.uv_size, [0.1_f32 * 8.0 / 12.0, 0.2 * 9.0 / 12.0]);
    }

    #[test]
    fn markers_entirely_outside_the_preview_region_are_dropped() {
        let clip = MmpbRegionRect::new(499, 379, 216, 166);
        let mut instances = Vec::new();

        push_entry_clipped(
            &mut instances,
            marker_entry(),
            [715, 400],
            MARKER_DEPTH,
            clip,
        );
        push_entry_clipped(
            &mut instances,
            marker_entry(),
            [520, 367],
            MARKER_DEPTH,
            clip,
        );

        assert!(instances.is_empty());
    }
}
