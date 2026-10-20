//! Pending Select6AB80D/6AB0C9 -> real HouseBegin4FA350/Suspend4FA910 ->
//! same paid Infantry head -> ordinary exit. The parent supplies a healthy
//! stock base/terrain and the actual gadget/projection/frame helpers. This
//! is a Rust production regression; native execution controls live in
//! tools/sidebar_oracle/pending_production.{json,md}.

use super::*;
use crate::ui::sidebar::cameo_order::CameoRequestState;

fn infantry_fixture() -> Option<Fixture> {
    let mut fixture = stock_base_fixture(
        "Russians",
        &[
            ("NACNST", 4),
            ("NAPOWR", 11),
            ("NAHAND", 18),
            ("NARADR", 25),
        ],
        false,
    )?;
    let owner = fixture.owner;
    for _ in 0..8 {
        let frame = advance_commands(&mut fixture, &[]);
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
    }
    refresh_fixture_sidebar(&mut fixture, SidebarTab::Infantry);
    Some(fixture)
}

fn next_command(fixture: &Fixture, command: Command) -> CommandEnvelope {
    CommandEnvelope::new(fixture.owner, fixture.sim.session.tick() + 1, command)
}

fn native_e2_category_gates(phase_name: &str) -> Vec<ProductionCategory> {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/sidebar_oracle/pending_production.json",
    ))
    .unwrap();
    let history = corpus["histories"]
        .as_array()
        .unwrap()
        .iter()
        .find(|history| history["name"] == "idle_hold_resume")
        .unwrap();
    let phase = history["phases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|phase| phase["name"] == phase_name)
        .unwrap();
    // Original House5005B0 stores Infantry at53D1 and falls through to
    // nonnaval Vehicle53D2. These are observed bytes, not derived gates.
    let bytes = phase["after"]["category_request_bytes"].as_array().unwrap();
    [
        (1, ProductionCategory::Infantry),
        (2, ProductionCategory::Vehicle),
    ]
    .into_iter()
    .filter_map(|(offset, category)| (bytes[offset].as_u64().unwrap() != 0).then_some(category))
    .collect()
}

#[test]
fn pending_first_e2_right_holds_one_real_head_then_resumes_its_paid_exit() {
    for shift in [false, true] {
        let Some(mut fixture) = infantry_fixture() else {
            return;
        };
        let owner = fixture.owner;
        let e2 = fixture.sim.interner.get("E2").unwrap();
        let baseline_money = fixture.sim.houses[&owner].economy().credits();
        let baseline_spent = fixture.sim.houses[&owner].economy().spent_credits();
        let baseline_entities = fixture.sim.entities().len();

        let left = press_cameo(&mut fixture, "E2", SidebarTab::Infantry, CameoPress::Left);
        assert_eq!(left.order, Some(CameoOrder::Produce));
        assert_eq!(
            fixture.projection.cameo_request(e2),
            Some(CameoRequestState::Started)
        );
        assert!(
            fixture
                .sim
                .production
                .factories()
                .view(owner, ProductionCategory::Infantry)
                .is_none()
        );
        assert_eq!(fixture.sim.entities().len(), baseline_entities);

        let right = press_cameo(
            &mut fixture,
            "E2",
            SidebarTab::Infantry,
            CameoPress::Right { shift },
        );
        assert_eq!(right.order, Some(CameoOrder::Suspend));
        assert!(right.sound);
        assert_eq!(right.eva, Some(sidebar_eva::EVA_ON_HOLD));
        assert_eq!(
            fixture.projection.cameo_request(e2),
            Some(CameoRequestState::Held)
        );
        let pending = refresh_fixture_sidebar(&mut fixture, SidebarTab::Infantry);
        let pending = pending
            .items
            .iter()
            .find(|item| item.type_id == "E2")
            .unwrap();
        assert!(pending.is_on_hold && !pending.is_ready);
        assert_eq!(pending.queued_count, 0, "a request has no real Factory yet");
        let repeated = press_cameo(
            &mut fixture,
            "E2",
            SidebarTab::Infantry,
            CameoPress::Right { shift },
        );
        assert_eq!(repeated.order, None);
        assert!(!repeated.sound);
        assert_eq!(repeated.eva, None);

        // Queue both accepted decisions before the ordinary dispatch. Their
        // order, rather than a synthetic paused Factory, causes the hold.
        let commands = [
            next_command(&fixture, Command::QueueProduction { type_id: e2 }),
            next_command(
                &fixture,
                Command::SuspendProduction {
                    category: ProductionCategory::Infantry,
                },
            ),
        ];
        let frame = advance_commands(&mut fixture, &commands);
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
        let factory = fixture
            .sim
            .production
            .factories()
            .view(owner, ProductionCategory::Infantry)
            .unwrap();
        let order = factory.construction_order();
        let held_id = factory.object.unwrap().entity_id.unwrap();
        assert!(factory.suspended && !factory.is_running());
        assert_eq!(factory.progress, 0);
        assert!(factory.queue.is_empty());
        assert!(fixture.sim.entities().get(held_id).unwrap().in_limbo());
        assert_eq!(
            fixture.projection.cameo_request(e2),
            Some(CameoRequestState::Held),
            "linking preserves the pending HOLD request"
        );
        assert_eq!(
            fixture
                .projection
                .saved_order()
                .unwrap()
                .prediction_categories(),
            native_e2_category_gates("receive_produce_then_suspend")
        );
        assert_eq!(
            fixture.sim.houses[&owner].economy().credits(),
            baseline_money
        );
        assert_eq!(
            fixture.sim.houses[&owner].economy().spent_credits(),
            baseline_spent
        );

        for _ in 0..12 {
            let frame = advance_commands(&mut fixture, &[]);
            consume_frame_outputs(&mut fixture, &frame, Some(owner));
        }
        let factory = fixture
            .sim
            .production
            .factories()
            .view(owner, ProductionCategory::Infantry)
            .unwrap();
        assert_eq!(factory.progress, 0);
        assert_eq!(factory.object.unwrap().entity_id, Some(held_id));
        assert!(
            refresh_fixture_sidebar(&mut fixture, SidebarTab::Infantry)
                .items
                .iter()
                .find(|item| item.type_id == "E2")
                .unwrap()
                .is_on_hold
        );

        let resume = press_cameo(&mut fixture, "E2", SidebarTab::Infantry, CameoPress::Left);
        assert_eq!(resume.order, Some(CameoOrder::Produce));
        assert_eq!(
            fixture.projection.cameo_request(e2),
            Some(CameoRequestState::Started)
        );
        assert_eq!(
            fixture
                .projection
                .saved_order()
                .unwrap()
                .prediction_categories(),
            native_e2_category_gates("left_held_resume"),
            "held Left marks both native gates before PRODUCE dispatch"
        );
        let command = next_command(&fixture, Command::QueueProduction { type_id: e2 });
        let frame = advance_commands(&mut fixture, &[command]);
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
        let factory = fixture
            .sim
            .production
            .factories()
            .view(owner, ProductionCategory::Infantry)
            .unwrap();
        assert_eq!(factory.construction_order(), order);
        assert_eq!(factory.object.unwrap().entity_id, Some(held_id));
        assert!(factory.is_running());
        assert!(
            factory.queue.is_empty(),
            "resume does not append a second E2"
        );

        let mut delivered = false;
        for _ in 0..1024 {
            let frame = advance_commands(&mut fixture, &[]);
            consume_frame_outputs(&mut fixture, &frame, Some(owner));
            if fixture
                .sim
                .entities()
                .get(held_id)
                .is_some_and(|entity| !entity.in_limbo())
            {
                delivered = true;
                break;
            }
        }
        assert!(
            delivered,
            "the same paid E2 reaches ordinary Infantry ExitObject"
        );
        let cost = fixture
            .sim
            .cost_of(owner, fixture.rules.object("E2").unwrap(), &fixture.rules);
        assert_eq!(
            fixture.sim.houses[&owner].economy().spent_credits(),
            baseline_spent + cost
        );
        assert_eq!(
            fixture.sim.houses[&owner].economy().credits(),
            baseline_money - cost
        );
        assert!(
            production::queue_view_for_owner(&fixture.sim, &fixture.rules, "Russians").is_empty()
        );
        assert_eq!(fixture.projection.cameo_request(e2), None);
        let sidebar = refresh_fixture_sidebar(&mut fixture, SidebarTab::Infantry);
        let cameo = sidebar
            .items
            .iter()
            .find(|item| item.type_id == "E2")
            .unwrap();
        assert!(!cameo.is_on_hold && !cameo.is_ready && !cameo.is_building_this_type);
        assert_eq!(cameo.queued_count, 0);
        let delivered = fixture.sim.entities().get(held_id).unwrap();
        assert_eq!(delivered.type_ref(), e2);
        assert_eq!(delivered.owner(), owner);
        assert!(delivered.is_alive());
        assert_eq!(
            fixture
                .sim
                .entities()
                .iter_sorted()
                .filter(|(_, entity)| entity.owner() == owner && entity.type_ref() == e2)
                .count(),
            1
        );
    }
}

#[test]
fn held_e2_resume_suppresses_a_vehicle_pending_request_before_dispatch() {
    let Some(mut fixture) = stock_base_fixture(
        "Russians",
        &[
            ("NACNST", 4),
            ("NAPOWR", 11),
            ("NAHAND", 18),
            ("NAWEAP", 25),
        ],
        false,
    ) else {
        return;
    };
    let owner = fixture.owner;
    let e2 = fixture.sim.interner.get("E2").unwrap();
    let htnk = fixture.sim.interner.get("HTNK").unwrap();
    for _ in 0..8 {
        let frame = advance_commands(&mut fixture, &[]);
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
    }
    refresh_fixture_sidebar(&mut fixture, SidebarTab::Infantry);
    let commands = [
        next_command(&fixture, Command::QueueProduction { type_id: e2 }),
        next_command(
            &fixture,
            Command::SuspendProduction {
                category: ProductionCategory::Infantry,
            },
        ),
    ];
    let frame = advance_commands(&mut fixture, &commands);
    consume_frame_outputs(&mut fixture, &frame, Some(owner));
    assert!(
        fixture
            .projection
            .saved_order()
            .unwrap()
            .prediction_categories()
            .is_empty()
    );
    let resumed = press_cameo(&mut fixture, "E2", SidebarTab::Infantry, CameoPress::Left);
    assert_eq!(resumed.order, Some(CameoOrder::Produce));
    let vehicle = press_cameo(&mut fixture, "HTNK", SidebarTab::Vehicle, CameoPress::Left);
    assert_eq!(vehicle.order, Some(CameoOrder::Produce));
    assert_eq!(fixture.projection.cameo_request(htnk), None);
    let right = press_cameo(
        &mut fixture,
        "HTNK",
        SidebarTab::Vehicle,
        CameoPress::Right { shift: false },
    );
    assert_eq!(right.order, None);
    assert!(!right.sound);
    assert_eq!(right.eva, None);
}

#[test]
fn busy_infantry_left_and_queued_right_do_not_hold_the_current_head() {
    let Some(mut fixture) = infantry_fixture() else {
        return;
    };
    let owner = fixture.owner;
    let e2 = fixture.sim.interner.get("E2").unwrap();
    let flakt = fixture.sim.interner.get("FLAKT").unwrap();
    let command = next_command(&fixture, Command::QueueProduction { type_id: e2 });
    let frame = advance_commands(&mut fixture, &[command]);
    consume_frame_outputs(&mut fixture, &frame, Some(owner));
    let held = fixture
        .sim
        .production
        .factories()
        .view(owner, ProductionCategory::Infantry)
        .unwrap()
        .object
        .unwrap()
        .entity_id;
    let queued_left = press_cameo(
        &mut fixture,
        "FLAKT",
        SidebarTab::Infantry,
        CameoPress::Left,
    );
    assert_eq!(queued_left.order, Some(CameoOrder::Produce));
    assert_eq!(
        fixture.projection.cameo_request(flakt),
        None,
        "a busy-category request has no pending-first flag"
    );
    let command = next_command(&fixture, Command::QueueProduction { type_id: flakt });
    let frame = advance_commands(&mut fixture, &[command]);
    consume_frame_outputs(&mut fixture, &frame, Some(owner));
    let right = press_cameo(
        &mut fixture,
        "FLAKT",
        SidebarTab::Infantry,
        CameoPress::Right { shift: false },
    );
    assert_eq!(right.order, Some(CameoOrder::Abandon { all: false }));
    assert_eq!(right.eva, None);
    let factory = fixture
        .sim
        .production
        .factories()
        .view(owner, ProductionCategory::Infantry)
        .unwrap();
    assert_eq!(factory.object.unwrap().entity_id, held);
    assert!(factory.is_running());
    assert_eq!(factory.queue.len(), 1);
}

#[test]
fn resumed_clock_queries_live_held_duration_even_with_a_zero_stored_rate() {
    let Some(mut fixture) = infantry_fixture() else {
        return;
    };
    let owner = fixture.owner;
    let e2 = fixture.sim.interner.get("E2").unwrap();
    let commands = [
        next_command(&fixture, Command::QueueProduction { type_id: e2 }),
        next_command(
            &fixture,
            Command::SuspendProduction {
                category: ProductionCategory::Infantry,
            },
        ),
    ];
    let frame = advance_commands(&mut fixture, &commands);
    consume_frame_outputs(&mut fixture, &frame, Some(owner));
    let factories = fixture.sim.production.factories().clone();
    let wallet = fixture.sim.houses[&owner].economy().clone();
    let corpus = crate::test_fixture::json("tools/sidebar_oracle/pending_production.json");
    let mut compared = 0;
    for row in corpus["numeric_controls"].as_array().unwrap() {
        let Some(expected) = row["phase"]["calls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|call| call["name"] == "factory_prediction_rate")
        else {
            continue;
        };
        let mut rules = fixture.rules.clone();
        if let Some(bits) = row["inputs"]["prepared_build_speed_bits"].as_str() {
            rules.production.build_speed = crate::util::native_x87::NativeF64Bits::from_bits(
                u64::from_str_radix(bits, 16).unwrap().swap_bytes(),
            );
        }
        let factory = fixture
            .sim
            .production
            .factories()
            .view(owner, ProductionCategory::Infantry)
            .unwrap();
        assert!(factory.suspended && !factory.is_running());
        assert_eq!(
            i64::from(factory.build_step_rate(&fixture.sim, &rules)),
            expected["return_i32"].as_i64().unwrap(),
            "{}: original4C9FB0 reads current held-object rules",
            row["name"],
        );
        compared += 1;
    }
    assert_eq!(compared, 3);
    assert_eq!(*fixture.sim.production.factories(), factories);
    assert_eq!(*fixture.sim.houses[&owner].economy(), wallet);
}
