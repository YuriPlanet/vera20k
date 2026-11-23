//! Native in-game menu B5/BBA/FF, sharing72F540 composition with Game
//! Controls.

use super::SHELL_LABEL_TEXT_RGB;
use super::gui_paint::{ButtonArt, FrameArt, GuiStyle};
use super::in_game_shell::{self, InGamePaint};
use super::launcher_options;
use crate::app::AppState;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::pause_menu::{
    FOOTER, INTERNET_CAPTIONS, INTERNET_GROUP, INTERNET_GROUP_LABEL, INTERNET_VALUES,
    InternetSlider, KEYBOARD_LABEL, PAUSE_MENU_FOOTER, PauseMenuButton, PauseMenuButtonState,
    PauseMenuTemplate, TITLE,
};
use crate::ui::sidebar::SidebarTheme;

/// Type2 chooses released0 / pressed1 / timer-highlight2 (612EE8..612F5B).
/// Disabled buttons retain their SHP frame;612F5F changes their text color only.
pub(super) fn button_frame(state: PauseMenuButtonState) -> usize {
    vera20k_gui::shell::button::owner_button_frame(state.pressed, state.highlighted)
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

/// The app supplies every button state, including Load/Delete eligibility.
/// This renderer owns no button actions and cannot silently substitute a
/// no-op route.
///
/// The menu's buttons, title, status line and FF's group box, trackbars and
/// statics paint from its `vera20k-gui` nodes ([`super::gui_paint`]), made
/// current first every frame. The Options page's column buttons are its
/// own dialog's nodes, painted as the menu's on the side's art.
pub(crate) fn render_pause_menu_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
    buttons: [PauseMenuButtonState; 8],
) -> anyhow::Result<()> {
    let (layout, page_layout) =
        crate::app::input::pause_menu::sync(state, &buttons).expect("active shell");
    let mut overlay = in_game_shell::ShellFrameOverlay::default();
    let template = layout.template;
    let label = match template {
        PauseMenuTemplate::Offline => "Active Pause Menu B5",
        PauseMenuTemplate::Network => "Active Pause Menu BBA",
        PauseMenuTemplate::Internet => "Active Pause Menu FF",
    };
    let frame = {
        let mut paint = InGamePaint::new(state);
        let state = paint.state;
        let ui = &state.match_state.match_presentation.dialog_ui;
        let csf = state.process_assets.csf.as_ref();
        let pause = &state.match_state.match_presentation.internet_pause;
        let shared_pause = crate::app::input::pause_menu::multiplayer_control(state)
            .map(|control| control.caption);
        let slot = |ids: [u16; 2], id: u16| -> Option<InternetSlider> {
            let index = ids.iter().position(|of| *of == id)?;
            Some(InternetSlider::ALL[index])
        };
        let label = |(key, fallback): (&str, &str)| state.csf_label(key, fallback);
        let text_of = |id: i32| -> Option<String> {
            let id = u16::try_from(id).ok()?;
            if let Some(button) = template.button(id) {
                return Some(match button {
                    PauseMenuButton::SharedPause => {
                        shared_pause.map(crate::i18n::tr).unwrap_or_default()
                    }
                    PauseMenuButton::Resume if shared_pause.is_some() => {
                        crate::i18n::tr("VERA:ReturnToGame")
                    }
                    PauseMenuButton::Keyboard => label(KEYBOARD_LABEL),
                    _ => label(template.control(button)?.caption()),
                });
            }
            if let Some(slider) = slot(INTERNET_CAPTIONS, id) {
                let (key, fallback) = slider.caption();
                return Some(
                    csf.map_or_else(|| fallback.to_string(), |csf| csf.text(key).into_owned()),
                );
            }
            if let Some(slider) = slot(INTERNET_VALUES, id) {
                let (key, fallback) = pause.value_caption(slider);
                return Some(crate::i18n::label(csf, key, fallback));
            }
            match id {
                TITLE => Some(label(template.title().caption())),
                // The common handler shows the hovered control's 6040B0 help
                // in 0x695.
                FOOTER => Some(match crate::app::input::pause_menu::hovered_help(state) {
                    Some(key) => label((key, "")),
                    None => label(PAUSE_MENU_FOOTER.caption()),
                }),
                INTERNET_GROUP => Some(label(INTERNET_GROUP_LABEL)),
                // The trackbars show their position.
                _ => None,
            }
        };
        let frame_of = |id: i32| (id == i32::from(INTERNET_GROUP)).then_some(FrameArt::Group);
        let style = GuiStyle {
            buttons: ButtonArt::Side,
            side: Some(paint.side_buttons()),
            text_of: Some(&text_of),
            frame_of: Some(&frame_of),
            ..Default::default()
        };
        // The template's buttons in their sidebar order, VERA's after them;
        // one without a rect is hidden and paints nothing.
        paint.nodes(
            ui,
            &PauseMenuButton::ALL.map(|button| template.id(button)),
            style,
        );
        paint.nodes(ui, &[TITLE, FOOTER], style);
        let page = state.match_state.match_presentation.options_page.as_ref();
        if let (Some(page_layout), Some(page)) = (&page_layout, page) {
            // The page's sidebar buttons, Options or the Options column, are the
            // menu's own buttons, on this side's art; the page that shows is
            // lit. Its own controls paint only with the control art
            // (switching YR Original Style failed to rebuild it); the menu's
            // own buttons still work. The open list paints after every
            // control and text.
            let column = launcher_options::ColumnArt::Side(paint.side_buttons());
            let widgets =
                launcher_options::paint_page(state, &page.dialog, page_layout.scale, column, &[]);
            paint.art.extend(widgets.side);
            paint.controls.extend(widgets.controls);
            paint.texts.extend(widgets.texts);
            (overlay.controls, overlay.texts) = (widgets.popup, widgets.popup_texts);
        }
        // FF's group box, then each plaque trackbar with its caption and
        // value static (60B7A0), where its page shows them and the control
        // art is there.
        if state.frontend.skirmish_shell_chrome.is_some() {
            let [connection, speed] = InternetSlider::ALL.map(InternetSlider::id);
            let internet = [
                INTERNET_GROUP,
                connection,
                INTERNET_CAPTIONS[0],
                INTERNET_VALUES[0],
                speed,
                INTERNET_CAPTIONS[1],
                INTERNET_VALUES[1],
            ];
            paint.nodes(ui, &internet, style);
        }
        paint.finish(label)
    };
    in_game_shell::render_shell_frame(
        state,
        encoder,
        destination,
        frame,
        in_game_shell::ShellFrameArt::Sidebar,
        overlay,
        in_game_shell::ShellFrameCursor::Visible,
    )
}

#[cfg(all(test, tests_app_frontend))]
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
