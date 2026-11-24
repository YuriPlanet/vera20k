//! Air locomotor Process visits with cell-list upkeep: the Fly phase
//! completion and the Fly take-off callback.

use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::intern::StringInterner;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::world::{FrameEffects, ObjectSubstrate, Simulation};

impl Simulation {
    /// One air locomotor's Process visit: a Fly's (`0x004CCB40`,
    /// [`Self::fly_process`]) or a Jumpjet's (`world::jumpjet_cruise`). Each
    /// keeps its own AircraftTracker membership (Add `0x004134A0`, Remove
    /// `0x004135D0`, Update `0x004138C0`), and `FootClass::Limbo` removes a
    /// tracked object ([`ObjectSubstrate::release_foot_air_tracker_before_limbo`](crate::sim::world::ObjectSubstrate::release_foot_air_tracker_before_limbo)).
    /// `FootClass::AI` calls the active locomotor's Process with no layer
    /// test (`0x004DA806..0x004DA877`), so every live Fly runs, whatever
    /// layer its locomotor caches: one a Chrono Warp or an Ares UnitDelivery
    /// left on the ground plane takes off at its next order.
    pub(crate) fn tick_air_movement_with_cell_lists_one(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
    ) -> Result<crate::sim::movement::air_movement::AirMovementTickStats, super::FrameAdvanceError>
    {
        use crate::rules::locomotor_type::LocomotorKind;

        let fly = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                entity.lifecycle.object_alive
                    && !entity.lifecycle.in_limbo
                    && entity
                        .locomotor
                        .as_ref()
                        .is_some_and(|locomotor| locomotor.active_kind() == LocomotorKind::Fly)
            });
        if fly {
            return Ok(self.fly_process(stable_id, rules, registry, effects));
        }
        let jumpjet_layer_before = self
            .substrate
            .entities
            .get(stable_id)
            .filter(|entity| {
                entity
                    .locomotor
                    .as_ref()
                    .is_some_and(|l| l.active_kind() == LocomotorKind::Jumpjet)
            })
            .and_then(|_| self.entity_display_layer(stable_id, rules));
        // A cruising Jumpjet runs the native Update/State3 body
        // (`world::jumpjet_cruise`).
        let stats = match self.tick_jumpjet_cruise_one(stable_id, rules, effects)? {
            // State 5's impact notice UnInits the wreck, so `Process`'s layer
            // tail finds it dead (`0x0054B16C`); the object turn commits it.
            Some(stats) if stats.impact => return Ok(stats),
            Some(stats) => stats,
            None => crate::sim::movement::air_movement::AirMovementTickStats::default(),
        };
        if let Some(before) = jumpjet_layer_before {
            Self::complete_jumpjet_display_process(
                &self.interner,
                &mut self.substrate,
                &self.resolved_terrain,
                stable_id,
                before,
                rules,
            );
        }
        Ok(stats)
    }

    /// Fly4CCB40 ->4CD2A0 after4CD600's movement/height Mark pair.
    /// Non-Landable aircraft return before the phase's OWN Mark/Display pair.
    /// Landing runs first, then rechecks takeoff; both resubmit on equal layers.
    /// Return whether the phase performed the Display transaction.
    pub(in crate::sim::world) fn complete_fly_phase(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
    ) -> bool {
        let admitted = self.substrate.entities.get(id).is_some_and(|entity| {
            entity.lifecycle.object_alive
                && entity.health.current() > 0
                && entity
                    .locomotor
                    .as_ref()
                    .is_some_and(|l| l.is_powered() && l.fly_runtime().is_some())
        });
        if !admitted {
            return false;
        }
        let entity = self.substrate.entities.get(id).unwrap();
        let non_landable_level = (entity.category() == EntityCategory::Aircraft)
            .then(|| {
                self.interner
                    .object_type(entity.type_ref(), rules)
                    .filter(|object| !object.landable)
                    .map(|object| object.flight_level(rules.general.flight_level))
            })
            .flatten();
        if let Some(flight_level) = non_landable_level {
            self.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .locomotor
                .as_mut()
                .unwrap()
                .fly_runtime_mut()
                .unwrap()
                .force_non_landable_flight(flight_level);
            return false;
        }
        if !entity
            .locomotor
            .as_ref()
            .unwrap()
            .fly_runtime()
            .unwrap()
            .has_phase_callback()
        {
            return false;
        }
        let before = self.entity_display_layer(id, rules);
        self.foot_mark_remove(id, rules, registry, effects);
        self.substrate.display.remove(id);
        if self
            .substrate
            .entities
            .get(id)
            .and_then(|e| e.locomotor.as_ref())
            .and_then(|l| l.fly_runtime())
            .is_some_and(|s| s.landing())
        {
            self.apply_fly_landing_callback(id, rules, effects);
        }
        if self
            .substrate
            .entities
            .get(id)
            .and_then(|e| e.locomotor.as_ref())
            .and_then(|l| l.fly_runtime())
            .is_some_and(|s| s.taking_off())
        {
            Self::apply_fly_takeoff_callback(
                &self.interner,
                &self.session,
                &mut self.substrate,
                &self.resolved_terrain,
                id,
                rules,
            );
        }
        let after = self.entity_display_layer(id, rules);
        if before != after {
            // `0x004CD3B6` asks about the cell under the aircraft (MapAtCoord).
            let coord = crate::sim::movement::ground_pose::position_world_coord(
                &self.substrate.entities.get(id).unwrap().position,
            );
            let below = NavTargetRef::cell((coord.x / 256) as u16, (coord.y / 256) as u16);
            if after == Some(super::display_layers::DisplayLayer::GROUND)
                && !self
                    .substrate
                    .entities
                    .get(id)
                    .and_then(|e| e.locomotor.as_ref())
                    .and_then(|l| l.fly_runtime())
                    .is_some_and(|s| s.taking_off())
                && !self.foot_land_zone_clear(id, below, rules, None)
            {
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    entity
                        .locomotor
                        .as_mut()
                        .unwrap()
                        .fly_runtime_mut()
                        .unwrap()
                        .reject_landing_cell();
                    entity.on_bridge.store(false);
                }
                self.foot_mark_remove(id, rules, registry, effects);
                let height = crate::sim::movement::air_movement::current_fly_height(
                    self.substrate.entities.get(id).unwrap(),
                    self.resolved_terrain.as_ref(),
                );
                self.set_object_height(id, height.wrapping_add(10), rules, effects);
                self.foot_mark_put(id, rules, registry, effects);
            } else {
                self.finish_fly_layer_transition(id, after, rules, effects);
            }
        }
        // Display4A9720 has no limbo gate, so the tail resubmits even an owner
        // the landing retry's C4 receiver just destroyed; store removal expires
        // that registration. Object Mark5F5850 refuses a Limbo owner, so the
        // cell lists never regain it.
        Self::submit_entity_display(
            &self.interner,
            &mut self.substrate,
            &self.resolved_terrain,
            id,
            rules,
            None,
        );
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(|e| !e.lifecycle.in_limbo)
        {
            self.foot_mark_put(id, rules, registry, effects);
        }
        true
    }

    /// Admitted callback4CE680. The phase caller owns Mark/Display sequencing;
    /// the callback owns flag clearing and the two existing facing controllers.
    pub(in crate::sim::world) fn apply_fly_takeoff_callback(
        interner: &StringInterner,
        session: &ScenarioSession,
        substrate: &mut ObjectSubstrate,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        id: u64,
        rules: &RuleSet,
    ) {
        use crate::sim::movement::{fly_height::TakeoffFacing, ground_pose};
        let entity = substrate.entities.get(id).expect("admitted Fly callback");
        let xy = ground_pose::position_world_xy(&entity.position);
        let height = Self::fly_bridge_relative_height(entity, resolved_terrain.as_ref(), false);
        let landing_base = crate::sim::aircraft::landing_base::landing_base(
            entity,
            &substrate.entities,
            Some((rules, interner)),
        );
        let entity = substrate
            .entities
            .get_mut(id)
            .expect("admitted Fly callback");
        entity.ensure_fly_secondary_facing();
        let state = entity
            .locomotor
            .as_mut()
            .unwrap()
            .fly_runtime_mut()
            .unwrap();
        let destination = state.destination();
        match state.complete_takeoff(height, landing_base) {
            TakeoffFacing::Unchanged => {}
            TakeoffFacing::SecondaryToPrimaryDestination => {
                let desired = entity.body_facing.destination();
                entity
                    .barrel_facing
                    .as_mut()
                    .unwrap()
                    .set(desired, session.binary_frame());
            }
            TakeoffFacing::PrimaryToDestination => {
                // Original also evaluates the zero delta; do not special-case
                // it to the current heading. The shared native table owns it.
                let desired = crate::util::direction_tables::facing16_from_delta(
                    destination.x.wrapping_sub(xy[0]),
                    destination.y.wrapping_sub(xy[1]),
                );
                entity.body_facing.set(desired, session.binary_frame());
                entity
                    .locomotor
                    .as_mut()
                    .unwrap()
                    .fly_runtime_mut()
                    .unwrap()
                    .request_full_speed();
            }
        }
    }
}
