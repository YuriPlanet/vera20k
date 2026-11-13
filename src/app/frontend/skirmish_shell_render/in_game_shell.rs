//! Active-scenario shell composition: 621E90 -> 72F540, geometry 72FC60.
//! A complete RGB565 surface over the match frame: gamemd lays it out at the
//! game resolution, so a zoomed match zooms it with the battlefield. Here the
//! game resolution is the frame in sidebar pixels ([`in_game_shell_size`]),
//! so at a larger Sidebar size (Options) the shell grows with the sidebar and
//! its column covers the battle's.

use crate::app::AppState;
use crate::render::batch::SpriteInstance;
use crate::render::scaled_copy::ScaledSurface;
use crate::render::sidebar_chrome::{SidebarChromeAtlas, SidebarChromeEntry};
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::in_game_shell::{InGameShellLayout, InGameShellSizes};
use crate::ui::sidebar::command_bar::spacer_pieces;

pub(super) fn shell_layout(
    atlas: &SidebarChromeAtlas,
    width: i32,
    height: i32,
    center_background: bool,
) -> Option<InGameShellLayout> {
    let size = |entry: SidebarChromeEntry| entry.pixel_size.map(|v| v as i32);
    let layout = InGameShellLayout::new(
        width,
        height,
        InGameShellSizes {
            background_small: size(atlas.background_small?),
            background_medium: size(atlas.background_medium?),
            background_large: size(atlas.background_large?),
            credits: size(atlas.top_strip_thin?),
            top: size(atlas.top_strip_sidebar?),
            radar: size(atlas.radar),
            side1: size(atlas.side1),
            side2: size(atlas.side2),
            side3: size(atlas.side3),
            addon: size(atlas.unknown_top_housing?),
            bottom_spacer: size(atlas.command_bar.spacer?),
            left_cap: size(atlas.command_bar.left_cap[2]?),
            button_background: size(atlas.command_bar.background?),
            right_cap: size(atlas.command_bar.right_cap?),
        },
    )?;
    // Phobos `[Sidebar] CenterPauseMenuBackground`; the hook rewrites the same
    // rectangle every active-game modal paints first. Without the switch the
    // art is stretched over the battlefield, VERA's frames being any size.
    Some(if center_background {
        layout.with_centered_background(width, height)
    } else {
        layout.with_battlefield_background()
    })
}

/// The pixels every active-game shell lays out, paints and hit-tests in: the
/// match frame in sidebar pixels (`SidebarFrame::layout_size`), the frame
/// itself at Normal.
pub(crate) fn in_game_shell_size(state: &AppState) -> (i32, i32) {
    let (width, height) = state.sidebar_frame().layout_size();
    (width as i32, height as i32)
}

/// The pointer in [`in_game_shell_size`]'s pixels: the shell pixel shown at
/// the frame pixel under it.
pub(crate) fn in_game_shell_cursor(state: &AppState) -> (i32, i32) {
    let (x, y) = state.cursor_position();
    state
        .sidebar_frame()
        .pixel_to_layout(x.round() as i32, y.round() as i32)
}

/// An active-game shell's pixel space and its native layout there.
#[derive(Debug, Clone, Copy)]
pub(crate) struct InGameShellGeometry {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) layout: InGameShellLayout,
    /// The shell button art's canvas.
    pub(crate) button_size: [i32; 2],
}

/// Shared active-game art gate for the implemented full-screen modals.
pub(crate) fn native_in_game_shell_active(state: &AppState) -> bool {
    state.frontend.screen == crate::app::GameScreen::InGame
        && matches!(
            state.match_state.match_presentation.in_game_menu,
            crate::ui::pause_menu::InGameMenuState::Menu
                | crate::ui::pause_menu::InGameMenuState::AbortConfirm
                | crate::ui::pause_menu::InGameMenuState::Sound
                | crate::ui::pause_menu::InGameMenuState::Keyboard
                | crate::ui::pause_menu::InGameMenuState::SavedGame(_)
                | crate::ui::pause_menu::InGameMenuState::Diplomacy
                | crate::ui::pause_menu::InGameMenuState::MissionBriefing
        )
        && (matches!(
            state.match_state.match_presentation.in_game_menu,
            crate::ui::pause_menu::InGameMenuState::Menu
                | crate::ui::pause_menu::InGameMenuState::AbortConfirm
        ) || state.frontend.skirmish_shell_chrome.is_some())
        && crate::app::presentation::sidebar_render::current_sidebar_chrome(state)
            .is_some_and(|atlas| atlas.in_game_shell.panel_tile.is_some())
        && current_in_game_shell_layout(state).is_some()
}

pub(crate) fn current_in_game_shell_layout(state: &AppState) -> Option<InGameShellGeometry> {
    let atlas = crate::app::presentation::sidebar_render::current_sidebar_chrome(state)?;
    let (width, height) = in_game_shell_size(state);
    // The Phobos switch is absent from builds without the capability, so those
    // keep the native top-left placement.
    #[cfg(feature = "phobos")]
    let center_background = state
        .match_state
        .match_presentation
        .center_pause_menu_background;
    #[cfg(not(feature = "phobos"))]
    let center_background = false;
    Some(InGameShellGeometry {
        width,
        height,
        layout: shell_layout(atlas, width, height, center_background)?,
        button_size: atlas.in_game_shell.buttons[0]?
            .pixel_size
            .map(|value| value as i32),
    })
}

/// Native blit at canvas size, with an optional source-preserving clip. Only
/// the background and LSPACER are stretched ([`push_art_scaled`]).
pub(super) fn push_art(
    out: &mut Vec<SpriteInstance>,
    entry: SidebarChromeEntry,
    rect: RectPx,
    clip: RectPx,
) {
    let [w, h] = entry.pixel_size.map(|v| v as i32);
    push_art_scaled(out, entry, RectPx::new(rect.x, rect.y, w, h), clip);
}

/// The whole canvas drawn over `rect`, scaled to its size, then clipped as
/// [`push_art`] clips; at the canvas size it is that blit.
fn push_art_scaled(
    out: &mut Vec<SpriteInstance>,
    entry: SidebarChromeEntry,
    rect: RectPx,
    clip: RectPx,
) {
    let x = rect.x.max(clip.x);
    let y = rect.y.max(clip.y);
    let right = (rect.x + rect.w).min(clip.x + clip.w);
    let bottom = (rect.y + rect.h).min(clip.y + clip.h);
    if right <= x || bottom <= y {
        return;
    }
    let sx = entry.uv_size[0] / rect.w as f32;
    let sy = entry.uv_size[1] / rect.h as f32;
    out.push(SpriteInstance {
        position: [x as f32, y as f32],
        size: [(right - x) as f32, (bottom - y) as f32],
        uv_origin: [
            entry.uv_origin[0] + (x - rect.x) as f32 * sx,
            entry.uv_origin[1] + (y - rect.y) as f32 * sy,
        ],
        uv_size: [(right - x) as f32 * sx, (bottom - y) as f32 * sy],
        tint: [1.0; 3],
        alpha: 1.0,
        depth: 0.0006,
        ..Default::default()
    });
}

pub(super) fn background_instances(
    atlas: &SidebarChromeAtlas,
    layout: InGameShellLayout,
    width: i32,
    height: i32,
) -> Vec<SpriteInstance> {
    let mut out = Vec::new();
    let screen = RectPx::new(0, 0, width, height);
    let background = match width {
        640 => atlas.background_small,
        800 => atlas.background_medium,
        _ => atlas.background_large,
    };
    // Original 72F540 order; the background clear is performed by the render
    // pass. The background fills its layout rectangle (`shell_layout`).
    if let Some(entry) = background {
        push_art_scaled(&mut out, entry, layout.background, screen);
    }
    for (entry, rect) in [
        (atlas.top_strip_thin, layout.credits),
        (atlas.top_strip_sidebar, layout.top),
        (Some(atlas.radar), layout.radar),
        (Some(atlas.side1), layout.side1),
    ] {
        if let Some(entry) = entry {
            push_art(&mut out, entry, rect, screen);
        }
    }
    if let Some(tile) = atlas.in_game_shell.panel_tile {
        for row in 0..layout.side2_count {
            push_art(
                &mut out,
                tile,
                layout.side2.translate(0, row * layout.side2.h),
                screen,
            );
        }
    }
    push_art(&mut out, atlas.side3, layout.side3, screen);
    if let Some(entry) = atlas.unknown_top_housing {
        push_art(&mut out, entry, layout.addon, screen);
    }
    // LSPACER reaches the closed left cap as the match's bar lays it out
    // (`spacer_pieces`).
    if let Some(entry) = atlas.command_bar.spacer {
        let strip = layout.bottom_clip;
        let cap_x = (layout.closed_left_cap.x - layout.bottom_spacer.x).max(0) as u32;
        let pieces = spacer_pieces(cap_x, entry.pixel_size[0] as u32);
        for piece in pieces.into_iter().flatten() {
            let rect = RectPx::new(
                layout.bottom_spacer.x + piece.x as i32,
                layout.bottom_spacer.y,
                piece.w as i32,
                layout.bottom_spacer.h,
            );
            let columns = entry.columns(piece.source_x as f32, piece.source_w as f32);
            push_art_scaled(&mut out, columns, rect, strip);
        }
    }
    // 621FCE passes false: closed LENDCAP frame2, without BTTNBKGD repeats.
    if let Some(entry) = atlas.command_bar.left_cap[2] {
        push_art(&mut out, entry, layout.closed_left_cap, screen);
    }
    if let Some(entry) = atlas.command_bar.right_cap {
        push_art(&mut out, entry, layout.right_cap, screen);
    }
    out
}

pub(super) struct InGameShellFrame {
    pub art: Vec<SpriteInstance>,
    pub controls: Vec<SpriteInstance>,
    pub texts: Vec<crate::render::shell_text::ShellTextDraw>,
    pub label: &'static str,
}

/// One compositor for active-game shells: sidebar art, optional generic
/// controls, individually clipped text and the software cursor, in native order.
pub(super) fn render_in_game_shell_frame(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
    frame: InGameShellFrame,
) -> anyhow::Result<()> {
    render_shell_frame(
        state,
        encoder,
        destination,
        frame,
        ShellFrameArt::Sidebar,
        ShellFrameOverlay::default(),
        ShellFrameCursor::Visible,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ShellFrameArt {
    Sidebar,
    Launcher,
    /// Original mode-2 overlays preserve the already composed backing.
    Retained,
    /// No compatible completed backing exists (for example, just after resize).
    Clear,
}

/// Static native progress dialogs suppress the pointer while their caller
/// blocks; ordinary interactive shell frames paint it last.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ShellFrameCursor {
    Visible,
    Hidden,
}

#[derive(Default)]
pub(super) struct ShellFrameOverlay {
    /// A dialog background from the score art's atlas (`SCRBK` under
    /// BC6), drawn before the parent art.
    pub score_backdrop: Option<SpriteInstance>,
    /// Shared ProgressScreen atlas draws, retained by the frontend owner.
    pub progress_meter: Vec<SpriteInstance>,
    /// Map thumbnail from the shared preview cache, below ordinary controls.
    pub preview: Option<SpriteInstance>,
    pub controls: Vec<SpriteInstance>,
    pub texts: Vec<crate::render::shell_text::ShellTextDraw>,
    /// VERA's enlarged map preview, over everything but the cursor.
    pub zoom: Option<super::preview::ZoomDraws>,
    /// The page's chrome goes through VERA's online client's smoothing
    /// ([`crate::render::batch::BatchRenderer::draw_chrome`]).
    pub smooth_art: bool,
}

/// Shared shell compositor. Only the parent art atlas differs between launcher
/// and active-game children; popups paint after all ordinary controls. The
/// launcher paints at the window's size. An active-game shell paints at
/// [`in_game_shell_size`] into the shell surface, which a second pass copies
/// over the match frame at the Sidebar size, as the sidebar's surface is;
/// the cursor goes on top at the frame's own size, as over the battlefield.
/// A zoomed match then scales the frame up. Texts laid out for the window
/// ([`crate::render::bit_font::BitFont::raster`], set by the caller through
/// `window_ui::shell_raster`) are drawn there after that, with the popups
/// and the cursor over them (`app::presentation::window_ui`).
pub(super) fn render_shell_frame(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
    frame: InGameShellFrame,
    art: ShellFrameArt,
    overlay: ShellFrameOverlay,
    cursor_policy: ShellFrameCursor,
) -> anyhow::Result<()> {
    let width = state.render_width();
    let height = state.render_height();
    let window_raster = Some(state.renderer.bit_font.raster())
        .filter(|raster| *raster != crate::render::bit_font::TextRaster::NATIVE);
    let sidebar_frame = matches!(art, ShellFrameArt::Sidebar).then(|| state.sidebar_frame());
    let (shell_width, shell_height) =
        sidebar_frame.map_or((width, height), |frame| frame.layout_size());
    if let Some(frame) = sidebar_frame {
        // Frame-sized, so the shell and the cursor share the frame's camera:
        // the shell fills the top-left corner at its own size.
        let renderer = &mut state.renderer;
        let gpu = &renderer.gpu;
        let surface = renderer.in_game_shell_surface.get_or_insert_with(|| {
            ScaledSurface::new(
                &gpu.device,
                "In-Game Shell Surface",
                gpu.surface_format,
                (width, height),
                Some(wgpu::TextureFormat::Depth32Float),
            )
        });
        surface.resize(&gpu.device, (width, height));
        surface.place(
            &gpu.queue,
            frame.layout_origin(),
            frame.scale(),
            frame.is_sharp(),
        );
    }
    let background_texture = match art {
        ShellFrameArt::Sidebar => Some(
            &crate::app::presentation::sidebar_render::current_sidebar_chrome(state)
                .expect("active side art")
                .texture,
        ),
        ShellFrameArt::Launcher => Some(
            &state
                .frontend
                .skirmish_shell_chrome
                .as_ref()
                .expect("launcher art")
                .texture,
        ),
        ShellFrameArt::Retained | ShellFrameArt::Clear => None,
    };
    let control_atlas = state.frontend.skirmish_shell_chrome.as_ref();
    let InGameShellFrame {
        art: instances,
        controls,
        texts,
        label,
    } = frame;
    let batch = &state.renderer.batch_renderer;
    let gpu = &state.renderer.gpu;
    // The frame dispatcher records this instead of the battlefield, so this
    // shared camera upload cannot alter previously recorded world commands.
    batch.update_camera(
        gpu,
        width as f32,
        height as f32,
        0.0,
        0.0,
        1.0,
        crate::render::batch::DepthAxis::NONE,
    );
    let backdrop_buffer = overlay
        .score_backdrop
        .as_ref()
        .and_then(|instance| batch.create_instance_buffer(gpu, &[*instance]));
    let background_buffer = batch.create_instance_buffer(gpu, &instances);
    let control_buffer = batch.create_instance_buffer(gpu, &controls);
    let preview_buffer = overlay
        .preview
        .as_ref()
        .and_then(|instance| batch.create_instance_buffer(gpu, &[*instance]));
    let text_buffers: Vec<_> = texts
        .iter()
        .map(|d| batch.create_instance_buffer(gpu, &d.instances))
        .collect();
    // The open list goes to the window over its texts, moved and grown
    // there as they are.
    let overlay_buffer = match window_raster {
        Some(raster) => {
            let controls: Vec<_> = overlay
                .controls
                .iter()
                .map(|quad| raster.quad(*quad))
                .collect();
            batch.create_instance_buffer(gpu, &controls)
        }
        None => batch.create_instance_buffer(gpu, &overlay.controls),
    };
    let progress_buffer = batch.create_instance_buffer(gpu, &overlay.progress_meter);
    let overlay_text_buffers: Vec<_> = overlay
        .texts
        .iter()
        .map(|d| batch.create_instance_buffer(gpu, &d.instances))
        .collect();
    let zoom_buffers = overlay.zoom.as_ref().map(|zoom| zoom.buffers(state));
    let cursor_raster = window_raster.map(|_| {
        crate::app::presentation::window_ui::dialog_raster(state)
            .unwrap_or(crate::render::bit_font::TextRaster::NATIVE)
    });
    let cursor: Vec<_> = match cursor_policy {
        ShellFrameCursor::Visible => {
            crate::app::frontend::shell_pass::software_cursor(state, super::SHELL_CURSOR_DEPTH)
                .map(|(_, instance)| cursor_raster.map_or(instance, |raster| raster.quad(instance)))
                .into_iter()
                .collect()
        }
        ShellFrameCursor::Hidden => Vec::new(),
    };
    let cursor_buffer = batch.create_instance_buffer(gpu, &cursor);
    let cursor_texture = state
        .match_state
        .match_presentation
        .software_cursor
        .as_ref()
        .and_then(|c| c.get(crate::app::types::CursorId::Default))
        .and_then(|s| s.frames.first())
        .map(|f| &f.texture);
    let source = state.renderer.shell_surface_presenter.source_render_view();
    let shell_surface = sidebar_frame.and(state.renderer.in_game_shell_surface.as_ref());
    let (color, depth) = match shell_surface {
        Some(surface) => (surface.color_view(), surface.depth_view()),
        None => (&source, &state.renderer.depth_view),
    };
    let color_load = match art {
        ShellFrameArt::Retained => wgpu::LoadOp::Load,
        _ => wgpu::LoadOp::Clear(wgpu::Color::BLACK),
    };
    let mut pass = begin_shell_pass(encoder, label, color, depth, color_load, CLEAR_DEPTH);
    // Both targets are at least frame-sized (the presenter's is the
    // window's); the frame is their top-left corner.
    pass.set_viewport(0.0, 0.0, width as f32, height as f32, 0.0, 1.0);
    if let (Some((buffer, count)), Some(art)) =
        (&backdrop_buffer, state.frontend.campaign_score_art.as_ref())
    {
        batch.draw_with_buffer_passthrough(&mut pass, &art.texture, buffer, *count);
    }
    if let (Some((buffer, count)), Some(background_texture)) =
        (&background_buffer, background_texture)
    {
        batch.draw_chrome(
            &mut pass,
            background_texture,
            buffer,
            *count,
            overlay.smooth_art,
        );
    }
    if let (Some((buffer, count)), Some(preview)) = (
        &preview_buffer,
        state.frontend.skirmish_preview_texture.as_ref(),
    ) {
        batch.draw_with_buffer_passthrough(&mut pass, &preview.texture, buffer, *count);
    }
    if let (Some((buffer, count)), Some(control_atlas)) = (&control_buffer, control_atlas) {
        batch.draw_chrome(
            &mut pass,
            &control_atlas.texture,
            buffer,
            *count,
            overlay.smooth_art,
        );
    }
    for (draw, buffer) in texts.iter().zip(&text_buffers) {
        if let Some((buffer, count)) = buffer.as_ref().filter(|_| window_raster.is_none()) {
            let x = draw.scissor.x.min(shell_width);
            let y = draw.scissor.y.min(shell_height);
            let w = draw.scissor.w.min(shell_width - x);
            let h = draw.scissor.h.min(shell_height - y);
            if w == 0 || h == 0 {
                continue;
            }
            pass.set_scissor_rect(x, y, w, h);
            batch.draw_with_buffer_passthrough(
                &mut pass,
                state.renderer.bit_font.atlas(),
                buffer,
                *count,
            );
        }
    }
    pass.set_scissor_rect(0, 0, shell_width, shell_height);
    if let (Some((buffer, count)), Some(atlas), None) =
        (&overlay_buffer, control_atlas, window_raster)
    {
        batch.draw_chrome(
            &mut pass,
            &atlas.texture,
            buffer,
            *count,
            overlay.smooth_art,
        );
    }
    if let (Some((buffer, count)), Some(atlas)) = (
        &progress_buffer,
        state.frontend.loading_screen_atlas.as_ref(),
    ) {
        batch.draw_with_buffer_passthrough(&mut pass, &atlas.texture, buffer, *count);
    }
    for (draw, buffer) in overlay.texts.iter().zip(&overlay_text_buffers) {
        if let Some((buffer, count)) = buffer.as_ref().filter(|_| window_raster.is_none()) {
            let x = draw.scissor.x.min(shell_width);
            let y = draw.scissor.y.min(shell_height);
            let w = draw.scissor.w.min(shell_width - x);
            let h = draw.scissor.h.min(shell_height - y);
            if w == 0 || h == 0 {
                continue;
            }
            pass.set_scissor_rect(x, y, w, h);
            batch.draw_with_buffer_passthrough(
                &mut pass,
                state.renderer.bit_font.atlas(),
                buffer,
                *count,
            );
        }
    }
    if let (Some(buffers), Some(atlas)) = (&zoom_buffers, control_atlas) {
        pass.set_scissor_rect(0, 0, shell_width, shell_height);
        super::preview::draw_zoom(&mut pass, state, atlas, buffers);
    }
    drop(pass);
    if let Some(surface) = shell_surface {
        let mut pass = begin_shell_pass(
            encoder,
            label,
            &source,
            &state.renderer.depth_view,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            CLEAR_DEPTH,
        );
        pass.set_viewport(0.0, 0.0, width as f32, height as f32, 0.0, 1.0);
        surface.draw(&mut pass, None, [0, 0, width, height]);
    }
    let presenter = &state.renderer.shell_surface_presenter;
    if matches!(art, ShellFrameArt::Sidebar | ShellFrameArt::Launcher) {
        presenter.stage_cursor_free_composition(
            &state.renderer.retail_screenshot_frame_cache,
            &gpu.device,
            encoder,
            (width, height),
        );
    }
    if let (Some((buffer, count)), Some(texture), None) =
        (&cursor_buffer, cursor_texture, window_raster)
    {
        let mut pass = begin_shell_pass(
            encoder,
            "Shell cursor after retained composition",
            &source,
            &state.renderer.depth_view,
            wgpu::LoadOp::Load,
            wgpu::LoadOp::Load,
        );
        pass.set_viewport(0.0, 0.0, width as f32, height as f32, 0.0, 1.0);
        batch.draw_with_buffer_passthrough(&mut pass, texture, buffer, *count);
    }
    match state.match_zoom_pass() {
        Some(zoom) => {
            presenter.encode_present(encoder, zoom.color_texture());
            zoom.draw(encoder, &destination.create_view(&Default::default()));
        }
        None => presenter.encode_present(encoder, destination),
    }
    if let Some(raster) = window_raster {
        let view = destination.create_view(&Default::default());
        let mut pass = state.renderer.window_ui.begin_pass(state, encoder, &view);
        let atlas = state.renderer.bit_font.atlas();
        for (draw, buffer) in texts.iter().zip(&text_buffers) {
            if let Some((buffer, count)) = buffer {
                pass.draw(atlas, buffer, *count, draw.scissor);
            }
        }
        if let (Some((buffer, count)), Some(control_atlas)) = (&overlay_buffer, control_atlas) {
            let shell = crate::render::shell_text::ScissorRect::of(
                crate::render::shell_text::TextRect {
                    x: 0,
                    y: 0,
                    w: shell_width,
                    h: shell_height,
                },
                raster,
            );
            pass.draw(&control_atlas.texture, buffer, *count, shell);
        }
        for (draw, buffer) in overlay.texts.iter().zip(&overlay_text_buffers) {
            if let Some((buffer, count)) = buffer {
                pass.draw(atlas, buffer, *count, draw.scissor);
            }
        }
        if let (Some((buffer, count)), Some(texture)) = (&cursor_buffer, cursor_texture) {
            let frame = crate::app::presentation::window_ui::frame_rect(state);
            pass.draw_cursor(texture, buffer, *count, frame);
        }
    }
    Ok(())
}

const CLEAR_DEPTH: wgpu::LoadOp<f32> = wgpu::LoadOp::Clear(1.0);

/// A shell pass over `color` and `depth`, each cleared or kept as given.
fn begin_shell_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    label: &'static str,
    color: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    color_load: wgpu::LoadOp<wgpu::Color>,
    depth_load: wgpu::LoadOp<f32>,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: color,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: color_load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(wgpu::Operations {
                load: depth_load,
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// At the 1121x741 frame the 856x736 background reaches the 953x709
    /// battlefield's far corner and still shows its whole canvas.
    #[test]
    fn scaled_background_samples_its_whole_canvas_over_the_battlefield() {
        let entry = SidebarChromeEntry {
            uv_origin: [0.25, 0.5],
            uv_size: [0.5, 0.25],
            pixel_size: [856.0, 736.0],
        };
        let mut out = Vec::new();
        let battlefield = RectPx::new(0, 0, 953, 709);
        push_art_scaled(&mut out, entry, battlefield, RectPx::new(0, 0, 1121, 741));
        let [sprite] = out.as_slice() else {
            panic!("one background sprite, got {}", out.len());
        };
        assert_eq!((sprite.position, sprite.size), ([0.0; 2], [953.0, 709.0]));
        assert_eq!(sprite.uv_origin, entry.uv_origin);
        for (uv, expected) in sprite.uv_size.into_iter().zip(entry.uv_size) {
            assert!((uv - expected).abs() < 1e-6, "{uv} != {expected}");
        }
    }
}
