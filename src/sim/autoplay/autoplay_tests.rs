//! The actuation path on the retail rival arena: the host's gate and envelope,
//! the scripted policy's release schedule, and the observation reading the
//! owners it claims to read.

use crate::rules::ruleset::RuleSet;
use crate::sim::autoplay::host::AutoPlayHost;
use crate::sim::autoplay::scripted_policy::{ScriptStep, ScriptedPolicy};
use crate::sim::behavior::managed::{MicroKind, MicroParams, Stage};
use crate::sim::behavior::order::{ApplyOutcome, Refusal};
use crate::sim::behavior::{Funnel, Goal};
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::control::board::AutoPlayDomains;
use crate::sim::control::brain::{Brain, Outbox};
use crate::sim::control::external::{
    ActionSlot, ENTITY_SLOTS, GLOBAL_SLOTS, OBSERVATION_SCHEMA_VERSION, ObservationBatch,
    ObservationSchema, PolicySource, batch_for,
};
use crate::sim::control::observation::Observation;
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::production::ProductionCategory;
use crate::sim::strategy::assist::AssistBrain;
use crate::sim::world::Simulation;

/// The 32x32 arena with its two houses — `Americans` human, `Russians` a
/// computer house — and retail rules, needed to spawn real object types. None
/// without `RA2_DIR`, as the rest of the suite does.
fn arena(seed: u64) -> Option<(Simulation, RuleSet)> {
    let retail = crate::rules::retail_ini_fixture::retail_battle_rules()?;
    let rules = retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, seed);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    Some((sim, rules))
}

/// The same arena on synthetic rules, for what needs no object type: it runs
/// everywhere, so the gate and the observation stay covered without RA2.
fn synthetic_arena(seed: u64) -> (Simulation, RuleSet) {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(""))
        .expect("synthetic rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, seed);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    (sim, rules)
}

fn american(sim: &mut Simulation) -> crate::sim::intern::InternedId {
    sim.interner.intern("Americans")
}

/// A host for `Americans` playing `script`.
fn host(sim: &mut Simulation, script: Vec<(u64, Command)>) -> AutoPlayHost {
    AutoPlayHost::new(
        Box::new(ScriptedPolicy::from_pairs(script)),
        american(sim),
        usize::MAX,
    )
}

/// A host for `Americans` playing the assist brain.
fn retail_host(sim: &mut Simulation, seed: u64) -> AutoPlayHost {
    AutoPlayHost::new(Box::new(AssistBrain::new(seed)), american(sim), usize::MAX)
}

/// One scripted brain frame: the commands the script releases now.
fn script_frame(
    agent: &mut ScriptedPolicy,
    rules: &RuleSet,
    sim: &Simulation,
    owner: InternedId,
) -> Vec<Command> {
    let obs = Observation::new(sim, owner);
    let mut out = Outbox::default();
    agent.think(rules, &obs, &mut out);
    out.commands
}

/// One assist brain frame, as the commands the control layer would issue for
/// it.
///
/// The brain is read directly, so a test can inspect its plan and its memory
/// afterwards; admission is the host's business and is covered by the gate
/// tests and the end-to-end chain tests. Orders go through the same order
/// funnel the board uses, so a mid-transition object shows up as "no command"
/// here exactly as it would in play.
fn frame_commands(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    sim: &Simulation,
    owner: InternedId,
) -> Vec<Command> {
    let obs = Observation::new(sim, owner);
    let mut out = Outbox::default();
    brain.think(rules, &obs, &mut out);
    let mut commands: Vec<Command> = out.commands;
    for intent in out.intents {
        commands.push(intent.command());
    }
    let funnel = Funnel::new(sim, rules, owner);
    for order in &out.orders {
        if let Ok(command) = funnel.prepare(order.actor, order.goal) {
            commands.push(command);
        }
    }
    commands
}

/// A factory holding a finished product: the cameo a player's sidebar shows
/// as done, and the object the house has not taken out of it yet.
fn arm_finished(
    sim: &mut Simulation,
    owner: InternedId,
    category: ProductionCategory,
    type_name: &str,
) {
    let type_id = sim.interner.intern(type_name);
    let factories = sim.production_mut_for_test().factories_mut();
    assert!(
        factories.test_enqueue_kernel(owner, category, type_id, 1, 700),
        "the factory takes the product"
    );
    assert!(
        factories.test_arm_ready(owner, category),
        "the product is finished and still held"
    );
}

#[test]
fn the_board_takes_a_finished_product_out_of_its_factory() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let owner = american(&mut sim);
    arm_finished(&mut sim, owner, ProductionCategory::Vehicle, "MTNK");

    let mut host = host(&mut sim, vec![]);
    host.step(&mut sim, &rules);

    let queued = sim.pending_command_snapshot();
    assert!(
        queued.iter().any(|envelope| matches!(
            envelope.payload,
            Command::PlaceProducedMobile {
                category: ProductionCategory::Vehicle
            }
        )),
        "a finished product must be taken out, or its category stays busy for good"
    );
}

/// A house with a yard and a wallet: what the brain spends it on.
fn yard_with_money(sim: &mut Simulation, rules: &RuleSet, owner: InternedId, credits: i32) {
    sim.spawn_object("GACNST", "Americans", 16, 16, 64, rules)
        .expect("a construction yard");
    sim.houses
        .get_mut(&owner)
        .expect("the house")
        .economy_mut()
        .set_credits_for_test(credits);
}

#[test]
fn the_retail_brain_queues_what_the_base_needs_next() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let owner = american(&mut sim);
    yard_with_money(&mut sim, &rules, owner, 10_000);

    let mut brain = AssistBrain::new(0xA17);
    let commands = frame_commands(&mut brain, &rules, &sim, owner);

    assert!(
        commands
            .iter()
            .any(|command| matches!(command, Command::QueueProduction { .. })),
        "a yard that is free and a wallet that can pay must start something"
    );
}

#[test]
fn the_retail_brain_queues_nothing_it_cannot_pay_for() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let owner = american(&mut sim);
    yard_with_money(&mut sim, &rules, owner, 0);

    let mut brain = AssistBrain::new(0xA17);
    let commands = frame_commands(&mut brain, &rules, &sim, owner);

    assert!(
        !commands
            .iter()
            .any(|command| matches!(command, Command::QueueProduction { .. })),
        "an empty wallet must queue nothing: this house earns what it spends"
    );
}

/// The names of the types the brain asked a factory to start.
fn queued_names(sim: &Simulation, commands: &[Command]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::QueueProduction { type_id } => Some(sim.interner.resolve(*type_id).to_owned()),
            _ => None,
        })
        .collect()
}

/// A house past its opening with no yard: a war factory stands and the wallet
/// is full, which is all a lost yard leaves to rebuild from.
fn factory_with_money(sim: &mut Simulation, rules: &RuleSet, owner: InternedId, credits: i32) {
    sim.spawn_object("GAWEAP", "Americans", 12, 12, 64, rules)
        .expect("a war factory");
    sim.houses
        .get_mut(&owner)
        .expect("the house")
        .economy_mut()
        .set_credits_for_test(credits);
}

#[test]
fn a_house_that_lost_its_yard_buys_another_maker() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let owner = american(&mut sim);
    sim.spawn_object("GADEPT", "Americans", 16, 16, 64, &rules)
        .expect("a service depot");
    factory_with_money(&mut sim, &rules, owner, 20_000);

    let mut brain = AssistBrain::new(0xA17);
    let commands = frame_commands(&mut brain, &rules, &sim, owner);
    let queued = queued_names(&sim, &commands);

    assert!(
        queued.iter().any(|name| name == "AMCV"),
        "nothing the yard builds can start without a yard, so the factory's one \
         useful product is the MCV that carries DeploysInto= back to one; got {queued:?}"
    );
}

#[test]
fn a_maker_that_cannot_deploy_where_it_stands_is_driven_to_a_site() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let owner = american(&mut sim);
    // A ConYard's foundation is put down one cell north-west of the maker, so
    // a maker on the map's first cell has nowhere to put one: the deploy is
    // refused, and asking again changes nothing.
    let mcv = sim
        .spawn_object("AMCV", "Americans", 1, 1, 64, &rules)
        .expect("an MCV");
    factory_with_money(&mut sim, &rules, owner, 0);

    let mut brain = AssistBrain::new(0xA17);
    let commands = frame_commands(&mut brain, &rules, &sim, owner);

    assert!(
        commands
            .iter()
            .any(|command| matches!(command, Command::Move { entity_id, .. } if *entity_id == mcv)),
        "a blocked maker has to drive to a site the yard fits on before it deploys"
    );
    assert!(
        !commands.iter().any(
            |command| matches!(command, Command::DeployMcv { entity_id } if *entity_id == mcv)
        ),
        "and pressing D where the yard cannot stand is the refusal it already knows"
    );
}

#[test]
fn a_factory_with_no_yard_and_no_maker_starts_what_it_can() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let owner = american(&mut sim);
    // No depot, so no MCV is buildable: the factory must keep producing rather
    // than stand idle waiting for a type `CanBuild` refuses.
    factory_with_money(&mut sim, &rules, owner, 20_000);
    sim.spawn_object("GAREFN", "Americans", 8, 8, 64, &rules)
        .expect("a refinery");
    for cell in [(20u16, 20u16), (20, 22)] {
        sim.spawn_object("CMIN", "Americans", cell.0, cell.1, 64, &rules)
            .expect("a harvester for the bay");
    }

    let mut brain = AssistBrain::new(0xA17);
    let commands = frame_commands(&mut brain, &rules, &sim, owner);

    assert!(
        commands
            .iter()
            .any(|command| matches!(command, Command::QueueProduction { .. })),
        "a house with no yard still fights with what its factories make"
    );
}

/// The (name, price) of every type the brain asked a factory to start.
fn queued_with_price(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    commands: &[Command],
) -> Vec<(String, i32)> {
    let obs = Observation::new(sim, owner);
    commands
        .iter()
        .filter_map(|command| match command {
            Command::QueueProduction { type_id } => Some(*type_id),
            _ => None,
        })
        .filter_map(|type_id| {
            obs.object_type(rules, type_id)
                .map(|ty| (ty.id.clone(), obs.cost_of(rules, ty)))
        })
        .collect()
}

/// The band the wallet reaches for: under [`POOR_CREDITS`] the cheap band,
/// past [`RICH_CREDITS`] the dear one, and never a band the house cannot pay
/// for. The infantry line works at its own, lower floor so a thin wallet still
/// fields soldiers instead of saving for a tank it will not reach.
#[test]
fn the_wallet_decides_how_dear_a_band_the_house_buys() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let owner = american(&mut sim);
    sim.spawn_object("GAWEAP", "Americans", 12, 12, 64, &rules)
        .expect("a war factory");
    sim.spawn_object("GAREFN", "Americans", 8, 8, 64, &rules)
        .expect("a refinery");
    for cell in [(20u16, 20u16), (20, 22)] {
        sim.spawn_object("CMIN", "Americans", cell.0, cell.1, 64, &rules)
            .expect("a harvester for the bay");
    }

    let buy = |sim: &mut Simulation, credits: i32| {
        sim.houses
            .get_mut(&owner)
            .expect("the house")
            .economy_mut()
            .set_credits_for_test(credits);
        let mut brain = AssistBrain::new(0xA17);
        let commands = frame_commands(&mut brain, &rules, sim, owner);
        queued_with_price(sim, &rules, owner, &commands)
    };
    let thin = buy(&mut sim, 2_100);
    let thick = buy(&mut sim, 8_000);

    assert!(
        !thin.is_empty() && thin.iter().all(|(_, price)| *price <= 1_000),
        "a wallet below the rich ceiling buys the cheap and the line band, not the heavy end; \
         got {thin:?}"
    );
    let thin_dearest = thin.iter().map(|(_, price)| *price).max().unwrap_or(0);
    assert!(
        thick.iter().any(|(_, price)| *price >= thin_dearest),
        "a full wallet reaches at least as far up the price bands as a thin one; \
         got {thick:?} vs {thin:?}"
    );
}

#[test]
fn the_board_leaves_the_players_own_units_alone() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let tank = sim
        .spawn_object("MTNK", "Americans", 16, 16, 64, &rules)
        .expect("an American tank");
    let mut host = host(&mut sim, vec![(0, Command::Stop { entity_id: tank })]);
    host.take_back([tank].into_iter().collect());

    host.step(&mut sim, &rules);

    assert!(
        sim.pending_command_snapshot().is_empty(),
        "a unit in the player's control group is the player's to command"
    );
}

#[test]
fn grouped_only_drives_the_players_control_group_and_never_ungrouped_units() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let grouped = sim
        .spawn_object("MTNK", "Americans", 16, 16, 64, &rules)
        .expect("one grouped tank");
    let ungrouped = sim
        .spawn_object("MTNK", "Americans", 20, 20, 64, &rules)
        .expect("one ungrouped tank");
    let mut host = host(&mut sim, vec![
        (0, Command::Stop { entity_id: grouped }),
        (0, Command::Stop { entity_id: ungrouped }),
    ]);
    host.take_back([grouped].into_iter().collect());
    let mut domains = AutoPlayDomains::ALL;
    domains.grouped_only = true;
    host.set_domains(domains);
    let issued = host.step(&mut sim, &rules);
    assert_eq!(issued, vec![Command::Stop { entity_id: grouped }],
        "the fifth key only changes actor ownership, not domain switches");
}

/// The gate is the second line, not the first: a policy that only ever gets
/// refused spends the frame's budget on the player's units and then believes
/// for `ORDER_REFRESH_FRAMES` that it has sent them. It has to see them.
#[test]
fn the_policy_does_not_reach_for_a_unit_the_player_keeps() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let owner = american(&mut sim);
    let rifleman = sim
        .spawn_object("E1", "Americans", 16, 16, 64, &rules)
        .expect("an American rifleman");
    let kept: std::collections::BTreeSet<u64> = [rifleman].into_iter().collect();

    let mut free = Outbox::default();
    AssistBrain::new(0xA17).think(&rules, &Observation::new(&sim, owner), &mut free);
    let mut held = Outbox::default();
    AssistBrain::new(0xA17).think(
        &rules,
        &Observation::new(&sim, owner).with_player_groups(&kept),
        &mut held,
    );

    assert!(
        free.orders.iter().any(|order| order.actor == rifleman),
        "an idle rifleman nobody keeps is put on area guard: {:?}",
        free.orders
    );
    assert!(
        held.orders.iter().all(|order| order.actor != rifleman),
        "and the same rifleman in one of the player's groups is never proposed: {:?}",
        held.orders
    );
}

#[test]
fn the_opening_buys_power_or_income_before_anything_else() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let owner = american(&mut sim);
    yard_with_money(&mut sim, &rules, owner, 10_000);

    let mut brain = AssistBrain::new(0xA17);
    let commands = frame_commands(&mut brain, &rules, &sim, owner);
    let queued: Vec<String> = commands
        .iter()
        .filter_map(|command| match command {
            Command::QueueProduction { type_id } => Some(sim.interner.resolve(*type_id).to_owned()),
            _ => None,
        })
        .collect();

    assert!(!queued.is_empty(), "the opening starts something");
    for name in &queued {
        assert!(
            rules.build_power_types.iter().any(|power| power == name)
                || rules.build_refinery_types.iter().any(|bay| bay == name),
            "the opening buys power or income first, not {name}"
        );
    }
}

#[test]
fn host_queues_commands_with_the_house_owner_and_the_current_tick() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let tank = sim
        .spawn_object("MTNK", "Americans", 16, 16, 64, &rules)
        .expect("an American tank");
    let mut host = host(&mut sim, vec![(0, Command::Stop { entity_id: tank })]);

    host.step(&mut sim, &rules);

    let queued = sim.pending_command_snapshot();
    assert_eq!(queued.len(), 1, "one admitted command reaches the queue");
    assert_eq!(queued[0].owner, american(&mut sim));
    assert_eq!(queued[0].execute_tick, sim.session().tick());
    assert_eq!(queued[0].payload, Command::Stop { entity_id: tank });
    assert_eq!(host.rejected(), 0);
}

#[test]
fn gate_rejects_an_actor_the_house_does_not_own() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let theirs = sim
        .spawn_object("MTNK", "Russians", 18, 16, 64, &rules)
        .expect("a Russian tank");
    let mut host = host(&mut sim, vec![(0, Command::Stop { entity_id: theirs })]);

    host.step(&mut sim, &rules);

    assert!(
        sim.pending_command_snapshot().is_empty(),
        "an order onto another house's object never reaches the queue"
    );
    assert_eq!(host.rejected(), 1);
}

#[test]
fn gate_rejects_a_command_naming_no_live_actor() {
    let (mut sim, rules) = synthetic_arena(0xA17);
    let mut host = host(&mut sim, vec![(0, Command::Stop { entity_id: 0xDEAD })]);

    host.step(&mut sim, &rules);

    assert!(
        sim.pending_command_snapshot().is_empty(),
        "an order onto an id no object holds is refused"
    );
    assert_eq!(host.rejected(), 1);
}

#[test]
fn gate_passes_house_scoped_commands_without_an_actor() {
    let (mut sim, rules) = synthetic_arena(0xA17);
    let power_plant = sim.interner.intern("GAPOWR");
    let mut host = host(
        &mut sim,
        vec![(
            0,
            Command::QueueProduction {
                type_id: power_plant,
            },
        )],
    );

    host.step(&mut sim, &rules);

    assert_eq!(
        sim.pending_command_snapshot().len(),
        1,
        "production names no actor and is admitted"
    );
}

#[test]
fn host_issues_nothing_while_input_is_locked() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let tank = sim
        .spawn_object("MTNK", "Americans", 16, 16, 64, &rules)
        .expect("an American tank");
    let mut host = host(&mut sim, vec![(0, Command::Stop { entity_id: tank })]);
    sim.session.set_input_locked_for_test(true);

    host.step(&mut sim, &rules);

    assert!(
        sim.pending_command_snapshot().is_empty(),
        "a locked session takes no agent commands"
    );
}

#[test]
fn the_per_frame_cap_drops_the_overflow() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let tank = sim
        .spawn_object("MTNK", "Americans", 16, 16, 64, &rules)
        .expect("an American tank");
    let owner = american(&mut sim);
    let mut host = AutoPlayHost::new(
        Box::new(ScriptedPolicy::from_pairs([
            (0, Command::Stop { entity_id: tank }),
            (0, Command::Scatter { entity_id: tank }),
            (
                0,
                Command::Guard {
                    entity_id: tank,
                    target: None,
                },
            ),
        ])),
        owner,
        2,
    );

    host.step(&mut sim, &rules);

    assert_eq!(
        sim.pending_command_snapshot().len(),
        2,
        "the third command of the frame is dropped"
    );
    assert_eq!(host.rejected(), 1);
}

#[test]
fn the_two_switches_split_building_from_troops() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let tank = sim
        .spawn_object("MTNK", "Americans", 16, 16, 64, &rules)
        .expect("an American tank");
    let power = sim.interner.get("GAPOWR").expect("the power plant type");
    let owner = american(&mut sim);
    let script = || {
        ScriptedPolicy::new(vec![
            ScriptStep {
                frame: 0,
                command: Command::QueueProduction { type_id: power },
            },
            ScriptStep {
                frame: 0,
                command: Command::Guard {
                    entity_id: tank,
                    target: None,
                },
            },
        ])
    };

    // Forces off: the base keeps building, the troops are the player's again.
    let mut host = AutoPlayHost::new(Box::new(script()), owner, usize::MAX);
    host.set_domains(crate::sim::autoplay::AutoPlayDomains {
        grouped_only: false,
        production: true,
        forces: false,
        abilities: false,
    });
    host.step(&mut sim, &rules);
    let queued = sim.pending_command_snapshot();
    assert_eq!(queued.len(), 1, "only the base's command survives");
    assert!(
        matches!(queued[0].payload, Command::QueueProduction { .. }),
        "the survivor is the production command, got {:?}",
        queued[0].payload
    );
    // The frame's DoList scan runs what the board admitted, so the next step
    // starts from an empty queue.
    sim.advance_tick(&[], Some(&rules), None, None, 33);
    assert!(
        sim.pending_command_snapshot().is_empty(),
        "the frame consumed what the board queued"
    );

    // Production off: the troops may still be ordered, the base is the player's.
    let mut host = AutoPlayHost::new(Box::new(script()), owner, usize::MAX);
    host.set_domains(crate::sim::autoplay::AutoPlayDomains {
        grouped_only: false,
        production: false,
        forces: true,
        abilities: false,
    });
    host.step(&mut sim, &rules);
    let queued = sim.pending_command_snapshot();
    assert_eq!(queued.len(), 1, "only the troops' command survives");
    assert!(
        matches!(queued[0].payload, Command::Guard { .. }),
        "the survivor is the unit order, got {:?}",
        queued[0].payload
    );
}

#[test]
fn the_scripted_policy_releases_each_step_once_at_its_frame() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let tank = sim
        .spawn_object("MTNK", "Americans", 16, 16, 64, &rules)
        .expect("an American tank");
    let mut agent = ScriptedPolicy::new(vec![
        ScriptStep {
            frame: 5,
            command: Command::Stop { entity_id: tank },
        },
        ScriptStep {
            frame: 5,
            command: Command::Scatter { entity_id: tank },
        },
        ScriptStep {
            frame: 9,
            command: Command::Guard {
                entity_id: tank,
                target: None,
            },
        },
    ]);
    let owner = american(&mut sim);

    // Out of order input is sorted, and same-frame steps keep their given order.
    assert_eq!(agent.pending().len(), 3);
    assert!(script_frame(&mut agent, &rules, &sim, owner).is_empty());
    assert_eq!(sim.session().tick(), 0);

    // Frame 5 releases both of its steps, once, in script order.
    advance_to(&mut sim, &rules, 5);
    assert_eq!(sim.session().tick(), 5);
    let released = script_frame(&mut agent, &rules, &sim, owner);
    assert_eq!(
        released,
        vec![
            Command::Stop { entity_id: tank },
            Command::Scatter { entity_id: tank }
        ]
    );
    assert!(
        script_frame(&mut agent, &rules, &sim, owner).is_empty(),
        "a step is released once, not every frame"
    );

    // Frame 9 releases the last one.
    advance_to(&mut sim, &rules, 9);
    assert_eq!(
        script_frame(&mut agent, &rules, &sim, owner),
        vec![Command::Guard {
            entity_id: tank,
            target: None
        }]
    );
    assert!(agent.pending().is_empty());
}

/// The host stamps a command with the tick it stepped on, and the frame drains
/// everything due at `execute_tick <= tick + 1`. Which frame's drain picks it
/// up is easy to state wrongly, so pin it on a real frame loop.
#[test]
fn a_host_issued_command_runs_in_the_frame_that_issues_it() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let tank = sim
        .spawn_object("MTNK", "Americans", 16, 16, 64, &rules)
        .expect("an American tank");
    let owner = american(&mut sim);
    let mut host = AutoPlayHost::new(
        Box::new(ScriptedPolicy::from_pairs([(
            5,
            Command::Stop { entity_id: tank },
        )])),
        owner,
        usize::MAX,
    );

    // Gameplay commands stay in the queue until the frame's DoList scan, so
    // what the host issued shows up as an executed command of that same frame.
    let mut executed_per_frame = Vec::new();
    for _ in 0..8 {
        host.step(&mut sim, &rules);
        let result = sim.advance_tick(&[], Some(&rules), None, None, 33);
        executed_per_frame.push(result.executed_commands);
    }

    assert_eq!(
        executed_per_frame,
        [0, 0, 0, 0, 0, 1, 0, 0],
        "the step on tick 5 runs in that frame, not in a later one"
    );
}

#[test]
fn observation_reads_the_wallet_and_power_from_their_owners() {
    let (mut sim, _rules) = synthetic_arena(0xA17);
    let owner = american(&mut sim);
    let view = Observation::new(&sim, owner);

    assert_eq!(view.house(), owner);
    assert_eq!(view.tick(), sim.session().tick());
    assert_eq!(
        view.credits(),
        sim.houses().get(&owner).unwrap().economy().credits(),
        "credits is the house wallet, not a presentation value"
    );
    let state = sim.power_states().get(&owner);
    assert_eq!(view.power_output(), state.map_or(0, |s| s.total_output()));
    assert_eq!(view.power_drain(), state.map_or(0, |s| s.total_drain()));
    assert_eq!(
        view.power_balance(),
        view.power_output().wrapping_sub(view.power_drain())
    );
}

#[test]
fn observation_lists_only_the_houses_own_objects() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let tank = sim
        .spawn_object("MTNK", "Americans", 16, 16, 64, &rules)
        .expect("an American tank");
    let theirs = sim
        .spawn_object("MTNK", "Russians", 18, 16, 64, &rules)
        .expect("a Russian tank");
    let owner = american(&mut sim);

    let view = Observation::new(&sim, owner);
    let ids: Vec<u64> = view.own_entities().map(|entity| entity.entity_id).collect();
    assert!(ids.contains(&tank));
    assert!(
        !ids.contains(&theirs),
        "the rival house's tank is not visible"
    );

    let mine = view
        .own_entities()
        .find(|entity| entity.entity_id == tank)
        .expect("the tank in the view");
    assert_eq!(mine.cell, (16, 16));
    assert!(
        view.max_health(&rules, mine.type_id)
            .is_some_and(|hp| hp > 0),
        "strength comes from the frozen rules the object was built with"
    );
    assert!(view.is_ground_walkable(16, 17), "the arena is flat ground");
}

/// The queue is the authority; this only counts what the host has handed it.
#[test]
fn a_house_with_no_registration_issues_nothing() {
    let (mut sim, rules) = synthetic_arena(0xA17);
    let stranger = sim.interner.intern("Stranger");
    let mut host = AutoPlayHost::new(
        Box::new(ScriptedPolicy::from_pairs([(0, Command::Cheer)])),
        stranger,
        usize::MAX,
    );

    host.step(&mut sim, &rules);

    assert!(sim.pending_command_snapshot().is_empty());
}

/// The army half: a combat unit no team has taken goes on area guard where it
/// stands — the posture a unit outside a team holds, and what a player's guard
/// click would be — and it is not sent at a hostile across the map, nor ordered
/// again the next frame.
#[test]
fn the_retail_policy_puts_an_idle_unit_on_area_guard() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let owner = american(&mut sim);
    let country = sim.interner.intern("Americans");
    sim.houses_mut_for_test()
        .get_mut(&owner)
        .expect("the American house")
        .set_country_for_test(Some(country));
    sim.spawn_object("GACNST", "Americans", 16, 16, 64, &rules)
        .expect("the house's yard");
    let tank = sim
        .spawn_object("MTNK", "Americans", 12, 18, 64, &rules)
        .expect("an American tank");
    sim.spawn_object("MTNK", "Russians", 28, 28, 64, &rules)
        .expect("a Russian tank across the arena");
    let mut policy = AssistBrain::new(0x5EED);

    let decided = frame_commands(&mut policy, &rules, &sim, owner);
    assert!(
        decided.contains(&Command::Guard {
            entity_id: tank,
            target: None,
        }),
        "the tank goes on area guard where it stands, got {decided:?}"
    );
    assert!(
        !decided
            .iter()
            .any(|command| matches!(command, Command::Attack { .. })),
        "the army does not charge the enemy on its own, got {decided:?}"
    );

    let again = frame_commands(&mut policy, &rules, &sim, owner);
    assert!(
        !again.iter().any(
            |command| matches!(command, Command::Guard { entity_id, .. } if *entity_id == tank)
        ),
        "the order already stands, got {again:?}"
    );
}

/// A neutral or civilian house is not a target: the map's tech buildings,
/// garrisonable houses and parked cars belong to `MultiplayPassive` houses, and
/// the computer AI leaves them alone.
#[test]
fn the_retail_policy_leaves_a_passive_house_alone() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let owner = american(&mut sim);
    let country = sim.interner.intern("Americans");
    sim.houses_mut_for_test()
        .get_mut(&owner)
        .expect("the American house")
        .set_country_for_test(Some(country));
    let russian = sim.interner.intern("Russians");
    sim.houses_mut_for_test()
        .get_mut(&russian)
        .expect("the Russian house")
        .make_multiplay_passive();
    let tank = sim
        .spawn_object("MTNK", "Americans", 16, 16, 64, &rules)
        .expect("an American tank");
    sim.spawn_object("MTNK", "Russians", 18, 16, 64, &rules)
        .expect("a neutral tank beside it");

    let mut policy = AssistBrain::new(0x5EED);
    let decided = frame_commands(&mut policy, &rules, &sim, owner);
    assert!(
        !decided.iter().any(|command| matches!(
            command,
            Command::Attack { attacker_id, .. } if *attacker_id == tank
        )),
        "a passive house is not attacked, got {decided:?}"
    );
}

/// An engineer has no weapon: it captures the nearest building it may take
/// instead of being sent at it with an attack order it cannot execute.
#[test]
fn the_retail_policy_sends_an_engineer_to_capture_a_building() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let owner = american(&mut sim);
    let country = sim.interner.intern("Americans");
    sim.houses_mut_for_test()
        .get_mut(&owner)
        .expect("the American house")
        .set_country_for_test(Some(country));
    let engineer = rules
        .type_array_ids(crate::rules::object_type::ObjectCategory::Infantry)
        .iter()
        .find(|name| {
            rules
                .object_in_category(crate::rules::object_type::ObjectCategory::Infantry, name)
                .is_some_and(|ty| ty.engineer)
        })
        .cloned()
        .expect("the rules name an engineer");
    let capturable = rules
        .building_ids
        .iter()
        .find(|name| {
            rules
                .object_in_category(crate::rules::object_type::ObjectCategory::Building, name)
                .is_some_and(|ty| ty.capturable)
        })
        .cloned()
        .expect("the rules name a capturable building");
    let sapper = sim
        .spawn_object(&engineer, "Americans", 16, 16, 64, &rules)
        .expect("an engineer");
    let prize = sim
        .spawn_object(&capturable, "Russians", 24, 24, 64, &rules)
        .expect("a capturable building");

    let mut policy = AssistBrain::new(0x5EED);
    let decided = frame_commands(&mut policy, &rules, &sim, owner);
    assert!(
        decided.contains(&Command::CaptureBuilding {
            engineer_id: sapper,
            target_building_id: prize,
        }),
        "the engineer is sent to capture it, got {decided:?}"
    );
    assert!(
        !decided.iter().any(|command| matches!(
            command,
            Command::Attack { attacker_id, .. } if *attacker_id == sapper
        )),
        "an unarmed engineer is never given an attack order"
    );
}

/// A team's script owns its members: the team AI pass has no house gate, so a
/// policy-driven house's teams recruit its units and drive them like any other
/// house's. The policy's own order step stands down for them, or two issuers
/// would give the same unit its next step.
#[test]
fn the_retail_policy_leaves_a_teams_members_to_its_script() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let owner = american(&mut sim);
    let country = sim.interner.intern("Americans");
    sim.houses_mut_for_test()
        .get_mut(&owner)
        .expect("the American house")
        .set_country_for_test(Some(country));
    let tank = sim
        .spawn_object("MTNK", "Americans", 16, 16, 64, &rules)
        .expect("an American tank");
    sim.spawn_object("MTNK", "Russians", 18, 16, 64, &rules)
        .expect("a Russian tank beside it");
    let script = sim.interner.intern("MEMBER_SCRIPT");
    let task_force = sim.interner.intern("MEMBER_TASKFORCE");
    let team_type = sim.interner.intern("MEMBER_TEAMTYPE");
    sim.team_script_vm
        .register_script(crate::sim::team_script_vm::TeamScriptDefinition {
            id: script,
            actions: Vec::new(),
            source: crate::rules::team_ai_ini::TeamAiDefinitionSource::Scenario,
        });
    sim.team_script_vm
        .register_task_force(crate::sim::team_script_vm::TeamTaskForceDefinition {
            id: task_force,
            group: -1,
            entries: vec![crate::sim::team_script_vm::TeamTaskForceEntry {
                member_type: crate::sim::team_script_vm::TeamMemberTypeIdentity {
                    category: crate::rules::object_type::ObjectCategory::Vehicle,
                    id: sim.interner.intern("MTNK"),
                },
                count: 1,
            }],
            source: crate::rules::team_ai_ini::TeamAiDefinitionSource::Scenario,
        });
    sim.team_script_vm
        .register_team_type(crate::sim::team_script_vm::TeamTypeDefinition {
            id: team_type,
            script_id: script,
            task_force_id: task_force,
            priority: 0,
            is_base_defense: true,
            suicide: false,
            aggressive: false,
            combined_movement_zone: crate::rules::locomotor_type::MovementZone::Normal,
            base_zone_relation_enforced: false,
            transport_crossing_required: false,
        });
    let team = sim
        .team_script_vm
        .construct_team(
            team_type,
            owner,
            crate::rules::rule_policy::TeamLimitScope::PerHouse,
            0,
            true,
            None,
        )
        .expect("a forming team");
    sim.team_add_member(team, tank, false, &rules, &mut Default::default());

    let mut policy = AssistBrain::new(0x5EED);
    let decided = frame_commands(&mut policy, &rules, &sim, owner);
    assert!(
        !decided.iter().any(|command| matches!(
            command,
            Command::Attack { attacker_id, .. } if *attacker_id == tank
        )),
        "a recruited member is the script's, got {decided:?}"
    );
}

/// One auto play frame: the brain decides, the frame drains what it issued.
fn step_policy(
    sim: &mut Simulation,
    rules: &RuleSet,
    policy: &mut AssistBrain,
    owner: crate::sim::intern::InternedId,
) {
    let commands = frame_commands(policy, rules, sim, owner);
    let due: Vec<CommandEnvelope> = commands
        .iter()
        .map(|command| CommandEnvelope::new(owner, sim.session().tick(), command.clone()))
        .collect();
    sim.advance_tick(&due, Some(rules), None, None, 33);
}

/// The house's placed structures.
fn buildings(sim: &Simulation, owner: crate::sim::intern::InternedId) -> usize {
    sim.entities()
        .values()
        .filter(|entity| {
            entity.owner() == owner
                && entity.category() == crate::map::entities::EntityCategory::Structure
                && !entity.lifecycle.in_limbo()
        })
        .count()
}

/// Whether a live, placed object of `name` belongs to `owner`.
fn stands(sim: &Simulation, owner: crate::sim::intern::InternedId, name: &str) -> bool {
    let Some(type_id) = sim.interner().get(name) else {
        return false;
    };
    sim.entities().values().any(|entity| {
        entity.owner() == owner && entity.type_ref() == type_id && !entity.lifecycle.in_limbo()
    })
}

/// Drains the queue the way the production frame loop does, up to `tick`.
fn advance_to(sim: &mut Simulation, rules: &RuleSet, tick: u64) {
    while sim.session().tick() < tick {
        let _: Vec<CommandEnvelope> = sim.take_due_commands();
        sim.advance_tick(&[], Some(rules), None, None, 33);
    }
}

// ---------------------------------------------------------------------------
// The order funnel, managed mode, and the reserved external seam.
// ---------------------------------------------------------------------------

/// Every command this frame admitted, without advancing the frame.
///
/// The board's output is the queue: gameplay commands stay in it until the
/// frame's DoList scan runs them, so the queue — not the frame's drained
/// ingress — is what a step produced.
fn frame_queued(host: &mut AutoPlayHost, sim: &mut Simulation, rules: &RuleSet) -> Vec<Command> {
    host.step(sim, rules);
    sim.pending_command_snapshot()
        .into_iter()
        .map(|envelope| envelope.payload)
        .collect()
}

#[test]
fn the_order_funnel_refuses_an_absent_actor_and_names_a_present_one() {
    let Some((mut sim, rules)) = arena(0xB0B) else {
        return;
    };
    let owner = american(&mut sim);
    let tank = sim
        .spawn_object("MTNK", "Americans", 16, 16, 64, &rules)
        .expect("an American tank");
    let funnel = Funnel::new(&sim, &rules, owner);

    assert_eq!(
        funnel.prepare(u64::MAX, Goal::Stop),
        Err(Refusal::NoSuchActor),
        "a gone object is refused, not silently dropped"
    );
    assert_eq!(Refusal::NoSuchActor.outcome(), ApplyOutcome::Rejected);
    assert_eq!(
        funnel.prepare(tank, Goal::Stop),
        Ok(Command::Stop { entity_id: tank })
    );
    assert!(funnel.owns(tank));
}

/// `Ready_To_Commence` answers whether the mission may be *written* now, and
/// the MEGAMISSION family never writes one: `EventClass::Execute`'s arm queues
/// at `0x004C73B9` and promotes at the object's own AI slot. Gating that family
/// on readiness made a walking unit unreachable — the assist brain could not
/// pull it out of its walk, so a unit passed every enemy in reach without being
/// told to fight it, and the brain asked again on every frame of the walk.
#[test]
fn a_unit_that_is_not_ready_still_takes_the_commands_that_queue() {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[Countries]\n0=Americans\n[VehicleTypes]\n0=MTNK\n\
         [MTNK]\nStrength=200\nSpeed=5\nSpeedType=Track\nMovementZone=Normal\n\
         Armor=none\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
    ))
    .expect("tank rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xB0B);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = american(&mut sim);
    let tank = sim
        .spawn_object("MTNK", "Americans", 16, 16, 64, &rules)
        .expect("an American tank");
    // `Sticky` is never ready: the answer is false before any world lookup.
    let now = sim.session().binary_frame();
    sim.substrate
        .mission_assign_exact(tank, MissionId::from_known(MissionType::Sticky), now)
        .expect("a unit takes Sticky");
    assert!(
        !sim.mission_ready_to_commence(tank, &rules),
        "the fixture object is mid-transition"
    );

    let funnel = Funnel::new(&sim, &rules, owner);
    assert_eq!(
        funnel.prepare(tank, Goal::Move { cell: (20, 16) }),
        Ok(Command::Move {
            entity_id: tank,
            target_rx: 20,
            target_ry: 16
        }),
        "the march queues and is taken"
    );
    assert_eq!(
        funnel.prepare(tank, Goal::Deploy),
        Err(Refusal::NotReady),
        "a direct write still waits for the object"
    );
}

#[test]
fn a_managed_object_is_driven_by_its_routine_and_keeps_its_phase() {
    let Some((mut sim, rules)) = arena(0xB0B) else {
        return;
    };
    let tank = sim
        .spawn_object("MTNK", "Americans", 8, 8, 64, &rules)
        .expect("an American tank");
    let _enemy = sim
        .spawn_object("MTNK", "Russians", 10, 8, 64, &rules)
        .expect("a Russian tank");
    let mut host = retail_host(&mut sim, 7);
    assert!(host.attach(tank, MicroKind::Kite, MicroParams::default(), None));
    assert!(
        !host.attach(tank, MicroKind::Kite, MicroParams::default(), None),
        "a second attach must not reset a phase"
    );

    let issued = frame_queued(&mut host, &mut sim, &rules);

    let attacks: Vec<&Command> = issued
        .iter()
        .filter(|command| {
            matches!(command, Command::Attack { attacker_id, .. } if *attacker_id == tank)
        })
        .collect();
    assert_eq!(attacks.len(), 1, "one issuer per object: its routine");
    let managed = host.managed().get(tank).expect("still attached");
    assert_eq!(managed.kind, MicroKind::Kite);
    assert_eq!(managed.phase.stage, Stage::Engage);
    assert_eq!(managed.refusals, 0);
}

#[test]
fn a_routine_hands_a_healthy_object_back() {
    let Some((mut sim, rules)) = arena(0xB0B) else {
        return;
    };
    let tank = sim
        .spawn_object("MTNK", "Americans", 8, 8, 64, &rules)
        .expect("an American tank");
    let mut host = retail_host(&mut sim, 7);
    assert!(host.attach(tank, MicroKind::Retreat, MicroParams::default(), None));

    host.step(&mut sim, &rules);

    assert!(
        !host.managed().is_managed(tank),
        "nothing to retreat from: the brain gets the object back"
    );
}

#[test]
fn the_forces_switch_stops_managed_stepping() {
    let Some((mut sim, rules)) = arena(0xB0B) else {
        return;
    };
    let tank = sim
        .spawn_object("MTNK", "Americans", 8, 8, 64, &rules)
        .expect("an American tank");
    let _enemy = sim
        .spawn_object("MTNK", "Russians", 10, 8, 64, &rules)
        .expect("a Russian tank");
    let mut host = retail_host(&mut sim, 7);
    host.set_domains(AutoPlayDomains {
        grouped_only: false,
        production: true,
        forces: false,
        abilities: false,
    });
    assert!(host.attach(tank, MicroKind::Kite, MicroParams::default(), None));

    let issued = frame_queued(&mut host, &mut sim, &rules);

    assert!(
        !issued.iter().any(
            |command| matches!(command, Command::Attack { attacker_id, .. } if *attacker_id == tank)
        ),
        "the forces switch stops the routine too"
    );
    assert!(host.managed().is_managed(tank), "and does not forget it");
}

#[test]
fn two_identical_managed_frames_agree() {
    let Some((mut sim, rules)) = arena(0xD00D) else {
        return;
    };
    let tank = sim
        .spawn_object("MTNK", "Americans", 8, 8, 64, &rules)
        .expect("an American tank");
    let _enemy = sim
        .spawn_object("MTNK", "Russians", 10, 8, 64, &rules)
        .expect("a Russian tank");
    let owner = american(&mut sim);

    // Two tables, the same attach, the same world: the steps must agree, and so
    // must the phases they leave behind. This is the invariant that keeps
    // managed state out of the checksum: it is a pure function of the world and
    // the phase, so a replay reproduces it from the command stream.
    let mut first = crate::sim::control::micro::MicroTable::default();
    let mut second = crate::sim::control::micro::MicroTable::default();
    assert!(first.attach(tank, MicroKind::Kite, MicroParams::default(), None));
    assert!(second.attach(tank, MicroKind::Kite, MicroParams::default(), None));
    {
        let threats = crate::sim::behavior::ThreatView::new(&sim, owner);
        let left = first.step_one(tank, &sim, &rules, &threats);
        let right = second.step_one(tank, &sim, &rules, &threats);
        assert_eq!(left, right);
        assert!(matches!(
            left,
            Some(crate::sim::behavior::MicroStep::Goal(_))
        ));
    }
    assert_eq!(first.get(tank), second.get(tank));
}

#[test]
fn the_external_batch_layout_is_fixed_versioned_and_reproducible() {
    let Some((mut sim, rules)) = arena(0xC0DE) else {
        return;
    };
    let owner = american(&mut sim);
    let obs = Observation::new(&sim, owner);
    let schema = ObservationSchema::current();
    assert_eq!(schema.version, OBSERVATION_SCHEMA_VERSION);
    assert_eq!(schema.globals, GLOBAL_SLOTS);
    assert_eq!(schema.entity_features, ENTITY_SLOTS);
    assert_eq!(schema.global_features.len(), GLOBAL_SLOTS);
    assert_eq!(schema.entity_feature_names.len(), ENTITY_SLOTS);

    let batch = batch_for(&obs, &rules);
    assert_eq!(batch.version, OBSERVATION_SCHEMA_VERSION);
    assert_eq!(batch.globals.len(), GLOBAL_SLOTS);
    assert_eq!(batch.entities.len(), batch.rows() * ENTITY_SLOTS);
    assert_eq!(batch.rows(), obs.own_entities().count());
    assert_eq!(
        batch,
        batch_for(&obs, &rules),
        "two builds from one state agree"
    );

    let ids: Vec<u64> = (0..batch.rows())
        .filter_map(|row| batch.row_entity(row))
        .collect();
    let mut ascending = ids.clone();
    ascending.sort_unstable();
    assert_eq!(ids, ascending, "rows follow stable id order");
}

#[test]
fn an_action_slot_resolves_its_rows_and_rejects_an_out_of_range_one() {
    let Some((mut sim, rules)) = arena(0xC0DE) else {
        return;
    };
    let owner = american(&mut sim);
    sim.spawn_object("MTNK", "Americans", 8, 8, 64, &rules)
        .expect("an American tank");
    sim.spawn_object("MTNK", "Americans", 8, 10, 64, &rules)
        .expect("a second American tank");
    let obs = Observation::new(&sim, owner);
    let batch = batch_for(&obs, &rules);
    assert!(batch.rows() >= 2, "the arena gives the player objects");

    let slot = ActionSlot {
        row: 0,
        goal: Goal::Attack { target: 0 },
        target_row: Some(1),
        cell: None,
    };
    let (actor, goal) = slot.resolve(&batch).expect("rows in range");
    assert_eq!(Some(actor), batch.row_entity(0));
    match goal {
        Goal::Attack { target } => assert_eq!(Some(target), batch.row_entity(1)),
        other => panic!("expected an attack, got {other:?}"),
    }

    let out_of_range = ActionSlot {
        row: u32::MAX,
        goal: Goal::Stop,
        target_row: None,
        cell: None,
    };
    assert!(
        out_of_range.resolve(&batch).is_none(),
        "a malformed answer is not an empty one"
    );
}

#[test]
fn a_policy_source_is_object_safe_and_carries_the_schema_version() {
    struct Echo;
    impl PolicySource for Echo {
        fn schema(&self) -> ObservationSchema {
            ObservationSchema::current()
        }

        fn act(&mut self, _batch: &ObservationBatch) -> Vec<ActionSlot> {
            vec![ActionSlot {
                row: 0,
                goal: Goal::Stop,
                target_row: None,
                cell: None,
            }]
        }
    }

    let mut source: Box<dyn PolicySource> = Box::new(Echo);
    let batch = ObservationBatch {
        version: OBSERVATION_SCHEMA_VERSION,
        globals: vec![0; GLOBAL_SLOTS],
        entities: vec![0; ENTITY_SLOTS],
    };
    assert_eq!(source.schema().version, OBSERVATION_SCHEMA_VERSION);
    let answer = source.act(&batch);
    assert_eq!(answer.len(), 1);
    assert_eq!(answer[0].goal, Goal::Stop);
}

/// Asks for one object to be attached and, in the same frame, orders it
/// somewhere else: the control layer's half of `Outbox::attach`.
///
/// A brain cannot reach the managed table — it is L2's state — so it asks
/// through the outbox, and the object it hands over must not also receive the
/// brain's own order for that frame. One object, one issuer.
struct AttachAndMove {
    actor: u64,
}

impl Brain for AttachAndMove {
    fn name(&self) -> &'static str {
        "test-attach"
    }

    fn think(&mut self, _rules: &RuleSet, _obs: &Observation<'_>, out: &mut Outbox) {
        out.attach(
            self.actor,
            MicroKind::Retreat,
            MicroParams {
                health_floor: 900,
                standoff_cells: 4,
                radius_cells: 10,
                hold_frames: 5,
            },
            None,
        );
        out.order(self.actor, Goal::Move { cell: (28, 28) });
    }
}

/// The rules come from the repository's own `ini/` snapshot, so this runs
/// without a retail install.
fn running_arena(seed: u64) -> Option<(Simulation, RuleSet)> {
    let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules()?;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, seed);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    Some((sim, rules))
}

#[test]
fn an_attached_object_does_not_also_take_the_brains_order() {
    let Some((mut sim, rules)) = running_arena(0xA17) else {
        return;
    };
    let owner = american(&mut sim);
    let e1 = sim
        .spawn_object("E1", "Americans", 10, 10, 255, &rules)
        .expect("a G.I.");

    let mut host = AutoPlayHost::new(Box::new(AttachAndMove { actor: e1 }), owner, usize::MAX);
    host.step(&mut sim, &rules);

    // Nothing was refused and nothing was mid-transition, so the only thing
    // that could have kept the move from being issued is the attach.
    assert_eq!(host.rejected(), 0, "the attach was not refused");
    assert_eq!(host.not_ready(), 0, "the unit was in no transition");
    let obs = Observation::new(&sim, owner);
    let mission = obs
        .own_entities()
        .find(|entity| entity.entity_id == e1)
        .map(|entity| entity.mission);
    assert_ne!(
        mission,
        Some(crate::sim::mission::MissionId::from_known(
            crate::sim::mission::MissionType::Move
        )),
        "the routine is the object's only issuer: {mission:?}"
    );
}

/// The client mirrors a press's presentation writes for what the frame queued,
/// so the step reports those commands in issue order — the same list the queue
/// hands the drain.
#[test]
fn a_step_reports_the_commands_it_queued() {
    let (mut sim, rules) = synthetic_arena(0xA17);
    let power_plant = sim.interner.intern("GAPOWR");
    let power = Command::QueueProduction {
        type_id: power_plant,
    };
    let mut host = host(&mut sim, vec![(0, power.clone())]);

    let issued = host.step(&mut sim, &rules);

    assert_eq!(
        issued,
        vec![power.clone()],
        "the hand reports what it issued"
    );
    assert_eq!(
        sim.pending_command_snapshot()
            .into_iter()
            .map(|envelope| envelope.payload)
            .collect::<Vec<_>>(),
        vec![power],
        "and the report is the queue the frame drains"
    );
}

#[test]
fn a_full_queue_refuses_the_order_without_reporting_it_as_issued() {
    use std::cell::Cell;
    use std::rc::Rc;

    struct BuildAndMove {
        power: InternedId,
        actor: u64,
        goal: Goal,
        refused: Rc<Cell<Option<(u64, Goal)>>>,
    }

    impl Brain for BuildAndMove {
        fn name(&self) -> &'static str {
            "build-and-move"
        }

        fn think(&mut self, _: &RuleSet, _: &Observation<'_>, out: &mut Outbox) {
            out.command(Command::QueueProduction {
                type_id: self.power,
            });
            out.order(self.actor, self.goal);
        }

        fn orders_refused(&mut self, refused: &[(u64, Goal)]) {
            self.refused.set(refused.first().copied());
        }
    }

    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[VehicleTypes]\n0=PLANNER\n[PLANNER]\nStrength=100\nSpeed=8\nSpeedType=Track\nMovementZone=Normal\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
    ))
    .expect("synthetic rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
    let tank = sim
        .spawn_object("PLANNER", "Americans", 16, 16, 0, &rules)
        .expect("an American tank");
    let owner = american(&mut sim);
    let type_id = sim.interner.intern("GAPOWR");
    let power = Command::QueueProduction { type_id };
    let capacity = crate::sim::command_queue::SINGLE_PLAYER_DOLIST_CAPACITY;
    let future = CommandEnvelope::new(owner, sim.session().tick() + 60, power.clone());
    sim.queue_commands(std::iter::repeat_n(future, capacity - 1));

    let goal = Goal::Move { cell: (20, 20) };
    let refused = Rc::new(Cell::new(None));
    let mut host = AutoPlayHost::new(
        Box::new(BuildAndMove {
            power: type_id,
            actor: tank,
            goal,
            refused: Rc::clone(&refused),
        }),
        owner,
        8,
    );

    assert_eq!(host.step(&mut sim, &rules), [power]);
    assert_eq!(refused.get(), Some((tank, goal)));
    assert_eq!(host.rejected(), 1);
}

/// The brain sees the same independent domain switches as the board's
/// command gate. Its building task must never infer that enabled production
/// implicitly enables the unit commands needed to clear a footprint.
#[test]
fn work_switches_are_projected_without_changing_player_group_scope() {
    let (mut sim, _) = synthetic_arena(0xC1EA);
    let owner = american(&mut sim);
    let grouped_id = 42_u64;
    let members = std::collections::BTreeSet::from([grouped_id]);
    let obs = Observation::new(&sim, owner)
        .with_control_scope(&members, true)
        .with_work_scope(true, false, true);
    assert!(obs.production_enabled());
    assert!(!obs.forces_enabled());
    assert!(obs.abilities_enabled());
    assert!(!obs.owned_by_hand(grouped_id),
        "a player-numbered-group member is admitted by group-only scope");
    assert!(obs.owned_by_hand(43),
        "a non-grouped object is outside group-only scope");
    let paused = Observation::new(&sim, owner)
        .with_control_scope(&members, true)
        .with_work_scope(false, false, false);
    assert!(!paused.production_enabled());
    assert!(!paused.forces_enabled());
    assert!(!paused.abilities_enabled());
    assert!(!paused.owned_by_hand(grouped_id),
        "the global work gate must not reset the fifth-key scope");
}

/// A request rejected by L2 did not reach the native queue and must not
/// enter the board's 15-frame accepted-intent dedupe. The brain hears every
/// rejection and can retry its *same* building site when the budget recovers.
#[test]
fn rejected_house_intents_are_reported_without_ghost_deduplication() {
    use std::cell::Cell;
    use std::rc::Rc;

    struct Placer {
        ty: InternedId,
        rejections: Rc<Cell<usize>>,
    }
    impl Brain for Placer {
        fn name(&self) -> &'static str { "unfunded-placer" }
        fn think(&mut self, _: &RuleSet, _: &Observation<'_>, out: &mut Outbox) {
            out.intent(crate::sim::control::intent::Intent::PlaceBuilding {
                type_id: self.ty, cell: (14, 14),
            });
        }
        fn intents_refused(&mut self, refused: &[crate::sim::control::intent::Intent]) {
            self.rejections.set(self.rejections.get() + refused.len());
        }
    }

    let (mut sim, rules) = synthetic_arena(0xA17);
    let owner = american(&mut sim);
    let ty = sim.interner.intern("GAPOWR");
    let count = Rc::new(Cell::new(0));
    let mut host = AutoPlayHost::new(Box::new(Placer {
        ty, rejections: Rc::clone(&count),
    }), owner, 0); // Zero available commands, even for house intents.
    for _ in 0..3 {
        assert!(host.step(&mut sim, &rules).is_empty());
        sim.advance_tick(&[], Some(&rules), None, None, 33);
    }
    assert_eq!(count.get(), 3,
        "each native-gate rejection has feedback and may retry next tick");
}

/// A brain with one house request it asks for again every frame.
///
/// The assist brain does exactly this: it reads the projection, sees no power
/// plant, and asks for one. On XMP29U2.MAP that filled a 24,736-tick match with
/// 12,898 `QueueProduction` commands for one type at a median spacing of one
/// tick, and the yard never finished it. The board holds the request for a term.
#[test]
fn a_house_request_repeated_every_frame_is_issued_once_a_term() {
    use crate::sim::control::intent::Intent;

    struct Nagging {
        power: InternedId,
    }

    impl Brain for Nagging {
        fn name(&self) -> &'static str {
            "nagging"
        }

        fn think(&mut self, _: &RuleSet, _: &Observation<'_>, out: &mut Outbox) {
            out.intents.push(Intent::QueueProduction {
                type_id: self.power,
            });
        }
    }

    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(""))
        .expect("synthetic rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
    let owner = american(&mut sim);
    let type_id = sim.interner.intern("GAPOWR");
    let mut host = AutoPlayHost::new(Box::new(Nagging { power: type_id }), owner, 8);

    let mut issued = Vec::new();
    for _ in 0..30 {
        issued.extend(host.step(&mut sim, &rules));
        sim.advance_tick(&[], Some(&rules), None, None, 33);
    }
    let queued = issued
        .iter()
        .filter(|command| matches!(command, Command::QueueProduction { .. }))
        .count();
    assert_eq!(
        queued, 2,
        "one request when asked, one after the term, not one a frame"
    );
}

/// A brain that gives one object the same order every frame.
///
/// The assist brain does this too: its passes re-ask for a guard post, an
/// approach or a capture each frame, and the funnel answers a unit that is
/// mid-transition with `NotReady`. Measured on Barrel.mmx: 118-127 orders a
/// frame were refused that way, and one engineer was sent into the same
/// building 321 times in 12,000 ticks.
#[test]
fn an_order_repeated_every_frame_reaches_the_unit_once_a_term() {
    struct Nagging {
        actor: u64,
    }

    impl Brain for Nagging {
        fn name(&self) -> &'static str {
            "nagging"
        }

        fn think(&mut self, _: &RuleSet, _: &Observation<'_>, out: &mut Outbox) {
            out.order(self.actor, Goal::Guard);
        }
    }

    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[VehicleTypes]\n0=PLANNER\n[PLANNER]\nStrength=100\nSpeed=8\nSpeedType=Track\nMovementZone=Normal\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
    ))
    .expect("synthetic rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
    let tank = sim
        .spawn_object("PLANNER", "Americans", 16, 16, 0, &rules)
        .expect("an American tank");
    let owner = american(&mut sim);
    let mut host = AutoPlayHost::new(Box::new(Nagging { actor: tank }), owner, 8);

    let mut issued = Vec::new();
    for _ in 0..30 {
        issued.extend(host.step(&mut sim, &rules));
        sim.advance_tick(&[], Some(&rules), None, None, 33);
    }
    let guards = issued
        .iter()
        .filter(|command| matches!(command, Command::Guard { .. }))
        .count();
    assert_eq!(
        guards, 2,
        "one order when asked, one after the term, not one a frame"
    );
}
