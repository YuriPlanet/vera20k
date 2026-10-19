//! Ported scenarios from ra2-mono's SuperweaponTargetPreflightTests.cs,
//! contributed under VERA's GPLv3 from the author's earlier ra2-mono work.

use super::*;

fn input(requirements: &TargetRequirements) -> TargetPreflightInput<'_> {
    TargetPreflightInput {
        target_cell: Some((10, 10)),
        requires_target_cell: false,
        requirements,
        contents: &[],
        target_in_bounds: true,
        terrain_is_water: Some(false),
        fire_into_shroud: true,
        shrouded: None,
        range: (-1.0, -1.0),
        requires_launch_site: false,
        launch_sites: &[],
    }
}

fn site(center: (i32, i32), eligible: bool) -> LaunchSite {
    LaunchSite {
        center,
        eligible,
        range_minimum_override: None,
        range_maximum_override: None,
    }
}

#[test]
fn mono_requires_target_uses_land_water_semantics() {
    for (requirement, water, accepted) in [
        ("land", false, true),
        ("land", true, false),
        ("water", true, true),
        ("water", false, false),
        ("all", true, true),
        ("all", false, true),
    ] {
        let requirements = TargetRequirements::parse(requirement, "");
        let mut input = input(&requirements);
        input.terrain_is_water = Some(water);
        assert_eq!(
            evaluate_target(&input).is_ok(),
            accepted,
            "{requirement}/{water}"
        );
        input.terrain_is_water = None;
        assert!(
            evaluate_target(&input).is_ok(),
            "unknown terrain is not a mismatch"
        );
    }
}

#[test]
fn mono_unknown_token_fails_closed_with_diagnostic() {
    for requirements in [
        TargetRequirements::parse("future-content-mask", ""),
        TargetRequirements::parse("", "future-content-mask"),
    ] {
        assert_eq!(
            evaluate_target(&input(&requirements)),
            Err(TargetReject::UnsupportedTargetRequirement(
                "future-content-mask".to_owned()
            ))
        );
    }
}

#[test]
fn mono_mask_parser_preserves_composable_semantics() {
    for (raw, bits) in [
        ("empty", 4),
        ("unit", 16),
        ("units,buildings", 48),
        ("land water all-contents", 63),
        ("all-cells+infantry", 11),
        ("none", 0),
        ("<none>", 0),
    ] {
        assert_eq!(TargetMask::parse(raw), Ok(TargetMask(bits)), "{raw}");
    }
    assert_eq!(HouseMask::parse("owner,allies"), HouseMask::parse("team"));
    assert_eq!(
        HouseMask::parse("enemies|allies"),
        HouseMask::parse("others")
    );
}

#[test]
fn mono_content_mask_distinguishes_empty_aircraft_and_building() {
    let empty = TargetRequirements::parse("empty", "");
    assert!(evaluate_target(&input(&empty)).is_ok());
    let units = TargetRequirements::parse("units", "enemies");
    let mut input = input(&units);
    assert_eq!(
        evaluate_target(&input),
        Err(TargetReject::TargetContentMismatch)
    );
    let aircraft = [TargetContent {
        kind: TargetMask::UNIT,
        relation: HouseMask::ENEMIES,
    }];
    input.contents = &aircraft;
    assert!(evaluate_target(&input).is_ok());
    let other = TargetRequirements::parse("infantry,buildings", "");
    input.requirements = &other;
    assert_eq!(
        evaluate_target(&input),
        Err(TargetReject::TargetContentMismatch)
    );
}

#[test]
fn mono_target_kind_and_house_must_match_same_candidate() {
    let requirements = TargetRequirements::parse("units", "enemies");
    let mut input = input(&requirements);
    input.contents = &[
        TargetContent {
            kind: TargetMask::UNIT,
            relation: HouseMask::OWNER,
        },
        TargetContent {
            kind: TargetMask::BUILDING,
            relation: HouseMask::ENEMIES,
        },
    ];
    assert_eq!(
        evaluate_target(&input),
        Err(TargetReject::TargetHouseMismatch)
    );
}

#[test]
fn mono_house_requirement_alone_does_not_reject_empty_cell() {
    let requirements = TargetRequirements::parse("", "enemies");
    assert!(evaluate_target(&input(&requirements)).is_ok());
}

#[test]
fn mono_fire_into_shroud_rejects_only_never_explored_shroud() {
    let requirements = TargetRequirements::parse("", "");
    let mut input = input(&requirements);
    input.fire_into_shroud = false;
    for (shrouded, expected) in [(Some(true), false), (Some(false), true), (None, true)] {
        input.shrouded = shrouded;
        assert_eq!(evaluate_target(&input).is_ok(), expected);
    }
}

#[test]
fn mono_range_boundary_is_inclusive_for_any_eligible_launcher() {
    let requirements = TargetRequirements::parse("", "");
    let mut input = input(&requirements);
    input.range = (5.0, 10.0);
    let sites = [
        site((0, 0), false),
        site((3, 4), true),
        site((20, 20), true),
    ];
    input.launch_sites = &sites;
    for target in [(0, 0), (9, 12)] {
        input.target_cell = Some(target);
        assert!(evaluate_target(&input).is_ok());
    }
}

#[test]
fn mono_range_rejects_when_only_inactive_launcher_is_in_range() {
    let requirements = TargetRequirements::parse("", "");
    let mut input = input(&requirements);
    input.target_cell = Some((5, 0));
    input.range = (-1.0, 5.0);
    let sites = [site((0, 0), false), site((20, 20), true)];
    input.launch_sites = &sites;
    assert_eq!(
        evaluate_target(&input),
        Err(TargetReject::NoEligibleLaunchSite)
    );
}

#[test]
fn mono_range_uses_each_launchers_effective_range() {
    let requirements = TargetRequirements::parse("", "");
    let mut input = input(&requirements);
    input.target_cell = Some((18, 0));
    let sites = [
        LaunchSite {
            range_maximum_override: Some(4.0),
            ..site((0, 0), true)
        },
        LaunchSite {
            range_minimum_override: Some(2.0),
            range_maximum_override: Some(8.0),
            ..site((10, 0), true)
        },
    ];
    input.launch_sites = &sites;
    assert!(evaluate_target(&input).is_ok());
}

#[test]
fn mono_range_or_required_site_without_launcher_fails_closed() {
    let requirements = TargetRequirements::parse("", "");
    let mut input = input(&requirements);
    input.range = (-1.0, 8.0);
    assert_eq!(
        evaluate_target(&input),
        Err(TargetReject::NoEligibleLaunchSite)
    );
    input.range = (-1.0, -1.0);
    input.requires_launch_site = true;
    assert_eq!(
        evaluate_target(&input),
        Err(TargetReject::NoEligibleLaunchSite)
    );
    let sites = [site((10, 10), true)];
    input.launch_sites = &sites;
    assert!(evaluate_target(&input).is_ok());
}

#[test]
fn mono_invalid_range_and_missing_target_are_rejected() {
    let requirements = TargetRequirements::parse("", "");
    let mut input = input(&requirements);
    for range in [(8.0, 2.0), (f64::INFINITY, 5.0), (f64::NAN, -1.0)] {
        input.range = range;
        assert_eq!(
            evaluate_target(&input),
            Err(TargetReject::InvalidRangeConfiguration)
        );
    }
    input.range = (-1.0, -1.0);
    input.target_cell = None;
    assert!(evaluate_target(&input).is_ok());
    input.requires_target_cell = true;
    assert_eq!(evaluate_target(&input), Err(TargetReject::TargetRequired));
}
