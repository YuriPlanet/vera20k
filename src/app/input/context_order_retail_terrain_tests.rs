//! Plain and Ctrl clicks over a Terrain object on retail rules. The point
//! picker (`0x006DA380`) reaches a Terrain object only through the clicked
//! cell's ground list (`0x006DA4B8..0x006DA4FB`) and hands it to Techno
//! What_Action_OnObject (`0x006FFEC0`). Without Ctrl a tree is no legal
//! target (`0x00700400..0x0070043E`), so the answer is 0, which a player's
//! Unit (`0x0073FD50`) or Infantryman (`0x0051F05E`) turns into NoMove and a
//! Building keeps (no rally). With Ctrl an armed object attacks it; over an
//! `Immune=` TerrainType the Unit (`0x00740486`) and Infantry
//! (`0x0051F160..0x0051F179`) tails answer NoMove. Foot `0x004D74E0` sends
//! NoMove as a move near the object.

use super::retail_entry_tests::{arena, retail};
use super::*;
use crate::app::input::entity_pick::HoverObject;
use crate::app::types::CursorFeedbackKind;
use crate::map::overlay::TerrainObject;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

const TERRAIN_CELL: (u16, u16) = (16, 17);

/// The player's Grizzly and GI, and a `kind` Terrain object a few cells away.
fn scene(rules: &RuleSet, kind: &str) -> (Simulation, u64, u64, u64) {
    let mut sim = arena(rules);
    // The Russians keep a building, so the skirmish has no winner.
    sim.spawn_object("NAPOWR", "Russians", 3, 3, 0, rules)
        .expect("russian power plant");
    let tank = sim
        .spawn_object("MTNK", "Americans", 13, 16, 0, rules)
        .expect("tank");
    let gi = sim
        .spawn_object("E1", "Americans", 13, 19, 0, rules)
        .expect("gi");
    assert_eq!(
        crate::sim::terrain_spawn::construct_terrain_objects(
            &mut sim,
            &[TerrainObject {
                rx: TERRAIN_CELL.0,
                ry: TERRAIN_CELL.1,
                name: kind.into(),
            }],
            rules,
            false,
        ),
        1
    );
    let terrain = sim.terrain_objects().id_at(TERRAIN_CELL).expect(kind);
    (sim, tank, gi, terrain)
}

/// The object the picker hands DetermineAction at the centre of `cell`.
fn picked(sim: &Simulation, cell: (u16, u16)) -> Option<HoverObject> {
    let half = crate::util::fixed_math::SimFixed::from_num(128);
    let (x, y) = crate::util::lepton::lepton_to_screen(cell.0, cell.1, half, half, 0);
    crate::app::input::entity_pick::hover_object_at_point(sim, x, y, "Americans", true)
}

fn cursor(
    sim: &Simulation,
    rules: &RuleSet,
    registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    actor: u64,
    terrain: u64,
    ctrl: bool,
) -> Option<CursorFeedbackKind> {
    crate::app::input::cursor::terrain_object_feedback(
        sim,
        actor,
        terrain,
        ctrl,
        1,
        Some(rules),
        Some(registry),
    )
}

/// Each of `movers` is sent a move near the object, nothing else.
fn assert_moves_near(commands: &[Command], movers: &[u64]) {
    assert_eq!(commands.len(), movers.len(), "{commands:?}");
    for (command, &mover) in commands.iter().zip(movers) {
        assert!(
            matches!(
                command,
                Command::Move { entity_id, .. } | Command::MoveToObject { entity_id, .. }
                    if *entity_id == mover
            ),
            "{command:?}"
        );
    }
}

#[test]
fn a_tree_under_ctrl_is_attacked() {
    let Some((retail, registry)) = retail() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, tank, gi, tree) = scene(rules, "TREE01");
    assert!(
        !rules
            .terrain_object_type_case_insensitive("TREE01")
            .unwrap()
            .immune
    );
    assert_eq!(picked(&sim, TERRAIN_CELL), Some(HoverObject::Terrain(tree)));
    for actor in [tank, gi] {
        assert_eq!(
            cursor(&sim, rules, &registry, actor, tree, true),
            Some(CursorFeedbackKind::EnemyUnit)
        );
    }
    let dispatch = attack_branch_orders(
        &sim,
        rules,
        &[tank, gi],
        TargetKind::Entity(tree),
        true,
        &registry,
    );
    assert_eq!(
        dispatch.commands,
        [tank, gi].map(|attacker_id| Command::Attack {
            attacker_id,
            target_id: tree,
        })
    );
    assert!(sim.apply_command("Americans", &dispatch.commands[0], Some(rules)));
    assert_eq!(
        sim.entities().get(tank).unwrap().attack_target.target(),
        Some(TargetKind::Entity(tree))
    );
}

#[test]
fn an_immune_terrain_object_under_ctrl_is_no_move() {
    let Some((retail, registry)) = retail() else {
        return;
    };
    let rules = &retail.rules;
    let (sim, tank, gi, tree) = scene(rules, "TIBTRE01");
    assert!(
        rules
            .terrain_object_type_case_insensitive("TIBTRE01")
            .unwrap()
            .immune
    );
    assert_eq!(picked(&sim, TERRAIN_CELL), Some(HoverObject::Terrain(tree)));
    for actor in [tank, gi] {
        assert_eq!(
            cursor(&sim, rules, &registry, actor, tree, true),
            Some(CursorFeedbackKind::Invalid)
        );
    }
    let dispatch = attack_branch_orders(
        &sim,
        rules,
        &[tank, gi],
        TargetKind::Entity(tree),
        true,
        &registry,
    );
    assert!(dispatch.consumed);
    assert_moves_near(&dispatch.commands, &[tank, gi]);
}

#[test]
fn a_plain_click_over_a_tree_moves_near_it_and_sets_no_rally() {
    let Some((retail, registry)) = retail() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, tank, gi, tree) = scene(rules, "TREE01");
    let factory = sim
        .spawn_object("GAWEAP", "Americans", 20, 4, 0, rules)
        .expect("war factory");
    for actor in [tank, gi] {
        assert_eq!(
            cursor(&sim, rules, &registry, actor, tree, false),
            Some(CursorFeedbackKind::Invalid)
        );
    }
    assert_eq!(
        cursor(&sim, rules, &registry, factory, tree, false),
        Some(CursorFeedbackKind::Default)
    );
    let dispatch = attack_branch_orders(
        &sim,
        rules,
        &[tank, gi, factory],
        TargetKind::Entity(tree),
        false,
        &registry,
    );
    assert!(dispatch.consumed);
    assert!(dispatch.receivers.contains(&factory));
    assert_moves_near(&dispatch.commands, &[tank, gi]);
}

#[test]
fn an_unarmed_engineer_under_ctrl_moves_near_a_tree() {
    let Some((retail, registry)) = retail() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, _, _, tree) = scene(rules, "TREE01");
    let engineer = sim
        .spawn_object("ENGINEER", "Americans", 13, 21, 0, rules)
        .expect("engineer");
    assert_eq!(
        cursor(&sim, rules, &registry, engineer, tree, true),
        Some(CursorFeedbackKind::Invalid)
    );
    let dispatch = attack_branch_orders(
        &sim,
        rules,
        &[engineer],
        TargetKind::Entity(tree),
        true,
        &registry,
    );
    assert_moves_near(&dispatch.commands, &[engineer]);
}

/// Techno What_Action's key rungs over a tree: Shift answers ToggleSelect
/// (`0x00700107`), which selects nothing; Alt and Ctrl+Alt answer Move
/// (`0x007001E9..0x00700217`; a tree is no ally for the guard rung,
/// `0x007001A7`), which Foot `0x004D74E0` sends as a move near the tree.
/// Plain, Ctrl and Ctrl+Shift keep the object's own answer. Over a Techno
/// ally Ctrl+Alt's guard rung answers first (`0x007001DD`). A berserk
/// receiver answers before the key reads (`0x006FFEC9`).
#[test]
fn shift_alt_and_ctrl_alt_over_a_tree_take_the_key_rungs() {
    let Some((retail, _)) = retail() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, tank, gi, tree) = scene(rules, "TREE01");
    for actor in [tank, gi] {
        for (modifier, alt, action) in [
            (
                OrderModifier::Shift,
                false,
                Some(ObjectKeyAction::ToggleSelect),
            ),
            (OrderModifier::ForceMove, true, Some(ObjectKeyAction::Move)),
            (OrderModifier::GuardArea, true, Some(ObjectKeyAction::Move)),
            (OrderModifier::AttackMove, true, Some(ObjectKeyAction::Move)),
            (OrderModifier::AttackMove, false, None),
            (OrderModifier::Normal, false, None),
            (OrderModifier::ForceFire, false, None),
        ] {
            assert_eq!(
                object_key_action(&sim, rules, actor, None, modifier, alt),
                action,
                "{modifier:?} alt {alt}"
            );
        }
        let owner = sim.entities().get(actor).unwrap().owner();
        assert!(
            crate::app::input::commands::object_move_destination(
                &sim,
                rules,
                owner,
                actor,
                tree,
                crate::sim::world::MoveAction::Move,
                true,
            )
            .is_some(),
            "a move near the tree"
        );
    }
    // Over the player's own GI, a Techno ally, Ctrl+Alt's guard rung admits
    // the tank first; Alt alone still moves.
    for (modifier, action) in [
        (OrderModifier::Shift, ObjectKeyAction::ToggleSelect),
        (OrderModifier::GuardArea, ObjectKeyAction::GuardArea),
        (OrderModifier::ForceMove, ObjectKeyAction::Move),
    ] {
        assert_eq!(
            object_key_action(&sim, rules, tank, Some(gi), modifier, true),
            Some(action),
            "{modifier:?} over the GI"
        );
    }
    sim.entities_mut_for_test()
        .get_mut(tank)
        .unwrap()
        .berserk
        .set_active_for_test(true);
    assert_eq!(
        object_key_action(&sim, rules, tank, None, OrderModifier::Shift, false),
        None
    );
}

/// A player's Crazy Ivan with Ctrl over a tree: every retail TerrainType is
/// `Bombable=` (ObjectType ReadINI `0x005F9417..0x005F9431`, constructor 1)
/// and a Terrain object carries no bomb, so the Ivan arm answers IvanBomb
/// (`0x0051EB48..0x0051EB66`) and the click attacks the tree. His shot plants
/// nothing: BombListClass::Attach gets a null target for a non-Techno
/// (`0x0046934D..0x00469361`), and the IvanBomb arm deals no damage. The
/// weapon is `FireOnce=yes`, so he then drops the target.
#[test]
fn a_crazy_ivan_under_ctrl_bombs_a_tree_without_planting_a_bomb() {
    let Some((retail, registry)) = retail() else {
        return;
    };
    let rules = &retail.rules;
    assert!(
        rules
            .terrain_object_type_case_insensitive("TREE01")
            .unwrap()
            .bombable
    );
    let (mut sim, _, _, tree) = scene(rules, "TREE01");
    // The player keeps a building too, so the house is not defeated.
    sim.spawn_object("GAPOWR", "Americans", 3, 24, 0, rules)
        .expect("american power plant");
    let ivan = sim
        .spawn_object("IVAN", "Americans", 14, 17, 0, rules)
        .expect("ivan");
    assert_eq!(
        cursor(&sim, rules, &registry, ivan, tree, true),
        Some(CursorFeedbackKind::IvanBomb)
    );
    assert_eq!(
        cursor(&sim, rules, &registry, ivan, tree, false),
        Some(CursorFeedbackKind::Invalid)
    );
    // Unforced the answer is NoMove, so the Ivan arm is skipped
    // (`0x0051EA6F`) and he moves near the tree like any infantryman.
    let plain = attack_branch_orders(
        &sim,
        rules,
        &[ivan],
        TargetKind::Entity(tree),
        false,
        &registry,
    );
    assert_moves_near(&plain.commands, &[ivan]);
    let dispatch = attack_branch_orders(
        &sim,
        rules,
        &[ivan],
        TargetKind::Entity(tree),
        true,
        &registry,
    );
    assert_eq!(
        dispatch.commands,
        [Command::Attack {
            attacker_id: ivan,
            target_id: tree,
        }]
    );
    let before = sim.terrain_objects().get(tree).cloned();
    assert!(sim.apply_command("Americans", &dispatch.commands[0], Some(rules)));
    for _ in 0..150 {
        sim.advance_tick(&[], Some(rules), None, None, 67);
    }
    // IvanBomber is `FireOnce=yes`: one shot, then the target goes.
    let ivan = sim.entities().get(ivan).unwrap();
    assert!(ivan.last_fire_frame.frame() >= 0, "the Ivan fired");
    assert_eq!(ivan.attack_target.target(), None);
    assert_eq!(sim.terrain_objects().get(tree).cloned(), before);
    assert!(sim.entities().values().all(|entity| entity.bomb.is_none()));
}
