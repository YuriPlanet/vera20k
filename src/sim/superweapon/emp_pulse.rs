//! Ares `Type=EMPulse` (`src/Misc/SWTypes/EMPulse.cpp`): the owner's
//! EMP cannons fire at the target cell, or each detonates its weapon on
//! itself.
//!
//! Installed Ares 3.0p1: Activate `0x100721D0`, GetLaunchSiteRange `0x10072390`
//! and IsLaunchSite `0x100724A0` (`targeting::launch_sites`). A fired
//! cannon runs gamemd's Mission_Missile EMP arm
//! (`world::techno_ai::building_missile`), with Ares hooks 44CCE7 and 44CE46.
//!
//! Ledger: no RNG draws here (the cannon's report sound draws Main later);
//! the House's EMPTarget (`+0x5780`) and each cannon's FiringSWType
//! (`+0x5F8`) and TechnoExt SuperWeapon/SuperTarget are written; Commence
//! (`0x005B3570`) restarts the cannon's mission clock through the mission
//! owner. No detach calls.
//!
//! The original end stays fixed if a detonation removes a house building
//! during the walk. Rust retains the original slot indices without reading
//! an expired object reference or using a pointer into a reallocated array.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponType;
use crate::sim::hunter_seeker::AttachedSuper;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionType;
use crate::sim::world::{FrameEffects, Simulation};

/// SW_EMPulse::Activate (Ares 3.0p1 `0x100721D0`) for `owner`'s Super of `sw` at
/// `cell`: the House's EMPTarget becomes the cell; then, up to
/// `SW.MaxCount` (negative: all) of the House's buildings in its list order
/// that are launch sites of the type, within their range of the cell
/// unless `EMPulse.Linked` or `.TargetSelf` (IsLaunchSiteEligible
/// `0x1006EA30`), each either
/// - remembers the Super and the cell (TryGetCellAt: none off the cell
///   grid), takes the type's index as its firing type, queues Missile and
///   commences it; or, under `EMPulse.TargetSelf`,
/// - detonates its weapon 0's bullet at its own coordinates
///   ([`Simulation::emp_cannon_detonate_self`]; nothing without a weapon).
///
/// Always activates.
pub(super) fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type_id: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
    overlays: Option<&OverlayTypeRegistry>,
    effects: &mut FrameEffects<'_>,
) -> bool {
    let Some(emp) = rules
        .super_weapon_metadata(&sw.id)
        .map(|metadata| metadata.launch().emp_pulse())
    else {
        return true;
    };
    let Some(house) = sim.houses.get_mut(&owner) else {
        return true;
    };
    house.aim_emp_cannons(cell);
    let end = house.base_projection_mut().begin_building_slot_walk();
    let count = usize::try_from(emp.max_count).unwrap_or(usize::MAX);
    let ignore_range = emp.linked || emp.target_self;
    let target = (i32::from(cell.0), i32::from(cell.1));
    let super_target = sim
        .resolved_terrain
        .as_ref()
        .and_then(|terrain| terrain.cell(cell.0, cell.1))
        .map(|_| cell);
    let firing_type = rules
        .super_weapon_index(&sw.id)
        .map_or(-1, |index| index as i32);
    let mut fired = 0;
    for slot in 0..end {
        if fired >= count {
            break;
        }
        // 0x10072260 dereferences the current slot. After the synchronous
        // detonation at 0x10072349, 0x1007236A advances it by one.
        let Some(id) = sim
            .houses
            .get(&owner)
            .and_then(|house| house.base_projection().building_slot(slot))
        else {
            continue;
        };
        let Some(site_owner) = sim.substrate.entities.get(id).map(|entity| entity.owner()) else {
            continue;
        };
        let Some(site) = super::targeting::launch_site(sim, rules, site_owner, sw, id) else {
            continue;
        };
        let eligible = if ignore_range {
            site.eligible
        } else {
            site.admits(target, (-1.0, -1.0))
        };
        if !eligible {
            continue;
        }
        fired += 1;
        if emp.target_self {
            sim.emp_cannon_detonate_self(id, rules, overlays, effects);
            continue;
        }
        if let Some(building) = sim.substrate.entities.get_mut(id) {
            building.attach_ares_super_target(
                AttachedSuper {
                    owner,
                    sw_type: sw_type_id,
                },
                super_target,
            );
            building
                .mission_leaf
                .set_building_firing_super_weapon(firing_type);
        }
        crate::sim::world::queue_and_commence(sim, id, MissionType::Missile, rules);
    }
    if let Some(house) = sim.houses.get_mut(&owner) {
        house.base_projection_mut().finish_building_slot_walk();
    }
    true
}
