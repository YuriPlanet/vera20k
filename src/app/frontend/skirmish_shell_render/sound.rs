//! Full-screen B8, original6B6300 and shared native shell/control owners.
use super::controls::{ControlPaint, paint_control};
use super::pause_menu::button_text_rect;
use super::{SHELL_CONTROL_DEPTH, SHELL_CONTROL_TEXT_DEPTH};
use crate::app::AppState;
use crate::app::input::dialog_page;
use crate::app::input::sound::DIALOG;
use crate::render::shell_paint;
use crate::ui::shell::geom::{RectPx, ShellAlign};
use crate::ui::shell::sound::{SoundButton, SoundControl, SoundLayout, SoundSlider};
use vera20k_gui::shell::list::{ListMetrics, ShellListGeometry};

pub(crate) fn render_sound_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    super::in_game_shell::paint_dialog(state, encoder, destination, &DIALOG, |paint, ui| {
        let state = paint.state;
        let layout = paint.shell.place(SoundLayout::new);
        let dialog = state
            .match_state
            .match_presentation
            .sound_dialog
            .as_ref()
            .expect("active B8 state");
        let pressed_of = |id: SoundButton| dialog_page::pressed(ui, SoundControl::Button(id).id());
        let list = dialog.list().expect("B8's list");
        let positions = dialog.positions();
        // B8 reads the control under the pointer on every move, a held
        // trackbar's drag included.
        let (x, y) = super::in_game_shell_cursor(state);
        let hovered = match dialog_page::held_trackbar(ui) {
            Some(_) => dialog_page::hit(ui, x, y),
            None => dialog_page::hovered(ui),
        };
        let help = hovered
            .and_then(SoundControl::from_id)
            .map(|control| paint.label(control.help()));
        paint.help = help;
        let atlas = state
            .frontend
            .skirmish_shell_chrome
            .as_ref()
            .expect("active B8 controls");
        let chrome = atlas.control_chrome();
        let controls = &mut paint.controls;
        for id in SoundSlider::ALL {
            let rect = layout.sliders[id as usize];
            // B8 opens only with an output device ([`SoundState`]).
            let trackbar = ControlPaint::Trackbar {
                rect,
                thumb_left: dialog.thumb_left(id, rect),
                plaque: true,
                disabled: false,
            };
            paint_control(
                controls,
                &chrome,
                trackbar,
                crate::ui::shell::geom::RETAIL_SCALE,
            );
        }
        let profile = &state.persistence.options_profile;
        for (rect, checked) in [
            (layout.shuffle, profile.is_score_shuffle),
            (layout.repeat, profile.is_score_repeat),
        ] {
            paint_control(
                controls,
                &chrome,
                ControlPaint::Checkbox { rect, checked },
                crate::ui::shell::geom::RETAIL_SCALE,
            );
        }
        let geometry = ShellListGeometry::new(
            layout.list,
            dialog.rows.len(),
            list.top,
            ListMetrics::RETAIL,
        );
        let (top, selected, held) = (list.top, list.selected, list.pressed);
        super::list::paint_list(
            controls,
            atlas,
            geometry,
            top,
            selected,
            held,
            false,
            crate::ui::shell::geom::RETAIL_SCALE,
        );
        // 609FC7..609FE2/60A330 selects type3. MNBTTN uses native canvas
        // centered on the resource control, not the side column's SIDEBTTN.
        let buttons = [SoundButton::Play, SoundButton::Stop].map(|id| shell_paint::ModalButton {
            rect: layout.button(id),
            pressed: pressed_of(id),
            enabled: true,
        });
        let depths = shell_paint::ModalDepths {
            background: SHELL_CONTROL_DEPTH,
            button: SHELL_CONTROL_DEPTH,
            text: SHELL_CONTROL_TEXT_DEPTH,
        };
        let frames = super::chrome::type3_button_frames(atlas);
        let screen = paint.screen();
        let sprites = shell_paint::paint_modal_sprites(
            None,
            frames,
            screen,
            &buttons,
            crate::ui::shell::geom::RETAIL_SCALE,
            depths,
        );
        paint.controls.extend(sprites);
        let centred = ShellAlign::H_CENTER | ShellAlign::V_CENTER;
        for (id, label) in [
            (SoundButton::Play, ("GUI:Play", "Play")),
            (SoundButton::Stop, ("GUI:Stop", "Stop")),
        ] {
            let caption = paint.label(label);
            paint.text(
                &caption,
                button_text_rect(layout.button(id), pressed_of(id)),
                centred,
            );
        }
        let labels = [
            ("GUI:MusicVolume", "Music Volume"),
            ("GUI:SoundVolume", "Sound Volume"),
            ("GUI:VoiceVolume", "Voice Volume"),
        ];
        for (index, label) in labels.into_iter().enumerate() {
            let caption = paint.label(label);
            paint.text(&caption, layout.labels[index], ShellAlign::H_RIGHT);
            let value = vera20k_gui::shell::trackbar::value_text_rect(
                layout.sliders[index],
                crate::ui::shell::geom::RETAIL_SCALE,
            );
            paint.text(&positions[index].to_string(), value, centred);
        }
        for (rect, label) in [
            (layout.shuffle, ("GUI:Shuffle", "Shuffle")),
            (layout.repeat, ("GUI:Repeat", "Repeat")),
        ] {
            let caption = paint.label(label);
            paint.text(
                &caption,
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
            paint.text(
                &row.text,
                RectPx::new(r.x + 2, r.y, r.w - 2, r.h),
                ShellAlign::NONE,
            );
        }
    })
}
