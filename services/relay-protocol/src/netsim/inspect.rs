//! A wire checker that reads every datagram's bytes by the specs and calls
//! no production decoder, so it is also an independent check of the format
//! (e2e-plan §1.3): the compacted wire of services/relay/bandwidth-
//! compaction/spec.md ("compaction spec"), envelope v3 (§6.1), PACK v2
//! (§5.2, §5.3), the varint ordering messages (§7) and plaintext batches
//! (§3.1), carrying the adaptive transport's messages
//! (services/relay/adaptive-transport/spec.md §3). The format itself lives
//! in `v3`; this module holds what judgments read of it. Every PACK record
//! carries its canonical record bytes (transport spec §3.5: `[kind][seq
//! u64][frame u32][rest]`) for byte comparisons (compaction spec §5.1,
//! §12.2).

pub mod v3;

/// One datagram, by its envelope and payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Datagram {
    pub kind: &'static str,
    /// The envelope's per-hop `seq` (compaction spec §6.1).
    pub hop_seq: u64,
    /// The envelope's `session` (compaction spec §6.1, §6.2), which names
    /// the hop; 0 on HELLO, CONFIRM and CHALLENGE.
    pub session: u32,
    /// The envelope's payload length.
    pub payload_len: usize,
    pub detail: Detail,
}

/// The `up_high` / `up_bits` / `echo_delay` tail of ACK and STATUS
/// (transport spec §3.3, §3.4; compaction spec §7.2, §7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feedback {
    pub high: u64,
    pub bits: u64,
    pub echo_delay_ms: u16,
}

/// UPLINK (request 8, transport spec §3.2, compaction spec §7.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uplink {
    pub flags: u8,
    pub recv_next: u64,
    pub down_high: u64,
    pub down_bits: u64,
    pub srtt_ms: u16,
    pub base_ms: u16,
    pub rttvar_ms: u16,
    pub link: u8,
    pub checksums: Vec<(u32, u32)>,
    /// `(pace, frames_per_second, tick_micros)` when flags bit1.
    pub pace: Option<(u8, u16, u32)>,
    pub first_batch_id: u64,
    /// Each batch's plaintext length, IDs `first_batch_id + i`.
    pub batches: Vec<usize>,
}

pub const FLAG_PUSH: u8 = 1;
pub const FLAG_PACE: u8 = 2;
pub const FLAG_PROBE: u8 = 4;
pub const FLAG_RESTORING: u8 = 8;

impl Uplink {
    pub fn batch_ids(&self) -> impl Iterator<Item = u64> + '_ {
        (0..self.batches.len() as u64).map(|i| self.first_batch_id + i)
    }
    pub fn carries(&self, batch_id: u64) -> bool {
        self.batch_ids().any(|id| id == batch_id)
    }
}

/// One record of a PACK (compaction spec §5.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackRecord {
    pub seq: u64,
    pub kind: u8,
    pub frame: u32,
    /// BATCH and DEPARTURE.
    pub source: Option<u8>,
    /// BATCH.
    pub batch_id: Option<u64>,
    /// BATCH plaintext length.
    pub opaque_len: Option<usize>,
    /// DEPARTURE of a lost connection.
    pub lost: bool,
    /// The canonical record: `[kind][seq u64][frame u32][rest]` (transport
    /// spec §3.5, compaction spec §5.1).
    pub canonical: Vec<u8>,
    /// The bytes this record's entry took in its PACK.
    pub wire_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detail {
    None,
    ReadyRequest,
    Pull {
        next: u64,
    },
    Uplink(Uplink),
    OrderReady {
        ready_mask: u32,
        sources_mask: u32,
    },
    Status {
        head_frame: u32,
        oldest_seq: u64,
        next_seq: u64,
        active_sources: u32,
        tick_hz: u16,
        start_delay_ms: u32,
        started: bool,
        feedback: Feedback,
    },
    Ack {
        batch_id: u64,
        frame: u32,
        feedback: Feedback,
    },
    Error {
        code: u8,
        expected: u64,
    },
    Pack {
        base: u64,
        count: u8,
        records: Vec<PackRecord>,
    },
    /// The READY answering a CONFIRM (relay sequence 1): the hop's new
    /// session (compaction spec §6.1, §6.2).
    HopReady {
        session: u32,
    },
}

pub use v3::{client_datagram, server_datagram};
