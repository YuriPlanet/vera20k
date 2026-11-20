//! Phobos `WarheadTypeExt::Detonate`'s LaunchSW block
//! (`Ext/WarheadType/Detonate.cpp:121-150`): for every listed type, when the
//! house can pay its `Money.Amount` and (with `RealLaunch`) the Super is
//! present, ready and not suspended, the inhibitor/designator gates pass,
//! the optional money label is emitted and the Super launches at the impact
//! cell. `RealLaunch` consumes the charge; otherwise the launch saves and
//! restores it.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, sim::superweapon and sim::flying_text.

use crate::rules::ruleset::RuleSet;
use crate::rules::warhead_type::WarheadType;
use crate::sim::intern::InternedId;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::world::{FrameEffects, Simulation};

pub(crate) fn apply_on_detonation(
    sim: &mut Simulation,
    rules: &RuleSet,
    house: Option<InternedId>,
    impact: ProjectileCoord,
    warhead: &WarheadType,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    let settings = &warhead.phobos_launch_sw;
    if settings.super_types.is_empty() {
        return;
    }
    let Some(owner) = house else {
        return;
    };
    let cell = (
        crate::util::lepton::lepton_to_cell(impact.x).clamp(0, i32::from(u16::MAX)) as u16,
        crate::util::lepton::lepton_to_cell(impact.y).clamp(0, i32::from(u16::MAX)) as u16,
    );
    for name in &settings.super_types {
        let Some(sw) = rules.super_weapon(name) else {
            continue;
        };
        let Some(type_id) = sim.interner.get(name) else {
            continue;
        };
        let Some(instance) = sim
            .super_weapons
            .get(&owner)
            .and_then(|supers| supers.get(&type_id))
        else {
            continue;
        };
        // `!RealLaunch || (pSuper->IsPresent && pSuper->IsReady && !pSuper->IsSuspended)`
        let ready = instance.is_active() && instance.is_ready() && !instance.is_suspended();
        if settings.real_launch && !ready {
            continue;
        }
        if !super::chaining::affordable(sim, owner, sw) {
            continue;
        }
        if !super::chaining::influence_gates_pass(
            sim,
            rules,
            owner,
            sw,
            cell,
            settings.ignore_inhibitors,
            settings.ignore_designators,
        ) {
            continue;
        }
        if settings.display_money && sw.money_amount != 0 {
            effects.emit_bounty_text(
                sim,
                rules,
                crate::sim::flying_text::BountyText {
                    entity_id: None,
                    amount: sw.money_amount,
                    coord: impact,
                    z_offset: 0,
                    display_to: Some(settings.display_money_houses),
                    display_owner: Some(owner),
                    pixel_offset: settings.display_money_offset,
                },
            );
        }
        super::fire::launch_chained_super(
            sim,
            rules,
            owner,
            type_id,
            sw,
            cell,
            overlay_registry,
            effects,
            settings.real_launch,
        );
    }
}
