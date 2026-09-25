//! Device-independent ThemeClass state and track preparation.
//!
//! Active Yuri's Revenge keeps Theme data and the logical active/retained/
//! pending slots alive even when its optional stream player is absent.  This
//! module mirrors that ownership: rodio remains in `music`, while the catalog,
//! playlist selection, sentinels, scenario queueing, and lifecycle transitions
//! remain testable without an audio device.
//!
//! gamemd-derived: singleton `g_Theme @ 0x00A83D10`; ctor `0x00720960`,
//! catalog load `0x00720590` (THEMEMD.INI only, string `0x00825D94`), entry
//! reader `0x00720480`, availability scan `0x007207F0`, `From_Name`
//! `0x00721210`, `Is_Allowed` `0x00721140`, `Next_Song` `0x00720A80`,
//! `Queue_Song` `0x00720B20`, `Play_Song` `0x00720BB0`, `Stop` `0x00720EA0`,
//! `AI` `0x007209D0` (audio pump `0x00406F70`, every screen, > 33 ms).

use crate::assets::asset_manager::AssetManager;
use crate::assets::aud_file;
use crate::audio::sfx::decode_wav;
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::sim::rng::SimRng;

/// Theme fade length. Native is rate-based, not duration-based: the stream's
/// own `VolumeInterp` (`stream+0x14 -> +0x10`) is initialised by
/// `StreamPlayer__PlayFile` with `SetTargetImmediate(0x4000)` (`MOV EDX,0x4000`
/// @ `0x00407E7D`); the stream constructor (`FUN_00401000`) builds that
/// interpolator with `FUN_00407100` (rate `0x10624D`, 1000 ms per-tick elapsed
/// cap) and then `FUN_004071A0(range 0x4000, 1000 ms)`, which recomputes the
/// same rate `(0x4000 << 16) / 1000 = 0x10624D` per ms; `0x004080C0` -> `VolumeInterp__SetTarget @
/// 0x00407170` retargets it to 0, and `VolumeInterp__Tick @ 0x004071C0` steps
/// `current += rate * elapsed_ms` toward `target << 16`. Because the stream
/// interpolator always starts at full scale 0x4000 — ScoreVolume lives in a
/// separate interpolator (`0x0087E744`, `OptionsClass::SetScoreVolume @
/// 0x005FA4A0` targets `volume * 0x4000`) that multiplies on top — the fade
/// takes `(0x4000 << 16) / 0x10624D` = 1000.0007 ms for every ScoreVolume.
/// VERA-internal residual: the native integer stepper needs 1001 whole
/// milliseconds to reach zero and clamps a stalled frame's elapsed at 1000 ms;
/// Rust uses a 1000 ms linear ramp on wall time (at most 1 ms difference,
/// inaudible, per fade).
const THEME_FADE_MS: u64 = 1_000;
const THEME_INI_NAME: &str = "thememd.ini";
const MENU_THEME_SECTION: &str = "INTRO";
/// `Next_Song` shuffle rejection budget (`CMP EDI,0x3E8` @ `0x00720AC6`).
const SHUFFLE_TRIES: u32 = 1_000;

/// Native slot sentinels (`g_Theme+0x0/+0x4/+0x8`).
pub(crate) const THEME_NONE: i32 = -1;
pub(crate) const THEME_AUTO: i32 = -2;
pub(crate) const THEME_HOLD: i32 = -3;

/// The physical stream observation supplied to Theme's AI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MusicOutputState {
    /// No music stream object exists (`g_Theme+0x2C == NULL`).
    Unavailable,
    /// A stream object exists but no source is active.
    Idle,
    /// The current source is still playing.
    Playing,
    /// The current source reached its physical end.
    Finished,
}

impl MusicOutputState {
    fn stream_exists(self) -> bool {
        self != Self::Unavailable
    }
}

/// One `[Themes]` entry (native 0x290-byte record, ctor inside `0x00720590`,
/// reader `0x00720480`, scan `0x007207F0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ThemeEntry {
    /// `[Themes]` value = section key (+0x000). `From_Name` matches this only.
    pub(crate) key: String,
    /// `Sound=` with leading `$`/`#` stripped (+0x100); `.WAV` appended at play.
    pub(crate) sound: String,
    /// `Name=` CSF key for the native localized +0x200 display name. B8's
    /// projection resolves it using the process CSF, with a 63-UTF16-unit cap.
    pub(crate) name_key: String,
    /// Whole seconds from native WAV metadata scan `7207F0`; independent of
    /// physical playback length (especially for IMA ADPCM music).
    pub(crate) duration_seconds: u32,
    /// `Scenario=` (+0x280, default 0).
    pub(crate) scenario: i32,
    /// `Normal=` (+0x288, default yes).
    pub(crate) normal: bool,
    /// `Repeat=` (+0x289, default no). Keyed by entry, never by `Sound=` stem.
    pub(crate) repeat: bool,
    /// `Side=` name (+0x28C holds the resolved index; default -1 = any side).
    pub(crate) side_name: Option<String>,
    /// Resolved `Side=` index, -1 when unset. Unknown names natively register
    /// a fresh `SideClass` that never equals the player's side.
    pub(crate) side: i32,
    /// `CCFileClass::IsAvailable(Sound + ".WAV")` (+0x28A).
    pub(crate) available: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThemeSlots {
    /// +0x00 currently playing index.
    pub(crate) active: i32,
    /// +0x04 last requested index (`Next_Song`'s "previous").
    pub(crate) retained: i32,
    /// +0x08 pending index or sentinel.
    pub(crate) pending: i32,
}

impl Default for ThemeSlots {
    fn default() -> Self {
        Self {
            active: THEME_NONE,
            retained: THEME_NONE,
            pending: THEME_NONE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThemeGates {
    pub(crate) launcher_audio_available: bool,
    pub(crate) startup_suppressed: bool,
}

/// Device-independent PCM payload ready for optional physical submission.
#[derive(Debug)]
pub(crate) struct PreparedTrack {
    pub(crate) stem: String,
    pub(crate) samples: Vec<f32>,
    pub(crate) sample_rate: u32,
}

#[derive(Debug, Default)]
pub(crate) struct ThemeAction {
    pub(crate) stop_output: bool,
    pub(crate) start: Option<PreparedTrack>,
    pub(crate) theme_scale: Option<f64>,
}

impl ThemeAction {
    fn then(mut self, next: Self) -> Self {
        self.stop_output |= next.stop_output;
        if next.start.is_some() {
            self.start = next.start;
        }
        if next.theme_scale.is_some() {
            self.theme_scale = next.theme_scale;
        }
        self
    }
}

/// `Is_Allowed` player/scenario context (`g_PlayerPtr->HouseType->+0xBC`,
/// `g_GameMode`, `Scen+0x1254`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ThemeAllowContext {
    /// Local player's side index; `None` when no player exists (shell).
    pub(crate) local_side: Option<i32>,
    /// `Some(scenario number)` only in campaign (`g_GameMode == 0`); skirmish
    /// (`g_GameMode != 0`) skips the `Scenario=` gate.
    pub(crate) campaign_scenario: Option<i32>,
}

/// Process-lifetime logical Theme owner.
#[derive(Debug)]
pub(crate) struct ThemeRuntime {
    catalog_loaded: bool,
    entries: Vec<ThemeEntry>,
    slots: ThemeSlots,
    /// +0x10 `IsScoreRepeat`.
    global_repeat: bool,
    /// +0x11 fading flag plus the interpolator start time.
    fading: bool,
    fade_started_at_ms: Option<u64>,
    /// +0x12 `IsScoreShuffle`.
    shuffle: bool,
    /// Whether Start_Scenario owns the pending slot (scenario reset cancel).
    scenario_owns_pending: bool,
    allow_context: ThemeAllowContext,
    /// VERA-internal: the shuffle draw. Native draws `g_MainRng @ 0x00886B88`
    /// (`MOV ECX,0x886B88` @ `0x00720AB5`), which `Init_Random_Number_System @
    /// 0x0052FC20` seeds from `g_RngSeed` right after `Scen->Random`, and which
    /// sim bodies (`TechnoClass__ReceiveDamage`, `FootClass__AI`,
    /// `HouseClass__Update`) also consume. Drawing it from the wall-clock audio
    /// pump would make sim RNG consumption depend on audio timing, so VERA
    /// keeps a presentation-side copy seeded from the same match seed at
    /// Start_Scenario; the gamemd draw sequence is deliberately not reproduced.
    shuffle_rng: SimRng,
}

impl Default for ThemeRuntime {
    fn default() -> Self {
        // ctor 0x00720960: slots -1, repeat 0, fading 0, shuffle 0.
        Self {
            catalog_loaded: false,
            entries: Vec::new(),
            slots: ThemeSlots::default(),
            global_repeat: false,
            fading: false,
            fade_started_at_ms: None,
            shuffle: false,
            scenario_owns_pending: false,
            allow_context: ThemeAllowContext::default(),
            shuffle_rng: SimRng::new(0),
        }
    }
}

impl ThemeRuntime {
    /// Catalog load `0x00720590` + scan `0x007207F0`, once per process.
    pub(crate) fn initialize_catalog(&mut self, assets: &AssetManager) {
        if self.catalog_loaded {
            return;
        }
        if let Some(bytes) = assets.get_ref(THEME_INI_NAME)
            && let Ok(ini) = IniFile::from_bytes(bytes)
        {
            self.entries = catalog_from_ini(&ini);
        }
        for entry in &mut self.entries {
            let wav = (!entry.sound.is_empty())
                .then(|| assets.get_ref(&format!("{}.wav", entry.sound)))
                .flatten();
            entry.available = wav.is_some();
            entry.duration_seconds = wav
                .and_then(crate::assets::wav_file::WavFile::parse)
                .and_then(|wav| wav.native_duration_seconds())
                .unwrap_or(0);
        }
        self.catalog_loaded = true;
    }

    #[cfg(test)]
    fn with_entries(entries: Vec<ThemeEntry>) -> Self {
        Self {
            catalog_loaded: true,
            entries,
            ..Default::default()
        }
    }

    pub(crate) fn entries(&self) -> &[ThemeEntry] {
        &self.entries
    }

    #[cfg(test)]
    pub(crate) fn slots(&self) -> ThemeSlots {
        self.slots
    }

    /// Native current query: retained (+4), or pending (+8) when retained is
    /// -1. B8 `6B65CE`, `6B67F8`, `6B6918`; launcher zero-volume `55FBE5`.
    pub(crate) fn current_song(&self) -> i32 {
        if self.slots.retained == THEME_NONE {
            self.slots.pending
        } else {
            self.slots.retained
        }
    }

    /// `OptionsClass__ReadFromINI @ 0x005FA620` writes `IsScoreRepeat` to
    /// `0x00A83D20` (@ `0x005FAB1C`) and `IsScoreShuffle` to `0x00A83D22`
    /// (@ `0x005FAB5B`).
    pub(crate) fn set_score_options(&mut self, repeat: bool, shuffle: bool) {
        self.global_repeat = repeat;
        self.shuffle = shuffle;
    }

    /// Start_Scenario-time context: reseed the presentation shuffle stream
    /// from the match seed and pin the local player's side for `Is_Allowed`.
    pub(crate) fn begin_scenario(
        &mut self,
        match_seed: u32,
        context: ThemeAllowContext,
        resolve_side: impl Fn(&str) -> Option<i32>,
    ) {
        self.shuffle_rng = SimRng::new(u64::from(match_seed));
        self.allow_context = context;
        for entry in &mut self.entries {
            entry.side = match entry.side_name.as_deref() {
                None => -1,
                Some(name) => resolve_side(name).unwrap_or(i32::MAX),
            };
        }
    }

    fn admitted(&self, gates: ThemeGates) -> bool {
        self.catalog_loaded && gates.launcher_audio_available && !gates.startup_suppressed
    }

    /// `ThemeClass__From_Name @ 0x00721210`: case-insensitive section-key
    /// match (`0x007C8D20`); -1 for null, empty, or unknown names.
    pub(crate) fn from_name(&self, name: &str) -> i32 {
        if name.is_empty() {
            return THEME_NONE;
        }
        self.entries
            .iter()
            .position(|entry| entry.key.eq_ignore_ascii_case(name))
            .map_or(THEME_NONE, |index| index as i32)
    }

    fn entry(&self, index: i32) -> Option<&ThemeEntry> {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.entries.get(i))
    }

    fn entry_repeats(&self, index: i32) -> bool {
        self.entry(index).is_some_and(|entry| entry.repeat)
    }

    /// `ThemeClass__Is_Allowed @ 0x00721140`.
    pub(crate) fn is_allowed(&self, index: i32) -> bool {
        if index == THEME_HOLD || index == THEME_AUTO {
            return true;
        }
        // `idx >= count` (unsigned compare, so -1 too) is denied.
        let Some(entry) = self.entry(index) else {
            return false;
        };
        if !entry.available || !entry.normal {
            return false;
        }
        if let Some(local_side) = self.allow_context.local_side
            && entry.side != -1
            && local_side != entry.side
        {
            return false;
        }
        if let Some(scenario) = self.allow_context.campaign_scenario
            && scenario < entry.scenario
        {
            return false;
        }
        true
    }

    /// `ThemeClass__Next_Song @ 0x00720A80`.
    fn next_song(&mut self, prev: i32) -> i32 {
        let count = self.entries.len() as i32;
        if prev >= 0 && (self.entry_repeats(prev) || self.global_repeat) {
            return prev;
        }
        if self.shuffle {
            let mut tries = 0u32;
            let mut draw;
            loop {
                draw = self.shuffle_rng.next_range_i32_inclusive(0, count - 1);
                tries += 1;
                if tries >= SHUFFLE_TRIES {
                    break;
                }
                if draw != prev && self.is_allowed(draw) {
                    break;
                }
            }
            return if tries == SHUFFLE_TRIES { 0 } else { draw };
        }
        let mut index = prev;
        let mut probes = count + 1;
        loop {
            index += 1;
            if index >= count {
                index = 0;
            }
            probes -= 1;
            if probes == 0 {
                return 0;
            }
            if self.is_allowed(index) {
                return index;
            }
        }
    }

    pub(crate) fn play_track(
        &mut self,
        track_name: &str,
        assets: &AssetManager,
        gates: ThemeGates,
        physical: MusicOutputState,
        wall_ms: u64,
    ) -> ThemeAction {
        self.initialize_catalog(assets);
        let index = self.from_name(track_name);
        let mut prepare = |stem: &str| prepare_track(stem, assets);
        self.play_song(index, gates, physical, wall_ms, &mut prepare)
    }

    /// `Main__PrepareSession @ 0x0052D9A0`: `Play(From_Name("INTRO"))`.
    pub(crate) fn play_menu_theme(
        &mut self,
        assets: &AssetManager,
        gates: ThemeGates,
        physical: MusicOutputState,
        wall_ms: u64,
    ) -> ThemeAction {
        self.play_track(MENU_THEME_SECTION, assets, gates, physical, wall_ms)
    }

    /// `ScenarioClass__Start_Scenario @ 0x00683AB0` after `Read_Scenario`:
    /// `Scen+0x1C70 == -1` (no resolvable `[Basic] Theme=`) → `Stop(fade=1)`;
    /// else `Queue_Song(index)`. `[Basic] Theme=` resolves through
    /// `0x004758F0` = `From_Name` of the read string.
    pub(crate) fn request_scenario_theme(
        &mut self,
        requested_section: Option<&str>,
        assets: &AssetManager,
        gates: ThemeGates,
        physical: MusicOutputState,
        wall_ms: u64,
    ) -> ThemeAction {
        self.initialize_catalog(assets);
        let index = requested_section
            .map(str::trim)
            .map_or(THEME_NONE, |section| self.from_name(section));
        let admitted = self.admitted(gates);
        let action = if index == THEME_NONE {
            self.stop(gates, true, physical, wall_ms)
        } else {
            self.queue_song(index, gates, physical, wall_ms)
        };
        self.scenario_owns_pending = admitted;
        action
    }

    /// `Main_Tick @ 0x0055D360` head while a scenario runs: `cur = retained,
    /// or pending when retained == -1`; `InGameMusic == 0 && cur != -3` →
    /// `Stop(1)` + `Queue(-3)`; else `cur == -1` → `Queue(-2)`.
    pub(crate) fn main_tick(
        &mut self,
        in_game_music: bool,
        gates: ThemeGates,
        physical: MusicOutputState,
        wall_ms: u64,
    ) -> ThemeAction {
        if !self.admitted(gates) || !physical.stream_exists() {
            return ThemeAction::default();
        }
        let current = self.current_song();
        if !in_game_music {
            if current != THEME_HOLD {
                let stopped = self.stop(gates, true, physical, wall_ms);
                return stopped.then(self.queue_song(THEME_HOLD, gates, physical, wall_ms));
            }
            ThemeAction::default()
        } else if current == THEME_NONE {
            self.queue_song(THEME_AUTO, gates, physical, wall_ms)
        } else {
            ThemeAction::default()
        }
    }

    /// `WOL_Main` entry (`0x0077B2D7..0x0077B31D`): with INTRO current,
    /// `Queue(-3)`; the score shuffle flag `[0x00A83D22]` is saved and forced
    /// on; then the lobby-music rule `0x0077E110`: with `LobMusic`, when no
    /// stream plays (`Still_Playing` `0x00720FD0`) or INTRO is still current,
    /// `Stop(0)` and `Queue(-2)` (Theme AI then shuffles the next track);
    /// without it, `Queue(-3)`. Returns the shuffle flag to restore on leaving.
    pub(crate) fn enter_wol_lobby(
        &mut self,
        lobby_music: bool,
        gates: ThemeGates,
        physical: MusicOutputState,
        wall_ms: u64,
    ) -> (ThemeAction, bool) {
        let intro = self.from_name(MENU_THEME_SECTION);
        let mut action = ThemeAction::default();
        if self.current_song() == intro {
            action = action.then(self.queue_song(THEME_HOLD, gates, physical, wall_ms));
        }
        let saved_shuffle = std::mem::replace(&mut self.shuffle, true);
        if lobby_music {
            if !self.still_playing(physical, wall_ms) || self.current_song() == intro {
                action = action.then(self.stop(gates, false, physical, wall_ms));
                action = action.then(self.queue_song(THEME_AUTO, gates, physical, wall_ms));
            }
        } else {
            action = action.then(self.queue_song(THEME_HOLD, gates, physical, wall_ms));
        }
        (action, saved_shuffle)
    }

    /// Every `WOL_Main` exit restores the saved shuffle flag
    /// (`0x0077B549`, `0x0077B56D`, `0x0077B59C`, ...); the menu then plays
    /// INTRO again.
    pub(crate) fn leave_wol_lobby(&mut self, saved_shuffle: bool) {
        self.shuffle = saved_shuffle;
    }

    /// Fade bookkeeping shared by AI/Queue/Play/Stop: when fading and the
    /// interpolator reached its target (`0x004080D0 == 0`), `StreamPlayer__Stop`.
    fn settle_fade(&mut self, wall_ms: u64) -> ThemeAction {
        let mut action = ThemeAction::default();
        if self.fading
            && let Some(started) = self.fade_started_at_ms
            && wall_ms.saturating_sub(started) >= THEME_FADE_MS
        {
            action.stop_output = true;
            action.theme_scale = Some(1.0);
            self.fading = false;
            self.fade_started_at_ms = None;
        }
        action
    }

    fn start_fade(&mut self, wall_ms: u64) {
        self.fading = true;
        self.fade_started_at_ms = Some(wall_ms);
    }

    /// Whether the stream is still audible from Theme's point of view: a
    /// physically playing source, or a fade that has not reached its target.
    fn still_playing(&self, physical: MusicOutputState, wall_ms: u64) -> bool {
        physical == MusicOutputState::Playing
            && !(self.fading
                && self
                    .fade_started_at_ms
                    .is_some_and(|started| wall_ms.saturating_sub(started) >= THEME_FADE_MS))
    }

    /// `ThemeClass__Queue_Song @ 0x00720B20`.
    pub(crate) fn queue_song(
        &mut self,
        index: i32,
        gates: ThemeGates,
        physical: MusicOutputState,
        wall_ms: u64,
    ) -> ThemeAction {
        if !self.admitted(gates) {
            return ThemeAction::default();
        }
        self.slots.pending = index;
        self.scenario_owns_pending = false;
        let mut action = ThemeAction::default();
        if index != THEME_NONE
            && index != THEME_AUTO
            && index != self.slots.retained
            && physical.stream_exists()
        {
            action = self.settle_fade(wall_ms);
            if self.still_playing(physical, wall_ms) {
                self.start_fade(wall_ms);
            }
        }
        action
    }

    /// Launcher `55FBE5..55FC06` and B8 `6B6918..6B6939` ScoreVolume zero:
    /// `Queue(retained, else pending)` followed immediately by `Stop(0)`.
    pub(crate) fn queue_then_stop_score_zero(
        &mut self,
        gates: ThemeGates,
        physical: MusicOutputState,
        wall_ms: u64,
    ) -> ThemeAction {
        if !self.admitted(gates) {
            return ThemeAction::default();
        }
        let request = self.current_song();
        let queued = self.queue_song(request, gates, physical, wall_ms);
        queued.then(self.stop(gates, false, physical, wall_ms))
    }

    /// B8 Play `6B67B1..6B6828`: an admitted list identity different from
    /// retained-or-pending requests Stop(1), then Queue(index). The app owns
    /// the preceding Options.IsScore=true write; Theme never owns the profile.
    pub(crate) fn play_selection(
        &mut self,
        index: i32,
        gates: ThemeGates,
        physical: MusicOutputState,
        wall_ms: u64,
    ) -> ThemeAction {
        if !self.admitted(gates)
            || index < 0
            || !self.is_allowed(index)
            || self.current_song() == index
        {
            return ThemeAction::default();
        }
        let stopped = self.stop(gates, true, physical, wall_ms);
        stopped.then(self.queue_song(index, gates, physical, wall_ms))
    }

    /// B8 Stop `6B69C8..6B69E5`: fade the stream, then queue the hold sentinel.
    /// The app applies the following Options.IsScore=false write separately.
    pub(crate) fn stop_selection(
        &mut self,
        gates: ThemeGates,
        physical: MusicOutputState,
        wall_ms: u64,
    ) -> ThemeAction {
        let stopped = self.stop(gates, true, physical, wall_ms);
        stopped.then(self.queue_song(THEME_HOLD, gates, physical, wall_ms))
    }

    /// `ThemeClass__Stop @ 0x00720EA0`. `fade` while playing starts the
    /// 1000 ms ramp and marks fading; every path clears all three slots. The
    /// immediate path (`fade == 0`, or nothing playing) only issues
    /// `StreamPlayer__Stop` and leaves `+0x11` as it stands — `Play_Song`
    /// clears it — so a stale flag survives here too; its only later effect is
    /// one redundant stop/scale-reset from `settle_fade`.
    pub(crate) fn stop(
        &mut self,
        gates: ThemeGates,
        fade: bool,
        physical: MusicOutputState,
        wall_ms: u64,
    ) -> ThemeAction {
        if !self.admitted(gates) || self.slots.active == THEME_NONE {
            return ThemeAction::default();
        }
        let mut action = ThemeAction::default();
        if fade && physical.stream_exists() {
            action = self.settle_fade(wall_ms);
            if self.still_playing(physical, wall_ms) {
                self.start_fade(wall_ms);
                self.slots = ThemeSlots::default();
                self.scenario_owns_pending = false;
                return action;
            }
        }
        self.slots = ThemeSlots::default();
        self.scenario_owns_pending = false;
        action.stop_output = true;
        action.theme_scale = Some(1.0);
        action
    }

    /// Cancel only Start_Scenario's queued ownership during scenario reset.
    /// Active and retained identities remain owned by Theme.
    pub(crate) fn cancel_scenario_theme_request(&mut self) -> ThemeAction {
        if self.scenario_owns_pending {
            self.slots.pending = THEME_NONE;
        }
        self.scenario_owns_pending = false;
        self.fading = false;
        self.fade_started_at_ms = None;
        ThemeAction {
            theme_scale: Some(1.0),
            ..Default::default()
        }
    }

    pub(crate) fn update(
        &mut self,
        assets: &AssetManager,
        gates: ThemeGates,
        physical: MusicOutputState,
        wall_ms: u64,
    ) -> ThemeAction {
        self.initialize_catalog(assets);
        let mut prepare = |stem: &str| prepare_track(stem, assets);
        self.ai(gates, physical, wall_ms, &mut prepare)
    }

    /// `ThemeClass__AI @ 0x007209D0`.
    ///
    /// VERA-internal: with no stream object native consumes the pending slot
    /// every poll (silently cycling `Play_Song`); Rust idles instead so an
    /// audio-less process does not churn the catalog.
    fn ai(
        &mut self,
        gates: ThemeGates,
        physical: MusicOutputState,
        wall_ms: u64,
        prepare: &mut impl FnMut(&str) -> Option<PreparedTrack>,
    ) -> ThemeAction {
        if !self.admitted(gates) || !physical.stream_exists() {
            return ThemeAction::default();
        }
        let mut action = ThemeAction::default();
        if physical == MusicOutputState::Playing {
            if self.fading
                && let Some(started) = self.fade_started_at_ms
            {
                let elapsed = wall_ms.saturating_sub(started);
                if elapsed < THEME_FADE_MS {
                    action.theme_scale = Some(1.0 - elapsed as f64 / THEME_FADE_MS as f64);
                    return action;
                }
                action = self.settle_fade(wall_ms);
            } else {
                return action;
            }
        }

        let pending = self.slots.pending;
        if pending == THEME_NONE || pending == THEME_HOLD {
            return action;
        }
        self.scenario_owns_pending = false;
        let index = if pending == THEME_AUTO {
            self.next_song(self.slots.retained)
        } else {
            pending
        };
        // Play_Song sees the stream released (fade stop or natural end).
        action =
            action.then(self.play_song(index, gates, MusicOutputState::Idle, wall_ms, prepare));
        // AI unconditionally restores -2 after attempting Play.
        self.slots.pending = THEME_AUTO;
        action
    }

    /// `ThemeClass__Play_Song @ 0x00720BB0`. The score-volume gate (`+0xC`)
    /// and the dead CD-check branch are not modelled.
    fn play_song(
        &mut self,
        index: i32,
        gates: ThemeGates,
        physical: MusicOutputState,
        wall_ms: u64,
        prepare: &mut impl FnMut(&str) -> Option<PreparedTrack>,
    ) -> ThemeAction {
        if !self.admitted(gates) {
            return ThemeAction::default();
        }
        let mut action = ThemeAction::default();
        if physical.stream_exists() {
            action = self.settle_fade(wall_ms);
            if self.still_playing(physical, wall_ms) && self.slots.retained == index {
                return action;
            }
        }
        // Inline Stop(0).
        if self.slots.active != THEME_NONE {
            self.slots = ThemeSlots::default();
            action.stop_output = true;
            action.theme_scale = Some(1.0);
        }
        self.fading = false;
        self.fade_started_at_ms = None;
        if index == THEME_NONE || index == THEME_HOLD {
            return action;
        }
        if index >= 0 {
            self.slots.retained = index;
            if physical.stream_exists() {
                let stem = self.entry(index).map(|entry| entry.sound.clone());
                match stem.as_deref().and_then(|stem| prepare(stem)) {
                    Some(prepared) => {
                        action.start = Some(prepared);
                        action.theme_scale = Some(1.0);
                        self.slots.active = index;
                    }
                    None => {
                        // PlayFile's failure is ignored natively (active :=
                        // index while nothing streams); Rust leaves active -1
                        // so AI's next poll consumes pending the same way.
                        self.slots.active = THEME_NONE;
                    }
                }
            } else {
                self.slots.active = THEME_NONE;
            }
            if !self.global_repeat && !self.entry_repeats(index) {
                return action;
            }
        }
        self.slots.pending = index;
        action
    }
}

/// Load and decode a track before optional physical submission.
fn prepare_track(track_name: &str, assets: &AssetManager) -> Option<PreparedTrack> {
    for filename in [format!("{track_name}.wav"), format!("{track_name}.aud")] {
        let Some(data) = assets.get_ref(&filename) else {
            continue;
        };

        if data.len() >= 44
            && &data[0..4] == b"RIFF"
            && let Some(decoded) = decode_wav(data, &filename)
            && decoded.sample_rate != 0
            && !decoded.samples.is_empty()
        {
            return Some(PreparedTrack {
                stem: track_name.to_string(),
                samples: decoded.samples,
                sample_rate: decoded.sample_rate,
            });
        }

        let Some((header, samples)) = aud_file::decode_aud(data) else {
            continue;
        };
        if samples.is_empty() || header.sample_rate == 0 {
            log::warn!("Track {track_name} decoded to 0 samples");
            return None;
        }
        let stereo = if header.is_stereo() {
            samples
                .iter()
                .map(|&sample| sample as f32 / 32768.0)
                .collect()
        } else {
            samples
                .iter()
                .flat_map(|&sample| {
                    let value = sample as f32 / 32768.0;
                    [value, value]
                })
                .collect()
        };
        return Some(PreparedTrack {
            stem: track_name.to_string(),
            samples: stereo,
            sample_rate: u32::from(header.sample_rate),
        });
    }
    log::warn!("Music track not found: resolved='{track_name}'");
    None
}

fn find_section<'a>(ini: &'a IniFile, name: &str) -> Option<&'a IniSection> {
    ini.section(name).or_else(|| {
        ini.section_names()
            .into_iter()
            .find(|candidate| candidate.eq_ignore_ascii_case(name))
            .and_then(|candidate| ini.section(candidate))
    })
}

/// Catalog load `0x00720590`: every non-empty `[Themes]` value in source
/// order becomes an entry keyed by that value (duplicates re-read the existing
/// entry); `0x00720480` fills the section fields with the ctor defaults
/// (Normal=1, Repeat=0, Scenario=0, Side=-1, Sound="").
pub(crate) fn catalog_from_ini(ini: &IniFile) -> Vec<ThemeEntry> {
    let mut entries: Vec<ThemeEntry> = Vec::new();
    let Some(themes) = ini.section("Themes") else {
        return entries;
    };
    for key in themes.get_values() {
        if key.is_empty() {
            continue;
        }
        let existing = entries
            .iter()
            .position(|entry| entry.key.eq_ignore_ascii_case(key));
        let index = match existing {
            Some(index) => index,
            None => {
                entries.push(ThemeEntry {
                    key: key.to_string(),
                    sound: String::new(),
                    name_key: String::new(),
                    duration_seconds: 0,
                    scenario: 0,
                    normal: true,
                    repeat: false,
                    side_name: None,
                    side: -1,
                    available: false,
                });
                entries.len() - 1
            }
        };
        let entry = &mut entries[index];
        let Some(section) = find_section(ini, &entry.key) else {
            continue;
        };
        if let Some(sound) = section.get("Sound") {
            entry.sound = sound.trim_start_matches(['$', '#']).to_string();
        }
        if let Some(scenario) = section.get_i32("Scenario") {
            entry.scenario = scenario;
        }
        if let Some(normal) = section.get_bool("Normal") {
            entry.normal = normal;
        }
        if entry.normal {
            entry.name_key = section.get("Name").unwrap_or("").to_owned();
        }
        if let Some(repeat) = section.get_bool("Repeat") {
            entry.repeat = repeat;
        }
        if let Some(side) = section.get("Side") {
            entry.side_name = Some(side.to_string());
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    const STOCK_SHAPED: &str = "[Themes]\n1=INTRO\n2=;Grinder\n14=SCORE\n15=LOADING\n\
        16=CREDITS\n17=RA2Options\n19=\n21=BrainFreeze\n22=Drok\n23=Deceiver\n24=PhatAttack\n\
        [INTRO]\nName=THEME:Intro\nSound=Drok\nNormal=no\nRepeat=yes\n\
        [LOADING]\nSound=Bully\nNormal=no\nRepeat=yes\n\
        [CREDITS]\nSound=OptionX\nNormal=no\nRepeat=yes\n\
        [SCORE]\nSound=ScoreX\nNormal=no\nRepeat=yes\n\
        [BrainFreeze]\nSound=BrainFre\nNormal=yes\n\
        [Drok]\nSound=Drok\nNormal=yes\n\
        [Deceiver]\nSound=$Deceiver\nNormal=yes\nSide=Soviet\n\
        [PhatAttack]\nSound=PhatAtta\nNormal=yes\nScenario=3\n";

    fn gates(enabled: bool) -> ThemeGates {
        ThemeGates {
            launcher_audio_available: enabled,
            startup_suppressed: false,
        }
    }

    fn stock_runtime() -> ThemeRuntime {
        let mut entries = catalog_from_ini(&IniFile::from_str(STOCK_SHAPED));
        for entry in &mut entries {
            entry.available = !entry.sound.is_empty();
        }
        ThemeRuntime::with_entries(entries)
    }

    fn prepared(stem: &str) -> Option<PreparedTrack> {
        Some(PreparedTrack {
            stem: stem.to_string(),
            samples: vec![0.0, 0.0],
            sample_rate: 22_050,
        })
    }

    #[test]
    fn entering_wol_cuts_intro_and_shuffles_until_leaving() {
        let mut theme = stock_runtime();
        let mut ok = |stem: &str| prepared(stem);
        theme.play_song(0, gates(true), MusicOutputState::Idle, 0, &mut ok);
        assert_eq!(theme.current_song(), 0, "INTRO plays");
        let (action, saved) =
            theme.enter_wol_lobby(true, gates(true), MusicOutputState::Playing, 100);
        assert!(!saved, "stock IsScoreShuffle is off");
        // Queue(-3) started a fade on INTRO; Stop(0) then stops the stream
        // at once and, like native, leaves the fading flag for Play to clear.
        assert!(
            action.stop_output,
            "Stop(0) cuts INTRO without waiting for the fade"
        );
        assert_eq!(theme.slots().pending, THEME_AUTO);
        // Theme AI starts a shuffled normal track, never INTRO.
        let started = theme.ai(gates(true), MusicOutputState::Idle, 200, &mut ok);
        let stem = started
            .start
            .map(|track| track.stem)
            .expect("a lobby track");
        assert_ne!(theme.current_song(), 0, "{stem}");
        assert!(theme.shuffle);
        theme.leave_wol_lobby(saved);
        assert!(!theme.shuffle);
    }

    #[test]
    fn wol_without_lobby_music_holds_the_current_song() {
        let mut theme = stock_runtime();
        let mut ok = |stem: &str| prepared(stem);
        theme.play_song(0, gates(true), MusicOutputState::Idle, 0, &mut ok);
        let (action, _) = theme.enter_wol_lobby(false, gates(true), MusicOutputState::Playing, 100);
        assert!(!action.stop_output);
        assert_eq!(theme.slots().pending, THEME_HOLD);
    }

    #[test]
    fn catalog_is_index_keyed_in_themes_order_with_per_entry_repeat() {
        let theme = stock_runtime();
        let keys: Vec<&str> = theme.entries().iter().map(|e| e.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "INTRO",
                "SCORE",
                "LOADING",
                "CREDITS",
                "RA2Options",
                "BrainFreeze",
                "Drok",
                "Deceiver",
                "PhatAttack"
            ]
        );
        let intro = &theme.entries()[0];
        let drok = &theme.entries()[6];
        assert_eq!(intro.sound, "Drok");
        assert!(intro.repeat && !intro.normal);
        assert_eq!(drok.sound, "Drok");
        assert!(
            !drok.repeat && drok.normal,
            "Repeat= belongs to the entry, not the stem"
        );
        let options = &theme.entries()[4];
        assert_eq!(options.sound, "");
        assert!(options.normal && !options.available);
        assert_eq!(theme.entries()[7].sound, "Deceiver", "leading $ stripped");
        assert_eq!(theme.entries()[7].side_name.as_deref(), Some("Soviet"));
        assert_eq!(theme.entries()[8].scenario, 3);
        assert_eq!(theme.from_name("intro"), 0);
        assert_eq!(theme.from_name("Drok"), 6);
        assert_eq!(
            theme.from_name("BrainFre"),
            -1,
            "Sound= stems are not aliases"
        );
        assert_eq!(theme.from_name(""), -1);
    }

    /// Catalog load `0x00720590` opens only `THEMEMD.INI` (`0x00825D94`); the
    /// RA2 `theme.ini` is never merged. Scan `0x007207F0` marks availability
    /// from `Sound + ".WAV"`.
    #[test]
    fn catalog_reads_thememd_only_and_scans_wav_availability() {
        let dir =
            std::env::temp_dir().join(format!("vera20k-theme-catalog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("test asset dir");
        std::fs::write(
            dir.join("theme.ini"),
            "[Themes]\n1=Grinder\n2=Drok\n[Grinder]\nSound=Grinder\nNormal=yes\n\
             [Drok]\nSound=Drok\nNormal=yes\nRepeat=yes\n",
        )
        .expect("write theme.ini");
        std::fs::write(
            dir.join("thememd.ini"),
            "[Themes]\n1=INTRO\n2=Drok\n[INTRO]\nSound=Drok\nNormal=no\nRepeat=yes\n\
             [Drok]\nSound=Drok\nName=THEME:Drok\nNormal=yes\n",
        )
        .expect("write thememd.ini");
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/storage_oracle/sound_theme_metadata.json"
        ))
        .unwrap();
        let drok = fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["name"] == "Drok")
            .unwrap();
        let hex = drok["header_hex"].as_str().unwrap();
        let mut wav: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        wav.resize(wav.len() + drok["data_bytes"].as_u64().unwrap() as usize, 0);
        std::fs::write(dir.join("DROK.WAV"), wav).expect("write wav");
        std::fs::write(dir.join("GRINDER.WAV"), b"RIFF").expect("write wav");
        let assets = AssetManager::from_loose_root_for_test(&dir);

        let mut theme = ThemeRuntime::default();
        theme.initialize_catalog(&assets);
        let keys: Vec<&str> = theme.entries().iter().map(|e| e.key.as_str()).collect();
        assert_eq!(keys, ["INTRO", "Drok"], "theme.ini [Themes] is not merged");
        assert_eq!(theme.from_name("Grinder"), THEME_NONE);
        assert!(theme.entries()[0].repeat && !theme.entries()[0].normal);
        assert!(
            !theme.entries()[1].repeat,
            "theme.ini's [Drok] Repeat=yes must not leak in"
        );
        assert!(theme.entries().iter().all(|entry| entry.available));
        assert_eq!(theme.entries()[1].name_key, "THEME:Drok");
        assert_eq!(
            theme.entries()[1].duration_seconds,
            drok["seconds"].as_u64().unwrap() as u32
        );
        assert!(
            theme.entries()[0].name_key.is_empty(),
            "Normal=no omits Name lookup"
        );

        // Missing THEMEMD.INI leaves an empty catalog even with theme.ini present.
        std::fs::remove_file(dir.join("thememd.ini")).expect("remove thememd.ini");
        let assets = AssetManager::from_loose_root_for_test(&dir);
        let mut theme = ThemeRuntime::default();
        theme.initialize_catalog(&assets);
        assert!(theme.entries().is_empty());
    }

    #[test]
    fn is_allowed_filters_availability_normal_side_and_campaign_scenario() {
        let mut theme = stock_runtime();
        assert!(theme.is_allowed(THEME_AUTO) && theme.is_allowed(THEME_HOLD));
        assert!(!theme.is_allowed(99));
        assert!(!theme.is_allowed(0), "Normal=no");
        assert!(!theme.is_allowed(4), "file unavailable");
        assert!(theme.is_allowed(5));
        // Side= resolved against the local player's side.
        theme.begin_scenario(
            7,
            ThemeAllowContext {
                local_side: Some(0),
                campaign_scenario: None,
            },
            |name| (name == "Soviet").then_some(1),
        );
        assert!(!theme.is_allowed(7));
        assert!(theme.is_allowed(8), "Scenario= ignored outside campaign");
        theme.begin_scenario(
            7,
            ThemeAllowContext {
                local_side: Some(1),
                campaign_scenario: Some(2),
            },
            |name| (name == "Soviet").then_some(1),
        );
        assert!(theme.is_allowed(7));
        assert!(!theme.is_allowed(8), "campaign scenario 2 < Scenario=3");
        theme.begin_scenario(
            7,
            ThemeAllowContext {
                local_side: Some(1),
                campaign_scenario: None,
            },
            |_| None,
        );
        assert!(!theme.is_allowed(7), "unknown Side= never matches");
        theme.begin_scenario(7, ThemeAllowContext::default(), |_| None);
        assert!(
            theme.is_allowed(7),
            "no player (shell) skips the Side= gate"
        );
    }

    #[test]
    fn cyclic_next_song_starts_after_prev_and_honors_repeat() {
        let mut theme = stock_runtime();
        assert_eq!(
            theme.next_song(-1),
            5,
            "first allowed after -1 is BrainFreeze"
        );
        assert_eq!(theme.next_song(5), 6);
        assert_eq!(theme.next_song(8), 5, "wraps past the shell-only entries");
        assert_eq!(theme.next_song(0), 0, "INTRO repeats");
        theme.set_score_options(true, false);
        assert_eq!(theme.next_song(5), 5, "global repeat returns prev");
        let mut empty = ThemeRuntime::with_entries(Vec::new());
        assert_eq!(empty.next_song(-1), 0);
    }

    #[test]
    fn shuffle_rejects_prev_and_disallowed_with_a_fixed_presentation_rng() {
        let mut theme = stock_runtime();
        theme.set_score_options(false, true);
        theme.begin_scenario(0x1234, ThemeAllowContext::default(), |_| None);
        let mut expected = SimRng::new(0x1234);
        let count = theme.entries().len() as i32;
        let mut prev = -1;
        for _ in 0..16 {
            let mut want;
            loop {
                want = expected.next_range_i32_inclusive(0, count - 1);
                if want != prev && (5..=8).contains(&want) {
                    break;
                }
            }
            let got = theme.next_song(prev);
            assert_eq!(got, want);
            assert_ne!(got, prev);
            prev = got;
        }
        // No allowed entry at all: 1000 rejections then index 0.
        let mut none = ThemeRuntime::with_entries(vec![ThemeEntry {
            key: "X".into(),
            sound: "X".into(),
            name_key: String::new(),
            duration_seconds: 0,
            scenario: 0,
            normal: false,
            repeat: false,
            side_name: None,
            side: -1,
            available: true,
        }]);
        none.set_score_options(false, true);
        assert_eq!(none.next_song(-1), 0);
    }

    #[test]
    fn start_scenario_hand_off_loading_then_fade_then_first_allowed() {
        let mut theme = stock_runtime();
        let mut ok = prepared;
        // Shell: INTRO playing (Play sets pending = INTRO because Repeat=yes).
        let action = theme.play_song(0, gates(true), MusicOutputState::Idle, 0, &mut ok);
        assert_eq!(action.start.as_ref().map(|p| p.stem.as_str()), Some("Drok"));
        assert_eq!(
            theme.slots(),
            ThemeSlots {
                active: 0,
                retained: 0,
                pending: 0
            }
        );

        // Start_Scenario: Play(From_Name("LOADING")) hard-replaces INTRO.
        let loading = theme.from_name("LOADING");
        let action = theme.play_song(
            loading,
            gates(true),
            MusicOutputState::Playing,
            100,
            &mut ok,
        );
        assert!(action.stop_output);
        assert_eq!(
            action.start.as_ref().map(|p| p.stem.as_str()),
            Some("Bully")
        );
        assert_eq!(
            theme.slots(),
            ThemeSlots {
                active: 2,
                retained: 2,
                pending: 2
            }
        );

        // Map without [Basic] Theme= -> Stop(fade=1): fading, slots cleared.
        let action = theme.stop(gates(true), true, MusicOutputState::Playing, 200);
        assert!(!action.stop_output);
        assert!(theme.fading);
        assert_eq!(theme.slots(), ThemeSlots::default());

        // Main_Tick: nothing retained -> Queue(-2) (no fade for -2).
        theme.main_tick(true, gates(true), MusicOutputState::Playing, 210);
        assert_eq!(theme.slots().pending, THEME_AUTO);

        // AI during the fade reports the ramp and waits.
        let action = theme.ai(gates(true), MusicOutputState::Playing, 700, &mut ok);
        assert_eq!(action.theme_scale, Some(0.5));
        assert!(action.start.is_none());

        // Fade reached target: stop the stream, Next_Song(-1) = BrainFreeze.
        let action = theme.ai(gates(true), MusicOutputState::Playing, 1_200, &mut ok);
        assert!(action.stop_output);
        assert_eq!(
            action.start.as_ref().map(|p| p.stem.as_str()),
            Some("BrainFre")
        );
        assert_eq!(
            theme.slots(),
            ThemeSlots {
                active: 5,
                retained: 5,
                pending: THEME_AUTO
            }
        );
        assert!(!theme.fading);

        // Natural end -> next cyclic track (Drok entry, index 6, not INTRO).
        let action = theme.ai(gates(true), MusicOutputState::Finished, 5_000, &mut ok);
        assert_eq!(action.start.as_ref().map(|p| p.stem.as_str()), Some("Drok"));
        assert_eq!(
            theme.slots(),
            ThemeSlots {
                active: 6,
                retained: 6,
                pending: THEME_AUTO
            }
        );
    }

    #[test]
    fn scenario_with_basic_theme_queues_and_fades_the_loading_track() {
        let mut theme = stock_runtime();
        let mut ok = prepared;
        theme.play_song(2, gates(true), MusicOutputState::Idle, 0, &mut ok);
        let action = theme.queue_song(7, gates(true), MusicOutputState::Playing, 50);
        assert!(!action.stop_output);
        assert!(theme.fading);
        assert_eq!(theme.slots().pending, 7);
        let action = theme.ai(gates(true), MusicOutputState::Playing, 1_100, &mut ok);
        assert!(action.stop_output);
        assert_eq!(
            theme.slots(),
            ThemeSlots {
                active: 7,
                retained: 7,
                pending: THEME_AUTO
            }
        );
    }

    #[test]
    fn main_tick_in_game_music_off_holds_with_fade_and_minus_three() {
        let mut theme = stock_runtime();
        let mut ok = prepared;
        theme.play_song(5, gates(true), MusicOutputState::Idle, 0, &mut ok);
        theme.main_tick(false, gates(true), MusicOutputState::Playing, 10);
        assert!(theme.fading);
        assert_eq!(
            theme.slots(),
            ThemeSlots {
                active: -1,
                retained: -1,
                pending: THEME_HOLD
            }
        );
        let before = theme.slots();
        theme.main_tick(false, gates(true), MusicOutputState::Playing, 20);
        assert_eq!(theme.slots(), before);
        let action = theme.ai(gates(true), MusicOutputState::Playing, 1_100, &mut ok);
        assert!(action.stop_output && action.start.is_none());
        assert_eq!(theme.slots().pending, THEME_HOLD, "-3 is never consumed");
    }

    #[test]
    fn queue_sentinels_and_same_retained_track_do_not_fade() {
        let mut theme = stock_runtime();
        theme.slots = ThemeSlots {
            active: 5,
            retained: 5,
            pending: THEME_HOLD,
        };
        for request in [THEME_NONE, THEME_AUTO, 5] {
            theme.queue_song(request, gates(true), MusicOutputState::Playing, 100);
            assert_eq!(theme.slots().pending, request);
            assert!(!theme.fading);
        }
        theme.queue_song(THEME_HOLD, gates(true), MusicOutputState::Playing, 300);
        assert!(theme.fading);
        assert_eq!(theme.fade_started_at_ms, Some(300));
    }

    #[test]
    fn play_same_retained_while_playing_is_a_no_op_and_failed_gates_preserve_slots() {
        let mut theme = stock_runtime();
        let mut ok = prepared;
        theme.play_song(5, gates(true), MusicOutputState::Idle, 0, &mut ok);
        let before = theme.slots();
        let action = theme.play_song(5, gates(true), MusicOutputState::Playing, 10, &mut ok);
        assert!(action.start.is_none() && !action.stop_output);
        assert_eq!(theme.slots(), before);

        let action = theme.queue_song(7, gates(false), MusicOutputState::Playing, 10);
        assert!(!action.stop_output && action.start.is_none());
        assert_eq!(theme.slots(), before);
        let suppressed = ThemeGates {
            launcher_audio_available: true,
            startup_suppressed: true,
        };
        theme.stop(suppressed, false, MusicOutputState::Playing, 10);
        assert_eq!(theme.slots(), before);
    }

    #[test]
    fn play_sentinels_stop_and_auto_sets_pending() {
        let mut theme = stock_runtime();
        let mut ok = prepared;
        theme.play_song(5, gates(true), MusicOutputState::Idle, 0, &mut ok);
        let action = theme.play_song(
            THEME_NONE,
            gates(true),
            MusicOutputState::Playing,
            1,
            &mut ok,
        );
        assert!(action.stop_output);
        assert_eq!(theme.slots(), ThemeSlots::default());
        theme.play_song(5, gates(true), MusicOutputState::Idle, 2, &mut ok);
        let action = theme.play_song(
            THEME_AUTO,
            gates(true),
            MusicOutputState::Playing,
            3,
            &mut ok,
        );
        assert!(action.stop_output);
        assert_eq!(
            theme.slots(),
            ThemeSlots {
                active: -1,
                retained: -1,
                pending: THEME_AUTO
            }
        );
    }

    #[test]
    fn ai_ignores_none_hold_and_unavailable_and_restores_auto_after_failure() {
        let mut theme = stock_runtime();
        let mut ok = prepared;
        theme.slots = ThemeSlots {
            active: 5,
            retained: 5,
            pending: THEME_NONE,
        };
        theme.ai(gates(true), MusicOutputState::Finished, 1, &mut ok);
        assert_eq!(
            theme.slots(),
            ThemeSlots {
                active: 5,
                retained: 5,
                pending: THEME_NONE
            }
        );
        theme.slots.pending = THEME_HOLD;
        theme.ai(gates(true), MusicOutputState::Finished, 2, &mut ok);
        assert_eq!(theme.slots().pending, THEME_HOLD);
        theme.slots.pending = THEME_AUTO;
        theme.ai(gates(true), MusicOutputState::Unavailable, 3, &mut ok);
        assert_eq!(
            theme.slots(),
            ThemeSlots {
                active: 5,
                retained: 5,
                pending: THEME_AUTO
            }
        );

        let mut missing = |_stem: &str| None;
        theme.ai(gates(true), MusicOutputState::Finished, 4, &mut missing);
        assert_eq!(
            theme.slots(),
            ThemeSlots {
                active: -1,
                retained: 6,
                pending: THEME_AUTO
            }
        );
    }

    #[test]
    fn score_zero_is_queue_then_stop_not_unconditional_reset() {
        let mut theme = stock_runtime();
        theme.slots = ThemeSlots {
            active: 5,
            retained: 6,
            pending: THEME_HOLD,
        };
        let action = theme.queue_then_stop_score_zero(gates(true), MusicOutputState::Playing, 0);
        assert!(action.stop_output);
        assert_eq!(theme.slots(), ThemeSlots::default());

        theme.slots = ThemeSlots {
            active: -1,
            retained: 6,
            pending: THEME_HOLD,
        };
        let action = theme.queue_then_stop_score_zero(gates(true), MusicOutputState::Idle, 0);
        assert!(!action.stop_output);
        assert_eq!(
            theme.slots(),
            ThemeSlots {
                active: -1,
                retained: 6,
                pending: 6
            }
        );
    }

    #[test]
    fn current_song_matches_both_original_callers() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/storage_oracle/sound_theme_metadata.json"
        ))
        .unwrap();
        let cases = fixture["current_song_cases"].as_array().unwrap();
        assert_eq!(cases.len(), 4);
        let mut theme = stock_runtime();
        for case in cases {
            theme.slots = ThemeSlots {
                active: case["active"].as_i64().unwrap() as i32,
                retained: case["retained"].as_i64().unwrap() as i32,
                pending: case["pending"].as_i64().unwrap() as i32,
            };
            let current = theme.current_song();
            assert_eq!(current, case["launcher"].as_i64().unwrap() as i32);
            assert_eq!(current, case["sound"].as_i64().unwrap() as i32);
        }
    }

    #[test]
    fn score_zero_preserves_pending_request_when_nothing_is_retained_or_active() {
        let mut theme = stock_runtime();
        theme.slots.pending = 7;
        let action = theme.queue_then_stop_score_zero(gates(true), MusicOutputState::Idle, 0);
        assert!(!action.stop_output);
        assert_eq!(
            theme.slots.pending, 7,
            "native Stop has no active track to clear"
        );
    }

    #[test]
    fn sound_play_and_stop_preserve_theme_queue_and_fade_authority() {
        let mut theme = stock_runtime();
        let mut prepare = prepared;
        theme.play_song(5, gates(true), MusicOutputState::Idle, 0, &mut prepare);
        let same = theme.play_selection(5, gates(true), MusicOutputState::Playing, 50);
        assert!(same.start.is_none() && !same.stop_output && !theme.fading);
        assert_eq!(theme.slots.active, 5);

        let next = theme.play_selection(6, gates(true), MusicOutputState::Playing, 100);
        assert!(next.start.is_none() && !next.stop_output);
        assert!(theme.fading);
        assert_eq!(
            theme.slots,
            ThemeSlots {
                active: -1,
                retained: -1,
                pending: 6
            }
        );
        let during = theme.ai(gates(true), MusicOutputState::Playing, 600, &mut prepare);
        assert_eq!(during.theme_scale, Some(0.5));
        assert!(during.start.is_none());
        let finished = theme.ai(gates(true), MusicOutputState::Playing, 1100, &mut prepare);
        assert!(finished.stop_output);
        assert_eq!(
            finished.start.as_ref().map(|track| track.stem.as_str()),
            Some("Drok")
        );
        assert_eq!(theme.current_song(), 6);

        let stopped = theme.stop_selection(gates(true), MusicOutputState::Playing, 1200);
        assert!(!stopped.stop_output && stopped.start.is_none());
        assert_eq!(theme.current_song(), THEME_HOLD);
        theme.main_tick(false, gates(true), MusicOutputState::Playing, 1300);
        let finished = theme.ai(gates(true), MusicOutputState::Playing, 2200, &mut prepare);
        assert!(finished.stop_output && finished.start.is_none());
        assert_eq!(
            theme.slots.pending, THEME_HOLD,
            "Stop must not auto-restart music"
        );
    }

    #[test]
    fn scenario_cancel_clears_only_scenario_owned_pending() {
        let mut theme = stock_runtime();
        theme.slots.pending = THEME_AUTO;
        theme.cancel_scenario_theme_request();
        assert_eq!(theme.slots().pending, THEME_AUTO);

        theme.queue_song(7, gates(true), MusicOutputState::Playing, 10);
        theme.scenario_owns_pending = true;
        theme.cancel_scenario_theme_request();
        assert_eq!(theme.slots().pending, THEME_NONE);
    }
}
