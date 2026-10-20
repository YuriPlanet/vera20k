//! Consumed original509140/4CA5A0 paid-E2 boundaries, not whole-frame/UI parity.
//!
//! The native profile supplies a lone Temporal link before71B0EA, a completed
//! sale's Limbo callback, a later509140 callback and successful delivery before
//! CompletedProduction. Here the existing Temporal/lifecycle/production owners
//! execute those boundaries. Select/redraw/OutList bytes, the partial initial
//! +278-before+270 state and physical sale/capture/Exit remain separate evidence.
//! Native image identity and replay: tools/sidebar_oracle/factory_system_hold.md
//! and factory_system_hold.meta.json.

use super::{PRODUCTION_STEPS, ProductionCategory, native_e2_head};
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::production::{self, factory_lifecycle, lifecycle_tests};
use crate::sim::rng::{SimRng, trace_draws};
use crate::sim::runtime::SimRuntime;
use crate::sim::world::FrameEffects;
use serde_json::{Value, json};

const CATEGORY: ProductionCategory = ProductionCategory::Infantry;

#[test]
fn explicit_power_toggle_holds_and_resumes_the_existing_paid_factory_head() {
    let corpus = crate::test_fixture::json("tools/sidebar_oracle/factory_system_hold.json");
    let Some(mut fixture) = PaidHead::new(&corpus["lifecycle"][0], false, &corpus["type_inputs"])
    else {
        return;
    };
    let before = fixture.head();
    let sim = &mut fixture.runtime.simulation;
    let rules = &fixture.runtime.resources.rules;
    let rng = sim.rng_state();
    assert!(sim.go_building_offline(fixture.target, rules, FrameEffects::default()));
    assert!(
        !sim.production
            .factories()
            .view(fixture.owner, CATEGORY)
            .unwrap()
            .suspended
    );
    assert!(sim.go_building_offline(fixture.barracks, rules, FrameEffects::default()));
    let held = sim
        .production
        .factories()
        .view(fixture.owner, CATEGORY)
        .unwrap();
    assert!(held.suspended);
    assert_eq!(held.object.unwrap().entity_id, Some(fixture.original));
    let stage = held.progress;
    let queued: Vec<_> = held.queue.iter().map(|entry| entry.type_id).collect();
    assert!(sim.go_building_online(fixture.barracks, rules, FrameEffects::default()));
    let resumed = sim
        .production
        .factories()
        .view(fixture.owner, CATEGORY)
        .unwrap();
    assert!(!resumed.suspended);
    assert_eq!(resumed.object.unwrap().entity_id, Some(fixture.original));
    assert_eq!(resumed.progress, stage);
    assert_eq!(
        resumed
            .queue
            .iter()
            .map(|entry| entry.type_id)
            .collect::<Vec<_>>(),
        queued
    );
    assert_eq!(sim.rng_state(), rng);
    assert_eq!(fixture.head()["stage"], before["stage"]);
}

struct PaidHead {
    runtime: SimRuntime,
    owner: InternedId,
    type_id: InternedId,
    barracks: u64,
    target: u64,
    attacker: u64,
    original: u64,
    native_original: u64,
    native_type_index: i64,
    successor: Option<u64>,
    notice_executed: bool,
    frame_offset: u32,
    factory_order: u64,
    pending: Vec<CommandEnvelope>,
}

impl PaidHead {
    fn new(row: &Value, forced: bool, types: &Value) -> Option<Self> {
        // The native fixture declares a steady Russian base and wallet10000.
        // Readers, constructed held identity, FIFO, rates and payment execute;
        // no AFTER value is installed as a gameplay prerequisite.
        let (mut runtime, owner, barracks) =
            lifecycle_tests::retail_barracks_sale_runtime(!forced, false, 10_000)?;
        let rules = &runtime.resources.rules;
        assert_eq!(
            i64::from(rules.object("E2").unwrap().cost),
            types["E2"]["cost"].as_i64().unwrap()
        );
        let sim = &mut runtime.simulation;
        let other = sim.interner.intern("Opponent");
        let mut house = HouseState::new(other, 1, Some(owner), false, 0, 10);
        house.project_country_mults(rules, &sim.interner);
        sim.houses.register(house);
        let attacker = sim
            .spawn_object("CLEG", "Opponent", 12, 20, 0, rules)
            .expect("the stock CLEG prior constructs before the comparison");
        let target = if forced {
            barracks
        } else {
            sim.entities()
                .iter_sorted()
                .find(|(id, entity)| {
                    *id != barracks && sim.interner.resolve(entity.type_ref()) == "NAHAND"
                })
                .map(|(id, _)| id)
                .expect("the supplied second stock barracks")
        };
        // JoinedFixture's declared retained House power input. This is not an
        // assessment or an online/factory answer; the real rate owner reads it.
        let power = sim.power_states.entry(owner).or_default();
        power.set_total_output_for_test(99);
        power.set_total_drain_for_test(0);
        assert!(sim.recheck_house_tech_tree(owner, rules).is_empty());
        for _ in 0..2 {
            assert!(production::enqueue_by_type(
                sim,
                rules,
                "Russians",
                "E2",
                FrameEffects::default(),
                false,
            ));
        }
        // The native harness also visits only FactoryAI here. A whole frame
        // would publish readiness/deliver the old head before the forced seam.
        let target_stage = if forced { PRODUCTION_STEPS } else { 1 };
        for _ in 0..20_000 {
            if sim
                .production
                .factories()
                .view(owner, CATEGORY)
                .unwrap()
                .progress
                == target_stage
            {
                break;
            }
            sim.session
                .set_binary_frame_for_test(sim.session.binary_frame().wrapping_add(1));
            production::step_factories(sim);
        }
        let factory = sim.production.factories().view(owner, CATEGORY).unwrap();
        assert_eq!(factory.progress, target_stage);
        let original = factory.object.unwrap().entity_id.unwrap();
        let factory_order = factory.construction_order();
        let frame_offset = sim
            .session
            .binary_frame()
            .wrapping_sub(row["initial"]["frame"].as_u64().unwrap() as u32);
        let native_original = row["initial"]["factory"]["held_id"].as_u64().unwrap();
        let type_id = sim.interner.get("E2").unwrap();
        // All fixture constructors/payments precede this recorded boundary.
        // Streams are installed once and retained through every measured phase.
        sim.main_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["main"].as_str().unwrap())
                .into();
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["scenario"].as_str().unwrap());
        sim.mapgen_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["mapgen"].as_str().unwrap());
        Some(Self {
            runtime,
            owner,
            type_id,
            barracks,
            target,
            attacker,
            original,
            native_original,
            native_type_index: types["E2"]["index"].as_i64().unwrap(),
            successor: None,
            notice_executed: false,
            frame_offset,
            factory_order,
            pending: Vec::new(),
        })
    }

    fn head(&self) -> Value {
        let sim = &self.runtime.simulation;
        let mut value = native_e2_head(
            sim.production.factories(),
            &sim.houses,
            self.owner,
            self.type_id,
        );
        if let Some(id) = value["held_id"].as_u64() {
            value["held_id"] = if id == self.original {
                self.native_original.into()
            } else {
                assert_eq!(
                    Some(id),
                    self.successor,
                    "only the observed successor may replace the head"
                );
                (self.native_original + 1).into()
            };
        }
        let timer_start = value["timer_start"].as_i64().unwrap() as i32;
        value["timer_start"] = timer_start.wrapping_sub(self.frame_offset as i32).into();
        value
    }

    fn rng(&self) -> Value {
        let sim = &self.runtime.simulation;
        json!({
            "main": sim.main_rng.native_state_hex(),
            "scenario": sim.scenario_rng.native_state_hex(),
            "mapgen": sim.mapgen_rng.native_state_hex(),
        })
    }

    fn operation(&mut self, phase: &Value) {
        let sim = &mut self.runtime.simulation;
        let rules = &self.runtime.resources.rules;
        match phase["name"].as_str().unwrap() {
            "prior_user_right" | "same_head_left" => {
                assert!(self.pending.is_empty());
                let events = phase["events"].as_array().unwrap();
                assert_eq!(events.len(), 1);
                assert_eq!(events[0]["instance_kind"], 16);
                assert_eq!(events[0]["naval"], 0);
                assert_eq!(events[0]["index"].as_i64().unwrap(), self.native_type_index);
                let payload = match events[0]["kind"].as_u64().unwrap() {
                    15 => Command::SuspendProduction { category: CATEGORY },
                    14 => Command::QueueProduction {
                        type_id: self.type_id,
                    },
                    other => panic!("unmapped original local event {other}"),
                };
                // Core starts at the native event boundary. Actual Select and
                // cameo hit testing remain with the app's separate input test.
                self.pending.push(CommandEnvelope::new(
                    self.owner,
                    sim.session.tick(),
                    payload,
                ));
            }
            "prior_user_suspend_receive" | "same_head_resume_receive" => {
                assert_eq!(self.pending.len(), 1);
                let commands = std::mem::take(&mut self.pending);
                let (executed, spawned) = sim.apply_due_commands(
                    &commands,
                    Some(rules),
                    sim.session.tick(),
                    None,
                    FrameEffects::default(),
                );
                assert_eq!(executed, 1);
                assert!(!spawned);
            }
            "temporal_building_notice" => {
                sim.temporal_initiate_warp(
                    self.attacker,
                    Some(self.target),
                    rules,
                    None,
                    FrameEffects::default(),
                );
                self.notice_executed = true;
            }
            "warp_alone_factory_ai"
            | "offline_backup_factory_ai"
            | "release_alone_factory_ai"
            | "after_left_factory_ai"
            | "after_callback_factory_ai"
            | "promoted_factory_ai" => production::step_factories(sim),
            "begin_online_barracks_sale" => assert!(production::sell_back(
                sim,
                rules,
                self.barracks,
                production::SellOrder::Player,
                None,
                FrameEffects::default(),
            )),
            "online_barracks_limbo" => {
                sim.techno_limbo_with_rules(self.barracks, rules, None);
            }
            "original_temporal_release" => sim.temporal_let_go(self.attacker),
            "release_house_suffix" => {
                assert!(sim.recheck_house_tech_tree(self.owner, rules).is_empty())
            }
            "release_rate_refresh" => {
                production::refresh_factory_rates_for_house(sim, rules, self.owner)
            }
            "later_infantry_factory_callback" => production::update_factory_queue(
                sim,
                rules,
                self.owner,
                CATEGORY,
                FrameEffects::default(),
            ),
            "declared_successful_delivery_completion" => {
                assert!(sim.production.factories_mut().complete_production(
                    self.owner,
                    CATEGORY,
                    sim.session.binary_frame(),
                ))
            }
            "original_start_next_queued" => {
                let next_id = sim.next_stable_object_id();
                factory_lifecycle::start_next_queued(
                    sim,
                    rules,
                    self.owner,
                    CATEGORY,
                    FrameEffects::default(),
                );
                let successor = sim
                    .production
                    .factories()
                    .view(self.owner, CATEGORY)
                    .unwrap()
                    .object
                    .unwrap()
                    .entity_id
                    .unwrap();
                assert_eq!(
                    successor, next_id,
                    "the actual constructor allocates the successor"
                );
                assert_ne!(successor, self.original);
                assert!(self.successor.replace(successor).is_none());
            }
            "external_spend_wallet" => {
                let money = crate::sim::credit_income::available_money(sim, self.owner);
                crate::sim::credit_income::spend_money(&mut sim.houses, self.owner, money);
            }
            other => panic!("unconsumed original phase {other}"),
        }
    }

    fn compare(&mut self, row: &Value) -> usize {
        assert_eq!(
            self.head(),
            row["initial"]["factory"],
            "{} initial",
            row["name"]
        );
        assert_eq!(self.rng(), row["rng_before"]);
        let mut phases = 0;
        for phase in row["phases"].as_array().unwrap() {
            let label = format!("{} {}", row["name"], phase["name"]);
            self.runtime.simulation.session.set_binary_frame_for_test(
                (phase["before"]["frame"].as_u64().unwrap() as u32).wrapping_add(self.frame_offset),
            );
            assert_eq!(self.head(), phase["before"]["factory"], "{label} before");
            let before_rng = self.rng();
            let before_id = self.runtime.simulation.next_stable_object_id();
            let held_before = self
                .runtime
                .simulation
                .production
                .factories()
                .view(self.owner, CATEGORY)
                .unwrap()
                .object
                .and_then(|object| object.entity_id);
            let ((), draws) = trace_draws(|| self.operation(phase));
            assert_eq!(self.head(), phase["after"]["factory"], "{label} after");
            let sim = &self.runtime.simulation;
            let factory = sim
                .production
                .factories()
                .view(self.owner, CATEGORY)
                .unwrap();
            assert_eq!(
                factory.construction_order(),
                self.factory_order,
                "{label} Factory identity"
            );
            assert_eq!(phase["factory_identity_unchanged"], true);
            assert_eq!(
                factory.object.and_then(|object| object.entity_id) == held_before,
                phase["held_identity_unchanged"].as_bool().unwrap(),
                "{label} held identity",
            );
            assert_eq!(
                sim.next_stable_object_id() - before_id,
                phase["identity_delta"].as_u64().unwrap(),
                "{label} constructor identity count",
            );
            assert!(
                sim.entities().get(self.original).is_some(),
                "{label} no paid-head destruction"
            );
            assert_eq!(
                draws.len() as u64,
                phase["rng"]["raw_advance_count"].as_u64().unwrap(),
                "{label} actual draws"
            );
            if draws.is_empty() {
                assert_eq!(phase["rng"]["unchanged"], true);
                assert_eq!(
                    self.rng(),
                    before_rng,
                    "{label} all three streams unchanged"
                );
            } else {
                // Techno6F3254 calls Scenario RAW65C780;6F3259 stores AX at3C8.
                // The ordinary limbo constructor owns this draw; the adapter
                // never calls next_u32 or supplies the resulting word.
                assert_eq!(draws.len(), 1);
                let request = &phase["rng"]["requests"][0];
                assert_eq!(request["entry"], 0x65C780);
                assert_eq!(request["site"], 0x6F3254);
                assert_eq!(
                    draws[0]["value"], request["result"],
                    "{label} constructor raw result"
                );
                assert_eq!(before_rng["scenario"], request["state_before"]);
                let after_rng = self.rng();
                assert_eq!(after_rng["scenario"], request["state_after"]);
                assert_eq!(after_rng["main"], before_rng["main"]);
                assert_eq!(after_rng["mapgen"], before_rng["mapgen"]);
                assert_eq!(
                    u64::from(
                        sim.entities()
                            .get(self.successor.unwrap())
                            .unwrap()
                            .techno_ctor_random_word()
                    ),
                    request["result"].as_u64().unwrap() & 0xFFFF,
                    "{label} constructor retained word",
                );
            }
            // +278-before+270 is an explicitly supplied initial native prior.
            // From the real notice onward, compare the represented owner state.
            if self.notice_executed {
                let target = sim.entities().get(self.target).unwrap();
                assert_eq!(
                    target.building_online(),
                    phase["after"]["building_online"].as_bool().unwrap(),
                    "{label} online"
                );
                assert_eq!(
                    target.is_warped_out(),
                    phase["after"]["building_warped"].as_bool().unwrap(),
                    "{label} warped"
                );
                assert_eq!(
                    target.temporal.chain_head().is_some(),
                    phase["after"]["building_temporal_head"].as_bool().unwrap(),
                    "{label} head"
                );
                assert_eq!(
                    target.lifecycle.in_limbo(),
                    phase["after"]["building_limbo"].as_bool().unwrap(),
                    "{label} target Limbo"
                );
                assert_eq!(
                    sim.houses[&self.owner].tech_recheck_pending(),
                    phase["after"]["house_recheck"].as_u64().unwrap() != 0,
                    "{label} tech request"
                );
                assert_eq!(
                    sim.power_states[&self.owner].power_recheck_for_test(),
                    phase["after"]["house_power_dirty"].as_u64().unwrap() != 0,
                    "{label} power request"
                );
            }
            phases += 1;
        }
        assert!(self.pending.is_empty());
        assert_eq!(
            self.rng(),
            row["rng_after"],
            "{} complete final RNG buffers",
            row["name"]
        );
        phases
    }
}

#[test]
fn offline_backup_lifecycle_matches_original_paid_boundaries() {
    let corpus = crate::test_fixture::json("tools/sidebar_oracle/factory_system_hold.json");
    let rows = corpus["lifecycle"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    let mut phases = 0;
    for row in rows {
        let Some(mut fixture) = PaidHead::new(row, false, &corpus["type_inputs"]) else {
            return;
        };
        phases += fixture.compare(row);
    }
    assert_eq!(phases, 36);
}

#[test]
fn forced_offline_successor_matches_original_constructor_and_resume() {
    let corpus = crate::test_fixture::json("tools/sidebar_oracle/factory_system_hold.json");
    let rows = corpus["forced_promotion"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let mut phases = 0;
    for row in rows {
        let Some(mut fixture) = PaidHead::new(row, true, &corpus["type_inputs"]) else {
            return;
        };
        phases += fixture.compare(row);
    }
    assert_eq!(phases, 13);
}

#[path = "factory_temporal_promotion_tests.rs"]
mod temporal_promotion_tests;
