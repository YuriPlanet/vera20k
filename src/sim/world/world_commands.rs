//! Individual command payload behavior for the Simulation.
//!
//! Contains `apply_command()` and its helper methods: selection snapshots,
//! ownership checks, and friendship queries. `command_schedule` owns queue
//! admission and scheduled batch order.
//!
//! Dependency rules: same as sim/ (depends on rules/, map/; never render/ui/audio/net).

use std::collections::BTreeSet;

use super::ground_move::GroundMove;
use super::{SimSoundEvent, Simulation, SimulationWallRuntimeHost};
use crate::map::entities::EntityCategory;
use crate::map::houses::are_houses_friendly;
#[cfg(test)]
use crate::rules::locomotor_type::MovementZone;
use crate::rules::locomotor_type::SpeedType;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_rect::canonical_cell_coord;
use crate::sim::combat;
#[cfg(test)]
use crate::sim::combat::combat_aoe::CellTargetDetach;
use crate::sim::combat::combat_aoe::expire_cell_target_references;
use crate::sim::command::{
    COMMAND_RECORD_LEN, Command, CommandEnvelope, CommandRecord, ExitRecord, MegaMissionOrder,
    MegaMissionRecord, MegaMissionTarget, SellWallAtCellRecord,
};
use crate::sim::components::OrderIntent;
use crate::sim::mission::MissionType;
use crate::sim::movement;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::overlay_grid::{
    OverlayRecalcOutcome, RecomputeResult, runtime_wall_cleanup_visit_at,
};
use crate::sim::passenger;
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::zone_incremental::ZoneRepairKind;
use crate::sim::production;
use crate::util::fixed_math::{SimFixed, ra2_speed_to_leptons_per_second};

/// Read-only snapshot of entity + rules data needed for issuing movement commands.
/// Captured once to avoid repeated entity lookups and type_ref clones.
///
/// `pub(crate)` so the pursuit pre-combat stage in `world_orders.rs` can reuse
/// it — pursuit-issued movement must match Move-command-issued movement
/// exactly to keep behavior consistent.
pub(crate) struct MoveInfo {
    pub(crate) speed: SimFixed,
    pub(crate) loco_layer: MovementLayer,
    pub(crate) speed_type: SpeedType,
    pub(crate) is_harvester: bool,
    #[cfg(test)]
    pub(crate) movement_zone: MovementZone,
    #[cfg(test)]
    pub(crate) regular_crusher: bool,
    #[cfg(test)]
    pub(crate) omni_crusher: bool,
    #[cfg(test)]
    pub(crate) drive_accelerates: bool,
}

#[cfg(test)]
impl MoveInfo {
    pub(crate) fn crush_capability(&self) -> movement::bump_crush::CrushCapability {
        movement::bump_crush::CrushCapability::new(self.regular_crusher, self.omni_crusher)
    }

    pub(crate) fn can_crush_units(&self) -> bool {
        self.crush_capability().can_crush_units()
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WallSellZoneRepairTestStep {
    pub(crate) repair_cell: (u16, u16),
    pub(crate) walkable_cross: [bool; 5],
    pub(crate) movement_class_cross: [u8; 5],
}

#[cfg(test)]
std::thread_local! {
    static WALL_SELL_ZONE_REPAIR_TEST_TRACE:
        std::cell::RefCell<Vec<WallSellZoneRepairTestStep>> = const {
            std::cell::RefCell::new(Vec::new())
        };
}

#[cfg(test)]
pub(crate) fn clear_wall_sell_zone_repair_test_trace() {
    WALL_SELL_ZONE_REPAIR_TEST_TRACE.with(|trace| trace.borrow_mut().clear());
}

#[cfg(test)]
pub(crate) fn take_wall_sell_zone_repair_test_trace() -> Vec<WallSellZoneRepairTestStep> {
    WALL_SELL_ZONE_REPAIR_TEST_TRACE.with(|trace| std::mem::take(&mut *trace.borrow_mut()))
}

#[cfg(test)]
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

impl Simulation {
    /// G/command-bar Guard730D60 chooses each admitted actor's current
    /// navigation Cell before Player_Send_Command6FFBE0 emits mission11.
    /// IsControllable700C40 owns the human/bunker/warp/slave gates; after
    /// that predicate the additional IsActive7010D0 test is the current
    /// weapon pair. Unit slave-manager/Harvester fallback bypasses both.
    /// Existing controllability lifecycle residuals remain documented at
    /// that owner. This is an input producer, never an event-execution gate.
    pub(crate) fn area_guard_key_command(&self, id: u64, rules: &RuleSet) -> Option<Command> {
        let actor = self.substrate.entities.get(id)?;
        if actor.category == EntityCategory::Structure {
            return None;
        }
        let object = self.object_type(actor.type_ref(), rules)?;
        let ordinary = self.techno_player_controllable(id, rules)
            && combat::combat_weapon::is_armed(actor, object);
        let miner = actor.category == EntityCategory::Unit
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

    /// Build the native MegaMission record for an ordinary Move or Area Guard.
    ///
    /// `EventClass__BuildMegaMissionEnvelope` at `gamemd.exe` `0x004C6860`
    /// stores HouseClass registration and Abstract stable identity separately;
    /// the source therefore need not belong to the issuing house. Rust-only
    /// queued waypoints are not representable here.
    pub(crate) fn encode_megamission_record(
        &self,
        command_owner: crate::sim::intern::InternedId,
        command: &Command,
    ) -> Option<CommandRecord> {
        let (source_id, order) = match *command {
            Command::Move {
                entity_id,
                target_rx,
                target_ry,
                queue: false,
            } => (
                entity_id,
                MegaMissionOrder::Move {
                    target_x: i16::try_from(target_rx).ok()?,
                    target_y: i16::try_from(target_ry).ok()?,
                },
            ),
            Command::Guard { entity_id, target } => {
                let post = match target {
                    None => MegaMissionTarget::Null,
                    Some(combat::TargetKind::Cell(x, y)) => MegaMissionTarget::Cell {
                        // Preserve native signed CellStruct word bits. The
                        // codec still rejects pairs its token cannot roundtrip.
                        x: x as i16,
                        y: y as i16,
                    },
                    Some(combat::TargetKind::Entity(id)) => MegaMissionTarget::Object {
                        id: i32::try_from(id).ok()?,
                    },
                };
                (entity_id, MegaMissionOrder::AreaGuard { post })
            }
            _ => return None,
        };
        if !self.houses.contains_key(&command_owner)
            || self.substrate.entities.get(source_id).is_none()
        {
            return None;
        }
        let house_id = self
            .session
            .house_order
            .iter()
            .position(|&owner| owner == command_owner)
            .and_then(|index| i8::try_from(index).ok())?;
        let typed = MegaMissionRecord {
            house_id,
            frame: self.session.binary_frame as i32,
            source_id: i32::try_from(source_id).ok()?,
            order,
        };
        let mut record = CommandRecord::decode_exact(&[0; COMMAND_RECORD_LEN]).ok()?;
        typed.write_into(&mut record).ok()?;
        Some(record)
    }

    /// Encode the native synchronized record for one locally issued wall sale.
    /// House bytes are HouseClass registration indices, never interner ids.
    pub(crate) fn encode_sell_wall_at_cell_record(
        &self,
        command_owner: crate::sim::intern::InternedId,
        x: i16,
        y: i16,
    ) -> Option<CommandRecord> {
        if !self.houses.contains_key(&command_owner) {
            return None;
        }
        let house_id = self
            .session
            .house_order
            .iter()
            .position(|&owner| owner == command_owner)
            .and_then(|index| i8::try_from(index).ok())?;
        SellWallAtCellRecord {
            house_id,
            frame: self.session.binary_frame,
            x,
            y,
        }
        .encode()
        .ok()
    }

    /// Encode the header-only native EXIT event issued by Abort confirmation.
    /// House bytes are HouseClass registration indices, never interner ids.
    pub(crate) fn encode_exit_record(
        &self,
        command_owner: crate::sim::intern::InternedId,
    ) -> Option<CommandRecord> {
        if !self.houses.contains_key(&command_owner) {
            return None;
        }
        let house_id = self
            .session
            .house_order
            .iter()
            .position(|&owner| owner == command_owner)
            .and_then(|index| i8::try_from(index).ok())?;
        ExitRecord {
            house_id,
            frame: self.session.binary_frame,
        }
        .encode()
        .ok()
    }

    /// Decode one synchronized record after its queue has admitted the stamped
    /// frame. The semantic envelope is only a typed execution view of the raw
    /// bytes; timing/processed-bit ownership remains with the raw queue.
    pub(crate) fn decode_native_command_record(
        &self,
        record: &CommandRecord,
        execute_tick: u64,
    ) -> Option<CommandEnvelope> {
        if let Some(typed) = MegaMissionRecord::decode(record) {
            let house_index = usize::try_from(typed.house_id).ok()?;
            let owner = *self.session.house_order.get(house_index)?;
            if !self.houses.contains_key(&owner) {
                return None;
            }
            let entity_id = u64::try_from(typed.source_id).ok()?;
            if self.substrate.entities.get(entity_id).is_none() {
                return None;
            }
            let command = match typed.order {
                MegaMissionOrder::Move { target_x, target_y } => Command::Move {
                    entity_id,
                    target_rx: u16::try_from(target_x).ok()?,
                    target_ry: u16::try_from(target_y).ok()?,
                    queue: false,
                },
                MegaMissionOrder::AreaGuard { post } => Command::Guard {
                    entity_id,
                    target: match post {
                        MegaMissionTarget::Null => None,
                        MegaMissionTarget::Cell { x, y } => {
                            Some(combat::TargetKind::Cell(x as u16, y as u16))
                        }
                        MegaMissionTarget::Object { id } => {
                            Some(combat::TargetKind::Entity(u64::try_from(id).ok()?))
                        }
                    },
                },
            };
            return Some(CommandEnvelope::new(owner, execute_tick, command));
        }

        if let Some(typed) = ExitRecord::decode(record) {
            let house_index = usize::try_from(typed.house_id).ok()?;
            let owner = *self.session.house_order.get(house_index)?;
            return self
                .houses
                .contains_key(&owner)
                .then(|| CommandEnvelope::new(owner, execute_tick, Command::ExitMatch));
        }

        let typed = SellWallAtCellRecord::decode(record)?;
        let house_index = usize::try_from(typed.house_id).ok()?;
        let owner = *self.session.house_order.get(house_index)?;
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
    /// path/cost view and the retained base movement-class topology. Zone ID
    /// assignment remains owned by the ordered native repair callback.
    fn refresh_wall_sale_recalc_prefix(
        &mut self,
        tail_grid: &mut Option<PathGrid>,
        rx: u16,
        ry: u16,
        navigation_changed: bool,
    ) {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };

        if navigation_changed {
            crate::sim::pathfinding::terrain_cost::refresh_canonical_terrain_costs_at(
                &mut self.terrain_costs,
                terrain,
                (rx, ry),
            );
            if tail_grid.as_ref().is_some_and(|tail| {
                tail.width() != terrain.width() || tail.height() != terrain.height()
            }) {
                *tail_grid = None;
            }
            if let (Some(tail), Some(cell)) = (tail_grid.as_mut(), terrain.cell(rx, ry)) {
                let _ = tail.refresh_resolved_cell(cell, false);
            }
        }

        if let Some(zone_grid) = self.zone_grid.as_mut() {
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
        #[cfg(not(test))]
        let _ = sold_cell;
        let Some(_zone_grid) = self.zone_grid.as_mut() else {
            return;
        };
        #[cfg(test)]
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

    fn sell_wall_at_cell(
        &mut self,
        command_owner: &str,
        x: i16,
        y: i16,
        rules: &RuleSet,
        overlays: &crate::rules::overlay_types::OverlayTypeRegistry,
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
        let owner_admitted = if self.session.game_mode_nonzero {
            owner_house.is_human
        } else {
            owner_house.is_human || owner_house.player_control
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
        if rules.general.sell_sound.is_some()
            && let Some(receiver) = self.interner.get(command_owner)
        {
            self.sound_events.push(SimSoundEvent::WallSold { receiver });
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
        self.refresh_wall_sale_recalc_prefix(&mut tail_grid, rx, ry, sold_navigation_changed);

        // Selling invokes exactly one PostDestructionWallCleanup at the sold
        // cell: N, E, S, W, self. It is not damage's four-cardinal fan-out.
        const CROSS: [(i32, i32); 5] = [(0, -1), (1, 0), (0, 1), (-1, 0), (0, 0)];
        #[cfg(test)]
        let mut detach_trace: Vec<CellTargetDetach> = Vec::new();
        for (dx, dy) in CROSS {
            let visit = {
                let mut host = SimulationWallRuntimeHost {
                    entities: &mut self.substrate.entities,
                    #[cfg(test)]
                    detach_trace: &mut detach_trace,
                    radar_dirty_cells: &mut self.radar_terrain_dirty_cells,
                    radar_dirty_generation: &mut self.radar_terrain_dirty_generation,
                    tactical_dirty_cells: &mut self.tactical_dirty_cells,
                    terrain_costs: &mut self.terrain_costs,
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
            self.refresh_wall_sale_recalc_prefix(&mut tail_grid, nx, ny, recalc.navigation_changed);
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
        self.mark_radar_terrain_dirty_cells([(rx, ry)]);

        expire_cell_target_references(
            &mut self.substrate.entities,
            rx,
            ry,
            #[cfg(test)]
            &mut detach_trace,
        );

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

    /// Snapshot entity + rules data needed for movement dispatch in one lookup.
    pub(crate) fn resolve_move_info(
        &self,
        entity_id: u64,
        rules: Option<&RuleSet>,
    ) -> Option<MoveInfo> {
        let e = self.substrate.entities.get(entity_id)?;
        let loco = e.locomotor.as_ref();
        let loco_layer = e.movement_layer_or_ground();
        let speed_type = loco.map(|l| l.speed_type).unwrap_or(SpeedType::Track);

        let obj = rules.and_then(|r| self.object_type(e.type_ref(), r));
        // The resolver behind Move, AttackMove, Enter, C4, capture and
        // bunker-entry.
        let speed = crate::sim::movement::order_speed(e, obj, rules, &self.houses);

        Some(MoveInfo {
            speed,
            loco_layer,
            speed_type,
            is_harvester: obj.map_or(false, |o| o.harvester),
            #[cfg(test)]
            movement_zone: obj.map_or(MovementZone::Normal, |o| o.movement_zone),
            #[cfg(test)]
            regular_crusher: e.regular_crusher,
            #[cfg(test)]
            omni_crusher: e.omni_crusher,
            #[cfg(test)]
            drive_accelerates: e.drive_accelerates,
        })
    }

    /// Dispatch a single command, returning true if it was successfully applied.
    #[cfg(test)]
    pub(crate) fn apply_command(
        &mut self,
        command_owner: &str,
        cmd: &Command,
        rules: Option<&RuleSet>,
    ) -> bool {
        self.apply_command_with_overlays(command_owner, cmd, rules, None)
    }

    pub(crate) fn apply_command_with_overlays(
        &mut self,
        command_owner: &str,
        cmd: &Command,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        match cmd {
            Command::Select { entity_ids, .. } => self.apply_selection_snapshot(entity_ids, rules),
            Command::Move {
                entity_id,
                target_rx,
                target_ry,
                queue,
            } => {
                if !self.entity_owned_by_id(command_owner, *entity_id) {
                    return false;
                }
                if self
                    .substrate
                    .entities
                    .get(*entity_id)
                    .is_some_and(|e| e.is_deployed())
                    && !rules.is_some_and(|rules| {
                        self.infantry_setter_receiver(
                            *entity_id,
                            crate::sim::components::NavTargetRef::cell(*target_rx, *target_ry),
                            rules,
                        ) || self.drive_unit_setter_receiver(*entity_id)
                    })
                {
                    return false;
                }
                // Event4C7353 queues Move and clears Target before the class
                // applies its own refusal. Infantry51AA40 tests human Doing;
                // Unit741970 tests deployment after the same-Nav/force arm.
                // Original non-Deployer GI and delayed deployed MTNK controls
                // in walk_first_path.json retain queued2 even when the void
                // class setter leaves NavCom NULL. Decoded commands must not
                // repeat the input wrapper's separate no-order decision.
                // Other class adapters retain the legacy early gate until
                // their destination owner is represented (remaining #687).
                // Native order admission: a dead, zero-strength or in-limbo
                // actor abandons the whole order and keeps its previous one.
                if !self.order_actor_admits(*entity_id) {
                    return false;
                }
                // Queue Move. The destination/radio owners handle any existing
                // dock contact.
                self.queue_megamission(*entity_id, MissionType::Move, rules);
                // Clear attack and order intent.
                let _ = self.assign_target_represented(*entity_id, None, rules);
                if let Some(e) = self.substrate.entities.get_mut(*entity_id) {
                    // Event MegaMission4C7467 calls Assign_Target before the
                    // destination setter. Its changed-null path6FCF5B also
                    // resets retained burst state; dropping Target alone cannot.
                    e.order_intent = None;
                    e.c4_plant = None;
                }
                // Snapshot speed, locomotor, and rules data in one lookup.
                let Some(info) = self.resolve_move_info(*entity_id, rules) else {
                    return false;
                };
                let aircraft = self
                    .substrate
                    .entities
                    .get(*entity_id)
                    .is_some_and(|e| e.category == EntityCategory::Aircraft);
                let result = if let Some(rules) = rules.filter(|_| aircraft) {
                    // Event4C747C's virtual+480 is Aircraft41AA80: the NavCom
                    // and the locomotor's Move_To. Mission_Move flies it from
                    // there (`aircraft::move_mission`).
                    self.assign_aircraft_destination(
                        *entity_id,
                        Some(crate::sim::components::NavTargetRef::cell(
                            *target_rx, *target_ry,
                        )),
                        rules,
                    );
                    true
                } else if info.loco_layer == MovementLayer::Air {
                    // Air units fly in straight lines — no A* pathfinding needed.
                    self.issue_air_cell_destination(
                        *entity_id,
                        (*target_rx, *target_ry),
                        info.speed,
                        rules,
                    )
                } else if !*queue
                    && let Some(rules) = rules
                    && self.unit_setter_receiver(*entity_id, Some(rules))
                {
                    // Event4C746F pushes1;4C747C calls virtual+480 with
                    // the resolved destination. Unit741970 owns same-NavCom
                    // return/force, class preprocessing and queue clearing.
                    // Native: track_destination.json Unit rows, replayed by
                    // command_move_destinations_match_native_unit_setter.
                    // The setter is void: a guarded class refusal still
                    // executes this Event. Use the shared dispatcher, as
                    // for internal Drive orders, rather than interpreting
                    // the class's implementation result as input refusal.
                    // Original delayed6E0 MTNK: walk_first_path.json.
                    self.assign_destination_represented(
                        *entity_id,
                        Some(crate::sim::components::NavTargetRef::cell(
                            *target_rx, *target_ry,
                        )),
                        Some(rules),
                        overlay_registry,
                    )
                    .map_or_else(
                        |cause| {
                            log::warn!("Unit Move destination {}: {cause}", entity_id);
                            false
                        },
                        |()| true,
                    )
                } else {
                    self.issue_ground_move(
                        GroundMove {
                            entity_id: *entity_id,
                            target: (*target_rx, *target_ry),
                            speed: info.speed,
                            queue: *queue,
                            speed_type: Some(info.speed_type),
                            owner_blocks: true,
                            object_destination: None,
                        },
                        rules,
                        overlay_registry,
                    )
                };
                result
            }
            Command::Stop { entity_id } => {
                if !self.entity_owned_by_id(command_owner, *entity_id) {
                    return false;
                }
                // Native order admission (actor half).
                if !self.order_actor_admits(*entity_id) {
                    return false;
                }
                // The IDLE arm returns at `0x004C7504..0x004C750C` when the
                // object's tether byte (`+0x418`) is set: a miner that has
                // entered its dock ignores Stop and finishes unloading, and so
                // does a vehicle still leaving its war factory (the other
                // writer of `dock_entered_with`). Current missions 0x12/0x13
                // also return before radio, destination and target writes.
                if self
                    .substrate
                    .entities
                    .get(*entity_id)
                    .is_some_and(|entity| {
                        entity.dock_entered_with.is_some()
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
                // Event6 sends BREAK to every sparse contact (vt28065ACE0
                // at4C75E0), before NULL destination4C75ED/target4C75F8.
                // Unlike MEGAMISSION it never clears Foot pending-entry+500.
                crate::sim::radio::broadcast_break(self, *entity_id, rules);
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
                self.assign_null_destination(*entity_id, rules, overlay_registry);
                if let Some(e) = self.substrate.entities.get_mut(*entity_id) {
                    e.order_intent = None;
                    e.c4_plant = None;
                }
                // Event Stop4C75F8 invokes virtual+3C8 AFTER its null
                // destination4C75ED, including the Infantry class effects.
                let _ = self.assign_target_represented(*entity_id, None, rules);
                // `0x004C75FE..0x004C7624`: a `BalloonHover=` type takes both
                // again. Its setter's arm (`0x00741983`) kept the NavCom while
                // the Target stood; without the Target this one clears it.
                let balloon = rules.is_some_and(|rules| {
                    self.substrate
                        .entities
                        .get(*entity_id)
                        .and_then(|entity| self.object_type(entity.type_ref(), rules))
                        .is_some_and(|object| object.balloon_hover)
                });
                if balloon {
                    self.assign_null_destination(*entity_id, rules, overlay_registry);
                    let _ = self.assign_target_represented(*entity_id, None, rules);
                }
                // `0x004C762A..0x004C7634`: a spawner's manager drops its
                // targets, so a queued launch is cancelled and an attacking
                // wing is recalled at the next pass.
                crate::sim::spawn_manager::clear_all_spawn_targets(
                    self,
                    *entity_id,
                    rules,
                    overlay_registry,
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
                if may_end && let Some(e) = self.substrate.entities.get_mut(*entity_id) {
                    crate::sim::movement::locomotor_owner::end_admitted_piggyback(e);
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
                if let Some(rules) = rules {
                    self.open_topped_passengers_take_target(*entity_id, None, rules);
                }
                self.commit_stop_miner_guard(*entity_id);
                // `0x004C769C..0x004C76AC`: Stop takes a Slave Miner off its
                // hunt (`sim::slave_manager`).
                if let Some(rules) = rules {
                    self.reset_slave_manager(*entity_id, rules);
                }
                true
            }
            Command::Attack {
                attacker_id,
                target_id,
            } => {
                if !self.entity_owned_by_id(command_owner, *attacker_id) {
                    return false;
                }
                if !self.substrate.entities.contains(*target_id) {
                    return false;
                }
                if !self.can_attack_target_by_id(*attacker_id, *target_id) {
                    return false;
                }
                // Native order admission: BOTH the actor and the clicked Target
                // object are gated, and a failure on either abandons the whole
                // order. A victim that dies in the same tick is still resolvable
                // in the store, so without the Target half the attacker would
                // retask onto a corpse instead of keeping its previous order.
                if !self.order_actor_admits(*attacker_id)
                    || !self.order_object_token_admits(*target_id)
                {
                    return false;
                }
                // Retask onto Attack keeping the interrupt stack (combat sets
                // the target).
                self.queue_megamission(*attacker_id, MissionType::Attack, rules);
                if let Some(e) = self.substrate.entities.get_mut(*attacker_id) {
                    e.order_intent = None;
                }
                let issued = self.order_attack_target(
                    *attacker_id,
                    combat::TargetKind::Entity(*target_id),
                    rules,
                );
                if issued {
                    // `0x004C747C`: the order's NULL destination through the
                    // class setter, after its target (`0x004C7467`).
                    self.assign_null_destination(*attacker_id, rules, overlay_registry);
                    // `0x004C7482..0x004C749D`: an open-topped transport's
                    // riders take the ordered target.
                    if let Some(rules) = rules {
                        self.open_topped_passengers_take_target(
                            *attacker_id,
                            Some(combat::TargetKind::Entity(*target_id)),
                            rules,
                        );
                    }
                }
                issued
            }
            Command::ForceAttack {
                attacker_id,
                target_id,
            } => {
                if !self.entity_owned_by_id(command_owner, *attacker_id) {
                    return false;
                }
                if !self.substrate.entities.contains(*target_id) {
                    return false;
                }
                // Native order admission (actor + Target token). Force-fire
                // bypasses the alliance test, not the liveness gate.
                if !self.order_actor_admits(*attacker_id)
                    || !self.order_object_token_admits(*target_id)
                {
                    return false;
                }
                // Force-attack bypasses friendship check (Ctrl+click). Retask
                // onto Attack keeping fields.
                self.queue_megamission(*attacker_id, MissionType::Attack, rules);
                if let Some(e) = self.substrate.entities.get_mut(*attacker_id) {
                    e.order_intent = None;
                }
                let issued = self.order_attack_target(
                    *attacker_id,
                    combat::TargetKind::Entity(*target_id),
                    rules,
                );
                if issued {
                    self.assign_null_destination(*attacker_id, rules, overlay_registry);
                    // `0x004C7482..0x004C749D`: an open-topped transport's
                    // riders take the ordered target.
                    if let Some(rules) = rules {
                        self.open_topped_passengers_take_target(
                            *attacker_id,
                            Some(combat::TargetKind::Entity(*target_id)),
                            rules,
                        );
                    }
                }
                issued
            }
            Command::ForceAttackCell {
                attacker_id,
                target_rx,
                target_ry,
            } => {
                if !self.entity_owned_by_id(command_owner, *attacker_id) {
                    return false;
                }
                // Native order admission (actor half only — a cell token names
                // no object, so the Target/Destination gates do not apply).
                if !self.order_actor_admits(*attacker_id) {
                    return false;
                }
                // No target-entity existence check — cells always "exist".
                // Retask onto Attack keeping fields.
                self.queue_megamission(*attacker_id, MissionType::Attack, rules);
                if let Some(e) = self.substrate.entities.get_mut(*attacker_id) {
                    e.order_intent = None;
                }
                let issued = self.order_attack_target(
                    *attacker_id,
                    combat::TargetKind::Cell(*target_rx, *target_ry),
                    rules,
                );
                if issued {
                    self.assign_null_destination(*attacker_id, rules, overlay_registry);
                    // `0x004C7482..0x004C749D`, a cell target included.
                    if let Some(rules) = rules {
                        self.open_topped_passengers_take_target(
                            *attacker_id,
                            Some(combat::TargetKind::Cell(*target_rx, *target_ry)),
                            rules,
                        );
                    }
                }
                issued
            }
            Command::AttackMove {
                entity_id,
                target_rx,
                target_ry,
                queue,
            } => {
                if !self.entity_owned_by_id(command_owner, *entity_id) {
                    return false;
                }
                if self
                    .substrate
                    .entities
                    .get(*entity_id)
                    .is_some_and(|e| e.is_deployed())
                {
                    return false;
                }
                // Native order admission (actor half only — attack-move carries
                // a cell token).
                if !self.order_actor_admits(*entity_id) {
                    return false;
                }
                // Retask onto AttackMove (the order_intent set after the move
                // issues is the real driver).
                self.queue_megamission(*entity_id, MissionType::AttackMove, rules);
                let _ = self.assign_target_represented(*entity_id, None, rules);

                // Snapshot speed, locomotor, and rules data in one lookup.
                let Some(info) = self.resolve_move_info(*entity_id, rules) else {
                    return false;
                };
                let aircraft = self
                    .substrate
                    .entities
                    .get(*entity_id)
                    .is_some_and(|e| e.category == EntityCategory::Aircraft);
                let issued = if let Some(rules) = rules.filter(|_| aircraft) {
                    // Event4C747C's virtual+480 (Aircraft41AA80) takes the
                    // ordered cell, so Mission_Move, until the Commence ends
                    // it, steers there too. The original sends mission 29 to
                    // Mission_Sleep (`0x005B34C4`; `aircraft::dispatch_mission`'s
                    // RESIDUAL).
                    self.assign_aircraft_destination(
                        *entity_id,
                        Some(crate::sim::components::NavTargetRef::cell(
                            *target_rx, *target_ry,
                        )),
                        rules,
                    );
                    true
                } else if info.loco_layer == MovementLayer::Air {
                    // Air units fly in straight lines.
                    self.issue_air_cell_destination(
                        *entity_id,
                        (*target_rx, *target_ry),
                        info.speed,
                        rules,
                    )
                } else {
                    self.issue_ground_move(
                        GroundMove {
                            entity_id: *entity_id,
                            target: (*target_rx, *target_ry),
                            speed: info.speed,
                            queue: *queue,
                            speed_type: Some(info.speed_type),
                            owner_blocks: true,
                            object_destination: None,
                        },
                        rules,
                        overlay_registry,
                    )
                };
                if issued {
                    if let Some(e) = self.substrate.entities.get_mut(*entity_id) {
                        e.order_intent = Some(OrderIntent::AttackMove {
                            goal_rx: *target_rx,
                            goal_ry: *target_ry,
                        });
                    }
                }
                issued
            }
            Command::Guard { entity_id, target } => self.apply_guard_command(
                command_owner,
                *entity_id,
                *target,
                rules,
                overlay_registry,
            ),
            Command::DeployMcv { entity_id } => {
                let Some(rules) = rules else { return false };
                if !self.entity_owned_by_id(command_owner, *entity_id) {
                    return false;
                }
                crate::sim::deploy::issue_order(self, *entity_id, rules)
            }
            // RESIDUAL: retail undeploys a building only through a cell click
            // (`0x004436F0`: the rally/ArchiveTarget event 0x1E, then SELL
            // 0x16 -> `Sell_Back(-1) @ 0x00447110` queuing Selling), and its
            // What_Action never answers ACTION_SELF for an UndeploysInto
            // building (`0x00447210`); VERA's app offers the undeploy on a
            // self-click and starts it here with no destination cell: the
            // pack-up times as an archive-bearing one, but the unit is not
            // sent anywhere after the conversion.
            Command::UndeployBuilding { entity_id } => {
                let Some(rules) = rules else { return false };
                if !self.entity_owned_by_id(command_owner, *entity_id) {
                    return false;
                }
                self.undeploy_building(*entity_id, rules, overlay_registry)
            }
            // The synchronized self-deploy order queues Unload; the concrete
            // Infantry51F6E0 handler selects Doing27/31 on the object's visit.
            // CanDeploy700D50's UI tube-neighborhood gate484AE0 is a separate
            // required query; it is not invented by this event receiver.
            Command::ToggleInfantryDeploy { entity_id } => {
                if !self.entity_owned_by_id(command_owner, *entity_id)
                    || !self.order_actor_admits(*entity_id)
                {
                    return false;
                }
                let Some(rules) = rules else { return false };
                let Some(actor) = self.substrate.entities.get(*entity_id) else {
                    return false;
                };
                if actor.category != EntityCategory::Infantry
                    || !self
                        .object_type(actor.type_ref(), rules)
                        .is_some_and(|object| object.deployer)
                {
                    return false;
                }
                self.order_unload(*entity_id, rules, overlay_registry);
                true
            }
            Command::SetRally {
                rx,
                ry,
                producer_ids,
            } => self.set_rally_point_for_producers(command_owner, producer_ids, *rx, *ry, rules),
            // Production events act on the event's own house (the
            // EventClass header's house id); the payload names no owner.
            Command::QueueProduction { type_id } => {
                let Some(rules) = rules else { return false };
                let type_s = self.interner.resolve(*type_id).to_string();
                production::enqueue_by_type(self, rules, command_owner, &type_s)
            }
            Command::SuspendProduction { category } => {
                production::suspend_production(self, command_owner, *category)
            }
            Command::CycleProducerFocus { category } => {
                let Some(rules) = rules else { return false };
                production::cycle_active_producer_for_owner_category(
                    self,
                    rules,
                    command_owner,
                    *category,
                )
            }
            Command::PlaceReadyBuilding { type_id, rx, ry } => {
                let Some(rules) = rules else { return false };
                let type_s = self.interner.resolve(*type_id).to_string();
                let placed = production::place_production_with_overlays(
                    self,
                    rules,
                    command_owner,
                    production::ProductionPlacement::Building {
                        type_id: &type_s,
                        cell: (*rx, *ry),
                    },
                    overlay_registry,
                );
                if !placed {
                    // `HouseClass::Place_Production 0x004FB369..0x004FB377`:
                    // a placement event whose Unlimbo fails speaks
                    // `EVA_CannotDeployHere` when `this == PlayerPtr`; the
                    // app applies the local-owner half.
                    if let Some(owner) = self.interner.get(command_owner) {
                        self.sound_events
                            .push(SimSoundEvent::CannotDeployHere { owner });
                    }
                }
                placed
            }
            Command::PlaceProducedMobile { category } => {
                let Some(rules) = rules else {
                    return false;
                };
                production::place_production_with_overlays(
                    self,
                    rules,
                    command_owner,
                    production::ProductionPlacement::Mobile {
                        category: *category,
                    },
                    overlay_registry,
                )
            }
            Command::CancelProductionByType { type_id, all } => {
                let Some(rules) = rules else { return false };
                let type_s = self.interner.resolve(*type_id).to_string();
                production::cancel_by_type_for_owner(self, rules, command_owner, &type_s, *all)
            }
            // The SELL event (`EventClass::Execute 0x004C6F20`): the target's
            // owner must be the event's house (`0x004C6F45`); a building
            // takes `Sell_Back(-1)` (`0x004C6FA2`).
            Command::SellBuilding { entity_id } => {
                let Some(rules) = rules else { return false };
                if !self.entity_owned_by_id(command_owner, *entity_id) {
                    return false;
                }
                production::sell_back(
                    self,
                    rules,
                    *entity_id,
                    production::SellOrder::Player,
                    overlay_registry,
                )
            }
            Command::SellWallAtCell { x, y } => {
                let (Some(rules), Some(overlays)) = (rules, overlay_registry) else {
                    return false;
                };
                self.sell_wall_at_cell(command_owner, *x, *y, rules, overlays)
            }
            // Offline game-speed transitions are consumed at master-frame
            // ingress so early authoritative animation work sees the new rate.
            // Reaching the ordinary EventClass-shaped tail must not apply one.
            Command::SetGameSpeed { .. } => false,
            Command::ExitMatch => {
                let Some(owner) = self.interner.get(command_owner) else {
                    return false;
                };
                if !self.houses.contains_key(&owner) {
                    return false;
                }
                // EventClass__Execute @ 0x004C6CB0, opcode 0x13: the due
                // EXIT event writes the termination byte at 0x004C7917. The
                // app consumes the owner-tagged edge after this tail dispatch.
                if !self.quit_requested {
                    self.executed_exit_owner = Some(owner);
                }
                self.quit_requested = true;
                true
            }
            // `EventClass::Execute 0x004C6ED2..0x004C6F01`: a live target
            // (`+0x90`) toggles (`ToggleRepair(-1)`).
            Command::ToggleRepair { entity_id } => {
                if !self.entity_owned_by_id(command_owner, *entity_id)
                    || !self
                        .substrate
                        .entities
                        .get(*entity_id)
                        .is_some_and(|entity| entity.lifecycle.object_alive)
                {
                    return false;
                }
                rules.is_some_and(|rules| {
                    production::toggle_repair(
                        self,
                        rules,
                        *entity_id,
                        production::RepairControl::Toggle,
                    )
                })
            }
            Command::MinerReturn {
                entity_id,
                target_refinery_id,
            } => {
                if !self.entity_owned_by_id(command_owner, *entity_id) {
                    return false;
                }
                if self
                    .substrate
                    .entities
                    .get(*entity_id)
                    .is_some_and(|e| e.is_deployed())
                {
                    return false;
                }
                let explicit_refinery = match target_refinery_id {
                    Some(refinery_id) => {
                        let Some(rules) = rules else { return false };
                        if !self.valid_explicit_miner_refinery(
                            command_owner,
                            *entity_id,
                            *refinery_id,
                            rules,
                        ) {
                            return false;
                        }
                        Some(*refinery_id)
                    }
                    None => None,
                };
                // A harvester's dock is its native Enter/Unload missions: the
                // order's MEGAMISSION radio break (`0x004C72E8`) leaves the
                // refinery link, and because the Harvest assign below replaces
                // Unload without its contact gate (`0x0073DEE0`), the unload
                // latch drops here too — as for `Command::HarvestCell`.
                if !self.order_actor_admits(*entity_id)
                    || self
                        .substrate
                        .entities
                        .get(*entity_id)
                        .is_none_or(|e| e.miner.is_none())
                {
                    return false;
                }
                self.begin_megamission_retask(*entity_id, MissionType::Enter, rules);
                if self
                    .substrate
                    .entities
                    .get(*entity_id)
                    .is_some_and(crate::sim::game_entity::GameEntity::is_harvester)
                {
                    crate::sim::miner::clear_unload_latch(self, *entity_id);
                }
                let previous_refinery = self
                    .substrate
                    .entities
                    .get(*entity_id)
                    .and_then(|e| e.miner.as_ref())
                    .and_then(|m| m.reserved_refinery);
                let explicit_refinery_changed = explicit_refinery
                    .is_some_and(|refinery_id| previous_refinery != Some(refinery_id));
                if explicit_refinery_changed {
                    if let Some(old_refinery) = previous_refinery {
                        // BREAK both ends: the old refinery's slot frees for
                        // the next miner, and this miner loses the contact
                        // that would let it path through that refinery.
                        crate::sim::miner::miner_dock::break_contact(
                            self,
                            *entity_id,
                            old_refinery,
                        );
                    }
                }
                // Update miner state in EntityStore.
                let Some(e) = self.substrate.entities.get_mut(*entity_id) else {
                    return false;
                };
                let Some(ref mut miner) = e.miner else {
                    return false;
                };
                if let Some(refinery_id) = explicit_refinery {
                    miner.reserved_refinery = Some(refinery_id);
                }
                // Clear any in-progress movement — the miner system will path to refinery.
                e.movement_target = None;
                // Commit the Harvest mission and the ForcedReturn cursor of
                // record. Assign resets the handler state and dispatch timer
                // (prompt redispatch); the cursor write lands after it.
                // UNCHECKED: the native return-order mission shape is
                // unverified — this preserves the legacy immediate-effect
                // command behavior.
                let now = self.session.binary_frame;
                let _ = self.mission_assign_exact(
                    *entity_id,
                    crate::sim::mission::MissionId::from_known(MissionType::Harvest),
                    now,
                );
                if let Some(e) = self.substrate.entities.get_mut(*entity_id) {
                    e.mission
                        .set_handler_state(crate::sim::miner::MinerState::ForcedReturn.cursor());
                }
                true
            }
            Command::RepairAtDepot {
                entity_id,
                depot_id,
            } => {
                let Some(rules) = rules else { return false };
                if !self.entity_owned_by_id(command_owner, *entity_id) {
                    return false;
                }
                if self
                    .substrate
                    .entities
                    .get(*entity_id)
                    .is_some_and(|e| e.is_deployed())
                {
                    return false;
                }
                // Validate depot exists, is friendly, and has UnitRepair=yes.
                let depot_ok = self.substrate.entities.get(*depot_id).is_some_and(|depot| {
                    command_owner.eq_ignore_ascii_case(self.interner.resolve(depot.owner()))
                        && self
                            .object_type(depot.type_ref(), rules)
                            .is_some_and(|obj| obj.unit_repair)
                });
                if !depot_ok {
                    return false;
                }
                // A damaged vehicle: the player's click issues the order only
                // for Units (`input::context_order`), and the depot's CAN_LOAD
                // refuses any sender but a Unit or an Aircraft (Building
                // Receive_Radio 0x0F, UnitRepair arm).
                let entity_ok = self.substrate.entities.get(*entity_id).is_some_and(|e| {
                    e.category == crate::map::entities::EntityCategory::Unit
                        && self
                            .object_type(e.type_ref(), rules)
                            .is_some_and(|object| e.health.current < object.strength)
                        && !e.dying
                });
                if !entity_ok {
                    return false;
                }
                // Native order admission (actor + Destination token).
                if !self.order_actor_admits(*entity_id)
                    || !self.order_object_token_admits(*depot_id)
                {
                    return false;
                }
                // Duplicate Enter onto the building this unit is already linked
                // to is consumed without touching anything.
                if self.duplicate_enter_is_noop(*entity_id, *depot_id) {
                    return true;
                }
                // Queue Enter; the Unit setter below owns depot admission and
                // contact changes, without a parallel reservation teardown.
                self.queue_megamission(*entity_id, MissionType::Enter, Some(rules));
                // Event4C7467 dispatches the class target setter before Dest.
                let _ = self.assign_target_represented(*entity_id, None, Some(rules));
                if let Some(e) = self.substrate.entities.get_mut(*entity_id) {
                    e.order_intent = None;
                }
                // Event4C747C: the Unit class setter with the depot.
                self.set_unit_destination(
                    *entity_id,
                    crate::sim::components::NavTargetRef::Building { id: *depot_id },
                    rules,
                    true,
                );
                true
            }
            Command::EnterTransport {
                passenger_id,
                transport_id,
            } => {
                let Some(rules) = rules else { return false };
                if !self.entity_owned_by_id(command_owner, *passenger_id) {
                    return false;
                }
                if self
                    .substrate
                    .entities
                    .get(*passenger_id)
                    .is_some_and(|e| e.is_deployed())
                {
                    return false;
                }
                // Validate transport exists and has cargo capacity.
                let transport_info = self.substrate.entities.get(*transport_id).and_then(|t| {
                    let obj = self.object_type(t.type_ref(), rules)?;
                    let cargo = t.passenger_role.cargo()?;
                    Some((t.position.rx, t.position.ry, obj.clone(), cargo.clone()))
                });
                let Some((trx, try_, transport_obj, cargo)) = transport_info else {
                    return false;
                };
                // Validate passenger can enter.
                let pax_ok = self.substrate.entities.get(*passenger_id).and_then(|p| {
                    let pobj = self.object_type(p.type_ref(), rules)?;
                    if passenger::can_enter_transport(
                        p,
                        self.substrate.entities.get(*transport_id)?,
                        pobj,
                        &transport_obj,
                        &cargo,
                        rules,
                        &self.houses,
                        self.path_grid(),
                    ) {
                        Some(())
                    } else {
                        None
                    }
                });
                if pax_ok.is_none() {
                    return false;
                }
                // Native order admission (actor + Destination token).
                if !self.order_actor_admits(*passenger_id)
                    || !self.order_object_token_admits(*transport_id)
                {
                    return false;
                }
                // Duplicate Enter onto a *building* transport (garrison,
                // Grinder, bunker) the passenger is already linked to is
                // consumed without touching anything — no fresh Boarding role
                // and no re-path, so the unit walks in instead of backing off.
                // Vehicle transports never take this branch.
                if self.duplicate_enter_is_noop(*passenger_id, *transport_id) {
                    return true;
                }
                let transport_ref = crate::sim::components::NavTargetRef::object(*transport_id);
                let object_destination =
                    if self.infantry_setter_receiver(*passenger_id, transport_ref, rules) {
                        // Infantry51F190 -> Foot4D76F8 ->6FFBE0 carries the
                        // clicked object as Destination for Enter7. Event
                        //4C747C preserves it; Infantry51AA40 samples its live
                        //+4C through the shared coordinate owner after its
                        //prelude (foot_navigation_coordinate corpus).
                        let Ok(coord) = crate::sim::movement::nav_target_coordinate(
                            transport_ref,
                            Some(*passenger_id),
                            &self.substrate.entities,
                            self.resolved_terrain.as_ref(),
                            Some((rules, &self.interner)),
                        ) else {
                            return false;
                        };
                        Some((transport_ref, coord))
                    } else {
                        // Other receiver classes retain their existing Cell
                        // adapter pending their object-click/setter migration.
                        None
                    };
                // Retask onto Enter; the target setter below owns combat cancellation.
                self.queue_megamission(*passenger_id, MissionType::Enter, Some(rules));
                // Clear existing state on the passenger.
                // Event4C7467 dispatches the class target setter before Dest.
                let _ = self.assign_target_represented(*passenger_id, None, Some(rules));
                if let Some(e) = self.substrate.entities.get_mut(*passenger_id) {
                    e.order_intent = None;
                    e.passenger_role = passenger::PassengerRole::Boarding {
                        target_transport_id: *transport_id,
                    };
                }
                // The represented Infantry class keeps the transport token;
                // the route adapter still uses its physical approach cell.
                let info = self.resolve_move_info(*passenger_id, Some(rules));
                let speed = info
                    .as_ref()
                    .map(|i| i.speed)
                    .unwrap_or(ra2_speed_to_leptons_per_second(4));
                let speed_type = info
                    .as_ref()
                    .map(|i| i.speed_type)
                    .unwrap_or(SpeedType::Track);
                self.issue_ground_move(
                    GroundMove {
                        entity_id: *passenger_id,
                        target: (trx, try_),
                        speed,
                        queue: false,
                        speed_type: Some(speed_type),
                        owner_blocks: true,
                        object_destination,
                    },
                    Some(rules),
                    overlay_registry,
                );
                true
            }
            Command::UnloadPassengers { transport_id } => {
                if !self.entity_owned_by_id(command_owner, *transport_id) {
                    return false;
                }
                let has_passengers = self
                    .substrate
                    .entities
                    .get(*transport_id)
                    .and_then(|t| t.passenger_role.cargo())
                    .is_some_and(|c| !c.is_empty());
                if !has_passengers {
                    return false;
                }
                let category = self
                    .substrate
                    .entities
                    .get(*transport_id)
                    .map(|t| t.category);
                match category {
                    Some(crate::map::entities::EntityCategory::Structure) => {
                        // A garrison's Unload is its own mission
                        // (`BuildingClass::Mission_Unload @ 0x0044D880`),
                        // commenced at the building's ready checks.
                        if !self.order_actor_admits(*transport_id) {
                            return false;
                        }
                        self.queue_megamission(*transport_id, MissionType::Unload, rules);
                        true
                    }
                    Some(crate::map::entities::EntityCategory::Unit)
                    | Some(crate::map::entities::EntityCategory::Aircraft) => {
                        // Vehicle/aircraft transports unload through the Unload
                        // mission (`UnitClass::Mission_Unload @ 0x0073D630`
                        // transport branch; Aircraft slot `0x004151E0`).
                        let Some(rules) = rules else { return false };
                        let is_transport = self
                            .substrate
                            .entities
                            .get(*transport_id)
                            .and_then(|t| self.object_type(t.type_ref(), rules))
                            .is_some_and(|obj| obj.passengers > 0);
                        if !is_transport || !self.order_actor_admits(*transport_id) {
                            return false;
                        }
                        if category == Some(crate::map::entities::EntityCategory::Aircraft) {
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
                            let now = self.session.binary_frame;
                            let _ = self.mission_assign_exact(
                                *transport_id,
                                crate::sim::mission::MissionId::from_known(MissionType::Unload),
                                now,
                            );
                        } else {
                            self.order_unload(*transport_id, rules, overlay_registry);
                        }
                        if let Some(e) = self.substrate.entities.get_mut(*transport_id) {
                            e.order_intent = None;
                        }
                        true
                    }
                    _ => false,
                }
            }
            Command::HarvestCell {
                entity_id,
                target_rx,
                target_ry,
            } => {
                if !self.entity_owned_by_id(command_owner, *entity_id) {
                    return false;
                }
                if self
                    .substrate
                    .entities
                    .get(*entity_id)
                    .is_some_and(|e| e.is_deployed())
                {
                    return false;
                }
                if self
                    .substrate
                    .entities
                    .get(*entity_id)
                    .is_none_or(|e| e.miner.is_none())
                {
                    return false;
                }
                if !self.order_actor_admits(*entity_id) {
                    return false;
                }
                // Shared Event prefix ends the refinery handshake. This
                // legacy Harvest mission path still owns the unload latch.
                self.begin_megamission_retask(*entity_id, MissionType::Harvest, rules);
                crate::sim::miner::clear_unload_latch(self, *entity_id);
                // Native (EventClass::Execute MEGAMISSION, disassembled
                // 2026-09-05/-25): the client's mission byte passes through
                // `FootClass 0x004DF0E0` (vtable `+0x4A4`, unchanged unless
                // 0x1D) into `Queue_Mission(mission, 0)` at `0x004C73B9`,
                // clears SuspendedNavCom/SuspendedTarCom, clears the
                // ArchiveTarget (`0x004C7448`) and hands the clicked cell to
                // the class setter (`vt+0x480(cell, 1)`, `0x004C747C`).
                // Queue_Mission (`0x005B35E0`) leaves a miner already on
                // Harvest alone (`0x005B3601..0x005B3612`): its state, stage
                // and Unit+0x6D2 carry on, so state 1 waits out the drive as
                // a hop and cuts where it ends. Any other mission queues
                // Harvest for the host's next Ready/Commence, and state 0
                // cuts where the drive ends. VERA's ForcedReturn cursor
                // stands for the return order's Enter mission, which Queue
                // replaces, so that miner restarts at state 0 here.
                // RESIDUAL: `0x004DA1C0` after the archive clear and the
                // Assign_Target of the event's target are not represented.
                let now = self.session.binary_frame;
                let harvest = crate::sim::mission::MissionId::from_known(MissionType::Harvest);
                let forced_return = self.substrate.entities.get(*entity_id).is_some_and(|e| {
                    e.miner_state() == Some(crate::sim::miner::MinerState::ForcedReturn)
                });
                if forced_return {
                    let _ = self.mission_assign_exact(*entity_id, harvest, now);
                } else {
                    let _ = self.mission_queue_exact(
                        *entity_id,
                        harvest,
                        0,
                        now,
                        &crate::sim::mission::authority::EntityReadyInputProvider,
                    );
                }
                // `0x004C73E1..0x004C73EA`, before the archive clear and the
                // setter: the order takes a Slave Miner off its hunt; the
                // Harvest it queued then sends it to the clicked field
                // (HandleReturnedSlaves, `sim::slave_manager`).
                if let Some(rules) = rules {
                    self.reset_slave_manager(*entity_id, rules);
                }
                if let Some(e) = self.substrate.entities.get_mut(*entity_id) {
                    e.set_archive_target(None);
                    // Clear in-progress movement so the miner re-paths.
                    e.movement_target = None;
                }
                if let Some(rules) = rules {
                    let _ = crate::sim::miner::miner_system::issue_stock_miner_drive_move(
                        self,
                        rules,
                        *entity_id,
                        (*target_rx, *target_ry),
                        overlay_registry,
                    );
                }
                true
            }
            Command::PlantC4 {
                attacker_id,
                target_building_id,
            } => {
                let Some(rules) = rules else { return false };
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
                // Validate target is a CanC4, non-invisible enemy building, not iron-curtained.
                // TODO(parity): also reject selling-in-progress buildings (Mission==0x13);
                // requires building Mission state which isn't modeled yet.
                let target_info = self
                    .substrate
                    .entities
                    .get(*target_building_id)
                    .and_then(|b| {
                        if b.category != crate::map::entities::EntityCategory::Structure {
                            return None;
                        }
                        if b.dying {
                            return None;
                        }
                        let obj = self.object_type(b.type_ref(), rules)?;
                        if !obj.can_c4 || obj.invisible_in_game {
                            return None;
                        }
                        if crate::sim::superweapon::invulnerability::is_invulnerable(
                            b.invulnerability.as_ref(),
                            self.session.binary_frame,
                        ) {
                            return None;
                        }
                        Some((
                            b.position.rx,
                            b.position.ry,
                            b.owner(),
                            crate::sim::movement::nav_target_coordinate(
                                crate::sim::components::NavTargetRef::Building {
                                    id: *target_building_id,
                                },
                                Some(*attacker_id),
                                &self.substrate.entities,
                                self.resolved_terrain.as_ref(),
                                Some((rules, &self.interner)),
                            )
                            .ok()?,
                        ))
                    });
                let Some((trx, try_, target_owner, target_coord)) = target_info else {
                    return false;
                };
                // Enemy-only.
                if crate::map::houses::are_houses_friendly(
                    &self.house_alliances,
                    command_owner,
                    self.interner.resolve(target_owner),
                ) {
                    return false;
                }
                // Native order admission (actor + Destination token).
                if !self.order_actor_admits(*attacker_id)
                    || !self.order_object_token_admits(*target_building_id)
                {
                    return false;
                }
                // Retask onto Sabotage; the target setter below owns combat cancellation.
                self.queue_megamission(*attacker_id, MissionType::Sabotage, Some(rules));
                // Clear conflicting state and set c4_plant.
                // Event4C7467 dispatches the class target setter before Dest.
                let _ = self.assign_target_represented(*attacker_id, None, Some(rules));
                if let Some(e) = self.substrate.entities.get_mut(*attacker_id) {
                    e.order_intent = None;
                    e.c4_plant = Some(crate::sim::components::C4PlantState {
                        target_building_id: *target_building_id,
                    });
                }
                // Event4C747C: Set_Destination(target building, 1). The building is
                // the NavCom, so the Infantry +1AC admits its footprint cells to a
                // Sabotage C4 carrier (`0x0051C2D3..`) and the walk ends inside.
                let info = self.resolve_move_info(*attacker_id, Some(rules));
                let speed = info
                    .as_ref()
                    .map(|i| i.speed)
                    .unwrap_or(ra2_speed_to_leptons_per_second(4));
                let speed_type = info
                    .as_ref()
                    .map(|i| i.speed_type)
                    .unwrap_or(crate::rules::locomotor_type::SpeedType::Foot);
                self.issue_ground_move(
                    GroundMove {
                        entity_id: *attacker_id,
                        target: (trx, try_),
                        speed,
                        queue: false,
                        speed_type: Some(speed_type),
                        owner_blocks: true,
                        object_destination: Some((
                            crate::sim::components::NavTargetRef::Building {
                                id: *target_building_id,
                            },
                            target_coord,
                        )),
                    },
                    Some(rules),
                    overlay_registry,
                );
                true
            }
            Command::CaptureBuilding {
                engineer_id,
                target_building_id,
            } => {
                let Some(rules) = rules else { return false };
                if !self.entity_owned_by_id(command_owner, *engineer_id) {
                    return false;
                }
                // Validate target is a capturable enemy building.
                let target_info = self
                    .substrate
                    .entities
                    .get(*target_building_id)
                    .and_then(|b| {
                        if b.category != crate::map::entities::EntityCategory::Structure {
                            return None;
                        }
                        if !b.lifecycle.object_alive
                            || b.health.current <= 0
                            || b.lifecycle.in_limbo
                        {
                            return None;
                        }
                        Some((
                            b.position.rx,
                            b.position.ry,
                            crate::sim::movement::nav_target_coordinate(
                                crate::sim::components::NavTargetRef::Building {
                                    id: *target_building_id,
                                },
                                Some(*engineer_id),
                                &self.substrate.entities,
                                self.resolved_terrain.as_ref(),
                                Some((rules, &self.interner)),
                            )
                            .ok()?,
                        ))
                    });
                let Some((trx, try_, target_coord)) = target_info else {
                    return false;
                };
                // Infantry51F190 maps actions28/29 toFootaction9 before
                // delayed Event delivery. Mutable type, relation and HP
                // action predicates are re-evaluated only at actual arrival.
                // Native order admission (actor + Destination token).
                if !self.order_actor_admits(*engineer_id)
                    || !self.order_object_token_admits(*target_building_id)
                {
                    return false;
                }
                // Retask onto Capture; the target setter below owns combat cancellation.
                self.queue_megamission(*engineer_id, MissionType::Capture, Some(rules));
                // Clear conflicting state and set capture target.
                // Event4C7467 dispatches the class target setter before Dest.
                let _ = self.assign_target_represented(*engineer_id, None, Some(rules));
                if let Some(e) = self.substrate.entities.get_mut(*engineer_id) {
                    e.order_intent = None;
                }
                // Event4C747C dispatches Infantry51AA40 with the Building
                // token. The class publishes NavCom and the Walk destination
                // after its prelude; a caller write would change its moving
                // and same-reference tests before the actual class call.
                let info = self.resolve_move_info(*engineer_id, Some(rules));
                let speed = info
                    .as_ref()
                    .map(|i| i.speed)
                    .unwrap_or(ra2_speed_to_leptons_per_second(4));
                let speed_type = info
                    .as_ref()
                    .map(|i| i.speed_type)
                    .unwrap_or(crate::rules::locomotor_type::SpeedType::Foot);
                self.issue_ground_move(
                    GroundMove {
                        entity_id: *engineer_id,
                        target: (trx, try_),
                        speed,
                        queue: false,
                        speed_type: Some(speed_type),
                        owner_blocks: true,
                        object_destination: Some((
                            crate::sim::components::NavTargetRef::Building {
                                id: *target_building_id,
                            },
                            target_coord,
                        )),
                    },
                    Some(rules),
                    overlay_registry,
                );
                true
            }
            Command::LaunchSuperWeapon {
                sw_type_id,
                target_rx,
                target_ry,
            } => {
                let Some(rules) = rules else { return false };
                // The SPECIAL_PLACE event (`EventClass::Execute 0x004C78D6`).
                let owner = self.interner.intern(command_owner);
                self.fire_super_weapon(
                    rules,
                    owner,
                    *sw_type_id,
                    (*target_rx, *target_ry),
                    overlay_registry,
                )
            }
            Command::EnterBunker { unit_id, bunker_id } => {
                let Some(rules) = rules else { return false };
                if !self.entity_owned_by_id(command_owner, *unit_id) {
                    return false;
                }
                if self
                    .substrate
                    .entities
                    .get(*unit_id)
                    .is_some_and(|e| e.is_deployed())
                {
                    return false;
                }
                // Target must be an own tank bunker (seeded `bunker_runtime`).
                let is_bunker = self
                    .substrate
                    .entities
                    .get(*bunker_id)
                    .is_some_and(|b| b.bunker_runtime.is_some());
                if !is_bunker || !self.entity_owned_by_id(command_owner, *bunker_id) {
                    return false;
                }
                // Native order admission (actor + Destination token), ahead of
                // any radio traffic.
                if !self.order_actor_admits(*unit_id) || !self.order_object_token_admits(*bunker_id)
                {
                    return false;
                }
                // Duplicate Enter onto the bunker this unit is already linked to
                // is consumed without touching anything — no CanEnter/DockNow
                // round trip and no re-approach.
                if self.duplicate_enter_is_noop(*unit_id, *bunker_id) {
                    return true;
                }
                // Rules-gated weapon/Bunkerable check (the bus stays rules-free).
                if !crate::sim::docking::bunker_link::can_auto_deploy_here(self, *unit_id, rules) {
                    return false;
                }
                // Admission query over the bus; commit only on ROGER.
                if crate::sim::radio::transmit(
                    self,
                    *unit_id,
                    *bunker_id,
                    crate::sim::radio::RadioMessage::CanEnter,
                    crate::sim::radio::RadioPayload::default(),
                    None,
                ) != crate::sim::radio::RadioResponse::Roger
                {
                    return false;
                }
                // Commit: start the install machine (ArriveWait + installing_unit).
                crate::sim::radio::transmit(
                    self,
                    *unit_id,
                    *bunker_id,
                    crate::sim::radio::RadioMessage::DockNow,
                    crate::sim::radio::RadioPayload::default(),
                    None,
                );
                // Retask onto Enter (no dock reservation), mark the unit as
                // approaching THIS bunker (the install machine's keep-alive gate).
                self.queue_megamission(*unit_id, MissionType::Enter, Some(rules));
                // Event4C7467 dispatches the class target setter before Dest.
                let _ = self.assign_target_represented(*unit_id, None, Some(rules));
                if let Some(e) = self.substrate.entities.get_mut(*unit_id) {
                    e.order_intent = None;
                    e.c4_plant = None;
                    e.bunker_link = crate::sim::game_entity::BunkerLink::Approaching(*bunker_id);
                }
                // Issue an approach move toward the bunker cell (mirror EnterTransport).
                let bunker_cell = self
                    .substrate
                    .entities
                    .get(*bunker_id)
                    .map(|b| (b.position.rx, b.position.ry));
                if let Some((brx, bry)) = bunker_cell {
                    let info = self.resolve_move_info(*unit_id, Some(rules));
                    let speed = info
                        .as_ref()
                        .map(|i| i.speed)
                        .unwrap_or(ra2_speed_to_leptons_per_second(4));
                    let speed_type = info
                        .as_ref()
                        .map(|i| i.speed_type)
                        .unwrap_or(SpeedType::Track);
                    self.issue_ground_move(
                        GroundMove {
                            entity_id: *unit_id,
                            target: (brx, bry),
                            speed,
                            queue: false,
                            speed_type: Some(speed_type),
                            owner_blocks: true,
                            object_destination: None,
                        },
                        Some(rules),
                        overlay_registry,
                    );
                }
                true
            }
            Command::EjectBunker { bunker_id } => {
                let Some(rules) = rules else { return false };
                if !self.entity_owned_by_id(command_owner, *bunker_id) {
                    return false;
                }
                let has_occupant = self
                    .substrate
                    .entities
                    .get(*bunker_id)
                    .is_some_and(|b| b.bunker_occupant.is_some());
                if !has_occupant {
                    return false;
                }
                crate::sim::docking::bunker_link::release_normal(self, *bunker_id, rules);
                true
            }
        }
    }

    /// The rally click's event 0x1E for each selected factory:
    /// `BuildingClass::SetRallyPoint @ 0x00443860` queues it and
    /// `EventClass::Execute @ 0x004C6DAA` runs `Set_ArchiveTarget @
    /// 0x0070C610`, so the factory archives the cell (its only store).
    ///
    /// The input owner `factory_rally_cell_input` has already run native
    /// Map56DC20's nearby-passable search separately for each producer. This
    /// synchronized event stores that prepared target without searching again.
    fn set_rally_point_for_producers(
        &mut self,
        command_owner: &str,
        producer_ids: &[u64],
        rx: u16,
        ry: u16,
        rules: Option<&RuleSet>,
    ) -> bool {
        let Some(rules) = rules else {
            return true;
        };
        let mut ids = producer_ids.to_vec();
        ids.sort_unstable();
        ids.dedup();
        for stable_id in ids {
            if self.takes_rally_point(stable_id, command_owner, rules) {
                if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                    entity.set_archive_target(Some(crate::sim::combat::TargetKind::Cell(rx, ry)));
                }
            }
        }
        // `EVA_NewRallyPointEstablished` is not a sim fact: `BuildingClass::
        // SetRallyPoint 0x00443A69` speaks inside the click handler, after the
        // rally EventClass is pushed and before it executes — the app owns it.
        true
    }

    /// Whether a rally order from `owner` reaches this building: `owner`'s
    /// structure whose type has a rally point. gamemd's cell click
    /// (`BuildingClass::Active_Click_With 0x004436F0`) calls `SetRallyPoint`
    /// only for a building with `HasRallyPoint` (vt+0x284, `0x00443792`) or
    /// an `UndeploysInto=` type (`0x0044377E`); the undeploy arm's
    /// repack-and-move order is not ported. The click and the event share
    /// this rule so no building the event ignores is announced.
    pub(crate) fn takes_rally_point(&self, stable_id: u64, owner: &str, rules: &RuleSet) -> bool {
        self.substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                entity.category == crate::map::entities::EntityCategory::Structure
                    && owner.eq_ignore_ascii_case(self.interner.resolve(entity.owner()))
                    && self
                        .object_type(entity.type_ref(), rules)
                        .is_some_and(|obj| obj.has_rally_line())
            })
    }

    /// Replace the current selection with exactly the given stable entity IDs.
    ///
    /// Mirrors gamemd's mutation flow: omitted old members are deselected,
    /// requested old members retain their existing admission, and only genuinely
    /// new members run through `ObjectClass::Select`. This distinction matters
    /// for an already-selected Chrono unit in warp-out: warp blocks a fresh
    /// selection, but does not retroactively remove an existing one.
    fn apply_selection_snapshot(&mut self, stable_ids: &[u64], rules: Option<&RuleSet>) -> bool {
        let requested: BTreeSet<u64> = stable_ids.iter().copied().collect();
        // Deselect only omitted old members. Requested old members remain set,
        // so the final-admission gates below are never reapplied to them.
        let keys: Vec<u64> = self.substrate.entities.keys_sorted();
        for &id in &keys {
            if !requested.contains(&id)
                && let Some(e) = self.substrate.entities.get_mut(id)
            {
                e.selected = false;
            }
        }
        // Iterate the original payload, not the membership set: source order is
        // authoritative even though this sim layer stores only selected bits.
        for &stable_id in stable_ids {
            self.try_select_object(stable_id, rules);
        }
        true
    }

    /// Object5F6C30 (+138), with Foot4DFA50's +6AD guard for mobile objects.
    /// Native +D0(1) is the disguise House: Unit7465F0 may return NULL;
    /// Infantry5226C0 defaults it to the current House. Reuse the existing
    /// +C8 disguise owner instead of treating current fog as selectability.
    pub(crate) fn object_is_selectable(&self, stable_id: u64, rules: Option<&RuleSet>) -> bool {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return false;
        };
        if entity.category != crate::map::entities::EntityCategory::Structure
            && entity.foot_locomotor_swap_active
        {
            return false;
        }
        if entity.category == crate::map::entities::EntityCategory::Unit
            && entity
                .disguise
                .as_ref()
                .is_some_and(|d| d.house().is_none())
            && self.session.current_house.is_some_and(|house| {
                crate::sim::cloak_disguise::object_disguised_to(
                    entity,
                    house,
                    Some(&self.fog),
                    Some(&self.house_alliances),
                    &self.interner,
                )
            })
        {
            return false;
        }
        rules.is_none_or(|r| {
            r.object(self.interner.resolve(entity.type_ref()))
                .is_none_or(|object| object.selectable)
        })
    }

    /// Object5F4520's state/type admission, shared by provisional local input
    /// and committed selection. Membership and pending placement are caller
    /// state: input checks its newest ledger/placement, the sim its stored bit.
    /// Original does not read Alive or Health here; search/click callers do.
    pub(crate) fn can_select_object(&self, stable_id: u64, rules: Option<&RuleSet>) -> bool {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return false;
        };
        !entity.lifecycle.in_limbo
            && self.object_is_selectable(stable_id, rules)
            && !(entity.is_mission_only()
                && rules.is_some_and(|rules| self.techno_player_controllable(stable_id, rules)))
            && !entity.is_warped_out()
    }

    /// Commit one admitted Object5F4520 selection. Caller-specific owner gates
    /// remain with the command producer; enemy click-selection is legitimate.
    pub(crate) fn try_select_object(&mut self, stable_id: u64, rules: Option<&RuleSet>) -> bool {
        if !self.can_select_object(stable_id, rules)
            || self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|e| e.selected)
        {
            return false;
        }
        match self.substrate.entities.get_mut(stable_id) {
            Some(entity) => {
                entity.selected = true;
                true
            }
            None => false,
        }
    }

    /// Check ownership using stable_id via EntityStore.
    ///
    /// VERA-internal; the gamemd equivalent is that there ISN'T one. The
    /// MEGAMISSION arm loads `Houses[event.house]` into EDI at
    /// 0x004C6CBD-0x004C6CCA, then overwrites EDI with the resolved acting
    /// object at 0x004C71D2 and never compares the two — there is no house test
    /// anywhere in 0x004C71CA-0x004C74CA. So VERA is strict where retail is
    /// permissive. The two agree today because commands execute on the issuing
    /// tick and there is one local house; they would part company for a peer or
    /// replayed event naming an object whose owner changed — capture or mind
    /// control — between issue and execute, where retail still obeys the order
    /// and VERA drops it. Kept because dropping it would let a malformed or
    /// hostile envelope drive another house's units.
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
    /// * **Two arms still ungated.** `Simulation::command_uses_megamission`
    ///   already enumerates which VERA commands are MEGAMISSION-shaped — it is
    ///   what splits due commands into the non-MEGAMISSION pass and the staged
    ///   batch, mirroring opcode 0x04 in `net::lockstep`. Checked against it,
    ///   `EjectBunker` and `ToggleInfantryDeploy` carry only the ownership test.
    ///   Trigger: issuing
    ///   one of those to a dying or limboed actor. Player effect: the order runs
    ///   where retail abandons it. Frequency: low per order. These gates
    ///   remain outside ordinary Unit depot servicing.
    /// * **Duplicate-Enter is not applied to `MinerReturn`**, which is the
    ///   fourth Enter-shaped order (right-clicking your own refinery). Same
    ///   stall-and-re-approach the predicate exists to stop, on the one Enter
    ///   the player repeats most.
    /// * **Mission replacement remains partly per-site.** The shared
    ///   `begin_megamission_retask` owns radio, +500 and Team removal before
    ///   Queue4C73B9. The common funnel also clears suspended target2B8 and
    ///   Foot destination5A8; MinerReturn and HarvestCell bypass those
    ///   post-Queue archive clears. A later Restore can resume a cancelled
    ///   target or route. Their complete mission/DTO migration is separate.
    ///   Aircraft reservation teardown remains a legacy command policy.
    /// * **The manager abandon** at 0x004C73E1-0x004C73EA calls 0x006B0C80 on
    ///   `[actor+0x2D8]`, the SlaveManagerClass (not a spawn manager), whenever
    ///   the queued mission is not Attack; it is not Foot-gated. The funnel
    ///   below, the HarvestCell arm and Stop's own copy
    ///   (0x004C769C-0x004C76AC) run it (`Simulation::reset_slave_manager`);
    ///   the other orders outside the funnel do not yet.
    /// * **`TeamClass__Remove_Member`** runs in the MEGAMISSION funnel
    ///   and in its shared prefix; EjectBunker and the dormant aircraft Unload
    ///   arm bypass that prefix and keep a team member in its team.
    pub(crate) fn order_actor_admits(&self, stable_id: u64) -> bool {
        self.substrate.entities.get(stable_id).is_some_and(|e| {
            e.lifecycle.object_alive && e.health.current > 0 && !e.lifecycle.in_limbo
        })
    }

    /// The Target/Destination half of the native order-admission gate.
    ///
    /// When the order names an object, that object is subjected to the same
    /// three tests as the actor, and a failure abandons the whole order. This
    /// is what keeps an attacker on its previous order when the unit it was
    /// clicked onto dies in the same tick, instead of retasking it onto a
    /// corpse that is still resolvable in the store.
    pub(crate) fn order_object_token_admits(&self, stable_id: u64) -> bool {
        self.substrate.entities.get(stable_id).is_some_and(|e| {
            e.lifecycle.object_alive && e.health.current != 0 && !e.lifecycle.in_limbo
        })
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
    pub(crate) fn duplicate_enter_is_noop(&self, receiver_id: u64, destination_id: u64) -> bool {
        let Some(receiver) = self.substrate.entities.get(receiver_id) else {
            return false;
        };
        if receiver.category == crate::map::entities::EntityCategory::Structure {
            return false;
        }
        if receiver.mission.current()
            != crate::sim::mission::MissionId::from_known(MissionType::Enter)
        {
            return false;
        }
        let destination_is_building = self
            .substrate
            .entities
            .get(destination_id)
            .is_some_and(|d| d.category == crate::map::entities::EntityCategory::Structure);
        if !destination_is_building {
            return false;
        }
        receiver.radio_contacts.slot(0) == Some(destination_id)
    }

    /// Validate an explicit refinery selected by a player miner-return order.
    fn valid_explicit_miner_refinery(
        &self,
        command_owner: &str,
        miner_id: u64,
        refinery_id: u64,
        rules: &RuleSet,
    ) -> bool {
        let Some(miner) = self.substrate.entities.get(miner_id) else {
            return false;
        };
        if miner.miner.is_none() {
            return false;
        }
        let harvester_type = self.interner.resolve(miner.type_ref());
        let Some(refinery) = self.substrate.entities.get(refinery_id) else {
            return false;
        };
        if refinery.category != crate::map::entities::EntityCategory::Structure {
            return false;
        }
        if refinery.health.current == 0 || refinery.dying || refinery.building_up() {
            return false;
        }
        let refinery_owner = self.interner.resolve(refinery.owner());
        if !are_houses_friendly(&self.house_alliances, command_owner, refinery_owner) {
            return false;
        }
        let refinery_type = self.interner.resolve(refinery.type_ref());
        rules.is_refinery_type(refinery_type)
            && rules.harvester_can_dock_at(harvester_type, refinery_type)
    }

    /// Check whether the attacker can attack the target (i.e. they are not allies).
    /// Uses EntityStore for ownership lookup.
    fn can_attack_target_by_id(&self, attacker_id: u64, target_id: u64) -> bool {
        let Some(attacker) = self.substrate.entities.get(attacker_id) else {
            return false;
        };
        let Some(target) = self.substrate.entities.get(target_id) else {
            return false;
        };
        !are_houses_friendly(
            &self.house_alliances,
            self.interner.resolve(attacker.owner()),
            self.interner.resolve(target.owner()),
        )
    }

    /// Event4C73B9 queues AreaGuard, clears suspended references, then the
    /// Foot arm4C7409..4C7430 clears Target and assigns the first event token
    /// as destination and ArchiveTarget. The retained post belongs to the
    /// existing AreaGuard4D6AA0 handler, including acquisition and return.
    fn apply_guard_command(
        &mut self,
        command_owner: &str,
        entity_id: u64,
        target: Option<combat::TargetKind>,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        if !self.entity_owned_by_id(command_owner, entity_id) {
            return false;
        }
        // Guard reaches the same MEGAMISSION arm as every other order — it
        // branches on mission 0x0B (Area_Guard) at 0x004C73EF — so it is subject
        // to the same admission gate, and the gate runs BEFORE anything is
        // written. Retail's whole contract on this arm is that a failure
        // abandons the order and touches nothing.
        if !self.order_actor_admits(entity_id) {
            return false;
        }
        // TargetClass6E6E20 resolves an absent object to NULL. A present but
        // dead/limboed object rejects the entire event before retask writes;
        // friendship is not an event-admission test.
        let target = target.filter(|target| match *target {
            combat::TargetKind::Entity(id) => self.substrate.entities.contains(id),
            combat::TargetKind::Cell(..) => true,
        });
        if let Some(combat::TargetKind::Entity(id)) = target
            && !self.order_object_token_admits(id)
        {
            return false;
        }
        let foot =
            self.substrate.entities.get(entity_id).unwrap().category != EntityCategory::Structure;
        self.queue_megamission(entity_id, MissionType::AreaGuard, rules);
        self.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .order_intent = None;
        // Non-Foot receivers take the ordinary Event4C7467/4C747C arm;
        // G itself excludes them, but a decoded record still reaches it.
        self.assign_target_represented(entity_id, if foot { None } else { target }, rules)
            .unwrap_or_else(|cause| panic!("Area Guard order target: {cause}"));
        self.assign_destination_represented(
            entity_id,
            if foot { target.map(Into::into) } else { None },
            rules,
            overlay_registry,
        )
        .unwrap_or_else(|cause| panic!("Area Guard order destination: {cause}"));
        if foot {
            // The archive write follows the virtual destination call even
            // when that class setter refuses its request.
            self.substrate
                .entities
                .get_mut(entity_id)
                .unwrap()
                .set_archive_target(target);
        }
        true
    }

    /// The target an Attack order hands its object. A building takes it
    /// through BuildingClass::SetTarget (vt+0x3C8, `0x00443B90`), which
    /// refuses one it cannot reach; its queued Attack stands either way and
    /// Mission_Attack's null-target arm hands it back to Guard
    /// (`techno_ai::building_missions`). Every class uses the same concrete
    /// target authority; Infantry51B1F0 owns its action, DeployFire and path
    /// effects before the Techno base.
    fn order_attack_target(
        &mut self,
        attacker_id: u64,
        target: combat::TargetKind,
        rules: Option<&RuleSet>,
    ) -> bool {
        let Some(attacker) = self.substrate.entities.get(attacker_id) else {
            return false;
        };
        // The cell order's defensive refusal of an unarmed attacker
        // (What_Action_OnCell7008BD's current-weapon test).
        if matches!(target, combat::TargetKind::Cell(..))
            && !rules
                .and_then(|rules| rules.object(self.interner.resolve(attacker.type_ref())))
                .is_some_and(|obj| combat::combat_weapon::is_armed(attacker, obj))
        {
            return false;
        }
        // Event4C7467 calls virtual+3C8, before4C747C's null destination.
        // A class refusal still leaves the accepted queued mission in place.
        self.assign_target_represented(attacker_id, Some(target), rules)
            .is_ok()
    }

    /// The Unload order a self-click sends: `ClickedMission(Unload, NULL,
    /// NULL)` (FootClass slot +0x144 case 4, `0x004D74E0`) issues a
    /// MEGAMISSION. Its arm queues Unload, then for a Foot clears
    /// ArchiveTarget (`0x004C7448`) and runs the class target (`0x004C7467`)
    /// and destination (`0x004C747C`) setters with the event's NULL tokens,
    /// so a moving unit stops where it is and unloads there.
    ///
    /// RESIDUAL: the deploy key sends DEPLOY instead (`DeployCommandClass`
    /// `0x00730AF0` → `ClickedEvent(9)`), and VERA does not tell the two
    /// apart. That arm (`0x004C7762..0x004C7812`) also nulls the destination
    /// and target, but it refuses a current mission 0x12 or 0x13, an
    /// aircraft, a cell holding a `WeaponsFactory=` building (BuildingType
    /// +0x16BD, read at `0x00460A72`), and a unit off a bridge on a flat cell
    /// whose vt+0x2B0 answers true; it leaves ArchiveTarget and the suspended
    /// NavCom/TarCom alone. Trigger: the deploy key on a loaded transport or
    /// a deployer infantry in one of those states. Effect: VERA unloads or
    /// deploys where native ignores the key, for example a transport still
    /// in its war factory's cell. Frequency: rare. Risk: low.
    fn order_unload(
        &mut self,
        id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        self.queue_megamission(id, MissionType::Unload, Some(rules));
        // Foot4DA1C0 (`0x004C7453`) clears the +5AC vector, distinct from
        // NavQueue588/598, and has no represented producer here.
        self.assign_target_represented(id, None, Some(rules))
            .unwrap_or_else(|cause| panic!("Unload order target: {cause}"));
        self.assign_destination_represented(id, None, Some(rules), overlay_registry)
            .unwrap_or_else(|cause| panic!("Unload order destination: {cause}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::house_state::HouseState;
    use crate::sim::miner::{Miner, MinerConfig, MinerKind, MinerState};
    use crate::sim::mission::MissionId;
    use crate::sim::movement::locomotor::LocomotorState;

    fn amcv_move_rules() -> RuleSet {
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

    fn spawn_rule_backed_unit(sim: &mut Simulation, sid: u64, type_id: &str, rules: &RuleSet) {
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
            Health { current: health },
            type_ref,
            EntityCategory::Unit,
            0,
            obj.sight.max(0) as u16,
            true,
        );
        entity.locomotor = Some(LocomotorState::from_object_type(
            obj,
            sim.session.binary_frame,
        ));
        entity.regular_crusher = obj.crusher;
        entity.drive_accelerates = obj.accelerates;
        entity.omni_crusher = obj.omni_crusher;
        // A directly-inserted GameEntity keeps the constructed `in_limbo`
        // byte; production spawns clear it through Reveal. Order admission
        // reads that byte, so the fixture must model a revealed object.
        entity.lifecycle.in_limbo = false;
        sim.substrate.entities.insert(entity);
    }

    /// Event IDLE's null destination (`0x004C75ED`) is the Unit's class
    /// setter (`0x00741970`), which returns before any write without a NavCom
    /// unless its `+0x1F8` override is up (`0x00741A80`): Stop on an idle
    /// Unit keeps its queued waypoints and Foot's movement timer.
    #[test]
    fn stop_on_an_idle_unit_takes_the_unit_setter_and_writes_nothing() {
        let rules = amcv_move_rules();
        let mut sim = Simulation::new();
        sim.session.binary_frame = 100;
        spawn_rule_backed_unit(&mut sim, 1, "AMCV", &rules);
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.navigation.nav_queue = vec![crate::sim::components::NavTargetRef::cell(25, 20)];
        entity.navigation.path_runtime.start_movement(40, 30);
        assert!(entity.navigation.nav_com.is_none());
        assert!(sim.apply_command("Americans", &Command::Stop { entity_id: 1 }, Some(&rules)));
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(entity.navigation.nav_queue.len(), 1);
        let timer = entity.navigation.path_runtime.movement_timer;
        assert_eq!((timer.start_frame(), timer.duration()), (40, 30));
    }

    /// Original Event6 retains Foot+500; accepted MegaMission4C7353 clears
    /// it. Both break an untethered unit's old contact through the radio owner.
    /// Native controls: building_repair.depot_service.json,
    /// pending_entry_lifecycle, original admitted Event slices.
    #[test]
    fn depot_order_cancellation_follows_native_event_cleanup() {
        for (command, pending_survives) in [
            (Command::Stop { entity_id: 1 }, true),
            (
                Command::Guard {
                    entity_id: 1,
                    target: None,
                },
                false,
            ),
            (
                Command::Move {
                    entity_id: 1,
                    target_rx: 25,
                    target_ry: 20,
                    queue: false,
                },
                false,
            ),
            (
                Command::AttackMove {
                    entity_id: 1,
                    target_rx: 25,
                    target_ry: 20,
                    queue: false,
                },
                false,
            ),
        ] {
            let rules = amcv_move_rules();
            let mut sim = Simulation::new();
            let grid = PathGrid::new(40, 40);
            sim.install_fixture_path_grid(Some(&grid));
            spawn_rule_backed_unit(&mut sim, 1, "AMCV", &rules);
            spawn_structure_for_owner(&mut sim, 2, "AMCV", "Americans", 24, 20);
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .set_pending_entry(Some(2));
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .mark_live_contact_with(2);
            sim.substrate
                .entities
                .get_mut(2)
                .unwrap()
                .mark_live_contact_with(1);
            assert!(
                sim.apply_command("Americans", &command, Some(&rules)),
                "{command:?}"
            );
            let actor = sim.substrate.entities.get(1).unwrap();
            assert_eq!(
                actor.pending_entry(),
                pending_survives.then_some(2),
                "{command:?}"
            );
            assert!(
                actor.radio_contacts.is_empty(),
                "{command:?}: actor contact"
            );
            assert!(
                sim.substrate
                    .entities
                    .get(2)
                    .unwrap()
                    .radio_contacts
                    .is_empty(),
                "{command:?}: former depot contact"
            );
        }
    }

    fn gsi_16_01_insert_identity_entity(
        sim: &mut Simulation,
        stable_id: u64,
        owner: crate::sim::intern::InternedId,
    ) {
        let type_ref = sim.interner.intern("TESTUNIT");
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            stable_id,
            10,
            20,
            0,
            0,
            owner,
            Health { current: 100 },
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            false,
        );
        entity.lifecycle.in_limbo = false;
        sim.substrate.entities.insert(entity);
    }

    #[test]
    fn gsi_16_01_registered_house_move_roundtrips_without_a_semantic_sidecar() {
        let mut sim = Simulation::new();
        let local = sim.interner.intern("Local");
        let source_owner = sim.interner.intern("SourceOwner");
        sim.houses
            .insert(local, HouseState::new(local, 0, None, false, 0, 10));
        sim.houses.insert(
            source_owner,
            HouseState::new(source_owner, 1, None, false, 0, 10),
        );
        sim.session.house_order = vec![local, source_owner];
        sim.session.binary_frame = 77;
        gsi_16_01_insert_identity_entity(&mut sim, 42, source_owner);

        let record = sim
            .encode_megamission_record(
                local,
                &Command::Move {
                    entity_id: 42,
                    target_rx: 34,
                    target_ry: 12,
                    queue: false,
                },
            )
            .expect("registered issuer and source encode");
        assert_eq!(
            record.house_id(),
            0,
            "issuer is independent of source owner"
        );
        assert_eq!(record.frame_stamp(), 77);
        assert_eq!(
            sim.decode_native_command_record(&record, 900),
            Some(CommandEnvelope::new(
                local,
                900,
                Command::Move {
                    entity_id: 42,
                    target_rx: 34,
                    target_ry: 12,
                    queue: false,
                }
            ))
        );
    }

    #[test]
    fn gsi_16_01_move_admission_rejects_unregistered_issuer_and_identity_overflow() {
        let mut sim = Simulation::new();
        let local = sim.interner.intern("Local");
        let absent = sim.interner.intern("Absent");
        sim.houses
            .insert(local, HouseState::new(local, 0, None, false, 0, 10));
        sim.session.house_order = vec![local];
        gsi_16_01_insert_identity_entity(&mut sim, 42, local);
        assert_eq!(
            sim.encode_megamission_record(
                absent,
                &Command::Move {
                    entity_id: 42,
                    target_rx: 10,
                    target_ry: 20,
                    queue: false
                }
            ),
            None
        );

        let overflow_id = i32::MAX as u64 + 1;
        gsi_16_01_insert_identity_entity(&mut sim, overflow_id, local);
        assert_eq!(
            sim.encode_megamission_record(
                local,
                &Command::Move {
                    entity_id: overflow_id,
                    target_rx: 10,
                    target_ry: 20,
                    queue: false
                }
            ),
            None
        );
        assert_eq!(
            sim.encode_megamission_record(
                local,
                &Command::Move {
                    entity_id: 42,
                    target_rx: u16::MAX,
                    target_ry: 20,
                    queue: false
                }
            ),
            None,
            "native signed CellStruct coordinates must not be truncated"
        );
    }

    #[test]
    fn resolve_move_info_uses_stock_amcv_speed_without_deployable_multiplier() {
        let rules = amcv_move_rules();
        let mut sim = Simulation::new();
        spawn_rule_backed_unit(&mut sim, 1, "AMCV", &rules);

        let info = sim.resolve_move_info(1, Some(&rules)).expect("move info");

        assert_eq!(info.speed, ra2_speed_to_leptons_per_second(4));
    }

    #[test]
    fn resolve_move_info_carries_regular_crusher() {
        let rules = amcv_move_rules();
        let mut sim = Simulation::new();
        spawn_rule_backed_unit(&mut sim, 1, "AMCV", &rules);

        let info = sim.resolve_move_info(1, Some(&rules)).expect("move info");

        assert!(info.regular_crusher);
        assert!(!info.omni_crusher);
        assert_eq!(info.movement_zone, MovementZone::Normal);
        assert!(info.can_crush_units());
        assert_eq!(
            info.crush_capability(),
            movement::bump_crush::CrushCapability::new(true, false)
        );
    }

    #[test]
    fn resolve_move_info_carries_accelerates_flag() {
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
             Accelerates=false\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("amcv rules");
        let mut sim = Simulation::new();
        spawn_rule_backed_unit(&mut sim, 1, "AMCV", &rules);

        let info = sim.resolve_move_info(1, Some(&rules)).expect("move info");

        assert!(!info.drive_accelerates);
    }

    /// The Drive order accepts without a search (Unit741970); the first
    /// Process's search reads the Simulation's zone grid.
    #[test]
    fn player_drive_move_first_process_search_uses_the_zone_grid() {
        let rules = amcv_move_rules();
        let mut sim = Simulation::new();
        spawn_rule_backed_unit(&mut sim, 1, "AMCV", &rules);
        crate::sim::arena_fixture::supply_native_map(&mut sim);
        let grid = (*sim.path_grid_snapshot().unwrap()).clone();
        crate::sim::movement::reset_path_search_used_zone_grid_marker();

        let applied = sim.apply_command(
            "Americans",
            &Command::Move {
                entity_id: 1,
                target_rx: 25,
                target_ry: 20,
                queue: false,
            },
            Some(&rules),
        );

        assert!(applied);
        assert!(!crate::sim::movement::path_search_used_zone_grid_marker());
        sim.process_ground_locomotor_for_test(1, Some(&rules), Some(&grid), None)
            .expect("the first Process requests the route");
        assert!(crate::sim::movement::path_search_used_zone_grid_marker());
    }

    // ===== Order admission (GSI-07.01): liveness, strength and limbo =====

    /// A Move onto a live, revealed unit is the control case for the three
    /// admission tests below: it must be admitted.
    #[test]
    fn move_order_is_admitted_for_a_live_revealed_actor() {
        let rules = amcv_move_rules();
        let mut sim = Simulation::new();
        spawn_rule_backed_unit(&mut sim, 1, "AMCV", &rules);
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        sim.install_fixture_path_grid(Some(&grid));

        assert!(sim.order_actor_admits(1));
        assert!(sim.apply_command(
            "Americans",
            &Command::Move {
                entity_id: 1,
                target_rx: 25,
                target_ry: 20,
                queue: false,
            },
            Some(&rules),
        ));
        assert!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .movement_target
                .is_some()
        );
    }

    /// Event4C73B9 queues before the special raw-mission11 arm runs.
    #[test]
    fn area_guard_regression_player_order_queues_the_native_mission() {
        let rules = amcv_move_rules();
        let mut sim = Simulation::new();
        spawn_rule_backed_unit(&mut sim, 1, "AMCV", &rules);
        assert!(sim.apply_command(
            "Americans",
            &Command::Guard {
                entity_id: 1,
                target: None,
            },
            Some(&rules),
        ));
        // Event4C73B9 queues before the AreaGuard target/destination/archive
        // arm. The next paid Ready/Commence owns promotion to current.
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().mission.queued(),
            MissionId::from_known(MissionType::AreaGuard),
        );
    }

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
            actor.suspended_attack_target = Some(combat::TargetKind::Entity(2));
            actor.navigation.suspended_nav_com = Some(NavTargetRef::cell(12, 12));
            actor.set_archive_target(Some(combat::TargetKind::Cell(5, 5)));
            actor.order_intent = Some(OrderIntent::AttackMove {
                goal_rx: 12,
                goal_ry: 12,
            });

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
            assert!(actor.suspended_attack_target.is_none());
            assert!(actor.navigation.suspended_nav_com.is_none());
            assert!(actor.attack_target.is_none());
            assert!(actor.order_intent.is_none());
            assert_eq!(actor.archive_target(), post);
            assert_eq!(actor.navigation.nav_com, post.map(Into::into));
        }
    }

    #[test]
    fn guard_order_is_dropped_for_a_limboed_actor_without_touching_it() {
        let rules = amcv_move_rules();
        let mut sim = Simulation::new();
        spawn_rule_backed_unit(&mut sim, 1, "AMCV", &rules);
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .lifecycle
            .in_limbo = true;

        assert!(!sim.order_actor_admits(1));
        assert!(!sim.apply_command(
            "Americans",
            &Command::Guard {
                entity_id: 1,
                target: None,
            },
            Some(&rules),
        ));
        let actor = sim.substrate.entities.get(1).unwrap();
        assert!(
            actor.order_intent.is_none(),
            "a rejected guard order must not have written anything"
        );
        assert_eq!(actor.mission.queued(), MissionId::NONE);
    }

    /// A resolved but limboed post rejects the whole event, preserving the
    /// actor's previous movement. Friendship does not reject a Guard post.
    #[test]
    fn a_limboed_guard_post_leaves_the_actor_moving() {
        let rules = amcv_move_rules();
        let mut sim = Simulation::new();
        spawn_rule_backed_unit(&mut sim, 1, "AMCV", &rules);
        spawn_rule_backed_unit(&mut sim, 2, "AMCV", &rules);
        sim.substrate
            .entities
            .get_mut(2)
            .unwrap()
            .lifecycle
            .in_limbo = true;
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        sim.install_fixture_path_grid(Some(&grid));
        assert!(sim.apply_command(
            "Americans",
            &Command::Move {
                entity_id: 1,
                target_rx: 25,
                target_ry: 20,
                queue: false,
            },
            Some(&rules),
        ));
        assert!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .movement_target
                .is_some()
        );

        assert!(!sim.apply_command(
            "Americans",
            &Command::Guard {
                entity_id: 1,
                target: Some(crate::sim::combat::TargetKind::Entity(2)),
            },
            Some(&rules),
        ));
        assert!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .movement_target
                .is_some(),
            "the refused guard must not have stopped the unit"
        );
    }

    /// An in-limbo actor — inside a transport, a tank bunker or a garrison —
    /// abandons the whole order: no queued mission, no path.
    #[test]
    fn move_order_is_dropped_when_the_actor_is_in_limbo() {
        let rules = amcv_move_rules();
        let mut sim = Simulation::new();
        spawn_rule_backed_unit(&mut sim, 1, "AMCV", &rules);
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .lifecycle
            .in_limbo = true;
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        sim.install_fixture_path_grid(Some(&grid));

        assert!(!sim.order_actor_admits(1));
        assert!(!sim.apply_command(
            "Americans",
            &Command::Move {
                entity_id: 1,
                target_rx: 25,
                target_ry: 20,
                queue: false,
            },
            Some(&rules),
        ));
        let actor = sim.substrate.entities.get(1).unwrap();
        assert!(actor.movement_target.is_none());
        assert_eq!(actor.mission.queued(), MissionId::NONE);
    }

    /// A not-natively-alive or zero-strength actor is rejected on the same
    /// clause, independently of store presence.
    #[test]
    fn move_order_is_dropped_for_a_dead_or_zero_strength_actor() {
        let rules = amcv_move_rules();

        for kill in [
            |e: &mut GameEntity| e.lifecycle.object_alive = false,
            |e: &mut GameEntity| e.health.current = 0,
        ] {
            let mut sim = Simulation::new();
            spawn_rule_backed_unit(&mut sim, 1, "AMCV", &rules);
            kill(sim.substrate.entities.get_mut(1).unwrap());
            let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
            sim.install_fixture_path_grid(Some(&grid));

            assert!(!sim.order_actor_admits(1));
            assert!(!sim.apply_command(
                "Americans",
                &Command::Move {
                    entity_id: 1,
                    target_rx: 25,
                    target_ry: 20,
                    queue: false,
                },
                Some(&rules),
            ));
            assert!(
                sim.substrate
                    .entities
                    .get(1)
                    .unwrap()
                    .movement_target
                    .is_none()
            );
        }
    }

    /// The Target half: a victim that hit zero strength this tick is still
    /// resolvable in the store, and the whole order is abandoned rather than
    /// retasking the attacker onto it. Store presence alone is not admission.
    #[test]
    fn attack_order_is_dropped_when_the_clicked_target_is_already_dead() {
        let rules = amcv_move_rules();
        let mut sim = Simulation::new();
        spawn_rule_backed_unit(&mut sim, 1, "AMCV", &rules);
        spawn_structure_for_owner(&mut sim, 2, "AMCV", "Soviet", 24, 20);

        // Control: a live enemy target passes the token gate.
        assert!(sim.order_object_token_admits(2));

        // Now the same target at zero strength, still present in the store.
        sim.substrate.entities.get_mut(2).unwrap().health.current = 0;
        assert!(
            sim.substrate.entities.contains(2),
            "target stays resolvable"
        );
        assert!(!sim.order_object_token_admits(2));
        assert!(!sim.apply_command(
            "Americans",
            &Command::Attack {
                attacker_id: 1,
                target_id: 2,
            },
            Some(&rules),
        ));
        let attacker = sim.substrate.entities.get(1).unwrap();
        assert!(attacker.attack_target.is_none());
        assert_eq!(attacker.mission.queued(), MissionId::NONE);
    }

    // ===== Duplicate Enter on a building is a no-op (GSI-07.01 C1) =====

    /// All four clauses must hold — committed mission already Enter, receiver
    /// is not a structure, destination IS a structure, and contact slot 0
    /// already names that destination.
    #[test]
    fn duplicate_enter_predicate_requires_all_four_clauses() {
        let rules = amcv_move_rules();
        let mut sim = Simulation::new();
        spawn_rule_backed_unit(&mut sim, 1, "AMCV", &rules);
        spawn_structure_for_owner(&mut sim, 2, "AMCV", "Americans", 24, 20);
        spawn_rule_backed_unit(&mut sim, 3, "AMCV", &rules); // a non-building

        // No mission, no link.
        assert!(!sim.duplicate_enter_is_noop(1, 2));

        // Linked but the committed mission is not Enter.
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .mark_live_contact_with(2);
        assert!(!sim.duplicate_enter_is_noop(1, 2));

        // Committed Enter + link to that building: the no-op.
        sim.mission_assign_exact(1, MissionId::from_known(MissionType::Enter), 0)
            .expect("receiver present");
        assert!(sim.duplicate_enter_is_noop(1, 2));

        // Same mission and link, but the destination is not a building.
        assert!(!sim.duplicate_enter_is_noop(1, 3));
    }

    fn miner_return_rules() -> RuleSet {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             0=HARV\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GAREFN\n\
             1=OTHERPROC\n\
             [HARV]\n\
             Name=War Miner\n\
             Harvester=yes\n\
             Dock=GAREFN\n\
             Speed=4\n\
             [GAREFN]\n\
             Name=Ore Refinery\n\
             Strength=900\n\
             Foundation=4x3\n\
             Refinery=yes\nDockUnload=yes\n\
             [OTHERPROC]\n\
             Name=Other Refinery\n\
             Strength=900\n\
             Foundation=4x3\n\
             Refinery=yes\nDockUnload=yes\n",
        );
        RuleSet::from_ini(&ini).expect("miner return rules")
    }

    fn spawn_miner(sim: &mut Simulation, sid: u64) {
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("HARV");
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            sid,
            20,
            20,
            0,
            0,
            owner,
            Health { current: 600 },
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        // Direct insertion skips Unlimbo; player orders require a live,
        // revealed actor before the shared Event retask prefix runs.
        entity.lifecycle.in_limbo = false;
        entity.miner = Some(Miner::new(MinerKind::War, &MinerConfig::default(), 0));
        sim.substrate.entities.insert(entity);
    }

    fn spawn_refinery(sim: &mut Simulation, sid: u64, type_id: &str, rx: u16, ry: u16) {
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern(type_id);
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            sid,
            rx,
            ry,
            0,
            0,
            owner,
            Health { current: 900 },
            type_ref,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        entity.lifecycle.in_limbo = false;
        sim.substrate.entities.insert(entity);
    }

    fn rally_rules() -> RuleSet {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GAPILE\n\
             1=GAWEAP\n\
             2=GAPOWR\n\
             3=NAWEAP\n\
             [GAPILE]\nFactory=InfantryType\nStrength=500\n\
             [GAWEAP]\nFactory=UnitType\nStrength=1000\n\
             [GAPOWR]\nStrength=750\n\
             [NAWEAP]\nFactory=UnitType\nStrength=1000\n",
        );
        RuleSet::from_ini(&ini).expect("rally rules")
    }

    fn spawn_structure_for_owner(
        sim: &mut Simulation,
        sid: u64,
        type_id: &str,
        owner_name: &str,
        rx: u16,
        ry: u16,
    ) {
        let owner = sim.interner.intern(owner_name);
        let type_ref = sim.interner.intern(type_id);
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            sid,
            rx,
            ry,
            0,
            0,
            owner,
            Health { current: 1000 },
            type_ref,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        entity.lifecycle.in_limbo = false;
        sim.substrate.entities.insert(entity);
    }

    #[test]
    fn set_rally_updates_only_owned_eligible_producers() {
        let rules = rally_rules();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let enemy = sim.interner.intern("Soviet");
        sim.houses.insert(
            owner,
            HouseState::new(owner, 0, Some(owner), true, 10_000, 10),
        );
        sim.houses.insert(
            enemy,
            HouseState::new(enemy, 1, Some(enemy), false, 10_000, 10),
        );
        spawn_structure_for_owner(&mut sim, 2, "GAPILE", "Americans", 10, 10);
        spawn_structure_for_owner(&mut sim, 3, "GAWEAP", "Americans", 12, 10);
        spawn_structure_for_owner(&mut sim, 4, "GAPOWR", "Americans", 14, 10);
        spawn_structure_for_owner(&mut sim, 5, "NAWEAP", "Soviet", 16, 10);

        let command = Command::SetRally {
            rx: 40,
            ry: 41,
            producer_ids: vec![3, 2, 2, 4, 5],
        };

        assert!(sim.apply_command("Americans", &command, Some(&rules)));
        let rally = |id| sim.substrate.entities.get(id).unwrap().rally_cell();
        assert_eq!(rally(2), Some((40, 41)));
        assert_eq!(rally(3), Some((40, 41)));
        assert_eq!(rally(4), None);
        assert_eq!(rally(5), None);
    }

    #[test]
    fn miner_return_with_explicit_refinery_seeds_clicked_target() {
        let rules = miner_return_rules();
        let mut sim = Simulation::new();
        spawn_miner(&mut sim, 1);
        spawn_refinery(&mut sim, 2, "GAREFN", 10, 10);
        spawn_refinery(&mut sim, 3, "GAREFN", 30, 30);
        // Docked and unloading at refinery 2: linked and tethered both ways,
        // Unload current with its latch raised.
        for (id, partner) in [(1, 2), (2, 1)] {
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity.radio_contacts.set_slot(0, partner);
            entity.dock_entered_with = Some(partner);
        }
        let now = sim.session.binary_frame;
        sim.mission_assign_exact(
            1,
            crate::sim::mission::MissionId::from_known(MissionType::Unload),
            now,
        )
        .unwrap();
        {
            let entity = sim.substrate.entities.get_mut(1).unwrap();
            entity.display_type_override = Some(sim.interner.intern("HORV"));
            let miner = entity.miner.as_mut().unwrap();
            miner.unload_active = true;
            entity.restart_native_stage(0, now as i32, 1);
        }

        let applied = sim.apply_command(
            "Americans",
            &Command::MinerReturn {
                entity_id: 1,
                target_refinery_id: Some(3),
            },
            Some(&rules),
        );

        assert!(applied);
        let miner_entity = sim.substrate.entities.get(1).unwrap();
        let miner = miner_entity.miner.as_ref().unwrap();
        assert_eq!(miner.reserved_refinery, Some(3));
        assert_eq!(miner_entity.miner_state(), Some(MinerState::ForcedReturn));
        // The order's radio break leaves the old refinery on both ends and
        // the unload latch with it; a latch left up would refuse every later
        // Ready_To_Commence (0x007442AB) and hold the miner in place.
        assert!(!miner.unload_active);
        assert_eq!(miner_entity.display_type_override, None);
        assert!(!miner_entity.radio_contacts.contains(2));
        assert_eq!(miner_entity.dock_entered_with, None);
        let refinery = sim.substrate.entities.get(2).unwrap();
        assert!(!refinery.radio_contacts.contains(1));
        assert_eq!(refinery.dock_entered_with, None);
        spawn_miner(&mut sim, 7);
        assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, 7));
    }

    #[test]
    fn generic_miner_return_can_reselect_later_without_rules() {
        let mut sim = Simulation::new();
        spawn_miner(&mut sim, 1);

        let applied = sim.apply_command(
            "Americans",
            &Command::MinerReturn {
                entity_id: 1,
                target_refinery_id: None,
            },
            None,
        );

        assert!(applied);
        let miner = sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .miner
            .as_ref()
            .unwrap();
        assert_eq!(miner.reserved_refinery, None);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().miner_state(),
            Some(MinerState::ForcedReturn)
        );
    }

    #[test]
    fn explicit_miner_return_rejects_incompatible_refinery() {
        let rules = miner_return_rules();
        let mut sim = Simulation::new();
        spawn_miner(&mut sim, 1);
        spawn_refinery(&mut sim, 2, "OTHERPROC", 10, 10);

        let applied = sim.apply_command(
            "Americans",
            &Command::MinerReturn {
                entity_id: 1,
                target_refinery_id: Some(2),
            },
            Some(&rules),
        );

        assert!(!applied);
        let miner = sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .miner
            .as_ref()
            .unwrap();
        assert_eq!(miner.reserved_refinery, None);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().miner_state(),
            Some(MinerState::SearchOre)
        );
    }

    fn bunker_rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=TANK\n1=NOGUN\n\n[InfantryTypes]\n\n[AircraftTypes]\n\n\
             [BuildingTypes]\n0=NATBNK\n\n\
             [TANK]\nStrength=400\nArmor=heavy\nSpeed=6\nBunkerable=yes\nPrimary=120mm\n\n\
             [NOGUN]\nStrength=400\nArmor=heavy\nSpeed=6\nBunkerable=yes\n\n\
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
            Health { current: 1000 },
            type_id,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        ge.bunker_runtime = Some(crate::sim::docking::bunker_install::BunkerRuntime::idle());
        // Revealed object: order admission reads the limbo byte.
        ge.lifecycle.in_limbo = false;
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
            Health { current: 400 },
            type_id,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        // Revealed object: order admission reads the limbo byte.
        ge.lifecycle.in_limbo = false;
        sim.substrate.entities.insert(ge);
    }

    #[test]
    fn enter_bunker_admits_and_starts_install_machine() {
        use crate::sim::docking::bunker_install::BunkerState;
        use crate::sim::game_entity::BunkerLink;
        let rules = bunker_rules();
        let mut sim = Simulation::new();
        spawn_bunker_struct(&mut sim, 2, "Americans", 10, 10);
        spawn_bunkerable(&mut sim, 1, "Americans", "TANK", 14, 14);

        let applied = sim.apply_command(
            "Americans",
            &Command::EnterBunker {
                unit_id: 1,
                bunker_id: 2,
            },
            Some(&rules),
        );

        assert!(applied);
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.bunker_link, BunkerLink::Approaching(2));
        assert_eq!(
            unit.mission.queued(),
            MissionId::from_known(MissionType::Enter)
        );
        let rt = sim
            .substrate
            .entities
            .get(2)
            .unwrap()
            .bunker_runtime
            .unwrap();
        assert_eq!(rt.state, BunkerState::ArriveWait);
        assert_eq!(rt.installing_unit, Some(1));
    }

    #[test]
    fn enter_bunker_rejects_unit_without_a_weapon() {
        use crate::sim::docking::bunker_install::BunkerState;
        use crate::sim::game_entity::BunkerLink;
        let rules = bunker_rules();
        let mut sim = Simulation::new();
        spawn_bunker_struct(&mut sim, 2, "Americans", 10, 10);
        // Bunkerable but no Primary → CanAutoDeployHere rejects it.
        spawn_bunkerable(&mut sim, 1, "Americans", "NOGUN", 14, 14);

        let applied = sim.apply_command(
            "Americans",
            &Command::EnterBunker {
                unit_id: 1,
                bunker_id: 2,
            },
            Some(&rules),
        );

        assert!(!applied);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().bunker_link,
            BunkerLink::None
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(2)
                .unwrap()
                .bunker_runtime
                .unwrap()
                .state,
            BunkerState::Idle,
            "rejected admission leaves the machine idle"
        );
    }

    #[test]
    fn enter_enemy_bunker_is_rejected() {
        let rules = bunker_rules();
        let mut sim = Simulation::new();
        spawn_bunker_struct(&mut sim, 2, "Soviets", 10, 10);
        spawn_bunkerable(&mut sim, 1, "Americans", "TANK", 14, 14);

        let applied = sim.apply_command(
            "Americans",
            &Command::EnterBunker {
                unit_id: 1,
                bunker_id: 2,
            },
            Some(&rules),
        );

        assert!(!applied, "cannot bunker into an enemy building");
    }

    #[test]
    fn eject_bunker_releases_occupant() {
        use crate::sim::game_entity::BunkerLink;
        let rules = bunker_rules();
        let mut sim = Simulation::new();
        spawn_bunker_struct(&mut sim, 2, "Americans", 10, 10);
        spawn_bunkerable(&mut sim, 1, "Americans", "TANK", 14, 14);
        let unit = sim.substrate.entities.get_mut(1).unwrap();
        unit.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Drive,
            ),
        );
        unit.locomotor
            .as_mut()
            .unwrap()
            .ensure_installed_track_state();
        // Admission-only spawn helper preclears Limbo without placing the
        // object. Restore its constructor gate and publish through Reveal.
        unit.lifecycle.in_limbo = true;
        assert!(matches!(
            sim.reveal(1),
            crate::sim::world::RevealOutcome::Revealed { .. }
        ));
        crate::sim::docking::bunker_link::install_bunker_link(&mut sim, 2, 1, &rules);
        assert_eq!(
            sim.substrate.entities.get(2).unwrap().bunker_occupant,
            Some(1)
        );

        let applied = sim.apply_command(
            "Americans",
            &Command::EjectBunker { bunker_id: 2 },
            Some(&rules),
        );

        assert!(applied);
        assert_eq!(sim.substrate.entities.get(2).unwrap().bunker_occupant, None);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().bunker_link,
            BunkerLink::None
        );
        // Force starts at the retained pose; the SW exit is a destination.
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!((unit.position.rx, unit.position.ry), (14, 14));
        assert_eq!(
            unit.navigation.nav_com,
            Some(crate::sim::components::NavTargetRef::cell(9, 11))
        );
        assert_eq!(
            unit.locomotor
                .as_ref()
                .and_then(|loco| loco.selected_drive_runtime())
                .and_then(|runtime| runtime.retained())
                .unwrap()
                .track()
                .turn_index,
            0x47
        );
        assert_eq!(
            unit.mission.queued(),
            MissionId::from_known(MissionType::Move)
        );
    }

    #[test]
    fn eject_empty_bunker_is_noop() {
        let rules = bunker_rules();
        let mut sim = Simulation::new();
        spawn_bunker_struct(&mut sim, 2, "Americans", 10, 10);

        let applied = sim.apply_command(
            "Americans",
            &Command::EjectBunker { bunker_id: 2 },
            Some(&rules),
        );

        assert!(!applied, "ejecting an empty bunker does nothing");
    }

    #[test]
    fn bunker_full_lifecycle_enter_install_then_eject() {
        use crate::sim::docking::bunker_install::{BunkerState, tick_bunker_install};
        use crate::sim::game_entity::BunkerLink;
        let rules = bunker_rules();
        let mut sim = Simulation::new();
        spawn_bunker_struct(&mut sim, 2, "Americans", 10, 10);
        // Place the tank ON the bunker cell. The native destination setter
        // still admits this same-cell order; Drive Process consumes it before
        // the install machine can observe the stopped candidate.
        spawn_bunkerable(&mut sim, 1, "Americans", "TANK", 10, 10);
        let unit = sim.substrate.entities.get_mut(1).unwrap();
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
        unit.lifecycle.in_limbo = true;
        assert!(matches!(
            sim.reveal(1),
            crate::sim::world::RevealOutcome::Revealed { .. }
        ));

        // 1) Enter: admission + install machine starts.
        assert!(sim.apply_command(
            "Americans",
            &Command::EnterBunker {
                unit_id: 1,
                bunker_id: 2,
            },
            Some(&rules),
        ));
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().bunker_link,
            BunkerLink::Approaching(2)
        );
        // Production visits movement before bunker installation. The shared
        // Drive4B066C..4B06D2 prefix consumes a same-cell NavCom through the
        // class NULL setter before acquiring any path-search inputs.
        sim.process_ground_locomotor_stats_for_test(1, Some(&rules), None)
            .unwrap();

        // 2) Drive the install machine to Occupied, completing each body turn
        // it issues in this component harness.
        for _ in 0..6 {
            tick_bunker_install(&mut sim, &rules, None);
            let frame = sim.session.binary_frame;
            if let Some(u) = sim.substrate.entities.get_mut(1) {
                let destination = u.body_facing.destination();
                u.body_facing.snap(destination, frame);
            }
        }
        let rt = sim
            .substrate
            .entities
            .get(2)
            .unwrap()
            .bunker_runtime
            .unwrap();
        assert_eq!(rt.state, BunkerState::Occupied);
        assert_eq!(
            sim.substrate.entities.get(2).unwrap().bunker_occupant,
            Some(1)
        );
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.bunker_link, BunkerLink::Installed(2));
        assert!(
            unit.in_logic_vector,
            "install preserves LogicVector membership"
        );
        assert!(!unit.lifecycle.in_limbo);
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
        assert_eq!(sim.substrate.entities.get(2).unwrap().bunker_occupant, None);
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.bunker_link, BunkerLink::None);
        assert!(unit.in_logic_vector, "occupant stays active on eject");
        assert!(!unit.lifecycle.in_limbo);
        assert_eq!((unit.position.rx, unit.position.ry), (10, 10));
        assert_eq!(
            unit.locomotor
                .as_ref()
                .and_then(|loco| loco.selected_drive_runtime())
                .and_then(|runtime| runtime.retained())
                .unwrap()
                .track()
                .turn_index,
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
    }
}
