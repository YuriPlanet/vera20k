//! [`super`] through the production object pass (`advance_tick`) on
//! production-spawned buildings, whose house power decides the operational
//! state. Control flow is instruction-level reading (see the module doc);
//! these are Rust behaviour tests, not native executions.

use super::{SimSoundEvent, Simulation, working_sound_owner};
use crate::map::resolved_terrain::test_flat_ground_grid;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::RulesLayerStack;
use crate::rules::ruleset::RuleSet;
use crate::rules::sound_ini::SoundRegistry;
use crate::sim::house_state::HouseState;

const TICK_MS: u32 = 67;

/// `PLANT` powers the house. `DEF` is a powered defence with both cues and
/// `HUM` a powered building with the working cue alone.
fn rules() -> RuleSet {
    let base = "Strength=1000\nArmor=concrete\nFoundation=1x1\n\
                CanPassiveAquire=no\nHasStupidGuardMode=no\n";
    let powered = "Powered=yes\nPower=-50\n";
    let layers = RulesLayerStack::new(crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[BuildingTypes]\n0=PLANT\n1=DEF\n2=HUM\n\
         [PLANT]\n{base}Power=200\n\
         [DEF]\n{base}{powered}WorkingSound=PowerOn\nNotWorkingSound=PowerOff\n\
         [HUM]\n{base}{powered}WorkingSound=OilDerrickLoop\n"
    )));
    let processed = layers.process().unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    let sounds = SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n0=PowerOn\n1=PowerOff\n2=OilDerrickLoop\n\
         [PowerOn]\nSounds=gpowon\n[PowerOff]\nSounds=gpowof\n\
         [OilDerrickLoop]\nSounds=boil1\nControl=loop random all decay attack\n",
    ));
    rules.bind_type_sound_references(processed.ini(), &sounds);
    rules
}

/// A world whose `Americans` House owns power state, so a building's removal
/// reaches the power recount (`invalidate_house_power`).
fn world(size: u16) -> Simulation {
    let mut sim = Simulation::new();
    sim.install_resolved_terrain_for_new_map(test_flat_ground_grid(size));
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 10_000, 10));
    sim
}

/// A powered house with `PLANT` and one building of `type_name`.
fn scene(rules: &RuleSet, type_name: &str) -> (Simulation, u64, u64) {
    let mut sim = world(16);
    let plant = sim
        .spawn_object("PLANT", "Americans", 3, 3, 0, rules)
        .expect("fixture plant");
    let id = sim
        .spawn_object(type_name, "Americans", 10, 10, 0, rules)
        .expect("fixture building");
    (sim, plant, id)
}

/// The ordered effects this building's `+0x6B4` handle received.
fn working_events(sim: &Simulation, id: u64) -> Vec<String> {
    let owner = working_sound_owner(id);
    sim.sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::ObjectSoundDetached { owner: held } if *held == owner => {
                Some("detach".to_string())
            }
            SimSoundEvent::AnimationStarted {
                anim_id, sound_id, ..
            } if *anim_id == owner => Some(format!("start {sound_id}")),
            SimSoundEvent::ObjectSoundReleased { owner: held } if *held == owner => {
                Some("release".to_string())
            }
            _ => None,
        })
        .collect()
}

fn tick(sim: &mut Simulation, rules: &RuleSet) {
    sim.sound_events.clear();
    sim.advance_tick(&[], Some(rules), None, None, TICK_MS);
}

/// Tick until the building's handle receives something, within `frames`.
fn next_edge(sim: &mut Simulation, rules: &RuleSet, id: u64, frames: usize) -> Vec<String> {
    for _ in 0..frames {
        tick(sim, rules);
        let events = working_events(sim, id);
        if !events.is_empty() {
            return events;
        }
    }
    Vec::new()
}

#[test]
fn a_working_cue_alone_only_detaches_on_the_off_edge() {
    let rules = rules();
    let (mut sim, plant, id) = scene(&rules, "HUM");
    assert_eq!(
        next_edge(&mut sim, &rules, id, 60),
        ["detach", "start OilDerrickLoop"]
    );
    sim.uninit_with_rules(plant, &rules);
    assert_eq!(next_edge(&mut sim, &rules, id, 60), ["detach"]);
}

#[test]
fn limbo_keeps_the_handle_and_the_destructor_releases_it() {
    let rules = rules();
    let (mut sim, _, id) = scene(&rules, "DEF");
    assert_eq!(
        next_edge(&mut sim, &rules, id, 60),
        ["detach", "start PowerOn"]
    );
    sim.sound_events.clear();
    sim.uninit_with_rules(id, &rules);
    assert!(
        working_events(&sim, id).is_empty(),
        "Limbo leaves +0x6B4 alone"
    );
    sim.process_pending_delete_with(
        Some(&rules),
        None,
        crate::sim::world::FrameEffects::default(),
        1,
        sim.session.started(),
    );
    assert_eq!(working_events(&sim, id), ["release"]);
    assert_eq!(sim.looping_sound_owner_coord(working_sound_owner(id)), None);
}

/// Retail `[NASAM]` (Patriot) through the production rules owner, powered by
/// a retail `GAPOWR`.
#[test]
fn a_retail_patriot_plays_its_power_cues() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = battle.rules;
    let mut sim = world(32);
    let plant = sim
        .spawn_object("GAPOWR", "Americans", 4, 4, 0, &rules)
        .expect("retail power plant");
    let id = sim
        .spawn_object("NASAM", "Americans", 14, 14, 0, &rules)
        .expect("retail Patriot");
    assert_eq!(
        next_edge(&mut sim, &rules, id, 300),
        ["detach", "start PowerOn"]
    );
    sim.uninit_with_rules(plant, &rules);
    assert_eq!(
        next_edge(&mut sim, &rules, id, 300),
        ["detach", "start PowerOff"]
    );
}
