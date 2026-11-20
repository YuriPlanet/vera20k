//! B8 orchestration and physical input. Original6B6300 owns control projection;
//! Options/Theme retain values and playback, including while simulation is paused.
use super::dialog_page::InGameDialog;
use crate::app::persistence::options::audio::{AudioVolume, apply_volume};
use crate::app::{App, AppState};
use crate::ui::pause_menu::InGameMenuState;
use crate::ui::shell::page::PageEvent;
use crate::ui::shell::sound::{
    self, SoundButton, SoundControl, SoundLayout, SoundSlider, SoundState, SoundTrackRow,
};
use crate::util::time::Instant;

fn channel(id: SoundSlider) -> AudioVolume {
    match id {
        SoundSlider::Music => AudioVolume::Score,
        SoundSlider::Sound => AudioVolume::Sound,
        SoundSlider::Voice => AudioVolume::Voice,
    }
}
/// B8: title 0x694 and Back 0x686 as the lower right-panel button.
pub(crate) const DIALOG: InGameDialog = InGameDialog {
    id: sound::DIALOG,
    title: Some(("GUI:SoundOptions", "Sound Options")),
    lower: (0x686, ("GUI:Back", "Back")),
    label: "Active Sound B8",
    ui: |state| {
        Some(
            &state
                .match_state
                .match_presentation
                .sound_dialog
                .as_ref()?
                .ui,
        )
    },
    ui_mut: |state| {
        Some(
            &mut state
                .match_state
                .match_presentation
                .sound_dialog
                .as_mut()?
                .ui,
        )
    },
    own_checkbox_sound: false,
};
pub(crate) fn open(state: &mut AppState) {
    if !state.audio.launcher_audio_available {
        return;
    }
    // 4E2370 -> accepted4E1D9A: parent apply/write precede state6/B8. The
    // in-game menu, whose page the Sound button is on, does both as it closes
    // for this state.
    App::enter_in_game_menu_state(state, InGameMenuState::Sound);
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
    // BBB's 0x52D writes state 6; State_Machine 0x0048C8B0 then runs B8
    // (0x006B6230 at 0x0048C9EF) inside the same pause bracket. The transition
    // owner clears B8's retained state, so the new dialog is installed after it.
    state.match_state.match_presentation.sound_dialog = Some(SoundState::new(
        positions,
        rows,
        state.audio.theme.current_song(),
    ));
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
    apply_back_volumes(state, dialog.positions());
    // State_Machine 0x0048C9F4 writes state 5 after B8 returns, where gamemd
    // recreates BBB. VERA's Game Controls is the in-game menu's page, so the
    // menu is what comes back.
    App::enter_in_game_menu_state(state, InGameMenuState::Menu);
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
    let dialog = state.match_state.match_presentation.sound_dialog.as_mut()?;
    let window = &state.platform.window;
    dialog.ui.poll_then(|| window.request_redraw())
}

/// Shuffle and Repeat exclude each other (`0x006B6300`); both write the
/// Options owner, which the theme player reads.
fn toggle_score_option(state: &mut AppState, shuffle: bool) {
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
}

/// One pointer message on B8 (`down`: `None` moves). Back, Play and Stop are
/// owner-draw (GUIMainButtonSound, 0x0061376B); a row press plays the
/// generic click; 6B6382/6B63EA/6B6444 suppress the rail's.
pub(super) fn pointer(state: &mut AppState, down: Option<bool>) {
    let Some(layout) = super::dialog_page::layout(state, SoundLayout::new) else {
        return;
    };
    let profile = &state.persistence.options_profile;
    let boxes = [
        (SoundControl::Shuffle, profile.is_score_shuffle),
        (SoundControl::Repeat, profile.is_score_repeat),
    ];
    let sync = |gui: &mut vera20k_gui::Gui, root| {
        for (control, on) in boxes {
            let value = vera20k_gui::Value::Bool(on);
            crate::ui::shell::page::set(gui, root, control.id(), vera20k_gui::Prop::Checked, value);
        }
    };
    // SoundState::new built B8's controls.
    let nodes = || vera20k_gui::NodeSpec::new(vera20k_gui::NodeKind::Dialog);
    let events = DIALOG.route(state, down, nodes, |id| layout.rect(id), sync);
    state.platform.window.request_redraw();
    for event in events {
        if let Some((id, volume)) = sound::shell_sound_volume(&event) {
            apply_volume(state, channel(id), volume, true);
            continue;
        }
        let (PageEvent::Activated(id) | PageEvent::Toggled(id, _)) = event else {
            continue;
        };
        match SoundControl::from_id(id) {
            Some(SoundControl::Button(button)) => activate(state, button),
            Some(SoundControl::Shuffle) => toggle_score_option(state, true),
            Some(SoundControl::Repeat) => toggle_score_option(state, false),
            _ => {}
        }
    }
}
