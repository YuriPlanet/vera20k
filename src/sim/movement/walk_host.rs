//! World placement corridors of Walk75AEC0: boundary75C117 and completion75BD7D.
//! walk_step owns the paid numeric approach; this owner supplies synchronous
//! Mark/PerCell placement before the pass tail.
use super::ground_pose;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::{
    components::DriveCoord,
    world::{FrameEffects, Simulation},
};

#[cfg(test)]
#[path = "walk_completion_tests.rs"]
mod tests;

impl Simulation {
    /// Walk75BE42..75BF64 runs after PerCell and reloads the live destination.
    /// The Infantry setter may refuse; the separate speed/Stop suffix still runs.
    /// Original executable comparison: tools/spatial_oracle/walk_completion.
    pub(crate) fn finish_walk_navigation(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        effects: FrameEffects<'_>,
    ) -> Result<(), String> {
        let Some(actor) = self.substrate.entities.get(id) else {
            return Ok(());
        };
        if !actor.lifecycle.object_alive() || actor.lifecycle.in_limbo() || actor.is_falling_down()
        {
            return Ok(());
        }
        let Some(loco) = actor
            .locomotor
            .as_ref()
            .filter(|l| l.active_kind() == crate::rules::locomotor_type::LocomotorKind::Walk)
        else {
            return Ok(());
        };
        let destination = loco.walk_destination();
        let arrived = if let Some(destination) = destination {
            // Foot+4C may supply a retained head or Tube exit after PerCell;
            // the physical XYZ alone is not the navigation coordinate owner.
            let current = self.foot_navigation_coordinate(id)?;
            (current.x / 256) as i16 == (destination.x / 256) as i16
                && (current.y / 256) as i16 == (destination.y / 256) as i16
                && current.z.wrapping_sub(destination.z).wrapping_abs()
                    < 2 * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS
        } else {
            true
        };
        if arrived {
            self.assign_null_destination(id, rules, None, effects);
            if let Some(actor) = self.substrate.entities.get_mut(id) {
                //75BF38 invokes Foot4D3710(0.0), independently of setter admission.
                actor
                    .foot_speed
                    .set_speed_fraction(crate::util::fixed_math::SIM_ZERO);
                if let Some(loco) = actor.locomotor.as_mut() {
                    loco.set_step_head(None);
                }
            }
            self.walk_stop_moving(id, rules, effects)?;
        }
        Ok(())
    }

    /// Walk75C117..75C1AE relinks current XYZ while retaining the paid head
    /// and Foot path entry. This corridor does not invoke PerCell.
    pub(crate) fn run_walk_boundary(
        &mut self,
        id: u64,
        coord: DriveCoord,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(old_cell) = self
            .substrate
            .entities
            .get(id)
            .map(|e| (e.position.rx(), e.position.ry()))
        else {
            return;
        };
        self.foot_mark_remove(id, rules, registry, effects);
        //75C12E: SetLocation (vt+0x1B4).
        ground_pose::foot_set_location(
            &mut self.substrate.entities,
            id,
            coord,
            rules,
            &self.interner,
        );
        let Some(e) = self.substrate.entities.get_mut(id) else {
            return;
        };
        let cell = (e.position.rx(), e.position.ry());
        // Recalc may replace the canonical PathGrid. Read its current cell
        // flags after REMOVE, then SetHeight samples current resolved terrain.
        let update = super::movement_bridge::resolve_cell_transition_bridge_state(
            &mut e.position,
            self.path_grid.as_deref(),
            old_cell,
            cell,
            e.on_bridge.get(),
        );
        super::movement_bridge::apply_bridge_layer_state(
            &mut e.locomotor,
            &mut e.on_bridge,
            update,
        );
        let on_bridge = e.on_bridge();
        e.position.set_height(
            on_bridge,
            0,
            self.resolved_terrain.as_ref(),
            self.path_grid.as_deref(),
        );
        // OccupancyGrid is a list projection, not an independent subcell
        // reservation chooser. The current coordinate supplies its slot.
        e.sub_cell
            .hold(crate::sim::cell_kernel::infantry_preferred_spot(
                crate::sim::cell_kernel::CellQueryPoint {
                    x: coord.x,
                    y: coord.y,
                },
            ));
        e.navigation.path_runtime.set_path_blocked(false);
        self.foot_mark_put(id, rules, registry, effects);
        //75C1EA: the boundary placement tail clears +68A after Mark(PUT).
        //This is not the post-PerCell dead/limbo/falling exit at75C1F1.
        if let Some(e) = self.substrate.entities.get_mut(id) {
            e.navigation.path_runtime.clear_scold_latch();
        }
    }

    pub(crate) fn run_completed_walk_step(
        &mut self,
        id: u64,
        head: DriveCoord,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<bool, crate::sim::world::FrameAdvanceError> {
        self.foot_mark_remove(id, rules, registry, effects);
        let Some(e) = self.substrate.entities.get_mut(id) else {
            return Ok(false);
        };
        let head = e
            .locomotor
            .as_ref()
            .and_then(|l| l.step_head())
            .unwrap_or(head);
        super::path_markers::consume_walk_path_replay(&mut e.navigation.path_replay);
        //75BDC0: SetLocation (vt+0x1B4) at the completed head.
        ground_pose::foot_set_location(
            &mut self.substrate.entities,
            id,
            head,
            rules,
            &self.interner,
        );
        let Some(e) = self.substrate.entities.get_mut(id) else {
            return Ok(false);
        };
        e.navigation
            .path_replay
            .set_reference_cell(Some((e.position.rx() as i16, e.position.ry() as i16)));
        //Infantry+1CC=5F5FA0; marked is already false, so the SetHeight0
        //receiver samples current ground+OnBridge without nested Mark calls.
        let on_bridge = e.on_bridge();
        e.position.set_height(
            on_bridge,
            0,
            self.resolved_terrain.as_ref(),
            self.path_grid.as_deref(),
        );
        //75BE11 clears only the paid-step latch before head retirement/PerCell.
        e.navigation.path_runtime.set_path_blocked(false);
        let (sub_x, sub_y) = (e.position.sub_x(), e.position.sub_y());
        e.sub_cell.hold_offset(sub_x, sub_y);
        //75BE18: FindSubCellDest(null) retires the head, clearing its raw
        //bits and marking the Foot's own.
        self.walk_find_sub_cell_dest(id, None, rules, registry, effects)
            .map_err(|cause| crate::sim::world::FrameAdvanceError {
                tick: self.session.tick(),
                binary_frame: self.session.binary_frame(),
                entity_id: id,
                cause,
            })?;
        //75BE20..75BE2F calls Foot4D3710(0.0) for the -1 path head BEFORE
        //PerCell can conceal a boarding passenger and skip the finish suffix.
        //An intermediate head retains its fraction. Native execution controls:
        //tools/spatial_oracle/foot_speed_owner.walk_arrival.json (Steam3e81a617).
        if let Some(e) = self.substrate.entities.get_mut(id)
            && e.navigation.path_replay.remaining_directions().is_empty()
        {
            e.foot_speed
                .set_speed_fraction(crate::util::fixed_math::SIM_ZERO);
        }
        //75BE3C: Per_Cell_Process(2); 75BE42..75BE69 then leaves at 75C1F1
        //for a dead, limboed or falling owner.
        let changed = self.per_cell_process(
            id,
            super::per_cell::PerCellReason::Arrival,
            rules,
            registry,
            effects,
        )?;
        if !self.track_survives(id) {
            return Ok(changed);
        }
        self.finish_walk_navigation(id, rules, effects)
            .map_err(|cause| crate::sim::world::FrameAdvanceError {
                tick: self.session.tick(),
                binary_frame: self.session.binary_frame(),
                entity_id: id,
                cause,
            })?;
        self.foot_mark_put(id, rules, registry, effects);
        //75BF77 follows the final Mark(PUT). The earlier post-PerCell exits
        //jump to75C1F1 and must retain the byte on a surviving object.
        if let Some(e) = self.substrate.entities.get_mut(id) {
            e.navigation.path_runtime.clear_scold_latch();
        }
        Ok(changed)
    }
}
