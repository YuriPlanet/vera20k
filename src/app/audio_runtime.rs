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
    fn fade_stream_out(&mut self, wall_ms: u64);
    fn stop(&mut self);
    fn submit(&mut self, prepared: PreparedTrack) -> bool;
}

impl ThemeMusicOutput for MusicPlayer {
    fn fade_stream_out(&mut self, wall_ms: u64) {
        MusicPlayer::fade_stream_out(self, wall_ms);
    }

    fn stop(&mut self) {
        MusicPlayer::stop(self);
    }

    fn submit(&mut self, prepared: PreparedTrack) -> bool {
        MusicPlayer::submit(self, prepared)
    }
}

/// Native order: a completed fade's Stop, PlayFile, then the new fade's
/// SetTarget, which acts on the stream just started.
fn apply_theme_action_to_output(
    mut output: Option<&mut impl ThemeMusicOutput>,
    action: ThemeAction,
    wall_ms: u64,
) {
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
    if action.fade_out
        && let Some(output) = output
    {
        output.fade_stream_out(wall_ms);
    }
}

pub(crate) struct AppAudioRuntime {
    pub(crate) master_groups: AudioMasterGroups,
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

/// The process volume groups that only lifecycle code writes, with the
/// movie hold flag `0x00ABF35C`.
///
/// gamemd-derived (`VolumeInterp` objects created by `AudioSystem__Init`,
/// `0x00406B92..0x00406BB7`):
/// - `sound_master` is `[0x0087E758]`, `ch+0x9C` of every Voc channel only
///   (`0x0040571E`, `0x00405B9C`). The movie hold targets `0x2000`
///   (`0x005BF598`, full-screen `0x005BEE17`) and its release `0x4000`
///   (`0x005BF548`); `GameExit__Victory 0x00685670`, `GameExit__Defeat
///   0x00685DC0` and `0x00686570` glide it to zero (`0x00407070` at
///   `0x00685715`) and restore it immediately (`0x006857A9`), as
///   `Clear_Scene` does (`0x0068521F`).
/// - `eva_master` is `[0x0087E750]`, `ch+0x9C` of the EVA stream only
///   (`VoxClass::Init 0x00752327`); only the movie hold and its release
///   write it (`0x005BF5A8`, `0x005BF558`, full-screen `0x005BEE27`).
///
/// Each stream player keeps its own `+0x10` interp (`MusicPlayer`, and the
/// EVA and taunt streams in `SfxPlayer`); the radar-movie pre-fade
/// `0x00408390` (called only at `0x0065799B`) and every hold release
/// (`StreamPlayers__RestoreVolumeTargets 0x004083D0`) walk them all.
///
/// Neither master reaches the Theme stream, whose groups are Score and
/// `[0x0087E754]` (`0x00720743..0x0072075A`).
pub(crate) struct AudioMasterGroups {
    held: bool,
    /// `[0x0083D834]`: the effects-master target an in-game modal saved.
    modal_saved_target: i32,
    /// Full-screen `Play_Movie` also suspended the sound events
    /// (`0x00406EA0`).
    sounds_suspended: bool,
    sound_master: VolumeInterp,
    eva_master: VolumeInterp,
}
impl Default for AudioMasterGroups {
    fn default() -> Self {
        Self {
            held: false,
            modal_saved_target: 0x4000,
            sounds_suspended: false,
            sound_master: VolumeInterp::new(0x4000, 0),
            eva_master: VolumeInterp::new(0x4000, 0),
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
    /// `0x00408390`: glide every stream player's own interp to silence
    /// before the radar movie is admitted.
    pub(crate) fn prepare_movie_streams(&mut self, wall_ms: u64) {
        if let Some(music) = self.music_player.as_mut() {
            music.fade_stream_out(wall_ms);
        }
        if let Some(sfx) = self.sfx_player.as_mut() {
            sfx.fade_streams_out(wall_ms);
        }
    }
    /// `0x00408410`: every playing stream's own interp has reached its
    /// target.
    pub(crate) fn movie_streams_ready(&self) -> bool {
        self.music_player
            .as_ref()
            .is_none_or(|music| music.state() != MusicOutputState::Ramping)
            && self
                .sfx_player
                .as_ref()
                .is_none_or(SfxPlayer::streams_settled)
    }
    /// `Audio__PauseForMovie @ 0x005BF580` / `Audio__ResumeAfterMovie @
    /// 0x005BF530`: pause EVA and every stream player and glide both masters
    /// to half (`0x2000`) and back. Voc sounds keep playing. Only a release
    /// of a taken hold restores the stream interps (`0x004083D0`).
    pub(crate) fn set_movie_audio_held(&mut self, held: bool, wall_ms: u64) {
        if held == self.master_groups.held {
            return;
        }
        self.master_groups.held = held;
        let target = if held { 0x2000 } else { 0x4000 };
        self.master_groups.sound_master.set_target(target, wall_ms);
        self.master_groups.eva_master.set_target(target, wall_ms);
        if let Some(sfx) = self.sfx_player.as_mut() {
            sfx.set_movie_held(held, wall_ms);
        }
        if let Some(music) = self.music_player.as_mut() {
            if held {
                music.pause_output();
            } else {
                music.resume_output();
                music.restore_stream_volume(wall_ms);
            }
        }
        if !held && let Some(sfx) = self.sfx_player.as_mut() {
            sfx.restore_stream_volumes(wall_ms);
        }
    }
    /// Full-screen `Play_Movie` alone also suspends the sound events
    /// (`0x00406EA0` at `0x005BEE3E` after the hold; `0x00406EC0` at
    /// `0x005BEE62` before its release).
    pub(crate) fn set_movie_sounds_suspended(&mut self, suspended: bool, wall_ms: u64) {
        self.master_groups.sounds_suspended = suspended;
        if let Some(sfx) = self.sfx_player.as_mut() {
            sfx.set_movie_sounds_suspended(suspended, wall_ms);
        }
    }
    /// `Audio__ResumeAfterMovieAndCleanup @ 0x005BF450` restores only inside
    /// its hold-flag branch, so a pre-fade that never took the hold leaves
    /// the streams silent until their next `PlayFile`.
    pub(crate) fn retire_movie_audio(&mut self, wall_ms: u64) {
        if self.master_groups.sounds_suspended {
            self.set_movie_sounds_suspended(false, wall_ms);
        }
        self.set_movie_audio_held(false, wall_ms);
    }

    /// Whether the shared movie lifecycle has released both of its credits.
    pub(crate) fn movie_audio_released(&self) -> bool {
        !self.master_groups.held && !self.master_groups.sounds_suspended
    }
    /// `StateMachine__EnterPause @ 0x00683EB0`: an in-game modal saves the
    /// effects master's target (`0x00683EBC` -> `0x004072B0`) and snaps it
    /// to full (`0x00683ED1` -> `0x00407150`), so its sounds play at full
    /// volume even under a radar transmission's half-volume hold.
    pub(crate) fn enter_state_machine_pause(&mut self) {
        let groups = &mut self.master_groups;
        groups.modal_saved_target = groups.sound_master.target_value();
        groups.sound_master.set_target_immediate(0x4000);
    }
    /// `StateMachine__ExitPause @ 0x00683FB0` snaps the master back to the
    /// saved target and resets the save (`0x0068402B..0x00684037`).
    pub(crate) fn exit_state_machine_pause(&mut self) {
        let groups = &mut self.master_groups;
        let saved = std::mem::replace(&mut groups.modal_saved_target, 0x4000);
        groups.sound_master.set_target_immediate(saved);
    }
    fn service_master_groups(&mut self, wall_ms: u64) {
        if let Some(music) = self.music_player.as_mut() {
            music.tick_stream_volume(wall_ms);
        }
        self.master_groups.sound_master.tick(wall_ms);
        self.master_groups.eva_master.tick(wall_ms);
        self.apply_output_scales();
    }
    fn apply_output_scales(&mut self) {
        let groups = &self.master_groups;
        if let Some(player) = self.sfx_player.as_mut() {
            player.set_master_groups(groups.sound_master.value(), groups.eva_master.value());
        }
    }
    /// `OptionsClass::Set_Sound_Volume @ 0x005FA510`'s output: the effects
    /// volume every SFX channel scales by. The profile write is the caller's.
    pub(crate) fn set_sound_volume(&mut self, volume: f64) {
        if let Some(player) = self.sfx_player.as_mut() {
            player.set_sound_volume(volume);
        }
    }

    /// `OptionsClass::Set_Voice_Volume @ 0x005FA590`'s output: unit voices and
    /// EVA. The profile write is the caller's.
    pub(crate) fn set_voice_volume(&mut self, volume: f64) {
        if let Some(player) = self.sfx_player.as_mut() {
            player.set_voice_volume(volume);
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

    fn apply_theme_action(&mut self, action: ThemeAction, wall_ms: u64) {
        apply_theme_action_to_output(self.music_player.as_mut(), action, wall_ms);
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
        let action = self.theme.play_menu_theme(assets, gates, physical);
        self.apply_theme_action(action, wall_ms);
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
        self.service_audio_outputs(wall_ms, paused);
        let retained = main.clone();
        let mut draw_main =
            crate::audio::sfx::PlaybackDraws::new(move |low, high| retained.ranged(low, high));
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
                catalog.eva(),
                assets,
                catalog.index(),
                &mut draw_main,
            );
        }
        self.service_theme(assets, wall_ms, &mut |low, high| main.ranged(low, high));
    }

    /// The part of a service pass that is not the rated pump: master fades,
    /// the pause state and the device queues.
    pub(crate) fn service_audio_outputs(&mut self, wall_ms: u64, paused: bool) {
        self.service_master_groups(wall_ms);
        if let Some(sfx) = self.sfx_player.as_mut() {
            sfx.set_paused(paused, wall_ms);
            sfx.service_device_outputs(wall_ms);
        }
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
        let action = self.theme.update(assets, gates, physical, draw_main);
        self.apply_theme_action(action, wall_ms);
    }

    /// `Play_Song(From_Name(track))`.
    pub(crate) fn play_theme(&mut self, track: &str, assets: &AssetManager, wall_ms: u64) -> bool {
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action = self.theme.play_track(track, assets, gates, physical);
        let logical_started = action.start.is_some();
        self.apply_theme_action(action, wall_ms);
        logical_started
    }

    /// Ares loading hook inputs use the existing Theme catalog/stream owner.
    #[cfg(feature = "ares")]
    pub(crate) fn play_ares_loading_theme<'a>(
        &mut self,
        names: impl IntoIterator<Item = Option<&'a str>>,
        assets: &AssetManager,
        wall_ms: u64,
    ) {
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action = self
            .theme
            .play_ares_loading_theme(names, assets, gates, physical);
        self.apply_theme_action(action, wall_ms);
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
        let action = self
            .theme
            .request_scenario_theme(requested_section, assets, gates, physical);
        self.apply_theme_action(action, wall_ms);
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
        self.theme
            .restore_saved_slots(saved, in_game_music, assets, gates, physical, |action| {
                apply_theme_action_to_output(output.as_mut(), action, wall_ms);
                output
                    .as_ref()
                    .map_or(MusicOutputState::Unavailable, MusicPlayer::state)
            });
    }

    /// `Main_Tick @ 0x0055D360` head rule while a scenario runs.
    pub(crate) fn main_tick_theme(&mut self, in_game_music: bool, wall_ms: u64) {
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action = self.theme.main_tick(in_game_music, gates, physical);
        self.apply_theme_action(action, wall_ms);
    }

    pub(crate) fn cancel_scenario_theme_request(&mut self) {
        self.theme.cancel_scenario_theme_request();
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
        let action = self.theme.queue_song(index, gates, physical);
        self.apply_theme_action(action, wall_ms);
    }

    /// `WOL_Main` entry: INTRO gives way to the shuffled lobby music
    /// (`ThemeRuntime::enter_wol_lobby`). Returns the shuffle flag to restore.
    pub(crate) fn enter_wol_lobby_music(&mut self, lobby_music: bool, wall_ms: u64) -> bool {
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let (action, saved_shuffle) = self.theme.enter_wol_lobby(lobby_music, gates, physical);
        self.apply_theme_action(action, wall_ms);
        saved_shuffle
    }

    /// The lobby-music rule of `0x0077E110` on its own.
    pub(crate) fn apply_wol_lobby_music_rule(&mut self, lobby_music: bool, wall_ms: u64) {
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action = self
            .theme
            .wol_lobby_music_rule(lobby_music, gates, physical);
        self.apply_theme_action(action, wall_ms);
    }

    /// `WOL_Main` exit: the saved shuffle flag returns.
    pub(crate) fn leave_wol_lobby_music(&mut self, saved_shuffle: bool) {
        self.theme.leave_wol_lobby(saved_shuffle);
    }

    /// `ThemeClass::Stop(fade=0)`.
    pub(crate) fn stop_theme(&mut self) {
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action = self.theme.stop(gates, false, physical);
        // Stop(0) only ever stops the stream; no fade needs a time.
        debug_assert!(!action.fade_out && action.start.is_none());
        self.apply_theme_action(action, 0);
    }

    /// GameExit Victory685670/Defeat685DC0/Abort686570 clear speech and
    /// kind-1 sound channels (406E80), then restore master87E758. Theme streams
    /// are kind0x1000 (4078C1 -> 4036D8 -> 402449), so this cleanup preserves
    /// them. Abort already requested its own Theme fade; destination owners
    /// select SCORE, INTRO or the next scenario. Source: original instructions.
    pub(crate) fn apply_scenario_exit_tick(
        &mut self,
        tick: &crate::app::match_runtime::scenario_exit::ScenarioExitTick,
        assets: Option<&AssetManager>,
        wall_ms: u64,
    ) {
        if tick.fade_sound_master {
            self.master_groups.sound_master.set_target(0, wall_ms);
            self.service_master_groups(wall_ms);
        }
        if tick.clear_sounds {
            if let Some(player) = self.sfx_player.as_mut() {
                player.stop_all();
            }
            self.restore_sound_master();
        }
        // Score dialogs explicitly select their theme after the master is
        // restored: 5C9B10 (multiplayer), original gamemd.exe, or the side's
        // name under the hook at 0x5C9B75. Campaign WM_INIT selects its theme
        // after the Win movie in open_campaign_score_page.
        if let Some(crate::app::match_runtime::scenario_exit::ScenarioExitAudioAction::PlayTheme(
            theme,
        )) = &tick.after_cleanup
            && let Some(assets) = assets
        {
            let _ = self.play_theme(&theme, assets, wall_ms);
        }
    }

    /// `ThemeClass::Stop(fade=1)`: Theme ramps its own stream to silence and
    /// Theme AI stops it when the ramp ends.
    pub(crate) fn fade_out_theme(&mut self, wall_ms: u64) {
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action = self.theme.stop(gates, true, physical);
        self.apply_theme_action(action, wall_ms);
    }

    /// `ThemeClass::Still_Playing @ 0x00720FD0`, which the quit wait polls
    /// (`0x0052E79C`, `0x0052E7DD`): a fade whose ramp has ended stops the
    /// stream and reads as not playing.
    pub(crate) fn theme_still_playing(&mut self) -> bool {
        let (playing, action) = self.theme.still_playing(self.music_output_state());
        // Still_Playing only ever stops the stream; no start needs a time.
        self.apply_theme_action(action, 0);
        playing
    }

    /// Whether the effects master is still gliding; the scenario exits wait on
    /// it (`VolumeInterp__WaitUntilDone @ 0x004070B0`).
    pub(crate) fn sound_master_moving(&self) -> bool {
        self.master_groups.sound_master.moving()
    }

    /// `SetTargetImmediate([0x0087E758], 0x4000)`: the scenario exits after
    /// their cleanup (`0x006857A9`), `Clear_Scene` (`0x0068521F`) and the
    /// ordinary load cleanup (`0x0069BB40`).
    pub(crate) fn restore_sound_master(&mut self) {
        // Clear_Scene also resets the modal save first (`0x00685219`), so a
        // load from the in-game menu leaves the master full when it closes.
        self.master_groups.modal_saved_target = 0x4000;
        self.master_groups.sound_master.set_target_immediate(0x4000);
        self.master_groups.sound_master.tick(0);
        self.apply_output_scales();
    }

    /// B8 user commands preserve Theme fade/queue ordering and output ownership.
    pub(crate) fn play_sound_selection(&mut self, index: i32, wall_ms: u64) {
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action = self.theme.play_selection(index, gates, physical);
        self.apply_theme_action(action, wall_ms);
    }
    pub(crate) fn stop_sound_selection(&mut self, wall_ms: u64) {
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action = self.theme.stop_selection(gates, physical);
        self.apply_theme_action(action, wall_ms);
    }
    /// Launcher ScoreVolume zero (`0x0055FAA0`): `Queue(cur)` then `Stop(0)`.
    pub(crate) fn queue_then_stop_score_zero(&mut self, wall_ms: u64) {
        let gates = self.theme_gates();
        let physical = self.music_output_state();
        let action = self.theme.queue_then_stop_score_zero(gates, physical);
        self.apply_theme_action(action, wall_ms);
    }
}

/// InitSideMixFiles534FAC / LoadContent67E761 bind saved Scenario.Side,
/// not the separately restored current House. Only successful adoption calls
/// here; reset retains the outgoing cache until this explicit replacement.
pub(crate) fn bind_match_eva_context(state: &mut crate::app::AppState) {
    if let (Some(runtime), Some(sfx)) = (
        state.match_state.sim_runtime.as_ref(),
        state.audio.sfx_player.as_mut(),
    ) {
        sfx.set_eva_side(
            runtime.simulation.session().scenario_side(),
            &runtime.resources.rules,
        );
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
        FadeOut(u64),
        Stop,
        Submit(String),
    }

    struct FakeOutput {
        calls: Vec<OutputCall>,
        submit_succeeds: bool,
    }

    impl ThemeMusicOutput for FakeOutput {
        fn fade_stream_out(&mut self, wall_ms: u64) {
            self.calls.push(OutputCall::FadeOut(wall_ms));
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
                master_groups: Default::default(),
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
            master_groups: Default::default(),
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
    fn defeat_audio_cleanup_preserves_theme_until_the_next_destination_requests_one() {
        use crate::app::match_runtime::scenario_exit::{
            ScenarioExitCascade, ScenarioExitDestination,
        };
        let root =
            std::env::temp_dir().join(format!("vera20k-defeat-theme-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("thememd.ini"),
            "[Themes]\n0=Battle\n1=INTRO\n2=SCORE\n[Battle]\nSound=Battle\n[INTRO]\nSound=Drok\nNormal=no\n[SCORE]\nSound=ScoreX\nNormal=no\n",
        )
        .unwrap();
        // One stereo PCM frame is a transport fixture, not a retail sound.
        std::fs::write(
            root.join("Battle.wav"),
            b"RIFF\x28\0\0\0WAVEfmt \x10\0\0\0\x01\0\x02\0\x22\x56\0\0\x88\x58\x01\0\x04\0\x10\0data\x04\0\0\0\0\0\0\0",
        )
        .unwrap();
        let assets = AssetManager::from_loose_root_for_test(&root);
        let mut runtime = AppAudioRuntime {
            theme: ThemeRuntime::default(),
            service_clock: AudioServiceClock::default(),
            music_player: None,
            sfx_player: None,
            launcher_audio_available: true,
            theme_startup_suppressed: false,
            master_groups: Default::default(),
        };
        runtime.initialize_theme(&assets);
        // Restore through the production Theme owner with an admitted stream;
        // a device-less restore leaves active=-1 and cannot expose this bug.
        let gates = runtime.theme_gates();
        runtime.theme.restore_saved_slots(
            [0, -1],
            true,
            &assets,
            gates,
            MusicOutputState::Idle,
            |_| MusicOutputState::Playing,
        );
        assert_eq!(runtime.theme.slots().active, 0);
        let before = runtime.theme.saved_slots();
        assert_eq!(before, [0, -1]);
        let mut exit = ScenarioExitCascade::start(0, ScenarioExitDestination::CampaignDefeat);
        runtime.apply_scenario_exit_tick(&exit.tick(0, false, false), Some(&assets), 0);
        let moving = runtime.sound_master_moving();
        runtime.apply_scenario_exit_tick(&exit.tick(500, moving, false), Some(&assets), 500);
        // GameExit__Defeat glides only the effects master (0x00685715); the
        // Theme stream keeps playing at full.
        assert_eq!(runtime.master_groups.sound_master.value(), 0x2000);
        assert_eq!(runtime.theme.saved_slots(), before);
        let moving = runtime.sound_master_moving();
        runtime.apply_scenario_exit_tick(&exit.tick(1_000, moving, false), Some(&assets), 1_000);
        assert_eq!(runtime.master_groups.sound_master.value(), 0x4000);
        assert_eq!(
            runtime.theme.saved_slots(),
            before,
            "GameExit cleanup preserves Theme"
        );
        runtime.maintain_main_menu_theme(&assets, 1_001);
        assert_eq!(
            runtime.theme.current_song(),
            1,
            "PrepareSession selects INTRO on shell return"
        );
        let mut score = ScenarioExitCascade::start(
            2_000,
            ScenarioExitDestination::Score {
                title: String::new(),
                detail: String::new(),
                model: crate::ui::score_shell::ScoreScreenModel::default(),
            },
        );
        runtime.apply_scenario_exit_tick(&score.tick(3_000, false, false), Some(&assets), 3_000);
        runtime.apply_scenario_exit_tick(&score.tick(3_000, false, false), Some(&assets), 3_000);
        assert_eq!(
            runtime.theme.current_song(),
            2,
            "score explicitly selects SCORE"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    /// The quit wait (`0x0052E764..0x0052E7E4`) follows Theme's own 1000 ms
    /// stream fade, whatever the score volume, until the ended ramp stops it.
    #[test]
    fn the_quit_fade_lasts_the_theme_ramp_at_a_low_score_volume() {
        let (mut music, _output) = MusicPlayer::offline_for_test();
        assert!(music.submit(PreparedTrack::decoded(
            "Silence".into(),
            vec![0.0; 22_050 * 2 * 10],
            22_050,
        )));
        let root = std::env::temp_dir().join(format!("vera20k-quit-fade-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("thememd.ini"),
            "[Themes]\n0=Battle\n[Battle]\nSound=Battle\n",
        )
        .unwrap();
        // One stereo PCM frame is a transport fixture, not a retail sound.
        std::fs::write(
            root.join("Battle.wav"),
            b"RIFF\x28\0\0\0WAVEfmt \x10\0\0\0\x01\0\x02\0\x22\x56\0\0\x88\x58\x01\0\x04\0\x10\0data\x04\0\0\0\0\0\0\0",
        )
        .unwrap();
        let assets = AssetManager::from_loose_root_for_test(&root);
        let mut runtime = AppAudioRuntime {
            theme: ThemeRuntime::default(),
            service_clock: AudioServiceClock::default(),
            music_player: Some(music),
            sfx_player: None,
            launcher_audio_available: true,
            theme_startup_suppressed: false,
            master_groups: Default::default(),
        };
        runtime.initialize_theme(&assets);
        let gates = runtime.theme_gates();
        runtime.theme.restore_saved_slots(
            [0, -1],
            true,
            &assets,
            gates,
            MusicOutputState::Idle,
            |_| MusicOutputState::Playing,
        );
        assert_eq!(runtime.theme.slots().active, 0);
        runtime.set_score_volume(0.4);
        runtime.fade_out_theme(0);
        runtime.service_master_groups(500);
        assert!(runtime.theme_still_playing(), "the ramp is half done");
        runtime.service_master_groups(1_001);
        assert!(!runtime.theme_still_playing());
        assert_eq!(runtime.music_output_state(), MusicOutputState::Idle);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// `0x005BF580` halves `[0x0087E758]` and `[0x0087E750]` and leaves the
    /// stream interps where the radar pre-fade (`0x00408390`) put them. Only a
    /// release of a taken hold glides them back (`0x005BF450` and `0x005BF530`
    /// call `0x004083D0` inside their hold-flag branch).
    #[test]
    fn movie_hold_halves_both_masters_and_only_a_taken_hold_restores_streams() {
        let (music, _output) = MusicPlayer::offline_for_test();
        let mut runtime = AppAudioRuntime {
            theme: ThemeRuntime::default(),
            service_clock: AudioServiceClock::default(),
            music_player: Some(music),
            sfx_player: None,
            launcher_audio_available: false,
            theme_startup_suppressed: false,
            master_groups: Default::default(),
        };
        let stream_target = |runtime: &AppAudioRuntime| {
            runtime
                .music_player
                .as_ref()
                .expect("offline music output")
                .stream_volume_target()
        };
        runtime.prepare_movie_streams(0);
        assert!(runtime.movie_streams_ready(), "no stream is playing");
        runtime.retire_movie_audio(50);
        assert_eq!(stream_target(&runtime), 0);
        runtime.set_movie_audio_held(true, 100);
        let groups = &runtime.master_groups;
        assert_eq!(groups.sound_master.target_value(), 0x2000);
        assert_eq!(groups.eva_master.target_value(), 0x2000);
        assert_eq!(stream_target(&runtime), 0);
        runtime.retire_movie_audio(300);
        let groups = &runtime.master_groups;
        assert_eq!(groups.sound_master.target_value(), 0x4000);
        assert_eq!(groups.eva_master.target_value(), 0x4000);
        assert_eq!(stream_target(&runtime), 0x4000);
    }

    /// An in-game modal opened during a radar hold snaps the effects master to
    /// full and closing it snaps it back to half (`0x00683EB0`, `0x00683FB0`).
    #[test]
    fn in_game_modal_restores_full_effects_volume_until_it_closes() {
        let mut runtime = AppAudioRuntime {
            theme: ThemeRuntime::default(),
            service_clock: AudioServiceClock::default(),
            music_player: None,
            sfx_player: None,
            launcher_audio_available: false,
            theme_startup_suppressed: false,
            master_groups: Default::default(),
        };
        runtime.set_movie_audio_held(true, 0);
        runtime.service_master_groups(100);
        assert!(runtime.sound_master_moving());
        runtime.enter_state_machine_pause();
        runtime.service_master_groups(110);
        assert_eq!(runtime.master_groups.sound_master.value(), 0x4000);
        runtime.exit_state_machine_pause();
        runtime.service_master_groups(120);
        assert_eq!(runtime.master_groups.sound_master.value(), 0x2000);
        assert_eq!(runtime.master_groups.modal_saved_target, 0x4000);

        // A load from the menu runs Clear_Scene (`0x00685219`) before the
        // menu closes; the master must come back full, not to the old save.
        runtime.enter_state_machine_pause();
        runtime.retire_movie_audio(130);
        runtime.restore_sound_master();
        runtime.exit_state_machine_pause();
        runtime.service_master_groups(140);
        assert_eq!(runtime.master_groups.sound_master.value(), 0x4000);
    }

    #[test]
    fn post_admission_output_failure_cannot_rewrite_theme_state() {
        let theme = ThemeRuntime::default();
        let before = format!("{:?}", theme);
        let action = ThemeAction {
            stop_output: true,
            fade_out: true,
            start: Some(PreparedTrack::decoded(
                "Drok".into(),
                vec![0.0, 0.0],
                22_050,
            )),
        };
        let mut output = FakeOutput {
            calls: Vec::new(),
            submit_succeeds: false,
        };

        apply_theme_action_to_output(Some(&mut output), action, 7);

        assert_eq!(
            output.calls,
            vec![
                OutputCall::Stop,
                OutputCall::Submit("Drok".into()),
                OutputCall::FadeOut(7),
            ]
        );
        assert_eq!(format!("{:?}", theme), before);
    }
}
