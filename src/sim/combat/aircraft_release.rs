//! Aircraft Mission_Attack's admitted state4 release (417FE0/418403).
//! Shared admission and FireAt remain in the receiver; this caller owns the
//! synchronous burst, pending-ammo write and successful mission suffix.

use super::*;
use crate::sim::aircraft::AircraftMission;

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
        snap: build_attacker_snapshot(entity, target.unwrap(), 0, 0, None, None, None),
        obj,
        selected,
        target_coords,
        target_type_ref,
        is_garrison: false,
    });
    Some((burst, shot))
}

pub(super) fn fire(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    snap: &AttackerSnapshot,
    fog: Option<&FogState>,
    require_playfield_membership: bool,
    binary_frame: u32,
    tick_ms: u32,
    out: &mut CombatEmit,
    under_attack_events: &mut Vec<UnderAttackEvent>,
) {
    let id = snap.stable_id;
    // A phase-local receipt is not permission after an intervening mission change.
    if !world.substrate.entities.get(id).is_some_and(|entity| {
        matches!(
            entity.aircraft_mission,
            Some(AircraftMission::Attack { sub_state: 4 })
        )
    }) {
        return;
    }
    let Some(obj) = rules.object(world.interner.resolve(snap.type_id)) else {
        return;
    };
    // Aircraft FireAt415EEE dispatches DropPayload while passengers remain.
    // Its retained payload counter and6C9 admission history still need migration;
    // keep that required carrier arm blocked rather than firing its gun instead.
    if world
        .substrate
        .entities
        .get(id)
        .and_then(|e| e.passenger_role.cargo())
        .is_some_and(|cargo| !cargo.is_empty())
    {
        return;
    }
    let mut snap = snap.clone();
    if !combat_weapon::aircraft_strafes(rules, obj, snap.veterancy) {
        // State4 setters4182D3..41830C precede GetFireError; Set does not snap.
        if let Some(desired) = world.substrate.entities.get(id).and_then(|entity| {
            crate::sim::movement::turret::facing_toward_target(
                entity,
                &snap.target,
                &world.substrate.entities,
                Some(rules),
                &world.interner,
            )
        }) {
            if let Some(entity) = world.substrate.entities.get_mut(id) {
                let initial = u16::from(entity.facing) << 8;
                let body = entity.body_facing.get_or_insert_with(|| {
                    crate::sim::movement::FacingClass::new(initial, obj.turret_rot)
                });
                body.set(desired, binary_frame);
                let secondary = entity.barrel_facing.get_or_insert_with(|| {
                    crate::sim::movement::FacingClass::new(initial, obj.turret_rot)
                });
                secondary.set(desired, binary_frame);
                snap.hull_facing = entity.body_facing;
                snap.barrel_facing = entity.barrel_facing;
            }
        }
    }
    let mut boundary = FireCommitBoundary::capture(out);
    if admit_attacker_fire(
        world,
        rules,
        overlay_registry,
        &snap,
        fog,
        require_playfield_membership,
        binary_frame,
        tick_ms,
        world.active_wave_links.contains_key(&id),
        out,
    )
    .is_none()
    {
        boundary.commit(
            world,
            run,
            rules,
            overlay_registry,
            out,
            under_attack_events,
        );
        return;
    }
    // 41840E precedes the first SelectWeapon and the signed Burst<=0 test.
    if let Some(ammo) = world
        .substrate
        .entities
        .get_mut(id)
        .and_then(|e| e.aircraft_ammo.as_mut())
    {
        ammo.begin_release();
    }
    let mut count = 0i32;
    while let Some((burst, _)) = live_shot(world, rules, id) {
        if count >= burst {
            break;
        }
        // Native reselects once for the bound and once before the FireAt call.
        if let Some((_, Some(shot))) = live_shot(world, rules, id) {
            out.current_weapon_updates.push((
                id,
                shot.selected.index as u8,
                world.interner.intern(shot.selected.weapon_id),
            ));
            emit_admitted_fire(world, rules, overlay_registry, shot, binary_frame, out);
        }
        boundary.commit(
            world,
            run,
            rules,
            overlay_registry,
            out,
            under_attack_events,
        );
        boundary = FireCommitBoundary::capture(out);
        count = count.wrapping_add(1);
    }
    boundary.commit(
        world,
        run,
        rules,
        overlay_registry,
        out,
        under_attack_events,
    );
    finish_release(world, rules, id, binary_frame);
}

/// 4184C2..418584: suffix does not depend on FireAt's returned bullet. Native
/// reveal481670 between the loop and this suffix remains a visibility residual.
fn finish_release(world: &mut Simulation, rules: &RuleSet, id: u64, frame: u32) {
    let Some(entity) = world.substrate.entities.get(id) else {
        return;
    };
    let Some(obj) = rules.object(world.interner.resolve(entity.type_ref())) else {
        return;
    };
    let strafe = combat_weapon::aircraft_strafes(rules, obj, entity.veterancy);
    let fighter = obj.fighter;
    let ammo = entity
        .aircraft_ammo
        .as_ref()
        .map_or(-1, |ammo| ammo.current);
    let (state, delay, ready) = if strafe || fighter {
        let Some(weapon) = combat_weapon::primary_for_tier(obj, entity.veterancy)
            .and_then(|name| rules.weapon(name))
        else {
            return;
        };
        (
            if strafe {
                6
            } else if ammo > 0 {
                1
            } else {
                10
            },
            weapon.rof,
            true,
        )
    } else {
        (5, 1, false)
    };
    let entity = world.substrate.entities.get_mut(id).unwrap();
    entity.aircraft_mission = Some(AircraftMission::Attack { sub_state: state });
    if ready && entity.mission_leaf.as_aircraft().is_some() {
        entity.mission_leaf.set_aircraft_action_latch(true);
    }
    entity.mission.write_dispatch_epilogue(frame as i32, delay);
}
