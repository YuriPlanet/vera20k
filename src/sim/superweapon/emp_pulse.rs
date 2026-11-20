//! Ares 3.0 `Type=EMPulse` (`src/Misc/SWTypes/EMPulse.cpp`): the owner's
//! EMP cannons fire at the target cell, or each detonates its weapon on
//! itself.
//!
//! Installed Ares: Activate `0x100711C0`, GetLaunchSiteRange `0x10071380`
//! and IsLaunchSite `0x10071490` (`targeting::launch_sites`), Initialize
//! `0x10071440` and LoadFromINI `0x10071510` (the Rules owners). A fired
//! cannon runs gamemd's Mission_Missile EMP arm
//! (`world::techno_ai::building_missile`), with Ares hooks 44CCE7 and 44CE46.
//!
//! Ledger: no RNG draws here (the cannon's report sound draws Main later);
//! the House's EMPTarget (`+0x5780`) and each cannon's FiringSWType
//! (`+0x5F8`) and TechnoExt SuperWeapon/SuperTarget are written; Commence
//! (`0x005B3570`) restarts the cannon's mission clock through the mission
//! owner. No detach calls.
//!
//! RESIDUALS:
//! - Activate walks `Owner->Buildings` live; a `TargetSelf` detonation that
//!   removes a building there shifts the vector under the walk, so native
//!   skips the building after it. VERA walks the list as it stood and skips
//!   only buildings no longer eligible. Trigger: a self-detonating cannon
//!   that kills another of the House's buildings; rare.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponType;
use crate::sim::hunter_seeker::AttachedSuper;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionType;
use crate::sim::world::{FrameEffects, Simulation};

/// SW_EMPulse::Activate (`0x100711C0`) for `owner`'s Super of `sw` at
/// `cell`: the House's EMPTarget becomes the cell; then, up to
/// `SW.MaxCount` (negative: all) of the House's buildings in its list order
/// that are launch sites of the type, within their range of the cell
/// unless `EMPulse.Linked` or `.TargetSelf` (IsLaunchSiteEligible
/// `0x1006DA20`), each either
/// - remembers the Super and the cell (TryGetCellAt: none off the cell
///   grid), takes the type's index as its firing type, queues Missile and
///   commences it; or, under `EMPulse.TargetSelf`,
/// - detonates its weapon 0's bullet at its own coordinates
///   ([`Simulation::emp_cannon_detonate_self`]; nothing without a weapon).
///
/// Always activates.
#[allow(clippy::too_many_arguments)]
pub(super) fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type_id: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
    overlays: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> bool {
    let Some(emp) = rules
        .super_weapon_metadata(&sw.id)
        .map(|metadata| metadata.launch().emp_pulse())
    else {
        return true;
    };
    if let Some(house) = sim.houses.get_mut(&owner) {
        house.aim_emp_cannons(cell);
    }
    // `FireSuperWeaponExt` (`Ext/SWType/FireSuperWeapon.cpp:37`) calls
    // HandleEMPulseLaunch only for a non-TargetSelf EMPulse. VERA reaches
    // the same tail from the Ares launch owner rather than adding a second
    // launch path.
    if !emp.target_self {
        suspend_phobos_others(sim, rules, owner, sw_type_id, &emp);
    }
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
    let buildings = sim
        .houses
        .get(&owner)
        .map(|house| house.base_projection().buildings().to_vec())
        .unwrap_or_default();
    let mut fired = 0;
    for id in buildings {
        if fired >= count {
            break;
        }
        // The walk asks each building as it reaches it.
        let Some(site) = super::targeting::launch_site(sim, rules, owner, sw, id) else {
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
            // `HandleEMPulseLaunch`'s per-cannon latch
            // (`Ext/SWType/FireSuperWeapon.cpp:365`).
            if !emp.target_self {
                building.set_phobos_current_empulse_sw(sw_type_id);
            }
            building
                .mission_leaf
                .set_building_firing_super_weapon(firing_type);
        }
        crate::sim::world::queue_and_commence(sim, id, MissionType::Missile, rules);
    }
    true
}

/// `SWTypeExt::HandleEMPulseLaunch`'s SuspendOthers arm
/// (`Ext/SWType/FireSuperWeapon.cpp:380-404`): every other EMPulse Super of
/// the House is held when both cannon lists are empty (no restriction) or
/// when the two lists share a type. `HouseExt::SuspendedEMPulseSWs` keeps
/// the held type IDs under the firing SW type until Mission_Missile resumes
/// them (`Ext/Building/Hooks.cpp:170-180`).
fn suspend_phobos_others(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    firing: InternedId,
    emp: &crate::rules::superweapon_type::AresEMPulseRules,
) {
    if !emp.empulse_suspend_others {
        return;
    }
    let held: Vec<InternedId> = {
        let Some(supers) = sim.super_weapons.get(&owner) else {
            return;
        };
        let interner = &sim.interner;
        supers
            .iter()
            .filter_map(|(&ty, _)| {
                if ty == firing {
                    return None;
                }
                let name = interner.resolve(ty);
                let other = rules.super_weapon(name)?;
                if other.kind != crate::rules::superweapon_type::SuperWeaponKind::EMPulse {
                    return None;
                }
                let other_cannons = &rules
                    .super_weapon_metadata(name)?
                    .launch()
                    .emp_pulse()
                    .cannons;
                let shares_cannon = emp.cannons.iter().any(|ours| {
                    other_cannons
                        .iter()
                        .any(|theirs| ours.eq_ignore_ascii_case(theirs))
                });
                (emp.cannons.is_empty() && other_cannons.is_empty() || shares_cannon).then_some(ty)
            })
            .collect()
    };
    let frame = sim.session.binary_frame() as u32;
    for &ty in &held {
        if let Some(instance) = sim.super_weapons.instance_mut(&owner, &ty) {
            instance.phobos_set_suspended(true, frame);
        }
    }
    if let Some(house) = sim.houses.get_mut(&owner) {
        house.hold_emp_pulse_sws(firing, held);
    }
}
