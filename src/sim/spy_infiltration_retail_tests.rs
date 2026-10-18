//! Stock Spy blackout through real AI-paid Building production.
//! Native4572CB/50BC90, House4F844B/508C30 and rate-only4CA6E0;
//! numeric/cadence controls: tools/spatial_oracle/house_power_consumers.
//! The loader/ordinary command/runtime supply the world history. Observed
//! input timing is not a native arrival, production or whole-world golden.

use crate::headless_scenario::{SIM_TICK_MS, battle_session, load_session};
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::production::Factory;
use crate::sim::spy_infiltration::InfiltrationEffect;
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation, TickLane};
use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition, LaunchTeam};

fn paid_building_head(sim: &Simulation, owner: crate::sim::intern::InternedId) -> Option<Factory> {
    sim.production
        .factories
        .iter_insertion_ordered()
        .into_iter()
        .find(|factory| {
            factory.owner == owner
                && factory.category == crate::sim::production::ProductionCategory::Building
                && factory.object.is_some()
        })
        .cloned()
}

/// Whole4CA6E0 changes only+38. The selected world inputs admit non-due
/// Factory visits at entry and expiry, so the entire remaining snapshot,
/// including private latch/identity fields, must survive those frames.
fn assert_rate_only(before: &Factory, after: &Factory) {
    let mut expected = before.clone();
    expected.step_rate_frames = after.step_rate_frames;
    assert_eq!(*after, expected);
}

#[test]
#[ignore = "slow; stock Spy blackout compares two 1600-frame histories"]
fn stock_spy_blackout_preserves_paid_building_timers_through_recovery() {
    let Some((root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let directory = crate::map::source::test_support::TestDirectory::new("spy-power-production");
    let path = directory.path().join("spy-power-production.map");
    let mut map =
        String::from_utf8(assets.resolve_ref("XMP03T4.MAP").unwrap().bytes.to_vec()).unwrap();
    // Only ordinary map actors are authored. Type/rules/AI/cash/timer state
    // remains the real Battle load's responsibility.
    for (section, records) in [
        (
            "Infantry",
            "PowerSpy=<human player>,SPY,256,30,89,0,Guard,0,None,0,0,0,1,1\n",
        ),
        (
            "Structures",
            "PowerBarracks=Computer,NAHAND,256,31,85,0,None,0,0,1,0,0,0,None,None,None,0\n\
             PowerPlant=Computer,NAPOWR,256,31,89,0,None,0,0,1,0,0,0,None,None,None,0\n",
        ),
    ] {
        let marker = format!("[{section}]");
        if let Some(start) = map.find(&marker) {
            let line_end = start + map[start..].find('\n').unwrap() + 1;
            map.insert_str(line_end, records);
        } else {
            map.push_str(&format!("\n{marker}\n{records}"));
        }
    }
    std::fs::write(&path, map).unwrap();
    let name = path.to_str().unwrap();
    let mut histories = Vec::new();
    for infiltrate in [false, true] {
        let mut launch = battle_session(name, &[(LaunchCountry::Russia, AiDifficulty::Easy)]);
        launch.local.country = LaunchCountry::America;
        launch.local.start_position = LaunchStartPosition::Position(0);
        launch.local.team = LaunchTeam::None;
        launch.opponents[0].start_position = LaunchStartPosition::Position(1);
        launch.opponents[0].team = LaunchTeam::None;
        launch.options.starting_credits = 5000;
        launch.options.bases = true;
        launch.options.unit_count = 0;
        launch.options.tech_level = 10;
        launch.options.short_game = false;
        launch.options.crates = false;
        let mut scene = load_session(&root, name, 0x1234_5678, launch).unwrap();
        let rules = &scene.runtime.resources.rules;
        assert_eq!(rules.general.spy_power_blackout, 1000);
        let plant_type = rules.object("NAPOWR").unwrap();
        assert!(plant_type.power > 0 && plant_type.spyable && !plant_type.radar);
        let plant_cost = plant_type.cost;
        let sim = scene.sim();
        let initial_hash = sim.state_hash();
        let local = sim.session.current_house.unwrap();
        let computer = sim.scenario_house_named("Computer").unwrap();
        let actor = |kind: &str, owner, cell| {
            sim.entities()
                .values()
                .find(|entity| {
                    entity.owner() == owner
                        && sim.interner.resolve(entity.type_ref()) == kind
                        && (entity.position.rx(), entity.position.ry()) == cell
                })
                .unwrap()
                .stable_id()
        };
        let spy = actor("SPY", local, (30, 89));
        let plant = actor("NAPOWR", computer, (31, 89));
        let plant_health = sim.entities().get(plant).unwrap().health;
        assert!(sim.infiltrator_building_action(spy, plant, rules));
        let mut first_product = None;
        let mut first_delivery = None;
        let mut entry = None;
        let mut recovery = None;
        let mut recovery_product = None;
        let mut recovery_delivery = None;
        let mut power_events = 0;
        for _ in 0..1600 {
            let sim = scene.sim();
            let native_frame = sim.session.binary_frame;
            let before = paid_building_head(sim, computer);
            let before_power = sim.power_states[&computer].blackout_remaining(native_frame);
            if first_product.is_none()
                && let Some(factory) = before.as_ref()
                && factory.progress > 0
                && factory.progress < crate::sim::production::PRODUCTION_STEPS
            {
                let object = factory.object.as_ref().unwrap();
                assert_eq!(sim.interner.resolve(object.type_id), "NAPOWR");
                let id = object
                    .entity_id
                    .expect("AI constructed the actual paid head");
                assert!(sim.entities().get(id).unwrap().in_limbo());
                assert!(!factory.on_hold && !factory.suspended);
                assert!(sim.houses[&computer].economy.spent_credits() > 0);
                assert!(factory.balance > 0 && factory.balance < plant_cost);
                first_product = Some(id);
                if infiltrate {
                    let execute_tick = sim.session.tick + 1;
                    scene.runtime.simulation.queue_command(CommandEnvelope::new(
                        local,
                        execute_tick,
                        Command::CaptureBuilding {
                            engineer_id: spy,
                            target_building_id: plant,
                        },
                    ));
                }
            }
            let commands = scene.runtime.simulation.take_due_commands();
            let frame = scene
                .runtime
                .advance_frame(
                    &commands,
                    SIM_TICK_MS,
                    TickLane::Ordinary,
                    FrameEffects::default(),
                )
                .unwrap();
            power_events += frame.sound_events.iter().filter(|event| matches!(event,
                SimSoundEvent::BuildingInfiltrated { owner, infiltrator, effect: InfiltrationEffect::Power, .. }
                    if *owner == computer && *infiltrator == local
            )).count();
            let sim = scene.sim();
            let power = &sim.power_states[&computer];
            let remaining = power.blackout_remaining(sim.session.binary_frame);
            let after = paid_building_head(sim, computer);
            if entry.is_none() && remaining > 0 {
                assert!(infiltrate);
                let before = before.as_ref().expect("paid head exists before entry");
                let after = after.as_ref().expect("same paid head remains after entry");
                // This fixture's real arrival is not due; no copied charge
                // algorithm or manually seeded countdown supplies this prior.
                assert!(before.step_timer.remaining(native_frame as i32) > 0);
                assert_rate_only(before, after);
                assert!(after.step_rate_frames > before.step_rate_frames);
                let timer = power.blackout_timer_for_test();
                assert_eq!(timer.start_frame(), native_frame as i32);
                assert_eq!(timer.duration(), 1000);
                assert!(sim.entities().get(spy).is_none());
                entry = Some(sim.session.binary_frame);
            }
            if entry.is_some() && recovery.is_none() {
                if remaining > 0 {
                    assert_eq!(power.total_output, 0);
                    assert!(power.is_low_power);
                } else {
                    assert_eq!(before_power, 1, "House expiry occurs at remaining one");
                    let before = before.as_ref().expect("real paid successor spans expiry");
                    let after = after.as_ref().expect("same successor remains after expiry");
                    assert!(before.progress > 0 && before.balance > 0);
                    assert!(before.step_timer.remaining(native_frame as i32) > 0);
                    assert_rate_only(before, after);
                    assert!(after.step_rate_frames < before.step_rate_frames);
                    let timer = power.blackout_timer_for_test();
                    assert_eq!(
                        (timer.start_frame(), timer.duration()),
                        (native_frame as i32, 0)
                    );
                    recovery_product = after.object.as_ref().unwrap().entity_id;
                    assert!(recovery_product.is_some());
                    recovery = Some(sim.session.binary_frame);
                }
            }
            for (product, delivered) in [
                (first_product, &mut first_delivery),
                (recovery_product, &mut recovery_delivery),
            ] {
                if delivered.is_none()
                    && product.is_some_and(|id| {
                        sim.entities().get(id).is_some_and(|entity| {
                            entity.owner() == computer
                                && !entity.in_limbo()
                                && entity.lifecycle.object_alive()
                                && entity.building_art.actually_placed()
                        })
                    })
                {
                    *delivered = Some(sim.session.binary_frame);
                }
            }
        }
        assert!(
            first_product.is_some(),
            "AI must genuinely pay for the first NAPOWR"
        );
        assert!(
            first_delivery.is_some(),
            "same paid NAPOWR must be placed normally"
        );
        assert_eq!(power_events, usize::from(infiltrate));
        assert_eq!(entry.is_some(), infiltrate);
        assert_eq!(recovery.is_some(), infiltrate);
        assert_eq!(recovery_delivery.is_some(), infiltrate);
        if infiltrate {
            assert!(recovery_delivery.unwrap() > recovery.unwrap());
        }
        let sim = scene.sim();
        let power = &sim.power_states[&computer];
        assert!(power.total_output > power.total_drain && !power.is_low_power);
        assert_eq!(sim.entities().get(spy).is_none(), infiltrate);
        let plant = sim.entities().get(plant).unwrap();
        assert_eq!((plant.owner(), plant.health), (computer, plant_health));
        histories.push((initial_hash, first_product, first_delivery));
    }
    assert_eq!(histories[0].0, histories[1].0, "same actual loaded prior");
    assert_eq!(
        histories[0].1, histories[1].1,
        "same genuinely constructed first head"
    );
    assert!(
        histories[1].2 > histories[0].2,
        "real blackout delays its paid delivery"
    );
}
