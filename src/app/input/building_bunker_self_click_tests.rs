//! Physical Hills, authored full-HP stock tank and paid stock bunker.
//! Actual MCV deploy/production/placement precede Enter/radio/install, then
//! the production point-selection, hover and Building release owners.
//! Window-independent semantic input/selection delivery, not OS or raw replay.

use super::*;
use crate::app::input::commands::schedule_command_in_sim;
use crate::app::input::cursor::capability_cursor_for_hover;
use crate::app::input::dispatch::selected_stable_ids_in_order;
use crate::app::input::entity_pick::{
    compute_click_selection_snapshot_with_playfield, hover_target_at_point,
};
use crate::app::types::CursorFeedbackKind;
use crate::headless_scenario::{HeadlessScenario, battle_session, load_session};
use crate::sim::production::{
    placement_preview_for_owner_with_overlays, ready_building_entity_for_owner,
};
use crate::sim::world::{FrameEffects, SimFrameOutput, SimSoundEvent, TickLane};
use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition};

fn insert_records(map: &mut String, section: &str, records: &str) {
    let marker = format!("[{section}]");
    if let Some(start) = map.find(&marker) {
        let end = start + map[start..].find('\n').expect("physical section newline") + 1;
        map.insert_str(end, records);
    } else {
        map.push_str(&format!("\n{marker}\n{records}"));
    }
}

fn frame(scene: &mut HeadlessScenario) -> SimFrameOutput {
    let commands = scene.runtime.simulation.take_due_commands();
    let output = scene
        .runtime
        .advance_frame(
            &commands,
            crate::headless_scenario::SIM_TICK_MS,
            TickLane::Ordinary,
            FrameEffects::default(),
        )
        .expect("ordinary bound-resource frame");
    assert!(output.tick.frame_committed && !output.network_waiting && !output.outcome_waiting);
    output
}

/// Actual point pick/admission followed by the existing committed Select owner.
/// Bulk-selection's dynamic bunker exclusion is deliberately not this route.
fn point_select(scene: &mut HeadlessScenario, id: u64, local: &str) -> HoverTargetKindWithId {
    let selected = {
        let sim = scene.sim();
        let rules = &scene.runtime.resources.rules;
        let actor = sim.entities().get(id).unwrap();
        let point = crate::render::locomotor_visual::screen_position(actor);
        let hover = hover_target_at_point(sim, point.0, point.1, local, false)
            .expect("ordinary visible object point pick");
        assert_eq!(
            hover.stable_id, id,
            "natural actor pose must actually be picked"
        );
        let encounter: Vec<_> = sim.display_layers().ordered_ids().copied().collect();
        let current = selected_stable_ids_in_order(Some(sim), Some(rules), &[], false);
        let mutation = compute_click_selection_snapshot_with_playfield(
            sim,
            &encounter,
            &current,
            Some(sim.fog()),
            Some(local),
            point.0,
            point.1,
            false,
            Some(rules),
            Some(sim.interner()),
            true,
            sim.resolved_terrain(),
        )
        .expect("actual click selection mutation");
        assert!(mutation.clear && mutation.deselect.is_empty());
        assert_eq!(mutation.select, [id]);
        assert!(sim.can_select_object(id, rules) && sim.techno_select_admits(id));
        mutation.select
    };
    assert!(scene.runtime.apply_command_for_test(
        local,
        &Command::Select {
            entity_ids: selected,
            additive: false,
        },
    ));
    let sim = scene.sim();
    let rules = &scene.runtime.resources.rules;
    assert_eq!(
        selected_stable_ids_in_order(Some(sim), Some(rules), &[], false),
        [id]
    );
    let point = crate::render::locomotor_visual::screen_position(sim.entities().get(id).unwrap());
    let hover = hover_target_at_point(sim, point.0, point.1, local, false).unwrap();
    assert_eq!(hover.stable_id, id);
    hover
}

fn query_snapshot(scene: &HeadlessScenario, bunker: u64, tank: u64) -> serde_json::Value {
    let sim = scene.sim();
    serde_json::json!({
        "bunker": sim.entities().get(bunker).unwrap(),
        "tank": sim.entities().get(tank).unwrap(),
        "sound_facts": format!("{:?}", sim.sound_events()),
        "hash": sim.state_hash(),
        "main": sim.main_rng().native_state_hex(),
        "scenario": sim.scenario_rng().native_state_hex(),
        "mapgen": sim.mapgen_rng().native_state_hex(),
    })
}

#[test]
#[ignore = "requires physical Hills/Rules/ART/TMP; paid bunker, stock Enter/install and app point input"]
fn stock_installed_tank_bunker_self_click_is_inert_but_its_tank_can_deploy() {
    let (root, assets) = crate::rules::retail_ini_fixture::retail_assets()
        .expect("selected regression requires physical retail assets");
    let physical = assets.resolve_ref("XMP03T4.MAP").expect("physical Hills");
    let mut map = String::from_utf8(physical.bytes.to_vec()).unwrap();
    // A full-health256 stock Unit is a legal current-House map actor.
    // BuildingReadINI44F8CB refuses current-House Structures in skirmish,
    // so the own bunker below must instead be paid and placed normally.
    // No type, terrain, actor, wallet or random-stream state is patched.
    insert_records(
        &mut map,
        "Units",
        "BunkerSelfTank=<human player>,LTNK,256,30,85,64,Guard,None,0,-1,0,-1,1,1\n",
    );
    let directory = crate::map::source::test_support::TestDirectory::new("bunker-self-click");
    let path = directory.path().join("bunker-self-click.map");
    std::fs::write(&path, map).unwrap();
    let name = path.to_str().unwrap();
    let mut launch = battle_session(name, &[(LaunchCountry::Russia, AiDifficulty::Easy)]);
    launch.local_slot_mut().country = LaunchCountry::Yuri;
    launch.local_slot_mut().start_position = LaunchStartPosition::Position(0);
    launch.opponents[0].start_position = LaunchStartPosition::Position(1);
    launch.options.unit_count = 0;
    launch.options.bases = true;
    launch.options.short_game = false;
    launch.options.crates = false;
    launch.options.shroud = false;
    launch.options.fog_of_war = false;
    let mut scene = load_session(&root, name, 0x458e50, launch)
        .expect("physical Hills with a stock Yuri start and authored full-HP Lasher");
    let (owner, local, mcv, tank) = {
        let sim = scene.sim();
        let owner = sim.session().current_house().unwrap();
        let tank = sim
            .entities()
            .values()
            .find(|e| {
                e.owner() == owner
                    && sim.resolve(e.type_ref()) == "LTNK"
                    && (e.position.rx(), e.position.ry()) == (30, 85)
            })
            .expect("actual authored current-House LTNK admitted")
            .stable_id();
        let mcvs: Vec<_> = sim
            .entities()
            .values()
            .filter(|e| e.owner() == owner && sim.resolve(e.type_ref()) == "PCV")
            .collect();
        assert_eq!(mcvs.len(), 1, "the ordinary Yuri start creates one PCV");
        assert!(mcvs[0].lifecycle.object_alive() && mcvs[0].in_playfield() && !mcvs[0].in_limbo());
        assert_eq!(
            mcvs[0].health.current(),
            scene
                .runtime
                .resources
                .rules
                .object("PCV")
                .unwrap()
                .strength
        );
        assert!(sim.logic_order().contains(&mcvs[0].stable_id()));
        assert!(
            !sim.entities()
                .values()
                .any(|e| { e.owner() == owner && sim.resolve(e.type_ref()) == "NATBNK" })
        );
        (
            owner,
            sim.resolve(owner).to_owned(),
            mcvs[0].stable_id(),
            tank,
        )
    };
    schedule_command_in_sim(
        &mut scene.runtime.simulation,
        &local,
        Command::DeployMcv { entity_id: mcv },
    )
    .expect("ordinary startup PCV Deploy admission");
    let mut yard = None;
    for _ in 0..1200 {
        frame(&mut scene);
        yard = scene
            .sim()
            .entities()
            .values()
            .find(|e| {
                e.owner() == owner
                    && scene.sim().resolve(e.type_ref()) == "YACNST"
                    && e.lifecycle.object_alive()
                    && e.in_playfield()
                    && !e.in_limbo()
                    && e.building_art.actually_placed()
                    && !e.building_up()
                    && e.building_online()
            })
            .map(|e| e.stable_id());
        if yard.is_some() {
            break;
        }
    }
    let yard = yard.expect("ordinary scheduled PCV conversion establishes the YACNST");
    assert!(scene.sim().logic_order().contains(&yard));
    assert!(
        scene
            .sim()
            .entities()
            .get(mcv)
            .is_none_or(|e| { !e.lifecycle.object_alive() || !e.in_playfield() })
    );
    let (bunker_type, credits_before, spent_before) = {
        let sim = scene.sim();
        let rules = &scene.runtime.resources.rules;
        let object = rules.object("NATBNK").unwrap();
        assert_eq!(object.cost, 400);
        assert_eq!(object.power, 0);
        assert!(!object.powered);
        let [_, frames, rate] = rules.buildup_control("NATBNK");
        assert!(rules.has_buildup("NATBNK") && frames > 1 && rate > 0);
        let economy = sim.houses()[&owner].economy();
        (
            sim.interner().get("NATBNK").unwrap(),
            economy.credits(),
            economy.spent_credits(),
        )
    };
    // The shared window-independent scheduler carries QueueProduction and
    // PlaceReadyBuilding semantically; AppState's bound-rule Produce/Place
    // byte encoder is outside this fixture. The real Factory/payment/placement
    // and subsequent ordinary Logic owners still execute without substitution.
    schedule_command_in_sim(
        &mut scene.runtime.simulation,
        &local,
        Command::QueueProduction {
            type_id: bunker_type,
        },
    )
    .expect("ordinary app admission of paid NATBNK production");
    let (mut paid, mut ready) = (false, None);
    for _ in 0..1200 {
        frame(&mut scene);
        let sim = scene.sim();
        paid |= sim.houses()[&owner].economy().spent_credits() > spent_before;
        ready = ready_building_entity_for_owner(
            sim,
            &scene.runtime.resources.rules,
            owner,
            bunker_type,
        );
        if ready.is_some() {
            break;
        }
    }
    let bunker = ready.expect("ordinary paid Factory completes and holds its NATBNK");
    assert!(paid, "normal Factory steps actually charge the human House");
    {
        let sim = scene.sim();
        let economy = sim.houses()[&owner].economy();
        assert_eq!(economy.spent_credits() - spent_before, 400);
        assert_eq!(credits_before - economy.credits(), 400);
        let held = sim.entities().get(bunker).unwrap();
        assert!(held.in_limbo() && !held.in_playfield());
        assert_eq!(held.owner(), owner);
        assert_eq!(sim.resolve(held.type_ref()), "NATBNK");
        assert!(
            placement_preview_for_owner_with_overlays(
                sim,
                &scene.runtime.resources.rules,
                &local,
                "NATBNK",
                31,
                85,
                Some(&scene.runtime.resources.overlay_registry),
            )
            .expect("real stock NATBNK placement query")
            .valid,
            "the physical Hills cell must be legally adjacent and clear"
        );
    }
    schedule_command_in_sim(
        &mut scene.runtime.simulation,
        &local,
        Command::PlaceReadyBuilding {
            type_id: bunker_type,
            rx: 31,
            ry: 85,
        },
    )
    .expect("ordinary app placement admission");
    let mut saw_buildup = false;
    for _ in 0..1200 {
        frame(&mut scene);
        let building = scene.sim().entities().get(bunker).unwrap();
        saw_buildup |= building.in_playfield() && building.building_up();
        if building.in_playfield()
            && !building.in_limbo()
            && building.building_art.actually_placed()
            && !building.building_up()
            && building.building_online()
        {
            break;
        }
    }
    assert!(
        saw_buildup,
        "actual finite physical buildup runs before entry"
    );
    {
        let sim = scene.sim();
        let building = sim.entities().get(bunker).unwrap();
        assert!(building.building_art.actually_placed() && !building.building_up());
        assert!(building.building_online());
        assert_eq!((building.position.rx(), building.position.ry()), (31, 85));
        assert_eq!(building.owner(), owner);
        assert!(
            ready_building_entity_for_owner(
                sim,
                &scene.runtime.resources.rules,
                owner,
                bunker_type
            )
            .is_none()
        );
    }
    {
        let sim = scene.sim();
        let rules = &scene.runtime.resources.rules;
        for (id, kind) in [(bunker, "NATBNK"), (tank, "LTNK")] {
            let actor = sim.entities().get(id).unwrap();
            assert_eq!(actor.health.current(), rules.object(kind).unwrap().strength);
            assert!(actor.lifecycle.object_alive() && actor.in_playfield() && !actor.in_limbo());
            assert!(sim.logic_order().contains(&id));
        }
        let object = rules.object("NATBNK").unwrap();
        assert!(object.bunker && !object.can_be_occupied);
        assert!(!object.infantry_absorb && !object.unit_absorb && !object.gap_generator);
        assert!(object.factory_type().is_none());
        let b = sim.entities().get(bunker).unwrap();
        assert!(b.bunker.occupant().is_none() && b.passenger_role.occupants().is_empty());
        let terrain = sim.resolved_terrain().unwrap();
        for (x, y) in [(30, 85), (31, 85), (32, 85), (31, 86), (32, 86)] {
            let cell = terrain.cell(x, y).unwrap();
            assert!(!cell.outside_playfield && !cell.has_bridge_deck && !cell.has_ramp);
            assert!(!cell.base_build_blocked && !cell.base_ground_walk_blocked);
            assert!(!cell.terrain_object_blocks && !cell.overlay_blocks);
        }
    }
    point_select(&mut scene, tank, &local);
    let orders = {
        let sim = scene.sim();
        let rules = &scene.runtime.resources.rules;
        let b = sim.entities().get(bunker).unwrap();
        let point = crate::render::locomotor_visual::screen_position(b);
        let hover = hover_target_at_point(sim, point.0, point.1, &local, false).unwrap();
        assert_eq!(hover.stable_id, bunker);
        let mut selected = vec![tank];
        let orders = entry_orders(
            sim,
            rules,
            &[tank],
            &mut selected,
            &hover,
            OrderModifier::Normal,
            false,
            Some(&scene.runtime.resources.overlay_registry),
        )
        .expect("real stock CAN_LOAD entry action");
        assert!(selected.is_empty());
        assert_eq!(
            orders,
            [Command::clicked_mission(
                tank,
                crate::sim::mission::MissionType::Enter,
                TargetKind::Entity(bunker)
            )]
        );
        orders
    };
    for command in orders {
        schedule_command_in_sim(&mut scene.runtime.simulation, &local, command)
            .expect("ordinary app entry command admission");
    }
    let (mut walls_up, mut raised_slots) = (0, false);
    for _ in 0..1200 {
        let output = frame(&mut scene);
        walls_up += output
            .sound_events
            .iter()
            .filter(|e| matches!(e, SimSoundEvent::BunkerWallsUp { .. }))
            .count();
        let b = scene.sim().entities().get(bunker).unwrap();
        raised_slots |=
            b.building_art.anim_slots()[10].is_some() && b.building_art.anim_slots()[11].is_some();
        if b.bunker.occupant() == Some(tank) {
            break;
        }
    }
    assert_eq!(
        walls_up, 1,
        "scheduled install emits the actual wall-rise fact"
    );
    assert!(
        raised_slots,
        "physical wall-up AnimClass slots were actually created"
    );
    {
        let sim = scene.sim();
        let b = sim.entities().get(bunker).unwrap();
        let t = sim.entities().get(tank).unwrap();
        assert_eq!(b.bunker.occupant(), Some(tank));
        assert_eq!(t.bunker.link().installed_in(), Some(bunker));
        assert!(
            b.passenger_role.occupants().is_empty(),
            "tank link is not garrison+694"
        );
        assert!(
            b.passenger_role
                .cargo()
                .is_none_or(|cargo| cargo.is_empty())
        );
        assert_eq!(
            serde_json::to_value(b.bunker_runtime).unwrap()["state"],
            "Occupied"
        );
        assert!(!t.in_limbo() && t.in_playfield() && !t.selection.is_selected());
    }
    let hover = point_select(&mut scene, bunker, &local);
    let before = query_snapshot(&scene, bunker, tank);
    let (cursor, click) = {
        let sim = scene.sim();
        let rules = &scene.runtime.resources.rules;
        (
            capability_cursor_for_hover(
                sim,
                &[bunker],
                Some(bunker),
                &hover,
                Some(rules),
                Some(&scene.runtime.resources.overlay_registry),
                OrderModifier::Normal,
                false,
                false,
            ),
            building_selection_click(
                sim,
                rules,
                &scene.runtime.resources.overlay_registry,
                &[bunker],
                &[bunker],
                owner,
                Some(&hover),
                None,
                (31, 85),
                OrderModifier::Normal,
                false,
            ),
        )
    };
    // First intended raw failure: unsupported Building Eject after REAL install.
    // Do not author or deliver a negative Eject event just to exercise teardown.
    let has_order = matches!(&click, BuildingClick::Orders(commands, _) if !commands.is_empty());
    assert!(
        !has_order,
        "stock installed bunker self-click must issue no order: {click:?}"
    );
    assert_eq!(cursor, CursorFeedbackKind::Default);
    assert_eq!(
        query_snapshot(&scene, bunker, tank),
        before,
        "hover/release queries preserve links, missions, wall slots, sound and RNG"
    );
    let idle = frame(&mut scene);
    assert!(
        !idle
            .sound_events
            .iter()
            .any(|e| matches!(e, SimSoundEvent::BunkerWallsDown { .. }))
    );
    assert_eq!(
        scene
            .sim()
            .entities()
            .get(bunker)
            .unwrap()
            .bunker
            .occupant(),
        Some(tank)
    );
    assert_eq!(
        scene
            .sim()
            .entities()
            .get(tank)
            .unwrap()
            .bunker
            .link()
            .installed_in(),
        Some(bunker)
    );

    // An installed tank is excluded from bulk selection, but its real natural
    // parked head remains point-pickable. Require the actual picker; never set
    // selection or invent an installed-tank hover identity to prove this arm.
    let hover = point_select(&mut scene, tank, &local);
    let inside = crate::sim::movement::ground_pose::position_world_xy(
        &scene.sim().entities().get(tank).unwrap().position,
    );
    let deploy = {
        let sim = scene.sim();
        let rules = &scene.runtime.resources.rules;
        assert_eq!(
            capability_cursor_for_hover(
                sim,
                &[tank],
                Some(tank),
                &hover,
                Some(rules),
                Some(&scene.runtime.resources.overlay_registry),
                OrderModifier::Normal,
                false,
                false
            ),
            CursorFeedbackKind::Deploy
        );
        self_click_command(
            sim,
            rules,
            Some(&scene.runtime.resources.overlay_registry),
            owner,
            &[tank],
            Some(&hover),
            false,
        )
        .expect("installed UNIT self Deploy")
    };
    assert_eq!(deploy, Command::DeployMcv { entity_id: tank });
    let tick = schedule_command_in_sim(&mut scene.runtime.simulation, &local, deploy)
        .expect("ordinary app self-click command admission");
    {
        let record = {
            let queues = scene.runtime.simulation.network_command_queues();
            let records = queues.local_records().unwrap();
            crate::sim::command::CommandRecord::clone(
                records.last().expect("self-click reached ordinary OutList"),
            )
        };
        let sim = scene.sim();
        assert_eq!(
            record.opcode(),
            4,
            "self-click is MegaMission, not D-key Event9"
        );
        assert_eq!(
            sim.decode_native_command_record(&record, tick)
                .unwrap()
                .payload,
            Command::MegaMission {
                entity_id: tank,
                mission: crate::sim::mission::MissionType::Unload,
                target: None,
                destination: None,
            }
        );
    }
    let mut walls_down = 0;
    for _ in 0..600 {
        let output = frame(&mut scene);
        walls_down += output
            .sound_events
            .iter()
            .filter(|e| matches!(e, SimSoundEvent::BunkerWallsDown { .. }))
            .count();
        if scene
            .sim()
            .entities()
            .get(bunker)
            .unwrap()
            .bunker
            .occupant()
            .is_none()
        {
            break;
        }
    }
    assert_eq!(
        walls_down, 1,
        "Unit Deploy reaches scheduled Unload/normal release"
    );
    let sim = scene.sim();
    let b = sim.entities().get(bunker).unwrap();
    let t = sim.entities().get(tank).unwrap();
    assert!(b.bunker.occupant().is_none() && t.bunker.link().installed_in().is_none());
    assert!(b.building_art.anim_slots()[10].is_none() && b.building_art.anim_slots()[11].is_none());
    assert!(
        t.mission.current().known() == Some(crate::sim::mission::MissionType::Move)
            || t.mission.queued().known() == Some(crate::sim::mission::MissionType::Move)
    );
    assert!(t.lifecycle.object_alive() && t.in_playfield() && !t.in_limbo());
    let mut drove_out = false;
    for _ in 0..300 {
        frame(&mut scene);
        let t = scene.sim().entities().get(tank).unwrap();
        let xy = crate::sim::movement::ground_pose::position_world_xy(&t.position);
        if (xy[0] - inside[0]).abs().max((xy[1] - inside[1]).abs()) > 256 {
            drove_out = true;
            break;
        }
    }
    assert!(drove_out, "released tank follows its real exit Move");
}
