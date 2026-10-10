//! SOUNDMD.INI and EVAMD.INI readers for process audio definitions.
//!
//! SOUNDMD.INI has sections like:
//! ```ini
//! [VGCannon1]
//! Sounds=vgcannon.wav
//! Volume=100
//! Priority=1
//! Control=random
//! Type=global
//! ```
//!
//! Each section name is a sound ID referenced by weapons (Report=), units
//! (VoiceSelect=, VoiceMove=, DieSound=), and EVA announcements.
//!
//! gamemd-derived: `VocClass::ReadSoundListINI @ 0x007510D0` walks
//! `[SoundList]` by entry index and registers one sound per entry VALUE; the
//! INI's sections are never scanned. `VocClass::ReadINI @ 0x00750440` then
//! reads the fourteen keys below out of the section named by that value, in
//! this order, with these defaults —
//! `Sounds` (""), `Volume` ([Defaults] float, static 80.0), `VShift` (0),
//! `MinVolume` ([Defaults] float, static 20.0), `Priority` ("NORMAL"),
//! `Attack` (0), `Decay` (0), `Control` ([Defaults] flags, static 0), `Type`
//! ([Defaults] flags, static SCREEN), `Limit` ([Defaults], static 5), `Loop`
//! (0), `Range` ([Defaults], static 10), `Delay` (""), `FShift` (""). The
//! `[Defaults]` section itself is read once by `VocClass::ReadSoundListINI @
//! 0x007510D0` (`0x00751126..0x0075128C`). Token lists are split by
//! `CRT strtok` on the delimiter string at `0x00846570` = `" \t\n"` — commas
//! are NOT separators.
//!
//! ## Dependency rules
//! - Part of rules/ — depends on rules/ini_parser. No sim/render dependencies.

use std::collections::HashMap;

use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::ini_value::{crt_atoi, strtok};

/// `Control=` flag words. gamemd-derived: the `(char*, u32)` table at
/// `0x008160C0` walked by `AudioEventClass::ParseControlFlag @ 0x00406820`
/// (strings read via `inspect_memory_content 0x00816100`). The compare is
/// `FUN_007C8D20`, a case-insensitive ASCII compare; an unmatched token lands
/// on the NULL terminator whose value is 0, so it ORs nothing.
pub mod control {
    pub const LOOP: u32 = 0x01;
    pub const RANDOM: u32 = 0x02;
    pub const ALL: u32 = 0x04;
    pub const PREDELAY: u32 = 0x08;
    pub const INTERRUPT: u32 = 0x10;
    pub const ATTACK: u32 = 0x20;
    pub const DECAY: u32 = 0x40;
    pub const AMBIENT: u32 = 0x80;
}

/// `Type=` flag words. gamemd-derived: the table at `0x00816048` walked by
/// `AudioEventClass::ParseTypeFlag @ 0x00406870`. `NORMAL` is a real entry
/// carrying 0. SCREEN/LOCAL (`0x60`) and UNSHROUD/SHROUD (`0xC00`) are
/// exclusive pairs: matching one clears the other member first.
pub mod sound_type {
    pub const NORMAL: u32 = 0x0;
    pub const VIOLENT: u32 = 0x01;
    pub const MOVEMENT: u32 = 0x02;
    pub const QUIET: u32 = 0x04;
    pub const LOUD: u32 = 0x08;
    pub const GLOBAL: u32 = 0x10;
    pub const SCREEN: u32 = 0x20;
    pub const LOCAL: u32 = 0x40;
    pub const PLAYER: u32 = 0x80;
    pub const NOISE_SHY: u32 = 0x100;
    pub const GUN_SHY: u32 = 0x200;
    pub const UNSHROUD: u32 = 0x400;
    pub const SHROUD: u32 = 0x800;
    pub const AMBIENT: u32 = 0x1000;
}

const CONTROL_TABLE: [(&str, u32); 8] = [
    ("ALL", control::ALL),
    ("LOOP", control::LOOP),
    ("RANDOM", control::RANDOM),
    ("PREDELAY", control::PREDELAY),
    ("INTERRUPT", control::INTERRUPT),
    ("ATTACK", control::ATTACK),
    ("DECAY", control::DECAY),
    ("AMBIENT", control::AMBIENT),
];

const TYPE_TABLE: [(&str, u32); 14] = [
    ("AMBIENT", sound_type::AMBIENT),
    ("VIOLENT", sound_type::VIOLENT),
    ("MOVEMENT", sound_type::MOVEMENT),
    ("QUIET", sound_type::QUIET),
    ("LOUD", sound_type::LOUD),
    ("GLOBAL", sound_type::GLOBAL),
    ("SCREEN", sound_type::SCREEN),
    ("LOCAL", sound_type::LOCAL),
    ("PLAYER", sound_type::PLAYER),
    ("NORMAL", sound_type::NORMAL),
    ("GUN_SHY", sound_type::GUN_SHY),
    ("NOISE_SHY", sound_type::NOISE_SHY),
    ("UNSHROUD", sound_type::UNSHROUD),
    ("SHROUD", sound_type::SHROUD),
];

/// Native linear volume scale: `VolumeInterp` values are `0..=0x4000`.
pub const VOLUME_SCALE: i32 = 0x4000;

/// Sample slots per event: `VocClass::AddSample @ 0x004064A0` refuses the
/// 33rd (`+0x134 == 0x20`).
pub const MAX_SAMPLES: usize = 0x20;

/// Usable bytes in a sound id. `0x007512CA PUSH 0x20` hands
/// `CCINIClass::ReadString @ 0x00528A10` a 32-byte stack buffer, and its tail
/// is `strncpy(dst, value, 0x20); dst[0x1f] = 0; strtrim()`, so a `[SoundList]`
/// value survives as at most 31 bytes. `AudioEventClass::FindOrCreate @
/// 0x004063B0` cuts to the same ceiling again — `0x00406460 PUSH 0x1f` into the
/// zero-filled name field at `+0x6c` — and `VocClass::ReadINI @ 0x00750440`
/// then looks the section up by that **stored** name (`0x00750462 CALL
/// 0x00405170` GetName -> `0x0075046A FindSectionByName`), never by the raw
/// list value.
const MAX_ID_BYTES: usize = 0x1f;

/// The `char[0x800]` buffer `VocClass::ReadINI` and `ReadSoundListINI` read
/// every text key into (`0x007504A5..0x007508B5`, `0x00751187..0x00751214`).
const TEXT_CAPACITY: usize = 0x800;

/// `[Defaults]` values. gamemd-derived: `VocClass::ReadSoundListINI @
/// 0x007510D0` — Volume -> `0x008464B4` (static 80.0f), MinVolume ->
/// `0x008464B8` (static 20.0f), Priority -> `0x008464B0` (static 2),
/// Control -> `0x00B1D3B0` (static 0; reset to 0 before parsing only when a
/// token exists), Type -> `0x008464BC` (static 0x20; reset to 0x20 before
/// parsing only when a token exists), Limit -> `0x008464C4` (static 5),
/// Range -> `0x008464C0` (static 10). Statics read via `read_memory`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoundDefaults {
    pub volume: f32,
    pub min_volume: f32,
    pub priority: u8,
    pub control: u32,
    pub type_flags: u32,
    pub limit: i32,
    pub range: i32,
}

impl Default for SoundDefaults {
    fn default() -> Self {
        Self {
            volume: 80.0,
            min_volume: 20.0,
            priority: SOUND_PRIORITY_DEFAULT,
            control: 0,
            type_flags: sound_type::SCREEN,
            limit: 5,
            range: 10,
        }
    }
}

impl SoundDefaults {
    fn read(section: Option<&IniSection>) -> Self {
        let mut defaults = Self::default();
        let Some(section) = section else {
            return defaults;
        };
        // `VocClass::ReadSoundListINI @ 0x007510D0` stores both into the
        // float32 statics (`0x0075113E FSTP dword [0x008464B4]`,
        // `0x00751161 FSTP dword [0x008464B8]`).
        defaults.volume = section.read_float("Volume", defaults.volume);
        defaults.min_volume = section.read_float("MinVolume", defaults.min_volume);
        defaults.priority = section
            .read_name("Priority", TEXT_CAPACITY)
            .map_or(defaults.priority, parse_sound_priority);
        if let Some(control) = parse_control_list(section.read_name("Control", TEXT_CAPACITY)) {
            defaults.control = control;
        }
        if let Some(type_flags) = parse_type_list(section.read_name("Type", TEXT_CAPACITY)) {
            defaults.type_flags = type_flags;
        }
        defaults.limit = section.read_int("Limit", defaults.limit);
        defaults.range = section.read_int("Range", defaults.range);
        defaults
    }

    /// The minimum-volume floor as the stored fraction (see
    /// [`SoundEntry::min_volume`]).
    pub fn min_volume_fraction(&self) -> f32 {
        min_volume_fraction(f64::from(self.min_volume))
    }
}

/// A single sound definition from sound.ini.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundEntry {
    /// Section name / sound ID (e.g., "VGCannon1").
    pub id: String,
    /// Sample names in `Sounds=` order, `$`/`#` prefixes stripped, at most
    /// [`MAX_SAMPLES`]. `Attack=` counts the first N as attack samples and
    /// `Decay=` the last M as decay samples; the rest is the body.
    pub sounds: Vec<String>,
    /// Playback volume in percent, `0..=100`. VERA-internal, gamemd
    /// equivalent UNCHECKED: native keeps no percentage — `ReadINI` converts
    /// straight to [`Self::volume_linear`] and throws the percentage away.
    /// This rounded copy exists for display and tests and feeds no playback
    /// path.
    pub volume: u8,
    /// Native linear volume `ftol(clamp(Volume * 0.01f, 0, 1) * 16384f)` as
    /// stored by `AudioEventClass::SetVolumeRamp @ 0x00406550`
    /// (`0x007504EE..0x00750548`). The x87 chain truncates, so `Volume=100`
    /// yields 16383, not `0x4000` — only the high clamp reaches `0x4000`. See
    /// [`volume_linear`] for why the whole product must stay in `f64`.
    pub volume_linear: i32,
    /// Eviction priority, `LOWEST(0)`..`CRITICAL(4)`; higher survives longer.
    ///
    /// `sound(md).ini` writes this as a symbolic token, never a number.
    pub priority: u8,
    /// Audible range in cells (default 10). `max audible pixel distance =
    /// Range * 60` in `VocClass::CalcVolumeAndPan @ 0x00750AC0`.
    ///
    /// A full `int`, as `AudioEventClass::SetRange @ 0x004065E0` stores it
    /// (`MOV [ECX+0x50], EDX; RET` — no clamp of any kind).
    pub range: i32,
    /// `MinVolume=` as the stored fraction `clamp(MinVolume * 0.01, 0, 1)`
    /// (`0x00750589..0x007505E3`, `AudioEventClass::SetMinVolume @
    /// 0x004065F0` at `+0x54`). The floor applies only for `Type=GLOBAL`.
    pub min_volume: f32,
    /// `Control=` flag bits ([`control`]), stored by
    /// `AudioEventClass::SetControlFlags @ 0x00406570` at `+0x10`.
    pub control: u32,
    /// `Type=` flag bits ([`sound_type`]), stored at `+0x14`.
    pub type_flags: u32,
    /// `Limit=` concurrent-instance cap (`+0x48`).
    pub limit: i32,
    /// `Loop=` pass count for looping events (`+0x4C`, 0 = forever).
    pub loop_count: i32,
    /// `Delay=` pre-delay range in milliseconds `(min, max)` (`+0x58/+0x5C`);
    /// a single token sets both, no token sets `(0, 0)`. The pair is real — see
    /// [`parse_int_pair`] for why the decompiler's single-value rendering of
    /// this tail is wrong.
    pub delay_ms: (i32, i32),
    /// `FShift=` frequency shift range in percent `(min, max)`
    /// (`+0x60/+0x64`); same one-or-two token rule as `Delay=`.
    pub fshift: (i32, i32),
    /// `VShift=` random volume reduction ceiling in percent, clamped
    /// `0..=100` by `AudioEventClass::SetVShift @ 0x00406620` (`+0x68`).
    pub vshift: i32,
    /// Attack sample count (`+0x138`) after `SetControlFlags` normalisation:
    /// 0 without `Control=ATTACK`, at least 1 with it.
    pub attack: i32,
    /// Decay sample count (`+0x13C`), normalised the same way for `DECAY`.
    pub decay: i32,
}

impl SoundEntry {
    /// The state a listed id keeps when its section does not exist.
    ///
    /// `VocClass::ReadINI @ 0x00750440` looks the section up by the event's own
    /// name (`0x0075046A INIClass::FindSectionByName`) and returns at
    /// `0x00750476` without reading a single key when it is missing, so the
    /// object keeps what `AudioEventClass::FindOrCreate @ 0x004063B0` built:
    /// a zero-filled `0x148` allocation (`0x00406401..0x00406415` runs
    /// `REP STOSD` over all `0x52` dwords of it, twice) plus `+0xC = 1`,
    /// `+0x10 = 0` (Control), `+0x14 = 0x20` (SCREEN), `+0x40 = 2` (NORMAL),
    /// `+0x48 = 3` (Limit), `+0x50 = 10` (Range), `+0x54 = 0.2f` (MinVolume),
    /// and the name `strncpy`d to 31 chars. Those are the **constructor's**
    /// values, not `[Defaults]`' — the `Limit` differs (3 against the static
    /// 5) precisely because `[Defaults]` was never applied here.
    ///
    /// The **volume is full, not zero**: `0x0040641F MOV EDX,0x4000;
    /// 0x00406424 LEA ECX,[ESI+0x18]; 0x00406458 CALL 0x00407100` seeds the
    /// interpolator at `+0x18` — the same subobject `SetVolumeRamp @
    /// 0x00406550` writes (`LEA ESI,[ECX+0x18]`) when `Volume=` is read — and
    /// `0x00407100` is `MOV EDI,EDX; SHL EDI,0x10; MOV [ESI+0x8],EDI`, i.e. the
    /// 16.16 current value becomes `0x4000 << 16` = [`VOLUME_SCALE`]. The
    /// zero-fill is overwritten one instruction later, so an unread entry is at
    /// full volume, not silent-by-volume.
    ///
    /// The sample count `+0x134` stays 0, and `SoundEvent::UpdateState @
    /// 0x004055C0` state 0 abandons the event when it is
    /// (`0x0040563C CMP [EDI+0x134],EBX; JZ 0x004057A3`), so such an id is
    /// registered and permanently silent. Stock `[SoundList]` reaches this once,
    /// through a decorative `============ Mission Disk sounds ============`
    /// entry.
    fn unread(name: &str) -> Self {
        Self {
            id: name.to_string(),
            sounds: Vec::new(),
            // `+0x18` interp seeded to `0x4000` (`0x0040641F`/`0x00406458`).
            volume: 100,
            volume_linear: VOLUME_SCALE,
            priority: SOUND_PRIORITY_DEFAULT,
            range: 10,
            min_volume: 0.2,
            control: 0,
            type_flags: sound_type::SCREEN,
            limit: 3,
            loop_count: 0,
            delay_ms: (0, 0),
            fshift: (0, 0),
            vshift: 0,
            attack: 0,
            decay: 0,
        }
    }

    /// Body sample index range `attack .. count - decay` (may be empty).
    pub fn body_range(&self) -> std::ops::Range<usize> {
        let count = self.sounds.len() as i32;
        let start = self.attack.clamp(0, count);
        let end = (count - self.decay).clamp(start, count);
        start as usize..end as usize
    }
}

/// Registry of all sound definitions, keyed by uppercase sound ID.
#[derive(Debug, Clone, Default)]
pub struct SoundRegistry {
    entries: HashMap<String, SoundEntry>,
    defaults: SoundDefaults,
}

impl SoundRegistry {
    /// Parse the selected SOUNDMD.INI into the process Voc registry.
    ///
    /// gamemd-derived: `VocClass::ReadSoundListINI @ 0x007510D0` reads
    /// `[Defaults]` once, then finds `[SoundList]` (`0x00751298`) and walks it
    /// by index — `GetEntryCount @ 0x00526960`, `GetEntryNameByIndex @
    /// 0x00526CC0`, then `CCINIClass::ReadString @ 0x00528A10` for that
    /// entry's **value**, which is the sound id (`0x007512C6..0x007512EE`). An
    /// entry whose value reads back empty is skipped (`0x007512EC TEST EAX,EAX;
    /// JZ`); every other value becomes an `AudioEventClass` through
    /// `FindOrCreate @ 0x004063B0` and is filled in by `VocClass::ReadINI @
    /// 0x00750440`.
    ///
    /// The INI's sections are **not** scanned, which is why this walks the list
    /// rather than the file: a section `[SoundList]` never names is never
    /// registered (stock `soundmd.ini` has exactly one, `[GuardianGiUnDeploy]`,
    /// and no rules/art key references it), and a listed id whose section is
    /// missing still registers — see [`SoundEntry::unread`].
    ///
    /// Init_Game52C763/52C796 supplies SOUNDMD.INI alone. Source selection
    /// and process lifetime belong to `audio_sources::AudioDefinitions`.
    pub fn from_ini(ini: &IniFile) -> Self {
        let mut entries: HashMap<String, SoundEntry> = HashMap::new();
        let defaults = SoundDefaults::read(ini.section("Defaults"));

        // No `[SoundList]` means no sounds: `0x0075129D TEST EAX,EAX; JZ`
        // leaves the whole registration loop unexecuted.
        let Some(list) = ini.section("SoundList") else {
            log::info!("SoundRegistry: no [SoundList] section, 0 sound definitions");
            return Self { entries, defaults };
        };

        // Values in source order, which is native's entry-index order: each
        // entry name through `ReadString` into `char[32]` (`0x007512CA`). The
        // INI parser has already dropped entries with an empty value, matching
        // the `ReadString` length gate.
        for value in list
            .keys()
            .filter_map(|key| list.read_name(key, MAX_ID_BYTES + 1))
        {
            let key = value.to_ascii_uppercase();
            // `0x007512F4..0x0075133D` compares the id against the names of the
            // events already created (`FUN_007C8D20`, case-insensitive) and,
            // on a hit, hands that same object to `ReadINI` again under its
            // FIRST spelling. `AddSample` appends rather than replaces, so a
            // twice-listed id ends up carrying its sample list twice — stock
            // `[SoundList]` lists `KirovVoiceDie` twice, in both sound files.
            let previous: Option<SoundEntry> = entries.remove(&key);
            let name: &str = previous.as_ref().map_or(value, |entry| entry.id.as_str());
            let mut entry: SoundEntry = read_entry(ini, name, &defaults);
            if let Some(previous) = previous {
                let mut sounds: Vec<String> = previous.sounds;
                sounds.extend(entry.sounds);
                sounds.truncate(MAX_SAMPLES);
                entry.sounds = sounds;
            }
            entries.insert(key, entry);
        }

        log::info!("SoundRegistry: loaded {} sound definitions", entries.len());
        Self { entries, defaults }
    }

    /// Look up a sound entry by ID (case-insensitive).
    pub fn get(&self, sound_id: &str) -> Option<&SoundEntry> {
        self.entries.get(&sound_id.to_ascii_uppercase())
    }

    /// Read a Rules sound reference against the fixed SOUNDMD catalog.
    /// TechnoType712FF1/7130A5 and Rules6699C8 use ReadString128 followed by
    /// Voc FindIndex7514D0. Missing, empty and unknown names retain the
    /// previous signed ID (constructor -1); later rules passes do not erase
    /// an earlier valid selection. Names here represent that resolved ID.
    pub(crate) fn read_rules_reference(&self, section: &IniSection, key: &str) -> Option<String> {
        section.read_string_with(key, 0x80, None, |current, name| {
            self.get(name).map(|entry| entry.id.clone()).or(current)
        })
    }

    /// ReadSoundList525430 reads at most127 bytes, tokenizes on commas, then
    /// resolves each token through Voc FindPtr751520/FindIndex7515C0. Unknown
    /// tokens and FindPtr's reserved `<none>` are skipped, duplicates retained.
    /// Bare `none` still resolves if registered. The processed Rules section
    /// owns reached pass order: empty/missing reads retain the prior vector;
    /// a nonempty read replaces it, even when no token resolves.
    pub(crate) fn read_rules_sound_list(&self, section: &IniSection, key: &str) -> Vec<String> {
        section
            .read_sound_list(key)
            .unwrap_or_default()
            .into_iter()
            .filter(|name| !name.eq_ignore_ascii_case("<none>"))
            .filter_map(|name| self.get(name).map(|entry| entry.id.clone()))
            .collect()
    }

    /// The `[Defaults]` values this registry was read with.
    pub fn defaults(&self) -> &SoundDefaults {
        &self.defaults
    }

    /// Total number of registered sound definitions.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// One `VocClass::ReadINI @ 0x00750440` pass for `name`: read the fourteen keys
/// out of `[name]`, or keep the constructor state when there is no such section
/// (`0x0075046A..0x00750476`).
///
/// The section lookup is exact-case where native hashes the name through
/// `CRCEngine::AddData` (`INIClass::FindSectionByName @ 0x00526810`). Both stock
/// files spell all 820/500 `[SoundList]` values exactly as their section
/// headers, so the two agree on retail data; a mod that spelled one differently
/// would register the id with [`SoundEntry::unread`]'s silent state here.
fn read_entry(ini: &IniFile, name: &str, defaults: &SoundDefaults) -> SoundEntry {
    let Some(section) = ini.section(name) else {
        return SoundEntry::unread(name);
    };

    // `VocClass::AddSample @ 0x004064A0` skips every leading `$`/`#` and stops
    // accepting at 32 samples. An absent or token-less `Sounds=` is registered,
    // not skipped: `ReadINI` reads it with the empty-string default at
    // `0x0075049D` and simply adds nothing, so the id must still resolve here
    // rather than fall through to the audio-bag path. Eight stock sections take
    // that path — `Dummy`, `CampfireLoop`, `PropagandaTruck`, `DolphinFear`,
    // `OspreyCollision`, `SquidFear`, `SubFear`, `RobotTankPowerDown` — and
    // `Dummy.wav` is a real 1180-byte file in `audiomd.mix`, so registering the
    // silent entry is what keeps `[AudioVisual] Construction=`/`GateUp=`/
    // `GateDown=` from playing a buffer gamemd never starts.
    let sounds: Vec<String> = strtok(
        section.read_name("Sounds", TEXT_CAPACITY).unwrap_or(""),
        TOKEN_DELIMITERS,
    )
    .map(|s| s.trim_start_matches(['$', '#']).to_string())
    .filter(|s| !s.is_empty())
    .take(MAX_SAMPLES)
    .collect();

    let volume_raw = section.read_double("Volume", f64::from(defaults.volume));
    let volume_linear = volume_linear(volume_raw);
    let volume: u8 = volume_raw.clamp(0.0, 100.0).round() as u8;
    let vshift = section.read_int("VShift", 0).clamp(0, 100);
    let min_volume =
        min_volume_fraction(section.read_double("MinVolume", f64::from(defaults.min_volume)));
    let priority: u8 = section
        .read_name("Priority", TEXT_CAPACITY)
        .map_or(SOUND_PRIORITY_DEFAULT, parse_sound_priority);
    let attack_raw = section.read_int("Attack", 0);
    let decay_raw = section.read_int("Decay", 0);
    let control =
        parse_control_list(section.read_name("Control", TEXT_CAPACITY)).unwrap_or(defaults.control);
    // `AudioEventClass::SetControlFlags @ 0x00406570`: without the flag the
    // count is zeroed; with it a zero count becomes 1.
    let attack = normalise_envelope_count(attack_raw, control & control::ATTACK != 0);
    let decay = normalise_envelope_count(decay_raw, control & control::DECAY != 0);
    let type_flags =
        parse_type_list(section.read_name("Type", TEXT_CAPACITY)).unwrap_or(defaults.type_flags);
    let limit = section.read_int("Limit", defaults.limit);
    let loop_count = section.read_int("Loop", 0);
    let range: i32 = section.read_int("Range", defaults.range);
    let delay_ms = parse_int_pair(section.read_name("Delay", TEXT_CAPACITY));
    let fshift = parse_int_pair(section.read_name("FShift", TEXT_CAPACITY));

    SoundEntry {
        id: name.to_string(),
        sounds,
        volume,
        volume_linear,
        priority,
        range,
        min_volume,
        control,
        type_flags,
        limit,
        loop_count,
        delay_ms,
        fshift,
        vshift,
        attack,
        decay,
    }
}

/// `0.01f` at `0x007EAAE0`, the *float* constant the sound reader multiplies a
/// percentage by. It is `10737418 / 2^30`, i.e. slightly **below** 0.01, which
/// is why the whole chain has to stay at x87 precision — see
/// [`volume_linear`].
const ONE_PERCENT_F32_AS_F64: f64 = 0.01f32 as f64;

/// `0x007504EE..0x00750548`: the `ReadDouble` result times the *float* 0.01
/// at `0x007EAAE0`, clamped to `[0, 1]`, times the float 16384 at `0x007EF38C`,
/// then `Math::ftol` (truncation) into `AudioEventClass::SetVolumeRamp @
/// 0x00406550`.
///
/// **The product never leaves the x87 stack.** Between `0x00750507 FMUL float
/// [0x007EAAE0]` and the `ftol` call at `0x0075053F` there is no `FSTP float`
/// of any kind — only `FCOM`/`FLD` of the clamp constants — and the game's
/// control word keeps a 53-bit mantissa (0x0E7F, installed by Math__ftol
/// `0x007C5F00` and never restored; it also chops), so the chain carries 53
/// bits all the way into the truncation. Reproducing
/// that in `f64` is load-bearing: with an `f32` intermediate, `Volume=`
/// 25/50/75/100 (and the `Volume=5000%` form) each round *up* across a linear
/// step and land one higher than gamemd — 4096/8192/12288/16384 instead of
/// 4095/8191/12287/16383 — because `0.01f` sits just below 0.01, so the exact
/// product sits just below the round value. 55 stock `soundmd.ini` sections
/// write one of those four.
///
/// The clamp constants are asymmetric and are transcribed as the binary has
/// them: the high test compares against the *double* 1.0 at `0x007E1718` and
/// substitutes the *float* 1.0 at `0x007E2AC8`; the low test compares against
/// the float 0.0 at `0x007E1748` and takes "less **or** equal"
/// (`0x0075052C TEST AH,0x41`).
fn volume_linear(volume: f64) -> i32 {
    let product = volume * ONE_PERCENT_F32_AS_F64;
    let clamped = if product > 1.0 {
        f64::from(1.0f32)
    } else if product <= 0.0 {
        f64::from(0.0f32)
    } else {
        product
    };
    (clamped * f64::from(VOLUME_SCALE)).trunc() as i32
}

/// `0x00750589..0x007505E3`: `MinVolume * 0.01f` clamped to `[0, 1]`, stored
/// as a float by `AudioEventClass::SetMinVolume @ 0x004065F0` at `+0x54`.
///
/// Unlike [`volume_linear`] this chain *does* narrow: `0x007505A8 FST float
/// [ESP+8]` writes the product to a float32 slot while leaving the
/// full-precision value in `ST0`, so the `> 1.0` test at `0x007505AC` compares
/// the **un-narrowed** product against the double 1.0, and the `<= 0.0f` test
/// at `0x007505C7` reloads the **narrowed** float. Both clamps then write the
/// literal bit patterns `0x3F800000` / `0x00000000`.
///
/// RESIDUAL: this product, its float narrowing and [`volume_linear`]'s
/// product round to nearest where the game's control word chops: at most one
/// ulp (e.g. `MinVolume=70`), audio only.
fn min_volume_fraction(min_volume: f64) -> f32 {
    let product = min_volume * ONE_PERCENT_F32_AS_F64;
    let narrowed = product as f32;
    if product > 1.0 {
        1.0
    } else if narrowed <= 0.0 {
        0.0
    } else {
        narrowed
    }
}

fn normalise_envelope_count(raw: i32, flagged: bool) -> i32 {
    match (flagged, raw) {
        (false, _) => 0,
        (true, 0) => 1,
        (true, n) => n,
    }
}

/// The `strtok` delimiter string every sound token list splits on
/// (`0x00846570`, `" \t\n"`).
const TOKEN_DELIMITERS: &[char] = &[' ', '\t', '\n'];

/// `Control=` list: `None` when the key is absent or has no token (the
/// caller keeps the `[Defaults]` flags, `0x007506C9..0x00750700`).
fn parse_control_list(raw: Option<&str>) -> Option<u32> {
    let mut tokens = strtok(raw?, TOKEN_DELIMITERS).peekable();
    tokens.peek()?;
    Some(tokens.fold(0, |flags, token| flags | parse_control_token(token)))
}

/// `Type=` list: starts from SCREEN when at least one token exists
/// (`0x00750759`), otherwise `None` so the caller keeps the `[Defaults]`
/// flags.
fn parse_type_list(raw: Option<&str>) -> Option<u32> {
    let mut tokens = strtok(raw?, TOKEN_DELIMITERS).peekable();
    tokens.peek()?;
    let mut flags = sound_type::SCREEN;
    for token in tokens {
        apply_type_token(&mut flags, token);
    }
    Some(flags)
}

fn parse_control_token(token: &str) -> u32 {
    CONTROL_TABLE
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(token))
        .map_or(0, |&(_, value)| value)
}

/// `AudioEventClass::ParseTypeFlag @ 0x00406870`.
fn apply_type_token(flags: &mut u32, token: &str) {
    let value = TYPE_TABLE
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(token))
        .map_or(0, |&(_, value)| value);
    if value & (sound_type::SCREEN | sound_type::LOCAL) != 0 {
        *flags &= !(sound_type::SCREEN | sound_type::LOCAL);
    } else if value & (sound_type::UNSHROUD | sound_type::SHROUD) != 0 {
        *flags &= !(sound_type::UNSHROUD | sound_type::SHROUD);
    }
    *flags |= value;
}

/// `Delay=` / `FShift=` (`0x0075083C..0x00750886`, `0x0075089E..0x007508FF`):
/// the first token through `atoi` is the minimum; the second, when present, is
/// the maximum, otherwise the minimum is reused. No token gives `(0, 0)`.
///
/// **Read the disassembly, not the decompiler, for these two tails.** Ghidra's
/// pseudocode for both shows the first token's value being *overwritten* by the
/// second — i.e. a single-value read — and it is wrong. The instructions are
/// unambiguous: `0x00750853 XOR EBX,EBX` seeds the minimum, `0x00750866 MOV
/// EBX,EAX` takes `atoi(token1)`, `0x00750872 MOV ECX,EBX` pre-loads the
/// maximum with the minimum, `0x0075087F MOV ECX,EAX` replaces it with
/// `atoi(token2)` only when a second token exists, and `0x00750881 PUSH ECX;
/// 0x00750884 MOV EDX,EBX; CALL SetDelay @ 0x00406600` passes the pair to
/// `+0x58/+0x5C`. `FShift=` is the same shape through `EDI` into `SetFShift @
/// 0x00406610` (`+0x60/+0x64`). Do not "simplify" this against the decompiler.
fn parse_int_pair(raw: Option<&str>) -> (i32, i32) {
    let Some(raw) = raw else {
        return (0, 0);
    };
    let mut tokens = strtok(raw, TOKEN_DELIMITERS);
    let Some(first) = tokens.next() else {
        return (0, 0);
    };
    let min = crt_atoi(first);
    let max = tokens.next().map_or(min, crt_atoi);
    (min, max)
}

/// `VoxClass` entry `Type=` (`VoxClass::ReadINI @ 0x00752DB0`, entry `+0x4C`).
///
/// Parsed by `stricmp` in this order: `QUEUE` (`0x008467CC`) -> 1,
/// `STANDARD` (`0x008467C0`) -> 0, `INTERRUPT` (`0x00816120`) -> 2,
/// `QUEUED_INTERRUPT` (`0x008467AC`) -> 3. An empty or unknown token keeps the
/// `VoxClass::ReadEVAINI @ 0x00753000` default of 0 (STANDARD). Routing per
/// value lives in `VoxClass::InsertIntoQueue @ 0x00752590`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EvaType {
    #[default]
    Standard,
    Queue,
    Interrupt,
    QueuedInterrupt,
}

impl EvaType {
    fn parse(token: &str) -> Option<Self> {
        let token = token.trim();
        if token.eq_ignore_ascii_case("QUEUE") {
            Some(Self::Queue)
        } else if token.eq_ignore_ascii_case("STANDARD") {
            Some(Self::Standard)
        } else if token.eq_ignore_ascii_case("INTERRUPT") {
            Some(Self::Interrupt)
        } else if token.eq_ignore_ascii_case("QUEUED_INTERRUPT") {
            Some(Self::QueuedInterrupt)
        } else {
            None
        }
    }
}

/// `VoxClass` entry `Priority=` (`VoxClass::ReadINI @ 0x00752DB0`, entry
/// `+0x48`): `LOW` (`0x008161DC`) -> 0, `NORMAL` (`0x008161D4`) -> 1,
/// `IMPORTANT` (`0x008467A0`) -> 2, `CRITICAL` (`0x008161C0`) -> 3. The
/// `ReadEVAINI` default is 1 (NORMAL). Ordering is load-bearing: the pending
/// slot compares `node.priority < new.priority` (`0x0075264A`) and the four
/// `Type=QUEUE` lists are indexed by this value (`0x007525F9..0x007525FE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum EvaPriority {
    Low = 0,
    #[default]
    Normal = 1,
    Important = 2,
    Critical = 3,
}

impl EvaPriority {
    fn parse(token: &str) -> Option<Self> {
        let token = token.trim();
        if token.eq_ignore_ascii_case("LOW") {
            Some(Self::Low)
        } else if token.eq_ignore_ascii_case("NORMAL") {
            Some(Self::Normal)
        } else if token.eq_ignore_ascii_case("IMPORTANT") {
            Some(Self::Important)
        } else if token.eq_ignore_ascii_case("CRITICAL") {
            Some(Self::Critical)
        } else {
            None
        }
    }

    /// Index into the four `Type=QUEUE` FIFOs (`0xB1D450 + priority * 0xC`).
    pub fn list_index(self) -> usize {
        self as usize
    }
}

/// The voice column `VoxClass::PlayNextQueued @ 0x00752760` reads for the
/// session side stored by `VoxClass::SetSide @ 0x007534E0` (`0xB1D4C8`):
/// `0` -> `Allied=` (`+0x3E`), `1` -> `Russian=` (`+0x35`), anything else ->
/// `Yuri=` (`+0x2C`) (`0x007528E8..0x007528FE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EvaSide {
    #[default]
    Allied,
    Russian,
    Yuri,
}

impl EvaSide {
    /// `SetSide(-1)` stores 0 (`0x007534E0`); every other value is stored as
    /// is and only 0/1 select a named column at play time.
    pub fn from_side_index(side: i32) -> Self {
        match side {
            -1 | 0 => Self::Allied,
            1 => Self::Russian,
            _ => Self::Yuri,
        }
    }
}

/// One `[DialogList]` entry of evamd.ini (`VoxClass::ReadINI @ 0x00752DB0`).
#[derive(Debug, Clone, PartialEq)]
pub struct EvaEntry {
    /// The `[DialogList]` value, exact case, used as the entry identity
    /// (`VoxClass::PlayEVA @ 0x00752700` matches by `stricmp`).
    pub name: String,
    /// `Allied=` column (`+0x3E`, `strncpy` 9 bytes). `None` when empty.
    pub allied: Option<String>,
    /// `Russian=` column (`+0x35`).
    pub russian: Option<String>,
    /// `Yuri=` column (`+0x2C`).
    pub yuri: Option<String>,
    /// `Type=` (`+0x4C`), default STANDARD.
    pub eva_type: EvaType,
    /// `Priority=` (`+0x48`), default NORMAL.
    pub priority: EvaPriority,
    /// `Volume=` (`+0x28`, `ReadDouble` default 1.0). Stored as native does;
    /// its consumer is outside the queue pipeline (`SetGlobalVolume @
    /// 0x00752AB0` only stores a clamped byte at `0x00846614`) — carried,
    /// not applied. gamemd consumer UNCHECKED.
    pub volume: f32,
}

impl EvaEntry {
    fn from_section(name: &str, section: Option<&IniSection>) -> Self {
        let mut entry = Self {
            name: name.to_string(),
            allied: None,
            russian: None,
            yuri: None,
            eva_type: EvaType::Standard,
            priority: EvaPriority::Normal,
            volume: 1.0,
        };
        // `VoxClass::ReadINI` returns 0 when `FindSectionByName` fails; the
        // entry keeps its `ReadEVAINI` defaults and empty columns.
        let Some(section) = section else {
            return entry;
        };
        // A float field (`0x00752DF9 FSTP dword [ESI+0x28]`).
        entry.volume = section.read_float("Volume", entry.volume);
        // The text keys read into `char[500]` (`0x00752E18..0x00752FC1`).
        if let Some(eva_type) = section.read_name("Type", 0x1F4).and_then(EvaType::parse) {
            entry.eva_type = eva_type;
        }
        if let Some(priority) = section
            .read_name("Priority", 0x1F4)
            .and_then(EvaPriority::parse)
        {
            entry.priority = priority;
        }
        // `strncpy(dst, value, 9)` then a forced NUL: at most 8 characters
        // survive, and an empty value leaves the column empty.
        let column = |key: &str| -> Option<String> {
            section
                .read_name(key, 0x1F4)
                .map(|value| value.chars().take(8).collect())
        };
        entry.allied = column("Allied");
        entry.russian = column("Russian");
        entry.yuri = column("Yuri");
        entry
    }

    /// The sample name for one side column, `None` when that column is empty.
    pub fn column(&self, side: EvaSide) -> Option<&str> {
        match side {
            EvaSide::Allied => self.allied.as_deref(),
            EvaSide::Russian => self.russian.as_deref(),
            EvaSide::Yuri => self.yuri.as_deref(),
        }
    }
}

/// The `VoxClass` entry array (`0xB1D4A4`, count `0xB1D4B0`), built by
/// `VoxClass::ReadEVAINI @ 0x00753000` from **evamd.ini only** (`Init_Game
/// @ 0x0052C8A0` hands it the `EVAMD.INI` CCINI, strings `0x00825DF0`,
/// "Reading EVAMD.INI" `0x00825DFC`; gamemd never opens `eva.ini`).
///
/// `ReadEVAINI` walks `[DialogList]` by entry index, skips a value that is
/// already registered (`stricmp` scan), allocates the entry with its defaults
/// and calls `VoxClass::ReadINI` on the section of the same name.
#[derive(Debug, Clone, Default)]
pub struct EvaRegistry {
    entries: HashMap<String, EvaEntry>,
}

impl EvaRegistry {
    /// Parse the registry from evamd.ini data.
    pub fn from_ini(ini: &IniFile) -> Self {
        let mut entries: HashMap<String, EvaEntry> = HashMap::new();
        if let Some(list) = ini.section("DialogList") {
            // Each entry name through `ReadString` into `char[200]` (`0x0075304D`).
            for key in list.keys() {
                let Some(name) = list.read_name(key, 0xC8) else {
                    continue;
                };
                let id = name.to_ascii_uppercase();
                if entries.contains_key(&id) {
                    continue;
                }
                entries.insert(id, EvaEntry::from_section(name, ini.section(name)));
            }
        }
        log::info!(
            "EvaRegistry: loaded {} EVA event definitions",
            entries.len()
        );
        Self { entries }
    }

    /// `VoxClass::PlayEVA`'s `stricmp` scan: the entry for an event name.
    pub fn entry(&self, event_name: &str) -> Option<&EvaEntry> {
        self.entries.get(&event_name.to_ascii_uppercase())
    }

    /// Look up an EVA sample name by event name and side column.
    pub fn get(&self, event_name: &str, side: EvaSide) -> Option<&str> {
        self.entry(event_name)?.column(side)
    }

    /// Number of EVA event definitions.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// `AudioEventClass::ParsePriority @ 0x004067D0` walks the `(token, value)`
/// table at `0x00816018` with a case-insensitive compare and writes the value
/// beside the first match. The table's NULL terminator carries `2`, so an
/// unrecognised or absent token resolves to `NORMAL`.
const SOUND_PRIORITY_TABLE: [(&str, u8); 5] = [
    ("LOWEST", 0),
    ("LOW", 1),
    ("NORMAL", 2),
    ("HIGH", 3),
    ("CRITICAL", 4),
];

/// The terminator's value in the same table.
const SOUND_PRIORITY_DEFAULT: u8 = 2;

/// Resolve one `Priority=` token.
///
/// Stock `soundmd.ini` authors 183 of these and every one is symbolic — `low`,
/// `high`, `lowest`, `critical`, `normal`. Reading them through the numeric INI
/// path silently yields `0` for all of them, which inverts the ordering the key
/// exists to express.
fn parse_sound_priority(raw: &str) -> u8 {
    let token = raw.trim();
    SOUND_PRIORITY_TABLE
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(token))
        .map_or(SOUND_PRIORITY_DEFAULT, |&(_, value)| value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stock `[Defaults]` block, verbatim from `soundmd.ini`.
    const STOCK_DEFAULTS: &str = "[Defaults]\nMinVolume=50\nRange=10\nVolume=80\nLimit=5\nType= NORMAL SCREEN UNSHROUD\nPriority=NORMAL \n";

    /// Native registers only what `[SoundList]` names, so every fixture needs
    /// one. This builds it the way the stock file does — numbered entries whose
    /// values are the sound ids, in source order — from the section headers in
    /// `body`, skipping the meta-sections retail never lists.
    fn with_sound_list(body: &str) -> IniFile {
        let mut list = String::from("[SoundList]\n");
        let mut index = 0;
        for name in body
            .lines()
            .filter_map(|line| line.strip_prefix('['))
            .filter_map(|line| line.split(']').next())
        {
            if ["Defaults", "SoundList", "General"]
                .iter()
                .any(|meta| meta.eq_ignore_ascii_case(name))
            {
                continue;
            }
            index += 1;
            list.push_str(&format!("{index}={name}\n"));
        }
        IniFile::from_str(&format!("{list}{body}"))
    }

    fn registry(body: &str) -> SoundRegistry {
        SoundRegistry::from_ini(&with_sound_list(&format!("{STOCK_DEFAULTS}{body}")))
    }

    #[test]
    fn test_parse_single_sound() {
        let ini: IniFile =
            with_sound_list("[VGCannon1]\nSounds=vgcannon.wav\nVolume=80\nPriority=high\n");
        let reg: SoundRegistry = SoundRegistry::from_ini(&ini);
        assert_eq!(reg.len(), 1);
        let entry: &SoundEntry = reg.get("VGCannon1").expect("should find entry");
        assert_eq!(entry.sounds, vec!["vgcannon.wav"]);
        assert_eq!(entry.volume, 80);
        assert_eq!(entry.priority, 3);
    }

    /// Every token in the native table, in both cases, plus the terminator's
    /// default for an unrecognised or numeric value.
    #[test]
    fn priority_tokens_match_the_native_parse_table() {
        assert_eq!(parse_sound_priority("LOWEST"), 0);
        assert_eq!(parse_sound_priority("low"), 1);
        assert_eq!(parse_sound_priority("Normal"), 2);
        assert_eq!(parse_sound_priority("HIGH"), 3);
        assert_eq!(parse_sound_priority(" critical "), 4);
        // The terminator's own value is the fact the fix turns on, so it is
        // asserted as the literal 2 rather than against the constant that
        // carries it — the old broken behaviour resolved everything to 0, and a
        // constant-relative assertion would pass for that too.
        assert_eq!(SOUND_PRIORITY_DEFAULT, 2);
        assert_eq!(parse_sound_priority("5"), 2);
        assert_eq!(parse_sound_priority(""), 2);
        assert_eq!(parse_sound_priority("URGENT"), 2);
    }

    /// `strtok` on `" \t\n"` — a comma is part of the sample name, so a
    /// comma-joined list is one (missing) sample, never three.
    #[test]
    fn sounds_split_on_native_whitespace_only() {
        let ini: IniFile =
            with_sound_list("[E1Voice]\nSounds=e1sel01 e1sel02\te1sel03\nVolume=100\n");
        let reg: SoundRegistry = SoundRegistry::from_ini(&ini);
        let entry: &SoundEntry = reg.get("E1Voice").expect("should find entry");
        assert_eq!(entry.sounds, vec!["e1sel01", "e1sel02", "e1sel03"]);
        let comma = SoundRegistry::from_ini(&with_sound_list("[X]\nSounds=a.wav,b.wav\n"));
        assert_eq!(comma.get("X").unwrap().sounds, vec!["a.wav,b.wav"]);
    }

    #[test]
    fn test_case_insensitive_lookup() {
        let ini: IniFile = with_sound_list("[TestSound]\nSounds=test.wav\n");
        let reg: SoundRegistry = SoundRegistry::from_ini(&ini);
        assert!(reg.get("testsound").is_some());
        assert!(reg.get("TESTSOUND").is_some());
    }

    /// Registration comes from `[SoundList]`, not from the section headers:
    /// `VocClass::ReadSoundListINI @ 0x007510D0` only ever visits the ids that
    /// list names, so a section it does not name stays unregistered however
    /// complete it looks. Stock `soundmd.ini` has one such section,
    /// `[GuardianGiUnDeploy]`, and nothing in rules/art references it.
    #[test]
    fn only_soundlist_names_register_however_complete_the_section() {
        let ini: IniFile = IniFile::from_str(
            "[SoundList]\n1=Real\n[General]\nSounds=nothing.wav\n[Real]\nSounds=real.wav\n[GuardianGiUnDeploy]\nSounds=vgrdund\nVolume=70\n",
        );
        let reg: SoundRegistry = SoundRegistry::from_ini(&ini);
        assert!(reg.get("General").is_none());
        assert!(reg.get("GuardianGiUnDeploy").is_none());
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.get("Real").unwrap().sounds, vec!["real.wav"]);
    }

    /// Without a `[Defaults]` section the static initialisers apply:
    /// Volume 80, MinVolume 20, Type SCREEN, Limit 5, Range 10, Control 0.
    #[test]
    fn static_defaults_match_the_binary_initialisers() {
        let ini: IniFile = with_sound_list("[MinimalSound]\nSounds=min.wav\n");
        let reg: SoundRegistry = SoundRegistry::from_ini(&ini);
        let entry: &SoundEntry = reg.get("MinimalSound").unwrap();
        assert_eq!(entry.volume, 80);
        assert_eq!(entry.volume_linear, 13107); // ftol(0.8 * 16384)
        assert!((entry.min_volume - 0.2).abs() < 1e-6);
        // `VocClass::ReadINI @ 0x00750440` passes the literal "NORMAL" as the
        // ReadString default, so an absent key lands on the same 2 the
        // terminator carries.
        assert_eq!(entry.priority, 2);
        assert_eq!(entry.type_flags, sound_type::SCREEN);
        assert_eq!(entry.control, 0);
        assert_eq!(entry.limit, 5);
        assert_eq!(entry.loop_count, 0);
        assert_eq!(entry.range, 10);
        assert_eq!(entry.delay_ms, (0, 0));
        assert_eq!(entry.fshift, (0, 0));
        assert_eq!(entry.vshift, 0);
        assert_eq!((entry.attack, entry.decay), (0, 0));
    }

    /// The stock `[Defaults]` block: an entry authoring nothing but `Sounds=`
    /// inherits NORMAL|SCREEN|UNSHROUD, a 0.5 floor, Limit 5 and Range 10.
    #[test]
    fn stock_defaults_flow_into_a_bare_entry() {
        let reg = registry("[Bare]\nSounds=bare\n");
        let entry = reg.get("Bare").unwrap();
        assert_eq!(entry.type_flags, sound_type::SCREEN | sound_type::UNSHROUD);
        assert_eq!(entry.min_volume, 0.5);
        assert_eq!(entry.volume_linear, 13107);
        assert_eq!(entry.limit, 5);
        assert_eq!(entry.range, 10);
        assert_eq!(reg.defaults().min_volume_fraction(), 0.5);
        assert_eq!(reg.defaults().control, 0);
    }

    /// Stock `Type=` spellings. A `Type=` with any token restarts from SCREEN
    /// (so `[Defaults]`' UNSHROUD is dropped), LOCAL replaces SCREEN, and
    /// SHROUD replaces UNSHROUD.
    #[test]
    fn type_flags_follow_the_native_table_and_exclusive_pairs() {
        let reg = registry(
            "[G]\nSounds=g\nType=global\n[L]\nSounds=l\nType= Local\n[GS]\nSounds=gs\nType=global shroud\n[LS]\nSounds=ls\nType= local shroud\n[D]\nSounds=d\nType= NORMAL SCREEN UNSHROUD\n[U]\nSounds=u\nType=bogus\n[E]\nSounds=e\nType=\n",
        );
        use sound_type::*;
        assert_eq!(reg.get("G").unwrap().type_flags, SCREEN | GLOBAL);
        assert_eq!(reg.get("L").unwrap().type_flags, LOCAL);
        assert_eq!(reg.get("GS").unwrap().type_flags, SCREEN | GLOBAL | SHROUD);
        assert_eq!(reg.get("LS").unwrap().type_flags, LOCAL | SHROUD);
        assert_eq!(reg.get("D").unwrap().type_flags, SCREEN | UNSHROUD);
        assert_eq!(reg.get("U").unwrap().type_flags, SCREEN);
        // An empty value has no token: the [Defaults] flags are kept.
        assert_eq!(reg.get("E").unwrap().type_flags, SCREEN | UNSHROUD);
        let mut flags = SCREEN | UNSHROUD;
        apply_type_token(&mut flags, "shroud");
        apply_type_token(&mut flags, "LOCAL");
        apply_type_token(&mut flags, "screen");
        assert_eq!(flags, SCREEN | SHROUD);
    }

    /// Stock `Control=` spellings through the `0x008160C0` table, plus the
    /// attack/decay normalisation `SetControlFlags` applies afterwards.
    #[test]
    fn control_flags_and_envelope_counts_match_the_native_setters() {
        let reg = registry(
            "[R]\nSounds=r\nControl=random\n[RI]\nSounds=ri\nControl= random interrupt \n[RP]\nSounds=rp\nControl=random predelay\n[Amb]\nSounds=a\nControl= random loop all ambient\n[Env]\nSounds=a1 a2 a3 b1 b2 d1 d2 d3\nControl= loop random all decay attack\nAttack=3\nDecay=3\n[EnvNoCount]\nSounds=a b c\nControl= random attack decay\n[NoFlag]\nSounds=a b c\nControl=random\nAttack=2\nDecay=1\n[Empty]\nSounds=x\nControl=\n[Bogus]\nSounds=x\nControl=Random bogus\n",
        );
        use control::*;
        assert_eq!(reg.get("R").unwrap().control, RANDOM);
        assert_eq!(reg.get("RI").unwrap().control, RANDOM | INTERRUPT);
        assert_eq!(reg.get("RP").unwrap().control, RANDOM | PREDELAY);
        assert_eq!(
            reg.get("Amb").unwrap().control,
            RANDOM | LOOP | ALL | AMBIENT
        );
        let env = reg.get("Env").unwrap();
        assert_eq!(env.control, LOOP | RANDOM | ALL | DECAY | ATTACK);
        assert_eq!((env.attack, env.decay), (3, 3));
        assert_eq!(env.body_range(), 3..5);
        let env_no_count = reg.get("EnvNoCount").unwrap();
        assert_eq!((env_no_count.attack, env_no_count.decay), (1, 1));
        let no_flag = reg.get("NoFlag").unwrap();
        assert_eq!((no_flag.attack, no_flag.decay), (0, 0));
        assert_eq!(no_flag.body_range(), 0..3);
        assert_eq!(reg.get("Empty").unwrap().control, 0);
        assert_eq!(reg.get("Bogus").unwrap().control, RANDOM);
        assert_eq!(parse_control_token("INTERRUPT"), 0x10);
        assert_eq!(parse_control_token("predelay"), 0x08);
    }

    /// `Delay=`/`FShift=` one-or-two token rule and the `VShift=` clamp. Key
    /// lookup is exact-case (`ReadString` hashes the key bytes through
    /// `CRCEngine::AddData` at `0x00528A7E`), so the stock lower-case
    /// `Fshift`/`Vshift` spellings (3 and 9 `soundmd.ini` sections) are never
    /// read.
    #[test]
    fn delay_fshift_vshift_follow_native_pairs_and_clamps() {
        let reg = registry(
            "[A]\nSounds=a\nDelay=0 400\nFShift= -10 10\nVShift=20\n[B]\nSounds=b\nDelay= 400\nFshift=-5 5\nVshift=15\n[C]\nSounds=c\nVShift=140\nFShift=\nDelay= 5000 8000 9000\n[D]\nSounds=d\nVShift=-3\n",
        );
        let a = reg.get("A").unwrap();
        assert_eq!(a.delay_ms, (0, 400));
        assert_eq!(a.fshift, (-10, 10));
        assert_eq!(a.vshift, 20);
        let b = reg.get("B").unwrap();
        assert_eq!(b.delay_ms, (400, 400));
        assert_eq!(b.fshift, (0, 0));
        assert_eq!(b.vshift, 0);
        let c = reg.get("C").unwrap();
        assert_eq!(c.vshift, 100);
        assert_eq!(c.fshift, (0, 0));
        assert_eq!(c.delay_ms, (5000, 8000));
        assert_eq!(reg.get("D").unwrap().vshift, 0);
        assert_eq!(crt_atoi(" -12x"), -12);
        assert_eq!(crt_atoi("abc"), 0);
    }

    /// `Volume`/`MinVolume` go through `ReadDouble` (a double, `%` scaling)
    /// and the per-entry clamp before the 16384 scale.
    ///
    /// The exact values are derived from the native chain, not from VERA: the
    /// product stays at x87 precision from `0x00750507 FMUL float 0.01f` to
    /// `0x0075053F CALL Math::ftol`, so `Volume=100` truncates
    /// `100 * 0.01f * 16384 = 16383.99963...` to **16383**, one below the
    /// round `0x4000`. `Volume=250` reaches `0x4000` only through the high
    /// clamp, which substitutes the literal float `1.0` at `0x007E2AC8`.
    #[test]
    fn volume_and_min_volume_follow_read_double_and_the_clamps() {
        let reg = registry(
            "[Full]\nSounds=f\nVolume=100\n[Over]\nSounds=o\nVolume=250\nMinVolume=130\n[Neg]\nSounds=n\nVolume=-5\nMinVolume=-1\n[Pct]\nSounds=p\nVolume=5000%\n[Half]\nSounds=h\nVolume=50.5\nMinVolume=25\n[Lower]\nSounds=l\nvolume=60\n",
        );
        // Just under 0x4000: `100 * 0.01f` is `1073741800/2^30`, so the ×16384
        // product is 16383.9996337890625 and `ftol` truncates.
        assert_eq!(reg.get("Full").unwrap().volume_linear, VOLUME_SCALE - 1);
        let over = reg.get("Over").unwrap();
        // Only the clamp reaches exactly 0x4000.
        assert_eq!(over.volume_linear, VOLUME_SCALE);
        assert_eq!(over.min_volume, 1.0);
        let neg = reg.get("Neg").unwrap();
        assert_eq!(neg.volume_linear, 0);
        assert_eq!(neg.min_volume, 0.0);
        // `5000%` -> `ReadDouble` returns the double 50.0, then the same chain.
        assert_eq!(reg.get("Pct").unwrap().volume_linear, 8191);
        let half = reg.get("Half").unwrap();
        assert_eq!(half.volume_linear, volume_linear(50.5));
        assert_eq!(half.volume_linear, 8273); // ftol(0.505 * 16384)
        assert_eq!(half.min_volume, 0.25);
        // `volume=` is not `Volume=`: the entry keeps the static default.
        assert_eq!(reg.get("Lower").unwrap().volume, 80);
        // `sscanf("%f")` accepts an exponent, and the shared
        // `ini_value::parse_read_double` reproduces it — the private copy this
        // reader used to carry did not, and read `1e2` as 1.
        let exponent = registry("[Exp]\nSounds=e\nVolume=1e2\n");
        assert_eq!(exponent.get("Exp").unwrap().volume_linear, VOLUME_SCALE - 1);
    }

    /// The four stock `Volume=` values whose exact `Volume * 0.01f * 16384`
    /// product sits just *below* the round linear step. An `f32` intermediate
    /// anywhere in the chain rounds each of them up and yields
    /// 4096/8192/12288/16384; the native x87 chain truncates to one less.
    /// 55 stock `soundmd.ini` sections write one of these four.
    #[test]
    fn quarter_volumes_truncate_one_below_the_round_linear_step() {
        assert_eq!(volume_linear(25.0), 4095);
        assert_eq!(volume_linear(50.0), 8191);
        assert_eq!(volume_linear(75.0), 12287);
        assert_eq!(volume_linear(100.0), 16383);
        // Values whose product lands above the step are unaffected.
        assert_eq!(volume_linear(80.0), 13107);
        assert_eq!(volume_linear(85.0), 13926);
        assert_eq!(volume_linear(90.0), 14745);
    }

    /// Native registers a `[SoundList]` name whose `Sounds=` yields no usable
    /// tokens as a zero-sample event (`VocClass::ReadINI @ 0x00750440` reads
    /// `Sounds=` with the empty-string default at `0x0075049D` and
    /// `AddSample @ 0x004064A0` stores nothing), so the id must still resolve
    /// here — dropping it lets VERA's audio-bag fallback play a file gamemd
    /// never starts, because `SoundEvent::UpdateState @ 0x004055C0` state 0
    /// abandons an event with a zero sample count at `0x0040563C`.
    ///
    /// `Range=` is stored as a full int by `AudioEventClass::SetRange @
    /// 0x004065E0` (`MOV [ECX+0x50], EDX`) — no clamp, either end.
    #[test]
    fn empty_sample_list_registers_a_zero_sample_entry_and_range_keeps_the_native_int() {
        let reg = registry("[Silent]\nSounds=$\nRange=100000\n[Named]\nSounds=x\nRange=-4\n");
        let silent = reg
            .get("Silent")
            .expect("a sigil-only Sounds= still registers");
        assert!(silent.sounds.is_empty());
        assert_eq!(silent.range, 100_000);
        assert_eq!(reg.get("Named").unwrap().range, -4);
    }

    /// The eight stock `soundmd.ini` sections that write no usable `Sounds=`,
    /// in their retail shapes: six omit the key outright and two write only a
    /// comment, which the INI parser truncates away. All eight are named in
    /// `[SoundList]`, so all eight must register as silent entries.
    ///
    /// This is the case that matters in production: `Dummy.wav` really is in
    /// `audiomd.mix` (1180 bytes, RIFF), and `rulesmd.ini` routes `Dummy`
    /// through `[AudioVisual] Construction=`, `GateUp=` and `GateDown=`. If the
    /// id did not resolve here, VERA's bag/MIX fallback would play that buffer
    /// on every building start and gate cycle while gamemd stays silent.
    #[test]
    fn stock_sections_without_a_usable_sample_list_still_register_silently() {
        let reg = registry(concat!(
            "[Dummy]\nPriority=lowest\nVolume=0\n",
            "[DolphinFear]\nVolume=100\nControl=random\n",
            "[OspreyCollision]\nVolume=100\n",
            "[SquidFear]\nVolume=100\n",
            "[SubFear]\nVolume=100\n",
            "[RobotTankPowerDown]\nVolume=100\n",
            "[CampfireLoop]\nSounds= ;gcamlo1a gcamlo1b gcamlo1c\nControl=loop\n",
            "[PropagandaTruck]\nSounds= ;GEF Removed because they won't be in AudioMD ;$aprotr1\nControl=loop\n",
        ));
        for id in [
            "Dummy",
            "CampfireLoop",
            "PropagandaTruck",
            "DolphinFear",
            "OspreyCollision",
            "SquidFear",
            "SubFear",
            "RobotTankPowerDown",
        ] {
            let entry = reg
                .get(id)
                .unwrap_or_else(|| panic!("{id} is a [SoundList] name and must register"));
            assert!(
                entry.sounds.is_empty(),
                "{id} must carry no samples, got {:?}",
                entry.sounds
            );
        }
        assert_eq!(reg.len(), 8);
    }

    /// A `[SoundList]` name with no section at all. `ReadINI` returns at
    /// `0x00750476` without reading a key, so the entry keeps what
    /// `AudioEventClass::FindOrCreate @ 0x004063B0` wrote: Priority 2, Limit 3
    /// (not `[Defaults]`' 5), Range 10, MinVolume 0.2f, Type SCREEN, volume
    /// `0x4000` (`0x0040641F MOV EDX,0x4000` -> `0x00406458 CALL 0x00407100`,
    /// which stores `EDX << 16` into the `+0x18` interp the `Volume=` key
    /// writes), and no samples. Stock hits this through the decorative
    /// `============ Mission Disk sounds ============` list entry.
    #[test]
    fn listed_name_without_a_section_keeps_the_constructor_state() {
        let ini = IniFile::from_str(&format!(
            "{STOCK_DEFAULTS}[SoundList]\n1============= Mission Disk sounds ============\n2=Ghost\n"
        ));
        let reg = SoundRegistry::from_ini(&ini);
        let ghost = reg.get("Ghost").expect("a listed id registers regardless");
        assert!(ghost.sounds.is_empty());
        assert_eq!(ghost.priority, 2);
        assert_eq!(ghost.limit, 3);
        assert_eq!(ghost.range, 10);
        assert_eq!(ghost.min_volume, 0.2);
        assert_eq!(ghost.type_flags, sound_type::SCREEN);
        // Full, not silent: the zero-fill at `0x00406401` is overwritten by
        // `0x00406458`'s seed of the `+0x18` volume interp.
        assert_eq!(ghost.volume_linear, VOLUME_SCALE);
        // `[Defaults]` still applies to entries that do have a section.
        assert_eq!(reg.defaults().limit, 5);
    }

    /// `[SoundList]` values are cut to 31 bytes on the way in: `0x007512CA
    /// PUSH 0x20` gives `CCINIClass::ReadString @ 0x00528A10` a 32-byte buffer
    /// and its tail is `strncpy(dst, value, 0x20); dst[0x1f] = 0; strtrim()`.
    /// The cut precedes the trim, and the section lookup uses the cut name
    /// (`0x00750462` GetName -> `0x0075046A FindSectionByName`), so a section
    /// spelled with the full name is unreachable.
    #[test]
    fn a_list_value_is_cut_to_the_native_thirty_one_bytes() {
        // 31 + 4 characters. Native keeps "AAAA…A" (31) and drops "Tail".
        let long = format!("{}Tail", "A".repeat(MAX_ID_BYTES));
        let cut = "A".repeat(MAX_ID_BYTES);
        let ini = IniFile::from_str(&format!(
            "[SoundList]\n1={long}\n[{long}]\nSounds=$never\n[{cut}]\nSounds=$cut\n"
        ));
        let reg = SoundRegistry::from_ini(&ini);
        assert_eq!(reg.len(), 1);
        assert!(
            reg.get(&long).is_none(),
            "the full-length id must not exist"
        );
        assert_eq!(reg.get(&cut).expect("the cut id registers").sounds, ["cut"]);

        // The cut is by byte and lands before the trim, so a space that ends up
        // at the boundary is stripped: `strtrim` runs on the copied 31 bytes.
        let padded = format!("{} Tail", "B".repeat(MAX_ID_BYTES - 1));
        let ini = IniFile::from_str(&format!("[SoundList]\n1={padded}\n"));
        let reg = SoundRegistry::from_ini(&ini);
        assert_eq!(reg.len(), 1);
        assert!(reg.get(&"B".repeat(MAX_ID_BYTES - 1)).is_some());

        // A value at or under the ceiling is untouched.
        let ini = IniFile::from_str(&format!("[SoundList]\n1=KirovVoiceDie\n2={cut}\n"));
        let reg = SoundRegistry::from_ini(&ini);
        assert!(reg.get("KirovVoiceDie").is_some());
        assert!(reg.get(&cut).is_some());
    }

    /// A twice-listed id: the dedupe scan at `0x007512F4` hands the existing
    /// event back to `ReadINI`, and `AddSample` appends, so the sample list
    /// arrives twice. Stock lists `KirovVoiceDie` twice in both sound files.
    #[test]
    fn a_twice_listed_id_reads_its_section_twice_and_appends_samples() {
        let ini = IniFile::from_str(
            "[SoundList]\n1=KirovVoiceDie\n2=KirovVoiceDie\n[KirovVoiceDie]\nSounds= $vkirdia $vkirdib $vkirdic $vkirdid\nControl= random\nPriority=low\nVolume=70\n",
        );
        let reg = SoundRegistry::from_ini(&ini);
        assert_eq!(reg.len(), 1);
        let entry = reg.get("KirovVoiceDie").unwrap();
        assert_eq!(
            entry.sounds,
            vec![
                "vkirdia", "vkirdib", "vkirdic", "vkirdid", "vkirdia", "vkirdib", "vkirdic",
                "vkirdid"
            ]
        );
        // The scalars are simply re-read, not accumulated.
        assert_eq!(entry.priority, 1);
        assert_eq!(entry.control, control::RANDOM);
    }

    /// 33 samples: the native slot table holds 32.
    #[test]
    fn sample_list_is_capped_at_the_native_slot_count() {
        let list = (0..40)
            .map(|i| format!("s{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let reg = SoundRegistry::from_ini(&with_sound_list(&format!("[Many]\nSounds={list}\n")));
        assert_eq!(reg.get("Many").unwrap().sounds.len(), MAX_SAMPLES);
    }

    #[test]
    fn test_whitespace_separated() {
        let ini: IniFile = with_sound_list("[GISelect]\nSounds= igisea igiseb igisec\nVolume=85\n");
        let reg: SoundRegistry = SoundRegistry::from_ini(&ini);
        let entry: &SoundEntry = reg.get("GISelect").expect("should find entry");
        assert_eq!(entry.sounds, vec!["igisea", "igiseb", "igisec"]);
        assert_eq!(entry.volume, 85);
    }

    #[test]
    fn test_strip_dollar_prefix() {
        let ini: IniFile =
            with_sound_list("[VoiceTest]\nSounds= $igisea $igiseb $igisec\nVolume=85\n");
        let reg: SoundRegistry = SoundRegistry::from_ini(&ini);
        let entry: &SoundEntry = reg.get("VoiceTest").expect("should find entry");
        assert_eq!(entry.sounds, vec!["igisea", "igiseb", "igisec"]);
    }

    #[test]
    fn test_strip_hash_prefix() {
        let ini: IniFile = with_sound_list("[HashTest]\nSounds= #sound1 #$sound2\n");
        let reg: SoundRegistry = SoundRegistry::from_ini(&ini);
        let entry: &SoundEntry = reg.get("HashTest").expect("should find entry");
        assert_eq!(entry.sounds, vec!["sound1", "sound2"]);
    }

    #[test]
    fn test_inline_comment_filtered() {
        let ini: IniFile = with_sound_list("[CommentTest]\nSounds= irocdiea ;$irocdib $irocdic\n");
        let reg: SoundRegistry = SoundRegistry::from_ini(&ini);
        let entry: &SoundEntry = reg.get("CommentTest").expect("should find entry");
        assert_eq!(entry.sounds, vec!["irocdiea"]);
    }

    /// `VoxClass::ReadEVAINI @ 0x00753000` walks `[DialogList]` values and
    /// `VoxClass::ReadINI @ 0x00752DB0` reads `Type=`/`Priority=`/`Volume=`
    /// and the three columns; defaults are STANDARD / NORMAL / 1.0.
    #[test]
    fn eva_registry_carries_type_priority_and_columns_from_dialog_list() {
        let ini = IniFile::from_str(
            "[DialogList]\n0=EVA_UnitLost\n1=EVA_LowPower\n2=EVA_Plain\n3=EVA_Missing\n\
             4=EVA_UnitLost\n\
             [EVA_UnitLost]\nText=Unit lost.\nRussian=csof064\nAllied=ceva064\nYuri=cyur064\n\
             Priority= IMPORTANT\n\
             [EVA_LowPower]\nRussian=csof053\nAllied=ceva053\nYuri=cyur053\nType=queue\n\
             Priority=Important\nVolume=0.5\n\
             [EVA_Plain]\nAllied=ceva001\nType=bogus\nPriority=\n\
             [EVA_NotListed]\nAllied=ceva999\n",
        );
        let reg = EvaRegistry::from_ini(&ini);
        assert_eq!(reg.len(), 4, "duplicate DialogList values register once");

        let lost = reg.entry("eva_unitlost").unwrap();
        assert_eq!(lost.eva_type, EvaType::Standard);
        assert_eq!(lost.priority, EvaPriority::Important);
        assert_eq!(lost.column(EvaSide::Allied), Some("ceva064"));
        assert_eq!(lost.column(EvaSide::Russian), Some("csof064"));
        assert_eq!(lost.column(EvaSide::Yuri), Some("cyur064"));
        assert!((lost.volume - 1.0).abs() < f32::EPSILON);

        let power = reg.entry("EVA_LowPower").unwrap();
        assert_eq!(power.eva_type, EvaType::Queue, "stricmp: case-insensitive");
        assert_eq!(power.priority, EvaPriority::Important);
        assert!((power.volume - 0.5).abs() < f32::EPSILON);

        // Unknown or empty tokens keep the ReadEVAINI defaults.
        let plain = reg.entry("EVA_Plain").unwrap();
        assert_eq!(plain.eva_type, EvaType::Standard);
        assert_eq!(plain.priority, EvaPriority::Normal);
        assert_eq!(plain.column(EvaSide::Russian), None);

        // A listed name without a section keeps defaults and empty columns.
        let missing = reg.entry("EVA_Missing").unwrap();
        assert_eq!(missing.column(EvaSide::Allied), None);
        // A section that is not in DialogList is not an entry.
        assert!(reg.entry("EVA_NotListed").is_none());
        assert_eq!(reg.get("EVA_UnitLost", EvaSide::Russian), Some("csof064"));
    }

    /// `VoxClass::SetSide @ 0x007534E0` stores the side as is (`-1` → 0);
    /// `PlayNextQueued 0x007528E8..0x007528FE` selects Allied for 0, Russian
    /// for 1 and the Yuri column for every other value.
    #[test]
    fn eva_side_column_follows_the_native_side_index_select() {
        assert_eq!(EvaSide::from_side_index(-1), EvaSide::Allied);
        assert_eq!(EvaSide::from_side_index(0), EvaSide::Allied);
        assert_eq!(EvaSide::from_side_index(1), EvaSide::Russian);
        assert_eq!(EvaSide::from_side_index(2), EvaSide::Yuri);
        assert_eq!(EvaSide::from_side_index(7), EvaSide::Yuri);
    }

    /// `ReadINI` priority tokens map to the four list indices in the native
    /// order (`LOW`→0 .. `CRITICAL`→3); `HIGH`/`LOWEST` at `0x008161CC`/
    /// `0x008161E0` are not consulted by this reader.
    #[test]
    fn eva_priority_tokens_match_the_native_parse_table() {
        assert_eq!(EvaPriority::parse("LOW"), Some(EvaPriority::Low));
        assert_eq!(EvaPriority::parse("normal"), Some(EvaPriority::Normal));
        assert_eq!(
            EvaPriority::parse(" IMPORTANT"),
            Some(EvaPriority::Important)
        );
        assert_eq!(EvaPriority::parse("Critical"), Some(EvaPriority::Critical));
        assert_eq!(EvaPriority::parse("HIGH"), None);
        assert_eq!(EvaPriority::Critical.list_index(), 3);
        assert!(EvaPriority::Low < EvaPriority::Normal);
        assert_eq!(
            EvaType::parse("QUEUED_INTERRUPT"),
            Some(EvaType::QueuedInterrupt)
        );
        assert_eq!(EvaType::parse("interrupt"), Some(EvaType::Interrupt));
    }
}
