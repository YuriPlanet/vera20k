//! Authored House names through the normal retail skirmish loader. Native
//! FromName50C170 compares House+15FF4 in array order; CreateHouses68804A/
//! 688210 writes literal human/Computer names independently of display handles.
//! BuildingReadINI44F8CB skips the noncampaign current House before its ctor;
//! InfantryReadINI51FB00 has no corresponding current-House gate.
//! Original gamemd SHA256:
//! 1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c.

use super::*;
use crate::headless_scenario::{battle_session, load_session};
use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition};

#[test]
fn authored_computer_barracks_is_captured_and_delivers_paid_e2() {
    let Some((root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let directory = crate::map::source::test_support::TestDirectory::new("house-scenario-owner");
    let path = directory.path().join("house-scenario-owner.map");
    let mut bytes = assets
        .resolve_ref("XMP03T4.MAP")
        .expect("physical Hills map")
        .bytes
        .to_vec();
    // These clear-cell input positions are the ordinary placed NAPOWR/NAHAND
    // positions used by the paid retail chain. No House, entity or wallet is
    // installed after loading. The extra rows distinguish native names from
    // case-folded owner keys and prove the current-House Structures exclusion.
    for (section, records) in [
        (
            "Infantry",
            "HouseEngineer=<human player>,ENGINEER,256,30,85,0,Guard,0,None,0,0,0,1,1\n",
        ),
        (
            "Structures",
            "HouseBarracks=Computer,NAHAND,256,31,85,0,None,0,0,1,0,0,0,None,None,None,0\n\
          HousePower=Computer,NAPOWR,256,31,89,0,None,0,0,1,0,0,0,None,None,None,0\n\
          HumanMustSkip=<human player>,NACNST,256,20,85,0,None\n\
          WrongCaseMustSkip=computer,NAHAND,256,20,80,0,None\n\
          InternalKeyMustSkip=Computer1,NAPOWR,256,20,89,0,None\n",
        ),
    ] {
        // Preserve the physical retail map and insert into its first section.
        // Appending a duplicate section would leave these records out of the
        // first-section query used by both native INI and the production reader.
        let marker = format!("[{section}]");
        if let Some(start) = bytes
            .windows(marker.len())
            .position(|window| window == marker.as_bytes())
        {
            let line_end = start
                + bytes[start..]
                    .iter()
                    .position(|&byte| byte == b'\n')
                    .unwrap();
            bytes.splice(line_end + 1..line_end + 1, records.bytes());
        } else {
            bytes.extend_from_slice(format!("\n{marker}\n{records}").as_bytes());
        }
    }
    std::fs::write(&path, bytes).unwrap();
    let map_name = path.to_str().unwrap();
    let mut launch = battle_session(map_name, &[(LaunchCountry::Russia, AiDifficulty::Easy)]);
    launch.local_player_mut().name = "Computer".to_owned();
    launch.local_slot_mut().country = LaunchCountry::Russia;
    launch.local_slot_mut().start_position = LaunchStartPosition::Position(0);
    launch.opponents[0].start_position = LaunchStartPosition::Position(1);
    launch.options.starting_credits = 5000;
    launch.options.unit_count = 0;
    launch.options.tech_level = 10;
    launch.options.bases = false;
    launch.options.short_game = false;
    launch.options.crates = false;
    let scene = load_session(&root, map_name, 0x1234_5678, launch)
        .expect("normal authored retail Battle startup");
    let sim = scene.sim();
    let local = sim.interner.get("Computer").unwrap();
    let computer = sim.interner.get("Computer1").unwrap();
    assert_ne!(local, computer, "display handle does not merge Houses");
    assert_eq!(sim.session.current_house, Some(local));
    assert!(
        scene.map.entities.iter().any(|entity| {
            entity.type_id == "NAHAND"
                && entity.owner == "Computer"
                && (entity.cell_x, entity.cell_y) == (31, 85)
        }),
        "authored row survives the production map reader: {:?}",
        scene
            .map
            .entities
            .iter()
            .filter(|entity| entity.owner != "Neutral")
            .collect::<Vec<_>>()
    );
    let barracks = sim
        .entities()
        .values()
        .find(|entity| {
            sim.interner.resolve(entity.type_ref()) == "NAHAND"
                && (entity.position.rx(), entity.position.ry()) == (31, 85)
        })
        .unwrap_or_else(|| {
            panic!(
                "authored Computer barracks is successfully placed; world={:?}",
                sim.entities()
                    .values()
                    .map(|entity| (
                        sim.interner.resolve(entity.type_ref()),
                        sim.interner.resolve(entity.owner()),
                        entity.position.rx(),
                        entity.position.ry(),
                    ))
                    .collect::<Vec<_>>()
            )
        });
    assert_eq!(
        barracks.owner(),
        computer,
        "native Computer denotes the first AI even with the same human handle"
    );
    let engineers: Vec<_> = sim
        .entities()
        .values()
        .filter(|entity| sim.interner.resolve(entity.type_ref()) == "ENGINEER")
        .collect();
    assert_eq!(
        engineers.len(),
        1,
        "the current human's Infantry row is admitted"
    );
    assert_eq!(engineers[0].owner(), local);
    for cell in [(20, 85), (20, 80), (20, 89)] {
        assert!(
            sim.entities()
                .values()
                .all(|entity| (entity.position.rx(), entity.position.ry()) != cell),
            "rejected native owner row must not construct at {cell:?}"
        );
    }
    let barracks_id = barracks.stable_id();
    let engineer_id = engineers[0].stable_id();
    let mut fixture = Fixture {
        sim: scene.runtime.simulation,
        rules: scene.runtime.resources.rules,
        overlays: scene.runtime.resources.overlay_registry,
        owner: local,
        input: MatchInputState::new(HotkeyBindings::default()),
        projection: SidebarProjectionState::default(),
    };
    let before = refresh_fixture_sidebar(&mut fixture, SidebarTab::Infantry);
    assert!(before.items.iter().all(|item| item.type_id != "E2"));
    let capture = CommandEnvelope::new(
        local,
        fixture.sim.session.tick + 1,
        Command::CaptureBuilding {
            engineer_id,
            target_building_id: barracks_id,
        },
    );
    let mut captured = false;
    for step in 0..600 {
        let frame = advance_commands(
            &mut fixture,
            if step == 0 {
                std::slice::from_ref(&capture)
            } else {
                &[]
            },
        );
        consume_frame_outputs(&mut fixture, &frame, Some(local));
        if fixture
            .sim
            .entities()
            .get(barracks_id)
            .is_some_and(|building| building.owner() == local)
        {
            captured = true;
            break;
        }
    }
    assert!(
        captured,
        "the real Engineer Capture command must reach the admitted AI barracks"
    );
    assert!(
        fixture
            .sim
            .entities()
            .get(engineer_id)
            .is_none_or(|actor| !actor.lifecycle.object_alive()),
        "capture consumes the actual Engineer"
    );
    // A capture after this frame's House rung requests its next normal recheck.
    for _ in 0..8 {
        let frame = advance_commands(&mut fixture, &[]);
        consume_frame_outputs(&mut fixture, &frame, Some(local));
    }
    let available = refresh_fixture_sidebar(&mut fixture, SidebarTab::Infantry);
    assert!(
        available
            .items
            .iter()
            .any(|item| item.type_id == "E2" && item.enabled),
        "capture must unlock the retained E2 cameo"
    );
    let e2 = fixture.sim.interner.get("E2").unwrap();
    let credits = fixture.sim.houses[&local].economy.credits();
    let spent = fixture.sim.houses[&local].economy.spent_credits();
    let cost = fixture
        .sim
        .cost_of(local, fixture.rules.object("E2").unwrap(), &fixture.rules);
    let click = press_cameo(&mut fixture, "E2", SidebarTab::Infantry, CameoPress::Left);
    assert_eq!(click.order, Some(CameoOrder::Produce));
    assert_eq!(click.eva, Some(sidebar_eva::EVA_TRAINING));
    let owner_name = fixture.sim.resolve(local).to_owned();
    assert!(
        crate::app::input::commands::schedule_command_in_sim(
            &mut fixture.sim,
            &owner_name,
            Command::QueueProduction { type_id: e2 },
        )
        .is_some()
    );
    let mut held = None;
    let mut saved = false;
    let mut delivered = false;
    for _ in 0..4096 {
        let frame = advance_commands(&mut fixture, &[]);
        consume_frame_outputs(&mut fixture, &frame, Some(local));
        refresh_fixture_sidebar(&mut fixture, SidebarTab::Infantry);
        if let Some(product) = fixture
            .sim
            .production
            .factories
            .view(local, ProductionCategory::Infantry)
            .and_then(|factory| factory.object)
        {
            assert_eq!(product.type_id, e2);
            let id = product
                .entity_id
                .expect("the Factory retains the paid E2 body");
            assert!(held.is_none_or(|previous| previous == id));
            held = Some(id);
            if !saved && fixture.sim.houses[&local].economy.spent_credits() > spent {
                fixture.sim.session.set_started(true);
                roundtrip_fixture_save(&mut fixture, "scenario-house-captured-barracks");
                assert_eq!(fixture.sim.session.current_house, Some(local));
                assert_eq!(
                    fixture.sim.entities().get(barracks_id).unwrap().owner(),
                    local
                );
                saved = true;
            }
        }
        if held.is_some_and(|id| {
            fixture
                .sim
                .entities()
                .get(id)
                .is_some_and(|actor| actor.lifecycle.object_alive() && !actor.lifecycle.in_limbo())
        }) {
            delivered = true;
            break;
        }
    }
    assert!(
        saved,
        "the paid in-progress product must resume through SaveRepository/PreparedLoad"
    );
    assert!(
        delivered,
        "the captured factory must deliver its same paid E2 after loading"
    );
    let product = fixture.sim.entities().get(held.unwrap()).unwrap();
    assert_eq!(product.owner(), local);
    assert_eq!(
        product.health.current(),
        fixture.rules.object("E2").unwrap().strength
    );
    assert_eq!(fixture.sim.houses[&local].economy.credits(), credits - cost);
    assert_eq!(
        fixture.sim.houses[&local].economy.spent_credits(),
        spent + cost
    );
    for _ in 0..32 {
        let frame = advance_commands(&mut fixture, &[]);
        consume_frame_outputs(&mut fixture, &frame, Some(local));
    }
    assert!(
        fixture
            .sim
            .production
            .factories
            .view(local, ProductionCategory::Infantry)
            .is_none_or(|factory| factory.object.is_none()),
        "delivery clears the Factory's held object"
    );
    assert_eq!(
        fixture
            .sim
            .entities()
            .values()
            .filter(|actor| actor.owner() == local
                && actor.type_ref() == e2
                && actor.lifecycle.object_alive())
            .count(),
        1
    );
    assert_eq!(
        fixture.sim.houses[&local].economy.credits(),
        credits - cost,
        "no extra product or charge after exit"
    );
}
