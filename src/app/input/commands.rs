//! Build and production commands — queuing builds, placing buildings, owner management.
//!
//! Split from the presentation render path. Part of the app layer — may depend on everything.

use std::collections::HashMap;

use crate::app::AppState;
use crate::map::entities::EntityCategory;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::intern::InternedId;
use crate::sim::production;

#[cfg(test)]
#[path = "ground_command_native_tests.rs"]
mod ground_command_native_tests;

/// Default owner name when no playable house is found.
const DEFAULT_OWNER: &str = "Americans";

/// Intern a string (owner name or type id) against the live simulation's
/// interner, returning its InternedId. Returns the default ID when there is
/// no simulation yet.
fn intern_in_sim(state: &mut AppState, s: &str) -> InternedId {
    state
        .match_state
        .sim_runtime
        .as_mut()
        .map(|rt| &mut rt.simulation)
        .map(|sim| sim.intern(s))
        .unwrap_or_default()
}

/// Resolve the local owner and return the owned String.
///
/// Read-only: identity comes from the live scenario's saved current House
/// (or the sandbox heuristic when absent). Command issue never rewrites it —
/// the old per-action override write made "who am I" drift with whatever
/// the heuristic last returned, a lockstep hazard.
fn resolve_owner(state: &mut AppState) -> String {
    preferred_local_owner(state).unwrap_or_else(|| DEFAULT_OWNER.to_string())
}

/// Queue `repeat` builds of `type_id` as `repeat` ordinary PRODUCE events.
///
/// `repeat` comes from [`crate::app::input::sidebar_eva::shift_queue_repeat`],
/// which reads the configured `[VERA] ShiftQueueCount=`; its default lives with
/// the profile that owns the setting.
///
/// Each copy is the same deterministic command, so several of them replay and
/// hash exactly like repeated clicks. All of them carry the current tick, so
/// they execute in one frame and in issue order.
pub(crate) fn queue_build_by_type(state: &mut AppState, type_id: &str, repeat: u32) {
    let owner: String = resolve_owner(state);
    let type_interned = intern_in_sim(state, type_id);
    for _ in 0..repeat.max(1) {
        schedule_command(
            state,
            &owner,
            Command::QueueProduction {
                type_id: type_interned,
            },
        );
    }
    log::info!(
        "Build command queued: owner={} type={} count={} issue_frame=current",
        owner,
        type_id,
        repeat.max(1)
    );
}

pub(crate) fn suspend_build(state: &mut AppState, category: production::ProductionCategory) {
    let owner: String = resolve_owner(state);
    schedule_command(state, &owner, Command::SuspendProduction { category });
    log::info!(
        "Build hold command queued: owner={} category={} issue_frame=current",
        owner,
        category.label()
    );
}

pub(crate) fn cycle_active_producer(
    state: &mut AppState,
    category: production::ProductionCategory,
) {
    let owner: String = resolve_owner(state);
    schedule_command(state, &owner, Command::CycleProducerFocus { category });
    log::info!(
        "Producer focus cycle queued: owner={} category={} issue_frame=current",
        owner,
        category.label()
    );
}

pub(crate) fn cancel_build_by_type(state: &mut AppState, type_id: &str, all: bool) {
    let owner: String = resolve_owner(state);
    let type_interned = intern_in_sim(state, type_id);
    schedule_command(
        state,
        &owner,
        Command::CancelProductionByType {
            type_id: type_interned,
            all,
        },
    );
    log::info!(
        "Build cancel-by-type queued: owner={} type={} all={} issue_frame=current",
        owner,
        type_id,
        all
    );
}

/// Stable id of the visible object selected by the tactical object picker.
fn visible_object_under_point(state: &AppState, world_x: f32, world_y: f32) -> Option<u64> {
    let sim = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)?;
    let owner = preferred_local_owner(state)?;
    crate::app::input::entity_pick::hover_target_at_point(
        crate::app::input::entity_pick::presented(state, sim),
        world_x,
        world_y,
        &owner,
        state.match_state.sandbox_full_visibility,
    )
    .map(|hover| hover.stable_id)
}

/// SuperWeaponType6CEF80's pointer gate, shared by the cursor and release.
/// Type10 accepts only the picked Building whose owner considers the current
/// player allied (original4F9A50, called with the target House as ECX).
/// The native launch body and non-pointer producers do not have this gate.
fn super_weapon_pointer_target_is_valid(
    sim: &crate::sim::world::Simulation,
    viewer: InternedId,
    kind: crate::rules::superweapon_type::SuperWeaponKind,
    picked: Option<u64>,
) -> bool {
    if kind != crate::rules::superweapon_type::SuperWeaponKind::ForceShield {
        return true;
    }
    picked
        .and_then(|id| sim.entities().get(id))
        .is_some_and(|target| {
            target.category() == crate::map::entities::EntityCategory::Structure
                && crate::map::houses::is_allied_with(
                    sim.house_alliances(),
                    sim.interner().resolve(target.owner()),
                    sim.interner().resolve(viewer),
                )
        })
}

/// Reuse the ordinary visible-object picker; do not prefer a building behind
/// the object the pointer actually selects. No target or alliance is retained.
pub(crate) fn super_weapon_pointer_target_at_cursor_is_valid(
    state: &AppState,
    section: &str,
) -> bool {
    let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
        return true;
    };
    let Some(sw) = runtime.resources.rules.super_weapon(section) else {
        return true;
    };
    if sw.kind != crate::rules::superweapon_type::SuperWeaponKind::ForceShield {
        return true;
    }
    let Some(viewer) =
        preferred_local_owner(state).and_then(|owner| runtime.simulation.interner().get(&owner))
    else {
        return false;
    };
    let world = crate::app::match_runtime::sim_tick::screen_point_to_world(
        state,
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    super_weapon_pointer_target_is_valid(
        &runtime.simulation,
        viewer,
        sw.kind,
        visible_object_under_point(state, world.0, world.1),
    )
}

fn own_building_id(state: &AppState, stable_id: u64) -> Option<u64> {
    let sim = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)?;
    let owner = preferred_local_owner(state)?;
    let entity = sim.entities().get(stable_id)?;
    (entity.category() == EntityCategory::Structure
        && sim
            .interner()
            .resolve(entity.owner())
            .eq_ignore_ascii_case(&owner))
    .then_some(stable_id)
}

/// The local player's own building under the given world point when the
/// repair cursor takes it: `DisplayClass::DetermineAction 0x006927CD..
/// 0x0069280D` needs its owner to be the local player (`IsHumanPlayer`) and
/// asks the building's `Can_Repair` (`production::can_repair_building`).
/// Shared by the repair cursor feedback (`input::cursor`) and the repair
/// click handler below so the two never disagree about the target.
pub(crate) fn own_repairable_building_under_point(
    state: &AppState,
    world_x: f32,
    world_y: f32,
) -> Option<u64> {
    own_repairable_building_id(state, visible_object_under_point(state, world_x, world_y)?)
}

fn own_repairable_building_id(state: &AppState, stable_id: u64) -> Option<u64> {
    let id = own_building_id(state, stable_id)?;
    let sim = &state.match_state.sim_runtime.as_ref()?.simulation;
    crate::sim::production::can_repair_building(sim, state.rules()?, id).then_some(id)
}

/// The local player's own building under the given world point when the sell
/// cursor takes it: `DisplayClass::DetermineAction 0x006929F2` asks the
/// building's `CanSell` (`production::can_sell_building`).
pub(crate) fn own_sellable_building_under_point(
    state: &AppState,
    world_x: f32,
    world_y: f32,
) -> Option<u64> {
    own_sellable_building_id(state, visible_object_under_point(state, world_x, world_y)?)
}

fn own_sellable_building_id(state: &AppState, stable_id: u64) -> Option<u64> {
    let id = own_building_id(state, stable_id)?;
    let sim = &state.match_state.sim_runtime.as_ref()?.simulation;
    crate::sim::production::can_sell_building(sim, state.rules()?, id).then_some(id)
}

pub(crate) fn sell_wall_under_cursor_is_eligible(state: &AppState) -> bool {
    let (world_x, world_y) = crate::app::match_runtime::sim_tick::screen_point_to_world(
        state,
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    if visible_object_under_point(state, world_x, world_y).is_some() {
        return false;
    }
    let (rx, ry) = crate::app::match_runtime::sim_tick::screen_point_to_world_cell(
        state,
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    let Some(owner) = preferred_local_owner(state) else {
        return false;
    };
    match (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        state.overlay_registry(),
    ) {
        (Some(sim), Some(overlays)) => sell_wall_command_for_cell(
            sim,
            overlays,
            &owner,
            rx,
            ry,
            state.match_state.sandbox_full_visibility,
            false,
        )
        .is_some(),
        _ => false,
    }
}

fn sell_wall_command_for_cell(
    sim: &crate::sim::world::Simulation,
    overlays: &crate::rules::overlay_types::OverlayTypeRegistry,
    local_owner: &str,
    rx: u16,
    ry: u16,
    ignore_visibility: bool,
    object_under_cursor: bool,
) -> Option<Command> {
    if object_under_cursor || (rx, ry) == (0, 0) {
        return None;
    }
    let local_owner_id = sim.interner().get(local_owner)?;
    if crate::app::presentation::instances::cell_visibility_for_local_owner(
        Some(local_owner_id),
        Some(sim.fog()),
        rx,
        ry,
        ignore_visibility,
        sim.resolved_terrain(),
    ) != crate::app::presentation::instances::CellVisibilityState::Visible
    {
        return None;
    }
    let grid = sim.overlay_grid()?;
    if rx >= grid.width() || ry >= grid.height() {
        return None;
    }
    let cell = grid.cell(rx, ry);
    let overlay_id = cell.overlay_id?;
    if !overlays.flags(overlay_id).is_some_and(|flags| flags.wall) {
        return None;
    }
    let wall_owner = cell.wall_owner?;
    let wall_owner_house = sim.houses().get(&wall_owner)?;
    let owner_is_human_player = if sim.session().game_mode_nonzero() {
        wall_owner == local_owner_id
    } else {
        wall_owner_house.is_human() || wall_owner_house.player_control()
    };
    owner_is_human_player.then_some(Command::SellWallAtCell {
        x: rx as i16,
        y: ry as i16,
    })
}

/// Handle a tactical left-click while the sidebar Repair or Sell cursor mode is
/// active. Issues `ToggleRepair` / `SellBuilding` on the own building under the
/// cursor. The mode stays active afterwards — gamemd repair/sell modes are
/// sticky: the player keeps repairing/selling until right-click, Esc, or a
/// second click on the sidebar button clears the mode. Returns `true` when a
/// repair/sell mode was active (the click is consumed by the mode regardless of
/// whether it landed on a building), `false` when neither mode is on.
pub(crate) fn try_repair_sell_mode_click(state: &mut AppState) -> bool {
    let repair = state
        .match_state
        .match_presentation
        .sidebar_gadget_state
        .repair_mode_on;
    let sell = state
        .match_state
        .match_presentation
        .sidebar_gadget_state
        .sell_mode_on;
    if !repair && !sell {
        return false;
    }
    let (world_x, world_y) = crate::app::match_runtime::sim_tick::screen_point_to_world(
        state,
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    let object_under_cursor = visible_object_under_point(state, world_x, world_y);
    let building = object_under_cursor.and_then(|id| {
        if repair {
            own_repairable_building_id(state, id)
        } else {
            own_sellable_building_id(state, id)
        }
    });
    if let Some(entity_id) = building {
        let owner: String =
            preferred_local_owner(state).unwrap_or_else(|| DEFAULT_OWNER.to_string());
        let payload = if repair {
            Command::ToggleRepair { entity_id }
        } else {
            Command::SellBuilding { entity_id }
        };
        schedule_command(state, &owner, payload);
    } else if sell && object_under_cursor.is_none() {
        let (rx, ry) = crate::app::match_runtime::sim_tick::screen_point_to_world_cell(
            state,
            state.match_state.input.cursor_x,
            state.match_state.input.cursor_y,
        );
        let Some(owner) = preferred_local_owner(state) else {
            return true;
        };
        let payload = match (
            state
                .match_state
                .sim_runtime
                .as_ref()
                .map(|rt| &rt.simulation),
            state.overlay_registry(),
        ) {
            (Some(sim), Some(overlays)) => sell_wall_command_for_cell(
                sim,
                overlays,
                &owner,
                rx,
                ry,
                state.match_state.sandbox_full_visibility,
                false,
            ),
            _ => None,
        };
        if let Some(payload) = payload {
            schedule_command(state, &owner, payload);
        }
    }
    true
}

/// BandBox_LeftUp's placement branch (`0x004ABA16..0x004ABC67`) for the
/// clicked `cursor_cell`: the tactical click's Cell, or the radar release's
/// (`0x00653D58`).
pub(crate) fn place_ready_building_at_cell(
    state: &mut AppState,
    type_id: &str,
    cursor_cell: (u16, u16),
) {
    // The original producer refuses a null current product and uses the
    // issuer captured by ManualPlace, rather than choosing a House again.
    let Some(issuer) = state.match_state.input.building_placement_issuer() else {
        return;
    };
    let Some(owner) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|runtime| runtime.simulation.interner().resolve(issuer).to_string())
    else {
        return;
    };
    // LeftUp4ABA16..4ABA59 uses its supplied clicked Cell and recomputes
    // proximity, rather than latching the previous draw's coordinate/result.
    let (origin, in_build_area) = state
        .match_state
        .sim_runtime
        .as_ref()
        .and_then(|runtime| {
            let sim = &runtime.simulation;
            let rules = &runtime.resources.rules;
            rules.object(type_id).map(|object| {
                // LeftUp4ABA16 adds the same type-derived offset as preview,
                // using this fresh clicked Cell rather than the last draw.
                let origin = crate::rules::foundation::placement_origin_at_cursor(
                    &object.foundation,
                    cursor_cell,
                );
                let in_build_area = crate::sim::production::is_within_build_area(
                    sim, rules, &owner, object, origin.0, origin.1,
                );
                (origin, in_build_area)
            })
        })
        .unwrap_or((cursor_cell, false));
    let (rx, ry) = origin;
    // Visibility1181 is latched by UpdateFoundationCenter4A94DA. LeftUp
    // checks it after the freshly recomputed proximity1180(4ABAA4/4ABAB2).
    // RESIDUAL: gamemd latches it from the tactical gadget only
    // (`0x004AAC10` calls `0x004A91B0` at `0x004AAD04`); this preview follows
    // the cursor's screen point over the sidebar too. Trigger: a radar
    // placement (`0x00653D58`) in a shrouded game. Effect: it tests the Cell
    // behind the radar rather than the last tactical one, so it may refuse
    // or admit differently. Frequency: rare.
    let visible = state
        .match_state
        .input
        .building_placement_preview
        .as_ref()
        .is_none_or(|preview| preview.foundation_visible);
    if !in_build_area || !visible {
        if let Some(reason) = state
            .match_state
            .input
            .building_placement_preview
            .as_ref()
            .and_then(|preview| preview.reason.as_ref())
        {
            log::warn!(
                "Ready building placement rejected locally: owner={} type={} cell=({}, {}) reason={}",
                owner,
                type_id,
                rx,
                ry,
                reason.label()
            );
        }
        // LeftUp4ABAA4/4ABAB2 checks independent proximity/visibility
        // bytes1180/1181, not the Cell draw's IsClearToBuild result. Failure
        // reaches4ABC76 local CannotDeployHere before queuing. Near red terrain
        // still emits PLACE; House4FB0E0/Unlimbo refuses it and restores the
        // saved ready product through the existing input lifecycle owner.
        // Native: cameo_order local_invalid_site_retains_then_valid_place;
        // placement_render documents the Cell draw/admission boundary.
        crate::app::input::dispatch::push_local_eva(
            state,
            crate::app::input::sidebar_eva::EVA_CANNOT_DEPLOY_HERE,
        );
        return;
    }
    let type_interned = intern_in_sim(state, type_id);
    schedule_command(
        state,
        &owner,
        Command::PlaceReadyBuilding {
            type_id: type_interned,
            rx,
            ry,
        },
    );
    // Clear placement mode immediately so the foundation preview stops following
    // the cursor after the order is issued.
    state.match_state.input.submit_building_placement();
    log::info!(
        "Ready building placement queued: owner={} type={} cell=({}, {}) execute_tick=current",
        owner,
        type_id,
        rx,
        ry
    );
}

/// Schedule `Command::LaunchSuperWeapon` at the current cursor cell.
///
/// `section` is the SW INI section name (e.g., "LightningStormSpecial").
///
/// Returns early when the cursor is over the sidebar or minimap — this
/// matters for the release of the arming click itself, which lands on the
/// cameo. On a real tactical-map click it schedules the command, as
/// `DisplayClass::BandBox_LeftUp @ 0x004AB9B0` queues the SPECIAL_PLACE event
/// (`0x12`), and keeps the selection: Launch's tail clears or moves it
/// (`match_runtime::super_selection`). Pointer action admission runs first;
/// the sim's ClickFire still owns instance admission. The event names the first
/// type whose `Action=` is the click's action (`SuperWeaponTypeClass::FindFirstOfAction`); retail
/// types' actions are unique, so that is the selected one.
pub(crate) fn launch_super_weapon_at_cursor(state: &mut AppState, section: &str) {
    // Guard: arming click's RELEASE lands on the cameo. Don't fire the SW
    // at a bogus off-map cell behind the sidebar panel; leave the mode
    // armed so the next real map click fires.
    if crate::app::presentation::sidebar_render::is_cursor_over_minimap(state)
        || crate::app::input::cursor::current_sidebar_view_hit(state)
    {
        return;
    }

    // DetermineAction692B1D -> Type6CEF80 returns NoForceShield70 for an
    // invalid pointer target. FindFirstOfAction6CEEB0 then finds no retail
    // type, so BandBox_LeftUp4AC20C queues no SPECIAL_PLACE and keeps the
    // selected Super. Do not move this UI-only gate into the launch receiver.
    if !super_weapon_pointer_target_at_cursor_is_valid(state, section) {
        return;
    }

    let cell = crate::app::match_runtime::sim_tick::screen_point_to_world_cell(
        state,
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    launch_super_weapon_at_cell(state, section, cell);
}

/// BandBox_LeftUp's SPECIAL_PLACE (`0x004AC20C`) for `section` at the clicked
/// `(rx, ry)`: the tactical click's Cell, or the radar release's
/// (`0x00653D58`), whose action names no pointer target to admit.
pub(crate) fn launch_super_weapon_at_cell(
    state: &mut AppState,
    section: &str,
    (rx, ry): (u16, u16),
) {
    let owner: String = resolve_owner(state);
    let sw_type_id = intern_in_sim(state, section);
    queue_super_weapon_click(state, &owner, sw_type_id, (rx, ry));
    log::info!(
        "SuperWeapon launch queued: owner={} section={} cell=({}, {}) issue_frame=current",
        owner,
        section,
        rx,
        ry
    );
}

/// Paid source/final clicks use explicit extension commands. Stock commands
/// and native capture payloads are unchanged.
pub(crate) fn queue_super_weapon_click(
    state: &mut AppState,
    owner: &str,
    sw_type_id: InternedId,
    cell: (u16, u16),
) {
    let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
        return;
    };
    let sim = &runtime.simulation;
    let Some(owner_id) = sim.interner().get(owner) else {
        return;
    };
    let Some(command) =
        super_weapon_click_command(sim, &runtime.resources.rules, owner_id, sw_type_id, cell)
    else {
        return;
    };
    try_schedule_command(state, owner, command);
}

fn super_weapon_click_command(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    owner_id: InternedId,
    sw_type_id: InternedId,
    cell: (u16, u16),
) -> Option<Command> {
    let ticket = sim
        .paid_super_weapon_selection(owner_id)
        .filter(|ticket| ticket.destination == sw_type_id);
    let command = if let Some(selection) = ticket {
        Command::CompletePaidSuperWeapon {
            selection,
            target_rx: cell.0,
            target_ry: cell.1,
        }
    } else if let Some(mut selection) =
        sim.next_paid_super_weapon_selection(rules, owner_id, sw_type_id)
    {
        if let Some(queued) = sim.pending_paid_super_weapon_sequence(owner_id, sw_type_id) {
            let next = queued.checked_add(1)?;
            selection.sequence = selection.sequence.max(next);
        }
        Command::BeginPaidSuperWeapon {
            selection,
            target_rx: cell.0,
            target_ry: cell.1,
        }
    } else {
        Command::LaunchSuperWeapon {
            sw_type_id,
            target_rx: cell.0,
            target_ry: cell.1,
        }
    };
    Some(command)
}

/// Every local cursor cancellation/reselection submits a ticketed command,
/// never a direct Simulation write. Called after input and before the next
/// ordinary frame so building placement and campaign mode clears participate.
pub(crate) fn reconcile_paid_super_weapon_cursor(state: &mut AppState) {
    let owner = resolve_owner(state);
    let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
        return;
    };
    let sim = &runtime.simulation;
    let Some(owner_id) = sim.interner().get(&owner) else {
        return;
    };
    let selected = state
        .match_state
        .input
        .targeting_mode
        .as_ref()
        .and_then(crate::app::types::TargetingMode::as_super_weapon);
    if let Some(command) = paid_super_weapon_cursor_cancellation(sim, owner_id, selected) {
        try_schedule_command(state, &owner, command);
    }
}

fn paid_super_weapon_cursor_cancellation(
    sim: &crate::sim::world::Simulation,
    owner: InternedId,
    selected: Option<&str>,
) -> Option<Command> {
    let (selection, in_queue) = sim.projected_paid_super_weapon_selection(owner)?;
    let expected = if in_queue {
        selection.source
    } else {
        selection.destination
    };
    (!selected.is_some_and(|name| name.eq_ignore_ascii_case(sim.interner().resolve(expected))))
        .then_some(Command::CancelPaidSuperWeapon { selection })
}

/// Dev "Base" button: start the next opening building (power plant, barracks,
/// refinery) the owner has none of on the map and the strip offers. A
/// building never queues (`production::enqueue_by_type`), so each press
/// starts one while the building factory holds nothing; place it, then press
/// again.
pub(crate) fn place_starter_base_for_local_owner(state: &mut AppState) {
    let owner: String = resolve_owner(state);
    let (Some(sim), Some(rules)) = (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        state.rules(),
    ) else {
        return;
    };
    let opening = [
        pick_building_for_owner(rules, &owner, &["GAPOWR", "NAPOWR", "YAPOWR"]),
        pick_building_for_owner(
            rules,
            &owner,
            &["GAPILE", "NAHAND", "YABRCK", "NABRCK", "GABARR"],
        ),
        pick_building_for_owner(rules, &owner, &["GAREFN", "NAREFN", "YAREFN", "GAOREP"]),
    ];
    let owner_id = sim.interner().get(&owner);
    let build_options = production::build_options_for_owner(sim, rules, &owner);
    let next = opening.into_iter().flatten().find(|type_id| {
        let on_map = sim.interner().get(type_id).is_some_and(|type_ref| {
            sim.entities().values().any(|entity| {
                Some(entity.owner()) == owner_id
                    && entity.type_ref() == type_ref
                    && !entity.in_limbo()
            })
        });
        !on_map
            && build_options.iter().any(|opt| {
                opt.enabled
                    && sim
                        .interner()
                        .resolve(opt.type_id)
                        .eq_ignore_ascii_case(type_id)
            })
    });
    let Some(type_id) = next else {
        log::warn!("Starter opening: owner={owner} has no opening building left to start");
        return;
    };
    let Some(category) = rules.object(&type_id).map(production::category_for_object) else {
        return;
    };
    if owner_id.is_some_and(|owner_id| {
        sim.production()
            .factories()
            .view(owner_id, category)
            .is_some_and(|factory| factory.object.is_some())
    }) {
        log::warn!(
            "Starter opening: owner={owner} must place or cancel its current building first"
        );
        return;
    }
    let type_interned = intern_in_sim(state, &type_id);
    schedule_command(
        state,
        &owner,
        Command::QueueProduction {
            type_id: type_interned,
        },
    );
    log::info!("Starter opening started: owner={owner} type={type_id}");
}

/// Queue one attack-move per object for a dev stress army. Every order goes
/// through the ordinary local gate, so the stress march obeys the same input
/// lock, owner interning and megamission byte round-trip as a player's click.
pub(crate) fn schedule_stress_attack_moves(
    sim: &mut crate::sim::world::Simulation,
    owner: &str,
    entity_ids: &[u64],
    (rx, ry): (u16, u16),
) -> usize {
    entity_ids
        .iter()
        .filter(|&&entity_id| {
            schedule_command_in_sim(
                sim,
                owner,
                Command::AttackMove {
                    entity_id,
                    target: None,
                    destination: Some(crate::sim::combat::TargetKind::Cell(rx, ry)),
                },
            )
            .is_some()
        })
        .count()
}

pub(crate) fn spawn_test_units_for_local_owner(state: &mut AppState) {
    let owner: String = resolve_owner(state);
    let sw: f32 = state.render_width() as f32;
    let sh: f32 = state.render_height() as f32;
    let (mut base_rx, mut base_ry) =
        crate::app::match_runtime::sim_tick::screen_point_to_world_cell(state, sw * 0.5, sh * 0.5);
    let path_grid = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
        .and_then(crate::sim::world::Simulation::path_grid_snapshot);
    let Some(rt) = state.match_state.sim_runtime.as_mut() else {
        return;
    };
    let resources = &rt.resources;
    let sim = &mut rt.simulation;
    let rules = &resources.rules;
    if let Some(grid) = path_grid.as_deref() {
        (base_rx, base_ry) =
            crate::app::match_runtime::sim_tick::clamp_cell_to_grid(grid, (base_rx, base_ry));
    }

    let mut debug_types: Vec<String> = {
        let options = production::build_options_for_owner(sim, rules, &owner);
        let mut selected: Vec<String> = options
            .iter()
            .filter(|o| {
                o.enabled
                    && o.object_category == crate::rules::object_type::ObjectCategory::Infantry
            })
            .take(3)
            .map(|o| sim.interner().resolve(o.type_id).to_string())
            .collect();
        if selected.len() < 3 {
            let vehicles = options
                .iter()
                .filter(|o| {
                    o.enabled
                        && o.object_category == crate::rules::object_type::ObjectCategory::Vehicle
                })
                .take(3 - selected.len())
                .map(|o| sim.interner().resolve(o.type_id).to_string());
            selected.extend(vehicles);
        }
        selected
    };
    if debug_types.is_empty() {
        debug_types = vec!["HTNK".to_string(), "MTNK".to_string(), "E1".to_string()];
    }

    let mut spawned: u32 = 0;
    let mut first_spawn: Option<(u16, u16)> = None;
    for (i, type_id) in debug_types.iter().enumerate() {
        let mut desired = (
            base_rx.saturating_add(2 + i as u16 * 2),
            base_ry.saturating_add(2),
        );
        if let Some(grid) = path_grid.as_deref() {
            desired = crate::app::match_runtime::sim_tick::clamp_cell_to_grid(grid, desired);
        }
        let spawn_cell = path_grid
            .as_deref()
            .and_then(|g| {
                crate::app::match_runtime::sim_tick::nearest_walkable_cell(g, desired, 16)
            })
            .unwrap_or(desired);
        if sim
            .spawn_object_with_overlay_registry(
                type_id,
                &owner,
                spawn_cell.0,
                spawn_cell.1,
                64,
                rules,
                &resources.overlay_registry,
            )
            .is_some()
        {
            if first_spawn.is_none() {
                first_spawn = Some(spawn_cell);
            }
            let name = rules
                .object(type_id)
                .and_then(|o| o.name.clone())
                .unwrap_or_else(|| type_id.clone());
            log::info!("Spawn test unit: {} ({})", name, type_id);
            spawned += 1;
        }
    }
    if spawned > 0 {
        crate::app::match_runtime::sim_tick::refresh_entity_atlases(state);
        if let Some((rx, ry)) = first_spawn {
            crate::app::input::camera::center_camera_on_cell(state, rx, ry);
        }
    }
    log::info!(
        "Spawn test units: owner={} spawned={} at ({},{}) types={:?}",
        owner,
        spawned,
        base_rx,
        base_ry,
        debug_types
    );
}

pub(crate) fn cycle_local_owner(state: &mut AppState) {
    // Debug cycling is available only without a scenario current House.
    // It cannot repoint ordinary saved identity through a presentation choice.
    if state.match_state.local_player_owner().is_some() {
        log::info!("Cycle owner ignored: local player is pinned for this match");
        return;
    }
    let mut owners = collect_playable_owners(state);
    if owners.is_empty() {
        return;
    }
    let current = preferred_local_owner(state);
    let next_idx = current
        .as_ref()
        .and_then(|c| owners.iter().position(|o| o.eq_ignore_ascii_case(c)))
        .map(|idx| (idx + 1) % owners.len())
        .unwrap_or(0);
    // Move out of Vec instead of cloning, then clone once for the override.
    let next = owners.swap_remove(next_idx);
    state.match_state.local_owner_override = Some(next.clone());
    state.match_state.input.targeting_mode = None;
    log::info!("Local owner switched to {}", next);
}

pub(crate) fn preferred_local_owner_name(state: &AppState) -> Option<String> {
    preferred_local_owner(state)
}

pub(crate) fn preferred_local_owner(state: &AppState) -> Option<String> {
    let sim = &state.match_state.sim_runtime.as_ref()?.simulation;
    preferred_local_owner_for_sim(
        sim,
        state.rules(),
        &state.match_state.match_presentation.house_roster,
        state.match_state.local_owner_override.as_deref(),
    )
}

fn preferred_local_owner_for_sim(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    roster: &crate::map::houses::HouseRoster,
    sandbox_override: Option<&str>,
) -> Option<String> {
    // Normal command and presentation consumers share saved native identity.
    // App debug preferences and selection apply only to identity-less sandboxes.
    if let Some(owner) = crate::app::match_runtime::state::scenario_local_owner(sim) {
        return Some(owner.to_string());
    }
    // Sandbox fallback: prefer owner of selected unit first.
    for entity in sim.entities().values() {
        let owner_str = sim.interner().resolve(entity.owner());
        if entity.selection.is_selected() && is_playable_house_name(owner_str) {
            return Some(owner_str.to_string());
        }
    }

    // Then explicit local override set by debug actions.
    if let Some(owner) = sandbox_override
        && is_playable_house_name(owner)
    {
        return Some(owner.to_string());
    }

    // Prefer owners that currently have structures.
    let mut structure_counts: HashMap<String, usize> = HashMap::new();
    for entity in sim.entities().values() {
        let owner_str = sim.interner().resolve(entity.owner());
        if entity.category() == EntityCategory::Structure && is_playable_house_name(owner_str) {
            *structure_counts.entry(owner_str.to_string()).or_insert(0) += 1;
        }
    }
    if !structure_counts.is_empty() {
        let mut ranked: Vec<(usize, String)> = structure_counts
            .into_iter()
            .filter_map(|(owner, count)| {
                let strict_buildable = rules.is_some_and(|rules| {
                    production::has_build_option_for_owner(sim, rules, &owner)
                });
                strict_buildable.then_some((count, owner))
            })
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        if let Some((_, owner)) = ranked.first() {
            return Some(owner.to_string());
        }
    }

    // Next fallback: playable houses from map config.
    let houses = collect_playable_owners_from(roster, Some(sim));
    if let Some(owner) = houses.first() {
        return Some(owner.to_string());
    }

    // Last fallback: any playable owner present in entity store.
    let mut owners: Vec<String> = sim
        .entities()
        .values()
        .map(|e| sim.interner().resolve(e.owner()).to_string())
        .filter(|o| is_playable_house_name(o))
        .collect();
    owners.sort();
    owners.dedup();
    owners.first().cloned()
}

pub(crate) fn collect_playable_owners(state: &AppState) -> Vec<String> {
    collect_playable_owners_from(
        &state.match_state.match_presentation.house_roster,
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
    )
}

fn collect_playable_owners_from(
    roster: &crate::map::houses::HouseRoster,
    simulation: Option<&crate::sim::world::Simulation>,
) -> Vec<String> {
    let mut owners: Vec<String> = roster
        .houses
        .iter()
        .filter(|house| is_playable_house_name(&house.name))
        .filter(|house| house.player_control != Some(false))
        .map(|house| house.name.clone())
        .collect();
    if let Some(sim) = simulation {
        for entity in sim.entities().values() {
            let owner_str = sim.interner().resolve(entity.owner());
            if is_playable_house_name(owner_str) {
                owners.push(owner_str.to_string());
            }
        }
    }
    owners.sort();
    owners.dedup();
    owners
}

fn pick_building_for_owner(
    rules: &crate::rules::ruleset::RuleSet,
    owner: &str,
    candidates: &[&str],
) -> Option<String> {
    for id in candidates {
        let Some(obj) = rules.object(id) else {
            continue;
        };
        if obj.category != crate::rules::object_type::ObjectCategory::Building {
            continue;
        }
        if !obj.owner.is_empty() && !obj.owner.iter().any(|o| o.eq_ignore_ascii_case(owner)) {
            continue;
        }
        return Some((*id).to_string());
    }
    for id in candidates {
        let Some(obj) = rules.object(id) else {
            continue;
        };
        if obj.category == crate::rules::object_type::ObjectCategory::Building {
            return Some((*id).to_string());
        }
    }
    None
}

pub(crate) fn schedule_command_in_sim(
    sim: &mut crate::sim::world::Simulation,
    owner: &str,
    payload: Command,
) -> Option<u64> {
    // The ordinary client producer is behind the native54F200/6930A0 lock
    // gates. Escape's modal remains usable: its speed/exit records have
    // their existing authoritative owners and are not tactical orders.
    if sim.session().input_locked()
        && !matches!(
            &payload,
            Command::SetGameSpeed { .. }
                | Command::LiveGameSpeed { .. }
                | Command::SetLatencyFudge { .. }
                | Command::ExitMatch
                | Command::Destruct
                | Command::RemovePlayer { .. }
                | Command::Ally { .. }
        )
    {
        return None;
    }
    let execute_tick = sim.session().tick();
    let owner_id = match &payload {
        Command::SetGameSpeed { speed } => {
            if *speed > crate::sim::game_options::IN_GAME_OPTIONS_MAX_SPEED {
                return None;
            }
            let owner_id = sim.interner().get(owner)?;
            if !sim.houses().contains_key(&owner_id) {
                return None;
            }
            let requested_speed = sim
                .projected_in_game_options_speed()
                .map(i32::from)
                .unwrap_or(sim.session().game_options().game_speed);
            if requested_speed == i32::from(*speed) {
                return Some(execute_tick);
            }
            owner_id
        }
        _ => sim.intern(owner),
    };
    let envelope = match payload {
        Command::SetLatencyFudge { value } => {
            let record = sim.encode_latency_fudge_record(owner_id, value)?;
            CommandEnvelope::synchronized(record, execute_tick)
        }
        Command::Scatter { entity_id } => {
            let record = sim.encode_scatter_record(owner_id, entity_id)?;
            CommandEnvelope::synchronized(record, execute_tick)
        }
        Command::Cheer => {
            let record = sim.encode_cheer_record(owner_id)?;
            CommandEnvelope::synchronized(record, execute_tick)
        }
        Command::ExitMatch => {
            let record = sim.encode_exit_record(owner_id)?;
            CommandEnvelope::synchronized(record, execute_tick)
        }
        Command::Destruct => {
            let record = sim.encode_destruct_record(owner_id)?;
            CommandEnvelope::synchronized(record, execute_tick)
        }
        Command::RemovePlayer { house_index } => {
            let record = sim.encode_remove_player_record(owner_id, house_index)?;
            CommandEnvelope::synchronized(record, execute_tick)
        }
        Command::Ally { house_index } => {
            let record = sim.encode_ally_record(owner_id, house_index)?;
            CommandEnvelope::synchronized(record, execute_tick)
        }
        Command::SellWallAtCell { x, y } => {
            let record = sim.encode_sell_wall_at_cell_record(owner_id, x, y)?;
            CommandEnvelope::synchronized(record, execute_tick)
        }
        payload => CommandEnvelope::new(owner_id, execute_tick, payload),
    };
    let envelope = encode_ordinary_local_command(sim, envelope)?;
    // PlayerSend6FFDE6 ignores a full OutList's false return. The click is
    // still handled and its voice/action-line effects are not rolled back.
    let _ = sim.issue_local_command(envelope);
    Some(execute_tick)
}

/// Move-click producer for a click on a Cell. Each receiver resolves its own
/// Cell ([`Simulation::cell_click_destination`]) before event encoding;
/// synchronized/replay/internal Move payloads are already resolved. Original
/// initialized GI and MTNK Shift0x10 still execute that resolver and emit
/// ordinary MegaMission (111-byte Event)/Move2 -> class(Cell,1); neither
/// appends a queued waypoint (Infantry51AA40, Unit741970). Native
/// input/event/true-RET controls: walk_first_path.json `gi_reissue`.
///
/// [`Simulation::cell_click_destination`]: crate::sim::world::Simulation::cell_click_destination
pub(crate) fn ordinary_cell_move_goal(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    viewer: InternedId,
    entity_id: u64,
    clicked: (u16, u16),
    shift: bool,
) -> Option<(u16, u16)> {
    sim.cell_click_destination(viewer, entity_id, clicked, shift, rules, registry)
        .unwrap_or_else(|cause| {
            log::warn!("Cell click input: {cause}");
            None
        })
}

/// Move-click producer for a click over an object: the object, or a Cell
/// near it ([`Simulation::object_move_click`]).
///
/// [`Simulation::object_move_click`]: crate::sim::world::Simulation::object_move_click
pub(crate) fn object_move_destination(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    viewer: InternedId,
    entity_id: u64,
    target_id: u64,
    action: crate::sim::world::MoveAction,
    force_move_key: bool,
) -> Option<crate::sim::combat::TargetKind> {
    sim.object_move_click(viewer, entity_id, target_id, action, force_move_key, rules)
        .unwrap_or_else(|cause| {
            log::warn!("Move click over an object: {cause}");
            None
        })
}

/// Make ordinary local Move and AreaGuard bytes authoritative at issue time.
///
/// Active YR routes a cell click through `ClickedAction` (`0x004D7D50`), and a
/// Move over an object through `0x004D74E0`, to
/// `EventClass__BuildMegaMissionEnvelope` (`0x004C6860`). Guard key730D60
/// reaches the same producer through QueueMegaMission6FFBE0. Move, to a Cell
/// or an object, and AreaGuard share this codec; other semantic commands pass
/// through until their native record is established.
pub(crate) fn encode_ordinary_local_command(
    sim: &crate::sim::world::Simulation,
    envelope: CommandEnvelope,
) -> Option<CommandEnvelope> {
    let CommandEnvelope {
        owner,
        execute_tick,
        payload,
    } = envelope;
    match payload {
        Command::Move { .. } | Command::MoveToObject { .. } | Command::Guard { .. } => {
            let record = sim.encode_megamission_record(owner, &payload)?;
            Some(CommandEnvelope::synchronized(record, execute_tick))
        }
        Command::Stop { entity_id } => {
            let record = sim.encode_stop_record(owner, entity_id)?;
            Some(CommandEnvelope::synchronized(record, execute_tick))
        }
        payload => Some(CommandEnvelope::new(owner, execute_tick, payload)),
    }
}

/// Component tests can inspect the executable view without running a frame;
/// production retains the encoded record until the DoList admits it.
#[cfg(test)]
pub(crate) fn roundtrip_ordinary_local_megamission(
    sim: &crate::sim::world::Simulation,
    envelope: CommandEnvelope,
) -> Option<CommandEnvelope> {
    let command = encode_ordinary_local_command(sim, envelope)?;
    if let Command::SynchronizedRecord { record } = &command.payload {
        sim.decode_native_command_record(record, command.execute_tick)
    } else {
        Some(command)
    }
}

/// Queue one ordinary deterministic command and return its actual execute tick.
///
/// Tactical certification records this raw issue ordinal. Offline input does
/// not pre-delay the envelope; the next ordinary command drain admits it.
/// Network transfer overwrites the stamp with its negotiated ahead frame.
pub(crate) fn try_schedule_command(
    state: &mut AppState,
    owner: &str,
    payload: Command,
) -> Option<u64> {
    state
        .match_state
        .sim_runtime
        .as_mut()
        .map(|rt| &mut rt.simulation)
        .and_then(|sim| schedule_command_in_sim(sim, owner, payload))
}

/// Preserve the existing unit-returning command boundary for ordinary callers.
pub(crate) fn schedule_command(state: &mut AppState, owner: &str, payload: Command) {
    let _ = try_schedule_command(state, owner, payload);
}

/// AllToCheer536C00 ->730F30. The input owner retains only its admission
/// frame; the ordinary scheduler and native Event2D codec own the command.
pub(crate) fn queue_house_cheer(state: &mut AppState) {
    let Some(owner) = preferred_local_owner_name(state) else {
        return;
    };
    let Some(runtime) = state.match_state.sim_runtime.as_mut() else {
        return;
    };
    let _ = queue_house_cheer_in_sim(
        &mut runtime.simulation,
        &runtime.resources.rules,
        &mut state.match_state.input,
        &owner,
    );
}

pub(super) fn queue_house_cheer_in_sim(
    sim: &mut crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    input: &mut crate::app::input::state::MatchInputState,
    owner: &str,
) -> Option<u64> {
    // Preserve the ordinary keyboard/bar Scenario+35A2 lock before entering
    // the producer. Original730F30's own guard is the distinct Session+300
    // defeat flag (active gamemd; tools/input_oracle/house_cheer.md).
    if sim.session().input_locked() {
        return None;
    }
    let current_frame = sim.session().binary_frame() as i32;
    let local_defeated = sim.local_player_defeated();
    input.try_issue_cheer(
        current_frame,
        rules.general.maximum_cheer_rate(),
        local_defeated,
        || schedule_command_in_sim(sim, owner, Command::Cheer),
    )
}

pub(crate) fn is_playable_house_name(name: &str) -> bool {
    let up = name.to_ascii_uppercase();
    !matches!(
        up.as_str(),
        "NEUTRAL" | "SPECIAL" | "CIVILIAN" | "GOODGUY" | "BADGUY" | "JP"
    )
}

#[cfg(test)]
mod tests {
    use super::{
        schedule_command_in_sim, schedule_stress_attack_moves, sell_wall_command_for_cell,
    };

    use crate::map::entities::EntityCategory;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::combat::{Health, TargetKind};
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::game_entity::GameEntity;
    use crate::sim::house_state::HouseState;
    use crate::sim::intern::InternedId;
    use crate::sim::world::Simulation;

    #[test]
    fn stress_march_queues_one_attack_move_per_object_through_the_local_gate() {
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        sim.houses_mut_for_test()
            .register(HouseState::new(local, 0, None, true, 0, 10));

        let accepted = schedule_stress_attack_moves(&mut sim, "Local", &[11, 12, 13], (40, 60));
        assert_eq!(
            accepted, 3,
            "every object of the stress army gets its own order"
        );
        let queued = sim.pending_command_snapshot();
        assert_eq!(queued.len(), 3);
        assert!(
            queued.iter().all(|command| matches!(
                command.payload,
                Command::AttackMove {
                    target: None,
                    destination: Some(TargetKind::Cell(40, 60)),
                    ..
                }
            )),
            "one click, one destination for the whole army: {queued:?}"
        );

        sim.session_mut_for_test().set_input_locked_for_test(true);
        assert_eq!(
            schedule_stress_attack_moves(&mut sim, "Local", &[14], (40, 60)),
            0,
            "the lock that rejects a player's click rejects the stress army too"
        );
        assert_eq!(sim.pending_command_snapshot().len(), 3);
    }

    #[test]
    fn cinematic_lock_rejects_tactical_commands_but_keeps_escape_options_usable() {
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        let kind = sim.intern("FixtureType");
        sim.houses_mut_for_test()
            .register(HouseState::new(local, 0, None, true, 0, 10));
        sim.session_mut_for_test().set_input_locked_for_test(true);
        let before_hash = sim.state_hash();
        let before_names = sim.interner().len();
        for payload in [
            Command::Stop { entity_id: 11 },
            Command::SellBuilding { entity_id: 12 },
            Command::ToggleRepair { entity_id: 13 },
            Command::PlaceReadyBuilding {
                type_id: kind,
                rx: 20,
                ry: 21,
            },
            Command::LaunchSuperWeapon {
                sw_type_id: kind,
                target_rx: 22,
                target_ry: 23,
            },
        ] {
            assert_eq!(schedule_command_in_sim(&mut sim, "Unknown", payload), None);
        }
        assert!(sim.pending_command_snapshot().is_empty());
        assert_eq!(sim.interner().len(), before_names);
        assert_eq!(sim.state_hash(), before_hash);

        assert_eq!(
            schedule_command_in_sim(&mut sim, "Local", Command::SetGameSpeed { speed: 4 }),
            Some(0)
        );
        assert_eq!(
            schedule_command_in_sim(&mut sim, "Local", Command::ExitMatch),
            Some(0)
        );
        assert_eq!(sim.pending_command_snapshot().len(), 2);
        sim.session_mut_for_test().set_input_locked_for_test(false);
        assert_eq!(
            schedule_command_in_sim(&mut sim, "Local", Command::Stop { entity_id: 11 }),
            Some(0)
        );
        assert_eq!(sim.pending_command_snapshot().len(), 3);
    }

    #[test]
    fn options_game_speed_queues_once_without_immediate_sim_mutation() {
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        sim.houses_mut_for_test()
            .register(HouseState::new(local, 0, None, true, 0, 10));
        let before_hash = sim.state_hash();

        assert_eq!(
            schedule_command_in_sim(&mut sim, "Local", Command::SetGameSpeed { speed: 4 }),
            Some(0)
        );
        assert_eq!(sim.session().game_options().game_speed, 1);
        assert_eq!(sim.state_hash(), before_hash);
        assert_eq!(sim.pending_command_snapshot().len(), 1);

        assert_eq!(
            schedule_command_in_sim(&mut sim, "Local", Command::SetGameSpeed { speed: 4 }),
            Some(0),
            "reopening Options before admission accepts the existing request"
        );
        assert_eq!(
            sim.pending_command_snapshot().len(),
            1,
            "duplicate request is elided"
        );
        assert_eq!(
            schedule_command_in_sim(&mut sim, "Local", Command::SetGameSpeed { speed: 7 }),
            None,
            "the in-game trackbar cannot emit stored speed 7"
        );

        let interned_before_unknown = sim.interner().len();
        assert_eq!(
            schedule_command_in_sim(&mut sim, "Unknown", Command::SetGameSpeed { speed: 3 }),
            None
        );
        assert_eq!(sim.interner().len(), interned_before_unknown);
    }

    fn two_human_sim() -> (Simulation, InternedId, InternedId) {
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        let peer = sim.intern("Peer");
        sim.houses_mut_for_test()
            .register(HouseState::new(local, 0, None, true, 0, 10));
        sim.houses_mut_for_test()
            .register(HouseState::new(peer, 0, None, true, 0, 10));
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(local));
        sim.session_mut_for_test().set_tick_for_test(41);
        sim.session_mut_for_test().set_binary_frame_for_test(73);
        (sim, local, peer)
    }

    #[test]
    fn local_outlist_overflow_keeps_click_success_and_local_options() {
        let (mut sim, _, _) = two_human_sim();
        // TryAppend6521C0 admits 128 events globally, before any payload
        // receiver runs. A stale Stop still occupies its slot. PlayerSend
        // 6FFDE6..6FFDEE ignores a full OutList's refusal and returns true.
        for _ in 0..127 {
            assert_eq!(
                schedule_command_in_sim(&mut sim, "Local", Command::Stop { entity_id: 42 }),
                Some(41)
            );
        }
        assert_eq!(
            schedule_command_in_sim(&mut sim, "Local", Command::LiveGameSpeed { speed: 4 }),
            Some(41)
        );
        assert_eq!(
            schedule_command_in_sim(&mut sim, "Peer", Command::LiveGameSpeed { speed: 6 }),
            Some(41),
            "overflow does not turn ordinary input feedback into a refusal"
        );
        assert_eq!(
            schedule_command_in_sim(&mut sim, "Local", Command::SetGameSpeed { speed: 2 }),
            Some(41),
            "offline Options is outside EventClass OutList admission"
        );
        let due = sim.take_due_commands();
        assert_eq!(due.len(), 1, "only local Options uses pre-Logic ingress");
        // This second attempt is still before transfer; taking the ingress
        // batch cannot free the event queue's occupied slots.
        assert_eq!(
            schedule_command_in_sim(&mut sim, "Peer", Command::LiveGameSpeed { speed: 6 }),
            Some(41)
        );
        sim.advance_tick(&due, None, None, None, 33);
        assert_eq!(
            sim.session().game_options().game_speed,
            4,
            "local Options runs first, then slot 128; the peer's 129th event was dropped"
        );
        assert_eq!(
            schedule_command_in_sim(&mut sim, "Peer", Command::LiveGameSpeed { speed: 6 }),
            Some(42)
        );
        run_due(&mut sim);
        assert_eq!(
            sim.session().game_options().game_speed,
            6,
            "transfer reopens the queue"
        );
    }

    fn run_due(sim: &mut Simulation) -> bool {
        let due = sim.take_due_commands();
        sim.advance_tick(&due, None, None, None, 33).frame_committed
    }

    #[test]
    fn a_remote_exit_marks_the_house_without_quitting_or_killing_it() {
        // Execute_DoList 0x0064C7E7 marks House+0x248 for every sender; only
        // the local one reaches Execute's quit byte (0x004C7917).
        let (mut sim, local, peer) = two_human_sim();
        schedule_command_in_sim(&mut sim, "Peer", Command::ExitMatch).unwrap();
        assert!(run_due(&mut sim), "a remote EXIT does not end the frame");
        assert!(!sim.quit_requested());
        assert_eq!(sim.take_executed_exit_owner(), None);
        assert_eq!(sim.take_departed_owners(), [peer]);
        assert!(sim.houses()[&peer].left_game());
        // 0x005E2BA0: the local human is undefeated, so no Flag_To_Die.
        assert!(!sim.houses()[&peer].is_to_die());
        assert!(!sim.houses()[&local].left_game());

        // The local EXIT still quits.
        schedule_command_in_sim(&mut sim, "Local", Command::ExitMatch).unwrap();
        assert!(!run_due(&mut sim));
        assert!(sim.quit_requested());
        assert_eq!(sim.take_executed_exit_owner(), Some(local));
        assert!(sim.take_departed_owners().is_empty());
    }

    #[test]
    fn an_exit_with_no_other_undefeated_human_flags_the_sender_to_die() {
        let (mut sim, local, peer) = two_human_sim();
        sim.houses_mut_for_test()
            .get_mut(&local)
            .unwrap()
            .set_is_defeated_for_test(true);
        schedule_command_in_sim(&mut sim, "Peer", Command::ExitMatch).unwrap();
        run_due(&mut sim);
        assert!(sim.houses()[&peer].is_to_die());
    }

    #[test]
    fn remove_player_flags_only_a_human_house_to_die() {
        // EventClass::Execute 0x004C7B73: IsHumanControlled, then
        // Flag_To_Die; the payload is the House's registration index.
        let (mut sim, _, peer) = two_human_sim();
        let ai = sim.intern("Ai");
        sim.houses_mut_for_test()
            .register(HouseState::new(ai, 0, None, false, 0, 10));
        for index in [1, 2] {
            schedule_command_in_sim(
                &mut sim,
                "Local",
                Command::RemovePlayer { house_index: index },
            )
            .unwrap();
        }
        let record = sim
            .encode_remove_player_record(sim.interner().get("Local").unwrap(), 1)
            .unwrap();
        assert_eq!(record.opcode(), crate::sim::command::REMOVE_PLAYER_OPCODE);
        assert_eq!(&record.payload()[..4], &1i32.to_le_bytes());
        run_due(&mut sim);
        assert!(sim.houses()[&peer].is_to_die());
        assert!(!sim.houses()[&ai].is_to_die());
    }

    #[test]
    fn a_wol_tournament_remove_player_skips_the_human_test() {
        // 0x004C7BBF..0x004C7BCE: GameMode 4 with [0xB779C4] set flags the
        // House without IsHumanControlled.
        let (mut sim, _, _) = two_human_sim();
        let ai = sim.intern("Ai");
        sim.houses_mut_for_test()
            .register(HouseState::new(ai, 0, None, false, 0, 10));
        sim.session_mut_for_test()
            .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Wol);
        sim.install_network_timing(10);
        sim.install_wol_start(1, Default::default());
        schedule_command_in_sim(&mut sim, "Local", Command::RemovePlayer { house_index: 2 })
            .unwrap();
        run_due(&mut sim);
        assert!(sim.houses()[&ai].is_to_die());
    }

    #[test]
    fn the_ally_record_carries_the_other_house_index_through_the_queue() {
        // Ally 0x00731877..0x007318C8: EventClass(PlayerPtr, 3, index).
        let (mut sim, local, peer) = two_human_sim();
        let record = sim.encode_ally_record(local, 1).unwrap();
        assert_eq!(record.opcode(), crate::sim::command::ALLY_OPCODE);
        assert_eq!(&record.payload()[..4], &1i32.to_le_bytes());
        schedule_command_in_sim(&mut sim, "Local", Command::Ally { house_index: 1 }).unwrap();
        let due = sim.take_frame_commands_for_test();
        assert!(matches!(
            due.as_slice(),
            [envelope] if envelope.owner == local
                && envelope.payload == Command::Ally { house_index: 1 }
        ));
        let _ = peer;
    }

    #[test]
    fn gsi_01_04_abort_waits_for_one_due_exit_dispatch_before_terminal_edge() {
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        sim.houses_mut_for_test()
            .register(HouseState::new(local, 0, None, true, 0, 10));
        sim.session_mut_for_test().set_tick_for_test(41);
        sim.session_mut_for_test().set_binary_frame_for_test(73);

        assert_eq!(
            schedule_command_in_sim(&mut sim, "Local", Command::ExitMatch),
            Some(41)
        );
        assert_eq!(sim.pending_command_snapshot().len(), 1);
        assert_eq!(
            sim.pending_command_snapshot()[0].payload,
            Command::ExitMatch
        );
        assert!(!sim.quit_requested(), "confirmation only queues EXIT");
        assert_eq!(
            sim.take_executed_exit_owner(),
            None,
            "confirmation cannot trigger app teardown"
        );

        let due = sim.take_due_commands();
        assert!(
            due.is_empty(),
            "the local EXIT remains staged until the tail"
        );
        let result = sim.advance_tick(&due, None, None, None, 33);
        assert_eq!(result.executed_commands, 1);
        assert!(
            !result.frame_committed,
            "EXIT terminates its dispatch frame"
        );
        assert!(sim.quit_requested());
        assert_eq!(
            sim.take_executed_exit_owner(),
            Some(local),
            "the app cascade receives exactly one executed edge"
        );
        assert_eq!(
            crate::app::match_runtime::scenario_exit::route_scenario_termination(
                crate::app::match_runtime::scenario_exit::TerminationRequests {
                    exit: true,
                    ..Default::default()
                }
            ),
            Some(crate::app::match_runtime::scenario_exit::TerminationRoute::Exit),
            "standalone EXIT keeps the abort route"
        );
        assert_eq!(
            sim.take_executed_exit_owner(),
            None,
            "repeated drains cannot duplicate the cascade trigger"
        );
    }

    /// B6 results 3/4 queue EVENT_DESTRUCT: a header-only type-8 record whose
    /// execution (0x004C7BDF) flags the header House to die with no human
    /// gate and starts no match outcome.
    #[test]
    fn destruct_queues_a_header_only_type_8_event_that_flags_the_house_to_die() {
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        let ai = sim.intern("Ai");
        sim.houses_mut_for_test()
            .register(HouseState::new(local, 0, None, true, 0, 10));
        sim.houses_mut_for_test()
            .register(HouseState::new(ai, 1, None, false, 0, 10));
        sim.session_mut_for_test().set_tick_for_test(41);
        sim.session_mut_for_test().set_binary_frame_for_test(73);

        let record = sim.encode_destruct_record(local).expect("record");
        assert_eq!(record.opcode(), crate::sim::command::DESTRUCT_OPCODE);
        assert_eq!(record.house_id(), 0);
        assert_eq!(record.frame_stamp(), 73);
        assert!(record.payload().iter().all(|byte| *byte == 0));

        assert_eq!(
            schedule_command_in_sim(&mut sim, "Local", Command::Destruct),
            Some(41)
        );
        // The payload itself has no Human guard. Exercise that receiver at
        // the tail below, after the House rung's pending-death processing.
        let pending = sim.pending_command_snapshot();
        assert_eq!(pending.len(), 1);
        assert!(pending.iter().all(|c| c.payload == Command::Destruct));
        assert!(!sim.houses()[&local].is_to_die());

        let due = sim.take_due_commands();
        let result = sim.advance_tick(&due, None, None, None, 33);
        assert_eq!(result.executed_commands, 1);
        assert!(sim.apply_command("Ai", &Command::Destruct, None));
        assert!(sim.houses()[&local].is_to_die());
        assert!(sim.houses()[&ai].is_to_die());
        assert!(!sim.quit_requested());
        assert_eq!(sim.houses()[&local].outcome(), None);
    }

    #[test]
    fn leave_beside_observe_runs_destruct_before_exit_in_one_frame() {
        // 48CA34: result 3 queues DESTRUCT, then Queue_Exit; both execute in
        // the same frame in queue order.
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        sim.houses_mut_for_test()
            .register(HouseState::new(local, 0, None, true, 0, 10));
        sim.session_mut_for_test().set_tick_for_test(41);
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(local));
        schedule_command_in_sim(&mut sim, "Local", Command::Destruct).unwrap();
        schedule_command_in_sim(&mut sim, "Local", Command::ExitMatch).unwrap();
        let order: Vec<_> = sim
            .pending_command_snapshot()
            .iter()
            .map(|c| c.payload.clone())
            .collect();
        assert_eq!(order, [Command::Destruct, Command::ExitMatch]);

        let due = sim.take_due_commands();
        let result = sim.advance_tick(&due, None, None, None, 33);
        assert_eq!(result.executed_commands, 2);
        assert!(sim.houses()[&local].is_to_die());
        assert_eq!(sim.take_executed_exit_owner(), Some(local));
    }

    #[test]
    fn gsi_01_04_same_frame_outcome_expiry_preempts_due_exit_after_consuming_edge() {
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        let enemy = sim.intern("Enemy");
        let mut local_house = HouseState::new(local, 0, None, true, 0, 10);
        let mut enemy_house = HouseState::new(enemy, 1, None, false, 0, 10);
        local_house
            .tracking_mut_for_test()
            .set_buildings_for_test(1);
        enemy_house
            .tracking_mut_for_test()
            .set_buildings_for_test(1);
        sim.houses_mut_for_test().register(local_house);
        sim.houses_mut_for_test().register(enemy_house);
        sim.session_mut_for_test()
            .game_options_mut_for_test()
            .short_game = false;
        sim.session_mut_for_test().set_tick_for_test(41);
        sim.session_mut_for_test().set_binary_frame_for_test(42);
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(local));
        assert!(
            sim.houses_mut_for_test()
                .get_mut(&local)
                .expect("local house")
                .accept_win_result(41, 1, false)
        );
        assert_eq!(
            schedule_command_in_sim(&mut sim, "Local", Command::ExitMatch),
            Some(41)
        );

        let due = sim.take_due_commands();
        let result = sim.advance_tick(&due, None, None, None, 33);
        let local_outcome_exit_ready = sim.houses()[&local]
            .outcome()
            .is_some_and(|outcome| outcome.exit_ready);

        assert!(!result.frame_committed);
        assert!(local_outcome_exit_ready, "House expiry happened this frame");
        assert_eq!(sim.take_executed_exit_owner(), Some(local));
        assert_eq!(
            crate::app::match_runtime::scenario_exit::route_scenario_termination(
                crate::app::match_runtime::scenario_exit::TerminationRequests {
                    outcome_ready: local_outcome_exit_ready,
                    exit: true,
                    ..Default::default()
                }
            ),
            Some(crate::app::match_runtime::scenario_exit::TerminationRoute::Outcome),
            "Main_Game's victory/loss route has priority over simultaneous EXIT"
        );
        assert_eq!(
            sim.take_executed_exit_owner(),
            None,
            "the suppressed abort edge is still consumed exactly once"
        );
    }

    #[test]
    fn all_app_command_producers_use_identical_batch_ingress_order() {
        fn fixture() -> (
            Simulation,
            crate::sim::intern::InternedId,
            crate::sim::intern::InternedId,
        ) {
            let mut sim = Simulation::new();
            let local = sim.intern("Local");
            let remote = sim.intern("Remote");
            sim.session_mut_for_test().set_tick_for_test(40);
            sim.set_input_delay_ticks(9);
            (sim, local, remote)
        }

        let (mut singles, local, remote) = fixture();
        let (mut batches, batch_local, batch_remote) = fixture();
        assert_eq!((batch_local, batch_remote), (local, remote));

        let commands = vec![
            CommandEnvelope::new(remote, 40, Command::Stop { entity_id: 11 }),
            CommandEnvelope::new(local, 42, Command::Stop { entity_id: 12 }),
            CommandEnvelope::new(local, 40, Command::Stop { entity_id: 13 }),
            CommandEnvelope::new(remote, 40, Command::Stop { entity_id: 14 }),
        ];
        let single_hash_before = singles.state_hash();
        let batch_hash_before = batches.state_hash();
        let single_rng_before = singles.rng_state();
        let batch_rng_before = batches.rng_state();

        for command in commands.iter().cloned() {
            singles.queue_command(command);
        }
        batches.queue_commands(commands[..2].iter().cloned());
        batches.queue_commands(commands[2..].iter().cloned());

        assert_eq!(
            singles.pending_command_snapshot(),
            batches.pending_command_snapshot(),
            "single and context/minimap-shaped batches append identically"
        );
        assert_eq!(batches.pending_command_snapshot(), commands.as_slice());
        assert_eq!(singles.state_hash(), single_hash_before);
        assert_eq!(batches.state_hash(), batch_hash_before);
        assert_eq!(singles.rng_state(), single_rng_before);
        assert_eq!(batches.rng_state(), batch_rng_before);

        let due_from_singles = singles.take_frame_commands_for_test();
        let due_from_batches = batches.take_frame_commands_for_test();
        let expected_due = vec![
            commands[0].clone(),
            commands[2].clone(),
            commands[3].clone(),
        ];
        assert_eq!(due_from_singles, expected_due);
        assert_eq!(due_from_batches, due_from_singles);
        assert_eq!(
            singles.pending_command_snapshot(),
            std::slice::from_ref(&commands[1])
        );
        assert_eq!(
            batches.pending_command_snapshot(),
            singles.pending_command_snapshot()
        );
    }

    #[test]
    fn gsi_16_01_local_scheduler_sends_ordinary_moves_through_their_bytes() {
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        let type_ref = sim.intern("TESTUNIT");
        sim.houses_mut_for_test()
            .register(HouseState::new(local, 0, None, false, 0, 10));
        sim.session_mut_for_test().set_tick_for_test(123);
        sim.session_mut_for_test().set_binary_frame_for_test(77);
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            42,
            1,
            1,
            0,
            0,
            local,
            Health::new(100),
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            false,
        );
        entity.lifecycle.set_in_limbo_for_test(false);
        sim.entities_mut_for_test().insert(entity);

        // A cell the Megamission record cannot hold shows the ordinary Move
        // takes its bytes: the codec refuses it and nothing is queued.
        assert_eq!(
            schedule_command_in_sim(
                &mut sim,
                "Local",
                Command::Move {
                    entity_id: 42,
                    target_rx: u16::MAX,
                    target_ry: 12,
                },
            ),
            None
        );
        assert!(sim.pending_command_snapshot().is_empty());

        assert_eq!(
            schedule_command_in_sim(
                &mut sim,
                "Local",
                Command::Move {
                    entity_id: 42,
                    target_rx: 34,
                    target_ry: 12,
                },
            ),
            Some(123)
        );
        assert_eq!(
            sim.pending_command_snapshot()[0],
            CommandEnvelope::new(
                local,
                123,
                Command::Move {
                    entity_id: 42,
                    target_rx: 34,
                    target_ry: 12,
                }
            ),
            "an ordinary Move keeps its cell through its Megamission bytes"
        );

        schedule_command_in_sim(
            &mut sim,
            "Local",
            Command::MoveToObject {
                entity_id: 42,
                target_id: 7,
            },
        )
        .unwrap();
        assert_eq!(
            sim.pending_command_snapshot()[1].payload,
            Command::MoveToObject {
                entity_id: 42,
                target_id: 7,
            },
            "a Move onto an object keeps it through its Megamission bytes"
        );
    }

    #[test]
    fn recorded_scheduler_stamps_the_current_raw_issue_ordinal() {
        let mut sim = Simulation::new();
        sim.session_mut_for_test().set_tick_for_test(41);
        sim.set_input_delay_ticks(7);

        let execute_tick =
            schedule_command_in_sim(&mut sim, "Russians", Command::DeployMcv { entity_id: 99 })
                .unwrap();

        assert_eq!(execute_tick, 41);
        assert_eq!(sim.pending_command_snapshot().len(), 1);
        assert_eq!(sim.pending_command_snapshot()[0].execute_tick, execute_tick);
        assert_eq!(
            sim.interner()
                .resolve(sim.pending_command_snapshot()[0].owner),
            "Russians"
        );
        assert_eq!(
            sim.pending_command_snapshot()[0].payload,
            Command::DeployMcv { entity_id: 99 }
        );
    }

    #[test]
    fn gsi_04_07_wall_sell_scheduler_uses_current_tick_and_signed_cell_payload() {
        let mut sim = Simulation::new();
        sim.session_mut_for_test().set_tick_for_test(23);
        let local = sim.intern("Local");
        let remote = sim.intern("Remote");
        let ai = sim.intern("AI");
        let missing_house = sim.intern("MissingHouse");
        let mut local_house = HouseState::new(local, 0, None, false, 0, 10);
        local_house.set_player_control_for_test(true);
        sim.houses_mut_for_test().register(local_house);
        sim.houses_mut_for_test()
            .register(HouseState::new(remote, 1, None, true, 0, 10));
        sim.houses_mut_for_test()
            .register(HouseState::new(ai, 2, None, false, 0, 10));
        sim.session_mut_for_test().set_binary_frame_for_test(23);

        let issued = sim
            .encode_sell_wall_at_cell_record(local, 0, 1)
            .expect("registered local house encodes");
        assert_eq!(
            issued.opcode(),
            crate::sim::command::SELL_WALL_AT_CELL_OPCODE
        );
        assert_eq!(issued.house_id(), 0);
        assert_eq!(issued.frame_stamp(), 23);
        assert_eq!(&issued.payload()[..4], &[0, 0, 1, 0]);
        assert_eq!(
            crate::net::lockstep::SynchronizedCommand::opaque(issued)
                .decode_for_simulation(&sim, 23)
                .unwrap(),
            CommandEnvelope::new(local, 23, Command::SellWallAtCell { x: 0, y: 1 })
        );

        let overlay_ini = IniFile::from_str(
            "[OverlayTypes]\n0=GAWALL\n1=ORE\n\
             [GAWALL]\nWall=yes\n\
             [ORE]\nWall=no\n",
        );
        let overlays =
            crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&overlay_ini, None);
        *sim.overlay_grid_mut_for_test() = Some(crate::sim::overlay_grid::OverlayGrid::new(4, 4));
        for cell in [(0, 1), (1, 1), (3, 3)] {
            sim.fog_mut_for_test()
                .mark_visible_for_owner(local, cell.0, cell.1);
        }

        fn attempt(
            sim: &mut Simulation,
            overlays: &crate::rules::overlay_types::OverlayTypeRegistry,
            cell: (u16, u16),
            object_under_cursor: bool,
        ) -> Option<u64> {
            let payload = sell_wall_command_for_cell(
                sim,
                overlays,
                "Local",
                cell.0,
                cell.1,
                false,
                object_under_cursor,
            )?;
            schedule_command_in_sim(sim, "Local", payload)
        }

        assert_eq!(attempt(&mut sim, &overlays, (0, 0), false), None);
        assert_eq!(attempt(&mut sim, &overlays, (3, 3), false), None);

        sim.overlay_grid_mut_for_test()
            .as_mut()
            .unwrap()
            .place_owned_wall(1, 1, 1, 0, local);
        assert_eq!(attempt(&mut sim, &overlays, (1, 1), false), None);

        sim.overlay_grid_mut_for_test()
            .as_mut()
            .unwrap()
            .place_owned_wall(2, 2, 0, 0, local);
        assert_eq!(attempt(&mut sim, &overlays, (2, 2), false), None);

        sim.overlay_grid_mut_for_test()
            .as_mut()
            .unwrap()
            .place_overlay(1, 1, 0, 0);
        assert_eq!(attempt(&mut sim, &overlays, (1, 1), false), None);
        sim.overlay_grid_mut_for_test()
            .as_mut()
            .unwrap()
            .place_owned_wall(1, 1, 0, 0, missing_house);
        assert_eq!(attempt(&mut sim, &overlays, (1, 1), false), None);
        sim.overlay_grid_mut_for_test()
            .as_mut()
            .unwrap()
            .place_owned_wall(1, 1, 0, 0, ai);
        assert_eq!(attempt(&mut sim, &overlays, (1, 1), false), None);

        sim.overlay_grid_mut_for_test()
            .as_mut()
            .unwrap()
            .place_owned_wall(0, 1, 0, 0, remote);
        assert_eq!(attempt(&mut sim, &overlays, (0, 1), false), Some(23));
        assert_eq!(sim.pending_command_snapshot().len(), 1);
        assert_eq!(sim.pending_command_snapshot()[0].execute_tick, 23);
        assert_eq!(
            sim.pending_command_snapshot()[0].payload,
            Command::SellWallAtCell { x: 0, y: 1 }
        );
        assert_eq!(sim.take_frame_commands_for_test().len(), 1);

        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.overlay_grid_mut_for_test()
            .as_mut()
            .unwrap()
            .place_owned_wall(1, 1, 0, 0, remote);
        assert_eq!(attempt(&mut sim, &overlays, (1, 1), false), None);
        sim.overlay_grid_mut_for_test()
            .as_mut()
            .unwrap()
            .place_owned_wall(1, 1, 0, 0, local);
        assert_eq!(attempt(&mut sim, &overlays, (1, 1), true), None);
        assert!(sim.pending_command_snapshot().is_empty());

        sim.overlay_grid_mut_for_test()
            .as_mut()
            .unwrap()
            .place_owned_wall(0, 1, 0, 0, local);
        assert_eq!(attempt(&mut sim, &overlays, (0, 1), false), Some(23));
        assert_eq!(sim.pending_command_snapshot().len(), 1);
        assert_eq!(sim.pending_command_snapshot()[0].execute_tick, 23);
        assert_eq!(
            sim.pending_command_snapshot()[0].payload,
            Command::SellWallAtCell { x: 0, y: 1 }
        );
    }
}

#[cfg(test)]
#[path = "local_owner_tests.rs"]
mod local_owner_tests;

#[cfg(test)]
#[path = "paid_superweapon_tests.rs"]
mod paid_superweapon_tests;

#[cfg(test)]
#[path = "force_shield_target_tests.rs"]
mod force_shield_target_tests;
