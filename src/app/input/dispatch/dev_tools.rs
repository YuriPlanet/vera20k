//! Developer diagnostics: the black-cell report, the `dev-overlays` overlay
//! toggles, debug pause and frame step, and the Ctrl+Shift dev hotkey chord.

#[cfg(feature = "dev-overlays")]
use crate::app::AppState;
#[cfg(feature = "dev-overlays")]
use crate::app::diagnostics::state::DiagnosticsState;
#[cfg(feature = "dev-overlays")]
use crate::app::match_runtime::frame_pacer::LocalFramePacer;
use crate::app::match_runtime::state::MatchState;
#[cfg(feature = "dev-overlays")]
use crate::app::types::OrderMode;
// Only the dev chord matches on bare `KeyCode`; the retail path below keeps
// fully-qualified `winit::keyboard::KeyCode`.
#[cfg(feature = "dev-overlays")]
use winit::keyboard::KeyCode;

/// Explain every terrain cell that draws as black, and say which cause is responsible.
///
/// A cell renders black for exactly two reasons, and they need completely different
/// fixes: the vision system never revealed it, or it was revealed but its tile key is
/// missing from the atlas. Both look identical on screen, so this counts them separately
/// instead of leaving the diagnosis to guesswork.
pub(crate) fn report_black_cell_causes(match_state: &MatchState) {
    let Some(grid) = match_state.match_presentation.terrain_grid.as_ref() else {
        log::info!("Black-cell report: no terrain grid loaded");
        return;
    };

    let owner =
        crate::app::input::commands::preferred_local_owner_name(match_state).and_then(|name| {
            match_state
                .sim_runtime
                .runtime()
                .map(|rt| &rt.simulation)?
                .interner()
                .get(&name)
        });
    let fog = match (
        match_state.sim_runtime.runtime().map(|rt| &rt.simulation),
        owner,
    ) {
        _ if match_state.sandbox_full_visibility => None,
        (Some(sim), Some(id)) => Some((id, sim.fog())),
        _ => None,
    };

    let mut unrevealed: u32 = 0;
    let mut missing_tile: u32 = 0;
    // Checked for every cell regardless of shroud. The fog-gated count above only sees
    // cells the player has explored, so on an unexplored map it can report zero while the
    // rest of the map is full of unresolvable tiles. This one cannot be fooled that way.
    let mut missing_tile_anywhere: u32 = 0;
    let mut missing_samples: Vec<(u16, u16, u16, u8)> = Vec::new();
    let mut unrevealed_samples: Vec<(u16, u16)> = Vec::new();

    for cell in &grid.cells {
        if let Some(atlas) = match_state.match_presentation.tile_atlas.as_ref() {
            let key = crate::map::theater::TileKey {
                tile_id: cell.tile_id,
                sub_tile: cell.sub_tile,
                variant: 0,
            };
            if atlas.get_uv(key).is_none() {
                missing_tile_anywhere += 1;
            }
        }
        if let Some((id, fog_state)) = fog
            && !fog_state.is_cell_revealed(id, cell.rx, cell.ry)
        {
            unrevealed += 1;
            if unrevealed_samples.len() < 12 {
                unrevealed_samples.push((cell.rx, cell.ry));
            }
            // An unrevealed cell is never drawn, so its tile is irrelevant.
            continue;
        }
        if let Some(atlas) = match_state.match_presentation.tile_atlas.as_ref() {
            let key = crate::map::theater::TileKey {
                tile_id: cell.tile_id,
                sub_tile: cell.sub_tile,
                variant: 0,
            };
            if atlas.get_uv(key).is_none() {
                missing_tile += 1;
                if missing_samples.len() < 12 {
                    missing_samples.push((cell.rx, cell.ry, cell.tile_id, cell.sub_tile));
                }
            }
        }
    }

    log::info!(
        "Black-cell report: {} cells total | fog {} | unrevealed={} | revealed-but-no-tile={}          | no-tile-anywhere={}",
        grid.cells.len(),
        if fog.is_some() { "ON" } else { "OFF" },
        unrevealed,
        missing_tile,
        missing_tile_anywhere,
    );
    if !unrevealed_samples.is_empty() {
        log::info!("  unrevealed sample (rx,ry): {unrevealed_samples:?}");
    }
    if !missing_samples.is_empty() {
        log::info!("  no-tile sample (rx,ry,tile_id,sub_tile): {missing_samples:?}");
    }
    if unrevealed == 0 && missing_tile == 0 {
        log::info!("  no cell is black for either reason — the black must come from elsewhere");
    }
}

/// Toggle the PathGrid / terrain-cost debug overlay.
///
/// Beyond flipping `diag.debug_show_pathgrid`, this resets the per-overlay
/// SpeedType override to None when the overlay turns off, so reopening
/// the overlay defaults back to "auto from selected unit".
#[cfg(feature = "dev-overlays")]
pub(crate) fn toggle_pathgrid_overlay(diag: &mut DiagnosticsState) {
    diag.debug_show_pathgrid = !diag.debug_show_pathgrid;
    if !diag.debug_show_pathgrid {
        diag.debug_terrain_cost_speed_type = None;
    }
    log::info!(
        "Debug terrain cost overlay: {}",
        if diag.debug_show_pathgrid {
            "ON"
        } else {
            "OFF"
        }
    );
}

/// Toggle the foot-path step overlay (Ctrl+Shift+O).
///
/// Independent of `toggle_pathgrid_overlay`: the terrain-cost wash and the
/// numbered route answer different questions, and the route is useful with the
/// cost grid off.
#[cfg(feature = "dev-overlays")]
pub(crate) fn toggle_foot_path_overlay(diag: &mut DiagnosticsState) {
    diag.debug_show_foot_path = !diag.debug_show_foot_path;
    log::info!(
        "Debug foot-path step overlay: {}",
        if diag.debug_show_foot_path {
            "ON (blue=route, magenta=next step, yellow=arrival)"
        } else {
            "OFF"
        }
    );
}

#[cfg(feature = "dev-overlays")]
pub(crate) fn toggle_cell_grid_overlay(diag: &mut DiagnosticsState) {
    diag.debug_show_cell_grid = !diag.debug_show_cell_grid;
    log::info!(
        "Debug cell grid overlay: {}",
        if diag.debug_show_cell_grid {
            "ON (blue=terrain, yellow=overlay)"
        } else {
            "OFF"
        }
    );
}

#[cfg(feature = "dev-overlays")]
pub(crate) fn toggle_heightmap_overlay(diag: &mut DiagnosticsState) {
    diag.debug_show_heightmap = !diag.debug_show_heightmap;
    log::info!(
        "Debug height map overlay: {}",
        if diag.debug_show_heightmap {
            "ON (brighter = higher elevation, blue = bridge deck)"
        } else {
            "OFF"
        }
    );
}

#[cfg(feature = "dev-overlays")]
pub(crate) fn cycle_terrain_cost_speed_type(
    diag: &mut DiagnosticsState,
    match_state: &MatchState,
    forward: bool,
) {
    if diag.debug_show_pathgrid {
        let current = crate::app::diagnostics::debug_overlays::resolve_debug_speed_type(
            diag,
            &match_state.sim_runtime,
        );
        let next = if forward {
            current.cycle_next()
        } else {
            current.cycle_prev()
        };
        diag.debug_terrain_cost_speed_type = Some(next);
        log::info!("Terrain cost overlay: {}", next.name());
    }
}

#[cfg(feature = "dev-overlays")]
pub(crate) fn request_debug_frame_step(diag: &mut DiagnosticsState, match_state: &MatchState) {
    if match_state.paused() {
        diag.debug_frame_step_requested = true;
    }
}

/// Toggle debug pause (J hotkey).
///
/// On resume, local frame admission is re-anchored so elapsed modal time
/// cannot cause a catch-up frame.
#[cfg(feature = "dev-overlays")]
pub(crate) fn toggle_debug_pause(frame_pacer: &mut LocalFramePacer, match_state: &mut MatchState) {
    match_state.debug_pause = !match_state.paused();
    if !match_state.paused() {
        frame_pacer.reset_for_immediate_frame();
    }
    log::info!(
        "Debug pause: {}",
        if match_state.paused() { "ON" } else { "OFF" }
    );
}

/// Dev/debug hotkeys require Ctrl+Shift, a chord stock binds to no command.
///
/// Gated on `dev-overlays`: every arm here is a diagnostic, a sandbox cheat or
/// a developer shortcut, and none of them is reachable from the retail UI path.
#[cfg(feature = "dev-overlays")]
pub(super) fn handle_dev_hotkey_pressed(state: &mut AppState, code: winit::keyboard::KeyCode) {
    match code {
        KeyCode::KeyM => {
            crate::app::persistence::commands::quicksave(
                &state.platform,
                &state.renderer,
                &state.frontend.screen,
                &mut state.match_state,
                &state.persistence,
                &state.audio,
                &state.diag,
                &state.process_assets,
            );
        }
        KeyCode::KeyN => {
            crate::app::persistence::commands::quickload(&state.platform, &mut state.persistence);
        }
        KeyCode::F5 => {
            // Diagnostic shortcut shares the retail browser and modal owner.
            crate::app::App::open_saved_game_browser(
                &mut state.platform,
                &state.renderer,
                &mut state.frontend,
                &mut state.match_state,
                &state.process_assets,
                &mut state.audio,
                &mut state.persistence,
                crate::ui::skirmish_shell::SavedSeedMode::Load,
            );
        }
        // Interim order-mode arms until the stock click modifiers
        // (Ctrl+Shift+click attack move, beacon key) are implemented.
        KeyCode::KeyA => {
            state.match_state.input.queued_order_mode = OrderMode::AttackMove;
            log::info!("Order mode armed: AttackMove");
        }
        KeyCode::KeyB => {
            state.match_state.input.queued_order_mode = OrderMode::Move;
        }
        KeyCode::KeyL => {
            toggle_cell_grid_overlay(&mut state.diag);
        }
        KeyCode::KeyK => {
            toggle_heightmap_overlay(&mut state.diag);
        }
        KeyCode::F9 | KeyCode::KeyP => {
            toggle_pathgrid_overlay(&mut state.diag);
        }
        KeyCode::KeyO => {
            toggle_foot_path_overlay(&mut state.diag);
        }
        KeyCode::BracketRight => {
            cycle_terrain_cost_speed_type(&mut state.diag, &state.match_state, true);
        }
        KeyCode::BracketLeft => {
            cycle_terrain_cost_speed_type(&mut state.diag, &state.match_state, false);
        }
        KeyCode::F10 | KeyCode::KeyV => {
            #[cfg(feature = "debug-tools")]
            crate::app::debug_tools::toggle_reveal(state);
            #[cfg(not(feature = "debug-tools"))]
            {
                state.match_state.sandbox_full_visibility =
                    !state.match_state.sandbox_full_visibility;
                log::info!(
                    "Fog of war: {}",
                    if state.match_state.sandbox_full_visibility {
                        "OFF (full visibility)"
                    } else {
                        "ON"
                    }
                );
            }
        }
        #[cfg(feature = "debug-tools")]
        KeyCode::KeyX => {
            crate::app::debug_tools::toggle_inspector(&mut state.match_state.sim_runtime)
        }
        KeyCode::F8 => {
            report_black_cell_causes(&state.match_state);
        }
        KeyCode::KeyJ => {
            toggle_debug_pause(&mut state.platform.frame_pacer, &mut state.match_state);
        }
        KeyCode::Period if state.match_state.paused() => {
            request_debug_frame_step(&mut state.diag, &state.match_state);
        }
        KeyCode::Equal | KeyCode::NumpadAdd | KeyCode::Minus | KeyCode::NumpadSubtract
            if crate::app::match_runtime::sim_tick::debug_speed_allowed(&state.match_state) =>
        {
            let faster = matches!(code, KeyCode::Equal | KeyCode::NumpadAdd);
            let speed = state.platform.frame_pacer.change_debug_speed(faster);
            crate::app::input::messages::post_selection_navigation_text(
                &state.platform,
                &state.renderer,
                &state.diag,
                &state.frontend,
                &mut state.match_state,
                &state.persistence,
                &format!("Debug speed {speed}x"),
            );
        }
        _ => {}
    }
}
