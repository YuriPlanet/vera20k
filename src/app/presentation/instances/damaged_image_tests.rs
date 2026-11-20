//! The damaged-image bands of [`super::swap`]: the green/yellow/red split, the
//! red band's fallback to the yellow one, and the water pair that replaces the
//! land pair instead of following it.

use super::swap;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;

/// `[AudioVisual] ConditionYellow=50% / ConditionRed=25%`, one Unit type and
/// one Aircraft type whose keys the case passes in.
fn rules(unit: &str, aircraft: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[AudioVisual]\nConditionYellow=50%\nConditionRed=25%\n\
         [VehicleTypes]\n0=TANK\n[TANK]\nStrength=200\n{unit}\
         [AircraftTypes]\n0=PLANE\n[PLANE]\nStrength=200\n{aircraft}"
    )))
    .expect("rules")
}

fn entity(category: EntityCategory, health: i32) -> GameEntity {
    let type_name = match category {
        EntityCategory::Aircraft => "PLANE",
        _ => "TANK",
    };
    let mut entity =
        GameEntity::test_default_of_category(1, type_name, "Americans", 10, 10, category);
    entity.health.set_for_test(health);
    entity
}

fn unit_swap<'a>(rules: &'a RuleSet, health: i32, water: bool) -> Option<&'a str> {
    swap(
        &entity(EntityCategory::Unit, health),
        rules.object("TANK").unwrap(),
        rules,
        water,
    )
}

fn aircraft_swap<'a>(rules: &'a RuleSet, health: i32) -> Option<&'a str> {
    swap(
        &entity(EntityCategory::Aircraft, health),
        rules.object("PLANE").unwrap(),
        rules,
        false,
    )
}

/// `Ext/TechnoType/Body.cpp:1114`: neither key set leaves `NeedDamagedImage`
/// false, so every band keeps the type's own art.
#[test]
fn unset_keys_never_swap() {
    let rules = rules("", "");
    for health in [200, 150, 80, 50, 1] {
        assert_eq!(unit_swap(&rules, health, false), None, "unit {health}");
        assert_eq!(unit_swap(&rules, health, true), None, "unit water {health}");
        assert_eq!(aircraft_swap(&rules, health), None, "aircraft {health}");
    }
}

/// `Ext/Unit/Body.cpp:353-374`: the yellow band takes
/// `Image.ConditionYellow=`, the red band `Image.ConditionRed=` and, when that
/// is unset, the yellow one again.
#[test]
fn the_land_pair_swaps_both_damaged_bands() {
    let both = rules(
        "Image.ConditionYellow=TANKY\nImage.ConditionRed=TANKR\n",
        "Image.ConditionYellow=PLANEY\n",
    );
    assert_eq!(unit_swap(&both, 150, false), None, "green keeps the type");
    assert_eq!(unit_swap(&both, 80, false), Some("TANKY"));
    assert_eq!(unit_swap(&both, 50, false), Some("TANKR"));

    let yellow_only = rules("Image.ConditionYellow=TANKY\n", "");
    assert_eq!(unit_swap(&yellow_only, 80, false), Some("TANKY"));
    assert_eq!(
        unit_swap(&yellow_only, 50, false),
        Some("TANKY"),
        "a red band without a red image keeps the yellow one"
    );

    let red_only = rules("Image.ConditionRed=TANKR\n", "");
    assert_eq!(unit_swap(&red_only, 80, false), None);
    assert_eq!(unit_swap(&red_only, 50, false), Some("TANKR"));

    assert_eq!(aircraft_swap(&both, 150), None);
    assert_eq!(aircraft_swap(&both, 80), Some("PLANEY"));
    assert_eq!(
        aircraft_swap(&both, 50),
        Some("PLANEY"),
        "the aircraft reader has no red key here"
    );
}

/// `Ext/Unit/Body.cpp:352-374`: on water the water pair replaces the land pair
/// outright — it is an `if`/`else if`, not a fallback chain — and a type with
/// only the land pair swaps nothing there.
#[test]
fn the_water_pair_replaces_the_land_pair() {
    let land_only = rules(
        "Image.ConditionYellow=TANKY\nImage.ConditionRed=TANKR\n",
        "",
    );
    assert_eq!(
        unit_swap(&land_only, 80, true),
        None,
        "water does not fall back to the land pair"
    );
    assert_eq!(unit_swap(&land_only, 50, true), None);

    let water_only = rules(
        "WaterImage.ConditionYellow=WATERY\nWaterImage.ConditionRed=WATERR\n",
        "",
    );
    assert_eq!(unit_swap(&water_only, 150, true), None, "green keeps it");
    assert_eq!(unit_swap(&water_only, 80, true), Some("WATERY"));
    assert_eq!(unit_swap(&water_only, 50, true), Some("WATERR"));
    assert_eq!(
        unit_swap(&water_only, 80, false),
        None,
        "the land band ignores the water pair"
    );

    let water_yellow = rules(
        "Image.ConditionYellow=TANKY\nWaterImage.ConditionYellow=WATERY\n",
        "",
    );
    assert_eq!(unit_swap(&water_yellow, 80, true), Some("WATERY"));
    assert_eq!(
        unit_swap(&water_yellow, 50, true),
        Some("WATERY"),
        "a water red band without a red image keeps the water yellow one"
    );
    assert_eq!(
        unit_swap(&water_yellow, 80, false),
        Some("TANKY"),
        "the land band keeps its own key"
    );
}
