//! Loading frame drawing: layout, atlas instances, text, encoding and presentation.
//!
//! The parent owns attempt state and progress policy. This child exposes only
//! complete drawing operations; layout helpers remain private.

use super::{LoadingProgressState, NativeLoadingScreenState};
use crate::app::loading::campaign_presentation::{
    CampaignLoadingLayout, CampaignLoadingPresentation,
};
use crate::app::loading::composition::{
    LoadingCompositionSnapshot, MmpbRegionRect, loading_authored_size, loading_base_origin,
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
use crate::render::shell_text::{
    ScissorRect, ShellAlign, ShellTextDraw, TextRect, draw_in_rect_scaled,
};
use crate::rules::color_scheme::scheme_entry_for_priority;
use crate::ui::shell::geom::ShellScale;

const BACKGROUND_DEPTH: f32 = 0.90;
const PREVIEW_DEPTH: f32 = 0.80;
const MARKER_DEPTH: f32 = 0.70;
/// Native prints the campaign title before the briefing's backing and copy.
const CAMPAIGN_TITLE_TEXT_DEPTH: f32 = 0.70;
const TEXT_BACKING_DEPTH: f32 = 0.60;
const TEXT_DEPTH: f32 = 0.50;
const TEXT_BACKING_ALPHA: f32 = 159.0 / 255.0;
const TEXT_BACKING_PADDING: f32 = 2.0;
/// The `+0x71` house-colour outline sits just behind the bar so the bar draws over it.
const PROGRESS_FRAME_DEPTH: f32 = 0.20;
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

///6941A0/694Cxx share ProgressScreen's row/span/text paint. Counts are unique
/// received/sent192-byte chunks, including the final short chunk. The session
/// protocol owns these counts; this returns only derived presentation.
pub(crate) fn transfer_progress_draws(
    state: &mut crate::app::AppState,
    completed: u32,
    total: u32,
) -> anyhow::Result<(Vec<SpriteInstance>, Vec<ShellTextDraw>)> {
    use crate::rules::house_colors::{HouseColorIndex, house_text_rgb};
    let local = state
        .match_state
        .session
        .network_setup()
        .map(|launch| &launch.humans[launch.local_player_index])
        .ok_or_else(|| anyhow::anyhow!("map transfer has no local participant"))?;
    let priority = if local.is_observer() {
        -2
    } else {
        i32::from(local.slot.color_index)
    };
    let color = HouseColorIndex(scheme_entry_for_priority(priority) as u8);
    let rules = state
        .rules()
        .ok_or_else(|| anyhow::anyhow!("map transfer has no color schemes"))?;
    let rgb = house_text_rgb(&rules.color_schemes, color).map(|byte| f32::from(byte) / 255.0);
    let ramp = *rules.house_color_ramps.ramp(color);
    if state.frontend.loading_screen_atlas.is_none() {
        let assets = state
            .process_assets
            .manager()
            .ok_or_else(|| anyhow::anyhow!("map transfer has no assets"))?;
        state.frontend.loading_screen_atlas =
            crate::render::loading_screen_chrome::build_transfer_progress_atlas(
                &state.renderer.gpu,
                &state.renderer.batch_renderer,
                assets,
                &ramp,
            );
    }
    let atlas = state
        .frontend
        .loading_screen_atlas
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("map transfer PROGBARM is unavailable"))?;
    let font = &state.renderer.bit_font;
    let scale = ShellScale::RETAIL;
    let size = [state.render_width(), state.render_height()];
    let layout = layout_loading_progress_row(
        LoadingProgressRowPlacement::AutoCentered(size),
        atlas.progress_frame0.pixel_size.map(|value| value as i32),
        None,
        [
            font.text_width(PROGRESS_ROW_MEASURE_TEXT) as i32,
            font.cell_height() as i32,
        ],
        scale,
        0,
    );
    let mut progress = LoadingProgressState::standard_skirmish();
    if total != 0 {
        progress
            .advance_progress((u64::from(completed.min(total)) * 100 / u64::from(total)) as u32);
    }
    let mut instances = Vec::new();
    push_loading_participant_row(
        &mut instances,
        atlas.progress_frame0,
        None,
        atlas.solid_texel,
        &progress,
        Some(rgb),
        &layout,
        scale,
    );
    let label = state
        .process_assets
        .csf
        .as_ref()
        .and_then(|csf| csf.get("GUI:MapTransfer"))
        .unwrap_or("GUI:MapTransfer");
    let draws = build_native_loading_text_draws(
        font,
        atlas.solid_texel,
        None,
        None,
        label,
        Some(&layout),
        rgb,
        rgb,
        size,
        scale,
    );
    Ok((instances, draws.into_iter().map(|draw| draw.text).collect()))
}

/// Encode the prepared native frame into the caller's existing frame submission.
pub(super) fn encode_native_loading_frame(
    renderer: &RendererState,
    native: &NativeLoadingScreenState,
    scale: ShellScale,
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
        scale,
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
    progress_frame: LoadingScreenEntry,
    insignia_measure_size: Option<[i32; 2]>,
    progress: &LoadingProgressState,
    render_size: [u32; 2],
    campaign: Option<&CampaignLoadingPresentation>,
    scale: ShellScale,
    source_index: usize,
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
            progress_frame.pixel_size[0] as i32,
            progress_frame.pixel_size[1] as i32,
        ],
        insignia_measure_size.filter(|_| campaign.is_none()),
        [
            font.text_width(PROGRESS_ROW_MEASURE_TEXT) as i32,
            font.cell_height() as i32,
        ],
        scale,
        source_index,
    ))
}

fn build_native_loading_text_draws(
    font: &BitFont,
    solid_texel: LoadingScreenEntry,
    composition: Option<&LoadingCompositionSnapshot>,
    campaign: Option<&CampaignLoadingPresentation>,
    row_label: &str,
    row_layout: Option<&LoadingProgressRowLayout>,
    text_rgb: [f32; 3],
    row_rgb: [f32; 3],
    render_size: [u32; 2],
    scale: ShellScale,
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
                scale.factor(),
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
            // The measured box is authored; the draw divides its rect back by the
            // glyph scale, so the stretched copy lands in the same box.
            let grow = |px: i32| (px as f32 * scale.factor()).round() as i32;
            draws.push(build_native_loading_text_draw(
                font,
                solid_texel,
                text,
                crate::ui::shell::geom::RectPx::new(
                    campaign.briefing_origin[0],
                    campaign.briefing_origin[1],
                    grow(measured.width as i32),
                    grow(measured.height as i32),
                ),
                text_rgb,
                ShellAlign::V_CENTER,
                Some(CampaignLoadingPresentation::BRIEFING_BACKING_PADDING as f32),
                TEXT_DEPTH,
                scale.factor(),
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
                scale.factor(),
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
                scale.factor(),
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
                scale.factor(),
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
                scale.factor(),
            ));
        }
    }
    if let Some(layout) = row_layout
        && !row_label.is_empty()
        && layout.label_rect.w > 0
        && layout.label_rect.h > 0
    {
        draws.push(build_native_loading_text_draw(
            font,
            solid_texel,
            row_label,
            layout.label_rect,
            row_rgb,
            ShellAlign::NONE,
            None,
            ROW_LABEL_DEPTH,
            scale.factor(),
        ));
    }
    draws
}

fn build_native_loading_text_draw(
    font: &BitFont,
    solid_texel: LoadingScreenEntry,
    text: &str,
    rect: crate::ui::shell::geom::RectPx,
    color: [f32; 3],
    align: ShellAlign,
    backing_padding: Option<f32>,
    depth: f32,
    glyph_scale: f32,
) -> NativeLoadingTextDraw {
    let width = rect.w.max(0) as u32;
    let height = rect.h.max(0) as u32;
    let text_rect = TextRect {
        x: rect.x,
        y: rect.y,
        w: width,
        h: height,
    };
    let text_draw = draw_in_rect_scaled(
        font,
        text,
        text_rect,
        color,
        align,
        [0.0, 0.0],
        depth,
        glyph_scale,
    );
    let mut backing = Vec::new();
    if let Some(padding) = backing_padding
        && !text_draw.instances.is_empty()
    {
        // The draw lays the copy out in the box's authored units, so the backing
        // measures there too and grows back with the glyphs it has to sit behind.
        let authored = (width as f32 / glyph_scale).round().max(0.0) as u32;
        let authored_height = (height as f32 / glyph_scale).round().max(0.0) as u32;
        let layout = font.wrap_layout(text, authored);
        let aligned_x = if align.contains(ShellAlign::H_RIGHT) && layout.width < authored {
            rect.x + ((authored - layout.width) as f32 * glyph_scale) as i32
        } else if align.contains(ShellAlign::H_CENTER) && layout.width < authored {
            rect.x + (((authored - layout.width) as f32 * glyph_scale) / 2.0) as i32
        } else {
            rect.x
        };
        push_entry_tinted(
            &mut backing,
            solid_texel,
            [aligned_x as f32 - padding, rect.y as f32 - padding],
            [
                (layout.width as f32 + padding * 2.0) * glyph_scale,
                ((layout.height.min(authored_height) as f32 + padding * 2.0) * glyph_scale),
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
    base_origin: [i32; 2],
    scale: ShellScale,
) -> Vec<SpriteInstance> {
    let mut instances = Vec::with_capacity(12);
    if let Some(campaign) = campaign {
        push_campaign_loading_chrome(
            &mut instances,
            &campaign.layout,
            atlas
                .background
                .expect("campaign atlas has its required background"),
            atlas.title_bar,
            atlas.progress_background,
            atlas.solid_texel,
            scale,
        );
    } else if let Some(background) = atlas.background {
        // Standard art hangs off the same base origin as its row and copy,
        // mapped through the scale as they are.
        let [x, y] = scale.scale_point(base_origin);
        push_entry_scaled(
            &mut instances,
            background,
            [x as f32, y as f32],
            scale.art_size(background.pixel_size[0], background.pixel_size[1]),
            BACKGROUND_DEPTH,
            [1.0; 3],
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
                    scale,
                );
            }
        }
    }

    instances
}

/// One progress-row painter, shared by campaign, local and remote humans.
fn push_loading_participant_row(
    instances: &mut Vec<SpriteInstance>,
    progress_frame: LoadingScreenEntry,
    side_icon: Option<LoadingScreenEntry>,
    solid_texel: LoadingScreenEntry,
    progress: &LoadingProgressState,
    backing_rgb: Option<[f32; 3]>,
    layout: &LoadingProgressRowLayout,
    scale: ShellScale,
) {
    let bar_origin = [layout.bar_origin[0] as f32, layout.bar_origin[1] as f32];
    push_loading_progress_span(
        instances,
        progress_frame,
        solid_texel,
        progress,
        bar_origin,
        backing_rgb,
        scale,
    );
    if let (Some(icon), Some(origin)) = (side_icon, layout.icon_origin) {
        push_entry_scaled(
            instances,
            icon,
            [origin[0] as f32, origin[1] as f32],
            scale.art_size(icon.pixel_size[0], icon.pixel_size[1]),
            SIDE_ICON_DEPTH,
            [1.0; 3],
        );
    }
}

/// DrawLoading552F65..553057: unscaled frame0 at the three executed rectangle
/// origins. Missing title/bar shapes fill the original black background
/// (4E8120 initializesA83CD9..DB), with no country layers.
#[allow(clippy::too_many_arguments)]
fn push_campaign_loading_chrome(
    out: &mut Vec<SpriteInstance>,
    layout: &CampaignLoadingLayout,
    background: LoadingScreenEntry,
    title_bar: Option<LoadingScreenEntry>,
    progress_background: Option<LoadingScreenEntry>,
    solid_texel: LoadingScreenEntry,
    scale: ShellScale,
) {
    for (entry, rect) in [
        (title_bar, layout.title),
        (Some(background), layout.body),
        (progress_background, layout.bar),
    ] {
        if let Some(entry) = entry {
            push_entry_scaled(
                out,
                entry,
                [rect.x as f32, rect.y as f32],
                scale.art_size(entry.pixel_size[0], entry.pixel_size[1]),
                BACKGROUND_DEPTH,
                [1.0; 3],
            );
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

/// `ProgressMeterClass__DrawFill @ 0x00643400` emits the optional house-colour
/// outline (`+0x71`), then the one clipped shape span. The existing progress owner
/// supplies native ftol; both loading families use this same submission body.
fn push_loading_progress_span(
    out: &mut Vec<SpriteInstance>,
    frame: LoadingScreenEntry,
    solid_texel: LoadingScreenEntry,
    progress: &LoadingProgressState,
    bar_origin: [f32; 2],
    backing_rgb: Option<[f32; 3]>,
    scale: ShellScale,
) {
    let bar_size = scale.art_size(frame.pixel_size[0], frame.pixel_size[1]);
    // The `+0x71` branch is a one-pixel **outline**, not a filled panel:
    // `0x0064349B..0x00643529` calls surface vtable slot `+0x58` = `XSurface__DrawRect`
    // (`0x007BAD90`), which forwards to `XSurface__DrawRectEx` (`0x007BADC0`) and draws
    // the rect with four `DrawLineEx` strokes over `x..x+w-1`, `y..y+h-1` (last row and
    // column included). The rect itself is the frame outset by 3px:
    // `0x0064341E..0x0064345E` builds `(helper+3, helper+3, W, H)` and `0x0072A9E0(rect, -3)`
    // applies `x += n; y += n; w += n * -2; h += n * -2`, leaving
    // `(helper, helper, W + 6, H + 6)`. So the house colour rings the meter and the
    // un-filled interior keeps showing the loading background through it.
    let ring = (3.0 * scale.factor()).round();
    let frame_origin = [bar_origin[0] - ring, bar_origin[1] - ring];
    let frame_size = [bar_size[0] + 2.0 * ring, bar_size[1] + 2.0 * ring];
    if let Some(frame_rgb) = backing_rgb {
        let stroke = scale.factor().round().max(1.0);
        let [frame_w, frame_h] = frame_size;
        let [frame_x, frame_y] = frame_origin;
        for (origin, size) in [
            ([frame_x, frame_y], [frame_w, stroke]),
            ([frame_x, frame_y + frame_h - stroke], [frame_w, stroke]),
            ([frame_x, frame_y], [stroke, frame_h]),
            ([frame_x + frame_w - stroke, frame_y], [stroke, frame_h]),
        ] {
            push_entry_tinted(
                out,
                solid_texel,
                origin,
                size,
                PROGRESS_FRAME_DEPTH,
                frame_rgb,
            );
        }
    }
    let full_width = frame.pixel_size[0].max(1.0);
    let progress_width = progress.fill_width_gamemd_ftol_positive_domain(full_width as u32);
    if progress_width > 0 {
        // Preserve the native integer clip width before scaling. Dividing by
        // the full frame and multiplying back loses that width even at1x.
        push_progress_fill(
            out,
            frame,
            bar_origin,
            progress_width,
            scale,
            PROGRESS_DEPTH,
        );
    }
}

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
    scale: ShellScale,
) -> NativeLoadingFramePlan {
    let authored = loading_authored_size(render_size, scale);
    let mut instances = build_native_loading_instances(
        atlas,
        composition,
        campaign,
        loading_base_origin(authored),
        scale,
    );
    let mut text_draws = build_native_loading_text_draws(
        font,
        atlas.solid_texel,
        composition,
        campaign,
        "",
        None,
        text_rgb,
        backing_rgb,
        render_size,
        scale,
    );
    if campaign.is_some() {
        if let Some(layout) = native_loading_row_layout(
            font,
            atlas.progress_frame0,
            None,
            progress,
            render_size,
            campaign,
            scale,
            0,
        ) {
            push_loading_participant_row(
                &mut instances,
                atlas.progress_frame0,
                None,
                atlas.solid_texel,
                progress,
                None,
                &layout,
                scale,
            );
        }
    } else {
        // Redraw643C0B..36 draws every source-node row, including a peer at0%.
        // House/color sorting affects neither this order nor the local cursor.
        for (index, row) in progress_row.participants.iter().enumerate() {
            let art = atlas.participants.get(index).copied().unwrap_or(
                crate::render::loading_screen_chrome::LoadingParticipantArt {
                    progress_frame: atlas.progress_frame0,
                    side_icon: atlas.side_icon,
                },
            );
            let Some(layout) = native_loading_row_layout(
                font,
                art.progress_frame,
                atlas.insignia_measure_size,
                progress,
                render_size,
                None,
                scale,
                index,
            ) else {
                continue;
            };
            let row_progress = row.remote_progress.as_ref().unwrap_or(progress);
            push_loading_participant_row(
                &mut instances,
                art.progress_frame,
                art.side_icon,
                atlas.solid_texel,
                row_progress,
                Some(row.backing_rgb),
                &layout,
                scale,
            );
            text_draws.extend(build_native_loading_text_draws(
                font,
                atlas.solid_texel,
                None,
                None,
                &row.label,
                Some(&layout),
                text_rgb,
                row.backing_rgb,
                render_size,
                scale,
            ));
        }
    }
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
    campaign: Option<&CampaignLoadingPresentation>,
    progress_row: &LoadingProgressRowSnapshot,
    progress: &LoadingProgressState,
    backing_rgb: [f32; 3],
    text_rgb: [f32; 3],
    render_size: [u32; 2],
    scale: ShellScale,
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
        scale,
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
/// an edge is cut off there instead of spilling onto the loading art. The marker
/// art grows with the page's scale, and the crop is taken in those window pixels,
/// so the same part of the diamond survives on either mapping.
fn push_entry_clipped(
    out: &mut Vec<SpriteInstance>,
    entry: LoadingScreenEntry,
    position: [i32; 2],
    depth: f32,
    clip: MmpbRegionRect,
    scale: ShellScale,
) {
    let [art_w, art_h] = scale.art_size(entry.pixel_size[0], entry.pixel_size[1]);
    let (width, height) = (art_w.round() as i32, art_h.round() as i32);
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
            entry.uv_origin[0] + entry.uv_size[0] * cropped[0] / art_w.max(1.0),
            entry.uv_origin[1] + entry.uv_size[1] * cropped[1] / art_h.max(1.0),
        ],
        uv_size: [
            entry.uv_size[0] * visible[0] / art_w.max(1.0),
            entry.uv_size[1] * visible[1] / art_h.max(1.0),
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
/// the fill fraction while the V axis stays whole. `source_width` is the native
/// integer clip width. The page's scale grows that width and the full height.
fn push_progress_fill(
    out: &mut Vec<SpriteInstance>,
    entry: LoadingScreenEntry,
    position: [f32; 2],
    source_width: u32,
    scale: ShellScale,
    depth: f32,
) {
    let full_width = entry.pixel_size[0].max(1.0);
    let width = (source_width as f32).min(full_width);
    let fraction = width / full_width;
    out.push(SpriteInstance {
        position,
        size: scale.art_size(width, entry.pixel_size[1]),
        uv_origin: entry.uv_origin,
        uv_size: [entry.uv_size[0] * fraction, entry.uv_size[1]],
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

    fn campaign_native() -> &'static Value {
        let fixture = crate::test_fixture::json("tools/input_oracle/campaign_start.json");
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
                    &CampaignLoadingLayout::for_authored_size(size),
                    background,
                    Some(title),
                    Some(bar),
                    solid,
                    ShellScale::RETAIL,
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
                ShellScale::RETAIL,
                0,
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
                ShellScale::RETAIL,
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

        push_progress_fill(
            &mut instances,
            entry,
            [24.0, 332.0],
            100,
            ShellScale::RETAIL,
            PROGRESS_DEPTH,
        );

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
            ShellScale::RETAIL,
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
            ShellScale::RETAIL,
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
            ShellScale::RETAIL,
        );
        push_entry_clipped(
            &mut instances,
            marker_entry(),
            [520, 367],
            MARKER_DEPTH,
            clip,
            ShellScale::RETAIL,
        );

        assert!(instances.is_empty());
    }
}
