//! The scan's ally arms: Evaluate_Candidate's G10 (`0x006F7EA2..0x006F7FFB`)
//! and Scan_Cell_For_Target's walk (`0x006F8A30..0x006F8BE0`).

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::capture_manager::MindControlLink;
use crate::sim::combat::ScanMission;
use crate::sim::command::Command;
use crate::sim::house_state::HouseState;
use crate::sim::intern::test_interner;
use crate::sim::passenger::PassengerRole;
use crate::sim::world::Simulation;

const OWNER: &str = "Americans";

/// A human Americans FV on the flat arena with an Engineer boarded through
/// the production boarding, which selects its `Weapon2=RepairBullet`.
fn repair_ifv(rules: &RuleSet) -> (Simulation, u64) {
    let mut sim = Simulation::new();
    for (name, side) in [(OWNER, 0), ("Russians", 1)] {
        let id = sim.interner.intern(name);
        sim.houses
            .insert(id, HouseState::new(id, side, None, true, 0, 10));
        sim.session.house_order.push(id);
    }
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let ifv = sim.spawn_object("FV", OWNER, 16, 16, 64, rules).unwrap();
    let engineer = sim
        .spawn_object("ENGINEER", OWNER, 16, 15, 64, rules)
        .unwrap();
    assert!(sim.apply_command(
        OWNER,
        &Command::EnterTransport {
            passenger_id: engineer,
            transport_id: ifv,
        },
        Some(rules),
    ));
    let grid = sim.path_grid_snapshot().unwrap();
    for _ in 0..240 {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 33);
        if sim
            .substrate
            .entities
            .get(engineer)
            .unwrap()
            .passenger_role
            .is_inside_transport()
        {
            break;
        }
    }
    assert_eq!(
        sim.substrate
            .entities
            .get(ifv)
            .unwrap()
            .current_weapon_number(),
        1,
        "RepairBullet"
    );
    (sim, ifv)
}

fn guard_scan(sim: &mut Simulation, rules: &RuleSet, scanner: u64) -> Option<u64> {
    sim.greatest_threat_represented(
        rules,
        None,
        scanner,
        ScanMission::Guard,
        None,
        crate::sim::combat::combat_targeting::greatest_threat_for_entity,
    )
}

/// A repair IFV's scan takes a damaged vehicle of its own house, not a
/// full-health one, a damaged infantryman or a damaged enemy. Idle, its
/// GetFireError probe refuses the full-health tank (U6,
/// `0x007410F9..0x00741138`); rearming, the probe answers REARM first (T45,
/// `0x006FC94F`) and G10's health test (`0x006F7F4F..0x006F7F67`) refuses it.
/// RepairBullet's Verses refuse the GI and mask `0x4000` the enemy.
#[test]
fn retail_repair_ifv_scan_takes_only_a_damaged_vehicle() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, ifv) = repair_ifv(rules);
    let tank = sim.spawn_object("MTNK", OWNER, 17, 16, 64, rules).unwrap();
    let gi = sim.spawn_object("E1", OWNER, 16, 17, 64, rules).unwrap();
    let enemy = sim
        .spawn_object("HTNK", "Russians", 15, 16, 64, rules)
        .unwrap();
    for id in [gi, enemy] {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .health
            .set_for_test(50);
    }
    let full = rules.object("MTNK").unwrap().strength;
    for rearming in [false, true] {
        let frame = sim.session.binary_frame as i32;
        sim.substrate.entities.get_mut(ifv).unwrap().rearm_timer =
            crate::sim::timer::CdTimer::started(frame, if rearming { 80 } else { 0 });
        sim.substrate
            .entities
            .get_mut(tank)
            .unwrap()
            .health
            .set_for_test(full);
        assert_eq!(
            guard_scan(&mut sim, rules, ifv),
            None,
            "rearming {rearming}"
        );
        sim.substrate
            .entities
            .get_mut(tank)
            .unwrap()
            .health
            .set_for_test(100);
        assert_eq!(
            guard_scan(&mut sim, rules, ifv),
            Some(tank),
            "rearming {rearming}"
        );
    }
}

/// Through the frame: a human's idle repair IFV acquires the damaged tank
/// beside it and its RepairBullet mends it.
#[test]
fn retail_repair_ifv_mends_a_damaged_tank_beside_it() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, ifv) = repair_ifv(rules);
    let tank = sim.spawn_object("MTNK", OWNER, 17, 16, 64, rules).unwrap();
    sim.substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .health
        .set_for_test(100);
    let grid = sim.path_grid_snapshot().unwrap();
    for _ in 0..300 {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 33);
        if sim.substrate.entities.get(tank).unwrap().health.current() > 100 {
            break;
        }
    }
    assert!(
        sim.substrate.entities.get(tank).unwrap().health.current() > 100,
        "the IFV ({:?}) mends the tank",
        sim.substrate.entities.get(ifv).unwrap().attack_target
    );
}

fn place(
    entities: &mut EntityStore,
    id: u64,
    type_id: &str,
    owner: &str,
    cell: (u16, u16),
    category: EntityCategory,
    health: i32,
) {
    let mut entity =
        GameEntity::test_default_of_category(id, type_id, owner, cell.0, cell.1, category);
    entity.lifecycle.set_in_limbo_for_test(false);
    entity.lifecycle.cell_marked = true;
    entity.health.set_for_test(health);
    if category == EntityCategory::Structure {
        entity.type_facts.set_foundation_for_test("1x1".to_string());
    }
    entities.insert(entity);
}

/// A Guard scan with no world: no GetFireError probe, a human owner and
/// rebuilt Cell lists, whose non-buildings are prepended in id order.
fn scan_without_world(entities: &EntityStore, rules: &RuleSet, scanner: u64) -> Option<u64> {
    super::super::acquire_best_target_for_entity(
        entities,
        &OccupancyGrid::rebuild(entities),
        rules,
        &test_interner(),
        scanner,
        None,
        None,
        false,
        ScanMission::Guard,
        None,
        crate::sim::combat::line_of_fire::LineOfFireInputs::default(),
        None,
        None,
    )
    .target()
}

/// With no probe, G10 alone decides a healing vehicle's allies: it takes a
/// damaged vehicle (vt+0x80 of a Unit, `0x00746400`) or a landed aircraft
/// (GetHeight, vt+0x1C8) off a building's cell (`0x0047C520`), and refuses a
/// full-health one (C3, `0x006F7F4F..0x006F7F67`) and an infantryman
/// (`0x004263B0`). Mask `0x4000` refuses a damaged enemy.
#[test]
fn a_healing_vehicle_takes_a_damaged_vehicle_or_a_landed_aircraft_off_a_pad() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=FIXER\n1=TANK\n[InfantryTypes]\n0=MAN\n[AircraftTypes]\n0=PLANE\n\
         [BuildingTypes]\n0=PAD\n[WeaponTypes]\n0=FIX\n\
         [FIXER]\nStrength=200\nArmor=light\nPrimary=FIX\n\
         [TANK]\nStrength=300\nArmor=heavy\n\
         [MAN]\nStrength=100\nArmor=heavy\n\
         [PLANE]\nStrength=150\nArmor=light\n\
         [PAD]\nStrength=500\nArmor=heavy\nFoundation=1x1\n\
         [FIX]\nDamage=-50\nROF=80\nRange=3\nWarhead=MECH\n\
         [MECH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .unwrap();
    const FIXER: u64 = 1;
    let mut entities = EntityStore::new();
    place(
        &mut entities,
        FIXER,
        "FIXER",
        OWNER,
        (5, 5),
        EntityCategory::Unit,
        200,
    );
    place(
        &mut entities,
        2,
        "TANK",
        "Russians",
        (4, 5),
        EntityCategory::Unit,
        100,
    );
    place(
        &mut entities,
        3,
        "MAN",
        OWNER,
        (5, 6),
        EntityCategory::Infantry,
        50,
    );
    place(
        &mut entities,
        4,
        "TANK",
        OWNER,
        (6, 5),
        EntityCategory::Unit,
        300,
    );
    let scan = |entities: &EntityStore| scan_without_world(entities, &rules, FIXER);
    assert_eq!(scan(&entities), None);
    entities.get_mut(4).unwrap().health.set_for_test(100);
    assert_eq!(scan(&entities), Some(4), "a damaged tank");
    entities.get_mut(4).unwrap().health.set_for_test(300);
    place(
        &mut entities,
        5,
        "PLANE",
        OWNER,
        (6, 6),
        EntityCategory::Aircraft,
        50,
    );
    assert_eq!(scan(&entities), Some(5), "a landed aircraft");
    place(
        &mut entities,
        6,
        "PAD",
        OWNER,
        (6, 6),
        EntityCategory::Structure,
        500,
    );
    assert_eq!(scan(&entities), None, "on a pad");
}

/// A rider of a mind-controlled `OpenTopped=` transport fights for the
/// controller's house. Its cell walk stops at what the controller allies
/// (`0x006F8AB8..0x006F8ACE`), even behind a man of the rider's own house,
/// and G10 refuses it (`0x006F7FE9..0x006F7FFB`); a man of the rider's own
/// house alone in a cell is offered (`0x006F8BDA`) and taken. Berserk, the
/// rider stops at the first Techno (`0x006F8AAA`) and still spares the
/// controller's.
#[test]
fn a_controlled_transports_rider_takes_its_own_house_and_spares_the_controllers() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=FORT\n[InfantryTypes]\n0=GI\n[WeaponTypes]\n0=M60\n\
         [FORT]\nStrength=600\nArmor=heavy\nOpenTopped=yes\n\
         [GI]\nStrength=100\nArmor=none\nPrimary=M60\n\
         [M60]\nDamage=15\nROF=20\nRange=5\nWarhead=SA\n\
         [SA]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .unwrap();
    const RIDER: u64 = 1;
    const YURI: &str = "YuriCountry";
    let mut entities = EntityStore::new();
    let infantry = EntityCategory::Infantry;
    place(&mut entities, RIDER, "GI", OWNER, (5, 5), infantry, 100);
    // The transport passes to its controller's house.
    place(
        &mut entities,
        2,
        "FORT",
        YURI,
        (5, 5),
        EntityCategory::Unit,
        600,
    );
    place(&mut entities, 3, "GI", YURI, (20, 20), infantry, 100);
    place(&mut entities, 4, "GI", YURI, (6, 5), infantry, 100);
    // One cell, the rider's own house's man at its head.
    place(&mut entities, 6, "GI", YURI, (7, 5), infantry, 100);
    place(&mut entities, 7, "GI", OWNER, (7, 5), infantry, 100);
    entities.get_mut(RIDER).unwrap().passenger_role = PassengerRole::inside_for_test(2, true);
    entities.get_mut(2).unwrap().mind_control = MindControlLink::controlled_by_for_test(3);
    let scan = |entities: &EntityStore| scan_without_world(entities, &rules, RIDER);
    assert_eq!(scan(&entities), None);
    place(&mut entities, 5, "GI", OWNER, (8, 5), infantry, 100);
    assert_eq!(scan(&entities), Some(5), "the rider's own house's man");
    entities.get_mut(RIDER).unwrap().berserk.active = true;
    assert!(matches!(scan(&entities), Some(5 | 7)), "berserk");
}
