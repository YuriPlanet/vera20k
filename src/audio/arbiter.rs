//! The sound channel arbiter: which ordinary channel a cue gets,
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
//! `MOV EDX,0x280 ; MOV ECX,0x12c`) that competes for one of the13 ordinary
//! channels. Startup creates16 DirectSound secondary buffers
//! (`DSoundChannel::CreateAll @ 0x00403530`, requested as
//! `MOV EDX,0x10` at `0x00406C25` with `CMP EAX,0x10 ; JNZ` disabling output
//! unless exactly 16 are made).
//!
//! ## Dependency rules
//! - Part of audio/. Depends on nothing but `std` and `rules::sound_ini`
//!   constants. Does NOT depend on render/, ui/, sim/, or on any
//!   audio device.

use std::collections::BTreeMap;

use crate::rules::sound_ini::{VOLUME_SCALE, control};

/// Product of two native linear volumes: `DSoundBuffer::CombineInterps
/// FUN_004010C0` (`0x004010C7..0x004010D6`), `(a * b) >> 14`.
///
/// **VERA-internal, gamemd equivalent UNCHECKED: both `.clamp`s.** The native
/// volume path is `SHR EDX,0x10; SHR EAX,0x10; IMUL EDX,EAX; SHR EDX,0xe`
/// (`disassemble_function 0x004010C0`, read this session) — no bound at all;
/// the two operands are merely the top halves of 32-bit fixed-point fields, so
/// native accepts anything up to `0xFFFF` on each side and can return well
/// past `0x4000`. (The *pan* path at `0x00401116..0x0040114C` does clamp to
/// `0..0x4000`; the volume path does not, so this is not borrowed from there.)
/// Trigger: an operand outside `0..=0x4000`, which no VERA producer can make —
/// `SoundEntry::volume_linear` comes out of the native `[0, 1]` clamp times
/// 16384, and [`crate::audio::sfx::PlayShifts::volume_linear`] is `0x4000 - ((vshift << 14)/100)`
/// with `vshift` already held to `0..=100` by native `SetVShift @ 0x00406620`.
/// Player effect: none reachable. Frequency: never. Downstream risk: the guard
/// is what keeps [`crate::audio::sfx::native_volume_amplitude`]'s fixed 101-entry table index in
/// bounds, where Rust would panic and gamemd would read past the table.
pub(super) fn combine_linear(a: i32, b: i32) -> i32 {
    (a.clamp(0, VOLUME_SCALE) * b.clamp(0, VOLUME_SCALE)) >> 14
}

/// Ordinary SoundEvent channels after the process stream initializers.
///
/// `AudioSystem::Init @ 0x00406B10` calls `DSoundChannel::CreateAll @
/// 0x00403530` with `MOV EDX,0x10` (`0x00406C25`) and disables audio output
/// entirely unless the call returns exactly16 (`CMP EAX,0x10 ; JNZ` at
/// `0x00406C31`). Its Vox752290 and Speech752AD0 initializers each reserve
/// one; Init_Game's Theme ReadINI720590 reserves another. Each calls407860
/// ->4036C0 ->402440, changing a channel from group1 to group4096/flags8.
/// FindAvailable403632 excludes those streams even while idle. Original
/// startup and saturated-pool controls: tools/input_oracle/unit_voice_playback.
///
/// The app's separate stream outputs retain those reservations until process
/// shutdown. World replacement clears sound events without recreating them.
pub const MAX_CHANNELS: usize = 16 - 3;

/// `SoundEvent` records in the pool: `SoundEventPool::Init @ 0x00403ED0`
/// allocates `MemPool(count = 0x12c, elem = 0x280)`. Up to 300 cues can be
/// alive at once; only [`MAX_CHANNELS`] of them hold a channel.
pub const MAX_SOUND_EVENTS: usize = 300;

/// The service period. `AudioSystem::Pump @ 0x00406F70` runs a pass only when
/// more than `0x21` ms have elapsed since the last admitted one
/// (`now.wrapping_sub(last) > 0x21` on the 64-bit timestamp), and it is driven from
/// `Network_ServiceLoop @ 0x0048D080` — the message/service loop, not the
/// simulation frame. Menus, modal dialogs, loading screens and the frame
/// pacer's idle all keep the audio service running.
pub const PUMP_PERIOD_MS: u64 = 0x21;

/// One process AudioSystem406F70 cadence for Sound, Vox and Theme. Device
/// callbacks are independent; this clock gates only the main-thread service.
#[derive(Default)]
pub(crate) struct AudioServiceClock {
    /// Native process global87E760/764 starts at zero;406FD5 writes both
    /// halves only after an admitted service.
    last_ms: u64,
}

impl AudioServiceClock {
    pub(crate) fn admit(&mut self, now_ms: u64) -> bool {
        //406FA6..406FB5 subtract both timestamp halves before the >33 gate.
        if now_ms.wrapping_sub(self.last_ms) <= PUMP_PERIOD_MS {
            return false;
        }
        self.last_ms = now_ms;
        true
    }
}

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
/// result below `0x21` (signed, `0x0040574F`).
///
/// The same value splits the playout. From a `Delay=` low bound of `0x21` up
/// ([`delay_below_floor`]) every start pass loads one sample that
/// `SoundEvent::SelectNextSample @ 0x00404BB0` picks (`sfx/sample_queue.rs`)
/// and plays it alone. Whenever the high bound reaches the floor,
/// `OnChannelSampleEnd @ 0x00405AC0` may draw a gap after the buffer, which
/// [`SoundArbiter::notify_playout_ended`] waits out in state 2 before the next
/// pass ([`PlayoutEnd`]).
pub const PREDELAY_FLOOR_MS: i32 = 0x21;

/// `Delay.min < 0x21` as `SoundEvent::LoadSamples @ 0x004048B0`,
/// `PreparePlayout @ 0x00404700` and `AdvancePlaylist @ 0x004047B0` test it:
/// unsigned (`JC`/`JNC`), so a negative low bound counts as above the floor.
/// `OnChannelSampleEnd @ 0x00405AC0` tests `Delay.max` and its gap draw the
/// same way.
pub const fn delay_below_floor(delay_ms: i32) -> bool {
    (delay_ms as u32) < PREDELAY_FLOOR_MS as u32
}

/// What `SoundEvent::OnChannelDone @ 0x00405A00` reads when a channel runs out
/// of buffers: the gap `OnChannelSampleEnd @ 0x00405AC0` drew (`+0x13C`, 0 for
/// none), whether it held a next buffer back for after that gap (`+0x148`),
/// and SelectNextSample's remaining queue (`+0x268`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PlayoutEnd {
    pub gap_ms: u32,
    pub next: bool,
    pub queue_count: i32,
}

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
    /// `0x40` — a pending release consumed by state3 at40589E. The current
    /// buffer stops and unused decay may replace it; NO_REPLAY remains set.
    pub const RELEASED: u32 = 0x40;
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
    /// 4 — finished; the next service stops and reaps it. The serial and
    /// handle can still be valid until that service sets the DEAD flag.
    Finished,
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

/// A delayed output belongs to this allocation, never to a reused pool slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EventToken {
    pub event: EventId,
    pub serial: u32,
}

/// Separate native handles on one object must never alias. Existing positional
/// owners include the retained animation, Foot, Gattling and bomb identities;
/// Techno+4DC is a distinct non-positional handle even for the same object ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HandleOwner {
    Positional(u64),
    UnitVoice(u64),
    /// A Voc750E20 request owns its handle independently of the source object.
    Coordinate(u64),
    /// A process-global VocHandle, distinct from any object or coordinate.
    Global(u32),
}

impl From<u64> for HandleOwner {
    fn from(object: u64) -> Self {
        Self::Positional(object)
    }
}

/// Service-time work at the two native phases that may consume Main RNG.
/// Pool/Limit/channel rejection must happen before these calls. The sample
/// owner retains shifts and decoded buffers; the arbiter retains admission.
pub trait PlaybackService {
    /// UpdateState4055C0, immediately after acquiring a channel: draw the
    /// frequency, volume and optional pre-delay shifts, returning the delay.
    fn channel_acquired(&mut self, event: EventId) -> i32;
    /// LoadSamples4048B0, before PreparePlayout's NO_REPLAY test, again
    /// after each eviction. SelectNextSample404BB0 reads `plays_attack`
    /// (flags8 clear) for an above-floor entry's attack slot.
    fn load_samples(&mut self, event: EventId, plays_attack: bool) -> bool;
    /// PreparePlayout404700 in the final start pass. A failed preparation
    /// enters the existing lower-priority retry path.
    fn prepare_playout(&mut self, event: EventId, plays_attack: bool) -> bool;
    /// StartPlayback4054A0 fills the native ring before the next Ready
    /// event's Load/Prepare. Deferring its draws until all events prepare
    /// would reorder the one process Main stream.
    fn start_playout(&mut self, event: EventId, pan: i32) -> bool;
    /// DriverStop4025B0 in405B8C/405319 completes before channel reuse,
    /// later admission draws and sample loading. Cancel retained sources at
    /// that boundary; collected Stop actions still retire their payloads.
    fn stop_playout(&mut self, event: EventId);
}

/// The 16-byte `{ SoundEvent*, serial, VocClass*, &g_AudioIndex }` an owner
/// holds for a sustained cue (`SoundEvent::SetLoopHandle @ 0x004060F0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct VocHandle {
    event: Option<EventId>,
    serial: u32,
    entry: u32,
    /// The named sound is a loop `AnimClass::UpdateLoopingSound` allocates
    /// again (`AudioEventClass::IsLoopable`), so the handle outlives its event.
    loopable: bool,
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
    /// The entry holds samples (`+0x134` non-zero, `+0x0C` set). Without
    /// them `UpdateState` state 0 kills its events before they take a channel.
    pub samples_ok: bool,
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
    /// The event's starting linear volume (spatial x entry). Channel-local
    /// VShift4054A0 does not affect Limit/priority ranking.
    pub volume_linear: i32,
    /// Pan `0..=0x4000`, `0x2000` centred.
    pub pan: i32,
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
    },
    /// Live gain update while playing (the `VolumeInterp` glide).
    Gain {
        event: EventId,
        volume_linear: i32,
        pan: i32,
    },
    /// Released state3 stops its current buffer and selects decay4058E5.
    Decay { event: EventId },
    /// Retire the output after the synchronous source cancellation. The
    /// arbiter owns whether its dead slot has already been reaped.
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
    /// `+0x140`, absolute while running and remaining while suspended.
    predelay_deadline_ms: u64,
    predelay_remaining_ms: u64,
    /// `+0x154`. Written only by `AllocateFromPool` (to 0) anywhere in the
    /// audio module, so the effective priority in stock play is `0..4`.
    priority_bonus: i32,
    /// `+0x158`. Non-zero blocks the start pass, freezes the pre-delay and
    /// removes the event from the many-sounds volume sum.
    suspend_depth: i32,
    /// The owner that holds this event's `VocHandle` (`+0x278`).
    handle_owner: Option<HandleOwner>,
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
    handles: BTreeMap<HandleOwner, VocHandle>,
    stamp: u32,
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
    pub fn loop_handle_key(&self, owner: HandleOwner) -> Option<&str> {
        let handle = self.handles.get(&owner)?;
        handle.event?;
        self.names.get(handle.entry as usize).map(String::as_str)
    }

    /// The identity an owner's handle names, with or without a live event.
    pub fn handle_sound_key(&self, owner: HandleOwner) -> Option<&str> {
        let handle = self.handles.get(&owner)?;
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

        // Original405190 owns full-pool DEAD-only reaping and one retry,
        // shared by750920/7509E0. Finished state4 remains occupied until
        // its ordinary Sound service actually stops it.
        if self.events.len() == MAX_SOUND_EVENTS && self.events.iter().all(Option::is_some) {
            for id in self.order.clone() {
                if self.event(id).is_some_and(EventRec::is_dead) {
                    self.reap(id);
                }
            }
        }

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
        // 0x2000. Pending SetVolume4061D0 sets and ticks immediately; pending
        // SetPan406270 only sets its target, leaving current at the centre
        // until Sound service ticks the group. Executed pending controls:
        // tools/spatial_oracle/building_damage_sound_joined.json.
        let mut volume = VolumeInterp::new(VOLUME_SCALE, now_ms);
        volume.set_target_immediate(request.volume_linear.clamp(0, VOLUME_SCALE));
        volume.tick(now_ms);
        let mut pan = VolumeInterp::new(VOLUME_SCALE / 2, now_ms);
        pan.set_target_immediate(request.pan.clamp(0, VOLUME_SCALE));

        self.events[slot] = Some(EventRec {
            entry,
            state: EventState::NeedsChannel,
            resume_state: EventState::Ready,
            flags: 0,
            channel: None,
            volume,
            pan,
            serial,
            predelay_deadline_ms: 0,
            predelay_remaining_ms: 0,
            priority_bonus: 0,
            suspend_depth: 0,
            handle_owner: None,
        });
        self.order.push(id);
        Some(id)
    }

    /// `VocHandle::ValidateOrClear @ 0x00406130`: the owner-side check. A
    /// handle whose event has been recycled (different entry) or invalidated
    /// (serial mismatch) is cleared and reported as empty.
    pub fn validate_loop_handle(&mut self, owner: HandleOwner) -> Option<EventId> {
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
    pub fn set_loop_handle(&mut self, owner: HandleOwner, event: Option<EventId>, key: &str) {
        let entry = self.entry_index(key);
        let record = event.and_then(|id| self.event(id));
        let serial = record.map_or(0, |event| event.serial);
        let loopable = record
            .and_then(|event| self.entries[event.entry as usize].facts)
            .is_some_and(|facts| facts.is_loopable());
        self.handles.insert(
            owner,
            VocHandle {
                event,
                serial,
                entry,
                loopable,
            },
        );
        if let Some(id) = event
            && let Some(record) = self.event_mut(id)
        {
            record.handle_owner = Some(owner);
        }
    }

    /// `SoundEvent::SetLoopHandle(handle, 0, voc)`: the handle names `key`
    /// with no event (`VocHandle+8` keeps the sound id), as `VocClass::PlayAt
    /// @ 0x007509E0` leaves it when the first play is inaudible and
    /// `AnimClass::UpdateLoopingSound` when the volume falls to zero
    /// (`0x00750E0C..0x00750E11`). Only Release and StopAndClear clear it.
    pub fn keep_loop_sound(&mut self, owner: HandleOwner, key: &str, loopable: bool) {
        let entry = self.entry_index(key);
        self.handles.insert(
            owner,
            VocHandle {
                event: None,
                serial: 0,
                entry,
                loopable,
            },
        );
    }

    /// The loopable sound an owner's handle keeps without a live event: what
    /// `AnimClass::UpdateLoopingSound` allocates again once the owner is
    /// audible (`0x00750D8C..0x00750DB3`).
    pub fn kept_loop_key(&mut self, owner: HandleOwner) -> Option<&str> {
        if self.validate_loop_handle(owner).is_some() {
            return None;
        }
        let handle = self.handles.get(&owner).filter(|handle| handle.loopable)?;
        self.names.get(handle.entry as usize).map(String::as_str)
    }

    /// Drop an owner's handle entirely (the owner object is gone).
    pub fn clear_loop_handle(&mut self, owner: HandleOwner) {
        self.handles.remove(&owner);
    }

    /// Owners to re-drive: those whose handle names a live event, and those
    /// whose handle keeps a loopable sound after its event ended (inaudible,
    /// or killed by `Limit=`), which native's owner update allocates again.
    /// Any other handle whose event is gone is dropped, the bookkeeping half
    /// of `VocHandle::ValidateOrClear`.
    pub fn loop_handle_owners(&mut self) -> Vec<u64> {
        let owners: Vec<HandleOwner> = self.handles.keys().copied().collect();
        let mut live = Vec::new();
        for owner in owners {
            let HandleOwner::Positional(object) = owner else {
                continue;
            };
            if self.validate_loop_handle(owner).is_some()
                || self
                    .handles
                    .get(&owner)
                    .is_some_and(|handle| handle.loopable)
            {
                live.push(object);
            } else {
                self.handles.remove(&owner);
            }
        }
        live
    }

    /// `SoundEvent::Release @ 0x00406060` on an owner's handle. Release its
    /// current event before clearing the handle. One-shots finish; uncounted
    /// loops switch to decay at the next state3 service.
    pub fn release_owner(&mut self, owner: HandleOwner) {
        if let Some(event) = self.validate_loop_handle(owner) {
            self.release(event);
        }
        // Native406060 clears handle.type but retains stale pointer/serial.
        // Validate406149..406162 rejects that type0 and clears the pointer;
        // GetType406170/Update750D60 cannot reallocate it. Removing the
        // typed Rust handle preserves that authority without stale storage.
        self.clear_loop_handle(owner);
    }

    /// FootLimbo's handle405FD0: unlike Release406060, every valid event
    /// receives flags0x60, including counted loops and one-shots. Native
    /// brackets a live channel while writing these flags; state3 consumes
    /// RELEASED on the next service. The handle immediately stops naming it.
    pub fn detach_owner(&mut self, owner: HandleOwner) {
        if let Some(id) = self.validate_loop_handle(owner)
            && let Some(event) = self.event_mut(id)
            && event.flags & event_flags::NO_REPLAY == 0
        {
            event.flags |= event_flags::NO_REPLAY | event_flags::RELEASED;
        }
        self.clear_loop_handle(owner);
    }

    /// `SoundEvent::Release @ 0x00406060`'s event half: a live looping event
    /// whose entry has no `Loop=` count and is not already released sets
    /// flags0x60. The next state3 service replaces its buffer with decay.
    pub fn release(&mut self, id: EventId) {
        let Some(event) = self.event(id) else {
            return;
        };
        if event.is_dead() || event.flags & event_flags::NO_REPLAY != 0 {
            return;
        }
        let Some(facts) = self.entries[event.entry as usize].facts else {
            return;
        };
        if facts.control & control::LOOP == 0 || facts.loop_count != 0 {
            return;
        }
        if let Some(event) = self.event_mut(id) {
            event.flags |= event_flags::NO_REPLAY | event_flags::RELEASED;
        }
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

    pub(crate) fn event_token(&self, id: EventId) -> Option<EventToken> {
        self.event(id)
            .filter(|event| !event.is_dead() && event.serial != 0)
            .map(|event| EventToken {
                event: id,
                serial: event.serial,
            })
    }

    pub(crate) fn is_live_token(&self, token: EventToken) -> bool {
        self.event_token(token.event) == Some(token)
    }

    /// A dead record with serial0 remains physically occupied until Reap.
    /// A different live serial proves that this token was already reaped
    /// before slot reuse. A reused dead slot waits conservatively for Reap.
    pub(crate) fn is_token_retired(&self, token: EventToken) -> bool {
        self.event(token.event)
            .is_none_or(|event| event.serial != 0 && event.serial != token.serial)
    }

    pub(crate) fn event_slot_occupied(&self, id: EventId) -> bool {
        self.event(id).is_some()
    }

    #[cfg(test)]
    pub(crate) fn native_event_snapshot(&self, id: EventId) -> Option<serde_json::Value> {
        // Read-only witness of this owner, not a fixture-supplied decision.
        // Native4055C0 state numbers and405A00 endpoint fields.
        let event = self.event(id)?;
        let state = match event.state {
            EventState::NeedsChannel => 0,
            EventState::Ready => 1,
            EventState::PreDelay => 2,
            EventState::Playing => 3,
            EventState::Finished => 4,
        };
        Some(serde_json::json!({
            "state": state, "flags": event.flags, "serial": event.serial,
            "volume_current_fixed": event.volume.current,
            "volume_target_fixed": event.volume.target,
            "pan_current_fixed": event.pan.current,
            "pan_target_fixed": event.pan.target,
        }))
    }

    pub(crate) fn token_has_channel(&self, token: EventToken) -> bool {
        self.is_live_token(token)
            && self
                .event(token.event)
                .and_then(|event| event.channel)
                .is_some_and(|index| self.channels[index].event == Some(token.event))
    }

    pub(crate) fn playout_no_replay(&self, id: EventId) -> bool {
        self.event(id)
            .is_some_and(|event| event.flags & event_flags::NO_REPLAY != 0)
    }

    /// `SoundEvent::SetVolume @ 0x004061D0`: snap while the event has not
    /// started, glide once it is playing.
    /// Input is spatial linear volume. Both native arms multiply by entry
    /// +1C then SHR14 (4061EF..4061FB /406225..406233);750A84 and750DDB
    /// supply that same input on initial reuse and retained re-drive.
    pub fn set_volume(&mut self, id: EventId, linear: i32, now_ms: u64) {
        let Some(event) = self.event(id) else {
            return;
        };
        let linear = combine_linear(linear, self.facts_of(event).entry_volume_linear);
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

    /// `SoundEvent::SetPan @ 0x00406270`: playing uses407170; pending only
    /// tail-jumps407150. Unlike volume, it does not immediately tick4071C0.
    pub fn set_pan(&mut self, id: EventId, pan: i32, now_ms: u64) {
        let pan = pan.clamp(0, VOLUME_SCALE);
        let Some(event) = self.event_mut(id) else {
            return;
        };
        if event.state == EventState::Playing {
            event.pan.set_target(pan, now_ms);
        } else {
            event.pan.set_target_immediate(pan);
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

    /// Load's SoundSystem404E70 kills and reaps every global event, while
    /// the outgoing objects' destruction and loaded constructors discard
    /// their handles. Keep catalog entries, serial allocation and service
    /// epochs; neither operation constructs another process audio system.
    pub fn clear_for_world_replacement(&mut self) {
        for id in std::mem::take(&mut self.order) {
            self.stop(id);
            self.reap(id);
        }
        self.handles.clear();
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
            event.state = EventState::Finished;
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
            self.suspend_event(id, now_ms);
        }
    }

    /// `SoundEvent::Suspend4053E0`, shared by404FD0 and coordinate751040.
    pub(crate) fn suspend_event(&mut self, id: EventId, now_ms: u64) {
        let Some(event) = self.event_mut(id) else {
            return;
        };
        if event.suspend_depth == 0 && event.state == EventState::PreDelay {
            event.predelay_remaining_ms = event.predelay_deadline_ms.saturating_sub(now_ms);
        }
        event.suspend_depth = event.suspend_depth.wrapping_add(1);
    }

    /// `SoundSystem::ResumeAll @ 0x00405040` (`GamePause::Exit @ 0x00406F40`):
    /// decrement, clamped at zero, and re-absolutise the pre-delay.
    pub fn resume_all(&mut self, now_ms: u64) {
        for id in self.order.clone() {
            self.resume_event(id, now_ms);
        }
    }

    /// `SoundEvent::Resume405430`, shared by751080 and405040.
    pub(crate) fn resume_event(&mut self, id: EventId, now_ms: u64) {
        let Some(event) = self.event_mut(id) else {
            return;
        };
        if event.suspend_depth == 0 {
            return;
        }
        event.suspend_depth = event.suspend_depth.wrapping_sub(1).max(0);
        if event.suspend_depth == 0 && event.state == EventState::PreDelay {
            event.predelay_deadline_ms = now_ms.saturating_add(event.predelay_remaining_ms);
        }
    }

    /// The device reporting that a channel ran out of buffers:
    /// `SoundEvent::OnChannelDone @ 0x00405A00`. It clears PLAYING, and an
    /// event already in state 4 stays there. An event with a held-back next
    /// buffer, a remaining queue or a gap goes back to state 1, through state
    /// 2 for the gap (resume 1, `+0x140 = now + gap`, or the gap alone while
    /// suspended). Any other event reaches state 4. Neither path releases
    /// the channel or invalidates the serial, so Techno6F9EBB can still
    /// validate the handle before the next service reaches 405974.
    ///
    /// Returns whether the event plays again.
    pub(crate) fn notify_playout_ended(
        &mut self,
        token: EventToken,
        end: PlayoutEnd,
        now_ms: u64,
    ) -> bool {
        if !self.is_live_token(token) {
            return false;
        }
        let id = token.event;
        let Some(event) = self.event_mut(id) else {
            return false;
        };
        event.flags &= !event_flags::PLAYING;
        let channel = event.channel;
        let continues = event.state != EventState::Finished
            && (end.next || end.queue_count != 0 || end.gap_ms != 0);
        if event.state != EventState::Finished {
            let next = if continues {
                EventState::Ready
            } else {
                EventState::Finished
            };
            if end.gap_ms == 0 {
                event.state = next;
            } else {
                event.resume_state = next;
                if event.suspend_depth == 0 {
                    event.predelay_deadline_ms = now_ms.saturating_add(u64::from(end.gap_ms));
                } else {
                    event.predelay_remaining_ms = u64::from(end.gap_ms);
                }
                event.state = EventState::PreDelay;
            }
        }
        if let Some(channel) = channel
            && self.channels[channel].event == Some(id)
        {
            self.channels[channel].playing = false;
        }
        continues
    }

    /// DriverStop4025B0's stop callback (`0x004059F0`) sets state 4: a
    /// released event that starts no decay ends with its channel's buffer.
    pub(crate) fn notify_playout_stopped(&mut self, token: EventToken) {
        if !self.is_live_token(token) {
            return;
        }
        let id = token.event;
        let Some(event) = self.event_mut(id) else {
            return;
        };
        event.flags &= !event_flags::PLAYING;
        event.state = EventState::Finished;
        if let Some(channel) = event.channel
            && self.channels[channel].event == Some(id)
        {
            self.channels[channel].playing = false;
        }
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
                samples_ok: true,
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
    fn allocate_channel(
        &mut self,
        id: EventId,
        priority: i32,
        playback: &mut impl PlaybackService,
    ) -> Option<usize> {
        let index = self.find_available_channel()?;
        if self.channels[index].busy && priority < self.channels[index].priority {
            return None;
        }
        // Original405B8C invokes DriverStop before replacing channel fields
        // or drawing the new event's shifts. The old allocation remains
        // logically present until Reap, but its source must stop now.
        if let Some(old) = self.channels[index].event {
            playback.stop_playout(old);
        }
        // The dispossessed event is deliberately left pointing at the channel:
        // native never notifies it, and the reaping pass catches it with
        // `puVar3 != *(channel + 0xc0)` (`0x004043B7`) — an event whose
        // channel no longer names it is killed on the spot. That is how a
        // additional equal-priority cue in a full pass silences a cue that
        // took a channel earlier in the same pass.
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
    /// ([`AudioServiceClock`]); this runs the pass unconditionally so tests can
    /// step it.
    pub fn update_tick(
        &mut self,
        now_ms: u64,
        playback: &mut impl PlaybackService,
    ) -> Vec<ArbiterAction> {
        let mut actions = Vec::new();

        self.limit_epoch = self.limit_epoch.wrapping_add(1);
        self.enforce_limits(now_ms, playback, &mut actions);
        self.run_state_machine(now_ms, playback, &mut actions);
        self.reap_and_rank(now_ms, playback, &mut actions);
        self.start_pass(now_ms, playback, &mut actions);
        self.emit_live_gains(&mut actions);
        actions
    }

    /// Pass 1 (`0x0040421C..0x0040433D`): reset each entry's counter on first
    /// touch this epoch, tick the event's interp group, re-sort the event
    /// into the entry's descending-volume list, then enforce `Limit=` by
    /// killing the **tail** — the quietest instance, or with
    /// `Control=interrupt` the oldest already-started one.
    ///
    /// Equal-volume ties without INTERRUPT can cull a pending cue before it
    /// starts (`0x00404312`); stock MenuClick's Limit5 reaches this path.
    fn enforce_limits(
        &mut self,
        now_ms: u64,
        playback: &mut impl PlaybackService,
        actions: &mut Vec<ArbiterAction>,
    ) {
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
            if event.state == EventState::Finished {
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
                    playback.stop_playout(victim);
                    actions.push(ArbiterAction::Stop { event: victim });
                }
                self.entries[entry_index].count -= 1;
            }
        }
    }

    /// Pass 2: `SoundEvent::UpdateState @ 0x004055C0` on every event.
    fn run_state_machine(
        &mut self,
        now_ms: u64,
        playback: &mut impl PlaybackService,
        actions: &mut Vec<ArbiterAction>,
    ) {
        for id in self.order.clone() {
            self.update_state(id, now_ms, playback, actions);
        }
    }

    fn update_state(
        &mut self,
        id: EventId,
        now_ms: u64,
        playback: &mut impl PlaybackService,
        actions: &mut Vec<ArbiterAction>,
    ) {
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
                // `0x0040563C..0x0040564B`: an entry without samples takes the
                // `0x004057A3` kill before any channel or draw.
                let channel = if facts.samples_ok {
                    self.allocate_channel(id, priority, playback)
                } else {
                    None
                };
                match channel {
                    Some(index) => {
                        let predelay_ms = playback.channel_acquired(id);
                        let Some(event) = self.event_mut(id) else {
                            return;
                        };
                        event.channel = Some(index);
                        event.state = EventState::Ready;
                        // `if ((Voc.Control & 0x88) == 0) return;` — no
                        // `Control=predelay` and no `Control=ambient` means no
                        // pre-delay at all, whatever `Delay=` says.
                        if facts.control & (control::PREDELAY | control::AMBIENT) == 0 {
                            return;
                        }
                        // `if (iVar5 < 0x21) return;` — a draw under the
                        // 33 ms floor is discarded and the cue starts at once.
                        if predelay_ms < PREDELAY_FLOOR_MS {
                            return;
                        }
                        event.resume_state = EventState::Ready;
                        if event.suspend_depth == 0 {
                            event.predelay_deadline_ms = now_ms.saturating_add(predelay_ms as u64);
                        } else {
                            event.predelay_remaining_ms = predelay_ms as u64;
                        }
                        event.state = EventState::PreDelay;
                    }
                    None => {
                        // No samples or allocation refused: `0x004057A3` —
                        // release, mark dead, never audible.
                        if self.event(id).is_some_and(|event| !event.is_dead()) {
                            self.release_channel(id);
                            self.kill(id);
                            playback.stop_playout(id);
                            actions.push(ArbiterAction::Stop { event: id });
                        }
                    }
                }
            }
            // Case `0x004055EB`: the pre-delay wait, serviced by the same
            // > 33 ms pass, so pre-delays quantise to the pump period.
            EventState::PreDelay => {
                // A release consumed in state 2 takes `0x0040589E`: DriverStop
                // ends the channel's buffer and its stop callback (`0x004059F0`)
                // sets state 4.
                //
                // RESIDUAL: with `Control=decay` and flags0x10 clear,
                // `0x004058DC` instead restarts the event on its decay buffer.
                // No retail entry that waits in state 2 authors decay.
                if event.flags & event_flags::RELEASED != 0 {
                    let event = self.event_mut(id).expect("current event");
                    event.flags &= !event_flags::RELEASED;
                    if event.channel.is_some() {
                        event.state = EventState::Finished;
                    }
                    return;
                }
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
                if event.flags & event_flags::RELEASED != 0 {
                    // Original40589E..4058FC: consume release once, stop the
                    // current buffer, AdvancePlaylist under NO_REPLAY, then
                    // start its decay. Channel/serial survive until endpoint
                    // and the later Finished service; this is not a hard stop.
                    self.event_mut(id).expect("current event").flags &= !event_flags::RELEASED;
                    actions.push(ArbiterAction::Decay { event: id });
                    return;
                }
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
                    playback.stop_playout(id);
                    actions.push(ArbiterAction::Stop { event: id });
                }
            }
            // Case405974: only the subsequent sound service releases a
            // naturally completed event and invalidates its owner handle.
            EventState::Finished => {
                if !event.is_dead() {
                    self.stop(id);
                    playback.stop_playout(id);
                    actions.push(ArbiterAction::Stop { event: id });
                }
            }
            EventState::Ready => {}
        }
    }

    /// Passes 3-5: clear the buckets, bump the ranking epoch, reap every
    /// event that lost its channel or is already dead, rank the loudest live
    /// instance of each entry into `bucket[priority][volume / 1638]`, and set
    /// the many-sounds target.
    fn reap_and_rank(
        &mut self,
        now_ms: u64,
        playback: &mut impl PlaybackService,
        actions: &mut Vec<ArbiterAction>,
    ) {
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
                playback.stop_playout(id);
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
            // simply not rankable, so `preempt_for_playout` cannot pick
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

    /// PreparePlayout failure retry4045E4..40466B: select a strictly lower
    /// ranked entry, stop/reap all of its events, then retry. NO_REPLAY set by
    /// Limbo405FD0 before first playback is a production trigger; freeing
    /// sample memory cannot make that flag pass PreparePlayout404716.
    ///
    /// RESIDUAL: native's fixed 1 MB sample pool can also fail LoadSamples.
    /// VERA owns decoded buffers without that budget, so memory-starvation
    /// admission differs under high distinct-sample pressure. This shared
    /// retry owner is live for the represented NO_REPLAY rejection.
    fn preempt_for_playout(
        &mut self,
        priority: i32,
        playback: &mut impl PlaybackService,
    ) -> Vec<EventId> {
        let mut stopped = Vec::new();
        let Some(victim_entry) = self.find_lowest_priority(priority) else {
            return stopped;
        };
        for other in self.order.clone() {
            if self
                .event(other)
                .is_some_and(|event| event.entry == victim_entry)
            {
                self.stop(other);
                playback.stop_playout(other);
                self.reap(other);
                stopped.push(other);
            }
        }
        stopped
    }

    /// Pass 6 (`0x004045A6..0x004046C6`): every event in state 1 that is not
    /// suspended runs `LoadSamples` -> `PreparePlayout` ->
    /// (preempt and retry) -> `StartPlayback`.
    ///
    /// Both sample preparation and the NO_REPLAY gate live here: Limbo may
    /// detach an event while it is still queued.
    fn start_pass(
        &mut self,
        now_ms: u64,
        playback: &mut impl PlaybackService,
        actions: &mut Vec<ArbiterAction>,
    ) {
        for id in self.order.clone() {
            let Some(event) = self.event(id) else {
                continue;
            };
            if event.state != EventState::Ready || event.suspend_depth != 0 {
                continue;
            }
            let no_replay = event.flags & event_flags::NO_REPLAY != 0;
            let plays_attack = event.flags & event_flags::STARTED == 0;
            let priority = self.effective_priority(event);
            if event.channel.is_none() {
                self.kill(id);
                playback.stop_playout(id);
                actions.push(ArbiterAction::Stop { event: id });
                continue;
            }
            // Native LoadSamples precedes PreparePlayout's NO_REPLAY gate.
            // A loaded set is retained by the sample owner on retry.
            let mut prepared = playback.load_samples(id, plays_attack)
                && !no_replay
                && playback.prepare_playout(id, plays_attack);
            while !prepared {
                let victims = self.preempt_for_playout(priority, playback);
                if victims.is_empty() {
                    break;
                }
                actions.extend(
                    victims
                        .into_iter()
                        .map(|event| ArbiterAction::Stop { event }),
                );
                prepared = playback.load_samples(id, plays_attack)
                    && !no_replay
                    && playback.prepare_playout(id, plays_attack);
            }
            if !prepared {
                self.stop(id);
                playback.stop_playout(id);
                self.reap(id);
                actions.push(ArbiterAction::Stop { event: id });
                continue;
            }
            // Original404673 unconditionally prepares again after the
            // first success/retry. Each call resets the one playlist; the
            // second returned buffer is the StartPlayback input.
            if !playback.prepare_playout(id, plays_attack) {
                self.stop(id);
                playback.stop_playout(id);
                self.reap(id);
                actions.push(ArbiterAction::Stop { event: id });
                continue;
            }
            // `SoundEvent::StartPlayback @ 0x004054A0`: snap both interps
            // before the buffer starts, set `flags |= 8 | 2`, state 3.
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
            if !playback.start_playout(id, pan) {
                self.stop(id);
                playback.stop_playout(id);
                self.reap(id);
                actions.push(ArbiterAction::Stop { event: id });
                continue;
            }
            actions.push(ArbiterAction::Start {
                event: id,
                volume_linear,
                pan,
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
#[derive(Default)]
pub(crate) struct TestPlayback {
    predelay_ms: i32,
}

#[cfg(test)]
impl PlaybackService for TestPlayback {
    fn channel_acquired(&mut self, _: EventId) -> i32 {
        self.predelay_ms
    }
    fn load_samples(&mut self, _: EventId, _: bool) -> bool {
        true
    }
    fn prepare_playout(&mut self, _: EventId, _: bool) -> bool {
        true
    }

    fn start_playout(&mut self, _: EventId, _: i32) -> bool {
        true
    }

    fn stop_playout(&mut self, _: EventId) {}
}

#[cfg(test)]
#[path = "arbiter_move_sound_tests.rs"]
mod move_sound_tests;

#[cfg(test)]
#[path = "arbiter_coordinate_tests.rs"]
pub(super) mod coordinate_tests;

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
            samples_ok: true,
        }
    }

    fn request(key: &str, facts: EntryFacts, volume_linear: i32) -> PlayRequest {
        PlayRequest {
            key: key.to_owned(),
            facts,
            volume_linear,
            pan: VOLUME_SCALE / 2,
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

    #[test]
    fn channel_reuse_and_limit_stop_old_playout_before_next_admission() {
        struct BoundaryProbe(Vec<(&'static str, EventId)>);
        impl PlaybackService for BoundaryProbe {
            fn channel_acquired(&mut self, event: EventId) -> i32 {
                self.0.push(("admission", event));
                0
            }
            fn load_samples(&mut self, _: EventId, _: bool) -> bool {
                true
            }
            fn prepare_playout(&mut self, _: EventId, _: bool) -> bool {
                true
            }
            fn start_playout(&mut self, _: EventId, _: i32) -> bool {
                true
            }
            fn stop_playout(&mut self, event: EventId) {
                self.0.push(("stop", event));
            }
        }

        // Original405B8C stops the selected channel before reallocation;
        // limit enforcement404322 calls Stop4052F0 before the state pass.
        // The capability boundary must precede new shift/load draws so a
        // retained old source cannot keep selecting into process Main.
        for limited in [false, true] {
            let mut arbiter = SoundArbiter::new(0);
            let mut playback = BoundaryProbe(Vec::new());
            let count = if limited { 1 } else { MAX_CHANNELS };
            let mut old = Vec::new();
            for index in 0..count {
                old.push(
                    arbiter
                        .submit(
                            &request(
                                &format!("OLD{index}"),
                                facts(2, i32::from(limited)),
                                VOLUME_SCALE / 2,
                            ),
                            0,
                        )
                        .unwrap(),
                );
            }
            assert_eq!(
                arbiter
                    .update_tick(100, &mut playback)
                    .iter()
                    .filter(|action| { matches!(action, ArbiterAction::Start { .. }) })
                    .count(),
                count
            );
            playback.0.clear();
            let next = arbiter
                .submit(
                    &request(
                        if limited { "OLD0" } else { "NEW" },
                        facts(if limited { 2 } else { 3 }, i32::from(limited)),
                        VOLUME_SCALE,
                    ),
                    110,
                )
                .unwrap();
            arbiter.update_tick(150, &mut playback);
            let stop = playback.0.iter().position(|call| *call == ("stop", old[0]));
            let admission = playback
                .0
                .iter()
                .position(|call| *call == ("admission", next));
            assert!(
                stop.zip(admission)
                    .is_some_and(|(stop, admission)| stop < admission),
                "old source must stop before new admission (limited={limited}): {:?}",
                playback.0
            );
        }
    }

    #[test]
    fn ordinary_startup_reserves_three_channels_for_streams() {
        // Executed unit_voice_playback startup census: original Audio's
        // Vox752290 and Speech752AD0, then Theme ReadINI720590, reserve
        // three channels through407860 ->4036C0 ->402440. Native group1
        // contains13 channels even while those streams are not playing.
        let mut arbiter = SoundArbiter::new(0);
        for index in 0..16 {
            arbiter
                .submit(
                    &request(&format!("CUE{index}"), facts(2, 0), VOLUME_SCALE),
                    0,
                )
                .expect("pool slot");
        }
        let actions = arbiter.update_tick(1034, &mut TestPlayback::default());
        assert_eq!(started(&actions).len(), 13);
        assert_eq!(arbiter.busy_channel_count(), 13);
    }

    /// One extra equal-priority cue in the same full pass still *takes*
    /// a channel — `StreamBuffer::Allocate @
    /// 0x00405B50` refuses only a strictly lower priority — and the
    /// dispossessed event is caught by the reaping pass' "my channel no
    /// longer names me" test.
    #[test]
    fn an_extra_equal_priority_cue_displaces_one_in_the_full_ordinary_pool() {
        let mut arbiter = SoundArbiter::new(0);
        let mut ids = Vec::new();
        for index in 0..=MAX_CHANNELS {
            let key = format!("CUE{index}");
            ids.push(
                arbiter
                    .submit(&request(&key, facts(2, 0), VOLUME_SCALE), 0)
                    .expect("pool slot"),
            );
        }
        let actions = arbiter.update_tick(100, &mut TestPlayback::default());
        assert_eq!(started(&actions).len(), MAX_CHANNELS);
        assert_eq!(arbiter.busy_channel_count(), MAX_CHANNELS);
        // `FindAvailable` walks the list in order and keeps the first
        // lowest-priority candidate, so the newcomer takes channel 0 and the
        // event that had it is stopped — not the newcomer.
        assert_eq!(stopped(&actions), vec![ids[0]]);
        assert!(started(&actions).contains(&ids[MAX_CHANNELS]));
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
        assert_eq!(
            started(&arbiter.update_tick(100, &mut TestPlayback::default())).len(),
            MAX_CHANNELS
        );

        let loud = arbiter
            .submit(&request("LOUD", facts(4, 0), VOLUME_SCALE), 200)
            .expect("pool slot");
        let actions = arbiter.update_tick(200, &mut TestPlayback::default());
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
        arbiter.update_tick(100, &mut TestPlayback::default());
        let quiet = arbiter
            .submit(&request("QUIET", facts(0, 0), VOLUME_SCALE), 200)
            .expect("pool slot");
        let actions = arbiter.update_tick(200, &mut TestPlayback::default());
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
        let actions = arbiter.update_tick(100, &mut TestPlayback::default());
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
            arbiter.update_tick(100, &mut TestPlayback::default());
            let second = arbiter
                .submit(&request("CUE", cue, VOLUME_SCALE), 200)
                .expect("pool slot");
            let victims = stopped(&arbiter.update_tick(200, &mut TestPlayback::default()));
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
        // Fill and start the pool so every channel is `playing`.
        let mut ids = Vec::new();
        for index in 0..MAX_CHANNELS {
            ids.push(
                arbiter
                    .submit(&request(&format!("C{index}"), facts(2, 0), VOLUME_SCALE), 0)
                    .expect("pool slot"),
            );
        }
        arbiter.update_tick(100, &mut TestPlayback::default());
        // These consecutive stamps are never0x666 apart: the first
        // candidate (channel 0, the oldest) is kept and it is the one the
        // newcomer takes.
        let newcomer = arbiter
            .submit(&request("NEW", facts(2, 0), VOLUME_SCALE), 200)
            .expect("pool slot");
        let actions = arbiter.update_tick(200, &mut TestPlayback::default());
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
        let actions = arbiter.update_tick(100, &mut TestPlayback::default());
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
        arbiter.set_loop_handle(
            HandleOwner::Positional(1234),
            Some(event),
            "ROCKETEERMOVELOOP",
        );

        let actions = arbiter.update_tick(100, &mut TestPlayback::default());
        assert!(matches!(
            actions
                .iter()
                .find(|a| matches!(a, ArbiterAction::Start { .. })),
            Some(ArbiterAction::Start { .. })
        ));
        // Ten more passes with the handle intact: still alive.
        for pass in 1..=10 {
            let actions = arbiter.update_tick(100 + pass * 40, &mut TestPlayback::default());
            assert!(stopped(&actions).is_empty(), "pass {pass} stopped the loop");
        }

        arbiter.set_loop_handle(HandleOwner::Positional(1234), None, "ROCKETEERMOVELOOP");
        let actions = arbiter.update_tick(1000, &mut TestPlayback::default());
        assert_eq!(stopped(&actions), vec![event]);
    }

    /// `SoundEvent::Release @ 0x00406060`: a live owner-driven loop stops
    /// repeating and plays out (`flags |= 0x60`), so the leash spares it once
    /// the owner lets go of its handle; a one-shot and a `Loop=`-counted
    /// entry are left alone.
    #[test]
    fn release_lets_an_owner_loop_play_out_and_spares_other_entries() {
        let mut arbiter = SoundArbiter::new(0);
        let mut loop_facts = facts(1, 0);
        loop_facts.control = control::LOOP;
        let mut counted_facts = loop_facts;
        counted_facts.loop_count = 3;
        let looped = arbiter
            .submit(&request("SPINLOOP", loop_facts, VOLUME_SCALE), 0)
            .expect("pool slot");
        let counted = arbiter
            .submit(&request("COUNTED", counted_facts, VOLUME_SCALE), 0)
            .expect("pool slot");
        let one_shot = arbiter
            .submit(&request("SHOT", facts(1, 0), VOLUME_SCALE), 0)
            .expect("pool slot");
        arbiter.set_loop_handle(HandleOwner::Positional(1), Some(looped), "SPINLOOP");
        arbiter.update_tick(100, &mut TestPlayback::default());
        for id in [looped, counted, one_shot] {
            arbiter.release(id);
        }
        let released = |arbiter: &SoundArbiter, id: EventId| {
            arbiter.event(id).map(|event| {
                event.flags & (event_flags::NO_REPLAY | event_flags::RELEASED)
                    == event_flags::NO_REPLAY | event_flags::RELEASED
            })
        };
        assert_eq!(released(&arbiter, looped), Some(true));
        assert_eq!(released(&arbiter, counted), Some(false));
        assert_eq!(released(&arbiter, one_shot), Some(false));
        arbiter.clear_loop_handle(HandleOwner::Positional(1));
        let actions = arbiter.update_tick(200, &mut TestPlayback::default());
        assert!(
            !stopped(&actions).contains(&looped),
            "a released loop plays out its pass"
        );
    }

    /// `AnimClass::UpdateLoopingSound`'s re-allocation needs the handle to
    /// outlive its event: after the event ends (a stop at zero volume or a
    /// `Limit=` kill) a loopable sound stays named and its owner stays in the
    /// re-drive list, while a one-shot's handle is dropped. `keep_loop_sound`
    /// is an inaudible first play; clearing the handle ends it for good.
    #[test]
    fn a_loopable_handle_outlives_its_event_for_the_re_drive() {
        let mut arbiter = SoundArbiter::new(0);
        let mut loop_facts = facts(1, 0);
        loop_facts.control = control::LOOP;
        let looped = arbiter
            .submit(&request("SPINLOOP", loop_facts, VOLUME_SCALE), 0)
            .expect("pool slot");
        let one_shot = arbiter
            .submit(&request("SHOT", facts(1, 0), VOLUME_SCALE), 0)
            .expect("pool slot");
        arbiter.set_loop_handle(HandleOwner::Positional(1), Some(looped), "SPINLOOP");
        arbiter.set_loop_handle(HandleOwner::Positional(2), Some(one_shot), "SHOT");
        arbiter.keep_loop_sound(HandleOwner::Positional(3), "SPINLOOP", true);
        arbiter.update_tick(100, &mut TestPlayback::default());
        arbiter.stop(looped);
        arbiter.stop(one_shot);
        arbiter.update_tick(200, &mut TestPlayback::default());
        assert_eq!(arbiter.loop_handle_owners(), vec![1, 3]);
        assert_eq!(
            arbiter.kept_loop_key(HandleOwner::Positional(1)),
            Some("SPINLOOP")
        );
        assert_eq!(
            arbiter.kept_loop_key(HandleOwner::Positional(3)),
            Some("SPINLOOP")
        );
        assert_eq!(arbiter.kept_loop_key(HandleOwner::Positional(2)), None);
        arbiter.clear_loop_handle(HandleOwner::Positional(1));
        assert_eq!(arbiter.loop_handle_owners(), vec![3]);
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
        arbiter.update_tick(100, &mut TestPlayback::default());
        let actions = arbiter.update_tick(200, &mut TestPlayback::default());
        assert_eq!(stopped(&actions), vec![event]);
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
        let held = request("GRIZZLYSTART", delayed, VOLUME_SCALE);

        let event = arbiter.submit(&held, 0).expect("pool slot");
        assert!(
            started(&arbiter.update_tick(40, &mut TestPlayback { predelay_ms: 300 })).is_empty()
        );
        assert!(started(&arbiter.update_tick(200, &mut TestPlayback::default())).is_empty());
        assert!(started(&arbiter.update_tick(400, &mut TestPlayback::default())).contains(&event));

        // A draw under the floor never parks the event.
        let mut arbiter = SoundArbiter::new(0);
        let prompt = request("GRIZZLYSTART", delayed, VOLUME_SCALE);

        let event = arbiter.submit(&prompt, 0).expect("pool slot");
        assert!(
            started(&arbiter.update_tick(
                40,
                &mut TestPlayback {
                    predelay_ms: PREDELAY_FLOOR_MS - 1
                }
            ))
            .contains(&event)
        );

        // Without `Control=predelay`/`ambient` the draw is ignored outright:
        // `if ((Voc.Control & 0x88) == 0) return;`.
        let mut arbiter = SoundArbiter::new(0);
        let plain = request("PLAIN", facts(2, 0), VOLUME_SCALE);

        let event = arbiter.submit(&plain, 0).expect("pool slot");
        assert!(
            started(&arbiter.update_tick(40, &mut TestPlayback { predelay_ms: 300 }))
                .contains(&event)
        );
    }

    /// Pause suspends events rather than stopping the service: bookkeeping
    /// keeps running, the pre-delay clock freezes, and nothing starts.
    #[test]
    fn suspension_freezes_the_predelay_and_blocks_the_start_pass() {
        let mut arbiter = SoundArbiter::new(0);
        let mut delayed = facts(2, 0);
        delayed.control = control::PREDELAY;
        delayed.delay_ms = (100, 100);
        let held = request("CUE", delayed, VOLUME_SCALE);

        let event = arbiter.submit(&held, 0).expect("pool slot");
        arbiter.update_tick(10, &mut TestPlayback { predelay_ms: 100 });
        arbiter.suspend_all(50);
        // 10 seconds of paused passes: nothing starts, and the service keeps
        // running throughout.
        for pass in 0..10 {
            assert!(
                started(&arbiter.update_tick(100 + pass * 1000, &mut TestPlayback::default()))
                    .is_empty()
            );
        }
        assert_eq!(arbiter.live_event_count(), 1);
        arbiter.resume_all(10_100);
        assert!(started(&arbiter.update_tick(10_140, &mut TestPlayback::default())).is_empty());
        assert!(
            started(&arbiter.update_tick(10_200, &mut TestPlayback::default())).contains(&event)
        );
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
        arbiter.update_tick(100, &mut TestPlayback::default());

        // Equal priority is never preempted here (`for p in 0..prio-1`).
        assert!(
            arbiter
                .preempt_for_playout(1, &mut TestPlayback::default())
                .is_empty()
        );
        // A priority-3 caller takes the whole `QUIET` entry, both instances.
        let mut victims = arbiter.preempt_for_playout(3, &mut TestPlayback::default());
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
        arbiter.update_tick(100, &mut TestPlayback::default());
        assert_eq!(arbiter.many_sounds_linear(), VOLUME_SCALE);

        arbiter
            .submit(&request("C6", quiet, VOLUME_SCALE), 200)
            .expect("pool slot");
        arbiter.update_tick(200, &mut TestPlayback::default());
        // The scaler glides rather than jumping (`SetTarget`, not
        // `SetTargetImmediate`), so it has only started to move here.
        arbiter.update_tick(4000, &mut TestPlayback::default());
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
        arbiter.update_tick(40, &mut TestPlayback::default());
        // Playing: glide.
        arbiter.set_volume(event, VOLUME_SCALE, 40);
        assert_eq!(arbiter.live_gain(event).map(|(v, _)| v), Some(0));
        arbiter.update_tick(540, &mut TestPlayback::default());
        assert_eq!(
            arbiter.live_gain(event).map(|(v, _)| v),
            Some(VOLUME_SCALE / 2 - 1)
        );
    }

    /// The pump gate is `> 33 ms`, not `>=`.
    #[test]
    fn the_pump_gate_is_strictly_more_than_thirty_three_milliseconds() {
        let mut clock = AudioServiceClock::default();
        assert!(!clock.admit(0));
        assert!(!clock.admit(PUMP_PERIOD_MS));
        assert!(clock.admit(PUMP_PERIOD_MS + 1));
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
        arbiter.set_loop_handle(HandleOwner::Positional(7), Some(first), "CUE");
        assert_eq!(
            arbiter.validate_loop_handle(HandleOwner::Positional(7)),
            Some(first)
        );
        arbiter.stop(first);
        arbiter.update_tick(100, &mut TestPlayback::default());
        assert_eq!(
            arbiter.validate_loop_handle(HandleOwner::Positional(7)),
            None
        );
    }

    /// Original device worker40974F -> endpoint405A00 clears PLAYING and
    /// sets state4, but the handle remains valid until service405974. The
    /// unit_voice_playback oracle reaches an intervening full Logic pass:
    /// a different pending acknowledgement must still wait during that pass.
    #[test]
    fn natural_completion_retains_voice_handle_until_sound_service() {
        let mut arbiter = SoundArbiter::new(1000);
        let owner = HandleOwner::UnitVoice(1);
        let event = arbiter
            .submit(&request("GIMOVE", facts(2, 5), 13926), 1000)
            .unwrap();
        arbiter.set_loop_handle(owner, Some(event), "GIMOVE");
        arbiter.update_tick(1034, &mut TestPlayback::default());
        let serial = arbiter.event(event).unwrap().serial;
        let channel = arbiter.event(event).unwrap().channel;
        let token = arbiter.event_token(event).unwrap();
        assert!(!arbiter.notify_playout_ended(token, PlayoutEnd::default(), 1100));
        assert_eq!(arbiter.validate_loop_handle(owner), Some(event));
        let ended = arbiter.event(event).unwrap();
        assert_eq!(ended.state, EventState::Finished);
        assert_eq!(ended.serial, serial);
        assert_eq!(ended.channel, channel);
        assert_eq!(ended.flags & event_flags::PLAYING, 0);
        assert_eq!(ended.flags & event_flags::DEAD, 0);
        arbiter.update_tick(2635, &mut TestPlayback::default());
        assert_eq!(arbiter.validate_loop_handle(owner), None);
        assert_eq!(arbiter.live_event_count(), 0);
        assert_eq!(arbiter.busy_channel_count(), 0);
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
        arbiter.update_tick(100, &mut TestPlayback::default());

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
        assert_eq!(
            arbiter.preempt_for_playout(3, &mut TestPlayback::default()),
            vec![quiet]
        );
        // Asked again, the same caller finds nothing: the loud entry is out
        // of its reach. Clamping the column to 9 would have put it in row 2
        // and surrendered it here.
        assert!(
            arbiter
                .preempt_for_playout(3, &mut TestPlayback::default())
                .is_empty()
        );
        // It takes a caller one tier higher to reach it.
        assert_eq!(
            arbiter.preempt_for_playout(4, &mut TestPlayback::default()),
            vec![loud]
        );
    }

    /// `SoundEvent::OnChannelDone @ 0x00405A00` on the rows executed by
    /// tools/spatial_oracle/ambient_playout (`channel_done`).
    #[test]
    fn channel_done_matches_the_original_rows() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/ambient_playout.json",
        ))
        .unwrap();
        let now = native["now"].as_u64().unwrap();
        for row in native["channel_done"].as_array().unwrap() {
            let input = &row["input"];
            let mut arbiter = SoundArbiter::new(0);
            let event = arbiter
                .submit(&request("CUE", facts(2, 0), VOLUME_SCALE), 0)
                .unwrap();
            arbiter.update_tick(40, &mut TestPlayback::default());
            let paused = input["paused"] == 1;
            if paused {
                arbiter.suspend_all(now);
            }
            if input["state"] == 4 {
                arbiter.event_mut(event).unwrap().state = EventState::Finished;
            }
            let token = arbiter.event_token(event).unwrap();
            let end = PlayoutEnd {
                gap_ms: input["gap"].as_u64().unwrap() as u32,
                next: input["next"] == 1,
                queue_count: input["queue"].as_i64().unwrap() as i32,
            };
            let continues = arbiter.notify_playout_ended(token, end, now);
            let snapshot = arbiter.native_event_snapshot(event).unwrap();
            assert_eq!(snapshot["state"], row["state"], "{input}");
            assert_eq!(
                snapshot["flags"].as_u64().unwrap() & u64::from(event_flags::PLAYING),
                row["flags"].as_u64().unwrap() & 2,
                "{input}"
            );
            assert_eq!(continues, row["state"] == 1 || row["state"] == 2, "{input}");
            if row["state"] == 2 {
                let event = arbiter.event(event).unwrap();
                assert_eq!(row["resume"], 1, "{input}");
                assert_eq!(event.resume_state, EventState::Ready, "{input}");
                let time = if paused {
                    event.predelay_remaining_ms
                } else {
                    event.predelay_deadline_ms
                };
                assert_eq!(time, row["qwtime"].as_u64().unwrap(), "{input}");
            }
        }
    }

    /// A gap waits in state 2 and the next start pass plays again; a release
    /// consumed there ends the event (`0x0040589E`, stop callback
    /// `0x004059F0`) instead of a later start pass refusing NO_REPLAY.
    #[test]
    fn a_gap_restarts_the_event_and_a_release_in_it_ends_the_event() {
        let mut arbiter = SoundArbiter::new(0);
        let owner = HandleOwner::Positional(9);
        let event = arbiter
            .submit(&request("AMB", facts(2, 0), VOLUME_SCALE), 0)
            .unwrap();
        arbiter.set_loop_handle(owner, Some(event), "AMB");
        assert!(started(&arbiter.update_tick(40, &mut TestPlayback::default())).contains(&event));
        let token = arbiter.event_token(event).unwrap();
        let gap = PlayoutEnd {
            gap_ms: 5000,
            next: false,
            queue_count: 2,
        };
        assert!(arbiter.notify_playout_ended(token, gap, 100));
        assert!(started(&arbiter.update_tick(5100, &mut TestPlayback::default())).is_empty());
        assert!(started(&arbiter.update_tick(5140, &mut TestPlayback::default())).contains(&event));

        assert!(arbiter.notify_playout_ended(token, gap, 6000));
        arbiter.detach_owner(owner);
        let actions = arbiter.update_tick(6040, &mut TestPlayback::default());
        assert!(started(&actions).is_empty() && stopped(&actions).is_empty());
        assert_eq!(arbiter.event(event).unwrap().state, EventState::Finished);
        let actions = arbiter.update_tick(12_000, &mut TestPlayback::default());
        assert_eq!(stopped(&actions), [event]);
        assert!(started(&actions).is_empty());
    }

    /// `UpdateState` state 0 (`0x0040563C..0x0040564B`) kills an event whose
    /// entry has no samples before it takes a channel or draws its shifts.
    #[test]
    fn an_entry_without_samples_dies_before_taking_a_channel() {
        struct Admissions(u32);
        impl PlaybackService for Admissions {
            fn channel_acquired(&mut self, _: EventId) -> i32 {
                self.0 += 1;
                0
            }
            fn load_samples(&mut self, _: EventId, _: bool) -> bool {
                true
            }
            fn prepare_playout(&mut self, _: EventId, _: bool) -> bool {
                true
            }
            fn start_playout(&mut self, _: EventId, _: i32) -> bool {
                true
            }
            fn stop_playout(&mut self, _: EventId) {}
        }
        let mut arbiter = SoundArbiter::new(0);
        let mut empty = facts(3, 0);
        empty.samples_ok = false;
        let event = arbiter
            .submit(&request("CAMPFIRELOOP", empty, VOLUME_SCALE), 0)
            .unwrap();
        let mut playback = Admissions(0);
        let actions = arbiter.update_tick(40, &mut playback);
        assert_eq!(stopped(&actions), [event]);
        assert!(started(&actions).is_empty());
        assert_eq!(playback.0, 0);
        assert_eq!(arbiter.busy_channel_count(), 0);
    }
}
