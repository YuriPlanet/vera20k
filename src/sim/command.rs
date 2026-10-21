//! Deterministic simulation command model.
//!
//! All gameplay inputs are translated into explicit commands that can be
//! scheduled by tick, logged, replayed, and sent over lockstep transport.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::sim::intern::InternedId;
use crate::sim::production::ProductionCategory;

/// Fixed byte width of one synchronized command record.
pub const COMMAND_RECORD_LEN: usize = 0x6f;
/// Byte width available to opcode-specific command data.
pub const COMMAND_RECORD_PAYLOAD_LEN: usize = COMMAND_RECORD_LEN - COMMAND_RECORD_PAYLOAD_OFFSET;

const COMMAND_RECORD_OPCODE_OFFSET: usize = 0;
const COMMAND_RECORD_FLAGS_OFFSET: usize = 1;
const COMMAND_RECORD_HOUSE_OFFSET: usize = 2;
const COMMAND_RECORD_FRAME_OFFSET: usize = 3;
const COMMAND_RECORD_PAYLOAD_OFFSET: usize = 7;
const COMMAND_RECORD_PROCESSED_FLAG: u8 = 0x01;

/// Native `EventClass` opcode for selling one wall-overlay cell.
pub const SELL_WALL_AT_CELL_OPCODE: u8 = 0x17;
/// Native `EventClass` opcode for leaving the active scenario.
pub const EXIT_OPCODE: u8 = 0x13;
/// Native header-only DESTRUCT event: `EventClass::Execute`'s type-8 entry
/// (`0x004C7BDF`) calls `HouseClass::Flag_To_Die 0x004FC980` on the header
/// House, ungated.
pub const DESTRUCT_OPCODE: u8 = 0x08;
/// EVENT_REMOVEPLAYER: `EventClass(house, 0x23, index)` (`0x004C6720`)
/// carries the removed House's index as its dword payload. DestroyConnection
/// queues it for a lost connection mid-match (`0x005DA898`).
pub const REMOVE_PLAYER_OPCODE: u8 = 0x23;
/// EVENT_ALLY: `EventClass(house, 3, index)` (`0x004C6720`) carries the
/// other House's index as its dword payload. The Diplomacy Allies box
/// (`Ally 0x00731840`) and the ToggleAlliance hotkey (`0x007318F0`) queue it.
pub const ALLY_OPCODE: u8 = 0x03;
/// House-wide Cheer, issued by AllToCheer730F30.
pub const CHEER_OPCODE: u8 = 0x2d;
/// Native `EventClass` opcode for a MegaMission order envelope.
pub const MEGAMISSION_OPCODE: u8 = 0x04;
/// Native selected-object Scatter event.
pub const SCATTER_OPCODE: u8 = 0x07;
/// Native `EventClass` opcode 0x0D, the GAMESPEED event. `EventClass::Execute`
/// type 13 (`0x004C794B`) writes the payload dword at Event+7 into the
/// GameSpeed setting `0x00A8EB60`, then posts `TXT_PLAYER_CHANGED_SPEED` when
/// the sender is not the local player. Offline Options uses `SetGameSpeed`.
pub const GAME_SPEED_OPCODE: u8 = 0x0d;

/// Payload of the native GAMESPEED event (opcode `0x0D`, type 13 at
/// `0x004C794B`): the setting dword at Event+7; the sender rides the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameSpeedRecord {
    pub house_id: i8,
    pub frame: i32,
    pub speed: i32,
}

impl GameSpeedRecord {
    pub fn encode(self) -> Result<CommandRecord, CommandRecordError> {
        let mut payload = [0u8; 4];
        payload.copy_from_slice(&self.speed.to_le_bytes());
        CommandRecord::encode(
            GAME_SPEED_OPCODE,
            i32::from(self.house_id),
            self.frame,
            &payload,
        )
    }

    pub fn decode(record: &CommandRecord) -> Option<Self> {
        if record.opcode() != GAME_SPEED_OPCODE {
            return None;
        }
        Some(Self {
            house_id: record.house_id(),
            frame: record.frame_stamp(),
            speed: i32::from_le_bytes(record.payload()[..4].try_into().ok()?),
        })
    }
}

/// Native `EventClass` opcodes 0x01 / 0x02 (Building On Line / Off Line).
/// Both dispatch branches (`0x004C6CF0` / `0x004C6D4D`) read the target as the
/// Abstract token at payload `[0..4]` with kind `0x34` at `[4]` and hand the
/// resolved object to GoOnline `0x00452260` / GoOffline `0x00452360`; the
/// direction is the opcode, never a payload byte. EVA strings
/// `EVA_BuildingOnLine` / `EVA_BuildingOffLine` name the pair.
pub const BUILDING_ON_LINE_OPCODE: u8 = 0x01;
pub const BUILDING_OFF_LINE_OPCODE: u8 = 0x02;

/// Typed view of native opcodes 0x01 / 0x02 (Building On Line / Off Line).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildingOnLineRecord {
    pub house_id: i8,
    pub frame: i32,
    pub entity_id: i32,
    /// The opcode's direction: `0x01` On Line, `0x02` Off Line.
    pub on: bool,
}

impl BuildingOnLineRecord {
    pub fn encode(self) -> Result<CommandRecord, CommandRecordError> {
        let opcode = if self.on {
            BUILDING_ON_LINE_OPCODE
        } else {
            BUILDING_OFF_LINE_OPCODE
        };
        let mut payload = [0u8; 5];
        let (value, kind) = MegaMissionTarget::Object { id: self.entity_id }
            .token()
            .expect("an Object token encodes a 0x34 Abstract");
        payload[..4].copy_from_slice(&value.to_le_bytes());
        payload[4] = kind;
        CommandRecord::encode(opcode, i32::from(self.house_id), self.frame, &payload)
    }

    /// Decode either direction. A non-`0x34` kind byte decodes to nothing:
    /// the native resolver `0x006E7A80` compares `[token+4]` against `0x34`
    /// first and answers null for anything else, which the dispatch drops.
    pub fn decode(record: &CommandRecord) -> Option<Self> {
        let on = match record.opcode() {
            BUILDING_ON_LINE_OPCODE => true,
            BUILDING_OFF_LINE_OPCODE => false,
            _ => return None,
        };
        let value = i32::from_le_bytes(record.payload()[..4].try_into().ok()?);
        let MegaMissionTarget::Object { id } =
            MegaMissionTarget::from_token(value, record.payload()[4])?
        else {
            return None;
        };
        Some(Self {
            house_id: record.house_id(),
            frame: record.frame_stamp(),
            entity_id: id,
            on,
        })
    }
}

const MEGAMISSION_SOURCE_VALUE_OFFSET: usize = 0;
const MEGAMISSION_SOURCE_KIND_OFFSET: usize = 4;
const MEGAMISSION_ACTION_OFFSET: usize = 5;
const MEGAMISSION_SECONDARY_VALUE_OFFSET: usize = 7;
const MEGAMISSION_SECONDARY_KIND_OFFSET: usize = 11;
const MEGAMISSION_DESTINATION_VALUE_OFFSET: usize = 12;
const MEGAMISSION_DESTINATION_KIND_OFFSET: usize = 16;
const MEGAMISSION_AUXILIARY_VALUE_OFFSET: usize = 17;
const MEGAMISSION_AUXILIARY_KIND_OFFSET: usize = 21;
const MEGAMISSION_PLANNING_OFFSET: usize = 22;
const MEGAMISSION_OWNED_PAYLOAD_LEN: usize = 23;

const ABSTRACT_OBJECT_TARGET_KIND: u8 = 0x34;
const CELL_TARGET_KIND: u8 = 0x0b;
const NULL_TARGET_KIND: u8 = 0;
const MOVE_ACTION: u8 = 2;
const AREA_GUARD_ACTION: u8 = 11;
const CELL_TOKEN_ROW_STRIDE: i32 = 1000;

/// A malformed fixed-width synchronized command record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CommandRecordError {
    #[error("command record must be exactly {expected} bytes, got {actual}")]
    InvalidLength { expected: usize, actual: usize },
    #[error("command payload can contain at most {max} bytes, got {actual}")]
    PayloadTooLong { max: usize, actual: usize },
}

/// A cell which cannot make the native `x + y * 1000` round trip exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("cell ({x}, {y}) is not exactly decodable from its native target token")]
pub struct MegaMissionCellTokenError {
    pub x: i16,
    pub y: i16,
}

/// Native-width synchronized command envelope.
///
/// The header is seven bytes: opcode, flags, signed house id, and a signed
/// little-endian frame stamp. The remaining 104 bytes are an opcode-specific
/// payload. Keeping the payload opaque here lets the admission boundary
/// preserve every byte without coupling transport to command dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRecord {
    bytes: [u8; COMMAND_RECORD_LEN],
}

impl Serialize for CommandRecord {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeTuple;

        let mut record = serializer.serialize_tuple(COMMAND_RECORD_LEN)?;
        for byte in &self.bytes {
            record.serialize_element(byte)?;
        }
        record.end()
    }
}

impl<'de> Deserialize<'de> for CommandRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct CommandRecordVisitor;

        impl<'de> serde::de::Visitor<'de> for CommandRecordVisitor {
            type Value = CommandRecord;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(
                    formatter,
                    "an exact {COMMAND_RECORD_LEN}-byte command record"
                )
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut bytes = [0; COMMAND_RECORD_LEN];
                for (index, byte) in bytes.iter_mut().enumerate() {
                    *byte = sequence
                        .next_element()?
                        .ok_or_else(|| serde::de::Error::invalid_length(index, &self))?;
                }
                Ok(CommandRecord { bytes })
            }
        }

        deserializer.deserialize_tuple(COMMAND_RECORD_LEN, CommandRecordVisitor)
    }
}

impl CommandRecord {
    /// Encode a freshly issued command with deterministic zeroed padding.
    ///
    /// A negative house id produces the native null-event header: opcode zero
    /// and house `-1`. The frame stamp is still written and the payload is not
    /// admitted.
    pub fn encode(
        opcode: u8,
        house_id: i32,
        frame_stamp: i32,
        payload: &[u8],
    ) -> Result<Self, CommandRecordError> {
        if payload.len() > COMMAND_RECORD_PAYLOAD_LEN {
            return Err(CommandRecordError::PayloadTooLong {
                max: COMMAND_RECORD_PAYLOAD_LEN,
                actual: payload.len(),
            });
        }

        let mut record = Self {
            bytes: [0; COMMAND_RECORD_LEN],
        };
        record.set_issue_header(opcode, house_id, frame_stamp);
        if house_id >= 0 {
            record.payload_mut()[..payload.len()].copy_from_slice(payload);
        }
        Ok(record)
    }

    /// Decode one record without applying queue-admission mutations.
    pub fn decode_exact(bytes: &[u8]) -> Result<Self, CommandRecordError> {
        let bytes = <[u8; COMMAND_RECORD_LEN]>::try_from(bytes).map_err(|_| {
            CommandRecordError::InvalidLength {
                expected: COMMAND_RECORD_LEN,
                actual: bytes.len(),
            }
        })?;
        Ok(Self { bytes })
    }

    /// Admit one wire/replay record into the synchronized command queue.
    ///
    /// Admission is intentionally structural: any opcode, house byte, frame,
    /// and payload are preserved. Only the processed marker is cleared.
    pub fn admit_exact(bytes: &[u8]) -> Result<Self, CommandRecordError> {
        let mut record = Self::decode_exact(bytes)?;
        record.clear_processed();
        Ok(record)
    }

    #[inline]
    pub fn opcode(&self) -> u8 {
        self.bytes[COMMAND_RECORD_OPCODE_OFFSET]
    }

    #[inline]
    pub fn flags(&self) -> u8 {
        self.bytes[COMMAND_RECORD_FLAGS_OFFSET]
    }

    #[inline]
    pub fn house_id(&self) -> i8 {
        self.bytes[COMMAND_RECORD_HOUSE_OFFSET] as i8
    }

    #[inline]
    pub fn frame_stamp(&self) -> i32 {
        i32::from_le_bytes(
            self.bytes[COMMAND_RECORD_FRAME_OFFSET..COMMAND_RECORD_PAYLOAD_OFFSET]
                .try_into()
                .expect("frame stamp is four bytes"),
        )
    }

    #[inline]
    pub fn payload(&self) -> &[u8] {
        &self.bytes[COMMAND_RECORD_PAYLOAD_OFFSET..]
    }

    #[inline]
    pub fn payload_mut(&mut self) -> &mut [u8] {
        &mut self.bytes[COMMAND_RECORD_PAYLOAD_OFFSET..]
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8; COMMAND_RECORD_LEN] {
        &self.bytes
    }

    #[cfg(any(test, feature = "test-support"))]
    #[inline]
    pub fn into_bytes(self) -> [u8; COMMAND_RECORD_LEN] {
        self.bytes
    }

    /// Apply the header writes made when a local command is issued.
    ///
    /// Flags and payload remain untouched, including for a null event.
    pub fn set_issue_header(&mut self, opcode: u8, house_id: i32, frame_stamp: i32) {
        if house_id < 0 {
            self.bytes[COMMAND_RECORD_OPCODE_OFFSET] = 0;
            self.bytes[COMMAND_RECORD_HOUSE_OFFSET] = u8::MAX;
        } else {
            self.bytes[COMMAND_RECORD_OPCODE_OFFSET] = opcode;
            self.bytes[COMMAND_RECORD_HOUSE_OFFSET] = house_id as u8;
        }
        self.set_frame_stamp(frame_stamp);
    }

    #[inline]
    pub fn set_frame_stamp(&mut self, frame_stamp: i32) {
        self.bytes[COMMAND_RECORD_FRAME_OFFSET..COMMAND_RECORD_PAYLOAD_OFFSET]
            .copy_from_slice(&frame_stamp.to_le_bytes());
    }

    #[inline]
    pub fn set_house_id(&mut self, house_id: i8) {
        self.bytes[COMMAND_RECORD_HOUSE_OFFSET] = house_id as u8;
    }

    #[inline]
    pub fn is_processed(&self) -> bool {
        self.flags() & COMMAND_RECORD_PROCESSED_FLAG != 0
    }

    #[inline]
    pub fn mark_processed(&mut self) {
        self.bytes[COMMAND_RECORD_FLAGS_OFFSET] |= COMMAND_RECORD_PROCESSED_FLAG;
    }

    #[inline]
    pub fn clear_processed(&mut self) {
        self.bytes[COMMAND_RECORD_FLAGS_OFFSET] &= !COMMAND_RECORD_PROCESSED_FLAG;
    }
}

/// Typed view of native opcode `0x17`'s fixed record fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SellWallAtCellRecord {
    pub house_id: i8,
    pub frame: u32,
    pub x: i16,
    pub y: i16,
}

impl SellWallAtCellRecord {
    pub fn encode(self) -> Result<CommandRecord, CommandRecordError> {
        let mut payload = [0u8; 4];
        payload[..2].copy_from_slice(&self.x.to_le_bytes());
        payload[2..].copy_from_slice(&self.y.to_le_bytes());
        CommandRecord::encode(
            SELL_WALL_AT_CELL_OPCODE,
            i32::from(self.house_id),
            self.frame as i32,
            &payload,
        )
    }

    pub fn decode(record: &CommandRecord) -> Option<Self> {
        if record.opcode() != SELL_WALL_AT_CELL_OPCODE {
            return None;
        }
        Some(Self {
            house_id: record.house_id(),
            frame: record.frame_stamp() as u32,
            x: i16::from_le_bytes(record.payload()[..2].try_into().ok()?),
            y: i16::from_le_bytes(record.payload()[2..4].try_into().ok()?),
        })
    }
}

/// Typed view of native opcode `0x13`'s header-only EXIT event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitRecord {
    pub house_id: i8,
    pub frame: u32,
}

impl ExitRecord {
    pub fn encode(self) -> Result<CommandRecord, CommandRecordError> {
        CommandRecord::encode(
            EXIT_OPCODE,
            i32::from(self.house_id),
            self.frame as i32,
            &[],
        )
    }

    pub fn decode(record: &CommandRecord) -> Option<Self> {
        (record.opcode() == EXIT_OPCODE).then(|| Self {
            house_id: record.house_id(),
            frame: record.frame_stamp() as u32,
        })
    }
}

/// Target event used by Scatter. Original `EventClass` constructor4C65E0
/// writes the Abstract token at Event+7..11, preserving flags and unused tail.
/// Its signed house check uses the existing null-header owner, including
/// negative-house/positive-UID and positive-house/negative-UID controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScatterRecord {
    pub house_id: i8,
    pub frame: i32,
    pub entity_id: i32,
}

impl ScatterRecord {
    pub fn write_into(self, record: &mut CommandRecord) {
        record.set_issue_header(SCATTER_OPCODE, i32::from(self.house_id), self.frame);
        if self.house_id < 0 {
            return;
        }
        let (value, kind) = MegaMissionTarget::Object { id: self.entity_id }
            .token()
            .expect("an Abstract token has no Cell conversion");
        record.payload_mut()[..4].copy_from_slice(&value.to_le_bytes());
        record.payload_mut()[4] = kind;
    }

    pub fn decode(record: &CommandRecord) -> Option<Self> {
        if record.opcode() != SCATTER_OPCODE {
            return None;
        }
        let value = i32::from_le_bytes(record.payload()[..4].try_into().ok()?);
        let MegaMissionTarget::Object { id } =
            MegaMissionTarget::from_token(value, record.payload()[4])?
        else {
            return None;
        };
        Some(Self {
            house_id: record.house_id(),
            frame: record.frame_stamp(),
            entity_id: id,
        })
    }
}

/// The represented ordinary orders of native MegaMission opcode `0x04`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MegaMissionOrder {
    Attack {
        target: MegaMissionTarget,
    },
    AttackMove {
        target: MegaMissionTarget,
        destination: MegaMissionTarget,
    },
    Move {
        target_x: i16,
        target_y: i16,
    },
    /// Move with an object as its destination token, which the object
    /// receiver `0x004D74E0` sends (`0x004D7C76`).
    MoveToObject {
        id: i32,
    },
    AreaGuard {
        post: MegaMissionTarget,
    },
    /// FootClicked4D76F6..4D77A8 and4D7E8E send Enter/Capture/Eaten,
    /// Sabotage and Harvest with a NULL target and a destination token.
    /// Player_Send_Command6FFC23..6FFCA9 preserves them in Planning Mode.
    /// Building443509 also sends Unload with self as Target; mobile
    /// self-click4D75E1 sends Unload with both tokens NULL.
    Mission {
        mission: crate::sim::mission::MissionType,
        target: MegaMissionTarget,
        destination: MegaMissionTarget,
    },
}

/// A TargetClass token used by an ordinary Move or Area Guard order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MegaMissionTarget {
    Null,
    Cell { x: i16, y: i16 },
    Object { id: i32 },
}

impl MegaMissionTarget {
    pub(crate) fn token(self) -> Result<(i32, u8), MegaMissionCellTokenError> {
        match self {
            Self::Null => Ok((0, NULL_TARGET_KIND)),
            Self::Cell { x, y } => Ok((encode_megamission_cell_token(x, y)?, CELL_TARGET_KIND)),
            Self::Object { id } => Ok((id, ABSTRACT_OBJECT_TARGET_KIND)),
        }
    }

    pub(crate) fn from_token(value: i32, kind: u8) -> Option<Self> {
        match kind {
            NULL_TARGET_KIND if value == 0 => Some(Self::Null),
            CELL_TARGET_KIND => {
                let (x, y) = decode_megamission_cell_token(value)?;
                Some(Self::Cell { x, y })
            }
            ABSTRACT_OBJECT_TARGET_KIND => Some(Self::Object { id: value }),
            _ => None,
        }
    }
}

/// Typed view of the ordinary Move and Area Guard MegaMission records.
///
/// `EventClass__BuildMegaMissionEnvelope` at `gamemd.exe` `0x004C6860`
/// writes a byte mission, leaving Event+0D padding untouched. It also leaves
/// the processed flag and payload tail untouched. The original constructor
/// comparisons in `tools/input_oracle/area_guard.json` cover both orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MegaMissionRecord {
    pub house_id: i8,
    pub frame: i32,
    pub source_id: i32,
    pub order: MegaMissionOrder,
}

impl MegaMissionRecord {
    /// Write the active-YR ordinary order fields into an existing record.
    ///
    /// The native null-issuer arm writes only opcode zero, house `-1`, and the
    /// frame. It returns before validating or touching any MegaMission payload.
    pub fn write_into(self, record: &mut CommandRecord) -> Result<(), MegaMissionCellTokenError> {
        record.set_issue_header(MEGAMISSION_OPCODE, i32::from(self.house_id), self.frame);
        if self.house_id < 0 {
            return Ok(());
        }

        // Player_Send_Command6FFBE0 ->646E90 places a Guard post in the
        // first token. Move places its cell in the second token instead.
        let (action, target, destination) = match self.order {
            MegaMissionOrder::Attack { target } => (1, target, MegaMissionTarget::Null),
            MegaMissionOrder::AttackMove {
                target,
                destination,
            } => (29, target, destination),
            MegaMissionOrder::Move { target_x, target_y } => (
                MOVE_ACTION,
                MegaMissionTarget::Null,
                MegaMissionTarget::Cell {
                    x: target_x,
                    y: target_y,
                },
            ),
            MegaMissionOrder::MoveToObject { id } => (
                MOVE_ACTION,
                MegaMissionTarget::Null,
                MegaMissionTarget::Object { id },
            ),
            MegaMissionOrder::AreaGuard { post } => {
                (AREA_GUARD_ACTION, post, MegaMissionTarget::Null)
            }
            MegaMissionOrder::Mission {
                mission,
                target,
                destination,
            } => (mission as u8, target, destination),
        };
        let (target_value, target_kind) = target.token()?;
        let (destination_value, destination_kind) = destination.token()?;
        let payload = &mut record.payload_mut()[..MEGAMISSION_OWNED_PAYLOAD_LEN];
        payload[MEGAMISSION_SOURCE_VALUE_OFFSET..MEGAMISSION_SOURCE_KIND_OFFSET]
            .copy_from_slice(&self.source_id.to_le_bytes());
        payload[MEGAMISSION_SOURCE_KIND_OFFSET] = ABSTRACT_OBJECT_TARGET_KIND;
        payload[MEGAMISSION_ACTION_OFFSET] = action;
        payload[MEGAMISSION_SECONDARY_VALUE_OFFSET..MEGAMISSION_SECONDARY_KIND_OFFSET]
            .copy_from_slice(&target_value.to_le_bytes());
        payload[MEGAMISSION_SECONDARY_KIND_OFFSET] = target_kind;
        payload[MEGAMISSION_DESTINATION_VALUE_OFFSET..MEGAMISSION_DESTINATION_KIND_OFFSET]
            .copy_from_slice(&destination_value.to_le_bytes());
        payload[MEGAMISSION_DESTINATION_KIND_OFFSET] = destination_kind;
        payload[MEGAMISSION_AUXILIARY_VALUE_OFFSET..MEGAMISSION_AUXILIARY_KIND_OFFSET]
            .copy_from_slice(&self.source_id.to_le_bytes());
        payload[MEGAMISSION_AUXILIARY_KIND_OFFSET] = NULL_TARGET_KIND;
        payload[MEGAMISSION_PLANNING_OFFSET] = 0;
        Ok(())
    }

    /// Decode represented ordinary movement, combat and special mission
    /// forms. Planning flags are dispatched by the PlanMgr owner.
    pub fn decode(record: &CommandRecord) -> Option<Self> {
        if record.opcode() != MEGAMISSION_OPCODE || record.house_id() < 0 {
            return None;
        }
        let payload = record.payload();
        let source_id = i32::from_le_bytes(
            payload[MEGAMISSION_SOURCE_VALUE_OFFSET..MEGAMISSION_SOURCE_KIND_OFFSET]
                .try_into()
                .ok()?,
        );
        let action = payload[MEGAMISSION_ACTION_OFFSET];
        let secondary = i32::from_le_bytes(
            payload[MEGAMISSION_SECONDARY_VALUE_OFFSET..MEGAMISSION_SECONDARY_KIND_OFFSET]
                .try_into()
                .ok()?,
        );
        let destination = i32::from_le_bytes(
            payload[MEGAMISSION_DESTINATION_VALUE_OFFSET..MEGAMISSION_DESTINATION_KIND_OFFSET]
                .try_into()
                .ok()?,
        );
        if payload[MEGAMISSION_SOURCE_KIND_OFFSET] != ABSTRACT_OBJECT_TARGET_KIND
            || payload[MEGAMISSION_AUXILIARY_KIND_OFFSET] != NULL_TARGET_KIND
            || payload[MEGAMISSION_PLANNING_OFFSET] != 0
        {
            return None;
        }
        let target =
            MegaMissionTarget::from_token(secondary, payload[MEGAMISSION_SECONDARY_KIND_OFFSET])?;
        let destination = MegaMissionTarget::from_token(
            destination,
            payload[MEGAMISSION_DESTINATION_KIND_OFFSET],
        )?;
        let order = match (action, target, destination) {
            (1, target, MegaMissionTarget::Null) => MegaMissionOrder::Attack { target },
            (29, target, destination) => MegaMissionOrder::AttackMove {
                target,
                destination,
            },
            (MOVE_ACTION, MegaMissionTarget::Null, MegaMissionTarget::Cell { x, y }) => {
                MegaMissionOrder::Move {
                    target_x: x,
                    target_y: y,
                }
            }
            (MOVE_ACTION, MegaMissionTarget::Null, MegaMissionTarget::Object { id }) => {
                MegaMissionOrder::MoveToObject { id }
            }
            (AREA_GUARD_ACTION, post, MegaMissionTarget::Null) => {
                MegaMissionOrder::AreaGuard { post }
            }
            (mission @ (7..=10 | 16 | 17 | 25), target, destination) => MegaMissionOrder::Mission {
                mission: crate::sim::mission::MissionType::from_id(mission)?,
                target,
                destination,
            },
            _ => return None,
        };
        Some(Self {
            house_id: record.house_id(),
            frame: record.frame_stamp(),
            source_id,
            order,
        })
    }
}

fn encode_megamission_cell_token(x: i16, y: i16) -> Result<i32, MegaMissionCellTokenError> {
    let token = i32::from(x) + i32::from(y) * CELL_TOKEN_ROW_STRIDE;
    if decode_megamission_cell_token(token) == Some((x, y)) {
        Ok(token)
    } else {
        Err(MegaMissionCellTokenError { x, y })
    }
}

fn decode_megamission_cell_token(token: i32) -> Option<(i16, i16)> {
    // Native signed division truncates toward zero, as Rust's integer `/` and
    // `%` do. Re-encoding is checked by the writer because mixed-sign or
    // |x|>=1000 coordinates do not have a unique target-token representation.
    let y = i16::try_from(token / CELL_TOKEN_ROW_STRIDE).ok()?;
    let x = i16::try_from(token % CELL_TOKEN_ROW_STRIDE).ok()?;
    (i32::from(x) + i32::from(y) * CELL_TOKEN_ROW_STRIDE == token).then_some((x, y))
}

impl AsRef<[u8]> for CommandRecord {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl TryFrom<&[u8]> for CommandRecord {
    type Error = CommandRecordError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        Self::decode_exact(bytes)
    }
}

/// One gameplay command payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    /// PlanMgr637E00/638120 keeps native command records while editing.
    /// The same record travels through transport, replay and snapshot storage.
    PlanningRecord { record: CommandRecord },
    /// Complete local selection snapshot, committed through ObjectSelect
    /// admission before live Logic. Kept as a command for replay/persistence.
    Select {
        entity_ids: Vec<u64>,
        additive: bool,
    },
    /// Move one entity to a target cell: the class setter (vt+0x480) takes
    /// the cell.
    Move {
        entity_id: u64,
        target_rx: u16,
        target_ry: u16,
    },
    /// Move one entity onto an object, the destination a Move click over an
    /// object keeps (`Simulation::object_move_click`): the class setter
    /// (vt+0x480) takes the object.
    MoveToObject { entity_id: u64, target_id: u64 },
    /// Stop movement/combat intent on one entity.
    Stop { entity_id: u64 },
    /// The Attack event onto an object: a plain or Ctrl click, and the
    /// friendly repair, defuse and Ivan actions. Native encodes them alike.
    Attack { attacker_id: u64, target_id: u64 },
    /// Attack-move: the MEGAMISSION event with mission 29 that ClickedMission
    /// (`0x006FFBE0`) promotes a Move or Attack click to. Its tokens are the
    /// click's: an attack click's object as `target` (`0x004D76B1..0x004D76C1`),
    /// a Move click's Cell or object as `destination` (`0x004D7C76`).
    /// `Assign_Target_Command` stores them as MegaTarget and MegaDestination.
    AttackMove {
        entity_id: u64,
        target: Option<crate::sim::combat::TargetKind>,
        destination: Option<crate::sim::combat::TargetKind>,
    },
    /// Area Guard with the first MegaMission token as its destination/post.
    /// A null post is retained until the Foot handler chooses its own cell.
    Guard {
        entity_id: u64,
        target: Option<crate::sim::combat::TargetKind>,
    },
    /// Unit DEPLOY event: convert an MCV or toggle a SimpleDeployer.
    /// The existing wire spelling is retained for saved commands/replays.
    DeployMcv { entity_id: u64 },
    /// Undeploy a structure back into its mobile unit (e.g. ConYard → MCV).
    /// Reads UndeploysInto from rules.ini to determine the spawned unit type.
    UndeployBuilding { entity_id: u64 },
    /// Archive a rally cell on each selected factory the command owner owns.
    SetRally {
        rx: u16,
        ry: u16,
        producer_ids: Vec<u64>,
    },
    // The production commands act on the envelope's house, as a native
    // production event acts on its header's house id (`EventClass::Execute
    // 0x004C6CB0` reads it at `0x004C6CC4`); none names another.
    /// PRODUCE (`EventClass::Execute` type 14 → `HouseClass::Begin_Production
    /// @ 0x004FA350`): start, queue or resume a build of this type.
    QueueProduction { type_id: InternedId },
    /// SUSPEND (type 15 → `HouseClass::Suspend_Production @ 0x004FA910`):
    /// put the category's running build on hold.
    SuspendProduction { category: ProductionCategory },
    /// Cycle the active producer facility of one category.
    CycleProducerFocus { category: ProductionCategory },
    /// Place one completed building that is waiting for placement.
    PlaceReadyBuilding {
        type_id: InternedId,
        rx: u16,
        ry: u16,
    },
    /// ABANDON (type 16) or, with `all`, ABANDON_ALL (type 46) →
    /// `HouseClass::Abandon_Production @ 0x004FAA10`: cancel a queued copy, or
    /// every copy, of this type and then the active build of it.
    CancelProductionByType { type_id: InternedId, all: bool },
    /// Sell a building, refunding a percentage of its cost and despawning it.
    SellBuilding { entity_id: u64 },
    /// Toggle repair mode on a building (spend credits to heal over time).
    ToggleRepair { entity_id: u64 },
    /// Eject the occupant of a friendly occupied tank bunker. Targets the bunker
    /// (the hidden occupant is not selectable in this slice); the occupant
    /// reappears on a passable cell SW of the bunker and drives out.
    EjectBunker { bunker_id: u64 },
    /// Order a transport or garrison to unload: the Unload mission of its class.
    UnloadPassengers { transport_id: u64 },
    /// The Capture click (action 9) of an Engineer, spy or occupier on a building:
    /// the infantryman walks in, and on arrival `InfantryClass::PerCellProcess`
    /// captures, repairs, infiltrates or garrisons it (`sim::movement::per_cell`).
    CaptureBuilding {
        /// Historical field name: the Capture actor, including a GI/Conscript.
        engineer_id: u64,
        target_building_id: u64,
    },
    /// Order a C4 infantryman (Navy SEAL, Tanya, Psi-Corps Trooper) to
    /// sabotage an enemy building: Sabotage, with the building as its
    /// destination. On arrival `InfantryClass::Per_Cell_Process` plants the
    /// charge (`sim::c4_plant`), and `C4Delay` later the building takes its
    /// full health as `C4Warhead=` damage. Gating happens in `world_commands`:
    /// the attacker must be `C4=yes`, and the target a `CanC4=yes` building
    /// that is not invisible in game, Iron Curtained or fogged.
    PlantC4 {
        attacker_id: u64,
        target_building_id: u64,
    },
    /// Fire a superweapon at a target cell: the SPECIAL_PLACE event, which
    /// runs `HouseClass::Fire_SW` (`Simulation::fire_super_weapon`; its
    /// ClickFire admits the Super).
    LaunchSuperWeapon {
        sw_type_id: InternedId,
        target_rx: u16,
        target_ry: u16,
    },
    /// Toggle an infantry unit's deploy-fire state.
    ///
    /// Three transitions:
    /// - `None → Deploying` (start deploy animation)
    /// - `Deployed → Undeploying` (start undeploy animation)
    /// - mid-transition (Deploying / Undeploying) → no-op (matches gamemd)
    ///
    /// Silently no-op if the entity's type is not `DeployFire=yes`.
    ToggleInfantryDeploy { entity_id: u64 },
    /// The Attack event with a Cell target (Ctrl + left-click on the
    /// ground). The input orders it only for a receiver whose What_Action
    /// answers Attack there, so never for an unarmed unit; the event itself
    /// does not test the weapon.
    ForceAttackCell {
        attacker_id: u64,
        target_rx: u16,
        target_ry: u16,
    },
    /// Sell the wall overlay at one native signed `CellStruct` coordinate.
    /// The command envelope names the receiver, not necessarily the wall owner.
    SellWallAtCell { x: i16, y: i16 },
    /// Execute the local player's native EXIT event and end the active scenario.
    ExitMatch,
    /// Apply an offline in-game Options speed transition before the next logic
    /// frame. The dialog emits only the stored range 0..=6; live network opcode
    /// 0x0D keeps its separate EventClass-tail timing when networking is added.
    SetGameSpeed { speed: u8 },
    /// Strip6A8EB8's mobile PLACE event. House4FB0E0 resolves the category's
    /// current completed head at execution; native carries type_index=-1,
    /// with no product identity or nonce. Appended to preserve saved indices.
    PlaceProducedMobile { category: ProductionCategory },
    /// Event7: Scatter(NULL, forced=true, no_kidding=false). Appended so
    /// existing saved/replayed command variant indices remain unchanged.
    Scatter { entity_id: u64 },
    /// PRIMARY17: toggle the named live Building's retained primary flag
    /// (`EventClass4C718C` -> `Building448160`). A normal self-click emits it
    /// only for a nonprimary factory; a queued duplicate can toggle it off.
    /// Appended to preserve existing saved/replayed command variant indices.
    TogglePrimaryFactory { entity_id: u64 },
    /// Event2D: execute House50C8C0. Input rate admission is local, not replayed.
    Cheer,
    /// Decoded special MEGAMISSION. The shared Event4C72xx
    /// receiver owns admission and class setters; input capability checks
    /// have already run. Also used by committed Planning Mode records.
    /// Appended to preserve existing serialized command variant indices.
    MegaMission {
        entity_id: u64,
        mission: crate::sim::mission::MissionType,
        target: Option<crate::sim::combat::TargetKind>,
        destination: Option<crate::sim::combat::TargetKind>,
    },
    /// EVENT_DESTRUCT for the command House: the in-game abort's Leave and
    /// Observe results 3/4 (`0x0048CA34`, `0x0048CAB2`) queue it for the
    /// local House. Appended to preserve existing serialized variant indices.
    Destruct,
    /// EVENT_REMOVEPLAYER for the House at registration index `house_index`
    /// (`EventClass::Execute 0x004C7B73`): Flag_To_Die when it is human.
    /// Appended to preserve existing serialized variant indices.
    RemovePlayer { house_index: i32 },
    /// The MoveFlash a Move click on a Cell spawns there
    /// (`FootClass::ClickedAction_Cell`, `0x004D7EB5..0x004D8065`). Native
    /// constructs it at the click, outside the event queue; VERA carries it
    /// as a command so the anim stays a product of the input stream.
    /// Appended to preserve existing serialized variant indices.
    MoveFlash { rx: u16, ry: u16 },
    /// EVENT_ALLY toward the House at registration index `house_index`
    /// (`EventClass::Execute 0x004C6DEE`): the command House breaks an
    /// alliance it holds, else offers one, announced. Appended to preserve
    /// existing serialized variant indices.
    Ally { house_index: i32 },
    /// VERA extension: the saved one-use source selection, not native
    /// SPECIAL_PLACE. Appended to preserve existing command discriminants.
    CompletePaidSuperWeapon {
        selection: crate::sim::superweapon::PaidSuperWeaponSelection,
        target_rx: u16,
        target_ry: u16,
    },
    CancelPaidSuperWeapon {
        selection: crate::sim::superweapon::PaidSuperWeaponSelection,
    },
    BeginPaidSuperWeapon {
        selection: crate::sim::superweapon::PaidSuperWeaponSelection,
        target_rx: u16,
        target_ry: u16,
    },
    /// The live GAMESPEED event (native opcode 0x0D, `EventClass::Execute`
    /// type 13 at `0x004C794B`): store the new GameSpeed setting
    /// (`0x00A8EB60`). Unlike offline `SetGameSpeed` it applies at the
    /// EventClass tail. Residuals: native stores the dword unclamped, while
    /// `apply_in_game_speed` refuses values above the Options range (only a
    /// crafted record carries one), and native's `TXT_PLAYER_CHANGED_SPEED`
    /// message for a remote sender is not posted. Appended to preserve
    /// existing serialized variant indices.
    LiveGameSpeed { speed: u8 },
    /// Toggle one Building's On/Off Line latch (native opcodes 0x01/0x02,
    /// `EventClass::Execute` branches `0x004C6CF0`/`0x004C6D4D` -> GoOnline
    /// `0x00452260` / GoOffline `0x00452360`). The direction rides `on`.
    /// Native admits only a live (`+0x90`), non-limbo (`+0x81`) object whose
    /// owner's `+0x16C0` byte is clear; that House byte is not ported.
    /// Appended to preserve existing serialized variant indices.
    SetBuildingOnLine { entity_id: u64, on: bool },
    /// ARCHIVE (native opcode 0x1E) naming an object: a non-factory
    /// building's Move click over an object (`BuildingClass::ClickedAction
    /// @ 0x00443410`, `0x0044363C..0x00443688`) archives that object.
    /// `EventClass::Execute` (`0x004C6DAA..0x004C6DDE`) runs
    /// `Set_ArchiveTarget @ 0x0070C610` on a live (`+0x90`) building; a
    /// target gone by then resolves to null and clears the archive.
    /// Appended to preserve existing serialized variant indices.
    ArchiveObject { entity_id: u64, target_id: u64 },
}

impl Command {
    /// The MEGAMISSION `FootClass::ClickedAction` sends with no Target and
    /// `destination` as its Destination: Enter or Eaten on a clicked object
    /// (`0x004D74E0`; a depot, refinery, bunker, transport or Grinder) and
    /// Harvest on a clicked ore cell (`0x004D7D50` case 6).
    pub fn clicked_mission(
        entity_id: u64,
        mission: crate::sim::mission::MissionType,
        destination: crate::sim::combat::TargetKind,
    ) -> Self {
        Self::MegaMission {
            entity_id,
            mission,
            target: None,
            destination: Some(destination),
        }
    }

    /// Actor of an ordinary MegaMission DTO. Scheduling and PlanningMode
    /// cancellation share this classification; internal mission changes do not
    /// pass through it. Event4C72xx clears planning tokens before retasking.
    pub(crate) fn megamission_actor(&self) -> Option<u64> {
        match self {
            Self::Move { entity_id, .. }
            | Self::MoveToObject { entity_id, .. }
            | Self::AttackMove { entity_id, .. }
            | Self::Guard { entity_id, .. }
            | Self::MegaMission { entity_id, .. }
            | Self::ToggleInfantryDeploy { entity_id } => Some(*entity_id),
            Self::Attack { attacker_id, .. }
            | Self::ForceAttackCell { attacker_id, .. }
            | Self::PlantC4 { attacker_id, .. } => Some(*attacker_id),
            Self::EjectBunker { bunker_id } => Some(*bunker_id),
            Self::UnloadPassengers { transport_id } => Some(*transport_id),
            Self::CaptureBuilding { engineer_id, .. } => Some(*engineer_id),
            _ => None,
        }
    }
}

/// Command with deterministic execution metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandEnvelope {
    pub owner: InternedId,
    pub execute_tick: u64,
    pub payload: Command,
}

impl CommandEnvelope {
    pub fn new(owner: InternedId, execute_tick: u64, payload: Command) -> Self {
        Self {
            owner,
            execute_tick,
            payload,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ABSTRACT_OBJECT_TARGET_KIND, COMMAND_RECORD_LEN, COMMAND_RECORD_PAYLOAD_LEN, CommandRecord,
        CommandRecordError, EXIT_OPCODE, ExitRecord, MOVE_ACTION, MegaMissionCellTokenError,
        MegaMissionOrder, MegaMissionRecord, MegaMissionTarget, NULL_TARGET_KIND,
        SELL_WALL_AT_CELL_OPCODE, SellWallAtCellRecord,
    };

    fn native_record(value: &serde_json::Value) -> CommandRecord {
        let hex = value.as_str().unwrap();
        assert!(hex.is_ascii());
        assert_eq!(hex.len(), COMMAND_RECORD_LEN * 2);
        let bytes: Vec<_> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        CommandRecord::decode_exact(&bytes).unwrap()
    }

    #[test]
    fn destination_missions_preserve_native_constructor_and_planning_fields() {
        let controls =
            crate::test_fixture::json("tools/spatial_oracle/planning_mission_events.json");
        for row in controls.as_array().unwrap() {
            let original = native_record(&row["constructed"]);
            let planned = native_record(&row["planned"]);
            assert_eq!(original.payload()[22], 0);
            assert_eq!(planned.payload()[22], 1);
            assert!(MegaMissionRecord::decode(&planned).is_none());
            let decoded = MegaMissionRecord::decode(&original).expect("native client mission");
            assert_eq!(
                (decoded.house_id, decoded.source_id, decoded.frame),
                (3, 101, 55)
            );
            let mission = row["mission"].as_u64().unwrap() as u8;
            if matches!(mission, 7..=10 | 16 | 17 | 25) {
                let destination = match row["destination"][1].as_u64().unwrap() {
                    0 => MegaMissionTarget::Null,
                    11 => MegaMissionTarget::Cell { x: 5, y: 6 },
                    0x34 => MegaMissionTarget::Object { id: 202 },
                    _ => unreachable!(),
                };
                assert_eq!(
                    decoded.order,
                    MegaMissionOrder::Mission {
                        mission: crate::sim::mission::MissionType::from_id(mission).unwrap(),
                        target: if row["target"][1].as_u64().unwrap() == 0 {
                            MegaMissionTarget::Null
                        } else {
                            MegaMissionTarget::Object {
                                id: row["target"][0].as_i64().unwrap() as i32,
                            }
                        },
                        destination,
                    }
                );
            }
            let mut written = original.clone();
            decoded.write_into(&mut written).unwrap();
            assert_eq!(&written.as_bytes()[..24], &original.as_bytes()[..24]);
            // A NULL auxiliary token ignores its numeric value. The existing
            // codec retains a source-template UID there (Connect63684F);
            // this original control supplies0. Compare kind/flag/tail too.
            assert_eq!(&written.as_bytes()[28..], &original.as_bytes()[28..]);
        }
    }

    #[test]
    fn area_guard_native_constructor_matches_ordinary_bytes_and_null_issuer() {
        // Executed original EventClass constructor 4C6860. The before/after
        // records preserve all 111 bytes, including mission padding at +0D.
        // Raw large-Cell controls remain outside the ordinary typed codec.
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/input_oracle/area_guard.json",
        ))
        .unwrap();
        let rows = corpus["constructor_rows"].as_array().unwrap();
        let record = native_record;
        let mut seen = std::collections::BTreeSet::new();
        let mut ordinary = 0;
        let mut negative = 0;
        let mut excluded = 0;
        for row in rows {
            let name = row["name"].as_str().unwrap();
            assert!(seen.insert(name), "duplicate native constructor row {name}");
            assert_eq!(row["returned_receiver"], true, "{name}");
            let before = record(&row["before"]);
            let native = record(&row["after"]);
            assert_ne!(before.as_bytes()[0x0d], 0, "nonzero padding control");
            assert_eq!(
                native.as_bytes()[0x0d],
                before.as_bytes()[0x0d],
                "{name}: original +0D padding"
            );
            let args: [u32; 10] = serde_json::from_value(row["args"].clone()).unwrap();
            let order = match name {
                "move" | "guard" => {
                    // These supplied raw Cell values cannot be represented by
                    // the codec's signed coordinates. Do not compare an
                    // invented typed substitute with their constructor bytes.
                    assert_eq!(MegaMissionRecord::decode(&native), None, "{name}");
                    excluded += 1;
                    continue;
                }
                // The ordinary corpus declares Cell token50087, i.e.87,50.
                // Expected wire bytes always come from native execution.
                "ordinary_move" => MegaMissionOrder::Move {
                    target_x: 87,
                    target_y: 50,
                },
                "ordinary_guard_cell" => MegaMissionOrder::AreaGuard {
                    post: MegaMissionTarget::Cell { x: 87, y: 50 },
                },
                "ordinary_guard_object" => MegaMissionOrder::AreaGuard {
                    post: MegaMissionTarget::Object {
                        id: i32::from_le_bytes(args[4].to_le_bytes()),
                    },
                },
                "ordinary_guard_null" => MegaMissionOrder::AreaGuard {
                    post: MegaMissionTarget::Null,
                },
                // Negative issuers return before reading the raw tokens. This
                // compares that early-return record only; deliberately invalid
                // typed coordinates also guard against premature validation.
                "move_null_house" => MegaMissionOrder::Move {
                    target_x: i16::MAX,
                    target_y: i16::MIN,
                },
                "guard_null_house" => MegaMissionOrder::AreaGuard {
                    post: MegaMissionTarget::Cell {
                        x: i16::MAX,
                        y: i16::MIN,
                    },
                },
                _ => panic!("uncovered native constructor row {name}"),
            };
            let typed = MegaMissionRecord {
                house_id: i8::try_from(i32::from_le_bytes(args[0].to_le_bytes())).unwrap(),
                frame: i32::try_from(row["frame"].as_i64().unwrap()).unwrap(),
                source_id: i32::from_le_bytes(args[1].to_le_bytes()),
                order,
            };
            let mut actual = before.clone();
            typed.write_into(&mut actual).unwrap();
            assert_eq!(
                actual.as_bytes(),
                native.as_bytes(),
                "{name}: full native record"
            );
            if typed.house_id < 0 {
                assert_eq!(native.payload(), before.payload(), "{name}: opaque payload");
                assert_eq!(MegaMissionRecord::decode(&native), None, "{name}");
                negative += 1;
            } else {
                assert_eq!(MegaMissionRecord::decode(&native), Some(typed), "{name}");
                ordinary += 1;
            }
        }
        assert_eq!((seen.len(), ordinary, negative, excluded), (8, 4, 2, 2));
    }

    #[test]
    fn gsi_16_01_signed_cell_tokens_require_an_exact_native_roundtrip() {
        for (x, y) in [(999, i16::MAX), (-999, i16::MIN), (-999, 0), (0, 0)] {
            let mut record = CommandRecord::decode_exact(&[0; COMMAND_RECORD_LEN]).unwrap();
            let typed = MegaMissionRecord {
                house_id: 0,
                frame: 1,
                source_id: 7,
                order: MegaMissionOrder::Move {
                    target_x: x,
                    target_y: y,
                },
            };
            typed.write_into(&mut record).unwrap();
            assert_eq!(MegaMissionRecord::decode(&record), Some(typed));
        }

        let mut record = CommandRecord::decode_exact(&[0; COMMAND_RECORD_LEN]).unwrap();
        assert_eq!(
            MegaMissionRecord {
                house_id: 0,
                frame: 1,
                source_id: 7,
                order: MegaMissionOrder::Move {
                    target_x: 1000,
                    target_y: 0
                },
            }
            .write_into(&mut record),
            Err(MegaMissionCellTokenError { x: 1000, y: 0 })
        );
    }

    #[test]
    fn gsi_16_01_megamission_move_rejects_wrong_tokens_action_and_planning() {
        let mut valid = CommandRecord::decode_exact(&[0; COMMAND_RECORD_LEN]).unwrap();
        MegaMissionRecord {
            house_id: 0,
            frame: 1,
            source_id: 7,
            order: MegaMissionOrder::Move {
                target_x: 10,
                target_y: 20,
            },
        }
        .write_into(&mut valid)
        .unwrap();

        for (offset, value) in [(4, 0x33), (11, 1), (16, 0x0a), (21, 1), (22, 1)] {
            let mut invalid = valid.clone();
            invalid.payload_mut()[offset] = value;
            assert_eq!(MegaMissionRecord::decode(&invalid), None);
        }
        let mut wrong_action = valid.clone();
        wrong_action.payload_mut()[5..7].copy_from_slice(&3_i16.to_le_bytes());
        assert_eq!(MegaMissionRecord::decode(&wrong_action), None);
        let mut wrong_repeat = valid.clone();
        wrong_repeat.payload_mut()[17..21].copy_from_slice(&8_i32.to_le_bytes());
        // Connect63684F rewrites only source. A null auxiliary token's value
        // remains the source template's identity and is semantically ignored.
        assert_eq!(
            MegaMissionRecord::decode(&wrong_repeat),
            MegaMissionRecord::decode(&valid)
        );
    }

    /// The object receiver `0x004D74E0` sends Move with the object as its
    /// destination token (`0x004D7C76`), which the constructor writes in the
    /// slot a Move cell takes. A Move with no destination stays outside the
    /// represented forms.
    #[test]
    fn megamission_move_to_object_carries_an_abstract_destination_token() {
        let mut record = CommandRecord::decode_exact(&[0xcc; COMMAND_RECORD_LEN]).unwrap();
        let typed = MegaMissionRecord {
            house_id: 1,
            frame: 9,
            source_id: 7,
            order: MegaMissionOrder::MoveToObject { id: 0x0102_0304 },
        };
        typed.write_into(&mut record).unwrap();
        let bytes = record.as_bytes();
        assert_eq!(bytes[12], MOVE_ACTION);
        assert_eq!(&bytes[14..18], &0_i32.to_le_bytes());
        assert_eq!(bytes[18], NULL_TARGET_KIND);
        assert_eq!(&bytes[19..23], &0x0102_0304_i32.to_le_bytes());
        assert_eq!(bytes[23], ABSTRACT_OBJECT_TARGET_KIND);
        assert_eq!(MegaMissionRecord::decode(&record), Some(typed));

        record.payload_mut()[12..16].copy_from_slice(&0_i32.to_le_bytes());
        record.payload_mut()[16] = NULL_TARGET_KIND;
        assert_eq!(MegaMissionRecord::decode(&record), None);
    }

    #[test]
    fn gsi_04_07_wall_sell_raw_record_golden_and_signed_roundtrip() {
        let typed = SellWallAtCellRecord {
            house_id: 3,
            frame: 0x89ab_cdef,
            x: -2,
            y: 0x1234,
        };
        let record = typed.encode().expect("wall-sale record");
        let bytes = record.as_bytes();
        assert_eq!(bytes[0], SELL_WALL_AT_CELL_OPCODE);
        assert_eq!(bytes[1], 0);
        assert_eq!(bytes[2], 3);
        assert_eq!(&bytes[3..7], &0x89ab_cdef_u32.to_le_bytes());
        assert_eq!(&bytes[7..9], &(-2_i16).to_le_bytes());
        assert_eq!(&bytes[9..11], &0x1234_i16.to_le_bytes());
        assert!(bytes[11..].iter().all(|&byte| byte == 0));
        assert_eq!(SellWallAtCellRecord::decode(&record), Some(typed));

        let other = CommandRecord::encode(0x16, 3, 1, &[]).unwrap();
        assert_eq!(SellWallAtCellRecord::decode(&other), None);
    }

    #[test]
    fn gsi_01_04_exit_raw_record_is_header_only_opcode_13() {
        let typed = ExitRecord {
            house_id: 2,
            frame: 0x89ab_cdef,
        };
        let record = typed.encode().expect("EXIT record");
        let bytes = record.as_bytes();

        assert_eq!(bytes[0], EXIT_OPCODE);
        assert_eq!(bytes[1], 0);
        assert_eq!(bytes[2], 2);
        assert_eq!(&bytes[3..7], &0x89ab_cdef_u32.to_le_bytes());
        assert!(bytes[7..].iter().all(|&byte| byte == 0));
        assert_eq!(ExitRecord::decode(&record), Some(typed));

        let other = CommandRecord::encode(0x12, 2, 1, &[]).unwrap();
        assert_eq!(ExitRecord::decode(&other), None);
    }

    #[test]
    fn command_record_bincode_roundtrip_preserves_all_fixed_bytes() {
        let mut bytes = [0; COMMAND_RECORD_LEN];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = index as u8;
        }
        let record = CommandRecord::decode_exact(&bytes).unwrap();

        let encoded = crate::util::save_codec::serialize(&record).unwrap();
        assert_eq!(encoded.as_slice(), bytes.as_slice());
        assert_eq!(
            crate::util::save_codec::deserialize::<CommandRecord>(&encoded).unwrap(),
            record
        );
    }

    #[test]
    fn admission_preserves_opaque_bytes_and_clears_only_processed_bit() {
        let mut bytes = [0x7b; COMMAND_RECORD_LEN];
        bytes[0] = 0xff;
        bytes[1] = 0xa5;
        bytes[2] = 0xfe;
        bytes[3..7].copy_from_slice(&(-2_i32).to_le_bytes());

        let decoded = CommandRecord::decode_exact(&bytes).unwrap();
        assert_eq!(decoded.flags(), 0xa5);

        let admitted = CommandRecord::admit_exact(&bytes).unwrap();
        assert_eq!(admitted.opcode(), 0xff);
        assert_eq!(admitted.flags(), 0xa4);
        assert_eq!(admitted.house_id(), -2);
        assert_eq!(admitted.frame_stamp(), -2);
        assert_eq!(admitted.payload(), &bytes[7..]);
    }

    #[test]
    fn record_and_payload_widths_are_enforced_exactly() {
        assert_eq!(
            CommandRecord::decode_exact(&[0; COMMAND_RECORD_LEN - 1]),
            Err(CommandRecordError::InvalidLength {
                expected: COMMAND_RECORD_LEN,
                actual: COMMAND_RECORD_LEN - 1,
            })
        );
        assert_eq!(
            CommandRecord::decode_exact(&[0; COMMAND_RECORD_LEN + 1]),
            Err(CommandRecordError::InvalidLength {
                expected: COMMAND_RECORD_LEN,
                actual: COMMAND_RECORD_LEN + 1,
            })
        );
        assert_eq!(
            CommandRecord::encode(4, 0, 0, &[0; COMMAND_RECORD_PAYLOAD_LEN + 1]),
            Err(CommandRecordError::PayloadTooLong {
                max: COMMAND_RECORD_PAYLOAD_LEN,
                actual: COMMAND_RECORD_PAYLOAD_LEN + 1,
            })
        );
    }
}
