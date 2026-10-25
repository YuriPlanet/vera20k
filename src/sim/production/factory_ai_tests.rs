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

const AI_LIMIT_RULES_TAIL: &str = "[InfantryTypes]\n[AircraftTypes]\n[VehicleTypes]\n0=TANK\n\
    [BuildingTypes]\n0=YARD\n1=PLAIN\n2=WEAP\n\
    [YARD]\nStrength=1000\nConstructionYard=yes\nFactory=BuildingType\n\
    [PLAIN]\nStrength=1000\nCost=100\n\
    [WEAP]\nStrength=1000\nFactory=UnitType\nCost=200\n\
    [TANK]\nStrength=300\nSpeed=5\nCost=100\n{TAIL}\n\
    Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
    [Clear]\nBuildable=yes\n";

/// The shared fixture plus two computer war factories and the preset vehicle
/// choice, for the NPSE per-type, reservation and legacy limit tests. They drive
/// `factory_ai` directly, so no strategy pass rewrites the preset choice.
fn ai_limit_fixture(general: &str, tank_tail: &str) -> (Fixture, u64, u64) {
    let rules = format!(
        "[General]\nPlacementDelay=.05\nAIAlternateProductionCreditCutoff=10\n{general}\n{}",
        AI_LIMIT_RULES_TAIL.replace("{TAIL}", tank_tail)
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
    (f, weap_1, weap_2)
}

fn ai_limit_holds(f: &Fixture, building: u64) -> bool {
    f.sim
        .production
        .factories()
        .building_factory(building)
        .and_then(|factory| factory.object())
        .is_some()
}

fn ai_limit_visit(f: &mut Fixture, building: u64) {
    factory_ai(
        &mut f.sim,
        &f.rules,
        building,
        None,
        crate::sim::world::FrameEffects::default(),
    );
}

/// No `AIBuildLimit=` (`i32::MAX`, the constructor default `0xC32950`): the
/// war factories clone the standing choice as stock does.
#[test]
fn without_aibuild_limit_the_war_factories_clone_the_choice() {
    let (mut f, weap_1, weap_2) = ai_limit_fixture("", "");
    assert_eq!(f.rules.object("TANK").unwrap().aibuild_limit, i32::MAX);
    ai_limit_visit(&mut f, weap_1);
    ai_limit_visit(&mut f, weap_2);
    assert!(ai_limit_holds(&f, weap_1) && ai_limit_holds(&f, weap_2));
}

/// `AIBuildLimit=1`: a factory starts while the house tracks nothing, and
/// once the first tank is in production (the limbo object tracks from
/// production start) a factoryless building skips and the choice slot
/// clears (`0xC335C4..0xC33616`) — the clone production never exceeds the
/// limit.
#[cfg(feature = "npatch")]
#[test]
fn aibuild_limit_skips_and_clears_the_choice_at_the_limit() {
    let (mut f, weap_1, weap_2) = ai_limit_fixture("", "AIBuildLimit=1");
    ai_limit_visit(&mut f, weap_1);
    assert!(ai_limit_holds(&f, weap_1), "below the limit it starts");
    ai_limit_visit(&mut f, weap_2);
    assert!(
        !ai_limit_holds(&f, weap_2),
        "the held tank already reaches the limit"
    );
    assert_eq!(
        f.sim.houses[&f.owner]
            .ai_unit_choices()
            .get(crate::sim::ai_unit_choice::UnitChoiceKind::Unit),
        -1,
        "the at-limit arm cleared the unit choice slot"
    );
}

/// A non-positive `AIBuildLimit=` compares by magnitude against the
/// produced and owned counts (the native negated signed compares), so
/// `-1` blocks once one tank exists.
#[cfg(feature = "npatch")]
#[test]
fn aibuild_limit_non_positive_compares_the_magnitude() {
    let (mut f, weap_1, _) = ai_limit_fixture("", "AIBuildLimit=-1");
    f.sim
        .spawn_object("TANK", "AIHouse", 14, 14, 0, &f.rules)
        .unwrap();
    ai_limit_visit(&mut f, weap_1);
    assert!(
        !ai_limit_holds(&f, weap_1),
        "one owned tank is already at the magnitude"
    );
    assert_eq!(
        f.sim.houses[&f.owner]
            .ai_unit_choices()
            .get(crate::sim::ai_unit_choice::UnitChoiceKind::Unit),
        -1
    );
}

/// Even an authored zero cap cannot affect a build without NPatch.
#[cfg(not(feature = "npatch"))]
#[test]
fn aibuild_limit_is_ignored_without_npatch() {
    let (mut f, weap_1, weap_2) = ai_limit_fixture("", "AIBuildLimit=0");
    assert_eq!(f.rules.object("TANK").unwrap().aibuild_limit, i32::MAX);
    ai_limit_visit(&mut f, weap_1);
    ai_limit_visit(&mut f, weap_2);
    assert!(ai_limit_holds(&f, weap_1) && ai_limit_holds(&f, weap_2));
    assert_eq!(
        f.sim.houses[&f.owner]
            .ai_unit_choices()
            .get(UnitChoiceKind::Unit),
        0,
        "the ignored extension does not invalidate the standing choice",
    );
}

/// The legacy global reader is opt-in and requires the compiled capability.
#[test]
fn legacy_ai_limit_switch_keeps_default_cloning_and_requires_npatch() {
    let (mut plain, first, second) = ai_limit_fixture("", "BuildLimit=1");
    assert!(!plain.rules.general.enable_ai_build_limitation);
    ai_limit_visit(&mut plain, first);
    ai_limit_visit(&mut plain, second);
    assert!(ai_limit_holds(&plain, first) && ai_limit_holds(&plain, second));

    let (mut limited, first, second) =
        ai_limit_fixture("EnableAIBuildLimitation=yes", "BuildLimit=1");
    assert_eq!(
        limited.rules.general.enable_ai_build_limitation,
        crate::rules::NPATCH_ENABLED
    );
    ai_limit_visit(&mut limited, first);
    ai_limit_visit(&mut limited, second);
    assert!(ai_limit_holds(&limited, first));
    assert_eq!(
        ai_limit_holds(&limited, second),
        !crate::rules::NPATCH_ENABLED
    );
}

/// NPExt173 C2D00D..C2D037 also checks owned counts for a negative cap,
/// even when no factory has produced an object of this type yet.
#[cfg(feature = "npatch")]
#[test]
fn legacy_negative_ai_limit_includes_owned_unproduced_objects() {
    let (mut f, first, _) = ai_limit_fixture("EnableAIBuildLimitation=yes", "BuildLimit=-1");
    f.sim
        .spawn_object("TANK", "AIHouse", 14, 14, 0, &f.rules)
        .unwrap();
    let index = f
        .rules
        .type_array_index(ObjectCategory::Vehicle, "TANK")
        .unwrap();
    assert_eq!(
        f.sim.houses[&f.owner].produced_count(ObjectCategory::Vehicle, index),
        0
    );
    ai_limit_visit(&mut f, first);
    assert!(!ai_limit_holds(&f, first));
    assert_eq!(
        f.sim.houses[&f.owner]
            .ai_unit_choices()
            .get(UnitChoiceKind::Unit),
        -1
    );
}

/// The ext173 gate reads the house's tracked table for every class; the stock
/// `CheckBuildLimit` swaps in the runway-pad count for an `AirportBound=`
/// aircraft instead. A capped aircraft with no airfield would look blocked
/// through `CanBuild`, but the AI gate has to compare the type's own count.
#[cfg(feature = "npatch")]
#[test]
fn the_gate_compares_an_airport_bound_aircraft_by_its_own_count() {
    let mut f = fixture_with(
        "[General]\nEnableAIBuildLimitation=yes\nPlacementDelay=.05\n\
         AIAlternateProductionCreditCutoff=10\n\
         [InfantryTypes]\n[AircraftTypes]\n0=HELI\n[VehicleTypes]\n0=TANK\n\
         [BuildingTypes]\n0=YARD\n1=PLAIN\n2=WEAP\n\
         [YARD]\nStrength=1000\nConstructionYard=yes\nFactory=BuildingType\n\
         [PLAIN]\nStrength=1000\nCost=100\n\
         [WEAP]\nStrength=1000\nFactory=UnitType\nCost=200\n\
         [TANK]\nStrength=300\nSpeed=5\nCost=100\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
         [HELI]\nStrength=300\nCost=100\nBuildLimit=1\nAirportBound=yes\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
         [Clear]\nBuildable=yes\n",
    );
    let heli = f.rules.object("HELI").expect("HELI is an aircraft type");
    assert!(heli.airport_bound);
    assert!(
        crate::sim::production::can_build::check_build_limit(&f.sim, &f.rules, f.owner, heli),
        "the stock CanBuild branch compares runway pads, not the limit"
    );
    assert!(
        !super::ai_build_limit_blocks(&f.sim, &f.rules, f.owner, heli, heli.build_limit),
        "the gate compares the house's count of the type"
    );
}

/// Retail default: the house's war factories all fetch the same standing
/// choice and clone it, like the cloning vats (NPSE 1.4.11.4). This guards
/// the behavior the switch exists to suppress.
#[test]
fn without_the_switch_both_war_factories_clone_the_choice() {
    let (mut f, weap_1, weap_2) = ai_limit_fixture("", "");
    assert!(!f.rules.general.disable_parallel_ai_queues);
    ai_limit_visit(&mut f, weap_1);
    assert!(
        ai_limit_holds(&f, weap_1),
        "the first factory starts the tank"
    );
    ai_limit_visit(&mut f, weap_2);
    assert!(ai_limit_holds(&f, weap_2), "the second factory clones it");
}

/// `[General] DisableParallelAIQueues=yes`: while one factory of the house
/// produces, no other starts (`0x45033C` hook, cave `0xC2F07C`); the block
/// releases through `Exit_Object` and `AbandonProduction`'s reservation
/// hooks (`0xC33940`, `0xC2EFB4`).
#[cfg(feature = "npatch")]
#[test]
fn disable_parallel_ai_queues_serializes_war_factory_production() {
    let (mut f, weap_1, weap_2) = ai_limit_fixture("DisableParallelAIQueues=yes", "");
    assert!(f.rules.general.disable_parallel_ai_queues);
    ai_limit_visit(&mut f, weap_1);
    assert!(
        ai_limit_holds(&f, weap_1),
        "the first factory starts the tank"
    );
    ai_limit_visit(&mut f, weap_2);
    assert!(!ai_limit_holds(&f, weap_2), "the second factory waits");
    abandon_production(
        &mut f.sim,
        &f.rules,
        FactoryHolder::Building(weap_1),
        f.owner,
        crate::sim::world::FrameEffects::default(),
    );
    assert!(!ai_limit_holds(&f, weap_1), "the production ended");
    // The abandon clears the standing choice; the house's chooser refills it
    // every 8th frame. The chooser itself is outside this test.
    f.sim
        .houses
        .get_mut(&f.owner)
        .unwrap()
        .ai_unit_choices_mut()
        .set(crate::sim::ai_unit_choice::UnitChoiceKind::Unit, 0);
    ai_limit_visit(&mut f, weap_2);
    assert!(
        ai_limit_holds(&f, weap_2),
        "the release lets the idle factory start"
    );
}

const NAVAL_RULES_TAIL: &str = "[InfantryTypes]\n[AircraftTypes]\n[VehicleTypes]\n0=TANK\n1=SHIP\n\
    [BuildingTypes]\n0=YARD\n1=PLAIN\n2=WEAP\n3=SYRD\n\
    [YARD]\nStrength=1000\nConstructionYard=yes\nFactory=BuildingType\n\
    [PLAIN]\nStrength=1000\nCost=100\n\
    [WEAP]\nStrength=1000\nFactory=UnitType\nCost=200\n\
    [SYRD]\nStrength=1000\nFactory=UnitType\nNaval=yes\nCost=200\n\
    [TANK]\nStrength=300\nSpeed=5\nCost=100\n\
    Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
    [SHIP]\nStrength=300\nSpeed=5\nCost=100\nNaval=yes\n\
    Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
    [Clear]\nBuildable=yes\n";

/// NPSE `Suggest_New_Object` (`0x4FBDAF -> 0xC2F130`) reads `House+0x16130`
/// from a naval producer and `House+0x5650` from a land one, so a shipyard and
/// a war factory stop sharing one vehicle choice. Before this split the
/// shipyard fetched the war factory's tank, failed the `Naval=` mismatch check
/// at `0x450343` and idled.
#[cfg(feature = "npatch")]
#[test]
fn a_shipyard_takes_the_naval_choice_while_the_war_factory_keeps_its_own() {
    use crate::rules::object_type::ObjectCategory;
    use crate::sim::ai_unit_choice::UnitChoiceKind;

    let mut f = fixture_with(
        "[General]\nPlacementDelay=.05\nAIAlternateProductionCreditCutoff=10\n\
         [InfantryTypes]\n[AircraftTypes]\n[VehicleTypes]\n0=TANK\n1=SHIP\n\
         [BuildingTypes]\n0=YARD\n1=PLAIN\n2=WEAP\n3=SYRD\n\
         [YARD]\nStrength=1000\nConstructionYard=yes\nFactory=BuildingType\n\
         [PLAIN]\nStrength=1000\nCost=100\n\
         [WEAP]\nStrength=1000\nFactory=UnitType\nCost=200\n\
         [SYRD]\nStrength=1000\nFactory=UnitType\nNaval=yes\nCost=200\n\
         [TANK]\nStrength=300\nSpeed=5\nCost=100\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
         [SHIP]\nStrength=300\nSpeed=5\nCost=100\nNaval=yes\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
         [Clear]\nBuildable=yes\n",
    );
    let weap = f
        .sim
        .spawn_object("WEAP", "AIHouse", 8, 8, 0, &f.rules)
        .unwrap();
    let syrd = f
        .sim
        .spawn_object("SYRD", "AIHouse", 8, 14, 0, &f.rules)
        .unwrap();
    let tank = f
        .rules
        .type_array_index(ObjectCategory::Vehicle, "TANK")
        .expect("TANK is a vehicle type");
    let ship = f
        .rules
        .type_array_index(ObjectCategory::Vehicle, "SHIP")
        .expect("SHIP is a vehicle type");
    {
        let house = f.sim.houses.get_mut(&f.owner).unwrap();
        house.ai_unit_choices_mut().set(UnitChoiceKind::Unit, tank);
        house
            .ai_unit_choices_mut()
            .set(UnitChoiceKind::NavalUnit, ship);
    }
    let held = |f: &Fixture, building: u64| -> Option<String> {
        f.sim
            .production
            .factories()
            .building_factory(building)
            .and_then(|factory| factory.object())
            .and_then(|object| f.sim.object_type(object.type_id, &f.rules))
            .map(|kind| kind.id.clone())
    };

    factory_ai(
        &mut f.sim,
        &f.rules,
        weap,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    factory_ai(
        &mut f.sim,
        &f.rules,
        syrd,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    assert_eq!(
        held(&f, weap).as_deref(),
        Some("TANK"),
        "the war factory builds the land channel"
    );
    assert_eq!(
        held(&f, syrd).as_deref(),
        Some("SHIP"),
        "the shipyard builds the naval channel"
    );
    abandon_production(
        &mut f.sim,
        &f.rules,
        FactoryHolder::Building(syrd),
        f.owner,
        crate::sim::world::FrameEffects::default(),
    );
    let choices = f.sim.houses[&f.owner].ai_unit_choices();
    assert_eq!(
        choices.get(UnitChoiceKind::NavalUnit),
        -1,
        "abandon clears the naval choice"
    );
    assert_eq!(
        choices.get(UnitChoiceKind::Unit),
        tank,
        "land choice survives"
    );
}

/// The switch's unit counters split on `UnitType+0xCCE` (the `Naval=` byte),
/// so a ground factory's tank does not fill the naval yard's counter — NPSE
/// #333 fixed exactly the global switch stopping the AI's navy. Two naval
/// yards still serialize, so the split is not a blanket disable. Each
/// producer reads its own choice slot, as `0xC2F130` routes them.
#[cfg(feature = "npatch")]
#[test]
fn disable_parallel_ai_queues_keeps_naval_and_ground_counters_apart() {
    use crate::rules::object_type::ObjectCategory;
    use crate::sim::ai_unit_choice::UnitChoiceKind;

    let mut f = fixture_with(&format!(
        "[General]\nDisableParallelAIQueues=yes\nPlacementDelay=.05\n\
         AIAlternateProductionCreditCutoff=10\n{NAVAL_RULES_TAIL}"
    ));
    let weap = f
        .sim
        .spawn_object("WEAP", "AIHouse", 8, 8, 0, &f.rules)
        .unwrap();
    let syrd_1 = f
        .sim
        .spawn_object("SYRD", "AIHouse", 8, 14, 0, &f.rules)
        .unwrap();
    let syrd_2 = f
        .sim
        .spawn_object("SYRD", "AIHouse", 8, 20, 0, &f.rules)
        .unwrap();
    let tank = f
        .rules
        .type_array_index(ObjectCategory::Vehicle, "TANK")
        .expect("TANK is a vehicle type");
    let ship = f
        .rules
        .type_array_index(ObjectCategory::Vehicle, "SHIP")
        .expect("SHIP is a vehicle type");
    let set = |f: &mut Fixture, index: i32| {
        f.sim
            .houses
            .get_mut(&f.owner)
            .unwrap()
            .ai_unit_choices_mut()
            .set(UnitChoiceKind::Unit, index);
    };

    set(&mut f, tank);
    ai_limit_visit(&mut f, weap);
    assert!(
        ai_limit_holds(&f, weap),
        "the ground factory starts its tank"
    );
    f.sim
        .houses
        .get_mut(&f.owner)
        .unwrap()
        .ai_unit_choices_mut()
        .set(UnitChoiceKind::NavalUnit, ship);
    ai_limit_visit(&mut f, syrd_1);
    assert!(
        ai_limit_holds(&f, syrd_1),
        "a naval yard is not blocked by the ground unit's counter"
    );
    ai_limit_visit(&mut f, syrd_2);
    assert!(
        !ai_limit_holds(&f, syrd_2),
        "two naval yards still serialize"
    );
}

/// Extension keys cannot change production in a build without NPatch.
#[cfg(not(feature = "npatch"))]
#[test]
fn npse_switches_are_ignored_without_the_build_capability() {
    let (mut f, weap_1, weap_2) = ai_limit_fixture("DisableParallelAIQueues=yes", "");
    assert!(!f.rules.general.disable_parallel_ai_queues);
    ai_limit_visit(&mut f, weap_1);
    ai_limit_visit(&mut f, weap_2);
    assert!(ai_limit_holds(&f, weap_1));
    assert!(
        ai_limit_holds(&f, weap_2),
        "NPSE keys leave stock parallel production intact"
    );
}

/// NPSE 2.0 C3395E releases before the caller drops the Factory's held
/// pointer. A scan still sees that pointer and incorrectly blocks a start.
#[cfg(feature = "npatch")]
#[test]
fn parallel_reservation_releases_at_exit_before_factory_cleanup() {
    let (mut f, first, second) = ai_limit_fixture("DisableParallelAIQueues=yes", "");
    ai_limit_visit(&mut f, first);
    let product = f
        .sim
        .production
        .factories()
        .building_factory(first)
        .unwrap()
        .object()
        .unwrap()
        .entity_id
        .unwrap();
    exit_produced_object(
        &mut f.sim,
        &f.rules,
        first,
        product,
        (8, 9),
        None,
        crate::sim::world::FrameEffects::default(),
    );
    assert!(
        ai_limit_holds(&f, first),
        "caller has not dropped the held pointer"
    );
    f.sim
        .houses
        .get_mut(&f.owner)
        .unwrap()
        .ai_unit_choices_mut()
        .set(UnitChoiceKind::Unit, 0);
    ai_limit_visit(&mut f, second);
    assert!(
        ai_limit_holds(&f, second),
        "exit already released the reservation"
    );
}

/// PointerExpired4CA580 clears only the held pointer; it does not run the
/// C2EFB4 Abandon release. The remaining reservation must save and hash.
#[cfg(feature = "npatch")]
#[test]
fn parallel_reservation_survives_product_expiry_and_state_roundtrip() {
    let (mut f, first, second) = ai_limit_fixture("DisableParallelAIQueues=yes", "");
    ai_limit_visit(&mut f, first);
    let product = f
        .sim
        .production
        .factories()
        .building_factory(first)
        .unwrap()
        .object()
        .unwrap()
        .entity_id
        .unwrap();
    assert!(f.sim.discard_constructed_limbo(
        product,
        &f.rules,
        crate::sim::world::FrameEffects::default()
    ));
    assert!(!ai_limit_holds(&f, first));
    ai_limit_visit(&mut f, second);
    assert!(
        !ai_limit_holds(&f, second),
        "pointer expiry does not release the reservation"
    );
    crate::sim::production::validate_restored_factory_state(&f.sim, &f.rules).unwrap();
    let saved = serde_json::to_string(f.sim.houses[&f.owner].ai_production()).unwrap();
    let state_hash = f.sim.state_hash();
    {
        let state = f.sim.houses.get_mut(&f.owner).unwrap().ai_production_mut();
        state.release_parallel(ObjectCategory::Vehicle, false);
        assert!(state.reserve_parallel(ObjectCategory::Infantry, false));
    }
    assert_ne!(
        f.sim.state_hash(),
        state_hash,
        "different reserved classes hash differently"
    );
    *f.sim.houses.get_mut(&f.owner).unwrap().ai_production_mut() = Default::default();
    assert_ne!(
        f.sim.state_hash(),
        state_hash,
        "reservation participates in the state hash"
    );
    *f.sim.houses.get_mut(&f.owner).unwrap().ai_production_mut() =
        serde_json::from_str(&saved).unwrap();
    assert_eq!(f.sim.state_hash(), state_hash);
    ai_limit_visit(&mut f, second);
    assert!(
        !ai_limit_holds(&f, second),
        "restored reservation still blocks"
    );
}

/// NPSE 2.0 C2F0FB..C2F10F uses one Building counter, regardless of Naval.
#[cfg(feature = "npatch")]
#[test]
fn parallel_building_reservation_does_not_split_on_naval() {
    let ini = RULES
        .replace("[General]\n", "[General]\nDisableParallelAIQueues=yes\n")
        .replace("1=PLAIN\n", "1=PLAIN\n2=DOCK\n")
        + "\n[DOCK]\nNaval=yes\nStrength=1000\nCost=100\n";
    let mut f = fixture_with(&ini);
    let first = f.yard;
    let second = f
        .sim
        .spawn_object("YARD", "AIHouse", 8, 8, 0, &f.rules)
        .unwrap();
    let dock = f.rules.building_type_index("DOCK").unwrap();
    f.sim
        .houses
        .get_mut(&f.owner)
        .unwrap()
        .ai_production_mut()
        .set_for_test(0, dock, true);
    ai_limit_visit(&mut f, first);
    assert!(ai_limit_holds(&f, first));
    let plain = f.rules.building_type_index("PLAIN").unwrap();
    f.sim
        .houses
        .get_mut(&f.owner)
        .unwrap()
        .ai_production_mut()
        .set_for_test(0, plain, true);
    ai_limit_visit(&mut f, second);
    assert!(
        !ai_limit_holds(&f, second),
        "naval and land buildings share the counter"
    );
}
