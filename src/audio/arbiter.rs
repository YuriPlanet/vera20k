//! The sound channel arbiter: which of the 16 hardware channels a cue gets,
//! which cue loses one, how many instances an entry may hold, how a looping
//! cue sustains, and how often the whole thing is serviced.
//!
//! Device-free by construction: this module owns the *decisions* and emits
//! [`ArbiterAction`]s; `audio::sfx` owns the rodio plumbing that applies them.
//! That split is what makes the mechanism reachable from `cargo test --lib`,
//! where `SfxPlayer::new` returns `None` because there is no audio device.
//!
//! gamemd-derived: `SoundSystem::UpdateTick @ 0x004041D0`, driven by
//! `AudioSystem::Pump @ 0x00406F70`. The native object is a `SoundEvent`
//! (0x280 bytes, pool of 300 — `SoundEventPool::Init @ 0x00403ED0`
//! `MOV EDX,0x280 ; MOV ECX,0x12c`) that competes for one of 16 DirectSound
//! secondary buffers (`DSoundChannel::CreateAll @ 0x00403530`, requested as
//! `MOV EDX,0x10` at `0x00406C25` with `CMP EAX,0x10 ; JNZ` disabling output
//! unless exactly 16 are made).
//!
//! ## Dependency rules
//! - Part of audio/. Depends on nothing but `std` and `rules::sound_ini`
//!   constants. Does NOT depend on render/, ui/, sidebar/, sim/, or on any
//!   audio device.

use std::collections::BTreeMap;

use crate::rules::sound_ini::{VOLUME_SCALE, control};

/// DirectSound secondary buffers the mixer owns.
///
/// `AudioSystem::Init @ 0x00406B10` calls `DSoundChannel::CreateAll @
/// 0x00403530` with `MOV EDX,0x10` (`0x00406C25`) and disables audio output
/// entirely unless the call returns exactly 16 (`CMP EAX,0x10 ; JNZ` at
/// `0x00406C31`). A `SoundEvent` is the *request*; a channel is the scarce
/// resource.
pub const MAX_CHANNELS: usize = 16;

/// `SoundEvent` records in the pool: `SoundEventPool::Init @ 0x00403ED0`
/// allocates `MemPool(count = 0x12c, elem = 0x280)`. Up to 300 cues can be
/// alive at once; only [`MAX_CHANNELS`] of them hold a channel.
pub const MAX_SOUND_EVENTS: usize = 300;

/// The service period. `AudioSystem::Pump @ 0x00406F70` runs a pass only when
/// more than `0x21` ms have elapsed since the last admitted one
/// (`0x21 < (uint)now - g_AudioPumpLastTimestamp`), and it is driven from
/// `Network_ServiceLoop @ 0x0048D080` — the message/service loop, not the
/// simulation frame. Menus, modal dialogs, loading screens and the frame
/// pacer's idle all keep the audio service running.
pub const PUMP_PERIOD_MS: u64 = 0x21;

/// Priority rows in `g_PriorityVolumeBuckets @ 0x0087DE38` (row stride 0x78).
pub const PRIORITY_ROWS: usize = 7;

/// Volume buckets per priority row (bucket stride 0x0C).
pub const VOLUME_BUCKETS: usize = 10;

/// Volume-bucket divisor. `SoundSystem::UpdateTick` computes the bucket with
/// the magic divide at `0x004044AE..0x004044D6`
/// (`MOV EAX,0x40140141 ; MUL ESI ; SUB ESI,EDX ; SHR ESI,1 ; ADD ESI,EDX ;
/// SHR ESI,0xa`), which is an unsigned divide of `event+0xBC >> 16` by 1638.
///
/// A full-scale instance (`0x4000`) yields **10**, one past the row, so it
/// spills into `bucket[prio + 1][0]` — harmless with the 7-row array, but it
/// makes a full-volume entry look one priority tier higher to
/// [`SoundArbiter::find_lowest_priority`], i.e. harder to preempt.
///
/// The spill is reproduced, not clamped away: the ranking insert at
/// `0x004044CA` (`LEA EBX,[EAX*8 + 0x87de38]`, row stride 120) and
/// `0x0040454D` (`LEA ECX,[EBX + ECX*4]`, bucket stride 12) applies **no
/// bound to either index**, so the write is a flat byte offset from the array
/// base and bucket 10 of row `p` is bucket 0 of row `p + 1`.
pub const BUCKET_DIVISOR: i32 = 1638;

/// Age gap two equal-priority channels need before the older one is taken.
/// `DSoundChannel::FindAvailable @ 0x004035F0`, `0x0040366B..0x00403675`:
/// `MOV EBX,ESI ; SUB EBX,EDX ; CMP EBX,0x666 ; JC skip` — the candidate is
/// only replaced when `bestStamp - stamp >= 0x666`.
pub const EQUAL_PRIORITY_AGE_GAP: u32 = 0x666;

/// Milliseconds the native ramp takes to cross the whole `0..0x4000` range.
///
/// `InterpGroup::Init @ 0x00401000` seeds every group with
/// `VolumeInterp::SetRate(span = 0x4000, ms = 1000)` for volume and pan
/// (`PUSH 0x0 ; PUSH 0x3e8 ; CALL 0x004071A0`), and `VolumeInterp::Init @
/// 0x00407100` writes the same rate literal `0x10624D` = `(0x4000 << 16) /
/// 1000`. **This is the only fade anywhere in the native audio pipeline** —
/// there is no per-sample fade-in or fade-out.
pub const RAMP_SPAN_MS: u32 = 1000;

/// Pre-delay floor and the `Control=ambient` minimum, in milliseconds.
/// `SoundEvent::UpdateState @ 0x004055C0` draws
/// `RandomRanged(AMBIENT ? 0x21 : Delay.min, Delay.max)` and discards any
/// result below `0x21` (`if (iVar5 < 0x21) return;`).
///
/// **Deferred DRIFT — the whole `Delay.min >= 0x21` playout path is a second
/// native mechanism VERA does not implement (gamemd read, VERA behaviour
/// UNCHECKED against it).** This value is not only a pre-delay floor: it
/// selects between two different sample loaders, and VERA implements only
/// the low side.
///
/// - Below the floor (what VERA models): `SoundEvent::PreparePlayout @
///   0x00404700` builds the whole playlist at `event+0x160` and
///   `AdvancePlaylist @ 0x004047B0` walks it; all three of that function's
///   arms — chain, LOOP restart, DECAY tail — sit inside its opening
///   `if (Voc+0x58 < 0x21 || (flags & 0x20))`.
/// - At or above it: `SoundEvent::LoadSamples @ 0x004048B0` takes a wholly
///   separate branch that loads **one** body sample per service pass, chosen
///   by `SoundEvent::SelectNextSample @ 0x00404BB0` (plus the decay sample
///   when `Control=decay`). `SelectNextSample` keeps its own playlist at
///   `event+0x1E8` with its own cursor `+0x270`, remaining count `+0x268`
///   and pass counter `+0x26C`, and returns `-1` to end the cue: after pass 1
///   without `Control=loop`, or when `Loop != 0 && Loop <= pass`. So the
///   `Loop=` budget and the `Control=all` set are honoured there, one sample
///   per `SoundSystem::UpdateTick @ 0x004041D0` pass, with the pre-delay
///   between them. `LoadSamples` is called only from `UpdateTick`, and
///   `SelectNextSample` only from `LoadSamples` (`get_function_callers`).
///
/// VERA instead resolves every entry through the low-side path:
/// `resolve_entry_playback_pass` chains `select_playout_pass`'s whole order
/// into one payload regardless of `Delay`. Trigger: submitting an entry
/// whose `Delay=` low bound is >= 33 ms. Player effect, both halves: VERA
/// plays the full chained set back-to-back as one pass where gamemd plays a
/// single sample, waits out the re-drawn pre-delay, then plays the next; and
/// because [`SoundArbiter::advance_loop`] correctly refuses the
/// `AdvancePlaylist` restart above the floor while VERA models no substitute,
/// VERA then *stops*, where gamemd sustains the cue under
/// `SelectNextSample`'s own `Loop=` budget. So an ambient authored to run
/// forever would play once and fall silent.
///
/// Trigger set, stated precisely because the obvious reading is wrong: the
/// loader split at `0x004048B0` tests `Voc+0x58` alone, with **no `Control=`
/// term**, so it is not the 24 `Control=loop` entries that qualify but every
/// entry above the floor — **30** in `ini/soundmd.ini`. Six of those carry no
/// `Control=loop`, and two of the six, `UpgradeVeteran`/`UpgradeElite`
/// (`Delay=400`), are produced on **every unit promotion**
/// (`rules/ruleset.rs` → `sim/world/techno_ai.rs` → `audio/events.rs`). They
/// are nonetheless inaudibly identical, because a single-sample entry with no
/// `Control=loop` yields that same one sample from either loader and then
/// ends.
///
/// The two halves diverge over **different sets, and neither contains the
/// other** — state each with its own criterion rather than adding them up.
/// Both are counted off `ini/soundmd.ini` with the native tokenizer (`strtok`
/// on `" \t\n"`, the delimiter string at `0x00846570`), so `Delay=` min is the
/// first token and `Delay=10000 20000` is above the floor.
///
/// - **Chaining set — 22 entries**: above the floor, `Control=all`, and more
///   than one `Sounds=` name. All three terms are load-bearing. Dropping the
///   `Control=` term would give 24, but wrongly: without `all`,
///   `select_playout_pass` (`src/audio/sfx.rs`) pushes a single index too, so
///   `CowAmbient` (`random interrupt ambient`) and `MIGMove`
///   (`random predelay`) yield one sample from either loader and cannot
///   chain. `Control=attack`/`decay` would qualify as well, but that arm is
///   vacuous on stock data — no above-floor entry writes `Attack=` or
///   `Decay=`.
/// - **Sustain set — 24 entries**: above the floor and `Control=loop`,
///   single-sample ones included. `SelectNextSample` ends a cue only on
///   `((flags & 1) == 0 && pass == 1)` or
///   `((flags & 1) != 0 && Voc+0x4C != 0 && Voc+0x4C <= pass)`, and `Voc+0x4C`
///   is `Loop=` read with **default 0**: `VocClass::ReadINI @ 0x00750834`
///   calls `CCINIClass::ReadInt(section, "Loop", 0)` (key string at
///   `0x00824238`) into `AudioEventClass::SetLoop @ 0x00406640`
///   (`MOV [ECX+0x4C],EDX`). The file's one `Loop=` is
///   `[TestEnvelopeFShift] Loop=3`, which is out of scope twice over — its
///   `Delay=` is commented out so it sits below the floor, and its
///   `Control= random` carries no loop bit. So all 24 leave `Voc+0x4C == 0`,
///   the second disjunct can never fire, and every one sustains forever
///   natively while [`SoundArbiter::advance_loop`] refuses it.
///
/// The two sets share **21** entries, so their union is 22 + 24 − 21 = **25**.
/// Four entries break a naive reading of one criterion or the other, and they
/// are why the union is not the larger set: `CruiseShipAmbience` (`gship1a`)
/// and `_Amb_DesertHawk` sustain but carry a single `Sounds=` name and no
/// `all`, so they never chain; `ChimpAmbient` (`random all ambient`) chains
/// but carries no `loop`, so it is the one chaining entry outside the sustain
/// set; and `PropagandaTruck` is above-floor `Control=loop` carrying **zero**
/// `Sounds=` names — its list is commented out at `ini/soundmd.ini:2012` — so
/// it is in the sustain set yet silent under either engine. Audibly
/// divergent is therefore 25 − 1 = **24**, every one `Control=ambient` except
/// the debug `TestRandomLoopDelayAll` (`random loop all`, `Priority=HIGH`).
/// Beware that this 24 and the sustain set's 24 are equal-sized *different*
/// sets: the sustain set holds `PropagandaTruck` and not `ChimpAmbient`, the
/// audible union the reverse.
///
/// Frequency: **zero today**. `rulesmd.ini` points `AmbientSound=` at six of
/// the 25, but `AmbientSound=` has no producer anywhere in the crate — the
/// token appears in `src/` only in this comment. All 41 distinct `MoveSound=`
/// values in `ini/rulesmd.ini` resolve to entries with `Delay` min 0. It goes
/// live the day ambients get a producer. Downstream risk:
/// closing it needs the per-buffer playlist that residuals R4 and R9 also
/// wait on, plus a port of `SelectNextSample`'s separate cursor state — it
/// is a mechanism port, not a gate on the existing one, which is why the
/// half-fix (truncating the chain to one sample here) is deliberately not
/// applied: it would pick the wrong sample and still not loop correctly.
///
/// VERA-internal, gamemd equivalent UNCHECKED: native's floor test reads
/// `*(uint *)(Voc+0x58)` and compares UNSIGNED, so a negative `Delay=` low
/// bound (which `crt_atoi` will happily parse from a leading `-`) lands above
/// the floor there and below it here, where the comparison is signed `i32`.
/// No stock `Delay=` is negative, so this cannot fire on retail data; it is
/// labelled only because the rest of this module labels its unreachable
/// divergences.
pub const PREDELAY_FLOOR_MS: i32 = 0x21;

/// Threshold above which the many-sounds limiter engages, and the numerator
/// it holds the total at. `SoundSystem::UpdateTick @ 0x00404565..0x0040457B`:
/// `CMP ECX,0x64 ; JLE ; MOV EAX,0x190000 ; XOR EDX,EDX ; DIV ECX`.
const MANY_SOUNDS_THRESHOLD: i32 = 100;
const MANY_SOUNDS_NUMERATOR: i32 = 0x0019_0000;

/// `SoundEvent+0x18` flag bits, read from the sites that set them.
mod event_flags {
    /// `0x01` — reaped: the event is dead and awaiting pool return.
    pub const DEAD: u32 = 0x01;
    /// `0x02` — the DirectSound buffer is running
    /// (`SoundEvent::StartPlayback @ 0x004054A0`, `flags |= 2`).
    pub const PLAYING: u32 = 0x02;
    /// `0x08` — the playout has begun at least once. Set by
    /// `StartPlayback` and by `SoundEvent::MarkStarted @ 0x004052E0` (the
    /// only caller of which is `AnimClass::UpdateLoopingSound @ 0x00750D40`).
    /// `SoundEvent::PreparePlayout @ 0x00404700` reads it to decide whether
    /// the `Control=attack` sample still heads the playout, so a loop
    /// restart never replays the attack.
    pub const STARTED: u32 = 0x08;
    /// `0x20` — no-replay: suppresses the body and loop branches of
    /// `SoundEvent::AdvancePlaylist @ 0x004047B0`.
    pub const NO_REPLAY: u32 = 0x20;
}

/// `SoundEvent+0x1C`, the switch at `0x004059D0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventState {
    /// 0 — needs a channel. `UpdateState` case `0x00405639`.
    NeedsChannel,
    /// 1 — has a channel, waiting for the start pass.
    Ready,
    /// 2 — counting down a pre-delay. `UpdateState` case `0x004055EB`.
    PreDelay,
    /// 3 — playing. `UpdateState` case `0x004057DC`, which is also where the
    /// looping leash lives.
    Playing,
    /// 4 — finished; reaped by the next pass.
    Dead,
}

/// One `VolumeInterp` (40 bytes in native): a value that glides toward a
/// target at a fixed rate, or snaps to it.
///
/// gamemd-derived layout, read from `VolumeInterp::Tick @ 0x004071C0` and
/// `VolumeInterp::Init @ 0x00407100`: `+0x00` flags (bit0 snap, bit1 dirty),
/// `+0x04` current 16.16, `+0x08` target 16.16, `+0x0C` rate per ms,
/// `+0x10` 64-bit max `dt`, `+0x18` 64-bit last stamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeInterp {
    snap: bool,
    /// 16.16 fixed point, as native stores it.
    current: i64,
    target: i64,
    /// `(span << 16) / ms` — `VolumeInterp::SetRate @ 0x004071A0`.
    rate: i64,
    max_dt_ms: u64,
    last_ms: u64,
}

impl VolumeInterp {
    /// `VolumeInterp::Init @ 0x00407100`: snap flag set, rate `0x10624D`,
    /// max `dt` 1000 ms, target `value << 16`, then a tail-jump to `Tick`
    /// which copies target into current.
    pub fn new(value: i32, now_ms: u64) -> Self {
        let scaled = i64::from(value) << 16;
        Self {
            snap: true,
            current: scaled,
            target: scaled,
            rate: (i64::from(VOLUME_SCALE) << 16) / i64::from(RAMP_SPAN_MS),
            max_dt_ms: u64::from(RAMP_SPAN_MS),
            last_ms: now_ms,
        }
    }

    /// `VolumeInterp::SetTargetImmediate @ 0x00407150`:
    /// `flags |= 1 ; [this+0x8] = value << 16`.
    pub fn set_target_immediate(&mut self, value: i32) {
        self.snap = true;
        self.target = i64::from(value) << 16;
    }

    /// `VolumeInterp::SetTarget @ 0x00407170`: clears the snap bit so the
    /// value glides, and re-stamps the clock **only when the interp was at
    /// rest** (`CMP EAX,ECX ; JNZ` at `0x00407181` compares target against
    /// current) — a glide already in flight keeps accumulating from its last
    /// tick.
    pub fn set_target(&mut self, value: i32, now_ms: u64) {
        self.snap = false;
        if self.target == self.current {
            self.last_ms = now_ms;
        }
        self.target = i64::from(value) << 16;
    }

    /// `VolumeInterp::Tick @ 0x004071C0`. Returns whether anything moved.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        let delta = self.target - self.current;
        if delta == 0 {
            return false;
        }
        if self.snap {
            self.current = self.target;
            return true;
        }
        let elapsed = now_ms.saturating_sub(self.last_ms);
        self.last_ms = now_ms;
        let dt = elapsed.min(self.max_dt_ms);
        let step = self.rate.saturating_mul(dt as i64);
        if delta >= 0 {
            self.current += step.min(delta);
        } else {
            self.current -= step.min(-delta);
        }
        true
    }

    /// The integral part native reads everywhere as `>> 0x10`.
    pub fn value(&self) -> i32 {
        (self.current >> 16) as i32
    }

    pub fn target_value(&self) -> i32 {
        (self.target >> 16) as i32
    }
}

/// Identifier of one live `SoundEvent`. VERA-internal: native uses the record
/// pointer, and pairs it with `+0x138` (the serial) to detect a stale handle.
/// The slot index plus the same serial carries that contract without pointers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EventId(pub u32);

/// The 16-byte `{ SoundEvent*, serial, VocClass*, &g_AudioIndex }` an owner
/// holds for a sustained cue (`SoundEvent::SetLoopHandle @ 0x004060F0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct VocHandle {
    event: Option<EventId>,
    serial: u32,
    entry: u32,
}

/// The registry facts the arbiter needs, copied at submit time the way native
/// reads them off the `VocClass` it already points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryFacts {
    /// `Priority=` (`VocClass+0x40`), `LOWEST 0`..`CRITICAL 4`.
    pub priority: i32,
    /// `Limit=` (`+0x48`), 0 = unlimited. `[Defaults] Limit=5` in stock
    /// `soundmd.ini`, so **every** entry is limited.
    pub limit: i32,
    /// `Control=` flag word (`+0x10`).
    pub control: u32,
    /// `Loop=` (`+0x4C`). 0 with `Control=loop` means owner-driven sustain.
    pub loop_count: i32,
    /// `Delay=` min/max in ms (`+0x58`/`+0x5C`).
    pub delay_ms: (i32, i32),
    /// The entry's authored `Volume=` as the native linear value (`+0x1C`
    /// integral part). Feeds the `Limit=` sort band and the many-sounds sum.
    pub entry_volume_linear: i32,
}

impl EntryFacts {
    /// `AudioEventClass::IsLoopable @ 0x00406650`:
    /// `(Control & LOOP) && Loop == 0`.
    pub fn is_loopable(&self) -> bool {
        self.control & control::LOOP != 0 && self.loop_count == 0
    }
}

/// One submitted cue.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayRequest {
    /// The `[SoundList]` identity, uppercased. All instances of one identity
    /// share a `Limit=` counter and one priority-bucket slot.
    pub key: String,
    pub facts: EntryFacts,
    /// The event's starting linear volume (spatial x entry x `VShift=`).
    pub volume_linear: i32,
    /// Pan `0..=0x4000`, `0x2000` centred.
    pub pan: i32,
    /// The caller's pre-delay draw, in milliseconds.
    ///
    /// Native draws it inside `UpdateState` state 0 *after* the channel is
    /// taken, as the third of three `RandomRanged` calls (FShift, VShift,
    /// then `RandomRanged(AMBIENT ? 0x21 : Delay.min, Delay.max)`). VERA draws
    /// all three together in `PlayShifts::draw` so the RNG order is preserved,
    /// and hands the result here; the arbiter applies it at native's place in
    /// the state machine, including the `0x21` ms floor.
    pub predelay_ms: i32,
}

/// What the device layer must do as a result of one service pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArbiterAction {
    /// Begin playback. Native `SoundEvent::StartPlayback @ 0x004054A0` snaps
    /// the volume and pan interps and starts the buffer.
    Start {
        event: EventId,
        volume_linear: i32,
        pan: i32,
        /// `Control=loop` with the loop budget still open — the device layer
        /// must keep the buffer queue topped up.
        sustaining: bool,
    },
    /// Live gain update while playing (the `VolumeInterp` glide).
    Gain {
        event: EventId,
        volume_linear: i32,
        pan: i32,
    },
    /// Release the output. The slot is already free on the arbiter side.
    Stop { event: EventId },
}

/// Per-channel state. Native fields: `+0x0C` kind, `+0xA0` priority,
/// `+0xA4` busy flags (`& 0x15` in use, `& 1` actively playing), `+0xC0` the
/// event that owns it, `+0xDC` the start stamp.
#[derive(Debug, Clone, Copy, Default)]
struct ChannelRec {
    event: Option<EventId>,
    priority: i32,
    /// Native `ch+0xDC`. Its writer was not located; its role as a
    /// monotonically increasing start stamp is inferred from the
    /// `(bestStamp - stamp) >= 0x666` comparison in `FindAvailable`, and the
    /// unit of `0x666` is UNCHECKED. VERA uses a monotonic allocation counter,
    /// so the gap compares allocation age rather than milliseconds.
    stamp: u32,
    /// `ch+0xA4 & 0x15` — holds an event at all.
    busy: bool,
    /// `ch+0xA4 & 1` — the buffer is actually running.
    playing: bool,
}

/// Per-`[SoundList]`-identity runtime state.
#[derive(Debug, Clone, Default)]
struct EntryRuntime {
    facts: Option<EntryFacts>,
    /// `VocClass+0x8C/+0x90`: the live instances, sorted by descending
    /// current volume, rebuilt from scratch every pass.
    list: Vec<EventId>,
    /// `+0x44`, the live count for this pass.
    count: i32,
    /// `+0x98`, the pass id the counter was reset on.
    limit_epoch: u32,
    /// `+0x9C`, the pass id this entry was ranked on.
    rank_epoch: u32,
    /// `+0xA0`, the best effective priority seen this ranking pass.
    best_priority: i32,
    /// `+0xB0`, the best volume bucket seen this ranking pass.
    best_bucket: i32,
    /// Where the entry currently sits in the bucket array, if anywhere, as a
    /// **flat** slot. Native addresses the array as one byte offset
    /// (`base + 120*row + 12*bucket`) with no per-index bound, so a flat
    /// index is what reproduces the bucket-10 spill into the next row.
    bucket_slot: Option<usize>,
}

#[derive(Debug, Clone)]
struct EventRec {
    entry: u32,
    state: EventState,
    /// `+0x20`, the state a pre-delay resumes into.
    resume_state: EventState,
    flags: u32,
    channel: Option<usize>,
    /// The event group's volume interp (`event+0xB8`, current at `+0xBC`).
    volume: VolumeInterp,
    /// The group's pan interp (`event+0xB8 + 0x50`).
    pan: VolumeInterp,
    /// `+0x138`. Zero means invalidated; a handle holding a different value
    /// is stale.
    serial: u32,
    /// The caller's pre-delay draw, applied in state 0 at native's place.
    predelay_ms: i32,
    /// `+0x140`, absolute while running and remaining while suspended.
    predelay_deadline_ms: u64,
    predelay_remaining_ms: u64,
    /// `+0x154`. Written only by `AllocateFromPool` (to 0) anywhere in the
    /// audio module, so the effective priority in stock play is `0..4`.
    priority_bonus: i32,
    /// `+0x158`. Non-zero blocks the start pass, freezes the pre-delay and
    /// removes the event from the many-sounds volume sum.
    suspend_depth: i32,
    /// `+0x1E4`, the completed loop passes.
    loop_iteration: i32,
    /// The owner that holds this event's `VocHandle` (`+0x278`).
    handle_owner: Option<u64>,
}

impl EventRec {
    fn is_dead(&self) -> bool {
        self.flags & event_flags::DEAD != 0
    }
}

/// The native sound system's decision half.
pub struct SoundArbiter {
    events: Vec<Option<EventRec>>,
    /// Insertion order of the global `SoundEvent` list (`g_SoundEventList @
    /// 0x0087E180`). Every pass walks it in this order.
    order: Vec<EventId>,
    channels: [ChannelRec; MAX_CHANNELS],
    entries: Vec<EntryRuntime>,
    keys: BTreeMap<String, u32>,
    /// The `[SoundList]` identity of each entry, parallel to `entries`.
    names: Vec<String>,
    /// `g_SoundLimitEpoch @ 0x0087E2AC`.
    limit_epoch: u32,
    /// `g_SoundRankEpoch @ 0x0087E2B0`.
    rank_epoch: u32,
    /// `g_SoundEventSerial @ 0x0081610C`. Starts at 1 so that 0 stays the
    /// "invalidated" value native writes on a kill.
    serial: u32,
    /// `g_PriorityVolumeBuckets @ 0x0087DE38`, cleared every pass.
    buckets: Vec<Vec<u32>>,
    /// `g_ManySoundsVolumeGroup @ 0x0087E1B8`, the master scaler chained into
    /// every channel at `ch+0x98`.
    many_sounds: VolumeInterp,
    /// The owner-held loop handles (`SoundEvent::SetLoopHandle @ 0x004060F0`).
    handles: BTreeMap<u64, VocHandle>,
    stamp: u32,
    last_pump_ms: Option<u64>,
}

impl SoundArbiter {
    pub fn new(now_ms: u64) -> Self {
        Self {
            events: Vec::new(),
            order: Vec::new(),
            channels: [ChannelRec::default(); MAX_CHANNELS],
            entries: Vec::new(),
            keys: BTreeMap::new(),
            names: Vec::new(),
            limit_epoch: 0,
            rank_epoch: 0,
            serial: 1,
            buckets: vec![Vec::new(); PRIORITY_ROWS * VOLUME_BUCKETS],
            many_sounds: VolumeInterp::new(VOLUME_SCALE, now_ms),
            handles: BTreeMap::new(),
            stamp: 0,
            last_pump_ms: None,
        }
    }

    fn entry_index(&mut self, key: &str) -> u32 {
        if let Some(index) = self.keys.get(key) {
            return *index;
        }
        let index = self.entries.len() as u32;
        self.entries.push(EntryRuntime::default());
        self.names.push(key.to_owned());
        self.keys.insert(key.to_owned(), index);
        index
    }

    /// The `[SoundList]` identity an owner's live loop handle names, so the
    /// caller can look its `Range`/`Type`/`MinVolume` back up for the
    /// per-update `CalcVolumeAndPan` re-drive.
    pub fn loop_handle_key(&self, owner: u64) -> Option<&str> {
        let handle = self.handles.get(&owner)?;
        handle.event?;
        self.names.get(handle.entry as usize).map(String::as_str)
    }

    fn event(&self, id: EventId) -> Option<&EventRec> {
        self.events.get(id.0 as usize)?.as_ref()
    }

    fn event_mut(&mut self, id: EventId) -> Option<&mut EventRec> {
        self.events.get_mut(id.0 as usize)?.as_mut()
    }

    /// `SoundEvent::AllocateFromPool @ 0x00405190`: take a pooled record,
    /// zero it, point it at the entry, seed the interp group and hand out the
    /// next serial. Returns `None` when the pool is exhausted — native reaps
    /// its dead events and retries once, then gives up and the cue simply
    /// never plays.
    pub fn submit(&mut self, request: &PlayRequest, now_ms: u64) -> Option<EventId> {
        let entry = self.entry_index(&request.key);
        self.entries[entry as usize].facts = Some(request.facts);

        let slot = match self.events.iter().position(Option::is_none) {
            Some(slot) => slot,
            None if self.events.len() < MAX_SOUND_EVENTS => {
                self.events.push(None);
                self.events.len() - 1
            }
            None => return None,
        };
        let id = EventId(slot as u32);
        let serial = self.serial;
        self.serial = self.serial.wrapping_add(1).max(1);

        // `InterpGroup::Init @ 0x00401000` seeds volume to 0x4000 and pan to
        // 0x2000; `SoundEvent::SetVolume/SetPan` then snap them because the
        // event is not yet in state 3.
        let mut volume = VolumeInterp::new(VOLUME_SCALE, now_ms);
        volume.set_target_immediate(request.volume_linear.clamp(0, VOLUME_SCALE));
        volume.tick(now_ms);
        let mut pan = VolumeInterp::new(VOLUME_SCALE / 2, now_ms);
        pan.set_target_immediate(request.pan.clamp(0, VOLUME_SCALE));
        pan.tick(now_ms);

        self.events[slot] = Some(EventRec {
            entry,
            state: EventState::NeedsChannel,
            resume_state: EventState::Ready,
            flags: 0,
            channel: None,
            volume,
            pan,
            serial,
            predelay_ms: request.predelay_ms,
            predelay_deadline_ms: 0,
            predelay_remaining_ms: 0,
            priority_bonus: 0,
            suspend_depth: 0,
            loop_iteration: 0,
            handle_owner: None,
        });
        self.order.push(id);
        Some(id)
    }

    /// `VocHandle::ValidateOrClear @ 0x00406130`: the owner-side check. A
    /// handle whose event has been recycled (different entry) or invalidated
    /// (serial mismatch) is cleared and reported as empty.
    pub fn validate_loop_handle(&mut self, owner: u64) -> Option<EventId> {
        let handle = *self.handles.get(&owner)?;
        let id = handle.event?;
        let ok = self
            .event(id)
            .is_some_and(|event| event.entry == handle.entry && event.serial == handle.serial);
        if ok {
            return Some(id);
        }
        if let Some(stored) = self.handles.get_mut(&owner) {
            stored.event = None;
        }
        None
    }

    /// `SoundEvent::SetLoopHandle @ 0x004060F0`: bind an owner's handle to an
    /// event, or clear it. Clearing is what stops a sustained cue — the
    /// leash in `UpdateState` state 3 kills any looping event whose owner no
    /// longer names it.
    pub fn set_loop_handle(&mut self, owner: u64, event: Option<EventId>, key: &str) {
        let entry = self.entry_index(key);
        let serial = event
            .and_then(|id| self.event(id))
            .map_or(0, |event| event.serial);
        self.handles.insert(
            owner,
            VocHandle {
                event,
                serial,
                entry,
            },
        );
        if let Some(id) = event
            && let Some(record) = self.event_mut(id)
        {
            record.handle_owner = Some(owner);
        }
    }

    /// Drop an owner's handle entirely (the owner object is gone).
    pub fn clear_loop_handle(&mut self, owner: u64) {
        self.handles.remove(&owner);
    }

    /// Owners that still name a live event. Handles whose event is gone are
    /// dropped, which is the bookkeeping half of `VocHandle::ValidateOrClear`.
    pub fn loop_handle_owners(&mut self) -> Vec<u64> {
        let owners: Vec<u64> = self.handles.keys().copied().collect();
        let mut live = Vec::new();
        for owner in owners {
            if self.validate_loop_handle(owner).is_some() {
                live.push(owner);
            } else {
                self.handles.remove(&owner);
            }
        }
        live
    }

    /// `SoundEvent::MarkStarted @ 0x004052E0` — `flags |= 8`. The only native
    /// caller is `AnimClass::UpdateLoopingSound @ 0x00750D40`, right after it
    /// allocates a loop event, which is why an anim-driven loop never plays
    /// its `Control=attack` sample.
    pub fn mark_started(&mut self, id: EventId) {
        if let Some(event) = self.event_mut(id) {
            event.flags |= event_flags::STARTED;
        }
    }

    /// Whether the first playout of this event still heads with the
    /// `Control=attack` sample: `SoundEvent::PreparePlayout @ 0x00404700`
    /// takes the attack path only while `flags & 8` is clear.
    pub fn plays_attack_sample(&self, id: EventId) -> bool {
        self.event(id)
            .is_some_and(|event| event.flags & event_flags::STARTED == 0)
    }

    /// `SoundEvent::SetVolume @ 0x004061D0`: snap while the event has not
    /// started, glide once it is playing.
    pub fn set_volume(&mut self, id: EventId, linear: i32, now_ms: u64) {
        let linear = linear.clamp(0, VOLUME_SCALE);
        let Some(event) = self.event_mut(id) else {
            return;
        };
        if event.state == EventState::Playing {
            event.volume.set_target(linear, now_ms);
        } else {
            event.volume.set_target_immediate(linear);
            event.volume.tick(now_ms);
        }
    }

    /// `SoundEvent::SetPan @ 0x00406270`, the same snap/glide rule.
    pub fn set_pan(&mut self, id: EventId, pan: i32, now_ms: u64) {
        let pan = pan.clamp(0, VOLUME_SCALE);
        let Some(event) = self.event_mut(id) else {
            return;
        };
        if event.state == EventState::Playing {
            event.pan.set_target(pan, now_ms);
        } else {
            event.pan.set_target_immediate(pan);
            event.pan.tick(now_ms);
        }
    }

    /// Current glide values, as the device layer must apply them.
    #[cfg(test)]
    pub fn live_gain(&self, id: EventId) -> Option<(i32, i32)> {
        let event = self.event(id)?;
        Some((event.volume.value(), event.pan.value()))
    }

    /// `SoundEvent::Stop @ 0x004052F0`: release the channel, drop the loaded
    /// samples, invalidate the serial and mark the record dead. The reaping
    /// pass returns it to the pool.
    pub fn stop(&mut self, id: EventId) {
        if self.event(id).is_none_or(EventRec::is_dead) {
            return;
        }
        self.release_channel(id);
        self.kill(id);
    }

    fn release_channel(&mut self, id: EventId) {
        let Some(event) = self.event_mut(id) else {
            return;
        };
        let channel = event.channel.take();
        event.flags &= !event_flags::PLAYING;
        if let Some(index) = channel
            && self.channels[index].event == Some(id)
        {
            self.channels[index] = ChannelRec::default();
        }
    }

    fn kill(&mut self, id: EventId) {
        if let Some(event) = self.event_mut(id) {
            event.state = EventState::Dead;
            event.serial = 0;
            event.handle_owner = None;
            event.flags |= event_flags::DEAD;
        }
    }

    /// `SoundEvent::ReturnToPool @ 0x00404DD0`: unlink from the global list
    /// and free the record.
    fn reap(&mut self, id: EventId) {
        if let Some(event) = self.event(id)
            && let Some(owner) = event.handle_owner
            && let Some(handle) = self.handles.get_mut(&owner)
            && handle.event == Some(id)
        {
            handle.event = None;
        }
        self.order.retain(|other| *other != id);
        self.events[id.0 as usize] = None;
    }

    /// `SoundSystem::SuspendAll @ 0x00404FD0`, reached from
    /// `GamePause::Enter @ 0x00406F00`: bump every event's suspend depth and
    /// convert a running pre-delay deadline into a remaining duration.
    /// Bookkeeping — reaping, ranking, `Limit=` — keeps running while paused;
    /// only starting and the pre-delay clock stop.
    pub fn suspend_all(&mut self, now_ms: u64) {
        for id in self.order.clone() {
            let Some(event) = self.event_mut(id) else {
                continue;
            };
            if event.suspend_depth == 0 && event.state == EventState::PreDelay {
                event.predelay_remaining_ms = event.predelay_deadline_ms.saturating_sub(now_ms);
            }
            event.suspend_depth += 1;
        }
    }

    /// `SoundSystem::ResumeAll @ 0x00405040` (`GamePause::Exit @ 0x00406F40`):
    /// decrement, clamped at zero, and re-absolutise the pre-delay.
    pub fn resume_all(&mut self, now_ms: u64) {
        for id in self.order.clone() {
            let Some(event) = self.event_mut(id) else {
                continue;
            };
            if event.suspend_depth == 0 {
                continue;
            }
            event.suspend_depth = (event.suspend_depth - 1).max(0);
            if event.suspend_depth == 0 && event.state == EventState::PreDelay {
                event.predelay_deadline_ms = now_ms.saturating_add(event.predelay_remaining_ms);
            }
        }
    }

    /// Whether a service pass is due: `0x21 < now - lastPass`
    /// (`AudioSystem::Pump @ 0x00406F70`).
    pub fn pump_due(&self, now_ms: u64) -> bool {
        match self.last_pump_ms {
            None => true,
            Some(last) => now_ms.saturating_sub(last) > PUMP_PERIOD_MS,
        }
    }

    /// `AdvancePlaylist @ 0x004047B0` step 2 asked from the device side: may
    /// this looping event start another pass?
    ///
    /// The whole body is gated on `Voc+0x58 < 0x21 || (flags & 0x20)` (first
    /// line of the decompiled body), and step 2 itself is
    /// `(Control & LOOP) && !(flags & 0x20) && (Loop == 0 || iteration <
    /// Loop - 1)`. The two combine to `Delay.min < 0x21 && !(flags & 0x20)
    /// && ...`: an entry whose `Delay=` low bound is 33 ms or more never
    /// reaches *this* LOOP branch.
    ///
    /// That is a claim about this path only. Such an entry still loops, and
    /// still plays its whole `Sounds=` set, through a second mechanism VERA
    /// does not model — see the `Delay >= 0x21` residual on
    /// [`PREDELAY_FLOOR_MS`]. Do not read this gate as "the entry does not
    /// loop".
    ///
    /// Consumes one iteration when it answers yes.
    pub fn advance_loop(&mut self, id: EventId) -> bool {
        let Some(event) = self.event(id) else {
            return false;
        };
        if event.is_dead() || event.flags & event_flags::NO_REPLAY != 0 {
            return false;
        }
        let Some(facts) = self.entries[event.entry as usize].facts else {
            return false;
        };
        if facts.delay_ms.0 >= PREDELAY_FLOOR_MS {
            return false;
        }
        if facts.control & control::LOOP == 0 {
            return false;
        }
        if facts.loop_count != 0 && event.loop_iteration >= facts.loop_count - 1 {
            return false;
        }
        if let Some(event) = self.event_mut(id) {
            event.loop_iteration += 1;
        }
        true
    }

    /// The device reporting that a buffer chain ran dry with nothing left to
    /// play — `LAB_00405A00`, the `ch+0xB8` callback, which sets state 4.
    pub fn notify_playout_ended(&mut self, id: EventId) {
        if self.event(id).is_none_or(EventRec::is_dead) {
            return;
        }
        self.release_channel(id);
        self.kill(id);
    }

    /// Number of live records (native `g_LiveSoundEventCount @ 0x0087E28C`).
    pub fn live_event_count(&self) -> usize {
        self.order.len()
    }

    /// Events currently holding a channel.
    pub fn busy_channel_count(&self) -> usize {
        self.channels.iter().filter(|ch| ch.busy).count()
    }

    /// The many-sounds master scaler (`0x0087E1B8`), chained into every
    /// channel at `ch+0x98` and multiplied in as `(a * b) >> 14`.
    pub fn many_sounds_linear(&self) -> i32 {
        self.many_sounds.value()
    }

    fn facts_of(&self, event: &EventRec) -> EntryFacts {
        self.entries[event.entry as usize]
            .facts
            .unwrap_or(EntryFacts {
                priority: 0,
                limit: 0,
                control: 0,
                loop_count: 0,
                delay_ms: (0, 0),
                entry_volume_linear: 0,
            })
    }

    fn effective_priority(&self, event: &EventRec) -> i32 {
        self.facts_of(event).priority + event.priority_bonus
    }

    /// `DSoundChannel::FindAvailable @ 0x004035F0`.
    ///
    /// 1. The first fully idle channel is returned immediately, with no
    ///    further search (`TEST byte ptr [EAX+0xa4],0x15 ; JZ` at
    ///    `0x00403637`).
    /// 2. Otherwise two candidates are tracked: the lowest-priority channel
    ///    that is *not* actively playing, and the lowest-priority one that
    ///    is. Among actively-playing channels of **equal** priority the older
    ///    stamp wins, but only when the gap is at least
    ///    [`EQUAL_PRIORITY_AGE_GAP`].
    /// 3. The non-playing candidate wins unless the playing candidate has a
    ///    strictly lower priority (`CMP [ESP+0x10],EDI ; JLE` at
    ///    `0x004036AA`).
    fn find_available_channel(&self) -> Option<usize> {
        let mut best_playing: Option<usize> = None;
        let mut best_playing_priority = 0x7f;
        let mut best_playing_stamp = u32::MAX;
        let mut best_idle: Option<usize> = None;
        let mut best_idle_priority = 0x7f;

        for (index, channel) in self.channels.iter().enumerate() {
            if !channel.busy {
                return Some(index);
            }
            if channel.playing {
                if channel.priority < best_playing_priority {
                    best_playing_priority = channel.priority;
                    best_playing_stamp = channel.stamp;
                    best_playing = Some(index);
                } else if channel.priority == best_playing_priority
                    && channel.stamp < best_playing_stamp
                    && best_playing_stamp - channel.stamp >= EQUAL_PRIORITY_AGE_GAP
                {
                    // `0x00403677: MOV EDI,ECX ; MOV EBP,EAX ; MOV ESI,EDX` —
                    // all three candidate registers move together.
                    best_playing_priority = channel.priority;
                    best_playing_stamp = channel.stamp;
                    best_playing = Some(index);
                }
            } else if channel.priority < best_idle_priority {
                best_idle_priority = channel.priority;
                best_idle = Some(index);
            }
        }

        match best_idle {
            None => best_playing,
            Some(idle) if best_idle_priority <= best_playing_priority => Some(idle),
            Some(_) => best_playing,
        }
    }

    /// `StreamBuffer::Allocate @ 0x00405B50`: take the channel
    /// [`Self::find_available_channel`] picked, **unless** it is busy and the
    /// newcomer's priority is strictly lower — equal priority still wins the
    /// channel. A failed allocation drops the cue outright.
    fn allocate_channel(&mut self, id: EventId, priority: i32) -> Option<usize> {
        let index = self.find_available_channel()?;
        if self.channels[index].busy && priority < self.channels[index].priority {
            return None;
        }
        // The dispossessed event is deliberately left pointing at the channel:
        // native never notifies it, and the reaping pass catches it with
        // `puVar3 != *(channel + 0xc0)` (`0x004043B7`) — an event whose
        // channel no longer names it is killed on the spot. That is how a
        // 17th equal-priority cue in one pass silences one of the sixteen
        // that took a channel earlier in the same pass.
        self.stamp = self.stamp.wrapping_add(1);
        self.channels[index] = ChannelRec {
            event: Some(id),
            priority,
            stamp: self.stamp,
            busy: true,
            playing: false,
        };
        Some(index)
    }

    /// `DSoundChannel::FindLowestPriority @ 0x00404E20`: scan
    /// `g_PriorityVolumeBuckets` rows `[0, priority)` — **strictly lower
    /// priority only, equal priority is never preempted here** — and inside a
    /// row take the quietest bucket first. Returns the losing entry.
    ///
    /// The row bound is `for (p = 0; p < param_1; p++)` with no upper clamp —
    /// native would walk past the 7-row array for a priority above 7. The
    /// `PRIORITY_ROWS` ceiling here is VERA-internal, gamemd equivalent
    /// UNCHECKED; it is unreachable on stock data, where `Priority=` tops out
    /// at `CRITICAL(4)`.
    #[cfg(test)]
    fn find_lowest_priority(&mut self, priority: i32) -> Option<u32> {
        for row in 0..priority.clamp(0, PRIORITY_ROWS as i32) as usize {
            for bucket in 0..VOLUME_BUCKETS {
                let slot = row * VOLUME_BUCKETS + bucket;
                if let Some(entry) = self.buckets[slot].first().copied() {
                    // The caller unlinks the winner from its bucket
                    // (`0x00404605 LEA ECX,[EBX+0xa4] ; CALL 0x00407450`) so
                    // the retry loop cannot pick the same entry forever.
                    self.buckets[slot].remove(0);
                    self.entries[entry as usize].bucket_slot = None;
                    return Some(entry);
                }
            }
        }
        None
    }

    /// One `SoundSystem::UpdateTick @ 0x004041D0` pass, in native phase
    /// order. The caller is responsible for the `> 33 ms` gate
    /// ([`Self::pump_due`]); this runs the pass unconditionally so tests can
    /// step it.
    pub fn update_tick(&mut self, now_ms: u64) -> Vec<ArbiterAction> {
        self.last_pump_ms = Some(now_ms);
        let mut actions = Vec::new();

        self.limit_epoch = self.limit_epoch.wrapping_add(1);
        self.enforce_limits(now_ms, &mut actions);
        self.run_state_machine(now_ms, &mut actions);
        self.reap_and_rank(now_ms, &mut actions);
        self.start_pass(now_ms, &mut actions);
        self.emit_live_gains(&mut actions);
        actions
    }

    /// Pass 1 (`0x0040421C..0x0040433D`): reset each entry's counter on first
    /// touch this epoch, tick the event's interp group, re-sort the event
    /// into the entry's descending-volume list, then enforce `Limit=` by
    /// killing the **tail** — the quietest instance, or with
    /// `Control=interrupt` the oldest already-started one.
    ///
    /// The new cue is never the victim; the least audible one is.
    fn enforce_limits(&mut self, now_ms: u64, actions: &mut Vec<ArbiterAction>) {
        for id in self.order.clone() {
            let Some(event) = self.event(id) else {
                continue;
            };
            let entry_index = event.entry as usize;
            if self.entries[entry_index].limit_epoch != self.limit_epoch {
                self.entries[entry_index].list.clear();
                self.entries[entry_index].count = 0;
                self.entries[entry_index].limit_epoch = self.limit_epoch;
            }
            // `FUN_00401080` — tick the whole interp group.
            if let Some(event) = self.event_mut(id) {
                event.volume.tick(now_ms);
                event.pan.tick(now_ms);
            }
            let Some(event) = self.event(id) else {
                continue;
            };
            if event.state == EventState::Dead {
                continue;
            }

            let facts = self.facts_of(event);
            let volume = event.volume.value();
            let interrupt = facts.control & control::INTERRUPT != 0;
            // `iVar2 < (Voc+0x1c / 0xa0000)`: the tie-break band is a tenth
            // of the entry's own authored volume.
            let band = facts.entry_volume_linear / 10;

            let slot = self.entries[entry_index]
                .list
                .iter()
                .position(|other| {
                    let Some(other) = self.event(*other) else {
                        return true;
                    };
                    let other_volume = other.volume.value();
                    let gap = (other_volume - volume).abs();
                    if gap < band {
                        if interrupt {
                            other.state != EventState::NeedsChannel
                        } else {
                            other.state == EventState::NeedsChannel
                        }
                    } else {
                        other_volume < volume
                    }
                })
                .unwrap_or(self.entries[entry_index].list.len());
            self.entries[entry_index].list.retain(|other| *other != id);
            let slot = slot.min(self.entries[entry_index].list.len());
            self.entries[entry_index].list.insert(slot, id);
            self.entries[entry_index].count += 1;

            let limit = facts.limit;
            let count = self.entries[entry_index].count;
            if limit != 0 && count > limit {
                let Some(victim) = self.entries[entry_index].list.pop() else {
                    continue;
                };
                if self.event(victim).is_some_and(|event| !event.is_dead()) {
                    self.release_channel(victim);
                    self.kill(victim);
                    actions.push(ArbiterAction::Stop { event: victim });
                }
                self.entries[entry_index].count -= 1;
            }
        }
    }

    /// Pass 2: `SoundEvent::UpdateState @ 0x004055C0` on every event.
    fn run_state_machine(&mut self, now_ms: u64, actions: &mut Vec<ArbiterAction>) {
        for id in self.order.clone() {
            self.update_state(id, now_ms, actions);
        }
    }

    fn update_state(&mut self, id: EventId, now_ms: u64, actions: &mut Vec<ArbiterAction>) {
        let Some(event) = self.event(id) else {
            return;
        };
        match event.state {
            // Case `0x00405639`: take a channel, draw the shifts, install the
            // callbacks, and either go straight to state 1 or park in the
            // pre-delay.
            EventState::NeedsChannel => {
                let priority = self.effective_priority(event);
                let facts = self.facts_of(event);
                match self.allocate_channel(id, priority) {
                    Some(index) => {
                        let Some(event) = self.event_mut(id) else {
                            return;
                        };
                        event.channel = Some(index);
                        event.loop_iteration = 0;
                        event.state = EventState::Ready;
                        // `if ((Voc.Control & 0x88) == 0) return;` — no
                        // `Control=predelay` and no `Control=ambient` means no
                        // pre-delay at all, whatever `Delay=` says.
                        if facts.control & (control::PREDELAY | control::AMBIENT) == 0 {
                            return;
                        }
                        // `if (iVar5 < 0x21) return;` — a draw under the
                        // 33 ms floor is discarded and the cue starts at once.
                        if event.predelay_ms < PREDELAY_FLOOR_MS {
                            return;
                        }
                        event.resume_state = EventState::Ready;
                        if event.suspend_depth == 0 {
                            event.predelay_deadline_ms =
                                now_ms.saturating_add(event.predelay_ms as u64);
                        } else {
                            event.predelay_remaining_ms = event.predelay_ms as u64;
                        }
                        event.state = EventState::PreDelay;
                    }
                    None => {
                        // Allocation refused: `0x004057A3` — release, mark
                        // dead, never audible.
                        if self.event(id).is_some_and(|event| !event.is_dead()) {
                            self.release_channel(id);
                            self.kill(id);
                            actions.push(ArbiterAction::Stop { event: id });
                        }
                    }
                }
            }
            // Case `0x004055EB`: the pre-delay wait, serviced by the same
            // > 33 ms pass, so pre-delays quantise to the pump period.
            EventState::PreDelay => {
                if event.suspend_depth != 0 {
                    return;
                }
                if now_ms <= event.predelay_deadline_ms {
                    return;
                }
                let resume = event.resume_state;
                if let Some(event) = self.event_mut(id) {
                    event.state = resume;
                }
            }
            // Case `0x004057DC`: the looping leash. A sustained event whose
            // owner no longer names it dies here, which is what stops a
            // Rocketeer's engine loop when the unit stops or leaves earshot.
            EventState::Playing => {
                let facts = self.facts_of(event);
                let leashed = facts.control & control::LOOP != 0
                    && facts.loop_count == 0
                    && event.flags & event_flags::NO_REPLAY == 0;
                if !leashed {
                    return;
                }
                let owner = event.handle_owner;
                let still_ours = owner.is_some_and(|owner| {
                    self.handles.get(&owner).is_some_and(|handle| {
                        handle.event == Some(id)
                            && self.event(id).is_some_and(|event| {
                                event.entry == handle.entry && event.serial == handle.serial
                            })
                    })
                });
                if still_ours {
                    return;
                }
                if self.event(id).is_some_and(|event| !event.is_dead()) {
                    self.release_channel(id);
                    self.kill(id);
                    actions.push(ArbiterAction::Stop { event: id });
                }
            }
            EventState::Ready | EventState::Dead => {}
        }
    }

    /// Passes 3-5: clear the buckets, bump the ranking epoch, reap every
    /// event that lost its channel or is already dead, rank the loudest live
    /// instance of each entry into `bucket[priority][volume / 1638]`, and set
    /// the many-sounds target.
    fn reap_and_rank(&mut self, now_ms: u64, actions: &mut Vec<ArbiterAction>) {
        for bucket in &mut self.buckets {
            bucket.clear();
        }
        for entry in &mut self.entries {
            entry.bucket_slot = None;
        }
        self.rank_epoch = self.rank_epoch.wrapping_add(1);

        let mut volume_sum: i32 = 0;
        for id in self.order.clone() {
            let Some(event) = self.event(id) else {
                continue;
            };
            let orphaned = match event.channel {
                None => true,
                Some(index) => self.channels[index].event != Some(id),
            };
            if orphaned && !event.is_dead() {
                self.release_channel(id);
                self.kill(id);
                actions.push(ArbiterAction::Stop { event: id });
            }
            if self.event(id).is_none_or(EventRec::is_dead) {
                self.reap(id);
                continue;
            }
            let Some(event) = self.event(id) else {
                continue;
            };

            // `0x00404474..0x00404496`: each unsuspended live event adds
            // `(Volume=linear * 5) >> 12` — about `VolumePercent / 5` — to the
            // budget the limiter holds at 100.
            let facts = self.facts_of(event);
            if event.suspend_depth == 0 {
                volume_sum = volume_sum.saturating_add((facts.entry_volume_linear * 5) >> 12);
            }

            let priority = self.effective_priority(event);
            let bucket = event.volume.value() / BUCKET_DIVISOR;
            let entry_index = event.entry as usize;
            let entry = &mut self.entries[entry_index];
            let rank_epoch = self.rank_epoch;
            let replaces = if entry.rank_epoch == rank_epoch {
                priority > entry.best_priority
                    || (priority == entry.best_priority && bucket > entry.best_bucket)
            } else {
                entry.rank_epoch = rank_epoch;
                true
            };
            if !replaces {
                continue;
            }
            entry.best_priority = priority;
            entry.best_bucket = bucket;
            if let Some(slot) = entry.bucket_slot.take() {
                self.buckets[slot].retain(|other| *other != entry_index as u32);
            }
            // `0x004044CA` / `0x0040454D`: the insert address is
            // `0x0087DE38 + 120*row + 12*bucket` with **no clamp on either
            // index**, so a full-scale instance's bucket 10 lands in
            // `bucket[row + 1][0]` — a full-volume entry reads as one
            // priority tier higher to `find_lowest_priority`, i.e. harder to
            // preempt. Compute the same flat slot.
            let slot = priority * VOLUME_BUCKETS as i32 + bucket;
            // VERA-internal, gamemd equivalent UNCHECKED: native has no bound
            // here and would write past the 7x10 array. Unreachable on stock
            // data — `Priority=` parses to `LOWEST(0)..CRITICAL(4)` and the
            // spill costs one row, so the highest slot a stock cue can reach
            // is row 5, bucket 0. Trigger: an effective priority of 6 or more,
            // or a negative one. Player effect if it ever fired: the entry is
            // simply not rankable, so `preempt_for_sample_memory` cannot pick
            // it. Frequency: never on stock `soundmd.ini`. Downstream risk:
            // none — nothing else reads the bucket array.
            let Ok(slot) = usize::try_from(slot) else {
                continue;
            };
            if slot >= self.buckets.len() {
                continue;
            }
            self.buckets[slot].push(entry_index as u32);
            self.entries[entry_index].bucket_slot = Some(slot);
        }

        let target = if volume_sum > MANY_SOUNDS_THRESHOLD {
            MANY_SOUNDS_NUMERATOR / volume_sum
        } else {
            VOLUME_SCALE
        };
        self.many_sounds.set_target(target, now_ms);
        self.many_sounds.tick(now_ms);
    }

    /// The entry-level preemption layer:
    /// `DSoundChannel::FindLowestPriority @ 0x00404E20` plus the caller loop
    /// at `0x004045FA..0x0040466F`, which stops **every** `SoundEvent` of the
    /// losing entry (`SoundEvent::Stop @ 0x004052F0` + `ReturnToPool @
    /// 0x00404DD0`) and retries.
    ///
    /// **Its native trigger is sample-memory starvation, not channel
    /// starvation.** The start pass reaches it only when
    /// `SoundEvent::PreparePlayout @ 0x00404700` returns 0. That function has
    /// three `return 0` paths: a zero loaded-sample count (`+0xA8`, i.e.
    /// `SoundEvent::LoadSamples @ 0x004048B0` got nothing back from
    /// `SampleTracker::LoadSample`), `flags & 0x20` (NO_REPLAY), and
    /// `+0x1E0 < 1` after the attack/decay reservation. **On stock data only
    /// the first is reachable** — all 33 `Control=attack`/`decay` entries in
    /// `soundmd.ini` carry at least three `Sounds=`, so the reservation
    /// cannot empty the list — and it means the streaming pool
    /// (`FUN_004019E0(idx, 0x100000, 200, 0x2000)` at `0x00403F29`: 127
    /// blocks of 8 KB inside a 1 MB budget) is full. Stopping lower-priority
    /// entries frees those blocks.
    ///
    /// RESIDUAL (UNCHECKED trigger) — VERA decodes each cue into an owned
    /// `Vec<f32>` with no fixed budget, so nothing here can starve and this
    /// layer has no production caller. Trigger in gamemd: more than 1 MB of
    /// distinct sample data live at once. Player effect of the gap: in the
    /// rare native case where the stream pool fills, gamemd silences a whole
    /// low-priority entry to admit a high-priority one, while VERA plays
    /// both. Frequency: never on this decoder. Downstream risk: none — the
    /// audible priority arbitration a player hears in a busy fight is the
    /// *channel* layer ([`Self::find_available_channel`] /
    /// [`Self::allocate_channel`]), which is live. Reproducing this layer
    /// needs a fixed decoded-sample budget first; the mechanism is kept and
    /// tested so that budget is the only thing missing.
    #[cfg(test)]
    pub fn preempt_for_sample_memory(&mut self, priority: i32) -> Vec<EventId> {
        let mut stopped = Vec::new();
        let Some(victim_entry) = self.find_lowest_priority(priority) else {
            return stopped;
        };
        for other in self.order.clone() {
            if self
                .event(other)
                .is_some_and(|event| event.entry == victim_entry && !event.is_dead())
            {
                self.release_channel(other);
                self.kill(other);
                stopped.push(other);
            }
        }
        stopped
    }

    /// Pass 6 (`0x004045A6..0x004046C6`): every event in state 1 that is not
    /// suspended runs `LoadSamples` -> `PreparePlayout` ->
    /// (preempt and retry) -> `StartPlayback`.
    ///
    /// VERA's payload is decoded before [`Self::submit`], so `LoadSamples`
    /// and `PreparePlayout` have already succeeded by the time an event
    /// exists — only `StartPlayback` remains here. See
    /// [`Self::preempt_for_sample_memory`] for the retry branch and why it
    /// has no trigger on this decoder.
    fn start_pass(&mut self, now_ms: u64, actions: &mut Vec<ArbiterAction>) {
        for id in self.order.clone() {
            let Some(event) = self.event(id) else {
                continue;
            };
            if event.state != EventState::Ready || event.suspend_depth != 0 {
                continue;
            }
            if event.channel.is_none() {
                if !event.is_dead() {
                    self.kill(id);
                    actions.push(ArbiterAction::Stop { event: id });
                }
                continue;
            }

            // `SoundEvent::StartPlayback @ 0x004054A0`: snap both interps
            // before the buffer starts, set `flags |= 8 | 2`, state 3.
            let facts = self.facts_of(event);
            let sustaining = facts.control & control::LOOP != 0
                && (facts.loop_count == 0 || facts.loop_count > 1);
            let Some(event) = self.event_mut(id) else {
                continue;
            };
            event
                .volume
                .set_target_immediate(event.volume.target_value());
            event.volume.tick(now_ms);
            event.pan.set_target_immediate(event.pan.target_value());
            event.pan.tick(now_ms);
            event.flags |= event_flags::STARTED | event_flags::PLAYING;
            event.state = EventState::Playing;
            let volume_linear = event.volume.value();
            let pan = event.pan.value();
            if let Some(index) = event.channel {
                self.channels[index].playing = true;
            }
            actions.push(ArbiterAction::Start {
                event: id,
                volume_linear,
                pan,
                sustaining,
            });
        }
    }

    /// Every playing event reports its glided gain so the device layer can
    /// track the `VolumeInterp` ramp. Native does this implicitly: the
    /// channel reads `ch+0x90` (the event group) every mix.
    fn emit_live_gains(&mut self, actions: &mut Vec<ArbiterAction>) {
        for id in self.order.clone() {
            let Some(event) = self.event(id) else {
                continue;
            };
            if event.state != EventState::Playing || event.is_dead() {
                continue;
            }
            let started_this_pass = actions
                .iter()
                .any(|action| matches!(action, ArbiterAction::Start { event, .. } if *event == id));
            if started_this_pass {
                continue;
            }
            actions.push(ArbiterAction::Gain {
                event: id,
                volume_linear: event.volume.value(),
                pan: event.pan.value(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(priority: i32, limit: i32) -> EntryFacts {
        EntryFacts {
            priority,
            limit,
            control: 0,
            loop_count: 0,
            delay_ms: (0, 0),
            entry_volume_linear: VOLUME_SCALE,
        }
    }

    fn request(key: &str, facts: EntryFacts, volume_linear: i32) -> PlayRequest {
        PlayRequest {
            key: key.to_owned(),
            facts,
            volume_linear,
            pan: VOLUME_SCALE / 2,
            predelay_ms: 0,
        }
    }

    fn started(actions: &[ArbiterAction]) -> Vec<EventId> {
        actions
            .iter()
            .filter_map(|action| match action {
                ArbiterAction::Start { event, .. } => Some(*event),
                _ => None,
            })
            .collect()
    }

    fn stopped(actions: &[ArbiterAction]) -> Vec<EventId> {
        actions
            .iter()
            .filter_map(|action| match action {
                ArbiterAction::Stop { event } => Some(*event),
                _ => None,
            })
            .collect()
    }

    /// `DSoundChannel::CreateAll @ 0x00403530` makes exactly 16 buffers, so
    /// only 16 cues can be audible at once. A 17th equal-priority cue in the
    /// same pass still *takes* a channel — `StreamBuffer::Allocate @
    /// 0x00405B50` refuses only a strictly lower priority — and the
    /// dispossessed event is caught by the reaping pass' "my channel no
    /// longer names me" test.
    #[test]
    fn sixteen_channels_cap_the_pool_and_an_equal_priority_seventeenth_displaces_one() {
        let mut arbiter = SoundArbiter::new(0);
        let mut ids = Vec::new();
        for index in 0..17 {
            let key = format!("CUE{index}");
            ids.push(
                arbiter
                    .submit(&request(&key, facts(2, 0), VOLUME_SCALE), 0)
                    .expect("pool slot"),
            );
        }
        let actions = arbiter.update_tick(100);
        assert_eq!(started(&actions).len(), MAX_CHANNELS);
        assert_eq!(arbiter.busy_channel_count(), MAX_CHANNELS);
        // `FindAvailable` walks the list in order and keeps the first
        // lowest-priority candidate, so the newcomer takes channel 0 and the
        // event that had it is stopped — not the newcomer.
        assert_eq!(stopped(&actions), vec![ids[0]]);
        assert!(started(&actions).contains(&ids[16]));
    }

    /// A `CRITICAL` cue arriving into a full pool of `LOWEST` cues takes a
    /// channel: `FindLowestPriority` walks rows `[0, 4)`, finds the quietest
    /// low-priority entry and stops every one of its events.
    #[test]
    fn a_higher_priority_cue_preempts_a_full_pool_of_lower_priority_ones() {
        let mut arbiter = SoundArbiter::new(0);
        for index in 0..MAX_CHANNELS {
            let key = format!("QUIET{index}");
            arbiter
                .submit(&request(&key, facts(0, 0), VOLUME_SCALE / 4), 0)
                .expect("pool slot");
        }
        assert_eq!(started(&arbiter.update_tick(100)).len(), MAX_CHANNELS);

        let loud = arbiter
            .submit(&request("LOUD", facts(4, 0), VOLUME_SCALE), 200)
            .expect("pool slot");
        let actions = arbiter.update_tick(200);
        assert!(started(&actions).contains(&loud));
        assert_eq!(stopped(&actions).len(), 1);
    }

    /// The reverse: an old `CRITICAL` bed is never displaced by a new
    /// `LOWEST` cue. `StreamBuffer::Allocate` rejects the newcomer outright.
    #[test]
    fn a_lower_priority_cue_never_displaces_a_full_pool_of_higher_priority_ones() {
        let mut arbiter = SoundArbiter::new(0);
        for index in 0..MAX_CHANNELS {
            let key = format!("LOUD{index}");
            arbiter
                .submit(&request(&key, facts(4, 0), VOLUME_SCALE), 0)
                .expect("pool slot");
        }
        arbiter.update_tick(100);
        let quiet = arbiter
            .submit(&request("QUIET", facts(0, 0), VOLUME_SCALE), 200)
            .expect("pool slot");
        let actions = arbiter.update_tick(200);
        assert!(!started(&actions).contains(&quiet));
        assert_eq!(stopped(&actions), vec![quiet]);
        assert_eq!(arbiter.busy_channel_count(), MAX_CHANNELS);
    }

    /// `[Defaults] Limit=5` applies to all 588 stock entries, and 17 of them
    /// author `Limit=1`. The victim is the entry's **quietest** live instance
    /// (the list tail), never the newcomer.
    #[test]
    fn limit_one_keeps_the_loudest_instance_and_kills_the_quietest() {
        let mut arbiter = SoundArbiter::new(0);
        let quiet = arbiter
            .submit(&request("CUE", facts(2, 1), VOLUME_SCALE / 8), 0)
            .expect("pool slot");
        let loud = arbiter
            .submit(&request("CUE", facts(2, 1), VOLUME_SCALE), 0)
            .expect("pool slot");
        let actions = arbiter.update_tick(100);
        assert_eq!(stopped(&actions), vec![quiet]);
        assert!(started(&actions).contains(&loud));
    }

    /// `Control=interrupt` is a `Limit=` **sort** tie-break, not a "stop the
    /// previous instance" flag — but at equal volume it does decide which of
    /// two instances survives the cap.
    ///
    /// `0x004042AF..0x004042C6`: inside the volume band, without INTERRUPT the
    /// newcomer sorts before the first instance still in state 0, and with
    /// INTERRUPT before the first instance that has *left* state 0. So against
    /// one already-playing instance the newcomer sorts to the tail without the
    /// flag (and is culled) and to the head with it (culling the playing one).
    /// 106 stock entries author it.
    #[test]
    fn control_interrupt_only_flips_the_limit_sort_tie_break() {
        // Two instances at the same volume, `Limit=1`. One is already
        // playing, one has just been submitted.
        let run = |interrupt: bool| {
            let mut arbiter = SoundArbiter::new(0);
            let mut cue = facts(2, 1);
            if interrupt {
                cue.control = control::INTERRUPT;
            }
            let first = arbiter
                .submit(&request("CUE", cue, VOLUME_SCALE), 0)
                .expect("pool slot");
            arbiter.update_tick(100);
            let second = arbiter
                .submit(&request("CUE", cue, VOLUME_SCALE), 200)
                .expect("pool slot");
            let victims = stopped(&arbiter.update_tick(200));
            (first, second, victims)
        };

        // Without INTERRUPT the walk never breaks (no list member is still in
        // state 0), so the newcomer is appended at the tail and the cap kills
        // it — the already-playing instance keeps the slot.
        let (_first, second, victims) = run(false);
        assert_eq!(victims, vec![second]);

        // With INTERRUPT the walk breaks on the first member that has left
        // state 0, so the newcomer takes the head and the playing instance
        // becomes the tail that is culled.
        let (first, _second, victims) = run(true);
        assert_eq!(victims, vec![first]);
    }

    /// Two equal-priority channels: the older one only loses its channel when
    /// the age gap reaches `0x666` (`CMP EBX,0x666 ; JC skip` at
    /// `0x0040366F`). Below that gap `FindAvailable` keeps the first
    /// candidate it saw.
    #[test]
    fn equal_priority_channels_need_a_1638_stamp_gap_before_the_older_one_loses() {
        let mut arbiter = SoundArbiter::new(0);
        // Fill and start all 16 so every channel is `playing`.
        let mut ids = Vec::new();
        for index in 0..MAX_CHANNELS {
            ids.push(
                arbiter
                    .submit(&request(&format!("C{index}"), facts(2, 0), VOLUME_SCALE), 0)
                    .expect("pool slot"),
            );
        }
        arbiter.update_tick(100);
        // Stamps here are 1..16, so no pair is 0x666 apart: the first
        // candidate (channel 0, the oldest) is kept and it is the one the
        // newcomer takes.
        let newcomer = arbiter
            .submit(&request("NEW", facts(2, 0), VOLUME_SCALE), 200)
            .expect("pool slot");
        let actions = arbiter.update_tick(200);
        assert!(started(&actions).contains(&newcomer));
        assert_eq!(stopped(&actions), vec![ids[0]]);
    }

    /// `Limit=0` is unlimited (`if (Voc+0x48 != 0 && ...)`).
    #[test]
    fn limit_zero_is_unlimited() {
        let mut arbiter = SoundArbiter::new(0);
        for _ in 0..6 {
            arbiter
                .submit(&request("CUE", facts(2, 0), VOLUME_SCALE), 0)
                .expect("pool slot");
        }
        let actions = arbiter.update_tick(100);
        assert!(stopped(&actions).is_empty());
        assert_eq!(started(&actions).len(), 6);
    }

    /// A `Control=loop` cue with `Loop=` absent sustains for as long as its
    /// owner re-drives the handle, and dies on the first pass after the owner
    /// clears it (`UpdateState` state 3, `0x004057DC`).
    #[test]
    fn an_owner_driven_loop_sustains_until_the_handle_is_cleared() {
        let mut arbiter = SoundArbiter::new(0);
        let mut loop_facts = facts(1, 3);
        loop_facts.control = control::LOOP | control::RANDOM | control::ALL;
        let event = arbiter
            .submit(
                &request("ROCKETEERMOVELOOP", loop_facts, VOLUME_SCALE / 4),
                0,
            )
            .expect("pool slot");
        arbiter.set_loop_handle(1234, Some(event), "ROCKETEERMOVELOOP");

        let actions = arbiter.update_tick(100);
        assert!(matches!(
            actions
                .iter()
                .find(|a| matches!(a, ArbiterAction::Start { .. })),
            Some(ArbiterAction::Start {
                sustaining: true,
                ..
            })
        ));
        // Ten more passes with the handle intact: still alive.
        for pass in 1..=10 {
            let actions = arbiter.update_tick(100 + pass * 40);
            assert!(stopped(&actions).is_empty(), "pass {pass} stopped the loop");
        }
        assert!(arbiter.advance_loop(event));

        arbiter.set_loop_handle(1234, None, "ROCKETEERMOVELOOP");
        let actions = arbiter.update_tick(1000);
        assert_eq!(stopped(&actions), vec![event]);
    }

    /// A looping event with no owner at all is killed on its first serviced
    /// pass in state 3: the leash reads `event+0x278` and falls straight to
    /// the kill when it is null.
    #[test]
    fn a_loop_without_an_owner_dies_immediately() {
        let mut arbiter = SoundArbiter::new(0);
        let mut loop_facts = facts(1, 0);
        loop_facts.control = control::LOOP;
        let event = arbiter
            .submit(&request("ORPHAN", loop_facts, VOLUME_SCALE), 0)
            .expect("pool slot");
        arbiter.update_tick(100);
        let actions = arbiter.update_tick(200);
        assert_eq!(stopped(&actions), vec![event]);
    }

    /// `Loop=N` is a finite budget: `AdvancePlaylist` allows another pass
    /// only while `iteration < Loop - 1`.
    #[test]
    fn a_finite_loop_budget_runs_out() {
        let mut arbiter = SoundArbiter::new(0);
        let mut loop_facts = facts(2, 0);
        loop_facts.control = control::LOOP;
        loop_facts.loop_count = 3;
        let event = arbiter
            .submit(&request("THREE", loop_facts, VOLUME_SCALE), 0)
            .expect("pool slot");
        arbiter.update_tick(100);
        assert!(arbiter.advance_loop(event));
        assert!(arbiter.advance_loop(event));
        assert!(!arbiter.advance_loop(event));
    }

    /// The pre-delay is a real wait, and the `0x21` ms floor discards a
    /// shorter draw entirely (`if (iVar5 < 0x21) return;`). `[GTNK]
    /// MoveSound=GrizzlyTankMoveStart` is the stock shape: `Control= random
    /// predelay`, `Delay=0 400`.
    #[test]
    fn a_predelay_holds_the_cue_and_a_sub_floor_draw_is_discarded() {
        let mut delayed = facts(2, 0);
        delayed.control = control::PREDELAY;
        delayed.delay_ms = (0, 400);

        let mut arbiter = SoundArbiter::new(0);
        let mut held = request("GRIZZLYSTART", delayed, VOLUME_SCALE);
        held.predelay_ms = 300;
        let event = arbiter.submit(&held, 0).expect("pool slot");
        assert!(started(&arbiter.update_tick(40)).is_empty());
        assert!(started(&arbiter.update_tick(200)).is_empty());
        assert!(started(&arbiter.update_tick(400)).contains(&event));

        // A draw under the floor never parks the event.
        let mut arbiter = SoundArbiter::new(0);
        let mut prompt = request("GRIZZLYSTART", delayed, VOLUME_SCALE);
        prompt.predelay_ms = PREDELAY_FLOOR_MS - 1;
        let event = arbiter.submit(&prompt, 0).expect("pool slot");
        assert!(started(&arbiter.update_tick(40)).contains(&event));

        // Without `Control=predelay`/`ambient` the draw is ignored outright:
        // `if ((Voc.Control & 0x88) == 0) return;`.
        let mut arbiter = SoundArbiter::new(0);
        let mut plain = request("PLAIN", facts(2, 0), VOLUME_SCALE);
        plain.predelay_ms = 300;
        let event = arbiter.submit(&plain, 0).expect("pool slot");
        assert!(started(&arbiter.update_tick(40)).contains(&event));
    }

    /// Pause suspends events rather than stopping the service: bookkeeping
    /// keeps running, the pre-delay clock freezes, and nothing starts.
    #[test]
    fn suspension_freezes_the_predelay_and_blocks_the_start_pass() {
        let mut arbiter = SoundArbiter::new(0);
        let mut delayed = facts(2, 0);
        delayed.control = control::PREDELAY;
        delayed.delay_ms = (100, 100);
        let mut held = request("CUE", delayed, VOLUME_SCALE);
        held.predelay_ms = 100;
        let event = arbiter.submit(&held, 0).expect("pool slot");
        arbiter.update_tick(10);
        arbiter.suspend_all(50);
        // 10 seconds of paused passes: nothing starts, and the service keeps
        // running throughout.
        for pass in 0..10 {
            assert!(started(&arbiter.update_tick(100 + pass * 1000)).is_empty());
        }
        assert_eq!(arbiter.live_event_count(), 1);
        arbiter.resume_all(10_100);
        assert!(started(&arbiter.update_tick(10_140)).is_empty());
        assert!(started(&arbiter.update_tick(10_200)).contains(&event));
    }

    /// The entry-level layer takes the whole losing entry, quietest bucket
    /// first, and never touches an equal tier.
    #[test]
    fn sample_memory_preemption_stops_every_event_of_the_lowest_priority_entry() {
        let mut arbiter = SoundArbiter::new(0);
        let a = arbiter
            .submit(&request("QUIET", facts(1, 0), VOLUME_SCALE / 8), 0)
            .expect("pool slot");
        let b = arbiter
            .submit(&request("QUIET", facts(1, 0), VOLUME_SCALE / 8), 0)
            .expect("pool slot");
        let peer = arbiter
            .submit(&request("PEER", facts(3, 0), VOLUME_SCALE), 0)
            .expect("pool slot");
        arbiter.update_tick(100);

        // Equal priority is never preempted here (`for p in 0..prio-1`).
        assert!(arbiter.preempt_for_sample_memory(1).is_empty());
        // A priority-3 caller takes the whole `QUIET` entry, both instances.
        let mut victims = arbiter.preempt_for_sample_memory(3);
        victims.sort();
        assert_eq!(victims, vec![a, b]);
        assert!(arbiter.event(peer).is_some_and(|event| !event.is_dead()));
    }

    /// The many-sounds limiter is a *volume budget*, not a sound count: each
    /// live event contributes `Volume= / 5`, and the master is scaled to hold
    /// the total at 100 once the sum passes it. Seven events at the stock
    /// `[Defaults] Volume=80` (linear 13107, contribution 15) cross it.
    #[test]
    fn the_many_sounds_limiter_engages_on_the_summed_volume_budget() {
        let mut arbiter = SoundArbiter::new(0);
        let mut quiet = facts(2, 0);
        quiet.entry_volume_linear = 13107; // Volume=80
        for index in 0..6 {
            arbiter
                .submit(&request(&format!("C{index}"), quiet, VOLUME_SCALE), 0)
                .expect("pool slot");
        }
        arbiter.update_tick(100);
        assert_eq!(arbiter.many_sounds_linear(), VOLUME_SCALE);

        arbiter
            .submit(&request("C6", quiet, VOLUME_SCALE), 200)
            .expect("pool slot");
        arbiter.update_tick(200);
        // The scaler glides rather than jumping (`SetTarget`, not
        // `SetTargetImmediate`), so it has only started to move here.
        arbiter.update_tick(4000);
        assert!(
            arbiter.many_sounds_linear() < VOLUME_SCALE,
            "limiter never engaged at a 105-unit budget"
        );
    }

    /// The ramp is `0x4000` over 1000 ms, and it is the *only* fade native
    /// applies. A change on a playing event glides; a change before it
    /// starts snaps.
    #[test]
    fn the_volume_ramp_glides_over_one_second_and_snaps_before_playback() {
        let mut interp = VolumeInterp::new(0, 0);
        interp.set_target(VOLUME_SCALE, 0);
        // rate = (0x4000 << 16) / 1000 = 0x10624D, truncated exactly as the
        // literal `VolumeInterp::Init @ 0x0040712B` writes it, so the ramp
        // lands one unit short at the nominal half and full marks.
        interp.tick(500);
        assert_eq!(interp.value(), VOLUME_SCALE / 2 - 1);
        interp.tick(1000);
        assert_eq!(interp.value(), VOLUME_SCALE - 1);
        interp.tick(1001);
        assert_eq!(interp.value(), VOLUME_SCALE);

        let mut arbiter = SoundArbiter::new(0);
        let event = arbiter
            .submit(&request("CUE", facts(2, 0), VOLUME_SCALE), 0)
            .expect("pool slot");
        // Before playback: snap.
        arbiter.set_volume(event, 0, 0);
        assert_eq!(arbiter.live_gain(event), Some((0, VOLUME_SCALE / 2)));
        arbiter.update_tick(40);
        // Playing: glide.
        arbiter.set_volume(event, VOLUME_SCALE, 40);
        assert_eq!(arbiter.live_gain(event).map(|(v, _)| v), Some(0));
        arbiter.update_tick(540);
        assert_eq!(
            arbiter.live_gain(event).map(|(v, _)| v),
            Some(VOLUME_SCALE / 2 - 1)
        );
    }

    /// The pump gate is `> 33 ms`, not `>=`.
    #[test]
    fn the_pump_gate_is_strictly_more_than_thirty_three_milliseconds() {
        let mut arbiter = SoundArbiter::new(0);
        assert!(arbiter.pump_due(0));
        arbiter.update_tick(0);
        assert!(!arbiter.pump_due(PUMP_PERIOD_MS));
        assert!(arbiter.pump_due(PUMP_PERIOD_MS + 1));
    }

    /// A stale handle (the event was recycled) is cleared by the owner-side
    /// check rather than resurrecting a different cue.
    #[test]
    fn a_recycled_event_invalidates_its_owner_handle() {
        let mut arbiter = SoundArbiter::new(0);
        let mut loop_facts = facts(2, 0);
        loop_facts.control = control::LOOP;
        let first = arbiter
            .submit(&request("CUE", loop_facts, VOLUME_SCALE), 0)
            .expect("pool slot");
        arbiter.set_loop_handle(7, Some(first), "CUE");
        assert_eq!(arbiter.validate_loop_handle(7), Some(first));
        arbiter.stop(first);
        arbiter.update_tick(100);
        assert_eq!(arbiter.validate_loop_handle(7), None);
    }

    /// `AdvancePlaylist @ 0x004047B0` opens with
    /// `if ((Voc+0x58 < 0x21) || (flags & 0x20))`, so a `Control=loop` entry
    /// whose `Delay=` low bound is 33 ms or more never reaches the LOOP
    /// branch: its sustain is the streaming callback re-drawing
    /// `RandomRanged(Delay.min, Delay.max)` between samples, not a chained
    /// restart. 24 stock entries have that shape — the sustain set of
    /// [`PREDELAY_FLOOR_MS`] — and every one is `Control=ambient` (`_Amb_*`,
    /// `PropagandaTruck`, `CruiseShipAmbience`) except the debug
    /// `TestRandomLoopDelayAll`.
    #[test]
    fn a_loop_entry_with_a_delay_floor_never_reaches_the_loop_branch() {
        let mut spaced = facts(2, 0);
        spaced.control = control::LOOP;
        spaced.delay_ms = (PREDELAY_FLOOR_MS, 5000);
        let mut arbiter = SoundArbiter::new(0);
        let event = arbiter
            .submit(&request("AMB", spaced, VOLUME_SCALE), 0)
            .expect("pool slot");
        arbiter.update_tick(100);
        assert!(!arbiter.advance_loop(event));

        // One millisecond under the floor and the same entry loops.
        let mut tight = spaced;
        tight.delay_ms = (PREDELAY_FLOOR_MS - 1, 5000);
        let mut arbiter = SoundArbiter::new(0);
        let event = arbiter
            .submit(&request("AMB", tight, VOLUME_SCALE), 0)
            .expect("pool slot");
        arbiter.update_tick(100);
        assert!(arbiter.advance_loop(event));
    }

    /// The ranking insert applies no bound to the volume column
    /// (`0x004044CA LEA EBX,[EAX*8 + 0x87de38]`, `0x0040454D
    /// LEA ECX,[EBX + ECX*4]`), so a full-scale instance's bucket
    /// `0x4000 / 1638 = 10` is written at `base + 120*prio + 120` —
    /// `bucket[prio + 1][0]`. A full-volume entry therefore reads one
    /// priority tier higher to `FindLowestPriority` and survives a caller
    /// that takes its quieter same-priority peer.
    #[test]
    fn a_full_volume_entry_spills_into_the_next_priority_row() {
        let mut arbiter = SoundArbiter::new(0);
        let loud = arbiter
            .submit(&request("LOUD", facts(2, 0), VOLUME_SCALE), 0)
            .expect("pool slot");
        let quiet = arbiter
            .submit(&request("QUIET", facts(2, 0), VOLUME_SCALE / 4), 0)
            .expect("pool slot");
        arbiter.update_tick(100);

        // `VOLUME_SCALE / 4 = 4096` buckets to 2 and stays in row 2 (slot
        // 22); `VOLUME_SCALE` buckets to 10 and lands at slot 30, which is
        // row 3 bucket 0.
        let loud_entry = arbiter.entry_index("LOUD") as usize;
        let quiet_entry = arbiter.entry_index("QUIET") as usize;
        assert_eq!(
            arbiter.entries[loud_entry].bucket_slot,
            Some(3 * VOLUME_BUCKETS)
        );
        assert_eq!(
            arbiter.entries[quiet_entry].bucket_slot,
            Some(2 * VOLUME_BUCKETS + 2)
        );

        // Rows `[0, 3)` reach row 2, where only the quiet entry sits.
        assert_eq!(arbiter.preempt_for_sample_memory(3), vec![quiet]);
        // Asked again, the same caller finds nothing: the loud entry is out
        // of its reach. Clamping the column to 9 would have put it in row 2
        // and surrendered it here.
        assert!(arbiter.preempt_for_sample_memory(3).is_empty());
        // It takes a caller one tier higher to reach it.
        assert_eq!(arbiter.preempt_for_sample_memory(4), vec![loud]);
    }
}
