//! The crush arms of `UnitClass::Approach_Target @ 0x007414E0`: the native
//! rows of `tools/unit_crush_oracle.json` `approach`, and Rust regression
//! tests of the head and the per-cell range stop on retail rules.

use super::ApproachCrush;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{AttackTarget, TargetKind};
use crate::sim::components::NavTargetRef;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::Simulation;
use crate::util::native_x87::object_distance;
use std::cell::Cell;

/// `0x007414E0` executed on scratch objects up to its SetDestination or its
/// fall to `FootClass::Approach_Target`, with `ObjectClass::Distance` run
/// natively: the computer arm takes a Target strictly inside `Crush=` (460
/// leptons on an axis roots to 460 and is refused), the OmniCrusher arm any
/// crushable Object but a human house's passively acquired one. Each arm
/// measures the distance and asks `Is_Crushable_By` only where native does.
#[test]
fn approach_crush_matches_native_execution() {
    let payload: serde_json::Value =
        serde_json::from_str(crate::test_fixture::text("tools/unit_crush_oracle.json")).unwrap();
    let rows = payload["approach"].as_array().unwrap();
    assert!(rows.len() >= 30);
    for row in rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let flag = |key: &str| input[key].as_bool().unwrap();
        let xyz = |key: &str| -> [i32; 3] {
            let v = &input[key];
            [0, 1, 2].map(|axis| v[axis].as_i64().unwrap() as i32)
        };
        let flags = input["flags"].as_i64().unwrap();
        let facts = ApproachCrush {
            controlled_by_human: flag("controlled_by_human"),
            nav_com: flag("nav_com"),
            crusher: flag("crusher") || flag("ability"),
            reach: input["crush"].as_i64().unwrap() as i32,
            techno: flags & 1 != 0,
            object: flags & 2 != 0,
            omni: flag("omni"),
            is_human: flag("is_human"),
            passive: flag("passive"),
        };
        let (measured, asked) = (Cell::new(0), Cell::new(0));
        let distance = || {
            measured.set(measured.get() + 1);
            object_distance(xyz("unit"), xyz("target"), None)
        };
        let crushable = || {
            asked.set(asked.get() + 1);
            flag("crushable")
        };
        let takes = flag("has_target")
            && (facts.computer_arm(distance, crushable) || facts.omni_arm(crushable));
        let output = &row["output"];
        let set = output["destination"].as_str() == Some("target");
        let calls = output["calls"].as_array().unwrap();
        let count = |name: &str| calls.iter().filter(|call| *call == name).count();
        assert_eq!(measured.get(), count("target_coords"), "{name}: Distance");
        assert_eq!(asked.get(), count("crushable"), "{name}: Is_Crushable_By");
        let foot = count("foot") == 1;
        assert_eq!(set, takes && !flag("no_move"), "{name}");
        assert_eq!(
            output["returned_target"].as_bool().unwrap(),
            takes,
            "{name}"
        );
        assert_eq!(foot, !takes, "{name}");
    }
}

fn retail_rules() -> Option<RuleSet> {
    let (ini, art) = crate::rules::retail_ini_fixture::retail_rules_and_art()?;
    Some(RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("retail rules"))
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, cell: (u16, u16)) -> u64 {
    sim.spawn_object(kind, owner, cell.0, cell.1, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

fn target(sim: &mut Simulation, actor: u64, target: u64) {
    sim.substrate.entities.get_mut(actor).unwrap().attack_target = Some(AttackTarget {
        target: TargetKind::Entity(target),
    });
}

fn start_mission(sim: &mut Simulation, actor: u64, mission: MissionType) {
    let now = sim.session.binary_frame;
    sim.mission_assign_exact(actor, MissionId::from_known(mission), now)
        .unwrap();
    let _ = sim.mission_commence_exact(actor, now);
}

fn set_nav_com(sim: &mut Simulation, actor: u64, nav: NavTargetRef) {
    sim.substrate
        .entities
        .get_mut(actor)
        .unwrap()
        .navigation
        .nav_com = Some(nav);
}

/// A computer's Rhino takes a GI a cell away as its destination; a human's
/// Rhino does not, nor one already holding a NavCom, nor for a GI two cells
/// away, an allied GI or a tank it cannot crush.
#[test]
fn computer_crusher_heads_for_a_crushable_target_within_reach() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let head = |rhino_owner: &str,
                victim: (&str, &str, (u16, u16)),
                setup: &dyn Fn(&mut Simulation, u64)| {
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x7414E0);
        let rhino = spawn(&mut sim, &rules, "HTNK", rhino_owner, (10, 10));
        let (kind, owner, cell) = victim;
        let victim = spawn(&mut sim, &rules, kind, owner, cell);
        target(&mut sim, rhino, victim);
        setup(&mut sim, rhino);
        sim.unit_approach_head(rhino, &rules) == Some(NavTargetRef::object(victim))
    };
    let none = |_: &mut Simulation, _: u64| {};
    assert!(head("Russians", ("E1", "Americans", (11, 10)), &none));
    assert!(!head("Americans", ("E1", "Russians", (11, 10)), &none));
    assert!(!head("Russians", ("E1", "Americans", (12, 10)), &none));
    assert!(!head("Russians", ("MTNK", "Americans", (11, 10)), &none));
    assert!(!head(
        "Russians",
        ("E1", "Americans", (11, 10)),
        &|sim, rhino| set_nav_com(sim, rhino, NavTargetRef::cell(4, 4))
    ));
    assert!(!head(
        "Russians",
        ("E1", "Americans", (11, 10)),
        &|sim, _| {
            sim.house_alliances = crate::map::houses::HouseAllianceMap::from([(
                "RUSSIANS".to_string(),
                std::collections::BTreeSet::from(["AMERICANS".to_string()]),
            )]);
        }
    ));
}

/// A Battle Fortress (`OmniCrusher=`) takes infantry it targets as its
/// destination at any range, also a human's; a human's spares a GI its own
/// scan picked up, a computer's does not, and neither drives onto a building.
#[test]
fn battle_fortress_heads_for_infantry_it_targets() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let head = |owner: &str, victim: (&str, &str), passive: bool| {
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x741603);
        let fortress = spawn(&mut sim, &rules, "BFRT", owner, (10, 10));
        let victim = spawn(&mut sim, &rules, victim.0, victim.1, (17, 10));
        target(&mut sim, fortress, victim);
        sim.substrate
            .entities
            .get_mut(fortress)
            .unwrap()
            .passively_acquired_target = passive;
        sim.unit_approach_head(fortress, &rules) == Some(NavTargetRef::object(victim))
    };
    assert!(head("Americans", ("E1", "Russians"), false));
    assert!(!head("Americans", ("E1", "Russians"), true));
    assert!(head("Russians", ("E1", "Americans"), true));
    assert!(!head("Americans", ("NAPOWR", "Russians"), false));
}

/// A computer's Rhino whose charge at a GI the crush arm carries takes the
/// per-cell range stop on arriving in the cell before the GI's
/// (`0x004D882F..0x004D896E`), the GI in weapon range: its NavCom is dropped,
/// and its next Attack tick charges again.
#[test]
fn a_crush_charge_takes_the_per_cell_range_stop() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x4D882F);
    let rhino = spawn(&mut sim, &rules, "HTNK", "Russians", (11, 10));
    let gi = spawn(&mut sim, &rules, "E1", "Americans", (12, 10));
    target(&mut sim, rhino, gi);
    start_mission(&mut sim, rhino, MissionType::Attack);
    set_nav_com(&mut sim, rhino, NavTargetRef::object(gi));
    sim.per_cell_process(
        rhino,
        crate::sim::movement::PerCellReason::Arrival,
        Some(&rules),
        None,
        crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    let entity = sim.substrate.entities.get(rhino).unwrap();
    assert_eq!(entity.navigation.nav_com, None);
    assert!(sim.substrate.entities.contains(gi));
    assert_eq!(
        sim.unit_approach_head(rhino, &rules),
        Some(NavTargetRef::object(gi))
    );
}
