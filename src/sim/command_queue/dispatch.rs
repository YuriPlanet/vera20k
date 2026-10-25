//! Execute_DoList's ordered scan, staging, acknowledgement and retirement.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::sim::command::{
    CommandRecord, MEGAMISSION_OPCODE, MEGAMISSION_STAGE_CAPACITY, stage_megamission,
};
use crate::sim::intern::InternedId;

pub const DO_LIST_CAPACITY: usize = 0x4000;
const FRAMEINFO_OPCODE: u8 = 0x1c;

/// Derived occupancy of the one DoList, including a temporarily moved scan.
/// Source counts bound authenticated multiplayer traffic without rescanning
/// the complete FIFO for every actor in a large input batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeCommandOccupancy {
    pub total: usize,
    by_house: [usize; 128],
}

impl Default for NativeCommandOccupancy {
    fn default() -> Self {
        Self {
            total: 0,
            by_house: [0; 128],
        }
    }
}

impl NativeCommandOccupancy {
    pub fn for_house(&self, house: i8) -> usize {
        usize::try_from(house)
            .ok()
            .map_or(0, |index| self.by_house[index])
    }

    fn add<T: DoListEvent>(&mut self, entry: &T) {
        if entry.consumes_native_dolist_slot() {
            self.total += 1;
            if let Some(house) = entry.native_record().map(|record| record.house_id())
                && let Ok(index) = usize::try_from(house)
            {
                self.by_house[index] += 1;
            }
        }
    }

    fn remove<T: DoListEvent>(&mut self, entry: &T) {
        if entry.consumes_native_dolist_slot() {
            self.total -= 1;
            if let Some(house) = entry.native_record().map(|record| record.house_id())
                && let Ok(index) = usize::try_from(house)
            {
                self.by_house[index] -= 1;
            }
        }
    }
}

/// The two clocks have different authorities. Native headers always use the
/// signed binary frame; Rust-only commands retain their monotonic schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandDispatchClock {
    pub native_frame: i32,
    pub execute_tick: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandDispatchMode {
    Offline,
    Network,
}

/// Fallback metadata only for an entry with no native record. A native record
/// never consults these values, including when its header cannot be resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticEventHeader {
    pub owner: InternedId,
    pub execute_tick: u64,
    pub opcode: Option<u8>,
}

/// One authoritative representation of a queued event. Implementations keep
/// native processed state in the record; only semantic entries need a separate
/// processed bit. Payload decoding belongs to the execution host, after scan
/// admission and MegaMission staging have already happened.
pub trait DoListEvent: Clone {
    fn native_record(&self) -> Option<&CommandRecord>;
    fn native_record_mut(&mut self) -> Option<&mut CommandRecord>;
    fn semantic_header(&self) -> Option<SemanticEventHeader>;
    fn consumes_native_dolist_slot(&self) -> bool;
    // Header/processed mutations preserve the entry's native-slot category.
    fn is_processed(&self) -> bool;
    fn mark_processed(&mut self);
    fn clear_processed(&mut self);
}

/// A registered slot, without compacting ineligible or missing Houses.
/// The host rechecks eligibility at the start of every House's scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandDispatchHouse {
    pub owner: InternedId,
    pub house_id: i8,
    pub dispatch_eligible: bool,
    pub current_player: bool,
}

impl CommandDispatchHouse {
    pub const fn new(owner: InternedId, house_id: i8, dispatch_eligible: bool) -> Self {
        Self {
            owner,
            house_id,
            dispatch_eligible,
            current_player: false,
        }
    }

    pub const fn with_current_player(mut self, current_player: bool) -> Self {
        self.current_player = current_player;
        self
    }
}

/// Execute_DoList64C380's retained timing window. Execution can publish a new
/// window, but the current scan applies only the window present on entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoListRetimeWindow {
    start: i32,
    end: i32,
}

impl DoListRetimeWindow {
    /// EventClass::Execute4C800C..4C8077 publishes the next scan's window.
    /// Only widening MaxAhead or send rate requires rescheduling. The active
    /// scan still uses its entry snapshot (Execute_DoList64C380).
    pub fn apply_timing_update(
        &mut self,
        event_frame: i32,
        previous_max_ahead: u16,
        previous_frame_send_rate: std::num::NonZeroU8,
        proposed_max_ahead: u16,
        proposed_frame_send_rate: std::num::NonZeroU8,
        scenario_subtracts_ten: bool,
    ) -> u16 {
        let adjusted = if scenario_subtracts_ten {
            proposed_max_ahead.wrapping_sub(10)
        } else {
            proposed_max_ahead
        };
        if adjusted > previous_max_ahead || proposed_frame_send_rate > previous_frame_send_rate {
            let rate = i32::from(proposed_frame_send_rate.get());
            self.set(
                event_frame,
                event_frame
                    .wrapping_add(i32::from(adjusted))
                    .wrapping_add(rate - 1)
                    .wrapping_div(rate)
                    .wrapping_mul(rate),
            );
        } else {
            self.set(0, 0);
        }
        adjusted
    }

    pub fn set(&mut self, start: i32, end: i32) {
        self.start = start;
        self.end = end;
    }

    pub fn bounds(&self) -> (i32, i32) {
        (self.start, self.end)
    }
}

/// One mutable host supplies all gameplay hooks, so production does not need
/// several closures holding competing mutable borrows of Simulation.
pub trait DoListHost<T: DoListEvent> {
    type Error;

    fn house_is_eligible(&self, house: CommandDispatchHouse) -> bool {
        house.dispatch_eligible
    }

    fn before_entry(&mut self, _house: CommandDispatchHouse, _entry: &T, _is_late: bool) {}

    fn late_entry(&mut self, house: CommandDispatchHouse, entry: &T);

    /// Called only for an on-time FRAMEINFO in network mode. Return true only
    /// when a checksum was compared; timing-gated records are still consumed.
    fn frame_info(&mut self, house: CommandDispatchHouse, entry: &T) -> Result<bool, Self::Error>;

    fn adjust_megamissions(&mut self, house: CommandDispatchHouse, entries: &mut [T]);

    fn execute(
        &mut self,
        house: CommandDispatchHouse,
        entry: &T,
        is_late: bool,
        retime_window: &mut DoListRetimeWindow,
    );
}

/// Counters describe dispatch, not successful gameplay receiver admission.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DispatchSummary {
    pub executed: usize,
    pub timing_consumed: usize,
    pub frame_info_compared: usize,
    pub late_executed: usize,
    pub late_skipped: usize,
    pub megamission_dropped: usize,
    pub retired: usize,
}

/// The one synchronized FIFO owner. Fresh admission clears only the processed
/// flag; restoring, splitting and rejoining existing entries preserve it.
#[derive(Debug, Clone, Serialize)]
pub struct DoList<T, const CAPACITY: usize = DO_LIST_CAPACITY> {
    entries: VecDeque<T>,
    retime_window: DoListRetimeWindow,
    /// Derived occupancy, never saved. Mutable external access invalidates it;
    /// the next admission rebuilds it once before resuming constant-time checks.
    #[serde(skip)]
    native_count: Option<NativeCommandOccupancy>,
}

impl<T: PartialEq, const CAPACITY: usize> PartialEq for DoList<T, CAPACITY> {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries && self.retime_window == other.retime_window
    }
}

impl<T: Eq, const CAPACITY: usize> Eq for DoList<T, CAPACITY> {}

impl<T, const CAPACITY: usize> Default for DoList<T, CAPACITY> {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            retime_window: DoListRetimeWindow::default(),
            native_count: Some(NativeCommandOccupancy::default()),
        }
    }
}

impl<'de, T: Deserialize<'de> + DoListEvent, const CAPACITY: usize> Deserialize<'de>
    for DoList<T, CAPACITY>
{
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Stored<T> {
            entries: VecDeque<T>,
            retime_window: DoListRetimeWindow,
        }
        let stored = Stored::<T>::deserialize(deserializer)?;
        let mut native_count = NativeCommandOccupancy::default();
        for entry in &stored.entries {
            native_count.add(entry);
        }
        if native_count.total > CAPACITY {
            return Err(serde::de::Error::custom(
                "saved synchronized event queue exceeds native capacity",
            ));
        }
        Ok(Self {
            entries: stored.entries,
            retime_window: stored.retime_window,
            native_count: Some(native_count),
        })
    }
}

impl<T, const CAPACITY: usize> DoList<T, CAPACITY> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.entries.iter()
    }

    /// Fixture access to queued entries. Gameplay mutates them only through
    /// admission, dispatch and retirement; this invalidates the occupancy cache.
    #[cfg(any(test, feature = "test-support"))]
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.native_count = None;
        self.entries.iter_mut()
    }

    pub fn retime_window(&self) -> DoListRetimeWindow {
        self.retime_window
    }

    pub fn set_retime_window(&mut self, start: i32, end: i32) {
        self.retime_window.set(start, end);
    }

    pub fn take_where(&mut self, mut predicate: impl FnMut(&T) -> bool) -> Vec<T>
    where
        T: DoListEvent,
    {
        let mut taken = Vec::new();
        let mut kept = VecDeque::with_capacity(self.entries.len());
        let mut native_count = NativeCommandOccupancy::default();
        for entry in std::mem::take(&mut self.entries) {
            if predicate(&entry) {
                taken.push(entry);
            } else {
                native_count.add(&entry);
                kept.push_back(entry);
            }
        }
        self.entries = kept;
        self.native_count = Some(native_count);
        taken
    }

    pub fn remove_first(&mut self, predicate: impl FnMut(&T) -> bool) -> Option<T>
    where
        T: DoListEvent,
    {
        let index = self.entries.iter().position(predicate)?;
        let entry = self.entries.remove(index)?;
        if let Some(native_count) = &mut self.native_count {
            native_count.remove(&entry);
        }
        Some(entry)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.native_count = Some(NativeCommandOccupancy::default());
    }
}

impl<T: DoListEvent, const CAPACITY: usize> DoList<T, CAPACITY> {
    pub fn native_len(&self) -> usize {
        self.native_occupancy().total
    }

    pub fn native_occupancy(&self) -> NativeCommandOccupancy {
        self.native_count.unwrap_or_else(|| {
            let mut count = NativeCommandOccupancy::default();
            for entry in &self.entries {
                count.add(entry);
            }
            count
        })
    }

    pub fn native_house_len(&self, house: i8) -> usize {
        if let Some(count) = &self.native_count {
            count.for_house(house)
        } else {
            self.native_occupancy().for_house(house)
        }
    }

    fn refresh_native_count(&mut self) -> usize {
        if self.native_count.is_none() {
            self.native_count = Some(self.native_occupancy());
        }
        self.native_count.as_ref().unwrap().total
    }

    /// New DoList admission consumes a refused event without evicting another
    /// record to create room. Clear bit 0 only on fresh admission.
    pub fn admit(&mut self, entry: T) -> bool {
        self.admit_with_reserved_native(entry, 0)
    }

    /// During a host callback the active scan may be temporarily moved out of
    /// its host. Reserve its still-occupied slots when admitting new events.
    pub fn admit_with_reserved_native(&mut self, entry: T, reserved: usize) -> bool {
        self.admit_bounded(
            entry,
            NativeCommandOccupancy {
                total: reserved,
                ..Default::default()
            },
            CAPACITY,
            usize::MAX,
        )
    }

    pub fn admit_bounded(
        &mut self,
        mut entry: T,
        reserved: NativeCommandOccupancy,
        maximum: usize,
        maximum_per_house: usize,
    ) -> bool {
        let native = entry.consumes_native_dolist_slot();
        let native_count = self.refresh_native_count();
        if native && native_count.saturating_add(reserved.total) >= maximum.min(CAPACITY) {
            return false;
        }
        if native
            && let Some(house) = entry.native_record().map(|record| record.house_id())
            && self
                .native_house_len(house)
                .saturating_add(reserved.for_house(house))
                >= maximum_per_house
        {
            return false;
        }
        entry.clear_processed();
        self.native_count.as_mut().unwrap().add(&entry);
        self.entries.push_back(entry);
        true
    }

    /// Rejoin an already-admitted tail without re-acknowledging its records.
    /// The receiver owns the existing retiming window.
    pub fn append_retained(&mut self, other: &mut Self) -> usize {
        let mut retained = 0;
        let mut native_count = self.refresh_native_count();
        for entry in other.entries.drain(..) {
            let native = entry.consumes_native_dolist_slot();
            if native && native_count >= CAPACITY {
                continue;
            }
            native_count += usize::from(native);
            self.native_count.as_mut().unwrap().add(&entry);
            self.entries.push_back(entry);
            retained += 1;
        }
        other.native_count = Some(NativeCommandOccupancy::default());
        retained
    }

    /// Original gamemd.exe Execute_DoList64C380: first retime the complete
    /// entry-count snapshot, then scan it once per registered eligible House,
    /// copy opcode 4 before decoding, mark consumed originals, and retire only
    /// the processed/expired head (retirement64D990). House staging drains
    /// before the next House.
    pub fn dispatch<H: DoListHost<T>>(
        &mut self,
        clock: CommandDispatchClock,
        houses: &[CommandDispatchHouse],
        mode: CommandDispatchMode,
        host: &mut H,
    ) -> Result<DispatchSummary, H::Error> {
        let mut summary = DispatchSummary::default();
        self.refresh_native_count();
        let scan_len = self.entries.len();
        for entry in self.entries.iter_mut().take(scan_len) {
            if let Some(record) = entry.native_record_mut() {
                let frame = record.frame_stamp();
                if record.opcode() != FRAMEINFO_OPCODE
                    && self.retime_window.start < frame
                    && frame < self.retime_window.end
                {
                    record.set_frame_stamp(self.retime_window.end);
                }
            }
        }

        for &house in houses {
            if !host.house_is_eligible(house) {
                continue;
            }
            let mut staged = Vec::new();
            let mut staged_late = Vec::new();
            let stage_capacity =
                if mode == CommandDispatchMode::Network && CAPACITY > DO_LIST_CAPACITY {
                    super::NETWORK_SOURCE_DOLIST_CAPACITY
                } else {
                    MEGAMISSION_STAGE_CAPACITY
                };
            for entry in self.entries.iter_mut().take(scan_len) {
                if entry.is_processed() {
                    continue;
                }
                let (opcode, is_late) = if let Some(record) = entry.native_record() {
                    if record.house_id() != house.house_id
                        || record.frame_stamp() > clock.native_frame
                    {
                        continue;
                    }
                    (
                        Some(record.opcode()),
                        record.frame_stamp() < clock.native_frame,
                    )
                } else {
                    let header = entry
                        .semantic_header()
                        .expect("a semantic queue entry must provide its scheduling header");
                    if header.owner != house.owner || header.execute_tick > clock.execute_tick {
                        continue;
                    }
                    // A semantic fallback has no native Event+3 stamp. Keep its
                    // established due policy until its producer emits native bytes.
                    (header.opcode, false)
                };

                host.before_entry(house, entry, is_late);
                if is_late
                    && opcode != Some(FRAMEINFO_OPCODE)
                    && mode == CommandDispatchMode::Network
                {
                    host.late_entry(house, entry);
                    summary.late_skipped += 1;
                    continue;
                }

                match opcode {
                    Some(FRAMEINFO_OPCODE) => {
                        if !is_late && mode == CommandDispatchMode::Network {
                            summary.frame_info_compared +=
                                usize::from(host.frame_info(house, entry)?);
                        }
                        summary.timing_consumed += 1;
                    }
                    Some(0x22) => {}
                    Some(0x0c) if !house.current_player => {}
                    Some(MEGAMISSION_OPCODE) => {
                        if stage_megamission(&mut staged, entry, stage_capacity) {
                            staged_late.push(is_late);
                        } else {
                            summary.megamission_dropped += 1;
                        }
                    }
                    _ => {
                        host.execute(house, entry, is_late, &mut self.retime_window);
                        summary.executed += 1;
                        summary.late_executed += usize::from(is_late);
                    }
                }
                entry.mark_processed();
            }
            if !staged.is_empty() {
                host.adjust_megamissions(house, &mut staged);
            }
            for (entry, is_late) in staged.into_iter().zip(staged_late) {
                host.execute(house, &entry, is_late, &mut self.retime_window);
                summary.executed += 1;
                summary.late_executed += usize::from(is_late);
            }
        }

        // Semantic fallback envelopes used to be consumed as a due batch,
        // including rejected senders. Until their native headers are ported,
        // retain that behavior without reinterpreting their issue ordinal.
        for entry in self.entries.iter_mut().take(scan_len) {
            if entry.native_record().is_none()
                && entry
                    .semantic_header()
                    .is_some_and(|header| header.execute_tick <= clock.execute_tick)
            {
                entry.mark_processed();
            }
        }

        while self.entries.front().is_some_and(|entry| {
            entry.is_processed()
                || entry.native_record().map_or_else(
                    || {
                        entry
                            .semantic_header()
                            .expect("a semantic queue entry must provide its scheduling header")
                            .execute_tick
                            < clock.execute_tick
                    },
                    |record| record.frame_stamp() < clock.native_frame,
                )
        }) {
            let entry = self
                .entries
                .pop_front()
                .expect("the retirement head exists");
            if let Some(native_count) = &mut self.native_count {
                native_count.remove(&entry);
            }
            summary.retired += 1;
        }
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    enum Event {
        Native(CommandRecord),
        Extension { tick: u64, processed: bool },
    }

    fn native(frame: i32) -> Event {
        Event::Native(CommandRecord::encode(0x15, 0, frame, &[]).unwrap())
    }

    fn extension(tick: u64) -> Event {
        Event::Extension {
            tick,
            processed: false,
        }
    }

    impl DoListEvent for Event {
        fn native_record(&self) -> Option<&CommandRecord> {
            match self {
                Self::Native(record) => Some(record),
                Self::Extension { .. } => None,
            }
        }

        fn native_record_mut(&mut self) -> Option<&mut CommandRecord> {
            match self {
                Self::Native(record) => Some(record),
                Self::Extension { .. } => None,
            }
        }

        fn semantic_header(&self) -> Option<SemanticEventHeader> {
            match self {
                Self::Native(_) => None,
                Self::Extension { tick, .. } => Some(SemanticEventHeader {
                    owner: InternedId::from_index(1),
                    execute_tick: *tick,
                    opcode: None,
                }),
            }
        }

        fn consumes_native_dolist_slot(&self) -> bool {
            matches!(self, Self::Native(_))
        }

        fn is_processed(&self) -> bool {
            match self {
                Self::Native(record) => record.is_processed(),
                Self::Extension { processed, .. } => *processed,
            }
        }

        fn mark_processed(&mut self) {
            match self {
                Self::Native(record) => record.mark_processed(),
                Self::Extension { processed, .. } => *processed = true,
            }
        }

        fn clear_processed(&mut self) {
            match self {
                Self::Native(record) => record.clear_processed(),
                Self::Extension { processed, .. } => *processed = false,
            }
        }
    }

    #[derive(Default)]
    struct Host {
        executed: usize,
    }

    impl DoListHost<Event> for Host {
        type Error = Infallible;

        fn late_entry(&mut self, _house: CommandDispatchHouse, _entry: &Event) {}

        fn frame_info(
            &mut self,
            _house: CommandDispatchHouse,
            _entry: &Event,
        ) -> Result<bool, Self::Error> {
            Ok(false)
        }

        fn adjust_megamissions(&mut self, _house: CommandDispatchHouse, _entries: &mut [Event]) {}

        fn execute(
            &mut self,
            _house: CommandDispatchHouse,
            _entry: &Event,
            _is_late: bool,
            _retime_window: &mut DoListRetimeWindow,
        ) {
            self.executed += 1;
        }
    }

    #[test]
    fn occupancy_cache_survives_mutation_rejoin_restore_and_blocked_retirement() {
        let mut queue: DoList<Event> = DoList::new();
        assert!(queue.admit(native(5)));
        assert!(queue.admit(extension(0)));
        assert_eq!(
            (queue.len(), queue.native_count.map(|count| count.total)),
            (2, Some(1))
        );

        let same_entries = queue.clone();
        drop(queue.iter_mut());
        assert_eq!(queue.native_count.map(|count| count.total), None);
        assert_eq!(queue, same_entries, "cache validity is not gameplay state");

        *queue.iter_mut().next().unwrap() = extension(0);
        assert_eq!(queue.native_len(), 0);
        assert_eq!(
            queue.native_count.map(|count| count.total),
            None,
            "shared reads do not write the cache"
        );
        assert!(queue.admit(native(5)));
        assert_eq!(
            queue.native_count.map(|count| count.total),
            Some(1),
            "admission refreshes once"
        );
        assert!(
            queue
                .remove_first(|entry| entry.native_record().is_some())
                .is_some()
        );
        assert_eq!(queue.native_count.map(|count| count.total), Some(0));
        assert!(queue.admit(native(5)));
        let extensions = queue.take_where(|entry| entry.native_record().is_none());
        assert_eq!(extensions.len(), 2);
        assert_eq!(
            (queue.len(), queue.native_count.map(|count| count.total)),
            (1, Some(1))
        );

        let mut tail = DoList::new();
        assert!(tail.admit(native(0)));
        assert!(tail.admit(extension(0)));
        tail.iter_mut().last().unwrap().mark_processed();
        assert_eq!(queue.append_retained(&mut tail), 2);
        assert_eq!(
            (tail.len(), tail.native_count.map(|count| count.total)),
            (0, Some(0))
        );
        assert_eq!(
            (queue.len(), queue.native_count.map(|count| count.total)),
            (3, Some(2))
        );
        assert!(queue.iter().last().unwrap().is_processed());

        let houses = [CommandDispatchHouse::new(
            InternedId::from_index(1),
            0,
            true,
        )];
        let mut host = Host::default();
        let summary = queue
            .dispatch(
                CommandDispatchClock {
                    native_frame: 0,
                    execute_tick: 0,
                },
                &houses,
                CommandDispatchMode::Offline,
                &mut host,
            )
            .unwrap();
        assert_eq!((summary.executed, summary.retired), (1, 0));
        assert_eq!(
            queue.native_count.map(|count| count.total),
            Some(2),
            "processed entries still occupy slots behind a future head"
        );

        drop(queue.iter_mut());
        let saved = serde_json::to_value(&queue).unwrap();
        assert!(saved.get("native_count").is_none());
        let mut restored: DoList<Event> = serde_json::from_value(saved).unwrap();
        assert_eq!(restored, queue);
        assert_eq!(restored.native_count.map(|count| count.total), Some(2));
        assert!(restored.iter().nth(1).unwrap().is_processed());
        let summary = restored
            .dispatch(
                CommandDispatchClock {
                    native_frame: 5,
                    execute_tick: 5,
                },
                &houses,
                CommandDispatchMode::Offline,
                &mut host,
            )
            .unwrap();
        assert_eq!((summary.executed, summary.retired), (1, 3));
        assert_eq!(host.executed, 2);
        assert_eq!(
            (
                restored.len(),
                restored.native_count.map(|count| count.total)
            ),
            (0, Some(0))
        );
        assert!(restored.admit(native(6)));
        restored.clear();
        assert_eq!(
            (
                restored.len(),
                restored.native_count.map(|count| count.total)
            ),
            (0, Some(0))
        );
    }
}
