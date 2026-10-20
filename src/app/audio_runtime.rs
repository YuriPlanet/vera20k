//! Process-wide audio output and Theme runtime. Immutable SFX/EVA definitions
//! and the selected sample index belong to `ProcessAssets`; match event queues
//! belong to `MatchAudioState`. Output state survives scenario transitions.

use crate::assets::asset_manager::AssetManager;
use crate::audio::arbiter::{AudioServiceClock, VolumeInterp};
use crate::audio::music::MusicPlayer;
use crate::audio::sfx::SfxPlayer;
use crate::audio::theme::{
    MusicOutputState, PreparedTrack, ThemeAction, ThemeAllowContext, ThemeGates, ThemeRuntime,
};
use crate::sim::trigger_runtime::ThemeRequest;

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
        && let Some(output) = output
        && !output.submit(prepared)
    {
        // gamemd ignores PlayFile's return and still writes logical active.
        log::warn!("Physical music submission failed after Theme admission");
    }
}

pub(crate) struct AppAudioRuntime {
    pub(crate) movie_audio: MovieAudioHold,
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

///5BF580/5BF530's independent boolean and existing407170 interps. The
///lifecycle master remains separate, so exit fades cannot be overwritten by
///a movie finishing. Native stream-list/device callbacks are represented by
///rodio's output pause plus this stream gain; no guessed completion timeout.
pub(crate) struct MovieAudioHold {
    held: bool,
    stream_gain: VolumeInterp,
    master_gain: VolumeInterp,
    lifecycle_scale: f64,
}
impl Default for MovieAudioHold {
    fn default() -> Self {
        Self {
            held: false,
            stream_gain: VolumeInterp::new(0x4000, 0),
            master_gain: VolumeInterp::new(0x4000, 0),
            lifecycle_scale: 1.0,
        }
    }
}

/// gamemd-derived: Theme admission and logical/physical command ordering come
/// from `ThemeClass` AI `0x007209D0`, Next `0x00720A80`, Queue `0x00720B20`,
/// Play `0x00720BB0`, and Stop `0x00720EA0`. Their shared device gate calls
/// `FUN_00407000` (`DAT_0087E728 != 0`) alongside Theme initialization
/// `DAT_00A8EC74 != 0` and startup suppression `DAT_00A8ED64 == 0`.
impl AppAudioRuntime {
    pub(crate) fn movie_mixer(&self) -> Option<&rodio::mixer::Mixer> {
        self.music_player
            .as_ref()
            .map(MusicPlayer::mixer)
            .or_else(|| self.sfx_player.as_ref().map(SfxPlayer::mixer))
    }
    ///408390 asks existing stream interps to reach silence before admission.
    ///Rust's physical stream gain is a substitute for the DirectSound list
    ///cursor predicate408410, not a claim about hardware completion timing.
    pub(crate) fn prepare_movie_streams(&mut self, wall_ms: u64) {
        if self.music_player.is_some() {
            self.movie_audio.stream_gain.set_target(0, wall_ms);
        } else {
            self.movie_audio.stream_gain.set_target_immediate(0);
            self.movie_audio.stream_gain.tick(wall_ms);
        }
    }
    pub(crate) fn movie_streams_ready(&self) -> bool {
        self.movie_audio.stream_gain.value() == 0
    }
    pub(crate) fn set_movie_audio_held(&mut self, held: bool, wall_ms: u64) {
        if held == self.movie_audio.held {
            return;
        }
        self.movie_audio.held = held;
        self.movie_audio
            .master_gain
            .set_target(if held { 0x2000 } else { 0x4000 }, wall_ms);
        //5BF580 and full-screen5BEDFB..5BEE38 target both native
        //VolumeInterp objects. Do not leave Radar's prelude stream at0.
        self.movie_audio
            .stream_gain
            .set_target(if held { 0x2000 } else { 0x4000 }, wall_ms);
        if let Some(sfx) = self.sfx_player.as_mut() {
            sfx.set_movie_held(held, wall_ms);
        }
        if let Some(music) = self.music_player.as_mut() {
            if held {
                music.pause_output();
            } else {
                music.resume_output();
            }
        }
    }
    ///Clear5BF450 releases even a prelude that has not acquired the hold yet.
    pub(crate) fn retire_movie_audio(&mut self, wall_ms: u64) {
        self.set_movie_audio_held(false, wall_ms);
        self.movie_audio.stream_gain.set_target(0x4000, wall_ms);
    }
    fn service_movie_gain(&mut self, wall_ms: u64) {
        self.movie_audio.stream_gain.tick(wall_ms);
        self.movie_audio.master_gain.tick(wall_ms);
        self.apply_output_scales();
    }
    fn apply_output_scales(&mut self) {
        let master = self.movie_audio.lifecycle_scale
            * f64::from(self.movie_audio.master_gain.value())
            / 16384.0;
        if let Some(player) = self.music_player.as_mut() {
            player.set_output_scale(
                master * f64::from(self.movie_audio.stream_gain.value()) / 16384.0,
            );
        }
        if let Some(player) = self.sfx_player.as_mut() {
            player.set_output_scale(master);
        }
    }
    /// `OptionsClass::Set_Score_Volume` at `0x005FA4A0` publishes the stored
    /// Options f32 to the stream first, then Theme's `+0xC` gate. Theme owns its
    /// native quantization; the player's safe 0..1 gain never feeds back into
    /// that logical state or the persisted preference.
    pub(crate) fn set_score_volume(&mut self, value: f32) {
        if let Some(player) = self.music_player.as_mut() {
            player.set_volume(f64::from(value));
        }
        self.theme.set_score_volume(value);
    }

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
    /// call. Each consumer retains draws on the same Main886B88 cell.
    pub(crate) fn service_audio(
        &mut self,
        wall_ms: u64,
        paused: bool,
        assets: Option<&AssetManager>,
        catalog: Option<&crate::app::process_assets::ProcessAudioCatalog>,
        main: &mut crate::sim::rng::MainRngDraws,
    ) {
        self.service_movie_gain(wall_ms);
        let retained = main.clone();
        let mut draw_main =
            crate::audio::sfx::PlaybackDraws::new(move |low, high| retained.ranged(low, high));
        if let Some(sfx) = self.sfx_player.as_mut() {
            sfx.set_paused(paused, wall_ms);
            sfx.service_device_outputs(wall_ms);
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
        self.service_theme(assets, wall_ms, &mut |low, high| main.ranged(low, high));
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

    /// Start_Scenario tail after shared `bind_match_theme_context`: `Stop(1)` /
    /// `Queue_Song([Basic] Theme)`. Later AI borrows the installed Main stream.
    pub(crate) fn request_scenario_theme(
        &mut self,
        requested_section: Option<&str>,
        assets: &AssetManager,
        wall_ms: u64,
    ) {
        self.theme.initialize_catalog(assets);
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action =
            self.theme
                .request_scenario_theme(requested_section, assets, gates, physical, wall_ms);
        self.apply_theme_action(action);
    }

    pub(crate) fn restore_saved_theme(
        &mut self,
        saved: [i32; 2],
        in_game_music: bool,
        assets: &AssetManager,
        wall_ms: u64,
    ) {
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let output = &mut self.music_player;
        self.theme.restore_saved_slots(
            saved,
            in_game_music,
            assets,
            gates,
            physical,
            wall_ms,
            |action| {
                apply_theme_action_to_output(output.as_mut(), action);
                output
                    .as_ref()
                    .map_or(MusicOutputState::Unavailable, MusicPlayer::state)
            },
        );
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

    /// `Queue_Song @ 0x00720B20`; Theme AI starts it. Named callers use
    /// the same From_Name721210 owner as type8 map actions; Action20 can
    /// also supply the native signed index/sentinels directly.
    pub(crate) fn queue_theme(&mut self, request: &ThemeRequest, wall_ms: u64) {
        let index = match request {
            ThemeRequest::Name(track) => self.theme.from_name(track),
            ThemeRequest::Index(index) => *index,
        };
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

    /// The lobby-music rule of `0x0077E110` on its own.
    pub(crate) fn apply_wol_lobby_music_rule(&mut self, lobby_music: bool, wall_ms: u64) {
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action = self
            .theme
            .wol_lobby_music_rule(lobby_music, gates, physical, wall_ms);
        self.apply_theme_action(action);
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
        self.movie_audio.lifecycle_scale = scale;
        self.apply_output_scales();
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

/// Bind Theme's Is_Allowed context from the incoming world on either fresh
/// start or saved load. Native reads live House/Side state; the Rust projection
/// must not keep the previous map's local side or registry indices.
pub(crate) fn bind_match_theme_context(state: &mut crate::app::AppState) {
    if let Some(assets) = state.process_assets.manager() {
        state.audio.initialize_theme(assets);
    }
    let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
        return;
    };
    let simulation = &runtime.simulation;
    let local_side = state.match_state.local_player_owner().and_then(|owner| {
        crate::sim::house_state::house_state_for_owner(
            simulation.houses(),
            owner,
            simulation.interner(),
        )
        .map(|house| i32::from(house.side_index()))
    });
    state.audio.theme.begin_scenario(
        ThemeAllowContext {
            local_side,
            campaign_scenario: simulation.session().campaign_mission_counter(),
        },
        |name| {
            runtime
                .resources
                .rules
                .side_index(name)
                .map(|index| i32::from(index.0))
        },
    );
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

    fn empty_assets(label: &str) -> AssetManager {
        let dir = std::env::temp_dir().join(format!(
            "vera20k-audio-runtime-{}-{label}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("test asset dir");
        AssetManager::from_loose_root_for_test(&dir)
    }

    /// The app setter must reach Theme even when the physical music device
    /// is absent. Expected slots come from the executed native Load/AI rows;
    /// no waveform decoding or hardware output is part of this check.
    #[test]
    fn score_volume_setter_controls_saved_theme_without_music_output() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/sound_theme_metadata.json",
        ))
        .unwrap();
        let root =
            std::env::temp_dir().join(format!("vera20k-app-score-volume-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut ini = String::from("[Themes]\n");
        for index in 0..8 {
            ini.push_str(&format!("{index}=Track{index}\n"));
        }
        for index in 0..8 {
            ini.push_str(&format!("[Track{index}]\nSound=Track{index}\n"));
        }
        std::fs::write(root.join("thememd.ini"), ini).unwrap();
        let assets = AssetManager::from_loose_root_for_test(&root);
        for volume in [0, 1, 255] {
            let native = corpus["ai_volume_cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["input"]["volume"] == volume)
                .unwrap();
            let options = corpus["score_volume_cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["theme_volume"] == volume)
                .unwrap();
            let mut runtime = AppAudioRuntime {
                theme: ThemeRuntime::default(),
                service_clock: AudioServiceClock::default(),
                music_player: None,
                sfx_player: None,
                launcher_audio_available: true,
                theme_startup_suppressed: false,
                movie_audio: Default::default(),
            };
            runtime.initialize_theme(&assets);
            let stored = f32::from_bits(options["stored_f32_bits"].as_u64().unwrap() as u32);
            runtime.set_score_volume(stored);
            runtime.restore_saved_theme([6, 7], true, &assets, 2_000);
            let actual = runtime.theme.slots();
            assert_eq!(
                serde_json::json!([actual.active, actual.retained, actual.pending]),
                native["loaded_slots"],
                "volume {volume} loaded slots"
            );
            runtime.service_theme(&assets, 2_001, &mut |_, _| {
                panic!("native fixed-pending AI consumed no Main draws")
            });
            let actual = runtime.theme.slots();
            assert_eq!(
                serde_json::json!([actual.active, actual.retained, actual.pending]),
                native["after_ai"]["slots"],
                "volume {volume} AI slots"
            );
        }
        std::fs::remove_dir_all(root).unwrap();
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
            movie_audio: Default::default(),
        };
        let mut frontend = crate::sim::rng::MainRng::new(1);
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
    fn shared_movie_hold_targets_both_interps_and_keeps_the_exit_master() {
        let mut runtime = AppAudioRuntime {
            theme: ThemeRuntime::default(),
            service_clock: AudioServiceClock::default(),
            music_player: None,
            sfx_player: None,
            launcher_audio_available: false,
            theme_startup_suppressed: false,
            movie_audio: Default::default(),
        };
        runtime.prepare_movie_streams(0);
        runtime.set_master_output_scale(0.25);
        runtime.set_movie_audio_held(true, 100);
        assert_eq!(runtime.movie_audio.master_gain.target_value(), 0x2000);
        assert_eq!(
            runtime.movie_audio.stream_gain.target_value(),
            0x2000,
            "5BF580/5BEDFB targets stream87E750 as well as master87E758"
        );
        runtime.set_movie_audio_held(true, 200);
        assert_eq!(runtime.movie_audio.lifecycle_scale, 0.25);
        runtime.retire_movie_audio(250);
        runtime.retire_movie_audio(300);
        assert_eq!(runtime.movie_audio.master_gain.target_value(), 0x4000);
        assert_eq!(runtime.movie_audio.stream_gain.target_value(), 0x4000);
        assert_eq!(
            runtime.movie_audio.lifecycle_scale, 0.25,
            "movie retirement cannot cancel a scenario-exit fade"
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
