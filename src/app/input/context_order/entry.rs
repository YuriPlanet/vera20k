//! Garrison, transport entry and capture actions: which infantry enter or capture
//! the hovered object and the orders they receive.

use super::{
    AttackAction, LeftRelease, ObjectKeyAction, OrderModifier, attack_action_with, cursor_owner,
    left_release, object_key_action_with,
};
use crate::app::input::entity_pick::HoverTargetKindWithId;
use crate::app::types::HoverTargetKind;
use crate::map::entities::EntityCategory;
use crate::sim::combat::TargetKind;
use crate::sim::command::Command;
use crate::sim::world::ControlProjection;

/// Infantry's garrison arm (`0x0051E668..0x0051E6B1`): `actor_id` may
/// occupy the `CanBeOccupied=` building `building_id`
/// ([`crate::sim::passenger::can_enter_transport`]). It ignores Foot's
/// answer, save Shift's ToggleSelect, which returns first (`0x0051E453`),
/// and only Ctrl without Shift refuses it (`0x0051E3B0..0x0051E42A`): Alt
/// and Ctrl+Shift+Alt garrison, Ctrl and Ctrl+Alt do not. Cursor and click
/// ask it alike.
pub(in crate::app::input) fn infantry_garrisons(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    building_id: u64,
    modifier: OrderModifier,
    force_move_key: bool,
) -> bool {
    infantry_garrisons_with(
        sim,
        &sim.control_projection(),
        rules,
        actor_id,
        building_id,
        modifier,
        force_move_key,
    )
}

pub(super) fn infantry_garrisons_with(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    building_id: u64,
    modifier: OrderModifier,
    force_move_key: bool,
) -> bool {
    !matches!(
        modifier,
        OrderModifier::ForceFire | OrderModifier::GuardArea
    ) && sim
        .entities()
        .get(building_id)
        .filter(|target| target.category() == EntityCategory::Structure)
        .and_then(|target| sim.interner().object_type(target.type_ref(), rules))
        .is_some_and(|object| object.can_be_occupied)
        && object_key_action_with(
            sim,
            control,
            rules,
            actor_id,
            building_id,
            Some(building_id),
            modifier,
            force_move_key,
        ) != Some(ObjectKeyAction::ToggleSelect)
        && crate::sim::passenger::can_enter_transport(sim, rules, actor_id, building_id)
}

/// The garrison orders of the `selected` objects [`infantry_garrisons`]
/// admits: Infantry What_Action answers action 9, and Foot ClickedAction
/// (`0x004D7716..0x004D773A`) sends Capture8 with NULL Target and the
/// building as Destination. Actual arrival belongs to the existing
/// Capture/PerCell owners, as for computer occupiers (#1400). Passenger
/// carriers take [`entry_orders`].
pub(super) fn garrison_entry_orders(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &[u64],
    building_id: u64,
    modifier: OrderModifier,
    force_move_key: bool,
) -> Vec<Command> {
    let control = sim.control_projection();
    selected
        .iter()
        .copied()
        .filter(|&id| {
            infantry_garrisons_with(
                sim,
                &control,
                rules,
                id,
                building_id,
                modifier,
                force_move_key,
            )
        })
        .map(|engineer_id| Command::CaptureBuilding {
            engineer_id,
            target_building_id: building_id,
        })
        .collect()
}

/// [`Simulation::entry_click_action`] under the base action the key rungs
/// ([`object_key_action`]) and [`attack_action`] give:
/// - Shift's ToggleSelect (8) returns first (`0x0051E453`), and Ctrl+Alt's
///   GuardArea (0x1A) skips every entry row;
/// - Alt answers Move (1) first (`0x007001E9`), in the Ctrl+Shift+Alt chord
///   too, which only the passenger rows take, as they take the Move or
///   NoMove Foot's shroud rung makes of Shift's or Alt's answer in a
///   campaign;
/// - otherwise [`attack_action`], forced under Ctrl: Select (7) takes every
///   row, and Attack (5) none. Every other answer takes only the passenger
///   rows: an armed player's object over another house's object (0,
///   `0x0070056C`) or a selected one. Ctrl leaves the attack rung to the
///   tail for an actor neither armed nor `Infiltrate=`
///   (`0x00700444..0x00700474`), so an Engineer's Ctrl over a carrier still
///   asks it.
///
/// A player's deployed `Deployer=` infantryman (Doing 27..30) answers NoMove
/// (2) for a Move before the rows (`0x0051EB81..0x0051EBD7`).
///
/// RESIDUAL: Ctrl+Alt skips the rows also where its guard rung refuses (an
/// Aircraft, `0x007001B4..0x007001BA`) and Alt's Move answers, or where the
/// campaign shroud rung rewrites it; natively the rows that take Alt's Move
/// take those. Cursor and click skip them alike. Trigger: Ctrl+Alt by an
/// Aircraft over a friendly object it may enter, or over a shrouded carrier
/// in a campaign; rare.
///
/// [`Simulation::entry_click_action`]: crate::sim::world::Simulation::entry_click_action
#[cfg(test)]
pub(super) fn entry_action(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target_id: u64,
    modifier: OrderModifier,
    force_move_key: bool,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> Option<crate::sim::world::EntryClickAction> {
    entry_action_with(
        sim,
        &sim.control_projection(),
        rules,
        actor_id,
        target_id,
        modifier,
        force_move_key,
        overlay_registry,
    )
}

pub(in crate::app::input) fn hovered_entry_action(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    hover: &HoverTargetKindWithId,
    modifier: OrderModifier,
    force_move_key: bool,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> Option<crate::sim::world::EntryClickAction> {
    use crate::sim::world::EntryClickAction;

    let grind_only = match hover.kind {
        HoverTargetKind::FriendlyStructure | HoverTargetKind::FriendlyUnit => false,
        // TacticalPick classifies for the represented House. The delegated
        // actor may still enter its own Grinder (Infantry 0x0051F0AF,
        // Unit 0x00740122), even when that House is hostile to the viewer.
        HoverTargetKind::EnemyStructure if sim.session().has_custom_control_grants() => {
            let actor = sim.entities().get(actor_id)?;
            let target = sim.entities().get(hover.stable_id)?;
            if actor.owner() != target.owner() {
                return None;
            }
            true
        }
        _ => return None,
    };
    let action = entry_action_with(
        sim,
        control,
        rules,
        actor_id,
        hover.stable_id,
        modifier,
        force_move_key,
        overlay_registry,
    )?;
    (!grind_only || action == EntryClickAction::Grind).then_some(action)
}

fn entry_action_with(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target_id: u64,
    modifier: OrderModifier,
    force_move_key: bool,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> Option<crate::sim::world::EntryClickAction> {
    use crate::sim::world::EntryBaseAction;
    if modifier == OrderModifier::GuardArea {
        return None;
    }
    let key = object_key_action_with(
        sim,
        control,
        rules,
        actor_id,
        target_id,
        Some(target_id),
        modifier,
        force_move_key,
    );
    let (base, moves) = match key {
        Some(ObjectKeyAction::ToggleSelect | ObjectKeyAction::GuardArea) => return None,
        Some(ObjectKeyAction::Move) => (EntryBaseAction::Other, true),
        Some(ObjectKeyAction::NoMove) => (EntryBaseAction::Other, false),
        None => match attack_action_with(
            sim,
            control,
            rules,
            actor_id,
            &TargetKind::Entity(target_id),
            modifier == OrderModifier::ForceFire,
            overlay_registry,
        ) {
            AttackAction::Select => (EntryBaseAction::Select, false),
            AttackAction::Attack => return None,
            action => (EntryBaseAction::Other, action == AttackAction::Move),
        },
    };
    if moves
        && sim.entities().get(actor_id).is_some_and(|actor| {
            actor.category() == EntityCategory::Infantry
                && control.can_order(actor.owner())
                && actor.infantry_deploy_doing()
                && sim
                    .interner()
                    .object_type(actor.type_ref(), rules)
                    .is_some_and(|object| object.deployer)
        })
    {
        return None;
    }
    sim.entry_click_action_with(control, actor_id, target_id, base, rules)
}

/// The selected objects whose action over a friendly object is Enter, Grind
/// or NoEnter ([`entry_action`]) leave `selected`, each on its own
/// What_Action as `Selection__DispatchMultiUnitOrder`'s object loop
/// (`0x004AE829..0x004AE85A`) asks it. Foot ClickedAction (`0x004D74E0`)
/// sends a controllable (vt+0xA0) one mission Enter (case 3) or Eaten (case
/// 0xB) with NULL Target and the object as Destination; NoEnter has no case
/// and orders nothing. Without Alt the loop runs only when the action of the
/// object that owns the cursor ([`cursor_owner`] of `current_objects`) is an
/// order: one of these, or its [`attack_action`] when the [`left_release`]
/// of that dispatches. For its Select, or a 0 over an object the player may
/// select, BandBox_LeftUp selects the object instead (`0x004AB9B0` ->
/// `0x004ABE18`) and never reaches the dispatcher. A selected carrier that
/// owns the cursor over itself answers NoMove (its rung skips itself at
/// `0x00740312`, and `0x007403EB` turns the 0 into NoMove), so the passengers
/// selected with it still enter. Alt's Move is an order, so the loop always
/// runs, and the objects it leaves take the force-move. None when no
/// selected object takes an entry action.
pub(super) fn entry_orders(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    current_objects: &[u64],
    selected: &mut Vec<u64>,
    hover: &HoverTargetKindWithId,
    modifier: OrderModifier,
    force_move_key: bool,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> Option<Vec<Command>> {
    if !matches!(
        hover.kind,
        HoverTargetKind::FriendlyStructure | HoverTargetKind::FriendlyUnit
    ) && !(hover.kind == HoverTargetKind::EnemyStructure
        && sim.session().has_custom_control_grants())
    {
        return None;
    }
    let control = sim.control_projection();
    let action = |id| {
        hovered_entry_action(
            sim,
            &control,
            rules,
            id,
            hover,
            modifier,
            force_move_key,
            overlay_registry,
        )
    };
    let force_move = modifier == OrderModifier::ForceMove
        || (modifier == OrderModifier::AttackMove && force_move_key);
    if !force_move {
        let owner = cursor_owner(sim, rules, current_objects, hover.stable_id)?;
        if action(owner).is_none()
            && left_release(
                sim,
                rules,
                attack_action_with(
                    sim,
                    &control,
                    rules,
                    owner,
                    &TargetKind::Entity(hover.stable_id),
                    modifier == OrderModifier::ForceFire,
                    overlay_registry,
                )
                .into(),
                Some(hover.stable_id),
            ) != LeftRelease::Dispatch
        {
            return None;
        }
    }
    let mut consumed = false;
    let mut orders = Vec::new();
    selected.retain(|&id| {
        let Some(action) = action(id) else {
            return true;
        };
        consumed = true;
        let mission = match action {
            crate::sim::world::EntryClickAction::Enter => crate::sim::mission::MissionType::Enter,
            crate::sim::world::EntryClickAction::Grind => crate::sim::mission::MissionType::Eaten,
            crate::sim::world::EntryClickAction::NoEnter
            | crate::sim::world::EntryClickAction::Nothing => return false,
            // Aircraft ClickedAction (`0x00417BD0`) case 0x11:
            // Player_Assign_Mission(Move, NULL, unit).
            crate::sim::world::EntryClickAction::Tote => {
                if sim.local_techno_controllable_with(&control, id, rules) {
                    orders.push(Command::MoveToObject {
                        entity_id: id,
                        target_id: hover.stable_id,
                    });
                }
                return false;
            }
        };
        if sim.local_techno_controllable_with(&control, id, rules) {
            orders.push(Command::MegaMission {
                entity_id: id,
                mission,
                target: None,
                destination: Some(TargetKind::Entity(hover.stable_id)),
            });
        }
        false
    });
    consumed.then_some(orders)
}

/// Ares 3.0 `10049690` (hook `0x51E7BF`): whether `actor` captures the
/// vehicle `target`, for the cursor and the click alike, while Ares plays.
pub(in crate::app::input) fn vehicle_capture_action(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    actor: u64,
    target: u64,
    modifier: OrderModifier,
    force_move_key: bool,
) -> bool {
    // Shift and the admitted Ctrl+Alt GuardArea return before51E7A6.
    rules.ares()
        && !matches!(
            object_key_action_with(
                sim,
                control,
                rules,
                actor,
                target,
                Some(target),
                modifier,
                force_move_key
            ),
            Some(ObjectKeyAction::ToggleSelect | ObjectKeyAction::GuardArea)
        )
        && sim.ares_vehicle_capture_action(control, actor, target, rules)
}

/// Engineer object actions before event enqueue. No duplicate geometry query.
/// Some(empty) is a consumed no-order hut click; None allows other actions.
pub(super) fn engineer_capture_orders(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &mut Vec<u64>,
    hover: &HoverTargetKindWithId,
) -> Option<Vec<Command>> {
    if !matches!(
        hover.kind,
        HoverTargetKind::FriendlyStructure | HoverTargetKind::EnemyStructure
    ) {
        return None;
    }
    let mut consumed = false;
    let mut orders = Vec::new();
    let control = sim.control_projection();
    selected.retain(|&id| {
        let Some(actor) = sim.entities().get(id) else {
            return true;
        };
        if actor.category() != EntityCategory::Infantry
            || !sim
                .interner()
                .object_type(actor.type_ref(), rules)
                .is_some_and(|o| o.engineer)
        {
            return true;
        }
        let Some(action) = sim.engineer_building_action_with(&control, id, hover.stable_id, rules)
        else {
            return true;
        };
        use crate::sim::world::EngineerBuildingAction;
        consumed = true;
        // Foot4D7716 calls Infantry+A0/Techno700C40 before enqueue.
        if sim.local_techno_controllable_with(&control, id, rules) {
            match action {
                EngineerBuildingAction::Capture
                | EngineerBuildingAction::Damage
                | EngineerBuildingAction::Repair(true) => orders.push(Command::CaptureBuilding {
                    engineer_id: id,
                    target_building_id: hover.stable_id,
                }),
                // Foot clicked actions3/11, 4D76F6/4D7761: NULL Target,
                // building Destination. Event4 and Event36 share these tokens.
                EngineerBuildingAction::EnterHospital | EngineerBuildingAction::EnterGrinder => {
                    orders.push(Command::MegaMission {
                        entity_id: id,
                        mission: if action == EngineerBuildingAction::EnterHospital {
                            crate::sim::mission::MissionType::Enter
                        } else {
                            crate::sim::mission::MissionType::Eaten
                        },
                        target: None,
                        destination: Some(TargetKind::Entity(hover.stable_id)),
                    });
                }
                // Ares NoEnter (10025220) gives no order either.
                EngineerBuildingAction::Repair(false) | EngineerBuildingAction::NoEnter => {}
            }
        }
        false
    });
    consumed.then_some(orders)
}
