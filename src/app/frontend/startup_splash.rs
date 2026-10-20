//! Retail process-start splash composition and presentation.
//!
//! This is intentionally separate from `app::loading`: the native GLS splash is
//! process initialization chrome, not selected-scenario loading presentation.
//! Any window other than the two sizes gamemd's shell runs at shows it fitted
//! like the menus, so it lines up with the menu that follows it.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::assets::asset_manager::AssetManager;
use crate::assets::csf_file::CsfFile;
use crate::assets::fnt_file::FntFile;
use crate::assets::pal_file::Palette;
use crate::assets::shp_file::ShpFile;
use crate::render::batch::{BatchRenderer, BatchTexture, SpriteInstance};
use crate::render::gpu::GpuContext;
use crate::render::shell_surface_present::ShellSurfacePresenter;
use crate::render::shell_ui_style_assets::{
    ShellUiAssetLocator, ShellUiAssetPair, ShellUiAssetResolver,
};
use crate::ui::shell::geom::{RectPx, SHELL_BASE_H, SHELL_BASE_W, ShellScale};

use super::shell_layout::window_takes_fitted_shell;

/// One game's startup splash art: its palette, the shape drawn at exactly 640
/// pixels wide, and the shape drawn at every other width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SplashArt {
    palette: &'static str,
    small: &'static str,
    large: &'static str,
}

impl SplashArt {
    fn shape_for_width(self, client_width: u32) -> &'static str {
        if client_width == 640 {
            self.small
        } else {
            self.large
        }
    }
}

// Native YR 1.11 gamemd.exe, GZF 2026-10-15, VA 0x005312a0
// (source record D:\ra2mdpost\Init.CPP; called from Init_Game at 0x0052ba60):
// load GLSMD.PAL, then GLSSMD.SHP at exactly 640 pixels wide or GLSLMD.SHP at
// every other width. If the optional SHP is unavailable, native still draws
// its text, presents the surface, and arms the 5-second deadline. The palette
// gets the shared `raw << 2` conversion (`Palette::from_bytes`) and a
// ConvertClass, and CC_Draw_Shape SHAPE_NORMAL skips only index 0
// (`ShpFile::frame_to_rgba`).
const YR_SPLASH: SplashArt = SplashArt {
    palette: "GLSMD.PAL",
    small: "GLSSMD.SHP",
    large: "GLSLMD.SHP",
};

/// Default Red Alert 2 splash art: the unsuffixed originals in
/// `ra2.mix -> local.mix`, the same single-frame 640x480 and 800x600 shapes
/// with a matching palette, chosen by the same width rule.
const RA2_SPLASH: SplashArt = SplashArt {
    palette: "GLS.PAL",
    small: "GLSS.SHP",
    large: "GLSL.SHP",
};

const fn splash_art(yr_original_style: bool) -> SplashArt {
    if yr_original_style {
        YR_SPLASH
    } else {
        RA2_SPLASH
    }
}
const MINIMUM_VISIBLE_TIME: Duration = Duration::from_millis(5000);
const TEXT_COLOR: [u8; 4] = [255, 255, 255, 255];

const COPYRIGHT_KEY: &str = "TXT_COPYRIGHT";
const COPYRIGHT_FALLBACK: &str = "© 2000, 2001 ELECTRONIC ARTS INC. ALL RIGHTS RESERVED";
const BRAND_KEY: &str = "GUI:WWBrand";
const BRAND_FALLBACK: &str = "WESTWOOD STUDIOS\u{99} IS AN ELECTRONIC ARTS\u{99} BRAND";
const LOADING_KEY: &str = "GUI:LoadingEx";
const LOADING_FALLBACK: &str = "Loading...";
const TRADEMARK_TOP_KEY: &str = "GUI:TradeMarkTop";
const TRADEMARK_TOP_FALLBACK: &str =
    "Command & Conquer and Yuri's Revenge are trademarks or registered";
const TRADEMARK_BOTTOM_KEY: &str = "GUI:TradeMarkBottom";
const TRADEMARK_BOTTOM_FALLBACK: &str =
    "trademarks of Electronic Arts Inc. in the U.S. and/or other countries.";

// ra2web-werhd/public/res/locale/zh-CN.json, displayed by SplashScreen.ts.
#[cfg(any(target_os = "ios", target_os = "macos", test))]
const WERHD_COPYRIGHT: [&str; 2] = [
    "© 2026 王二火大制作组保留游戏引擎和联机服务的权利",
    "版权所有，翻版必究",
];
#[cfg(any(target_os = "ios", target_os = "macos", test))]
const WERHD_DISCLAIMER: [&str; 3] = [
    "免责声明：",
    "\"王二火大\" 是一个非盈利的粉丝项目，与 Electronic Arts Inc.(EA) 没有任何关联。",
    "没有侵犯版权的意图。所有权利均归其各自所有者所有。",
];

/// The minimum on-screen hold, anchored at the first successful present.
///
/// Native anchors its timestamp *after* the blit and lets the rules/type
/// initialization that follows run inside that window, so the hold is
/// `max(work-after-present, MINIMUM_VISIBLE_TIME)` and never an added sleep.
/// Arming exactly once is what makes that true: the per-frame re-present would
/// otherwise push the deadline forward on every frame and hold the splash up
/// forever.
#[derive(Debug, Default)]
struct VisibleHold {
    deadline: Option<Instant>,
}

impl VisibleHold {
    fn mark_presented(&mut self, now: Instant) {
        if self.deadline.is_none() {
            self.deadline = Some(now + MINIMUM_VISIBLE_TIME);
        }
    }

    /// An unarmed hold is still active, so a first present that failed on a
    /// transient surface acquisition is retried instead of skipped.
    fn is_active(&self, now: Instant) -> bool {
        self.deadline.is_none_or(|deadline| now < deadline)
    }
}

pub(crate) struct StartupSplashPresentation {
    texture: BatchTexture,
    instance_buffer: wgpu::Buffer,
    hold: VisibleHold,
}

impl StartupSplashPresentation {
    pub(crate) fn build(
        gpu: &GpuContext,
        batch: &BatchRenderer,
        assets: &AssetManager,
        csf: Option<&CsfFile>,
        font: &FntFile,
        client_width: u32,
        client_height: u32,
        yr_original_style: bool,
    ) -> Result<Self> {
        anyhow::ensure!(
            client_width > 0 && client_height > 0,
            "startup splash requires a non-zero client size"
        );

        let ([width, height], placement) = splash_placement(client_width, client_height);
        #[cfg(not(any(target_os = "ios", target_os = "macos")))]
        let rgba = compose_startup_splash(
            assets,
            csf,
            font,
            width as usize,
            height as usize,
            yr_original_style,
        )?;
        #[cfg(any(target_os = "ios", target_os = "macos"))]
        let rgba = {
            // The caller only constructs this splash for non-capture startup.
            // Country-specific scenario loading remains in app::loading.
            let _ = (assets, ra2_look);
            compose_branded_startup_splash(csf, font, width, height)?
        };
        let texture = batch.create_texture(gpu, &rgba, width, height);
        let instance = SpriteInstance {
            position: [placement.x as f32, placement.y as f32],
            size: [placement.w as f32, placement.h as f32],
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            depth: 0.0,
            tint: [1.0, 1.0, 1.0],
            alpha: 1.0,
            ..Default::default()
        };
        let (instance_buffer, count) = batch
            .create_instance_buffer(gpu, &[instance])
            .context("create startup splash instance buffer")?;
        debug_assert_eq!(count, 1);

        Ok(Self {
            texture,
            instance_buffer,
            hold: VisibleHold::default(),
        })
    }

    pub(crate) fn mark_presented(&mut self, now: Instant) {
        self.hold.mark_presented(now);
    }

    pub(crate) fn is_active(&self, now: Instant) -> bool {
        self.hold.is_active(now)
    }
}

pub(crate) fn render_and_present(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    presenter: &ShellSurfacePresenter,
    depth: &wgpu::TextureView,
    fps_overlay: &crate::app::diagnostics::fps_overlay::FpsOverlay,
    splash: &StartupSplashPresentation,
) -> Result<()> {
    batch.update_camera(
        gpu,
        gpu.config.width as f32,
        gpu.config.height as f32,
        0.0,
        0.0,
        1.0,
        crate::render::batch::DepthAxis::NONE,
    );

    let output = gpu
        .surface
        .get_current_texture()
        .context("acquire startup splash surface")?;
    let source_view = presenter.source_render_view();
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Retail Startup Splash"),
        });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Retail Startup Splash Composition"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &source_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                // The shared passthrough batch pipeline declares Depth32Float
                // even though it compares Always and does not write depth.
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        batch.draw_with_buffer_ui_passthrough(
            &mut pass,
            &splash.texture,
            &splash.instance_buffer,
            1,
        );
    }
    presenter.encode_present(&mut encoder, &output.texture);
    fps_overlay.encode(gpu, batch, &mut encoder, &output.texture);
    gpu.queue.submit(std::iter::once(encoder.finish()));
    output.present();
    Ok(())
}

/// The size the splash is composed at and where it sits in the window. The two
/// windows gamemd's shell runs at keep its 1:1 composition at the window size.
/// Any other window gets the 800x600 composition scaled the way the menus scale
/// their page: the largest uniform fit, centred, with the spare width or height
/// left to the black clear. gamemd itself draws 1:1, centred, at every size.
fn splash_placement(client_width: u32, client_height: u32) -> ([u32; 2], RectPx) {
    if window_takes_fitted_shell((client_width, client_height)) {
        let scale = ShellScale::from_screen(client_width as i32, client_height as i32);
        (
            [SHELL_BASE_W as u32, SHELL_BASE_H as u32],
            scale.dialog_rect(),
        )
    } else {
        let window = RectPx::new(0, 0, client_width as i32, client_height as i32);
        ([client_width, client_height], window)
    }
}

/// gamemd's splash for a `client_width` x `client_height` client: the art
/// centred 1:1 and the text anchored to the client's corners.
fn compose_startup_splash(
    assets: &AssetManager,
    csf: Option<&CsfFile>,
    font: &FntFile,
    client_width: usize,
    client_height: usize,
    yr_original_style: bool,
) -> Result<Vec<u8>> {
    let art = splash_art(yr_original_style);
    let resolver = ShellUiAssetResolver::new(assets, yr_original_style);
    let mut canvas = vec![0u8; client_width * client_height * 4];
    for pixel in canvas.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }

    let shape_name = art.shape_for_width(client_width as u32);
    let palette_pair = ShellUiAssetPair::new(
        ShellUiAssetLocator::archive(art.palette, "ra2.mix -> local.mix"),
        ShellUiAssetLocator::active(art.palette),
    );
    let shape_pair = ShellUiAssetPair::new(
        ShellUiAssetLocator::archive(shape_name, "ra2.mix -> local.mix"),
        ShellUiAssetLocator::active(shape_name),
    );
    let palette_result = resolver.resolve(palette_pair);
    let shape_result = resolver.resolve(shape_pair);
    if let (Ok(palette_asset), Ok(shape_asset)) = (&palette_result, &shape_result) {
        match (
            Palette::from_bytes(palette_asset.bytes),
            ShpFile::from_bytes(shape_asset.bytes),
        ) {
            (Ok(palette), Ok(shp)) => {
                if let Some(frame) = shp.frames.first() {
                    let frame_rgba = shp
                        .frame_to_rgba(0, &palette)
                        .context("decode startup splash SHP frame 0")?;
                    let origin_x = centered_offset(client_width as i32, shp.width as i32)
                        + frame.frame_x as i32;
                    let origin_y = centered_offset(client_height as i32, shp.height as i32)
                        + frame.frame_y as i32;
                    blit_rgba_clipped(
                        &mut canvas,
                        client_width,
                        client_height,
                        &frame_rgba,
                        frame.frame_width as usize,
                        frame.frame_height as usize,
                        origin_x,
                        origin_y,
                        RgbaBlend::Copy,
                    );
                }
            }
            (Err(err), _) => log::warn!("Could not parse {}: {err}", art.palette),
            (_, Err(err)) => log::warn!(
                "Could not parse {}: {err}",
                art.shape_for_width(client_width as u32)
            ),
        }
    } else {
        if let Err(err) = palette_result {
            log::warn!(
                "Retail startup splash palette {} unavailable: {err:#}",
                art.palette
            );
        }
        if let Err(err) = shape_result {
            log::warn!("Retail startup splash shape {shape_name} unavailable: {err:#}");
        }
        log::warn!("Drawing startup splash text over black without its art");
    }

    paint_startup_text(
        &mut canvas,
        client_width,
        client_height,
        csf,
        font,
        [10, 10],
    );
    Ok(canvas)
}

/// Artwork and legal/loading labels remain separate presentation layers.
fn paint_startup_text(
    canvas: &mut [u8],
    client_width: usize,
    client_height: usize,
    csf: Option<&CsfFile>,
    font: &FntFile,
    loading_origin: [i32; 2],
) {
    let copyright = csf_text(csf, COPYRIGHT_KEY, COPYRIGHT_FALLBACK);
    let brand = csf_text(csf, BRAND_KEY, BRAND_FALLBACK);
    let loading = csf_text(csf, LOADING_KEY, LOADING_FALLBACK);
    let trademark_top = csf_text(csf, TRADEMARK_TOP_KEY, TRADEMARK_TOP_FALLBACK);
    let trademark_bottom = csf_text(csf, TRADEMARK_BOTTOM_KEY, TRADEMARK_BOTTOM_FALLBACK);

    let first_bottom_y = client_height as i32 - 40;
    let second_bottom_y = first_bottom_y + 3 + font.cell_height as i32;

    draw_text(
        canvas,
        client_width,
        client_height,
        font,
        &copyright,
        client_width as i32 - font.text_width(&copyright) as i32 - 10,
        first_bottom_y,
    );
    draw_text(
        canvas,
        client_width,
        client_height,
        font,
        &brand,
        client_width as i32 - font.text_width(&brand) as i32 - 10,
        second_bottom_y,
    );
    draw_text(
        canvas,
        client_width,
        client_height,
        font,
        &loading,
        loading_origin[0],
        loading_origin[1],
    );
    draw_text(
        canvas,
        client_width,
        client_height,
        font,
        &trademark_top,
        10,
        first_bottom_y,
    );
    draw_text(
        canvas,
        client_width,
        client_height,
        font,
        &trademark_bottom,
        10,
        second_bottom_y,
    );
}

#[cfg(any(target_os = "ios", target_os = "macos", test))]
fn compose_branded_startup_splash(
    csf: Option<&CsfFile>,
    font: &FntFile,
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    const ROWS: u32 = 13;
    const LINE_HEIGHT: u32 = 15;
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    let mut rasterizer = crate::render::bit_font::UiGlyphRasterizer::new(font.bitmap_rows)
        .context("create WERHD startup text rasterizer")?;
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    let mut rasterize = |codepoint, width| rasterizer.glyph(codepoint, width);
    #[cfg(not(any(target_os = "ios", target_os = "macos")))]
    let mut rasterize = |_, _| None;
    let column_width = (width.saturating_sub(40)) / 2;
    let disclaimer = wrap_footer_lines(font, &WERHD_DISCLAIMER, column_width, ROWS);
    let copyright = wrap_footer_lines(font, &WERHD_COPYRIGHT, column_width, ROWS);
    let footer_height =
        (disclaimer.len().max(copyright.len()) as u32 * LINE_HEIGHT + 20).min(height / 3);
    let art_height = height - footer_height;
    let art_width = (art_height * 4 / 3).min(width);
    let art_height = art_width * 3 / 4;
    let art_x = (width - art_width) / 2;
    let art_y = (height - footer_height - art_height) / 2;
    let image = image::load_from_memory_with_format(
        include_bytes!("../../../art/branding/werhd-loading.png"),
        image::ImageFormat::Png,
    )
    .context("decode built-in WERHD startup loading image")?
    // Remove the generated cyan edge before resampling; retain the 4:3 art.
    .crop_imm(4, 3, 1440, 1080)
    .resize_exact(art_width, art_height, image::imageops::FilterType::Lanczos3)
    .into_rgba8();
    let mut canvas = vec![0; width as usize * height as usize * 4];
    for pixel in canvas.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    blit_rgba_clipped(
        &mut canvas,
        width as usize,
        height as usize,
        image.as_raw(),
        art_width as usize,
        art_height as usize,
        art_x as i32,
        art_y as i32,
        RgbaBlend::SourceOver,
    );
    let loading = csf_text(csf, LOADING_KEY, LOADING_FALLBACK);
    draw_text_with_rasterizer(
        &mut canvas,
        width as usize,
        height as usize,
        font,
        &loading,
        art_x as i32 + 10,
        (art_y + art_height) as i32 - font.cell_height as i32 - 6,
        &mut rasterize,
    );
    // Like the web splash: independent left/right blocks, bottom-aligned,
    // with 10px outer margins. Reserve their space rather than covering art.
    for (lines, right_aligned) in [(&disclaimer, false), (&copyright, true)] {
        let first_y = height as i32 - 10 - lines.len() as i32 * LINE_HEIGHT as i32;
        for (index, line) in lines.iter().enumerate() {
            let line_width = scaled_footer_width(font, line, ROWS);
            let x = if right_aligned {
                width as i32 - 10 - line_width as i32
            } else {
                10
            };
            let source_width = font.text_width(line).max(1);
            let source_rows = u32::from(font.bitmap_rows).max(1);
            // Keep straight-alpha mask RGB white even where coverage is zero;
            // Lanczos must interpolate coverage without darkening it twice.
            let mut bitmap = vec![255; source_width as usize * source_rows as usize * 4];
            for pixel in bitmap.chunks_exact_mut(4) {
                pixel[3] = 0;
            }
            draw_text_with_rasterizer(
                &mut bitmap,
                source_width as usize,
                source_rows as usize,
                font,
                line,
                0,
                0,
                &mut rasterize,
            );
            let image = image::RgbaImage::from_raw(source_width, source_rows, bitmap)
                .expect("footer text buffer dimensions");
            let image = image::imageops::resize(
                &image,
                line_width.max(1),
                ROWS,
                image::imageops::FilterType::Lanczos3,
            );
            blit_rgba_clipped(
                &mut canvas,
                width as usize,
                height as usize,
                image.as_raw(),
                image.width() as usize,
                image.height() as usize,
                x,
                first_y + index as i32 * LINE_HEIGHT as i32,
                RgbaBlend::SourceOver,
            );
        }
    }
    Ok(canvas)
}

#[cfg(any(target_os = "ios", target_os = "macos", test))]
fn scaled_footer_width(font: &FntFile, text: &str, rows: u32) -> u32 {
    (font.text_width(text) * rows).div_ceil(u32::from(font.bitmap_rows).max(1))
}

#[cfg(any(target_os = "ios", target_os = "macos", test))]
fn wrap_footer_lines(font: &FntFile, paragraphs: &[&str], width: u32, rows: u32) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in paragraphs {
        let mut line = String::new();
        for ch in paragraph.chars() {
            let previous_len = line.len();
            line.push(ch);
            if previous_len > 0 && scaled_footer_width(font, &line, rows) > width {
                let split = line[..previous_len]
                    .rfind(' ')
                    .filter(|index| *index > 0)
                    .unwrap_or(previous_len);
                lines.push(line[..split].trim_end().to_owned());
                line = line[split..].trim_start().to_owned();
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines
}

fn centered_offset(client_extent: i32, art_extent: i32) -> i32 {
    (client_extent - art_extent) / 2
}

fn csf_text<'a>(csf: Option<&'a CsfFile>, key: &str, fallback: &'a str) -> String {
    match csf {
        Some(table) => table.text(key).into_owned(),
        None => fallback.to_string(),
    }
}

#[derive(Clone, Copy)]
enum RgbaBlend {
    Copy,
    SourceOver,
}

// Both buffers use straight alpha. The startup canvas is opaque, while a
// footer mask starts transparent and is composited onto it after resampling.
fn source_over_rgba(destination: &mut [u8], source: &[u8]) {
    let source_alpha = u32::from(source[3]);
    if source_alpha == 0 {
        return;
    }
    if source_alpha == 255 {
        destination.copy_from_slice(source);
        return;
    }
    let retained_alpha = u32::from(destination[3]) * (255 - source_alpha);
    let alpha_numerator = source_alpha * 255 + retained_alpha;
    for channel in 0..3 {
        let color_numerator = u32::from(source[channel]) * source_alpha * 255
            + u32::from(destination[channel]) * retained_alpha;
        destination[channel] = ((color_numerator + alpha_numerator / 2) / alpha_numerator) as u8;
    }
    destination[3] = ((alpha_numerator + 127) / 255) as u8;
}

fn blit_rgba_clipped(
    destination: &mut [u8],
    destination_width: usize,
    destination_height: usize,
    source: &[u8],
    source_width: usize,
    source_height: usize,
    destination_x: i32,
    destination_y: i32,
    blend: RgbaBlend,
) {
    for source_y in 0..source_height {
        let target_y = destination_y + source_y as i32;
        if !(0..destination_height as i32).contains(&target_y) {
            continue;
        }
        for source_x in 0..source_width {
            let target_x = destination_x + source_x as i32;
            if !(0..destination_width as i32).contains(&target_x) {
                continue;
            }
            let source_offset = (source_y * source_width + source_x) * 4;
            let alpha = source.get(source_offset + 3).copied().unwrap_or(0);
            if alpha == 0 {
                continue;
            }
            let destination_offset =
                (target_y as usize * destination_width + target_x as usize) * 4;
            let destination = &mut destination[destination_offset..destination_offset + 4];
            let source = &source[source_offset..source_offset + 4];
            match blend {
                RgbaBlend::Copy => destination.copy_from_slice(source),
                RgbaBlend::SourceOver => source_over_rgba(destination, source),
            }
        }
    }
}

fn draw_text(
    canvas: &mut [u8],
    canvas_width: usize,
    canvas_height: usize,
    font: &FntFile,
    text: &str,
    x: i32,
    y: i32,
) {
    draw_text_with_rasterizer(
        canvas,
        canvas_width,
        canvas_height,
        font,
        text,
        x,
        y,
        &mut |_, _| None,
    );
}

fn draw_text_with_rasterizer(
    canvas: &mut [u8],
    canvas_width: usize,
    canvas_height: usize,
    font: &FntFile,
    text: &str,
    x: i32,
    y: i32,
    rasterize: &mut impl FnMut(u16, u32) -> Option<Vec<u8>>,
) {
    let mut cursor_x = x;
    for ch in text.chars() {
        let codepoint = ch as u32;
        if codepoint > u16::MAX as u32 {
            continue;
        }
        let Some(glyph) = font.glyph(codepoint as u16) else {
            continue;
        };
        let modern = rasterize(codepoint as u16, glyph.width);
        let pixels = modern.as_deref().unwrap_or(&glyph.rgba);
        for glyph_y in 0..font.bitmap_rows as usize {
            let target_y = y + glyph_y as i32;
            if !(0..canvas_height as i32).contains(&target_y) {
                continue;
            }
            for glyph_x in 0..glyph.width as usize {
                let target_x = cursor_x + glyph_x as i32;
                if !(0..canvas_width as i32).contains(&target_x) {
                    continue;
                }
                let glyph_offset = (glyph_y * glyph.width as usize + glyph_x) * 4;
                let alpha = pixels.get(glyph_offset + 3).copied().unwrap_or(0);
                if alpha == 0 {
                    continue;
                }
                let canvas_offset = (target_y as usize * canvas_width + target_x as usize) * 4;
                let destination = &mut canvas[canvas_offset..canvas_offset + 4];
                if modern.is_some() {
                    source_over_rgba(
                        destination,
                        &[TEXT_COLOR[0], TEXT_COLOR[1], TEXT_COLOR[2], alpha],
                    );
                } else {
                    // Preserve the retail FNT's original binary-mask drawing.
                    destination.copy_from_slice(&TEXT_COLOR);
                }
            }
        }
        cursor_x += glyph.width as i32 + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::asset_manager::MediaArchiveMode;

    fn two_pixel_test_font() -> FntFile {
        const HEADER_BYTES: usize = 28;
        const LOOKUP_BYTES: usize = 65_536 * 2;
        let mut data = vec![0; HEADER_BYTES + LOOKUP_BYTES + 2];
        for (index, word) in [0x546e_6f66_u32, 20, 1, 1, 2, 1, 2].into_iter().enumerate() {
            data[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        data[HEADER_BYTES + usize::from(b'A') * 2] = 1;
        data[HEADER_BYTES + LOOKUP_BYTES..].copy_from_slice(&[2, 0x80]);
        FntFile::from_bytes(&data).expect("two-pixel FNT fixture")
    }

    fn compare_composed_splash_to_retail_shape(
        assets: &AssetManager,
        font: &FntFile,
        art: SplashArt,
        width: usize,
        height: usize,
    ) {
        let shape_name = art.shape_for_width(width as u32);
        let yr_original_style = art.palette == YR_SPLASH.palette;
        let resolver = ShellUiAssetResolver::new(assets, yr_original_style);
        let palette = Palette::from_bytes(
            resolver
                .resolve(ShellUiAssetPair::new(
                    ShellUiAssetLocator::archive(art.palette, "ra2.mix -> local.mix"),
                    ShellUiAssetLocator::active(art.palette),
                ))
                .expect("retail splash palette")
                .bytes,
        )
        .expect("parse retail splash palette");
        let shape = ShpFile::from_bytes(
            resolver
                .resolve(ShellUiAssetPair::new(
                    ShellUiAssetLocator::archive(shape_name, "ra2.mix -> local.mix"),
                    ShellUiAssetLocator::active(shape_name),
                ))
                .expect("retail splash shape")
                .bytes,
        )
        .expect("parse retail splash shape");
        let frame = shape.frames.first().expect("splash frame 0");
        let frame_rgba = shape
            .frame_to_rgba(0, &palette)
            .expect("decode retail splash frame 0");
        let composed = compose_startup_splash(assets, None, font, width, height, yr_original_style)
            .expect("compose production startup splash");

        let origin_x = centered_offset(width as i32, shape.width as i32) + frame.frame_x as i32;
        let origin_y = centered_offset(height as i32, shape.height as i32) + frame.frame_y as i32;
        let mut compared = 0;
        for y in 80..height.saturating_sub(60) {
            for x in width / 8..width * 7 / 8 {
                let source_x = x as i32 - origin_x;
                let source_y = y as i32 - origin_y;
                if !(0..frame.frame_width as i32).contains(&source_x)
                    || !(0..frame.frame_height as i32).contains(&source_y)
                {
                    continue;
                }
                let source_offset =
                    (source_y as usize * frame.frame_width as usize + source_x as usize) * 4;
                if frame_rgba[source_offset + 3] == 0 {
                    continue;
                }
                let destination_offset = (y * width + x) * 4;
                assert_eq!(
                    &composed[destination_offset..destination_offset + 4],
                    &frame_rgba[source_offset..source_offset + 4],
                    "{shape_name} pixel at ({x}, {y}) differs from production composition"
                );
                compared += 1;
            }
        }
        assert!(
            compared > 10_000,
            "comparison covered only {compared} pixels"
        );
    }

    #[test]
    fn exact_640_selects_small_every_other_width_selects_large() {
        for yr_original_style in [false, true] {
            let art = splash_art(yr_original_style);
            assert_eq!(art.shape_for_width(640), art.small);
            assert_eq!(art.shape_for_width(639), art.large);
            assert_eq!(art.shape_for_width(800), art.large);
            assert_eq!(art.shape_for_width(1920), art.large);
        }
        // The unchecked default uses RA2's unsuffixed artwork; the option selects YR.
        assert_eq!(splash_art(false).large, "GLSL.SHP");
        assert_eq!(splash_art(true).large, "GLSLMD.SHP");
    }

    #[test]
    fn fitted_windows_scale_the_800x600_splash_onto_the_menu_page() {
        // The two windows gamemd's shell runs at keep its 1:1 composition.
        for (width, height) in [(800, 600), (640, 480)] {
            assert_eq!(
                splash_placement(width, height),
                (
                    [width, height],
                    RectPx::new(0, 0, width as i32, height as i32)
                )
            );
        }
        // Any other window scales the 800x600 composition to the largest
        // centred fit, with black bars on the spare sides.
        assert_eq!(
            splash_placement(1920, 1080),
            ([800, 600], RectPx::new(240, 0, 1440, 1080))
        );
        assert_eq!(
            splash_placement(1280, 1024),
            ([800, 600], RectPx::new(0, 32, 1280, 960))
        );
        // That is where the fitted main menu puts its page, so the menu that
        // follows the splash lines up with it.
        for (width, height) in [(1920, 1080), (1024, 768), (3024, 1964), (640, 400)] {
            let menu = crate::ui::main_menu_shell::compute_responsive_layout(width, height);
            assert_eq!(
                splash_placement(width, height),
                ([800, 600], menu.scale.dialog_rect())
            );
        }
    }

    #[test]
    fn centered_offset_uses_signed_truncation_toward_zero() {
        assert_eq!(centered_offset(800, 800), 0);
        assert_eq!(centered_offset(801, 800), 0);
        assert_eq!(centered_offset(799, 800), 0);
        assert_eq!(centered_offset(798, 800), -1);
        assert_eq!(centered_offset(806, 800), 3);
    }

    #[test]
    fn clipped_blit_preserves_black_border_and_skips_transparent_pixels() {
        let mut destination = vec![0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255];
        let source = vec![
            10, 20, 30, 255, 40, 50, 60, 0, 70, 80, 90, 255, 100, 110, 120, 255,
        ];
        blit_rgba_clipped(
            &mut destination,
            2,
            2,
            &source,
            2,
            2,
            -1,
            0,
            RgbaBlend::Copy,
        );
        assert_eq!(&destination[0..4], &[0, 0, 0, 255]);
        assert_eq!(&destination[8..12], &[100, 110, 120, 255]);
    }

    #[test]
    fn modern_splash_text_blends_over_art_while_retail_keeps_its_binary_mask() {
        let font = two_pixel_test_font();
        let original = [20, 40, 80, 255, 30, 60, 90, 255];
        let mut modern = original;
        draw_text_with_rasterizer(&mut modern, 2, 1, &font, "A", 0, 0, &mut |_, _| {
            Some(vec![255, 255, 255, 128, 255, 255, 255, 0])
        });
        assert_eq!(&modern[..4], &[138, 148, 168, 255]);
        assert_eq!(&modern[4..], &original[4..]);

        let mut retail = original;
        draw_text(&mut retail, 2, 1, &font, "A", 0, 0);
        assert_eq!(&retail[..4], &TEXT_COLOR);
        assert_eq!(&retail[4..], &original[4..]);
    }

    #[test]
    fn resized_footer_mask_applies_coverage_once_and_keeps_canvas_opaque() {
        let font = two_pixel_test_font();
        let mut mask = [255, 255, 255, 0, 255, 255, 255, 0];
        draw_text_with_rasterizer(&mut mask, 2, 1, &font, "A", 0, 0, &mut |_, _| {
            Some(vec![255, 255, 255, 128, 255, 255, 255, 0])
        });
        let image = image::RgbaImage::from_raw(2, 1, mask.to_vec()).expect("footer mask");
        let image = image::imageops::resize(&image, 13, 1, image::imageops::FilterType::Lanczos3);
        assert!(image.pixels().any(|pixel| (1..255).contains(&pixel[3])));
        assert!(image.pixels().all(|pixel| pixel.0[..3] == [255; 3]));

        let mut canvas = [0; 13 * 4];
        for pixel in canvas.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        blit_rgba_clipped(
            &mut canvas,
            13,
            1,
            image.as_raw(),
            13,
            1,
            0,
            0,
            RgbaBlend::SourceOver,
        );
        for (source, actual) in image.pixels().zip(canvas.chunks_exact(4)) {
            assert_eq!(actual, &[source[3], source[3], source[3], 255]);
        }
    }

    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn retail_composition_draws_both_looks_and_keeps_missing_small_art_text_only() {
        use crate::assets::asset_manager::AssetManager;

        let retail_dir = std::path::PathBuf::from(
            std::env::var_os("RA2_DIR").expect("set RA2_DIR to the retail RA2/YR install"),
        );
        let assets = AssetManager::new(&retail_dir, MediaArchiveMode::STOCK_DIGITAL)
            .expect("load retail RA2/YR assets");
        let font = FntFile::from_bytes(assets.get_ref("GAME.FNT").expect("retail GAME.FNT"))
            .expect("parse retail GAME.FNT");

        compare_composed_splash_to_retail_shape(&assets, &font, YR_SPLASH, 800, 600);
        // The unchecked default draws RA2's unsuffixed originals the same way.
        compare_composed_splash_to_retail_shape(&assets, &font, RA2_SPLASH, 800, 600);

        let small_art_available = assets.get_ref(YR_SPLASH.small).is_some();
        if !small_art_available {
            let composed = compose_startup_splash(&assets, None, &font, 640, 480, true)
                .expect("compose missing-small-art splash");
            let center = (240 * 640 + 320) * 4;
            assert_eq!(&composed[center..center + 4], &[0, 0, 0, 255]);
            let white_loading_pixels = (10..100)
                .flat_map(|x| (10..30).map(move |y| (y * 640 + x) * 4))
                .filter(|offset| composed[*offset..*offset + 4] == TEXT_COLOR)
                .count();
            assert!(white_loading_pixels > 0, "missing small art retains text");
        } else {
            compare_composed_splash_to_retail_shape(&assets, &font, YR_SPLASH, 640, 480);
        }

        // The web copy includes Chinese and long mixed-language lines. Verify
        // it fits both shell sizes with the real localized font, without
        // clipping either footer column or growing into the reserved artwork.
        for (width, height) in [(640, 480), (800, 600)] {
            let column_width = (width - 40) / 2;
            let mut max_lines = 0;
            for paragraphs in [&WERHD_DISCLAIMER[..], &WERHD_COPYRIGHT[..]] {
                for ch in paragraphs.iter().flat_map(|line| line.chars()) {
                    assert!(font.glyph(ch as u16).is_some(), "missing footer glyph {ch}");
                }
                let lines = wrap_footer_lines(&font, paragraphs, column_width, 13);
                max_lines = max_lines.max(lines.len());
                assert!(
                    lines
                        .iter()
                        .all(|line| { scaled_footer_width(&font, line, 13) <= column_width })
                );
            }
            assert!(max_lines as u32 * 15 + 20 <= height / 3);
            let branded = compose_branded_startup_splash(None, &font, width, height)
                .expect("compose approved Apple splash with web footer");
            assert_eq!(branded.len(), width as usize * height as usize * 4);
            assert!(branded.chunks_exact(4).all(|pixel| pixel[3] == 255));
        }
    }

    #[test]
    fn the_hold_anchors_at_the_first_present_and_never_rearms() {
        let start = Instant::now();
        let mut hold = VisibleHold::default();
        // Unarmed: still active, so a transient first-present failure retries
        // rather than dropping the splash.
        assert!(hold.is_active(start + Duration::from_secs(600)));

        hold.mark_presented(start);
        // The per-frame re-present calls this again on every frame; if it
        // re-armed, the splash would never expire.
        hold.mark_presented(start + Duration::from_secs(3));
        hold.mark_presented(start + MINIMUM_VISIBLE_TIME);

        assert!(hold.is_active(start));
        assert!(hold.is_active(start + MINIMUM_VISIBLE_TIME - Duration::from_millis(1)));
        // The hold is measured from the present, so initialization work that
        // runs after it is spent inside the five seconds, not added to them.
        assert!(!hold.is_active(start + MINIMUM_VISIBLE_TIME));
        assert!(!hold.is_active(start + MINIMUM_VISIBLE_TIME + Duration::from_secs(1)));
    }
}
