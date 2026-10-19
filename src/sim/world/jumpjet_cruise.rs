//! Mutable production host for Jumpjet Process54AEC0.
//!
//! Owner callbacks run at their original call sites. In particular State1's
//! scatter executes Unit741970 -> Foot4D94B0 -> Jumpjet54B1C0 before its Mark
//! suffix and phase write; State3's tail reads that callback's new destination.
//! Flight state belongs to JumpjetRuntime, retained tracker/slot Cells belong
//! to Foot, and XYZ/Mark/raw occupation use their existing shared owners.
//!
//! Native composed controls and coverage: tools/spatial_oracle/jumpjet_states.md.
//! Other large mechanisms remain bounded: target-facing hold, radio/tag
//! arrival side effects beyond the existing per-cell owner. The
//! Magnetron's callbacks run through `sim::locomotor_lift`.
//! Native Unit touchdown calls Cell PickupCrate481A00 at54C9F6 after phase0;
//! the touchdown asks `Simulation::pickup_crate_at` after the tracker removal
//! (54C9DC) as native does. The no-crate native controls do not establish
//! that separate mechanism.

use super::Simulation;
use crate::map::cell_index::NativeCellIdentity;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
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
use crate::sim::world::FrameEffects;
use crate::util::lepton::{ground_height_leptons, lepton_to_cell_packed};
use crate::util::retail_trig::TrigTable;

struct CruiseHost<'a> {
    sim: &'a mut Simulation,
    frame: u32,
    trig: &'a TrigTable,
    rules: Option<&'a RuleSet>,
    registry: Option<&'a OverlayTypeRegistry>,
    frame_effects: FrameEffects<'a>,
    stable_id: u64,
    touched_down: bool,
    impact: bool,
    crash_latched: bool,
    bridge_state_changed: bool,
    per_cell_error: Option<super::FrameAdvanceError>,
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
        self.rules.and_then(|rules| {
            self.sim
                .interner
                .object_type(self.owner().type_ref(), rules)
        })
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
        FlightOwnerKind::of(self.owner().category())
    }
    fn is_object_alive(&self) -> bool {
        self.owner().is_object_alive()
    }
    /// `Object+0x74`, Marked.
    fn marked(&self) -> bool {
        self.owner().lifecycle.cell_marked()
    }
    fn replace_marked_byte(&mut self, marked: bool) -> bool {
        self.sim.replace_foot_marked_byte(self.stable_id, marked)
    }
    /// `Object+0x9C`, Location.
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
        self.sim.set_object_z(
            self.stable_id,
            z,
            self.rules,
            self.registry,
            self.frame_effects,
        );
    }
    fn height_above_ground(&self) -> i32 {
        crate::sim::movement::air_movement::current_fly_height(self.owner(), self.terrain())
    }
    fn on_bridge(&self) -> bool {
        self.owner().on_bridge()
    }
    /// Update's grounded reset: the raw receiver at the Location, then
    /// OnBridge cleared (`0x0054D438`).
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
            .on_bridge
            .store(false);
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
                        self.sim
                            .interner
                            .object_type(building.type_ref(), rules)
                            .map(|object| rules.building_dimensions(object)[2])
                    })
                    .unwrap_or(0)
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
                NativeCellIdentity::Real(index) => terrain.cells()[index].land_type(),
                NativeCellIdentity::Dummy => 0,
            }
        })
    }
    /// `TechnoTypeClass+0xD6A`, `BalloonHover=`, as the locomotor caches it.
    fn balloon_hover(&self) -> bool {
        self.owner().locomotor.as_ref().unwrap().balloon_hover()
    }
    /// `Techno+0x2B4`, TarCom, is set.
    fn has_target(&self) -> bool {
        self.owner().attack_target.target().is_some()
    }
    /// `Foot+0x6AD`: a Magnetron lifted the owner.
    fn piggyback_active(&self) -> bool {
        self.owner().lift.swap_active()
    }
    fn piggyback_arrival(&mut self, runtime: &mut JumpjetRuntime) {
        let Some(holder) = self.owner().lift.source() else {
            return;
        };
        let id = self.stable_id;
        let rules = self.rules;
        let registry = self.registry;
        let frame_effects = self.frame_effects;
        runtime.with_owner_call(self.sim, id, |sim| {
            sim.release_locomotor(holder, true, rules, registry, frame_effects);
        });
    }
    fn simple_deployer(&self) -> bool {
        self.object_type()
            .is_some_and(|object| object.is_simple_deployer)
    }
    fn type_flag_6ad(&self) -> bool {
        self.object_type()
            .is_some_and(|object| object.deploy_to_land)
    }
    /// `FootClass::SetSpeedFraction @ 0x004D3710` (vt+0x544).
    fn set_speed_fraction(&mut self, fraction_bits: u64) {
        self.sim
            .substrate
            .entities
            .get_mut(self.stable_id)
            .unwrap()
            .foot_speed
            .set_speed_fraction_native_bits(fraction_bits);
    }
    fn arrival_notify(&mut self) {
        // Jumpjet arrival705D60 ->6385C0, at the state transition sites
        //54BE2F/54BE97/54BEDB and54C28C/54C2D2/54C2F0. The path owner
        //queues Wait before publishing the next event; it does not land here.
        self.sim.planning_start_actor(self.stable_id);
    }
    /// `FacingClass::UpdateFacing @ 0x004C9300` on the body facing (`+0x388`).
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
        let frame_effects = self.frame_effects;
        let registry = self.registry;
        runtime.with_owner_call(self.sim, id, |sim| {
            let _ = sim.assign_destination_represented(
                id,
                Some(crate::sim::components::NavTargetRef::cell(
                    cell.0 as u16,
                    cell.1 as u16,
                )),
                rules,
                registry,
                frame_effects,
            );
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
        let frame_effects = self.frame_effects;
        runtime.with_owner_call(self.sim, id, |sim| {
            sim.jumpjet_stop_moving(id, rules, registry, frame_effects);
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
            .filter(|contact| contact.category() == EntityCategory::Structure)
            .and_then(|contact| self.sim.interner.object_type(contact.type_ref(), rules))
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
        let frame_effects = self.frame_effects;
        runtime.with_owner_call(self.sim, id, |sim| {
            sim.jumpjet_lift_off_notify(id, rules, registry, frame_effects);
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
    fn touchdown(&mut self, runtime: &mut JumpjetRuntime) -> bool {
        self.touched_down = true;
        let id = self.stable_id;
        let rules = self.rules;
        let registry = self.registry;
        let frame_effects = self.frame_effects;
        let result = runtime.with_owner_call(self.sim, id, |sim| {
            let changed = sim.per_cell_process(
                id,
                crate::sim::movement::PerCellReason::Arrival,
                rules,
                registry,
                frame_effects,
            )?;
            sim.assign_null_destination(id, rules, registry, frame_effects);
            Ok(changed)
        });
        match result {
            Ok(changed) => self.bridge_state_changed |= changed,
            Err(error) => {
                self.per_cell_error = Some(error);
                return false;
            }
        }
        // State4_Descend54C905..54C92C jumps to54CA7C after both callbacks
        // for a dead, limbo or falling Foot. This is ObjectAlive, not Health.
        // Evidence: retail gamemd.exe, tools.native_inspect at54C8CB..54CA83.
        if !self.sim.substrate.entities.get(id).is_some_and(|owner| {
            owner.lifecycle.object_alive()
                && !owner.lifecycle.in_limbo()
                && !owner.is_falling_down()
        }) {
            return false;
        }
        self.sim.refresh_object_sight(id, rules, 0);
        let location = self.location();
        let here = self.cell_of([location[0], location[1]]);
        if self.holds_air_slot_at(here) {
            self.release_air_slot_at(here);
        }
        self.sim.aircraft_tracker_remove(id);
        true
    }
    fn finish_touchdown(&mut self, runtime: &mut JumpjetRuntime) {
        let id = self.stable_id;
        let location = self.location();
        let here = self.cell_of([location[0], location[1]]);
        // 0x0054C9F6: CellClass::PickupCrate on the Foot's cell (vt+0x1BC)
        // after the tracker removal (0x0054C9DC); its answer is not read.
        let rules = self.rules;
        let registry = self.registry;
        let frame_effects = self.frame_effects;
        // Publish phase0 before the crate's synchronous receivers, then keep
        // their runtime writes. Native does not repeat the alive gate here.
        runtime.with_owner_call(self.sim, id, |sim| {
            let _ = sim.pickup_crate_at(id, here, rules, registry, frame_effects);
        });
        let deploy_to_land = self.owner_kind() == FlightOwnerKind::Unit && self.type_flag_6ad();
        let owner = self.sim.substrate.entities.get_mut(id).unwrap();
        owner.lift.allow_end();
        owner.drop_credit.land();
        owner.crash.lower();
        if deploy_to_land {
            owner.clear_landing_for_deploy();
        }
    }
    fn crashing(&self) -> bool {
        self.owner().crash.is_crashing()
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
            self.sim.foot_mark_put(
                self.stable_id,
                self.rules,
                self.registry,
                self.frame_effects,
            );
        } else {
            self.sim.foot_mark_remove(
                self.stable_id,
                self.rules,
                self.registry,
                self.frame_effects,
            );
        }
    }
    fn crash_impact(&mut self) {
        self.impact = true;
    }
    fn crash_latched(&mut self) {
        self.crash_latched = true;
    }
    fn cell_coords(&self, cell: (i16, i16)) -> [i32; 3] {
        let coord = crate::sim::movement::target_cell_coord(
            cell.0 as u16,
            cell.1 as u16,
            self.terrain()
                .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                .as_ref(),
        );
        [coord.x, coord.y, coord.z]
    }
    /// Owner `+0x427`: a Magnetron dropped it.
    fn dropped(&self) -> bool {
        self.owner().drop_credit.being_manipulated()
    }
    fn crush_below(&mut self, runtime: &mut JumpjetRuntime) -> bool {
        let Some(rules) = self.rules else {
            return false;
        };
        let id = self.stable_id;
        let registry = self.registry;
        let frame_effects = self.frame_effects;
        let impact = runtime.with_owner_call(self.sim, id, |sim| {
            sim.dropped_impact(id, rules, registry, frame_effects)
        });
        self.bridge_state_changed |= impact.bridge_state_changed;
        impact.crash
    }
    fn jumpjet_type(&self) -> bool {
        self.object_type().is_some_and(|object| object.jumpjet)
    }
    fn wheel_clear(&self, cell: (i16, i16)) -> bool {
        use crate::rules::locomotor_type::{MovementZone, SpeedType};
        use crate::sim::cell_rect::{
            CellPassabilityArgs, canonical_cell_ref, check_cell_passability,
        };
        // The Cell `MapClass::operator[] @ 0x00565730` returned (`0x0054CDEF`).
        self.terrain().is_some_and(|terrain| {
            check_cell_passability(
                &canonical_cell_ref(terrain, terrain.native_cell_identity(cell)),
                CellPassabilityArgs {
                    speed_type: SpeedType::Wheel,
                    ignore_infantry: true,
                    ignore_vehicles: true,
                    zone: None,
                    movement_zone: MovementZone::Normal,
                    level: None,
                    is_bridge: true,
                },
                &self.sim.cell_passability_map(),
            )
            .admits()
        })
    }
    fn sink(&mut self, runtime: &mut JumpjetRuntime) {
        let Some(rules) = self.rules else {
            return;
        };
        let id = self.stable_id;
        let registry = self.registry;
        let frame_effects = self.frame_effects;
        runtime.with_owner_call(self.sim, id, |sim| {
            sim.dropped_sink(id, rules, registry, frame_effects);
        });
    }
    fn null_destination(&mut self, runtime: &mut JumpjetRuntime) {
        let id = self.stable_id;
        let rules = self.rules;
        let registry = self.registry;
        let frame_effects = self.frame_effects;
        runtime.with_owner_call(self.sim, id, |sim| {
            sim.assign_null_destination(id, rules, registry, frame_effects);
        });
    }
    /// The Jumpjet crash state `0x0054CA90`'s clear of `+0x428`/`+0x42C`
    /// (`0x0054CFEE..0x0054CFF7`).
    fn clear_drop_credit(&mut self) {
        let owner = self.sim.substrate.entities.get_mut(self.stable_id).unwrap();
        owner.drop_credit.forget();
    }
    /// The crash's OnBridge raise (`0x0054D012`).
    fn set_on_bridge(&mut self) {
        self.sim
            .substrate
            .entities
            .get_mut(self.stable_id)
            .unwrap()
            .on_bridge
            .store(true);
    }
}

fn jumpjet_locomotor(entity: &crate::sim::game_entity::GameEntity) -> bool {
    entity.locomotor.as_ref().is_some_and(|locomotor| {
        locomotor.active_kind() == LocomotorKind::Jumpjet && locomotor.jumpjet_runtime().is_some()
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
    pub(crate) fn jumpjet_lift_off_notify(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        use crate::sim::combat::TargetKind;
        let Some(owner) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(object) = self.interner.object_type(owner.type_ref(), rules) else {
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
            frame_effects,
        );
        let Some(owner) = self.substrate.entities.get(id) else {
            return;
        };
        let archive = owner.archive_target();
        let nav_com = owner.navigation.nav_com().map(TargetKind::from);
        if let Some(archive) = archive.filter(|archive| Some(*archive) != nav_com) {
            // An archived object that is gone gives no coordinate.
            if let TargetKind::Entity(target) = archive
                && self.substrate.entities.get(target).is_none()
            {
                return;
            }
            // Native does not read the setter's answer.
            let _ = self.assign_destination_represented(
                id,
                Some(crate::sim::components::NavTargetRef::from(archive)),
                Some(rules),
                registry,
                frame_effects,
            );
            return;
        }
        if let Some(owner) = self.substrate.entities.get_mut(id) {
            owner.navigation.stop_moving();
        }
        if let Err(cause) = self.scatter(
            id,
            None,
            crate::sim::movement::ScatterFlags::new(true, false),
            rules,
            registry,
            frame_effects,
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
        frame_effects: FrameEffects<'_>,
    ) -> Result<Option<AirMovementTickStats>, super::FrameAdvanceError> {
        if !self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(jumpjet_locomotor)
        {
            return Ok(None);
        }
        let frame = self.session.binary_frame;
        let trig = TrigTable::embedded();
        let stats = self.with_jumpjet_process(stable_id, |runtime, sim| {
            let entry_state = runtime.phase();
            let mut host = CruiseHost {
                sim,
                frame,
                trig,
                rules,
                registry,
                frame_effects,
                stable_id,
                touched_down: false,
                impact: false,
                crash_latched: false,
                bridge_state_changed: false,
                per_cell_error: None,
            };
            let state = jumpjet_flight::process(runtime, &mut host);
            if let Some(error) = host.per_cell_error {
                return Err(error);
            }
            let ended_cruise =
                entry_state == STATE_TRANSLATE && matches!(state, STATE_HOLD | STATE_DESCEND);
            let entity = host.sim.substrate.entities.get_mut(stable_id).unwrap();
            if host.crash_latched
                && entity.category() == EntityCategory::Infantry
                && let Some(rules) = rules
                && let Err(cause) = host.sim.infantry_do_action(
                    stable_id,
                    crate::sim::movement::infantry_action::DO_AIR_DEATH_START,
                    false,
                    rules,
                    frame_effects,
                )
            {
                log::debug!("infantry {stable_id} AirDeathStart: {cause}");
            }
            Ok(AirMovementTickStats {
                arrivals: u32::from(host.touched_down || ended_cruise),
                impact: host.impact,
                touched_down: host.touched_down,
                bridge_state_changed: host.bridge_state_changed,
            })
        });
        stats.transpose()
    }
}

#[cfg(test)]
#[path = "jumpjet_cruise_native_tests.rs"]
mod native_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::jumpjet_params::JumpjetParams;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::ground_pose::position_world_coord;
    use crate::sim::movement::jumpjet_movement::jumpjet_flight::STATE_ASCEND;
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::util::fixed_math::SimFixed;
    use serde_json::Value;

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
    /// (state 0, locomotor facing `0x4000`), hovering at 500 at cell (10,10);
    /// [`order_east`] gives it its move to cell (16,10). The terrain is the
    /// oracle fixture: level 0 and flat, except cell (9,10) at level 2 with
    /// slope 1.
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
        entity.set_category_for_test(EntityCategory::Unit);
        entity.body_facing.snap(u16::from(body_facing) << 8, 0);
        entity.position.set_sub_x_for_test(SimFixed::from_num(128));
        entity.position.set_sub_y_for_test(SimFixed::from_num(128));
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
            flight.facing_mut().snap(u16::from(body_facing) << 8, 0);
            flight.set_target_height(500);
            *runtime = runtime
                .clone()
                .with_phase_for_test(STATE_TRANSLATE)
                .with_moving_for_test(true)
                .with_flight_for_test(flight);
        }
        entity.locomotor = Some(locomotor);
        sim.substrate.entities.insert(entity);
        sim.set_object_height_unmarked(1, 500);
        sim
    }

    /// The fixture's order to cell (16,10) at frame 1001, as Foot's setter
    /// gives it: the Cell NavCom (`0x004D9510`), then the Jumpjet's `Move_To`
    /// with the cell's coordinate (`0x004D965D`).
    fn order_east(sim: &mut Simulation) {
        sim.session.binary_frame = 1001;
        let request = crate::sim::movement::target_cell_coord(
            16,
            10,
            sim.resolved_terrain
                .as_ref()
                .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                .as_ref(),
        );
        sim.substrate
            .entities
            .get_mut(1)
            .expect("jumpjet")
            .navigation
            .set_nav_com_for_test(Some(crate::sim::components::NavTargetRef::cell(16, 10)));
        sim.jumpjet_move_to(1, request, None)
            .expect("Move_To on the fixture map");
    }

    fn touchdown_crate_fixture() -> (Simulation, RuleSet, OverlayTypeRegistry) {
        let mut sim = hovering_jumpjet(0x40);
        let rules = crate::sim::crates::tests::crate_ruleset("");
        let registry = crate::sim::crates::tests::crate_registry();
        sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(24, 16));
        let wood = registry.id_for_name("WOOD").unwrap();
        sim.overlay_grid
            .as_mut()
            .unwrap()
            .place_overlay(10, 10, wood, 0);
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("JUMPJETUNIT");
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
        );
        let e = sim.substrate.entities.get_mut(1).unwrap();
        e.set_owner_for_test(owner);
        e.set_type_ref_for_test(type_ref);
        e.lifecycle.set_in_limbo_for_test(false);
        e.lifecycle.set_cell_marked(true);
        e.position.put_z(0);
        let runtime = e.locomotor.as_mut().unwrap().jumpjet_runtime_mut().unwrap();
        *runtime = runtime
            .clone()
            .with_phase_for_test(STATE_DESCEND)
            .with_moving_for_test(true)
            .with_landing_latched_for_test(true)
            .with_destination_for_test(DriveCoord::cell(10, 10, 0));
        sim.aircraft_tracker_add(1);
        sim.substrate.air_slots.set_holder((10, 10), Some(1));
        (sim, rules, registry)
    }

    /// Supply an admitted State4 entry after Update. Owner callbacks remain
    /// production calls, including the vanished-deck DropAsBomb below.
    fn descend_once(sim: &mut Simulation, rules: &RuleSet, registry: &OverlayTypeRegistry) -> i32 {
        sim.with_jumpjet_process(1, |runtime, sim| {
            let mut host = CruiseHost {
                sim,
                frame: 0,
                trig: TrigTable::embedded(),
                rules: Some(rules),
                registry: Some(registry),
                frame_effects: FrameEffects::default(),
                stable_id: 1,
                touched_down: false,
                impact: false,
                crash_latched: false,
                bridge_state_changed: false,
                per_cell_error: None,
            };
            let phase = jumpjet_flight::state4_descend(runtime, &mut host);
            assert!(host.touched_down);
            assert!(host.per_cell_error.is_none());
            phase
        })
        .unwrap()
    }

    #[test]
    fn a_jumpjet_that_starts_falling_at_touchdown_does_not_finish_landing() {
        let (mut sim, rules, registry) = touchdown_crate_fixture();
        let e = sim.substrate.entities.get_mut(1).unwrap();
        e.on_bridge.store(true);
        e.position
            .put_z(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
        e.drop_credit.set_being_manipulated_for_test(true);
        let runtime = e.locomotor.as_mut().unwrap().jumpjet_runtime_mut().unwrap();
        *runtime = runtime.clone().with_destination_for_test(DriveCoord::cell(
            10,
            10,
            crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS,
        ));
        assert_eq!(descend_once(&mut sim, &rules, &registry), STATE_DESCEND);
        let e = sim.substrate.entities.get(1).unwrap();
        assert!(
            e.is_falling_down(),
            "the actual PerCell callback started a fall"
        );
        assert_eq!(
            sim.overlay_grid.as_ref().unwrap().cell(10, 10).overlay_id,
            registry.id_for_name("WOOD"),
            "aborted touchdown must not collect its crate"
        );
        assert!(!e.lift.may_end());
        assert!(e.drop_credit.being_manipulated());
        assert!(
            e.air_spatial_bucket().is_some(),
            "the landing did not remove its tracker"
        );
    }

    /// Supplied flags exercise each native54C905 gate independently; zero HP
    /// deliberately keeps ObjectAlive set and is a successful control.
    #[test]
    fn touchdown_uses_object_lifecycle_flags_and_preserves_aborted_state() {
        for (name, aborted) in [
            ("dead", true),
            ("limbo", true),
            ("falling", true),
            ("healthy", false),
            ("health_zero", false),
        ] {
            let (mut sim, rules, registry) = touchdown_crate_fixture();
            let e = sim.substrate.entities.get_mut(1).unwrap();
            e.crash.set_crashing_for_test(true);
            e.drop_credit.set_being_manipulated_for_test(true);
            match name {
                "dead" => e.lifecycle.set_object_alive_for_test(false),
                "limbo" => e.lifecycle.set_in_limbo_for_test(true),
                "falling" => e.set_falling_down_for_test(true),
                "health_zero" => e.health.set_for_test(0),
                _ => {}
            }
            let rng = sim.scenario_rng.native_state_hex();
            let phase = descend_once(&mut sim, &rules, &registry);
            let e = sim.substrate.entities.get(1).unwrap();
            let runtime = e.locomotor.as_ref().unwrap().jumpjet_runtime().unwrap();
            assert_eq!(
                phase,
                if aborted {
                    STATE_DESCEND
                } else {
                    jumpjet_flight::STATE_GROUND
                },
                "{name}"
            );
            assert_eq!(
                runtime.landing_latched(),
                aborted,
                "{name}: admission latch"
            );
            assert_eq!(e.lift.may_end(), !aborted, "{name}: lift-end permission");
            assert_eq!(
                e.drop_credit.being_manipulated(),
                aborted,
                "{name}: drop credit"
            );
            assert_eq!(e.crash.is_crashing(), aborted, "{name}: crash latch");
            assert_eq!(e.air_spatial_bucket().is_some(), aborted, "{name}: tracker");
            assert_eq!(
                sim.substrate.air_slots.holder(10, 10),
                aborted.then_some(1),
                "{name}: slot"
            );
            assert_eq!(
                sim.overlay_grid
                    .as_ref()
                    .unwrap()
                    .cell(10, 10)
                    .overlay_id
                    .is_some(),
                aborted,
                "{name}: crate"
            );
            if aborted {
                assert_eq!(
                    sim.scenario_rng.native_state_hex(),
                    rng,
                    "{name}: no crate RNG"
                );
            }
        }
    }

    /// The real Infantry PerCell garrison arm conceals an occupier. Its
    /// landing must not consume the crate before that callback runs.
    #[test]
    fn an_infantry_concealed_by_its_touchdown_callback_leaves_the_crate() {
        use crate::sim::components::NavTargetRef;
        use crate::sim::mission::{MissionId, MissionType};
        let (mut sim, rules, registry) = super::super::entry_test_fixture::fixture_with_rules(
            "[JUMPJET]\nOccupier=yes\nBalloonHover=no\nJumpjetNoWobbles=yes\n\
             [BuildingTypes]\n1=TESTG\n[TESTG]\nStrength=1000\nFoundation=1x1\n\
             CanBeOccupied=yes\nMaxNumberOccupants=10\n\
             [OverlayTypes]\n239=WOOD\n[WOOD]\nCrate=yes\n",
        );
        let owner = sim.interner.intern("Americans");
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
        );
        let building = sim
            .spawn_object("TESTG", "Americans", 15, 15, 0, &rules)
            .unwrap();
        let id = sim
            .spawn_object("JUMPJET", "Americans", 13, 15, 0, &rules)
            .unwrap();
        let wood = registry.id_for_name("WOOD").unwrap();
        sim.overlay_grid
            .as_mut()
            .unwrap()
            .place_overlay(15, 15, wood, 0);
        let e = sim.substrate.entities.get_mut(id).unwrap();
        e.mission
            .set_current_for_test(MissionId::from_known(MissionType::Capture), 0);
        e.navigation
            .set_nav_com_for_test(Some(NavTargetRef::object(building)));
        let runtime = e.locomotor.as_mut().unwrap().jumpjet_runtime_mut().unwrap();
        let mut flight = runtime.flight();
        flight.set_target_height(0);
        *runtime = runtime
            .clone()
            .with_phase_for_test(STATE_DESCEND)
            .with_flight_for_test(flight)
            .with_moving_for_test(true)
            .with_landing_latched_for_test(true)
            .with_destination_for_test(DriveCoord::cell(15, 15, 0));
        sim.process_active_locomotor(
            id,
            Some(&rules),
            super::super::techno_ai::ObjectAiCtx {
                overlay_registry: Some(&registry),
                ..Default::default()
            },
        )
        .unwrap();
        let e = sim.substrate.entities.get(id).unwrap();
        assert!(
            e.lifecycle.in_limbo(),
            "the PerCell garrison receiver concealed the infantry"
        );
        assert!(!e.lift.may_end());
        assert_eq!(
            sim.overlay_grid.as_ref().unwrap().cell(15, 15).overlay_id,
            Some(wood)
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(building)
                .unwrap()
                .passenger_role
                .cargo()
                .unwrap()
                .passengers(),
            &[id]
        );
    }

    /// Park a Jumpjet with no order in a given native state and run the tick.
    fn idle_for(state: i32, balloon_hover: bool, frames: u32) -> Simulation {
        let mut sim = hovering_jumpjet(0x40);
        {
            let entity = sim.substrate.entities.get_mut(1).expect("jumpjet");
            let locomotor = entity.locomotor.as_mut().expect("locomotor");
            locomotor.set_balloon_hover_for_test(balloon_hover);
            let runtime = locomotor.jumpjet_runtime_mut().expect("runtime");
            *runtime = runtime
                .clone()
                .with_phase_for_test(state)
                .with_moving_for_test(false)
                .with_destination_for_test(JumpjetRuntime::NULL);
        }
        if state == jumpjet_flight::STATE_GROUND {
            sim.set_object_height_unmarked(1, 0);
        }
        for frame in 0..frames {
            sim.session.binary_frame = 1001 + frame;
            sim.tick_air_movement_with_cell_lists_one(
                1,
                None,
                None,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
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
            entity.set_foot_occupation_enabled(true);
            entity.position.set_exact_z_leptons_for_test(Some(0));
            let locomotor = entity.locomotor.as_mut().expect("locomotor");
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
        order_east(&mut sim);
        for frame in 0..80 {
            sim.session.binary_frame = 1001 + frame;
            sim.tick_air_movement_with_cell_lists_one(
                1,
                None,
                None,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        }
        let entity = sim.substrate.entities.get(1).expect("jumpjet");
        assert!(entity.position.exact_z_leptons().unwrap() >= 208);
        let here = (entity.position.rx(), entity.position.ry());
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
        assert_eq!(
            crate::sim::movement::air_movement::current_fly_height(
                entity,
                sim.resolved_terrain.as_ref()
            ),
            0
        );
        assert_eq!(
            locomotor.jumpjet_runtime().map(|runtime| runtime.phase()),
            Some(jumpjet_flight::STATE_GROUND)
        );
        assert_eq!((entity.position.rx(), entity.position.ry()), (10, 10));
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
        order_east(&mut sim);
        sim.tick_air_movement_with_cell_lists_one(
            1,
            None,
            None,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();

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
        assert_eq!((entity.position.rx(), entity.position.ry()), (10, 10));
    }

    /// Fly a native row through the production air tick, comparing the owner
    /// position and exact Z every frame, until arrival hands it to descent.
    fn fly_native_row(name: &str, body_facing: u8) {
        let row = native_row(name);
        let frames = row["output"]["frames"].as_array().expect("frames");
        let mut sim = hovering_jumpjet(body_facing);
        order_east(&mut sim);
        for (index, expected) in frames.iter().enumerate() {
            sim.session.binary_frame = 1001 + index as u32;
            let stats = sim
                .tick_air_movement_with_cell_lists_one(
                    1,
                    None,
                    None,
                    crate::sim::world::FrameEffects::default(),
                )
                .unwrap();
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
        let locomotor = entity.locomotor.as_ref().expect("locomotor");
        assert_eq!(
            locomotor.jumpjet_runtime().map(|runtime| runtime.phase()),
            Some(STATE_DESCEND)
        );
    }

    /// AircraftTracker Update (`0x004138C0`) runs only at the entry of State 1
    /// and State 3, after `Process` (`0x0054AEC0`) has moved the owner. A
    /// crashing Jumpjet still slides along its facing (`0x0054D55A..D607`)
    /// while State 5 drops it, so it can cross into another cell while its
    /// tracker cell (`+0x560`) stays where the fall began.
    #[test]
    fn a_falling_jumpjet_keeps_the_tracker_cell_it_crashed_in() {
        let mut sim = hovering_jumpjet(0x40);
        {
            let jumpjet = sim.substrate.entities.get_mut(1).expect("jumpjet");
            jumpjet.lifecycle.set_in_limbo_for_test(false);
            jumpjet.lifecycle.set_cell_marked(true);
            jumpjet.position.set_sub_x_for_test(SimFixed::from_num(250));
            let runtime = jumpjet
                .locomotor
                .as_mut()
                .and_then(|locomotor| locomotor.jumpjet_runtime_mut())
                .expect("runtime");
            let mut flight = runtime.flight();
            flight.set_current_speed_bits_for_test(14.0f64.to_bits());
            *runtime = runtime
                .clone()
                .with_phase_for_test(jumpjet_flight::STATE_CRASH)
                .with_flight_for_test(flight);
        }
        sim.aircraft_tracker_add(1);
        sim.session.binary_frame = 1001;
        let stats = sim
            .tick_air_movement_with_cell_lists_one(
                1,
                None,
                None,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        assert!(!stats.impact);
        let jumpjet = sim.substrate.entities.get(1).expect("jumpjet");
        assert_eq!((jumpjet.position.rx(), jumpjet.position.ry()), (11, 10));
        assert_eq!(jumpjet.air_tracker_cell(), (10, 10));
    }

    /// Production parity for the native `east_cruise` row, owner already facing
    /// east.
    #[test]
    fn production_cruise_flies_the_native_east_cruise_frames() {
        fly_native_row("east_cruise", 0x40);
    }

    /// Production parity for `east_from_west_facing`: the takeoff seed snaps the
    /// locomotor facing to the body facing (west), so the cruise turns around
    /// from there instead of starting from the linked `0x4000`.
    #[test]
    fn production_cruise_turns_from_the_body_facing() {
        fly_native_row("east_from_west_facing", 0xC0);
    }

    /// An explicit class NULL destination mid-cruise runs Foot's
    /// `Stop_Moving`, which re-targets the
    /// cell under the owner and keeps the moving byte, so it flies on to that
    /// cell's centre and lands there, and the NavCom its `Move_To` wrote is
    /// cleared again. It is applied once: the next frame finds no NavCom.
    #[test]
    fn a_null_destination_stops_at_the_cell_under_the_owner() {
        let mut sim = hovering_jumpjet(0x40);
        // Ordinary touchdown requires an actor outside constructor limbo.
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("JUMPJETUNIT");
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.set_owner_for_test(owner);
        entity.set_type_ref_for_test(type_ref);
        entity.lifecycle.set_in_limbo_for_test(false);
        order_east(&mut sim);
        for frame in 0..20 {
            sim.session.binary_frame = 1001 + frame;
            sim.tick_air_movement_with_cell_lists_one(
                1,
                None,
                None,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
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
        assert!(entity.navigation.nav_com().is_some());
        let here = (entity.position.rx(), entity.position.ry());
        assert!(here.0 < 16, "still on its way");
        sim.assign_null_destination(1, None, None, crate::sim::world::FrameEffects::default());

        sim.session.binary_frame = 1021;
        sim.tick_air_movement_with_cell_lists_one(
            1,
            None,
            None,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
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
        assert!(entity.navigation.nav_com().is_none());

        for frame in 0..400 {
            sim.session.binary_frame = 1022 + frame;
            sim.tick_air_movement_with_cell_lists_one(
                1,
                None,
                None,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        }
        let landed = runtime(&sim);
        assert_eq!(landed.phase(), jumpjet_flight::STATE_GROUND);
        assert!(!landed.moving());
        let entity = sim.substrate.entities.get(1).expect("jumpjet");
        assert_eq!((entity.position.rx(), entity.position.ry()), here);
        assert_eq!(entity.position.exact_z_leptons(), Some(0));
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
            sim.tick_air_movement_with_cell_lists_one(
                1,
                Some(&rules),
                None,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
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
            (entity.position.rx(), entity.position.ry())
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
                assert_eq!(entity.navigation.nav_com(), Some(goal_cell));
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
    /// closes past its bomb's range instead of stopping there, and stays on
    /// Hunt. A production witness, not a native comparison.
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
                entity.attack_target.target()
            else {
                continue;
            };
            let goal = center(sim, target);
            let goal_cell = crate::sim::components::NavTargetRef::cell(
                (goal.x / 256) as u16,
                (goal.y / 256) as u16,
            );
            if entity.navigation.nav_com() == Some(goal_cell) {
                approached.entry(target).or_insert(i32::MAX);
            }
            if let Some(closest) = approached.get_mut(&target) {
                let at = center(sim, kirov);
                let off = ((at.x - goal.x) as f64).hypot((at.y - goal.y) as f64) as i32;
                *closest = (*closest).min(off);
            }
        }
        // BlimpBomb's range is 1.5 cells (384 leptons): the Kirov closes past
        // it.
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
        let strength = sim.substrate.entities.get(tank).unwrap().health.current();
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
            damaged |= target.health.current() < strength;
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
            Some(i64::from(crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS))
        );
    }
}
