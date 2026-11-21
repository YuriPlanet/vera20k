//! Native command record encoding and decoding for the Simulation: the `encode_*_record`
//! builders, the sender/House lookups they stamp into the header byte, and
//! `decode_native_command_record`, which turns an admitted record back into a typed command.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{
    ArchiveRecord, BUILDING_OFF_LINE_OPCODE, BUILDING_ON_LINE_OPCODE, BuildingOnLineRecord,
    COMMAND_RECORD_LEN, Command, CommandEnvelope, CommandRecord, CycleProducerRecord, DeployRecord,
    ExitRecord, GameSpeedRecord, MegaMissionOrder, MegaMissionRecord, MegaMissionTarget,
    ObjectAction, ObjectActionRecord, PaidSuperAction, PaidSuperRecord, PlaceRecord,
    ProductionEvent, ProductionRecord, RepairRecord, ScatterRecord, SellWallAtCellRecord,
    SpecialPlaceRecord, StopRecord,
};
use crate::sim::control_authority::CommandCapability;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionType;
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation};
use crate::sim::{combat, production};
use std::borrow::Cow;

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
        effects: &mut FrameEffects<'_>,
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
                sound_id: rules.sound_name(sound_id),
            });
            effects.flush_sound(self, rules);
        }
    }

    /// 730F5C..730F6C -> shared EventCtor4C66C0. The payload is unused.
    /// Stock writes the House index. Scoped matches retain the issuing seat.
    pub fn encode_cheer_record(
        &self,
        owner: crate::sim::intern::InternedId,
    ) -> Option<CommandRecord> {
        let source = if self.session.has_custom_control_grants() {
            self.command_sender(owner)?
        } else {
            i8::try_from(self.houses.index_of(owner)?).ok()?
        };
        self.encode_source_header_event(source, crate::sim::command::CHEER_OPCODE)
    }

    /// ScatterSelectedObjects730FE0 admits Techno vt+A0, without Guard's
    /// IsActive test or miner exception. All retained entities are Technos.
    pub fn scatter_key_command(&self, id: u64, rules: &RuleSet) -> Option<Command> {
        self.local_techno_controllable(id, rules)
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
        let house_id = self.command_sender(command_owner)?;
        let typed = ScatterRecord {
            house_id,
            frame: self.session.binary_frame() as i32,
            entity_id: i32::try_from(entity_id).ok()?,
        };
        let mut record = CommandRecord::decode_exact(&[0; COMMAND_RECORD_LEN]).ok()?;
        typed.write_into(&mut record);
        Some(record)
    }

    /// `FootClass::ClickedAction_Object`'s Detonate (0x37) arm
    /// (`0x004D75F4..0x004D766F`): the DETONATE event on `carrier`, whose
    /// house byte is its bomb's house (`bomb+0x28`, its ArrayIndex). `None`
    /// without a bomb. Delegated control carries the issuing source.
    pub fn encode_detonate_record(&self, carrier: u64) -> Option<CommandRecord> {
        let house = self.substrate.entities.get(carrier)?.bomb.as_ref()?.house();
        let typed = crate::sim::command::DetonateRecord {
            house_id: self.subject_command_sender(house, CommandCapability::Tactical)?,
            frame: self.session.binary_frame() as i32,
            entity_id: i32::try_from(carrier).ok()?,
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

    /// TargetEvent4C65E0 is used by SELL16 (Execute4C6F12) and
    /// PRIMARY11 (Execute4C718C). Eject is the existing VERA bunker action,
    /// carried explicitly rather than claiming a native self-Unload mapping.
    pub fn encode_object_action_record(
        &self,
        owner: InternedId,
        command: &Command,
    ) -> Option<CommandRecord> {
        let (entity, action) = match *command {
            Command::SellBuilding { entity_id } => (entity_id, ObjectAction::Sell),
            Command::TogglePrimaryFactory { entity_id } => (entity_id, ObjectAction::Primary),
            Command::EjectBunker { bunker_id } => (bunker_id, ObjectAction::EjectBunker),
            _ => return None,
        };
        let mut record = self.encode_header_event(owner, action.opcode())?;
        ObjectActionRecord {
            house_id: record.house_id(),
            frame: record.frame_stamp(),
            entity_id: i32::try_from(entity).ok()?,
            action,
        }
        .write_into(&mut record);
        Some(record)
    }

    /// Event0D4C794B retains its dword GameSpeed payload for the shared receiver.
    pub fn encode_game_speed_record(&self, owner: InternedId, speed: u8) -> Option<CommandRecord> {
        let header = self.encode_header_event(owner, crate::sim::command::GAME_SPEED_OPCODE)?;
        GameSpeedRecord {
            house_id: header.house_id(),
            frame: header.frame_stamp(),
            speed: i32::from(speed),
        }
        .encode()
        .ok()
    }

    /// VERA sidebar cycles are ordered commands, because the selected primary
    /// affects delivery. Keep the next-candidate decision in production's
    /// existing cycle_active_producer_for_owner_category execution owner.
    pub fn encode_cycle_producer_record(
        &self,
        owner: InternedId,
        category: production::ProductionCategory,
    ) -> Option<CommandRecord> {
        let header =
            self.encode_header_event(owner, crate::sim::command::VERA_CYCLE_PRODUCER_OPCODE)?;
        CycleProducerRecord {
            house_id: header.house_id(),
            frame: header.frame_stamp(),
            category,
        }
        .encode()
        .ok()
    }

    /// SPECIAL_PLACE4C6B60 carries the rules-owned registry index, not an
    /// InternedId. Paid selections use one VERA record so their existing
    /// saved sequence guard and cancellation execute identically on all peers.
    pub fn encode_superweapon_record(
        &self,
        owner: InternedId,
        command: &Command,
        rules: &RuleSet,
    ) -> Option<CommandRecord> {
        let header = self.encode_header_event(owner, SpecialPlaceRecord::OPCODE)?;
        let index = |id| i32::try_from(rules.super_weapon_index(self.interner.resolve(id))?).ok();
        let (selection, action, cell) = match *command {
            Command::LaunchSuperWeapon {
                sw_type_id,
                target_rx,
                target_ry,
            } => {
                if target_rx >= self.cell_grid().side() || target_ry >= self.cell_grid().side() {
                    return None;
                }
                return SpecialPlaceRecord {
                    house_id: header.house_id(),
                    frame: header.frame_stamp(),
                    type_index: index(sw_type_id)?,
                    cell: (
                        i16::try_from(target_rx).ok()?,
                        i16::try_from(target_ry).ok()?,
                    ),
                }
                .encode()
                .ok();
            }
            Command::BeginPaidSuperWeapon {
                selection,
                target_rx,
                target_ry,
            } => (selection, PaidSuperAction::Begin, (target_rx, target_ry)),
            Command::CompletePaidSuperWeapon {
                selection,
                target_rx,
                target_ry,
            } => (selection, PaidSuperAction::Complete, (target_rx, target_ry)),
            Command::CancelPaidSuperWeapon { selection } => {
                (selection, PaidSuperAction::Cancel, (0, 0))
            }
            _ => return None,
        };
        // As for SpecialPlace above, a target lies in this map's cell array.
        let side = self.cell_grid().side();
        if action != PaidSuperAction::Cancel && (cell.0 >= side || cell.1 >= side) {
            return None;
        }
        PaidSuperRecord {
            house_id: header.house_id(),
            frame: header.frame_stamp(),
            action,
            source: index(selection.source)?,
            destination: index(selection.destination)?,
            sequence: selection.sequence,
            cell: (i16::try_from(cell.0).ok()?, i16::try_from(cell.1).ok()?),
        }
        .encode()
    }

    pub(in crate::sim::world) fn superweapon_command_from_record(
        &self,
        record: &CommandRecord,
        rules: &RuleSet,
    ) -> Option<(InternedId, Command)> {
        let (owner, record) = self.house_command_record(record)?;
        let record = record.as_ref();
        let type_id = |index| {
            self.interner
                .get(rules.super_weapon_order.get(usize::try_from(index).ok()?)?)
        };
        if let Some(event) = SpecialPlaceRecord::decode(record) {
            if !(0..self.cell_grid().side() as i16).contains(&event.cell.0)
                || !(0..self.cell_grid().side() as i16).contains(&event.cell.1)
            {
                return None;
            }
            return Some((
                owner,
                Command::LaunchSuperWeapon {
                    sw_type_id: type_id(event.type_index)?,
                    target_rx: u16::try_from(event.cell.0).ok()?,
                    target_ry: u16::try_from(event.cell.1).ok()?,
                },
            ));
        }
        let event = PaidSuperRecord::decode(record)?;
        let side = self.cell_grid().side() as i16;
        if event.action != PaidSuperAction::Cancel
            && (!(0..side).contains(&event.cell.0) || !(0..side).contains(&event.cell.1))
        {
            return None;
        }
        let selection = crate::sim::superweapon::PaidSuperWeaponSelection {
            source: type_id(event.source)?,
            destination: type_id(event.destination)?,
            sequence: event.sequence,
        };
        Some((
            owner,
            match event.action {
                PaidSuperAction::Begin => Command::BeginPaidSuperWeapon {
                    selection,
                    target_rx: event.cell.0 as u16,
                    target_ry: event.cell.1 as u16,
                },
                PaidSuperAction::Complete => Command::CompletePaidSuperWeapon {
                    selection,
                    target_rx: event.cell.0 as u16,
                    target_ry: event.cell.1 as u16,
                },
                PaidSuperAction::Cancel => Command::CancelPaidSuperWeapon { selection },
            },
        ))
    }

    /// SetRallyPoint4439C5 and object-click44367B ask the actor's vt+38
    /// (Techno6F9DB0), not PlayerPtr. Preserve that House and both tokens
    /// at issue; send-time retiming remains the packet codec's responsibility.
    /// Delegated control carries the issuing source with the same tokens.
    /// Source: original gamemd.exe constructor4C6780.
    pub(in crate::sim::world) fn encode_archive_record(
        &self,
        entity_id: u64,
        target: MegaMissionTarget,
    ) -> Option<CommandRecord> {
        let actor = self.substrate.entities.get(entity_id)?;
        let sender = self.subject_command_sender(actor.owner(), CommandCapability::Tactical)?;
        let mut record =
            self.encode_source_header_event(sender, crate::sim::command::ARCHIVE_OPCODE)?;
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
        self.encode_selected_production_record(command_owner, event, selection)
    }

    /// Typed callers retain Select6AAD00's existing RTTI/index/Naval fields.
    /// The name-only encoder above deliberately keeps broad-name semantics.
    pub fn encode_selected_production_record(
        &self,
        command_owner: InternedId,
        event: ProductionEvent,
        selection: production::ProductionSelection,
    ) -> Option<CommandRecord> {
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
        let object = self.interner.object_type_in_category(
            held.type_id,
            category.object_category(),
            rules,
        )?;
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
        let control = self.control_projection();
        let player_can_order_actor = control
            .local_command_house()
            .is_some_and(|player| control.can_order_from(actor.owner(), player));
        // Ares 3.0 GuardCommandClass_Execute (1005CAF0, hook 0x730DB0): only
        // the player's own controllable actor, the miner arm included.
        if rules.ares() && (!player_can_order_actor || !self.local_techno_controllable(id, rules)) {
            return None;
        }
        if actor.category() == EntityCategory::Structure {
            return None;
        }
        let ordinary =
            self.local_techno_controllable(id, rules) && self.local_techno_is_active(id, rules);
        let miner = actor.category() == EntityCategory::Unit
            && (actor.slave_manager.is_some() || actor.is_harvester());
        if !ordinary && !miner {
            return None;
        }
        // An actor with no navigation cell (a mod type built without a
        // locomotor) takes no Guard order, as its Move order is refused.
        let (x, y) = self
            .object_navigation_cell(id, rules)
            .map_err(|cause| log::warn!("Area Guard input refused for {id}: {cause}"))
            .ok()?;
        let command = Command::Guard {
            entity_id: id,
            target: Some(combat::TargetKind::Cell(x as u16, y as u16)),
        };
        self.can_issue_local_command(&command, rules)
            .then_some(command)
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
                // The cell words go bit for bit, like the Cell tokens above:
                // the decoder reads them back as the same unsigned cell.
                MegaMissionOrder::Move {
                    target_x: target_rx as i16,
                    target_y: target_ry as i16,
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
        let house_id = self.command_sender(command_owner)?;
        let typed = MegaMissionRecord {
            auxiliary: MegaMissionTarget::Null,
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
        let house_id = self.command_sender(command_owner)?;
        SellWallAtCellRecord {
            house_id,
            frame: self.session.binary_frame(),
            x,
            y,
        }
        .encode()
        .ok()
    }

    /// Ares 3.0 RaiseTrenchRedirectClick (`1006A170`, from the Enter click
    /// of `10016F60`, hook `0x443414`): the TrenchRedirectClick event (0x60)
    /// naming the clicked building's `cell` and the selected trench
    /// `source_id`, stamped with the source building's House.
    pub fn encode_ares_trench_redirect_record(
        &self,
        source_id: u64,
        (x, y): (u16, u16),
    ) -> Option<CommandRecord> {
        let owner = self.substrate.entities.get(source_id)?.owner();
        let house_id = self.subject_command_sender(owner, CommandCapability::Tactical)?;
        crate::sim::command::AresTrenchRedirectRecord::encode_cell(
            house_id,
            self.session.binary_frame() as i32,
            (i16::try_from(x).ok()?, i16::try_from(y).ok()?),
            i32::try_from(source_id).ok()?,
        )
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
    /// dword payload the removed sender (`0x004C6720`), its House's index
    /// unless players share Houses.
    pub fn encode_remove_player_record(
        &self,
        command_owner: crate::sim::intern::InternedId,
        source: i32,
    ) -> Option<CommandRecord> {
        self.encode_int_event(
            command_owner,
            crate::sim::command::REMOVE_PLAYER_OPCODE,
            source,
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
        self.encode_source_header_event(self.command_sender(command_owner)?, opcode)
    }

    fn encode_source_header_event(&self, source: i8, opcode: u8) -> Option<CommandRecord> {
        CommandRecord::encode(
            opcode,
            i32::from(source),
            self.session.binary_frame() as i32,
            &[],
        )
        .ok()
    }

    /// For stock ARCHIVE and DETONATE the subject contributes the header.
    /// A match with explicit grants binds that header to the local seat.
    fn subject_command_sender(
        &self,
        subject: InternedId,
        capability: CommandCapability,
    ) -> Option<i8> {
        if !self.session.has_custom_control_grants() {
            return self.command_sender(subject);
        }
        let (source, _) = self.local_grant_source()?;
        self.control_scope_for_source(source, subject)
            .permits(capability)
            .then_some(source)
    }

    /// The sender byte this process writes for `command_owner`'s commands:
    /// its own network source for the House it shares (VERA shared
    /// control), otherwise the House's index.
    pub(crate) fn command_sender(&self, command_owner: InternedId) -> Option<i8> {
        if self.session.has_custom_control_grants() {
            let (source, primary) = self.local_grant_source()?;
            return (command_owner == primary).then_some(source);
        }
        if let Some(source) = self.session.local_shared_house_source()
            && self.control_projection().local_command_house() == Some(command_owner)
        {
            return Some(source);
        }
        self.houses
            .index_of(command_owner)
            .and_then(|index| i8::try_from(index).ok())
    }

    /// The registered House a command record's sender byte names, the one
    /// sender admission for every record. VERA shared control: the sender
    /// is a network source, which is its House index unless players share
    /// Houses (`ScenarioSession::network_source_house`).
    pub(crate) fn command_sender_owner(&self, sender: i8) -> Option<InternedId> {
        let index = self.session.network_source_house(sender)?;
        self.houses.order().get(index).copied()
    }

    /// Address an economic event to a specific House while retaining the
    /// authentic local source. Ordinary sidebar encoders retain their native
    /// records and continue to issue against the source's primary House.
    pub fn encode_target_house_command(
        &self,
        target: InternedId,
        native: &CommandRecord,
    ) -> Option<CommandRecord> {
        let (sender, primary) = self.local_grant_source()?;
        if !self
            .control_scope_for_source(sender, target)
            .permits(CommandCapability::FullControl)
        {
            return None;
        }
        let target_slot = u8::try_from(self.houses.index_of(target)?).ok()?;
        let mut inner = native.clone();
        inner.set_house_id(sender);
        let targeted = crate::sim::command::TargetHouseRecord::encode(target_slot, &inner)?;
        if primary == target {
            Some(inner)
        } else {
            Some(targeted)
        }
    }

    /// Resolve the House that owns a native production, placement or power
    /// operation. An explicit target is authorized with the original source
    /// before its nested native payload is used. Dispatch separately checks
    /// that this record carries its admitted queue source.
    pub(in crate::sim::world) fn house_command_record<'a>(
        &self,
        record: &'a CommandRecord,
    ) -> Option<(InternedId, Cow<'a, CommandRecord>)> {
        if record.opcode() != crate::sim::command::TARGET_HOUSE_OPCODE {
            let owner = self.command_sender_owner(record.house_id())?;
            return Some((owner, Cow::Borrowed(record)));
        }
        let requested = crate::sim::command::TargetHouseRecord::decode(record)?;
        let owner = *self
            .houses
            .order()
            .get(usize::from(requested.target_house))?;
        let source = record.house_id();
        self.admitted_grant_source_owner(source)?;
        self.control_scope_for_source(source, owner)
            .permits(CommandCapability::FullControl)
            .then_some((owner, Cow::Owned(requested.command)))
    }

    /// VERA shared control: whether House `owner` still has an admitted
    /// sender other than `sender`. A House nobody shares has one sender, so
    /// its EXIT, DESTRUCT or loss always ends it, as in gamemd.
    pub(super) fn house_keeps_other_sender(&self, owner: InternedId, sender: i8) -> bool {
        let (Some(timing), Some(house)) = (
            self.session.network_command_timing(),
            self.houses.index_of(owner),
        ) else {
            return false;
        };
        self.session
            .network_sources(self.houses.order().len())
            .any(|(other, other_house)| {
                other_house == house && other != sender && timing.admits_source(other)
            })
    }

    /// The sender of the executing command: its record's header byte, or a
    /// semantic envelope's House index (offline, where nobody shares).
    pub(super) fn executing_sender(&self, sender: Option<i8>, owner: InternedId) -> Option<i8> {
        match (self.session.execution_topology().is_lockstep(), sender) {
            (true, Some(source)) => Some(source),
            (true, None) if self.session.shares_houses() => None,
            (false, Some(_) | None) | (true, None) => self
                .houses
                .index_of(owner)
                .and_then(|index| i8::try_from(index).ok()),
        }
    }

    /// VERA shared control: whether another player still controls the local
    /// player's House.
    pub fn local_house_has_teammate(&self) -> bool {
        match (
            self.session.local_shared_house_source(),
            self.session.current_house(),
        ) {
            (Some(sender), Some(owner)) => self.house_keeps_other_sender(owner, sender),
            _ => false,
        }
    }

    /// Decode one synchronized record after its queue has admitted the stamped
    /// frame. The semantic envelope is only a typed execution view of the raw
    /// bytes; timing/processed-bit ownership remains with the raw queue.
    /// Every typed view below reads its House from this same header byte.
    pub fn decode_native_command_record(
        &self,
        record: &CommandRecord,
        execute_tick: u64,
    ) -> Option<CommandEnvelope> {
        let owner = self.command_sender_owner(record.house_id())?;
        let envelope = |command| Some(CommandEnvelope::new(owner, execute_tick, command));
        let raw = || {
            envelope(Command::SynchronizedRecord {
                record: record.clone(),
            })
        };
        if let Some(event) = RepairRecord::decode(record) {
            let entity_id = u64::try_from(event.entity_id).ok()?;
            return envelope(Command::ToggleRepair { entity_id });
        }
        if let Some(event) = ArchiveRecord::decode(record) {
            // Preserve late object lookup and NULL. These semantic views do
            // not replace the native record held by queues/replay/snapshots.
            let entity_id = u64::try_from(event.entity_id).ok()?;
            return match event.target {
                // The Cell words are carried as the MegaMission arm's below.
                MegaMissionTarget::Cell { x, y } => envelope(Command::SetRally {
                    rx: x as u16,
                    ry: y as u16,
                    producer_ids: vec![entity_id],
                }),
                MegaMissionTarget::Object { id } if id >= 0 => envelope(Command::ArchiveObject {
                    entity_id,
                    target_id: id as u64,
                }),
                _ => raw(),
            };
        }
        if let Some(event) = ObjectActionRecord::decode(record) {
            let entity_id = u64::try_from(event.entity_id).ok()?;
            return envelope(match event.action {
                ObjectAction::Sell => Command::SellBuilding { entity_id },
                ObjectAction::Primary => Command::TogglePrimaryFactory { entity_id },
                ObjectAction::EjectBunker => Command::EjectBunker {
                    bunker_id: entity_id,
                },
            });
        }
        if let Some(event) = CycleProducerRecord::decode(record) {
            return envelope(Command::CycleProducerFocus {
                category: event.category,
            });
        }
        // Ares 3.0 TrenchRedirectClick (0x60) stays raw until its handler,
        // which answers on the switch; no stock opcode decodes as 0x60, and
        // only a game Ares plays in encodes one.
        if crate::sim::command::AresTrenchRedirectRecord::decode(record).is_some() {
            return raw();
        }
        // DETONATE stays raw until its handler, which resolves the carrier
        // late, as Execute does.
        if crate::sim::command::DetonateRecord::decode(record).is_some() {
            return raw();
        }
        if ProductionRecord::decode(record).is_some()
            || PlaceRecord::decode(record).is_some()
            || SpecialPlaceRecord::decode(record).is_some()
            || PaidSuperRecord::decode(record).is_some()
            || record.opcode() == crate::sim::command::TARGET_HOUSE_OPCODE
        {
            // Keep selectors raw: a category-only semantic view loses Naval,
            // RTTI aliases and the -1 wildcard before the House receiver runs.
            return raw();
        }
        if let Some(typed) = DeployRecord::decode(record) {
            let entity_id = u64::try_from(typed.entity_id).ok()?;
            self.substrate.entities.get(entity_id)?;
            return envelope(Command::DeployMcv { entity_id });
        }
        if record.opcode() == crate::sim::command::OPTIONS_OPCODE {
            return envelope(Command::OpenOptions);
        }
        if record.opcode() == crate::sim::command::LATENCY_FUDGE_OPCODE {
            return envelope(Command::SetLatencyFudge {
                value: i32::from_le_bytes(record.payload_field::<0, 4>()),
            });
        }
        // PlanMgr637E00/638120 owns the payload and validates live actors.
        // Keep its native byte record intact through the scheduled envelope.
        if matches!(record.opcode(), 0x2A..=0x2C)
            || matches!(MegaMissionRecord::planning_flag(record), Some(1 | 2))
        {
            return envelope(Command::PlanningRecord {
                record: record.clone(),
            });
        }
        if let Some(typed) = StopRecord::decode(record) {
            let entity_id = u64::try_from(typed.entity_id).ok()?;
            self.substrate.entities.get(entity_id)?;
            return envelope(Command::Stop { entity_id });
        }
        if let Some(typed) = ScatterRecord::decode(record) {
            let entity_id = u64::try_from(typed.entity_id).ok()?;
            self.substrate.entities.get(entity_id)?;
            return envelope(Command::Scatter { entity_id });
        }
        if let Some(typed) = MegaMissionRecord::decode(record) {
            let entity_id = u64::try_from(typed.source_id).ok()?;
            self.substrate.entities.get(entity_id)?;
            // Keep NavComAux in the native carrier until execution, where
            // the shared MegaMission receiver applies it after the setters.
            if typed.auxiliary != MegaMissionTarget::Null {
                return raw();
            }
            // A Cell word is the native signed short, carried bit for bit
            // in the u16 (-1 is 0xFFFF): every consumer reads it back as i16
            // (`TargetKind::cell_identity`), and a cell off the map, which a
            // peer may name, resolves to the shared fallback CellClass as
            // `MapClass::Get_CellClass` does (`Self::resolve_event_token`).
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
                    target_rx: target_x as u16,
                    target_ry: target_y as u16,
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
            return envelope(command);
        }

        // Event2D4C6E2E resolves its House and calls50C8C0 without input
        //lock, cooldown, selection or product admission being repeated.
        if record.opcode() == crate::sim::command::CHEER_OPCODE {
            return envelope(Command::Cheer);
        }

        if ExitRecord::decode(record).is_some() {
            return envelope(Command::ExitMatch);
        }

        let house_index_event: Option<fn(i32) -> Command> = match record.opcode() {
            crate::sim::command::REMOVE_PLAYER_OPCODE => {
                Some(|source| Command::RemovePlayer { source })
            }
            crate::sim::command::ALLY_OPCODE => Some(|house_index| Command::Ally { house_index }),
            _ => None,
        };
        if let Some(command) = house_index_event {
            let other = i32::from_le_bytes(record.payload_field::<0, 4>());
            return envelope(command(other));
        }

        if record.opcode() == crate::sim::command::DESTRUCT_OPCODE {
            return envelope(Command::Destruct);
        }

        // Native opcode 0x0D (EventClass::Execute type-13 @ 0x004C794B): the
        // payload dword at Event+7 is the new GameSpeed setting; the changer is
        // the header house. It is SetGameSpeed's live-network twin.
        if let Some(typed) = GameSpeedRecord::decode(record) {
            let speed = u8::try_from(typed.speed).ok()?;
            return envelope(Command::LiveGameSpeed { speed });
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
            let typed = BuildingOnLineRecord::decode(record)?;
            let entity_id = u64::try_from(typed.entity_id).ok()?;
            self.substrate.entities.get(entity_id)?;
            return envelope(Command::SetBuildingOnLine {
                entity_id,
                on: typed.on,
            });
        }

        let typed = SellWallAtCellRecord::decode(record)?;
        envelope(Command::SellWallAtCell {
            x: typed.x,
            y: typed.y,
        })
    }
}
