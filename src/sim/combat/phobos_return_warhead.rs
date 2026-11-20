//! Phobos `WarheadTypeExt::ApplyReturnWarhead` (`Ext/WarheadType/Detonate.cpp:610-636`),
//! called per target from `DetonateOnOneUnit` after the displacement arms and
//! before `PenetratesTransport` (`:229-231`): a warhead with `ReturnWarhead=`
//! can send a second warhead back at the techno that fired the bullet.
//!
//! Order: the chance dice (a fresh Scenario draw when
//! `ReturnWarhead.ApplyChancePerTarget` or the type-level
//! `ApplyPerTargetEffectsOnDetonate` is off, else the per-detonation buffer),
//! the source-house filter (`AffectsHouse` against the victim's owner), the
//! victim cell and class filters (`AffectsTarget`), then the full
//! `DetonateAt` arm at the original firer's coordinates with the victim as
//! the attacking techno, or a direct `ReceiveDamage` on the firer.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/ and the shared detonation receivers.

use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::LandType;
use crate::rules::warhead_type::WarheadType;
use crate::sim::combat::DeathEffects;
use crate::sim::combat::crit::{cell_eligible, techno_eligible};
use crate::sim::combat::receiver_call::{EntityDamageEvent, ReceiverCallFlags};
use crate::sim::combat::world_receiver::{ReceiverRun, commit_entities, commit_projectiles};
use crate::sim::intern::InternedId;
use crate::sim::projectile::{
    ProjectileDetonation, ProjectileDetonationReason, ProjectilePayload, ProjectileTarget,
};
use crate::sim::world::Simulation;

/// One `ApplyReturnWarhead`.
#[allow(clippy::too_many_arguments)]
pub(super) fn apply(
    world: &mut Simulation,
    run: &mut ReceiverRun<'_>,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    warhead: &WarheadType,
    return_buffer: f64,
    on_detonate: bool,
    source_house: Option<InternedId>,
    source_id: u64,
    target_id: u64,
    out: &mut DeathEffects,
) {
    let settings = &warhead.phobos_return_warhead;
    let Some(return_name) = settings.warhead.as_deref() else {
        return;
    };
    // `if (this->ReturnWarhead && pOwner)`: no original firer, no return.
    if source_id == 0 || world.substrate.entities.get(source_id).is_none() {
        return;
    }
    let dice = if warhead
        .crit
        .return_warhead_apply_chance_per_target(rules.crit_globals())
        || !on_detonate
    {
        world.scenario_rng.random_double()
    } else {
        return_buffer
    };
    if settings.chance < dice {
        return;
    }
    let Some(target) = world.substrate.entities.get(target_id) else {
        return;
    };
    let target_owner = target.owner();
    if let Some(house) = source_house
        && !crate::sim::combat::can_target_house(
            settings.affects_house,
            house,
            target_owner,
            &world.houses,
            &world.house_alliances,
            &world.interner,
        )
    {
        return;
    }
    let land = world.resolved_terrain.as_ref().and_then(|terrain| {
        terrain
            .cell(target.position.rx(), target.position.ry())
            .and_then(|cell| LandType::from_index(cell.land_type))
    });
    if !cell_eligible(land, settings.affects_target, None, false) {
        return;
    }
    if !techno_eligible(
        target.category(),
        settings.affects_target,
        world
            .interner
            .object_type(target.type_ref(), rules)
            .is_some_and(|object| object.is_vehicle()),
    ) {
        return;
    }
    let Some(return_warhead) = rules.warhead(return_name) else {
        return;
    };
    let warhead_ref = world.interner.intern(return_name);
    if settings.full_detonation {
        // `DetonateAt(ReturnWarhead, pOwner, pTarget, damage, pTarget->Owner)`:
        // the dummy bullet explodes at the firer's coordinates, is owned by
        // the victim, and its firing house is the victim's.
        let Some(source) = world.substrate.entities.get(source_id) else {
            return;
        };
        let location = crate::sim::movement::ground_pose::object_get_coords(
            source,
            world.resolved_terrain.as_ref(),
        );
        let packet = ProjectileDetonation {
            projectile_id: u64::MAX,
            source_id: target_id,
            target: ProjectileTarget::Entity(source_id),
            impact: crate::sim::projectile::ProjectileCoord::new(
                location.x, location.y, location.z,
            ),
            payload: ProjectilePayload::new(
                settings.damage,
                warhead_ref,
                None,
                return_warhead.bright,
            )
            .with_source_house(Some(target_owner)),
            reason: ProjectileDetonationReason::ReturnWarhead,
        };
        let commit = commit_projectiles(world, run, &[packet], rules, overlay_registry);
        out.append(commit.effects);
        out.under_attack_events.extend(commit.under_attack_events);
    } else {
        // `pOwner->ReceiveDamage(&damage, 0, ReturnWarhead, pTarget, false,
        // false, pTarget->Owner)`.
        let event = EntityDamageEvent::direct_receiver(
            source_id,
            settings.damage,
            0,
            target_id,
            Some(target_owner),
            warhead_ref,
            ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        );
        let (nested, mut pings) =
            commit_entities(world, run, &[event], None, rules, overlay_registry);
        out.append(nested);
        out.under_attack_events.append(&mut pings);
    }
}
