//! Native recording stream plus deterministic diagnostic-log helpers.
//!
//! Retail recordings are not save games. They contain a fixed startup header,
//! then presentation/sync records plus command batches at the command-transfer
//! rungs selected by the running session. Playback rebuilds the scenario
//! normally from the recorded header and consumes both record kinds at their
//! original scheduler rungs.
//!
//! [`ReplayLog`] remains the richer Rust-only command/hash diagnostic used by
//! parity tests. It is deliberately separate from [`NativeReplay`].

#[cfg(test)]
use std::collections::BTreeMap;
use std::num::NonZeroI32;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(test)]
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{COMMAND_RECORD_LEN, CommandEnvelope, CommandRecord};
#[cfg(test)]
use crate::sim::pathfinding::PathGrid;
#[cfg(test)]
use crate::sim::world::{Simulation, TickLane, TriggerInputs};

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

/// Legacy recorder bit. No active retail YR caller sets it.
pub const REPLAY_FLAG_RECORD: u32 = 0x01;
/// Playback bit set when session preparation opens an external `RECORD.BIN`.
pub const REPLAY_FLAG_PLAYBACK: u32 = 0x02;
/// Availability bit set by active retail YR's `-ATTRACT` command-line path.
pub const REPLAY_FLAG_AVAILABLE: u32 = 0x04;

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
        let len = name
            .as_bytes()
            .len()
            .min(NATIVE_REPLAY_SCENARIO_NAME_LEN - 1);
        self.scenario_name[..len].copy_from_slice(&name.as_bytes()[..len]);
    }

    pub fn scenario_name_bytes(&self) -> &[u8; NATIVE_REPLAY_SCENARIO_NAME_LEN] {
        &self.scenario_name
    }

    pub fn options_bytes(&self) -> &[u8; NATIVE_REPLAY_OPTIONS_LEN] {
        &self.options
    }

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

/// Eager legacy-format document codec used by tooling and fixtures.
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
    pub fn new(header: NativeReplayHeader) -> Self {
        Self {
            header,
            frames: Vec::new(),
        }
    }

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

    pub fn save(&self, path: &Path) -> Result<()> {
        let bytes = self.encode().context("Failed to encode native replay")?;
        std::fs::write(path, bytes)
            .with_context(|| format!("Failed to write native replay: {}", path.display()))
    }

    /// Initialize playback through the normal scenario-loading owner.
    ///
    /// No snapshot or serialized active-object vector is accepted here. The
    /// caller must rebuild the scenario from the header, after which frames are
    /// exposed in stream order for the scheduler's presentation and command
    /// rungs.
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
    pub version: u32,
    pub tick_hz: u32,
    pub seed: u64,
    pub map_name: String,
    pub rules_hash: u64,
}

/// One tick in the Rust-only deterministic diagnostic log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayTick {
    pub tick: u64,
    pub commands: Vec<CommandEnvelope>,
    pub state_hash: u64,
}

/// Rich Rust-only command/hash log used by tests and diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayLog {
    pub header: ReplayHeader,
    pub ticks: Vec<ReplayTick>,
}

impl ReplayLog {
    pub fn new(header: ReplayHeader) -> Self {
        Self {
            header,
            ticks: Vec::new(),
        }
    }

    pub fn record_tick(&mut self, tick: u64, commands: Vec<CommandEnvelope>, state_hash: u64) {
        self.ticks.push(ReplayTick {
            tick,
            commands,
            state_hash,
        });
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(self)
            .context("Failed to serialize deterministic diagnostic log to JSON")?;
        std::fs::write(path, bytes)
            .with_context(|| format!("Failed to write diagnostic log: {}", path.display()))
    }

    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("Failed to read diagnostic log: {}", path.display()))?;
        serde_json::from_slice(&bytes).context("Failed to parse deterministic diagnostic JSON")
    }
}

/// Headless runner for the Rust-only deterministic diagnostic log.
pub struct ReplayRunner;

impl ReplayRunner {
    /// Re-run diagnostic-log ticks through the bound runtime (F09): every
    /// tick runs the exact production frame transaction, so rules, heights,
    /// overlay registry, and trigger definitions come from the runtime
    /// resources and navigation re-snapshots per frame just as a recorded
    /// app session did - nothing is caller-substitutable and a mid-match
    /// path-grid change replays faithfully.
    pub fn run_runtime(
        runtime: &mut crate::sim::runtime::SimRuntime,
        replay: &ReplayLog,
        tick_ms: u32,
    ) -> Result<Vec<u64>> {
        debug_assert_eq!(
            runtime.simulation.session.seed, replay.header.seed,
            "replay playback sim must be constructed from header.seed"
        );
        let current = runtime.resources.rules.simulation_config_hash();
        if replay.header.rules_hash != 0 && replay.header.rules_hash != current {
            log::warn!(
                "replay rules_hash mismatch (header {:#018x} vs current \n                 {:#018x}) - recorded under a different rules set; \n                 playback will desync",
                replay.header.rules_hash,
                current
            );
        }
        let mut hashes: Vec<u64> = Vec::with_capacity(replay.ticks.len());
        for entry in &replay.ticks {
            let due_commands = runtime
                .simulation
                .take_due_replay_commands(entry.commands.iter().cloned());
            let output = runtime.advance_frame(
                &due_commands,
                tick_ms,
                crate::sim::world::TickLane::Ordinary,
            )?;
            hashes.push(output.tick.state_hash);
        }
        Ok(hashes)
    }

    /// Fixture-only replay runner (F09): re-run diagnostic-log ticks with
    /// explicitly supplied resources. Production replay execution goes
    /// through `run_runtime`, whose resources are bound in the `SimRuntime`.
    #[cfg(test)]
    pub(crate) fn run_fixture(
        sim: &mut Simulation,
        replay: &ReplayLog,
        rules: Option<&RuleSet>,
        height_map: &BTreeMap<(u16, u16), u8>,
        path_grid: Option<&PathGrid>,
        tick_ms: u32,
    ) -> Vec<u64> {
        Self::run_fixture_with_overlay_registry(
            sim, replay, rules, height_map, path_grid, None, tick_ms,
        )
    }

    /// Fixture-only variant with the static overlay type registry used by the
    /// recorded map. Overlay-backed simulation authority (including miner
    /// resource queries) must see the same registry on record and replay.
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run_fixture_with_overlay_registry(
        sim: &mut Simulation,
        replay: &ReplayLog,
        rules: Option<&RuleSet>,
        height_map: &BTreeMap<(u16, u16), u8>,
        path_grid: Option<&PathGrid>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        tick_ms: u32,
    ) -> Vec<u64> {
        Self::run_fixture_master_frame(
            sim,
            replay,
            rules,
            height_map,
            path_grid,
            overlay_registry,
            tick_ms,
            None,
        )
    }

    /// Fixture-only replay through the same master-frame admission gameplay
    /// uses, with caller-owned static map trigger definitions. Diagnostic
    /// logs own commands and hashes. Production replay execution is
    /// `run_runtime`.
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run_fixture_master_frame(
        sim: &mut Simulation,
        replay: &ReplayLog,
        rules: Option<&RuleSet>,
        height_map: &BTreeMap<(u16, u16), u8>,
        path_grid: Option<&PathGrid>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        tick_ms: u32,
        trigger_inputs: Option<TriggerInputs<'_>>,
    ) -> Vec<u64> {
        // The diagnostic playback must be constructed from the recorded seed.
        // A sim seeded
        // differently than the header it replays is a guaranteed silent
        // divergence.
        debug_assert_eq!(
            sim.session.seed, replay.header.seed,
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
        let mut hashes: Vec<u64> = Vec::with_capacity(replay.ticks.len());
        for entry in &replay.ticks {
            let due_commands = sim.take_due_replay_commands(entry.commands.iter().cloned());
            let result = sim
                .advance_master_frame(
                    &due_commands,
                    rules,
                    height_map,
                    path_grid,
                    overlay_registry,
                    tick_ms,
                    TickLane::Ordinary,
                    trigger_inputs,
                )
                .expect("fixture frame must complete");
            // Replay has no app layer to consume presentation-only trigger effects.
            let _ = sim.drain_trigger_effects();
            hashes.push(result.state_hash);
        }
        hashes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

        let mut initialized = NativeReplayHeader::default();
        initialized.version = 0x1122_3344;
        initialized.seed = 0xaabb_ccdd;
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
    fn diagnostic_log_json_roundtrip() {
        let mut log = ReplayLog::new(ReplayHeader {
            version: 1,
            tick_hz: 30,
            seed: 42,
            map_name: "test".to_string(),
            rules_hash: 123,
        });
        log.record_tick(
            1,
            vec![CommandEnvelope::new(
                crate::sim::intern::test_intern("Americans"),
                1,
                Command::SetRally {
                    owner: crate::sim::intern::test_intern("Americans"),
                    rx: 10,
                    ry: 11,
                    producer_ids: vec![1, 2],
                },
            )],
            999,
        );
        let json = serde_json::to_string(&log).expect("serialize");
        let parsed: ReplayLog = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.header.tick_hz, 30);
        assert_eq!(parsed.ticks.len(), 1);
        assert_eq!(parsed.ticks[0].tick, 1);
        assert_eq!(parsed.ticks[0].state_hash, 999);
    }

    #[test]
    fn gsi_04_07_wall_sell_diagnostic_replay_roundtrips_without_native_version_bump() {
        let owner = crate::sim::intern::test_intern("Receiver");
        let mut log = ReplayLog::new(ReplayHeader {
            version: 1,
            tick_hz: 15,
            seed: 7,
            map_name: "wall.map".to_string(),
            rules_hash: 9,
        });
        log.record_tick(
            4,
            vec![CommandEnvelope::new(
                owner,
                4,
                Command::SellWallAtCell { x: 0, y: 12 },
            )],
            11,
        );
        let json = serde_json::to_string(&log).unwrap();
        let decoded: ReplayLog = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.header.version, 1);
        assert_eq!(
            decoded.ticks[0].commands[0].payload,
            Command::SellWallAtCell { x: 0, y: 12 }
        );
        assert_eq!(NATIVE_REPLAY_VERSION, 10);
    }

    #[test]
    fn gsi_01_04_exit_diagnostic_replay_roundtrips_without_native_version_bump() {
        let mut sim = Simulation::with_seed(7);
        let owner = sim.interner.intern("Local");
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
        );
        sim.session.house_order.push(owner);
        sim.session.tick = 3;
        let mut log = ReplayLog::new(ReplayHeader {
            version: 1,
            tick_hz: 15,
            seed: 7,
            map_name: "abort.map".to_string(),
            rules_hash: 9,
        });
        log.record_tick(
            4,
            vec![CommandEnvelope::new(owner, 4, Command::ExitMatch)],
            11,
        );

        let json = serde_json::to_string(&log).unwrap();
        let decoded: ReplayLog = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.header.version, 1);
        assert_eq!(decoded.ticks[0].commands[0].payload, Command::ExitMatch);
        assert_eq!(NATIVE_REPLAY_VERSION, 10);

        let hashes =
            ReplayRunner::run_fixture(&mut sim, &decoded, None, &BTreeMap::new(), None, 33);
        assert_eq!(hashes.len(), 1);
        assert!(sim.quit_requested);
        assert_eq!(sim.take_executed_exit_owner(), Some(owner));
        assert_eq!(sim.take_executed_exit_owner(), None);
    }

    #[test]
    fn diagnostic_replay_reproduces_midmatch_game_speed_transition() {
        let make_sim = || {
            let mut sim = Simulation::with_seed(7);
            let owner = sim.interner.intern("Local");
            sim.houses.insert(
                owner,
                crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
            );
            sim.session.house_order.push(owner);
            (sim, owner)
        };
        let (mut recorded, owner) = make_sim();
        let command = CommandEnvelope::new(owner, 1, Command::SetGameSpeed { speed: 4 });
        let tick = recorded.advance_tick(
            std::slice::from_ref(&command),
            None,
            &BTreeMap::new(),
            None,
            None,
            67,
        );
        assert_eq!(recorded.session.game_options.game_speed, 4);

        let mut log = ReplayLog::new(ReplayHeader {
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
        let hashes =
            ReplayRunner::run_fixture(&mut replayed, &decoded, None, &BTreeMap::new(), None, 67);
        assert_eq!(hashes, vec![tick.state_hash]);
        assert_eq!(replayed.session.game_options.game_speed, 4);
        assert_eq!(replayed.state_hash(), recorded.state_hash());
        assert_eq!(NATIVE_REPLAY_VERSION, 10);
    }

    #[test]
    fn diagnostic_replay_discards_a_malformed_future_entry() {
        let mut sim = Simulation::with_seed(7);
        let owner = sim.interner.intern("Local");
        let mut log = ReplayLog::new(ReplayHeader {
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
            0,
        );
        log.record_tick(2, Vec::new(), 0);

        let hashes = ReplayRunner::run_fixture(&mut sim, &log, None, &BTreeMap::new(), None, 33);

        assert_eq!(hashes.len(), 2);
        assert!(
            sim.pending_commands_for_tests().is_empty(),
            "the old replay seam ignored and discarded a future-stamped entry"
        );
    }

    #[test]
    fn diagnostic_replay_does_not_double_admit_regenerated_pending_work() {
        let mut sim = Simulation::with_seed(7);
        let owner = sim.interner.intern("Local");
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
        );
        sim.session.house_order.push(owner);

        // Model a command regenerated during the preceding replay frame. The
        // live app drained and recorded the same command for this frame, so the
        // log is authoritative for execution while the regenerated queue entry
        // must remain isolated exactly as it was at the former direct seam.
        let regenerated = CommandEnvelope::new(owner, 1, Command::SetGameSpeed { speed: 4 });
        sim.queue_command(regenerated.clone());
        let mut log = ReplayLog::new(ReplayHeader {
            version: 1,
            tick_hz: 15,
            seed: 7,
            map_name: "regenerated-command.map".to_string(),
            rules_hash: 0,
        });
        log.record_tick(1, vec![regenerated.clone()], 0);

        let hashes = ReplayRunner::run_fixture(&mut sim, &log, None, &BTreeMap::new(), None, 33);

        assert_eq!(hashes.len(), 1);
        assert_eq!(sim.session.game_options.game_speed, 4);
        assert_eq!(
            sim.pending_commands_for_tests(),
            std::slice::from_ref(&regenerated),
            "the recorded copy executes while regenerated pending work stays isolated"
        );
    }
}
