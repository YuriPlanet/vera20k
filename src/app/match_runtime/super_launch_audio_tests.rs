//! Launch audio through existing client sound owners. Headless production
//! frames join provider/charge/command admission and callback delivery.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::components::DriveCoord;
use crate::sim::world::{FrameEffects, SoundBoundary, TickLane};
use std::cell::{Cell, RefCell};

#[test]
fn activation_prefix_keeps_selection_and_ready_until_the_local_launch_tail() {
    use crate::app::types::TargetingMode;
    for kind in [
        "ForceShield",
        "GeneticConverter",
        "PsychicReveal",
        "PsychicDominator",
    ] {
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[SuperWeaponTypes]\n0=Gate\n[Gate]\nType={kind}\nStartSound=Cue\n\
             [AudioVisual]\nGeneticMutatorActivateSound=Cue\nPsychicRevealActivateSound=Cue\nPsychicDominatorActivateSound=Cue\n"
        ))).unwrap();
        let mut sim = Simulation::new();
        let owner = sim.intern("Local");
        let sw_type = sim.intern("Gate");
        let activation = SimSoundEvent::SuperWeaponActivation {
            sw_type,
            coordinate: DriveCoord {
                x: 128,
                y: 128,
                z: 416,
            },
        };
        let tail = SimSoundEvent::SuperWeaponLaunched {
            owner,
            sw_type,
            rx: 0,
            ry: 0,
        };
        let mut selected = Some(TargetingMode::SuperWeapon("Gate".to_owned()));
        super::super::super_selection::follow_selection_writes(
            &mut selected,
            &[activation.clone()],
            &sim,
            &rules,
            Some("Local"),
        );
        assert!(
            selected.is_some(),
            "{kind}: activation has no selection write"
        );
        let mut queue = SoundEventQueue::new();
        dispatch_sim_sound_events(
            [activation],
            &sim,
            &rules,
            Some("Local"),
            &mut |_| true,
            &mut queue,
        );
        assert!(
            queue
                .drain()
                .iter()
                .all(|event| !matches!(event, GameSoundEvent::EvaRemove { .. }))
        );
        super::super::super_selection::follow_selection_writes(
            &mut selected,
            &[tail.clone()],
            &sim,
            &rules,
            Some("Local"),
        );
        assert!(
            selected.is_none(),
            "{kind}: native common tail finishes selection"
        );
        dispatch_sim_sound_events(
            [tail],
            &sim,
            &rules,
            Some("Local"),
            &mut |_| true,
            &mut queue,
        );
        let emitted = queue.drain();
        assert!(
            matches!(emitted.as_slice(), [GameSoundEvent::EvaRemove { .. }]),
            "{kind}: tail is silent except Ready removal"
        );
    }
}

#[derive(Default)]
struct ActivationBoundary {
    delivered: Cell<usize>,
    target: Option<u64>,
    /// Coordinate, radar already in prefix, tail already in prefix, target HP.
    observations: RefCell<Vec<(DriveCoord, bool, bool, Option<i32>)>>,
    played: RefCell<Vec<GameSoundEvent>>,
}

impl SoundBoundary for ActivationBoundary {
    fn consume(&self, sim: &Simulation, rules: &RuleSet, events: &[SimSoundEvent]) {
        let fresh = &events[self.delivered.get()..];
        for event in fresh {
            if let SimSoundEvent::SuperWeaponActivation { coordinate, .. } = event {
                self.observations.borrow_mut().push((
                    *coordinate,
                    fresh
                        .iter()
                        .any(|event| matches!(event, SimSoundEvent::RadarEvent { .. })),
                    fresh
                        .iter()
                        .any(|event| matches!(event, SimSoundEvent::SuperWeaponLaunched { .. })),
                    self.target
                        .and_then(|id| sim.entities().get(id))
                        .map(|entity| entity.health.current()),
                ));
            }
        }
        let mut queue = SoundEventQueue::new();
        dispatch_sim_sound_events(
            fresh.iter().cloned(),
            sim,
            rules,
            Some("Americans"),
            &mut |_| true,
            &mut queue,
        );
        self.played.borrow_mut().extend(queue.drain());
        self.delivered.set(events.len());
    }

    fn delivered(&self) -> usize {
        self.delivered.get()
    }
}

fn frame(
    sim: &mut Simulation,
    rules: &RuleSet,
    effects: FrameEffects<'_>,
) -> crate::sim::world::SimFrameOutput {
    let due = sim.take_due_commands();
    sim.advance_app_frame(&due, rules, None, 67, TickLane::Ordinary, effects)
        .expect("ordinary skirmish frame")
}

/// Physical Battle rules in a supplied healthy skirmish base. Construction
/// yards keep both players alive; real provider/power/charge AI admits the
/// semantic command. Only idle countdown and art frame counts are supplied.
#[test]
fn retail_launch_audio_captures_terrain_and_delivers_before_genetic_callbacks() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::scenario_session::ScenarioGameMode;
    use crate::sim::superweapon::SuperWeaponInstance;

    for (name, provider, cue, deck_sound) in [
        ("ForceShieldSpecial", "GATECH", "ForceShieldStarting", true),
        (
            "GeneticConverterSpecial",
            "YAGNTC",
            "GeneticMutatorActivate",
            true,
        ),
        (
            "PsychicRevealSpecial",
            "NAPSIS",
            "PsychicRevealActivate",
            false,
        ),
        (
            "PsychicDominatorSpecial",
            "YAPPET",
            "PsychicDominatorActivate",
            false,
        ),
    ] {
        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        let mut rules = retail.rules;
        let names = [
            rules.general.force_shield_invoke_anim.clone(),
            rules.general.ion_blast_anim.clone(),
            rules.general.dominator_first_anim.clone(),
            rules.general.dominator_second_anim.clone(),
            "GENDEATH".to_owned(),
        ];
        for anim in names {
            rules.bind_anim_frame_count_for_test(&anim, 20);
        }
        let mut sim = crate::sim::arena_fixture::rival_arena_with_size(&rules, 73, 64);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.session_mut_for_test()
            .set_game_mode(ScenarioGameMode::Skirmish);
        sim.session_mut_for_test()
            .game_options_mut_for_test()
            .super_weapons = true;
        sim.set_input_delay_ticks(0);
        let owner = sim.intern("Americans");
        let other = sim.intern("Russians");
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(64, 64);
        sim.fog_mut_for_test().reveal_all_for_owner(owner);
        for house in [owner, other] {
            for entry in &rules.super_weapon_order {
                let id = sim.intern(entry);
                sim.super_weapons_mut_for_test()
                    .entry(house)
                    .or_default()
                    .insert(id, SuperWeaponInstance::new(id, house, 0));
            }
        }
        for (kind, house, x, y) in [
            ("GACNST", "Americans", 5, 5),
            ("NACNST", "Russians", 5, 40),
            ("NANRCT", "Americans", 10, 5),
            (provider, "Americans", 20, 5),
        ] {
            sim.spawn_object_at_height(kind, house, x, y, 0, 0, &rules)
                .expect("healthy stock base");
        }
        for _ in 0..8 {
            frame(&mut sim, &rules, FrameEffects::default());
        }
        let sw_type = sim.intern(name);
        let instance = &sim.super_weapons()[&owner][&sw_type];
        assert!(
            instance.is_active() && !instance.is_suspended(),
            "{name}: real powered provider"
        );
        let deadline = instance
            .charge_start_tick()
            .wrapping_add(instance.charge_duration())
            .wrapping_add(1);
        sim.session_mut_for_test()
            .set_binary_frame_for_test(deadline as u32);
        frame(&mut sim, &rules, FrameEffects::default());
        assert!(
            sim.super_weapons()[&owner][&sw_type].is_ready(),
            "{name}: real ready edge"
        );
        let target = (40, 40);
        // The Genetic callback control uses the ground list. Other controls
        // carry a structural deck so the no-deck reveal/Dominator distinction
        // and the Force Shield deck are both exercised.
        let bridge = name != "GeneticConverterSpecial";
        let terrain = sim.resolved_terrain_mut_for_test().as_mut().unwrap();
        let cell = terrain.cell_mut_for_test(target.0, target.1).unwrap();
        cell.level = 2;
        cell.template_height = 2;
        if bridge {
            cell.bridge_facts.raw_flags |= crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
        }
        let victim = (name == "GeneticConverterSpecial").then(|| {
            sim.spawn_object_at_height("E1", "Americans", target.0, target.1, 0, 2, &rules)
                .expect("real mutation receiver")
        });
        let boundary = ActivationBoundary {
            target: victim,
            ..Default::default()
        };
        sim.queue_command(CommandEnvelope::new(
            owner,
            sim.session().tick() + 1,
            Command::LaunchSuperWeapon {
                sw_type_id: sw_type,
                target_rx: target.0,
                target_ry: target.1,
            },
        ));
        let result = frame(&mut sim, &rules, FrameEffects::for_sound(&boundary));
        let expected = DriveCoord {
            x: 10368,
            y: 10368,
            z: 208 + if bridge && deck_sound { 416 } else { 0 },
        };
        let observations = boundary.observations.borrow();
        assert_eq!(observations.len(), 1, "{name}: reached native cue point");
        assert_eq!(observations[0].0, expected, "{name}");
        assert!(
            !observations[0].2,
            "{name}: activation delivered before Ready tail"
        );
        if let Some(victim) = victim {
            assert!(!observations[0].1, "Genetic EVA/sound precedes radar");
            assert!(
                observations[0].3.unwrap() > 0,
                "activation delivered before fatal mutation"
            );
            assert!(
                sim.entities()
                    .get(victim)
                    .is_none_or(|entity| !entity.is_alive()),
                "mutation callback actually ran"
            );
        }
        drop(observations);
        // Complete the normal frame's unconsumed suffix once. The recorder
        // represents the same borrowed boundary as the client; no cue repeats.
        assert_eq!(result.sound_events_delivered, boundary.delivered());
        let mut suffix = SoundEventQueue::new();
        dispatch_sim_sound_events(
            result
                .sound_events
                .iter()
                .skip(result.sound_events_delivered)
                .cloned(),
            &sim,
            &rules,
            Some("Americans"),
            &mut |_| true,
            &mut suffix,
        );
        boundary.played.borrow_mut().extend(suffix.drain());
        let played = boundary.played.borrow();
        assert_eq!(
            played
                .iter()
                .filter(|event| event.sound_id() == cue)
                .count(),
            1,
            "{name}: one activation sound"
        );
        let activation = played.iter().find(|event| event.sound_id() == cue).unwrap();
        if name == "ForceShieldSpecial" {
            assert!(matches!(activation, GameSoundEvent::VocAt { .. }));
        } else {
            assert!(
                matches!(activation,GameSoundEvent::VocAtCoord { coordinate,flags:0,.. }
            if *coordinate == [expected.x,expected.y,expected.z])
            );
        }
        let ready = launch_drops_ready_line(rules.super_weapon(name).unwrap().kind).unwrap();
        assert_eq!(
            played
                .iter()
                .filter(|event| matches!(event,GameSoundEvent::EvaRemove { event } if event==ready))
                .count(),
            1
        );
        drop(played);
        // Mutating the map after producer emission cannot move a captured cue.
        let cell = sim
            .resolved_terrain_mut_for_test()
            .as_mut()
            .unwrap()
            .cell_mut_for_test(40, 40)
            .unwrap();
        cell.level = 0;
        cell.template_height = 0;
        cell.bridge_facts.raw_flags = 0;
        let mut replay = SoundEventQueue::new();
        dispatch_sim_sound_events(
            result.sound_events,
            &sim,
            &rules,
            Some("Americans"),
            &mut |_| true,
            &mut replay,
        );
        let replay = replay.drain();
        let source = replay
            .iter()
            .find(|event| event.sound_id() == cue)
            .unwrap()
            .source()
            .unwrap();
        assert_eq!(
            source,
            coordinate_sound_source(expected),
            "{name}: immutable launch coordinate"
        );
    }
}
