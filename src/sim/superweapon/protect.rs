//! Ares SW_Protect (`src/Misc/SWTypes/Protect.cpp`) handles `Type=Protect`
//! and redirected `Type=IronCurtain` or `Type=ForceShield`, with their own
//! area, duration, sounds and blackout.
//!
//! Ares 3.0p1: Activate `0x10075800`, Initialize `0x10075C10` (Iron
//! Curtain EVA lines, AnimHeight 5, AITargeting IronCurtain, the IronCurtain
//! cursor), LoadFromINI `0x10075CD0` (`Protect.Duration`, `.PowerOutage`,
//! `.PlayFadeSoundTime`), GetRange `0x10075B80` (the type's `SW.Range`, else
//! a 3x3 rectangle or `ForceShieldRadius=`) and GetAnim `0x10075B40`
//! (`SW.Animation`, else the corresponding stock invoke anim). The raw
//! ForceShield type sets the force byte in this handler.
//!
//! Activate on a charged Super, at the target cell's GetCoords: the type's
//! `StartSound=`, then (with a `SpecialSound=`) the fade countdown of
//! duration less `Protect.PlayFadeSoundTime` (default 0), a positive
//! `Protect.PowerOutage` blackout of the owner, then each Techno on the
//! area's cells (`CellClass::GetContent`) once, in UniqueID order
//! (DistinctCollector), skipping an immobilized (`+0x27C`) or warping-out
//! (`+0x270`) one and those `SW.AffectsHouse`/`SW.AffectsTarget` exclude,
//! gets IronCurtain (vt+0x154) with the duration, the owner and force byte.
//! It reports activation even when uncharged.
//!
//! Ledger: the invoke anim's and each curtain receiver's own RNG draws; timer
//! writes of each curtain, the Super's fade countdown and the owner's
//! blackout; the curtain receivers' detach calls (a parasite's ExitUnit).
//! Each curtain's duration scales by the type's `IronCurtain.Modifier` in
//! the Techno curtain (hook 70E2B0, 1004A880; `invulnerability`).

use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_targeting::TargetMask;
use crate::rules::superweapon_type::{SuperWeaponKind, SuperWeaponType};
use crate::sim::intern::InternedId;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::superweapon::invulnerability::InvulnKind;
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation};

/// Ares 3.0p1 SW_Protect::Activate `0x10075800` for `owner`'s Super of `sw_type`.
/// The Launch hook at `0x006CC390` skips the stock case for this handler and
/// resumes at `0x006CDE40` after the common Launch.
pub(super) fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type: InternedId,
    sw: &SuperWeaponType,
    (rx, ry): (u16, u16),
    overlay_registry: Option<&OverlayTypeRegistry>,
    frame_effects: &mut FrameEffects<'_>,
) {
    if !super::is_charged(sim, owner, sw_type) {
        return;
    }
    let Some(metadata) = rules.super_weapon_metadata(&sw.id) else {
        return;
    };
    let launch = metadata.launch();
    let targeting = metadata.targeting();
    let general = &rules.general;
    let force = sw.kind == SuperWeaponKind::ForceShield;
    // 0x10075869..0x1007592F chooses the defaults using Protect_IsForceShield.
    let (kind, duration, fade, outage) = if force {
        (
            InvulnKind::ForceShield,
            general.force_shield_duration,
            general.force_shield_fade_sound_time,
            general.force_shield_blackout_duration,
        )
    } else {
        (InvulnKind::IronCurtain, general.iron_curtain_duration, 0, 0)
    };
    let duration = launch.protect_duration().unwrap_or(duration);
    // 0x1007585F uses CellClass::GetCoords for StartSound and the fade
    // timer. The shared presentation adds bridge height to its own cues.
    let ground = crate::sim::projectile::cell_ground_coord(sim.resolved_terrain.as_ref(), rx, ry);
    let coords @ [x, y, z] = [ground.x, ground.y, ground.z];
    // 0x1007589F..0x100758AF: StartSound (+0xC4) through VocClass::PlayAt.
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
        let fade = launch.protect_fade_sound_time().unwrap_or(fade);
        instance.arm_fade(duration.wrapping_sub(fade), coords);
    }
    // HouseClass::CreatePowerOutage (0x0050BC90) for a positive blackout.
    let outage = launch.protect_power_outage().unwrap_or(outage);
    if outage > 0 && sim.houses.contains_key(&owner) {
        let frame = sim.session.binary_frame();
        sim.power_states
            .house_mut(owner)
            .start_blackout(frame, outage);
    }
    // GetRange 0x10075B80: SW.Range, else ForceShieldRadius or 3x3.
    let range = match launch.range() {
        (width, height) if width < 0.0 && height < 0 && force => {
            (general.force_shield_radius as f32, -1)
        }
        (width, height) if width < 0.0 && height < 0 => (3.0, 3),
        range => range,
    };
    for id in area_technos(sim, range, (rx, ry)) {
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
            kind,
            frame_effects,
        );
    }
}

/// Helpers::Alex::for_each_in_rect_or_range over an SWRange, collected by a
/// DistinctCollector: the Technos in the cells' GetContent lists, each once,
/// in UniqueID order. An SWRange that is neither a rectangle nor a radius
/// covers nothing.
pub(super) fn area_technos(sim: &Simulation, range: (f32, i32), center: (u16, u16)) -> Vec<u64> {
    area_technos_with(sim, range, center, false)
}

/// Ares 3.0p1 PsyDom_Fire (0x100382E0, hook 0x0053B080): a rectangle, or
/// the CellSpreadEnumerator at 0x10036560. The caller truncates the range
/// at 0x100384D7, ignores nonpositive values, and enumerates through band 256.
pub(super) fn area_technos_in_spread(
    sim: &Simulation,
    range: (f32, i32),
    center: (u16, u16),
) -> Vec<u64> {
    area_technos_with(sim, range, center, true)
}

fn area_technos_with(
    sim: &Simulation,
    (width, height): (f32, i32),
    center: (u16, u16),
    spread: bool,
) -> Vec<u64> {
    let Some(terrain) = sim.resolved_terrain.as_ref() else {
        return Vec::new();
    };
    // A centered rectangle this wide can span every cell of the largest
    // supported map. Refuse larger authored dimensions before enumeration.
    // CellSpread already clamps its input to the native table.
    let max_span = i32::from(crate::map::cell_index::MAX_CELL_SIDE) * 2 + 1;
    let rectangle = height > 0 && (width as i32) > 0;
    if (!spread || rectangle) && (width > max_span as f32 || height > max_span) {
        return Vec::new();
    }
    let (cx, cy) = (i32::from(center.0 as i16), i32::from(center.1 as i16));
    let mut found = std::collections::BTreeMap::new();
    let mut visit = |x, y| {
        // MapClass::TryGetCellAt: only real cells.
        let crate::sim::cell_rect::CellRef::Real(real) =
            crate::sim::cell_rect::get_cellclass_fallback(Some(terrain), x, y)
        else {
            return;
        };
        for member in sim.native_cell_content((real.rx, real.ry)) {
            let CellObjectMember::Entity(id) = member else {
                continue;
            };
            if let Some(entity) = sim.substrate.entities.get(id) {
                found.entry(entity.native_unique_id()).or_insert(id);
            }
        }
    };
    if spread && !rectangle {
        if height <= 0 && (1.0..2_147_483_648.0).contains(&width) {
            let band = (width as u16).min(crate::sim::cell_rect::MAX_CELL_SPREAD);
            for (dx, dy) in crate::sim::cell_rect::cell_spread_rings(0, band) {
                visit(cx + i32::from(dx), cy + i32::from(dy));
            }
        }
    } else if rectangle {
        // for_each_in_rect: width x height cells from center - size/2.
        let width = width as i32;
        let (left, top) = (cx - width / 2, cy - height / 2);
        for y in top..top + height {
            for x in left..left + width {
                visit(x, y);
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
                    visit(x, y);
                }
            }
        }
    }
    found.into_values().collect()
}

/// SWTypeExt::IsTechnoAffected: the Techno's cell must match the land/water
/// bits and its kind the content bits of `SW.AffectsTarget`, each only when
/// the mask names any of them.
pub(super) fn techno_affected(
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
