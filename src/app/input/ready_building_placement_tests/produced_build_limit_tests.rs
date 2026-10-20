//! Map-authored negative BuildLimit through the existing retail reader,
//! paid E2 Factory/exit, House accounting, retained strip and saved world.
//! The stale semantic PRODUCE control exercises the ordinary scheduler and
//! House receiver after the cameo has gone; it is not a second cameo click
//! or a native event-byte comparison. Supplied base/terrain and timeouts have
//! the same bounds as the parent fixture. VERA save bytes are not native SAV.

use super::*;
use crate::rules::ini_parser::IniFile;

#[test]
fn paid_e2_negative_build_limit_prunes_and_refuses_stale_produce_after_load() {
    let patch = IniFile::from_str("[E2]\nBuildLimit=-1\nDontScore=no\n");
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "XMP03T4.MAP",
        &patch,
    ) else {
        return;
    };
    let Some(mut fixture) = stock_base_fixture_from_retail(
        "Russians",
        &[("NACNST", 4), ("NAPOWR", 11), ("NAHAND", 18)],
        false,
        retail,
    ) else {
        return;
    };
    assert_eq!(fixture.rules.object("E2").unwrap().build_limit, -1);
    assert!(!fixture.rules.object("E2").unwrap().dont_score);
    // The empty overlay authority is supplied just as in the parent's saved
    // HOLD arena. Loading reconstructs it through the shared map owner.
    *fixture.sim.overlay_grid_mut_for_test() = Some(crate::sim::overlay_grid::OverlayGrid::new(
        fixture.sim.session().map_width(),
        fixture.sim.session().map_height(),
    ));
    fixture
        .sim
        .session_mut_for_test()
        .set_map_name_for_test("PRODUCED-LIMIT.MAP".into());
    let owner = fixture.owner;
    let e2 = fixture.sim.interner().get("E2").unwrap();
    for _ in 0..8 {
        let frame = advance_commands(&mut fixture, &[]);
        consume_frame_outputs(&mut fixture, &frame, Some(owner));
    }
    let initial_entities = fixture.sim.entities().len();
    let initial_credits = fixture.sim.houses()[&owner].economy().credits();
    let initial_spent = fixture.sim.houses()[&owner].economy().spent_credits();
    let initial_built = fixture.sim.houses()[&owner].stats().built();
    let cost = fixture
        .sim
        .cost_of(owner, fixture.rules.object("E2").unwrap(), &fixture.rules);
    let first = press_cameo(&mut fixture, "E2", SidebarTab::Infantry, CameoPress::Left);
    assert_eq!(first.order, Some(CameoOrder::Produce));
    assert_eq!(first.eva, Some(sidebar_eva::EVA_TRAINING));
    let owner_name = fixture.sim.resolve(owner).to_owned();
    assert!(
        crate::app::input::commands::schedule_command_in_sim(
            &mut fixture.sim,
            &owner_name,
            Command::QueueProduction { type_id: e2 },
        )
        .is_some()
    );
    let frame = advance_commands(&mut fixture, &[]);
    consume_frame_outputs(&mut fixture, &frame, Some(owner));
    let held_id = fixture
        .sim
        .production()
        .factories()
        .view(owner, ProductionCategory::Infantry)
        .unwrap()
        .object
        .unwrap()
        .entity_id
        .unwrap();
    assert!(fixture.sim.entities().get(held_id).unwrap().in_limbo());
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
    assert!(delivered, "the first real paid E2 reaches its normal exit");
    assert_eq!(
        fixture.sim.houses()[&owner].stats().built(),
        initial_built + 1
    );
    assert_eq!(
        fixture.sim.houses()[&owner].economy().credits(),
        initial_credits - cost
    );
    assert_eq!(
        fixture.sim.houses()[&owner].economy().spent_credits(),
        initial_spent + cost
    );
    // Human delivery records after this frame's House rung. Record4FB804
    // raises1FC; the next ordinary House4F9265..4F9302 pass owns the strip
    // recheck. Rendering must not bypass that lifecycle handoff.
    let recheck = advance_commands(&mut fixture, &[]);
    assert!(recheck.lifecycle_outputs.iter().any(
        |output| matches!(output, LifecycleOutput::SidebarRecalculated { owner: reached } if *reached == owner)
    ));
    consume_frame_outputs(&mut fixture, &recheck, Some(owner));

    for loaded in [false, true] {
        if loaded {
            fixture.sim.publish_scenario_started();
            roundtrip_fixture_save(&mut fixture, "paid-e2-produced-limit");
        }
        let sidebar = refresh_fixture_sidebar(&mut fixture, SidebarTab::Infantry);
        assert!(
            sidebar.items.iter().all(|item| item.type_id != "E2"),
            "negative E2 limit must remove the spent cameo; loaded={loaded}"
        );
        assert_eq!(fixture.projection.cameo_request(e2), None);
        let option =
            production::all_build_options_for_owner(&fixture.sim, &fixture.rules, &owner_name)
                .into_iter()
                .find(|option| option.type_id == e2)
                .unwrap();
        assert_eq!(
            option.reason,
            Some(production::BuildDisabledReason::CannotBuild)
        );
        assert!(!option.visible_in_sidebar());

        // A stale semantic event can outlive the visible cameo. It still
        // passes through the existing command queue and real House receiver;
        // no app-side BuildLimit check or direct factory mutation is supplied.
        assert!(fixture.sim.pending_command_snapshot().is_empty());
        let issue_tick = fixture.sim.session().tick();
        assert_eq!(
            crate::app::input::commands::schedule_command_in_sim(
                &mut fixture.sim,
                &owner_name,
                Command::QueueProduction { type_id: e2 },
            ),
            Some(issue_tick)
        );
        assert_eq!(fixture.sim.pending_command_snapshot().len(), 1);
        for _ in 0..64 {
            let frame = advance_commands(&mut fixture, &[]);
            consume_frame_outputs(&mut fixture, &frame, Some(owner));
            assert!(
                production::queue_view_for_owner(&fixture.sim, &fixture.rules, &owner_name)
                    .is_empty(),
                "the House receiver refuses the stale PRODUCE; loaded={loaded}"
            );
            assert_eq!(fixture.sim.entities().len(), initial_entities + 1);
            assert_eq!(
                fixture
                    .sim
                    .entities()
                    .iter_sorted()
                    .filter(|(_, entity)| entity.owner() == owner && entity.type_ref() == e2)
                    .map(|(id, _)| id)
                    .collect::<Vec<_>>(),
                vec![held_id],
                "the same delivered E2 remains the only copy; loaded={loaded}"
            );
            let delivered = fixture.sim.entities().get(held_id).unwrap();
            assert!(delivered.is_alive() && !delivered.in_limbo());
            assert_eq!(
                fixture.sim.houses()[&owner].stats().built(),
                initial_built + 1
            );
            assert_eq!(
                fixture.sim.houses()[&owner].economy().credits(),
                initial_credits - cost
            );
            assert_eq!(
                fixture.sim.houses()[&owner].economy().spent_credits(),
                initial_spent + cost
            );
        }
        assert!(fixture.sim.pending_command_snapshot().is_empty());
        assert!(
            refresh_fixture_sidebar(&mut fixture, SidebarTab::Infantry)
                .items
                .iter()
                .all(|item| item.type_id != "E2")
        );
    }
}
