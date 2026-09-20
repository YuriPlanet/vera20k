//! Jumpjet locomotor — altitude-holding hover flight with acceleration and wobble.
//!
//! Jumpjets are distinct from Fly aircraft: they hover at a fixed altitude with
//! wobble, use acceleration/deceleration curves, and have turn rate limits.
//! TS-style jumpjet infantry walk for short moves (≤3 cells) when !HoverAttack.
//!
//! ## Key RA2/YR rules (from locomotor report)
//! - JumpjetAccel: acceleration rate; deceleration = accel × 1.5
//! - JumpjetTurnRate: facing change limit per tick
//! - JumpjetWobbles + JumpjetDeviation: lateral oscillation during hover
//! - JumpjetCrash: crash descent speed = climb + crash
//! - BalloonHover=yes: stay airborne after reaching destination
//! - Infantry + Jumpjet + !HoverAttack: Walk for ≤3 cells
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/locomotor, sim/movement.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::sim::movement::locomotor::{AirMovePhase, LocomotorState};
use crate::util::fixed_math::{SIM_1_5, SIM_ZERO, SimFixed};

/// Persistent Jumpjet instance fields: cached XYZ at +40, moving at +4C,
/// phase at +50 (interface-relative offsets are four bytes smaller).
/// Constructor54AC40 and stream54B750/54B7E0 retain these independently.
/// AirMovePhase describes the older motion adapter and is not this authority.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct JumpjetRuntime {
    pub destination: crate::sim::components::DriveCoord,
    pub moving: bool,
    pub phase: i32,
    /// The type block `Link_To_Object @ 0x0054AD30` copies (`+0x1C..+0x3C`).
    pub params: super::jumpjet_flight::JumpjetFlightParams,
    /// Facing, speeds, target height and bob (`+0x54..+0x8C`).
    pub flight: super::jumpjet_flight::JumpjetFlight,
    /// Locomotor `+0x90`: State 4 sets this once it has admitted a landing, so
    /// later descent frames stop re-testing the cell. Touchdown clears it.
    #[serde(default)]
    pub landing_latched: bool,
}

impl Default for JumpjetRuntime {
    fn default() -> Self {
        Self {
            destination: Self::NULL,
            moving: false,
            phase: 0,
            params: Default::default(),
            flight: Default::default(),
            landing_latched: false,
        }
    }
}

impl JumpjetRuntime {
    pub(crate) const NULL: crate::sim::components::DriveCoord =
        crate::sim::components::DriveCoord { x: 0, y: 0, z: 0 };

    /// Jumpjet54D9B0, before Foot4DBDF0 applies its full NullCoord fallback.
    pub(crate) fn coordinate(
        &self,
        current: crate::sim::components::DriveCoord,
    ) -> crate::sim::components::DriveCoord {
        if self.phase == 0 {
            current
        } else {
            self.destination
        }
    }

    /// `Link_To_Object @ 0x0054AD30`: copy the type block and rebuild the
    /// locomotor facing at the type's turn rate, snapped to `0x4000`.
    pub(crate) fn link(&mut self, type_params: &crate::rules::jumpjet_params::JumpjetParams) {
        self.params = super::jumpjet_flight::JumpjetFlightParams::link(type_params);
        self.flight = super::jumpjet_flight::JumpjetFlight::linked(&self.params);
    }

    /// 54B22F stores the request before the possibly failing placement call.
    pub(crate) fn begin_move(&mut self, requested: crate::sim::components::DriveCoord) {
        self.destination = requested;
        if requested == Self::NULL {
            self.moving = false;
        }
    }

    /// Infantry RTTI15 takes 54B415's adjusted-coordinate copy. A failed
    /// 54D6D0 return leaves both the request and the previous moving byte.
    pub(crate) fn accept_infantry_destination(
        &mut self,
        adjusted: Option<crate::sim::components::DriveCoord>,
    ) {
        if let Some(adjusted) = adjusted.filter(|p| *p != Self::NULL) {
            self.destination = adjusted;
            self.moving = true;
            if self.phase == 4 {
                self.phase = 1;
            }
        }
    }

    /// State0 handler54B980, after its owner callbacks. House53A130 is the
    /// original constantfalse leaf, not a house-policy input.
    ///
    /// Test-only since the locomotor took over the tick. Production reaches the
    /// same promotion through [`super::jumpjet_flight::state0_ground`], which
    /// `world::jumpjet_cruise` dispatches from `Process 0x0054AEC0`; that is the
    /// single authority. This mirror survives only because the
    /// `jumpjet_coordinates` corpus drives `activate` as one of its actions.
    #[cfg(test)]
    pub(crate) fn activate(&mut self) {
        if self.phase == 0 && self.moving {
            self.phase = 1;
        }
    }

    /// 54B6BC runs after the failed Stop search's damage callback.
    pub(crate) fn clear_failed_stop_destination(&mut self) {
        self.destination = Self::NULL;
    }
}

/// 54D6D0's Infantry (RTTI15) receiver: 4ACA10 selects the actual raw
/// subcell, then ground height is sampled AGAIN at the returned XY. The
/// second sample matters on slopes. No raw reservation is written here.
#[allow(clippy::too_many_arguments)]
pub(crate) fn infantry_destination_coordinate(
    input: crate::sim::components::DriveCoord,
    terrain: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    raw: &crate::sim::occupancy::RawCellOccupationGrid,
    occupancy: &crate::sim::occupancy::OccupancyGrid,
    entities: &crate::sim::entity_store::EntityStore,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    interner: &crate::sim::intern::StringInterner,
    rng: &mut crate::sim::rng::SimRng,
) -> Option<crate::sim::components::DriveCoord> {
    use super::locomotor::MovementLayer;
    use crate::sim::{cell_kernel, occupancy::RawCellKey};
    let cell = terrain.native_cell_identity(((input.x / 256) as i16, (input.y / 256) as i16));
    let xy = terrain.native_cell_coord(cell);
    let level = match cell {
        crate::map::cell_index::NativeCellIdentity::Real(index) => terrain.cells()[index].level,
        crate::map::cell_index::NativeCellIdentity::Dummy => {
            terrain.shared_cell_dummy().snapshot().level as u8
        }
    };
    let bridge = cell_kernel::selects_infantry_bridge_layer(
        terrain.native_cell_flags(cell) & 0x100 != 0,
        level,
        input.z,
    );
    let key = RawCellKey::from_native(terrain, cell);
    let ground = raw.bits_at(key, MovementLayer::Ground);
    let selected = raw.bits_at(
        key,
        if bridge {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        },
    );
    let gate_open = selected & 0x20 == 0
        && ground & 0x40 != 0
        && occupancy
            .first_building_on_layer(xy.0 as u16, xy.1 as u16, MovementLayer::Ground)
            .and_then(|id| entities.get(id))
            .is_some_and(|building| {
                rules
                    .and_then(|rules| rules.object(interner.resolve(building.type_ref())))
                    .is_some_and(|object| object.gate)
                    && building
                        .building_gate
                        .is_some_and(|gate| gate.can_garrison_passable())
            });
    let slot = super::walk_head::select_slot(input, false, selected, ground, gate_open, rng)?;
    let first_ground =
        super::ground_pose::ground_surface_z_at([input.x, input.y], false, Some(terrain), None)?;
    let mut adjusted = super::walk_head::selected_head(input, slot, first_ground, bridge);
    adjusted.z = super::ground_pose::ground_surface_z_at(
        [adjusted.x, adjusted.y],
        false,
        Some(terrain),
        None,
    )?;
    let selected_cell =
        terrain.native_cell_identity(((adjusted.x / 256) as i16, (adjusted.y / 256) as i16));
    if terrain.native_cell_flags(selected_cell) & 0x100 != 0 {
        adjusted.z = adjusted.z.wrapping_add(416);
    }
    Some(adjusted)
}

impl crate::sim::world::Simulation {
    /// World-owned grids/RNG feed the existing air order owner. Jumpjet's
    /// Infantry MoveTo54B1C0 first installs the request, then FNPC56DC20 and
    /// 54D6D0 replace it with a live selected subcell. Ordinary Fly callers
    /// continue through their established motion adapter.
    pub(crate) fn issue_air_cell_destination(
        &mut self,
        id: u64,
        target: (u16, u16),
        speed: SimFixed,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) -> bool {
        let is_jumpjet_infantry = self.substrate.entities.get(id).is_some_and(|entity| {
            entity.category == crate::map::entities::EntityCategory::Infantry
                && entity
                    .locomotor
                    .as_ref()
                    .and_then(|l| l.jumpjet_runtime())
                    .is_some()
        });
        if !is_jumpjet_infantry {
            return super::air_movement::issue_air_move_command(
                &mut self.substrate.entities,
                id,
                target,
                speed,
                crate::sim::movement::DestinationTiming::from_rules(
                    self.session.binary_frame,
                    rules.into(),
                ),
            );
        }
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let input = super::navcom::target_cell_coord(target.0, target.1, Some(terrain));
        let e = self.substrate.entities.get_mut(id).expect("selected mover");
        e.navigation.nav_com = Some(crate::sim::components::NavTargetRef::cell(
            target.0, target.1,
        ));
        e.navigation.nav_com_aux = None;
        e.locomotor
            .as_mut()
            .unwrap()
            .jumpjet_runtime_mut()
            .unwrap()
            .begin_move(input);
        let accepted = self.resolve_jumpjet_infantry_move(id, input, speed, rules);
        if accepted && let Some(entity) = self.substrate.entities.get_mut(id) {
            // Infantry51B1D2 returns through Foot4D96C2 after locomotor MoveTo.
            // The shared low-level MoveTo also runs inside Stop and cannot own this.
            super::DestinationTiming::from_rules(self.session.binary_frame, rules).accept(entity);
        }
        accepted
    }

    /// Runtime Foot null-destination dispatch to Jumpjet54B4D0. Command
    /// execution is outside ScenarioInit's A8E7AC suppression scope. Successful
    /// Stop searches at current XYZ, then invokes MoveTo again (including its
    /// second search and subcell RNG); it never fabricates an idle cache.
    pub(crate) fn stop_jumpjet_infantry_destination(
        &mut self,
        id: u64,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        use crate::sim::{components::DriveCoord, find_nearby_cell::*};
        let Some(e) = self.substrate.entities.get(id) else {
            return false;
        };
        if e.category != crate::map::entities::EntityCategory::Infantry {
            return true;
        }
        let Some(loco) = e.locomotor.as_ref() else {
            return true;
        };
        let Some(state) = loco.jumpjet_runtime() else {
            return true;
        };
        if !state.moving {
            return true;
        }
        let passability = PassabilityArgs {
            speed_type: loco.speed_type,
            required_zone_id: None,
            movement_zone: loco.movement_zone,
            bridge_aware_zone: false,
        };
        let input = super::ground_pose::position_world_coord(&e.position);
        let speed = e
            .movement_target
            .as_ref()
            .map(|t| t.speed)
            .or_else(|| self.resolve_move_info(id, rules).map(|i| i.speed))
            .unwrap_or(SIM_ZERO);
        let seed = ((input.x / 256) as i16, (input.y / 256) as i16);
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let size = self
            .playfield_bounds
            .zip(self.playfield_size_height)
            .map(|(b, h)| (b.base, h))
            .or_else(|| self.bridge_state.as_ref()?.native_zone_source_size());
        let Some(size) = size else {
            return false;
        };
        let grid = self.path_grid_snapshot();
        let selected = find_nearby_passable_cell(
            (i32::from(seed.0), i32::from(seed.1)),
            &NearbyQuery {
                native_cells: None,
                raw_occupation: Some(&self.substrate.raw_cell_occupation),
                passability,
                footprint: NearbyFootprint::SINGLE,
                anchor_gate: NearbyAnchorGate::NativeHeightAware,
                allow_bridge_cells: true,
                check_height: false,
                check_occupancy: false,
                radius_cap: map_owned_radius_cap(size.0, size.1),
                target_cell: None,
                path_grid: grid.as_deref(),
                resolved_terrain: Some(terrain),
                overlay_grid: self.overlay_grid.as_ref(),
                occupancy: Some(&self.substrate.occupancy),
                entities: Some(&self.substrate.entities),
                zone_grid: self.zone_grid.as_ref(),
                playfield_bounds: self.playfield_bounds,
            },
            self.session.binary_frame,
        );
        if let Some(selected) = selected.filter(|xy| *xy != (0, 0)) {
            let mut destination = DriveCoord {
                x: i32::from(selected.0 as i16) * 256 + 128,
                y: i32::from(selected.1 as i16) * 256 + 128,
                z: 0,
            };
            let Some(z) = super::ground_pose::ground_surface_z_at(
                [destination.x, destination.y],
                false,
                Some(terrain),
                None,
            ) else {
                return false;
            };
            destination.z = z;
            let c = terrain
                .native_cell_identity(((destination.x / 256) as i16, (destination.y / 256) as i16));
            if terrain.native_cell_flags(c) & 0x100 != 0 {
                destination.z = destination.z.wrapping_add(416);
            }
            self.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .locomotor
                .as_mut()
                .unwrap()
                .jumpjet_runtime_mut()
                .unwrap()
                .begin_move(destination);
            return self.resolve_jumpjet_infantry_move(id, destination, speed, rules);
        }
        // Original54B698 returns before damage AND cache-clear if Health<=0.
        let health = self.substrate.entities.get(id).unwrap().health.current;
        if health <= 0 {
            return true;
        }
        let Some(rules) = rules else {
            return false;
        };
        use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
        let warhead = self.interner.intern(&rules.bridge_warheads.c4_name);
        // Native passes &Health. This uses the shared bridge_ground stock
        // C4Warhead=Super fatal-path quotient, not general aliased packet
        // support: override-only early damage-pointer writes remain outside it.
        self.commit_direct_damage_receiver(
            rules,
            registry,
            EntityDamageEvent::direct_receiver(
                id,
                i32::from(health),
                0,
                RAD_NO_ATTACKER,
                None,
                warhead,
                ReceiverCallFlags {
                    ignore_defenses: true,
                    arg6: true,
                },
            ),
        );
        // Read the retained live owner only AFTER synchronous damage/lifecycle.
        if let Some(state) = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|e| e.locomotor.as_mut())
            .and_then(|l| l.jumpjet_runtime_mut())
        {
            state.clear_failed_stop_destination();
        }
        true
    }

    fn resolve_jumpjet_infantry_move(
        &mut self,
        id: u64,
        input: crate::sim::components::DriveCoord,
        speed: SimFixed,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) -> bool {
        use super::locomotor::{AirMovePhase, MovementLayer};
        use crate::rules::locomotor_type::{MovementZone, SpeedType};
        use crate::sim::{
            components::{DriveCoord, MovementTarget, NavTargetRef},
            find_nearby_cell::*,
        };
        if input == JumpjetRuntime::NULL {
            return true;
        }
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let seed = ((input.x / 256) as i16, (input.y / 256) as i16);
        let cell = terrain.native_cell_identity(seed);
        let bridge = terrain.native_cell_flags(cell) & 0x100 != 0;
        let size = self
            .playfield_bounds
            .zip(self.playfield_size_height)
            .map(|(bounds, height)| (bounds.base, height))
            .or_else(|| self.bridge_state.as_ref()?.native_zone_source_size());
        let Some(size) = size else {
            return false;
        };
        let grid = self.path_grid_snapshot();
        let selected = find_nearby_passable_cell(
            (i32::from(seed.0), i32::from(seed.1)),
            &NearbyQuery {
                native_cells: None,
                raw_occupation: Some(&self.substrate.raw_cell_occupation),
                passability: PassabilityArgs {
                    speed_type: SpeedType::Hover,
                    required_zone_id: None,
                    movement_zone: MovementZone::Fly,
                    bridge_aware_zone: bridge,
                },
                footprint: NearbyFootprint::SINGLE,
                anchor_gate: NearbyAnchorGate::NativeHeightAware,
                allow_bridge_cells: true,
                check_height: false,
                check_occupancy: false,
                radius_cap: map_owned_radius_cap(size.0, size.1),
                target_cell: None,
                path_grid: grid.as_deref(),
                resolved_terrain: Some(terrain),
                overlay_grid: self.overlay_grid.as_ref(),
                occupancy: Some(&self.substrate.occupancy),
                entities: Some(&self.substrate.entities),
                zone_grid: self.zone_grid.as_ref(),
                playfield_bounds: self.playfield_bounds,
            },
            self.session.binary_frame,
        )
        .unwrap_or((0, 0));
        // MoveTo calls the placement receiver even when FNPC returns NullCell.
        let center = DriveCoord {
            x: i32::from(selected.0 as i16) * 256 + 128,
            y: i32::from(selected.1 as i16) * 256 + 128,
            z: 0,
        };
        let adjusted = infantry_destination_coordinate(
            center,
            terrain,
            &self.substrate.raw_cell_occupation,
            &self.substrate.occupancy,
            &self.substrate.entities,
            rules,
            &self.interner,
            &mut self.scenario_rng,
        );
        let e = self
            .substrate
            .entities
            .get_mut(id)
            .expect("map query retains mover");
        let state = e.locomotor.as_mut().unwrap().jumpjet_runtime_mut().unwrap();
        state.accept_infantry_destination(adjusted);
        let destination = state.destination;
        let moving = state.moving;
        if adjusted.is_some() && e.mission.current().raw() != 7 && e.mission.queued().raw() != 7 {
            let c = terrain
                .native_cell_identity(((destination.x / 256) as i16, (destination.y / 256) as i16));
            let xy = terrain.native_cell_coord(c);
            e.navigation.nav_com = Some(NavTargetRef::cell(xy.0 as u16, xy.1 as u16));
        }
        if moving {
            let target = ((destination.x / 256) as u16, (destination.y / 256) as u16);
            // Jumpjet54B1C0/54B4D0 do not reconstruct Foot timers/retries.
            // Their outer accepted destination caller publishes the Foot tail.
            e.movement_target = Some(MovementTarget {
                path: vec![target],
                path_layers: vec![MovementLayer::Air],
                next_index: 0,
                speed,
                final_goal: Some(target),
                ..Default::default()
            });
            let loco = e.locomotor.as_mut().unwrap();
            if loco.air_phase == AirMovePhase::Landed {
                loco.air_phase = AirMovePhase::Ascending;
            }
        }
        true
    }
}

/// Max cells for infantry walk fallback (TS-style jumpjet infantry).
const INFANTRY_WALK_THRESHOLD_CELLS: u32 = 3;

/// Apply acceleration toward target speed.
///
/// Ramps `jumpjet_current_speed` toward `jumpjet_speed` using `jumpjet_accel`.
/// Deceleration (when current > target) uses `accel * 1.5` per the RA2 formula.
pub fn tick_jumpjet_acceleration(loco: &mut LocomotorState, dt: SimFixed, moving: bool) {
    let target_speed: SimFixed = if moving { loco.jumpjet_speed } else { SIM_ZERO };

    if loco.jumpjet_accel <= SIM_ZERO {
        // No acceleration — snap to target speed.
        loco.jumpjet_current_speed = target_speed;
        return;
    }

    if loco.jumpjet_current_speed < target_speed {
        // Accelerating.
        loco.jumpjet_current_speed += loco.jumpjet_accel * dt;
        if loco.jumpjet_current_speed > target_speed {
            loco.jumpjet_current_speed = target_speed;
        }
    } else if loco.jumpjet_current_speed > target_speed {
        // Decelerating at 1.5× acceleration rate.
        loco.jumpjet_current_speed -= loco.jumpjet_accel * SIM_1_5 * dt;
        if loco.jumpjet_current_speed < target_speed {
            loco.jumpjet_current_speed = target_speed;
        }
    }
}

/// Apply turn rate limit: rotate facing toward desired facing, clamped to max delta.
///
/// Uses shortest-arc rotation. `turn_rate` is max facing units (0-255) per tick.
/// Returns the new facing value.
pub fn apply_turn_rate(current: u8, desired: u8, turn_rate: i32) -> u8 {
    if turn_rate <= 0 || current == desired {
        return desired;
    }

    // Compute shortest-arc signed delta in the 0-255 wrapping space.
    let diff: i16 = desired as i16 - current as i16;
    let wrapped: i16 = if diff > 128 {
        diff - 256
    } else if diff < -128 {
        diff + 256
    } else {
        diff
    };

    let max_delta: i16 = turn_rate as i16;
    let clamped: i16 = wrapped.clamp(-max_delta, max_delta);
    (current as i16 + clamped).rem_euclid(256) as u8
}

/// Compute deterministic hover wobble offset for visual position.
///
/// Returns (wobble_x, wobble_y) offset in screen pixels. The wobble is a
/// sinusoidal oscillation based on the simulation tick, producing smooth
/// hovering motion. Each entity gets a unique phase from `entity_seed`.
/// KEPT as f32 — render-only visual effect.
pub fn compute_wobble(tick: u64, entity_seed: u64, wobbles: f32, deviation: i32) -> (f32, f32) {
    if wobbles <= 0.0 || deviation <= 0 {
        return (0.0, 0.0);
    }

    // Use tick and entity seed for deterministic phase.
    let phase_x: f32 = (tick as f32 * 0.1 + entity_seed as f32 * 0.37) % std::f32::consts::TAU;
    let phase_y: f32 = (tick as f32 * 0.13 + entity_seed as f32 * 0.53) % std::f32::consts::TAU;

    let amplitude: f32 = wobbles * deviation as f32 * 0.01;
    let wx: f32 = phase_x.sin() * amplitude;
    let wy: f32 = phase_y.cos() * amplitude;
    (wx, wy)
}

/// Whether an idle jumpjet should begin landing (descending).
///
/// Returns true if the unit has `balloon_hover=false` (should land when idle)
/// and is currently hovering without a movement order.
pub fn should_land(loco: &LocomotorState) -> bool {
    !loco.balloon_hover && loco.air_phase == AirMovePhase::Hovering
}

/// Whether a jumpjet infantry unit should use ground Walk for this move distance.
///
/// TS-style rule: infantry with Jumpjet + !HoverAttack walk for ≤3 cells.
pub fn should_use_walk_fallback(
    hover_attack: bool,
    is_infantry: bool,
    distance_cells: u32,
) -> bool {
    is_infantry && !hover_attack && distance_cells <= INFANTRY_WALK_THRESHOLD_CELLS
}

/// Advance the jumpjet altitude state machine for one entity.
///
/// Like `tick_altitude` but uses jumpjet-specific crash speed when the unit
/// is in a crash descent (air_phase == Descending with crash_speed > 0).
pub fn tick_jumpjet_altitude(loco: &mut LocomotorState, dt: SimFixed) {
    match loco.air_phase {
        AirMovePhase::Ascending => {
            loco.altitude += loco.climb_rate * dt;
            if loco.altitude >= loco.target_altitude {
                loco.altitude = loco.target_altitude;
                loco.air_phase = AirMovePhase::Hovering;
            }
        }
        AirMovePhase::Descending => {
            // Use crash speed if set (unit was killed mid-air), otherwise normal climb.
            let descent_rate: SimFixed = if loco.jumpjet_crash_speed > SIM_ZERO {
                loco.jumpjet_crash_speed
            } else {
                loco.climb_rate
            };
            loco.altitude -= descent_rate * dt;
            if loco.altitude <= SIM_ZERO {
                loco.altitude = SIM_ZERO;
                loco.air_phase = AirMovePhase::Landed;
            }
        }
        AirMovePhase::Hovering => {
            // Maintain hover altitude.
            loco.altitude = loco.target_altitude;
        }
        AirMovePhase::Cruising => {
            // Jumpjets shouldn't be in Cruising, but treat as Hovering.
            loco.altitude = loco.target_altitude;
            loco.air_phase = AirMovePhase::Hovering;
        }
        AirMovePhase::Landed => {}
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn retained_coordinates_and_placement_match_original_jumpjet_bodies() {
        use crate::map::resolved_terrain::ResolvedTerrainGrid;
        use crate::sim::{
            components::DriveCoord,
            entity_store::EntityStore,
            intern::{InternedId, StringInterner},
            occupancy::{OccupancyGrid, RawCellOccupationGrid},
            rng::SimRng,
        };
        use serde_json::json;
        let rows: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/jumpjet_coordinates.json"
        ))
        .unwrap();
        assert_eq!(rows.as_array().unwrap().len(), 12);
        for row in rows.as_array().unwrap() {
            let input = &row["input"];
            let mut terrain = ResolvedTerrainGrid::from_cells(
                11,
                11,
                (0..11)
                    .flat_map(|y| {
                        (0..11).map(move |x| {
                            crate::sim::world::common_raw_test_terrain_cell(x, y, 0, false)
                        })
                    })
                    .collect(),
            );
            let c = terrain.cell_mut(10, 10).unwrap();
            c.level = input["level"].as_u64().unwrap_or(2) as u8;
            c.slope_type = input["slope"].as_u64().unwrap_or(0) as u8;
            c.bridge_facts.raw_flags = if input["structural"].as_bool().unwrap_or(false) {
                0x100
            } else {
                0
            };
            let mut raw = RawCellOccupationGrid::default();
            raw.mark_ground_infantry(
                10,
                10,
                input["ground"].as_u64().unwrap_or(0) as u8,
                InternedId::from_index(0),
            );
            let occupancy = OccupancyGrid::new();
            let entities = EntityStore::new();
            let interner = StringInterner::new();
            let mut rng = SimRng::new(31);
            let current = DriveCoord {
                x: 2496,
                y: 2624,
                z: 0,
            };
            let mut state = JumpjetRuntime {
                phase: input["phase"].as_i64().unwrap_or(0) as i32,
                ..Default::default()
            };
            let snapshot = |state: &JumpjetRuntime| {
                let getter = state.coordinate(current);
                let foot = if getter == JumpjetRuntime::NULL {
                    current
                } else {
                    getter
                };
                json!({"destination":[state.destination.x,state.destination.y,state.destination.z],"moving":state.moving,"phase":state.phase,"getter":[getter.x,getter.y,getter.z],"foot":[foot.x,foot.y,foot.z]})
            };
            let mut trace = vec![snapshot(&state)];
            for action in input["actions"].as_array().unwrap() {
                match action.as_str().unwrap() {
                    "move" | "stop" => {
                        // Same supplied FNPC10,10 seam as the native corpus.
                        // The placement/second ground sample/RNG are production bodies.
                        let request = if action == "move" {
                            DriveCoord {
                                x: 2688,
                                y: 2688,
                                z: 900,
                            }
                        } else {
                            DriveCoord::cell(10, 10, 208)
                        };
                        state.begin_move(request);
                        let adjusted = infantry_destination_coordinate(
                            DriveCoord::cell(10, 10, 0),
                            &terrain,
                            &raw,
                            &occupancy,
                            &entities,
                            None,
                            &interner,
                            &mut rng,
                        );
                        state.accept_infantry_destination(adjusted);
                    }
                    "activate" => state.activate(),
                    "null" => state.begin_move(JumpjetRuntime::NULL),
                    // Corpus damage callback is supplied and Health is positive.
                    "stop_fail" => state.clear_failed_stop_destination(),
                    unknown => panic!("unknown action {unknown}"),
                }
                trace.push(snapshot(&state));
            }
            assert_eq!(json!(trace), row["output"]["trace"], "{input}");
            let logical = rng.logical_view();
            assert_eq!(
                json!([logical.index_a, logical.index_b]),
                row["output"]["random_indices"],
                "{input}"
            );
        }
    }
    use super::*;
    use crate::sim::movement::locomotion::LocomotorSlot;
    use crate::util::fixed_math::{SIM_ONE, sim_from_f32};

    #[test]
    fn test_acceleration_ramps_up() {
        let mut loco = make_test_jj_loco();
        loco.jumpjet_accel = SimFixed::from_num(2);
        loco.jumpjet_speed = SimFixed::from_num(14);
        loco.jumpjet_current_speed = SIM_ZERO;

        tick_jumpjet_acceleration(&mut loco, SIM_ONE, true);
        assert_eq!(loco.jumpjet_current_speed, SimFixed::from_num(2));

        tick_jumpjet_acceleration(&mut loco, SIM_ONE, true);
        assert_eq!(loco.jumpjet_current_speed, SimFixed::from_num(4));
    }

    #[test]
    fn test_deceleration_is_1_5x() {
        let mut loco = make_test_jj_loco();
        loco.jumpjet_accel = SimFixed::from_num(2);
        loco.jumpjet_speed = SimFixed::from_num(14);
        loco.jumpjet_current_speed = SimFixed::from_num(14);

        // Decelerate (not moving).
        tick_jumpjet_acceleration(&mut loco, SIM_ONE, false);
        // Should decrease by 2.0 * 1.5 = 3.0.
        assert_eq!(loco.jumpjet_current_speed, SimFixed::from_num(11));
    }

    #[test]
    fn test_acceleration_caps_at_target() {
        let mut loco = make_test_jj_loco();
        loco.jumpjet_accel = SimFixed::from_num(100);
        loco.jumpjet_speed = SimFixed::from_num(14);
        loco.jumpjet_current_speed = SIM_ZERO;

        tick_jumpjet_acceleration(&mut loco, SIM_ONE, true);
        assert_eq!(loco.jumpjet_current_speed, SimFixed::from_num(14));
    }

    #[test]
    fn test_turn_rate_limits_facing() {
        // Current=0 (north), desired=64 (east), turn_rate=8.
        let result = apply_turn_rate(0, 64, 8);
        assert_eq!(result, 8, "Should turn by max 8 units");
    }

    #[test]
    fn test_turn_rate_shortest_arc() {
        // Current=250, desired=10 — shortest arc is +16 (wrapping through 0).
        let result = apply_turn_rate(250, 10, 8);
        // Should turn +8 (from 250 toward 10 through 0).
        assert_eq!(
            result, 2,
            "Should wrap through 0: 250 + 8 = 258 mod 256 = 2"
        );
    }

    #[test]
    fn test_turn_rate_already_at_target() {
        let result = apply_turn_rate(64, 64, 8);
        assert_eq!(result, 64);
    }

    #[test]
    fn test_wobble_nonzero_when_enabled() {
        let (wx, wy) = compute_wobble(100, 42, 0.15, 40);
        // At least one axis should be non-zero with these params.
        assert!(wx.abs() > 0.0001 || wy.abs() > 0.0001);
    }

    #[test]
    fn test_wobble_zero_when_disabled() {
        let (wx, wy) = compute_wobble(100, 42, 0.0, 40);
        assert!((wx).abs() < 0.0001);
        assert!((wy).abs() < 0.0001);

        let (wx2, wy2) = compute_wobble(100, 42, 0.15, 0);
        assert!((wx2).abs() < 0.0001);
        assert!((wy2).abs() < 0.0001);
    }

    #[test]
    fn test_should_land_balloon_hover_false() {
        let mut loco = make_test_jj_loco();
        loco.balloon_hover = false;
        loco.air_phase = AirMovePhase::Hovering;
        assert!(should_land(&loco));
    }

    #[test]
    fn test_should_not_land_balloon_hover_true() {
        let mut loco = make_test_jj_loco();
        loco.balloon_hover = true;
        loco.air_phase = AirMovePhase::Hovering;
        assert!(!should_land(&loco));
    }

    #[test]
    fn test_infantry_walk_fallback() {
        assert!(should_use_walk_fallback(false, true, 2));
        assert!(should_use_walk_fallback(false, true, 3));
        assert!(!should_use_walk_fallback(false, true, 4));
        assert!(!should_use_walk_fallback(true, true, 2)); // hover_attack blocks fallback
        assert!(!should_use_walk_fallback(false, false, 2)); // not infantry
    }

    #[test]
    fn test_jumpjet_crash_descent() {
        let mut loco = make_test_jj_loco();
        loco.altitude = SimFixed::from_num(500);
        loco.air_phase = AirMovePhase::Descending;
        loco.jumpjet_crash_speed = SimFixed::from_num(150); // (5+5)*15

        tick_jumpjet_altitude(&mut loco, SIM_ONE);
        assert_eq!(
            loco.altitude,
            SimFixed::from_num(350),
            "Should descend at crash speed"
        );
    }

    fn make_test_jj_loco() -> LocomotorState {
        use crate::rules::locomotor_type::{LocomotorKind, SpeedType};
        use crate::sim::movement::locomotor::{GroundMovePhase, MovementLayer};
        LocomotorState {
            kind: LocomotorKind::Jumpjet,
            slot: LocomotorSlot::from_kind(LocomotorKind::Jumpjet),
            powered: true,
            piggyback: None,
            runtime_payload: crate::sim::movement::locomotion::LocomotorRuntimePayload::for_kind(
                LocomotorKind::Jumpjet,
                0,
            ),
            layer: MovementLayer::Air,
            phase: GroundMovePhase::Idle,
            air_phase: AirMovePhase::Landed,
            speed_multiplier: SIM_ONE,
            speed_fraction: SIM_ONE,
            fly_current_speed: SIM_ZERO,
            altitude: SIM_ZERO,
            target_altitude: SimFixed::from_num(500),
            climb_rate: sim_from_f32(75.0),
            jumpjet_speed: SimFixed::from_num(14),
            jumpjet_accel: SimFixed::from_num(2),
            jumpjet_current_speed: SIM_ZERO,
            jumpjet_deviation: 40,
            jumpjet_crash_speed: SimFixed::from_num(150),
            jumpjet_turn_rate: 4,
            balloon_hover: true,
            hover_attack: true,
            speed_type: SpeedType::Track,
            movement_zone: crate::rules::locomotor_type::MovementZone::Normal,
            rot: 0,
            air_progress: SIM_ZERO,
            infantry_wobble_phase: 0.0,
            subcell_dest: None,
            hover_throttle: crate::util::fixed_math::SIM_ZERO,
            hover_speed_request: crate::util::fixed_math::SIM_ZERO,
            hover_bob_offset: crate::util::fixed_math::SIM_ZERO,
        }
    }
}
