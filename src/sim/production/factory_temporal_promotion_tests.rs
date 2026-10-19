//! Issue1317's real cancel/promotion route. Native StartNext4CA60A calls
//! Begin4FA350(force1); offline fallback4FA45B supplies Resume4C9EA0(1).
//! PR1809 already ports that shared behavior. These stock mobile categories
//! protect it through the command/frame and actual Temporal owners.
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::production::{FactoryHolder, ProductionCategory};
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::world::{FrameEffects, Simulation, TickLane};

fn frame(runtime: &mut SimRuntime, owner: InternedId, commands: &[Command]) {
    let sim = &mut runtime.simulation;
    let next = sim.session.tick + 1;
    sim.queue_commands(
        commands
            .iter()
            .cloned()
            .map(|command| CommandEnvelope::new(owner, next, command)),
    );
    let due = sim.take_due_commands();
    runtime
        .advance_frame(&due, 67, TickLane::Ordinary, FrameEffects::default())
        .unwrap();
}

fn fixture(rules: &RuleSet, producer: &str, backup: bool) -> (SimRuntime, InternedId, u64, u64) {
    let mut sim = Simulation::with_seed(0x4FA4_3800);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false));
    sim.input_delay_ticks = 0;
    let owner = sim.interner.intern("Americans");
    let enemy = sim.interner.intern("Russians");
    for (id, human, cash) in [(owner, true, 10_000), (enemy, true, 0)] {
        let mut house = HouseState::new(id, 0, Some(id), human, cash, 10);
        house.project_country_mults(rules, &sim.interner);
        sim.houses.insert(id, house);
        sim.session.house_order.push(id);
    }
    sim.session.current_house = Some(owner);
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    if rules.object(producer).unwrap().naval {
        crate::sim::arena_fixture::flood(
            &mut sim,
            rules,
            32,
            (12..28).flat_map(|y| (16..30).map(move |x| (x, y))),
        );
    }
    for (name, x, y) in [
        ("GACNST", 5, 5),
        ("GAPOWR", 12, 5),
        ("GAPOWR", 12, 8),
        ("GATECH", 5, 12),
        ("GAPILE", 12, 12),
        ("GAAIRC", 5, 22),
    ] {
        if name != producer {
            sim.spawn_object(name, "Americans", x, y, 0, rules).unwrap();
        }
    }
    let factory = sim
        .spawn_object(producer, "Americans", 20, 14, 0, rules)
        .unwrap();
    if backup {
        sim.spawn_object(producer, "Americans", 20, 22, 0, rules)
            .unwrap();
    }
    let attacker = sim
        .spawn_object("CLEG", "Russians", 14, 20, 0, rules)
        .unwrap();
    (
        SimRuntime {
            simulation: sim,
            resources: SimResources {
                rules: rules.clone(),
                ..SimResources::empty()
            },
        },
        owner,
        factory,
        attacker,
    )
}

#[test]
fn retail_mobile_promotion_uses_offline_held_start_and_online_backup() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = retail.rules;
    let native = crate::test_fixture::json("tools/sidebar_oracle/temporal_vehicle_promotion.json");
    let weapon = rules
        .weapon(rules.object("CLEG").unwrap().primary().unwrap())
        .unwrap();
    assert!(
        rules
            .warhead(weapon.warhead.as_deref().unwrap())
            .unwrap()
            .temporal
    );
    for (producer, first, next, category) in [
        ("GAWEAP", "MTNK", "FV", ProductionCategory::Vehicle),
        ("GAPILE", "E1", "ENGINEER", ProductionCategory::Infantry),
        ("GAYARD", "DEST", "AEGIS", ProductionCategory::Ship),
        // Wildcard active-head cancellation is used for the two identical
        // ORCAs: indexed ABANDON would prefer the queued copy natively.
        ("GAAIRC", "ORCA", "ORCA", ProductionCategory::Aircraft),
    ] {
        for backup in [false, true] {
            let label = format!("{producer}/{first}->{next}/online-backup={backup}");
            let control = &native["controls"][usize::from(backup)];
            assert_eq!(control["online_backup"], backup);
            let native_phase = |name: &str| {
                &control["phases"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|phase| phase["name"] == name)
                    .unwrap()["after"]
            };
            let expected_promoted = &native_phase("cancel_mtnk_promote_fv")["factory"];
            let promotion = control["phases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|phase| phase["name"] == "cancel_mtnk_promote_fv")
                .unwrap();
            let calls = promotion["calls"].as_array().unwrap();
            let resume = calls
                .iter()
                .find(|call| call["name"] == "set_production_rate")
                .unwrap();
            // Original byte parameters ignore unrelated upper stack bytes.
            assert_eq!(
                resume["args"][0].as_u64().unwrap() & 255,
                u64::from(!backup)
            );
            let begin = calls
                .iter()
                .find(|call| call["name"] == "begin_production")
                .unwrap();
            assert_eq!(begin["args"][3].as_u64().unwrap() & 255, 1);

            let (mut runtime, owner, factory, attacker) = fixture(&rules, producer, backup);
            let first_id = runtime.simulation.interner.get(first).unwrap();
            let next_id = runtime.simulation.interner.get(next).unwrap();
            let holder = FactoryHolder::House(owner, category);
            frame(
                &mut runtime,
                owner,
                &[
                    Command::QueueProduction { type_id: first_id },
                    Command::QueueProduction { type_id: next_id },
                ],
            );
            for _ in 0..2000 {
                if runtime
                    .simulation
                    .production
                    .factories
                    .view(owner, category)
                    .unwrap()
                    .progress
                    > 0
                {
                    break;
                }
                frame(&mut runtime, owner, &[]);
            }
            let before = runtime
                .simulation
                .production
                .factories
                .view(owner, category)
                .unwrap();
            assert_eq!(before.object.unwrap().type_id, first_id, "{label}");
            assert_eq!(before.queue.len(), 1, "{label}");
            assert!(before.progress > 0 && before.progress < 54, "{label}");
            assert!(
                crate::sim::production::production_tech::strip_keeps(
                    &runtime.simulation,
                    &rules,
                    owner,
                    rules.object(next).unwrap()
                ),
                "{label}: supplied retail base admits the successor before Temporal starts"
            );
            let old_entity = before.object.unwrap().entity_id.unwrap();
            let factory_order = before.construction_order();
            runtime.simulation.temporal_initiate_warp(
                attacker,
                Some(factory),
                &rules,
                None,
                FrameEffects::default(),
            );
            assert!(
                !runtime
                    .simulation
                    .entities()
                    .get(factory)
                    .unwrap()
                    .building_online(),
                "{label}"
            );
            assert!(
                !runtime
                    .simulation
                    .production
                    .factories
                    .view(owner, category)
                    .unwrap()
                    .suspended,
                "warp alone has no native UpdateFactoryQueue callback: {label}"
            );
            if first == next {
                // Native wildcard Abandon is also the successful mobile
                // delivery caller's queue-promotion boundary, not an input
                // command exposed by the sidebar's indexed cancellation.
                assert!(crate::sim::production::cancel_production_for_owner(
                    &mut runtime.simulation,
                    &rules,
                    "Americans",
                    crate::sim::production::ProductionCancellation::Category(category),
                    false,
                    FrameEffects::default(),
                ));
                frame(&mut runtime, owner, &[]);
            } else {
                frame(
                    &mut runtime,
                    owner,
                    &[Command::CancelProductionByType {
                        type_id: first_id,
                        all: false,
                    }],
                );
            }
            let promoted = runtime
                .simulation
                .production
                .factories
                .view(owner, category)
                .unwrap_or_else(|| {
                    panic!(
                        "{label}: promoted Factory missing; retained commands={:?}",
                        runtime.simulation.pending_command_snapshot()
                    )
                });
            assert_eq!(promoted.object.unwrap().type_id, next_id, "{label}");
            assert!(promoted.queue.is_empty(), "{label}");
            assert_eq!(
                promoted.construction_order(),
                factory_order,
                "{label}: retained Factory"
            );
            let successor = promoted.object.unwrap().entity_id.unwrap();
            assert_ne!(old_entity, successor, "{label}");
            assert!(
                !runtime.simulation.entities().contains(old_entity),
                "{label}: old head disposed"
            );
            assert_eq!(
                promoted.suspended,
                expected_promoted["suspended"].as_bool().unwrap(),
                "{label}: online-factory decision"
            );
            if backup {
                for _ in 0..1000 {
                    let view = runtime
                        .simulation
                        .production
                        .factories
                        .view(owner, category)
                        .unwrap();
                    if view.progress > 0 {
                        break;
                    }
                    frame(&mut runtime, owner, &[]);
                }
                assert!(
                    runtime
                        .simulation
                        .production
                        .factories
                        .view(owner, category)
                        .unwrap()
                        .progress
                        > 0,
                    "{label}: online backup advances"
                );
                frame(
                    &mut runtime,
                    owner,
                    &[Command::QueueProduction { type_id: next_id }],
                );
                let queued = runtime
                    .simulation
                    .production
                    .factories
                    .view(owner, category)
                    .unwrap();
                assert_eq!(
                    queued.object.unwrap().entity_id,
                    Some(successor),
                    "{label}: running head retained"
                );
                assert_eq!(
                    queued.queue.len(),
                    native_phase("resume_same_fv")["queue"]
                        .as_array()
                        .unwrap()
                        .len(),
                    "{label}: a running cameo click queues, rather than resumes"
                );
                continue;
            }
            let held = &runtime.simulation.production.factories.factories[&holder];
            assert_eq!(held.progress, 0, "{label}");
            assert_eq!(held.step_timer.duration(), 0, "{label}");
            assert!(held.auto_resume_blocked, "Resume1's native latch: {label}");
            if category == ProductionCategory::Vehicle {
                assert_eq!(
                    held.progress as u64,
                    expected_promoted["stage"].as_u64().unwrap()
                );
                assert_eq!(
                    held.balance as i64,
                    expected_promoted["balance"].as_i64().unwrap()
                );
                assert_eq!(
                    held.step_timer.duration() as i64,
                    expected_promoted["timer_duration"].as_i64().unwrap()
                );
                assert_eq!(
                    held.auto_resume_blocked,
                    expected_promoted["latch"].as_bool().unwrap()
                );
                assert_eq!(
                    runtime.simulation.houses[&owner].economy.credits() as i64,
                    expected_promoted["credits"].as_i64().unwrap()
                );
            }
            let balance = held.balance;
            let wallet = runtime.simulation.houses[&owner].economy.clone();
            for _ in 0..8 {
                frame(&mut runtime, owner, &[]);
            }
            let held = &runtime.simulation.production.factories.factories[&holder];
            assert_eq!(
                (held.progress, held.balance),
                (0, balance),
                "{label}: frozen progress/debt"
            );
            assert_eq!(
                runtime.simulation.houses[&owner].economy, wallet,
                "{label}: frozen spend"
            );
            runtime.simulation.temporal_let_go(attacker);
            for _ in 0..3 {
                frame(&mut runtime, owner, &[]);
            }
            assert!(
                runtime
                    .simulation
                    .entities()
                    .get(factory)
                    .unwrap()
                    .building_online(),
                "{label}"
            );
            assert!(
                runtime
                    .simulation
                    .production
                    .factories
                    .view(owner, category)
                    .unwrap()
                    .suspended,
                "release alone does not resume: {label}"
            );
            super::super::super::update_factory_queue(
                &mut runtime.simulation,
                &rules,
                owner,
                category,
                FrameEffects::default(),
            );
            assert!(
                runtime
                    .simulation
                    .production
                    .factories
                    .view(owner, category)
                    .unwrap()
                    .suspended,
                "forced Resume1 latch survives a later callback: {label}"
            );
            frame(
                &mut runtime,
                owner,
                &[Command::QueueProduction { type_id: next_id }],
            );
            let resumed = runtime
                .simulation
                .production
                .factories
                .view(owner, category)
                .unwrap();
            assert_eq!(
                resumed.suspended,
                native_phase("resume_same_fv")["factory"]["suspended"]
                    .as_bool()
                    .unwrap(),
                "{label}: same-head PRODUCE resumes"
            );
            assert_eq!(
                resumed.object.unwrap().entity_id,
                Some(successor),
                "{label}: no replacement construction"
            );
            assert!(
                resumed.queue.is_empty(),
                "{label}: resume does not append another copy"
            );
            for _ in 0..1000 {
                if runtime
                    .simulation
                    .production
                    .factories
                    .view(owner, category)
                    .unwrap()
                    .progress
                    > 0
                {
                    break;
                }
                frame(&mut runtime, owner, &[]);
            }
            assert!(
                runtime
                    .simulation
                    .production
                    .factories
                    .view(owner, category)
                    .unwrap()
                    .progress
                    > 0,
                "{label}: resumed payment advances"
            );
        }
    }
}
