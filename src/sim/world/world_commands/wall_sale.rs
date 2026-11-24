//! Selling a wall overlay at a cell (`sell_wall_at_cell`) and the transaction-local
//! zone and path-grid repair that follows the wall's removal.

#[cfg(any(test, feature = "test-support"))]
use super::trace_wall_sell_zone_repair_step;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ruleset::RuleSet;
use crate::sim::control_authority::CommandCapability;
use crate::sim::intern::{InternKey, InternedId};
use crate::sim::overlay_grid::{
    OverlayGrid, OverlayRecalcOutcome, RecomputeResult, runtime_wall_cleanup_visit_at,
};
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::zone_incremental::ZoneRepairKind;
use crate::sim::pathfinding::zone_map::ZoneGrid;
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation, SimulationWallRuntimeHost};

impl Simulation {
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
        super::super::repair_zone_after_recalc(
            &mut self.zone_grid,
            tail_grid,
            terrain,
            self.bridge_state.as_ref(),
            *self.playfield_bounds,
            repair_cell,
            repair,
        );
    }

    /// The canonical cell and overlay a wall sale's signed CellStruct names.
    pub(super) fn wall_sale_cell(
        &self,
        x: i16,
        y: i16,
    ) -> Option<(u16, u16, crate::sim::overlay_grid::OverlayCell)> {
        // EventClass rejects only the exact packed null CellStruct. Every
        // other signed pair is resolved by the map's linear cell array (512
        // wide on retail-size maps), so out-of-range components may alias a
        // canonical slot.
        if (x, y) == (0, 0) {
            return None;
        }
        let (rx, ry) = self
            .cell_grid()
            .canonical_coord(i32::from(x), i32::from(y))?;
        let grid = self.overlay_grid.as_ref()?;
        if rx >= grid.width() || ry >= grid.height() {
            return None;
        }
        Some((rx, ry, *grid.cell(rx, ry)))
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
        command_owner: InternKey<'_>,
        granted_source: Option<(i8, InternedId)>,
        x: i16,
        y: i16,
        suppress_local_cue: bool,
        rules: &RuleSet,
        overlays: &crate::rules::overlay_types::OverlayTypeRegistry,
        effects: &mut FrameEffects<'_>,
    ) -> bool {
        let Some((rx, ry, cell)) = self.wall_sale_cell(x, y) else {
            return false;
        };
        let (Some(overlay_id), Some(wall_owner)) = (cell.overlay_id, cell.wall_owner) else {
            return false;
        };
        let Some(owner_house) = self.houses.get(&wall_owner) else {
            return false;
        };
        let native_owner_admitted = owner_house.is_human()
            || (self.session.rule_policy().counts_player_control_as_human()
                && owner_house.player_control());
        let owner_admitted = native_owner_admitted
            || granted_source.is_some_and(|(source, primary)| {
                primary != wall_owner
                    && self
                        .control_scope_for_source(source, wall_owner)
                        .permits(CommandCapability::FullControl)
            });
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
            && let Some(receiver) = self.interner.key_id(command_owner)
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
                    playfield_bounds: *self.playfield_bounds,
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
        self.stop_all_targeting_cell(rx, ry, rules, effects);

        if let Some(tail_grid) = tail_grid.map(std::sync::Arc::new) {
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
                self.rebuild_zone_grid_full(std::sync::Arc::clone(&tail_grid));
            }
            *self.path_grid = Some(tail_grid);
        }
        true
    }
}
