//! Retail Battle rules -> paid House factories -> retained sidebar -> gadget
//! press -> SelectClass decision -> the shared pending-placement input owner.
//!
//! Clear terrain and the initial healthy base are supplied fixture inputs. The
//! ordinary frame spine pays and completes the builds; this test does not pin
//! native production cadence, whole-map loading, a window/GPU, or physical audio.
//! The pending guard is HouseClass::Manual_Place4FB852..4FB859: any pending
//! building rejects a new entry, including another type's completed building.

use super::MatchInputState;
use crate::app::input::gadget_input::ID_CAMEO_BASE;
use crate::app::input::hotkeys::HotkeyBindings;
use crate::app::input::sidebar_eva::{self, CameoClick, CameoOrder, CameoPress};
use crate::app::sidebar_projection::{SidebarProjectionState, cameo_candidates};
use crate::app::types::TargetingMode;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::production::{self, BuildQueueState, ProductionCategory};
use crate::sim::world::{
    FrameEffects, LifecycleOutput, SimFrameOutput, SimSoundEvent, Simulation, TickLane,
};
use crate::ui::gadget::focus::FocusState;
use crate::ui::gadget::list::{GadgetList, GadgetSpec};
use crate::ui::gadget::tick::{GadgetInput, TickOutput, tick};
use crate::ui::gadget::{
    GadgetRect, KEY_LMB_DOWN, KEY_LMB_UP, KEY_RMB_DOWN, KEY_RMB_UP, ListId, RESULT_BUTTON,
    RESULT_RIGHT,
};
use crate::ui::sidebar::cameo_order::{CameoId, CameoRequestState};
use crate::ui::sidebar::gadget_flash::SidebarGadgetState;
use crate::ui::sidebar::{
    ArmedSidebarEntry, SidebarAction, SidebarChromeLayoutSpec, SidebarTab,
    build_sidebar_view_with_spec, hit_test_item,
};

#[path = "ready_building_placement_tests/pending_production_tests.rs"]
mod pending_production_tests;
#[path = "ready_building_placement_tests/produced_build_limit_tests.rs"]
mod produced_build_limit_tests;
#[path = "ready_building_placement_tests/scenario_house_name_tests.rs"]
mod scenario_house_name_tests;

struct Fixture {
    sim: Simulation,
    rules: RuleSet,
    overlays: OverlayTypeRegistry,
    owner: InternedId,
    input: MatchInputState,
    projection: SidebarProjectionState,
}

fn stock_base_fixture(
    owner_name: &str,
    buildings: &[(&str, u16)],
    bind_buildup: bool,
) -> Option<Fixture> {
    let retail = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")?;
    stock_base_fixture_from_retail(owner_name, buildings, bind_buildup, retail)
}

fn stock_base_fixture_from_retail(
    owner_name: &str,
    buildings: &[(&str, u16)],
    bind_buildup: bool,
    retail: crate::rules::retail_ini_fixture::RetailBattleRules,
) -> Option<Fixture> {
    let overlays = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let mut rules = retail.rules;
    if bind_buildup {
        let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets()?;
        crate::map::theater::load_theater(&mut assets, "TEMPERATE");
        rules.bind_building_buildup_assets(&assets, "TEMPERATE");
    }
    let mut sim = Simulation::with_seed(0x4FB8_4000);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.intern(owner_name);
    let mut house = HouseState::new(owner, 0, Some(owner), true, 50_000, 10);
    house.project_country_mults(&rules, &sim.interner);
    sim.houses.insert(owner, house);
    sim.session.house_order.push(owner);
    sim.session.current_house = Some(owner);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    // Like the supplied flat terrain/base, mapped ground is an explicit
    // fixture prerequisite. This fixture does not execute map reveal startup.
    sim.fog = crate::sim::vision::FogState::new(32, 32);
    sim.fog.reveal_all_for_owner(owner);
    for &(name, rx) in buildings {
        sim.spawn_object(name, owner_name, rx, 4, 0, &rules)
            .unwrap_or_else(|| panic!("healthy retail placement prerequisite {name}"));
    }
    Some(Fixture {
        sim,
        rules,
        overlays,
        owner,
        input: MatchInputState::new(HotkeyBindings::default()),
        projection: SidebarProjectionState::default(),
    })
}

/// The supplied arena has no saved retail-map resource record. Its bound
/// terrain/rules enter the existing fixture adapter to the same fallible
/// world preparation and commit used by the app's resource loader.
fn roundtrip_fixture_save(fixture: &mut Fixture, label: &str) {
    let saved_order = fixture.projection.saved_order();
    let bytes = crate::sim::snapshot::GameSnapshot::save_validated_with_presentation(
        &fixture.sim,
        0,
        fixture.rules.simulation_config_hash(),
        label,
        0,
        &[],
        saved_order.as_ref(),
        None,
        None,
        None,
    );
    let directory = crate::map::source::test_support::TestDirectory::new(label);
    let repository = crate::app::persistence::SaveRepository::at(directory.path());
    let path = repository.write_named("fixture.bin", &bytes).unwrap();
    let terrain = fixture.sim.resolved_terrain.as_ref().unwrap().clone();
    let mut runtime = crate::sim::runtime::SimRuntime {
        simulation: std::mem::replace(&mut fixture.sim, Simulation::new()),
        resources: crate::sim::runtime::SimResources {
            rules: fixture.rules.clone(),
            overlay_registry: fixture.overlays.clone(),
            terrain_template: Some(terrain),
            ..crate::sim::runtime::SimResources::empty()
        },
    };
    let prepared = crate::app::persistence::PreparedLoad::from_repository(
        crate::app::persistence::LoadPreparationView::from_runtime(
            &repository,
            Some(&runtime),
            Some(0),
        ),
        &path,
    )
    .expect("paid production world restores through the app owner");
    let committed = prepared.commit_into(&mut runtime);
    fixture.projection.restore_order(committed.sidebar_order);
    fixture.sim = runtime.simulation;
}

fn paid_ready_fixture() -> Option<Fixture> {
    let Fixture {
        mut sim,
        rules,
        overlays,
        owner,
        input,
        projection,
    } = stock_base_fixture(
        "Americans",
        &[("GACNST", 4), ("GAPOWR", 11), ("GAPILE", 18)],
        false,
    )?;

    let starting_credits = production::credits_for_owner(&sim, "Americans");
    let total_cost: i32 = ["GAPOWR", "GAPILL"]
        .iter()
        .map(|name| sim.cost_of(owner, rules.object(name).unwrap(), &rules))
        .sum();
    assert!(production::enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "GAPOWR",
        FrameEffects::default(),
        false,
    ));
    assert!(production::enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "GAPILL",
        FrameEffects::default(),
        false,
    ));
    // A fixture timeout, not an expected native completion frame. Both strips
    // run their real timers and charge the same House economy during these frames.
    for _ in 0..4096 {
        sim.advance_app_frame(
            &[],
            Some(&rules),
            Some(&overlays),
            67,
            TickLane::Ordinary,
            None,
            FrameEffects::default(),
        )
        .expect("ordinary paid production frame");
        if production::ready_buildings_for_owner(&sim, &rules, "Americans").len() == 2 {
            break;
        }
    }
    let queue = production::queue_view_for_owner(&sim, &rules, "Americans");
    assert_eq!(queue.len(), 2, "one real object in each structure strip");
    assert!(queue.iter().all(|item| item.state == BuildQueueState::Done));
    assert_eq!(
        production::credits_for_owner(&sim, "Americans"),
        starting_credits - total_cost,
        "the completed objects must have been paid through the House economy"
    );
    assert_eq!(sim.houses[&owner].economy.spent_credits(), total_cost);

    Some(Fixture {
        sim,
        rules,
        overlays,
        owner,
        input,
        projection,
    })
}

fn refresh_fixture_sidebar(
    fixture: &mut Fixture,
    tab: SidebarTab,
) -> crate::ui::sidebar::SidebarView {
    let owner_name = fixture.sim.resolve(fixture.owner).to_owned();
    let queue = production::queue_view_for_owner(&fixture.sim, &fixture.rules, &owner_name);
    let ready = production::ready_buildings_for_owner(&fixture.sim, &fixture.rules, &owner_name);
    let options =
        production::all_build_options_for_owner(&fixture.sim, &fixture.rules, &owner_name);
    let candidates = cameo_candidates(
        &fixture.sim,
        &fixture.rules,
        None,
        fixture.owner,
        &options,
        &ready,
        &[],
    );
    let layout_spec = SidebarChromeLayoutSpec::stock();
    let visible_rows = crate::ui::sidebar::compute_layout_with_spec(layout_spec, 800.0, 1200.0, 0)
        .side2_tile_count;
    // Use the production Sidebar seed/pending-additions boundary. Eligibility
    // changes cannot admit a new cameo without its real House lifecycle pass.
    let seed_additions = fixture
        .projection
        .needs_cameo_seed(fixture.owner)
        .then(|| production::initial_construction_options(&fixture.sim, &fixture.rules))
        .unwrap_or_default();
    fixture
        .projection
        .reconcile_cameos(fixture.owner, &candidates, visible_rows, &seed_additions);
    fixture
        .projection
        .reconcile_factory_links(&fixture.sim, fixture.owner);
    fixture
        .projection
        .advance_cameo_requests(&fixture.sim, fixture.owner);
    let armed = fixture
        .input
        .targeting_mode
        .as_ref()
        .and_then(TargetingMode::as_building_placement)
        .map(|name| ArmedSidebarEntry::BuildingPlacement(name.to_owned()));
    let (power_produced, power_drained) =
        production::power_balance_for_owner(&fixture.sim, &fixture.rules, &owner_name);
    let factory_statuses = fixture
        .projection
        .factory_statuses(&fixture.sim, fixture.owner);
    build_sidebar_view_with_spec(
        layout_spec,
        800.0,
        1200.0,
        tab,
        production::credits_for_owner(&fixture.sim, &owner_name),
        power_produced,
        power_drained,
        None,
        &queue,
        &factory_statuses,
        &options,
        &ready,
        armed.as_ref(),
        &[],
        Some(&fixture.sim.interner),
        &[],
        &SidebarGadgetState::new(),
        None,
        None,
        None,
        None,
        [None; 2],
        fixture.projection.cameo_strips(),
    )
}

fn press_cameo(
    fixture: &mut Fixture,
    name: &str,
    tab: SidebarTab,
    press: CameoPress,
) -> CameoClick {
    let view = refresh_fixture_sidebar(fixture, tab);
    let owner_name = fixture.sim.resolve(fixture.owner);
    let queue = production::queue_view_for_owner(&fixture.sim, &fixture.rules, owner_name);
    let ready = production::ready_buildings_for_owner(&fixture.sim, &fixture.rules, owner_name);
    let slot = view
        .items
        .iter()
        .position(|item| item.type_id == name)
        .unwrap_or_else(|| panic!("retail {name} on retained {tab:?} strip"));
    let item = &view.items[slot];
    if name == "GAPOWR"
        && fixture
            .sim
            .production
            .factories
            .view(fixture.owner, ProductionCategory::Building)
            .and_then(|factory| factory.complete_object())
            .is_some_and(|object| fixture.sim.interner.resolve(object.type_id) == "GAPILE")
    {
        assert!(
            item.is_ready,
            "StripDraw6A9822 reads the same rearmed Factory as input"
        );
        assert!(!item.is_on_hold);
        assert_eq!(
            item.queued_count, 0,
            "the separate House/type badge has no old Power product"
        );
    }
    if ready
        .iter()
        .any(|ready| fixture.sim.resolve(ready.type_id) == name)
    {
        assert!(item.is_ready, "the actual completed factory draws Ready");
    }
    let mut list = GadgetList::new(ListId(1));
    let handles: Vec<_> = view
        .items
        .iter()
        .enumerate()
        .map(|(slot, item)| {
            let rect = item.rect;
            list.add_tail(GadgetSpec::cameo(
                GadgetRect::new(
                    rect.x.round() as i32,
                    rect.y.round() as i32,
                    rect.w.round() as i32,
                    rect.h.round() as i32,
                ),
                ID_CAMEO_BASE + slot as u16,
            ))
        })
        .collect();
    let x = (item.rect.x + item.rect.w / 2.0).round() as i32;
    let y = (item.rect.y + item.rect.h / 2.0).round() as i32;
    let mut focus = FocusState::new();
    let mut output = TickOutput::default();
    let (is_right, shift) = match press {
        CameoPress::Left => (false, false),
        CameoPress::Right { shift } => (true, shift),
    };
    let result = tick(
        &mut list,
        &mut focus,
        &GadgetInput {
            queued_key: if is_right { KEY_RMB_DOWN } else { KEY_LMB_DOWN },
            event_x: x,
            event_y: y,
            mouse_x: x,
            mouse_y: y,
            left_held: !is_right,
            right_held: is_right,
            shift,
            ..Default::default()
        },
        &mut output,
    );
    let right_flag = if is_right { RESULT_RIGHT } else { 0 };
    assert_eq!(
        result,
        RESULT_BUTTON | right_flag | (ID_CAMEO_BASE + slot as u16)
    );
    assert_eq!(output.consumed_by, Some(handles[slot]));
    let fired_slot = ((result & !(RESULT_BUTTON | RESULT_RIGHT)) - ID_CAMEO_BASE) as usize;
    let action = hit_test_item(&view.items[fired_slot], (result & RESULT_RIGHT) != 0, shift);
    let SidebarAction::CameoPress {
        type_id,
        right,
        shift,
        at_build_limit,
    } = action
    else {
        panic!("build gadget must reach the ordinary cameo decision");
    };
    assert_eq!(type_id, name);
    let dispatched_press = if right {
        CameoPress::Right { shift }
    } else {
        CameoPress::Left
    };
    assert_eq!(dispatched_press, press);
    let category = production::category_for_object(fixture.rules.object(&type_id).unwrap());
    let click = sidebar_eva::cameo_click(
        &queue,
        category,
        fixture.sim.interner.get(&type_id),
        fixture
            .projection
            .cameo_factory(
                &fixture.sim,
                fixture.owner,
                fixture.sim.interner.get(&type_id).unwrap(),
            )
            .map(|factory| factory.cameo_state()),
        fixture
            .projection
            .cameo_request(fixture.sim.interner.get(&type_id).unwrap()),
        dispatched_press,
        at_build_limit,
    );
    if click.clear_building_placement {
        fixture.input.cancel_building_placement();
    }
    fixture.projection.record_cameo_click(
        &fixture.sim,
        &fixture.rules,
        fixture.owner,
        fixture.sim.interner.get(&type_id).unwrap(),
        click,
    );
    // Cameos fire on DOWN; the real UP edge must not dispatch another entry.
    let release = tick(
        &mut list,
        &mut focus,
        &GadgetInput {
            queued_key: if is_right { KEY_RMB_UP } else { KEY_LMB_UP },
            event_x: x,
            event_y: y,
            mouse_x: x,
            mouse_y: y,
            shift,
            ..Default::default()
        },
        &mut output,
    );
    assert_eq!(release & RESULT_BUTTON, 0);
    click
}

fn press_ready_cameo(fixture: &mut Fixture, name: &str, tab: SidebarTab) -> bool {
    let click = press_cameo(fixture, name, tab, CameoPress::Left);
    assert!(
        click.sound,
        "ready presses retain the ordinary GUI sound request"
    );
    assert_eq!(click.eva, None);
    assert_eq!(click.order, Some(CameoOrder::Place));
    assert!(!click.clear_building_placement);
    let type_id = fixture
        .projection
        .cameo_factory(
            &fixture.sim,
            fixture.owner,
            fixture.sim.interner.get(name).unwrap(),
        )
        .unwrap()
        .complete_object()
        .unwrap()
        .type_id;
    let product = production::ready_building_entity_for_owner(
        &fixture.sim,
        &fixture.rules,
        fixture.owner,
        type_id,
    )
    .expect("completed product comes from the authoritative House factory");
    production::object_find_factory(&fixture.sim, &fixture.rules, product, false, false)
        .expect("the actual held object finds its healthy retail builder");
    fixture.input.begin_building_placement(
        fixture.sim.interner.resolve(type_id),
        product,
        fixture.owner,
    )
}

fn arm_gapowr_with_preview(fixture: &mut Fixture) -> production::BuildingPlacementPreview {
    assert!(press_ready_cameo(fixture, "GAPOWR", SidebarTab::Building));
    let preview = production::placement_preview_for_owner_with_overlays(
        &fixture.sim,
        &fixture.rules,
        "Americans",
        "GAPOWR",
        8,
        4,
        Some(&fixture.overlays),
    )
    .expect("armed ready factory supplies its real placement preview");
    assert!(preview.valid, "clear adjacent healthy-yard placement site");
    fixture.input.building_placement_preview = Some(preview.clone());
    preview
}

fn ready_identities(fixture: &Fixture) -> [(InternedId, u64); 2] {
    ["GAPOWR", "GAPILL"].map(|name| {
        let type_id = fixture.sim.interner.get(name).unwrap();
        let product = production::ready_building_entity_for_owner(
            &fixture.sim,
            &fixture.rules,
            fixture.owner,
            type_id,
        )
        .expect("the paid factory must still hold its completed object");
        (type_id, product)
    })
}

fn ready_product(fixture: &Fixture, name: &str) -> u64 {
    production::ready_building_entity_for_owner(
        &fixture.sim,
        &fixture.rules,
        fixture.owner,
        fixture.sim.interner.get(name).unwrap(),
    )
    .expect("completed product identity remains owned by its House factory")
}

fn submit_ready_building(
    fixture: &mut Fixture,
    name: &str,
    tab: SidebarTab,
    cell: (u16, u16),
) -> CommandEnvelope {
    assert!(press_ready_cameo(fixture, name, tab));
    submit_current_building(fixture, cell)
}

fn submit_current_building(fixture: &mut Fixture, cell: (u16, u16)) -> CommandEnvelope {
    let name = fixture
        .input
        .targeting_mode
        .as_ref()
        .and_then(TargetingMode::as_building_placement)
        .expect("the current actual product type supplies PLACE")
        .to_string();
    let preview = production::placement_preview_for_owner_with_overlays(
        &fixture.sim,
        &fixture.rules,
        "Americans",
        &name,
        cell.0,
        cell.1,
        Some(&fixture.overlays),
    )
    .expect("ready object's real preview");
    assert!(
        preview.in_build_area && preview.foundation_visible,
        "native local proximity/visibility gates admit the PLACE event"
    );
    fixture.input.building_placement_preview = Some(preview);
    assert_eq!(
        fixture.input.building_placement_issuer(),
        Some(fixture.owner)
    );
    let command = CommandEnvelope::new(
        fixture.input.building_placement_issuer().unwrap(),
        fixture.sim.session.tick + 1,
        Command::PlaceReadyBuilding {
            type_id: fixture.sim.interner.get(&name).unwrap(),
            rx: cell.0,
            ry: cell.1,
        },
    );
    fixture.input.submit_building_placement();
    assert!(fixture.input.targeting_mode.is_none());
    assert!(fixture.input.building_placement_preview.is_none());
    assert!(fixture.input.building_placement_issuer().is_none());
    command
}

fn cancel_envelope(fixture: &Fixture, name: &str) -> CommandEnvelope {
    CommandEnvelope::new(
        fixture.owner,
        fixture.sim.session.tick + 1,
        Command::CancelProductionByType {
            type_id: fixture.sim.interner.get(name).unwrap(),
            all: false,
        },
    )
}

fn advance_commands(fixture: &mut Fixture, commands: &[CommandEnvelope]) -> SimFrameOutput {
    fixture.sim.queue_commands(commands.iter().cloned());
    let due = fixture.sim.take_due_commands();
    fixture
        .sim
        .advance_app_frame(
            &due,
            Some(&fixture.rules),
            Some(&fixture.overlays),
            67,
            TickLane::Ordinary,
            None,
            FrameEffects::default(),
        )
        .expect("ordinary authoritative command frame")
}

#[test]
fn pending_produce_right_click_suspends_on_arrival_without_duplicate_orders() {
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/sidebar_oracle/draw_availability.json",
    ))
    .unwrap();
    for idle in native["pending_right_clicks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|control| control["before"] == 0)
    {
        let name = idle["source"].as_str().unwrap();
        let tab = SidebarTab::all()[idle["tab"].as_u64().unwrap() as usize];
        let Some(mut fixture) = stock_base_fixture(
            "Americans",
            &[
                ("GACNST", 4),
                ("GAPOWR", 10),
                ("GAPILE", 15),
                ("GAREFN", 21),
                ("GAWEAP", 27),
            ],
            false,
        ) else {
            return;
        };
        let category = production::category_for_object(fixture.rules.object(name).unwrap());
        let right_idle = press_cameo(&mut fixture, name, tab, CameoPress::Right { shift: false });
        assert_eq!(right_idle, CameoClick::default());
        assert_eq!(idle["after"], 0);
        assert!(idle["events"].as_array().unwrap().is_empty());
        let type_id = fixture.sim.interner.get(name).unwrap();
        let left = press_cameo(&mut fixture, name, tab, CameoPress::Left);
        assert_eq!(left.order, Some(CameoOrder::Produce));
        crate::app::input::commands::schedule_command_in_sim(
            &mut fixture.sim,
            "Americans",
            Command::QueueProduction { type_id },
        )
        .unwrap();
        assert!(
            fixture
                .projection
                .cameo_factory(&fixture.sim, fixture.owner, type_id)
                .is_none()
        );

        let right = press_cameo(&mut fixture, name, tab, CameoPress::Right { shift: false });
        assert_eq!(
            right.order,
            Some(CameoOrder::Suspend),
            "{name}: unlinked request1"
        );
        let running = native["pending_right_clicks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|control| control["source"] == name && control["before"] == 1)
            .unwrap();
        assert_eq!(running["events"].as_array().unwrap().len(), 1);
        assert_eq!(running["events"][0]["kind"], 15);
        assert!(right.sound && !right.clear_building_placement);
        assert_eq!(right.eva, Some(sidebar_eva::EVA_ON_HOLD));
        crate::app::input::commands::schedule_command_in_sim(
            &mut fixture.sim,
            "Americans",
            Command::SuspendProduction { category },
        )
        .unwrap();
        assert_eq!(
            fixture.projection.cameo_request(type_id),
            Some(CameoRequestState::Held)
        );
        assert_eq!(running["after"], 2);
        let repeated = press_cameo(&mut fixture, name, tab, CameoPress::Right { shift: true });
        assert_eq!(
            repeated,
            CameoClick::default(),
            "{name}: request2 adds no second event"
        );
        let held = native["pending_right_clicks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|control| control["source"] == name && control["before"] == 2)
            .unwrap();
        assert_eq!(held["after"], running["after"]);
        assert!(held["events"].as_array().unwrap().is_empty());
        assert_eq!(fixture.sim.pending_command_snapshot().len(), 2);

        let owner = fixture.owner;
        let frame = advance_commands(&mut fixture, &[]);
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
        let factory = fixture
            .sim
            .production
            .factories
            .view(fixture.owner, category)
            .unwrap();
        assert!(
            factory.object.is_some(),
            "the original PRODUCE still executes"
        );
        assert_eq!(factory.cameo_state(), BuildQueueState::Paused);
        let resume = press_cameo(&mut fixture, name, tab, CameoPress::Left);
        assert_eq!(resume.order, Some(CameoOrder::Produce));
        let command = CommandEnvelope::new(
            owner,
            fixture.sim.session.tick + 1,
            Command::QueueProduction { type_id },
        );
        let frame = advance_commands(&mut fixture, &[command]);
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
        assert_eq!(
            fixture
                .sim
                .production
                .factories
                .view(fixture.owner, category)
                .unwrap()
                .cameo_state(),
            BuildQueueState::Building
        );
        // Scheduling alone cannot mutate the retained request. The local
        // click owner and Factory lifecycle keep their separate authorities.
        let request = fixture.projection.cameo_request(type_id);
        crate::app::input::commands::schedule_command_in_sim(
            &mut fixture.sim,
            "Americans",
            Command::QueueProduction { type_id },
        )
        .unwrap();
        assert_eq!(fixture.projection.cameo_request(type_id), request);
    }
}

#[test]
fn pending_alternating_clicks_retain_native_hold_request_and_resume_on_arrival() {
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/sidebar_oracle/draw_availability.json",
    ))
    .unwrap();
    for history in native["pending_click_histories"].as_array().unwrap() {
        let name = history["source"].as_str().unwrap();
        let Some(mut fixture) = stock_base_fixture(
            "Americans",
            &[
                ("GACNST", 4),
                ("GAPOWR", 10),
                ("GAPILE", 15),
                ("GAREFN", 21),
                ("GAWEAP", 27),
            ],
            false,
        ) else {
            return;
        };
        let category = production::category_for_object(fixture.rules.object(name).unwrap());
        for step in history["steps"].as_array().unwrap() {
            let clicked_name = step["source"].as_str().unwrap();
            let clicked_tab = SidebarTab::all()[step["tab"].as_u64().unwrap() as usize];
            let type_id = fixture.sim.interner.get(clicked_name).unwrap();
            let clicked_category =
                production::category_for_object(fixture.rules.object(clicked_name).unwrap());
            let press = match step["press"].as_str().unwrap() {
                "left" => CameoPress::Left,
                "right" => CameoPress::Right { shift: false },
                _ => unreachable!(),
            };
            let click = press_cameo(&mut fixture, clicked_name, clicked_tab, press);
            let event = match click.order {
                Some(CameoOrder::Produce) => Some((14, Command::QueueProduction { type_id })),
                Some(CameoOrder::Suspend) => Some((
                    15,
                    Command::SuspendProduction {
                        category: clicked_category,
                    },
                )),
                None => None,
                _ => panic!("unexpected unlinked pending click: {click:?}"),
            };
            assert_eq!(
                serde_json::json!(
                    event
                        .as_ref()
                        .map(|(kind, _)| *kind)
                        .into_iter()
                        .collect::<Vec<_>>()
                ),
                step["events"],
                "{name}: {press:?} must emit the native events"
            );
            if let Some((_, command)) = event {
                crate::app::input::commands::schedule_command_in_sim(
                    &mut fixture.sim,
                    "Americans",
                    command,
                )
                .unwrap();
            } else {
                assert_eq!(click, CameoClick::default());
            }
            let mut expected: Vec<_> = step["requests"]
                .as_array()
                .unwrap()
                .iter()
                .map(|request| {
                    let id = fixture
                        .sim
                        .interner
                        .get(request["source"].as_str().unwrap())
                        .unwrap();
                    let state = match request["request_state"].as_u64().unwrap() {
                        1 => CameoRequestState::Started,
                        2 => CameoRequestState::Held,
                        _ => unreachable!(),
                    };
                    (id, state)
                })
                .collect();
            let mut actual: Vec<_> = fixture
                .projection
                .cameo_strips()
                .requested_ids()
                .filter_map(|id| match id {
                    CameoId::Object(id) => fixture
                        .projection
                        .cameo_request(id)
                        .map(|state| (id, state)),
                    CameoId::SuperWeapon(_) => None,
                })
                .collect();
            expected.sort_by_key(|(id, _)| *id);
            actual.sort_by_key(|(id, _)| *id);
            assert_eq!(
                actual, expected,
                "{name}: {press:?} must retain the native request word"
            );
        }
        let frame = advance_commands(&mut fixture, &[]);
        let owner = fixture.owner;
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
        assert_eq!(
            fixture
                .sim
                .production
                .factories
                .view(owner, category)
                .unwrap()
                .cameo_state(),
            BuildQueueState::Building,
            "the final native PRODUCE resumes the factory without a fourth SUSPEND"
        );
    }
}

/// Admission summary, not an ordered cursor journal. The production cache
/// owner supplies the link-dependent House prefix during output consumption.
fn placement_facts(frame: &SimFrameOutput) -> Vec<LifecycleOutput> {
    frame
        .lifecycle_outputs
        .iter()
        .copied()
        .filter_map(|output| match output {
            LifecycleOutput::BuildingPlacementRetry { .. }
            | LifecycleOutput::BuildingPlacementSucceeded { .. } => Some(output),
            LifecycleOutput::ProductionFactoryAbandoning {
                owner,
                category: ProductionCategory::Building | ProductionCategory::Defense,
                ..
            } => Some(LifecycleOutput::BuildingProductionAbandoned { owner }),
            _ => None,
        })
        .collect()
}

fn consume_frame_outputs(
    fixture: &mut Fixture,
    frame: &SimFrameOutput,
    local_house: Option<InternedId>,
) -> usize {
    frame
        .lifecycle_outputs
        .iter()
        .filter(|output| consume_lifecycle_output(fixture, output, local_house))
        .count()
}

fn consume_lifecycle_output(
    fixture: &mut Fixture,
    output: &LifecycleOutput,
    local_house: Option<InternedId>,
) -> bool {
    if let Some(cursor_clear) = fixture
        .projection
        .apply_lifecycle_output(output, local_house)
    {
        fixture
            .input
            .apply_building_placement_output(&cursor_clear, local_house);
    }
    fixture
        .input
        .apply_building_placement_output(output, local_house)
}

fn start_mixed_infantry_queue(fixture: &mut Fixture) -> (u64, u64) {
    let commands: Vec<_> = ["E1", "ENGINEER"]
        .map(|name| {
            CommandEnvelope::new(
                fixture.owner,
                fixture.sim.session.tick + 1,
                Command::QueueProduction {
                    type_id: fixture.sim.interner.get(name).unwrap(),
                },
            )
        })
        .into();
    let frame = advance_commands(fixture, &commands);
    consume_frame_outputs(fixture, &frame, Some(fixture.owner));
    // Seed the same retained projection used by gadget input and Draw.
    let click = press_cameo(fixture, "E1", SidebarTab::Infantry, CameoPress::Left);
    assert_eq!(click.order, Some(CameoOrder::Produce));
    let factory = fixture
        .sim
        .production
        .factories
        .view(fixture.owner, ProductionCategory::Infantry)
        .unwrap();
    assert_eq!(fixture.sim.resolve(factory.object.unwrap().type_id), "E1");
    (
        factory.construction_order(),
        factory.object.unwrap().entity_id.unwrap(),
    )
}

/// Stock SPY519FF8 -> NATECH4573DC/4573EA -> the same frame's House4F9280
/// admission -> retained Infantry strip -> Select/paid Factory -> CIVAN exit.
/// Native control/data: tools/spy_infiltration_oracle; this test exercises the
/// connected production route, not native movement or Factory timing goldens.
#[test]
fn retail_spy_stolen_soviet_tech_admits_and_delivers_chrono_ivan() {
    use crate::sim::spy_infiltration::InfiltrationEffect;

    let Some(mut fixture) = stock_base_fixture(
        "Americans",
        &[
            ("GACNST", 4),
            ("GAPOWR", 11),
            ("GAPILE", 18),
            ("GATECH", 25),
        ],
        false,
    ) else {
        return;
    };
    let owner = fixture.owner;
    fixture.sim.session.game_mode_nonzero = true;
    let enemy = fixture.sim.intern("Russians");
    // The supplied rival base is idle. Both House array entries are human,
    // so unrelated AI construction cannot manufacture the missing request.
    let mut house = HouseState::new(enemy, 1, Some(enemy), true, 0, 10);
    house.project_country_mults(&fixture.rules, &fixture.sim.interner);
    fixture.sim.houses.insert(enemy, house);
    fixture.sim.session.house_order.push(enemy);
    let laboratory = fixture
        .sim
        .spawn_object_at_height("NATECH", "Russians", 18, 20, 0, 0, &fixture.rules)
        .expect("healthy enemy stock Battle Lab");
    let spy = fixture
        .sim
        .spawn_object_at_height("SPY", "Americans", 14, 20, 0, 0, &fixture.rules)
        .expect("stock Spy starts outside the Battle Lab");
    let laboratory_health = fixture
        .sim
        .entities()
        .get(laboratory)
        .unwrap()
        .health
        .current();
    let ivan = fixture.sim.interner.get("CIVAN").unwrap();
    assert!(fixture.rules.is_build_tech("NATECH"));
    assert_eq!(
        fixture
            .rules
            .object("NATECH")
            .unwrap()
            .ai_base_planning_side,
        1
    );
    assert!(
        fixture
            .rules
            .object("CIVAN")
            .unwrap()
            .requires_stolen_soviet_tech
    );
    assert!(
        fixture
            .sim
            .infiltrator_building_action(spy, laboratory, &fixture.rules)
    );

    // Drain constructor/discovery/Super requests before seeding the sidebar.
    // Waiting for quiet outputs avoids reaching the reward through a stale
    // initial House+1FC instead of the infiltration's own writer.
    let mut quiet_frames = 0;
    for _ in 0..64 {
        let frame = advance_commands(&mut fixture, &[]);
        let local_recheck = frame.lifecycle_outputs.iter().any(|output| {
            matches!(output, LifecycleOutput::SidebarRecalculated { owner: reached } if *reached == owner)
        });
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
        quiet_frames = if local_recheck { 0 } else { quiet_frames + 1 };
        if quiet_frames == 2 {
            break;
        }
    }
    assert_eq!(
        quiet_frames, 2,
        "the supplied base must settle before infiltration"
    );
    assert!(fixture.projection.needs_cameo_seed(owner));
    let initial = refresh_fixture_sidebar(&mut fixture, SidebarTab::Infantry);
    assert!(
        initial
            .items
            .iter()
            .any(|item| item.type_id == "E1" && item.enabled)
    );
    assert!(!initial.items.iter().any(|item| item.type_id == "CIVAN"));
    assert!(!fixture.projection.needs_cameo_seed(owner));
    assert!(
        production::all_build_options_for_owner(&fixture.sim, &fixture.rules, "Americans")
            .iter()
            .any(|option| option.type_id == ivan && !option.visible_in_sidebar()),
        "the stock Soviet stolen-tech gate must be closed before entry"
    );

    let capture = CommandEnvelope::new(
        owner,
        fixture.sim.session.tick + 1,
        Command::CaptureBuilding {
            engineer_id: spy,
            target_building_id: laboratory,
        },
    );
    let mut entered = false;
    for step in 0..600 {
        let frame = advance_commands(
            &mut fixture,
            if step == 0 {
                std::slice::from_ref(&capture)
            } else {
                &[]
            },
        );
        let tech_entry = frame.sound_events.iter().any(|event| {
            matches!(event, SimSoundEvent::BuildingInfiltrated {
                owner: target_owner,
                infiltrator,
                effect: InfiltrationEffect::Tech,
                ..
            } if *target_owner == enemy && *infiltrator == owner)
        });
        let recheck = frame.lifecycle_outputs.iter().position(|output| {
            matches!(output, LifecycleOutput::SidebarRecalculated { owner: reached } if *reached == owner)
        });
        if !tech_entry {
            assert!(
                recheck.is_none(),
                "an unrelated local request would mask the Spy writer"
            );
        }
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
        let view = refresh_fixture_sidebar(&mut fixture, SidebarTab::Infantry);
        if !tech_entry {
            assert!(!view.items.iter().any(|item| item.type_id == "CIVAN"));
            continue;
        }
        assert!(
            fixture
                .sim
                .entities()
                .get(spy)
                .is_none_or(|actor| !actor.lifecycle.object_alive()),
            "the real entry consumes the Spy before the House suffix"
        );
        assert!(
            production::all_build_options_for_owner(&fixture.sim, &fixture.rules, "Americans")
                .iter()
                .any(|option| option.type_id == ivan && option.enabled),
            "the entry must actually grant Soviet tech before testing sidebar admission"
        );
        let addition = frame.lifecycle_outputs.iter().position(|output| {
            matches!(output, LifecycleOutput::SidebarConstructionOption { owner: reached, type_id } if *reached == owner && *type_id == ivan)
        });
        assert!(
            addition.is_some(),
            "stock Spy stole Soviet tech, but its entry frame did not admit CIVAN through House+1FC"
        );
        assert!(recheck.is_some_and(|recheck| addition.unwrap() < recheck));
        assert!(
            view.items
                .iter()
                .any(|item| item.type_id == "CIVAN" && item.enabled)
        );
        assert!(!fixture.projection.needs_cameo_seed(owner));
        entered = true;
        break;
    }
    assert!(
        entered,
        "the ordinary CaptureBuilding order must reach the Battle Lab"
    );
    let target = fixture.sim.entities().get(laboratory).unwrap();
    assert_eq!(target.owner(), enemy);
    assert_eq!(target.health.current(), laboratory_health);

    // The exact newly retained entry reaches the ordinary gadget and Select
    // decision; dispatch its PRODUCE result through the normal command queue.
    let click = press_cameo(
        &mut fixture,
        "CIVAN",
        SidebarTab::Infantry,
        CameoPress::Left,
    );
    assert_eq!(click.order, Some(CameoOrder::Produce));
    let credits_before = production::credits_for_owner(&fixture.sim, "Americans");
    let spent_before = fixture.sim.houses[&owner].economy.spent_credits();
    let cost = fixture.sim.cost_of(
        owner,
        fixture.rules.object("CIVAN").unwrap(),
        &fixture.rules,
    );
    let produce = CommandEnvelope::new(
        owner,
        fixture.sim.session.tick + 1,
        Command::QueueProduction { type_id: ivan },
    );
    let mut held = None;
    let mut delivered = false;
    for step in 0..4096 {
        let frame = advance_commands(
            &mut fixture,
            if step == 0 {
                std::slice::from_ref(&produce)
            } else {
                &[]
            },
        );
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
        refresh_fixture_sidebar(&mut fixture, SidebarTab::Infantry);
        if let Some(product) = fixture
            .sim
            .production
            .factories
            .view(owner, ProductionCategory::Infantry)
            .and_then(|factory| factory.object)
        {
            assert_eq!(product.type_id, ivan);
            let id = product
                .entity_id
                .expect("the Factory holds its actual CIVAN");
            assert!(held.is_none_or(|previous| previous == id));
            held = Some(id);
        }
        if held.is_some_and(|id| {
            fixture.sim.entities().get(id).is_some_and(|product| {
                product.lifecycle.object_alive() && !product.lifecycle.in_limbo()
            })
        }) {
            delivered = true;
            break;
        }
    }
    assert!(
        delivered,
        "the paid held CIVAN must leave the real barracks"
    );
    assert!(production::queue_view_for_owner(&fixture.sim, &fixture.rules, "Americans").is_empty());
    assert_eq!(
        production::credits_for_owner(&fixture.sim, "Americans"),
        credits_before - cost
    );
    assert_eq!(
        fixture.sim.houses[&owner].economy.spent_credits(),
        spent_before + cost
    );
}

/// House4F92E2 alone recalculates membership. Draw6A98D3 clears DARKEN
/// for the linked paid head; queued Select6AB656 checks its build limit
/// independently of Draw's unavailable producer. This follows the ordinary
/// command/frame spine through physical retail reverse buildup and Limbo.
#[test]
#[ignore = "slow; run when changing what it covers"]
fn selling_barracks_retains_paid_clock_and_queued_dark_cameo_until_house_recheck() {
    let Some(mut fixture) = stock_base_fixture(
        "Russians",
        &[
            ("NACNST", 4),
            ("NAPOWR", 11),
            ("NAHAND", 18),
            ("NARADR", 25),
        ],
        true,
    ) else {
        return;
    };
    assert!(fixture.rules.has_buildup("NAHAND"));
    let owner = fixture.owner;
    let barracks = fixture
        .sim
        .entities()
        .values()
        .find(|entity| fixture.sim.resolve(entity.type_ref()) == "NAHAND")
        .unwrap()
        .stable_id();
    for _ in 0..8 {
        let frame = advance_commands(&mut fixture, &[]);
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
    }
    let initial = refresh_fixture_sidebar(&mut fixture, SidebarTab::Infantry);
    assert!(
        initial
            .items
            .iter()
            .any(|item| item.type_id == "E2" && item.enabled)
    );
    assert!(
        initial
            .items
            .iter()
            .any(|item| item.type_id == "FLAKT" && item.enabled)
    );
    let commands = ["E2", "E2", "FLAKT"].map(|name| {
        CommandEnvelope::new(
            owner,
            fixture.sim.session.tick + 1,
            Command::QueueProduction {
                type_id: fixture.sim.interner.get(name).unwrap(),
            },
        )
    });
    let frame = advance_commands(&mut fixture, &commands);
    consume_frame_outputs(&mut fixture, &frame, Some(owner));
    for _ in 0..8 {
        let frame = advance_commands(&mut fixture, &[]);
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
    }
    let before = refresh_fixture_sidebar(&mut fixture, SidebarTab::Infantry);
    let head = before
        .items
        .iter()
        .find(|item| item.type_id == "E2")
        .unwrap();
    assert!(head.progress > 0.0 && head.progress < 1.0);
    assert_eq!(head.queued_count, 2);
    assert!(fixture.sim.houses[&owner].economy.spent_credits() > 0);
    let identities = fixture.projection.cameo_strips().saved_identities();
    let sale = CommandEnvelope::new(
        owner,
        fixture.sim.session.tick + 1,
        Command::SellBuilding {
            entity_id: barracks,
        },
    );
    let mut frame = advance_commands(&mut fixture, &[sale]);
    let mut live_sale_frames = 0;
    let mut saw_clock = false;
    let mut saw_ready = false;
    loop {
        let on_map = fixture
            .sim
            .entities()
            .get(barracks)
            .is_some_and(|entity| !entity.in_limbo());
        if !on_map {
            break;
        }
        assert!(frame.lifecycle_outputs.iter().all(|output| {
            !matches!(output, LifecycleOutput::SidebarRecalculated { owner: reached } if *reached == owner)
        }), "settled sale does not itself request a House pass before map exit");
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
        let view = refresh_fixture_sidebar(&mut fixture, SidebarTab::Infantry);
        assert_eq!(
            fixture.projection.cameo_strips().saved_identities(),
            identities
        );
        let head = view.items.iter().find(|item| item.type_id == "E2").unwrap();
        let tail = view
            .items
            .iter()
            .find(|item| item.type_id == "FLAKT")
            .unwrap();
        assert!(head.enabled && !head.is_on_hold);
        if head.is_ready {
            // IsComplete6A9822/READY6A983B wins over the stopped rate/Hold
            // tail. A complete product may wait while FindFactory rejects
            // Selling, without losing its retained Factory link.
            assert!(!head.is_building_this_type);
            assert_eq!(head.progress, 1.0);
            if !saw_ready {
                assert_eq!(
                    press_cameo(&mut fixture, "E2", SidebarTab::Infantry, CameoPress::Left).order,
                    Some(CameoOrder::Place),
                    "a stopped complete mobile reaches Select's producer query/event path"
                );
            }
            saw_ready = true;
        } else {
            assert!(head.is_building_this_type);
            saw_clock = true;
        }
        assert!(head.progress > 0.0);
        assert_eq!(head.queued_count, 2);
        assert!(!tail.enabled && !tail.at_build_limit && !tail.is_building_this_type);
        assert_eq!(tail.queued_count, 1);
        live_sale_frames += 1;
        if live_sale_frames == 1 {
            assert_eq!(
                press_cameo(
                    &mut fixture,
                    "E2",
                    SidebarTab::Infantry,
                    CameoPress::Right { shift: false }
                )
                .order,
                Some(CameoOrder::Suspend)
            );
            assert_eq!(
                press_cameo(
                    &mut fixture,
                    "FLAKT",
                    SidebarTab::Infantry,
                    CameoPress::Right { shift: false }
                )
                .order,
                Some(CameoOrder::Abandon { all: false })
            );
            assert_eq!(
                press_cameo(
                    &mut fixture,
                    "FLAKT",
                    SidebarTab::Infantry,
                    CameoPress::Left
                )
                .order,
                Some(CameoOrder::Produce)
            );
        }
        assert!(
            live_sale_frames < 256,
            "fixture sale must reach ordinary Limbo"
        );
        frame = advance_commands(&mut fixture, &[]);
    }
    consume_frame_outputs(&mut fixture, &frame, Some(owner));
    assert!(
        live_sale_frames > 1,
        "physical buildup keeps the paid Factory live during sale"
    );
    assert!(
        saw_clock && saw_ready,
        "fixture exercises both native linked drawing arms"
    );
    for _ in 0..8 {
        let view = refresh_fixture_sidebar(&mut fixture, SidebarTab::Infantry);
        if view.items.is_empty() {
            assert!(
                production::queue_view_for_owner(&fixture.sim, &fixture.rules, "Russians")
                    .is_empty()
            );
            return;
        }
        let frame = advance_commands(&mut fixture, &[]);
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
    }
    panic!("terminal sale's House +1FC pass must prune the retained Infantry entries");
}

/// Real Select/SUSPEND -> a stock power-building sale -> House power/rate
/// reassessment -> saved-world preparation/commit -> Select/PRODUCE same-head
/// resume -> ordinary delivery. The save is VERA's format, not native SAV.
/// Suspend4C9E60 writes +70; rate pass4CA6E0 changes only +38, so Draw and
/// Select must still see a stopped Factory. Native evidence owners:
/// tools/sidebar_oracle/factory_hold_resume.md and
/// tools/spatial_oracle/{factory_cadence,house_power_consumers}.json.
#[test]
#[ignore = "requires configured retail assets; binds real power-plant sale buildup"]
fn paid_user_hold_stays_stopped_after_a_power_building_sale_refresh() {
    let Some(mut fixture) = stock_base_fixture(
        "Russians",
        &[
            ("NACNST", 4),
            ("NAPOWR", 11),
            ("NAPOWR", 18),
            ("NAHAND", 25),
        ],
        true,
    ) else {
        return;
    };
    assert!(fixture.rules.has_buildup("NAPOWR"));
    // The clear arena has no overlays. Saved-world preparation still requires
    // that empty map authority, as the ordinary map loader supplies it.
    fixture.sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(
        fixture.sim.session.map_width,
        fixture.sim.session.map_height,
    ));
    let owner = fixture.owner;
    for _ in 0..8 {
        let frame = advance_commands(&mut fixture, &[]);
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
    }
    let _ = refresh_fixture_sidebar(&mut fixture, SidebarTab::Infantry);
    let e2 = fixture.sim.interner.get("E2").unwrap();
    let commands = [0, 1].map(|_| {
        CommandEnvelope::new(
            owner,
            fixture.sim.session.tick + 1,
            Command::QueueProduction { type_id: e2 },
        )
    });
    let frame = advance_commands(&mut fixture, &commands);
    consume_frame_outputs(&mut fixture, &frame, Some(owner));
    for _ in 0..8 {
        let frame = advance_commands(&mut fixture, &[]);
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
    }
    let factory = fixture
        .sim
        .production
        .factories
        .view(owner, ProductionCategory::Infantry)
        .unwrap();
    let held_id = factory.object.unwrap().entity_id.unwrap();
    let order = factory.construction_order();
    assert!(factory.progress > 0 && factory.progress < production::PRODUCTION_STEPS);
    let press = press_cameo(
        &mut fixture,
        "E2",
        SidebarTab::Infantry,
        CameoPress::Right { shift: false },
    );
    assert_eq!(press.order, Some(CameoOrder::Suspend));
    assert_eq!(press.eva, Some(sidebar_eva::EVA_ON_HOLD));
    let suspend = CommandEnvelope::new(
        owner,
        fixture.sim.session.tick + 1,
        Command::SuspendProduction {
            category: ProductionCategory::Infantry,
        },
    );
    let frame = advance_commands(&mut fixture, &[suspend]);
    consume_frame_outputs(&mut fixture, &frame, Some(owner));
    let progress = fixture
        .sim
        .production
        .factories
        .view(owner, ProductionCategory::Infantry)
        .unwrap()
        .progress;
    let spent = fixture.sim.houses[&owner].economy.spent_credits();
    assert!(spent > 0);
    let power = fixture
        .sim
        .entities()
        .values()
        .find(|entity| fixture.sim.resolve(entity.type_ref()) == "NAPOWR")
        .unwrap()
        .stable_id();
    let sale = CommandEnvelope::new(
        owner,
        fixture.sim.session.tick + 1,
        Command::SellBuilding { entity_id: power },
    );
    let mut frame = advance_commands(&mut fixture, &[sale]);
    for step in 0..1024 {
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
        let factory = fixture
            .sim
            .production
            .factories
            .view(owner, ProductionCategory::Infantry)
            .unwrap();
        assert_eq!(factory.progress, progress);
        assert_eq!(factory.construction_order(), order);
        assert_eq!(factory.object.unwrap().entity_id, Some(held_id));
        assert_eq!(fixture.sim.houses[&owner].economy.spent_credits(), spent);
        let view = refresh_fixture_sidebar(&mut fixture, SidebarTab::Infantry);
        let head = view.items.iter().find(|item| item.type_id == "E2").unwrap();
        assert!(
            head.is_on_hold && !head.is_building_this_type,
            "user HOLD survives ordinary House rate refresh at sale step {step}"
        );
        assert_eq!(head.queued_count, 2);
        if fixture
            .sim
            .entities()
            .get(power)
            .is_none_or(|entity| entity.in_limbo())
        {
            break;
        }
        assert!(step < 1023, "real power-building sale must finish");
        frame = advance_commands(&mut fixture, &[]);
    }
    fixture.sim.session.map_name = "HOLD-RESUME.MAP".into();
    fixture.sim.session.set_started(true);
    let before_factories = fixture.sim.production.factories.clone();
    let before_wallet = fixture.sim.houses[&owner].economy.clone();
    roundtrip_fixture_save(&mut fixture, "paid-user-hold");
    assert_eq!(
        fixture.sim.production.factories, before_factories,
        "saved Factory authorities"
    );
    assert_eq!(
        fixture.sim.houses[&owner].economy, before_wallet,
        "saved paid wallet"
    );
    // Whole pre/post-load hash equality is not a load invariant: native load
    // resets Scenario RNG and rebuilds Tiberium queues/navigation history.
    // Compare all Factory fields and paid wallet above, then exercise the real
    // loaded world through resume/delivery; no load-state normalization here.
    assert!(fixture.sim.entities().get(held_id).unwrap().in_limbo());
    let restored_view = refresh_fixture_sidebar(&mut fixture, SidebarTab::Infantry);
    let restored = restored_view
        .items
        .iter()
        .find(|item| item.type_id == "E2")
        .unwrap();
    assert!(restored.is_on_hold && !restored.is_building_this_type);
    assert_eq!(restored.queued_count, 2);
    assert_eq!(
        press_cameo(
            &mut fixture,
            "E2",
            SidebarTab::Infantry,
            CameoPress::Right { shift: false }
        )
        .order,
        Some(CameoOrder::Abandon { all: false })
    );
    let resume = press_cameo(&mut fixture, "E2", SidebarTab::Infantry, CameoPress::Left);
    assert_eq!(resume.order, Some(CameoOrder::Produce));
    assert_eq!(resume.eva, Some(sidebar_eva::EVA_TRAINING));
    let command = CommandEnvelope::new(
        owner,
        fixture.sim.session.tick + 1,
        Command::QueueProduction { type_id: e2 },
    );
    let frame = advance_commands(&mut fixture, &[command]);
    consume_frame_outputs(&mut fixture, &frame, Some(owner));
    let factory = fixture
        .sim
        .production
        .factories
        .view(owner, ProductionCategory::Infantry)
        .unwrap();
    assert_eq!(factory.construction_order(), order);
    assert_eq!(factory.object.unwrap().entity_id, Some(held_id));
    assert!(factory.is_running());
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
    assert!(delivered, "resumed same paid E2 must reach the real exit");
    let successor = fixture
        .sim
        .production
        .factories
        .view(owner, ProductionCategory::Infantry)
        .unwrap();
    assert_eq!(successor.construction_order(), order);
    assert_ne!(successor.object.unwrap().entity_id, Some(held_id));
    let queued = production::queue_view_for_owner(&fixture.sim, &fixture.rules, "Russians");
    assert_eq!(queued.len(), 1, "resume did not append a third E2");
    assert_eq!(
        fixture.sim.houses[&owner].economy.spent_credits(),
        fixture
            .sim
            .cost_of(owner, fixture.rules.object("E2").unwrap(), &fixture.rules)
    );
}

fn assert_old_infantry_cameo_released(fixture: &mut Fixture, order: u64) {
    let old_type = fixture.sim.interner.get("E1").unwrap();
    let successor = fixture.sim.interner.get("ENGINEER").unwrap();
    let factory = fixture
        .projection
        .cameo_factory(&fixture.sim, fixture.owner, successor)
        .expect("Begin links the promoted ENGINEER to the retained Factory");
    assert_eq!(factory.construction_order(), order);
    assert_eq!(factory.object.unwrap().type_id, successor);
    assert!(factory.is_running());
    assert!(
        fixture
            .projection
            .cameo_factory(&fixture.sim, fixture.owner, old_type)
            .is_none(),
        "HouseAbandon4FAB74→Strip6ABBDF clears E1 before successor promotion"
    );
    assert!(
        fixture
            .projection
            .factory_statuses(&fixture.sim, fixture.owner)
            .iter()
            .all(|status| status.type_id != old_type),
        "the old cameo must not draw the successor's clock or Hold"
    );
    let preview = arm_gapowr_with_preview(fixture);
    let pending = fixture.input.targeting_mode.clone();
    let click = press_cameo(
        fixture,
        "E1",
        SidebarTab::Infantry,
        CameoPress::Right { shift: false },
    );
    assert_eq!(click.order, None, "no E1 copy remains queued");
    assert!(!click.sound);
    assert!(!click.clear_building_placement);
    assert_eq!(fixture.input.targeting_mode, pending);
    let retained = fixture.input.building_placement_preview.as_ref().unwrap();
    assert_same_preview(retained, &preview);
}

/// Local House4FAB74 unlinks the old cameo before4FAC96 promotes on the
/// same Factory. The queued-only4FAAEE return must preserve its active link.
#[test]
fn mixed_infantry_cancel_clears_old_cameo_before_same_factory_promotion() {
    let Some(mut fixture) = paid_ready_fixture() else {
        return;
    };
    let (order, product) = start_mixed_infantry_queue(&mut fixture);
    let owner = fixture.owner;
    let queued_cancel = cancel_envelope(&fixture, "ENGINEER");
    let frame = advance_commands(&mut fixture, &[queued_cancel]);
    consume_frame_outputs(&mut fixture, &frame, Some(owner));
    assert_eq!(
        fixture
            .projection
            .cameo_factory(&fixture.sim, owner, fixture.sim.interner.get("E1").unwrap())
            .unwrap()
            .construction_order(),
        order,
        "queued-only removal keeps the active Factory link"
    );
    let enqueue = CommandEnvelope::new(
        owner,
        fixture.sim.session.tick + 1,
        Command::QueueProduction {
            type_id: fixture.sim.interner.get("ENGINEER").unwrap(),
        },
    );
    let suspend = CommandEnvelope::new(
        owner,
        fixture.sim.session.tick + 1,
        Command::SuspendProduction {
            category: ProductionCategory::Infantry,
        },
    );
    let frame = advance_commands(&mut fixture, &[enqueue, suspend]);
    consume_frame_outputs(&mut fixture, &frame, Some(owner));
    assert_eq!(
        press_cameo(
            &mut fixture,
            "E1",
            SidebarTab::Infantry,
            CameoPress::Right { shift: false }
        )
        .order,
        Some(CameoOrder::Abandon { all: false })
    );
    let cancel = cancel_envelope(&fixture, "E1");
    let frame = advance_commands(&mut fixture, &[cancel]);
    consume_frame_outputs(&mut fixture, &frame, Some(owner));
    assert!(!fixture.sim.substrate.entities.contains(product));
    assert_old_infantry_cameo_released(&mut fixture, order);
}

/// Accepted mobile HousePlace4FB663 reaches that same House unlink. Both
/// units are actually charged; ordinary Strip completion queues the PLACE.
#[test]
#[ignore = "slow; run when changing what it covers"]
fn mixed_infantry_delivery_clears_old_cameo_before_same_factory_promotion() {
    let Some(mut fixture) = paid_ready_fixture() else {
        return;
    };
    let (order, product) = start_mixed_infantry_queue(&mut fixture);
    let owner = fixture.owner;
    let successor = fixture.sim.interner.get("ENGINEER").unwrap();
    for _ in 0..4096 {
        let frame = advance_commands(&mut fixture, &[]);
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
        if fixture
            .sim
            .production
            .factories
            .view(owner, ProductionCategory::Infantry)
            .is_some_and(|factory| {
                factory
                    .object
                    .is_some_and(|object| object.type_id == successor)
            })
        {
            break;
        }
    }
    let delivered = fixture
        .sim
        .substrate
        .entities
        .get(product)
        .expect("successful delivery retains the E1");
    assert!(!delivered.lifecycle.in_limbo());
    assert!(delivered.lifecycle.cell_marked());
    assert_old_infantry_cameo_released(&mut fixture, order);
}

/// Terminal mobile4FB5B5 also unlinks before refund/destruction/promotion.
/// Supply an independent existing HELLO contact at delivery; this is not a
/// native timing claim for how an ordinary barracks becomes occupied.
#[test]
fn mixed_infantry_terminal_delivery_clears_old_cameo_before_same_factory_promotion() {
    let Some(mut fixture) = paid_ready_fixture() else {
        return;
    };
    let (order, product) = start_mixed_infantry_queue(&mut fixture);
    let owner = fixture.owner;
    for _ in 0..4096 {
        if fixture
            .sim
            .production
            .factories
            .view(owner, ProductionCategory::Infantry)
            .is_some_and(|factory| factory.complete_object().is_some())
        {
            break;
        }
        let frame = advance_commands(&mut fixture, &[]);
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
    }
    let producer =
        production::object_find_factory(&fixture.sim, &fixture.rules, product, false, true)
            .expect("the actual paid E1 finds its barracks");
    assert!(
        fixture
            .sim
            .production
            .factories
            .building_factory(producer)
            .is_none()
    );
    let contact = fixture
        .sim
        .spawn_object("E1", "Americans", 25, 25, 0, &fixture.rules)
        .unwrap();
    assert_eq!(
        crate::sim::radio::transmit(
            &mut fixture.sim,
            contact,
            producer,
            crate::sim::radio::RadioMessage::Hello,
            crate::sim::radio::RadioPayload::default(),
            Some(&fixture.rules),
            FrameEffects::default(),
        )
        .code(),
        1
    );
    assert!(
        fixture
            .sim
            .substrate
            .entities
            .get(producer)
            .unwrap()
            .radio_contacts
            .first_free()
            .is_none()
    );
    let credits = production::credits_for_owner(&fixture.sim, "Americans");
    let frame = advance_commands(&mut fixture, &[]);
    consume_frame_outputs(&mut fixture, &frame, Some(owner));
    assert!(
        !fixture.sim.substrate.entities.contains(product),
        "the busy terminal refusal destroys the held E1"
    );
    assert!(
        production::credits_for_owner(&fixture.sim, "Americans")
            >= credits
                + fixture
                    .sim
                    .cost_of(owner, fixture.rules.object("E1").unwrap(), &fixture.rules)
    );
    assert_old_infantry_cameo_released(&mut fixture, order);
}

fn install_late_stationary_blocker(fixture: &mut Fixture, cell: (u16, u16)) -> u64 {
    // Original43B740/440580 can reveal this independent stationary object
    // between admitted input and Event execution. Supply that lifecycle
    // boundary through the shared constructor/Unlimbo owner, not a whole
    // trigger cadence or a fabricated failing PLACE return.
    fixture
        .sim
        .spawn_object("GAPILL", "Americans", cell.0, cell.1, 0, &fixture.rules)
        .expect("independent stationary GAPILL blocks the admitted site")
}

fn native_step(history_id: &str, command: &str, occurrence: usize) -> &'static serde_json::Value {
    static CORPUS: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    let corpus = CORPUS.get_or_init(|| {
        serde_json::from_str(crate::test_fixture::text(
            "tools/sidebar_oracle/cameo_order.json",
        ))
        .expect("executed ready-building corpus")
    });
    let history = corpus["ready_building_histories"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"].as_str() == Some(history_id))
        .unwrap_or_else(|| panic!("native ready-building history {history_id}"));
    history["steps"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|step| step["command"].as_str() == Some(command))
        .nth(occurrence)
        .unwrap_or_else(|| panic!("native {history_id} step {command}"))
}

fn native_after(history_id: &str, command: &str, occurrence: usize) -> &'static serde_json::Value {
    &native_step(history_id, command, occurrence)["after"]
}

fn native_placement_after(
    history_id: &str,
    command: &str,
    occurrence: usize,
) -> &'static serde_json::Value {
    &native_after(history_id, command, occurrence)["placement"]
}

// The executable corpus pins these control-flow fields. Fixture coordinates,
// numeric stable IDs, and camera-derived preview pixels are independent
// inputs rather than assertions of whole native scene parity.
fn assert_current_matches_native(fixture: &Fixture, expected: &serde_json::Value) {
    assert_eq!(
        fixture
            .input
            .targeting_mode
            .as_ref()
            .and_then(TargetingMode::as_building_placement),
        expected["current_type"].as_str(),
    );
    let product_present = match fixture.input.targeting_mode.as_ref() {
        Some(TargetingMode::BuildingPlacement(target)) => target.product_id.is_some(),
        _ => false,
    };
    assert_eq!(product_present, !expected["current_product"].is_null());
    assert_eq!(
        fixture
            .input
            .targeting_mode
            .as_ref()
            .and_then(TargetingMode::as_building_placement)
            .is_some(),
        expected["active_foundation"].as_str() != Some("0x0"),
    );
}

fn assert_saved_matches_native(fixture: &Fixture, expected: &serde_json::Value) {
    assert_eq!(
        fixture
            .input
            .saved_building_placement
            .as_ref()
            .map(|saved| saved.target.type_id()),
        expected["saved_type"].as_str(),
    );
    assert_eq!(
        fixture
            .input
            .saved_building_placement
            .as_ref()
            .is_some_and(|saved| saved.target.product_id.is_some()),
        !expected["saved_product"].is_null(),
    );
    assert_eq!(
        fixture.input.saved_building_placement.is_some(),
        expected["saved_foundation"].as_str() != Some("0x0"),
    );
}

fn assert_same_preview(
    after: &production::BuildingPlacementPreview,
    before: &production::BuildingPlacementPreview,
) {
    assert_eq!(
        (
            after.rx,
            after.ry,
            after.width,
            after.height,
            after.valid,
            after.in_build_area,
            after.foundation_visible,
            &after.reason,
            &after.cells,
            &after.wall_autofill_cells,
        ),
        (
            before.rx,
            before.ry,
            before.width,
            before.height,
            before.valid,
            before.in_build_area,
            before.foundation_visible,
            &before.reason,
            &before.cells,
            &before.wall_autofill_cells,
        ),
        "every placement preview field is retained"
    );
}

fn assert_pending_repeat(name: &str, tab: SidebarTab) {
    let Some(mut fixture) = paid_ready_fixture() else {
        return;
    };
    let queue_before: Vec<_> =
        production::queue_view_for_owner(&fixture.sim, &fixture.rules, "Americans")
            .into_iter()
            .map(|item| (item.type_id, item.queue_category, item.state, item.progress))
            .collect();
    let held_before: Vec<_> = [ProductionCategory::Building, ProductionCategory::Defense]
        .map(|category| {
            let factory = fixture
                .sim
                .production
                .factories
                .view(fixture.owner, category)
                .unwrap();
            let object = factory.complete_object().unwrap();
            (object.type_id, object.entity_id.unwrap())
        })
        .into_iter()
        .collect();
    let credits_before = production::credits_for_owner(&fixture.sim, "Americans");
    let spent_before = fixture.sim.houses[&fixture.owner].economy.spent_credits();
    let hash_before = fixture.sim.state_hash();
    let rng_before = (
        fixture.sim.main_rng.logical_state(),
        fixture.sim.scenario_rng.logical_state(),
        fixture.sim.mapgen_rng.logical_state(),
    );
    let preview = arm_gapowr_with_preview(&mut fixture);
    let pending_before = fixture.input.targeting_mode.clone();
    let entered = press_ready_cameo(&mut fixture, name, tab);

    let queue_after: Vec<_> =
        production::queue_view_for_owner(&fixture.sim, &fixture.rules, "Americans")
            .into_iter()
            .map(|item| (item.type_id, item.queue_category, item.state, item.progress))
            .collect();
    assert_eq!(
        queue_after, queue_before,
        "ready clicks cannot edit either factory queue"
    );
    for (type_id, stable_id) in held_before {
        let category = production::category_for_object(
            fixture.rules.object(fixture.sim.resolve(type_id)).unwrap(),
        );
        let object = fixture
            .sim
            .production
            .factories
            .view(fixture.owner, category)
            .unwrap()
            .complete_object()
            .unwrap();
        assert_eq!(object.type_id, type_id);
        assert_eq!(object.entity_id, Some(stable_id));
        assert!(
            fixture
                .sim
                .entities()
                .get(stable_id)
                .unwrap()
                .lifecycle
                .in_limbo()
        );
    }
    assert_eq!(
        production::credits_for_owner(&fixture.sim, "Americans"),
        credits_before
    );
    assert_eq!(
        fixture.sim.houses[&fixture.owner].economy.spent_credits(),
        spent_before
    );
    assert_eq!(fixture.sim.state_hash(), hash_before);
    assert_eq!(
        (
            fixture.sim.main_rng.logical_state(),
            fixture.sim.scenario_rng.logical_state(),
            fixture.sim.mapgen_rng.logical_state(),
        ),
        rng_before,
        "input admission has no simulated RNG side effects"
    );
    assert!(
        !entered,
        "Manual_Place refuses every entry while another placement is pending"
    );
    assert_eq!(
        fixture
            .input
            .targeting_mode
            .as_ref()
            .and_then(TargetingMode::as_building_placement),
        Some("GAPOWR"),
        "the first completed building remains pending"
    );
    assert_eq!(
        fixture.input.targeting_mode, pending_before,
        "a refused entry also retains the actual product and House issuer"
    );
    let after = fixture
        .input
        .building_placement_preview
        .as_ref()
        .expect("pending preview is retained");
    assert_same_preview(after, &preview);
}

#[test]
fn same_ready_cameo_retains_pending_building_and_preview() {
    assert_pending_repeat("GAPOWR", SidebarTab::Building);
}

#[test]
fn other_ready_cameo_retains_pending_building_and_preview() {
    assert_pending_repeat("GAPILL", SidebarTab::Defense);
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn own_ready_cameo_right_clears_other_category_pending_before_abandon() {
    for shift in [false, true] {
        let Some(mut fixture) = paid_ready_fixture() else {
            return;
        };
        arm_gapowr_with_preview(&mut fixture);
        let identities_before = ready_identities(&fixture);
        let credits_before = production::credits_for_owner(&fixture.sim, "Americans");
        let spent_before = fixture.sim.houses[&fixture.owner].economy.spent_credits();
        let hash_before = fixture.sim.state_hash();
        let click = press_cameo(
            &mut fixture,
            "GAPILL",
            SidebarTab::Defense,
            CameoPress::Right { shift },
        );

        assert!(click.clear_building_placement);
        assert!(click.sound);
        assert_eq!(click.eva, Some(sidebar_eva::EVA_CANCELED));
        assert_eq!(click.order, Some(CameoOrder::Abandon { all: shift }));
        assert!(fixture.input.targeting_mode.is_none());
        assert!(fixture.input.building_placement_preview.is_none());
        assert_eq!(ready_identities(&fixture), identities_before);
        assert_eq!(
            production::credits_for_owner(&fixture.sim, "Americans"),
            credits_before
        );
        assert_eq!(
            fixture.sim.houses[&fixture.owner].economy.spent_credits(),
            spent_before
        );
        assert_eq!(
            fixture.sim.state_hash(),
            hash_before,
            "the local prefix clears input before the later Abandon receiver"
        );
        let queue = production::queue_view_for_owner(&fixture.sim, &fixture.rules, "Americans");
        assert_eq!(queue.len(), 2);
        assert!(queue.iter().all(|item| item.state == BuildQueueState::Done));
    }
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn queued_only_cameo_right_retains_pending_and_preview_before_abandon() {
    for shift in [false, true] {
        let Some(mut fixture) = paid_ready_fixture() else {
            return;
        };
        // Infantry genuinely queues in native. Keep the existing completed
        // building strips separate from this real barracks queue; no building
        // queue fixture relies on VERA's recorded BeginProduction residual.
        let options =
            production::all_build_options_for_owner(&fixture.sim, &fixture.rules, "Americans");
        for name in ["E1", "ENGINEER"] {
            let type_id = fixture.sim.interner.get(name).unwrap();
            assert!(
                options
                    .iter()
                    .any(|option| option.type_id == type_id && option.visible_in_sidebar()),
                "the retail barracks fixture must expose {name}"
            );
            assert!(production::enqueue_by_type(
                &mut fixture.sim,
                &fixture.rules,
                "Americans",
                name,
                FrameEffects::default(),
                false,
            ));
        }
        let engineer = fixture.sim.interner.get("ENGINEER").unwrap();
        let queue = production::queue_view_for_owner(&fixture.sim, &fixture.rules, "Americans");
        assert!(queue.iter().any(|item| {
            item.type_id == engineer
                && item.queue_category == ProductionCategory::Infantry
                && item.state == BuildQueueState::Queued
        }));
        let preview = arm_gapowr_with_preview(&mut fixture);
        let pending_before = fixture.input.targeting_mode.clone();
        let identities_before = ready_identities(&fixture);
        let hash_before = fixture.sim.state_hash();
        let credits_before = production::credits_for_owner(&fixture.sim, "Americans");
        let click = press_cameo(
            &mut fixture,
            "ENGINEER",
            SidebarTab::Infantry,
            CameoPress::Right { shift },
        );

        assert!(!click.clear_building_placement);
        assert!(click.sound);
        assert_eq!(click.eva, None);
        assert_eq!(click.order, Some(CameoOrder::Abandon { all: shift }));
        assert_eq!(
            fixture
                .input
                .targeting_mode
                .as_ref()
                .and_then(TargetingMode::as_building_placement),
            Some("GAPOWR")
        );
        assert_eq!(fixture.input.targeting_mode, pending_before);
        assert_same_preview(
            fixture.input.building_placement_preview.as_ref().unwrap(),
            &preview,
        );
        assert_eq!(ready_identities(&fixture), identities_before);
        assert_eq!(
            production::credits_for_owner(&fixture.sim, "Americans"),
            credits_before
        );
        assert_eq!(fixture.sim.state_hash(), hash_before);
        let after = production::queue_view_for_owner(&fixture.sim, &fixture.rules, "Americans");
        assert_eq!(
            after
                .iter()
                .map(|item| (item.type_id, item.queue_category, item.state, item.progress))
                .collect::<Vec<_>>(),
            queue
                .iter()
                .map(|item| (item.type_id, item.queue_category, item.state, item.progress))
                .collect::<Vec<_>>(),
            "the gadget prefix does not execute the requested queued-copy Abandon"
        );
    }
}

/// Supplied 32x32 terrain and healthy base, with paid retail GAPOWR identity.
/// UpdateFoundationCenter4A9480/4A9485 stores proximity independently of the
/// footprint's buildability; Cell47EE6C/47EE73 makes every ordinary preview
/// cell invalid when that flag is zero, before its own IsClearToBuild call.
/// This regression covers that production decision and queued refusal/input
/// cleanup, not native terrain production, cursor projection or PLACE raster.
#[test]
fn mixed_slope_preview_uses_proximity_and_preserves_paid_product_lifecycle() {
    let Some(mut fixture) = paid_ready_fixture() else {
        return;
    };
    let owner = fixture.owner;
    let product = ready_product(&fixture, "GAPOWR");
    let other_product = ready_product(&fixture, "GAPILL");
    let credits = production::credits_for_owner(&fixture.sim, "Americans");
    let near = (8, 4);
    let far = (25, 25);
    let near_clear = arm_gapowr_with_preview(&mut fixture);
    assert_eq!((near_clear.rx, near_clear.ry), near);
    assert!(near_clear.cells.iter().all(|cell| cell.valid));
    let far_clear = production::placement_preview_for_owner_with_overlays(
        &fixture.sim,
        &fixture.rules,
        "Americans",
        "GAPOWR",
        far.0,
        far.1,
        Some(&fixture.overlays),
    )
    .expect("the same paid product has a far preview inside the supplied map");
    assert_eq!(
        far_clear.reason,
        Some(production::BuildingPlacementError::OutOfBuildArea)
    );
    assert!(far_clear.cells.iter().all(|cell| !cell.valid));

    // Only current Cell slope is supplied. No ready flag, factory progress,
    // proximity result or preview admission is forged by this terrain fixture.
    for cell in [near, far] {
        fixture
            .sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap()
            .slope_type = 1;
    }
    let near_mixed = production::placement_preview_for_owner_with_overlays(
        &fixture.sim,
        &fixture.rules,
        "Americans",
        "GAPOWR",
        near.0,
        near.1,
        Some(&fixture.overlays),
    )
    .unwrap();
    assert!(!near_mixed.valid);
    assert!(
        near_mixed.in_build_area && near_mixed.foundation_visible,
        "red terrain still passes native local event gates"
    );
    assert_eq!(
        near_mixed.reason,
        Some(production::BuildingPlacementError::BlockedTerrain)
    );
    assert!(near_mixed.cells.iter().any(|cell| cell.valid));
    assert!(near_mixed.cells.iter().any(|cell| !cell.valid));
    let far_mixed = production::placement_preview_for_owner_with_overlays(
        &fixture.sim,
        &fixture.rules,
        "Americans",
        "GAPOWR",
        far.0,
        far.1,
        Some(&fixture.overlays),
    )
    .unwrap();
    assert!(!far_mixed.valid);
    assert!(!far_mixed.in_build_area && far_mixed.foundation_visible);
    assert_eq!(
        far_mixed.reason,
        Some(production::BuildingPlacementError::BlockedTerrain),
        "the final terrain refusal must not stand in for the proximity result"
    );
    assert!(
        far_mixed.cells.iter().all(|cell| !cell.valid),
        "proximity denies every far cell, including the otherwise clear cells"
    );
    assert_eq!(ready_product(&fixture, "GAPOWR"), product);

    // Check the independent visibility prerequisite on clear terrain before
    // submitting a red near-base site. This fixture drives the ordinary PLACE
    // receiver/input transaction, not AppState's physical mouse dispatcher.
    fixture
        .sim
        .resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(near.0, near.1)
        .unwrap()
        .slope_type = 0;
    fixture
        .sim
        .fog
        .set_ground_bits_for_test(owner, near.0, near.1, 0);
    let hidden = production::placement_preview_for_owner_with_overlays(
        &fixture.sim,
        &fixture.rules,
        "Americans",
        "GAPOWR",
        near.0,
        near.1,
        Some(&fixture.overlays),
    )
    .unwrap();
    assert!(
        !hidden.valid,
        "the independent local visibility gate refuses the click"
    );
    assert!(hidden.in_build_area && !hidden.foundation_visible);
    assert!(!hidden.cells[0].valid);
    assert!(hidden.cells[1..].iter().all(|cell| cell.valid));
    assert_eq!(ready_product(&fixture, "GAPOWR"), product);
    fixture
        .sim
        .fog
        .set_ground_bits_for_test(owner, near.0, near.1, 0x18);
    fixture
        .sim
        .resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(near.0, near.1)
        .unwrap()
        .slope_type = 1;
    let command = submit_current_building(&mut fixture, near);
    let refused = advance_commands(&mut fixture, &[command]);
    assert_eq!(
        placement_facts(&refused),
        vec![LifecycleOutput::BuildingPlacementRetry { owner }]
    );
    assert_eq!(
        consume_frame_outputs(&mut fixture, &refused, Some(owner)),
        1
    );
    assert_eq!(ready_product(&fixture, "GAPOWR"), product);
    assert_eq!(
        fixture
            .input
            .building_placement_view()
            .current
            .unwrap()
            .product_id(),
        Some(product)
    );
    assert!(fixture.input.saved_building_placement.is_none());
    assert!(fixture.input.cancel_building_placement());
    assert!(fixture.input.targeting_mode.is_none());
    assert!(fixture.input.building_placement_preview.is_none());
    assert_eq!(ready_product(&fixture, "GAPOWR"), product);

    fixture
        .sim
        .resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(near.0, near.1)
        .unwrap()
        .slope_type = 0;
    let command = submit_ready_building(&mut fixture, "GAPOWR", SidebarTab::Building, near);
    let accepted = advance_commands(&mut fixture, &[command]);
    assert!(
        placement_facts(&accepted).contains(&LifecycleOutput::BuildingPlacementSucceeded { owner })
    );
    consume_frame_outputs(&mut fixture, &accepted, Some(owner));
    let placed = fixture.sim.entities().get(product).unwrap();
    assert!(!placed.lifecycle.in_limbo());
    assert!(placed.lifecycle.cell_marked());
    assert_eq!((placed.position.rx(), placed.position.ry()), near);
    assert!(
        production::ready_building_entity_for_owner(
            &fixture.sim,
            &fixture.rules,
            owner,
            fixture.sim.interner.get("GAPOWR").unwrap(),
        )
        .is_none()
    );
    assert!(fixture.input.targeting_mode.is_none());
    assert!(fixture.input.building_placement_preview.is_none());
    assert!(fixture.input.saved_building_placement.is_none());
    assert_eq!(ready_product(&fixture, "GAPILL"), other_product);
    assert_eq!(
        production::credits_for_owner(&fixture.sim, "Americans"),
        credits,
        "refusal, cursor cancel and placement do not purchase another product"
    );
}

/// Executed HousePlace4FB35F/4FB395 control: an independent stationary
/// GAPILL appears after admission. The failure restores the saved product
/// and House issuer, then consumes that one backup.
#[test]
fn late_stationary_blocker_restores_saved_product_and_requests_preview_refresh() {
    let Some(mut fixture) = paid_ready_fixture() else {
        return;
    };
    let product = ready_product(&fixture, "GAPOWR");
    let command = submit_ready_building(&mut fixture, "GAPOWR", SidebarTab::Building, (8, 4));
    assert!(fixture.input.saved_building_placement.is_some());
    let blocker = install_late_stationary_blocker(&mut fixture, (8, 4));
    let credits = production::credits_for_owner(&fixture.sim, "Americans");
    let frame = advance_commands(&mut fixture, &[command]);

    assert_eq!(frame.tick.executed_commands, 1);
    assert!(!frame.tick.spawned_entities);
    assert_eq!(
        placement_facts(&frame),
        vec![LifecycleOutput::BuildingPlacementRetry {
            owner: fixture.owner
        }]
    );
    assert_eq!(
        frame
            .sound_events
            .iter()
            .filter(|event| matches!(event, SimSoundEvent::CannotDeployHere { owner } if *owner == fixture.owner))
            .count(),
        1
    );
    let owner = fixture.owner;
    assert_eq!(consume_frame_outputs(&mut fixture, &frame, Some(owner)), 1);
    let Some(TargetingMode::BuildingPlacement(target)) = fixture.input.targeting_mode.as_ref()
    else {
        panic!("failed Unlimbo restores the exact saved placement");
    };
    assert_eq!(target.type_id(), "GAPOWR");
    assert_eq!(target.product_id, Some(product));
    assert_eq!(target.owner, fixture.owner);
    assert_eq!(
        fixture.input.building_placement_issuer(),
        Some(fixture.owner)
    );
    assert!(fixture.input.saved_building_placement.is_none());
    assert_eq!(ready_product(&fixture, "GAPOWR"), product);
    let native = native_placement_after(
        "admitted_then_blocked_retry_then_abandon",
        "local_event_corridor",
        0,
    );
    assert_current_matches_native(&fixture, native);
    assert_saved_matches_native(&fixture, native);
    assert!(
        fixture
            .sim
            .entities()
            .get(product)
            .unwrap()
            .lifecycle
            .in_limbo()
    );
    assert!(
        !fixture
            .sim
            .entities()
            .get(blocker)
            .unwrap()
            .lifecycle
            .in_limbo()
    );
    assert_eq!(
        production::credits_for_owner(&fixture.sim, "Americans"),
        credits
    );
    // The consumer requests a fresh actual-cursor preview. This headless
    // fixture does not equate its supplied (8,4) preview with native tactical
    // cursor projection or execute the AppState camera/GPU wrapper.
}

/// Executed HousePlace4FB328..4FB33F and4FB395 controls in both insertion
/// orders: the first success clears the saved second click; the later actual
/// Unlimbo failure therefore restores NULL while retaining that held object.
#[test]
#[ignore = "slow; run when changing what it covers"]
fn same_frame_success_then_failure_clears_backup_in_both_building_orders() {
    for orders in [
        [
            ("GAPOWR", SidebarTab::Building),
            ("GAPILL", SidebarTab::Defense),
        ],
        [
            ("GAPILL", SidebarTab::Defense),
            ("GAPOWR", SidebarTab::Building),
        ],
    ] {
        let Some(mut fixture) = paid_ready_fixture() else {
            return;
        };
        let first_id = ready_product(&fixture, orders[0].0);
        let second_id = ready_product(&fixture, orders[1].0);
        let first = submit_ready_building(&mut fixture, orders[0].0, orders[0].1, (8, 4));
        let second = submit_ready_building(&mut fixture, orders[1].0, orders[1].1, (8, 4));
        let credits = production::credits_for_owner(&fixture.sim, "Americans");
        let frame = advance_commands(&mut fixture, &[first, second]);

        assert_eq!(frame.tick.executed_commands, 2);
        assert!(frame.tick.spawned_entities);
        // Successful building HousePlace4FB2C7 also reaches HouseAbandon
        // before4FB328 clears the current and saved placement transaction.
        assert_eq!(
            placement_facts(&frame),
            vec![
                LifecycleOutput::BuildingProductionAbandoned {
                    owner: fixture.owner
                },
                LifecycleOutput::BuildingPlacementSucceeded {
                    owner: fixture.owner
                },
                LifecycleOutput::BuildingPlacementRetry {
                    owner: fixture.owner
                },
            ]
        );
        let owner = fixture.owner;
        assert_eq!(consume_frame_outputs(&mut fixture, &frame, Some(owner)), 0);
        assert!(fixture.input.targeting_mode.is_none());
        assert!(fixture.input.building_placement_preview.is_none());
        assert!(fixture.input.saved_building_placement.is_none());
        let history = if orders[0].0 == "GAPILL" {
            "queued_same_cell_GAPILL_then_GAPOWR"
        } else {
            "queued_same_cell_GAPOWR_then_GAPILL"
        };
        let native = native_placement_after(history, "local_event_corridor", 0);
        assert_current_matches_native(&fixture, native);
        assert_saved_matches_native(&fixture, native);
        let placed = fixture.sim.entities().get(first_id).unwrap();
        assert!(!placed.lifecycle.in_limbo());
        assert_eq!((placed.position.rx(), placed.position.ry()), (8, 4));
        assert_eq!(ready_product(&fixture, orders[1].0), second_id);
        assert!(
            fixture
                .sim
                .entities()
                .get(second_id)
                .unwrap()
                .lifecycle
                .in_limbo()
        );
        let ready =
            production::ready_buildings_for_owner(&fixture.sim, &fixture.rules, "Americans");
        assert_eq!(ready.len(), 1);
        assert_eq!(fixture.sim.resolve(ready[0].type_id), orders[1].0);
        assert_eq!(
            production::credits_for_owner(&fixture.sim, "Americans"),
            credits
        );
    }
}

#[test]
fn retry_then_linked_abandon_preserves_current_until_product_expiry_in_the_same_batch() {
    let Some(mut fixture) = paid_ready_fixture() else {
        return;
    };
    let abandoned_product = ready_product(&fixture, "GAPILL");
    let retained_product = ready_product(&fixture, "GAPOWR");
    let place = submit_ready_building(&mut fixture, "GAPOWR", SidebarTab::Building, (8, 4));
    install_late_stationary_blocker(&mut fixture, (8, 4));
    let abandon = cancel_envelope(&fixture, "GAPILL");
    let frame = advance_commands(&mut fixture, &[place, abandon]);

    assert_eq!(frame.tick.executed_commands, 2);
    assert!(!frame.tick.spawned_entities);
    assert_eq!(
        placement_facts(&frame),
        vec![
            LifecycleOutput::BuildingPlacementRetry {
                owner: fixture.owner
            },
            LifecycleOutput::BuildingProductionAbandoned {
                owner: fixture.owner
            },
        ]
    );
    let expiry = frame
        .lifecycle_outputs
        .iter()
        .position(|output| {
            *output
                == LifecycleOutput::ObjectPointerExpired {
                    stable_id: abandoned_product,
                }
        })
        .unwrap();
    let owner = fixture.owner;
    let native_calls =
        native_step("queued_failure_then_abandon", "local_event_corridor", 0)["native_calls"]
            .as_array()
            .unwrap();
    let native_clear = native_calls
        .iter()
        .find(|call| call["name"] == "abandon_clear_current")
        .unwrap();
    let mut retries = 0;
    for output in &frame.lifecycle_outputs[..=expiry] {
        retries += usize::from(consume_lifecycle_output(&mut fixture, output, Some(owner)));
    }
    // The original callback immediately before4FAB83 still has Power's
    // current product/type/foundation after the linked Pill destructor.
    assert_current_matches_native(&fixture, &native_clear["placement"]);
    for output in &frame.lifecycle_outputs[expiry + 1..] {
        retries += usize::from(consume_lifecycle_output(&mut fixture, output, Some(owner)));
    }
    assert_eq!(retries, 1);
    assert!(fixture.input.targeting_mode.is_none());
    assert!(fixture.input.building_placement_preview.is_none());
    assert!(fixture.input.saved_building_placement.is_none());
    assert!(fixture.sim.entities().get(abandoned_product).is_none());
    assert_eq!(ready_product(&fixture, "GAPOWR"), retained_product);
    let native = native_placement_after("queued_failure_then_abandon", "local_event_corridor", 0);
    assert_current_matches_native(&fixture, native);
    assert_saved_matches_native(&fixture, native);
    assert_eq!(
        production::ready_buildings_for_owner(&fixture.sim, &fixture.rules, "Americans").len(),
        1
    );
}

#[test]
fn unlinked_abandon_clears_current_before_product_expiry() {
    let Some(mut fixture) = paid_ready_fixture() else {
        return;
    };
    let owner = fixture.owner;
    let power_type = fixture.sim.interner.get("GAPOWR").unwrap();
    let power_product = ready_product(&fixture, "GAPOWR");
    let pill_type = fixture.sim.interner.get("GAPILL").unwrap();
    let pill_product = ready_product(&fixture, "GAPILL");
    let pill_order = fixture
        .sim
        .production
        .factories
        .view(owner, ProductionCategory::Defense)
        .unwrap()
        .construction_order();
    // Supply the executed StripClear6A81B0 helper boundary: no Building
    // links, an independent Defense link, and the real completed product
    // still current. This is not whole UI/scenario reset coverage. Other
    // regressions cover actual gadget Select through the same input owner.
    let _ = fixture.projection.apply_lifecycle_output(
        &LifecycleOutput::ProductionFactoryLinked {
            owner,
            type_id: pill_type,
            category: ProductionCategory::Defense,
            construction_order: pill_order,
        },
        Some(owner),
    );
    production::object_find_factory(&fixture.sim, &fixture.rules, power_product, false, false)
        .expect("the real completed product finds its healthy builder");
    assert!(
        fixture
            .input
            .begin_building_placement("GAPOWR", power_product, owner)
    );
    assert!(
        fixture
            .projection
            .cameo_factory(&fixture.sim, owner, power_type)
            .is_none()
    );
    let history = "local_unlinked_strip_clear_active_abandon_order";
    assert_current_matches_native(
        &fixture,
        &native_after(history, "strip_clear", 0)["placement"],
    );

    let cancel = cancel_envelope(&fixture, "GAPOWR");
    let frame = advance_commands(&mut fixture, &[cancel]);
    let expiry = frame
        .lifecycle_outputs
        .iter()
        .position(|output| {
            *output
                == LifecycleOutput::ObjectPointerExpired {
                    stable_id: power_product,
                }
        })
        .unwrap();
    let native_calls = native_step(history, "house_abandon", 0)["native_calls"]
        .as_array()
        .unwrap();
    let native_expiry = native_calls
        .iter()
        .find(|call| call["name"] == "dispatch_pointer_expired")
        .unwrap();
    let mut clears = 0;
    for (index, output) in frame.lifecycle_outputs.iter().enumerate() {
        if let Some(cursor_clear) = fixture
            .projection
            .apply_lifecycle_output(output, Some(owner))
        {
            clears += 1;
            fixture
                .input
                .apply_building_placement_output(&cursor_clear, Some(owner));
        }
        fixture
            .input
            .apply_building_placement_output(output, Some(owner));
        if index == expiry {
            assert_current_matches_native(&fixture, &native_expiry["placement"]);
            assert_eq!(clears, 1, "zero-link House prefix precedes product expiry");
        }
    }
    assert_eq!(clears, 1, "the finish boundary must not clear twice");
    assert_current_matches_native(
        &fixture,
        &native_after(history, "house_abandon", 0)["placement"],
    );
    assert_saved_matches_native(
        &fixture,
        &native_after(history, "house_abandon", 0)["placement"],
    );
    assert!(!fixture.sim.substrate.entities.contains(power_product));
    assert!(
        fixture
            .sim
            .production
            .factories
            .view(owner, ProductionCategory::Building)
            .is_none()
    );
    assert_eq!(
        fixture
            .projection
            .cameo_factory(&fixture.sim, owner, pill_type)
            .unwrap()
            .construction_order(),
        pill_order
    );
    assert_eq!(ready_product(&fixture, "GAPILL"), pill_product);
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn abandon_then_place_rejects_early_without_retry_or_cannot_deploy() {
    let Some(mut fixture) = paid_ready_fixture() else {
        return;
    };
    let product = ready_product(&fixture, "GAPOWR");
    let place = submit_ready_building(&mut fixture, "GAPOWR", SidebarTab::Building, (8, 4));
    let abandon = cancel_envelope(&fixture, "GAPOWR");
    let frame = advance_commands(&mut fixture, &[abandon, place]);

    assert_eq!(frame.tick.executed_commands, 2);
    assert!(!frame.tick.spawned_entities);
    assert_eq!(
        placement_facts(&frame),
        vec![LifecycleOutput::BuildingProductionAbandoned {
            owner: fixture.owner
        }]
    );
    assert!(!frame.sound_events.iter().any(|event| {
        matches!(
            event,
            SimSoundEvent::CannotDeployHere { .. } | SimSoundEvent::BuildingPlaced { .. }
        )
    }));
    let owner = fixture.owner;
    assert_eq!(consume_frame_outputs(&mut fixture, &frame, Some(owner)), 0);
    assert!(fixture.input.targeting_mode.is_none());
    assert!(fixture.input.saved_building_placement.is_none());
    assert!(fixture.sim.entities().get(product).is_none());
    assert_eq!(
        production::ready_buildings_for_owner(&fixture.sim, &fixture.rules, "Americans").len(),
        1
    );
}

#[test]
fn remote_success_and_retry_facts_do_not_consume_the_local_backup() {
    let Some(mut fixture) = paid_ready_fixture() else {
        return;
    };
    let first = submit_ready_building(&mut fixture, "GAPOWR", SidebarTab::Building, (8, 4));
    let second_product = ready_product(&fixture, "GAPILL");
    let second = submit_ready_building(&mut fixture, "GAPILL", SidebarTab::Defense, (8, 4));
    let frame = advance_commands(&mut fixture, &[first, second]);
    // Supplied nonlocal House identity for the output consumer; this does
    // not claim a second House's production or native player initialization.
    let remote = fixture.sim.interner.intern("Russians");
    for local_house in [None, Some(remote)] {
        assert_eq!(consume_frame_outputs(&mut fixture, &frame, local_house), 0);
        assert!(fixture.input.targeting_mode.is_none());
        let saved = fixture.input.saved_building_placement.as_ref().unwrap();
        assert_eq!(saved.target.type_id(), "GAPILL");
        assert_eq!(saved.target.product_id, Some(second_product));
        assert_eq!(saved.target.owner, fixture.owner);
    }
    let owner = fixture.owner;
    assert_eq!(consume_frame_outputs(&mut fixture, &frame, Some(owner)), 0);
    assert!(fixture.input.targeting_mode.is_none());
    assert!(fixture.input.saved_building_placement.is_none());
}

#[test]
fn remote_other_category_abandon_does_not_clear_the_current_building() {
    let Some(mut fixture) = paid_ready_fixture() else {
        return;
    };
    let preview = arm_gapowr_with_preview(&mut fixture);
    let pending = fixture.input.targeting_mode.clone();
    let abandon = cancel_envelope(&fixture, "GAPILL");
    let frame = advance_commands(&mut fixture, &[abandon]);
    let remote = fixture.sim.interner.intern("Russians");
    assert_eq!(consume_frame_outputs(&mut fixture, &frame, Some(remote)), 0);
    assert_eq!(fixture.input.targeting_mode, pending);
    assert_same_preview(
        fixture.input.building_placement_preview.as_ref().unwrap(),
        &preview,
    );
    let owner = fixture.owner;
    assert_eq!(consume_frame_outputs(&mut fixture, &frame, Some(owner)), 0);
    assert!(fixture.input.targeting_mode.is_none());
    assert!(fixture.input.building_placement_preview.is_none());
}

/// DispatchPointerExpired7258D0's product-only control keeps type/foundation;
/// BandBoxLeftUp4AB9B0 subsequently has no House issuer to enqueue. This test
/// reaches the shared expiry/output/getter boundaries, not the AppState click.
#[test]
#[ignore = "slow; run when changing what it covers"]
fn current_product_expiry_retains_type_and_preview_but_has_no_issuer() {
    let Some(mut fixture) = paid_ready_fixture() else {
        return;
    };
    let preview = arm_gapowr_with_preview(&mut fixture);
    let product = ready_product(&fixture, "GAPOWR");
    let defense_product = ready_product(&fixture, "GAPILL");
    fixture.sim.detach_all_pointer_expired(
        product,
        &fixture.rules,
        Some(&fixture.overlays),
        FrameEffects::default(),
    );
    let frame = advance_commands(&mut fixture, &[]);
    assert!(
        frame
            .lifecycle_outputs
            .contains(&LifecycleOutput::ObjectPointerExpired { stable_id: product })
    );
    let owner = fixture.owner;
    assert_eq!(consume_frame_outputs(&mut fixture, &frame, Some(owner)), 0);
    assert_eq!(
        fixture
            .input
            .targeting_mode
            .as_ref()
            .and_then(TargetingMode::as_building_placement),
        Some("GAPOWR")
    );
    assert!(fixture.input.building_placement_issuer().is_none());
    assert_same_preview(
        fixture.input.building_placement_preview.as_ref().unwrap(),
        &preview,
    );
    let native = native_placement_after("current_product_expiry", "dispatch_pointer_expired", 0);
    assert_current_matches_native(&fixture, native);
    assert_saved_matches_native(&fixture, native);
    let factory = fixture
        .sim
        .production
        .factories
        .view(fixture.owner, ProductionCategory::Building)
        .expect("product expiry alone retains its factory before a building-state recheck");
    assert_eq!(factory.progress, 54);
    assert!(factory.object.is_none());
    assert!(factory.suspended);
    assert_eq!(ready_product(&fixture, "GAPILL"), defense_product);
    assert!(
        production::ready_building_entity_for_owner(
            &fixture.sim,
            &fixture.rules,
            fixture.owner,
            fixture.sim.interner.get("GAPOWR").unwrap(),
        )
        .is_none()
    );
    assert!(
        !production::ready_buildings_for_owner(&fixture.sim, &fixture.rules, "Americans")
            .iter()
            .any(|ready| fixture.sim.resolve(ready.type_id) == "GAPOWR")
    );
    assert!(
        !fixture.input.cancel_building_placement(),
        "TacticalRight4AAD34 tests current product, not the retained type"
    );
    assert_same_preview(
        fixture.input.building_placement_preview.as_ref().unwrap(),
        &preview,
    );
    assert!(!press_ready_cameo(
        &mut fixture,
        "GAPILL",
        SidebarTab::Defense
    ));
}

#[test]
fn null_product_cameo_right_retains_orphan_until_house_abandon() {
    let Some(mut fixture) = paid_ready_fixture() else {
        return;
    };
    let preview = arm_gapowr_with_preview(&mut fixture);
    let product = ready_product(&fixture, "GAPOWR");
    let defense_product = ready_product(&fixture, "GAPILL");
    let credits = production::credits_for_owner(&fixture.sim, "Americans");
    fixture.sim.detach_all_pointer_expired(
        product,
        &fixture.rules,
        Some(&fixture.overlays),
        FrameEffects::default(),
    );
    let frame = advance_commands(&mut fixture, &[]);
    let owner = fixture.owner;
    consume_frame_outputs(&mut fixture, &frame, Some(owner));
    let click = press_cameo(
        &mut fixture,
        "GAPOWR",
        SidebarTab::Building,
        CameoPress::Right { shift: false },
    );
    assert!(click.clear_building_placement);
    assert!(click.sound);
    assert_eq!(click.eva, Some(sidebar_eva::EVA_CANCELED));
    assert_eq!(click.order, Some(CameoOrder::Abandon { all: false }));
    assert_current_matches_native(
        &fixture,
        native_placement_after("current_product_expiry_right_receiver", "right_cameo", 0),
    );
    assert_eq!(
        fixture
            .input
            .targeting_mode
            .as_ref()
            .and_then(TargetingMode::as_building_placement),
        Some("GAPOWR"),
        "Select6AADCF's current-product guard does not admit the orphan"
    );
    assert_same_preview(
        fixture.input.building_placement_preview.as_ref().unwrap(),
        &preview,
    );
    let abandon = cancel_envelope(&fixture, "GAPOWR");
    let frame = advance_commands(&mut fixture, &[abandon]);
    assert_eq!(
        placement_facts(&frame),
        vec![LifecycleOutput::BuildingProductionAbandoned { owner }]
    );
    consume_frame_outputs(&mut fixture, &frame, Some(owner));
    assert!(fixture.input.targeting_mode.is_none());
    assert!(fixture.input.building_placement_preview.is_none());
    assert_current_matches_native(
        &fixture,
        native_placement_after(
            "current_product_expiry_right_receiver",
            "local_event_corridor",
            0,
        ),
    );
    assert!(
        fixture
            .sim
            .production
            .factories
            .view(owner, ProductionCategory::Building)
            .is_none()
    );
    assert_eq!(ready_product(&fixture, "GAPILL"), defense_product);
    assert_eq!(
        production::credits_for_owner(&fixture.sim, "Americans"),
        credits
    );
    assert!(press_ready_cameo(
        &mut fixture,
        "GAPILL",
        SidebarTab::Defense
    ));
    let click = press_cameo(
        &mut fixture,
        "GAPOWR",
        SidebarTab::Building,
        CameoPress::Right { shift: false },
    );
    assert_eq!(
        click,
        CameoClick::default(),
        "deleted Factory links are invalidated"
    );
    assert_current_matches_native(
        &fixture,
        native_placement_after(
            "null_product_direct_abandon_then_no_link_right",
            "right_cameo",
            0,
        ),
    );
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn null_product_house_abandon_accepts_nonzero_buildcat_without_a_held_type() {
    for name in ["GAPOWR", "GAPILE"] {
        let Some(mut fixture) = paid_ready_fixture() else {
            return;
        };
        arm_gapowr_with_preview(&mut fixture);
        let product = ready_product(&fixture, "GAPOWR");
        let credits = production::credits_for_owner(&fixture.sim, "Americans");
        fixture.sim.detach_all_pointer_expired(
            product,
            &fixture.rules,
            Some(&fixture.overlays),
            FrameEffects::default(),
        );
        let frame = advance_commands(&mut fixture, &[]);
        let owner = fixture.owner;
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
        assert!(fixture.rules.object("GACNST").unwrap().build_cat.is_none());
        let abandon = cancel_envelope(&fixture, "GACNST");
        let frame = advance_commands(&mut fixture, &[abandon]);
        assert!(
            placement_facts(&frame).is_empty(),
            "retail BuildCat0 retains the empty Factory"
        );
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
        assert_current_matches_native(
            &fixture,
            native_placement_after("null_product_requested_category_guards", "house_abandon", 0),
        );
        assert!(
            fixture
                .sim
                .production
                .factories
                .view(owner, ProductionCategory::Building)
                .is_some()
        );
        assert!(fixture.rules.object(name).unwrap().build_cat.is_some());
        let abandon = cancel_envelope(&fixture, name);
        let frame = advance_commands(&mut fixture, &[abandon]);
        assert_eq!(
            placement_facts(&frame),
            vec![LifecycleOutput::BuildingProductionAbandoned { owner }],
            "HouseAbandon4FABF6 skips the index match for a NULL held object"
        );
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
        assert!(fixture.input.targeting_mode.is_none());
        assert_current_matches_native(
            &fixture,
            native_placement_after("null_product_requested_category_guards", "house_abandon", 1),
        );
        assert!(
            fixture
                .sim
                .production
                .factories
                .view(owner, ProductionCategory::Building)
                .is_none()
        );
        assert_eq!(
            production::credits_for_owner(&fixture.sim, "Americans"),
            credits
        );
    }
}

#[test]
fn defense_null_product_house_abandon_clears_only_its_live_factory() {
    let Some(mut fixture) = paid_ready_fixture() else {
        return;
    };
    assert!(press_ready_cameo(
        &mut fixture,
        "GAPILL",
        SidebarTab::Defense
    ));
    let product = ready_product(&fixture, "GAPILL");
    let power = ready_product(&fixture, "GAPOWR");
    let credits = production::credits_for_owner(&fixture.sim, "Americans");
    fixture.sim.detach_all_pointer_expired(
        product,
        &fixture.rules,
        Some(&fixture.overlays),
        FrameEffects::default(),
    );
    let frame = advance_commands(&mut fixture, &[]);
    let owner = fixture.owner;
    consume_frame_outputs(&mut fixture, &frame, Some(owner));
    let abandon = cancel_envelope(&fixture, "GAPILL");
    let frame = advance_commands(&mut fixture, &[abandon]);
    assert_eq!(
        placement_facts(&frame),
        vec![LifecycleOutput::BuildingProductionAbandoned { owner }]
    );
    consume_frame_outputs(&mut fixture, &frame, Some(owner));
    assert_current_matches_native(
        &fixture,
        native_placement_after("defense_null_product_direct_abandon", "house_abandon", 0),
    );
    assert!(
        fixture
            .sim
            .production
            .factories
            .view(owner, ProductionCategory::Defense)
            .is_none()
    );
    assert_eq!(ready_product(&fixture, "GAPOWR"), power);
    assert_eq!(
        production::credits_for_owner(&fixture.sim, "Americans"),
        credits
    );
}

#[test]
fn same_factory_rearm_retains_the_old_cameos_factory_link_and_construction_order() {
    let Some(mut fixture) = paid_ready_fixture() else {
        return;
    };
    let product = ready_product(&fixture, "GAPOWR");
    let factory_order = fixture
        .sim
        .production
        .factories
        .iter_insertion_ordered()
        .iter()
        .map(|factory| (factory.category, factory.insertion_seq))
        .collect::<Vec<_>>();
    // A real admission is saved; its returned envelope is deliberately not
    // delivered before the supplied product-expiry notification.
    let _pending = submit_ready_building(&mut fixture, "GAPOWR", SidebarTab::Building, (8, 4));
    fixture.sim.detach_all_pointer_expired(
        product,
        &fixture.rules,
        Some(&fixture.overlays),
        FrameEffects::default(),
    );
    let frame = advance_commands(&mut fixture, &[]);
    let owner = fixture.owner;
    consume_frame_outputs(&mut fixture, &frame, Some(owner));
    assert!(press_ready_cameo(
        &mut fixture,
        "GAPILL",
        SidebarTab::Defense
    ));
    assert!(production::enqueue_by_type(
        &mut fixture.sim,
        &fixture.rules,
        "Americans",
        "GAPILE",
        FrameEffects::default(),
        false,
    ));
    let type_id = fixture.sim.interner.get("GAPILE").unwrap();
    for _ in 0..4096 {
        if production::ready_building_entity_for_owner(&fixture.sim, &fixture.rules, owner, type_id)
            .is_some()
        {
            break;
        }
        let frame = advance_commands(&mut fixture, &[]);
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
    }
    let product =
        production::ready_building_entity_for_owner(&fixture.sim, &fixture.rules, owner, type_id)
            .expect("the rearmed Factory pays and completes its new retail build");
    let native_rearm = native_after(
        "expired_product_same_category_rearm_retained_old_right",
        "produce_GAPILE",
        0,
    );
    assert_eq!(
        native_rearm["factory_registry"],
        native_after(
            "expired_product_same_category_rearm_retained_old_right",
            "dispatch_pointer_expired",
            0,
        )["factory_registry"],
        "executed native Factory identities and order survive rearm"
    );
    let native_order = native_rearm["factory_registry"]
        .as_array()
        .unwrap()
        .iter()
        .map(|factory| match factory["id"].as_str().unwrap() {
            "GAPOWR_factory" => ProductionCategory::Building,
            "GAPILL_factory" => ProductionCategory::Defense,
            id => panic!("unexpected native Factory identity {id}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        fixture
            .sim
            .production
            .factories
            .iter_insertion_ordered()
            .iter()
            .map(|factory| factory.category)
            .collect::<Vec<_>>(),
        native_order,
    );
    assert_eq!(
        fixture
            .sim
            .production
            .factories
            .iter_insertion_ordered()
            .iter()
            .map(|factory| (factory.category, factory.insertion_seq))
            .collect::<Vec<_>>(),
        factory_order,
        "native Begin reuses the Factory rather than appending a new one"
    );
    let credits = production::credits_for_owner(&fixture.sim, "Americans");
    let click = press_cameo(
        &mut fixture,
        "GAPOWR",
        SidebarTab::Building,
        CameoPress::Right { shift: false },
    );
    assert!(click.clear_building_placement);
    assert!(click.sound);
    assert_eq!(click.order, Some(CameoOrder::Abandon { all: false }));
    assert!(fixture.input.targeting_mode.is_none());
    assert_current_matches_native(
        &fixture,
        native_placement_after(
            "expired_product_same_category_rearm_retained_old_right",
            "right_cameo",
            0,
        ),
    );
    let abandon = cancel_envelope(&fixture, "GAPOWR");
    let frame = advance_commands(&mut fixture, &[abandon]);
    assert!(
        placement_facts(&frame).is_empty(),
        "the receiver refuses the mismatched held GAPILE index"
    );
    assert_eq!(
        production::ready_building_entity_for_owner(&fixture.sim, &fixture.rules, owner, type_id),
        Some(product)
    );
    assert_eq!(
        production::credits_for_owner(&fixture.sim, "Americans"),
        credits
    );
    assert!(press_ready_cameo(
        &mut fixture,
        "GAPOWR",
        SidebarTab::Building
    ));
    assert_current_matches_native(
        &fixture,
        native_placement_after(
            "expired_product_same_category_rearm_retained_old_left",
            "left_cameo",
            0,
        ),
    );
    assert_eq!(fixture.input.building_placement_issuer(), Some(owner));
    assert_eq!(
        fixture
            .input
            .targeting_mode
            .as_ref()
            .and_then(TargetingMode::as_building_placement),
        Some("GAPILE")
    );
    let command = submit_current_building(&mut fixture, (8, 4));
    assert_saved_matches_native(
        &fixture,
        native_placement_after(
            "expired_product_same_category_rearm_retained_old_left",
            "tactical_left_up",
            0,
        ),
    );
    let frame = advance_commands(&mut fixture, &[command]);
    assert_eq!(
        placement_facts(&frame),
        vec![
            LifecycleOutput::BuildingProductionAbandoned { owner },
            LifecycleOutput::BuildingPlacementSucceeded { owner },
        ]
    );
    consume_frame_outputs(&mut fixture, &frame, Some(owner));
    let expected = native_placement_after(
        "expired_product_same_category_rearm_retained_old_left",
        "local_event_corridor",
        0,
    );
    assert_current_matches_native(&fixture, expected);
    assert_saved_matches_native(&fixture, expected);
    assert!(
        fixture
            .sim
            .production
            .factories
            .view(owner, ProductionCategory::Building)
            .is_none()
    );
    assert_eq!(
        production::credits_for_owner(&fixture.sim, "Americans"),
        credits
    );
}

/// TacticalPointerExpired6DA57D consumes the saved product transaction while
/// the independent active placement survives. A later failure restores NULL.
#[test]
fn saved_product_expiry_clears_backup_without_clearing_another_current_product() {
    let Some(mut fixture) = paid_ready_fixture() else {
        return;
    };
    let product = ready_product(&fixture, "GAPOWR");
    let submitted = submit_ready_building(&mut fixture, "GAPOWR", SidebarTab::Building, (8, 4));
    assert!(press_ready_cameo(
        &mut fixture,
        "GAPILL",
        SidebarTab::Defense
    ));
    let pending = fixture.input.targeting_mode.clone();
    fixture.sim.detach_all_pointer_expired(
        product,
        &fixture.rules,
        Some(&fixture.overlays),
        FrameEffects::default(),
    );
    let frame = advance_commands(&mut fixture, &[submitted]);
    let owner = fixture.owner;
    assert_eq!(consume_frame_outputs(&mut fixture, &frame, Some(owner)), 0);
    assert_eq!(fixture.input.targeting_mode, pending);
    assert!(fixture.input.saved_building_placement.is_none());
    assert_saved_matches_native(
        &fixture,
        native_placement_after("saved_product_expiry", "dispatch_pointer_expired", 0),
    );
    assert!(
        production::ready_building_entity_for_owner(
            &fixture.sim,
            &fixture.rules,
            fixture.owner,
            fixture.sim.interner.get("GAPOWR").unwrap(),
        )
        .is_none()
    );
    assert!(
        !production::ready_buildings_for_owner(&fixture.sim, &fixture.rules, "Americans")
            .iter()
            .any(|ready| fixture.sim.resolve(ready.type_id) == "GAPOWR")
    );
    assert!(
        placement_facts(&frame).is_empty(),
        "the expired saved object's queued PLACE is an early guard"
    );

    install_late_stationary_blocker(&mut fixture, (8, 4));
    let failed = CommandEnvelope::new(
        fixture.owner,
        fixture.sim.session.tick + 1,
        Command::PlaceReadyBuilding {
            type_id: fixture.sim.interner.get("GAPILL").unwrap(),
            rx: 8,
            ry: 4,
        },
    );
    let frame = advance_commands(&mut fixture, &[failed]);
    assert_eq!(
        placement_facts(&frame),
        vec![LifecycleOutput::BuildingPlacementRetry { owner }]
    );
    assert_eq!(consume_frame_outputs(&mut fixture, &frame, Some(owner)), 0);
    assert!(fixture.input.targeting_mode.is_none());
    assert!(fixture.input.saved_building_placement.is_none());
}

#[test]
fn fresh_targeting_reset_drops_current_and_saved_without_consuming_factories() {
    let Some(mut fixture) = paid_ready_fixture() else {
        return;
    };
    let identities = ready_identities(&fixture);
    submit_ready_building(&mut fixture, "GAPOWR", SidebarTab::Building, (8, 4));
    assert!(press_ready_cameo(
        &mut fixture,
        "GAPILL",
        SidebarTab::Defense
    ));
    assert!(fixture.input.saved_building_placement.is_some());
    let hash = fixture.sim.state_hash();
    fixture.input.reset_targeting_for_new_world();

    assert!(fixture.input.targeting_mode.is_none());
    assert!(fixture.input.building_placement_preview.is_none());
    assert!(fixture.input.saved_building_placement.is_none());
    assert!(fixture.input.building_placement_issuer().is_none());
    assert_eq!(ready_identities(&fixture), identities);
    assert_eq!(fixture.sim.state_hash(), hash);
    assert!(press_ready_cameo(
        &mut fixture,
        "GAPOWR",
        SidebarTab::Building
    ));
    assert_eq!(
        fixture.input.building_placement_issuer(),
        Some(fixture.owner)
    );
    // Exercises the input owner's reset; fresh-world production integration
    // is validated separately at the scenario-installation boundary.
    // It does not execute held saved-load/scenario replacement or claim that
    // native RuleType pointers live across a world replacement.
}

/// HousePlace4FB520 routes zero/negative cell sentinels to the actual yard's
/// human BuildingExit443C60 failure. It never enters building Unlimbo, and
/// the human failure does not refund or destroy the completed object.
#[test]
fn sentinel_placement_keeps_human_factories_and_current_without_retry_or_eva() {
    let Some(mut fixture) = paid_ready_fixture() else {
        return;
    };
    let preview = arm_gapowr_with_preview(&mut fixture);
    let pending = fixture.input.targeting_mode.clone();
    let identities = ready_identities(&fixture);
    let product = ready_product(&fixture, "GAPOWR");
    production::object_find_factory(&fixture.sim, &fixture.rules, product, false, true)
        .expect("the sentinel's alternate ObjectFindFactory(0,1) finds the actual healthy yard");
    let credits = production::credits_for_owner(&fixture.sim, "Americans");
    let spent = fixture.sim.houses[&fixture.owner].economy.spent_credits();
    for (occurrence, cell) in [(0, 0), (u16::MAX, u16::MAX)].into_iter().enumerate() {
        let command = CommandEnvelope::new(
            fixture.owner,
            fixture.sim.session.tick + 1,
            Command::PlaceReadyBuilding {
                type_id: fixture.sim.interner.get("GAPOWR").unwrap(),
                rx: cell.0,
                ry: cell.1,
            },
        );
        let frame = advance_commands(&mut fixture, &[command]);
        assert_eq!(frame.tick.executed_commands, 1);
        assert!(!frame.tick.spawned_entities);
        assert!(placement_facts(&frame).is_empty());
        assert!(!frame.sound_events.iter().any(|event| {
            matches!(
                event,
                SimSoundEvent::CannotDeployHere { .. } | SimSoundEvent::BuildingPlaced { .. }
            )
        }));
        let owner = fixture.owner;
        assert_eq!(consume_frame_outputs(&mut fixture, &frame, Some(owner)), 0);
        assert_eq!(fixture.input.targeting_mode, pending);
        assert_same_preview(
            fixture.input.building_placement_preview.as_ref().unwrap(),
            &preview,
        );
        assert_eq!(ready_identities(&fixture), identities);
        assert_eq!(
            production::credits_for_owner(&fixture.sim, "Americans"),
            credits
        );
        assert_eq!(
            fixture.sim.houses[&fixture.owner].economy.spent_credits(),
            spent
        );
        let native = native_placement_after(
            "direct_empty_and_negative_cell_guards",
            "direct_receiver",
            occurrence,
        );
        assert_current_matches_native(&fixture, native);
        assert_saved_matches_native(&fixture, native);
    }
}
