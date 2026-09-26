//! Full-screen B8, original6B6300 and shared native shell/control owners.
use super::controls::{ControlPaint, paint_control};
use super::in_game_shell::{self, InGameShellFrame};
use super::pause_menu::{button_frame, button_text_rect};
use super::text::{localized_label, push_text_draw, rect_to_text_rect};
use super::{SHELL_CONTROL_DEPTH, SHELL_CONTROL_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB};
use crate::app::AppState;
use crate::render::{shell_paint, shell_text::ShellAlign};
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::list::ShellListGeometry;
use crate::ui::shell::pause_menu::PauseMenuButtonState;
use crate::ui::shell::sound::{SoundButton, SoundLayout, SoundSlider};

pub(crate) fn render_sound_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let width = state.renderer.gpu.config.width as i32;
    let height = state.renderer.gpu.config.height as i32;
    let (shell, size) =
        in_game_shell::current_in_game_shell_layout(state).expect("active B8 geometry");
    let layout = SoundLayout::new(width, height, shell, size);
    let dialog = state
        .match_state
        .match_presentation
        .sound_dialog
        .as_ref()
        .expect("active B8 state");
    let atlas = crate::app::presentation::sidebar_render::current_sidebar_chrome(state)
        .expect("active B8 art");
    let mut art = in_game_shell::background_instances(atlas, shell, width, height);
    let pressed = dialog.buttons.is_pressed(SoundButton::Back);
    let frame = button_frame(PauseMenuButtonState {
        pressed,
        ..Default::default()
    });
    if let Some(entry) = atlas.in_game_shell.buttons[frame].or(atlas.in_game_shell.buttons[0]) {
        in_game_shell::push_art(
            &mut art,
            entry,
            layout.back,
            RectPx::new(0, 0, width, height),
        );
    }
    let atlas = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .expect("active B8 controls");
    let mut controls = Vec::new();
    let chrome = atlas.control_chrome();
    for id in SoundSlider::ALL {
        let rect = layout.sliders[id as usize];
        paint_control(
            &mut controls,
            &chrome,
            ControlPaint::Trackbar {
                rect,
                thumb_left: dialog.thumb_left(id, rect),
                plaque: true,
            },
        );
    }
    let profile = &state.persistence.options_profile;
    for (rect, checked) in [
        (layout.shuffle, profile.is_score_shuffle),
        (layout.repeat, profile.is_score_repeat),
    ] {
        paint_control(
            &mut controls,
            &chrome,
            ControlPaint::Checkbox { rect, checked },
        );
    }
    let geometry = ShellListGeometry::new(layout.list, dialog.rows.len(), dialog.top);
    super::list::paint_list(
        &mut controls,
        atlas,
        geometry,
        dialog.top,
        dialog.selected,
        dialog.scroll.pressed_part(),
        false,
    );
    // 609FC7..609FE2/60A330 selects type3. MNBTTN uses native canvas
    // centered on the resource control, not the side column's SIDEBTTN.
    let buttons = [SoundButton::Play, SoundButton::Stop].map(|id| shell_paint::ModalButton {
        rect: layout.button(id),
        pressed: dialog.buttons.is_pressed(id),
        enabled: true,
    });
    controls.extend(shell_paint::paint_modal_sprites(
        None,
        super::chrome::type3_button_frames(atlas),
        RectPx::new(0, 0, width, height),
        &buttons,
        shell_paint::ModalDepths {
            background: SHELL_CONTROL_DEPTH,
            button: SHELL_CONTROL_DEPTH,
            text: SHELL_CONTROL_TEXT_DEPTH,
        },
    ));
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
        )
    };
    text(
        &localized_label(state, "GUI:SoundOptions", "Sound Options"),
        layout.title,
        ShellAlign::H_CENTER,
    );
    for (id, key, fallback) in [
        (SoundButton::Back, "GUI:Back", "Back"),
        (SoundButton::Play, "GUI:Play", "Play"),
        (SoundButton::Stop, "GUI:Stop", "Stop"),
    ] {
        text(
            &localized_label(state, key, fallback),
            button_text_rect(layout.button(id), dialog.buttons.is_pressed(id)),
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
        );
    }
    for (index, (key, fallback)) in [
        ("GUI:MusicVolume", "Music Volume"),
        ("GUI:SoundVolume", "Sound Volume"),
        ("GUI:VoiceVolume", "Voice Volume"),
    ]
    .into_iter()
    .enumerate()
    {
        text(
            &localized_label(state, key, fallback),
            layout.labels[index],
            ShellAlign::H_RIGHT,
        );
        text(
            &dialog.positions[index].to_string(),
            crate::ui::shell::trackbar::value_text_rect(layout.sliders[index]),
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
        );
    }
    for (rect, key, fallback) in [
        (layout.shuffle, "GUI:Shuffle", "Shuffle"),
        (layout.repeat, "GUI:Repeat", "Repeat"),
    ] {
        text(
            &localized_label(state, key, fallback),
            RectPx::new(rect.x + 26, rect.y, rect.w - 26, rect.h),
            ShellAlign::V_CENTER,
        );
    }
    for (visible, row) in dialog
        .rows
        .iter()
        .skip(dialog.top)
        .take(geometry.visible_rows)
        .enumerate()
    {
        let r = geometry.row(visible);
        text(
            &row.text,
            RectPx::new(r.x + 2, r.y, r.w - 2, r.h),
            ShellAlign::NONE,
        );
    }
    if let Some(control) = dialog.hovered {
        let (key, fallback) = control.help();
        text(
            &localized_label(state, key, fallback),
            layout.footer,
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
            label: "Active Sound B8",
        },
    )
}
