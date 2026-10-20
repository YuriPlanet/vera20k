//! Refinery detection, completion-owned FreeUnit spawning, and fallback cell finding.
//!
//! Extracted from production_placement.rs for file-size limits.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::find_nearby_cell::NearbySearchOptions;
use crate::sim::world::FrameEffects;
use crate::sim::world::{PlacementEvidence, Simulation};

use crate::rules::foundation::foundation_dimensions;

/// Native primary FreeUnit facing byte. Under the project facing convention,
/// 0xC0 is west.
const FREE_UNIT_FACING_PRIMARY: u8 = 0xC0;
/// Native fallback FreeUnit facing byte. Under the project facing convention,
/// 0xA0 is southwest.
const FREE_UNIT_FACING_FALLBACK: u8 = 0xA0;

/// The two ordered nearby-cell searches gamemd runs once the primary placement is
/// refused. Both calls are argument-for-argument identical except for overlay
/// rejection: the first pass refuses to drop the free unit on a cell carrying an
/// overlay (so it walks out to bare ground instead of standing on the ore field),
/// and only the second pass accepts one.
const FREE_UNIT_FALLBACK_ATTEMPTS: [NearbySearchOptions; 2] = [
    NearbySearchOptions {
        reject_any_overlay: true,
    },
    NearbySearchOptions {
        reject_any_overlay: false,
    },
];

/// Original Grand_Opening446AA9's FreeUnit tail. It runs inline for one
/// building, so its constructor and Unlimbo are visible to subsequent Logic
/// visits. ScenarioInit/capture gates belong to the caller.
///
/// Residual: the human stored-price300 gate (446AF2..446B10) is not retained
/// by the existing production object owner. Retail reach requires a refinery
/// with a nonpositive miner Cost_Of (26 Industrial Plants). Its old late
/// completion caller also omitted this gate; no basic construction type has
/// FreeUnit. The original spatial FreeUnit corpus bounds placement below.
pub(crate) fn spawn_building_free_unit(
    sim: &mut Simulation,
    stable_id: u64,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> bool {
    let Some((owner_id, type_ref, rx, ry, width, height)) =
        sim.substrate.entities.get(stable_id).and_then(|entity| {
            if entity.category() != EntityCategory::Structure || entity.death.is_dying() {
                return None;
            }
            let (width, height) = foundation_dimensions(entity.type_facts.foundation());
            Some((
                entity.owner(),
                entity.type_ref(),
                entity.position.rx(),
                entity.position.ry(),
                width,
                height,
            ))
        })
    else {
        return false;
    };
    let owner = sim.interner.resolve(owner_id).to_owned();
    let building_type_id = sim.interner.resolve(type_ref).to_owned();
    try_spawn_building_free_unit(
        sim,
        rules,
        &owner,
        &building_type_id,
        rx,
        ry,
        width,
        height,
        overlay_registry,
        effects,
    )
}

fn try_spawn_building_free_unit(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: &str,
    building_type_id: &str,
    building_rx: u16,
    building_ry: u16,
    width: u16,
    height: u16,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> bool {
    let Some(free_unit_type) = rules.building_free_unit(building_type_id) else {
        return false;
    };
    let free_unit_type = free_unit_type.to_owned();

    let primary = primary_free_unit_cell(building_rx, building_ry, width, height);
    // Both nearby searches are seeded from the building's NORTH-WEST footprint cell —
    // not the foundation centre the primary cell is derived from, and not the
    // footprint rectangle.
    let search_seed = (building_rx, building_ry);

    // Native constructs one UnitClass, then retries Unlimbo on that same object.
    // Keep that one stable ID at its constructor's zero Location until a
    // placement commits. Each attempt supplies its own coordinate, and the
    // object is not on the map until one does.
    let (initial_rx, initial_ry) = primary.unwrap_or(search_seed);
    let initial_z = sim.terrain_cell_level(initial_rx, initial_ry).unwrap_or(0);
    let Some(free_unit_id) = sim.construct_object_limbo_at_height(
        &free_unit_type,
        owner,
        initial_rx,
        initial_ry,
        FREE_UNIT_FACING_PRIMARY,
        initial_z,
        rules,
        effects,
    ) else {
        let refund = refund_free_unit(sim, rules, owner, &free_unit_type);
        log::warn!(
            "Completed refinery {} could not construct free unit {}; refunded {} to {}",
            building_type_id,
            free_unit_type,
            refund,
            owner,
        );
        return false;
    };

    if let Some((primary_rx, primary_ry)) = primary
        && try_place_free_unit(
            sim,
            free_unit_id,
            primary_rx,
            primary_ry,
            FREE_UNIT_FACING_PRIMARY,
            rules,
            overlay_registry,
            effects,
        )
    {
        log::info!(
            "Completed refinery {} spawned free {} at ({},{}) for {}",
            building_type_id,
            free_unit_type,
            primary_rx,
            primary_ry,
            owner
        );
        return true;
    }

    // Exactly two ordered nearby searches, each followed by one placement try. The
    // second attempt runs only when the first produced no cell or its placement was
    // refused, and each search runs here — after the primary has failed — rather
    // than being precomputed, so it sees the occupancy the placement will meet.
    for options in FREE_UNIT_FALLBACK_ATTEMPTS {
        let Some((fallback_rx, fallback_ry)) =
            find_free_unit_nearby_cell(sim, rules, &free_unit_type, search_seed, options)
        else {
            continue;
        };
        if try_place_free_unit(
            sim,
            free_unit_id,
            fallback_rx,
            fallback_ry,
            FREE_UNIT_FACING_FALLBACK,
            rules,
            overlay_registry,
            effects,
        ) {
            log::info!(
                "Completed refinery {} spawned free {} at fallback ({},{}) for {}",
                building_type_id,
                free_unit_type,
                fallback_rx,
                fallback_ry,
                owner
            );
            return true;
        }
    }

    // gamemd refunds before uninitializing the constructed UnitClass.
    let refund = refund_free_unit(sim, rules, owner, &free_unit_type);
    sim.uninit_with_context(
        free_unit_id,
        crate::sim::world::UninitContext::new(Some(rules), overlay_registry).with_effects(effects),
    );
    log::warn!(
        "Completed refinery {} could not place free unit {}; refunded {} to {}",
        building_type_id,
        free_unit_type,
        refund,
        owner,
    );
    false
}

fn try_place_free_unit(
    sim: &mut Simulation,
    free_unit_id: u64,
    rx: u16,
    ry: u16,
    facing: u8,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> bool {
    // BuildingClass::OnConstructionComplete @ 0x00445F80 invokes the newly
    // constructed UnitClass through virtual Unlimbo for every primary/fallback
    // attempt. ObjectClass::Unlimbo @ 0x005F4EC0 reaches the concrete
    // UnitClass::Can_Enter_Cell @ 0x0073F0A0 before Reveal mutates coordinates
    // or occupancy, so this path must use the same exact gate as every other
    // newly constructed Unit placement. Stock 4x3 refinery primary cells remain
    // occupied by their own live building object and therefore enter the native
    // nearby-search sequence; independent occupants are evaluated in that same
    // ordered cell list rather than through the old coarse ground-blocker model.
    sim.reveal_constructed_object_at_height_with_overlay_context(
        free_unit_id,
        rx,
        ry,
        facing,
        sim.terrain_cell_level(rx, ry).unwrap_or(0),
        PlacementEvidence::EvaluateMark,
        rules,
        overlay_registry,
        effects,
    )
    .is_some()
}

/// A free unit that cannot be made or placed pays its house its GetRefund with
/// `full` set ([`super::type_refund`]; `0x00446E71..0x00446E8F`,
/// `0x00446EB9..0x00446EDD`) through `HouseClass::Add_Credits @ 0x004F9950`.
/// Returns the amount credited.
fn refund_free_unit(sim: &mut Simulation, rules: &RuleSet, owner: &str, unit_type: &str) -> i32 {
    let Some(owner_id) = sim.interner.get(owner) else {
        return 0;
    };
    let refund = match (
        rules.object(unit_type),
        sim.houses.get(&owner_id),
        sim.house_cost_factors(owner_id),
    ) {
        (Some(object), Some(house), Some(factors)) => super::type_refund(
            rules,
            object,
            house,
            &factors,
            sim.session.game_mode_nonzero(),
            true,
        ),
        _ => return 0,
    };
    crate::sim::credit_income::add_credits(&mut sim.houses, owner_id, refund);
    refund
}

fn primary_free_unit_cell(
    building_rx: u16,
    building_ry: u16,
    width: u16,
    height: u16,
) -> Option<(u16, u16)> {
    let center_x = u32::from(building_rx).checked_add(u32::from(width) / 2)?;
    let center_y = u32::from(building_ry).checked_add(u32::from(height) / 2)?;
    let primary_y = center_y.checked_add(1)?;
    Some((
        u16::try_from(center_x).ok()?,
        u16::try_from(primary_y).ok()?,
    ))
}

/// One of the two ordered nearby-cell searches for a refused FreeUnit placement
/// (`0x00446BF4..0x00446CCD`, `0x00446D42..0x00446E10`): seeded at the building's
/// Location cell (`+0x9C`, its north-west footprint cell) for the free unit's
/// MovementZone, with the overlay rejection `options` carries.
///
/// The seed is under the building, but Cell Recalc's zone class ignores
/// buildings, except a FirestormWall one (`0x00483D8E`; none in retail), so
/// the seed answers the zone of the ground around it.
fn find_free_unit_nearby_cell(
    sim: &Simulation,
    rules: &RuleSet,
    free_unit_type: &str,
    seed: (u16, u16),
    options: NearbySearchOptions,
) -> Option<(u16, u16)> {
    let free_unit = rules.object(free_unit_type)?;
    sim.nearby_wheel_cell(seed, free_unit.movement_zone, options)
}

#[cfg(test)]
mod tests {
    use super::FREE_UNIT_FALLBACK_ATTEMPTS;

    #[test]
    fn fallback_attempts_reject_overlays_first_then_allow_them() {
        // Exactly two attempts, and the only argument that differs between them is
        // overlay rejection — set on the first, clear on the second. Reversing the
        // order would drop the free unit onto the ore field it should walk off.
        assert_eq!(FREE_UNIT_FALLBACK_ATTEMPTS.len(), 2);
        assert!(FREE_UNIT_FALLBACK_ATTEMPTS[0].reject_any_overlay);
        assert!(!FREE_UNIT_FALLBACK_ATTEMPTS[1].reject_any_overlay);
    }
}
