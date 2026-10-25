//! `RadarClass::Action @ 0x006539D0` through the production What_Action port,
//! on retail rules.

use super::*;
use crate::app::input::context_order::OrderModifier;
use crate::app::types::OrderMode;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

fn scene() -> Option<(Simulation, RuleSet)> {
    let battle = crate::rules::retail_ini_fixture::retail_battle_rules()?;
    let (sim, rules, _) = Simulation::walk_cell_input_test_scene_with(battle.rules, false);
    Some((sim, rules))
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, type_id: &str, (rx, ry): (u16, u16)) -> u64 {
    let id = sim
        .spawn_object(type_id, "Local", rx, ry, 0, rules)
        .expect(type_id);
    sim.resolve_type_handles(rules);
    id
}

/// The radar asks without mouse-over (`0x00653C9A`) and with no key held.
fn query<'a>(sim: &'a Simulation, rules: &'a RuleSet, selected: &'a [u64]) -> ActionQuery<'a> {
    ActionQuery {
        sim,
        rules: Some(rules),
        overlays: None,
        viewer: sim.interner().get("Local"),
        selected,
        modifier: OrderModifier::Normal,
        force_move_key: false,
        order_mode: OrderMode::Move,
        planning: false,
        mouse_over: false,
        full_visibility: false,
    }
}

fn planning<'a>(sim: &'a Simulation, rules: &'a RuleSet, selected: &'a [u64]) -> ActionQuery<'a> {
    ActionQuery {
        planning: true,
        ..query(sim, rules, selected)
    }
}

/// A tracked object's pick: its Cell and the Techno What_Action sees.
fn dot(sim: &Simulation, id: u64) -> ((u16, u16), Option<HoverObject>) {
    let entity = sim.entities().get(id).expect("tracked object");
    (
        (entity.position.rx(), entity.position.ry()),
        crate::app::input::entity_pick::hover_object_by_id(sim, id, "Local"),
    )
}

/// Open ground inside the scene's playfield, beside the tanks.
const GROUND: (u16, u16) = (11, 5);
/// A river Cell, which a tank cannot enter.
const RIVER: (u16, u16) = (9, 5);

/// With a selection, a radar click is the selection's order at the radar
/// Cell: the best object's What_Action there (`0x00653C88..0x00653CA1`) is
/// Move, which the keep table keeps, so the press leaves the view where it is
/// and the release hands Move to BandBox_LeftUp (`0x00653D58`).
#[test]
fn a_selected_tank_moves_to_the_radar_cell() {
    let Some((mut sim, rules)) = scene() else {
        return;
    };
    let tank = spawn(&mut sim, &rules, "MTNK", (12, 5));
    assert_eq!(
        action(&query(&sim, &rules, &[tank]), None, GROUND, None),
        Some(RadarAction::Move)
    );
    assert_eq!(
        RadarAction::Move.cursor(false, false),
        CursorId::MinimapMove
    );
}

/// The radar asks the Cell's What_Action without mouse-over
/// (`0x00653C9A`), so Can_Enter_Cell answers it (`0x00700B51..0x00700B7E`):
/// over the river the tank's radar action is NoMove, which the keep table
/// keeps and the cursor draws as the barred mini, where the tactical cursor
/// (mouse-over 1) shows Move.
#[test]
fn the_radar_asks_can_enter_cell() {
    let Some((mut sim, rules)) = scene() else {
        return;
    };
    let tank = spawn(&mut sim, &rules, "MTNK", (12, 5));
    let selected = [tank];
    assert_eq!(
        action(&query(&sim, &rules, &selected), None, RIVER, None),
        Some(RadarAction::NoMove)
    );
    assert_eq!(
        RadarAction::NoMove.cursor(false, false),
        CursorId::MinimapNoMove
    );
    let tactical = ActionQuery {
        mouse_over: true,
        ..query(&sim, &rules, &selected)
    };
    assert_eq!(
        tactical.selection_action(RIVER, None),
        Some(CursorFeedbackKind::Move)
    );
}

/// With nothing selected and nothing armed there is no action
/// (`0x00653B91..0x00653BD3` exits to `0x00653D62`): the cursor is the plain
/// arrow mini and the press recentres.
#[test]
fn without_a_selection_the_radar_only_recentres() {
    let Some((mut sim, rules)) = scene() else {
        return;
    };
    let tank = spawn(&mut sim, &rules, "MTNK", (12, 5));
    assert_eq!(action(&query(&sim, &rules, &[]), None, GROUND, None), None);
    let (cell, object) = dot(&sim, tank);
    assert_eq!(action(&query(&sim, &rules, &[]), None, cell, object), None);
}

/// Over a tracked object the radar asks the object's What_Action
/// (`0x00653C72..0x00653C83`). The lone selected tank over its own dot
/// answers NoMove — its Self arm leaves 0, which the Unit tail turns into
/// NoMove for an object its player orders (`0x007403EB..0x00740414`) — and
/// the keep table keeps it, so that press does not recentre. Another own
/// tank's dot answers Select, which it drops: that press recentres.
#[test]
fn own_dots_split_on_the_keep_table() {
    let Some((mut sim, rules)) = scene() else {
        return;
    };
    let tank = spawn(&mut sim, &rules, "MTNK", (12, 5));
    let other = spawn(&mut sim, &rules, "MTNK", (4, 6));
    let selected = [tank];
    let (cell, object) = dot(&sim, tank);
    assert_eq!(
        action(&query(&sim, &rules, &selected), None, cell, object),
        Some(RadarAction::NoMove)
    );
    let (cell, object) = dot(&sim, other);
    assert_eq!(
        action(&query(&sim, &rules, &selected), None, cell, object),
        None
    );
}

/// A Crazy Ivan's forced Attack over a bombed object is NoIvanBomb
/// (`0x0051EB24..0x0051EB75`; `ivan_already_bombed` in
/// `tools/spatial_oracle/allied_object_action.json`). It shares NoMove's
/// cursor row, not its keep: the radar drops it, where it keeps the
/// unforced answer over the same dot, NoMove.
#[test]
fn a_forced_ivan_over_a_bombed_dot_has_no_radar_action() {
    let Some((mut sim, rules)) = scene() else {
        return;
    };
    let ivan = spawn(&mut sim, &rules, "IVAN", (12, 5));
    let tank = sim
        .spawn_object("HTNK", "Enemy", 13, 7, 0, &rules)
        .expect("HTNK");
    sim.resolve_type_handles(&rules);
    sim.bomb_attach_for_test(ivan, Some(tank), &rules);
    sim.bomb_list_update_for_test(&rules);
    sim.bomb_list_update_for_test(&rules);
    let selected = [ivan];
    let forced = ActionQuery {
        modifier: OrderModifier::ForceFire,
        ..query(&sim, &rules, &selected)
    };
    let (cell, object) = dot(&sim, tank);
    assert_eq!(
        forced.selection_action(cell, object),
        Some(CursorFeedbackKind::NoIvanBomb)
    );
    assert_eq!(action(&forced, None, cell, object), None);
    assert_eq!(
        action(&query(&sim, &rules, &selected), None, cell, object),
        Some(RadarAction::NoMove)
    );
}

/// The keep table reads the action, not its cursor row: NoMove (2) stays
/// and NoIvanBomb (0x36) goes although both draw row 19, and the selection,
/// deployment, repair and other tactical-only actions go too. Over a Cell
/// the player has not mapped the shrouded switch (`0x004AB35A`) sends
/// Attack (5) to the Move row and Harvest, Enter and Sabotage to row 0,
/// and GuardArea (0x1A) keeps row 22 in both switches (`0x004AB4D9`,
/// `0x004AB3A1`).
#[test]
fn the_keep_table_reads_the_action_not_its_row() {
    assert_eq!(
        RadarAction::kept(CursorFeedbackKind::Invalid),
        Some(RadarAction::NoMove)
    );
    for kind in [
        CursorFeedbackKind::NoIvanBomb,
        CursorFeedbackKind::Select,
        CursorFeedbackKind::Deploy,
        CursorFeedbackKind::RepairAction(true),
        CursorFeedbackKind::IvanBomb,
        CursorFeedbackKind::Grind,
        CursorFeedbackKind::NoEnter,
        CursorFeedbackKind::WeaponDemolish,
    ] {
        assert_eq!(RadarAction::kept(kind), None, "{kind:?}");
    }
    for kind in [
        CursorFeedbackKind::EnemyUnit,
        CursorFeedbackKind::EnemyOutOfRange,
    ] {
        let attack = RadarAction::kept(kind).unwrap();
        assert_eq!(attack.cursor(false, false), CursorId::MinimapAttack);
        assert_eq!(attack.cursor(true, false), CursorId::MinimapMove);
    }
    for shrouded in [false, true] {
        assert_eq!(
            RadarAction::GuardArea.cursor(shrouded, false),
            CursorId::MinimapGuardArea
        );
        // `0x0070F0B0` turns Move and Attack into 0x3E/0x3F ahead of either
        // switch, and both draw row 71, whose mini set is the attack one.
        assert_eq!(
            RadarAction::AttackMove.cursor(shrouded, false),
            CursorId::MinimapAttack
        );
    }
    for action in [
        RadarAction::Harvest,
        RadarAction::Enter,
        RadarAction::Sabotage,
    ] {
        assert_eq!(action.cursor(true, false), CursorId::DefaultMini);
    }
    // The shrouded NoMove takes row 18 when the first selected type has
    // MoveToShroud (`0x004AB3EC..0x004AB42B`).
    assert_eq!(
        RadarAction::NoMove.cursor(true, true),
        CursorId::MinimapMove
    );
    assert_eq!(
        RadarAction::NoMove.cursor(true, false),
        CursorId::MinimapNoMove
    );
}

/// The armed superweapon's index answers before the selection
/// (`0x00653B3A..0x00653C63`): Nuke (index 0) with nothing selected; Iron
/// Curtain (index 1) maps to an action the keep table drops, even with a
/// selection; the Chronosphere's index 3 is not listed, so the selection
/// answers, and with none there is no action.
#[test]
fn the_armed_superweapon_index_answers_first() {
    let Some((mut sim, rules)) = scene() else {
        return;
    };
    let tank = spawn(&mut sim, &rules, "MTNK", (12, 5));
    let none = query(&sim, &rules, &[]);
    let selected = [tank];
    let some = query(&sim, &rules, &selected);
    assert_eq!(
        action(&none, Some(0), GROUND, None),
        Some(RadarAction::SuperWeapon(RadarSuperWeapon::Nuke))
    );
    assert_eq!(
        RadarAction::SuperWeapon(RadarSuperWeapon::Nuke).cursor(false, false),
        CursorId::NukeMini
    );
    assert_eq!(action(&some, Some(1), GROUND, None), None);
    assert_eq!(action(&none, Some(3), GROUND, None), None);
    assert_eq!(
        action(&some, Some(3), GROUND, None),
        Some(RadarAction::Move)
    );
}

/// In Planning Mode (`0x00637AA0`) the selection walk 0x00639DA0 gates the
/// action (`0x00653CC9..0x00653CD1`), a superweapon's included: a tank may
/// take waypoints, so its radar Move stands; an aircraft in the selection
/// may not, and the radar has no action.
#[test]
fn planning_mode_needs_a_selection_that_takes_waypoints() {
    let Some((mut sim, rules)) = scene() else {
        return;
    };
    let tank = spawn(&mut sim, &rules, "MTNK", (12, 5));
    let harrier = spawn(&mut sim, &rules, "ORCA", (3, 3));
    assert_eq!(
        action(&planning(&sim, &rules, &[tank]), None, GROUND, None),
        Some(RadarAction::Move)
    );
    let mixed = [tank, harrier];
    assert_eq!(
        action(&planning(&sim, &rules, &mixed), None, GROUND, None),
        None
    );
    assert_eq!(
        action(&planning(&sim, &rules, &mixed), Some(0), GROUND, None),
        None
    );
}

/// The radar maps the armed index, not the type's `Action=`, and
/// SPECIAL_PLACE names the first type whose `Action=` is that action
/// (`SuperWeaponTypeClass::FindFirstOfAction @ 0x006CEEB0`, against
/// `CCINIClass::ReadAction @ 0x00474EE0`'s case-insensitive name). In retail
/// rules each kept index's own type is that first type, so the radar
/// launches the armed type.
#[test]
fn retail_kept_indices_are_the_first_type_of_their_action() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &battle.rules;
    let mut kept = 0;
    for index in 0..rules.super_weapon_order.len() {
        let Armed::Kept(super_weapon) = armed_action(index) else {
            continue;
        };
        let first = rules.super_weapon_order.iter().position(|section| {
            rules
                .super_weapon(section)
                .and_then(|super_weapon| super_weapon.action.as_deref())
                .is_some_and(|name| name.eq_ignore_ascii_case(super_weapon.action_name()))
        });
        assert_eq!(first, Some(index), "{super_weapon:?}");
        kept += 1;
    }
    assert_eq!(kept, 7);
}
