//! Deterministic PRNG used by simulation systems.
//!
//! Mirrors gamemd.exe `Random::RandomRanged`/scenario RNG shape for
//! gameplay-visible randomness: one auditable stream, 250-word XOR-lag state,
//! inclusive sorted ranged draws, and rejection sampling.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::util::sync::Mutex;

use crate::rng_continuation::MapGenRngContinuation;
use crate::util::native_random;

/// One raw `Random__Next()` draw taken through native's `CDQ/XOR/SUB` absolute
/// value and then `CDQ/IDIV`.
///
/// Typed selectors469D7A..469D81/702320..702339 and constructor axes
/// 74957B..749621/42259B..4225F4 take the absolute value BEFORE division.
/// INT_MIN retains its negative two's-complement value. This intentionally
/// differs from [`SimRng::next_raw_abs_modulo`]'s remainder-first contract.
///
/// Native divisor0 and INT_MIN/-1 divide faults remain outside the supported
/// arithmetic domain. Preserve existing compatibility recovery (zero for a
/// zero divisor, wrapping remainder for overflow) rather than emulating traps.
pub(crate) fn abs_before_modulo(draw: u32, divisor: i32) -> i32 {
    if divisor == 0 {
        return 0;
    }
    (draw as i32).wrapping_abs().wrapping_rem(divisor)
}

const RNG_TABLE_LEN: usize = native_random::STATE_WORDS;

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static DRAW_TRACE: std::cell::RefCell<Option<Vec<serde_json::Value>>> = const { std::cell::RefCell::new(None) };
    static DRAW_LOGIC_OBJECT: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Observation only: identify the live Logic caller even when several objects
/// enter the same mission handler. The guard restores an enclosing callback's
/// identity on normal return, early exit or panic; it never enters sim state.
#[cfg(any(test, feature = "test-support"))]
pub(crate) struct DrawLogicObject(Option<u64>);

#[cfg(any(test, feature = "test-support"))]
impl Drop for DrawLogicObject {
    fn drop(&mut self) {
        DRAW_LOGIC_OBJECT.set(self.0);
    }
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn observe_logic_object(id: u64) -> DrawLogicObject {
    DrawLogicObject(DRAW_LOGIC_OBJECT.replace(Some(id)))
}

/// Test-only observation of real production calls. It neither supplies words
/// nor changes stream cursors; callers keep traces outside serialized state.
#[cfg(any(test, feature = "test-support"))]
pub fn trace_draws<T>(run: impl FnOnce() -> T) -> (T, Vec<serde_json::Value>) {
    DRAW_TRACE.with_borrow_mut(|trace| {
        assert!(trace.is_none(), "nested RNG traces");
        *trace = Some(Vec::new());
    });
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            DRAW_TRACE.with_borrow_mut(|trace| *trace = None);
        }
    }
    let reset = Reset;
    let result = run();
    let trace = DRAW_TRACE.with_borrow_mut(|trace| trace.take().unwrap());
    drop(reset);
    (result, trace)
}

/// Keeps this thread's draws out of a running [`trace_draws`] until it drops
/// ([`untraced_draws`]).
#[cfg(any(test, feature = "test-support"))]
pub(crate) struct UntracedDraws(Option<Vec<serde_json::Value>>);

#[cfg(any(test, feature = "test-support"))]
impl Drop for UntracedDraws {
    fn drop(&mut self) {
        DRAW_TRACE.set(self.0.take());
    }
}

/// Leave this thread's draws out of a running [`trace_draws`] while the guard
/// lives. Work the worker pool shares with the thread that asked for it
/// ([`crate::sim::parallel`]) draws only on private copies of a stream, and
/// a worker's share never reaches the trace: the asking thread's share stays
/// out too, so a trace does not depend on which thread took an item.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn untraced_draws() -> UntracedDraws {
    UntracedDraws(DRAW_TRACE.take())
}

/// A draw's backtrace without this machine's checkout and home prefixes, so a
/// saved trace names `src/...` and `~/...` and compares across checkouts.
#[cfg(any(test, feature = "test-support"))]
fn portable_callers(callers: String) -> String {
    let separator = std::path::MAIN_SEPARATOR;
    let root = crate::test_fixture::checkout_root();
    let mut callers = callers;
    // Std prints a frame under the working directory as `./path`. Cargo runs
    // this crate's tests from `src/`, so put back the `src/` that a run from
    // the checkout root prints.
    if let Ok(cwd) = std::env::current_dir()
        && let (Ok(cwd), Ok(canonical_root)) = (cwd.canonicalize(), root.canonicalize())
        && let Ok(below) = cwd.strip_prefix(&canonical_root)
        && !below.as_os_str().is_empty()
    {
        callers = callers.replace(
            &format!("at .{separator}"),
            &format!("at .{separator}{}{separator}", below.display()),
        );
    }
    let checkout = format!("{}{separator}", root.display());
    let mut callers = callers.replace(&checkout, "");
    for home in ["HOME", "USERPROFILE"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok())
    {
        let home = home.trim_end_matches(['/', '\\']);
        if !home.is_empty() {
            callers = callers.replace(&format!("{home}{separator}"), &format!("~{separator}"));
        }
    }
    callers
}

/// Deterministic simulation RNG.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SimRng {
    disabled: u8,
    index_a: i32,
    index_b: i32,
    /// Held inline, so a copy (a local anim's constructor stream, a scan's
    /// disguise roll) is a memcpy and not a heap allocation. Saved, traced
    /// and hashed as the `Vec<u32>` it once was: a length, then the words.
    #[serde(with = "table")]
    state: [u32; RNG_TABLE_LEN],
    /// The live object pass's turn streams (`sim::world::turn_streams`),
    /// set only while a pass that drew a key runs, so never saved: a save
    /// falls between frames.
    #[serde(skip)]
    turns: TurnStreams,
}

/// The Scenario stream's turn part (VERA-only, sim-scale;
/// `sim::world::turn_streams`): while a Logic slot's turn runs, every raw
/// draw comes from that turn's own generator and the table and its cursors
/// stay as they are. It lives in the value, so a copy made in a turn draws
/// what the turn would draw next and, dropped, leaves the turn where it was,
/// as a dropped copy leaves the table today; written back, it carries its
/// draws with it.
#[derive(Debug, Clone, Copy, Default)]
struct TurnStreams {
    /// The pass's key: the frame in the high half, the pass's one raw
    /// draw from the table in the low half.
    key: Option<u64>,
    /// The running turn's generator.
    turn: Option<TurnStream>,
}

/// One Logic slot's generator: SplitMix64 (Steele, Lea and Flood, "Fast
/// Splittable Pseudorandom Number Generators", OOPSLA 2014, with the
/// constants of Vigna's `splitmix64.c`), started at `key ^ object`. Each
/// draw adds the golden gamma to the state and mixes it; the raw word is the
/// mix's high half. Integer arithmetic only, so every platform draws the
/// same words.
#[derive(Debug, Clone, Copy)]
struct TurnStream {
    /// The stable id of the object whose slot this is.
    object: u64,
    state: u64,
    /// Debug builds: the table and cursors as the turn began, which only
    /// a table draw or a replaced stream could change.
    #[cfg(debug_assertions)]
    table_at_entry: u64,
}

impl TurnStream {
    const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

    #[inline]
    fn draw(&mut self) -> u32 {
        self.state = self.state.wrapping_add(Self::GAMMA);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 32) as u32
    }
}

/// The RNG table's wire form: a sequence, as a `Vec<u32>` writes and reads
/// it, so saves made while the table was a `Vec` load unchanged. A table of
/// another length is refused when it is read.
mod table {
    use super::RNG_TABLE_LEN;
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(
        words: &[u32; RNG_TABLE_LEN],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(words)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<[u32; RNG_TABLE_LEN], D::Error> {
        let words = Vec::<u32>::deserialize(deserializer)?;
        // The save-layout tracer reads one made-up word.
        if crate::util::save_codec::layout::describing() {
            return Ok([0; RNG_TABLE_LEN]);
        }
        words.try_into().map_err(|words: Vec<u32>| {
            serde::de::Error::custom(format!(
                "the random number table holds {} words, not {RNG_TABLE_LEN}",
                words.len()
            ))
        })
    }
}

/// Owner of the app's process Main886B88 cell. It sits behind a lock because
/// two threads draw it: the frontend and presentation on the main thread, and
/// the audio output thread, which draws playlist picks on refill through a
/// retained [`MainRngDraws`]. A scenario's simulation owns its Main as a plain
/// [`SimRng`] drawn through `&mut`, seeded from this cell at install
/// ([`crate::sim::world::Simulation::install_fresh_process_main`]), so the
/// simulation thread and the app share no RNG (docs/sim-thread.md). Main
/// remains outside Scenario saves and the deterministic world hash. A clone
/// shares this one cell.
#[derive(Debug, Clone)]
pub struct MainRng {
    state: Arc<Mutex<SimRng>>,
}

impl MainRng {
    pub fn new(seed: u64) -> Self {
        SimRng::new(seed).into()
    }

    /// Retain draw access without exposing seed, replacement or RNG storage.
    pub fn draws(&self) -> MainRngDraws {
        MainRngDraws {
            state: Arc::clone(&self.state),
        }
    }

    /// Move a finished bootstrap or loaded cursor into the process cell. The
    /// app must synchronously cancel outgoing playback before this seam.
    pub(super) fn install(&self, cursor: SimRng) {
        *self.state.lock().expect("Main RNG poisoned") = cursor;
    }

    pub fn next_u32(&self) -> u32 {
        self.state.lock().expect("Main RNG poisoned").next_u32()
    }

    /// Observe one coherent boundary while callback draws can run elsewhere.
    pub fn logical_state(&self) -> SimRngLogicalState {
        self.state
            .lock()
            .expect("Main RNG poisoned")
            .logical_state()
    }

    /// A detached, draw-free cursor checkpoint. It never grants another live
    /// owner of the process cell.
    pub fn checkpoint(&self) -> SimRng {
        self.state.lock().expect("Main RNG poisoned").clone()
    }

    pub fn state(&self) -> u64 {
        self.state.lock().expect("Main RNG poisoned").state()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn native_state_hex(&self) -> String {
        self.logical_state().native_state_hex()
    }
}

impl From<SimRng> for MainRng {
    fn from(state: SimRng) -> Self {
        Self {
            state: Arc::new(Mutex::new(state)),
        }
    }
}

/// Retained draws on the app's process Main cell, for the main thread and
/// the audio output thread. Clone shares the owner's cell; it cannot seed,
/// replace, borrow simulation state or expose an independent cursor.
#[derive(Clone)]
pub struct MainRngDraws {
    state: Arc<Mutex<SimRng>>,
}

impl MainRngDraws {
    pub fn next_u32(&self) -> u32 {
        self.state.lock().expect("Main RNG poisoned").next_u32()
    }

    /// Keep the entire native rejection-sampling operation under one lock.
    pub fn ranged(&self, low: i32, high: i32) -> i32 {
        self.state
            .lock()
            .expect("Main RNG poisoned")
            .next_range_i32_inclusive(low, high)
    }
}

/// Allocation-free logical view of the native-significant RNG fields.
///
/// Native padding is deliberately absent: Rust has no verified storage or
/// comparison policy for those three bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimRngLogicalView<'a> {
    pub disabled: u8,
    pub index_a: i32,
    pub index_b: i32,
    pub words: &'a [u32],
}

/// Owned logical RNG evidence captured at a named simulation boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimRngLogicalState {
    pub disabled: u8,
    pub index_a: i32,
    pub index_b: i32,
    pub words: [u32; RNG_TABLE_LEN],
}

#[cfg(any(test, feature = "test-support"))]
impl SimRngLogicalState {
    /// Encode coherent boundary evidence in the original Random2Class form.
    pub fn native_state_hex(&self) -> String {
        let mut bytes = Vec::with_capacity(0x3f4);
        bytes.extend_from_slice(&u32::from(self.disabled).to_le_bytes());
        bytes.extend_from_slice(&self.index_a.to_le_bytes());
        bytes.extend_from_slice(&self.index_b.to_le_bytes());
        for word in &self.words {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

impl SimRng {
    /// Reject malformed external cursor checkpoints before any indexed draw.
    /// The table's length was checked when it was read.
    pub fn valid_checkpoint(&self) -> bool {
        (0..RNG_TABLE_LEN as i32).contains(&self.index_a)
            && (0..RNG_TABLE_LEN as i32).contains(&self.index_b)
    }

    /// Create a new RNG with the given seed.
    pub fn new(seed: u64) -> Self {
        Self {
            disabled: 0,
            index_a: 0,
            index_b: native_random::LAG as i32,
            state: native_random::seeded_words(seed as u32),
            turns: TurnStreams::default(),
        }
    }

    /// Adopt the exact retained `g_MapGenRng` cursor without replaying draws.
    ///
    /// The native disabled byte, 250-word state and lag cursors belong to the
    /// same logical object across preview, launch and fresh Scenario boundaries.
    pub fn from_mapgen_continuation(continuation: MapGenRngContinuation) -> Self {
        let (disabled, words, index_a, index_b) = continuation.into_native_parts();
        Self {
            disabled,
            index_a: i32::try_from(index_a).expect("MapGen cursor A fits native i32"),
            index_b: i32::try_from(index_b).expect("MapGen cursor B fits native i32"),
            state: words,
            turns: TurnStreams::default(),
        }
    }

    /// Seal a retained process cursor for a fresh Scenario handoff. This
    /// snapshot grants no draw capability and preserves the native disabled
    /// byte as well as both lag indexes and all 250 words.
    pub(super) fn mapgen_continuation(&self) -> MapGenRngContinuation {
        let state = self.logical_state();
        MapGenRngContinuation::from_native_parts(
            state.disabled,
            state.words,
            usize::try_from(state.index_a).expect("MapGen cursor A is non-negative"),
            usize::try_from(state.index_b).expect("MapGen cursor B is non-negative"),
        )
    }

    /// Compact deterministic fingerprint of the full internal state, with a
    /// running turn's generator (never between frames).
    ///
    /// This is for tests and debug comparisons. Use `hash_state` when feeding
    /// the authoritative world hash so every field contributes directly.
    pub fn state(&self) -> u64 {
        let table = self.table_state();
        match self.turns.turn {
            Some(turn) => crate::util::fnv::fnv1a64_fold_bytes(table, &turn.state.to_le_bytes()),
            None => table,
        }
    }

    /// [`Self::state`] without the turn part: the disabled byte, both
    /// cursors and the table.
    fn table_state(&self) -> u64 {
        let mut hash = crate::util::fnv::FNV1A64_OFFSET_BASIS;
        let mut mix = |value: u64| {
            hash = crate::util::fnv::fnv1a64_fold_bytes(hash, &value.to_le_bytes());
        };
        mix(u64::from(self.disabled));
        mix(self.index_a as u32 as u64);
        mix(self.index_b as u32 as u64);
        for &word in &self.state {
            mix(u64::from(word));
        }
        hash
    }

    /// Whether this stream is `other` but for a running turn's generator:
    /// the disabled byte, both cursors, the table and the pass's key alike.
    /// A turn never changes anything else ([`Self::leave_turn`] checks it in
    /// debug builds), so a copy a turn drew on answers yes against the
    /// stream it was copied from.
    pub fn equal_but_for_turn(&self, other: &SimRng) -> bool {
        self.disabled == other.disabled
            && self.index_a == other.index_a
            && self.index_b == other.index_b
            && self.state == other.state
            && self.turns.key == other.turns.key
    }

    /// The live object pass begins with turn streams on
    /// (`sim::world::turn_streams`): take one raw draw from the table as the
    /// pass's key, with `frame` in its high half so no two passes share one.
    pub(crate) fn begin_turn_streams(&mut self, frame: u32) {
        debug_assert!(self.turns.turn.is_none(), "a pass began inside a turn");
        let word = self.next_u32();
        self.turns.key = Some((u64::from(frame) << 32) | u64::from(word));
    }

    /// The pass is over: every later draw takes the table again.
    pub(crate) fn end_turn_streams(&mut self) {
        self.turns = TurnStreams::default();
    }

    /// `object`'s Logic slot begins: while the pass has a key, its draws
    /// come from its own generator, started at `key ^ object`, until
    /// [`Self::leave_turn`]. Without a key it draws from the table as ever.
    pub(crate) fn enter_turn(&mut self, object: u64) {
        let Some(key) = self.turns.key else {
            return;
        };
        self.turns.turn = Some(TurnStream {
            object,
            state: key ^ object,
            #[cfg(debug_assertions)]
            table_at_entry: self.table_state(),
        });
    }

    /// `object`'s Logic slot is over. Debug builds panic when the turn
    /// changed anything but its generator: a table draw, or a stream
    /// replaced or reseeded inside the turn.
    pub(crate) fn leave_turn(&mut self, object: u64) {
        let turn = self.turns.turn.take();
        #[cfg(debug_assertions)]
        if self.turns.key.is_some() {
            let turn = turn.unwrap_or_else(|| {
                panic!("object {object}'s turn replaced the Scenario stream's turn part")
            });
            assert_eq!(
                turn.object, object,
                "object {object}'s slot ended in object {}'s turn",
                turn.object
            );
            assert_eq!(
                self.table_state(),
                turn.table_at_entry,
                "object {object}'s turn changed the Scenario table or its cursors"
            );
        }
        let _ = (turn, object);
    }

    /// Whether a pass's key is set (`sim::world::turn_streams`).
    pub(crate) fn has_turn_key(&self) -> bool {
        self.turns.key.is_some()
    }

    /// Borrow every logical field without copying or exposing mutation.
    pub fn logical_view(&self) -> SimRngLogicalView<'_> {
        SimRngLogicalView {
            disabled: self.disabled,
            index_a: self.index_a,
            index_b: self.index_b,
            words: &self.state,
        }
    }

    /// The native Random2Class object bytes (`+0x00` disabled dword, the two
    /// indices, then the table), as hex: the form the Unicorn oracles record
    /// Scenario RNG states in.
    #[cfg(any(test, feature = "test-support"))]
    pub fn native_state_hex(&self) -> String {
        self.logical_state().native_state_hex()
    }

    /// Import a pinned original Random2Class comparison boundary. This reads
    /// logical fields only; it does not seed or execute a second RNG port.
    #[cfg(any(test, feature = "test-support"))]
    pub fn from_native_state_hex_for_test(hex: &str) -> Self {
        assert_eq!(hex.len(), 0x3f4 * 2, "native Random object byte count");
        let bytes: Vec<u8> = hex
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        assert_eq!(&bytes[1..4], &[0, 0, 0], "unmodeled native padding");
        let word =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        Self {
            disabled: bytes[0],
            index_a: word(4) as i32,
            index_b: word(8) as i32,
            state: std::array::from_fn(|i| word(12 + i * 4)),
            turns: TurnStreams::default(),
        }
    }

    /// A stream whose next draws are `draws`, in order: the lead cursor starts
    /// at word 0 and the lagged one at word 125, over zeros. For a test that
    /// needs a draw no seed reaches, such as `i32::MIN`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_next_draws_for_test(draws: &[u32]) -> Self {
        assert!(draws.len() <= 125, "the lagged words must stay zero");
        let mut state = [0; RNG_TABLE_LEN];
        state[..draws.len()].copy_from_slice(draws);
        Self {
            disabled: 0,
            index_a: 0,
            index_b: 125,
            state,
            turns: TurnStreams::default(),
        }
    }

    /// Copy every logical field into immutable boundary evidence.
    pub fn logical_state(&self) -> SimRngLogicalState {
        SimRngLogicalState {
            disabled: self.disabled,
            index_a: self.index_a,
            index_b: self.index_b,
            words: self.state,
        }
    }

    /// The first seeded stream whose next `RandomRanged(low, high)` answers
    /// `value`: how oracle replays make the Rust draw the native answer.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn answering(low: i32, high: i32, value: i32) -> Self {
        (0..)
            .map(Self::new)
            .find(|rng| rng.clone().next_range_i32_inclusive(low, high) == value)
            .expect("some seed answers every in-range value")
    }

    /// Test/debug accessor for the secondary lag index. Used by the two-stream
    /// routing tests to assert both streams seed to the gamemd `index_b = 0x67`
    /// start. Not part of the gameplay API.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn index_b(&self) -> i32 {
        self.index_b
    }

    /// Hash the complete RNG state for deterministic replay/desync checks.
    /// An array hashes as its slice, the length and then the words, as the
    /// `Vec` the table once was did.
    pub fn hash_state(&self, hasher: &mut impl Hasher) {
        self.disabled.hash(hasher);
        self.index_a.hash(hasher);
        self.index_b.hash(hasher);
        self.state.hash(hasher);
        // A running turn's generator, which no frame boundary sees.
        if let Some(turn) = &self.turns.turn {
            turn.state.hash(hasher);
        }
    }

    /// Advance and return next random u64.
    #[cfg(test)]
    pub fn next_u64(&mut self) -> u64 {
        let lo = u64::from(self.next_u32());
        let hi = u64::from(self.next_u32());
        lo | (hi << 32)
    }

    /// Next random u32.
    pub fn next_u32(&mut self) -> u32 {
        if self.disabled != 0 {
            return 0;
        }

        // A running turn's draws come from its own generator
        // (`sim::world::turn_streams`); the table stays as it is.
        if let Some(turn) = self.turns.turn.as_mut() {
            let value = turn.draw();
            #[cfg(any(test, feature = "test-support"))]
            DRAW_TRACE.with_borrow_mut(|trace| {
                if let Some(trace) = trace {
                    let callers = std::backtrace::Backtrace::force_capture().to_string();
                    trace.push(serde_json::json!({
                        "stream": "turn", "turn_object": turn.object, "value": value,
                        "logic_object": DRAW_LOGIC_OBJECT.get(),
                        "callers": portable_callers(callers),
                    }));
                }
            });
            return value;
        }

        let a = self.index_a as usize;
        let b = self.index_b as usize;
        let (mut lead, mut lagged) = (a, b);
        let value = native_random::draw(&mut self.state, &mut lead, &mut lagged);
        self.index_a = lead as i32;
        self.index_b = lagged as i32;

        #[cfg(any(test, feature = "test-support"))]
        DRAW_TRACE.with_borrow_mut(|trace| {
            if let Some(trace) = trace {
                let callers = std::backtrace::Backtrace::force_capture().to_string();
                trace.push(serde_json::json!({
                    "stream": "table", "before_indices": [a, b], "value": value,
                    "logic_object": DRAW_LOGIC_OBJECT.get(),
                    "callers": portable_callers(callers),
                }));
            }
        });

        value
    }

    /// Random integer in [0, max_exclusive). Returns 0 for max_exclusive=0.
    pub fn next_range_u32(&mut self, max_exclusive: u32) -> u32 {
        if max_exclusive == 0 {
            return 0;
        }
        self.next_range_u32_inclusive(0, max_exclusive - 1)
    }

    /// Take the high two bits of one raw draw, returning `0..=3`.
    ///
    /// This is the bridge walkers' MapGen `598030(0,3)` operation under the
    /// original startup PC53/chop mode. Unlike the Scenario range helper, it
    /// neither masks the low bits nor rejection-samples. A disabled stream
    /// still receives its one Next call, which returns zero without advancing.
    /// Native boundary values and complete states: `tools/spatial_oracle/mapgen_range`.
    ///
    /// This deliberately exposes only the operation our repair caller needs.
    /// Wider/reversed native intervals and disturbed ambient FPU modes can
    /// round or retry differently; the former generic integer approximation
    /// was not valid for those inputs. RMG has its own range implementation.
    pub(crate) fn next_high_two_bits(&mut self) -> u8 {
        (self.next_u32() >> 30) as u8
    }

    /// Signed form of `Random__RandomRanged @ 0x0065C7E0`. The native compares
    /// its two `int` bounds signed, swaps reversed bounds, and rejection-samples
    /// the masked span exactly like [`Self::next_range_u32_inclusive`]; a rules
    /// value can make `high` negative (`OreTwinkleChance - 1` with chance `<= 0`),
    /// which the unsigned form would misread as a huge span. Spans of
    /// `0x80000000` or more never terminate natively (zero mask); the same
    /// `lo + 0x80000000` guard as the unsigned form stands in for that
    /// unreachable custom-data case.
    pub fn next_range_i32_inclusive(&mut self, low: i32, high: i32) -> i32 {
        let (lo, hi) = if low <= high {
            (low, high)
        } else {
            (high, low)
        };
        if lo == hi {
            return lo;
        }
        let span = hi.wrapping_sub(lo) as u32;
        if span >= 0x7FFF_FFFF {
            return lo.wrapping_add(i32::MIN);
        }
        let mask = u32::MAX >> span.leading_zeros();
        loop {
            let sample = self.next_u32() & mask;
            if sample <= span {
                return lo.wrapping_add(sample as i32);
            }
        }
    }

    /// Random integer in `[low, high]` inclusive on both ends.
    /// Sorts reversed bounds and consumes no draw when the bounds are equal.
    /// Mirrors binary `Random__RandomRanged(low, high)` for ordinary spans.
    pub fn next_range_u32_inclusive(&mut self, low: u32, high: u32) -> u32 {
        let (lo, hi) = if low <= high {
            (low, high)
        } else {
            (high, low)
        };
        if lo == hi {
            return lo;
        }

        let span = hi.wrapping_sub(lo);
        if span >= 0x7FFF_FFFF {
            return lo.wrapping_add(0x8000_0000);
        }

        // Mask one bit wider than the span's highest set bit, matching the
        // rejection-sampling mask 2^(msb+1)-1. next_power_of_two() is wrong
        // because it returns the span itself when span is already a power of
        // two, producing a mask one bit too short (e.g. span=4 -> 3 instead of
        // 7): that biases the output (the inclusive top is never reached) and
        // changes how many raw draws are consumed. span is guaranteed in
        // 1..=0x7FFF_FFFE here (lo==hi early return handles span==0; the
        // span >= 0x7FFF_FFFF guard above handles the top), so leading_zeros is
        // 1..=31 and the shift never reaches 32.
        let mask = u32::MAX >> span.leading_zeros();
        loop {
            let sample = self.next_u32() & mask;
            if sample <= span {
                return lo.wrapping_add(sample);
            }
        }
    }

    /// Raw signed-abs remainder draw: `abs((next_u32() as i32) % n)`, one draw.
    ///
    /// Models gamemd's particle-lifetime / fire-insert-offset primitive: a
    /// single raw draw taken as a signed int, `% n`, then absolute value.
    /// Unlike `next_range_u32` (mask-and-reject), this consumes EXACTLY ONE draw
    /// — no rejection loop — so the shared scenario cursor advances by one per
    /// call. That fixed advance is the parity point: rejection sampling shifts
    /// the cursor a variable amount and desyncs every later consumer that tick.
    /// Returns 0 for `n == 0` (the original would divide-by-zero here; callers
    /// guard with `.max(1)`, so this branch never fires on stock systems).
    /// A raw `i32::MIN` over `n == u32::MAX` (a divisor of -1, as `MaxEC=-1`
    /// gives) overflows gamemd's IDIV; this returns the true remainder, 0.
    ///
    /// Draws that take the absolute value first (`CDQ; XOR; SUB; CDQ; IDIV`),
    /// such as Gas drift and the Spark burst size and Z velocity, use
    /// [`abs_before_modulo`] instead: for a raw `i32::MIN` its remainder stays
    /// negative.
    pub fn next_raw_abs_modulo(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        (self.next_u32() as i32)
            .wrapping_rem(n as i32)
            .unsigned_abs()
    }

    /// Raw signed remainder draw: `(next_u32() as i32) % n`, one draw, may be
    /// negative.
    ///
    /// Models gamemd's particle jitter / spawn-offset primitive (`CDQ; IDIV`,
    /// no absolute value): a negative remainder yields a negative offset, so the
    /// output spans `-(n-1)..=n-1`. Same single-draw cursor advance as
    /// `next_raw_abs_modulo`; the only difference is the sign is preserved.
    /// Returns 0 for `n == 0`, and the true remainder 0 where gamemd's IDIV
    /// overflows (a raw `i32::MIN` over a divisor of -1).
    pub fn next_raw_modulo_signed(&mut self, n: u32) -> i32 {
        if n == 0 {
            return 0;
        }
        (self.next_u32() as i32).wrapping_rem(n as i32)
    }

    /// `Randomizer::RandomDouble` (YRpp `Helpers/Randomizer.h`):
    /// `RandomRanged(1, INT_MAX) / (INT_MAX + 1)`, one inclusive-range draw
    /// into `[1/2^31, 1)`. Phobos probability gates (`ArmorMultiplier.Chance`,
    /// `Critical.Chance`) compare against it.
    pub fn random_double(&mut self) -> f64 {
        f64::from(self.next_range_i32_inclusive(1, i32::MAX)) / 2_147_483_648.0
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::{MainRng, SimRng, trace_draws};

    #[test]
    fn traced_draw_callers_name_no_checkout_or_home_path() {
        let (_, draws) = trace_draws(|| SimRng::new(1).next_u32());
        let callers = draws[0]["callers"].as_str().unwrap();
        assert!(
            callers.contains("next_u32"),
            "the trace keeps the call stack"
        );
        assert!(!callers.contains(&*crate::test_fixture::checkout_root().to_string_lossy()));
        // Saved traces feed tools that read `at ./src/...` frames.
        assert!(
            !cfg!(unix) || callers.contains("at ./src/sim/rng.rs:"),
            "frames name their file from the checkout root: {callers}"
        );
        for home in ["HOME", "USERPROFILE"]
            .into_iter()
            .filter_map(|key| std::env::var(key).ok())
        {
            let home = home.trim_end_matches(['/', '\\']);
            let prefix = format!("{home}{}", std::path::MAIN_SEPARATOR);
            assert!(home.is_empty() || !callers.contains(&prefix));
        }
    }

    #[test]
    fn retained_mapgen_transport_preserves_all_logical_fields() {
        for disabled in [0, 7] {
            let mut resident = SimRng::new(0xC041_52FC);
            for _ in 0..353 {
                let _ = resident.next_u32();
            }
            resident.disabled = disabled;
            let before = resident.logical_state();
            let mut resumed = SimRng::from_mapgen_continuation(resident.mapgen_continuation());

            assert_eq!(resumed.logical_state(), before);
            assert_eq!(resident.logical_state(), before, "snapshot cannot draw");
            assert_eq!(resumed.next_u32(), resident.next_u32());
            assert_eq!(resumed.logical_state(), resident.logical_state());
        }
    }

    #[test]
    fn cloned_main_capabilities_interleave_callback_and_owner_draws() {
        let owner = MainRng::new(1);
        let local = owner.draws();
        let callback = local.clone();
        let (requests, pending) = std::sync::mpsc::sync_channel::<()>(0);
        let (answers, completed) = std::sync::mpsc::sync_channel(0);
        let worker = std::thread::spawn(move || {
            while pending.recv().is_ok() {
                answers
                    .send((callback.next_u32(), callback.ranged(0, 4)))
                    .unwrap();
            }
        });
        let mut reference = SimRng::new(1);
        for _ in 0..32 {
            let frozen = owner.logical_state();
            let before = reference.logical_state();
            assert_eq!(owner.next_u32(), reference.next_u32());
            requests.send(()).unwrap();
            let (raw, ranged) = completed.recv().unwrap();
            assert_eq!(raw, reference.next_u32());
            assert_eq!(ranged, reference.next_range_i32_inclusive(0, 4));
            assert_eq!(local.next_u32(), reference.next_u32());
            assert_eq!(owner.logical_state(), reference.logical_state());
            assert_eq!(
                frozen, before,
                "owned evidence survives later callback draws"
            );
        }
        drop(requests);
        worker.join().unwrap();
    }

    #[test]
    fn high_two_bits_match_original_mapgen_range_and_all_retained_words() {
        fn retained(hex: &str) -> SimRng {
            let bytes: Vec<_> = hex
                .as_bytes()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect();
            assert_eq!(bytes.len(), 0x3f4);
            let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
            SimRng {
                disabled: bytes[0],
                index_a: word(4) as i32,
                index_b: word(8) as i32,
                state: std::array::from_fn(|i| word(12 + i * 4)),
                turns: Default::default(),
            }
        }
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/mapgen_range.json",
        ))
        .unwrap();
        let mut compared = 0;
        for row in corpus["cases"].as_array().unwrap() {
            for call in row["calls"].as_array().unwrap() {
                // The generic native helper's other ranges and disturbed
                // incoming precision/rounding are deliberate negative controls,
                // outside the operation exposed by this Rust API.
                if call["low"] != 0
                    || call["high"] != 3
                    || call["before_control"]["fpcw"].as_u64().unwrap() & 0xf00 != 0xe00
                {
                    continue;
                }
                let mut rng = retained(call["before_state_hex"].as_str().unwrap());
                let after = retained(call["after_state_hex"].as_str().unwrap());
                assert_eq!(
                    u64::from(rng.next_high_two_bits()),
                    call["result"].as_u64().unwrap(),
                    "{}",
                    row["input"]["name"]
                );
                assert_eq!(
                    rng.logical_state(),
                    after.logical_state(),
                    "{}",
                    row["input"]["name"]
                );
                assert_eq!(call["raw_draw_count"], 1);
                assert_eq!(call["state_advance_count"], u64::from(rng.disabled == 0));
                compared += 1;
            }
        }
        assert_eq!(compared, 72, "all startup-mode bridge range requests");
    }

    #[test]
    fn seed_zero_logical_state_matches_native_fixture_edges() {
        let state = SimRng::new(0).logical_state();
        assert_eq!(
            &state.words[..4],
            &[0xAD2E_AA18, 0x6670_51BB, 0xBEB8_385D, 0x1293_A6D6]
        );
        assert_eq!(
            &state.words[246..],
            &[0x62FD_BE0F, 0x2034_06B3, 0xAD5E_A053, 0xD72E_1536]
        );
    }

    #[test]
    fn test_inclusive_range_degenerate() {
        let mut rng = SimRng::new(1);
        let before = rng.state();
        assert_eq!(rng.next_range_u32_inclusive(7, 7), 7);
        assert_eq!(rng.state(), before);

        let before = rng.state();
        let value = rng.next_range_u32_inclusive(7, 3);
        assert!((3..=7).contains(&value));
        assert_ne!(rng.state(), before);
    }

    #[test]
    fn test_exclusive_single_choice_consumes_no_draw() {
        let mut rng = SimRng::new(99);
        let before = rng.state();
        assert_eq!(rng.next_range_u32(1), 0);
        assert_eq!(rng.state(), before);
    }

    #[test]
    fn test_gamemd_raw_sequence_seed_one() {
        let mut rng = SimRng::new(1);
        assert_eq!(rng.next_u32(), 0x78B7_6ED5);
        assert_eq!(rng.next_u32(), 0x275D_74AE);
        assert_eq!(rng.next_u32(), 0xDA63_B931);
    }

    #[test]
    fn next_raw_abs_modulo_seed_one_golden_and_single_draw() {
        // seed=1 raw stream is 0x78B76ED5, 0x275D74AE, 0xDA63B931
        // (test_gamemd_raw_sequence_seed_one). As i32: +2_025_287_381,
        // +660_436_142, -630_998_735. abs(raw % 80) -> 21, 62, 15.
        let mut rng = SimRng::new(1);
        assert_eq!(rng.next_raw_abs_modulo(80), 21);
        assert_eq!(rng.next_raw_abs_modulo(80), 62);
        assert_eq!(rng.next_raw_abs_modulo(80), 15);

        // Exactly one raw draw per call (no rejection loop): one call leaves the
        // stream where a single next_u32() does — distinguishing raw-modulo
        // (always 1 advance) from the mask-and-reject ranged draw (variable).
        let mut a = SimRng::new(1);
        a.next_raw_abs_modulo(80);
        let mut reference = SimRng::new(1);
        reference.next_u32();
        assert_eq!(a.state(), reference.state());
    }

    #[test]
    fn next_raw_modulo_signed_seed_one_golden_keeps_sign() {
        // Same seed=1 stream; signed remainder % 10 -> 1, 2, -5 (sign follows
        // the dividend; the negative third draw must stay negative — abs would
        // wrongly yield +5).
        let mut rng = SimRng::new(1);
        assert_eq!(rng.next_raw_modulo_signed(10), 1);
        assert_eq!(rng.next_raw_modulo_signed(10), 2);
        assert_eq!(rng.next_raw_modulo_signed(10), -5);

        let mut a = SimRng::new(1);
        a.next_raw_modulo_signed(10);
        let mut reference = SimRng::new(1);
        reference.next_u32();
        assert_eq!(a.state(), reference.state());
    }

    /// A raw `i32::MIN` over a divisor of -1 (`MaxEC=-1`) overflows gamemd's
    /// IDIV; both helpers return the true remainder, 0, after their one draw.
    #[test]
    fn raw_modulo_of_min_by_minus_one_is_zero() {
        for signed in [false, true] {
            let mut rng = SimRng::new(1);
            let (a, b) = (rng.index_a as usize, rng.index_b as usize);
            rng.state[a] = rng.state[b] ^ 0x8000_0000;
            let mut twin = rng.clone();
            if signed {
                assert_eq!(rng.next_raw_modulo_signed(u32::MAX), 0);
            } else {
                assert_eq!(rng.next_raw_abs_modulo(u32::MAX), 0);
            }
            assert_eq!(twin.next_u32(), 0x8000_0000);
            assert_eq!(rng.state(), twin.state());
        }
    }

    #[test]
    fn raw_modulo_zero_n_returns_zero_and_consumes_no_draw() {
        // n == 0 guard: both helpers return 0 without advancing the cursor
        // (callers guard with .max(1); the helper must not divide-by-zero).
        let mut rng = SimRng::new(1);
        let before = rng.state();
        assert_eq!(rng.next_raw_abs_modulo(0), 0);
        assert_eq!(rng.next_raw_modulo_signed(0), 0);
        assert_eq!(rng.state(), before);
    }

    #[test]
    fn signed_random_ranged_matches_unsigned_form_and_swaps_negative_bounds() {
        let mut signed = SimRng::new(0x5EED_0001);
        let mut unsigned = SimRng::new(0x5EED_0001);
        for _ in 0..64 {
            assert_eq!(
                signed.next_range_i32_inclusive(0, 29) as u32,
                unsigned.next_range_u32_inclusive(0, 29)
            );
        }
        assert_eq!(signed.state(), unsigned.state());

        // `RandomRanged(0, -1)` swaps to `(-1, 0)`: one masked draw, result in {-1, 0}.
        let mut swapped = SimRng::new(0x5EED_0002);
        let mut reference = SimRng::new(0x5EED_0002);
        let value = swapped.next_range_i32_inclusive(0, -1);
        let expected = -1 + (reference.next_u32() & 1) as i32;
        assert_eq!(value, expected);
        assert_eq!(swapped.state(), reference.state());

        // Equal bounds consume nothing.
        let mut equal = SimRng::new(0x5EED_0003);
        let before = equal.state();
        assert_eq!(equal.next_range_i32_inclusive(0, 0), 0);
        assert_eq!(equal.state(), before);
    }

    #[test]
    fn random_ranged_power_of_two_span_matches_gamemd_draw_stream() {
        // Seed=1 raw draws are 0x78B76ED5, 0x275D74AE, 0xDA63B931
        // (pinned by test_gamemd_raw_sequence_seed_one). With span=4 the
        // rejection mask is 7, so masked values are 5, 6, 1 -> reject, reject,
        // accept(1). Correct behavior: returns 1 AND consumes exactly 3 raw
        // draws.
        let mut rng = SimRng::new(1);
        let v = rng.next_range_u32_inclusive(0, 4);
        assert_eq!(
            v, 1,
            "RandomRanged(0,4) on seed 1 must reject 5,6 then accept 1"
        );

        // Pin the exact raw-draw count: a reference advanced exactly 3 times
        // must match. The old span-1 mask (=3) accepts the first draw
        // (0x78B76ED5 & 3 = 1), returning 1 but consuming only ONE draw, so
        // this state assert fails on the old code and passes on the corrected
        // code.
        let mut reference = SimRng::new(1);
        for _ in 0..3 {
            reference.next_u32();
        }
        assert_eq!(
            rng.state(),
            reference.state(),
            "RandomRanged(0,4) must consume exactly 3 raw draws (rejection sampling)"
        );
    }

    /// Original Random65C780 / RandomRanged65C7E0 histories execute raw and
    /// signed ranged calls across rejection and cursor-wrap boundaries.
    #[test]
    fn ranged_and_raw_calls_match_native_values_draws_and_complete_state() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/rmg_oracle/vectors/rng.json",
        ))
        .unwrap();
        let mut compared = 0;
        for history in native["ranged_cases"].as_array().unwrap() {
            let label = history["id"].as_str().unwrap();
            let mut random = SimRng::new(history["seed"].as_u64().unwrap());
            for _ in 0..history["advance_raw"].as_u64().unwrap() {
                let _ = random.next_u32();
            }
            assert_eq!(
                random.native_state_hex(),
                history["initial_state_hex"].as_str().unwrap(),
                "{label}"
            );
            for (index, step) in history["steps"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    random.native_state_hex(),
                    step["before_state_hex"].as_str().unwrap(),
                    "{label}/{index}"
                );
                let (actual, trace) = super::trace_draws(|| match step["kind"].as_str().unwrap() {
                    "raw" => i64::from(random.next_u32()),
                    "ranged" => i64::from(random.next_range_i32_inclusive(
                        step["low"].as_i64().unwrap() as i32,
                        step["high"].as_i64().unwrap() as i32,
                    )),
                    kind => panic!("unhandled native draw {kind}"),
                });
                assert_eq!(actual, step["result"].as_i64().unwrap(), "{label}/{index}");
                assert_eq!(
                    trace.len() as u64,
                    step["raw_draw_count"].as_u64().unwrap(),
                    "trace {label}/{index}"
                );
                assert_eq!(
                    trace
                        .iter()
                        .map(|draw| draw["value"].clone())
                        .collect::<Vec<_>>(),
                    *step["raw_draws"].as_array().unwrap(),
                    "raw trace {label}/{index}",
                );
                assert_eq!(
                    random.native_state_hex(),
                    step["after_state_hex"].as_str().unwrap(),
                    "{label}/{index}"
                );
                compared += 1;
            }
        }
        assert_eq!(compared, 98);
    }

    /// `RandomDouble` is one `RandomRanged(1, INT_MAX)` draw scaled by
    /// `2^31`, so it advances the shared cursor exactly like that draw.
    #[test]
    fn random_double_consumes_one_inclusive_range_draw() {
        let mut doubled = SimRng::new(0x5EED);
        let mut ranged = SimRng::new(0x5EED);
        let value = doubled.random_double();
        let raw = ranged.next_range_i32_inclusive(1, i32::MAX);
        assert_eq!(value, f64::from(raw) / 2_147_483_648.0);
        assert!((0.0..1.0).contains(&value), "{value}");
        assert_eq!(
            doubled.native_state_hex(),
            ranged.native_state_hex(),
            "same cursor advance"
        );
    }

    /// The table as it was saved while it was a `Vec<u32>`, under the same
    /// serde names, so its bytes, traced layout and hash are the old ones.
    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(rename = "SimRng")]
    struct VecTableRng {
        disabled: u8,
        index_a: i32,
        index_b: i32,
        state: Vec<u32>,
    }

    #[test]
    fn the_inline_table_keeps_the_vec_bytes_layout_and_hash() {
        use crate::util::save_codec;
        use std::hash::Hasher;

        let mut rng = SimRng::new(0x5EED);
        for _ in 0..77 {
            rng.next_u32();
        }
        let old = VecTableRng {
            disabled: rng.disabled,
            index_a: rng.index_a,
            index_b: rng.index_b,
            state: rng.state.to_vec(),
        };
        let old_bytes = save_codec::serialize(&old).unwrap();
        assert_eq!(save_codec::serialize(&rng).unwrap(), old_bytes);
        let loaded: SimRng = save_codec::deserialize(&old_bytes).unwrap();
        assert_eq!(loaded.logical_state(), rng.logical_state());
        assert_eq!(
            save_codec::layout::describe::<SimRng>().unwrap(),
            save_codec::layout::describe::<VecTableRng>().unwrap()
        );

        let mut new_hash = std::collections::hash_map::DefaultHasher::new();
        rng.hash_state(&mut new_hash);
        let mut old_hash = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&old.disabled, &mut old_hash);
        std::hash::Hash::hash(&old.index_a, &mut old_hash);
        std::hash::Hash::hash(&old.index_b, &mut old_hash);
        std::hash::Hash::hash(&old.state, &mut old_hash);
        assert_eq!(new_hash.finish(), old_hash.finish());
    }

    #[test]
    fn a_table_of_another_length_is_refused_when_read() {
        let short = VecTableRng {
            disabled: 0,
            index_a: 0,
            index_b: 0,
            state: vec![1; 249],
        };
        let bytes = crate::util::save_codec::serialize(&short).unwrap();
        let error = crate::util::save_codec::deserialize::<SimRng>(&bytes)
            .unwrap_err()
            .to_string();
        assert!(error.contains("holds 249 words, not 250"), "{error}");
    }

    /// Object 42's and 43's first words in the pass of frame 7 whose table
    /// draw is `0x12345678`: Vigna's reference `splitmix64.c` `next()` from
    /// `0x0000000712345678 ^ id`, each output's high half.
    const TURN_42: [u32; 4] = [0x06bb_4e99, 0x418a_7724, 0x2ebb_1fb7, 0x680b_647a];
    const TURN_43: [u32; 4] = [0xd00a_e455, 0xbba2_2da9, 0xf625_cf44, 0x91a8_58a7];

    /// A turn draws SplitMix64 from the pass's key and its object's id alone:
    /// another table that gives the pass the same word and frame gives the
    /// turn the same words, entering again starts them again, and the table
    /// draws on after the turns as if they had drawn nothing.
    #[test]
    fn a_turn_draws_its_own_stream_from_the_pass_key_and_its_object() {
        let turn = |rng: &mut SimRng, object| {
            rng.enter_turn(object);
            let words: Vec<u32> = (0..4).map(|_| rng.next_u32()).collect();
            rng.leave_turn(object);
            words
        };
        let mut rng = SimRng::with_next_draws_for_test(&[0x1234_5678, 0xAAAA_0001, 0xAAAA_0002]);
        rng.begin_turn_streams(7);
        assert_eq!(turn(&mut rng, 42), TURN_42);
        assert_eq!(turn(&mut rng, 43), TURN_43);
        assert_eq!(turn(&mut rng, 42), TURN_42);
        assert_eq!(rng.next_u32(), 0xAAAA_0001, "a draw outside a turn");
        rng.end_turn_streams();
        assert_eq!(rng.next_u32(), 0xAAAA_0002);

        let mut other = SimRng::with_next_draws_for_test(&[5, 6, 0x1234_5678]);
        let _ = (other.next_u32(), other.next_u32());
        other.begin_turn_streams(7);
        assert_eq!(turn(&mut other, 43), TURN_43);
    }

    /// A copy made in a turn draws what the turn would draw next. Dropped, it
    /// leaves the turn where it was, as a dropped copy leaves the table;
    /// written back, it carries its draws, and differs from the stream it
    /// was copied from only in the turn's generator. Debug builds check as a
    /// turn ends that it changed nothing else: a stream copied before the
    /// turn, drawn on and written back in it, names the object.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "object 42's turn replaced the Scenario stream's turn part")]
    fn a_copy_made_in_a_turn_keeps_its_draws_only_when_written_back() {
        let mut rng = SimRng::with_next_draws_for_test(&[0x1234_5678, 1]);
        rng.begin_turn_streams(7);
        let mut before_the_turn = rng.clone();
        rng.enter_turn(42);
        {
            let mut dropped = rng.clone();
            assert_eq!(dropped.next_u32(), TURN_42[0]);
        }
        let mut kept = rng.clone();
        assert_eq!(kept.next_u32(), TURN_42[0]);
        assert!(kept.equal_but_for_turn(&rng));
        assert_ne!(
            kept.state(),
            rng.state(),
            "the state names the turn's draws"
        );
        rng = kept;
        assert_eq!(rng.next_u32(), TURN_42[1]);
        rng.leave_turn(42);

        rng.enter_turn(42);
        let _ = before_the_turn.next_u32();
        rng = before_the_turn;
        rng.leave_turn(42);
    }
}
