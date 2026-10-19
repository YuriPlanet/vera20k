//! Diplomacy73 over the existing active-game shell and generic control art.
//! Native657DA0 hides chat/taunt controls in offline skirmish; alliance boxes
//! of computer Houses are disabled, while names/flags/kills remain visible.

use super::controls::{ControlPaint, paint_control};
use super::in_game_shell::{self, InGameShellFrame};
use super::text::{push_text_draw, rect_to_text_rect};
use super::{SHELL_CONTROL_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB};
use crate::app::AppState;
use crate::render::shell_text::{ShellAlign, ShellTextDraw};
use crate::ui::shell::geom::{RectPx, ShellScale};

pub(crate) fn render_diplomacy_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let shell =
        in_game_shell::current_in_game_shell_layout(state).expect("active diplomacy geometry");
    let layout = crate::app::input::diplomacy::layout(state).expect("active diplomacy layout");
    let dialog = state
        .match_state
        .match_presentation
        .diplomacy
        .as_ref()
        .expect("active diplomacy state");
    let pressed = dialog.buttons.is_pressed(());
    let rows = crate::app::input::diplomacy::rows(state);
    let top = dialog.top.min(
        rows.len()
            .saturating_sub(crate::ui::shell::diplomacy::VISIBLE_ROWS),
    );
    let side = crate::app::presentation::sidebar_render::current_sidebar_chrome(state)
        .expect("active side art");
    let mut art =
        in_game_shell::background_instances(side, shell.layout, shell.width, shell.height);
    if let Some(entry) =
        side.in_game_shell.buttons[usize::from(pressed)].or(side.in_game_shell.buttons[0])
    {
        in_game_shell::push_art(
            &mut art,
            entry,
            layout.back,
            RectPx::new(0, 0, shell.width, shell.height),
        );
    }
    let mut texts: Vec<ShellTextDraw> = Vec::new();
    let mut label = |text: &str, rect: RectPx, rgb: [f32; 3], align: ShellAlign| {
        push_text_draw(
            &mut texts,
            state,
            text,
            rect_to_text_rect(rect),
            rgb,
            align,
            SHELL_CONTROL_TEXT_DEPTH,
            1.0,
        );
    };
    let localize = |key| crate::app::frontend::shell_pass::resolve_csf(state, key);
    label(
        &localize("GUI:DiplomacyMenu"),
        layout.title,
        SHELL_LABEL_TEXT_RGB,
        ShellAlign::H_CENTER,
    );
    label(
        &localize("GUI:Return"),
        super::pause_menu::button_text_rect(layout.back, pressed),
        SHELL_LABEL_TEXT_RGB,
        ShellAlign::H_CENTER | ShellAlign::V_CENTER,
    );
    let show_allies = state
        .rules()
        .is_some_and(|rules| rules.general.allies_allowed);
    for (key, rect, visible) in [
        ("GUI:Player", layout.headers[0], true),
        ("GUI:Allies", layout.headers[1], show_allies),
        ("GUI:Kills", layout.headers[2], true),
    ] {
        if visible {
            label(&localize(key), rect, SHELL_LABEL_TEXT_RGB, ShellAlign::NONE);
        }
    }
    let chrome = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .expect("active control art");
    let mut controls = Vec::new();
    for (i, row) in rows
        .iter()
        .skip(top)
        .take(crate::ui::shell::diplomacy::VISIBLE_ROWS)
        .enumerate()
    {
        let rgb = row.rgb.map(|c| f32::from(c) / 255.0);
        label(&row.name, layout.players[i], rgb, ShellAlign::NONE);
        label(
            &row.kills.to_string(),
            layout.kills[i],
            rgb,
            ShellAlign::NONE,
        );
        if let Some(flag) = row
            .flag_country
            .and_then(super::flag_pcx_for_side_item_data)
            .and_then(|name| super::flag_entry(chrome, name))
        {
            super::push_flag_entry_clipped_centered(
                &mut controls,
                flag,
                layout.flags[i],
                ShellScale::RETAIL,
                super::SHELL_SWATCH_DEPTH,
            );
        }
        if show_allies && row.alliance_visible {
            paint_control(
                &mut controls,
                &chrome.control_chrome(),
                ControlPaint::Checkbox {
                    checked: row.allied,
                    rect: layout.allies[i],
                },
                ShellScale::RETAIL,
            );
        }
    }
    in_game_shell::render_in_game_shell_frame(
        state,
        encoder,
        destination,
        InGameShellFrame {
            art,
            controls,
            texts,
            label: "Active Diplomacy 73",
        },
    )
}
