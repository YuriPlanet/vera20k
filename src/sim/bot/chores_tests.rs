//! Bot chores: repair, idle miners and crate scouts.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::Simulation;

const RULES: &str = "[VehicleTypes]\n0=HARV\n[BuildingTypes]\n0=PLANT\n\
    [HARV]\nStrength=600\nSpeed=4\nSpeedType=Track\nSight=3\nHarvester=yes\n\
    Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
    [PLANT]\nStrength=750\nPower=100\nSight=3\nCost=800\n";

fn arena() -> (Simulation, RuleSet, crate::sim::intern::InternedId) {
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(RULES),
        &IniFile::from_str("[PLANT]\nFoundation=2x2\n"),
    )
    .unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xB07);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.fog.set_width_for_test(32);
    sim.fog.set_height_for_test(32);
    let me = sim.interner().get("Americans").unwrap();
    sim.houses
        .get_mut(&me)
        .unwrap()
        .economy_mut()
        .set_credits_for_test(1000);
    (sim, rules, me)
}

/// The orders of one think; the map is all explored (`FullMapVision`), which
/// these chores read only for ore.
fn orders(sim: &Simulation, rules: &RuleSet, me: crate::sim::intern::InternedId) -> Vec<Command> {
    let overlays = crate::sim::tiberium::test_support::overlay_registry_with_land();
    let view = BotView::new(
        sim,
        rules,
        Some(overlays),
        me,
        &[crate::sim::bot::Cheat::FullMapVision],
    );
    let mut out = Vec::new();
    repair_buildings(&view, &mut out);
    tend_miners(&view, 15, &mut Vec::new(), &mut out);
    out
}

/// A damaged building whose repair stopped (it healed once, or the money ran
/// out) is ordered to repair again; one already repairing is left alone.
/// (Both bots used to remember a building once ordered and never repaired it
/// a second time.)
#[test]
fn a_damaged_building_not_repairing_is_ordered_to_repair_each_time() {
    let (mut sim, rules, me) = arena();
    let plant = sim
        .spawn_object_at_height("PLANT", "Americans", 5, 5, 0, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(plant)
        .unwrap()
        .health
        .set(300);
    let repair = Command::ToggleRepair { entity_id: plant };
    assert_eq!(orders(&sim, &rules, me), vec![repair.clone()]);
    sim.substrate
        .entities
        .get_mut(plant)
        .unwrap()
        .building
        .repairing
        .set_for_test(true);
    assert!(orders(&sim, &rules, me).is_empty(), "already repairing");
    // The repair stopped; it is hit again.
    sim.substrate
        .entities
        .get_mut(plant)
        .unwrap()
        .building
        .repairing
        .set_for_test(false);
    assert_eq!(orders(&sim, &rules, me), vec![repair]);
}

/// A miner parked on Guard (its ore search found nothing) is sent to the
/// explored ore; one already harvesting is not.
#[test]
fn a_parked_miner_is_sent_to_explored_ore() {
    let (mut sim, rules, me) = arena();
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
    crate::sim::tiberium::test_support::place_tiberium_on_map(
        &mut sim,
        (9, 7),
        crate::sim::miner::ResourceType::Ore,
        5,
    );
    let harv = sim
        .spawn_object_at_height("HARV", "Americans", 7, 7, 0, 0, &rules)
        .unwrap();
    let _ = sim
        .substrate
        .mission_assign_exact(harv, MissionId::from_known(MissionType::Guard), 0);
    let sent = orders(&sim, &rules, me);
    assert!(
        matches!(
            sent.as_slice(),
            [Command::MegaMission {
                entity_id,
                mission: MissionType::Harvest,
                destination: Some(crate::sim::combat::TargetKind::Cell(9, 7)),
                ..
            }] if *entity_id == harv
        ),
        "{sent:?}"
    );
    let _ =
        sim.substrate
            .mission_assign_exact(harv, MissionId::from_known(MissionType::Harvest), 0);
    assert!(orders(&sim, &rules, me).is_empty(), "harvesting already");
}

/// A crate scout (a dog of the bot's, crates on) that sees an armed enemy
/// within `SCOUT_FLEE` turns back toward home; with the enemy gone from
/// near it, it explores (a move elsewhere, never toward that enemy).
#[test]
fn a_crate_scout_turns_home_from_a_near_enemy() {
    let rules_ini = format!(
        "{RULES}[InfantryTypes]\n0=DOG\n1=GI\n[DOG]\nStrength=100\nSpeed=6\nSight=4\n\
         [GI]\nStrength=100\nSpeed=4\nSight=4\nPrimary=M60\n\
         [M60]\nDamage=15\nROF=20\nRange=5\nWarhead=SA\n[SA]\n"
    );
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(&rules_ini),
        &IniFile::from_str("[PLANT]\nFoundation=2x2\n"),
    )
    .unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xB07);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.fog.set_width_for_test(32);
    sim.fog.set_height_for_test(32);
    sim.session_mut_for_test()
        .game_options_mut_for_test()
        .crates = true;
    let me = sim.interner().get("Americans").unwrap();
    let dog = sim
        .spawn_object_at_height("DOG", "Americans", 16, 16, 0, 0, &rules)
        .unwrap();
    let gi = sim
        .spawn_object_at_height("GI", "Russians", 22, 16, 0, 0, &rules)
        .unwrap();
    let mut scouts = CrateScouts::default();
    let think = |sim: &Simulation, scouts: &mut CrateScouts| {
        let view = BotView::new(
            sim,
            &rules,
            None,
            me,
            &[crate::sim::bot::Cheat::FullMapVision],
        );
        let mut out = Vec::new();
        scouts.run(&view, (4, 16), None, &mut out);
        out
    };
    let out = think(&sim, &mut scouts);
    assert_eq!(scouts.ids(), &[dog]);
    assert!(
        matches!(out.as_slice(), [Command::Move { entity_id, target_rx, .. }]
            if *entity_id == dog && *target_rx < 16),
        "back toward home: {out:?}"
    );
    // The enemy gone: off exploring, never toward where it stood.
    sim.substrate.entities.remove(gi);
    let view = BotView::new(
        &sim,
        &rules,
        None,
        me,
        &[crate::sim::bot::Cheat::FullMapVision],
    );
    let mut out = Vec::new();
    scouts.goals.clear();
    scouts.run(&view, (4, 16), None, &mut out);
    assert!(
        matches!(out.as_slice(), [Command::Move { entity_id, .. }] if *entity_id == dog),
        "{out:?}"
    );
}

/// Rules with an armed enemy GI (reach 5) and a dog for the bot.
fn armed_arena() -> (Simulation, RuleSet, crate::sim::intern::InternedId) {
    let rules_ini = format!(
        "{}[InfantryTypes]\n0=DOG\n1=GI\n[DOG]\nStrength=100\nSpeed=6\nSight=4\n\
         [GI]\nStrength=100\nSpeed=4\nSight=4\nPrimary=M60\n\
         [M60]\nDamage=15\nROF=20\nRange=5\nWarhead=SA\n[SA]\n\
         [DEPOT]\nStrength=800\nUnitRepair=yes\n",
        RULES.replace("0=PLANT\n", "0=PLANT\n1=DEPOT\n")
    );
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(&rules_ini),
        &IniFile::from_str("[PLANT]\nFoundation=2x2\n[DEPOT]\nFoundation=2x2\n"),
    )
    .unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xB07);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.fog.set_width_for_test(32);
    sim.fog.set_height_for_test(32);
    let me = sim.interner().get("Americans").unwrap();
    (sim, rules, me)
}

/// A plain move whose straight way crosses a seen enemy's reach is sent to
/// a waypoint first (a crate's tank once drove home through an enemy base);
/// with the enemy gone it goes straight.
#[test]
fn a_move_through_enemy_reach_takes_a_waypoint() {
    let (mut sim, rules, me) = armed_arena();
    let dog = sim
        .spawn_object_at_height("DOG", "Americans", 6, 16, 0, 0, &rules)
        .unwrap();
    let gi = sim
        .spawn_object_at_height("GI", "Russians", 16, 16, 0, 0, &rules)
        .unwrap();
    let straight = Command::Move {
        entity_id: dog,
        target_rx: 26,
        target_ry: 16,
    };
    let run = |sim: &Simulation| {
        let view = BotView::new(
            sim,
            &rules,
            None,
            me,
            &[crate::sim::bot::Cheat::FullMapVision],
        );
        let mut out = vec![straight.clone()];
        detour(&view, &mut out);
        out
    };
    let out = run(&sim);
    let [
        Command::Move {
            target_rx,
            target_ry,
            ..
        },
    ] = out.as_slice()
    else {
        panic!("{out:?}")
    };
    let way = (*target_rx, *target_ry);
    assert_ne!(way, (26, 16));
    // Both legs keep out of the GI's reach (5 + 2 cells).
    let view = BotView::new(
        &sim,
        &rules,
        None,
        me,
        &[crate::sim::bot::Cheat::FullMapVision],
    );
    let danger = Danger::seen(&view, 2);
    assert!(!danger.on_way((6, 16), way) && !danger.on_way(way, (26, 16)));
    sim.substrate.entities.remove(gi);
    assert_eq!(run(&sim), vec![straight]);
}

/// A vehicle with a Terror Drone inside drives into the Service Depot,
/// whose repair forces the drone out.
#[test]
fn an_infested_vehicle_drives_to_the_depot() {
    let (mut sim, rules, me) = armed_arena();
    let depot = sim
        .spawn_object_at_height("DEPOT", "Americans", 4, 4, 0, 0, &rules)
        .unwrap();
    let harv = sim
        .spawn_object_at_height("HARV", "Americans", 12, 12, 0, 0, &rules)
        .unwrap();
    let think = |sim: &Simulation| {
        let view = BotView::new(
            sim,
            &rules,
            None,
            me,
            &[crate::sim::bot::Cheat::FullMapVision],
        );
        let mut out = Vec::new();
        fight_drones(&view, 15, &[], &mut out);
        out
    };
    assert!(think(&sim).is_empty());
    sim.substrate
        .entities
        .get_mut(harv)
        .unwrap()
        .parasite
        .set_eater_for_test(Some(999));
    assert_eq!(
        think(&sim),
        vec![Command::MoveToObject {
            entity_id: harv,
            target_id: depot
        }]
    );
}

/// A garrisoned building is dangerous though its own type has no weapon (a
/// scout dog once ran into one's range and died); empty, it is not.
#[test]
fn a_garrisoned_building_is_in_the_danger_map() {
    let (mut sim, rules, me) = armed_arena();
    let plant = sim
        .spawn_object_at_height("PLANT", "Russians", 16, 16, 0, 0, &rules)
        .unwrap();
    let danger = |sim: &Simulation| {
        let view = BotView::new(
            sim,
            &rules,
            None,
            me,
            &[crate::sim::bot::Cheat::FullMapVision],
        );
        Danger::seen(&view, 0).at((16, 22))
    };
    assert!(!danger(&sim));
    sim.substrate
        .entities
        .get_mut(plant)
        .unwrap()
        .passenger_role_mut_for_test()
        .set_occupants_for_test(vec![4242], 0);
    assert!(danger(&sim));
}

/// Playtest, 2026-11-14 ("stay away from enemy fire unless you're certain you
/// can kill them"): a lone GI in reach of three enemy GIs walks back toward
/// its yard instead of its order; with five of its own beside it, it keeps
/// the order.
#[test]
fn an_outgunned_unit_steps_back_from_enemy_reach() {
    let rules_ini = format!(
        "{}[InfantryTypes]\n0=GI\n[GI]\nStrength=100\nSpeed=4\nSight=4\nCost=200\nPrimary=M60\n\
         [M60]\nDamage=15\nROF=20\nRange=5\nWarhead=SA\n[SA]\n\
         [YARD]\nStrength=1000\nConstructionYard=yes\n",
        RULES.replace("0=PLANT\n", "0=PLANT\n1=YARD\n")
    );
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(&rules_ini),
        &IniFile::from_str("[PLANT]\nFoundation=2x2\n[YARD]\nFoundation=2x2\n"),
    )
    .unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xB07);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.fog.set_width_for_test(32);
    sim.fog.set_height_for_test(32);
    let me = sim.interner().get("Americans").unwrap();
    sim.spawn_object_at_height("YARD", "Americans", 2, 16, 0, 0, &rules)
        .unwrap();
    let gi = sim
        .spawn_object_at_height("GI", "Americans", 20, 16, 0, 0, &rules)
        .unwrap();
    for y in [15, 16, 17] {
        sim.spawn_object_at_height("GI", "Russians", 24, y, 0, 0, &rules)
            .unwrap();
    }
    let order = Command::Move {
        entity_id: gi,
        target_rx: 28,
        target_ry: 16,
    };
    let run = |sim: &Simulation| {
        let view = BotView::new(
            sim,
            &rules,
            None,
            me,
            &[crate::sim::bot::Cheat::FullMapVision],
        );
        let mut out = vec![order.clone()];
        hold_back(&view, &[], &mut out);
        out
    };
    let out = run(&sim);
    assert!(
        matches!(out.as_slice(), [Command::Move { entity_id, target_rx, .. }]
            if *entity_id == gi && *target_rx < 20),
        "{out:?}"
    );
    for y in [14, 15, 17, 18, 19] {
        sim.spawn_object_at_height("GI", "Americans", 20, y, 0, 0, &rules)
            .unwrap();
    }
    let out = run(&sim);
    assert!(out.contains(&order), "{out:?}");
}

/// A tech building (`TechLevel=-1`) an enemy holds is retaken by an engineer
/// (playtest, 2026-11-14: "keep capturing neutral airports (in fact, try to
/// capture all tech buildings)"); before, only neutral ones within 40 cells
/// of home and derricks were.
#[test]
fn an_engineer_retakes_an_enemy_held_tech_building() {
    let rules_ini = format!(
        "{}[InfantryTypes]\n0=ENGI\n[ENGI]\nStrength=75\nSpeed=4\nSight=4\nEngineer=yes\n\
         [AIRP]\nStrength=800\nTechLevel=-1\nCapturable=yes\nNeedsEngineer=yes\n",
        RULES.replace("0=PLANT\n", "0=PLANT\n1=AIRP\n")
    );
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(&rules_ini),
        &IniFile::from_str("[PLANT]\nFoundation=2x2\n[AIRP]\nFoundation=2x2\n"),
    )
    .unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xB07);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.fog.set_width_for_test(32);
    sim.fog.set_height_for_test(32);
    let me = sim.interner().get("Americans").unwrap();
    let engineer = sim
        .spawn_object_at_height("ENGI", "Americans", 4, 16, 0, 0, &rules)
        .unwrap();
    let airport = sim
        .spawn_object_at_height("AIRP", "Russians", 20, 16, 0, 0, &rules)
        .unwrap();
    let view = BotView::new(
        &sim,
        &rules,
        None,
        me,
        &[crate::sim::bot::Cheat::FullMapVision],
    );
    let mut out = Vec::new();
    TechBuildings::default().run(&view, (4, 16), 12, &[], &[], &mut out);
    assert!(
        out.contains(&Command::CaptureBuilding {
            engineer_id: engineer,
            target_building_id: airport,
        }),
        "{out:?}"
    );
}

/// Rules with a long-range tank (reach 10) for the bot and the enemy plant.
fn tank_arena() -> (Simulation, RuleSet, crate::sim::intern::InternedId) {
    let rules_ini = format!(
        "{}[TANK]\nStrength=400\nSpeed=5\nSight=8\nPrimary=BEAM\n\
         Locomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\n\
         [BEAM]\nDamage=50\nROF=60\nRange=10\nWarhead=AP\n[AP]\n",
        RULES.replace("0=HARV\n", "0=HARV\n1=TANK\n")
    );
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(&rules_ini),
        &IniFile::from_str("[PLANT]\nFoundation=2x2\n"),
    )
    .unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xB07);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.fog.set_width_for_test(32);
    sim.fog.set_height_for_test(32);
    let me = sim.interner().get("Americans").unwrap();
    (sim, rules, me)
}

/// Playtest, 2026-11-22: a long-range unit told to attack a building drives
/// closer than its range needs. The attack becomes a move to the farthest
/// cell on the way from which the weapon reaches; there the attack goes in.
/// A unit already in reach keeps its attack.
#[test]
fn a_long_range_attack_on_a_building_stops_at_its_longest_reach() {
    let (mut sim, rules, me) = tank_arena();
    let tank = sim
        .spawn_object_at_height("TANK", "Americans", 2, 16, 0, 0, &rules)
        .unwrap();
    let plant = sim
        .spawn_object_at_height("PLANT", "Russians", 26, 16, 0, 0, &rules)
        .unwrap();
    let attack = Command::Attack {
        attacker_id: tank,
        target_id: plant,
    };
    let view = BotView::new(
        &sim,
        &rules,
        None,
        me,
        &[crate::sim::bot::Cheat::FullMapVision],
    );
    let mut keep = RangeKeep::default();
    let mut out = vec![attack.clone()];
    keep.rewrite(&view, &mut out);
    let [
        Command::Move {
            target_rx,
            target_ry,
            ..
        },
    ] = out.as_slice()
    else {
        panic!("{out:?}")
    };
    let stand = (*target_rx, *target_ry);
    let (t, p) = (view.entity(tank).unwrap(), view.entity(plant).unwrap());
    // On the line from the plant's middle (27, 17) toward the tank.
    assert!((16..=17).contains(&stand.1) && stand.0 > 10, "{stand:?}");
    assert!(view.in_range_from(t, p, stand), "{stand:?}");
    assert!(
        !view.in_range_from(t, p, (stand.0 - 1, stand.1)),
        "{stand:?}"
    );
    // Not yet there: nothing; at the stand cell: the attack.
    let mut out = Vec::new();
    keep.run(&view, &mut out);
    assert!(out.is_empty(), "{out:?}");
    // A tank standing there.
    let tank = sim
        .spawn_object_at_height("TANK", "Americans", stand.0, stand.1, 0, 0, &rules)
        .unwrap();
    let attack = Command::Attack {
        attacker_id: tank,
        target_id: plant,
    };
    keep.walks = vec![(tank, plant, 0)];
    let view = BotView::new(
        &sim,
        &rules,
        None,
        me,
        &[crate::sim::bot::Cheat::FullMapVision],
    );
    keep.run(&view, &mut out);
    assert_eq!(out, vec![attack.clone()]);
    // In reach already: the attack stays as it is.
    let mut out = vec![attack.clone()];
    keep.rewrite(&view, &mut out);
    assert_eq!(out, vec![attack]);
}
