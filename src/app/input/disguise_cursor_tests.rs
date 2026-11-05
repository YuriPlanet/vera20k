use super::*;
use crate::app::input::cursor::{capability_cursor_for_hover, cursor_id_for_feedback};
use crate::app::input::entity_pick::{PickSource, PresentedWorld, hover_target_at_point};
use crate::app::presentation::interpolation::RenderInterpolation;
use crate::app::types::{CursorId, HoverTargetKind};
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::{FrameEffects, Simulation};

fn scene(patch: &str) -> Option<(RuleSet, OverlayTypeRegistry, Simulation, u64, u64)> {
    let battle = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "XMP03T4.MAP",
        &IniFile::from_str(&format!("[General]\nAttackCursorOnDisguise=no\n{patch}")),
    )?;
    let overlays = OverlayTypeRegistry::from_ini(&battle.processed_rules, Some(&battle.fixed_art));
    let rules = battle.rules;
    assert!(!rules.general.attack_cursor_on_disguise);
    let mut sim = super::retail_entry_tests::arena(&rules);
    sim.spawn_object("GAPOWR", "Americans", 3, 3, 0, &rules)
        .unwrap();
    sim.spawn_object("NAPOWR", "Russians", 40, 40, 0, &rules)
        .unwrap();
    let mirage = sim
        .spawn_object("MGTK", "Russians", 16, 16, 0, &rules)
        .unwrap();
    sim.advance_tick(&[], Some(&rules), None, None, 67);
    assert!(
        sim.entities()
            .get(mirage)
            .unwrap()
            .disguise
            .as_ref()
            .unwrap()
            .is_disguised()
    );
    let actor = sim
        .spawn_object("MTNK", "Americans", 14, 16, 0, &rules)
        .unwrap();
    for name in ["Americans", "Russians"] {
        let owner = sim.interner().get(name).unwrap();
        sim.fog_mut_for_test().reveal_all_for_owner(owner);
    }
    sim.entities_mut_for_test()
        .get_mut(actor)
        .unwrap()
        .selection
        .set_selected_for_test(true);
    Some((rules, overlays, sim, actor, mirage))
}

fn disguised_to_actor(sim: &Simulation, actor: u64, target: u64) -> bool {
    sim.entities().get(target).unwrap().is_disguised_to(
        sim.entities().get(actor).unwrap().owner(),
        Some(sim.fog()),
        Some(sim.house_alliances()),
        sim.interner(),
    )
}

fn action(
    sim: &Simulation,
    rules: &RuleSet,
    overlays: &OverlayTypeRegistry,
    actor: u64,
    target: u64,
    forced: bool,
) -> AttackAction {
    attack_action(
        sim,
        rules,
        actor,
        &TargetKind::Entity(target),
        forced,
        Some(overlays),
    )
}

fn cursor(
    sim: &Simulation,
    rules: &RuleSet,
    overlays: &OverlayTypeRegistry,
    actor: u64,
    target: u64,
) -> Option<CursorId> {
    let interpolation = RenderInterpolation::new(false);
    let source = PresentedWorld {
        sim,
        rules: Some(rules),
        interpolation: &interpolation,
        translucent_viewer: None,
    };
    let (x, y) = source.screen_position(sim.entities().get(target).unwrap());
    let hover = hover_target_at_point(source, x, y, "Americans", false).unwrap();
    assert_eq!(hover.stable_id, target);
    assert_eq!(hover.kind, HoverTargetKind::EnemyUnit);
    cursor_id_for_feedback(capability_cursor_for_hover(
        sim,
        &[actor],
        Some(actor),
        &hover,
        Some(rules),
        Some(overlays),
        OrderModifier::Normal,
        false,
        false,
    ))
}

#[test]
fn attack_cursor_disguise_rule_and_firing_blink_reach_click_admission() {
    let Some((rules, overlays, mut sim, actor, mirage)) = scene("") else {
        return;
    };
    assert!(disguised_to_actor(&sim, actor, mirage));
    assert_eq!(
        cursor(&sim, &rules, &overlays, actor, mirage),
        Some(CursorId::Move)
    );
    assert_ne!(
        action(&sim, &rules, &overlays, actor, mirage, false),
        AttackAction::Attack
    );
    let owner = sim.entities().get(actor).unwrap().owner();
    let click = |sim: &Simulation, modifier| {
        object_attack_click_command(
            sim,
            &rules,
            &overlays,
            owner,
            OrderMode::Move,
            modifier,
            false,
            actor,
            mirage,
        )
    };
    assert!(matches!(
        click(&sim, OrderModifier::Normal),
        Some(Command::Move { entity_id, .. } | Command::MoveToObject { entity_id, .. })
            if entity_id == actor
    ));
    assert!(matches!(
        click(&sim, OrderModifier::ForceFire),
        Some(Command::Attack { .. })
    ));
    sim.commit_fire_visit(
        crate::sim::combat::world_fire::FireVisit::Direct {
            id: mirage,
            target: TargetKind::Entity(actor),
            weapon_index: 0,
        },
        &rules,
        Some(&overlays),
        FrameEffects::default(),
    );
    let timer = sim
        .entities()
        .get(mirage)
        .unwrap()
        .disguise
        .as_ref()
        .unwrap()
        .blink_timer();
    assert!(
        timer.duration() > 0,
        "the real MGTK shot armed its firing blink"
    );
    let expires = timer.start_frame().wrapping_add(timer.duration()) as u32;
    sim.session_mut_for_test()
        .set_binary_frame_for_test(expires.wrapping_sub(1));
    assert_eq!(
        action(&sim, &rules, &overlays, actor, mirage, false),
        AttackAction::Attack
    );
    assert_eq!(
        cursor(&sim, &rules, &overlays, actor, mirage),
        Some(CursorId::Attack)
    );
    assert!(matches!(
        click(&sim, OrderModifier::Normal),
        Some(Command::Attack { .. })
    ));
    sim.session_mut_for_test()
        .set_binary_frame_for_test(expires);
    assert_ne!(
        action(&sim, &rules, &overlays, actor, mirage, false),
        AttackAction::Attack
    );
    assert!(matches!(
        click(&sim, OrderModifier::ForceFire),
        Some(Command::Attack { .. })
    ));
}

#[test]
fn attack_cursor_disguise_keeps_permanent_and_terrain_weapon_exceptions() {
    let Some((rules, overlays, mut sim, actor, mirage)) = scene("[MGTK]\nPermaDisguise=yes\n")
    else {
        return;
    };
    let house = sim.entities().get(actor).unwrap().owner();
    assert!(rules.object("MGTK").unwrap().perma_disguise);
    let kind = sim.entities().get(actor).unwrap().type_ref();
    let frame = sim.session().binary_frame();
    sim.entities_mut_for_test()
        .get_mut(mirage)
        .unwrap()
        .disguise
        .as_mut()
        .unwrap()
        .acquire(frame, Some(kind), Some(house));
    assert!(disguised_to_actor(&sim, actor, mirage));
    assert_eq!(
        action(&sim, &rules, &overlays, actor, mirage, false),
        AttackAction::Attack
    );

    let Some((rules, overlays, mut sim, actor, mirage)) = scene("[105mm]\nTerrainFire=yes\n")
    else {
        return;
    };
    assert!(disguised_to_actor(&sim, actor, mirage));
    assert!(sim.fires_at_terrain(actor, &TargetKind::Entity(mirage), &rules, Some(&overlays)));
    assert_eq!(
        action(&sim, &rules, &overlays, actor, mirage, false),
        AttackAction::Attack
    );
    assert_eq!(
        crate::sim::terrain_spawn::construct_terrain_objects(
            &mut sim,
            &[crate::map::overlay::TerrainObject {
                rx: 18,
                ry: 16,
                name: "TREE01".into(),
            }],
            &rules,
            false,
        ),
        1
    );
    let tree = sim.terrain_objects().id_at((18, 16)).unwrap();
    let terrain_feedback = crate::app::input::cursor::terrain_object_feedback(
        &sim,
        actor,
        tree,
        false,
        1,
        Some(&rules),
        Some(&overlays),
    );
    let terrain_orders = attack_branch_orders(
        &sim,
        &rules,
        &[actor],
        TargetKind::Entity(tree),
        false,
        OrderMode::Move,
        &overlays,
    );
    let terrain_admitted = terrain_feedback
        == Some(crate::app::types::CursorFeedbackKind::EnemyUnit)
        && terrain_orders.commands
            == vec![Command::Attack {
                attacker_id: actor,
                target_id: tree,
            }];

    let Some((rules, overlays, sim, actor, mirage)) =
        scene("[MTNK]\nAttackCursorOnFriendlies=yes\n")
    else {
        return;
    };
    assert!(disguised_to_actor(&sim, actor, mirage));
    let owner = sim.entities().get(actor).unwrap().owner();
    let friendly_orders = object_attack_click_command(
        &sim,
        &rules,
        &overlays,
        owner,
        OrderMode::Move,
        OrderModifier::Normal,
        false,
        actor,
        mirage,
    );
    let friendly_override = cursor(&sim, &rules, &overlays, actor, mirage)
        == Some(CursorId::Attack)
        && friendly_orders
            == Some(Command::Attack {
                attacker_id: actor,
                target_id: mirage,
            });
    assert_eq!((terrain_admitted, friendly_override), (true, true));
}
