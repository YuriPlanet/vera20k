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
    /// The two extra war factories of the parallel-switch tests.
    war: Option<[u64; 2]>,
}

fn fixture() -> Fixture {
    fixture_with(RULES)
}

fn fixture_with(rules: &str) -> Fixture {
    let art = IniFile::from_str("[YARD]\nFoundation=2x2\n[PLAIN]\nFoundation=2x2\n");
    fixture_from_sources(&IniFile::from_str(rules), &art)
}

fn fixture_from_sources(ini: &IniFile, art: &IniFile) -> Fixture {
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(ini, art).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(art));
    let mut sim = Simulation::new();
    let path = crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let owner = sim.interner.intern("AIHouse");
    let mut house = HouseState::new(owner, 0, None, false, 1_000, 10);
    house.enable_ai_deploy_latches();
    house.base_plan_mut().set_nodes_for_test(vec![BasePlanNode {
        type_or_control: rules.building_type_index("PLAIN").unwrap(),
        packed_cell: pack_base_plan_cell(16, 16),
        filled: false,
        retry_count: 0,
    }]);
    sim.houses.register(house);
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
        war: None,
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
            .factories()
            .building_factory(self.yard)
            .and_then(|factory| factory.object())
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
            .map(|e| (e.position.rx(), e.position.ry()))
    }

    fn node(&self) -> ((i16, i16), i32) {
        let node = self.sim.houses[&self.owner].base_plan().nodes()[0];
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
    assert_eq!(house.economy().credits(), 900, "paid for");
    assert_eq!(house.stats().built(), 1, "Record_Last_Built");
    assert_eq!(
        house.produced_count(
            ObjectCategory::Building,
            f.rules.building_type_index("PLAIN").unwrap()
        ),
        1
    );
    assert!(f.held_object().is_none(), "the yard's factory is gone");
}

/// AI4501A4 records after ExitObject has consumed its wall. The original
/// deferred-delete boundary preserves type and raw ctor Location for that
/// call; Rust's earlier disposal must preserve the same input facts.
#[test]
fn ai_wall_consumption_keeps_its_accounting_and_one_creation_cue() {
    let mut source = RULES.replace(
        "[PLAIN]\n",
        "[PLAIN]\nWall=yes\nCreateSound=FixtureWallCreated\n",
    );
    source.push_str(
        "\n[OverlayTypes]\n0=PLAIN\n[SoundList]\n0=FixtureWallCreated\n\
         [FixtureWallCreated]\nSounds=wall\n",
    );
    let ini = IniFile::from_str(&source);
    let art =
        IniFile::from_str("[YARD]\nFoundation=2x2\n[PLAIN]\nFoundation=2x2\nToOverlay=PLAIN\n");
    let sounds = crate::rules::sound_ini::SoundRegistry::from_ini(&ini);
    let mut f = fixture_from_sources(&ini, &art);
    f.rules.bind_type_sound_references(&ini, &sounds);
    let wall = f.rules.object("PLAIN").unwrap();
    assert!(wall.wall);
    assert_eq!(wall.to_overlay.as_deref(), Some("PLAIN"));
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
    f.sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
    let mut held = None;
    for _ in 0..3_000 {
        f.sim
            .advance_tick(&[], Some(&f.rules), Some(&f.path), Some(&registry), 67);
        held = f.held_object().or(held);
        if f.sim.houses[&f.owner].stats().built() != 0 {
            break;
        }
    }
    let held = held.expect("the AI factory constructed its paid wall");
    assert_eq!(f.sim.houses[&f.owner].economy().credits(), 900);
    assert_eq!(f.sim.houses[&f.owner].stats().built(), 1);
    assert!(!f.sim.substrate.entities.contains(held));
    assert!(
        f.held_object().is_none(),
        "the completed AI factory was removed"
    );
    let cues = f
        .sim
        .sound_events
        .iter()
        .filter_map(|event| {
            let crate::sim::world::SimSoundEvent::VocAt {
                sound_id,
                audible_to,
                rx,
                ry,
                sub_x,
                sub_y,
                world_z_leptons,
            } = event
            else {
                return None;
            };
            (sound_id == "FixtureWallCreated").then_some((
                *audible_to,
                *rx,
                *ry,
                *sub_x,
                *sub_y,
                *world_z_leptons,
            ))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        cues,
        [(
            None,
            0,
            0,
            crate::util::fixed_math::SimFixed::ZERO,
            crate::util::fixed_math::SimFixed::ZERO,
            0
        )]
    );
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
        .register(HouseState::new(enemy, 1, None, false, 0, 10));
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
    assert_eq!(
        f.sim.houses[&f.owner].economy().credits(),
        1_000,
        "refunded"
    );
    assert_eq!(f.sim.houses[&f.owner].stats().built(), 0);
    assert_eq!(
        f.sim.houses[&f.owner].produced_count(
            ObjectCategory::Building,
            f.rules.building_type_index("PLAIN").unwrap(),
        ),
        0,
        "a failed ExitObject does not consume the lifetime limit"
    );
}

const WAR_RULES_TAIL: &str = "[InfantryTypes]\n[AircraftTypes]\n[VehicleTypes]\n0=TANK\n\
    [BuildingTypes]\n0=YARD\n1=PLAIN\n2=WEAP\n\
    [YARD]\nStrength=1000\nConstructionYard=yes\nFactory=BuildingType\n\
    [PLAIN]\nStrength=1000\nCost=100\n\
    [WEAP]\nStrength=1000\nFactory=UnitType\nCost=200\n\
    [TANK]\nStrength=300\nSpeed=5\nCost=100\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
    [Clear]\nBuildable=yes\n";

/// The shared fixture plus a second computer war factory and the preset
/// vehicle choice, for the NPSE parallel-production switch tests. The tests
/// drive `factory_ai` and the charge sweep directly, so no strategy pass
/// rewrites the preset choice.
fn war_fixture(general: &str, tank_tail: &str) -> Fixture {
    let rules = format!(
        "[General]\n{general}PlacementDelay=.05\nAIAlternateProductionCreditCutoff=10\n{}",
        WAR_RULES_TAIL.replace("[TANK]\n", tank_tail)
    );
    let mut f = fixture_with(&rules);
    let weap_1 = f
        .sim
        .spawn_object("WEAP", "AIHouse", 8, 8, 0, &f.rules)
        .unwrap();
    let weap_2 = f
        .sim
        .spawn_object("WEAP", "AIHouse", 8, 12, 0, &f.rules)
        .unwrap();
    f.sim
        .houses
        .get_mut(&f.owner)
        .unwrap()
        .ai_unit_choices_mut()
        .set(crate::sim::ai_unit_choice::UnitChoiceKind::Unit, 0);
    f.war = Some([weap_1, weap_2]);
    f
}

impl Fixture {
    fn war(&self) -> [u64; 2] {
        self.war.expect("war fixture")
    }

    fn holds(&self, building: u64) -> bool {
        self.sim
            .production
            .factories()
            .building_factory(building)
            .and_then(|factory| factory.object())
            .is_some()
    }

    /// One `BuildingClass::Factory_AI` visit.
    fn visit(&mut self, building: u64) {
        factory_ai(
            &mut self.sim,
            &self.rules,
            building,
            None,
            crate::sim::world::FrameEffects::default(),
        );
    }
}

/// Retail default: the house's war factories all fetch the same standing
/// choice and clone it, like the cloning vats (NPSE 1.4.11.4). This guards
/// the behavior the switch exists to suppress.
#[test]
fn without_the_switch_both_war_factories_clone_the_choice() {
    let mut f = war_fixture("", "[TANK]\n");
    assert!(!f.rules.general.disable_parallel_ai_queues);
    assert!(
        !f.rules
            .object("TANK")
            .unwrap()
            .disable_ai_parallel_production
    );
    let [weap_1, weap_2] = f.war();
    f.visit(weap_1);
    assert!(f.holds(weap_1), "the first factory starts the tank");
    f.visit(weap_2);
    assert!(f.holds(weap_2), "the second factory clones it");
}

/// `[General] DisableParallelAIQueues=yes`: while one factory of the house
/// produces, no other starts (`0x45033C` hook, cave `0xC2F07C`); the block
/// releases when that production ends — the native counter decrements in
/// `Exit_Object` (`0x443CCA..0x4452F0`) and `AbandonProduction` (`0x4CA004`).
#[cfg(feature = "npatch")]
#[test]
fn disable_parallel_ai_queues_serializes_war_factory_production() {
    let mut f = war_fixture("DisableParallelAIQueues=yes\n", "[TANK]\n");
    assert!(f.rules.general.disable_parallel_ai_queues);
    let [weap_1, weap_2] = f.war();
    f.visit(weap_1);
    assert!(f.holds(weap_1), "the first factory starts the tank");
    f.visit(weap_2);
    assert!(!f.holds(weap_2), "the second factory waits");
    abandon_production(
        &mut f.sim,
        &f.rules,
        FactoryHolder::Building(weap_1),
        f.owner,
        crate::sim::world::FrameEffects::default(),
    );
    assert!(!f.holds(weap_1), "the production ended");
    // The abandon clears the standing choice; the house's chooser refills it
    // every 8th frame. The chooser itself is outside this test.
    f.sim
        .houses
        .get_mut(&f.owner)
        .unwrap()
        .ai_unit_choices_mut()
        .set(crate::sim::ai_unit_choice::UnitChoiceKind::Unit, 0);
    f.visit(weap_2);
    assert!(f.holds(weap_2), "the release lets the idle factory start");
}

/// `[TANK] DisableAIParallelProduction=yes` with the global switch left no:
/// the type alone is not produced twice (说明书 1.4.11.4 局部代码).
#[cfg(feature = "npatch")]
#[test]
fn disable_ai_parallel_production_scopes_the_block_to_the_type() {
    let mut f = war_fixture(
        "",
        "[TANK]\nStrength=300\nSpeed=5\nCost=100\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
         DisableAIParallelProduction=yes\n",
    );
    assert!(!f.rules.general.disable_parallel_ai_queues);
    assert!(
        f.rules
            .object("TANK")
            .unwrap()
            .disable_ai_parallel_production
    );
    let [weap_1, weap_2] = f.war();
    f.visit(weap_1);
    assert!(f.holds(weap_1), "the first factory starts the tank");
    f.visit(weap_2);
    assert!(!f.holds(weap_2), "the second factory waits");
    abandon_production(
        &mut f.sim,
        &f.rules,
        FactoryHolder::Building(weap_1),
        f.owner,
        crate::sim::world::FrameEffects::default(),
    );
    f.sim
        .houses
        .get_mut(&f.owner)
        .unwrap()
        .ai_unit_choices_mut()
        .set(crate::sim::ai_unit_choice::UnitChoiceKind::Unit, 0);
    f.visit(weap_2);
    assert!(f.holds(weap_2), "the release lets the idle factory start");
}

/// Extension keys cannot change production in a build without NPatch.
#[cfg(not(feature = "npatch"))]
#[test]
fn npse_switches_are_ignored_without_the_build_capability() {
    let mut f = war_fixture(
        "DisableParallelAIQueues=yes\n",
        "[TANK]\nDisableAIParallelProduction=yes\n",
    );
    assert!(!f.rules.general.disable_parallel_ai_queues);
    assert!(
        !f.rules
            .object("TANK")
            .unwrap()
            .disable_ai_parallel_production
    );
    let [weap_1, weap_2] = f.war();
    f.visit(weap_1);
    f.visit(weap_2);
    assert!(f.holds(weap_1));
    assert!(
        f.holds(weap_2),
        "NPSE keys leave stock parallel production intact"
    );
}
