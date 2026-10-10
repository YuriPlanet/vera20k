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
/// Native `EventClass` opcode for a MegaMission order envelope.
pub const MEGAMISSION_OPCODE: u8 = 0x04;

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

    #[cfg(test)]
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

/// The represented ordinary orders of native MegaMission opcode `0x04`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MegaMissionOrder {
    Move { target_x: i16, target_y: i16 },
    AreaGuard { post: MegaMissionTarget },
}

/// A TargetClass token used by an ordinary Area Guard order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MegaMissionTarget {
    Null,
    Cell { x: i16, y: i16 },
    Object { id: i32 },
}

impl MegaMissionTarget {
    fn token(self) -> Result<(i32, u8), MegaMissionCellTokenError> {
        match self {
            Self::Null => Ok((0, NULL_TARGET_KIND)),
            Self::Cell { x, y } => Ok((encode_megamission_cell_token(x, y)?, CELL_TARGET_KIND)),
            Self::Object { id } => Ok((id, ABSTRACT_OBJECT_TARGET_KIND)),
        }
    }

    fn from_token(value: i32, kind: u8) -> Option<Self> {
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
            MegaMissionOrder::Move { target_x, target_y } => (
                MOVE_ACTION,
                MegaMissionTarget::Null,
                MegaMissionTarget::Cell {
                    x: target_x,
                    y: target_y,
                },
            ),
            MegaMissionOrder::AreaGuard { post } => {
                (AREA_GUARD_ACTION, post, MegaMissionTarget::Null)
            }
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

    /// Decode the represented ordinary Move/Area Guard forms. Planning,
    /// attack-move and other token/action shapes remain outside this contract.
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
        let auxiliary = i32::from_le_bytes(
            payload[MEGAMISSION_AUXILIARY_VALUE_OFFSET..MEGAMISSION_AUXILIARY_KIND_OFFSET]
                .try_into()
                .ok()?,
        );
        if payload[MEGAMISSION_SOURCE_KIND_OFFSET] != ABSTRACT_OBJECT_TARGET_KIND
            || auxiliary != source_id
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
            (MOVE_ACTION, MegaMissionTarget::Null, MegaMissionTarget::Cell { x, y }) => {
                MegaMissionOrder::Move {
                    target_x: x,
                    target_y: y,
                }
            }
            (AREA_GUARD_ACTION, post, MegaMissionTarget::Null) => {
                MegaMissionOrder::AreaGuard { post }
            }
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
    /// Selection intent (kept for replay/debug parity; not authoritative sim state yet).
    Select {
        entity_ids: Vec<u64>,
        additive: bool,
    },
    /// Move one entity to a target cell.
    /// Speed is resolved at dispatch time by `movement::order_speed`.
    Move {
        entity_id: u64,
        target_rx: u16,
        target_ry: u16,
        queue: bool,
    },
    /// Stop movement/combat intent on one entity.
    Stop { entity_id: u64 },
    /// Attack an explicit target.
    Attack { attacker_id: u64, target_id: u64 },
    /// Force-attack a target (ignores friendship — Ctrl+click).
    ForceAttack { attacker_id: u64, target_id: u64 },
    /// Attack-move toward a cell (logic can retarget along path).
    AttackMove {
        entity_id: u64,
        target_rx: u16,
        target_ry: u16,
        queue: bool,
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
    /// Force a miner to return to its refinery (right-click on own refinery or 'D' key).
    /// Both kinds run the Harvest return; a Chrono Miner warps onto the pad
    /// from within `ChronoHarvTooFarDistance`.
    MinerReturn {
        entity_id: u64,
        /// Explicit refinery clicked by the player. Keyboard/generic forced
        /// return leaves this empty and lets the miner choose a refinery.
        target_refinery_id: Option<u64>,
    },
    /// Send a unit to a repair depot for repairs.
    /// The unit pathfinds to the depot, docks, and auto-repairs until full HP or out of credits.
    RepairAtDepot { entity_id: u64, depot_id: u64 },
    /// Order an infantry/vehicle to enter a friendly transport or garrisonable building.
    /// The passenger pathfinds to the transport's cell and boards on arrival.
    EnterTransport {
        passenger_id: u64,
        transport_id: u64,
    },
    /// Order a bunkerable vehicle into a friendly `Bunker=yes` building (tank
    /// bunker). The unit pathfinds to the bunker cell; the install machine takes
    /// over on arrival (turn → slide onto the cell → turn south → hide). Gating
    /// happens in `world_commands`: own-owner, the bunker idle/empty, and the
    /// unit `Bunkerable=yes` with a primary weapon.
    EnterBunker { unit_id: u64, bunker_id: u64 },
    /// Eject the occupant of a friendly occupied tank bunker. Targets the bunker
    /// (the hidden occupant is not selectable in this slice); the occupant
    /// reappears on a passable cell SW of the bunker and drives out.
    EjectBunker { bunker_id: u64 },
    /// Order a transport or garrison to unload: the Unload mission of its class.
    UnloadPassengers { transport_id: u64 },
    /// Direct a harvester to go harvest a specific ore cell.
    /// The miner will path to the cell, then enter Harvest state on arrival.
    HarvestCell {
        entity_id: u64,
        target_rx: u16,
        target_ry: u16,
    },
    /// Order an engineer to capture an enemy building.
    /// Engineer walks to the building, instantly transfers ownership on arrival,
    /// and is consumed.
    CaptureBuilding {
        engineer_id: u64,
        target_building_id: u64,
    },
    /// Order a C4-capable infantry (SEAL / Tanya / Psi-Corp Trooper) to plant
    /// on an enemy building. The unit walks to the building's cell; on arrival
    /// the building's `pending_c4_detonation` is set; after `C4Delay` ticks the
    /// building takes full-HP damage with C4Warhead and dies. The attacker
    /// survives and scatters one cell. Gating happens in `world_commands` —
    /// attacker must have `C4=yes`, target must be a `CanC4=yes` building, not
    /// invisible-in-game, not iron-curtained, not in fog.
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
    /// Force-attack on a ground cell (Ctrl + left-click on empty terrain).
    ///
    /// Bypasses friendship check and entity-targeting — fires the attacker's
    /// weapon at the cell's center. Unarmed units must NOT receive this
    /// command; the order-resolution layer routes them to `Move` instead.
    /// Defensive sim-side check in `issue_attack_cell_command` warn-logs and
    /// no-ops if a stray `ForceAttackCell` reaches an unarmed unit.
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
        COMMAND_RECORD_LEN, COMMAND_RECORD_PAYLOAD_LEN, CommandRecord, CommandRecordError,
        EXIT_OPCODE, ExitRecord, MEGAMISSION_OPCODE, MegaMissionCellTokenError, MegaMissionOrder,
        MegaMissionRecord, MegaMissionTarget, SELL_WALL_AT_CELL_OPCODE, SellWallAtCellRecord,
    };

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
        let record = |value: &serde_json::Value| {
            let hex = value.as_str().unwrap();
            assert!(hex.is_ascii());
            assert_eq!(hex.len(), COMMAND_RECORD_LEN * 2);
            let bytes: Vec<_> = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect();
            CommandRecord::decode_exact(&bytes).unwrap()
        };
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
    fn gsi_16_01_area_guard_regression_constructor_preserves_fields_and_padding() {
        let mut bytes = [0xcc; COMMAND_RECORD_LEN];
        bytes[1] = 0xa4;
        let mut record = CommandRecord::decode_exact(&bytes).unwrap();
        let typed = MegaMissionRecord {
            house_id: 3,
            frame: 0x1234_5678,
            source_id: 0x0102_0304,
            order: MegaMissionOrder::Move {
                target_x: 34,
                target_y: 12,
            },
        };

        typed.write_into(&mut record).unwrap();

        let bytes = record.as_bytes();
        assert_eq!(bytes[0], MEGAMISSION_OPCODE);
        assert_eq!(bytes[1], 0xa4, "the codec does not own queue flags");
        assert_eq!(bytes[2], 3);
        assert_eq!(&bytes[3..7], &0x1234_5678_i32.to_le_bytes());
        assert_eq!(&bytes[7..11], &0x0102_0304_i32.to_le_bytes());
        assert_eq!(bytes[11], 0x34);
        assert_eq!(bytes[12], 2);
        assert_eq!(
            bytes[13], 0xcc,
            "original 4C6860 does not write mission padding"
        );
        assert_eq!(&bytes[14..18], &0_i32.to_le_bytes());
        assert_eq!(bytes[18], 0);
        assert_eq!(&bytes[19..23], &12_034_i32.to_le_bytes());
        assert_eq!(bytes[23], 0x0b);
        assert_eq!(&bytes[24..28], &0x0102_0304_i32.to_le_bytes());
        assert_eq!(bytes[28], 0);
        assert_eq!(bytes[29], 0);
        assert!(bytes[30..].iter().all(|&byte| byte == 0xcc));
        assert_eq!(MegaMissionRecord::decode(&record), Some(typed));
    }

    #[test]
    fn gsi_16_01_negative_issuer_leaves_flags_and_payload_untouched() {
        let mut bytes = [0xcc; COMMAND_RECORD_LEN];
        bytes[1] = 0x5a;
        let mut record = CommandRecord::decode_exact(&bytes).unwrap();

        MegaMissionRecord {
            house_id: -1,
            frame: -17,
            source_id: i32::MAX,
            order: MegaMissionOrder::Move {
                target_x: i16::MAX,
                target_y: i16::MIN,
            },
        }
        .write_into(&mut record)
        .unwrap();

        assert_eq!(record.opcode(), 0);
        assert_eq!(record.flags(), 0x5a);
        assert_eq!(record.house_id(), -1);
        assert_eq!(record.frame_stamp(), -17);
        assert!(record.payload().iter().all(|&byte| byte == 0xcc));
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
        assert_eq!(MegaMissionRecord::decode(&wrong_repeat), None);
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
    fn command_record_encoding_uses_the_fixed_native_layout() {
        let record = CommandRecord::encode(0x15, 3, 0x1234_5678, &[0x10, 0x20, 0x30]).unwrap();
        let bytes = record.as_bytes();

        assert_eq!(bytes.len(), 111);
        assert_eq!(bytes[0], 0x15);
        assert_eq!(bytes[1], 0);
        assert_eq!(bytes[2], 3);
        assert_eq!(&bytes[3..7], &[0x78, 0x56, 0x34, 0x12]);
        assert_eq!(&bytes[7..10], &[0x10, 0x20, 0x30]);
        assert!(bytes[10..].iter().all(|byte| *byte == 0));

        let decoded = CommandRecord::decode_exact(bytes).unwrap();
        assert_eq!(decoded, record);
        assert_eq!(decoded.opcode(), 0x15);
        assert_eq!(decoded.house_id(), 3);
        assert_eq!(decoded.frame_stamp(), 0x1234_5678);
    }

    #[test]
    fn command_record_bincode_roundtrip_preserves_all_fixed_bytes() {
        let mut bytes = [0; COMMAND_RECORD_LEN];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = index as u8;
        }
        let record = CommandRecord::decode_exact(&bytes).unwrap();

        let encoded = bincode::serialize(&record).unwrap();
        assert_eq!(encoded.as_slice(), bytes.as_slice());
        assert_eq!(
            bincode::deserialize::<CommandRecord>(&encoded).unwrap(),
            record
        );
    }

    #[test]
    fn negative_house_id_nulls_only_the_issued_header() {
        let mut bytes = [0xcc; COMMAND_RECORD_LEN];
        bytes[1] = 0x5a;
        let mut record = CommandRecord::decode_exact(&bytes).unwrap();

        record.set_issue_header(0x15, -1, 41);

        assert_eq!(record.opcode(), 0);
        assert_eq!(record.flags(), 0x5a);
        assert_eq!(record.house_id(), -1);
        assert_eq!(record.frame_stamp(), 41);
        assert!(record.payload().iter().all(|byte| *byte == 0xcc));

        let encoded = CommandRecord::encode(0x15, -1, 41, &[1, 2, 3]).unwrap();
        assert!(encoded.payload().iter().all(|byte| *byte == 0));
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
    fn processed_marker_mutation_preserves_other_flag_bits() {
        let mut bytes = [0; COMMAND_RECORD_LEN];
        bytes[1] = 0xa4;
        let mut record = CommandRecord::decode_exact(&bytes).unwrap();

        assert!(!record.is_processed());
        record.mark_processed();
        assert!(record.is_processed());
        assert_eq!(record.flags(), 0xa5);
        record.clear_processed();
        assert_eq!(record.flags(), 0xa4);
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
