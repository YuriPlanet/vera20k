//! The connection dialog 0xEA over the active-game shell (WM_PAINT
//! 0x00621E90, then the per-node bars 0x0064A7B0): Quit Game and the status
//! line in the right-panel places, the row buttons, bars, countdown and list
//! as ordinary children.

use super::chrome::{push_rect_outline, push_solid_rect};
use super::in_game_shell;
use super::pause_menu::button_text_rect;
use super::{SHELL_CONTROL_DEPTH, SHELL_CONTROL_TEXT_DEPTH};
use crate::app::AppState;
use crate::app::input::connection::DIALOG;
use crate::app::input::dialog_page::pressed;
use crate::render::shell_paint;
use crate::ui::shell::connection::{
    BAR_OUTLINE_RGB, CONNECTION_ROWS, ConnectionControl, bar_paint, connection_layout,
};
use crate::ui::shell::geom::{RectPx, ShellAlign};
use vera20k_gui::shell::list::{ListMetrics, ShellListGeometry};

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
    super::in_game_shell::paint_dialog(state, encoder, destination, &DIALOG, |paint, ui| {
        let state = paint.state;
        let layout = paint.shell.place(connection_layout);
        let page = state.match_state.network_wait.page().expect("open 0xEA");
        let visible = page.visible_range();
        let player_pressed =
            |row: usize| pressed(ui, ConnectionControl::Player(row - visible.start).id());
        let chrome = state
            .frontend
            .skirmish_shell_chrome
            .as_ref()
            .expect("active 0xEA controls");
        // 0x00609FC7..0x00609FE2 / 0x0060A330: the row buttons are type3, MNBTTN.
        let buttons: Vec<_> = visible
            .clone()
            .enumerate()
            .map(|(slot, row)| shell_paint::ModalButton {
                rect: layout.players[slot],
                pressed: player_pressed(row),
                enabled: true,
            })
            .collect();
        let depths = shell_paint::ModalDepths {
            background: SHELL_CONTROL_DEPTH,
            button: SHELL_CONTROL_DEPTH,
            text: SHELL_CONTROL_TEXT_DEPTH,
        };
        let frames = super::chrome::type3_button_frames(chrome);
        let screen = paint.screen();
        let controls = &mut paint.controls;
        controls.extend(shell_paint::paint_modal_sprites(
            None,
            frames,
            screen,
            &buttons,
            crate::ui::shell::geom::RETAIL_SCALE,
            depths,
        ));
        // 0x0064A7B0 draws one bar per live node, row 0 (local) with no lag.
        for (slot, row) in visible.clone().enumerate() {
            let Some(source) = page.row_source(row) else {
                continue;
            };
            let (lag, age) = if row == 0 {
                (0, 0)
            } else {
                state.match_state.network_wait.bar_timing(source)
            };
            let bar = bar_paint(layout.bars[slot], lag, age);
            push_rect_outline(
                controls,
                chrome,
                bar.outline,
                rgb(BAR_OUTLINE_RGB),
                SHELL_CONTROL_DEPTH,
            );
            push_solid_rect(
                controls,
                chrome,
                bar.fill,
                rgb(bar.fill_rgb),
                SHELL_CONTROL_DEPTH - 0.00001,
            );
        }
        let top = page.top;
        let geometry =
            ShellListGeometry::new(layout.list, page.lines.len(), top, ListMetrics::RETAIL);
        super::list::paint_list(
            controls,
            chrome,
            geometry,
            top,
            None,
            None,
            false,
            crate::ui::shell::geom::RETAIL_SCALE,
        );
        if page.page_count() > 1 {
            let held = [ConnectionControl::PreviousPage, ConnectionControl::NextPage]
                .map(|c| pressed(ui, c.id()));
            let shown = page.page.index(page.rows.len(), CONNECTION_ROWS) + 1;
            paint.paging(layout.paging, held, shown, page.page_count());
        }
        for (slot, row) in visible.clone().enumerate() {
            let at = button_text_rect(layout.players[slot], player_pressed(row));
            paint.text(
                &page.rows[row].name,
                at,
                ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            );
        }
        let countdown = match &page.countdown {
            Some(countdown) => countdown.clone(),
            None => paint.label(("GUI:TimeRemaining", "Time Remaining:")),
        };
        paint.text(&countdown, layout.countdown, ShellAlign::NONE);
        for (visible, line) in page
            .lines
            .iter()
            .skip(top)
            .take(geometry.visible_rows)
            .enumerate()
        {
            let r = geometry.row(visible);
            let at = RectPx::new(r.x + 2, r.y, r.w - 4, r.h);
            paint.text(
                &String::from_utf16_lossy(&line.text),
                at,
                ShellAlign::V_CENTER,
            );
        }
    })
}
