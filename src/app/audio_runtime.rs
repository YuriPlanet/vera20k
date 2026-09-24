//! Process-wide audio runtime (F12 `AppAudioRuntime`): output players and
//! sound/EVA registries that live for the whole process.
//!
//! The per-match event queue lives in
//! `app::match_audio::MatchAudioState`; gameplay EVA latches live in sim.
//! This process audio owner survives matches. The registries are reloaded on
//! each map load today (redundant but harmless —
//! they consume no map-specific input); that behavior is unchanged here.

use crate::assets::asset_manager::AssetManager;
use crate::audio::music::MusicPlayer;
use crate::audio::sfx::SfxPlayer;
use crate::audio::theme::{
    MusicOutputState, PreparedTrack, ThemeAction, ThemeAllowContext, ThemeGates, ThemeRuntime,
};

/// `AudioSystem__Pump @ 0x00406F70` runs its services only when more than
/// 0x21 ms elapsed since the previous pass.
const THEME_POLL_GATE_MS: u64 = 0x21;
use crate::rules::sound_ini::{EvaRegistry, SoundRegistry};

pub(crate) const fn derive_launcher_audio_available(
    audio_requested: bool,
    music_output_ready: bool,
    sfx_output_ready: bool,
) -> bool {
    audio_requested && (music_output_ready || sfx_output_ready)
}

trait ThemeMusicOutput {
    fn set_theme_scale(&mut self, scale: f64);
    fn stop(&mut self);
    fn submit(&mut self, prepared: PreparedTrack) -> bool;
}

impl ThemeMusicOutput for MusicPlayer {
    fn set_theme_scale(&mut self, scale: f64) {
        MusicPlayer::set_theme_scale(self, scale);
    }

    fn stop(&mut self) {
        MusicPlayer::stop(self);
    }

    fn submit(&mut self, prepared: PreparedTrack) -> bool {
        MusicPlayer::submit(self, prepared)
    }
}

fn apply_theme_action_to_output(
    mut output: Option<&mut impl ThemeMusicOutput>,
    action: ThemeAction,
) {
    if let Some(scale) = action.theme_scale
        && let Some(output) = output.as_deref_mut()
    {
        output.set_theme_scale(scale);
    }
    if action.stop_output
        && let Some(output) = output.as_deref_mut()
    {
        output.stop();
    }
    if let Some(prepared) = action.start
        && let Some(output) = output.as_deref_mut()
        && !output.submit(prepared)
    {
        // gamemd ignores PlayFile's return and still writes logical active.
        log::warn!("Physical music submission failed after Theme admission");
    }
}

pub(crate) struct AppAudioRuntime {
    /// Always-present device-independent Theme owner.
    pub(crate) theme: ThemeRuntime,
    /// Last wall time the Theme AI ran (the audio pump's own > 33 ms gate).
    pub(crate) last_theme_poll_ms: Option<u64>,
    /// Background music player (rodio). `None` when audio output is disabled
    /// or initialization failed.
    pub(crate) music_player: Option<MusicPlayer>,
    /// Sound effect player (rodio) — one-shot SFX (weapons, voices, UI).
    pub(crate) sfx_player: Option<SfxPlayer>,
    /// sound.ini / soundmd.ini registry mapping IDs to .wav filenames.
    pub(crate) sound_registry: SoundRegistry,
    /// audio.idx/bag indices for bag-based sound lookup (voices, EVA).
    /// Searched in order (YR audiomd first, then base audio).
    pub(crate) audio_indices: Vec<crate::assets::audio_bag::AudioIndex>,
    /// The process-start audio decision persisted for later scenario reloads.
    pub(crate) audio_indices_enabled: bool,
    /// Rust-native substitute for native's one shared DirectSound-device gate.
    /// Frozen after both process-start output constructor attempts.
    pub(crate) launcher_audio_available: bool,
    /// Native has a startup-suppression gate. Current Rust has no non-default
    /// route, so production initializes this false and keeps one explicit seam.
    pub(crate) theme_startup_suppressed: bool,
    /// EVA announcement registry from eva.ini / evamd.ini.
    /// Maps EVA event names to per-faction audio.bag sound IDs.
    pub(crate) eva_registry: EvaRegistry,
}

/// gamemd-derived: Theme admission and logical/physical command ordering come
/// from `ThemeClass` AI `0x007209D0`, Next `0x00720A80`, Queue `0x00720B20`,
/// Play `0x00720BB0`, and Stop `0x00720EA0`. Their shared device gate calls
/// `FUN_00407000` (`DAT_0087E728 != 0`) alongside Theme initialization
/// `DAT_00A8EC74 != 0` and startup suppression `DAT_00A8ED64 == 0`.
impl AppAudioRuntime {
    fn theme_gates(&self) -> ThemeGates {
        ThemeGates {
            launcher_audio_available: self.launcher_audio_available,
            startup_suppressed: self.theme_startup_suppressed,
        }
    }

    fn music_output_state(&self) -> MusicOutputState {
        self.music_player
            .as_ref()
            .map_or(MusicOutputState::Unavailable, MusicPlayer::state)
    }

    fn apply_theme_action(&mut self, action: ThemeAction) {
        apply_theme_action_to_output(self.music_player.as_mut(), action);
    }

    pub(crate) fn initialize_theme(&mut self, assets: &AssetManager) {
        self.theme.initialize_catalog(assets);
    }

    pub(crate) fn maintain_main_menu_theme(&mut self, assets: &AssetManager, wall_ms: u64) {
        // Let Theme AI consume a real physical completion first. A subsequent
        // direct INTRO maintenance call then takes the native same-track no-op
        // when AI already restarted the repeating menu theme.
        self.update_theme(assets, wall_ms);
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action = self.theme.play_menu_theme(assets, gates, physical, wall_ms);
        self.apply_theme_action(action);
    }

    /// `ThemeClass::AI @ 0x007209D0` as driven by `AudioSystem__Pump @
    /// 0x00406F70`: every screen, rate-gated to more than 33 ms.
    pub(crate) fn update_theme(&mut self, assets: &AssetManager, wall_ms: u64) {
        if self
            .last_theme_poll_ms
            .is_some_and(|last| wall_ms.saturating_sub(last) <= THEME_POLL_GATE_MS)
        {
            return;
        }
        self.last_theme_poll_ms = Some(wall_ms);
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        if physical == MusicOutputState::Finished
            && let Some(output) = self.music_player.as_mut()
        {
            output.discard_finished();
        }
        let action = self.theme.update(assets, gates, physical, wall_ms);
        self.apply_theme_action(action);
    }

    /// `Play_Song(From_Name(track))`.
    pub(crate) fn play_theme(&mut self, track: &str, assets: &AssetManager, wall_ms: u64) -> bool {
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action = self
            .theme
            .play_track(track, assets, gates, physical, wall_ms);
        let logical_started = action.start.is_some();
        self.apply_theme_action(action);
        logical_started
    }

    /// Start_Scenario tail: seed the presentation shuffle stream, pin the
    /// local player's side, then `Stop(1)` / `Queue_Song([Basic] Theme)`.
    pub(crate) fn request_scenario_theme(
        &mut self,
        requested_section: Option<&str>,
        assets: &AssetManager,
        match_seed: u32,
        context: ThemeAllowContext,
        resolve_side: impl Fn(&str) -> Option<i32>,
        wall_ms: u64,
    ) {
        self.theme.initialize_catalog(assets);
        self.theme.begin_scenario(match_seed, context, resolve_side);
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action =
            self.theme
                .request_scenario_theme(requested_section, assets, gates, physical, wall_ms);
        self.apply_theme_action(action);
    }

    /// `Main_Tick @ 0x0055D360` head rule while a scenario runs.
    pub(crate) fn main_tick_theme(&mut self, in_game_music: bool, wall_ms: u64) {
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action = self
            .theme
            .main_tick(in_game_music, gates, physical, wall_ms);
        self.apply_theme_action(action);
    }

    pub(crate) fn cancel_scenario_theme_request(&mut self) {
        let action = self.theme.cancel_scenario_theme_request();
        self.apply_theme_action(action);
    }

    /// `Queue_Song(From_Name(track))` @ `0x00720B20`; Theme AI starts it.
    pub(crate) fn queue_theme(&mut self, track: &str, wall_ms: u64) {
        let index = self.theme.from_name(track);
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action = self.theme.queue_song(index, gates, physical, wall_ms);
        self.apply_theme_action(action);
    }

    /// `ThemeClass::Stop(fade=0)`.
    pub(crate) fn stop_theme(&mut self) {
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action = self.theme.stop(gates, false, physical, 0);
        self.apply_theme_action(action);
    }

    /// B8 user commands preserve Theme fade/queue ordering and output ownership.
    pub(crate) fn play_sound_selection(&mut self, index: i32, wall_ms: u64) {
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action = self.theme.play_selection(index, gates, physical, wall_ms);
        self.apply_theme_action(action);
    }
    pub(crate) fn stop_sound_selection(&mut self, wall_ms: u64) {
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action = self.theme.stop_selection(gates, physical, wall_ms);
        self.apply_theme_action(action);
    }
    /// Launcher ScoreVolume zero (`0x0055FAA0`): `Queue(cur)` then `Stop(0)`.
    pub(crate) fn queue_then_stop_score_zero(&mut self, wall_ms: u64) {
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action = self
            .theme
            .queue_then_stop_score_zero(gates, physical, wall_ms);
        self.apply_theme_action(action);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    enum OutputCall {
        Scale(f64),
        Stop,
        Submit(String),
    }

    struct FakeOutput {
        calls: Vec<OutputCall>,
        submit_succeeds: bool,
    }

    impl ThemeMusicOutput for FakeOutput {
        fn set_theme_scale(&mut self, scale: f64) {
            self.calls.push(OutputCall::Scale(scale));
        }

        fn stop(&mut self) {
            self.calls.push(OutputCall::Stop);
        }

        fn submit(&mut self, prepared: PreparedTrack) -> bool {
            self.calls.push(OutputCall::Submit(prepared.stem));
            self.submit_succeeds
        }
    }

    #[test]
    fn launcher_audio_gate_uses_one_process_start_predicate() {
        assert!(!derive_launcher_audio_available(false, false, false));
        assert!(!derive_launcher_audio_available(false, true, true));
        assert!(!derive_launcher_audio_available(true, false, false));
        assert!(derive_launcher_audio_available(true, true, false));
        assert!(derive_launcher_audio_available(true, false, true));
        assert!(derive_launcher_audio_available(true, true, true));
    }

    fn empty_assets(label: &str) -> AssetManager {
        let dir = std::env::temp_dir().join(format!(
            "vera20k-audio-runtime-{}-{label}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("test asset dir");
        AssetManager::from_loose_root_for_test(&dir)
    }

    /// `AudioSystem__Pump @ 0x00406F70` services `ThemeClass::AI` only when
    /// more than 0x21 ms passed; the owner carries that gate itself so the
    /// unconditional per-frame pump (`frame.rs`, outside every screen gate)
    /// reaches AI on the menu, loading and score screens alike.
    #[test]
    fn theme_poll_carries_the_audio_pump_rate_gate_independent_of_screen() {
        let assets = empty_assets("poll-gate");
        let mut runtime = AppAudioRuntime {
            theme: ThemeRuntime::default(),
            last_theme_poll_ms: None,
            music_player: None,
            sfx_player: None,
            sound_registry: SoundRegistry::default(),
            audio_indices: Vec::new(),
            audio_indices_enabled: false,
            launcher_audio_available: true,
            theme_startup_suppressed: false,
            eva_registry: EvaRegistry::default(),
        };
        runtime.update_theme(&assets, 100);
        assert_eq!(runtime.last_theme_poll_ms, Some(100));
        runtime.update_theme(&assets, 133);
        assert_eq!(runtime.last_theme_poll_ms, Some(100), "33 ms is not > 0x21");
        runtime.update_theme(&assets, 134);
        assert_eq!(runtime.last_theme_poll_ms, Some(134));
    }

    #[test]
    fn post_admission_output_failure_cannot_rewrite_theme_state() {
        let theme = ThemeRuntime::default();
        let before = format!("{:?}", theme);
        let action = ThemeAction {
            stop_output: true,
            theme_scale: Some(0.25),
            start: Some(PreparedTrack {
                stem: "Drok".into(),
                samples: vec![0.0, 0.0],
                sample_rate: 22_050,
            }),
        };
        let mut output = FakeOutput {
            calls: Vec::new(),
            submit_succeeds: false,
        };

        apply_theme_action_to_output(Some(&mut output), action);

        assert_eq!(
            output.calls,
            vec![
                OutputCall::Scale(0.25),
                OutputCall::Stop,
                OutputCall::Submit("Drok".into()),
            ]
        );
        assert_eq!(format!("{:?}", theme), before);
    }
}
