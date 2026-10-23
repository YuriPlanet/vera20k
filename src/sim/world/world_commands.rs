//! Individual command payload behavior for the Simulation.
//!
//! Contains `apply_command()` and its helper methods: selection snapshots,
//! ownership checks and the MEGAMISSION event arm. `command_schedule` owns
//! queue admission and scheduled batch order.
//!
//! Dependency rules: same as sim/ (depends on rules/, map/; never render/ui/audio/net).

use crate::sim::world::FrameEffects;
use std::collections::BTreeSet;

use super::{SimSoundEvent, Simulation, SimulationWallRuntimeHost};
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_rect::canonical_cell_coord;
use crate::sim::combat;
use crate::sim::command::{
    ArchiveRecord, BUILDING_OFF_LINE_OPCODE, BUILDING_ON_LINE_OPCODE, BuildingOnLineRecord,
    COMMAND_RECORD_LEN, Command, CommandEnvelope, CommandRecord, DeployRecord, ExitRecord,
    GameSpeedRecord, MegaMissionOrder, MegaMissionRecord, MegaMissionTarget, PlaceRecord,
    ProductionEvent, ProductionRecord, RepairRecord, ScatterRecord, SellWallAtCellRecord,
    StopRecord,
};
use crate::sim::house_diplomacy::HouseAlliances;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::mission::MissionType;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::overlay_grid::{
    OverlayRecalcOutcome, RecomputeResult, runtime_wall_cleanup_visit_at,
};
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::zone_incremental::ZoneRepairKind;
use crate::sim::pathfinding::zone_map::ZoneGrid;
use crate::sim::production;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::vision::FogState;
use crate::sim::world::ObjectSubstrate;

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WallSellZoneRepairTestStep {
    pub(crate) repair_cell: (u16, u16),
    pub(crate) walkable_cross: [bool; 5],
    pub(crate) movement_class_cross: [u8; 5],
}

#[cfg(any(test, feature = "test-support"))]
std::thread_local! {
    static WALL_SELL_ZONE_REPAIR_TEST_TRACE:
        std::cell::RefCell<Vec<WallSellZoneRepairTestStep>> = const {
            std::cell::RefCell::new(Vec::new())
        };
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn clear_wall_sell_zone_repair_test_trace() {
    WALL_SELL_ZONE_REPAIR_TEST_TRACE.with(|trace| trace.borrow_mut().clear());
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn take_wall_sell_zone_repair_test_trace() -> Vec<WallSellZoneRepairTestStep> {
    WALL_SELL_ZONE_REPAIR_TEST_TRACE.with(|trace| std::mem::take(&mut *trace.borrow_mut()))
}

#[cfg(any(test, feature = "test-support"))]
fn trace_wall_sell_zone_repair_step(
    zone_grid: &crate::sim::pathfinding::zone_map::ZoneGrid,
    tail_grid: &PathGrid,
    sold_cell: (u16, u16),
    repair_cell: (u16, u16),
) {
    const CROSS: [(i32, i32); 5] = [(0, -1), (1, 0), (0, 1), (-1, 0), (0, 0)];
    let mut walkable_cross = [false; 5];
    let mut movement_class_cross = [crate::map::resolved_terrain::zone_class::OUTSIDE; 5];
    for (index, (dx, dy)) in CROSS.into_iter().enumerate() {
        let x = i32::from(sold_cell.0) + dx;
        let y = i32::from(sold_cell.1) + dy;
        if x < 0 || y < 0 {
            continue;
        }
        let (x, y) = (x as u16, y as u16);
        walkable_cross[index] = tail_grid.is_walkable(x, y);
        movement_class_cross[index] = zone_grid
            .base_movement_class_at(x, y)
            .unwrap_or(crate::map::resolved_terrain::zone_class::OUTSIDE);
    }
    WALL_SELL_ZONE_REPAIR_TEST_TRACE.with(|trace| {
        trace.borrow_mut().push(WallSellZoneRepairTestStep {
            repair_cell,
            walkable_cross,
            movement_class_cross,
        });
    });
}

/// What [`Simulation::apply_megamission_command`] did with an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MegaMissionExecution {
    /// The actor or an object token failed the admission gate: nothing
    /// changed, and the actor keeps its previous order.
    Refused,
    /// A repeated Enter onto the building the actor is already linked to,
    /// consumed before any write (`0x004C729F..0x004C72E2`).
    RepeatedEnter,
    /// The arm ran through its setters.
    Executed,
}

impl MegaMissionExecution {
    /// Whether the event ran, a repeated Enter included.
    fn accepted(self) -> bool {
        self != Self::Refused
    }
}

impl Simulation {
    /// HouseClass::Cheer50C8C0, original gamemd. Trigger113 and Event2D
    /// share this owner. Capture the initial Techno count; re-read each live
    /// ordinal and delegate admission to virtual388/Infantry522C00. There is
    /// no House-level selected, alive, limbo, health or local-player test.
    /// Building7E3EBC/Unit7F5C70 slot388 targets41BF90 (RET4/no effect).
    /// Source: tools/input_oracle/house_cheer.md; existing infantry_cheer.json.
    pub(crate) fn house_cheer(
        &mut self,
        owner: crate::sim::intern::InternedId,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let count = self.substrate.entities.techno_registry_len();
        for ordinal in 0..count {
            let Some(id) = self.substrate.entities.techno_registry_at(ordinal) else {
                continue;
            };
            if self
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.owner() == owner)
            {
                self.infantry_cheer(id, false, rules, effects);
            }
        }
        //50C905/50C90B: centered cue follows every member callback, even
        //when the registry is empty or every infantry refuses the action.
        if let Some(sound_id) = rules.general.cheer_sound() {
            self.sound_events.emit(SimSoundEvent::VocCentered {
                sound_id: sound_id.to_owned(),
            });
            effects.flush_sound(self, rules);
        }
    }

    ///730F5C..730F6C -> shared EventCtor4C66C0. The payload is unused.
    /// The House byte is the registration index, independent of interning.
    pub fn encode_cheer_record(
        &self,
        owner: crate::sim::intern::InternedId,
    ) -> Option<CommandRecord> {
        if !self.houses.contains_key(&owner) {
            return None;
        }
        let index = self
            .houses
            .order()
            .iter()
            .position(|&registered| registered == owner)?;
        let index = i8::try_from(index).ok()?;
        CommandRecord::encode(
            crate::sim::command::CHEER_OPCODE,
            i32::from(index),
            self.session.binary_frame() as i32,
            &[],
        )
        .ok()
    }

    /// ScatterSelectedObjects730FE0 admits Techno vt+A0, without Guard's
    /// IsActive test or miner exception. All retained entities are Technos.
    pub fn scatter_key_command(&self, id: u64, rules: &RuleSet) -> Option<Command> {
        self.techno_player_controllable(id, rules)
            .then_some(Command::Scatter { entity_id: id })
    }

    /// ClickedEvent6FFE00 ->4C65E0 stores issuer registration and actor UID
    /// independently. Ownership is not rechecked by Event7 at execution.
    pub fn encode_scatter_record(
        &self,
        command_owner: crate::sim::intern::InternedId,
        entity_id: u64,
    ) -> Option<CommandRecord> {
        if !self.houses.contains_key(&command_owner)
            || self.substrate.entities.get(entity_id).is_none()
        {
            return None;
        }
        let house_id = self
            .houses
            .order()
            .iter()
            .position(|&owner| owner == command_owner)
            .and_then(|index| i8::try_from(index).ok())?;
        let typed = ScatterRecord {
            house_id,
            frame: self.session.binary_frame() as i32,
            entity_id: i32::try_from(entity_id).ok()?,
        };
        let mut record = CommandRecord::decode_exact(&[0; COMMAND_RECORD_LEN]).ok()?;
        typed.write_into(&mut record);
        Some(record)
    }

    /// StopCommand730EE7 -> ClickedEvent6FFE34 -> EventCtor4C65E0 emits
    /// IDLE6 with the current issuer and the actor's five-byte object token.
    /// Keep the token even if its actor disappears before dispatch: native
    /// Stop4C74CE resolves the identifier only when the event executes.
    /// Source: original gamemd.exe instructions.
    pub fn encode_stop_record(
        &self,
        command_owner: InternedId,
        entity_id: u64,
    ) -> Option<CommandRecord> {
        let entity_id = i32::try_from(entity_id).ok()?;
        let mut record =
            self.encode_header_event(command_owner, crate::sim::command::IDLE_OPCODE)?;
        StopRecord {
            house_id: record.house_id(),
            frame: record.frame_stamp(),
            entity_id,
        }
        .write_into(&mut record);
        Some(record)
    }

    /// D-key ClickedEvent6FFE00 -> EventCtor4C65E0. Preserve a stale UID
    /// until the event executes; the native receiver has no actor-owner gate.
    pub fn encode_deploy_record(
        &self,
        command_owner: InternedId,
        entity_id: u64,
    ) -> Option<CommandRecord> {
        let mut record =
            self.encode_header_event(command_owner, crate::sim::command::DEPLOY_OPCODE)?;
        DeployRecord {
            house_id: record.house_id(),
            frame: record.frame_stamp(),
            entity_id: i32::try_from(entity_id).ok()?,
        }
        .write_into(&mut record);
        Some(record)
    }

    /// Display4AC0D3..4AC198 constructs REPAIR15 with PlayerPtr+30 and
    /// the picked object's token. Preserve that token until execution;
    /// constructor4C65E0 does not read its owner or repair eligibility.
    /// Source: original gamemd.exe instructions.
    pub fn encode_repair_record(
        &self,
        command_owner: InternedId,
        entity_id: u64,
    ) -> Option<CommandRecord> {
        let mut record =
            self.encode_header_event(command_owner, crate::sim::command::REPAIR_OPCODE)?;
        RepairRecord {
            house_id: record.house_id(),
            frame: record.frame_stamp(),
            entity_id: i32::try_from(entity_id).ok()?,
        }
        .write_into(&mut record);
        Some(record)
    }

    /// SetRallyPoint4439C5 and object-click44367B ask the actor's vt+38
    /// (Techno6F9DB0), not PlayerPtr. Preserve that House and both tokens
    /// at issue; send-time retiming remains the packet codec's responsibility.
    /// Source: original gamemd.exe constructor4C6780.
    pub(super) fn encode_archive_record(
        &self,
        entity_id: u64,
        target: MegaMissionTarget,
    ) -> Option<CommandRecord> {
        let owner = self.substrate.entities.get(entity_id)?.owner();
        let mut record = self.encode_header_event(owner, crate::sim::command::ARCHIVE_OPCODE)?;
        ArchiveRecord {
            house_id: record.house_id(),
            frame: record.frame_stamp(),
            entity_id: i32::try_from(entity_id).ok()?,
            target,
        }
        .write_into(&mut record)
        .ok()?;
        Some(record)
    }

    /// Select6AAD00's production events use the clicked type's native array
    /// index, RTTI and Naval flag, captured now rather than when dispatched.
    pub fn encode_production_record(
        &self,
        command_owner: InternedId,
        event: ProductionEvent,
        type_id: InternedId,
        rules: &RuleSet,
    ) -> Option<CommandRecord> {
        let selection =
            production::ProductionSelection::for_type(rules, self.object_type(type_id, rules)?)?;
        let mut record = self.encode_header_event(command_owner, event.opcode())?;
        ProductionRecord {
            house_id: record.house_id(),
            frame: record.frame_stamp(),
            event,
            rtti: selection.rtti,
            type_index: selection.type_index,
            naval: u32::from(selection.naval),
        }
        .write_into(&mut record);
        Some(record)
    }

    /// Display4ABB39..4ABB55 uses the selected type's index and the held
    /// building's object RTTI6. Cell words retain their native signed bits.
    pub fn encode_place_building_record(
        &self,
        command_owner: InternedId,
        type_id: InternedId,
        cell: (u16, u16),
        rules: &RuleSet,
    ) -> Option<CommandRecord> {
        let object = self.object_type(type_id, rules)?;
        if object.category != crate::rules::object_type::ObjectCategory::Building {
            return None;
        }
        let selection = production::ProductionSelection::for_type(rules, object)?;
        self.encode_place_record(
            command_owner,
            production::ProductionSelection {
                rtti: selection.object_rtti(),
                ..selection
            },
            cell,
        )
    }

    /// StripAI6A8EE8 snapshots the complete object's RTTI/Naval, but writes
    /// index-1 and B07DD8's Cell(0,0), initialized by6A4BC0. The receiver
    /// looks up the later held head; (-1,-1) is accepted but not issued here.
    pub fn encode_place_mobile_record(
        &self,
        command_owner: InternedId,
        category: production::ProductionCategory,
        rules: &RuleSet,
    ) -> Option<CommandRecord> {
        let held = self
            .production
            .factories()
            .view(command_owner, category)?
            .complete_object()?;
        let object = self.object_type(held.type_id, rules)?;
        if object.category == crate::rules::object_type::ObjectCategory::Building {
            return None;
        }
        let selection = production::ProductionSelection::for_type(rules, object)?;
        self.encode_place_record(
            command_owner,
            production::ProductionSelection {
                rtti: selection.object_rtti(),
                type_index: -1,
                ..selection
            },
            (0, 0),
        )
    }

    /// Shared EventCtor4C6AE0. Callers supply their native selector: manual
    /// building placement, completed cameo, or automatic Strip completion.
    pub fn encode_place_record(
        &self,
        command_owner: InternedId,
        selection: production::ProductionSelection,
        cell: (u16, u16),
    ) -> Option<CommandRecord> {
        let mut record =
            self.encode_header_event(command_owner, crate::sim::command::PLACE_OPCODE)?;
        PlaceRecord {
            house_id: record.house_id(),
            frame: record.frame_stamp(),
            rtti: selection.rtti,
            type_index: selection.type_index,
            naval: u32::from(selection.naval),
            x: cell.0 as i16,
            y: cell.1 as i16,
        }
        .write_into(&mut record);
        Some(record)
    }

    /// G/command-bar Guard730D60 chooses each admitted actor's current
    /// navigation Cell before Player_Send_Command6FFBE0 emits mission11.
    /// It admits an actor both IsControllable700C40 and IsActive7010D0
    /// accept. Unit slave-manager/Harvester fallback bypasses both.
    /// Existing controllability lifecycle residuals remain documented at
    /// that owner. This is an input producer, never an event-execution gate.
    pub fn area_guard_key_command(&self, id: u64, rules: &RuleSet) -> Option<Command> {
        let actor = self.substrate.entities.get(id)?;
        if actor.category() == EntityCategory::Structure {
            return None;
        }
        let ordinary =
            self.techno_player_controllable(id, rules) && self.techno_is_active(id, rules);
        let miner = actor.category() == EntityCategory::Unit
            && (actor.slave_manager.is_some() || actor.is_harvester());
        if !ordinary && !miner {
            return None;
        }
        let (x, y) = self
            .object_navigation_cell(id, rules)
            .unwrap_or_else(|cause| panic!("Area Guard input cell: {cause}"));
        Some(Command::Guard {
            entity_id: id,
            target: Some(combat::TargetKind::Cell(x as u16, y as u16)),
        })
    }

    /// Build the native MegaMission record for represented player missions.
    ///
    /// `EventClass__BuildMegaMissionEnvelope` at `gamemd.exe` `0x004C6860`
    /// stores HouseClass registration and Abstract stable identity separately;
    /// the source therefore need not belong to the issuing house.
    pub fn encode_megamission_record(
        &self,
        command_owner: crate::sim::intern::InternedId,
        command: &Command,
    ) -> Option<CommandRecord> {
        let token = |target: Option<combat::TargetKind>| -> Option<MegaMissionTarget> {
            Some(match target {
                None => MegaMissionTarget::Null,
                Some(combat::TargetKind::Cell(x, y)) => MegaMissionTarget::Cell {
                    x: x as i16,
                    y: y as i16,
                },
                Some(combat::TargetKind::Entity(id)) => MegaMissionTarget::Object {
                    id: i32::try_from(id).ok()?,
                },
            })
        };
        let (source_id, order) = match *command {
            Command::Attack {
                attacker_id,
                target_id,
            } => (
                attacker_id,
                MegaMissionOrder::Attack {
                    target: token(Some(combat::TargetKind::Entity(target_id)))?,
                },
            ),
            Command::ForceAttackCell {
                attacker_id,
                target_rx,
                target_ry,
            } => (
                attacker_id,
                MegaMissionOrder::Attack {
                    target: token(Some(combat::TargetKind::Cell(target_rx, target_ry)))?,
                },
            ),
            Command::AttackMove {
                entity_id,
                target,
                destination,
            } => (
                entity_id,
                MegaMissionOrder::AttackMove {
                    target: token(target)?,
                    destination: token(destination)?,
                },
            ),
            Command::Move {
                entity_id,
                target_rx,
                target_ry,
            } => (
                entity_id,
                MegaMissionOrder::Move {
                    target_x: i16::try_from(target_rx).ok()?,
                    target_y: i16::try_from(target_ry).ok()?,
                },
            ),
            Command::MoveToObject {
                entity_id,
                target_id,
            } => (
                entity_id,
                MegaMissionOrder::MoveToObject {
                    id: i32::try_from(target_id).ok()?,
                },
            ),
            Command::Guard { entity_id, target } => {
                let post = token(target)?;
                (entity_id, MegaMissionOrder::AreaGuard { post })
            }
            Command::PlantC4 {
                attacker_id,
                target_building_id,
            } => (
                attacker_id,
                MegaMissionOrder::Mission {
                    target: MegaMissionTarget::Null,
                    mission: MissionType::Sabotage,
                    destination: token(Some(combat::TargetKind::Entity(target_building_id)))?,
                },
            ),
            Command::CaptureBuilding {
                engineer_id,
                target_building_id,
            } => (
                engineer_id,
                MegaMissionOrder::Mission {
                    target: MegaMissionTarget::Null,
                    mission: MissionType::Capture,
                    destination: token(Some(combat::TargetKind::Entity(target_building_id)))?,
                },
            ),
            Command::UnloadPassengers { transport_id } => (
                transport_id,
                MegaMissionOrder::Mission {
                    target: if self
                        .substrate
                        .entities
                        .get(transport_id)
                        .is_some_and(|actor| actor.category() == EntityCategory::Structure)
                    {
                        MegaMissionTarget::Object {
                            id: i32::try_from(transport_id).ok()?,
                        }
                    } else {
                        MegaMissionTarget::Null
                    },
                    mission: MissionType::Unload,
                    destination: MegaMissionTarget::Null,
                },
            ),
            Command::ToggleInfantryDeploy { entity_id } | Command::DeployMcv { entity_id } => (
                entity_id,
                MegaMissionOrder::Mission {
                    target: MegaMissionTarget::Null,
                    mission: MissionType::Unload,
                    destination: MegaMissionTarget::Null,
                },
            ),
            Command::MegaMission {
                entity_id,
                mission,
                target,
                destination,
            } => (
                entity_id,
                MegaMissionOrder::Mission {
                    mission,
                    target: token(target)?,
                    destination: token(destination)?,
                },
            ),
            _ => return None,
        };
        if !self.houses.contains_key(&command_owner)
            || self.substrate.entities.get(source_id).is_none()
        {
            return None;
        }
        let house_id = self
            .houses
            .order()
            .iter()
            .position(|&owner| owner == command_owner)
            .and_then(|index| i8::try_from(index).ok())?;
        let typed = MegaMissionRecord {
            house_id,
            frame: self.session.binary_frame() as i32,
            source_id: i32::try_from(source_id).ok()?,
            order,
        };
        let mut record = CommandRecord::decode_exact(&[0; COMMAND_RECORD_LEN]).ok()?;
        typed.write_into(&mut record).ok()?;
        Some(record)
    }

    /// Encode the native synchronized record for one locally issued wall sale.
    /// House bytes are HouseClass registration indices, never interner ids.
    pub fn encode_sell_wall_at_cell_record(
        &self,
        command_owner: crate::sim::intern::InternedId,
        x: i16,
        y: i16,
    ) -> Option<CommandRecord> {
        if !self.houses.contains_key(&command_owner) {
            return None;
        }
        let house_id = self
            .houses
            .order()
            .iter()
            .position(|&owner| owner == command_owner)
            .and_then(|index| i8::try_from(index).ok())?;
        SellWallAtCellRecord {
            house_id,
            frame: self.session.binary_frame(),
            x,
            y,
        }
        .encode()
        .ok()
    }

    /// Encode the header-only native EXIT event issued by Abort confirmation.
    /// House bytes are HouseClass registration indices, never interner ids.
    pub fn encode_exit_record(
        &self,
        command_owner: crate::sim::intern::InternedId,
    ) -> Option<CommandRecord> {
        self.encode_header_event(command_owner, crate::sim::command::EXIT_OPCODE)
    }

    pub fn encode_options_record(&self, command_owner: InternedId) -> Option<CommandRecord> {
        self.encode_header_event(command_owner, crate::sim::command::OPTIONS_OPCODE)
    }

    pub fn encode_latency_fudge_record(
        &self,
        command_owner: InternedId,
        value: i32,
    ) -> Option<CommandRecord> {
        self.encode_int_event(
            command_owner,
            crate::sim::command::LATENCY_FUDGE_OPCODE,
            value,
        )
    }

    /// EVENT_DESTRUCT's header-only record: `EventClass(index, 8)`
    /// (`0x004C66C0`) writes only the type, House and frame.
    pub fn encode_destruct_record(
        &self,
        command_owner: crate::sim::intern::InternedId,
    ) -> Option<CommandRecord> {
        self.encode_header_event(command_owner, crate::sim::command::DESTRUCT_OPCODE)
    }

    /// EVENT_REMOVEPLAYER's record: the header House is the sender, the
    /// dword payload the removed House's index (`0x004C6720`).
    pub fn encode_remove_player_record(
        &self,
        command_owner: crate::sim::intern::InternedId,
        house_index: i32,
    ) -> Option<CommandRecord> {
        self.encode_int_event(
            command_owner,
            crate::sim::command::REMOVE_PLAYER_OPCODE,
            house_index,
        )
    }

    /// EVENT_ALLY's record: the header House is the sender, the dword
    /// payload the other House's index (`0x00731877..0x007318C8`).
    pub fn encode_ally_record(
        &self,
        command_owner: crate::sim::intern::InternedId,
        house_index: i32,
    ) -> Option<CommandRecord> {
        self.encode_int_event(command_owner, crate::sim::command::ALLY_OPCODE, house_index)
    }

    /// `EventClass(house, type, value)` (`0x004C6720`): a header event
    /// whose payload is one unmodified signed dword.
    fn encode_int_event(
        &self,
        command_owner: crate::sim::intern::InternedId,
        opcode: u8,
        value: i32,
    ) -> Option<CommandRecord> {
        let mut record = self.encode_header_event(command_owner, opcode)?;
        record.payload_mut()[..4].copy_from_slice(&value.to_le_bytes());
        Some(record)
    }

    fn encode_header_event(
        &self,
        command_owner: crate::sim::intern::InternedId,
        opcode: u8,
    ) -> Option<CommandRecord> {
        if !self.houses.contains_key(&command_owner) {
            return None;
        }
        let house_id = self
            .houses
            .order()
            .iter()
            .position(|&owner| owner == command_owner)
            .and_then(|index| i8::try_from(index).ok())?;
        CommandRecord::encode(
            opcode,
            i32::from(house_id),
            self.session.binary_frame() as i32,
            &[],
        )
        .ok()
    }

    /// Decode one synchronized record after its queue has admitted the stamped
    /// frame. The semantic envelope is only a typed execution view of the raw
    /// bytes; timing/processed-bit ownership remains with the raw queue.
    pub fn decode_native_command_record(
        &self,
        record: &CommandRecord,
        execute_tick: u64,
    ) -> Option<CommandEnvelope> {
        if let Some(event) = RepairRecord::decode(record) {
            let owner = *self
                .houses
                .order()
                .get(usize::try_from(event.house_id).ok()?)?;
            let entity_id = u64::try_from(event.entity_id).ok()?;
            return self.houses.contains_key(&owner).then(|| {
                CommandEnvelope::new(owner, execute_tick, Command::ToggleRepair { entity_id })
            });
        }
        if let Some(event) = ArchiveRecord::decode(record) {
            let owner = *self
                .houses
                .order()
                .get(usize::try_from(event.house_id).ok()?)?;
            if !self.houses.contains_key(&owner) {
                return None;
            }
            // Preserve late object lookup and NULL. These semantic views do
            // not replace the native record held by queues/replay/snapshots.
            let entity_id = u64::try_from(event.entity_id).ok()?;
            let payload = match event.target {
                MegaMissionTarget::Cell { x, y } => Command::SetRally {
                    rx: x as u16,
                    ry: y as u16,
                    producer_ids: vec![entity_id],
                },
                MegaMissionTarget::Object { id } if id >= 0 => Command::ArchiveObject {
                    entity_id,
                    target_id: id as u64,
                },
                _ => Command::SynchronizedRecord {
                    record: record.clone(),
                },
            };
            return Some(CommandEnvelope::new(owner, execute_tick, payload));
        }
        if ProductionRecord::decode(record).is_some() || PlaceRecord::decode(record).is_some() {
            let owner = *self
                .houses
                .order()
                .get(usize::try_from(record.house_id()).ok()?)?;
            // Keep selectors raw: a category-only semantic view loses Naval,
            // RTTI aliases and the -1 wildcard before the House receiver runs.
            return self.houses.contains_key(&owner).then(|| {
                CommandEnvelope::new(
                    owner,
                    execute_tick,
                    Command::SynchronizedRecord {
                        record: record.clone(),
                    },
                )
            });
        }
        if let Some(typed) = DeployRecord::decode(record) {
            let owner = *self
                .houses
                .order()
                .get(usize::try_from(typed.house_id).ok()?)?;
            let entity_id = u64::try_from(typed.entity_id).ok()?;
            self.substrate.entities.get(entity_id)?;
            return self.houses.contains_key(&owner).then(|| {
                CommandEnvelope::new(owner, execute_tick, Command::DeployMcv { entity_id })
            });
        }
        if matches!(
            record.opcode(),
            crate::sim::command::OPTIONS_OPCODE | crate::sim::command::LATENCY_FUDGE_OPCODE
        ) {
            let house_index = usize::try_from(record.house_id()).ok()?;
            let owner = *self.houses.order().get(house_index)?;
            let command = if record.opcode() == crate::sim::command::OPTIONS_OPCODE {
                Command::OpenOptions
            } else {
                Command::SetLatencyFudge {
                    value: i32::from_le_bytes(record.payload()[..4].try_into().ok()?),
                }
            };
            return self
                .houses
                .contains_key(&owner)
                .then(|| CommandEnvelope::new(owner, execute_tick, command));
        }
        // PlanMgr637E00/638120 owns the payload and validates live actors.
        // Keep its native byte record intact through the scheduled envelope.
        if matches!(record.opcode(), 0x2A..=0x2C)
            || (record.opcode() == crate::sim::command::MEGAMISSION_OPCODE
                && matches!(record.payload()[22], 1 | 2))
        {
            let house_index = usize::try_from(record.house_id()).ok()?;
            let owner = *self.houses.order().get(house_index)?;
            return self.houses.contains_key(&owner).then(|| {
                CommandEnvelope::new(
                    owner,
                    execute_tick,
                    Command::PlanningRecord {
                        record: record.clone(),
                    },
                )
            });
        }
        if let Some(typed) = StopRecord::decode(record) {
            let house_index = usize::try_from(typed.house_id).ok()?;
            let owner = *self.houses.order().get(house_index)?;
            if !self.houses.contains_key(&owner) {
                return None;
            }
            let entity_id = u64::try_from(typed.entity_id).ok()?;
            self.substrate.entities.get(entity_id)?;
            return Some(CommandEnvelope::new(
                owner,
                execute_tick,
                Command::Stop { entity_id },
            ));
        }
        if let Some(typed) = ScatterRecord::decode(record) {
            let house_index = usize::try_from(typed.house_id).ok()?;
            let owner = *self.houses.order().get(house_index)?;
            if !self.houses.contains_key(&owner) {
                return None;
            }
            let entity_id = u64::try_from(typed.entity_id).ok()?;
            self.substrate.entities.get(entity_id)?;
            return Some(CommandEnvelope::new(
                owner,
                execute_tick,
                Command::Scatter { entity_id },
            ));
        }
        if let Some(typed) = MegaMissionRecord::decode(record) {
            let house_index = usize::try_from(typed.house_id).ok()?;
            let owner = *self.houses.order().get(house_index)?;
            if !self.houses.contains_key(&owner) {
                return None;
            }
            let entity_id = u64::try_from(typed.source_id).ok()?;
            self.substrate.entities.get(entity_id)?;
            let target = |token: MegaMissionTarget| -> Option<Option<combat::TargetKind>> {
                Some(match token {
                    MegaMissionTarget::Null => None,
                    MegaMissionTarget::Cell { x, y } => {
                        Some(combat::TargetKind::Cell(x as u16, y as u16))
                    }
                    MegaMissionTarget::Object { id } => {
                        Some(combat::TargetKind::Entity(u64::try_from(id).ok()?))
                    }
                })
            };
            let command = match typed.order {
                MegaMissionOrder::Attack {
                    target: attack_target,
                } => match target(attack_target)? {
                    // Ctrl and ordinary Attack encode identical bytes.
                    Some(combat::TargetKind::Entity(target_id)) => Command::Attack {
                        attacker_id: entity_id,
                        target_id,
                    },
                    Some(combat::TargetKind::Cell(target_rx, target_ry)) => {
                        Command::ForceAttackCell {
                            attacker_id: entity_id,
                            target_rx,
                            target_ry,
                        }
                    }
                    None => return None,
                },
                MegaMissionOrder::AttackMove {
                    target: attack_target,
                    destination,
                } => Command::AttackMove {
                    entity_id,
                    target: target(attack_target)?,
                    destination: target(destination)?,
                },
                MegaMissionOrder::Move { target_x, target_y } => Command::Move {
                    entity_id,
                    target_rx: u16::try_from(target_x).ok()?,
                    target_ry: u16::try_from(target_y).ok()?,
                },
                MegaMissionOrder::MoveToObject { id } => Command::MoveToObject {
                    entity_id,
                    target_id: u64::try_from(id).ok()?,
                },
                MegaMissionOrder::AreaGuard { post } => Command::Guard {
                    entity_id,
                    target: target(post)?,
                },
                MegaMissionOrder::Mission {
                    mission,
                    target: order_target,
                    destination,
                } => Command::MegaMission {
                    entity_id,
                    mission,
                    target: target(order_target)?,
                    destination: target(destination)?,
                },
            };
            return Some(CommandEnvelope::new(owner, execute_tick, command));
        }

        // Event2D4C6E2E resolves its House and calls50C8C0 without input
        //lock, cooldown, selection or product admission being repeated.
        if record.opcode() == crate::sim::command::CHEER_OPCODE {
            let index = usize::try_from(record.house_id()).ok()?;
            let owner = *self.houses.order().get(index)?;
            return self
                .houses
                .contains_key(&owner)
                .then(|| CommandEnvelope::new(owner, execute_tick, Command::Cheer));
        }

        if let Some(typed) = ExitRecord::decode(record) {
            let house_index = usize::try_from(typed.house_id).ok()?;
            let owner = *self.houses.order().get(house_index)?;
            return self
                .houses
                .contains_key(&owner)
                .then(|| CommandEnvelope::new(owner, execute_tick, Command::ExitMatch));
        }

        let house_index_event: Option<fn(i32) -> Command> = match record.opcode() {
            crate::sim::command::REMOVE_PLAYER_OPCODE => {
                Some(|house_index| Command::RemovePlayer { house_index })
            }
            crate::sim::command::ALLY_OPCODE => Some(|house_index| Command::Ally { house_index }),
            _ => None,
        };
        if let Some(command) = house_index_event {
            let house_index = usize::try_from(record.house_id()).ok()?;
            let owner = *self.houses.order().get(house_index)?;
            let other = i32::from_le_bytes(record.payload()[..4].try_into().ok()?);
            return self
                .houses
                .contains_key(&owner)
                .then(|| CommandEnvelope::new(owner, execute_tick, command(other)));
        }

        if record.opcode() == crate::sim::command::DESTRUCT_OPCODE {
            let house_index = usize::try_from(record.house_id()).ok()?;
            let owner = *self.houses.order().get(house_index)?;
            return self
                .houses
                .contains_key(&owner)
                .then(|| CommandEnvelope::new(owner, execute_tick, Command::Destruct));
        }

        // Native opcode 0x0D (EventClass::Execute type-13 @ 0x004C794B): the
        // payload dword at Event+7 is the new GameSpeed setting; the changer is
        // the header house. It is SetGameSpeed's live-network twin.
        if let Some(typed) = GameSpeedRecord::decode(record) {
            let house_index = usize::try_from(typed.house_id).ok()?;
            let owner = *self.houses.order().get(house_index)?;
            let speed = u8::try_from(typed.speed).ok()?;
            return self.houses.contains_key(&owner).then(|| {
                CommandEnvelope::new(owner, execute_tick, Command::LiveGameSpeed { speed })
            });
        }

        // Native opcodes 0x01/0x02 (EventClass::Execute branches 0x004C6CF0 /
        // 0x004C6D4D) drive the Building+660 latch via GoOnline 0x00452260 /
        // GoOffline 0x00452360; the direction is the opcode, the target the
        // Abstract token at payload[0..4] (kind 0x34). No owner check here --
        // input gating is the client's job, as in native.
        if matches!(
            record.opcode(),
            BUILDING_ON_LINE_OPCODE | BUILDING_OFF_LINE_OPCODE
        ) {
            let Some(typed) = BuildingOnLineRecord::decode(record) else {
                return None;
            };
            let house_index = usize::try_from(typed.house_id).ok()?;
            let owner = *self.houses.order().get(house_index)?;
            let entity_id = u64::try_from(typed.entity_id).ok()?;
            self.substrate.entities.get(entity_id)?;
            return self.houses.contains_key(&owner).then(|| {
                CommandEnvelope::new(
                    owner,
                    execute_tick,
                    Command::SetBuildingOnLine {
                        entity_id,
                        on: typed.on,
                    },
                )
            });
        }

        let typed = SellWallAtCellRecord::decode(record)?;
        let house_index = usize::try_from(typed.house_id).ok()?;
        let owner = *self.houses.order().get(house_index)?;
        self.houses.contains_key(&owner).then(|| {
            CommandEnvelope::new(
                owner,
                execute_tick,
                Command::SellWallAtCell {
                    x: typed.x,
                    y: typed.y,
                },
            )
        })
    }

    /// Publish one wall-sale RecalcAttributes result to the transaction-local
    /// path view and the retained base movement-class topology. Zone ID
    /// assignment remains owned by the ordered native repair callback.
    fn refresh_wall_sale_recalc_prefix(
        world_zone_grid: &mut Option<ZoneGrid>,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        overlay_grid: &Option<OverlayGrid>,
        tail_grid: &mut Option<PathGrid>,
        rx: u16,
        ry: u16,
        navigation_changed: bool,
    ) {
        let Some(terrain) = resolved_terrain.as_ref() else {
            return;
        };

        if navigation_changed {
            if tail_grid.as_ref().is_some_and(|tail| {
                tail.width() != terrain.width() || tail.height() != terrain.height()
            }) {
                *tail_grid = None;
            }
            if let (Some(tail), Some(cell)) = (tail_grid.as_mut(), terrain.cell(rx, ry)) {
                let _ = tail.refresh_resolved_cell(cell, overlay_grid.as_ref(), false);
            }
        }

        if let Some(zone_grid) = world_zone_grid.as_mut() {
            let _ = zone_grid.refresh_base_cell_attributes_at(terrain, rx, ry);
        }
    }

    /// Run the exact AssignOrphaned + local graph repair against the current
    /// visit-prefix view. This does not publish `tail_grid`; the sale commits
    /// the completed transaction once all cleanup visits finish.
    fn repair_wall_sale_zone_prefix(
        &mut self,
        tail_grid: &PathGrid,
        sold_cell: (u16, u16),
        repair_cell: (u16, u16),
        repair: ZoneRepairKind,
    ) {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        #[cfg(not(any(test, feature = "test-support")))]
        let _ = sold_cell;
        let Some(_zone_grid) = self.zone_grid.as_mut() else {
            return;
        };
        #[cfg(any(test, feature = "test-support"))]
        trace_wall_sell_zone_repair_step(_zone_grid, tail_grid, sold_cell, repair_cell);
        super::repair_zone_after_recalc(
            &mut self.zone_grid,
            tail_grid,
            terrain,
            self.bridge_state.as_ref(),
            self.playfield_bounds,
            repair_cell,
            repair,
        );
    }

    /// `HouseClass::Sell_Building_At_Cell @ 0x004FCE80` for house
    /// `command_owner` at cell `(x, y)`: the player's wall sale and the
    /// computer's in `AI_Manage_Build_Queue` (`sim::ai_build_need`). It sells
    /// only a wall whose owner a human controls, and refunds nothing.
    /// `suppress_local_cue` is the original second explicit argument:
    /// 0x4FCF78..0x4FCF98 skips only SellSound; removal and target cleanup still run.
    /// Returns whether a wall went.
    pub(crate) fn sell_wall_at_cell(
        &mut self,
        command_owner: &str,
        x: i16,
        y: i16,
        suppress_local_cue: bool,
        rules: &RuleSet,
        overlays: &crate::rules::overlay_types::OverlayTypeRegistry,
        effects: FrameEffects<'_>,
    ) -> bool {
        // EventClass rejects only the exact packed null CellStruct. Every
        // other signed pair is resolved by MapClass' fixed 512-wide linear
        // cell array, so out-of-range components may alias a canonical slot.
        if (x, y) == (0, 0) {
            return false;
        }
        let Some((rx, ry)) = canonical_cell_coord(i32::from(x), i32::from(y)) else {
            return false;
        };
        let Some(grid) = self.overlay_grid.as_ref() else {
            return false;
        };
        if rx >= grid.width() || ry >= grid.height() {
            return false;
        }
        let cell = *grid.cell(rx, ry);
        let (Some(overlay_id), Some(wall_owner)) = (cell.overlay_id, cell.wall_owner) else {
            return false;
        };
        let Some(owner_house) = self.houses.get(&wall_owner) else {
            return false;
        };
        let owner_admitted = if self.session.game_mode_nonzero() {
            owner_house.is_human()
        } else {
            owner_house.is_human() || owner_house.player_control()
        };
        if !owner_admitted || !overlays.flags(overlay_id).is_some_and(|flags| flags.wall) {
            return false;
        }
        let Some(wall_type) = rules.first_building_type_for_overlay(overlay_id, overlays) else {
            return false;
        };
        if wall_type.unsellable {
            return false;
        }

        // Native emits the global cue before the discarded actual-cost call
        // and before clearing the overlay. Locality belongs to the receiver.
        if !suppress_local_cue
            && rules.general.sell_sound.is_some()
            && let Some(receiver) = self.interner.get(command_owner)
        {
            self.sound_events.emit(SimSoundEvent::WallSold { receiver });
        }
        let _discarded_actual_cost = rules.building_actual_cost(wall_type);

        let mut tail_grid = self.path_grid.as_deref().cloned();
        let sold_navigation_changed = if let Some(grid) = self.overlay_grid.as_mut() {
            // gamemd-derived: `HouseClass::Sell_Building_At_Cell @ 0x004FCE80`
            // clears the wall identity itself (`+0x44 = -1` at `0x004FCFBC`,
            // `+0x11E = 0` at `0x004FCFCA`, `+0x50 = -1` at `0x004FCFDD`) and
            // never touches `CellClass+0x122` anywhere in its body; the
            // `PostDestructionWallCleanup(0)` it runs at `0x004FCFFB` then skips
            // the just-cleared cell at `0x004807CA`. The absent plane decrement
            // here is that behaviour, not an oversight: a sold wall keeps its
            // eight neighbour contributions permanently.
            grid.clear_overlay(rx, ry);
            if let Some(terrain) = self.resolved_terrain.as_mut() {
                grid.recalculate_runtime_cell(terrain, overlays, (rx, ry))
                    .navigation_changed
            } else {
                false
            }
        } else {
            false
        };
        Self::refresh_wall_sale_recalc_prefix(
            &mut self.zone_grid,
            &self.resolved_terrain,
            &self.overlay_grid,
            &mut tail_grid,
            rx,
            ry,
            sold_navigation_changed,
        );

        // Selling invokes exactly one PostDestructionWallCleanup at the sold
        // cell: N, E, S, W, self. It is not damage's four-cardinal fan-out.
        const CROSS: [(i32, i32); 5] = [(0, -1), (1, 0), (0, 1), (-1, 0), (0, 0)];
        for (dx, dy) in CROSS {
            let visit = {
                let mut host = SimulationWallRuntimeHost {
                    dirty_cells: &mut self.dirty_cells,
                    zone_grid: &mut self.zone_grid,
                    path_grid: &mut self.path_grid,
                    bridge_state: self.bridge_state.as_ref(),
                    playfield_bounds: self.playfield_bounds,
                };
                self.overlay_grid.as_mut().and_then(|grid| {
                    runtime_wall_cleanup_visit_at(
                        grid,
                        overlays,
                        self.resolved_terrain.as_ref(),
                        i32::from(rx) + dx,
                        i32::from(ry) + dy,
                        Some(&mut host),
                    )
                })
            };
            let Some(visit) = visit else {
                continue;
            };

            if !visit.was_wall {
                continue;
            }
            let Some((nx, ny)) = visit.real_cell else {
                // RecalcAttributes suppresses the cleared/updated shared
                // dummy after the overlay step, so it has no represented
                // navigation, zone, or retained-count output.
                continue;
            };
            let result = visit.recomputed;
            let recalc = match (self.overlay_grid.as_mut(), self.resolved_terrain.as_mut()) {
                (Some(grid), Some(terrain)) => {
                    grid.recalculate_runtime_cell(terrain, overlays, (nx, ny))
                }
                _ => OverlayRecalcOutcome::default(),
            };
            Self::refresh_wall_sale_recalc_prefix(
                &mut self.zone_grid,
                &self.resolved_terrain,
                &self.overlay_grid,
                &mut tail_grid,
                nx,
                ny,
                recalc.navigation_changed,
            );
            let cleanup_zone_changed = recalc.zone_changed;
            if cleanup_zone_changed && let Some(prefix_grid) = tail_grid.as_ref() {
                self.repair_wall_sale_zone_prefix(
                    prefix_grid,
                    (rx, ry),
                    (nx, ny),
                    if result == RecomputeResult::Destroyed {
                        ZoneRepairKind::AssignOrphaned
                    } else {
                        ZoneRepairKind::MergeAdjacent
                    },
                );
            }
            // gamemd-derived: `CellClass::PostDestructionWallCleanup @
            // 0x00480630` runs its own eight-step `+0x122` decrement
            // (`0x00480999..0x004809EF`) only when this cell's hardcoded
            // isolated removal fired (`TEST BL,BL` at `0x0048097D`) AND
            // `RecalcAttributes` changed its zone type (load at
            // `0x00480972`, compare at `0x00480975`). A removed wall whose zone is unchanged keeps its
            // eight contributions, natively.
            if result == RecomputeResult::Destroyed && cleanup_zone_changed {
                let terrain = self
                    .resolved_terrain
                    .as_ref()
                    .expect("wall-sale cleanup zone comparison requires terrain");
                self.overlay_grid
                    .as_mut()
                    .expect("wall-sale cleanup requires overlay grid")
                    .remove_retained_wall_neighbor_source(Some(terrain), nx, ny);
            }
        }
        self.dirty_cells.mark_radar([(rx, ry)]);

        // House4FD018 calls70D4A0, not7258D0 pointer expiry: Restore runs
        // before the conditional target clear, then Team targets detach.
        self.stop_all_targeting_cell(rx, ry, rules, Some(overlays), effects);

        if let Some(tail_grid) = tail_grid {
            if self.zone_grid.is_some() {
                // HouseClass performs the sold-cell AssignOrphaned/graph tail
                // after PostDestructionWallCleanup has completed every visit.
                self.repair_wall_sale_zone_prefix(
                    &tail_grid,
                    (rx, ry),
                    (rx, ry),
                    ZoneRepairKind::AssignOrphaned,
                );
            } else {
                self.rebuild_zone_grid_full(&tail_grid);
            }
            self.path_grid = Some(std::sync::Arc::new(tail_grid));
        }
        true
    }

    /// Execute4C70E1..4C718A hands the packet selector to the existing House
    /// production owners. Keep it intact until this call; neither the issuing
    /// client's type interner nor its then-current Factory head is authority.
    fn apply_production_record(
        &mut self,
        record: &CommandRecord,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(owner) = usize::try_from(record.house_id())
            .ok()
            .and_then(|index| self.houses.order().get(index))
            .copied()
            .filter(|owner| self.houses.contains_key(owner))
        else {
            return false;
        };
        let owner = self.interner.resolve(owner).to_owned();
        if let Some(event) = ProductionRecord::decode(record) {
            let selection = production::ProductionSelection {
                rtti: event.rtti,
                type_index: event.type_index,
                naval: event.naval != 0,
            };
            return match event.event {
                ProductionEvent::Produce => {
                    production::enqueue_selected(self, rules, &owner, selection, effects, false)
                }
                ProductionEvent::Suspend => {
                    let Some(category) = selection.category(rules) else {
                        return false;
                    };
                    production::suspend_production(
                        &self.interner,
                        &mut self.production,
                        &self.session,
                        &owner,
                        category,
                    )
                }
                ProductionEvent::Abandon | ProductionEvent::AbandonAll => {
                    production::cancel_production_for_owner(
                        self,
                        rules,
                        &owner,
                        production::ProductionCancellation::Selected(selection),
                        event.event == ProductionEvent::AbandonAll,
                        effects,
                    )
                }
            };
        }
        if let Some(event) = PlaceRecord::decode(record) {
            return production::place_production_with_overlays(
                self,
                rules,
                &owner,
                production::ProductionPlacement::Selected {
                    selection: production::ProductionSelection {
                        rtti: event.rtti,
                        type_index: event.type_index,
                        naval: event.naval != 0,
                    },
                    cell: (event.x as u16, event.y as u16),
                },
                overlay_registry,
                effects,
            );
        }
        false
    }

    /// Dispatch a single command, returning true if it was successfully applied.
    #[cfg(any(test, feature = "test-support"))]
    pub fn apply_command(
        &mut self,
        command_owner: &str,
        cmd: &Command,
        rules: Option<&RuleSet>,
    ) -> bool {
        self.apply_command_with_overlays(
            command_owner,
            cmd,
            rules.unwrap_or(crate::rules::ruleset::RuleSet::empty_for_test()),
            None,
            crate::sim::world::FrameEffects::default(),
        )
    }

    pub(crate) fn apply_command_with_overlays(
        &mut self,
        command_owner: &str,
        cmd: &Command,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        // Event command receivers clear Plan tokens before applying ordinary
        // orders. Flag2 acknowledges its path without detaching it; use the
        // same payload dispatcher while keeping state installed for callbacks.
        let decoded_planned;
        let planned = matches!(cmd, Command::PlanningRecord { record }
            if record.opcode() == 4 && record.payload()[22] == 2);
        let cmd = if planned {
            let Command::PlanningRecord { record } = cmd else {
                unreachable!()
            };
            let mut ordinary = record.clone();
            ordinary.payload_mut()[22] = 0;
            let Some(typed) = MegaMissionRecord::decode(&ordinary) else {
                return false;
            };
            let Some(envelope) = self.decode_native_command_record(&ordinary, self.session.tick())
            else {
                return false;
            };
            if let Ok(actor) = u64::try_from(typed.source_id) {
                self.planning_acknowledge(actor);
            }
            decoded_planned = envelope.payload;
            &decoded_planned
        } else {
            cmd
        };
        let ordinary_actor = match cmd {
            Command::Stop { entity_id }
            | Command::Scatter { entity_id }
            | Command::DeployMcv { entity_id }
            | Command::ToggleRepair { entity_id } => Some(*entity_id),
            _ => cmd.megamission_actor(),
        };
        if !planned && let Some(actor) = ordinary_actor {
            self.planning_detach_actor(actor);
        }
        match cmd {
            Command::OpenOptions => {
                // Execute_DoList64C8D8..64C911 dispatches OPTIONS only for
                // the current House. Event4C7929 suppresses playback, then
                // writes GameState1 at4C7939. Source: original instructions.
                if self.interner.get(command_owner) != self.session.current_house() {
                    return false;
                }
                #[cfg(any(test, feature = "test-support"))]
                if self.replaying_recorded_commands {
                    return false;
                }
                self.lifecycle_outputs
                    .emit(super::LifecycleOutput::OptionsRequested);
                true
            }
            Command::SetLatencyFudge { value } => {
                // Event LATENCYFUDGE4C7F4F stores the unmodified dword; its
                // network-session owner receives the write after MainTick.
                let Some(owner) = self.interner.get(command_owner) else {
                    return false;
                };
                self.lifecycle_outputs
                    .emit(super::LifecycleOutput::LatencyFudgeChanged {
                        owner,
                        value: *value,
                    });
                true
            }
            Command::MegaMission {
                entity_id,
                mission,
                target,
                destination,
            } => self
                .apply_megamission_command(
                    *entity_id,
                    *mission,
                    *target,
                    *destination,
                    rules,
                    overlay_registry,
                    frame_effects,
                )
                .accepted(),
            Command::PlanningRecord { record } => {
                self.apply_planning_record(command_owner, record, rules)
            }
            Command::SynchronizedRecord { record } => {
                if let Some(event) = ArchiveRecord::decode(record) {
                    let Ok(entity_id) = u64::try_from(event.entity_id) else {
                        return false;
                    };
                    let target = match event.target {
                        MegaMissionTarget::Null => None,
                        MegaMissionTarget::Cell { x, y } => {
                            Some(combat::TargetKind::Cell(x as u16, y as u16))
                        }
                        MegaMissionTarget::Object { id } => {
                            u64::try_from(id).ok().map(combat::TargetKind::Entity)
                        }
                    };
                    return self.execute_archive_event(entity_id, target);
                }
                self.apply_production_record(record, rules, overlay_registry, frame_effects)
            }
            Command::AssignControlGroup { group, entity_ids } => {
                self.assign_control_group(*group, entity_ids)
            }
            Command::Select { entity_ids, .. } => self.apply_selection_snapshot(
                entity_ids,
                super::UninitContext::new(rules, overlay_registry).with_effects(frame_effects),
            ),
            Command::Scatter { entity_id } => {
                // Event7 4C7859..4C78D5: clear planning tokens, then Foot,
                // alive, on-map and untethered. There is no HP/owner/mission
                // gate. Planning associations were cleared above.
                let admitted = self
                    .substrate
                    .entities
                    .get(*entity_id)
                    .is_some_and(|actor| {
                        actor.category().is_foot()
                            && actor.lifecycle.object_alive()
                            && !actor.lifecycle.in_limbo()
                            && actor.tether.partner().is_none()
                    });
                if !admitted {
                    return false;
                }
                // Native also writes +6B2. Its observed consumers are class
                // CRC4DBD60 and raw native Save/Load41035E/4103CD, not active
                // peer checksum64DAB0. No unread Rust bookkeeping shadow is
                // retained; class CRC/raw-native-image equivalence is absent.
                self.scatter(
                    *entity_id,
                    None,
                    crate::sim::movement::ScatterFlags::new(true, false),
                    rules,
                    overlay_registry,
                    frame_effects,
                )
                .unwrap_or_else(|cause| panic!("player Scatter event: {cause}"));
                true
            }
            Command::Move {
                entity_id,
                target_rx,
                target_ry,
            } => self
                .apply_megamission_command(
                    *entity_id,
                    MissionType::Move,
                    None,
                    Some(combat::TargetKind::Cell(*target_rx, *target_ry)),
                    rules,
                    overlay_registry,
                    frame_effects,
                )
                .accepted(),
            Command::MoveToObject {
                entity_id,
                target_id,
            } => self
                .apply_megamission_command(
                    *entity_id,
                    MissionType::Move,
                    None,
                    Some(combat::TargetKind::Entity(*target_id)),
                    rules,
                    overlay_registry,
                    frame_effects,
                )
                .accepted(),
            Command::Stop { entity_id } => {
                // Event IDLE 0x004C74CB..0x004C76BB replaces the Event
                // receiver with its decoded actor at 0x004C74D3. It never
                // compares the sender's House with the actor's owner; sender
                // eligibility belongs to Execute_DoList's House scan.
                // Source: original gamemd.exe instructions.
                if !self.order_actor_admits(*entity_id) {
                    return false;
                }
                // The IDLE arm returns at `0x004C7504..0x004C750C` when the
                // object's tether byte (`+0x418`) is set: a miner that has
                // entered its dock ignores Stop and finishes unloading, and so
                // does a vehicle still leaving its war factory (the tether's
                // other writer). Current missions 0x12/0x13
                // also return before radio, destination and target writes.
                if self
                    .substrate
                    .entities
                    .get(*entity_id)
                    .is_some_and(|entity| {
                        entity.tether.partner().is_some()
                            || matches!(entity.mission.current().raw(), 0x12 | 0x13)
                    })
                {
                    return true;
                }
                // Event6 IDLE4C74CB..4C76BB retains the committed/queued
                // mission and its dispatch timer. Only the ore-miner exception
                // below writes Guard. Ordinary Move/Attack handlers see the
                // cleared NavCom/TarCom on their next dispatch and own the idle
                // transition, including Temporal LetGo at that later boundary.
                // `0x004C757D..0x004C759C`: a Foot's navigation list is
                // emptied (twice: inline, then `0x004DA1C0`); the planning-path
                // writes that follow have no VERA state.
                if let Some(entity) = self.substrate.entities.get_mut(*entity_id)
                    && entity.category() != crate::map::entities::EntityCategory::Structure
                {
                    entity.navigation.clear_nav_list();
                }
                // Event6 sends BREAK to every sparse contact (vt28065ACE0
                // at4C75E0), before NULL destination4C75ED/target4C75F8.
                // Unlike MEGAMISSION it never clears Foot pending-entry+500.
                crate::sim::radio::broadcast_break(self, *entity_id, rules, frame_effects);
                // `0x004C75ED`: the class setter's null destination, which
                // reaches the active locomotor's Stop_Moving (a Teleport's
                // drops only an armed warp, a moving Jumpjet's re-targets the
                // cell under it). A Unit's returns before any write without a
                // NavCom unless its `+0x1F8` override is up (`0x00741A80`); a
                // human infantryman's refuses during a deploy action, and Walk
                // Stop `0x0075ADA0` consumes a pending Deploy through owner
                // +0x54C and keeps a paid head. An attacking Aircraft with a
                // TarCom skips the Stop (`0x004D9672`). Building455D50 clears
                // an eligible rally ArchiveTarget without touching Foot NavCom;
                // its native matrix is factory_destination.json.
                self.assign_null_destination(*entity_id, rules, overlay_registry, frame_effects);
                // Event Stop4C75F8 invokes virtual+3C8 AFTER its null
                // destination4C75ED, including the Infantry class effects.
                let _ = self.assign_target_represented(
                    *entity_id,
                    None,
                    rules,
                    overlay_registry,
                    frame_effects,
                );
                // `0x004C75FE..0x004C7624`: a `BalloonHover=` type takes both
                // again. Its setter's arm (`0x00741983`) kept the NavCom while
                // the Target stood; without the Target this one clears it.
                let balloon = self
                    .substrate
                    .entities
                    .get(*entity_id)
                    .and_then(|entity| self.object_type(entity.type_ref(), rules))
                    .is_some_and(|object| object.balloon_hover);
                if balloon {
                    self.assign_null_destination(
                        *entity_id,
                        rules,
                        overlay_registry,
                        frame_effects,
                    );
                    let _ = self.assign_target_represented(
                        *entity_id,
                        None,
                        rules,
                        overlay_registry,
                        frame_effects,
                    );
                }
                // `0x004C762A..0x004C7634`: a spawner's manager drops its
                // targets, so a queued launch is cancelled and an attacking
                // wing is recalled at the next pass.
                crate::sim::spawn_manager::clear_all_spawn_targets(
                    self,
                    *entity_id,
                    rules,
                    overlay_registry,
                    frame_effects,
                );
                // **VERA-internal: retail Stop leaves the installed locomotor
                // alone.** This existing unwind policy uses the same END gate
                // as FootAI4DAEC3 / SetDestination742587 (an active Drive's
                // IsOKToEnd4AF970), after navigation is cleared. A live Drive
                // head still refuses it.
                // Keep that timing while centralizing the actual instance
                // transfer/retirement in locomotor_owner.
                // Trigger: Stop on a piggybacked Chrono Miner, a few times per
                // ordinary Allied match; a premature unwind can change the next
                // command's locomotor. Native Stop parity remains open. The
                // production command's admitted/refused lifetime is covered by
                // locomotor_owner_tests::stop_command_retires_only_the_drive_admitted_by_its_existing_gate.
                let may_end = self.substrate.entities.get(*entity_id).is_some_and(|e| {
                    e.locomotor
                        .as_ref()
                        .is_some_and(|loco| loco.is_overridden())
                        && crate::sim::movement::locomotor_owner::piggyback_end_admitted(e)
                });
                if may_end {
                    self.end_admitted_piggyback(*entity_id, rules);
                }
                // Ore-miner arm, last — retail runs it after the radio break,
                // the navigation clear, the target clear and the path-cursor
                // reset. A vehicle carrying the miner type flag whose committed
                // mission is Harvest or Return is force-assigned Guard and
                // commenced in the same command, so it is off the harvest loop
                // until it is re-ordered. Without it the miner halts for a beat
                // and then drives straight back to the ore field, ignoring the
                // order outright.
                //
                // `0x004C7639..0x004C7650`: an open-topped transport's riders
                // let go of their targets too.
                self.open_topped_passengers_take_target(
                    *entity_id,
                    None,
                    rules,
                    overlay_registry,
                    frame_effects,
                );
                self.commit_stop_miner_guard(*entity_id, rules);
                // `0x004C769C..0x004C76AC`: Stop takes a Slave Miner off its
                // hunt (`sim::slave_manager`).
                self.reset_slave_manager(*entity_id, rules, frame_effects);
                true
            }
            // A Cell target is the Ctrl click on the ground.
            Command::Attack {
                attacker_id,
                target_id,
            } => self
                .apply_megamission_command(
                    *attacker_id,
                    MissionType::Attack,
                    Some(combat::TargetKind::Entity(*target_id)),
                    None,
                    rules,
                    overlay_registry,
                    frame_effects,
                )
                .accepted(),
            Command::ForceAttackCell {
                attacker_id,
                target_rx,
                target_ry,
            } => self
                .apply_megamission_command(
                    *attacker_id,
                    MissionType::Attack,
                    Some(combat::TargetKind::Cell(*target_rx, *target_ry)),
                    None,
                    rules,
                    overlay_registry,
                    frame_effects,
                )
                .accepted(),
            Command::AttackMove {
                entity_id,
                target,
                destination,
            } => self
                .apply_megamission_command(
                    *entity_id,
                    MissionType::AttackMove,
                    *target,
                    *destination,
                    rules,
                    overlay_registry,
                    frame_effects,
                )
                .accepted(),
            Command::Guard { entity_id, target } => self
                .apply_megamission_command(
                    *entity_id,
                    MissionType::AreaGuard,
                    *target,
                    None,
                    rules,
                    overlay_registry,
                    frame_effects,
                )
                .accepted(),
            Command::DeployMcv { entity_id } => {
                // Event09's4C76BC receiver has no actor-owner comparison.
                // D/self-click selection checks belong to their producers.
                crate::sim::deploy::issue_order(self, *entity_id, rules, frame_effects)
            }
            // Legacy targetless order: no client sends it since the deploy key
            // refuses buildings (CanDeploySlashUnload), but recorded commands
            // keep it. The cell-click path sends SetRally followed by
            // SellBuilding, preserving native event1E/16.
            Command::UndeployBuilding { entity_id } => {
                if !self.entity_owned_by_id(command_owner, *entity_id) {
                    return false;
                }
                self.undeploy_building(*entity_id, rules, overlay_registry, frame_effects)
            }
            // The synchronized self-deploy order is the Unload MEGAMISSION
            // (see the transport arm of `UnloadPassengers`); the concrete
            // Infantry51F6E0 handler selects Doing27/31 on the object's visit.
            // CanDeploySlashUnload (`0x00700D50`) is the sender's admission
            // (deploy key, `Simulation::can_deploy_slash_unload`), not this
            // receiver's.
            //
            // RESIDUAL: this arm uses the clicked-mission owner instead of
            // Event9 (`0x004C76D7..0x004C7818`). Trigger: a deployer infantry
            // in Construction/Selling, a WeaponsFactory cell or a rejected
            // ground projection. Effect: it can deploy where native ignores D.
            // Frequency: rare; downstream risk: low.
            Command::ToggleInfantryDeploy { entity_id } => {
                if !self.entity_owned_by_id(command_owner, *entity_id) {
                    return false;
                }
                let Some(actor) = self.substrate.entities.get(*entity_id) else {
                    return false;
                };
                if actor.category() != EntityCategory::Infantry
                    || !self
                        .object_type(actor.type_ref(), rules)
                        .is_some_and(|object| object.deployer)
                {
                    return false;
                }
                self.apply_megamission_command(
                    *entity_id,
                    MissionType::Unload,
                    None,
                    None,
                    rules,
                    overlay_registry,
                    frame_effects,
                )
                .accepted()
            }
            Command::SetRally {
                rx,
                ry,
                producer_ids,
            } => self.set_rally_point_for_producers(producer_ids, *rx, *ry),
            Command::ArchiveObject {
                entity_id,
                target_id,
            } => {
                self.execute_archive_event(*entity_id, Some(combat::TargetKind::Entity(*target_id)))
            }
            // Production events act on the event's own house (the
            // EventClass header's house id); the payload names no owner.
            Command::QueueProduction { type_id } => {
                let type_s = self.interner.resolve(*type_id).to_string();
                production::enqueue_by_type(
                    self,
                    rules,
                    command_owner,
                    &type_s,
                    frame_effects,
                    false,
                )
            }
            Command::SuspendProduction { category } => production::suspend_production(
                &self.interner,
                &mut self.production,
                &self.session,
                command_owner,
                *category,
            ),
            Command::TogglePrimaryFactory { entity_id } => {
                // Event17 4C718F resolves a Building, then6386E0 receives
                // that pointer (NULL is a no-op). Only afterward4C71A7 tests
                // +90. A failed cast must not detach an unrelated Foot plan.
                let Some(alive) = self.substrate.entities.get(*entity_id).and_then(|entity| {
                    (entity.category() == EntityCategory::Structure)
                        .then_some(entity.lifecycle.object_alive())
                }) else {
                    return false;
                };
                self.planning_detach_actor(*entity_id);
                if !alive {
                    return false;
                }
                // The issuer adds no ownership/online/limbo/HP admission.
                production::toggle_factory_primary(self, rules, *entity_id)
            }
            Command::CycleProducerFocus { category } => {
                production::cycle_active_producer_for_owner_category(
                    self,
                    rules,
                    command_owner,
                    *category,
                )
            }
            Command::PlaceReadyBuilding { type_id, rx, ry } => {
                let type_s = self.interner.resolve(*type_id).to_string();
                production::place_production_with_overlays(
                    self,
                    rules,
                    command_owner,
                    production::ProductionPlacement::Building {
                        type_id: &type_s,
                        cell: (*rx, *ry),
                    },
                    overlay_registry,
                    frame_effects,
                )
            }
            Command::PlaceProducedMobile { category } => {
                production::place_production_with_overlays(
                    self,
                    rules,
                    command_owner,
                    production::ProductionPlacement::Mobile {
                        category: *category,
                    },
                    overlay_registry,
                    frame_effects,
                )
            }
            Command::CancelProductionByType { type_id, all } => {
                let type_s = self.interner.resolve(*type_id).to_string();
                production::cancel_production_for_owner(
                    self,
                    rules,
                    command_owner,
                    crate::sim::production::ProductionCancellation::Type {
                        type_id: &type_s,
                        naval: rules.object(&type_s).is_some_and(|object| object.naval),
                    },
                    *all,
                    frame_effects,
                )
            }
            // The SELL event (`EventClass::Execute 0x004C6F20`): the target's
            // owner must be the event's house (`0x004C6F45`); a building
            // takes `Sell_Back(-1)` (`0x004C6FA2`).
            Command::SellBuilding { entity_id } => {
                if !self.entity_owned_by_id(command_owner, *entity_id) {
                    return false;
                }
                production::sell_back(
                    self,
                    rules,
                    *entity_id,
                    production::SellOrder::Player,
                    overlay_registry,
                    frame_effects,
                )
            }
            Command::SellWallAtCell { x, y } => {
                let Some(overlays) = overlay_registry else {
                    return false;
                };
                // Event 0x4C6FC4 passes 0; ordinary SELL remains audible.
                self.sell_wall_at_cell(command_owner, *x, *y, false, rules, overlays, frame_effects)
            }
            // Event1/Event2 (0x004C6CB0 branches 0x004C6CF0/0x004C6D4D) drive the
            // BuildingClass+0x660 power latch through GoOnline 0x00452260 /
            // GoOffline 0x00452360. go_building_online/offline carry the native
            // semantics (idempotent latch, house power + tech-tree invalidation,
            // Stuff flag, factory queue refresh, EVA line for a human house). The
            // dispatch has no owner check, and none is added here.
            Command::SetBuildingOnLine { entity_id, on } => {
                let Some(owner) = self.interner.get(command_owner) else {
                    return false;
                };
                if !self.houses.contains_key(&owner) {
                    return false;
                }
                // `0x004C6D09`/`0x004C6D17` (and `0x004C6D68`/`0x004C6D76` on the
                // 0x02 branch): a null target, one whose ObjectClass IsAlive
                // (`+0x90`) is clear, or one in Limbo (`+0x81`) is dropped
                // before GoOnline/GoOffline. `+0x81` is Limbo, not a busy flag:
                // `0x005438DB` sets it while clearing `+0x80` and detaching the
                // object's cells (Limbo), `0x00543805` clears it (Unlimbo), and
                // the create path at `0x0044EC68`/`0x0044ECC4` brackets object
                // construction with a save/set/restore of the same byte.
                let Some(entity) = self.substrate.entities.get(*entity_id) else {
                    return false;
                };
                if !entity.lifecycle.object_alive() || entity.lifecycle.in_limbo() {
                    return false;
                }
                let type_ref = entity.type_ref();
                // The branch's last gate reads `[target+0x520]+0x16C0`, and
                // `+0x520` is `BuildingClass::pType` (docs/ghidra-workflow.md):
                // the target TYPE's `FirestormWall=` byte
                // (`ObjectType.firestorm_wall`). A Firestorm Wall building is
                // driven by the firestorm logic, not an On/Off Line event, so
                // the record is dropped. Stock rules leave the key unset.
                if self
                    .object_type(type_ref, rules)
                    .is_some_and(|object| object.firestorm_wall)
                {
                    return false;
                }
                if *on {
                    self.go_building_online(*entity_id, rules, frame_effects)
                } else {
                    self.go_building_offline(*entity_id, rules, frame_effects)
                }
            }
            // Offline game-speed transitions are consumed at master-frame
            // ingress so early authoritative animation work sees the new rate.
            // Reaching the ordinary EventClass-shaped tail must not apply one.
            Command::SetGameSpeed { .. } => false,
            // 0x0D (type 13, `0x004C794B`) stores the setting at the
            // EventClass tail, before any House check.
            Command::LiveGameSpeed { speed } => self.session.apply_in_game_speed(*speed),
            Command::Cheer => {
                let Some(owner) = self.interner.get(command_owner) else {
                    return false;
                };
                if !self.houses.contains_key(&owner) {
                    return false;
                }
                self.house_cheer(owner, rules, frame_effects);
                true
            }
            Command::Destruct => {
                let Some(owner) = self.interner.get(command_owner) else {
                    return false;
                };
                // Type-8 entry 0x004C7BDF: Flag_To_Die on the header House,
                // with no human or mode test. Destruction waits for the
                // House rung (Update 0x004F87C2).
                self.flag_house_to_die(owner);
                true
            }
            Command::ExitMatch => {
                let Some(owner) = self.interner.get(command_owner) else {
                    return false;
                };
                let Some(house) = self.houses.get_mut(&owner) else {
                    return false;
                };
                // Execute_DoList's EXIT block (0x0064C7BF..0x0064C862), for
                // every sender: House+0x248, then Flag_To_Die when no other
                // human House is undefeated (0x005E2BA0).
                house.mark_left_game();
                if self.other_undefeated_humans(owner) == 0 {
                    self.flag_house_to_die(owner);
                }
                // A Simulation without a current House (diagnostic and replay
                // drivers) has no remote senders.
                if self
                    .session
                    .current_house()
                    .is_none_or(|local| local == owner)
                {
                    // The local sender's EventClass__Execute (0x004C7903)
                    // writes the termination byte at 0x004C7917. The app
                    // consumes the owner-tagged edge after this tail.
                    if !self.quit_requested {
                        self.executed_exit_owner = Some(owner);
                    }
                    self.quit_requested = true;
                } else {
                    // A remote sender never reaches Execute (0x0064C91B):
                    // the app runs DestroyConnection(house, 0) for it
                    // (0x0064C9B1).
                    self.departed_owners.push(owner);
                }
                true
            }
            Command::MoveFlash { rx, ry } => self.spawn_move_flash(rules, (*rx, *ry)),
            Command::Ally { house_index } => {
                // EventClass::Execute 0x004C6DEE..0x004C6E23: no admission
                // gate; the sender toggles on its own mask, announced.
                let (Some(owner), Some(other)) = (
                    self.interner.get(command_owner),
                    usize::try_from(*house_index)
                        .ok()
                        .and_then(|index| self.houses.order().get(index).copied()),
                ) else {
                    return false;
                };
                if crate::sim::combat::combat_weapon::is_ally_by_object(
                    Some(self.house_alliances()),
                    &self.interner,
                    owner,
                    other,
                ) {
                    self.make_enemy(owner, other, rules, overlay_registry, frame_effects, true);
                } else {
                    self.make_ally(owner, other, rules, overlay_registry, frame_effects, true);
                }
                true
            }
            Command::RemovePlayer { house_index } => {
                // EventClass::Execute 0x004C7B73: IsHumanControlled
                // (0x0050B730, House+0x1EC outside mode 0), then
                // Flag_To_Die (0x004C7BE1). A WOL tournament game skips the
                // human test (0x004C7BBF..0x004C7BCE).
                // RESIDUAL: mode 4's ladder-stats call (0x004C7BA0..0x004C7BBA)
                // is backend.
                let Some(owner) = usize::try_from(*house_index)
                    .ok()
                    .and_then(|index| self.houses.order().get(index).copied())
                else {
                    return false;
                };
                if !self.session.wol_tournament_game()
                    && !self
                        .houses
                        .get(&owner)
                        .is_some_and(|house| house.is_human())
                {
                    return false;
                }
                self.flag_house_to_die(owner);
                true
            }
            // Event4C6ED2 resolves Techno and detaches planning6386E0 above,
            // before its only admission: Object+90. Accepted events survive
            // capture; neither sender ownership nor CanRepair is rechecked.
            // Source: original gamemd.exe instructions4C6ED2..4C6F01.
            Command::ToggleRepair { entity_id } => {
                if !self
                    .substrate
                    .entities
                    .get(*entity_id)
                    .is_some_and(|entity| entity.lifecycle.object_alive())
                {
                    return false;
                }
                production::toggle_repair(
                    self,
                    rules,
                    *entity_id,
                    production::RepairControl::Toggle,
                )
            }
            Command::UnloadPassengers { transport_id } => {
                if !self.entity_owned_by_id(command_owner, *transport_id) {
                    return false;
                }
                let has_passengers = self.substrate.entities.get(*transport_id).is_some_and(|t| {
                    !t.passenger_role.occupants().is_empty()
                        || t.passenger_role
                            .cargo()
                            .is_some_and(|cargo| !cargo.is_empty())
                });
                // A gap generator's self click sends the same Unload
                // (`0x004434FF..0x00443514`) with no cargo; its mission
                // toggles the super-charge.
                let gap_generator = self
                    .substrate
                    .entities
                    .get(*transport_id)
                    .filter(|t| t.category() == crate::map::entities::EntityCategory::Structure)
                    .and_then(|t| self.interner.object_type(t.type_ref(), rules))
                    .is_some_and(|object| object.gap_generator);
                if !has_passengers && !gap_generator {
                    return false;
                }
                let category = self
                    .substrate
                    .entities
                    .get(*transport_id)
                    .map(|t| t.category());
                match category {
                    Some(crate::map::entities::EntityCategory::Structure) => {
                        // A garrison's Unload is its own mission
                        // (`BuildingClass::Mission_Unload @ 0x0044D880`),
                        // commenced at the building's ready checks.
                        //
                        // RESIDUAL: this payload stands for two native events.
                        // The self-click sends a MEGAMISSION Unload whose
                        // target token is the building itself
                        // (`BuildingClass::ClickedAction_Object @ 0x00443410`
                        // -> ClickedMission `0x006FFBE0`); the deploy key
                        // sends DEPLOY (`0x00730AF0`, vt+0x374(9)). Here the
                        // order queues Unload through the MEGAMISSION prefix
                        // alone, without the event's target and destination
                        // setters. Trigger: every garrison unload. Effect:
                        // the building's target is not run through
                        // BuildingClass::SetTarget with the event's token.
                        // Risk: low; Mission_Unload empties it either way.
                        // The self-click's flash of its own token is kept.
                        if !self.order_actor_admits(*transport_id) {
                            return false;
                        }
                        self.queue_megamission(
                            *transport_id,
                            MissionType::Unload,
                            None,
                            None,
                            rules,
                            frame_effects,
                        );
                        Self::flash_order_object(
                            &self.interner,
                            &self.session,
                            &self.fog,
                            &self.house_alliances,
                            &mut self.substrate,
                            *transport_id,
                            Some(combat::TargetKind::Entity(*transport_id)),
                            None,
                        );
                        true
                    }
                    Some(crate::map::entities::EntityCategory::Unit)
                    | Some(crate::map::entities::EntityCategory::Aircraft) => {
                        // Vehicle/aircraft transports unload through the Unload
                        // mission (`UnitClass::Mission_Unload @ 0x0073D630`
                        // transport branch; Aircraft slot `0x004151E0`).
                        let is_transport = self
                            .substrate
                            .entities
                            .get(*transport_id)
                            .and_then(|t| self.object_type(t.type_ref(), rules))
                            .is_some_and(|obj| obj.passengers > 0);
                        if !is_transport {
                            return false;
                        }
                        if category == Some(crate::map::entities::EntityCategory::Aircraft) {
                            if !self.order_actor_admits(*transport_id) {
                                return false;
                            }
                            // Aircraft readiness has no live transition-latch
                            // producer, so a queued mission would never
                            // promote; commit through Assign.
                            //
                            // RESIDUAL: native's self-click sends an aircraft
                            // the same MEGAMISSION (AircraftClass slot +0x144
                            // case 4, `0x00417BD0`), whose NULL target and
                            // destination setters this arm does not run.
                            // Dormant: no retail `[AircraftTypes]` entry has
                            // `Passengers=`.
                            let now = self.session.binary_frame();
                            let _ = self.substrate.mission_assign_exact(
                                *transport_id,
                                crate::sim::mission::MissionId::from_known(MissionType::Unload),
                                now,
                            );
                            return true;
                        }
                        // The self-click's `ClickedMission(Unload, NULL, NULL)`
                        // (FootClass slot +0x144 case 4, `0x004D74E0`): the
                        // event's NULL target and destination stop a moving
                        // transport where it is, to unload there.
                        //
                        // Unit transport D uses deploy::issue_order instead:
                        // Event9 (`0x004C76D7..0x004C7818`) retains its own
                        // admission guards and archive/suspended state.
                        self.apply_megamission_command(
                            *transport_id,
                            MissionType::Unload,
                            None,
                            None,
                            rules,
                            overlay_registry,
                            frame_effects,
                        )
                        .accepted()
                    }
                    _ => false,
                }
            }
            Command::PlantC4 {
                attacker_id,
                target_building_id,
            } => {
                if !self.entity_owned_by_id(command_owner, *attacker_id) {
                    return false;
                }
                if self
                    .substrate
                    .entities
                    .get(*attacker_id)
                    .is_some_and(|e| e.is_deployed())
                {
                    return false;
                }
                // Validate attacker has C4=yes flag.
                let c4_ok = self.substrate.entities.get(*attacker_id).and_then(|e| {
                    let obj = self.object_type(e.type_ref(), rules)?;
                    obj.c4.then_some(())
                });
                if c4_ok.is_none() {
                    return false;
                }
                // Validate target is a CanC4, non-invisible building, not iron-curtained.
                // These gates stand in for the cursor's (What_Action), which
                // is unread for a building being sold (Mission 0x13); the
                // arrival plants nothing on one (`sim::c4_plant`).
                let target_info = self
                    .substrate
                    .entities
                    .get(*target_building_id)
                    .and_then(|b| {
                        if b.category() != crate::map::entities::EntityCategory::Structure {
                            return None;
                        }
                        if b.death.is_dying() {
                            return None;
                        }
                        let obj = self.object_type(b.type_ref(), rules)?;
                        if !obj.can_c4 || obj.invisible_in_game {
                            return None;
                        }
                        if crate::sim::superweapon::invulnerability::is_invulnerable(
                            b.invulnerability.as_ref(),
                            self.session.binary_frame(),
                        ) {
                            return None;
                        }
                        // The class setter samples the building's +4C.
                        crate::sim::movement::nav_target_coordinate(
                            crate::sim::components::NavTargetRef::object(*target_building_id),
                            Some(*attacker_id),
                            self,
                            self.resolved_terrain.as_ref(),
                            Some((rules, &self.interner)),
                        )
                        .ok()?;
                        Some(())
                    });
                let Some(()) = target_info else {
                    return false;
                };
                // The command carries the selected Sabotage action. Native
                // What_Action permits Ctrl on allies (7003C4..700406); the
                // click arm4D7788 emits mission17. That arm and arrival
                // 51A4BF..51A664 do not test the target's alliance.
                // Event4C747C: Set_Destination(target building, 1). The building is
                // the NavCom, so the Infantry +1AC admits its footprint cells to a
                // Sabotage C4 carrier (`0x0051C2D3..`); the walk ends inside, where
                // the arrival plants the charge (`sim::c4_plant`).
                self.apply_megamission_command(
                    *attacker_id,
                    MissionType::Sabotage,
                    None,
                    Some(combat::TargetKind::Entity(*target_building_id)),
                    rules,
                    overlay_registry,
                    frame_effects,
                )
                .accepted()
            }
            Command::CaptureBuilding {
                engineer_id,
                target_building_id,
            } => {
                if !self.entity_owned_by_id(command_owner, *engineer_id) {
                    return false;
                }
                // A Building destination with a coordinate; the event arm
                // gates its liveness. Capture's class arrival owner evaluates
                // its specific entry action.
                let target_info = self
                    .substrate
                    .entities
                    .get(*target_building_id)
                    .and_then(|b| {
                        if b.category() != crate::map::entities::EntityCategory::Structure {
                            return None;
                        }
                        // The class setter samples the building's +4C.
                        crate::sim::movement::nav_target_coordinate(
                            crate::sim::components::NavTargetRef::object(*target_building_id),
                            Some(*engineer_id),
                            self,
                            self.resolved_terrain.as_ref(),
                            Some((rules, &self.interner)),
                        )
                        .ok()
                    });
                if target_info.is_none() {
                    return false;
                }
                // Infantry51F190 maps actions28/29 toFootaction9 before
                // delayed Event delivery. Mutable type, relation and HP
                // action predicates are re-evaluated only at actual arrival.
                // Event4C747C dispatches Infantry51AA40 with the Building
                // token; the class publishes NavCom and the Walk destination
                // after its prelude.
                self.apply_megamission_command(
                    *engineer_id,
                    MissionType::Capture,
                    None,
                    Some(combat::TargetKind::Entity(*target_building_id)),
                    rules,
                    overlay_registry,
                    frame_effects,
                )
                .accepted()
            }
            Command::LaunchSuperWeapon {
                sw_type_id,
                target_rx,
                target_ry,
            } => {
                // The SPECIAL_PLACE event (`EventClass::Execute 0x004C78D6`).
                let owner = self.interner.intern(command_owner);
                self.fire_super_weapon(
                    rules,
                    owner,
                    *sw_type_id,
                    (*target_rx, *target_ry),
                    overlay_registry,
                    frame_effects,
                    false,
                )
            }
            Command::CompletePaidSuperWeapon {
                selection,
                target_rx,
                target_ry,
            } => {
                let owner = self.interner.intern(command_owner);
                self.fire_super_weapon_selected(
                    rules,
                    owner,
                    selection.destination,
                    (*target_rx, *target_ry),
                    overlay_registry,
                    frame_effects,
                    false,
                    Some(*selection),
                )
            }
            Command::CancelPaidSuperWeapon { selection } => {
                let owner = self.interner.intern(command_owner);
                Self::cancel_paid_super_weapon_selection(
                    &mut self.substrate,
                    &mut self.sound_events,
                    &mut self.super_weapons,
                    rules,
                    owner,
                    *selection,
                )
            }
            Command::BeginPaidSuperWeapon {
                selection,
                target_rx,
                target_ry,
            } => {
                let owner = self.interner.intern(command_owner);
                self.fire_super_weapon_selected(
                    rules,
                    owner,
                    selection.source,
                    (*target_rx, *target_ry),
                    overlay_registry,
                    frame_effects,
                    false,
                    Some(*selection),
                )
            }
            Command::EjectBunker { bunker_id } => {
                if !self.entity_owned_by_id(command_owner, *bunker_id) {
                    return false;
                }
                let has_occupant = self
                    .substrate
                    .entities
                    .get(*bunker_id)
                    .is_some_and(|b| b.bunker.occupant().is_some());
                if !has_occupant {
                    return false;
                }
                crate::sim::docking::bunker_link::release_normal(
                    self,
                    *bunker_id,
                    rules,
                    frame_effects,
                );
                true
            }
        }
    }

    /// The rally click's event 0x1E for each selected factory:
    /// `BuildingClass::SetRallyPoint @ 0x00443860` queues it and
    /// [`Self::execute_archive_event`] runs it, so the factory archives the
    /// cell (its only store). Input uses [`Self::takes_rally_point`] before
    /// issuing it. The accepted event does not repeat ownership/admission:
    /// native4C6DAA..4C6DDE tests only the resolved actor's Alive byte. A
    /// capture between enqueue and dispatch must retain an accepted event,
    /// even if a full OutList refuses the new owner's replacement rally.
    ///
    /// The input owner `factory_rally_cell_input` has already run native
    /// Map56DC20's nearby-passable search separately for each producer. This
    /// synchronized event stores that prepared target without searching again.
    fn set_rally_point_for_producers(&mut self, producer_ids: &[u64], rx: u16, ry: u16) -> bool {
        for &stable_id in producer_ids {
            self.execute_archive_event(
                stable_id,
                Some(crate::sim::combat::TargetKind::Cell(rx, ry)),
            );
        }
        // `EVA_NewRallyPointEstablished` is not a sim fact: `BuildingClass::
        // SetRallyPoint 0x00443A69` speaks inside the click handler, after the
        // rally EventClass is pushed and before it executes — the app owns it.
        true
    }

    /// `EventClass::Execute @ 0x004C6DAA`'s event 0x1E (ARCHIVE) for
    /// `entity_id`: its planning tokens are cleared (`0x006386E0`, event
    /// NULL), a dead object (`+0x90`) is skipped, and otherwise
    /// `Set_ArchiveTarget @ 0x0070C610` stores `target` as the event resolves
    /// it ([`Self::resolve_event_token`]): null for an object no longer
    /// present, while a Cell always resolves. Returns whether it archived.
    fn execute_archive_event(
        &mut self,
        entity_id: u64,
        target: Option<crate::sim::combat::TargetKind>,
    ) -> bool {
        self.planning_detach_actor(entity_id);
        // The target resolves only for a live object (`0x004C6DCD` precedes
        // `0x006E6E20`), so a dead one stamps no fallback Cell.
        if !self
            .substrate
            .entities
            .get(entity_id)
            .is_some_and(|object| object.lifecycle.object_alive())
        {
            return false;
        }
        let target = self.resolve_event_token(target);
        if let Some(object) = self.substrate.entities.get_mut(entity_id) {
            object.set_archive_target(target);
        }
        true
    }

    /// Building44377E admits HasRallyPoint OR UndeploysInto; the latter
    /// archives the move goal before SELL. ConstructionYards must also pass
    /// the cell click's44374F..443778 redeploy gates.
    pub fn takes_rally_point(&self, stable_id: u64, owner: &str, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return false;
        };
        if entity.category() != crate::map::entities::EntityCategory::Structure
            || !owner.eq_ignore_ascii_case(self.interner.resolve(entity.owner()))
        {
            return false;
        }
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return false;
        };
        if object.undeploys_into.is_some() {
            self.can_undeploy_building_runtime(stable_id, rules)
                && (!object.construction_yard || self.session.game_mode_nonzero())
        } else {
            object.has_rally_line()
        }
    }

    /// Replace the current selection with exactly the given stable entity IDs.
    ///
    /// Mirrors gamemd's mutation flow: omitted old members are deselected,
    /// requested old members retain their existing admission, and only genuinely
    /// new members run through `ObjectClass::Select`. This distinction matters
    /// for an already-selected Chrono unit in warp-out: warp blocks a fresh
    /// selection, but does not retroactively remove an existing one.
    /// VERA-only: applies the local `Command::Select` snapshot, its source of truth, to
    /// IsSelected `Object+0x83` through `Deselect @ 0x005F44A0` and `Select @ 0x005F4520`.
    ///
    /// RESIDUAL: the input ledger resolves a mutation's Select calls, their
    /// planning expansion (`0x00637840`) and Unselect_All drains included;
    /// this then Selects only the final new members, in vector order, with
    /// the `0x00AC4CF6` guard clear. So (a) a Select a partner's Selected tag
    /// event (33) starts expands its own tail node, where native's guard
    /// returns early; (b) an object Selected and then drained within one
    /// mutation, or a selected object drained and Selected again within it,
    /// raises no event 33 (native raises one per Select, `0x006FBFE4`; the
    /// re-Selected object keeps its bit here, so `try_select_object` sees it
    /// already selected); (c) the new members' events run in vector order, not
    /// call order. Trigger: a tag with event 33 on a planning partner (a);
    /// on an object a later Select of the same mutation drains, which takes
    /// another house's object on the player's planning node (b); or on two
    /// objects one mutation selects (c). Effect: extra partners selected, or
    /// a missing or reordered event 33 and the actions it runs (RNG draws
    /// included). Frequency: rare; needs Selected triggers, and (a, b)
    /// Planning Mode. Downstream risk: the selection's later orders, and
    /// whatever those trigger actions change.
    fn apply_selection_snapshot(
        &mut self,
        stable_ids: &[u64],
        context: super::UninitContext<'_>,
    ) -> bool {
        let requested: BTreeSet<u64> = stable_ids.iter().copied().collect();
        // Deselect only omitted old members. Requested old members remain set,
        // so the final-admission gates below are never reapplied to them.
        let keys: Vec<u64> = self.substrate.entities.keys_sorted();
        for &id in &keys {
            if !requested.contains(&id)
                && let Some(e) = self.substrate.entities.get_mut(id)
            {
                e.selection.deselect();
            }
        }
        // Iterate the original payload, not the membership set: source order is
        // authoritative even though this sim layer stores only selected bits.
        for &stable_id in stable_ids {
            self.try_select_object(stable_id, context);
        }
        true
    }

    /// Object5F6C30 (+138), with Foot4DFA50's +6AD guard for mobile objects.
    /// Its disguise arm (+C8 with a NULL +D0(1)) is
    /// [`crate::sim::game_entity::GameEntity::disguise_hides_house_from`]; it
    /// does not treat current fog as selectability.
    pub fn object_is_selectable(&self, stable_id: u64, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return false;
        };
        if entity.category() != crate::map::entities::EntityCategory::Structure
            && entity.lift.swap_active()
        {
            return false;
        }
        if self.session.current_house().is_some_and(|house| {
            entity.disguise_hides_house_from(
                house,
                Some(&self.fog),
                Some(self.house_alliances.as_map()),
                &self.interner,
            )
        }) {
            return false;
        }
        self.interner
            .object_type(entity.type_ref(), rules)
            .is_none_or(|object| object.selectable)
    }

    /// Object5F4520's state/type admission, shared by provisional local input
    /// and committed selection. Membership and pending placement are caller
    /// state: input checks its newest ledger/placement, the sim its stored bit.
    /// Original does not read Alive or Health here; search/click callers do.
    pub fn can_select_object(&self, stable_id: u64, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return false;
        };
        !entity.lifecycle.in_limbo()
            && self.object_is_selectable(stable_id, rules)
            && !(entity.is_mission_only() && self.techno_player_controllable(stable_id, rules))
            && !entity.is_warped_out()
    }

    /// Commit an input-admitted Techno selection through Object5F4520, then
    /// its successful Techno6FBFD0..6FBFE4 Event33 callback (Steam gamemd).
    /// The app's input ledger applies TechnoClass::Select's gate
    /// ([`Self::techno_select_admits`]) and Unselect_All
    /// ([`Self::select_unselects_all`]) before emitting the ordered Select
    /// command. Keep the graphical-client B73550 gate there: replaying this
    /// command must not depend on a renderer.
    /// Raw lifecycle Select writers use Selection::select and do not raise33.
    pub(crate) fn try_select_object(
        &mut self,
        stable_id: u64,
        context: super::UninitContext<'_>,
    ) -> bool {
        self.object_select(stable_id, false, context)
    }

    /// `TechnoClass::Select @ 0x006FBFA0`'s gate (`0x006FBFA3..0x006FBFC3`):
    /// the current house has discovered the object (`+0x41B`) or its owner
    /// is the human player (`HouseClass::IsHumanPlayer @ 0x0050B6F0`). Its
    /// third arm, a zero main window handle (`0x00B73550`), never holds in a
    /// running game. Every Select of a Techno passes it: the game's own
    /// ([`Self::techno_select`]) and each Select the app's input ledger
    /// resolves, planning partners included.
    pub fn techno_select_admits(&self, stable_id: u64) -> bool {
        self.substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                entity.discovery.discovered_by_current_house()
                    || self.house_is_human_player(entity.owner())
            })
    }

    /// `ObjectClass::Select @ 0x005F4520` into a non-empty selection whose
    /// first object (current-objects `[0]`) is `first`
    /// (`0x005F45AF..0x005F45F8`): Unselect_All (`0x0048DC90`) runs unless
    /// both owners are the human player (`0x0050B6F0`), so a selection holds
    /// either the player's objects or one other object. Both Select owners
    /// decide with this: the input ledger, which knows `[0]`, and
    /// [`Self::object_select`].
    pub fn select_unselects_all(&self, first: u64, new: u64) -> bool {
        let human = |id: u64| {
            self.substrate
                .entities
                .get(id)
                .is_some_and(|entity| self.house_is_human_player(entity.owner()))
        };
        let first_human = human(first);
        first_human != human(new) || !first_human
    }

    /// ObjectClass::Deselect @ 0x005F44A0: an unselected object does
    /// nothing; otherwise remove it from selection, then release Follow
    /// (0x005F4503..0x005F4513). The app owns the retained input ledger.
    pub(crate) fn object_deselect(&mut self, stable_id: u64) {
        if let Some(entity) = self.substrate.entities.get_mut(stable_id)
            && entity.selection.is_selected()
        {
            entity.selection.deselect();
            self.lifecycle_outputs
                .emit(super::LifecycleOutput::FollowReleased { stable_id });
        }
    }

    /// `ObjectClass::Select @ 0x005F4520` and the Selected tag event that
    /// `TechnoClass::Select` raises after it (`0x006FBFD0..0x006FBFE4`). A
    /// Select the game makes itself (`logic`) first runs Unselect_All
    /// (`0x0048DC90`: Deselect `0x005F44A0` of every selected object, each
    /// releasing Follow, [`LifecycleOutput::FollowReleased`]) when
    /// [`Self::select_unselects_all`] says so, then reports its insertion
    /// into the current-objects vector ([`LifecycleOutput::ObjectSelected`]),
    /// before the tag event can Select anything else. An input Select arrives
    /// as a snapshot its owner already resolved.
    ///
    /// The simulation keeps selected bytes, not the vector's order, so this
    /// reads the first selected object in store order as `[0]`. Every
    /// Select keeps a selection all the player's or a single other object,
    /// so any member answers as `[0]` would; they differ only if a selected
    /// object changed owner and stayed selected.
    ///
    /// [`LifecycleOutput::ObjectSelected`]: super::LifecycleOutput::ObjectSelected
    /// [`LifecycleOutput::FollowReleased`]: super::LifecycleOutput::FollowReleased
    fn object_select(
        &mut self,
        stable_id: u64,
        logic: bool,
        context: super::UninitContext<'_>,
    ) -> bool {
        if !self.can_select_object(stable_id, context.rules())
            || self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|e| e.selection.is_selected())
        {
            return false;
        }
        if logic {
            let first = self
                .substrate
                .entities
                .values()
                .find(|entity| entity.selection.is_selected())
                .map(|entity| entity.stable_id());
            if first.is_some_and(|first| self.select_unselects_all(first, stable_id)) {
                for id in self.substrate.entities.keys_sorted() {
                    self.object_deselect(id);
                }
            }
        }
        let Some(entity) = self.substrate.entities.get_mut(stable_id) else {
            return false;
        };
        entity.selection.select();
        if logic {
            self.lifecycle_outputs
                .emit(super::LifecycleOutput::ObjectSelected { stable_id });
        }
        // Selection has already changed when the synchronous Tag actions run.
        // They may change ownership, remove the Tag, or retire the object.
        self.dispatch_object_tag_event(stable_id, 33, Some(stable_id), None, context);
        true
    }

    /// `TechnoClass::Select @ 0x006FBFA0` (vt+0x14C) for a logic caller: an
    /// object [`Self::techno_select_admits`] refuses is not selected; then
    /// ObjectClass::Select and the tag event ([`Self::object_select`]);
    /// then, for a human player's
    /// or passive house's object while the selection-voice latch
    /// `0x00822CF2` is set (`0x006FBFF4..0x006FC018`), VoiceSelect (vt+0x360)
    /// through [`Self::selection_voice_request`]; last, the planning-node add
    /// (`0x006FC020`, [`Self::planning_select_tail_node`]), which in the local
    /// player's Planning Mode Selects this object's tail-node partners
    /// through this same function. A passenger or parasite keeps its path
    /// through Limbo, so a reselected escapee or released parasite pulls in
    /// its node's partners.
    pub(crate) fn techno_select(
        &mut self,
        stable_id: u64,
        voice_latch: bool,
        context: super::UninitContext<'_>,
    ) -> bool {
        if !self.techno_select_admits(stable_id) || !self.object_select(stable_id, true, context) {
            return false;
        }
        let rules = context.rules();
        if let Some(sound_id) = self.selection_voice_request(rules, stable_id, voice_latch) {
            let sound_id = sound_id.to_owned();
            self.sound_events.emit(SimSoundEvent::UnitVoiceRequested {
                owner: stable_id,
                sound_id,
            });
        }
        self.planning_select_tail_node(stable_id, voice_latch, context);
        true
    }

    /// Check ownership using stable_id via EntityStore.
    ///
    /// Used only by receivers whose contract includes an owner gate. The
    /// MEGAMISSION receiver4C71CA..4C74CA resolves its actor independently of
    /// the event house; its admission checks lifetime and tokens instead.
    pub(crate) fn entity_owned_by_id(&self, command_owner: &str, stable_id: u64) -> bool {
        self.substrate
            .entities
            .get(stable_id)
            .is_some_and(|e| command_owner.eq_ignore_ascii_case(self.interner.resolve(e.owner())))
    }

    /// The acting object's half of the native order-admission gate.
    ///
    /// Before the synchronized order path touches anything it requires the
    /// acting object to be present, natively alive, above zero strength and out
    /// of limbo. Any failure abandons the **whole** order, so the object keeps
    /// the mission, target and destination it already had rather than being
    /// retasked. `in_limbo` is a genuinely independent byte here — an object
    /// riding inside a transport, sitting in a tank bunker or garrisoning a
    /// building is alive and at full strength but still refuses orders.
    ///
    /// Note the strength test is `> 0` for the actor and merely `!= 0` for a
    /// target/destination token (see [`Simulation::order_object_token_admits`]);
    /// the two collapse to the same predicate on VERA's unsigned HP but the
    /// asymmetry is preserved so a future signed-HP change keeps the native
    /// meaning.
    ///
    /// Residuals on this gate, recorded not fixed:
    ///
    /// * **EjectBunker is ungated.** `Simulation::command_uses_megamission`
    ///   enumerates which VERA commands are MEGAMISSION-shaped — it is what
    ///   splits due commands into the non-MEGAMISSION pass and the staged
    ///   batch, mirroring opcode 0x04 in `net::lockstep`. Checked against it,
    ///   `EjectBunker` carries only the ownership test. Trigger: issuing it to
    ///   a dying or limboed bunker. Player effect: the order runs where retail
    ///   abandons it. Frequency: low per order.
    /// * Aircraft reservation teardown remains a legacy command policy.
    /// * **The manager abandon** at 0x004C73E1-0x004C73EA calls 0x006B0C80 on
    ///   `[actor+0x2D8]`, the SlaveManagerClass (not a spawn manager), whenever
    ///   the queued mission is not Attack; it is not Foot-gated. The funnel
    ///   below and Stop's own copy
    ///   (0x004C769C-0x004C76AC) run it (`Simulation::reset_slave_manager`);
    ///   the other orders outside the funnel do not yet.
    /// * **`TeamClass__Remove_Member`** runs in the MEGAMISSION funnel
    ///   and in its shared prefix; EjectBunker and the dormant aircraft Unload
    ///   arm bypass that prefix and keep a team member in its team.
    pub(crate) fn order_actor_admits(&self, stable_id: u64) -> bool {
        self.substrate.entities.get(stable_id).is_some_and(|e| {
            e.lifecycle.object_alive() && e.health.current() > 0 && !e.lifecycle.in_limbo()
        })
    }

    /// The Target/Destination half of the native order-admission gate.
    ///
    /// When the order names an object, that object is subjected to the same
    /// three tests as the actor, and a failure abandons the whole order. This
    /// is what keeps an attacker on its previous order when the unit it was
    /// clicked onto dies in the same tick, instead of retasking it onto a
    /// corpse that is still resolvable in the store.
    /// `EventClass::Execute @ 0x004C6CB0`'s MEGAMISSION token tests of `+0x90`, `+0x6C` != 0
    /// and `+0x81` (target `0x004C7225..0x004C725D`, destination `0x004C7263..0x004C7299`).
    pub(crate) fn order_object_token_admits(&self, stable_id: u64) -> bool {
        if let Some(e) = self.substrate.entities.get(stable_id) {
            e.lifecycle.object_alive() && e.health.current() != 0 && !e.lifecycle.in_limbo()
        } else {
            // Event4C7225..725D reads Object lifetime/Health, not Techno RTTI.
            self.terrain_objects
                .objects()
                .get(&stable_id)
                .is_some_and(|target| target.is_live() && target.health() != 0)
        }
    }

    /// Whether a re-issued Enter order onto `destination_id` is the native
    /// duplicate-Enter no-op: the receiver's committed mission is already
    /// `Enter`, the destination is a Building, and the receiver is already
    /// radio-linked to that same building. The synchronized order path returns
    /// without touching a single field in that case — no radio break, no queued
    /// mission, no re-path — so a player who re-clicks a garrison, service
    /// depot, Grinder or bunker while the unit is already at the door does not
    /// see it stall and re-approach.
    ///
    /// Structures never take this branch (it is gated on the Foot bit), and the
    /// link tested is contact slot 0, the same slot the original reads.
    /// `EventClass::Execute @ 0x004C6CB0`, MEGAMISSION `0x004C729F..0x004C72E2`: an Enter event,
    /// `+0xAC` Enter, the Foot bit, then the destination token converted
    /// (`0x004C72BF`, which stamps the fallback CellClass for a Cell off the
    /// map), a Building, and contact slot 0 (`+0xE4`, `0x0065AD30`).
    pub(crate) fn duplicate_enter_is_noop(
        &self,
        receiver_id: u64,
        destination: Option<combat::TargetKind>,
    ) -> bool {
        let Some(receiver) = self.substrate.entities.get(receiver_id) else {
            return false;
        };
        if receiver.category() == crate::map::entities::EntityCategory::Structure {
            return false;
        }
        if receiver.mission.current()
            != crate::sim::mission::MissionId::from_known(MissionType::Enter)
        {
            return false;
        }
        let Some(combat::TargetKind::Entity(destination_id)) =
            self.resolve_event_token(destination)
        else {
            return false;
        };
        let destination_is_building = self
            .substrate
            .entities
            .get(destination_id)
            .is_some_and(|d| d.category() == crate::map::entities::EntityCategory::Structure);
        if !destination_is_building {
            return false;
        }
        receiver.radio_contacts.slot(0) == Some(destination_id)
    }

    /// TargetClass `0x006E6FF0`: the object an event token names, while it
    /// is still present. A Cell names none and is not looked up.
    pub(crate) fn event_token_object(&self, token: Option<combat::TargetKind>) -> Option<u64> {
        let combat::TargetKind::Entity(id) = token? else {
            return None;
        };
        (self.substrate.entities.contains(id) || self.terrain_objects.objects().contains_key(&id))
            .then_some(id)
    }

    /// TargetClass `0x006E6E20`: what an event token resolves to. An object
    /// no longer present resolves to NULL. A Cell always resolves, through
    /// `MapClass::Get_CellClass` (`0x006E6F05`), which stamps the shared
    /// fallback CellClass with a cell off the map: a Cell TarCom then reads
    /// that CellClass as it stands ([`combat::TargetKind::cell_identity`]).
    pub(crate) fn resolve_event_token(
        &self,
        token: Option<combat::TargetKind>,
    ) -> Option<combat::TargetKind> {
        match token? {
            combat::TargetKind::Entity(_) => self
                .event_token_object(token)
                .map(combat::TargetKind::Entity),
            combat::TargetKind::Cell(x, y) => {
                crate::sim::cell_rect::get_cellclass_fallback(
                    self.resolved_terrain.as_ref(),
                    i32::from(x),
                    i32::from(y),
                );
                token
            }
        }
    }

    /// The MEGAMISSION event's flash (`0x004C7385..0x004C739F`): vt+0x148
    /// Flash(7) ([`Self::flash_object`]) on one object when the current
    /// player (A83D4C) is allied with the ordering object
    /// (`HouseClass::IsAlliedWith(ObjectClass*) @ 0x004F9A90`: its owner, or
    /// a bit of the player's ally mask). `0x004C7225..0x004C727E` picks that
    /// object: As_Object of a present destination token, else of the target
    /// token, so a Cell destination flashes nothing. It runs after the
    /// actor/token refusals and the repeated-Enter exit, before the setters.
    fn flash_order_object(
        interner: &StringInterner,
        session: &ScenarioSession,
        fog: &FogState,
        house_alliances: &HouseAlliances,
        substrate: &mut ObjectSubstrate,
        actor_id: u64,
        target: Option<combat::TargetKind>,
        destination: Option<combat::TargetKind>,
    ) {
        let Some(combat::TargetKind::Entity(flashed)) = destination.or(target) else {
            return;
        };
        let Some(player) = session.current_house() else {
            return;
        };
        if substrate.entities.get(actor_id).is_some_and(|actor| {
            combat::combat_weapon::is_ally_by_object(
                Some(house_alliances.as_map()),
                interner,
                player,
                actor.owner(),
            )
        }) {
            Self::flash_object(
                interner,
                session,
                fog,
                house_alliances,
                substrate,
                flashed,
                7,
            );
        }
    }

    /// The MEGAMISSION event's arm (`EventClass::Execute`,
    /// `0x004C71F6..0x004C74BA`) and its one port. Move, Attack (Ctrl and
    /// Cell clicks included), attack-move (mission 29), Area Guard, a vehicle
    /// transport's Unload and Enter, Capture and Sabotage onto their object
    /// and Harvest run here. A tank bunker's Enter uses the same destination
    /// and radio owners.
    /// A garrison's ordinary Unload (one command
    /// for the self-click and the deploy key's DEPLOY event) and the dormant
    /// aircraft Unload retain their adapters. In native order:
    ///
    /// 1. The actor gate (`0x004C71F6..0x004C721F`,
    ///    [`Self::order_actor_admits`]), then each object token's (target
    ///    `0x004C7225..0x004C725D`, destination `0x004C7263..0x004C7299`,
    ///    [`Self::order_object_token_admits`]): a dead, zero-strength or
    ///    limboed object abandons the whole order before any write. An
    ///    object no longer present resolves to NULL
    ///    ([`Self::resolve_event_token`]) and the event runs.
    /// 2. A repeated Enter onto the building the actor is already linked to
    ///    returns before any write (`0x004C729F..0x004C72E2`,
    ///    [`Self::duplicate_enter_is_noop`]).
    /// 3. The shared prefix, `Assign_Target_Command`, Queue_Mission and the
    ///    archive, slave-manager and Foot clears
    ///    ([`Self::queue_megamission`]).
    /// 4. Area Guard on a Foot (`0x004C73EF..0x004C7430`): the target setter
    ///    takes NULL, and the target token becomes the destination and the
    ///    ArchiveTarget. The retained post belongs to the AreaGuard
    ///    handler (`0x004D6AA0`), including acquisition and return.
    /// 5. Otherwise the class target setter (vt+0x3C8, `0x004C7467`), the
    ///    class destination setter (vt+0x480 mode 1, `0x004C747C`) and, for
    ///    a non-null target, an open-topped transport's riders
    ///    (`0x004C7482..0x004C749D`). The setters are void, so a class
    ///    refusal still executes the event: BuildingClass::SetTarget
    ///    (`0x00443B90`) drops a target its weapon cannot reach and the
    ///    queued Attack stands, for Mission_Attack's null-target arm to hand
    ///    back to Guard (`techno_ai::building_missions`).
    ///
    /// The tail (`0x004C749D..0x004C74BA`) hands a non-NULL third token to
    /// the NavComAux setter (vt+0x47C). No represented order carries one:
    /// [`crate::sim::command::MegaMissionRecord::decode`] refuses a non-NULL
    /// auxiliary kind.
    ///
    /// The arm tests neither the actor's owner (`0x004C71CA` resolves it
    /// independently of the event house) nor friendship nor the actor's
    /// weapon; the input decides those (What_Action). The event has no
    /// queued form: a Shift-click still builds an ordinary MegaMission
    /// (walk_first_path.json `gi_reissue`).
    ///
    /// Before the setters the event flashes one object
    /// ([`Self::flash_order_object`]).
    fn apply_megamission_command(
        &mut self,
        entity_id: u64,
        mission: MissionType,
        target: Option<combat::TargetKind>,
        destination: Option<combat::TargetKind>,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> MegaMissionExecution {
        // A capture after issue does not revoke an event. Event4C7353 queues
        // Move and clears Target before the class applies its own refusal.
        // Infantry51AA40 tests human Doing; Unit741970 tests deployment
        // after the same-Nav/force arm. Original non-Deployer GI and delayed
        // deployed MTNK controls in walk_first_path.json retain queued2 even
        // when the void class setter leaves NavCom NULL. Decoded commands
        // must not repeat the input wrapper's separate no-order decision.
        if !self.order_actor_admits(entity_id) {
            return MegaMissionExecution::Refused;
        }
        for token in [target, destination] {
            if let Some(id) = self.event_token_object(token)
                && !self.order_object_token_admits(id)
            {
                return MegaMissionExecution::Refused;
            }
        }
        if mission == MissionType::Enter && self.duplicate_enter_is_noop(entity_id, destination) {
            return MegaMissionExecution::RepeatedEnter;
        }
        // Assign_Target_Command reads the event tokens only for mission 29.
        // The destination/radio owners handle any existing dock contact.
        self.queue_megamission(
            entity_id,
            mission,
            target,
            destination,
            rules,
            frame_effects,
        );
        Self::flash_order_object(
            &self.interner,
            &self.session,
            &self.fog,
            &self.house_alliances,
            &mut self.substrate,
            entity_id,
            target,
            destination,
        );
        // Each setter takes what its token resolves to, converted just before
        // it: a Cell's lookup can restamp the fallback CellClass.
        let foot = self
            .substrate
            .entities
            .get(entity_id)
            .is_some_and(|actor| actor.category() != EntityCategory::Structure);
        if mission == MissionType::AreaGuard && foot {
            self.assign_target_represented(entity_id, None, rules, overlay_registry, frame_effects)
                .unwrap_or_else(|cause| panic!("Area Guard order target: {cause}"));
            // The target token, converted at `0x004C7418` for the
            // destination and again at `0x004C7428` for the archive.
            let post = self.resolve_event_token(target);
            self.assign_destination_represented(
                entity_id,
                post.map(Into::into),
                rules,
                overlay_registry,
                frame_effects,
            )
            .unwrap_or_else(|cause| panic!("Area Guard order destination: {cause}"));
            // The archive write follows the virtual destination call even
            // when that class setter refuses its request.
            let archive = self.resolve_event_token(target);
            self.substrate
                .entities
                .get_mut(entity_id)
                .unwrap()
                .set_archive_target(archive);
            return MegaMissionExecution::Executed;
        }
        // `0x004C745B`, then the target setter.
        let target = self.resolve_event_token(target);
        // Its changed-null path6FCF5B also resets retained burst state;
        // dropping Target alone cannot.
        let _ = self.assign_target_represented(
            entity_id,
            target,
            rules,
            overlay_registry,
            frame_effects,
        );
        // `0x004C7474`, then the destination setter: Unit741970,
        // Infantry51AA40, Aircraft41AA80 or Building455D50, whatever the
        // locomotor. Each owns its same-NavCom return, preprocessing and
        // queue clearing. Native: track_destination.json Unit rows, replayed
        // by command_move_destinations_match_native_unit_setter; original
        // delayed 6E0 MTNK: walk_first_path.json.
        let destination = self.resolve_event_token(destination);
        if let Err(cause) = self.assign_destination_represented(
            entity_id,
            destination.map(Into::into),
            rules,
            overlay_registry,
            frame_effects,
        ) {
            log::warn!("MegaMission destination {entity_id}: {cause}");
        }
        if let Some(target) = target {
            self.open_topped_passengers_take_target(
                entity_id,
                Some(target),
                rules,
                overlay_registry,
                frame_effects,
            );
        }
        MegaMissionExecution::Executed
    }
}

#[cfg(test)]
#[path = "battle_opening_tests.rs"]
mod battle_opening_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;

    use crate::sim::mission::MissionId;
    use crate::sim::movement::locomotor::LocomotorState;

    pub(super) fn amcv_move_rules() -> RuleSet {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             0=AMCV\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [AMCV]\n\
             Strength=1000\n\
             Speed=4\n\
             Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             MovementZone=Normal\n\
             Crusher=yes\n\
             DeploysInto=GACNST\n",
        );
        RuleSet::from_ini(&ini).expect("amcv rules")
    }

    pub(super) fn spawn_rule_backed_unit(
        sim: &mut Simulation,
        sid: u64,
        type_id: &str,
        rules: &RuleSet,
    ) {
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern(type_id);
        let obj = rules.object(type_id).expect("object type");
        let health = obj.strength;
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            sid,
            20,
            20,
            0,
            0,
            owner,
            Health::new(health),
            type_ref,
            EntityCategory::Unit,
            0,
            obj.sight.max(0) as u16,
            true,
        );
        entity.locomotor = Some(LocomotorState::from_object_type(
            obj,
            sim.session.binary_frame(),
        ));
        entity.mover_facts.set_regular_crusher_for_test(obj.crusher);
        entity
            .mover_facts
            .set_drive_accelerates_for_test(obj.accelerates);
        entity
            .mover_facts
            .set_omni_crusher_for_test(obj.omni_crusher);
        // A directly-inserted GameEntity keeps the constructed `in_limbo`
        // byte; production spawns clear it through Reveal. Order admission
        // reads that byte, so the fixture must model a revealed object.
        entity.lifecycle.set_in_limbo_for_test(false);
        sim.substrate.entities.insert(entity);
    }

    #[test]
    fn accepted_archive_keeps_its_tokens_after_capture_and_resolves_missing_target_to_null() {
        use crate::sim::house_state::HouseState;
        let rules = amcv_move_rules();
        let mut sim = Simulation::new();
        let old_owner = sim.intern("Americans");
        let new_owner = sim.intern("British");
        for owner in [old_owner, new_owner] {
            sim.houses
                .register(HouseState::new(owner, 0, None, true, 0, 10));
        }
        spawn_rule_backed_unit(&mut sim, 42, "AMCV", &rules);
        spawn_rule_backed_unit(&mut sim, 43, "AMCV", &rules);
        sim.substrate
            .entities
            .get_mut(42)
            .unwrap()
            .set_archive_target(Some(combat::TargetKind::Cell(5, 6)));
        assert!(sim.issue_local_command(CommandEnvelope::new(
            old_owner,
            0,
            Command::ArchiveObject {
                entity_id: 42,
                target_id: 43,
            }
        )));
        let record = sim.network_command_queues().local_records().unwrap()[0].clone();
        assert_eq!(
            ArchiveRecord::decode(&record).unwrap().target,
            MegaMissionTarget::Object { id: 43 }
        );
        sim.change_owner_with_rules(42, new_owner, &rules, None, FrameEffects::default());
        sim.substrate.entities.remove(43);
        let queued = sim.take_frame_commands_for_test();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].owner, old_owner);
        assert!(sim.apply_command("Americans", &queued[0].payload, Some(&rules)));
        assert_eq!(
            sim.substrate.entities.get(42).unwrap().archive_target(),
            None
        );

        // Native6E6F0F resolves an unknown kind to NULL too. It must still
        // execute the archive setter, rather than silently dropping the event.
        sim.substrate
            .entities
            .get_mut(42)
            .unwrap()
            .set_archive_target(Some(combat::TargetKind::Cell(7, 8)));
        let mut null = record;
        null.payload_mut()[9] = 0xff;
        let command = sim.decode_native_command_record(&null, 0).unwrap();
        assert!(sim.apply_command("Americans", &command.payload, Some(&rules)));
        assert_eq!(
            sim.substrate.entities.get(42).unwrap().archive_target(),
            None
        );
    }

    #[test]
    fn accepted_repair_resolves_the_captured_building_and_stale_target_at_dispatch() {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = retail.rules;
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x4C6ED2);
        let old_owner = sim.interner.get("Americans").unwrap();
        let new_owner = sim.interner.get("Russians").unwrap();
        let building = sim
            .spawn_object("GAPILE", "Americans", 8, 8, 0, &rules)
            .unwrap();
        sim.substrate
            .entities
            .get_mut(building)
            .unwrap()
            .health
            .set(300);
        let record = sim.encode_repair_record(old_owner, building).unwrap();
        assert!(sim.admit_native_command_record(record.clone()));
        sim.change_owner_with_rules(building, new_owner, &rules, None, FrameEffects::default());
        assert!(
            !sim.substrate
                .entities
                .get(building)
                .unwrap()
                .repairing
                .is_on()
        );
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "repair", 0);
        let mut sim = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        sim.restore_after_snapshot_load().unwrap();
        let admitted = sim.take_frame_commands_for_test();
        assert_eq!(admitted.len(), 1);
        assert_eq!(admitted[0].owner, old_owner);
        let (accepted, draws) = crate::sim::rng::trace_draws(|| {
            sim.apply_command("Americans", &admitted[0].payload, Some(&rules))
        });
        assert!(accepted, "the old issuer is not receiver admission");
        assert!(draws.is_empty());
        assert!(
            sim.substrate
                .entities
                .get(building)
                .unwrap()
                .repairing
                .is_on()
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(building)
                .unwrap()
                .health
                .current(),
            300
        );
        sim.substrate.entities.remove(building);
        let stale = sim.decode_native_command_record(&record, 0).unwrap();
        assert!(!sim.apply_command("Americans", &stale.payload, Some(&rules)));
    }

    #[test]
    fn idle_record_preserves_unresolved_token_and_decodes_at_execution() {
        use crate::sim::house_state::HouseState;

        let rules = amcv_move_rules();
        let mut sim = Simulation::new();
        sim.interner.intern("Interner gap");
        let actor_owner = sim.interner.intern("Americans");
        let issuer = sim.interner.intern("British");
        sim.houses
            .register(HouseState::new(actor_owner, 0, None, true, 0, 10));
        sim.houses
            .register(HouseState::new(issuer, 1, None, true, 0, 10));
        sim.session.set_binary_frame_for_test(u32::MAX - 3);

        // Encoding must not resolve a stale identifier. The native token is
        // retained until4C74CE, independently of the issuer's House.
        let record = sim.encode_stop_record(issuer, 42).unwrap();
        assert_eq!(record.house_id(), 1);
        assert_eq!(record.frame_stamp(), -4);
        assert_eq!(StopRecord::decode(&record).unwrap().entity_id, 42);
        assert!(sim.decode_native_command_record(&record, 123).is_none());
        assert!(sim.encode_scatter_record(issuer, 42).is_none());

        spawn_rule_backed_unit(&mut sim, 42, "AMCV", &rules);
        let actor = sim.substrate.entities.get_mut(42).unwrap();
        actor.navigation.set_nav_list_for_test(
            vec![crate::sim::components::NavTargetRef::cell(12, 13)],
            false,
        );
        let command = sim.decode_native_command_record(&record, 123).unwrap();
        assert_eq!(command.owner, issuer);
        assert_eq!(command.execute_tick, 123);
        assert_eq!(command.payload, Command::Stop { entity_id: 42 });
        assert!(sim.apply_command("British", &command.payload, Some(&rules)));
        assert!(
            sim.substrate
                .entities
                .get(42)
                .unwrap()
                .navigation
                .nav_list()
                .is_empty()
        );
    }

    #[test]
    fn idle_record_rejects_unmapped_issuer_and_out_of_range_identifier() {
        use crate::sim::house_state::HouseState;

        let mut sim = Simulation::new();
        let issuer = sim.interner.intern("British");
        assert!(sim.encode_stop_record(issuer, 1).is_none());
        sim.houses
            .register(HouseState::new(issuer, 0, None, true, 0, 10));
        assert!(sim.encode_stop_record(issuer, i32::MAX as u64).is_some());
        assert!(
            sim.encode_stop_record(issuer, i32::MAX as u64 + 1)
                .is_none()
        );
    }

    // ===== Order admission (GSI-07.01): liveness, strength and limbo =====

    /// Original Event4C73C7/4C73D7 clear the suspended references while
    /// preserving the suspended mission selector. AreaGuard4C7409..4C7430
    /// installs the first token as destination/post, never as combat Target.
    /// Executed Cell/object/null controls: input_oracle/area_guard.json.
    #[test]
    fn area_guard_clears_suspended_references_and_keeps_the_ordered_post() {
        use crate::sim::components::NavTargetRef;
        use crate::sim::mission::MissionDispatchTimer;
        use crate::sim::mission::state::MissionTestFixture;

        for post in [
            None,
            Some(combat::TargetKind::Cell(20, 20)),
            Some(combat::TargetKind::Entity(2)),
            Some(combat::TargetKind::Entity(999)),
        ] {
            let rules = amcv_move_rules();
            let mut sim = Simulation::new();
            crate::sim::arena_fixture::supply_native_map(&mut sim);
            spawn_rule_backed_unit(&mut sim, 1, "AMCV", &rules);
            spawn_rule_backed_unit(&mut sim, 2, "AMCV", &rules);
            let before_timer = MissionDispatchTimer::from_raw(4, 45);
            let actor = sim.substrate.entities.get_mut(1).unwrap();
            actor.mission.apply_test_fixture(MissionTestFixture {
                current: MissionId::from_known(MissionType::Guard),
                suspended: MissionId::from_known(MissionType::Attack),
                queued: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: before_timer,
            });
            actor
                .suspended_attack_target
                .set_for_test(Some(combat::TargetKind::Entity(2)));
            actor
                .navigation
                .set_suspended_nav_com_for_test(Some(NavTargetRef::cell(12, 12)));
            actor.set_archive_target(Some(combat::TargetKind::Cell(5, 5)));
            actor.mega_mission = crate::sim::mission::mega_mission::MegaMission::attack_move(Some(
                crate::sim::combat::TargetKind::Cell(12, 12),
            ));

            assert!(sim.apply_command(
                "Americans",
                &Command::Guard {
                    entity_id: 1,
                    target: post
                },
                Some(&rules),
            ));
            let actor = sim.substrate.entities.get(1).unwrap();
            let post = post.filter(|post| *post != combat::TargetKind::Entity(999));
            assert_eq!(
                actor.mission.current(),
                MissionId::from_known(MissionType::Guard)
            );
            assert_eq!(
                actor.mission.queued(),
                MissionId::from_known(MissionType::AreaGuard)
            );
            assert_eq!(
                actor.mission.suspended(),
                MissionId::from_known(MissionType::Attack)
            );
            assert_eq!(actor.mission.dispatch_timer(), before_timer);
            assert!(actor.suspended_attack_target.target().is_none());
            assert!(actor.navigation.suspended_nav_com().is_none());
            assert!(actor.attack_target.target().is_none());
            assert!(
                !actor.mega_mission.is_attack_move(),
                "Assign_Target_Command answers Area Guard with Clear_All_TarCom"
            );
            assert_eq!(actor.archive_target(), post);
            assert_eq!(actor.navigation.nav_com(), post.map(Into::into));
        }
    }

    // ===== Duplicate Enter on a building is a no-op (GSI-07.01 C1) =====

    fn bunker_rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=TANK\n1=NOGUN\n\n[InfantryTypes]\n\n[AircraftTypes]\n\n\
             [BuildingTypes]\n0=NATBNK\n\n\
             [TANK]\nStrength=400\nArmor=heavy\nSpeed=6\nTurret=yes\nPrimary=120mm\n\n\
             [NOGUN]\nStrength=400\nArmor=heavy\nSpeed=6\nTurret=yes\n\n\
             [NATBNK]\nStrength=1000\nArmor=heavy\nBunker=yes\n",
        ))
        .expect("bunker rules")
    }

    fn spawn_bunker_struct(sim: &mut Simulation, sid: u64, owner: &str, rx: u16, ry: u16) {
        let owner_id = sim.interner.intern(owner);
        let type_id = sim.interner.intern("NATBNK");
        let mut ge = GameEntity::new_at_frame_zero_for_test(
            sid,
            rx,
            ry,
            0,
            0,
            owner_id,
            Health::new(1000),
            type_id,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        ge.bunker_runtime = Some(crate::sim::docking::bunker_install::BunkerRuntime::idle());
        // Revealed object: order admission reads the limbo byte.
        ge.lifecycle.set_in_limbo_for_test(false);
        sim.substrate.entities.insert(ge);
    }

    fn spawn_bunkerable(
        sim: &mut Simulation,
        sid: u64,
        owner: &str,
        type_name: &str,
        rx: u16,
        ry: u16,
    ) {
        let owner_id = sim.interner.intern(owner);
        let type_id = sim.interner.intern(type_name);
        let mut ge = GameEntity::new_at_frame_zero_for_test(
            sid,
            rx,
            ry,
            0,
            0,
            owner_id,
            Health::new(400),
            type_id,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        // The native Unit setter dispatches through an installed locomotor.
        ge.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Drive,
            ),
        );
        ge.locomotor
            .as_mut()
            .unwrap()
            .ensure_installed_track_state();
        // Revealed object: order admission reads the limbo byte.
        ge.lifecycle.set_in_limbo_for_test(false);
        sim.substrate.entities.insert(ge);
    }

    /// The MEGAMISSION arm tests neither friendship nor the actor's weapon,
    /// and an object token no longer present resolves to NULL
    /// (`0x004C71F6..0x004C749D`): an Attack onto an own unit, an Attack onto
    /// a removed object and an unarmed unit's Cell attack each queue Attack
    /// with the target the setter receives. What_Action refuses them at input.
    #[test]
    fn attack_event_runs_whatever_the_relation_weapon_or_token() {
        let rules = bunker_rules();
        for (command, attacker, target) in [
            (
                Command::Attack {
                    attacker_id: 1,
                    target_id: 2,
                },
                1,
                Some(combat::TargetKind::Entity(2)),
            ),
            (
                Command::Attack {
                    attacker_id: 1,
                    target_id: 999,
                },
                1,
                None,
            ),
            (
                Command::ForceAttackCell {
                    attacker_id: 3,
                    target_rx: 20,
                    target_ry: 20,
                },
                3,
                Some(combat::TargetKind::Cell(20, 20)),
            ),
        ] {
            let mut sim = Simulation::new();
            spawn_bunkerable(&mut sim, 1, "Americans", "TANK", 14, 14);
            spawn_bunkerable(&mut sim, 2, "Americans", "TANK", 16, 14);
            spawn_bunkerable(&mut sim, 3, "Americans", "NOGUN", 18, 14);

            assert!(
                sim.apply_command("Americans", &command, Some(&rules)),
                "{command:?}"
            );
            let actor = sim.substrate.entities.get(attacker).unwrap();
            assert_eq!(
                actor.mission.queued(),
                MissionId::from_known(MissionType::Attack),
                "{command:?}"
            );
            assert_eq!(actor.attack_target.target(), target, "{command:?}");
        }
    }

    /// What_Action's CAN_LOAD query refuses an unarmed unit (no Primary,
    /// so CanAutoDeployHere rejects it) and an enemy bunker. This is input
    /// admission; the MEGAMISSION event does not repeat the query.
    #[test]
    fn bunker_click_query_rejects_an_unarmed_unit_or_an_enemy_bunker() {
        use crate::sim::docking::bunker_install::BunkerState;
        use crate::sim::docking::bunker_link::BunkerLink;
        let rules = bunker_rules();
        for (bunker_owner, unit_type) in [("Americans", "NOGUN"), ("Soviets", "TANK")] {
            let mut sim = Simulation::new();
            spawn_bunker_struct(&mut sim, 2, bunker_owner, 10, 10);
            spawn_bunkerable(&mut sim, 1, "Americans", unit_type, 14, 14);

            assert_ne!(
                crate::sim::radio::receive::can_load_query(&sim, 2, 1, &rules),
                crate::sim::radio::RadioResponse::Roger,
                "{bunker_owner} bunker, {unit_type}"
            );
            assert_eq!(
                sim.substrate.entities.get(1).unwrap().bunker.link(),
                BunkerLink::None
            );
            assert_eq!(
                sim.substrate
                    .entities
                    .get(2)
                    .unwrap()
                    .bunker_runtime
                    .unwrap()
                    .state(),
                BunkerState::Idle,
                "rejected admission leaves the machine idle"
            );
        }
    }

    /// CAN_LOAD refuses a bunker under Construction or being sold
    /// (`0x0043C326..0x0043C346`) without mutating the tank's current seek
    /// or the target's ready latch. This compares the query only; no
    /// replacement event or UI click is executed.
    #[test]
    fn bunker_click_query_preserves_seek_while_target_builds_up_or_is_sold() {
        use crate::sim::docking::bunker_install::BunkerState;
        let rules = bunker_rules();
        for mission in [MissionType::Construction, MissionType::Selling] {
            let mut sim = Simulation::new();
            crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
            spawn_bunker_struct(&mut sim, 2, "Americans", 10, 10);
            spawn_bunker_struct(&mut sim, 3, "Americans", 16, 10);
            spawn_bunkerable(&mut sim, 1, "Americans", "TANK", 13, 14);
            assert!(sim.apply_command(
                "Americans",
                &Command::clicked_mission(1, MissionType::Enter, combat::TargetKind::Entity(3)),
                Some(&rules),
            ));
            sim.substrate
                .entities
                .get_mut(2)
                .unwrap()
                .mission
                .set_current_for_test(MissionId::from_known(mission), 1);

            assert_eq!(
                crate::sim::radio::receive::can_load_query(&sim, 2, 1, &rules),
                crate::sim::radio::RadioResponse::Negatory,
                "{mission:?}"
            );
            let building = sim.substrate.entities.get(2).unwrap();
            assert_eq!(building.building_ready_latch(), 0, "{mission:?}");
            assert_eq!(building.mission.queued(), MissionId::NONE, "{mission:?}");
            assert_eq!(
                building.bunker_runtime.unwrap().state(),
                BunkerState::Idle,
                "{mission:?}"
            );
            assert_eq!(
                sim.substrate
                    .entities
                    .get(1)
                    .unwrap()
                    .radio_contacts
                    .slot(0),
                Some(3),
                "{mission:?} keeps the current seek"
            );
            assert_eq!(
                sim.substrate
                    .entities
                    .get(3)
                    .unwrap()
                    .radio_contacts
                    .slot(0),
                Some(1),
                "{mission:?}"
            );
        }
    }

    #[test]
    fn bunker_full_lifecycle_enter_install_then_eject() {
        use crate::sim::docking::bunker_install::{BunkerState, update_bunker};
        use crate::sim::docking::bunker_link::BunkerLink;
        let rules = bunker_rules();
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let mut sim = Simulation::new();
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        spawn_bunker_struct(&mut sim, 2, "Americans", 10, 10);
        // Keep this component fixture's declared TANK rules, but use the
        // production ART reader's retail 2x2 foundation and LTNK turn rate.
        // This exercises the same-cell case that must slide away from the
        // building's raw display-registration coordinate before installation.
        sim.substrate
            .entities
            .get_mut(2)
            .unwrap()
            .type_facts
            .set_foundation_for_test(
                retail
                    .rules
                    .object("NATBNK")
                    .unwrap()
                    .foundation
                    .clone()
                    .into(),
            );
        // The install's arrival test reads its cell's building list
        // (`0x0047C520`), so the bunker stands on its retail foundation.
        for (rx, ry) in
            crate::sim::occupancy::entity_occupancy_cells(sim.substrate.entities.get(2).unwrap())
        {
            sim.substrate.occupancy.add(
                rx,
                ry,
                2,
                crate::sim::movement::locomotor::MovementLayer::Ground,
                None,
                crate::sim::occupancy::CellListInsertion::AppendBuilding,
            );
        }
        let center = crate::sim::movement::ground_pose::object_get_coords(
            sim.substrate.entities.get(2).unwrap(),
            sim.resolved_terrain.as_ref(),
        );
        // The tank stands in the bunker's cell, where it stops after its
        // approach.
        spawn_bunkerable(&mut sim, 1, "Americans", "TANK", 10, 10);
        let unit = sim.substrate.entities.get_mut(1).unwrap();
        unit.body_facing
            .set_rot(retail.rules.object("LTNK").unwrap().turret_rot);
        unit.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Drive,
            ),
        );
        unit.locomotor
            .as_mut()
            .unwrap()
            .ensure_installed_track_state();
        // Exercise real cell/Logic membership, not the admission-only helper's
        // already-clear Limbo byte (which makes Reveal a no-op).
        unit.lifecycle.set_in_limbo_for_test(true);
        assert!(matches!(
            sim.reveal(1),
            crate::sim::world::RevealOutcome::Revealed { .. }
        ));

        // The supplied arrival retains Enter, object NavCom and contact
        // zero, as in the original bunker_entry history. Run its real
        // PerCell receiver to queue Repair and power off the Drive.
        let unit = sim.substrate.entities.get_mut(1).unwrap();
        unit.mission
            .set_current_for_test(MissionId::from_known(MissionType::Enter), 0);
        unit.navigation
            .set_nav_com_for_test(Some(crate::sim::components::NavTargetRef::object(2)));
        unit.radio_contacts.set_slot(0, 2);
        sim.substrate
            .entities
            .get_mut(2)
            .unwrap()
            .radio_contacts
            .set_slot(0, 1);
        assert!(sim.unit_dock_now(1, &rules, None, FrameEffects::default()));
        // This fixture starts at a completed, stopped arrival. Give the
        // building its first visit before a new Drive Process: a fresh
        // Process with Enter/NavCom but no retained destination re-aims the
        // Drive (4B0971..4B09A6), so it is no longer this stopped boundary.
        update_bunker(&mut sim, &rules, None, 2, FrameEffects::default());
        assert_eq!(
            sim.substrate
                .entities
                .get(2)
                .unwrap()
                .bunker_runtime
                .unwrap()
                .state(),
            BunkerState::ClearWait,
        );
        // Prepared same-cell arrival, then the real production locomotor
        // corridor and installer, in their production order, one frame at a
        // time. No head/position writes or Facing::Snap complete the motion.
        // Object/Techno AI and a whole native match remain outside this test.
        let mut saw_track_step = false;
        let mut moved = 0;
        for _ in 0..600 {
            let movement = sim
                .process_ground_locomotor_stats_for_test(1, Some(&rules), None)
                .expect("flat arena supplies every production movement receiver");
            moved += movement.moved_steps;
            update_bunker(
                &mut sim,
                &rules,
                None,
                2,
                crate::sim::world::FrameEffects::default(),
            );
            let state = sim
                .substrate
                .entities
                .get(2)
                .unwrap()
                .bunker_runtime
                .unwrap()
                .state();
            saw_track_step |= state == BunkerState::TrackStep;
            sim.session
                .set_binary_frame_for_test(sim.session.binary_frame() + 1);
            if state == BunkerState::Occupied {
                break;
            }
        }
        assert!(
            saw_track_step,
            "same-cell installation must not skip Force_Track"
        );
        assert!(
            moved > 0,
            "the production track host must actually place the unit"
        );
        let rt = sim
            .substrate
            .entities
            .get(2)
            .unwrap()
            .bunker_runtime
            .unwrap();
        assert_eq!(rt.state(), BunkerState::Occupied);
        assert_eq!(
            sim.substrate.entities.get(2).unwrap().bunker.occupant(),
            Some(1)
        );
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.bunker.link(), BunkerLink::Installed(2));
        assert!(
            unit.in_logic_vector.is_member(),
            "install preserves LogicVector membership"
        );
        assert!(!unit.lifecycle.in_limbo());
        let parked = crate::sim::movement::ground_pose::object_get_coords(
            unit,
            sim.resolved_terrain.as_ref(),
        );
        assert_eq!(
            parked, center,
            "real Drive Process parks at the foundation centre"
        );
        assert_ne!(
            parked,
            crate::sim::movement::ground_pose::object_location(
                sim.substrate.entities.get(2).unwrap(),
                sim.resolved_terrain.as_ref(),
            )
        );
        assert!(crate::sim::movement::track_head::committed_track_head(unit).is_none());
        assert_eq!(
            sim.bunker_wall_events.iter().filter(|e| e.up).count(),
            1,
            "one walls-up event on install"
        );

        // 3) Eject: Force starts without teleporting, links clear, walls lower.
        sim.bunker_wall_events.clear();
        assert!(sim.apply_command(
            "Americans",
            &Command::EjectBunker { bunker_id: 2 },
            Some(&rules),
        ));
        assert_eq!(
            sim.substrate.entities.get(2).unwrap().bunker.occupant(),
            None
        );
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.bunker.link(), BunkerLink::None);
        assert!(
            unit.in_logic_vector.is_member(),
            "occupant stays active on eject"
        );
        assert!(!unit.lifecycle.in_limbo());
        assert_eq!(
            crate::sim::movement::ground_pose::object_get_coords(
                unit,
                sim.resolved_terrain.as_ref(),
            ),
            parked,
            "Eject starts Force_Track without teleporting"
        );
        assert_eq!(
            unit.locomotor
                .as_ref()
                .and_then(|loco| loco.selected_drive_runtime())
                .and_then(|runtime| runtime.retained())
                .unwrap()
                .track()
                .turn_index(),
            0x47
        );
        assert_eq!(
            unit.mission.queued(),
            MissionId::from_known(MissionType::Move)
        );
        assert_eq!(
            sim.bunker_wall_events.iter().filter(|e| !e.up).count(),
            1,
            "one walls-down event on eject"
        );
        let exit = match unit.navigation.nav_com() {
            Some(crate::sim::components::NavTargetRef::Cell { rx, ry }) => (rx, ry),
            other => panic!("normal bunker release must issue its exit cell: {other:?}"),
        };
        for _ in 0..600 {
            sim.process_ground_locomotor_stats_for_test(1, Some(&rules), None)
                .expect("normal release reaches the production movement corridor");
            sim.session
                .set_binary_frame_for_test(sim.session.binary_frame() + 1);
            let unit = sim.substrate.entities.get(1).unwrap();
            if (unit.position.rx(), unit.position.ry()) == exit
                && crate::sim::movement::track_head::committed_track_head(unit).is_none()
                && crate::sim::movement::motion_query::is_moving(&unit) != Some(true)
            {
                break;
            }
        }
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!((unit.position.rx(), unit.position.ry()), exit);
        assert!(crate::sim::movement::track_head::committed_track_head(unit).is_none());
        assert!(crate::sim::movement::motion_query::is_moving(&unit) != Some(true));
        assert_eq!(unit.bunker.link(), BunkerLink::None);
        assert_eq!(
            sim.substrate
                .entities
                .get(2)
                .unwrap()
                .bunker_runtime
                .unwrap()
                .state(),
            BunkerState::Idle
        );
    }
}
