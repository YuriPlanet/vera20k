//! Loading frame drawing: layout, atlas instances, text, encoding and presentation.
//!
//! The parent owns attempt state and progress policy. This child exposes only
//! complete drawing operations; layout helpers remain private.

use super::{LoadingProgressState, NativeLoadingScreenState};
use crate::app::loading::campaign_presentation::{
    CampaignLoadingLayout, CampaignLoadingPresentation,
};
use crate::app::loading::composition::{
    LoadingCompositionSnapshot, MmpbRegionRect, loading_base_origin,
};
use crate::app::loading::progress_row::{
    LoadingProgressRowLayout, LoadingProgressRowPlacement, LoadingProgressRowSnapshot,
    PROGRESS_ROW_MEASURE_TEXT, layout_loading_progress_row,
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
/// Native prints the campaign title before the briefing's backing and copy.
const CAMPAIGN_TITLE_TEXT_DEPTH: f32 = 0.70;
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
            "native loading atlas was not available for render"
        ));
    };
    let frame_plan = build_native_loading_frame_plan(
        &renderer.bit_font,
        atlas,
        native.composition.as_ref(),
        native.campaign_presentation.as_ref(),
        &native.progress_row,
        &native.progress,
        native.backing_rgb,
        native.text_rgb,
        [renderer.gpu.config.width, renderer.gpu.config.height],
    );
    encode_native_loading_plan(
        &renderer.gpu,
        &renderer.shell_surface_presenter,
        &renderer.depth_view,
        &renderer.batch_renderer,
        &renderer.bit_font,
        atlas,
        frame_plan,
        encoder,
        destination,
    )
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
    campaign: Option<&CampaignLoadingPresentation>,
) -> Option<LoadingProgressRowLayout> {
    if progress.current_value() == 0.0 {
        return None;
    }
    let placement = campaign.map_or(
        LoadingProgressRowPlacement::StandardSkirmish(render_size),
        |campaign| LoadingProgressRowPlacement::Campaign(campaign.layout.progress_point),
    );
    Some(layout_loading_progress_row(
        placement,
        [
            atlas.progress_frame0.pixel_size[0] as i32,
            atlas.progress_frame0.pixel_size[1] as i32,
        ],
        (campaign.is_none())
            .then_some(atlas.side_icon)
            .flatten()
            .map(|icon| [icon.pixel_size[0] as i32, icon.pixel_size[1] as i32]),
        [
            font.text_width(PROGRESS_ROW_MEASURE_TEXT) as i32,
            font.cell_height() as i32,
        ],
    ))
}

#[expect(
    clippy::too_many_arguments,
    reason = "combines immutable family metadata with the shared loading text owner"
)]
fn build_native_loading_text_draws(
    font: &BitFont,
    solid_texel: LoadingScreenEntry,
    composition: Option<&LoadingCompositionSnapshot>,
    campaign: Option<&CampaignLoadingPresentation>,
    row: &LoadingProgressRowSnapshot,
    row_layout: Option<&LoadingProgressRowLayout>,
    text_rgb: [f32; 3],
    row_rgb: [f32; 3],
    render_size: [u32; 2],
) -> Vec<NativeLoadingTextDraw> {
    let mut draws = Vec::with_capacity(5);
    if let Some(campaign) = campaign {
        // Campaign DrawLoading552D60 returns before all country/loading copy
        // and player labels. PrintUnicode4A61C0→4A5EB0's 0x19 title flags have
        // no 0x100/0x200 alignment or 0x400/0x8000 backing bits. Its bottom
        // BitText_Print434B90 call has no width/height limit and clips to the
        // screen, unlike the briefing's rectangle-relative Path-A flags.
        if let Some(text) = campaign.title.as_deref() {
            let mut draw = build_native_loading_text_draw(
                font,
                solid_texel,
                text,
                crate::ui::shell::geom::RectPx::new(
                    campaign.title_origin[0],
                    campaign.title_origin[1],
                    0,
                    0,
                ),
                text_rgb,
                ShellAlign::NONE,
                None,
                CAMPAIGN_TITLE_TEXT_DEPTH,
            );
            draw.text.scissor = ScissorRect {
                x: 0,
                y: 0,
                w: render_size[0],
                h: render_size[1],
            };
            draws.push(draw);
        }
        if let Some(text) = campaign.briefing.as_deref() {
            // 553194/5531EA both measure with the original fixed 400px cap.
            // 553214 expands that measured rectangle by4 before the alpha159
            // backing; 5532F4 draws Path A with0xC (vertical center, no reveal).
            let measured = font.wrap_layout(
                text,
                CampaignLoadingPresentation::BRIEFING_WRAP_WIDTH as u32,
            );
            draws.push(build_native_loading_text_draw(
                font,
                solid_texel,
                text,
                crate::ui::shell::geom::RectPx::new(
                    campaign.briefing_origin[0],
                    campaign.briefing_origin[1],
                    measured.width as i32,
                    measured.height as i32,
                ),
                text_rgb,
                ShellAlign::V_CENTER,
                Some(CampaignLoadingPresentation::BRIEFING_BACKING_PADDING as f32),
                TEXT_DEPTH,
            ));
        }
        return draws;
    }
    if let Some(composition) = composition {
        if let Some(text) = composition.text.country_name.as_deref() {
            draws.push(build_native_loading_text_draw(
                font,
                solid_texel,
                text,
                composition.text_rects.country_name,
                text_rgb,
                ShellAlign::H_RIGHT,
                Some(TEXT_BACKING_PADDING),
                TEXT_DEPTH,
            ));
        }
        if let Some(text) = composition.text.special_unit.as_deref() {
            draws.push(build_native_loading_text_draw(
                font,
                solid_texel,
                text,
                composition.text_rects.special_unit,
                [0.0, 0.0, 0.0],
                ShellAlign::NONE,
                None,
                TEXT_DEPTH,
            ));
        }
        if let Some(text) = composition.text.load_brief.as_deref() {
            draws.push(build_native_loading_text_draw(
                font,
                solid_texel,
                text,
                composition.text_rects.load_brief,
                text_rgb,
                ShellAlign::NONE,
                Some(TEXT_BACKING_PADDING),
                TEXT_DEPTH,
            ));
        }
        if let Some(text) = composition.text.loading.as_deref() {
            draws.push(build_native_loading_text_draw(
                font,
                solid_texel,
                text,
                composition.text_rects.loading,
                text_rgb,
                ShellAlign::NONE,
                Some(TEXT_BACKING_PADDING),
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
            solid_texel,
            &row.label,
            layout.label_rect,
            row_rgb,
            ShellAlign::NONE,
            None,
            ROW_LABEL_DEPTH,
        ));
    }
    draws
}

#[allow(clippy::too_many_arguments)]
fn build_native_loading_text_draw(
    font: &BitFont,
    solid_texel: LoadingScreenEntry,
    text: &str,
    rect: crate::ui::shell::geom::RectPx,
    color: [f32; 3],
    align: ShellAlign,
    backing_padding: Option<f32>,
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
    if let Some(padding) = backing_padding
        && !text_draw.instances.is_empty()
    {
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
            solid_texel,
            [aligned_x as f32 - padding, rect.y as f32 - padding],
            [
                layout.width as f32 + padding * 2.0,
                layout.height.min(height) as f32 + padding * 2.0,
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
    campaign: Option<&CampaignLoadingPresentation>,
    progress: &LoadingProgressState,
    backing_rgb: [f32; 3],
    row_layout: Option<&LoadingProgressRowLayout>,
    base_origin: [i32; 2],
) -> Vec<SpriteInstance> {
    let mut instances = Vec::with_capacity(12);
    if let Some(campaign) = campaign {
        push_campaign_loading_chrome(
            &mut instances,
            &campaign.layout,
            atlas.background,
            atlas.title_bar,
            atlas.progress_background,
            atlas.solid_texel,
        );
    } else {
        // Standard art hangs off the same base origin as its row and copy.
        push_entry(
            &mut instances,
            atlas.background,
            [base_origin[0] as f32, base_origin[1] as f32],
            BACKGROUND_DEPTH,
        );
    }

    if campaign.is_none()
        && let Some(composition) = composition
    {
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
    let bar_origin = [
        row_layout.bar_origin[0] as f32,
        row_layout.bar_origin[1] as f32,
    ];

    // ReadScenario6847A3 passes both placement flags false for mode0;
    // DrawFill643400's +71 solid-fill branch is therefore suppressed.
    push_loading_progress_span(
        &mut instances,
        atlas.progress_frame0,
        atlas.solid_texel,
        progress,
        bar_origin,
        campaign.is_none().then_some(backing_rgb),
    );

    // Country insignia follows the progress span. The atlas has already applied
    // the verified RGB-magenta key.
    if campaign.is_none()
        && let (Some(icon), Some(icon_origin)) = (atlas.side_icon, row_layout.icon_origin)
    {
        push_entry(
            &mut instances,
            icon,
            [icon_origin[0] as f32, icon_origin[1] as f32],
            SIDE_ICON_DEPTH,
        );
    }

    instances
}

/// DrawLoading552F65..553057: unscaled frame0 at the three executed rectangle
/// origins. Missing title/bar shapes fill the original black background
/// (4E8120 initializesA83CD9..DB), with no country layers.
fn push_campaign_loading_chrome(
    out: &mut Vec<SpriteInstance>,
    layout: &CampaignLoadingLayout,
    background: LoadingScreenEntry,
    title_bar: Option<LoadingScreenEntry>,
    progress_background: Option<LoadingScreenEntry>,
    solid_texel: LoadingScreenEntry,
) {
    for (entry, rect) in [
        (title_bar, layout.title),
        (Some(background), layout.body),
        (progress_background, layout.bar),
    ] {
        if let Some(entry) = entry {
            push_entry(out, entry, [rect.x as f32, rect.y as f32], BACKGROUND_DEPTH);
        } else {
            push_entry_tinted(
                out,
                solid_texel,
                [rect.x as f32, rect.y as f32],
                [rect.w as f32, rect.h as f32],
                BACKGROUND_DEPTH,
                [0.0; 3],
            );
        }
    }
}

/// DrawFill643400 emits the optional solid player fill (G3), then the one
/// clipped shape span (G2). The existing progress owner supplies native ftol;
/// both loading families use this same submission body.
fn push_loading_progress_span(
    out: &mut Vec<SpriteInstance>,
    frame: LoadingScreenEntry,
    solid_texel: LoadingScreenEntry,
    progress: &LoadingProgressState,
    bar_origin: [f32; 2],
    backing_rgb: Option<[f32; 3]>,
) {
    if let Some(backing_rgb) = backing_rgb {
        push_entry_tinted(
            out,
            solid_texel,
            bar_origin,
            frame.pixel_size,
            SOLID_FILL_DEPTH,
            backing_rgb,
        );
    }
    let progress_width =
        progress.fill_width_gamemd_ftol_positive_domain(frame.pixel_size[0] as u32);
    if progress_width > 0 {
        push_progress_fill(
            out,
            frame,
            bar_origin,
            progress_width as f32,
            PROGRESS_DEPTH,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn build_native_loading_frame_plan(
    font: &BitFont,
    atlas: &LoadingScreenAtlas,
    composition: Option<&LoadingCompositionSnapshot>,
    campaign: Option<&CampaignLoadingPresentation>,
    progress_row: &LoadingProgressRowSnapshot,
    progress: &LoadingProgressState,
    backing_rgb: [f32; 3],
    text_rgb: [f32; 3],
    render_size: [u32; 2],
) -> NativeLoadingFramePlan {
    let row_layout = native_loading_row_layout(font, atlas, progress, render_size, campaign);
    let instances = build_native_loading_instances(
        atlas,
        composition,
        campaign,
        progress,
        backing_rgb,
        row_layout.as_ref(),
        loading_base_origin(render_size),
    );
    let text_draws = build_native_loading_text_draws(
        font,
        atlas.solid_texel,
        composition,
        campaign,
        progress_row,
        row_layout.as_ref(),
        text_rgb,
        backing_rgb,
        render_size,
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
#[expect(
    clippy::too_many_arguments,
    reason = "borrows existing loading and render owners for the synchronous repaint"
)]
pub(super) fn present_native_loading(
    gpu: &GpuContext,
    presenter: &ShellSurfacePresenter,
    depth_view: &wgpu::TextureView,
    batch: &BatchRenderer,
    font: &BitFont,
    atlas: &LoadingScreenAtlas,
    composition: Option<&LoadingCompositionSnapshot>,
    campaign: Option<&CampaignLoadingPresentation>,
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
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Native Loading Repaint"),
        });

    let frame_plan = build_native_loading_frame_plan(
        font,
        atlas,
        composition,
        campaign,
        progress_row,
        progress,
        backing_rgb,
        text_rgb,
        render_size,
    );
    encode_native_loading_plan(
        gpu,
        presenter,
        depth_view,
        batch,
        font,
        atlas,
        frame_plan,
        &mut encoder,
        &output.texture,
    )?;
    gpu.queue.submit(std::iter::once(encoder.finish()));
    output.present();
    Ok(())
}

/// One upload/draw/present encoding body serves the frame loop and native
/// synchronous repaint, for both loading families.
#[expect(
    clippy::too_many_arguments,
    reason = "borrows existing render owners for one frame"
)]
fn encode_native_loading_plan(
    gpu: &GpuContext,
    presenter: &ShellSurfacePresenter,
    depth_view: &wgpu::TextureView,
    batch: &BatchRenderer,
    font: &BitFont,
    atlas: &LoadingScreenAtlas,
    frame_plan: NativeLoadingFramePlan,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let view = presenter.source_render_view();
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
            // Backings use their own measured rectangle, not the previous
            // text draw's scissor. Only glyphs use the supplied text clip.
            pass.set_scissor_rect(0, 0, gpu.config.width, gpu.config.height);
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

    presenter.encode_present(encoder, destination);
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

/// Reveal the selected PROGBARM/SPLDBR frame0 from the left, at full height.
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
    use serde_json::Value;

    fn campaign_native() -> Value {
        let fixture: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/input_oracle/campaign_start.json",
        ))
        .expect("original campaign-start corpus");
        assert_eq!(
            fixture["native_sha256"],
            "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
        );
        fixture
    }

    /// An arbitrary atlas slot carrying a physical SHP's full canvas size.
    /// UVs deliberately vary between roles so the submission cannot silently
    /// replace a title, mission image or bar with another loading entry.
    fn retail_shape_entry(
        assets: &crate::assets::asset_manager::AssetManager,
        name: &str,
        uv_origin: [f32; 2],
    ) -> LoadingScreenEntry {
        let shape = crate::assets::shp_file::ShpFile::from_bytes(
            assets.get_ref(name).expect("retail loading SHP"),
        )
        .expect("retail loading SHP parses");
        LoadingScreenEntry {
            uv_origin,
            uv_size: [0.25, 0.125],
            pixel_size: [f32::from(shape.width), f32::from(shape.height)],
        }
    }

    #[test]
    fn campaign_chrome_uses_physical_frames_at_the_original_rectangle_origins() {
        let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        assert!(assets.register_loading_archives().unwrap());
        let mission =
            crate::rules::ini_parser::IniFile::from_bytes(assets.get_ref("MISSIONMD.INI").unwrap())
                .unwrap();
        let fixture = campaign_native();
        for filename in ["ALL01UMD.MAP", "SOV01UMD.MAP"] {
            let mut metadata = crate::rules::campaign_loading::CampaignLoadingMetadata::new();
            metadata.apply_ini(&mission, filename);
            for row in fixture["loading_geometry"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["mode"].as_u64() == Some(0))
            {
                let size = [
                    row["width"].as_u64().unwrap() as u32,
                    row["height"].as_u64().unwrap() as u32,
                ];
                let (background_name, title_name, bar_name) = if size[0] == 640 {
                    (
                        metadata.background_name_640(),
                        "TTLBR640.SHP",
                        "SPLDBRS.SHP",
                    )
                } else {
                    (
                        metadata.background_name_800(),
                        "TTLBR800.SHP",
                        "SPLDBRL.SHP",
                    )
                };
                let background = retail_shape_entry(&assets, background_name, [0.25, 0.0]);
                let title = retail_shape_entry(&assets, title_name, [0.0, 0.0]);
                let bar = retail_shape_entry(&assets, bar_name, [0.5, 0.0]);
                let solid = LoadingScreenEntry {
                    uv_origin: [0.75, 0.0],
                    uv_size: [0.01, 0.01],
                    pixel_size: [1.0, 1.0],
                };
                let mut instances = Vec::new();
                push_campaign_loading_chrome(
                    &mut instances,
                    &CampaignLoadingLayout::for_render_size(size),
                    background,
                    Some(title),
                    Some(bar),
                    solid,
                );
                assert_eq!(instances.len(), 3, "{filename} {size:?}");
                for ((draw, entry), role) in instances
                    .iter()
                    .zip([title, background, bar])
                    .zip(["title", "body", "bar"])
                {
                    let expected = row["rects"][role].as_array().unwrap();
                    assert_eq!(
                        draw.position,
                        [
                            expected[0].as_i64().unwrap() as f32,
                            expected[1].as_i64().unwrap() as f32,
                        ],
                        "{filename} {size:?} {role}"
                    );
                    assert_eq!(draw.size, entry.pixel_size, "unscaled physical frame");
                    assert_eq!(draw.uv_origin, entry.uv_origin, "{role} atlas role");
                    assert_eq!(draw.uv_size, entry.uv_size, "full frame {role}");
                }
            }
        }
    }

    #[test]
    fn campaign_progress_submission_matches_original_executed_clip_rectangles() {
        let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        assert!(assets.register_loading_archives().unwrap());
        let frame = retail_shape_entry(&assets, "SPLDBR.SHP", [0.25, 0.5]);
        let solid = LoadingScreenEntry {
            uv_origin: [0.75, 0.0],
            uv_size: [0.01, 0.01],
            pixel_size: [1.0, 1.0],
        };
        let fixture = campaign_native();
        let rows = fixture["progress_rows"]["rows"].as_array().unwrap();
        assert!(!rows.is_empty(), "executed campaign progress cases");
        for row in rows {
            let pair = |name: &str| {
                [
                    row[name][0].as_i64().unwrap() as i32,
                    row[name][1].as_i64().unwrap() as i32,
                ]
            };
            assert_eq!(
                frame.pixel_size,
                pair("bar_size").map(|value| value as f32),
                "physical SPLDBR frame dimensions"
            );
            let layout = layout_loading_progress_row(
                LoadingProgressRowPlacement::Campaign(pair("point")),
                pair("bar_size"),
                None,
                pair("font_size"),
            );
            let mut progress = LoadingProgressState::standard_skirmish();
            progress.advance_progress(row["percent"].as_u64().unwrap() as u32);
            let mut draws = Vec::new();
            push_loading_progress_span(
                &mut draws,
                frame,
                solid,
                &progress,
                layout.bar_origin.map(|value| value as f32),
                None,
            );
            let clip = row["clip_rect"].as_array().unwrap();
            let clip_width = clip[2].as_i64().unwrap() as f32;
            if clip_width == 0.0 {
                assert!(draws.is_empty(), "no visible zero-width span: {row}");
                continue;
            }
            // Original CC_DrawShape's [0,0] point is relative to this clip
            // (SHAPE_WIN_REL). GPU submission uses its absolute clip origin.
            assert_eq!(pair("draw_shape_point"), [0, 0]);
            assert_eq!(draws.len(), 1, "campaign has no solid player fill");
            let draw = &draws[0];
            assert_eq!(
                draw.position,
                [
                    clip[0].as_i64().unwrap() as f32,
                    clip[1].as_i64().unwrap() as f32,
                ],
                "native absolute clip origin: {row}"
            );
            assert_eq!(
                draw.size,
                [clip_width, clip[3].as_i64().unwrap() as f32],
                "native executed ftol and full frame height: {row}"
            );
            // The atlas representation reveals exactly that native width;
            // this CPU check does not claim native raster/palette equality.
            assert_eq!(draw.uv_origin, frame.uv_origin);
            assert_eq!(
                draw.uv_size,
                [
                    frame.uv_size[0] * (clip_width / frame.pixel_size[0]),
                    frame.uv_size[1],
                ]
            );
        }
    }

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
