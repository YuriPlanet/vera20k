//! Ares 3.0 action cursors (`src/Misc/Actions.cpp`, `src/Ext/Techno/
//! Hooks.Cursor.cpp` of the restored source).
//!
//! Ares keeps a table of cursor overrides keyed by action
//! (`Actions::CursorOverrides`, Ares.dll `100C1E58`): entry 0 overrides every
//! action, entry `action + 1` one action, each with a shroud mode. The
//! What_Action hooks write it while the best selected object answers the
//! cursor's action, `MouseClass_Update_Reset @ 100583C0` (hook `0x5BDDC0`)
//! empties it every frame, and `DisplayClass_SetAction_CustomCursor @
//! 10057F10` (hook `0x4AB35A`) replaces `SetCursorFromAction`'s switches with
//! one that reads it before the action's own cursor. VERA asks the What_Action
//! port once per cursor query, so [`begin_query`] empties the table at the
//! start of each one; the table lives on the client thread only.
//!
//! The writers are the hooks that name a cursor:
//! - `TechnoClass_GetActionOnObject_Cursors @ 1004C990` (`0x6FFEC0`) and
//!   `TechnoClass_GetActionOnCell_Cursors @ 1004C920` (`0x700600`): the
//!   object's `Cursor.Move`/`Cursor.NoMove`, the hovered Techno's
//!   `Cursor.Enter` (Enter and Repair) and `Cursor.NoEnter`.
//! - `TechnoClass_GetActionOnObject_AttackCursor @ 1004C960` (`0x70055D`),
//!   `TechnoClass_GetActionOnCell_AttackCursor @ 1004C900` (`0x700AA8`) and
//!   10057F10's own range test: the selected weapon's `Cursor.Attack`, or its
//!   `Cursor.AttackOutOfRange` for a lone selection out of range
//!   (`TechnoExt::GetAttackCursor @ 10044A80`).
//! - `TechnoClass_GetActionOnObject_SelfDeployCursor @ 1004CA20` (`0x7000CD`)
//!   and `UnitClass_GetActionOnObject_SelfDeployCursor_Bunker @ 1004CA70`
//!   (`0x7400F0`): `Cursor.Deploy`/`Cursor.NoDeploy`, a bunkered unit's
//!   bunker's `Cursor.Deploy`.
//! - `InfantryClass_GetActionOnObject_Saboteur @ 10025FC0` (`0x51EE6B`): the
//!   building's `Cursor.Spy` for a spy, Sabotage (0x5D) for a saboteur;
//!   `..._CanCapture @ 10049690` (`0x51E7BF`): TakeVehicle (0x5C);
//!   `..._EngineerOverFriendlyBuilding @ 10025420` (`0x51E635`): RepairTrench
//!   (0x5E) over a rubble; `..._MultiEngineerB @ 10025550` (`0x51E5E1`):
//!   EngineerDamage (0x57).
//! - `SWTypeExt::ExtData::WhatAction @ 10033EB0` (via `0x6CEF84` and the
//!   radar's `0x653B3A`): a handled superweapon's `Cursor`/`NoCursor` as the
//!   override of every action.
//!
//! `RadarClass_GetMouseAction_AllowMinimap @ 100583F0` (`0x653CA6`) lets the
//! radar keep an overridden action only when its cursor has radar frames.
//!
//! The superweapon cursors are superweapon code and follow
//! [`RuleSet::ares_super_weapons`], as the superweapons do; the other hooks
//! follow [`RuleSet::ares`]. Only Rules read the Ares way hold the scenario's
//! `[MouseCursors]` lines and `Cursor.*` keys, so a game Ares plays in whose
//! Rules were read the stock way shows the cursors Ares draws without them.
//! The match cursor draws through the list whenever either switch is on
//! ([`draws_through_list`]).
//!
//! Cursor choice is presentation only: the table and its readers stay in the
//! client, and no simulation state, RNG draw or hash depends on them.

use std::cell::Cell;

use crate::app::input::cursor::ActionQuery;
use crate::app::input::entity_pick::HoverObject;
use crate::app::types::{CursorFeedbackKind, CursorId};
use crate::rules::ares_mouse_cursor::{MouseCursors, index};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;

/// The native `Action` values the table is keyed by.
pub(crate) mod action {
    pub(crate) const NONE: u8 = 0x00;
    pub(crate) const MOVE: u8 = 0x01;
    pub(crate) const NO_MOVE: u8 = 0x02;
    pub(crate) const ENTER: u8 = 0x03;
    pub(crate) const SELF_DEPLOY: u8 = 0x04;
    pub(crate) const ATTACK: u8 = 0x05;
    pub(crate) const HARVEST: u8 = 0x06;
    pub(crate) const SELECT: u8 = 0x07;
    pub(crate) const CAPTURE: u8 = 0x09;
    /// YRpp `Repair`: entering a Grinder or a repair depot.
    pub(crate) const REPAIR: u8 = 0x0B;
    pub(crate) const SABOTAGE: u8 = 0x10;
    pub(crate) const TOTE: u8 = 0x11;
    pub(crate) const GUARD_AREA: u8 = 0x1A;
    pub(crate) const DAMAGE: u8 = 0x1C;
    pub(crate) const GREPAIR: u8 = 0x1D;
    pub(crate) const NO_DEPLOY: u8 = 0x1E;
    pub(crate) const NO_ENTER: u8 = 0x1F;
    pub(crate) const NO_GREPAIR: u8 = 0x20;
    pub(crate) const AREA_ATTACK: u8 = 0x34;
    pub(crate) const IVAN_BOMB: u8 = 0x35;
    pub(crate) const NO_IVAN_BOMB: u8 = 0x36;
    pub(crate) const DETONATE: u8 = 0x37;
    pub(crate) const DISARM_BOMB: u8 = 0x39;
    pub(crate) const ATTACK_MOVE_NAV: u8 = 0x3E;
    pub(crate) const DEMOLISH: u8 = 0x40;
    pub(crate) const AIRSTRIKE: u8 = 0x47;
}

const TABLE_LEN: usize = 0x4A;

/// Whether the match cursor draws every cursor through the `[MouseCursors]`
/// list (`MouseClass_UpdateCursor @ 10007AC0`, hook `0x5BDC8C`;
/// `render::cursor_atlas::build_ares_software_cursor`): whenever a hook here
/// may name one of its entries, so while Ares plays or its superweapons are
/// on.
pub(crate) fn draws_through_list(rules: &RuleSet) -> bool {
    rules.ares() || rules.ares_super_weapons()
}

thread_local! {
    /// The cursor of each action's entry, 0 for none. The What_Action hooks
    /// write shroud mode 0 only; the superweapon's global entry 0 (modes 1
    /// and 2) is applied by its callers ([`super_weapon_cursor`]).
    static OVERRIDES: Cell<[i32; TABLE_LEN]> = const { Cell::new([0; TABLE_LEN]) };
    /// VERA's own mark: the What_Action port answered Capture (9) where its
    /// feedback says Enter ([`CursorFeedbackKind::Enter`] covers both).
    static ANSWERED_CAPTURE: Cell<bool> = const { Cell::new(false) };
}

/// `MouseClass_Update_Reset @ 100583C0` (hook `0x5BDDC0`): empty the table
/// before a What_Action query writes it.
pub(crate) fn begin_query() {
    OVERRIDES.with(|table| table.set([0; TABLE_LEN]));
    ANSWERED_CAPTURE.with(|capture| capture.set(false));
}

/// `Actions::SetCursor @ 10057E20`: override `action`'s cursor (entry
/// `action + 1`), shroud mode 0.
pub(crate) fn set_cursor(cursor: i32, action: u8) {
    OVERRIDES.with(|table| {
        let mut entries = table.get();
        if let Some(entry) = entries.get_mut(usize::from(action) + 1) {
            *entry = cursor;
        }
        table.set(entries);
    });
}

/// Mark that the answer being built is Capture (9), not Enter (3).
pub(crate) fn note_capture() {
    ANSWERED_CAPTURE.with(|capture| capture.set(true));
}

/// `GetCursorOverride` (inlined twice in 10057F10) without the global
/// entry: the action's cursor, unless it holds none.
fn override_for(action: u8) -> Option<i32> {
    OVERRIDES
        .with(|table| table.get().get(usize::from(action) + 1).copied())
        .filter(|&cursor| cursor != 0)
}

/// The native action of a What_Action answer, as 10057F10 receives it.
/// With more than one object selected an out-of-range Attack reaches its
/// switch as action 6, the vanilla fall-through into Harvest's cursor.
pub(crate) fn native_action(kind: CursorFeedbackKind, selected_count: usize) -> Option<u8> {
    use CursorFeedbackKind as Kind;
    use action::*;
    Some(match kind {
        Kind::Default => NONE,
        Kind::Select | Kind::FriendlyUnit | Kind::FriendlyStructure => SELECT,
        Kind::Move => MOVE,
        Kind::AttackMove => ATTACK_MOVE_NAV,
        Kind::GuardArea => GUARD_AREA,
        Kind::EnemyUnit | Kind::EnemyStructure => ATTACK,
        Kind::EnemyOutOfRange if selected_count == 1 => ATTACK,
        Kind::EnemyOutOfRange | Kind::Harvest => HARVEST,
        Kind::Invalid => NO_MOVE,
        Kind::NoIvanBomb => NO_IVAN_BOMB,
        Kind::Enter if ANSWERED_CAPTURE.with(Cell::get) => CAPTURE,
        Kind::Enter => ENTER,
        Kind::Grind => REPAIR,
        Kind::NoEnter => NO_ENTER,
        Kind::Tote => TOTE,
        Kind::EngineerDamage => DAMAGE,
        Kind::RepairAction(true) => GREPAIR,
        Kind::RepairAction(false) => NO_GREPAIR,
        Kind::Demolish => SABOTAGE,
        Kind::WeaponDemolish => DEMOLISH,
        Kind::AirStrike => AIRSTRIKE,
        Kind::IvanBomb => IVAN_BOMB,
        Kind::DisarmBomb => DISARM_BOMB,
        Kind::Detonate => DETONATE,
        Kind::Deploy => SELF_DEPLOY,
        Kind::NoDeploy => NO_DEPLOY,
        // DetermineAction's own modes and the cursor's, never a What_Action
        // answer.
        _ => return None,
    })
}

/// `DisplayClass_SetAction_CustomCursor @ 10057F10`, past its Attack range
/// test (made where the table is written): the cursor of `action` over a
/// shrouded cell or not. Over shroud a mode-0 override, like none, lets the
/// shroud switch remap the action first; the override of the resulting
/// action wins over its table cursor.
pub(crate) fn cursor_from_action(action: u8, shrouded: bool, lead_moves_to_shroud: bool) -> i32 {
    let action = if shrouded {
        shrouded_action(action, lead_moves_to_shroud)
    } else {
        action
    };
    override_for(action).unwrap_or_else(|| action_cursor(action))
}

/// 10057F10's shroud switch (`10058230`/`1005825C`): NoMove moves when the
/// first selected object's type has MoveToShroud.
fn shrouded_action(action: u8, lead_moves_to_shroud: bool) -> u8 {
    match action {
        0x02 if lead_moves_to_shroud => 0x01,
        0x03
        | 0x04
        | 0x06..=0x09
        | 0x0B
        | 0x10
        | 0x12
        | 0x13
        | 0x15..=0x19
        | 0x1C
        | 0x1D
        | 0x23
        | 0x30
        | 0x34..=0x38
        | 0x3A
        | 0x3B
        | 0x40
        | 0x47 => 0x00,
        0x05 => 0x01,
        0x0A | 0x20 => 0x0F,
        0x0C | 0x0D => 0x0E,
        0x21 => 0x22,
        0x24 => 0x1F,
        0x28 => 0x27,
        0x3D => 0x07,
        0x41 => 0x29,
        other => other,
    }
}

/// 10057F10's action table (`100582A4`/`10058340`), the vanilla switch
/// (`0x004AB884`) with Ares' own cursors for Tote, TogglePower and
/// NoTogglePower, Detonate's row for Detonate/DetonateAll and none for
/// Damage.
fn action_cursor(action: u8) -> i32 {
    match action {
        0x01 => index::MOVE,
        0x02 | 0x36 => index::NO_MOVE,
        0x03 | 0x09 | 0x0B | 0x23 => index::ENTER,
        0x04 | 0x34 => index::DEPLOY,
        0x05 => index::ATTACK,
        0x06 => index::ATTACK_OUT_OF_RANGE,
        0x07 | 0x08 | 0x3D => 0x11,
        0x0A => 0x22,
        0x0C => 0x1E,
        0x0D => 0x1F,
        0x0E => 0x20,
        0x0F | 0x20 => 0x23,
        0x10 | 0x40 => 0x34,
        0x11 => index::TOTE,
        0x14 => index::NUKE,
        0x1A => 0x16,
        0x1B | 0x2A..=0x2F | 0x31..=0x33 => index::DISALLOWED,
        0x1D => index::REPAIR,
        0x1E => index::NO_DEPLOY,
        0x1F => index::NO_ENTER,
        0x21 => index::TOGGLE_POWER,
        0x22 => index::NO_TOGGLE_POWER,
        0x25 => index::IRON_CURTAIN,
        0x26 => index::LIGHTNING_STORM,
        0x27 | 0x28 => index::CHRONOSPHERE,
        0x29 | 0x41 => index::PARA_DROP,
        0x35 => index::IVAN_BOMB,
        0x37 | 0x38 => index::DETONATE,
        0x39 => 0x3B,
        0x3C => 0x4E,
        0x3E | 0x3F => 0x47,
        0x42 => index::PSYCHIC_DOMINATOR,
        0x43 => index::SPY_PLANE,
        0x44 => index::GENETIC_MUTATOR,
        0x45 => index::FORCE_SHIELD,
        0x46 => index::NO_FORCE_SHIELD,
        0x47 => 0x52,
        0x48 => index::PSYCHIC_REVEAL,
        _ => index::DEFAULT,
    }
}

/// The tactical cursor of a What_Action answer: the Ares cursor 10057F10
/// picks when it draws other frames than the row the stock id draws, else
/// `kind`. A `none` cursor (-1) faults in Ares; VERA keeps the stock cursor.
/// `kind` without Ares.
pub(crate) fn tactical(
    kind: CursorFeedbackKind,
    selected_count: usize,
    rules: Option<&RuleSet>,
) -> CursorFeedbackKind {
    let (Some(action), Some(rules)) = (
        native_action(kind, selected_count),
        rules.filter(|rules| rules.ares()),
    ) else {
        return kind;
    };
    let cursors = rules.ares_mouse_cursors();
    let cursor = cursor_from_action(action, false, false);
    let (Ok(index), Some(drawn_set)) = (u16::try_from(cursor), drawn(cursors, cursor, false))
    else {
        return kind;
    };
    let stock = crate::app::input::cursor::cursor_id_for_feedback(kind)
        .and_then(crate::render::cursor_atlas::vanilla_row)
        .and_then(|(row, mini)| drawn(cursors, i32::from(row), mini));
    if stock == Some(drawn_set) {
        kind
    } else {
        CursorFeedbackKind::AresAction(index)
    }
}

/// The frames, interval and hotspot an entry draws for its main or radar
/// set (`0x5BDC8C`); `None` outside the list.
fn drawn(cursors: &MouseCursors, cursor: i32, mini: bool) -> Option<[i32; 5]> {
    let entry = cursors.get(cursor)?;
    let (frame, count) = if mini && entry.mini_frame >= 0 {
        (entry.mini_frame, entry.mini_count)
    } else {
        (entry.frame, entry.count)
    };
    Some([frame, count, entry.interval, entry.hot_x, entry.hot_y])
}

/// `RadarClass_GetMouseAction_AllowMinimap @ 100583F0` (hook `0x653CA6`) for
/// a What_Action answer: with an override the radar keeps the action only
/// when its cursor has radar frames (`MiniFrame >= 0`) and draws the cursor
/// 10057F10 picks; without one `vanilla`, the stock keep table (`0x653CAB`),
/// decides. A kept action that draws the stock frames stays `vanilla`.
/// `vanilla` without Ares.
pub(crate) fn radar_action(
    query: &ActionQuery<'_>,
    kind: CursorFeedbackKind,
    vanilla: Option<crate::app::input::radar::RadarAction>,
) -> Option<crate::app::input::radar::RadarAction> {
    if !query.rules.is_some_and(|rules| rules.ares()) {
        return vanilla;
    }
    let Some(action) = native_action(kind, query.selected.len()) else {
        return vanilla;
    };
    let Some(cursor) = override_for(action) else {
        return vanilla;
    };
    let cursors = query.rules?.ares_mouse_cursors();
    if cursors.get(cursor)?.mini_frame < 0 {
        return None;
    }
    let lead = query.lead_moves_to_shroud();
    let (main, shrouded) = (
        cursor_from_action(action, false, lead),
        cursor_from_action(action, true, lead),
    );
    let stock = |shrouded: bool| {
        let (row, mini) =
            crate::render::cursor_atlas::vanilla_row(vanilla?.cursor(shrouded, lead))?;
        drawn(cursors, i32::from(row), mini)
    };
    if vanilla.is_some()
        && stock(false) == drawn(cursors, main, true)
        && stock(true) == drawn(cursors, shrouded, true)
    {
        return vanilla;
    }
    Some(crate::app::input::radar::RadarAction::Ares { main, shrouded })
}

/// A superweapon cursor: `SWTypeExt::ExtData::WhatAction @ 10033EB0` for a
/// type with Ares cursors ([`crate::rules::ruleset::RuleSet::
/// ares_super_weapon_cursors`]): `Cursor` over a cell it may fire at (`allowed`: its
/// SW.FireIntoShroud test and its handler's CanFireAt, the shared
/// `super_weapon_target_check`), `NoCursor` elsewhere. Ares sets it as the
/// override of every action (`Actions::Set @ 10057E40`, action -1), so it is
/// the cursor whatever the shroud mode does to the action. `None` for the
/// other types, and without the Ares superweapons.
pub(crate) fn super_weapon_cursor(rules: &RuleSet, section: &str, allowed: bool) -> Option<i32> {
    if !rules.ares_super_weapons() {
        return None;
    }
    let cursors = rules.ares_super_weapon_cursors(section)?;
    Some(if allowed {
        cursors.cursor
    } else {
        cursors.no_cursor
    })
}

/// The radar's armed Ares superweapon (`RadarClass_GetMouseAction_
/// CustomSWAction @ 10034D00`, hook `0x653B3A`, then `0x653CA6`): kept only
/// when its cursor has radar frames; a disallowed cell keeps no launch. An
/// allowed launch stays the stock index map's answer (`vanilla`) when that
/// draws the same frames and its action is the armed type's `Action=`, so
/// SPECIAL_PLACE launches the armed type from it too (hook `0x4AC20C`,
/// `commands::special_place_type`). `None` for a type without Ares cursors,
/// and without the Ares superweapons.
pub(crate) fn radar_super_weapon(
    query: &ActionQuery<'_>,
    armed: usize,
    cell: (u16, u16),
    vanilla: Option<crate::app::input::radar::RadarAction>,
) -> Option<Option<crate::app::input::radar::RadarAction>> {
    let rules = query.rules.filter(|rules| rules.ares_super_weapons())?;
    let section = rules.super_weapon_order.get(armed)?;
    rules.ares_super_weapon_cursors(section)?;
    let allowed = query
        .viewer
        .zip(query.sim.interner().get(section))
        .is_some_and(|(viewer, sw_type)| {
            query
                .sim
                .super_weapon_target_check(rules, viewer, sw_type, cell, false)
                .is_ok()
        });
    let cursor = super_weapon_cursor(rules, section, allowed)?;
    let cursors = rules.ares_mouse_cursors();
    if cursors.get(cursor).is_none_or(|entry| entry.mini_frame < 0) {
        return Some(None);
    }
    let stock = vanilla.and_then(|vanilla| {
        let (row, mini) = crate::render::cursor_atlas::vanilla_row(vanilla.cursor(false, false))?;
        drawn(cursors, i32::from(row), mini)
    });
    let launches_armed = matches!(
        vanilla,
        Some(crate::app::input::radar::RadarAction::SuperWeapon(stock))
            if rules
                .super_weapon(section)
                .is_some_and(|sw| sw.has_action(stock.action_name()))
    );
    if allowed && launches_armed && stock.is_some() && stock == drawn(cursors, cursor, true) {
        return Some(vanilla);
    }
    Some(Some(
        crate::app::input::radar::RadarAction::AresSuperWeapon {
            cursor,
            launch: allowed,
        },
    ))
}

/// The hooks the best object's What_Action reaches before its own rungs:
/// the Techno entry's Move/NoMove and Enter/Repair/NoEnter (`0x6FFEC0`,
/// `0x700600`) and, over itself as the lone selection, the self-deploy
/// cursors (`0x7000CD`, `0x7400F0`).
pub(crate) fn what_action_entry(
    query: &ActionQuery<'_>,
    best_id: Option<u64>,
    hover_object: Option<HoverObject>,
) {
    begin_query();
    let (Some(actor_id), Some(rules)) = (best_id, query.rules) else {
        return;
    };
    let sim = query.sim;
    let cursors_of = |id: u64| {
        sim.entities()
            .get(id)
            .and_then(|entity| sim.interner().object_type(entity.type_ref(), rules))
            .map(|object| *object.ares_cursors())
    };
    let Some(actor) = cursors_of(actor_id) else {
        return;
    };
    set_cursor(actor.move_, action::MOVE);
    set_cursor(actor.no_move, action::NO_MOVE);
    let Some(target_id) = hover_object
        .and_then(HoverObject::techno)
        .map(|hover| hover.stable_id)
    else {
        return;
    };
    if let Some(target) = cursors_of(target_id) {
        set_cursor(target.enter, action::REPAIR);
        set_cursor(target.enter, action::ENTER);
        set_cursor(target.no_enter, action::NO_ENTER);
    }
    if target_id == actor_id && query.selected.len() == 1 {
        set_cursor(actor.deploy, action::AREA_ATTACK);
        set_cursor(actor.deploy, action::SELF_DEPLOY);
        set_cursor(actor.no_deploy, action::NO_DEPLOY);
        let bunker = sim
            .entities()
            .get(actor_id)
            .filter(|entity| entity.category() == crate::map::entities::EntityCategory::Unit)
            .and_then(|entity| entity.bunker.link().installed_in());
        if let Some(bunker) = bunker.and_then(cursors_of) {
            set_cursor(bunker.deploy, action::SELF_DEPLOY);
        }
    }
}

/// The Attack cursors of the best object's answer: its weapon's
/// `Cursor.Attack` where Techno's What_Action returns Attack (`0x70055D`,
/// `0x700AA8`), then 10057F10's own test, `Cursor.AttackOutOfRange` for a
/// lone selection out of range.
pub(crate) fn what_action_attack(
    query: &ActionQuery<'_>,
    best_id: Option<u64>,
    target: TargetKind,
    kind: CursorFeedbackKind,
) {
    let out_of_range = match kind {
        CursorFeedbackKind::EnemyUnit | CursorFeedbackKind::EnemyStructure => false,
        CursorFeedbackKind::EnemyOutOfRange => true,
        _ => return,
    };
    let (Some(actor_id), Some(rules)) = (best_id, query.rules) else {
        return;
    };
    set_cursor(
        attack_cursor(query, rules, actor_id, target, false),
        action::ATTACK,
    );
    if out_of_range && query.selected.len() == 1 {
        set_cursor(
            attack_cursor(query, rules, actor_id, target, true),
            action::ATTACK,
        );
    }
}

/// `TechnoExt::GetAttackCursor @ 10044A80`: the selected weapon's
/// `Cursor.Attack` or `Cursor.AttackOutOfRange`, or the vanilla Attack and
/// AttackOutOfRange cursors without a weapon.
fn attack_cursor(
    query: &ActionQuery<'_>,
    rules: &RuleSet,
    actor_id: u64,
    target: TargetKind,
    out_of_range: bool,
) -> i32 {
    use crate::sim::combat::combat_weapon::{
        attacker_facts, get_weapon, select_weapon_against_in_query,
    };
    let sim = query.sim;
    let weapon = sim.entities().get(actor_id).and_then(|actor| {
        let object = sim.interner().object_type(actor.type_ref(), rules)?;
        let (weapon, _) = select_weapon_against_in_query(
            rules,
            actor,
            object,
            &attacker_facts(actor, object),
            Some(&target),
            sim,
            sim.interner(),
            None,
            Some(sim.house_alliances()),
        )?;
        get_weapon(actor, object, weapon, sim.entities(), rules, sim.interner())
    });
    match (weapon, out_of_range) {
        (Some(weapon), false) => weapon.ares_cursors.attack,
        (Some(weapon), true) => weapon.ares_cursors.attack_out_of_range,
        (None, false) => index::ATTACK,
        (None, true) => index::ATTACK_OUT_OF_RANGE,
    }
}

/// `InfantryClass_GetActionOnObject_Saboteur @ 10025FC0` (`0x51EE6B`): the
/// infiltration's Capture with the building's `Cursor.Spy` for a spy and
/// Sabotage (0x5D) for a saboteur, while Ares plays.
pub(crate) fn infiltration(
    sim: &crate::sim::world::Simulation,
    rules: &RuleSet,
    infantry_id: u64,
    building_id: u64,
) {
    use crate::sim::world::InfiltrateAction;
    if !rules.ares() {
        return;
    }
    match sim.infiltrate_action(infantry_id, building_id, rules) {
        InfiltrateAction::Spy => {
            if let Some(building) = sim
                .entities()
                .get(building_id)
                .and_then(|entity| sim.interner().object_type(entity.type_ref(), rules))
            {
                set_cursor(building.ares_cursors().spy, action::CAPTURE);
            }
        }
        InfiltrateAction::Sabotage => set_cursor(index::SABOTAGE, action::CAPTURE),
        _ => {}
    }
    note_capture();
}

/// The cursor a [`CursorFeedbackKind::AresAction`] or superweapon override
/// draws: its list entry's main set. A negative or out-of-list cursor draws
/// the arrow, where Ares faults.
pub(crate) fn cursor_id(cursor: i32, mini: bool) -> CursorId {
    u16::try_from(cursor).map_or(CursorId::Default, |index| CursorId::Ares { index, mini })
}

#[cfg(test)]
#[path = "ares_cursor_tests.rs"]
mod tests;
