//! Retail launcher Options (D5): physical shell paint over the retained model.
//! Active owner55FC80, resourceD5, common static6153E0 and trackbar61D950.
//! At the main menu D5 is a family page: its heading, status line and
//! monitor are the shared family statics, and its column slides in and out
//! (`0x006071E0`).

use super::{chrome::*, controls::*, *};
use crate::app::AppState;
use crate::render::batch::SpriteInstance;
use crate::render::shell_text::{self, ShellAlign, ShellTextDraw, TextRect};
use crate::render::shell_text_reveal::PathAReveal;
use crate::ui::main_menu_dialogs::options::shell::{
    LauncherLabelAlign, LauncherOptionsLayout, LauncherStatusHelp,
};
use crate::ui::main_menu_dialogs::options::{
    LauncherCheckboxId, LauncherComboId, LauncherTrackbarId, OptionsDialogState, Zoom,
};
use crate::ui::shell::geom::{RectPx, ShellScale};
use std::time::Instant;

/// A Zoom row: the zoom and the battle's size in game pixels in this window,
/// what `App::sync_zoom_pass` will give the match.
fn zoom_label(zoom: Zoom, (width, height): (u32, u32), original_shape: bool) -> String {
    use crate::render::upscale_pass::{FrameShape, ZoomGeometry};
    let shape = if original_shape {
        FrameShape::Original
    } else {
        FrameShape::Window
    };
    let geometry = ZoomGeometry::new(width, height, zoom.percent(width, height), shape);
    let size = format!("({}x{})", geometry.src_width, geometry.src_height);
    match zoom {
        Zoom::Auto => format!("Auto {size}"),
        Zoom::Percent(percent) => format!("{percent}% {size}"),
    }
}

fn text_draw(
    font: &BitFont,
    text: &str,
    rect: RectPx,
    align: ShellAlign,
    rgb: [f32; 3],
    glyph_scale: f32,
) -> ShellTextDraw {
    shell_text::draw_in_rect_scaled(
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
        glyph_scale,
    )
}

/// The left-side controls, and the right-panel buttons unless a slide draws
/// the column in their place.
fn build_controls(
    atlas: &SkirmishShellChromeAtlas,
    layout: &LauncherOptionsLayout,
    dialog: &OptionsDialogState,
    buttons: bool,
) -> Vec<SpriteInstance> {
    let mut out = Vec::new();
    let chrome = atlas.control_chrome();
    let scale = layout.scale;
    for &(id, rect) in &layout.trackbars {
        paint_control(
            &mut out,
            &chrome,
            ControlPaint::Trackbar {
                rect,
                thumb_left: dialog.shell_trackbar_thumb_left(id, rect, scale),
                plaque: id.plaque_reserve() > 0,
            },
            scale,
        );
    }
    for &(id, rect) in &layout.checkboxes {
        paint_control(
            &mut out,
            &chrome,
            ControlPaint::Checkbox {
                rect,
                checked: dialog.shell_checkbox_checked(id),
            },
            scale,
        );
    }
    let open = dialog.shell_popup(layout).map(|popup| popup.combo);
    for &(combo, rect) in &layout.combos {
        paint_control(
            &mut out,
            &chrome,
            ControlPaint::Combo {
                rect,
                swatch: None,
                open: open == Some(combo),
                disabled: false,
            },
            scale,
        );
    }
    for &(id, rect) in layout.buttons.iter().filter(|_| buttons) {
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
    buttons: bool,
) -> Vec<ShellTextDraw> {
    let mut out = Vec::new();
    let scale = layout.scale;
    let glyph = scale.factor();
    for label in dialog.shell_labels(layout) {
        let align = match label.align {
            LauncherLabelAlign::Left => ShellAlign::NONE,
            LauncherLabelAlign::Center => ShellAlign::H_CENTER,
            LauncherLabelAlign::Right => ShellAlign::H_RIGHT,
        };
        if label.title {
            if let Some(window) = title {
                out.push(shell_text::draw_in_rect_path_a_scaled(
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
                    glyph,
                ));
            }
        } else {
            out.push(text_draw(
                font,
                label.text,
                label.rect,
                align,
                SHELL_LABEL_TEXT_RGB,
                glyph,
            ));
        }
    }
    for &(id, rect) in &layout.checkboxes {
        // The box's icon column keeps its authored 26-pixel reserve.
        let text = scale.derive_rect(rect, |rect| {
            RectPx::new(rect.x + 26, rect.y, rect.w - 26, rect.h)
        });
        out.push(text_draw(
            font,
            dialog.shell_checkbox_label(id),
            text,
            ShellAlign::V_CENTER,
            SHELL_LABEL_TEXT_RGB,
            glyph,
        ));
    }
    for &(id, rect) in &layout.trackbars {
        if matches!(
            id,
            LauncherTrackbarId::Score | LauncherTrackbarId::Sound | LauncherTrackbarId::Voice
        ) {
            out.push(text_draw(
                font,
                &dialog.trackbar_position(id).to_string(),
                crate::ui::shell::trackbar::value_text_rect(rect, scale),
                ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                if dialog.launcher_audio_available() {
                    SHELL_LABEL_TEXT_RGB
                } else {
                    SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F
                },
                glyph,
            ));
        }
    }
    let face_text = |rect| {
        scale.derive_rect(rect, |rect| {
            RectPx::new(rect.x + 3, rect.y, rect.w - 23, rect.h)
        })
    };
    for &(combo, rect) in &layout.combos {
        let text = match combo {
            LauncherComboId::Zoom => zoom_label(
                dialog.shell_selected_zoom(layout),
                layout.screen(),
                dialog.shell_checkbox_checked(LauncherCheckboxId::OriginalShape),
            ),
            LauncherComboId::Sidebar => dialog.shell_selected_sidebar(layout).label().to_owned(),
            LauncherComboId::Display => dialog.shell_display_label(),
        };
        out.push(text_draw(
            font,
            &text,
            face_text(rect),
            ShellAlign::V_CENTER,
            SHELL_LABEL_TEXT_RGB,
            glyph,
        ));
    }
    for &(id, rect) in layout.buttons.iter().filter(|_| buttons) {
        let pressed = dialog.shell_button_pressed(id);
        // 612B70 type1: ordinary press retains AC18A4 foreground; the text
        // rectangle changes at61358D..6135CD, independently of SHP geometry.
        let native_text = button_text_rect(rect, pressed, scale);
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
            glyph,
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
    scale: ShellScale,
) -> Vec<SpriteInstance> {
    let common = crate::ui::skirmish_shell::compute_layout_on(scale, width, height);
    let mut out = Vec::new();
    let background = if common.authored_width() == 640 {
        atlas.generic_background_640_mnscrns_shell
    } else {
        atlas.generic_background_large_mnscrnl_shell
    };
    if let Some(background) = background {
        if scale.is_retail() {
            push_entry_native(&mut out, background, 0, 0, SHELL_PARENT_BACKGROUND_DEPTH);
        } else {
            // A scaled page grows the MNSCRN canvas with its dialog, as the
            // family menu pages do.
            let dialog = scale.dialog_rect();
            push_entry_grown(
                &mut out,
                background,
                dialog.x,
                dialog.y,
                scale,
                SHELL_PARENT_BACKGROUND_DEPTH,
            );
        }
    }
    push_right_panel_base_instances(&mut out, atlas, &common, false);
    push_lower_strip_instance(&mut out, atlas, &common);
    out
}

pub(crate) fn render_launcher_options(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    use crate::app::frontend::shell_transition::ShellSlideKind;
    let now = Instant::now();
    let page = state
        .frontend
        .options_dialog
        .as_ref()
        .expect("launcher active")
        .page();
    let layout = crate::app::frontend::shell_layout::launcher_options_layout(state, page);
    let scale = layout.scale;
    // The teardown slide starts with a full dialog repaint (`0x00622C4F`) and
    // pumps no messages until it ends: every child stays blank. The entry
    // slide suppresses the heading, status line, monitor and column buttons
    // (`0x00606800`); the left-side controls paint.
    let exit_wave = crate::app::frontend::shell_transition::shell_exit_wave(
        state,
        ShellSlideKind::options_page(page),
    )
    .cloned();
    let leaving = exit_wave.is_some();
    let wave = exit_wave.or_else(|| state.frontend.shell_first_paint_slide.clone());
    let sliding = wave.is_some();
    // Like the sibling family pages, the monitor shows its current frame
    // with no timer while the entry slide runs.
    let frames = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .map_or(0, |atlas| atlas.launcher_warning_frames.len());
    let monitor_frame = if leaving {
        None
    } else {
        state.frontend.shell_monitor.paint(now, frames, !sliding)
    };
    let (title, status_label) = if sliding {
        (None, None)
    } else {
        let title = state.frontend.shell_page_title.paint(now);
        let status_text = state
            .frontend
            .options_dialog
            .as_ref()
            .and_then(|dialog| dialog.shell_status_help())
            .map(|help| match help {
                LauncherStatusHelp::Csf(key) => {
                    crate::app::frontend::shell_pass::resolve_csf(state, key).into_owned()
                }
                LauncherStatusHelp::Text(text) => text.to_owned(),
            })
            .unwrap_or_default();
        let status = crate::app::frontend::menu_page_render::paint_shell_status_line(
            state,
            status_text,
            layout.status_help,
        );
        (title, status)
    };
    let dialog = state
        .frontend
        .options_dialog
        .as_ref()
        .expect("launcher active");
    let atlas = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .expect("launcher atlas");
    let mut instances =
        launcher_background_instances(atlas, state.render_width(), state.render_height(), scale);
    if let Some(frame) = monitor_frame.and_then(|frame| atlas.launcher_warning_frames.get(frame)) {
        push_flag_entry_clipped_centered(
            &mut instances,
            *frame,
            layout.warning,
            scale,
            SHELL_CONTROL_DEPTH,
        );
    }
    let mut texts = Vec::new();
    if let Some(wave) = wave.as_ref() {
        let common = crate::ui::skirmish_shell::compute_layout_on(
            scale,
            state.render_width(),
            state.render_height(),
        );
        push_slide_column(
            &mut instances,
            atlas,
            &common,
            &wave.button_draws(),
            SHELL_CONTROL_DEPTH,
        );
    }
    if !leaving {
        instances.extend(build_controls(atlas, &layout, dialog, !sliding));
        texts = build_text(&state.renderer.bit_font, &layout, dialog, title, !sliding);
    }
    texts.extend(crate::render::shell_paint::paint_labels_scaled(
        &state.renderer.bit_font,
        &status_label.into_iter().collect::<Vec<_>>(),
        scale.factor(),
    ));
    let mut popup_instances = Vec::new();
    let mut popup_text = Vec::new();
    if let Some(popup) = dialog.shell_popup(&layout).filter(|_| !leaving) {
        push_solid_rect_px(
            &mut popup_instances,
            atlas.white_pixel,
            popup.rect,
            SHELL_DROPDOWN_BG_RGB_PENDING_COMBODROPWIN_SOURCE_CAPTURE,
            SHELL_DROPDOWN_DEPTH,
        );
        let text_inset = (3.0 * scale.factor()).round() as i32;
        let original_shape = dialog.shell_checkbox_checked(LauncherCheckboxId::OriginalShape);
        let rows: Vec<String> = match popup.combo {
            LauncherComboId::Zoom => layout
                .zoom_choices()
                .into_iter()
                .map(|zoom| zoom_label(zoom, layout.screen(), original_shape))
                .collect(),
            LauncherComboId::Sidebar => layout
                .sidebar_choices()
                .into_iter()
                .map(|size| size.label().to_owned())
                .collect(),
            LauncherComboId::Display => dialog.shell_display_rows(),
        };
        for (i, row_text) in rows.iter().enumerate() {
            let rect = popup.row_rect(i);
            if popup.hovered.unwrap_or(popup.selected) == i {
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
                row_text,
                RectPx::new(rect.x + text_inset, rect.y, rect.w - 2 * text_inset, rect.h),
                ShellAlign::V_CENTER,
                SHELL_LABEL_TEXT_RGB,
                scale.factor(),
            ));
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::zoom_label;
    use crate::ui::main_menu_dialogs::options::Zoom;

    #[test]
    fn zoom_rows_name_the_battle_size() {
        let hd = (1920, 1080);
        let rows: Vec<_> = Zoom::choices(hd.0, hd.1)
            .into_iter()
            .map(|zoom| zoom_label(zoom, hd, false))
            .collect();
        assert_eq!(
            rows,
            [
                "Auto (1920x1080)",
                "100% (1920x1080)",
                "150% (1280x720)",
                "200% (960x540)"
            ]
        );
        // The original 4:3 shape trims the width.
        assert_eq!(zoom_label(Zoom::Percent(150), hd, true), "150% (960x720)");
    }
}
