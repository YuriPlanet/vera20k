//! The LAN Cooperative result dialog's records inside the global connection
//! envelope: BC6's loop `0x0046FC00` and `Dialog0xBC6__DlgProc` `0x0046DFD0`
//! send them with ack through `0x00541260` on the global manager `0xA8E9C0`.
//! The signals are the kind and the sender's name, 0x2F bytes; the master's
//! options `0x32` (`0x0046D4A0`) is 455. The round state and the slot values
//! belong to their app owners; this module only encodes and reads fields.

use super::{LAN_MESSAGE_LEN, LanMessage, LanMessageError, LanMessageKind};

pub const SIGNAL_LEN: usize = 0x2f;
pub const OPTIONS_KIND: u32 = 0x32;
const DIFFICULTY_OFFSET: usize = 0x2f;
const HUMAN_START_OFFSET: usize = 0x30;
const HUMAN_COUNTRY_OFFSET: usize = 0x50;
const HUMAN_COLOR_OFFSET: usize = 0x70;
const AI_COUNTRY_OFFSET: usize = 0x90;
const AI_COLOR_OFFSET: usize = 0xb0;
const AI_DIFFICULTY_OFFSET: usize = 0xd0;
const AI_START_OFFSET: usize = 0xf0;
const AI_TEAM_OFFSET: usize = 0x110;
const ROWS: usize = 8;

/// A header-only BC6 record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CooperativeSignal {
    /// `0x23`, a guest's ack of the begin (`0x00470896`).
    Ack,
    /// `0x24`, Quit (`0x0046E2C6`).
    Left,
    /// `0x25`, not ready (`0x0046FED6`).
    Waiting,
    /// `0x26`, ready (`0x0046FDC9`).
    Ready,
    /// `0x31`, the master's go (`0x00470A8D`, `0x00470B2C`).
    Go,
    /// `0x34`, the master's begin (`0x0046FF9C`, `0x00470244`).
    Begin,
}

impl CooperativeSignal {
    pub const fn kind(self) -> u32 {
        match self {
            Self::Ack => 0x23,
            Self::Left => 0x24,
            Self::Waiting => 0x25,
            Self::Ready => 0x26,
            Self::Go => 0x31,
            Self::Begin => 0x34,
        }
    }

    /// The kind dword, then the narrow name from `+0x04`.
    pub fn encode(self, name: &str) -> Result<[u8; SIGNAL_LEN], LanMessageError> {
        let message = LanMessage::with_name(LanMessageKind::Unknown(self.kind()), name)?;
        Ok(message.as_bytes()[..SIGNAL_LEN]
            .try_into()
            .expect("signal prefix"))
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != SIGNAL_LEN {
            return None;
        }
        Some(
            match u32::from_le_bytes(bytes[..4].try_into().expect("kind")) {
                0x23 => Self::Ack,
                0x24 => Self::Left,
                0x25 => Self::Waiting,
                0x26 => Self::Ready,
                0x31 => Self::Go,
                0x34 => Self::Begin,
                _ => return None,
            },
        )
    }
}

/// One raw slot's human words: `-1` in all three for an empty or AI slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CooperativeHumanWords {
    pub start: i32,
    pub country: i32,
    pub color: i32,
}

/// One raw slot's AI words (`0xA8B29C`, `0xA8B2BC`, `0xA8B27C`,
/// `0xA8B2DC`, `0xA8B2FC`, the order of the log at `0x0081B320`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CooperativeAiWords {
    pub country: i32,
    pub color: i32,
    pub difficulty: i32,
    pub start: i32,
    pub team: i32,
}

/// The master's options `0x32`: the difficulty byte `+0x2F`
/// (`[0xA8EB68]`), the human start/country/colour arrays `+0x30`/`+0x50`/
/// `+0x70`, then the AI country, colour, difficulty, start and ally arrays
/// `+0x90..+0x110`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CooperativeOptionsRecord {
    pub difficulty: u8,
    pub humans: [CooperativeHumanWords; ROWS],
    pub ai: [CooperativeAiWords; ROWS],
}

impl CooperativeOptionsRecord {
    /// New records zero the native buffer's uninitialised tail.
    pub fn encode(&self, name: &str) -> Result<LanMessage, LanMessageError> {
        let mut message = LanMessage::with_name(LanMessageKind::Unknown(OPTIONS_KIND), name)?;
        message.bytes[DIFFICULTY_OFFSET] = self.difficulty;
        for (row, (human, ai)) in self.humans.iter().zip(&self.ai).enumerate() {
            let word = |base: usize| base + row * 4;
            message.put_u32(word(HUMAN_START_OFFSET), human.start as u32);
            message.put_u32(word(HUMAN_COUNTRY_OFFSET), human.country as u32);
            message.put_u32(word(HUMAN_COLOR_OFFSET), human.color as u32);
            message.put_u32(word(AI_COUNTRY_OFFSET), ai.country as u32);
            message.put_u32(word(AI_COLOR_OFFSET), ai.color as u32);
            message.put_u32(word(AI_DIFFICULTY_OFFSET), ai.difficulty as u32);
            message.put_u32(word(AI_START_OFFSET), ai.start as u32);
            message.put_u32(word(AI_TEAM_OFFSET), ai.team as u32);
        }
        Ok(message)
    }

    pub fn decode(message: &LanMessage) -> Option<Self> {
        if message.u32(0) != OPTIONS_KIND {
            return None;
        }
        let word = |base: usize, row: usize| message.u32(base + row * 4) as i32;
        Some(Self {
            difficulty: message.bytes[DIFFICULTY_OFFSET],
            humans: std::array::from_fn(|row| CooperativeHumanWords {
                start: word(HUMAN_START_OFFSET, row),
                country: word(HUMAN_COUNTRY_OFFSET, row),
                color: word(HUMAN_COLOR_OFFSET, row),
            }),
            ai: std::array::from_fn(|row| CooperativeAiWords {
                country: word(AI_COUNTRY_OFFSET, row),
                color: word(AI_COLOR_OFFSET, row),
                difficulty: word(AI_DIFFICULTY_OFFSET, row),
                start: word(AI_START_OFFSET, row),
                team: word(AI_TEAM_OFFSET, row),
            }),
        })
    }
}

const _: () = assert!(AI_TEAM_OFFSET + ROWS * 4 <= LAN_MESSAGE_LEN);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_are_the_native_kind_and_name_prefix() {
        let bytes = CooperativeSignal::Ready.encode("Bob").unwrap();
        assert_eq!(bytes.len(), 0x2f);
        assert_eq!(&bytes[..8], &[0x26, 0, 0, 0, b'B', b'o', b'b', 0]);
        assert_eq!(
            CooperativeSignal::decode(&bytes),
            Some(CooperativeSignal::Ready)
        );
        // A full-length record is never a signal.
        assert_eq!(CooperativeSignal::decode(&[0x26; LAN_MESSAGE_LEN]), None);
    }

    #[test]
    fn options_words_sit_at_the_native_offsets() {
        let mut record = CooperativeOptionsRecord {
            difficulty: 2,
            humans: [CooperativeHumanWords {
                start: -1,
                country: -1,
                color: -1,
            }; ROWS],
            ai: [CooperativeAiWords {
                country: -1,
                color: -1,
                difficulty: 0,
                start: -2,
                team: -2,
            }; ROWS],
        };
        record.humans[1] = CooperativeHumanWords {
            start: -2,
            country: 4,
            color: 6,
        };
        record.ai[7] = CooperativeAiWords {
            country: 9,
            color: 3,
            difficulty: 1,
            start: 5,
            team: 2,
        };
        let message = record.encode("Host").unwrap();
        let bytes = message.as_bytes();
        let word =
            |offset: usize| i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        assert_eq!(word(0), 0x32);
        assert_eq!(bytes[0x2f], 2);
        assert_eq!(
            (word(0x34), word(0x54), word(0x74)),
            (-2, 4, 6),
            "slot 1's start, country and colour"
        );
        assert_eq!(
            (word(0xac), word(0xcc), word(0xec), word(0x10c), word(0x12c)),
            (9, 3, 1, 5, 2),
            "slot 7's AI country, colour, difficulty, start and ally"
        );
        assert_eq!(CooperativeOptionsRecord::decode(&message), Some(record));
    }
}
