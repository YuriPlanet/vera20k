//! Individual command payload behavior for the Simulation.
//!
//! Contains `apply_command()` and its helper methods: selection snapshots,
//! ownership checks, and friendship queries. `command_schedule` owns queue
//! admission and scheduled batch order.
//!
//! Dependency rules: same as sim/ (depends on rules/, map/; never render/ui/audio/net).

use std::collections::{BTreeMap, BTreeSet};

use super::{SimSoundEvent, Simulation, SimulationWallRuntimeHost};
use crate::map::houses::are_houses_friendly;
#[cfg(test)]
use crate::rules::locomotor_type::MovementZone;
use crate::rules::locomotor_type::{LocomotorKind, SpeedType};
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_rect::canonical_cell_coord;
use crate::sim::combat;
#[cfg(test)]
use crate::sim::combat::combat_aoe::CellTargetDetach;
use crate::sim::combat::combat_aoe::expire_cell_target_references;
use crate::sim::command::{
    COMMAND_RECORD_LEN, Command, CommandEnvelope, CommandRecord, ExitRecord, MegaMissionMoveRecord,
    SellWallAtCellRecord,
};
use crate::sim::components::OrderIntent;
use crate::sim::docking::building_dock::{self, DockState};
use crate::sim::mission::{DockTeardown, MissionType};
use crate::sim::movement;
use crate::sim::movement::bump_crush;
use crate::sim::movement::jumpjet_movement;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::movement::teleport_movement;
use crate::sim::overlay_grid::{
    NavigationPublication, OverlayRecalcOutcome, RecomputeResult, runtime_wall_cleanup_visit_at,
};
use crate::sim::passenger;
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::terrain_cost::build_canonical_terrain_cost_grids;
use crate::sim::pathfinding::zone_incremental::{
    PackedZoneCoord, ZoneRepairKind, repair_zone_cell,
};
use crate::sim::production;
use crate::util::fixed_math::{SIM_ZERO, SimFixed, ra2_speed_to_leptons_per_second};

/// Read-only snapshot of entity + rules data needed for issuing movement commands.
/// Captured once to avoid repeated entity lookups and type_ref clones.
///
/// `pub(crate)` so the pursuit pre-combat stage in `world_orders.rs` can reuse
/// it — pursuit-issued movement must match Move-command-issued movement
/// exactly to keep behavior consistent.
pub(crate) struct MoveInfo {
    pub(crate) speed: SimFixed,
    pub(crate) loco_kind: Option<LocomotorKind>,
    pub(crate) loco_layer: MovementLayer,
    pub(crate) speed_type: SpeedType,
    pub(crate) hover_attack: bool,
    pub(crate) is_teleporter: bool,
    pub(crate) is_harvester: bool,
    pub(crate) is_infantry: bool,
    pub(crate) accel_factor: SimFixed,
    pub(crate) decel_factor: SimFixed,
    pub(crate) slowdown_distance: SimFixed,
    #[cfg(test)]
    pub(crate) movement_zone: MovementZone,
    pub(crate) position: (u16, u16),
    #[cfg(test)]
    pub(crate) regular_crusher: bool,
    #[cfg(test)]
    pub(crate) omni_crusher: bool,
    #[cfg(test)]
    pub(crate) drive_accelerates: bool,
    pub(crate) mover_is_crusher: bool,
}

#[cfg(test)]
impl MoveInfo {
    pub(crate) fn crush_capability(&self) -> bump_crush::CrushCapability {
        bump_crush::CrushCapability::new(self.regular_crusher, self.omni_crusher)
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
    /// Build the exact native MegaMission record for one ordinary local Move.
    ///
    /// `EventClass__BuildMegaMissionEnvelope` at `gamemd.exe` `0x004C6860`
    /// stores HouseClass registration and Abstract stable identity separately;
    /// the source therefore need not belong to the issuing house. Rust-only
    /// queued waypoints and move-group metadata are not representable here.
    pub(crate) fn encode_megamission_move_record(
        &self,
        command_owner: crate::sim::intern::InternedId,
        source_id: u64,
        target_rx: u16,
        target_ry: u16,
    ) -> Option<CommandRecord> {
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
        let typed = MegaMissionMoveRecord {
            house_id,
            frame: self.session.binary_frame as i32,
            source_id: i32::try_from(source_id).ok()?,
            target_x: i16::try_from(target_rx).ok()?,
            target_y: i16::try_from(target_ry).ok()?,
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
        if let Some(typed) = MegaMissionMoveRecord::decode(record) {
            let house_index = usize::try_from(typed.house_id).ok()?;
            let owner = *self.session.house_order.get(house_index)?;
            if !self.houses.contains_key(&owner) {
                return None;
            }
            let entity_id = u64::try_from(typed.source_id).ok()?;
            if self.substrate.entities.get(entity_id).is_none() {
                return None;
            }
            return Some(CommandEnvelope::new(
                owner,
                execute_tick,
                Command::Move {
                    entity_id,
                    target_rx: u16::try_from(typed.target_x).ok()?,
                    target_ry: u16::try_from(typed.target_y).ok()?,
                    queue: false,
                    group_id: None,
                },
            ));
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
            self.terrain_costs = build_canonical_terrain_cost_grids(terrain);
            let resolved =
                PathGrid::from_resolved_terrain_with_bridges(terrain, self.bridge_state.as_ref());
            if tail_grid.as_ref().is_some_and(|tail| {
                tail.width() != resolved.width() || tail.height() != resolved.height()
            }) {
                *tail_grid = None;
            }
            if let Some(tail) = tail_grid.as_mut() {
                let _ = tail.replace_cell_from(&resolved, rx, ry);
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
        let Some(zone_grid) = self.zone_grid.as_mut() else {
            return;
        };
        let bridge_records = self
            .bridge_state
            .as_ref()
            .map(|state| state.endpoint_records())
            .unwrap_or(&[]);
        #[cfg(test)]
        trace_wall_sell_zone_repair_step(zone_grid, tail_grid, sold_cell, repair_cell);
        let _ = repair_zone_cell(
            zone_grid,
            PackedZoneCoord::new(repair_cell.0 as i16, repair_cell.1 as i16),
            repair,
            tail_grid,
            self.playfield_bounds,
            terrain,
            bridge_records,
        );
    }

    fn sell_wall_at_cell(
        &mut self,
        command_owner: &str,
        x: i16,
        y: i16,
        rules: &RuleSet,
        path_grid: Option<&PathGrid>,
        overlays: &crate::map::overlay_types::OverlayTypeRegistry,
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

        let mut tail_grid = path_grid
            .cloned()
            .or_else(|| self.path_grid.as_deref().cloned());
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
                grid.recalculate_runtime_cell(
                    terrain,
                    overlays,
                    (rx, ry),
                    NavigationPublication::NextPathReader,
                )
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
                (Some(grid), Some(terrain)) => grid.recalculate_runtime_cell(
                    terrain,
                    overlays,
                    (nx, ny),
                    NavigationPublication::NextPathReader,
                ),
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
        let loco_kind = loco.map(|l| l.kind);
        let loco_layer = e.movement_layer_or_ground();
        let speed_type = loco.map(|l| l.speed_type).unwrap_or(SpeedType::Track);
        let hover_attack = loco.map(|l| l.hover_attack).unwrap_or(false);
        let loco_multiplier = loco
            .map(|l| l.speed_multiplier)
            .unwrap_or(SimFixed::from_num(1));

        let obj = rules.and_then(|r| self.object_type(e.type_ref(), r));
        // `FootClass::GetCurrentSpeed @ 0x004DB1A0`: every ordered move asks the
        // getter for the speed, so the `FASTER` multiply belongs here, between
        // the truncated type speed and the locomotor fraction — not only on the
        // deferred repath. This is the resolver behind Move, AttackMove, Enter,
        // C4, capture and bunker-entry, so skipping it ran every promoted unit
        // at rookie speed.
        let base_speed = crate::sim::combat::veterancy::entity_mover_speed_leptons_per_second(
            e,
            obj,
            obj.map_or(4, |o| o.speed),
            rules.map_or(1.0, |r| r.general.veteran_speed),
        );
        let speed = (base_speed * loco_multiplier).max(SimFixed::lit("25"));

        Some(MoveInfo {
            speed,
            loco_kind,
            loco_layer,
            speed_type,
            hover_attack,
            is_teleporter: obj.map_or(false, |o| o.teleporter),
            is_harvester: obj.map_or(false, |o| o.harvester),
            is_infantry: obj.map_or(false, |o| o.category == ObjectCategory::Infantry),
            accel_factor: obj.map_or(SIM_ZERO, |o| o.accel_factor),
            decel_factor: obj.map_or(SIM_ZERO, |o| o.decel_factor),
            slowdown_distance: obj.map_or(SIM_ZERO, |o| SimFixed::from_num(o.slowdown_distance)),
            #[cfg(test)]
            movement_zone: obj.map_or(MovementZone::Normal, |o| o.movement_zone),
            position: (e.position.rx, e.position.ry),
            #[cfg(test)]
            regular_crusher: e.regular_crusher,
            #[cfg(test)]
            omni_crusher: e.omni_crusher,
            #[cfg(test)]
            drive_accelerates: e.drive_accelerates,
            mover_is_crusher: bump_crush::CrushCapability::of(e).can_crush_units(),
        })
    }

    /// Dispatch a single command, returning true if it was successfully applied.
    #[cfg(test)]
    pub(crate) fn apply_command(
        &mut self,
        command_owner: &str,
        cmd: &Command,
        rules: Option<&RuleSet>,
        path_grid: Option<&PathGrid>,
        height_map: &BTreeMap<(u16, u16), u8>,
    ) -> bool {
        self.apply_command_with_overlays(command_owner, cmd, rules, path_grid, height_map, None)
    }

    pub(crate) fn apply_command_with_overlays(
        &mut self,
        command_owner: &str,
        cmd: &Command,
        rules: Option<&RuleSet>,
        path_grid: Option<&PathGrid>,
        height_map: &BTreeMap<(u16, u16), u8>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        match cmd {
            Command::Select { entity_ids, .. } => self.apply_selection_snapshot(entity_ids, rules),
            Command::Move {
                entity_id,
                target_rx,
                target_ry,
                queue,
                group_id,
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
                // Native order admission: a dead, zero-strength or in-limbo
                // actor abandons the whole order and keeps its previous one.
                if !self.order_actor_admits(*entity_id) {
                    return false;
                }
                // Drop any dock reservation (depot + aircraft + docked-idle) and
                // retask onto a fresh Move via the verb API. The legacy field
                // clears below stay authoritative in Slice 6.
                self.queue_megamission_with_teardown(
                    *entity_id,
                    MissionType::Move,
                    DockTeardown::All,
                );
                // Clear attack and order intent.
                if let Some(e) = self.substrate.entities.get_mut(*entity_id) {
                    e.attack_target = None;
                    // Provenance cannot outlive the target it describes.
                    e.passively_acquired_target = false;
                    e.order_intent = None;
                    e.dock_state = None;
                    e.c4_plant = None;
                    Self::clear_aircraft_dock_phase(e);
                }
                // Snapshot speed, locomotor, and rules data in one lookup.
                let Some(info) = self.resolve_move_info(*entity_id, rules) else {
                    return false;
                };
                // Chrono Miners (Teleporter=yes + Harvester=yes) drive normally for
                // player commands — they only teleport on return-to-refinery
                // (handled by miner_system::chrono_teleport, not here).
                let use_teleport_move = !info.is_harvester
                    && (info.loco_kind == Some(LocomotorKind::Teleport) || info.is_teleporter);

                // Build entity block set for friendly-passable pathfinding.
                let (entity_blocks, entity_block_map) = bump_crush::build_entity_block_set(
                    &self.substrate.entities,
                    command_owner,
                    &self.house_alliances,
                    &self.interner,
                    rules,
                );
                let general_rules = rules.map(|r| &r.general);
                let result = if use_teleport_move {
                    // Teleport locomotor or non-harvester Teleporter=yes: instant relocation.
                    // `use_teleport_move` already excludes harvesters, so is_harvester=false.
                    let default_general = crate::rules::ruleset::GeneralRules::default();
                    teleport_movement::issue_teleport_command(
                        &mut self.substrate.entities,
                        *entity_id,
                        (*target_rx, *target_ry),
                        general_rules.unwrap_or(&default_general),
                        false,
                        self.session.binary_frame,
                    )
                } else if info.loco_layer == MovementLayer::Air {
                    // Jumpjet infantry walk fallback: ≤3 cells + !HoverAttack → ground walk.
                    if info.loco_kind == Some(LocomotorKind::Jumpjet) && info.is_infantry {
                        let dx = (*target_rx as i32 - info.position.0 as i32).unsigned_abs();
                        let dy = (*target_ry as i32 - info.position.1 as i32).unsigned_abs();
                        let dist_cells = dx.max(dy);
                        if jumpjet_movement::should_use_walk_fallback(
                            info.hover_attack,
                            true,
                            dist_cells,
                        ) {
                            let Some(grid) = path_grid else { return false };
                            let cost_grid = self.terrain_costs.get(&info.speed_type);
                            let blocker_neighbor_counts =
                                bump_crush::build_blocker_neighbor_counts_with_overlays(
                                    &self.substrate.entities,
                                    grid.width(),
                                    grid.height(),
                                    self.resolved_terrain.as_ref(),
                                    self.overlay_grid.as_ref(),
                                    overlay_registry,
                                    &self.interner,
                                    rules,
                                );
                            return movement::issue_move_command_with_layered(
                                &mut self.substrate.entities,
                                grid,
                                *entity_id,
                                (*target_rx, *target_ry),
                                info.speed,
                                *queue,
                                cost_grid,
                                Some(&entity_blocks),
                                self.resolved_terrain.as_ref(),
                                self.zone_grid.as_ref(),
                                Some(&entity_block_map),
                                info.mover_is_crusher,
                                Some(&blocker_neighbor_counts),
                                self.playfield_bounds,
                                Some(&mut self.substrate.cell_occupation),
                                crate::sim::movement::DestinationTiming::from_rules(
                                    self.session.binary_frame,
                                    rules.into(),
                                ),
                            );
                        }
                    }
                    // Air units fly in straight lines — no A* pathfinding needed.
                    let ok = self.issue_air_cell_destination(
                        *entity_id,
                        (*target_rx, *target_ry),
                        info.speed,
                        rules,
                    );
                    // Set Move mission so the aircraft flies to destination
                    // before the Idle handler can redirect it to RTB.
                    if ok {
                        if let Some(e) = self.substrate.entities.get_mut(*entity_id) {
                            if e.aircraft_mission.is_some() {
                                e.aircraft_mission =
                                    Some(crate::sim::aircraft::AircraftMission::Move {
                                        sub_state: 0,
                                    });
                            }
                        }
                    }
                    ok
                } else {
                    let Some(grid) = path_grid else { return false };
                    let cost_grid = self.terrain_costs.get(&info.speed_type);
                    let blocker_neighbor_counts =
                        bump_crush::build_blocker_neighbor_counts_with_overlays(
                            &self.substrate.entities,
                            grid.width(),
                            grid.height(),
                            self.resolved_terrain.as_ref(),
                            self.overlay_grid.as_ref(),
                            overlay_registry,
                            &self.interner,
                            rules,
                        );
                    movement::issue_move_command_with_layered(
                        &mut self.substrate.entities,
                        grid,
                        *entity_id,
                        (*target_rx, *target_ry),
                        info.speed,
                        *queue,
                        cost_grid,
                        Some(&entity_blocks),
                        self.resolved_terrain.as_ref(),
                        self.zone_grid.as_ref(),
                        Some(&entity_block_map),
                        info.mover_is_crusher,
                        Some(&blocker_neighbor_counts),
                        self.playfield_bounds,
                        Some(&mut self.substrate.cell_occupation),
                        crate::sim::movement::DestinationTiming::from_rules(
                            self.session.binary_frame,
                            rules.into(),
                        ),
                    )
                };
                // Stamp acceleration/deceleration parameters onto the newly created
                // MovementTarget so the per-tick movement loop can ramp speed.
                if result {
                    if let Some(e) = self.substrate.entities.get_mut(*entity_id) {
                        if let Some(ref mut mt) = e.movement_target {
                            mt.accel_factor = info.accel_factor;
                            mt.decel_factor = info.decel_factor;
                            mt.slowdown_distance = info.slowdown_distance;
                            mt.group_id = *group_id;
                        }
                    }
                }
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
                // entered its dock ignores Stop and finishes unloading.
                if self
                    .substrate
                    .entities
                    .get(*entity_id)
                    .is_some_and(|entity| entity.dock_entered_with.is_some())
                {
                    return true;
                }
                // Retail breaks EVERY radio contact on Stop (it broadcasts the
                // break message to the whole contact list), so the refinery,
                // airfield and service-depot links all go at once. Cancelling
                // only the depot reservation left an aircraft that was told to
                // stop while inbound to a helipad holding that pad for the rest
                // of the match — a permanent leak that compounds.
                let mcv = rules.is_some_and(|rules| {
                    self.substrate
                        .entities
                        .get(*entity_id)
                        .is_some_and(|e| crate::sim::mcv_deploy::is_mcv(self, e, rules))
                });
                // Event6 retains an ordinary MCV's mission and runtime +0x68C.
                // Its null-destination operation still runs below.
                if mcv {
                    self.run_dock_teardown(*entity_id, DockTeardown::All);
                } else {
                    self.queue_mission_with_teardown(
                        *entity_id,
                        MissionType::Stop,
                        DockTeardown::All,
                    );
                }
                if let Some(e) = self.substrate.entities.get_mut(*entity_id) {
                    movement::stop_navigation_at_committed_head(e);
                    e.attack_target = None;
                    e.passively_acquired_target = false;
                    e.order_intent = None;
                    e.dock_state = None;
                    e.c4_plant = None;
                }
                if !self.stop_jumpjet_infantry_destination(*entity_id, rules, overlay_registry) {
                    return false;
                }
                if let Some(entity) = self.substrate.entities.get_mut(*entity_id) {
                    // Accepted null Foot setter4D96C2 follows locomotor Stop,
                    // including a no-op Stop, and preserves the retry counter.
                    movement::DestinationTiming::from_rules(self.session.binary_frame, rules)
                        .accept(entity);
                }
                // Cancel any special locomotor states in progress.
                // **VERA-internal: retail Stop leaves the installed locomotor
                // alone.** This existing unwind policy uses the same END gate
                // as FootAI4DAEC3 / SetDestination742587, after navigation is
                // cleared but before teleport/layer cleanup. A live Drive head
                // still refuses it. Keep that timing while centralizing the
                // actual instance transfer/retirement in locomotor_owner.
                // Trigger: Stop on a piggybacked Chrono Miner, a few times per
                // ordinary Allied match; a premature unwind can change the next
                // command's locomotor. Native Stop parity remains open. The
                // production command's admitted/refused lifetime is covered by
                // locomotor_owner_tests::stop_command_retires_only_the_drive_admitted_by_its_existing_gate.
                let may_end = self.substrate.entities.get(*entity_id).is_some_and(|e| {
                    let gate = crate::sim::movement::locomotor_end_gate_context(e);
                    e.locomotor.as_ref().is_some_and(|loco| {
                        loco.is_overridden()
                            && loco.can_restore_primary_from_piggyback(
                                gate.owner_moving,
                                gate.owner_teleporting,
                                gate.owner_deploying,
                            )
                    })
                });
                if let Some(e) = self.substrate.entities.get_mut(*entity_id) {
                    e.teleport_state = None;
                    // Restore ground layer and base locomotor if overridden.
                    if let Some(ref mut loco) = e.locomotor
                        && loco.layer == MovementLayer::Underground
                    {
                        loco.layer = MovementLayer::Ground;
                    }
                    if may_end {
                        crate::sim::movement::locomotor_owner::restore_admitted_primary(e);
                    }
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
                // The radio break itself: the IDLE arm pushes BREAK to every
                // link (`PUSH 3; CALL [vt+0x280]` at `0x004C75DC`). Only the
                // miner's refinery handshake is modelled on that bus here.
                // A tethered miner never gets here (see the top of this arm).
                crate::sim::miner::miner_dock::break_for_retask(self, *entity_id);
                self.commit_stop_miner_guard(*entity_id);
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
                // Cancel aircraft RTB/wait + docked-idle (not depot), then retask
                // onto Attack keeping the interrupt stack (combat sets the target).
                self.queue_megamission_with_teardown(
                    *attacker_id,
                    MissionType::Attack,
                    DockTeardown::AircraftOnly,
                );
                if let Some(e) = self.substrate.entities.get_mut(*attacker_id) {
                    e.order_intent = None;
                    Self::clear_aircraft_dock_phase(e);
                }
                let issued = combat::issue_attack_command(
                    &mut self.substrate.entities,
                    *attacker_id,
                    *target_id,
                    rules,
                    &self.interner,
                );
                if issued {
                    self.finish_ordered_walk_attack(*attacker_id, rules);
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
                // Force-attack bypasses friendship check (Ctrl+click). Release a
                // docked-idle aircraft only, then retask onto Attack keeping fields.
                self.queue_megamission_with_teardown(
                    *attacker_id,
                    MissionType::Attack,
                    DockTeardown::IdleOnly,
                );
                if let Some(e) = self.substrate.entities.get_mut(*attacker_id) {
                    e.order_intent = None;
                }
                let issued = combat::issue_attack_command(
                    &mut self.substrate.entities,
                    *attacker_id,
                    *target_id,
                    rules,
                    &self.interner,
                );
                if issued {
                    self.finish_ordered_walk_attack(*attacker_id, rules);
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
                // No target-entity existence check — cells always "exist". Release
                // a docked-idle aircraft only, then retask onto Attack keeping fields.
                self.queue_megamission_with_teardown(
                    *attacker_id,
                    MissionType::Attack,
                    DockTeardown::IdleOnly,
                );
                if let Some(e) = self.substrate.entities.get_mut(*attacker_id) {
                    e.order_intent = None;
                    Self::clear_aircraft_dock_phase(e);
                }
                let issued = combat::issue_attack_cell_command(
                    &mut self.substrate.entities,
                    *attacker_id,
                    *target_rx,
                    *target_ry,
                    rules,
                    &self.interner,
                );
                if issued {
                    self.finish_ordered_walk_attack(*attacker_id, rules);
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
                // Release a docked-idle aircraft only, then retask onto AttackMove
                // (the order_intent set after the move issues is the real driver).
                self.queue_megamission_with_teardown(
                    *entity_id,
                    MissionType::AttackMove,
                    DockTeardown::IdleOnly,
                );
                if let Some(e) = self.substrate.entities.get_mut(*entity_id) {
                    e.attack_target = None;
                    e.passively_acquired_target = false;
                }

                // Snapshot speed, locomotor, and rules data in one lookup.
                let Some(info) = self.resolve_move_info(*entity_id, rules) else {
                    return false;
                };
                // Chrono Miners drive normally for player commands.
                let use_teleport_move = !info.is_harvester
                    && (info.loco_kind == Some(LocomotorKind::Teleport) || info.is_teleporter);

                let (entity_blocks, entity_block_map) = bump_crush::build_entity_block_set(
                    &self.substrate.entities,
                    command_owner,
                    &self.house_alliances,
                    &self.interner,
                    rules,
                );
                let default_general = crate::rules::ruleset::GeneralRules::default();
                let general_rules_ref = rules.map(|r| &r.general).unwrap_or(&default_general);
                let issued = if use_teleport_move {
                    // `use_teleport_move` excludes harvesters, so is_harvester=false.
                    teleport_movement::issue_teleport_command(
                        &mut self.substrate.entities,
                        *entity_id,
                        (*target_rx, *target_ry),
                        general_rules_ref,
                        false,
                        self.session.binary_frame,
                    )
                } else if info.loco_layer == MovementLayer::Air {
                    // Air units fly in straight lines.
                    let ok = self.issue_air_cell_destination(
                        *entity_id,
                        (*target_rx, *target_ry),
                        info.speed,
                        rules,
                    );
                    if ok {
                        if let Some(e) = self.substrate.entities.get_mut(*entity_id) {
                            if e.aircraft_mission.is_some() {
                                e.aircraft_mission =
                                    Some(crate::sim::aircraft::AircraftMission::Move {
                                        sub_state: 0,
                                    });
                            }
                        }
                    }
                    ok
                } else {
                    let Some(grid) = path_grid else { return false };
                    let cost_grid = self.terrain_costs.get(&info.speed_type);
                    let blocker_neighbor_counts =
                        bump_crush::build_blocker_neighbor_counts_with_overlays(
                            &self.substrate.entities,
                            grid.width(),
                            grid.height(),
                            self.resolved_terrain.as_ref(),
                            self.overlay_grid.as_ref(),
                            overlay_registry,
                            &self.interner,
                            rules,
                        );
                    movement::issue_move_command_with_layered(
                        &mut self.substrate.entities,
                        grid,
                        *entity_id,
                        (*target_rx, *target_ry),
                        info.speed,
                        *queue,
                        cost_grid,
                        Some(&entity_blocks),
                        self.resolved_terrain.as_ref(),
                        self.zone_grid.as_ref(),
                        Some(&entity_block_map),
                        info.mover_is_crusher,
                        Some(&blocker_neighbor_counts),
                        self.playfield_bounds,
                        Some(&mut self.substrate.cell_occupation),
                        crate::sim::movement::DestinationTiming::from_rules(
                            self.session.binary_frame,
                            rules.into(),
                        ),
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
            Command::Guard {
                entity_id,
                target_id,
            } => self.apply_guard_command(command_owner, *entity_id, *target_id, rules),
            Command::DeployMcv { entity_id } => {
                let Some(rules) = rules else { return false };
                if !self.entity_owned_by_id(command_owner, *entity_id) {
                    return false;
                }
                if self
                    .substrate
                    .entities
                    .get(*entity_id)
                    .is_some_and(|entity| {
                        self.object_type(entity.type_ref(), rules)
                            .is_some_and(|obj| obj.enslaves.is_some() && obj.deploys_into.is_some())
                    })
                {
                    return crate::sim::slave_miner::deploy_slave_miner_with_overlay_context(
                        self,
                        *entity_id,
                        rules,
                        overlay_registry,
                    )
                    .is_some();
                }
                crate::sim::mcv_deploy::issue_order(self, *entity_id, rules)
            }
            Command::UndeployBuilding { entity_id } => {
                let Some(rules) = rules else { return false };
                if !self.entity_owned_by_id(command_owner, *entity_id) {
                    return false;
                }
                if self
                    .substrate
                    .entities
                    .get(*entity_id)
                    .is_some_and(|entity| {
                        self.object_type(entity.type_ref(), rules)
                            .is_some_and(|obj| {
                                obj.enslaves.is_some() && obj.undeploys_into.is_some()
                            })
                    })
                {
                    return crate::sim::slave_miner::undeploy_slave_miner_with_overlay_context(
                        self,
                        *entity_id,
                        rules,
                        overlay_registry,
                    )
                    .is_some();
                }
                self.undeploy_building(*entity_id, rules)
            }
            Command::ToggleInfantryDeploy { entity_id } => {
                if !self.entity_owned_by_id(command_owner, *entity_id) {
                    return false;
                }
                let Some(rules) = rules else { return false };
                // INI gate: only DeployFire=yes types respond.
                let type_str = match self.substrate.entities.get(*entity_id) {
                    Some(e) => self.interner.resolve(e.type_ref()).to_string(),
                    None => return false,
                };
                let Some(obj) = rules.object(&type_str) else {
                    return false;
                };
                if !obj.deploy_fire {
                    return false;
                }
                let deploy_sound = obj.deploy_sound.clone();
                let undeploy_sound = obj.undeploy_sound.clone();
                // Per-type animation duration from artmd.ini sequence frame
                // counts. Fall back to DEPLOY_DEFAULT_TICKS when the art
                // section or sequence is missing.
                let art_entry = rules
                    .art_registry
                    .resolve_metadata_entry(&type_str, &obj.image);
                let deploying_ticks = crate::sim::deploy::compute_anim_ticks(
                    art_entry,
                    crate::sim::deploy::DeployPhaseKind::Deploying,
                );
                let undeploying_ticks = crate::sim::deploy::compute_anim_ticks(
                    art_entry,
                    crate::sim::deploy::DeployPhaseKind::Undeploying,
                );

                let Some(entity) = self.substrate.entities.get_mut(*entity_id) else {
                    return false;
                };
                let (rx, ry) = (entity.position.rx, entity.position.ry);
                let new_phase: Option<crate::sim::deploy::DeployPhase>;
                let mut emit_deploy_sound = false;
                let mut emit_undeploy_sound = false;
                match entity.deploy_state {
                    None => {
                        new_phase = Some(crate::sim::deploy::DeployPhase::Deploying {
                            ticks_remaining: deploying_ticks,
                        });
                        emit_deploy_sound = true;
                        // Deploy begins: the locomotor powers down for the
                        // duration. Undeploy completing powers it back on.
                        if let Some(loco) = entity.locomotor.as_mut() {
                            loco.power_off();
                        }
                    }
                    Some(crate::sim::deploy::DeployPhase::Deployed) => {
                        new_phase = Some(crate::sim::deploy::DeployPhase::Undeploying {
                            ticks_remaining: undeploying_ticks,
                        });
                        emit_undeploy_sound = true;
                        // Belt-and-braces: clear any stale movement target.
                        entity.movement_target = None;
                    }
                    Some(crate::sim::deploy::DeployPhase::Deploying { .. })
                    | Some(crate::sim::deploy::DeployPhase::Undeploying { .. }) => {
                        return false;
                    }
                }
                // Sound plays BEFORE state field write — matches the original's
                // Do_Action ordering (voc cue precedes the Doing-field mutation).
                if emit_deploy_sound {
                    if let Some(sound_name) = deploy_sound {
                        let sound_id = self.interner.intern(&sound_name);
                        self.sound_events
                            .push(crate::sim::world::SimSoundEvent::EntityDeployed {
                                deploy_sound_id: sound_id,
                                rx,
                                ry,
                            });
                    }
                }
                if emit_undeploy_sound {
                    if let Some(sound_name) = undeploy_sound {
                        let sound_id = self.interner.intern(&sound_name);
                        self.sound_events.push(
                            crate::sim::world::SimSoundEvent::EntityUndeployed {
                                undeploy_sound_id: sound_id,
                                rx,
                                ry,
                            },
                        );
                    }
                }
                entity.deploy_state = new_phase;
                true
            }
            Command::SetRally {
                owner,
                rx,
                ry,
                producer_ids,
            } => {
                production::set_rally_point_for_owner(self, owner, *rx, *ry);
                self.set_rally_target_for_producers(command_owner, producer_ids, *rx, *ry, rules)
            }
            Command::QueueProduction { owner, type_id, .. } => {
                let Some(rules) = rules else { return false };
                let owner_s = self.interner.resolve(*owner).to_string();
                let type_s = self.interner.resolve(*type_id).to_string();
                production::enqueue_by_type(self, rules, &owner_s, &type_s)
            }
            Command::TogglePauseProduction { owner, category } => {
                let owner_s = self.interner.resolve(*owner).to_string();
                production::toggle_pause_for_owner_category(self, &owner_s, *category)
            }
            Command::CycleProducerFocus { owner, category } => {
                let Some(rules) = rules else { return false };
                let owner_s = self.interner.resolve(*owner).to_string();
                production::cycle_active_producer_for_owner_category(
                    self, rules, &owner_s, *category,
                )
            }
            Command::PlaceReadyBuilding {
                owner,
                type_id,
                rx,
                ry,
            } => {
                let Some(rules) = rules else { return false };
                if self.interner.get(command_owner) != Some(*owner) {
                    return false;
                }
                let owner_s = self.interner.resolve(*owner).to_string();
                let type_s = self.interner.resolve(*type_id).to_string();
                let placed = production::place_ready_building_with_overlays(
                    self,
                    rules,
                    &owner_s,
                    &type_s,
                    *rx,
                    *ry,
                    path_grid,
                    height_map,
                    overlay_registry,
                );
                if !placed {
                    // `HouseClass::Place_Production 0x004FB369..0x004FB377`:
                    // a placement event whose Unlimbo fails speaks
                    // `EVA_CannotDeployHere` when `this == PlayerPtr`; the
                    // app applies the local-owner half.
                    self.sound_events
                        .push(SimSoundEvent::CannotDeployHere { owner: *owner });
                }
                placed
            }
            Command::CancelLastProduction { owner } => {
                let Some(rules) = rules else { return false };
                let owner_s = self.interner.resolve(*owner).to_string();
                production::cancel_last_for_owner(self, rules, &owner_s)
            }
            Command::CancelProductionByType { owner, type_id } => {
                let Some(rules) = rules else { return false };
                let owner_s = self.interner.resolve(*owner).to_string();
                let type_s = self.interner.resolve(*type_id).to_string();
                production::cancel_by_type_for_owner(self, rules, &owner_s, &type_s)
            }
            Command::SellBuilding { entity_id } => {
                let Some(rules) = rules else { return false };
                if !self.entity_owned_by_id(command_owner, *entity_id) {
                    return false;
                }
                production::sell_building(self, rules, *entity_id)
            }
            Command::SellWallAtCell { x, y } => {
                let (Some(rules), Some(overlays)) = (rules, overlay_registry) else {
                    return false;
                };
                self.sell_wall_at_cell(command_owner, *x, *y, rules, path_grid, overlays)
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
            Command::ToggleRepair { entity_id } => {
                if !self.entity_owned_by_id(command_owner, *entity_id) {
                    return false;
                }
                rules.is_some_and(|rules| production::toggle_repair(self, rules, *entity_id))
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
                    if explicit_refinery_changed {
                        miner.dock_queued = false;
                        miner.dock_phase = crate::sim::miner::RefineryDockPhase::Approach;
                    }
                }
                miner.forced_return = true;
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
                let depot_info = self.substrate.entities.get(*depot_id).and_then(|depot| {
                    if !command_owner.eq_ignore_ascii_case(self.interner.resolve(depot.owner())) {
                        return None;
                    }
                    let obj = self.object_type(depot.type_ref(), rules)?;
                    if !obj.unit_repair {
                        return None;
                    }
                    Some((depot.position.rx, depot.position.ry, obj.foundation.clone()))
                });
                let Some((depot_rx, depot_ry, foundation)) = depot_info else {
                    return false;
                };
                // Validate entity is a unit or infantry (not structure/aircraft).
                let entity_ok = self.substrate.entities.get(*entity_id).is_some_and(|e| {
                    matches!(
                        e.category,
                        crate::map::entities::EntityCategory::Unit
                            | crate::map::entities::EntityCategory::Infantry
                    ) && self
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
                // Cancel any existing depot reservation, then retask onto Enter.
                self.queue_megamission_with_teardown(
                    *entity_id,
                    MissionType::Enter,
                    DockTeardown::Depot,
                );
                // Set dock state and issue move toward depot.
                let (dock_rx, dock_ry) =
                    building_dock::depot_dock_cell(depot_rx, depot_ry, &foundation);
                if let Some(e) = self.substrate.entities.get_mut(*entity_id) {
                    e.attack_target = None;
                    e.passively_acquired_target = false;
                    e.order_intent = None;
                    e.dock_state = Some(DockState::approach(*depot_id));
                }
                // Issue movement toward dock cell.
                let info = self.resolve_move_info(*entity_id, Some(rules));
                let speed = info
                    .as_ref()
                    .map(|i| i.speed)
                    .unwrap_or(ra2_speed_to_leptons_per_second(4));
                let speed_type = info
                    .as_ref()
                    .map(|i| i.speed_type)
                    .unwrap_or(SpeedType::Track);
                let crusher = info.as_ref().map_or(false, |i| i.mover_is_crusher);
                let (entity_blocks, entity_block_map) = bump_crush::build_entity_block_set(
                    &self.substrate.entities,
                    command_owner,
                    &self.house_alliances,
                    &self.interner,
                    Some(rules),
                );
                if let Some(grid) = path_grid {
                    let cost_grid = self.terrain_costs.get(&speed_type);
                    let blocker_neighbor_counts =
                        bump_crush::build_blocker_neighbor_counts_with_overlays(
                            &self.substrate.entities,
                            grid.width(),
                            grid.height(),
                            self.resolved_terrain.as_ref(),
                            self.overlay_grid.as_ref(),
                            overlay_registry,
                            &self.interner,
                            Some(rules),
                        );
                    movement::issue_move_command_with_layered(
                        &mut self.substrate.entities,
                        grid,
                        *entity_id,
                        (dock_rx, dock_ry),
                        speed,
                        false,
                        cost_grid,
                        Some(&entity_blocks),
                        self.resolved_terrain.as_ref(),
                        self.zone_grid.as_ref(),
                        Some(&entity_block_map),
                        crusher,
                        Some(&blocker_neighbor_counts),
                        self.playfield_bounds,
                        Some(&mut self.substrate.cell_occupation),
                        crate::sim::movement::DestinationTiming::from_rules(
                            self.session.binary_frame,
                            rules.into(),
                        ),
                    );
                }
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
                        path_grid,
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
                // Retask onto Enter (no dock reservation touched); the legacy
                // field clears below stay authoritative.
                self.queue_megamission_with_teardown(
                    *passenger_id,
                    MissionType::Enter,
                    DockTeardown::None,
                );
                // Clear existing state on the passenger.
                if let Some(e) = self.substrate.entities.get_mut(*passenger_id) {
                    e.attack_target = None;
                    e.passively_acquired_target = false;
                    e.order_intent = None;
                    e.dock_state = None;
                    e.passenger_role = passenger::PassengerRole::Boarding {
                        target_transport_id: *transport_id,
                        phase: passenger::BoardingPhase::Approach,
                    };
                }
                // Issue movement toward transport cell.
                let info = self.resolve_move_info(*passenger_id, Some(rules));
                let speed = info
                    .as_ref()
                    .map(|i| i.speed)
                    .unwrap_or(ra2_speed_to_leptons_per_second(4));
                let speed_type = info
                    .as_ref()
                    .map(|i| i.speed_type)
                    .unwrap_or(SpeedType::Track);
                let crusher = info.as_ref().map_or(false, |i| i.mover_is_crusher);
                let (entity_blocks, entity_block_map) = bump_crush::build_entity_block_set(
                    &self.substrate.entities,
                    command_owner,
                    &self.house_alliances,
                    &self.interner,
                    Some(rules),
                );
                if let Some(grid) = path_grid {
                    let cost_grid = self.terrain_costs.get(&speed_type);
                    let blocker_neighbor_counts =
                        bump_crush::build_blocker_neighbor_counts_with_overlays(
                            &self.substrate.entities,
                            grid.width(),
                            grid.height(),
                            self.resolved_terrain.as_ref(),
                            self.overlay_grid.as_ref(),
                            overlay_registry,
                            &self.interner,
                            Some(rules),
                        );
                    movement::issue_move_command_with_layered(
                        &mut self.substrate.entities,
                        grid,
                        *passenger_id,
                        (trx, try_),
                        speed,
                        false,
                        cost_grid,
                        Some(&entity_blocks),
                        self.resolved_terrain.as_ref(),
                        self.zone_grid.as_ref(),
                        Some(&entity_block_map),
                        crusher,
                        Some(&blocker_neighbor_counts),
                        self.playfield_bounds,
                        Some(&mut self.substrate.cell_occupation),
                        crate::sim::movement::DestinationTiming::from_rules(
                            self.session.binary_frame,
                            rules.into(),
                        ),
                    );
                }
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
                        // Garrison eviction stays on the per-tick order path.
                        if let Some(e) = self.substrate.entities.get_mut(*transport_id) {
                            e.order_intent = Some(OrderIntent::Unloading);
                        }
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
                            let now = self.session.binary_frame;
                            let _ = self.mission_assign_exact(
                                *transport_id,
                                crate::sim::mission::MissionId::from_known(MissionType::Unload),
                                now,
                            );
                        } else {
                            self.queue_megamission_with_teardown(
                                *transport_id,
                                MissionType::Unload,
                                DockTeardown::All,
                            );
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
                let Some(e) = self.substrate.entities.get_mut(*entity_id) else {
                    return false;
                };
                let Some(ref mut miner) = e.miner else {
                    return false;
                };
                miner.target_ore_cell = Some((*target_rx, *target_ry));
                // Clear in-progress movement so the miner re-paths to the new target.
                e.movement_target = None;
                // A harvest order is a MEGAMISSION like any other: it ends a
                // refinery handshake in progress (`miner_dock::break_for_retask`).
                crate::sim::miner::miner_dock::break_for_retask(self, *entity_id);
                // Commit the Harvest mission and the MoveToOre cursor of
                // record. Native (EventClass::Execute MEGAMISSION,
                // disassembled 2026-09-05): the client's mission byte passes
                // through `FootClass 0x004DF0E0` (vtable `+0x4A4`, unchanged
                // unless 0x1D) into `Queue_Mission(mission, 0)` at
                // `0x004C73B9`, then clears SuspendedNavCom/SuspendedTarCom.
                // VERA assigns instead of queueing — the miner therefore
                // leaves Guard on this frame rather than at the host's next
                // Ready-to-Commence step (a one-frame shape difference,
                // VERA-internal). This is the production path that returns a
                // war miner parked on Guard by the Harvest idle tail to work.
                let now = self.session.binary_frame;
                let _ = self.mission_assign_exact(
                    *entity_id,
                    crate::sim::mission::MissionId::from_known(MissionType::Harvest),
                    now,
                );
                if let Some(e) = self.substrate.entities.get_mut(*entity_id) {
                    e.mission
                        .set_handler_state(crate::sim::miner::MinerState::MoveToOre.cursor());
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
                        Some((b.position.rx, b.position.ry, b.owner()))
                    });
                let Some((trx, try_, target_owner)) = target_info else {
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
                // Retask onto Sabotage (no dock reservation touched); the legacy
                // field clears below stay authoritative.
                self.queue_megamission_with_teardown(
                    *attacker_id,
                    MissionType::Sabotage,
                    DockTeardown::None,
                );
                // Clear conflicting state and set c4_plant.
                if let Some(e) = self.substrate.entities.get_mut(*attacker_id) {
                    e.attack_target = None;
                    e.passively_acquired_target = false;
                    e.order_intent = None;
                    e.dock_state = None;
                    e.capture_target = None;
                    e.c4_plant = Some(crate::sim::components::C4PlantState {
                        target_building_id: *target_building_id,
                    });
                }
                // Issue movement toward the building's cell.
                let info = self.resolve_move_info(*attacker_id, Some(rules));
                let speed = info
                    .as_ref()
                    .map(|i| i.speed)
                    .unwrap_or(ra2_speed_to_leptons_per_second(4));
                let speed_type = info
                    .as_ref()
                    .map(|i| i.speed_type)
                    .unwrap_or(crate::rules::locomotor_type::SpeedType::Foot);
                let crusher = info.as_ref().map_or(false, |i| i.mover_is_crusher);
                let (entity_blocks, entity_block_map) =
                    crate::sim::movement::bump_crush::build_entity_block_set(
                        &self.substrate.entities,
                        command_owner,
                        &self.house_alliances,
                        &self.interner,
                        Some(rules),
                    );
                if let Some(grid) = path_grid {
                    let cost_grid = self.terrain_costs.get(&speed_type);
                    let blocker_neighbor_counts =
                        crate::sim::movement::bump_crush::build_blocker_neighbor_counts_with_overlays(
                            &self.substrate.entities,
                            grid.width(),
                            grid.height(),
                            self.resolved_terrain.as_ref(),
                            self.overlay_grid.as_ref(),
                            overlay_registry,
                            &self.interner,
                            Some(rules),
                        );
                    movement::issue_move_command_with_layered(
                        &mut self.substrate.entities,
                        grid,
                        *attacker_id,
                        (trx, try_),
                        speed,
                        false,
                        cost_grid,
                        Some(&entity_blocks),
                        self.resolved_terrain.as_ref(),
                        self.zone_grid.as_ref(),
                        Some(&entity_block_map),
                        crusher,
                        Some(&blocker_neighbor_counts),
                        self.playfield_bounds,
                        Some(&mut self.substrate.cell_occupation),
                        crate::sim::movement::DestinationTiming::from_rules(
                            self.session.binary_frame,
                            rules.into(),
                        ),
                    );
                }
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
                if self
                    .substrate
                    .entities
                    .get(*engineer_id)
                    .is_some_and(|e| e.is_deployed())
                {
                    return false;
                }
                // Validate engineer has Engineer=yes flag.
                let eng_ok = self.substrate.entities.get(*engineer_id).and_then(|e| {
                    let obj = self.object_type(e.type_ref(), rules)?;
                    obj.engineer.then_some(())
                });
                if eng_ok.is_none() {
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
                        if b.dying {
                            return None;
                        }
                        let obj = self.object_type(b.type_ref(), rules)?;
                        if !obj.capturable && !obj.bridge_repair_hut {
                            return None;
                        }
                        Some((
                            b.position.rx,
                            b.position.ry,
                            b.owner(),
                            // Building447E90 delegates ordinary targets to+48.
                            // Special Helipad/UnitRepair/Bunker +A8 docking
                            // coordinates remain the existing bounded adapter;
                            // stock CABHUT has none of those flags.
                            if obj.helipad || obj.unit_repair || obj.bunker {
                                crate::sim::movement::ground_pose::position_world_coord(&b.position)
                            } else {
                                crate::sim::movement::ground_pose::object_center_coord(b, obj)
                            },
                        ))
                    });
                let Some((trx, try_, target_owner, target_coord)) = target_info else {
                    return false;
                };
                // Must be an enemy building.
                if crate::map::houses::are_houses_friendly(
                    &self.house_alliances,
                    command_owner,
                    self.interner.resolve(target_owner),
                ) {
                    return false;
                }
                // Native order admission (actor + Destination token).
                if !self.order_actor_admits(*engineer_id)
                    || !self.order_object_token_admits(*target_building_id)
                {
                    return false;
                }
                // Retask onto Capture (no dock reservation touched); the legacy
                // field clears below stay authoritative.
                self.queue_megamission_with_teardown(
                    *engineer_id,
                    MissionType::Capture,
                    DockTeardown::None,
                );
                // Clear conflicting state and set capture target.
                if let Some(e) = self.substrate.entities.get_mut(*engineer_id) {
                    e.attack_target = None;
                    e.passively_acquired_target = false;
                    e.order_intent = None;
                    e.dock_state = None;
                    e.capture_target = Some(*target_building_id);
                    // Event4C747C -> Infantry51AA40 -> Foot4D9510 writes
                    // the actual object destination before locomotor approach.
                    e.navigation.nav_com = Some(crate::sim::components::NavTargetRef::Building {
                        id: *target_building_id,
                    });
                    e.navigation.nav_com_aux = None;
                    e.navigation.pending_arrival_clear = false;
                    movement::set_walk_destination_coord(
                        e,
                        target_coord,
                        self.resolved_terrain.as_ref(),
                    );
                }
                // Issue movement toward the building's cell.
                let info = self.resolve_move_info(*engineer_id, Some(rules));
                let speed = info
                    .as_ref()
                    .map(|i| i.speed)
                    .unwrap_or(ra2_speed_to_leptons_per_second(4));
                let speed_type = info
                    .as_ref()
                    .map(|i| i.speed_type)
                    .unwrap_or(crate::rules::locomotor_type::SpeedType::Foot);
                let crusher = info.as_ref().map_or(false, |i| i.mover_is_crusher);
                let (entity_blocks, entity_block_map) =
                    crate::sim::movement::bump_crush::build_entity_block_set(
                        &self.substrate.entities,
                        command_owner,
                        &self.house_alliances,
                        &self.interner,
                        Some(rules),
                    );
                if let Some(grid) = path_grid {
                    let cost_grid = self.terrain_costs.get(&speed_type);
                    let blocker_neighbor_counts =
                        crate::sim::movement::bump_crush::build_blocker_neighbor_counts_with_overlays(
                            &self.substrate.entities,
                            grid.width(),
                            grid.height(),
                            self.resolved_terrain.as_ref(),
                            self.overlay_grid.as_ref(),
                            overlay_registry,
                            &self.interner,
                            Some(rules),
                        );
                    movement::issue_move_command_with_destination(
                        &mut self.substrate.entities,
                        grid,
                        *engineer_id,
                        (trx, try_),
                        speed,
                        false,
                        cost_grid,
                        Some(&entity_blocks),
                        self.resolved_terrain.as_ref(),
                        self.zone_grid.as_ref(),
                        Some(&entity_block_map),
                        crusher,
                        Some(&blocker_neighbor_counts),
                        self.playfield_bounds,
                        Some(&mut self.substrate.cell_occupation),
                        Some((
                            crate::sim::components::NavTargetRef::Building {
                                id: *target_building_id,
                            },
                            target_coord,
                        )),
                        crate::sim::movement::DestinationTiming::from_rules(
                            self.session.binary_frame,
                            rules.into(),
                        ),
                    );
                }
                true
            }
            Command::LaunchSuperWeapon {
                sw_type_id,
                target_rx,
                target_ry,
            } => {
                if !self.session.game_options.super_weapons {
                    return false;
                }
                let owner_iid = self.interner.intern(command_owner);
                let sw_type_str = self.interner.resolve(*sw_type_id).to_string();

                // Look up the instance and verify it's ready.
                let is_ready = self
                    .super_weapons
                    .get(&owner_iid)
                    .and_then(|weapons| weapons.get(sw_type_id))
                    .map_or(false, |inst| inst.is_active && inst.is_ready);
                if !is_ready {
                    log::warn!(
                        "LaunchSuperWeapon '{}' by '{}' — not ready",
                        sw_type_str,
                        command_owner,
                    );
                    return false;
                }

                // Look up the type to determine dispatch kind.
                let Some(sw_type) = rules.and_then(|r| r.super_weapon(&sw_type_str)) else {
                    return false;
                };
                let kind = sw_type.kind;
                let recharge = sw_type.recharge_time_frames;

                // Dispatch based on kind.
                let success = match kind {
                    crate::rules::superweapon_type::SuperWeaponKind::LightningStorm => {
                        let rules = rules.unwrap();
                        crate::sim::superweapon::lightning_storm::start(
                            self,
                            rules,
                            owner_iid,
                            *target_rx,
                            *target_ry,
                            *sw_type_id,
                        )
                    }
                    crate::rules::superweapon_type::SuperWeaponKind::IronCurtain => {
                        let rules = rules.unwrap();
                        crate::sim::superweapon::iron_curtain::launch(
                            self,
                            rules,
                            owner_iid,
                            *target_rx,
                            *target_ry,
                            *sw_type_id,
                            overlay_registry,
                        )
                    }
                    crate::rules::superweapon_type::SuperWeaponKind::ForceShield => {
                        let rules = rules.unwrap();
                        crate::sim::superweapon::force_shield::launch(
                            self,
                            rules,
                            owner_iid,
                            *target_rx,
                            *target_ry,
                            *sw_type_id,
                        )
                    }
                    crate::rules::superweapon_type::SuperWeaponKind::GeneticConverter => {
                        let rules = rules.unwrap();
                        crate::sim::superweapon::genetic_converter::launch(
                            self,
                            rules,
                            owner_iid,
                            *target_rx,
                            *target_ry,
                            *sw_type_id,
                            overlay_registry,
                        )
                    }
                    crate::rules::superweapon_type::SuperWeaponKind::PsychicReveal => {
                        let rules = rules.unwrap();
                        crate::sim::superweapon::psychic_reveal::launch(
                            self,
                            rules,
                            owner_iid,
                            *target_rx,
                            *target_ry,
                            *sw_type_id,
                        )
                    }
                    crate::rules::superweapon_type::SuperWeaponKind::ParaDrop => {
                        let rules = rules.unwrap();
                        crate::sim::superweapon::paradrop::launch(
                            self,
                            rules,
                            owner_iid,
                            *target_rx,
                            *target_ry,
                            crate::sim::superweapon::paradrop::ParaDropKind::Generic,
                            *sw_type_id,
                            path_grid,
                        )
                    }
                    crate::rules::superweapon_type::SuperWeaponKind::AmerParaDrop => {
                        let rules = rules.unwrap();
                        crate::sim::superweapon::paradrop::launch(
                            self,
                            rules,
                            owner_iid,
                            *target_rx,
                            *target_ry,
                            crate::sim::superweapon::paradrop::ParaDropKind::American,
                            *sw_type_id,
                            path_grid,
                        )
                    }
                    other => {
                        log::warn!("SuperWeapon kind {:?} not yet implemented", other);
                        false
                    }
                };

                if success {
                    // Reset the instance — restart charging.
                    if let Some(weapons) = self.super_weapons.get_mut(&owner_iid) {
                        if let Some(inst) = weapons.get_mut(sw_type_id) {
                            inst.reset_after_fire(recharge, self.session.binary_frame);
                        }
                    }
                }
                success
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
                );
                // Retask onto Enter (no dock reservation), mark the unit as
                // approaching THIS bunker (the install machine's keep-alive gate).
                self.queue_megamission_with_teardown(
                    *unit_id,
                    MissionType::Enter,
                    DockTeardown::None,
                );
                if let Some(e) = self.substrate.entities.get_mut(*unit_id) {
                    e.attack_target = None;
                    e.passively_acquired_target = false;
                    e.order_intent = None;
                    e.dock_state = None;
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
                    let crusher = info.as_ref().map_or(false, |i| i.mover_is_crusher);
                    let (entity_blocks, entity_block_map) = bump_crush::build_entity_block_set(
                        &self.substrate.entities,
                        command_owner,
                        &self.house_alliances,
                        &self.interner,
                        Some(rules),
                    );
                    if let Some(grid) = path_grid {
                        let cost_grid = self.terrain_costs.get(&speed_type);
                        let blocker_neighbor_counts =
                            bump_crush::build_blocker_neighbor_counts_with_overlays(
                                &self.substrate.entities,
                                grid.width(),
                                grid.height(),
                                self.resolved_terrain.as_ref(),
                                self.overlay_grid.as_ref(),
                                overlay_registry,
                                &self.interner,
                                Some(rules),
                            );
                        movement::issue_move_command_with_layered(
                            &mut self.substrate.entities,
                            grid,
                            *unit_id,
                            (brx, bry),
                            speed,
                            false,
                            cost_grid,
                            Some(&entity_blocks),
                            self.resolved_terrain.as_ref(),
                            self.zone_grid.as_ref(),
                            Some(&entity_block_map),
                            crusher,
                            Some(&blocker_neighbor_counts),
                            self.playfield_bounds,
                            Some(&mut self.substrate.cell_occupation),
                            crate::sim::movement::DestinationTiming::from_rules(
                                self.session.binary_frame,
                                rules.into(),
                            ),
                        );
                    }
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
                crate::sim::docking::bunker_link::release_normal(
                    self, *bunker_id, rules, path_grid,
                );
                true
            }
        }
    }

    fn set_rally_target_for_producers(
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
            let eligible = self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| {
                    entity.category == crate::map::entities::EntityCategory::Structure
                        && command_owner.eq_ignore_ascii_case(self.interner.resolve(entity.owner()))
                        && self
                            .object_type(entity.type_ref(), rules)
                            .is_some_and(|obj| obj.has_rally_line())
                });
            if eligible {
                if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                    entity.rally_target = Some((rx, ry));
                }
            }
        }
        // `EVA_NewRallyPointEstablished` is not a sim fact: `BuildingClass::
        // SetRallyPoint 0x00443A69` speaks inside the click handler, after the
        // rally EventClass is pushed and before it executes — the app owns it.
        true
    }

    /// Drop the entity's depot contact slot before a new order retasks it.
    /// The depot keeps no queue (admission is `radio_contacts` capacity), so
    /// this is a BREAK of the unit↔depot link when one exists. VERA-internal
    /// timing: native tears the link down later (PerCellProcess `0x08` /
    /// the depot's repair mission), gamemd equivalent UNCHECKED.
    pub(crate) fn cancel_depot_dock(&mut self, entity_id: u64) {
        let depot_id = self
            .substrate
            .entities
            .get(entity_id)
            .and_then(|e| e.dock_state.as_ref().map(|ds| ds.dock_building_id));
        if let Some(depot_id) = depot_id {
            let linked = self
                .substrate
                .entities
                .get(entity_id)
                .is_some_and(|e| e.radio_contacts.contains(depot_id));
            if linked {
                let _ = crate::sim::radio::transmit(
                    self,
                    entity_id,
                    depot_id,
                    crate::sim::radio::RadioMessage::Break,
                    crate::sim::radio::RadioPayload::default(),
                );
            }
        }
    }

    /// Cancel aircraft dock reservation if in ReturnToBase or WaitForDock phase.
    pub(crate) fn cancel_aircraft_dock(&mut self, entity_id: u64) {
        if let Some(e) = self.substrate.entities.get(entity_id) {
            if let Some(ref ammo) = e.aircraft_ammo {
                use crate::sim::docking::aircraft_dock::AircraftDockPhase;
                if matches!(
                    ammo.dock_phase,
                    Some(AircraftDockPhase::ReturnToBase) | Some(AircraftDockPhase::WaitForDock)
                ) {
                    self.production.airfield_docks.release(entity_id);
                }
            }
        }
    }

    /// Clear aircraft dock phase on an entity if interruptible (RTB/WaitForDock).
    fn clear_aircraft_dock_phase(entity: &mut crate::sim::game_entity::GameEntity) {
        if let Some(ref mut ammo) = entity.aircraft_ammo {
            use crate::sim::docking::aircraft_dock::AircraftDockPhase;
            if matches!(
                ammo.dock_phase,
                Some(AircraftDockPhase::ReturnToBase) | Some(AircraftDockPhase::WaitForDock)
            ) {
                ammo.dock_phase = None;
                ammo.target_airfield = None;
            }
        }
    }

    /// Release a DockedIdle aircraft from its helipad and trigger takeoff.
    /// Called when a docked aircraft receives a Move or Attack command.
    pub(crate) fn release_docked_idle(&mut self, entity_id: u64) {
        let Some(entity) = self.substrate.entities.get_mut(entity_id) else {
            return;
        };
        if let Some(crate::sim::aircraft::AircraftMission::DockedIdle { .. }) =
            entity.aircraft_mission
        {
            // Release dock slot.
            self.production.airfield_docks.release(entity_id);
            // Clear to Idle — the command handler will set the appropriate mission.
            entity.aircraft_mission = Some(crate::sim::aircraft::AircraftMission::Idle);
            // Trigger takeoff.
            if let Some(ref mut loco) = entity.locomotor {
                if loco.air_phase == crate::sim::movement::locomotor::AirMovePhase::Landed {
                    loco.air_phase = crate::sim::movement::locomotor::AirMovePhase::Ascending;
                }
            }
        }
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

    /// `TechnoClass::Select` then `ObjectClass::Select` — commit one object into
    /// the selection group.
    ///
    /// Caller-specific TechnoClass paths own their owner gate: bandbox and
    /// TypeSelect admit only the local house, while an ordinary click may pass
    /// a discovered nonlocal object. The final ObjectClass gates reject an
    /// object that is dead, in limbo, already selected, leaving through a
    /// chrono warp, or whose type answers no to
    /// `CanBeSelected` — i.e. `Selectable=no`, which is how the scripted
    /// paradrop/spy planes stay out of the player's hands. Without rules loaded
    /// the type answer is unknown, and the type default is yes.
    pub(crate) fn try_select_object(&mut self, stable_id: u64, rules: Option<&RuleSet>) -> bool {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return false;
        };
        if !entity.lifecycle.object_alive
            || entity.lifecycle.in_limbo
            || entity.selected
            || entity
                .teleport_state
                .as_ref()
                .is_some_and(|teleport| teleport.warp_out_active())
        {
            return false;
        }
        let type_ref = entity.type_ref();
        let selectable = rules.is_none_or(|r| {
            r.object(self.interner.resolve(type_ref))
                .is_none_or(|obj| obj.selectable)
        });
        if !selectable {
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
    /// * **Five arms still ungated.** `Simulation::command_uses_megamission`
    ///   already enumerates which VERA commands are MEGAMISSION-shaped — it is
    ///   what splits due commands into the non-MEGAMISSION pass and the staged
    ///   batch, mirroring opcode 0x04 in `net::lockstep`. Checked against it,
    ///   `MinerReturn`, `HarvestCell`, `UnloadPassengers`, `EjectBunker` and
    ///   `ToggleInfantryDeploy` carry only the ownership test. Trigger: issuing
    ///   one of those to a dying or limboed actor. Player effect: the order runs
    ///   where retail abandons it. Frequency: low per order, but miner orders
    ///   are among the most frequent in a match. Downstream risk: none — the
    ///   gate is a pure precondition, two lines per site.
    /// * **Duplicate-Enter is not applied to `MinerReturn`**, which is the
    ///   fourth Enter-shaped order (right-clicking your own refinery). Same
    ///   stall-and-re-approach the predicate exists to stop, on the one Enter
    ///   the player repeats most.
    /// * **Replacement is per-site here and uniform in retail.** After the gate
    ///   retail runs one sequence for EVERY MEGAMISSION: `[EDI+0x500] = 0`
    ///   (write at 0x004C7353, skipped by the `JZ` at 0x004C7351 when the field
    ///   is already zero), `TeamClass__Remove_Member` when Foot and `[+0x5D4]`
    ///   and mission != 0x10 (0x004C736B-0x004C7380), `Queue_Mission(mission, 0)`
    ///   (0x004C73B9), then `[+0x2B8] = 0` (0x004C73D7) and the manager abandon
    ///   (0x004C73E1-0x004C73EA). Only `[+0x5A8] = 0` (0x004C73C7) is Foot-gated:
    ///   the `TEST byte [EDI+0x14],0x4` at 0x004C73BF jumps to 0x004C7440, which
    ///   zeroes EBP and rejoins at 0x004C73D1 — BEFORE the other write and
    ///   before the abandon. VERA substitutes five hand-picked
    ///   `DockTeardown` subsets whose own doc calls them "the exact subset that
    ///   site cancels today" — preserved legacy, not derived. The subsets happen
    ///   to be close for Move and Attack; the divergence is structural.
    /// * **Spawn-manager abandon is modelled nowhere.** 0x004C73E1-0x004C73EA
    ///   calls 0x006B0C80 on `[actor+0x2D8]` whenever the queued mission is not
    ///   Attack, which drops the spawner's target and re-tasks its spawns.
    ///   Trigger: ordering an Aircraft Carrier or any `Spawns=` unit to move out
    ///   of a fight. Player effect: retail's planes break off, VERA's keep
    ///   attacking. Frequency: several times per naval match. Downstream risk:
    ///   none — `spawn_manager.rs` exists, it just has no order-boundary hook.
    ///   Note when wiring it: the abandon is NOT Foot-gated, so it must fire for
    ///   non-Foot spawners too.
    /// * **`TeamClass__Remove_Member` has no VERA equivalent.** Zero frequency
    ///   today (no AI teams); wrong the moment AI teams exist.
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
        if refinery.health.current == 0 || refinery.dying || refinery.building_up.is_some() {
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

    /// Apply a Guard command: anchor at current position, optionally attack a target.
    fn apply_guard_command(
        &mut self,
        command_owner: &str,
        entity_id: u64,
        target_id: Option<u64>,
        rules: Option<&RuleSet>,
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
        if let Some(tid) = target_id.filter(|&tid| self.substrate.entities.contains(tid))
            && !self.order_object_token_admits(tid)
        {
            return false;
        }
        let anchor = self
            .substrate
            .entities
            .get(entity_id)
            .map(|e| (e.position.rx, e.position.ry));
        let Some((anchor_rx, anchor_ry)) = anchor else {
            return false;
        };
        // Decide before mutating: the alliance test below can still reject, and
        // clearing the movement target first would stop the unit on an order
        // that then fails.
        if let Some(tid) = target_id.filter(|&tid| self.substrate.entities.contains(tid))
            && !self.can_attack_target_by_id(entity_id, tid)
        {
            return false;
        }
        if let Some(e) = self.substrate.entities.get_mut(entity_id) {
            e.movement_target = None;
        }
        match target_id.filter(|&tid| self.substrate.entities.contains(tid)) {
            Some(tid) => {
                let issued = combat::issue_attack_command(
                    &mut self.substrate.entities,
                    entity_id,
                    tid,
                    rules,
                    &self.interner,
                );
                if issued {
                    if let Some(e) = self.substrate.entities.get_mut(entity_id) {
                        e.order_intent = Some(OrderIntent::Guard {
                            anchor_rx,
                            anchor_ry,
                        });
                    }
                }
                issued
            }
            None => {
                if let Some(e) = self.substrate.entities.get_mut(entity_id) {
                    e.attack_target = None;
                    e.passively_acquired_target = false;
                    e.order_intent = Some(OrderIntent::Guard {
                        anchor_rx,
                        anchor_ry,
                    });
                }
                true
            }
        }
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
    use crate::sim::miner::{Miner, MinerConfig, MinerKind, MinerState, RefineryDockPhase};
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
            rules.general.flight_level,
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
            .encode_megamission_move_record(local, 42, 34, 12)
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
                    group_id: None,
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
        assert_eq!(sim.encode_megamission_move_record(absent, 42, 10, 20), None);

        let overflow_id = i32::MAX as u64 + 1;
        gsi_16_01_insert_identity_entity(&mut sim, overflow_id, local);
        assert_eq!(
            sim.encode_megamission_move_record(local, overflow_id, 10, 20),
            None
        );
        assert_eq!(
            sim.encode_megamission_move_record(local, 42, u16::MAX, 20),
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
            bump_crush::CrushCapability::new(true, false)
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

    #[test]
    fn player_drive_move_command_passes_zone_grid_to_path_search() {
        let rules = amcv_move_rules();
        let mut sim = Simulation::new();
        spawn_rule_backed_unit(&mut sim, 1, "AMCV", &rules);
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        sim.zone_grid = Some(crate::sim::pathfinding::zone_map::ZoneGrid::build(
            &grid,
            &BTreeMap::new(),
            5,
            1,
        ));
        crate::sim::movement::reset_path_search_used_zone_grid_marker();

        let applied = sim.apply_command(
            "Americans",
            &Command::Move {
                entity_id: 1,
                target_rx: 25,
                target_ry: 20,
                queue: false,
                group_id: None,
            },
            Some(&rules),
            Some(&grid),
            &BTreeMap::new(),
        );

        assert!(applied);
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

        assert!(sim.order_actor_admits(1));
        assert!(sim.apply_command(
            "Americans",
            &Command::Move {
                entity_id: 1,
                target_rx: 25,
                target_ry: 20,
                queue: false,
                group_id: None,
            },
            Some(&rules),
            Some(&grid),
            &BTreeMap::new(),
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

    /// Guard branches on mission 0x0B (Area_Guard) at 0x004C73EF, so it travels
    /// the same MEGAMISSION arm as Move and carries the same admission gate —
    /// and, like every arm there, a failure must abandon the order having
    /// touched nothing.
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
                target_id: None,
            },
            Some(&rules),
            None,
            &BTreeMap::new(),
        ));
        let actor = sim.substrate.entities.get(1).unwrap();
        assert!(
            actor.order_intent.is_none(),
            "a rejected guard order must not have written anything"
        );
        assert_eq!(actor.mission.queued(), MissionId::NONE);
    }

    /// The other half of "a failure touches nothing": a Guard onto a target the
    /// alliance test refuses must leave the actor's movement alone. Clearing it
    /// before that test stopped the unit on an order that then failed.
    #[test]
    fn a_refused_guard_target_leaves_the_actor_moving() {
        let rules = amcv_move_rules();
        let mut sim = Simulation::new();
        spawn_rule_backed_unit(&mut sim, 1, "AMCV", &rules);
        // An ally: `can_attack_target_by_id` refuses it, so the order must bail.
        spawn_rule_backed_unit(&mut sim, 2, "AMCV", &rules);
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        assert!(sim.apply_command(
            "Americans",
            &Command::Move {
                entity_id: 1,
                target_rx: 25,
                target_ry: 20,
                queue: false,
                group_id: None,
            },
            Some(&rules),
            Some(&grid),
            &BTreeMap::new(),
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
                target_id: Some(2),
            },
            Some(&rules),
            Some(&grid),
            &BTreeMap::new(),
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

        assert!(!sim.order_actor_admits(1));
        assert!(!sim.apply_command(
            "Americans",
            &Command::Move {
                entity_id: 1,
                target_rx: 25,
                target_ry: 20,
                queue: false,
                group_id: None,
            },
            Some(&rules),
            Some(&grid),
            &BTreeMap::new(),
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

            assert!(!sim.order_actor_admits(1));
            assert!(!sim.apply_command(
                "Americans",
                &Command::Move {
                    entity_id: 1,
                    target_rx: 25,
                    target_ry: 20,
                    queue: false,
                    group_id: None,
                },
                Some(&rules),
                Some(&grid),
                &BTreeMap::new(),
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
            None,
            &BTreeMap::new(),
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
             Refinery=yes\n\
             [OTHERPROC]\n\
             Name=Other Refinery\n\
             Strength=900\n\
             Foundation=4x3\n\
             Refinery=yes\n",
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
        entity.miner = Some(Miner::new(MinerKind::War, &MinerConfig::default(), 0));
        sim.substrate.entities.insert(entity);
    }

    fn spawn_refinery(sim: &mut Simulation, sid: u64, type_id: &str, rx: u16, ry: u16) {
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern(type_id);
        let entity = GameEntity::new_at_frame_zero_for_test(
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
            owner,
            rx: 40,
            ry: 41,
            producer_ids: vec![3, 2, 2, 4, 5],
        };

        assert!(sim.apply_command("Americans", &command, Some(&rules), None, &BTreeMap::new()));
        assert_eq!(
            sim.substrate.entities.get(2).unwrap().rally_target,
            Some((40, 41))
        );
        assert_eq!(
            sim.substrate.entities.get(3).unwrap().rally_target,
            Some((40, 41))
        );
        assert_eq!(sim.substrate.entities.get(4).unwrap().rally_target, None);
        assert_eq!(sim.substrate.entities.get(5).unwrap().rally_target, None);
        assert_eq!(sim.houses.get(&owner).unwrap().rally_point, Some((40, 41)));
    }

    #[test]
    fn miner_return_with_explicit_refinery_seeds_clicked_target() {
        let rules = miner_return_rules();
        let mut sim = Simulation::new();
        spawn_miner(&mut sim, 1);
        spawn_refinery(&mut sim, 2, "GAREFN", 10, 10);
        spawn_refinery(&mut sim, 3, "GAREFN", 30, 30);
        {
            let miner = sim
                .substrate
                .entities
                .get_mut(1)
                .unwrap()
                .miner
                .as_mut()
                .unwrap();
            miner.reserved_refinery = Some(2);
            miner.dock_queued = true;
            miner.dock_phase = RefineryDockPhase::Unloading;
        }
        assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, 1));

        let applied = sim.apply_command(
            "Americans",
            &Command::MinerReturn {
                entity_id: 1,
                target_refinery_id: Some(3),
            },
            Some(&rules),
            None,
            &BTreeMap::new(),
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
        assert_eq!(miner.reserved_refinery, Some(3));
        assert!(miner.forced_return);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().miner_state(),
            Some(MinerState::ForcedReturn)
        );
        assert!(!miner.dock_queued);
        assert_eq!(miner.dock_phase, RefineryDockPhase::Approach);
        // The redirect BREAKs both ends. A contact left on the miner would keep
        // the old refinery's footprint enterable for it; one left on the
        // refinery would refuse every later miner.
        assert!(!crate::sim::miner::miner_dock::test_support::dock_test_is_occupied(&sim, 2));
        let miner_entity = sim.substrate.entities.get(1).unwrap();
        assert!(!miner_entity.radio_contacts.contains(2));
        assert_eq!(miner_entity.dock_entered_with, None);
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
            None,
            &BTreeMap::new(),
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
        assert!(miner.forced_return);
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
            None,
            &BTreeMap::new(),
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
        assert!(!miner.forced_return);
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
            None,
            &BTreeMap::new(),
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
            None,
            &BTreeMap::new(),
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
            None,
            &BTreeMap::new(),
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
        unit.drive_locomotion = Some(Default::default());
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
            None,
            &BTreeMap::new(),
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
            unit.drive_locomotion.as_ref().unwrap().track.turn_index,
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
            None,
            &BTreeMap::new(),
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
        // Place the tank ON the bunker cell so the install needs no pathfinding
        // (the movement subsystem is not run in this harness).
        spawn_bunkerable(&mut sim, 1, "Americans", "TANK", 10, 10);
        let unit = sim.substrate.entities.get_mut(1).unwrap();
        unit.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Drive,
            ),
        );
        unit.drive_locomotion = Some(Default::default());
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
            None,
            &BTreeMap::new(),
        ));
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().bunker_link,
            BunkerLink::Approaching(2)
        );

        // 2) Drive the install machine to Occupied. Clear facing_target each tick
        // to simulate the body turn completing (no movement subsystem here).
        for _ in 0..6 {
            tick_bunker_install(&mut sim, &rules, None);
            if let Some(u) = sim.substrate.entities.get_mut(1) {
                u.facing_target = None;
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
            None,
            &BTreeMap::new(),
        ));
        assert_eq!(sim.substrate.entities.get(2).unwrap().bunker_occupant, None);
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.bunker_link, BunkerLink::None);
        assert!(unit.in_logic_vector, "occupant stays active on eject");
        assert!(!unit.lifecycle.in_limbo);
        assert_eq!((unit.position.rx, unit.position.ry), (10, 10));
        assert_eq!(
            unit.drive_locomotion.as_ref().unwrap().track.turn_index,
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
