//! Idle sleep changes no result: a sleeping building and an awake twin stay
//! in the same state, hash by hash.

use super::Simulation;
use crate::map::entities::{EntityCategory, MapEntity};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::pathfinding::PathGrid;
use crate::sim::scenario_session::ScenarioGameMode;

fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[General]\n[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=GAPOWR\n\n\
         [GAPOWR]\nStrength=750\nArmor=wood\nFoundation=2x2\nSight=5\nPower=100\n\
         PoweredSpecial=yes\n",
    ))
    .expect("idle sleep test rules parse")
}

/// A skirmish with one power plant (entity 1), sleeping on or off.
fn power_plant(rules: &RuleSet, sleep: bool) -> (Simulation, PathGrid) {
    let mut sim = Simulation::with_seed(0x51EE_9000_0001);
    sim.session_mut_for_test()
        .set_game_mode(ScenarioGameMode::Skirmish);
    let grid = crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    sim.spawn_from_map(
        &[MapEntity {
            owner: "Americans".to_string(),
            type_id: "GAPOWR".to_string(),
            health: 256,
            cell_x: 10,
            cell_y: 10,
            facing: 64,
            category: EntityCategory::Structure,
            sub_cell: 0,
            veterancy: Some(0),
            high: false,
            mission: None,
            trigger_tag: None,
            recruitable_a: true,
            recruitable_b: true,
            group: -1,
            follows: None,
            structure_columns: Default::default(),
            structure_ai_sellable: false,
            structure_ai_repairable: false,
            structure_online: true,
        }],
        Some(rules),
    );
    sim.sleep_idle_objects(sleep);
    (sim, grid)
}

fn tick(sim: &mut Simulation, rules: &RuleSet, grid: &PathGrid) {
    let _ = sim.advance_tick(&[], Some(rules), Some(grid), None, 67);
}

#[test]
fn a_sleeping_building_matches_its_awake_twin_hash_by_hash() {
    let rules = rules();
    let (mut asleep, grid) = power_plant(&rules, true);
    let (mut awake, awake_grid) = power_plant(&rules, false);
    for frame in 0..260 {
        tick(&mut asleep, &rules, &grid);
        tick(&mut awake, &rules, &awake_grid);
        assert_eq!(
            asleep.state_hash(),
            awake.state_hash(),
            "the twins differ after frame {frame}"
        );
    }
    let (_, counted) = asleep.idle_sleep_counts();
    assert!(counted > 200, "the plant slept through {counted} visits");
    assert_eq!(awake.idle_sleep_counts(), (0, 0));
    // The save form reads the sleeper as settled.
    assert_eq!(asleep.idle_sleep_counts().0, 1, "the plant is asleep");
    assert_eq!(
        serde_json::to_value(&asleep.substrate.entities).unwrap(),
        serde_json::to_value(&awake.substrate.entities).unwrap(),
    );
}

#[test]
fn the_admitted_sleep_policy_keeps_an_eligible_skirmish_building_awake() {
    let rules = rules();
    let mode = ScenarioGameMode::Skirmish;
    let stock = mode.stock_rule_policy(None);
    for (policy, expected) in [(stock, 1), (stock.with_allows_idle_object_sleep(false), 0)] {
        let (mut sim, grid) = power_plant(&rules, true);
        sim.session.install_admission(mode, None, policy);
        for _ in 0..40 {
            tick(&mut sim, &rules, &grid);
        }
        assert_eq!(sim.idle_sleep_counts().0, expected);
    }
}

#[test]
fn every_turn_sleeping_would_skip_changes_only_the_owed_counters() {
    let rules = rules();
    let (mut sim, grid) = power_plant(&rules, true);
    sim.check_idle_sleep(true);
    for _ in 0..260 {
        tick(&mut sim, &rules, &grid);
    }
    assert!(sim.idle_sleep_counts().1 > 200);
}

#[test]
fn starting_repair_wakes_the_building_with_its_owed_visits() {
    use crate::sim::production::{RepairControl, toggle_repair};
    let rules = rules();
    let (mut asleep, grid) = power_plant(&rules, true);
    let (mut awake, awake_grid) = power_plant(&rules, false);
    for _ in 0..40 {
        tick(&mut asleep, &rules, &grid);
        tick(&mut awake, &rules, &awake_grid);
    }
    assert_eq!(asleep.idle_sleep_counts().0, 1, "the plant is asleep");
    assert!(toggle_repair(&mut asleep, &rules, 1, RepairControl::Start));
    assert!(toggle_repair(&mut awake, &rules, 1, RepairControl::Start));
    assert_eq!(asleep.idle_sleep_counts().0, 0, "repair wakes the building");
    let woken = asleep
        .substrate
        .entities
        .get(1)
        .unwrap()
        .mission
        .ai_counter();
    assert_eq!(
        woken,
        awake
            .substrate
            .entities
            .get(1)
            .unwrap()
            .mission
            .ai_counter()
    );
}

/// A blackout is no write to the building: the owner's power note wakes it
/// on the frame the blackout starts and on the frame it ends.
#[test]
fn a_blackout_wakes_a_sleeper_as_it_starts_and_as_it_ends() {
    let rules = rules();
    let (mut asleep, grid) = power_plant(&rules, true);
    let (mut awake, awake_grid) = power_plant(&rules, false);
    let mut went_dark = false;
    for frame in 0..150 {
        if frame == 40 {
            for sim in [&mut asleep, &mut awake] {
                let owner = sim.substrate.entities.get(1).unwrap().owner();
                let now = sim.session.binary_frame();
                sim.power_states.house_mut(owner).start_blackout(now, 30);
            }
        }
        tick(&mut asleep, &rules, &grid);
        tick(&mut awake, &rules, &awake_grid);
        went_dark |= !awake
            .substrate
            .entities
            .get(1)
            .unwrap()
            .building
            .building_last_operational
            .is_operational();
        assert_eq!(
            asleep.state_hash(),
            awake.state_hash(),
            "the twins differ after frame {frame}"
        );
    }
    assert!(went_dark, "the blackout stopped the plant");
    assert!(asleep.idle_sleep_counts().1 > 60);
}

/// A skirmish with one retail GI (Americans, a human player's) standing on
/// Guard on open ground, sleeping on or off.
fn standing_gi(rules: &RuleSet, sleep: bool) -> (Simulation, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(rules, 0x51EE_9000_0002, 32);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.session_mut_for_test()
        .set_game_mode(ScenarioGameMode::Skirmish);
    // A house left without buildings loses on the first frame.
    for (house, cell) in [("Americans", 4), ("Russians", 26)] {
        sim.spawn_object("GAPOWR", house, cell, cell, 0, rules)
            .expect("a plant spawns");
    }
    let id = sim
        .spawn_object("E1", "Americans", 16, 16, 0, rules)
        .expect("the GI spawns");
    sim.sleep_idle_objects(sleep);
    (sim, id)
}

fn frame(sim: &mut Simulation, rules: &RuleSet) {
    let commands = sim.take_due_commands();
    sim.advance_app_frame(
        &commands,
        rules,
        33,
        super::TickLane::Ordinary,
        &mut super::FrameEffects::default(),
    )
    .expect("an ordinary frame");
}

/// Run a sleeping GI and an awake twin, doing `event` to both before frame
/// `at`, and compare them hash by hash.
fn gi_twins(rules: &RuleSet, frames: u32, at: u32, event: impl Fn(&mut Simulation, u64)) {
    let (mut asleep, id) = standing_gi(rules, true);
    let (mut awake, _) = standing_gi(rules, false);
    let mut slept = 0;
    for n in 0..frames {
        if n == at {
            event(&mut asleep, id);
            event(&mut awake, id);
        }
        frame(&mut asleep, rules);
        frame(&mut awake, rules);
        slept += u32::from(asleep.substrate.entities.sleeps_until(id).is_some());
        assert_eq!(
            asleep.state_hash(),
            awake.state_hash(),
            "the twins differ after frame {n}"
        );
    }
    // Most frames before the event find the GI asleep.
    assert!(slept > at.min(frames) / 2, "the GI slept {slept} frames");
}

/// The GI's passive scan (every targeting delay, one Scenario draw) and its
/// Guard handler (one draw, the idle fidget's draws) both wake it.
#[test]
fn a_sleeping_gi_matches_its_awake_twin_hash_by_hash() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    gi_twins(&fixture.rules, 400, u32::MAX, |_, _| {});
}

/// A building placed over a sleeping GI is no write to it, and one that
/// cannot hide things (`CanHideThings=False`, a retail civilian prop) wakes
/// nothing as it is marked: the visit sees it and wakes the GI, so it
/// scatters out on the same frame.
#[test]
fn a_building_in_a_sleeping_gis_cell_wakes_it() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    // Map placement, as a scenario's own buildings: it takes no Can_Place.
    gi_twins(rules, 300, 150, |sim, gi| {
        sim.spawn_from_map(
            &[MapEntity {
                owner: "Americans".to_string(),
                type_id: "CAMSC01".to_string(),
                health: 256,
                cell_x: 16,
                cell_y: 16,
                facing: 64,
                category: EntityCategory::Structure,
                sub_cell: 0,
                veterancy: Some(0),
                high: false,
                mission: None,
                trigger_tag: None,
                recruitable_a: true,
                recruitable_b: true,
                group: -1,
                follows: None,
                structure_columns: Default::default(),
                structure_ai_sellable: false,
                structure_ai_repairable: false,
                structure_online: true,
            }],
            Some(rules),
        );
        let gi = sim.substrate.entities.get(gi).unwrap();
        let prop = sim
            .ground_building_in_cell(gi)
            .expect("the prop stands over the GI");
        assert!(
            sim.substrate
                .entities
                .get(prop)
                .unwrap()
                .seldom
                .type_facts
                .building_hidden_occupancy()
                .is_none_or(|profile| !profile.can_hide_things),
            "the prop hides nothing"
        );
    });
}

/// An empty garrisonable house (`CanBeOccupied=yes`, a retail city building)
/// sleeps like any plain building, and a GI garrisoning it wakes it: the
/// twins stay equal through the admission and the occupied turns after it.
#[test]
fn an_empty_garrisonable_house_sleeps_and_wakes_for_its_occupant() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    assert!(rules.object("CATEXS03").unwrap().can_be_occupied);
    let twin = |sleep: bool| {
        let (mut sim, gi) = standing_gi(rules, sleep);
        let house = sim
            .spawn_object("CATEXS03", "Americans", 20, 16, 0, rules)
            .expect("the house spawns");
        (sim, gi, house)
    };
    let (mut asleep, gi, house) = twin(true);
    let (mut awake, _, _) = twin(false);
    let mut slept = 0;
    for n in 0..400 {
        if n == 100 {
            for sim in [&mut asleep, &mut awake] {
                assert!(sim.apply_command(
                    "Americans",
                    &crate::sim::command::Command::CaptureBuilding {
                        engineer_id: gi,
                        target_building_id: house,
                    },
                    Some(rules),
                ));
            }
        }
        frame(&mut asleep, rules);
        frame(&mut awake, rules);
        if n < 100 {
            slept += u32::from(asleep.substrate.entities.sleeps_until(house).is_some());
        }
        assert_eq!(
            asleep.state_hash(),
            awake.state_hash(),
            "the twins differ after frame {n}"
        );
    }
    assert!(slept > 50, "the empty house slept {slept} of 100 frames");
    for sim in [&asleep, &awake] {
        let occupants = sim
            .substrate
            .entities
            .get(house)
            .unwrap()
            .passenger_role()
            .occupants();
        assert_eq!(occupants.ids(), [gi], "the GI garrisoned the house");
    }
}

/// Radiation in a sleeping GI's cell is no write to it: the visit on an
/// application frame sees it and wakes it.
#[test]
fn radiation_in_a_sleeping_gis_cell_wakes_it() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    gi_twins(rules, 200, 88, |sim, gi| {
        let now = sim.session.binary_frame();
        // The GI sleeps through the next frame radiation applies on.
        let pulse = now.next_multiple_of(rules.radiation.application_delay as u32);
        if let Some((wake_at, _)) = sim.substrate.entities.sleeps_until(gi) {
            assert!(wake_at > pulse, "the GI wakes at {wake_at}, before {pulse}");
        }
        let theater = sim.session.theater();
        sim.radiation.apply_detonation(
            crate::sim::radiation::RadDetonation {
                rx: 16,
                ry: 16,
                rad_level: 500,
                spread: 2,
                rad_type: None,
                firing_house: None,
                invoker: None,
            },
            now,
            &theater,
            rules,
            sim.resolved_terrain.as_ref(),
            &mut sim.lighting_sources,
        );
    });
}
