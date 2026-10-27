//! Bounded, optional copy of records already accepted by the process logger.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use crate::util::time::Instant;

const CAPACITY: usize = 4_000;
const MESSAGE_BYTES: usize = 2_048;
const TARGET_BYTES: usize = 128;

#[derive(Debug, Clone)]
pub struct LogLine {
    pub seq: u64,
    pub level: u8,
    pub target: String,
    pub message: String,
    pub t_ms: u64,
}

#[derive(Default)]
struct State {
    next: u64,
    sent: u64,
    lines: VecDeque<LogLine>,
}

pub struct LogRing {
    started: Instant,
    state: Mutex<State>,
}

impl LogRing {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            state: Mutex::new(State::default()),
        }
    }

    fn push(&self, level: log::Level, target: &str, message: &str) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.next = state.next.saturating_add(1);
        let line = LogLine {
            seq: state.next,
            level: match level {
                log::Level::Error => 1,
                log::Level::Warn => 2,
                log::Level::Info => 3,
                log::Level::Debug => 4,
                log::Level::Trace => 5,
            },
            target: bounded(target, TARGET_BYTES),
            message: bounded(message, MESSAGE_BYTES),
            t_ms: self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        };
        if state.lines.len() == CAPACITY {
            state.lines.pop_front();
        }
        state.lines.push_back(line);
    }

    /// Consume at most `limit` new records. Retained pending records are not
    /// counted as dropped; only actual ring eviction is reported.
    pub fn take_since_last(&self, limit: usize) -> (Vec<LogLine>, u64) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if limit == 0 {
            return (Vec::new(), 0);
        }
        let dropped = state.lines.front().map_or(0, |line| {
            line.seq.saturating_sub(state.sent.saturating_add(1))
        });
        let picked: Vec<_> = state
            .lines
            .iter()
            .filter(|line| line.seq > state.sent)
            .take(limit.min(CAPACITY))
            .cloned()
            .collect();
        if let Some(last) = picked.last() {
            state.sent = last.seq;
        }
        (picked, dropped)
    }
}

fn bounded(text: &str, maximum: usize) -> String {
    let mut end = text.len().min(maximum);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

static RING: OnceLock<LogRing> = OnceLock::new();

/// Enable only after the optional dashboard server has started successfully.
pub fn enable_telemetry_capture() {
    RING.get_or_init(LogRing::new);
}

pub fn ring() -> Option<&'static LogRing> {
    RING.get()
}

pub(super) fn record(level: log::Level, target: &str, message: &str) {
    if let Some(ring) = ring() {
        ring.push(level, target, message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_records_are_delivered_once_without_being_reported_lost() {
        let ring = LogRing::new();
        for i in 0..5 {
            ring.push(log::Level::Info, "source", &i.to_string());
        }
        let (first, lost) = ring.take_since_last(2);
        assert_eq!((first[0].seq, first[1].seq, lost), (1, 2, 0));
        let (rest, lost) = ring.take_since_last(20);
        assert_eq!(
            rest.iter().map(|line| line.seq).collect::<Vec<_>>(),
            vec![3, 4, 5]
        );
        assert_eq!(lost, 0);
        assert!(ring.take_since_last(1).0.is_empty());
    }

    #[test]
    fn eviction_is_counted_and_message_storage_is_bounded_utf8() {
        let ring = LogRing::new();
        for _ in 0..CAPACITY + 3 {
            ring.push(log::Level::Warn, &"t".repeat(300), &"€".repeat(2_000));
        }
        let (lines, lost) = ring.take_since_last(1);
        assert_eq!(lost, 3);
        assert_eq!(lines[0].seq, 4);
        assert_eq!(lines[0].target.len(), TARGET_BYTES);
        assert!(lines[0].message.len() <= MESSAGE_BYTES);
        assert_eq!(lines[0].level, 2);
        assert_eq!(ring.take_since_last(1).1, 0);
    }
}
