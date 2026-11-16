//! Phobos `BuildingExt::DisplayIncomeString` (`Ext/Building/Body.cpp:12-36`),
//! run by the `BuildingClass_AI` hook at `0x43FE69` right after
//! `TechnoClass_AI`.
//!
//! Every non-zero `AccumulatedIncome` is printed at the building's
//! `GetRenderCoords()` on frames where `frame % delay == 0`, filtered by the
//! type's/global `DisplayIncome.Houses` and by `DisplayIncome_AllowAI`, then
//! reset whether or not the label was emitted.
//!
//! ## Dependency rules
//! - Part of sim/ — presentation consumes the emitted label, never the reverse.

use crate::rules::ruleset::RuleSet;
use crate::sim::flying_text::BountyText;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::world::{FrameEffects, Simulation};

/// One building's periodic income label; `effects` carries it to the client.
pub(crate) fn display_income_string(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    effects: FrameEffects<'_>,
) {
    let frame = sim.session.binary_frame() as i32;
    let Some(building) = sim.substrate.entities.get(id) else {
        return;
    };
    let accumulated = building.phobos_accumulated_income();
    if accumulated == 0 {
        return;
    }
    let Some(object) = sim.object_type(building.type_ref(), rules) else {
        return;
    };
    let settings = object.phobos_display_income;
    let delay = settings
        .delay
        .unwrap_or(rules.phobos_globals.display_income_delay);
    // Both readers force an authored zero to one, so the modulo is safe.
    if delay <= 0 || frame % delay != 0 {
        return;
    }
    let owner = building.owner();
    let human = sim.house_is_human_player(owner);
    let enabled = settings
        .enabled
        .unwrap_or(rules.phobos_globals.display_income);
    let coords = crate::sim::movement::ground_pose::object_get_coords(
        building,
        sim.resolved_terrain.as_ref(),
    );
    // Phobos clears the accumulator before the filters can reject the label
    // (`Ext/Building/Body.cpp:34`).
    if let Some(building) = sim.substrate.entities.get_mut(id) {
        building.set_phobos_accumulated_income(0);
    }
    if !(rules.phobos_globals.display_income_allow_ai || human) || !enabled {
        return;
    }
    effects.emit_bounty_text(
        sim,
        rules,
        BountyText {
            entity_id: Some(id),
            amount: accumulated,
            coord: ProjectileCoord::new(coords.x, coords.y, coords.z),
            z_offset: 0,
            display_to: Some(
                settings
                    .houses
                    .unwrap_or(rules.phobos_globals.display_income_houses),
            ),
            display_owner: None,
            pixel_offset: settings.offset,
        },
    );
}

#[cfg(test)]
#[path = "building_display_income_tests.rs"]
mod tests;
