//! Deterministic lockstep command staging and dispatch.
//!
//! Local commands retain their issue frame until transfer. Single-player
//! transfer admits that frame unchanged; network transfer overwrites every
//! staged record with the send frame plus the negotiated ahead window.
//!
//! Network rescheduling and physical packetization/compression are later
//! transport-owner boundaries. Opcode `0x13` local-vs-remote EXIT routing and
//! player removal are session-owner boundaries: due `0x13` records reach the
//! generic execute callback, but this module does not claim that callback alone
//! reproduces the native EXIT path.

use std::collections::VecDeque;
use std::mem::size_of;
#[cfg(test)]
use std::num::NonZeroU8;
use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

use crate::sim::command::{CommandRecord, CommandRecordError};
use crate::sim::intern::InternedId;

pub const MEGAMISSION_EVENT_OPCODE: u8 = 0x04;
pub const FRAMEINFO_EVENT_OPCODE: u8 = 0x1c;
pub const CHECKSUM_HISTORY_LEN: usize = 0x100;
pub const OUT_LIST_CAPACITY: usize = 0x80;
pub const DO_LIST_CAPACITY: usize = 0x4000;
pub const NETWORK_LOCAL_MIRROR_LIMIT: usize = 0x2000;
pub const MEGAMISSION_STAGE_CAPACITY: usize = 0x100;

const FRAMEINFO_CHECKSUM_PAYLOAD_OFFSET: usize = 0;
const FRAMEINFO_TIMING_WORD_PAYLOAD_OFFSET: usize = 4;
const FRAMEINFO_DELAY_PAYLOAD_OFFSET: usize = 6;
#[cfg(test)]
const QUEUE_ONLY_EVENT_OPCODE_0C: u8 = 0x0c;
#[cfg(test)]
const QUEUE_ONLY_EVENT_OPCODE_22: u8 = 0x22;

#[cfg(test)]
#[inline]
const fn queue_consumes_without_execute(opcode: u8) -> bool {
    matches!(
        opcode,
        QUEUE_ONLY_EVENT_OPCODE_0C | QUEUE_ONLY_EVENT_OPCODE_22
    )
}

/// The two verified network send-frame policies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkSendPolicy {
    /// Send every frame and stamp `current_frame + max_ahead`.
    EveryFrame,
    /// Mode 2 sends only on `frame_send_rate` boundaries and rounds the
    /// execute frame up to a boundary as well.
    FrameSendRate2 { frame_send_rate: NonZeroU32 },
}

impl NetworkSendPolicy {
    /// Whether the network path transfers its local queue on this frame.
    pub fn should_send(self, current_frame: u32) -> bool {
        match self {
            Self::EveryFrame => true,
            Self::FrameSendRate2 { frame_send_rate } => {
                let rate = frame_send_rate.get();
                let boundary = current_frame
                    .wrapping_add(rate.wrapping_sub(1))
                    .wrapping_div(rate)
                    .wrapping_mul(rate);
                current_frame == boundary
            }
        }
    }

    /// Frame written into each record at network transfer time.
    pub fn execute_frame(self, current_frame: u32, max_ahead: u32) -> u32 {
        match self {
            Self::EveryFrame => current_frame.wrapping_add(max_ahead),
            Self::FrameSendRate2 { frame_send_rate } => {
                let rate = frame_send_rate.get();
                rate.wrapping_add(current_frame)
                    .wrapping_sub(1)
                    .wrapping_add(max_ahead)
                    .wrapping_div(rate)
                    .wrapping_mul(rate)
            }
        }
    }
}

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

    #[cfg(test)]
    #[inline]
    pub fn get(&self, frame: u32) -> u32 {
        self.values[(frame & 0xff) as usize]
    }

    #[cfg(test)]
    fn compare(&self, frame_info: FrameInfo) -> Result<(), ChecksumMismatch> {
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

/// Exact diagnostic facts available when one due FRAMEINFO comparison fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "multiplayer checksum mismatch for house {house_id} at frame {event_frame}: \
     local {local_checksum:#010x}, remote {remote_checksum:#010x}, \
     history[{history_index}] with delay {delay}"
)]
pub struct ChecksumMismatch {
    pub house_id: i8,
    pub event_frame: i32,
    pub delay: u8,
    pub history_index: u8,
    pub local_checksum: u32,
    pub remote_checksum: u32,
}

/// One byte-exact native synchronized command.
///
/// Decoded Rust commands deliberately cannot be attached here. A native queue
/// may be serialized to replay or copied to the wire, so accepting a semantic
/// sidecar whose bytes do not decode to that same command would make the record
/// non-authoritative.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SynchronizedCommand {
    record: CommandRecord,
}

impl SynchronizedCommand {
    pub fn opaque(record: CommandRecord) -> Self {
        Self { record }
    }

    #[inline]
    pub fn record(&self) -> &CommandRecord {
        &self.record
    }

    /// Mutate only opcode-specific bytes during the required MegaMission batch
    /// adjustment; the synchronized header and acknowledgement flag stay owned
    /// by the queue.
    #[inline]
    pub fn payload_mut(&mut self) -> &mut [u8] {
        self.record.payload_mut()
    }

    #[cfg(test)]
    pub fn into_record(self) -> CommandRecord {
        self.record
    }

    /// Decode the bounded native command set at the synchronized execution
    /// seam. No semantic sidecar is retained beside the authoritative bytes.
    pub(crate) fn decode_for_simulation(
        &self,
        sim: &crate::sim::world::Simulation,
        execute_tick: u64,
    ) -> Option<crate::sim::command::CommandEnvelope> {
        sim.decode_native_command_record(&self.record, execute_tick)
    }

    #[cfg(test)]
    fn stages_as_megamission(&self) -> bool {
        self.record.opcode() == MEGAMISSION_EVENT_OPCODE
    }

    #[cfg(test)]
    fn stamp_for_network(&mut self, house_id: i8, execute_frame: u32) {
        self.record.set_house_id(house_id);
        self.record.set_frame_stamp(execute_frame as i32);
        self.record.clear_processed();
    }
}

/// Result counters from one native-order DoList scan.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DispatchSummary {
    /// Records forwarded to the command executor.
    pub executed: usize,
    /// Due timing records consumed without command execution.
    pub timing_consumed: usize,
    /// Due-on-this-frame timing records compared with local history.
    pub frame_info_compared: usize,
    /// Executed non-timing records whose frame was already past.
    pub late_executed: usize,
    /// Late non-FRAMEINFO records skipped by network-mode dispatch.
    pub late_skipped: usize,
    /// Due MegaMission records dropped because the native staging ring was full.
    pub megamission_dropped: usize,
    /// Contiguous processed/expired records retired from the DoList head.
    pub retired: usize,
}

#[derive(Debug, Clone, Copy)]
enum DispatchMode<'a> {
    Offline,
    Network {
        checksum_history: &'a MultiplayerChecksumHistory,
        frame_info_gate: FrameInfoCompareGate,
    },
}

/// One registered HouseClass slot presented to command dispatch.
///
/// `house_id` remains the native signed byte carried by command records.
/// `dispatch_eligible` is the caller-owned `IsHuman || PlayerControl` result;
/// keeping it explicit prevents skipped AI-only houses from compressing later
/// house ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandDispatchHouse {
    pub owner: InternedId,
    pub house_id: i8,
    pub dispatch_eligible: bool,
}

impl CommandDispatchHouse {
    pub const fn new(owner: InternedId, house_id: i8, dispatch_eligible: bool) -> Self {
        Self {
            owner,
            house_id,
            dispatch_eligible,
        }
    }
}

/// Local issue queue and synchronized received-command list.
///
/// `VecDeque` preserves the two native FIFO scans while admission methods
/// enforce the retail OutList and DoList capacities.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SynchronizedCommandQueue {
    local: VecDeque<SynchronizedCommand>,
    do_list: VecDeque<SynchronizedCommand>,
    retime_window_start: i32,
    retime_window_end: i32,
}

impl SynchronizedCommandQueue {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    #[inline]
    pub fn local_len(&self) -> usize {
        self.local.len()
    }

    #[cfg(test)]
    #[inline]
    pub fn do_list_len(&self) -> usize {
        self.do_list.len()
    }

    #[cfg(test)]
    pub fn local_records(&self) -> impl Iterator<Item = &SynchronizedCommand> {
        self.local.iter()
    }

    #[cfg(test)]
    pub fn synchronized_records(&self) -> impl Iterator<Item = &SynchronizedCommand> {
        self.do_list.iter()
    }

    /// Publish the persistent DoList retiming window produced by an executed
    /// network timing-update event.
    ///
    /// The returned value is the MaxAhead value after the native scenario-bit
    /// adjustment, which is the value the session owner must commit. A timing
    /// update that widens neither unsigned negotiated value clears the window.
    ///
    /// VERIFIED: gamemd.exe `EventClass__Execute @ 0x004C7600` case `0x20`
    /// publishes the adjusted window consumed by `FUN_0064C380 @ 0x0064C380`.
    #[cfg(test)]
    pub fn apply_timing_update(
        &mut self,
        event_frame: i32,
        previous_max_ahead: u16,
        previous_frame_send_rate: NonZeroU8,
        proposed_max_ahead: u16,
        proposed_frame_send_rate: NonZeroU8,
        scenario_subtracts_ten: bool,
    ) -> u16 {
        let adjusted_max_ahead = if scenario_subtracts_ten {
            proposed_max_ahead.wrapping_sub(10)
        } else {
            proposed_max_ahead
        };

        if adjusted_max_ahead > previous_max_ahead
            || proposed_frame_send_rate > previous_frame_send_rate
        {
            let rate = i32::from(proposed_frame_send_rate.get());
            self.retime_window_start = event_frame;
            self.retime_window_end = event_frame
                .wrapping_add(i32::from(adjusted_max_ahead))
                .wrapping_add(rate.wrapping_sub(1))
                .wrapping_div(rate)
                .wrapping_mul(rate);
        } else {
            self.retime_window_start = 0;
            self.retime_window_end = 0;
        }

        adjusted_max_ahead
    }

    /// Append one locally issued command in issue order. Retail silently drops
    /// the 129th record while the 128-slot OutList is full.
    pub fn issue(&mut self, command: SynchronizedCommand) -> bool {
        if self.local.len() >= OUT_LIST_CAPACITY {
            return false;
        }
        self.local.push_back(command);
        true
    }

    /// Admit one received/replay command in arrival order. A record received
    /// while the native DoList's 0x4000-entry admission cap is full is consumed
    /// but not retained.
    pub fn admit(&mut self, mut command: SynchronizedCommand) -> bool {
        if self.do_list.len() >= DO_LIST_CAPACITY {
            return false;
        }
        command.record.clear_processed();
        self.do_list.push_back(command);
        true
    }

    /// Admit an unknown fixed-width record without interpreting its payload.
    #[cfg(test)]
    pub fn admit_bytes(&mut self, bytes: &[u8]) -> Result<bool, CommandRecordError> {
        let record = CommandRecord::admit_exact(bytes)?;
        Ok(self.admit(SynchronizedCommand::opaque(record)))
    }

    /// Single-player transfer: preserve the issue-frame and house bytes.
    #[cfg(test)]
    pub fn transfer_single_player(&mut self) -> usize {
        let drained = self.local.len();
        while let Some(mut command) = self.local.pop_front() {
            command.record.clear_processed();
            if self.do_list.len() < DO_LIST_CAPACITY {
                self.do_list.push_back(command);
            }
        }
        drained
    }

    /// Build one logical network-transfer batch.
    ///
    /// The transport owner supplies how many FIFO command records it selected
    /// for this send. This queue layer prepends FRAMEINFO, stamps and mirrors at
    /// most that many records, and enforces the native
    /// `0x2000 - DoListCount` local-mirror allowance. Physical packet sizing,
    /// packet splitting, compression, and extended payload ownership remain
    /// transport-layer work.
    #[cfg(test)]
    pub fn transfer_network(
        &mut self,
        current_frame: u32,
        max_ahead: u32,
        local_house_id: i8,
        policy: NetworkSendPolicy,
        current_checksum: u32,
        timing_word: u16,
        selected_command_limit: usize,
    ) -> Vec<CommandRecord> {
        if !policy.should_send(current_frame) {
            return Vec::new();
        }

        let execute_frame = policy.execute_frame(current_frame, max_ahead);
        let transfer_count = self
            .local
            .len()
            .min(NETWORK_LOCAL_MIRROR_LIMIT.saturating_sub(self.do_list.len()))
            .min(selected_command_limit);
        let mut transfer = Vec::with_capacity(transfer_count + 1);
        transfer.push(
            FrameInfo {
                house_id: local_house_id,
                event_frame: execute_frame as i32,
                checksum: current_checksum,
                timing_word,
                delay: max_ahead as u8,
            }
            .encode(),
        );

        for _ in 0..transfer_count {
            let mut command = self
                .local
                .pop_front()
                .expect("transfer count was bounded by the local queue");
            command.stamp_for_network(local_house_id, execute_frame);
            transfer.push(command.record.clone());
            self.do_list.push_back(command);
        }
        transfer
    }

    /// Dispatch all due offline records in session house-registration order.
    ///
    /// Each house scans the DoList from its current head in arrival order.
    /// `adjust_megamissions` receives the complete staged run once before any
    /// member executes. The execute callback runs before bit 0 is marked.
    /// House ids and native dispatch eligibility are supplied explicitly by
    /// the session owner.
    #[cfg(test)]
    pub fn dispatch_due_offline<A, F>(
        &mut self,
        current_frame: i32,
        houses: &[CommandDispatchHouse],
        adjust_megamissions: A,
        execute: F,
    ) -> DispatchSummary
    where
        A: FnMut(InternedId, &mut [SynchronizedCommand]),
        F: FnMut(InternedId, &SynchronizedCommand, bool),
    {
        self.dispatch_due_inner(
            current_frame,
            houses,
            DispatchMode::Offline,
            |_, _| {},
            adjust_megamissions,
            execute,
        )
        .expect("checksum comparison is disabled")
    }

    /// Multiplayer dispatch. Late non-FRAMEINFO records are diagnosed and
    /// passed to `handle_late_record` in dispatch order before being skipped
    /// without acknowledgement. Expired records are still retired when the
    /// contiguous DoList head reaches them. Current-frame FRAMEINFO is compared
    /// only when the supplied native timing gate is open.
    #[cfg(test)]
    pub fn dispatch_due_network<L, A, F>(
        &mut self,
        current_frame: i32,
        houses: &[CommandDispatchHouse],
        checksum_history: &MultiplayerChecksumHistory,
        frame_info_gate: FrameInfoCompareGate,
        handle_late_record: L,
        adjust_megamissions: A,
        execute: F,
    ) -> Result<DispatchSummary, ChecksumMismatch>
    where
        L: FnMut(InternedId, &SynchronizedCommand),
        A: FnMut(InternedId, &mut [SynchronizedCommand]),
        F: FnMut(InternedId, &SynchronizedCommand, bool),
    {
        self.dispatch_due_inner(
            current_frame,
            houses,
            DispatchMode::Network {
                checksum_history,
                frame_info_gate,
            },
            handle_late_record,
            adjust_megamissions,
            execute,
        )
    }

    #[cfg(test)]
    fn dispatch_due_inner<L, A, F>(
        &mut self,
        current_frame: i32,
        houses: &[CommandDispatchHouse],
        mode: DispatchMode<'_>,
        mut handle_late_record: L,
        mut adjust_megamissions: A,
        mut execute: F,
    ) -> Result<DispatchSummary, ChecksumMismatch>
    where
        L: FnMut(InternedId, &SynchronizedCommand),
        A: FnMut(InternedId, &mut [SynchronizedCommand]),
        F: FnMut(InternedId, &SynchronizedCommand, bool),
    {
        let mut summary = DispatchSummary::default();
        let scan_len = self.do_list.len();

        // VERIFIED: gamemd.exe `FUN_0064C380 @ 0x0064C380` scans the complete
        // entry-count snapshot before house eligibility and keeps this window
        // active until a later timing update replaces or clears it.
        for record_index in 0..scan_len {
            let command = self
                .do_list
                .get_mut(record_index)
                .expect("retime scan is bounded by the DoList snapshot");
            let frame = command.record.frame_stamp();
            if command.record.opcode() != FRAMEINFO_EVENT_OPCODE
                && self.retime_window_start < frame
                && frame < self.retime_window_end
            {
                command.record.set_frame_stamp(self.retime_window_end);
            }
        }

        for house in houses.iter().copied() {
            if !house.dispatch_eligible {
                continue;
            }
            let owner = house.owner;
            let house_id = house.house_id;
            let mut staged_megamissions = Vec::new();
            let mut staged_late = Vec::new();
            for record_index in 0..scan_len {
                let Some(command) = self.do_list.get_mut(record_index) else {
                    break;
                };
                if command.record.house_id() != house_id
                    || command.record.frame_stamp() > current_frame
                    || command.record.is_processed()
                {
                    continue;
                }

                let is_late = command.record.frame_stamp() < current_frame;
                if is_late
                    && command.record.opcode() != FRAMEINFO_EVENT_OPCODE
                    && matches!(mode, DispatchMode::Network { .. })
                {
                    handle_late_record(owner, command);
                    summary.late_skipped += 1;
                    continue;
                }
                match command.record.opcode() {
                    FRAMEINFO_EVENT_OPCODE => {
                        if command.record.frame_stamp() == current_frame {
                            if let DispatchMode::Network {
                                checksum_history,
                                frame_info_gate,
                            } = mode
                            {
                                if !frame_info_gate.allows_compare(current_frame) {
                                    summary.timing_consumed += 1;
                                    command.record.mark_processed();
                                    continue;
                                }
                                let frame_info = FrameInfo::decode(&command.record)
                                    .expect("opcode was checked above");
                                checksum_history.compare(frame_info)?;
                                summary.frame_info_compared += 1;
                            }
                        }
                        summary.timing_consumed += 1;
                    }
                    opcode if queue_consumes_without_execute(opcode) => {}
                    _ if command.stages_as_megamission() => {
                        if staged_megamissions.len() < MEGAMISSION_STAGE_CAPACITY {
                            staged_megamissions.push(command.clone());
                            staged_late.push(is_late);
                        } else {
                            summary.megamission_dropped += 1;
                        }
                    }
                    _ => {
                        execute(owner, command, is_late);
                        summary.executed += 1;
                        summary.late_executed += usize::from(is_late);
                    }
                }
                command.record.mark_processed();
            }
            if !staged_megamissions.is_empty() {
                adjust_megamissions(owner, &mut staged_megamissions);
            }
            for (command, is_late) in staged_megamissions.into_iter().zip(staged_late) {
                execute(owner, &command, is_late);
                summary.executed += 1;
                summary.late_executed += usize::from(is_late);
            }
        }

        while self.do_list.front().is_some_and(|command| {
            command.record.is_processed() || command.record.frame_stamp() < current_frame
        }) {
            self.do_list.pop_front();
            summary.retired += 1;
        }
        Ok(summary)
    }
}

/// Builds one byte-exact local issue record. Network delay is deliberately not
/// applied here; transfer owns stamping.
#[derive(Debug, Clone, Copy, Default)]
pub struct LockstepScheduler;

impl LockstepScheduler {
    pub fn new() -> Self {
        Self
    }

    pub fn issue(
        &self,
        issue_frame: u32,
        house_id: i32,
        opcode: u8,
        encoded_payload: &[u8],
    ) -> Result<SynchronizedCommand, CommandRecordError> {
        let record = CommandRecord::encode(opcode, house_id, issue_frame as i32, encoded_payload)?;
        Ok(SynchronizedCommand::opaque(record))
    }
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU8, NonZeroU32};

    use super::{
        CommandDispatchHouse, DO_LIST_CAPACITY, FrameInfo, FrameInfoCompareGate, LockstepScheduler,
        MEGAMISSION_STAGE_CAPACITY, MultiplayerChecksumHistory, NETWORK_LOCAL_MIRROR_LIMIT,
        NetworkSendPolicy, OUT_LIST_CAPACITY, SynchronizedCommand, SynchronizedCommandQueue,
    };
    use crate::sim::command::{COMMAND_RECORD_LEN, CommandRecord};
    use crate::sim::intern::InternedId;

    fn opaque(opcode: u8, house: i32, frame: i32, payload: &[u8]) -> SynchronizedCommand {
        SynchronizedCommand::opaque(CommandRecord::encode(opcode, house, frame, payload).unwrap())
    }

    fn eligible(owner: InternedId, house_id: i8) -> CommandDispatchHouse {
        CommandDispatchHouse::new(owner, house_id, true)
    }

    fn nonzero_u8(value: u8) -> NonZeroU8 {
        NonZeroU8::new(value).unwrap()
    }

    #[test]
    fn synchronized_queue_roundtrip_preserves_bytes_order_and_dispatch_state() {
        let owner = InternedId::from_index(3);
        let mut queue = SynchronizedCommandQueue::new();

        let mut opaque_bytes = [0xa5; COMMAND_RECORD_LEN];
        opaque_bytes[0] = 0xfe;
        opaque_bytes[1] = 0x80;
        opaque_bytes[2] = 2;
        opaque_bytes[3..7].copy_from_slice(&17_i32.to_le_bytes());
        queue.issue(SynchronizedCommand::opaque(
            CommandRecord::decode_exact(&opaque_bytes).unwrap(),
        ));
        queue.issue(SynchronizedCommand::opaque(
            CommandRecord::encode(0x15, 0, 18, &[7, 8]).unwrap(),
        ));

        queue.admit(opaque(0x15, 0, 12, &[1]));
        queue.admit(opaque(0x15, 0, 10, &[2]));
        queue.dispatch_due_offline(10, &[eligible(owner, 0)], |_, _| {}, |_, _, _| {});

        let encoded = bincode::serialize(&queue).unwrap();
        let restored: SynchronizedCommandQueue = bincode::deserialize(&encoded).unwrap();

        assert_eq!(restored.local_len(), 2);
        assert_eq!(restored.do_list_len(), 2);
        assert_eq!(
            restored.local_records().next().unwrap().record().as_bytes(),
            &opaque_bytes
        );
        let encoded = restored.local_records().nth(1).unwrap();
        assert_eq!(&encoded.record().payload()[..2], &[7, 8]);
        assert_eq!(
            restored
                .synchronized_records()
                .map(|command| command.record().is_processed())
                .collect::<Vec<_>>(),
            vec![false, true]
        );
    }

    #[test]
    fn gsi_16_02_retime_prepass_scans_the_complete_snapshot_and_persists() {
        let owner = InternedId::from_index(3);
        let mut queue = SynchronizedCommandQueue::new();
        for (opcode, frame, payload) in [
            (0x15, 100, 0),
            (0x15, 101, 1),
            (super::FRAMEINFO_EVENT_OPCODE, 110, 2),
            (0x15, 119, 3),
            (0x15, 120, 4),
            (0x15, 130, 5),
        ] {
            assert!(queue.admit(opaque(opcode, 0, frame, &[payload])));
        }
        queue.do_list[3].record.mark_processed();
        let unchanged_parts = queue
            .synchronized_records()
            .map(|command| {
                (
                    command.record().opcode(),
                    command.record().flags(),
                    command.record().house_id(),
                    command.record().payload().to_vec(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            queue.apply_timing_update(100, 10, nonzero_u8(5), 20, nonzero_u8(10), false,),
            20
        );
        let encoded = bincode::serialize(&queue).unwrap();
        let mut restored: SynchronizedCommandQueue = bincode::deserialize(&encoded).unwrap();

        let ineligible_house = CommandDispatchHouse::new(owner, 0, false);
        let history = MultiplayerChecksumHistory::new();
        restored
            .dispatch_due_network(
                99,
                &[ineligible_house],
                &history,
                FrameInfoCompareGate::OPEN,
                |_, _| panic!("no record is late"),
                |_, _| panic!("no MegaMission is due"),
                |_, _, _| panic!("house eligibility is checked after retiming"),
            )
            .unwrap();

        assert_eq!(
            restored
                .synchronized_records()
                .map(|command| command.record().frame_stamp())
                .collect::<Vec<_>>(),
            vec![100, 120, 110, 120, 120, 130]
        );
        assert_eq!(
            restored
                .synchronized_records()
                .map(|command| {
                    (
                        command.record().opcode(),
                        command.record().flags(),
                        command.record().house_id(),
                        command.record().payload().to_vec(),
                    )
                })
                .collect::<Vec<_>>(),
            unchanged_parts
        );
        assert!(
            restored
                .synchronized_records()
                .nth(3)
                .unwrap()
                .record()
                .is_processed(),
            "processed records are retimed without clearing their flag"
        );

        assert!(restored.admit(opaque(0x15, 0, 115, &[6])));
        restored
            .dispatch_due_network(
                99,
                &[ineligible_house],
                &history,
                FrameInfoCompareGate::OPEN,
                |_, _| {},
                |_, _| {},
                |_, _, _| {},
            )
            .unwrap();
        assert_eq!(
            restored
                .synchronized_records()
                .last()
                .unwrap()
                .record()
                .frame_stamp(),
            120,
            "the retime window is not consumed by a dispatch"
        );
    }

    #[test]
    fn gsi_16_02_timing_update_applies_scenario_adjustment_and_clears_on_nonincrease() {
        let mut queue = SynchronizedCommandQueue::new();
        queue.retime_window_start = 1;
        queue.retime_window_end = 2;

        assert_eq!(
            queue.apply_timing_update(100, 15, nonzero_u8(5), 20, nonzero_u8(5), true,),
            10
        );
        assert_eq!((queue.retime_window_start, queue.retime_window_end), (0, 0));
    }

    #[test]
    fn gsi_16_02_executed_update_affects_the_next_dispatch_prepass() {
        let owner = InternedId::from_index(3);
        let history = MultiplayerChecksumHistory::new();
        let mut queue = SynchronizedCommandQueue::new();
        assert!(queue.admit(opaque(0x20, 0, 100, &[0])));
        assert!(queue.admit(opaque(0x15, 0, 110, &[1])));

        let mut saw_timing_update = false;
        queue
            .dispatch_due_network(
                100,
                &[eligible(owner, 0)],
                &history,
                FrameInfoCompareGate::OPEN,
                |_, _| panic!("no record is late"),
                |_, _| panic!("no MegaMission is due"),
                |_, command, _| {
                    assert_eq!(command.record().opcode(), 0x20);
                    saw_timing_update = true;
                },
            )
            .unwrap();
        assert!(saw_timing_update);
        assert_eq!(
            queue
                .synchronized_records()
                .next()
                .unwrap()
                .record()
                .frame_stamp(),
            110,
            "the update is published after its dispatch call's prepass"
        );

        queue.apply_timing_update(100, 10, nonzero_u8(5), 20, nonzero_u8(10), false);
        let summary = queue
            .dispatch_due_network(
                110,
                &[eligible(owner, 0)],
                &history,
                FrameInfoCompareGate::OPEN,
                |_, _| panic!("the command must be retimed before lateness handling"),
                |_, _| panic!("no MegaMission is due"),
                |_, _, _| panic!("the command must not execute inside the widened window"),
            )
            .unwrap();
        assert_eq!(summary.executed, 0);
        assert_eq!(
            queue
                .synchronized_records()
                .next()
                .unwrap()
                .record()
                .frame_stamp(),
            120
        );

        let mut executed = Vec::new();
        let summary = queue
            .dispatch_due_network(
                120,
                &[eligible(owner, 0)],
                &history,
                FrameInfoCompareGate::OPEN,
                |_, _| panic!("the retimed command is not late"),
                |_, _| {},
                |_, command, _| executed.push(command.record().payload()[0]),
            )
            .unwrap();
        assert_eq!(summary.executed, 1);
        assert_eq!(executed, vec![1]);
    }

    #[test]
    fn single_player_preserves_issue_frame_and_dispatches_house_then_fifo_order() {
        let owner_a = InternedId::from_index(9);
        let owner_b = InternedId::from_index(2);
        let scheduler = LockstepScheduler::new();
        let mut queue = SynchronizedCommandQueue::new();

        queue.issue(scheduler.issue(10, 1, 0x15, &[0xb1]).unwrap());
        queue.issue(scheduler.issue(10, 0, 0x15, &[0xa1]).unwrap());
        queue.issue(scheduler.issue(10, 1, 0x15, &[0xb2]).unwrap());

        assert_eq!(queue.transfer_single_player(), 3);
        let mut executed = Vec::new();
        let summary = queue.dispatch_due_offline(
            10,
            &[eligible(owner_a, 0), eligible(owner_b, 1)],
            |_, _| {},
            |owner, command, late| {
                executed.push((
                    owner,
                    command.record().payload()[0],
                    command.record().frame_stamp(),
                    late,
                ));
            },
        );

        assert_eq!(
            executed,
            vec![
                (owner_a, 0xa1, 10, false),
                (owner_b, 0xb1, 10, false),
                (owner_b, 0xb2, 10, false),
            ]
        );
        assert_eq!(summary.executed, 3);
        assert_eq!(summary.retired, 3);
        assert_eq!(queue.do_list_len(), 0);
    }

    #[test]
    fn network_stamps_all_staged_commands_from_the_send_frame() {
        let scheduler = LockstepScheduler::new();
        let mut queue = SynchronizedCommandQueue::new();
        for issue_frame in [100, 101] {
            queue.issue(
                scheduler
                    .issue(issue_frame, 0, 0x15, &[issue_frame as u8])
                    .unwrap(),
            );
        }

        let outgoing = queue.transfer_network(
            110,
            15,
            2,
            NetworkSendPolicy::EveryFrame,
            0x1122_3344,
            0xabcd,
            2,
        );
        assert_eq!(outgoing.len(), 3);
        assert_eq!(
            FrameInfo::decode(&outgoing[0]),
            Some(FrameInfo {
                house_id: 2,
                event_frame: 125,
                checksum: 0x1122_3344,
                timing_word: 0xabcd,
                delay: 15,
            })
        );
        assert!(
            outgoing
                .iter()
                .skip(1)
                .all(|record| record.frame_stamp() == 125 && record.house_id() == 2)
        );
        assert_eq!(
            queue
                .synchronized_records()
                .map(|command| command.record().frame_stamp())
                .collect::<Vec<_>>(),
            vec![125, 125]
        );
    }

    #[test]
    fn network_send_boundary_emits_frame_info_without_local_commands() {
        let mut queue = SynchronizedCommandQueue::new();

        let outgoing =
            queue.transfer_network(20, 7, 1, NetworkSendPolicy::EveryFrame, 0xaabb_ccdd, 9, 0);

        assert_eq!(outgoing.len(), 1);
        assert_eq!(
            FrameInfo::decode(&outgoing[0]),
            Some(FrameInfo {
                house_id: 1,
                event_frame: 27,
                checksum: 0xaabb_ccdd,
                timing_word: 9,
                delay: 7,
            })
        );
    }

    #[test]
    fn logical_transfer_limit_zero_emits_frame_info_without_selecting_commands() {
        let mut queue = SynchronizedCommandQueue::new();
        assert!(queue.issue(opaque(0x15, 0, 20, &[1])));

        let frame_only = queue.transfer_network(20, 7, 0, NetworkSendPolicy::EveryFrame, 0, 0, 0);
        assert_eq!(frame_only.len(), 1);
        assert_eq!(frame_only[0].opcode(), super::FRAMEINFO_EVENT_OPCODE);
        assert_eq!(queue.local_len(), 1);
        assert_eq!(queue.do_list_len(), 0);

        let selected = queue.transfer_network(20, 7, 0, NetworkSendPolicy::EveryFrame, 0, 0, 1);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[1].payload()[0], 1);
        assert_eq!(queue.local_len(), 0);
        assert_eq!(queue.do_list_len(), 1);
    }

    #[test]
    fn logical_transfer_limit_selects_fifo_records_only() {
        let mut queue = SynchronizedCommandQueue::new();
        assert!(queue.issue(opaque(0x15, 0, 100, &[1])));
        assert!(queue.issue(opaque(0x15, 0, 101, &[2])));

        let first = queue.transfer_network(110, 15, 3, NetworkSendPolicy::EveryFrame, 0x1234, 9, 1);
        assert_eq!(first.len(), 2);
        assert_eq!(first[1].payload()[0], 1);
        assert_eq!(first[1].frame_stamp(), 125);
        assert_eq!(first[1].house_id(), 3);
        assert_eq!(queue.local_len(), 1);
        assert_eq!(queue.do_list_len(), 1);

        let second =
            queue.transfer_network(111, 15, 3, NetworkSendPolicy::EveryFrame, 0x1234, 9, 1);
        assert_eq!(second.len(), 2);
        assert_eq!(second[1].payload()[0], 2);
        assert_eq!(second[1].frame_stamp(), 126);
        assert_eq!(second[1].house_id(), 3);
        assert_eq!(queue.local_len(), 0);
        assert_eq!(queue.do_list_len(), 2);
    }

    #[test]
    fn frame_send_rate_two_waits_and_uses_unsigned_boundary_rounding() {
        let policy = NetworkSendPolicy::FrameSendRate2 {
            frame_send_rate: NonZeroU32::new(5).unwrap(),
        };
        let mut queue = SynchronizedCommandQueue::new();
        queue.issue(opaque(0x15, 0, 11, &[7]));

        assert!(!policy.should_send(11));
        assert!(
            queue
                .transfer_network(11, 7, 0, policy, 0x55aa, 3, 1)
                .is_empty()
        );
        assert_eq!(queue.local_len(), 1);

        assert!(policy.should_send(15));
        let outgoing = queue.transfer_network(15, 7, 0, policy, 0x55aa, 3, 1);
        assert_eq!(outgoing[0].opcode(), super::FRAMEINFO_EVENT_OPCODE);
        assert_eq!(outgoing[0].frame_stamp(), 25);
        assert_eq!(outgoing[1].frame_stamp(), 25);
        assert_eq!(policy.execute_frame(15, 0), 15);
        assert_eq!(
            NetworkSendPolicy::EveryFrame.execute_frame(u32::MAX - 2, 5),
            2
        );
    }

    #[test]
    fn network_dispatch_skips_late_non_timing_records_and_retires_reachable_head() {
        let owner = InternedId::from_index(7);
        let history = MultiplayerChecksumHistory::new();
        let mut queue = SynchronizedCommandQueue::new();
        assert!(queue.admit(opaque(0x15, 0, 10, &[1])));
        assert!(queue.admit(opaque(0x15, 0, 9, &[2])));

        let mut observed = Vec::new();
        let mut late_records = Vec::new();
        let summary = queue
            .dispatch_due_network(
                10,
                &[eligible(owner, 0)],
                &history,
                FrameInfoCompareGate::OPEN,
                |late_owner, command| {
                    assert!(!command.record().is_processed());
                    late_records.push((late_owner, command.record().payload()[0]));
                },
                |_, _| {},
                |_, command, late| observed.push((command.record().payload()[0], late)),
            )
            .unwrap();

        assert_eq!(observed, vec![(1, false)]);
        assert_eq!(late_records, vec![(owner, 2)]);
        assert_eq!(summary.executed, 1);
        assert_eq!(summary.late_executed, 0);
        assert_eq!(summary.late_skipped, 1);
        assert_eq!(summary.retired, 2);
        assert_eq!(queue.do_list_len(), 0);
    }

    #[test]
    fn skipped_late_network_record_stays_unmarked_behind_a_future_head() {
        let owner = InternedId::from_index(7);
        let history = MultiplayerChecksumHistory::new();
        let mut queue = SynchronizedCommandQueue::new();
        assert!(queue.admit(opaque(0x15, 0, 11, &[1])));
        assert!(queue.admit(opaque(0x15, 0, 9, &[2])));

        let summary = queue
            .dispatch_due_network(
                10,
                &[eligible(owner, 0)],
                &history,
                FrameInfoCompareGate::OPEN,
                |_, command| assert!(!command.record().is_processed()),
                |_, _| {},
                |_, _, _| panic!("late network record must not execute"),
            )
            .unwrap();

        assert_eq!(summary.executed, 0);
        assert_eq!(summary.late_executed, 0);
        assert_eq!(summary.late_skipped, 1);
        assert_eq!(summary.retired, 0);
        assert_eq!(queue.do_list_len(), 2);
        assert!(
            !queue
                .synchronized_records()
                .nth(1)
                .unwrap()
                .record()
                .is_processed()
        );
    }

    #[test]
    fn late_network_transport_hook_follows_house_then_fifo_order() {
        let owner_a = InternedId::from_index(7);
        let owner_b = InternedId::from_index(8);
        let history = MultiplayerChecksumHistory::new();
        let mut queue = SynchronizedCommandQueue::new();
        assert!(queue.admit(opaque(0x15, 0, 11, &[0])));
        assert!(queue.admit(opaque(0x15, 1, 9, &[0xb1])));
        assert!(queue.admit(opaque(0x15, 0, 9, &[0xa1])));
        assert!(queue.admit(opaque(0x15, 1, 9, &[0xb2])));
        assert!(queue.admit(opaque(0x15, 0, 9, &[0xa2])));

        let mut late_order = Vec::new();
        let summary = queue
            .dispatch_due_network(
                10,
                &[eligible(owner_a, 0), eligible(owner_b, 1)],
                &history,
                FrameInfoCompareGate::OPEN,
                |owner, command| {
                    assert!(!command.record().is_processed());
                    late_order.push((owner, command.record().payload()[0]));
                },
                |_, _| {},
                |_, _, _| panic!("late network record must not execute"),
            )
            .unwrap();

        assert_eq!(
            late_order,
            vec![
                (owner_a, 0xa1),
                (owner_a, 0xa2),
                (owner_b, 0xb1),
                (owner_b, 0xb2),
            ]
        );
        assert_eq!(summary.late_skipped, 4);
        assert_eq!(summary.retired, 0);
        assert!(
            queue
                .synchronized_records()
                .skip(1)
                .all(|command| !command.record().is_processed())
        );
    }

    #[test]
    fn due_scan_executes_late_unknown_bytes_consumes_timing_and_retains_future() {
        let owner = InternedId::from_index(7);
        let mut queue = SynchronizedCommandQueue::new();

        let mut late_bytes = [0x7b; COMMAND_RECORD_LEN];
        late_bytes[0] = 0xfe;
        late_bytes[1] = 0x80;
        late_bytes[2] = 0;
        late_bytes[3..7].copy_from_slice(&9_i32.to_le_bytes());
        queue.admit_bytes(&late_bytes).unwrap();
        queue.admit(opaque(0x1c, 0, 10, &[1, 2]));
        queue.admit(opaque(0x15, 0, 11, &[3, 4]));

        let mut observed = Vec::new();
        let summary = queue.dispatch_due_offline(
            10,
            &[eligible(owner, 0)],
            |_, _| {},
            |registered_owner, command, late| {
                observed.push((
                    registered_owner,
                    command.record().clone().into_bytes(),
                    late,
                    command.record().is_processed(),
                ));
            },
        );

        assert_eq!(observed.len(), 1);
        assert_eq!(observed[0].0, owner);
        assert_eq!(observed[0].1[0], 0xfe);
        assert_eq!(observed[0].1[1], 0x80);
        assert!(observed[0].1[7..].iter().all(|byte| *byte == 0x7b));
        assert!(observed[0].2);
        assert!(!observed[0].3, "bit 0 is marked after execution");
        assert_eq!(summary.executed, 1);
        assert_eq!(summary.late_executed, 1);
        assert_eq!(summary.timing_consumed, 1);
        assert_eq!(summary.retired, 2);
        assert_eq!(queue.do_list_len(), 1);
        assert_eq!(
            queue
                .synchronized_records()
                .next()
                .unwrap()
                .record()
                .frame_stamp(),
            11
        );
    }

    #[test]
    fn processed_records_behind_a_future_head_stay_marked_until_head_can_retire() {
        let owner = InternedId::from_index(1);
        let mut queue = SynchronizedCommandQueue::new();
        queue.admit(opaque(0x15, 0, 12, &[]));
        queue.admit(opaque(0x15, 0, 10, &[]));

        let summary =
            queue.dispatch_due_offline(10, &[eligible(owner, 0)], |_, _| {}, |_, _, _| {});
        assert_eq!(summary.executed, 1);
        assert_eq!(summary.retired, 0);
        assert!(
            !queue
                .synchronized_records()
                .next()
                .unwrap()
                .record()
                .is_processed()
        );
        assert!(
            queue
                .synchronized_records()
                .nth(1)
                .unwrap()
                .record()
                .is_processed()
        );
    }

    fn assert_queue_only_opcode_is_marked_without_execute(opcode: u8) {
        let owner = InternedId::from_index(1);
        let mut queue = SynchronizedCommandQueue::new();
        queue.admit(opaque(0x15, 0, 11, &[]));
        queue.admit(opaque(opcode, 0, 10, &[1]));

        let summary = queue.dispatch_due_offline(
            10,
            &[eligible(owner, 0)],
            |_, _| panic!("queue-only records are not MegaMissions"),
            |_, _, _| panic!("queue-only records must not reach EventClass execute"),
        );

        assert_eq!(summary.executed, 0);
        assert_eq!(summary.retired, 0);
        assert!(
            queue
                .synchronized_records()
                .nth(1)
                .unwrap()
                .record()
                .is_processed()
        );
    }

    #[test]
    fn opcode_0c_is_marked_without_event_execute() {
        assert_queue_only_opcode_is_marked_without_execute(0x0c);
    }

    #[test]
    fn opcode_22_is_marked_without_event_execute() {
        assert_queue_only_opcode_is_marked_without_execute(0x22);
    }

    #[test]
    fn megamission_batch_hook_sees_ordered_run_and_mutations_reach_execute() {
        let owner = InternedId::from_index(5);
        let mut queue = SynchronizedCommandQueue::new();
        queue.admit(opaque(0x04, 0, 10, &[1]));
        queue.admit(opaque(0x15, 0, 10, &[2]));
        queue.admit(opaque(0x04, 0, 10, &[3]));

        let mut order = Vec::new();
        let mut batches = Vec::new();
        let summary = queue.dispatch_due_offline(
            10,
            &[eligible(owner, 0)],
            |batch_owner, batch| {
                batches.push((
                    batch_owner,
                    batch
                        .iter()
                        .map(|command| command.record().payload()[0])
                        .collect::<Vec<_>>(),
                ));
                batch[0].payload_mut()[0] = 10;
                batch[1].payload_mut()[0] = 30;
            },
            |_, command, _| {
                order.push((command.record().opcode(), command.record().payload()[0]));
                assert!(
                    !command.record().is_processed(),
                    "the staged copy retains its pre-acknowledgement flags"
                );
            },
        );

        assert_eq!(batches, vec![(owner, vec![1, 3])]);
        assert_eq!(order, vec![(0x15, 2), (0x04, 10), (0x04, 30)]);
        assert_eq!(summary.executed, 3);
        assert_eq!(summary.retired, 3);
    }

    #[test]
    fn megamission_staging_drops_the_257th_due_record() {
        let owner = InternedId::from_index(5);
        let mut queue = SynchronizedCommandQueue::new();
        for payload in 0..=MEGAMISSION_STAGE_CAPACITY {
            assert!(queue.admit(opaque(0x04, 0, 10, &[payload as u8])));
        }

        let mut executed = 0;
        let summary = queue.dispatch_due_offline(
            10,
            &[eligible(owner, 0)],
            |_, batch| assert_eq!(batch.len(), MEGAMISSION_STAGE_CAPACITY),
            |_, _, _| executed += 1,
        );

        assert_eq!(executed, MEGAMISSION_STAGE_CAPACITY);
        assert_eq!(summary.executed, MEGAMISSION_STAGE_CAPACITY);
        assert_eq!(summary.megamission_dropped, 1);
        assert_eq!(summary.retired, MEGAMISSION_STAGE_CAPACITY + 1);
    }

    #[test]
    fn native_queue_capacities_drop_overflow_and_offline_transfer_drains_it() {
        let mut queue = SynchronizedCommandQueue::new();
        for payload in 0..OUT_LIST_CAPACITY {
            assert!(queue.issue(opaque(0x15, 0, 10, &[payload as u8])));
        }
        assert!(!queue.issue(opaque(0x15, 0, 10, &[0xff])));
        assert_eq!(queue.local_len(), OUT_LIST_CAPACITY);

        for payload in 0..DO_LIST_CAPACITY {
            assert!(queue.admit(opaque(0x15, 0, 11, &[payload as u8])));
        }
        assert!(!queue.admit(opaque(0x15, 0, 11, &[0xff])));
        assert_eq!(queue.do_list_len(), DO_LIST_CAPACITY);

        assert_eq!(queue.transfer_single_player(), OUT_LIST_CAPACITY);
        assert_eq!(queue.local_len(), 0);
        assert_eq!(queue.do_list_len(), DO_LIST_CAPACITY);
    }

    #[test]
    fn network_transfer_stops_at_the_initial_local_mirror_allowance() {
        let mut queue = SynchronizedCommandQueue::new();
        for payload in 0..NETWORK_LOCAL_MIRROR_LIMIT - 1 {
            assert!(queue.admit(opaque(0x15, 0, 11, &[payload as u8])));
        }
        assert!(queue.issue(opaque(0x15, 0, 10, &[1])));
        assert!(queue.issue(opaque(0x15, 0, 10, &[2])));

        let transfer = queue.transfer_network(10, 5, 0, NetworkSendPolicy::EveryFrame, 0, 0, 2);

        assert_eq!(transfer.len(), 2);
        assert_eq!(transfer[1].payload()[0], 1);
        assert_eq!(queue.local_len(), 1);
        assert_eq!(queue.do_list_len(), NETWORK_LOCAL_MIRROR_LIMIT);
    }

    #[test]
    fn ineligible_ai_house_is_skipped_without_reindexing_later_house_ids() {
        let owner_0 = InternedId::from_index(10);
        let owner_1 = InternedId::from_index(11);
        let owner_2 = InternedId::from_index(12);
        let mut queue = SynchronizedCommandQueue::new();
        assert!(queue.admit(opaque(0x15, 1, 10, &[1])));
        assert!(queue.admit(opaque(0x15, 2, 10, &[2])));

        let houses = [
            CommandDispatchHouse::new(owner_0, 0, true),
            CommandDispatchHouse::new(owner_1, 1, false),
            CommandDispatchHouse::new(owner_2, 2, true),
        ];
        let mut observed = Vec::new();
        let summary = queue.dispatch_due_offline(
            10,
            &houses,
            |_, _| {},
            |owner, command, _| observed.push((owner, command.record().house_id())),
        );

        assert_eq!(observed, vec![(owner_2, 2)]);
        assert_eq!(summary.executed, 1);
        assert_eq!(summary.retired, 0);
        assert!(
            !queue
                .synchronized_records()
                .next()
                .unwrap()
                .record()
                .is_processed()
        );
        assert!(
            queue
                .synchronized_records()
                .nth(1)
                .unwrap()
                .record()
                .is_processed()
        );
    }

    #[test]
    fn scheduler_issues_only_the_supplied_native_record() {
        let scheduler = LockstepScheduler::new();
        let command = scheduler.issue(8, 2, 0x15, &[0x11, 0x22]).unwrap();

        assert_eq!(command.record().opcode(), 0x15);
        assert_eq!(command.record().house_id(), 2);
        assert_eq!(command.record().frame_stamp(), 8);
        assert_eq!(&command.record().payload()[..2], &[0x11, 0x22]);
        assert_eq!(command.into_record().payload()[2], 0);
    }

    #[test]
    fn frame_info_overwrites_only_verified_fields() {
        let original = [0xa4; COMMAND_RECORD_LEN];
        let mut record = CommandRecord::decode_exact(&original).unwrap();
        FrameInfo {
            house_id: 3,
            event_frame: -9,
            checksum: 0x1234_5678,
            timing_word: 0xabcd,
            delay: 0xef,
        }
        .write_to(&mut record);

        let bytes = record.as_bytes();
        assert_eq!(bytes[0], super::FRAMEINFO_EVENT_OPCODE);
        assert_eq!(bytes[1], 0xa4, "flags are not normalized");
        assert_eq!(bytes[2], 3);
        assert_eq!(&bytes[3..7], &(-9_i32).to_le_bytes());
        assert_eq!(&bytes[7..11], &0x1234_5678_u32.to_le_bytes());
        assert_eq!(&bytes[11..13], &0xabcd_u16.to_le_bytes());
        assert_eq!(bytes[13], 0xef);
        assert!(bytes[14..].iter().all(|&byte| byte == 0xa4));
    }

    #[test]
    fn frame_info_compares_delayed_history_with_wrapping_index() {
        let owner = InternedId::from_index(1);
        let mut history = MultiplayerChecksumHistory::new();
        history.record(u32::MAX, 0xdead_beef);
        let mut queue = SynchronizedCommandQueue::new();
        queue.admit(SynchronizedCommand::opaque(
            FrameInfo {
                house_id: 0,
                event_frame: 1,
                checksum: 0xdead_beef,
                timing_word: 0,
                delay: 2,
            }
            .encode(),
        ));

        let summary = queue
            .dispatch_due_network(
                1,
                &[eligible(owner, 0)],
                &history,
                FrameInfoCompareGate::OPEN,
                |_, _| {},
                |_, _| {},
                |_, _, _| panic!("FRAMEINFO must not reach command execution"),
            )
            .unwrap();

        assert_eq!(summary.timing_consumed, 1);
        assert_eq!(summary.frame_info_compared, 1);
        assert_eq!(summary.retired, 1);
    }

    #[test]
    fn closed_frame_info_gate_acknowledges_without_comparing() {
        let owner = InternedId::from_index(1);
        let mut history = MultiplayerChecksumHistory::new();
        history.record(8, 0x1111_1111);
        let mut queue = SynchronizedCommandQueue::new();
        assert!(
            queue.admit(SynchronizedCommand::opaque(
                FrameInfo {
                    house_id: 0,
                    event_frame: 10,
                    checksum: 0x2222_2222,
                    timing_word: 0,
                    delay: 2,
                }
                .encode(),
            ))
        );

        let summary = queue
            .dispatch_due_network(
                10,
                &[eligible(owner, 0)],
                &history,
                FrameInfoCompareGate::new(9, 2),
                |_, _| {},
                |_, _| {},
                |_, _, _| panic!("FRAMEINFO must not reach command execution"),
            )
            .unwrap();

        assert_eq!(summary.timing_consumed, 1);
        assert_eq!(summary.frame_info_compared, 0);
        assert_eq!(summary.retired, 1);
    }

    #[test]
    fn checksum_mismatch_stops_before_acknowledging_frame_info() {
        let owner = InternedId::from_index(1);
        let mut history = MultiplayerChecksumHistory::new();
        history.record(40, 0x1111_1111);
        let mut queue = SynchronizedCommandQueue::new();
        queue.admit(SynchronizedCommand::opaque(
            FrameInfo {
                house_id: 0,
                event_frame: 45,
                checksum: 0x2222_2222,
                timing_word: 0,
                delay: 5,
            }
            .encode(),
        ));

        let mismatch = queue
            .dispatch_due_network(
                45,
                &[eligible(owner, 0)],
                &history,
                FrameInfoCompareGate::OPEN,
                |_, _| {},
                |_, _| {},
                |_, _, _| {},
            )
            .unwrap_err();

        assert_eq!(mismatch.history_index, 40);
        assert_eq!(mismatch.local_checksum, 0x1111_1111);
        assert_eq!(mismatch.remote_checksum, 0x2222_2222);
        assert_eq!(queue.do_list_len(), 1);
        assert!(
            !queue
                .synchronized_records()
                .next()
                .unwrap()
                .record()
                .is_processed()
        );
    }

    #[test]
    fn late_frame_info_is_consumed_without_comparison() {
        let owner = InternedId::from_index(1);
        let history = MultiplayerChecksumHistory::new();
        let mut queue = SynchronizedCommandQueue::new();
        queue.admit(SynchronizedCommand::opaque(
            FrameInfo {
                house_id: 0,
                event_frame: 9,
                checksum: 0xffff_ffff,
                timing_word: 0,
                delay: 0,
            }
            .encode(),
        ));

        let summary = queue
            .dispatch_due_network(
                10,
                &[eligible(owner, 0)],
                &history,
                FrameInfoCompareGate::OPEN,
                |_, _| {},
                |_, _| {},
                |_, _, _| {},
            )
            .unwrap();
        assert_eq!(summary.timing_consumed, 1);
        assert_eq!(summary.frame_info_compared, 0);
        assert_eq!(summary.retired, 1);
    }
}
