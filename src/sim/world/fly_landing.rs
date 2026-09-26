//! Fly landing4CE840 and changed-layer suffix4CD380..4CD4DE.
//! The surrounding Mark/Display pair belongs to lifecycle::complete_fly_phase.
//! Native executable corpus: tools/spatial_oracle/fly_landing_phase.*.
use super::{Simulation, display_layers::DisplayLayer};
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::movement::{
    DestinationTiming, air_movement, ground_pose, locomotor::MovementLayer,
};
use crate::util::fixed_math::{SIM_ZERO, SimFixed};

impl Simulation {
    /// BeginTakeoff4CF9B9 adds only when no existing bucket is retained.
    pub(crate) fn register_fly_air_tracker(&mut self, id: u64) {
        let Some(e) = self.substrate.entities.get(id) else {
            return;
        };
        if e.air_spatial_bucket.is_some() {
            return;
        }
        let bucket = crate::sim::occupancy::air_spatial_bucket_index(
            e.position.rx,
            e.position.ry,
            self.session.map_width,
            self.session.map_height,
        );
        let order = self.substrate.next_occupancy_enter_order.next();
        let e = self.substrate.entities.get_mut(id).unwrap();
        e.air_spatial_bucket = Some(bucket);
        e.air_spatial_enter_order = order;
    }

    /// `AircraftTracker::Remove @ 0x004135D0`: a Fly leaves the airborne index
    /// at its touchdown or its crash impact, a Jumpjet at its crash impact
    /// (`0x0054D075`).
    pub(crate) fn aircraft_tracker_remove(&mut self, id: u64) {
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.air_spatial_bucket = None;
            entity.air_spatial_enter_order = 0;
        }
    }

    pub(crate) fn finish_fly_takeoff_entry(&mut self, id: u64, rules: Option<&RuleSet>) {
        self.register_fly_air_tracker(id);
        let Some(e) = self.substrate.entities.get_mut(id) else {
            return;
        };
        air_movement::ensure_fly_facings(e);
        if air_movement::current_fly_height(e, self.resolved_terrain.as_ref()) == 0 {
            e.body_facing.as_mut().unwrap().snap(
                e.barrel_facing.unwrap().destination(),
                self.session.binary_frame,
            );
        }
        let sound = rules
            .and_then(|r| r.object(self.interner.resolve(e.type_ref())))
            .and_then(|o| o.aux_sound1.as_deref());
        if let Some(sound) = sound {
            let c = ground_pose::position_world_coord(&e.position);
            let world = crate::sim::anim_class::AnimWorldCoord {
                x: c.x,
                y: c.y,
                z: c.z,
            };
            let sound_id = self.interner.intern(sound);
            self.sound_events
                .push(super::SimSoundEvent::AircraftPhase { sound_id, world });
        }
    }

    /// `ObjectClass::SetHeight` (vtable `+0x1CC`): the Location's Z at
    /// `height` above the floor, or above the deck for an object on a bridge.
    pub(crate) fn set_object_height(&mut self, id: u64, height: i32) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let xy = ground_pose::position_world_xy(&entity.position);
        let ground =
            ground_pose::ground_surface_z_at(xy, false, self.resolved_terrain.as_ref(), None)
                .unwrap_or(0);
        let entity = self.substrate.entities.get_mut(id).unwrap();
        entity.position.exact_z_leptons = Some(
            ground
                .wrapping_add(height)
                .wrapping_add(if entity.on_bridge { 416 } else { 0 }),
        );
        if let Some(loco) = entity.locomotor.as_mut() {
            loco.altitude = SimFixed::from_num(height.clamp(-32768, 32767));
        }
    }

    pub(super) fn fly_building_at(&self, coord: DriveCoord) -> Option<u64> {
        let requested = ((coord.x / 256) as i16, (coord.y / 256) as i16);
        // Preserve the shared Dummy stamp made by MapAtCoord before lookup.
        let (x, y) = self.resolved_terrain.as_ref().map_or(requested, |terrain| {
            terrain.native_cell_coord(terrain.native_cell_identity(requested))
        });
        self.substrate
            .occupancy
            .first_building_on_layer(x as u16, y as u16, MovementLayer::Ground)
    }

    pub(super) fn apply_fly_landing_callback(&mut self, id: u64, rules: Option<&RuleSet>) {
        let entity = self
            .substrate
            .entities
            .get(id)
            .expect("admitted Fly landing");
        let coord = ground_pose::position_world_coord(&entity.position);
        let mut height = air_movement::current_fly_height(entity, self.resolved_terrain.as_ref());
        let bridge = self.resolved_terrain.as_ref().is_some_and(|terrain| {
            let cell =
                terrain.native_cell_identity(((coord.x / 256) as i16, (coord.y / 256) as i16));
            terrain.native_cell_flags(cell) & 0x100 != 0
        });
        // Unlike takeoff,4CE8AF does not test the owner's OnBridge flag.
        if bridge && height >= 416 {
            height = height.wrapping_sub(416);
        }
        let object = rules.and_then(|r| r.object(self.interner.resolve(entity.type_ref())));
        let dropship = object.is_some_and(|o| o.is_dropship);
        let carryall =
            entity.category == EntityCategory::Aircraft && object.is_some_and(|o| o.carryall);
        let airport_bound = entity
            .locomotor
            .as_ref()
            .unwrap()
            .fly_runtime()
            .unwrap()
            .airport_bound();
        let sound = object.and_then(|o| o.aux_sound2.clone());
        let entity = self.substrate.entities.get_mut(id).unwrap();
        if dropship && height == 0 {
            entity.flight_attitude.settle();
        }
        entity.locomotor.as_mut().unwrap().speed_fraction = SIM_ZERO;
        let entity = self.substrate.entities.get(id).unwrap();
        let base = crate::sim::aircraft::landing_base::landing_base(
            entity,
            &self.substrate.entities,
            rules.map(|r| (r, &self.interner)),
        );
        let destination = entity
            .locomotor
            .as_ref()
            .unwrap()
            .fly_runtime()
            .unwrap()
            .destination();
        let admitted = if airport_bound {
            self.fly_building_at(destination).is_some_and(|building| {
                self.substrate
                    .entities
                    .get(id)
                    .unwrap()
                    .radio_contacts
                    .contains(building)
            })
        } else {
            self.fly_landing_destination_admitted(id, destination)
        };
        if !admitted && !self.retry_fly_landing(id, rules) {
            return;
        }
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        let effect = entity
            .locomotor
            .as_mut()
            .unwrap()
            .fly_runtime_mut()
            .unwrap()
            .admit_landing_effect(height);
        if effect {
            let coord = ground_pose::position_world_coord(&entity.position);
            let ground = ground_pose::ground_surface_z_at(
                [coord.x, coord.y],
                false,
                self.resolved_terrain.as_ref(),
                None,
            )
            .unwrap_or(0);
            let world = crate::sim::anim_class::AnimWorldCoord {
                x: coord.x,
                y: coord.y,
                z: ground,
            };
            if let Some(rules) = rules {
                let animation = if dropship {
                    Some("DROPLAND")
                } else if carryall {
                    Some("CARYLAND")
                } else {
                    None
                };
                if let Some(animation) = animation {
                    let name = self.interner.intern(animation);
                    let (rx, ry, sx, sy, z) = world.to_cell_sub_z();
                    let descriptor = crate::sim::components::AnimClassSpawnDescriptor {
                        loop_count: 1,
                        draw_flags: 0x600,
                        ..crate::sim::components::AnimClassSpawnDescriptor::new(
                            name, rx, ry, sx, sy, z,
                        )
                    };
                    if let Err(error) = self.spawn_anim_at_world(rules, descriptor, world) {
                        log::debug!("landing animation [{animation}] did not construct: {error}");
                    }
                }
            }
            if self
                .substrate
                .entities
                .get(id)
                .is_some_and(|e| e.health.current > 0)
                && let Some(sound) = sound
            {
                let sound_id = self.interner.intern(&sound);
                self.sound_events
                    .push(super::SimSoundEvent::AircraftPhase { sound_id, world });
            }
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if height > base || entity.flight_attitude.blocks_landing() {
            return;
        }
        let coord = ground_pose::position_world_coord(&entity.position);
        //4CECAF queries the CURRENT cell again after refusal/effect callbacks.
        let bridge = self.resolved_terrain.as_ref().is_some_and(|terrain| {
            let cell =
                terrain.native_cell_identity(((coord.x / 256) as i16, (coord.y / 256) as i16));
            terrain.native_cell_flags(cell) & 0x100 != 0
        });
        if bridge {
            let ground = ground_pose::ground_surface_z_at(
                [coord.x, coord.y],
                false,
                self.resolved_terrain.as_ref(),
                None,
            )
            .unwrap_or(0);
            if coord.z >= ground.wrapping_add(416) {
                self.substrate.entities.get_mut(id).unwrap().on_bridge = true;
            }
        }
        self.set_object_height(id, base);
        self.aircraft_tracker_remove(id);
        let entity = self.substrate.entities.get_mut(id).unwrap();
        let loco = entity.locomotor.as_mut().unwrap();
        loco.fly_runtime_mut().unwrap().finish_landing();
        entity.foot_speed.applied_fraction = SIM_ZERO;
        loco.speed_fraction = SIM_ZERO;
        loco.fly_current_speed = SIM_ZERO;
        self.foot_neighbors_after_fly_landing(id);
        let entity = self.substrate.entities.get(id).unwrap();
        let destination = entity
            .locomotor
            .as_ref()
            .unwrap()
            .fly_runtime()
            .unwrap()
            .destination();
        let current = ground_pose::position_world_coord(&entity.position);
        let same = ((destination.x / 256) as i16, (destination.y / 256) as i16)
            == ((current.x / 256) as i16, (current.y / 256) as i16);
        if same || self.fly_building_at(destination) == entity.radio_contacts.slot(0) {
            self.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .locomotor
                .as_mut()
                .unwrap()
                .fly_runtime_mut()
                .unwrap()
                .finish_destination();
            self.clear_landed_fly_destination(id, rules);
        }
    }

    /// Landing4CEF6E -> null MoveTo -> AssignDestination(NULL,1). Both native
    /// IsMoving inputs are now zero; Foot's Stop receiver returns immediately.
    fn clear_landed_fly_destination(&mut self, id: u64, rules: Option<&RuleSet>) {
        let entity = self.substrate.entities.get(id).unwrap();
        if !air_movement::fly_coordinate_admitted(entity) {
            self.clear_fly_foot_destination(id, rules);
            return;
        }
        let current = ground_pose::position_world_coord(&entity.position);
        let height = air_movement::current_fly_height(entity, self.resolved_terrain.as_ref());
        let base = crate::sim::aircraft::landing_base::landing_base(
            entity,
            &self.substrate.entities,
            rules.map(|r| (r, &self.interner)),
        );
        let entity = self.substrate.entities.get_mut(id).unwrap();
        let begin = entity
            .locomotor
            .as_mut()
            .unwrap()
            .fly_runtime_mut()
            .unwrap()
            .null_destination(current, height, base);
        debug_assert!(!begin, "landed null MoveTo cannot request another landing");
        self.clear_fly_foot_destination(id, rules);
    }

    fn clear_fly_foot_destination(&mut self, id: u64, rules: Option<&RuleSet>) {
        let entity = self.substrate.entities.get_mut(id).unwrap();
        entity.navigation.nav_com_aux = None;
        entity.navigation.nav_com = None;
        entity.navigation.pending_arrival_clear = false;
        entity.movement_target = None;
        DestinationTiming::from_rules(self.session.binary_frame, rules).accept(entity);
    }

    /// Aircraft4196B0: Winged passability always succeeds. The mode0 owned
    /// aircraft arm reads native ground shroud knowledge, not current sight.
    fn fly_landing_destination_admitted(&self, id: u64, dest: DriveCoord) -> bool {
        let e = self.substrate.entities.get(id).unwrap();
        let cell = ((dest.x / 256) as i16, (dest.y / 256) as i16);
        if let Some(t) = &self.resolved_terrain {
            t.native_cell_identity(cell);
        }
        if self.session.game_mode_nonzero
            || !e.discovery.owned_by_current_house
            || e.is_mission_only()
        {
            return true;
        }
        let xyz = crate::sim::movement::target_cell_coord(
            cell.0 as u16,
            cell.1 as u16,
            self.resolved_terrain.as_ref(),
        );
        let q = xyz.z / 104;
        let offset = q / 2 + i32::from(q & 1 != 0);
        let projected = (
            ((xyz.x / 256) as i16).wrapping_sub(offset as i16),
            ((xyz.y / 256) as i16).wrapping_sub(offset as i16),
        );
        let visible = |c: (i16, i16)| {
            let resolved = self.resolved_terrain.as_ref().map_or(c, |t| {
                let identity = t.native_cell_identity(c);
                t.native_cell_coord(identity)
            });
            (
                self.fog
                    .is_ground_unshrouded(e.owner(), resolved.0 as u16, resolved.1 as u16),
                resolved,
            )
        };
        let (first_visible, first_cell) = visible(projected);
        first_visible
            || q & 1 != 0 && visible((first_cell.0.wrapping_add(1), first_cell.1.wrapping_add(1))).0
    }

    /// Foot4DDC60: height-aware playfield, nearest ground-list Techno, then
    /// Track/Normal passability and other live aircraft's Cell NavComs.
    pub(super) fn fly_landing_cell_admitted(&self, id: u64, rules: Option<&RuleSet>) -> bool {
        use crate::rules::locomotor_type::{MovementZone, SpeedType};
        use crate::sim::cell_rect::{
            IsClearToMoveResult, LiveCellPassabilityQuery, cell_is_in_playfield_height_aware,
            evaluate_live_cell_passability,
        };
        let e = self.substrate.entities.get(id).unwrap();
        let coord = ground_pose::position_world_coord(&e.position);
        let requested = ((coord.x / 256) as i16, (coord.y / 256) as i16);
        // The caller passes the Cell returned by MapAtCoord; Foot4DDC84
        // reads that receiver's coordinates. Fixed512 aliases must use the
        // resolved cell for playfield, occupant and passability queries.
        let identity = self
            .resolved_terrain
            .as_ref()
            .map(|t| t.native_cell_identity(requested));
        let cell = self
            .resolved_terrain
            .as_ref()
            .map_or(requested, |t| t.native_cell_coord(identity.unwrap()));
        if !cell_is_in_playfield_height_aware(
            (i32::from(cell.0), i32::from(cell.1)),
            self.playfield_bounds,
            self.resolved_terrain.as_ref(),
        ) {
            return false;
        }
        if let Some(other) = super::techno_ai_cloak::find_nearest_object_in_cell(
            self,
            (cell.0 as u16, cell.1 as u16),
        ) {
            return other == id
                || e.radio_contacts.slot(0) == Some(other)
                || rules
                    .and_then(|r| r.object(self.interner.resolve(e.type_ref())))
                    .is_some_and(|o| o.spawned)
                    && self.substrate.entities.get(other).is_some_and(|o| {
                        o.spawn_manager.is_some()
                            || rules
                                .and_then(|r| r.object(self.interner.resolve(o.type_ref())))
                                .is_some_and(|o| o.spawned)
                    });
        }
        let grid = self.path_grid_snapshot();
        let land = self
            .resolved_terrain
            .as_ref()
            .and_then(|t| t.cell(cell.0 as u16, cell.1 as u16))
            .is_some_and(|c| {
                rules
                    .and_then(|r| r.terrain_rules.semantics_for_land_type(c.yr_cell_land_type))
                    .and_then(|row| row.cost_for_speed_type(SpeedType::Track))
                    .is_some_and(|cost| cost > 0)
            });
        let result = evaluate_live_cell_passability(LiveCellPassabilityQuery {
            target: (cell.0 as u16, cell.1 as u16),
            speed_type: SpeedType::Track,
            movement_zone: MovementZone::Normal,
            requested_zone: None,
            actual_zone: 0,
            requested_layer: None,
            ignore_infantry: false,
            ignore_vehicles: false,
            land_passable: land,
            path_grid: grid.as_deref(),
            resolved_terrain: self.resolved_terrain.as_ref(),
            raw_occupation: Some(&self.substrate.raw_cell_occupation),
        });
        if !matches!(
            result,
            IsClearToMoveResult::Clear { .. } | IsClearToMoveResult::ClearWinged
        ) {
            return false;
        }
        !self.substrate.entities.values().any(|other| {
            other.stable_id() != id
                && other.category == EntityCategory::Aircraft
                && other.lifecycle.object_alive
                && !other.lifecycle.in_limbo
                && match other.navigation.nav_com {
                    Some(NavTargetRef::Cell { rx, ry }) => {
                        self.resolved_terrain.as_ref().map_or(
                            (rx, ry) == (cell.0 as u16, cell.1 as u16),
                            |t| {
                                use crate::map::cell_index::NativeCellIdentity;
                                //4DDDBF compares retained pointers. Do not
                                // restamp the shared Dummy while comparing.
                                let reserved = t
                                    .native_fixed_cell_index(rx as i16, ry as i16)
                                    .map_or(NativeCellIdentity::Dummy, NativeCellIdentity::Real);
                                Some(reserved) == identity
                            },
                        )
                    }
                    _ => false,
                }
        })
    }

    pub(super) fn finish_fly_layer_transition(
        &mut self,
        id: u64,
        layer: Option<DisplayLayer>,
        rules: Option<&RuleSet>,
    ) {
        use crate::sim::radio::{self, RadioMessage};
        if layer == Some(DisplayLayer::GROUND) {
            self.clear_fly_foot_destination(id, rules);
            radio::broadcast(self, id, RadioMessage::Tether, rules);
            if let Some(entity) = self.substrate.entities.get(id) {
                let config = crate::sim::vision::VisionConfig {
                    require_playfield_membership: true,
                    veteran_sight: rules.map_or(0.0, |r| r.general.veteran_sight),
                    leptons_per_sight_increase: rules
                        .map_or(0, |r| r.general.leptons_per_sight_increase),
                    reveal_by_height: rules.is_none_or(|r| r.general.reveal_by_height),
                    fog_of_war: self.session.game_options.fog_of_war,
                };
                let grid = self.path_grid_snapshot();
                let heights = grid.as_ref().map(|g| g.ground_height_grid());
                let ability =
                    crate::sim::vision::entity_has_sight_ability(entity, &self.interner, rules);
                crate::sim::vision::force_refresh_entity_vision(
                    &mut self.fog,
                    entity,
                    &config,
                    heights.as_deref(),
                    ability,
                    &self.interner,
                );
                //567DA0 updates client fog edges/redraw requests. VERA's
                // ShroudBuffer rebuilds from this owner when its generation
                // changes; there is no second simulation fog-edge authority.
            }
        } else {
            radio::broadcast(self, id, RadioMessage::Untether, rules);
            if self.substrate.entities.get(id).is_some_and(|e| {
                !e.radio_contacts.is_empty()
                    && e.navigation.nav_com.is_some()
                    && match e.navigation.nav_com {
                        Some(
                            NavTargetRef::Entity { id }
                            | NavTargetRef::Object { id }
                            | NavTargetRef::Building { id },
                        ) => e.radio_contacts.slot(0) != Some(id),
                        Some(NavTargetRef::Cell { .. }) => true,
                        None => false,
                    }
            }) {
                radio::broadcast_break(self, id, rules);
            }
        }
    }

    fn retry_fly_landing(&mut self, id: u64, rules: Option<&RuleSet>) -> bool {
        use crate::rules::locomotor_type::{MovementZone, SpeedType};
        use crate::sim::find_nearby_cell::{
            NearbyAnchorGate, NearbyFootprint, NearbyQuery, PassabilityArgs,
            find_nearby_passable_cell, map_owned_radius_cap,
        };
        let Some(rules) = rules else {
            return false;
        };
        let e = self.substrate.entities.get(id).unwrap();
        let coord = ground_pose::position_world_coord(&e.position);
        self.begin_fly_takeoff(id, Some(rules));
        let grid = self.path_grid_snapshot();
        let size = self
            .playfield_bounds
            .zip(self.playfield_size_height)
            .map(|(b, h)| (b.base, h));
        let Some((width, height)) = size else {
            return false;
        };
        let target = find_nearby_passable_cell(
            ((coord.x / 256) as i16 as i32, (coord.y / 256) as i16 as i32),
            &NearbyQuery {
                native_cells: None,
                raw_occupation: Some(&self.substrate.raw_cell_occupation),
                passability: PassabilityArgs {
                    speed_type: SpeedType::Track,
                    required_zone_id: None,
                    movement_zone: MovementZone::Fly,
                    bridge_aware_zone: false,
                },
                footprint: NearbyFootprint::SINGLE,
                anchor_gate: NearbyAnchorGate::NativeHeightAware,
                allow_bridge_cells: true,
                check_height: false,
                check_occupancy: false,
                radius_cap: map_owned_radius_cap(width, height),
                target_cell: None,
                path_grid: grid.as_deref(),
                resolved_terrain: self.resolved_terrain.as_ref(),
                overlay_grid: self.overlay_grid.as_ref(),
                occupancy: Some(&self.substrate.occupancy),
                entities: Some(&self.substrate.entities),
                zone_grid: self.zone_grid.as_ref(),
                playfield_bounds: self.playfield_bounds,
            },
            self.session.binary_frame,
        );
        if let Some(cell) = target {
            //4CEAD9..4CEB35 constructs from the selected CellStruct, then
            // queries world ground before adding the structural bridge rise.
            let mut coord = DriveCoord::cell(cell.0, cell.1, 0);
            coord.z = ground_pose::ground_surface_z_at(
                [coord.x, coord.y],
                false,
                self.resolved_terrain.as_ref(),
                None,
            )
            .unwrap_or(0);
            if let Some(terrain) = self.resolved_terrain.as_ref() {
                let identity = terrain.native_cell_identity((cell.0 as i16, cell.1 as i16));
                if terrain.native_cell_flags(identity) & 0x100 != 0 {
                    coord.z = coord.z.wrapping_add(416);
                }
            }
            self.move_air_coordinate(id, coord, SIM_ZERO, None, Some(rules));
            true
        } else {
            let e = self.substrate.entities.get(id).unwrap();
            let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
                id,
                e.health.current,
                0,
                crate::sim::combat::RAD_NO_ATTACKER,
                None,
                self.interner.intern(&rules.bridge_warheads.c4_name),
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses: true,
                    arg6: true,
                },
            );
            self.commit_direct_damage_receiver(rules, None, event);
            if let Some(state) = self
                .substrate
                .entities
                .get_mut(id)
                .and_then(|e| e.locomotor.as_mut())
                .and_then(|l| l.fly_runtime_mut())
            {
                state.clear_destination_after_failed_landing();
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::movement::{
        fly_height::{FlightAttitude, FlyRuntime},
        locomotor::LocomotorState,
    };
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::world::lifecycle_tests::{insert_entity, install_common_raw_terrain};

    fn fixture(row: &serde_json::Value) -> (Simulation, RuleSet) {
        let input = &row["input"];
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[General]\nFlightLevel=1500\n[AircraftTypes]\n0=TEST\n[TEST]\nLandable=yes\nStrength=100\nSpeed=10\nFlightLevel={}\nLocomotor={{4A582746-9839-11D1-B709-00A024DDAFD1}}\nIsDropship={}\nAirportBound={}\n",
            input["flight_level"].as_i64().unwrap_or(-1),
            input["dropship"].as_bool().unwrap_or(false),
            input["airport_bound"].as_bool().unwrap_or(false),
        ))).unwrap();
        let mut rules = rules;
        rules.general.blockage_path_delay_ticks = 11;
        let mut sim = Simulation::with_seed(31);
        sim.session.map_width = 128;
        sim.session.map_height = 128;
        sim.playfield_bounds = Some(
            crate::map::playfield::PlayfieldBounds::from_normalized_local_size(64, 0, 0, 64, 64),
        );
        sim.playfield_size_height = Some(64);
        install_common_raw_terrain(
            &mut sim,
            128,
            128,
            0,
            input["bridge"]
                .as_bool()
                .unwrap_or(false)
                .then_some((64, 64)),
        );
        let cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(64, 64)
            .unwrap();
        cell.level = input["level"].as_u64().unwrap_or(0) as u8;
        cell.slope_type = input["slope"].as_u64().unwrap_or(0) as u8;
        sim.overlay_grid =
            Some(crate::sim::overlay_grid::OverlayGrid::new_with_retained_wall_plane(128, 128));
        sim.overlay_grid
            .as_mut()
            .unwrap()
            .seed_neighbor_counts_for_tests(input["neighbor_seed"].as_u64().unwrap_or(0) as u8);
        let id = sim.allocate_stable_id();
        assert_eq!(id, 1);
        insert_entity(&mut sim, id, EntityCategory::Aircraft);
        let before = &row["before"];
        let e = sim.substrate.entities.get_mut(id).unwrap();
        e.lifecycle.object_alive = true;
        e.lifecycle.in_limbo = false;
        e.position.rx = 64;
        e.position.ry = 64;
        e.position.sub_x = SimFixed::from_num(128);
        e.position.sub_y = SimFixed::from_num(128);
        e.position.exact_z_leptons = Some(input["registration_z"].as_i64().unwrap_or(900) as i32);
        e.health.current = input["health"].as_i64().unwrap_or(100) as i32;
        e.on_bridge = input["on_bridge"].as_bool().unwrap_or(false);
        e.locomotor = Some(LocomotorState::from_object_type(
            rules.object("TEST").unwrap(),
            0,
        ));
        let l = e.locomotor.as_mut().unwrap();
        *l.fly_runtime_mut().unwrap()=serde_json::from_value::<FlyRuntime>(serde_json::json!({"target_height":0,"taking_off":false,"landing":before["phase"][1]==1,"landing_effect_latched":before["phase"][2]==1,"airport_bound":input["airport_bound"].as_bool().unwrap_or(false),"moving":true,"destination":before["destination"]})).unwrap();
        l.speed_fraction = SimFixed::lit("0.75");
        l.fly_current_speed = SimFixed::lit("0.5");
        e.flight_attitude=serde_json::from_value::<FlightAttitude>(serde_json::json!({"pitch":SimFixed::from_num(input["owner_float_2e8"].as_f64().unwrap_or(0.0))})).unwrap();
        e.navigation.neighbor_state =
            serde_json::from_value(serde_json::json!({"cell": before["neighbor_cell"]})).unwrap();
        e.navigation.path_runtime.blocked_timer = crate::sim::timer::CdTimer::from_raw(0, 0);
        if input["air_registered"].as_bool().unwrap_or(true) {
            sim.register_fly_air_tracker(id);
        }
        sim.add_entity_occupancy(id);
        sim.submit_entity_display(id, Some(&rules), None);
        let e = sim.substrate.entities.get_mut(id).unwrap();
        e.position.exact_z_leptons = Some(before["coordinates"][2].as_i64().unwrap() as i32);
        if input["blocking_unit"].as_bool().unwrap_or(false)
            || input["reserved"].as_bool().unwrap_or(false)
        {
            let id = sim.allocate_stable_id();
            insert_entity(
                &mut sim,
                id,
                if input["reserved"].as_bool().unwrap_or(false) {
                    EntityCategory::Aircraft
                } else {
                    EntityCategory::Unit
                },
            );
            let e = sim.substrate.entities.get_mut(id).unwrap();
            e.lifecycle.object_alive = true;
            e.lifecycle.in_limbo = false;
            e.position.rx = 64;
            e.position.ry = 64;
            e.position.sub_x = SimFixed::from_num(128);
            e.position.sub_y = SimFixed::from_num(128);
            e.position.exact_z_leptons = Some(0);
            if input["reserved"].as_bool().unwrap_or(false) {
                e.navigation.nav_com = Some(NavTargetRef::cell(64, 64));
            } else {
                sim.add_entity_occupancy(id);
            }
        }
        sim.session.binary_frame = 100;
        (sim, rules)
    }

    #[test]
    fn fly_takeoff_entry_matches_original_spatial_and_facing_transaction() {
        use crate::sim::movement::FacingClass;
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/fly_takeoff_entry.json"
        ))
        .unwrap();
        assert_eq!(rows.len(), 26);
        for row in rows {
            let input = &row["input"];
            let supplied = serde_json::json!({"input":input, "before":row["before"]["state"]});
            let (mut sim, rules) = fixture(&supplied);
            let e = sim.substrate.entities.get_mut(1).unwrap();
            let loco = e.locomotor.as_mut().unwrap();
            loco.powered = input["powered"].as_bool().unwrap_or(true);
            loco.set_fly_target_height(37);
            loco.fly_runtime_mut().unwrap().finish_destination();
            for (slot, initial, target) in [
                (&mut e.body_facing, 0x4000, 0xC000),
                (&mut e.barrel_facing, 0x6000, 0x2000),
            ] {
                let mut facing = FacingClass::new(initial, 5);
                facing.set(target, 90);
                *slot = Some(facing);
            }
            let rng = sim.scenario_rng.logical_state();
            if input["move"].as_bool().unwrap() {
                sim.move_air_coordinate(
                    1,
                    DriveCoord {
                        x: 16768,
                        y: 16512,
                        z: 0,
                    },
                    SimFixed::from_num(10),
                    None,
                    Some(&rules),
                );
            } else {
                sim.begin_fly_takeoff(1, Some(&rules));
            }
            let e = sim.substrate.entities.get(1).unwrap();
            let state = e.locomotor.as_ref().unwrap().fly_runtime().unwrap();
            let expected = &row["after"];
            assert_eq!(
                state.target_height(),
                expected["target_height"].as_i64().unwrap() as i32,
                "{input}"
            );
            assert_eq!(
                state.cruise_mode(),
                expected["mode"].as_bool().unwrap(),
                "{input}"
            );
            assert_eq!(
                serde_json::json!([
                    u8::from(state.taking_off()),
                    u8::from(state.landing()),
                    u8::from(state.landing_effect_latched())
                ]),
                expected["state"]["phase"],
                "{input}"
            );
            assert_eq!(
                e.air_spatial_bucket.is_some(),
                expected["state"]["air_members"] == 1,
                "{input}"
            );
            assert_eq!(
                state.moving(),
                expected["state"]["moving"].as_bool().unwrap(),
                "{input}"
            );
            let destination = state.destination();
            assert_eq!(
                serde_json::json!([destination.x, destination.y, destination.z]),
                expected["state"]["destination"],
                "{input}"
            );
            for (facing, expected) in [e.body_facing.unwrap(), e.barrel_facing.unwrap()]
                .into_iter()
                .zip(expected["facings"].as_array().unwrap())
            {
                let actual = serde_json::to_value(facing).unwrap();
                for (rust, native) in [
                    ("current", "destination"),
                    ("prev", "previous"),
                    ("start_frame", "start"),
                    ("duration_frames", "duration"),
                    ("rot_per_frame", "rate"),
                ] {
                    // Native stopped timers use -1; Rust owns that as None.
                    let native_value = if native == "start" && expected[native] == u32::MAX {
                        serde_json::Value::Null
                    } else {
                        expected[native].clone()
                    };
                    assert_eq!(actual[rust], native_value, "{input}; {native}");
                }
            }
            assert_eq!(sim.scenario_rng.logical_state(), rng, "{input}");
        }
    }

    #[test]
    fn fly_can_enter_matches_original_ground_shroud_queries() {
        use crate::sim::vision::OwnerVisibility;
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/fly_can_enter.json"
        ))
        .unwrap();
        assert_eq!(rows.len(), 102);
        let base: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/fly_landing_phase.json"
        ))
        .unwrap();
        for row in rows {
            let input = &row["input"];
            let supplied = serde_json::json!({"input":input,"before":base[1]["before"]});
            let (mut sim, _) = fixture(&supplied);
            sim.session.game_mode_nonzero = input["game_mode"].as_u64().unwrap_or(0) != 0;
            let e = sim.substrate.entities.get_mut(1).unwrap();
            e.discovery.owned_by_current_house = input["owned"].as_bool().unwrap_or(true);
            if input["mission_only"].as_bool().unwrap_or(false) {
                e.mark_mission_only();
            }
            let owner = e.owner();
            let mut visibility = serde_json::to_value(OwnerVisibility::new(128, 128)).unwrap();
            let x = row["exposed"][0].as_u64().unwrap() as usize;
            let y = row["exposed"][1].as_u64().unwrap() as usize;
            visibility["cell_runtime"][y * 128 + x]["alt_flags"] = input["bits"].clone();
            sim.fog
                .by_owner
                .insert(owner, serde_json::from_value(visibility).unwrap());
            assert!(!sim.fog.is_cell_visible(owner, x as u16, y as u16));
            let request = DriveCoord {
                x: row["query"][0].as_i64().unwrap() as i32,
                y: row["query"][1].as_i64().unwrap() as i32,
                z: row["query"][2].as_i64().unwrap() as i32,
            };
            let target = crate::sim::movement::target_cell_coord(
                (request.x / 256) as i16 as u16,
                (request.y / 256) as i16 as u16,
                sim.resolved_terrain.as_ref(),
            );
            assert_eq!(
                serde_json::json!([target.x, target.y, target.z]),
                row["coordinate"],
                "{input}"
            );
            assert_eq!(
                sim.fly_landing_destination_admitted(1, request),
                row["result"] == 0,
                "{input}"
            );
            sim.fog.build_merged_for(owner, &sim.interner);
            assert_eq!(
                sim.fly_landing_destination_admitted(1, request),
                row["result"] == 0,
                "{input}"
            );
        }
    }

    #[test]
    fn fly_landing_space_matches_original_occupants_and_reservations() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/fly_landing_space.json"
        ))
        .unwrap();
        assert_eq!(rows.len(), 26);
        let base: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/fly_landing_phase.json"
        ))
        .unwrap();
        for row in rows {
            let input = &row["input"];
            let (mut sim, _) = fixture(&base[1]);
            let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
                "[AircraftTypes]\n0=TEST\n[VehicleTypes]\n0=BLOCKER\n\
                 [TEST]\nSpawned={}\n[BLOCKER]\nSpawned={}\nSpawns=TEST\nSpawnsNumber=1\n\
                 [Clear]\nTrack={}%\n",
                input["spawned"].as_bool().unwrap_or(false),
                input["occupant_spawned"].as_bool().unwrap_or(false),
                if input["land_cost"].as_f64().unwrap_or(1.0) == 0.0 {
                    0
                } else {
                    100
                },
            )))
            .unwrap();
            let query = input
                .get("query")
                .cloned()
                .unwrap_or(serde_json::json!([64, 64]));
            let e = sim.substrate.entities.get_mut(1).unwrap();
            e.position.rx = query[0].as_u64().unwrap() as u16;
            e.position.ry = query[1].as_u64().unwrap() as u16;
            let owner = e.owner();
            let occupant = input["occupant"].as_str();
            if occupant == Some("self") {
                sim.remove_entity_occupancy(1);
                sim.add_entity_occupancy(1);
            } else if occupant.is_some() || input.get("reservation").is_some() {
                let type_ref = sim.interner.intern("BLOCKER");
                let mut other = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
                    2,
                    64,
                    64,
                    0,
                    0,
                    owner,
                    crate::sim::components::Health { current: 100 },
                    type_ref,
                    if occupant.is_some() {
                        EntityCategory::Unit
                    } else {
                        EntityCategory::Aircraft
                    },
                    0,
                    5,
                    true,
                );
                other.lifecycle.object_alive = input["alive"].as_bool().unwrap_or(true);
                other.lifecycle.in_limbo = input["limbo"].as_bool().unwrap_or(false);
                other.position.sub_x = SimFixed::from_num(128);
                other.position.sub_y = SimFixed::from_num(128);
                other.position.exact_z_leptons = Some(0);
                if input["spawn_manager"].as_bool().unwrap_or(false) {
                    other.spawn_manager = crate::sim::spawn_manager::init_spawn_manager(
                        rules.object("BLOCKER").unwrap(),
                        &rules,
                        &mut sim.interner,
                        100,
                    );
                    assert!(other.spawn_manager.is_some());
                }
                sim.substrate.entities.insert(other);
                if occupant.is_some() {
                    sim.add_entity_occupancy(2);
                }
            }
            if occupant == Some("contact") {
                sim.substrate
                    .entities
                    .get_mut(1)
                    .unwrap()
                    .radio_contacts
                    .insert(2);
            }
            assert_eq!(
                super::super::techno_ai_cloak::find_nearest_object_in_cell(&sim, (64, 64)),
                occupant.map(|kind| if kind == "self" { 1 } else { 2 }),
                "supplied native occupant: {input}",
            );
            if let Some(reserved) = input.get("reservation") {
                let id = if input["reservation_self"].as_bool().unwrap_or(false) {
                    1
                } else {
                    2
                };
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .navigation
                    .nav_com = Some(NavTargetRef::cell(
                    reserved[0].as_u64().unwrap() as u16,
                    reserved[1].as_u64().unwrap() as u16,
                ));
            }
            sim.substrate.raw_cell_occupation.mark_ground(
                64,
                64,
                input["occupation"].as_u64().unwrap_or(0) as u8,
            );
            let rng = sim.scenario_rng.logical_state();
            assert_eq!(
                sim.fly_landing_cell_admitted(1, Some(&rules)),
                row["result"].as_bool().unwrap(),
                "{input}"
            );
            assert_eq!(sim.scenario_rng.logical_state(), rng);
        }
    }

    #[test]
    fn fly_landing_phase_matches_complete_original_calls() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/fly_landing_phase.json"
        ))
        .unwrap();
        assert_eq!(rows.len(), 41);
        for row in rows {
            let (mut sim, rules) = fixture(&row);
            let rng = sim.scenario_rng.logical_state();
            sim.complete_fly_phase(1, Some(&rules));
            let e = sim.substrate.entities.get(1).unwrap();
            let l = e.locomotor.as_ref().unwrap();
            let f = l.fly_runtime().unwrap();
            let c = ground_pose::position_world_coord(&e.position);
            let after = &row["after"];
            assert_eq!(
                serde_json::json!([c.x, c.y, c.z]),
                after["coordinates"],
                "{}",
                row["input"]
            );
            assert_eq!(
                serde_json::json!([
                    u8::from(f.taking_off()),
                    u8::from(f.landing()),
                    u8::from(f.landing_effect_latched())
                ]),
                after["phase"],
                "{}",
                row["input"]
            );
            assert_eq!(
                serde_json::json!([
                    l.speed_fraction.to_num::<f64>(),
                    l.fly_current_speed.to_num::<f64>()
                ]),
                after["speeds"],
                "{}",
                row["input"]
            );
            let d = f.destination();
            assert_eq!(
                serde_json::json!([d.x, d.y, d.z]),
                after["destination"],
                "{}",
                row["input"]
            );
            assert_eq!(
                f.moving(),
                after["moving"].as_bool().unwrap(),
                "{}",
                row["input"]
            );
            assert_eq!(
                e.on_bridge,
                after["on_bridge"].as_bool().unwrap(),
                "{}",
                row["input"]
            );
            assert_eq!(
                e.lifecycle.cell_marked,
                after["marked"].as_bool().unwrap(),
                "{}",
                row["input"]
            );
            assert_eq!(
                u64::from(e.air_spatial_bucket.is_some()),
                after["air_members"].as_u64().unwrap(),
                "{}",
                row["input"]
            );
            let timer = e.navigation.path_runtime.blocked_timer;
            assert_eq!(
                serde_json::json!([timer.start_frame(), timer.duration()]),
                after["path_timer"],
                "{}",
                row["input"]
            );
            for index in 0..5 {
                assert_eq!(
                    sim.substrate
                        .display
                        .members(DisplayLayer::from_index(index).unwrap())
                        .len() as u64,
                    after["layers"][index as usize].as_u64().unwrap(),
                    "{}",
                    row["input"]
                );
            }
            let plane = sim
                .overlay_grid
                .as_ref()
                .unwrap()
                .retained_neighbor_counts()
                .unwrap();
            for c in after["neighbors"].as_array().unwrap() {
                assert_eq!(
                    u64::from(
                        plane[c[1].as_u64().unwrap() as usize * 128
                            + c[0].as_u64().unwrap() as usize]
                    ),
                    c[2].as_u64().unwrap(),
                    "{}",
                    row["input"]
                );
            }
            assert_eq!(rng, sim.scenario_rng.logical_state());
        }
    }

    #[test]
    fn fly_landing_production_descent_and_restore_retain_air_until_completion() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/fly_landing_phase.json"
        ))
        .unwrap();
        let row = rows
            .iter()
            .find(|r| r["input"]["name"] == "height_301")
            .unwrap();
        let (mut sim, rules) = fixture(row);
        sim.tick_air_movement_with_cell_lists_one(1, Some(&rules));
        let e = sim.substrate.entities.get(1).unwrap();
        assert!(e.air_spatial_bucket.is_some());
        assert!(
            e.locomotor
                .as_ref()
                .unwrap()
                .fly_runtime()
                .unwrap()
                .landing_effect_latched()
        );
        // Native Scenario Load deliberately reseeds its RNG to0. Normalize
        // both branches at this checkpoint so unrelated load policy is equal.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = GameSnapshot::save(&sim, 0, 0, "Fly landing continuation", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.retain_in_scenario_process_state_from(&sim);
        restored.resolved_terrain = sim.resolved_terrain.clone();
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(sim.state_hash(), restored.state_hash());
        for frame in 101..126 {
            for s in [&mut sim, &mut restored] {
                s.session.binary_frame = frame;
                s.tick_air_movement_with_cell_lists_one(1, Some(&rules));
            }
            assert_eq!(sim.state_hash(), restored.state_hash(), "frame{frame}");
        }
        let e = sim.substrate.entities.get(1).unwrap();
        assert_eq!(e.position.exact_z_leptons, Some(0));
        assert_eq!(e.air_spatial_bucket, None);
        assert!(
            !e.locomotor
                .as_ref()
                .unwrap()
                .fly_runtime()
                .unwrap()
                .has_phase_callback()
        );
        assert_eq!(
            sim.substrate.display.layer_of(1),
            Some(DisplayLayer::GROUND)
        );
    }
}
