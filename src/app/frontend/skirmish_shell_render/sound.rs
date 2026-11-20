//! Full-screen B8, original6B6300 and shared native shell/control owners.
use super::controls::{ControlPaint, paint_control};
use super::in_game_shell::{self, InGameShellFrame};
use super::pause_menu::button_text_rect;
use super::text::{localized_label, push_text_draw, rect_to_text_rect};
use super::{SHELL_CONTROL_DEPTH, SHELL_CONTROL_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB};
use crate::app::AppState;
use crate::render::{shell_paint, shell_text::ShellAlign};
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::list::ListMetrics;
use crate::ui::shell::list::ShellListGeometry;
use crate::ui::shell::sound::{SoundButton, SoundControl, SoundLayout, SoundSlider};

pub(crate) fn render_sound_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let in_game_shell::InGameShellGeometry {
        width,
        height,
        layout: shell,
        button_size: size,
    } = in_game_shell::current_in_game_shell_layout(state).expect("active B8 geometry");
    let layout = SoundLayout::new(width, height, shell, size);
    let dialog = state
        .match_state
        .match_presentation
        .sound_dialog
        .as_ref()
        .expect("active B8 state");
    let pressed_of = |id: SoundButton| {
        crate::app::input::dialog_page::pressed(&dialog.ui, SoundControl::Button(id).id())
    };
    let list = dialog.list().expect("B8's list");
    let positions = dialog.positions();
    // B8 reads the control under the pointer on every move, a held
    // trackbar's drag included.
    let (x, y) = in_game_shell::in_game_shell_cursor(state);
    let hovered = match crate::app::input::dialog_page::held_trackbar(&dialog.ui) {
        Some(_) => crate::app::input::dialog_page::hit(&dialog.ui, x, y),
        None => crate::app::input::dialog_page::hovered(&dialog.ui),
    }
    .and_then(SoundControl::from_id);
    let atlas = crate::app::presentation::sidebar_render::current_sidebar_chrome(state)
        .expect("active B8 art");
    let mut art = in_game_shell::background_instances(atlas, shell, width, height);
    let mut texts = Vec::new();
    let back = localized_label(state, "GUI:Back", "Back");
    let back = (layout.back, back.as_str(), pressed_of(SoundButton::Back));
    let screen = RectPx::new(0, 0, width, height);
    in_game_shell::push_panel_button(&mut art, &mut texts, state, atlas, back, screen);
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
                // B8 opens only with an output device ([`SoundState`]).
                disabled: false,
            },
            ShellScale::RETAIL,
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
            ShellScale::RETAIL,
        );
    }
    let geometry = ShellListGeometry::new(
        layout.list,
        dialog.rows.len(),
        list.top,
        ListMetrics::RETAIL,
    );
    super::list::paint_list(
        &mut controls,
        atlas,
        geometry,
        list.top,
        list.selected,
        list.pressed,
        false,
        ShellScale::RETAIL,
    );
    // 609FC7..609FE2/60A330 selects type3. MNBTTN uses native canvas
    // centered on the resource control, not the side column's SIDEBTTN.
    let buttons = [SoundButton::Play, SoundButton::Stop].map(|id| shell_paint::ModalButton {
        rect: layout.button(id),
        pressed: pressed_of(id),
        enabled: true,
    });
    controls.extend(shell_paint::paint_modal_sprites(
        None,
        super::chrome::type3_button_frames(atlas),
        RectPx::new(0, 0, width, height),
        &buttons,
        ShellScale::RETAIL,
        shell_paint::ModalDepths {
            background: SHELL_CONTROL_DEPTH,
            button: SHELL_CONTROL_DEPTH,
            text: SHELL_CONTROL_TEXT_DEPTH,
        },
    ));
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
        &localized_label(state, "GUI:SoundOptions", "Sound Options"),
        layout.title,
        ShellAlign::PANEL_TITLE,
    );
    for (id, key, fallback) in [
        (SoundButton::Play, "GUI:Play", "Play"),
        (SoundButton::Stop, "GUI:Stop", "Stop"),
    ] {
        text(
            &localized_label(state, key, fallback),
            button_text_rect(layout.button(id), pressed_of(id)),
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
            &positions[index].to_string(),
            crate::ui::shell::trackbar::value_text_rect(layout.sliders[index], ShellScale::RETAIL),
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
        .skip(list.top)
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
    if let Some(control) = hovered {
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
