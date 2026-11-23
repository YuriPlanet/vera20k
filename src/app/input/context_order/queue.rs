//! The context-order entry point: `try_queue_context_order` and the band-box
//! left-release decision (`LeftRelease`, berserk selections) it dispatches on.

use super::{
    AttackAction, AttackDispatch, BuildingClick, InfantryC4Action, ObjectKeyAction, OrderModifier,
    ares_head_hook_selection_click, ares_trench_enter, armed_action_with, attack_branch_orders,
    building_click_orders, building_selection_click, cell_guard_area_action_with,
    cell_harvest_order, cursor_owner, disarm_bomb_engineers, engineer_capture_orders, entry_orders,
    finish_order, friendly_bomb_ivans, garrison_entry_orders, infantry_arm_attacks_with,
    infantry_c4_action_with, infiltrator_capture_orders, locked_move_cells, lone_self_foot_release,
    lone_self_healer_guard, move_click_command, object_action_answers_friendly,
    object_attack_click_command_with, object_click_release, object_key_action_with,
    object_key_target, projected_order_selection, push_move_flash, resolve_order_modifiers,
    selection_can_attack_move, self_answers_no_deploy, self_click_command, self_detonates,
    terrain_forced, unit_repair_branch_orders, vehicle_capture_action,
};
use crate::app::AppState;
use crate::app::input::commands::preferred_local_owner;
use crate::app::input::dispatch::{
    is_alt_held, is_ctrl_held, is_shift_held, selected_stable_ids_in_order,
};
use crate::app::input::entity_pick::{HoverObject, HoverTargetKindWithId, hover_object_at_point};
use crate::app::match_runtime::sim_host::SimWrite;
use crate::app::types::{HoverTargetKind, OrderMode};
use crate::map::entities::EntityCategory;
use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::intern::InternedId;

/// What `DisplayClass::BandBox_LeftUp @ 0x004AB9B0` does with the action
/// DetermineAction answered ([`left_release`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LeftRelease {
    /// Every selected object gets the action (`0x004ABE88`); one whose
    /// ClickedAction refuses sends nothing, and the action-line timer still
    /// starts (`0x004ABFA9`).
    Dispatch,
    /// Nothing (`0x004AC294`).
    Nothing,
    /// The ordinary selection click on the object under the cursor
    /// (`0x004ABE18`).
    Select,
}

/// The part of DetermineAction's action that [`left_release`] tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReleaseAction {
    Select,
    Zero,
    Other,
}

impl From<AttackAction> for ReleaseAction {
    fn from(action: AttackAction) -> Self {
        match action {
            AttackAction::Select => Self::Select,
            AttackAction::None => Self::Zero,
            AttackAction::Attack
            | AttackAction::NoMove
            | AttackAction::Rally
            | AttackAction::Move
            | AttackAction::GuardArea
            | AttackAction::AttackSupport => Self::Other,
        }
    }
}

/// `DisplayClass::BandBox_LeftUp @ 0x004AB9B0` for `action`, with `target`
/// the object under the cursor: Select selects it (`0x004ABDD3..0x004ABDDA`),
/// and 0 selects an object the player may select (vt+0x138) and has not
/// (`+0x83`) (`0x004ABDE1..0x004ABE11`); otherwise both do nothing. Every
/// other action dispatches.
pub(super) fn left_release(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    action: ReleaseAction,
    target: Option<u64>,
) -> LeftRelease {
    let selects = match action {
        ReleaseAction::Other => return LeftRelease::Dispatch,
        ReleaseAction::Select => target.is_some(),
        ReleaseAction::Zero => target.is_some_and(|id| {
            sim.object_is_selectable(id, rules)
                && sim
                    .entities()
                    .get(id)
                    .is_some_and(|object| !object.seldom.selection.is_selected())
        }),
    };
    if selects {
        LeftRelease::Select
    } else {
        LeftRelease::Nothing
    }
}

/// What a click answers before its orders, in What_Action's order: an Ares
/// hook at the head of the cursor owner's class What_Action
/// ([`ares_head_hook_selection_click`]) precedes Techno's Shift rung
/// (`shift`, [`super::object_key_action`]), whose ToggleSelect
/// (`0x00700107`) leaves the click to the selection's pick. So does a
/// building's trench Enter (Ares 3.0 `100171B0`, hook `0x0044725F`, before
/// Building What_Action calls Techno's at `0x00447267`), which the building
/// click sends ([`building_selection_click`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HeadRelease {
    /// The hook's release, or the bombed object a Detonate sets off.
    Hook(Result<LeftRelease, u64>),
    ToggleSelect,
}

pub(super) fn head_release(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    current_objects: &[u64],
    hover: Option<&HoverTargetKindWithId>,
    cell: (u16, u16),
    shift: Option<ObjectKeyAction>,
) -> Option<HeadRelease> {
    if let Some(release) = ares_head_hook_selection_click(sim, rules, current_objects, hover, cell)
    {
        return Some(HeadRelease::Hook(release));
    }
    let trench = || {
        hover.is_some_and(|target| {
            cursor_owner(sim, rules, current_objects, target.stable_id)
                .is_some_and(|best| ares_trench_enter(sim, rules, best, target.stable_id))
        })
    };
    (shift == Some(ObjectKeyAction::ToggleSelect) && !trench()).then_some(HeadRelease::ToggleSelect)
}

/// The left release for a selection whose every object is berserk.
/// DetermineAction's best object is then berserk too (rank 0, `0x00535435`),
/// and its What_Action base answers 0 whatever the modifiers (`0x006FFEC9`,
/// cell `0x00700609`). The class rungs decide the rest:
/// - Infantry: NoMove for a human owner (`0x0051F05E`), on the ground too
///   unless `JumpJet=` (`0x0051FAAB`). A healer's heal arm comes first
///   (`0x0051E6D8`), so a berserk healer infantryman may answer Select over
///   an ally.
/// - Unit: NoMove over an object (`0x007403EB`), 0 on the ground
///   (`0x007404B0`). A healer's repair arm still runs (`0x0073FD7C`), so a
///   berserk repair IFV may answer Select or Repair.
/// - Aircraft: 0, over an object through its cell action (`0x00417D7C`).
///
/// [`left_release`] then decides what the release does.
pub(super) fn berserk_selection_click(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    current_objects: &[u64],
    hover: Option<&HoverTargetKindWithId>,
    cell: (u16, u16),
    issuing_house: InternedId,
) -> LeftRelease {
    use crate::app::input::cursor::ActionDistanceTarget;
    let target = hover.map_or(ActionDistanceTarget::CellCentre(cell.0, cell.1), |target| {
        ActionDistanceTarget::Object(target.stable_id)
    });
    let Some((best_id, best)) = crate::app::input::cursor::select_best_for_action(
        sim,
        current_objects,
        target,
        Some(rules),
    )
    .and_then(|id| Some((id, sim.entities().get(id)?))) else {
        return LeftRelease::Dispatch;
    };
    let action_zero = match best.category() {
        EntityCategory::Aircraft => true,
        EntityCategory::Unit => hover.is_none(),
        EntityCategory::Infantry => {
            hover.is_none()
                && sim
                    .interner()
                    .object_type(best.type_ref(), rules)
                    .is_some_and(|object| object.jumpjet)
        }
        EntityCategory::Structure => false,
    };
    let action = if action_zero {
        ReleaseAction::Zero
    } else if hover.is_some_and(|target| {
        use super::HealerAction;
        use crate::sim::world::{InfantryHealAction, UnitRepairAction};
        matches!(
            super::healer_object_action(
                sim,
                rules,
                best_id,
                target.stable_id,
                issuing_house,
                OrderModifier::Normal,
                false,
            ),
            Some(
                HealerAction::Unit(UnitRepairAction::Select)
                    | HealerAction::Infantry(InfantryHealAction::Select)
            )
        )
    }) {
        ReleaseAction::Select
    } else {
        ReleaseAction::Other
    };
    left_release(sim, rules, action, hover.map(|target| target.stable_id))
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ContextOrderTarget {
    ScreenPoint(f32, f32),
    Radar(crate::render::radar::NativeRadarInputTarget),
}

/// What a click that reached the context order leaves to the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextClick {
    /// The context dispatch consumed the click, including a refused event.
    Consumed,
    /// The selection's click pick takes the click as the held keys make it:
    /// under Shift, the toggle (action 8, `0x004ABD24`).
    Pick,
    /// DetermineAction answered Select or 0, whose arm of
    /// `DisplayClass::BandBox_LeftUp` clears the selection before it selects
    /// (`0x004ABDD3`, `0x004ABDE1` → `0x004ABE4A`), so Shift toggles nothing.
    Select,
}

/// Attempt to issue a context-sensitive order at the given input target.
///
/// When `select_friendly_clicks` is true, clicks on friendly units/structures
/// fall through to the selection so the caller can treat them as selection
/// clicks instead.
pub(crate) fn try_queue_context_order(
    state: &mut AppState,
    target: ContextOrderTarget,
    select_friendly_clicks: bool,
) -> ContextClick {
    let mut selects = false;
    if queue_context_order(state, target, select_friendly_clicks, &mut selects) {
        ContextClick::Consumed
    } else if selects {
        ContextClick::Select
    } else {
        ContextClick::Pick
    }
}

/// [`try_queue_context_order`]: `true` when the dispatch consumed the click,
/// `false` when the selection takes it, with `selects` set when DetermineAction
/// answered Select or 0 ([`LeftRelease::Select`]).
fn queue_context_order(
    state: &mut AppState,
    target: ContextOrderTarget,
    select_friendly_clicks: bool,
    selects: &mut bool,
) -> bool {
    if !state.match_state.sim_runtime.is_installed() {
        return false;
    }
    let (target_rx, target_ry) = match target {
        ContextOrderTarget::ScreenPoint(x, y) => {
            crate::app::match_runtime::sim_tick::screen_point_to_world_cell(state, x, y)
        }
        ContextOrderTarget::Radar(target) => target.cell,
    };
    let world_point = match target {
        ContextOrderTarget::ScreenPoint(x, y) => Some(
            crate::app::match_runtime::sim_tick::screen_point_to_world(state, x, y),
        ),
        ContextOrderTarget::Radar(_) => None,
    };
    // Retail modifier map: Ctrl = force fire, Alt = force move,
    // Ctrl+Shift = attack move, Ctrl+Alt = guard area. Shift alone only makes a
    // Cell's What_Action answer Move — see `OrderModifier` for the full
    // derivation.
    let mut modifier = resolve_order_modifiers(
        is_ctrl_held(state),
        is_shift_held(state),
        is_alt_held(state),
    );
    // The object receiver 4D74E0 reads the force-move key itself
    // (`0x004D79EC..0x004D7A14`), whatever chord it is part of.
    let force_move_key = is_alt_held(state);
    let order_mode = state.match_state.input.queued_order_mode;
    let Some(owner) = preferred_local_owner(state) else {
        return false;
    };
    let owner_id: InternedId = state
        .match_state
        .sim_runtime
        .runtime()
        .map(|rt| &rt.simulation)
        .and_then(|s| s.interner().get(&owner))
        .unwrap_or_default();
    let translucent_viewer = state.match_state.sim_runtime.runtime().and_then(|rt| {
        crate::app::presentation::instances::translucent_fog_viewer(state, &rt.simulation)
    });

    let mut queued: Vec<CommandEnvelope> = Vec::new();
    // The one object that speaks the order-ack line. Retail lets only the first
    // entry of the selection array speak.
    let mut speaker_id: Option<u64> = None;
    let selected_ids = selected_stable_ids_in_order(
        state
            .match_state
            .sim_runtime
            .runtime()
            .map(|rt| &rt.simulation),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    // `EVA_NewRallyPointEstablished` is spoken by the click handler itself,
    // after the sim borrow below ends.
    let mut rally_announce = false;
    let mut consumed_attack_action = false;
    let mut planning_cell = None;
    let planning = state.match_state.input.planning.active();

    if let Some(rt) = state.match_state.sim_runtime.write_sim(SimWrite::Command) {
        let resources = &rt.resources;
        let overlay_registry = resources.rules.overlay_registry();
        let sim = &mut rt.simulation;
        let control = sim.control_projection();
        let execute_tick = sim.session().tick();
        let hover_object = match target {
            ContextOrderTarget::ScreenPoint(..) => {
                let (world_x, world_y) = world_point.expect("tactical input has a world point");
                hover_object_at_point(
                    crate::app::input::entity_pick::PresentedWorld {
                        sim,
                        rules: Some(&resources.rules),
                        translucent_viewer,
                    },
                    world_x,
                    world_y,
                    &owner,
                    state.match_state.sandbox_full_visibility,
                )
            }
            // Radar653C6A asks What_Action for the tracker identity returned
            // by656750, never the tactical object at the camera-clamped cell.
            ContextOrderTarget::Radar(target) => target
                .tracker_hit
                .and_then(|id| crate::app::input::entity_pick::hover_object_by_id(sim, id, &owner)),
        };
        let hover = hover_object.and_then(HoverObject::techno);
        let current_objects = projected_order_selection(
            sim,
            Some(&resources.rules),
            &selected_ids,
            hover.as_ref().map(|target| target.stable_id),
        );
        if current_objects.is_empty() {
            return false;
        }
        speaker_id = current_objects.first().copied();
        // `CurrentObjects`: DetermineAction and the gates that read the
        // selection's size see every selected object. Orders go only to the
        // objects whose ClickedAction can still send one.
        let selected_ids = sim.click_order_receivers(&current_objects, resources.rules.ares());

        let mut selected_units: Vec<u64> = Vec::new();

        for &sid in &selected_ids {
            let Some(entity) = sim.entities().get(sid) else {
                continue;
            };
            if entity.category() != EntityCategory::Structure {
                selected_units.push(sid);
            }
        }
        // Techno's Shift rung (`0x00700082` -> action 8) precedes the
        // Infantry and Unit class rows, but not the Ares hooks at their heads
        // ([`head_release`]). Let the selection consumer handle Shift over a
        // visible object;
        // bare ground still reaches its immediate Move. Berserk-only groups
        // keep their native base-action-zero route below.
        let shift_key_action = hover_object
            .and_then(|object| object_key_target(object, hover.as_ref()))
            .filter(|_| modifier == OrderModifier::Shift && !selected_ids.is_empty())
            .and_then(|(object, target)| {
                let owner = cursor_owner(sim, &resources.rules, &current_objects, object)?;
                object_key_action_with(
                    sim,
                    &control,
                    &resources.rules,
                    owner,
                    object,
                    target,
                    modifier,
                    force_move_key,
                )
            });
        // Selection__DispatchMultiUnitOrder clamps the shared Cell first
        // (`0x004AE77E`, `Clamp_To_Playfield @ 0x00586E50`); every later
        // receiver's What_Action and ClickedAction see it. Object hit-testing
        // above keeps the raw screen-derived world point.
        let (target_rx, target_ry) = sim.clamped_click_cell((target_rx, target_ry));
        let head = head_release(
            sim,
            &resources.rules,
            &current_objects,
            hover.as_ref(),
            (target_rx, target_ry),
            shift_key_action,
        );
        if head == Some(HeadRelease::ToggleSelect) {
            return false;
        }
        // Foot's Cell wrapper (`0x004DDDE0`): over the clicked Cell's centre,
        // raised to a bridge deck, that the player's map shrouds, a Foot's
        // Attack becomes Move or NoMove, which the Move rungs below answer; a
        // Building keeps it (`0x00447540`). Nothing is shrouded under the
        // sandbox's full visibility, as for the cursor.
        let full_visibility = state.match_state.sandbox_full_visibility;
        let foot_shrouded = |id: u64| {
            !full_visibility
                && sim
                    .foot_cell_shroud_action(owner_id, id, (target_rx, target_ry), &resources.rules)
                    .is_some()
        };
        // The chord test fails — and the order resolves normally — unless every
        // selected object can accept an attack-move order. The walk covers the
        // whole selection, so a selected building or aircraft kills the chord.
        if modifier == OrderModifier::AttackMove
            && !selection_can_attack_move(sim, Some(&resources.rules), &current_objects)
        {
            modifier = if force_move_key {
                OrderModifier::ForceMove
            } else {
                OrderModifier::Normal
            };
        }
        // Techno What_Action answers Self for the lone selected object clicking
        // itself (`0x007000B9..0x007000E8`) before Alt's force-move rung
        // (`0x007001E9..0x00700217`), so Alt leaves that click a plain one.
        if modifier != OrderModifier::Shift
            && hover
                .as_ref()
                .is_some_and(|target| current_objects.as_ref() == [target.stable_id])
        {
            modifier = OrderModifier::Normal;
        }
        // Foot's shroud rung makes Shift's answer Move or NoMove, as it does
        // Alt's ([`object_key_action`]), and the release dispatches it: each
        // receiver takes its own, as under Alt. Infantry's Engineer arms
        // after the rung ignore its answer, so the Engineer prefixes below
        // run as they do for Alt. The key rungs still see Shift
        // (`key_modifier`): its rung admits a player's object that is no
        // loaner, Alt's only one the player may control (vt+0xA0). A
        // selected building's rungs read the force-move key itself
        // ([`building_click_orders`]), so its Shift orders nothing.
        let key_modifier = modifier;
        if matches!(
            shift_key_action,
            Some(ObjectKeyAction::Move | ObjectKeyAction::NoMove)
        ) {
            modifier = OrderModifier::ForceMove;
        }
        let shift_click: bool = modifier == OrderModifier::Shift;
        let force_fire: bool = modifier == OrderModifier::ForceFire;
        let force_move: bool = modifier == OrderModifier::ForceMove
            || (modifier == OrderModifier::AttackMove && force_move_key);
        // Legacy capability batches below run for ordinary input. Engineer
        // overrides run in their native prefix for every chord except Shift;
        // forced C4/infiltration use the shared admitted-Attack suffix.
        let context_actions_enabled: bool = matches!(
            modifier,
            OrderModifier::Normal | OrderModifier::Shift | OrderModifier::AttackMove
        );
        // A held chord overrides the sticky sidebar order mode for this click.
        let order_mode = match modifier {
            OrderModifier::AttackMove => OrderMode::AttackMove,
            _ => order_mode,
        };

        if let Some(HeadRelease::Hook(release)) = head {
            return match release {
                // Detonate passes the ClickedAction refusal (0x4D74EC).
                Err(bombed) => {
                    if let Some(record) = sim.encode_detonate_record(bombed) {
                        queued.push(CommandEnvelope::new(
                            owner_id,
                            execute_tick,
                            Command::SynchronizedRecord { record },
                        ));
                    }
                    finish_order(state, queued, None, rally_announce, None)
                }
                // An EMP'd deployer's NoDeploy over himself: each other
                // receiver's ClickedAction asks its own What_Action over
                // him, as over any selected object; his own is NoDeploy
                // again, which orders nothing.
                Ok(LeftRelease::Dispatch) => {
                    if let Some(target) = hover.as_ref() {
                        queued.extend(
                            selected_units
                                .iter()
                                .filter(|&&id| id != target.stable_id)
                                .filter_map(|&id| {
                                    object_attack_click_command_with(
                                        sim,
                                        &control,
                                        &resources.rules,
                                        overlay_registry,
                                        owner_id,
                                        order_mode,
                                        modifier,
                                        force_move_key,
                                        id,
                                        target.stable_id,
                                    )
                                })
                                .map(|order| CommandEnvelope::new(owner_id, execute_tick, order)),
                        );
                    }
                    finish_order(state, queued, speaker_id, rally_announce, None)
                }
                Ok(LeftRelease::Nothing) => true,
                Ok(LeftRelease::Select) => {
                    *selects = true;
                    false
                }
            };
        }
        if selected_ids.is_empty() {
            return match berserk_selection_click(
                sim,
                &resources.rules,
                &current_objects,
                hover.as_ref(),
                (target_rx, target_ry),
                owner_id,
            ) {
                LeftRelease::Dispatch => {
                    finish_order(state, queued, speaker_id, rally_announce, None)
                }
                LeftRelease::Nothing => true,
                LeftRelease::Select => {
                    *selects = true;
                    false
                }
            };
        }

        let mut selected_ids = selected_ids;
        let repair_dispatch = hover.as_ref().map(|target| {
            unit_repair_branch_orders(
                sim,
                &resources.rules,
                &current_objects,
                &selected_ids,
                target,
                owner_id,
                modifier,
                order_mode,
                force_move_key,
            )
        });
        if let Some(repair_dispatch) = repair_dispatch {
            if repair_dispatch.selects {
                *selects = true;
                return false;
            }
            consumed_attack_action |= repair_dispatch.consumed;
            queued.extend(
                repair_dispatch
                    .commands
                    .into_iter()
                    .map(|payload| CommandEnvelope::new(owner_id, execute_tick, payload)),
            );
            selected_units.retain(|id| !repair_dispatch.receivers.contains(id));
            selected_ids.retain(|id| !repair_dispatch.receivers.contains(id));
            if selected_ids.is_empty() {
                let sent = finish_order(state, queued, speaker_id, rally_announce, None);
                return sent || (consumed_attack_action && !repair_dispatch.select_fallback);
            }
        }

        // An Engineer's DisarmBomb is its first action over a bombed object.
        // Every selected object takes its own action
        // (`Selection__DispatchMultiUnitOrder` 0x004AE750, object loop
        // 0x004AE829..0x004AE85A): the Engineers defuse, and over an enemy the rest go on to
        // their own orders below. Over a friend the rest have nothing to do,
        // and the dispatch happens only when the object that owns the cursor
        // is one of the Engineers; otherwise the click selects.
        if modifier != OrderModifier::Shift
            && let Some(target) = hover.as_ref()
        {
            let friendly = matches!(
                target.kind,
                HoverTargetKind::FriendlyUnit | HoverTargetKind::FriendlyStructure
            );
            let engineers = disarm_bomb_engineers(
                sim,
                &resources.rules,
                &selected_units,
                target.stable_id,
                owner_id,
            );
            let dispatched = !engineers.is_empty()
                && (!friendly
                    || cursor_owner(sim, &resources.rules, &selected_ids, target.stable_id)
                        .is_some_and(|best| engineers.contains(&best)));
            if dispatched {
                for &attacker_id in &engineers {
                    queued.push(CommandEnvelope::new(
                        owner_id,
                        execute_tick,
                        Command::Attack {
                            attacker_id,
                            target_id: target.stable_id,
                        },
                    ));
                }
                selected_units.retain(|id| !engineers.contains(id));
                selected_ids.retain(|id| !engineers.contains(id));
                if friendly || selected_units.is_empty() {
                    return finish_order(state, queued, speaker_id, rally_announce, None);
                }
            }
        }

        // Infantry51E49E's Engineer building override precedes Ctrl/Alt's
        // base action. Reuse the existing admission/command producer here;
        // later legacy capability batches must not reclassify these actors.
        if modifier != OrderModifier::Shift
            && let Some(target) = hover.as_ref()
        {
            if let Some(orders) =
                engineer_capture_orders(sim, &resources.rules, &mut selected_units, target)
            {
                consumed_attack_action = true;
                queued.extend(
                    orders
                        .into_iter()
                        .map(|payload| CommandEnvelope::new(owner_id, execute_tick, payload)),
                );
                let remaining: std::collections::BTreeSet<_> =
                    selected_units.iter().copied().collect();
                selected_ids.retain(|id| {
                    remaining.contains(id)
                        || sim
                            .entities()
                            .get(*id)
                            .is_some_and(|actor| actor.category() == EntityCategory::Structure)
                });
                if selected_ids.is_empty() {
                    return finish_order(state, queued, speaker_id, rally_announce, None)
                        || consumed_attack_action;
                }
            }
        }

        if let Some(target) = hover.as_ref() {
            let mut consumed = false;
            selected_units.retain(|&id| {
                if !vehicle_capture_action(
                    sim,
                    &control,
                    &resources.rules,
                    id,
                    target.stable_id,
                    modifier,
                    force_move_key,
                ) {
                    return true;
                }
                consumed = true;
                // Foot4D7716 asks vt+A0 before publishing the Capture event.
                if sim.local_techno_controllable_with(&control, id, &resources.rules) {
                    queued.push(CommandEnvelope::new(
                        owner_id,
                        execute_tick,
                        Command::MegaMission {
                            entity_id: id,
                            mission: crate::sim::mission::MissionType::Capture,
                            target: None,
                            destination: Some(TargetKind::Entity(target.stable_id)),
                        },
                    ));
                }
                false
            });
            if consumed {
                consumed_attack_action = true;
                let remaining: std::collections::BTreeSet<_> =
                    selected_units.iter().copied().collect();
                selected_ids.retain(|id| {
                    remaining.contains(id)
                        || sim
                            .entities()
                            .get(*id)
                            .is_some_and(|actor| actor.category() == EntityCategory::Structure)
                });
                if selected_ids.is_empty() {
                    return finish_order(state, queued, speaker_id, rally_announce, None)
                        || consumed_attack_action;
                }
            }
        }

        // Attack-capable Ctrl receivers, including fixed/occupied buildings,
        // resolve the same action as the cursor before the legacy rally/mobile
        // split. Non-attack capabilities keep their existing handlers below.
        // Over a Terrain object the cursor owner's key rungs answer first
        // ([`object_key_action`]): Alt's Move dispatches each receiver's own
        // (a building's are its own orders below). When they refuse, as for
        // a plain, Ctrl or Ctrl+Shift click, each receiver takes its
        // What_Action on the object (`terrain_object_feedback` for the
        // cursor).
        let hover_terrain = match hover_object {
            Some(HoverObject::Terrain(id)) => Some(id),
            _ => None,
        };
        let terrain_target = hover_terrain.filter(|&terrain| {
            cursor_owner(sim, &resources.rules, &current_objects, terrain).is_none_or(|owner| {
                object_key_action_with(
                    sim,
                    &control,
                    &resources.rules,
                    owner,
                    terrain,
                    None,
                    key_modifier,
                    force_move_key,
                )
                .is_none()
            })
        });
        // Ctrl+Alt over a Terrain object whose key rungs refused still holds
        // Ctrl for the later rungs ([`terrain_forced`]).
        let object_forced = force_fire || (terrain_target.is_some() && terrain_forced(modifier));
        let attack_target = match (hover_object, terrain_target) {
            (Some(HoverObject::Techno(target)), _) => TargetKind::Entity(target.stable_id),
            (_, Some(id)) => TargetKind::Entity(id),
            _ => TargetKind::Cell(target_rx, target_ry),
        };
        let enemy_object = hover.as_ref().is_some_and(|target| {
            matches!(
                target.kind,
                HoverTargetKind::EnemyUnit | HoverTargetKind::EnemyStructure
            )
        });
        let cell_shot = force_fire && matches!(attack_target, TargetKind::Cell(..));
        let attack_dispatch = if force_fire
            || terrain_target.is_some()
            || (enemy_object && context_actions_enabled)
        {
            // Foot's shroud wrapper takes a Foot's forced shot at a shrouded
            // Cell back.
            let receivers: Vec<u64> = selected_ids
                .iter()
                .copied()
                .filter(|&id| !(cell_shot && foot_shrouded(id)))
                .collect();
            attack_branch_orders(
                sim,
                &resources.rules,
                &receivers,
                attack_target,
                object_forced,
                order_mode,
                overlay_registry,
            )
        } else if let TargetKind::Cell(rx, ry) = attack_target
            && !force_fire
            && modifier != OrderModifier::GuardArea
        {
            // Each selected object's What_Action on the Cell reaches its
            // unforced Attack rung on its own (0x004AE750's object loop);
            // its tail is the forced shot's (Attack, a healing Unit's
            // AttackSupport, or Infantry's GuardArea). Other answers keep the
            // object on its Move rungs below, as does Foot's shroud wrapper.
            let rung: Vec<u64> = selected_ids
                .iter()
                .copied()
                .filter(|&id| {
                    !foot_shrouded(id)
                        && sim.cell_attack_rung_admits_with(
                            &control,
                            id,
                            (rx, ry),
                            &resources.rules,
                            Some(overlay_registry),
                        )
                        && matches!(
                            armed_action_with(
                                sim,
                                &control,
                                &resources.rules,
                                id,
                                &attack_target,
                                true,
                                Some(overlay_registry),
                            ),
                            Some(
                                AttackAction::Attack
                                    | AttackAction::AttackSupport
                                    | AttackAction::GuardArea
                            )
                        )
                })
                .collect();
            attack_branch_orders(
                sim,
                &resources.rules,
                &rung,
                attack_target,
                true,
                order_mode,
                overlay_registry,
            )
        } else {
            AttackDispatch {
                receivers: std::collections::BTreeSet::new(),
                commands: Vec::new(),
                consumed: false,
                select_fallback: false,
                selects: false,
            }
        };
        consumed_attack_action |= attack_dispatch.consumed;
        queued.extend(
            attack_dispatch
                .commands
                .into_iter()
                .map(|command| CommandEnvelope::new(owner_id, execute_tick, command)),
        );
        let attack_receivers = attack_dispatch.receivers;
        let select_fallback = attack_dispatch.select_fallback;
        if let TargetKind::Entity(target_id) = attack_target {
            let receivers: Vec<u64> = attack_receivers.iter().copied().collect();
            match object_click_release(
                sim,
                &resources.rules,
                &current_objects,
                &receivers,
                target_id,
                overlay_registry,
                object_forced,
            ) {
                LeftRelease::Select => {
                    *selects = true;
                    return false;
                }
                LeftRelease::Nothing => return true,
                LeftRelease::Dispatch => {}
            }
        }
        // A blocked stationary actor cannot suppress another selected actor's
        // accepted attack. An owner this branch took has decided the release
        // above; otherwise, with every receiver taken, Action7 alone allows
        // click-selection and Action0 consumes the click.
        let selected_ids: Vec<_> = selected_ids
            .into_iter()
            .filter(|id| !attack_receivers.contains(id))
            .collect();
        if selected_ids.is_empty() {
            let sent = finish_order(state, queued, speaker_id, rally_announce, None);
            return sent || (consumed_attack_action && !select_fallback);
        }
        selected_units.retain(|id| !attack_receivers.contains(id));
        let mobile_count = selected_units.len();
        let structure_selected = selected_ids.iter().any(|&id| {
            sim.entities()
                .get(id)
                .is_some_and(|entity| entity.category() == EntityCategory::Structure)
        });

        // Each selected building answers its own What_Action in the click
        // loops (`0x004AE829..0x004AE85A` over an object, `0x004AE95B..0x004AE990`
        // on a Cell). Beside mobile receivers, theirs decide the release below.
        if structure_selected && mobile_count > 0 {
            let (orders, announce) = building_click_orders(
                sim,
                &resources.rules,
                &selected_ids,
                owner_id,
                hover.as_ref(),
                hover_terrain,
                (target_rx, target_ry),
                modifier,
                force_move_key,
            );
            queued.extend(
                orders
                    .into_iter()
                    .map(|command| CommandEnvelope::new(owner_id, execute_tick, command)),
            );
            rally_announce |= announce;
        }

        if structure_selected && mobile_count == 0 {
            match building_selection_click(
                sim,
                &resources.rules,
                overlay_registry,
                &current_objects,
                &selected_ids,
                owner_id,
                hover.as_ref(),
                hover_terrain,
                (target_rx, target_ry),
                modifier,
                force_move_key,
            ) {
                BuildingClick::Select => {
                    *selects = true;
                    return false;
                }
                BuildingClick::Nothing => return true,
                BuildingClick::Orders(orders, announce) => {
                    queued.extend(
                        orders
                            .into_iter()
                            .map(|command| CommandEnvelope::new(owner_id, execute_tick, command)),
                    );
                    rally_announce |= announce;
                }
            }
        } else {
            // Unforced late infiltration follows the garrison/C4 prefixes.
            // Forced Attack uses the same suffix in attack_branch_orders.
            let mut consumed_building_action = false;
            if context_actions_enabled && let Some(target) = hover.as_ref() {
                if let Some(orders) = infiltrator_capture_orders(
                    sim,
                    &resources.rules,
                    Some(overlay_registry),
                    &mut selected_units,
                    target,
                    key_modifier,
                    force_move_key,
                ) {
                    consumed_building_action = true;
                    queued.extend(
                        orders
                            .into_iter()
                            .map(|command| CommandEnvelope::new(owner_id, execute_tick, command)),
                    );
                }
                if consumed_building_action && selected_units.is_empty() {
                    return finish_order(state, queued, speaker_id, rally_announce, None);
                }
            }

            // The passenger rows take Alt's Move too, and Ctrl's tail for an
            // unarmed receiver; the objects they leave take the force-move or
            // the forced click below.
            let mut consumed_entry_action = false;
            if (context_actions_enabled || force_move || force_fire)
                && let Some(target) = hover.as_ref()
                && let Some(orders) = entry_orders(
                    sim,
                    &resources.rules,
                    &current_objects,
                    &mut selected_units,
                    target,
                    key_modifier,
                    force_move_key,
                    Some(overlay_registry),
                )
            {
                consumed_building_action = true;
                consumed_entry_action = true;
                queued.extend(
                    orders
                        .into_iter()
                        .map(|command| CommandEnvelope::new(owner_id, execute_tick, command)),
                );
                if selected_units.is_empty() {
                    return finish_order(state, queued, speaker_id, rally_announce, None);
                }
            }

            // The garrison arm runs under Alt too ([`infantry_garrisons`]);
            // there the other receivers take Alt's Move below.
            if let Some(target) = hover.as_ref() {
                let orders = garrison_entry_orders(
                    sim,
                    &resources.rules,
                    &selected_units,
                    target.stable_id,
                    key_modifier,
                    force_move_key,
                );
                if !orders.is_empty() {
                    selected_units.retain(|&id| {
                        !orders.iter().any(|order| {
                            matches!(order, Command::CaptureBuilding { engineer_id, .. } if *engineer_id == id)
                        })
                    });
                    queued.extend(
                        orders
                            .into_iter()
                            .map(|command| CommandEnvelope::new(owner_id, execute_tick, command)),
                    );
                    if !force_move || selected_units.is_empty() {
                        return finish_order(state, queued, speaker_id, rally_announce, None);
                    }
                }
            }

            // C4 precedes the late infiltration action (51E9DA before51EE54).
            if context_actions_enabled && let Some(target) = hover.as_ref() {
                let c4_attackers: Vec<_> = selected_units
                    .iter()
                    .copied()
                    .filter(|&id| {
                        let attacks = infantry_arm_attacks_with(
                            sim,
                            &control,
                            &resources.rules,
                            id,
                            target.stable_id,
                            key_modifier,
                            force_move_key,
                            Some(overlay_registry),
                        );
                        infantry_c4_action_with(
                            sim,
                            &control,
                            &resources.rules,
                            id,
                            target,
                            attacks,
                        ) == Some(InfantryC4Action::Sabotage)
                    })
                    .collect();
                if !c4_attackers.is_empty() {
                    for attacker_id in c4_attackers {
                        queued.push(CommandEnvelope::new(
                            owner_id,
                            execute_tick,
                            Command::PlantC4 {
                                attacker_id,
                                target_building_id: target.stable_id,
                            },
                        ));
                    }
                    return finish_order(state, queued, speaker_id, rally_announce, None);
                }
            }

            let clicked_friendly = hover.as_ref().is_some_and(|target| {
                matches!(
                    target.kind,
                    HoverTargetKind::FriendlyUnit | HoverTargetKind::FriendlyStructure
                )
            });
            // Infantry's healer self arm (`0x0051E748`) replaces the lone
            // object's Detonate and self answers; the release dispatches.
            if context_actions_enabled
                && let Some(target) = hover.as_ref()
                && let Some(order) = lone_self_healer_guard(
                    sim,
                    &control,
                    &resources.rules,
                    &current_objects,
                    target.stable_id,
                )
            {
                queued
                    .extend(order.map(|order| CommandEnvelope::new(owner_id, execute_tick, order)));
                return finish_order(state, queued, speaker_id, rally_announce, None);
            }
            if let Some(target) = hover.as_ref()
                && self_detonates(sim, &resources.rules, &current_objects, target.stable_id)
                && let Some(record) = sim.encode_detonate_record(target.stable_id)
            {
                queued.push(CommandEnvelope::new(
                    owner_id,
                    execute_tick,
                    Command::SynchronizedRecord { record },
                ));
                return finish_order(state, queued, None, rally_announce, None);
            }
            if context_actions_enabled
                && let Some(cmd) = self_click_command(
                    sim,
                    &resources.rules,
                    Some(overlay_registry),
                    owner_id,
                    &current_objects,
                    hover.as_ref(),
                    planning,
                )
            {
                queued.push(CommandEnvelope::new(owner_id, execute_tick, cmd));
                return finish_order(state, queued, speaker_id, rally_announce, None);
            }
            if context_actions_enabled
                && hover.as_ref().is_some_and(|target| {
                    self_answers_no_deploy(
                        sim,
                        &resources.rules,
                        Some(overlay_registry),
                        owner_id,
                        &current_objects,
                        target,
                        planning,
                    )
                })
            {
                return finish_order(state, queued, speaker_id, rally_announce, None);
            }
            if context_actions_enabled
                && let Some(target) = hover.as_ref()
                && let Some(release) = lone_self_foot_release(
                    sim,
                    &resources.rules,
                    owner_id,
                    &current_objects,
                    target.stable_id,
                    order_mode,
                    force_move_key,
                )
            {
                return match release {
                    (LeftRelease::Dispatch, order) => {
                        queued.extend(
                            order.map(|order| CommandEnvelope::new(owner_id, execute_tick, order)),
                        );
                        finish_order(state, queued, speaker_id, rally_announce, None)
                    }
                    _ => true,
                };
            }
            // A Crazy Ivan bombs a friend by a plain click, when the object that
            // owns the cursor is one of the bombing Ivans.
            if clicked_friendly
                && context_actions_enabled
                && !force_move
                && let Some(target) = hover.as_ref()
            {
                let ivans = friendly_bomb_ivans(
                    sim,
                    &resources.rules,
                    &selected_units,
                    current_objects.len(),
                    target.stable_id,
                    Some(overlay_registry),
                );
                if cursor_owner(sim, &resources.rules, &selected_ids, target.stable_id)
                    .is_some_and(|best| ivans.contains(&best))
                {
                    for attacker_id in ivans {
                        queued.push(CommandEnvelope::new(
                            owner_id,
                            execute_tick,
                            Command::Attack {
                                attacker_id,
                                target_id: target.stable_id,
                            },
                        ));
                    }
                    return finish_order(state, queued, speaker_id, rally_announce, None);
                }
            }
            // The cursor owner's What_Action distinguishes a different allied
            // House from an own object. Preserve the capability routes above.
            // The shared release below, not display friendliness, decides
            // Select versus dispatch. Where the owner's action is the friendly
            // selection route it answers Select. RESIDUAL: Shift reaches here
            // only when the owner's Shift rung refused (a mission-only owner,
            // `+0x3D4`); its own action should decide, but the pick toggles.
            let friendly_unit_action = clicked_friendly
                && hover.as_ref().is_some_and(|target| {
                    cursor_owner(sim, &resources.rules, &current_objects, target.stable_id)
                        .is_some_and(|id| {
                            object_action_answers_friendly(
                                sim,
                                &current_objects,
                                id,
                                target.stable_id,
                            )
                        })
                });
            if select_friendly_clicks
                && clicked_friendly
                && context_actions_enabled
                && (!friendly_unit_action
                    || modifier == OrderModifier::Shift
                    || consumed_building_action
                    || consumed_attack_action)
            {
                return if consumed_building_action || consumed_attack_action {
                    finish_order(state, queued, speaker_id, rally_announce, None)
                } else {
                    *selects = !friendly_unit_action;
                    false
                };
            }

            // Alt over an object: Techno's force-move rung answers Move for a
            // player's controllable receiver (`0x007001E9..0x00700217`), so
            // nothing under the cursor is a target, and the object receiver
            // 4D74E0 orders the object or a Cell near it
            // ([`Simulation::object_move_click`]). Rungs that answer first
            // (Self) or rewrite it (the passenger rows, [`entry_orders`]) are
            // handled above. The class rungs noted above are not asked here
            // (#605). In a campaign Foot's shroud rung makes the answer Move
            // or NoMove, Shift's too ([`object_key_action`]); each receiver
            // takes its own, and the release dispatches either, so it selects
            // nothing (`0x004ABE88`). An enemy the player cannot see is no
            // object to click, as for the cursor: the Cell under it takes the
            // click.
            let force_move_object = hover_object
                .and_then(|object| object_key_target(object, hover.as_ref()))
                .filter(|&(object, target)| {
                    // A Techno's Ctrl+Alt Move goes through its guard click.
                    (force_move || target.is_none())
                        && cursor_owner(sim, &resources.rules, &current_objects, object)
                            .is_some_and(|owner| {
                                matches!(
                                    object_key_action_with(
                                        sim,
                                        &control,
                                        &resources.rules,
                                        owner,
                                        object,
                                        target,
                                        key_modifier,
                                        force_move_key,
                                    ),
                                    Some(ObjectKeyAction::Move | ObjectKeyAction::NoMove)
                                )
                            })
                });
            let force_move_target = force_move_object.map(|(object, _)| object);
            consumed_attack_action |= force_move_target.is_some();
            // When the cursor owner's key rungs refuse the Techno (vt+0xA0,
            // `0x00700191..0x00700217`), its What_Action takes the attack
            // rung (`0x0070021A`), Ctrl still forcing it under Ctrl+Alt, and
            // that answer decides the release, as for a plain click.
            let owner_key_refused = |target_id| {
                cursor_owner(sim, &resources.rules, &current_objects, target_id).is_some_and(
                    |owner| {
                        object_key_action_with(
                            sim,
                            &control,
                            &resources.rules,
                            owner,
                            target_id,
                            Some(target_id),
                            key_modifier,
                            force_move_key,
                        )
                        .is_none()
                    },
                )
            };

            // Scroll693271..69328B gives the same picked object to action
            // and release. Re-picking with an empty viewer for Ctrl lost the
            // local shroud and sensor policy; retain the original pick.
            let attack_target = hover
                .as_ref()
                .filter(|target| {
                    !force_move
                        && (force_fire
                            || (select_friendly_clicks
                                && friendly_unit_action
                                && matches!(
                                    modifier,
                                    OrderModifier::Normal | OrderModifier::AttackMove
                                ))
                            || modifier == OrderModifier::GuardArea
                            || matches!(
                                target.kind,
                                HoverTargetKind::EnemyUnit | HoverTargetKind::EnemyStructure
                            ))
                        || (force_move
                            && key_modifier != OrderModifier::Shift
                            && force_move_object.is_none()
                            && target.kind != HoverTargetKind::HiddenEnemy
                            && owner_key_refused(target.stable_id))
                })
                .map(|target| target.stable_id);
            // Foot4DDDE0's hidden-cell wrapper rewrites a nonzero action
            // except the planning action 0x33 to Move when MoveToShroud and the playfield
            // admit it, otherwise NoMove (4DDE8F..4DDEB0). It does not preserve
            // Attack5. The ordinary MTNK fall-through below takes Move; other
            // class/shroud combinations remain part of the shared-cell audit.
            if let Some(target_id) = attack_target {
                // The cursor owner's attack rung, forced or not, decides
                // the release; its guard or Alt Move rung dispatches.
                let release_forced = match modifier {
                    OrderModifier::Normal | OrderModifier::AttackMove => Some(false),
                    OrderModifier::ForceFire => Some(true),
                    OrderModifier::GuardArea => owner_key_refused(target_id).then_some(true),
                    OrderModifier::ForceMove => Some(false),
                    OrderModifier::Shift => None,
                };
                // Entries already dispatched the release.
                if !consumed_entry_action && let Some(forced) = release_forced {
                    // A selected building is a receiver as well: when it
                    // owns the cursor, its own answer decides the release.
                    match object_click_release(
                        sim,
                        &resources.rules,
                        &current_objects,
                        &selected_ids,
                        target_id,
                        overlay_registry,
                        forced,
                    ) {
                        LeftRelease::Select => {
                            *selects = true;
                            return false;
                        }
                        LeftRelease::Nothing => return true,
                        LeftRelease::Dispatch => {}
                    }
                }
                // A dispatched release selects nothing, whatever each
                // receiver sends (`0x004ABE88`).
                consumed_attack_action = true;
            }
            if attack_target.is_none() && force_move_target.is_none() {
                planning_cell = Some((target_rx, target_ry));
            }
            // VERA formation lock: a ground move, queued and forced ones
            // too, sends each locked unit to its own place around the
            // clicked Cell.
            let locked_cells = (!planning
                && attack_target.is_none()
                && force_move_target.is_none()
                && force_move_object.is_none())
            .then(|| {
                locked_move_cells(
                    sim,
                    &state.match_state.input.formation_locks,
                    &selected_units,
                    (target_rx, target_ry),
                )
            })
            .flatten();
            for stable_id in selected_units {
                let payload = if let Some(target_id) = attack_target {
                    let Some(payload) = object_attack_click_command_with(
                        sim,
                        &control,
                        &resources.rules,
                        overlay_registry,
                        owner_id,
                        order_mode,
                        modifier,
                        force_move_key,
                        stable_id,
                        target_id,
                    ) else {
                        continue;
                    };
                    payload
                } else if let Some((_, Some(target_id))) = force_move_object
                    && key_modifier != OrderModifier::Shift
                {
                    // Alt over a Techno: each receiver's own key rungs, and
                    // the attack rung for one whose rungs refuse
                    // ([`object_attack_click_command`]).
                    let Some(payload) = object_attack_click_command_with(
                        sim,
                        &control,
                        &resources.rules,
                        overlay_registry,
                        owner_id,
                        order_mode,
                        key_modifier,
                        force_move_key,
                        stable_id,
                        target_id,
                    ) else {
                        continue;
                    };
                    payload
                } else if modifier == OrderModifier::GuardArea
                    && force_move_target.is_none()
                    && cell_guard_area_action_with(
                        sim,
                        &control,
                        &resources.rules,
                        stable_id,
                        Some(owner_id),
                        (target_rx, target_ry),
                    )
                {
                    // Ctrl+Alt over bare ground uses the same rung as hover;
                    // a Foot over shroud takes the Move path below.
                    Command::Guard {
                        entity_id: stable_id,
                        target: Some(TargetKind::Cell(target_rx, target_ry)),
                    }
                } else if force_move_target.is_none()
                    && let Some(order) = cell_harvest_order(
                        sim,
                        &control,
                        &resources.rules,
                        owner_id,
                        stable_id,
                        (target_rx, target_ry),
                        shift_click,
                    )
                {
                    // The receiver's own Cell action; under Ctrl too once it
                    // has no weapon to force-fire (`0x007008C5`).
                    order
                } else if force_fire && !foot_shrouded(stable_id) {
                    // Unarmed Cell-click fall-through resolves the original
                    // clicked Cell before committing its Move event.
                    push_move_flash(
                        &mut queued,
                        sim,
                        &control,
                        &resources.rules,
                        owner_id,
                        execute_tick,
                        &current_objects,
                        stable_id,
                        (target_rx, target_ry),
                        shift_click,
                    );
                    let Some(goal) = crate::app::input::commands::ordinary_cell_move_goal_with(
                        sim,
                        &control,
                        &resources.rules,
                        owner_id,
                        stable_id,
                        (target_rx, target_ry),
                        shift_click,
                    ) else {
                        continue;
                    };
                    Command::Move {
                        entity_id: stable_id,
                        target_rx: goal.0,
                        target_ry: goal.1,
                    }
                } else {
                    match order_mode {
                        OrderMode::Move | OrderMode::AttackMove => {
                            if force_move_target.is_none() {
                                push_move_flash(
                                    &mut queued,
                                    sim,
                                    &control,
                                    &resources.rules,
                                    owner_id,
                                    execute_tick,
                                    &current_objects,
                                    stable_id,
                                    (target_rx, target_ry),
                                    shift_click,
                                );
                            }
                            let destination = match force_move_object {
                                Some((object, target)) => {
                                    // The receiver's own key answer.
                                    let action = match object_key_action_with(
                                        sim,
                                        &control,
                                        &resources.rules,
                                        stable_id,
                                        object,
                                        target,
                                        key_modifier,
                                        force_move_key,
                                    ) {
                                        Some(ObjectKeyAction::NoMove) => {
                                            crate::sim::world::MoveAction::NoMove
                                        }
                                        _ => crate::sim::world::MoveAction::Move,
                                    };
                                    crate::app::input::commands::object_move_destination_with(
                                        sim,
                                        &control,
                                        &resources.rules,
                                        owner_id,
                                        stable_id,
                                        object,
                                        action,
                                        force_move_key,
                                    )
                                }
                                None => crate::app::input::commands::ordinary_cell_move_goal_with(
                                    sim,
                                    &control,
                                    &resources.rules,
                                    owner_id,
                                    stable_id,
                                    locked_cells
                                        .as_ref()
                                        .and_then(|cells| cells.get(&stable_id))
                                        .copied()
                                        .unwrap_or((target_rx, target_ry)),
                                    shift_click,
                                )
                                .map(|(rx, ry)| TargetKind::Cell(rx, ry)),
                            };
                            let Some(destination) = destination else {
                                continue;
                            };
                            move_click_command(
                                sim,
                                &resources.rules,
                                order_mode,
                                stable_id,
                                destination,
                            )
                        }
                    }
                };
                queued.push(CommandEnvelope::new(owner_id, execute_tick, payload));
            }
        }
    }

    finish_order(state, queued, speaker_id, rally_announce, planning_cell) || consumed_attack_action
}
