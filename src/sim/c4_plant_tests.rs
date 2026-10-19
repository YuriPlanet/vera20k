//! Rust regression tests for the C4 plant: Infantry Mission_Attack's C4 arm
//! (`0x0051F3E3..0x0051F455`), Mission_Hunt's (`0x004D53F9..0x004D5467`),
//! the Sabotage dispatch to Mission_Capture and `InfantryClass::
//! Per_Cell_Process`'s C4 arm (`0x0051A4BF..0x0051A664`). Expected values
//! come from instruction reading. On a Walk arrival the arm draws GetROF's
//! mid-burst `RandomRanged(3, 5)` only; a second planter, whose null
//! destination stops the Walk, also draws its Scatter's `RandomRanged(0, 4)`.

use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::{FrameEffects, ObjectAiCtx, Simulation};

/// Retail's demolition flags and weapons: Tanya's `Secondary=Sapper`, a
/// `CanC4=no` building, and `[CombatDamage] C4Delay=.03`.
const RULES: &str = "\
[InfantryTypes]
0=TANY
1=GHOST
2=E1
[BuildingTypes]
0=GAPILE
1=CAMISC01
[Attack]
Rate=.016
[Hunt]
Rate=.016
[Sabotage]
Rate=.016
[CombatDamage]
C4Warhead=SA
C4Delay=.03
[TANY]
Strength=125
Speed=4
C4=yes
Primary=Pistols
Secondary=Sapper
Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}
[GHOST]
Strength=125
Speed=4
C4=yes
Primary=Pistols
Secondary=Sapper
Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}
[E1]
Strength=125
Speed=4
Primary=Pistols
Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}
[GAPILE]
Strength=500
Armor=wood
[CAMISC01]
Strength=600
CanC4=no
[Pistols]
Damage=10
ROF=20
Range=5
Warhead=SA
[Sapper]
Damage=2500
ROF=100
Range=1.5
Warhead=SA
[SA]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
";

const ART: &str = "\
[GAPILE]
Foundation=2x2
[CAMISC01]
Foundation=1x1
";

const SABOTAGE: MissionId = MissionId::from_known(MissionType::Sabotage);

fn rules() -> RuleSet {
    let art = IniFile::from_str(ART);
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(RULES),
        &art,
    )
    .expect("C4 plant rules");
    rules.install_art_data(ArtRegistry::from_ini(&art));
    rules
}

/// The flat arena with the computer `Russians` and the human `Americans`,
/// on a frame that is no multiple of 16 (`TechnoClass::AI_Update` lets go
/// of an unreachable target on those before the mission runs).
fn arena(rules: &RuleSet) -> (Simulation, PathGrid) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x51A4BF);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.input_delay_ticks = 0;
    sim.session.binary_frame = 1;
    let grid = sim
        .path_grid_snapshot()
        .map(|grid| (*grid).clone())
        .unwrap();
    (sim, grid)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, rx: u16, ry: u16) -> u64 {
    sim.spawn_object_at_height(kind, owner, rx, ry, 0, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

fn step(sim: &mut Simulation, rules: &RuleSet, grid: &PathGrid) {
    let commands = sim.take_due_commands();
    sim.advance_tick(&commands, Some(rules), Some(grid), None, 33);
}

fn order_plant(sim: &mut Simulation, owner: &str, attacker_id: u64, target_building_id: u64) {
    let owner = sim.interner.intern(owner);
    let tick = sim.session.tick + 1;
    sim.queue_command(CommandEnvelope::new(
        owner,
        tick,
        Command::PlantC4 {
            attacker_id,
            target_building_id,
        },
    ));
}

/// Steps until `building` holds a charge, for at most 300 frames. Answers
/// the frame the plant ran on.
fn step_until_planted(
    sim: &mut Simulation,
    rules: &RuleSet,
    grid: &PathGrid,
    building: u64,
) -> u32 {
    for _ in 0..300 {
        let frame = sim.session.binary_frame;
        step(sim, rules, grid);
        if sim
            .substrate
            .entities
            .get(building)
            .is_some_and(|building| building.pending_c4_detonation.pending().is_some())
        {
            return frame;
        }
    }
    panic!("no charge was planted");
}

/// Stands `id` on the building's anchor cell on Sabotage, heading for the
/// building, as a Walk arrival leaves it for Per_Cell_Process: the Walk
/// still holds that cell as its destination (`0x0075BE3C` runs before
/// `0x0075BEF0..0x0075BF38` lets it go).
fn stand_in(sim: &mut Simulation, id: u64, building: u64) {
    let anchor = sim
        .substrate
        .entities
        .get(building)
        .map(|building| (building.position.rx(), building.position.ry()))
        .unwrap();
    let now = sim.session.binary_frame;
    sim.mission_assign_exact(id, SABOTAGE, now).unwrap();
    let planter = sim.substrate.entities.get_mut(id).unwrap();
    {
        let (rx, ry) = anchor;
        planter.position.set_rx_for_test(rx);
        planter.position.set_ry_for_test(ry);
    }
    planter
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::object(building)));
    planter
        .locomotor
        .as_mut()
        .unwrap()
        .set_walk_destination(Some(DriveCoord::cell(anchor.0, anchor.1, 0)));
}

/// The arm's draws, in its order: GetROF's `RandomRanged(3, 5)`, then, when
/// the Scatter goes through, its `RandomRanged(0, 4)`. Answers the rearm and
/// the stream after them.
fn expected_draws(sim: &Simulation, scatters: bool) -> (i32, crate::sim::rng::SimRngLogicalState) {
    let mut rng = sim.scenario_rng.clone();
    let rof = rng.next_range_u32_inclusive(3, 5) as i32;
    if scatters {
        rng.next_range_u32_inclusive(0, 4);
    }
    (rof, rng.logical_state())
}

/// Infantry Mission_Attack's C4 arm takes a `C4=` infantryman's building
/// Target when its type is `CanC4=`, for either house: it heads for the
/// building, queues Sabotage and returns 1. A building it may not plant on,
/// or an infantryman without C4, falls through to the Foot body.
#[test]
fn mission_attacks_c4_arm_sends_a_demolition_man_to_sabotage() {
    let rules = rules();
    for (owner, enemy, kind, building, takes) in [
        ("Russians", "Americans", "TANY", "GAPILE", true),
        ("Americans", "Russians", "GHOST", "GAPILE", true),
        ("Russians", "Americans", "TANY", "CAMISC01", false),
        ("Russians", "Americans", "E1", "GAPILE", false),
    ] {
        let (mut sim, _) = arena(&rules);
        let target = spawn(&mut sim, &rules, building, enemy, 16, 10);
        let actor = spawn(&mut sim, &rules, kind, owner, 10, 10);
        sim.substrate
            .entities
            .get_mut(actor)
            .unwrap()
            .attack_target
            .set_for_test(Some(TargetKind::Entity(target)));
        let now = sim.session.binary_frame;
        sim.mission_assign_exact(actor, MissionId::from_known(MissionType::Attack), now)
            .unwrap();

        sim.object_ai_visit_one(actor, Some(&rules), ObjectAiCtx::default());

        let entity = sim.substrate.entities.get(actor).unwrap();
        let case = format!("{owner} {kind} on {building}");
        if takes {
            assert_eq!(
                entity.navigation.nav_com(),
                Some(NavTargetRef::object(target)),
                "{case}"
            );
            assert_eq!(entity.mission.queued(), SABOTAGE, "{case}");
            assert_eq!(entity.mission.dispatch_timer().delay(), 1, "{case}");
        } else {
            assert_ne!(entity.mission.queued(), SABOTAGE, "{case}");
        }
    }
}

/// Mission_Hunt's C4 arm: a hunting `C4=` infantryman whose scan holds a
/// building heads for it on Sabotage.
#[test]
fn a_hunting_demolition_man_whose_scan_finds_a_building_sabotages_it() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let target = spawn(&mut sim, &rules, "GAPILE", "Americans", 26, 26);
    let actor = spawn(&mut sim, &rules, "TANY", "Russians", 4, 4);
    let now = sim.session.binary_frame;
    sim.mission_assign_exact(actor, MissionId::from_known(MissionType::Hunt), now)
        .unwrap();

    sim.object_ai_visit_one(actor, Some(&rules), ObjectAiCtx::default());

    let entity = sim.substrate.entities.get(actor).unwrap();
    assert!(
        matches!(
            entity.attack_target.target(),
            Some(crate::sim::combat::TargetKind::Entity(id)) if id == target
        ),
        "the scan holds the building"
    );
    assert!(
        matches!(
            entity.navigation.nav_com(),
            Some(NavTargetRef::Object { id }) if id == target
        ),
        "{:?}",
        entity.navigation.nav_com()
    );
    assert!(
        entity.mission.current() == SABOTAGE || entity.mission.queued() == SABOTAGE,
        "{:?}",
        entity.mission
    );
}

/// A player's Sabotage order walks a Tanya into the building. On the cell
/// it reaches, the building takes a 26-frame charge and a 13-frame flash,
/// and the planter stops and rearms. Its Scatter refuses it, as the Walk
/// still heads for the building's centre, so the plant draws for the rearm
/// alone. That walk's next cell is the building's, which the planter may no
/// longer enter: the Walk's blocked arm puts her on Attack against the
/// building, and Mission_Attack's C4 arm sends her back in on Sabotage. The
/// charge goes off on its 26th frame.
#[test]
fn a_tanya_ordered_to_sabotage_walks_in_and_plants_the_charge() {
    let rules = rules();
    let (mut sim, grid) = arena(&rules);
    let building = spawn(&mut sim, &rules, "GAPILE", "Russians", 16, 10);
    let tanya = spawn(&mut sim, &rules, "TANY", "Americans", 10, 10);
    order_plant(&mut sim, "Americans", tanya, building);

    let (planted, draws) =
        crate::sim::rng::trace_draws(|| step_until_planted(&mut sim, &rules, &grid, building));

    let plant_draws: Vec<&str> = draws
        .iter()
        .filter_map(|draw| draw["callers"].as_str())
        .filter(|callers| callers.contains("infantry_per_cell_c4"))
        .collect();
    assert!(!plant_draws.is_empty(), "the plant draws for its rearm");
    assert!(
        plant_draws
            .iter()
            .all(|callers| callers.contains("rearm_c4_planter")
                && !callers.contains("scatter_from_building")),
        "only the rearm draws at the plant"
    );
    let pending = sim
        .substrate
        .entities
        .get(building)
        .unwrap()
        .pending_c4_detonation
        .pending()
        .unwrap();
    assert_eq!(pending.source_entity_id, Some(tanya));
    assert_eq!(
        (pending.timer.start_frame(), pending.timer.duration()),
        (planted as i32, 26)
    );
    let flash = sim
        .substrate
        .entities
        .get(building)
        .unwrap()
        .flashing
        .frames();
    assert!((12..=13).contains(&flash), "flash {flash}");
    let planter = sim.substrate.entities.get(tanya).unwrap();
    assert_eq!(planter.rearm_timer.start_frame(), planted as i32);
    assert!((3..=5).contains(&planter.rearm_timer.duration()));
    assert_eq!(planter.navigation.nav_com(), None, "the planter stopped");
    assert!(
        (16..18).contains(&planter.position.rx()) && (10..12).contains(&planter.position.ry()),
        "the planter did not scatter at the plant"
    );

    let attack = MissionId::from_known(MissionType::Attack);
    let (mut overridden, mut sent_back) = (false, false);
    loop {
        let frame = sim.session.binary_frame;
        step(&mut sim, &rules, &grid);
        let planter = sim.substrate.entities.get(tanya).unwrap();
        let on_building = planter
            .attack_target
            .target()
            .is_some_and(|attack| attack == crate::sim::combat::TargetKind::Entity(building));
        overridden |= planter.mission.current() == attack && on_building;
        sent_back |= overridden
            && planter.mission.current() == SABOTAGE
            && planter.navigation.nav_com() == Some(NavTargetRef::object(building));
        let gone = sim
            .substrate
            .entities
            .get(building)
            .is_none_or(|building| building.death.is_dying());
        if gone {
            assert_eq!(frame, planted + 26);
            break;
        }
        assert!(frame < planted + 26, "the charge went off late");
    }
    assert!(overridden && sent_back, "blocked, the planter goes back in");
    assert!(
        sim.substrate
            .entities
            .get(tanya)
            .is_some_and(|tanya| !tanya.death.is_dying())
    );
}

/// Two identical worlds plant and set the charge off frame for frame alike.
#[test]
fn a_plant_replays_identically() {
    let run = || {
        let rules = rules();
        let (mut sim, grid) = arena(&rules);
        let building = spawn(&mut sim, &rules, "GAPILE", "Russians", 16, 10);
        let tanya = spawn(&mut sim, &rules, "TANY", "Americans", 10, 10);
        order_plant(&mut sim, "Americans", tanya, building);
        (0..160)
            .map(|_| {
                step(&mut sim, &rules, &grid);
                sim.state_hash()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
}

/// The plant itself (`0x0051A5A7..0x0051A664`) on a Walk arrival: the
/// charge with the planter as its source, the flash, the stop and GetROF(1)'s
/// draw. The Scatter refuses the still-walking planter and draws nothing.
#[test]
fn a_walking_planter_plants_and_draws_for_its_rearm_alone() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let building = spawn(&mut sim, &rules, "GAPILE", "Russians", 16, 10);
    let tanya = spawn(&mut sim, &rules, "TANY", "Americans", 15, 10);
    stand_in(&mut sim, tanya, building);
    let (rof, after) = expected_draws(&sim, false);

    assert!(
        sim.infantry_per_cell_c4(tanya, &rules, None, FrameEffects::default())
            .unwrap()
    );

    let target = sim.substrate.entities.get(building).unwrap();
    let pending = target.pending_c4_detonation.pending().unwrap();
    assert_eq!(
        (pending.timer.duration(), pending.source_entity_id),
        (26, Some(tanya))
    );
    assert_eq!(target.flashing.frames(), 13);
    let planter = sim.substrate.entities.get(tanya).unwrap();
    assert_eq!(planter.rearm_timer.duration(), rof);
    assert_eq!(planter.navigation.nav_com(), None);
    assert_eq!(
        planter
            .locomotor
            .as_ref()
            .and_then(|loco| loco.walk_destination_cell()),
        Some((16, 10)),
        "the Walk still holds its cell"
    );
    assert_eq!(sim.scenario_rng.logical_state(), after);
}

/// A second planter reaching a building that already holds a charge
/// (`+0x6DF`) leaves it as it is: it drops its destination, which stops its
/// Walk, then rearms and scatters (`0x0051A550..0x0051A5A4`), drawing for
/// both.
#[test]
fn a_second_planter_leaves_the_first_charge_alone() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let building = spawn(&mut sim, &rules, "GAPILE", "Russians", 16, 10);
    let first = spawn(&mut sim, &rules, "TANY", "Americans", 15, 10);
    let second = spawn(&mut sim, &rules, "GHOST", "Americans", 15, 11);
    stand_in(&mut sim, first, building);
    assert!(
        sim.infantry_per_cell_c4(first, &rules, None, FrameEffects::default())
            .unwrap()
    );
    let charge = sim
        .substrate
        .entities
        .get(building)
        .unwrap()
        .pending_c4_detonation;
    sim.session.binary_frame += 5;
    sim.substrate
        .entities
        .get_mut(building)
        .unwrap()
        .flashing
        .start(0);
    stand_in(&mut sim, second, building);
    let (rof, after) = expected_draws(&sim, true);

    assert!(
        sim.infantry_per_cell_c4(second, &rules, None, FrameEffects::default())
            .unwrap()
    );

    let target = sim.substrate.entities.get(building).unwrap();
    assert_eq!(target.pending_c4_detonation, charge);
    assert_eq!(target.flashing.frames(), 0, "no second flash");
    let planter = sim.substrate.entities.get(second).unwrap();
    assert_eq!(
        (
            planter.rearm_timer.start_frame(),
            planter.rearm_timer.duration()
        ),
        (sim.session.binary_frame as i32, rof)
    );
    assert_ne!(
        planter.navigation.nav_com(),
        Some(NavTargetRef::object(building))
    );
    assert_eq!(sim.scenario_rng.logical_state(), after);
}

/// A building being sold, or Iron Curtained, takes no charge; the planter
/// still stops and rearms, and its Scatter refuses it (`0x0051A52B`,
/// `0x0051A540`).
#[test]
fn a_sold_or_curtained_building_takes_no_charge() {
    let rules = rules();
    for curtained in [false, true] {
        let (mut sim, _) = arena(&rules);
        let building = spawn(&mut sim, &rules, "GAPILE", "Russians", 16, 10);
        let tanya = spawn(&mut sim, &rules, "TANY", "Americans", 15, 10);
        let now = sim.session.binary_frame;
        if curtained {
            crate::sim::superweapon::invulnerability::apply_invulnerability(
                sim.substrate.entities.get_mut(building).unwrap(),
                now,
                40,
                crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain,
            );
        } else {
            sim.mission_assign_exact(building, MissionId::from_known(MissionType::Selling), now)
                .unwrap();
        }
        stand_in(&mut sim, tanya, building);
        let (rof, after) = expected_draws(&sim, false);

        assert!(
            sim.infantry_per_cell_c4(tanya, &rules, None, FrameEffects::default())
                .unwrap()
        );

        let target = sim.substrate.entities.get(building).unwrap();
        assert!(
            target.pending_c4_detonation.pending().is_none(),
            "curtained {curtained}"
        );
        assert_eq!(target.flashing.frames(), 0, "curtained {curtained}");
        let planter = sim.substrate.entities.get(tanya).unwrap();
        assert_eq!(planter.rearm_timer.duration(), rof);
        assert_eq!(sim.scenario_rng.logical_state(), after);
    }
}

/// The arm asks for Sabotage, a `C4=` type and a cell whose first ground
/// building is the NavCom; anything else falls through to the Foot body
/// untouched.
#[test]
fn the_arm_plants_only_from_a_sabotage_c4_arrival_at_its_navcom() {
    let rules = rules();
    for case in ["not sabotage", "no c4", "other navcom"] {
        let (mut sim, _) = arena(&rules);
        let building = spawn(&mut sim, &rules, "GAPILE", "Russians", 16, 10);
        let other = spawn(&mut sim, &rules, "GAPILE", "Russians", 20, 20);
        let kind = if case == "no c4" { "E1" } else { "TANY" };
        let actor = spawn(&mut sim, &rules, kind, "Americans", 15, 10);
        stand_in(&mut sim, actor, building);
        match case {
            "not sabotage" => {
                let now = sim.session.binary_frame;
                sim.mission_assign_exact(actor, MissionId::from_known(MissionType::Capture), now)
                    .unwrap();
            }
            "other navcom" => {
                sim.substrate
                    .entities
                    .get_mut(actor)
                    .unwrap()
                    .navigation
                    .set_nav_com_for_test(Some(NavTargetRef::object(other)));
            }
            _ => {}
        }
        let before = sim.scenario_rng.logical_state();

        assert!(
            !sim.infantry_per_cell_c4(actor, &rules, None, FrameEffects::default())
                .unwrap(),
            "{case}"
        );

        assert!(
            sim.substrate
                .entities
                .get(building)
                .unwrap()
                .pending_c4_detonation
                .pending()
                .is_none(),
            "{case}"
        );
        assert_eq!(sim.scenario_rng.logical_state(), before, "{case}");
    }
}
