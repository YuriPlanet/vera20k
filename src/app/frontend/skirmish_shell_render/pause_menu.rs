//! Native offline pause B5, sharing72F540 composition with Game Controls.

use super::in_game_shell::{self, InGameShellFrame};
use super::{SHELL_CONTROL_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB};
use crate::app::AppState;
use crate::assets::csf_file::CsfFile;
use crate::render::bit_font::BitFont;
use crate::render::shell_text::{self, ShellAlign, ShellTextDraw, TextRect};
use crate::sidebar::SidebarTheme;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::pause_menu::{
    PAUSE_MENU_FOOTER, PAUSE_MENU_TITLE, PauseMenuButton, PauseMenuButtonState, PauseMenuLayout,
    pause_menu_layout,
};

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
    layout: PauseMenuLayout,
    states: [PauseMenuButtonState; 6],
) -> Vec<ShellTextDraw> {
    let mut draws = Vec::new();
    for button in PauseMenuButton::ALL {
        let state = states[button as usize];
        draws.push(text_draw(
            font,
            &caption(csf, button.descriptor().csf_key),
            button_text_rect(layout.buttons[button as usize], state.pressed),
            button_text_rgb(theme, state.enabled),
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
        ));
    }
    for (control, rect, align) in [
        (PAUSE_MENU_TITLE, layout.title, ShellAlign::H_CENTER),
        (PAUSE_MENU_FOOTER, layout.footer, ShellAlign::NONE),
    ] {
        let text = caption(csf, control.csf_key);
        if !text.is_empty() {
            draws.push(text_draw(font, &text, rect, SHELL_LABEL_TEXT_RGB, align));
        }
    }
    draws
}

/// The app supplies all six states, including Load/Delete eligibility. This
/// renderer owns no button actions and cannot silently substitute a no-op route.
pub(crate) fn render_pause_menu_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
    buttons: [PauseMenuButtonState; 6],
) -> anyhow::Result<()> {
    let width = state.renderer.gpu.config.width as i32;
    let height = state.renderer.gpu.config.height as i32;
    let (shell, button_size) =
        in_game_shell::current_in_game_shell_layout(state).expect("active shell geometry");
    let layout = pause_menu_layout(width, height, shell, button_size);
    let atlas = crate::app::presentation::sidebar_render::current_sidebar_chrome(state)
        .expect("active side art");
    let mut art = in_game_shell::background_instances(atlas, shell, width, height);
    for button in PauseMenuButton::ALL {
        let frame = button_frame(buttons[button as usize]);
        if let Some(entry) = atlas.in_game_shell.buttons[frame].or(atlas.in_game_shell.buttons[0]) {
            in_game_shell::push_art(
                &mut art,
                entry,
                layout.buttons[button as usize],
                RectPx::new(0, 0, width, height),
            );
        }
    }
    let texts = text_draws(
        &state.renderer.bit_font,
        state.process_assets.csf.as_ref(),
        crate::app::presentation::sidebar_render::current_sidebar_theme(state),
        layout,
        buttons,
    );
    in_game_shell::render_in_game_shell_frame(
        state,
        encoder,
        destination,
        InGameShellFrame {
            art,
            controls: Vec::new(),
            texts,
            label: "Active Pause Menu B5",
        },
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
