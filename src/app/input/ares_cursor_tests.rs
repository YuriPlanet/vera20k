//! Ares 3.0 action cursors through the production What_Action port, on
//! retail rules read the Ares way, with authored `[MouseCursors]` entries and
//! cursor keys.

use super::*;
use crate::app::input::context_order::OrderModifier;
use crate::app::types::OrderMode;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

const PATCH: &str = "[MouseCursors]\n\
                     Spin=100,8,2\n\
                     Still=110,1,0,63,1\n\
                     [MTNK]\n\
                     Cursor.Move=Spin\n\
                     [105mm]\n\
                     Cursor.AttackOutOfRange=Still\n";

fn scene(patch: &str) -> Option<(Simulation, RuleSet)> {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let battle = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "Hills.mmx",
        &IniFile::from_str(patch),
    )?;
    let (sim, rules, _) = Simulation::walk_cell_input_test_scene_with(battle.rules, false);
    Some((sim, rules))
}

fn tank(sim: &mut Simulation, rules: &RuleSet) -> u64 {
    let id = sim
        .spawn_object("MTNK", "Local", 12, 5, 0, rules)
        .expect("MTNK");
    sim.resolve_type_handles(rules);
    id
}

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
        mouse_over: true,
        full_visibility: true,
    }
}

/// Open ground beside the tank, and ground beyond the river, out of range.
const GROUND: (u16, u16) = (11, 5);
const FAR: (u16, u16) = (1, 5);

/// 10057F10's table: an override wins over the action's cursor, and over
/// shroud the remapped action's override shows (Attack moves; NoMove moves
/// for a MoveToShroud lead). Ares' own rows answer Tote, Detonate and Damage.
#[test]
fn the_override_table_follows_the_shroud_switch() {
    begin_query();
    set_cursor(0x60, action::MOVE);
    set_cursor(0x61, action::ATTACK);
    assert_eq!(cursor_from_action(action::ATTACK, false, false), 0x61);
    assert_eq!(cursor_from_action(action::ATTACK, true, false), 0x60);
    assert_eq!(cursor_from_action(action::NO_MOVE, true, true), 0x60);
    assert_eq!(
        cursor_from_action(action::NO_MOVE, true, false),
        index::NO_MOVE
    );
    assert_eq!(cursor_from_action(0x11, false, false), index::TOTE);
    assert_eq!(cursor_from_action(0x37, false, false), index::DETONATE);
    assert_eq!(
        cursor_from_action(action::DAMAGE, false, false),
        index::DEFAULT
    );
    begin_query();
    assert_eq!(cursor_from_action(action::ATTACK, true, false), index::MOVE);
}

/// The tank's `Cursor.Move` (`0x6FFEC0`/`0x700600`) shows over ground, and
/// its weapon's `Cursor.AttackOutOfRange` (10057F10's range test) for a forced
/// shot it cannot reach; choosing them leaves the simulation alone.
#[test]
fn cursor_keys_name_the_tactical_cursors() {
    let Some((mut sim, rules)) = scene(PATCH) else {
        return;
    };
    let tank = tank(&mut sim, &rules);
    let before = sim.state_hash();
    let cursors = rules.ares_mouse_cursors();
    let [spin, still] = ["Spin", "Still"].map(|name| cursors.find(name).unwrap() as u16);
    let selected = [tank];
    let moving = query(&sim, &rules, &selected).selection_action(GROUND, None);
    assert_eq!(moving, Some(CursorFeedbackKind::Move));
    assert_eq!(
        tactical(moving.unwrap(), 1, Some(&rules)),
        CursorFeedbackKind::AresAction(spin)
    );
    let forced = ActionQuery {
        modifier: OrderModifier::ForceFire,
        ..query(&sim, &rules, &selected)
    };
    let firing = forced.selection_action(FAR, None);
    assert_eq!(firing, Some(CursorFeedbackKind::EnemyOutOfRange));
    assert_eq!(
        tactical(firing.unwrap(), 1, Some(&rules)),
        CursorFeedbackKind::AresAction(still)
    );
    assert_eq!(sim.state_hash(), before);
}

/// `0x653CA6`: the radar keeps an overridden action only when its cursor has
/// radar frames, so the tank's Move without them leaves no radar action,
/// while its out-of-range cursor with them draws its radar set.
#[test]
fn the_radar_keeps_an_override_only_with_radar_frames() {
    let Some((mut sim, rules)) = scene(PATCH) else {
        return;
    };
    let tank = tank(&mut sim, &rules);
    let selected = [tank];
    let radar = ActionQuery {
        mouse_over: false,
        ..query(&sim, &rules, &selected)
    };
    assert_eq!(
        crate::app::input::radar::action(&radar, None, GROUND, None),
        None
    );
    let forced = ActionQuery {
        modifier: OrderModifier::ForceFire,
        ..radar
    };
    let still = rules.ares_mouse_cursors().find("Still").unwrap();
    let action = crate::app::input::radar::action(&forced, None, FAR, None).unwrap();
    assert_eq!(
        action.cursor(false, false),
        CursorId::Ares {
            index: still as u16,
            mini: true
        }
    );
}

/// `SWTypeExt::ExtData::WhatAction @ 10033EB0`: the Spy Plane, whose Ares
/// handler names the SpyPlane cursor (10075800), shows it over a cell it may
/// fire at and Disallowed elsewhere; a stock type without a ported handler
/// keeps the stock path.
#[test]
fn a_handled_super_weapon_shows_its_cursor_or_no_cursor() {
    let Some((_, rules)) = scene("") else {
        return;
    };
    assert_eq!(
        super_weapon_cursor(&rules, "SpyPlaneSpecial", true),
        Some(index::SPY_PLANE)
    );
    assert_eq!(
        super_weapon_cursor(&rules, "SpyPlaneSpecial", false),
        Some(index::DISALLOWED)
    );
    assert_eq!(
        super_weapon_cursor(&rules, "ForceShieldSpecial", true),
        None
    );
}
