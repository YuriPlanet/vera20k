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
use crate::sim::aircraft::attack_mission::{StrafeKeys, StrikeFacts, StrikeHost, strike_visit};
use crate::sim::aircraft::fire_at::{AircraftShot, FireAtHost};
use crate::sim::components::NavTargetRef;
use crate::sim::projectile::ProjectileVelocity;
use crate::sim::world::HumanPlayerClients;

#[cfg(all(test, tests_sim_combat_a_c))]
#[path = "aircraft_release_tests.rs"]
mod tests;

/// Re-read Target, selection facts and weapon tier after each synchronous shot.
/// A detached/null target still participates in SelectWeapon and the loop bound,
/// but Techno FireAt6FDDAE returns without emitting or rearming. `aim` fires
/// the shot at that target instead (Phobos's strafing target cell,
/// `Ext/Aircraft/Hooks.cpp:255`); the weapon is still the one SelectWeapon
/// keeps for the live Target, as `CurrentAircraftWeaponIndex` is.
fn live_shot_for<'r>(
    world: &Simulation,
    rules: &'r RuleSet,
    id: u64,
    aim: Option<TargetKind>,
) -> Option<(i32, Option<AdmittedFire<'r>>)> {
    let entity = world.substrate.entities.get(id)?;
    let obj = world.interner.object_type(entity.type_ref(), rules)?;
    let selection = entity.attack_target.target();
    let target = aim.or(selection);
    let (_, selected) = combat_weapon::select_weapon_against(
        rules,
        entity,
        obj,
        &combat_weapon::attacker_facts(entity, obj),
        selection.as_ref(),
        world,
        &world.interner,
        world.resolved_terrain.as_ref(),
        Some(&world.house_alliances),
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
        Some(target @ TargetKind::Cell(..)) => {
            resolve_target_coords(&target, world).map(|coords| (coords, entity.type_ref()))
        }
        Some(target @ TargetKind::Bullet(..)) => {
            resolve_target_coords(&target, world).map(|coords| (coords, entity.type_ref()))
        }
        None => None,
    };
    let shot = coordinates.map(|(target_coords, target_type_ref)| AdmittedFire {
        snap: build_attacker_snapshot(entity, target.unwrap(), None),
        obj,
        selected,
        target_coords,
        target_type_ref,
        is_garrison: false,
        // Aircraft release is its own admitted shot; the class pause hooks
        // never apply to Aircraft (`Hooks.Firing.cpp:658-662`).
        at_firing_frame: true,
        // The raw FireAt paths run no class firing update.
        pause_handled: false,
    });
    Some((burst, shot))
}

/// [`live_shot_for`] at the entity's own Target.
fn live_shot<'r>(
    world: &Simulation,
    rules: &'r RuleSet,
    id: u64,
) -> Option<(i32, Option<AdmittedFire<'r>>)> {
    live_shot_for(world, rules, id, None)
}

/// One strike visit (states 4..9), inside the aircraft's own dispatch. Its
/// Mission+0xBC write is `AircraftClass::Mission_Attack`'s own
/// (`0x00417FE0`, stores at `0x0041801B..0x00418D17`).
pub(super) fn visit(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    rules: &RuleSet,
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
    let strafing = weapon0.map_or(StrafeKeys::STOCK, |weapon| {
        StrafeKeys::from_weapon(weapon, &rules.phobos_globals)
    });
    // Phobos `FiringForceScatter` (`Ext/AircraftType/Body.cpp:36`, default from
    // `[General] AircraftFiringForceScatter=`, constructor true
    // `Ext/Rules/Body.h:888`).
    let firing_force_scatter = obj
        .phobos_firing_force_scatter
        .unwrap_or(rules.phobos_globals.aircraft_firing_force_scatter);
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
        strafing,
        firing_force_scatter: rules.aircraft_firing_force_scatter_for(Some(obj)),
    };
    let mut host = CombatStrike {
        fog_enabled: fog_enabled(world),
        world,
        run,
        rules,
        binary_frame,
        out,
        under_attack_events,
        obj,
        strafing,
        firing_force_scatter: facts.firing_force_scatter,
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
    /// GetFireError reads the live fog when the scenario has it.
    fog_enabled: bool,
    binary_frame: u32,
    out: &'w mut CombatEmit,
    under_attack_events: &'w mut Vec<UnderAttackEvent>,
    obj: &'r ObjectType,
    /// The selected slot's `Strafing.*` keys, `Get(5)`/globals applied.
    strafing: StrafeKeys,
    /// Phobos `AircraftTypeExt::FiringForceScatter`, globals applied.
    firing_force_scatter: bool,
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
        let target = self.target()?;
        self.with_subject_at(target, ask)
    }

    /// [`Self::with_subject`] with the question asked of `aim` instead — the
    /// strafing target cell (`Ext/Aircraft/Hooks.cpp:252`) — while the weapon
    /// stays the one SelectWeapon keeps for the live Target, like the retained
    /// `CurrentAircraftWeaponIndex`.
    fn with_subject_at<T>(
        &self,
        aim: TargetKind,
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
            Some(&world.house_alliances),
        )?;
        Some(ask(&fire_error_world::FireSubject {
            world,
            rules: self.rules,
            fog: self
                .fog_enabled
                .then(|| world.fog.fire_view(&world.house_alliances)),
            firer,
            obj: self.obj,
            target: Some(aim),
            weapon_index,
        }))
    }

    /// `vt+0x3CC FireAt(Target, SelectWeapon(Target))` once:
    /// `AircraftClass::Fire_At @ 0x00415EE0`. `aim` fires the shot at that
    /// target instead of the entity's Target (Phobos's strafing cell).
    fn shoot_at(&mut self, aim: Option<TargetKind>) {
        crate::sim::aircraft::fire_at::fire_at(&mut AircraftFireAt {
            strike: self,
            target: None,
            aim,
            bullet: None,
        });
    }

    /// The entity's primary weapon over its Target is a strafe
    /// (`IFlyControl +0x18`, `0x0041B7F0`).
    fn strafe(&self) -> bool {
        self.world
            .substrate
            .entities
            .get(self.id())
            .is_some_and(|entity| {
                combat_weapon::aircraft_strafes(self.rules, self.obj, entity.veterancy.rank())
            })
    }

    /// Phobos `AircraftExt::Strafe_BombsDroppedThisRound`
    /// (`Ext/Aircraft/Body.h:14`).
    fn live_strafe_counter(&self) -> i32 {
        self.world
            .substrate
            .entities
            .get(self.id())
            .map_or(0, |entity| {
                entity
                    .mission_leaf
                    .aircraft_strafe_bombs_dropped_this_round()
            })
    }

    fn write_strafe_counter(&mut self, value: i32) {
        if let Some(entity) = self.world.substrate.entities.get_mut(self.id())
            && entity.mission_leaf.as_aircraft().is_some()
        {
            entity
                .mission_leaf
                .set_aircraft_strafe_bombs_dropped_this_round(value);
        }
    }

    /// One `pThis->Fire` of `AircraftExt::FireWeapon`'s burst loop
    /// (`Ext/Aircraft/Body.cpp:24-29`): `SimulateBurst` alternates
    /// `CurrentBurstIndex` across a single-shot strafing weapon's passes.
    fn fire_weapon_shot(&mut self, aim: Option<TargetKind>, burst: i32) {
        if self.strafe() && burst < 2 && self.strafing.simulate_burst {
            let index = i32::from(self.live_strafe_counter() % 2 == 0);
            let id = self.id();
            if let Some(entity) = self.world.substrate.entities.get_mut(id) {
                entity.seldom.weapon_burst.set_index(index);
            }
        }
        self.shoot_at(aim);
    }

    /// `AircraftExt::FireWeapon`'s tail (`Ext/Aircraft/Body.cpp:31-46`): one
    /// more bomb this round, and with `Strafing.UseAmmoPerShot` one Ammo per
    /// shot, `ShouldLoseAmmo` cleared (`+0x6C8`, VERA's pending release), and
    /// an empty aircraft's Target and Destination dropped.
    fn fire_weapon_tail(&mut self) {
        if !self.strafe() {
            return;
        }
        let counter = self.live_strafe_counter();
        self.write_strafe_counter(counter.wrapping_add(1));
        if !self.strafing.use_ammo_per_shot {
            return;
        }
        let mut empty = false;
        if let Some(entity) = self.world.substrate.entities.get_mut(self.id())
            && let Some(ammo) = entity.ammo_mut()
        {
            ammo.add(-1);
            ammo.cancel_release();
            empty = ammo.current() == 0;
        }
        if empty {
            let id = self.id();
            if let Some(entity) = self.world.substrate.entities.get_mut(id) {
                entity.attack_target.set(None);
            }
            self.world
                .assign_aircraft_destination(id, None, self.rules, self.run.effects());
        }
    }
}

/// The receiver's side of one `AircraftClass::Fire_At`.
struct AircraftFireAt<'s, 'w, 'r, 'f> {
    strike: &'s mut CombatStrike<'w, 'r, 'f>,
    /// Fire_At's target, Mission_Attack's Target as the shot read it.
    target: Option<TargetKind>,
    /// The shot's target when an effect overrides the entity's Target (the
    /// Phobos strafing cell); `None` fires at the entity's own Target.
    aim: Option<TargetKind>,
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
            .and_then(|entity| entity.passenger_role().cargo())
            .is_some_and(|cargo| !cargo.is_empty())
    }

    /// Phobos `AircraftClass_Fire_KickOutPassengers` (`0x415EEE`,
    /// `Ext/Aircraft/Hooks.cpp:455-474`): `GetWeapon(weaponIdx)` is the
    /// selected weapon's `KickOutPassengers`, else
    /// `[General] AircraftWeapon.KickOutPassengers=`; no selected weapon
    /// keeps the stock drop.
    fn kick_out_passengers(&mut self) -> bool {
        let strike = &mut *self.strike;
        let Some((_, Some(shot))) =
            live_shot_for(strike.world, strike.rules, strike.id(), self.aim)
        else {
            return true;
        };
        shot.selected.weapon.phobos_kick_out_passengers.unwrap_or(
            strike
                .rules
                .phobos_globals
                .aircraft_weapon_kick_out_passengers,
        )
    }

    fn drop_payload(&mut self) {
        let strike = &mut *self.strike;
        crate::sim::aircraft::drop_payload::drop_payload(
            strike.world,
            strike.id,
            strike.rules,
            strike.run.effects(),
        );
    }

    /// The shared emission, its inline damage committed after the shot.
    fn techno_fire_at(&mut self) -> Option<AircraftShot> {
        let strike = &mut *self.strike;
        let id = strike.id();
        self.target = self.aim.or_else(|| strike.target());
        let boundary = FireCommitBoundary::capture(strike.out);
        let mut fired = None;
        if let Some((_, Some(shot))) = live_shot_for(strike.world, strike.rules, id, self.aim) {
            // Phobos `AircraftClass_FireAt_Vertical` (`0x415F25`,
            // `Misc/Hooks.BugFixes.cpp:2423`): a `Vertical=` bullet whose
            // `Vertical.AircraftFix` (else the `[General]` key, default yes)
            // is on keeps FireAt's velocity; the ROT course block is skipped.
            let (rot, keeps_velocity) = {
                let kind = bullet_type(shot.selected.weapon, strike.rules);
                let keeps_velocity = kind.vertical
                    && kind
                        .vertical_aircraft_fix
                        .unwrap_or(strike.rules.phobos_globals.vertical_aircraft_fix);
                (kind.rot, keeps_velocity)
            };
            fired = emit_admitted_fire(
                strike.world,
                strike.rules,
                shot,
                strike.binary_frame,
                strike.out,
                Some(strike.rules.overlay_registry()),
                strike.run.effects(),
            )
            .map(|bullet| (bullet, rot, keeps_velocity));
        }
        boundary.commit(
            strike.world,
            strike.run,
            strike.rules,
            Some(strike.rules.overlay_registry()),
            strike.out,
            strike.under_attack_events,
        );
        let (bullet, rot, keeps_velocity) = fired?;
        self.bullet = Some(bullet);
        let velocity = strike
            .world
            .projectiles
            .get(bullet)
            .map_or(ProjectileVelocity::new(0, 0, 0), |bullet| bullet.velocity());
        Some(AircraftShot::new(rot, velocity).keeping_velocity(keeps_velocity))
    }

    /// Fly's Apparent_Speed (`0x004CFE20`). RESIDUAL: another Locomotor
    /// answers its own speed natively and 0 here; no aircraft that fires
    /// flies one (the Rocket missiles carry no weapon).
    fn apparent_speed(&mut self) -> i32 {
        let fraction = self
            .entity()
            .and_then(|entity| entity.locomotor.as_ref())
            .and_then(|locomotor| locomotor.fly_runtime())
            .map_or(
                crate::util::native_x87::NativeF64Bits::POSITIVE_ZERO,
                |fly| fly.current_speed(),
            );
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

    /// [`Simulation::human_player_client_set`] of the owner: each human
    /// house's map takes what its own client would, and in a campaign a
    /// `PlayerControl=` house's shot asks the player's map too.
    fn player_clients(&mut self) -> HumanPlayerClients {
        self.entity().map_or(HumanPlayerClients::None, |entity| {
            self.strike.world.human_player_client_set(entity.owner())
        })
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

    /// On `client`'s map. A world without a map (a fixture) has nothing to
    /// ask.
    fn is_shrouded(&mut self, client: InternedId, point: ProjectileCoord) -> bool {
        let world = &*self.strike.world;
        let Some(terrain) = world.resolved_terrain.as_ref() else {
            return false;
        };
        crate::sim::vision::point_is_shrouded(
            &world.fog,
            &crate::map::resolved_terrain::NativeCellQuery::canonical(terrain),
            client,
            crate::sim::components::DriveCoord {
                x: point.x,
                y: point.y,
                z: point.z,
            },
        )
    }

    /// On `client`'s map, when RevealArea's house gate
    /// (`0x00567AB0..0x00567B12`, [`crate::sim::vision::reveal_admits`])
    /// admits the owner there: the owner's own map, or an ally's under
    /// `AllyReveal=`.
    /// RESIDUAL: a negative `AttackingAircraftSightRange=` indexes before
    /// RevealArea's count table natively; VERA reveals nothing. Retail is 2.
    fn reveal_area(&mut self, client: InternedId, final_pass: bool) {
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
        let config = world.sight_reveal_config(strike.rules);
        if !crate::sim::vision::reveal_admits(
            &world.house_alliances,
            owner,
            client,
            &config,
            &world.interner,
        ) {
            return;
        }
        let heights = world.sight_height_grid(&config);
        let _ = world.reveal_area(
            &[client],
            (
                location.x.div_euclid(256) as u16,
                location.y.div_euclid(256) as u16,
            ),
            location.z,
            radius,
            Some(&heights),
            final_pass,
            &config,
            crate::sim::world::UninitContext::new(strike.rules).with_effects(strike.run.effects()),
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
            crate::sim::world::UninitContext::new(strike.rules).with_effects(strike.run.effects()),
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
        let frame = self.binary_frame;
        if let Some(entity) = world.substrate.entities.get_mut(id) {
            entity.ensure_fly_secondary_facing();
            entity.body_facing.set(desired, frame);
            if let Some(secondary) = entity.barrel_facing.as_mut() {
                secondary.set(desired, frame);
            }
        }
    }

    /// `0x00418403..0x004184BD`: pending ammo first (`0x0041840E`, before the
    /// first SelectWeapon and the signed Burst test), then
    /// `AircraftExt::FireWeapon`'s burst (`Ext/Aircraft/Body.cpp:20-33`),
    /// then the Scatter.
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
        let mut fired = false;
        while let Some((burst, _)) = live_shot(self.world, self.rules, id) {
            if count >= burst {
                break;
            }
            self.fire_weapon_shot(None, burst);
            fired = true;
            count = count.wrapping_add(1);
        }
        if fired {
            self.fire_weapon_tail();
        }
        // Phobos `AircraftClass_MissionAttack_ScatterCell1`
        // (`Ext/Aircraft/Hooks.cpp:344-349`, hook `0x41847E`): the stock
        // Scatter runs only while `FiringForceScatter` is on.
        if self.firing_force_scatter {
            self.scatter();
        }
    }

    /// One strafe state's `AircraftExt::FireWeapon` (`Ext/Aircraft/Hooks.cpp:167-197`).
    fn fire_at(&mut self) {
        let id = self.id();
        let Some((burst, _)) = live_shot(self.world, self.rules, id) else {
            return;
        };
        if burst <= 0 {
            return;
        }
        self.fire_weapon_shot(None, burst);
        self.fire_weapon_tail();
    }

    fn fire_at_cell(&mut self, cell: (u16, u16)) {
        let id = self.id();
        let aim = Some(TargetKind::Cell(cell.0, cell.1));
        let Some((burst, _)) = live_shot_for(self.world, self.rules, id, aim) else {
            return;
        };
        if burst <= 0 {
            return;
        }
        self.fire_weapon_shot(aim, burst);
        self.fire_weapon_tail();
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
            Some(self.rules.overlay_registry()),
            self.run.effects(),
        ) {
            log::debug!("aircraft {id} target cell Scatter failed: {cause}");
        }
    }

    /// Phobos `AircraftClass_Mission_Attack_StrafeCell`
    /// (`Ext/Aircraft/Hooks.cpp:257`): `pTargetCell->ScatterContent(Location,
    /// true, false, false)` on the stored strafing cell, which outlives the
    /// Target this shot was aimed at.
    fn scatter_at_cell(&mut self, cell: (u16, u16)) {
        let id = self.id();
        if let Err(cause) = self.world.scatter_target_cell_from(
            id,
            TargetKind::Cell(cell.0, cell.1),
            self.rules,
            Some(self.rules.overlay_registry()),
            self.run.effects(),
        ) {
            log::debug!("aircraft {id} strafe cell Scatter failed: {cause}");
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
        let id = self.id();
        uncloak_to_fire(
            &self.world.session,
            &mut self.world.substrate,
            &mut self.world.sound_events,
            self.rules,
            self.obj,
            id,
            sound,
        );
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

    fn strafe_counter(&mut self) -> i32 {
        self.live_strafe_counter()
    }

    fn set_strafe_counter(&mut self, value: i32) {
        self.write_strafe_counter(value);
    }

    fn strafe_cell(&mut self) -> Option<(u16, u16)> {
        self.world
            .substrate
            .entities
            .get(self.id)
            .and_then(|entity| entity.mission_leaf.aircraft_strafe_target_cell())
    }

    fn set_strafe_cell(&mut self, cell: Option<(u16, u16)>) {
        if let Some(entity) = self.world.substrate.entities.get_mut(self.id)
            && entity.mission_leaf.as_aircraft().is_some()
        {
            entity.mission_leaf.set_aircraft_strafe_target_cell(cell);
        }
    }

    /// `MapClass::GetCellAt(pThis->Target->GetCoords())` (`Ext/Aircraft/Hooks.cpp:234-235`).
    fn store_target_cell(&mut self) {
        let Some(target) = self.target() else {
            return;
        };
        if let Some((rx, ry, _, _)) = resolve_target_coords(&target, &*self.world) {
            self.set_strafe_cell(Some((rx, ry)));
        }
    }

    fn cell_fire_error(&mut self, cell: (u16, u16)) -> fire_error::FireError {
        self.with_subject_at(TargetKind::Cell(cell.0, cell.1), |subject| {
            subject.fire_error(true)
        })
        .unwrap_or(fire_error::FireError::Illegal)
    }

    fn assign_cell_destination(&mut self, cell: (u16, u16)) {
        self.world.assign_aircraft_destination(
            self.id(),
            Some(NavTargetRef::cell(cell.0, cell.1)),
            self.rules,
            self.run.effects(),
        );
    }
}
