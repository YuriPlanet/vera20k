//! World-owned synchronous FireAt execution: fire admission, emission and
//! bookkeeping for each firer's own visit. The receivers it reaches stay in
//! `world_receiver`.

use super::world_receiver::{
    ReceiverRun, callbacks_enabled, commit_area, commit_smudges, fog_enabled, sound_enabled,
};
#[cfg(test)]
use super::world_receiver::{commit_projectile_detonations_inline, radiation_enabled};
use super::*;
use crate::sim::houses::Houses;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::world::FrameChannel;
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::techno_ai_cloak::emit_configured_cloak_sound;
use crate::sim::world::{FrameEffects, Simulation};

#[path = "aircraft_release.rs"]
mod aircraft_release;

/// FireAt6FF4CC..6FF647 has one exclusive effect ladder. Sonic's earlier
/// Wave creation is independent; these flags select the later alternative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FireAtEffect {
    None,
    Laser,
    ElectricBolt,
    RadiationBeam,
    RadiationEruption,
    MagneticWave,
}

pub(crate) fn fire_at_effect(weapon: &crate::rules::weapon_type::WeaponType) -> FireAtEffect {
    if weapon.is_laser {
        FireAtEffect::Laser
    } else if weapon.is_electric_bolt {
        FireAtEffect::ElectricBolt
    } else if weapon.is_rad_beam {
        FireAtEffect::RadiationBeam
    } else if weapon.is_rad_eruption {
        FireAtEffect::RadiationEruption
    } else if weapon.is_mag_beam {
        FireAtEffect::MagneticWave
    } else {
        FireAtEffect::None
    }
}

/// How far one fire routine got.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct FireReach {
    /// The GetFireError code the class acted on, when the routine reached
    /// GetFireError at all.
    pub(super) code: Option<fire_error::FireError>,
    /// An infantryman's Stage matched his fire frame (`0x005209AF`): the
    /// routine's tail ([`infantry_fire_tail`]) follows whether the shot fired
    /// or was refused.
    pub(super) infantry_fire_frame: bool,
}

pub(super) fn resolve_attacker_fire(
    world: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    snap: &AttackerSnapshot,
    fog_enabled: bool,
    binary_frame: u32,
    has_active_wave: bool,
    out: &mut CombatEmit,
    effects: FrameEffects<'_>,
) -> FireReach {
    let mut reach = FireReach::default();
    if let Some(shot) = admit_attacker_fire(
        world,
        rules,
        overlay_registry,
        snap,
        fog_enabled,
        binary_frame,
        has_active_wave,
        &mut reach,
        out,
        effects,
    ) {
        emit_admitted_fire(
            world,
            rules,
            shot,
            binary_frame,
            out,
            overlay_registry,
            effects,
        );
    }
    reach
}

/// `TechnoClass::Fire`'s per-shot report (`0x006FF349..0x006FF38F`, every
/// class, buildings included): none for an empty `Report=` (a signed count
/// test) or an `IsGattling=` type, whose report is its stage loop
/// (`combat::gattling`); otherwise `Report[(u16)+0x3C8 % Count]`, the low
/// word of the constructor's Scenario draw picking the item (an unsigned
/// `div`). Native rows: `tools/spatial_oracle/building_gattling.json`
/// (`report_gate`).
pub(crate) fn per_shot_report(
    weapon: &crate::rules::weapon_type::WeaponType,
    is_gattling: bool,
    sequence: u16,
) -> Option<&str> {
    let count = weapon.report_count();
    if count <= 0 || is_gattling {
        return None;
    }
    weapon.report_item(usize::from(sequence) % count as usize)
}

fn admit_attacker_fire<'r>(
    world: &mut Simulation,
    rules: &'r RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    snap: &AttackerSnapshot,
    fog_enabled: bool,
    binary_frame: u32,
    has_active_wave: bool,
    reach: &mut FireReach,
    out: &mut CombatEmit,
    effects: FrameEffects<'_>,
) -> Option<AdmittedFire<'r>> {
    let sound_enabled = sound_enabled(world);
    let delayed_building_slot = match snap.building_shot {
        Some(super::BuildingShot::Delayed { slot, .. }) => Some(slot),
        _ => None,
    };
    let mission_building_weapon = match snap.building_shot {
        Some(super::BuildingShot::Mission { weapon, .. }) => Some(weapon),
        _ => None,
    };
    let obj = match world.interner.object_type(snap.type_id, rules) {
        Some(o) => o,
        None => {
            if delayed_building_slot.is_none() {
                out.remove_attack.push(snap.stable_id);
            }
            return None;
        }
    };

    // Stock Sonic weapons occupy index 0, and native FireAt tests that
    // WeaponType before resolving the target. Preserve that whole-call gate
    // so a stale/missing target cannot retarget or clear the order while the
    // owner's exact Wave link remains live. GetFireError's T37/T46 (the live
    // Wave on either slot) keep the same protection for other layouts.
    if snap.category == EntityCategory::Unit
        && has_active_wave
        && combat_weapon::primary_for_tier(obj, snap.veterancy)
            .and_then(|weapon_id| rules.weapon(weapon_id))
            .is_some_and(|weapon| weapon.is_sonic)
    {
        return None;
    }

    // Check if target is alive and get its data.
    // For structures, target_coords returns the foundation center instead
    // of the NW corner.
    // For Cell targets (force-fire on terrain), synthesize the data:
    // cell-center coords, "always alive" (hp 1: cells don't despawn), and the
    // attacker's own type as the target type.
    let target_data: Option<(u16, u16, SimFixed, SimFixed, i32, InternedId)> = match snap.target {
        TargetKind::Entity(target_id) => world
            .substrate
            .entities
            .get(target_id)
            .map(|t| {
                let (trx, try_, tsx, tsy) = target_coords(t);
                (trx, try_, tsx, tsy, t.health.current(), t.type_ref())
            })
            .or_else(|| {
                // FireAt6FDDAE..6FDDC0 retains the actual Object pointer;
                // Terrain's +48 is Object5F65A0, owned by the shared resolver.
                // Read its actual Terrain type/Health, without a Techno shell.
                let target = world.terrain_objects.objects().get(&target_id)?;
                let (rx, ry, sx, sy) = resolve_target_coords(&snap.target, world)?;
                Some((rx, ry, sx, sy, target.health(), target.type_ref()))
            }),
        TargetKind::Cell(..) => resolve_target_coords(&snap.target, world)
            .map(|(trx, try_, tsx, tsy)| (trx, try_, tsx, tsy, 1i32, snap.type_id)),
    };

    let (target_rx, target_ry, target_sub_x, target_sub_y, target_type_ref) = match target_data {
        Some((rx, ry, sx, sy, hp, type_ref)) if hp > 0 => (rx, ry, sx, sy, type_ref),
        _ => {
            if delayed_building_slot.is_some() {
                return None;
            }
            // Pointer expiry clears a dead target natively before any fire
            // routine runs; only a kill that skips the broadcast reaches here.
            out.remove_attack.push(snap.stable_id);
            return None;
        }
    };

    // Weapon selection, none of which asks legality (GetFireError below, or
    // the building's own visit, does): a delayed building shot resolves the
    // slot it saved while arming, a building's Mission_Attack shot the weapon
    // its visit selected ([`super::BuildingShot::Mission`]), and everything
    // else runs the native selection ladder (`What_Weapon_Should_I_Use`
    // `0x006F3330`). Each index resolves through GetWeapon
    // ([`combat_weapon::resolve_weapon_index`]), so an occupied building fires
    // its firing occupant's weapon (`0x004526F0`).
    let firer = world.substrate.entities.get(snap.stable_id)?;
    let is_garrison = snap.garrison.is_some();
    let (mut weapon_index, mut selected) = if let Some(saved_slot) = delayed_building_slot {
        let index = match saved_slot {
            WeaponSlot::Primary => 0,
            WeaponSlot::Secondary => 1,
        };
        let selected = combat_weapon::resolve_weapon_index(
            rules,
            firer,
            obj,
            index,
            &world.substrate.entities,
            &world.interner,
        )?;
        (selected.index, Some(selected))
    } else if let Some(weapon) = mission_building_weapon {
        (
            weapon,
            combat_weapon::resolve_weapon_index(
                rules,
                firer,
                obj,
                weapon,
                &world.substrate.entities,
                &world.interner,
            ),
        )
    } else {
        let Some((index, selected)) = combat_weapon::select_weapon_against(
            rules,
            firer,
            obj,
            &combat_weapon::attacker_facts(firer, obj),
            Some(&snap.target),
            world,
            &world.interner,
            world.resolved_terrain.as_ref(),
            fog_enabled.then_some(world.house_alliances.as_map()),
        ) else {
            out.remove_attack.push(snap.stable_id);
            return None;
        };
        (index, selected)
    };

    let infantry_fire = snap.category == EntityCategory::Infantry;
    let sequences = rules.animation_sequence(world.interner.resolve(snap.type_id));
    let firing_at_entry = infantry_fire
        && world
            .substrate
            .entities
            .get(snap.stable_id)
            .is_some_and(|entity| entity.mission_leaf.foot_firing_sequence_latch() != 0);
    if firing_at_entry {
        let entity = world.substrate.entities.get(snap.stable_id)?;
        let prone = entity
            .infantry
            .as_ref()
            .is_some_and(|infantry| infantry.is_prone());
        //5206DE skips the first GetFireError.5209AF asks the second only
        // at exact equality, including signed/out-of-range retained stages.
        if entity.native_stage().value() != infantry_fire_frame(obj, sequences, weapon_index, prone)
        {
            return None;
        }
        reach.infantry_fire_frame = true;
    }

    // `GetFireError` (vt+0x3C0, `fire_error`) with `SelectWeapon(Target)` and
    // the range check, asked where each class's fire routine asks it:
    // `UnitClass::Fire_At_Target @ 0x00736E3A`, `InfantryClass::
    // Fire_At_Target @ 0x005206F3` (`0x005209DE` on the fire frame) and
    // `AircraftClass::Mission_Attack @ 0x0041832E`. Each routine then acts on
    // the code as below. A building shoots only a FireAt its own visit asked
    // for this frame (`techno_ai::building_missions`): Mission_Attack's FireAt
    // arm, whose GetFireError (`0x0044B00F`) answered OK, or
    // ProcessDelayedFire's expiry, whose GetFireError (`0x00450476`) did;
    // neither is asked again.
    let subject = |world: &Simulation,
                   answer: &mut dyn FnMut(&fire_error_world::FireSubject<'_>)| {
        if let Some(firer) = world.substrate.entities.get(snap.stable_id) {
            answer(&fire_error_world::FireSubject {
                world,
                rules,
                overlay_registry,
                fog: fog_enabled.then(|| world.fog.fire_view(world.house_alliances.as_map())),
                firer,
                obj,
                target: Some(snap.target),
                weapon_index,
            });
        }
    };
    let code = if snap.category == EntityCategory::Structure {
        snap.building_shot?;
        fire_error::FireError::Ok
    } else {
        let mut code = None;
        subject(&*world, &mut |s| code = Some(s.fire_error(true)));
        code?
    };
    let direction = crate::sim::movement::turret::facing_toward_lepton(
        snap.pos_rx,
        snap.pos_ry,
        snap.sub_x,
        snap.sub_y,
        target_rx,
        target_ry,
        target_sub_x,
        target_sub_y,
    );
    match snap.category {
        // The table at `0x00737148`.
        EntityCategory::Unit => match code {
            // Case 2 (`0x00736F78..0x0073701C`): a turretless vehicle
            // (`UnitType+0xE11`, which ReadINI sets to !`Turret=` at
            // `0x00747759`) with no NavCom (`+0x5A4`, `0x00736FB6`) whose
            // locomotor's Is_Moving (ILocomotion+0x10, `0x00736FE1`) is false
            // turns its hull toward the target at its own `ROT=` (`0x00737004`)
            // and copies the hull's destination into the turret slot
            // (`0x0073701C`). A pending order is neither. A turret follows its
            // target through the turret sweep.
            fire_error::FireError::Facing => {
                if snap.barrel_facing.is_none()
                    && world
                        .substrate
                        .entities
                        .get(snap.stable_id)
                        .is_some_and(|firer| {
                            firer.navigation.nav_com().is_none()
                                && crate::sim::movement::motion_query::is_moving(firer)
                                    != Some(true)
                        })
                    && let Some(update) = out
                        .unit_facing
                        .iter_mut()
                        .find(|u| u.entity_id == snap.stable_id)
                {
                    update.hull_destination = Some(direction);
                }
            }
            // Case 5 (`0x00736E7E`).
            fire_error::FireError::Illegal => {
                if heal_weapon_drops_target(
                    world,
                    rules,
                    selected.as_ref().map(|selected| selected.weapon),
                    snap.target,
                    EntityCategory::Unit,
                ) {
                    out.remove_attack.push(snap.stable_id);
                }
            }
            // Case 9 (`0x00737023`): surface only in range (`CanFireAt
            // 0x006F77B0`).
            fire_error::FireError::Cloaked => {
                let mut in_range = false;
                subject(&*world, &mut |s| in_range = s.in_range());
                if in_range {
                    uncloak_to_fire(
                        &world.session,
                        &mut world.substrate,
                        &mut world.sound_events,
                        rules,
                        obj,
                        snap.stable_id,
                        sound_enabled,
                    );
                }
            }
            // Case 6 (`0x00737054`): a CANT shot clears a spawner's targets
            // (`SpawnManagerClass::ClearAllTargets 0x006B7BB0`).
            fire_error::FireError::Cant => {
                crate::sim::spawn_manager::clear_all_spawn_targets(
                    world,
                    snap.stable_id,
                    rules,
                    overlay_registry,
                    effects,
                );
            }
            _ => {}
        },
        EntityCategory::Infantry => {
            if firing_at_entry {
                if code != fire_error::FireError::Ok {
                    world
                        .substrate
                        .entities
                        .get_mut(snap.stable_id)?
                        .mission_leaf
                        .set_foot_firing_sequence(0);
                    world.infantry_fire_refused_action(snap.stable_id, rules, effects);
                }
            } else {
                match code {
                    fire_error::FireError::Illegal => {
                        if heal_weapon_drops_target(
                            world,
                            rules,
                            selected.as_ref().map(|selected| selected.weapon),
                            snap.target,
                            EntityCategory::Infantry,
                        ) {
                            out.remove_attack.push(snap.stable_id);
                        }
                    }
                    fire_error::FireError::Cloaked => {
                        uncloak_to_fire(
                            &world.session,
                            &mut world.substrate,
                            &mut world.sound_events,
                            rules,
                            obj,
                            snap.stable_id,
                            sound_enabled,
                        );
                    }
                    _ => {}
                }
            }
        }
        // A delayed shot is dropped on any refusal (`0x004504D7`); an ordinary
        // one's codes were acted on by Mission_Attack (`0x0044B728`).
        EntityCategory::Structure => {}
        // An aircraft fires only in Mission_Attack's strike states, which act
        // on their codes in `aircraft_release`.
        EntityCategory::Aircraft => {}
    }
    reach.code = Some(code);
    if code != fire_error::FireError::Ok {
        return None;
    }
    if infantry_fire && !firing_at_entry {
        let actor = world.substrate.entities.get(snap.stable_id)?;
        let fire_fly =
            obj.jumpjet && crate::sim::movement::infantry_action::uses_jumpjet_locomotor(actor);
        let requested = if fire_fly {
            crate::sim::movement::infantry_action::DO_FIRE_FLY
        } else {
            infantry_fire_action(
                sequences,
                weapon_index,
                actor
                    .infantry
                    .as_ref()
                    .is_some_and(|infantry| infantry.is_prone()),
                actor.infantry_deploy_doing(),
            )
        };
        //5208FE may refuse unchanged/noninterruptible/absent actions.520912
        // raises+68D regardless of AL; the retained Doing/Stage still decides
        // when this attempt can discharge.
        // Infantry520866..520884: an Airstrike owner firing secondary
        // restarts only +F8, even when its existing action is noninterruptible.
        if actor.airstrike_link().is_some() && matches!(requested, 40 | 41) {
            world
                .substrate
                .entities
                .get_mut(snap.stable_id)
                .unwrap()
                .set_native_stage_value(0);
        } else if let Err(cause) =
            world.infantry_do_action(snap.stable_id, requested, false, rules, effects)
        {
            log::debug!("infantry {} firing Do_Action: {cause}", snap.stable_id);
        }
        let actor = world.substrate.entities.get_mut(snap.stable_id)?;
        actor.mission_leaf.set_foot_firing_sequence(1);
        actor.body_facing.snap(direction, binary_frame);
        let target_is_navcom = actor.navigation.nav_com() == Some(snap.target.into());
        if target_is_navcom {
            //52093C/520946: shared FootStop4DF0D0 then the concrete+500
            // receiver. That receiver owns any DoAction/Walk Stop callback.
            actor.navigation.stop_moving();
            if let Err(cause) =
                world.foot_stop_driver(snap.stable_id, rules, overlay_registry, effects)
            {
                log::debug!("infantry {} firing Stop_Driver: {cause}", snap.stable_id);
            }
        }
        let actor = world.substrate.entities.get(snap.stable_id)?;
        let prone = actor
            .infantry
            .as_ref()
            .is_some_and(|infantry| infantry.is_prone());
        if actor.native_stage().value() != infantry_fire_frame(obj, sequences, weapon_index, prone)
        {
            return None;
        }
        reach.infantry_fire_frame = true;
        //5209C7/5209DE reselect and query live state after action/facing/Stop,
        // even when a frame-zero action can shoot in this same call.
        // Both actor and target are live reads after the synchronous callbacks.
        // Native5209BB/5209D2 reloads Target for SelectWeapon/GetFireError.
        let target = actor.attack_target.target();
        (weapon_index, selected) = combat_weapon::select_weapon_against(
            rules,
            actor,
            obj,
            &combat_weapon::attacker_facts(actor, obj),
            target.as_ref(),
            world,
            &world.interner,
            world.resolved_terrain.as_ref(),
            fog_enabled.then_some(world.house_alliances.as_map()),
        )?;
        let code = fire_error_world::FireSubject {
            world,
            rules,
            overlay_registry,
            fog: fog_enabled.then(|| world.fog.fire_view(world.house_alliances.as_map())),
            firer: actor,
            obj,
            target,
            weapon_index,
        }
        .fire_error(true);
        reach.code = Some(code);
        if code != fire_error::FireError::Ok {
            world
                .substrate
                .entities
                .get_mut(snap.stable_id)?
                .mission_leaf
                .set_foot_firing_sequence(0);
            world.infantry_fire_refused_action(snap.stable_id, rules, effects);
            return None;
        }
    }
    // Keep emission's FLH/heading/prone snapshot in the native post-action
    // window. Other classes retain their existing caller-owned snapshot.
    let firing_snapshot;
    let snap = if infantry_fire {
        let actor = world.substrate.entities.get(snap.stable_id)?;
        firing_snapshot =
            build_attacker_snapshot(actor, actor.attack_target.target()?, snap.garrison.clone());
        &firing_snapshot
    } else {
        snap
    };
    let selected = selected?;
    let (target_coords, target_type_ref) = if infantry_fire {
        match snap.target {
            TargetKind::Entity(id) => {
                let coords = resolve_target_coords(&snap.target, world)?;
                let type_ref = world
                    .substrate
                    .entities
                    .get(id)
                    .map(|target| target.type_ref())
                    .or_else(|| {
                        world
                            .terrain_objects
                            .objects()
                            .get(&id)
                            .map(|target| target.type_ref())
                    })?;
                (coords, type_ref)
            }
            TargetKind::Cell(..) => (resolve_target_coords(&snap.target, world)?, snap.type_id),
        }
    } else {
        (
            (target_rx, target_ry, target_sub_x, target_sub_y),
            target_type_ref,
        )
    };

    Some(AdmittedFire {
        snap: snap.clone(),
        obj,
        selected,
        target_coords,
        target_type_ref,
        is_garrison,
    })
}

/// A unit whose turn still reaches the firing update: alive, and not warped
/// out. `UnitClass::AI` returns first on vt+0x1D4, BeingWarpedOut `+0x270`
/// (`0x007362FB..0x0073635A`), which a Temporal chain and the teleport's
/// warp-out both set, and gates the update on IsAlive (`+0x90`,
/// `0x007365BB`), so a crashing Unit's wreck still reaches it at Health 0.
fn unit_reaches_fire_update(world: &Simulation, id: u64) -> bool {
    world.substrate.entities.get(id).is_some_and(|entity| {
        entity.category() == EntityCategory::Unit && entity.is_ai_alive() && !entity.is_warped_out()
    })
}

/// `Fire_At_Target`'s ILLEGAL arm (Unit `0x00736E7E`, Infantry `0x00520721`):
/// a healing weapon (`0x006F3970` of the selected slot, Damage + AmbientDamage
/// below zero) keeps only a damaged target of the firer's own class (health
/// ratio below `Rules+0x16F8`, 1.0; a NaN ratio keeps); any other target is
/// dropped. Every other weapon keeps it for `TechnoClass::AI`'s 16-frame
/// check.
fn heal_weapon_drops_target(
    world: &Simulation,
    rules: &RuleSet,
    weapon: Option<&WeaponType>,
    target: TargetKind,
    class: EntityCategory,
) -> bool {
    if weapon.is_none_or(|weapon| weapon.damage.wrapping_add(weapon.ambient_damage) >= 0) {
        return false;
    }
    let TargetKind::Entity(id) = target else {
        return true;
    };
    let Some(target) = world
        .substrate
        .entities
        .get(id)
        .filter(|target| target.category() == class)
    else {
        return true;
    };
    let strength = world
        .interner
        .object_type(target.type_ref(), rules)
        .map_or(0, |object| object.strength);
    target.health.is_full(strength)
}

/// StartUncloaking (vt+0x45C) from a building's Mission_Attack CLOAKED arm
/// (`0x0044B284`).
pub(crate) fn start_uncloaking_to_fire(world: &mut Simulation, rules: &RuleSet, id: u64) {
    let sound_enabled = sound_enabled(world);
    let Some(obj) = world
        .substrate
        .entities
        .get(id)
        .and_then(|entity| world.interner.object_type(entity.type_ref(), rules))
    else {
        return;
    };
    uncloak_to_fire(
        &world.session,
        &mut world.substrate,
        &mut world.sound_events,
        rules,
        obj,
        id,
        sound_enabled,
    );
}

/// `TechnoClass::Uncloak` (vt+0x45C, `0x007036C0`) with its sound, each
/// class's CLOAKED (9) arm.
fn uncloak_to_fire(
    session: &ScenarioSession,
    substrate: &mut ObjectSubstrate,
    sound_events: &mut FrameChannel<SimSoundEvent>,
    rules: &RuleSet,
    obj: &ObjectType,
    id: u64,
    sound_enabled: bool,
) {
    let binary_frame = session.binary_frame();
    // `UnitClass::Fire_At_Target @ 0x00736DF0` case 9 invokes virtual
    // `StartUncloaking +0x45C @ 0x007036C0` after rechecking CanFireAt.
    let start = substrate
        .entities
        .get_mut(id)
        .and_then(|entity| entity.cloak.as_mut())
        .map(|cloak| {
            cloak.start_uncloaking(
                binary_frame as i32,
                obj.cloaking_speed,
                rules.general.cloaking_stages,
                false,
            )
        });
    if start.is_some_and(|result| result.play_sound) && sound_enabled {
        emit_configured_cloak_sound(substrate, sound_events, id, rules);
    }
}

/// Call-local result of admission and fire-action work.
/// This is call-local data, never a saved permission to fire on a later frame.
/// Native Mission_Attack418403 checks legality once before its burst loop;
/// separating emission lets the aircraft caller reselect from live state
/// without repeating admission for every FireAt call.
pub(super) struct AdmittedFire<'a> {
    pub(super) snap: AttackerSnapshot,
    pub(super) obj: &'a ObjectType,
    pub(super) selected: combat_weapon::SelectedWeapon<'a>,
    pub(super) target_coords: (u16, u16, SimFixed, SimFixed),
    pub(super) target_type_ref: InternedId,
    pub(super) is_garrison: bool,
}

/// Object `id`'s GetCoords (`vt+0x48`,
/// [`crate::sim::movement::ground_pose::object_get_coords`]) as a projectile
/// coordinate.
pub(super) fn object_get_coords(world: &Simulation, id: u64) -> Option<ProjectileCoord> {
    let coord = crate::sim::movement::ground_pose::target_get_coords(
        TargetKind::Entity(id),
        world,
        world
            .resolved_terrain
            .as_ref()
            .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
            .as_ref(),
    )?;
    Some(ProjectileCoord::new(coord.x, coord.y, coord.z))
}

/// `TechnoClass::ReceiveDamage @ 0x00702A58..0x00702B2F`, after
/// `ShouldRetaliate` agreed: the retaliation is issued only when the source is
/// in range of the weapon selected against it (vt+0x3A8, `0x00702A69`), or
/// the owner is a computer (`0x00702A7D`), or the source stands within the
/// victim's sight: `ftol(Sqrt_Approx(dx² + dy² + dz²))` between the two
/// GetCoords at most `(Sight + 0.5) * 256` (`0x00702A8A..0x00702B2C`). A
/// player's unit therefore does not charge an out-of-sight V3 or Grand Cannon.
pub(super) fn retaliation_reaches(
    world: &Simulation,
    rules: &RuleSet,
    victim_id: u64,
    source_id: u64,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> bool {
    let entities = &world.substrate.entities;
    let (Some(victim), Some(source)) = (entities.get(victim_id), entities.get(source_id)) else {
        return false;
    };
    let (Some(victim_type), Some(source_type)) = (
        world.interner.object_type(victim.type_ref(), rules),
        world.interner.object_type(source.type_ref(), rules),
    ) else {
        return false;
    };
    // Original702A69 asks CanFireAt(vt+3A8), including the shared wall
    // query, before the human Sight fallback at702A7D..702B2C.
    let in_range = super::fire_error_world::FireSubject {
        world,
        rules,
        overlay_registry,
        fog: Some(world.fog.fire_view(world.house_alliances.as_map())),
        firer: victim,
        obj: victim_type,
        target: Some(TargetKind::Entity(source_id)),
        weapon_index: combat_targeting::retaliation_weapon_index(
            world,
            rules,
            victim,
            victim_type,
            source,
            source_type,
        ),
    }
    .in_range();
    let human = world.owner_is_human(victim.owner());
    if in_range || !human {
        return true;
    }
    let (Some(from), Some(to)) = (
        object_get_coords(world, victim_id),
        object_get_coords(world, source_id),
    ) else {
        return false;
    };
    let distance =
        crate::util::native_x87::distance_3d_leptons([to.x, to.y, to.z], [from.x, from.y, from.z]);
    // (Sight + 0.5) * 256 >= distance, exactly: 2 * distance <= (2 * Sight + 1) * 256.
    2 * i64::from(distance) <= (2 * i64::from(victim_type.sight) + 1) * 256
}

/// Where `TechnoClass::FireAt @ 0x006FDD50` launches from: `[ESP+0x44]`, which
/// the bullet launch (`BulletClass::Fire`, vt+0x1F0 at `0x006FF014`), the
/// report (`0x006FF38F`) and the muzzle anim (`0x006FF3C2`) all read.
struct FireAtLaunchSource {
    /// The fire coordinate (`GetFLH`, vt+0xB0, `0x006FE268`), replaced for a
    /// `Dropping=` projectile (BulletType `+0x29C`) by the firer's GetCoords
    /// (vt+0x48, `0x006FE2D8..0x006FE2FE`; `0x006FE960` re-reads the same
    /// value). Such a shot starts, flashes and sounds at the firer's centre.
    /// `Arcing=` is `+0x29B` and does not move the source: a cannon shell
    /// leaves its barrel. The only retail `Dropping=` projectile,
    /// `[V3AirburstP]`, belongs to no weapon in use.
    coord: ProjectileCoord,
    /// `coord.y` minus the Y of the object coordinate (vt+0xAC) the fire
    /// coordinate's offset was added to: a building's muzzle-anim `ZAdjust`
    /// input (`0x006FF3E2..0x006FF40B`), taken from the launch source.
    offset_y: i32,
}

fn fireat_launch_source(
    world: &Simulation,
    rules: &RuleSet,
    snap: &AttackerSnapshot,
    fire: &super::fire_coord::FireCoordinate,
    weapon: &crate::rules::weapon_type::WeaponType,
) -> FireAtLaunchSource {
    let dropping = weapon
        .projectile
        .as_deref()
        .and_then(|id| rules.projectile(id))
        .is_some_and(|projectile| projectile.dropping);
    match object_get_coords(world, snap.stable_id).filter(|_| dropping) {
        Some(coord) => FireAtLaunchSource {
            coord,
            offset_y: fire.offset_y + (coord.y - fire.coord.y),
        },
        None => FireAtLaunchSource {
            coord: fire.coord,
            offset_y: fire.offset_y,
        },
    }
}

/// What a shot's delta aims at and how fast it launches.
struct FireAtLaunchAim {
    /// The delta's endpoint (`0x006FE62F` -> `0x0070BCB0`): the current
    /// target's vt+0x58 coordinate, led along its facing when it is a moving
    /// UnitClass (`launch::lead_aim`).
    aim: ProjectileCoord,
    /// `WeaponTypeClass::GetSpeed` (`0x006FE53A`) at FireAt's distance from
    /// the launch source to the target coordinate (`0x006FE4F6..0x006FE537`).
    speed: i32,
}

/// `TechnoClass::FireAt`'s aim and launch speed for the shot `snap` fires
/// with `weapon` from `source` at `target_coord` (the target's unled
/// vt+0x58/vt+0xA4 coordinate). Native execution of the numeric leaves:
/// `tools/projectile_oracle/fireat_speed.py`. The aim (`0x0070BCB0`) reads
/// the call-local TarCom authority independently of FireAt's argument. The
/// argument's coordinate supplies only the launch-speed distance here
/// (`0x006FE1DA..0x006FE218`, `0x006FE53A`).
fn fireat_launch_aim(
    world: &Simulation,
    rules: &RuleSet,
    snap: &AttackerSnapshot,
    weapon: &crate::rules::weapon_type::WeaponType,
    source: ProjectileCoord,
    target_coord: ProjectileCoord,
    tarcom: Option<TargetKind>,
) -> FireAtLaunchAim {
    use crate::sim::projectile::launch::{
        LaunchSpeedProjectile, fireat_launch_distance, weapon_launch_speed,
    };
    let speed_projectile = |weapon: &crate::rules::weapon_type::WeaponType| {
        weapon
            .projectile
            .as_deref()
            .and_then(|id| rules.projectile(id))
            .map(|projectile| LaunchSpeedProjectile {
                rot: projectile.rot,
                floater: projectile.floater,
            })
    };
    let speed = weapon_launch_speed(
        weapon.speed,
        speed_projectile(weapon),
        rules.general.gravity,
        fireat_launch_distance(source, target_coord),
    );
    FireAtLaunchAim {
        aim: super::aim_coord::led_target_coordinate(world, rules, snap.stable_id, tarcom),
        speed,
    }
}

/// `TechnoClass::FireAt`'s damage build (`damage::attacker::fire_damage`)
/// for the shot `snap` fires with `weapon`. The firer's own rank and type
/// decide the FIREPOWER stage; for a garrison shot that is the building,
/// exactly as native's `this` is.
///
/// House+188 comes from the shared SetDifficulty owner (6FE33D); Techno+160
/// is the firer's live firepower_multiplier, raised by the Firepower crate.
///
/// A passenger of an `OpenTopped=` transport (retail: `[BFRT]`) fires with
/// `+0x82` set (`PerCellProcess 0x0051A45E`/`0x0073A75D` ->
/// `SetInOpenTransport 0x00710470`), which takes the open-topped stage.
fn fireat_damage(
    world: &Simulation,
    rules: &RuleSet,
    snap: &AttackerSnapshot,
    obj: &ObjectType,
    weapon: &crate::rules::weapon_type::WeaponType,
    is_garrison: bool,
) -> i32 {
    use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};
    let firer = world.substrate.entities.get(snap.stable_id);
    let f32_bits = |value: f32| NativeF32Bits::from_bits(value.to_bits());
    let multipliers = &rules.garrison_rules;
    let stages = damage::attacker::FireDamageStages {
        house_firepower: firer
            .and_then(|firer| world.houses.get(&firer.owner()))
            .map_or(NativeF64Bits::ONE, |house| house.firepower_bias()),
        unit_firepower: firer.map_or(NativeF64Bits::ONE, |firer| {
            firer.crate_multipliers.firepower()
        }),
        rank_firepower: self::veterancy::has_weapon_ability(
            self::veterancy::rank_from_u16(snap.veterancy),
            obj,
            crate::rules::object_type::Ability::Firepower,
        )
        .then(|| NativeF64Bits::from_bits(rules.general.veteran_combat.to_bits())),
        // `vt+0x400`: BuildingClass `0x00458DD0`, CanBeOccupied &&
        // CanOccupyFire && an occupant; false for every other class.
        occupied: is_garrison.then(|| f32_bits(multipliers.occupy_damage_multiplier)),
        // `+0x2E4` on a non-building: a unit installed in a Tank Bunker.
        bunkered: firer
            .filter(|firer| firer.category() != EntityCategory::Structure)
            .and_then(|firer| firer.bunker.link().installed_in())
            .map(|_| f32_bits(multipliers.bunker_damage_multiplier)),
        open_topped: firer
            .filter(|firer| firer.passenger_role.in_open_transport())
            .map(|_| f32_bits(multipliers.open_topped_damage_multiplier)),
    };
    damage::attacker::fire_damage(
        weapon.damage,
        weapon.is_sonic || weapon.use_fire_particles,
        &stages,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FireDisclosurePath {
    Shot,
    Spawner,
}

/// FireAt6FDE73..6FDF5A /6FF66C..6FF73D. Both routes share the
/// victim's Map5673A0(radius3, final0) and its shifted-XYZ border6.
/// Spawner's fresh-XYZ border4 also runs when the victim/weapon leaf gates
/// fail; normal fire requires those gates for both calls. Each human map
/// represents that client's PlayerPtr; computer Houses have no client shroud.
fn reveal_on_fire(
    world: &mut Simulation,
    rules: &RuleSet,
    firer_id: u64,
    target: TargetKind,
    enabled: bool,
    path: FireDisclosurePath,
) {
    let TargetKind::Entity(target_id) = target else {
        return;
    };
    let victim = if let Some(target) = world.substrate.entities.get(target_id) {
        Some(target.owner())
    } else if world.terrain_objects.objects().contains_key(&target_id) {
        // Terrain ctor71BBFF installs7F522C; slot+3C=4104A0 returnsNULL.
        // This is a real Object target: Spawner still reaches its border.
        None
    } else {
        return;
    };
    let mut viewers = if path == FireDisclosurePath::Spawner {
        // No callback or RNG occurs in this border. Use stable House order
        // for the logical client planes and the shared canonical Dummy.
        let mut viewers: Vec<_> = world
            .houses
            .order()
            .iter()
            .copied()
            .filter(|&house| world.owner_is_human(house))
            .collect();
        let mut extra: Vec<_> = world
            .houses
            .keys()
            .copied()
            .filter(|house| !world.houses.order().contains(house) && world.owner_is_human(*house))
            .collect();
        extra.sort_unstable();
        viewers.extend(extra);
        viewers
    } else if let Some(victim) = victim.filter(|&victim| enabled && world.owner_is_human(victim)) {
        vec![victim]
    } else {
        return;
    };
    viewers.dedup();
    let context = crate::sim::world::UninitContext::new(rules, None);
    let config = world.sight_reveal_config(rules);
    let heights = world.sight_height_grid(&config);
    for house in viewers {
        let Some(firer) = world.substrate.entities.get(firer_id) else {
            return;
        };
        // Both fire tails and their admission query dispatch virtual+48:
        // Object5F65A0 / Building447AC0, through the existing coordinate owner.
        let coord = crate::sim::movement::ground_pose::object_get_coords(
            firer,
            world.resolved_terrain.as_ref(),
        );
        let owned = firer.owner() == house;
        let discovered = world.session.game_mode_nonzero()
            || (world.session.current_house() == Some(house)
                && firer.discovery.discovered_by_current_house());
        if owned || discovered {
            if owned && firer.category() == EntityCategory::Aircraft {
                continue;
            }
            //6FDE9A/6FF692: IsShrouded, else5865E0 (constant false).
            let shrouded = match world.resolved_terrain.as_ref() {
                Some(terrain) => {
                    let cells = crate::map::resolved_terrain::NativeCellQuery::isolated(terrain);
                    crate::sim::vision::coordinate_is_shrouded(&cells, coord, &|cell| {
                        Ok(match cell {
                            crate::map::cell_index::NativeCellIdentity::Real(index) => {
                                let cell = &terrain.cells()[index];
                                world.fog.is_cell_revealed(house, cell.rx, cell.ry)
                            }
                            crate::map::cell_index::NativeCellIdentity::Dummy => false,
                        })
                    })
                    .unwrap_or(false)
                }
                None => {
                    !world
                        .fog
                        .is_cell_revealed(house, firer.position.rx(), firer.position.ry())
                }
            };
            if !shrouded {
                continue;
            }
        }
        // Existing area/spiral geometry accepts cell-centred coordinates.
        // Subcell/negative raw XYZ correspondence remains the surrounding
        // Map geometry residual, rather than a second FireAt arithmetic port.
        let origin = ((coord.x / 256) as u16, (coord.y / 256) as u16, coord.z);
        if enabled && victim == Some(house) {
            let local_origin = world.reveal_area(
                &[house],
                (origin.0, origin.1),
                origin.2,
                3,
                Some(&heights),
                false,
                &config,
                context,
                crate::sim::vision::MapRevealProjection::Fire,
                Some(house),
            );
            if let Some(local_origin) = local_origin {
                Simulation::refresh_sight_border(
                    &mut world.fog,
                    &world.resolved_terrain,
                    house,
                    local_origin,
                    6,
                    context,
                );
            }
        }
        //6FDF4D/6FF738: independent of Map's center/radius admission.
        Simulation::refresh_sight_border(
            &mut world.fog,
            &world.resolved_terrain,
            house,
            origin,
            4,
            context,
        );
    }
}

/// `TechnoClass::GetROF` (vt+0x318) for a shot: of the weapon GetWeapon
/// answers (a garrison's next occupant's), at the stepped burst index. The rank
/// and class are the firer's (a garrison shot reads the building's). FireAt
/// created the Spark system just before (`0x006FF1AD..0x006FF1F5`);
/// GetROF reads the actual +308 link, including a retained earlier system.
/// RESIDUAL: Fire/Railgun systems still use the firing weapon's flags as
/// their preexisting launch approximation; their retained links are unported.
fn fireat_get_rof(
    scenario_rng: &mut SimRng,
    substrate: &ObjectSubstrate,
    houses: &Houses,
    rules: &RuleSet,
    snap: &AttackerSnapshot,
    obj: &ObjectType,
    weapon: &WeaponType,
    rof_weapon: Option<&WeaponType>,
    next_index: i32,
) -> i32 {
    let live = super::rof::LiveParticles {
        spark: substrate
            .entities
            .get(snap.stable_id)
            .is_some_and(|firer| firer.spark_particle_system.system().is_some()),
        fire: weapon.use_fire_particles,
        railgun: weapon.is_railgun,
    };
    snapshot_get_rof(
        scenario_rng,
        substrate,
        houses,
        rules,
        snap,
        obj,
        rof_weapon,
        live,
        next_index,
    )
}

/// `TechnoClass::GetROF` (vt+0x318) of the object `snap` describes, for
/// `rof_weapon` at `burst_index` with the `live` particle systems it holds:
/// FireAt's shot ([`fireat_get_rof`]) and the C4 plant's `GetROF(1)`
/// (`sim::c4_plant`), also the receiving gunner's GetROF7/0
/// (`sim::passenger`).
pub(crate) fn snapshot_get_rof(
    scenario_rng: &mut SimRng,
    substrate: &ObjectSubstrate,
    houses: &Houses,
    rules: &RuleSet,
    snap: &AttackerSnapshot,
    obj: &ObjectType,
    rof_weapon: Option<&WeaponType>,
    live: super::rof::LiveParticles,
    burst_index: i32,
) -> i32 {
    let firer = substrate.entities.get(snap.stable_id);
    super::rof::get_rof(
        &super::rof::RofQuery {
            // RESIDUAL: a building's own Ammo (`+0x2FC`) is not kept. The
            // retail buildings with `Ammo=` (CAAIRP, CAPOWR: 5) are unarmed,
            // so none reaches GetROF.
            building_ammo: None,
            weapon: rof_weapon,
            live,
            burst_index,
            unit_burst_delays: (snap.category == EntityCategory::Unit).then_some(obj.burst_delays),
            house_rof: houses
                .get(&snap.owner)
                .map_or(crate::util::native_x87::NativeF64Bits::ONE, |house| {
                    house.rof_bias()
                }),
            rof_ability: self::veterancy::has_weapon_ability(
                self::veterancy::rank_from_u16(snap.veterancy),
                obj,
                crate::rules::object_type::Ability::Rof,
            ),
            veteran_rof: rules.general.veteran_rof,
            occupants: snap.garrison.as_ref().map(|gs| gs.occupant_count as i32),
            bunkered: snap.category != EntityCategory::Structure
                && firer.is_some_and(|firer| {
                    matches!(
                        firer.bunker.link(),
                        crate::sim::docking::bunker_link::BunkerLink::Installed(_)
                    )
                }),
            occupy_rof_multiplier: rules.garrison_rules.occupy_rof_multiplier,
            bunker_rof_multiplier: rules.garrison_rules.bunker_rof_multiplier,
        },
        scenario_rng,
    )
}

/// Existing FireAt delivery and bookkeeping, shared by the world receiver.
/// The caller still owns legality, fire-action timing and inline damage commit.
/// Answers the bullet `TechnoClass::FireAt @ 0x006FDD50` returns: the one
/// launched, or `None` where it returns NULL (Suicide, a refused sonic shot, the
/// spawner, drain and DiskLaser arms, and a launch with no solution).
pub(super) fn emit_admitted_fire(
    world: &mut Simulation,
    rules: &RuleSet,
    shot: AdmittedFire<'_>,
    binary_frame: u32,
    out: &mut CombatEmit,
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> Option<u64> {
    // Infantry51DF70 clears68D for every direct FireAt caller, including
    // Guard521432 and a launch subsequently refused by Techno6FDD50.
    if shot.snap.category == EntityCategory::Infantry
        && let Some(actor) = world.substrate.entities.get_mut(shot.snap.stable_id)
    {
        actor.mission_leaf.set_foot_firing_sequence(0);
    }
    let AdmittedFire {
        snap,
        obj,
        selected,
        target_coords: (target_rx, target_ry, target_sub_x, target_sub_y),
        target_type_ref,
        is_garrison,
    } = shot;
    let snap = &snap;
    let weapon = selected.weapon;
    // ObjectCtor5F3B37 sets Abstract flag0x2 (Object, not Foot0x4).
    // FireAt6FDD88..6FDDBE refuses any Object with +81 InLimbo before
    // Suicide or launch work. Cells bypass this gate; missing IDs represent
    // retired Objects. Infantry's pre-base firing-latch clear stays above it.
    if let TargetKind::Entity(id) = snap.target {
        let in_limbo = world
            .substrate
            .entities
            .get(id)
            .map(|target| target.lifecycle.in_limbo())
            .or_else(|| {
                world
                    .terrain_objects
                    .objects()
                    .get(&id)
                    .map(|target| !target.is_live())
            })
            .unwrap_or(true);
        if in_limbo {
            return None;
        }
    }
    if weapon.suicide {
        // Techno6FDDC0..6FDE0E: the local packet is type Strength, not
        // current Health or weapon Damage. The real self receiver completes
        // its nested DeathWeapon and class death before FireAt returns NULL;
        // ordinary launch/GetROF/rearm/last-fire work is never reached.
        // Original execution: tools/spatial_oracle/suicide_fire.json.
        let c4 = world.interner.intern(&rules.bridge_warheads.c4_name);
        let event = EntityDamageEvent::direct_receiver(
            snap.stable_id,
            obj.strength,
            0,
            RAD_NO_ATTACKER,
            None,
            c4,
            ReceiverCallFlags {
                ignore_defenses: true,
                arg6: false,
            },
        );
        world
            .commit_direct_damage_receiver(rules, overlay_registry, event, effects)
            .retain_frame_notifications(&mut out.effects);
        return None;
    }
    if weapon.is_sonic && world.active_wave_id(snap.stable_id).is_some() {
        return None;
    }
    // 6FDE36..6FDE48: direct FireAt callers also refuse a flagged weapon
    // while the actual Techno+308 system remains retained. GetFireError is
    // not the only admission caller, and a guessed expiry tick is insufficient.
    if weapon.use_spark_particles
        && world
            .substrate
            .entities
            .get(snap.stable_id)
            .is_some_and(|firer| firer.spark_particle_system.system().is_some())
    {
        return None;
    }

    // Spawner weapon: gamemd's Fire_At short-circuits here. It calls
    // `SpawnManagerClass::SetTarget` and returns NULL — no bullet, no damage,
    // no detonation effects, and no rearm timer write (the rearm write lives
    // further down Fire_At, past this branch). Because the branch returns above
    // the first random draw as well as above bullet allocation, a spawner fire
    // consumes zero scenario-RNG draws natively.
    //
    // GetFireError T35 (`0x006FC606`) has already refused a launch from a
    // bridge-side cell (`IsOnBridge_ForFiring 0x00703B10`), from a paralysed
    // firer, and with no spawn out of regeneration (REARM).
    if weapon.spawner {
        let alive = world
            .substrate
            .entities
            .get(snap.stable_id)
            .and_then(|e| e.spawn_manager.as_ref())
            .map(|m| m.count_alive_spawns())
            .unwrap_or(0);
        if alive > 0 {
            out.spawn_target_updates.push((snap.stable_id, snap.target));
        }
        //6FDF30/4D occurs even though this path returns no Bullet. It adds
        // no Scenario draw, rearm write or detach; SetTarget keeps its owner.
        reveal_on_fire(
            world,
            rules,
            snap.stable_id,
            snap.target,
            weapon.reveal_on_fire,
            FireDisclosurePath::Spawner,
        );
        return None;
    }

    // `TechnoClass::Fire_At @ 0x006FDF5D..0x006FDF9D`: a `DrainWeapon=yes`
    // weapon (`WeaponType+0x142`) against a Techno whose type is
    // `Drainable=yes` (`+0x5EF`, read at `0x006FDF7B`) calls the link
    // installer `0x0070FD70`, then `Assign_Target(NULL)` (`[vtable+0x3C8]`
    // at `0x006FDF97`, applied where the link is installed) and returns NULL
    // — no bullet, no rearm, no report — so the shot below never runs. A
    // DrainWeapon aimed at anything else takes the `0x006FDE03` exit
    // (its identity is UNCHECKED; unreachable in stock, where the selection
    // ladder's arm K only picks the DrainWeapon against a Drainable target).
    if weapon.drain_weapon {
        if let TargetKind::Entity(target_id) = snap.target
            && world
                .interner
                .object_type(target_type_ref, rules)
                .is_some_and(|target_obj| target_obj.drainable)
        {
            out.drain_links.push((snap.stable_id, target_id));
        }
        return None;
    }

    // `0x006FE095..0x006FE245`: the target argument and its coordinate. A
    // `SprayAttack=` type (TechnoType `+0x691`) aims at one of eight fixed
    // offsets around itself instead, the first picked by a Scenario draw;
    // retail sets the key on no type, so that branch is dormant and not
    // ported. Otherwise an `AreaFire=` weapon (`+0x150`,
    // `0x006FE140..0x006FE1D8`) fires at the cell under the firer: GetCoords
    // (vt+0x48) XY /256 toward zero, packed to 16 bits, Map `0x005657A0`.
    // That cell's own GetCoords (`0x00486840`: centre XY and ground height,
    // not vt+0x58's bridge deck) becomes the target coordinate, and Map
    // `0x00565730` looks the same cell up again as the new target. Every later
    // step reads these: CreateBullet, the Inviso placement, the beams and
    // RevealOnFire, which a Cell skips (`0x006FF6D4`). TarCom, which GetFLH and
    // the estimate debit read, keeps the victim. Infantry GetFireError refuses
    // an AreaFire shot off its own cell (`0x0051CB08`); a Unit fires it from
    // range, so a Chaos Drone's gas rises around the drone, not the victim. A
    // fixture without terrain has no cell and keeps the victim.
    let area_fire_cell = (weapon.area_fire && !obj.spray_attack)
        .then(|| object_get_coords(world, snap.stable_id))
        .flatten()
        .and_then(|coord| world.packed_map_cell(coord.x, coord.y));
    // The Object local FireAt took at entry (`0x006FDD88..0x006FDDA1`),
    // which the retarget leaves alone; the Inviso OnBridge copy reads it.
    let entry_target = snap.target;
    let area_fire_snap;
    let (snap, (target_rx, target_ry, target_sub_x, target_sub_y)) = match area_fire_cell {
        Some((rx, ry)) => {
            area_fire_snap = AttackerSnapshot {
                target: TargetKind::Cell(rx, ry),
                ..snap.clone()
            };
            (&area_fire_snap, cell_center_coords(rx, ry))
        }
        None => (snap, (target_rx, target_ry, target_sub_x, target_sub_y)),
    };

    // Fire one shot!
    //
    // The burst index is needed twice — the fire coordinate mirrors its lateral
    // offset on odd shots, and the burst state machine advances on it — so it is
    // resolved once here.
    let burst = world
        .substrate
        .entities
        .get(snap.stable_id)
        .map(|entity| entity.weapon_burst)
        .unwrap_or_default();
    // FLH uses only odd/even parity; retain the signed dword in its owner.
    let burst_index = (burst.index() & 1) as u8;
    let tarcom = fireat_tarcom(world, snap);
    // One fire coordinate per shot: the bullet origin, the muzzle animation and
    // the report sound all take it (`combat::fire_coord`).
    let fire = super::fire_coord::fire_coordinate(
        world,
        rules,
        &super::fire_coord::FireSource {
            tar_com: tarcom,
            ..super::fire_coord::FireSource::from(snap)
        },
        obj,
        selected.index,
        burst_index,
        crate::rules::flh::Flh::default(),
    );
    let launch_source = fireat_launch_source(world, rules, snap, &fire, weapon);
    // Laser width reads +664 inside FireAt, before its caller applies/clears
    // the support multiplier. Preserve that read across the existing deferred
    // bullet payload construction below.
    let prism_support_count = world
        .substrate
        .entities
        .get(snap.stable_id)
        .map_or(0, |entity| entity.prism_support_count.count());
    let warhead = selected.warhead;
    let base_damage = fireat_damage(world, rules, snap, obj, weapon, is_garrison);
    // FireAt6FE460..6FE4EF constructs a retained DiskLaser before GetROF,
    // advances the burst, stores the unhalved rearm, then calls Fire4A71A0
    // and returns NULL. Disk AI owns the later range/crash gate, discharge
    // and report; no Bullet, muzzle/reveal or last-fire tail is reached here.
    // Executable lifecycle controls: tools/spatial_oracle/disk_laser.md.
    if weapon.disk_laser {
        let disk_id = world.construct_disk_laser();
        let rof_weapon = world
            .substrate
            .entities
            .get(snap.stable_id)
            .and_then(|firer| {
                combat_weapon::get_weapon(
                    firer,
                    obj,
                    selected.index,
                    &world.substrate.entities,
                    rules,
                    &world.interner,
                )
            });
        let rof = fireat_get_rof(
            &mut world.scenario_rng,
            &world.substrate,
            &world.houses,
            rules,
            snap,
            obj,
            weapon,
            rof_weapon,
            burst.next_index(),
        );
        if let Some(entity) = world.substrate.entities.get_mut(snap.stable_id) {
            entity.rearm_after_fire(binary_frame as i32, rof);
            entity.weapon_burst.complete_shot(weapon.burst.max(1));
        }
        let weapon_id = world.interner.intern(selected.weapon_id);
        world.fire_disk_laser(
            disk_id,
            Some(snap.stable_id),
            Some(snap.target),
            Some(weapon_id),
            base_damage,
        );
        return None;
    }
    let ProjectileDelivery {
        collision,
        ballistic,
        vertical,
        launch_scatter_is_flak,
        guidance,
        ..
    } = classify_projectile_delivery(weapon, rules);
    // `CreateBullet @ 0x0046B050` (called at `0x006FE55D`) takes the bullet's
    // unique id (`0x00410230`) before the launch math, so a launch that then
    // fails has still spent one.
    let bullet_id = world.substrate.allocate_stable_id();
    let native_unique_id = world.next_native_runtime_id();
    fireat_estimate_debit(world, rules, snap.stable_id, tarcom, obj, weapon);
    let launched = {
        let impact_world_z_leptons =
            attack_world_z_leptons(snap.target, world, world.resolved_terrain.as_ref());
        let origin_world_z_leptons = fire.source_z;
        let impact = ProjectileCoord::new(
            i32::from(target_rx) * 256 + target_sub_x.to_num::<i32>(),
            i32::from(target_ry) * 256 + target_sub_y.to_num::<i32>(),
            impact_world_z_leptons,
        );
        let target = snap.target.projectile_target(world);
        // The bullet leaves from the launch source (the fire coordinate, or for
        // a `Dropping=` projectile the firer's GetCoords, `0x006FE2E2`); the
        // delta aims at the target, led when it is a moving vehicle
        // (`0x0070BCB0`). See `fireat_launch_source`/`fireat_launch_aim`.
        let launch_geometry = fireat_launch_aim(
            world,
            rules,
            snap,
            weapon,
            launch_source.coord,
            // 6FE1EE reads the Building argument's+A4. The launch delta and
            // Bullet::Fire reference below independently read+58 instead.
            // AreaFire's coordinate is its cell's +48 (6FE1A3).
            match area_fire_cell {
                Some((rx, ry)) => crate::sim::projectile::cell_ground_coord(
                    world.resolved_terrain.as_ref(),
                    rx,
                    ry,
                ),
                None => super::fire_coord::effect_target_coordinate(
                    world,
                    rules,
                    snap.target.projectile_target(world),
                )
                .unwrap_or(impact),
            },
            tarcom,
        );
        let origin = launch_source.coord;
        let projectile_type = weapon
            .projectile
            .as_deref()
            .and_then(|projectile_id| rules.projectile(projectile_id));
        // `TechnoClass::FireAt` step order, `0x006FE663`..`0x006FEA52`:
        // resolve the target delta, scatter it, recompute the facing from the
        // scattered delta, clamp the launch speed to half the straight-line
        // distance, and then apply the radial fan and homing/vertical scalar
        // gate. `BulletClass::Fire` keeps the target's own
        // unled coordinate as the bullet's (`0x00468700..0x0046872B`).
        let frozen_target_position = impact;
        let aim = launch_geometry.aim;
        let delta = ProjectileCoord::new(aim.x - origin.x, aim.y - origin.y, aim.z - origin.z);
        let delta = match launch_scatter_is_flak {
            Some(flak) => {
                // vt+0x168 (`TechnoClass::GetWeaponRange @ 0x007012C0`) for
                // the fired weapon: an open-topped firer's passengers cap it,
                // and an occupied building answers its firing occupant's
                // weapon, the one this shot fires.
                let range = world.substrate.entities.get(snap.stable_id).map_or(
                    weapon.range_leptons,
                    |firer| {
                        combat_weapon::weapon_range(
                            firer,
                            obj,
                            selected.index,
                            &world.substrate.entities,
                            rules,
                            &world.interner,
                        )
                    },
                );
                crate::sim::projectile::launch::fireat_launch_scatter(
                    delta,
                    rules.combat_damage.ballistic_scatter,
                    range,
                    flak,
                    &mut world.scenario_rng,
                )
            }
            None => delta,
        };
        use crate::sim::projectile::launch::{
            FireAtLaunch, FireAtLaunchResult, fireat_launch, high_arc_root,
        };
        let raw_source = ProjectileCoord::new(
            i32::from(snap.pos_rx) * 256 + snap.sub_x.to_num::<i32>(),
            i32::from(snap.pos_ry) * 256 + snap.sub_y.to_num::<i32>(),
            origin_world_z_leptons,
        );
        // 70D590 reads the source's current target (TarCom), independently of
        // the FireAt parameter and the scattered launch delta.
        let current_target = tarcom;
        let target_location = |target: TargetKind| -> Option<ProjectileCoord> {
            match target {
                TargetKind::Entity(id) => object_get_coords(world, id),
                TargetKind::Cell(rx, ry) => {
                    use crate::sim::cell_rect::{CellRef, get_cellclass_fallback};
                    let cell = get_cellclass_fallback(
                        world.resolved_terrain.as_ref(),
                        i32::from(rx),
                        i32::from(ry),
                    );
                    let (x, y, level, slope) = match cell {
                        CellRef::Real(cell) => (
                            i32::from(cell.rx as i16),
                            i32::from(cell.ry as i16),
                            cell.level,
                            cell.slope_type,
                        ),
                        CellRef::Dummy { cell } => {
                            let cell = cell.snapshot();
                            (
                                cell.coord.0,
                                cell.coord.1,
                                cell.level as u8,
                                cell.slope_type,
                            )
                        }
                    };
                    let x = x * 256 + 128;
                    let y = y * 256 + 128;
                    Some(ProjectileCoord::new(
                        x,
                        y,
                        crate::util::lepton::ground_height_leptons(level, slope, x, y)
                            .expect("native target cell slope"),
                    ))
                }
            }
        };
        let flh_origin_z = fire.coord.z;
        // 6FE947..6FE98A: directed launches read virtual+308. Dropping's
        // second GetCoords read (0x006FE960) stores the value the launch
        // source already holds (`fireat_launch_source`).
        // RadialFireSegments reads PrimaryFacing.Current independently of
        // the directed launch's virtual+308 (which can use a turret).
        let body_heading = world
            .substrate
            .entities
            .get(snap.stable_id)
            .map_or(snap.hull_facing.current(binary_frame), |source| {
                source.body_facing_current(binary_frame)
            });
        let directed_heading = projectile_type
            .filter(|projectile| projectile.dropping || projectile.rot != 0)
            .map(|_| {
                let source = world.substrate.entities.get(snap.stable_id);
                match snap.category {
                    EntityCategory::Unit if obj.has_turret => source
                        .and_then(|source| source.barrel_facing.as_ref())
                        .map_or(0, |facing| facing.current(binary_frame)),
                    EntityCategory::Unit | EntityCategory::Infantry => body_heading,
                    EntityCategory::Aircraft => source
                        .and_then(|source| source.barrel_facing.as_ref())
                        .map_or(0, |facing| facing.current(binary_frame)),
                    // A building's is its fire facing, vt+0x308 (`0x0044D7D0`,
                    // read at `0x006FE2D2` and `0x006FE950`).
                    EntityCategory::Structure => fire.aim_facing16,
                }
            });
        let current_target_coord = (ballistic && !weapon.lobber)
            .then(|| current_target.and_then(target_location))
            .flatten();
        // The scalar launch consumer receives the source +300 Z. Stock
        // Building +300 preserves raw Z; Infantry delegates its FLH getter.
        // Unit/Aircraft use the transformed zero-vector pivot. The existing
        // flat FLH/pivot producer still lacks locomotor slope translation.
        let pivot_z = if snap.category == EntityCategory::Infantry {
            flh_origin_z
        } else {
            raw_source.z
        };
        let voxel = projectile_type.is_some_and(|projectile| projectile.voxel);
        let building_pitch_height = (!ballistic && !voxel && delta.z.wrapping_abs() > 200)
            .then_some(current_target)
            .flatten()
            .and_then(|target| match target {
                TargetKind::Entity(id) => world.substrate.entities.get(id),
                TargetKind::Cell(_, _) => None,
            })
            .filter(|target| target.category() == EntityCategory::Structure)
            .and_then(|target| world.interner.object_type(target.type_ref(), rules))
            .map(|target_type| {
                rules
                    .building_launch_height(target_type)
                    .wrapping_mul(200)
                    .wrapping_sub(pivot_z)
            });
        let voxel_downward = (!ballistic && voxel).then(|| {
            target_location(snap.target)
                .expect("live native FireAt target")
                .z
                < raw_source.z
        });
        //6FEAC5..6FEBE6 consumes +43C before pitch or Bullet::Fire can fail.
        // A nonpositive segment count leaves this independent index alone.
        let radial_index = world
            .substrate
            .entities
            .get_mut(snap.stable_id)
            .map_or(0, |source| {
                source
                    .weapon_burst
                    .take_radial_index(obj.radial_fire_segments)
            });
        let launch = fireat_launch(FireAtLaunch {
            radial_segments: obj.radial_fire_segments,
            radial_index,
            body_heading,
            delta,
            speed: launch_geometry.speed,
            vertical: vertical.is_some(),
            homing: guidance.is_some(),
            heading: directed_heading,
            arcing: ballistic,
            gravity: crate::sim::projectile::projectile_gravity(
                rules.general.gravity,
                collision.floater,
            ),
            high_root: high_arc_root(weapon.lobber, raw_source, current_target_coord),
            voxel_downward,
            building_pitch_height,
        });
        // A launch with no ballistic solution deletes its bullet
        // (`0x006FF000` -> `0x006FF93C`, or `0x006FF01E` when
        // `BulletClass::Fire` refuses) and resumes at `0x006FF749`.
        let launched = launch.is_some();
        if let Some(FireAtLaunchResult {
            velocity,
            speed: launch_speed,
            ..
        }) = launch
        {
            // ProcessDelayedFire writes its support bonus on the bullet this
            // FireAt returns (`0x00450496..0x004504CD`). The rest of FireAt
            // reads neither the bullet's multiplier nor the count, save the
            // laser width (`0x006FF52B`), sampled before this consumption.
            let damage_multiplier = match snap.building_shot {
                Some(super::BuildingShot::Delayed { .. }) => {
                    world.substrate.take_support_bonus(snap.stable_id, rules)
                }
                _ => ProjectilePayload::UNSCALED,
            };
            let payload = ProjectilePayload::new(
                base_damage,
                world.interner.intern(&warhead.id),
                projectile_type.map(|kind| world.interner.intern(&kind.id)),
                weapon.bright,
            )
            .with_installed_weapon(Some(world.interner.intern(selected.weapon_id)))
            .with_damage_multiplier(damage_multiplier);
            // `BulletClass::Fire` (`0x006FF014`) at the launch source.
            super::admit_fired_bullet(
                world,
                rules,
                weapon,
                bullet_id,
                super::FiredBullet {
                    native_unique_id,
                    source_id: snap.stable_id,
                    target,
                    target_position: frozen_target_position,
                    payload,
                    max_speed: weapon.speed,
                    origin,
                    velocity,
                    // Bullet::Fire normalizes ROT>0 after FireAt's scalar
                    // math. Keep the current-speed cache derived from that
                    // returned vector, as the homing owner does each visit.
                    speed: if guidance.is_some() {
                        crate::sim::projectile::projectile_velocity_magnitude(velocity)
                            .clamp(0.0, f64::from(u16::MAX)) as u16
                    } else {
                        launch_speed.clamp(0, i32::from(u16::MAX)) as u16
                    },
                    target_on_bridge: super::projectile_delivery::target_on_bridge(
                        world,
                        entry_target.projectile_target(world),
                    ),
                },
            );
        }
        launched
    };

    // A failed launch resumes at `0x006FF749`, past the rest of the shot
    // (`0x006FF031..0x006FF743`): the occupant advance, recoil, particle
    // systems, the burst step, GetROF and its draws, the rearm, the muzzle
    // anim, Report, the laser/bolt/wave, DecreaseAmmo (`0x006FF656`), the
    // `+0x3BC` 15-frame timer (`0x006FF4B0`), RevealOnFire and the `+0x120`
    // store, so the firer may try again next frame. The tail from
    // `0x006FF749` still runs.
    if !launched {
        fireat_tail(world, rules, snap, weapon, None, overlay_registry, effects);
        return None;
    }

    let sequence = world
        .substrate
        .entities
        .get(snap.stable_id)
        .map_or(0, |firer| firer.techno_ctor_random_word());
    let report_sound_id = per_shot_report(weapon, obj.is_gattling, sequence)
        .map(|report| world.interner.intern(report));
    let in_open_transport = world
        .substrate
        .entities
        .get(snap.stable_id)
        .is_some_and(|firer| firer.passenger_role.in_open_transport());
    #[cfg(any(test, feature = "test-support"))]
    let facing = (snap.hull_facing.current(binary_frame) >> 8) as u8;
    out.fire_events.push(SimFireEvent {
        attacker_id: snap.stable_id,
        #[cfg(any(test, feature = "test-support"))]
        weapon_slot: selected.slot,
        weapon_id: world.interner.intern(selected.weapon_id),
        #[cfg(any(test, feature = "test-support"))]
        facing,
        #[cfg(any(test, feature = "test-support"))]
        origin_snapshot: FireOriginSnapshot { facing },
        target: snap.target,
        report_sound_id,
        // The report and the muzzle anim read the launch source (`[ESP+0x44]`,
        // `0x006FF38F`, `0x006FF3C2`), not the fire coordinate itself.
        fire_coord: launch_source.coord,
        fire_offset_y: launch_source.offset_y,
        muzzle_anim: super::fire_coord::muzzle_anim_name(
            weapon,
            fire.aim_facing16,
            snap.garrison.is_some(),
            in_open_transport,
        )
        .map(|name| world.interner.intern(name)),
        occupied_building: snap.garrison.is_some(),
        firer_category: snap.category,
    });

    // `TechnoClass::FireAt @ 0x006FF031..0x006FF085`, right after
    // `BulletClass::Fire`: an occupied building advances its firing occupant,
    // `(+0x69C + 1) % occupants`. Everything after reads the new index: the
    // rearm below (`GetROF` at `0x006FF289` takes the building's weapon
    // through `BuildingClass::GetWeapon @ 0x004526F0`, i.e. the NEXT
    // occupant's ROF and Burst) and the shot's own kill credit, since its
    // Inviso bullet detonates later in its own AI (possibly this frame's
    // Logic tail, once its impact/fuse admits it).
    if is_garrison
        && let Some(cargo) = world
            .substrate
            .entities
            .get_mut(snap.stable_id)
            .and_then(|building| building.passenger_role.cargo_mut())
    {
        cargo.advance_garrison_fire_index();
    }
    // Techno6FF0B7..6FF15B: only a successfully launched shot arms recoil.
    // Stock recoil buildings have their own Turret=yes. Building4527D0's
    // upgrade-provided turret remains part of the separate upgrade mechanism.
    if let Some(entity) = world.substrate.entities.get_mut(snap.stable_id) {
        entity.fire_voxel_recoil(obj.has_turret);
    }
    // FireAt6FF1AD..6FF1F5: after successful Bullet::Fire and recoil, before
    // burst/GetROF/rearm. The coordinate is the retained launch-source local
    // [ESP+44], not a later recomputation. Constructor62DC50 owns identity,
    // registration and target derivation; its result owns Techno+308 liveness.
    if weapon.use_spark_particles
        && world
            .substrate
            .entities
            .get(snap.stable_id)
            .is_some_and(|firer| firer.spark_particle_system.system().is_none())
    {
        let system = weapon
            .attached_particle_system
            .as_deref()
            .and_then(|name| rules.ps_type_id_by_name(name))
            .and_then(|type_id| {
                world.spawn_particle_system(
                    type_id,
                    glam::IVec3::new(
                        launch_source.coord.x,
                        launch_source.coord.y,
                        launch_source.coord.z,
                    ),
                    Some(snap.target),
                    Some(snap.stable_id),
                    glam::IVec3::ZERO,
                    None,
                    rules,
                )
            });
        if let Some(firer) = world.substrate.entities.get_mut(snap.stable_id) {
            firer.spark_particle_system.bind(system);
        }
    }
    // GetROF (`0x006FCFA0`) reads its weapon through GetWeapon (vt+0x3F8,
    // `0x006FCFCA`), after the advance above. Preserve a missing weapon:
    // `0x006FCFD2..0x006FCFDD` returns 1 without an RNG draw, rather than
    // reusing the weapon just fired (native `techno_rearm.json` null arm).
    let rof_weapon = world
        .substrate
        .entities
        .get(snap.stable_id)
        .and_then(|firer| {
            combat_weapon::get_weapon(
                firer,
                obj,
                selected.index,
                &world.substrate.entities,
                rules,
                &world.interner,
            )
        });

    let next_index = burst.next_index();
    // `CALL [EDX+0x318]` at `0x006FF289`.
    let rof = fireat_get_rof(
        &mut world.scenario_rng,
        &world.substrate,
        &world.houses,
        rules,
        snap,
        obj,
        weapon,
        rof_weapon,
        next_index,
    );

    // `0x006FF274..0x006FF2CB`, all on the firer: the burst step around GetROF,
    // then the rearm (`+0x2EC`) with GetROF's value, which a berserk firer
    // (`+0x298`) halves, signed and toward zero (`0x006FF28F..0x006FF29C`).
    // `0x006FF743` stores the frame in `+0x120`, the since-my-last-shot mark
    // `UnitClass::Facing_Update`'s idle dwell and a Gattling building's idle
    // decay (`0x0043FEF5`) read (the constructor at `0x006F2B9C` is its only
    // other writer).
    // The remainder (`0x006FF2C5`) divides by the Burst of the weapon fired,
    // where GetROF's mid-burst test read the (next) weapon GetWeapon answers.
    if let Some(entity) = world.substrate.entities.get_mut(snap.stable_id) {
        let rearm = fireat_rearm_frames(rof, entity.berserk.is_active());
        entity.rearm_after_fire(binary_frame as i32, rearm);
        entity.weapon_burst.complete_shot(weapon.burst.max(1));
        entity.last_fire_frame.record(binary_frame);
    }
    // `0x006FF394..0x006FF43F`, after GetROF: the muzzle anim, appended to the
    // Logic vector behind this shot's bullet.
    if let Some(event) = out.fire_events.last().cloned() {
        crate::sim::world::damage_consequences::admit_muzzle_anim(world, rules, &event);
        // Sonic6FF470 precedes the independent later-effect ladder.
        if weapon.is_sonic && callbacks_enabled(world) {
            world.commit_fired_wave(rules, &event, 0);
        }
    }
    // 6FF4CC..6FF54B: successful FireAt only, after burst advance and before
    // ammo/RevealOnFire. SpawnLaser recomputes its own current GetFLH.
    super::laser::fired(
        world,
        rules,
        snap.stable_id,
        snap.target,
        selected.index,
        weapon,
        prism_support_count,
    );
    super::electric_bolt::fired(world, rules, snap.stable_id, snap.target, weapon);
    super::radiation_beam::fired(world, rules, snap.stable_id, snap.target, weapon);
    // Mag6FF5F5..6FF650 admits at most one linked Wave. The native Unit
    // constructor/update can reverse its endpoints before returning here.
    if fire_at_effect(weapon) == FireAtEffect::MagneticWave
        && world.active_wave_id(snap.stable_id).is_none()
        && callbacks_enabled(world)
        && let Some(event) = out.fire_events.last().cloned()
    {
        world.commit_fired_wave(rules, &event, 3);
    }
    // `0x006FF656`: DecreaseAmmo (vt+0x390). Only an aircraft holds Ammo here,
    // and `AircraftClass`'s slot is a bare RET (`0x0041B900`).
    // `0x006FF66C`, after DecreaseAmmo (`0x006FF656`): RevealOnFire.
    reveal_on_fire(
        world,
        rules,
        snap.stable_id,
        snap.target,
        weapon.reveal_on_fire,
        FireDisclosurePath::Shot,
    );

    fireat_tail(
        world,
        rules,
        snap,
        weapon,
        Some(bullet_id),
        overlay_registry,
        effects,
    );
    // UnitClass::Fire741340 wraps the base FireAt6FDD50 (7413CE).
    // Its non-null return seeds the Unit firing counter at741421, after the
    // base tail (including LimboLaunch). Scalable then reaches46B280 at741439.
    // This existing class-FireAt owner serves
    // UnitTarget and direct mission calls; constructors/shrapnel/other
    // classes never pass this Unit-only postprocessing boundary.
    if snap.category == EntityCategory::Unit {
        let frames = rules
            .unit_shp_read_state(world.interner.resolve(snap.type_id))
            .map_or(0, |layout| layout.firing_frames());
        if let Some(entity) = world.substrate.entities.get_mut(snap.stable_id) {
            crate::sim::animation::commit_unit_firing_counter(entity, frames);
        }
        if let Some(kind) = world
            .projectiles
            .get(bullet_id)
            .and_then(|bullet| bullet.payload().projectile_type())
            .and_then(|type_id| rules.projectile(world.interner.resolve(type_id)))
        {
            world.projectiles.init_scalable(bullet_id, kind);
        }
    }
    // Fire741340's last step (74143E..741471): a DisguiseWhenStill= Unit
    // (UnitType+0xD32) starts its disguise blink for the weapon's
    // DisguiseFakeBlinkTime= (+0x13C), read by IsClearlyVisibleTo and the
    // computer's disguise detection.
    if snap.category == EntityCategory::Unit
        && obj.disguise_when_still
        && let Some(disguise) = world
            .substrate
            .entities
            .get_mut(snap.stable_id)
            .and_then(|entity| entity.disguise.as_mut())
    {
        disguise.start_fire_blink(binary_frame, weapon.disguise_fake_blink_time);
        world.note_disguise_blink_started();
    }
    Some(bullet_id)
}

/// `TechnoClass::FireAt 0x006FF749..0x006FF939`, which runs after a launched
/// and a failed shot alike: a `LimboLaunch=` weapon takes the firer off the
/// map (`0x006FF7F3`). Its `Parasite=` arm re-fires the launched `bullet`
/// with the firer as its Owner (`0x006FF825`, the tail's one bullet
/// dereference), so a parasite shot is never a failed launch.
///
/// `FireOnce` (`0x006FF8F1..0x006FF929`) then releases a Foot's team
/// mission target and marks its action complete, before the firer's concrete
/// `Assign_Target(NULL)`. It runs after both successful and failed launches.
/// Native execution: `tools/projectile_oracle/fire_once.py`.
///
fn fireat_tail(
    world: &mut Simulation,
    rules: &RuleSet,
    snap: &AttackerSnapshot,
    weapon: &WeaponType,
    bullet: Option<u64>,
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    if weapon.limbo_launch {
        world.parasite_limbo_launch(
            snap.stable_id,
            snap.target,
            weapon,
            bullet,
            rules,
            overlay_registry,
            effects,
        );
    }
    // FireAt6FF872..6FF8EB uses its explicit argument, on both successful
    // and failed-launch tails. Unit mission Attack(1) bypasses this arm;
    // Buildings take it even on Attack. Keep the real class setter's burst,
    // passive-acquisition and SpawnManager side effects in that owner.
    let distributed = world
        .substrate
        .entities
        .get(snap.stable_id)
        .is_some_and(|entity| {
            world
                .interner
                .object_type(entity.type_ref(), rules)
                .is_some_and(|kind| kind.distributed_fire)
                && (entity.category() == EntityCategory::Structure
                    || entity.mission.current().known()
                        != Some(crate::sim::mission::MissionType::Attack))
        });
    if distributed {
        world
            .substrate
            .entities
            .get_mut(snap.stable_id)
            .unwrap()
            .remember_distributed_fire_target(snap.target);
        world
            .assign_target_represented(snap.stable_id, None, rules, overlay_registry, effects)
            .expect("DistributedFire firer retains its concrete target receiver");
    }
    if weapon.fire_once {
        // Native Abstract+0x14 bit 4 admits Foot receivers, not buildings.
        if snap.category != EntityCategory::Structure {
            world.team_fire_once_complete(snap.stable_id, rules, overlay_registry, effects);
        }
        world
            .assign_target_represented(snap.stable_id, None, rules, overlay_registry, effects)
            .expect("FireOnce firer retains its concrete target receiver");
    }
}

/// What a firer shoots this frame, and its infantry fire latch: a building's
/// request carries the target its visit fired at ([`super::BuildingShot`]);
/// any other firer fires at its live target, the Target (`+0x2B4`) that
/// `UnitClass::Fire_At_Target` (`0x00736DF0`) and an aircraft's Mission_Attack read.
fn shot_target(
    entity: &crate::sim::game_entity::GameEntity,
    building_shot: Option<super::BuildingShot>,
) -> Option<TargetKind> {
    match building_shot {
        Some(shot) => Some(shot.target()),
        None => entity.attack_target.target(),
    }
}

/// FireAt6FE268 GetFLH reads the source's live TarCom, independently of
/// the explicit FireAt target argument (which SprayAttack may replace at
///6FE245). Building calls are synchronous, so no deferred target override is
/// needed. Executed retained/cleared controls: electric_bolt.json.
fn fireat_tarcom(world: &Simulation, snap: &AttackerSnapshot) -> Option<TargetKind> {
    world
        .substrate
        .entities
        .get(snap.stable_id)
        .and_then(|firer| firer.attack_target.target())
}

/// `TechnoClass::FireAt 0x006FE582..0x006FE622`, right after the bullet is
/// built and before the launch math, so a launch that then fails has debited
/// too. A Foot firer whose locomotor `Is_Moving` (vt `+0x10`) and whose type
/// is not `JumpJet=` (`+0xD94`) marks the bullet (`+0xB4`, read nowhere in the
/// bullet's AI, Fire or Detonate), and a marked bullet skips the debit.
/// Otherwise, unless the BulletType is `Inaccurate=` (`+0x2A2`), a TarCom
/// (`+0x2B4`) that is a Techno has `EstimateDamage(TarCom, weapon)` taken off
/// its retained estimate (`+0x70`). The weapon is the one FireAt just
/// installed on the bullet (`SetWeaponType 0x0046B260`); a shot with no
/// BulletType fires the default Inviso type, which is not Inaccurate.
fn fireat_estimate_debit(
    world: &mut Simulation,
    rules: &RuleSet,
    firer_id: u64,
    tarcom: Option<TargetKind>,
    obj: &ObjectType,
    weapon: &WeaponType,
) {
    let Some(firer) = world.substrate.entities.get(firer_id) else {
        return;
    };
    let foot = matches!(
        firer.category(),
        EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
    );
    let marked = foot
        && crate::sim::movement::motion_query::is_moving(firer).unwrap_or(false)
        && !obj.jumpjet;
    let inaccurate = weapon
        .projectile
        .as_deref()
        .and_then(|id| rules.projectile(id))
        .is_some_and(|projectile| projectile.inaccurate);
    let Some(TargetKind::Entity(target_id)) = tarcom else {
        return;
    };
    if marked || inaccurate {
        return;
    }
    let estimate = estimated_damage_on(world, rules, firer_id, target_id, weapon);
    if let Some(target) = world.substrate.entities.get_mut(target_id) {
        target.estimated_health.debit(estimate);
    }
}

/// The rearm duration FireAt stores from GetROF's value: a berserk firer
/// (`TechnoClass+0x298`) halves it, signed and toward zero
/// (`0x006FF28F..0x006FF29C`). Native execution:
/// `tools/spatial_oracle/rearm_timer.py` (`fire` rows).
pub(crate) fn fireat_rearm_frames(get_rof: i32, berserk: bool) -> i32 {
    if berserk { get_rof / 2 } else { get_rof }
}

fn commit_fire_bookkeeping(
    world: &mut Simulation,
    rules: &RuleSet,
    emit: &mut CombatEmit,
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    let spawn_target_updates = std::mem::take(&mut emit.spawn_target_updates);
    let drain_links = std::mem::take(&mut emit.drain_links);
    // Spawner weapons: hand the fire target to the parent's spawn manager.
    // `SpawnManagerClass::SetTarget` only queues a target that differs from the
    // live one; the manager's own AI pass promotes it.
    for &(parent_id, target) in &spawn_target_updates {
        if let Some(manager) = world
            .substrate
            .entities
            .get_mut(parent_id)
            .and_then(|e| e.spawn_manager.as_mut())
        {
            manager.set_target(Some(target));
        }
    }
    // Drain weapons: `0x0070FD70` installs the reciprocal
    // `DrainTarget`/`DrainingMe` pair when the drainer sits over the victim.
    // `Fire_At @ 0x006FDF93..0x006FDF97` then calls `[vtable+0x3C8]` =
    // `TechnoClass::Assign_Target @ 0x006FCDB0` with NULL unconditionally
    // (the install's own cell gate does not feed back), which clears the
    // Target (`+0x2B4`), the passive-acquire byte (`+0x50C`), the burst index
    // (`+0x3B8`) and, when a SpawnManager (`+0x2D0`) exists, its target. The
    // disc therefore leaves `Fire_At` with no target and its Attack mission
    // takes the no-target exit into idle mode on its next dispatch. The
    // `+0x304` link the setter also releases is not modelled (identity
    // UNCHECKED; no stock drainer carries a SpawnManager or that link).
    for &(drainer_id, victim_id) in &drain_links {
        // `0x0070FDBD`: a drained Psychic Tower frees its captives; then the
        // drainer leaves its team without idling (`0x0070FE19..0x0070FE32`).
        if crate::sim::credit_income::install_drain_link(
            &mut world.substrate,
            &world.houses,
            &mut world.power_states,
            drainer_id,
            victim_id,
        ) {
            world.free_all_captures(victim_id, rules, overlay_registry, effects);
            crate::sim::credit_income::attach_drain_anim(world, drainer_id, rules);
            world.leave_team(drainer_id, true, rules, effects);
        }
        if let Some(drainer) = world.substrate.entities.get_mut(drainer_id) {
            represented_assign_target(drainer, None);
        }
    }
}

/// Boundaries of one synchronous FireAt transaction in the event accumulator.
struct FireCommitBoundary {
    damage_start: usize,
    explosion_start: usize,
    smudge_start: usize,
    #[cfg(any(test, feature = "test-support"))]
    fire_event_start: usize,
}

impl FireCommitBoundary {
    fn capture(emit: &CombatEmit) -> Self {
        Self {
            damage_start: emit.damage_events.len(),
            explosion_start: emit.effects.explosion_effects.len(),
            smudge_start: emit.effects.smudge_spawn_requests.len(),
            #[cfg(any(test, feature = "test-support"))]
            fire_event_start: emit.fire_events.len(),
        }
    }

    fn commit(
        self,
        world: &mut Simulation,
        run: &mut ReceiverRun,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        emit: &mut CombatEmit,
        under_attack_events: &mut Vec<UnderAttackEvent>,
    ) {
        #[cfg(any(test, feature = "test-support"))]
        let fire_event_start = self.fire_event_start;
        let Self {
            damage_start,
            explosion_start,
            smudge_start,
            ..
        } = self;
        let outer_explosion_effects = emit.effects.explosion_effects.split_off(explosion_start);
        let outer_anim_requests = emit.effects.smudge_spawn_requests.split_off(smudge_start);
        let (inline_death, mut pings) = commit_area(
            world,
            run,
            &emit.damage_events[damage_start..],
            rules,
            overlay_registry,
        );
        emit.effects.append(inline_death);
        emit.effects
            .explosion_effects
            .extend(outer_explosion_effects);
        commit_smudges(
            world,
            rules,
            overlay_registry,
            outer_anim_requests,
            &mut emit.effects.smudge_spawn_requests,
        );
        under_attack_events.append(&mut pings);
        commit_fire_bookkeeping(world, rules, emit, overlay_registry, run.effects());
        #[cfg(any(test, feature = "test-support"))]
        for event in &emit.fire_events[fire_event_start..] {
            world.trace_lifecycle_for_test(
                crate::sim::world::LifecycleTestEvent::CombatFireEffectsCommitted {
                    attacker_id: event.attacker_id,
                    scenario_rng_state: world.scenario_rng.state(),
                },
            );
        }
    }
}

/// The frame of the object's own last shot, `TechnoClass+0x120`. The
/// constructor seeds `-100` (`0x006F2B9C`), so a freshly built object is
/// already past the idle-turret dwell on frame 0, and the single other writer
/// in the image is `TechnoClass::Fire_At @ 0x006FF743`, which stores
/// `g_CurrentFrameCounter` ([`Self::record`]). `UnitClass::Facing_Update`
/// gates the idle turret return on `frame - this >= GuardAreaTargetingDelay +
/// 5`, so the dwell is measured from the unit's own last shot, NOT from
/// target loss. BuildingClass::Update starts a Gattling building's idle decay
/// once `frame - this > GuardAreaTargetingDelay + 5` (`0x0043FEE9`).
///
/// Not folded into `world_hash`: its two consumers' results are, so a
/// divergence surfaces one tick later through `barrel_facing` or `gattling`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct LastFireFrame {
    frame: i64,
}

impl Default for LastFireFrame {
    /// The constructor's `-100` (`0x006F2B9C`).
    fn default() -> Self {
        Self { frame: -100 }
    }
}

impl LastFireFrame {
    /// The frame of the last shot.
    pub const fn frame(self) -> i64 {
        self.frame
    }

    /// `Fire_At @ 0x006FF743`: a shot at `frame`.
    fn record(&mut self, frame: u32) {
        self.frame = i64::from(frame);
    }

    /// Install a fixture's frame.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_for_test(&mut self, frame: i64) {
        self.frame = frame;
    }
}

/// Call-local native trigger; no variant stores a future shot.
pub enum FireVisit {
    /// Building44B6D0/450492 already admitted the exact target and slot.
    Building { id: u64, shot: super::BuildingShot },
    /// Unit7365E1 Fire_At_Target then7365E8 Facing_Update.
    UnitTarget(u64),
    /// Infantry51BF59 dispatches Fire_At_Target5206B0, including Stage admission.
    InfantryTarget(u64),
    /// A mission already checked GetFireError and calls FireAt directly.
    Direct {
        id: u64,
        target: TargetKind,
        weapon_index: i32,
    },
    /// Aircraft Mission_Attack's strike states 4..9 (`0x00417FE0`), whose
    /// shots each commit their own receiver transaction.
    AircraftStrike(u64),
}

/// `InfantryClass::Fire_At_Target`'s tail (`0x00520A57..0x00520AC7`), on the
/// fire frame after FireAt and its effects, or after a refused shot's reset
/// (`0x005209FD..0x00520A51`): with a Target, when weapon 0 (GetWeapon(0),
/// `0x00520A68`) is slower (`+0xA8`, signed) than `[CombatDamage] Incoming=`,
/// the Target's cell objects scatter away from the infantryman
/// ([`Simulation::scatter_target_cell_from`]).
fn infantry_fire_tail(
    world: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    let Some(actor) = world.substrate.entities.get(id) else {
        return;
    };
    let Some(target) = actor
        .attack_target
        .target()
        .filter(|&target| target_present(Some(target), world))
    else {
        return;
    };
    let slow = world
        .interner
        .object_type(actor.type_ref(), rules)
        .and_then(|obj| {
            combat_weapon::get_weapon(
                actor,
                obj,
                0,
                &world.substrate.entities,
                rules,
                &world.interner,
            )
        })
        .is_some_and(|weapon| weapon.speed < rules.combat_damage.incoming);
    if !slow {
        return;
    }
    if let Err(cause) = world.scatter_target_cell_from(id, target, rules, overlay_registry, effects)
    {
        log::debug!("infantry {id} target cell Scatter failed: {cause}");
    }
}

/// One inline receiver transaction for both class and mission FireAt callers.
/// All emission remains in the existing emit_admitted_fire owner.
pub(crate) fn visit_fire(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    visit: FireVisit,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    emit: &mut CombatEmit,
    pings: &mut Vec<UnderAttackEvent>,
) {
    if let FireVisit::AircraftStrike(id) = visit {
        // Each FireAt of the strike commits its own transaction, so no outer
        // boundary wraps the visit.
        aircraft_release::visit(world, run, rules, overlay_registry, id, emit, pings);
        return;
    }
    let boundary = FireCommitBoundary::capture(emit);
    let remove_start = emit.remove_attack.len();
    let mut unit_tail = None;
    let mut infantry_tail = None;
    let facing_start = emit.unit_facing.len();
    let fog_enabled = fog_enabled(world);
    match visit {
        FireVisit::Building { id, shot } => {
            let snapshot = world.substrate.entities.get(id).map(|actor| {
                let garrison = world
                    .interner
                    .object_type(actor.type_ref(), rules)
                    .filter(|obj| combat_weapon::is_occupied(actor, obj))
                    .and_then(|_| actor.passenger_role.cargo())
                    .map(|cargo| GarrisonSnapshot {
                        fire_index: cargo.garrison_fire_index(),
                        occupant_count: cargo.count() as u8,
                    });
                AttackerSnapshot {
                    building_shot: Some(shot),
                    ..build_attacker_snapshot(actor, shot.target(), garrison)
                }
            });
            if let Some(snapshot) = snapshot {
                resolve_attacker_fire(
                    world,
                    rules,
                    overlay_registry,
                    &snapshot,
                    fog_enabled,
                    world.session.binary_frame(),
                    world.active_wave_id(id).is_some(),
                    emit,
                    run.effects(),
                );
            }
        }
        FireVisit::UnitTarget(id) => {
            if unit_reaches_fire_update(world, id)
                && let Some(actor) = world.substrate.entities.get(id)
                && !(actor.passenger_role.is_inside_transport()
                    && !actor.passenger_role.in_open_transport())
            {
                let snap = shot_target(actor, None)
                    .map(|target| build_attacker_snapshot(actor, target, None));
                let mut fire_error = None;
                if let Some(snap) = snap {
                    emit.unit_facing.push(UnitFacingUpdate {
                        entity_id: id,
                        turret_destination: None,
                        turret_spins: false,
                        hull_destination: None,
                        turret_destination_is_idle_return: false,
                    });
                    fire_error = resolve_attacker_fire(
                        world,
                        rules,
                        overlay_registry,
                        &snap,
                        fog_enabled,
                        world.session.binary_frame(),
                        world.active_wave_id(id).is_some(),
                        emit,
                        run.effects(),
                    )
                    .code;
                }
                unit_tail = Some((id, fire_error));
            }
        }
        FireVisit::InfantryTarget(id) => {
            let actor = world.substrate.entities.get(id);
            let target = actor.and_then(|actor| actor.attack_target.target());
            if let (Some(actor), Some(target)) = (actor, target) {
                let snap = build_attacker_snapshot(actor, target, None);
                let reach = resolve_attacker_fire(
                    world,
                    rules,
                    overlay_registry,
                    &snap,
                    fog_enabled,
                    world.session.binary_frame(),
                    world.active_wave_id(id).is_some(),
                    emit,
                    run.effects(),
                );
                infantry_tail = reach.infantry_fire_frame.then_some(id);
            } else if let Some(actor) = world.substrate.entities.get_mut(id) {
                //520AD2: absence of TarCom clears the raw firing latch.
                actor.mission_leaf.set_foot_firing_sequence(0);
            }
        }
        FireVisit::Direct {
            id,
            target,
            weapon_index,
        } => {
            // These are the actual FireAt arguments, independently of TarCom.
            // Legality and dispatch cadence belong to the calling mission.
            let shot = world.substrate.entities.get(id).and_then(|actor| {
                let obj = world.interner.object_type(actor.type_ref(), rules)?;
                let selected = combat_weapon::resolve_weapon_index(
                    rules,
                    actor,
                    obj,
                    weapon_index,
                    &world.substrate.entities,
                    &world.interner,
                )?;
                let coords = resolve_target_coords(&target, world)?;
                let type_ref = match target {
                    TargetKind::Entity(target) => world
                        .substrate
                        .entities
                        .get(target)
                        .map(|target| target.type_ref())
                        .or_else(|| {
                            world
                                .terrain_objects
                                .objects()
                                .get(&target)
                                .map(|target| target.type_ref())
                        })?,
                    TargetKind::Cell(..) => actor.type_ref(),
                };
                Some(AdmittedFire {
                    snap: build_attacker_snapshot(actor, target, None),
                    obj,
                    selected,
                    target_coords: coords,
                    target_type_ref: type_ref,
                    is_garrison: false,
                })
            });
            if let Some(shot) = shot {
                emit_admitted_fire(
                    world,
                    rules,
                    shot,
                    world.session.binary_frame(),
                    emit,
                    overlay_registry,
                    run.effects(),
                );
            }
        }
        // Visited above, before the boundary.
        FireVisit::AircraftStrike(_) => {}
    }
    boundary.commit(world, run, rules, overlay_registry, emit, pings);
    for removed in emit.remove_attack.drain(remove_start..) {
        world
            .assign_target_represented(
                removed,
                None,
                rules,
                overlay_registry,
                FrameEffects::default(),
            )
            .expect("FireAt target receiver remains retained");
    }
    if let Some(id) = infantry_tail {
        infantry_fire_tail(world, id, rules, overlay_registry, run.effects());
    }
    if let Some((id, fire_error)) = unit_tail {
        let fire_hull = if emit.unit_facing.len() > facing_start {
            emit.unit_facing.pop().and_then(|f| f.hull_destination)
        } else {
            None
        };
        if unit_reaches_fire_update(world, id) {
            world.unit_fire_update_tail(
                id,
                fire_error.map_or(
                    gattling::UnitFireOutcome::NoTarget,
                    gattling::UnitFireOutcome::Code,
                ),
                rules,
                run.effects(),
            );
        }
        if let Some(entity) = world.substrate.entities.get(id) {
            let binary_frame = world.session.binary_frame();
            let mut facing = UnitFacingUpdate::from_facing_update(
                id,
                crate::sim::movement::turret::facing_update(
                    entity,
                    world,
                    rules,
                    &world.interner,
                    binary_frame,
                ),
            );
            if fire_hull.is_some() {
                facing.hull_destination = fire_hull;
            }
            crate::sim::world::unit_post::apply_unit_facing(
                &mut world.substrate.entities,
                std::slice::from_ref(&facing),
                rules,
                &world.interner,
                binary_frame,
            );
        }
    }
}

/// The receiver fixture's combat pass: supplied detonations, then each Unit
/// and Infantry fire visit in live order. Production fires every class in its
/// own Logic slot and commits each Bullet in its own slot.
#[cfg(test)]
pub(crate) fn tick_combat(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    tick_ms: u32,
    live_order: &[u64],
    fire_suppressed: &BTreeSet<u64>,
    projectile_detonations: &[ProjectileDetonation],
) -> CombatTickResult {
    let sound_enabled = sound_enabled(world);
    let binary_frame = world.session.binary_frame();

    if tick_ms == 0 {
        return CombatTickResult {
            fixture_anims: Vec::new(),
            projectile_spawns: Vec::new(),
            consequences: crate::sim::world::damage_consequences::DamageConsequences::ordinary(
                DeathEffects::default(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
        };
    }

    // Each detonation commits ReceiveDamage and any recursive death weapon
    // before the next detonation or attacker reads wall, target, health, or
    // RNG state.
    let mut emit = CombatEmit::default();
    let mut under_attack_events = Vec::new();
    commit_projectile_detonations_inline(
        world,
        run,
        rules,
        overlay_registry,
        projectile_detonations,
        &mut emit,
        &mut under_attack_events,
    );

    let live_index: std::collections::HashMap<u64, usize> = live_order
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();
    let mut visits: Vec<u64> = Vec::new();
    if world.receiver_fixture.is_some() {
        visits.extend(
            world
                .substrate
                .entities
                .keys_sorted()
                .into_iter()
                .filter(|&id| {
                    !fire_suppressed.contains(&id)
                        && world.substrate.entities.get(id).is_some_and(|actor| {
                            matches!(
                                actor.category(),
                                EntityCategory::Unit | EntityCategory::Infantry
                            ) && actor.is_ai_alive()
                                && !actor.lifecycle.in_limbo()
                        })
                }),
        );
    }
    visits.sort_by_key(|&id| (live_index.get(&id).copied().unwrap_or(usize::MAX), id));
    for id in visits {
        let actor = world.substrate.entities.get_mut(id).unwrap();
        let visit = if actor.category() == EntityCategory::Unit {
            FireVisit::UnitTarget(id)
        } else {
            if actor.mission_leaf.as_infantry().is_none() {
                actor.mission_leaf = crate::sim::mission::MissionLeafState::for_entity_category(
                    EntityCategory::Infantry,
                );
            }
            FireVisit::InfantryTarget(id)
        };
        visit_fire(
            world,
            run,
            visit,
            rules,
            overlay_registry,
            &mut emit,
            &mut under_attack_events,
        );
    }
    // Every projectile, missile and live-order attack receiver has committed.
    // Periodic radiation belongs to each Foot AI, before its locomotor.
    // Destructure back into the named locals for post-fire state updates.
    let CombatEmit {
        mut effects,
        damage_events,
        mut remove_attack,
        fire_events,
        unit_facing: _,
        spawn_target_updates: _,
        drain_links: _,
    } = emit;

    // Fixture-supplied detonations can still carry pending site creation.
    // Production Bullet transactions install their sites synchronously through
    // DamageConsequences.
    if radiation_enabled(world) {
        for det in effects.rad_detonations.drain(..) {
            world.radiation.apply_detonation(
                det,
                binary_frame,
                &world.session.theater(),
                &rules.radiation,
                world.resolved_terrain.as_ref(),
                &mut crate::sim::light_sources::LightingSources::default(),
            );
        }
    }

    // Clear finished attackers' targets.
    remove_attack.sort_unstable();
    remove_attack.dedup();
    for &attacker_id in &remove_attack {
        let _ = world.assign_target_represented(
            attacker_id,
            None,
            rules,
            overlay_registry,
            FrameEffects::default(),
        );
    }

    // Push the synchronously selected death sounds to the presentation sink.
    // Infantry, units and buildings UnInit inside their receivers;
    // DamageConsequences UnInits only grounded aircraft and fixture victims.
    if sound_enabled {
        let sink = &mut world.sound_events;
        for (die_id, rx, ry) in effects.death_sounds.drain(..) {
            sink.emit(SimSoundEvent::EntityDied {
                die_sound_id: die_id,
                rx,
                ry,
            });
        }
    }

    if !damage_events.is_empty() {
        log::trace!(
            "Combat tick: {} shots fired, {} entities destroyed",
            damage_events.len(),
            run.handled_deaths.len(),
        );
    }

    CombatTickResult {
        fixture_anims: Vec::new(),
        projectile_spawns: Vec::new(),
        consequences: crate::sim::world::damage_consequences::DamageConsequences::ordinary(
            effects,
            under_attack_events,
            run.navigation_changed_cells.clone(),
            fire_events,
        ),
    }
}

#[cfg(test)]
#[path = "house_firepower_tests.rs"]
mod house_firepower_tests;

#[cfg(test)]
mod reveal_on_fire_tests {
    use super::*;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::house_state::HouseState;

    /// A computer Soviet tank at (10, 10) shooting a human American tank at
    /// (12, 10), in a multiplayer game.
    fn world() -> (Simulation, RuleSet) {
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=300\n",
        ))
        .unwrap();
        let mut sim = Simulation::new();
        sim.fog.set_width_for_test(32);
        sim.fog.set_height_for_test(32);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        for (name, human) in [("Americans", true), ("Soviet", false)] {
            let id = sim.interner.intern(name);
            sim.houses
                .register(HouseState::new(id, 0, None, human, 0, 10));
        }
        for (id, owner, rx) in [(1, "Soviet", 10), (2, "Americans", 12)] {
            let mut entity = GameEntity::test_default(id, "MTNK", owner, rx, 10);
            entity.set_owner_for_test(sim.interner.intern(owner));
            entity.set_type_ref_for_test(sim.interner.intern("MTNK"));
            sim.substrate.entities.insert(entity);
        }
        (sim, rules)
    }

    fn revealed(sim: &Simulation, house: &str, rx: u16, ry: u16) -> bool {
        sim.fog
            .is_cell_revealed(sim.interner.get(house).unwrap(), rx, ry)
    }

    #[test]
    fn a_shot_from_shroud_reveals_the_firer_to_its_human_victim() {
        let (mut sim, rules) = world();
        reveal_on_fire(
            &mut sim,
            &rules,
            1,
            TargetKind::Entity(2),
            true,
            FireDisclosurePath::Shot,
        );
        assert!(revealed(&sim, "Americans", 10, 10));
        assert!(revealed(&sim, "Americans", 10, 13), "radius 3");
        assert!(!revealed(&sim, "Americans", 10, 14));
        assert!(
            !revealed(&sim, "Soviet", 10, 10),
            "the firer's map is untouched"
        );
    }

    #[test]
    fn no_reveal_for_a_computer_victim_or_a_mapped_firer_cell() {
        let (mut sim, rules) = world();
        reveal_on_fire(
            &mut sim,
            &rules,
            2,
            TargetKind::Entity(1),
            true,
            FireDisclosurePath::Shot,
        );
        assert!(!revealed(&sim, "Soviet", 12, 10), "a computer victim");

        let (mut sim, rules) = world();
        let americans = sim.interner.get("Americans").unwrap();
        crate::sim::vision::reveal_radius(&mut sim.fog, americans, 10, 10, 1);
        assert!(!revealed(&sim, "Americans", 10, 12));
        reveal_on_fire(
            &mut sim,
            &rules,
            1,
            TargetKind::Entity(2),
            true,
            FireDisclosurePath::Shot,
        );
        assert!(
            !revealed(&sim, "Americans", 10, 12),
            "the firer's own cell is already mapped: IsShrouded is false"
        );

        let (mut sim, rules) = world();
        reveal_on_fire(
            &mut sim,
            &rules,
            1,
            TargetKind::Cell(12, 10),
            true,
            FireDisclosurePath::Shot,
        );
        assert!(!revealed(&sim, "Americans", 10, 10), "a cell target");
    }

    #[test]
    fn spawner_border_survives_disabled_leaf_but_shot_border_does_not() {
        for path in [FireDisclosurePath::Shot, FireDisclosurePath::Spawner] {
            let (mut sim, rules) = world();
            let viewer = sim.interner.get("Americans").unwrap();
            // A visible neighboring cell with an unwritten ground frame;
            // the source itself remains shrouded, admitting the native tail.
            crate::sim::vision::reveal_radius(&mut sim.fog, viewer, 10, 12, 1);
            assert!(!revealed(&sim, "Americans", 10, 10));
            assert_eq!(
                sim.fog.by_owner()[&viewer]
                    .map_cell_edge_cache_for_test((10, 11))
                    .0[0],
                -2
            );
            reveal_on_fire(&mut sim, &rules, 1, TargetKind::Entity(2), false, path);
            assert!(!revealed(&sim, "Americans", 10, 10), "disabled leaf");
            let frame = sim.fog.by_owner()[&viewer]
                .map_cell_edge_cache_for_test((10, 11))
                .0[0];
            assert_eq!(frame != -2, path == FireDisclosurePath::Spawner);
        }
    }

    #[test]
    fn spawner_computer_victim_still_refreshes_the_current_clients_border() {
        let (mut sim, rules) = world();
        let viewer = sim.interner.get("Americans").unwrap();
        let computer = sim.interner.get("Soviet").unwrap();
        sim.substrate
            .entities
            .get_mut(2)
            .unwrap()
            .set_owner_for_test(computer);
        crate::sim::vision::reveal_radius(&mut sim.fog, viewer, 10, 12, 1);
        reveal_on_fire(
            &mut sim,
            &rules,
            1,
            TargetKind::Entity(2),
            true,
            FireDisclosurePath::Spawner,
        );
        assert!(!revealed(&sim, "Americans", 10, 10), "no human victim leaf");
        assert_ne!(
            sim.fog.by_owner()[&viewer]
                .map_cell_edge_cache_for_test((10, 11))
                .0[0],
            -2
        );
        assert!(
            !sim.fog.by_owner().contains_key(&computer),
            "no computer client projection"
        );
    }

    #[test]
    fn terrain_object_target_keeps_the_spawner_border_without_a_victim_house() {
        let (mut sim, rules) = world();
        let viewer = sim.interner.get("Americans").unwrap();
        let tree = sim.interner.intern("TREE01");
        let ini = crate::rules::ini_parser::IniFile::from_str("[TREE01]\nStrength=100\n");
        let ty = crate::rules::terrain_object_type::TerrainObjectType::from_ini_section(
            "TREE01",
            ini.section("TREE01").unwrap(),
        );
        sim.terrain_objects.objects_mut_for_test().insert(
            3,
            crate::sim::terrain_object::TerrainObjectState::new(
                3,
                tree,
                (12, 10),
                &ty,
                false,
                None,
            ),
        );
        crate::sim::vision::reveal_radius(&mut sim.fog, viewer, 10, 12, 1);
        reveal_on_fire(
            &mut sim,
            &rules,
            1,
            TargetKind::Entity(3),
            true,
            FireDisclosurePath::Spawner,
        );
        assert!(!revealed(&sim, "Americans", 10, 10));
        assert_ne!(
            sim.fog.by_owner()[&viewer]
                .map_cell_edge_cache_for_test((10, 11))
                .0[0],
            -2
        );
    }

    #[test]
    fn mapped_source_and_owned_aircraft_skip_the_complete_spawner_tail() {
        for aircraft in [false, true] {
            let (mut sim, rules) = world();
            let viewer = sim.interner.get("Americans").unwrap();
            let cell = if aircraft { (10, 11) } else { (10, 10) };
            // Seed a single ordinary observation without refreshing its frame.
            sim.fog
                .by_owner_mut_for_test()
                .get_or_insert_with(viewer, || crate::sim::vision::OwnerVisibility::new(32, 32))
                .mark_visible(cell.0, cell.1);
            if aircraft {
                let firer = sim.substrate.entities.get_mut(1).unwrap();
                firer.set_owner_for_test(viewer);
                firer.set_category_for_test(EntityCategory::Aircraft);
            }
            reveal_on_fire(
                &mut sim,
                &rules,
                1,
                TargetKind::Entity(2),
                true,
                FireDisclosurePath::Spawner,
            );
            assert_eq!(
                sim.fog.by_owner()[&viewer]
                    .map_cell_edge_cache_for_test(cell)
                    .0[0],
                -2
            );
        }
    }
}

#[cfg(test)]
#[path = "fire_once_tests.rs"]
mod fire_once_tests;

/// Infantry52083E..520904: deployment uses actual Doing27..30; secondary
/// action availability is its signed native count, never a discharge-frame
/// comparison. Jumpjet's earlier class arm is resolved by the live caller.
fn infantry_fire_action(
    sequences: Option<&SequenceSet>,
    weapon_index: i32,
    is_prone: bool,
    deploying: bool,
) -> i32 {
    if deploying {
        return 29;
    }
    let has = |action| {
        sequences
            .and_then(|set| set.infantry_action(action))
            .is_some_and(|record| record.frames_per_facing != 0)
    };
    if weapon_index != 0 {
        if is_prone && has(41) {
            return 41;
        }
        if has(40) {
            return 40;
        }
    }
    if is_prone { 8 } else { 4 }
}

/// Infantry52094C..5209A0: signed live type fields; a deployed action does
/// not imply prone. An absent secondary sequence falls back to the matching
/// primary discharge frame, independently of action admission.
fn infantry_fire_frame(
    obj: &ObjectType,
    sequences: Option<&SequenceSet>,
    weapon_index: i32,
    is_prone: bool,
) -> i32 {
    let mut frame = if is_prone {
        obj.fire_prone_frame
    } else {
        obj.fire_up_frame
    };
    if weapon_index != 0 {
        let action = if is_prone { 41 } else { 40 };
        if sequences
            .and_then(|set| set.infantry_action(action))
            .is_some_and(|record| record.frames_per_facing != 0)
        {
            frame = if is_prone {
                obj.secondary_prone_frame
            } else {
                obj.secondary_fire_frame
            };
        }
    }
    frame
}

/// `TechnoClass::EstimateDamage @ 0x006FDB80` for `attacker_id` shooting
/// `target_id` with `weapon` (see [`damage::estimate`]): the target house's
/// category multiplier for the attacker's type, the attacker's
/// `ArmorMultiplier`, the attacker's FIREPOWER and the target's STRONGER ranks.
///
/// House+188 comes from the shared SetDifficulty owner, as in FireAt's
/// damage build; Techno+160 is the attacker's live firepower_multiplier.
pub(crate) fn estimated_damage_on(
    sim: &crate::sim::world::Simulation,
    rules: &RuleSet,
    attacker_id: u64,
    target_id: u64,
    weapon: &crate::rules::weapon_type::WeaponType,
) -> i32 {
    use crate::rules::object_type::Ability;
    use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};
    let entities = &sim.substrate.entities;
    let (Some(attacker), Some(target)) = (entities.get(attacker_id), entities.get(target_id))
    else {
        return 0;
    };
    let Some(attacker_obj) = sim.interner.object_type(attacker.type_ref(), rules) else {
        return 0;
    };
    let target_obj = sim.interner.object_type(target.type_ref(), rules);
    let house_type_armor = sim.houses.get(&target.owner()).map_or(1.0, |house| {
        rules.country_armor_mult_for_type(sim.interner.resolve(house.house_type_id()), attacker_obj)
    });
    let rank_firepower = super::veterancy::has_weapon_ability(
        attacker.veterancy.rank_kind(),
        attacker_obj,
        Ability::Firepower,
    )
    .then(|| NativeF64Bits::from_bits(rules.general.veteran_combat.to_bits()));
    let rank_armor = target_obj
        .is_some_and(|object| {
            super::veterancy::has_weapon_ability(
                target.veterancy.rank_kind(),
                object,
                Ability::Stronger,
            )
        })
        .then(|| NativeF64Bits::from_bits(rules.general.veteran_armor.to_bits()));
    let warhead = combat_weapon::warhead_of(rules, weapon);
    damage::estimate::estimated_damage(&damage::estimate::EstimateInputs {
        damage: weapon.damage,
        zeroed: weapon.is_sonic || weapon.use_fire_particles,
        stages: damage::attacker::FireDamageStages {
            house_firepower: sim
                .houses
                .get(&attacker.owner())
                .map_or(NativeF64Bits::ONE, |house| house.firepower_bias()),
            unit_firepower: attacker.crate_multipliers.firepower(),
            rank_firepower,
            occupied: None,
            bunkered: None,
            open_topped: None,
        },
        divisors: damage::DefenceDivisors {
            house_type_armor: NativeF32Bits::from_bits(house_type_armor.to_bits()),
            unit_armor: attacker.crate_multipliers.armor(),
            rank_armor,
        },
        warhead: warhead.map(|warhead| damage::estimate::EstimateWarhead {
            cell_spread: warhead.cell_spread_f64,
            percent_at_max: warhead.percent_at_max_f64,
            armor_multiplier: warhead
                .interaction(
                    target_obj.map_or(crate::rules::armor::ArmorId::NONE, |object| {
                        object.armor_id()
                    }),
                )
                .multiplier(),
        }),
        scenario_no_damage: sim.session.no_damage(),
        max_damage: rules.combat_damage.max_damage,
    })
}

#[cfg(test)]
#[path = "welding_native_tests.rs"]
mod welding_native_tests;
