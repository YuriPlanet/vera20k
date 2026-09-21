//! Jumpjet locomotor instance state and its destination acceptance.
//!
//! `JumpjetRuntime` is the locomotor's own persistent block (destination,
//! moving byte, the state field at `+0x50`, flight values); this module also
//! owns `Move_To`-side destination acceptance. The per-frame flight is the
//! native kernel in `jumpjet_flight`, hosted by `world::jumpjet_cruise`. An
//! older VERA-only physics model (acceleration curves, `f32` wobble, its own
//! altitude ticker over `AirMovePhase`) lived here with no caller left and is
//! gone.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/locomotor, sim/movement.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::util::fixed_math::{SIM_ZERO, SimFixed};

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
            let flight_level = rules.map_or(500, |rules| {
                self.substrate
                    .entities
                    .get(id)
                    .and_then(|e| rules.object(self.interner.resolve(e.type_ref())))
                    .map_or(rules.general.flight_level, |o| {
                        o.flight_level(rules.general.flight_level)
                    })
            });
            return super::air_movement::issue_air_move_command(
                &mut self.substrate.entities,
                id,
                target,
                speed,
                crate::sim::movement::DestinationTiming::from_rules(
                    self.session.binary_frame,
                    rules.into(),
                ),
                flight_level,
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
        use super::locomotor::MovementLayer;
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
        }
        true
    }
}

/// Max cells for infantry walk fallback (TS-style jumpjet infantry).
const INFANTRY_WALK_THRESHOLD_CELLS: u32 = 3;

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

    #[test]
    fn test_infantry_walk_fallback() {
        assert!(should_use_walk_fallback(false, true, 2));
        assert!(should_use_walk_fallback(false, true, 3));
        assert!(!should_use_walk_fallback(false, true, 4));
        assert!(!should_use_walk_fallback(true, true, 2)); // hover_attack blocks fallback
        assert!(!should_use_walk_fallback(false, false, 2)); // not infantry
    }
}
