//! Keyboard A3: shared launcher/active frame and native control composition.
use super::chrome::*;
use super::controls::{ControlPaint, paint_control};
use super::in_game_shell::{self, InGameShellFrame, ShellFrameArt, ShellFrameOverlay};
use super::text::{localized_label, rect_to_text_rect};
use super::{
    OWNERDRAW_SELECTED_RGB_FROM_DAT_00AC4604_PACKED_000000FF, SHELL_CONTROL_DEPTH,
    SHELL_CONTROL_TEXT_DEPTH, SHELL_DROPDOWN_BG_RGB_PENDING_COMBODROPWIN_SOURCE_CAPTURE,
    SHELL_DROPDOWN_DEPTH, SHELL_DROPDOWN_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB,
};
use crate::app::AppState;
use crate::app::input::{hotkeys::catalog::registered_commands, keyboard::key_name};
use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::shell_text_reveal::PathAReveal;
use crate::render::skirmish_shell_chrome::SkirmishShellChromeAtlas;
use crate::render::{
    shell_paint,
    shell_text::{self, ShellAlign, ShellTextDraw},
};
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::keyboard::{KEYBOARD_PAGE, KeyboardButton, KeyboardLayout, KeyboardParent};
use crate::ui::shell::list::ShellListGeometry;
use crate::ui::shell::static_reveal::Kind1PaintWindow;
use std::time::Instant;

fn text(font: &BitFont, value: &str, rect: RectPx, align: ShellAlign, depth: f32) -> ShellTextDraw {
    shell_text::draw_in_rect(
        font,
        value,
        rect_to_text_rect(rect),
        SHELL_LABEL_TEXT_RGB,
        align,
        [0.0; 2],
        depth,
    )
}

/// Original61E700 group and6208F0 plain frames both use two bevel rings and
/// averaged mixed corners. A group's top edge leaves the native caption gap.
fn frame_segments(
    rect: RectPx,
    caption_width: Option<i32>,
    outer_light: bool,
) -> Vec<(RectPx, [u8; 3])> {
    let light = [0xc5, 0xbe, 0xa7];
    let dark = [0x80, 0x7a, 0x68];
    let mixed = [0xa2, 0x9c, 0x87];
    let mut out = Vec::new();
    for ring in 0..2 {
        let l = rect.x + ring;
        let t = rect.y + ring;
        let r = rect.x + rect.w - ring - 1;
        let b = rect.y + rect.h - ring - 1;
        let (tl, br) = if outer_light == (ring == 0) {
            (light, dark)
        } else {
            (dark, light)
        };
        if let Some(width) = caption_width.filter(|width| *width > 0) {
            out.push((RectPx::new(l, t, rect.x + 8 - l + 1, 1), tl));
            let start = rect.x + width + 12;
            out.push((RectPx::new(start, t, (r - start + 1).max(0), 1), tl));
        } else {
            out.push((RectPx::new(l, t, r - l, 1), tl));
        }
        out.push((RectPx::new(l, t + 1, 1, b - t), tl));
        out.push((RectPx::new(l, b, r - l + 1, 1), br));
        out.push((RectPx::new(r, t + 1, 1, (b - t - 1).max(0)), br));
        out.push((RectPx::new(r, t, 1, 1), mixed));
        out.push((RectPx::new(l, b, 1, 1), mixed));
    }
    out
}

fn push_frame(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    rect: RectPx,
    caption_width: Option<i32>,
    outer_light: bool,
) {
    for (index, (rect, rgb)) in frame_segments(rect, caption_width, outer_light)
        .into_iter()
        .enumerate()
    {
        push_solid_rect(
            out,
            atlas,
            rect,
            rgb.map(|c| f32::from(c) / 255.0),
            SHELL_CONTROL_DEPTH - index as f32 * 0.0000001,
        );
    }
}

pub(crate) fn render_keyboard_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    use crate::app::frontend::shell_transition::ShellSlideKind;
    let width = state.renderer.gpu.config.width as i32;
    let height = state.renderer.gpu.config.height as i32;
    let title_text = localized_label(state, KEYBOARD_PAGE.title_key, "Keyboard Options");
    let now = Instant::now();
    let active = state
        .frontend
        .keyboard_dialog
        .as_ref()
        .expect("active A3 state")
        .parent
        == KeyboardParent::GameControls;
    // The front-end page slides like its family: the teardown slide repaints
    // the dialog and pumps no messages until it ends, so every child stays
    // blank; the entry slide suppresses the heading, status line and Back
    // (`0x00606800`) while the left-side controls paint.
    let exit_wave = (!active)
        .then(|| {
            crate::app::frontend::shell_transition::shell_exit_wave(state, ShellSlideKind::Keyboard)
                .cloned()
        })
        .flatten();
    let leaving = exit_wave.is_some();
    let wave = exit_wave.or_else(|| {
        (!active)
            .then(|| state.frontend.shell_first_paint_slide.clone())
            .flatten()
    });
    let sliding = wave.is_some();
    let (title, receipt) = if active {
        let dialog = state
            .frontend
            .keyboard_dialog
            .as_mut()
            .expect("active A3 state");
        dialog.title.start(&title_text, now);
        dialog.title.poll_timer(now);
        match dialog.title.paint_window() {
            Kind1PaintWindow::Hidden => (None, None),
            Kind1PaintWindow::Retained(window) => (Some(window), None),
            Kind1PaintWindow::Due { window, receipt } => (Some(window), Some(receipt)),
        }
    } else {
        // Heading and status line are the family statics: their reveals
        // start when the entry slide ends.
        let title = (!sliding)
            .then(|| state.frontend.shell_page_title.paint(now))
            .flatten();
        (title, None)
    };
    let status_label = if active || sliding {
        None
    } else {
        let help = state
            .frontend
            .keyboard_dialog
            .as_ref()
            .and_then(|dialog| dialog.hovered)
            .map(|control| localized_label(state, control.help_key(), ""))
            .unwrap_or_default();
        let window = KeyboardLayout::new(width, height, None).footer;
        crate::app::frontend::menu_page_render::paint_shell_status_line(state, help, window)
    };
    let shell = active
        .then(|| in_game_shell::current_in_game_shell_layout(state).expect("active A3 frame"));
    let layout = KeyboardLayout::new(width, height, shell);
    let dialog = state
        .frontend
        .keyboard_dialog
        .as_ref()
        .expect("active A3 state");
    let atlas = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .expect("A3 control art");
    let mut art = if let Some((shell, _)) = shell {
        let sidebar = crate::app::presentation::sidebar_render::current_sidebar_chrome(state)
            .expect("active A3 side art");
        let mut art = in_game_shell::background_instances(sidebar, shell, width, height);
        let frame = crate::ui::shell::button::owner_button_frame(
            dialog.buttons.is_pressed(KeyboardButton::Back),
            false,
        );
        if let Some(entry) =
            sidebar.in_game_shell.buttons[frame].or(sidebar.in_game_shell.buttons[0])
        {
            in_game_shell::push_art(
                &mut art,
                entry,
                layout.back,
                RectPx::new(0, 0, width, height),
            );
        }
        art
    } else {
        super::launcher_options::launcher_background_instances(atlas, width as u32, height as u32)
    };
    if let Some(wave) = wave.as_ref() {
        push_slide_column(
            &mut art,
            atlas,
            &super::compute_layout(width as u32, height as u32),
            &wave.button_draws(),
            SHELL_CONTROL_DEPTH,
        );
    } else if !active {
        push_right_panel_button_shp(
            &mut art,
            atlas,
            layout.back,
            dialog.buttons.is_pressed(KeyboardButton::Back),
            false,
            SHELL_CONTROL_DEPTH,
        );
    }
    if leaving {
        in_game_shell::render_shell_frame(
            state,
            encoder,
            destination,
            InGameShellFrame {
                art,
                controls: Vec::new(),
                texts: Vec::new(),
                label: "Keyboard A3",
            },
            ShellFrameArt::Launcher,
            ShellFrameOverlay::default(),
        )?;
        state.platform.window.request_redraw();
        return Ok(());
    }
    let mut controls = Vec::new();
    paint_control(
        &mut controls,
        &atlas.control_chrome(),
        ControlPaint::Combo {
            rect: layout.category,
            swatch: None,
            open: dialog.category_open,
            disabled: false,
        },
    );
    let geometry = ShellListGeometry::new(layout.commands, dialog.rows.len(), dialog.top);
    super::list::paint_list(
        &mut controls,
        atlas,
        geometry,
        dialog.top,
        dialog.selected,
        dialog.scroll.pressed_part(),
        false,
    );
    let font = &state.renderer.bit_font;
    let group_caption = localized_label(state, "GUI:Description", "Description");
    let group_top = font.cell_height() as i32 / 2;
    push_frame(
        &mut controls,
        atlas,
        RectPx::new(
            layout.group.x,
            layout.group.y + group_top,
            layout.group.w,
            layout.group.h - group_top,
        ),
        Some(font.text_width(&group_caption) as i32),
        false,
    );
    //61ED86..88 adds one to the window extents before6208F0 expands border2.
    push_frame(
        &mut controls,
        atlas,
        RectPx::new(
            layout.capture.x - 2,
            layout.capture.y - 2,
            layout.capture.w + 5,
            layout.capture.h + 5,
        ),
        None,
        true,
    );
    let buttons =
        [KeyboardButton::Assign, KeyboardButton::ResetAll].map(|id| shell_paint::ModalButton {
            rect: layout.button(id),
            pressed: dialog.buttons.is_pressed(id),
            enabled: true,
        });
    controls.extend(shell_paint::paint_modal_sprites(
        None,
        type3_button_frames(atlas),
        RectPx::new(0, 0, width, height),
        &buttons,
        shell_paint::ModalDepths {
            background: SHELL_CONTROL_DEPTH,
            button: SHELL_CONTROL_DEPTH,
            text: SHELL_CONTROL_TEXT_DEPTH,
        },
    ));
    let mut texts = Vec::new();
    if let Some(window) = title {
        texts.push(shell_text::draw_in_rect_path_a(
            font,
            &title_text,
            rect_to_text_rect(layout.title),
            ShellAlign::H_CENTER,
            [0.0; 2],
            SHELL_CONTROL_TEXT_DEPTH,
            PathAReveal {
                count: window.count,
                range: window.range,
                base_rgb: [255, 255, 0],
                highlight_rgb: [255; 3],
            },
        ));
    }
    for (key, rect, centered) in layout.labels {
        texts.push(text(
            font,
            &localized_label(state, key, key),
            rect,
            if centered {
                ShellAlign::H_CENTER
            } else {
                ShellAlign::NONE
            },
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    }
    let captions: &[_] = if sliding {
        &[
            (KeyboardButton::Assign, "GUI:Assign", "Assign"),
            (KeyboardButton::ResetAll, "GUI:ResetAll", "Reset All"),
        ]
    } else {
        &[
            (KeyboardButton::Back, "GUI:Back", "Back"),
            (KeyboardButton::Assign, "GUI:Assign", "Assign"),
            (KeyboardButton::ResetAll, "GUI:ResetAll", "Reset All"),
        ]
    };
    for &(id, key, fallback) in captions {
        let rect =
            super::pause_menu::button_text_rect(layout.button(id), dialog.buttons.is_pressed(id));
        texts.push(text(
            font,
            &localized_label(state, key, fallback),
            rect,
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    }
    texts.push(text(
        font,
        &group_caption,
        RectPx::new(
            layout.group.x + 10,
            layout.group.y,
            layout.group.w - 10,
            font.cell_height() as i32,
        ),
        ShellAlign::NONE,
        SHELL_CONTROL_TEXT_DEPTH,
    ));
    if let Some(category) = dialog.categories.get(dialog.category) {
        texts.push(text(
            font,
            category,
            crate::ui::skirmish_shell::combo_text_rect(layout.category),
            ShellAlign::V_CENTER,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    }
    for (visible, row) in dialog
        .rows
        .iter()
        .skip(dialog.top)
        .take(geometry.visible_rows)
        .enumerate()
    {
        let rect = geometry.row(visible);
        texts.push(text(
            font,
            &dialog.commands[*row].name,
            RectPx::new(rect.x + 2, rect.y, rect.w - 2, rect.h),
            ShellAlign::NONE,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    }
    let catalog = registered_commands();
    let bindings = &state.match_state.input.hotkey_bindings;
    if let Some(selected) = dialog.selected_command() {
        texts.push(text(
            font,
            &selected.description,
            layout.description,
            ShellAlign::NONE,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
        if let Some(metadata) = catalog.get(selected.command_index) {
            let current = bindings
                .first_key(metadata.command)
                .map(key_name)
                .unwrap_or_default();
            texts.push(text(
                font,
                &current,
                layout.current_shortcut,
                ShellAlign::NONE,
                SHELL_CONTROL_TEXT_DEPTH,
            ));
        }
    }
    if let Some(owner) = bindings
        .command_at(dialog.captured)
        .filter(|_| dialog.current_owner_visible)
    {
        if let Some(row) = dialog.commands.iter().find(|row| {
            catalog
                .get(row.command_index)
                .is_some_and(|metadata| metadata.command == owner)
        }) {
            texts.push(text(
                font,
                &row.name,
                layout.current_owner,
                ShellAlign::NONE,
                SHELL_CONTROL_TEXT_DEPTH,
            ));
        }
    }
    //620F60 ignores the supplied0x24 flags; it paints at the inset origin.
    texts.push(text(
        font,
        &key_name(dialog.captured),
        RectPx::new(
            layout.capture.x + 4,
            layout.capture.y + 4,
            layout.capture.w - 8,
            layout.capture.h - 8,
        ),
        ShellAlign::NONE,
        SHELL_CONTROL_TEXT_DEPTH,
    ));
    if let Some(key) = dialog.error {
        texts.push(text(
            font,
            &localized_label(state, key, key),
            layout.error,
            ShellAlign::NONE,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    }
    if let Some(hovered) = dialog.hovered.filter(|_| active) {
        texts.push(text(
            font,
            &localized_label(state, hovered.help_key(), ""),
            layout.footer,
            ShellAlign::NONE,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    }
    texts.extend(shell_paint::paint_labels(
        font,
        &status_label.into_iter().collect::<Vec<_>>(),
    ));
    let mut overlay = ShellFrameOverlay::default();
    if dialog.category_open {
        let popup = layout.category_popup(dialog.categories.len());
        push_solid_rect(
            &mut overlay.controls,
            atlas,
            popup,
            SHELL_DROPDOWN_BG_RGB_PENDING_COMBODROPWIN_SOURCE_CAPTURE,
            SHELL_DROPDOWN_DEPTH,
        );
        for (i, category) in dialog.categories.iter().enumerate() {
            let row = RectPx::new(popup.x + 2, popup.y + 2 + i as i32 * 23, popup.w - 4, 23);
            if dialog.category_hovered.or(Some(dialog.category)) == Some(i) {
                push_solid_rect(
                    &mut overlay.controls,
                    atlas,
                    row,
                    OWNERDRAW_SELECTED_RGB_FROM_DAT_00AC4604_PACKED_000000FF,
                    SHELL_DROPDOWN_DEPTH - 0.00001,
                );
            }
            overlay.texts.push(text(
                font,
                category,
                RectPx::new(row.x + 3, row.y, row.w - 6, row.h),
                ShellAlign::V_CENTER,
                SHELL_DROPDOWN_TEXT_DEPTH,
            ));
        }
        push_ownerdraw_two_pixel_bevel_frame(
            &mut overlay.controls,
            atlas,
            popup,
            SHELL_DROPDOWN_DEPTH - 0.00002,
        );
    }
    in_game_shell::render_shell_frame(
        state,
        encoder,
        destination,
        InGameShellFrame {
            art,
            controls,
            texts,
            label: "Keyboard A3",
        },
        if active {
            ShellFrameArt::Sidebar
        } else {
            ShellFrameArt::Launcher
        },
        overlay,
    )?;
    if let Some(dialog) = state.frontend.keyboard_dialog.as_mut() {
        dialog.title_receipt = receipt;
    }
    state.platform.window.request_redraw();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_group_caption_gap_and_bevel_polarity_are_preserved() {
        //61E700:207px group at95,159;17px font moves its top edge down8.
        let segments = frame_segments(RectPx::new(95, 167, 207, 146), Some(66), false);
        assert_eq!(
            segments[0],
            (RectPx::new(95, 167, 9, 1), [0x80, 0x7a, 0x68])
        );
        assert_eq!(
            segments[1],
            (RectPx::new(173, 167, 129, 1), [0x80, 0x7a, 0x68])
        );
        assert_eq!(
            segments[7],
            (RectPx::new(96, 168, 8, 1), [0xc5, 0xbe, 0xa7])
        );
        assert!(segments.contains(&(RectPx::new(301, 167, 1, 1), [0xa2, 0x9c, 0x87])));
    }

    #[test]
    fn hotkey_frame_includes_native_extra_pixel_before_border_expansion() {
        //61ED86..88:186x23 window becomes187x24 before two outside rings.
        let segments = frame_segments(RectPx::new(94, 396, 191, 28), None, true);
        assert_eq!(
            segments[0],
            (RectPx::new(94, 396, 190, 1), [0xc5, 0xbe, 0xa7])
        );
        assert!(segments.contains(&(RectPx::new(284, 396, 1, 1), [0xa2, 0x9c, 0x87])));
        assert!(segments.contains(&(RectPx::new(94, 423, 1, 1), [0xa2, 0x9c, 0x87])));
    }
}
