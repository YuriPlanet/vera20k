//! In-game LAN application records inside the global connection envelope.
//!
//! `SessionClass` loading progress `0x0069AE90` sends 51 bytes; chat's
//! `0x0055EDBF` construction and `TauntCommandClass::Execute` `0x00536350`
//! send 455; the beacon placement and deletion records of
//! `BeaconManagerClass::PlaceBeacon` `0x00430BA0` and `DeleteBeacon`
//! `0x004311C0` send 61. `Network__ProcessGlobalPackets` `0x0048D1E0`
//! consumes them. Endpoint admission, routing, loading state, taunt playback
//! and beacons belong to their existing app owners. This module only encodes
//! and reads the fields.

use super::{
    LAN_MESSAGE_LEN, LanMessage, LanMessageError, LanMessageKind, SessionScope, game_name_hash,
};

pub const LOADING_PROGRESS_LEN: usize = 51;
pub const SCOPED_LOADING_MAGIC: &[u8; 4] = b"VLP2";
pub const SCOPED_LOADING_LEN: usize = 4 + SessionScope::LEN + LOADING_PROGRESS_LEN;
const LOADING_KIND: u32 = 0x0f;
const CHAT_KIND: u32 = 0x0c;
const TAUNT_KIND: u32 = 0x1d;
const RELAY_OFFSET: usize = 0x2f;
const CHAT_TEXT_OFFSET: usize = 0x30;
const CHAT_COLOR_OFFSET: usize = 0x110;
const CHAT_HASH_OFFSET: usize = 0x114;
const CHAT_TEXT_UNITS: usize = (CHAT_COLOR_OFFSET - CHAT_TEXT_OFFSET) / 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InGameMessageError {
    #[error(transparent)]
    Message(#[from] LanMessageError),
    #[error("loading progress has {actual} bytes, expected51")]
    WrongLoadingLength { actual: usize },
    #[error("loading progress has message kind{actual:#x}, expected0x0f")]
    WrongLoadingKind { actual: u32 },
    #[error("loading progress belongs to a different LAN admission")]
    WrongLoadingScope,
    #[error("chat is {actual} UTF-16 units, maximum{maximum}")]
    ChatTooLong { actual: usize, maximum: usize },
    #[error("beacon record has {actual} bytes, expected61")]
    WrongBeaconLength { actual: usize },
    #[error("beacon record has message kind{actual:#x}, expected0x20 or0x21")]
    WrongBeaconKind { actual: u32 },
}

/// The 61-byte beacon records (`Send_Global_Message(.., 0x3D, ack 1, ..)`).
pub const BEACON_LEN: usize = 0x3d;
const BEACON_PLACED_KIND: u32 = 0x20;
const BEACON_DELETED_KIND: u32 = 0x21;
const BEACON_COORD_OFFSET: usize = 0x2f;
const BEACON_SLOT_OFFSET: usize = 0x3b;
const BEACON_HOUSE_OFFSET: usize = 0x3c;

/// One beacon record's fields. House and slot are the sender's bytes; the
/// receiver sign-extends both (`MOVSX`, `0x0048D798`/`0x0048D7B8`,
/// `0x0048D7D5`/`0x0048D7DC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeaconRecord {
    /// Kind `0x20` (`0x00430D9F..0x00430E41`): the leptons coordinate at
    /// `+0x2F`, the slot the placement filled at `+0x3B`, the House at `+0x3C`.
    Placed {
        house: i8,
        slot: i8,
        coord: [i32; 3],
    },
    /// Kind `0x21` (`0x00431367..0x004313E9`): slot `+0x3B`, House `+0x3C`.
    Deleted { house: i8, slot: i8 },
}

/// The short beacon view shares the raw record owner of the other LAN
/// messages; only its first 61 bytes are transmitted. Native builds it in a
/// stack buffer it never clears, so new unused bytes are zero here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeaconMessage {
    message: LanMessage,
}

impl BeaconMessage {
    /// The sender's narrowed node name at `+0x04`
    /// (`0x00430DA9..0x00430DE0`) and the record's fields.
    pub fn new(name: &str, record: BeaconRecord) -> Result<Self, LanMessageError> {
        let (kind, house, slot) = match record {
            BeaconRecord::Placed { house, slot, .. } => (BEACON_PLACED_KIND, house, slot),
            BeaconRecord::Deleted { house, slot } => (BEACON_DELETED_KIND, house, slot),
        };
        let mut message = LanMessage::with_name(LanMessageKind::Unknown(kind), name)?;
        if let BeaconRecord::Placed { coord, .. } = record {
            for (index, value) in coord.into_iter().enumerate() {
                message.put_u32(BEACON_COORD_OFFSET + index * 4, value as u32);
            }
        }
        message.bytes[BEACON_SLOT_OFFSET] = slot as u8;
        message.bytes[BEACON_HOUSE_OFFSET] = house as u8;
        Ok(Self { message })
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, InGameMessageError> {
        if bytes.len() != BEACON_LEN {
            return Err(InGameMessageError::WrongBeaconLength {
                actual: bytes.len(),
            });
        }
        let mut raw = [0; LAN_MESSAGE_LEN];
        raw[..BEACON_LEN].copy_from_slice(bytes);
        let message = LanMessage::from_bytes(raw);
        let kind = message.u32(0);
        if !matches!(kind, BEACON_PLACED_KIND | BEACON_DELETED_KIND) {
            return Err(InGameMessageError::WrongBeaconKind { actual: kind });
        }
        Ok(Self { message })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.message.as_bytes()[..BEACON_LEN]
    }

    pub fn record(&self) -> BeaconRecord {
        let house = self.message.bytes[BEACON_HOUSE_OFFSET] as i8;
        let slot = self.message.bytes[BEACON_SLOT_OFFSET] as i8;
        if self.message.u32(0) == BEACON_DELETED_KIND {
            return BeaconRecord::Deleted { house, slot };
        }
        let coord =
            std::array::from_fn(|index| self.message.u32(BEACON_COORD_OFFSET + index * 4) as i32);
        BeaconRecord::Placed { house, slot, coord }
    }
}

/// The short Type0F view shares the same raw record owner as other LAN
/// messages. Only its first51 bytes are transmitted by69AFCC/69B006.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadingProgressMessage {
    message: LanMessage,
}

impl LoadingProgressMessage {
    /// The argument is the effective signed wire percent.69AE90 applies its
    /// scenario/display projection before69AF7A writes this field; the codec
    /// neither halves nor clamps the supplied value a second time.
    pub fn new(name: &str, percent: i32) -> Result<Self, LanMessageError> {
        let mut message = LanMessage::with_name(LanMessageKind::Unknown(LOADING_KIND), name)?;
        message.put_u32(RELAY_OFFSET, percent as u32);
        Ok(Self { message })
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, InGameMessageError> {
        if bytes.len() != LOADING_PROGRESS_LEN {
            return Err(InGameMessageError::WrongLoadingLength {
                actual: bytes.len(),
            });
        }
        let mut raw = [0; LAN_MESSAGE_LEN];
        raw[..LOADING_PROGRESS_LEN].copy_from_slice(bytes);
        let message = LanMessage::from_bytes(raw);
        let kind = message.u32(0);
        if kind != LOADING_KIND {
            return Err(InGameMessageError::WrongLoadingKind { actual: kind });
        }
        Ok(Self { message })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.message.as_bytes()[..LOADING_PROGRESS_LEN]
    }

    /// VERA binds the native loading record to the exact final host options.
    /// Delayed progress100 from a previous match cannot satisfy a new barrier.
    pub fn scoped_bytes(&self, scope: SessionScope) -> [u8; SCOPED_LOADING_LEN] {
        let mut bytes = [0; SCOPED_LOADING_LEN];
        bytes[..4].copy_from_slice(SCOPED_LOADING_MAGIC);
        bytes[4..4 + SessionScope::LEN].copy_from_slice(&scope.encode());
        bytes[4 + SessionScope::LEN..].copy_from_slice(self.as_bytes());
        bytes
    }

    pub fn decode_scoped(bytes: &[u8], expected: SessionScope) -> Result<Self, InGameMessageError> {
        if bytes.len() != SCOPED_LOADING_LEN
            || &bytes[..4] != SCOPED_LOADING_MAGIC
            || SessionScope::decode(&bytes[4..4 + SessionScope::LEN]) != Some(expected)
        {
            return Err(InGameMessageError::WrongLoadingScope);
        }
        Self::decode(&bytes[4 + SessionScope::LEN..])
    }

    pub fn name_bytes(&self) -> &[u8] {
        self.message.name_bytes()
    }

    pub fn name(&self) -> String {
        self.message.name()
    }

    /// `0x0048DAA3` uses FILD dword: negative wire values remain signed.
    pub fn percent(&self) -> i32 {
        self.message.u32(RELAY_OFFSET) as i32
    }
}

/// Borrowed Type0C fields. The raw owner retains all455 bytes, including
/// unpaired UTF-16 surrogates and bytes after the first text terminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InGameChat<'a> {
    pub relay_mask: u8,
    pub color: u32,
    pub game_name_hash: u32,
    text_bytes: &'a [u8],
}

impl InGameChat<'_> {
    pub fn text_units(&self) -> impl Iterator<Item = u16> + '_ {
        self.text_bytes
            .chunks_exact(2)
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
            .take_while(|unit| *unit != 0)
    }

    pub fn text(&self) -> String {
        String::from_utf16_lossy(&self.text_units().collect::<Vec<_>>())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InGameTaunt {
    pub relay_mask: u8,
    /// Already packed by the existing app taunt owner536350.
    pub byte: u8,
    pub color: u32,
    pub game_name_hash: u32,
}

impl LanMessage {
    /// `0x0055EDBF..0x0055EE60`: narrow sender name, UTF-16 chat, color and
    /// the shared5DAC00 game-name hash. Direct LAN sends have relay mask0.
    /// The native UI supplies at most97 text units; the wire field can hold
    ///111 plus NUL. Oversize text is rejected before any following field.
    ///
    /// Native reuses a buffer without clearing unused bytes. New records
    /// initialize unused bytes to zero; decoded records preserve them.
    pub fn in_game_chat(
        name: &str,
        color: u32,
        game_name: &str,
        text: &str,
    ) -> Result<Self, InGameMessageError> {
        let units: Vec<_> = text.encode_utf16().take_while(|unit| *unit != 0).collect();
        if units.len() >= CHAT_TEXT_UNITS {
            return Err(InGameMessageError::ChatTooLong {
                actual: units.len(),
                maximum: CHAT_TEXT_UNITS - 1,
            });
        }
        let mut message = Self::with_name(LanMessageKind::Unknown(CHAT_KIND), name)?;
        for (index, unit) in units.into_iter().enumerate() {
            let offset = CHAT_TEXT_OFFSET + index * 2;
            message.bytes[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
        }
        message.put_u32(CHAT_COLOR_OFFSET, color);
        message.put_u32(CHAT_HASH_OFFSET, game_name_hash(game_name));
        Ok(message)
    }

    /// `0x0048D8DD..0x0048D9B5` reads text/color/hash at30/110/114.
    pub fn in_game_chat_fields(&self) -> Option<InGameChat<'_>> {
        (self.u32(0) == CHAT_KIND).then(|| InGameChat {
            relay_mask: self.bytes[RELAY_OFFSET],
            color: self.u32(CHAT_COLOR_OFFSET),
            game_name_hash: self.u32(CHAT_HASH_OFFSET),
            text_bytes: &self.bytes[CHAT_TEXT_OFFSET..CHAT_COLOR_OFFSET],
        })
    }

    /// `0x00536404..0x00536446`: the existing taunt owner supplies its byte;
    /// the packet carries that byte at30, color at31 and shared hash at35.
    /// Native stack padding is unspecified; new unused bytes are zero.
    pub fn in_game_taunt(
        name: &str,
        color: u32,
        game_name: &str,
        byte: u8,
    ) -> Result<Self, LanMessageError> {
        let mut message = Self::with_name(LanMessageKind::Unknown(TAUNT_KIND), name)?;
        message.bytes[0x30] = byte;
        message.put_u32(0x31, color);
        message.put_u32(0x35, game_name_hash(game_name));
        Ok(message)
    }

    /// `0x0048D9BD..0x0048DA3B`: hash at35 and playback byte at30; color is
    /// carried by the sender although this receiver does not use it.
    pub fn in_game_taunt_fields(&self) -> Option<InGameTaunt> {
        (self.u32(0) == TAUNT_KIND).then(|| InGameTaunt {
            relay_mask: self.bytes[RELAY_OFFSET],
            byte: self.bytes[0x30],
            color: self.u32(0x31),
            game_name_hash: self.u32(0x35),
        })
    }
}

#[cfg(test)]
#[path = "ingame_tests.rs"]
mod tests;
