//! GetFireError through the production fire path: the facts the world
//! supplies and each class's reaction to the code.

use super::super::fire_error::FireError;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{AttackTarget, tick_combat};
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::test_interner;
use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::rng::SimRng;

fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=TANK\n1=MEDV\n2=HOUND\n3=BEAST\n4=CARR\n\
         [InfantryTypes]\n\
         [BuildingTypes]\n0=COIL\n1=BUNK\n\
         [AircraftTypes]\n\
         [TANK]\nStrength=300\nArmor=heavy\nPrimary=Gun\n\
         [MEDV]\nStrength=300\nArmor=heavy\nPrimary=Mend\n\
         [HOUND]\nStrength=300\nArmor=heavy\nPrimary=Gun\nNatural=yes\n\
         [BEAST]\nStrength=300\nArmor=heavy\nPrimary=Gun\nUnnatural=yes\n\
         [CARR]\nStrength=300\nArmor=heavy\nPrimary=Launch\n\
         [COIL]\nStrength=600\nArmor=concrete\nPrimary=Zap\nPowered=yes\nPower=-75\n\
         [BUNK]\nStrength=600\nArmor=concrete\nCanBeOccupied=yes\nCanOccupyFire=yes\n\
         MaxNumberOccupants=5\n\
         [Gun]\nDamage=50\nROF=30\nRange=6\nWarhead=AP\nOmniFire=yes\n\
         [Mend]\nDamage=-50\nROF=30\nRange=6\nWarhead=AP\nOmniFire=yes\n\
         [Launch]\nDamage=50\nROF=30\nRange=6\nWarhead=AP\nOmniFire=yes\nSpawner=yes\n\
         [Zap]\nDamage=100\nROF=60\nRange=6\nWarhead=AP\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("fire-error fixture rules")
}

fn spawn(
    store: &mut EntityStore,
    id: u64,
    type_name: &str,
    category: EntityCategory,
    owner: &str,
    at: (u16, u16),
) {
    let mut entity = GameEntity::test_default(id, type_name, owner, at.0, at.1);
    entity.category = category;
    entity.health.current = 300;
    entity.lifecycle.in_limbo = false;
    store.insert(entity);
}

/// A coil (id 1) aimed at a tank (id 2) at `tank_at`; `setup` adjusts the
/// coil. Returns the tank's health and whether the coil kept its target.
fn coil_shot(tank_at: (u16, u16), setup: impl FnOnce(&mut GameEntity)) -> (i32, bool) {
    let rules = rules();
    let mut store = EntityStore::new();
    spawn(
        &mut store,
        1,
        "COIL",
        EntityCategory::Structure,
        "Soviet",
        (10, 10),
    );
    spawn(
        &mut store,
        2,
        "TANK",
        EntityCategory::Unit,
        "Americans",
        tank_at,
    );
    let coil = store.get_mut(1).unwrap();
    coil.attack_target = Some(AttackTarget::new(2));
    setup(coil);
    let mut interner = test_interner();
    let mut occupancy = OccupancyGrid::rebuild(&store);
    let mut rng = SimRng::new(7);
    tick_combat(
        &mut store,
        &mut occupancy,
        &rules,
        &mut interner,
        1,
        67,
        1,
        &mut rng,
    );
    (
        store.get(2).unwrap().health.current,
        store.get(1).unwrap().attack_target.is_some(),
    )
}

/// `BuildingClass::GetFireError` B2 (`0x00447F45`, DrainingMe) answers
/// ILLEGAL, and `BuildingClass::Mission_Attack` (`0x0044B0DE`) drops the
/// target: a Floating Disc's victim stops shooting.
#[test]
fn a_drained_defence_drops_its_target_and_holds_fire() {
    assert_eq!(
        coil_shot((12, 10), |coil| coil.draining_me = Some(9)),
        (300, false)
    );
    let (health, kept) = coil_shot((12, 10), |_| {});
    assert!(
        health < 300 && kept,
        "the undrained coil fires and keeps it"
    );
}

/// The base asks the ROF timer (T45, REARM) before range (T61, RANGE), and a
/// building keeps a target on REARM but drops it on RANGE (`0x0044B728`).
#[test]
fn a_building_drops_a_target_out_of_range_but_keeps_it_while_reloading() {
    assert_eq!(coil_shot((20, 10), |_| {}), (300, false));
    assert_eq!(
        coil_shot((20, 10), |coil| coil.rearm_timer =
            crate::sim::timer::CdTimer::started(0, 30)),
        (300, true)
    );
}

/// T3 (`0x006FC0D3`, vt+0x1D8): a unit still materialising after a
/// Chronosphere or Chrono Legionnaire warp answers REARM and holds fire; it
/// keeps its target and shoots once it has landed.
#[test]
fn a_warping_in_unit_holds_fire_until_it_lands() {
    let fire = |warp_ticks: u32| {
        let rules = rules();
        let mut store = EntityStore::new();
        spawn(
            &mut store,
            1,
            "TANK",
            EntityCategory::Unit,
            "Soviet",
            (10, 10),
        );
        spawn(
            &mut store,
            2,
            "TANK",
            EntityCategory::Unit,
            "Americans",
            (12, 10),
        );
        let tank = store.get_mut(1).unwrap();
        tank.attack_target = Some(AttackTarget::new(2));
        tank.teleport_state = Some(TeleportState {
            phase: TeleportPhase::ChronoDelay,
            target_rx: 10,
            target_ry: 10,
            being_warped_ticks: warp_ticks,
        });
        let mut interner = test_interner();
        let mut occupancy = OccupancyGrid::rebuild(&store);
        let mut rng = SimRng::new(7);
        tick_combat(
            &mut store,
            &mut occupancy,
            &rules,
            &mut interner,
            1,
            67,
            1,
            &mut rng,
        );
        (
            store.get(2).unwrap().health.current,
            store.get(1).unwrap().attack_target.is_some(),
        )
    };
    assert_eq!(fire(5), (300, true));
    let (health, kept) = fire(0);
    assert!(health < 300 && kept, "a landed unit fires");
}

/// One combat frame over `store`, with `power` as the houses' power states.
fn combat_frame(
    store: &mut EntityStore,
    rules: &RuleSet,
    power: &std::collections::BTreeMap<
        crate::sim::intern::InternedId,
        crate::sim::power_system::PowerState,
    >,
) {
    let mut interner = test_interner();
    let mut occupancy = OccupancyGrid::rebuild(store);
    let mut rng = SimRng::new(7);
    crate::sim::combat::tick_combat_with_fog(
        store,
        &mut occupancy,
        rules,
        &mut interner,
        None,
        power,
        None,
        None,
        None,
        None,
        1,
        67,
        1,
        &[],
        None,
        &mut rng,
    );
}

/// B5 (`0x00447F95`, `Is_Operational_For_Output` `0x004555D0`): a Powered
/// defence of a house short of power answers CANT, and Mission_Attack
/// (`0x0044B728`) drops the target. The pre-scan's own low-power term is gone;
/// this is its only path now.
#[test]
fn an_unpowered_defence_drops_its_target() {
    let rules = rules();
    let shot = |output: i32| {
        let mut store = EntityStore::new();
        spawn(
            &mut store,
            1,
            "COIL",
            EntityCategory::Structure,
            "Soviet",
            (10, 10),
        );
        spawn(
            &mut store,
            2,
            "TANK",
            EntityCategory::Unit,
            "Americans",
            (12, 10),
        );
        store.get_mut(1).unwrap().attack_target = Some(AttackTarget::new(2));
        let mut power = std::collections::BTreeMap::new();
        power.insert(
            test_interner().intern("Soviet"),
            crate::sim::power_system::PowerState {
                total_output: output,
                total_drain: 75,
                ..Default::default()
            },
        );
        combat_frame(&mut store, &rules, &power);
        (
            store.get(2).unwrap().health.current,
            store.get(1).unwrap().attack_target.is_some(),
        )
    };
    assert_eq!(shot(50), (300, false), "short of power");
    let (health, kept) = shot(100);
    assert!(health < 300 && kept, "powered, it fires");
}

/// B1 (`0x00447F15`): an occupiable building with nobody inside is ILLEGAL,
/// and the building drops its target.
#[test]
fn an_empty_garrison_drops_its_target() {
    let rules = rules();
    let mut store = EntityStore::new();
    spawn(
        &mut store,
        1,
        "BUNK",
        EntityCategory::Structure,
        "Soviet",
        (10, 10),
    );
    spawn(
        &mut store,
        2,
        "TANK",
        EntityCategory::Unit,
        "Americans",
        (12, 10),
    );
    store.get_mut(1).unwrap().attack_target = Some(AttackTarget::new(2));
    combat_frame(&mut store, &rules, &Default::default());
    assert_eq!(store.get(2).unwrap().health.current, 300);
    assert!(store.get(1).unwrap().attack_target.is_none());
}

/// T5 (`0x006FC109`): a unit in its Chronosphere relocation frame is
/// ILLEGAL. `UnitClass::Fire_At_Target` (`0x00737148`) keeps the target of a
/// weapon that does not heal, so the unit holds fire and its target.
#[test]
fn a_relocating_unit_holds_fire_and_its_target() {
    let rules = rules();
    let mut store = EntityStore::new();
    spawn(
        &mut store,
        1,
        "TANK",
        EntityCategory::Unit,
        "Soviet",
        (10, 10),
    );
    spawn(
        &mut store,
        2,
        "TANK",
        EntityCategory::Unit,
        "Americans",
        (12, 10),
    );
    let tank = store.get_mut(1).unwrap();
    tank.attack_target = Some(AttackTarget::new(2));
    tank.teleport_state = Some(TeleportState {
        phase: TeleportPhase::Relocate,
        target_rx: 10,
        target_ry: 10,
        being_warped_ticks: 0,
    });
    combat_frame(&mut store, &rules, &Default::default());
    assert_eq!(store.get(2).unwrap().health.current, 300);
    assert!(store.get(1).unwrap().attack_target.is_some());
}

/// U6 (`0x007410F9`): a repair weapon fires only at a damaged vehicle, and
/// `Fire_At_Target`'s case 5 (`0x00737148`) drops a healthy one.
#[test]
fn a_repair_weapon_lets_go_of_a_healthy_vehicle() {
    let rules = rules();
    let mend = |health: i32| {
        let mut store = EntityStore::new();
        spawn(
            &mut store,
            1,
            "MEDV",
            EntityCategory::Unit,
            "Americans",
            (10, 10),
        );
        spawn(
            &mut store,
            2,
            "TANK",
            EntityCategory::Unit,
            "Americans",
            (12, 10),
        );
        store.get_mut(2).unwrap().health.current = health;
        store.get_mut(1).unwrap().attack_target = Some(AttackTarget::new(2));
        combat_frame(&mut store, &rules, &Default::default());
        (
            store.get(2).unwrap().health.current,
            store.get(1).unwrap().attack_target.is_some(),
        )
    };
    assert_eq!(mend(300), (300, false), "nothing to repair");
    let (health, kept) = mend(150);
    assert!(health > 150 && kept, "a damaged tank is repaired");
}

/// A flat 16x16 map whose row `y = 10` from `x = 10` to `x = 12` is a bridge
/// (cell flag `0x100`).
fn bridge_row_terrain() -> crate::map::resolved_terrain::ResolvedTerrainGrid {
    let cells = (0..16u16)
        .flat_map(|ry| (0..16u16).map(move |rx| (rx, ry)))
        .map(|(rx, ry)| {
            let mut cell = crate::map::resolved_terrain::test_flat_cell(rx, ry);
            if ry == 10 && (10..=12).contains(&rx) {
                cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
            }
            cell
        })
        .collect();
    crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(16, 16, cells)
}

/// GetFireError through [`super::FireSubject`] on a bridge map, range unasked.
fn bridge_code(sim: &crate::sim::world::Simulation, rules: &RuleSet, firer: &str) -> FireError {
    super::FireSubject {
        world: sim,
        rules,
        overlay_registry: None,
        fog: None,
        firer: sim.substrate.entities.get(1).unwrap(),
        obj: rules.object(firer).unwrap(),
        target: Some(crate::sim::combat::TargetKind::Entity(2)),
        weapon_index: 0,
        garrison: None,
    }
    .fire_error(false)
}

fn bridge_scene(firer: &str) -> crate::sim::world::Simulation {
    let mut sim = crate::sim::world::Simulation::new();
    sim.resolved_terrain = Some(bridge_row_terrain());
    spawn(
        &mut sim.substrate.entities,
        1,
        firer,
        EntityCategory::Unit,
        "Soviet",
        (10, 10),
    );
    spawn(
        &mut sim.substrate.entities,
        2,
        "TANK",
        EntityCategory::Unit,
        "Americans",
        (12, 10),
    );
    // After the spawns: `test_default` interns their names.
    sim.interner = test_interner();
    sim
}

/// T58 (`0x006FCBE6`): an object on the deck (OnBridge `+0x8C`,
/// `GameEntity::on_bridge`) and one under it, both in bridge cells, are
/// ILLEGAL to each other.
#[test]
fn a_tank_on_a_bridge_deck_cannot_shoot_the_tank_below() {
    let rules = rules();
    let mut sim = bridge_scene("TANK");
    assert_eq!(
        bridge_code(&sim, &rules, "TANK"),
        FireError::Ok,
        "both below"
    );
    sim.substrate.entities.get_mut(1).unwrap().on_bridge = true;
    assert_eq!(
        bridge_code(&sim, &rules, "TANK"),
        FireError::Illegal,
        "one on the deck"
    );
}

/// T35 (`0x006FC606`, `IsOnBridge_ForFiring` `0x00703B10`): a spawner in a
/// bridge cell cannot launch (CANT); on the deck it is exempt, and with no
/// spawn ready it waits (REARM).
#[test]
fn a_spawner_under_a_bridge_cannot_launch() {
    let rules = rules();
    let mut sim = bridge_scene("CARR");
    assert_eq!(bridge_code(&sim, &rules, "CARR"), FireError::Cant);
    sim.substrate.entities.get_mut(1).unwrap().on_bridge = true;
    assert_eq!(bridge_code(&sim, &rules, "CARR"), FireError::Rearm);
}

/// Evaluate_Candidate's GetFireError probe (`0x006F7CE8`): a Natural scanner's
/// passive scan passes over an Unnatural enemy (T14 answers ILLEGAL), where
/// the same scanner without the flag acquires it.
#[test]
fn a_natural_scanner_passes_over_an_unnatural_candidate() {
    let scan = |scanner: &str| {
        let rules = rules();
        let mut sim = crate::sim::world::Simulation::new();
        spawn(
            &mut sim.substrate.entities,
            1,
            scanner,
            EntityCategory::Unit,
            "Americans",
            (10, 10),
        );
        spawn(
            &mut sim.substrate.entities,
            2,
            "BEAST",
            EntityCategory::Unit,
            "Soviet",
            (12, 10),
        );
        for id in [1, 2] {
            // On the map: the scan walks the cell lists.
            sim.substrate.entities.get_mut(id).unwrap().lifecycle.cell_marked = true;
        }
        sim.interner = test_interner();
        sim.substrate.occupancy = OccupancyGrid::rebuild(&sim.substrate.entities);
        crate::sim::combat::acquire_best_target_for_entity(
            &sim.substrate.entities,
            &sim.substrate.occupancy,
            &rules,
            &sim.interner,
            1,
            None,
            None,
            false,
            crate::sim::combat::ScanMission::Guard,
            None,
            crate::sim::combat::line_of_fire::LineOfFireInputs::default(),
            Some(&sim),
        )
    };
    assert_eq!(scan("TANK"), Some(2));
    assert_eq!(scan("HOUND"), None);
}
