//! A spy's disguise: the `MakesDisguise=` detonation arm
//! (`0x00469A03..0x00469A2A`) and the `DisguiseAs` bodies it calls through
//! the firer's vt+0x46C (`InfantryClass::DisguiseAs @ 0x00522700`,
//! `TechnoClass::DisguiseAs @ 0x0070E280`). Expected values come from
//! instruction reading; neither body draws.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::cloak_disguise::{DisguiseRuntime, object_disguised_to};
use crate::sim::combat::TargetKind;
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::{FrameEffects, Simulation};

/// The retail spy's flags and its `[MakeupKit]`, `[InvisibleAll]` and
/// `[Snapshot]` lines.
const RULES: &str = "\
[InfantryTypes]
0=SPY
1=E1
2=E2
[VehicleTypes]
0=TANK
[BuildingTypes]
0=PLANT
[SPY]
Strength=100
Speed=4
SpeedType=Foot
Primary=MakeupKit
CanPassiveAquire=no
CanRetaliate=no
Armor=flak
Agent=yes
Infiltrate=yes
CanDisguise=yes
PermaDisguise=yes
Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}
[E1]
Strength=125
Speed=4
SpeedType=Foot
Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}
[E2]
Strength=100
Speed=4
SpeedType=Foot
Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}
[TANK]
Strength=300
Speed=6
SpeedType=Track
Locomotor={4A582745-9839-11d1-B709-00A024DDAFD1}
[PLANT]
Strength=750
Power=100
[MakeupKit]
Damage=1
ROF=100
Range=-2
Projectile=InvisibleAll
Speed=100
FireOnce=yes
Warhead=Snapshot
RevealOnFire=no
[InvisibleAll]
Inviso=yes
Image=none
AA=yes
AG=yes
[Snapshot]
Verses=100%,100%,100%,0%,0%,0%,0%,0%,0%,100%,100%
MakesDisguise=yes
";

fn rules() -> RuleSet {
    RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(RULES),
        &IniFile::from_str("[PLANT]\nFoundation=2x2\n"),
    )
    .unwrap()
}

/// The flat arena with the computer `Russians` and the human `Americans`.
fn arena(rules: &RuleSet) -> (Simulation, crate::sim::pathfinding::PathGrid) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x522700);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.input_delay_ticks = 0;
    sim.session.game_mode_nonzero = true;
    sim.session.current_house = sim.interner.get("Americans");
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

fn id(sim: &Simulation, name: &str) -> InternedId {
    sim.interner.get(name).unwrap()
}

fn disguise(sim: &Simulation, actor: u64) -> &DisguiseRuntime {
    sim.substrate
        .entities
        .get(actor)
        .and_then(|entity| entity.disguise.as_ref())
        .unwrap()
}

/// Orders `actor` to attack `target` as a team's Attack does, then runs the
/// production frame until it is disguised, for at most 60 frames. Answers
/// the frame its disguise was raised on.
fn attack_until_disguised(
    sim: &mut Simulation,
    rules: &RuleSet,
    grid: &crate::sim::pathfinding::PathGrid,
    actor: u64,
    target: u64,
) -> u32 {
    let now = sim.session.binary_frame;
    sim.mission_assign_exact(actor, MissionId::from_known(MissionType::Attack), now)
        .unwrap();
    sim.assign_target_represented(
        actor,
        Some(TargetKind::Entity(target)),
        Some(rules),
        None,
        FrameEffects::default(),
    )
    .unwrap();
    for _ in 0..60 {
        let frame = sim.session.binary_frame;
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(rules), Some(grid), None, 33);
        if disguise(sim, actor).is_disguised() {
            return frame;
        }
    }
    panic!("the spy never disguised");
}

/// The MakeupKit's shot does no damage and dresses the spy as the
/// infantryman it hit, in that infantryman's house: hidden from that house,
/// and from no other.
#[test]
fn a_spy_takes_the_look_of_the_infantryman_its_makeup_kit_hits() {
    let rules = rules();
    let (mut sim, grid) = arena(&rules);
    let spy = spawn(&mut sim, &rules, "SPY", "Russians", 6, 10);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", 20, 10);

    let frame = attack_until_disguised(&mut sim, &rules, &grid, spy, gi);

    let worn = disguise(&sim, spy);
    assert_eq!(
        (worn.type_id(), worn.house(), worn.creation_frame()),
        (Some(id(&sim, "E1")), Some(id(&sim, "Americans")), frame)
    );
    assert_eq!(
        sim.substrate.entities.get(gi).unwrap().health.current(),
        125
    );
    let spy = sim.substrate.entities.get(spy).unwrap();
    let seen_by = |house: &str| {
        object_disguised_to(
            spy,
            id(&sim, house),
            Some(&sim.fog),
            Some(&sim.house_alliances),
            &sim.interner,
        )
    };
    assert!(seen_by("Americans"), "the Americans take it for their own");
    assert!(!seen_by("Russians"), "its own house sees through it");
}

/// A spy that hits a disguised infantryman wears that disguise
/// (GetDisguise(1) and GetDisguiseHouse(1)), not its real type and house.
#[test]
fn a_spy_hitting_a_disguised_spy_wears_its_disguise() {
    let rules = rules();
    let (mut sim, grid) = arena(&rules);
    let spy = spawn(&mut sim, &rules, "SPY", "Russians", 6, 10);
    let rival = spawn(&mut sim, &rules, "SPY", "Americans", 20, 10);
    let (conscript, russians) = (id(&sim, "E2"), id(&sim, "Russians"));
    let mut worn = DisguiseRuntime::new(0);
    worn.acquire(0, Some(conscript), Some(russians));
    sim.substrate.entities.get_mut(rival).unwrap().disguise = Some(worn);

    attack_until_disguised(&mut sim, &rules, &grid, spy, rival);

    let worn = disguise(&sim, spy);
    assert_eq!(
        (worn.type_id(), worn.house()),
        (Some(conscript), Some(russians))
    );
}

/// Only an Infantry Target disguises an infantryman (WhatAmI 15 at
/// `0x00522713`). A Building or Aircraft takes the Techno base, which raises
/// the flag for any non-null Target and keeps the type and house.
#[test]
fn disguise_as_takes_the_firers_class_body() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let spy = spawn(&mut sim, &rules, "SPY", "Russians", 6, 10);
    let tank = spawn(&mut sim, &rules, "TANK", "Americans", 20, 10);
    let plant = spawn(&mut sim, &rules, "PLANT", "Americans", 20, 20);

    sim.disguise_as(spy, Some(tank), true);
    sim.disguise_as(spy, None, true);
    assert!(!disguise(&sim, spy).is_disguised(), "a vehicle or a cell");

    sim.disguise_as(plant, None, false);
    assert!(!disguise(&sim, plant).is_disguised(), "a null Target");
    sim.session.binary_frame = 7;
    sim.disguise_as(plant, None, true);
    let worn = disguise(&sim, plant);
    assert_eq!(
        (worn.is_disguised(), worn.creation_frame()),
        (true, 7),
        "a cell"
    );
    assert_eq!((worn.type_id(), worn.house()), (None, None));
}

/// Script action 0's quarry 4 (`Quarry_To_Threat` mask `0x8`) takes the
/// flat walk (`0x006F8FE0`), so a spy team's leader finds an enemy
/// infantryman anywhere on the map.
#[test]
fn a_spy_leaders_infantry_quarry_finds_an_enemy_anywhere() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let spy = spawn(&mut sim, &rules, "SPY", "Russians", 2, 2);
    let conscript = spawn(&mut sim, &rules, "E2", "Americans", 29, 29);
    let found = crate::sim::world::direct_greatest_threat(
        &mut sim,
        &rules,
        None,
        spy,
        crate::sim::combat::ScanMission::Quarry {
            mask: 0x8,
            only_target_house_enemy: false,
        },
    );
    assert_eq!(found, Some(conscript));
}
