//! Active-scenario shell composition: 621E90 -> 72F540, geometry 72FC60.
//! A complete RGB565 surface over the match frame: gamemd lays it out at the
//! game resolution, so a zoomed match zooms it with the battlefield. Here the
//! game resolution is the frame in sidebar pixels ([`in_game_shell_size`]),
//! so at a larger Sidebar size (Options) the shell grows with the sidebar and
//! its column covers the battle's.

use super::in_game_options;
use crate::app::AppState;
use crate::render::batch::SpriteInstance;
use crate::render::scaled_copy::ScaledSurface;
use crate::render::sidebar_chrome::{SidebarChromeAtlas, SidebarChromeEntry};
use crate::ui::shell::geom::{self, RectPx};
use crate::ui::shell::in_game_options::{build_in_game_options_descriptor, control};
use crate::ui::shell::in_game_shell::{InGameShellLayout, InGameShellSizes};
use crate::ui::shell::layout::{InGameOptionsAnchor, layout_pass_in_game_options};

pub(super) fn shell_layout(
    atlas: &SidebarChromeAtlas,
    width: i32,
    height: i32,
) -> Option<InGameShellLayout> {
    let size = |entry: SidebarChromeEntry| entry.pixel_size.map(|v| v as i32);
    InGameShellLayout::new(
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
    )
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
                | crate::ui::pause_menu::InGameMenuState::Options
                | crate::ui::pause_menu::InGameMenuState::SavedGame(_)
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
    Some(InGameShellGeometry {
        width,
        height,
        layout: shell_layout(atlas, width, height)?,
        button_size: atlas.in_game_shell.buttons[0]?
            .pixel_size
            .map(|value| value as i32),
    })
}

/// Native blit at canvas size, with an optional source-preserving clip. No art
/// is stretched to fill unused pixels at resolutions larger than its canvas.
pub(super) fn push_art(
    out: &mut Vec<SpriteInstance>,
    entry: SidebarChromeEntry,
    rect: RectPx,
    clip: RectPx,
) {
    let x = rect.x.max(clip.x);
    let y = rect.y.max(clip.y);
    let right = (rect.x + entry.pixel_size[0] as i32).min(clip.x + clip.w);
    let bottom = (rect.y + entry.pixel_size[1] as i32).min(clip.y + clip.h);
    if right <= x || bottom <= y {
        return;
    }
    let sx = entry.uv_size[0] / entry.pixel_size[0];
    let sy = entry.uv_size[1] / entry.pixel_size[1];
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
    // Original 72F540 order; the background clear is performed by the render pass.
    for (entry, rect) in [
        (background, layout.background),
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
    if let Some(entry) = atlas.command_bar.spacer {
        push_art(&mut out, entry, layout.bottom_spacer, layout.bottom_clip);
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

pub(crate) fn render_in_game_options_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let InGameShellGeometry {
        width,
        height,
        layout,
        button_size,
    } = current_in_game_shell_layout(state).expect("active shell geometry");
    let sound_raw = geom::dlu_rect(425, 122, 108, 23);
    let anchor = InGameOptionsAnchor {
        button_canvas_w: button_size[0],
        button_canvas_h: button_size[1],
        button_stack_top_y: layout.button_rect(sound_raw, button_size).y,
        // Original60B350 active branch60B3FC..60B41C, BBB child686.
        back_button_y: layout.side3.y - button_size[1],
    };
    state.match_state.match_presentation.in_game_options_anchor = Some(anchor);
    let atlas = crate::app::presentation::sidebar_render::current_sidebar_chrome(state)
        .expect("active side art");
    let mut instances = background_instances(atlas, layout, width, height);
    let options = &state.match_state.match_presentation.in_game_options;
    let desc = build_in_game_options_descriptor();
    for child in layout_pass_in_game_options(&desc, width, height, anchor) {
        if matches!(child.id, control::BACK | control::KEYBOARD | control::SOUND) {
            let frame = usize::from(options.buttons.is_pressed(child.id));
            if let Some(entry) =
                atlas.in_game_shell.buttons[frame].or(atlas.in_game_shell.buttons[0])
            {
                push_art(
                    &mut instances,
                    entry,
                    child.rect,
                    RectPx::new(0, 0, width, height),
                );
            }
        }
    }
    let control_atlas = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .expect("active control art");
    let controls = in_game_options::build_in_game_options_instances(
        &control_atlas.control_chrome(),
        width,
        height,
        anchor,
        options,
    );
    let texts = in_game_options::build_in_game_options_text_instances(
        &state.renderer.bit_font,
        state.process_assets.csf.as_ref(),
        width,
        height,
        anchor,
        options,
        crate::app::presentation::sidebar_render::current_sidebar_theme(state),
    );
    render_in_game_shell_frame(
        state,
        encoder,
        destination,
        InGameShellFrame {
            art: instances,
            controls,
            texts,
            label: "Active Game Controls BBB",
        },
    )
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
    pub controls: Vec<SpriteInstance>,
    pub texts: Vec<crate::render::shell_text::ShellTextDraw>,
}

/// Shared shell compositor. Only the parent art atlas differs between launcher
/// and active-game children; popups paint after all ordinary controls. The
/// launcher paints at the window's size. An active-game shell paints at
/// [`in_game_shell_size`] into the shell surface, which a second pass copies
/// over the match frame at the Sidebar size, as the sidebar's surface is;
/// the cursor goes on top at the frame's own size, as over the battlefield.
/// A zoomed match then scales the frame up.
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
    let background_buffer = batch.create_instance_buffer(gpu, &instances);
    let control_buffer = batch.create_instance_buffer(gpu, &controls);
    let text_buffers: Vec<_> = texts
        .iter()
        .map(|d| batch.create_instance_buffer(gpu, &d.instances))
        .collect();
    let overlay_buffer = batch.create_instance_buffer(gpu, &overlay.controls);
    let overlay_text_buffers: Vec<_> = overlay
        .texts
        .iter()
        .map(|d| batch.create_instance_buffer(gpu, &d.instances))
        .collect();
    let cursor: Vec<_> = match cursor_policy {
        ShellFrameCursor::Visible => {
            crate::app::frontend::shell_pass::software_cursor(state, super::SHELL_CURSOR_DEPTH)
                .map(|(_, instance)| instance)
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
    if let (Some((buffer, count)), Some(background_texture)) =
        (&background_buffer, background_texture)
    {
        batch.draw_with_buffer_passthrough(&mut pass, background_texture, buffer, *count);
    }
    if let (Some((buffer, count)), Some(control_atlas)) = (&control_buffer, control_atlas) {
        batch.draw_with_buffer_passthrough(&mut pass, &control_atlas.texture, buffer, *count);
    }
    for (draw, buffer) in texts.iter().zip(&text_buffers) {
        if let Some((buffer, count)) = buffer {
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
    if let (Some((buffer, count)), Some(atlas)) = (&overlay_buffer, control_atlas) {
        batch.draw_with_buffer_passthrough(&mut pass, &atlas.texture, buffer, *count);
    }
    for (draw, buffer) in overlay.texts.iter().zip(&overlay_text_buffers) {
        if let Some((buffer, count)) = buffer {
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
    if let (Some((buffer, count)), Some(texture)) = (&cursor_buffer, cursor_texture) {
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
    })
}
