//! Stock Soviet recovery through the bound production runtime. Original bodies:
//! CanMakeMoney4F6540, CheckBuildNeed4FD9A0, ManageBuildQueue4FDD10 and
//! Strategy4FD500, gamemd SHA256
//! 1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c.
//! Instruction reading establishes the gates, sale/abandon and mode changes;
//! this regression exercises Rust, not an original executable or native timing.

use super::*;
use crate::headless_scenario::{HeadlessScenario, battle_session, load_session};
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::world::TickLane;
use crate::skirmish_launch::{
    AiDifficulty, LaunchCountry, LaunchStartPosition, SkirmishLaunchOptions,
};

struct ManageObservation(Option<u32>);

impl ManageObservation {
    fn start() -> Self {
        Self(MANAGE_CALLS.with(|calls| calls.replace(Some(0))))
    }

    fn count(&self) -> u32 {
        MANAGE_CALLS.with(|calls| calls.get().unwrap())
    }
}

impl Drop for ManageObservation {
    fn drop(&mut self) {
        MANAGE_CALLS.with(|calls| calls.set(self.0));
    }
}

fn gatherers(sim: &Simulation, rules: &RuleSet, owner: InternedId) -> Vec<u64> {
    sim.entities()
        .iter_sorted()
        .filter(|(_, entity)| {
            entity.owner() == owner
                && entity.is_active()
                && entity.is_alive()
                && !entity.lifecycle.in_limbo()
                && sim
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| object.resource_gatherer)
        })
        .map(|(id, _)| id)
        .collect()
}

fn stock_scene(root: &std::path::Path) -> HeadlessScenario {
    let mut launch = battle_session(
        "XMP03T4.MAP",
        &[(LaunchCountry::Russia, AiDifficulty::Easy)],
    );
    launch.player_name = "VERA-OBSERVER".to_owned();
    launch.local.country = LaunchCountry::Russia;
    launch.local.start_position = LaunchStartPosition::Position(0);
    launch.opponents[0].start_position = LaunchStartPosition::Position(1);
    launch.options = SkirmishLaunchOptions {
        starting_credits: 5000,
        unit_count: 6,
        tech_level: 10,
        game_speed: 1,
        default_ai_difficulty: 0,
        short_game: false,
        bases: true,
        bridges_destroyable: true,
        super_weapons: true,
        build_off_ally: true,
        crates: false,
        mcv_redeploy: true,
        fog_of_war: false,
        shroud: true,
        tiberium_grows: true,
        multi_engineer: false,
        harvester_truce: false,
        ally_change_allowed: true,
    };
    load_session(root, "XMP03T4.MAP", 0x1234_5678, launch).expect("stock Battle loads")
}

/// The scene supplies only lobby settings and ordinary local Move/Attack
/// envelopes. AI deployment, base plan, spending, free/held units, loss, sales,
/// destruction, replacement and income must come from their production owners.
/// The two histories share preparation; timings/cells are input choices, not
/// native output goldens. The 22 ms input matches the client capture cadence,
/// rather than headless_scenario's separately documented 66 ms default.
#[test]
#[ignore = "slow; stock AI recovery uses retail assets and two 10000-frame histories"]
fn stock_soviet_miner_loss_admits_recovery_and_returns_to_income() {
    let (root, _) = crate::rules::retail_ini_fixture::retail_assets()
        .expect("run with RA2_DIR and VERA20K_REQUIRE_RETAIL_ASSETS=1");

    for attack in [false, true] {
        let mut scene = stock_scene(&root);
        let sim = scene.sim();
        let local = sim.interner.get("VERA-OBSERVER").unwrap();
        let owner = sim.interner.get("Computer1").unwrap();
        let tanks: Vec<_> = sim
            .entities()
            .iter_sorted()
            .filter(|(_, entity)| {
                entity.owner() == local && sim.interner.resolve(entity.type_ref()) == "HTNK"
            })
            .map(|(id, _)| id)
            .collect();
        assert_eq!(tanks.len(), 3, "ordinary starting-force admission");
        let calls = ManageObservation::start();
        let mut original = None;
        let mut held = None;
        let mut war_factory = None;
        let mut lost = false;
        let mut recovered = None;
        let mut recovered_harvested = false;
        let mut recovered_unloaded = false;
        let mut recovered_income = false;
        let mut first_recovery = false;

        for step in 0..10_000 {
            let sim = scene.sim();
            let rules = &scene.runtime.resources.rules;
            let live = gatherers(sim, rules, owner);
            assert_eq!(
                sim.houses[&owner].tracking.resource_gatherers(),
                live.len() as i32,
                "committed-frame tracking consistency at {step}"
            );
            original = original.or_else(|| live.first().copied());
            let mut commands = Vec::new();
            if let Some(goals) = match step {
                0 => Some([(70, 41), (71, 41), (70, 42)]),
                3000 => Some([(70, 34), (69, 34), (69, 35)]),
                _ => None,
            } {
                commands.extend(tanks.iter().zip(goals).map(
                    |(&entity_id, (target_rx, target_ry))| {
                        CommandEnvelope::new(
                            local,
                            sim.session.tick,
                            Command::Move {
                                entity_id,
                                target_rx,
                                target_ry,
                            },
                        )
                    },
                ));
            }
            if step == 4910 {
                let original = original.expect("the AI generated a free miner");
                assert_eq!(live, [original], "one on-map gatherer before the attack");
                let house = &sim.houses[&owner];
                assert!(house.tracking.resource_destinations() > 0);
                assert!(house.tracking.war_factories() > 0);
                assert!(!house.build_const_order.is_empty());
                assert!(!house.base_plan.nodes.is_empty());
                assert_eq!(
                    house.difficulty_value(&rules.general.harvesters_per_refinery),
                    1,
                    "processed stock Easy input through the complete rules reader"
                );
                assert!(can_make_money(sim, rules, owner));
                let miner = sim.entities().get(original).unwrap();
                assert!(
                    sim.fog
                        .is_cell_visible(local, miner.position.rx(), miner.position.ry())
                );
                let owed = balance_owed(sim, rules, owner, |object| {
                    object.category == ObjectCategory::Vehicle && object.resource_gatherer
                })
                .expect("the earlier retained choice reached an actual Factory");
                assert!(
                    available_money(sim, owner) < owed,
                    "genuine poor held-production prior"
                );
                let held_candidates: Vec<_> = sim
                    .entities()
                    .iter_sorted()
                    .filter(|(_, entity)| {
                        entity.owner() == owner
                            && entity.lifecycle.in_limbo()
                            && sim
                                .object_type(entity.type_ref(), rules)
                                .is_some_and(|object| object.resource_gatherer)
                    })
                    .map(|(id, _)| id)
                    .collect();
                assert_eq!(held_candidates.len(), 1, "one actual limbo replacement");
                held = held_candidates.first().copied();
                war_factory = sim
                    .entities()
                    .iter_sorted()
                    .find(|(_, entity)| {
                        entity.owner() == owner
                            && !entity.lifecycle.in_limbo()
                            && sim
                                .object_type(entity.type_ref(), rules)
                                .is_some_and(|object| object.weapons_factory)
                    })
                    .map(|(id, _)| id);
                assert!(war_factory.is_some());
                if attack {
                    commands.extend(tanks.iter().map(|&attacker_id| {
                        CommandEnvelope::new(
                            local,
                            sim.session.tick,
                            Command::Attack {
                                attacker_id,
                                target_id: original,
                            },
                        )
                    }));
                }
            }

            let before_calls = calls.count();
            let admission = lost && !first_recovery && check_build_need(sim, rules, owner);
            let due = sim.houses[&owner]
                .strategy_timer
                .expired(sim.session.binary_frame as i32);
            let before_income = i64::from(sim.houses[&owner].economy.credits())
                + i64::from(sim.houses[&owner].economy.spent_credits());
            let carrying_before = recovered
                .and_then(|id| sim.entities().get(id))
                .and_then(|entity| entity.miner.as_ref())
                .is_some_and(|miner| !miner.cargo().is_empty());
            scene
                .runtime
                .advance_frame(&commands, 22, TickLane::Ordinary, FrameEffects::default())
                .expect("ordinary bound-resource frame");
            let sim = scene.sim();
            let rules = &scene.runtime.resources.rules;
            let live = gatherers(sim, rules, owner);
            if let Some(id) = original {
                if !live.contains(&id) {
                    assert!(attack && step > 4910, "healthy miner must keep earning");
                    lost = true;
                } else if !attack {
                    let miner = sim.entities().get(id).unwrap();
                    assert_eq!(
                        miner.health.current(),
                        sim.object_type(miner.type_ref(), rules).unwrap().strength
                    );
                }
            }
            if calls.count() > before_calls && !first_recovery {
                assert!(
                    attack && lost && admission && due,
                    "ordinary due Strategy admitted recovery"
                );
                assert!(
                    sim.entities().get(held.unwrap()).is_none(),
                    "held miner was abandoned"
                );
                let factory = sim.entities().get(war_factory.unwrap()).unwrap();
                assert_eq!(
                    factory.mission.current(),
                    MissionId::from_known(MissionType::Selling)
                );
                let house = &sim.houses[&owner];
                let harvester = EconomyTypes::of(sim, rules, house).harvester.unwrap();
                assert_eq!(house.ai_production.mode(), 2);
                assert_eq!(
                    Some(house.ai_unit_choices.get(UnitChoiceKind::Unit)),
                    rules.type_array_index(ObjectCategory::Vehicle, &harvester.id)
                );
                first_recovery = true;
            }
            if lost {
                recovered = recovered.or_else(|| live.first().copied());
                if let Some(miner) = recovered
                    .and_then(|id| sim.entities().get(id))
                    .and_then(|entity| entity.miner.as_ref())
                {
                    recovered_harvested |= miner.harvesting() && !miner.cargo().is_empty();
                    recovered_unloaded |= miner.unload_active();
                    let after_income = i64::from(sim.houses[&owner].economy.credits())
                        + i64::from(sim.houses[&owner].economy.spent_credits());
                    recovered_income |= recovered_unloaded
                        && carrying_before
                        && miner.cargo().is_empty()
                        && after_income > before_income;
                }
            }
        }
        if attack {
            assert!(lost && first_recovery && calls.count() > 0);
            assert_ne!(original, recovered, "a new production identity recovered");
            assert!(
                gatherers(scene.sim(), &scene.runtime.resources.rules, owner)
                    .contains(&recovered.unwrap()),
                "the recovered production identity remains on the map"
            );
            assert!(
                recovered_harvested && recovered_unloaded && recovered_income,
                "replacement must harvest, unload and deliver actual wallet income"
            );
        } else {
            assert!(!lost && original.is_some());
            assert_eq!(
                calls.count(),
                0,
                "healthy earning AI needs no emergency rebuild"
            );
        }
    }
}
