//! Process-wide audio output and Theme runtime. Immutable SFX/EVA definitions
//! and the selected sample index belong to `ProcessAssets`; match event queues
//! belong to `MatchAudioState`. Output state survives scenario transitions.

use crate::assets::asset_manager::AssetManager;
use crate::audio::arbiter::AudioServiceClock;
use crate::audio::music::MusicPlayer;
use crate::audio::sfx::SfxPlayer;
use crate::audio::theme::{
    MusicOutputState, PreparedTrack, ThemeAction, ThemeAllowContext, ThemeGates, ThemeRuntime,
};

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
    /// One process cadence for Sound, Vox and Theme; device callbacks run
    /// independently of this gate.
    pub(crate) service_clock: AudioServiceClock,
    /// Background music player (rodio). `None` when audio output is disabled
    /// or initialization failed.
    pub(crate) music_player: Option<MusicPlayer>,
    /// Sound effect player (rodio) — one-shot SFX (weapons, voices, UI).
    pub(crate) sfx_player: Option<SfxPlayer>,
    /// Rust-native substitute for native's one shared DirectSound-device gate.
    /// Frozen after both process-start output constructor attempts.
    pub(crate) launcher_audio_available: bool,
    /// Native has a startup-suppression gate. Current Rust has no non-default
    /// route, so production initializes this false and keeps one explicit seam.
    pub(crate) theme_startup_suppressed: bool,
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
        // The common audio service already consumed any physical completion.
        // INTRO maintenance takes the native same-track no-op when Theme AI
        // restarted the repeating menu theme on that service pass.
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action = self.theme.play_menu_theme(assets, gates, physical, wall_ms);
        self.apply_theme_action(action);
    }

    /// `AudioSystem406F70`: service Sound4041D0, Vox752760, then Theme7209D0
    /// behind one process gate. Device completion/refill is serviced on every
    /// call. Each consumer borrows the same Main886B88 continuation.
    pub(crate) fn service_audio(
        &mut self,
        wall_ms: u64,
        paused: bool,
        assets: Option<&AssetManager>,
        catalog: Option<&crate::app::process_assets::ProcessAudioCatalog>,
        main: &mut crate::sim::rng::MainRngDraws<'_>,
    ) {
        let mut draw_main = |low, high| main.ranged(low, high);
        if let Some(sfx) = self.sfx_player.as_mut() {
            sfx.set_paused(paused, wall_ms);
            if let Some(catalog) = catalog {
                sfx.service_device_outputs(wall_ms, catalog.sounds(), &mut draw_main);
            }
        }
        if !self.service_clock.admit(wall_ms) {
            return;
        }
        let Some(assets) = assets else {
            return;
        };
        if let (Some(sfx), Some(catalog)) = (self.sfx_player.as_mut(), catalog) {
            sfx.service_events(
                wall_ms,
                catalog.sounds(),
                assets,
                catalog.index(),
                &mut draw_main,
            );
        }
        self.service_theme(assets, wall_ms, &mut draw_main);
    }

    /// Theme AI is the last service in the shared AudioSystem pass. Callers
    /// request tracks separately; none run another independently gated AI.
    fn service_theme(
        &mut self,
        assets: &AssetManager,
        wall_ms: u64,
        draw_main: &mut impl FnMut(i32, i32) -> i32,
    ) {
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        if physical == MusicOutputState::Finished
            && let Some(output) = self.music_player.as_mut()
        {
            output.discard_finished();
        }
        let action = self
            .theme
            .update(assets, gates, physical, wall_ms, draw_main);
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

    /// Start_Scenario tail: pin the local player's side, then `Stop(1)` /
    /// `Queue_Song([Basic] Theme)`. Later AI borrows the installed Main stream.
    pub(crate) fn request_scenario_theme(
        &mut self,
        requested_section: Option<&str>,
        assets: &AssetManager,
        context: ThemeAllowContext,
        resolve_side: impl Fn(&str) -> Option<i32>,
        wall_ms: u64,
    ) {
        self.theme.initialize_catalog(assets);
        self.theme.begin_scenario(context, resolve_side);
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

    /// `WOL_Main` entry: INTRO gives way to the shuffled lobby music
    /// (`ThemeRuntime::enter_wol_lobby`). Returns the shuffle flag to restore.
    pub(crate) fn enter_wol_lobby_music(&mut self, lobby_music: bool, wall_ms: u64) -> bool {
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let (action, saved_shuffle) =
            self.theme
                .enter_wol_lobby(lobby_music, gates, physical, wall_ms);
        self.apply_theme_action(action);
        saved_shuffle
    }

    /// `WOL_Main` exit: the saved shuffle flag returns.
    pub(crate) fn leave_wol_lobby_music(&mut self, saved_shuffle: bool) {
        self.theme.leave_wol_lobby(saved_shuffle);
    }

    /// `ThemeClass::Stop(fade=0)`.
    pub(crate) fn stop_theme(&mut self) {
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action = self.theme.stop(gates, false, physical, 0);
        self.apply_theme_action(action);
    }

    /// `ThemeClass::Stop(fade=1)`: Theme ramps its own stream to silence and
    /// Theme AI stops it when the ramp ends.
    pub(crate) fn fade_out_theme(&mut self, wall_ms: u64) {
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action = self.theme.stop(gates, true, physical, wall_ms);
        self.apply_theme_action(action);
    }

    /// The audio master (`[0x0087E758]`), one multiplier over the music and
    /// sound outputs; the scenario exits fade it to zero and restore it.
    pub(crate) fn set_master_output_scale(&mut self, scale: f64) {
        if let Some(player) = self.music_player.as_mut() {
            player.set_output_scale(scale);
        }
        if let Some(player) = self.sfx_player.as_mut() {
            player.set_output_scale(scale);
        }
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
    fn common_audio_service_owns_the_gate_without_an_output_device() {
        let assets = empty_assets("poll-gate");
        let mut runtime = AppAudioRuntime {
            theme: ThemeRuntime::default(),
            service_clock: AudioServiceClock::default(),
            music_player: None,
            sfx_player: None,
            launcher_audio_available: true,
            theme_startup_suppressed: false,
        };
        let mut frontend = crate::sim::rng::SimRng::new(1);
        let mut main = crate::app::state::process_main_draws(None, &mut frontend);
        runtime.service_audio(100, false, Some(&assets), None, &mut main);
        assert!(!runtime.service_clock.admit(100));
        runtime.service_audio(133, false, Some(&assets), None, &mut main);
        assert!(
            runtime.service_clock.admit(134),
            "the denied call did not reset the gate"
        );
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
