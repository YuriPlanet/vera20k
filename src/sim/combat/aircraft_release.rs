//! Aircraft Mission_Attack's strike states 4..9 (`0x00417FE0`), run by the
//! receiver where VERA's FireAt lives, inside the aircraft's own AI visit:
//! the host [`aircraft::attack_mission`]'s [`strike_visit`] asks for
//! GetFireError, IsClose, the facings, the state-4 burst and the single shots
//! of states 5..9. Each shot is
//! [`aircraft::fire_at`], whose TechnoClass::FireAt is the receiver's
//! emission.
//!
//! [`aircraft::attack_mission`]: crate::sim::aircraft::attack_mission
//! [`aircraft::fire_at`]: crate::sim::aircraft::fire_at

use super::*;
use crate::sim::aircraft::attack_mission::{StrikeFacts, StrikeHost, strike_visit};
use crate::sim::aircraft::fire_at::{AircraftShot, FireAtHost};
use crate::sim::projectile::ProjectileVelocity;

#[cfg(test)]
#[path = "aircraft_release_tests.rs"]
mod tests;

/// Re-read Target, selection facts and weapon tier after each synchronous shot.
/// A detached/null target still participates in SelectWeapon and the loop bound,
/// but Techno FireAt6FDDAE returns without emitting or rearming.
fn live_shot<'r>(
    world: &Simulation,
    rules: &'r RuleSet,
    id: u64,
) -> Option<(i32, Option<AdmittedFire<'r>>)> {
    let entity = world.substrate.entities.get(id)?;
    let obj = world.interner.object_type(entity.type_ref(), rules)?;
    let target = entity.attack_target.target();
    let (_, selected) = combat_weapon::select_weapon_against(
        rules,
        entity,
        obj,
        &combat_weapon::attacker_facts(entity, obj),
        target.as_ref(),
        world,
        &world.interner,
        world.resolved_terrain.as_ref(),
        Some(world.house_alliances.as_map()),
    )?;
    let selected = selected?;
    let burst = selected.weapon.burst;
    let coordinates = match target {
        Some(TargetKind::Entity(id)) => world
            .substrate
            .entities
            .get(id)
            .map(|target| (target_coords(target), target.type_ref()))
            .or_else(|| {
                let terrain = world.terrain_objects.objects().get(&id)?;
                Some((
                    resolve_target_coords(&TargetKind::Entity(id), world)?,
                    terrain.type_ref(),
                ))
            }),
        Some(TargetKind::Cell(rx, ry)) => Some((cell_center_coords(rx, ry), entity.type_ref())),
        None => None,
    };
    let shot = coordinates.map(|(target_coords, target_type_ref)| AdmittedFire {
        snap: build_attacker_snapshot(entity, target.unwrap(), None),
        obj,
        selected,
        target_coords,
        target_type_ref,
        is_garrison: false,
    });
    Some((burst, shot))
}

/// One strike visit (states 4..9), inside the aircraft's own dispatch. Its
/// Mission+0xBC write is `AircraftClass::Mission_Attack`'s own
/// (`0x00417FE0`, stores at `0x0041801B..0x00418D17`).
pub(super) fn visit(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    id: u64,
    out: &mut CombatEmit,
    under_attack_events: &mut Vec<UnderAttackEvent>,
) {
    let Some(entity) = world.substrate.entities.get(id) else {
        return;
    };
    let Some(state) =
        crate::sim::aircraft::attack_state(entity).filter(|state| (4..=9).contains(state))
    else {
        return;
    };
    let Some(obj) = world.interner.object_type(entity.type_ref(), rules) else {
        return;
    };
    let binary_frame = world.session.binary_frame();
    let weapon0 = combat_weapon::primary_for_tier(obj, entity.veterancy.rank())
        .and_then(|name| rules.weapon(name));
    let facts = StrikeFacts {
        state,
        target: crate::sim::combat::target_present(entity.attack_target.target(), world),
        ammo: entity.ammo().map_or(-1, |ammo| ammo.current()),
        strafe: combat_weapon::aircraft_strafes(rules, obj, entity.veterancy.rank()),
        fighter: obj.fighter,
        curley_shuffle: rules.general.curley_shuffle,
        weapon0_rof: weapon0.map_or(0, |weapon| weapon.rof),
        weapon0_range: weapon0.map_or(0, |weapon| weapon.range_leptons),
        speed: crate::util::fixed_math::ra2_speed_to_leptons_per_frame(obj.speed),
    };
    let mut host = CombatStrike {
        fog_enabled: fog_enabled(world),
        world,
        run,
        rules,
        overlay_registry,
        binary_frame,
        out,
        under_attack_events,
        obj,
        id,
    };
    let visit = strike_visit(&facts, &mut host);
    let entity = host.world.substrate.entities.get_mut(id).unwrap();
    entity.mission.set_handler_state(u32::from(visit.state));
    if let Some(latch) = visit.latch
        && entity.mission_leaf.as_aircraft().is_some()
    {
        entity.mission_leaf.set_aircraft_action_latch(latch);
    }
    entity
        .mission
        .write_dispatch_epilogue(binary_frame as i32, visit.delay);
}

/// The receiver's side of a strike visit.
struct CombatStrike<'w, 'r, 'f> {
    world: &'w mut Simulation,
    run: &'w mut ReceiverRun<'f>,
    rules: &'r RuleSet,
    overlay_registry: Option<&'w OverlayTypeRegistry>,
    /// GetFireError reads the live fog when the scenario has it.
    fog_enabled: bool,
    binary_frame: u32,
    out: &'w mut CombatEmit,
    under_attack_events: &'w mut Vec<UnderAttackEvent>,
    obj: &'r ObjectType,
    id: u64,
}

impl CombatStrike<'_, '_, '_> {
    fn id(&self) -> u64 {
        self.id
    }

    /// Target (`+0x2B4`), read live as `AircraftClass::Mission_Attack`
    /// (`0x00417FE0`)'s strike states read it.
    fn target(&self) -> Option<TargetKind> {
        self.world
            .substrate
            .entities
            .get(self.id())
            .and_then(|entity| entity.attack_target.target())
    }

    /// `vt+0x2E4` SelectWeapon(Target) with the live target, and the question
    /// `ask` puts to GetFireError's owner with that slot.
    fn with_subject<T>(
        &self,
        ask: impl FnOnce(&fire_error_world::FireSubject<'_>) -> T,
    ) -> Option<T> {
        let world = &*self.world;
        let firer = world.substrate.entities.get(self.id())?;
        let target = self.target()?;
        let cells = world
            .resolved_terrain
            .as_ref()
            .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
        let weapon_index = combat_weapon::select_weapon_index_against_in_query(
            self.rules,
            firer,
            self.obj,
            &combat_weapon::attacker_facts(firer, self.obj),
            Some(&target),
            world,
            &world.interner,
            cells.as_ref(),
            Some(world.house_alliances.as_map()),
        )?;
        Some(ask(&fire_error_world::FireSubject {
            world,
            rules: self.rules,
            overlay_registry: self.overlay_registry,
            fog: self
                .fog_enabled
                .then(|| world.fog.fire_view(world.house_alliances.as_map())),
            firer,
            obj: self.obj,
            target: Some(target),
            weapon_index,
        }))
    }

    /// `vt+0x3CC FireAt(Target, SelectWeapon(Target))` once:
    /// `AircraftClass::Fire_At @ 0x00415EE0`.
    fn shoot(&mut self) {
        crate::sim::aircraft::fire_at::fire_at(&mut AircraftFireAt {
            strike: self,
            target: None,
            bullet: None,
        });
    }
}

/// The receiver's side of one `AircraftClass::Fire_At`.
struct AircraftFireAt<'s, 'w, 'r, 'f> {
    strike: &'s mut CombatStrike<'w, 'r, 'f>,
    /// Fire_At's target, Mission_Attack's Target as the shot read it.
    target: Option<TargetKind>,
    /// The bullet TechnoClass::FireAt answered.
    bullet: Option<u64>,
}

impl AircraftFireAt<'_, '_, '_, '_> {
    fn entity(&self) -> Option<&GameEntity> {
        self.strike.world.substrate.entities.get(self.strike.id())
    }
}

impl FireAtHost for AircraftFireAt<'_, '_, '_, '_> {
    /// The first passenger (`+0x118`) `AircraftClass::Fire_At` (`0x00415EE0`)
    /// reads at `0x00415EEE`.
    fn carries_passenger(&mut self) -> bool {
        self.entity()
            .and_then(|entity| entity.passenger_role.cargo())
            .is_some_and(|cargo| !cargo.is_empty())
    }

    fn drop_payload(&mut self) {
        let strike = &mut *self.strike;
        crate::sim::aircraft::drop_payload::drop_payload(
            strike.world,
            strike.id,
            strike.rules,
            strike.overlay_registry,
            strike.run.effects(),
        );
    }

    /// The shared emission, its inline damage committed after the shot.
    fn techno_fire_at(&mut self) -> Option<AircraftShot> {
        let strike = &mut *self.strike;
        let id = strike.id();
        self.target = strike.target();
        let boundary = FireCommitBoundary::capture(strike.out);
        let mut fired = None;
        if let Some((_, Some(shot))) = live_shot(strike.world, strike.rules, id) {
            let rot = bullet_type(shot.selected.weapon, strike.rules).rot;
            fired = emit_admitted_fire(
                strike.world,
                strike.rules,
                shot,
                strike.binary_frame,
                strike.out,
                strike.overlay_registry,
                strike.run.effects(),
            )
            .map(|bullet| (bullet, rot));
        }
        boundary.commit(
            strike.world,
            strike.run,
            strike.rules,
            strike.overlay_registry,
            strike.out,
            strike.under_attack_events,
        );
        let (bullet, rot) = fired?;
        self.bullet = Some(bullet);
        let velocity = strike
            .world
            .projectiles
            .get(bullet)
            .map_or(ProjectileVelocity::new(0, 0, 0), |bullet| bullet.velocity());
        Some(AircraftShot::new(rot, velocity))
    }

    /// Fly's Apparent_Speed (`0x004CFE20`). RESIDUAL: another Locomotor
    /// answers its own speed natively and 0 here; no aircraft that fires
    /// flies one (the Rocket missiles carry no weapon).
    fn apparent_speed(&mut self) -> i32 {
        let fraction = self
            .entity()
            .and_then(|entity| entity.locomotor.as_ref())
            .and_then(|locomotor| locomotor.fly_runtime())
            .map_or(crate::util::fixed_math::SIM_ZERO, |fly| fly.current_speed());
        crate::sim::movement::air_movement::current_fly_speed(
            crate::util::fixed_math::ra2_speed_to_leptons_per_frame(self.strike.obj.speed),
            fraction,
        )
    }

    /// An aircraft's SecondaryFacing is its `barrel_facing`, which every
    /// aircraft gets at construction (`world_spawn::construction`); a
    /// fixture without one stands in with its body.
    fn facing(&mut self) -> u16 {
        let frame = self.strike.binary_frame;
        self.entity().map_or(0, |entity| {
            entity
                .barrel_facing
                .as_ref()
                .unwrap_or(&entity.body_facing)
                .current(frame)
        })
    }

    fn coords(&mut self) -> ProjectileCoord {
        object_get_coords(self.strike.world, self.strike.id())
            .unwrap_or(ProjectileCoord::new(0, 0, 0))
    }

    fn target_coords(&mut self) -> ProjectileCoord {
        let world = &*self.strike.world;
        let cells = world
            .resolved_terrain
            .as_ref()
            .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
        self.target
            .and_then(|target| {
                crate::sim::movement::ground_pose::target_get_coords(target, world, cells.as_ref())
            })
            .map_or(ProjectileCoord::new(0, 0, 0), |coord| {
                ProjectileCoord::new(coord.x, coord.y, coord.z)
            })
    }

    /// GetWeapon(0) at the aircraft's rank, whatever slot fired. A type
    /// without one faults natively (`0x004162ED` reads its NULL type); VERA
    /// scales to 0.
    fn weapon0_speed(&mut self) -> i32 {
        let strike = &*self.strike;
        self.entity()
            .and_then(|entity| combat_weapon::primary_for_tier(strike.obj, entity.veterancy.rank()))
            .and_then(|name| strike.rules.weapon(name))
            .map_or(0, |weapon| weapon.speed)
    }

    fn set_velocity(&mut self, velocity: ProjectileVelocity) {
        if let Some(bullet) = self.bullet {
            self.strike.world.projectiles.redirect(bullet, velocity);
        }
    }

    /// `0x0050B6F0` asks whether the owner is the local player; each human
    /// house's map takes what its own client would. RESIDUAL: in a campaign
    /// (GameMode 0) it also admits a `PlayerControl=` house, whose shot then
    /// asks and maps the player's map (through RevealArea's house gate,
    /// `0x00567AB0..0x00567B12`); VERA asks and maps that house's own.
    fn owner_is_player(&mut self) -> bool {
        self.entity()
            .is_some_and(|entity| self.strike.world.owner_is_human(entity.owner()))
    }

    fn location(&mut self) -> ProjectileCoord {
        let world = &*self.strike.world;
        self.entity()
            .map_or(ProjectileCoord::new(0, 0, 0), |entity| {
                let location = crate::sim::movement::ground_pose::object_location(
                    entity,
                    world.resolved_terrain.as_ref(),
                );
                ProjectileCoord::new(location.x, location.y, location.z)
            })
    }

    /// On the owner's map, as [`Self::owner_is_player`] reads it. A world
    /// without a map (a fixture) has nothing to ask.
    fn is_shrouded(&mut self, point: ProjectileCoord) -> bool {
        let world = &*self.strike.world;
        let (Some(owner), Some(terrain)) = (
            self.entity().map(GameEntity::owner),
            world.resolved_terrain.as_ref(),
        ) else {
            return false;
        };
        crate::sim::vision::point_is_shrouded(
            &world.fog,
            &crate::map::resolved_terrain::NativeCellQuery::canonical(terrain),
            owner,
            crate::sim::components::DriveCoord {
                x: point.x,
                y: point.y,
                z: point.z,
            },
        )
    }

    /// On the owner's map only: the other houses' clients do not run it.
    /// RESIDUAL: a negative `AttackingAircraftSightRange=` indexes before
    /// RevealArea's count table natively; VERA reveals nothing. Retail is 2.
    fn reveal_area(&mut self, final_pass: bool) {
        let strike = &mut *self.strike;
        let world = &mut *strike.world;
        let Some(entity) = world.substrate.entities.get(strike.id) else {
            return;
        };
        let owner = entity.owner();
        let location = crate::sim::movement::ground_pose::object_location(
            entity,
            world.resolved_terrain.as_ref(),
        );
        let radius = strike
            .rules
            .general
            .attacking_aircraft_sight_range
            .clamp(0, i32::from(u16::MAX)) as u16;
        let config = world.sight_reveal_config(Some(strike.rules));
        let heights = world.sight_height_grid(&config);
        let _ = world.reveal_area(
            &[owner],
            (
                location.x.div_euclid(256) as u16,
                location.y.div_euclid(256) as u16,
            ),
            location.z,
            radius,
            Some(&heights),
            final_pass,
            &config,
            crate::sim::world::UninitContext::new(Some(strike.rules), strike.overlay_registry)
                .with_effects(strike.run.effects()),
            crate::sim::vision::MapRevealProjection::Transient,
            world.session.current_house(),
        );
    }

    /// The kamikaze tracker's membership stands for `+0x6CA`.
    fn destroy_after_firing(&mut self) -> bool {
        self.strike.world.kamikaze.contains(self.strike.id())
    }

    fn uninit(&mut self) {
        let strike = &mut *self.strike;
        strike.world.uninit_with_context(
            strike.id,
            crate::sim::world::UninitContext::new(Some(strike.rules), strike.overlay_registry)
                .with_effects(strike.run.effects()),
        );
    }
}

impl StrikeHost for CombatStrike<'_, '_, '_> {
    fn fire_error(&mut self) -> fire_error::FireError {
        self.with_subject(|subject| subject.fire_error(true))
            .unwrap_or(fire_error::FireError::Illegal)
    }

    fn is_close(&mut self) -> bool {
        self.with_subject(|subject| subject.in_range())
            .unwrap_or(false)
    }

    /// State4 setters4182D3..41830C and state 5's `0x004185AC..0x004185DF`;
    /// Set does not snap.
    fn face_target(&mut self) {
        let id = self.id();
        let Some(target) = self.target() else {
            return;
        };
        let world = &mut *self.world;
        let Some(desired) = world.substrate.entities.get(id).and_then(|entity| {
            crate::sim::movement::turret::facing_toward_target(entity, &target, world)
        }) else {
            return;
        };
        let (frame, rot) = (self.binary_frame, self.obj.turret_rot);
        if let Some(entity) = world.substrate.entities.get_mut(id) {
            let initial = entity.body_facing.current(frame);
            entity.body_facing.set(desired, frame);
            entity
                .barrel_facing
                .get_or_insert_with(|| crate::sim::movement::FacingClass::new(initial, rot))
                .set(desired, frame);
        }
    }

    /// `0x00418403..0x004184BD`: pending ammo first (`0x0041840E`, before the
    /// first SelectWeapon and the signed Burst test), then the burst, each
    /// shot re-reading SelectWeapon's Burst; then the Scatter.
    fn release(&mut self) {
        let id = self.id();
        if let Some(ammo) = self
            .world
            .substrate
            .entities
            .get_mut(id)
            .and_then(|e| e.ammo_mut())
        {
            ammo.begin_release();
        }
        let mut count = 0i32;
        while let Some((burst, _)) = live_shot(self.world, self.rules, id) {
            if count >= burst {
                break;
            }
            self.shoot();
            count = count.wrapping_add(1);
        }
        self.scatter();
    }

    fn fire_at(&mut self) {
        self.shoot();
    }

    /// `0x00418478..0x004184BD` and the later shots' copies:
    /// [`Simulation::scatter_target_cell_from`] away from the aircraft. Native
    /// reads the Target unchecked (`0x004184A5`); VERA skips a missing one.
    fn scatter(&mut self) {
        let id = self.id();
        let Some(target) = self.target() else {
            return;
        };
        if let Err(cause) = self.world.scatter_target_cell_from(
            id,
            target,
            self.rules,
            self.overlay_registry,
            self.run.effects(),
        ) {
            log::debug!("aircraft {id} target cell Scatter failed: {cause}");
        }
    }

    fn assign_target_destination(&mut self) {
        let id = self.id();
        if let Some(target) = self.target() {
            let destination = target.into();
            self.world.assign_aircraft_destination(
                id,
                Some(destination),
                self.rules,
                self.run.effects(),
            );
        }
    }

    fn uncloak(&mut self) {
        let sound = sound_enabled(self.world);
        uncloak_to_fire(self.world, self.rules, self.obj, self.id(), sound);
    }

    fn epilogue(&mut self) -> i32 {
        crate::sim::mission::authority::rate_epilogue(
            self.rules,
            crate::sim::mission::MissionType::Attack,
            &mut self.world.scenario_rng,
        )
    }

    fn ammo(&mut self) -> i32 {
        self.world
            .substrate
            .entities
            .get(self.id)
            .and_then(|entity| entity.ammo())
            .map_or(-1, |ammo| ammo.current())
    }
}
