//! Clicks with buildings selected: cell and object orders, rally points, Ares
//! trench entry and the producers that take a rally point.

use super::{
    LeftRelease, OrderModifier, cell_guard_area_action_with, cursor_owner, object_click_release,
    projected_actor_can_order, self_click_command,
};
use crate::app::input::entity_pick::HoverTargetKindWithId;
use crate::app::types::HoverTargetKind;
use crate::map::entities::EntityCategory;
use crate::sim::command::Command;
use crate::sim::intern::InternedId;
use crate::sim::world::ControlProjection;

/// Building4436F0(1/6, cell): a Construction Yard must pass the repack
/// checks before preparing archive event1E or SELL16. An admitted click
/// prepares an archive, then SELL16 for an UndeploysInto type. A failed FNPC
/// still queues SELL and retains an earlier archive, except that a Construction
/// Yard archives its own Cell
/// ([`crate::sim::world::Simulation::factory_rally_cell_input`]). A
/// factory announces its rally click whether or not the search found a Cell
/// (`0x00443A7B..0x00443A89` jumps to the announcement at `0x00443A2B`).
///
/// The click loop clamps the Cell to the playfield first
/// ([`crate::sim::world::Simulation::clamped_click_cell`]), then asks each
/// building's own What_Action on it (`0x004AE974`,
/// [`crate::sim::world::Simulation::cell_move_action`]). Active_Click_With_Cell
/// has no arm for NoMove or for the Ctrl+Alt guard rung's 0x1A
/// (`0x0044372F..0x00443737`): a Construction Yard over a Cell it may not
/// stand on neither archives nor packs up, nor does an active building
/// without a rally line under `guard_area` ([`cell_guard_area_action`]).
/// A query the sim cannot answer (no map cells) keeps Move: VERA-internal, as
/// a live gamemd session always has a map.
pub(in crate::app::input) fn building_cell_orders(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &[u64],
    owner: InternedId,
    cell: (u16, u16),
    shift: bool,
    guard_area: bool,
) -> (Vec<Command>, bool) {
    let cell = sim.clamped_click_cell(cell);
    let query = crate::sim::world::CellActionQuery {
        mouse_over: false,
        shift,
    };
    let mut orders = Vec::new();
    let mut announce = false;
    let control = sim.control_projection();
    for id in selected_rally_producer_ids(sim, &control, rules, selected, owner) {
        let Some(entity) = sim.entities().get(id) else {
            continue;
        };
        let Some(object) = sim.interner().object_type(entity.type_ref(), rules) else {
            continue;
        };
        if guard_area && cell_guard_area_action_with(sim, &control, rules, id, Some(owner), cell) {
            continue;
        }
        let action = sim
            .cell_move_action_with(&control, owner, id, cell, query, rules)
            .unwrap_or(crate::sim::world::MoveAction::Move);
        if action != crate::sim::world::MoveAction::Move {
            continue;
        }
        // BuildingClass::Active_Click_With_Cell @ 0x004436F0:
        // 0x0044377A skips both orders even when the yard has a rally line.
        if object.construction_yard
            && !sim.can_redeploy_construction_yard_from_input(entity, Some(&control))
        {
            continue;
        }
        match sim.factory_rally_cell_input_with(&control, id, cell, rules) {
            Ok(found) => {
                if let Some((rx, ry)) = found {
                    orders.push(Command::SetRally {
                        rx,
                        ry,
                        producer_ids: vec![id],
                    });
                }
                // A missed search still reaches the announcement
                // (`0x00443977` -> `0x00443A7B` -> `0x00443A2B`).
                announce |= crate::app::match_runtime::eva_producers::rally_point_announces(object);
            }
            Err(error) => log::warn!("building cell input: {error}"),
        }
        if object.undeploys_into.is_some() {
            let sale = Command::SellBuilding { entity_id: id };
            if sim.can_issue_local_command_with(&control, &sale, rules) {
                orders.push(sale);
            }
        }
    }
    (orders, announce)
}

/// What a left click does for a selection of buildings only.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum BuildingClick {
    /// The ordinary selection click on the object under the cursor.
    Select,
    /// The click is consumed with no order.
    Nothing,
    /// The click dispatches these orders; `true` announces a new rally point.
    Orders(Vec<Command>, bool),
}

/// A left click for a selection of buildings only, after the attack
/// receivers have been taken out of `receivers`: a lone building's self
/// action, the release over an object ([`building_object_release`]), then
/// each building's own orders ([`building_click_orders`]). Over the player's
/// own or an allied object the cursor owner's What_Action decides the
/// release too ([`object_action_answers_friendly`]): Select or 0 selects an
/// object the player may select and has not, and nothing else happens.
/// Shift over an object reaches here only for the cursor owner's trench
/// Enter ([`super::queue::head_release`]); the caller hands any other to
/// the selection consumer first.
pub(super) fn building_selection_click(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    current_objects: &[u64],
    receivers: &[u64],
    owner: InternedId,
    hover: Option<&HoverTargetKindWithId>,
    terrain: Option<u64>,
    cell: (u16, u16),
    modifier: OrderModifier,
    force_move_key: bool,
) -> BuildingClick {
    let context_actions_enabled = matches!(
        modifier,
        OrderModifier::Normal | OrderModifier::Shift | OrderModifier::AttackMove
    );
    if context_actions_enabled
        // Building What_Action's self rows have no planning test.
        && let Some(command) = self_click_command(
            sim,
            rules,
            Some(overlay_registry),
            owner,
            current_objects,
            hover,
            false,
        )
    {
        return BuildingClick::Orders(vec![command], false);
    }
    if let Some(target) = hover.filter(|target| target.kind != HoverTargetKind::HiddenEnemy) {
        match building_object_release(
            sim,
            rules,
            overlay_registry,
            current_objects,
            receivers,
            target.stable_id,
            modifier == OrderModifier::ForceFire,
            force_move_key,
        ) {
            LeftRelease::Select => return BuildingClick::Select,
            LeftRelease::Nothing => return BuildingClick::Nothing,
            LeftRelease::Dispatch => {}
        }
    }
    let (orders, announce) = building_click_orders(
        sim,
        rules,
        receivers,
        owner,
        hover,
        terrain,
        cell,
        modifier,
        force_move_key,
    );
    // Over a Terrain object a building that does not answer Move answers 0,
    // which selects nothing (a tree is no object to select).
    if terrain.is_some() && orders.is_empty() {
        return BuildingClick::Nothing;
    }
    BuildingClick::Orders(orders, announce)
}

/// The selected buildings' own orders for a left click: over a visible
/// object or a Terrain object (`terrain`) ([`building_object_orders`]; an
/// enemy the player cannot see is no object, and its Cell takes the click)
/// or on a Cell ([`building_cell_orders`]). Ctrl+Alt over the player's own
/// or an allied object answers 0x1A, which has no Building ClickedAction
/// arm; a Terrain object is no ally, so there it answers Alt's Move
/// ([`object_key_action`]).
pub(super) fn building_click_orders(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &[u64],
    owner: InternedId,
    hover: Option<&HoverTargetKindWithId>,
    terrain: Option<u64>,
    cell: (u16, u16),
    modifier: OrderModifier,
    force_move_key: bool,
) -> (Vec<Command>, bool) {
    let guard_area = modifier == OrderModifier::GuardArea;
    match hover.filter(|target| target.kind != HoverTargetKind::HiddenEnemy) {
        Some(target)
            if guard_area
                && matches!(
                    target.kind,
                    HoverTargetKind::FriendlyUnit | HoverTargetKind::FriendlyStructure
                ) =>
        {
            (Vec::new(), false)
        }
        Some(target) => {
            building_object_orders(sim, rules, selected, target.stable_id, force_move_key)
        }
        None if let Some(terrain) = terrain => {
            building_object_orders(sim, rules, selected, terrain, force_move_key)
        }
        None => building_cell_orders(
            sim,
            rules,
            selected,
            owner,
            cell,
            modifier == OrderModifier::Shift,
            guard_area,
        ),
    }
}

/// Whether Techno What_Action over an object (`0x006FFEC0`) answers on its
/// force-move rungs for the selected building `id`: the force-move key held
/// (`0x0070000E..0x0070003D`), a human player's building, and one the player
/// may order (vt+0xA0). Over the player's own or an allied object Ctrl+Alt
/// answers 0x1A (`0x00700191..0x007001E7`), otherwise Move
/// (`0x007001E9..0x00700217`). Without them Building What_Action_OnObject
/// (`0x00447210`) is left with the attack rung or the Techno tail's Select
/// or 0 ([`attack_action`]), so a plain click over an object never rallies
/// or packs a building up: Building ClickedAction_Object (`0x00443410`)
/// sends neither for those.
fn building_moves_over_object(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    id: u64,
    force_move_key: bool,
) -> bool {
    force_move_key
        && projected_actor_can_order(sim, control, id)
        && sim.local_techno_controllable_with(control, id, rules)
}

/// The left release over an object for a selection of buildings only: the
/// action of the object that owns the cursor ([`cursor_owner`]). A force-move
/// rung dispatches ([`building_moves_over_object`]); any other answer is
/// [`object_click_release`]'s, so an unarmed factory or Construction Yard
/// selects the object as DisplayClass does for Select (`0x004ABDD3`).
fn building_object_release(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    current_objects: &[u64],
    receivers: &[u64],
    target_id: u64,
    forced: bool,
    force_move_key: bool,
) -> LeftRelease {
    let control = sim.control_projection();
    if cursor_owner(sim, rules, current_objects, target_id).is_some_and(|best| {
        receivers.contains(&best)
            && (building_moves_over_object(sim, &control, rules, best, force_move_key)
                // Ares 3.0 100171B0 (hook 0x44725F): the trench Enter.
                || ares_trench_enter_with(sim, &control, rules, best, target_id))
    }) {
        return LeftRelease::Dispatch;
    }
    object_click_release(
        sim,
        rules,
        current_objects,
        receivers,
        target_id,
        overlay_registry,
        forced,
    )
}

/// Building ClickedAction_Object (`0x00443410`) for each selected building
/// whose action over `target` is Move: the force-move rung
/// ([`building_moves_over_object`]) and the building's own factory rungs
/// ([`Simulation::building_object_move_action`]); a NoMove sends nothing. An
/// Infantry, Unit or Aircraft factory (Type+0xEB8, `0x0044361B..0x0044363A`)
/// sets its rally point from the Cell under the object's GetCoords
/// (`0x0044368D..0x004436D8`, Building443860, [`Simulation::factory_rally_cell_input`]).
/// Any other building, a Construction Yard among them, archives the object
/// itself (event 0x1E, `0x0044363C..0x00443688`, [`Command::ArchiveObject`])
/// and never packs up: a depot then releases repaired units toward it,
/// Cloning Vats hand it to their clones, and a yard's later pack-up that does
/// not archive again sends its MCV or Slave Miner to the object.
///
/// [`Simulation::factory_rally_cell_input`]: crate::sim::world::Simulation::factory_rally_cell_input
/// [`Simulation::building_object_move_action`]: crate::sim::world::Simulation::building_object_move_action
fn building_object_orders(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &[u64],
    target: u64,
    force_move_key: bool,
) -> (Vec<Command>, bool) {
    use crate::rules::object_type::FactoryType;
    let Some(cell) = sim.object_coords_cell(target) else {
        return (Vec::new(), false);
    };
    let mut orders = Vec::new();
    let mut announce = false;
    let control = sim.control_projection();
    for &id in selected {
        let Some(object) = sim
            .entities()
            .get(id)
            .filter(|building| building.category() == EntityCategory::Structure)
            .and_then(|building| sim.interner().object_type(building.type_ref(), rules))
        else {
            continue;
        };
        // Ares 3.0 10016F60 (hook 0x443414): a building whose action over
        // the building is the trench Enter (100171B0, hook 0x44725F) raises
        // TrenchRedirectClick (0x60, RaiseTrenchRedirectClick 1006A170)
        // with that building's Cell, and sends nothing else.
        if ares_trench_enter_with(sim, &control, rules, id, target) {
            if let Some(record) = sim.encode_ares_trench_redirect_record(id, cell) {
                orders.push(Command::SynchronizedRecord { record });
            }
            continue;
        }
        if !building_moves_over_object(sim, &control, rules, id, force_move_key) {
            continue;
        }
        if !matches!(
            object.factory_type(),
            Some(FactoryType::InfantryType | FactoryType::UnitType | FactoryType::AircraftType)
        ) {
            orders.push(Command::ArchiveObject {
                entity_id: id,
                target_id: target,
            });
            continue;
        }
        match sim.building_object_move_action(id, target, rules) {
            Ok(crate::sim::world::MoveAction::Move) => {}
            Ok(_) => continue,
            Err(error) => {
                log::warn!("building object input: {error}");
                continue;
            }
        }
        match sim.factory_rally_cell_input_with(&control, id, cell, rules) {
            Ok(found) => {
                if let Some((rx, ry)) = found {
                    orders.push(Command::SetRally {
                        rx,
                        ry,
                        producer_ids: vec![id],
                    });
                }
                // A missed search still reaches the announcement
                // (`0x00443977` -> `0x00443A7B` -> `0x00443A2B`).
                announce |= crate::app::match_runtime::eva_producers::rally_point_announces(object);
            }
            Err(error) => log::warn!("building object input: {error}"),
        }
    }
    (orders, announce)
}

/// Ares 3.0 BuildingClass_GetActionOnObject_TargetABuilding (`100171B0`,
/// hook `0x44725F`): the building `id` answers Enter over the building
/// `target` when its occupants may traverse there (canTraverseTo
/// `10011690`), while Ares plays. The cursor asks it too.
pub(in crate::app::input) fn ares_trench_enter(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    id: u64,
    target: u64,
) -> bool {
    ares_trench_enter_with(sim, &sim.control_projection(), rules, id, target)
}

fn ares_trench_enter_with(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    id: u64,
    target: u64,
) -> bool {
    if !rules.ares() || !sim.ares_can_traverse_to(id, target, rules) {
        return false;
    }
    if !sim.session().has_custom_control_grants() {
        return true;
    }
    sim.object_coords_cell(target)
        .and_then(|cell| sim.encode_ares_trench_redirect_record(id, cell))
        .is_some_and(|record| {
            sim.can_issue_local_command_with(
                control,
                &Command::SynchronizedRecord { record },
                rules,
            )
        })
}

pub(super) fn selected_rally_producer_ids(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    selected_ids: &[u64],
    owner: InternedId,
) -> Vec<u64> {
    let mut seen = std::collections::HashSet::with_capacity(selected_ids.len());
    selected_ids
        .iter()
        .copied()
        .filter(|&stable_id| {
            seen.insert(stable_id)
                && sim.local_takes_rally_point_with(control, stable_id, owner, rules)
        })
        .collect()
}
