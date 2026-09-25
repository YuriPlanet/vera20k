//! Aircraft Mission_Attack's strike states 4..9 (`0x00417FE0`) in the combat
//! phase, where VERA's FireAt lives: the host [`aircraft::attack_mission`]'s
//! [`strike_visit`] asks for GetFireError, IsClose, the facings, the state-4
//! burst and the single shots of states 5..9. The shots reuse the receiver's
//! emission.
//!
//! [`aircraft::attack_mission`]: crate::sim::aircraft::attack_mission

use super::*;
use crate::sim::aircraft::AircraftMission;
use crate::sim::aircraft::attack_mission::{self, StrikeFacts, StrikeHost, strike_visit};

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
    let obj = rules.object(world.interner.resolve(entity.type_ref()))?;
    let target = entity.attack_target.as_ref().map(|attack| attack.target);
    let target_facts = match target {
        Some(TargetKind::Entity(id)) => world.substrate.entities.get(id).and_then(|target| {
            let obj = rules.object(world.interner.resolve(target.type_ref()))?;
            Some(combat_weapon::techno_target_facts(
                target,
                obj,
                world.resolved_terrain.as_ref(),
                combat_weapon::is_ally_by_object(
                    Some(&world.house_alliances),
                    &world.interner,
                    entity.owner(),
                    target.owner(),
                ),
            ))
        }),
        Some(TargetKind::Cell(rx, ry)) => Some(combat_weapon::cell_target_facts(
            rx,
            ry,
            world.resolved_terrain.as_ref(),
        )),
        None => None,
    };
    let selected = combat_weapon::select_weapon_for_emission(
        rules,
        obj,
        &combat_weapon::attacker_facts(entity, obj),
        target_facts.as_ref(),
    )?;
    let burst = selected.weapon.burst;
    let coordinates = match target {
        Some(TargetKind::Entity(id)) => world
            .substrate
            .entities
            .get(id)
            .filter(|target| !target.lifecycle.in_limbo)
            .map(|target| {
                (
                    target_coords(target, Some(rules), &world.interner),
                    target.type_ref(),
                )
            }),
        Some(TargetKind::Cell(rx, ry)) => Some((cell_center_coords(rx, ry), entity.type_ref())),
        None => None,
    };
    let shot = coordinates.map(|(target_coords, target_type_ref)| AdmittedFire {
        snap: build_attacker_snapshot(entity, target.unwrap(), None, None, None),
        obj,
        selected,
        target_coords,
        target_type_ref,
        is_garrison: false,
    });
    Some((burst, shot))
}

/// One strike visit (states 4..9) for an aircraft whose dispatch asked for it.
#[allow(clippy::too_many_arguments)]
pub(super) fn visit(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    snap: &AttackerSnapshot,
    fog: Option<&FogState>,
    binary_frame: u32,
    out: &mut CombatEmit,
    under_attack_events: &mut Vec<UnderAttackEvent>,
) {
    let id = snap.stable_id;
    // A phase-local receipt is not permission after an intervening mission change.
    let Some(state) =
        world
            .substrate
            .entities
            .get(id)
            .and_then(|entity| match entity.aircraft_mission {
                Some(AircraftMission::Attack { sub_state }) if (4..=9).contains(&sub_state) => {
                    Some(sub_state)
                }
                _ => None,
            })
    else {
        return;
    };
    let Some(obj) = rules.object(world.interner.resolve(snap.type_id)) else {
        return;
    };
    // Aircraft FireAt415EEE dispatches DropPayload while passengers remain.
    // Its retained payload counter and6C9 admission history still need migration;
    // keep that required carrier arm blocked rather than firing its gun instead.
    // RESIDUAL: the blocked carrier stays in its strike state and re-requests
    // every due visit. No stock AircraftType has Passengers=, and PDPLANE's
    // cargo flies ParaDrop, so this arm is unreachable with retail data.
    if world
        .substrate
        .entities
        .get(id)
        .and_then(|e| e.passenger_role.cargo())
        .is_some_and(|cargo| !cargo.is_empty())
    {
        return;
    }
    let entity = world.substrate.entities.get(id).unwrap();
    let weapon0 =
        combat_weapon::primary_for_tier(obj, entity.veterancy).and_then(|name| rules.weapon(name));
    let facts = StrikeFacts {
        state,
        target: attack_mission::aircraft_target_present(
            entity.attack_target.as_ref(),
            &world.substrate.entities,
        ),
        ammo: entity
            .aircraft_ammo
            .as_ref()
            .map_or(-1, |ammo| ammo.current),
        strafe: combat_weapon::aircraft_strafes(rules, obj, entity.veterancy),
        fighter: obj.fighter,
        curley_shuffle: rules.general.curley_shuffle,
        weapon0_rof: weapon0.map_or(0, |weapon| weapon.rof),
        weapon0_range: weapon0.map_or(0, |weapon| weapon.range_leptons),
        speed: crate::util::fixed_math::ra2_speed_to_leptons_per_frame(obj.speed),
    };
    let mut host = CombatStrike {
        world,
        run,
        rules,
        overlay_registry,
        fog,
        binary_frame,
        out,
        under_attack_events,
        obj,
        snap: snap.clone(),
    };
    let visit = strike_visit(&facts, &mut host);
    let entity = host.world.substrate.entities.get_mut(id).unwrap();
    entity.aircraft_mission = Some(AircraftMission::Attack {
        sub_state: visit.state,
    });
    if let Some(latch) = visit.latch
        && entity.mission_leaf.as_aircraft().is_some()
    {
        entity.mission_leaf.set_aircraft_action_latch(latch);
    }
    entity
        .mission
        .write_dispatch_epilogue(binary_frame as i32, visit.delay);
}

/// The combat phase's side of a strike visit.
struct CombatStrike<'w, 'r> {
    world: &'w mut Simulation,
    run: &'w mut ReceiverRun,
    rules: &'r RuleSet,
    overlay_registry: Option<&'w OverlayTypeRegistry>,
    fog: Option<&'w FogState>,
    binary_frame: u32,
    out: &'w mut CombatEmit,
    under_attack_events: &'w mut Vec<UnderAttackEvent>,
    obj: &'r ObjectType,
    snap: AttackerSnapshot,
}

impl CombatStrike<'_, '_> {
    fn id(&self) -> u64 {
        self.snap.stable_id
    }

    fn target(&self) -> Option<TargetKind> {
        self.world
            .substrate
            .entities
            .get(self.id())
            .and_then(|entity| entity.attack_target.as_ref())
            .map(|attack| attack.target)
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
        let target_facts = match target {
            TargetKind::Entity(id) => world.substrate.entities.get(id).and_then(|target| {
                let obj = self
                    .rules
                    .object(world.interner.resolve(target.type_ref()))?;
                Some(combat_weapon::techno_target_facts(
                    target,
                    obj,
                    world.resolved_terrain.as_ref(),
                    combat_weapon::is_ally_by_object(
                        Some(&world.house_alliances),
                        &world.interner,
                        firer.owner(),
                        target.owner(),
                    ),
                ))
            }),
            TargetKind::Cell(rx, ry) => Some(combat_weapon::cell_target_facts(
                rx,
                ry,
                world.resolved_terrain.as_ref(),
            )),
        };
        let weapon_index = combat_weapon::what_weapon_should_i_use(
            self.rules,
            self.obj,
            &combat_weapon::attacker_facts(firer, self.obj),
            target_facts.as_ref(),
        );
        Some(ask(&fire_error_world::FireSubject {
            world,
            rules: self.rules,
            overlay_registry: self.overlay_registry,
            fog: self.fog,
            firer,
            obj: self.obj,
            target: Some(target),
            weapon_index,
            garrison: None,
        }))
    }

    /// `vt+0x3CC FireAt(Target, SelectWeapon(Target))` once through the
    /// shared emission, its inline damage committed after the shot.
    fn shoot(&mut self) {
        let id = self.id();
        let boundary = FireCommitBoundary::capture(self.out);
        if let Some((_, Some(shot))) = live_shot(self.world, self.rules, id) {
            self.out.current_weapon_updates.push((
                id,
                shot.selected.index as u8,
                self.world.interner.intern(shot.selected.weapon_id),
            ));
            emit_admitted_fire(self.world, self.rules, shot, self.binary_frame, self.out);
        }
        boundary.commit(
            self.world,
            self.run,
            self.rules,
            self.overlay_registry,
            self.out,
            self.under_attack_events,
        );
    }
}

impl StrikeHost for CombatStrike<'_, '_> {
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
            crate::sim::movement::turret::facing_toward_target(
                entity,
                &target,
                &world.substrate.entities,
                Some(self.rules),
                &world.interner,
            )
        }) else {
            return;
        };
        let (frame, rot) = (self.binary_frame, self.obj.turret_rot);
        if let Some(entity) = world.substrate.entities.get_mut(id) {
            let initial = u16::from(entity.facing) << 8;
            entity
                .body_facing
                .get_or_insert_with(|| crate::sim::movement::FacingClass::new(initial, rot))
                .set(desired, frame);
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
            .and_then(|e| e.aircraft_ammo.as_mut())
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

    /// RESIDUAL (see `aircraft::attack_mission`): the source-aware Cell
    /// Scatter_Objects is not ported; the NullCoord blocker helper is not
    /// equivalent.
    fn scatter(&mut self) {}

    fn assign_target_destination(&mut self) {
        let id = self.id();
        if let Some(target) = self.target() {
            let destination = match target {
                TargetKind::Entity(id) => crate::sim::components::NavTargetRef::Entity { id },
                TargetKind::Cell(rx, ry) => crate::sim::components::NavTargetRef::cell(rx, ry),
            };
            self.world
                .assign_aircraft_attack_destination(id, Some(destination), self.rules);
        }
    }

    fn uncloak(&mut self) {
        let sound = sound_enabled(self.world);
        uncloak_to_fire(self.world, self.rules, self.obj, self.id(), sound);
    }

    fn epilogue(&mut self) -> i32 {
        attack_mission::mission_epilogue(
            self.rules,
            crate::sim::mission::MissionType::Attack,
            &mut self.world.scenario_rng,
        )
    }
}
