//! Bounded campaign controls under the existing extended LAN setup tag.
//! The application checks the admitted source, room scope and stage attempt
//! before delivering these records to either synchronization owner.

use super::{MAGIC, SessionScope};

const KIND: u8 = 2;
const HEADER: usize = 5 + SessionScope::LEN + 4;
const FACTS_START: usize = HEADER + 1 + 8 + 8;
const LENGTH: usize = FACTS_START + 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StageChoice {
    Retry,
    Route(u16),
}

impl StageChoice {
    fn encode(self) -> u64 {
        match self {
            Self::Retry => 0,
            Self::Route(index) => u64::from(index) + 1,
        }
    }

    fn decode(raw: u64) -> Option<Self> {
        if raw == 0 {
            Some(Self::Retry)
        } else {
            u16::try_from(raw - 1).ok().map(Self::Route)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Signal {
    NarrativeReady {
        boundary: u64,
        tick: u64,
    },
    NarrativeRelease {
        boundary: u64,
        tick: u64,
    },
    NarrativeAbort {
        boundary: u64,
        tick: u64,
    },
    StageVote(StageChoice),
    StagePrepare {
        choice: StageChoice,
        ending: u64,
        next: [u8; 32],
    },
    StageAccepted(StageChoice),
    StageBegin(StageChoice),
    StageAbort,
    StageBegun(StageChoice),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Record {
    /// `None` uses the authenticated WOL relay's match identity. Direct LAN
    /// always sends its accepted setup scope.
    pub scope: Option<SessionScope>,
    pub attempt: u32,
    pub signal: Signal,
}

pub fn is_control(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC) && bytes.get(MAGIC.len()) == Some(&KIND)
}

impl Record {
    pub fn encode(self) -> [u8; LENGTH] {
        let (tag, first, second) = match self.signal {
            Signal::NarrativeReady { boundary, tick } => (0, boundary, tick),
            Signal::NarrativeRelease { boundary, tick } => (1, boundary, tick),
            Signal::NarrativeAbort { boundary, tick } => (2, boundary, tick),
            Signal::StageVote(choice) => (3, choice.encode(), 0),
            Signal::StagePrepare { choice, ending, .. } => (4, choice.encode(), ending),
            Signal::StageAccepted(choice) => (5, choice.encode(), 0),
            Signal::StageBegin(choice) => (6, choice.encode(), 0),
            Signal::StageAbort => (7, 0, 0),
            Signal::StageBegun(choice) => (8, choice.encode(), 0),
        };
        let mut bytes = [0; LENGTH];
        bytes[..MAGIC.len()].copy_from_slice(MAGIC);
        bytes[MAGIC.len()] = KIND;
        if let Some(scope) = self.scope {
            bytes[5..HEADER - 4].copy_from_slice(&scope.encode());
        }
        bytes[HEADER - 4..HEADER].copy_from_slice(&self.attempt.to_le_bytes());
        bytes[HEADER] = tag;
        bytes[HEADER + 1..HEADER + 9].copy_from_slice(&first.to_le_bytes());
        bytes[HEADER + 9..FACTS_START].copy_from_slice(&second.to_le_bytes());
        if let Signal::StagePrepare { next, .. } = self.signal {
            bytes[FACTS_START..].copy_from_slice(&next);
        }
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != LENGTH || !is_control(bytes) {
            return None;
        }
        let scope_bytes = &bytes[5..HEADER - 4];
        let scope = if scope_bytes == [0; SessionScope::LEN] {
            None
        } else {
            let scope = SessionScope::decode(scope_bytes)?;
            if scope.epoch == [0; 16] || scope.revision == 0 {
                return None;
            }
            Some(scope)
        };
        let attempt = u32::from_le_bytes(bytes[HEADER - 4..HEADER].try_into().ok()?);
        if attempt == 0 {
            return None;
        }
        let first = u64::from_le_bytes(bytes[HEADER + 1..HEADER + 9].try_into().ok()?);
        let second = u64::from_le_bytes(bytes[HEADER + 9..FACTS_START].try_into().ok()?);
        let next: [u8; 32] = bytes[FACTS_START..].try_into().ok()?;
        if bytes[HEADER] != 4 && next != [0; 32] {
            return None;
        }
        let signal = match bytes[HEADER] {
            0 if first != 0 => Signal::NarrativeReady {
                boundary: first,
                tick: second,
            },
            1 if first != 0 => Signal::NarrativeRelease {
                boundary: first,
                tick: second,
            },
            2 if first != 0 => Signal::NarrativeAbort {
                boundary: first,
                tick: second,
            },
            3 if second == 0 => Signal::StageVote(StageChoice::decode(first)?),
            4 => Signal::StagePrepare {
                choice: StageChoice::decode(first)?,
                ending: second,
                next,
            },
            5 if second == 0 => Signal::StageAccepted(StageChoice::decode(first)?),
            6 if second == 0 => Signal::StageBegin(StageChoice::decode(first)?),
            7 if first == 0 && second == 0 => Signal::StageAbort,
            8 if second == 0 => Signal::StageBegun(StageChoice::decode(first)?),
            _ => return None,
        };
        Some(Self {
            scope,
            attempt,
            signal,
        })
    }
}

#[cfg(all(test, tests_net))]
mod tests {
    use super::*;

    #[test]
    fn campaign_controls_do_not_decode_as_setup_and_reject_invalid_extent() {
        let record = Record {
            scope: Some(SessionScope {
                epoch: [4; 16],
                revision: 18,
            }),
            attempt: 2,
            signal: Signal::StagePrepare {
                choice: StageChoice::Route(1),
                ending: u64::MAX,
                next: [8; 32],
            },
        };
        let mut bytes = record.encode();
        assert_eq!(Record::decode(&bytes), Some(record));
        assert_eq!(Record::decode(&bytes[..bytes.len() - 1]), None);
        bytes[HEADER - 4..HEADER].fill(0);
        assert_eq!(Record::decode(&bytes), None);
    }
}
