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
//!
//! VERA presentation extension: append available, normal RA2 `THEME.INI`
//! entries after the unchanged YR catalog. Native YR never merges that file.
//! YR keeps every existing index and wins case-insensitive key collisions;
//! both catalogs use the same native readers, scan, playlist and player.
//! A larger playlist intentionally changes shuffle ranges and rejection draws
//! on the existing shared Main RNG; it does not introduce another RNG owner.
//! User-requested YR parity exception: preserve this expanded gameplay playlist
//! during future parity work unless the user explicitly asks to change it.

use std::ops::Range;

use crate::assets::asset_manager::AssetManager;
use crate::assets::aud_file;
use crate::assets::mix_archive::SharedBytes;
use crate::assets::wav_file::{WavFile, WavPcmCursor};
use crate::audio::sfx::upmix_i16_to_f32_stereo;
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::util::native_x87::X87Chop53;

const THEME_INI_NAME: &str = "thememd.ini";
const RA2_THEME_INI_NAME: &str = "theme.ini";
const MENU_THEME_SECTION: &str = "INTRO";
/// `Next_Song` shuffle rejection budget (`CMP EDI,0x3E8` @ `0x00720AC6`).
const SHUFFLE_TRIES: u32 = 1_000;

#[cfg(test)]
#[path = "theme_saved_tests.rs"]
mod saved_tests;

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
    /// The current source is still playing and the stream's own volume
    /// interp is moving (`Audio__IsStreamPlayerVolumeInterpolating`, read by
    /// `Still_Playing` at `0x00720FE1`).
    Ramping,
    /// The current source reached its physical end.
    Finished,
}

impl MusicOutputState {
    fn stream_exists(self) -> bool {
        self != Self::Unavailable
    }

    pub(crate) fn playing(self) -> bool {
        matches!(self, Self::Playing | Self::Ramping)
    }

    /// The stream as the next step of the same native call chain reads it
    /// again, once this step's Stop, PlayFile and SetTarget have run.
    fn after(self, action: &ThemeAction) -> Self {
        if !self.stream_exists() {
            return self;
        }
        let state = if action.start.is_some() {
            Self::Playing
        } else if action.stop_output {
            Self::Idle
        } else {
            self
        };
        if action.fade_out && state.playing() {
            Self::Ramping
        } else {
            state
        }
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

/// A song ready for optional physical submission, pulled as interleaved
/// stereo samples.
///
/// gamemd-derived: `ThemeClass::Play_Song @ 0x00720BB0` hands the file to
/// `StreamPlayer__PlayFile @ 0x00407B60`, which streams it through the one
/// block decoder `IMA_ADPCM__DecodeBlock @ 0x0040AA70` rather than decoding it
/// whole. Preparation reads the WAV header and decodes the first piece, whose
/// failure is the whole decode's empty result; the rest decodes as the output
/// pulls, on the device thread. The VERA-only AUD fallback arrives whole.
pub(crate) struct PreparedTrack {
    pub(crate) stem: String,
    pub(crate) sample_rate: u32,
    /// Decoded samples; those from `pulled` on are still to play.
    pcm: Vec<f32>,
    pulled: usize,
    /// The WAV payload left to decode; `None` once decoded through, or for
    /// a track decoded whole.
    stream: Option<WavStream>,
}

/// A WAV song's payload, read from a share of its archive storage so neither
/// thread copies it.
struct WavStream {
    file: SharedBytes,
    payload: Range<usize>,
    channels: u16,
    cursor: WavPcmCursor,
    piece: Vec<i16>,
}

impl PreparedTrack {
    /// A track decoded whole up front.
    pub(crate) fn decoded(stem: String, samples: Vec<f32>, sample_rate: u32) -> Self {
        Self {
            stem,
            sample_rate,
            pcm: samples,
            pulled: 0,
            stream: None,
        }
    }

    /// A WAV song decoding as it is pulled; `None` wherever the whole-file
    /// `decode_wav` fails or yields no samples, or the rate is zero.
    pub(super) fn streamed(stem: &str, file: &SharedBytes) -> Option<Self> {
        let wav = WavFile::parse(file)?;
        let cursor = WavPcmCursor::new(&wav)?;
        if wav.sample_rate == 0 {
            return None;
        }
        let mut track = Self {
            stem: stem.to_string(),
            sample_rate: wav.sample_rate,
            pcm: Vec::new(),
            pulled: 0,
            stream: Some(WavStream {
                payload: wav.data_range(),
                channels: wav.channels,
                file: file.clone(),
                cursor,
                piece: Vec::new(),
            }),
        };
        track.decode_next();
        if track.exhausted() {
            return None;
        }
        file.read_ahead();
        Some(track)
    }

    /// Every sample has been pulled.
    pub(crate) fn exhausted(&self) -> bool {
        self.pulled >= self.pcm.len()
    }

    /// Replace the pulled samples with the next decoded piece, leaving the
    /// track exhausted when the payload has none.
    fn decode_next(&mut self) {
        let Some(stream) = self.stream.as_mut() else {
            return;
        };
        self.pcm.clear();
        self.pulled = 0;
        stream.piece.clear();
        let payload = &stream.file[stream.payload.clone()];
        if stream.cursor.decode_next(payload, &mut stream.piece) {
            upmix_i16_to_f32_stereo(&mut self.pcm, &stream.piece, stream.channels);
        } else {
            self.stream = None;
        }
    }
}

impl Iterator for PreparedTrack {
    type Item = f32;

    /// Decodes the next piece as soon as the last decoded sample is pulled,
    /// so [`PreparedTrack::exhausted`] holds only at the true end.
    fn next(&mut self) -> Option<f32> {
        let sample = *self.pcm.get(self.pulled)?;
        self.pulled += 1;
        if self.exhausted() {
            self.decode_next();
        }
        Some(sample)
    }
}

impl std::fmt::Debug for PreparedTrack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedTrack")
            .field("stem", &self.stem)
            .field("sample_rate", &self.sample_rate)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
pub(crate) struct ThemeAction {
    pub(crate) stop_output: bool,
    pub(crate) start: Option<PreparedTrack>,
    /// `Audio__SetStreamPlayerVolumeTargetZero @ 0x004080C0`: glide the
    /// stream's own interp to zero.
    pub(crate) fade_out: bool,
}

impl ThemeAction {
    fn then(mut self, next: Self) -> Self {
        self.stop_output |= next.stop_output;
        if next.start.is_some() {
            self.start = next.start;
        }
        self.fade_out |= next.fade_out;
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
/// Constructor720960 defaults: ThemeSlots keeps its -1 sentinels; repeat,
/// fading and shuffle start false.
#[derive(Debug)]
pub(crate) struct ThemeRuntime {
    catalog_loaded: bool,
    entries: Vec<ThemeEntry>,
    slots: ThemeSlots,
    /// Signed +0x0C gate; derived from Options' stored f32, independent of
    /// the physical stream's separate gain. Ctor72096F initializes 255.
    score_volume: i32,
    /// +0x10 `IsScoreRepeat`.
    global_repeat: bool,
    /// +0x11 fading flag; the ramp itself is the stream's own interp.
    fading: bool,
    /// +0x12 `IsScoreShuffle`.
    shuffle: bool,
    /// Whether Start_Scenario owns the pending slot (scenario reset cancel).
    scenario_owns_pending: bool,
    allow_context: ThemeAllowContext,
}

impl Default for ThemeRuntime {
    fn default() -> Self {
        Self {
            catalog_loaded: false,
            entries: Vec::new(),
            slots: ThemeSlots::default(),
            score_volume: 255,
            global_repeat: false,
            fading: false,
            shuffle: false,
            scenario_owns_pending: false,
            allow_context: ThemeAllowContext::default(),
        }
    }
}

impl ThemeRuntime {
    /// Options5FA4A0/ReadFromINI5FAAE6/apply5FB251 derive this gate from
    /// the already stored, upper-clamped Options f32 times double255.0.
    /// The product is exact in f64 (at most 32 significand bits); reuse
    /// Math::ftol's signed64/low32 and masked-invalid conversion owner.
    /// Executed controls: tools/storage_oracle/sound_theme_metadata.
    pub(crate) fn set_score_volume(&mut self, stored_volume: f32) {
        self.set_volume(X87Chop53::ftol_f64_low_masked(
            f64::from(stored_volume) * 255.0,
        ));
    }

    /// ThemeSetVolume721290 clamps only the signed upper side.
    fn set_volume(&mut self, volume: i32) {
        self.score_volume = volume.min(255);
    }

    /// Catalog load `0x00720590` + scan `0x007207F0`, once per process, with
    /// the requested RA2 gameplay playlist extension appended afterwards.
    pub(crate) fn initialize_catalog(&mut self, assets: &AssetManager) {
        if self.catalog_loaded {
            return;
        }
        if let Ok(selected) = crate::rules::retail_sources::select_ini(assets, THEME_INI_NAME) {
            self.entries = catalog_from_ini(&selected.ini);
        }
        scan_catalog_availability(&mut self.entries, assets);

        // Read RA2 independently: merging INIs would let its INTRO, SCORE,
        // LOADING and CREDITS sections replace the YR lifecycle themes.
        if let Ok(selected) = crate::rules::retail_sources::select_ini(assets, RA2_THEME_INI_NAME) {
            let mut ra2_entries = catalog_from_ini(&selected.ini);
            scan_catalog_availability(&mut ra2_entries, assets);
            for entry in ra2_entries {
                if entry.normal
                    && entry.available
                    && !self
                        .entries
                        .iter()
                        .any(|yr| yr.key.eq_ignore_ascii_case(&entry.key))
                {
                    self.entries.push(entry);
                }
            }
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

    /// ThemeSave721010 writes +4 then +8. The active output slot and stream
    /// position are process state, not part of the native eight-byte record.
    pub(crate) fn saved_slots(&self) -> [i32; 2] {
        [self.slots.retained, self.slots.pending]
    }

    /// ThemeLoad721040, reached by LoadContent67F79A. InGameMusic gates both
    /// Play(saved +4) and Queue(saved +8). Reuse those same owners and observe
    /// the physical output after Play before Queue decides whether to fade.
    /// Native controls: tools/storage_oracle/sound_theme_metadata.
    #[expect(
        clippy::too_many_arguments,
        reason = "native load inputs plus physical output handoff"
    )]
    pub(crate) fn restore_saved_slots(
        &mut self,
        saved: [i32; 2],
        in_game_music: bool,
        assets: &AssetManager,
        gates: ThemeGates,
        physical: MusicOutputState,
        mut apply_output: impl FnMut(ThemeAction) -> MusicOutputState,
    ) {
        if !in_game_music {
            return;
        }
        let mut prepare = |stem: &str| prepare_track(stem, assets);
        let played = self.play_song(saved[0], gates, physical, &mut prepare);
        let physical = apply_output(played);
        let queued = self.queue_song(saved[1], gates, physical);
        apply_output(queued);
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

    /// Start_Scenario-time context: pin the local player's side for
    /// `Is_Allowed`. Main RNG lifetime belongs to the caller, not Theme.
    pub(crate) fn begin_scenario(
        &mut self,
        context: ThemeAllowContext,
        resolve_side: impl Fn(&str) -> Option<i32>,
    ) {
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
    #[expect(
        clippy::wrong_self_convention,
        reason = "named after the native ThemeClass::From_Name lookup"
    )]
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

    /// `ThemeClass__Next_Song @ 0x00720A80`: `0x00720AB5` draws inclusive
    /// `[0, count - 1]` from Main `0x00886B88`. Borrow the caller's draw so
    /// Sound, Vox and Theme (`AudioSystem::Pump 0x00406F70`) share one cursor.
    fn next_song(&self, prev: i32, draw_main: &mut impl FnMut(i32, i32) -> i32) -> i32 {
        let count = self.entries.len() as i32;
        if prev >= 0 && (self.entry_repeats(prev) || self.global_repeat) {
            return prev;
        }
        if self.shuffle {
            let mut tries = 0u32;
            let mut draw;
            loop {
                draw = draw_main(0, count - 1);
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
    ) -> ThemeAction {
        self.initialize_catalog(assets);
        let index = self.from_name(track_name);
        let mut prepare = |stem: &str| prepare_track(stem, assets);
        self.play_song(index, gates, physical, &mut prepare)
    }

    /// ReadTheme4758F0's name/default tail after the production ReadString128.
    /// A nonempty unknown name becomes-1; only missing/empty keeps the default.
    fn read_theme_name(&self, name: Option<&str>, default: i32) -> i32 {
        name.filter(|name| !name.is_empty())
            .map_or(default, |name| self.from_name(name))
    }

    /// Installed Ares1002FB80/1002FC20: ordered Side then Country, or one
    /// campaign Basic name. Exactly-2 retains retail LOADING; unknown-1 must
    /// reach the same Play_Song owner. Native: rules_oracle/ares_loading_themes.
    #[cfg(feature = "ares")]
    pub(crate) fn play_ares_loading_theme<'a>(
        &mut self,
        names: impl IntoIterator<Item = Option<&'a str>>,
        assets: &AssetManager,
        gates: ThemeGates,
        physical: MusicOutputState,
    ) -> ThemeAction {
        self.initialize_catalog(assets);
        let selected = names
            .into_iter()
            .fold(THEME_AUTO, |prior, name| self.read_theme_name(name, prior));
        let selected = if selected == THEME_AUTO {
            self.from_name("LOADING")
        } else {
            selected
        };
        let mut prepare = |stem: &str| prepare_track(stem, assets);
        self.play_song(selected, gates, physical, &mut prepare)
    }

    /// `Main__PrepareSession @ 0x0052D9A0`: `Play(From_Name("INTRO"))`.
    pub(crate) fn play_menu_theme(
        &mut self,
        assets: &AssetManager,
        gates: ThemeGates,
        physical: MusicOutputState,
    ) -> ThemeAction {
        self.play_track(MENU_THEME_SECTION, assets, gates, physical)
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
    ) -> ThemeAction {
        self.initialize_catalog(assets);
        let index = self.read_theme_name(requested_section.map(str::trim), THEME_NONE);
        let admitted = self.admitted(gates);
        let action = if index == THEME_NONE {
            self.stop(gates, true, physical)
        } else {
            self.queue_song(index, gates, physical)
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
    ) -> ThemeAction {
        if !self.admitted(gates) || !physical.stream_exists() {
            return ThemeAction::default();
        }
        let current = self.current_song();
        if !in_game_music {
            if current != THEME_HOLD {
                let stopped = self.stop(gates, true, physical);
                let physical = physical.after(&stopped);
                return stopped.then(self.queue_song(THEME_HOLD, gates, physical));
            }
            ThemeAction::default()
        } else if current == THEME_NONE {
            self.queue_song(THEME_AUTO, gates, physical)
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
    ) -> (ThemeAction, bool) {
        let intro = self.from_name(MENU_THEME_SECTION);
        let mut action = ThemeAction::default();
        let mut physical = physical;
        if self.current_song() == intro {
            let queued = self.queue_song(THEME_HOLD, gates, physical);
            physical = physical.after(&queued);
            action = action.then(queued);
        }
        let saved_shuffle = std::mem::replace(&mut self.shuffle, true);
        action = action.then(self.wol_lobby_music_rule(lobby_music, gates, physical));
        (action, saved_shuffle)
    }

    /// The music half of `0x0077E110`, also WOL Options' Play Music preview
    /// (`0x0078EBC0` case `0x5F8`): with lobby music, `Stop(0)` and
    /// `Queue(-2)` when no stream plays or INTRO (current, else pending) is
    /// still the song; otherwise the current song stays. Without it,
    /// `Queue(-3)`.
    pub(crate) fn wol_lobby_music_rule(
        &mut self,
        lobby_music: bool,
        gates: ThemeGates,
        physical: MusicOutputState,
    ) -> ThemeAction {
        let mut action = ThemeAction::default();
        if lobby_music {
            let intro = self.from_name(MENU_THEME_SECTION);
            let (playing, settled) = self.still_playing(physical);
            let physical = physical.after(&settled);
            action = action.then(settled);
            if !playing || self.current_song() == intro {
                let stopped = self.stop(gates, false, physical);
                let physical = physical.after(&stopped);
                action = action.then(stopped);
                action = action.then(self.queue_song(THEME_AUTO, gates, physical));
            }
        } else {
            action = action.then(self.queue_song(THEME_HOLD, gates, physical));
        }
        action
    }

    /// Every `WOL_Main` exit restores the saved shuffle flag
    /// (`0x0077B549`, `0x0077B56D`, `0x0077B59C`, ...); the menu then plays
    /// INTRO again.
    pub(crate) fn leave_wol_lobby(&mut self, saved_shuffle: bool) {
        self.shuffle = saved_shuffle;
    }

    /// `Still_Playing @ 0x00720FD0..0x00721005`, also inlined in
    /// Queue/Stop/Play/AI. A fade is complete once the stream's own interp
    /// stops moving (`0x00720FE1`); it stops the stream (`0x00720FED`) before
    /// querying playback and does not clear Theme's fading flag. Play clears
    /// it later at720C6D. Return the resulting observation with the deferred output
    /// action so callers cannot reuse a stale Playing observation after Stop.
    pub(crate) fn still_playing(&self, physical: MusicOutputState) -> (bool, ThemeAction) {
        let mut action = ThemeAction::default();
        if physical.stream_exists() && self.fading && physical != MusicOutputState::Ramping {
            action.stop_output = true;
            return (false, action);
        }
        (physical.playing(), action)
    }

    fn start_fade(&mut self) -> ThemeAction {
        self.fading = true;
        // Queue720B20 / Stop720EA0 ->4080C0 ->407170 retargets the
        // existing ramp; SetTarget only restamps an interpolator at rest, so
        // repeated requests do not restart a fade already in flight.
        // Executed history: trigger_action_values --music-theme.
        ThemeAction {
            fade_out: true,
            ..Default::default()
        }
    }

    /// `ThemeClass__Queue_Song @ 0x00720B20`.
    pub(crate) fn queue_song(
        &mut self,
        index: i32,
        gates: ThemeGates,
        physical: MusicOutputState,
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
            let playing;
            (playing, action) = self.still_playing(physical);
            if playing {
                action = action.then(self.start_fade());
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
    ) -> ThemeAction {
        if !self.admitted(gates) {
            return ThemeAction::default();
        }
        let request = self.current_song();
        let queued = self.queue_song(request, gates, physical);
        let physical = physical.after(&queued);
        queued.then(self.stop(gates, false, physical))
    }

    /// B8 Play `6B67B1..6B6828`: an admitted list identity different from
    /// retained-or-pending requests Stop(1), then Queue(index). The app owns
    /// the preceding Options.IsScore=true write; Theme never owns the profile.
    pub(crate) fn play_selection(
        &mut self,
        index: i32,
        gates: ThemeGates,
        physical: MusicOutputState,
    ) -> ThemeAction {
        if !self.admitted(gates)
            || index < 0
            || !self.is_allowed(index)
            || self.current_song() == index
        {
            return ThemeAction::default();
        }
        let stopped = self.stop(gates, true, physical);
        let physical = physical.after(&stopped);
        stopped.then(self.queue_song(index, gates, physical))
    }

    /// B8 Stop `6B69C8..6B69E5`: fade the stream, then queue the hold sentinel.
    /// The app applies the following Options.IsScore=false write separately.
    pub(crate) fn stop_selection(
        &mut self,
        gates: ThemeGates,
        physical: MusicOutputState,
    ) -> ThemeAction {
        let stopped = self.stop(gates, true, physical);
        let physical = physical.after(&stopped);
        stopped.then(self.queue_song(THEME_HOLD, gates, physical))
    }

    /// `ThemeClass__Stop @ 0x00720EA0`. `fade` while playing starts the
    /// 1000 ms ramp and marks fading; every path clears all three slots. The
    /// immediate path (`fade == 0`, or nothing playing) only issues
    /// `StreamPlayer__Stop` and leaves `+0x11` as it stands — `Play_Song`
    /// clears it — so a stale flag survives here too; its only later effect is
    /// redundant stop/scale-reset actions from `still_playing`.
    pub(crate) fn stop(
        &mut self,
        gates: ThemeGates,
        fade: bool,
        physical: MusicOutputState,
    ) -> ThemeAction {
        if !self.admitted(gates) || self.slots.active == THEME_NONE {
            return ThemeAction::default();
        }
        let mut action = ThemeAction::default();
        if fade && physical.stream_exists() {
            let playing;
            (playing, action) = self.still_playing(physical);
            if playing {
                action = action.then(self.start_fade());
                self.slots = ThemeSlots::default();
                self.scenario_owns_pending = false;
                return action;
            }
        }
        self.slots = ThemeSlots::default();
        self.scenario_owns_pending = false;
        action.stop_output = true;
        action
    }

    /// Cancel only Start_Scenario's queued ownership during scenario reset.
    /// Active and retained identities remain owned by Theme. VERA-internal
    /// (native has no cancel): a fade in flight runs on in the stream's own
    /// interp and stops the stream when it ends, as it would natively.
    pub(crate) fn cancel_scenario_theme_request(&mut self) {
        if self.scenario_owns_pending {
            self.slots.pending = THEME_NONE;
        }
        self.scenario_owns_pending = false;
    }

    pub(crate) fn update(
        &mut self,
        assets: &AssetManager,
        gates: ThemeGates,
        physical: MusicOutputState,
        draw_main: &mut impl FnMut(i32, i32) -> i32,
    ) -> ThemeAction {
        self.initialize_catalog(assets);
        let mut prepare = |stem: &str| prepare_track(stem, assets);
        self.ai(gates, physical, &mut prepare, draw_main)
    }

    /// `ThemeClass__AI @ 0x007209D0`. Signed volume gate7209FF precedes
    /// stream/fade/pending/RNG work. An absent stream still consumes pending
    /// when the common device gate admits this owner and volume is positive.
    fn ai(
        &mut self,
        gates: ThemeGates,
        physical: MusicOutputState,
        prepare: &mut impl FnMut(&str) -> Option<PreparedTrack>,
        draw_main: &mut impl FnMut(i32, i32) -> i32,
    ) -> ThemeAction {
        if !self.admitted(gates) || self.score_volume <= 0 {
            return ThemeAction::default();
        }
        let (playing, mut action) = self.still_playing(physical);
        if playing {
            return action;
        }

        let pending = self.slots.pending;
        if pending == THEME_NONE || pending == THEME_HOLD {
            return action;
        }
        self.scenario_owns_pending = false;
        let index = if pending == THEME_AUTO {
            self.next_song(self.slots.retained, draw_main)
        } else {
            pending
        };
        // A released source is idle; an absent stream remains absent. Play's
        // retained/pending writes still run without decoding physical audio.
        let physical = if physical.stream_exists() {
            MusicOutputState::Idle
        } else {
            MusicOutputState::Unavailable
        };
        action = action.then(self.play_song(index, gates, physical, prepare));
        // AI unconditionally restores -2 after attempting Play.
        self.slots.pending = THEME_AUTO;
        action
    }

    /// `ThemeClass__Play_Song @ 0x00720BB0`. The CD-check branch remains
    /// outside this port; ordinary loaded-song controls execute the body in
    /// tools/storage_oracle/sound_theme_metadata.
    fn play_song(
        &mut self,
        index: i32,
        gates: ThemeGates,
        physical: MusicOutputState,
        prepare: &mut impl FnMut(&str) -> Option<PreparedTrack>,
    ) -> ThemeAction {
        if !self.admitted(gates) {
            return ThemeAction::default();
        }
        let mut action = ThemeAction::default();
        if physical.stream_exists() {
            let playing;
            (playing, action) = self.still_playing(physical);
            if playing && self.slots.retained == index {
                return action;
            }
        }
        // Inline Stop(0).
        if self.slots.active != THEME_NONE {
            self.slots = ThemeSlots::default();
            action.stop_output = true;
        }
        self.fading = false;
        if index == THEME_NONE || index == THEME_HOLD {
            return action;
        }
        // Signed volume720C8E is checked after same-song admission, inline
        // Stop and sentinels. A muted request only replaces Pending.
        if index >= 0 && self.score_volume > 0 {
            self.slots.retained = index;
            if physical.stream_exists() {
                let stem = self.entry(index).map(|entry| entry.sound.clone());
                match stem.as_deref().and_then(prepare) {
                    Some(prepared) => {
                        action.start = Some(prepared);
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

/// Resolve a track for optional physical submission; a WAV song decodes only
/// its first piece here (see [`PreparedTrack`]).
fn prepare_track(track_name: &str, assets: &AssetManager) -> Option<PreparedTrack> {
    for filename in [format!("{track_name}.wav"), format!("{track_name}.aud")] {
        let Some(file) = assets.get_shared(&filename) else {
            continue;
        };

        if let Some(track) = PreparedTrack::streamed(track_name, &file) {
            return Some(track);
        }

        let Some((header, samples)) = aud_file::decode_aud(&file) else {
            continue;
        };
        if samples.is_empty() || header.sample_rate == 0 {
            log::warn!("Track {track_name} decoded to 0 samples");
            return None;
        }
        let mut stereo = Vec::new();
        upmix_i16_to_f32_stereo(&mut stereo, &samples, header.channels());
        return Some(PreparedTrack::decoded(
            track_name.to_string(),
            stereo,
            u32::from(header.sample_rate),
        ));
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

/// Native catalog scan `0x007207F0`: WAV availability and declared metadata
/// supply the playlist row; the shared player owns subsequent decoding.
fn scan_catalog_availability(entries: &mut [ThemeEntry], assets: &AssetManager) {
    for entry in entries {
        let wav = (!entry.sound.is_empty())
            .then(|| assets.get_ref(&format!("{}.wav", entry.sound)))
            .flatten();
        entry.available = wav.is_some();
        entry.duration_seconds = wav
            .and_then(crate::assets::wav_file::WavFile::parse)
            .and_then(|wav| wav.native_duration_seconds())
            .unwrap_or(0);
    }
}

/// Installer diagnostics use the playlist's own catalog and availability scan,
/// without initializing an audio device or decoding whole songs.
pub(crate) fn missing_install_music(assets: &AssetManager) -> Vec<String> {
    let mut missing = Vec::new();
    for name in [THEME_INI_NAME, RA2_THEME_INI_NAME] {
        let selected = match crate::rules::retail_sources::select_ini(assets, name) {
            Ok(selected) => selected,
            Err(error) => {
                if name == THEME_INI_NAME {
                    missing.push(format!("{name}: {error}"));
                }
                continue;
            }
        };
        let mut entries = catalog_from_ini(&selected.ini);
        scan_catalog_availability(&mut entries, assets);
        for entry in entries {
            if entry.sound.is_empty() {
                continue;
            }
            let wav = format!("{}.wav", entry.sound);
            if !entry.available {
                let issue = format!("{wav}: music track unavailable");
                if !missing.contains(&issue) {
                    missing.push(issue);
                }
            }
        }
    }
    missing
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
    // Each entry through ReadString(0x20) (`0x007205DE`).
    for entry_name in themes.keys() {
        let Some(key) = themes.read_name(entry_name, 0x20) else {
            continue;
        };
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
        // `0x00720480`: ReadString 0x100 over an empty default, then
        // ReadInt/ReadBool over each field and the side-name read
        // (`0x004756F0`, 0x80 bytes).
        entry.sound = section
            .read_string("Sound", "", 0x100)
            .trim_start_matches(['$', '#'])
            .to_string();
        entry.scenario = section.read_int("Scenario", entry.scenario);
        entry.normal = section.read_bool("Normal", entry.normal);
        entry.repeat = section.read_bool("Repeat", entry.repeat);
        if let Some(side) = section.read_name("Side", 0x80) {
            entry.side_name = Some(side.to_string());
        }
        if entry.normal {
            // The string-table label read `0x00529160` copies up to 0x3FF bytes.
            entry.name_key = section.read_string("Name", "", 0x400);
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::rng::SimRng;

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
        Some(PreparedTrack::decoded(
            stem.to_string(),
            vec![0.0, 0.0],
            22_050,
        ))
    }

    fn no_draw(_low: i32, _high: i32) -> i32 {
        panic!("this Theme path must not draw Main")
    }

    #[test]
    fn entering_wol_cuts_intro_and_shuffles_until_leaving() {
        let mut theme = stock_runtime();
        let mut ok = |stem: &str| prepared(stem);
        theme.play_song(0, gates(true), MusicOutputState::Idle, &mut ok);
        assert_eq!(theme.current_song(), 0, "INTRO plays");
        let (action, saved) = theme.enter_wol_lobby(true, gates(true), MusicOutputState::Playing);
        assert!(!saved, "stock IsScoreShuffle is off");
        // Queue(-3) started a fade on INTRO; Stop(0) then stops the stream
        // at once and, like native, leaves the fading flag for Play to clear.
        assert!(
            action.stop_output,
            "Stop(0) cuts INTRO without waiting for the fade"
        );
        assert_eq!(theme.slots().pending, THEME_AUTO);
        // Theme AI starts a shuffled normal track, never INTRO.
        let mut main = SimRng::new(0);
        let started = theme.ai(
            gates(true),
            MusicOutputState::Idle,
            &mut ok,
            &mut |low, high| main.next_range_i32_inclusive(low, high),
        );
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
        theme.play_song(0, gates(true), MusicOutputState::Idle, &mut ok);
        let (action, _) = theme.enter_wol_lobby(false, gates(true), MusicOutputState::Playing);
        assert!(!action.stop_output);
        assert_eq!(theme.slots().pending, THEME_HOLD);
    }

    #[test]
    fn wol_music_rule_keeps_a_playing_song_and_holds_without_lobby_music() {
        // 0x0077E110 and WOL Options' Play Music preview (0x0078EBC0 0x5F8).
        let mut theme = stock_runtime();
        let mut ok = |stem: &str| prepared(stem);
        theme.play_song(1, gates(true), MusicOutputState::Idle, &mut ok);
        let action = theme.wol_lobby_music_rule(true, gates(true), MusicOutputState::Playing);
        assert!(!action.stop_output, "a playing non-INTRO song stays");
        assert_eq!(theme.current_song(), 1);
        let action = theme.wol_lobby_music_rule(false, gates(true), MusicOutputState::Playing);
        assert!(!action.stop_output);
        assert_eq!(theme.slots().pending, THEME_HOLD, "Queue(-3)");
        // Checked again while the song still plays and Queue(-3)'s fade is
        // still moving: nothing changes.
        theme.wol_lobby_music_rule(true, gates(true), MusicOutputState::Ramping);
        assert_eq!(theme.current_song(), 1);
        // No stream plays: Stop(0), then Queue(-2).
        let action = theme.wol_lobby_music_rule(true, gates(true), MusicOutputState::Idle);
        assert!(action.stop_output);
        assert_eq!(theme.slots().pending, THEME_AUTO);
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

    /// Native catalog fields and indices remain authoritative when the VERA
    /// presentation extension appends available RA2 gameplay entries. Scan
    /// `0x007207F0` marks availability from `Sound + ".WAV"`.
    #[test]
    fn ra2_catalog_appends_available_gameplay_without_overwriting_yr_entries() {
        let dir =
            std::env::temp_dir().join(format!("vera20k-theme-catalog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("test asset dir");
        std::fs::write(
            dir.join("theme.ini"),
            "[Themes]\n1=Grinder\n2=drok\n3=intro\n4=Unavailable\n5=Hidden\n6=Silent\n\
             [Grinder]\nSound=Grinder\nName=THEME:Grinder\nNormal=yes\n\
             [drok]\nSound=Grinder\nNormal=yes\nRepeat=yes\n\
             [intro]\nSound=Grinder\nNormal=yes\nRepeat=no\n\
             [Unavailable]\nSound=Absent\n[Hidden]\nSound=Grinder\nNormal=no\n\
             [Silent]\nName=THEME:Silent\n",
        )
        .expect("write theme.ini");
        std::fs::write(
            dir.join("thememd.ini"),
            "[Themes]\n1=INTRO\n2=Drok\n[INTRO]\nSound=Drok\nNormal=no\nRepeat=yes\n\
             [Drok]\nSound=Drok\nName=THEME:Drok\nNormal=yes\n",
        )
        .expect("write thememd.ini");
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/sound_theme_metadata.json",
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
        assert_eq!(keys, ["INTRO", "Drok", "Grinder"]);
        assert_eq!(theme.from_name("Grinder"), 2);
        assert_eq!(theme.entries()[0].sound, "Drok");
        assert_eq!(theme.entries()[1].sound, "Drok");
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
        assert_eq!(theme.entries()[2].name_key, "THEME:Grinder");
        assert!(
            theme.is_allowed(2),
            "RA2 reaches the existing playlist owner"
        );

        // Missing RA2 metadata leaves the original YR catalog intact.
        std::fs::remove_file(dir.join("theme.ini")).expect("remove theme.ini");
        let assets = AssetManager::from_loose_root_for_test(&dir);
        let mut theme = ThemeRuntime::default();
        theme.initialize_catalog(&assets);
        assert_eq!(theme.entries().len(), 2);

        // Both optional inputs missing leave an empty catalog.
        std::fs::remove_file(dir.join("thememd.ini")).expect("remove thememd.ini");
        let assets = AssetManager::from_loose_root_for_test(&dir);
        let mut theme = ThemeRuntime::default();
        theme.initialize_catalog(&assets);
        assert!(theme.entries().is_empty());
        std::fs::remove_dir_all(dir).expect("remove theme test assets");
    }

    /// `StreamPlayer__PlayFile @ 0x00407B60` streams: preparing a song decodes
    /// only its first piece, and pulling it yields exactly the whole-file
    /// decode across block boundaries, the flushed short tail, the stereo
    /// interleave and upmixed PCM runs, ending only after the last sample.
    #[test]
    fn a_prepared_song_decodes_one_piece_and_streams_the_whole_decode() {
        let dir = std::env::temp_dir().join(format!("vera20k-theme-stream-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("test asset dir");
        let wav = |tag: u16, channels: u16, block_align: u16, bits: u16, data: &[u8]| {
            let mut file = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0".to_vec();
            file.extend(tag.to_le_bytes());
            file.extend(channels.to_le_bytes());
            file.extend(22_050u32.to_le_bytes());
            file.extend(0u32.to_le_bytes());
            file.extend(block_align.to_le_bytes());
            file.extend(bits.to_le_bytes());
            file.extend(b"data");
            file.extend((data.len() as u32).to_le_bytes());
            file.extend(data);
            file
        };
        // Stereo IMA: three 24-byte blocks (a valid preamble pair, then two
        // 8-byte L/R groups: 17 frames) and a 12-byte tail flushed as a block.
        let mut ima = Vec::new();
        for block in 0..3u8 {
            ima.extend([0x00, 0x10 * (block + 1), 10, 0, 0x00, 0xF0, 20, 0]);
            ima.extend((0..16u8).map(|i| block.wrapping_mul(37).wrapping_add(i * 11)));
        }
        ima.extend([0x34, 0x12, 30, 0, 0xCD, 0xAB, 40, 0, 0x5A, 0xA5, 0x3C, 0xC3]);
        // Mono 16-bit PCM past one 4096-byte run, with an odd trailing byte.
        let pcm: Vec<u8> = (0..5_001u32).map(|i| (i * 7) as u8).collect();
        for (name, file, first_piece, total) in [
            ("Ima", wav(0x11, 2, 24, 4, &ima), 34, 4 * 34),
            ("Pcm", wav(1, 1, 2, 16, &pcm), 2 * 2048, 2 * 2500),
        ] {
            std::fs::write(dir.join(format!("{name}.wav")), &file).expect("write wav");
            let assets = AssetManager::from_loose_root_for_test(&dir);
            let mut track = prepare_track(name, &assets).expect("song prepares");
            assert_eq!(track.pcm.len(), first_piece, "{name} decodes one piece");
            let whole = crate::audio::sfx::decode_wav(&file, name).unwrap().samples;
            assert_eq!(whole.len(), total);
            for (index, &want) in whole.iter().enumerate() {
                assert!(!track.exhausted(), "{name} ended early at {index}");
                assert_eq!(track.next(), Some(want), "{name} sample {index}");
            }
            assert!(track.exhausted());
            assert_eq!(track.next(), None);
        }
        std::fs::remove_dir_all(dir).expect("remove theme test assets");
    }

    #[test]
    #[ignore = "slow; run when changing what it covers"]
    fn retail_ra2_music_reaches_catalog_and_shared_decoder() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let mut theme = ThemeRuntime::default();
        theme.initialize_catalog(&assets);
        let yr_keys = [
            "INTRO",
            "SCORE",
            "LOADING",
            "CREDITS",
            "RA2Options",
            "BrainFreeze",
            "Drok",
            "Deceiver",
            "PhatAttack",
            "BullyKit",
            "DefendTheBase",
            "Tactics",
            "TranceLVania",
        ];
        let ra2_keys = [
            "Grinder",
            "Power",
            "Fortification",
            "InDeep",
            "Tension",
            "EagleHunter",
            "IndustroFunk",
            "200Meters",
            "BlowItUp",
            "Destroy",
            "Burn",
            "Motorized",
            "HM2",
        ];
        let keys: Vec<&str> = theme
            .entries()
            .iter()
            .map(|entry| entry.key.as_str())
            .collect();
        assert_eq!(&keys[..yr_keys.len()], &yr_keys);
        assert_eq!(&keys[yr_keys.len()..], &ra2_keys);
        assert_eq!(
            theme
                .entries()
                .iter()
                .enumerate()
                .filter(|(index, _)| theme.is_allowed(*index as i32))
                .count(),
            21,
        );
        assert_eq!(theme.entries()[0].sound, "Drok");
        assert_eq!(theme.entries()[1].sound, "ScoreX");
        assert_eq!(theme.entries()[2].sound, "Bully");
        assert_eq!(theme.entries()[3].sound, "OptionX");
        let csf = crate::assets::csf_file::CsfFile::from_bytes(
            assets.get_ref("ra2md.csf").expect("retail string table"),
        )
        .expect("retail CSF parses");

        for (offset, key) in ra2_keys.iter().enumerate() {
            let index = yr_keys.len() + offset;
            let entry = &theme.entries()[index];
            assert_eq!(theme.from_name(key), index as i32);
            assert!(
                theme.is_allowed(index as i32),
                "{key} reaches the Sound list"
            );
            assert!(entry.duration_seconds > 0, "{key} has retail WAV duration");
            assert!(
                csf.get(&entry.name_key).is_some(),
                "{key} has its retail display name"
            );
            assert_eq!(
                assets
                    .resolve_ref(&format!("{}.wav", entry.sound))
                    .unwrap()
                    .source_archive,
                "theme.mix",
                "{key} resolves from original RA2 music",
            );
            let prepared = prepare_track(&entry.sound, &assets).expect("RA2 music decodes");
            assert_eq!(prepared.sample_rate, 22_050);
            let file = assets.get_ref(&format!("{}.wav", entry.sound)).unwrap();
            let whole = crate::audio::sfx::decode_wav(file, key).unwrap().samples;
            assert!(prepared.eq(whole), "{key} streams its whole decode");
        }
        let intro = assets
            .resolve_ref("Drok.wav")
            .expect("YR INTRO remains available");
        assert_eq!(intro.source_archive, "thememd.mix");
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
            ThemeAllowContext {
                local_side: Some(0),
                campaign_scenario: None,
            },
            |name| (name == "Soviet").then_some(1),
        );
        assert!(!theme.is_allowed(7));
        assert!(theme.is_allowed(8), "Scenario= ignored outside campaign");
        theme.begin_scenario(
            ThemeAllowContext {
                local_side: Some(1),
                campaign_scenario: Some(2),
            },
            |name| (name == "Soviet").then_some(1),
        );
        assert!(theme.is_allowed(7));
        assert!(!theme.is_allowed(8), "campaign scenario 2 < Scenario=3");
        theme.begin_scenario(
            ThemeAllowContext {
                local_side: Some(1),
                campaign_scenario: None,
            },
            |_| None,
        );
        assert!(!theme.is_allowed(7), "unknown Side= never matches");
        theme.begin_scenario(ThemeAllowContext::default(), |_| None);
        assert!(
            theme.is_allowed(7),
            "no player (shell) skips the Side= gate"
        );
    }

    #[test]
    fn cyclic_next_song_starts_after_prev_and_honors_repeat() {
        let mut theme = stock_runtime();
        assert_eq!(
            theme.next_song(-1, &mut no_draw),
            5,
            "first allowed after -1 is BrainFreeze"
        );
        assert_eq!(theme.next_song(5, &mut no_draw), 6);
        assert_eq!(
            theme.next_song(8, &mut no_draw),
            5,
            "wraps past the shell-only entries"
        );
        assert_eq!(theme.next_song(0, &mut no_draw), 0, "INTRO repeats");
        theme.set_score_options(true, false);
        assert_eq!(
            theme.next_song(5, &mut no_draw),
            5,
            "global repeat returns prev"
        );
        let empty = ThemeRuntime::with_entries(Vec::new());
        assert_eq!(empty.next_song(-1, &mut no_draw), 0);
    }

    /// Borrow one already advanced Main cursor, including native ranged
    /// rejection and both lag-index wraps. These goldens execute RandomRanged
    /// 65C7E0 and raw 65C780; they establish RNG continuation, not the complete
    /// Theme Next_Song chooser. All supplied catalog entries are allowed.
    #[test]
    fn theme_shuffle_continues_the_callers_native_main_cursor() {
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/rmg_oracle/vectors/rng.json",
        ))
        .unwrap();
        let cases = fixture["ranged_cases"].as_array().unwrap();
        assert_eq!(cases.len(), 7);
        for case in cases {
            let name = case["id"].as_str().unwrap();
            let step = &case["steps"][0];
            assert_eq!(step["kind"], "ranged", "{name}");
            let low = step["low"].as_i64().unwrap() as i32;
            let high = step["high"].as_i64().unwrap() as i32;
            assert_eq!(low, 0, "{name}");
            let entries = (0..=high)
                .map(|index| ThemeEntry {
                    key: format!("Fixture{index}"),
                    sound: String::new(),
                    name_key: String::new(),
                    duration_seconds: 0,
                    scenario: 0,
                    normal: true,
                    repeat: false,
                    side_name: None,
                    side: -1,
                    available: true,
                })
                .collect();
            let mut theme = ThemeRuntime::with_entries(entries);
            theme.set_score_options(false, true);
            let before = case["initial_state_hex"].as_str().unwrap();
            assert_eq!(before, step["before_state_hex"].as_str().unwrap(), "{name}");
            let mut main = SimRng::from_native_state_hex_for_test(before);
            theme.begin_scenario(ThemeAllowContext::default(), |_| None);
            let mut ranges = Vec::new();
            let (song, raw) = crate::sim::rng::trace_draws(|| {
                theme.next_song(THEME_NONE, &mut |draw_low, draw_high| {
                    ranges.push((draw_low, draw_high));
                    main.next_range_i32_inclusive(draw_low, draw_high)
                })
            });
            assert_eq!(ranges, [(low, high)], "{name}");
            assert_eq!(song, step["result"].as_i64().unwrap() as i32, "{name}");
            let raw_words: Vec<_> = raw.iter().map(|draw| draw["value"].clone()).collect();
            assert_eq!(raw_words, *step["raw_draws"].as_array().unwrap(), "{name}");
            assert_eq!(
                main.native_state_hex(),
                step["after_state_hex"].as_str().unwrap(),
                "{name} after Theme"
            );

            // Re-entering scenario context cannot seed or replace the
            // caller's process stream. The next independent raw consumer
            // must observe the original continuation of Theme's draw.
            theme.begin_scenario(
                ThemeAllowContext {
                    local_side: Some(1),
                    campaign_scenario: Some(3),
                },
                |_| None,
            );
            let continuation = &case["steps"][1];
            assert_eq!(continuation["kind"], "raw", "{name}");
            assert_eq!(
                main.native_state_hex(),
                continuation["before_state_hex"].as_str().unwrap(),
                "{name} context change"
            );
            assert_eq!(
                main.next_u32(),
                continuation["result"].as_u64().unwrap() as u32,
                "{name} caller continuation"
            );
            assert_eq!(
                main.native_state_hex(),
                continuation["after_state_hex"].as_str().unwrap(),
                "{name} after caller continuation"
            );
        }
    }

    /// Original Theme ctor720960, complete NextSong720A80 and IsAllowed721140,
    /// with explicitly authored catalog flags. Native raw65C780 and ranged65C7E0
    /// run on one Main object; this compares every range boundary and the next
    /// independent raw consumer, including the 1000-attempt fallback.
    #[test]
    fn next_song_matches_original_catalog_gates_and_shared_main_continuation() {
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/input_oracle/unit_voice_playback.json",
        ))
        .unwrap();
        let rows = fixture["theme_cases"].as_array().unwrap();
        assert_eq!(rows.len(), 9);
        for row in rows {
            let name = row["name"].as_str().unwrap();
            let input = &row["input"];
            let state = |reference: &serde_json::Value| {
                row["complete_rng_states"][reference.as_str().unwrap()]["bytes"]
                    .as_str()
                    .unwrap()
            };
            let entries = input["entries"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(index, entry)| ThemeEntry {
                    key: format!("Native{index}"),
                    sound: String::new(),
                    name_key: String::new(),
                    duration_seconds: 0,
                    scenario: entry["scenario"].as_i64().unwrap() as i32,
                    normal: entry["normal"].as_bool().unwrap(),
                    repeat: entry["repeat"].as_bool().unwrap(),
                    side_name: None,
                    side: entry["side"].as_i64().unwrap() as i32,
                    available: entry["available"].as_bool().unwrap(),
                })
                .collect();
            let mut theme = ThemeRuntime::with_entries(entries);
            theme.set_score_options(
                input["repeat"].as_bool().unwrap(),
                input["shuffle"].as_bool().unwrap(),
            );
            // The native fixture supplies already-resolved Side indices.
            theme.allow_context = ThemeAllowContext {
                local_side: Some(input["current_side"].as_i64().unwrap() as i32),
                campaign_scenario: (input["game_mode"] == 0)
                    .then(|| input["scenario_number"].as_i64().unwrap() as i32),
            };
            let mut main = SimRng::from_native_state_hex_for_test(state(&row["main_before"]));
            let native_ranges = row["ranged_draws"].as_array().unwrap();
            let mut range_index = 0;
            let (returned, raw) = crate::sim::rng::trace_draws(|| {
                theme.next_song(
                    input["previous"].as_i64().unwrap() as i32,
                    &mut |low, high| {
                        let expected = native_ranges.get(range_index).unwrap_or_else(|| {
                            panic!("{name}: unexpected range call {range_index}")
                        });
                        assert_eq!(
                            serde_json::json!([low, high]),
                            expected["args"],
                            "{name} range {range_index}"
                        );
                        assert_eq!(
                            main.native_state_hex(),
                            expected["state_before"].as_str().unwrap(),
                            "{name} before range {range_index}"
                        );
                        let result = main.next_range_i32_inclusive(low, high);
                        assert_eq!(
                            result,
                            expected["result"].as_i64().unwrap() as i32,
                            "{name} range {range_index}"
                        );
                        assert_eq!(
                            main.native_state_hex(),
                            expected["state_after"].as_str().unwrap(),
                            "{name} after range {range_index}"
                        );
                        range_index += 1;
                        result
                    },
                )
            });
            assert_eq!(returned, row["returned"].as_i64().unwrap() as i32, "{name}");
            assert_eq!(range_index, native_ranges.len(), "{name} ranged count");
            assert_eq!(
                raw.iter().map(|draw| &draw["value"]).collect::<Vec<_>>(),
                row["raw_draws"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|draw| &draw["raw"])
                    .collect::<Vec<_>>(),
                "{name} raw rejection/continuation words"
            );
            assert_eq!(
                main.native_state_hex(),
                state(&row["main_after"]),
                "{name} final Theme Main"
            );

            theme.begin_scenario(ThemeAllowContext::default(), |_| None);
            assert_eq!(
                main.native_state_hex(),
                state(&row["main_after"]),
                "{name} context cannot reseed caller"
            );
            assert_eq!(
                main.next_u32(),
                row["continuation"]["returned"].as_u64().unwrap() as u32,
                "{name} next Main consumer"
            );
            assert_eq!(
                main.native_state_hex(),
                state(&row["continuation"]["main_after"]),
                "{name} continuation state"
            );
        }
    }

    #[test]
    fn start_scenario_hand_off_loading_then_fade_then_first_allowed() {
        let mut theme = stock_runtime();
        let mut ok = prepared;
        // Shell: INTRO playing (Play sets pending = INTRO because Repeat=yes).
        let action = theme.play_song(0, gates(true), MusicOutputState::Idle, &mut ok);
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
        let action = theme.play_song(loading, gates(true), MusicOutputState::Playing, &mut ok);
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
        let action = theme.stop(gates(true), true, MusicOutputState::Playing);
        assert!(!action.stop_output && action.fade_out);
        assert!(theme.fading);
        assert_eq!(theme.slots(), ThemeSlots::default());

        // Main_Tick: nothing retained -> Queue(-2) (no fade for -2).
        theme.main_tick(true, gates(true), MusicOutputState::Playing);
        assert_eq!(theme.slots().pending, THEME_AUTO);

        // AI waits while the stream's own interp is still moving.
        let action = theme.ai(
            gates(true),
            MusicOutputState::Ramping,
            &mut ok,
            &mut no_draw,
        );
        assert!(!action.stop_output && action.start.is_none());

        // Fade reached target: stop the stream, Next_Song(-1) = BrainFreeze.
        let action = theme.ai(
            gates(true),
            MusicOutputState::Playing,
            &mut ok,
            &mut no_draw,
        );
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
        let action = theme.ai(
            gates(true),
            MusicOutputState::Finished,
            &mut ok,
            &mut no_draw,
        );
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
        theme.play_song(2, gates(true), MusicOutputState::Idle, &mut ok);
        let action = theme.queue_song(7, gates(true), MusicOutputState::Playing);
        assert!(!action.stop_output);
        assert!(theme.fading);
        assert_eq!(theme.slots().pending, 7);
        let action = theme.ai(
            gates(true),
            MusicOutputState::Playing,
            &mut ok,
            &mut no_draw,
        );
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
        theme.play_song(5, gates(true), MusicOutputState::Idle, &mut ok);
        theme.main_tick(false, gates(true), MusicOutputState::Playing);
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
        theme.main_tick(false, gates(true), MusicOutputState::Playing);
        assert_eq!(theme.slots(), before);
        let action = theme.ai(
            gates(true),
            MusicOutputState::Playing,
            &mut ok,
            &mut no_draw,
        );
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
            theme.queue_song(request, gates(true), MusicOutputState::Playing);
            assert_eq!(theme.slots().pending, request);
            assert!(!theme.fading);
        }
        let action = theme.queue_song(THEME_HOLD, gates(true), MusicOutputState::Playing);
        assert!(theme.fading && action.fade_out);
    }

    #[test]
    fn play_same_retained_while_playing_is_a_no_op_and_failed_gates_preserve_slots() {
        let mut theme = stock_runtime();
        let mut ok = prepared;
        theme.play_song(5, gates(true), MusicOutputState::Idle, &mut ok);
        let before = theme.slots();
        let action = theme.play_song(5, gates(true), MusicOutputState::Playing, &mut ok);
        assert!(action.start.is_none() && !action.stop_output);
        assert_eq!(theme.slots(), before);

        let action = theme.queue_song(7, gates(false), MusicOutputState::Playing);
        assert!(!action.stop_output && action.start.is_none());
        assert_eq!(theme.slots(), before);
        let suppressed = ThemeGates {
            launcher_audio_available: true,
            startup_suppressed: true,
        };
        theme.stop(suppressed, false, MusicOutputState::Playing);
        assert_eq!(theme.slots(), before);
    }

    #[test]
    fn play_sentinels_stop_and_auto_sets_pending() {
        let mut theme = stock_runtime();
        let mut ok = prepared;
        theme.play_song(5, gates(true), MusicOutputState::Idle, &mut ok);
        let action = theme.play_song(THEME_NONE, gates(true), MusicOutputState::Playing, &mut ok);
        assert!(action.stop_output);
        assert_eq!(theme.slots(), ThemeSlots::default());
        theme.play_song(5, gates(true), MusicOutputState::Idle, &mut ok);
        let action = theme.play_song(THEME_AUTO, gates(true), MusicOutputState::Playing, &mut ok);
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
    fn ai_ignores_none_hold_and_device_gate_and_restores_auto_after_failure() {
        let mut theme = stock_runtime();
        let mut ok = prepared;
        theme.slots = ThemeSlots {
            active: 5,
            retained: 5,
            pending: THEME_NONE,
        };
        theme.ai(
            gates(true),
            MusicOutputState::Finished,
            &mut ok,
            &mut no_draw,
        );
        assert_eq!(
            theme.slots(),
            ThemeSlots {
                active: 5,
                retained: 5,
                pending: THEME_NONE
            }
        );
        theme.slots.pending = THEME_HOLD;
        theme.ai(
            gates(true),
            MusicOutputState::Finished,
            &mut ok,
            &mut no_draw,
        );
        assert_eq!(theme.slots().pending, THEME_HOLD);
        theme.slots.pending = THEME_AUTO;
        theme.ai(
            gates(false),
            MusicOutputState::Unavailable,
            &mut ok,
            &mut no_draw,
        );
        assert_eq!(
            theme.slots(),
            ThemeSlots {
                active: 5,
                retained: 5,
                pending: THEME_AUTO
            }
        );

        let mut missing = |_stem: &str| None;
        theme.ai(
            gates(true),
            MusicOutputState::Finished,
            &mut missing,
            &mut no_draw,
        );
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
        let action = theme.queue_then_stop_score_zero(gates(true), MusicOutputState::Playing);
        assert!(action.stop_output);
        assert_eq!(theme.slots(), ThemeSlots::default());

        theme.slots = ThemeSlots {
            active: -1,
            retained: 6,
            pending: THEME_HOLD,
        };
        let action = theme.queue_then_stop_score_zero(gates(true), MusicOutputState::Idle);
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
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/sound_theme_metadata.json",
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
    fn sound_play_and_stop_preserve_theme_queue_and_fade_authority() {
        let mut theme = stock_runtime();
        let mut prepare = prepared;
        theme.play_song(5, gates(true), MusicOutputState::Idle, &mut prepare);
        let same = theme.play_selection(5, gates(true), MusicOutputState::Playing);
        assert!(same.start.is_none() && !same.stop_output && !theme.fading);
        assert_eq!(theme.slots.active, 5);

        let next = theme.play_selection(6, gates(true), MusicOutputState::Playing);
        assert!(next.start.is_none() && !next.stop_output && next.fade_out);
        assert!(theme.fading);
        assert_eq!(
            theme.slots,
            ThemeSlots {
                active: -1,
                retained: -1,
                pending: 6
            }
        );
        let during = theme.ai(
            gates(true),
            MusicOutputState::Ramping,
            &mut prepare,
            &mut no_draw,
        );
        assert!(during.start.is_none() && !during.stop_output);
        let finished = theme.ai(
            gates(true),
            MusicOutputState::Playing,
            &mut prepare,
            &mut no_draw,
        );
        assert!(finished.stop_output);
        assert_eq!(
            finished.start.as_ref().map(|track| track.stem.as_str()),
            Some("Drok")
        );
        assert_eq!(theme.current_song(), 6);

        let stopped = theme.stop_selection(gates(true), MusicOutputState::Playing);
        assert!(!stopped.stop_output && stopped.start.is_none());
        assert_eq!(theme.current_song(), THEME_HOLD);
        theme.main_tick(false, gates(true), MusicOutputState::Playing);
        let finished = theme.ai(
            gates(true),
            MusicOutputState::Playing,
            &mut prepare,
            &mut no_draw,
        );
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

        theme.queue_song(7, gates(true), MusicOutputState::Playing);
        theme.scenario_owns_pending = true;
        theme.cancel_scenario_theme_request();
        assert_eq!(theme.slots().pending, THEME_NONE);
    }
}
