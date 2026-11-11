//! The least a headless client needs of the record stream: its contiguous
//! watermark, a reorder buffer, the gap test and frame-end progress
//! (e2e-plan "不再实现一遍协议调度"). It holds no sending policy, so both the
//! phase 1 model client and the phase 2 production-driven client read
//! through it.

use std::collections::BTreeMap;

use crate::{OrderEntry, OrderStatus, OrderingGap};

/// Records past the watermark kept for later (spec §3.5: `recv_next + 256`;
/// the current client's `REORDER_WINDOW`, src/net/live_commands/sequenced.rs
/// line 22).
pub const REORDER_WINDOW: u64 = 256;

/// One record as the comparison of streams needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Record {
    Batch {
        frame: u32,
        source: u8,
        batch_id: u64,
    },
    FrameEnd {
        frame: u32,
    },
    Departure {
        frame: u32,
        source: u8,
    },
    Pause {
        frame: u32,
        paused: bool,
    },
    Rate {
        frame: u32,
        frames_per_second: u16,
    },
    Desync {
        frame: u32,
        sample_frame: u32,
    },
}

impl Record {
    pub fn of(entry: &OrderEntry) -> (u64, Self) {
        match *entry {
            OrderEntry::Batch {
                sequence,
                frame,
                source,
                batch_id,
                ..
            } => (
                sequence,
                Self::Batch {
                    frame,
                    source,
                    batch_id,
                },
            ),
            OrderEntry::FrameEnd { sequence, frame } => (sequence, Self::FrameEnd { frame }),
            OrderEntry::Departure {
                sequence,
                frame,
                source,
                ..
            } => (sequence, Self::Departure { frame, source }),
            OrderEntry::Pause {
                sequence,
                frame,
                paused,
                ..
            } => (sequence, Self::Pause { frame, paused }),
            OrderEntry::Rate {
                sequence,
                frame,
                frames_per_second,
                ..
            } => (
                sequence,
                Self::Rate {
                    frame,
                    frames_per_second,
                },
            ),
            OrderEntry::Desync {
                sequence,
                frame,
                sample_frame,
                ..
            } => (
                sequence,
                Self::Desync {
                    frame,
                    sample_frame,
                },
            ),
        }
    }
}

#[derive(Debug)]
pub struct StreamReader {
    /// `recv_next`: every record below it has been read.
    next: u64,
    /// The relay's `next_seq` from its latest STATUS.
    remote_next: u64,
    head_frame: u32,
    started: bool,
    ahead: BTreeMap<u64, Record>,
    /// Every record read, in order, for the stream comparison.
    read: Vec<Record>,
}

impl Default for StreamReader {
    fn default() -> Self {
        Self {
            next: 1,
            remote_next: 1,
            head_frame: 0,
            started: false,
            ahead: BTreeMap::new(),
            read: Vec::new(),
        }
    }
}

impl StreamReader {
    pub fn next(&self) -> u64 {
        self.next
    }

    pub fn started(&self) -> bool {
        self.started
    }

    pub fn head_frame(&self) -> u32 {
        self.head_frame
    }

    pub fn read(&self) -> &[Record] {
        &self.read
    }

    /// A relay that lost the room (NotConfigured) starts it again; the
    /// stream read so far stays.
    pub fn forget_start(&mut self) {
        self.started = false;
    }

    pub fn status(&mut self, status: &OrderStatus) {
        self.started |= status.started;
        self.head_frame = self.head_frame.max(status.head_frame);
        self.remote_next = self.remote_next.max(status.next_seq);
    }

    /// Buffers a record. Older and too distant ones are dropped, and a
    /// sequence keeps its first copy (spec §3.5).
    pub fn insert(&mut self, entry: &OrderEntry) {
        let (sequence, record) = Record::of(entry);
        if sequence >= self.next && sequence - self.next < REORDER_WINDOW {
            self.ahead.entry(sequence).or_insert(record);
        }
    }

    /// The relay holds records past the watermark: a later one is buffered,
    /// or its STATUS names a later `next_seq`.
    pub fn gap(&self) -> bool {
        self.remote_next > self.next
            || self
                .ahead
                .first_key_value()
                .is_some_and(|(sequence, _)| *sequence > self.next)
    }

    /// The gap as spec §6.3 row 2 names it: a buffered record past the
    /// watermark is reordering; otherwise a STATUS ahead of it.
    pub fn gap_kind(&self) -> Option<OrderingGap> {
        if self
            .ahead
            .first_key_value()
            .is_some_and(|(sequence, _)| *sequence > self.next)
        {
            Some(OrderingGap::Reordered)
        } else if self.remote_next > self.next {
            Some(OrderingGap::BehindStatus)
        } else {
            None
        }
    }

    /// The relay's `next_seq` from its latest STATUS.
    pub fn remote_next(&self) -> u64 {
        self.remote_next
    }

    /// The next contiguous record, which moves the watermark past it.
    pub fn pop(&mut self) -> Option<Record> {
        let entry = self.ahead.first_entry()?;
        if *entry.key() != self.next {
            return None;
        }
        let record = entry.remove();
        self.next += 1;
        self.read.push(record);
        Some(record)
    }
}

/// How often spec §6.3 row 2 sends a gap UPLINK on this stream (§6.7 item
/// 1): first when a gap appears, at once for reordering or after `turn_ms`
/// for a STATUS ahead; then once more if it is still open `max(16, 4
/// rttvar)` ms after that first send (counted from the first, §6.7).
#[derive(Debug, Default)]
pub struct GapTriggers {
    since: Option<u64>,
    first: Option<u64>,
    second: bool,
    /// When each first trigger fired, in virtual µs.
    pub times: Vec<u64>,
    /// When each second trigger fired.
    pub second_times: Vec<u64>,
}

impl GapTriggers {
    pub fn observe(
        &mut self,
        gap: Option<OrderingGap>,
        now_ms: u64,
        turn_ms: u64,
        rttvar_ms: u64,
        at_us: u64,
    ) {
        let Some(gap) = gap else {
            self.since = None;
            self.first = None;
            self.second = false;
            return;
        };
        let since = *self.since.get_or_insert(now_ms);
        match self.first {
            None if gap == OrderingGap::Reordered || now_ms.saturating_sub(since) >= turn_ms => {
                self.first = Some(now_ms);
                self.times.push(at_us);
            }
            Some(first)
                if !self.second && now_ms.saturating_sub(first) >= (4 * rttvar_ms).max(16) =>
            {
                self.second = true;
                self.second_times.push(at_us);
            }
            _ => {}
        }
    }
}
