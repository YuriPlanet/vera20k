//! Phobos `SWTypeExt::ApplyDetonation` (`Ext/SWType/FireSuperWeapon.cpp:267-303`),
//! the `Detonate.*` arm of the shared `FireSuperWeaponExt` tail: a superweapon
//! detonates a weapon or warhead at the target cell (or at its launch
//! building), before `SW.Next`.
//!
//! - `Detonate.Weapon=`: `WeaponTypeExt::DetonateAt` builds a bullet of the
//!   weapon's projectile and explodes it at the coordinates with the weapon's
//!   warhead; `Detonate.Damage` overrides the weapon damage.
//! - else `Detonate.Warhead=`: with `Detonate.Warhead.Full` (default yes) a
//!   bullet with no weapon type explodes there; otherwise the engine
//!   `MapClass::DamageArea` runs directly (no bullet).
//! - `Detonate.AtFirer=`: the coordinates are the launching building's centre;
//!   with no eligible building they are `CoordStruct::Empty` (`(0,0,0)`), as
//!   native.
//!
//! The firer is the first owned building in House+68 order that provides the
//! superweapon, is alive, in play and operational. Native
//! `IsLaunchSiteEligible` also applies `SW.RangeMinimum`/`.RangeMaximum`
//! against the target cell; VERA reads those keys into the Ares metadata and
//! the Phobos superweapon metadata, so both builds gate the firer.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, sim/combat, sim/world.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponType;
use crate::sim::combat::{RAD_NO_ATTACKER, world_receiver};
use crate::sim::intern::InternedId;
use crate::sim::projectile::{
    ProjectileCoord, ProjectileDetonation, ProjectileDetonationReason, ProjectilePayload,
    ProjectileTarget,
};
use crate::sim::world::{FrameEffects, Simulation};

/// The `Detonate.*` arm. `cell` is the launch's target cell.
pub(super) fn apply(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: &mut FrameEffects<'_>,
) {
    let Some(detonate) = rules.phobos_super_weapon_detonate(&sw.id) else {
        return;
    };
    if detonate.is_empty() {
        return;
    }
    let firer = detonation_firer(sim, rules, owner, sw, cell);
    let coords = if detonate.at_firer() {
        firer
            .and_then(|id| {
                let entity = sim.substrate.entities.get(id)?;
                let [x, y] = crate::sim::movement::ground_pose::object_center_xy(entity);
                let location = crate::sim::movement::ground_pose::object_get_coords(
                    entity,
                    sim.resolved_terrain.as_ref(),
                );
                Some(ProjectileCoord::new(x, y, location.z))
            })
            .unwrap_or(ProjectileCoord::new(0, 0, 0))
    } else {
        let [x, y, z] = super::deck_coords(sim, cell);
        ProjectileCoord::new(x, y, z)
    };
    if !coordinates_legal(sim, coords) {
        log::warn!(
            "Phobos Detonate.* of superweapon \"{}\" at ({}, {}) is off the map",
            sw.id,
            coords.x,
            coords.y
        );
        return;
    }
    let source = firer.unwrap_or(RAD_NO_ATTACKER);
    if let Some(weapon_name) = detonate.weapon() {
        let Some(weapon) = rules.weapon(weapon_name) else {
            return;
        };
        let Some(warhead_name) = weapon.warhead.as_deref() else {
            return;
        };
        let damage = detonate.damage().unwrap_or(weapon.damage);
        let warhead_ref = sim.interner.intern(warhead_name);
        let weapon_ref = sim.interner.intern(weapon_name);
        let projectile = weapon
            .projectile
            .as_deref()
            .map(|name| sim.interner.intern(name));
        let payload = ProjectilePayload::new(damage, warhead_ref, projectile, weapon.bright)
            .with_installed_weapon(Some(weapon_ref))
            .with_source_house(Some(owner));
        commit(
            sim,
            rules,
            overlay_registry,
            source,
            coords,
            payload,
            effects,
        );
        return;
    }
    let Some(warhead_name) = detonate.warhead() else {
        return;
    };
    let Some(warhead) = rules.warhead(warhead_name) else {
        return;
    };
    let damage = detonate.damage().unwrap_or(0);
    let warhead_ref = sim.interner.intern(warhead_name);
    if detonate.warhead_full() {
        let payload = ProjectilePayload::new(damage, warhead_ref, None, warhead.bright)
            .with_source_house(Some(owner));
        commit(
            sim,
            rules,
            overlay_registry,
            source,
            coords,
            payload,
            effects,
        );
    } else {
        // `MapClass::DamageArea(coords, damage, pFirer, warhead, true, pHouse)`.
        world_receiver::apply_area_damage(
            sim,
            rules,
            overlay_registry,
            coords,
            damage,
            warhead,
            true,
            (source, Some(owner), warhead_ref),
            effects,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn commit(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    source: u64,
    impact: ProjectileCoord,
    payload: ProjectilePayload,
    effects: &mut FrameEffects<'_>,
) {
    let detonation = ProjectileDetonation {
        projectile_id: source,
        source_id: source,
        target: ProjectileTarget::None,
        impact,
        payload,
        reason: ProjectileDetonationReason::SwDetonate,
    };
    let mut run = world_receiver::ReceiverRun::default().with_effects(effects);
    let commit =
        world_receiver::commit_projectiles(sim, &mut run, &[detonation], rules, overlay_registry);
    let navigation = run.finish();
    sim.absorb_noncombat_damage_effects(
        rules,
        commit.effects,
        commit.under_attack_events,
        navigation,
        effects,
    );
}

/// `SWTypeExt::IsLaunchSiteEligible(cell, building, false)`: the building
/// provides the superweapon, is alive, in play and powered, and within the
/// type's launch-site range (native `GetLaunchSiteRange`).
fn detonation_firer(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
) -> Option<u64> {
    let house = sim.houses.get(&owner)?;
    let range = launch_site_range(rules, sw);
    house
        .base_projection()
        .buildings()
        .iter()
        .copied()
        .find(|&id| {
            let Some(entity) = sim.substrate.entities.get(id) else {
                return false;
            };
            let Some(object) = sim.object_type(entity.type_ref(), rules) else {
                return false;
            };
            if !super::type_super_weapons(object).any(|name| name.eq_ignore_ascii_case(&sw.id)) {
                return false;
            }
            if !entity.is_object_alive() || entity.lifecycle.in_limbo() {
                return false;
            }
            if sim.building_operational_state(id, rules) != Some(true) {
                return false;
            }
            let Some(range) = range else {
                return true;
            };
            let location = crate::sim::movement::ground_pose::object_get_coords(
                entity,
                sim.resolved_terrain.as_ref(),
            );
            let center = (location.x / 256, location.y / 256);
            let dx = f64::from(center.0 - i32::from(cell.0));
            let dy = f64::from(center.1 - i32::from(cell.1));
            let distance_sq = dx * dx + dy * dy;
            let (minimum, maximum) = range;
            (minimum < 0.0 || distance_sq >= minimum * minimum)
                && (maximum < 0.0 || distance_sq <= maximum * maximum)
        })
}

/// `GetLaunchSiteRange` for the types VERA owns range keys for; the negative
/// sentinel answers "no range restriction".
pub(super) fn launch_site_range(rules: &RuleSet, sw: &SuperWeaponType) -> Option<(f64, f64)> {
    {
        rules
            .super_weapon_target_policy(&sw.id)
            .map(|policy| policy.range())
    }
}

/// `MapClass::CoordinatesLegal(CellClass::Coord2Cell(coords))`.
fn coordinates_legal(sim: &Simulation, coords: ProjectileCoord) -> bool {
    let cell = (
        crate::util::lepton::lepton_to_cell(coords.x).clamp(0, i32::from(u16::MAX)) as u16,
        crate::util::lepton::lepton_to_cell(coords.y).clamp(0, i32::from(u16::MAX)) as u16,
    );
    sim.resolved_terrain
        .as_ref()
        .is_some_and(|terrain| terrain.cell(cell.0, cell.1).is_some())
}
