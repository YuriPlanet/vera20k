//! Ares `Type=Protect` (`src/Misc/SWTypes/Protect.cpp`): an Iron Curtain
//! over a rectangle or range with its own duration, sounds and blackout.
//!
//! Installed Ares: Activate `0x10074750`, Initialize `0x10074B60` (Iron
//! Curtain EVA lines, AnimHeight 5, AITargeting IronCurtain, the IronCurtain
//! cursor), LoadFromINI `0x10074C20` (`Protect.Duration`, `.PowerOutage`,
//! `.PlayFadeSoundTime`), GetRange `0x10074AD0` (the type's `SW.Range`, else
//! a 3x3 rectangle) and GetAnim `0x10074A90` (`SW.Animation`, else
//! `IronCurtainInvokeAnim=`, built by the common Launch). A Protect type is
//! never the Force Shield arm: Protect_IsForceShield is set only for a
//! redirected `Type=ForceShield`, whose stock owner VERA keeps.
//!
//! Activate on a charged Super, at the target cell's GetCoords: the type's
//! `StartSound=`, then (with a `SpecialSound=`) the fade countdown of
//! duration less `Protect.PlayFadeSoundTime` (default 0), a positive
//! `Protect.PowerOutage` blackout of the owner, then each Techno on the
//! area's cells (`CellClass::GetContent`) once, in UniqueID order
//! (DistinctCollector), skipping an immobilized (`+0x27C`) or warping-out
//! (`+0x270`) one and those `SW.AffectsHouse`/`SW.AffectsTarget` exclude,
//! gets IronCurtain (vt+0x154) with the duration, the owner and force 0.
//! It reports activation even when uncharged.
//!
//! Ledger: the invoke anim's and each curtain receiver's own RNG draws; timer
//! writes of each curtain, the Super's fade countdown and the owner's
//! blackout; the curtain receivers' detach calls (a parasite's ExitUnit).
//! RESIDUAL: TechnoTypeExt `IronCurtain.Modifier` (hook 70E2B0, 1004A880)
//! scales each curtain's duration; VERA applies the duration unscaled (the
//! default modifier 1.0 gives the same frames).

use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_targeting::TargetMask;
use crate::rules::superweapon_type::SuperWeaponType;
use crate::sim::intern::InternedId;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation};

/// SW_Protect::Activate `0x10074750` for `owner`'s Super of `sw_type`.
pub(super) fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type: InternedId,
    sw: &SuperWeaponType,
    (rx, ry): (u16, u16),
    overlay_registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    if !super::is_charged(sim, owner, sw_type) {
        return;
    }
    let Some(metadata) = rules.super_weapon_metadata(&sw.id) else {
        return;
    };
    let launch = metadata.launch();
    let targeting = metadata.targeting();
    let duration = launch
        .protect_duration()
        .unwrap_or(rules.general.iron_curtain_duration);
    let coords @ [x, y, z] = super::deck_coords(sim, (rx, ry));
    // 1007479B: StartSound (+0xC4) through VocClass::PlayAt.
    if sw.start_sound.is_some() {
        sim.sound_events.emit(SimSoundEvent::SuperWeaponActivation {
            sw_type,
            world: crate::sim::anim_class::AnimWorldCoord { x, y, z },
        });
    }
    // The SpecialSound countdown and its location (Super+0x50/+0x54).
    if sw.special_sound.is_some()
        && let Some(instance) = sim.super_weapons.instance_mut(&owner, &sw_type)
    {
        let fade = launch.protect_fade_sound_time().unwrap_or(0);
        instance.arm_fade(duration.wrapping_sub(fade), coords);
    }
    // HouseClass::CreatePowerOutage (0x0050BC90) for a positive blackout.
    let outage = launch.protect_power_outage().unwrap_or(0);
    if outage > 0 && sim.houses.contains_key(&owner) {
        let frame = sim.session.binary_frame();
        sim.power_states
            .house_mut(owner)
            .start_blackout(frame, outage);
    }
    for id in area_technos(sim, launch.range(), (rx, ry)) {
        let Some(entity) = sim.substrate.entities.get(id) else {
            continue;
        };
        if entity.chrono_warp_latch() || entity.is_warped_out() {
            continue;
        }
        let relation = sim.ares_house_relation(owner, entity.owner());
        if !targeting.affects_house().intersects(relation)
            || !techno_affected(sim, entity, targeting.affects_target())
        {
            continue;
        }
        let category = entity.category();
        super::iron_curtain::curtain_object(
            sim,
            rules,
            overlay_registry,
            id,
            category,
            Some(owner),
            duration,
            frame_effects,
        );
    }
}

/// Helpers::Alex::for_each_in_rect_or_range over GetRange10074AD0's area
/// (`SW.Range`, else 3x3), collected by a DistinctCollector: the Technos
/// in the cells' GetContent lists, each once, in UniqueID order.
fn area_technos(sim: &Simulation, (width, height): (f32, i32), center: (u16, u16)) -> Vec<u64> {
    let (width, height) = if width < 0.0 && height < 0 {
        (3.0, 3)
    } else {
        (width, height)
    };
    let (cx, cy) = (i32::from(center.0 as i16), i32::from(center.1 as i16));
    let mut cells = Vec::new();
    if height > 0 && (width as i32) > 0 {
        // for_each_in_rect: width x height cells from center - size/2.
        let width = width as i32;
        let (left, top) = (cx - width / 2, cy - height / 2);
        for y in top..top + height {
            for x in left..left + width {
                cells.push((x, y));
            }
        }
    } else if height <= 0 && width >= 0.0 {
        // CellRangeEnumerator: LTRB of range = 2*floor(r+0.99)+1 from
        // center - range/2 to left/top + range (inclusive), kept within the
        // squared cell distance.
        let radius = f64::from(width);
        let range = ((radius + 0.99).floor() as i32) * 2 + 1;
        let (left, top) = (cx - range / 2, cy - range / 2);
        for y in top..=top + range {
            for x in left..=left + range {
                let (dx, dy) = (f64::from(x - cx), f64::from(y - cy));
                if dx * dx + dy * dy <= radius * radius {
                    cells.push((x, y));
                }
            }
        }
    }
    let terrain = sim.resolved_terrain.as_ref();
    let mut found = std::collections::BTreeMap::new();
    for (x, y) in cells {
        // MapClass::TryGetCellAt: only real cells.
        let crate::sim::cell_rect::CellRef::Real(real) =
            crate::sim::cell_rect::get_cellclass_fallback(terrain, x, y)
        else {
            continue;
        };
        for member in sim.native_cell_content((real.rx, real.ry)) {
            let CellObjectMember::Entity(id) = member else {
                continue;
            };
            if let Some(entity) = sim.substrate.entities.get(id) {
                found.entry(entity.native_unique_id()).or_insert(id);
            }
        }
    }
    found.into_values().collect()
}

/// SWTypeExt::IsTechnoAffected: the Techno's cell must match the land/water
/// bits and its kind the content bits of `SW.AffectsTarget`, each only when
/// the mask names any of them.
fn techno_affected(
    sim: &Simulation,
    entity: &crate::sim::game_entity::GameEntity,
    allowed: TargetMask,
) -> bool {
    if allowed.intersects(TargetMask::ALL_CELLS) {
        let coords = crate::sim::movement::ground_pose::object_get_coords(
            entity,
            sim.resolved_terrain.as_ref(),
        );
        let water = sim
            .resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.cell((coords.x / 256) as u16, (coords.y / 256) as u16))
            .is_some_and(|cell| {
                cell.land_type == crate::rules::terrain_rules::LandType::Water.as_index()
            });
        let needed = if water {
            TargetMask::WATER
        } else {
            TargetMask::LAND
        };
        if !allowed.intersects(needed) {
            return false;
        }
    }
    if allowed.intersects(TargetMask::ALL_CONTENTS) {
        let kind = match entity.category() {
            EntityCategory::Infantry => TargetMask::INFANTRY,
            EntityCategory::Unit | EntityCategory::Aircraft => TargetMask::UNIT,
            EntityCategory::Structure => TargetMask::BUILDING,
        };
        return allowed.intersects(kind);
    }
    true
}
