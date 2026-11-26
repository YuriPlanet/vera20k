//! Ares `Type=GenericWarhead`: a synchronous detonation, not a projectile.
//!
//! Source: Ares `src/Misc/SWTypes/GenericWarhead.cpp::Activate` and
//! `src/Misc/SWTypes.cpp::IsLaunchSite`. Payload dispatch also follows
//! ra2-mono TryQueueSupportWarheadSuperweapon's resolved-WH / damage / owner
//! boundary; deferred support effects are intentionally not copied here.
//! Ares 3.0p1 Activate (0x10072670) runs its warhead effects directly from
//! the launch site. There is no BulletClass or projectile cluster loop.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponType;
use crate::sim::combat::{
    CombatLightRequest, DeathEffects, RAD_NO_ATTACKER, detonation_anim, world_receiver,
};
use crate::sim::intern::InternedId;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::world::{FrameEffects, Simulation};

#[cfg(all(test, tests_sim_superweapon))]
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
    effects: &mut FrameEffects<'_>,
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
    let mut run = world_receiver::ReceiverRun::default().with_effects(effects);
    let mut death = DeathEffects::default();
    // P1 0x10072788..0x100727BE calls the ripple, curtain, EMP and attach
    // effect owners in this order, before inspecting the cell's live head.
    sim.ares_apply_ripples([x, y, z], warhead);
    sim.apply_ares_iron_curtain_detonation(
        &mut run,
        impact,
        Some(owner),
        warhead,
        settings.damage,
        rules,
        overlays,
        &mut death,
    );
    sim.apply_ares_emp_detonation(
        true,
        impact,
        (source != RAD_NO_ATTACKER).then_some(source),
        warhead,
        rules,
        run.effects(),
    );
    sim.apply_ares_attach_effect_detonation(true, impact, Some(owner), warhead, rules);
    let target = match sim.native_cell_content(cell).next() {
        Some(crate::sim::occupancy::CellObjectMember::Entity(id)) => Some(id),
        _ => None,
    };
    // P1 0x100727DD..0x100727EA passes the first selected-layer object
    // to applyPermaMC. Success skips damage, the explosion anim and flash.
    let captured = sim.apply_ares_perma_mc_detonation(
        target,
        Some(owner),
        warhead,
        rules,
        overlays,
        run.effects(),
    );
    let mut pings = Vec::new();
    let crit_active = if captured {
        false
    } else {
        // P1 0x10072800 calls 0x1000AC30 -> MapClass::DamageArea 0x00489280.
        // Its spread hooks replace the stock offset table for this launch too.
        let (area_death, crit_active, _) = world_receiver::apply_area_damage_in_run_with_options(
            sim,
            &mut run,
            rules,
            overlays,
            impact,
            settings.damage,
            warhead,
            true,
            (source, Some(owner), warhead_ref),
            world_receiver::AreaDamageOptions {
                use_ares_spread: true,
                ..world_receiver::AreaDamageOptions::default()
            },
            &mut pings,
        );
        death.append(area_death);
        crit_active
    };
    let navigation = run.finish();
    sim.absorb_noncombat_damage_effects(rules, death, pings, navigation, effects);
    if captured {
        return true;
    }
    // Ares selects its damage animation and flash after DamageArea (which
    // may synchronously destroy the source or change the target's land).
    let land = detonation_anim::land_at(sim, impact);
    if let Some(effect) = detonation_anim::effect(
        sim,
        rules,
        warhead,
        settings.damage,
        land,
        impact,
        impact,
        crit_active,
    ) {
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
