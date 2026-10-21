//! The connection dialog 0xEA over the active-game shell (WM_PAINT
//! 0x00621E90, then the per-node bars 0x0064A7B0): Quit Game and the status
//! line in the right-panel places, the row buttons, bars, countdown and list
//! as ordinary children.

use super::chrome::{push_rect_outline, push_solid_rect};
use super::in_game_shell::{self, InGameShellFrame};
use super::pause_menu::{button_frame, button_text_rect};
use super::text::{localized_label, push_text_draw, rect_to_text_rect};
use super::{SHELL_CONTROL_DEPTH, SHELL_CONTROL_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB};
use crate::app::AppState;
use crate::render::{shell_paint, shell_text::ShellAlign};
use crate::ui::shell::connection::{
    BAR_OUTLINE_RGB, ConnectionButton, QUIT_LABEL, bar_paint, connection_layout,
};
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::list::{ListMetrics, ShellListGeometry};
use crate::ui::shell::pause_menu::PauseMenuButtonState;

fn rgb(unit: [u8; 3]) -> [f32; 3] {
    unit.map(|channel| f32::from(channel) / 255.0)
}

/// 0xEA can be drawn: it needs the same art as every active-game page.
pub(crate) fn connection_shell_drawable(state: &AppState) -> bool {
    state.frontend.screen == crate::app::GameScreen::InGame
        && state.match_state.network_wait.dialog_open()
        && state.frontend.skirmish_shell_chrome.is_some()
        && crate::app::presentation::sidebar_render::current_sidebar_chrome(state)
            .is_some_and(|atlas| atlas.in_game_shell.panel_tile.is_some())
        && in_game_shell::current_in_game_shell_layout(state).is_some()
}

pub(crate) fn render_connection_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let in_game_shell::InGameShellGeometry {
        width,
        height,
        layout: shell,
        button_size,
    } = in_game_shell::current_in_game_shell_layout(state).expect("active 0xEA geometry");
    let layout = connection_layout(width, height, shell, button_size);
    let page = state
        .match_state
        .network_wait
        .page()
        .expect("open 0xEA")
        .clone();
    let nodes = crate::app::match_runtime::network_match::nodes(&state.match_state.session);
    let screen = RectPx::new(0, 0, width, height);
    let atlas = crate::app::presentation::sidebar_render::current_sidebar_chrome(state)
        .expect("active 0xEA art");
    let mut art = in_game_shell::background_instances(atlas, shell, width, height);
    let pressed = page.buttons.is_pressed(ConnectionButton::Quit);
    let frame = button_frame(PauseMenuButtonState {
        pressed,
        ..Default::default()
    });
    if let Some(entry) = atlas.in_game_shell.buttons[frame].or(atlas.in_game_shell.buttons[0]) {
        in_game_shell::push_art(&mut art, entry, layout.quit, screen);
    }
    let chrome = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .expect("active 0xEA controls");
    let mut controls = Vec::new();
    // 0x00609FC7..0x00609FE2 / 0x0060A330: the row buttons are type3, MNBTTN.
    let rows = page.rows.len();
    let buttons: Vec<_> = (0..rows)
        .map(|row| shell_paint::ModalButton {
            rect: layout.players[row],
            pressed: page.buttons.is_pressed(ConnectionButton::Player(row)),
            enabled: true,
        })
        .collect();
    controls.extend(shell_paint::paint_modal_sprites(
        None,
        super::chrome::type3_button_frames(chrome),
        screen,
        &buttons,
        ShellScale::RETAIL,
        shell_paint::ModalDepths {
            background: SHELL_CONTROL_DEPTH,
            button: SHELL_CONTROL_DEPTH,
            text: SHELL_CONTROL_TEXT_DEPTH,
        },
    ));
    // 0x0064A7B0 draws one bar per live node, row 0 (local) with no lag.
    for (row, source) in nodes.iter().enumerate().take(rows) {
        let (lag, age) = if row == 0 {
            (0, 0)
        } else {
            state.match_state.network_wait.bar_timing(*source)
        };
        let bar = bar_paint(layout.bars[row], lag, age);
        push_rect_outline(
            &mut controls,
            chrome,
            bar.outline,
            rgb(BAR_OUTLINE_RGB),
            SHELL_CONTROL_DEPTH,
        );
        push_solid_rect(
            &mut controls,
            chrome,
            bar.fill,
            rgb(bar.fill_rgb),
            SHELL_CONTROL_DEPTH - 0.00001,
        );
    }
    let geometry = ShellListGeometry::new(
        layout.list,
        page.lines.len(),
        page.list.top_index,
        ListMetrics::RETAIL,
    );
    super::list::paint_list(
        &mut controls,
        chrome,
        geometry,
        page.list.top_index,
        None,
        None,
        false,
        ShellScale::RETAIL,
    );

    let mut texts = Vec::new();
    let mut text = |value: &str, rect: RectPx, align: ShellAlign| {
        push_text_draw(
            &mut texts,
            state,
            value,
            rect_to_text_rect(rect),
            SHELL_LABEL_TEXT_RGB,
            align,
            SHELL_CONTROL_TEXT_DEPTH,
            1.0,
        )
    };
    text(
        &localized_label(state, QUIT_LABEL.0, QUIT_LABEL.1),
        button_text_rect(layout.quit, pressed),
        ShellAlign::H_CENTER | ShellAlign::V_CENTER,
    );
    for (row, name) in page.rows.iter().enumerate() {
        text(
            name,
            button_text_rect(
                layout.players[row],
                page.buttons.is_pressed(ConnectionButton::Player(row)),
            ),
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
        );
    }
    let countdown = page
        .countdown
        .clone()
        .unwrap_or_else(|| localized_label(state, "GUI:TimeRemaining", "Time Remaining:"));
    text(&countdown, layout.countdown, ShellAlign::NONE);
    for (visible, line) in page
        .lines
        .iter()
        .skip(page.list.top_index)
        .take(geometry.visible_rows)
        .enumerate()
    {
        let r = geometry.row(visible);
        text(
            &String::from_utf16_lossy(&line.text),
            RectPx::new(r.x + 2, r.y, r.w - 4, r.h),
            ShellAlign::V_CENTER,
        );
    }
    if let Some(control) = page.hovered {
        text(
            &localized_label(state, control.help_key(), ""),
            layout.status_line,
            ShellAlign::NONE,
        );
    }
    in_game_shell::render_in_game_shell_frame(
        state,
        encoder,
        destination,
        InGameShellFrame {
            art,
            controls,
            texts,
            label: "Active Connection Shell EA",
        },
    )
}
