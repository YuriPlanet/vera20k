//! Native in-game menu B5/BBA/FF, sharing72F540 composition with Game
//! Controls.

use super::chrome::{push_ownerdraw_two_pixel_bevel_frame_px, push_solid_rect_px};
use super::controls::{ControlPaint, group_box_geometry, paint_control, push_group_or_plain_frame};
use super::in_game_shell::{self, InGameShellFrame};
use super::{SHELL_CONTROL_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB};
use crate::app::AppState;
use crate::assets::csf_file::CsfFile;
use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::shell_text::{self, ShellAlign, ShellTextDraw, TextRect};
use crate::ui::main_menu_dialogs::options::{
    FULL_SCREEN_LABEL, ORIGINAL_SHAPE_LABEL, SIDEBAR_LABEL, ZOOM_LABEL,
};
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::geom::ShellScale;
use crate::ui::shell::pause_menu::{
    DISPLAY_GROUP_CAPTION, DisplayList, InternetSlider, PAUSE_MENU_FOOTER, PauseMenuButton,
    PauseMenuButtonState, PauseMenuLayout, PauseMenuTemplate,
};
use crate::ui::shell::trackbar::value_text_rect;
use crate::ui::sidebar::SidebarTheme;

/// Type2 chooses released0 / pressed1 / timer-highlight2 (612EE8..612F5B).
/// Disabled buttons retain their SHP frame;612F5F changes their text color only.
pub(super) fn button_frame(state: PauseMenuButtonState) -> usize {
    crate::ui::shell::button::owner_button_frame(state.pressed, state.highlighted)
}

pub(super) fn button_text_rgb(theme: SidebarTheme, enabled: bool) -> [f32; 3] {
    if enabled {
        return SHELL_LABEL_TEXT_RGB;
    }
    // Original612F5F..613136 uses active scenario side and the initialized colors
    // 72A8E0/72A900/72A920 (registered815454/58/5C). Its RGB565 encode/decode
    // truncates [0,82,117] to[0,80,112] for Allied and [72,0,0] for Soviet/Yuri.
    let rgb = match theme {
        SidebarTheme::Allied => [0, 80, 112],
        SidebarTheme::Soviet | SidebarTheme::Yuri => [72, 0, 0],
    };
    rgb.map(|value| value as f32 / 255.0)
}

fn caption(csf: Option<&CsfFile>, key: &str) -> String {
    if let Some(csf) = csf {
        return csf.text(key).into_owned();
    }
    match key {
        "GUI:GameControls" => "Game Controls",
        "GUI:LoadGame" => "Load Game",
        "GUI:SaveGame" => "Save Game",
        "GUI:DeleteGame" => "Delete Game",
        "GUI:AbortMission" => "Abort Mission",
        "GUI:ResumeMission" => "Resume Mission",
        "GUI:GameOptions" => "Game Options",
        "GUI:OptionsInternet" => "Internet Options",
        "GUI:InternetGameControls" => "Internet Game Controls",
        _ => "",
    }
    .to_string()
}

fn text_draw(
    font: &BitFont,
    text: &str,
    rect: RectPx,
    rgb: [f32; 3],
    align: ShellAlign,
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
        [0.0; 2],
        SHELL_CONTROL_TEXT_DEPTH,
    )
}

pub(super) fn button_text_rect(rect: RectPx, pressed: bool) -> RectPx {
    // Common native type2 text tail61358D..6135EE: fixed right/bottom, and
    // the pressed state moves left/top by2/4 within that same rectangle.
    let dx = if pressed { 2 } else { 0 };
    let dy = if pressed { 4 } else { 0 };
    RectPx::new(
        rect.x + dx,
        rect.y + 1 + dy,
        rect.w - 2 - dx,
        rect.h - 1 - dy,
    )
}

fn text_draws(
    font: &BitFont,
    csf: Option<&CsfFile>,
    theme: SidebarTheme,
    layout: &PauseMenuLayout,
    states: [PauseMenuButtonState; 7],
    help: Option<&str>,
    shared_pause_caption: Option<&str>,
) -> Vec<ShellTextDraw> {
    let mut draws = Vec::new();
    for (button, rect) in layout.buttons() {
        let state = states[button as usize];
        let label = if button == PauseMenuButton::SharedPause {
            shared_pause_caption.unwrap_or_default().to_owned()
        } else if button == PauseMenuButton::Resume && shared_pause_caption.is_some() {
            "Return to Game".to_owned()
        } else {
            let control = layout.template.control(button).expect("template button");
            caption(csf, control.csf_key)
        };
        draws.push(text_draw(
            font,
            &label,
            button_text_rect(rect, state.pressed),
            button_text_rgb(theme, state.enabled),
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
        ));
    }
    let title = caption(csf, layout.template.title().csf_key);
    if !title.is_empty() {
        draws.push(text_draw(
            font,
            &title,
            layout.title,
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::H_CENTER,
        ));
    }
    // The common handler shows the hovered control's 6040B0 help in 0x695.
    let footer = caption(csf, help.unwrap_or(PAUSE_MENU_FOOTER.csf_key));
    if !footer.is_empty() {
        draws.push(text_draw(
            font,
            &footer,
            layout.footer,
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::NONE,
        ));
    }
    draws
}

/// The app supplies every button state, including Load/Delete eligibility.
/// This renderer owns no button actions and cannot silently substitute a
/// no-op route.
pub(crate) fn render_pause_menu_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
    buttons: [PauseMenuButtonState; 7],
) -> anyhow::Result<()> {
    let in_game_shell::InGameShellGeometry {
        width,
        height,
        layout: shell,
        ..
    } = in_game_shell::current_in_game_shell_layout(state).expect("active shell geometry");
    let layout = crate::app::input::pause_menu::current_layout(state).expect("active shell");
    crate::app::input::pause_menu::sync_display_controls(state);
    let atlas = crate::app::presentation::sidebar_render::current_sidebar_chrome(state)
        .expect("active side art");
    let mut art = in_game_shell::background_instances(atlas, shell, width, height);
    for (button, rect) in layout.buttons() {
        let frame = button_frame(buttons[button as usize]);
        if let Some(entry) = atlas.in_game_shell.buttons[frame].or(atlas.in_game_shell.buttons[0]) {
            in_game_shell::push_art(&mut art, entry, rect, RectPx::new(0, 0, width, height));
        }
    }
    let help = crate::app::input::pause_menu::hovered_help(state);
    let shared_pause_caption =
        crate::app::input::pause_menu::multiplayer_control(state).map(|control| control.caption);
    let font = &state.renderer.bit_font;
    let csf = state.process_assets.csf.as_ref();
    let mut texts = text_draws(
        font,
        csf,
        crate::app::presentation::sidebar_render::current_sidebar_theme(state),
        &layout,
        buttons,
        help,
        shared_pause_caption,
    );
    let mut controls = Vec::new();
    let chrome_atlas = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .expect("active menu controls");
    let chrome = chrome_atlas.control_chrome();
    let cell = font.cell_height() as i32;
    let group_box = |controls: &mut Vec<SpriteInstance>,
                     texts: &mut Vec<ShellTextDraw>,
                     rect: RectPx,
                     text: &str| {
        let (frame, caption_rect) = group_box_geometry(rect, cell, ShellScale::RETAIL);
        push_group_or_plain_frame(
            controls,
            chrome_atlas.white_pixel,
            frame,
            Some(font.text_width(text) as i32),
            false,
            ShellScale::RETAIL,
        );
        texts.push(text_draw(
            font,
            text,
            caption_rect,
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::NONE,
        ));
    };
    // VERA's Display group: the main menu's Zoom and Sidebar lists, Full
    // Screen and Original 4:3 Shape.
    let display_layout = layout.display;
    let display = &state.match_state.match_presentation.display_pause;
    group_box(
        &mut controls,
        &mut texts,
        display_layout.group,
        DISPLAY_GROUP_CAPTION,
    );
    for (list, caption_text) in [
        (DisplayList::Zoom, ZOOM_LABEL),
        (DisplayList::Sidebar, SIDEBAR_LABEL),
    ] {
        let face = display_layout.face(list);
        paint_control(
            &mut controls,
            &chrome,
            ControlPaint::Combo {
                rect: face,
                swatch: None,
                open: display.open == Some(list),
                disabled: false,
            },
            ShellScale::RETAIL,
        );
        texts.push(text_draw(
            font,
            caption_text,
            display_layout.captions[list as usize],
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::H_RIGHT | ShellAlign::V_CENTER,
        ));
        // The face's text clears the arrow, as on the main menu's Options page.
        texts.push(text_draw(
            font,
            display.row_label(list, display.selected(list)),
            RectPx::new(face.x + 3, face.y, face.w - 23, face.h),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::V_CENTER,
        ));
    }
    for (rect, checked, caption_text) in [
        (
            display_layout.full_screen,
            display.full_screen,
            FULL_SCREEN_LABEL,
        ),
        (
            display_layout.original_shape,
            display.original_shape,
            ORIGINAL_SHAPE_LABEL,
        ),
    ] {
        paint_control(
            &mut controls,
            &chrome,
            ControlPaint::Checkbox { checked, rect },
            ShellScale::RETAIL,
        );
        // 61663A..616674: the caption starts 26 pixels into the box's window.
        texts.push(text_draw(
            font,
            caption_text,
            RectPx::new(rect.x + 26, rect.y, rect.w - 26, rect.h),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::V_CENTER,
        ));
    }
    // The open list paints after every control and text, in the popup layer.
    let mut overlay = in_game_shell::ShellFrameOverlay::default();
    if let Some(open) = display.open {
        let rows = display.rows(open);
        let list = layout.display_list(open, rows);
        push_solid_rect_px(
            &mut overlay.controls,
            chrome_atlas.white_pixel,
            list,
            super::SHELL_DROPDOWN_BG_RGB_PENDING_COMBODROPWIN_SOURCE_CAPTURE,
            super::SHELL_DROPDOWN_DEPTH,
        );
        let row_h = crate::ui::skirmish_shell::COMBO_DROPDOWN_ROW_H;
        for row in 0..rows {
            let rect = RectPx::new(list.x, list.y + row as i32 * row_h, list.w, row_h);
            if display.hovered.unwrap_or(display.selected(open)) == row {
                push_solid_rect_px(
                    &mut overlay.controls,
                    chrome_atlas.white_pixel,
                    rect,
                    super::OWNERDRAW_SELECTED_RGB_FROM_DAT_00AC4604_PACKED_000000FF,
                    super::SHELL_DROPDOWN_DEPTH,
                );
            }
            overlay.texts.push(text_draw(
                font,
                display.row_label(open, row),
                RectPx::new(rect.x + 3, rect.y, rect.w - 6, rect.h),
                SHELL_LABEL_TEXT_RGB,
                ShellAlign::V_CENTER,
            ));
        }
        push_ownerdraw_two_pixel_bevel_frame_px(
            &mut overlay.controls,
            chrome_atlas.white_pixel,
            list,
            super::SHELL_DROPDOWN_DEPTH,
        );
    }
    if let Some(internet) = layout.internet {
        // FF's group box, both plaque trackbars and their statics (60B7A0).
        let group_caption = caption(csf, "GUI:InternetGameControls");
        group_box(&mut controls, &mut texts, internet.group, &group_caption);
        let pause = &state.match_state.match_presentation.internet_pause;
        for slider in InternetSlider::ALL {
            let index = slider as usize;
            let rect = internet.sliders[index];
            let value = pause.positions[index];
            paint_control(
                &mut controls,
                &chrome,
                ControlPaint::Trackbar {
                    rect,
                    thumb_left: slider
                        .range()
                        .thumb_left_in(value, rect, ShellScale::RETAIL),
                    plaque: true,
                    disabled: false,
                },
                ShellScale::RETAIL,
            );
            texts.push(text_draw(
                font,
                &value.to_string(),
                value_text_rect(rect, ShellScale::RETAIL),
                SHELL_LABEL_TEXT_RGB,
                ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            ));
            let (key, fallback) = slider.caption();
            let left = csf.map_or_else(|| fallback.to_string(), |csf| csf.text(key).into_owned());
            texts.push(text_draw(
                font,
                &left,
                internet.captions[index],
                SHELL_LABEL_TEXT_RGB,
                ShellAlign::H_RIGHT,
            ));
            let (key, fallback) = pause.value_caption(slider);
            let right = csf.map_or_else(|| fallback.to_string(), |csf| csf.text(key).into_owned());
            texts.push(text_draw(
                font,
                &right,
                internet.values[index],
                SHELL_LABEL_TEXT_RGB,
                ShellAlign::NONE,
            ));
        }
    }
    let label = match layout.template {
        PauseMenuTemplate::Offline => "Active Pause Menu B5",
        PauseMenuTemplate::Network => "Active Pause Menu BBA",
        PauseMenuTemplate::Internet => "Active Pause Menu FF",
    };
    in_game_shell::render_shell_frame(
        state,
        encoder,
        destination,
        InGameShellFrame {
            art,
            controls,
            texts,
            label,
        },
        in_game_shell::ShellFrameArt::Sidebar,
        overlay,
        in_game_shell::ShellFrameCursor::Visible,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_type2_changes_side_specific_text_without_dimming_art() {
        for theme in [
            SidebarTheme::Allied,
            SidebarTheme::Soviet,
            SidebarTheme::Yuri,
        ] {
            assert_eq!(button_text_rgb(theme, true), SHELL_LABEL_TEXT_RGB);
        }
        assert_eq!(
            button_text_rgb(SidebarTheme::Allied, false),
            [0.0, 80.0 / 255.0, 112.0 / 255.0]
        );
        assert_eq!(
            button_text_rgb(SidebarTheme::Soviet, false),
            [72.0 / 255.0, 0.0, 0.0]
        );
        assert_eq!(
            button_text_rgb(SidebarTheme::Yuri, false),
            button_text_rgb(SidebarTheme::Soviet, false)
        );
        let released = PauseMenuButtonState {
            enabled: false,
            ..Default::default()
        };
        assert_eq!(button_frame(released), 0);
        assert_eq!(
            button_frame(PauseMenuButtonState {
                highlighted: true,
                ..released
            }),
            2
        );
        assert_eq!(
            button_frame(PauseMenuButtonState {
                pressed: true,
                highlighted: true,
                ..released
            }),
            1
        );
    }

    #[test]
    fn pressed_caption_preserves_native_right_and_bottom_clip_edges() {
        let rect = RectPx::new(653, 227, 125, 25);
        assert_eq!(
            button_text_rect(rect, false),
            RectPx::new(653, 228, 123, 24)
        );
        assert_eq!(button_text_rect(rect, true), RectPx::new(655, 232, 121, 20));
    }
}
