//! Control groups and team keys: digit-press resolution, group assignment,
//! and the recall, add-team and centre-camera team walks.

use super::{
    KeyModifiers, ORDINARY_SELECTION_ACTION_LINE_POLICY, TEAM_SELECTION_VOICE_POLICY,
    apply_selection_action_line_policy, apply_selection_mutation, selected_stable_ids_in_order,
};
use crate::app::PlatformState;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::state::FrontendState;
use crate::app::input::commands::preferred_command_owner;
use crate::app::input::entity_pick::SelectionMutation;
use crate::app::match_runtime::sim_host::SimHost;
use crate::app::match_runtime::sim_host::SimWrite;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::audio::sfx::SfxPlayer;
use crate::map::entities::EntityCategory;
use crate::sim::command::Command;

/// Double-tap window for the centre-on-group shortcut, in milliseconds.
/// The engine compares `timeGetTime()` against the last recall stamp, so this
/// is wall clock and never sim state.
const GROUP_DOUBLE_TAP_MS: u128 = 800;

/// What a digit press resolves to before any state is touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GroupPressAction {
    /// Ctrl+digit — replace the slot's membership with the current selection.
    Assign,
    /// Shift+digit — add the slot's members to the current selection.
    AddToSelection,
    /// Alt+digit, or a bare double-tap — put the camera on the group.
    Center,
    /// Bare digit — clear the selection and select the slot's members.
    Recall,
}

/// Resolve a control-group digit press.
///
/// The four team commands are separate bindings — bare digit, Shift+digit,
/// Ctrl+digit, Alt+digit — and the dispatcher matches the modifier bits
/// exactly, so a two-modifier chord matches no binding and does nothing.
///
/// The double-tap arm is the subtle one: the recall routine only centres when
/// the current selection is *exactly* the group. It bails out on the first
/// group member that is unselected and on the first selected object outside the
/// group — two distinct tests, not "at least one member selected". After a
/// plain recall that condition holds, which is why the familiar double-tap
/// works; shift-clicking one extra unit between the taps makes the second tap
/// recall instead of centre. Only a plain recall stamps the timer.
pub(super) fn control_group_press_action(
    modifiers: KeyModifiers,
    slot: usize,
    group: &[u64],
    selected: &[u64],
    last_press: Option<(usize, std::time::Duration)>,
) -> Option<GroupPressAction> {
    if modifiers.only_ctrl() {
        return Some(GroupPressAction::Assign);
    }
    if modifiers.only_shift() {
        return Some(GroupPressAction::AddToSelection);
    }
    if modifiers.only_alt() {
        return Some(GroupPressAction::Center);
    }
    if modifiers.any() {
        // Two modifiers at once: no binding carries that key value.
        return None;
    }
    let within_window = last_press.is_some_and(|(last_slot, elapsed)| {
        last_slot == slot && elapsed.as_millis() < GROUP_DOUBLE_TAP_MS
    });
    let selection_is_exactly_the_group = !group.is_empty()
        && group.iter().all(|id| selected.contains(id))
        && selected.iter().all(|id| group.contains(id));
    if within_window && selection_is_exactly_the_group {
        return Some(GroupPressAction::Center);
    }
    Some(GroupPressAction::Recall)
}

/// A Team button's left release (`TabClass::Action @ 0x006D0680`,
/// 0x006D07EC..0x006D0822): `Team__CountMembers @ 0x00730A10` zero assigns
/// the selection (`TeamAssign 0x00731060`); else
/// `Team__AllMembersSelected @ 0x00730990` centres (`0x007313A0`) and anything
/// less recalls (`0x007311C0`). Both predicates skip what the team walks skip,
/// so `group` and `selected` arrive already filtered by `team_walk_members`.
pub(super) fn command_bar_team_press_action(group: &[u64], selected: &[u64]) -> GroupPressAction {
    if group.is_empty() {
        GroupPressAction::Assign
    } else if group.iter().all(|id| selected.contains(id)) {
        GroupPressAction::Center
    } else {
        GroupPressAction::Recall
    }
}

/// Who pressed a control group: a key binding, a command-bar Team button,
/// or a binding with one fixed action.
#[derive(Debug, Clone, Copy)]
pub(super) enum GroupPress {
    Key,
    CommandBar,
    Fixed(GroupPressAction),
}

pub(super) fn control_group_action_for_input(
    modifiers: KeyModifiers,
    slot: usize,
    group: &[u64],
    selected: &[u64],
    last_press: Option<(usize, std::time::Duration)>,
    sim: Option<&crate::sim::world::Simulation>,
) -> Option<GroupPressAction> {
    // Recall's double-tap test (`0x0073123B..0x0073129D`) walks only
    // non-limbo objects of eligible owners, as the team walks do.
    control_group_press_action(
        modifiers,
        slot,
        &team_walk_members(sim, group),
        &team_walk_members(sim, selected),
        last_press,
    )
}

/// CreateTeamCommand535CC0 passes its one-based parameter to TeamAssign731060;
/// TeamClear7310D0 (command bar6D0990) performs its clear without setting any
/// selected fields. The simulation owns both effects on Techno+214, or in a
/// network game on the local player's slot (`GameEntity::control_group`). Registered
/// digit0 is parameter10, hence raw group9; digits1..9 map to raw0..8.
pub(super) fn schedule_control_group_assignment(
    sim: &mut crate::sim::world::Simulation,
    owner: &str,
    slot: usize,
    entity_ids: Vec<u64>,
) -> Option<u64> {
    if slot >= 10 {
        return None;
    }
    crate::app::input::commands::schedule_command_in_sim(
        sim,
        owner,
        Command::AssignControlGroup {
            group: ((slot + 9) % 10) as i32,
            entity_ids,
        },
    )
}

pub(super) fn queue_control_group_assignment(
    sim_runtime: &mut SimHost,
    slot: usize,
    entity_ids: Vec<u64>,
) {
    let Some(owner) = preferred_command_owner(sim_runtime) else {
        return;
    };
    let Some(runtime) = sim_runtime.write_sim(SimWrite::Command) else {
        return;
    };
    let _ = schedule_control_group_assignment(&mut runtime.simulation, &owner, slot, entity_ids);
}

/// CenterCamera's Unselect_All gate (`0x007313E3..0x00731419`): a non-empty
/// selection whose first object is not a Foot (AbstractFlags bit 4,
/// `0x007313F8..0x00731404`) of this team (`+0x214`, `0x0073140B`). A team
/// building first in the selection deselects everything.
fn center_team_deselects(
    sim: Option<&crate::sim::world::Simulation>,
    selected: &[u64],
    group: &[u64],
) -> bool {
    selected.first().is_some_and(|first| {
        let foot = sim
            .and_then(|sim| sim.entities().get(*first))
            .is_some_and(|entity| entity.category() != EntityCategory::Structure);
        !(foot && group.contains(first))
    })
}

/// A team key's change to the selection.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct TeamSelection {
    pub(super) clear: bool,
    pub(super) select: Vec<u64>,
}

/// What CenterCamera `0x007313A0`, AddTeam `0x007314C0` or Recall
/// `0x007311C0` does to the selection. `None` when the key clears nothing
/// and finds no unselected member to Select, so nothing is queued.
pub(super) fn team_selection(
    sim: Option<&crate::sim::world::Simulation>,
    action: GroupPressAction,
    group: &[u64],
    selected: &[u64],
) -> Option<TeamSelection> {
    let select = team_walk_members(sim, group);
    let clear = match action {
        GroupPressAction::Recall => {
            return Some(TeamSelection {
                clear: true,
                select,
            });
        }
        GroupPressAction::Center => center_team_deselects(sim, selected, group),
        GroupPressAction::AddToSelection => false,
        GroupPressAction::Assign => return None,
    };
    (clear || select.iter().any(|id| !selected.contains(id)))
        .then_some(TeamSelection { clear, select })
}

/// The members a team walk Selects, in its order. Recall `0x007311C0`,
/// AddTeam `0x007314C0` and CenterCamera `0x007313A0` walk the Techno array
/// from index 0 (construction order, which ascending stable ids keep; see
/// `Simulation`'s threat rebuild) and skip limbo (`+0x81`) before calling
/// Select; members already selected (`+0x83`) are skipped by the ledger.
/// Each Select runs the ledger's gate and Unselect_All, so a lone selected
/// enemy is drained by the first member.
pub(super) fn team_walk_members(
    sim: Option<&crate::sim::world::Simulation>,
    group: &[u64],
) -> Vec<u64> {
    let Some(sim) = sim else {
        return Vec::new();
    };
    let mut members: Vec<u64> = live_group_members(Some(sim), group)
        .into_iter()
        .filter(|id| {
            sim.entities()
                .get(*id)
                .is_some_and(|entity| !entity.in_limbo())
        })
        .collect();
    members.sort_unstable();
    members
}

/// Existing eligible members in retained membership order. The House owner decides the
/// mode-zero PlayerControl/nonzero current-House predicate. Limbo policy is
/// left to the selection consumer; assignment and clear use the simulation owner.
pub(super) fn live_group_members(
    sim: Option<&crate::sim::world::Simulation>,
    group: &[u64],
) -> Vec<u64> {
    let Some(sim) = sim else {
        return Vec::new();
    };
    let control = sim.control_projection();
    group
        .iter()
        .copied()
        .filter(|id| {
            sim.entities()
                .get(*id)
                .is_some_and(|entity| control.can_select(entity.owner()))
        })
        .collect()
}

pub(super) fn handle_control_group_command(
    platform: &PlatformState,
    renderer: &mut RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
    persistence: &PersistenceState,
    group_idx: usize,
    press: GroupPress,
) {
    if group_idx >= 10 {
        return;
    }
    let sim = match_state.sim_runtime.runtime().map(|rt| &rt.simulation);
    let group = sim
        .map(|sim| sim.projected_control_groups()[group_idx].clone())
        .unwrap_or_default();
    let selected = selected_stable_ids_in_order(
        sim,
        crate::app::state::active_rules(frontend, match_state),
        &match_state.input.selection_order,
        match_state.input.selection_order_pending,
    );
    let last_press = match_state
        .input
        .last_control_group_press
        .map(|(slot, at)| (slot, at.elapsed()));
    let action = match press {
        GroupPress::Fixed(action) => action,
        GroupPress::CommandBar => command_bar_team_press_action(
            &team_walk_members(sim, &group),
            &team_walk_members(sim, &selected),
        ),
        GroupPress::Key => control_group_action_for_input(
            KeyModifiers::default(),
            group_idx,
            &group,
            &selected,
            last_press,
            sim,
        )
        .expect("a bare team-select binding always resolves"),
    };

    if action != GroupPressAction::Assign {
        // Recall731205, CenterCamera7313DE and AddTeam7314FD each START by
        // turning off every armed map mode on DisplayClass 0x87F7E8:
        // SetTogglePowerMode(0) 0x4AC820 (0x7311D3, 0x7313AC, 0x7314CB; only
        // Ares' TogglePower command arms it), SetWaypointMode
        // (0,0) 0x4AC700 — its +0x11B3 byte is never armed in YR, the only
        // write of 1 (0x004AC7A7) only ever runs with 0, see cursor.rs —
        // SetRepairMode(0) 0x4AC8C0 and SetSellMode(0) 0x4AC660, then
        // SetPlaceBeaconMode(0) 0x4AC960. The mode owners refuse these under
        // a pending placement themselves. The PlanMgr
        // planning mode is a separate owner these Display setters do not
        // cancel. TeamAssign 731060 is not evidenced to clear any mode, so
        // Assign keeps every armed mode as it is.
        crate::app::input::power_mode::disable(match_state);
        match_state
            .match_presentation
            .sidebar_gadget_state
            .disable_selection_modes(match_state.input.armed_building_type().is_some());
        crate::app::input::beacon::set_place_beacon_mode(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            sfx_player,
            persistence,
            crate::app::input::beacon::BeaconModeRequest::Off,
        );
    }
    let sim = match_state.sim_runtime.runtime().map(|rt| &rt.simulation);
    match action {
        GroupPressAction::Assign => {
            queue_control_group_assignment(&mut match_state.sim_runtime, group_idx, selected);
        }
        GroupPressAction::Center => {
            // Alt+digit and Recall's double tap both run CenterCamera, which
            // centres on the resulting selection (`MapClass::CenterMap
            // 0x004AE290` at `0x0073149E`), extra kept units included.
            if let Some(selection) = team_selection(sim, action, &group, &selected) {
                queue_team_selection(
                    platform,
                    renderer,
                    diag,
                    frontend,
                    match_state,
                    process_assets,
                    sfx_player,
                    persistence,
                    selection,
                );
            }
            crate::app::input::camera::center_view_on_selection(
                platform,
                renderer,
                frontend,
                match_state,
                persistence,
            );
        }
        GroupPressAction::AddToSelection => {
            if let Some(selection) = team_selection(sim, action, &group, &selected) {
                queue_team_selection(
                    platform,
                    renderer,
                    diag,
                    frontend,
                    match_state,
                    process_assets,
                    sfx_player,
                    persistence,
                    selection,
                );
            }
        }
        GroupPressAction::Recall => {
            // A recall on an empty group still clears the selection: the
            // deselect-all runs before the select loop, unconditionally.
            match_state.input.last_control_group_press =
                Some((group_idx, crate::util::time::Instant::now()));
            if let Some(selection) = team_selection(sim, action, &group, &selected) {
                queue_team_selection(
                    platform,
                    renderer,
                    diag,
                    frontend,
                    match_state,
                    process_assets,
                    sfx_player,
                    persistence,
                    selection,
                );
            }
            apply_selection_action_line_policy(match_state, ORDINARY_SELECTION_ACTION_LINE_POLICY);
        }
    }
}

/// One team walk's Selects (Recall, AddTeam, CenterCamera), with their
/// latch rule.
fn queue_team_selection(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
    persistence: &PersistenceState,
    selection: TeamSelection,
) {
    apply_selection_mutation(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        sfx_player,
        persistence,
        SelectionMutation {
            clear: selection.clear,
            select: selection.select,
            ..Default::default()
        },
        TEAM_SELECTION_VOICE_POLICY,
    );
}
