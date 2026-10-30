//! Native recording stream plus deterministic diagnostic-log helpers.
//!
//! Retail recordings are not save games. They contain a fixed startup header,
//! then presentation/sync records plus command batches at the command-transfer
//! rungs selected by the running session. Playback rebuilds the scenario
//! normally from the recorded header and consumes both record kinds at their
//! original scheduler rungs.
//!
//! [`ReplayLog`] is the Rust command/hash recording. Version 3 retains the
//! resolved authored Battle startup for standalone verification through the
//! normal loader and frame owner. It is separate from [`NativeReplay`]. Production
//! does not record or play native streams yet; only tests build
//! [`NativeReplay`], which is kept for native recording playback.

use std::num::NonZeroI32;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(any(test, feature = "test-support"))]
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{COMMAND_RECORD_LEN, CommandEnvelope, CommandRecord};
#[cfg(any(test, feature = "test-support"))]
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::world::TickLane;

/// Value written by the compiled legacy recorder at the start of a stream.
///
/// Active retail YR playback does not reject other values, and its record bit
/// is unreachable; this constant is therefore a tooling default, not a
/// playback version gate.
pub const NATIVE_REPLAY_VERSION: u32 = 10;
/// Exact byte width of the seven-field retail recording header.
pub const NATIVE_REPLAY_HEADER_LEN: usize = 0x1d0;
/// Fixed scenario-name buffer stored in the native header.
pub const NATIVE_REPLAY_SCENARIO_NAME_LEN: usize = 0x104;
/// Raw retail options block stored at the end of the native header.
pub const NATIVE_REPLAY_OPTIONS_LEN: usize = 0xb8;

/// Malformed native recording data.
#[derive(Debug, Error)]
pub enum NativeReplayError {
    #[error("native replay header must be {expected} bytes, got {actual}")]
    HeaderLength { expected: usize, actual: usize },
    #[error("native replay ended at byte {offset} while reading {field}")]
    Truncated { offset: usize, field: &'static str },
    #[error("native replay {field} count does not fit in i32: {count}")]
    CountOverflow { field: &'static str, count: usize },
    #[error("native replay command record is malformed: {0}")]
    Command(#[from] crate::sim::command::CommandRecordError),
    #[error("native replay frame order does not allow {operation}")]
    FrameOrder { operation: &'static str },
}

/// Exact seven-field startup header used by retail recordings.
///
/// Three fields do not yet have a verified semantic name, so they remain
/// literal rather than acquiring an inferred meaning. The raw options block is
/// likewise preserved byte-for-byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeReplayHeader {
    pub version: u32,
    pub seed: u32,
    pub scenario_value: u32,
    scenario_name: [u8; NATIVE_REPLAY_SCENARIO_NAME_LEN],
    pub session_value: u32,
    pub special_flags: u32,
    options: [u8; NATIVE_REPLAY_OPTIONS_LEN],
}

impl Default for NativeReplayHeader {
    /// An explicit zero-initialized destination for best-effort header reads.
    ///
    /// Callers that need native-like preexisting values should supply those
    /// values instead; short reads preserve the unread suffix of that
    /// destination.
    fn default() -> Self {
        Self {
            version: 0,
            seed: 0,
            scenario_value: 0,
            scenario_name: [0; NATIVE_REPLAY_SCENARIO_NAME_LEN],
            session_value: 0,
            special_flags: 0,
            options: [0; NATIVE_REPLAY_OPTIONS_LEN],
        }
    }
}

impl NativeReplayHeader {
    /// Build a recording header for a normally initialized scenario.
    pub fn new(seed: u32, scenario_name: &str) -> Self {
        let mut header = Self {
            version: NATIVE_REPLAY_VERSION,
            seed,
            scenario_value: 0,
            scenario_name: [0; NATIVE_REPLAY_SCENARIO_NAME_LEN],
            session_value: 0,
            special_flags: 0,
            options: [0; NATIVE_REPLAY_OPTIONS_LEN],
        };
        header.set_scenario_name(scenario_name);
        header
    }

    /// Decode the fixed native header without assigning meaning to opaque data.
    pub fn decode(bytes: &[u8]) -> std::result::Result<Self, NativeReplayError> {
        if bytes.len() != NATIVE_REPLAY_HEADER_LEN {
            return Err(NativeReplayError::HeaderLength {
                expected: NATIVE_REPLAY_HEADER_LEN,
                actual: bytes.len(),
            });
        }

        let mut scenario_name = [0; NATIVE_REPLAY_SCENARIO_NAME_LEN];
        scenario_name.copy_from_slice(&bytes[0x0c..0x110]);
        let mut options = [0; NATIVE_REPLAY_OPTIONS_LEN];
        options.copy_from_slice(&bytes[0x118..0x1d0]);

        Ok(Self {
            version: u32::from_le_bytes(bytes[0x00..0x04].try_into().unwrap()),
            seed: u32::from_le_bytes(bytes[0x04..0x08].try_into().unwrap()),
            scenario_value: u32::from_le_bytes(bytes[0x08..0x0c].try_into().unwrap()),
            scenario_name,
            session_value: u32::from_le_bytes(bytes[0x110..0x114].try_into().unwrap()),
            special_flags: u32::from_le_bytes(bytes[0x114..0x118].try_into().unwrap()),
            options,
        })
    }

    /// Encode the seven native writes as one contiguous header.
    pub fn encode(&self) -> [u8; NATIVE_REPLAY_HEADER_LEN] {
        let mut bytes = [0; NATIVE_REPLAY_HEADER_LEN];
        bytes[0x00..0x04].copy_from_slice(&self.version.to_le_bytes());
        bytes[0x04..0x08].copy_from_slice(&self.seed.to_le_bytes());
        bytes[0x08..0x0c].copy_from_slice(&self.scenario_value.to_le_bytes());
        bytes[0x0c..0x110].copy_from_slice(&self.scenario_name);
        bytes[0x110..0x114].copy_from_slice(&self.session_value.to_le_bytes());
        bytes[0x114..0x118].copy_from_slice(&self.special_flags.to_le_bytes());
        bytes[0x118..0x1d0].copy_from_slice(&self.options);
        bytes
    }

    /// Scenario file name up to the first native NUL terminator.
    pub fn scenario_name(&self) -> String {
        let len = self
            .scenario_name
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(self.scenario_name.len());
        String::from_utf8_lossy(&self.scenario_name[..len]).into_owned()
    }

    /// Replace the native C-string buffer, truncating to leave a NUL byte.
    pub fn set_scenario_name(&mut self, name: &str) {
        self.scenario_name.fill(0);
        let len = name.len().min(NATIVE_REPLAY_SCENARIO_NAME_LEN - 1);
        self.scenario_name[..len].copy_from_slice(&name.as_bytes()[..len]);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_options_bytes(&mut self, options: [u8; NATIVE_REPLAY_OPTIONS_LEN]) {
        self.options = options;
    }
}

/// Per-frame replay data read before the active-object pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeReplayPresentation {
    /// Opaque eight-byte scenario sync value written by the recording path.
    pub scenario_hash: [u8; 8],
    /// Additive checksum recorded from the selection vector.
    pub selection_checksum: u32,
    /// Packed selection identities in current-selection order.
    pub selected_objects: Vec<u32>,
    /// Two raw cursor/mouse dwords consumed by the playback render path.
    pub cursor: [u32; 2],
}

impl NativeReplayPresentation {
    pub fn new(scenario_hash: [u8; 8], selected_objects: Vec<u32>, cursor: [u32; 2]) -> Self {
        let selection_checksum = selection_checksum(&selected_objects);
        Self {
            scenario_hash,
            selection_checksum,
            selected_objects,
            cursor,
        }
    }

    /// Capture the two cursor words and clear their live accumulator, matching
    /// the recording path's post-write reset.
    pub fn record(
        scenario_hash: [u8; 8],
        selected_objects: Vec<u32>,
        live_cursor: &mut [u32; 2],
    ) -> Self {
        let cursor = *live_cursor;
        *live_cursor = [0; 2];
        Self::new(scenario_hash, selected_objects, cursor)
    }
}

/// One complete native recording frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeReplayFrame {
    pub presentation: NativeReplayPresentation,
    /// A command-transfer record when that scheduler rung wrote one.
    ///
    /// `Some(empty)` is distinct from `None`: the former writes a zero count;
    /// the latter writes no command bytes before the next presentation record.
    pub commands: Option<Vec<CommandRecord>>,
}

impl NativeReplayFrame {
    /// Construct the frame exactly as the recorder does: only unprocessed
    /// synchronized commands stamped for the current frame are emitted without
    /// sorting.
    pub fn record<'a>(
        presentation: NativeReplayPresentation,
        current_frame: i32,
        write_command_batch: bool,
        do_list: impl IntoIterator<Item = &'a CommandRecord>,
    ) -> Self {
        let commands = write_command_batch.then(|| {
            do_list
                .into_iter()
                .filter(|record| record.frame_stamp() == current_frame && !record.is_processed())
                .cloned()
                .collect()
        });
        Self {
            presentation,
            commands,
        }
    }
}

/// Eager legacy-format document codec used by fixtures.
///
/// Active retail YR cannot enter the corresponding recording path, and its
/// playback path is streaming and tolerant of presentation short reads. Use
/// [`NativeReplayStream`] when reproducing playback behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeReplay {
    pub header: NativeReplayHeader,
    pub frames: Vec<NativeReplayFrame>,
}

impl NativeReplay {
    pub fn encode(&self) -> std::result::Result<Vec<u8>, NativeReplayError> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.header.encode());
        for frame in &self.frames {
            encode_frame(frame, &mut bytes)?;
        }
        Ok(bytes)
    }

    /// Decode using the running session's command-transfer schedule.
    ///
    /// The native stream has no tag for an omitted command batch. Its presence
    /// is known from session mode, frame, and network cadence, so the scheduler
    /// supplies that decision after each presentation record is decoded.
    #[cfg(any(test, feature = "test-support"))]
    pub fn decode_with_command_schedule(
        bytes: &[u8],
        mut has_command_batch: impl FnMut(usize, &NativeReplayPresentation) -> bool,
    ) -> std::result::Result<Self, NativeReplayError> {
        if bytes.len() < NATIVE_REPLAY_HEADER_LEN {
            return Err(NativeReplayError::HeaderLength {
                expected: NATIVE_REPLAY_HEADER_LEN,
                actual: bytes.len(),
            });
        }
        let header = NativeReplayHeader::decode(&bytes[..NATIVE_REPLAY_HEADER_LEN])?;
        let mut cursor = NATIVE_REPLAY_HEADER_LEN;
        let mut frames = Vec::new();
        while cursor < bytes.len() {
            let presentation = decode_presentation(bytes, &mut cursor)?;
            let commands = if has_command_batch(frames.len(), &presentation) {
                Some(decode_command_batch(bytes, &mut cursor)?)
            } else {
                None
            };
            frames.push(NativeReplayFrame {
                presentation,
                commands,
            });
        }
        Ok(Self { header, frames })
    }

    /// Initialize playback through the normal scenario-loading owner.
    ///
    /// No snapshot or serialized active-object vector is accepted here. The
    /// caller must rebuild the scenario from the header, after which frames are
    /// exposed in stream order for the scheduler's presentation and command
    /// rungs.
    #[cfg(any(test, feature = "test-support"))]
    pub fn initialize_playback<S, E>(
        &self,
        initialize_scenario: impl FnOnce(&NativeReplayHeader) -> std::result::Result<S, E>,
    ) -> std::result::Result<NativeReplayPlayback<'_, S>, E> {
        let state = initialize_scenario(&self.header)?;
        Ok(NativeReplayPlayback {
            state,
            frames: self.frames.iter(),
            pending_tail: None,
        })
    }
}

/// A normally reinitialized scenario consuming native frames in file order.
pub struct NativeReplayPlayback<'a, S> {
    pub state: S,
    frames: std::slice::Iter<'a, NativeReplayFrame>,
    pending_tail: Option<&'a NativeReplayFrame>,
}

impl<'a, S> NativeReplayPlayback<'a, S> {
    /// Read the presentation/sync record at the pre-object scheduler rung.
    pub fn begin_frame(&mut self) -> Option<&'a NativeReplayPresentation> {
        assert!(
            self.pending_tail.is_none(),
            "native replay frame tail must be consumed before beginning another frame"
        );
        self.pending_tail = self.frames.next();
        self.pending_tail.map(|frame| &frame.presentation)
    }

    /// Read and admit this frame's command batch at the late command rung.
    pub fn finish_frame(&mut self) -> Option<&'a [CommandRecord]> {
        self.pending_tail
            .take()
            .expect("native replay frame must begin before its tail is consumed")
            .commands
            .as_deref()
    }
}

/// Exact successful updates from the pre-object playback reads of one frame.
///
/// Retail ignores the return value of several reads. A corrupt short read can
/// therefore leave native stack/storage bytes stale. This safe translation
/// consumes the same available bytes but exposes only fully read values; it
/// never manufactures updates from a partial value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeReplayPresentationUpdate {
    pub view: Option<[u8; 8]>,
    pub selection_count: Option<i32>,
    pub selection_checksum: Option<u32>,
    pub selected_objects: Vec<u32>,
    pub cursor: [Option<u32>; 2],
}

/// Session-owned command-transfer cadence for attract playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeReplayCadence {
    pub game_mode: i32,
    pub timing_mode: i32,
    pub frame_send_rate: NonZeroI32,
}

impl NativeReplayCadence {
    /// Whether retail's late command rung reads a batch on this frame.
    ///
    /// VERIFIED gamemd.exe `Process_Command_Queues` caller at `0x00647260`:
    /// frame zero reads only in game mode zero. Positive frames in modes other
    /// than zero/five use the negotiated send-rate divisor only for timing
    /// selector two; negative frames and all remaining paths read every frame.
    pub fn reads_command_batch(self, frame: i32) -> bool {
        if frame == 0 {
            return self.game_mode == 0;
        }
        if frame > 0 && self.game_mode != 0 && self.game_mode != 5 && self.timing_mode == 2 {
            return frame % self.frame_send_rate.get() == 0;
        }
        true
    }
}

/// On-demand active-retail playback cursor over one shared byte stream.
///
/// The header initializes a normal scenario first. Each frame then has a
/// best-effort presentation rung and an optional, fatal-boundary command rung.
pub struct NativeReplayStream<'a, S> {
    pub state: S,
    header: NativeReplayHeader,
    bytes: &'a [u8],
    cursor: usize,
    frame_open: bool,
}

impl<'a, S> NativeReplayStream<'a, S> {
    /// Consume the seven header reads and initialize the scenario normally.
    ///
    /// VERIFIED gamemd.exe attract playback initialization: the seven reads
    /// total `0x1D0` bytes when complete, but neither read counts nor the
    /// version word are checked. Partial reads overwrite only their returned
    /// prefix, so the caller supplies the initialized destination explicitly.
    pub fn initialize<E>(
        bytes: &'a [u8],
        mut header: NativeReplayHeader,
        initialize_scenario: impl FnOnce(&NativeReplayHeader) -> std::result::Result<S, E>,
    ) -> std::result::Result<Self, E> {
        let mut cursor = 0;
        read_stream_u32(bytes, &mut cursor, &mut header.version);
        read_stream_u32(bytes, &mut cursor, &mut header.seed);
        read_stream_u32(bytes, &mut cursor, &mut header.scenario_value);
        read_stream_into(bytes, &mut cursor, &mut header.scenario_name);
        read_stream_u32(bytes, &mut cursor, &mut header.session_value);
        read_stream_u32(bytes, &mut cursor, &mut header.special_flags);
        read_stream_into(bytes, &mut cursor, &mut header.options);

        let state = initialize_scenario(&header)?;
        Ok(Self {
            state,
            header,
            bytes,
            cursor,
            frame_open: false,
        })
    }

    pub fn header(&self) -> &NativeReplayHeader {
        &self.header
    }

    /// Consume the pre-object presentation reads for one frame.
    ///
    /// VERIFIED gamemd.exe main-tick playback rung: view and selection count
    /// update only on exact reads; an exact selection count causes the ignored-
    /// return checksum read and a strictly-positive token loop. Cursor read
    /// returns are ignored. Partial bytes are still consumed from the stream.
    pub fn begin_frame(
        &mut self,
    ) -> std::result::Result<NativeReplayPresentationUpdate, NativeReplayError> {
        if self.frame_open {
            return Err(NativeReplayError::FrameOrder {
                operation: "begin_frame while the previous frame is open",
            });
        }
        self.frame_open = true;

        let view = self.read_best_effort::<8>();
        let selection_count = self.read_best_effort::<4>().map(i32::from_le_bytes);
        let mut selection_checksum = None;
        let mut selected_objects = Vec::new();
        if let Some(count) = selection_count {
            selection_checksum = self.read_best_effort::<4>().map(u32::from_le_bytes);
            if count > 0 {
                // Once EOF is reached, further native reads cannot advance or
                // produce a successful token. Breaking is output/cursor
                // equivalent and avoids a corrupt count becoming an OOM/DoS.
                for _ in 0..count {
                    match self.read_best_effort::<4>() {
                        Some(token) => selected_objects.push(u32::from_le_bytes(token)),
                        None => break,
                    }
                }
            }
        }
        let cursor = [
            self.read_best_effort::<4>().map(u32::from_le_bytes),
            self.read_best_effort::<4>().map(u32::from_le_bytes),
        ];

        Ok(NativeReplayPresentationUpdate {
            view,
            selection_count,
            selection_checksum,
            selected_objects,
            cursor,
        })
    }

    /// Consume the late command rung when the session schedule selects it.
    ///
    /// An omitted rung reads no bytes. A selected rung requires an exact count
    /// and exact `0x6F` bytes per positive-count command; any short read is the
    /// retail game-stop boundary. Admission clears only command flag bit zero.
    pub fn finish_frame(
        &mut self,
        read_batch: bool,
    ) -> std::result::Result<Option<Vec<CommandRecord>>, NativeReplayError> {
        if !self.frame_open {
            return Err(NativeReplayError::FrameOrder {
                operation: "finish_frame before begin_frame",
            });
        }
        self.frame_open = false;
        if !read_batch {
            return Ok(None);
        }

        let command_count = i32::from_le_bytes(self.read_exact::<4>("command count")?);
        let mut commands = Vec::new();
        if command_count > 0 {
            for _ in 0..command_count {
                let record = self.read_exact::<COMMAND_RECORD_LEN>("command record")?;
                commands.push(CommandRecord::admit_exact(&record)?);
            }
        }
        Ok(Some(commands))
    }

    /// Finish through the verified native caller/session cadence.
    #[cfg(any(test, feature = "test-support"))]
    pub fn finish_scheduled_frame(
        &mut self,
        frame: i32,
        cadence: NativeReplayCadence,
    ) -> std::result::Result<Option<Vec<CommandRecord>>, NativeReplayError> {
        self.finish_frame(cadence.reads_command_batch(frame))
    }

    fn read_best_effort<const N: usize>(&mut self) -> Option<[u8; N]> {
        let mut value = [0; N];
        read_stream_into(self.bytes, &mut self.cursor, &mut value).then_some(value)
    }

    fn read_exact<const N: usize>(
        &mut self,
        field: &'static str,
    ) -> std::result::Result<[u8; N], NativeReplayError> {
        let offset = self.cursor;
        self.read_best_effort::<N>()
            .ok_or(NativeReplayError::Truncated { offset, field })
    }
}

/// Native wrapping selection checksum.
pub fn selection_checksum(selected_objects: &[u32]) -> u32 {
    selected_objects
        .iter()
        .fold(0_u32, |sum, packed| sum.wrapping_add(*packed))
}

/// Pack the value written for one selected object.
///
/// A zero kind is the native null sentinel. Other kinds occupy the high byte
/// and the object's heap-pool identity (or the kind-specific raw value) is
/// truncated to 24 bits.
#[cfg(any(test, feature = "test-support"))]
pub fn pack_selection_identity(kind: u8, raw_value: u32) -> u32 {
    if kind == 0 {
        u32::MAX
    } else {
        (u32::from(kind) << 24) | (raw_value & 0x00ff_ffff)
    }
}

fn encode_frame(
    frame: &NativeReplayFrame,
    bytes: &mut Vec<u8>,
) -> std::result::Result<(), NativeReplayError> {
    bytes.extend_from_slice(&frame.presentation.scenario_hash);
    let selection_count =
        i32::try_from(frame.presentation.selected_objects.len()).map_err(|_| {
            NativeReplayError::CountOverflow {
                field: "selection",
                count: frame.presentation.selected_objects.len(),
            }
        })?;
    bytes.extend_from_slice(&selection_count.to_le_bytes());
    bytes.extend_from_slice(&frame.presentation.selection_checksum.to_le_bytes());
    for selected in &frame.presentation.selected_objects {
        bytes.extend_from_slice(&selected.to_le_bytes());
    }
    for cursor_word in frame.presentation.cursor {
        bytes.extend_from_slice(&cursor_word.to_le_bytes());
    }

    if let Some(commands) = &frame.commands {
        let command_count =
            i32::try_from(commands.len()).map_err(|_| NativeReplayError::CountOverflow {
                field: "command",
                count: commands.len(),
            })?;
        bytes.extend_from_slice(&command_count.to_le_bytes());
        for command in commands {
            bytes.extend_from_slice(command.as_bytes());
        }
    }
    Ok(())
}

#[cfg(any(test, feature = "test-support"))]
fn decode_presentation(
    bytes: &[u8],
    cursor: &mut usize,
) -> std::result::Result<NativeReplayPresentation, NativeReplayError> {
    let scenario_hash = take::<8>(bytes, cursor, "scenario hash")?;
    let selection_count = i32::from_le_bytes(take::<4>(bytes, cursor, "selection count")?);
    // Retail enters the selection loop only when 0 < count. Corrupt negative
    // values therefore consume no object records, exactly like zero.
    let selection_count = usize::try_from(selection_count).unwrap_or(0);
    let selection_checksum = u32::from_le_bytes(take::<4>(bytes, cursor, "selection checksum")?);
    let mut selected_objects = Vec::with_capacity(selection_count);
    for _ in 0..selection_count {
        selected_objects.push(u32::from_le_bytes(take::<4>(
            bytes,
            cursor,
            "selected object",
        )?));
    }
    let cursor_words = [
        u32::from_le_bytes(take::<4>(bytes, cursor, "cursor x")?),
        u32::from_le_bytes(take::<4>(bytes, cursor, "cursor y")?),
    ];

    Ok(NativeReplayPresentation {
        scenario_hash,
        selection_checksum,
        selected_objects,
        cursor: cursor_words,
    })
}

#[cfg(any(test, feature = "test-support"))]
fn decode_command_batch(
    bytes: &[u8],
    cursor: &mut usize,
) -> std::result::Result<Vec<CommandRecord>, NativeReplayError> {
    let command_count = i32::from_le_bytes(take::<4>(bytes, cursor, "command count")?);
    // The native command loop has the same strictly-positive gate.
    let command_count = usize::try_from(command_count).unwrap_or(0);
    let mut commands = Vec::with_capacity(command_count);
    for _ in 0..command_count {
        let record = take::<COMMAND_RECORD_LEN>(bytes, cursor, "command record")?;
        commands.push(CommandRecord::admit_exact(&record)?);
    }
    Ok(commands)
}

#[cfg(any(test, feature = "test-support"))]
fn take<const N: usize>(
    bytes: &[u8],
    cursor: &mut usize,
    field: &'static str,
) -> std::result::Result<[u8; N], NativeReplayError> {
    let end = cursor
        .checked_add(N)
        .filter(|end| *end <= bytes.len())
        .ok_or(NativeReplayError::Truncated {
            offset: *cursor,
            field,
        })?;
    let value = bytes[*cursor..end].try_into().unwrap();
    *cursor = end;
    Ok(value)
}

fn read_stream_u32(bytes: &[u8], cursor: &mut usize, destination: &mut u32) -> bool {
    let mut value = destination.to_le_bytes();
    let complete = read_stream_into(bytes, cursor, &mut value);
    *destination = u32::from_le_bytes(value);
    complete
}

/// Reproduce one `fread(destination, 1, destination.len())`-style transfer.
/// The returned prefix is copied even when the requested width is unavailable.
fn read_stream_into(bytes: &[u8], cursor: &mut usize, destination: &mut [u8]) -> bool {
    let available = bytes.len().saturating_sub(*cursor).min(destination.len());
    let end = *cursor + available;
    destination[..available].copy_from_slice(&bytes[*cursor..end]);
    *cursor = end;
    available == destination.len()
}

/// Header for the Rust-only deterministic diagnostic log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayHeader {
    /// Match input; an old JSON log uses the documented default profile.
    #[serde(default)]
    pub pixel_conversion_bounds: crate::util::pixel_conversion::PixelConversionBounds,
    pub version: u32,
    pub tick_hz: u32,
    pub seed: u64,
    pub map_name: String,
    pub rules_hash: u64,
}

/// How often normal play's diagnostic log records the state hash: after every
/// 15th frame, by tick number, and after the frame that ends the match. A
/// fixed frame count, never wall time, so any run of the same commands hashes
/// the same frames. Tests, sim-bench and
/// replay verification can hash every frame instead
/// ([`Simulation::hash_state_every`](crate::sim::world::Simulation::hash_state_every)).
pub const PLAY_HASH_INTERVAL: std::num::NonZeroU64 = std::num::NonZeroU64::new(15).unwrap();

/// App service inputs which change scenario state between ordinary frames.
/// These replay the existing native owners; wall time and media stay outside sim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioServiceInput {
    /// Radar657CA0 acquires an untimed ScenarioPause684060 credit.
    Pause,
    /// Radar657CC8 releases its ScenarioPause684180 credit.
    Resume,
    /// Main_Tick55D815 observes that the system-clock timer has elapsed.
    ExpireTimedPause,
    /// Main_Tick55D83A visits53B560 without an ordinary world frame.
    StepChronoScreen,
}

impl ScenarioServiceInput {
    pub fn apply(self, sim: &mut Simulation) -> Vec<crate::sim::trigger_runtime::TriggerEffect> {
        match self {
            Self::Pause => sim.session.pause_scenario(0),
            Self::Resume => sim.session.resume_scenario(),
            Self::ExpireTimedPause => sim.session.expire_scenario_pause(),
            Self::StepChronoScreen => sim.session.process_chrono_screen_effect(),
        }
    }
}

/// One tick in the Rust-only deterministic diagnostic log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayTick {
    pub tick: u64,
    pub commands: Vec<CommandEnvelope>,
    /// The state hash after this tick, on the ticks that hashed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_hash: Option<u64>,
    /// Ordered inputs after this frame's hash and before the next frame.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scenario_services: Vec<ScenarioServiceInput>,
}

/// Rich Rust-only command/hash log used by tests and diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayLog {
    pub header: ReplayHeader,
    /// Present only for a self-contained, freshly constructed authored match.
    /// Old diagnostic segments remain readable but cannot claim standalone replay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup: Option<Box<ReplayStartup>>,
    pub ticks: Vec<ReplayTick>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub initial_scenario_services: Vec<ScenarioServiceInput>,
}

pub const REPLAY_VERSION: u32 = 3;

/// The resolved launch and draw-free initial cursor projection contain no
/// service session, ticket, network endpoint, authentication key or password.
/// Startup version 2 reconstructs the normal App's batched-path policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayStartup {
    version: u32,
    #[serde(default)]
    native_id_prefix: Option<u32>,
    tick_ms: u32,
    map_hash: u64,
    map_payload_len: usize,
    map_sha256: String,
    launch: crate::skirmish_launch::SkirmishLaunchSession,
    session: crate::sim::scenario_session::ScenarioSession,
    scenario_rng: crate::sim::rng::SimRng,
    main_rng: crate::sim::rng::SimRng,
    mapgen_rng: crate::sim::rng::SimRng,
    initial_hash: u64,
    every_frame_network: bool,
}

impl ReplayStartup {
    pub fn map_sha256(&self) -> &str {
        &self.map_sha256
    }

    pub fn launch_session(&self) -> &crate::skirmish_launch::SkirmishLaunchSession {
        &self.launch
    }

    pub fn seed(&self) -> u32 {
        self.session.seed() as u32
    }

    pub fn game_mode(&self) -> crate::sim::scenario_session::ScenarioGameMode {
        self.session.game_mode()
    }

    pub fn native_id_prefix(&self) -> Result<u32> {
        self.native_id_prefix
            .context("replay lacks its native-ID startup prefix; record a fresh match")
    }

    pub fn capture(
        runtime: &crate::sim::runtime::SimRuntime,
        launch: &crate::skirmish_launch::SkirmishLaunchSession,
        source: &crate::map::source::LoadedMapSource,
        map_hash: u64,
        tick_ms: u32,
        every_frame_network: bool,
    ) -> Result<Self> {
        let sim = &runtime.simulation;
        anyhow::ensure!(
            sim.session().tick() == 0
                && sim.session().binary_frame() == 0
                && sim.session().started(),
            "standalone replay requires a started frame-zero world"
        );
        anyhow::ensure!(
            sim.path_batch_stats().is_some(),
            "standalone replay requires the normal App's batched paths"
        );
        anyhow::ensure!(
            launch.mode.class == crate::skirmish_modes::MpModeClass::Battle,
            "standalone replay currently supports authored Battle launches"
        );
        let map_name = launch
            .selected_map_file
            .as_deref()
            .context("replay launch map")?;
        let map_name = crate::map::source::portable_map_name(map_name);
        crate::map::source::validate_network_map_name(map_name)?;
        anyhow::ensure!(
            map_name == sim.session().map_name(),
            "replay launch map differs from its world"
        );
        anyhow::ensure!(
            matches!(
                source,
                crate::map::source::LoadedMapSource::Loose { .. }
                    | crate::map::source::LoadedMapSource::Mix { .. }
            ),
            "generated/legacy maps lack authored replay startup"
        );
        let (map_payload_len, map_sha256) = source
            .physical_identity()
            .context("replay physical map identity")?;
        anyhow::ensure!(
            crate::map::source::valid_source_sha256(map_sha256),
            "invalid replay map identity"
        );
        anyhow::ensure!(
            tick_ms != 0 && tick_ms <= 1000,
            "invalid replay tick duration"
        );
        crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(launch.clone())
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let mut launch = launch.clone();
        launch.selected_map_file = Some(map_name.to_owned());
        Ok(Self {
            version: 2,
            native_id_prefix: Some(
                sim.native_identity_prefix_checkpoint()
                    .context("replay native-ID prefix")?,
            ),
            tick_ms,
            map_hash,
            map_payload_len,
            map_sha256: map_sha256.to_owned(),
            launch,
            session: sim.session().clone(),
            scenario_rng: sim.scenario_rng().clone(),
            main_rng: sim.main_rng().checkpoint(),
            mapgen_rng: sim.mapgen_rng().clone(),
            initial_hash: sim.state_hash(),
            every_frame_network,
        })
    }

    fn validate_header(&self, header: &ReplayHeader) -> Result<()> {
        anyhow::ensure!(
            header.version == REPLAY_VERSION && self.version == 2,
            "unsupported standalone replay version; startup version 2 with native-ID prefix is required"
        );
        self.native_id_prefix()?;
        anyhow::ensure!(
            self.tick_ms != 0
                && self.tick_ms <= 1000
                && header.tick_hz != 0
                && 1000 / header.tick_hz == self.tick_ms,
            "replay tick duration/header disagree"
        );
        anyhow::ensure!(
            header.seed == self.session.seed() && header.seed <= u64::from(u32::MAX),
            "replay seed/header disagree"
        );
        anyhow::ensure!(
            header.map_name == self.session.map_name()
                && self.launch.selected_map_file.as_deref() == Some(header.map_name.as_str()),
            "replay map/header disagree"
        );
        crate::map::source::validate_network_map_name(&header.map_name)?;
        anyhow::ensure!(
            self.launch.mode.class == crate::skirmish_modes::MpModeClass::Battle,
            "unsupported replay launch mode"
        );
        anyhow::ensure!(
            self.map_payload_len > 0 && crate::map::source::valid_source_sha256(&self.map_sha256),
            "invalid replay map identity"
        );
        anyhow::ensure!(
            self.session.tick() == 0 && self.session.binary_frame() == 0,
            "replay startup is not frame zero"
        );
        anyhow::ensure!(
            header.pixel_conversion_bounds == self.session.pixel_conversion_bounds(),
            "replay pixel bounds/header disagree"
        );
        Ok(())
    }
}

impl ReplayLog {
    /// Validate the immutable startup before a CLI or rendered loader opens
    /// assets. Diagnostic-only segments cannot silently become new matches.
    pub fn standalone_startup(&self) -> Result<&ReplayStartup> {
        let startup = self.startup.as_deref().context(
            "diagnostic log has no standalone startup; record a fresh authored Battle match",
        )?;
        startup.validate_header(&self.header)?;
        anyhow::ensure!(
            self.ticks.iter().any(|tick| tick.state_hash.is_some()),
            "replay contains no recorded state hashes"
        );
        Ok(startup)
    }

    pub fn new(header: ReplayHeader) -> Self {
        Self {
            header,
            startup: None,
            ticks: Vec::new(),
            initial_scenario_services: Vec::new(),
        }
    }

    pub fn record_tick(
        &mut self,
        tick: u64,
        commands: Vec<CommandEnvelope>,
        state_hash: Option<u64>,
    ) {
        self.ticks.push(ReplayTick {
            tick,
            commands,
            state_hash,
            scenario_services: Vec::new(),
        });
    }

    /// Apply and record at one boundary so live service cannot omit its input.
    pub fn apply_scenario_service(
        &mut self,
        sim: &mut Simulation,
        input: ScenarioServiceInput,
    ) -> Vec<crate::sim::trigger_runtime::TriggerEffect> {
        let effects = input.apply(sim);
        // A timed pause also polls an inactive Chrono effect. It changes nothing.
        if input != ScenarioServiceInput::StepChronoScreen || !effects.is_empty() {
            match self.ticks.last_mut() {
                Some(frame) => frame.scenario_services.push(input),
                None => self.initial_scenario_services.push(input),
            }
        }
        effects
    }

    pub fn write_json(&self, writer: &mut impl std::io::Write) -> Result<()> {
        use std::io::Write;
        // Keep every record and hash while bounding the serialization buffer;
        // a scale match's fixed byte arrays need no per-element indentation.
        let mut buffered = std::io::BufWriter::new(writer);
        serde_json::to_writer(&mut buffered, self).context("Failed to write diagnostic log")?;
        // BufWriter's Drop discards errors. The segment owner must observe a
        // final write/flush failure so it retains the log and removes the file.
        buffered.flush().context("Failed to flush diagnostic log")
    }

    pub fn load(path: &Path) -> Result<Self> {
        Self::load_bound(path).map(|(replay, _)| replay)
    }

    /// Decode and bind the exact immutable bytes used by a rendered capture.
    pub fn load_bound(path: &Path) -> Result<(Self, String)> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("Failed to read diagnostic log: {}", path.display()))?;
        let replay = serde_json::from_slice(&bytes)
            .context("Failed to parse deterministic diagnostic JSON")?;
        Ok((replay, crate::util::sha256::sha256_hex(&bytes)))
    }
}

/// Headless runner for the Rust-only deterministic diagnostic log.
pub struct ReplayRunner;

#[derive(Debug, Serialize)]
pub struct ReplayVerification {
    pub ticks: usize,
    pub checked_hashes: usize,
    pub final_tick: u64,
    pub final_state_hash: u64,
}

/// Only the queue-exchange decision differs from native diagnostic playback:
/// canonical records already contain their accepted frame and source order.
/// The existing checksum owner and simulation dispatcher remain shared.
struct ReplayNetworkBoundary {
    checksum: std::cell::RefCell<crate::sim::command_queue::FrameChecksumState>,
    every_frame_network: bool,
}

impl crate::sim::world::NetworkCommandBoundary for ReplayNetworkBoundary {
    fn exchange_commands(
        &self,
        context: crate::sim::world::NetworkQueueContext,
        _queues: &mut crate::sim::world::NetworkCommandQueues<'_>,
    ) -> std::result::Result<
        Option<crate::sim::world::NetworkQueuePass>,
        crate::sim::world::NetworkCommandError,
    > {
        Ok(self.every_frame_network.then_some(if context.frame == 0 {
            crate::sim::world::NetworkQueuePass::SkipDispatch
        } else {
            crate::sim::world::NetworkQueuePass::Dispatch
        }))
    }

    fn record_frame_checksum(&self, frame: u32, checksum: u32) {
        self.checksum
            .borrow_mut()
            .record_frame_checksum(frame, checksum);
    }

    fn compare_frame_info(
        &self,
        record: &CommandRecord,
        frame: i32,
    ) -> std::result::Result<bool, crate::sim::world::NetworkCommandError> {
        crate::sim::world::NetworkCommandBoundary::compare_frame_info(&self.checksum, record, frame)
    }
}

/// One playback cursor shared by headless verification and the normal client.
/// Recorded admissions enter the sole SimRuntime transaction; rendering uses
/// that transaction's output, without another Logic or command queue.
pub struct ReplayPlayback {
    replay: std::sync::Arc<ReplayLog>,
    index: usize,
    tick_ms: u32,
    network: ReplayNetworkBoundary,
    failure: Option<String>,
}

impl ReplayPlayback {
    /// Bind an authored world after map/rules checks, restoring only the
    /// recorded startup projection. App and headless loaders share this gate.
    pub fn prepare(
        runtime: &mut crate::sim::runtime::SimRuntime,
        replay: std::sync::Arc<ReplayLog>,
        source: &crate::map::source::LoadedMapSource,
        map_hash: u64,
    ) -> Result<Self> {
        let startup = replay.standalone_startup()?;
        anyhow::ensure!(
            source.physical_identity()
                == Some((startup.map_payload_len, startup.map_sha256.as_str())),
            "replay map bytes differ"
        );
        anyhow::ensure!(map_hash == startup.map_hash, "replay parsed map differs");
        anyhow::ensure!(
            runtime.resources.rules.simulation_config_hash() == replay.header.rules_hash,
            "replay rules/art differ"
        );
        runtime
            .simulation
            .restore_replay_startup(
                startup.session.clone(),
                startup.scenario_rng.clone(),
                startup.main_rng.clone(),
                startup.mapgen_rng.clone(),
            )
            .map_err(anyhow::Error::msg)?;
        let tick_ms = startup.tick_ms;
        Self::from_runtime(runtime, replay, tick_ms)
    }

    fn from_runtime(
        runtime: &mut crate::sim::runtime::SimRuntime,
        replay: std::sync::Arc<ReplayLog>,
        tick_ms: u32,
    ) -> Result<Self> {
        #[cfg(feature = "debug-tools")]
        runtime
            .simulation
            .set_debug_session_context(crate::sim::cheats::DebugSessionContext::Replay);
        anyhow::ensure!(
            runtime.simulation.session().pixel_conversion_bounds()
                == replay.header.pixel_conversion_bounds,
            "replay pixel conversion bounds differ from the initialized match"
        );
        anyhow::ensure!(
            runtime.simulation.session().seed() == replay.header.seed,
            "replay playback sim must be constructed from header.seed"
        );
        let current = runtime.resources.rules.simulation_config_hash();
        anyhow::ensure!(
            replay.header.rules_hash == 0 || replay.header.rules_hash == current,
            "replay rules hash mismatch: header {:#018x}, current {current:#018x}",
            replay.header.rules_hash
        );
        anyhow::ensure!(
            (1..=REPLAY_VERSION).contains(&replay.header.version),
            "unsupported replay version"
        );
        if replay.header.version == REPLAY_VERSION {
            anyhow::ensure!(
                runtime.simulation.session().map_name() == replay.header.map_name,
                "replay map name mismatch"
            );
        }
        if let Some(startup) = replay.startup.as_deref() {
            startup.validate_header(&replay.header)?;
            anyhow::ensure!(
                tick_ms == startup.tick_ms
                    && runtime.simulation.state_hash() == startup.initial_hash,
                "reconstructed replay startup hash differs: recorded {:#018x}, reconstructed {:#018x}",
                startup.initial_hash,
                runtime.simulation.state_hash()
            );
        }
        runtime
            .simulation
            .hash_state_every(std::num::NonZeroU64::MIN);
        for input in &replay.initial_scenario_services {
            input.apply(&mut runtime.simulation);
        }
        let every_frame_network = replay
            .startup
            .as_ref()
            .is_some_and(|startup| startup.every_frame_network);
        Ok(Self {
            replay,
            index: 0,
            tick_ms,
            failure: None,
            network: ReplayNetworkBoundary {
                checksum: std::cell::RefCell::new(
                    crate::sim::command_queue::FrameChecksumState::new(),
                ),
                every_frame_network,
            },
        })
    }

    pub fn is_finished(&self) -> bool {
        self.index == self.replay.ticks.len()
    }
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }
    pub fn can_advance(&self) -> bool {
        !self.is_finished() && self.failure.is_none()
    }
    pub fn recorded_final_hash(&self) -> Option<u64> {
        self.replay.ticks.last().and_then(|tick| tick.state_hash)
    }

    /// Advance one recorded frame, retaining the ordinary client's sound,
    /// radar and presentation effects. Only network exchange is substituted.
    pub fn advance(
        &mut self,
        runtime: &mut crate::sim::runtime::SimRuntime,
        effects: crate::sim::world::FrameEffects<'_>,
    ) -> Result<Option<crate::sim::world::SimFrameOutput>> {
        if !self.can_advance() {
            return Ok(None);
        }
        let result = self.advance_inner(runtime, effects);
        if let Err(error) = &result {
            self.failure = Some(format!("{error:#}"));
        }
        result.map(Some)
    }

    fn advance_inner(
        &mut self,
        runtime: &mut crate::sim::runtime::SimRuntime,
        effects: crate::sim::world::FrameEffects<'_>,
    ) -> Result<crate::sim::world::SimFrameOutput> {
        let entry = &self.replay.ticks[self.index];
        let commands = runtime
            .simulation
            .take_due_replay_commands(entry.commands.iter().cloned());
        let mut output = runtime.advance_frame(
            &commands,
            self.tick_ms,
            crate::sim::world::TickLane::Ordinary,
            effects.with_network_commands(&self.network),
        )?;
        if output.network_failure.is_some() {
            anyhow::ensure!(
                self.index + 1 == self.replay.ticks.len(),
                "diagnostic log continues after a fatal FRAMEINFO mismatch"
            );
            output = runtime
                .resume_after_network_failure(effects.with_network_commands(&self.network))?;
        }
        anyhow::ensure!(
            !output.network_waiting && !output.outcome_waiting,
            "replay frame suspended without a recorded completion"
        );
        if self.replay.startup.is_some() {
            // Standalone recordings label the completed frame. Legacy
            // diagnostic segments may instead label its pre-frame input;
            // their accepted stream and hashes still determine playback.
            anyhow::ensure!(
                output.tick.tick == entry.tick,
                "replay tick order differs at record {}",
                self.index
            );
        }
        if let Some(expected) = entry.state_hash {
            anyhow::ensure!(
                output.tick.state_hash == Some(expected),
                "replay state hash mismatch at tick {}: recorded {expected:#018x}, replayed {:?}",
                entry.tick,
                output.tick.state_hash
            );
        }
        for input in &entry.scenario_services {
            input.apply(&mut runtime.simulation);
        }
        self.index += 1;
        Ok(output)
    }

    pub fn verification(
        &self,
        runtime: &crate::sim::runtime::SimRuntime,
    ) -> Result<ReplayVerification> {
        anyhow::ensure!(
            self.is_finished() && self.failure.is_none(),
            "replay did not finish successfully"
        );
        let checked_hashes = self
            .replay
            .ticks
            .iter()
            .filter(|tick| tick.state_hash.is_some())
            .count();
        anyhow::ensure!(
            checked_hashes > 0,
            "replay contains no recorded state hashes"
        );
        Ok(ReplayVerification {
            ticks: self.index,
            checked_hashes,
            final_tick: runtime.simulation.session().tick(),
            final_state_hash: runtime.simulation.state_hash(),
        })
    }
}

impl ReplayRunner {
    /// Reconstruct with the same authored loader and playback cursor as the client.
    pub fn verify(
        retail_dir: &Path,
        replay: &ReplayLog,
        map_beside: Option<&str>,
    ) -> Result<ReplayVerification> {
        let startup = replay.standalone_startup()?;
        let map_file = map_beside.unwrap_or(&replay.header.map_name);
        let mut session = startup.launch.clone();
        session.selected_map_file = Some(map_file.to_owned());
        let launch = crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(session)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let mut loaded = crate::headless_scenario::load_with_launch_and_ini(
            retail_dir,
            map_file,
            startup.seed(),
            launch,
            &Default::default(),
            startup.game_mode(),
            Some(startup.native_id_prefix()?),
            None,
        )
        .map_err(anyhow::Error::msg)?;
        // The ordinary App handoff enables the path-search owner before
        // recording frame zero. It changes movement timing and is hashed.
        loaded.runtime.simulation.batch_path_searches();
        let mut playback = ReplayPlayback::prepare(
            &mut loaded.runtime,
            std::sync::Arc::new(replay.clone()),
            &loaded.source,
            loaded.map.ini.content_hash(),
        )?;
        while playback.can_advance() {
            playback.advance(
                &mut loaded.runtime,
                crate::sim::world::FrameEffects::empty(),
            )?;
        }
        playback.verification(&loaded.runtime)
    }

    /// Diagnostic fixtures retain their preconstructed world, but consume
    /// commands through exactly the production per-frame playback cursor.
    pub fn run_runtime(
        runtime: &mut crate::sim::runtime::SimRuntime,
        replay: &ReplayLog,
        tick_ms: u32,
    ) -> Result<Vec<Option<u64>>> {
        let mut playback =
            ReplayPlayback::from_runtime(runtime, std::sync::Arc::new(replay.clone()), tick_ms)?;
        let mut hashes = Vec::with_capacity(replay.ticks.len());
        while playback.can_advance() {
            if let Some(output) =
                playback.advance(runtime, crate::sim::world::FrameEffects::empty())?
            {
                hashes.push(output.tick.state_hash);
            }
        }
        Ok(hashes)
    }

    /// Fixture-only replay runner (F09): re-run diagnostic-log ticks with
    /// explicitly supplied resources. Production replay execution goes
    /// through `run_runtime`, whose resources are bound in the `SimRuntime`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn run_fixture(
        sim: &mut Simulation,
        replay: &ReplayLog,
        rules: Option<&RuleSet>,
        path_grid: Option<&PathGrid>,
        tick_ms: u32,
    ) -> Vec<Option<u64>> {
        Self::run_fixture_with_overlay_registry(sim, replay, rules, path_grid, None, tick_ms)
    }

    /// Fixture-only variant with the static overlay type registry used by the
    /// recorded map. Overlay-backed simulation authority (including miner
    /// resource queries) must see the same registry on record and replay.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn run_fixture_with_overlay_registry(
        sim: &mut Simulation,
        replay: &ReplayLog,
        rules: Option<&RuleSet>,
        path_grid: Option<&PathGrid>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        tick_ms: u32,
    ) -> Vec<Option<u64>> {
        Self::run_fixture_master_frame(
            sim,
            replay,
            rules,
            path_grid,
            overlay_registry,
            tick_ms,
            false,
        )
    }

    /// Fixture-only replay through the same master-frame admission gameplay
    /// uses; `poll_triggers` polls the simulation's installed triggers.
    /// Diagnostic logs own commands and hashes. Production replay execution
    /// is `run_runtime`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn run_fixture_master_frame(
        sim: &mut Simulation,
        replay: &ReplayLog,
        rules: Option<&RuleSet>,
        path_grid: Option<&PathGrid>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        tick_ms: u32,
        poll_triggers: bool,
    ) -> Vec<Option<u64>> {
        #[cfg(feature = "debug-tools")]
        sim.set_debug_session_context(crate::sim::cheats::DebugSessionContext::Replay);
        // The diagnostic playback must be constructed from the recorded seed.
        assert_eq!(
            sim.session.pixel_conversion_bounds(),
            replay.header.pixel_conversion_bounds,
            "replay pixel conversion bounds differ from the initialized match"
        );
        // A sim seeded
        // differently than the header it replays is a guaranteed silent
        // divergence.
        debug_assert_eq!(
            sim.session.seed(),
            replay.header.seed,
            "replay playback sim must be constructed from header.seed"
        );
        // A replay recorded under different processed rules or resolved ART
        // timing desyncs silently. Surface it. `rules_hash == 0` is
        // the "not stamped" sentinel (test/headless paths), so skip those.
        if let Some(rules) = rules {
            let current = rules.simulation_config_hash();
            if replay.header.rules_hash != 0 && replay.header.rules_hash != current {
                log::warn!(
                    "replay rules_hash mismatch (header {:#018x} vs current \
                     {:#018x}) — recorded under a different rules set; \
                     playback will desync",
                    replay.header.rules_hash,
                    current
                );
            }
        }
        sim.install_fixture_path_grid(path_grid);
        // Playback returns every frame's hash, so a test world hashes each one;
        // a recording compared with it must ask for the same.
        sim.hash_state_every(std::num::NonZeroU64::MIN);
        let network = std::cell::RefCell::new(crate::sim::command_queue::FrameChecksumState::new());
        let mut hashes: Vec<Option<u64>> = Vec::with_capacity(replay.ticks.len());
        for input in &replay.initial_scenario_services {
            input.apply(sim);
        }
        for (index, entry) in replay.ticks.iter().enumerate() {
            let due_commands = sim.take_due_replay_commands(entry.commands.iter().cloned());
            let mut result = sim
                .advance_master_frame(
                    &due_commands,
                    rules.unwrap_or(crate::rules::ruleset::RuleSet::empty_for_test()),
                    overlay_registry,
                    tick_ms,
                    TickLane::Ordinary,
                    poll_triggers,
                    crate::sim::world::FrameEffects::default().with_network_commands(&network),
                )
                .expect("fixture frame must complete")
                .into_tick();
            if sim.pending_network_failure().is_some() {
                assert_eq!(
                    index + 1,
                    replay.ticks.len(),
                    "diagnostic log continues after a fatal FRAMEINFO mismatch"
                );
                result = sim
                    .resume_after_network_failure(
                        rules.unwrap_or(crate::rules::ruleset::RuleSet::empty_for_test()),
                        overlay_registry,
                        crate::sim::world::FrameEffects::default().with_network_commands(&network),
                    )
                    .expect("recorded checksum modal tail must complete")
                    .tick;
            }
            // Replay has no app layer to consume presentation-only trigger effects.
            let _ = sim.drain_trigger_effects();
            hashes.push(result.state_hash);
            for input in &entry.scenario_services {
                input.apply(sim);
            }
        }
        hashes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "slow; authored retail Battle load and standalone replay reconstruction"]
    fn authored_ai_match_reconstructs_and_checks_actual_command_hashes() {
        use crate::sim::world::{FrameEffects, TickLane};
        let root = std::path::PathBuf::from(std::env::var_os("RA2_DIR").expect("retail RA2_DIR"));
        let mut launch = crate::headless_scenario::battle_session(
            "XMP03T4.MAP",
            &[(
                crate::skirmish_launch::LaunchCountry::Russia,
                crate::skirmish_launch::AiDifficulty::Normal,
            )],
        );
        launch.humans[0].name = "ReplayGuest".to_owned();
        let assets = crate::assets::asset_manager::AssetManager::new(
            &root,
            crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
        )
        .unwrap();
        let audio = crate::rules::audio_sources::AudioDefinitions::select(&assets);
        let (_, _, mut process_rules) = crate::rules::retail_sources::load_startup_rules(
            &assets,
            std::sync::Arc::clone(audio.sounds()),
            std::sync::Arc::clone(audio.eva()),
        )
        .unwrap()
        .into_parts();
        let cold = crate::headless_scenario::load_with_launch_and_ini(
            &root,
            "XMP03T4.MAP",
            0x5eed_015,
            crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(launch.clone())
                .unwrap(),
            &Default::default(),
            crate::sim::scenario_session::ScenarioGameMode::Wol,
            None,
            Some(&mut process_rules),
        )
        .unwrap();
        let cold_prefix = cold.sim().native_identity_prefix_checkpoint().unwrap();
        drop(cold);
        let mut scene = crate::headless_scenario::load_with_launch_and_ini(
            &root,
            "XMP03T4.MAP",
            0x5eed_015,
            crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(launch.clone())
                .unwrap(),
            &Default::default(),
            crate::sim::scenario_session::ScenarioGameMode::Wol,
            None,
            Some(&mut process_rules),
        )
        .unwrap();
        assert_ne!(
            scene.sim().native_identity_prefix_checkpoint(),
            Some(cold_prefix),
            "real first-load/rematch Rules history must exercise different native-ID prefixes"
        );
        let allocated_cursor = scene.sim().native_identity_cursor();
        assert!(
            scene
                .runtime
                .simulation
                .install_accepted_native_id_prefix(cold_prefix)
                .is_err(),
            "a loaded world cannot remap its existing native identities"
        );
        assert_eq!(scene.sim().native_identity_cursor(), allocated_cursor);
        assert!(
            ReplayStartup::capture(
                &scene.runtime,
                &launch,
                &scene.source,
                scene.map.ini.content_hash(),
                22,
                true,
            )
            .is_err(),
            "serial headless paths do not satisfy the normal App replay contract"
        );
        let sim = &mut scene.runtime.simulation;
        // Model the ordinary App handoff, including its authoritative path
        // policy; the headless reconstruction must install the same owner.
        sim.batch_path_searches();
        assert_eq!(
            sim.houses()[&sim.session().current_house().unwrap()].native_scenario_name(),
            Some("ReplayGuest")
        );
        sim.session.install_network_timing(40);
        sim.session
            .install_network_command_timing(0, std::num::NonZeroU8::new(1).unwrap(), [0]);
        sim.session.configure_sequenced_clock(45);
        sim.hash_state_every(std::num::NonZeroU64::MIN);
        let actor = sim
            .entities()
            .values_sorted()
            .find(|entity| {
                entity.owner() == sim.session().current_house().unwrap()
                    && entity.locomotor.is_some()
            })
            .unwrap()
            .stable_id();
        let startup = ReplayStartup::capture(
            &scene.runtime,
            &launch,
            &scene.source,
            scene.map.ini.content_hash(),
            22,
            true,
        )
        .unwrap();
        let mut log = ReplayLog::new(ReplayHeader {
            version: REPLAY_VERSION,
            tick_hz: 45,
            seed: 0x5eed_015,
            map_name: "XMP03T4.MAP".into(),
            rules_hash: scene.runtime.resources.rules.simulation_config_hash(),
            pixel_conversion_bounds: scene.sim().session().pixel_conversion_bounds(),
        });
        log.startup = Some(Box::new(startup));
        let mut wrong_header = log.header.clone();
        wrong_header.version += 1;
        assert!(
            log.startup
                .as_ref()
                .unwrap()
                .validate_header(&wrong_header)
                .is_err()
        );
        wrong_header = log.header.clone();
        wrong_header.seed += 1;
        assert!(
            log.startup
                .as_ref()
                .unwrap()
                .validate_header(&wrong_header)
                .is_err()
        );
        let network = ReplayNetworkBoundary {
            every_frame_network: true,
            checksum: Default::default(),
        };
        for frame in 0..=32 {
            let mut commands = Vec::new();
            if frame == 10 {
                let mut record = CommandRecord::encode(0, 0, frame, &[]).unwrap();
                crate::sim::command::StopRecord {
                    house_id: 0,
                    frame,
                    entity_id: i32::try_from(actor).unwrap(),
                }
                .write_into(&mut record);
                commands.push(CommandEnvelope::synchronized(
                    record,
                    scene.sim().session().tick(),
                ));
            }
            if frame == 32 {
                commands.push(CommandEnvelope::synchronized(
                    crate::sim::command::ExitRecord {
                        house_id: 0,
                        frame: frame as u32,
                    }
                    .encode()
                    .unwrap(),
                    scene.sim().session().tick(),
                ));
            }
            let mut output = scene
                .runtime
                .advance_frame(
                    &commands,
                    22,
                    TickLane::Ordinary,
                    FrameEffects::empty().with_network_commands(&network),
                )
                .unwrap();
            let admitted = output.take_admitted_commands();
            log.record_tick(output.tick.tick, admitted, output.tick.state_hash);
        }
        assert_eq!(log.ticks[31].tick, 32);
        assert_eq!(
            log.ticks[32].tick, 32,
            "EXIT skips the terminal frame commit"
        );
        assert!(scene.sim().quit_requested());
        assert!(log.ticks[32].commands.iter().any(|command| matches!(
            &command.payload,
            crate::sim::command::Command::SynchronizedRecord { record }
                if record.opcode() == crate::sim::command::EXIT_OPCODE
        )));
        assert!(scene.sim().session().started());
        let decoded: ReplayLog =
            serde_json::from_slice(&serde_json::to_vec(&log).unwrap()).unwrap();
        // Started is process lifecycle, deliberately absent from serialized
        // ScenarioSession. Reconstruction must retain the loader publication.
        assert!(!decoded.startup.as_ref().unwrap().session.started());
        let mut incomplete = decoded.clone();
        incomplete.startup.as_mut().unwrap().native_id_prefix = None;
        assert!(
            incomplete
                .standalone_startup()
                .unwrap_err()
                .to_string()
                .contains("native-ID startup prefix")
        );
        let verified = ReplayRunner::verify(&root, &decoded, None).unwrap();
        assert_eq!(verified.checked_hashes, 33);
        assert_eq!(verified.final_tick, 32);
        assert_eq!(verified.final_state_hash, scene.sim().state_hash());
        // A -SPAWN replay's map exists only beside the replay file.
        let beside = std::env::temp_dir().join(format!("vera20k-replay-{}", std::process::id()));
        std::fs::create_dir_all(&beside).unwrap();
        let map = beside.join("XMP03T4.MAP");
        std::fs::write(&map, assets.get("XMP03T4.MAP").unwrap()).unwrap();
        let verified = ReplayRunner::verify(&root, &decoded, map.to_str());
        std::fs::remove_dir_all(&beside).unwrap();
        assert_eq!(
            verified.unwrap().final_state_hash,
            scene.sim().state_hash()
        );
        let mut corrupted = decoded;
        corrupted.ticks[10].state_hash = Some(1);
        assert!(
            ReplayRunner::verify(&root, &corrupted, None)
                .unwrap_err()
                .to_string()
                .contains("state hash mismatch")
        );
    }

    #[test]
    fn standalone_replay_rejects_missing_startup_before_opening_assets() {
        let replay = ReplayLog::new(ReplayHeader {
            version: 2,
            tick_hz: 45,
            seed: 7,
            map_name: "XMP03T4.MAP".into(),
            rules_hash: 0,
            pixel_conversion_bounds: Default::default(),
        });
        assert!(
            ReplayRunner::verify(Path::new("absent-retail"), &replay, None)
                .unwrap_err()
                .to_string()
                .contains("no standalone startup")
        );
    }
    use crate::sim::command::Command;

    #[test]
    fn native_header_uses_exact_seven_field_layout() {
        let mut header = NativeReplayHeader::new(0x1234_5678, "maps\\test.map");
        header.scenario_value = 0xaabb_ccdd;
        header.session_value = 0x0102_0304;
        header.special_flags = 0x5566_7788;
        header.set_options_bytes([0x5a; NATIVE_REPLAY_OPTIONS_LEN]);

        let bytes = header.encode();
        assert_eq!(bytes.len(), 0x1d0);
        assert_eq!(&bytes[0x00..0x04], &10_u32.to_le_bytes());
        assert_eq!(&bytes[0x04..0x08], &0x1234_5678_u32.to_le_bytes());
        assert_eq!(&bytes[0x08..0x0c], &0xaabb_ccdd_u32.to_le_bytes());
        assert_eq!(&bytes[0x0c..0x19], b"maps\\test.map");
        assert_eq!(bytes[0x19], 0);
        assert!(bytes[0x1a..0x110].iter().all(|byte| *byte == 0));
        assert_eq!(&bytes[0x110..0x114], &0x0102_0304_u32.to_le_bytes());
        assert_eq!(&bytes[0x114..0x118], &0x5566_7788_u32.to_le_bytes());
        assert!(bytes[0x118..0x1d0].iter().all(|byte| *byte == 0x5a));

        assert_eq!(NativeReplayHeader::decode(&bytes).unwrap(), header);
        assert_eq!(header.scenario_name(), "maps\\test.map");
    }

    #[test]
    fn native_frame_layout_and_unknown_command_bytes_round_trip() {
        let mut unknown = [0x7b; COMMAND_RECORD_LEN];
        unknown[0] = 0xfe;
        unknown[1] = 0xa4;
        unknown[2] = 3;
        unknown[3..7].copy_from_slice(&17_i32.to_le_bytes());
        let command = CommandRecord::decode_exact(&unknown).unwrap();

        let presentation = NativeReplayPresentation::new(
            [1, 2, 3, 4, 5, 6, 7, 8],
            vec![0x3400_0001, 0x3400_0002],
            [0x1122_3344, 0x5566_7788],
        );
        let frame = NativeReplayFrame::record(presentation, 17, true, [&command]);
        let replay = NativeReplay {
            header: NativeReplayHeader::new(9, "x.map"),
            frames: vec![frame],
        };

        let bytes = replay.encode().unwrap();
        assert_eq!(&bytes[0x1d0..0x1d8], &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(&bytes[0x1d8..0x1dc], &2_i32.to_le_bytes());
        assert_eq!(&bytes[0x1dc..0x1e0], &0x6800_0003_u32.to_le_bytes());
        assert_eq!(&bytes[0x1e0..0x1e4], &0x3400_0001_u32.to_le_bytes());
        assert_eq!(&bytes[0x1e4..0x1e8], &0x3400_0002_u32.to_le_bytes());
        assert_eq!(&bytes[0x1e8..0x1ec], &0x1122_3344_u32.to_le_bytes());
        assert_eq!(&bytes[0x1ec..0x1f0], &0x5566_7788_u32.to_le_bytes());
        assert_eq!(&bytes[0x1f0..0x1f4], &1_i32.to_le_bytes());
        assert_eq!(&bytes[0x1f4..0x263], &unknown);
        assert_eq!(
            NativeReplay::decode_with_command_schedule(&bytes, |_, _| true).unwrap(),
            replay
        );
    }

    #[test]
    fn native_recording_filters_due_unprocessed_commands_in_queue_order() {
        let due_a = CommandRecord::encode(4, 0, 7, &[1]).unwrap();
        let future = CommandRecord::encode(5, 0, 8, &[2]).unwrap();
        let mut processed = CommandRecord::encode(6, 0, 7, &[3]).unwrap();
        processed.mark_processed();
        let due_b = CommandRecord::encode(7, 0, 7, &[4]).unwrap();

        let frame = NativeReplayFrame::record(
            NativeReplayPresentation::new([0; 8], Vec::new(), [0; 2]),
            7,
            true,
            [&due_a, &future, &processed, &due_b],
        );

        assert_eq!(frame.commands, Some(vec![due_a, due_b]));
    }

    #[test]
    fn native_selection_checksum_wraps_and_playback_starts_from_header() {
        assert_eq!(selection_checksum(&[u32::MAX, 2]), 1);
        assert_eq!(pack_selection_identity(0, 123), u32::MAX);
        assert_eq!(pack_selection_identity(0x34, 0xff12_3456), 0x3412_3456);

        let mut live_cursor = [44, 55];
        let captured = NativeReplayPresentation::record([0; 8], vec![1], &mut live_cursor);
        assert_eq!(captured.cursor, [44, 55]);
        assert_eq!(live_cursor, [0, 0]);

        let replay = NativeReplay {
            header: NativeReplayHeader::new(0xdead_beef, "arena.map"),
            frames: vec![
                NativeReplayFrame::record(
                    NativeReplayPresentation::new([0; 8], vec![1], [0; 2]),
                    0,
                    true,
                    std::iter::empty::<&CommandRecord>(),
                ),
                NativeReplayFrame::record(
                    NativeReplayPresentation::new([0; 8], vec![2], [0; 2]),
                    1,
                    true,
                    std::iter::empty::<&CommandRecord>(),
                ),
            ],
        };
        let mut playback = replay
            .initialize_playback(|header| {
                Ok::<_, ()>((header.seed, header.scenario_name(), Vec::<u32>::new()))
            })
            .unwrap();
        assert_eq!(playback.state.0, 0xdead_beef);
        assert_eq!(playback.state.1, "arena.map");
        while let Some(presentation) = playback.begin_frame() {
            playback.state.2.push(presentation.selected_objects[0]);
            assert!(playback.finish_frame().unwrap().is_empty());
        }
        assert_eq!(playback.state.2, vec![1, 2]);
    }

    #[test]
    fn native_decode_rejects_a_truncated_command_record() {
        let replay = NativeReplay {
            header: NativeReplayHeader::new(1, "x.map"),
            frames: vec![NativeReplayFrame::record(
                NativeReplayPresentation::new([0; 8], Vec::new(), [0; 2]),
                0,
                true,
                [&CommandRecord::encode(1, 0, 0, &[]).unwrap()],
            )],
        };
        let mut bytes = replay.encode().unwrap();
        bytes.pop();
        assert!(matches!(
            NativeReplay::decode_with_command_schedule(&bytes, |_, _| true),
            Err(NativeReplayError::Truncated {
                field: "command record",
                ..
            })
        ));
    }

    #[test]
    fn native_decode_treats_negative_selection_and_command_counts_as_empty() {
        let replay = NativeReplay {
            header: NativeReplayHeader::new(1, "x.map"),
            frames: vec![NativeReplayFrame::record(
                NativeReplayPresentation::new([0; 8], Vec::new(), [0; 2]),
                0,
                true,
                std::iter::empty::<&CommandRecord>(),
            )],
        };
        let mut bytes = replay.encode().unwrap();
        let selection_count_offset = NATIVE_REPLAY_HEADER_LEN + 8;
        bytes[selection_count_offset..selection_count_offset + 4]
            .copy_from_slice(&(-1_i32).to_le_bytes());
        let command_count_offset = NATIVE_REPLAY_HEADER_LEN + 8 + 4 + 4 + 8;
        bytes[command_count_offset..command_count_offset + 4]
            .copy_from_slice(&(-2_i32).to_le_bytes());

        assert_eq!(
            NativeReplay::decode_with_command_schedule(&bytes, |_, _| true).unwrap(),
            replay
        );
    }

    #[test]
    fn omitted_command_rung_writes_no_count_and_decodes_from_session_schedule() {
        let replay = NativeReplay {
            header: NativeReplayHeader::new(1, "x.map"),
            frames: vec![
                NativeReplayFrame::record(
                    NativeReplayPresentation::new([0x11; 8], Vec::new(), [0; 2]),
                    0,
                    false,
                    std::iter::empty::<&CommandRecord>(),
                ),
                NativeReplayFrame::record(
                    NativeReplayPresentation::new([0x22; 8], Vec::new(), [0; 2]),
                    1,
                    true,
                    std::iter::empty::<&CommandRecord>(),
                ),
            ],
        };

        let bytes = replay.encode().unwrap();
        let first_presentation_len = 8 + 4 + 4 + 8;
        assert_eq!(
            &bytes[0x1d0 + first_presentation_len..][..8],
            &[0x22; 8],
            "an omitted command rung has no zero-count placeholder"
        );
        assert_eq!(
            NativeReplay::decode_with_command_schedule(&bytes, |frame, _| frame != 0).unwrap(),
            replay
        );
    }

    #[test]
    fn gsi_17_07_stream_header_accepts_nonstandard_and_partial_values_before_normal_init() {
        let mut header = NativeReplayHeader::new(0x1234_5678, "arena.map");
        header.version = 77;
        let bytes = header.encode();
        let playback =
            NativeReplayStream::initialize(&bytes, NativeReplayHeader::default(), |loaded| {
                Ok::<_, ()>((loaded.version, loaded.seed, loaded.scenario_name()))
            })
            .unwrap();
        assert_eq!(playback.state, (77, 0x1234_5678, "arena.map".to_owned()));

        let initialized = NativeReplayHeader {
            version: 0x1122_3344,
            seed: 0xaabb_ccdd,
            ..Default::default()
        };
        let partial = [0xaa, 0xbb];
        let playback = NativeReplayStream::initialize(&partial, initialized, |loaded| {
            Ok::<_, ()>((loaded.version, loaded.seed))
        })
        .unwrap();
        assert_eq!(playback.state, (0x1122_bbaa, 0xaabb_ccdd));
        assert_eq!(playback.header().version, 0x1122_bbaa);
    }

    #[test]
    fn gsi_17_07_presentation_short_reads_are_safe_until_the_fatal_command_rung() {
        let header = NativeReplayHeader::new(1, "x.map").encode();
        let mut exact_view = Vec::from([0x11; 8]);
        let mut partial_count = exact_view.clone();
        partial_count.extend_from_slice(&[1, 2]);

        let mut partial_checksum = exact_view.clone();
        partial_checksum.extend_from_slice(&1_i32.to_le_bytes());
        partial_checksum.extend_from_slice(&[3, 4]);

        let mut partial_token = exact_view.clone();
        partial_token.extend_from_slice(&1_i32.to_le_bytes());
        partial_token.extend_from_slice(&0x1234_u32.to_le_bytes());
        partial_token.extend_from_slice(&[5, 6]);

        let mut partial_cursor = exact_view.split_off(0);
        partial_cursor.extend_from_slice(&0_i32.to_le_bytes());
        partial_cursor.extend_from_slice(&0x5678_u32.to_le_bytes());
        partial_cursor.extend_from_slice(&[7, 8]);

        for (tail, expected_view, expected_count, expected_checksum) in [
            (Vec::new(), None, None, None),
            (vec![1, 2, 3], None, None, None),
            (partial_count, Some([0x11; 8]), None, None),
            (partial_checksum, Some([0x11; 8]), Some(1), None),
            (partial_token, Some([0x11; 8]), Some(1), Some(0x1234)),
            (partial_cursor, Some([0x11; 8]), Some(0), Some(0x5678)),
        ] {
            let mut bytes = header.to_vec();
            bytes.extend_from_slice(&tail);
            let mut playback =
                NativeReplayStream::initialize(&bytes, NativeReplayHeader::default(), |_| {
                    Ok::<_, ()>(())
                })
                .unwrap();
            let update = playback.begin_frame().unwrap();
            assert_eq!(update.view, expected_view);
            assert_eq!(update.selection_count, expected_count);
            assert_eq!(update.selection_checksum, expected_checksum);
            assert!(update.selected_objects.is_empty());
            assert_eq!(update.cursor, [None, None]);
            assert!(matches!(
                playback.finish_frame(true),
                Err(NativeReplayError::Truncated {
                    field: "command count",
                    ..
                })
            ));
        }
    }

    #[test]
    fn gsi_17_07_omitted_batch_preserves_the_shared_cursor_for_the_next_frame() {
        let mut command_bytes = [0x7b; COMMAND_RECORD_LEN];
        command_bytes[0] = 0xfe;
        command_bytes[1] = 0xa4;
        command_bytes[2] = 3;
        command_bytes[3..7].copy_from_slice(&2_i32.to_le_bytes());
        let command = CommandRecord::decode_exact(&command_bytes).unwrap();
        let replay = NativeReplay {
            header: NativeReplayHeader::new(9, "x.map"),
            frames: vec![
                NativeReplayFrame::record(
                    NativeReplayPresentation::new(
                        [0x11; 8],
                        vec![0x3400_0001, 0x3400_0002],
                        [3, 4],
                    ),
                    1,
                    false,
                    std::iter::empty::<&CommandRecord>(),
                ),
                NativeReplayFrame::record(
                    NativeReplayPresentation::new([0x22; 8], Vec::new(), [5, 6]),
                    2,
                    true,
                    [&command],
                ),
            ],
        };
        let mut bytes = replay.encode().unwrap();
        let admitted_flags_offset = bytes.len() - COMMAND_RECORD_LEN + 1;
        bytes[admitted_flags_offset] = 0xa5;
        let cadence = NativeReplayCadence {
            game_mode: 1,
            timing_mode: 2,
            frame_send_rate: NonZeroI32::new(2).unwrap(),
        };
        let mut playback =
            NativeReplayStream::initialize(&bytes, NativeReplayHeader::default(), |_| {
                Ok::<_, ()>(())
            })
            .unwrap();

        let first = playback.begin_frame().unwrap();
        assert_eq!(first.view, Some([0x11; 8]));
        assert_eq!(first.selection_count, Some(2));
        assert_eq!(first.selection_checksum, Some(0x6800_0003));
        assert_eq!(first.selected_objects, [0x3400_0001, 0x3400_0002]);
        assert_eq!(first.cursor, [Some(3), Some(4)]);
        assert_eq!(playback.finish_scheduled_frame(1, cadence).unwrap(), None);

        let second = playback.begin_frame().unwrap();
        assert_eq!(second.view, Some([0x22; 8]));
        assert_eq!(second.cursor, [Some(5), Some(6)]);
        let commands = playback
            .finish_scheduled_frame(2, cadence)
            .unwrap()
            .unwrap();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].flags(), 0xa4);
        assert_eq!(commands[0].opcode(), 0xfe);
        assert_eq!(commands[0].payload(), &command_bytes[7..]);
    }

    #[test]
    fn gsi_17_07_negative_counts_are_empty_and_short_command_records_stop_playback() {
        let mut bytes = NativeReplayHeader::new(1, "x.map").encode().to_vec();
        bytes.extend_from_slice(&[0x44; 8]);
        bytes.extend_from_slice(&(-3_i32).to_le_bytes());
        bytes.extend_from_slice(&0x0102_0304_u32.to_le_bytes());
        bytes.extend_from_slice(&7_u32.to_le_bytes());
        bytes.extend_from_slice(&8_u32.to_le_bytes());
        bytes.extend_from_slice(&(-2_i32).to_le_bytes());
        let mut playback =
            NativeReplayStream::initialize(&bytes, NativeReplayHeader::default(), |_| {
                Ok::<_, ()>(())
            })
            .unwrap();
        let update = playback.begin_frame().unwrap();
        assert_eq!(update.selection_count, Some(-3));
        assert_eq!(update.selection_checksum, Some(0x0102_0304));
        assert!(update.selected_objects.is_empty());
        assert_eq!(update.cursor, [Some(7), Some(8)]);
        assert!(playback.finish_frame(true).unwrap().unwrap().is_empty());

        let mut truncated = NativeReplayHeader::new(1, "x.map").encode().to_vec();
        truncated.extend_from_slice(&[0; 8]);
        truncated.extend_from_slice(&0_i32.to_le_bytes());
        truncated.extend_from_slice(&0_u32.to_le_bytes());
        truncated.extend_from_slice(&[0; 8]);
        truncated.extend_from_slice(&1_i32.to_le_bytes());
        truncated.extend_from_slice(&[0xcc; COMMAND_RECORD_LEN - 1]);
        let mut playback =
            NativeReplayStream::initialize(&truncated, NativeReplayHeader::default(), |_| {
                Ok::<_, ()>(())
            })
            .unwrap();
        playback.begin_frame().unwrap();
        assert!(matches!(
            playback.finish_frame(true),
            Err(NativeReplayError::Truncated {
                field: "command record",
                ..
            })
        ));
    }

    #[test]
    fn gsi_17_07_frame_order_and_native_cadence_are_explicit() {
        let mode_zero = NativeReplayCadence {
            game_mode: 0,
            timing_mode: 2,
            frame_send_rate: NonZeroI32::new(3).unwrap(),
        };
        let mode_five = NativeReplayCadence {
            game_mode: 5,
            ..mode_zero
        };
        let gated = NativeReplayCadence {
            game_mode: 1,
            ..mode_zero
        };
        let ungated_timing = NativeReplayCadence {
            timing_mode: 1,
            ..gated
        };
        assert!(mode_zero.reads_command_batch(0));
        assert!(!gated.reads_command_batch(0));
        assert!(gated.reads_command_batch(-1));
        assert!(mode_zero.reads_command_batch(1));
        assert!(mode_five.reads_command_batch(1));
        assert!(ungated_timing.reads_command_batch(1));
        assert!(!gated.reads_command_batch(1));
        assert!(gated.reads_command_batch(3));

        let bytes = NativeReplayHeader::new(1, "x.map").encode();
        let mut playback =
            NativeReplayStream::initialize(&bytes, NativeReplayHeader::default(), |_| {
                Ok::<_, ()>(())
            })
            .unwrap();
        assert!(matches!(
            playback.finish_frame(false),
            Err(NativeReplayError::FrameOrder { .. })
        ));
        playback.begin_frame().unwrap();
        assert!(matches!(
            playback.begin_frame(),
            Err(NativeReplayError::FrameOrder { .. })
        ));
        assert_eq!(playback.finish_frame(false).unwrap(), None);
        playback.begin_frame().unwrap();
        assert_eq!(playback.finish_frame(false).unwrap(), None);
    }

    #[test]
    fn diagnostic_runners_compare_frame_info_and_finish_the_recorded_failure_tail() {
        use crate::sim::command_queue::{FrameChecksumState, FrameInfo};
        use crate::sim::runtime::SimRuntime;
        use crate::sim::world::FrameEffects;
        use std::cell::RefCell;

        fn world(frame: u32) -> Simulation {
            let mut sim = Simulation::with_seed(73);
            let owner = sim.interner.intern("Human");
            sim.houses
                .register(crate::sim::house_state::HouseState::new(
                    owner, 0, None, true, 0, 10,
                ));
            sim.session
                .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Wol);
            // WOL admission includes MaxAhead before the frame-256 House gate.
            sim.install_network_timing(10);
            sim.session.set_binary_frame_for_test(frame);
            sim.session.set_tick_for_test(u64::from(frame));
            sim.hash_state_every(std::num::NonZeroU64::MIN);
            sim
        }
        for (frame, wrong_crc) in [(0, true), (256, false), (256, true)] {
            let mut control = SimRuntime::from_simulation(world(frame));
            let checksum = control
                .advance_frame(&[], 33, TickLane::Ordinary, FrameEffects::empty())
                .unwrap()
                .multiplayer_checksum
                .unwrap()
                .checksum
                .unwrap()
                .value;
            let record = FrameInfo {
                house_id: 0,
                event_frame: frame as i32,
                checksum: if wrong_crc { checksum ^ 1 } else { checksum },
                timing_word: 0,
                delay: 0,
            }
            .encode();
            let mut live = SimRuntime::from_simulation(world(frame));
            let mut replay = ReplayLog::new(ReplayHeader {
                version: 1,
                tick_hz: 15,
                seed: 73,
                map_name: "frameinfo".into(),
                rules_hash: 0,
                pixel_conversion_bounds: live.simulation.session().pixel_conversion_bounds(),
            });
            let network = RefCell::new(FrameChecksumState::new());
            live.simulation.admit_native_command_record(record);
            let mut output = live
                .advance_frame(
                    &[],
                    33,
                    TickLane::Ordinary,
                    FrameEffects::empty().with_network_commands(&network),
                )
                .unwrap();
            assert_eq!(output.network_failure.is_some(), frame != 0 && wrong_crc);
            if output.network_failure.is_some() {
                output = live
                    .resume_after_network_failure(
                        FrameEffects::empty().with_network_commands(&network),
                    )
                    .unwrap();
            }
            replay.record_tick(
                output.tick.tick,
                output.take_admitted_commands(),
                output.tick.state_hash,
            );
            let expected = vec![output.tick.state_hash];
            let mut runtime = SimRuntime::from_simulation(world(frame));
            assert_eq!(
                ReplayRunner::run_runtime(&mut runtime, &replay, 33).unwrap(),
                expected
            );
            assert!(runtime.simulation.pending_network_failure().is_none());
            assert_eq!(runtime.simulation.session().binary_frame(), frame + 1);
            let mut fixture = world(frame);
            assert_eq!(
                ReplayRunner::run_fixture(&mut fixture, &replay, None, None, 33),
                expected
            );
            assert!(fixture.pending_network_failure().is_none());
            assert_eq!(fixture.session().binary_frame(), frame + 1);
        }
    }

    #[test]
    fn diagnostic_log_json_roundtrip() {
        let mut log = ReplayLog::new(ReplayHeader {
            pixel_conversion_bounds: Default::default(),
            version: 1,
            tick_hz: 30,
            seed: 42,
            map_name: "test".to_string(),
            rules_hash: 123,
        });
        log.record_tick(
            1,
            vec![
                CommandEnvelope::new(
                    crate::sim::intern::test_intern("Americans"),
                    1,
                    Command::SetRally {
                        rx: 10,
                        ry: 11,
                        producer_ids: vec![1, 2],
                    },
                ),
                CommandEnvelope::synchronized(
                    CommandRecord::encode(
                        0xfe,
                        2,
                        7,
                        &[0xa5; crate::sim::command::COMMAND_RECORD_PAYLOAD_LEN],
                    )
                    .unwrap(),
                    1,
                ),
            ],
            Some(999),
        );
        // A tick that did not hash leaves its hash out.
        log.record_tick(2, Vec::new(), None);
        log.initial_scenario_services
            .push(ScenarioServiceInput::Pause);
        log.ticks[0]
            .scenario_services
            .push(ScenarioServiceInput::Resume);
        let mut bytes = Vec::new();
        log.write_json(&mut bytes).expect("write diagnostic log");
        let json = std::str::from_utf8(&bytes).expect("JSON is UTF-8");
        assert_eq!(json.matches("state_hash").count(), 1);
        let parsed: ReplayLog = serde_json::from_slice(&bytes).expect("deserialize");
        assert_eq!(parsed.header.tick_hz, 30);
        assert_eq!(parsed.ticks.len(), 2);
        assert_eq!(parsed.ticks[0].tick, 1);
        assert_eq!(parsed.ticks[0].state_hash, Some(999));
        assert_eq!(parsed.ticks[0].commands, log.ticks[0].commands);
        assert_eq!(
            parsed.initial_scenario_services,
            log.initial_scenario_services
        );
        assert_eq!(
            parsed.ticks[0].scenario_services,
            log.ticks[0].scenario_services
        );
        assert_eq!(parsed.ticks[1].state_hash, None);
    }

    #[test]
    fn diagnostic_log_writer_propagates_final_flush_failure() {
        #[derive(Default)]
        struct FlushFailure(Vec<u8>);
        impl std::io::Write for FlushFailure {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::other("injected final flush failure"))
            }
        }
        let log = ReplayLog::new(ReplayHeader {
            pixel_conversion_bounds: Default::default(),
            version: 1,
            tick_hz: 30,
            seed: 42,
            map_name: "flush.map".to_owned(),
            rules_hash: 123,
        });
        let mut writer = FlushFailure::default();
        let error = log
            .write_json(&mut writer)
            .expect_err("flush failure must reach the segment owner");
        assert!(
            serde_json::from_slice::<ReplayLog>(&writer.0).is_ok(),
            "complete bytes were written before flush failed"
        );
        assert!(
            error
                .chain()
                .any(|cause| cause.to_string().contains("injected final flush failure"))
        );
    }

    #[test]
    fn gsi_01_04_exit_diagnostic_replay_roundtrips_without_native_version_bump() {
        let mut sim = Simulation::with_seed(7);
        let owner = sim.interner.intern("Local");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, true, 0, 10,
            ));
        sim.session.set_tick_for_test(3);
        let mut log = ReplayLog::new(ReplayHeader {
            pixel_conversion_bounds: Default::default(),
            version: 1,
            tick_hz: 15,
            seed: 7,
            map_name: "abort.map".to_string(),
            rules_hash: 9,
        });
        log.record_tick(
            4,
            vec![CommandEnvelope::new(owner, 4, Command::ExitMatch)],
            Some(11),
        );

        let json = serde_json::to_string(&log).unwrap();
        let decoded: ReplayLog = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.header.version, 1);
        assert_eq!(decoded.ticks[0].commands[0].payload, Command::ExitMatch);
        assert_eq!(NATIVE_REPLAY_VERSION, 10);

        let hashes = ReplayRunner::run_fixture(&mut sim, &decoded, None, None, 33);
        assert_eq!(hashes.len(), 1);
        assert!(sim.quit_requested());
        assert_eq!(sim.take_executed_exit_owner(), Some(owner));
        assert_eq!(sim.take_executed_exit_owner(), None);
    }

    #[test]
    fn diagnostic_replay_reproduces_midmatch_game_speed_transition() {
        let make_sim = || {
            let mut sim = Simulation::with_seed(7);
            sim.hash_state_every(std::num::NonZeroU64::MIN);
            let owner = sim.interner.intern("Local");
            sim.houses
                .register(crate::sim::house_state::HouseState::new(
                    owner, 0, None, true, 0, 10,
                ));
            (sim, owner)
        };
        let (mut recorded, owner) = make_sim();
        let command = CommandEnvelope::new(owner, 1, Command::SetGameSpeed { speed: 4 });
        let tick = recorded.advance_tick(std::slice::from_ref(&command), None, None, None, 67);
        assert_eq!(recorded.session.game_options().game_speed, 4);

        let mut log = ReplayLog::new(ReplayHeader {
            pixel_conversion_bounds: Default::default(),
            version: 1,
            tick_hz: 15,
            seed: 7,
            map_name: "speed.map".to_string(),
            rules_hash: 0,
        });
        log.record_tick(tick.tick, vec![command], tick.state_hash);
        let json = serde_json::to_string(&log).expect("serialize GameSpeed replay");
        let decoded: ReplayLog = serde_json::from_str(&json).expect("decode GameSpeed replay");

        let (mut replayed, _) = make_sim();
        let hashes = ReplayRunner::run_fixture(&mut replayed, &decoded, None, None, 67);
        assert_eq!(hashes, vec![tick.state_hash]);
        assert_eq!(replayed.session.game_options().game_speed, 4);
        assert_eq!(replayed.state_hash(), recorded.state_hash());
        assert_eq!(NATIVE_REPLAY_VERSION, 10);
    }

    #[test]
    fn diagnostic_replay_retains_a_future_admission_until_its_frame() {
        let mut sim = Simulation::with_seed(7);
        let owner = sim.interner.intern("Local");
        let mut log = ReplayLog::new(ReplayHeader {
            pixel_conversion_bounds: Default::default(),
            version: 1,
            tick_hz: 15,
            seed: 7,
            map_name: "future-command.map".to_string(),
            rules_hash: 0,
        });
        log.record_tick(
            1,
            vec![CommandEnvelope::new(
                owner,
                9,
                Command::Stop { entity_id: 41 },
            )],
            Some(0),
        );
        log.record_tick(2, Vec::new(), Some(0));

        let hashes = ReplayRunner::run_fixture(&mut sim, &log, None, None, 33);

        assert_eq!(hashes.len(), 2);
        assert!(
            matches!(sim.pending_command_snapshot().as_slice(), [command] if command.execute_tick == 9),
            "the admission journal retains accepted future work"
        );
    }

    #[test]
    fn diagnostic_replay_reconciles_each_recorded_admission_once() {
        let mut sim = Simulation::with_seed(7);
        let owner = sim.interner.intern("Local");
        let place = CommandEnvelope::new(
            owner,
            1,
            Command::PlaceProducedMobile {
                category: crate::sim::production::ProductionCategory::Infantry,
            },
        );
        let future = CommandEnvelope::new(owner, 9, place.payload.clone());
        let unrelated = CommandEnvelope::new(owner, 1, Command::Stop { entity_id: 41 });
        sim.queue_commands([
            place.clone(),
            unrelated.clone(),
            place.clone(),
            future.clone(),
        ]);

        let due = sim.take_due_replay_commands([place.clone(), future.clone()]);

        assert_eq!(due, [place.clone(), future]);
        assert_eq!(
            sim.pending_command_snapshot(),
            &[unrelated, place],
            "each recorded admission consumes one matching copy, including future work"
        );
    }
}
