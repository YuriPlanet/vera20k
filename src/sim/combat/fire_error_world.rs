//! The world's side of GetFireError: [`FireFacts`] read from live objects and
//! a [`FireQuery`] answered by each question's owner.
//!
//! RESIDUAL questions VERA cannot answer natively, each fixed to the value its
//! missing producer would leave (trigger, effect and frequency are on the
//! owning row in `fire_error.rs`):
//! - `target_layer` answers the ground layer: VERA has no locomotor layer, so
//!   T39 never fires (a Foot target taking off or landing below two levels).
//! - `deploy_cell_ok` answers true: `CellClass 0x00487C10` is not ported, and
//!   only U4's dormant DeployToFire reads it.
//! - `locomotor_can_fire` answers OK: only the Tunnel locomotor, unused in
//!   retail, refuses.

use super::fire_error::{
    CellFacts, FireError, FireFacts, FireQuery, FireTargetKind, FirerCell, FirerClass, FirerFacts,
    FirerTypeFacts, Link, ProjectileFacts, RadioLink, SpawnCounts, TargetFacts, TargetTypeFacts,
    Transporter, WarheadFacts, WeaponFacts,
};
use super::{TargetKind, combat_weapon, in_range, line_of_fire};
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::object_type::ObjectType;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::weapon_type::WeaponType;
use crate::sim::game_entity::GameEntity;
use crate::sim::vision::FogState;
use crate::sim::world::Simulation;

/// One firer, target and weapon slot, as GetFireError sees them.
pub(crate) struct FireSubject<'a> {
    pub world: &'a Simulation,
    pub rules: &'a RuleSet,
    pub overlay_registry: Option<&'a OverlayTypeRegistry>,
    pub fog: Option<&'a FogState>,
    pub firer: &'a GameEntity,
    pub obj: &'a ObjectType,
    pub target: Option<TargetKind>,
    pub weapon_index: i32,
    /// A garrisoned building's GetWeapon (`0x004526F0`) answers its firing
    /// occupant's `OccupyWeapon` for every slot, with the garrison range.
    pub garrison: Option<(&'a WeaponType, crate::util::fixed_math::SimFixed)>,
}

impl FireSubject<'_> {
    /// GetFireError for this subject.
    pub(crate) fn fire_error(&self, check_range: bool) -> FireError {
        let facts = self.facts();
        super::fire_error::get_fire_error(&facts, &mut WorldQuery { subject: self }, check_range)
    }

    /// `CanFireAt @ 0x006F77B0`: InRange (vt+0x3A8) with the subject's weapon.
    pub(crate) fn in_range(&self) -> bool {
        WorldQuery { subject: self }.in_range()
    }

    /// `GetWeaponDamageValue(-1) @ 0x006F3970` through this subject's
    /// GetWeapon, so a garrison reads its occupant's weapon.
    pub(crate) fn weapon_damage_value(&self) -> i32 {
        super::fire_error::weapon_value(&self.facts(), &mut WorldQuery { subject: self })
    }

    fn target_entity(&self) -> Option<&GameEntity> {
        match self.target {
            Some(TargetKind::Entity(id)) => self.world.substrate.entities.get(id),
            _ => None,
        }
    }

    fn target_obj(&self) -> Option<&ObjectType> {
        self.target_entity().and_then(|target| {
            self.rules
                .object(self.world.interner.resolve(target.type_ref()))
        })
    }

    fn terrain(&self) -> Option<&ResolvedTerrainGrid> {
        self.world.resolved_terrain.as_ref()
    }

    fn frame(&self) -> u32 {
        self.world.session.binary_frame
    }

    /// GetWeapon (vt+0x3F8) at an index: a garrison's occupant weapon for
    /// every slot (`0x004526F0`), else the rank-selected slot.
    pub(crate) fn weapon_at(&self, index: i32) -> Option<&WeaponType> {
        if let Some((weapon, _)) = self.garrison {
            return (index >= 0).then_some(weapon);
        }
        combat_weapon::weapon_for_index(self.obj, self.firer.veterancy, index)
            .and_then(|(weapon_id, _)| self.rules.weapon(weapon_id))
    }

    fn weapon_facts(&self, weapon: &WeaponType) -> WeaponFacts {
        let warhead = combat_weapon::warhead_of(self.rules, weapon);
        let projectile = weapon
            .projectile
            .as_deref()
            .and_then(|id| self.rules.projectile(id));
        let armor = self
            .target_obj()
            .map(|target| super::armor_index(&target.armor));
        WeaponFacts {
            damage: weapon.damage,
            ambient_damage: weapon.ambient_damage,
            burst: weapon.burst,
            range: weapon.range_leptons,
            use_fire_particles: weapon.use_fire_particles,
            use_spark_particles: weapon.use_spark_particles,
            omni_fire: weapon.omni_fire,
            is_railgun: weapon.is_railgun,
            is_sonic: weapon.is_sonic,
            spawner: weapon.spawner,
            decloak_to_fire: weapon.decloak_to_fire,
            fire_while_moving: weapon.fire_while_moving,
            drain_weapon: weapon.drain_weapon,
            fire_in_transport: weapon.fire_in_transport,
            area_fire: weapon.area_fire,
            is_mag_beam: weapon.is_mag_beam,
            warhead: warhead.map(|warhead| WarheadFacts {
                mind_control: warhead.mind_control,
                ivan_bomb: warhead.ivan_bomb,
                parasite: warhead.parasite,
                temporal: warhead.temporal,
                is_locomotor: warhead.is_locomotor,
                psychedelic: warhead.psychedelic,
                bomb_disarm: warhead.bomb_disarm,
                verses_zero: armor.is_some_and(|armor| {
                    warhead
                        .verses_f64
                        .get(armor)
                        .is_some_and(|verses| *verses == 0.0 || verses.is_nan())
                }),
            }),
            // A weapon naming no projectile section reads the reader's
            // defaults (AA=no, AG=yes), as weapon selection does; retail
            // weapons always name one.
            projectile: projectile.map_or(
                ProjectileFacts {
                    aa: false,
                    ag: true,
                    rot: 0,
                },
                |projectile| ProjectileFacts {
                    aa: projectile.aa,
                    ag: projectile.ag,
                    rot: projectile.rot,
                },
            ),
        }
    }

    fn facts(&self) -> FireFacts {
        let firer = self.firer;
        let obj = self.obj;
        let frame = self.frame();
        let class = match firer.category {
            EntityCategory::Unit => FirerClass::Unit,
            EntityCategory::Infantry => FirerClass::Infantry,
            EntityCategory::Aircraft => FirerClass::Aircraft,
            EntityCategory::Structure => FirerClass::Building,
        };
        let target_id = match self.target {
            Some(TargetKind::Entity(id)) => Some(id),
            _ => None,
        };
        let link = |to: Option<u64>| match to {
            None => Link::None,
            Some(id) if Some(id) == target_id => Link::Target,
            Some(_) => Link::Other,
        };
        let attack = firer.attack_target.as_ref();
        let transport = firer
            .passenger_role
            .inside_transport_id()
            .and_then(|id| self.world.substrate.entities.get(id))
            .map(|transport| {
                (
                    transport,
                    self.rules
                        .object(self.world.interner.resolve(transport.type_ref())),
                )
            });
        // U5 reads the `+0x418` tether byte that radio `0x18` sets, whose
        // stand-in is the dock-entered projection (VERA owns no exact tether
        // byte, `radio/receive.rs`), and `RadioLinks[0]`.
        let radio_link = firer
            .radio_contacts
            .slot(0)
            .and_then(|id| self.world.substrate.entities.get(id));
        let current = |facing: Option<crate::sim::movement::FacingClass>, fallback: u8| {
            facing.map_or(u16::from(fallback) << 8, |facing| facing.current(frame))
        };
        let firer_facts = FirerFacts {
            enslaved: firer.slave_harvester.is_some(),
            warped_out: firer.is_warped_out(),
            warping_in: firer.is_warping_in(),
            on_bridge: firer.on_bridge,
            z: self
                .terrain()
                .and_then(|terrain| in_range::effective_z_leptons(firer, terrain))
                .map_or(0, |z| z as i32),
            berserk: firer.berserk.active,
            falling: firer.object_is_falling_down != 0,
            // VERA's passengers never reach the fire path, so a passenger
            // reaches T32/T33 only through a target of its own.
            in_open_transport: crate::sim::passenger::open_topped_transport(
                &self.world.substrate.entities,
                self.rules,
                &self.world.interner,
                firer,
            )
            .is_some(),
            transporter: match transport {
                None => Transporter::None,
                Some((transport, _)) if Some(transport.stable_id()) == target_id => {
                    Transporter::Target
                }
                Some((transport, _)) if transport.is_warped_out() => Transporter::WarpedOut,
                Some((transport, _)) if transport.passenger_role.is_inside_transport() => {
                    Transporter::Nested
                }
                Some(_) => Transporter::Plain,
            },
            wave_live: self
                .world
                .active_wave_links
                .contains_key(&firer.stable_id()),
            // `+0x308`: the damage-spark system, armed and expired on
            // `session.tick` by `techno_ai`'s common post step (`0` none,
            // `u64::MAX` held).
            spark_particles_live: self.world.session.tick < firer.damage_particle_live_until,
            rearming: attack
                .is_some_and(|attack| attack.cooldown_ticks > 0 || attack.burst_delay_ticks > 0),
            ammo: firer.aircraft_ammo.as_ref().map_or(-1, |ammo| ammo.current),
            cloak_state: firer.cloak.as_ref().map_or(0, |cloak| cloak.state),
            current_weapon: combat_weapon::attacker_facts(firer, obj).current_weapon_number,
            draining_me: firer.draining_me.is_some(),
            drain_target: link(firer.drain_target),
            temporal: firer
                .temporal
                .has_link()
                .then(|| link(firer.temporal.warp_target())),
            owner_is_human: crate::sim::house_state::house_state_for_owner_id(
                &self.world.houses,
                firer.owner(),
            )
            .is_some_and(|house| house.is_human),
            paralyzed: firer.is_paralyzed(frame),
            spawns: firer.spawn_manager.as_ref().map(|manager| SpawnCounts {
                not_regenerating: manager.count_alive_spawns() as i32,
                launched_missiles: manager.count_launched_missiles(
                    &self.world.substrate.entities,
                    self.rules,
                    &self.world.interner,
                ) as i32,
            }),
            // A Building turns its turret with `+0x388`, which VERA keeps in
            // `barrel_facing`; other classes' `+0x388` is the body.
            primary_facing: if class == FirerClass::Building {
                current(firer.barrel_facing, firer.facing)
            } else {
                current(firer.body_facing, firer.facing)
            },
            secondary_facing: current(firer.barrel_facing.or(firer.body_facing), firer.facing),
            navcom: firer.navigation.nav_com.is_some(),
            // 0.1 lies between SimFixed raw 6553 and 6554; integer division
            // selects the largest admissible representable fraction. Original
            // boundary witnesses: tools/spatial_oracle/infantry_fire_speed.json.
            moving_faster_than_tenth: firer.foot_speed.applied_fraction
                > crate::util::fixed_math::SimFixed::ONE
                    / crate::util::fixed_math::SimFixed::from_num(10),
            deploying: unit_deploying(firer),
            tethered: firer.dock_entered_with.is_some(),
            radio_link: match radio_link {
                None => RadioLink::None,
                Some(contact) if contact.category == EntityCategory::Structure => {
                    RadioLink::Building
                }
                Some(_) => RadioLink::Other,
            },
            // `+0x6AF` as it stood before this frame's `Facing_Update`: Unit AI
            // runs `Fire_At_Target @ 0x007365E1` first, and VERA commits the
            // new latch in `apply_unit_facing` after the fire phase.
            turret_rotation_latch: firer.turret_rotation_latch,
            sequence: firer
                .mission_leaf
                .as_infantry()
                .map_or(-1, |leaf| leaf.doing()),
            effective_mission: if firer.building_up.is_some() {
                0x12
            } else {
                firer.mission.effective().raw()
            },
            delayed_fire_counter: firer
                .pending_building_fire
                .map_or(0, |pending| pending.remaining_ticks),
            ..FirerFacts::default()
        };
        let firer_type = FirerTypeFacts {
            natural: obj.natural,
            pushy: obj.pushy,
            land_targeting: obj.land_targeting,
            mobile_fire: obj.mobile_fire,
            turret_count: obj.turret_count,
            turret: obj.has_turret,
            is_gattling: obj.is_gattling,
            hunter_seeker: obj.hunter_seeker,
            balloon_hover: obj.balloon_hover,
            jumpjet: obj.jumpjet,
            organic: obj.organic,
            jumpjet_turn: obj.jumpjet_turn,
            can_be_occupied: obj.can_be_occupied,
            can_occupy_fire: obj.can_occupy_fire,
            emp_pulse_cannon: obj.emp_pulse_cannon,
            turret_anim_is_voxel: obj.turret_anim_is_voxel,
            ..FirerTypeFacts::default()
        };
        let (target, target_type) = match self.target {
            None => (TargetFacts::default(), TargetTypeFacts::default()),
            Some(TargetKind::Cell(rx, ry)) => (
                TargetFacts {
                    kind: FireTargetKind::Cell,
                    land_type: self
                        .terrain()
                        .and_then(|terrain| terrain.cell(rx, ry))
                        .map_or(0, |cell| i32::from(cell.yr_cell_land_type)),
                    ..TargetFacts::default()
                },
                TargetTypeFacts::default(),
            ),
            Some(TargetKind::Entity(_)) => match (self.target_entity(), self.target_obj()) {
                (Some(entity), Some(target_obj)) => (
                    TargetFacts {
                        kind: match entity.category {
                            EntityCategory::Unit => FireTargetKind::Unit,
                            EntityCategory::Infantry => FireTargetKind::Infantry,
                            EntityCategory::Aircraft => FireTargetKind::Aircraft,
                            EntityCategory::Structure => FireTargetKind::Building,
                        },
                        bomb: entity.bomb.is_some(),
                        in_limbo: entity.lifecycle.in_limbo,
                        on_bridge: entity.on_bridge,
                        z: self
                            .terrain()
                            .and_then(|terrain| in_range::effective_z_leptons(entity, terrain))
                            .map_or(0, |z| z as i32),
                        mission: entity.mission.current().raw(),
                        iron_curtained: crate::sim::superweapon::invulnerability::is_invulnerable(
                            entity.invulnerability.as_ref(),
                            frame,
                        ),
                        draining_me: entity.draining_me.is_some(),
                        warped_out: entity.is_warped_out(),
                        bunkered: entity.bunker_link.installed_in().is_some(),
                        health: entity.health.current,
                        parasite_lock_until: entity.parasite_launch_lock as i32,
                        deploying: unit_deploying(entity),
                        ..TargetFacts::default()
                    },
                    TargetTypeFacts {
                        strength: target_obj.strength,
                        drainable: target_obj.drainable,
                        berserk_friendly: target_obj.berserk_friendly,
                        unnatural: target_obj.unnatural,
                        immune_to_psionics: target_obj.immune_to_psionics,
                        spawned: target_obj.spawned,
                        balloon_hover: target_obj.balloon_hover,
                        jumpjet: target_obj.jumpjet,
                        organic: target_obj.organic,
                        is_simple_deployer: target_obj.is_simple_deployer,
                        non_vehicle: target_obj.non_vehicle,
                        building_vehicle: false,
                    },
                ),
                // A target without a type is no techno the tests can read.
                _ => (TargetFacts::default(), TargetTypeFacts::default()),
            },
        };
        FireFacts {
            class,
            frame: frame as i32,
            weapon_index: self.weapon_index,
            firer: firer_facts,
            firer_type,
            target,
            target_type,
        }
    }

    /// The target's coordinates in leptons (a building's foundation center).
    fn target_point(
        &self,
    ) -> Option<(
        u16,
        u16,
        crate::util::fixed_math::SimFixed,
        crate::util::fixed_math::SimFixed,
    )> {
        match self.target? {
            TargetKind::Entity(_) => self
                .target_entity()
                .map(|target| super::target_coords(target, Some(self.rules), &self.world.interner)),
            TargetKind::Cell(rx, ry) => Some(super::cell_center_coords(rx, ry)),
        }
    }

    fn cell_facts(&self, rx: u16, ry: u16) -> Option<CellFacts> {
        self.terrain()
            .and_then(|terrain| terrain.cell(rx, ry))
            .map(|cell| CellFacts {
                land_type: i32::from(cell.yr_cell_land_type),
                flags: cell.bridge_facts.raw_flags,
            })
    }
}

/// A garrisoned building's GetWeapon (`0x004526F0` through `IsOccupied`
/// `0x00458DD0`): its firing occupant's OccupyWeapon, as the attacker
/// snapshot resolves it, with the garrison fire range (half the foundation
/// plus `OccupyWeaponRange`).
pub(crate) fn garrison_weapon<'r>(
    world: &Simulation,
    rules: &'r RuleSet,
    building: &GameEntity,
    obj: &ObjectType,
    target: TargetKind,
) -> Option<(&'r WeaponType, crate::util::fixed_math::SimFixed)> {
    if !obj.can_be_occupied || !obj.can_occupy_fire {
        return None;
    }
    let cargo = building
        .passenger_role
        .cargo()
        .filter(|cargo| !cargo.is_empty())?;
    let occupant = world
        .substrate
        .entities
        .get(cargo.passengers[cargo.garrison_fire_index as usize % cargo.count() as usize])?;
    let (category, armor) = match target {
        TargetKind::Entity(id) => {
            let target = world.substrate.entities.get(id)?;
            (
                super::combat_target_category(target, rules, &world.interner),
                rules
                    .object(world.interner.resolve(target.type_ref()))
                    .map_or("none", |target| target.armor.as_str()),
            )
        }
        // The fire path reads a cell target as a Structure of the firer's
        // own type.
        TargetKind::Cell(..) => (EntityCategory::Structure, obj.armor.as_str()),
    };
    let selected = combat_weapon::select_garrison_weapon(
        rules,
        world.interner.resolve(occupant.type_ref()),
        occupant.veterancy,
        category,
        armor,
    )?;
    let (width, height) = crate::sim::production::foundation_dimensions(&obj.foundation);
    let cells = i32::from(width.min(height) / 2) + rules.garrison_rules.occupy_weapon_range;
    Some((
        selected.weapon,
        crate::util::fixed_math::SimFixed::from_num(cells.max(1)),
    ))
}

/// The owners' answers, asked by [`super::fire_error::get_fire_error`].
struct WorldQuery<'s, 'a> {
    subject: &'s FireSubject<'a>,
}

impl FireQuery for WorldQuery<'_, '_> {
    fn weapon(&mut self, index: i32) -> Option<WeaponFacts> {
        let subject = self.subject;
        subject
            .weapon_at(index)
            .map(|weapon| subject.weapon_facts(weapon))
    }

    fn in_range(&mut self) -> bool {
        let subject = self.subject;
        let Some(target) = subject.target else {
            return false;
        };
        let Some(weapon) = subject.weapon_at(subject.weapon_index) else {
            return false;
        };
        let Some((rx, ry, sub_x, sub_y)) = subject.target_point() else {
            return false;
        };
        let firer = subject.firer;
        let flat = |range: crate::util::fixed_math::SimFixed| {
            super::is_within_range_leptons(
                super::lepton_distance_sq_raw(
                    firer.position.rx,
                    firer.position.ry,
                    firer.position.sub_x,
                    firer.position.sub_y,
                    rx,
                    ry,
                    sub_x,
                    sub_y,
                ),
                range,
            )
        };
        // RESIDUAL (line of fire): a garrison shot measures flat distance with
        // the garrison range and never runs InRange's wall/cliff walk; the
        // override-aware range chain (M8) is recorded at the old range gate.
        if let Some((_, range)) = subject.garrison {
            return flat(range);
        }
        match subject.terrain() {
            Some(terrain) => in_range::fire_source_coords(
                firer,
                &target,
                weapon,
                &subject.world.substrate.entities,
                terrain,
            )
            .is_some_and(|source| {
                in_range::compute_in_range(
                    firer,
                    source,
                    &target,
                    weapon,
                    subject.rules,
                    &subject.world.interner,
                    &subject.world.substrate.entities,
                    terrain,
                    &line_of_fire::LineOfFireInputs {
                        overlay_grid: subject.world.overlay_grid.as_ref(),
                        overlay_registry: subject.overlay_registry,
                        alliances: subject.fog.map(|fog| &fog.alliances),
                    },
                )
            }),
            None => flat(weapon.range),
        }
    }

    fn naval_selector(&mut self) -> i32 {
        let subject = self.subject;
        let facts = subject
            .target_entity()
            .zip(subject.target_obj())
            .map(|(entity, obj)| {
                combat_weapon::techno_target_facts(
                    entity,
                    obj,
                    subject.terrain(),
                    self.house_allied(subject.firer.owner(), entity.owner()),
                )
            });
        combat_weapon::select_naval_targeting_weapon(subject.obj, facts.as_ref())
    }

    /// `TechnoClass::GetVisualState @ 0x00703860` as T17 asks it (`0x006FC25B`:
    /// the sensor argument set, the firer's house). `+0x41A`, whose writer is
    /// unidentified, is held clear; the map editor never runs.
    fn visual_state(&mut self) -> i32 {
        let subject = self.subject;
        let (Some(target), Some(obj)) = (subject.target_entity(), subject.target_obj()) else {
            return 0;
        };
        let building = target.category == EntityCategory::Structure;
        // `+0xC9A`: `Invisible=`, which a BuildingType's `InvisibleInGame=`
        // also sets (`0x00460E09`).
        if obj.invisible || (building && obj.invisible_in_game) {
            return 5;
        }
        let Some(cloak) = target.cloak.as_ref() else {
            return 0;
        };
        if cloak.state == 0 || building {
            return 0;
        }
        if cloak.state == 2 {
            // The firer's house senses the target's Location cell.
            let sensed = subject.fog.is_some_and(|fog| {
                fog.has_sensor_for_house(
                    subject.firer.owner(),
                    target.position.rx,
                    target.position.ry,
                )
            });
            return if sensed { 3 } else { 5 };
        }
        i32::from(cloak.transition_visual_state())
    }

    fn high_flying(&mut self) -> bool {
        // CellClass vt+0x54 (`0x00410530`) is constant false.
        self.subject
            .target_entity()
            .is_some_and(combat_weapon::target_is_high_flying)
    }

    fn low_flying(&mut self) -> bool {
        self.subject
            .target_entity()
            .is_some_and(|target| !combat_weapon::target_is_high_flying(target))
    }

    fn firer_high_flying(&mut self) -> bool {
        combat_weapon::target_is_high_flying(self.subject.firer)
    }

    fn target_layer(&mut self) -> i32 {
        2
    }

    fn target_cell(&mut self) -> Option<CellFacts> {
        let target = self.subject.target_entity()?;
        self.subject
            .cell_facts(target.position.rx, target.position.ry)
    }

    fn firer_cell(&mut self) -> FirerCell {
        let firer = self.subject.firer;
        let own = (firer.position.rx, firer.position.ry);
        if self.subject.target == Some(TargetKind::Cell(own.0, own.1)) {
            return FirerCell::Target;
        }
        self.subject
            .cell_facts(own.0, own.1)
            .map_or(FirerCell::None, FirerCell::Cell)
    }

    fn sensor(&mut self) -> bool {
        let subject = self.subject;
        let (Some(fog), Some((rx, ry, _, _))) = (subject.fog, subject.target_point()) else {
            return false;
        };
        fog.has_sensor_for_house(subject.firer.owner(), rx, ry)
    }

    fn allied(&mut self) -> bool {
        // T17 `0x006FC289..0x006FC296`: the target's house asks.
        let subject = self.subject;
        subject
            .target_entity()
            .is_some_and(|target| self.house_allied(target.owner(), subject.firer.owner()))
    }

    fn bridge_for_firing(&mut self) -> bool {
        self.subject
            .terrain()
            .is_some_and(|terrain| in_range::is_on_bridge_for_firing(self.subject.firer, terrain))
    }

    fn can_infect(&mut self) -> bool {
        let subject = self.subject;
        // CanInfect(NULL) refuses a target that is not a Foot.
        subject
            .target_entity()
            .zip(subject.target_obj())
            .is_some_and(|(target, target_obj)| {
                combat_weapon::ParasiteVictimFacts::of(target, target_obj, subject.terrain())
                    .admits(subject.obj.naval)
            })
    }

    fn can_capture(&mut self) -> bool {
        let subject = self.subject;
        match subject.target {
            Some(TargetKind::Entity(id)) => {
                subject
                    .world
                    .can_capture(subject.firer.stable_id(), id, subject.rules)
            }
            _ => false,
        }
    }

    fn deploy_cell_ok(&mut self) -> bool {
        true
    }

    fn locomotor_moving(&mut self) -> bool {
        crate::sim::movement::ready_producer::is_moving_now_for(
            self.subject.firer,
            self.subject.frame(),
        )
    }

    fn target_locomotor_moving(&mut self) -> bool {
        self.subject.target_entity().is_some_and(|target| {
            crate::sim::movement::ready_producer::is_moving_now_for(target, self.subject.frame())
        })
    }

    fn locomotor_can_fire(&mut self) -> FireError {
        FireError::Ok
    }

    fn jumpjet_locomotor(&mut self) -> bool {
        self.subject
            .firer
            .locomotor
            .as_ref()
            .is_some_and(|locomotor| {
                locomotor.kind == crate::rules::locomotor_type::LocomotorKind::Jumpjet
            })
    }

    fn fighter(&mut self) -> bool {
        self.subject.obj.fighter
    }

    fn direction_to_target(&mut self) -> u16 {
        self.direction()
    }

    fn turret_direction(&mut self) -> u16 {
        self.direction()
    }

    fn occupants(&mut self) -> i32 {
        self.subject
            .firer
            .passenger_role
            .cargo()
            .map_or(0, |cargo| cargo.count() as i32)
    }

    fn operational(&mut self) -> bool {
        self.subject
            .world
            .building_operational_state(self.subject.firer.stable_id(), self.subject.rules)
            .unwrap_or(true)
    }
}

impl WorldQuery<'_, '_> {
    /// `HouseClass::IsAlliedWith @ 0x004F9A50`: `asker`'s own ally bits.
    fn house_allied(
        &self,
        asker: crate::sim::intern::InternedId,
        other: crate::sim::intern::InternedId,
    ) -> bool {
        combat_weapon::is_ally_by_object(
            self.subject.fog.map(|fog| &fog.alliances),
            &self.subject.world.interner,
            asker,
            other,
        )
    }

    /// DirectionToTarget from the firer's position to the target's point.
    fn direction(&self) -> u16 {
        let subject = self.subject;
        let firer = subject.firer;
        subject.target_point().map_or(0, |(rx, ry, sub_x, sub_y)| {
            crate::sim::movement::turret::facing_toward_lepton(
                firer.position.rx,
                firer.position.ry,
                firer.position.sub_x,
                firer.position.sub_y,
                rx,
                ry,
                sub_x,
                sub_y,
            )
        })
    }
}

/// `0x00746DB0`: a Unit deploying or undeploying (`+0x6E1`/`+0x6E2`).
fn unit_deploying(entity: &GameEntity) -> bool {
    entity.category == EntityCategory::Unit
        && matches!(
            entity.deploy_state,
            Some(
                crate::sim::deploy::DeployPhase::Deploying { .. }
                    | crate::sim::deploy::DeployPhase::Undeploying { .. }
            )
        )
}

#[cfg(test)]
#[path = "fire_error_world_tests.rs"]
mod tests;
