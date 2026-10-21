//! The455-byte BB LAN application message, inside the global connection frame.
//!
//! Producers `0x005DF9E0`, `0x005DFE00`, `0x005DF410`, `0x005DB720` and
//! `0x005E00B0` zero the whole message before writing its type-specific fields.
//! All offsets below are hexadecimal; the country at0x2F is decimal47.
//! Parsing retains every byte, including unknown message kinds and reserved
//! fields. Session validation, endpoint ownership and roster changes stay with
//! the app; the actual source address is global-envelope metadata, not payload.

use crate::sim::multiplayer_checksum::RetailChecksumAccumulator;
use crate::util::native_string::{narrow_utf16_c_string, widen_bytes};

pub const LAN_MESSAGE_LEN: usize = 455;
const NAME_OFFSET: usize = 0x04;
const NAME_LEN: usize = 20;
const SERIAL_OFFSET: usize = 0x18;
const SERIAL_LEN: usize = 23;
const COUNTRY_OFFSET: usize = 0x2f;
const COLOR_OFFSET: usize = 0x33;

/// `0x005DAC00`: copy the UTF-16 game name, uppercase ASCII in the native
/// startup C locale (`0x007DDCD6`), then fold each remaining non-NUL u16 with
/// shared64DE80. This hashes wide units, not the narrowed packet name bytes.
pub fn game_name_hash(name: &str) -> u32 {
    let mut sum = RetailChecksumAccumulator::new();
    for unit in name.encode_utf16().take_while(|unit| *unit != 0) {
        let upper = if (u16::from(b'a')..=u16::from(b'z')).contains(&unit) {
            unit - 32
        } else {
            unit
        };
        sum.fold_value(u32::from(upper));
    }
    sum.value()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanMessageKind {
    Discovery,
    Advertisement,
    GameQuery,
    ParticipantDetails,
    Presence,
    Keepalive,
    JoinRequest,
    JoinAccepted,
    JoinDenied,
    Departure,
    Go,
    ReadyToGo,
    Roster,
    Options,
    PlayerChoices,
    StartPrompt,
    Unknown(u32),
}

impl LanMessageKind {
    pub const fn raw(self) -> u32 {
        match self {
            Self::Discovery => 0,
            Self::Advertisement => 1,
            Self::GameQuery => 2,
            Self::ParticipantDetails => 3,
            Self::Presence => 4,
            Self::Keepalive => 5,
            Self::JoinRequest => 6,
            Self::JoinAccepted => 7,
            Self::JoinDenied => 8,
            Self::Departure => 10,
            Self::Go => 0x0b,
            Self::ReadyToGo => 0x13,
            Self::Roster => 0x16,
            Self::Options => 0x17,
            Self::PlayerChoices => 0x18,
            Self::StartPrompt => 0x28,
            Self::Unknown(raw) => raw,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanNamedMessage {
    Discovery,
    GameQuery,
    Keepalive,
    Departure,
}

impl LanNamedMessage {
    const fn kind(self) -> LanMessageKind {
        match self {
            Self::Discovery => LanMessageKind::Discovery,
            Self::GameQuery => LanMessageKind::GameQuery,
            Self::Keepalive => LanMessageKind::Keepalive,
            Self::Departure => LanMessageKind::Departure,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanVersionInterval {
    pub minimum: u32,
    pub maximum: u32,
}

impl LanVersionInterval {
    /// Original `VersionClass` getters74FDA0/74FDB0 return these literals.
    pub const RETAIL: Self = Self {
        minimum: 0x20000,
        maximum: 0x20000,
    };
}

/// Join6's explicit compatibility inputs. Names encode their wire offsets:
/// sender5DF78D reads AC026C/AC0274/AC0270 and777B50 respectively. The receiver
/// checks them at5E1711..5E174D. No substitute/default digest is supplied here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanCompatibility {
    pub field_0x43: u32,
    pub field_0x47: u32,
    pub field_0x4b: u32,
    pub field_0x4f: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanJoinRequest {
    pub country: i32,
    pub color: i32,
    pub version: LanVersionInterval,
    pub compatibility: LanCompatibility,
    /// The native sender stores the low byte of its request argument as u32.
    pub observer_request: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanParticipantDetails {
    pub country: i32,
    pub color: i32,
    /// Produced by the existing5DAC00 wide-name hash owner.
    pub game_name_hash: u32,
    /// Type3 copies nativeA8DA8C here; its role is independent of Type7.
    pub field_0x53: u32,
    /// Sender comparesA8DA8C/8316A4 and the first node's observer flag.
    /// The receiver treats nonzero as observer; it is not Type7's row index.
    pub field_0x57: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanAcceptance {
    pub country: i32,
    pub color: i32,
    /// First eligible row assigned by5E9240, read by guest5E0958.
    pub assigned_row: i32,
    /// Native8316A4; compared with assigned_row at5E08BB.
    pub observer_row: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LanMessageError {
    #[error("LAN application message has {actual} bytes, expected455")]
    WrongLength { actual: usize },
    #[error("LAN {field} is {actual} narrow bytes, maximum{maximum}")]
    TextTooLong {
        field: &'static str,
        actual: usize,
        maximum: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanMessage {
    bytes: [u8; LAN_MESSAGE_LEN],
}

impl LanMessage {
    pub fn decode(bytes: &[u8]) -> Result<Self, LanMessageError> {
        let bytes = bytes.try_into().map_err(|_| LanMessageError::WrongLength {
            actual: bytes.len(),
        })?;
        Ok(Self { bytes })
    }

    pub const fn from_bytes(bytes: [u8; LAN_MESSAGE_LEN]) -> Self {
        Self { bytes }
    }

    pub const fn as_bytes(&self) -> &[u8; LAN_MESSAGE_LEN] {
        &self.bytes
    }

    pub fn into_bytes(self) -> [u8; LAN_MESSAGE_LEN] {
        self.bytes
    }

    pub fn kind(&self) -> LanMessageKind {
        match self.u32(0) {
            0 => LanMessageKind::Discovery,
            1 => LanMessageKind::Advertisement,
            2 => LanMessageKind::GameQuery,
            3 => LanMessageKind::ParticipantDetails,
            4 => LanMessageKind::Presence,
            5 => LanMessageKind::Keepalive,
            6 => LanMessageKind::JoinRequest,
            7 => LanMessageKind::JoinAccepted,
            8 => LanMessageKind::JoinDenied,
            10 => LanMessageKind::Departure,
            0x0b => LanMessageKind::Go,
            0x13 => LanMessageKind::ReadyToGo,
            0x16 => LanMessageKind::Roster,
            0x17 => LanMessageKind::Options,
            0x18 => LanMessageKind::PlayerChoices,
            0x28 => LanMessageKind::StartPrompt,
            raw => LanMessageKind::Unknown(raw),
        }
    }

    pub fn name_bytes(&self) -> &[u8] {
        c_string(&self.bytes[NAME_OFFSET..NAME_OFFSET + NAME_LEN])
    }

    pub fn name(&self) -> String {
        widen_bytes(self.name_bytes())
    }

    pub fn serial_bytes(&self) -> &[u8] {
        c_string(&self.bytes[SERIAL_OFFSET..SERIAL_OFFSET + SERIAL_LEN])
    }

    pub fn advertisement_flags(&self) -> Option<u32> {
        (self.kind() == LanMessageKind::Advertisement).then(|| self.u32(COUNTRY_OFFSET))
    }

    pub fn presence_fields(&self) -> Option<(i32, i32)> {
        (self.kind() == LanMessageKind::Presence).then(|| self.country_color())
    }

    pub fn participant_fields(&self) -> Option<LanParticipantDetails> {
        (self.kind() == LanMessageKind::ParticipantDetails).then(|| {
            let (country, color) = self.country_color();
            LanParticipantDetails {
                country,
                color,
                game_name_hash: self.u32(0x37),
                field_0x53: self.u32(0x53),
                field_0x57: self.u32(0x57),
            }
        })
    }

    pub fn join_fields(&self) -> Option<LanJoinRequest> {
        (self.kind() == LanMessageKind::JoinRequest).then(|| {
            let (country, color) = self.country_color();
            LanJoinRequest {
                country,
                color,
                version: LanVersionInterval {
                    minimum: self.u32(0x3b),
                    maximum: self.u32(0x3f),
                },
                compatibility: LanCompatibility {
                    field_0x43: self.u32(0x43),
                    field_0x47: self.u32(0x47),
                    field_0x4b: self.u32(0x4b),
                    field_0x4f: self.u32(0x4f),
                },
                observer_request: self.u32(0x57),
            }
        })
    }

    pub fn acceptance_fields(&self) -> Option<LanAcceptance> {
        (self.kind() == LanMessageKind::JoinAccepted).then(|| {
            let (country, color) = self.country_color();
            LanAcceptance {
                country,
                color,
                assigned_row: self.u32(0x53) as i32,
                observer_row: self.u32(0x57) as i32,
            }
        })
    }

    pub fn denial_reason(&self) -> Option<u32> {
        (self.kind() == LanMessageKind::JoinDenied).then(|| self.u32(COUNTRY_OFFSET))
    }

    pub fn named(kind: LanNamedMessage, name: &str) -> Result<Self, LanMessageError> {
        Self::with_name(kind.kind(), name)
    }

    pub fn advertisement(game_name: &str, open: bool) -> Result<Self, LanMessageError> {
        let mut message = Self::with_name(LanMessageKind::Advertisement, game_name)?;
        message.put_u32(COUNTRY_OFFSET, u32::from(open));
        Ok(message)
    }

    pub fn presence(name: &str, country: i32, color: i32) -> Result<Self, LanMessageError> {
        let mut message = Self::with_name(LanMessageKind::Presence, name)?;
        message.put_country_color(country, color);
        Ok(message)
    }

    pub fn participant(
        name: &str,
        serial: &[u8],
        fields: LanParticipantDetails,
    ) -> Result<Self, LanMessageError> {
        let mut message = Self::with_name(LanMessageKind::ParticipantDetails, name)?;
        message.put_serial(serial)?;
        message.put_country_color(fields.country, fields.color);
        message.put_u32(0x37, fields.game_name_hash);
        message.put_u32(0x53, fields.field_0x53);
        message.put_u32(0x57, fields.field_0x57);
        Ok(message)
    }

    pub fn join_request(
        name: &str,
        serial: &[u8],
        fields: LanJoinRequest,
    ) -> Result<Self, LanMessageError> {
        let mut message = Self::with_name(LanMessageKind::JoinRequest, name)?;
        message.put_serial(serial)?;
        message.put_country_color(fields.country, fields.color);
        message.put_u32(0x3b, fields.version.minimum);
        message.put_u32(0x3f, fields.version.maximum);
        message.put_u32(0x43, fields.compatibility.field_0x43);
        message.put_u32(0x47, fields.compatibility.field_0x47);
        message.put_u32(0x4b, fields.compatibility.field_0x4b);
        message.put_u32(0x4f, fields.compatibility.field_0x4f);
        message.put_u32(0x57, fields.observer_request & 0xff);
        Ok(message)
    }

    pub fn join_accepted(host_name: &str, fields: LanAcceptance) -> Result<Self, LanMessageError> {
        let mut message = Self::with_name(LanMessageKind::JoinAccepted, host_name)?;
        message.put_country_color(fields.country, fields.color);
        message.put_u32(0x53, fields.assigned_row as u32);
        message.put_u32(0x57, fields.observer_row as u32);
        Ok(message)
    }

    pub fn join_denied(reason: u32) -> Self {
        let mut message = Self::zeroed(LanMessageKind::JoinDenied);
        message.put_u32(COUNTRY_OFFSET, reason);
        message
    }

    fn zeroed(kind: LanMessageKind) -> Self {
        let mut message = Self {
            bytes: [0; LAN_MESSAGE_LEN],
        };
        message.put_u32(0, kind.raw());
        message
    }

    fn with_name(kind: LanMessageKind, name: &str) -> Result<Self, LanMessageError> {
        let mut message = Self::zeroed(kind);
        let narrow = narrow_utf16_c_string(name.encode_utf16());
        message.put_text(NAME_OFFSET, NAME_LEN, &narrow, "name")?;
        Ok(message)
    }

    fn put_serial(&mut self, serial: &[u8]) -> Result<(), LanMessageError> {
        self.put_text(SERIAL_OFFSET, SERIAL_LEN, c_string(serial), "serial")
    }

    fn put_text(
        &mut self,
        offset: usize,
        capacity: usize,
        text: &[u8],
        field: &'static str,
    ) -> Result<(), LanMessageError> {
        if text.len() >= capacity {
            return Err(LanMessageError::TextTooLong {
                field,
                actual: text.len(),
                maximum: capacity - 1,
            });
        }
        self.bytes[offset..offset + text.len()].copy_from_slice(text);
        Ok(())
    }

    fn country_color(&self) -> (i32, i32) {
        (
            self.u32(COUNTRY_OFFSET) as i32,
            self.u32(COLOR_OFFSET) as i32,
        )
    }

    fn put_country_color(&mut self, country: i32, color: i32) {
        self.put_u32(COUNTRY_OFFSET, country as u32);
        self.put_u32(COLOR_OFFSET, color as u32);
    }

    fn u32(&self, offset: usize) -> u32 {
        u32::from_le_bytes(
            self.bytes[offset..offset + 4]
                .try_into()
                .expect("fixed LAN field"),
        )
    }

    fn put_u32(&mut self, offset: usize, value: u32) {
        self.bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}

fn c_string(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len())]
}

#[cfg(test)]
#[path = "lan_messages_tests.rs"]
mod tests;

pub mod setup;

pub mod ingame;
