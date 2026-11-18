//! The least the netsim's client needs of the record stream: its
//! contiguous watermark, the reorder buffer and the gap, by the rules the
//! game's `SequencedSession` reads it with (`REORDER_WINDOW`,
//! `OrderingGap::of`). It holds no sending policy.

use std::collections::BTreeMap;

use crate::{OrderEntry, OrderStatus, OrderingGap, REORDER_WINDOW};

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

    pub fn read(&self) -> &[Record] {
        &self.read
    }

    pub fn status(&mut self, status: &OrderStatus) {
        self.started |= status.started;
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

    /// The gap spec §6.3 row 2 acts on. The netsim's client runs each
    /// frame as its frame end is read, so unlike the game's it never holds
    /// back a full frame buffer.
    pub fn gap(&self) -> Option<OrderingGap> {
        OrderingGap::of(
            self.next,
            self.remote_next,
            self.ahead.first_key_value().map(|(sequence, _)| *sequence),
        )
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
