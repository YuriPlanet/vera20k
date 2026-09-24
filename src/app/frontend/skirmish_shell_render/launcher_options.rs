//! Retail launcher Options (D5): physical shell paint over the retained model.
//! Active owner55FC80, resourceD5, common static6153E0 and trackbar61D950.

use super::{chrome::*, controls::*, *};
use crate::app::AppState;
use crate::render::batch::SpriteInstance;
use crate::render::shell_text::{self, ShellAlign, ShellTextDraw, TextRect};
use crate::render::shell_text_reveal::PathAReveal;
use crate::ui::main_menu_dialogs::options::shell::{LauncherLabelAlign, LauncherOptionsLayout};
use crate::ui::main_menu_dialogs::options::{LauncherTrackbarId, OptionsDialogState};
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::static_reveal::{Kind1PaintWindow, Kind1RevealReceipt, Kind1StaticReveal};
use crate::ui::shell::warning_monitor::WarningMonitor;
use std::time::Instant;

#[derive(Default)]
pub(crate) struct LauncherOptionsPresentation {
    pub(crate) title: Kind1StaticReveal,
    warning: WarningMonitor,
}

pub(crate) struct LauncherPaintReceipt {
    title: Option<Kind1RevealReceipt>,
}

impl LauncherOptionsPresentation {
    pub(crate) fn record_presented(&mut self, receipt: LauncherPaintReceipt) -> bool {
        if receipt
            .title
            .is_some_and(|r| !self.title.record_presented(r))
        {
            return false;
        }
        self.warning.commit_presented();
        true
    }
}

fn text_draw(
    font: &BitFont,
    text: &str,
    rect: RectPx,
    align: ShellAlign,
    rgb: [f32; 3],
) -> ShellTextDraw {
    shell_text::draw_in_rect(
        font,
        text,
        TextRect {
            x: rect.x,
            y: rect.y,
            w: rect.w.max(0) as u32,
            h: rect.h.max(0) as u32,
        },
        rgb,
        align,
        [0.0, 0.0],
        SHELL_CONTROL_TEXT_DEPTH,
        None,
    )
}

fn build_controls(
    atlas: &SkirmishShellChromeAtlas,
    layout: &LauncherOptionsLayout,
    dialog: &OptionsDialogState,
) -> Vec<SpriteInstance> {
    let mut out = Vec::new();
    let chrome = atlas.control_chrome();
    for (id, rect) in layout.trackbars {
        let thumb_px = dialog.shell_trackbar_thumb_left(id, rect.w) - 1;
        let plain = matches!(
            id,
            LauncherTrackbarId::Detail
                | LauncherTrackbarId::Difficulty
                | LauncherTrackbarId::Scroll
        );
        paint_control(
            &mut out,
            &chrome,
            if plain {
                ControlPaint::PlainTrackbar { rect, thumb_px }
            } else {
                ControlPaint::Trackbar { rect, thumb_px }
            },
        );
    }
    for (id, rect) in layout.checkboxes {
        paint_control(
            &mut out,
            &chrome,
            ControlPaint::Checkbox {
                rect,
                checked: dialog.shell_checkbox_checked(id),
            },
        );
    }
    paint_control(
        &mut out,
        &chrome,
        ControlPaint::Combo {
            rect: layout.resolution,
            swatch: None,
            open: dialog.shell_popup(layout).is_some(),
            disabled: false,
        },
    );
    for (id, rect) in layout.buttons {
        push_right_panel_button_shp(
            &mut out,
            atlas,
            rect,
            dialog.shell_button_pressed(id),
            false,
            SHELL_CONTROL_DEPTH,
        );
    }
    out
}

fn build_text(
    font: &BitFont,
    layout: &LauncherOptionsLayout,
    dialog: &OptionsDialogState,
    title: Option<crate::ui::shell::static_reveal::Kind1RevealWindow>,
) -> Vec<ShellTextDraw> {
    let mut out = Vec::new();
    for label in dialog.shell_labels(layout) {
        let align = match label.align {
            LauncherLabelAlign::Left => ShellAlign::NONE,
            LauncherLabelAlign::Center => ShellAlign::H_CENTER,
            LauncherLabelAlign::Right => ShellAlign::H_RIGHT,
        };
        if label.title {
            if let Some(window) = title {
                out.push(shell_text::draw_in_rect_path_a(
                    font,
                    label.text,
                    TextRect {
                        x: label.rect.x,
                        y: label.rect.y,
                        w: label.rect.w as u32,
                        h: label.rect.h as u32,
                    },
                    align,
                    [0.0, 0.0],
                    SHELL_CONTROL_TEXT_DEPTH,
                    PathAReveal {
                        count: window.count,
                        range: window.range,
                        base_rgb: [255, 255, 0],
                        highlight_rgb: [255, 255, 255],
                    },
                ));
            }
        } else {
            out.push(text_draw(
                font,
                label.text,
                label.rect,
                align,
                SHELL_LABEL_TEXT_RGB,
            ));
        }
    }
    for (id, rect) in layout.checkboxes {
        out.push(text_draw(
            font,
            dialog.shell_checkbox_label(id),
            RectPx::new(rect.x + 26, rect.y, rect.w - 26, rect.h),
            ShellAlign::V_CENTER,
            SHELL_LABEL_TEXT_RGB,
        ));
    }
    for (id, rect) in layout.trackbars {
        if matches!(
            id,
            LauncherTrackbarId::Score | LauncherTrackbarId::Sound | LauncherTrackbarId::Voice
        ) {
            out.push(text_draw(
                font,
                &dialog.trackbar_position(id).to_string(),
                crate::ui::skirmish_shell::trackbar_value_text_rect(rect),
                ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                if dialog.launcher_audio_available() {
                    SHELL_LABEL_TEXT_RGB
                } else {
                    SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F
                },
            ));
        }
    }
    out.push(text_draw(
        font,
        dialog.shell_selected_resolution_text(),
        RectPx::new(
            layout.resolution.x + 3,
            layout.resolution.y,
            layout.resolution.w - 23,
            layout.resolution.h,
        ),
        ShellAlign::V_CENTER,
        SHELL_LABEL_TEXT_RGB,
    ));
    for (id, rect) in layout.buttons {
        let pressed = dialog.shell_button_pressed(id);
        // 612B70 type1: ordinary press retains AC18A4 foreground; the text
        // rectangle changes at61358D..6135CD, independently of SHP geometry.
        let native_text = button_text_rect(rect, pressed);
        out.push(text_draw(
            font,
            dialog.shell_button_label(id),
            RectPx::new(
                native_text.x,
                native_text.y,
                native_text.w as i32,
                native_text.h as i32,
            ),
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            button_label_color(),
        ));
    }
    out
}

/// Shared mode1 launcher frame,60CF00 default background and621E90 composition.
/// Resource-specific children (such as D5 warning71C) remain with their owner.
pub(super) fn launcher_background_instances(
    atlas: &SkirmishShellChromeAtlas,
    width: u32,
    height: u32,
) -> Vec<SpriteInstance> {
    let common = compute_layout(width, height);
    let mut out = Vec::new();
    let background = if width == 640 {
        atlas.generic_background_640_mnscrns_shell
    } else {
        atlas.generic_background_large_mnscrnl_shell
    };
    if let Some(background) = background {
        push_entry_native(&mut out, background, 0, 0, SHELL_PARENT_BACKGROUND_DEPTH);
    }
    push_right_panel_base_instances(&mut out, atlas, &common, 0, false);
    push_lower_strip_instance(&mut out, atlas, &common);
    out
}

pub(crate) fn render_launcher_options(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<LauncherPaintReceipt> {
    let now = Instant::now();
    let dialog = state
        .frontend
        .options_dialog
        .as_ref()
        .expect("launcher active");
    let presentation = &mut state.frontend.launcher_options_presentation;
    presentation.title.start(dialog.shell_title_text(), now);
    presentation.title.poll_timer(now);
    let (title, title_receipt) = match presentation.title.paint_window() {
        Kind1PaintWindow::Hidden => (None, None),
        Kind1PaintWindow::Retained(window) => (Some(window), None),
        Kind1PaintWindow::Due { window, receipt } => (Some(window), Some(receipt)),
    };
    let atlas = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .expect("launcher atlas");
    let warning = presentation
        .warning
        .paint(now, atlas.launcher_warning_frames.len(), true);
    let layout =
        LauncherOptionsLayout::new(state.render_width() as i32, state.render_height() as i32);
    let mut instances =
        launcher_background_instances(atlas, state.render_width(), state.render_height());
    if let Some(frame) = warning.and_then(|frame| atlas.launcher_warning_frames.get(frame)) {
        push_flag_entry_native_clipped_centered(
            &mut instances,
            *frame,
            layout.warning,
            SHELL_CONTROL_DEPTH,
        );
    }
    instances.extend(build_controls(atlas, &layout, dialog));
    let texts = build_text(&state.renderer.bit_font, &layout, dialog, title);
    let mut popup_instances = Vec::new();
    let mut popup_text = Vec::new();
    if let Some(popup) = dialog.shell_popup(&layout) {
        push_solid_rect_px(
            &mut popup_instances,
            atlas.white_pixel,
            popup.rect,
            SHELL_DROPDOWN_BG_RGB_PENDING_COMBODROPWIN_SOURCE_CAPTURE,
            SHELL_DROPDOWN_DEPTH,
        );
        for (i, row) in dialog
            .shell_resolution_rows()
            .iter()
            .enumerate()
            .skip(popup.first_row)
            .take(popup.visible_rows)
        {
            let rect = RectPx::new(
                popup.content.x,
                popup.content.y + (i - popup.first_row) as i32 * popup.row_height,
                popup.content.w,
                popup.row_height,
            );
            if popup.hovered.or(popup.selected) == Some(i) {
                push_solid_rect_px(
                    &mut popup_instances,
                    atlas.white_pixel,
                    rect,
                    OWNERDRAW_SELECTED_RGB_FROM_DAT_00AC4604_PACKED_000000FF,
                    SHELL_DROPDOWN_DEPTH,
                );
            }
            popup_text.push(text_draw(
                &state.renderer.bit_font,
                &row.label,
                RectPx::new(rect.x + 3, rect.y, rect.w - 6, rect.h),
                ShellAlign::V_CENTER,
                SHELL_LABEL_TEXT_RGB,
            ));
        }
        if let (Some(scrollbar), Some(thumb)) = (popup.scrollbar, popup.thumb) {
            paint_control(
                &mut popup_instances,
                &atlas.control_chrome(),
                ControlPaint::ScrollBar {
                    scrollbar,
                    thumb,
                    pressed_part: None,
                },
            );
        }
        push_ownerdraw_two_pixel_bevel_frame_px(
            &mut popup_instances,
            atlas.white_pixel,
            popup.rect,
            SHELL_DROPDOWN_DEPTH,
        );
    }
    state.renderer.batch_renderer.update_camera(
        &state.renderer.gpu,
        state.render_width() as f32,
        state.render_height() as f32,
        0.0,
        0.0,
        1.0,
        crate::render::batch::DepthAxis::NONE,
    );
    let batch = &state.renderer.batch_renderer;
    let gpu = &state.renderer.gpu;
    let chrome_buffer = batch.create_instance_buffer(gpu, &instances);
    let text_buffers: Vec<_> = texts
        .iter()
        .map(|d| batch.create_instance_buffer(gpu, &d.instances))
        .collect();
    let popup_buffer = batch.create_instance_buffer(gpu, &popup_instances);
    let popup_text_buffers: Vec<_> = popup_text
        .iter()
        .map(|d| batch.create_instance_buffer(gpu, &d.instances))
        .collect();
    let cursor: Vec<_> =
        crate::app::frontend::shell_pass::software_cursor(state, SHELL_CURSOR_DEPTH)
            .map(|(_, instance)| instance)
            .into_iter()
            .collect();
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
        label: Some("Launcher Options D5"),
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
    if let Some((buffer, count)) = &chrome_buffer {
        batch.draw_with_buffer_passthrough(&mut pass, &atlas.texture, buffer, *count);
    }
    for (draw, buffer) in texts.iter().zip(&text_buffers) {
        if let Some((buffer, count)) = buffer {
            if draw.scissor.w == 0 || draw.scissor.h == 0 {
                continue;
            }
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
    }
    pass.set_scissor_rect(0, 0, state.render_width(), state.render_height());
    if let Some((buffer, count)) = &popup_buffer {
        batch.draw_with_buffer_passthrough(&mut pass, &atlas.texture, buffer, *count);
    }
    for (draw, buffer) in popup_text.iter().zip(&popup_text_buffers) {
        if let Some((buffer, count)) = buffer {
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
    }
    pass.set_scissor_rect(0, 0, state.render_width(), state.render_height());
    if let (Some((buffer, count)), Some(texture)) = (&cursor_buffer, cursor_texture) {
        batch.draw_with_buffer_passthrough(&mut pass, texture, buffer, *count);
    }
    drop(pass);
    state
        .renderer
        .shell_surface_presenter
        .encode_present(encoder, destination);
    state.platform.window.request_redraw();
    Ok(LauncherPaintReceipt {
        title: title_receipt,
    })
}
