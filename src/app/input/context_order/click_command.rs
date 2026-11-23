//! Per-receiver click commands: attack-move admission, object key actions, guard
//! area, the Move / object / attack click payloads, cell harvest and the move flash.

use super::{
    AttackAction, InfantryAttackAction, OrderModifier, ares_ivan_grinds, attack_action_with,
    ctrl_forces_attack_rung, healer_guards_itself, infantry_attack_action_with, ivan_cannot_bomb,
    shrouded_object_action,
};
use crate::app::input::entity_pick::{HoverObject, HoverTargetKindWithId};
use crate::app::types::{HoverTargetKind, OrderMode};
use crate::map::entities::EntityCategory;
use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::intern::InternedId;
use crate::sim::world::ControlProjection;

/// Does this object accept an attack-move order? Its `Can_Attack_Move`
/// ([`crate::sim::world::Simulation::techno_can_attack_move`]).
pub(super) fn entity_can_attack_move(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    stable_id: u64,
) -> bool {
    rules.is_some_and(|rules| sim.techno_can_attack_move(stable_id, rules))
}

/// Every selected object has to accept an attack-move order for the chord to fire.
///
/// The chord test walks the *whole* current selection — buildings and aircraft
/// included, not just the mobiles that would receive the order — and fails
/// outright the moment one member answers no.
pub(in crate::app::input) fn selection_can_attack_move(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    selected_ids: &[u64],
) -> bool {
    if selected_ids.is_empty() {
        return false;
    }
    selected_ids
        .iter()
        .all(|&sid| entity_can_attack_move(sim, rules, sid))
}

/// Foot ClickedAction_Object's AreaGuard case (`0x004D7543`) for a GuardArea
/// over an object: Ctrl+Alt's guard rung, or a healer infantryman over
/// itself, which Infantry's ClickedAction (`0x0051F190`) hands on
/// unchanged. A berserk receiver is refused at its head (`0x004D74E0`);
/// the case asks IsActive (vt+0x9C, `0x007010D0`) and vt+0xA0 and orders
/// nothing without them, so an unarmed member of a group stays put;
/// otherwise Area Guard on the object.
pub(super) fn object_guard_command(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    receiver: u64,
    target_id: u64,
) -> Option<Command> {
    (!sim
        .entities()
        .get(receiver)
        .is_some_and(|entity| entity.berserk.is_active())
        && sim.local_techno_is_active_with(control, receiver, rules)
        && sim.local_techno_controllable_with(control, receiver, rules))
    .then_some(Command::Guard {
        entity_id: receiver,
        target: Some(TargetKind::Entity(target_id)),
    })
}

/// Foot ClickedAction_Object's case 0x3B (`0x004D74E0`), a healer
/// infantryman's AttackSupport over an object
/// ([`crate::sim::world::InfantryHealAction::AttackSupport`]): Area Guard on
/// the healer itself when the Damage of its GetWeapon(0) (vt+0x3F8) heals,
/// else Guard on itself. The case asks neither IsActive nor vt+0xA0.
///
/// RESIDUAL: VERA sends no Guard order, so a healer whose Secondary heals
/// and whose Primary does not orders nothing. Only a mod's healers reach it.
pub(super) fn attack_support_command(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    receiver: u64,
) -> Option<Command> {
    let entity = sim.entities().get(receiver)?;
    let object = sim.interner().object_type(entity.type_ref(), rules)?;
    crate::sim::combat::combat_weapon::get_weapon(
        entity,
        object,
        0,
        sim.entities(),
        rules,
        sim.interner(),
    )
    .is_some_and(|weapon| weapon.damage < 0)
    .then_some(Command::Guard {
        entity_id: receiver,
        target: Some(TargetKind::Entity(receiver)),
    })
}

/// What Techno What_Action_OnObject's key rungs answer
/// (`0x00700006..0x00700217`), for the cursor and the click alike, as
/// Foot's shroud rung leaves them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app::input) enum ObjectKeyAction {
    /// Shift's ToggleSelect (8, `0x00700107`). Over a Terrain object, which
    /// cannot be selected, the release selects nothing and keeps the
    /// selection.
    ToggleSelect,
    /// Ctrl+Alt's GuardArea (0x1A, `0x007001DD`, [`guard_area_admitted`]).
    GuardArea,
    /// Alt's Move (`0x007001E9..0x00700217`), in any chord. Foot
    /// `0x004D74E0` moves the receiver to the object or near it.
    Move,
    /// Foot's shroud rung's NoMove for any of them ([`shrouded_object_action`]),
    /// which `0x004D74E0` refuses for a type without `MoveToShroud=`
    /// (`0x004D77BD..0x004D77EA`).
    NoMove,
}

/// The key rungs of `actor_id`'s What_Action over `object`: `target` is the
/// object as a Techno, or None for a Terrain object, which is no ally for the
/// guard rung (`0x004F9A90` reads its vt+0x3C, TerrainClass's null house).
/// They follow the berserk exit and warp arm (`0x006FFEC9`, `0x006FFF03`).
/// Shift without Ctrl answers ToggleSelect for a player's object that is not
/// mission-only (`0x007000EB..0x00700107`); Alt, Ctrl+Alt and Ctrl+Shift+Alt
/// answer Move for a player's controllable object (vt+0xA0) once the guard
/// rung refused. In a campaign a Foot's shroud rung then makes any of them
/// Move or NoMove over an object whose coordinate the player's map shrouds
/// ([`shrouded_object_action`]); Infantry tests for ToggleSelect only after
/// it (`0x0051E44C`).
/// None leaves the later rungs (for a Terrain object, plain, Ctrl and
/// Ctrl+Shift: [`crate::app::input::cursor::terrain_object_feedback`]). A
/// selected building's Move over a Terrain object then asks its own factory
/// rungs (`0x004473F4..0x0044750B`, [`building_object_orders`]).
pub(in crate::app::input) fn object_key_action(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    object: u64,
    target: Option<u64>,
    modifier: OrderModifier,
    force_move_key: bool,
) -> Option<ObjectKeyAction> {
    object_key_action_with(
        sim,
        &sim.control_projection(),
        rules,
        actor_id,
        object,
        target,
        modifier,
        force_move_key,
    )
}

pub(super) fn object_key_action_with(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    object: u64,
    target: Option<u64>,
    modifier: OrderModifier,
    force_move_key: bool,
) -> Option<ObjectKeyAction> {
    let actor = sim.entities().get(actor_id)?;
    if actor.berserk.is_active() || actor.is_warped_out() {
        return None;
    }
    let human = control.can_order(actor.owner());
    let alt = matches!(
        modifier,
        OrderModifier::ForceMove | OrderModifier::GuardArea
    ) || (modifier == OrderModifier::AttackMove && force_move_key);
    let action = if modifier == OrderModifier::Shift {
        (human && !actor.is_mission_only()).then_some(ObjectKeyAction::ToggleSelect)
    } else if modifier == OrderModifier::GuardArea
        && target.is_some()
        && guard_area_admitted(sim, control, rules, actor_id, target)
    {
        Some(ObjectKeyAction::GuardArea)
    } else {
        (alt && human && sim.local_techno_controllable_with(control, actor_id, rules))
            .then_some(ObjectKeyAction::Move)
    }?;
    Some(
        match shrouded_object_action(sim, control, rules, actor_id, object) {
            Some(crate::sim::world::MoveAction::Move) => ObjectKeyAction::Move,
            Some(_) => ObjectKeyAction::NoMove,
            None => action,
        },
    )
}

/// Whether What_Action's later rungs over a Terrain object see Ctrl: it sets
/// the forced flag even with Alt held (`0x00700048..0x00700072`), so Ctrl
/// and Ctrl+Alt force, while Ctrl+Shift clears it for the Shift test
/// (`0x007000AC..0x007000B4`).
pub(in crate::app::input) fn terrain_forced(modifier: OrderModifier) -> bool {
    matches!(
        modifier,
        OrderModifier::ForceFire | OrderModifier::GuardArea
    )
}

/// The object under the click as [`object_key_action`] takes it: a Techno
/// the player may see (`hover`; an enemy the player cannot see is no object
/// to click) or a Terrain object, with the Techno's id as its target.
pub(super) fn object_key_target(
    object: HoverObject,
    hover: Option<&HoverTargetKindWithId>,
) -> Option<(u64, Option<u64>)> {
    match object {
        HoverObject::Terrain(id) => Some((id, None)),
        HoverObject::Techno(_) => hover
            .filter(|target| target.kind != HoverTargetKind::HiddenEnemy)
            .map(|target| (target.stable_id, Some(target.stable_id))),
    }
}

/// Techno object700191..700217 / Cell700830..700894. The keyboard chord
/// alone is not a Guard action. Object targets must be allied and the actor
/// no Aircraft (RTTI 2, 7001B4..7001BA); bare ground requires IsActive7010D0.
/// The cell rung's patrol answer (0x33, `0x00700897`) needs a legacy House
/// waypoint, which YR never creates (see `Simulation::cell_move_action`).
/// A berserk object answers 0 before the rung (object `0x006FFEC9`, Cell
/// `0x00700609`). Shared by the hover and click producers; no simulation
/// state is changed.
pub(super) fn guard_area_admitted(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target_id: Option<u64>,
) -> bool {
    let Some(actor) = sim.entities().get(actor_id) else {
        return false;
    };
    if actor.berserk.is_active()
        || !control.can_order(actor.owner())
        || !sim.local_techno_controllable_with(control, actor_id, rules)
    {
        return false;
    }
    match target_id {
        // The ally test (`Is_Ally_ByObject @ 0x004F9A90`) has no identity
        // check: in a group, Ctrl+Alt on a member guards that member.
        Some(target_id) => {
            actor.category() != EntityCategory::Aircraft
                && sim.entities().get(target_id).is_some_and(|target| {
                    crate::map::houses::is_allied_with(
                        sim.house_alliances().as_map(),
                        sim.interner().resolve(actor.owner()),
                        sim.interner().resolve(target.owner()),
                    )
                })
        }
        None => sim.local_techno_is_active_with(control, actor_id, rules),
    }
}

/// Techno What_Action on a Cell's Ctrl+Alt rung (`0x00700815..0x007008A3`):
/// whether `actor_id` answers GuardArea (0x1A) on `cell`, ahead of the move
/// rungs, as `viewer` sees it. The class wrappers then keep it, except:
/// - over a shrouded Cell centre a Foot's wrapper turns it into Move or
///   NoMove ([`Simulation::foot_cell_shroud_action`]), as the move rungs
///   answer there;
/// - a Building with a rally line turns it into Move (`0x0044774B`); any
///   other Building's wrapper (`0x00447540`) keeps it, shroud or not.
///
/// The hover, the Foot click and the Building click all ask this.
///
/// [`Simulation::foot_cell_shroud_action`]: crate::sim::world::Simulation::foot_cell_shroud_action
pub(in crate::app::input) fn cell_guard_area_action(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    viewer: Option<InternedId>,
    cell: (u16, u16),
) -> bool {
    cell_guard_area_action_with(
        sim,
        &sim.control_projection(),
        rules,
        actor_id,
        viewer,
        cell,
    )
}

pub(super) fn cell_guard_area_action_with(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    viewer: Option<InternedId>,
    cell: (u16, u16),
) -> bool {
    let Some(actor) = sim.entities().get(actor_id) else {
        return false;
    };
    let rewritten = if actor.category() == EntityCategory::Structure {
        sim.interner()
            .object_type(actor.type_ref(), rules)
            .is_some_and(|object| object.has_rally_line())
    } else {
        viewer.is_some_and(|viewer| {
            sim.foot_cell_shroud_action(viewer, actor_id, cell, rules)
                .is_some()
        })
    };
    !rewritten && guard_area_admitted(sim, control, rules, actor_id, None)
}

/// The order one receiver commits for a Move click's destination.
/// ClickedMission (`0x006FFBE0`) promotes Move to attack-move (mission 29)
/// while the attack-move chord or toggle is on (`0x00731BF0`) and the
/// receiver can attack-move (vt+0x4C0), and the event keeps the click's
/// destination, a Cell or the object (`0x006FFDA5..0x006FFDE6`). The
/// promotion is per object: a unit whose type refuses it keeps the plain Move
/// it committed, even when the rest of the group attack-moves.
pub(super) fn move_click_command(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    order_mode: OrderMode,
    entity_id: u64,
    destination: TargetKind,
) -> Command {
    let attack_move =
        order_mode == OrderMode::AttackMove && entity_can_attack_move(sim, Some(rules), entity_id);
    match destination {
        _ if attack_move => Command::AttackMove {
            entity_id,
            target: None,
            destination: Some(destination),
        },
        TargetKind::Cell(target_rx, target_ry) => Command::Move {
            entity_id,
            target_rx,
            target_ry,
        },
        TargetKind::Entity(target_id) => Command::MoveToObject {
            entity_id,
            target_id,
        },
    }
}

/// The command one selected unit commits for a tactical click on an object.
///
/// ClickedMission (`0x006FFBE0`) promotes the committed **Attack** to
/// attack-move as readily as a committed Move, gated per object on its own
/// type predicate, and the event keeps the clicked object as its target
/// (`0x004D76B1..0x004D76C1`). `Assign_Target_Command` stores it as MegaTarget
/// and answers Attack, so the unit attacks it as an attack-mover; a member
/// whose type refuses attack-move commits the plain attack. A forced
/// Attack is promoted too: in the attack-move mode (`0x00B0FE58`, set by
/// `0x00731AF0`) `Is_Attack_Move_Chord_Active @ 0x00731BF0` passes before
/// it reads any key, so Ctrl there attack-moves onto the object.
pub(super) fn object_click_payload(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    order_mode: OrderMode,
    attacker_id: u64,
    target_id: u64,
) -> Option<Command> {
    let actor = sim.entities().get(attacker_id)?;
    // gamemd.exe InfantryClass::ClickedAction_Object 0x0051F190 (51F1C7)
    // and UnitClass::ClickedAction_Object 0x00738890 (7388D5) reject self Attack.
    if attacker_id == target_id
        && matches!(
            actor.category(),
            EntityCategory::Infantry | EntityCategory::Unit
        )
    {
        return None;
    }
    // gamemd.exe FootClass::ClickedAction_Object 0x004D769F..0x004D76C1:
    // the resulting Attack5 asks IsActive7010D0 before ClickedMission.
    // Infantry capture/sabotage return before this owner; Cell Attack4D80B1
    // and Building44343C do not have this Foot gate. Never recheck on receipt.
    if actor.category() != EntityCategory::Structure
        && !sim.local_techno_is_active(attacker_id, rules)
    {
        return None;
    }
    Some(match order_mode {
        OrderMode::AttackMove if entity_can_attack_move(sim, Some(rules), attacker_id) => {
            Command::AttackMove {
                entity_id: attacker_id,
                target: Some(TargetKind::Entity(target_id)),
                destination: None,
            }
        }
        _ => Command::Attack {
            attacker_id,
            target_id,
        },
    })
}

/// The order one receiver commits for a click on another house's object,
/// or an Alt or Ctrl+Alt click on any Techno: its own What_Action there,
/// which the selection's click loop hands to its ClickedAction
/// (`0x004AE829..0x004AE844`). The key rungs answer first
/// ([`object_key_action`]).
///
/// Unforced, that is [`attack_action`]'s answer. NoMove moves the receiver
/// to the object or a Cell near it (Foot `0x004D74E0`), as an attack-move
/// under the chord ([`Simulation::object_move_click`],
/// [`move_click_command`]); Select and 0 order nothing.
///
/// Attack orders [`object_click_payload`]. What_Action decides Attack before
/// ClickedMission promotes it to attack-move, so an unforced shot ILLEGAL to
/// the receiver leaves no Attack to promote: its tail answers 0 or Select
/// there ([`attack_action`]). A Crazy Ivan's Attack over an object he cannot
/// bomb is NoIvanBomb (`0x0051EB24`), which orders nothing; with Ares, his
/// Attack over an allied Grinder is Grind ([`ares_ivan_grinds`]), sent as
/// mission Eaten.
///
/// Ctrl bypasses friendly/ILLEGAL rejection in the existing action owner,
/// but still requires armed or Infiltrate admission (`700444..700474`).
/// Its Select/NoMove tail remains the ordinary selection/movement path.
/// Original controls: allied_object_action.json unarmed_object_controls.
///
/// [`Simulation::object_move_click`]: crate::sim::world::Simulation::object_move_click
#[cfg(test)]
pub(super) fn object_attack_click_command(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    issuing_house: InternedId,
    order_mode: OrderMode,
    modifier: OrderModifier,
    force_move_key: bool,
    attacker_id: u64,
    target_id: u64,
) -> Option<Command> {
    object_attack_click_command_with(
        sim,
        &sim.control_projection(),
        rules,
        overlay_registry,
        issuing_house,
        order_mode,
        modifier,
        force_move_key,
        matches!(
            modifier,
            OrderModifier::ForceFire | OrderModifier::AttackMove | OrderModifier::GuardArea
        ),
        attacker_id,
        target_id,
    )
}

/// `force_fire_key` is the force-fire key's own state, which Ares' Ivan rung
/// reads ([`ares_ivan_grinds`]).
pub(super) fn object_attack_click_command_with(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    issuing_house: InternedId,
    order_mode: OrderMode,
    modifier: OrderModifier,
    force_move_key: bool,
    force_fire_key: bool,
    attacker_id: u64,
    target_id: u64,
) -> Option<Command> {
    let move_near = |action| {
        let destination = crate::app::input::commands::object_move_destination_with(
            sim,
            control,
            rules,
            issuing_house,
            attacker_id,
            target_id,
            action,
            force_move_key,
        )?;
        Some(move_click_command(
            sim,
            rules,
            order_mode,
            attacker_id,
            destination,
        ))
    };
    // The receiver's own key rungs ([`object_key_action`]) come before its
    // attack rung: Shift's ToggleSelect, which Foot ClickedAction's default
    // case takes with no order (`0x004D7CC0`) and Infantry asks before its
    // heal arm (`0x0051E44C`); Ctrl+Alt's guard rung, else Alt's Move, which
    // Foot's shroud rung may make NoMove.
    let key_action = object_key_action_with(
        sim,
        control,
        rules,
        attacker_id,
        target_id,
        Some(target_id),
        modifier,
        force_move_key,
    );
    if key_action == Some(ObjectKeyAction::ToggleSelect) {
        return None;
    }
    // A healer's GuardArea over itself replaces its key answers
    // ([`healer_guards_itself`]).
    if target_id == attacker_id
        && healer_guards_itself(
            sim,
            control,
            rules,
            attacker_id,
            ctrl_forces_attack_rung(modifier, key_action),
        )
    {
        return object_guard_command(sim, control, rules, attacker_id, target_id);
    }
    match key_action {
        Some(ObjectKeyAction::GuardArea) => {
            return object_guard_command(sim, control, rules, attacker_id, target_id);
        }
        Some(ObjectKeyAction::NoMove) => {
            return move_near(crate::sim::world::MoveAction::NoMove);
        }
        Some(_) => return move_near(crate::sim::world::MoveAction::Move),
        None => {}
    }
    // A receiver the player may not control (vt+0xA0) passes both key
    // rungs (`0x00700191..0x00700217`) to the attack rung (`0x0070021A`),
    // Ctrl still forcing it under Ctrl+Alt.
    let forced = matches!(
        modifier,
        OrderModifier::ForceFire | OrderModifier::GuardArea
    );
    // Every other receiver asks its own action, including Ctrl; forcing
    // does not invent Attack admission.
    match attack_action_with(
        sim,
        control,
        rules,
        attacker_id,
        &TargetKind::Entity(target_id),
        forced,
        Some(overlay_registry),
    ) {
        // Ares' Grind over an allied Grinder: Foot ClickedAction
        // (`0x004D74E0`) case 0xB, for a receiver the player may order.
        AttackAction::Attack
            if ares_ivan_grinds(sim, rules, attacker_id, target_id, force_fire_key) =>
        {
            return sim
                .local_techno_controllable_with(control, attacker_id, rules)
                .then_some(Command::MegaMission {
                    entity_id: attacker_id,
                    mission: crate::sim::mission::MissionType::Eaten,
                    target: None,
                    destination: Some(TargetKind::Entity(target_id)),
                });
        }
        // NoIvanBomb (`0x0051EB24`) orders nothing.
        AttackAction::Attack if ivan_cannot_bomb(sim, rules, attacker_id, target_id) => {
            return None;
        }
        AttackAction::Attack => {}
        AttackAction::NoMove => {
            return move_near(crate::sim::world::MoveAction::NoMove);
        }
        AttackAction::Move => return move_near(crate::sim::world::MoveAction::Move),
        AttackAction::AttackSupport => return attack_support_command(sim, rules, attacker_id),
        AttackAction::Select
        | AttackAction::None
        | AttackAction::Rally
        | AttackAction::GuardArea => {
            return None;
        }
    }
    match infantry_attack_action_with(sim, control, rules, attacker_id, target_id) {
        Some(InfantryAttackAction::NoMove) => {
            return move_near(crate::sim::world::MoveAction::NoMove);
        }
        Some(InfantryAttackAction::Select) => return None,
        // Residual (dormant): Foot's Capture and Sabotage cases
        // (`0x004D7716`, `0x004D7788`) also refuse a receiver the player may
        // not control (vt+0xA0); these arms do not ask. No retail Engineer,
        // Spy or C4 infantryman can be bunkered, deactivated, paralysed or
        // EMP'd, the states that reach the attack rung under Alt.
        Some(InfantryAttackAction::Infiltrate) => {
            return Some(Command::CaptureBuilding {
                engineer_id: attacker_id,
                target_building_id: target_id,
            });
        }
        Some(InfantryAttackAction::Sabotage) => {
            return Some(Command::PlantC4 {
                attacker_id,
                target_building_id: target_id,
            });
        }
        _ => {}
    }
    object_click_payload(sim, rules, order_mode, attacker_id, target_id)
}

/// A receiver's order when its ClickedAction on a Cell re-asks its What_Action
/// with mouse-over 0 (`0x004D7DA1`) and gets Harvest
/// ([`crate::sim::world::Simulation::cell_move_action`]): case 6 sends
/// ClickedMission Harvest with the Cell as Destination and no Target
/// (`0x004D7E8E`), before the MoveFlash. The class wrapper answers 0 for a
/// House that is not the player's (`0x007404BF`); the MEGAMISSION event
/// itself tests no owner (`0x004C71F6..0x004C721F`).
pub(super) fn cell_harvest_order(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    viewer: InternedId,
    receiver: u64,
    cell: (u16, u16),
    shift: bool,
) -> Option<Command> {
    let query = crate::sim::world::CellActionQuery {
        mouse_over: false,
        shift,
    };
    (sim.cell_move_action_with(control, viewer, receiver, cell, query, rules)
        == Ok(crate::sim::world::MoveAction::Harvest))
    .then(|| {
        Command::clicked_mission(
            receiver,
            crate::sim::mission::MissionType::Harvest,
            TargetKind::Cell(cell.0, cell.1),
        )
    })
}

/// `FootClass::ClickedAction_Cell`'s MoveFlash (`0x004D7EB5..0x004D8065`)
/// for one receiver of a Cell click: only the selection's first object asks
/// ([`crate::sim::world::Simulation::cell_click_flashes_move`]), and its
/// flash is queued before its own order, as native constructs it before the
/// receiver's destination resolves. Planning Mode clicks reach the same
/// receiver loop (`0x004ABF94`), so they flash too; a plan's execution clears
/// the latch around its own ClickedAction (`0x004DC96F..0x004DC984`).
pub(super) fn push_move_flash(
    queued: &mut Vec<CommandEnvelope>,
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    owner_id: InternedId,
    execute_tick: u64,
    current_objects: &[u64],
    receiver: u64,
    cell: (u16, u16),
    shift: bool,
) {
    if current_objects.first() != Some(&receiver) {
        return;
    }
    let flashes = sim
        .cell_click_flashes_move_with(control, owner_id, receiver, cell, shift, rules)
        .unwrap_or_else(|cause| {
            log::warn!("MoveFlash click input: {cause}");
            false
        });
    if flashes {
        queued.push(CommandEnvelope::new(
            owner_id,
            execute_tick,
            Command::MoveFlash {
                rx: cell.0,
                ry: cell.1,
            },
        ));
    }
}
