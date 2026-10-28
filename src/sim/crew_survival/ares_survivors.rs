//! Installed Ares survivor hooks: Unit737F97 ->1004C0E0 and
//! Aircraft41668B ->10048C50 share100464C0.
//! Cargo, placement, veterancy, mind control and destruction retain their
//! existing owners; this module owns only the optional survivor transaction.

use super::{CrewUnlimbo, DyingTransport, Simulation, ares_techno_crew_type};
use crate::map::cell_index::NativeCellIdentity;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{KillCallback, KillSource};
use crate::sim::components::DriveCoord;
use crate::sim::movement::{ScatterFlags, ground_pose, locomotor::MovementLayer, walk_head};
use crate::sim::passenger::{DepartureRoute, depart_cargo_head};
use crate::sim::world::{FrameEffects, SimSoundEvent, UninitContext};
use crate::util::fixed_math::SimFixed;

impl Simulation {
    ///100445E0 picks one Scenario direction, then CellPlace481180(0,0,0).
    /// The wrapper passes returned XY and the original Z to10044690.
    pub(crate) fn place_ares_survivor(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        id: u64,
        mut point: DriveCoord,
        radius: i32,
        selected: bool,
        effects: FrameEffects<'_>,
    ) -> bool {
        let direction = self.scenario_rng.next_range_u32_inclusive(0, 7) as usize;
        let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[direction];
        point.x = point.x.wrapping_add(dx.wrapping_mul(radius));
        point.y = point.y.wrapping_add(dy.wrapping_mul(radius));
        let spot = Self::place_infantry_in_ground_cell(
            &self.interner,
            &mut self.scenario_rng,
            &self.substrate,
            &self.resolved_terrain,
            rules,
            point,
        );
        point = spot.map_or(
            DriveCoord {
                x: 0,
                y: 0,
                z: point.z,
            },
            |spot| walk_head::selected_head(point, spot, point.z, false),
        );
        self.ares_survivor_unlimbo(
            rules,
            registry,
            id,
            point,
            spot.unwrap_or(0),
            selected,
            effects,
        )
    }

    ///10044690: nullable fixed-stride cell lookup, bridge/floor, class Unlimbo
    /// or SpawnParachuted, retained cell, Scatter/mission, seek clear, Select.
    fn ares_survivor_unlimbo(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        id: u64,
        mut point: DriveCoord,
        spot: u8,
        selected: bool,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let packed = ((point.x / 256) as i16, (point.y / 256) as i16);
        //100446D9 rejects missing cells; do not stamp/use the shared Dummy.
        let Some(index) = terrain.native_fixed_cell_index(packed.0, packed.1) else {
            return false;
        };
        let cell = NativeCellIdentity::Real(index);
        let query = NativeCellQuery::canonical(terrain);
        let on_bridge = query.flags(cell) & 0x100 != 0;
        let retained = query.coord(cell);
        let Some((_, _, ground)) = crate::sim::cell_kernel::native_cell_own_coords(cell, &query)
        else {
            return false;
        };
        let floor = (ground as i32).wrapping_add(if on_bridge { 0x1a0 } else { 0 });
        let level = terrain.cells()[index].level;
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        entity.on_bridge.store(on_bridge);
        let prior_carrier = entity.passenger_role.inside_transport();
        let infantry = entity.category() == EntityCategory::Infantry;
        let airborne = point.z.wrapping_sub(floor) > 0xd0;
        let placed = if airborne {
            let context = UninitContext::new(rules, registry).with_effects(effects);
            self.techno_limbo_with_context(id, context);
            crate::sim::aircraft::drop_payload::spawn_parachuted(
                self, rules, registry, id, point, spot, effects,
            )
            .is_ok()
        } else {
            point.z = floor;
            let facing = (self.scenario_rng.next_range_u32_inclusive(0, 7) << 5) as u8;
            // The Rust relation suppresses occupation, so clear it immediately
            // before class Unlimbo and restore it on refusal. Native clears+11C
            // immediately after success100447E7, before any callbacks below.
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                entity.passenger_role.clear_carrier_relation();
            }
            let (sub_x, sub_y) = (
                SimFixed::from_num(point.x & 255),
                SimFixed::from_num(point.y & 255),
            );
            let unlimbo = if infantry && !on_bridge {
                CrewUnlimbo::Place {
                    cell: (packed.0 as u16, packed.1 as u16),
                    z: level,
                    raw_z: Some(floor),
                    request: (sub_x, sub_y),
                    caller_places_first: false,
                }
            } else {
                CrewUnlimbo::Exact {
                    rx: packed.0 as u16,
                    ry: packed.1 as u16,
                    z: level.wrapping_add(u8::from(on_bridge) * 4),
                    raw_z: Some(floor),
                    sub_x,
                    sub_y,
                    on_bridge,
                }
            };
            self.unlimbo_crew(rules, id, unlimbo, Some(facing), registry, effects)
        };
        if !placed {
            if let Some((carrier, open_topped)) = prior_carrier
                && let Some(entity) = self.substrate.entities.get_mut(id)
            {
                entity
                    .passenger_role
                    .set_inside_transport(carrier, open_topped);
            }
            return false;
        }
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.passenger_role.clear_carrier_relation();
        }
        Self::foot_neighbors_store_retained_cell(&mut self.substrate, id, retained);
        if airborne {
            let layer = if on_bridge {
                MovementLayer::Bridge
            } else {
                MovementLayer::Ground
            };
            let key = crate::sim::occupancy::RawCellKey::from_native(
                self.resolved_terrain.as_ref().unwrap(),
                cell,
            );
            if self.substrate.raw_cell_occupation.bits_at(key, layer) & 0x1c == 0x1c {
                let _ = self.scatter_cell_objects(
                    (retained.0 as u16, retained.1 as u16),
                    layer,
                    None,
                    ScatterFlags::new(true, true),
                    rules,
                    registry,
                    effects,
                );
            }
        } else {
            let _ = self.scatter(
                id,
                None,
                ScatterFlags::new(true, false),
                rules,
                registry,
                effects,
            );
            if let Some(entity) = self.substrate.entities.get(id) {
                let mission = if self.owner_is_human(entity.owner()) {
                    super::MissionType::Guard
                } else {
                    super::MissionType::Hunt
                };
                self.substrate.queue_crew_mission(id, mission);
            }
        }
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.building_entry.clear_occupy_seeks();
        }
        if selected {
            self.techno_select(
                id,
                true,
                UninitContext::new(rules, registry).with_effects(effects),
            );
        }
        true
    }

    /// A newly constructed pilot fails via RecordKill then scalar destructor,
    ///100465F9..10046616 /1004676C..10046789, rather than passenger UnInit.
    fn discard_ares_pilot(
        &mut self,
        id: u64,
        dying: DyingTransport,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        self.record_the_kill(
            id,
            KillSource::Techno(dying.attacker),
            KillCallback::Terminal,
            UninitContext::new(rules, registry).with_effects(effects),
        );
        self.discard_constructed_limbo(id, rules, effects);
    }

    /// Ares100464C0: the incoming flag is ReceiveDamage IgnoreDefenses (arg5),
    /// not arg6 PreventCrewEscape. Captor/pilots precede ordinary passengers.
    pub(crate) fn release_ares_vehicle_survivors(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        unit_id: u64,
        dying: DyingTransport,
        effects: FrameEffects<'_>,
    ) {
        let Some(unit) = self.substrate.entities.get(unit_id) else {
            return;
        };
        let Some(object) = self.entity_object_type(unit, rules) else {
            return;
        };
        let crew_rules = object.crew_rules();
        let crewed = object.crewed;
        let open_topped = object.open_topped;
        let original_owner = unit.owner();
        let origin = ground_pose::position_world_coord(&unit.position);
        if unit.category() == EntityCategory::Unit && open_topped {
            self.clear_all_in_open_transport(unit_id);
        }
        let release_crew = self
            .substrate
            .entities
            .get_mut(unit_id)
            .is_some_and(|unit| {
                unit.vehicle_driver
                    .begin_survivor_release(dying.ignore_defenses)
            });
        if release_crew {
            let mut count = crew_rules.pilot_count(i32::from(crewed));
            let captor = self.substrate.entities.get(unit_id).map(|unit| {
                (
                    unit.capture_infantry_type_index.get(),
                    unit.vehicle_driver.captured(),
                )
            });
            if let Some((index, saved)) = captor.filter(|(index, _)| *index >= 0) {
                // Clear+338 before every skip/placement result10046574.
                self.substrate
                    .entities
                    .get_mut(unit_id)
                    .unwrap()
                    .capture_infantry_type_index
                    .record(-1);
                if let Some(kind) =
                    rules.type_array_at(crate::rules::object_type::ObjectCategory::Infantry, index)
                    && !kind.driver_rules().one_time()
                {
                    let owner = saved.owner().unwrap_or(original_owner);
                    if self
                        .houses
                        .get(&owner)
                        .is_some_and(|house| !house.is_defeated())
                        && let Some(id) = self.construct_crew_limbo(
                            rules,
                            &kind.id,
                            owner,
                            ((origin.x / 256) as u16, (origin.y / 256) as u16),
                            0,
                            effects,
                        )
                    {
                        if let Some(pilot) = self.substrate.entities.get_mut(id) {
                            pilot.health.set(saved.health().max(10) / 2);
                            pilot.veterancy.inherit_raw(saved.experience());
                        }
                        if self.place_ares_survivor(
                            rules,
                            registry,
                            id,
                            origin,
                            144,
                            dying.selected_by_player,
                            effects,
                        ) {
                            if let Some(controller) = self
                                .substrate
                                .entities
                                .get(unit_id)
                                .and_then(|unit| unit.mind_control.controller())
                            {
                                self.with_object_placement_scope(|sim| {
                                    sim.free_unit(controller, unit_id, rules, registry, effects);
                                    sim.capture_unit(controller, id, rules, registry, effects);
                                });
                                self.substrate
                                    .queue_crew_mission(id, super::MissionType::Guard);
                            }
                            if let Some(sound_id) = kind.leave_transport_sound.clone() {
                                self.sound_events.emit(SimSoundEvent::VocAtCoord {
                                    sound_id,
                                    coordinate: origin,
                                    flags: 0,
                                });
                                effects.flush_sound(self, rules);
                            }
                            let killed = kind.driver_rules().kill_pilots();
                            count = if killed < 0 {
                                0
                            } else {
                                count.wrapping_sub(killed)
                            };
                        } else {
                            self.discard_ares_pilot(id, dying, rules, registry, effects);
                        }
                    }
                }
            }
            let raw = self
                .substrate
                .entities
                .get(unit_id)
                .map(|unit| unit.veterancy.raw());
            let chance = raw
                .map(|raw| crew_rules.pilot_chances()[rank_index(raw)])
                .unwrap_or(0);
            let chance = if chance < 0 {
                use crate::util::native_x87::X87Chop53 as X;
                X::load_f64(rules.general.crew_escape).map_or(0, |escape| {
                    X::ftol_i32_low_masked(X::mul(escape, X::load_i32(100)))
                })
            } else {
                chance
            };
            if crewed && chance > 0 {
                for _ in 0..count {
                    let crew = self.substrate.entities.get(unit_id).and_then(|unit| {
                        let object = self.interner.object_type(unit.type_ref(), rules)?;
                        let house = self.houses.get(&unit.owner())?;
                        ares_techno_crew_type(
                            &mut self.scenario_rng,
                            rules,
                            unit,
                            object,
                            house,
                            &self.interner,
                        )
                    });
                    let Some(crew) = crew else {
                        continue;
                    };
                    if self.scenario_rng.next_range_u32_inclusive(1, 100) as i32 > chance {
                        continue;
                    }
                    if let Some(id) = self.construct_crew_limbo(
                        rules,
                        &crew,
                        original_owner,
                        ((origin.x / 256) as u16, (origin.y / 256) as u16),
                        0,
                        effects,
                    ) {
                        let experience = self
                            .substrate
                            .entities
                            .get(unit_id)
                            .map(|unit| unit.veterancy.raw());
                        if let Some(pilot) = self.substrate.entities.get_mut(id) {
                            pilot.health.set(pilot.health.current() / 2);
                            if let Some(raw) = experience {
                                pilot.veterancy.inherit_raw(raw);
                            }
                        }
                        if self.place_ares_survivor(
                            rules,
                            registry,
                            id,
                            origin,
                            144,
                            dying.selected_by_player,
                            effects,
                        ) {
                            self.trigger_runtime.transfer_object_tag(unit_id, id);
                        } else {
                            self.discard_ares_pilot(id, dying, rules, registry, effects);
                        }
                    }
                }
            }
        }
        //100467B9 samples raw rank again after crew callbacks. Negative uses
        //the original Unit CanEnterCell0/2 fallback; positive chances bypass it.
        let Some(unit) = self.substrate.entities.get(unit_id) else {
            return;
        };
        let chance = crew_rules.passenger_chances()[rank_index(unit.veterancy.raw())];
        let unit_fallback = unit.category() == EntityCategory::Unit;
        let cell = self
            .resolved_terrain
            .as_ref()
            .map(|terrain| NativeCellQuery::canonical(terrain).lookup_world(origin.x, origin.y));
        while self
            .substrate
            .entities
            .get(unit_id)
            .is_some_and(|unit| unit.passenger_role.passenger_count() != 0)
        {
            let result = depart_cargo_head(
                self,
                rules,
                registry,
                unit_id,
                DepartureRoute::DeathEscape,
                |sim, id| {
                    let admitted = !dying.ignore_defenses
                        && if chance > 0 {
                            sim.scenario_rng.next_range_u32_inclusive(1, 100) as i32 <= chance
                        } else if chance < 0 && unit_fallback {
                            cell.is_some_and(|cell| matches!(sim.foot_can_enter(id, cell,
                            crate::sim::movement::infantry_entry::InfantryEntryArgs::REPAIR, rules, registry), Ok(0 | 2)))
                        } else {
                            false
                        };
                    if !admitted
                        || !sim.place_ares_survivor(
                            rules,
                            registry,
                            id,
                            origin,
                            128,
                            dying.selected_by_player,
                            effects,
                        )
                    {
                        sim.record_kill_and_uninit(
                            id,
                            dying.attacker,
                            UninitContext::new(rules, registry).with_effects(effects),
                        );
                    }
                    Ok(())
                },
                effects,
            );
            if result.is_err() {
                break;
            }
        }
    }
}

fn rank_index(raw: crate::util::native_x87::NativeF32Bits) -> usize {
    use crate::sim::combat::veterancy::{VeterancyRank, rank_of};
    match rank_of(raw) {
        VeterancyRank::Rookie => 0,
        VeterancyRank::Veteran => 1,
        VeterancyRank::Elite => 2,
    }
}
