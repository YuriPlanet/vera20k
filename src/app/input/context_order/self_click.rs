//! Clicks on the selection's own object: deploy, self-detonation, Ares deactivated
//! actions and the self-click command a lone foot unit or building sends.

use super::{
    LeftRelease, ReleaseAction, context_actor_viewer, context_owns_actor, left_release,
    move_click_command, object_guard_command,
};
use crate::app::input::entity_pick::HoverTargetKindWithId;
use crate::app::types::OrderMode;
use crate::map::entities::EntityCategory;
use crate::sim::command::Command;
use crate::sim::intern::InternedId;
use crate::sim::world::ControlProjection;

/// The data capability shared by self-click, self-hover and the deploy key.
/// `UnitClass::What_Action_OnObject @ 0x0074000B` admits `IsSimpleDeployer`
/// to the deploy action; `CanDeploySlashUnload @ 0x00700E81` reads that same
/// UnitType flag. Deploy/undeploy transitions do not remove the capability.
/// The simulation owns command admission and the deployment lifecycle.
pub(in crate::app::input) fn is_simple_deploy_unit(
    entity: &crate::sim::game_entity::GameEntity,
    object: &crate::rules::object_type::ObjectType,
) -> bool {
    entity.category() == EntityCategory::Unit && object.is_simple_deployer
}

/// Infantry51EC14..51EC9F blocks Doing27..31 only when Type+6C4
/// (UndeployDelay) > -1; Type+6A8 (DeployFireWeapon) is unrelated.
/// Unit740015..74003F blocks an empty mobile transport outside a tank bunker
/// (`+0x2E4`, `0x00740023`). Both answer NoDeploy30 rather than a self
/// action. The D-key's receiver is separate.
fn self_deploy_is_blocked(
    entity: &crate::sim::game_entity::GameEntity,
    object: &crate::rules::object_type::ObjectType,
) -> bool {
    (entity.category() == EntityCategory::Infantry
        && object.deployer
        && object.undeploy_delay > -1
        && entity
            .infantry_sprite_pose()
            .is_some_and(|(doing, _)| (27..=31).contains(&doing)))
        || (entity.category() == EntityCategory::Unit
            && object.passengers > 0
            && !object.is_simple_deployer
            && entity.bunker.link().installed_in().is_none()
            && !object.no_manual_unload()
            && entity
                .passenger_role()
                .cargo()
                .is_none_or(|cargo| cargo.is_empty()))
}

/// Techno What_Action's Detonate rung (`0x006FFF8D..0x00700003`) for the
/// lone selected (`[0xA8ECC8] == 1`) `target` over itself, past the berserk
/// and warp exits (`0x006FFEC9`, `0x006FFF03`): its bomb may go off on a
/// click ([`crate::sim::world::Simulation::bomb_detonate_action`]). The
/// carrier's owner is not asked: a player may set off a bomb on a selected
/// enemy. Cursor and click share it; both ask an Engineer's DisarmBomb
/// first (Infantry `0x0051E462`). The click sends the DETONATE event
/// (`Simulation::encode_detonate_record`), with no voice.
///
/// RESIDUALS: only Infantry and Units are represented. Native also answers
/// it for an Aircraft or Building carrier, whose class ladders VERA does not
/// route through this rung, and a player's healer Unit (negative weapon
/// damage) over itself answers Select first (`0x0073FDF0..0x0073FE3B`).
/// Shift's ToggleSelect, which native asks after this rung, is asked first.
pub(in crate::app::input) fn self_detonates(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected_ids: &[u64],
    target: u64,
) -> bool {
    // Ares 3.0 IvanBombsA (10056D30, hook 0x6FFEC0) answers before the
    // berserk and warp exits; the click's berserk and deactivation refusals
    // let Detonate through (0x51F1D8, 0x7388EB, 0x4D74EC: 10056BF0,
    // 10056DA0, 1004CBC0).
    let exits = |entity: &crate::sim::game_entity::GameEntity| {
        !rules.ares() && (entity.berserk.is_active() || entity.is_warped_out())
    };
    selected_ids == [target]
        && sim.entities().get(target).is_some_and(|entity| {
            matches!(
                entity.category(),
                EntityCategory::Infantry | EntityCategory::Unit
            ) && !exits(entity)
        })
        && sim.bomb_detonate_action(target, rules)
}

/// What a Deactivated (`+0x1C8`) object answers over an object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app::input) enum AresDeactivatedAction {
    Detonate,
    Select,
    None,
}

/// Ares hooks at the head of each class What_Action over an object (Unit
/// `0x73FD5A`, Infantry `0x51E440`, Aircraft `0x417CCB`, Building
/// `0x447218`; Ares.dll `1004CF50`, `1004CC30`, `1004CB00`, `1004CB70`):
/// a Deactivated `actor` answers `TechnoExt::GetDeactivatedAction @
/// 10044AD0` alone, None when it is not Deactivated. Detonate over itself as
/// the lone selection when its bomb may be set off ([`self_detonates`]),
/// Select over an allied (`HouseClass::IsAlliedWith`) selectable object,
/// else 0. None without Ares.
pub(in crate::app::input) fn ares_deactivated_action(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &[u64],
    actor: u64,
    target: u64,
) -> Option<AresDeactivatedAction> {
    if !rules.ares() {
        return None;
    }
    let actor = sim
        .entities()
        .get(actor)
        .filter(|actor| actor.deactivated.is_offline())?;
    Some(if self_detonates(sim, rules, selected, target) {
        AresDeactivatedAction::Detonate
    } else if sim.entities().get(target).is_some_and(|target| {
        sim.house_alliances()
            .is_allied(sim.interner(), actor.owner(), target.owner())
    }) && sim.object_is_selectable(target, rules)
    {
        AresDeactivatedAction::Select
    } else {
        AresDeactivatedAction::None
    })
}

/// Ares 3.0 InfantryClass_GetActionOnObject_EMP (Ares.dll `10053DB0`, hook
/// at InfantryClass::What_Action_OnObject's entry `0x0051E3B0`, resuming at
/// its return `0x0051F187`): an Infantryman whose type is `Deployer=`
/// (`+0xEC8`) answers NoDeploy (0x1E) over itself while under EMP (vt+0x37C),
/// whatever the selection and keys. It precedes every other rung: Ares'
/// Deactivated hook (`0x0051E440`, [`ares_deactivated_action`]), the healer
/// self arm, the bomb and Techno's self rung. False without Ares.
pub(in crate::app::input) fn ares_emp_deployer_refuses_itself(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor: u64,
    target: u64,
) -> bool {
    rules.ares()
        && actor == target
        && sim.entities().get(actor).is_some_and(|entity| {
            entity.category() == EntityCategory::Infantry
                && entity.is_under_emp()
                && sim
                    .interner()
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| object.deployer)
        })
}

/// The left release when an Ares hook at the head of the selection's cursor
/// owner's What_Action answers
/// ([`crate::app::input::cursor::select_best_for_action`]): DetermineAction
/// takes the owner's action for the whole selection (`0x00692634`,
/// `0x00692655`).
/// - An EMP'd deployer over itself answers NoDeploy
///   ([`ares_emp_deployer_refuses_itself`]), which dispatches
///   ([`LeftRelease::Dispatch`], `0x004ABE88`), so the click selects nothing.
/// - A Deactivated owner answers [`ares_deactivated_action`] over an object
///   and 0 over a Cell (`Simulation::cell_move_action`). Detonate is the
///   caller's; the other two reach [`left_release`], so no object receives
///   an order.
///
/// None without Ares. These hooks precede Techno's Shift rung
/// (`0x00700107`), for the click ([`super::queue::head_release`]) as for the cursor,
/// and a Select or 0 under Shift still clears the selection first
/// ([`super::ContextClick::Select`]).
pub(super) fn ares_head_hook_selection_click(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    current_objects: &[u64],
    hover: Option<&HoverTargetKindWithId>,
    cell: (u16, u16),
) -> Option<Result<LeftRelease, u64>> {
    use crate::app::input::cursor::ActionDistanceTarget;
    if !rules.ares() {
        return None;
    }
    let target = hover.map_or(ActionDistanceTarget::CellCentre(cell.0, cell.1), |target| {
        ActionDistanceTarget::Object(target.stable_id)
    });
    let best = crate::app::input::cursor::select_best_for_action(
        sim,
        current_objects,
        target,
        Some(rules),
    )?;
    let Some(target) = hover else {
        return sim
            .entities()
            .get(best)
            .is_some_and(|best| best.deactivated.is_offline())
            .then_some(Ok(LeftRelease::Nothing));
    };
    if ares_emp_deployer_refuses_itself(sim, rules, best, target.stable_id) {
        return Some(Ok(LeftRelease::Dispatch));
    }
    Some(
        match ares_deactivated_action(sim, rules, current_objects, best, target.stable_id)? {
            AresDeactivatedAction::Detonate => Err(target.stable_id),
            AresDeactivatedAction::Select => Ok(left_release(
                sim,
                rules,
                ReleaseAction::Select,
                Some(target.stable_id),
            )),
            AresDeactivatedAction::None => Ok(left_release(
                sim,
                rules,
                ReleaseAction::Zero,
                Some(target.stable_id),
            )),
        },
    )
}

/// The command a click on the one selected object issues when it clicks
/// itself. `TechnoClass::What_Action_OnObject` returns ACTION_SELF only when
/// the target is the acting object, exactly one object is selected
/// (`[0xA8ECC8] == 1`, `0x007000C1`) and its owner is the current player
/// (`0x007000B9..0x007000DF`). With more selected, a click on one of them
/// takes the tail (`0x0070056C..0x007005E6`), whose 0 over a selected object
/// (`0x007005BF`) a player's Unit or Infantryman turns into NoMove
/// (`0x007403EB`, `0x0051F05E`), as the cursor shows. NoMove dispatches
/// (`0x004ABE88`): each receiver asks its own What_Action, the others move
/// near the clicked one and a Unit refuses itself (`0x007388D5..0x007388E8`),
/// so the click selects nothing ([`object_click_release`]).
///
/// Shared self-action admission for click and hover. Transport cargo,
/// factory primary and simple deployment keep their existing owners; the
/// blocked Infantry/empty-transport suffix is self_deploy_is_blocked.
///
/// A Unit's self action answers NoDeploy where Unit What_Action's site
/// tests refuse it (`Simulation::unit_self_deploy_site_refused`: a
/// DeploysInto unit's placement, a loaded transport's first-passenger land
/// and height) or CanDeploySlashUnload does (`0x0073FFEA`)
/// ([`self_deploy_is_refused`]). In Planning Mode (`0x00AC4CF4`) a Unit's or
/// Infantryman's every self action answers NoDeploy
/// (`Planning__AllowsAction 0x00637DB0`, called at `0x0073FEA0`,
/// `0x0051EC1B`, `0x0051ECA4`); a building's or aircraft's is unaffected.
/// NoDeploy orders nothing.
pub(in crate::app::input) fn self_click_command(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    local_owner: InternedId,
    selected_ids: &[u64],
    hover: Option<&HoverTargetKindWithId>,
    planning: bool,
) -> Option<Command> {
    let target = hover?;
    if planning_refuses_self(sim, local_owner, selected_ids, target, planning)
        || self_is_warped_out(sim, target)
    {
        return None;
    }
    // Ares hooks 0x73FD5A, 0x51E440, 0x417CCB and 0x447218 answer a
    // Deactivated object's What_Action over itself with GetDeactivatedAction
    // 10044AD0 (Detonate or Select), never Self, so it neither deploys nor
    // repacks.
    if rules.ares()
        && sim
            .entities()
            .get(target.stable_id)
            .is_some_and(|entity| entity.deactivated.is_offline())
    {
        return None;
    }
    self_action(sim, rules, local_owner, selected_ids, target)
        .filter(|_| !unit_refuses_deploy(sim, rules, registry, target.stable_id))
}

/// The lone selected object's NoDeploy (0x1E) over itself: a refused self
/// action ([`self_deploy_is_refused`]), or one blocked before it
/// ([`self_deploy_is_blocked`], [`is_simple_deploy_unit`]). Cursor and click
/// share it. The release dispatches NoDeploy, so a click on it orders and
/// selects nothing (`0x004ABE88`).
pub(in crate::app::input) fn self_answers_no_deploy(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    local_owner: InternedId,
    selected_ids: &[u64],
    target: &HoverTargetKindWithId,
    planning: bool,
) -> bool {
    !self_is_warped_out(sim, target)
        && (self_deploy_is_refused(
            sim,
            rules,
            registry,
            local_owner,
            selected_ids,
            target,
            planning,
        ) || (selected_ids == [target.stable_id]
            && sim.entities().get(target.stable_id).is_some_and(|entity| {
                context_owns_actor(sim, entity, local_owner)
                    && sim
                        .interner()
                        .object_type(entity.type_ref(), rules)
                        .is_some_and(|object| {
                            self_deploy_is_blocked(entity, object)
                                || is_simple_deploy_unit(entity, object)
                        })
            })))
}

/// Techno's warp arm (`0x006FFF03..0x006FFF8A`) precedes the self rung
/// (`0x007000B9`): an object being warped out (a Chronosphere's warp-out,
/// which leaves it selected) answers 0 over itself, as it is selected, so it
/// neither deploys nor answers NoDeploy.
fn self_is_warped_out(sim: &crate::sim::world::Simulation, target: &HoverTargetKindWithId) -> bool {
    sim.entities()
        .get(target.stable_id)
        .is_some_and(|entity| entity.is_warped_out())
}

/// The release of the lone selected Unit or Infantryman `target` clicked by
/// its owner `owner`, once its self action has been neither sent
/// ([`self_click_command`]) nor refused ([`self_answers_no_deploy`]); `None`
/// for any other click. Techno What_Action's self rung
/// (`0x007000B9..0x007000DF`) answers before the attack rung, so no attack
/// grant applies: a Crazy Ivan clicking himself bombs nothing. What is left
/// of the class's self arm is 0 (Unit `0x00740115`, Infantry `0x0051ECFC`),
/// which the class tail turns into NoMove for an object the player may order
/// ([`Simulation::foot_object_class_action`]). NoMove dispatches
/// ([`LeftRelease::Dispatch`]): the receiver's ClickedAction asks the same and sends the
/// order [`Simulation::object_move_click`] gives, which a Unit refuses over
/// itself (`0x007388D5..0x007388E8`) and an Infantryman does not. A 0 over
/// the selected object itself does nothing ([`LeftRelease::Nothing`]).
///
/// [`Simulation::foot_object_class_action`]: crate::sim::world::Simulation::foot_object_class_action
/// [`Simulation::object_move_click`]: crate::sim::world::Simulation::object_move_click
pub(super) fn lone_self_foot_release(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    owner: InternedId,
    selected_ids: &[u64],
    target: u64,
    order_mode: OrderMode,
    force_move_key: bool,
) -> Option<(LeftRelease, Option<Command>)> {
    let entity = sim.entities().get(target).filter(|entity| {
        selected_ids == [target]
            && context_owns_actor(sim, entity, owner)
            && matches!(
                entity.category(),
                EntityCategory::Unit | EntityCategory::Infantry
            )
    })?;
    let viewer = context_actor_viewer(sim, entity.owner())?;
    let action = sim.foot_object_class_action(
        viewer,
        target,
        target,
        crate::sim::world::MoveAction::None,
        rules,
    );
    if action == crate::sim::world::MoveAction::None {
        return Some((LeftRelease::Nothing, None));
    }
    Some((
        LeftRelease::Dispatch,
        crate::app::input::commands::object_move_destination(
            sim,
            rules,
            owner,
            target,
            target,
            action,
            force_move_key,
        )
        .map(|destination| move_click_command(sim, rules, order_mode, target, destination)),
    ))
}

/// A lone healer infantryman's release over itself. Infantry What_Action's
/// healer self arm (`0x0051E748`, [`Simulation::infantry_healer_guards_itself`])
/// answers GuardArea in place of the object's Detonate and self answers, and
/// the release dispatches the order [`object_guard_command`] gives. `None`
/// when the arm does not answer.
///
/// [`Simulation::infantry_healer_guards_itself`]: crate::sim::world::Simulation::infantry_healer_guards_itself
pub(super) fn lone_self_healer_guard(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    selected_ids: &[u64],
    target: u64,
) -> Option<Option<Command>> {
    (selected_ids == [target] && sim.infantry_healer_guards_itself(control, target, rules))
        .then(|| object_guard_command(sim, control, rules, target, target))
}

/// Unit What_Action's NoDeploy answer (`0x0073FFF4`) for a self action its
/// site tests or CanDeploySlashUnload refuse, and Planning Mode's for any
/// Unit or Infantry self action.
fn self_deploy_is_refused(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    local_owner: InternedId,
    selected_ids: &[u64],
    target: &HoverTargetKindWithId,
    planning: bool,
) -> bool {
    planning_refuses_self(sim, local_owner, selected_ids, target, planning)
        || (unit_refuses_deploy(sim, rules, registry, target.stable_id)
            && self_action(sim, rules, local_owner, selected_ids, target).is_some())
}

/// `Planning__AllowsAction(4)` (`0x00637DB0`) refuses a Unit's or
/// Infantryman's own Self action while Planning Mode is on.
fn planning_refuses_self(
    sim: &crate::sim::world::Simulation,
    local_owner: InternedId,
    selected_ids: &[u64],
    target: &HoverTargetKindWithId,
    planning: bool,
) -> bool {
    planning
        && selected_ids == [target.stable_id]
        && sim.entities().get(target.stable_id).is_some_and(|entity| {
            context_owns_actor(sim, entity, local_owner)
                && matches!(
                    entity.category(),
                    EntityCategory::Unit | EntityCategory::Infantry
                )
        })
}

fn unit_refuses_deploy(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    id: u64,
) -> bool {
    sim.entities()
        .get(id)
        .is_some_and(|entity| entity.category() == EntityCategory::Unit)
        && (sim.unit_self_deploy_site_refused(id, rules, registry)
            || !sim.can_deploy_slash_unload(id, rules))
}

fn self_action(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    local_owner: InternedId,
    selected_ids: &[u64],
    target: &HoverTargetKindWithId,
) -> Option<Command> {
    if selected_ids != [target.stable_id] {
        return None;
    }
    let entity = sim
        .entities()
        .get(target.stable_id)
        .filter(|entity| context_owns_actor(sim, entity, local_owner))?;
    let obj = sim.interner().object_type(entity.type_ref(), rules);
    if obj.is_some_and(|object| self_deploy_is_blocked(entity, object)) {
        return None;
    }
    // Unit740023/740107: a unit in a tank bunker skips the transport arm
    // and offers Deploy whenever CanDeploySlashUnload admits it; its normal
    // Unload mission releases the installed tank from the bunker.
    if entity.category() == EntityCategory::Unit && entity.bunker.link().installed_in().is_some() {
        return Some(Command::DeployMcv {
            entity_id: target.stable_id,
        });
    }
    // Ares 3.0 `NoManualUnload`: Unit What_Action leaves an unbunkered
    // transport's self arm with action 0 (1004BEF0, hook 0x740031 ->
    // 0x740115) ahead of its empty-hold NoDeploy (`self_deploy_is_blocked`),
    // and Aircraft What_Action its Self (10048A90, hook 0x417DD2 -> 0x417DF4).
    if obj.is_some_and(|object| {
        object.no_manual_unload()
            && match entity.category() {
                EntityCategory::Unit => object.passengers > 0 && !object.is_simple_deployer,
                EntityCategory::Aircraft => true,
                _ => false,
            }
    }) {
        return None;
    }
    // Building447286..4472E7's gap rung precedes its cargo rungs; its Self
    // click queues Unload443514, which toggles the super-charge. Residual:
    // a type both GapGenerator= and a factory would also take the primary
    // tail4472EC and Event17 (44345F); no retail type is both.
    if let Some(offered) = sim.gap_generator_self_action(target.stable_id, rules) {
        return offered.then_some(Command::UnloadPassengers {
            transport_id: target.stable_id,
        });
    }
    // Ares 3.0 100173A0 (hook 0x44731C): a tunnel entrance with its
    // owner's network loaded offers the unload action. Only the Ares-form sim
    // fills a network, so this runs in every game. Its click is the
    // absorbers' ClickedMission(Unload) arm (0x443534), which Ares opens to
    // tunnel entrances (100172F0, hook 0x44351A).
    if sim.ares_tunnel_offers_unload(target.stable_id, rules) {
        return Some(Command::UnloadPassengers {
            transport_id: target.stable_id,
        });
    }
    if let Some(command) =
        super::super::transport_orders::unload_command(sim, entity, obj, Some(local_owner))
    {
        return Some(command);
    }
    if entity.category() == EntityCategory::Structure {
        // Building44731C..447358 keeps self action for cargo/garrisons or
        // factory primary selection. The tank-bunker link is separate from
        // those containers and does not grant the Building a self action.
        // Repacking is a cell-targeted order.
        let command = Command::TogglePrimaryFactory {
            entity_id: target.stable_id,
        };
        return if crate::sim::production::can_select_factory_primary(sim, rules, target.stable_id)
            && sim.can_issue_local_command(&command, rules)
        {
            // Building4472EC preserves Action4 only for a nonprimary
            // factory; Click443450..443494 queues this target's Event17.
            Some(command)
        } else {
            None
        };
    }
    if entity.category() == EntityCategory::Infantry && obj.is_some_and(|o| o.deploy_fire) {
        return Some(Command::ToggleInfantryDeploy {
            entity_id: target.stable_id,
        });
    }
    // Unit What_Action offers its self action to a simple deployer, a
    // DeploysInto unit or a `DeployFire=` one (`0x0074000B`, `0x007400F0`).
    obj.is_some_and(|o| {
        o.deploys_into.is_some()
            || if entity.category() == EntityCategory::Unit {
                o.is_simple_deployer || o.deploy_fire
            } else {
                o.deployer
            }
    })
    .then_some(Command::DeployMcv {
        entity_id: target.stable_id,
    })
}
