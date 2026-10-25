//! Event admission and synchronized dispatch shared by gameplay and byte queues.

mod checksum;
pub use checksum::{
    CHECKSUM_HISTORY_LEN, FRAMEINFO_EVENT_OPCODE, FrameChecksumState, FrameInfo,
    FrameInfoCompareGate, MultiplayerChecksumHistory,
};

mod dispatch;
mod timing;
pub use dispatch::{
    CommandDispatchClock, CommandDispatchHouse, CommandDispatchMode, DO_LIST_CAPACITY,
    DispatchSummary, DoList, DoListEvent, DoListHost, DoListRetimeWindow, NativeCommandOccupancy,
    SemanticEventHeader,
};
pub use timing::NetworkCommandTiming;

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

/// Exact diagnostic facts available when one due FRAMEINFO comparison fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
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

/// Try_Append_Event_To_OutList6521C0's process-wide capacity.
pub const OUT_LIST_CAPACITY: usize = 0x80;

/// Send_OutList649CA0 caps its local mirror at 0x2000 occupied DoList slots.
pub const NETWORK_LOCAL_MIRROR_LIMIT: usize = 0x2000;

/// VERA multiplayer retains a complete 20,000-actor input batch. This bound
/// includes pending records and local input for one authenticated source; the
/// queues allocate only admitted entries. Native/offline adapters keep their
/// original capacities. Reserved control slots keep timing and exit traffic
/// possible while ordinary player input is applying backpressure.
pub const NETWORK_SOURCE_COMMAND_CAPACITY: usize = 0x8000;
pub const NETWORK_CONTROL_RESERVE: usize = OUT_LIST_CAPACITY;
// The codec admits one FRAMEINFO per data packet. Even a one-record packet
// therefore fits without silently discarding checksum headers at the receiver.
pub const NETWORK_SOURCE_DOLIST_CAPACITY: usize = 2 * NETWORK_SOURCE_COMMAND_CAPACITY;
pub const NETWORK_COMMAND_CAPACITY: usize =
    NETWORK_SOURCE_DOLIST_CAPACITY * crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT;

/// Native events consume an OutList slot. Local-only Rust extensions share
/// issue order with them without pretending to be native EventClass records.
pub trait LocalCommandEvent {
    fn consumes_native_outlist_slot(&self) -> bool;
}

/// One authoritative local FIFO. Header construction belongs to its producer;
/// this owner preserves the admitted item until the transfer boundary removes
/// it. Raw queues contain only native events. Gameplay can also retain local
/// extensions in this order without charging them a native slot.
///
/// Original gamemd.exe: Try_Append_Event_To_OutList6521C0 accepts fewer than
/// 128 records and returns false without insertion when full. Offline transfer
/// 6474DC..647586 drains every item even when DoList refuses it. Network
/// transfer selects a prefix instead; the caller supplies that policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocalCommandStage<T, const CAPACITY: usize = OUT_LIST_CAPACITY> {
    entries: VecDeque<T>,
    #[serde(skip)]
    native_count: usize,
}

impl<'de, T: Deserialize<'de> + LocalCommandEvent, const CAPACITY: usize> Deserialize<'de>
    for LocalCommandStage<T, CAPACITY>
{
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Stored<T> {
            entries: VecDeque<T>,
        }
        let stored = Stored::<T>::deserialize(deserializer)?;
        let native_count = stored
            .entries
            .iter()
            .filter(|entry| entry.consumes_native_outlist_slot())
            .take(CAPACITY + 1)
            .count();
        if native_count > CAPACITY {
            return Err(serde::de::Error::custom(
                "saved local event queue exceeds native capacity",
            ));
        }
        Ok(Self {
            entries: stored.entries,
            native_count,
        })
    }
}

impl<T, const CAPACITY: usize> Default for LocalCommandStage<T, CAPACITY> {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            native_count: 0,
        }
    }
}

impl<T, const CAPACITY: usize> LocalCommandStage<T, CAPACITY> {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.entries.iter()
    }
}

impl<T: LocalCommandEvent, const CAPACITY: usize> LocalCommandStage<T, CAPACITY> {
    /// Consume one matching retained event without changing FIFO order.
    pub fn remove_first(&mut self, predicate: impl FnMut(&T) -> bool) -> Option<T> {
        let index = self.entries.iter().position(predicate)?;
        let entry = self.entries.remove(index)?;
        self.native_count -= usize::from(entry.consumes_native_outlist_slot());
        Some(entry)
    }

    /// Remove only the FIFO prefix selected by the transfer owner.
    pub fn take_prefix(&mut self, count: usize) -> impl Iterator<Item = T> + '_ {
        let count = count.min(self.entries.len());
        self.native_count -= self
            .entries
            .iter()
            .take(count)
            .filter(|entry| entry.consumes_native_outlist_slot())
            .count();
        self.entries.drain(..count)
    }
    /// Derived occupancy is updated only by this FIFO owner.
    pub fn native_len(&self) -> usize {
        self.native_count
    }

    pub fn try_push(&mut self, entry: T) -> bool {
        self.try_push_bounded(entry, CAPACITY)
    }

    pub fn try_push_bounded(&mut self, entry: T, limit: usize) -> bool {
        let native = entry.consumes_native_outlist_slot();
        if native && self.native_count >= limit.min(CAPACITY) {
            return false;
        }
        self.entries.push_back(entry);
        self.native_count += usize::from(native);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::intern::InternedId;

    #[test]
    fn restore_rejects_oversized_native_events_but_allows_local_extensions() {
        let event =
            CommandEnvelope::new(InternedId::from_index(0), 0, Command::Stop { entity_id: 1 });
        let malformed = serde_json::json!({"entries": vec![event; OUT_LIST_CAPACITY + 1]});
        assert!(serde_json::from_value::<LocalCommandStage<CommandEnvelope>>(malformed).is_err());
        let local = CommandEnvelope::new(
            InternedId::from_index(0),
            0,
            Command::MoveFlash { rx: 1, ry: 1 },
        );
        let stored = serde_json::json!({"entries": vec![local; OUT_LIST_CAPACITY + 1]});
        let restored =
            serde_json::from_value::<LocalCommandStage<CommandEnvelope>>(stored).unwrap();
        assert_eq!(restored.native_len(), 0);
        assert_eq!(restored.len(), OUT_LIST_CAPACITY + 1);
    }
}
