//! B8 orchestration and physical input. Original6B6300 owns control projection;
//! Options/Theme retain values and playback, including while simulation is paused.
use crate::app::persistence::options::{
    self,
    audio::{AudioVolume, apply_volume},
};
use crate::app::{App, AppState};
use crate::ui::pause_menu::InGameMenuState;
use crate::ui::shell::list::ListMetrics;
use crate::ui::shell::list::ShellListGeometry;
use crate::ui::shell::sound::{
    SoundButton, SoundControl, SoundLayout, SoundSlider, SoundState, SoundTrackRow,
};
use crate::util::time::Instant;
use winit::event::MouseButton;

fn channel(id: SoundSlider) -> AudioVolume {
    match id {
        SoundSlider::Music => AudioVolume::Score,
        SoundSlider::Sound => AudioVolume::Sound,
        SoundSlider::Voice => AudioVolume::Voice,
    }
}
fn layout(state: &AppState) -> Option<SoundLayout> {
    let shell = crate::app::frontend::skirmish_shell_render::current_in_game_shell_layout(state)?;
    Some(SoundLayout::new(
        shell.width,
        shell.height,
        shell.layout,
        shell.button_size,
    ))
}
pub(crate) fn open(state: &mut AppState) {
    if !state.audio.launcher_audio_available {
        return;
    }
    // 4E2370 -> accepted4E1D9A: parent apply/write precede state6/B8.
    options::accept_in_game_options(state);
    let positions = volume_positions(state);
    let rows = state
        .audio
        .theme
        .entries()
        .iter()
        .enumerate()
        .filter(|(index, _)| state.audio.theme.is_allowed(*index as i32))
        .enumerate()
        .map(|(row, (index, entry))| {
            let text = state
                .process_assets
                .csf
                .as_ref()
                .map(|csf| csf.text(&entry.name_key).into_owned())
                .unwrap_or_else(|| entry.name_key.clone());
            // Native720480's localized Name buffer holds63 UTF16 units.
            let name = String::from_utf16_lossy(&text.encode_utf16().take(63).collect::<Vec<_>>());
            SoundTrackRow {
                theme_index: index as i32,
                text: format!(
                    "{:02} - {} [{}:{:02}]",
                    row + 1,
                    name,
                    entry.duration_seconds / 60,
                    entry.duration_seconds % 60
                ),
            }
        })
        .collect();
    state.match_state.match_presentation.sound_dialog = Some(SoundState::new(
        positions,
        rows,
        state.audio.theme.current_song(),
    ));
    state.match_state.match_presentation.in_game_menu = InGameMenuState::Sound;
}
/// Back `0x686` in `0x6B6300`, shared by B8 and the shell's `0xD6`:
/// re-apply the three volumes; a zero score volume queues the current song
/// and stops it. No RA2MD write.
pub(crate) fn apply_back_volumes(state: &mut AppState, positions: [u8; 3]) {
    for id in SoundSlider::ALL {
        apply_volume(
            state,
            channel(id),
            (f64::from(positions[id as usize]) * 0.1) as f32,
            false,
        );
        if id == SoundSlider::Music && positions[0] == 0 {
            let now = crate::app::match_runtime::frame_pacer::wall_clock_ms();
            state.audio.queue_then_stop_score_zero(now);
        }
    }
}

/// Init `0x497`'s slider positions, rounded from the three volumes.
pub(crate) fn volume_positions(state: &AppState) -> [u8; 3] {
    let p = &state.persistence.options_profile;
    [p.score_volume, p.sound_volume, p.voice_volume]
        .map(crate::ui::main_menu_dialogs::options::admitted_volume_position)
}

/// A changed slider applies live (`WM_HSCROLL`).
pub(crate) fn apply_slider_volume(state: &mut AppState, id: SoundSlider, volume: f32) {
    apply_volume(state, channel(id), volume, true);
}

fn back(state: &mut AppState) {
    let Some(dialog) = state.match_state.match_presentation.sound_dialog.take() else {
        return;
    };
    apply_back_volumes(state, dialog.positions);
    state.match_state.match_presentation.in_game_options =
        options::in_game_options_from_profile(&state.persistence.options_profile);
    crate::app::input::in_game_options::on_open(state);
    state.match_state.match_presentation.in_game_menu = InGameMenuState::Options;
}
fn activate(state: &mut AppState, id: SoundButton) {
    let now = crate::app::match_runtime::frame_pacer::wall_clock_ms();
    match id {
        SoundButton::Back => back(state),
        SoundButton::Play => {
            state.persistence.options_profile.in_game_music = true;
            if let Some(index) = state
                .match_state
                .match_presentation
                .sound_dialog
                .as_ref()
                .and_then(SoundState::selected_theme)
            {
                state.audio.play_sound_selection(index, now);
            }
        }
        SoundButton::Stop => {
            state.audio.stop_sound_selection(now);
            state.persistence.options_profile.in_game_music = false;
        }
    }
}

/// Poll the captured list gesture and return its next event-loop wake deadline.
pub(crate) fn poll_scroll_repeat(state: &mut AppState) -> Option<Instant> {
    if state.match_state.match_presentation.in_game_menu != InGameMenuState::Sound {
        return None;
    }
    let layout = layout(state)?;
    let (x, y) = crate::app::frontend::skirmish_shell_render::in_game_shell_cursor(state);
    let dialog = state.match_state.match_presentation.sound_dialog.as_mut()?;
    let geometry = ShellListGeometry::new(
        layout.list,
        dialog.rows.len(),
        dialog.top,
        ListMetrics::RETAIL,
    );
    if dialog
        .scroll
        .poll(geometry, &mut dialog.top, x, y, Instant::now())
    {
        state.platform.window.request_redraw();
    }
    dialog.scroll.repeat_at()
}
pub(crate) fn cursor_moved(state: &mut AppState) {
    let Some(layout) = layout(state) else {
        return;
    };
    let (x, y) = crate::app::frontend::skirmish_shell_render::in_game_shell_cursor(state);
    let Some(dialog) = state.match_state.match_presentation.sound_dialog.as_mut() else {
        return;
    };
    dialog.hovered = layout.control_at(x, y);
    dialog.buttons.hovered = match dialog.hovered {
        Some(SoundControl::Button(id)) => Some(id),
        _ => None,
    };
    let geometry = ShellListGeometry::new(
        layout.list,
        dialog.rows.len(),
        dialog.top,
        ListMetrics::RETAIL,
    );
    dialog.scroll.pointer_moved(geometry, &mut dialog.top, x, y);
    let changed = dialog.dragging.and_then(|id| {
        dialog
            .set_from_pointer(id, layout.sliders[id as usize], x)
            .map(|v| (id, v))
    });
    if let Some((id, volume)) = changed {
        apply_volume(state, channel(id), volume, true);
    }
}
pub(crate) fn mouse(state: &mut AppState, button: MouseButton, pressed: bool) {
    if button != MouseButton::Left {
        return;
    }
    let Some(layout) = layout(state) else {
        return;
    };
    let (x, y) = crate::app::frontend::skirmish_shell_render::in_game_shell_cursor(state);
    let Some(dialog) = state.match_state.match_presentation.sound_dialog.as_mut() else {
        return;
    };
    let hit = layout.control_at(x, y);
    let over_button = match hit {
        Some(SoundControl::Button(id)) => Some(id),
        _ => None,
    };
    if !pressed {
        dialog.dragging = None;
        dialog.scroll.cancel();
        state.platform.window.request_redraw();
        let action = dialog.buttons.release(over_button);
        if let Some(id) = action {
            activate(state, id);
        }
        return;
    }
    dialog.scroll.cancel();
    dialog.buttons.press(over_button);
    match hit {
        // Back, Play and Stop are owner-draw: GUIMainButtonSound (0x0061376B).
        Some(SoundControl::Button(_)) => App::play_main_menu_button_sound(state),
        Some(SoundControl::Slider(id)) => {
            if let Some(volume) = dialog.press_slider(id, layout.sliders[id as usize], x, y) {
                apply_volume(state, channel(id), volume, true);
            }
            // 6B6382/6B63EA/6B6444 explicitly suppress generic rail click.
        }
        Some(SoundControl::Shuffle | SoundControl::Repeat) => {
            let shuffle = hit == Some(SoundControl::Shuffle);
            let rect = if shuffle {
                layout.shuffle
            } else {
                layout.repeat
            };
            if !crate::ui::shell::geom::RectPx::new(rect.x, rect.y, 18, 18).contains(x, y) {
                return;
            }
            let p = &mut state.persistence.options_profile;
            if shuffle {
                p.is_score_shuffle = !p.is_score_shuffle;
                if p.is_score_shuffle {
                    p.is_score_repeat = false;
                }
            } else {
                p.is_score_repeat = !p.is_score_repeat;
                if p.is_score_repeat {
                    p.is_score_shuffle = false;
                }
            }
            state
                .audio
                .theme
                .set_score_options(p.is_score_repeat, p.is_score_shuffle);
            let cue = state
                .rules()
                .and_then(|r| r.general.gui_checkbox_sound.clone());
            App::play_shell_ui_sound_by_id(state, cue.as_deref());
        }
        Some(SoundControl::List) => {
            let geometry = ShellListGeometry::new(
                layout.list,
                dialog.rows.len(),
                dialog.top,
                ListMetrics::RETAIL,
            );
            if let Some(part) = geometry.scroll_part_at(x, y) {
                dialog
                    .scroll
                    .press(part, geometry, &mut dialog.top, y, Instant::now());
            } else if let Some(row) = geometry.row_at(dialog.rows.len(), dialog.top, x, y) {
                dialog.selected = Some(row);
                App::play_skirmish_shell_generic_click_sound(state);
            }
        }
        None => {}
    }
}
