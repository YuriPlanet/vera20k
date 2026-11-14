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

/// The open-topped firepower factor for a shot (`0x6FE43B`,
/// `Ext/Techno/Hooks.Firing.cpp:714-741` and `Ext/Techno/Body.cpp:228-232`):
/// the carrier's `OpenTopped.DamageMultiplier` (else the YR
/// `[CombatDamage]` global) times the passenger's
/// `OpenTransport.DamageMultiplier` (else the Phobos `[CombatDamage]` global).
/// A firer that is not inside an `OpenTopped` carrier keeps the YR global.
pub(crate) fn open_topped_damage_multiplier(
    firer: Option<&crate::sim::game_entity::GameEntity>,
    rules: &RuleSet,
    entities: &crate::sim::entity_store::EntityStore,
    interner: &crate::sim::intern::StringInterner,
) -> f32 {
    #[cfg(feature = "phobos")]
    {
        let Some(firer) = firer else {
            return rules.garrison_rules.open_topped_damage_multiplier;
        };
        let Some(carrier) =
            crate::sim::attach_effect::open_topped_transport_type(firer, entities, rules, interner)
        else {
            return rules.garrison_rules.open_topped_damage_multiplier;
        };
        let carrier_mult = carrier.phobos_open_topped().damage_multiplier(rules);
        let rider_mult = interner
            .object_type(firer.type_ref(), rules)
            .map(|kind| kind.phobos_open_transport().damage_multiplier(rules))
            .unwrap_or(f32::from_bits(
                rules.phobos_globals.open_transport_damage_multiplier.bits(),
            ));
        carrier_mult * rider_mult
    }
    #[cfg(not(feature = "phobos"))]
    {
        let _ = (firer, entities, interner);
        rules.garrison_rules.open_topped_damage_multiplier
    }
}

/// FireAt6FF4CC..6FF647 has one exclusive effect ladder. Sonic's earlier
/// Wave creation is independent; these flags select the later alternative.
/// Retail's only `IsRadEruption=` key, on `RadEruptionWeapon`, is `no`, so
/// `RadiationEruption` ([`super::radiation_beam::erupted`]) needs a map or
/// mod that sets it.
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
    /// `Burst.NoDelay` asked for the rest of the burst and the first shot left
    /// the burst unfinished, so the class firing update repeats the dispatch
    /// (Phobos hooks `0x736F67` / `0x5209EE`, which are the `UpdateFiring`
    /// callers of this routine — not `TechnoClass::Fire` itself).
    pub(super) burst_repeat: bool,
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
    let Some(shot) = admit_attacker_fire(
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
    ) else {
        return reach;
    };
    // Phobos `Burst.NoDelay` applies to an infantry or vehicle firing a weapon
    // whose `Burst` is above 1; the two hooks exist only for those classes
    // (`0x5209EE` / `0x736F67`), and aircraft already empty their burst within
    // one visit (`aircraft_release::release`). The hooks' second clause is the
    // delayed-fire gate: a weapon that authors `DelayedFire.Duration` without
    // `DelayedFire.OnlyOnInitialBurst=yes` keeps its one-shot-per-frame cadence
    // even with the key on, through
    // [`crate::rules::weapon_type::WeaponType::delayed_fire_allows_burst_repeat`].
    #[cfg(feature = "phobos")]
    let burst_no_delay = shot.selected.weapon.phobos_burst_no_delay
        && shot.selected.weapon.delayed_fire_allows_burst_repeat()
        && shot.selected.weapon.burst > 1
        && matches!(
            snap.category,
            EntityCategory::Unit | EntityCategory::Infantry
        );
    #[cfg(not(feature = "phobos"))]
    let burst_no_delay = false;
    // Phobos's hooks test `pThis->Fire(...)`, whose false answer means the shot
    // never happened. The burst index is VERA's observable for that: a launched
    // shot steps it, a refused launch leaves it where it was.
    let burst_before = weapon_burst_index(world, snap.stable_id);
    emit_admitted_fire(
        world,
        rules,
        shot,
        binary_frame,
        out,
        overlay_registry,
        effects,
    );
    let burst_after = weapon_burst_index(world, snap.stable_id);
    reach.burst_repeat = burst_no_delay
        && matches!(burst_after, Some(index) if Some(index) != burst_before && index != 0);
    reach
}

/// One evaluated `UnitClass_UpdateFiring_OmniFireTurnToTarget @ 0x736E6E`
/// (`Ext/Unit/Hooks.Jumpjet.cpp:51-95`): the desired hull heading, whether
/// the REARM arm may apply, and whether the heading actually differs.
#[cfg(feature = "phobos")]
struct OmniTurn {
    direction: u16,
    rearm_allowed: bool,
    should_turn: bool,
}

/// Evaluate the hook over the pre-shot state: a non-warping unit whose fired
/// weapon sets `OmniFire=` and whose `OmniFire.TurnToTarget` resolves yes
/// turns toward the target when it is not moving — except a turreted or
/// `TurretSpins` type, a deployed-fire Unload, and a REARM when
/// `NoTurret.TrackTarget` resolves no. The fire-error filter and the facing
/// write stay at the call site. The hull's desired facing is the native
/// `SetDesired`; the jumpjet `LocomotionFacing` arm has no VERA owner
/// (recorded residual).
#[cfg(feature = "phobos")]
fn phobos_omni_fire_turn(
    world: &Simulation,
    rules: &RuleSet,
    id: u64,
    target: &TargetKind,
    fog_enabled: bool,
) -> Option<OmniTurn> {
    let entity = world.substrate.entities.get(id)?;
    if entity.is_warping_in() {
        return None;
    }
    let object = world.interner.object_type(entity.type_ref(), rules)?;
    // Native's `HasTurret` is written as `!Turret` (0x747759), so
    // `Turret && !HasTurret` reduces to `Turret`: every turreted or spinning
    // type skips the hull turn.
    if object.has_turret || object.turret_spins {
        return None;
    }
    let facts = combat_weapon::attacker_facts(entity, object);
    let (index, selected) = combat_weapon::select_weapon_against(
        rules,
        entity,
        object,
        &facts,
        Some(target),
        world,
        &world.interner,
        world.resolved_terrain.as_ref(),
        fog_enabled.then_some(&world.house_alliances),
    )
    .unwrap_or((0, None));
    let selected = selected?;
    if (object.deploy_fire || object.deploy_fire_weapon == index)
        && entity.mission.effective().known() == Some(crate::sim::mission::MissionType::Unload)
    {
        return None;
    }
    if !selected.weapon.omni_fire || !selected.weapon.phobos_omni_fire_turn_to_target(rules) {
        return None;
    }
    let frame = world.session.binary_frame();
    let speed = crate::sim::movement::SpeedRules::new(rules, &world.interner, &world.houses);
    if crate::sim::movement::motion_query::is_moving_now(entity, speed, frame) {
        return None;
    }
    let direction = crate::sim::movement::turret::facing_toward_target(entity, target, world)?;
    Some(OmniTurn {
        direction,
        rearm_allowed: object
            .phobos_no_turret_track_target
            .unwrap_or(rules.phobos_globals.no_turret_track_target),
        should_turn: entity.body_facing_current(frame) != direction,
    })
}

/// The firer's `CurrentBurstIndex` (`TechnoClass+0x3B8`), or `None` once it is
/// gone from the store.
fn weapon_burst_index(world: &Simulation, id: u64) -> Option<i32> {
    world
        .substrate
        .entities
        .get(id)
        .map(|entity| entity.weapon_burst.index())
}

/// The loop both Phobos hooks run after their first shot
/// (`UnitClass_UpdateFiring_BurstNoDelay @ 0x736F67`,
/// `InfantryClass_UpdateFiring_BurstNoDelay @ 0x5209EE`): capture the rearm the
/// shot armed, start `RearmTimer` at 0 so `GetFireError`'s T45 admits another,
/// and keep calling Fire while the burst has not wrapped, the error test still
/// answers OK and the launch itself succeeded. Then restore the last captured
/// rearm, which is the full ROF once the burst completed — so the mid-burst
/// `BurstDelay` values are skipped entirely rather than shortened. A burst
/// interrupted by an error keeps its remaining shots for later frames, because
/// the restore is the last shot's own value.
#[allow(clippy::too_many_arguments)]
fn fire_remaining_burst_this_frame(
    world: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    snap: &AttackerSnapshot,
    fog_enabled: bool,
    binary_frame: u32,
    has_active_wave: bool,
    reach: &mut FireReach,
    out: &mut CombatEmit,
    effects: FrameEffects<'_>,
) {
    let firer = snap.stable_id;
    let frame = binary_frame as i32;
    let mut rearm = world
        .substrate
        .entities
        .get(firer)
        .map_or(0, |entity| entity.rearm_timer.remaining(frame));
    loop {
        world
            .substrate
            .entities
            .get_mut(firer)
            .expect("firer")
            .rearm_timer
            .start(frame, 0);
        let Some(next) = admit_attacker_fire(
            world,
            rules,
            overlay_registry,
            snap,
            fog_enabled,
            binary_frame,
            has_active_wave,
            reach,
            out,
            effects,
        ) else {
            break;
        };
        let before = weapon_burst_index(world, firer);
        emit_admitted_fire(
            world,
            rules,
            next,
            binary_frame,
            out,
            overlay_registry,
            effects,
        );
        let after = weapon_burst_index(world, firer);
        // `&& pThis->Fire(pTarget, weaponIndex)`: a shot that never happened ends
        // the run, and so does the wrap that completes the burst.
        let Some(after) = after else { break };
        if Some(after) == before {
            break;
        }
        rearm = world
            .substrate
            .entities
            .get(firer)
            .map_or(0, |entity| entity.rearm_timer.remaining(frame));
        if after == 0 {
            break;
        }
    }
    world
        .substrate
        .entities
        .get_mut(firer)
        .expect("firer")
        .rearm_timer
        .start(frame, rearm);
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

/// Phobos `GeneralUtils::GetRangedRandomOrSingleValue`
/// (`src/Utilities/GeneralUtils.cpp:61-64`): an authored `[min,max]` interval
/// draws `ScenarioClass::Random.RandomRanged` (game frames), a single value is
/// used as-is, then `Math::max(..., 0)` (`Hooks.Firing.cpp:672`,
/// `Body.cpp:721`). VERA's Scenario RNG (`Simulation::scenario_rng`) is the
/// sole saved/synchronized RNG and is folded into `world_hash`, so a ranged
/// delay stays deterministic across peers.
#[cfg(feature = "phobos")]
fn phobos_delayed_fire_frames(
    duration: &crate::rules::ini_value::PartialIntPair,
    world: &mut Simulation,
) -> i32 {
    let frames = if duration.is_single() {
        duration.low
    } else {
        world
            .scenario_rng
            .next_range_i32_inclusive(duration.low, duration.high)
    };
    frames.max(0)
}

/// Phobos `TechnoExt::ResetDelayedFireTimer`
/// (`src/Ext/Techno/Body.cpp:658-669`): stop the timer, clear the index and
/// pause flag, and `UnInit` the tracked anim when its stored
/// `DelayedFireRemoveOnNoDelay` flag is set. `TechnoClass_DetachAnim @ 0x710415`
/// (`Body.cpp:1256-1267`) clears that pointer without destroying on the anim's
/// natural expiry; VERA's anim owns its lifetime, so a stale id is simply not
/// destroyed here.
#[cfg(feature = "phobos")]
pub(crate) fn reset_phobos_delayed_fire_timer(world: &mut Simulation, rules: &RuleSet, id: u64) {
    let anim = world
        .substrate
        .entities
        .get_mut(id)
        .map(|firer| firer.reset_phobos_delayed_fire())
        .flatten();
    if let Some(anim_id) = anim {
        world.destroy_anim(anim_id, rules);
    }
}

/// Phobos `TechnoExt::CreateDelayedFireAnim`
/// (`src/Ext/Techno/Body.cpp:671-697`): `GameCreate<AnimClass>` at the FLH or
/// centre coordinate, `AnimClass::SetOwnerObject @ 0x00424B50` only when
/// `AnimIsAttached`, and registration of `CurrentDelayedFireAnim` with the
/// `DelayedFireRemoveOnNoDelay` flag (so an unattached anim is never removed
/// by the reset path).
#[cfg(feature = "phobos")]
fn create_phobos_delayed_fire_anim(
    world: &mut Simulation,
    rules: &RuleSet,
    snap: &AttackerSnapshot,
    weapon: &crate::rules::weapon_type::WeaponType,
    weapon_index: i32,
    in_transport: bool,
) {
    // `Hooks.Firing.cpp:673-676` / `Body.cpp:722-725`: an `isset`
    // `OpenToppedAnimation` overrides `Animation` while inside a transport.
    // VERA's `None` on that nullable is "unset", which falls back to Animation.
    let anim_name = if in_transport {
        weapon
            .phobos_delayed_fire_open_topped_animation
            .as_deref()
            .or(weapon.phobos_delayed_fire_animation.as_deref())
    } else {
        weapon.phobos_delayed_fire_animation.as_deref()
    };
    let Some(anim_name) = anim_name else {
        return;
    };

    // `CenterAnimOnFirer` short-circuits to `GetCenterCoords()`
    // (`Body.cpp:677-680`); otherwise `GetFLHAbsoluteCoords`
    // (`Body.Internal.cpp:80-89`) with `AnimOffset` replacing the weapon FLH
    // (`Hooks.Firing.cpp:678-681`). RESIDUAL: VERA's `fire_coordinate` adds its
    // `Flh` base to the weapon FLH (and ignores it for Infantry/Structure)
    // rather than replacing it, and has no separate body/turret basis switch
    // for `AnimOnTurret`; the anim is still placed at a valid firing coord.
    let coord = if weapon.phobos_delayed_fire_center_anim_on_firer {
        object_get_coords(world, snap.stable_id).map(crate::sim::anim_class::AnimWorldCoord::from)
    } else {
        let obj = world
            .interner
            .object_type_in_category(snap.type_id, snap.category.into(), rules);
        obj.map(|obj| {
            let base = weapon
                .phobos_delayed_fire_anim_offset
                .map(crate::rules::flh::Flh::from)
                .unwrap_or_default();
            let source = super::fire_coord::FireSource {
                tar_com: fireat_tarcom(world, snap),
                ..super::fire_coord::FireSource::from(snap)
            };
            super::fire_coord::fire_coordinate(world, rules, &source, obj, weapon_index, 0, base)
                .coord
        })
        .map(crate::sim::anim_class::AnimWorldCoord::from)
    };
    let Some(coord) = coord else {
        return;
    };
    let _ = weapon.phobos_delayed_fire_anim_on_turret;

    let type_id = world.interner.intern(anim_name);
    let attached = weapon.phobos_delayed_fire_anim_is_attached;
    let owner = attached.then_some(snap.stable_id);
    let Some(anim_id) = world.spawn_owned_anim(rules, type_id, coord, snap.owner, owner, None)
    else {
        return;
    };
    if attached && let Some(firer) = world.substrate.entities.get_mut(snap.stable_id) {
        firer.set_phobos_delayed_fire_anim(
            Some(anim_id),
            weapon.phobos_delayed_fire_remove_anim_on_no_delay,
        );
    }
}

/// Phobos `TechnoClass_FireAt_BeforeTruelyFire @ 0x6FDDC0`
/// (`src/Ext/Techno/Hooks.Firing.cpp:638-693`, `SkipFiring=0x6FDE03`). The
/// hook runs inside `FireAt` after weapon selection, before `GetFireError`;
/// returning `true` makes `admit_attacker_fire` return `None` (suppress this
/// frame's launch). The timer is a frame-based `CDTimerClass` (native decrements
/// each game frame; VERA compares the absolute binary frame), and the weapon
/// index / pause flag / anim pointer mirror `TechnoExt` `Body.h:60-63,120-123`.
#[cfg(feature = "phobos")]
fn phobos_delayed_fire_suppresses_this_frame(
    world: &mut Simulation,
    rules: &RuleSet,
    snap: &AttackerSnapshot,
    weapon_index: i32,
    weapon: &crate::rules::weapon_type::WeaponType,
) -> bool {
    let id = snap.stable_id;
    let frame = world.session.binary_frame() as i32;

    // `DelayedFireWeaponIndex >= 0 && != weaponIndex` → reset
    // (`Hooks.Firing.cpp:651-652`, `Body.cpp:705-709`).
    let previous_index = world
        .substrate
        .entities
        .get(id)
        .map_or(-1, |firer| firer.phobos_delayed_fire_weapon_index());
    if previous_index >= 0 && previous_index != weapon_index {
        reset_phobos_delayed_fire_timer(world, rules, id);
    }

    // `Duration.isset() && (!Transporter || !SkipInTransport)`
    // (`Hooks.Firing.cpp:654`). `inside_transport_id` is `TechnoClass+0x11C`.
    let Some(duration) = weapon.phobos_delayed_fire_duration.as_ref() else {
        return false;
    };
    let in_transport = world
        .substrate
        .entities
        .get(id)
        .is_some_and(|firer| firer.passenger_role.inside_transport_id().is_some());
    if in_transport && weapon.phobos_delayed_fire_skip_in_transport {
        return false;
    }

    // `PauseFiringSequence && (Infantry || turretless SHP Unit)` yields to the
    // class pause-sequence hooks `0x736F61` / `0x5209AF`
    // (`Hooks.Firing.cpp:658-662`, `Body.cpp:699-750`). VERA keeps one timer
    // but also sets `DelayedFireSequencePaused` for the `0x736480` frame freeze.
    let pause_sequence = weapon.phobos_delayed_fire_pause_firing_sequence
        && matches!(
            snap.category,
            EntityCategory::Infantry | EntityCategory::Unit
        );

    // `Burst <= 1 || !OnlyOnInitialBurst || CurrentBurstIndex == 0`
    // (`Hooks.Firing.cpp:664`). `weapon_burst.index()` is `Techno+0x3B8`.
    let burst_index = world
        .substrate
        .entities
        .get(id)
        .map_or(0, |firer| firer.weapon_burst.index());
    let gate_open =
        weapon.burst <= 1 || !weapon.phobos_delayed_fire_only_on_initial_burst || burst_index == 0;
    if !gate_open {
        return false;
    }

    let (start_frame, duration_frames) =
        world.substrate.entities.get(id).map_or((-1, 0), |firer| {
            (
                firer.phobos_delayed_fire_start_frame(),
                firer.phobos_delayed_fire_duration(),
            )
        });

    if start_frame >= 0 {
        let elapsed = frame.wrapping_sub(start_frame);
        if elapsed < duration_frames {
            // `timer.InProgress()` (`Hooks.Firing.cpp:666-667`,
            // `Body.cpp:737-740`): keep suppressing; hold the frame freeze.
            if pause_sequence && let Some(firer) = world.substrate.entities.get_mut(id) {
                firer.set_phobos_delayed_fire_sequence_paused(true);
            }
            return true;
        }
        // `timer.Completed()` → `ResetDelayedFireTimer`, fall through and fire
        // this frame (`Hooks.Firing.cpp:688-691`).
        reset_phobos_delayed_fire_timer(world, rules, id);
        return false;
    }

    // `!timer.HasStarted()` (`Hooks.Firing.cpp:669-687`, `Body.cpp:718-735`):
    // record the index, start the timer, create the anim, suppress this frame.
    let frames = phobos_delayed_fire_frames(duration, world);
    if let Some(firer) = world.substrate.entities.get_mut(id) {
        firer.start_phobos_delayed_fire(frame, frames, weapon_index, pause_sequence);
    }
    create_phobos_delayed_fire_anim(world, rules, snap, weapon, weapon_index, in_transport);
    true
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
    let obj =
        match world
            .interner
            .object_type_in_category(snap.type_id, snap.category.into(), rules)
        {
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
        #[cfg(feature = "phobos")]
        TargetKind::Bullet(..) => resolve_target_coords(&snap.target, world)
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
    // its visit requested (the selected index, or an upgrade's fixed 0/1;
    // [`super::BuildingShot::Mission`]), and everything
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
            fog_enabled.then_some(&world.house_alliances),
        ) else {
            out.remove_attack.push(snap.stable_id);
            return None;
        };
        (index, selected)
    };

    // Phobos `TechnoClass_FireAt_BeforeTruelyFire @ 0x6FDDC0` lives inside
    // `TechnoClass::Fire_At @ 0x006FDD50`, after its InLimbo gate and before
    // the launch, so every Fire_At caller (the class fire routines, Direct
    // visits and the aircraft release) passes through it in
    // `emit_admitted_fire`; a suppressed frame returns `None`, i.e. native's
    // SkipFiring.

    let infantry_fire = snap.category == EntityCategory::Infantry;
    let sequences =
        world
            .interner
            .animation_sequence_in_category(snap.type_id, snap.category.into(), rules);
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
        let fire_up = infantry_fire_frame(obj, sequences, weapon_index, prone);
        // Phobos `InfantryClass_FiringAI @ 0x5209AF`
        // (`Ext/Infantry/Hooks.Firing.cpp:83-137`): `Burst.FireWithinSequence`
        // moves the expected stage to `fireUp + cumulativeDelay`; the loop
        // draws one `RandomRanged(3, 5)` per index with no `Burst.Delays`
        // entry, before the equality test.
        #[cfg(feature = "phobos")]
        let (firing_frame, overrun) = if let Some(weapon) =
            selected.as_ref().map(|selected| selected.weapon)
            && weapon.phobos_burst_fire_within_sequence
        {
            let (cumulative, projected) = phobos_burst_sequence_delays(
                weapon,
                entity.weapon_burst.index(),
                &mut world.scenario_rng,
            );
            (
                fire_up.wrapping_add(cumulative),
                fire_up.wrapping_add(projected)
                    > phobos_infantry_sequence_frames(entity, sequences),
            )
        } else {
            (fire_up, false)
        };
        #[cfg(not(feature = "phobos"))]
        let firing_frame = fire_up;
        //5206DE skips the first GetFireError.5209AF asks the second only
        // at exact equality, including signed/out-of-range retained stages.
        if entity.native_stage().value() != firing_frame {
            return None;
        }
        // Overrun: the next shot would fall past the sequence, so this shot
        // arms the full rearm and the burst step restarts the index.
        #[cfg(feature = "phobos")]
        if overrun {
            world
                .substrate
                .entities
                .get_mut(snap.stable_id)?
                .weapon_burst
                .set_force_full_rearm();
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
                fog: fog_enabled.then(|| world.fog.fire_view(&world.house_alliances)),
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
    // Phobos `0x6FCD1D` sits at GetFireError's OK return with the
    // target-in-range check set: an open-topped passenger's carrier
    // uncloaks at the admitted shot.
    #[cfg(feature = "phobos")]
    if code == fire_error::FireError::Ok {
        open_topped_decloak_carrier(world, rules, snap.stable_id);
    }
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
            // Case 2 (`0x00736F78..0x0073701C`): a turret first sets its aim
            // directly, before Facing_Update and without its +6AF gate.
            // Homing projectiles can reach FACING while that latch is set.
            // A turretless vehicle
            // (`UnitType+0xE11`, which ReadINI sets to !`Turret=` at
            // `0x00747759`) with no NavCom (`+0x5A4`, `0x00736FB6`) whose
            // locomotor's Is_Moving (ILocomotion+0x10, `0x00736FE1`) is false
            // turns its hull toward the target at its own `ROT=` (`0x00737004`)
            // and copies the hull's destination into the turret slot
            // (`0x0073701C`). A pending order is neither.
            fire_error::FireError::Facing => {
                if let Some(barrel) = world
                    .substrate
                    .entities
                    .get_mut(snap.stable_id)
                    .and_then(|firer| firer.barrel_facing.as_mut())
                {
                    // Set_Desired(+3A0)736FAC retains ROT and +6AF. Apply it
                    // now so the later Facing_Update samples this new arc.
                    barrel.set(direction, binary_frame);
                } else if snap.barrel_facing.is_none()
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
                        // Ares 3.0 InfantryClass_UpdateFiringState_Uncloak
                        // (1005CDD0, hook 0x52070F): as the Unit's case 9,
                        // only in range (`CanFireAt 0x006F77B0`).
                        let mut in_range = !crate::rules::ARES_ENABLED;
                        if !in_range {
                            subject(&*world, &mut |s| in_range = s.in_range());
                        }
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
        // gamemd Infantry::Fire_At_Target52083E..520884 checks retained
        // Doing40/41 before selecting a new ground firing action. FireFly
        // precedes this ladder. The direct restart changes only Stage+F8.
        if !fire_fly
            && actor.airstrike_link().is_some()
            && actor
                .mission_leaf
                .as_infantry()
                .is_some_and(|leaf| matches!(leaf.doing(), 40 | 41))
        {
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
        let fire_up = infantry_fire_frame(obj, sequences, weapon_index, prone);
        // The same Phobos `0x5209AF` comparison on the path that just started
        // the firing action; the loop draws in index order before the test.
        #[cfg(feature = "phobos")]
        let (firing_frame, overrun) = if let Some(weapon) =
            selected.as_ref().map(|selected| selected.weapon)
            && weapon.phobos_burst_fire_within_sequence
        {
            let (cumulative, projected) = phobos_burst_sequence_delays(
                weapon,
                actor.weapon_burst.index(),
                &mut world.scenario_rng,
            );
            (
                fire_up.wrapping_add(cumulative),
                fire_up.wrapping_add(projected) > phobos_infantry_sequence_frames(actor, sequences),
            )
        } else {
            (fire_up, false)
        };
        #[cfg(not(feature = "phobos"))]
        let firing_frame = fire_up;
        if actor.native_stage().value() != firing_frame {
            return None;
        }
        #[cfg(feature = "phobos")]
        if overrun {
            world
                .substrate
                .entities
                .get_mut(snap.stable_id)?
                .weapon_burst
                .set_force_full_rearm();
        }
        reach.infantry_fire_frame = true;
        //5209C7/5209DE reselect and query live state after action/facing/Stop,
        // even when a frame-zero action can shoot in this same call.
        // Both actor and target are live reads after the synchronous callbacks.
        // Native5209BB/5209D2 reloads Target for SelectWeapon/GetFireError.
        let actor = world.substrate.entities.get(snap.stable_id)?;
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
            fog_enabled.then_some(&world.house_alliances),
        )?;
        let code = fire_error_world::FireSubject {
            world,
            rules,
            overlay_registry,
            fog: fog_enabled.then(|| world.fog.fire_view(&world.house_alliances)),
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
            #[cfg(feature = "phobos")]
            TargetKind::Bullet(..) => (resolve_target_coords(&snap.target, world)?, snap.type_id),
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
    // Ares 3.0 InfantryClass_UpdateFiringState_Heal (1004E420, hook
    // 0x520731) and UnitClass_UpdateFiringState_Heal (1004E6E0, hook
    // 0x736E8E) keep any damaged Techno, of either class.
    let Some(target) = world
        .substrate
        .entities
        .get(id)
        .filter(|target| cfg!(feature = "ares") || target.category() == class)
    else {
        return true;
    };
    let strength = world
        .interner
        .object_type(target.type_ref(), rules)
        .map_or(0, |object| object.strength);
    // The infantry hook's IsDamagedTechno asks `ratio < 1.0`, so 0/0 drops
    // there; the unit hook's `ratio >= 1.0` keeps it, as the stock test does.
    if cfg!(feature = "ares") && class == EntityCategory::Infantry {
        return target.health.compare_ratio(strength, 1.0)
            != crate::util::native_x87::MaskedX87Ordering::Less;
    }
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

/// Phobos `TechnoClass_CanFire_OpenTopCloakFix @ 0x6FCD1D`
/// (`Ext/Techno/Hooks.Cloak.cpp:201-214`): at `GetFireError`'s OK return with
/// its target-in-range check set, a passenger inside an open-topped transport
/// whose type enables `OpenTopped.DecloakToFire` also uncloaks the CARRIER
/// (`pThis->Transporter->Uncloak(true)`), not just the passenger.
#[cfg(feature = "phobos")]
pub(crate) fn open_topped_decloak_carrier(
    world: &mut Simulation,
    rules: &RuleSet,
    passenger_id: u64,
) {
    let carrier = {
        let Some(passenger) = world.substrate.entities.get(passenger_id) else {
            return;
        };
        let Some(kind) = crate::sim::attach_effect::open_topped_transport_type(
            passenger,
            &world.substrate.entities,
            rules,
            &world.interner,
        ) else {
            return;
        };
        if !kind.phobos_open_topped().decloak_to_fire(rules) {
            return;
        }
        passenger.passenger_role.inside_transport_id()
    };
    let Some(carrier_id) = carrier else {
        return;
    };
    let Some(carrier_obj) = world
        .substrate
        .entities
        .get(carrier_id)
        .and_then(|carrier| world.interner.object_type(carrier.type_ref(), rules))
    else {
        return;
    };
    let sound_enabled = sound_enabled(world);
    uncloak_to_fire(
        &world.session,
        &mut world.substrate,
        &mut world.sound_events,
        rules,
        carrier_obj,
        carrier_id,
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
                obj.cloaking_stages(rules.general.cloaking_stages),
                false,
            )
        });
    if start.is_some_and(|result| result.play_sound) && sound_enabled {
        emit_configured_cloak_sound(substrate, sound_events, id, rules, Some(obj), true);
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
        fog: Some(world.fog.fire_view(&world.house_alliances)),
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
    let dropping = rules
        .weapon_projectile(weapon)
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
        rules
            .weapon_projectile(weapon)
            .map(|projectile| LaunchSpeedProjectile {
                rot: projectile.rot,
                floater: projectile.floater,
                gravity: projectile.gravity(),
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
fn fire_damage_stages(
    world: &Simulation,
    rules: &RuleSet,
    snap: &AttackerSnapshot,
    obj: &ObjectType,
    is_garrison: bool,
) -> damage::attacker::FireDamageStages {
    let firer = world.substrate.entities.get(snap.stable_id);
    // Phobos `TechnoClass_FireAt_OpenToppedDmgMult @ 0x6FE43B`
    // (`Ext/Techno/Hooks.Firing.cpp:714-741`) and the separate
    // `GetCurrentFirepowerMultiplier` (`Ext/Techno/Body.cpp:228-232`): the
    // carrier's `OpenTopped.DamageMultiplier` (else the YR global) times the
    // passenger's `OpenTransport.DamageMultiplier` (else `[CombatDamage]`).
    let open_topped_multiplier =
        open_topped_damage_multiplier(firer, rules, &world.substrate.entities, &world.interner);
    firepower_stages(
        firer,
        &world.houses,
        rules,
        Some(obj),
        open_topped_multiplier,
        self::veterancy::rank_from_u16(snap.veterancy),
        is_garrison,
    )
}

/// The firepower stages shared by FireAt's damage build and Phobos'
/// Bullet-init capture. `obj` is absent only when the bullet-init caller
/// cannot resolve the owner's type, which skips the rank stage.
fn firepower_stages(
    firer: Option<&crate::sim::game_entity::GameEntity>,
    houses: &crate::sim::houses::Houses,
    rules: &RuleSet,
    obj: Option<&ObjectType>,
    open_topped_multiplier: f32,
    rank: self::veterancy::VeterancyRank,
    is_garrison: bool,
) -> damage::attacker::FireDamageStages {
    use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};
    let f32_bits = |value: f32| NativeF32Bits::from_bits(value.to_bits());
    let multipliers = &rules.garrison_rules;
    damage::attacker::FireDamageStages {
        house_firepower: firer
            .and_then(|firer| houses.get(&firer.owner()))
            .map_or(NativeF64Bits::ONE, |house| house.firepower_bias()),
        unit_firepower: firer.map_or(NativeF64Bits::ONE, |firer| {
            firer.crate_multipliers.firepower()
        }),
        rank_firepower: obj
            .filter(|obj| {
                self::veterancy::has_weapon_ability(
                    rank,
                    obj,
                    crate::rules::object_type::Ability::Firepower,
                )
            })
            .map(|_| NativeF64Bits::from_bits(rules.general.veteran_combat.to_bits())),
        // `vt+0x400`: BuildingClass `0x00458DD0`, CanBeOccupied &&
        // CanOccupyFire && an occupant for FireAt; Phobos' Bullet-init
        // helper applies the same type flags without requiring an occupant.
        occupied: is_garrison.then(|| f32_bits(multipliers.occupy_damage_multiplier)),
        // `+0x2E4` on a non-building: a unit installed in a Tank Bunker.
        bunkered: firer
            .filter(|firer| firer.category() != EntityCategory::Structure)
            .and_then(|firer| firer.bunker.link().installed_in())
            .map(|_| f32_bits(multipliers.bunker_damage_multiplier)),
        open_topped: firer
            .filter(|firer| firer.passenger_role.in_open_transport())
            .map(|_| f32_bits(open_topped_multiplier)),
        #[cfg(feature = "phobos")]
        ae_firepower: firer.map_or(1.0, |firer| firer.phobos_stat_multipliers().firepower),
        #[cfg(not(feature = "phobos"))]
        ae_firepower: 1.0,
    }
}

#[cfg(test)]
fn fireat_damage(
    world: &Simulation,
    rules: &RuleSet,
    snap: &AttackerSnapshot,
    obj: &ObjectType,
    weapon: &crate::rules::weapon_type::WeaponType,
    is_garrison: bool,
) -> i32 {
    let stages = fire_damage_stages(world, rules, snap, obj, is_garrison);
    damage::attacker::fire_damage(weapon.damage, weapon.fire_damage_zeroed(), &stages)
}

/// Phobos `BulletExt`'s construction-time `TechnoExt::GetCurrentFirepowerMultiplier`:
/// unit (Techno+160, including Ares attachment), House, veteran FIREPOWER,
/// then the occupied/bunker/open-topped stages, in the hook's order.
#[cfg(feature = "phobos")]
pub(crate) fn phobos_firepower_multiplier(
    stages: &damage::attacker::FireDamageStages,
) -> crate::util::native_x87::NativeF64Bits {
    use crate::util::native_x87::MaskedX87Chop53 as X;
    let mut value = X::load_f64(stages.unit_firepower);
    value = X::mul(value, X::load_f64(stages.house_firepower));
    value = X::mul(
        value,
        X::load_f64(crate::util::native_x87::NativeF64Bits::from_bits(
            stages.ae_firepower.to_bits(),
        )),
    );
    if let Some(rank) = stages.rank_firepower {
        value = X::mul(value, X::load_f64(rank));
    }
    for multiplier in [stages.occupied, stages.bunkered, stages.open_topped]
        .into_iter()
        .flatten()
    {
        value = X::mul(value, X::load_f32(multiplier));
    }
    X::store_f64_masked_chop(value)
}

/// Phobos `BulletExt::InitializeState` (`0x466556`) through
/// `TechnoExt::GetCurrentFirepowerMultiplier`: the multiplier a newly
/// constructed Bullet captures from its Owner Techno. FireAt overwrites the
/// payload at launch, so its already-set value wins.
#[cfg(feature = "phobos")]
pub(crate) fn bullet_init_firepower_multiplier(
    interner: &crate::sim::intern::StringInterner,
    substrate: &crate::sim::world::ObjectSubstrate,
    houses: &crate::sim::houses::Houses,
    rules: &RuleSet,
    source_id: u64,
) -> Option<crate::util::native_x87::NativeF64Bits> {
    let firer = substrate.entities.get(source_id)?;
    let object = interner.object_type(firer.type_ref(), rules);
    // Phobos' helper applies the occupancy multiplier from the building
    // type's flags, not from whether an occupant is present.
    let occupied = firer.category() == EntityCategory::Structure
        && object.is_some_and(|object| {
            object.can_be_occupied && object.can_occupy_fire && object.max_number_occupants > 0
        });
    let open_topped_multiplier =
        open_topped_damage_multiplier(Some(firer), rules, &substrate.entities, interner);
    let stages = firepower_stages(
        Some(firer),
        houses,
        rules,
        object,
        open_topped_multiplier,
        firer.veterancy.rank_kind(),
        occupied,
    );
    Some(phobos_firepower_multiplier(&stages))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FireDisclosurePath {
    Shot,
    Spawner,
}

/// FireAt6FDE73..6FDF5A /6FF66C..6FF73D. Both routes share the
/// victim's Map5673A0(radius3, final0) and its shifted-XYZ border6.
/// Spawner's fresh-XYZ border4 also runs when the victim/weapon leaf gates
/// fail on an Object target; normal fire requires those gates for both calls.
/// A target that is no Object leaves both tails before either. Each human map
/// represents that client's PlayerPtr; computer Houses have no client shroud.
/// On each client the victim leaf asks IsHumanPlayer (`0x0050B6F0`) of the
/// victim's House with that client's PlayerPtr
/// ([`Simulation::human_player_clients`]), and Map5673A0's House gate
/// (`0x0056756A..0x005675D0`) writes that client's map for its own House or,
/// under `AllyReveal=`, an ally ([`crate::sim::vision::reveal_admits`]): a
/// shot at a campaign PlayerControl ally reveals its attacker on the
/// player's map.
fn reveal_on_fire(
    world: &mut Simulation,
    rules: &RuleSet,
    firer_id: u64,
    target: TargetKind,
    enabled: bool,
    path: FireDisclosurePath,
) {
    // A bare Cell is no Object: Fire_At keeps the target only when its
    // AbstractFlags hold Object ([+14]&2, 6FDD7D..6FDDA1), and both tails test
    // that copy first (Spawner 6FDEE9 -> exit 6FDE03, shot 6FF6D4), so a Cell
    // reaches neither Owner nor a border.
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
    // The clients whose victim/weapon leaf passes: 6FF6D0..6FF6FA (Spawner
    // 6FDEE0..6FDF0F), RevealOnFire and IsHumanPlayer of the victim's House.
    let admitted = match victim {
        Some(victim) if enabled => world.human_player_clients(victim),
        _ => Vec::new(),
    };
    let clients;
    let viewers = match path {
        // No callback or RNG occurs in this border. Every client runs it, in
        // stable House order.
        FireDisclosurePath::Spawner => {
            clients = world.fog_viewers();
            &clients[..]
        }
        FireDisclosurePath::Shot if admitted.is_empty() => return,
        FireDisclosurePath::Shot => &admitted[..],
    };
    let context = crate::sim::world::UninitContext::new(rules, None);
    let config = world.sight_reveal_config(rules);
    let heights = world.sight_height_grid(&config);
    for &house in viewers {
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
        if admitted.contains(&house)
            && let Some(victim) = victim
            && crate::sim::vision::reveal_admits(
                &world.house_alliances,
                victim,
                house,
                &config,
                &world.interner,
            )
        {
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
/// created the Fire, Spark and Railgun systems just before
/// (`0x006FF15F..0x006FF26E`); GetROF reads the actual +304, +308 and +314
/// links, including a retained earlier system.
fn fireat_get_rof(
    scenario_rng: &mut SimRng,
    substrate: &ObjectSubstrate,
    houses: &Houses,
    rules: &RuleSet,
    interner: &crate::sim::intern::StringInterner,
    snap: &AttackerSnapshot,
    obj: &ObjectType,
    rof_weapon: Option<&WeaponType>,
    next_index: i32,
) -> i32 {
    let live = substrate
        .entities
        .get(snap.stable_id)
        .map(|firer| firer.live_particles())
        .unwrap_or_default();
    snapshot_get_rof(
        scenario_rng,
        substrate,
        houses,
        rules,
        interner,
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
    interner: &crate::sim::intern::StringInterner,
    snap: &AttackerSnapshot,
    obj: &ObjectType,
    rof_weapon: Option<&WeaponType>,
    live: super::rof::LiveParticles,
    burst_index: i32,
) -> i32 {
    let firer = substrate.entities.get(snap.stable_id);
    super::rof::get_rof(
        &super::rof::RofQuery {
            // GetROF6FCFA9: a Building with more than one round returns1
            // before weapon queries and the ordinary ROF variation draw.
            building_ammo: firer
                .filter(|entity| entity.category() == EntityCategory::Structure)
                .and_then(|entity| entity.ammo())
                .map(|ammo| ammo.current()),
            weapon: rof_weapon,
            live,
            burst_index,
            unit_burst_delays: (snap.category == EntityCategory::Unit).then_some(obj.burst_delays),
            house_rof: houses
                .get(&snap.owner)
                .map_or(crate::util::native_x87::NativeF64Bits::ONE, |house| {
                    house.rof_bias()
                }),
            ae_rof: firer.map_or(1.0, |entity| entity.phobos_rof_multiplier()),
            #[cfg(feature = "ares")]
            attached_rof: Some(
                firer.map_or(crate::util::native_x87::NativeF64Bits::ONE, |entity| {
                    entity.attached_rof_multiplier()
                }),
            ),
            #[cfg(feature = "phobos")]
            rof_random_delay: Some(
                rof_weapon
                    .and_then(|weapon| weapon.rof_random_delay)
                    .unwrap_or(rules.general.rof_random_delay),
            ),
            // Phobos `0x6FD054` reads `TechnoExt::ForceFullRearmDelay` before
            // the mid-burst branch; `Burst.FireWithinSequence`'s overrun arm
            // sets it on this firer.
            #[cfg(feature = "phobos")]
            force_full_rearm: firer.is_some_and(|entity| entity.weapon_burst.force_full_rearm()),
            rof_ability: self::veterancy::has_weapon_ability(
                self::veterancy::rank_from_u16(snap.veterancy),
                obj,
                crate::rules::object_type::Ability::Rof,
            ),
            veteran_rof: rules.general.veteran_rof,
            occupants: snap.garrison.as_ref().map(|gs| gs.occupant_count),
            bunkered: snap.category != EntityCategory::Structure
                && firer.is_some_and(|firer| {
                    matches!(
                        firer.bunker.link(),
                        crate::sim::docking::bunker_link::BunkerLink::Installed(_)
                    )
                }),
            occupy_rof_multiplier: rules.garrison_rules.occupy_rof_multiplier,
            // Phobos `TechnoClass_RearmDelay_BuildingBunkerROFMult`
            // (`0x6FD1C7`, `Ext/BuildingType/Hooks.cpp:332-345`): the bunker
            // building type's nullable overrides
            // `[CombatDamage] BunkerROFMultiplier=`.
            bunker_rof_multiplier: {
                #[cfg(feature = "phobos")]
                {
                    phobos_bunker_rof_multiplier(substrate, firer, rules, interner)
                        .unwrap_or(rules.garrison_rules.bunker_rof_multiplier)
                }
                #[cfg(not(feature = "phobos"))]
                {
                    rules.garrison_rules.bunker_rof_multiplier
                }
            },
        },
        scenario_rng,
    )
}

/// Phobos `TechnoClass_RearmDelay_BuildingBunkerROFMult`
/// (`0x6FD1C7`, `Ext/BuildingType/Hooks.cpp:332-345`): a bunkered unit's ROF
/// multiplier comes from the bunker building type's
/// `BunkerROFMultMultiplier` when set; `None` keeps the `[CombatDamage]`
/// global.
#[cfg(feature = "phobos")]
fn phobos_bunker_rof_multiplier(
    substrate: &ObjectSubstrate,
    firer: Option<&crate::sim::game_entity::GameEntity>,
    rules: &RuleSet,
    interner: &crate::sim::intern::StringInterner,
) -> Option<f32> {
    let building = firer
        .and_then(|firer| firer.bunker.link().installed_in())
        .and_then(|building| substrate.entities.get(building))?;
    rules
        .object(interner.resolve(building.type_ref()))
        .and_then(|object| object.phobos_bunker_rof_multiplier)
}

/// Existing FireAt delivery and bookkeeping, shared by the world receiver.
/// The caller still owns legality, fire-action timing and inline damage commit.
/// Answers the bullet `TechnoClass::FireAt @ 0x006FDD50` returns: the one
/// launched, or `None` where it returns NULL (Suicide, a refused sonic shot, the
/// spawner, drain and DiskLaser arms, and a launch with no solution).
/// FireAt's Fire and Spark arms (`0x006FF15F..0x006FF1A7`,
/// `0x006FF1AD..0x006FF1F5`) construct the weapon's AttachedParticleSystem
/// alike (`0x0062DC50`): from the retained launch source toward the target,
/// owned by the firer, with no House.
/// `0x006FF190` pushes `[ebx+0x11C]`, where `ebx` is the `GetWeapon`
/// (vtable `+0x3F8`) result's WeaponType: that is `AttachedParticleSystem=`
/// (`+0x11C`, `0x0077290B`), the constructor's type argument, not the
/// firer's TechnoClass Transporter. The constructor's fourth argument is the
/// firer (`esi`), which VERA passes as the owner; vera20k#3838's Transporter
/// entry for this site is a base-register misread.
fn fireat_attached_system(
    world: &mut Simulation,
    rules: &RuleSet,
    weapon: &WeaponType,
    snap: &AttackerSnapshot,
    launch: glam::IVec3,
) -> Option<u64> {
    let type_id = rules.ps_type_id_by_name(weapon.attached_particle_system.as_deref()?)?;
    world.spawn_particle_system(
        type_id,
        launch,
        Some(snap.target),
        Some(snap.stable_id),
        glam::IVec3::ZERO,
        None,
        rules,
    )
}

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
    // Phobos `TechnoClass_FireAt_UpdateWeaponType @ 0x6FDD7D`
    // (`Ext/Techno/Hooks.Firing.cpp:588-607`), ahead of FireAt's own InLimbo
    // refusal below: a `LimboLaunch=` weapon whose warhead is
    // `UnlimboDetonate` and not `Parasite` cannot fire while the firer's
    // locomotor is really moving.
    #[cfg(feature = "phobos")]
    if super::phobos_unlimbo_detonate::refuses_moving_limbo_launch(
        world,
        rules,
        weapon,
        snap.stable_id,
        snap.category,
    ) {
        return None;
    }
    // Phobos `TechnoClass_FireAt_UpdateWeaponType @ 0x6FDD7D`
    // (`Ext/Techno/Hooks.Firing.cpp:609-633`), after the LimboLaunch arm and
    // before FireAt's InLimbo refusal: with `RecountBurst` on, a weapon change
    // recounts the retained burst index against the previous weapon; a
    // positive remaining ROF re-arms the shot and refuses this one, otherwise
    // the index resets. The weapon is stored on every call.
    #[cfg(feature = "phobos")]
    {
        let weapon_ref = world.interner.intern(selected.weapon_id);
        let (last_ref, index, ae_mult, last_fire) = world
            .substrate
            .entities
            .get(snap.stable_id)
            .map_or((None, 0, 1.0, 0), |entity| {
                (
                    entity.phobos_last_weapon_type,
                    entity.weapon_burst.index(),
                    entity.phobos_rof_multiplier(),
                    entity.last_fire_frame.frame(),
                )
            });
        let changed = last_ref != Some(weapon_ref);
        let recount = obj
            .phobos_recount_burst
            .unwrap_or(rules.phobos_globals.recount_burst);
        if recount && index != 0 && changed {
            if let Some(last) = last_ref
                .and_then(|last| rules.weapon(world.interner.resolve(last)))
                .filter(|last| last.burst != 0)
            {
                let frame = world.session.binary_frame() as i32;
                let ratio = f64::from(index) / f64::from(last.burst);
                let rof = (ratio * f64::from(last.rof) * ae_mult) as i32;
                let rof = rof.wrapping_sub(frame.wrapping_sub(last_fire as i32));
                if rof > 0 {
                    if let Some(entity) = world.substrate.entities.get_mut(snap.stable_id) {
                        entity.rearm_after_fire(frame, rof);
                        entity.weapon_burst.reset();
                        entity.phobos_last_weapon_type = Some(weapon_ref);
                    }
                    return None;
                }
            }
            if let Some(entity) = world.substrate.entities.get_mut(snap.stable_id) {
                entity.weapon_burst.reset();
            }
        }
        if let Some(entity) = world.substrate.entities.get_mut(snap.stable_id) {
            entity.phobos_last_weapon_type = Some(weapon_ref);
        }
    }
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
    // Phobos TechnoClass_FireAt_BeforeTruelyFire (0x6FDDC0): the delayed-fire
    // gate runs after FireAt's InLimbo refusal and before its DiscardOn
    // counters and launch. A suppressed frame starts nothing and fires
    // nothing; the completed timer's fall-through fires normally.
    #[cfg(feature = "phobos")]
    if phobos_delayed_fire_suppresses_this_frame(world, rules, snap, selected.index, weapon) {
        return None;
    }
    // Phobos TechnoClass_FireAt_BeforeTruelyFire (0x6FDDC0): an admitted shot
    // counts towards DiscardOn.Firing.Count, including the branches below
    // that launch no bullet.
    #[cfg(feature = "phobos")]
    world.record_phobos_attach_firing(snap.stable_id, rules);
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
    // 6FDE0E..6FDE48: direct FireAt callers also refuse a flagged weapon
    // while the actual Techno+304 Fire, +314 Railgun or +308 Spark system
    // remains retained. GetFireError is not the only admission caller, and a guessed
    // expiry tick is insufficient.
    if world
        .substrate
        .entities
        .get(snap.stable_id)
        .is_some_and(|firer| {
            (weapon.use_fire_particles && firer.fire_particle_system.system().is_some())
                || (weapon.is_railgun && firer.railgun_particle_system.system().is_some())
                || (weapon.use_spark_particles && firer.spark_particle_system.system().is_some())
        })
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
    // Phobos `TechnoClass_FireAt_AreaFire` (`0x6FE19A`,
    // `Ext/Techno/Hooks.Firing.cpp:743-798`) replaces the stock cell under
    // the firer with `AreaFire.Target=`'s answer: `random` draws an eligible
    // adjacent cell, `self` aims at the firer itself, and `base` keeps the
    // stock cell after the weapon's cell gate.
    let mut area_fire_cell = None;
    let mut area_fire_firer = false;
    if weapon.area_fire && combat_weapon::is_not_spray_attack(obj) {
        #[cfg(feature = "phobos")]
        {
            // `pThis->OnBridge` and `pThis->GetCell()->Level` feed the
            // hook's `allowBridges` for every arm.
            let firer = world.substrate.entities.get(snap.stable_id);
            let on_bridge = firer.is_some_and(|entity| entity.on_bridge());
            let firer_level = firer
                .map(|entity| {
                    i32::from(
                        world
                            .terrain_cell_level(entity.position.rx(), entity.position.ry())
                            .unwrap_or(0),
                    )
                })
                .unwrap_or(0);
            match weapon.phobos_area_fire_target() {
                crate::rules::phobos_weapon_extras::AreaFireTarget::Random => {
                    match phobos_area_fire_random_cell(
                        world,
                        snap.stable_id,
                        weapon,
                        rules,
                        on_bridge,
                        firer_level,
                    ) {
                        Some(cell) => area_fire_cell = Some(cell),
                        // `0x6FE4E7`: `xor eax,eax; ret 8` — no shot and no
                        // rearm when every candidate is refused.
                        None => return None,
                    }
                }
                crate::rules::phobos_weapon_extras::AreaFireTarget::Firer
                | crate::rules::phobos_weapon_extras::AreaFireTarget::Base => {
                    // Both arms gate the cell under the firer: the hook's
                    // base arm passes `nullptr` as owner and
                    // `explicitEmptyCells=false`, and `self` reads
                    // `pThis->GetCell()`.
                    let Some(entity) = firer else {
                        return None;
                    };
                    let cell = (entity.position.rx(), entity.position.ry());
                    if !phobos_area_fire_cell_gate(
                        world,
                        rules,
                        weapon,
                        cell,
                        None,
                        on_bridge,
                        firer_level,
                        false,
                    ) {
                        return None;
                    }
                    if matches!(
                        weapon.phobos_area_fire_target(),
                        crate::rules::phobos_weapon_extras::AreaFireTarget::Firer
                    ) {
                        area_fire_firer = true;
                    } else {
                        area_fire_cell = object_get_coords(world, snap.stable_id)
                            .and_then(|coord| world.packed_map_cell(coord.x, coord.y));
                    }
                }
            }
        }
        #[cfg(not(feature = "phobos"))]
        {
            area_fire_cell = object_get_coords(world, snap.stable_id)
                .and_then(|coord| world.packed_map_cell(coord.x, coord.y));
        }
    }
    // The Object local FireAt took at entry (`0x006FDD88..0x006FDDA1`),
    // which the retarget leaves alone; the Inviso OnBridge copy reads it.
    let entry_target = snap.target;
    let area_fire_snap;
    let (snap, (target_rx, target_ry, target_sub_x, target_sub_y)) = if area_fire_firer {
        // `R->EAX(pThis)` at `0x6FE1D5`: the firer object replaces the cell
        // argument, and its own coordinates replace the cell center.
        let Some(firer) = world.substrate.entities.get(snap.stable_id) else {
            return None;
        };
        let coords = crate::sim::combat::attack_target::target_coords(firer);
        area_fire_snap = AttackerSnapshot {
            target: TargetKind::Entity(snap.stable_id),
            ..snap.clone()
        };
        (&area_fire_snap, coords)
    } else if let Some((rx, ry)) = area_fire_cell {
        area_fire_snap = AttackerSnapshot {
            target: TargetKind::Cell(rx, ry),
            ..snap.clone()
        };
        (&area_fire_snap, cell_center_coords(rx, ry))
    } else {
        (snap, (target_rx, target_ry, target_sub_x, target_sub_y))
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
    // The frozen burst index selects a Phobos burst ART offset; GetFLH's
    // lateral shift reads only its parity. Retain the signed dword in its owner.
    let burst_index = burst.index() as u8;
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
    let damage_stages = fire_damage_stages(world, rules, snap, obj, is_garrison);
    let base_damage =
        damage::attacker::fire_damage(weapon.damage, weapon.fire_damage_zeroed(), &damage_stages);
    // Phobos `TechnoClass_FireAt_LateLogic` (`0x6FF660`): an interceptor
    // firing at a Bullet with ApplyFirepowerMult=no stores the weapon's raw
    // Damage as the bullet's Health.
    #[cfg(feature = "phobos")]
    let base_damage = if matches!(snap.target, TargetKind::Bullet(..))
        && obj.phobos_interceptor.as_ref().is_some_and(|interceptor| {
            !interceptor
                .apply_firepower_mult
                .unwrap_or(rules.phobos_globals.interceptor_apply_firepower_mult)
        }) {
        weapon.damage
    } else {
        base_damage
    };
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
            &world.interner,
            snap,
            obj,
            rof_weapon,
            burst.next_index(),
        );
        if let Some(entity) = world.substrate.entities.get_mut(snap.stable_id) {
            entity.rearm_after_fire(binary_frame as i32, rof);
            // Phobos `SetChargeTurretDelay`'s other FireAt rearm site
            // (`0x6FE4A4`, `0x6FE492` increments before GetROF).
            #[cfg(feature = "phobos")]
            entity.phobos_set_charge_turret_delay(
                binary_frame as i32,
                if weapon.burst > 1 { burst.index() } else { 0 },
                &weapon.phobos_extras.charge_turret_delays,
            );
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
        let projectile_type = rules.weapon_projectile(weapon);
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
                let bounds = crate::sim::projectile::launch::fireat_scatter_bounds(
                    flak,
                    rules.combat_damage.ballistic_scatter,
                );
                // Ares TechnoClass_Fire_BallisticScatter1/2 (10019930 at
                // 0x6FE709, 10019980 at 0x6FE7FE).
                #[cfg(feature = "ares")]
                let bounds = projectile_type.map_or(bounds, |kind| {
                    kind.bullet_type_ext
                        .launch_scatter_bounds(flak, rules.combat_damage.ballistic_scatter)
                });
                crate::sim::projectile::launch::fireat_launch_scatter(
                    delta,
                    bounds,
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
                #[cfg(feature = "phobos")]
                TargetKind::Bullet(id) => world
                    .projectiles
                    .get(id)
                    .map(|projectile| projectile.position()),
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
                #[cfg(feature = "phobos")]
                TargetKind::Bullet(..) => None,
            })
            .filter(|target| target.category() == EntityCategory::Structure)
            .and_then(|target| world.interner.object_type(target.type_ref(), rules))
            .map(|target_type| {
                rules
                    .building_launch_height(target_type)
                    .wrapping_mul(200)
                    .wrapping_sub(pivot_z)
            });
        // 0x6FED2F..0x6FED8F: the target-Z pitch arm's gate — vanilla reads
        // ObjectType+0x236 (Voxel); Phobos replaces it with the resolved
        // `VerticalInitialFacing` (`Misc/Hooks.BugFixes.cpp:2435`), so a
        // `Vertical=` shot gains the arm.
        #[cfg(feature = "phobos")]
        let voxel_downward = (!ballistic
            && super::projectile_delivery::vertical_initial_facing(projectile_type))
        .then(|| {
            target_location(snap.target)
                .expect("live native FireAt target")
                .z
                < raw_source.z
        });
        #[cfg(not(feature = "phobos"))]
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
        // Phobos `TechnoClass_Fire_BeforeMoveTo @ 0x6FF008` (`Ext/Bullet/
        // Hooks.cpp:513-531`): an arcing bullet whose
        // `Arcing.AllowElevationInaccuracy` refuses (the `[CombatDamage]`
        // global defaults `true`) has its finished velocity recomputed by
        // `BulletExt::ApplyArcingFix` before `BulletClass::Fire`.
        #[cfg(feature = "phobos")]
        let arcing_fix = ballistic
            && !projectile_type
                .and_then(|kind| kind.phobos_arcing_allow_elevation_inaccuracy)
                .unwrap_or(rules.phobos_globals.arcing_allow_elevation_inaccuracy);
        #[cfg(not(feature = "phobos"))]
        let arcing_fix = false;
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
                collision.gravity(),
                collision.floater,
            ),
            high_root: high_arc_root(weapon.lobber, raw_source, current_target_coord),
            voxel_downward,
            building_pitch_height,
            #[cfg(feature = "phobos")]
            arcing_fix,
            #[cfg(feature = "phobos")]
            lobber: weapon.lobber,
        });
        // A launch with no ballistic solution deletes its bullet
        // (`0x006FF000` -> `0x006FF93C`, or `0x006FF01E` when
        // `BulletClass::Fire` refuses) and resumes at `0x006FF749`.
        let mut launched = launch.is_some();
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
            // Ares BuildingClass_Update_Prism (100169A0) also adds the
            // tower's `PrismForwarding.DamageAdd` and damage reserve to the
            // returned bullet's Health.
            #[cfg(feature = "ares")]
            let base_damage = match snap.building_shot {
                Some(super::BuildingShot::Delayed { .. }) => base_damage
                    .wrapping_add(world.ares_prism_shot_damage_add(snap.stable_id, rules)),
                _ => base_damage,
            };
            let payload = ProjectilePayload::new(
                base_damage,
                world.interner.intern(&warhead.id),
                projectile_type.map(|kind| world.interner.intern(&kind.id)),
                weapon.bright,
            )
            .with_installed_weapon(Some(world.interner.intern(selected.weapon_id)))
            .with_damage_multiplier(damage_multiplier);
            // Ares TechnoClass_Fire_CreateBullet (1001ADE0, hook 0x6FE53F):
            // the bullet's Range is the weapon's ProjectileRange, which only
            // a `Ranged=` type spends.
            #[cfg(feature = "ares")]
            let payload =
                payload.with_ares_range(projectile_type.filter(|kind| kind.ranged).map(|_| {
                    let range = weapon.ares_projectile_range;
                    #[cfg(feature = "phobos")]
                    {
                        if let Some(firer) = world.substrate.entities.get(snap.stable_id) {
                            return crate::sim::attach_effect::initial_projectile_range(
                                rules,
                                &world.interner,
                                Some(&world.substrate.entities),
                                firer,
                                weapon,
                                range,
                            );
                        }
                    }
                    range
                }));
            #[cfg(feature = "phobos")]
            let payload =
                payload.with_firepower_multiplier(phobos_firepower_multiplier(&damage_stages));
            // `BulletClass::Fire` (`0x006FF014`) at the launch source.
            launched = super::admit_techno_fired_bullet(
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

    // Phobos `TechnoClass_FireAt_LateLogic` (`0x6FF660`): the fired bullet
    // remembers the interceptor TechnoType and is Targeted; an armorless
    // target bullet becomes Locked.
    #[cfg(feature = "phobos")]
    if let Some(interceptor) = obj.phobos_interceptor.as_ref()
        && let TargetKind::Bullet(target_bullet) = snap.target
    {
        use crate::sim::projectile::intercepted_status;
        let target_has_armor = world
            .projectiles
            .get(target_bullet)
            .and_then(|bullet| bullet.payload().projectile_type())
            .and_then(|type_id| rules.projectile(world.interner.resolve(type_id)))
            .is_some_and(|kind| kind.armor.is_some());
        if !target_has_armor {
            world
                .projectiles
                .add_phobos_intercepted_status(target_bullet, intercepted_status::LOCKED);
        }
        let _ = interceptor;
        world
            .projectiles
            .set_phobos_interceptor_type(bullet_id, Some(snap.type_id));
        world
            .projectiles
            .add_phobos_intercepted_status(bullet_id, intercepted_status::TARGETED);
        world
            .projectiles
            .set_phobos_interceptor_target(bullet_id, Some(target_bullet));
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
        // `0x6FF42B` takes the weapon, barrel and burst of a flash that
        // `Anim.Update` keeps on the firer, and only for a weapon `Anim=` list.
        // `FiringAnim_LastFacing`/`LastCoords` start zero (`Ext/Anim/Body.h:57-61`);
        // only `UpdateAsFiringAnim` writes them, so the first pass always
        // re-picks the direction type and re-anchors the muzzle.
        #[cfg(feature = "phobos")]
        firing_anim: (!weapon.anim.is_empty() && weapon.phobos_anim_update(rules)).then(|| {
            crate::sim::anim_class::FiringAnimState {
                weapon: world.interner.intern(selected.weapon_id),
                weapon_index: match selected.slot {
                    crate::sim::combat::WeaponSlot::Primary => 0,
                    crate::sim::combat::WeaponSlot::Secondary => 1,
                },
                burst_index: weapon_burst_index(world, snap.stable_id).unwrap_or(0) as u8,
                last_facing: 0,
                last_coords: crate::sim::anim_class::AnimWorldCoord { x: 0, y: 0, z: 0 },
            }
        }),
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
    if is_garrison && let Some(building) = world.substrate.entities.get_mut(snap.stable_id) {
        building.passenger_role.occupants_mut().advance_fire_index();
    }
    // Techno6FF0B7..6FF15B: only a successfully launched shot arms recoil.
    // FireAt6FF0BB dispatches HasTurret; Building4527D0 includes upgrades,
    // while recoil configuration and state still belong to the host.
    if let Some(entity) = world.substrate.entities.get_mut(snap.stable_id) {
        let has_turret = if entity.category() == EntityCategory::Structure {
            entity
                .building_upgrades()
                .has_turret(obj, rules, &world.interner)
        } else {
            obj.has_turret
        };
        // Phobos `TechnoClass_FireAt_TurretRecoil` (`0x6FF0DD`,
        // `Ext/Techno/Hooks.Firing.cpp:821-836`): a weapon with
        // `TurretRecoil.Suppress=yes` skips the whole vanilla recoil arm, so
        // the entity's recoil state stays clear.
        #[cfg(feature = "phobos")]
        let recoil_suppressed = weapon.phobos_turret_recoil_suppress();
        #[cfg(not(feature = "phobos"))]
        let recoil_suppressed = false;
        if !recoil_suppressed {
            entity.fire_voxel_recoil(has_turret);
        }
    }
    // FireAt6FF15F..6FF1A7: a UseFireParticles weapon with no live +304
    // system builds its AttachedParticleSystem as the Spark arm below does.
    let launch = glam::IVec3::new(
        launch_source.coord.x,
        launch_source.coord.y,
        launch_source.coord.z,
    );
    if weapon.use_fire_particles
        && world
            .substrate
            .entities
            .get(snap.stable_id)
            .is_some_and(|firer| firer.fire_particle_system.system().is_none())
    {
        let system = fireat_attached_system(world, rules, weapon, snap, launch);
        if let Some(system) = system {
            world
                .substrate
                .particle_systems
                .link_to_owner_fire_slot(system);
        }
        if let Some(firer) = world.substrate.entities.get_mut(snap.stable_id) {
            firer.fire_particle_system.bind(system);
        }
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
        let system = fireat_attached_system(world, rules, weapon, snap, launch);
        if let Some(firer) = world.substrate.entities.get_mut(snap.stable_id) {
            firer.spark_particle_system.bind(system);
        }
    }
    // FireAt6FF1FB..6FF26E: an IsRailgun weapon with no live +314 system runs
    // the line damage from the launch source, then builds the Railgun system
    // from there to the coordinate it returns, with no target and no House.
    // Ares TechnoClass_Fire_DetachedRailgun (Ares.dll 1004D0C0, hook
    // 0x6FF1FB) enters for IsDetachedRailgun regardless of both tests, and
    // DetachedRailgun2 (1004D0F0, hook 0x6FF26E) then skips the +314 store.
    #[cfg(feature = "ares")]
    let detached = weapon.ares_visuals.is_detached_railgun;
    #[cfg(not(feature = "ares"))]
    let detached = false;
    if detached
        || weapon.is_railgun
            && world
                .substrate
                .entities
                .get(snap.stable_id)
                .is_some_and(|firer| firer.railgun_particle_system.system().is_none())
    {
        let end = super::railgun::line_damage(
            world,
            rules,
            overlay_registry,
            effects,
            &mut out.effects,
            snap.stable_id,
            launch_source.coord,
            snap.target,
            weapon,
        );
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
                    None,
                    Some(snap.stable_id),
                    glam::IVec3::new(end.x, end.y, end.z),
                    None,
                    rules,
                )
            });
        if !detached && let Some(firer) = world.substrate.entities.get_mut(snap.stable_id) {
            firer.railgun_particle_system.bind(system);
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
        &world.interner,
        snap,
        obj,
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
        #[cfg(feature = "ares")]
        let rearm = ares_berserk_rearm_frames(
            rof,
            entity.berserk.is_active().then(|| {
                obj.techno_combat()
                    .combat()
                    .berserk_rof_multiplier(rules.ares_techno_combat().berserk_rof_multiplier())
            }),
        );
        #[cfg(not(feature = "ares"))]
        let rearm = fireat_rearm_frames(rof, entity.berserk.is_active());
        entity.rearm_after_fire(binary_frame as i32, rearm);
        // Phobos `SetChargeTurretDelay` hooks this rearm site (`0x6FF29E`);
        // the just-fired shot's list item (`CurrentBurstIndex - 1`, `0x6FF27C`
        // increments before GetROF) overrides the charge denominator.
        #[cfg(feature = "phobos")]
        {
            let burst_index = if weapon.burst > 1 {
                entity.weapon_burst.index()
            } else {
                0
            };
            entity.phobos_set_charge_turret_delay(
                binary_frame as i32,
                burst_index,
                &weapon.phobos_extras.charge_turret_delays,
            );
        }
        entity.weapon_burst.complete_shot(weapon.burst.max(1));
        entity.last_fire_frame.record(binary_frame);
    }
    // TechnoClass::FireAt6FF38F calls VocClass::PlayAt7509E0 before the
    // muzzle constructor6FF3C2. Admit through the shared sound owner here:
    // later Bullet AI can submit a death cue competing for the same pool.
    if let Some(report_id) = report_sound_id
        && sound_enabled(world)
    {
        world.sound_events.emit(SimSoundEvent::voc_at_world_coord(
            world.interner.resolve(report_id).to_owned(),
            crate::sim::anim_class::AnimWorldCoord {
                x: launch_source.coord.x,
                y: launch_source.coord.y,
                z: launch_source.coord.z,
            },
        ));
        effects.flush_sound(world, rules);
    }
    // `0x006FF394..0x006FF43F`, after GetROF: the muzzle anim, appended to the
    // Logic vector behind this shot's bullet.
    if let Some(event) = out.fire_events.last().cloned() {
        crate::sim::world::damage_consequences::admit_muzzle_anim(world, rules, &event);
    }
    // Phobos `TechnoClass_FireAt_FeedbackWeapon` (`0x6FF43F`,
    // `Ext/Techno/Hooks.Firing.cpp:802-812`), at the end of the muzzle block
    // and before Sonic (`0x006FF470`): the fired weapon's `FeedbackWeapon=`
    // detonates through `WeaponTypeExt::DetonateAt(weapon, firer, firer)`.
    // The shot takes the firer's `GetCoords()`, the firer as owner and
    // target, the feedback weapon's `Damage=`/`Warhead=`/`Bright=` and no
    // firing house (`DetonateAt`'s default). An open-topped passenger skips
    // it unless the feedback weapon's own `FireInTransport=`.
    #[cfg(feature = "phobos")]
    if let Some(feedback_name) = weapon.phobos_feedback_weapon()
        && let Some(feedback) = rules.weapon(feedback_name)
    {
        let in_open_transport = world
            .substrate
            .entities
            .get(snap.stable_id)
            .is_some_and(|firer| firer.passenger_role.in_open_transport());
        if !(in_open_transport && !feedback.fire_in_transport)
            && let Some(impact) = object_get_coords(world, snap.stable_id)
            && let Some(detonation) = world.weapon_detonation_with_target(
                feedback,
                snap.stable_id,
                crate::sim::projectile::ProjectileTarget::Entity(snap.stable_id),
                snap.stable_id,
                None,
                [impact.x, impact.y, impact.z],
                feedback.damage,
                crate::sim::projectile::ProjectileDetonationReason::FeedbackWeapon,
            )
        {
            world.commit_attach_effect_detonations(&[detonation], rules, overlay_registry, effects);
        }
    }
    // Sonic6FF470 precedes the independent later-effect ladder.
    if let Some(event) = out.fire_events.last().cloned()
        && weapon.is_sonic
        && callbacks_enabled(world)
    {
        world.commit_fired_wave(rules, &event, 0, selected.index, overlay_registry);
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
    super::radiation_beam::fired(
        world,
        rules,
        snap.stable_id,
        snap.target,
        weapon,
        selected.index,
    );
    super::radiation_beam::erupted(world, rules, snap.stable_id, weapon);
    // Mag6FF5F5..6FF650 admits at most one linked Wave. The native Unit
    // constructor/update can reverse its endpoints before returning here.
    if fire_at_effect(weapon) == FireAtEffect::MagneticWave
        && world.active_wave_id(snap.stable_id).is_none()
        && callbacks_enabled(world)
        && let Some(event) = out.fire_events.last().cloned()
    {
        world.commit_fired_wave(rules, &event, 3, selected.index, overlay_registry);
    }
    // Ares 3.0 TechnoClass_Fire_OtherWaves (10056E60, hook 0x6FF5F5): a
    // weapon past the four earlier effects and without IsMagBeam creates a TS
    // laser wave (type 2 for `Wave.IsBigLaser`, else 1 for `Wave.IsLaser`)
    // through CreateWave, whatever wave the firer links already; the new wave
    // replaces the link (Techno+0x324).
    #[cfg(feature = "ares")]
    if fire_at_effect(weapon) == FireAtEffect::None
        && let Some(wave_type) = weapon.ares_visuals.laser_wave_type()
        && callbacks_enabled(world)
        && let Some(event) = out.fire_events.last().cloned()
    {
        world.commit_fired_wave(rules, &event, wave_type, selected.index, overlay_registry);
    }
    // FireAt6FF65A: shared DecreaseAmmo70D670 (+390) debits only signed>0.
    // Aircraft's override is RET41B900; release/dock retain their own writes.
    if let Some(entity) = world.substrate.entities.get_mut(snap.stable_id)
        && entity.category() != EntityCategory::Aircraft
        && let Some(ammo) = entity.ammo_mut()
    {
        ammo.decrease_by(weapon.ammo_cost());
    }
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
    // Concrete class FireAt suffixes follow the base tail, including
    // LimboLaunch. Ares reload / Infantry panic and Unit's firing counter /
    // Scalable work stay here for ordinary and direct mission FireAt callers.
    // Phobos `InfantryClass_FireAt_ReloadInTransport` (0x51DF82): a
    // transported infantryman restarts its reload clock before Ares' arm.
    #[cfg(feature = "phobos")]
    if snap.category == EntityCategory::Infantry {
        world.reload_in_transport_after_infantry_fire(rules, snap.stable_id, binary_frame as i32);
    }
    #[cfg(feature = "ares")]
    if snap.category == EntityCategory::Infantry && weapon.ammo_cost() > 0 {
        let restart = world
            .substrate
            .entities
            .get(snap.stable_id)
            .and_then(|entity| {
                let live_type = world.interner.object_type(entity.type_ref(), rules)?;
                (!entity.lifecycle.in_limbo()
                    && live_type.ammo > 0
                    && entity
                        .ammo()
                        .is_some_and(|ammo| ammo.current() < live_type.ammo))
                .then_some(())
            });
        if restart.is_some() {
            // Installed Ares InfantryClass_Fire_Ammo10047910, gamemd51DF8C:
            // after a returned Bullet, start the shared reload clock. The
            // Weapon Ammo<=0 arm consumes nothing and does not restart it.
            crate::sim::shared_ammo::start_reloading(
                &mut world.substrate.entities,
                &world.interner,
                rules,
                snap.stable_id,
                binary_frame as i32,
            );
        }
    }
    // Installed Ares 51DF8C returns 0, so this panic follows its reload.
    if snap.category == EntityCategory::Infantry
        && let Some(entity) = world.substrate.entities.get_mut(snap.stable_id)
        && let Some(live_type) = world.interner.object_type(entity.type_ref(), rules)
        && crate::sim::infantry::apply_empty_magazine_panic(live_type, entity)
        && matches!(
            entity.mission.effective().known(),
            Some(crate::sim::mission::MissionType::Attack | crate::sim::mission::MissionType::Hunt)
        )
    {
        // InfantryClass::FireAt @0x0051DF60, retail gamemd.exe:
        //51DFB4..51DFD6 queues Guard(false) after the fear write. No immediate
        // Commence, target clear, RNG draw, timer write or detach belongs here.
        crate::sim::mission::authority::queue_entity_mission_deferred(
            entity,
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Guard),
        );
    }
    if snap.category == EntityCategory::Unit {
        // UnitFireAt7413D3..741401: only a nonnull base Bullet return
        // restarts a finite, non-full automatic magazine. Re-read the live
        // type after the common tail, before the firing counter/scalable.
        let restart = world
            .substrate
            .entities
            .get(snap.stable_id)
            .and_then(|entity| {
                let live_type = world.interner.object_type(entity.type_ref(), rules)?;
                if cfg!(feature = "ares") {
                    // Ares UnitClass_Fire_Ammo10047B30 replaces7413D3:
                    // positive Weapon Ammo invokes StartReloading directly.
                    weapon.ammo_cost() > 0
                } else {
                    live_type.ammo > 0
                        && entity
                            .ammo()
                            .is_some_and(|ammo| ammo.current() < live_type.ammo)
                        && !live_type.manual_reload
                }
                .then_some(())
            });
        if restart.is_some() {
            crate::sim::shared_ammo::start_reloading(
                &mut world.substrate.entities,
                &world.interner,
                rules,
                snap.stable_id,
                binary_frame as i32,
            );
        }
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

/// Phobos `AreaFire.Target=random`'s cell pick
/// (`Ext/Techno/Hooks.Firing.cpp:759-784`): one `RandomRanged(0, last)`
/// Scenario draw per candidate in [cell_spread_offsets] order around the
/// firer's cell; the first cell inside the map whose gate admits it wins.
/// Running out answers no cell, which the caller treats as `0x6FE4E7`'s
/// no-shot return.
#[cfg(feature = "phobos")]
fn phobos_area_fire_random_cell(
    world: &mut Simulation,
    firer: u64,
    weapon: &WeaponType,
    rules: &RuleSet,
    on_bridge: bool,
    firer_level: i32,
) -> Option<(u16, u16)> {
    let entity = world.substrate.entities.get(firer)?;
    let leptons = crate::sim::attach_effect::weapon_range_with_modifiers(
        rules,
        &world.interner,
        Some(&world.substrate.entities),
        entity,
        weapon,
    );
    // `GetRangeWithModifiers() / 256 + 0.99` truncated. A negative range
    // (`-512` sentinel) saturates to a single candidate here; the native
    // `static_cast<size_t>` wraps into the enumerator's Max cap, which no
    // retail area-fire weapon reaches.
    let spread = if leptons < 0 {
        0
    } else {
        ((leptons as f64) / 256.0 + 0.99) as usize
    };
    let base = object_get_coords(world, firer)
        .and_then(|coord| world.packed_map_cell(coord.x, coord.y))?;
    let offsets = crate::sim::cell_rect::cell_spread_offsets(spread);
    if offsets.is_empty() {
        return None;
    }
    let owner = world.substrate.entities.get(firer).map(|e| e.owner());
    let size = offsets.len();
    for i in 0..size {
        let rand = world
            .scenario_rng
            .next_range_i32_inclusive(0, size as i32 - 1) as usize;
        let index = (i + rand) % size;
        let (dx, dy) = offsets[index];
        let rx = i32::from(base.0) + i32::from(dx);
        let ry = i32::from(base.1) + i32::from(dy);
        let (Ok(rx), Ok(ry)) = (u16::try_from(rx), u16::try_from(ry)) else {
            continue;
        };
        if phobos_area_fire_cell_gate(
            world,
            rules,
            weapon,
            (rx, ry),
            owner,
            on_bridge,
            firer_level,
            true,
        ) {
            return Some((rx, ry));
        }
    }
    None
}

/// `WeaponTypeExt::SkipWeaponPicking`'s load-time derivation
/// (`Ext/WeaponType/Body.cpp:210-223`): true exactly when no target gate is
/// authored. VERA models every group except the CE/health/veterancy trio's
/// AE lists, which it does carry (`has_attach_effect_gates`).
#[cfg(feature = "phobos")]
fn phobos_skip_weapon_picking(weapon: &WeaponType) -> bool {
    let targeting = weapon.weapon_targeting();
    targeting.can_target == crate::rules::phobos_crit::AffectedTarget::ALL
        && targeting.min_health == 0.0
        && targeting.max_health == 1.0
        && targeting.veterancy == crate::rules::phobos_airburst::AffectedVeterancy::ALL
        && weapon.can_target_houses() == crate::rules::phobos_shields::AffectedHouse::ALL
        && !weapon.has_attach_effect_gates()
}

/// `EnumFunctions::AreCellAndObjectsEligible` (`Utilities/EnumFunctions.cpp:96-117`):
/// the cell gate, then every Techno in the cell must pass the house filter
/// (when an owner is supplied) and `IsTechnoEligible`. `explicit_empty` is
/// the random arm's `true`; the base/self arms pass `false` and no owner.
#[cfg(feature = "phobos")]
#[allow(clippy::too_many_arguments)]
fn phobos_area_fire_cell_gate(
    world: &Simulation,
    rules: &RuleSet,
    weapon: &WeaponType,
    cell: (u16, u16),
    owner: Option<crate::sim::intern::InternedId>,
    on_bridge: bool,
    firer_level: i32,
    explicit_empty: bool,
) -> bool {
    if phobos_skip_weapon_picking(weapon) {
        return true;
    }
    if world
        .resolved_terrain
        .as_ref()
        .and_then(|terrain| terrain.cell(cell.0, cell.1))
        .is_none()
    {
        return false;
    }
    // `CellClass::BridgeLevels` = 4. The hook's `allowBridges` needs the
    // candidate's bridge head AND (firer on a bridge or the level match).
    const BRIDGE_LEVELS: i32 = 4;
    let allow_bridges = world.cell_has_bridge_head(cell)
        && (on_bridge
            || i32::from(world.terrain_cell_level(cell.0, cell.1).unwrap_or(0)) + BRIDGE_LEVELS
                == firer_level);
    let targeting = weapon.weapon_targeting();
    let land = world
        .resolved_terrain
        .as_ref()
        .and_then(|terrain| terrain.cell(cell.0, cell.1))
        .and_then(|cell| crate::rules::terrain_rules::LandType::from_index(cell.land_type));
    let techno_content = explicit_empty.then(|| {
        matches!(
            world.native_cell_content(cell).next(),
            Some(crate::sim::occupancy::CellObjectMember::Entity(_))
        )
    });
    if !crate::sim::combat::crit::cell_eligible(
        land,
        targeting.can_target,
        techno_content,
        allow_bridges,
    ) {
        return false;
    }
    for member in world.native_cell_content(cell) {
        let crate::sim::occupancy::CellObjectMember::Entity(id) = member else {
            continue;
        };
        let Some(entity) = world.substrate.entities.get(id) else {
            return false;
        };
        if let Some(owner) = owner
            && !crate::sim::combat::can_target_house(
                weapon.can_target_houses(),
                owner,
                entity.owner(),
                &world.houses,
                world.house_alliances(),
                &world.interner,
            )
        {
            return false;
        }
        let Some(object) = world.interner.object_type(entity.type_ref(), rules) else {
            return false;
        };
        if !crate::sim::combat::crit::techno_eligible(
            entity.category(),
            targeting.can_target,
            object.is_vehicle(),
        ) {
            return false;
        }
    }
    true
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
        // Phobos `TechnoClass_FireAt_FireOnce` (`0x6FF905`,
        // `Ext/Techno/Hooks.Firing.cpp:838-851`): the fired weapon's
        // `FireOnce.ResetSequence=no` marks the infantryman before the
        // `Assign_Target(NULL)` below, whose class target-change block then
        // skips the latch reset and idle `Do_Action`.
        #[cfg(feature = "phobos")]
        if snap.category == EntityCategory::Infantry && !weapon.phobos_fire_once_reset_sequence() {
            world
                .substrate
                .entities
                .get_mut(snap.stable_id)
                .expect("the fired receiver is still present")
                .set_phobos_skip_target_change_reset_sequence(true);
        }
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
    let inaccurate = rules
        .weapon_projectile(weapon)
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

/// Ares TechnoClass_Fire_BerserkROFMultiplier (Ares.dll 1004A3D0, hook
/// 0x6FF28F) in place of the halving: a berserk firer's GetROF value times
/// its multiplier (`Berserk.ROFMultiplier`, else `[CombatDamage]
/// BerserkROFMultiplier`), x87 under the game's control word, then `_ftol`.
#[cfg(feature = "ares")]
pub(crate) fn ares_berserk_rearm_frames(
    get_rof: i32,
    multiplier: Option<crate::util::native_x87::NativeF64Bits>,
) -> i32 {
    use crate::util::native_x87::MaskedX87Chop53 as X;
    multiplier.map_or(get_rof, |multiplier| {
        X::ftol_i32_low_masked(X::mul(X::load_i32(get_rof), X::load_f64(multiplier)))
    })
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
                    .map(|_| GarrisonSnapshot {
                        fire_index: actor.passenger_role.occupants().fire_index(),
                        occupant_count: actor.passenger_role.occupants().count(),
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
                    #[cfg(feature = "phobos")]
                    let omni_turn =
                        phobos_omni_fire_turn(world, rules, id, &snap.target, fog_enabled);
                    emit.unit_facing.push(UnitFacingUpdate {
                        entity_id: id,
                        turret_destination: None,
                        turret_spins: false,
                        hull_destination: None,
                        turret_destination_is_idle_return: false,
                    });
                    let mut reach = resolve_attacker_fire(
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
                    if reach.burst_repeat {
                        fire_remaining_burst_this_frame(
                            world,
                            rules,
                            overlay_registry,
                            &snap,
                            fog_enabled,
                            world.session.binary_frame(),
                            world.active_wave_id(id).is_some(),
                            &mut reach,
                            emit,
                            run.effects(),
                        );
                    }
                    fire_error = reach.code;
                    #[cfg(feature = "phobos")]
                    if let Some(turn) = omni_turn
                        && let Some(code) = fire_error
                        && matches!(
                            code,
                            fire_error::FireError::Ok | fire_error::FireError::Rearm
                        )
                        && (code != fire_error::FireError::Rearm || turn.rearm_allowed)
                        && turn.should_turn
                        && let Some(update) = emit.unit_facing.last_mut()
                    {
                        update.hull_destination = Some(turn.direction);
                    }
                }
                unit_tail = Some((id, fire_error));
            }
        }
        FireVisit::InfantryTarget(id) => {
            let actor = world.substrate.entities.get(id);
            let target = actor.and_then(|actor| actor.attack_target.target());
            if let (Some(actor), Some(target)) = (actor, target) {
                let snap = build_attacker_snapshot(actor, target, None);
                let mut reach = resolve_attacker_fire(
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
                if reach.burst_repeat {
                    fire_remaining_burst_this_frame(
                        world,
                        rules,
                        overlay_registry,
                        &snap,
                        fog_enabled,
                        world.session.binary_frame(),
                        world.active_wave_id(id).is_some(),
                        &mut reach,
                        emit,
                        run.effects(),
                    );
                }
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
                    #[cfg(feature = "phobos")]
                    TargetKind::Bullet(..) => actor.type_ref(),
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
        false,
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
                rules,
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
        for (die_id, world) in effects.death_sounds.drain(..) {
            sink.emit(SimSoundEvent::EntityDied {
                die_sound_id: die_id,
                world,
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

    /// A campaign where the human Americans are the player and the Alliance
    /// their PlayerControl ally (ALL01, ALL03; SOV04's Germans): the computer
    /// Soviet tank at (10, 10) shoots the Alliance's tank at (12, 10).
    fn campaign_world(ally_reveal: bool) -> (Simulation, RuleSet) {
        let (mut sim, mut rules) = world();
        rules.general.ally_reveal = ally_reveal;
        sim.session
            .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Campaign);
        let americans = sim.interner.get("Americans").unwrap();
        sim.session.set_current_house_for_test(Some(americans));
        let alliance = sim.interner.intern("Alliance");
        let mut house = HouseState::new(alliance, 0, None, false, 0, 10);
        house.set_player_control_for_test(true);
        sim.houses.register(house);
        sim.house_alliances_mut_for_test()
            .insert("ALLIANCE".into(), ["AMERICANS".to_string()].into());
        sim.substrate
            .entities
            .get_mut(2)
            .unwrap()
            .set_owner_for_test(alliance);
        (sim, rules)
    }

    #[test]
    fn a_shot_at_a_player_controlled_ally_reveals_the_firer_on_the_players_map() {
        for path in [FireDisclosurePath::Shot, FireDisclosurePath::Spawner] {
            let (mut sim, rules) = campaign_world(true);
            reveal_on_fire(&mut sim, &rules, 1, TargetKind::Entity(2), true, path);
            assert!(revealed(&sim, "Americans", 10, 10), "AllyReveal=yes");

            let (mut sim, rules) = campaign_world(false);
            reveal_on_fire(&mut sim, &rules, 1, TargetKind::Entity(2), true, path);
            assert!(!revealed(&sim, "Americans", 10, 10), "AllyReveal=no");
        }
    }

    /// The actual FireAt disclosure continuation after ordinary scout sight
    /// and a powered retail GAGAP event. Shot admission/damage are separate.
    #[test]
    fn fire_disclosure_under_a_gap_remaps_previously_explored_ground() {
        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("Hills.mmx")
        else {
            return;
        };
        let rules = retail.rules;
        let mut sim = Simulation::with_seed(0x143);
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        sim.fog.set_width_for_test(32);
        sim.fog.set_height_for_test(32);
        sim.session
            .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
        for (index, name, human) in [(0, "Americans", true), (1, "French", false)] {
            let owner = sim.interner.intern(name);
            sim.houses.register(HouseState::new(
                owner,
                index,
                Some(owner),
                human,
                10_000,
                10,
            ));
        }
        let viewer = sim.interner.get("Americans").unwrap();
        sim.session.set_current_house_for_test(Some(viewer));
        let firer = sim
            .spawn_object("GTGCAN", "French", 20, 10, 0, &rules)
            .unwrap();
        let target = sim
            .spawn_object("E1", "Americans", 10, 13, 0, &rules)
            .unwrap();
        assert!(sim.map_cell_in_bounds((10, 13)));
        sim.spawn_object("GAPOWR", "French", 24, 16, 0, &rules)
            .unwrap();
        let scout = sim
            .spawn_object("AMCV", "Americans", 18, 10, 0, &rules)
            .unwrap();
        let context = crate::sim::world::UninitContext::with_rules(&rules);
        sim.update_object_sight_with_context(scout, 0, None, context);
        let coord = crate::sim::movement::ground_pose::object_get_coords(
            sim.entities().get(firer).unwrap(),
            sim.resolved_terrain.as_ref(),
        );
        let cell = ((coord.x / 256) as u16, (coord.y / 256) as u16);
        let index = usize::from(cell.1) * 32 + usize::from(cell.0);
        assert!(sim.fog.is_cell_revealed(viewer, cell.0, cell.1));
        sim.uninit_with_rules(scout, &rules);
        let gap = sim
            .spawn_object("GAGAP", "French", 20, 14, 0, &rules)
            .unwrap();
        crate::sim::power_system::tick_power_states(
            &mut sim.power_states,
            &mut sim.substrate.entities,
            &rules,
            &sim.interner,
            sim.session.binary_frame(),
        );
        sim.visit_building_operational(gap, &rules, None, FrameEffects::default());
        assert!(!sim.fog.is_cell_revealed(viewer, cell.0, cell.1));
        assert_eq!(
            sim.fog.by_owner()[&viewer].ground_flags_raw()[index] & 0x18,
            0
        );
        assert!(!sim.fog.by_owner()[&viewer].shroud_knowledge_raw()[index].pending());
        let rng = sim.rng_state();

        let enabled = rules.weapon("GrandCannonWeapon").unwrap().reveal_on_fire;
        assert!(enabled);
        reveal_on_fire(
            &mut sim,
            &rules,
            firer,
            TargetKind::Entity(target),
            enabled,
            FireDisclosurePath::Shot,
        );

        assert!(sim.fog.is_cell_revealed(viewer, cell.0, cell.1));
        assert_eq!(
            sim.fog.by_owner()[&viewer].ground_flags_raw()[index] & 0x18,
            0x18
        );
        assert!(sim.fog.by_owner()[&viewer].shroud_knowledge_raw()[index].pending());
        assert_eq!(sim.rng_state(), rng);
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

/// Phobos `Burst.FireWithinSequence`'s cumulative delay
/// (`Ext/Infantry/Hooks.Firing.cpp:97-113`, `Ext/Unit/Hooks.Firing.cpp:67-84`):
/// walk the burst's indices up to the current one, taking
/// `WeaponTypeExt::GetBurstDelay(i)` and drawing `RandomRanged(3, 5)` for each
/// index it leaves unset. Index 0 always answers 0 without a draw; another
/// index's delay is clamped to one frame because the sequence advances once per
/// frame. Returns the cumulative delay up to the current shot and the projected
/// delay after it (the current index's delay counted twice).
#[cfg(feature = "phobos")]
fn phobos_burst_sequence_delays(
    weapon: &WeaponType,
    burst_index: i32,
    rng: &mut SimRng,
) -> (i32, i32) {
    let mut cumulative = 0_i32;
    let mut projected = 0_i32;
    for i in 0..=burst_index {
        let mut delay = match weapon.phobos_burst_delay(i) {
            Some(delay) => delay,
            None => rng.next_range_u32_inclusive(3, 5) as i32,
        };
        if i != 0 {
            delay = delay.max(1);
        }
        cumulative = cumulative.wrapping_add(delay);
        if i == burst_index {
            projected = cumulative.wrapping_add(delay);
        }
    }
    (cumulative, projected)
}

/// `pThis->Type->Sequence->GetSequence(pThis->SequenceAnim).CountFrames`: the
/// live firing action's frames per facing, 0 without a record.
#[cfg(feature = "phobos")]
fn phobos_infantry_sequence_frames(
    entity: &crate::sim::game_entity::GameEntity,
    sequences: Option<&SequenceSet>,
) -> i32 {
    let doing = entity
        .mission_leaf
        .as_infantry()
        .map_or(-1, |leaf| leaf.doing());
    sequences
        .and_then(|set| set.infantry_action(doing))
        .map_or(0, |record| record.frames_per_facing)
}

/// `TechnoClass::EstimateDamage @ 0x006FDB80` for `attacker_id` shooting
/// `target_id` with `weapon` (see [`damage::estimate`]): the target house's
/// category multiplier for the attacker's type, the attacker's
/// `ArmorMultiplier`, the attacker's FIREPOWER, the target's AttachEffect
/// armor product and the target's STRONGER ranks.
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
    let warhead = rules.weapon_warhead(weapon);
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
            #[cfg(feature = "phobos")]
            ae_firepower: attacker.phobos_stat_multipliers().firepower,
            #[cfg(not(feature = "phobos"))]
            ae_firepower: 1.0,
        },
        divisors: damage::DefenceDivisors {
            house_type_armor: NativeF32Bits::from_bits(house_type_armor.to_bits()),
            unit_armor: attacker.crate_multipliers.armor(),
            // Phobos 0x006FDC87 divides by `CalculateArmorMultipliers(pTarget,
            // warhead, pThis->Owner)`: the target's effects, not the firer's.
            // RESIDUAL: this read-only estimate uses the cached unrestricted
            // product. Native also folds each active effect with a restricted
            // `ArmorMultiplier` (chance, house and warhead gates) and draws one
            // `RandomDouble` per such effect off its delay timer (`realHit`
            // false: no timer or HitAnim). A target carrying one is estimated
            // without it, and every target-scan estimate on it skips those
            // Scenario draws, shifting the stream.
            ae_armor: target.phobos_armor_multiplier(),
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

#[cfg(all(test, feature = "phobos"))]
#[path = "world_fire_bunker_tests.rs"]
mod world_fire_bunker_tests;

#[cfg(all(test, feature = "phobos"))]
#[path = "phobos_turret_recoil_tests.rs"]
mod phobos_turret_recoil_tests;

/// Ares 3.0 InfantryClass_UpdateFiringState_Heal (1004E420, hook 0x520731):
/// a healing infantryman keeps a damaged vehicle as its target, where the
/// stock ILLEGAL arm keeps infantry only.
#[cfg(all(test, feature = "ares"))]
mod ares_heal_drop_tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn ares_healer_keeps_any_damaged_techno_target() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=MEDIC\n[AircraftTypes]\n[BuildingTypes]\n\
             [VehicleTypes]\n0=MTNK\n\n\
             [MEDIC]\nStrength=125\nPrimary=Mend\n\n\
             [MTNK]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             Strength=300\nArmor=heavy\nSpeed=6\nSight=10\n\n\
             [Mend]\nDamage=-50\nROF=50\nRange=6\nWarhead=AP\n\n\
             [AP]\nVerses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%\n",
        ))
        .expect("healer rules parse");
        let mut world = Simulation::new();
        world.intern_rule_type_ids(&rules);
        world.resolve_type_handles(&rules);
        let tank = world
            .spawn_object("MTNK", "Americans", 5, 5, 0, &rules)
            .expect("a vehicle target");
        let mend = rules.weapon("Mend");
        let drops = |world: &Simulation| {
            heal_weapon_drops_target(
                world,
                &rules,
                mend,
                TargetKind::Entity(tank),
                EntityCategory::Infantry,
            )
        };
        let health = |world: &mut Simulation, value| {
            world
                .substrate
                .entities
                .get_mut(tank)
                .unwrap()
                .health
                .set_for_test(value)
        };
        health(&mut world, 100);
        assert!(!drops(&world), "a damaged vehicle stays the target");
        health(&mut world, 300);
        assert!(drops(&world), "a healthy one goes");
    }
}
