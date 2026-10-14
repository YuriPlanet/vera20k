use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::base_plan::{BasePlanNode, pack_base_plan_cell, unpack_base_plan_cell};
use crate::sim::house_state::HouseState;
use crate::sim::pathfinding::PathGrid;

const RULES: &str = "[General]\nPlacementDelay=.05\nAIAlternateProductionCreditCutoff=10\n\
    [AI]\nBuildConst=YARD\n\
    [InfantryTypes]\n[AircraftTypes]\n[VehicleTypes]\n0=TANK\n\
    [BuildingTypes]\n0=YARD\n1=PLAIN\n\
    [YARD]\nStrength=1000\nConstructionYard=yes\nFactory=BuildingType\n\
    [PLAIN]\nStrength=1000\nCost=100\n\
    [TANK]\nStrength=300\nSpeed=5\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
    [Clear]\nBuildable=yes\n";

/// A skirmish computer house with its production latches set, 1000 credits,
/// a yard at (12, 12) and one plan node, PLAIN at (16, 16) with its
/// foundation reserved, inside the fixture's `In_Bounds` diamond.
struct Fixture {
    sim: Simulation,
    rules: RuleSet,
    path: PathGrid,
    owner: InternedId,
    yard: u64,
}

fn fixture() -> Fixture {
    fixture_with(RULES)
}

fn fixture_with(rules: &str) -> Fixture {
    let art = IniFile::from_str("[YARD]\nFoundation=2x2\n[PLAIN]\nFoundation=2x2\n");
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(rules), &art).unwrap();
    let mut sim = Simulation::new();
    let path = crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    sim.session.game_mode_nonzero = true;
    let owner = sim.interner.intern("AIHouse");
    let mut house = HouseState::new(owner, 0, None, false, 1_000, 10);
    house.enable_ai_deploy_latches();
    house.base_plan.nodes = vec![BasePlanNode {
        type_or_control: rules.building_type_index("PLAIN").unwrap(),
        packed_cell: pack_base_plan_cell(16, 16),
        filled: false,
        retry_count: 0,
    }];
    sim.houses.insert(owner, house);
    sim.session.house_order.push(owner);
    let yard = sim
        .spawn_object("YARD", "AIHouse", 12, 12, 0, &rules)
        .unwrap();
    for (x, y) in [(16, 16), (17, 16), (16, 17), (17, 17)] {
        sim.substrate
            .base_reservations
            .reserve(sim.resolved_terrain.as_ref(), x, y, 0);
    }
    Fixture {
        sim,
        rules,
        path,
        owner,
        yard,
    }
}

impl Fixture {
    fn tick(&mut self) {
        self.sim
            .advance_tick(&[], Some(&self.rules), Some(&self.path), None, 67);
    }

    /// The yard's object while its factory holds one.
    fn held_object(&self) -> Option<u64> {
        self.sim
            .production
            .factories
            .building_factory(self.yard)
            .and_then(|factory| factory.object.as_ref())
            .and_then(|object| object.entity_id)
    }

    fn placed(&self) -> Option<(u16, u16)> {
        self.sim
            .substrate
            .entities
            .values()
            .find(|e| {
                !e.death.is_dying()
                    && !e.lifecycle.in_limbo()
                    && self.sim.interner.resolve(e.type_ref()) == "PLAIN"
            })
            .map(|e| (e.position.rx, e.position.ry))
    }

    fn node(&self) -> ((i16, i16), i32) {
        let node = self.sim.houses[&self.owner].base_plan.nodes[0];
        (unpack_base_plan_cell(node.packed_cell), node.retry_count)
    }
}

#[test]
fn the_yard_builds_its_choice_and_places_it_on_the_node() {
    let mut f = fixture();
    let mut frames = 0;
    while f.placed().is_none() && frames < 3000 {
        f.tick();
        frames += 1;
    }
    assert_eq!(f.placed(), Some((16, 16)), "after {frames} frames");
    let house = &f.sim.houses[&f.owner];
    assert_eq!(house.economy.credits(), 900, "paid for");
    assert_eq!(house.stats.built(), 1, "Record_Last_Built");
    assert!(f.held_object().is_none(), "the yard's factory is gone");
}

#[test]
fn a_placed_building_that_does_not_score_is_not_counted() {
    let mut f = fixture_with(&RULES.replace("[PLAIN]\n", "[PLAIN]\nDontScore=yes\n"));
    let mut frames = 0;
    while f.placed().is_none() && frames < 3000 {
        f.tick();
        frames += 1;
    }
    assert_eq!(f.placed(), Some((16, 16)), "after {frames} frames");
    assert_eq!(f.sim.houses[&f.owner].stats.built(), 0);
}

#[test]
fn after_a_blocked_try_the_yard_waits_the_placement_delay() {
    let mut f = fixture();
    // A unit of the house parks on the site; the exit tells it to leave.
    f.sim
        .spawn_object("TANK", "AIHouse", 17, 17, 0, &f.rules)
        .unwrap();
    let mut frames = 0;
    while f.node().1 == 0 && frames < 3000 {
        f.tick();
        frames += 1;
    }
    assert_eq!(
        f.node(),
        ((16, 16), 1),
        "one try later after {frames} frames"
    );
    let tried = frames;
    // The next try places the building or counts another failure.
    while f.node().1 == 1 && f.placed().is_none() && frames < tried + 200 {
        f.tick();
        frames += 1;
    }
    assert!(f.placed().is_some() || f.node().1 == 2);
    assert_eq!(
        frames - tried,
        f.rules.general.placement_delay_frames(),
        "the next try follows PlacementDelay=.05 later"
    );
}

#[test]
fn a_failed_exit_abandons_the_object_and_refunds_it() {
    let mut f = fixture();
    let enemy = f.sim.interner.intern("Enemy");
    f.sim
        .houses
        .insert(enemy, HouseState::new(enemy, 1, None, false, 0, 10));
    f.sim.session.house_order.push(enemy);
    f.sim
        .spawn_object("TANK", "Enemy", 16, 16, 0, &f.rules)
        .unwrap();
    let mut frames = 0;
    let mut object = None;
    while f.node().0 == (16, 16) && frames < 3000 {
        f.tick();
        frames += 1;
        object = object.or(f.held_object());
    }
    let object = object.expect("the yard made the object");
    assert_eq!(
        f.node(),
        ((0, 0), 0),
        "the node forgot its cell after {frames} frames"
    );
    assert!(
        f.sim
            .substrate
            .entities
            .get(object)
            .is_none_or(|e| e.death.is_dying()),
        "the object is destroyed"
    );
    assert_eq!(f.sim.houses[&f.owner].economy.credits(), 1_000, "refunded");
    assert_eq!(f.sim.houses[&f.owner].stats.built(), 0);
}
