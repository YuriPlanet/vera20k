//! GetFireError through the production fire path: the facts the world
//! supplies and each class's reaction to the code.

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
        "[VehicleTypes]\n0=TANK\n\
         [InfantryTypes]\n\
         [BuildingTypes]\n0=COIL\n\
         [AircraftTypes]\n\
         [TANK]\nStrength=300\nArmor=heavy\nPrimary=Gun\n\
         [COIL]\nStrength=600\nArmor=concrete\nPrimary=Zap\n\
         [Gun]\nDamage=50\nROF=30\nRange=6\nWarhead=AP\nOmniFire=yes\n\
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
        coil_shot((20, 10), |coil| coil
            .attack_target
            .as_mut()
            .unwrap()
            .cooldown_ticks = 30),
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
