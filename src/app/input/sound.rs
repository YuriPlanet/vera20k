//! B8 orchestration and physical input. Original6B6300 owns control projection;
//! Options/Theme retain values and playback, including while simulation is paused.
use super::dialog_page::InGameDialog;
use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::options::audio::{AudioVolume, apply_volume};
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::presentation::state::MatchPresentationState;
use crate::app::process_assets::ProcessAssets;
use crate::app::App;
use crate::audio::theme::ThemeRuntime;
use crate::ui::pause_menu::InGameMenuState;
use crate::ui::shell::page::PageEvent;
use crate::ui::shell::sound::{
    self, SoundButton, SoundControl, SoundLayout, SoundSlider, SoundState, SoundTrackRow,
};
use crate::util::time::Instant;
use winit::window::Window;
use crate::app::PlatformState;
use crate::app::persistence::PersistenceState;
use crate::app::renderer_state::RendererState;

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
    ui: |presentation| Some(&presentation.sound_dialog.as_ref()?.ui),
    ui_mut: |presentation| Some(&mut presentation.sound_dialog.as_mut()?.ui),
    own_checkbox_sound: false,
    // B8 reads the control under the pointer on every move, a held
    // trackbar's drag included.
    help_under_held: true,
    help_fallback: |key| {
        let mut helps = SoundControl::ALL.into_iter().map(SoundControl::help);
        helps.find(|(of, _)| *of == key).map_or("", |(_, fallback)| fallback)
    },
};
pub(crate) fn open(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
) {
    if !audio.launcher_audio_available {
        return;
    }
    // 4E2370 -> accepted4E1D9A: parent apply/write precede state6/B8. The
    // in-game menu, whose page the Sound button is on, does both as it closes
    // for this state.
    App::enter_in_game_menu_state(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
        InGameMenuState::Sound,
    );
    let positions = volume_positions(&persistence.options_profile);
    let rows = audio
        .theme
        .entries()
        .iter()
        .enumerate()
        .filter(|(index, _)| audio.theme.is_allowed(*index as i32))
        .enumerate()
        .map(|(row, (index, entry))| {
            let text = process_assets.csf_text(&entry.name_key);
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
    match_state.match_presentation.sound_dialog =
        Some(SoundState::new(positions, rows, audio.theme.current_song()));
}
/// Back `0x686` in `0x6B6300`, shared by B8 and the shell's `0xD6`:
/// re-apply the three volumes; a zero score volume queues the current song
/// and stops it. No RA2MD write.
pub(crate) fn apply_back_volumes(
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    options_profile: &mut RetailOptionsProfile,
    positions: [u8; 3],
) {
    for id in SoundSlider::ALL {
        apply_volume(
            frontend,
            match_state,
            process_assets,
            audio,
            options_profile,
            channel(id),
            (f64::from(positions[id as usize]) * 0.1) as f32,
            false,
        );
        if id == SoundSlider::Music && positions[0] == 0 {
            let now = crate::app::match_runtime::frame_pacer::wall_clock_ms();
            audio.queue_then_stop_score_zero(now);
        }
    }
}

/// Init `0x497`'s slider positions, rounded from the three volumes.
pub(crate) fn volume_positions(options_profile: &RetailOptionsProfile) -> [u8; 3] {
    let p = options_profile;
    [p.score_volume, p.sound_volume, p.voice_volume]
        .map(crate::ui::main_menu_dialogs::options::admitted_volume_position)
}

/// A changed slider applies live (`WM_HSCROLL`).
pub(crate) fn apply_slider_volume(
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    options_profile: &mut RetailOptionsProfile,
    id: SoundSlider,
    volume: f32,
) {
    apply_volume(
        frontend,
        match_state,
        process_assets,
        audio,
        options_profile,
        channel(id),
        volume,
        true,
    );
}

fn back(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
) {
    let Some(dialog) = match_state.match_presentation.sound_dialog.take() else {
        return;
    };
    apply_back_volumes(
        frontend,
        match_state,
        process_assets,
        audio,
        &mut persistence.options_profile,
        dialog.positions(),
    );
    // State_Machine 0x0048C9F4 writes state 5 after B8 returns, where gamemd
    // recreates BBB. VERA's Game Controls is the in-game menu's page, so the
    // menu is what comes back.
    App::enter_in_game_menu_state(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
        InGameMenuState::Menu,
    );
}
fn activate(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
    id: SoundButton,
) {
    let now = crate::app::match_runtime::frame_pacer::wall_clock_ms();
    match id {
        SoundButton::Back => back(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
        ),
        SoundButton::Play => {
            persistence.options_profile.in_game_music = true;
            if let Some(index) = match_state
                .match_presentation
                .sound_dialog
                .as_ref()
                .and_then(SoundState::selected_theme)
            {
                audio.play_sound_selection(index, now);
            }
        }
        SoundButton::Stop => {
            audio.stop_sound_selection(now);
            persistence.options_profile.in_game_music = false;
        }
    }
}

/// Poll the captured list gesture and return its next event-loop wake deadline.
pub(crate) fn poll_scroll_repeat(
    window: &Window,
    presentation: &mut MatchPresentationState,
) -> Option<Instant> {
    if presentation.in_game_menu != InGameMenuState::Sound {
        return None;
    }
    let dialog = presentation.sound_dialog.as_mut()?;
    dialog.ui.poll_then(|| window.request_redraw())
}

/// Shuffle and Repeat exclude each other (`0x006B6300`); both write the
/// Options owner, which the theme player reads.
fn toggle_score_option(
    theme: &mut ThemeRuntime,
    options_profile: &mut RetailOptionsProfile,
    shuffle: bool,
) {
    let p = options_profile;
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
    theme.set_score_options(p.is_score_repeat, p.is_score_shuffle);
}

/// SoundState::new built B8's controls.
fn nodes() -> vera20k_gui::NodeSpec {
    vera20k_gui::NodeSpec::new(vera20k_gui::NodeKind::Dialog)
}

/// Shuffle's and Repeat's Checked from the Options owner.
fn score_boxes(options_profile: &RetailOptionsProfile) -> [(SoundControl, bool); 2] {
    let profile = options_profile;
    [
        (SoundControl::Shuffle, profile.is_score_shuffle),
        (SoundControl::Repeat, profile.is_score_repeat),
    ]
}

fn put_boxes(
    gui: &mut vera20k_gui::Gui,
    root: vera20k_gui::NodeId,
    boxes: [(SoundControl, bool); 2],
) {
    for (control, on) in boxes {
        let value = vera20k_gui::Value::Bool(on);
        crate::ui::shell::page::set(gui, root, control.id(), vera20k_gui::Prop::Checked, value);
    }
}

/// Makes B8's nodes current without input (rects, the boxes' Checked): its
/// renderer's call every drawn frame before it paints from them.
pub(crate) fn sync(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    persistence: &PersistenceState,
) -> Option<SoundLayout> {
    let layout = super::dialog_page::layout(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
        SoundLayout::new,
    )?;
    let boxes = score_boxes(&persistence.options_profile);
    let sync = move |gui: &mut vera20k_gui::Gui, root| put_boxes(gui, root, boxes);
    DIALOG.sync(
        &mut match_state.match_presentation,
        nodes,
        |id| layout.rect(id),
        sync,
    );
    Some(layout)
}

/// One pointer message on B8 (`down`: `None` moves). Back, Play and Stop are
/// owner-draw (GUIMainButtonSound, 0x0061376B); a row press plays the
/// generic click; 6B6382/6B63EA/6B6444 suppress the rail's.
pub(super) fn pointer(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
    down: Option<bool>,
) {
    let Some(layout) = super::dialog_page::layout(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
        SoundLayout::new,
    ) else {
        return;
    };
    let boxes = score_boxes(&persistence.options_profile);
    let sync = move |gui: &mut vera20k_gui::Gui, root| put_boxes(gui, root, boxes);
    let events = DIALOG.route(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        &mut audio.sfx_player,
        persistence,
        down,
        nodes,
        |id| layout.rect(id),
        sync,
    );
    platform.window.request_redraw();
    for event in events {
        if let Some((id, volume)) = sound::shell_sound_volume(&event) {
            apply_volume(
                frontend,
                match_state,
                process_assets,
                audio,
                &mut persistence.options_profile,
                channel(id),
                volume,
                true,
            );
            continue;
        }
        let (PageEvent::Activated(id) | PageEvent::Toggled(id, _)) = event else {
            continue;
        };
        match SoundControl::from_id(id) {
            Some(SoundControl::Button(button)) => activate(
                platform,
                renderer,
                frontend,
                match_state,
                process_assets,
                audio,
                persistence,
                button,
            ),
            Some(SoundControl::Shuffle) => toggle_score_option(
                &mut audio.theme,
                &mut persistence.options_profile,
                true,
            ),
            Some(SoundControl::Repeat) => toggle_score_option(
                &mut audio.theme,
                &mut persistence.options_profile,
                false,
            ),
            _ => {}
        }
    }
}
