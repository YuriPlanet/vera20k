//! Build and production commands — queuing builds, placing buildings, owner management.
//!
//! Split from the presentation render path. Part of the app layer — may depend on everything.

use std::collections::HashMap;

use crate::app::AppState;
use crate::map::entities::EntityCategory;
use crate::net::lockstep::SynchronizedCommand;
use crate::sim::command::{Command, CommandEnvelope, QueueMode};
use crate::sim::intern::InternedId;
use crate::sim::production;

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
        .map(|sim| sim.interner.intern(s))
        .unwrap_or_default()
}

/// Resolve the local owner and return the owned String.
///
/// Read-only: identity comes from the match-scoped pin (or the sandbox
/// heuristic when unpinned). Command issue must never REWRITE identity —
/// the old per-action override write made "who am I" drift with whatever
/// the heuristic last returned, a lockstep hazard.
fn resolve_owner(state: &mut AppState) -> String {
    preferred_local_owner(state).unwrap_or_else(|| DEFAULT_OWNER.to_string())
}

pub(crate) fn queue_build_by_type(state: &mut AppState, type_id: &str) {
    let owner: String = resolve_owner(state);
    let owner_id = intern_in_sim(state, &owner);
    let type_interned = intern_in_sim(state, type_id);
    schedule_command(
        state,
        &owner,
        Command::QueueProduction {
            owner: owner_id,
            type_id: type_interned,
            mode: QueueMode::Append,
        },
    );
    log::info!(
        "Build command queued: owner={} type={} issue_frame=current",
        owner,
        type_id
    );
}

pub(crate) fn toggle_pause_build_queue(
    state: &mut AppState,
    category: production::ProductionCategory,
) {
    let owner: String = resolve_owner(state);
    let owner_id = intern_in_sim(state, &owner);
    schedule_command(
        state,
        &owner,
        Command::TogglePauseProduction {
            owner: owner_id,
            category,
        },
    );
    log::info!(
        "Build pause/resume command queued: owner={} category={} issue_frame=current",
        owner,
        category.label()
    );
}

pub(crate) fn cycle_active_producer(
    state: &mut AppState,
    category: production::ProductionCategory,
) {
    let owner: String = resolve_owner(state);
    let owner_id = intern_in_sim(state, &owner);
    schedule_command(
        state,
        &owner,
        Command::CycleProducerFocus {
            owner: owner_id,
            category,
        },
    );
    log::info!(
        "Producer focus cycle queued: owner={} category={} issue_frame=current",
        owner,
        category.label()
    );
}

pub(crate) fn cancel_last_build(state: &mut AppState) {
    let owner: String = resolve_owner(state);
    let owner_id = intern_in_sim(state, &owner);
    schedule_command(
        state,
        &owner,
        Command::CancelLastProduction { owner: owner_id },
    );
    log::info!("Build cancel command queued: owner={owner} issue_frame=current");
}

pub(crate) fn cancel_build_by_type(state: &mut AppState, type_id: &str) {
    let owner: String = resolve_owner(state);
    let owner_id = intern_in_sim(state, &owner);
    let type_interned = intern_in_sim(state, type_id);
    schedule_command(
        state,
        &owner,
        Command::CancelProductionByType {
            owner: owner_id,
            type_id: type_interned,
        },
    );
    log::info!(
        "Build cancel-by-type queued: owner={} type={} issue_frame=current",
        owner,
        type_id
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
        sim,
        world_x,
        world_y,
        &owner,
        state.match_state.sandbox_full_visibility,
        state.rules(),
        &state.height_map(),
        Some(
            &state
                .match_state
                .match_presentation
                .tactical_bridge_inverse_map,
        ),
    )
    .map(|hover| hover.stable_id)
}

fn own_building_id(state: &AppState, stable_id: u64) -> Option<u64> {
    let sim = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)?;
    let owner = preferred_local_owner(state)?;
    let entity = sim.entities().get(stable_id)?;
    (entity.category == EntityCategory::Structure
        && sim
            .interner
            .resolve(entity.owner())
            .eq_ignore_ascii_case(&owner))
    .then_some(stable_id)
}

/// Stable id of the local player's own building under the given world point, if
/// any. Shared by the repair/sell cursor feedback (`input::cursor`) and the
/// repair/sell click handler below so the two can never disagree about what is
/// an eligible building target. Only OWN structures qualify — allied buildings
/// are not repairable/sellable by the local player (the sim `ToggleRepair` /
/// `SellBuilding` handlers also enforce ownership).
pub(crate) fn own_building_under_point(
    state: &AppState,
    world_x: f32,
    world_y: f32,
) -> Option<u64> {
    own_building_id(state, visible_object_under_point(state, world_x, world_y)?)
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
    overlays: &crate::map::overlay_types::OverlayTypeRegistry,
    local_owner: &str,
    rx: u16,
    ry: u16,
    ignore_visibility: bool,
    object_under_cursor: bool,
) -> Option<Command> {
    if object_under_cursor || (rx, ry) == (0, 0) {
        return None;
    }
    let local_owner_id = sim.interner.get(local_owner)?;
    if crate::app::presentation::instances::cell_visibility_for_local_owner(
        Some(local_owner_id),
        Some(&sim.fog),
        rx,
        ry,
        ignore_visibility,
    ) != crate::app::presentation::instances::CellVisibilityState::Visible
    {
        return None;
    }
    let grid = sim.overlay_grid.as_ref()?;
    if rx >= grid.width() || ry >= grid.height() {
        return None;
    }
    let cell = grid.cell(rx, ry);
    let overlay_id = cell.overlay_id?;
    if !overlays.flags(overlay_id).is_some_and(|flags| flags.wall) {
        return None;
    }
    let wall_owner = cell.wall_owner?;
    let wall_owner_house = sim.houses.get(&wall_owner)?;
    let owner_is_human_player = if sim.session.game_mode_nonzero {
        wall_owner == local_owner_id
    } else {
        wall_owner_house.is_human || wall_owner_house.player_control
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
    if let Some(entity_id) = object_under_cursor.and_then(|id| own_building_id(state, id)) {
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

pub(crate) fn place_ready_building_at_cursor(state: &mut AppState, type_id: &str) {
    let owner: String = resolve_owner(state);
    // Use the preview's stored (rx, ry) so the placed building exactly matches
    // the ghost the player saw, avoiding any cursor-movement drift between frames.
    let (rx, ry) =
        if let Some(preview) = state.match_state.input.building_placement_preview.as_ref() {
            log::info!(
                "Click placement: using preview ({},{}) size={}x{} type={}",
                preview.rx,
                preview.ry,
                preview.width,
                preview.height,
                preview.type_id,
            );
            (preview.rx, preview.ry)
        } else {
            crate::app::match_runtime::sim_tick::screen_point_to_world_cell(
                state,
                state.match_state.input.cursor_x,
                state.match_state.input.cursor_y,
            )
        };
    if let Some(preview) = state.match_state.input.building_placement_preview.as_ref() {
        if !preview.valid {
            if let Some(reason) = &preview.reason {
                log::warn!(
                    "Ready building placement rejected locally: owner={} type={} cell=({}, {}) reason={}",
                    owner,
                    type_id,
                    rx,
                    ry,
                    reason.label()
                );
            }
            // `DisplayClass::BandBox_LeftUp 0x004ABAAC/0x004ABABA`: a
            // placement click whose pending building is not placeable
            // (`+0x1180` clear, `BuildingClass 0x00452670` refused the cell)
            // or not adjacent (`+0x1181` clear) goes to `0x004ABC76..
            // 0x004ABC80 PlayEVA("EVA_CannotDeployHere", -1)` on the clicking
            // machine, before any event is queued.
            crate::app::input::dispatch::push_local_eva(
                state,
                crate::app::input::sidebar_eva::EVA_CANNOT_DEPLOY_HERE,
            );
            return;
        }
    }
    let owner_id = intern_in_sim(state, &owner);
    let type_interned = intern_in_sim(state, type_id);
    schedule_command(
        state,
        &owner,
        Command::PlaceReadyBuilding {
            owner: owner_id,
            type_id: type_interned,
            rx,
            ry,
        },
    );
    // Clear placement mode immediately so the foundation preview stops following
    // the cursor after the order is issued.
    state.match_state.input.targeting_mode = None;
    state.match_state.input.building_placement_preview = None;
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
/// Returns early WITHOUT clearing `targeting_mode` when the cursor is over
/// the sidebar or minimap — this matters for the release of the arming
/// click itself, which lands on the cameo. Leaving the mode armed lets
/// the next real tactical-map click fire the SW. On a real tactical-map
/// click, schedules the command and clears the mode. The sim-side
/// dispatch validates `is_active && is_ready`; UI does not duplicate.
pub(crate) fn launch_super_weapon_at_cursor(state: &mut AppState, section: &str) {
    // Guard: arming click's RELEASE lands on the cameo. Don't fire the SW
    // at a bogus off-map cell behind the sidebar panel; leave the mode
    // armed so the next real map click fires.
    if crate::app::presentation::sidebar_render::is_cursor_over_minimap(state)
        || crate::app::input::cursor::current_sidebar_view_hit(state)
    {
        return;
    }

    let owner: String = resolve_owner(state);
    let sw_type_id = intern_in_sim(state, section);
    let (rx, ry) = crate::app::match_runtime::sim_tick::screen_point_to_world_cell(
        state,
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    schedule_command(
        state,
        &owner,
        Command::LaunchSuperWeapon {
            sw_type_id,
            target_rx: rx,
            target_ry: ry,
        },
    );
    state.match_state.input.targeting_mode = None;
    log::info!(
        "SuperWeapon launch queued: owner={} section={} cell=({}, {}) issue_frame=current",
        owner,
        section,
        rx,
        ry
    );
}

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
    let build_options = production::build_options_for_owner(sim, rules, &owner);
    let queueable: Vec<String> = opening
        .into_iter()
        .flatten()
        .filter(|type_id| {
            build_options.iter().any(|opt| {
                let opt_str = sim.interner.resolve(opt.type_id);
                opt_str.eq_ignore_ascii_case(type_id) && opt.enabled
            })
        })
        .collect();
    let mut queued = 0u32;
    for type_id in queueable {
        let owner_id = intern_in_sim(state, &owner);
        let type_interned = intern_in_sim(state, &type_id);
        schedule_command(
            state,
            &owner,
            Command::QueueProduction {
                owner: owner_id,
                type_id: type_interned,
                mode: QueueMode::Append,
            },
        );
        queued += 1;
    }
    if queued > 0 {
        log::info!(
            "Starter opening queued: owner={} count={} issue_frame=current",
            owner,
            queued
        );
    } else {
        log::warn!(
            "Starter opening queue failed: owner={} (no compatible first-build sequence available)",
            owner
        );
    }
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
            .map(|o| sim.interner.resolve(o.type_id).to_string())
            .collect();
        if selected.len() < 3 {
            let vehicles = options
                .iter()
                .filter(|o| {
                    o.enabled
                        && o.object_category == crate::rules::object_type::ObjectCategory::Vehicle
                })
                .take(3 - selected.len())
                .map(|o| sim.interner.resolve(o.type_id).to_string());
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
                &resources.height_map,
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
    // Debug-only control: inert whenever a match pinned the local player at
    // launch — identity must not move mid-match. Cycling remains available in
    // unpinned sandbox flows (empty-map dev sessions).
    if state.match_state.local_player_owner.is_some() {
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
    // Match-scoped pinned identity — set once at launch, never rewritten.
    // Selection must NEVER repoint the local player: under lockstep each
    // client issues commands as its fixed house, so identity cannot be a
    // per-call heuristic. Everything below is the legacy dev/sandbox
    // fallback, reachable only when no launch flow pinned an owner.
    if let Some(owner) = &state.match_state.local_player_owner {
        return Some(owner.clone());
    }
    let sim = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)?;
    // Sandbox fallback: prefer owner of selected unit first.
    for entity in sim.entities().values() {
        let owner_str = sim.interner.resolve(entity.owner());
        if entity.selected && is_playable_house_name(owner_str) {
            return Some(owner_str.to_string());
        }
    }

    // Then explicit local override set by debug actions.
    if let Some(owner) = &state.match_state.local_owner_override {
        if is_playable_house_name(owner) {
            return Some(owner.clone());
        }
    }

    // Prefer owners that currently have structures.
    let mut structure_counts: HashMap<String, usize> = HashMap::new();
    for entity in sim.entities().values() {
        let owner_str = sim.interner.resolve(entity.owner());
        if entity.category == EntityCategory::Structure && is_playable_house_name(owner_str) {
            *structure_counts.entry(owner_str.to_string()).or_insert(0) += 1;
        }
    }
    if !structure_counts.is_empty() {
        let mut ranked: Vec<(usize, String)> = structure_counts
            .into_iter()
            .filter_map(|(owner, count)| {
                let strict_buildable = state.rules().is_some_and(|rules| {
                    production::has_strict_build_option_for_owner(sim, rules, &owner)
                });
                strict_buildable.then_some((count, owner))
            })
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        if let Some((_, owner)) = ranked.first() {
            return Some(owner.clone());
        }
    }

    // Next fallback: playable houses from map config.
    let houses = collect_playable_owners(state);
    if let Some(owner) = houses.first() {
        return Some(owner.clone());
    }

    // Last fallback: any playable owner present in entity store.
    let mut owners: Vec<String> = sim
        .entities()
        .values()
        .map(|e| sim.interner.resolve(e.owner()).to_string())
        .filter(|o| is_playable_house_name(o))
        .collect();
    owners.sort();
    owners.dedup();
    owners.first().cloned()
}

pub(crate) fn collect_playable_owners(state: &AppState) -> Vec<String> {
    let mut owners: Vec<String> = state
        .match_state
        .match_presentation
        .house_roster
        .houses
        .iter()
        .filter(|house| is_playable_house_name(&house.name))
        .filter(|house| house.player_control != Some(false))
        .map(|house| house.name.clone())
        .collect();
    if let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    {
        for entity in sim.entities().values() {
            let owner_str = sim.interner.resolve(entity.owner());
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

fn schedule_command_in_sim(
    sim: &mut crate::sim::world::Simulation,
    owner: &str,
    payload: Command,
) -> Option<u64> {
    let execute_tick = sim.session.tick;
    let owner_id = match &payload {
        Command::SetGameSpeed { speed } => {
            if *speed > crate::sim::game_options::IN_GAME_OPTIONS_MAX_SPEED {
                return None;
            }
            let owner_id = sim.interner.get(owner)?;
            if !sim.houses.contains_key(&owner_id) {
                return None;
            }
            let requested_speed = sim
                .projected_in_game_options_speed()
                .map(i32::from)
                .unwrap_or(sim.session.game_options.game_speed);
            if requested_speed == i32::from(*speed) {
                return Some(execute_tick);
            }
            owner_id
        }
        _ => sim.interner.intern(owner),
    };
    let envelope = match payload {
        Command::ExitMatch => {
            let record = sim.encode_exit_record(owner_id)?;
            SynchronizedCommand::opaque(record).decode_for_simulation(sim, execute_tick)?
        }
        Command::SellWallAtCell { x, y } => {
            let record = sim.encode_sell_wall_at_cell_record(owner_id, x, y)?;
            SynchronizedCommand::opaque(record).decode_for_simulation(sim, execute_tick)?
        }
        payload => CommandEnvelope::new(owner_id, execute_tick, payload),
    };
    let envelope = roundtrip_ordinary_local_move(sim, envelope)?;
    sim.queue_command(envelope);
    Some(execute_tick)
}

/// Cell-click producer only. The native4DE1D0 resolver precedes event
/// encoding; synchronized/replay/internal Move payloads are already resolved.
pub(crate) fn ordinary_cell_move_goal(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    viewer: InternedId,
    entity_id: u64,
    clicked: (u16, u16),
    native_ground_receiver: bool,
) -> Option<(u16, u16)> {
    if native_ground_receiver
        && let Some(result) = sim.ordinary_ground_foot_cell_input(viewer, entity_id, clicked, rules)
    {
        return match result {
            Ok(cell) => cell,
            Err(cause) => {
                log::warn!("ordinary Foot cell input: {cause}");
                None
            }
        };
    }
    //Existing compatibility adapter for other locomotors (Hover, Teleport,
    //Jumpjet, Fly), high movers and attack-move/queued input. Their native
    //caller contracts remain separate.
    let mut goal = clicked;
    if let Some(grid) = sim.path_grid()
        && !crate::app::match_runtime::sim_tick::is_any_layer_walkable(grid, goal.0, goal.1)
        && let Some(nearest) =
            crate::app::match_runtime::sim_tick::nearest_walkable_cell_layered(grid, goal, 12)
    {
        goal = nearest;
    }
    Some(goal)
}

/// Make the verified ordinary local Move bytes authoritative at issue time.
///
/// Active YR routes a cell click through `ClickedAction` (`0x004D7D50`) to
/// `EventClass__BuildMegaMissionEnvelope` (`0x004C6860`). Only queue-false Move
/// is in that verified codec contract: queued/planning and every other semantic
/// command pass through unchanged until their own native record is established.
pub(crate) fn roundtrip_ordinary_local_move(
    sim: &crate::sim::world::Simulation,
    envelope: CommandEnvelope,
) -> Option<CommandEnvelope> {
    let CommandEnvelope {
        owner,
        execute_tick,
        payload,
    } = envelope;
    match payload {
        Command::Move {
            entity_id,
            target_rx,
            target_ry,
            queue: false,
            ..
        } => {
            let record =
                sim.encode_megamission_move_record(owner, entity_id, target_rx, target_ry)?;
            SynchronizedCommand::opaque(record).decode_for_simulation(sim, execute_tick)
        }
        payload => Some(CommandEnvelope::new(owner, execute_tick, payload)),
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

pub(crate) fn is_playable_house_name(name: &str) -> bool {
    let up = name.to_ascii_uppercase();
    !matches!(
        up.as_str(),
        "NEUTRAL" | "SPECIAL" | "CIVILIAN" | "GOODGUY" | "BADGUY" | "JP"
    )
}

#[cfg(test)]
mod tests {
    use super::{schedule_command_in_sim, sell_wall_command_for_cell};
    use std::collections::BTreeMap;

    use crate::map::entities::EntityCategory;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::house_state::HouseState;
    use crate::sim::world::Simulation;

    #[test]
    fn options_game_speed_queues_once_without_immediate_sim_mutation() {
        let mut sim = Simulation::new();
        let local = sim.interner.intern("Local");
        sim.houses
            .insert(local, HouseState::new(local, 0, None, true, 0, 10));
        sim.session.house_order.push(local);
        let before_hash = sim.state_hash();

        assert_eq!(
            schedule_command_in_sim(&mut sim, "Local", Command::SetGameSpeed { speed: 4 }),
            Some(0)
        );
        assert_eq!(sim.session.game_options.game_speed, 1);
        assert_eq!(sim.state_hash(), before_hash);
        assert_eq!(sim.pending_commands_for_tests().len(), 1);

        assert_eq!(
            schedule_command_in_sim(&mut sim, "Local", Command::SetGameSpeed { speed: 4 }),
            Some(0),
            "reopening Options before admission accepts the existing request"
        );
        assert_eq!(
            sim.pending_commands_for_tests().len(),
            1,
            "duplicate request is elided"
        );
        assert_eq!(
            schedule_command_in_sim(&mut sim, "Local", Command::SetGameSpeed { speed: 7 }),
            None,
            "the in-game trackbar cannot emit stored speed 7"
        );

        let interned_before_unknown = sim.interner.len();
        assert_eq!(
            schedule_command_in_sim(&mut sim, "Unknown", Command::SetGameSpeed { speed: 3 }),
            None
        );
        assert_eq!(sim.interner.len(), interned_before_unknown);
    }

    #[test]
    fn gsi_01_04_abort_waits_for_one_due_exit_dispatch_before_terminal_edge() {
        let mut sim = Simulation::new();
        let local = sim.interner.intern("Local");
        sim.houses
            .insert(local, HouseState::new(local, 0, None, true, 0, 10));
        sim.session.house_order = vec![local];
        sim.session.tick = 41;
        sim.session.binary_frame = 73;

        assert_eq!(
            schedule_command_in_sim(&mut sim, "Local", Command::ExitMatch),
            Some(41)
        );
        assert_eq!(sim.pending_commands_for_tests().len(), 1);
        assert_eq!(
            sim.pending_commands_for_tests()[0].payload,
            Command::ExitMatch
        );
        assert!(!sim.quit_requested, "confirmation only queues EXIT");
        assert_eq!(
            sim.take_executed_exit_owner(),
            None,
            "confirmation cannot trigger app teardown"
        );

        let due = sim.take_due_commands();
        assert_eq!(due.len(), 1);
        let result = sim.advance_tick(&due, None, &BTreeMap::new(), None, None, 33);
        assert_eq!(result.executed_commands, 1);
        assert!(
            !result.frame_committed,
            "EXIT terminates its dispatch frame"
        );
        assert!(sim.quit_requested);
        assert_eq!(
            sim.take_executed_exit_owner(),
            Some(local),
            "the app cascade receives exactly one executed edge"
        );
        assert_eq!(
            crate::app::match_runtime::scenario_exit::arbitrate_executed_exit(false),
            crate::app::match_runtime::scenario_exit::ExecutedExitDisposition::Abort,
            "standalone EXIT keeps the abort route"
        );
        assert_eq!(
            sim.take_executed_exit_owner(),
            None,
            "repeated drains cannot duplicate the cascade trigger"
        );
    }

    #[test]
    fn gsi_01_04_same_frame_outcome_expiry_preempts_due_exit_after_consuming_edge() {
        let mut sim = Simulation::new();
        let local = sim.interner.intern("Local");
        let enemy = sim.interner.intern("Enemy");
        let mut local_house = HouseState::new(local, 0, None, true, 0, 10);
        let mut enemy_house = HouseState::new(enemy, 1, None, false, 0, 10);
        local_house.owned_building_count = 1;
        enemy_house.owned_building_count = 1;
        sim.houses.insert(local, local_house);
        sim.houses.insert(enemy, enemy_house);
        sim.session.house_order = vec![local, enemy];
        sim.session.game_options.short_game = false;
        sim.session.tick = 41;
        assert!(
            sim.houses
                .get_mut(&local)
                .expect("local house")
                .flag_to_win(41, 1)
        );
        assert_eq!(
            schedule_command_in_sim(&mut sim, "Local", Command::ExitMatch),
            Some(41)
        );

        let due = sim.take_due_commands();
        let result = sim.advance_tick(&due, None, &BTreeMap::new(), None, None, 33);
        let local_outcome_exit_ready = sim.houses[&local]
            .outcome_state
            .is_some_and(|outcome| outcome.exit_ready);

        assert!(!result.frame_committed);
        assert!(local_outcome_exit_ready, "House expiry happened this frame");
        assert_eq!(sim.take_executed_exit_owner(), Some(local));
        assert_eq!(
            crate::app::match_runtime::scenario_exit::arbitrate_executed_exit(
                local_outcome_exit_ready
            ),
            crate::app::match_runtime::scenario_exit::ExecutedExitDisposition::Outcome,
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
            let local = sim.interner.intern("Local");
            let remote = sim.interner.intern("Remote");
            sim.session.tick = 40;
            sim.input_delay_ticks = 9;
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
            singles.pending_commands_for_tests(),
            batches.pending_commands_for_tests(),
            "single and context/minimap-shaped batches append identically"
        );
        assert_eq!(batches.pending_commands_for_tests(), commands.as_slice());
        assert_eq!(singles.state_hash(), single_hash_before);
        assert_eq!(batches.state_hash(), batch_hash_before);
        assert_eq!(singles.rng_state(), single_rng_before);
        assert_eq!(batches.rng_state(), batch_rng_before);

        let due_from_singles = singles.take_due_commands();
        let due_from_batches = batches.take_due_commands();
        let expected_due = vec![
            commands[0].clone(),
            commands[2].clone(),
            commands[3].clone(),
        ];
        assert_eq!(due_from_singles, expected_due);
        assert_eq!(due_from_batches, due_from_singles);
        assert_eq!(
            singles.pending_commands_for_tests(),
            std::slice::from_ref(&commands[1])
        );
        assert_eq!(
            batches.pending_commands_for_tests(),
            singles.pending_commands_for_tests()
        );
    }

    #[test]
    fn gsi_16_01_local_scheduler_uses_move_bytes_and_fences_queued_move_metadata() {
        let mut sim = Simulation::new();
        let local = sim.interner.intern("Local");
        let type_ref = sim.interner.intern("TESTUNIT");
        sim.houses
            .insert(local, HouseState::new(local, 0, None, false, 0, 10));
        sim.session.house_order = vec![local];
        sim.session.tick = 123;
        sim.session.binary_frame = 77;
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            42,
            1,
            1,
            0,
            0,
            local,
            Health { current: 100 },
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            false,
        );
        entity.lifecycle.in_limbo = false;
        sim.substrate.entities.insert(entity);

        assert_eq!(
            schedule_command_in_sim(
                &mut sim,
                "Local",
                Command::Move {
                    entity_id: 42,
                    target_rx: 34,
                    target_ry: 12,
                    queue: false,
                    group_id: Some(99),
                },
            ),
            Some(123)
        );
        assert_eq!(
            sim.pending_commands_for_tests()[0],
            CommandEnvelope::new(
                local,
                123,
                Command::Move {
                    entity_id: 42,
                    target_rx: 34,
                    target_ry: 12,
                    queue: false,
                    group_id: None,
                }
            ),
            "ordinary Move has no semantic sidecar for Rust-only group metadata"
        );

        schedule_command_in_sim(
            &mut sim,
            "Local",
            Command::Move {
                entity_id: 42,
                target_rx: 35,
                target_ry: 12,
                queue: true,
                group_id: Some(99),
            },
        )
        .unwrap();
        assert!(matches!(
            sim.pending_commands_for_tests()[1].payload,
            Command::Move {
                queue: true,
                group_id: Some(99),
                ..
            }
        ));
    }

    #[test]
    fn recorded_scheduler_stamps_the_current_raw_issue_ordinal() {
        let mut sim = Simulation::new();
        sim.session.tick = 41;
        sim.input_delay_ticks = 7;

        let execute_tick =
            schedule_command_in_sim(&mut sim, "Russians", Command::DeployMcv { entity_id: 99 })
                .unwrap();

        assert_eq!(execute_tick, 41);
        assert_eq!(sim.pending_commands_for_tests().len(), 1);
        assert_eq!(
            sim.pending_commands_for_tests()[0].execute_tick,
            execute_tick
        );
        assert_eq!(
            sim.interner
                .resolve(sim.pending_commands_for_tests()[0].owner),
            "Russians"
        );
        assert_eq!(
            sim.pending_commands_for_tests()[0].payload,
            Command::DeployMcv { entity_id: 99 }
        );
    }

    #[test]
    fn recorded_scheduler_does_not_apply_offline_input_delay() {
        let mut sim = Simulation::new();
        sim.session.tick = u64::MAX - 1;
        sim.input_delay_ticks = 8;

        let execute_tick =
            schedule_command_in_sim(&mut sim, "YuriCountry", Command::DeployMcv { entity_id: 7 })
                .unwrap();

        assert_eq!(execute_tick, u64::MAX - 1);
        assert_eq!(
            sim.pending_commands_for_tests()[0].execute_tick,
            u64::MAX - 1
        );
    }

    #[test]
    fn gsi_04_07_wall_sell_scheduler_uses_current_tick_and_signed_cell_payload() {
        let mut sim = Simulation::new();
        sim.session.tick = 23;
        let local = sim.interner.intern("Local");
        let remote = sim.interner.intern("Remote");
        let ai = sim.interner.intern("AI");
        let missing_house = sim.interner.intern("MissingHouse");
        let mut local_house = HouseState::new(local, 0, None, false, 0, 10);
        local_house.player_control = true;
        sim.houses.insert(local, local_house);
        sim.houses
            .insert(remote, HouseState::new(remote, 1, None, true, 0, 10));
        sim.houses
            .insert(ai, HouseState::new(ai, 2, None, false, 0, 10));
        sim.session.house_order = vec![local, remote, ai];
        sim.session.binary_frame = 23;

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
        let overlays = crate::map::overlay_types::OverlayTypeRegistry::from_ini(&overlay_ini, None);
        sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(4, 4));
        for cell in [(0, 1), (1, 1), (3, 3)] {
            sim.fog.mark_visible_for_owner(local, cell.0, cell.1);
        }

        fn attempt(
            sim: &mut Simulation,
            overlays: &crate::map::overlay_types::OverlayTypeRegistry,
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

        sim.overlay_grid
            .as_mut()
            .unwrap()
            .place_owned_wall(1, 1, 1, 0, local);
        assert_eq!(attempt(&mut sim, &overlays, (1, 1), false), None);

        sim.overlay_grid
            .as_mut()
            .unwrap()
            .place_owned_wall(2, 2, 0, 0, local);
        assert_eq!(attempt(&mut sim, &overlays, (2, 2), false), None);

        sim.overlay_grid.as_mut().unwrap().place_overlay(1, 1, 0, 0);
        assert_eq!(attempt(&mut sim, &overlays, (1, 1), false), None);
        sim.overlay_grid
            .as_mut()
            .unwrap()
            .place_owned_wall(1, 1, 0, 0, missing_house);
        assert_eq!(attempt(&mut sim, &overlays, (1, 1), false), None);
        sim.overlay_grid
            .as_mut()
            .unwrap()
            .place_owned_wall(1, 1, 0, 0, ai);
        assert_eq!(attempt(&mut sim, &overlays, (1, 1), false), None);

        sim.overlay_grid
            .as_mut()
            .unwrap()
            .place_owned_wall(0, 1, 0, 0, remote);
        assert_eq!(attempt(&mut sim, &overlays, (0, 1), false), Some(23));
        assert_eq!(sim.pending_commands_for_tests().len(), 1);
        assert_eq!(sim.pending_commands_for_tests()[0].execute_tick, 23);
        assert_eq!(
            sim.pending_commands_for_tests()[0].payload,
            Command::SellWallAtCell { x: 0, y: 1 }
        );
        assert_eq!(sim.take_due_commands().len(), 1);

        sim.session.game_mode_nonzero = true;
        sim.overlay_grid
            .as_mut()
            .unwrap()
            .place_owned_wall(1, 1, 0, 0, remote);
        assert_eq!(attempt(&mut sim, &overlays, (1, 1), false), None);
        sim.overlay_grid
            .as_mut()
            .unwrap()
            .place_owned_wall(1, 1, 0, 0, local);
        assert_eq!(attempt(&mut sim, &overlays, (1, 1), true), None);
        assert!(sim.pending_commands_for_tests().is_empty());

        sim.overlay_grid
            .as_mut()
            .unwrap()
            .place_owned_wall(0, 1, 0, 0, local);
        assert_eq!(attempt(&mut sim, &overlays, (0, 1), false), Some(23));
        assert_eq!(sim.pending_commands_for_tests().len(), 1);
        assert_eq!(sim.pending_commands_for_tests()[0].execute_tick, 23);
        assert_eq!(
            sim.pending_commands_for_tests()[0].payload,
            Command::SellWallAtCell { x: 0, y: 1 }
        );
    }
}
