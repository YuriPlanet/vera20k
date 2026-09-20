//! Active-scenario shell composition: 621E90 -> 72F540, geometry 72FC60.
//! A complete physical-pixel RGB565 surface, independent of battlefield scaling.

use super::in_game_options;
use crate::app::AppState;
use crate::render::batch::SpriteInstance;
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
        && crate::app::presentation::sidebar_render::current_sidebar_chrome(state).is_some_and(
            |atlas| {
                atlas.in_game_shell.panel_tile.is_some()
                    && atlas.in_game_shell.buttons[0].is_some()
                    && shell_layout(
                        atlas,
                        state.renderer.gpu.config.width as i32,
                        state.renderer.gpu.config.height as i32,
                    )
                    .is_some()
            },
        )
}

pub(crate) fn current_in_game_shell_layout(
    state: &AppState,
) -> Option<(InGameShellLayout, [i32; 2])> {
    let atlas = crate::app::presentation::sidebar_render::current_sidebar_chrome(state)?;
    Some((
        shell_layout(
            atlas,
            state.renderer.gpu.config.width as i32,
            state.renderer.gpu.config.height as i32,
        )?,
        atlas.in_game_shell.buttons[0]?
            .pixel_size
            .map(|value| value as i32),
    ))
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
    let width = state.renderer.gpu.config.width;
    let height = state.renderer.gpu.config.height;
    let atlas = crate::app::presentation::sidebar_render::current_sidebar_chrome(state)
        .expect("active side art");
    let layout = shell_layout(atlas, width as i32, height as i32).expect("active shell geometry");
    let button_size = atlas.in_game_shell.buttons[0]
        .expect("active button art")
        .pixel_size
        .map(|v| v as i32);
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
    let mut instances = background_instances(atlas, layout, width as i32, height as i32);
    let options = &state.match_state.match_presentation.in_game_options;
    let desc = build_in_game_options_descriptor();
    for child in layout_pass_in_game_options(&desc, width as i32, height as i32, anchor) {
        if matches!(child.id, control::BACK | control::KEYBOARD | control::SOUND) {
            let frame = usize::from(options.buttons.is_pressed(child.id));
            if let Some(entry) =
                atlas.in_game_shell.buttons[frame].or(atlas.in_game_shell.buttons[0])
            {
                push_art(
                    &mut instances,
                    entry,
                    child.rect,
                    RectPx::new(0, 0, width as i32, height as i32),
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
        width as i32,
        height as i32,
        anchor,
        options,
    );
    let texts = in_game_options::build_in_game_options_text_instances(
        &state.renderer.bit_font,
        state.process_assets.csf.as_ref(),
        width as i32,
        height as i32,
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
    )
}

pub(super) enum ShellFrameArt {
    Sidebar,
    Launcher,
}

#[derive(Default)]
pub(super) struct ShellFrameOverlay {
    pub controls: Vec<SpriteInstance>,
    pub texts: Vec<crate::render::shell_text::ShellTextDraw>,
}

/// Shared physical shell compositor. Only the parent art atlas differs between
/// launcher and active-game children; popups paint after all ordinary controls.
pub(super) fn render_shell_frame(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
    frame: InGameShellFrame,
    art: ShellFrameArt,
    overlay: ShellFrameOverlay,
) -> anyhow::Result<()> {
    // Entry itself must select the cursor; no subsequent pointer event is
    // required to undo the legacy pause menu's OS-cursor visibility.
    state
        .platform
        .window
        .set_cursor_visible(!state.use_software_cursor());
    state
        .renderer
        .egui
        .discard_pending_input(&state.platform.window);
    let width = state.renderer.gpu.config.width;
    let height = state.renderer.gpu.config.height;
    let background_texture = match art {
        ShellFrameArt::Sidebar => {
            &crate::app::presentation::sidebar_render::current_sidebar_chrome(state)
                .expect("active side art")
                .texture
        }
        ShellFrameArt::Launcher => {
            &state
                .frontend
                .skirmish_shell_chrome
                .as_ref()
                .expect("launcher art")
                .texture
        }
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
    let mut cursor: Vec<_> = super::shell_cursor_instance(state).into_iter().collect();
    let (cursor_x, cursor_y) = state.window_cursor_position();
    for instance in &mut cursor {
        instance.position[0] += cursor_x - state.match_state.input.cursor_x;
        instance.position[1] += cursor_y - state.match_state.input.cursor_y;
    }
    let cursor_buffer = batch.create_instance_buffer(gpu, &cursor);
    let cursor_texture = state
        .match_state
        .match_presentation
        .software_cursor
        .as_ref()
        .and_then(|c| c.get(crate::app::types::CursorId::Default))
        .and_then(|s| s.frames.first())
        .map(|f| &f.texture);
    let color = state.renderer.shell_surface_presenter.source_render_view();
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &color,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
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
    if let Some((buffer, count)) = &background_buffer {
        batch.draw_with_buffer_passthrough(&mut pass, background_texture, buffer, *count);
    }
    if let (Some((buffer, count)), Some(control_atlas)) = (&control_buffer, control_atlas) {
        batch.draw_with_buffer_passthrough(&mut pass, &control_atlas.texture, buffer, *count);
    }
    for (draw, buffer) in texts.iter().zip(&text_buffers) {
        if let Some((buffer, count)) = buffer {
            let x = draw.scissor.x.min(width);
            let y = draw.scissor.y.min(height);
            let w = draw.scissor.w.min(width - x);
            let h = draw.scissor.h.min(height - y);
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
    pass.set_scissor_rect(0, 0, width, height);
    if let (Some((buffer, count)), Some(atlas)) = (&overlay_buffer, control_atlas) {
        batch.draw_with_buffer_passthrough(&mut pass, &atlas.texture, buffer, *count);
    }
    for (draw, buffer) in overlay.texts.iter().zip(&overlay_text_buffers) {
        if let Some((buffer, count)) = buffer {
            let x = draw.scissor.x.min(width);
            let y = draw.scissor.y.min(height);
            let w = draw.scissor.w.min(width - x);
            let h = draw.scissor.h.min(height - y);
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
    pass.set_scissor_rect(0, 0, width, height);
    if let (Some((buffer, count)), Some(texture)) = (&cursor_buffer, cursor_texture) {
        batch.draw_with_buffer_passthrough(&mut pass, texture, buffer, *count);
    }
    drop(pass);
    state
        .renderer
        .shell_surface_presenter
        .encode_present(encoder, destination);
    Ok(())
}
