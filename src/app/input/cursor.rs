//! Cursor feedback analysis and software cursor frame selection.
//!
//! Determines what cursor state to show based on hover target, selection,
//! and game mode. Split from `presentation::ui_overlays` for file-size limits.

use std::time::Instant;

use crate::app::AppState;
use crate::app::input::commands::preferred_local_owner_name;
use crate::app::presentation::instances::CellVisibilityState;
use crate::app::types::{
    CursorFeedbackKind, CursorId, HoverTargetKind, ScrollDir, SoftwareCursorFrame,
    SoftwareCursorSequence,
};
use crate::sim::combat;

pub(crate) fn current_cursor_feedback_kind(state: &AppState) -> Option<CursorFeedbackKind> {
    // A right-drag pan owns the cursor for as long as it owns the camera, and it
    // is tested first because native gates the cursor write and the edge-scroll
    // block inside the same `capture == 0` else-arm of
    // `ScrollClass__UpdateMouseScrolling` 0x00692F30 — with the capture held,
    // neither the action cursor nor the edge arrow is reached.
    //
    // DRIFT, pre-existing and recorded not fixed: `edge_scroll_cursor_state`
    // never consults `captured`, so the edge arrow can still appear during a
    // captured gesture the pan branch declines — a band-box drag pushed into the
    // outer window pixel, or the part of a right drag before the threshold is
    // crossed. Trigger: dragging into a screen border with a button held.
    // Player effect: a scroll arrow gamemd never shows; the camera does not
    // actually move, because the motion path IS capture-gated. Frequency:
    // occasional, and transient — it clears on release. Downstream risk: none;
    // the fix is a `captured` check in that one helper.
    if let Some(blocked) = crate::app::input::camera::right_drag_pan_cursor_state(state) {
        return Some(match blocked {
            Some(dir) => CursorFeedbackKind::PanBlocked(dir),
            None => CursorFeedbackKind::Pan,
        });
    }
    // The active band is the outermost pixel of the whole window, including
    // the sidebar. It wins even when that pixel overlaps a sidebar/minimap hit.
    if let Some((dir, blocked)) = crate::app::input::camera::edge_scroll_cursor_state(state) {
        return Some(if blocked {
            CursorFeedbackKind::ScrollBlocked(dir)
        } else {
            CursorFeedbackKind::Scroll(dir)
        });
    }
    if state.match_state.input.minimap_dragging || is_cursor_over_minimap(state) {
        // Show the minimap-specific Move cursor when hovering over the minimap
        // (reference §7.4 — MiniFrame/MiniCount for the Move cursor = frames 42–51).
        return Some(CursorFeedbackKind::MinimapMove);
    }
    if current_sidebar_view_hit(state) {
        return None;
    }
    // Superweapon targeting cursor takes precedence over building placement.
    // Sidebar/minimap hits are already short-circuited above, so the SW reticle
    // only renders on the tactical map.
    if let Some(section) = state.armed_super_weapon_type() {
        let cursor_id = state
            .rules()
            .and_then(|r| r.super_weapon(section))
            .and_then(|sw| sw.action.as_deref())
            .and_then(super_weapon_cursor_id)
            .unwrap_or(CursorId::Default);
        return Some(CursorFeedbackKind::SuperWeaponTarget(cursor_id));
    }
    if let Some(preview) = state.match_state.input.building_placement_preview.as_ref() {
        return Some(if preview.valid {
            CursorFeedbackKind::PlaceValid
        } else {
            CursorFeedbackKind::PlaceInvalid
        });
    }
    if state.armed_building_type().is_some() {
        return Some(CursorFeedbackKind::Invalid);
    }
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return None;
    };
    // Repair / Sell cursor modes take over the tactical map regardless of
    // selection — the wrench/dollar shows over own buildings, no-repair/no-sell
    // elsewhere. Placed before the empty-selection early return below because
    // gamemd shows these cursors even with nothing selected.
    let (repair_mode, sell_mode) =
        crate::app::presentation::sidebar_render::current_sidebar_view(state)
            .map(|view| (view.repair_button.active, view.sell_button.active))
            .unwrap_or_default();
    if repair_mode || sell_mode {
        let repair = repair_mode;
        let (wx, wy) = crate::app::match_runtime::sim_tick::screen_point_to_world(
            state,
            state.match_state.input.cursor_x,
            state.match_state.input.cursor_y,
        );
        let valid = if repair {
            crate::app::input::commands::own_repairable_building_under_point(state, wx, wy)
                .is_some()
        } else {
            crate::app::input::commands::own_sellable_building_under_point(state, wx, wy).is_some()
                || crate::app::input::commands::sell_wall_under_cursor_is_eligible(state)
        };
        return Some(if repair {
            CursorFeedbackKind::RepairMode(valid)
        } else {
            CursorFeedbackKind::SellMode(valid)
        });
    }
    let selected = crate::app::input::dispatch::selected_stable_ids_in_order(
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    if selected.is_empty() {
        return None;
    }
    let owner = preferred_local_owner_name(state).unwrap_or_else(|| "Americans".to_string());
    let (world_x, world_y) = crate::app::match_runtime::sim_tick::screen_point_to_world(
        state,
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    let (hover_rx, hover_ry) = crate::app::match_runtime::sim_tick::screen_point_to_world_cell(
        state,
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    let owner_id = sim.interner.get(&owner);
    if crate::app::presentation::instances::cell_visibility_for_local_owner(
        owner_id,
        Some(&sim.fog),
        hover_rx,
        hover_ry,
        state.match_state.sandbox_full_visibility,
    ) != CellVisibilityState::Visible
    {
        // Over shrouded/fogged cells the player can still issue move orders,
        // so show the queued-order-mode cursor (Move / AttackMove)
        // instead of reverting to the default arrow.
        return Some(match state.match_state.input.queued_order_mode {
            crate::app::presentation::render::OrderMode::Move => CursorFeedbackKind::Move,
            crate::app::presentation::render::OrderMode::AttackMove => {
                CursorFeedbackKind::AttackMove
            }
        });
    }
    let modifier = crate::app::input::context_order::resolve_order_modifiers(
        crate::app::input::dispatch::is_ctrl_held(state),
        crate::app::input::dispatch::is_shift_held(state),
        crate::app::input::dispatch::is_alt_held(state),
    );
    let hover = crate::app::input::entity_pick::hover_target_at_point(
        sim,
        world_x,
        world_y,
        &owner,
        state.match_state.sandbox_full_visibility,
        state.rules(),
        &state.height_map(),
        crate::app::match_runtime::sim_tick::tactical_bridge_cells(sim),
    );
    // gamemd's DetermineAction resolves ONE object for the whole selection and
    // shows that object's action, for the cell branch as well as the object
    // branch. Resolve it before the split so every branch below reads the same
    // object instead of taking `.any()` over the selection.
    let action_target = hover
        .as_ref()
        .and_then(|h| sim.entities().get(h.stable_id))
        .map_or(ActionDistanceTarget::CellCentre(hover_rx, hover_ry), |e| {
            ActionDistanceTarget::Object(e.stable_id())
        });
    let best_id = select_best_for_action(sim, &selected, action_target, state.rules());

    // Force-fire override: with Ctrl held the cell path takes the attack branch
    // over allies, own units and empty ground. Ctrl+Shift is attack-move and
    // Ctrl+Alt is guard area — neither force-fires — so this reads the resolved
    // modifier verb rather than raw Ctrl. The armed test runs on the one
    // resolved object, matching gamemd's single-object dispatch.
    //
    // The armed predicate is the native one: `TechnoClass::What_Action_OnCell
    // @ 0x00700600` inlines `TechnoClass::Is_Armed` at `0x007008BD` and skips
    // every ACTION_ATTACK arm when it fails. It consults ONE weapon slot, so
    // `Primary=`/`Secondary=` is the wrong test for a `TurretCount>0` type —
    // it hid the force-fire cursor for the Prism Tank and the Gattling Cannon.
    if modifier == crate::app::input::context_order::OrderModifier::ForceFire {
        if let Some(kind) = hover
            .as_ref()
            .and_then(|hover| forced_bomb_feedback(sim, &selected, best_id, hover, state.rules()))
        {
            return Some(kind);
        }
        let best_is_armed = best_id.is_some_and(|id| {
            sim.entities().get(id).is_some_and(|e| {
                let type_str = sim.interner.resolve(e.type_ref());
                state
                    .rules()
                    .and_then(|r| r.object(type_str))
                    .is_some_and(|obj| crate::sim::combat::combat_weapon::is_armed(e, obj))
            })
        });
        if best_is_armed {
            // EnemyUnit is the standard attack-reticle cursor; reuse it for
            // force-fire over allies/own/empty. (Exact mouse SHP frame for
            // gamemd's distinct force-fire action is unverified — cosmetic-only
            // follow-up; tracked in the design doc.)
            return Some(CursorFeedbackKind::EnemyUnit);
        }
    }
    if let Some(hover) = hover.as_ref() {
        let kind = capability_cursor_for_hover(
            sim,
            &selected,
            best_id,
            hover,
            state.rules(),
            state.overlay_registry(),
        );
        return Some(kind);
    }

    // No object under the cursor. gamemd runs the full What_Action_OnCell ladder
    // for the resolved object and answers Move or No-Move, so an unreachable or
    // blocked destination shows the barred cursor instead of the move cursor.
    let cell_action = what_action_on_cell(
        sim,
        best_id,
        (hover_rx, hover_ry),
        sim.path_grid(),
        modifier,
    );
    if cell_action == CellAction::NoMove {
        return Some(CursorFeedbackKind::Invalid);
    }

    // Ore/gem harvest hangs off the cell action: UnitClass only substitutes the
    // harvest action when the base action came back Move, and it tests the one
    // resolved object's harvester flags, not the whole selection.
    let has_ore = match (
        sim.overlay_grid.as_ref(),
        state.overlay_registry(),
        state.rules(),
    ) {
        (Some(grid), Some(registry), Some(rules)) if !rules.tiberium_types.is_empty() => {
            crate::sim::tiberium::tiberium_cell_view(
                grid,
                registry,
                &rules.tiberium_types,
                (hover_rx, hover_ry),
            )
            .is_some()
        }
        _ => false,
    };
    if has_ore
        && best_id.is_some_and(|id| sim.entities().get(id).is_some_and(|e| e.miner.is_some()))
    {
        return Some(CursorFeedbackKind::Harvest);
    }
    Some(match state.match_state.input.queued_order_mode {
        crate::app::presentation::render::OrderMode::Move => CursorFeedbackKind::Move,
        crate::app::presentation::render::OrderMode::AttackMove => CursorFeedbackKind::AttackMove,
    })
}

/// gamemd's `What_Action_OnCell` outcome for an empty cell: action 1 (Move) or
/// action 2 (No-Move).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellAction {
    Move,
    NoMove,
}

/// Resolve the empty-cell action for the object that owns the cursor.
///
/// gamemd's ladder, in order: outside the playfield answers No-Move; Shift
/// answers Move and returns *before* the occupancy probe; Alt with the
/// force-move capability answers Move; a unit that cannot accept move orders
/// answers No-Move; otherwise the cell occupancy probe decides Move vs No-Move.
///
/// VERA implements the playfield test, all three modifier short-circuits, and
/// the terrain slice of the cell-entry probe through the same
/// `evaluate_can_enter_cell` the mover itself asks, on both ground and bridge
/// planes. What remains unmodelled is recorded rather than hidden:
///
/// * The probe's **occupancy** half. Native's `Can_Enter_Cell` also weighs the
///   cell's occupants and returns a cost class, so a cell blocked only by a unit
///   or a building still shows the move cursor here.
/// * The rung at 0x00700B84-0x00700BCE, which is NOT a gap: after the probe
///   rejects a cell, an object whose RTTI is 6 with a non-null type `+0x408`
///   whose `+0x5EC` is set still returns ACTION_MOVE at 0x00700BC5. RTTI 6 is
///   `BuildingClass` (vtable 0x007E3EBC, COL 0x007FC360, slot `+0x2C` at
///   0x00459EC0 returning 6), and the structure arm above already answers Move
///   before reaching the probe at all. Recorded so a later session does not add
///   a gate for it.
/// * A destroyed LOW bridge keeps its `bridge_walkable` bit from the resolved
///   cell (`PathGrid::from_resolved_terrain_with_bridges` gates only the
///   structural branch on intactness), so the walkable-deck gate closes only the
///   structural half of the destroyed-bridge class.
/// * The `vtable+0xA0` rung at 0x00700B43 — "this object cannot accept move
///   orders at all" — whose `JZ` lands on the ACTION_NOMOVE return at
///   0x00700C17. VERA has no equivalent test, so an object gamemd refuses
///   outright still gets the terrain answer.
/// * The **order path is looser than this cursor**. `nearest_reachable_goal`
///   still admits a goal from the coarse `is_any_layer_walkable` bit and, on
///   failure, retargets rather than refusing, so a click on open water still
///   walks the unit to the shoreline while the cursor over it is now barred.
///   Reconciling the two belongs to the move order's own row.
///
/// Aircraft and subterranean movers answer Move for any in-playfield cell.
///
/// With no path grid or no resolved object the cursor keeps its previous
/// behaviour and reports Move: VERA-internal, gamemd equivalent UNCHECKED (a
/// live gamemd session always has a map and a selection here).
fn what_action_on_cell(
    sim: &crate::sim::world::Simulation,
    best_id: Option<u64>,
    cell: (u16, u16),
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
    modifier: crate::app::input::context_order::OrderModifier,
) -> CellAction {
    use crate::sim::movement::locomotor::MovementLayer;

    let Some(grid) = path_grid else {
        return CellAction::Move;
    };
    if cell.0 >= grid.width() || cell.1 >= grid.height() {
        return CellAction::NoMove;
    }
    let Some(entity) = best_id.and_then(|id| sim.entities().get(id)) else {
        return CellAction::Move;
    };
    // Only mobile objects run this ladder; a selected structure takes a
    // different override in gamemd, and VERA answers a rally-point click there.
    if entity.category == crate::map::entities::EntityCategory::Structure {
        return CellAction::Move;
    }
    // Native returns ACTION_MOVE before the probe for all three: the Shift latch,
    // the Alt force-move capability, and the force-fire flag `param_3` at
    // 0x00700B51. Force-fire only arrives here at all when the resolved object
    // is unarmed — an armed one already took the attack-reticle branch above —
    // and gamemd still answers Move for it rather than running the probe.
    if matches!(
        modifier,
        crate::app::input::context_order::OrderModifier::Queue
            | crate::app::input::context_order::OrderModifier::ForceMove
            | crate::app::input::context_order::OrderModifier::ForceFire
    ) {
        return CellAction::Move;
    }
    match entity.movement_layer_or_ground() {
        MovementLayer::Air | MovementLayer::Underground => CellAction::Move,
        MovementLayer::Ground | MovementLayer::Bridge => {
            // Native asks the mover's own cell-entry virtual here, not a coarse
            // walkability bit: the tail of What_Action_OnCell 0x00700600 calls
            // vtable +0x1AC on the target cell and answers ACTION_NOMOVE (2)
            // when the result is > 1. Asking the same predicate the move order
            // asks is the point — otherwise the cursor promises a move the unit
            // then refuses, which is what a PathGrid-walkable water cell did to
            // every ordinary ground mover.
            //
            // The callsite pushes `(cell, -1, -1, 0, 1)` at 0x00700B5F, and in
            // `UnitClass::Can_Enter_Cell` 0x0073F0A0 the LEVEL argument is the
            // second of those two (guarded by `param_4 != -1`); the first is a
            // facing, feeding `param_3 - 4U & 7` into the step-by-direction
            // lookup. Passing -1 for the level is what makes the TARGET CELL
            // select the plane rather than the mover's current layer, and that
            // distinction is the whole bridge case: a tank on open ground
            // hovering a deck cell, and a tank already on the deck hovering open
            // ground, both have to answer Move. So the probe runs on both planes.
            //
            // The bridge plane additionally requires a walkable DECK, not merely
            // the structural bit. `PathGrid::from_resolved_terrain_with_bridges`
            // keeps `bridge_structural` set after a span collapses and clears
            // only `bridge_walkable`, and the bridge leaf answers Clear off the
            // structural bit alone — so without this the barred cursor would
            // disappear from a blown span, which is where a player most needs it.
            //
            // Only one probe, not native's two. The second is gated on
            // `Type+0xD2C`, the derived `MovementZone == Subterannean` flag
            // (`CMP EAX,0x6; SETZ` at 0x0071607E-0x0071608A), which is zero for
            // every stock YR type — so native always takes the `return 2` above
            // it and the second probe is Tiberian Sun legacy that stock cannot
            // reach.
            let speed_type = entity.locomotor.as_ref().map(|l| l.speed_type);
            let terrain_costs = speed_type.and_then(|st| sim.terrain_costs.get(&st));
            let admits = |layer| {
                crate::sim::pathfinding::cell_entry::evaluate_can_enter_cell(
                    crate::sim::pathfinding::cell_entry::CanEnterCellContext {
                        wall: None,
                        target: cell,
                        terrain_layer: layer,
                        movement_zone: entity.locomotor.as_ref().map(|l| l.movement_zone),
                        speed_type,
                        path_grid: Some(grid),
                        resolved_terrain: sim.resolved_terrain.as_ref(),
                        terrain_costs,
                        bypass_grid: false,
                        is_infantry: entity.category
                            == crate::map::entities::EntityCategory::Infantry,
                        mover_is_crusher: crate::sim::movement::bump_crush::CrushCapability::of(
                            entity,
                        )
                        .wall_arm_crusher(),
                    },
                )
                .is_clear()
            };
            let bridge_deck_open = grid.is_walkable_on_layer(cell.0, cell.1, MovementLayer::Bridge);
            if admits(MovementLayer::Ground) || (bridge_deck_open && admits(MovementLayer::Bridge))
            {
                CellAction::Move
            } else {
                CellAction::NoMove
            }
        }
    }
}

/// Determine the cursor feedback kind for a hover target, checking ObjectType
/// capability flags from rules.ini before falling back to the generic attack/select logic.
///
/// The original engine picks a single "best" selected unit via
/// `SelectBestObjectForAction` (priority: armed mobile > unarmed mobile >
/// immobile; ties broken by distance to target) and uses that unit's
/// `What_Action_OnObject` to determine the cursor for the entire group.
///
/// Priority (highest first):
/// 1. Deployer self-hover: the selected object itself has a deploy capability.
/// 2. SabotageCursor: selected unit has SabotageCursor=yes hovering an enemy structure.
/// 3. Engineer capturing: selected Engineer hovering capturable enemy building.
/// 4. Engineer repairing: selected Engineer hovering damaged friendly building.
/// 5. Infantry boarding: selected infantry hovering friendly transport (Passengers>0).
/// 6. Infantry garrisoning: selected Occupier infantry hovering friendly CanBeOccupied building.
/// 7. AttackCursorOnFriendlies: selected unit attacks friendlies, treat as attack target.
/// 8. Harvester docking: selected miner hovering friendly refinery (gamemd action 0x1A).
/// 9. Generic friendly/enemy/in-range/out-of-range fallback.
pub(super) fn capability_cursor_for_hover(
    sim: &crate::sim::world::Simulation,
    selected: &[u64],
    best_id: Option<u64>,
    hover: &crate::app::input::entity_pick::HoverTargetKindWithId,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> CursorFeedbackKind {
    use crate::map::entities::EntityCategory;

    let hovered_entity = sim.entities().get(hover.stable_id);
    let hovered_obj = rules
        .and_then(|r| hovered_entity.and_then(|e| r.object(sim.interner.resolve(e.type_ref()))));

    // 1. Deployer self-hover — the cursor is over the selected unit itself.
    //    Show the deploy cursor for units with Deployer=yes (e.g. GGI, Guardian GI)
    //    OR units with DeploysInto= (MCV → ConYard) or IsSimpleDeployer=yes
    //    (Siege Chopper). Each shows the deploy cursor over itself.
    //    gamemd gates its self-click actions on a selection of exactly one.
    if selected.len() == 1 && selected[0] == hover.stable_id {
        let entity = sim.entities().get(selected[0]);
        let obj =
            entity.and_then(|e| rules.and_then(|r| r.object(sim.interner.resolve(e.type_ref()))));
        if let (Some(entity), Some(obj)) = (entity, obj) {
            if super::context_order::is_simple_deploy_unit(entity, obj) {
                return if rules.is_some_and(|rules| sim.can_simple_deploy(hover.stable_id, rules)) {
                    CursorFeedbackKind::Deploy
                } else {
                    CursorFeedbackKind::NoDeploy
                };
            }
            if obj.deployer || obj.deploys_into.is_some() {
                // DRIFT, recorded not fixed: this branch does not see the order
                // modifier, so Alt over a deployer's own body still shows the
                // deploy cursor while the click issues a move —
                // native resolves Alt to action 1 at 0x00700B1A and would show
                // the move cursor. Trigger: holding Alt over the selected
                // deployer itself. Player effect: the cursor promises a deploy
                // and the unit moves instead. Frequency: rare — it needs Alt
                // held over exactly the one selected deployable. Downstream
                // risk: none; the fix is to thread the resolved modifier into
                // this helper, which changes no sim state.
                return CursorFeedbackKind::Deploy;
            }
        }
        // 1b. Garrisoned building self-hover — show deploy cursor to unload occupants.
        if let Some(entity) = entity {
            if entity.category == EntityCategory::Structure {
                if let Some(obj) = obj {
                    if obj.can_be_occupied {
                        let has_occupants =
                            entity.passenger_role.cargo().is_some_and(|c| !c.is_empty());
                        if has_occupants {
                            return CursorFeedbackKind::Deploy;
                        }
                    }
                }
                // Own occupied tank bunker self-hover → eject (deploy cursor).
                if entity.bunker_occupant.is_some() {
                    return CursorFeedbackKind::Deploy;
                }
                if rules.is_some_and(|rules| {
                    sim.should_show_undeploy_building_command(hover.stable_id, rules)
                }) {
                    return CursorFeedbackKind::Deploy;
                }
            }
        }
    }

    // The caller already resolved the single object whose action drives the
    // cursor for the whole selection, matching gamemd's DetermineAction. A
    // single selected object over itself takes the self action (4,
    // `TechnoClass::What_Action_OnObject`), never IvanBomb.
    let single_self = selected.len() == 1 && selected[0] == hover.stable_id;
    let best_is_ivan = !single_self
        && best_id
            .and_then(|id| sim.entities().get(id))
            .and_then(|e| rules.and_then(|r| r.object(sim.interner.resolve(e.type_ref()))))
            .is_some_and(|obj| obj.ivan);
    if let Some(best_id) = best_id {
        if let (Some(sel_entity), Some(sel_obj)) = (
            sim.entities().get(best_id),
            sim.entities()
                .get(best_id)
                .and_then(|e| rules.and_then(|r| r.object(sim.interner.resolve(e.type_ref())))),
        ) {
            // `InfantryClass::What_Action_OnObject @ 0x0051E3B0`, right after
            // the base action (0x0051E462..0x0051E49B): an Engineer of the
            // local player over any object carrying a bomb that player sees
            // (`+0x38`, `+0x68`) — own, allied or enemy — offers DisarmBomb.
            if sel_entity.category == EntityCategory::Infantry
                && sel_obj.engineer
                && hovered_entity.is_some_and(|e| e.bomb.is_some())
                && sim.bomb_seen_by(hover.stable_id, sel_entity.owner())
            {
                return CursorFeedbackKind::DisarmBomb;
            }

            // Object51E3B0 hut arm returns action29/32 unconditionally;
            // Display4AAE90 selects Repair/NoRepair, independently of owner.
            if matches!(
                hover.kind,
                HoverTargetKind::FriendlyStructure | HoverTargetKind::EnemyStructure
            ) && let Some(action) = rules.and_then(|rules| {
                sim.engineer_building_action(sel_entity.stable_id(), hover.stable_id, rules)
            }) {
                return match action {
                    crate::sim::world::EngineerBuildingAction::Repair(valid) => {
                        CursorFeedbackKind::RepairAction(valid)
                    }
                    crate::sim::world::EngineerBuildingAction::Damage => {
                        CursorFeedbackKind::EngineerDamage
                    }
                    crate::sim::world::EngineerBuildingAction::Capture
                    | crate::sim::world::EngineerBuildingAction::EnterHospital
                    | crate::sim::world::EngineerBuildingAction::EnterGrinder => {
                        CursorFeedbackKind::Enter
                    }
                };
            }

            // 2. C4 plant: SEAL / Tanya / Psi-Corp Trooper hovering an enemy
            //    structure with CanC4=yes, not InvisibleInGame, not iron-curtained.
            //    SabotageCursor flag remains in the data model (parsed in
            //    object_type.rs) for modder weapon-overlay use, but cursor
            //    logic is now driven by C4=yes — matches gamemd action 0x10.
            if sel_obj.c4
                && matches!(hover.kind, HoverTargetKind::EnemyStructure)
                && hovered_obj.map_or(false, |o| o.can_c4 && !o.invisible_in_game)
                && !hovered_entity.is_some_and(|e| {
                    crate::sim::superweapon::invulnerability::is_invulnerable(
                        e.invulnerability.as_ref(),
                        sim.session.tick as u32,
                    )
                })
            {
                return CursorFeedbackKind::Demolish;
            }

            let is_infantry = sel_entity.category == EntityCategory::Infantry;

            // 5. Infantry boarding a friendly transport (Passengers > 0).
            if is_infantry && matches!(hover.kind, HoverTargetKind::FriendlyUnit) {
                if hovered_obj.map_or(false, |o| o.passengers > 0) {
                    return CursorFeedbackKind::Enter;
                }
            }

            // 6. Infantry garrisoning uses the shared CanBeOccupiedBy-equivalent predicate.
            //    garrisonable — only show Enter for those, not actual enemy-player buildings.
            if is_infantry {
                if let Some(rules) = rules {
                    if crate::sim::passenger::can_entity_enter_garrison(
                        sim,
                        rules,
                        best_id,
                        hover.stable_id,
                    ) {
                        return CursorFeedbackKind::Enter;
                    }
                }
            }

            // 7. AttackCursorOnFriendlies — treat friendly targets as attack targets.
            //    Not over the lone selected object itself, which takes its self
            //    action first.
            if sel_obj.attack_cursor_on_friendlies
                && !single_self
                && matches!(
                    hover.kind,
                    HoverTargetKind::FriendlyUnit | HoverTargetKind::FriendlyStructure
                )
            {
                let in_range = resolved_unit_in_range(
                    sim,
                    best_id,
                    hover.stable_id,
                    rules,
                    sim.resolved_terrain.as_ref(),
                    overlay_registry,
                );
                let kind = if in_range {
                    if hover.kind == HoverTargetKind::FriendlyUnit {
                        CursorFeedbackKind::EnemyUnit
                    } else {
                        CursorFeedbackKind::EnemyStructure
                    }
                } else {
                    CursorFeedbackKind::EnemyOutOfRange
                };
                return ivan_attack_feedback(
                    kind,
                    best_is_ivan,
                    || click_refused(sim, rules, Some(best_id), hover.stable_id, overlay_registry),
                    hovered_entity,
                    hovered_obj,
                );
            }

            // 8. Harvester docking — selected miner hovering own/ally refinery.
            //    Matches gamemd action 0x1A (TechnoClass dock branch). Alliance
            //    gate comes from HoverTargetKind::FriendlyStructure; refinery
            //    detection from RuleSet::is_refinery_type (same key used by
            //    the click pipeline in `input::context_order`).
            if sel_entity.miner.is_some()
                && matches!(hover.kind, HoverTargetKind::FriendlyStructure)
                && hovered_entity.is_some_and(|e| {
                    rules.is_some_and(|r| r.is_refinery_type(sim.interner.resolve(e.type_ref())))
                })
            {
                return CursorFeedbackKind::Enter;
            }

            // Service depot — a damaged own vehicle over an own UnitRepair
            // building shows the enter/dock cursor (the click issues
            // RepairAtDepot; see `input::context_order`).
            if sel_entity.category == EntityCategory::Unit
                && sel_entity.health.current < sel_obj.strength
                && matches!(hover.kind, HoverTargetKind::FriendlyStructure)
                && hovered_obj.map_or(false, |o| o.unit_repair)
            {
                return CursorFeedbackKind::Enter;
            }

            // Tank bunker — an own bunkerable vehicle over an own EMPTY tank
            // bunker shows the enter cursor (the click issues EnterBunker).
            if matches!(hover.kind, HoverTargetKind::FriendlyStructure)
                && hovered_entity
                    .is_some_and(|he| he.bunker_runtime.is_some() && he.bunker_occupant.is_none())
                && rules.is_some_and(|r| {
                    crate::sim::docking::bunker_link::can_auto_deploy_here(sim, best_id, r)
                })
            {
                return CursorFeedbackKind::Enter;
            }
        }
    }

    // 9. Generic fallback.
    match hover.kind {
        HoverTargetKind::FriendlyUnit => CursorFeedbackKind::FriendlyUnit,
        HoverTargetKind::FriendlyStructure => CursorFeedbackKind::FriendlyStructure,
        HoverTargetKind::EnemyUnit | HoverTargetKind::EnemyStructure => {
            let in_range = best_id.is_some_and(|id| {
                resolved_unit_in_range(
                    sim,
                    id,
                    hover.stable_id,
                    rules,
                    sim.resolved_terrain.as_ref(),
                    overlay_registry,
                )
            });
            let kind = if in_range {
                if hover.kind == HoverTargetKind::EnemyUnit {
                    CursorFeedbackKind::EnemyUnit
                } else {
                    CursorFeedbackKind::EnemyStructure
                }
            } else {
                CursorFeedbackKind::EnemyOutOfRange
            };
            ivan_attack_feedback(
                kind,
                best_is_ivan,
                || click_refused(sim, rules, best_id, hover.stable_id, overlay_registry),
                hovered_entity,
                hovered_obj,
            )
        }
        HoverTargetKind::HiddenEnemy => CursorFeedbackKind::Invalid,
    }
}

/// `InfantryClass::What_Action_OnObject` (`0x0051EB24..0x0051EB7E`) for a
/// Crazy Ivan (`Ivan=`) whose action is Attack ([`ivan_bomb_action`]).
/// Unforced, the base action stays Attack only while the Ivan's GetFireError
/// for the target is not ILLEGAL (`refused`, [`click_refused`]), and a bombed
/// target is ILLEGAL to him at any distance (`0x006FCBAD`). An ILLEGAL action
/// falls back to None over another house's object and to Select over his own
/// (`TechnoClass::What_Action_OnObject`, `0x0070056C..0x007005E9`).
///
/// RESIDUAL: VERA shows Select for both; over another house's object retail
/// shows the plain arrow of None.
fn ivan_attack_feedback(
    kind: CursorFeedbackKind,
    selector_is_ivan: bool,
    refused: impl FnOnce() -> bool,
    target: Option<&crate::sim::game_entity::GameEntity>,
    target_obj: Option<&crate::rules::object_type::ObjectType>,
) -> CursorFeedbackKind {
    let attack = matches!(
        kind,
        CursorFeedbackKind::EnemyUnit
            | CursorFeedbackKind::EnemyStructure
            | CursorFeedbackKind::EnemyOutOfRange
    );
    if !selector_is_ivan || !attack {
        return kind;
    }
    if refused() {
        return CursorFeedbackKind::FriendlyUnit;
    }
    ivan_bomb_action(target, target_obj)
}

/// The Crazy Ivan's arm itself (`0x0051EB24`): IvanBomb on a `Bombable=`
/// target without a bomb (`+0x38`), NoIvanBomb (NoMove's row) otherwise.
fn ivan_bomb_action(
    target: Option<&crate::sim::game_entity::GameEntity>,
    target_obj: Option<&crate::rules::object_type::ObjectType>,
) -> CursorFeedbackKind {
    if target.is_some_and(|target| target.bomb.is_some()) {
        CursorFeedbackKind::Invalid
    } else if target_obj.is_some_and(|obj| obj.bombable) {
        CursorFeedbackKind::IvanBomb
    } else {
        CursorFeedbackKind::Invalid
    }
}

/// Whether `actor`'s GetFireError for `target` is ILLEGAL, which ends its
/// unforced Attack action ([`crate::sim::world::Simulation::selected_weapon_fire_error`]).
fn click_refused(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    actor: Option<u64>,
    target: u64,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> bool {
    rules.zip(actor).is_some_and(|(rules, actor)| {
        sim.selected_weapon_fire_error(
            rules,
            actor,
            crate::sim::combat::TargetKind::Entity(target),
            overlay_registry,
        ) == crate::sim::combat::fire_error::FireError::Illegal
    })
}

/// The bomb actions under force-fire. `InfantryClass::What_Action_OnObject`
/// checks DisarmBomb right after the base action (`0x0051E462`; only the
/// ToggleSelect return precedes it), so it wins over the forced Attack; a
/// Crazy Ivan's forced Attack becomes IvanBomb or NoIvanBomb (`0x0051EB24`).
/// A single selected object over itself keeps its self action.
fn forced_bomb_feedback(
    sim: &crate::sim::world::Simulation,
    selected: &[u64],
    best_id: Option<u64>,
    hover: &crate::app::input::entity_pick::HoverTargetKindWithId,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> Option<CursorFeedbackKind> {
    use crate::map::entities::EntityCategory;
    let rules = rules?;
    let best = best_id.and_then(|id| sim.entities().get(id))?;
    let best_obj = rules.object(sim.interner.resolve(best.type_ref()))?;
    let target = sim.entities().get(hover.stable_id)?;
    if best.category == EntityCategory::Infantry
        && best_obj.engineer
        && target.bomb.is_some()
        && sim.bomb_seen_by(hover.stable_id, best.owner())
    {
        return Some(CursorFeedbackKind::DisarmBomb);
    }
    if selected.len() == 1 && selected[0] == hover.stable_id {
        return None;
    }
    best_obj.ivan.then(|| {
        ivan_bomb_action(
            Some(target),
            rules.object(sim.interner.resolve(target.type_ref())),
        )
    })
}

/// Does the object that owns the cursor have a weapon that reaches the target?
///
/// gamemd shows the action of ONE resolved object, so the in-range split is a
/// property of that object alone, not of any unit in the selection. Both weapon
/// slots count, matching the all-slots weapon-range query the object resolver
/// itself uses.
///
/// `primary`/`secondary` here are the native weapon-array slots 0 and 1
/// (`TechnoTypeClass+0x898`/`+0x8B4`, filled by `Weapon1=`/`Weapon2=` for a
/// `TurretCount>0` type — see `ObjectType::read_weapon_arrays`), not the raw
/// `Primary=`/`Secondary=` INI keys. Reading the keys made this return `false`
/// unconditionally for `[SREF]` and `[YAGGUN]`, showing the out-of-range attack
/// cursor over every enemy at any distance.
fn resolved_unit_in_range(
    sim: &crate::sim::world::Simulation,
    actor_id: u64,
    target_id: u64,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> bool {
    let rules = match rules {
        Some(r) => r,
        None => return true,
    };
    let target_pos = match sim.entities().get(target_id) {
        Some(t) => (
            t.position.rx,
            t.position.ry,
            t.position.sub_x,
            t.position.sub_y,
        ),
        None => return false,
    };
    let Some(entity) = sim.entities().get(actor_id) else {
        return false;
    };
    let Some(obj) = rules.object(sim.interner.resolve(entity.type_ref())) else {
        return false;
    };
    // Height predicates execute native map queries. Keep their shared fallback
    // identity local to this input probe, across source/range and both slots.
    let cells = terrain.map(crate::map::resolved_terrain::NativeCellQuery::isolated);
    for slot in [obj.primary(), obj.secondary()] {
        let weapon = match slot.and_then(|w| rules.weapon(w)) {
            Some(w) => w,
            None => continue,
        };
        if weapon.range <= crate::util::fixed_math::SIM_ZERO {
            continue;
        }
        let in_range = if let Some(cells) = cells.as_ref() {
            let entity_target = combat::TargetKind::Entity(target_id);
            let Some(src) = combat::in_range::fire_source_coords_in_query(
                entity,
                &entity_target,
                weapon,
                sim.entities(),
                cells,
                (rules, &sim.interner),
            ) else {
                continue;
            };
            combat::in_range::compute_in_range_in_query(
                entity,
                src,
                &entity_target,
                weapon,
                rules,
                &sim.interner,
                sim.entities(),
                cells,
                &combat::line_of_fire::LineOfFireInputs {
                    overlay_grid: sim.overlay_grid.as_ref(),
                    overlay_registry,
                    alliances: Some(&sim.house_alliances),
                },
            )
        } else {
            let dist_sq = combat::lepton_distance_sq_raw(
                entity.position.rx,
                entity.position.ry,
                entity.position.sub_x,
                entity.position.sub_y,
                target_pos.0,
                target_pos.1,
                target_pos.2,
                target_pos.3,
            );
            combat::is_within_range_leptons(dist_sq, weapon.range)
        };
        if in_range {
            return true;
        }
    }
    false
}

/// What the object resolver measures its candidates' distance to.
///
/// gamemd's resolver takes a cell *or* an object: with a cell it measures to the
/// cell centre at ground level, with an object to that object's own world point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActionDistanceTarget {
    /// Cell centre, in cell coordinates.
    CellCentre(u16, u16),
    /// A specific object's world point.
    Object(u64),
}

/// Leptons per cell — a cell centre sits half a cell in on both axes.
const LEPTONS_PER_CELL: i64 = crate::util::lepton::LEPTONS_PER_CELL_I32 as i64;

/// World point in leptons for one entity, including altitude when terrain is
/// resolved. gamemd's resolver reads the object's full 3-D world coordinate.
fn entity_world_leptons(
    entity: &crate::sim::game_entity::GameEntity,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> (i64, i64, i64) {
    let z = terrain.map_or(0, |t| {
        i64::from(crate::sim::movement::ground_pose::object_world_z_leptons(
            entity,
            Some(t),
        ))
    });
    (
        entity.position.rx as i64 * LEPTONS_PER_CELL + entity.position.sub_x.to_num::<i64>(),
        entity.position.ry as i64 * LEPTONS_PER_CELL + entity.position.sub_y.to_num::<i64>(),
        z,
    )
}

/// Integer square root — the resolver compares whole-lepton distances, not
/// squares, so ties that round to the same distance must resolve the same way.
fn isqrt_u64(value: u64) -> u64 {
    if value < 2 {
        return value;
    }
    let mut guess = 1u64 << ((64 - value.leading_zeros()).div_ceil(2));
    loop {
        let next = (guess + value / guess) / 2;
        if next >= guess {
            return guess;
        }
        guess = next;
    }
}

/// Pick the single object whose action drives the cursor for the whole selection.
///
/// Matches the original engine's `SelectBestObjectForAction` score ladder:
///   5 — actionable, not a building, and at least one weapon slot has range
///   4 — actionable, not a building
///   3 — actionable
///   2 — on the map but not actionable
/// (the engine's tiers 1 and 0 cover deploy-in-progress and warp states that
/// VERA does not model here yet — recorded, not invented.)
///
/// Ties are *evaluated*, not skipped: a strictly higher score always replaces
/// the incumbent and overwrites the stored distance even when it is farther
/// away, while an equal score replaces only on a strictly smaller distance.
/// Distance is 3-D Euclidean in leptons — to the cell centre when a cell was
/// supplied, otherwise to the target object's own world point.
pub(crate) fn select_best_for_action(
    sim: &crate::sim::world::Simulation,
    selected: &[u64],
    target: ActionDistanceTarget,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> Option<u64> {
    use crate::map::entities::EntityCategory;

    let terrain = sim.resolved_terrain.as_ref();
    let target_point: (i64, i64, i64) = match target {
        ActionDistanceTarget::CellCentre(cx, cy) => (
            cx as i64 * LEPTONS_PER_CELL + LEPTONS_PER_CELL / 2,
            cy as i64 * LEPTONS_PER_CELL + LEPTONS_PER_CELL / 2,
            0,
        ),
        ActionDistanceTarget::Object(id) => match sim.entities().get(id) {
            Some(e) => entity_world_leptons(e, terrain),
            None => return None,
        },
    };

    let mut best_id: Option<u64> = None;
    let mut best_priority: i32 = -1;
    let mut best_dist: u64 = u64::MAX;

    for &sid in selected {
        let Some(entity) = sim.entities().get(sid) else {
            continue;
        };
        let priority = if entity.category == EntityCategory::Structure {
            // A building is actionable but is a building, so it stops at 3.
            3
        } else {
            // The engine's weapon test queries *all* weapon slots, so a
            // secondary-only unit scores 5 just like a primary-armed one.
            //
            // `primary`/`secondary` are native weapon-array slots 0/1
            // (`TechnoTypeClass+0x898`/`+0x8B4`), so a `TurretCount>0` type
            // reads through here correctly: `[SREF]` scores 5 on `Weapon1`.
            //
            // RESIDUAL (UNCHECKED, deferred): "all slots" is modelled as slots
            // 0 and 1 only, so a type whose sole weapon sits at slot >= 2 would
            // score 4. No stock section is shaped that way — `[FV]`, `[YTNK]`
            // and `[YAGGUN]` all author slot 0 — so the frequency is zero on
            // retail data. Downstream: cursor/order dispatch only, no sim
            // state.
            let obj = rules.and_then(|r| r.object(sim.interner.resolve(entity.type_ref())));
            let has_weapon = obj.is_some_and(|o| {
                [o.primary(), o.secondary()]
                    .into_iter()
                    .flatten()
                    .filter_map(|w| rules.and_then(|r| r.weapon(w)))
                    .any(|w| w.range > crate::util::fixed_math::SIM_ZERO)
            });
            if has_weapon { 5 } else { 4 }
        };

        let (sx, sy, sz) = entity_world_leptons(entity, terrain);
        let (dx, dy, dz) = (
            sx - target_point.0,
            sy - target_point.1,
            sz - target_point.2,
        );
        let dist = isqrt_u64((dx * dx + dy * dy + dz * dz) as u64);

        if priority > best_priority || (priority == best_priority && dist < best_dist) {
            best_priority = priority;
            best_dist = dist;
            best_id = Some(sid);
        }
    }
    best_id
}

/// Map a game-state cursor intent to the visual CursorId to display.
/// Returns None for feedback kinds that use procedural visuals instead of a software cursor
/// (e.g. building placement preview).
/// Alias mappings live here: FriendlyUnit→Select, etc.
pub(crate) fn cursor_id_for_feedback(kind: CursorFeedbackKind) -> Option<CursorId> {
    match kind {
        CursorFeedbackKind::FriendlyUnit | CursorFeedbackKind::FriendlyStructure => {
            Some(CursorId::Select)
        }
        CursorFeedbackKind::Move => Some(CursorId::Move),
        CursorFeedbackKind::AttackMove => Some(CursorId::AttackMove),
        CursorFeedbackKind::EnemyUnit | CursorFeedbackKind::EnemyStructure => {
            Some(CursorId::Attack)
        }
        // Harvest and out-of-range attack share cursor row 21 — the action
        // switch falls through from the attack case straight into the harvest
        // case, so both land on the same row.
        CursorFeedbackKind::EnemyOutOfRange | CursorFeedbackKind::Harvest => {
            Some(CursorId::AttackOutOfRange)
        }
        CursorFeedbackKind::Invalid => Some(CursorId::NoMove),
        CursorFeedbackKind::PlaceValid | CursorFeedbackKind::PlaceInvalid => None,
        CursorFeedbackKind::Scroll(dir) => Some(scroll_dir_to_cursor_id(dir)),
        CursorFeedbackKind::ScrollBlocked(dir) => Some(blocked_scroll_dir_to_cursor_id(dir)),
        // Cursor-table row 61, mouse.sha frame 385.
        CursorFeedbackKind::Pan => Some(CursorId::Pan),
        // Rows 62..69, frames 386..393 — a set of their own, NOT the barred
        // edge-scroll rows, which are rows 9..16 / frames 10..17 and carry
        // edge-anchored hotspots instead of centre/centre.
        CursorFeedbackKind::PanBlocked(dir) => Some(pan_dir_to_cursor_id(dir)),
        CursorFeedbackKind::MinimapMove => Some(CursorId::MinimapMove),
        CursorFeedbackKind::Enter => Some(CursorId::Enter),
        CursorFeedbackKind::EngineerDamage => Some(CursorId::EngineerRepair),
        CursorFeedbackKind::RepairAction(valid) => Some(if valid {
            CursorId::Repair
        } else {
            CursorId::NoRepair
        }),
        CursorFeedbackKind::Demolish => Some(CursorId::Demolish),
        // `DisplayClass::SetCursorFromAction @ 0x004AAE90`: action 0x35 ->
        // row 0x26, action 0x39 -> row 0x3B (action 0x36 NoIvanBomb shares
        // NoMove's row 0x13, so it is `Invalid`).
        CursorFeedbackKind::IvanBomb => Some(CursorId::IvanBomb),
        CursorFeedbackKind::DisarmBomb => Some(CursorId::Disarm),
        CursorFeedbackKind::Deploy => Some(CursorId::Deploy),
        CursorFeedbackKind::NoDeploy => Some(CursorId::NoDeploy),
        CursorFeedbackKind::RepairMode(valid) => Some(if valid {
            CursorId::Repair
        } else {
            CursorId::NoRepair
        }),
        CursorFeedbackKind::SellMode(valid) => Some(if valid {
            CursorId::Sell
        } else {
            CursorId::NoSell
        }),
        CursorFeedbackKind::SuperWeaponTarget(id) => Some(id),
    }
}

fn scroll_dir_to_cursor_id(dir: ScrollDir) -> CursorId {
    match dir {
        ScrollDir::N => CursorId::ScrollN,
        ScrollDir::NE => CursorId::ScrollNE,
        ScrollDir::E => CursorId::ScrollE,
        ScrollDir::SE => CursorId::ScrollSE,
        ScrollDir::S => CursorId::ScrollS,
        ScrollDir::SW => CursorId::ScrollSW,
        ScrollDir::W => CursorId::ScrollW,
        ScrollDir::NW => CursorId::ScrollNW,
    }
}

/// Cursor-table rows 62..69, the right-drag pan's directional variants.
fn pan_dir_to_cursor_id(dir: ScrollDir) -> CursorId {
    match dir {
        ScrollDir::N => CursorId::PanN,
        ScrollDir::NE => CursorId::PanNE,
        ScrollDir::E => CursorId::PanE,
        ScrollDir::SE => CursorId::PanSE,
        ScrollDir::S => CursorId::PanS,
        ScrollDir::SW => CursorId::PanSW,
        ScrollDir::W => CursorId::PanW,
        ScrollDir::NW => CursorId::PanNW,
    }
}

fn blocked_scroll_dir_to_cursor_id(dir: ScrollDir) -> CursorId {
    match dir {
        ScrollDir::N => CursorId::NoMoveN,
        ScrollDir::NE => CursorId::NoMoveNE,
        ScrollDir::E => CursorId::NoMoveE,
        ScrollDir::SE => CursorId::NoMoveSE,
        ScrollDir::S => CursorId::NoMoveS,
        ScrollDir::SW => CursorId::NoMoveSW,
        ScrollDir::W => CursorId::NoMoveW,
        ScrollDir::NW => CursorId::NoMoveNW,
    }
}

/// Phase of the animated software cursor.
///
/// The original keeps this in engine globals and the contract has two halves.
/// Setting a new mouse shape zeroes the current frame and re-anchors the timer,
/// so every cursor change restarts its sequence at frame 0 rather than dropping
/// into the middle of a shared free-running phase. The per-frame update then
/// advances by exactly **one** frame once the interval has elapsed and
/// re-anchors again — never several — so a frame-rate stall makes the animation
/// lag instead of skipping frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CursorAnimation {
    shape: Option<CursorId>,
    frame: usize,
    anchor_ms: u64,
}

impl CursorAnimation {
    pub(crate) const fn new() -> Self {
        Self {
            shape: None,
            frame: 0,
            anchor_ms: 0,
        }
    }

    /// Resolve the frame index to draw for `id` at `now_ms`, updating the phase.
    pub(crate) fn advance(
        &mut self,
        id: CursorId,
        frame_count: usize,
        interval_ms: u64,
        now_ms: u64,
    ) -> usize {
        if self.shape != Some(id) {
            self.shape = Some(id);
            self.frame = 0;
            self.anchor_ms = now_ms;
            return 0;
        }
        if frame_count <= 1 || interval_ms == 0 {
            return 0;
        }
        if now_ms.saturating_sub(self.anchor_ms) >= interval_ms {
            self.frame = (self.frame + 1) % frame_count;
            self.anchor_ms = now_ms;
        }
        self.frame
    }
}

thread_local! {
    static CURSOR_ANIMATION: std::cell::Cell<CursorAnimation> =
        const { std::cell::Cell::new(CursorAnimation::new()) };
}

/// The frame of `sequence` to draw for cursor `id` right now.
///
/// The id is part of the query because the phase is keyed on it: asking for a
/// different cursor than last frame restarts that cursor's animation.
pub(crate) fn software_cursor_frame_for(
    id: CursorId,
    sequence: &SoftwareCursorSequence,
) -> Option<&SoftwareCursorFrame> {
    if sequence.frames.is_empty() {
        return None;
    }
    let now_ms: u64 = cursor_animation_start()
        .elapsed()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX);
    let frame_idx = CURSOR_ANIMATION.with(|cell| {
        let mut animation = cell.get();
        let idx = animation.advance(id, sequence.frames.len(), sequence.interval_ms, now_ms);
        cell.set(animation);
        idx
    });
    sequence.frames.get(frame_idx)
}

/// Shell screens (menu, skirmish setup, score) only ever show the default
/// arrow, which is a single static frame.
pub(crate) fn current_software_cursor_frame(
    sequence: &SoftwareCursorSequence,
) -> Option<&SoftwareCursorFrame> {
    software_cursor_frame_for(CursorId::Default, sequence)
}

fn cursor_animation_start() -> &'static Instant {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now)
}

fn is_cursor_over_minimap(state: &AppState) -> bool {
    // Minimap interaction disabled when radar is not online.
    let minimap_visible: bool = state
        .match_state
        .match_presentation
        .radar_anim
        .as_ref()
        .map_or(true, |ra| ra.is_minimap_visible());
    if !minimap_visible {
        return false;
    }
    let Some(_minimap) = &state.match_state.match_presentation.minimap else {
        return false;
    };
    let rect = crate::app::presentation::sidebar_render::active_minimap_screen_rect(state);
    state
        .match_state
        .match_presentation
        .minimap
        .as_ref()
        .unwrap()
        .contains_screen_point_in_rect(
            state.match_state.input.cursor_x,
            state.match_state.input.cursor_y,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
        )
}

pub(crate) fn current_sidebar_view_hit(state: &AppState) -> bool {
    let sw = state
        .match_state
        .match_presentation
        .sidebar_layout_spec
        .sidebar_width;
    let panel_rect = crate::ui::sidebar::Rect {
        x: state.render_width() as f32 - sw - 10.0,
        y: 10.0,
        w: sw,
        h: state.render_height() as f32 - 20.0,
    };
    panel_rect.contains(
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    )
}

/// Map a SuperWeaponType `Action=` INI string to its targeting cursor.
///
/// Action strings come from `[SWType] Action=` in rulesmd.ini. Cursor
/// frame ranges are pre-loaded in `render/cursor_atlas.rs`.
///
/// Returns `None` for `IonCannon` (TS-legacy, no YR SW uses it) and any
/// unrecognized string. Caller should fall back to `CursorId::Default`.
pub(crate) fn super_weapon_cursor_id(action: &str) -> Option<CursorId> {
    match action {
        "Nuke" => Some(CursorId::Nuke),
        "ChronoSphere" => Some(CursorId::Chronosphere),
        "ChronoWarp" => Some(CursorId::Chronosphere),
        "IronCurtain" => Some(CursorId::IronCurtain),
        "LightningStorm" => Some(CursorId::LightningStorm),
        "ParaDrop" => Some(CursorId::Paradrop),
        "AmerParaDrop" => Some(CursorId::Paradrop),
        "PsychicDominator" => Some(CursorId::PsychicDominator),
        "SpyPlane" => Some(CursorId::SpyPlane),
        "GeneticConverter" => Some(CursorId::GeneticMutator),
        "ForceShield" => Some(CursorId::ForceShield),
        "PsychicReveal" => Some(CursorId::PsychicReveal),
        // IonCannon is TS-legacy — no YR superweapon uses this Action.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActionDistanceTarget, CellAction, capability_cursor_for_hover, select_best_for_action,
        super_weapon_cursor_id, what_action_on_cell,
    };
    use crate::app::input::entity_pick::HoverTargetKindWithId;
    use crate::app::types::{CursorFeedbackKind, CursorId, HoverTargetKind};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::world::Simulation;

    fn cursor_contract_rules() -> RuleSet {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             0=GHOST\n\
             [VehicleTypes]\n\
             0=CMIN\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=NAPOWR\n\
             1=GAREFN\n\
             [GHOST]\n\
             Strength=100\n\
             C4=yes\n\
             [CMIN]\n\
             Strength=1000\n\
             Harvester=yes\n\
             Dock=GAREFN\n\
             [NAPOWR]\n\
             Strength=750\n\
             CanC4=yes\n\
             [GAREFN]\n\
             Strength=1000\n\
             Refinery=yes\nDockUnload=yes\n",
        );
        RuleSet::from_ini(&ini).expect("cursor contract rules")
    }

    /// Repro for "SEAL right-click on enemy buildings does nothing".
    /// Loads a narrow stock-shaped rules contract, spawns a SEAL via
    /// `spawn_object` (the same code path the barracks uses on production
    /// completion), then calls `capability_cursor_for_hover` and asserts
    /// the returned cursor is `Demolish`. If it isn't, the body of the
    /// function prints which gate condition rejected it.
    #[test]
    fn seal_hovering_enemy_building_shows_demolish() {
        // 1. Load the narrow stock-shaped contract consumed by this cursor path.
        let rules = cursor_contract_rules();

        // 2. Build a Simulation. resolve_type_handles is required by the
        //    c4 tick path even though we don't tick here — keeps the sim in a
        //    consistent state with what the runtime would see.
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);

        // 3. Spawn a SEAL and an enemy Power Plant via the same path the
        //    barracks uses on production completion.
        let seal_id = sim
            .spawn_object("GHOST", "Americans", 5, 5, 0, &rules)
            .expect("SEAL spawned");
        let bld_id = sim
            .spawn_object("NAPOWR", "Soviets", 10, 10, 0, &rules)
            .expect("Power Plant spawned");

        // 4. Mark the SEAL as selected (mirrors clicking it in-game).
        if let Some(e) = sim.entities_mut().get_mut(seal_id) {
            e.selected = true;
        }

        // 5. Construct the hover descriptor the runtime would build when
        //    the cursor is over an enemy building.
        let hover = HoverTargetKindWithId {
            kind: HoverTargetKind::EnemyStructure,
            stable_id: bld_id,
        };

        // 6. Call the same function the live cursor pipeline calls.
        let result = capability_cursor_for_hover(
            &sim,
            &[seal_id],
            Some(seal_id),
            &hover,
            Some(&rules),
            None,
        );

        // 7. Dump the gate inputs so we can see which condition fails
        //    if the assertion below trips.
        let seal_obj = rules.object("GHOST");
        let bld_obj = rules.object("NAPOWR");
        eprintln!(
            "DIAG: seal.c4={:?} bld.can_c4={:?} bld.invis={:?} cursor={:?}",
            seal_obj.map(|o| o.c4),
            bld_obj.map(|o| o.can_c4),
            bld_obj.map(|o| o.invisible_in_game),
            result,
        );

        assert_eq!(
            result,
            CursorFeedbackKind::Demolish,
            "SEAL hovering an enemy Power Plant should show Demolish cursor",
        );
    }

    /// Chrono Miner hovering its own Allied Refinery should show the dock
    /// (Enter) cursor. gamemd action 0x1A — the TechnoClass dock branch fires
    /// for any harvester targeting a same-owner refinery.
    #[test]
    fn chrono_miner_hovering_own_refinery_shows_enter() {
        let rules = cursor_contract_rules();

        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);

        let miner_id = sim
            .spawn_object("CMIN", "Americans", 5, 5, 0, &rules)
            .expect("Chrono Miner spawned");
        let refinery_id = sim
            .spawn_object("GAREFN", "Americans", 10, 10, 0, &rules)
            .expect("Refinery spawned");

        if let Some(e) = sim.entities_mut().get_mut(miner_id) {
            e.selected = true;
        }

        let hover = HoverTargetKindWithId {
            kind: HoverTargetKind::FriendlyStructure,
            stable_id: refinery_id,
        };

        let result = capability_cursor_for_hover(
            &sim,
            &[miner_id],
            Some(miner_id),
            &hover,
            Some(&rules),
            None,
        );
        assert_eq!(
            result,
            CursorFeedbackKind::Enter,
            "Chrono Miner hovering its own Refinery should show the Enter (dock) cursor",
        );
    }

    #[test]
    fn maps_every_yr_active_action() {
        assert_eq!(super_weapon_cursor_id("Nuke"), Some(CursorId::Nuke));
        assert_eq!(
            super_weapon_cursor_id("ChronoSphere"),
            Some(CursorId::Chronosphere)
        );
        assert_eq!(
            super_weapon_cursor_id("ChronoWarp"),
            Some(CursorId::Chronosphere)
        );
        assert_eq!(
            super_weapon_cursor_id("IronCurtain"),
            Some(CursorId::IronCurtain)
        );
        assert_eq!(
            super_weapon_cursor_id("LightningStorm"),
            Some(CursorId::LightningStorm)
        );
        assert_eq!(super_weapon_cursor_id("ParaDrop"), Some(CursorId::Paradrop));
        assert_eq!(
            super_weapon_cursor_id("AmerParaDrop"),
            Some(CursorId::Paradrop)
        );
        assert_eq!(
            super_weapon_cursor_id("PsychicDominator"),
            Some(CursorId::PsychicDominator)
        );
        assert_eq!(super_weapon_cursor_id("SpyPlane"), Some(CursorId::SpyPlane));
        assert_eq!(
            super_weapon_cursor_id("GeneticConverter"),
            Some(CursorId::GeneticMutator)
        );
        assert_eq!(
            super_weapon_cursor_id("ForceShield"),
            Some(CursorId::ForceShield)
        );
        assert_eq!(
            super_weapon_cursor_id("PsychicReveal"),
            Some(CursorId::PsychicReveal)
        );
    }

    #[test]
    fn returns_none_for_ts_legacy_and_unknown() {
        assert_eq!(super_weapon_cursor_id("IonCannon"), None);
        assert_eq!(super_weapon_cursor_id(""), None);
        assert_eq!(super_weapon_cursor_id("BogusAction"), None);
    }

    /// Rules for the cell-action and best-object contracts: one plain armed
    /// tank, one unarmed truck, one unit armed only through `Secondary=`.
    fn cell_action_rules() -> RuleSet {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             0=MTNK\n\
             1=TRUCKA\n\
             2=SREF\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [MTNK]\n\
             Strength=300\n\
             Primary=105mm\n\
             [TRUCKA]\n\
             Strength=150\n\
             [SREF]\n\
             Strength=200\n\
             Secondary=105mm\n\
             [WeaponTypes]\n\
             0=105mm\n\
             [105mm]\n\
             Damage=60\n\
             Range=5\n",
        );
        RuleSet::from_ini(&ini).expect("cell action rules")
    }

    /// A tank that actually carries a locomotor, and therefore a SpeedType.
    ///
    /// `cell_action_rules` declares no `Speed=`, so its MTNK spawns with
    /// `locomotor: None` and every SpeedType-dependent branch of the cell-entry
    /// predicate short-circuits. Any test about the speed row or the bridge
    /// plane has to use this one instead.
    fn sim_with_track_tank() -> (Simulation, RuleSet, u64) {
        let ini = IniFile::from_str(
            "[InfantryTypes]
             [VehicleTypes]
             0=MTNK
             [AircraftTypes]
             [BuildingTypes]
             [MTNK]
             Strength=300
             Primary=105mm
             Speed=6
             SpeedType=Track
             MovementZone=Normal
             Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
             [WeaponTypes]
             0=105mm
             [105mm]
             Damage=60
             Range=5
",
        );
        let rules = RuleSet::from_ini(&ini).expect("track tank rules");
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        let tank = sim
            .spawn_object("MTNK", "Americans", 2, 2, 0, &rules)
            .expect("tank spawned");
        assert!(
            sim.entities()
                .get(tank)
                .and_then(|e| e.locomotor.as_ref())
                .is_some(),
            "the fixture must give the mover a locomotor, or the speed row is skipped",
        );
        (sim, rules, tank)
    }

    fn sim_with_tank() -> (Simulation, RuleSet, u64) {
        let rules = cell_action_rules();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        let tank = sim
            .spawn_object("MTNK", "Americans", 2, 2, 0, &rules)
            .expect("tank spawned");
        (sim, rules, tank)
    }

    #[test]
    fn attack_cursor_missing_cell_queries_preserve_simulation_dummy_and_hash() {
        use crate::map::resolved_terrain::ResolvedTerrainGrid;
        use crate::sim::combat::{TargetKind, in_range, line_of_fire::LineOfFireInputs};

        let (mut sim, rules, actor_id) = sim_with_tank();
        let target_id = sim
            .spawn_object("MTNK", "Soviets", 3, 2, 0, &rules)
            .expect("enemy target");
        // Both live objects deliberately miss this allocated map. Their exact
        // Z and marked state force source +54 and target +50/+54 to read the
        // native fallback Cell rather than bypassing the query as limbo objects.
        for id in [actor_id, target_id] {
            let entity = sim.entities_mut().get_mut(id).unwrap();
            entity.position.exact_z_leptons = Some(900);
            entity.lifecycle.cell_marked = true;
        }
        let mut retained_cell = crate::map::resolved_terrain::test_flat_cell(0, 0);
        // A bridge anchor retaining Dummy makes its requested coordinate a
        // deterministic future input. World hashing intentionally omits that
        // transient coordinate when no Bullet or bridge anchor retains it.
        retained_cell.bridge_facts.native_anchor =
            Some(crate::map::cell_index::NativeCellIdentity::Dummy);
        sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(
            1,
            1,
            vec![retained_cell],
        ));
        let dummy = sim.effective_shared_cell_dummy();
        dummy.stamp_coord(111, -222);
        dummy.set_level_slope(2, 1);
        dummy.test_set_land_type(4);
        dummy.test_set_retained_bridge_flags(0x1180);
        let initial = dummy.snapshot();
        let initial_hash = sim.state_hash();
        let hover = HoverTargetKindWithId {
            kind: HoverTargetKind::EnemyUnit,
            stable_id: target_id,
        };
        for _ in 0..3 {
            assert_eq!(
                capability_cursor_for_hover(
                    &sim,
                    &[actor_id],
                    Some(actor_id),
                    &hover,
                    Some(&rules),
                    None,
                ),
                CursorFeedbackKind::EnemyUnit,
            );
            assert_eq!(
                dummy.snapshot(),
                initial,
                "cursor must not stamp canonical Dummy"
            );
            assert_eq!(dummy.land_type(), 4);
            assert_eq!(
                sim.state_hash(),
                initial_hash,
                "input probing must not change simulation"
            );
        }

        // The ordinary simulation wrappers must still perform these same
        // lookups canonically. This also proves the fixture really reaches
        // missing-cell source and target reads instead of an early return.
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let actor = sim.entities().get(actor_id).unwrap();
        let target = TargetKind::Entity(target_id);
        let weapon = rules.weapon("105mm").unwrap();
        let source = in_range::fire_source_coords(
            actor,
            &target,
            weapon,
            sim.entities(),
            terrain,
            (&rules, &sim.interner),
        )
        .unwrap();
        assert_eq!(dummy.snapshot().coord, (2, 2));
        assert!(in_range::compute_in_range(
            actor,
            source,
            &target,
            weapon,
            &rules,
            &sim.interner,
            sim.entities(),
            terrain,
            &LineOfFireInputs::terrain_only(),
        ));
        // InRange's final source-cell bridge query6F7601 follows the
        // target height/ground queries, restoring the source's Dummy XY.
        assert_eq!(dummy.snapshot().coord, (2, 2));
        assert_ne!(sim.state_hash(), initial_hash);
    }

    /// gamemd answers action 2 (no-move) for a cell its occupancy probe rejects,
    /// so the barred cursor appears before the click — the move cursor is not a
    /// constant over empty ground.
    #[test]
    fn blocked_cell_resolves_to_no_move() {
        use crate::app::input::context_order::OrderModifier;
        use crate::sim::pathfinding::PathGrid;

        let (sim, _rules, tank) = sim_with_tank();
        let mut grid = PathGrid::new(8, 8);
        grid.set_blocked(6, 6, true);

        assert_eq!(
            what_action_on_cell(&sim, Some(tank), (4, 4), Some(&grid), OrderModifier::Normal),
            CellAction::Move,
            "an open cell inside the playfield answers move",
        );
        assert_eq!(
            what_action_on_cell(&sim, Some(tank), (6, 6), Some(&grid), OrderModifier::Normal),
            CellAction::NoMove,
            "a blocked cell answers no-move",
        );
    }

    /// The probe runs on the TARGET cell's planes, not the mover's current one.
    ///
    /// Native passes the level argument to `vtable+0x1AC` as -1, so the cell
    /// picks the plane. Pinning it to the mover's layer instead barred the
    /// cursor over every high-bridge deck — a tank on open ground would see the
    /// no-move cursor over a crossing, and a tank already on the deck would see
    /// it over the entire rest of the map.
    #[test]
    fn a_bridge_only_cell_answers_move_for_a_ground_mover() {
        use crate::app::input::context_order::OrderModifier;
        use crate::sim::movement::locomotor::MovementLayer;
        use crate::sim::pathfinding::PathGrid;

        let (sim, _rules, tank) = sim_with_tank();
        let mut grid = PathGrid::new(8, 8);
        // Deck cell: walkable on the bridge plane, closed on the ground plane.
        grid.set_cell_for_test(6, 6, 0, true, false);
        grid.set_blocked(6, 6, true);
        assert!(!grid.is_walkable_on_layer(6, 6, MovementLayer::Ground));
        assert!(grid.is_walkable_on_layer(6, 6, MovementLayer::Bridge));

        assert_eq!(
            what_action_on_cell(&sim, Some(tank), (6, 6), Some(&grid), OrderModifier::Normal),
            CellAction::Move,
            "a ground mover must be offered the deck",
        );

        // And a cell closed on both planes still answers no-move.
        grid.set_blocked(5, 5, true);
        assert!(!grid.is_walkable_on_layer(5, 5, MovementLayer::Bridge));
        assert_eq!(
            what_action_on_cell(&sim, Some(tank), (5, 5), Some(&grid), OrderModifier::Normal),
            CellAction::NoMove,
        );
    }

    /// A collapsed span keeps its structural bit and loses only its deck, and the
    /// bridge leaf answers Clear off the structural bit alone — so without the
    /// walkable-deck gate the barred cursor would vanish from a blown bridge,
    /// which is exactly where a player needs it. `DestroyableBridges=yes` is the
    /// stock default, so this is ordinary mid-game play.
    #[test]
    fn a_destroyed_span_answers_no_move_for_a_ground_mover() {
        use crate::app::input::context_order::OrderModifier;
        use crate::sim::pathfinding::{PathCell, PathGrid};

        // A mover WITH a speed type: without one the shared leaf short-circuits
        // on `land_passable` and never reaches the bridge-plane branch the deck
        // gate guards, so the test would pass with the gate deleted.
        let (sim, _rules, tank) = sim_with_track_tank();
        let open = PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 0,
            bridge_deck_level: 0,
            slope_type: 0,
            tube_index: None,
            low_bridge_tube_cell: false,
        };
        let mut cells = vec![open.clone(); 8 * 8];
        // The production shape after a span collapses: structural still set, deck
        // cleared, and the water underneath closed to ground movement.
        cells[6 * 8 + 6] = PathCell {
            ground_walkable: false,
            bridge_walkable: false,
            bridge_structural: true,
            bridge_deck_level: 4,
            ..open.clone()
        };
        // An intact deck on the same map, to prove the gate did not simply close
        // the bridge plane altogether.
        cells[3 * 8 + 3] = PathCell {
            ground_walkable: false,
            bridge_walkable: true,
            bridge_structural: true,
            bridge_deck_level: 4,
            ..open.clone()
        };
        let grid = PathGrid::from_cells(cells, 8, 8);

        assert_eq!(
            what_action_on_cell(&sim, Some(tank), (6, 6), Some(&grid), OrderModifier::Normal),
            CellAction::NoMove,
            "a blown span must still show the barred cursor",
        );
        assert_eq!(
            what_action_on_cell(&sim, Some(tank), (3, 3), Some(&grid), OrderModifier::Normal),
            CellAction::Move,
            "an intact deck must still be offered",
        );
    }

    /// The change's motivating case. `PathGrid::from_resolved_terrain_with_bridges`
    /// leaves water cells `ground_walkable`, so the old coarse bit showed the
    /// move cursor over open water for a Track mover; the shared predicate weighs
    /// the mover's SpeedType against the resolved LandType and refuses.
    #[test]
    fn open_water_answers_no_move_for_a_track_mover() {
        use crate::app::input::context_order::OrderModifier;

        let (mut sim, rules, tank) = sim_with_track_tank();
        const SIZE: u16 = 16;
        let mut cells = Vec::new();
        for ry in 0..SIZE {
            for rx in 0..SIZE {
                cells.push(crate::map::resolved_terrain::test_flat_cell(rx, ry));
            }
        }
        let mut terrain =
            crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(SIZE, SIZE, cells);
        let water = crate::rules::terrain_rules::LandType::Water.as_index();
        let cell = terrain.cell_mut(9, 9).expect("water cell inside the grid");
        cell.land_type = water;
        cell.yr_cell_land_type = water;
        cell.base_land_type = water;
        cell.base_yr_cell_land_type = water;
        cell.is_water = true;
        // Production water is ground-blocked and becomes `ground_walkable` only
        // through the `|| is_water` clause in the PathGrid derivation. Tie the
        // fixture to that so it cannot keep passing if the clause is dropped.
        cell.ground_walk_blocked = true;
        cell.base_ground_walk_blocked = true;
        // Water's retail speed row is zero for every land SpeedType; that zero
        // is what the shared predicate reads and the coarse walkable bit does not.
        cell.speed_costs = crate::rules::terrain_rules::SpeedCostProfile {
            foot: Some(0),
            track: Some(0),
            wheel: Some(0),
            float: Some(100),
            amphibious: Some(100),
            float_beach: Some(100),
            hover: Some(100),
        };
        cell.base_speed_costs = cell.speed_costs.clone();
        sim.resolved_terrain = Some(terrain);
        sim.rebuild_dynamic_navigation(&rules);
        let grid = sim.path_grid().cloned().expect("navigation rebuilt a grid");

        assert!(
            grid.is_walkable(9, 9),
            "the fixture only bites while the coarse bit still calls water walkable",
        );
        assert_eq!(
            what_action_on_cell(&sim, Some(tank), (9, 9), Some(&grid), OrderModifier::Normal),
            CellAction::NoMove,
            "a Track mover cannot enter water, so the cursor must say so",
        );
        assert_eq!(
            what_action_on_cell(&sim, Some(tank), (4, 4), Some(&grid), OrderModifier::Normal),
            CellAction::Move,
        );
    }

    /// Outside the playfield the ladder answers no-move before it looks at the
    /// cell at all.
    #[test]
    fn cell_outside_the_playfield_resolves_to_no_move() {
        use crate::app::input::context_order::OrderModifier;
        use crate::sim::pathfinding::PathGrid;

        let (sim, _rules, tank) = sim_with_tank();
        let grid = PathGrid::new(8, 8);

        assert_eq!(
            what_action_on_cell(&sim, Some(tank), (8, 3), Some(&grid), OrderModifier::Normal),
            CellAction::NoMove,
        );
    }

    /// gamemd returns action 1 on Shift and on Alt *before* running the
    /// occupancy probe, so both show the move cursor even over a blocked cell.
    #[test]
    fn shift_and_alt_skip_the_occupancy_probe() {
        use crate::app::input::context_order::OrderModifier;
        use crate::sim::pathfinding::PathGrid;

        let (sim, _rules, tank) = sim_with_tank();
        let mut grid = PathGrid::new(8, 8);
        grid.set_blocked(6, 6, true);

        assert_eq!(
            what_action_on_cell(&sim, Some(tank), (6, 6), Some(&grid), OrderModifier::Queue),
            CellAction::Move,
        );
        assert_eq!(
            what_action_on_cell(
                &sim,
                Some(tank),
                (6, 6),
                Some(&grid),
                OrderModifier::ForceMove
            ),
            CellAction::Move,
        );
    }

    /// The engine's weapon test queries every weapon slot, so a `Secondary=`-only
    /// unit reaches the armed tier and outranks an unarmed one regardless of
    /// which is closer.
    #[test]
    fn secondary_only_unit_outranks_a_closer_unarmed_unit() {
        let rules = cell_action_rules();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);

        let truck = sim
            .spawn_object("TRUCKA", "Americans", 9, 10, 0, &rules)
            .expect("unarmed truck");
        let arty = sim
            .spawn_object("SREF", "Americans", 2, 2, 0, &rules)
            .expect("secondary-only unit");

        let best = select_best_for_action(
            &sim,
            &[truck, arty],
            ActionDistanceTarget::CellCentre(10, 10),
            Some(&rules),
        );
        assert_eq!(
            best,
            Some(arty),
            "the armed tier wins even from much farther away",
        );
    }

    /// Ties inside one tier break on 3-D lepton distance to the *cell centre*.
    /// Both candidates sit in cells the same number of cell indices away, so a
    /// cell-index tie-break cannot separate them; the sub-cell offset can.
    #[test]
    fn tie_break_uses_lepton_distance_to_the_cell_centre() {
        let rules = cell_action_rules();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);

        let near = sim
            .spawn_object("MTNK", "Americans", 8, 10, 0, &rules)
            .expect("near tank");
        let far = sim
            .spawn_object("MTNK", "Americans", 12, 10, 0, &rules)
            .expect("far tank");
        // Nudge the far tank's sub-cell offset toward the target so that both
        // sit two cell indices away but the far one is closer in leptons.
        if let Some(e) = sim.entities_mut().get_mut(far) {
            e.position.sub_x = crate::util::fixed_math::SimFixed::from_num(-120);
        }

        let best = select_best_for_action(
            &sim,
            &[near, far],
            ActionDistanceTarget::CellCentre(10, 10),
            Some(&rules),
        );
        assert_eq!(
            best,
            Some(far),
            "sub-cell position decides the tie, not the cell index",
        );
    }

    /// Native51E551/51FA75: friendship does not gate the terminal hut arm.
    /// Display4AAE90 maps action29 to Repair row33 and action32 to NoRepair
    /// row35. The complete query and caller goldens live in spatial_oracle.
    #[test]
    fn engineer_hut_cursor_uses_native_repair_rows_for_all_relations() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=ENGINEER\n[BuildingTypes]\n0=CABHUT\n\
             [ENGINEER]\nStrength=75\nEngineer=yes\nC4=yes\n\
             [CABHUT]\nStrength=200\nFoundation=1x1\nBridgeRepairHut=yes\nCanC4=yes\nRepairable=yes\nCapturable=yes\n",
        )).unwrap();
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/engineer_bridge_cursor_caller.json",
        ))
        .unwrap();
        for relation in ["hostile", "allied", "self"] {
            for collapsed in [false, true] {
                let mut sim = Simulation::with_seed(0x587410);
                sim.resolve_type_handles(&rules);
                sim.session.game_mode_nonzero = true;
                sim.session.current_house = Some(sim.interner.intern("Americans"));
                // The ordinary Infantry Unlimbo query reads the native
                // Clear/Foot speed row before this cursor-only observation.
                let mut terrain = crate::map::resolved_terrain::test_flat_ground_grid(20);
                terrain.test_set_high_bridge_set_starts(Some(100), Some(200));
                for y in 9..=11 {
                    terrain.cell_mut(12, y).unwrap().bridge_facts.overlay_id =
                        Some(if collapsed { 0xE7 } else { 0xD4 });
                }
                sim.install_resolved_terrain_for_new_map(terrain);
                sim.bridge_state = Some(crate::sim::bridge_state::BridgeRuntimeState::default());
                let owner = if relation == "self" {
                    "Americans"
                } else {
                    "Soviets"
                };
                let engineer = sim
                    .spawn_object("ENGINEER", "Americans", 8, 10, 0, &rules)
                    .unwrap();
                let hut = sim
                    .spawn_object("CABHUT", owner, 10, 10, 0, &rules)
                    .unwrap();
                // A damaged friendly hut and a capturable hostile hut must
                // still terminate this branch instead of taking those fallbacks.
                sim.entities_mut().get_mut(hut).unwrap().health.current = 150;
                if relation == "allied" {
                    sim.house_alliances
                        .entry("AMERICANS".into())
                        .or_default()
                        .insert("SOVIETS".into());
                }
                let kind = if relation == "hostile" {
                    HoverTargetKind::EnemyStructure
                } else {
                    HoverTargetKind::FriendlyStructure
                };
                let before = sim.state_hash();
                let rng = (
                    sim.scenario_rng.logical_state(),
                    sim.main_rng.logical_state(),
                    sim.mapgen_rng.logical_state(),
                );
                let actual = capability_cursor_for_hover(
                    &sim,
                    &[engineer],
                    Some(engineer),
                    &HoverTargetKindWithId {
                        kind,
                        stable_id: hut,
                    },
                    Some(&rules),
                    None,
                );
                let row = native["actions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| {
                        row["input"]["route"] == "object"
                            && row["input"]["name"]
                                == format!(
                                    "{relation}_{}",
                                    if collapsed { "True" } else { "False" }
                                )
                    })
                    .unwrap();
                assert_eq!(row["output"]["action"], if collapsed { 29 } else { 32 });
                assert_eq!(
                    actual,
                    CursorFeedbackKind::RepairAction(collapsed),
                    "{relation} collapsed={collapsed}"
                );
                assert_eq!(
                    super::cursor_id_for_feedback(actual),
                    Some(if collapsed {
                        CursorId::Repair
                    } else {
                        CursorId::NoRepair
                    })
                );
                assert_eq!(
                    sim.state_hash(),
                    before,
                    "hover must leave gameplay state untouched"
                );
                assert_eq!(
                    (
                        sim.scenario_rng.logical_state(),
                        sim.main_rng.logical_state(),
                        sim.mapgen_rng.logical_state()
                    ),
                    rng
                );
            }
        }
    }

    /// Stock Crazy Ivan, Engineer and a heavy tank, with their weapons.
    fn bomb_cursor_rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n\
             0=IVAN\n\
             1=ENGINEER\n\
             [VehicleTypes]\n\
             0=HTNK\n\
             [Warheads]\n\
             0=IvanBomb\n\
             1=BombDisarm\n\
             2=AP\n\
             [CombatDamage]\n\
             IvanTimedDelay=450\n\
             [IVAN]\n\
             Strength=125\n\
             Primary=IvanBomber\n\
             Ivan=yes\n\
             AttackCursorOnFriendlies=yes\n\
             [ENGINEER]\n\
             Strength=75\n\
             Primary=DefuseKit\n\
             Engineer=yes\n\
             BombSight=4\n\
             [HTNK]\n\
             Strength=900\n\
             Primary=120mm\n\
             [IvanBomber]\n\
             Range=1.5\n\
             Projectile=Invisible\n\
             Warhead=IvanBomb\n\
             [DefuseKit]\n\
             Range=1.5\n\
             Projectile=InvisibleAll\n\
             Warhead=BombDisarm\n\
             [120mm]\n\
             Range=5.75\n\
             Projectile=Invisible\n\
             Warhead=AP\n\
             [Invisible]\n\
             Inviso=yes\n\
             [InvisibleAll]\n\
             Inviso=yes\n\
             [IvanBomb]\n\
             IvanBomb=yes\n\
             [BombDisarm]\n\
             BombDisarm=yes\n\
             [AP]\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .expect("bomb cursor rules")
    }

    fn bomb_hover(
        sim: &Simulation,
        rules: &RuleSet,
        actor: u64,
        kind: HoverTargetKind,
        target: u64,
    ) -> CursorFeedbackKind {
        let hover = HoverTargetKindWithId {
            kind,
            stable_id: target,
        };
        capability_cursor_for_hover(sim, &[actor], Some(actor), &hover, Some(rules), None)
    }

    /// `InfantryClass::What_Action_OnObject`: a Crazy Ivan offers IvanBomb on
    /// any target without a bomb — enemy or, by AttackCursorOnFriendlies, own —
    /// and falls back to Select on one that carries a bomb, whose GetFireError
    /// is ILLEGAL near or far; an Engineer offers DisarmBomb on any bombed
    /// object its player sees, own or enemy, and nothing on an unseen one.
    #[test]
    fn crazy_ivan_and_engineer_bomb_cursors() {
        let rules = bomb_cursor_rules();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        for house in ["Americans", "Soviets"] {
            let id = sim.interner.intern(house);
            sim.session.house_order.push(id);
        }
        let mut spawn = |kind: &str, owner: &str, rx: u16| {
            sim.spawn_object(kind, owner, rx, 5, 0, &rules)
                .expect("spawned")
        };
        let ivan = spawn("IVAN", "Americans", 5);
        let engineer = spawn("ENGINEER", "Americans", 7);
        let own_tank = spawn("HTNK", "Americans", 9);
        let enemy_tank = spawn("HTNK", "Soviets", 11);
        let enemy_ivan = spawn("IVAN", "Soviets", 20);
        let far_tank = spawn("HTNK", "Soviets", 22);

        use HoverTargetKind::{EnemyUnit, FriendlyUnit};
        assert_eq!(
            bomb_hover(&sim, &rules, ivan, EnemyUnit, enemy_tank),
            CursorFeedbackKind::IvanBomb
        );
        assert_eq!(
            bomb_hover(&sim, &rules, ivan, FriendlyUnit, own_tank),
            CursorFeedbackKind::IvanBomb,
            "AttackCursorOnFriendlies"
        );
        assert_eq!(
            bomb_hover(&sim, &rules, engineer, FriendlyUnit, own_tank),
            CursorFeedbackKind::FriendlyUnit,
            "no bomb, no DisarmBomb"
        );

        // The Ivan plants on the enemy tank and on the own tank; a Soviet Ivan
        // plants on a Soviet tank nobody American stands near.
        sim.bomb_attach(ivan, Some(enemy_tank), &rules);
        sim.bomb_attach(ivan, Some(own_tank), &rules);
        sim.bomb_attach(enemy_ivan, Some(far_tank), &rules);
        sim.bomb_list_update(&rules);
        sim.bomb_list_update(&rules);

        assert_eq!(
            bomb_hover(&sim, &rules, ivan, EnemyUnit, enemy_tank),
            CursorFeedbackKind::FriendlyUnit,
            "a bombed target falls back to Select"
        );
        assert_eq!(
            bomb_hover(&sim, &rules, engineer, EnemyUnit, enemy_tank),
            CursorFeedbackKind::DisarmBomb
        );
        assert_eq!(
            bomb_hover(&sim, &rules, engineer, FriendlyUnit, own_tank),
            CursorFeedbackKind::DisarmBomb,
            "own units are defused too"
        );
        assert_ne!(
            bomb_hover(&sim, &rules, engineer, EnemyUnit, far_tank),
            CursorFeedbackKind::DisarmBomb,
            "a bomb the player does not see"
        );
    }

    #[test]
    fn bomb_cursor_rows() {
        use super::cursor_id_for_feedback;
        assert_eq!(
            cursor_id_for_feedback(CursorFeedbackKind::IvanBomb),
            Some(CursorId::IvanBomb)
        );
        assert_eq!(
            cursor_id_for_feedback(CursorFeedbackKind::DisarmBomb),
            Some(CursorId::Disarm)
        );
    }
}

#[cfg(test)]
mod cursor_animation_tests {
    use super::CursorAnimation;
    use crate::app::types::{CursorFeedbackKind, CursorId, ScrollDir};

    /// Retail interval for every animated cursor row: rate 4 x 16 ms.
    const INTERVAL_MS: u64 = 64;

    /// The first look at a cursor starts it at frame 0 and anchors the timer
    /// there, instead of sampling a process-wide clock.
    #[test]
    fn a_new_cursor_shape_starts_at_frame_zero() {
        let mut anim = CursorAnimation::new();
        assert_eq!(anim.advance(CursorId::Move, 10, INTERVAL_MS, 5_000), 0);
    }

    /// One frame per elapsed interval, and exactly one — a long stall does not
    /// skip ahead.
    #[test]
    fn animation_advances_one_frame_per_interval_and_never_skips() {
        let mut anim = CursorAnimation::new();
        anim.advance(CursorId::Move, 10, INTERVAL_MS, 0);
        assert_eq!(anim.advance(CursorId::Move, 10, INTERVAL_MS, 63), 0);
        assert_eq!(anim.advance(CursorId::Move, 10, INTERVAL_MS, 64), 1);
        // A 1-second stall still yields a single step.
        assert_eq!(anim.advance(CursorId::Move, 10, INTERVAL_MS, 1_064), 2);
    }

    #[test]
    fn animation_wraps_at_the_end_of_the_sequence() {
        let mut anim = CursorAnimation::new();
        anim.advance(CursorId::Attack, 5, INTERVAL_MS, 0);
        for step in 1..=4 {
            assert_eq!(
                anim.advance(CursorId::Attack, 5, INTERVAL_MS, step * INTERVAL_MS),
                step as usize
            );
        }
        assert_eq!(
            anim.advance(CursorId::Attack, 5, INTERVAL_MS, 5 * INTERVAL_MS),
            0
        );
    }

    /// The clause a process-global phase cannot express: switching cursor
    /// restarts the new sequence at frame 0 rather than dropping into whatever
    /// phase the shared clock happened to be in.
    #[test]
    fn changing_cursor_restarts_the_sequence() {
        let mut anim = CursorAnimation::new();
        anim.advance(CursorId::Move, 10, INTERVAL_MS, 0);
        assert_eq!(
            anim.advance(CursorId::Move, 10, INTERVAL_MS, 3 * INTERVAL_MS),
            1
        );
        assert_eq!(
            anim.advance(CursorId::Attack, 5, INTERVAL_MS, 3 * INTERVAL_MS),
            0
        );
        // And going back restarts again rather than resuming.
        assert_eq!(
            anim.advance(CursorId::Move, 10, INTERVAL_MS, 3 * INTERVAL_MS),
            0
        );
    }

    /// A rate-0 row is static however much time passes.
    #[test]
    fn static_rows_never_advance() {
        let mut anim = CursorAnimation::new();
        anim.advance(CursorId::IronCurtain, 5, 0, 0);
        assert_eq!(anim.advance(CursorId::IronCurtain, 5, 0, 10_000), 0);
    }

    /// Harvest shares cursor row 21 with an out-of-range attack, and is not the
    /// attack-move reticle VERA used to show.
    #[test]
    fn harvest_feedback_maps_to_cursor_row_twenty_one() {
        assert_eq!(
            super::cursor_id_for_feedback(CursorFeedbackKind::Harvest),
            Some(CursorId::AttackOutOfRange)
        );
        assert_ne!(
            super::cursor_id_for_feedback(CursorFeedbackKind::Harvest),
            Some(CursorId::AttackMove)
        );
    }

    #[test]
    fn item82_allowed_and_blocked_scroll_feedback_use_directional_rows() {
        assert_eq!(
            super::cursor_id_for_feedback(CursorFeedbackKind::Scroll(ScrollDir::NE)),
            Some(CursorId::ScrollNE),
        );
        assert_eq!(
            super::cursor_id_for_feedback(CursorFeedbackKind::ScrollBlocked(ScrollDir::NE)),
            Some(CursorId::NoMoveNE),
        );
        assert_eq!(
            super::cursor_id_for_feedback(CursorFeedbackKind::ScrollBlocked(ScrollDir::S)),
            Some(CursorId::NoMoveS),
        );
    }

    // Native query and object-route admission are now compared through the
    // shared587410 owner and engineer_bridge_cursor_caller corpus. Minimap
    // hover, full base WhatAction/modifiers and final SHP drawing remain
    // outside this tactical object-route comparison.
}
