//! World placement corridors of Walk75AEC0: boundary75C117 and completion75BD7D.
//! The paid approach itself remains the existing Walk numeric adapter. This
//! owner supplies the synchronous Mark/PerCell boundary before the pass tail.
use super::{ground_pose, locomotor::MovementLayer};
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::{components::DriveCoord, pathfinding::PathGrid, world::Simulation};

#[cfg(test)]
#[path = "walk_completion_tests.rs"]
mod tests;

impl Simulation {
    /// EventClass4C7467 assigns TarCom before4C747C applies the Attack
    /// payload's null destination. WalkStop keeps an already accepted head;
    /// retain its execution adapter until the ordinary completion callback.
    pub(crate) fn finish_ordered_walk_attack(&mut self, id: u64, rules: Option<&RuleSet>) {
        if self.set_walk_null_destination(id, rules) {
            super::retain_committed_movement(
                self.substrate
                    .entities
                    .get_mut(id)
                    .expect("accepted Walk actor"),
            );
        }
    }

    /// Shared ordinary Infantry51AA40(NULL,true)->Foot4D94B0->Walk75ADA0
    /// state writes. NavQueue(+598) is a different owner and is not erased.
    /// MovementTarget continuation/finalization belongs to the caller.
    pub(crate) fn set_walk_null_destination(&mut self, id: u64, rules: Option<&RuleSet>) -> bool {
        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        if !actor
            .locomotor
            .as_ref()
            .is_some_and(|loco| loco.kind == crate::rules::locomotor_type::LocomotorKind::Walk)
        {
            return false;
        }
        let human = self
            .houses
            .get(&actor.owner())
            .is_some_and(|h| h.is_controlled_by_human(self.session.game_mode_nonzero));
        if human
            && actor
                .mission_leaf
                .as_infantry()
                .is_some_and(|leaf| (27..=30).contains(&leaf.doing()))
        {
            return false;
        }
        // 51AC25..51AD17: an Enter with no live radio contact skips the
        // setter's head write. 65AE30 answers whether any contact is nonnull;
        // with a null destination, 40DD70 then returns null without messaging.
        let clear_head = (actor.mission.effective().raw() != 7
            && actor.mission.queued().raw() != 7)
            || !actor.radio_contacts.is_empty();
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .expect("same setter actor");
        if clear_head {
            actor.navigation.path_replay.clear_live_head();
        }
        super::navcom::set_destination_internal_null(actor);
        super::DestinationTiming::from_rules(self.session.binary_frame, rules).accept(actor);
        true
    }

    /// Walk75BE42..75BF64 runs after PerCell and reloads the live destination.
    /// The Infantry setter may refuse; the separate speed/Stop suffix still runs.
    /// Original executable comparison: tools/spatial_oracle/walk_completion.
    pub(crate) fn finish_walk_navigation(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
    ) -> Result<(), String> {
        let Some(actor) = self.substrate.entities.get(id) else {
            return Ok(());
        };
        if !actor.lifecycle.object_alive
            || actor.lifecycle.in_limbo
            || actor.object_is_falling_down != 0
        {
            return Ok(());
        }
        let Some(loco) = actor
            .locomotor
            .as_ref()
            .filter(|l| l.kind == crate::rules::locomotor_type::LocomotorKind::Walk)
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
            self.set_walk_null_destination(id, rules);
            if let Some(actor) = self.substrate.entities.get_mut(id) {
                //75BF38 invokes Foot4D3710(0.0), independently of setter admission.
                actor.foot_speed.applied_fraction = crate::util::fixed_math::SIM_ZERO;
                if let Some(loco) = actor.locomotor.as_mut() {
                    loco.set_step_head(None);
                    loco.stop_walk();
                }
            }
        } else if let Some(destination) = destination
            && let Some(target) = self
                .substrate
                .entities
                .get_mut(id)
                .and_then(|actor| actor.movement_target.as_mut())
            && target.next_index >= target.path.len()
        {
            //75BF64 retains a changed destination, even in the same cell when
            //the height differs. Preserve Foot queue/reference/timers while
            //retiring only the exhausted route into the existing search request.
            target.path.clear();
            target.path_layers.clear();
            target.next_index = 0;
            target.final_goal = Some(((destination.x / 256) as u16, (destination.y / 256) as u16));
        }
        Ok(())
    }

    /// FootPerCell(mode2)4D882F..896E, reached after Infantry's own PerCell
    /// work. Select against TarCom first, then range against the Cell returned
    /// from the target's current XYZ. The range call precedes mission/queue
    /// gates; in particular its native map lookup can stamp the shared Dummy.
    /// See tools/spatial_oracle/walk_percell_stop.{py,json,meta.json}.
    fn finish_walk_pursuit_at_per_cell(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        use crate::map::entities::EntityCategory;
        use crate::sim::combat::{self, TargetKind};

        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        let range_stop = (|| {
            let target = &actor.attack_target.as_ref()?.target;
            // 4D883A selects the weapon before the target/Foot-bit gates.
            let weapon = combat::pursuit_selected_weapon(
                actor,
                target,
                &self.substrate.entities,
                rules,
                &self.interner,
                self.resolved_terrain.as_ref(),
                Some(&self.house_alliances),
            )?;
            let TargetKind::Entity(target_id) = *target else {
                return None;
            };
            let target_entity = self.substrate.entities.get(target_id)?;
            if target_entity.category == EntityCategory::Structure {
                return None;
            }
            let actor_type = self.object_type(actor.type_ref(), rules)?;
            if actor_type.open_topped {
                // Type+5E4 is the actor's own OpenTopped type property. Its
                // separate strict-distance arm has no stock Infantry producer
                // in the reviewed rules/9 modes/184 maps. It remains outside
                // this ordinary Infantry completion slice.
                return None;
            }
            let terrain = self.resolved_terrain.as_ref()?;
            // Foot+4F0 ->4D9FF0->41BDD0->5F65A0 returns actual owner XYZ,
            // independently of its retained locomotor head or destination.
            let xyz = ground_pose::position_world_coord(&target_entity.position);
            let cell = terrain.native_cell_identity(((xyz.x / 256) as i16, (xyz.y / 256) as i16));
            // Reuse the existing range/source owners with the SAME weapon;
            // selecting again against a Cell would change the native slot.
            combat::in_range::cell_target_in_range(
                actor,
                cell,
                weapon,
                rules,
                &self.interner,
                &self.substrate.entities,
                terrain,
                &combat::line_of_fire::LineOfFireInputs {
                    overlay_grid: self.overlay_grid.as_ref(),
                    overlay_registry: registry,
                    alliances: Some(&self.fog.alliances),
                },
            )
        })()
        .unwrap_or(false);
        if ![21, 11, 1, 15].contains(&actor.mission.effective().raw())
            || !range_stop
            || !actor.navigation.nav_queue.is_empty()
        {
            return;
        }
        let accepts = self.set_walk_null_destination(id, Some(rules));
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .expect("same PerCell actor");
        if accepts {
            // The proved null/non-Enter Infantry envelope reaches Foot4D94B0
            // and Walk75ADA0. There is no paid head left at this call site.
            // The setter already reset persistent Foot timers. Only the
            // execution adapter is retired at this completion boundary.
            if let Some(target) = actor.movement_target.as_mut() {
                target.next_index = target.path.len();
            }
        }
        // 4D896E executes even when the human Doing gate refuses +480.
        // Keep the backing suffix/cursor/reference; this is one native DWORD.
        actor.navigation.path_replay.clear_live_head();
    }

    /// Walk75C117..75C1AE relinks current XYZ while retaining the paid head
    /// and Foot path entry. This corridor does not invoke PerCell.
    pub(crate) fn run_walk_boundary(
        &mut self,
        id: u64,
        coord: DriveCoord,
        rules: Option<&RuleSet>,
        fallback: Option<&PathGrid>,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let Some(old_cell) = self
            .substrate
            .entities
            .get(id)
            .map(|e| (e.position.rx, e.position.ry))
        else {
            return;
        };
        self.foot_mark_remove(id, rules, fallback, registry);
        let Some(e) = self.substrate.entities.get_mut(id) else {
            return;
        };
        put_walk_coords(&mut e.position, coord);
        let cell = (e.position.rx, e.position.ry);
        let active_layer = e
            .movement_target
            .as_ref()
            .map(|t| t.layer_at(t.next_index))
            .unwrap_or_else(|| {
                if e.on_bridge {
                    MovementLayer::Bridge
                } else {
                    MovementLayer::Ground
                }
            });
        // Recalc may replace the canonical PathGrid. Read its current cell
        // flags after REMOVE, then SetHeight samples current resolved terrain.
        let update = super::movement_bridge::resolve_cell_transition_bridge_state(
            &mut e.position,
            self.path_grid.as_deref().or(fallback),
            old_cell,
            cell,
            e.on_bridge,
        );
        super::movement_bridge::apply_pending_bridge_render_state(
            &mut e.locomotor,
            &mut e.bridge_occupancy,
            &mut e.on_bridge,
            active_layer,
            update,
            id,
        );
        ground_pose::commit_ground_height(
            &mut e.position,
            e.on_bridge,
            self.resolved_terrain.as_ref(),
            self.path_grid.as_deref().or(fallback),
        );
        // OccupancyGrid is a list projection, not an independent subcell
        // reservation chooser. The current coordinate supplies its slot.
        e.sub_cell = Some(crate::sim::cell_kernel::infantry_preferred_spot(
            crate::sim::cell_kernel::CellQueryPoint {
                x: coord.x,
                y: coord.y,
            },
        ));
        e.navigation.path_runtime.path_blocked = false;
        self.foot_mark_put(id, rules, fallback, registry);
    }

    pub(crate) fn run_completed_walk_step(
        &mut self,
        id: u64,
        head: DriveCoord,
        rules: Option<&RuleSet>,
        fallback: Option<&PathGrid>,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<bool, crate::sim::world::FrameAdvanceError> {
        self.foot_mark_remove(id, rules, fallback, registry);
        let Some(e) = self.substrate.entities.get_mut(id) else {
            return Ok(false);
        };
        let head = e
            .locomotor
            .as_ref()
            .and_then(|l| l.step_head())
            .unwrap_or(head);
        let old_head = e.locomotor.as_ref().and_then(|l| l.step_head());
        super::path_markers::consume_walk_path_replay(&mut e.navigation.path_replay);
        put_walk_coords(&mut e.position, head);
        e.navigation.path_replay.reference_cell =
            Some((e.position.rx as i16, e.position.ry as i16));
        if let Some(target) = e.movement_target.as_mut() {
            super::movement_step::configure_motion_after_transition(
                target,
                &e.locomotor,
                &mut e.facing,
                &mut e.facing_target,
                e.category,
                0,
                &e.position,
            );
        }
        //Infantry+1CC=5F5FA0; marked is already false, so the SetHeight0
        //receiver samples current ground+OnBridge without nested Mark calls.
        ground_pose::commit_ground_height(
            &mut e.position,
            e.on_bridge,
            self.resolved_terrain.as_ref(),
            self.path_grid.as_deref().or(fallback),
        );
        let current = ground_pose::position_world_coord(&e.position);
        let owner = e.owner();
        if let Some(old) = old_head {
            super::walk_head::raw_at(
                &mut self.substrate.raw_cell_occupation,
                owner,
                old,
                false,
                self.resolved_terrain.as_ref(),
                self.path_grid.as_deref().or(fallback),
            );
        }
        if let Some(loco) = e.locomotor.as_mut() {
            loco.set_step_head(None);
            loco.subcell_dest = Some((e.position.sub_x, e.position.sub_y));
        }
        //75BE11 clears only the paid-step latch before head retirement/PerCell.
        e.navigation.path_runtime.path_blocked = false;
        e.sub_cell = Some(super::bump_crush::priority_sub_cell(
            e.position.sub_x,
            e.position.sub_y,
        ));
        super::walk_head::raw_at(
            &mut self.substrate.raw_cell_occupation,
            owner,
            current,
            true,
            self.resolved_terrain.as_ref(),
            self.path_grid.as_deref().or(fallback),
        );
        let changed = if let Some(rules) = rules {
            self.infantry_per_cell_bridge_repair(id, rules, registry)?
        } else {
            false
        };
        let survives = self.substrate.entities.get(id).is_some_and(|e| {
            e.lifecycle.object_alive && !e.lifecycle.in_limbo && e.object_is_falling_down == 0
        });
        if !survives {
            return Ok(changed);
        }
        if let Some(rules) = rules {
            self.refresh_unit_sensor_at_per_cell(id, rules);
            crate::sim::world::techno_ai_cloak::uncloak_on_sensor_neighbour_after_cell_entry(
                self, id, rules,
            );
            self.promote_entity_playfield_membership_after_move(id);
            self.finish_walk_pursuit_at_per_cell(id, rules, registry);
        }
        self.finish_walk_navigation(id, rules).map_err(|cause| {
            crate::sim::world::FrameAdvanceError {
                tick: self.session.tick,
                binary_frame: self.session.binary_frame,
                entity_id: id,
                cause,
            }
        })?;
        self.foot_mark_put(id, rules, fallback, registry);
        Ok(changed)
    }
}

fn put_walk_coords(position: &mut crate::sim::components::Position, coord: DriveCoord) {
    position.rx = (coord.x / 256) as u16;
    position.ry = (coord.y / 256) as u16;
    position.sub_x = crate::util::fixed_math::SimFixed::from_num(coord.x % 256);
    position.sub_y = crate::util::fixed_math::SimFixed::from_num(coord.y % 256);
    position.exact_z_leptons = Some(coord.z);
}
