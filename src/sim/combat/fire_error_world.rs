//! The world's side of GetFireError: [`FireFacts`] read from live objects and
//! a [`FireQuery`] answered by each question's owner.
//!
//! RESIDUAL questions VERA cannot answer natively, each fixed to the value its
//! missing producer would leave (trigger, effect and frequency are on the
//! owning row in `fire_error.rs`):
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
use crate::map::cell_index::NativeCellIdentity;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainGrid};
use crate::rules::object_type::ObjectType;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::weapon_type::WeaponType;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::ground_pose::{
    object_center_xy, position_world_coord, query_object_cell,
};
use crate::sim::terrain_object::TerrainObjectState;
use crate::sim::touch_log::live_read_check_enabled;
use crate::sim::vision::FireFog;
use crate::sim::world::Simulation;

/// One firer, target and weapon slot, as GetFireError sees them.
pub(crate) struct FireSubject<'a> {
    pub world: &'a Simulation,
    pub rules: &'a RuleSet,
    pub overlay_registry: Option<&'a OverlayTypeRegistry>,
    pub fog: Option<FireFog<'a>>,
    pub firer: &'a GameEntity,
    pub obj: &'a ObjectType,
    pub target: Option<TargetKind>,
    pub weapon_index: i32,
}

/// The firer's half of [`FireFacts`] ([`FireSubject::firer_part`]): what
/// GetFireError reads of the firer and its type, with the ids of the links
/// it compares with the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FirerPart {
    class: FirerClass,
    frame: i32,
    /// Every link to the target unset: [`Self::facts`] sets them.
    firer: FirerFacts,
    firer_type: FirerTypeFacts,
    /// The transport the firer rides, when it resolves.
    transport: Option<u64>,
    drain_target: Option<u64>,
    locomotor_target: Option<u64>,
    /// The warp target, when a temporal link is held.
    temporal_target: Option<Option<u64>>,
}

impl FirerPart {
    /// The facts against `target`, with the target's half.
    fn facts(
        &self,
        target: Option<TargetKind>,
        weapon_index: i32,
        target_facts: TargetFacts,
        target_type: TargetTypeFacts,
    ) -> FireFacts {
        let target_id = match target {
            Some(TargetKind::Entity(id)) => Some(id),
            _ => None,
        };
        let link = |to: Option<u64>| match to {
            None => Link::None,
            Some(id) if Some(id) == target_id => Link::Target,
            Some(_) => Link::Other,
        };
        let mut firer = self.firer;
        if self.transport.is_some() && self.transport == target_id {
            firer.transporter = Transporter::Target;
        }
        firer.drain_target = link(self.drain_target);
        firer.locomotor_target = link(self.locomotor_target);
        firer.temporal = self.temporal_target.map(link);
        FireFacts {
            class: self.class,
            frame: self.frame,
            weapon_index,
            firer,
            firer_type: self.firer_type,
            target: target_facts,
            target_type,
        }
    }
}

/// A subject's target object and its type ([`FireSubject::resolve_target`]).
#[derive(Clone, Copy)]
struct ResolvedTarget<'a> {
    entity: Option<&'a GameEntity>,
    obj: Option<&'a ObjectType>,
    terrain: Option<&'a TerrainObjectState>,
}

impl<'a> FireSubject<'a> {
    /// GetFireError for this subject.
    pub(crate) fn fire_error(&self, check_range: bool) -> FireError {
        self.fire_error_with(&self.firer_part(), check_range)
    }

    /// [`Self::fire_error`] with the firer's half of the facts already read
    /// ([`Self::firer_part`]): a target scan reads it once for all its
    /// candidates, since nothing a scan does changes the firer.
    pub(crate) fn fire_error_with(&self, firer: &FirerPart, check_range: bool) -> FireError {
        debug_assert!(
            !live_read_check_enabled() || *firer == self.firer_part(),
            "the firer part of {} went stale",
            self.firer.stable_id()
        );
        let mut query = WorldQuery::new(self);
        let facts = self.facts(firer, query.target);
        super::fire_error::get_fire_error(&facts, &mut query, check_range)
    }

    /// `CanFireAt @ 0x006F77B0`: InRange (vt+0x3A8) with the subject's weapon.
    pub(crate) fn in_range(&self) -> bool {
        WorldQuery::new(self).in_range()
    }

    /// `GetWeaponDamageValue(-1) @ 0x006F3970` through this subject's
    /// GetWeapon, so an occupied building reads its occupant's weapon.
    pub(crate) fn weapon_damage_value(&self) -> i32 {
        let mut query = WorldQuery::new(self);
        let facts = self.facts(&self.firer_part(), query.target);
        super::fire_error::weapon_value(&facts, &mut query)
    }

    /// The target object and its type, resolved once per question.
    fn resolve_target(&self) -> ResolvedTarget<'a> {
        let entity = match self.target {
            Some(TargetKind::Entity(id)) => self.world.substrate.entities.get(id),
            _ => None,
        };
        let terrain = match self.target {
            Some(TargetKind::Entity(id)) if entity.is_none() => {
                self.world.production.terrain_objects.get(&id)
            }
            _ => None,
        };
        ResolvedTarget {
            entity,
            terrain,
            obj: entity.and_then(|target| {
                self.world
                    .interner
                    .object_type(target.type_ref(), self.rules)
            }),
        }
    }

    fn terrain(&self) -> Option<&ResolvedTerrainGrid> {
        self.world.resolved_terrain.as_ref()
    }

    fn frame(&self) -> u32 {
        self.world.session.binary_frame()
    }

    fn speed_rules(&self) -> crate::sim::movement::SpeedRules<'_> {
        crate::sim::movement::SpeedRules::new(self.rules, &self.world.interner, &self.world.houses)
    }

    /// GetWeapon (vt+0x3F8) at an index ([`combat_weapon::get_weapon`]): an
    /// occupied building's firing occupant's weapon for every slot, else the
    /// rank-selected slot.
    pub(crate) fn weapon_at(&self, index: i32) -> Option<&'a WeaponType> {
        combat_weapon::get_weapon(
            self.firer,
            self.obj,
            index,
            &self.world.substrate.entities,
            self.rules,
            &self.world.interner,
        )
    }

    fn weapon_facts(&self, weapon: &WeaponType, target_obj: Option<&ObjectType>) -> WeaponFacts {
        let warhead = combat_weapon::warhead_of(self.rules, weapon);
        let projectile = weapon
            .projectile
            .as_deref()
            .and_then(|id| self.rules.projectile(id));
        let armor = target_obj.map(|target| super::armor_index(&target.armor));
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

    /// The firer's half of the facts, which reads nothing of the target: the
    /// links GetFireError compares with the target keep their ids.
    pub(crate) fn firer_part(&self) -> FirerPart {
        let firer = self.firer;
        let obj = self.obj;
        let frame = self.frame();
        let class = match firer.category() {
            EntityCategory::Unit => FirerClass::Unit,
            EntityCategory::Infantry => FirerClass::Infantry,
            EntityCategory::Aircraft => FirerClass::Aircraft,
            EntityCategory::Structure => FirerClass::Building,
        };
        let transport = firer
            .passenger_role
            .inside_transport_id()
            .and_then(|id| self.world.substrate.entities.get(id));
        // U5 reads the `+0x418` tether byte that radio `0x18` sets, whose
        // stand-in is the dock-entered projection (VERA owns no exact tether
        // byte, `radio/receive.rs`), and `RadioLinks[0]`.
        let radio_link = firer
            .radio_contacts
            .slot(0)
            .and_then(|id| self.world.substrate.entities.get(id));
        let firer_facts = FirerFacts {
            enslaved: firer.slave.owner().is_some(),
            warped_out: firer.is_warped_out(),
            warping_in: firer.is_warping_in(),
            robot_offline: firer.deactivated.is_offline(),
            on_bridge: firer.on_bridge(),
            z: self.terrain().map_or(0, |terrain| {
                crate::sim::movement::ground_pose::object_world_z_leptons(firer, Some(terrain))
            }),
            berserk: firer.berserk.is_active(),
            falling: firer.is_falling_down(),
            sinking: firer.sinking.is_active(),
            in_open_transport: firer.passenger_role.in_open_transport(),
            // The transport being the target (T9) is [`FirerPart::facts`]'s.
            transporter: match transport {
                None => Transporter::None,
                Some(transport) if transport.is_warped_out() => Transporter::WarpedOut,
                Some(transport) if transport.passenger_role.is_inside_transport() => {
                    Transporter::Nested
                }
                Some(_) => Transporter::Plain,
            },
            wave_live: self
                .world
                .active_wave_links
                .contains_key(&firer.stable_id()),
            // FireError6FC6D5/6FC9C7 reads the actual Techno+308 pointer.
            spark_particles_live: firer.spark_particle_system.system().is_some(),
            // T45: the object's own rearm countdown (`+0x2EC`).
            rearming: firer
                .rearm_timer
                .remaining(self.world.session.binary_frame() as i32)
                != 0,
            ammo: firer.ammo().map_or(-1, |ammo| ammo.current()),
            cloak_state: firer.cloak.as_ref().map_or(0, |cloak| cloak.state()),
            current_weapon: combat_weapon::attacker_facts(firer, obj).current_weapon_number,
            draining_me: firer.drain.draining_me().is_some(),
            // [`FirerPart::facts`] links these to the target.
            drain_target: Link::None,
            locomotor_target: Link::None,
            temporal: None,
            owner_is_human: crate::sim::house_state::house_state_for_owner_id(
                &self.world.houses,
                firer.owner(),
            )
            .is_some_and(|house| house.is_human),
            paralyzed: firer.parasite.is_paralyzed(frame),
            spawns: firer.spawn_manager.as_ref().map(|manager| SpawnCounts {
                not_regenerating: manager.count_alive_spawns() as i32,
                launched_missiles: manager.count_launched_missiles(
                    &self.world.substrate.entities,
                    self.rules,
                    &self.world.interner,
                ) as i32,
            }),
            // `+0x388`: the body, which a Building also turns as its turret.
            primary_facing: firer.body_facing_current(frame),
            secondary_facing: firer
                .barrel_facing
                .map_or(firer.body_facing_current(frame), |facing| {
                    facing.current(frame)
                }),
            magnetron_lifted: firer.lift.swap_active(),
            navcom: firer.navigation.nav_com().is_some(),
            moving_faster_than_tenth: firer.foot_speed.above_tenth(),
            deploying: firer.unit_deploying(),
            tethered: firer.tether.partner().is_some(),
            radio_link: match radio_link {
                None => RadioLink::None,
                Some(contact) if contact.category() == EntityCategory::Structure => {
                    RadioLink::Building
                }
                Some(_) => RadioLink::Other,
            },
            // `+0x6AF` as it stood before this frame's `Facing_Update`: Unit AI
            // runs `Fire_At_Target @ 0x007365E1` first, and VERA commits the
            // new latch in `apply_unit_facing` after the fire phase.
            turret_rotation_latch: firer.turret_rotation_latch.is_set(),
            sequence: firer
                .mission_leaf
                .as_infantry()
                .map_or(-1, |leaf| leaf.doing()),
            effective_mission: firer.mission.effective().raw(),
            delayed_fire_counter: firer.pending_building_fire.remaining_ticks(),
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
        FirerPart {
            class,
            frame: frame as i32,
            firer: firer_facts,
            firer_type,
            transport: transport.map(GameEntity::stable_id),
            drain_target: firer.drain.target(),
            locomotor_target: firer.lift.target(),
            temporal_target: firer
                .temporal
                .has_link()
                .then(|| firer.temporal.warp_target()),
        }
    }

    /// This subject's facts, both halves read now.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn whole_facts(&self) -> FireFacts {
        self.facts(&self.firer_part(), self.resolve_target())
    }

    /// GetFireError's facts: the firer's half and the resolved target's.
    fn facts(&self, firer: &FirerPart, resolved: ResolvedTarget<'_>) -> FireFacts {
        let frame = self.frame();
        let (target, target_type) = match self.target {
            None => (TargetFacts::default(), TargetTypeFacts::default()),
            Some(TargetKind::Cell(..)) => (
                TargetFacts {
                    kind: FireTargetKind::Cell,
                    land_type: self
                        .terrain()
                        .zip(self.cell_target_identity())
                        .map_or(0, |(terrain, cell)| {
                            NativeCellQuery::canonical(terrain).land_type(cell)
                        }),
                    ..TargetFacts::default()
                },
                TargetTypeFacts::default(),
            ),
            Some(TargetKind::Entity(_)) => match (resolved.entity, resolved.obj) {
                (Some(entity), Some(target_obj)) => (
                    TargetFacts {
                        kind: match entity.category() {
                            EntityCategory::Unit => FireTargetKind::Unit,
                            EntityCategory::Infantry => FireTargetKind::Infantry,
                            EntityCategory::Aircraft => FireTargetKind::Aircraft,
                            EntityCategory::Structure => FireTargetKind::Building,
                        },
                        bomb: entity.bomb.is_some(),
                        in_limbo: entity.lifecycle.in_limbo(),
                        sinking: entity.sinking.is_active(),
                        on_bridge: entity.on_bridge(),
                        z: self.terrain().map_or(0, |terrain| {
                            crate::sim::movement::ground_pose::object_world_z_leptons(
                                entity,
                                Some(terrain),
                            )
                        }),
                        mission: entity.mission.current().raw(),
                        iron_curtained: crate::sim::superweapon::invulnerability::is_invulnerable(
                            entity.invulnerability.as_ref(),
                            frame,
                        ),
                        draining_me: entity.drain.draining_me().is_some(),
                        warped_out: entity.is_warped_out(),
                        chrono_warp_latch: entity.chrono_warp_latch(),
                        bunkered: entity.bunker.link().installed_in().is_some(),
                        health: entity.health,
                        parasite_lock_until: entity.parasite.launch_lock() as i32,
                        deploying: entity.unit_deploying(),
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
                        // Building vt+80:457620 ->465D40, shared ART/undeploy
                        // predicate also used by the Unit repair action.
                        building_vehicle: target_obj.is_1x1_with_undeploy(),
                    },
                ),
                // GetFireError6FC177 preserves the original Object receiver
                // but derives EBP only from Techno bit0. Terrain enters the
                // common coordinate/range path and skips Techno-only facts.
                _ if resolved.terrain.is_some() => {
                    let terrain = resolved.terrain.unwrap();
                    (
                        TargetFacts {
                            kind: FireTargetKind::Object,
                            in_limbo: !terrain.is_live(),
                            z: terrain.world_coord().z,
                            health: super::Health::new(terrain.health),
                            ..TargetFacts::default()
                        },
                        TargetTypeFacts::default(),
                    )
                }
                _ => (TargetFacts::default(), TargetTypeFacts::default()),
            },
        };
        firer.facts(self.target, self.weapon_index, target, target_type)
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
        super::resolve_target_coords(&self.target?, self.world)
    }

    /// Native receives an already resolved Cell pointer. TargetKind stores the
    /// coordinate used to resolve it; recover allocation identity without an
    /// extra lookup before GetFireError's first native map query at 0x006FC197.
    fn cell_target_identity(&self) -> Option<NativeCellIdentity> {
        self.target?.cell_identity(self.terrain()?)
    }
}

/// The owners' answers, asked by [`super::fire_error::get_fire_error`].
struct WorldQuery<'s, 'a> {
    subject: &'s FireSubject<'a>,
    cells: Option<NativeCellQuery<'s>>,
    /// EDI at 0x006FC19C, retained until the sensor call at 0x006FC26F. A later
    /// lookup may stamp this same Dummy; never resolve the target again.
    target_center_cell: Option<NativeCellIdentity>,
    /// The subject's target, resolved once for all the questions.
    target: ResolvedTarget<'a>,
}

impl<'s, 'a> WorldQuery<'s, 'a> {
    fn new(subject: &'s FireSubject<'a>) -> Self {
        Self {
            subject,
            cells: subject.terrain().map(NativeCellQuery::canonical),
            target_center_cell: None,
            target: subject.resolve_target(),
        }
    }
}

impl FireQuery for WorldQuery<'_, '_> {
    fn retain_target_center_cell(&mut self) {
        let Some(cells) = self.cells.as_ref() else {
            return;
        };
        let xy = match self.subject.target {
            Some(TargetKind::Entity(_)) => {
                if let Some(target) = self.target.entity {
                    let [x, y] = object_center_xy(target);
                    (x, y)
                } else if let Some(target) = self.target.terrain {
                    let point = target.world_coord();
                    (point.x, point.y)
                } else {
                    return;
                }
            }
            Some(TargetKind::Cell(..)) => {
                let Some(cell) = self.subject.cell_target_identity() else {
                    return;
                };
                // Cell 0x00486840 reads the receiver's *current* packed coordinate;
                // a retained Dummy is not the originally requested coordinate.
                let [x, y] = crate::sim::cell_kernel::native_cell_own_xy(cell, cells);
                (x, y)
            }
            None => return,
        };
        self.target_center_cell = Some(cells.lookup_world(xy.0, xy.1));
    }

    fn weapon(&mut self, index: i32) -> Option<WeaponFacts> {
        let subject = self.subject;
        subject
            .weapon_at(index)
            .map(|weapon| subject.weapon_facts(weapon, self.target.obj))
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
        match subject.terrain() {
            // GetFireError6FCCEE calls InRange with its original target. A Cell
            // retains its Cell+48/+50 geometry through the existing range
            // owner; it must not acquire Techno low-flying source semantics.
            Some(terrain) if matches!(target, TargetKind::Cell(..)) => {
                let Some(cell) = subject.cell_target_identity() else {
                    return false;
                };
                in_range::cell_target_in_range(
                    firer,
                    cell,
                    weapon,
                    subject.rules,
                    &subject.world.interner,
                    terrain,
                    &line_of_fire::LineOfFireInputs {
                        overlay_grid: subject.world.overlay_grid.as_ref(),
                        overlay_registry: subject.overlay_registry,
                        alliances: subject.fog.map(|fog| fog.alliances),
                    },
                )
                .unwrap_or(false)
            }
            Some(terrain) => in_range::fire_source_coords(
                firer,
                &target,
                weapon,
                subject.world,
                terrain,
                (subject.rules, &subject.world.interner),
            )
            .is_some_and(|source| {
                in_range::compute_in_range(
                    firer,
                    source,
                    &target,
                    weapon,
                    subject.rules,
                    &subject.world.interner,
                    subject.world,
                    terrain,
                    &line_of_fire::LineOfFireInputs {
                        overlay_grid: subject.world.overlay_grid.as_ref(),
                        overlay_registry: subject.overlay_registry,
                        alliances: subject.fog.map(|fog| fog.alliances),
                    },
                )
            }),
            None => super::is_within_range_leptons(
                super::lepton_distance_sq_raw(
                    firer.position.rx(),
                    firer.position.ry(),
                    firer.position.sub_x(),
                    firer.position.sub_y(),
                    rx,
                    ry,
                    sub_x,
                    sub_y,
                ),
                weapon.range_leptons,
            ),
        }
    }

    fn naval_selector(&mut self) -> i32 {
        let subject = self.subject;
        let facts = self
            .target
            .entity
            .zip(self.target.obj)
            .map(|(entity, obj)| {
                combat_weapon::techno_target_facts(
                    entity,
                    obj,
                    subject.terrain(),
                    self.house_allied(subject.firer.owner(), entity.owner()),
                    subject.rules,
                    &subject.world.interner,
                )
            })
            .or_else(|| {
                self.target
                    .terrain
                    .map(|_| combat_weapon::TargetFacts::Terrain)
            });
        combat_weapon::select_naval_targeting_weapon(subject.obj, facts.as_ref())
    }

    /// T17 calls the shared703860 owner with `(1, firerHouse)` at6FC25B.
    /// This explicit sensor query deliberately has no alliance/owned shortcut.
    fn visual_state(&mut self) -> i32 {
        let subject = self.subject;
        let (Some(target), Some(obj)) = (self.target.entity, self.target.obj) else {
            return 0;
        };
        let invisible = obj.invisible
            || target.category() == EntityCategory::Structure && obj.invisible_in_game;
        let sensed = subject.fog.is_some_and(|fog| {
            fog.has_sensor_for_house(
                subject.firer.owner(),
                target.position.rx(),
                target.position.ry(),
            )
        });
        i32::from(target.visual_character(
            subject.rules.general.cloaking_stages,
            invisible,
            crate::sim::cloak_disguise::VisualCharacterQuery::sensor(true, sensed, false),
        ))
    }

    fn high_flying(&mut self) -> bool {
        // CellClass vt+0x54 (`0x00410530`) is constant false.
        if let Some(target) = self.target.entity {
            crate::sim::movement::air_movement::is_high_flying(
                target,
                self.subject.terrain(),
                Some((self.subject.rules, &self.subject.world.interner)),
            )
        } else {
            self.target
                .terrain
                .is_some_and(|target| target.is_high_flying_in_query(self.cells.as_ref()))
        }
    }

    fn low_flying(&mut self) -> bool {
        if let Some(target) = self.target.entity {
            crate::sim::movement::air_movement::is_low_flying(
                target,
                self.subject.terrain(),
                Some((self.subject.rules, &self.subject.world.interner)),
            )
        } else {
            self.target
                .terrain
                .is_some_and(|target| target.is_low_flying_in_query(self.cells.as_ref()))
        }
    }

    fn firer_high_flying(&mut self) -> bool {
        crate::sim::movement::air_movement::is_high_flying(
            self.subject.firer,
            self.subject.terrain(),
            Some((self.subject.rules, &self.subject.world.interner)),
        )
    }

    /// T39 asks only a Foot target: its vt+0x78 ([`entity_layer`]).
    ///
    /// [`entity_layer`]: crate::sim::world::entity_layer
    fn target_layer(&mut self) -> i32 {
        let Some(target) = self.target.entity else {
            return i32::from(crate::sim::world::display_layers::DisplayLayer::GROUND.index());
        };
        i32::from(
            crate::sim::world::entity_layer(
                target,
                self.subject.terrain(),
                Some((self.subject.rules, &self.subject.world.interner)),
            )
            .index(),
        )
    }

    fn target_cell(&mut self) -> Option<CellFacts> {
        let cells = self.cells.as_ref()?;
        let point = if let Some(target) = self.target.entity {
            position_world_coord(&target.position)
        } else {
            let point = self.target.terrain?.world_coord();
            crate::sim::components::DriveCoord {
                x: point.x,
                y: point.y,
                z: point.z,
            }
        };
        let cell = query_object_cell(cells, point);
        Some(CellFacts {
            land_type: cells.land_type(cell),
            flags: cells.flags(cell),
        })
    }

    fn firer_cell(&mut self) -> FirerCell {
        let Some(cells) = self.cells.as_ref() else {
            return FirerCell::None;
        };
        let own = query_object_cell(cells, position_world_coord(&self.subject.firer.position));
        if self
            .subject
            .cell_target_identity()
            .is_some_and(|target| own == target)
        {
            return FirerCell::Target;
        }
        FirerCell::Cell(CellFacts {
            land_type: cells.land_type(own),
            flags: cells.flags(own),
        })
    }

    fn sensor(&mut self) -> bool {
        let subject = self.subject;
        let Some(fog) = subject.fog else {
            return false;
        };
        let (rx, ry) = match (subject.terrain(), self.target_center_cell) {
            (Some(terrain), Some(cell @ NativeCellIdentity::Real(_))) => {
                let (x, y) = terrain.native_cell_coord(cell);
                (x as u16, y as u16)
            }
            // Dummy sensor counters have no represented producer. Do not ask
            // a real cell merely because a later lookup stamped its coordinate.
            (Some(_), _) => return false,
            // Legacy headless fixtures have no native map allocation domain.
            (None, _) => {
                let Some((rx, ry, _, _)) = subject.target_point() else {
                    return false;
                };
                (rx, ry)
            }
        };
        fog.has_sensor_for_house(subject.firer.owner(), rx, ry)
    }

    fn allied(&mut self) -> bool {
        // T17 `0x006FC289..0x006FC296`: the target's house asks.
        let subject = self.subject;
        self.target
            .entity
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
        self.target
            .entity
            .zip(self.target.obj)
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
        // Unit suffix 0x00741078 resolves physical XYZ on every admitted base
        // result, before testing DeployToFire. Preserve its Dummy stamp even
        // though Cell 0x00487C10's deployment verdict remains a dormant residual.
        if let Some(cells) = self.cells.as_ref() {
            let point = position_world_coord(&self.subject.firer.position);
            cells.lookup_world(point.x, point.y);
        }
        true
    }

    fn locomotor_moving(&mut self) -> bool {
        crate::sim::movement::motion_query::is_moving_now(
            self.subject.firer,
            Some(self.subject.speed_rules()),
            self.subject.frame(),
        )
    }

    fn target_locomotor_moving(&mut self) -> bool {
        self.target.entity.is_some_and(|target| {
            crate::sim::movement::motion_query::is_moving_now(
                target,
                Some(self.subject.speed_rules()),
                self.subject.frame(),
            )
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
                locomotor.active_kind() == crate::rules::locomotor_type::LocomotorKind::Jumpjet
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

    /// Building vt+0x408 (`0x004581F0`), the occupant count that
    /// `BuildingClass::GetFireError` (`0x00447F10`) reads.
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
            self.subject.fog.map(|fog| fog.alliances),
            &self.subject.world.interner,
            asker,
            other,
        )
    }

    /// DirectionToTarget from the firer's position to the target's point; a
    /// building's is vt+0x4E8 ([`super::fire_coord::building_direction_to`]).
    /// The base is `ObjectClass::DirectionToTarget` (`0x005F3DB0`).
    fn direction(&self) -> u16 {
        let subject = self.subject;
        let firer = subject.firer;
        if firer.category() == EntityCategory::Structure {
            return subject.target.map_or(0, |target| {
                super::fire_coord::building_direction_to(
                    subject.world,
                    subject.rules,
                    firer,
                    target,
                )
                .unwrap_or(0)
            });
        }
        subject.target_point().map_or(0, |(rx, ry, sub_x, sub_y)| {
            crate::sim::movement::turret::facing_toward_lepton(
                firer.position.rx(),
                firer.position.ry(),
                firer.position.sub_x(),
                firer.position.sub_y(),
                rx,
                ry,
                sub_x,
                sub_y,
            )
        })
    }
}

#[cfg(test)]
#[path = "fire_error_world_tests.rs"]
mod tests;

#[cfg(test)]
mod terrain_target_tests {
    use super::*;

    #[test]
    fn retail_tree_uses_object_weapon_and_fire_range_without_techno_facts() {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = &retail.rules;
        let mut world = crate::sim::arena_fixture::rival_arena(rules, 31);
        let firer_id = world
            .spawn_object("MTNK", "Americans", 8, 10, 0, rules)
            .expect("retail MTNK");
        assert_eq!(
            crate::sim::terrain_spawn::construct_terrain_objects(
                &mut world,
                &[crate::map::overlay::TerrainObject {
                    rx: 10,
                    ry: 10,
                    name: "TREE01".into()
                }],
                rules,
                false,
            ),
            1
        );
        let tree_id = world.production.terrain_object_cells[&(10, 10)];
        let tree = &world.production.terrain_objects[&tree_id];
        let target = TargetKind::Entity(tree_id);
        let firer = world.substrate.entities.get(firer_id).unwrap();
        let obj = world.interner.object_type(firer.type_ref(), rules).unwrap();
        let before_rng = world.rng_state();
        let (index, weapon) = combat_weapon::select_weapon_against(
            rules,
            firer,
            obj,
            &combat_weapon::attacker_facts(firer, obj),
            Some(&target),
            &world,
            &world.interner,
            world.resolved_terrain.as_ref(),
            Some(&world.house_alliances),
        )
        .expect("a live Terrain Object is a selection target");
        assert_eq!(weapon.unwrap().weapon_id, "105mm");
        let subject = FireSubject {
            world: &world,
            rules,
            overlay_registry: None,
            fog: None,
            firer,
            obj,
            target: Some(target),
            weapon_index: index,
        };
        let facts = subject.whole_facts();
        assert_eq!(facts.target.kind, FireTargetKind::Object);
        assert_eq!(facts.target.health.current(), tree.health);
        assert_eq!(
            subject.target_point(),
            super::super::resolve_target_coords(&target, &world)
        );
        assert!(
            subject.in_range(),
            "the original Object coordinate reaches the shared cannon range gate"
        );
        assert_ne!(subject.fire_error(false), FireError::Illegal);
        assert_eq!(world.rng_state(), before_rng);
    }
}
