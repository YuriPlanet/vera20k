//! Building4436F0: archive the clicked Cell, then enqueue SELL16.
use super::*;
use crate::rules::ruleset::RuleSet;
use crate::sim::mission::MissionType;
use crate::sim::world::Simulation;

fn step(sim: &mut Simulation, rules: &RuleSet) {
    let commands = sim.take_due_commands();
    sim.advance_app_frame(
        &commands,
        rules,
        None,
        33,
        crate::sim::world::TickLane::Ordinary,
        Default::default(),
    )
    .unwrap();
}

#[test]
fn retail_mcvs_repack_and_follow_the_ground_click_after_deployment() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let mut rules = retail.rules;
    for yard in ["GACNST", "NACNST", "YACNST"] {
        // This INI fixture does not load SHPs. Supply a finite build-up
        // animation binding; its timing is an authored test input.
        rules.set_buildup_control_for_test(yard, [0, 29, 1]);
    }
    for (mcv, yard_type, contact) in [("AMCV", "GACNST"), ("SMCV", "NACNST"), ("PCV", "YACNST")]
        .into_iter()
        .flat_map(|(mcv, yard)| [false, true].map(move |contact| (mcv, yard, contact)))
    {
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x4436F0);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner().get("Americans").unwrap();
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.session_mut_for_test()
            .game_options_mut_for_test()
            .mcv_redeploy = true;
        sim.session_mut_for_test()
            .game_options_mut_for_test()
            .short_game = false;
        sim.set_input_delay_ticks(0);
        sim.spawn_object("GAPOWR", "Russians", 3, 3, 0, &rules)
            .unwrap();
        let actor = sim
            .spawn_object(mcv, "Americans", 12, 12, 96, &rules)
            .unwrap();
        assert!(sim.apply_command(
            "Americans",
            &Command::DeployMcv { entity_id: actor },
            Some(&rules)
        ));
        for _ in 0..120 {
            step(&mut sim, &rules);
        }
        let yard = sim
            .entities()
            .values()
            .find(|e| sim.interner().resolve(e.type_ref()) == yard_type)
            .unwrap_or_else(|| panic!("{mcv} must deploy into {yard_type}"))
            .stable_id();
        assert!(
            !sim.entities().get(yard).unwrap().building_up(),
            "{yard_type}"
        );
        let peer = sim
            .spawn_object("GAPOWR", "Americans", 5, 5, 0, &rules)
            .unwrap();
        if contact {
            // Supplied existing radio state. The sale owner must retire both
            // sides, rather than requiring an empty link at input time.
            sim.entities_mut_for_test()
                .get_mut(yard)
                .unwrap()
                .radio_contacts
                .insert(peer)
                .unwrap();
            sim.entities_mut_for_test()
                .get_mut(peer)
                .unwrap()
                .radio_contacts
                .insert(yard)
                .unwrap();
        }
        let (orders, announce) =
            building_cell_orders(&sim, &rules, None, &[yard], owner, (23, 20), false, false);
        assert!(!announce);
        assert!(
            matches!(orders.as_slice(), [Command::SetRally { .. }, Command::SellBuilding { entity_id }]
            if *entity_id == yard),
            "{yard_type}: {orders:?}"
        );
        let goal = match orders[0] {
            Command::SetRally { rx, ry, .. } => (rx, ry),
            _ => unreachable!(),
        };
        let envelopes: Vec<_> = orders
            .into_iter()
            .map(|payload| CommandEnvelope::new(owner, sim.session().tick(), payload))
            .collect();
        sim.queue_commands(envelopes);
        let mut converted = None;
        for _ in 0..80 {
            step(&mut sim, &rules);
            converted = sim
                .entities()
                .values()
                .find(|e| sim.interner().resolve(e.type_ref()) == mcv)
                .map(|e| e.stable_id());
            if converted.is_some() {
                break;
            }
        }
        assert!(
            sim.entities().get(yard).is_none(),
            "{yard_type} did not repack: {:?}",
            sim.entities().get(yard).map(|e| (
                e.mission.effective(),
                e.archive_target(),
                e.building_down()
            ))
        );
        let converted = converted.expect("MCV must return");
        assert!(sim.entities().get(peer).unwrap().radio_contacts.is_empty());
        let unit = sim.entities().get(converted).unwrap();
        assert!(
            [unit.mission.current(), unit.mission.queued()]
                .iter()
                .any(|mission| mission.known() == Some(MissionType::Move)),
            "{mcv} must receive the archived move"
        );
        for _ in 0..600 {
            step(&mut sim, &rules);
            let unit = sim.entities().get(converted).unwrap();
            if (unit.position.rx(), unit.position.ry()) == goal {
                break;
            }
        }
        let unit = sim.entities().get(converted).unwrap();
        assert_eq!((unit.position.rx(), unit.position.ry()), goal, "{mcv}");
    }
}

/// Original Building4436F0 admits both empty and populated RadioClass
/// contacts; option/mode/House/mind-control gates are44374F..443778.
/// Contact membership is a supplied state here, not a stock producer claim.
#[test]
fn construction_yard_ground_orders_match_native_redeploy_admission() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    for (name, enabled, skirmish, human, contact, admitted) in [
        ("enabled", true, true, true, false, true),
        ("radio contact", true, true, true, true, true),
        ("disabled", false, true, true, false, false),
        ("campaign", true, false, true, false, false),
        ("computer", true, true, false, false, false),
    ] {
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x4436F0);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner().get("Americans").unwrap();
        sim.houses_mut_for_test()
            .get_mut(&owner)
            .unwrap()
            .set_is_human_for_test(human);
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(skirmish),
        );
        sim.session_mut_for_test()
            .game_options_mut_for_test()
            .mcv_redeploy = enabled;
        let yard = sim
            .spawn_object("GACNST", "Americans", 12, 12, 0, &rules)
            .unwrap();
        let peer = sim
            .spawn_object("GAPOWR", "Americans", 5, 5, 0, &rules)
            .unwrap();
        if contact {
            sim.entities_mut_for_test()
                .get_mut(yard)
                .unwrap()
                .radio_contacts
                .insert(peer)
                .unwrap();
        }
        assert_eq!(
            sim.takes_rally_point(yard, "Americans", &rules),
            admitted,
            "{name}"
        );
        let (orders, _) =
            building_cell_orders(&sim, &rules, None, &[yard], owner, (23, 20), false, false);
        assert_eq!(!orders.is_empty(), admitted, "{name}");
    }
}

/// A retail rival arena where the Americans are the player, in a skirmish
/// with MCV redeploy on, with its lower right corner flooded.
fn yard_arena(rules: &RuleSet) -> (Simulation, InternedId) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x447540);
    crate::sim::arena_fixture::flood(
        &mut sim,
        rules,
        32,
        (24..32).flat_map(|y| (24..32).map(move |x| (x, y))),
    );
    sim.resolve_type_handles(rules);
    let owner = sim.interner().get("Americans").unwrap();
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session_mut_for_test()
        .game_options_mut_for_test()
        .mcv_redeploy = true;
    (sim, owner)
}

/// The left click of a selection of buildings only, as the player's.
fn click(
    sim: &Simulation,
    rules: &RuleSet,
    selection: &[u64],
    hover: Option<(HoverTargetKind, u64)>,
    cell: (u16, u16),
    modifier: OrderModifier,
) -> BuildingClick {
    click_over(sim, rules, selection, hover, None, cell, modifier)
}

/// [`click`] with a Terrain object (`terrain`) under the cursor.
fn click_over(
    sim: &Simulation,
    rules: &RuleSet,
    selection: &[u64],
    hover: Option<(HoverTargetKind, u64)>,
    terrain: Option<u64>,
    cell: (u16, u16),
    modifier: OrderModifier,
) -> BuildingClick {
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::empty();
    let hover = hover.map(|(kind, stable_id)| HoverTargetKindWithId { kind, stable_id });
    let force_move_key = matches!(
        modifier,
        OrderModifier::ForceMove | OrderModifier::GuardArea
    );
    building_selection_click(
        sim,
        rules,
        &registry,
        selection,
        selection,
        sim.session().current_house().unwrap(),
        hover.as_ref(),
        terrain,
        cell,
        modifier,
        force_move_key,
    )
}

fn rallies(click: &BuildingClick) -> Vec<u64> {
    match click {
        BuildingClick::Orders(orders, _) => orders
            .iter()
            .flat_map(|order| match order {
                Command::SetRally { producer_ids, .. } => producer_ids.clone(),
                _ => Vec::new(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The buildings that archive an object (event 0x1E) and the object.
fn archives(click: &BuildingClick) -> Vec<(u64, u64)> {
    match click {
        BuildingClick::Orders(orders, _) => orders
            .iter()
            .filter_map(|order| match order {
                Command::ArchiveObject {
                    entity_id,
                    target_id,
                } => Some((*entity_id, *target_id)),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn packs_up(click: &BuildingClick) -> bool {
    matches!(click, BuildingClick::Orders(orders, _)
        if orders.iter().any(|order| matches!(order, Command::SellBuilding { .. })))
}

/// Building What_Action_OnObject (`0x00447210`) over another House's unit
/// is the Techno tail's Select for an unarmed factory or Construction Yard,
/// with or without Ctrl, so the click selects the unit; Building
/// ClickedAction_Object (`0x00443410`) neither rallies nor packs up. With Alt
/// the force-move rung answers Move: the war factory rallies at the unit's
/// Cell, and the yard only archives it, so it still does not pack up.
/// Ctrl+Alt over the player's own unit answers 0x1A, which sends nothing. An
/// enemy the player cannot see is no object: its Cell takes the click.
#[test]
fn a_selected_factory_or_yard_selects_another_houses_unit_instead_of_rallying() {
    use HoverTargetKind::{EnemyUnit, FriendlyUnit, HiddenEnemy};
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let (mut sim, _) = yard_arena(&rules);
    let factory = sim
        .spawn_object("GAWEAP", "Americans", 6, 6, 0, &rules)
        .unwrap();
    let yard = sim
        .spawn_object("GACNST", "Americans", 12, 12, 0, &rules)
        .unwrap();
    let enemy = sim
        .spawn_object("MTNK", "Russians", 20, 18, 64, &rules)
        .unwrap();
    let own = sim
        .spawn_object("MTNK", "Americans", 20, 10, 64, &rules)
        .unwrap();
    for selection in [vec![factory], vec![yard], vec![factory, yard]] {
        let over_enemy = |modifier| {
            click(
                &sim,
                &rules,
                &selection,
                Some((EnemyUnit, enemy)),
                (20, 17),
                modifier,
            )
        };
        for modifier in [OrderModifier::Normal, OrderModifier::ForceFire] {
            assert_eq!(over_enemy(modifier), BuildingClick::Select, "{selection:?}");
        }
        let alt = over_enemy(OrderModifier::ForceMove);
        assert!(!packs_up(&alt), "{selection:?}: {alt:?}");
        let expected = if selection.contains(&factory) {
            vec![factory]
        } else {
            Vec::new()
        };
        assert_eq!(rallies(&alt), expected, "{selection:?}: {alt:?}");
        let archived = if selection.contains(&yard) {
            vec![(yard, enemy)]
        } else {
            Vec::new()
        };
        assert_eq!(archives(&alt), archived, "{selection:?}: {alt:?}");
        assert_eq!(
            click(
                &sim,
                &rules,
                &selection,
                Some((FriendlyUnit, own)),
                (20, 10),
                OrderModifier::GuardArea,
            ),
            BuildingClick::Orders(Vec::new(), false),
            "{selection:?}"
        );
        // The hidden enemy's Cell is an ordinary ground click: the factory
        // rallies there, and the yard finds the Cell held by the tank
        // (Is_Clear_To_Build `0x0047C620`), so it does not pack up.
        let hidden = click(
            &sim,
            &rules,
            &selection,
            Some((HiddenEnemy, enemy)),
            (20, 18),
            OrderModifier::Normal,
        );
        assert!(!packs_up(&hidden), "{hidden:?}");
        assert_eq!(
            rallies(&hidden).contains(&factory),
            selection.contains(&factory),
            "{hidden:?}"
        );
    }
    // The factory's rally comes from the unit's own Cell, not the clicked one.
    let alt = click(
        &sim,
        &rules,
        &[factory],
        Some((EnemyUnit, enemy)),
        (20, 17),
        OrderModifier::ForceMove,
    );
    let expected = sim
        .factory_rally_cell_input(factory, (20, 18), &rules)
        .unwrap();
    assert!(
        matches!(&alt, BuildingClick::Orders(orders, _)
            if matches!(orders.as_slice(), [Command::SetRally { rx, ry, .. }]
                if Some((*rx, *ry)) == expected)),
        "{alt:?} vs {expected:?}"
    );
}

/// Building What_Action_OnCell (`0x00447540`) asks an UndeploysInto type's
/// own Can_Enter_Cell (`0x00449440`) even for the cursor: a Construction
/// Yard over water answers NoMove, so the click neither archives nor packs
/// up (`0x004436F0`). Shift answers Move before that test, and a Slave
/// Miner's yard, which undeploys into a ResourceGatherer, always moves.
#[test]
fn a_yard_neither_archives_nor_packs_up_over_a_cell_it_cannot_stand_on() {
    use crate::sim::world::{CellActionQuery, MoveAction};
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let (mut sim, owner) = yard_arena(&rules);
    let yard = sim
        .spawn_object("GACNST", "Americans", 12, 12, 0, &rules)
        .unwrap();
    let slave_yard = sim
        .spawn_object("YAREFN", "Americans", 4, 20, 0, &rules)
        .unwrap();
    let enemy_yard = sim
        .spawn_object("GACNST", "Russians", 4, 4, 0, &rules)
        .unwrap();
    // The cursor's query: the mouse-over argument does not skip the test.
    let action = |id, cell, shift| {
        sim.cell_move_action(
            owner,
            id,
            cell,
            CellActionQuery {
                mouse_over: true,
                shift,
            },
            &rules,
            None,
        )
        .unwrap()
    };
    let (land, water) = ((20, 12), (27, 27));
    assert_eq!(action(yard, land, false), MoveAction::Move);
    assert_eq!(action(yard, water, false), MoveAction::NoMove);
    assert_eq!(action(yard, water, true), MoveAction::Move);
    assert_eq!(action(slave_yard, water, false), MoveAction::Move);
    assert_eq!(action(enemy_yard, land, false), MoveAction::None);

    let ground = |cell| click(&sim, &rules, &[yard], None, cell, OrderModifier::Normal);
    let on_land = ground(land);
    assert!(
        matches!(&on_land, BuildingClick::Orders(orders, _)
            if matches!(orders.as_slice(), [Command::SetRally { .. }, Command::SellBuilding { entity_id }]
                if *entity_id == yard)),
        "{on_land:?}"
    );
    assert_eq!(ground(water), BuildingClick::Orders(Vec::new(), false));
}

/// SetRallyPoint's FNPC (`0x0056DC20`) searches only 31 rings around the
/// click. A force-move click on open water farther than that from the yard's
/// island misses, and the yard, which passed the redeploy gates, archives
/// the Cell under its own GetCoords before SELL (`0x00443A7B..0x00443B4B`),
/// replacing an older rally Cell so the MCV it packs into stays.
#[test]
fn a_yard_click_whose_rally_search_misses_archives_its_own_cell() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let size = 64;
    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(&rules, 0x443A7B, size);
    crate::sim::arena_fixture::flood(
        &mut sim,
        &rules,
        size,
        (0..size)
            .flat_map(|y| (0..size).map(move |x| (x, y)))
            .filter(|&(x, y)| x >= 16 || y >= 16),
    );
    sim.resolve_type_handles(&rules);
    let owner = sim.interner().get("Americans").unwrap();
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session_mut_for_test()
        .game_options_mut_for_test()
        .mcv_redeploy = true;
    sim.set_input_delay_ticks(0);
    let yard = sim
        .spawn_object("GACNST", "Americans", 6, 6, 0, &rules)
        .unwrap();
    sim.entities_mut_for_test()
        .get_mut(yard)
        .unwrap()
        .set_archive_target(Some(TargetKind::Cell(12, 12)));
    let own = sim.object_coords_cell(yard).unwrap();
    // GetCoords of the 4x4 yard at (6,6): 6*256 + 128 + 3*128 = 8*256.
    assert_eq!(own, (8, 8));
    // The force-move key answers Move over water (`0x00700B03`).
    let ordered = click(&sim, &rules, &[yard], None, (56, 56), OrderModifier::Shift);
    let BuildingClick::Orders(orders, false) = ordered else {
        panic!("{ordered:?}");
    };
    assert_eq!(
        orders,
        [
            Command::SetRally {
                rx: own.0,
                ry: own.1,
                producer_ids: vec![yard],
            },
            Command::SellBuilding { entity_id: yard },
        ]
    );
    let tick = sim.session().tick();
    sim.queue_commands(
        orders
            .into_iter()
            .map(|payload| CommandEnvelope::new(owner, tick, payload)),
    );
    step(&mut sim, &rules);
    assert_eq!(
        sim.entities().get(yard).map(|yard| yard.archive_target()),
        Some(Some(TargetKind::Cell(own.0, own.1)))
    );
    // A war factory's missed search sends no rally but still announces
    // (`0x00443A89` -> `0x00443A2B`).
    let factory = sim
        .spawn_object("GAWEAP", "Americans", 0, 11, 0, &rules)
        .unwrap();
    assert_eq!(
        click(
            &sim,
            &rules,
            &[factory],
            None,
            (56, 56),
            OrderModifier::Shift
        ),
        BuildingClick::Orders(Vec::new(), true)
    );
}

/// Techno What_Action on a Cell's Ctrl+Alt guard rung (`0x00700815..0x007008A3`)
/// answers 0x1A for the armed Slave Miner yard, which Active_Click_With_Cell
/// ignores; a war factory's rally line turns it back into Move (`0x0044774B`).
#[test]
fn ctrl_alt_on_the_ground_does_not_pack_up_an_active_yard() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let (mut sim, _) = yard_arena(&rules);
    let slave_yard = sim
        .spawn_object("YAREFN", "Americans", 4, 20, 0, &rules)
        .unwrap();
    let factory = sim
        .spawn_object("GAWEAP", "Americans", 6, 6, 0, &rules)
        .unwrap();
    let ground =
        |selection: &[u64], modifier| click(&sim, &rules, selection, None, (12, 20), modifier);
    assert!(packs_up(&ground(&[slave_yard], OrderModifier::Normal)));
    assert_eq!(
        ground(&[slave_yard], OrderModifier::GuardArea),
        BuildingClick::Orders(Vec::new(), false)
    );
    assert_eq!(
        rallies(&ground(&[factory], OrderModifier::GuardArea)),
        [factory]
    );
}

/// The selection's click loop clamps the clicked Cell to the playfield
/// (`0x004AE77E`) before any receiver's What_Action: a yard ordered past the
/// playfield's edge packs up toward the clamped Cell, though its cursor there
/// shows NoMove.
#[test]
fn a_yard_click_past_the_playfield_edge_packs_up_toward_the_clamped_cell() {
    use crate::sim::world::{CellActionQuery, MoveAction};
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let (mut sim, owner) = yard_arena(&rules);
    let yard = sim
        .spawn_object("GACNST", "Americans", 12, 12, 0, &rules)
        .unwrap();
    *sim.playfield_bounds_mut_for_test() = Some(crate::map::playfield::PlayfieldBounds {
        base: 32,
        off_fc: 4,
        off_100: 4,
        off_104: 20,
        off_108: 12,
    });
    let cursor = |cell| {
        sim.cell_move_action(
            owner,
            yard,
            cell,
            CellActionQuery {
                mouse_over: true,
                shift: false,
            },
            &rules,
            None,
        )
        .unwrap()
    };
    let edge = (0..24u16)
        .flat_map(|y| (0..24u16).map(move |x| (x, y)))
        .find(|&cell| {
            let clamped = sim.clamped_click_cell(cell);
            clamped != cell && cursor(clamped) == MoveAction::Move
        })
        .expect("a land Cell past the playfield edge");
    assert_eq!(cursor(edge), MoveAction::NoMove);
    let ordered = click(&sim, &rules, &[yard], None, edge, OrderModifier::Normal);
    assert!(packs_up(&ordered), "{edge:?}: {ordered:?}");
}

/// Building What_Action_OnObject's factory rungs under Alt
/// (`0x0044744E..0x0044750B`): a war factory answers NoMove over a unit its
/// zone cannot reach (IsInSameZoneAsCoords `0x00707F60`, across a channel or
/// on water), so the cursor is barred and the click sets no rally. Over a
/// unit on its own ground it rallies. A shipyard's Normal MovementZone
/// cannot stand on its own Water centre: a ship shares that blocked zone, so
/// only the zone type `+0x4C` (Water, not ground) refuses it, and a land
/// unit is in another zone. A service depot, which the player may order but
/// is no factory, keeps Move over all three units.
#[test]
fn an_alt_click_rallies_a_war_factory_only_onto_an_object_it_can_reach() {
    use crate::app::types::CursorFeedbackKind;
    use HoverTargetKind::EnemyUnit;
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x447210);
    crate::sim::arena_fixture::flood(
        &mut sim,
        &rules,
        32,
        (0..32).flat_map(|y| (13..=18).map(move |x| (x, y))),
    );
    sim.resolve_type_handles(&rules);
    let owner = sim.interner().get("Americans").unwrap();
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    let factory = sim
        .spawn_object("GAWEAP", "Americans", 3, 4, 0, &rules)
        .unwrap();
    let depot = sim
        .spawn_object("GADEPT", "Americans", 3, 20, 0, &rules)
        .unwrap();
    let shipyard = sim
        .spawn_object("GAYARD", "Americans", 14, 1, 0, &rules)
        .unwrap();
    let near = sim
        .spawn_object("MTNK", "Russians", 10, 12, 64, &rules)
        .unwrap();
    let across = sim
        .spawn_object("MTNK", "Russians", 24, 8, 64, &rules)
        .unwrap();
    let ship = sim
        .spawn_object("HYD", "Russians", 15, 24, 64, &rules)
        .unwrap();
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let cursor = |building, target| {
        super::super::cursor::capability_cursor_for_hover(
            &sim,
            &[building],
            Some(building),
            &HoverTargetKindWithId {
                kind: EnemyUnit,
                stable_id: target,
            },
            Some(&rules),
            None,
            OrderModifier::ForceMove,
            true,
            false,
        )
    };
    let alt = |building, target| {
        let entity = sim.entities().get(target).unwrap();
        click(
            &sim,
            &rules,
            &[building],
            Some((EnemyUnit, target)),
            (entity.position.rx(), entity.position.ry()),
            OrderModifier::ForceMove,
        )
    };
    for (building, target, reaches) in [
        (factory, near, true),
        (factory, across, false),
        (factory, ship, false),
        (shipyard, ship, false),
        (shipyard, near, false),
    ] {
        assert_eq!(
            cursor(building, target),
            if reaches {
                CursorFeedbackKind::Move
            } else {
                CursorFeedbackKind::Invalid
            },
            "{building} over {target}",
        );
        let rallied = if reaches { vec![building] } else { Vec::new() };
        assert_eq!(
            rallies(&alt(building, target)),
            rallied,
            "{building} over {target}"
        );
    }
    for target in [near, across, ship] {
        assert_eq!(cursor(depot, target), CursorFeedbackKind::Move);
    }
}

/// Alt over an object: a Construction Yard archives the object itself
/// (event 0x1E, `0x0044363C..0x00443688`; `EventClass::Execute
/// 0x004C6DAA`). A later pack-up whose click archives nothing (SELL alone)
/// keeps that archive, and the MCV is sent to the object (Assign_Destination
/// then Move, `0x0044A091..0x0044A0AE`).
#[test]
fn a_yard_alt_click_archives_the_object_and_its_mcv_drives_there() {
    use HoverTargetKind::FriendlyUnit;
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let mut rules = retail.rules;
    // As above: a finite build-up binding for the INI-only fixture.
    rules.set_buildup_control_for_test("GACNST", [0, 29, 1]);
    let (mut sim, owner) = yard_arena(&rules);
    sim.set_input_delay_ticks(0);
    sim.spawn_object("GAPOWR", "Russians", 3, 3, 0, &rules)
        .unwrap();
    let yard = sim
        .spawn_object("GACNST", "Americans", 12, 12, 0, &rules)
        .unwrap();
    let tank = sim
        .spawn_object("MTNK", "Americans", 21, 8, 64, &rules)
        .unwrap();
    let alt = click(
        &sim,
        &rules,
        &[yard],
        Some((FriendlyUnit, tank)),
        (21, 8),
        OrderModifier::ForceMove,
    );
    let BuildingClick::Orders(orders, false) = alt else {
        panic!("{alt:?}");
    };
    assert_eq!(
        orders,
        [Command::ArchiveObject {
            entity_id: yard,
            target_id: tank,
        }]
    );
    let tick = sim.session().tick();
    sim.queue_commands(
        orders
            .into_iter()
            .chain([Command::SellBuilding { entity_id: yard }])
            .map(|payload| CommandEnvelope::new(owner, tick, payload)),
    );
    step(&mut sim, &rules);
    assert_eq!(
        sim.entities().get(yard).map(|yard| yard.archive_target()),
        Some(Some(TargetKind::Entity(tank)))
    );
    let mut mcv = None;
    for _ in 0..80 {
        step(&mut sim, &rules);
        mcv = sim
            .entities()
            .values()
            .find(|e| sim.interner().resolve(e.type_ref()) == "AMCV")
            .map(|e| e.stable_id());
        if mcv.is_some() {
            break;
        }
    }
    let mcv = mcv.expect("the yard must pack up");
    let unit = sim.entities().get(mcv).unwrap();
    assert!(
        [unit.mission.current(), unit.mission.queued()]
            .iter()
            .any(|mission| mission.known() == Some(MissionType::Move))
    );
    let beside_tank = |sim: &Simulation| {
        let unit = sim.entities().get(mcv).unwrap();
        unit.position.rx().abs_diff(21) <= 1 && unit.position.ry().abs_diff(8) <= 1
    };
    for _ in 0..600 {
        if beside_tank(&sim) {
            break;
        }
        step(&mut sim, &rules);
    }
    assert!(beside_tank(&sim), "the MCV must drive to the archived tank");
}

/// `EventClass::Execute 0x004C6DAA` resolves the event's target when it
/// runs: an object gone by then is null, which clears the archive.
#[test]
fn an_object_archive_whose_target_is_gone_clears_the_archive() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let (mut sim, _) = yard_arena(&rules);
    let yard = sim
        .spawn_object("GACNST", "Americans", 12, 12, 0, &rules)
        .unwrap();
    sim.entities_mut_for_test()
        .get_mut(yard)
        .unwrap()
        .set_archive_target(Some(TargetKind::Cell(20, 12)));
    assert!(sim.apply_command(
        "Americans",
        &Command::ArchiveObject {
            entity_id: yard,
            target_id: 0xDEAD,
        },
        Some(&rules)
    ));
    assert_eq!(sim.entities().get(yard).unwrap().archive_target(), None);
    assert!(!sim.apply_command(
        "Russians",
        &Command::ArchiveObject {
            entity_id: yard,
            target_id: yard,
        },
        Some(&rules)
    ));
}

/// Alt or Ctrl+Alt over a Terrain object with a building selected: Techno
/// What_Action's Alt rung answers Move (a tree is no ally for Ctrl+Alt's
/// guard rung, `0x004F9A90`), which Building What_Action_OnObject's factory
/// rungs (`0x004473F4..0x0044750B`) take over the Cell under the tree. The
/// yard archives the tree (event 0x1E) and its MCV later drives to it.
#[test]
fn a_yard_alt_click_over_a_tree_archives_it_and_its_mcv_drives_there() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let mut rules = retail.rules;
    rules.set_buildup_control_for_test("GACNST", [0, 29, 1]);
    let (mut sim, owner) = yard_arena(&rules);
    sim.set_input_delay_ticks(0);
    sim.spawn_object("GAPOWR", "Russians", 3, 3, 0, &rules)
        .unwrap();
    let yard = sim
        .spawn_object("GACNST", "Americans", 12, 12, 0, &rules)
        .unwrap();
    let factory = sim
        .spawn_object("GAWEAP", "Americans", 6, 18, 0, &rules)
        .unwrap();
    assert_eq!(
        crate::sim::terrain_spawn::construct_terrain_objects(
            &mut sim,
            &[crate::map::overlay::TerrainObject {
                rx: 21,
                ry: 8,
                name: "TREE01".into(),
            }],
            &rules,
            false,
        ),
        1
    );
    let tree = sim.terrain_objects().id_at((21, 8)).expect("tree");
    let over_tree = |selection: &[u64], modifier| {
        click_over(&sim, &rules, selection, None, Some(tree), (21, 8), modifier)
    };
    for modifier in [OrderModifier::ForceMove, OrderModifier::GuardArea] {
        let alt = over_tree(&[yard], modifier);
        assert_eq!(archives(&alt), [(yard, tree)], "{modifier:?}");
        assert!(!packs_up(&alt), "{modifier:?}");
        // The war factory's rungs admit the Cell under the tree, and it
        // rallies beside it (`Building443860`).
        assert_eq!(
            rallies(&over_tree(&[factory], modifier)),
            [factory],
            "{modifier:?}"
        );
    }
    let tick = sim.session().tick();
    sim.queue_commands(
        [
            Command::ArchiveObject {
                entity_id: yard,
                target_id: tree,
            },
            Command::SellBuilding { entity_id: yard },
        ]
        .map(|payload| CommandEnvelope::new(owner, tick, payload)),
    );
    step(&mut sim, &rules);
    assert_eq!(
        sim.entities().get(yard).map(|yard| yard.archive_target()),
        Some(Some(TargetKind::Entity(tree)))
    );
    let mut mcv = None;
    for _ in 0..80 {
        step(&mut sim, &rules);
        mcv = sim
            .entities()
            .values()
            .find(|e| sim.interner().resolve(e.type_ref()) == "AMCV")
            .map(|e| e.stable_id());
        if mcv.is_some() {
            break;
        }
    }
    let mcv = mcv.expect("the yard must pack up");
    let beside_tree = |sim: &Simulation| {
        let unit = sim.entities().get(mcv).unwrap();
        unit.position.rx().abs_diff(21) <= 1 && unit.position.ry().abs_diff(8) <= 1
    };
    for _ in 0..600 {
        if beside_tree(&sim) {
            break;
        }
        step(&mut sim, &rules);
    }
    let unit = sim.entities().get(mcv).unwrap();
    assert!(
        beside_tree(&sim),
        "the MCV must drive to the archived tree: {:?}",
        (unit.position.rx(), unit.position.ry())
    );
}
