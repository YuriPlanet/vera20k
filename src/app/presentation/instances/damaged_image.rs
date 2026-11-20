//! Phobos damaged-image swaps: the type a Unit or Aircraft draws in the
//! yellow and red condition bands.
//!
//! `TechnoTypeExt` reads the land pair for every techno type
//! (`Ext/TechnoType/Body.cpp:1112-1113`) and `UnitTypeExt` adds the water pair
//! (`Ext/UnitType/Body.cpp:78-79`). Only two classes consume them:
//!
//! - `UnitExt::GetUnitTypeExtra` (`Ext/Unit/Body.cpp:343-375`): yellow takes
//!   `Image.ConditionYellow=`, or `WaterImage.ConditionYellow=` on water; red
//!   takes the red pair with the yellow one as its fallback. The water arm
//!   never falls back to the land pair.
//! - `AircraftExt::GetAircraftTypeExtra` (`Ext/Aircraft/Body.cpp:195-220`):
//!   the same two bands without water.
//!
//! `UnitClass_DrawVXL_WaterType_Extra` (`0x73B4DA`,
//! `Ext/Techno/Hooks.cpp:825-840`) and `UnitClass_DrawSHP_WaterType_Extra`
//! (`0x73C602`, `:842-861`) then draw the replacement type, so the swap
//! replaces the image, the voxel and the turret decisions together. Both hooks
//! ask `NeedDamagedImage`, `IsClearlyVisibleTo(CurrentPlayer)` and
//! `!Deployed`; the draw path that calls this already admitted the object for
//! the current viewer, and a fully deployed Unit left earlier through
//! `UnloadingClass=`.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;

/// `IsGreenHP`/`IsYellowHP` through `Object5F5DD0`, which is what the two
/// `Get*TypeExtra` readers call: 0 red, 1 yellow, 2 green.
fn band(entity: &GameEntity, object: &ObjectType, rules: &RuleSet) -> u8 {
    crate::sim::combat::health_category(
        entity.health.current(),
        object.strength,
        rules.general.condition_red,
        rules.general.condition_yellow,
    )
}

/// The type this frame draws instead of its own, if the current band names
/// one. `water` is `GetCell()->LandType == Water && !OnBridge`.
pub(crate) fn swap<'a>(
    entity: &GameEntity,
    object: &'a ObjectType,
    rules: &'a RuleSet,
    water: bool,
) -> Option<&'a str> {
    swap_health(
        entity.category(),
        entity.health.current(),
        object,
        rules,
        water,
    )
}

/// [`swap`] from the two facts the band reads, for a frame record that is not
/// the live entity.
pub(crate) fn swap_health<'a>(
    category: EntityCategory,
    health: i32,
    object: &'a ObjectType,
    rules: &'a RuleSet,
    water: bool,
) -> Option<&'a str> {
    let images = &object.phobos_damaged_image;
    if !images.need_damaged_image() {
        return None;
    }
    let band = crate::sim::combat::health_category(
        health,
        object.strength,
        rules.general.condition_red,
        rules.general.condition_yellow,
    );
    match (category, band) {
        // Green HP keeps the type's own art.
        (_, 2) => None,
        (EntityCategory::Unit, 1) if water => images.water_band_image(false),
        (EntityCategory::Unit, 0) if water => images
            .water_band_image(true)
            .or_else(|| images.water_band_image(false)),
        (EntityCategory::Unit, 1) => images.band_image(false),
        (EntityCategory::Unit, 0) => images.band_image(true).or_else(|| images.band_image(false)),
        (EntityCategory::Aircraft, 1) => images.band_image(false),
        (EntityCategory::Aircraft, 0) => {
            images.band_image(true).or_else(|| images.band_image(false))
        }
        _ => None,
    }
}

/// `GetCell()->LandType == LandType::Water && !OnBridge`
/// (`Ext/Unit/Body.cpp:352`, `:365`).
pub(crate) fn unit_on_water(
    entity: &GameEntity,
    cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
) -> bool {
    let Some(cells) = cells else {
        return false;
    };
    let at = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
    let cell = cells.lookup_world(at.x, at.y);
    !entity.on_bridge()
        && cells.land_type(cell)
            == i32::from(crate::rules::terrain_rules::LandType::Water.as_index())
}

#[cfg(all(test, tests_app_presentation_instances))]
#[path = "damaged_image_tests.rs"]
mod tests;
