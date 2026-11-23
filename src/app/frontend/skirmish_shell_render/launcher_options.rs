//! Retail launcher Options (D5): physical shell paint over the retained model.
//! Active owner55FC80, resourceD5, common static6153E0 and trackbar61D950.
//! At the main menu D5 is a family page: its heading, status line and
//! monitor are the shared family statics, and its column slides in and out
//! (`0x006071E0`). Every control paints from the dialog's tree.

use super::gui_paint::{ButtonArt, GuiPaint, GuiStyle, paint_gui_styled};
use super::{chrome::*, *};
use crate::app::AppState;
use crate::render::batch::SpriteInstance;
use crate::render::shell_text::{self, TextRect};
use crate::render::shell_text_reveal::PathAReveal;
use crate::ui::main_menu_dialogs::options::OptionsDialogState;
use crate::ui::main_menu_dialogs::options::shell::{Control, LauncherOptionsLayout};
use crate::ui::shell::geom::{RectPx, ShellAlign, ShellScale};
use crate::util::time::Instant;

/// The page's controls and its open list, painted from the dialog's tree
/// (synced to its layout by the caller). The page's own buttons are drawn as
/// Sound Options' Play and Stop are (609FC7..609FE2/60A330): owner-draw type
/// 3, MNBTTN centred on the control. With `column` the right-hand column's
/// buttons paint on the panel's art, the page that shows lit (612B70
/// record+C5, SDBTNANM 3); without, a slide or the in-game menu draws them.
/// Captions under `covered` are left out: a box draws all its art before its
/// text, so that text would otherwise show through the list.
pub(crate) fn paint_page(
    state: &AppState,
    dialog: &OptionsDialogState,
    scale: ShellScale,
    column: bool,
    covered: &[RectPx],
) -> GuiPaint {
    let (gui, panel) = dialog.shell_gui();
    let art = |id| match dialog.shell_control(id)? {
        Control::Button(button) if button.on_page() => Some(ButtonArt::ModalSqueezed),
        Control::Button(_) if column => Some(ButtonArt::Panel),
        _ => None,
    };
    let style = GuiStyle {
        art_of: Some(&art),
        ..Default::default()
    };
    paint_gui_styled(state, gui, panel, scale, covered, style)
}

/// Options' controls on another box's art: VERA's Esc menu shows a few of
/// them. They paint in submission order, after the box and its buttons.
pub(crate) fn paint_options_settings(
    draw: &mut crate::render::shell_paint::ModalDraw,
    state: &AppState,
    layout: &LauncherOptionsLayout,
    settings: &OptionsDialogState,
) {
    let list = settings.shell_popup_rect();
    let page = paint_page(state, settings, layout.scale, false, list.as_slice());
    draw.sprites.extend(page.controls);
    draw.text.extend(page.texts);
    draw.sprites.extend(page.popup);
    draw.text.extend(page.popup_texts);
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
    if let Some(background) = super::modals::generic_shell_background_entry(atlas, &common) {
        if scale.is_unscaled() {
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
    let category = state
        .frontend
        .options_dialog
        .as_ref()
        .expect("launcher active")
        .category();
    let layout = crate::app::frontend::shell_layout::launcher_options_layout(state, category);
    if let Some(dialog) = state.frontend.options_dialog.as_mut() {
        dialog.shell_sync(&layout);
    }
    let scale = layout.scale;
    // The teardown slide starts with a full dialog repaint (`0x00622C4F`) and
    // pumps no messages until it ends: every child stays blank. The entry
    // slide suppresses the heading, status line, monitor and column buttons
    // (`0x00606800`); the left-side controls paint.
    let exit_wave =
        crate::app::frontend::shell_transition::shell_exit_wave(state, ShellSlideKind::Options)
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
            .map(|key| state.csf_text(key))
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
    let font = &state.renderer.bit_font;
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
    if let Some(window) = title {
        texts.push(shell_text::draw_in_rect_path_a_scaled(
            font,
            dialog.shell_title_text(),
            TextRect {
                x: layout.title.x,
                y: layout.title.y,
                w: layout.title.w as u32,
                h: layout.title.h as u32,
            },
            ShellAlign::H_CENTER | ShellAlign::PANEL_FACE,
            [0.0, 0.0],
            SHELL_CONTROL_TEXT_DEPTH,
            PathAReveal {
                count: window.count,
                range: window.range,
                base_rgb: [255, 255, 0],
                highlight_rgb: [255, 255, 255],
            },
            scale.factor(),
        ));
    }
    let page = (!leaving).then(|| paint_page(state, dialog, scale, !sliding, &[]));
    let page = page.unwrap_or_default();
    instances.extend(page.controls);
    texts.extend(page.texts);
    texts.extend(crate::render::shell_paint::paint_labels_scaled(
        font,
        &status_label.into_iter().collect::<Vec<_>>(),
        scale.factor(),
    ));
    super::in_game_shell::render_shell_frame(
        state,
        encoder,
        destination,
        super::in_game_shell::InGameShellFrame {
            art: instances,
            controls: Vec::new(),
            texts,
            label: "Launcher Options D5",
        },
        super::in_game_shell::ShellFrameArt::Launcher,
        super::in_game_shell::ShellFrameOverlay {
            controls: page.popup,
            texts: page.popup_texts,
            ..Default::default()
        },
        super::in_game_shell::ShellFrameCursor::Visible,
    )?;
    state.platform.window.request_redraw();
    Ok(())
}

#[cfg(all(test, tests_app_frontend))]
mod tests {
    use crate::ui::main_menu_dialogs::options::{Zoom, zoom_label};

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
