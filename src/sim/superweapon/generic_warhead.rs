//! Ares `Type=GenericWarhead`: a synchronous detonation, not a projectile.
//!
//! Source: Ares `src/Misc/SWTypes/GenericWarhead.cpp::Activate` and
//! `src/Misc/SWTypes.cpp::IsLaunchSite`. Payload dispatch also follows
//! ra2-mono TryQueueSupportWarheadSuperweapon's resolved-WH / damage / owner
//! boundary; deferred support effects are intentionally not copied here.
//! Reuse the combat receiver, animation and light owners; Ares-specific
//! warhead extensions (EMP, attached effects, etc.) are not introduced here.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponType;
use crate::sim::combat::{CombatLightRequest, RAD_NO_ATTACKER, detonation_anim, world_receiver};
use crate::sim::intern::InternedId;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::world::{FrameEffects, Simulation};

#[cfg(test)]
#[path = "generic_warhead_tests.rs"]
mod tests;

pub(super) fn can_launch(
    sim: &Simulation,
    rules: &RuleSet,
    sw: &SuperWeaponType,
    (x, y): (u16, u16),
) -> bool {
    rules.super_weapon_target_policy(&sw.id).is_some()
        && sw
            .generic_warhead
            .as_ref()
            .and_then(|settings| settings.warhead.as_deref())
            .and_then(|id| rules.warhead(id))
            .is_some()
        && sim
            .resolved_terrain
            .as_ref()
            .is_some_and(|terrain| terrain.cell(x, y).is_some())
}

pub(super) fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
    overlays: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> bool {
    if !can_launch(sim, rules, sw, cell) {
        return false;
    }
    // ClickFire validates these before consuming readiness.
    let settings = sw
        .generic_warhead
        .as_ref()
        .expect("GenericWarhead settings");
    let warhead = rules
        .warhead(settings.warhead.as_deref().expect("bound warhead"))
        .expect("resolved warhead");
    let source = super::targeting::launch_sites(sim, rules, owner, sw)
        .into_iter()
        .find(|(_, site)| {
            site.admits(
                (i32::from(cell.0), i32::from(cell.1)),
                rules
                    .super_weapon_target_policy(&sw.id)
                    .expect("extension policy")
                    .range(),
            )
        })
        .map(|(id, _)| id)
        .unwrap_or(RAD_NO_ATTACKER);
    let [x, y, z] = super::deck_coords(sim, cell);
    let impact = ProjectileCoord::new(x, y, z);
    let warhead_ref = sim.interner.intern(&warhead.id);
    world_receiver::apply_area_damage(
        sim,
        rules,
        overlays,
        impact,
        settings.damage,
        warhead,
        (source, Some(owner), warhead_ref),
        effects,
    );
    // Ares selects its damage animation and flash after DamageArea (which
    // may synchronously destroy the source or change the target's land).
    let land = detonation_anim::land_at(sim, impact);
    if let Some(effect) =
        detonation_anim::effect(sim, rules, warhead, settings.damage, land, impact, impact)
    {
        crate::sim::world::damage_consequences::admit_explosion_effect(sim, rules, effect);
    }
    sim.combat_light_requests.emit(CombatLightRequest::Impact {
        target_id: None,
        damage: settings.damage,
        warhead_ref,
        coord: impact,
        force_create: false,
        flags: 0,
    });
    true
}
