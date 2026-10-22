//! Cursor feedback analysis and software cursor frame selection.
//!
//! Determines what cursor state to show based on hover target, selection,
//! and game mode. Split from `presentation::ui_overlays` for file-size limits.

use std::time::Instant;

use crate::app::AppState;
use crate::app::input::commands::preferred_local_owner_name;
use crate::app::input::entity_pick::HoverObject;
use crate::app::presentation::instances::CellVisibilityState;
use crate::app::types::{
    CursorFeedbackKind, CursorId, HoverTargetKind, ScrollDir, SoftwareCursorFrame,
    SoftwareCursorSequence,
};
use crate::sim::combat;

/// WWMouse's display depth, separate from Scenario's authoritative input
/// lock. Lock684240 hides only at depth0; Unlock684290 unwinds negative
/// depth only when the previous Scenario byte is1. Software presentation
/// uses the same visible/hidden result without Win32 ShowCursor counting.
/// Executed controls: trigger_action_values.json input_history (eight
/// histories, including repeated calls and initial depths-2/-3/+1). The
/// in-game menus' `State_Machine` (`0x0048C8B0`) shows it on entry and hides
/// it once on exit while the Scenario lock is set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CursorVisibility {
    level: i32,
}

impl CursorVisibility {
    pub(crate) fn level(self) -> i32 {
        self.level
    }

    /// Supply a native WWMouse prior in tests. LoadMission559D60 restores the
    /// pre-load depth; production keeps this client's existing counter across
    /// the view transition. Every i32 is a valid stored WWMouse depth.
    #[cfg(test)]
    pub(crate) fn restore_level(&mut self, level: i32) {
        self.level = level;
    }

    pub(crate) fn reset(&mut self) {
        self.level = 0;
    }

    pub(crate) fn is_visible(self) -> bool {
        self.level >= 0
    }

    /// `State_Machine` entry shows the mouse until `GetShowLevel` (`+0x28`)
    /// reads 0 (`ShowCursor` `+0x10`, `0x0048C928..0x0048C94F`).
    pub(crate) fn show_for_menus(&mut self) {
        self.level = 0;
    }

    /// `State_Machine` exit (`0x0048CC4B..0x0048CC5D`) calls `HideCursor`
    /// (`+0xC`) once when the Scenario's input lock (`+0x35A2`) is set, read
    /// after any load the menus made: `LoadOptionsClass::LoadMission`
    /// (`0x00559D60`) restores the depth it was called at.
    pub(crate) fn leave_menus(&mut self, locked: bool) {
        if locked {
            self.level -= 1;
        }
    }

    pub(crate) fn apply_input_lock(&mut self, locked: bool, was_locked: bool) {
        if locked {
            if self.level == 0 {
                self.level = -1;
            }
        } else if was_locked {
            // Original repeatedly calls Show7B9750 until depth reaches0.
            // That owner also clamps positive retained levels to0; only
            // terminal visibility has a software-cursor effect here.
            self.level = 0;
        }
    }
}

pub(crate) fn current_cursor_feedback_kind(state: &AppState) -> Option<CursorFeedbackKind> {
    // State_Machine entry (`StateMachine__EnterPause` 0x00683EB0) sets the
    // normal arrow (`SetCursor(0)`) for the whole modal; the in-match 0xCF
    // shows it over the battlefield.
    if state.match_state.match_presentation.in_game_menu.is_open() {
        return None;
    }
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
    if state.match_state.input.minimap_dragging
        || crate::app::presentation::sidebar_render::is_cursor_over_minimap(state)
    {
        // The radar's own action draws its row's mini set
        // ([`crate::app::input::radar::cursor`]).
        return Some(CursorFeedbackKind::Minimap(
            crate::app::input::radar::cursor(state),
        ));
    }
    if current_sidebar_view_hit(state) {
        return None;
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
    // Superweapon targeting cursor takes precedence over building placement.
    // Sidebar/minimap hits are already short-circuited above, so the SW reticle
    // only renders on the tactical map. DetermineAction answers the armed
    // superweapon only with nothing selected (`0x00692610..0x00692673`); with
    // a selection the best object's What_Action answers below.
    let pointer_mode = state.match_state.input.tactical_pointer_mode(
        &state.match_state.match_presentation.sidebar_gadget_state,
        selected.is_empty(),
    );
    if let super::state::TacticalPointerMode::SuperWeapon(section) = pointer_mode {
        if let Some(runtime) = state.match_state.sim_runtime.as_ref()
            && let Some(owner) = preferred_local_owner_name(state)
                .and_then(|name| runtime.simulation.interner().get(&name))
            && let Some(sw_type) = runtime.simulation.interner().get(section)
        {
            let cell = crate::app::match_runtime::sim_tick::screen_point_to_world_cell(
                state,
                state.match_state.input.cursor_x,
                state.match_state.input.cursor_y,
            );
            if runtime
                .simulation
                .super_weapon_target_check(&runtime.resources.rules, owner, sw_type, cell, false)
                .is_err()
            {
                return Some(CursorFeedbackKind::SuperWeaponTarget(CursorId::Default));
            }
        }
        if !crate::app::input::commands::super_weapon_pointer_target_at_cursor_is_valid(
            state, section,
        ) {
            return Some(CursorFeedbackKind::SuperWeaponTarget(
                CursorId::NoForceShield,
            ));
        }
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
    let sim = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)?;
    // DetermineAction's no-selection path keeps Repair/Sell only when no
    // armed Super overrides it. Use the same priority as the release owner;
    // its target admission still supplies wrench/dollar or refusal feedback.
    if matches!(pointer_mode, super::state::TacticalPointerMode::RepairSell) {
        let repair = state
            .match_state
            .match_presentation
            .sidebar_gadget_state
            .repair_mode_on;
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
    // No early return for an empty selection here. `242c79ad0` carried
    // `if selected.is_empty() { return None; }` (no comment, no native
    // backing), which made the no-selection arm below unreachable and showed
    // the plain arrow over every hovered object. gamemd resolves the object
    // under the cursor for an empty selection too — DetermineAction's
    // `0x00692673` arm answers action 7, row 17 (Select) — so the arm below
    // answers instead. See `no_selection_cursor`.
    // PlanMgr63AC41 selects native action0 over a planning node.
    if crate::app::input::planning_mode::node_at_cursor(state).is_some() {
        return Some(CursorFeedbackKind::Default);
    }
    let (world_x, world_y) = crate::app::match_runtime::sim_tick::screen_point_to_world(
        state,
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    let cell = crate::app::match_runtime::sim_tick::screen_point_to_world_cell(
        state,
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    // The cursor update asks the Cell's What_Action with mouse-over set
    // (`0x0069300B`).
    let query = ActionQuery::new(state, sim, &selected, true);
    if !query.cell_visible(cell) {
        return Some(query.hidden_cell(cell));
    }
    let owner = preferred_local_owner_name(state).unwrap_or_else(|| "Americans".to_string());
    let hover_object = crate::app::input::entity_pick::hover_object_at_point(
        crate::app::input::entity_pick::presented(state, sim),
        world_x,
        world_y,
        &owner,
        state.match_state.sandbox_full_visibility,
    );
    if selected.is_empty() {
        return no_selection_cursor(hover_object.and_then(HoverObject::techno).as_ref());
    }
    query.selection_action(cell, hover_object)
}

/// What the selection's best object is asked with besides the Cell and the
/// object: DetermineAction's (`0x00692610`) for the tactical cursor, and
/// RadarClass::Action's (`0x006539D0`) for the radar. Both resolve one object
/// for the whole selection (`SelectBestObjectForAction @ 0x005353D0`) and
/// answer its What_Action, which reads the modifier keys itself.
pub(crate) struct ActionQuery<'a> {
    pub(crate) sim: &'a crate::sim::world::Simulation,
    pub(crate) rules: Option<&'a crate::rules::ruleset::RuleSet>,
    pub(crate) overlays: Option<&'a crate::rules::overlay_types::OverlayTypeRegistry>,
    pub(crate) viewer: Option<crate::sim::intern::InternedId>,
    pub(crate) selected: &'a [u64],
    pub(crate) modifier: super::context_order::OrderModifier,
    pub(crate) force_move_key: bool,
    pub(crate) order_mode: crate::app::types::OrderMode,
    pub(crate) planning: bool,
    /// The Cell What_Action's mouse-over argument
    /// ([`crate::sim::world::CellActionQuery::mouse_over`]): 1 from the
    /// cursor update (`0x0069300B`), 0 from the radar (`0x00653C9A`).
    pub(crate) mouse_over: bool,
    /// The sandbox's full-visibility switch, under which no Cell is hidden.
    pub(crate) full_visibility: bool,
}

impl<'a> ActionQuery<'a> {
    pub(crate) fn new(
        state: &'a AppState,
        sim: &'a crate::sim::world::Simulation,
        selected: &'a [u64],
        mouse_over: bool,
    ) -> Self {
        let owner = preferred_local_owner_name(state).unwrap_or_else(|| "Americans".to_string());
        let mut modifier = crate::app::input::context_order::resolve_order_modifiers(
            crate::app::input::dispatch::is_ctrl_held(state),
            crate::app::input::dispatch::is_shift_held(state),
            crate::app::input::dispatch::is_alt_held(state),
        );
        let force_move_key = crate::app::input::dispatch::is_alt_held(state);
        if modifier == super::context_order::OrderModifier::AttackMove
            && !super::context_order::selection_can_attack_move(sim, state.rules(), selected)
        {
            modifier = if force_move_key {
                super::context_order::OrderModifier::ForceMove
            } else {
                super::context_order::OrderModifier::Normal
            };
        }
        Self {
            sim,
            rules: state.rules(),
            overlays: state.overlay_registry(),
            viewer: sim.interner().get(&owner),
            selected,
            modifier,
            force_move_key,
            order_mode: state.match_state.input.queued_order_mode,
            planning: state.match_state.input.planning.active(),
            mouse_over,
            full_visibility: state.match_state.sandbox_full_visibility,
        }
    }

    /// Whether the local player sees `cell`. Over a shrouded or fogged Cell
    /// no object is offered and [`Self::hidden_cell`] answers.
    pub(crate) fn cell_visible(&self, (rx, ry): (u16, u16)) -> bool {
        crate::app::presentation::instances::cell_visibility_for_local_owner(
            self.viewer,
            Some(self.sim.fog()),
            rx,
            ry,
            self.full_visibility,
            self.sim.resolved_terrain(),
        ) == CellVisibilityState::Visible
    }

    /// The action over a Cell the local player does not see: the empty-cell
    /// ladder of the object resolved for its centre, where Foot's shroud
    /// rung bars a type without MoveToShroud.
    pub(crate) fn hidden_cell(&self, cell: (u16, u16)) -> CursorFeedbackKind {
        let best_id = select_best_for_action(
            self.sim,
            self.selected,
            ActionDistanceTarget::CellCentre(cell.0, cell.1),
            self.rules,
        );
        self.empty_cell(best_id, cell)
    }

    /// The best selected object's What_Action on `hover_object`, or on the
    /// Cell `(cell_rx, cell_ry)` when nothing is picked, as the cursor shows
    /// it: Move and Attack become AttackMove while the attack-move chord or
    /// mode is active (`0x0070F0B0`, which `SetCursorFromAction` calls for
    /// the tactical map and the radar alike). `None` is Action0, the arrow.
    /// The selection is not empty.
    pub(crate) fn selection_action(
        &self,
        (cell_rx, cell_ry): (u16, u16),
        hover_object: Option<HoverObject>,
    ) -> Option<CursorFeedbackKind> {
        let sim = self.sim;
        let selected = self.selected;
        let modifier = self.modifier;
        let force_move_key = self.force_move_key;
        let hover = hover_object.and_then(HoverObject::techno);
        // gamemd's DetermineAction resolves ONE object for the whole selection and
        // shows that object's action, for the cell branch as well as the object
        // branch. Resolve it before the split so every branch below reads the same
        // object instead of taking `.any()` over the selection.
        let action_target = hover_object.map_or(
            ActionDistanceTarget::CellCentre(cell_rx, cell_ry),
            |object| ActionDistanceTarget::Object(object.id()),
        );
        let best_id = select_best_for_action(sim, selected, action_target, self.rules);
        let attack_move = modifier == super::context_order::OrderModifier::AttackMove
            || self.order_mode == crate::app::types::OrderMode::AttackMove;

        if let Some(hover) = hover.as_ref() {
            let kind = capability_cursor_for_hover(
                sim,
                selected,
                best_id,
                hover,
                self.rules,
                self.overlays,
                modifier,
                force_move_key,
                self.planning,
            );
            return Some(promote_attack_move_feedback(kind, attack_move));
        }

        // Shift, Alt and Ctrl+Alt over a Terrain object take the object's key
        // rungs ([`super::context_order::object_key_action`]).
        if let Some(HoverObject::Terrain(terrain_id)) = hover_object
            && let (Some(id), Some(rules)) = (best_id, self.rules)
            && let Some(action) = super::context_order::object_key_action(
                sim,
                rules,
                id,
                None,
                modifier,
                force_move_key,
            )
        {
            let kind = match action {
                super::context_order::ObjectKeyAction::ToggleSelect => CursorFeedbackKind::Select,
                super::context_order::ObjectKeyAction::GuardArea => CursorFeedbackKind::GuardArea,
                super::context_order::ObjectKeyAction::Move => {
                    key_move_feedback(sim, rules, id, terrain_id)
                }
            };
            return Some(promote_attack_move_feedback(kind, attack_move));
        }
        // When they refuse, as under a plain, Ctrl or Ctrl+Shift hover, the
        // object's later rungs answer.
        let forced = modifier == crate::app::input::context_order::OrderModifier::ForceFire;
        let terrain_forced = super::context_order::terrain_forced(modifier);
        if let Some(HoverObject::Terrain(terrain_id)) = hover_object
            && let Some(feedback) = best_id.and_then(|id| {
                terrain_object_feedback(
                    sim,
                    id,
                    terrain_id,
                    terrain_forced,
                    selected.len(),
                    self.rules,
                    self.overlays,
                )
            })
        {
            return Some(promote_attack_move_feedback(feedback, attack_move));
        }
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
        if forced {
            let target = combat::TargetKind::Cell(cell_rx, cell_ry);
            if let Some(feedback) = best_id.and_then(|id| {
                force_fire_feedback(sim, id, &target, selected.len(), self.rules, self.overlays)
            }) {
                return feedback;
            }
        }
        if let Some(feedback) = unforced_cell_attack_feedback(
            sim,
            self.viewer,
            best_id,
            (cell_rx, cell_ry),
            modifier,
            selected.len(),
            self.rules,
            self.overlays,
        ) {
            return Some(feedback);
        }
        Some(self.empty_cell(best_id, (cell_rx, cell_ry)))
    }

    /// The cursor over a Cell with no object offered: the resolved object's
    /// What_Action on the Cell, a Unit's harvest rungs included.
    fn empty_cell(&self, best_id: Option<u64>, cell: (u16, u16)) -> CursorFeedbackKind {
        empty_cell_cursor(
            self.sim,
            self.rules,
            self.overlays,
            self.viewer,
            best_id,
            cell,
            self.modifier,
            self.order_mode,
            self.mouse_over,
        )
    }
}

/// A hover over a Terrain object that no key rung answered: the resolved object's
/// What_Action on it ([`attack_action`](super::context_order::attack_action)),
/// then Infantry's weapon cursors and Crazy Ivan arm (`0x0051EA59..`).
/// Unforced, the Techno rung refuses a Terrain object that is not a legal
/// target, and every class's tail turns that 0 into its own answer: NoMove
/// for a player's Unit (`0x0073FD50`) or Infantryman (`0x0051F05E`), an
/// Aircraft's Cell action under the object (`0x00417D7C`), and 0 for a
/// Building (`0x00447210`), which shows no rally. With Ctrl an armed object
/// answers Attack, which an `Immune=` type turns into NoMove.
///
/// Shift, Alt and Ctrl+Alt answer before it
/// ([`super::context_order::object_key_action`]).
///
/// A player's Crazy Ivan with Ctrl answers IvanBomb (`0x0051EB48..0x0051EB66`)
/// over every retail Terrain object
/// ([`super::context_order::ivan_bomb_object_refused`]).
///
/// Ctrl+Shift and the sticky attack-move mode take the unforced answer,
/// whose Move ClickedMission promotes to attack-move (`0x006FFC09..0x006FFC1E`).
pub(crate) fn terrain_object_feedback(
    sim: &crate::sim::world::Simulation,
    actor_id: u64,
    terrain_id: u64,
    forced: bool,
    selected_count: usize,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> Option<CursorFeedbackKind> {
    let target = combat::TargetKind::Entity(terrain_id);
    let action = super::context_order::attack_action(
        sim,
        rules?,
        actor_id,
        &target,
        forced,
        overlay_registry,
    );
    let kind = action_feedback(
        sim,
        actor_id,
        &target,
        action,
        selected_count,
        rules,
        overlay_registry,
    )
    .unwrap_or(CursorFeedbackKind::Default);
    let is_ivan = sim
        .entities()
        .get(actor_id)
        .and_then(|actor| rules.and_then(|r| sim.interner().object_type(actor.type_ref(), r)))
        .is_some_and(|object| object.ivan);
    Some(
        infantry_weapon_feedback(sim, rules, actor_id, terrain_id, kind).unwrap_or_else(|| {
            ivan_attack_feedback(kind, is_ivan, || {
                rules.is_none_or(|rules| {
                    super::context_order::ivan_bomb_object_refused(sim, rules, terrain_id)
                })
            })
        }),
    )
}

/// Techno What_Action on a Cell's unforced Attack rung
/// ([`Simulation::cell_attack_rung_admits`]) precedes its Shift and Move
/// rungs, so it answers under every chord but Ctrl (the forced rung above)
/// and Ctrl+Alt. Its tail is the forced shot's; an answer other than Attack
/// or a healing Unit's AttackSupport (the arrow, `0x007404FD`) falls through
/// to the Move rungs. Foot's shroud wrapper turns it back into
/// Move or NoMove over a shrouded Cell (`0x004DDE64..0x004DDEBE`); the
/// Building wrapper (`0x00447540`) keeps a building's Attack there.
///
/// [`Simulation::cell_attack_rung_admits`]: crate::sim::world::Simulation::cell_attack_rung_admits
#[allow(clippy::too_many_arguments)]
fn unforced_cell_attack_feedback(
    sim: &crate::sim::world::Simulation,
    viewer: Option<crate::sim::intern::InternedId>,
    best_id: Option<u64>,
    cell: (u16, u16),
    modifier: crate::app::input::context_order::OrderModifier,
    selected_count: usize,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> Option<CursorFeedbackKind> {
    use crate::app::input::context_order::OrderModifier;
    let id = best_id?;
    let foot = sim
        .entities()
        .get(id)
        .is_some_and(|entity| entity.category() != crate::map::entities::EntityCategory::Structure);
    if matches!(
        modifier,
        OrderModifier::ForceFire | OrderModifier::GuardArea
    ) || (foot
        && viewer.is_some_and(|viewer| sim.clicked_cell_is_shrouded(viewer, cell) == Ok(true)))
        || !sim.cell_attack_rung_admits(id, cell, rules?, overlay_registry)
    {
        return None;
    }
    let target = combat::TargetKind::Cell(cell.0, cell.1);
    // The arrow here is a healing Unit's AttackSupport, which stops the rungs.
    match force_fire_feedback(sim, id, &target, selected_count, rules, overlay_registry)? {
        Some(
            feedback @ (CursorFeedbackKind::EnemyUnit
            | CursorFeedbackKind::EnemyOutOfRange
            | CursorFeedbackKind::Default),
        ) => Some(feedback),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn empty_cell_cursor(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    viewer: Option<crate::sim::intern::InternedId>,
    best_id: Option<u64>,
    cell: (u16, u16),
    modifier: crate::app::input::context_order::OrderModifier,
    order_mode: crate::app::presentation::render::OrderMode,
    mouse_over: bool,
) -> CursorFeedbackKind {
    use crate::sim::world::MoveAction;
    if modifier == super::context_order::OrderModifier::GuardArea
        && let (Some(rules), Some(id)) = (rules, best_id)
        && super::context_order::cell_guard_area_action(sim, rules, id, viewer, cell)
    {
        return CursorFeedbackKind::GuardArea;
    }
    let action = what_action_on_cell(
        sim, viewer, best_id, cell, modifier, rules, registry, mouse_over,
    );
    match action {
        MoveAction::None => return CursorFeedbackKind::Default,
        MoveAction::NoMove => return CursorFeedbackKind::Invalid,
        MoveAction::Move | MoveAction::Harvest => {}
    }
    // The one resolved object's harvest rungs, not the whole selection's.
    if action == MoveAction::Harvest {
        return CursorFeedbackKind::Harvest;
    }
    let shrouded =
        viewer.is_some_and(|viewer| sim.clicked_cell_is_shrouded(viewer, cell) == Ok(true));
    // Building447563..4475EE: an UndeploysInto resource gatherer/destination
    //shows Harvest over mapped Tiberium land; its click still archives + sells.
    let refinery_harvest = !shrouded
        && best_id.is_some_and(|id| {
            sim.entities().get(id).is_some_and(|entity| {
                entity.category() == crate::map::entities::EntityCategory::Structure
                    && rules
                        .and_then(|rules| sim.interner().object_type(entity.type_ref(), rules))
                        .is_some_and(|object| {
                            object.undeploys_into.is_some()
                                && object.resource_gatherer
                                && object.resource_destination
                        })
                    && sim.resolved_terrain().is_some_and(|terrain| {
                        let cells =
                            crate::map::resolved_terrain::NativeCellQuery::isolated(terrain);
                        cells.land_type(cells.lookup((cell.0 as i16, cell.1 as i16))) == 5
                    })
            })
        });
    if refinery_harvest {
        return CursorFeedbackKind::Harvest;
    }
    match order_mode {
        crate::app::presentation::render::OrderMode::Move
            if modifier == super::context_order::OrderModifier::AttackMove =>
        {
            CursorFeedbackKind::AttackMove
        }
        crate::app::presentation::render::OrderMode::Move => CursorFeedbackKind::Move,
        crate::app::presentation::render::OrderMode::AttackMove => CursorFeedbackKind::AttackMove,
    }
}

/// The resolved object's What_Action on an empty Cell. The cursor update
/// asks it with `mouse_over`: DetermineAction (`0x00692610`) from
/// `0x0069300B` sets the mouse-over argument, so the sim's
/// [`Simulation::cell_move_action`] never asks Can_Enter_Cell there and a
/// Cell the click will redirect shows Move. The radar asks without it
/// (`0x00653C9A`). A selected UndeploysInto building drops that argument and
/// asks Can_Enter_Cell even here (`0x004475A0`).
/// Without a local House, a resolved object or rules, or for a query the sim
/// cannot answer (no map cells), the cursor keeps Move: VERA-internal, as a
/// live gamemd session always has a map and a selection here.
///
/// [`Simulation::cell_move_action`]: crate::sim::world::Simulation::cell_move_action
#[allow(clippy::too_many_arguments)]
fn what_action_on_cell(
    sim: &crate::sim::world::Simulation,
    viewer: Option<crate::sim::intern::InternedId>,
    best_id: Option<u64>,
    cell: (u16, u16),
    modifier: crate::app::input::context_order::OrderModifier,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    mouse_over: bool,
) -> crate::sim::world::MoveAction {
    use crate::sim::world::{CellActionQuery, MoveAction};
    // gamemd only asks the selected object's What_Action when a selection exists:
    // `DisplayClass__DetermineAction @ 0x00692610` returns the vtable `+0x70` answer
    // from inside `if (g_CurrentObjects_Count != 0)`, so hovering a bare Cell with an
    // empty selection yields no action and keeps the plain arrow.
    let Some(id) = best_id else {
        return MoveAction::None;
    };
    let (Some(viewer), Some(rules)) = (viewer, rules) else {
        return MoveAction::Move;
    };
    let query = CellActionQuery {
        mouse_over,
        shift: modifier == crate::app::input::context_order::OrderModifier::Shift,
    };
    sim.cell_move_action(viewer, id, cell, query, rules, registry)
        .unwrap_or(MoveAction::Move)
}

/// Determine the cursor feedback kind for a hover target, checking ObjectType
/// capability flags from rules.ini before falling back to the generic attack/select logic.
///
/// The original engine picks a single "best" selected object
/// ([`select_best_for_action`], `SelectBestObjectForAction`) and uses that
/// object's `What_Action_OnObject` to determine the cursor for the entire
/// group.
///
/// Priority (highest first):
/// 1. Self-hover: deploy/unload capabilities or an own factory's primary action.
/// 2. Infantry class capabilities, then admitted weapon cursor overrides.
/// 3. Engineer capturing: selected Engineer hovering capturable enemy building.
/// 4. Engineer repairing: selected Engineer hovering damaged friendly building.
/// 5. Entry (Enter/NoEnter): a Unit's Select over a friendly building whose
///    CAN_LOAD admits it, an Infantryman's over a Grinder, either's over a
///    friendly passenger carrier, and an Aircraft's NoEnter over a
///    `UnitRepair=` building.
/// 6. Infantry garrisoning: selected Occupier infantry hovering friendly CanBeOccupied building.
/// 7. Admitted friendly Attack: consume the shared Techno grant and class action.
/// 9. Generic friendly/enemy/in-range/out-of-range fallback.
pub(super) fn capability_cursor_for_hover(
    sim: &crate::sim::world::Simulation,
    selected: &[u64],
    best_id: Option<u64>,
    hover: &crate::app::input::entity_pick::HoverTargetKindWithId,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    modifier: super::context_order::OrderModifier,
    force_move_key: bool,
    planning: bool,
) -> CursorFeedbackKind {
    use super::context_order::OrderModifier;
    use crate::map::entities::EntityCategory;
    // Techno's key rungs ([`super::context_order::object_key_action`]):
    // Shift answers ToggleSelect before every class row.
    let key_action = rules.and_then(|rules| {
        super::context_order::object_key_action(
            sim,
            rules,
            best_id?,
            Some(hover.stable_id),
            modifier,
            force_move_key,
        )
    });
    if key_action == Some(super::context_order::ObjectKeyAction::ToggleSelect) {
        return CursorFeedbackKind::Select;
    }

    if let Some(kind) = unit_repair_feedback(
        sim,
        selected,
        best_id,
        hover,
        rules,
        sim.session().current_house(),
    ) {
        return kind;
    }

    let hovered_entity = sim.entities().get(hover.stable_id);
    let hovered_obj = rules
        .and_then(|r| hovered_entity.and_then(|e| sim.interner().object_type(e.type_ref(), r)));

    // Infantry51E462's disarm prefix precedes self post-processing too.
    if let Some(actor) = best_id.and_then(|id| sim.entities().get(id))
        && actor.category() == EntityCategory::Infantry
        && sim.house_is_human_player(actor.owner())
        && rules
            .and_then(|rules| sim.interner().object_type(actor.type_ref(), rules))
            .is_some_and(|object| object.engineer)
        && hovered_entity.is_some_and(|target| target.bomb.is_some())
        && sim.bomb_seen_by(hover.stable_id, actor.owner())
    {
        return CursorFeedbackKind::DisarmBomb;
    }

    // Techno7000B9's self action precedes Ctrl/Alt. All self consumers share
    // admission with click/deploy-key, including loaded mobile transports.
    if selected == [hover.stable_id]
        && let (Some(rules), Some(entity)) = (rules, hovered_entity)
        && sim.session().current_house() == Some(entity.owner())
        && !entity.berserk.is_active()
        && !entity.is_warped_out()
    {
        if super::context_order::self_click_command(
            sim,
            rules,
            overlay_registry,
            entity.owner(),
            selected,
            Some(hover),
            planning,
        )
        .is_some()
        {
            return CursorFeedbackKind::Deploy;
        }
        if super::context_order::self_answers_no_deploy(
            sim,
            rules,
            overlay_registry,
            entity.owner(),
            selected,
            hover,
            planning,
        ) {
            return CursorFeedbackKind::NoDeploy;
        }

        // Aircraft417DCD..417DF4: the Self (4) of an Aircraft carrying no
        // passengers (+0x114) is 0, the arrow.
        let empty_aircraft = entity.category() == EntityCategory::Aircraft
            && entity
                .passenger_role
                .cargo()
                .is_none_or(|cargo| cargo.count() == 0);
        return if entity.category() == EntityCategory::Structure || empty_aircraft {
            CursorFeedbackKind::Default
        } else {
            CursorFeedbackKind::Invalid
        };
    }

    // The caller already resolved the single object whose action drives the
    // cursor for the whole selection, matching gamemd's DetermineAction. A
    // single selected object over itself takes the self action (4,
    // `TechnoClass::What_Action_OnObject`), never IvanBomb.
    let single_self = selected.len() == 1 && selected[0] == hover.stable_id;
    let best_is_ivan = !single_self
        && best_id
            .and_then(|id| sim.entities().get(id))
            .and_then(|e| rules.and_then(|r| sim.interner().object_type(e.type_ref(), r)))
            .is_some_and(|obj| obj.ivan);
    if let Some(best_id) = best_id
        && let (Some(sel_entity), Some(_)) = (
            sim.entities().get(best_id),
            sim.entities()
                .get(best_id)
                .and_then(|e| rules.and_then(|r| sim.interner().object_type(e.type_ref(), r))),
        )
    {
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
                | crate::sim::world::EngineerBuildingAction::EnterHospital => {
                    CursorFeedbackKind::Enter
                }
                crate::sim::world::EngineerBuildingAction::EnterGrinder => {
                    CursorFeedbackKind::Grind
                }
            };
        }

        // 5. Entry: a Unit's Select over a friendly building it may enter
        //    (refinery, Service Depot, Tank Bunker, Grinder), an Infantryman's
        //    over a Grinding= building, either's over a friendly passenger
        //    carrier, Alt's Move included, and an Aircraft's NoEnter over a
        //    UnitRepair= building (`context_order::entry_action`).
        if let Some(action) = rules
            .filter(|_| {
                matches!(
                    hover.kind,
                    HoverTargetKind::FriendlyStructure | HoverTargetKind::FriendlyUnit
                )
            })
            .and_then(|rules| {
                super::context_order::entry_action(
                    sim,
                    rules,
                    best_id,
                    hover.stable_id,
                    modifier,
                    force_move_key,
                    overlay_registry,
                )
            })
        {
            return match action {
                crate::sim::world::EntryClickAction::NoEnter => CursorFeedbackKind::NoEnter,
                crate::sim::world::EntryClickAction::Grind => CursorFeedbackKind::Grind,
                crate::sim::world::EntryClickAction::Enter => CursorFeedbackKind::Enter,
            };
        }

        let is_infantry = sel_entity.category() == EntityCategory::Infantry;

        // 6. Infantry garrisoning uses the shared CanBeOccupiedBy-equivalent predicate.
        //    garrisonable — only show Enter for those, not actual enemy-player buildings.
        if is_infantry
            && !matches!(
                modifier,
                OrderModifier::ForceFire | OrderModifier::GuardArea
            )
            && hovered_obj.is_some_and(|object| object.can_be_occupied)
            && let Some(rules) = rules
            && crate::sim::passenger::can_enter_transport(sim, rules, best_id, hover.stable_id)
        {
            return CursorFeedbackKind::Enter;
        }

        if key_action == Some(super::context_order::ObjectKeyAction::GuardArea) {
            return CursorFeedbackKind::GuardArea;
        }
        if key_action == Some(super::context_order::ObjectKeyAction::Move)
            && let Some(rules) = rules
        {
            return key_move_feedback(sim, rules, best_id, hover.stable_id);
        }
        if modifier == OrderModifier::ForceFire {
            if let Some(kind) = force_fire_feedback(
                sim,
                best_id,
                &combat::TargetKind::Entity(hover.stable_id),
                selected.len(),
                rules,
                overlay_registry,
            ) {
                let kind = kind.unwrap_or(CursorFeedbackKind::Default);
                return infantry_weapon_feedback(sim, rules, best_id, hover.stable_id, kind)
                    .unwrap_or_else(|| {
                        ivan_attack_feedback(kind, best_is_ivan, || {
                            !hovered_entity.is_some_and(|target| {
                                !super::context_order::ivan_bomb_refused(target, hovered_obj)
                            })
                        })
                    });
            }
        }

        // Native garrison precedes C4, which precedes the late infiltration
        // arm. C4 can terminate with ordinary Attack, not just Sabotage.
        let c4_action = rules.and_then(|rules| {
            super::context_order::infantry_c4_action(sim, rules, best_id, hover, false)
        });
        if c4_action == Some(super::context_order::InfantryC4Action::Sabotage) {
            return CursorFeedbackKind::Demolish;
        }
        if c4_action.is_none()
            && matches!(
                hover.kind,
                HoverTargetKind::FriendlyStructure | HoverTargetKind::EnemyStructure
            )
            && rules.is_some_and(|rules| {
                sim.infiltrator_building_action(best_id, hover.stable_id, rules)
            })
        {
            return CursorFeedbackKind::Enter;
        }

        // 7. Consume the admitted action, including AttackFriendlies and the
        //    qualified weapon grants. Infantry51EB24..7E's Ivan consumer does
        //    not require AttackCursorOnFriendlies a second time. A lone
        //    selected object over itself takes its self action first.
        if !single_self
            && matches!(
                hover.kind,
                HoverTargetKind::FriendlyUnit | HoverTargetKind::FriendlyStructure
            )
            && let Some(rules) = rules
            && super::context_order::attack_action(
                sim,
                rules,
                best_id,
                &combat::TargetKind::Entity(hover.stable_id),
                false,
                overlay_registry,
            ) == super::context_order::AttackAction::Attack
        {
            let in_range = attack_cursor_is_in_range(
                sim,
                best_id,
                &combat::TargetKind::Entity(hover.stable_id),
                selected.len(),
                Some(rules),
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
            return infantry_weapon_feedback(sim, Some(rules), best_id, hover.stable_id, kind)
                .unwrap_or_else(|| {
                    ivan_attack_feedback(kind, best_is_ivan, || {
                        !hovered_entity.is_some_and(|target| {
                            !super::context_order::ivan_bomb_refused(target, hovered_obj)
                        })
                    })
                });
        }
    }

    // 9. Generic fallback.
    //
    // A selected own unit under its own cursor answers NoMove, not Select:
    // retail refuses the move order onto a cell held by an own *selected*
    // techno, and the select action rejects an already-selected object — the
    // self-click resolves to ACTION_SELF / ACTION_SELECT instead
    // (`TechnoClass::What_Action_OnObject` `0x0070000B`, `[0xA8ECC8] == 1` at
    // `0x007000C1`, see `self_click_command`) — so the barred cursor wins.
    // Hovering an own unit that is NOT selected still takes Select, and a
    // no-selection hover keeps DetermineAction's no-selection Select. An
    // Aircraft cursor owner's 0 there becomes its Cell action instead
    // (`0x00417D7C`), and a Building's stays 0 (`0x0044750D`), which the
    // object action below answers.
    if hover_is_selected_own_unit(hover, &selected)
        && !best_id
            .and_then(|id| sim.entities().get(id))
            .is_some_and(|actor| {
                matches!(
                    actor.category(),
                    EntityCategory::Aircraft | EntityCategory::Structure
                )
            })
        // Unit `0x007403EB` and Infantry `0x0051F05E` make that 0 NoMove
        // only for an object they may control (vt+0xA0); else it stays 0.
        && best_id
            .zip(rules)
            .is_none_or(|(id, rules)| sim.techno_player_controllable(id, rules))
    {
        return CursorFeedbackKind::Invalid;
    }
    match hover.kind {
        HoverTargetKind::FriendlyUnit
        | HoverTargetKind::FriendlyStructure
        | HoverTargetKind::EnemyUnit
        | HoverTargetKind::EnemyStructure => {
            // The cursor owner's class tail is shared with click dispatch
            // ([`super::context_order::object_action_answers_friendly`]).
            // Another allied House is not an own-object Select: Techno70059B
            // answers 0, which Unit7403EB and Infantry51F05E turn into
            // NoMove and a Building keeps. Ivan's admitted Attack was handled
            // above; its refused attack still follows the shared
            // Select/NoMove tail instead of inventing one.
            let friendly = matches!(
                hover.kind,
                HoverTargetKind::FriendlyUnit | HoverTargetKind::FriendlyStructure
            );
            if friendly
                && !best_is_ivan
                && !best_id.is_some_and(|id| {
                    super::context_order::object_action_answers_friendly(
                        sim,
                        selected,
                        id,
                        hover.stable_id,
                    )
                })
            {
                return if hover.kind == HoverTargetKind::FriendlyUnit {
                    CursorFeedbackKind::FriendlyUnit
                } else {
                    CursorFeedbackKind::FriendlyStructure
                };
            }
            if let (Some(rules), Some(id)) = (rules, best_id) {
                use super::context_order::AttackAction;
                match super::context_order::attack_action(
                    sim,
                    rules,
                    id,
                    &combat::TargetKind::Entity(hover.stable_id),
                    false,
                    overlay_registry,
                ) {
                    AttackAction::None | AttackAction::AttackSupport => {
                        return CursorFeedbackKind::Default;
                    }
                    AttackAction::NoMove => return CursorFeedbackKind::Invalid,
                    AttackAction::Select => return CursorFeedbackKind::FriendlyUnit,
                    AttackAction::Rally | AttackAction::Move => return CursorFeedbackKind::Move,
                    AttackAction::Attack => {}
                }
            } else if friendly {
                return if hover.kind == HoverTargetKind::FriendlyUnit {
                    CursorFeedbackKind::FriendlyUnit
                } else {
                    CursorFeedbackKind::FriendlyStructure
                };
            }
            let in_range = best_id.is_some_and(|id| {
                attack_cursor_is_in_range(
                    sim,
                    id,
                    &combat::TargetKind::Entity(hover.stable_id),
                    selected.len(),
                    rules,
                    overlay_registry,
                )
            });
            let kind = if in_range {
                if matches!(
                    hover.kind,
                    HoverTargetKind::FriendlyUnit | HoverTargetKind::EnemyUnit
                ) {
                    CursorFeedbackKind::EnemyUnit
                } else {
                    CursorFeedbackKind::EnemyStructure
                }
            } else {
                CursorFeedbackKind::EnemyOutOfRange
            };
            best_id
                .and_then(|id| infantry_weapon_feedback(sim, rules, id, hover.stable_id, kind))
                .unwrap_or_else(|| {
                    ivan_attack_feedback(kind, best_is_ivan, || {
                        !hovered_entity.is_some_and(|target| {
                            !super::context_order::ivan_bomb_refused(target, hovered_obj)
                        })
                    })
                })
        }
        HoverTargetKind::HiddenEnemy => CursorFeedbackKind::Invalid,
    }
}

/// Alt's Move over the object `target` ([`super::context_order::ObjectKeyAction::Move`]),
/// which a building's factory rungs (`0x0044744E..0x0044750B`) can turn
/// into NoMove.
fn key_move_feedback(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target: u64,
) -> CursorFeedbackKind {
    let factory_refuses =
        sim.entities().get(actor_id).is_some_and(|actor| {
            actor.category() == crate::map::entities::EntityCategory::Structure
        }) && sim
            .building_object_move_action(actor_id, target, rules)
            .is_ok_and(|action| action == crate::sim::world::MoveAction::NoMove);
    if factory_refuses {
        CursorFeedbackKind::Invalid
    } else {
        CursorFeedbackKind::Move
    }
}

/// Techno70F0B0 promotes only Move1/Attack5, never Enter/C4/Select.
fn promote_attack_move_feedback(kind: CursorFeedbackKind, enabled: bool) -> CursorFeedbackKind {
    if enabled
        && matches!(
            kind,
            CursorFeedbackKind::Move
                | CursorFeedbackKind::EnemyUnit
                | CursorFeedbackKind::EnemyStructure
                | CursorFeedbackKind::EnemyOutOfRange
        )
    {
        CursorFeedbackKind::AttackMove
    } else {
        kind
    }
}

/// Infantry51EA59..51EB21, before Ivan. Select the actual weapon for this
/// target (including rank/IFV/naval choice); distance does not bar these rows.
fn infantry_weapon_feedback(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    actor_id: u64,
    target_id: u64,
    base: CursorFeedbackKind,
) -> Option<CursorFeedbackKind> {
    use super::context_order::InfantryAttackAction;
    if !matches!(
        base,
        CursorFeedbackKind::EnemyUnit
            | CursorFeedbackKind::EnemyStructure
            | CursorFeedbackKind::EnemyOutOfRange
    ) {
        return None;
    }
    Some(
        match super::context_order::infantry_attack_action(sim, rules?, actor_id, target_id)? {
            InfantryAttackAction::Attack => base,
            InfantryAttackAction::Sabotage => CursorFeedbackKind::Demolish,
            InfantryAttackAction::WeaponDemolish => CursorFeedbackKind::WeaponDemolish,
            InfantryAttackAction::AirStrike => CursorFeedbackKind::AirStrike,
            InfantryAttackAction::Select => CursorFeedbackKind::Select,
            InfantryAttackAction::Infiltrate => CursorFeedbackKind::Enter,
            InfantryAttackAction::NoMove => CursorFeedbackKind::Invalid,
        },
    )
}

/// The cursor for an empty selection: DetermineAction's no-selection arm
/// (`0x00692673`, entered when the selection count at `[0xA8ECC8]` is 0)
/// resolves the object under the cursor and, once it passes the gate chain —
/// the state byte at `+0x41A`, the type test at `+0x220`, the selectable
/// virtual (vtable `+0x138`) and the state bytes at `+0x6E7`/`+0x3D4` —
/// answers action 7: row 17, the Select cursor (frames 18-30). A hovered own
/// or enemy unit takes that answer in retail, shown side by side; with no
/// object under the cursor the arm falls through to the default arrow, which
/// is what `None` renders.
fn no_selection_cursor(
    hover: Option<&crate::app::input::entity_pick::HoverTargetKindWithId>,
) -> Option<CursorFeedbackKind> {
    match hover.map(|h| h.kind) {
        Some(
            HoverTargetKind::FriendlyUnit
            | HoverTargetKind::EnemyUnit
            | HoverTargetKind::FriendlyStructure
            | HoverTargetKind::EnemyStructure,
        ) => Some(CursorFeedbackKind::Select),
        _ => None,
    }
}

/// Whether the hovered own unit is itself selected — the self-hover that
/// answers NoMove instead of Select. Retail refuses the move order onto a
/// cell held by an own *selected* techno, and its select action rejects an
/// already-selected object: the self-click resolves to ACTION_SELF /
/// ACTION_SELECT instead (`TechnoClass::What_Action_OnObject` `0x0070000B`,
/// `[0xA8ECC8] == 1` at `0x007000C1`, see `self_click_command`), so the
/// barred cursor wins. An own unit that is not selected, a structure, or an
/// enemy is not this arm.
fn hover_is_selected_own_unit(
    hover: &crate::app::input::entity_pick::HoverTargetKindWithId,
    selected: &[u64],
) -> bool {
    hover.kind == HoverTargetKind::FriendlyUnit && selected.contains(&hover.stable_id)
}

/// The display mapping for Unit's shared repair-object action. Original
/// Display4AAE90 rows33/17/19 are saved in bunker_attack_action repair_rows.
fn unit_repair_feedback(
    sim: &crate::sim::world::Simulation,
    selected: &[u64],
    best_id: Option<u64>,
    hover: &crate::app::input::entity_pick::HoverTargetKindWithId,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    issuing_house: Option<crate::sim::intern::InternedId>,
) -> Option<CursorFeedbackKind> {
    if selected == [hover.stable_id] || hover.kind == HoverTargetKind::HiddenEnemy {
        return None;
    }
    let id = best_id?;
    let rules = rules?;
    let house = issuing_house?;
    Some(
        match sim.unit_repair_action(id, hover.stable_id, house, rules)? {
            crate::sim::world::UnitRepairAction::Repair => CursorFeedbackKind::RepairAction(true),
            crate::sim::world::UnitRepairAction::Select => CursorFeedbackKind::FriendlyUnit,
            crate::sim::world::UnitRepairAction::Move => CursorFeedbackKind::Invalid,
        },
    )
}

/// `InfantryClass::What_Action_OnObject` (`0x0051EB24..0x0051EB7E`) for a
/// Crazy Ivan (`Ivan=`) whose action is Attack: IvanBomb unless `refused`
/// ([`super::context_order::ivan_bomb_refused`]). A shot
/// ILLEGAL to him, such as one at a target that already carries a bomb
/// (`0x006FCBAD`), never reaches it unforced: [`super::context_order::attack_action`]
/// answers the Techno tail there.
fn ivan_attack_feedback(
    kind: CursorFeedbackKind,
    selector_is_ivan: bool,
    refused: impl FnOnce() -> bool,
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
    // The Crazy Ivan's arm itself (`0x0051EB24`,
    // [`super::context_order::ivan_bomb_refused`]): IvanBomb, or NoIvanBomb
    // (NoMove's row).
    if refused() {
        CursorFeedbackKind::NoIvanBomb
    } else {
        CursorFeedbackKind::IvanBomb
    }
}

/// Ordinary Display attack arm (`gamemd` 0x004AB4EE..0x004AB54B): row20
/// requires one selected Techno and its CanFireAtTarget verdict; otherwise row21.
/// Display4AAE90 resolves valid empty ground to CellClass* at4AAFF1, so both
/// Cell and Object targets participate. Native evidence:
/// tools/spatial_oracle/bunker_attack_action.json (full Display entry plus
/// selected-count controls; range verdict supplied, not range arithmetic).
fn attack_cursor_is_in_range(
    sim: &crate::sim::world::Simulation,
    actor_id: u64,
    target: &combat::TargetKind,
    selected_count: usize,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> bool {
    selected_count == 1
        && super::context_order::resolved_unit_in_range(
            sim,
            actor_id,
            target,
            rules,
            sim.resolved_terrain(),
            overlay_registry,
            Some(target),
        )
}

/// Ctrl action feedback. Outer None retains an unarmed receiver's fallback,
/// and a rejected Cell attack rung's movement tail (`0x00700AB7`);
/// Some(None) is native Action0 and preserves the default arrow.
fn force_fire_feedback(
    sim: &crate::sim::world::Simulation,
    actor_id: u64,
    target: &combat::TargetKind,
    selected_count: usize,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> Option<Option<CursorFeedbackKind>> {
    use super::context_order::{AttackAction, armed_action};
    let action = armed_action(sim, rules?, actor_id, target, true, overlay_registry)?;
    if action == AttackAction::None && matches!(target, combat::TargetKind::Cell(..)) {
        // The rejected attack rung still reaches the Cell movement owner.
        // Techno700AB7..700C08 returns Move for mouse_over=true; the
        // caller resolves that tail, separately from the clicked query.
        return None;
    }
    Some(action_feedback(
        sim,
        actor_id,
        target,
        action,
        selected_count,
        rules,
        overlay_registry,
    ))
}

/// The cursor row of an attack-rung answer; None is Action0, the arrow.
fn action_feedback(
    sim: &crate::sim::world::Simulation,
    actor_id: u64,
    target: &combat::TargetKind,
    action: super::context_order::AttackAction,
    selected_count: usize,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> Option<CursorFeedbackKind> {
    use super::context_order::AttackAction;
    match action {
        AttackAction::None => None,
        AttackAction::NoMove => Some(CursorFeedbackKind::Invalid),
        AttackAction::Rally | AttackAction::Move => Some(CursorFeedbackKind::Move),
        AttackAction::Select => Some(CursorFeedbackKind::FriendlyUnit),
        AttackAction::AttackSupport => Some(CursorFeedbackKind::Default),
        AttackAction::Attack => Some(
            if attack_cursor_is_in_range(
                sim,
                actor_id,
                target,
                selected_count,
                rules,
                overlay_registry,
            ) {
                CursorFeedbackKind::EnemyUnit
            } else {
                CursorFeedbackKind::EnemyOutOfRange
            },
        ),
    }
}

/// What the object resolver measures its candidates' distance to.
///
/// gamemd's resolver takes a cell *or* an object: with a cell it measures to the
/// cell centre at ground level, with an object to that object's GetCoords.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActionDistanceTarget {
    /// Cell centre, in cell coordinates.
    CellCentre(u16, u16),
    /// A specific object's GetCoords: a Techno's or a Terrain object's.
    Object(u64),
}

/// `SelectBestObjectForAction @ 0x005353D0`: the one selected object whose
/// action drives the cursor and the click for the whole selection
/// (DisplayClass::DetermineAction `0x00692634`, `0x00692655`). Each object
/// scores (`0x00535435..0x0053549C`):
///   0 — berserk (`+0x298`), so every calm object outranks it;
///   1 — disabled (`vt+0x37C`: EMP, or a Unit's death frames);
///   2 — unarmed (`Is_Armed`, `vt+0x2AC`);
///   3 — an armed building;
///   4 — any other armed object;
///   5 — one whose `GetWeaponDamageValue(-1)` is positive.
///
/// A higher score always wins; an equal one only when strictly nearer. The
/// distance is `Sqrt_Approx` of `(dx² + dz²) + dy²` through ftol
/// (`0x00535532..0x00535557`), from the object's GetCoords (`vt+0x48`) to the
/// cell centre at height 0 when a cell was supplied, else to the target
/// object's GetCoords.
///
/// RESIDUAL: score 1 is never given. VERA has no producer for EMP (`+0x504`)
/// or a Unit's death-frame counter (`+0x6D8`); see `techno_ai_cloak`.
pub(crate) fn select_best_for_action(
    sim: &crate::sim::world::Simulation,
    selected: &[u64],
    target: ActionDistanceTarget,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> Option<u64> {
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::combat_weapon::{is_armed, weapon_damage_value};
    use crate::sim::movement::ground_pose::object_get_coords;

    let terrain = sim.resolved_terrain();
    let target_point = match target {
        ActionDistanceTarget::CellCentre(cx, cy) => {
            let centre = |cell: u16| i32::from(cell) * 256 + 128;
            [centre(cx), centre(cy), 0]
        }
        ActionDistanceTarget::Object(id) => {
            let cells = terrain.map(crate::map::resolved_terrain::NativeCellQuery::canonical);
            let coords = crate::sim::movement::ground_pose::target_get_coords(
                combat::TargetKind::Entity(id),
                sim,
                cells.as_ref(),
            )?;
            [coords.x, coords.y, coords.z]
        }
    };

    let mut best_id: Option<u64> = None;
    let mut best_priority: i32 = -1;
    let mut best_dist = i32::MAX;

    for &sid in selected {
        let Some(entity) = sim.entities().get(sid) else {
            continue;
        };
        let obj = rules.and_then(|r| sim.interner().object_type(entity.type_ref(), r));
        let priority = if entity.berserk.is_active() {
            0
        } else if !obj.is_some_and(|obj| is_armed(entity, obj)) {
            2
        } else if entity.category() == EntityCategory::Structure {
            3
        } else if obj.zip(rules).is_some_and(|(obj, rules)| {
            weapon_damage_value(entity, obj, sim.entities(), rules, sim.interner()) > 0
        }) {
            5
        } else {
            4
        };

        let coords = object_get_coords(entity, terrain);
        let dist = crate::util::native_x87::sqrt_approx_length([
            coords.x.wrapping_sub(target_point[0]),
            coords.z.wrapping_sub(target_point[2]),
            coords.y.wrapping_sub(target_point[1]),
        ]);

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
        CursorFeedbackKind::Default => Some(CursorId::Default),
        // Row 17 — the empty-selection select answer of DetermineAction's
        // no-selection arm (action 7, 0x00692670).
        CursorFeedbackKind::Select => Some(CursorId::Select),
        CursorFeedbackKind::FriendlyUnit | CursorFeedbackKind::FriendlyStructure => {
            Some(CursorId::Select)
        }
        CursorFeedbackKind::Move => Some(CursorId::Move),
        CursorFeedbackKind::AttackMove => Some(CursorId::AttackMove),
        CursorFeedbackKind::GuardArea => Some(CursorId::GuardArea),
        CursorFeedbackKind::EnemyUnit | CursorFeedbackKind::EnemyStructure => {
            Some(CursorId::Attack)
        }
        // Harvest and out-of-range attack share cursor row 21 — the action
        // switch falls through from the attack case straight into the harvest
        // case, so both land on the same row.
        CursorFeedbackKind::EnemyOutOfRange | CursorFeedbackKind::Harvest => {
            Some(CursorId::AttackOutOfRange)
        }
        CursorFeedbackKind::Invalid | CursorFeedbackKind::NoIvanBomb => Some(CursorId::NoMove),
        CursorFeedbackKind::PlaceValid | CursorFeedbackKind::PlaceInvalid => None,
        CursorFeedbackKind::Scroll(dir) => Some(scroll_dir_to_cursor_id(dir)),
        CursorFeedbackKind::ScrollBlocked(dir) => Some(blocked_scroll_dir_to_cursor_id(dir)),
        // Cursor-table row 61, mouse.sha frame 385.
        CursorFeedbackKind::Pan => Some(CursorId::Pan),
        // Rows 62..69, frames 386..393 — a set of their own, NOT the barred
        // edge-scroll rows, which are rows 9..16 / frames 10..17 and carry
        // edge-anchored hotspots instead of centre/centre.
        CursorFeedbackKind::PanBlocked(dir) => Some(pan_dir_to_cursor_id(dir)),
        // The radar's own mini set travels in the payload: its action's row
        // ([`crate::app::input::radar::cursor`]), or the plain arrow mini.
        CursorFeedbackKind::Minimap(id) => Some(id),
        CursorFeedbackKind::Enter | CursorFeedbackKind::Grind => Some(CursorId::Enter),
        CursorFeedbackKind::NoEnter => Some(CursorId::NoEnter),
        CursorFeedbackKind::EngineerDamage => Some(CursorId::EngineerRepair),
        CursorFeedbackKind::RepairAction(valid) => Some(if valid {
            CursorId::Repair
        } else {
            CursorId::NoRepair
        }),
        CursorFeedbackKind::Demolish | CursorFeedbackKind::WeaponDemolish => {
            Some(CursorId::Demolish)
        }
        CursorFeedbackKind::AirStrike => Some(CursorId::AirStrike),
        // `DisplayClass::SetCursorFromAction @ 0x004AAE90`: action 0x35 ->
        // row 0x26, action 0x39 -> row 0x3B. NoIvanBomb (0x36) shares
        // NoMove's row 0x13 but retains its distinct input action.
        CursorFeedbackKind::IvanBomb => Some(CursorId::IvanBomb),
        CursorFeedbackKind::DisarmBomb => Some(CursorId::Disarm),
        CursorFeedbackKind::Deploy => Some(CursorId::Deploy),
        CursorFeedbackKind::NoDeploy => Some(CursorId::NoDeploy),
        // Sidebar repair mode: gamemd's repair-mode arm (DetermineAction
        // 0x006927CD..0x0069280D, the flag at MouseClass+0x11B0) answers
        // action 10 for an own repairable object — row 34, frames 170-189 —
        // and action 15 (NoRepair) otherwise. Row 33 (frames 150-169) is the
        // engineer's hover-repair cursor, RepairAction's row — not this
        // mode's.
        CursorFeedbackKind::RepairMode(valid) => Some(if valid {
            CursorId::EngineerRepair
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

/// DisplayCancel4AEAD0 resets cursor shape through the existing animation
/// owner. The next ordinary draw starts the default shape at frame0.
pub(crate) fn reset_cursor_shape() {
    CURSOR_ANIMATION.with(|animation| animation.set(CursorAnimation::new()));
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

pub(crate) fn current_sidebar_view_hit(state: &AppState) -> bool {
    sidebar_view_hit(
        state.sidebar_frame(),
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    )
}

fn sidebar_view_hit(frame: crate::ui::sidebar::SidebarFrame, cursor_x: f32, cursor_y: f32) -> bool {
    // The bottom command-bar surface owns its spacer and unused slots too,
    // even when the bar is closed (#2627). Reuse its frame-space bounds so
    // world cursor queries stop at the same boundary as drawing and clicks.
    let (x, y, w, h) = frame.strip();
    let strip_rect = crate::ui::sidebar::Rect {
        x: x as f32,
        y: y as f32,
        w: w as f32,
        h: h as f32,
    };
    let (x, _, w, h) = frame.column();
    let panel_rect = crate::ui::sidebar::Rect {
        x: x as f32 - 10.0,
        y: 10.0,
        w: w as f32,
        h: h as f32 - 20.0,
    };
    strip_rect.contains(cursor_x, cursor_y) || panel_rect.contains(cursor_x, cursor_y)
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
        "Attack" => Some(CursorId::Attack),
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

/// `cursor_confined` clips the OS pointer to the window rectangle ('s
/// edge-scroll lock), so it cannot leave the game while the player scrolls the
/// map. The OS drops the clip on focus loss (alt-tab), so callers re-apply this
/// on every focus regain. A platform that cannot confine (some Wayland setups)
/// logs and leaves the cursor free rather than failing the frame.
pub(crate) fn apply_cursor_confinement(state: &AppState) {
    apply_cursor_confinement_impl(state, true);
}

/// Keep the player's cursor-lock choice alive every frame clips
/// continuously). `ClipCursor` is a *system-wide* setting the OS drops on
/// focus loss or the Windows key, so a one-shot apply silently stops working
/// mid-session. While the lock is on and the window is focused we re-assert the
/// clip each frame; while it is in the background we leave the pointer alone so
/// we never trap it inside another application. A cleared lock always releases.
/// Silent (no per-frame log spam), and a no-op once the OS clip already matches
/// what the player chose, so re-asserting per frame issues no `ClipCursor` call.
pub(crate) fn reassert_cursor_confinement(state: &AppState) {
    if !state.platform.window_active && state.platform.cursor_confined {
        // Backgrounded: don't fight other apps; Focused(false) already released.
        return;
    }
    apply_cursor_confinement_impl(state, false);
}

/// The OS pointer clip this thread currently holds, as `(left, top, right,
/// bottom)` in screen pixels, or `None` when the pointer is free.
/// `ClipCursor(NULL)` does not clear the clip to nothing: it resets it to the
/// window station's desktop, which is the virtual-screen union, so that value
/// reads back as "free".
#[cfg(target_os = "windows")]
fn current_cursor_clip() -> Option<(i32, i32, i32, i32)> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClipCursor, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe {
        if GetClipCursor(&mut rect) == 0 {
            return None;
        }
        let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let right = left + GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let bottom = top + GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if (rect.left, rect.top, rect.right, rect.bottom) == (left, top, right, bottom) {
            None
        } else {
            Some((rect.left, rect.top, rect.right, rect.bottom))
        }
    }
}

fn apply_cursor_confinement_impl(state: &AppState, verbose: bool) {
    // Windows: clip to the client rectangle with the Win32 ClipCursor API.
    // winit's own grab path is deliberately not used here: `refresh_os_cursor`
    // clips a hidden cursor to a one-pixel box at the client centre
    // (winit window_state.rs, to keep the pointer off the taskbar), and VERA
    // hides the OS arrow whenever it draws mouse.shp, so `CursorGrabMode::Confined`
    // pins the pointer in place instead. Clipping the client rect directly is the
    // reliable form of the same thing.
    #[cfg(target_os = "windows")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use std::ffi::c_void;
        use windows_sys::Win32::Foundation::{POINT, RECT};
        use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
        use windows_sys::Win32::UI::WindowsAndMessaging::{ClipCursor, GetClientRect};
        let handle = match state.platform.window.window_handle() {
            Ok(h) => h,
            Err(err) => {
                if verbose {
                    log::warn!("cursor confinement: no window handle ({err})");
                }
                state.platform.pointer_held.set(false);
                return;
            }
        };
        let RawWindowHandle::Win32(w) = handle.as_raw() else {
            state.platform.pointer_held.set(false);
            return;
        };
        let hwnd = w.hwnd.get() as *mut c_void;
        let mut rect: RECT = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        unsafe {
            // Clip the *client* area, not the whole window: GetWindowRect would
            // include the title bar and the resize border, so the pointer could
            // still wander onto them. GetClientRect gives the drawable region in
            // window-local coords; ClientToScreen maps its corners into screen
            // space for ClipCursor. Both are in the same DPI-aware physical
            // pixels, so the rect matches what ClipCursor expects.
            GetClientRect(hwnd, &mut rect);
            let mut tl = POINT {
                x: rect.left,
                y: rect.top,
            };
            let mut br = POINT {
                x: rect.right,
                y: rect.bottom,
            };
            ClientToScreen(hwnd, &mut tl);
            ClientToScreen(hwnd, &mut br);
            // Clip to the *exact* client rectangle — do NOT inset. The game's
            // edge-scroll trigger is a one-pixel at-edge band (camera.rs
            // `edge_scroll_intent`: `x<=0 || x>=view_w-1 || y<=0 || y>=view_h-1`),
            // so the pointer must be able to reach the outer pixel
            // (x=0 / x=view_w-1). Insetting even 1px pins the pointer at
            // [1, view_w-2] and silently kills edge scrolling on all four sides.
            // The system arrow the clip edge used to expose is not suppressed
            // here. The OS arrow image has one owner,
            // `show_os_cursor_only_without_game_cursor`, and nothing re-asserts an
            // arrow inside the client area: Winit registers the window class with
            // a null cursor, so `DefWindowProc` has no arrow to restore for a
            // client hit. The validity check below still guards tiny windows from
            // an inverted rect.
            let clip = RECT {
                left: tl.x,
                top: tl.y,
                right: br.x,
                bottom: br.y,
            };
            // A not-yet-laid-out window reports a zero/empty client; clipping to
            // it would pin the pointer in place ("mouse won't move"), so defer
            // until the window has real geometry. The per-frame reassert picks it
            // up automatically once the window is sized.
            if clip.right <= clip.left || clip.bottom <= clip.top {
                if verbose {
                    log::info!(
                        "cursor confinement: client rect not ready ({},{},{},{}), deferring",
                        clip.left,
                        clip.top,
                        clip.right,
                        clip.bottom
                    );
                }
                state.platform.pointer_held.set(false);
                return;
            }
            // Only issue a call when the clip actually changes. Re-issuing
            // `ClipCursor` on a steady frame floods the queue with
            // `WM_MOUSEMOVE` (winit guards the same way in `refresh_os_cursor`),
            // and each spurious move re-runs the enter/leave bookkeeping that
            // decides who owns the pointer image.
            let wanted = if state.platform.cursor_confined {
                Some((clip.left, clip.top, clip.right, clip.bottom))
            } else {
                None
            };
            if current_cursor_clip() == wanted {
                state.platform.pointer_held.set(wanted.is_some());
                return;
            }
            if verbose {
                log::info!(
                    "cursor confinement: clipping to client ({},{},{},{}) confined={}",
                    clip.left,
                    clip.top,
                    clip.right,
                    clip.bottom,
                    state.platform.cursor_confined
                );
            }
            let ok = if state.platform.cursor_confined {
                ClipCursor(&clip)
            } else {
                ClipCursor(std::ptr::null())
            };
            if ok == 0 && verbose {
                log::warn!("cursor confinement: ClipCursor failed");
            }
            state
                .platform
                .pointer_held
                .set(state.platform.cursor_confined && ok != 0);
        }
    }
    // Other platforms: use winit's Confined grab. On X11/macOS this confines the
    // pointer to the window; on pure Wayland cursor confinement is unsupported
    // and set_cursor_grab returns an error, which we log and tolerate so the
    // frame is never blocked.
    #[cfg(not(target_os = "windows"))]
    {
        use winit::window::CursorGrabMode;
        let mode = if state.platform.cursor_confined {
            CursorGrabMode::Confined
        } else {
            CursorGrabMode::None
        };
        let grabbed = state.platform.window.set_cursor_grab(mode);
        if let Err(err) = &grabbed {
            if verbose {
                log::warn!("cursor confinement unavailable on this platform: {err:#}");
            }
        }
        state
            .platform
            .pointer_held
            .set(state.platform.cursor_confined && grabbed.is_ok());
    }
}

/// Drop the OS cursor clip entirely. Called on focus loss so alt-tab releases
/// the pointer instead of leaving it trapped over the game window (
/// behaviour). Presentation-only.
pub(crate) fn release_cursor_confinement(state: &AppState) {
    state.platform.pointer_held.set(false);
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::ClipCursor;
        unsafe {
            ClipCursor(std::ptr::null());
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Best-effort: releasing an already-free grab can error on some
        // compositors (pure Wayland), which we tolerate so the per-frame
        // re-assert path never spams the log.
        let _ = state
            .platform
            .window
            .set_cursor_grab(winit::window::CursorGrabMode::None);
    }
}

/// Flip the window cursor lock  and re-apply it immediately.
/// Bound to the runtime cursor-lock hotkey; never part of the sim or replay.
pub(crate) fn toggle_cursor_confinement(state: &mut AppState) {
    state.platform.cursor_confined = !state.platform.cursor_confined;
    apply_cursor_confinement(state);
    log::info!(
        "cursor confinement {}",
        if state.platform.cursor_confined {
            "enabled"
        } else {
            "disabled"
        }
    );
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "ares")]
    use super::super_weapon_cursor_id;
    use super::{
        ActionDistanceTarget, capability_cursor_for_hover, empty_cell_cursor, force_fire_feedback,
        select_best_for_action, what_action_on_cell,
    };
    use crate::app::input::entity_pick::HoverTargetKindWithId;
    use crate::app::types::{CursorFeedbackKind, CursorId, HoverTargetKind};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::world::Simulation;

    #[test]
    fn command_bar_blocks_world_cursor_across_the_whole_strip() {
        let frame = crate::ui::sidebar::SidebarFrame::native(800, 600);
        // Both open and closed bars retain the bottom surface, including the
        // spacer and unused command slots, not just the live button gadgets.
        assert!(super::sidebar_view_hit(frame, 1.0, 568.0));
        assert!(super::sidebar_view_hit(frame, 300.0, 580.0));
        assert!(super::sidebar_view_hit(frame, 580.0, 599.0));
        assert!(!super::sidebar_view_hit(frame, 300.0, 567.0));
        assert!(!super::sidebar_view_hit(frame, 300.0, 600.0));
    }

    #[test]
    fn command_bar_cursor_boundary_tracks_sidebar_scale() {
        let frame = crate::ui::sidebar::SidebarFrame::new(1280, 720, 150);
        assert!(super::sidebar_view_hit(frame, 300.0, 672.0));
        assert!(!super::sidebar_view_hit(frame, 300.0, 671.0));
        assert!(super::sidebar_view_hit(frame, 300.0, 719.0));
        assert!(!super::sidebar_view_hit(frame, 300.0, 720.0));
    }

    /// NoIvanBomb (0x36) draws NoMove's row 19 but stays its own action,
    /// which the radar's keep table drops ([`crate::app::input::radar`]).
    #[test]
    fn a_refused_ivan_bomb_draws_the_no_move_row() {
        let refused_ivan =
            super::ivan_attack_feedback(CursorFeedbackKind::EnemyUnit, true, || true);
        assert_eq!(refused_ivan, CursorFeedbackKind::NoIvanBomb);
        assert_eq!(
            super::cursor_id_for_feedback(refused_ivan),
            Some(CursorId::NoMove)
        );
    }

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

        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        let house = sim.interner().get("Americans");
        sim.session_mut_for_test().set_current_house_for_test(house);
        // 4. Mark the SEAL as selected (mirrors clicking it in-game).
        if let Some(e) = sim.entities_mut_for_test().get_mut(seal_id) {
            e.selection.set_selected_for_test(true);
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
            crate::app::input::context_order::OrderModifier::Normal,
            false,
            false,
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

        if let Some(e) = sim.entities_mut_for_test().get_mut(miner_id) {
            e.selection.set_selected_for_test(true);
        }
        // What_Action's Enter is a human player's (House `0x0050B6F0`).
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        let house = Some(sim.entities().get(miner_id).unwrap().owner());
        sim.session_mut_for_test().set_current_house_for_test(house);

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
            crate::app::input::context_order::OrderModifier::Normal,
            false,
            false,
        );
        assert_eq!(
            result,
            CursorFeedbackKind::Enter,
            "Chrono Miner hovering its own Refinery should show the Enter (dock) cursor",
        );
    }

    #[cfg(feature = "ares")]
    #[test]
    fn unit_delivery_superweapon_custom_action_uses_existing_paradrop_cursor() {
        let rules = crate::rules::ruleset::RuleSet::from_ini_with_fixed_art_for_test(
            &crate::rules::ini_parser::IniFile::from_str(
                "[SuperWeaponTypes]\n0=Delivery\n[Delivery]\nType=UnitDelivery\nAction=Custom\n",
            ),
            &crate::rules::ini_parser::IniFile::empty(),
        )
        .unwrap();
        let action = rules
            .super_weapon("Delivery")
            .unwrap()
            .action
            .as_deref()
            .unwrap();
        assert_eq!(super_weapon_cursor_id(action), Some(CursorId::Paradrop));
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

    // Techno700600's unforced Attack rung (0x007008A6..0x007009F1): an
    // enemy-owned Wall overlay under a Wall= warhead answers Attack under a
    // plain chord, through the forced shot's tail; an own wall, Ctrl+Alt and
    // a warhead without Wall= stay on the Move rungs.
    #[test]
    fn an_enemy_wall_takes_the_unforced_attack_cursor() {
        use crate::app::input::context_order::OrderModifier;
        use crate::map::resolved_terrain::ResolvedTerrainGrid;
        let ini = IniFile::from_str(
            "[InfantryTypes]
             [VehicleTypes]
             0=MTNK
             1=LTNK
             [AircraftTypes]
             [BuildingTypes]
             [MTNK]
             Strength=300
             Primary=105mm
             Speed=6
             SpeedType=Track
             MovementZone=Normal
             Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
             [LTNK]
             Strength=300
             Primary=Saw
             Speed=6
             SpeedType=Track
             MovementZone=Normal
             Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
             [WeaponTypes]
             0=105mm
             1=Saw
             [105mm]
             Damage=60
             Range=5
             Warhead=AP
             [Saw]
             Damage=60
             Range=5
             Warhead=Wd
             [Warheads]
             0=AP
             1=Wd
             [AP]
             Wall=yes
             [Wd]
             Wood=yes
             [OverlayTypes]
             0=GAWALL
             1=FENCE
             2=BARRIER
             [GAWALL]
             Wall=yes
             [FENCE]
             Wall=yes
             Armor=wood
             [BARRIER]
             Wall=yes
             LegalTarget=no
",
        );
        let rules = RuleSet::from_ini(&ini).expect("wall rules");
        let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
        let wall = registry.id_for_name("GAWALL").expect("GAWALL registered");
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        let tank = sim
            .spawn_object("MTNK", "Americans", 2, 2, 0, &rules)
            .expect("tank spawned");
        let saw = sim
            .spawn_object("LTNK", "Americans", 3, 3, 0, &rules)
            .expect("saw tank spawned");
        let owner = sim.entities().get(tank).unwrap().owner();
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(
            32,
            32,
            (0..32)
                .flat_map(|y| {
                    (0..32).map(move |x| crate::map::resolved_terrain::test_flat_cell(x, y))
                })
                .collect(),
        ));
        let enemy = sim.intern("Russians");
        let mut grid = crate::sim::overlay_grid::OverlayGrid::new(32, 32);
        grid.place_owned_wall(4, 2, wall, 0, enemy);
        grid.place_owned_wall(2, 4, wall, 0, owner);
        *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
        sim.fog_mut_for_test()
            .reveal_cells_for_owner(owner, (0..32).flat_map(|y| (0..32).map(move |x| (x, y))));
        *sim.overlay_grid_mut_for_test() = Some(grid);
        let feedback = |cell, modifier| {
            super::unforced_cell_attack_feedback(
                &sim,
                Some(owner),
                Some(tank),
                cell,
                modifier,
                1,
                Some(&rules),
                Some(&registry),
            )
        };
        assert_eq!(
            feedback((4, 2), OrderModifier::Normal),
            Some(CursorFeedbackKind::EnemyUnit)
        );
        assert_eq!(feedback((2, 4), OrderModifier::Normal), None);
        assert_eq!(feedback((4, 2), OrderModifier::GuardArea), None);
        assert_eq!(feedback((5, 5), OrderModifier::Normal), None);
        // Wood= reaches only an Armor=wood wall; LegalTarget=no bars the rung.
        let fence = registry.id_for_name("FENCE").expect("FENCE registered");
        let barrier = registry.id_for_name("BARRIER").expect("BARRIER registered");
        let grid = sim.overlay_grid_mut_for_test().as_mut().unwrap();
        grid.place_owned_wall(8, 2, fence, 0, enemy);
        grid.place_owned_wall(10, 2, barrier, 0, enemy);
        let admits = |id, cell| sim.cell_attack_rung_admits(id, cell, &rules, Some(&registry));
        assert!(admits(saw, (8, 2)));
        assert!(!admits(saw, (4, 2)), "GAWALL is not Armor=wood");
        assert!(!admits(tank, (10, 2)), "LegalTarget=no");
    }

    fn stationary_attack_fixture() -> (Simulation, RuleSet, u64) {
        stationary_attack_fixture_with_overrides("")
    }

    fn stationary_attack_fixture_with_overrides(extra: &str) -> (Simulation, RuleSet, u64) {
        // Keep overrides in the same physical body. Appending a second
        // [TANK] is not a rules layer and does not replace its first body.
        let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
            &(r#"
[InfantryTypes]
0=GI
[VehicleTypes]
0=TANK
[BuildingTypes]
0=TOWER
1=HOUSE
[TANK]
Strength=300
Primary=GUN
Secondary=LONG
Speed=6
SpeedType=Track
MovementZone=Normal
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
[TOWER]
Strength=500
Primary=GUN
Foundation=1x1
[HOUSE]
Strength=500
CanBeOccupied=yes
CanOccupyFire=yes
MaxNumberOccupants=10
Foundation=2x2
[GI]
Strength=100
Speed=4
SpeedType=Foot
MovementZone=Normal
Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}
Primary=GUN
OccupyWeapon=GUN
Occupier=yes
[GUN]
Damage=60
Range=5
Projectile=SHOT
Warhead=AP
[LONG]
Damage=60
Range=30
Projectile=SHOT
Warhead=AP
[SHOT]
AG=yes
AA=no
Inviso=yes
SubjectToElevation=no
[Warheads]
0=AP
[AP]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
"#
            .replacen("[TANK]\n", &format!("[TANK]\n{extra}"), 1)),
        ))
        .unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        let tank = sim
            .spawn_object("TANK", "Americans", 2, 2, 0, &rules)
            .unwrap();
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        let house = Some(sim.entities().get(tank).unwrap().owner());
        sim.session_mut_for_test().set_current_house_for_test(house);
        // Later objects use ordinary Unlimbo: supply clear rules, playfield,
        // terrain and navigation through the shared component-arena owner.
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        (sim, rules, tank)
    }

    #[test]
    fn bunker_attack_actions_follow_native_entry_and_display_corpus() {
        use crate::app::input::context_order::{AttackAction, armed_action, attack_action};
        use crate::sim::combat::TargetKind;
        use crate::sim::docking::bunker_link::BunkerLink;
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/bunker_attack_action.json",
        ))
        .unwrap();
        for row in corpus["rows"]
            .as_array()
            .unwrap()
            .iter()
            .chain(corpus["display_controls"].as_array().unwrap())
        {
            let case = &row["input"];
            let (mut sim, rules, actor) = stationary_attack_fixture();
            if case["installed"].as_bool().unwrap() {
                sim.entities_mut_for_test()
                    .get_mut(actor)
                    .unwrap()
                    .bunker
                    .set_bunker_link_for_test(BunkerLink::Installed(99));
            }
            // Active rearm never belongs to WhatAction's control/range gate.
            sim.entities_mut_for_test()
                .get_mut(actor)
                .unwrap()
                .rearm_timer
                .set_for_test(crate::sim::timer::CdTimer::started(0, 999));
            let x = if case["in_range"].as_bool().unwrap() {
                4
            } else {
                20
            };
            let target = if case["route"] == "cell" {
                TargetKind::Cell(x, 2)
            } else {
                let owner = if case["relation"] == "own" {
                    "Americans"
                } else {
                    "Soviets"
                };
                TargetKind::Entity(sim.spawn_object("TANK", owner, x, 2, 0, &rules).unwrap())
            };
            let expected = match row["action"].as_u64().unwrap() {
                0 => AttackAction::None,
                5 => AttackAction::Attack,
                7 => AttackAction::Select,
                other => panic!("unsupported original action {other}"),
            };
            let action = if case["ctrl"] == false {
                attack_action(&sim, &rules, actor, &target, false, None)
            } else {
                armed_action(&sim, &rules, actor, &target, true, None).unwrap()
            };
            assert_eq!(action, expected, "native input {case}");
            if case["ctrl"] != false {
                let selected_count =
                    usize::try_from(row["cursor"]["output"]["selected_count"].as_u64().unwrap())
                        .unwrap();
                let feedback =
                    force_fire_feedback(&sim, actor, &target, selected_count, Some(&rules), None)
                        .unwrap_or_else(|| {
                            Some(empty_cell_cursor(
                                &sim,
                                Some(&rules),
                                None,
                                sim.session().current_house(),
                                Some(actor),
                                (x, 2),
                                super::super::context_order::OrderModifier::ForceFire,
                                crate::app::types::OrderMode::Move,
                                true,
                            ))
                        });
                let cursor = feedback
                    .and_then(super::cursor_id_for_feedback)
                    .unwrap_or(CursorId::Default);
                let expected_cursor = match row["cursor"]["output"]["cursor_row"].as_u64().unwrap()
                {
                    0 => CursorId::Default,
                    17 => CursorId::Select,
                    20 => CursorId::Attack,
                    21 => CursorId::AttackOutOfRange,
                    other => panic!("unsupported native cursor {other}"),
                };
                assert_eq!(cursor, expected_cursor, "native input {case}");
            }
        }
    }

    #[test]
    fn bunker_fixed_and_occupied_buildings_share_selected_weapon_range_admission() {
        use crate::app::input::context_order::{AttackAction, armed_action};
        use crate::sim::combat::TargetKind;
        let (mut sim, rules, tank) = stationary_attack_fixture();
        let tower = sim
            .spawn_object("TOWER", "Americans", 2, 6, 0, &rules)
            .unwrap();
        let house = sim
            .spawn_object("HOUSE", "Americans", 2, 10, 0, &rules)
            .unwrap();
        let gi = sim
            .spawn_object("GI", "Americans", 1, 10, 0, &rules)
            .unwrap();
        sim.entities_mut_for_test()
            .get_mut(house)
            .unwrap()
            .passenger_role
            .cargo_mut()
            .unwrap()
            .board_forced_chain(&[(gi, 1.0)]);
        assert!(rules.object("HOUSE").unwrap().primary().is_none());
        for (actor, y) in [(tower, 6), (house, 10)] {
            assert_eq!(
                armed_action(&sim, &rules, actor, &TargetKind::Cell(4, y), true, None),
                Some(AttackAction::Attack)
            );
            assert_eq!(
                armed_action(&sim, &rules, actor, &TargetKind::Cell(20, y), true, None),
                Some(AttackAction::None)
            );
            assert_eq!(
                force_fire_feedback(&sim, actor, &TargetKind::Cell(4, y), 1, Some(&rules), None),
                Some(Some(CursorFeedbackKind::EnemyUnit)),
                "stationary actor's in-range ground uses the red attack cursor"
            );
            let feedback =
                force_fire_feedback(&sim, actor, &TargetKind::Cell(20, y), 1, Some(&rules), None)
                    .unwrap_or_else(|| {
                        Some(empty_cell_cursor(
                            &sim,
                            Some(&rules),
                            None,
                            sim.session().current_house(),
                            Some(actor),
                            (20, y),
                            super::super::context_order::OrderModifier::ForceFire,
                            crate::app::types::OrderMode::Move,
                            true,
                        ))
                    });
            assert_eq!(
                feedback,
                Some(CursorFeedbackKind::Default),
                "stationary actor's out-of-range action retains the default arrow"
            );
        }
        assert_eq!(
            armed_action(&sim, &rules, tank, &TargetKind::Cell(20, 2), true, None),
            Some(AttackAction::Attack),
            "ordinary mobile may close distance"
        );
    }

    #[test]
    fn bunker_attack_unit_postprocessing_matches_native_caller_controls() {
        use crate::app::input::context_order::{AttackAction, armed_action};
        use crate::sim::combat::TargetKind;
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/bunker_attack_action.json",
        ))
        .unwrap();
        let rows = corpus["postprocess_rows"].as_array().unwrap();
        assert_eq!(rows.len(), 24);
        for row in rows {
            let case = &row["input"];
            // Native wrapper fixture supplies slot0. Weapon selection itself
            // has its own corpus; this makes that seam explicit in Rust.
            let extra = format!(
                "Harvester={}\nIsSimpleDeployer={}\nDeployFire={}\nDeployFireWeapon=0\n",
                case["harvester"].as_bool().unwrap_or(false),
                case["simple_deployer"].as_bool().unwrap_or(false),
                case["deploy_fire"].as_bool().unwrap_or(false)
            );
            let (mut sim, rules, actor) = stationary_attack_fixture_with_overrides(&extra);
            let kind = rules.object("TANK").unwrap();
            assert_eq!(kind.harvester, case["harvester"].as_bool().unwrap_or(false));
            assert_eq!(
                kind.is_simple_deployer,
                case["simple_deployer"].as_bool().unwrap_or(false)
            );
            assert_eq!(
                kind.deploy_fire,
                case["deploy_fire"].as_bool().unwrap_or(false)
            );
            sim.entities_mut_for_test()
                .get_mut(actor)
                .unwrap()
                .set_unit_simple_deploy_for_test(
                    case["deployed"].as_bool().unwrap_or(false),
                    false,
                    false,
                );
            assert_eq!(
                sim.entities().get(actor).unwrap().is_fully_deployed(),
                case["deployed"].as_bool().unwrap_or(false)
            );
            let x = if case["in_range"].as_bool().unwrap() {
                4
            } else {
                20
            };
            if let Some(land_type) = case["land_type"].as_u64() {
                // Native Cell+EC is retained as 0 (clear) or 5 (ore).
                sim.resolved_terrain_mut_for_test()
                    .as_mut()
                    .unwrap()
                    .cell_mut_for_test(x, 2)
                    .unwrap()
                    .land_type = u8::try_from(land_type).unwrap();
            }
            let target = if case["route"] == "cell" {
                TargetKind::Cell(x, 2)
            } else {
                TargetKind::Entity(
                    sim.spawn_object("TANK", "Soviets", x, 2, 0, &rules)
                        .unwrap(),
                )
            };
            let expected = match row["action"].as_u64().unwrap() {
                2 => AttackAction::NoMove,
                5 => AttackAction::Attack,
                other => panic!("native action {other}"),
            };
            assert_eq!(
                armed_action(&sim, &rules, actor, &target, true, None),
                Some(expected),
                "{case}"
            );
            let feedback =
                force_fire_feedback(&sim, actor, &target, 1, Some(&rules), None).unwrap();
            let cursor = feedback
                .and_then(super::cursor_id_for_feedback)
                .unwrap_or(CursorId::Default);
            let expected_cursor = match row["cursor"]["output"]["cursor_row"].as_u64().unwrap() {
                19 => super::cursor_id_for_feedback(CursorFeedbackKind::Invalid).unwrap(),
                20 => CursorId::Attack,
                21 => CursorId::AttackOutOfRange,
                other => panic!("native cursor {other}"),
            };
            assert_eq!(cursor, expected_cursor, "{case}");
        }
    }

    #[test]
    fn bunker_selected_range_does_not_require_a_resolved_warhead() {
        use crate::map::resolved_terrain::ResolvedTerrainGrid;
        use crate::sim::combat::{TargetKind, line_of_fire::LineOfFireInputs};
        let (mut sim, rules, actor) = sim_with_track_tank();
        assert!(rules.weapon("105mm").unwrap().warhead.is_none());
        sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(
            32,
            32,
            (0..32)
                .flat_map(|y| {
                    (0..32).map(move |x| crate::map::resolved_terrain::test_flat_cell(x, y))
                })
                .collect(),
        ));
        for (target, expected) in [
            (TargetKind::Cell(4, 2), true),
            (TargetKind::Cell(20, 2), false),
        ] {
            assert_eq!(
                crate::sim::combat::can_fire_at_target(
                    &sim,
                    &rules,
                    sim.interner(),
                    actor,
                    &target,
                    sim.resolved_terrain().unwrap(),
                    Some(sim.house_alliances()),
                    &LineOfFireInputs {
                        overlay_grid: None,
                        overlay_registry: None,
                        alliances: Some(sim.house_alliances())
                    }
                ),
                expected
            );
            assert_eq!(
                crate::app::input::context_order::resolved_unit_in_range(
                    &sim,
                    actor,
                    &target,
                    Some(&rules),
                    sim.resolved_terrain(),
                    None,
                    Some(&target)
                ),
                expected
            );
        }
    }

    #[test]
    fn bunker_occupied_building_force_commands_reach_combat() {
        use crate::app::input::context_order::{AttackAction, armed_action};
        use crate::sim::combat::TargetKind;
        use crate::sim::command::{Command, CommandEnvelope};
        use crate::sim::passenger::PassengerRole;
        for cell_route in [false, true] {
            let (mut sim, rules, seed_tank) = stationary_attack_fixture();
            sim.set_input_delay_ticks(0);
            // Techno6FA30C..6FA46C drops an allied target unless its house
            // is human (House50B730). current_house alone is UI identity;
            // register the real house before spawning its counted objects.
            let owner = sim.entities().get(seed_tank).unwrap().owner();
            sim.houses_mut_for_test()
                .register(crate::sim::house_state::HouseState::new(
                    owner, 0, None, true, 0, 10,
                ));
            assert!(sim.owner_is_human(owner));
            let house = sim
                .spawn_object("HOUSE", "Americans", 2, 10, 0, &rules)
                .unwrap();
            // Prepare an already occupied building: this is a command-to-fire
            // regression, not a native boarding-lifecycle golden. The occupant
            // must not keep an independent map or Logic membership.
            let gi = sim
                .spawn_object_limbo_at_height("GI", "Americans", 2, 10, 0, 0, &rules)
                .unwrap();
            let target_id = sim
                .spawn_object("TANK", "Americans", 4, 10, 0, &rules)
                .unwrap();
            sim.entities_mut_for_test()
                .get_mut(house)
                .unwrap()
                .passenger_role
                .cargo_mut()
                .unwrap()
                .board_forced_chain(&[(gi, 1.0)]);
            sim.entities_mut_for_test()
                .get_mut(gi)
                .unwrap()
                .passenger_role = PassengerRole::inside_for_test(house, false);
            let occupant = sim.entities().get(gi).unwrap();
            assert!(occupant.lifecycle.in_limbo());
            assert!(!occupant.lifecycle.cell_marked());
            assert!(!occupant.in_logic_vector.is_member());
            assert!(!sim.live_object_order_snapshot().contains(&gi));
            assert_eq!(
                occupant.passenger_role.inside_transport(),
                Some((house, false))
            );
            let target = if cell_route {
                TargetKind::Cell(4, 10)
            } else {
                TargetKind::Entity(target_id)
            };
            assert_eq!(
                armed_action(&sim, &rules, house, &target, true, None),
                Some(AttackAction::Attack)
            );
            let command = if cell_route {
                Command::ForceAttackCell {
                    attacker_id: house,
                    target_rx: 4,
                    target_ry: 10,
                }
            } else {
                Command::Attack {
                    attacker_id: house,
                    target_id,
                }
            };
            assert_eq!(sim.entities().get(house).unwrap().owner(), owner);
            sim.queue_command(CommandEnvelope::new(
                owner,
                sim.session().tick() + 1,
                command,
            ));
            let grid = crate::sim::pathfinding::PathGrid::test_all_passable(32, 32);
            let state = |sim: &Simulation| {
                let actor = sim.entities().get(house).unwrap();
                format!(
                    "frame={} mission={:?} target={:?} leaf={:?} body={:?}/{:?} \
                     cargo={:?} rearm={:?} passive_timer={:?} passive={} \
                     frozen={} alive={} limbo={} logic={} health={} operational={:?}",
                    sim.session().binary_frame(),
                    actor.mission,
                    actor.attack_target,
                    actor.mission_leaf,
                    actor.building_body_state(),
                    actor.queued_building_body_state(),
                    actor.passenger_role.cargo(),
                    actor.rearm_timer.timer(),
                    actor.passive_scan.timer(),
                    actor.passive_scan.acquired(),
                    actor.ai_frozen(),
                    actor.lifecycle.object_alive(),
                    actor.lifecycle.in_limbo(),
                    actor.in_logic_vector.is_member(),
                    actor.health.current(),
                    sim.building_operational_state(house, &rules),
                )
            };
            let mut trace = vec![state(&sim)];
            let mut fired = false;
            for frame in 0..90 {
                let pending = sim.take_due_commands();
                sim.advance_tick(&pending, Some(&rules), Some(&grid), None, 100);
                if frame < 8 || frame == 89 {
                    trace.push(format!("commands={} {}", pending.len(), state(&sim)));
                }
                if sim
                    .fire_events()
                    .iter()
                    .any(|event| event.attacker_id == house && event.occupied_building)
                {
                    fired = true;
                    break;
                }
            }
            assert!(
                fired,
                "occupied building command must reach its occupant weapon: cell={cell_route}; \
                 final GetFireError={:?}; occupant={:?}; target={:?}; trace={trace:#?}",
                // Ask the live legality owner only after failure, so this
                // diagnostic cannot change the exercised frame sequence.
                sim.selected_weapon_fire_error(&rules, house, target, true, None),
                sim.entities().get(gi),
                sim.entities().get(target_id),
            );
        }
    }

    #[test]
    fn bunker_force_attack_input_queries_preserve_dummy_for_cell_and_entity() {
        use crate::app::input::context_order::armed_action;
        use crate::map::resolved_terrain::ResolvedTerrainGrid;
        use crate::sim::combat::TargetKind;
        use crate::sim::docking::bunker_link::BunkerLink;
        let (mut sim, rules, actor) = stationary_attack_fixture();
        let target = sim
            .spawn_object("TANK", "Soviets", 4, 2, 0, &rules)
            .unwrap();
        sim.entities_mut_for_test()
            .get_mut(actor)
            .unwrap()
            .bunker
            .set_bunker_link_for_test(BunkerLink::Installed(99));
        for id in [actor, target] {
            let entity = sim.entities_mut_for_test().get_mut(id).unwrap();
            entity.lifecycle.set_cell_marked_for_test(true);
            entity.position.set_exact_z_leptons_for_test(Some(900));
        }
        let mut retained = crate::map::resolved_terrain::test_flat_cell(0, 0);
        retained.bridge_facts.native_anchor =
            Some(crate::map::cell_index::NativeCellIdentity::Dummy);
        sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(
            1,
            1,
            vec![retained],
        ));
        let dummy = sim.effective_shared_cell_dummy();
        dummy.stamp_coord(111, -222);
        dummy.set_level_slope(2, 1);
        let original = dummy.snapshot();
        let hash = sim.state_hash();
        for target in [TargetKind::Cell(20, 2), TargetKind::Entity(target)] {
            for _ in 0..3 {
                let _ = armed_action(&sim, &rules, actor, &target, true, None);
                let _ = force_fire_feedback(&sim, actor, &target, 1, Some(&rules), None);
                assert_eq!(dummy.snapshot(), original);
                assert_eq!(sim.state_hash(), hash);
            }
        }
    }

    #[test]
    fn attack_cursor_missing_cell_queries_preserve_simulation_dummy_and_hash() {
        use crate::map::resolved_terrain::ResolvedTerrainGrid;
        use crate::sim::combat::{TargetKind, in_range, line_of_fire::LineOfFireInputs};

        // An AA projectile: at this height a ground-only shot is ILLEGAL
        // (T38, `0x006FC705`), and the cursor then asks no range. With it,
        // What_Action's GetFireError and the cursor's range query both read
        // the missing cells.
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=300\nPrimary=105mm\n\
             [105mm]\nDamage=60\nRange=5\nProjectile=Flak\n[Flak]\nAA=yes\n",
        ))
        .unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        let actor_id = sim
            .spawn_object("MTNK", "Americans", 2, 2, 0, &rules)
            .expect("tank spawned");
        // The attack rung is a human player's (`0x007003ED`).
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        let house = Some(sim.entities().get(actor_id).unwrap().owner());
        sim.session_mut_for_test().set_current_house_for_test(house);
        let target_id = sim
            .spawn_object("MTNK", "Soviets", 3, 2, 0, &rules)
            .expect("enemy target");
        // Both live objects deliberately miss this allocated map. Their exact
        // Z and marked state force source +54 and target +50/+54 to read the
        // native fallback Cell rather than bypassing the query as limbo objects.
        for id in [actor_id, target_id] {
            let entity = sim.entities_mut_for_test().get_mut(id).unwrap();
            entity.position.set_exact_z_leptons_for_test(Some(900));
            entity.lifecycle.set_cell_marked_for_test(true);
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
                    crate::app::input::context_order::OrderModifier::Normal,
                    false,
                    false,
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
        let terrain = sim.resolved_terrain().unwrap();
        let actor = sim.entities().get(actor_id).unwrap();
        let target = TargetKind::Entity(target_id);
        let weapon = rules.weapon("105mm").unwrap();
        let source = in_range::fire_source_coords(
            actor,
            &target,
            weapon,
            &sim,
            terrain,
            (&rules, sim.interner()),
        )
        .unwrap();
        assert_eq!(dummy.snapshot().coord, (2, 2));
        assert!(in_range::compute_in_range(
            actor,
            source,
            &target,
            weapon,
            &rules,
            sim.interner(),
            &sim,
            terrain,
            &LineOfFireInputs::terrain_only(),
        ));
        // InRange's final source-cell bridge query6F7601 follows the
        // target height/ground queries, restoring the source's Dummy XY.
        assert_eq!(dummy.snapshot().coord, (2, 2));
        assert_ne!(sim.state_hash(), initial_hash);
    }

    /// The cursor asks the sim's What_Action as the cursor update does, with
    /// the mouse-over argument (`0x0069300B`), so no Can_Enter_Cell: a retail
    /// Grizzly shows Move over the river that its click resolves to its own
    /// bank.
    #[test]
    fn the_cursor_shows_move_where_the_click_redirects() {
        use crate::app::input::context_order::OrderModifier;
        use crate::sim::world::MoveAction;
        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let (mut sim, rules, _) = Simulation::walk_cell_input_test_scene_with(battle.rules, false);
        let owner = sim.interner().get("Local").unwrap();
        let tank = sim.spawn_object("MTNK", "Local", 6, 4, 0, &rules).unwrap();
        sim.resolve_type_handles(&rules);
        let cursor = |modifier| {
            what_action_on_cell(
                &sim,
                Some(owner),
                Some(tank),
                (9, 5),
                modifier,
                Some(&rules),
                None,
                true,
            )
        };
        assert_eq!(cursor(OrderModifier::Normal), MoveAction::Move);
        assert_eq!(cursor(OrderModifier::ForceMove), MoveAction::Move);
        let goal = sim
            .cell_click_destination(owner, tank, (9, 5), false, &rules, None)
            .unwrap()
            .unwrap();
        assert!(goal.0 < 8, "{goal:?}");
    }

    /// The Unit harvest arms run only over a Cell the viewer has mapped
    /// (`0x007405DB..0x007405E6`): a retail War Miner shows Harvest over ore
    /// it can see and Move over ore under shroud.
    #[test]
    fn a_miner_shows_harvest_only_over_mapped_ore() {
        use crate::app::input::context_order::OrderModifier;
        use crate::app::presentation::render::OrderMode;
        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
            &battle.processed_rules,
            Some(&battle.fixed_art),
        );
        let (mut sim, rules, _) = Simulation::walk_cell_input_test_scene_with(battle.rules, false);
        let owner = sim.interner().get("Local").unwrap();
        let miner = sim.spawn_object("HARV", "Local", 6, 4, 0, &rules).unwrap();
        sim.resolve_type_handles(&rules);
        let ore = registry.id_for_name("TIB01").unwrap();
        let (seen, hidden) = ((11, 5), (12, 5));
        for cell in [seen, hidden] {
            sim.place_overlay_for_test(cell, ore, 6, &registry);
        }
        *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(16, 16);
        sim.fog_mut_for_test().reveal_cells_for_owner(
            owner,
            (0..16)
                .flat_map(|y| (0..16).map(move |x| (x, y)))
                .filter(|&cell| cell != hidden),
        );
        let cursor = |cell| {
            empty_cell_cursor(
                &sim,
                Some(&rules),
                Some(&registry),
                Some(owner),
                Some(miner),
                cell,
                OrderModifier::Normal,
                OrderMode::Move,
                true,
            )
        };
        assert_eq!(cursor(seen), CursorFeedbackKind::Harvest);
        assert_eq!(cursor(hidden), CursorFeedbackKind::Move);
    }

    /// With nothing selected gamemd never asks a What_Action for the hovered Cell:
    /// `DisplayClass__DetermineAction @ 0x00692610` returns the selected object's
    /// vtable `+0x70` answer from inside `if (g_CurrentObjects_Count != 0)`, so an
    /// empty selection answers no action and the pointer stays the plain arrow —
    /// including over unexplored ground, where the Move cursor used to appear.
    #[test]
    fn an_empty_selection_answers_the_default_cursor_on_a_bare_cell() {
        use crate::app::input::context_order::OrderModifier;
        use crate::app::presentation::render::OrderMode;
        let (sim, rules, _) = Simulation::walk_cell_input_test_scene(false);
        let owner = sim.interner().get("Local").unwrap();

        assert_eq!(
            empty_cell_cursor(
                &sim,
                Some(&rules),
                None,
                Some(owner),
                None,
                (9, 5),
                OrderModifier::Normal,
                OrderMode::Move,
                true,
            ),
            CursorFeedbackKind::Default,
        );
    }

    /// `Is_Armed` reads the current weapon slot, slot 0 without `TurretCount=`,
    /// so a `Secondary=`-only unit is unarmed like the truck (score 2), and
    /// the nearer of the two wins.
    #[test]
    fn a_secondary_only_unit_is_unarmed_and_the_nearer_one_wins() {
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
            &[arty, truck],
            ActionDistanceTarget::CellCentre(10, 10),
            Some(&rules),
        );
        assert_eq!(best, Some(truck));
    }

    /// A berserk object scores 0 (`0x00535435`), below even an unarmed one,
    /// however near it is.
    #[test]
    fn a_berserk_unit_ranks_below_every_calm_one() {
        let rules = cell_action_rules();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);

        let tank = sim
            .spawn_object("MTNK", "Americans", 9, 10, 0, &rules)
            .expect("near tank");
        let truck = sim
            .spawn_object("TRUCKA", "Americans", 2, 2, 0, &rules)
            .expect("far unarmed truck");
        let best = |sim: &Simulation| {
            select_best_for_action(
                sim,
                &[tank, truck],
                ActionDistanceTarget::CellCentre(10, 10),
                Some(&rules),
            )
        };
        assert_eq!(best(&sim), Some(tank));
        sim.entities_mut_for_test()
            .get_mut(tank)
            .unwrap()
            .berserk
            .set_active_for_test(true);
        assert_eq!(best(&sim), Some(truck));
    }

    /// Only a positive `GetWeaponDamageValue(-1)` reaches score 5
    /// (`0x00535493`); an armed unit whose weapon heals stays at 4.
    #[test]
    fn a_healing_weapon_stays_below_a_damaging_one() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n1=MEND\n\
             [MTNK]\nStrength=300\nPrimary=105mm\n\
             [MEND]\nStrength=300\nPrimary=Fix\n\
             [WeaponTypes]\n0=105mm\n1=Fix\n\
             [105mm]\nDamage=60\nRange=5\n\
             [Fix]\nDamage=-30\nRange=5\n",
        ))
        .expect("healer rules");
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);

        let mender = sim
            .spawn_object("MEND", "Americans", 9, 10, 0, &rules)
            .expect("near mender");
        let tank = sim
            .spawn_object("MTNK", "Americans", 2, 2, 0, &rules)
            .expect("far tank");
        let best = select_best_for_action(
            &sim,
            &[mender, tank],
            ActionDistanceTarget::CellCentre(10, 10),
            Some(&rules),
        );
        assert_eq!(best, Some(tank));
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
        if let Some(e) = sim.entities_mut_for_test().get_mut(far) {
            e.position
                .set_sub_x_for_test(crate::util::fixed_math::SimFixed::from_num(-120));
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
        let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
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
                sim.session_mut_for_test().set_game_mode(
                    crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
                );
                let house = Some(sim.intern("Americans"));
                sim.session_mut_for_test().set_current_house_for_test(house);
                // The ordinary Infantry Unlimbo query reads the native
                // Clear/Foot speed row before this cursor-only observation.
                let mut terrain = crate::map::resolved_terrain::test_flat_ground_grid(20);
                terrain.test_set_high_bridge_set_starts(Some(100), Some(200));
                sim.install_resolved_terrain_for_new_map(terrain);
                for y in 9..=11 {
                    sim.overlay_cell_mut_for_test(12, y).overlay_id =
                        Some(if collapsed { 0xE7 } else { 0xD4 });
                }
                *sim.bridge_state_mut_for_test() =
                    Some(crate::sim::bridge_state::BridgeRuntimeState::default());
                crate::sim::arena_fixture::supply_native_map(&mut sim);
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
                sim.entities_mut_for_test()
                    .get_mut(hut)
                    .unwrap()
                    .health
                    .set_for_test(150);
                if relation == "allied" {
                    sim.house_alliances_mut_for_test()
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
                    sim.scenario_rng().logical_state(),
                    sim.main_rng().logical_state(),
                    sim.mapgen_rng().logical_state(),
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
                    crate::app::input::context_order::OrderModifier::Normal,
                    false,
                    false,
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
                        sim.scenario_rng().logical_state(),
                        sim.main_rng().logical_state(),
                        sim.mapgen_rng().logical_state()
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
        capability_cursor_for_hover(
            sim,
            &[actor],
            Some(actor),
            &hover,
            Some(rules),
            None,
            crate::app::input::context_order::OrderModifier::Normal,
            false,
            false,
        )
    }

    /// `InfantryClass::What_Action_OnObject`: a Crazy Ivan offers IvanBomb on
    /// any target without a bomb — enemy or, by AttackCursorOnFriendlies, own.
    /// A bombed target's GetFireError is ILLEGAL to him near or far
    /// (`0x006FCBAD`), so the Techno tail answers (`0x0070056C..0x007005E6`):
    /// 0 over an enemy, which his Infantry tail makes NoMove (`0x0051F05E`),
    /// and Select over his own. An Engineer offers DisarmBomb on any bombed
    /// object its player sees, own or enemy, and nothing on an unseen one.
    #[test]
    fn crazy_ivan_and_engineer_bomb_cursors() {
        let rules = bomb_cursor_rules();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        for house in ["Americans", "Soviets"] {
            let id = sim.intern(house);
            sim.houses_mut_for_test()
                .register(crate::sim::house_state::HouseState::new(
                    id, 0, None, false, 0, 10,
                ));
        }
        // This is a local player's selected actor. Techno700C40's virtual
        // +A0 reaches House50B6F0; a house_order entry alone is not LocalPlayer.
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        let house = sim.interner().get("Americans");
        sim.session_mut_for_test().set_current_house_for_test(house);
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

        assert!(sim.techno_player_controllable(ivan, &rules));
        assert!(sim.techno_player_controllable(engineer, &rules));

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
        sim.bomb_attach_for_test(ivan, Some(enemy_tank), &rules);
        sim.bomb_attach_for_test(ivan, Some(own_tank), &rules);
        sim.bomb_attach_for_test(enemy_ivan, Some(far_tank), &rules);
        sim.bomb_list_update_for_test(&rules);
        sim.bomb_list_update_for_test(&rules);

        assert_eq!(
            bomb_hover(&sim, &rules, ivan, EnemyUnit, enemy_tank),
            CursorFeedbackKind::Invalid,
            "a bombed enemy: NoMove"
        );
        assert_eq!(
            bomb_hover(&sim, &rules, ivan, FriendlyUnit, own_tank),
            CursorFeedbackKind::FriendlyUnit,
            "a bombed own object: Select"
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
}

#[cfg(test)]
mod cursor_attack_support_tests {
    use super::force_fire_feedback;
    use crate::app::types::CursorFeedbackKind;

    /// SetCursorFromAction (`0x004AAE90`) has no case for a healing Unit's
    /// AttackSupport (0x3B): Ctrl over the ground shows the arrow.
    #[test]
    fn retail_engineer_ifv_ctrl_over_the_ground_shows_the_arrow() {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = &retail.rules;
        let (sim, ifv, _) =
            crate::app::input::context_order::retail_ifv_repair_tests::repair_ifv(rules);
        assert_eq!(
            force_fire_feedback(
                &sim,
                ifv,
                &crate::sim::combat::TargetKind::Cell(16, 19),
                1,
                Some(rules),
                None,
            ),
            Some(Some(CursorFeedbackKind::Default))
        );
    }
}

#[cfg(test)]
mod cursor_animation_tests {
    use super::CursorAnimation;
    use crate::app::input::entity_pick::HoverTargetKindWithId;
    use crate::app::types::{CursorFeedbackKind, CursorId, HoverTargetKind};

    /// Retail interval for every animated cursor row: rate 4 x 16 ms.
    const INTERVAL_MS: u64 = 64;

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

    /// The empty-selection hover answer is row 17 (Select), the same row the
    /// with-selection friendly hover uses — DetermineAction's no-selection arm
    /// (`0x00692610`) answers action 7 with no ownership test, so it is its own
    /// feedback kind rather than FriendlyUnit's.
    #[test]
    fn empty_selection_hover_maps_to_the_select_row() {
        let hover = |kind| HoverTargetKindWithId { kind, stable_id: 1 };
        assert_eq!(
            super::no_selection_cursor(Some(&hover(HoverTargetKind::FriendlyUnit))),
            Some(CursorFeedbackKind::Select)
        );
        assert_eq!(
            super::no_selection_cursor(Some(&hover(HoverTargetKind::EnemyUnit))),
            Some(CursorFeedbackKind::Select),
            "the no-selection arm carries no ownership test"
        );
        assert_eq!(
            super::no_selection_cursor(Some(&hover(HoverTargetKind::FriendlyStructure))),
            Some(CursorFeedbackKind::Select)
        );
        assert_eq!(
            super::no_selection_cursor(Some(&hover(HoverTargetKind::EnemyStructure))),
            Some(CursorFeedbackKind::Select)
        );
        assert_eq!(
            super::no_selection_cursor(None),
            None,
            "no object under the cursor keeps the default arrow"
        );
        assert_eq!(
            super::cursor_id_for_feedback(CursorFeedbackKind::Select),
            Some(CursorId::Select)
        );
    }

    /// A selected own unit under its own cursor answers NoMove — the barred
    /// cursor — not Select: retail's move order refuses a cell held by an own
    /// selected techno and its select action rejects an already-selected one
    /// (`TechnoClass::What_Action_OnObject` `0x0070000B`, `[0xA8ECC8] == 1` at
    /// `0x007000C1`, see `self_click_command`). An own unit that is not
    /// selected keeps Select; structures and enemies are not this arm.
    #[test]
    fn selected_own_unit_self_hover_answers_no_move() {
        let hover = |kind, stable_id| HoverTargetKindWithId { kind, stable_id };
        let selected = [7u64, 9];
        assert!(super::hover_is_selected_own_unit(
            &hover(HoverTargetKind::FriendlyUnit, 9),
            &selected
        ));
        assert!(!super::hover_is_selected_own_unit(
            &hover(HoverTargetKind::FriendlyUnit, 8),
            &selected
        ));
        assert!(!super::hover_is_selected_own_unit(
            &hover(HoverTargetKind::FriendlyStructure, 9),
            &selected
        ));
        assert!(!super::hover_is_selected_own_unit(
            &hover(HoverTargetKind::EnemyUnit, 9),
            &selected
        ));
        assert!(!super::hover_is_selected_own_unit(
            &hover(HoverTargetKind::FriendlyUnit, 9),
            &[]
        ));
        // The cursor that answer lands on is row 19's barred frame.
        assert_eq!(
            super::cursor_id_for_feedback(CursorFeedbackKind::Invalid),
            Some(CursorId::NoMove)
        );
    }

    // Native query and object-route admission are now compared through the
    // shared587410 owner and engineer_bridge_cursor_caller corpus. Minimap
    // hover, full base WhatAction/modifiers and final SHP drawing remain
    // outside this tactical object-route comparison.
}

#[cfg(test)]
#[path = "cursor_contract_tests.rs"]
mod contract_tests;
