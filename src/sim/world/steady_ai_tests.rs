//! Steady objects change no result: a tank and an infantryman crossing the
//! map, and twins with the switch off, stay in the same state and play the
//! same sounds, frame by frame.

use super::{FrameEffects, Simulation, TickLane};
use crate::map::entities::{EntityCategory, MapEntity};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::scenario_session::ScenarioGameMode;

const TANK: u64 = 1;
const GI: u64 = 2;

fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[General]\n[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=MTNK\n[AircraftTypes]\n\
         [BuildingTypes]\n\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nROT=5\nTurret=yes\nSight=6\n\
         [E1]\nStrength=125\nArmor=none\nSpeed=4\nSight=5\n",
    ))
    .expect("steady object test rules parse")
}

fn placed(type_id: &str, category: EntityCategory, cell_y: u16) -> MapEntity {
    MapEntity {
        owner: "Americans".to_string(),
        type_id: type_id.to_string(),
        health: 256,
        cell_x: 4,
        cell_y,
        facing: 64,
        category,
        sub_cell: 0,
        veterancy: Some(0),
        high: false,
        mission: None,
        trigger_tag: None,
        recruitable_a: true,
        recruitable_b: true,
        group: -1,
        follows: None,
        structure_columns: Default::default(),
        structure_ai_sellable: false,
        structure_ai_repairable: false,
        structure_online: true,
    }
}

/// A skirmish with a tank and an infantryman ordered across the map,
/// steady visits counted or not.
fn movers(rules: &RuleSet, steady: bool) -> Simulation {
    let mut sim = Simulation::with_seed(0x51EE_9000_0003);
    sim.session_mut_for_test()
        .set_game_mode(ScenarioGameMode::Skirmish);
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    sim.spawn_from_map(
        &[
            placed("MTNK", EntityCategory::Unit, 10),
            placed("E1", EntityCategory::Infantry, 14),
        ],
        Some(rules),
    );
    sim.sleep_idle_objects(steady);
    let americans = sim.interner.get("Americans").unwrap();
    for (id, target_ry) in [(TANK, 10), (GI, 14)] {
        sim.queue_command(CommandEnvelope::new(
            americans,
            sim.session.tick() + 1,
            Command::Move {
                entity_id: id,
                target_rx: 26,
                target_ry,
            },
        ));
    }
    sim
}

/// One frame; what it leaves: the state hash, both objects and the sounds.
fn frame(sim: &mut Simulation, rules: &RuleSet) -> (u64, String, String) {
    let commands = sim.take_due_commands();
    let output = sim
        .advance_app_frame(
            &commands,
            rules,
            33,
            TickLane::Ordinary,
            &mut FrameEffects::default(),
        )
        .expect("fixture frame completes");
    let objects =
        [TANK, GI].map(|id| serde_json::to_value(sim.substrate.entities.get(id)).unwrap());
    (
        sim.state_hash(),
        serde_json::to_string(&objects).unwrap(),
        format!("{:?}", output.sound_events),
    )
}

/// Both movers go steady between their timers, and each thing a counted
/// visit would miss makes it run: the mission and scan timers on the way and
/// after arriving, a hand-out (the tank damaged, then repaired, whose
/// estimated health then recovers on its frames), a stage clock left
/// running, and the client's pending voices. The twins match frame by frame.
#[test]
fn steady_movers_match_their_twins_frame_by_frame() {
    let rules = rules();
    let mut steady = movers(&rules, true);
    let mut full = movers(&rules, false);
    for step in 0..360 {
        for sim in [&mut steady, &mut full] {
            let now = sim.session.binary_frame() as i32;
            match step {
                40 => sim
                    .substrate
                    .entities
                    .get_mut(TANK)
                    .unwrap()
                    .health
                    .set(200),
                70 => sim
                    .substrate
                    .entities
                    .get_mut(TANK)
                    .unwrap()
                    .health
                    .set(300),
                200 => sim
                    .substrate
                    .entities
                    .get_mut(GI)
                    .unwrap()
                    .restart_native_stage(0, now, 3),
                // The client names the objects whose voice it waits on before
                // each frame (the frame forgets them).
                230..233 => sim.prepare_unit_voice_visits([TANK, GI].into()),
                _ => {}
            }
        }
        if step == 40 {
            assert!(!steady.steady(TANK), "a mutable hand-out ends the mark");
        }
        if step == 230 {
            assert!(steady.steady(TANK) && steady.steady(GI), "both are steady");
        }
        let counted = steady.steady_ai_visits();
        assert_eq!(
            frame(&mut steady, &rules),
            frame(&mut full, &rules),
            "the twins differ after step {step}"
        );
        if (230..233).contains(&step) {
            assert_eq!(
                steady.steady_ai_visits(),
                counted,
                "a pending voice runs the visit"
            );
        }
    }
    let counted = steady.steady_ai_visits();
    assert!(counted > 300, "{counted} visits counted");
    assert_eq!(full.steady_ai_visits(), 0, "the switch counts none");
}

/// Check mode runs every visit a steady object would count and compares.
#[test]
fn every_visit_a_steady_object_would_count_changes_only_its_counters() {
    let rules = rules();
    let mut sim = movers(&rules, true);
    sim.check_idle_sleep(true);
    for _ in 0..360 {
        frame(&mut sim, &rules);
    }
    assert!(sim.steady_ai_visits() > 300);
}
