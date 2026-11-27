use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::combat::{EntityDamageEvent, ReceiverCallFlags};
use crate::sim::components::NavTargetRef;
use crate::sim::mission::{MissionId, MissionType, authority::LiveReadyInputProvider};
use crate::sim::world::{FrameEffects, Simulation};

fn scene(landing: (u16, u16)) -> (Simulation, RuleSet, u64) {
    let art = IniFile::from_str(
        "[FIEND]\nSequence=FiendSequence\nCrawls=yes\n[FiendSequence]\n\
         Ready=0,1,1\nGuard=0,1,1\nWalk=0,2,2\nDown=0,1,1\nProne=0,1,1\nUp=0,1,1\n",
    );
    let (mut sim, rules, _) =
        crate::sim::world::entry_test_fixture::fixture_with_rules_and_fixed_art(
            "[InfantryTypes]\n2=FIEND\n[FIEND]\nDoggie=yes\nStrength=100\nSpeed=4\nSpeedType=Foot\n\
         Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
         [AudioVisual]\nConditionRed=25%\n[O102]\nLand=Tiberium\nNoUseTileLandType=yes\n",
            &art,
        );
    assert!(!rules.ares());
    assert!(rules.object("FIEND").unwrap().doggie);
    for (index, (name, human)) in [("Americans", true), ("Russians", false)]
        .into_iter()
        .enumerate()
    {
        let owner = sim.interner.intern(name);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner,
                index as u8,
                None,
                human,
                0,
                10,
            ));
        if human {
            sim.session.set_current_house_for_test(Some(owner));
        }
    }
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
    sim.session.game_options_mut_for_test().short_game = false;
    let registry = rules.overlay_registry();
    let ore_id = registry.id_for_name("O102").unwrap();
    let overlays = sim.overlay_grid.as_mut().unwrap();
    overlays.place_overlay(landing.0, landing.1, ore_id, 1);
    overlays.recalculate_runtime_cell(sim.resolved_terrain.as_mut().unwrap(), registry, landing);
    let dog = sim
        .spawn_object("FIEND", "Americans", 16, 15, 0, &rules)
        .unwrap();
    assert!(crate::sim::miner::ore_scan::cell_is_tiberium_land(
        &sim,
        Some(registry),
        landing
    ));
    (sim, rules, dog)
}

#[test]
fn builtin_doggie_lies_down_on_ore_then_gets_up_for_a_new_destination() {
    let (mut sim, rules, dog) = scene((16, 15));
    sim.spawn_object("ENGINEER", "Russians", 20, 15, 0, &rules)
        .unwrap();
    let now = sim.session.binary_frame();
    sim.mission_queue_exact(
        dog,
        MissionId::from_known(MissionType::Guard),
        0,
        now,
        &LiveReadyInputProvider { rules: &rules },
    )
    .unwrap();
    let _ = sim.substrate.mission_commence_exact(dog, now).unwrap();
    assert_eq!(
        sim.substrate
            .entities
            .get(dog)
            .unwrap()
            .mission
            .current()
            .known(),
        Some(MissionType::Guard)
    );
    let path = sim.path_grid.clone();
    for _ in 0..120 {
        let actor = sim.substrate.entities.get(dog).unwrap();
        if actor.infantry.as_ref().unwrap().is_prone()
            && actor.mission_leaf.as_infantry().unwrap().doing() != 5
        {
            break;
        }
        sim.advance_tick(
            &[],
            Some(&rules),
            path.as_deref(),
            Some(rules.overlay_registry()),
            67,
        );
    }
    let actor = sim.substrate.entities.get(dog).unwrap();
    assert_eq!(
        actor.body_facing.current(sim.session.binary_frame()),
        0x4000
    );
    assert!(actor.infantry.as_ref().unwrap().is_prone());

    sim.assign_destination_represented(
        dog,
        Some(NavTargetRef::cell(17, 15)),
        &rules,
        &mut FrameEffects::default(),
    )
    .unwrap();
    assert!(
        !sim.substrate
            .entities
            .get(dog)
            .unwrap()
            .infantry
            .as_ref()
            .unwrap()
            .is_prone()
    );
}

#[test]
fn builtin_doggie_reaches_max_panic_on_sourced_red_damage_and_searches_for_ore() {
    let (mut sim, rules, dog) = scene((17, 15));
    let source = sim
        .spawn_object("ENGINEER", "Russians", 18, 15, 0, &rules)
        .unwrap();
    let source_house = sim.substrate.entities.get(source).unwrap().owner();
    sim.substrate
        .entities
        .get_mut(dog)
        .unwrap()
        .health
        .set_for_test(26);
    let warhead = sim.interner.intern("SA");
    let mut effects = FrameEffects::default();
    sim.commit_direct_damage_receiver(
        &rules,
        Some(rules.overlay_registry()),
        EntityDamageEvent::direct_receiver(
            dog,
            1,
            0,
            source,
            Some(source_house),
            warhead,
            ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        ),
        &mut effects,
    );
    let actor = sim.substrate.entities.get(dog).unwrap();
    assert_eq!(actor.health.current(), 25);
    assert_eq!(actor.infantry.as_ref().unwrap().fear_level(), 300);

    sim.infantry_fear_turn(dog, &rules, Some(rules.overlay_registry()), &mut effects)
        .unwrap();
    let actor = sim.substrate.entities.get(dog).unwrap();
    assert_eq!(actor.navigation.nav_com(), Some(NavTargetRef::cell(17, 15)));
    assert_eq!(actor.mission.current().known(), Some(MissionType::Move));
    assert_eq!(actor.infantry.as_ref().unwrap().fear_level(), 299);
}
