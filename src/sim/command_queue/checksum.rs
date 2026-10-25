//! FRAMEINFO records and the process-lifetime checksum comparison state.

use std::mem::size_of;

use super::ChecksumMismatch;
use crate::sim::command::CommandRecord;

pub const FRAMEINFO_EVENT_OPCODE: u8 = 0x1c;
pub const CHECKSUM_HISTORY_LEN: usize = 0x100;

const FRAMEINFO_CHECKSUM_PAYLOAD_OFFSET: usize = 0;
const FRAMEINFO_TIMING_WORD_PAYLOAD_OFFSET: usize = 4;
const FRAMEINFO_DELAY_PAYLOAD_OFFSET: usize = 6;

/// Payload carried by the leading FRAMEINFO record of a multiplayer packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameInfo {
    pub house_id: i8,
    pub event_frame: i32,
    pub checksum: u32,
    pub timing_word: u16,
    pub delay: u8,
}

impl FrameInfo {
    pub fn encode(self) -> CommandRecord {
        assert!(
            self.house_id >= 0,
            "outgoing FRAMEINFO requires a registered house id"
        );
        let mut record = CommandRecord::encode(
            FRAMEINFO_EVENT_OPCODE,
            i32::from(self.house_id),
            self.event_frame,
            &[],
        )
        .expect("FRAMEINFO has a fixed in-record payload");
        self.write_to(&mut record);
        record
    }

    /// Overwrite only the verified FRAMEINFO fields. Flags and every unknown
    /// payload byte remain untouched.
    pub fn write_to(self, record: &mut CommandRecord) {
        assert!(
            self.house_id >= 0,
            "outgoing FRAMEINFO requires a registered house id"
        );
        record.set_issue_header(
            FRAMEINFO_EVENT_OPCODE,
            i32::from(self.house_id),
            self.event_frame,
        );
        let payload = record.payload_mut();
        payload[FRAMEINFO_CHECKSUM_PAYLOAD_OFFSET
            ..FRAMEINFO_CHECKSUM_PAYLOAD_OFFSET + size_of::<u32>()]
            .copy_from_slice(&self.checksum.to_le_bytes());
        payload[FRAMEINFO_TIMING_WORD_PAYLOAD_OFFSET
            ..FRAMEINFO_TIMING_WORD_PAYLOAD_OFFSET + size_of::<u16>()]
            .copy_from_slice(&self.timing_word.to_le_bytes());
        payload[FRAMEINFO_DELAY_PAYLOAD_OFFSET] = self.delay;
    }

    pub fn decode(record: &CommandRecord) -> Option<Self> {
        if record.opcode() != FRAMEINFO_EVENT_OPCODE {
            return None;
        }
        let payload = record.payload();
        Some(Self {
            house_id: record.house_id(),
            event_frame: record.frame_stamp(),
            checksum: u32::from_le_bytes(
                payload[FRAMEINFO_CHECKSUM_PAYLOAD_OFFSET
                    ..FRAMEINFO_CHECKSUM_PAYLOAD_OFFSET + size_of::<u32>()]
                    .try_into()
                    .expect("FRAMEINFO checksum is four bytes"),
            ),
            timing_word: u16::from_le_bytes(
                payload[FRAMEINFO_TIMING_WORD_PAYLOAD_OFFSET
                    ..FRAMEINFO_TIMING_WORD_PAYLOAD_OFFSET + size_of::<u16>()]
                    .try_into()
                    .expect("FRAMEINFO timing word is two bytes"),
            ),
            delay: payload[FRAMEINFO_DELAY_PAYLOAD_OFFSET],
        })
    }

    #[inline]
    pub fn history_index(self) -> usize {
        ((self.event_frame as u32).wrapping_sub(u32::from(self.delay)) & 0xff) as usize
    }
}

/// The two live fields read from the network timing gate before retail compares
/// a due FRAMEINFO checksum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameInfoCompareGate {
    /// Native timing-window start frame. `-1` means no elapsed-frame subtraction.
    pub base_frame: i32,
    /// Native `param_2[2]`, tested against elapsed frames.
    pub remaining_frames: i32,
}

impl FrameInfoCompareGate {
    /// An already-open gate.
    pub const OPEN: Self = Self {
        base_frame: -1,
        remaining_frames: 0,
    };

    pub const fn new(base_frame: i32, remaining_frames: i32) -> Self {
        Self {
            base_frame,
            remaining_frames,
        }
    }

    /// Retail compares when the no-base remainder is zero, or when elapsed
    /// frames have reached the stored remainder.
    #[inline]
    pub fn allows_compare(self, current_frame: i32) -> bool {
        if self.base_frame == -1 {
            self.remaining_frames == 0
        } else {
            self.remaining_frames <= current_frame.wrapping_sub(self.base_frame)
        }
    }
}

/// The active 256-frame checksum ring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiplayerChecksumHistory {
    values: [u32; CHECKSUM_HISTORY_LEN],
}

impl MultiplayerChecksumHistory {
    pub const fn new() -> Self {
        Self {
            values: [0; CHECKSUM_HISTORY_LEN],
        }
    }

    #[inline]
    pub fn record(&mut self, frame: u32, checksum: u32) {
        self.values[(frame & 0xff) as usize] = checksum;
    }

    /// Read the retained ring slot for diagnostics. Callers tracking more than
    /// 256 frames must read each value before its slot is overwritten.
    #[inline]
    pub fn get(&self, frame: u32) -> u32 {
        self.values[(frame & 0xff) as usize]
    }

    pub fn compare(&self, frame_info: FrameInfo) -> Result<(), ChecksumMismatch> {
        let history_index = frame_info.history_index();
        let local_checksum = self.values[history_index];
        if local_checksum == frame_info.checksum {
            Ok(())
        } else {
            Err(ChecksumMismatch {
                house_id: frame_info.house_id,
                event_frame: frame_info.event_frame,
                delay: frame_info.delay,
                history_index: history_index as u8,
                local_checksum,
                remote_checksum: frame_info.checksum,
            })
        }
    }
}

impl Default for MultiplayerChecksumHistory {
    fn default() -> Self {
        Self::new()
    }
}

/// Checksum history and comparison timing shared by a live process and its
/// replay runner. Each runtime keeps one instance across its frame visits.
/// The simulation borrows it at the queue boundary instead of storing a copy.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrameChecksumState {
    /// Global B04474, written by Queue_AI_Multiplayer6476A5. Launch and load
    /// retain it; the native queue's reset branch clears the ring.
    history: MultiplayerChecksumHistory,
    /// Process timer AFA450/AFA458, initialized lazily and rearmed together
    /// with the ring. Execute_DoList checks it before comparing FRAMEINFO.
    compare_gate: Option<FrameInfoCompareGate>,
}

impl FrameChecksumState {
    pub const fn new() -> Self {
        Self {
            history: MultiplayerChecksumHistory::new(),
            compare_gate: None,
        }
    }

    /// Store one multiplayer frame's checksum at `history[frame & 0xFF]`.
    /// Queue_AI_Multiplayer6476A5 stores frame 0 before 6476B1 ->647825
    /// clears the entire ring at647879. A8B8B8 also reaches the reset branch;
    /// that flag's transport writer remains outside this owner.
    pub fn record_frame_checksum(&mut self, frame: u32, checksum: u32) {
        //647615..64762F initializes the static timer on the first call, with
        // no grace period unless the reset branch below is also reached.
        self.compare_gate
            .get_or_insert(FrameInfoCompareGate::new(frame as i32, 0));
        self.history.record(frame, checksum);
        if frame == 0 {
            self.reset_frame_checksum_history(frame);
        }
    }

    /// Queue_AI_Multiplayer647849..647879 resets both history and its grace
    /// timer. The period is literally Frame+256, including a nonzero restart.
    pub fn reset_frame_checksum_history(&mut self, frame: u32) {
        self.history = MultiplayerChecksumHistory::new();
        self.compare_gate = Some(FrameInfoCompareGate::new(
            frame as i32,
            frame.wrapping_add(0x100) as i32,
        ));
    }

    /// Execute_DoList64C668..64C6F2 gates comparison before reading the ring.
    /// Its caller already owns the on-time frame and eligible-House checks.
    /// Source: original gamemd.exe instructions.
    pub fn compare_frame_info(
        &self,
        info: FrameInfo,
        current_frame: i32,
    ) -> Result<bool, ChecksumMismatch> {
        if !self
            .compare_gate
            .unwrap_or(FrameInfoCompareGate::OPEN)
            .allows_compare(current_frame)
        {
            return Ok(false);
        }
        self.history.compare(info)?;
        Ok(true)
    }

    /// The same history used by live FRAMEINFO comparisons, without mutation.
    pub fn checksum_history(&self) -> &MultiplayerChecksumHistory {
        &self.history
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn compare_gate(&self) -> Option<FrameInfoCompareGate> {
        self.compare_gate
    }
}
