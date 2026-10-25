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

/// Building43FC34 updates +6B4 before the same visit's C4 expiry440358
/// and destructor Release43BD1A. PowerOn is a positional one-shot: Release
/// alone leaves it playing, whereas an earlier zero-gain update stops it.
#[test]
fn retail_patriot_updates_its_working_sound_before_c4_destructor_release() {
    use std::cell::{Cell, RefCell};

    use crate::sim::anim_class::AnimWorldCoord;
    use crate::sim::command::Command;
    use crate::sim::world::{FrameEffects, SoundBoundary, TickLane};

    struct Heard {
        owner: u64,
        delivered: Cell<usize>,
        calls: RefCell<Vec<(&'static str, Option<AnimWorldCoord>)>>,
    }
    impl SoundBoundary for Heard {
        fn consume(&self, _: &Simulation, _: &RuleSet, events: &[SimSoundEvent]) {
            for event in &events[self.delivered.get()..] {
                let kind = match event {
                    SimSoundEvent::ObjectSoundDetached { owner } if *owner == self.owner => {
                        "detach"
                    }
                    SimSoundEvent::AnimationStarted {
                        anim_id, sound_id, ..
                    } if *anim_id == self.owner && sound_id == "PowerOn" => "PowerOn",
                    SimSoundEvent::ObjectSoundReleased { owner } if *owner == self.owner => {
                        "release"
                    }
                    _ => continue,
                };
                self.calls.borrow_mut().push((kind, None));
            }
            self.delivered.set(events.len());
        }
        fn delivered(&self) -> usize {
            self.delivered.get()
        }
        fn update_looping_sound(&self, _: &Simulation, owner: u64, world: AnimWorldCoord) {
            if owner == self.owner {
                self.calls.borrow_mut().push(("position", Some(world)));
            }
        }
    }

    let Some(mut retail) =
        crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    let roots = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&retail.rules);
    retail
        .rules
        .bind_anim_class_assets(&roots, &assets, theater.extension, "TEMPERATE");
    retail
        .rules
        .bind_effect_assets(&assets, theater.extension, "TEMPERATE");
    let rules = retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x43fc34);
    assert!(sim.session.apply_in_game_speed(4));
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.spawn_object("GAPOWR", "Americans", 4, 4, 0, &rules)
        .unwrap();
    sim.spawn_object("NAPOWR", "Russians", 24, 24, 0, &rules)
        .unwrap();
    let patriot = sim
        .spawn_object("NASAM", "Americans", 8, 10, 0, &rules)
        .unwrap();
    let tanya = sim
        .spawn_object("TANY", "Americans", 7, 10, 0, &rules)
        .unwrap();
    let heard = Heard {
        owner: working_sound_owner(patriot),
        delivered: Cell::new(0),
        calls: RefCell::default(),
    };
    let started = (0..120).any(|_| {
        heard.delivered.set(0);
        heard.calls.borrow_mut().clear();
        let output = sim
            .advance_app_frame(
                &[],
                &rules,
                None,
                64,
                TickLane::Ordinary,
                FrameEffects::for_sound(&heard),
            )
            .unwrap();
        heard.consume(&sim, &rules, &output.sound_events);
        heard
            .calls
            .borrow()
            .iter()
            .any(|(kind, _)| *kind == "PowerOn")
    });
    assert!(started, "the powered Patriot starts its real working cue");
    assert_eq!(
        heard.calls.borrow().as_slice(),
        [("detach", None), ("PowerOn", None)],
        "the changed operational edge skips the positional-update branch"
    );
    let started_frame = sim.session.binary_frame();
    let world = Simulation::movement_sound_world(sim.entities().get(patriot).unwrap());
    assert!(sim.apply_command(
        "Americans",
        &Command::PlantC4 {
            attacker_id: tanya,
            target_building_id: patriot
        },
        Some(&rules),
    ));
    let mut planted = false;
    let released = (0..180).any(|_| {
        heard.delivered.set(0);
        heard.calls.borrow_mut().clear();
        let output = sim
            .advance_app_frame(
                &[],
                &rules,
                None,
                64,
                TickLane::Ordinary,
                FrameEffects::for_sound(&heard),
            )
            .unwrap();
        // Destructors publish the undelivered suffix after their owner is
        // removed; consume it at the same boundary as the app.
        heard.consume(&sim, &rules, &output.sound_events);
        planted |= sim
            .entities()
            .get(patriot)
            .is_some_and(|entity| entity.pending_c4_detonation.pending().is_some());
        heard
            .calls
            .borrow()
            .iter()
            .any(|(kind, _)| *kind == "release")
    });
    assert!(planted, "Tanya's actual order arms the normal C4 fuse");
    assert!(
        released,
        "normal C4 expiry reaches the same-frame destructor"
    );
    assert!(
        sim.entities().get(patriot).is_none(),
        "the destructor removes the owner"
    );
    assert!(
        sim.session.binary_frame() - started_frame <= 45,
        "at selected GameSpeed4, 45 frames take 2880ms of PowerOn's 3044ms playout"
    );
    assert_eq!(
        heard.calls.borrow().as_slice(),
        [("position", Some(world)), ("release", None)]
    );
}

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
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
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
    sim.sound_events.clear_for_test();
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
    sim.sound_events.clear_for_test();
    sim.uninit_with_rules(id, &rules);
    assert!(
        working_events(&sim, id).is_empty(),
        "Limbo leaves +0x6B4 alone"
    );
    sim.process_pending_delete_with(
        &rules,
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
