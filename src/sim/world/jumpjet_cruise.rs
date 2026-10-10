//! Mutable production host for Jumpjet Process54AEC0.
//!
//! Owner callbacks run at their original call sites. In particular State1's
//! scatter executes Unit741970 -> Foot4D94B0 -> Jumpjet54B1C0 before its Mark
//! suffix and phase write; State3's tail reads that callback's new destination.
//! Flight state belongs to JumpjetRuntime, retained tracker/slot Cells belong
//! to Foot, and XYZ/Mark/raw occupation use their existing shared owners.
//!
//! Native composed controls and coverage: tools/spatial_oracle/jumpjet_states.md.
//! Other large mechanisms remain bounded: Magnetron arms, target-facing hold,
//! radio/tag arrival side effects and Infantry touchdown's full class chain.
//! Native Unit touchdown calls Cell PickupCrate481A00 at54C9F6 after phase0;
//! the touchdown asks `Simulation::pickup_crate_at` after the tracker removal
//! (54C9DC) as native does. The no-crate native controls do not establish
//! that separate mechanism.

use super::Simulation;
use crate::map::cell_index::NativeCellIdentity;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::retail_trig::{TrigTable, required_math_tables};
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::DriveCoord;
use crate::sim::movement::air_movement::AirMovementTickStats;
use crate::sim::movement::ground_pose::{
    foot_set_location, ground_surface_z_at, position_world_xy,
};
use crate::sim::movement::infantry_entry::InfantryEntryArgs;
use crate::sim::movement::jumpjet_movement::JumpjetRuntime;
use crate::sim::movement::jumpjet_movement::jumpjet_flight::{
    self, FlightOwnerKind, JumpjetFlightHost, STATE_DESCEND, STATE_HOLD, STATE_TRANSLATE,
};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::RawCellKey;
use crate::util::fixed_math::SimFixed;
use crate::util::lepton::{
    GROUND_LEVEL_HEIGHT_LEPTONS, ground_height_leptons, lepton_to_cell_packed,
};

struct CruiseHost<'a> {
    sim: &'a mut Simulation,
    frame: u32,
    trig: &'a TrigTable,
    rules: Option<&'a RuleSet>,
    registry: Option<&'a OverlayTypeRegistry>,
    stable_id: u64,
    touched_down: bool,
    impact: bool,
    crash_latched: bool,
}

impl CruiseHost<'_> {
    fn owner(&self) -> &crate::sim::game_entity::GameEntity {
        self.sim
            .substrate
            .entities
            .get(self.stable_id)
            .expect("Jumpjet Process owner exists")
    }

    fn terrain(&self) -> Option<&ResolvedTerrainGrid> {
        self.sim.resolved_terrain.as_ref()
    }

    fn object_type(&self) -> Option<&crate::rules::object_type::ObjectType> {
        self.rules
            .and_then(|rules| rules.object(self.sim.interner.resolve(self.owner().type_ref())))
    }

    fn slot_holder(&self, cell: (i16, i16)) -> Option<u64> {
        self.sim
            .substrate
            .air_slots
            .holder(cell.0 as u16, cell.1 as u16)
    }

    fn cell_terrain(&self, xy: [i32; 2]) -> (u8, u8) {
        let Some(terrain) = self.terrain() else {
            return (0, 0);
        };
        match terrain.native_cell_identity(self.cell_of(xy)) {
            NativeCellIdentity::Real(index) => {
                let cell = &terrain.cells()[index];
                (cell.level, cell.slope_type)
            }
            NativeCellIdentity::Dummy => {
                let dummy = terrain.shared_cell_dummy().snapshot();
                (dummy.level as u8, dummy.slope_type)
            }
        }
    }
}

impl JumpjetFlightHost for CruiseHost<'_> {
    fn binary_frame(&self) -> u32 {
        self.frame
    }
    fn trig(&self) -> &TrigTable {
        self.trig
    }
    fn owner_kind(&self) -> FlightOwnerKind {
        FlightOwnerKind::of(self.owner().category)
    }
    fn is_object_alive(&self) -> bool {
        self.owner().is_object_alive()
    }
    fn marked(&self) -> bool {
        self.owner().lifecycle.cell_marked
    }
    fn replace_marked_byte(&mut self, marked: bool) -> bool {
        self.sim.replace_foot_marked_byte(self.stable_id, marked)
    }
    fn location(&self) -> [i32; 3] {
        let xy = position_world_xy(&self.owner().position);
        let z =
            crate::sim::movement::ground_pose::object_world_z_leptons(self.owner(), self.terrain());
        [xy[0], xy[1], z]
    }
    fn set_location(&mut self, coord: [i32; 3]) {
        foot_set_location(
            &mut self.sim.substrate.entities,
            self.stable_id,
            DriveCoord {
                x: coord[0],
                y: coord[1],
                z: coord[2],
            },
            self.rules,
            &self.sim.interner,
        );
    }
    fn set_z(&mut self, z: i32) {
        self.sim
            .set_object_z(self.stable_id, z, self.rules, self.registry);
    }
    fn height_above_ground(&self) -> i32 {
        let location = self.location();
        location[2].wrapping_sub(
            ground_surface_z_at(
                [location[0], location[1]],
                self.on_bridge(),
                self.terrain(),
                None,
            )
            .unwrap_or(0),
        )
    }
    fn on_bridge(&self) -> bool {
        self.owner().on_bridge
    }
    fn grounded_reset(&mut self) {
        let location = self.location();
        self.sim.object_raw_receiver_at(
            self.stable_id,
            DriveCoord {
                x: location[0],
                y: location[1],
                z: location[2],
            },
            false,
        );
        self.sim
            .substrate
            .entities
            .get_mut(self.stable_id)
            .unwrap()
            .on_bridge = false;
    }
    fn floor_height(&self, xy: [i32; 2]) -> i32 {
        ground_surface_z_at(xy, false, self.terrain(), None).unwrap_or(0)
    }
    fn cell_high_bridge(&self, xy: [i32; 2]) -> bool {
        self.terrain().is_some_and(|terrain| {
            terrain.native_cell_flags(terrain.native_cell_identity(self.cell_of(xy))) & 0x100 != 0
        })
    }
    fn cell_top_height(&self, xy: [i32; 2]) -> i32 {
        let (level, slope) = self.cell_terrain(xy);
        let centre = ground_height_leptons(level, slope, 128, 128).unwrap_or(0);
        let (cx, cy) = self.cell_of(xy);
        if cx < 0 || cy < 0 {
            return jumpjet_flight::cell_top_height(centre, None, false);
        }
        let (rx, ry) = (cx as u16, cy as u16);
        // BuildingDimension2 464AF0 uses ART Height * native104; constructor
        // arithmetic is pinned by spatial_oracle/height_factor.json.
        let building_height = self
            .sim
            .substrate
            .occupancy
            .first_building_on_layer(rx, ry, MovementLayer::Ground)
            .map(|id| {
                self.sim
                    .substrate
                    .entities
                    .get(id)
                    .zip(self.rules)
                    .and_then(|(building, rules)| {
                        rules
                            .object(self.sim.interner.resolve(building.type_ref()))
                            .map(|object| rules.building_launch_height(object))
                    })
                    .unwrap_or(0)
                    .wrapping_mul(GROUND_LEVEL_HEIGHT_LEPTONS)
            });
        let any_techno = self
            .sim
            .substrate
            .occupancy
            .get(rx, ry)
            .is_some_and(|cell| cell.iter_layer(MovementLayer::Ground).next().is_some());
        jumpjet_flight::cell_top_height(centre, building_height, any_techno)
    }
    fn cell_land_type(&self, xy: [i32; 2]) -> u8 {
        self.terrain().map_or(0, |terrain| {
            match terrain.native_cell_identity(self.cell_of(xy)) {
                NativeCellIdentity::Real(index) => terrain.cells()[index].yr_cell_land_type,
                NativeCellIdentity::Dummy => 0,
            }
        })
    }
    fn balloon_hover(&self) -> bool {
        self.owner().locomotor.as_ref().unwrap().balloon_hover
    }
    fn has_target(&self) -> bool {
        self.owner().attack_target.is_some()
    }
    fn piggyback_active(&self) -> bool {
        self.owner().foot_locomotor_swap_active
    }
    fn piggyback_arrival(&mut self) {}
    fn simple_deployer(&self) -> bool {
        self.object_type()
            .is_some_and(|object| object.is_simple_deployer)
    }
    fn type_flag_6ad(&self) -> bool {
        self.object_type()
            .is_some_and(|object| object.deploy_to_land)
    }
    fn set_speed_fraction(&mut self, fraction_bits: u64) {
        self.sim
            .substrate
            .entities
            .get_mut(self.stable_id)
            .unwrap()
            .foot_speed
            .set_speed_fraction_native_bits(fraction_bits);
    }
    fn arrival_notify(&mut self) {}
    fn snap_body_facing(&mut self, facing: u16) {
        self.sim
            .substrate
            .entities
            .get_mut(self.stable_id)
            .unwrap()
            .body_facing
            .snap(facing, self.frame);
    }
    fn hold_target_facing(&self) -> Option<u16> {
        None
    }
    fn cell_of(&self, xy: [i32; 2]) -> (i16, i16) {
        (lepton_to_cell_packed(xy[0]), lepton_to_cell_packed(xy[1]))
    }
    fn body_facing(&self) -> u16 {
        self.owner().body_facing_current(self.frame)
    }
    fn random_direction(&mut self) -> u32 {
        self.sim.scenario_rng.next_range_i32_inclusive(0, 7) as u32
    }
    fn air_slot_taken_at(&mut self, cell: (i16, i16)) -> bool {
        self.slot_holder(cell)
            .is_some_and(|held| held != self.stable_id)
    }
    fn holds_air_slot_at(&self, cell: (i16, i16)) -> bool {
        self.slot_holder(cell) == Some(self.stable_id)
    }
    fn air_slot_empty_at(&self, cell: (i16, i16)) -> bool {
        self.slot_holder(cell).is_none()
    }
    fn tracker_cell(&self) -> (i16, i16) {
        self.owner().air_tracker_cell()
    }
    fn tracker_add(&mut self) {
        self.sim.aircraft_tracker_add(self.stable_id);
    }
    fn tracker_update(&mut self, cell: (i16, i16)) {
        self.sim.aircraft_tracker_update_cell(self.stable_id, cell);
    }
    fn claim_air_slot_at(&mut self, cell: (i16, i16)) {
        self.sim.set_cell_air_slot(cell, Some(self.stable_id));
    }
    fn release_air_slot_at(&mut self, cell: (i16, i16)) {
        self.sim.set_cell_air_slot(cell, None);
    }
    fn set_destination_cell(&mut self, cell: (i16, i16), runtime: &mut JumpjetRuntime) {
        let id = self.stable_id;
        let rules = self.rules;
        let speed = self.sim.jumpjet_order_speed(id, rules);
        runtime.with_owner_call(self.sim, id, |sim| {
            sim.jumpjet_cell_destination(id, (cell.0 as u16, cell.1 as u16), speed, rules);
        });
    }
    fn can_enter_cell(&self, cell: (i16, i16)) -> i32 {
        let answer = match self.rules {
            Some(rules) => self.sim.mover_can_enter(
                self.stable_id,
                cell,
                InfantryEntryArgs::REPAIR,
                crate::sim::movement::infantry_entry::EntryQueryMode::CheckLocomotor,
                rules,
                self.registry,
            ),
            None => Err("no rules".into()),
        };
        answer.map_or_else(
            |error| {
                // Existing no-rules/type/map fallback; native always has an answer.
                log::warn!("Jumpjet {} landing entry: {error}", self.stable_id);
                0
            },
            i32::from,
        )
    }
    fn sub_cell_free(&self, cell: (i16, i16), sub_cell: i32, bridge: bool) -> bool {
        if cell.0 < 0 || cell.1 < 0 || !(0..8).contains(&sub_cell) {
            return true;
        }
        let key = RawCellKey::Real(cell.0 as u16, cell.1 as u16);
        let layer = if bridge {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        self.sim.substrate.raw_cell_occupation.bits_at(key, layer) & (1u8 << sub_cell) == 0
    }
    fn mission_is_seven(&self) -> bool {
        crate::sim::movement::jumpjet_movement::owner_mission_is_enter(self.owner())
    }
    fn stop_moving(&mut self, runtime: &mut JumpjetRuntime) -> i32 {
        let id = self.stable_id;
        let rules = self.rules;
        let registry = self.registry;
        let speed = self.sim.jumpjet_order_speed(id, rules);
        runtime.with_owner_call(self.sim, id, |sim| {
            sim.jumpjet_stop_moving(id, rules, registry);
            sim.publish_jumpjet_destination(id, speed);
        });
        runtime.phase()
    }
    fn factory_contact_notify(&mut self, runtime: &mut JumpjetRuntime) {
        //54BC59 reads Contacts[0];54BC6D..54BCA1 wants a Building of one of
        // the four factory classes before the owner's Notify at54BCB3.
        let Some(rules) = self.rules else {
            return;
        };
        let factory_contact = self
            .owner()
            .radio_contacts
            .slot(0)
            .and_then(|contact| self.sim.substrate.entities.get(contact))
            .filter(|contact| contact.category == EntityCategory::Structure)
            .and_then(|contact| rules.object(self.sim.interner.resolve(contact.type_ref())))
            .is_some_and(|contact| {
                contact.weapons_factory
                    || contact.gdi_barracks()
                    || contact.nod_barracks()
                    || contact.yuri_barracks()
            });
        if !factory_contact {
            return;
        }
        let id = self.stable_id;
        let registry = self.registry;
        runtime.with_owner_call(self.sim, id, |sim| {
            sim.jumpjet_lift_off_notify(id, rules, registry);
        });
    }
    fn cell_high_bridge_at(&self, cell: (i16, i16)) -> bool {
        self.terrain().is_some_and(|terrain| {
            terrain.native_cell_flags(terrain.native_cell_identity(cell)) & 0x100 != 0
        })
    }
    fn begin_landing(&mut self, destination: [i32; 3]) {
        self.sim.object_raw_receiver_at(
            self.stable_id,
            DriveCoord {
                x: destination[0],
                y: destination[1],
                z: destination[2],
            },
            true,
        );
    }
    fn deploy_latched(&self) -> bool {
        self.owner().landing_for_deploy()
    }
    fn deploy_facing(&self) -> Option<u16> {
        self.rules
            .map(|rules| u16::from(rules.general.deploy_dir as u8) << 8)
    }
    fn touchdown(&mut self, runtime: &mut JumpjetRuntime) {
        self.touched_down = true;
        let id = self.stable_id;
        let rules = self.rules;
        let registry = self.registry;
        if self.owner_kind() == FlightOwnerKind::Unit {
            runtime.with_owner_call(self.sim, id, |sim| {
                // Unit739EC0 returns Ok(false); full Infantry bridge work remains
                // with the object-turn caller until its own bounded chain.
                sim.per_cell_process(
                    id,
                    crate::sim::movement::PerCellReason::Arrival,
                    rules,
                    registry,
                )
                .expect("Unit touchdown PerCell has no fallible arm");
                sim.assign_null_destination(id, rules, registry);
            });
        }
        let location = self.location();
        let here = self.cell_of([location[0], location[1]]);
        if self.holds_air_slot_at(here) {
            self.release_air_slot_at(here);
        }
        self.sim.aircraft_tracker_remove(id);
        // 0x0054C9F6: CellClass::PickupCrate on the Foot's cell (vt+0x1BC)
        // after the tracker removal (0x0054C9DC); its answer is not read.
        let rules = self.rules;
        let registry = self.registry;
        let _ = self.sim.pickup_crate_at(id, here, rules, registry);
    }
    fn finish_touchdown(&mut self) {
        let id = self.stable_id;
        let deploy_to_land = self.owner_kind() == FlightOwnerKind::Unit && self.type_flag_6ad();
        let owner = self.sim.substrate.entities.get_mut(id).unwrap();
        owner.crashing = false;
        if deploy_to_land {
            owner.set_landing_for_deploy(false);
        }
    }
    fn crashing(&self) -> bool {
        self.owner().crashing
    }
    fn in_bounds(&self, cell: (i16, i16)) -> bool {
        self.sim.map_size_diamond().is_some_and(|(width, height)| {
            crate::map::playfield::size_diamond_contains(width, height, cell)
        })
    }
    fn crash_relocate(&mut self, coord: [i32; 3]) {
        self.mark(false);
        self.set_location(coord);
        self.mark(true);
        self.sim
            .submit_entity_display(self.stable_id, self.rules, None);
    }
    fn mark(&mut self, put: bool) {
        if put {
            self.sim
                .foot_mark_put(self.stable_id, self.rules, self.registry);
        } else {
            self.sim
                .foot_mark_remove(self.stable_id, self.rules, self.registry);
        }
    }
    fn crash_impact(&mut self) {
        self.impact = true;
    }
    fn crash_latched(&mut self) {
        self.crash_latched = true;
    }
}

fn jumpjet_locomotor(entity: &crate::sim::game_entity::GameEntity) -> bool {
    entity.locomotor.as_ref().is_some_and(|locomotor| {
        locomotor.kind == LocomotorKind::Jumpjet && locomotor.jumpjet_runtime().is_some()
    })
}

impl Simulation {
    /// `INotifyProc::Notify(0x117B)` of a `BalloonHover=` or `JumpJet=` owner
    /// (Infantry `0x00522A60..0x00522AF4`, Unit `0x00746100..0x00746194`),
    /// which Jumpjet State 1 raises while the owner climbs in contact with its
    /// factory ([`JumpjetFlightHost::factory_contact_notify`]). In radio
    /// contact (`0x0065AE30`: any live slot) the owner sends literal 8 through
    /// `Contacts[0]`, so a barracks answers 25 and 3 and the tether ends. Then
    /// an ArchiveTarget that is not already the NavCom becomes the destination
    /// through the class setter (`vt+0x480(archive, 1)`); otherwise
    /// `0x004DF0D0` clears the NavCom pair and the owner scatters from a null
    /// coordinate with `(1, 0)`. Without a contact nothing runs.
    ///
    /// RESIDUAL: an object ArchiveTarget takes its cell here, not the object's
    /// `+0x4C` coordinate the Foot setter reads. Dormant: a factory's rally
    /// (`SetRally`) is always a cell.
    pub(crate) fn jumpjet_lift_off_notify(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        use crate::sim::combat::TargetKind;
        let Some(owner) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(object) = rules.object(self.interner.resolve(owner.type_ref())) else {
            return;
        };
        if !(object.balloon_hover || object.jumpjet) || owner.radio_contacts.is_empty() {
            return;
        }
        crate::sim::radio::transmit_to_contact(
            self,
            id,
            crate::sim::radio::RadioMessage::RequestClearance,
            Some(rules),
        );
        let Some(owner) = self.substrate.entities.get(id) else {
            return;
        };
        let archive = owner.archive_target();
        let nav_com = owner.navigation.nav_com.map(TargetKind::from);
        if let Some(archive) = archive.filter(|archive| Some(*archive) != nav_com) {
            let cell = match archive {
                TargetKind::Cell(rx, ry) => Some((rx, ry)),
                TargetKind::Entity(target) => self
                    .substrate
                    .entities
                    .get(target)
                    .map(|target| (target.position.rx, target.position.ry)),
            };
            let Some(cell) = cell else {
                return;
            };
            // Native does not read the setter's answer.
            let speed = self.jumpjet_order_speed(id, Some(rules));
            self.jumpjet_cell_destination(id, cell, speed, Some(rules));
            return;
        }
        if let Some(owner) = self.substrate.entities.get_mut(id) {
            crate::sim::movement::foot_stop_moving(owner);
        }
        if let Err(cause) = self.scatter_null(
            id,
            crate::sim::movement::ScatterFlags::new(true, false),
            rules,
            registry,
        ) {
            log::debug!("Jumpjet {id} lift-off scatter: {cause}");
        }
    }

    /// Process54AEC0 through the one private instance and live owner callbacks.
    pub(crate) fn tick_jumpjet_cruise_one(
        &mut self,
        stable_id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Option<AirMovementTickStats> {
        if !self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(jumpjet_locomotor)
        {
            return None;
        }
        self.apply_jumpjet_adapter_order(stable_id, rules);
        let frame = self.session.binary_frame;
        let (trig, _) = required_math_tables();
        let stats = self.with_jumpjet_process(stable_id, |runtime, sim| {
            let entry_state = runtime.phase();
            let mut host = CruiseHost {
                sim,
                frame,
                trig,
                rules,
                registry,
                stable_id,
                touched_down: false,
                impact: false,
                crash_latched: false,
            };
            let state = jumpjet_flight::process(runtime, &mut host);
            let ended_cruise =
                entry_state == STATE_TRANSLATE && matches!(state, STATE_HOLD | STATE_DESCEND);
            let height = host.height_above_ground();
            let entity = host.sim.substrate.entities.get_mut(stable_id).unwrap();
            // Adapter goal is only a remaining order projection; it cannot
            // overwrite the private class fields after a reentrant callback.
            if host.touched_down || ended_cruise {
                entity.movement_target = None;
            }
            entity.locomotor.as_mut().unwrap().altitude = SimFixed::from_num(height.max(0));
            if host.crash_latched
                && entity.category == EntityCategory::Infantry
                && let Some(rules) = rules
                && let Err(cause) = host.sim.infantry_do_action(
                    stable_id,
                    crate::sim::movement::infantry_action::DO_AIR_DEATH_START,
                    false,
                    rules,
                )
            {
                log::debug!("infantry {stable_id} AirDeathStart: {cause}");
            }
            AirMovementTickStats {
                arrivals: u32::from(host.touched_down || ended_cruise),
                impact: host.impact,
                touched_down: host.touched_down,
            }
        })?;
        Some(stats)
    }

    /// Remaining object-order adapter: a supplied goal reaches Move_To and
    /// retains its selected cell. Its producer migration remains in issue689.
    ///
    /// An absent projection is not a Stop command. Original Process54AEC0
    /// reads the class moving/phase fields; `state1_free_unmarked` frame102
    /// in jumpjet_states.json reaches only Process→State3 and retains its
    /// destination, NavCom and timers. Explicit Stop reaches the class setter
    /// through assign_null_destination instead.
    fn apply_jumpjet_adapter_order(&mut self, id: u64, rules: Option<&RuleSet>) {
        let goal = {
            let Some(entity) = self.substrate.entities.get(id) else {
                return;
            };
            let Some(runtime) = entity
                .locomotor
                .as_ref()
                .and_then(|locomotor| locomotor.jumpjet_runtime())
            else {
                return;
            };
            if entity.crashing {
                return;
            }
            match entity.movement_target.as_ref().and_then(|t| t.final_goal) {
                Some(goal) => {
                    let flying_to = (
                        lepton_to_cell_packed(runtime.destination().x),
                        lepton_to_cell_packed(runtime.destination().y),
                    );
                    if runtime.moving()
                        && runtime.destination() != JumpjetRuntime::NULL
                        && flying_to == (goal.0 as i16, goal.1 as i16)
                    {
                        return;
                    }
                    goal
                }
                None => return,
            }
        };
        let speed = self.jumpjet_order_speed(id, rules);
        let request = crate::sim::movement::target_cell_coord(
            goal.0,
            goal.1,
            self.resolved_terrain
                .as_ref()
                .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                .as_ref(),
        );
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.navigation.nav_com =
                Some(crate::sim::components::NavTargetRef::cell(goal.0, goal.1));
            entity.navigation.nav_com_aux = None;
        }
        self.jumpjet_move_to(id, request, rules);
        let moving = self.substrate.entities.get(id).is_some_and(|entity| {
            entity
                .locomotor
                .as_ref()
                .and_then(|locomotor| locomotor.jumpjet_runtime())
                .is_some_and(|runtime| runtime.moving())
        });
        if moving {
            self.publish_jumpjet_destination(id, speed);
        } else if let Some(entity) = self.substrate.entities.get_mut(id) {
            // A refused Move_To leaves the owner where it was; the goal
            // retires instead of being retried every frame.
            entity.movement_target = None;
        }
    }
}

#[cfg(test)]
#[path = "jumpjet_cruise_native_tests.rs"]
mod native_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::jumpjet_params::JumpjetParams;
    use crate::sim::components::MovementTarget;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::ground_pose::position_world_coord;
    use crate::sim::movement::jumpjet_movement::jumpjet_flight::STATE_ASCEND;
    use crate::sim::movement::locomotor::LocomotorState;
    use serde_json::Value;

    fn retail_tables_present() -> bool {
        let (trig, _) = required_math_tables();
        if !trig.matches_retail() {
            // With RA2_DIR set, a mismatched table is a failure, not a skip.
            assert!(
                std::env::var_os("RA2_DIR").is_none(),
                "RA2_DIR is set but the retail sine table does not match"
            );
            eprintln!("skipped: set RA2_DIR to the retail install to run this");
            return false;
        }
        true
    }

    fn native_row(name: &str) -> Value {
        let rows: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/jumpjet_flight.json",
        ))
        .expect("corpus parses");
        rows.as_array()
            .expect("rows")
            .iter()
            .find(|row| row["name"] == name)
            .cloned()
            .expect("named row")
    }

    /// A Unit Jumpjet with the corpus BASE type block, fresh from `link`
    /// (state 0, locomotor facing `0x4000`), hovering at 500 at cell (10,10)
    /// with a move to cell (16,10). The terrain is the oracle fixture: level 0
    /// and flat, except cell (9,10) at level 2 with slope 1.
    fn hovering_jumpjet(body_facing: u8) -> Simulation {
        let mut sim = Simulation::new();
        super::super::lifecycle_tests::install_common_raw_terrain(&mut sim, 24, 16, 0, None);
        // A playfield holding the corpus corridor (x 6..20, y 9..11), for the
        // orders' searches and `In_Bounds`.
        sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
            base: 12,
            off_fc: 0,
            off_100: 0,
            off_104: 16,
            off_108: 16,
        });
        sim.playfield_size_height = Some(16);
        {
            let cell = sim
                .resolved_terrain
                .as_mut()
                .and_then(|terrain| terrain.cell_mut(9, 10))
                .expect("fixture cell");
            cell.level = 2;
            cell.slope_type = 1;
        }
        let mut entity = GameEntity::test_default(1, "JUMPJETUNIT", "Americans", 10, 10);
        entity.category = EntityCategory::Unit;
        entity.body_facing.snap(u16::from(body_facing) << 8, 0);
        entity.position.sub_x = SimFixed::from_num(128);
        entity.position.sub_y = SimFixed::from_num(128);
        let mut locomotor = LocomotorState::for_test_kind(LocomotorKind::Jumpjet);
        locomotor
            .jumpjet_runtime_mut()
            .expect("jumpjet runtime")
            .link(&JumpjetParams {
                turn_rate: 4,
                speed: SimFixed::from_num(14),
                climb: 5.0,
                crash: 5.0,
                height: 500,
                accel: 2.0,
                wobbles: 0.15,
                deviation: 40,
                no_wobbles: false,
            });
        {
            let runtime = locomotor.jumpjet_runtime_mut().expect("jumpjet runtime");
            // The cruise rows begin mid-flight, which is what `Move_To` leaves
            // behind: State 3 with the moving byte set, and State 0's seed
            // already applied (locomotor facing snapped to the body, both speed
            // doubles and the bob zero, `+0x80` at `JumpjetHeight=`).
            let mut flight = runtime.flight();
            flight.facing.snap(u16::from(body_facing) << 8, 0);
            flight.target_height = 500;
            *runtime = runtime
                .clone()
                .with_phase_for_test(STATE_TRANSLATE)
                .with_moving_for_test(true)
                .with_flight_for_test(flight);
        }
        locomotor.altitude = SimFixed::from_num(500);

        entity.locomotor = Some(locomotor);
        entity.movement_target = Some(MovementTarget {
            speed: SimFixed::from_num(14),
            final_goal: Some((16, 10)),
            ..Default::default()
        });
        sim.substrate.entities.insert(entity);
        sim
    }

    /// Park a Jumpjet with no order in a given native state and run the tick.
    fn idle_for(state: i32, balloon_hover: bool, frames: u32) -> Simulation {
        let mut sim = hovering_jumpjet(0x40);
        {
            let entity = sim.substrate.entities.get_mut(1).expect("jumpjet");
            entity.movement_target = None;
            let locomotor = entity.locomotor.as_mut().expect("locomotor");
            locomotor.balloon_hover = balloon_hover;
            locomotor.altitude = SimFixed::from_num(if state == jumpjet_flight::STATE_GROUND {
                0
            } else {
                500
            });
            let runtime = locomotor.jumpjet_runtime_mut().expect("runtime");
            *runtime = runtime
                .clone()
                .with_phase_for_test(state)
                .with_moving_for_test(false)
                .with_destination_for_test(JumpjetRuntime::NULL);
        }
        for frame in 0..frames {
            sim.session.binary_frame = 1001 + frame;
            sim.tick_air_movement_with_cell_lists_one(1, None, None);
        }
        sim
    }

    /// A Jumpjet unit lifting off leaves its takeoff cell once it climbs past
    /// twice the level height: Update takes it off the map for its body and
    /// puts it back (`0x0054D12C` / `0x0054D6A6`), and that Mark(PUT) finds it
    /// in the Air layer (`0x0054B8D0`), so no ground list holds it and its
    /// 0x20 is gone (`0x00744210`).
    #[test]
    fn a_lifting_jumpjet_unit_leaves_its_takeoff_cell() {
        let mut sim = hovering_jumpjet(0x40);
        {
            let entity = sim.substrate.entities.get_mut(1).expect("jumpjet");
            entity.foot_occupation_enabled = true;
            entity.position.exact_z_leptons = Some(0);
            let locomotor = entity.locomotor.as_mut().expect("locomotor");
            locomotor.altitude = SimFixed::from_num(0);
            let runtime = locomotor.jumpjet_runtime_mut().expect("runtime");
            *runtime = runtime
                .clone()
                .with_phase_for_test(jumpjet_flight::STATE_GROUND)
                .with_moving_for_test(false)
                .with_destination_for_test(JumpjetRuntime::NULL);
        }
        sim.add_entity_occupancy(1);
        assert!(sim.substrate.occupancy.contains_entity(10, 10, 1));
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(10, 10), 0x20);
        for frame in 0..80 {
            sim.session.binary_frame = 1001 + frame;
            sim.tick_air_movement_with_cell_lists_one(1, None, None);
        }
        let entity = sim.substrate.entities.get(1).expect("jumpjet");
        assert!(entity.position.exact_z_leptons.unwrap() >= 208);
        let here = (entity.position.rx, entity.position.ry);
        for (rx, ry) in [(10, 10), here] {
            assert!(!sim.substrate.occupancy.contains_entity(rx, ry, 1));
            assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(rx, ry), 0);
        }
    }

    /// Before the locomotor owned this tick, VERA's air adapter cycled an idle
    /// non-balloon Jumpjet Ascending -> Hovering -> Descending -> Landed every
    /// 102 frames. `Process 0x0054AEC0` runs Update only while the moving byte
    /// is set or the state is not ground/hold, and State 0 leaves the ground
    /// only when moving, so an idle landed owner does nothing at all.
    #[test]
    fn an_idle_landed_jumpjet_never_leaves_the_ground() {
        let sim = idle_for(jumpjet_flight::STATE_GROUND, false, 400);
        let entity = sim.substrate.entities.get(1).expect("jumpjet");
        let locomotor = entity.locomotor.as_ref().expect("locomotor");
        assert_eq!(locomotor.altitude, SimFixed::from_num(0));
        assert_eq!(
            locomotor.jumpjet_runtime().map(|runtime| runtime.phase()),
            Some(jumpjet_flight::STATE_GROUND)
        );
        assert_eq!((entity.position.rx, entity.position.ry), (10, 10));
    }

    /// Re-opening a cruise must not leave a claim behind on the cell the owner
    /// drifted out of.
    ///
    /// Native State 2 releases the owner's cached cell (`+0x560`) at
    /// `0x0054BF75..0x0054BFA8` before releasing the current one. The drift is
    /// real: State 1 sets the target speed before promoting to the hold, so the
    /// owner leaves the cell it claimed. The retained tracker Cell is
    /// independent of the slot notification's +564 cache.
    ///
    /// The corpus cannot witness this: its declared substitution pins `+0x560`,
    /// so the original skips the cached-cell release there. Hence a regression
    /// test rather than a parity row.
    #[test]
    fn re_opening_a_cruise_drops_a_drifted_slot() {
        let mut sim = hovering_jumpjet(0x40);
        // The owner holds (13,10) but sits at (10,10) — the drift case.
        assert!(sim.set_cell_air_slot((13, 10), Some(1)));
        let actor = sim.substrate.entities.get_mut(1).unwrap();
        *actor = actor.clone().with_air_tracker_cell_for_test((13, 10));
        {
            let locomotor = sim
                .substrate
                .entities
                .get_mut(1)
                .and_then(|entity| entity.locomotor.as_mut())
                .expect("locomotor");
            let runtime = locomotor.jumpjet_runtime_mut().expect("runtime");
            *runtime = runtime
                .clone()
                .with_phase_for_test(STATE_HOLD)
                .with_moving_for_test(true);
        }
        sim.session.binary_frame = 1001;
        sim.tick_air_movement_with_cell_lists_one(1, None, None);

        assert_eq!(
            sim.substrate.air_slots.holder(13, 10),
            None,
            "the claim on the cell the owner drifted out of must not leak"
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .and_then(|entity| entity.locomotor.as_ref())
                .and_then(|locomotor| locomotor.jumpjet_runtime())
                .map(|runtime| runtime.phase()),
            Some(STATE_TRANSLATE),
            "a destination elsewhere re-opens the cruise"
        );
    }

    /// `Is_Moving_Now 0x0054D0D0` answers false for the hold too, so a parked
    /// balloon with no order neither bobs nor drifts.
    #[test]
    fn an_idle_hold_without_a_move_is_inert() {
        let sim = idle_for(STATE_HOLD, true, 400);
        let entity = sim.substrate.entities.get(1).expect("jumpjet");
        let locomotor = entity.locomotor.as_ref().expect("locomotor");
        assert_eq!(
            locomotor.jumpjet_runtime().map(|runtime| runtime.phase()),
            Some(STATE_HOLD)
        );
        assert_eq!((entity.position.rx, entity.position.ry), (10, 10));
    }

    /// Fly a native row through the production air tick, comparing the owner
    /// position and exact Z every frame, until arrival hands it to descent.
    fn fly_native_row(name: &str, body_facing: u8) {
        let row = native_row(name);
        let frames = row["output"]["frames"].as_array().expect("frames");
        let mut sim = hovering_jumpjet(body_facing);
        for (index, expected) in frames.iter().enumerate() {
            sim.session.binary_frame = 1001 + index as u32;
            let stats = sim.tick_air_movement_with_cell_lists_one(1, None, None);
            let entity = sim.substrate.entities.get(1).expect("jumpjet");
            let coord = position_world_coord(&entity.position);
            let native: Vec<i64> = expected["coord"]
                .as_array()
                .expect("coord")
                .iter()
                .map(|value| value.as_i64().expect("int"))
                .collect();
            assert_eq!(
                vec![i64::from(coord.x), i64::from(coord.y), i64::from(coord.z)],
                native,
                "{name}: frame {index}"
            );
            assert_eq!(
                u32::from(entity.body_facing_byte(sim.session.binary_frame)),
                (expected["body_facing"].as_u64().expect("body facing") >> 8) as u32,
                "{name}: body facing, frame {index}"
            );
            // `SetSpeedFraction` reaches Foot `+0x578`: the frame's last native
            // fraction, clamped to [0, 1] and truncated to `SimFixed`.
            if let Some(bits) = expected["speed_fractions"]
                .as_array()
                .and_then(|fractions| fractions.last())
            {
                let value = f64::from_bits(bits.as_u64().expect("fraction bits"));
                let truncated = if value.is_nan() || value <= 0.0 {
                    0
                } else if value >= 1.0 {
                    1 << 16
                } else {
                    (value * 65536.0).floor() as i32
                };
                assert_eq!(
                    entity.foot_speed.applied_fraction().to_bits(),
                    truncated,
                    "{name}: speed fraction, frame {index}"
                );
            }
            let last = index + 1 == frames.len();
            assert_eq!(stats.arrivals, u32::from(last), "{name}: frame {index}");
        }
        let entity = sim.substrate.entities.get(1).expect("jumpjet");
        assert!(entity.movement_target.is_none());
        let locomotor = entity.locomotor.as_ref().expect("locomotor");
        assert_eq!(
            locomotor.jumpjet_runtime().map(|runtime| runtime.phase()),
            Some(STATE_DESCEND)
        );
    }

    /// Production parity for the native `east_cruise` row, owner already facing
    /// east.
    #[test]
    fn production_cruise_flies_the_native_east_cruise_frames() {
        if retail_tables_present() {
            fly_native_row("east_cruise", 0x40);
        }
    }

    /// Production parity for `east_from_west_facing`: the takeoff seed snaps the
    /// locomotor facing to the body facing (west), so the cruise turns around
    /// from there instead of starting from the linked `0x4000`.
    #[test]
    fn production_cruise_turns_from_the_body_facing() {
        if retail_tables_present() {
            fly_native_row("east_from_west_facing", 0xC0);
        }
    }

    /// An explicit class NULL destination mid-cruise runs Foot's
    /// `Stop_Moving`, which re-targets the
    /// cell under the owner and keeps the moving byte, so it flies on to that
    /// cell's centre and lands there, and the NavCom its `Move_To` wrote is
    /// cleared again. It is applied once: the next frame finds no NavCom.
    #[test]
    fn a_null_destination_stops_at_the_cell_under_the_owner() {
        let mut sim = hovering_jumpjet(0x40);
        for frame in 0..20 {
            sim.session.binary_frame = 1001 + frame;
            sim.tick_air_movement_with_cell_lists_one(1, None, None);
        }
        let runtime = |sim: &Simulation| {
            sim.substrate
                .entities
                .get(1)
                .and_then(|entity| entity.locomotor.as_ref())
                .and_then(|locomotor| locomotor.jumpjet_runtime())
                .cloned()
                .expect("runtime")
        };
        let flying = runtime(&sim);
        assert_eq!(flying.phase(), STATE_TRANSLATE);
        assert_eq!(
            flying.destination(),
            crate::sim::components::DriveCoord {
                x: 16 * 256 + 128,
                y: 10 * 256 + 128,
                z: 0
            },
            "the order's Move_To placed the owner at the ordered cell's floor"
        );
        let entity = sim.substrate.entities.get_mut(1).expect("jumpjet");
        assert!(entity.navigation.nav_com.is_some());
        let here = (entity.position.rx, entity.position.ry);
        assert!(here.0 < 16, "still on its way");
        sim.assign_null_destination(1, None, None);

        sim.session.binary_frame = 1021;
        sim.tick_air_movement_with_cell_lists_one(1, None, None);
        let stopped = runtime(&sim);
        assert!(stopped.moving(), "Stop_Moving keeps the moving byte");
        assert_eq!(
            stopped.destination(),
            crate::sim::components::DriveCoord {
                x: i32::from(here.0) * 256 + 128,
                y: i32::from(here.1) * 256 + 128,
                z: 0
            }
        );
        let entity = sim.substrate.entities.get(1).expect("jumpjet");
        assert!(entity.navigation.nav_com.is_none());
        assert!(entity.movement_target.is_none());

        for frame in 0..400 {
            sim.session.binary_frame = 1022 + frame;
            sim.tick_air_movement_with_cell_lists_one(1, None, None);
        }
        let landed = runtime(&sim);
        assert_eq!(landed.phase(), jumpjet_flight::STATE_GROUND);
        assert!(!landed.moving());
        let entity = sim.substrate.entities.get(1).expect("jumpjet");
        assert_eq!((entity.position.rx, entity.position.ry), here);
        assert_eq!(entity.position.exact_z_leptons, Some(0));
    }

    /// State 4 admits a landing through the owner's own `Can_Enter_Cell`
    /// (`vt+0x1AC` at `0x0054C66D`) on the destination's cell, with no
    /// direction, no height and no source cell. A Unit landing on the cell
    /// centre passes the sub-cell gate (`0x0054C6BE`). Then 0 and 1 admit and
    /// set the landing latch; 2 refuses until the latch is set, and anything
    /// above 2 refuses, latched or not (`0x0054C6FD..0x0054C731`). A refusal
    /// runs `Stop_Moving`, which withdraws the latch and lifts the descent
    /// back into State 1.
    #[test]
    fn a_descent_lands_only_where_the_owners_can_enter_cell_admits() {
        use crate::sim::movement::fresh_oracle_seam::{self, FreshCallRecord};
        let rules =
            RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str("")).expect("rules");
        for (code, latched, admitted) in [
            (0, false, true),
            (1, false, true),
            (2, false, false),
            (2, true, true),
            (3, false, false),
            (3, true, false),
            (7, false, false),
        ] {
            let mut sim = hovering_jumpjet(0x40);
            // The owner's type is read through the world's interner once
            // rules are present.
            sim.interner = crate::sim::intern::test_interner();
            {
                let entity = sim.substrate.entities.get_mut(1).expect("jumpjet");
                entity.movement_target = None;
                let runtime = entity
                    .locomotor
                    .as_mut()
                    .and_then(|locomotor| locomotor.jumpjet_runtime_mut())
                    .expect("runtime");
                *runtime = runtime
                    .clone()
                    .with_phase_for_test(STATE_DESCEND)
                    .with_moving_for_test(true)
                    .with_landing_latched_for_test(latched)
                    .with_destination_for_test(crate::sim::components::DriveCoord {
                        x: 10 * 256 + 128,
                        y: 10 * 256 + 128,
                        z: 0,
                    });
            }
            sim.session.binary_frame = 1001;
            fresh_oracle_seam::install(vec![code], Vec::new());
            sim.tick_air_movement_with_cell_lists_one(1, Some(&rules), None);
            let (records, unused) = fresh_oracle_seam::finish();
            assert_eq!(
                records,
                [FreshCallRecord::CanEnter {
                    cell: (10, 10),
                    direction: -1,
                    height: -1,
                    code,
                }],
                "code {code}, latched {latched}"
            );
            assert_eq!(unused, 0, "code {code}, latched {latched}");
            let runtime = sim
                .substrate
                .entities
                .get(1)
                .and_then(|entity| entity.locomotor.as_ref())
                .and_then(|locomotor| locomotor.jumpjet_runtime())
                .expect("runtime");
            assert_eq!(runtime.landing_latched(), admitted, "code {code}");
            let phase = if admitted {
                STATE_DESCEND
            } else {
                STATE_ASCEND
            };
            assert_eq!(runtime.phase(), phase, "code {code}");
        }
    }

    /// On retail Dustbowl, a tank is placed on a Night Hawk's ordered cell
    /// while the Night Hawk descends onto it. The Night Hawk's own
    /// `Can_Enter_Cell` refuses the landing (`0x0054C66D`), so it never lands
    /// on the tank. It does not land beside it either:
    /// - Every Update outside the hold and the cruise takes the owner off the
    ///   map (`0x0054D0FF..0x0054D12C`). `ObjectClass::Mark` clears `+0x74`
    ///   (`0x005F5913`) before the layer query, which then answers Ground
    ///   (`0x0054B8D0`).
    /// - So `MapClass::Pick_Up`'s `RemoveContent` runs the Unit receiver
    ///   (`0x0047EB62..0x0047EB89`, `0x00744210`) on the cell below. The Night
    ///   Hawk is not listed there, but the receiver clears the tank's `0x20`.
    /// - `Stop_Moving`'s search (`0x0054B5DF`) reads only that plane
    ///   (`0x004834A0`), so it takes the tank's own cell.
    ///
    /// The Night Hawk hovers over the tank until it drives off, then lands
    /// there. Established by instruction reading. Before #931 ported Update's
    /// Mark bracket, the bit survived and the Night Hawk landed beside the tank.
    #[test]
    #[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
    fn retail_dustbowl_night_hawk_hovers_over_a_tank_on_its_cell() {
        use super::super::jumpjet_infantry_tests::{retail_dustbowl_rocketeer, retail_frame};
        use crate::headless_scenario::HeadlessScenario;
        use crate::sim::command::{Command, CommandEnvelope};

        // Open level ground from (x - 4, y - 1) to (x + 16, y + 1); the
        // helper's Rocketeer parks at (x, y).
        let (mut scenario, _, x, y) = retail_dustbowl_rocketeer();
        let spawn = |scenario: &mut HeadlessScenario, name: &str, cell: (u16, u16)| {
            let crate::sim::runtime::SimRuntime {
                simulation: sim,
                resources,
            } = &mut scenario.runtime;
            let id = sim
                .spawn_object_with_overlay_registry(
                    name,
                    "Americans",
                    cell.0,
                    cell.1,
                    64,
                    &resources.rules,
                    &resources.overlay_registry,
                )
                .expect("spawns on open ground");
            sim.resolve_type_handles(&resources.rules);
            id
        };
        let cell_of = |scenario: &HeadlessScenario, id: u64| {
            let entity = scenario
                .runtime
                .simulation
                .substrate
                .entities
                .get(id)
                .expect("live");
            (entity.position.rx, entity.position.ry)
        };
        let phase = |scenario: &HeadlessScenario, id: u64| {
            scenario
                .runtime
                .simulation
                .substrate
                .entities
                .get(id)
                .and_then(|entity| entity.locomotor.as_ref())
                .and_then(|locomotor| locomotor.jumpjet_runtime())
                .expect("Jumpjet")
                .phase()
        };

        let hawk = spawn(&mut scenario, "SHAD", (x + 2, y));
        let landing = (x + 6, y);
        let sim = &scenario.runtime.simulation;
        let mut orders = vec![CommandEnvelope::new(
            sim.interner.get("Americans").expect("house"),
            sim.session.tick + 1,
            Command::Move {
                entity_id: hawk,
                target_rx: landing.0,
                target_ry: landing.1,
                queue: false,
            },
        )];
        let mut frames = 0;
        while phase(&scenario, hawk) != STATE_DESCEND {
            retail_frame(&mut scenario, std::mem::take(&mut orders));
            frames += 1;
            assert!(frames < 200, "the Night Hawk reaches its descent");
        }
        assert_eq!(cell_of(&scenario, hawk), landing);
        let tank = spawn(&mut scenario, "MTNK", landing);
        for _ in 0..200 {
            retail_frame(&mut scenario, Vec::new());
            assert_ne!(phase(&scenario, hawk), jumpjet_flight::STATE_GROUND);
        }
        assert_eq!(cell_of(&scenario, hawk), landing);
        assert_eq!(cell_of(&scenario, tank), landing);
        let sim = &scenario.runtime.simulation;
        assert!(
            sim.substrate
                .occupancy
                .contains_entity(landing.0, landing.1, tank)
        );
        assert_eq!(
            sim.substrate
                .raw_cell_occupation
                .ground_bits(landing.0, landing.1)
                & 0x20,
            0,
            "the Night Hawk's Mark cleared the tank's bit"
        );

        let mut orders = vec![CommandEnvelope::new(
            sim.interner.get("Americans").expect("house"),
            sim.session.tick + 1,
            Command::Move {
                entity_id: tank,
                target_rx: x + 12,
                target_ry: y,
                queue: false,
            },
        )];
        let mut frames = 0;
        while phase(&scenario, hawk) != jumpjet_flight::STATE_GROUND {
            retail_frame(&mut scenario, std::mem::take(&mut orders));
            frames += 1;
            assert!(frames < 400, "the Night Hawk lands once the tank leaves");
        }
        assert_eq!(cell_of(&scenario, hawk), landing);
        assert_ne!(cell_of(&scenario, tank), landing);
    }

    /// A human player's Kirov on retail Dustbowl, ordered to attack a
    /// computer's Tesla Reactor ten cells away, flies to the reactor's centre
    /// and bombs it on the way. Its bomb (BlimpBombP, `Vertical=`) makes
    /// Approach_Target's BalloonHover arm (`0x00741599`) take the reactor as
    /// the destination; Jumpjet Move_To (`0x0054B43D..0x0054B44B`) then sets
    /// the NavCom to the cell under its centre. Before the arm the Kirov
    /// stopped some 550 leptons short, at weapon range. A production witness,
    /// not a native comparison.
    #[test]
    #[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
    fn retail_dustbowl_kirov_flies_over_a_reactor_and_bombs_it() {
        use super::super::jumpjet_infantry_tests::{retail_dustbowl_rocketeer, retail_frame};
        use crate::sim::command::{Command, CommandEnvelope};

        let (mut scenario, _, x, y) = retail_dustbowl_rocketeer();
        let (kirov, reactor) = {
            let crate::sim::runtime::SimRuntime {
                simulation: sim,
                resources,
            } = &mut scenario.runtime;
            let kirov = sim
                .spawn_object_with_overlay_registry(
                    "ZEP",
                    "Americans",
                    x + 2,
                    y,
                    64,
                    &resources.rules,
                    &resources.overlay_registry,
                )
                .expect("Kirov spawns");
            let reactor = sim
                .spawn_object_with_overlay_registry(
                    "NAPOWR",
                    "Russians",
                    x + 12,
                    y - 1,
                    0,
                    &resources.rules,
                    &resources.overlay_registry,
                )
                .expect("reactor spawns");
            sim.resolve_type_handles(&resources.rules);
            (kirov, reactor)
        };
        let sim = &scenario.runtime.simulation;
        let center = |sim: &crate::sim::world::Simulation, id: u64| {
            let entity = sim.substrate.entities.get(id).unwrap();
            crate::sim::movement::ground_pose::object_get_coords(
                entity,
                sim.resolved_terrain.as_ref(),
            )
        };
        let goal = center(sim, reactor);
        let goal_cell = crate::sim::components::NavTargetRef::cell(
            (goal.x / 256) as u16,
            (goal.y / 256) as u16,
        );
        let mut orders = vec![CommandEnvelope::new(
            sim.interner.get("Americans").expect("house"),
            sim.session.tick + 1,
            Command::Attack {
                attacker_id: kirov,
                target_id: reactor,
            },
        )];
        let mut closest = i32::MAX;
        let mut destroyed_at = None;
        let mut bombs = std::collections::BTreeMap::new();
        for frame in 0..900u32 {
            retail_frame(&mut scenario, std::mem::take(&mut orders));
            let sim = &scenario.runtime.simulation;
            let entity = sim.substrate.entities.get(kirov).expect("Kirov lives");
            if frame == 1 {
                assert_eq!(entity.navigation.nav_com, Some(goal_cell));
            }
            let at = center(sim, kirov);
            let off = ((at.x - goal.x) as f64).hypot((at.y - goal.y) as f64) as i32;
            if destroyed_at.is_none() {
                closest = closest.min(off);
            }
            if destroyed_at.is_none() && !sim.substrate.entities.contains(reactor) {
                destroyed_at = Some((frame, off));
            }
            for (&id, bomb) in sim.projectiles.iter() {
                if bomb.source_id == kirov {
                    let here = [bomb.position.x, bomb.position.y];
                    bombs.entry(id).or_insert_with(Vec::new).push(here);
                }
            }
        }
        let (frame, off) = destroyed_at.expect("the reactor dies");
        assert!(off < 256, "frame {frame}: {off} leptons from the centre");
        assert!(closest < 256, "{closest}");
        assert!(!bombs.is_empty());
        // Each bomb falls straight down from where it was dropped.
        for track in bombs.values() {
            assert!(track.iter().all(|at| *at == track[0]), "{track:?}");
        }
    }

    /// A computer's Kirov on Hunt on retail Dustbowl flies at what its scan
    /// picks: Mission_Hunt approaches what the scan holds (`0x004D54DD`), and
    /// the BalloonHover arm makes the target the destination, so the Kirov
    /// closes past its bomb's range instead of stopping there. It stays on
    /// Hunt; the pursuit pass's in-range stop, which this replaces for a
    /// balloon, would have switched it to Attack through the NULL
    /// destination's arm. A production witness, not a native comparison.
    #[test]
    #[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
    fn retail_dustbowl_hunting_kirov_flies_at_its_target_and_stays_on_hunt() {
        use super::super::jumpjet_infantry_tests::{retail_dustbowl_rocketeer, retail_frame};
        use crate::sim::mission::{MissionId, MissionType};

        let (mut scenario, _, x, y) = retail_dustbowl_rocketeer();
        let kirov = {
            let crate::sim::runtime::SimRuntime {
                simulation: sim,
                resources,
            } = &mut scenario.runtime;
            let kirov = sim
                .spawn_object_with_overlay_registry(
                    "ZEP",
                    "Russians",
                    x + 12,
                    y,
                    64,
                    &resources.rules,
                    &resources.overlay_registry,
                )
                .expect("Kirov spawns");
            sim.resolve_type_handles(&resources.rules);
            let now = sim.session.binary_frame;
            sim.mission_assign_exact(kirov, MissionId::from_known(MissionType::Hunt), now)
                .unwrap();
            kirov
        };
        let center = |sim: &crate::sim::world::Simulation, id: u64| {
            let entity = sim.substrate.entities.get(id).unwrap();
            crate::sim::movement::ground_pose::object_get_coords(
                entity,
                sim.resolved_terrain.as_ref(),
            )
        };
        // Each target whose cell the Kirov takes as its NavCom, and how close
        // it comes to that target while holding it.
        let mut approached = std::collections::BTreeMap::new();
        for frame in 0..900u32 {
            retail_frame(&mut scenario, Vec::new());
            let sim = &scenario.runtime.simulation;
            let entity = sim.substrate.entities.get(kirov).expect("Kirov lives");
            assert_eq!(
                entity.mission.current(),
                MissionId::from_known(MissionType::Hunt),
                "frame {frame}"
            );
            let Some(crate::sim::combat::TargetKind::Entity(target)) =
                entity.attack_target.as_ref().map(|attack| attack.target)
            else {
                continue;
            };
            let goal = center(sim, target);
            let goal_cell = crate::sim::components::NavTargetRef::cell(
                (goal.x / 256) as u16,
                (goal.y / 256) as u16,
            );
            if entity.navigation.nav_com == Some(goal_cell) {
                approached.entry(target).or_insert(i32::MAX);
            }
            if let Some(closest) = approached.get_mut(&target) {
                let at = center(sim, kirov);
                let off = ((at.x - goal.x) as f64).hypot((at.y - goal.y) as f64) as i32;
                *closest = (*closest).min(off);
            }
        }
        // BlimpBomb's range is 1.5 cells (384 leptons); the pursuit pass
        // stopped a balloon there.
        assert!(
            approached.values().any(|&closest| closest < 256),
            "{approached:?}"
        );
    }

    /// A player's Floating Disc ordered onto a Rhino tank on retail Dustbowl
    /// picks its laser (`DiskLaser`, `Range=7`, not `Vertical=`), so the
    /// BalloonHover arm hands Approach on to Foot's (`0x004D5690`), which
    /// stops it within the laser's reach instead of over the tank. It stays
    /// on Attack and burns the tank from there. A production witness, not a
    /// native comparison.
    #[test]
    #[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
    fn retail_dustbowl_floating_disc_lasers_a_tank_from_range() {
        use super::super::jumpjet_infantry_tests::{retail_dustbowl_rocketeer, retail_frame};
        use crate::sim::command::{Command, CommandEnvelope};
        use crate::sim::mission::{MissionId, MissionType};

        let (mut scenario, _, x, y) = retail_dustbowl_rocketeer();
        let (disc, tank) = {
            let crate::sim::runtime::SimRuntime {
                simulation: sim,
                resources,
            } = &mut scenario.runtime;
            let disc = sim
                .spawn_object_with_overlay_registry(
                    "DISK",
                    "Americans",
                    x + 2,
                    y,
                    64,
                    &resources.rules,
                    &resources.overlay_registry,
                )
                .expect("Floating Disc spawns");
            let tank = sim
                .spawn_object_with_overlay_registry(
                    "HTNK",
                    "Russians",
                    x + 12,
                    y,
                    0,
                    &resources.rules,
                    &resources.overlay_registry,
                )
                .expect("tank spawns");
            sim.resolve_type_handles(&resources.rules);
            (disc, tank)
        };
        let center = |sim: &crate::sim::world::Simulation, id: u64| {
            let entity = sim.substrate.entities.get(id).unwrap();
            crate::sim::movement::ground_pose::object_get_coords(
                entity,
                sim.resolved_terrain.as_ref(),
            )
        };
        let sim = &scenario.runtime.simulation;
        let strength = sim.substrate.entities.get(tank).unwrap().health.current;
        let mut orders = vec![CommandEnvelope::new(
            sim.interner.get("Americans").expect("house"),
            sim.session.tick + 1,
            Command::Attack {
                attacker_id: disc,
                target_id: tank,
            },
        )];
        let mut closest = i32::MAX;
        let mut damaged = false;
        for frame in 0..600u32 {
            retail_frame(&mut scenario, std::mem::take(&mut orders));
            let sim = &scenario.runtime.simulation;
            let Some(target) = sim.substrate.entities.get(tank) else {
                damaged = true;
                break;
            };
            damaged |= target.health.current < strength;
            let entity = sim.substrate.entities.get(disc).expect("Disc lives");
            if frame > 0 {
                assert_eq!(
                    entity.mission.current(),
                    MissionId::from_known(MissionType::Attack),
                    "frame {frame}"
                );
            }
            let (at, goal) = (center(sim, disc), center(sim, tank));
            closest = closest.min(((at.x - goal.x) as f64).hypot((at.y - goal.y) as f64) as i32);
        }
        assert!(damaged, "the laser reaches the tank");
        // The laser reaches 7 cells (1792 leptons); over the tank would be
        // under a cell.
        assert!((1024..=1792).contains(&closest), "{closest}");
    }

    /// `g_HeightFactor`, read from the native startup chain, is the multiplier
    /// the cell top height applies to a building's art `Height=`.
    #[test]
    fn building_height_factor_matches_the_native_startup_value() {
        let native: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/height_factor.json",
        ))
        .expect("height factor parses");
        assert_eq!(
            native["height_factor"].as_i64(),
            Some(i64::from(GROUND_LEVEL_HEIGHT_LEPTONS))
        );
    }
}
