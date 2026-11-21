//! Individual command payload behavior for the Simulation.
//!
//! Contains `apply_command()` and its helper methods: production, superweapon
//! and archive records, order-event tokens and the MEGAMISSION event arm.
//! `command_schedule` owns queue admission and scheduled batch order.
//!
//! Parts: `records` (native record encode/decode), `wall_sale` (selling a wall
//! at a cell), `selection` (rally points and selection), `admission` (ownership
//! checks and command subjects).
//!
//! Dependency rules: same as sim/ (depends on rules/, map/; never render/ui/audio/net).

use crate::sim::world::FrameEffects;

use super::Simulation;
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat;
#[cfg(test)]
use crate::sim::command::COMMAND_RECORD_LEN;
#[cfg(test)]
use crate::sim::command::CommandEnvelope;
#[cfg(test)]
use crate::sim::command::DeployRecord;
#[cfg(test)]
use crate::sim::command::StopRecord;
use crate::sim::command::{
    ArchiveRecord, AresTrenchRedirectRecord, Command, CommandRecord, CycleProducerRecord,
    MEGAMISSION_AUXILIARY_KIND_OFFSET, MEGAMISSION_PLANNING_OFFSET, MegaMissionRecord,
    MegaMissionTarget, PlaceRecord, ProductionEvent, ProductionRecord,
};
use crate::sim::control_authority::CommandCapability;
use crate::sim::house_diplomacy::HouseAlliances;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::mission::MissionType;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::pathfinding::PathGrid;
use crate::sim::production;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::vision::FogState;
use crate::sim::world::ObjectSubstrate;

mod admission;
mod records;
mod selection;
mod wall_sale;

/// A command's acting subject. Markers and destinations do not confer control.
enum CommandSubject<'a> {
    Refused,
    Local,
    Primary(CommandCapability),
    House(InternedId, CommandCapability),
    TargetHouse(Option<InternedId>),
    Actor(u64, CommandCapability),
    Actors(&'a [u64], CommandCapability),
    Bunker(u64),
    Trench(u64, AresTrenchRedirectRecord),
}

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

/// `ObjectClass+0x83`, IsSelected: the object is in the player's current
/// selection. It lives with its owners: `ObjectClass::Select @ 0x005F4520`
/// sets it ([`Simulation::techno_select`], [`Simulation::try_select_object`]),
/// `ObjectClass::Deselect @ 0x005F44A0` clears it
/// ([`Simulation::object_deselect`]), and `Command::Select` commits a
/// snapshot through `apply_selection_snapshot`. VERA keeps the
/// current-objects vector as the app's selection ledger (input order and
/// pending commands), which it reconciles with this byte after the frame.
/// Combat reads it for selected-unit effects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Selection {
    selected: bool,
}

impl Selection {
    /// The object is selected.
    pub const fn is_selected(self) -> bool {
        self.selected
    }

    /// `ObjectClass::Select`'s write. The MCV deploy and undeploy
    /// reselections (`world_spawn`) still write it directly.
    pub(in crate::sim) fn select(&mut self) {
        self.selected = true;
    }

    /// `ObjectClass::Deselect`'s write.
    fn deselect(&mut self) {
        self.selected = false;
    }

    /// Install a fixture's byte.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_selected_for_test(&mut self, selected: bool) {
        self.selected = selected;
    }
}

impl Simulation {
    /// Execute4C70E1..4C718A hands the packet selector to the existing House
    /// production owners. Keep it intact until this call; neither the issuing
    /// client's type interner nor its then-current Factory head is authority.
    fn apply_production_record(
        &mut self,
        record: &CommandRecord,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
    ) -> bool {
        let Some((owner, record)) = self.house_command_record(record) else {
            return false;
        };
        let record = record.as_ref();
        let owner = self.interner.resolve(owner).to_owned();
        if let Some(event) = CycleProducerRecord::decode(record) {
            return production::cycle_active_producer_for_owner_category(
                self,
                rules,
                &owner,
                event.category,
            );
        }
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
            None,
            command_owner,
            cmd,
            rules.unwrap_or(crate::rules::ruleset::RuleSet::empty_for_test()),
            None,
            &mut crate::sim::world::FrameEffects::default(),
        )
    }

    pub(crate) fn apply_command_with_overlays(
        &mut self,
        sender: Option<i8>,
        command_owner: &str,
        cmd: &Command,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) -> bool {
        // A semantic envelope cannot promote an embedded record into an
        // authenticated source. DoList supplies this byte from its raw entry.
        let has_grants = self.session.has_custom_control_grants();
        if has_grants
            && let Command::SynchronizedRecord { record } | Command::PlanningRecord { record } = cmd
            && sender != Some(record.house_id())
        {
            return false;
        }
        // Event command receivers clear Plan tokens before applying ordinary
        // orders. Flag2 acknowledges its path without detaching it; use the
        // same payload dispatcher while keeping state installed for callbacks.
        let decoded_native;
        let mut auxiliary = MegaMissionTarget::Null;
        let planned = matches!(cmd, Command::PlanningRecord { record }
            if super::planning::planning_dispatch_record(record));
        let native_mission = match cmd {
            Command::PlanningRecord { record } if planned => Some(record),
            Command::SynchronizedRecord { record }
                if MegaMissionRecord::planning_flag(record) == Some(0) =>
            {
                Some(record)
            }
            _ => None,
        };
        let cmd = if let Some(record) = native_mission {
            let Some((typed, _)) = MegaMissionRecord::decode_planned(record) else {
                return false;
            };
            auxiliary = typed.auxiliary;
            // Only the temporary semantic projection omits the planning flag
            // and NavComAux. The queue, plan graph and saved/replayed bytes
            // keep the full record.
            let mut ordinary = record.clone();
            ordinary.payload_mut()[MEGAMISSION_PLANNING_OFFSET] = 0;
            ordinary.payload_mut()[MEGAMISSION_AUXILIARY_KIND_OFFSET] = 0;
            let Some(envelope) = self.decode_native_command_record(&ordinary, self.session.tick())
            else {
                return false;
            };
            if planned {
                if has_grants {
                    // The generated record retires its own in-flight latch
                    // even when its source exited before this execution.
                    // The live-source gate below still refuses its effects.
                    let issuing_house = self.command_sender_owner(record.house_id());
                    if issuing_house != self.interner.get(command_owner)
                        || !u64::try_from(typed.source_id)
                            .is_ok_and(|actor| self.planning_acknowledge_generated(actor, record))
                    {
                        return false;
                    }
                } else if let Ok(actor) = u64::try_from(typed.source_id) {
                    self.planning_acknowledge(actor);
                }
            }
            decoded_native = envelope.payload;
            &decoded_native
        } else {
            cmd
        };
        let granted_source =
            sender.and_then(|source| self.resolve_command_grant_source(command_owner, source));
        if (has_grants && sender.is_some() && granted_source.is_none())
            || self.network_command_actor_foreign(command_owner, granted_source, cmd, rules, None)
        {
            return false;
        }
        let ordinary_actor = match cmd {
            Command::Stop { entity_id }
            | Command::Scatter { entity_id }
            | Command::DeployMcv { entity_id }
            | Command::ToggleRepair { entity_id }
            // EventClass::Execute @ 0x004C6CB0, SELL arm 0x004C6F20:
            // PlanningDetach precedes the target's alive/owner refusal.
            | Command::SellBuilding { entity_id } => Some(*entity_id),
            // The VERA-only bunker-eject record is Event9 DEPLOY on the
            // bunker's occupant: 0x004C76CA detaches that Unit's planning.
            Command::EjectBunker { bunker_id } => self
                .substrate
                .entities
                .get(*bunker_id)
                .and_then(|bunker| bunker.bunker.occupant()),
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
                let for_this_client = if has_grants {
                    self.local_grant_source()
                        .is_some_and(|(source, _)| sender == Some(source))
                } else {
                    self.interner.get(command_owner)
                        == self.control_projection().local_command_house()
                };
                if !for_this_client {
                    return false;
                }
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
                    auxiliary,
                    rules,
                    frame_effects,
                )
                .accepted(),
            Command::PlanningRecord { record } => {
                self.apply_planning_record(sender, command_owner, record, rules)
            }
            Command::SynchronizedRecord { record } => {
                if let Some((owner, command)) = self.superweapon_command_from_record(record, rules)
                {
                    return self.apply_superweapon_command_for_house(
                        owner,
                        &command,
                        rules,
                        overlay_registry,
                        frame_effects,
                    );
                }
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
                    return self.execute_archive_event(entity_id, target, rules.ares());
                }
                if let Some(event) = crate::sim::command::DetonateRecord::decode(record) {
                    let Ok(carrier) = u64::try_from(event.entity_id) else {
                        return false;
                    };
                    return self.execute_detonate_event(
                        carrier,
                        rules,
                        overlay_registry,
                        frame_effects,
                    );
                }
                if rules.ares()
                    && let Some(event) =
                        crate::sim::command::AresTrenchRedirectRecord::decode(record)
                {
                    return self.ares_respond_to_trench_redirect(
                        granted_source.map(|(source, _)| source),
                        &event,
                        rules,
                        overlay_registry,
                        frame_effects,
                    );
                }
                self.apply_production_record(record, rules, overlay_registry, frame_effects)
            }
            Command::AssignControlGroup { group, entity_ids } => {
                self.assign_control_group(*group, entity_ids)
            }
            Command::Select { entity_ids, .. } => self.apply_selection_snapshot(
                entity_ids,
                super::UninitContext::new(rules).with_effects(frame_effects),
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
                            && actor.seldom.tether.partner().is_none()
                    });
                if !admitted {
                    return false;
                }
                // Native also writes +6B2. Its observed consumers are class
                // CRC4DBD60 and raw native Save/Load41035E/4103CD, not active
                // peer checksum64DAB0. No unread Rust bookkeeping shadow is
                // retained; class CRC/raw-native-image equivalence is absent.
                //
                // The record may come from another player and name any Foot,
                // so a Scatter body error (a class state or map input VERA's
                // receiver needs and this actor lacks) refuses the order
                // instead of stopping the game, as the damage, idle and
                // cell callers do. The error is a function of the shared
                // simulation state, so every peer refuses the same record
                // after the same effects (an RNG draw or queued Move the body
                // made before it failed).
                if let Err(cause) = self.scatter(
                    *entity_id,
                    None,
                    crate::sim::movement::ScatterFlags::new(true, false),
                    rules,
                    overlay_registry,
                    frame_effects,
                ) {
                    log::warn!("Scatter order for {entity_id} refused: {cause}");
                    return false;
                }
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
                    auxiliary,
                    rules,
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
                    auxiliary,
                    rules,
                    frame_effects,
                )
                .accepted(),
            Command::Stop { entity_id } => {
                // Event IDLE 0x004C74CB..0x004C76BB replaces the Event
                // receiver with its decoded actor at 0x004C74D3. It never
                // compares the sender's House with the actor's owner; sender
                // eligibility belongs to Execute_DoList's House scan.
                // Source: original gamemd.exe instructions.
                // `0x004C74DE..0x004C74FE`: IDLE admits an actor that exists,
                // is alive (`+0x90`) and is out of limbo (`+0x81`). Unlike the
                // MEGAMISSION gate (`order_actor_admits`) it reads no
                // strength, so an object at 0 strength still stops. VERA's
                // dying mark stands for the UnInit that clears `+0x90`.
                if !self
                    .substrate
                    .entities
                    .get(*entity_id)
                    .is_some_and(|e| e.is_active() && !e.lifecycle.in_limbo())
                {
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
                        entity.seldom.tether.partner().is_some()
                            || matches!(entity.mission.current().raw(), 0x12 | 0x13)
                    })
                {
                    return true;
                }
                // `0x004C752A..0x004C7577`: off a bridge, an actor on a flat
                // cell whose screen projection lands in another cell (low
                // ground drawn behind a cliff) ends the event here, before any
                // write, as DEPLOY's arm does. A harvesting miner there keeps
                // its mission and NavCom.
                if crate::sim::movement::ground_pose::event_hidden_flat_cell(self, *entity_id) {
                    return true;
                }
                // Event6 IDLE4C74CB..4C76BB retains the committed/queued
                // mission and its dispatch timer. Only the ore-miner exception
                // below writes Guard. Ordinary Move/Attack handlers see the
                // cleared NavCom/TarCom on their next dispatch and own the idle
                // transition, including Temporal LetGo at that later boundary.
                // `0x004C757D..0x004C759C`: a Foot's navigation list is
                // emptied (twice: inline, then `0x004DA1C0`). The writes that
                // follow (`0x004C75BD..0x004C75D4`: `+0x520` = -1, `+0x524`,
                // `+0x528`, `+0x686`) reset the legacy waypoint path, whose
                // index YR never sets (`aircraft::move_mission`); VERA keeps no
                // state for it.
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
                let _ = self.assign_target_represented(*entity_id, None, rules, frame_effects);
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
                    let _ = self.assign_target_represented(*entity_id, None, rules, frame_effects);
                }
                // `0x004C762A..0x004C7634`: a spawner's manager drops its
                // targets, so a queued launch is cancelled and an attacking
                // wing is recalled at the next pass.
                crate::sim::spawn_manager::clear_all_spawn_targets(
                    self,
                    *entity_id,
                    rules,
                    frame_effects,
                );
                // The IDLE arm leaves the installed locomotor alone: a stopped
                // Chrono Miner's harvest Drive ends at the next frame's
                // FootClass::AI tail (`0x004DAE5F..0x004DAEC3`, `object_turn`),
                // when Is_Ok_To_End (`0x004AF970`) allows, so an order after
                // the Stop in the same frame still finds the Drive.
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
                self.open_topped_passengers_take_target(*entity_id, None, rules, frame_effects);
                // `0x004C7655..0x004C765F`: only a Unit (vt+0x2C is 1) goes on
                // to the miner arm and `0x004C769C..0x004C76AC`, which takes a
                // Slave Miner off its hunt (`sim::slave_manager`). A selected
                // Slave refinery's slaves keep working.
                if self
                    .substrate
                    .entities
                    .get(*entity_id)
                    .is_some_and(|e| e.category() == crate::map::entities::EntityCategory::Unit)
                {
                    self.commit_stop_miner_guard(*entity_id, rules);
                    self.reset_slave_manager(*entity_id, rules, frame_effects);
                }
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
                    auxiliary,
                    rules,
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
                    auxiliary,
                    rules,
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
                    auxiliary,
                    rules,
                    frame_effects,
                )
                .accepted(),
            Command::Guard { entity_id, target } => self
                .apply_megamission_command(
                    *entity_id,
                    MissionType::AreaGuard,
                    *target,
                    None,
                    auxiliary,
                    rules,
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
                if !self.command_entity_allowed(
                    command_owner,
                    granted_source,
                    *entity_id,
                    CommandCapability::FullControl,
                ) {
                    return false;
                }
                let delegated =
                    self.command_delegates_yard_redeploy(granted_source, *entity_id, rules);
                self.undeploy_building(
                    *entity_id,
                    delegated,
                    rules,
                    overlay_registry,
                    frame_effects,
                )
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
                if !self.command_entity_allowed(
                    command_owner,
                    granted_source,
                    *entity_id,
                    CommandCapability::Tactical,
                ) {
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
                    auxiliary,
                    rules,
                    frame_effects,
                )
                .accepted()
            }
            Command::SetRally {
                rx,
                ry,
                producer_ids,
            } => self.set_rally_point_for_producers(producer_ids, *rx, *ry, rules.ares()),
            Command::ArchiveObject {
                entity_id,
                target_id,
            } => self.execute_archive_event(
                *entity_id,
                Some(combat::TargetKind::Entity(*target_id)),
                rules.ares(),
            ),
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
            // EventClass::Execute @ 0x004C6CB0, SELL arm 0x004C6F12:
            // the prefix above detaches Planning (4C6F20), then +90/alive
            // and current Owner+30 must match the signed event House
            // (4C6F2D..4C6F48). A building takes SellBack(-1) at4C6FA2;
            // CanSell belongs to input admission, not this receiver.
            // Depot Unit/Aircraft sales at4C6F5A..4C6F96 run only when Ares
            // plays (below); otherwise buildings alone are sold.
            Command::SellBuilding { entity_id } => {
                let Some(entity) = self.substrate.entities.get(*entity_id) else {
                    return false;
                };
                if !entity.lifecycle.object_alive()
                    || !self.command_house_allowed(
                        command_owner,
                        granted_source,
                        entity.owner(),
                        CommandCapability::FullControl,
                    )
                {
                    return false;
                }
                // 4C6F5A..4C6F96: a Unit or Aircraft with a Building in the
                // cell of its coordinate takes On_Sold(-1). Ares makes the
                // player's SellUnit click send it (hook 0x6929FC); a game
                // without Ares keeps the building-only receiver.
                if rules.ares()
                    && matches!(
                        entity.category(),
                        crate::map::entities::EntityCategory::Unit
                            | crate::map::entities::EntityCategory::Aircraft
                    )
                {
                    let at = crate::sim::movement::ground_pose::object_get_coords(
                        entity,
                        self.resolved_terrain.as_ref(),
                    );
                    let on_building = u16::try_from(at.x.div_euclid(256))
                        .ok()
                        .zip(u16::try_from(at.y.div_euclid(256)).ok())
                        .and_then(|(rx, ry)| {
                            self.substrate.occupancy.first_building_on_layer(
                                rx,
                                ry,
                                crate::sim::movement::locomotor::MovementLayer::Ground,
                            )
                        })
                        .is_some();
                    return on_building
                        && production::foot_on_sold(
                            self,
                            rules,
                            *entity_id,
                            crate::sim::world::UninitContext::new(rules)
                                .with_effects(frame_effects),
                        );
                }
                let order =
                    if self.command_delegates_yard_redeploy(granted_source, *entity_id, rules) {
                        production::SellOrder::DelegatedRedeploy { explicit: false }
                    } else {
                        production::SellOrder::Player
                    };
                production::sell_back(
                    self,
                    rules,
                    *entity_id,
                    order,
                    overlay_registry,
                    frame_effects,
                )
            }
            Command::SellWallAtCell { x, y } => {
                let Some(overlays) = overlay_registry else {
                    return false;
                };
                // Event 0x4C6FC4 passes 0; ordinary SELL remains audible.
                self.sell_wall_at_cell(
                    command_owner,
                    granted_source,
                    *x,
                    *y,
                    false,
                    rules,
                    overlays,
                    frame_effects,
                )
            }
            // Event1/Event2 (0x004C6CB0 branches 0x004C6CF0/0x004C6D4D) drive the
            // BuildingClass+0x660 power latch through GoOnline 0x00452260 /
            // GoOffline 0x00452360. go_building_online/offline carry the native
            // semantics (idempotent latch, house power + tech-tree invalidation,
            // Stuff flag, factory queue refresh, EVA line for a human house). The
            // dispatch has no owner check; only the VERA network gate above adds one.
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
                // VERA shared control: one player resigning (B6 Leave beside
                // Observe, Surrender's OK) leaves a House other players still
                // control in the game; the EXIT that follows retires only them.
                if self
                    .executing_sender(sender, owner)
                    .is_some_and(|sender| self.house_keeps_other_sender(owner, sender))
                {
                    return true;
                }
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
                if !self.houses.contains_key(&owner) {
                    return false;
                }
                let sender = self.executing_sender(sender, owner);
                // VERA shared control: a House another player still controls
                // stays in the game; only this sender leaves.
                let house_left =
                    !sender.is_some_and(|sender| self.house_keeps_other_sender(owner, sender));
                // Execute_DoList's EXIT block (0x0064C7BF..0x0064C862), for
                // every sender: House+0x248, then Flag_To_Die when no other
                // human House is undefeated (0x005E2BA0).
                if house_left {
                    self.houses
                        .get_mut(&owner)
                        .expect("checked House")
                        .mark_left_game();
                }
                // Execute_DoList64C9B1 destroys this source on every peer.
                // Retire its existing NodeNameTag timing row at the same shared
                // event, before later records may choose another publisher.
                if let Some(sender) = sender {
                    self.session.retire_network_command_source(sender);
                }
                if house_left && self.other_undefeated_humans(owner) == 0 {
                    self.flag_house_to_die(owner);
                }
                // A Simulation without a current House (diagnostic and replay
                // drivers) has no remote senders. A shared House's senders
                // are told apart by their own sources. A watcher sends
                // nothing: the followed player's EXIT is a remote one too.
                let local = !self.session.watcher()
                    && match (self.session.local_shared_house_source(), sender) {
                        (Some(local), Some(sender)) => local == sender,
                        _ => self
                            .session
                            .current_house()
                            .is_none_or(|local| local == owner),
                    };
                if local {
                    // The local sender's EventClass__Execute (0x004C7903)
                    // writes the termination byte at 0x004C7917. The app
                    // consumes the owner-tagged edge after this tail.
                    if !self.quit_requested {
                        self.executed_exit_owner = Some(owner);
                    }
                    self.quit_requested = true;
                } else {
                    // A remote sender never reaches Execute (0x0064C91B):
                    // DestroyConnection(house, 0) runs for it (0x0064C9B1).
                    // Its WOL tournament tail (0x005DA8F7..0x005DA90F) kills
                    // the leaver's House here, where replays run it too; the
                    // app runs the connection and message work.
                    if house_left && self.session.wol_tournament_game() {
                        self.flag_house_to_die(owner);
                    }
                    self.departed_senders.extend(sender);
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
                    self.make_enemy(owner, other, rules, frame_effects, true);
                } else {
                    self.make_ally(owner, other, rules, overlay_registry, frame_effects, true);
                }
                true
            }
            Command::RemovePlayer { source } => {
                // EventClass::Execute 0x004C7B73: IsHumanControlled
                // (0x0050B730, House+0x1EC outside mode 0), then
                // Flag_To_Die (0x004C7BE1). A WOL tournament game skips the
                // human test (0x004C7BBF..0x004C7BCE).
                // RESIDUAL: mode 4's ladder-stats call (0x004C7BA0..0x004C7BBA)
                // is backend.
                let Ok(source) = i8::try_from(*source) else {
                    return false;
                };
                let Some(owner) = self.command_sender_owner(source) else {
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
                // VERA shared control: a House another player still controls
                // survives losing one of them.
                if !self.house_keeps_other_sender(owner, source) {
                    self.flag_house_to_die(owner);
                }
                // REMOVEPLAYER is synchronized source loss, distinct from
                // DESTRUCT/defeat: a connected WOL observer remains a publisher.
                self.session.retire_network_command_source(source);
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
                if !self.command_entity_allowed(
                    command_owner,
                    granted_source,
                    *transport_id,
                    CommandCapability::Tactical,
                ) {
                    return false;
                }
                let has_passengers = self.substrate.entities.get(*transport_id).is_some_and(|t| {
                    !t.passenger_role().occupants().is_empty()
                        || t.passenger_role()
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
                // Ares 3.0 100173A0 (hook 0x44731C): a tunnel entrance
                // unloads its owner's network (`sim::ares_tunnel`).
                let has_passengers = if rules.ares() {
                    has_passengers || self.ares_tunnel_offers_unload(*transport_id, rules)
                } else {
                    has_passengers
                };
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
                        // Ordinary self-click is encoded as MEGAMISSION with
                        // its own target and NULL destination before dispatch.
                        // This direct DTO adapter remains for internal/test
                        // callers. D uses the separate Event9 receiver, whose
                        // class setters clear both target and destination.
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
                        // transport branch; Aircraft slot `0x004151E0`). An
                        // aircraft needs only a loaded hold (+0x114, its
                        // What_Action `0x00417DCD..0x00417DDA`), a Carryall's
                        // cargo included.
                        let is_transport = category
                            == Some(crate::map::entities::EntityCategory::Aircraft)
                            || self
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
                            auxiliary,
                            rules,
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
                if !self.command_entity_allowed(
                    command_owner,
                    granted_source,
                    *attacker_id,
                    CommandCapability::Tactical,
                ) {
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
                    auxiliary,
                    rules,
                    frame_effects,
                )
                .accepted()
            }
            Command::CaptureBuilding {
                engineer_id,
                target_building_id,
            } => {
                if !self.command_entity_allowed(
                    command_owner,
                    granted_source,
                    *engineer_id,
                    CommandCapability::Tactical,
                ) {
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
                    auxiliary,
                    rules,
                    frame_effects,
                )
                .accepted()
            }
            Command::LaunchSuperWeapon { .. }
            | Command::CompletePaidSuperWeapon { .. }
            | Command::CancelPaidSuperWeapon { .. }
            | Command::BeginPaidSuperWeapon { .. } => {
                let owner = self.interner.intern(command_owner);
                self.apply_superweapon_command_for_house(
                    owner,
                    cmd,
                    rules,
                    overlay_registry,
                    frame_effects,
                )
            }
            // VERA-only opcode 0x31: keep the record decodable for old
            // recordings, but never release the tank here. Native
            // `BuildingClass::EmptyBunker @ 0x004595C0` has one caller, the
            // linked Unit's `Mission_Unload @ 0x0073D66D`, whose DEPLOY
            // receiver (`Event9` `0x004C76BC..0x004C7812`) owns the radio
            // Break, null destination/target, Unload queue and the actor's
            // alive/limbo admission. The release then happens in that Unit's
            // Logic update, not during command execution (#3675).
            Command::EjectBunker { bunker_id } => {
                if !self.command_entity_allowed(
                    command_owner,
                    granted_source,
                    *bunker_id,
                    CommandCapability::Tactical,
                ) {
                    return false;
                }
                let Some(unit_id) = self
                    .substrate
                    .entities
                    .get(*bunker_id)
                    .and_then(|b| b.bunker.occupant())
                else {
                    return false;
                };
                crate::sim::deploy::issue_order(self, unit_id, rules, frame_effects)
            }
        }
    }

    fn apply_superweapon_command_for_house(
        &mut self,
        owner: InternedId,
        command: &Command,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) -> bool {
        match command {
            Command::LaunchSuperWeapon {
                sw_type_id,
                target_rx,
                target_ry,
            } => {
                // The SPECIAL_PLACE event (`EventClass::Execute 0x004C78D6`).
                self.place_super_weapon(
                    rules,
                    owner,
                    *sw_type_id,
                    (*target_rx, *target_ry),
                    overlay_registry,
                    frame_effects,
                )
            }
            Command::CompletePaidSuperWeapon {
                selection,
                target_rx,
                target_ry,
            } => self.fire_super_weapon_selected(
                rules,
                owner,
                selection.destination,
                (*target_rx, *target_ry),
                overlay_registry,
                frame_effects,
                crate::sim::superweapon::SuperWeaponTargetSource::Manual,
                Some(*selection),
            ),
            Command::CancelPaidSuperWeapon { selection } => {
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
            } => self.fire_super_weapon_selected(
                rules,
                owner,
                selection.source,
                (*target_rx, *target_ry),
                overlay_registry,
                frame_effects,
                crate::sim::superweapon::SuperWeaponTargetSource::Manual,
                Some(*selection),
            ),
            _ => false,
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
    /// `ares` is [`RuleSet::ares`](crate::rules::ruleset::RuleSet::ares).
    fn set_rally_point_for_producers(
        &mut self,
        producer_ids: &[u64],
        rx: u16,
        ry: u16,
        ares: bool,
    ) -> bool {
        for &stable_id in producer_ids {
            self.execute_archive_event(
                stable_id,
                Some(crate::sim::combat::TargetKind::Cell(rx, ry)),
                ares,
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
    /// `ares` is [`RuleSet::ares`](crate::rules::ruleset::RuleSet::ares).
    fn execute_archive_event(
        &mut self,
        entity_id: u64,
        target: Option<crate::sim::combat::TargetKind>,
        ares: bool,
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
            // Ares 3.0 Networking_RespondToEvent_Selling (1005CE40, hook
            // 0x4C6DDB): an object on Selling (0x13) that has no archive
            // yet keeps none.
            if ares
                && object.mission.current().known()
                    == Some(crate::sim::mission::MissionType::Selling)
                && object.archive_target().is_none()
            {
                return true;
            }
            object.set_archive_target(target);
        }
        true
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
    /// * Event9 DEPLOY orders (`deploy::issue_order`,
    ///   `0x004C76BC..0x004C7812`) carry their own alive/limbo/tether gate
    ///   rather than this one. `Command::EjectBunker` shares that route
    ///   (#3675): the bunker-eject residual about an ungated command-time
    ///   release is gone with it.
    /// * Aircraft reservation teardown remains a legacy command policy.
    /// * **The manager abandon** at 0x004C73E1-0x004C73EA calls 0x006B0C80 on
    ///   `[actor+0x2D8]`, the SlaveManagerClass (not a spawn manager), whenever
    ///   the queued mission is not Attack; it is not Foot-gated. The funnel
    ///   below runs it for any actor and Stop's own copy
    ///   (0x004C769C-0x004C76AC) for a Unit only (0x004C7655-0x004C765F), both
    ///   through `Simulation::reset_slave_manager`; the other orders outside
    ///   the funnel do not yet.
    /// * **`TeamClass__Remove_Member`** runs in the MEGAMISSION funnel
    ///   and in its shared prefix; the Event9 DEPLOY route and the dormant
    ///   aircraft Unload arm bypass that prefix and keep a team member in
    ///   its team.
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
        receiver.seldom.radio_contacts.slot(0) == Some(destination_id)
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
                Some(house_alliances),
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
    /// A garrison's self-click Unload uses this event's self-target codec;
    /// its D key uses the separate DEPLOY event. Direct semantic Unload and
    /// dormant aircraft Unload retain their adapters. In native order:
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
    /// the NavComAux setter (vt+0x47C), after Set_Destination clears it.
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
        auxiliary: MegaMissionTarget,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
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
            // A refused setter is logged, as the ordinary arm below does
            // (a mod type built without a locomotor has no coordinate).
            if let Err(cause) =
                self.assign_target_represented(entity_id, None, rules, frame_effects)
            {
                log::warn!("Area Guard order target for {entity_id} refused: {cause}");
            }
            // The target token, converted at `0x004C7418` for the
            // destination and again at `0x004C7428` for the archive.
            let post = self.resolve_event_token(target);
            if let Err(cause) = self.assign_destination_represented(
                entity_id,
                post.map(Into::into),
                rules,
                frame_effects,
            ) {
                log::warn!("Area Guard order destination for {entity_id} refused: {cause}");
            }
            // The archive write follows the virtual destination call even
            // when that class setter refuses its request.
            let archive = self.resolve_event_token(target);
            // The order's actor was admitted above; the setters do not remove
            // it from the store, but a peer's record must not stop the game
            // if one ever did.
            if let Some(actor) = self.substrate.entities.get_mut(entity_id) {
                actor.set_archive_target(archive);
            }
            return MegaMissionExecution::Executed;
        }
        // `0x004C745B`, then the target setter.
        let target = self.resolve_event_token(target);
        // Its changed-null path6FCF5B also resets retained burst state;
        // dropping Target alone cannot.
        let _ = self.assign_target_represented(entity_id, target, rules, frame_effects);
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
            frame_effects,
        ) {
            log::warn!("MegaMission destination {entity_id}: {cause}");
        }
        if let Some(target) = target {
            self.open_topped_passengers_take_target(entity_id, Some(target), rules, frame_effects);
        }
        if foot && auxiliary != MegaMissionTarget::Null {
            let auxiliary = match auxiliary {
                MegaMissionTarget::Null => None,
                MegaMissionTarget::Cell { x, y } => {
                    Some(combat::TargetKind::Cell(x as u16, y as u16))
                }
                MegaMissionTarget::Object { id } => {
                    u64::try_from(id).ok().map(combat::TargetKind::Entity)
                }
            };
            let auxiliary = self.resolve_event_token(auxiliary);
            if let Some(actor) = self.substrate.entities.get_mut(entity_id) {
                actor.navigation.set_nav_com_aux(auxiliary.map(Into::into));
            }
        }
        MegaMissionExecution::Executed
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
#[path = "battle_opening_tests.rs"]
mod battle_opening_tests;

#[cfg(all(test, tests_sim_world_t_z))]
mod tests;
