//! Ported scenarios from ra2-mono's SuperweaponTargetPreflightTests.cs,
//! contributed under VERA's GPLv3 from the author's earlier ra2-mono work.

use super::*;

fn read_passes(type_name: &str, passes: &[&str]) -> AresSuperWeaponTargetingState {
    let mut state = AresSuperWeaponTargetingState::default();
    for pass in passes {
        let ini =
            crate::rules::ini_parser::IniFile::from_str(&format!("[SW]\nType={type_name}\n{pass}"));
        state.read_pass(ini.section("SW").unwrap(), handler_initial_mode(type_name));
    }
    state
}

fn house(has_enemy: bool, offensive_cell: (u16, u16)) -> HouseTargetingObservations {
    HouseTargetingObservations {
        has_enemy,
        offensive_cell,
    }
}

#[test]
fn handler_mode_row_supplies_constraints_until_a_pass_reads_some() {
    // MultiMissile's Initialize (100733BC) selects mode 1, whose row needs an
    // enemy; an unknown token keeps it and a comma-only list stores none.
    let nuke = read_passes("MultiMissile", &["", "SW.AITargeting.Constraints=bogus\n"]);
    assert!(!nuke.constraints().allows_manual_fire(house(false, (0, 0))));
    assert!(nuke.constraints().allows_manual_fire(house(true, (0, 0))));
    let cleared = read_passes("MultiMissile", &["SW.AITargeting.Constraints=,\n"]);
    assert!(
        cleared
            .constraints()
            .allows_manual_fire(house(false, (0, 0)))
    );
    // ChronoSphere's Initialize stores nothing, so mode 0 has no constraints.
    let chrono = read_passes("ChronoSphere", &[""]);
    assert!(
        chrono
            .constraints()
            .allows_manual_fire(house(false, (0, 0)))
    );
}

#[test]
fn manual_fire_reads_the_offensive_cell_and_skips_ai_only_constraints() {
    let state = read_passes(
        "GenericWarhead",
        &["SW.AITargeting.Constraints=offensive_cell_set,lowpower,attacked\n"],
    );
    assert!(!state.constraints().allows_manual_fire(house(true, (0, 0))));
    assert!(state.constraints().allows_manual_fire(house(false, (4, 5))));
}

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

fn requirements(target: TargetMask, house: HouseMask) -> TargetRequirements {
    TargetRequirements { target, house }
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
        (TargetMask::LAND, false, true),
        (TargetMask::LAND, true, false),
        (TargetMask::WATER, true, true),
        (TargetMask::WATER, false, false),
        (TargetMask::ALL, true, true),
        (TargetMask::ALL, false, true),
    ] {
        let requirements = requirements(requirement, HouseMask::NONE);
        let mut input = input(&requirements);
        input.terrain_is_water = Some(water);
        assert_eq!(
            evaluate_target(&input).is_ok(),
            accepted,
            "{requirement:?}/{water}"
        );
        input.terrain_is_water = None;
        assert!(
            evaluate_target(&input).is_ok(),
            "unknown terrain is not a mismatch"
        );
    }
}

#[test]
fn mono_content_mask_distinguishes_empty_aircraft_and_building() {
    let empty = requirements(TargetMask::EMPTY, HouseMask::NONE);
    assert!(evaluate_target(&input(&empty)).is_ok());
    let units = requirements(TargetMask::UNIT, HouseMask::ENEMIES);
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
    let other = requirements(TargetMask(0x28), HouseMask::NONE);
    input.requirements = &other;
    assert_eq!(
        evaluate_target(&input),
        Err(TargetReject::TargetContentMismatch)
    );
}

#[test]
fn mono_target_kind_and_house_must_match_same_candidate() {
    let requirements = requirements(TargetMask::UNIT, HouseMask::ENEMIES);
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
    let requirements = requirements(TargetMask::NONE, HouseMask::ENEMIES);
    assert!(evaluate_target(&input(&requirements)).is_ok());
}

#[test]
fn mono_fire_into_shroud_rejects_only_never_explored_shroud() {
    let requirements = requirements(TargetMask::NONE, HouseMask::NONE);
    let mut input = input(&requirements);
    input.fire_into_shroud = false;
    for (shrouded, expected) in [(Some(true), false), (Some(false), true), (None, true)] {
        input.shrouded = shrouded;
        assert_eq!(evaluate_target(&input).is_ok(), expected);
    }
}

#[test]
fn mono_range_boundary_is_inclusive_for_any_eligible_launcher() {
    let requirements = requirements(TargetMask::NONE, HouseMask::NONE);
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
    let requirements = requirements(TargetMask::NONE, HouseMask::NONE);
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
    let requirements = requirements(TargetMask::NONE, HouseMask::NONE);
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
    let requirements = requirements(TargetMask::NONE, HouseMask::NONE);
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
    let requirements = requirements(TargetMask::NONE, HouseMask::NONE);
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
